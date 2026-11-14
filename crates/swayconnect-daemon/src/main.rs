//! `swayconnectd` (`docs/DESIGN.md` 7.1). The only daemon file that reads the environment.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::{IsTerminal as _, Write as _};
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use swayconnect_daemon::admin::{AdminClient, AdminError, PairingRequest, PairingResult};
use swayconnect_daemon::apps::index::IndexDirs;
use swayconnect_daemon::audit;
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::services::{KeyboardHost, MediaHost};
use swayconnect_daemon::daemon::{Daemon, Host, IconSources};
use swayconnect_daemon::devices::{DeviceInfo, ScopeChange};
use swayconnect_daemon::icons::{self, raster};
use swayconnect_daemon::input::WaylandTarget;
use swayconnect_daemon::media::session_bus_address;
use swayconnect_daemon::paths::{Paths, own_uid};
use swayconnect_daemon::proc::{ChildEnv, RealRunner};
use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{DeviceId, Scope};
use swayconnect_sway::{EnvSnapshot, SwayTarget};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

const APP_DIR: &str = "swayconnect";
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

#[derive(Parser)]
#[command(
    name = "swayconnectd",
    version,
    about = "Remote control of a sway desktop"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon in the foreground (the default)
    Run,
    /// Open a pairing window and confirm the device that answers
    Pair {
        /// Scopes of the new device, comma separated
        #[arg(
            long,
            value_delimiter = ',',
            value_parser = parse_scope,
            default_value = "state.read,state.titles,control,audio,media,idle.inhibit,launch,input.keys,input.text"
        )]
        scopes: Vec<Scope>,
        /// Lifetime of the pairing window in seconds
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(30..=600))]
        ttl: u64,
        /// Host name or address to put into the pairing URI
        #[arg(long)]
        host: Option<String>,
    },
    /// List the paired devices, or change the scopes of one
    Devices {
        #[command(subcommand)]
        change: Option<DevicesChange>,
    },
    /// Delete a device and drop its sessions
    Revoke { device_id: String },
    /// Print the newest lines of the audit log
    Audit {
        /// How many lines
        #[arg(short = 'n', default_value_t = 20)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum DevicesChange {
    /// Give scopes to a device; its open connections end with `scopes_changed`
    Grant(ScopeArgs),
    /// Take scopes from a device; its open connections end with `scopes_changed`
    Deny(ScopeArgs),
}

#[derive(clap::Args)]
struct ScopeArgs {
    /// The first characters of the device id, at least 4 (see `swayconnectd devices`)
    prefix: String,
    /// Scope names, for example `state.titles`
    #[arg(required = true, value_parser = parse_scope)]
    scopes: Vec<Scope>,
}

/// An error with the exit code of `docs/DESIGN.md` 4.7.
struct Failure {
    exit_code: u8,
    error: anyhow::Error,
}

impl Failure {
    fn usage(error: impl Into<anyhow::Error>) -> Failure {
        Failure {
            exit_code: EXIT_USAGE,
            error: error.into(),
        }
    }

    fn runtime(error: impl Into<anyhow::Error>) -> Failure {
        Failure {
            exit_code: EXIT_FAILURE,
            error: error.into(),
        }
    }
}

impl From<AdminError> for Failure {
    fn from(error: AdminError) -> Failure {
        match error {
            AdminError::Path(_) => Failure::usage(error),
            _ => Failure::runtime(error),
        }
    }
}

fn parse_scope(name: &str) -> Result<Scope, String> {
    Scope::from_wire(name).ok_or_else(|| format!("unknown scope `{name}`"))
}

fn main() -> ExitCode {
    // The icon raster child (docs/DESIGN.md 11.5) limits its own memory, so it must run
    // before the async runtime starts any thread.
    let mut arguments = std::env::args_os().skip(1);
    if let (Some(first), None) = (arguments.next(), arguments.next())
        && first == raster::RASTER_ARG
    {
        return raster::child_main();
    }
    run_cli()
}

#[tokio::main]
async fn run_cli() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match cli.command.unwrap_or(Command::Run) {
        Command::Run => run().await,
        Command::Pair { scopes, ttl, host } => {
            let request = PairingRequest {
                scopes: scopes.into_iter().collect::<BTreeSet<Scope>>(),
                lifetime: Duration::from_secs(ttl),
                host,
            };
            pair(&request).await
        }
        Command::Devices { change: None } => devices().await,
        Command::Devices {
            change: Some(DevicesChange::Grant(args)),
        } => change_scopes(ScopeChange::Grant, args).await,
        Command::Devices {
            change: Some(DevicesChange::Deny(args)),
        } => change_scopes(ScopeChange::Deny, args).await,
        Command::Revoke { device_id } => revoke(&device_id).await,
        Command::Audit { lines } => print_audit(lines),
    };
    match outcome {
        Ok(exit_code) => exit_code,
        Err(failure) => {
            // Not `{:#}`: every error here names its cause in its own text, and the chain
            // would print that cause a second time.
            eprintln!("swayconnectd: {}", failure.error);
            ExitCode::from(failure.exit_code)
        }
    }
}

/// An absolute path from the environment. Unset, empty and relative count as not given,
/// as the XDG base directory rules say.
fn absolute_path_from(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn xdg_dir(variable: &str, home_fallback: &str) -> Result<PathBuf, Failure> {
    absolute_path_from(variable)
        .or_else(|| absolute_path_from("HOME").map(|home| home.join(home_fallback)))
        .ok_or_else(|| Failure::usage(anyhow!("neither {variable} nor HOME is an absolute path")))
}

/// `SWAYCONNECT_CONFIG` when set: it must be absolute and the file must exist.
fn config_override() -> Result<Option<PathBuf>, Failure> {
    let Some(file) = std::env::var_os("SWAYCONNECT_CONFIG").map(PathBuf::from) else {
        return Ok(None);
    };
    if !file.is_absolute() {
        return Err(Failure::usage(anyhow!(
            "SWAYCONNECT_CONFIG must be an absolute path"
        )));
    }
    // A missing default file means defaults; a missing named file is a typo.
    if !file.exists() {
        let file = file.display();
        return Err(Failure::usage(anyhow!(
            "SWAYCONNECT_CONFIG: no file at {file}"
        )));
    }
    Ok(Some(file))
}

fn paths_from_environment() -> Result<Paths, Failure> {
    let config_file = match config_override()? {
        Some(file) => file,
        None => xdg_dir("XDG_CONFIG_HOME", ".config")?
            .join(APP_DIR)
            .join("config.toml"),
    };
    Ok(Paths {
        config_file,
        data_dir: xdg_dir("XDG_DATA_HOME", ".local/share")?.join(APP_DIR),
        // Empty means unset: the admin socket code refuses it, there is no fallback.
        runtime_dir: absolute_path_from("XDG_RUNTIME_DIR").unwrap_or_default(),
    })
}

/// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`: where icons and desktop entries are found.
fn data_dirs() -> Result<Vec<PathBuf>, Failure> {
    let data_home = xdg_dir("XDG_DATA_HOME", ".local/share")?;
    Ok(icons::search_dirs(
        &data_home,
        std::env::var_os("XDG_DATA_DIRS").as_deref(),
    ))
}

/// The icon service reads `search_dirs`, caches below `$XDG_CACHE_HOME`, and runs this
/// program as its raster child (`docs/DESIGN.md` 11.5).
fn icon_sources(search_dirs: Vec<PathBuf>) -> Result<IconSources, Failure> {
    let raster_program = std::env::current_exe().map_err(|error| {
        Failure::runtime(anyhow!(
            "cannot name the own program for the icon raster child: {error}"
        ))
    })?;
    Ok(IconSources {
        search_dirs,
        cache_dir: xdg_dir("XDG_CACHE_HOME", ".cache")?.join(APP_DIR),
        raster_program,
    })
}

fn sway_target(config: &Config, paths: &Paths) -> Result<SwayTarget, Failure> {
    if let Some(socket) = &config.sway_socket {
        return Ok(SwayTarget::Explicit(socket.clone()));
    }
    let runtime_dir = &paths.runtime_dir;
    Ok(SwayTarget::Discover(EnvSnapshot {
        swaysock: absolute_path_from("SWAYSOCK"),
        xdg_runtime_dir: Some(runtime_dir.clone()).filter(|dir| !dir.as_os_str().is_empty()),
        uid: own_uid().map_err(Failure::runtime)?,
    }))
}

async fn run() -> Result<ExitCode, Failure> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let paths = paths_from_environment()?;
    let config = Config::load(&paths.config_file).map_err(Failure::usage)?;
    let runtime_dir = Some(paths.runtime_dir.clone()).filter(|dir| !dir.as_os_str().is_empty());
    let display = std::env::var_os("WAYLAND_DISPLAY");
    let bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let data_dirs = data_dirs()?;
    let host = Host {
        icons: Some(icon_sources(data_dirs.clone())?),
        // Children get the desktop session's variables, filtered by the allowlist.
        runner: Arc::new(RealRunner::new(ChildEnv::allowlisted(std::env::vars_os()))),
        keyboard: KeyboardHost::Wayland(WaylandTarget::new(
            &config.wayland,
            runtime_dir.clone(),
            display,
        )),
        media: MediaHost::Bus(session_bus_address(bus.as_deref(), runtime_dir.as_deref())),
        apps: IndexDirs {
            data_dirs,
            exec_path: std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .collect(),
        },
        ..Host::new(sway_target(&config, &paths)?)
    };
    // Registered before the start is logged, so a signal sent right after it is not lost.
    let mut interrupt = signal(SignalKind::interrupt()).map_err(Failure::runtime)?;
    let mut terminate = signal(SignalKind::terminate()).map_err(Failure::runtime)?;
    let mut panic_switch = signal(SignalKind::user_defined1()).map_err(Failure::runtime)?;

    let daemon = Daemon::start_on(config, paths, host)
        .await
        .map_err(|error| Failure {
            exit_code: error.exit_code(),
            error: error.into(),
        })?;
    loop {
        tokio::select! {
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
            _ = panic_switch.recv() => daemon.enter_read_only(),
        }
    }
    daemon.stop().await;
    Ok(ExitCode::SUCCESS)
}

/// The newest line from the terminal, with the instant at which it was read.
type TypedLine = Option<(Instant, String)>;

/// Reads the terminal from now on. A thread, so that a pairing that ends without an answer
/// does not keep the process alive in a blocked read. The channel closes when the terminal
/// does, or on a read error.
fn terminal_lines() -> watch::Receiver<TypedLine> {
    let (newest, lines) = watch::channel(None);
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            newest.send_replace(Some((Instant::now(), line)));
        }
    });
    lines
}

/// The owner's answer to the device shown at `shown_at`. A line typed before that instant
/// answers no device: it waited in the terminal, and whoever presents the pairing secret
/// first would get it. `None`: the terminal closed without an answer.
async fn answer_typed_after(
    lines: &mut watch::Receiver<TypedLine>,
    shown_at: Instant,
) -> Option<String> {
    let is_answer = |line: &TypedLine| matches!(line, Some((typed_at, _)) if *typed_at >= shown_at);
    let line = lines.wait_for(is_answer).await.ok()?;
    line.as_ref().map(|(_, text)| text.clone())
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn is_loopback_host(host: &str) -> bool {
    host.parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// Shows the pairing secret to the owner. It goes to the terminal only, never to a log.
fn print_pairing_uri(uri: &str) -> Result<(), Failure> {
    let code = QrCode::new(uri.as_bytes()).map_err(|error| {
        Failure::runtime(anyhow!(
            "the pairing URI does not fit into a QR code: {error}"
        ))
    })?;
    println!("{}", code.render::<Dense1x2>().build());
    println!("{uri}");
    let parsed = PairingUri::parse(uri)
        .map_err(|_| Failure::runtime(anyhow!("the daemon sent an invalid pairing URI")))?;
    if !is_loopback_host(&parsed.host) {
        let port = parsed.port;
        println!("hint: a host firewall must allow TCP port {port} from the device's network");
    }
    Ok(())
}

/// The last line is `paired: ...`, `refused` or `window expired` (`docs/DESIGN.md` 7.1).
async fn pair(request: &PairingRequest) -> Result<ExitCode, Failure> {
    let mut admin = AdminClient::connect(&paths_from_environment()?).await?;
    // Before the window opens, so that every line typed too early is seen as such.
    let mut typed = terminal_lines();
    let uri = admin.open_pairing(request).await?;
    print_pairing_uri(&uri)?;
    println!("waiting for a device ({} s)", request.lifetime.as_secs());
    let Some(device) = admin.pending_device().await? else {
        println!("window expired");
        return Ok(ExitCode::from(EXIT_FAILURE));
    };
    let shown_at = Instant::now();
    let platform = device.platform.as_deref().unwrap_or("unknown platform");
    println!(
        "pending: {} ({}) from {}",
        escape_for_terminal(&device.device_name),
        escape_for_terminal(platform),
        device.peer
    );
    println!("code: {}", escape_for_terminal(&device.confirm_code));
    print!("Confirm this device? [y/N] ");
    std::io::stdout().flush().map_err(Failure::runtime)?;

    let result = tokio::select! {
        result = admin.pairing_result() => result?,
        answer = answer_typed_after(&mut typed, shown_at) => {
            // No line (closed terminal, read error) is a no.
            let is_confirmed = answer.is_some_and(|answer| is_yes(&answer));
            admin.answer(is_confirmed).await?;
            admin.pairing_result().await?
        }
    };
    match result {
        PairingResult::Paired(paired) => {
            println!(
                "paired: {} id {} scopes {}",
                escape_for_terminal(&paired.device_name),
                escape_for_terminal(&paired.device_id),
                escape_for_terminal(&paired.scopes.join(","))
            );
            return Ok(ExitCode::SUCCESS);
        }
        PairingResult::Refused => println!("refused"),
        PairingResult::DeadlinePassed => println!("refused: no answer within the deadline"),
        PairingResult::ClientGone => println!("refused: the device closed its connection"),
    }
    Ok(ExitCode::from(EXIT_FAILURE))
}

fn device_line(device: &DeviceInfo) -> String {
    let text = |value: Option<&String>| escape_for_terminal(value.map_or("-", String::as_str));
    // The client of the last `auth` is newer than the one stored at pairing.
    let client = device.last_client.as_ref().or(device.client.as_ref());
    let last_seen = device
        .last_seen
        .map_or_else(|| "never".to_owned(), |seconds| seconds.to_string());
    format!(
        "{}  {}  platform {}  client {}  scopes {}  created {}  last seen {}  connections {}",
        escape_for_terminal(&device.id),
        escape_for_terminal(&device.name),
        text(device.platform.as_ref()),
        text(client),
        escape_for_terminal(&device.scopes.join(",")),
        device.created_at,
        last_seen,
        device.connections
    )
}

async fn devices() -> Result<ExitCode, Failure> {
    let mut admin = AdminClient::connect(&paths_from_environment()?).await?;
    let devices = admin.devices().await?;
    if devices.is_empty() {
        println!("no paired devices");
    }
    for device in &devices {
        println!("{}", device_line(device));
    }
    Ok(ExitCode::SUCCESS)
}

async fn change_scopes(change: ScopeChange, args: ScopeArgs) -> Result<ExitCode, Failure> {
    let scopes: BTreeSet<Scope> = args.scopes.into_iter().collect();
    let mut admin = AdminClient::connect(&paths_from_environment()?).await?;
    let changed = admin.change_scopes(&args.prefix, change, &scopes).await?;
    let outcome = if changed.changed {
        "scopes changed, open connections ended"
    } else {
        "no change"
    };
    println!(
        "{}: {outcome}; scopes {}",
        escape_for_terminal(&changed.device_id),
        escape_for_terminal(&changed.scopes.join(","))
    );
    Ok(ExitCode::SUCCESS)
}

async fn revoke(device_id: &str) -> Result<ExitCode, Failure> {
    let device = DeviceId::parse(device_id)
        .map_err(|_| Failure::usage(anyhow!("not a device id (see `swayconnectd devices`)")))?;
    let mut admin = AdminClient::connect(&paths_from_environment()?).await?;
    if admin.revoke(&device).await? {
        println!("revoked: {}", device.as_str());
        Ok(ExitCode::SUCCESS)
    } else {
        Err(Failure::runtime(anyhow!("no such device")))
    }
}

/// Reads the file itself: the log is useful while the daemon is stopped, too.
fn print_audit(count: usize) -> Result<ExitCode, Failure> {
    let file = audit::audit_file(&paths_from_environment()?);
    let uid = own_uid().map_err(Failure::runtime)?;
    let lines = audit::newest_lines_for_terminal(&file, uid, count).map_err(Failure::usage)?;
    if lines.is_empty() {
        println!("no audit lines");
    }
    for line in &lines {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
