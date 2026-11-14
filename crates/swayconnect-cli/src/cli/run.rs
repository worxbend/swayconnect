//! Runs one command against the server of the stored profile.

mod controls;

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use swayconnect_protocol::limits::PAIRING_CONFIRMATION_DEADLINE_MS;
use swayconnect_protocol::ops::{Args, KeepAwake, TOPIC_DESKTOP, WorkspaceTarget};
use swayconnect_protocol::text::{escape_for_terminal, is_display_name};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{
    AudioChange, IconGetResult, IconHash, InvalidMember, JsonObject, KeepAwakeResult, LockResult,
};
use tokio::time::{Instant, timeout, timeout_at};

use super::args::{AwakeSwitch, Cli, Command, Switch};
use super::duration::remaining_text;
use super::error::CliError;
use super::state::DesktopState;
use super::store::ProfileStore;
use super::{layout_map, render};
use crate::client::{ClientError, Connector, Event, Session};

const DEFAULT_DEVICE_NAME: &str = "swayconnect-cli";
const PLATFORM: &str = "linux";
const RAW_REPLY_WINDOW: Duration = Duration::from_secs(2);

/// Everything a command touches outside its arguments.
pub struct Context<'a> {
    pub connector: Connector,
    pub store: ProfileStore,
    pub input: &'a mut dyn BufRead,
    pub output: &'a mut dyn Write,
}

pub async fn run(cli: &Cli, context: Context<'_>) -> Result<(), CliError> {
    let mut app = App {
        prints_json: cli.json,
        answer_timeout: cli.timeout(),
        context,
    };
    match &cli.command {
        Command::Pair {
            name,
            uri_arguments,
        } => {
            if !uri_arguments.is_empty() {
                return Err(CliError::Usage(
                    "the pairing URI is read from stdin: other local users can read arguments",
                ));
            }
            app.pair(name.as_deref().unwrap_or(DEFAULT_DEVICE_NAME))
                .await
        }
        Command::Info => app.info().await,
        Command::State => app.state().await,
        Command::Workspaces => app.workspaces().await,
        Command::Windows => app.windows().await,
        Command::Watch => app.watch().await,
        Command::Workspace { number, id } => {
            let target = workspace_target(*number, *id)?;
            app.act(&Args::WorkspaceFocus(target)).await
        }
        Command::Focus { window_id } => {
            let window_id = *window_id;
            app.act(&Args::WindowFocus { window_id }).await
        }
        Command::Fullscreen { window_id, switch } => {
            let action = Args::WindowFullscreen {
                window_id: *window_id,
                enabled: *switch == Switch::On,
            };
            app.act(&action).await
        }
        Command::Move {
            window_id,
            number,
            id,
        } => {
            let target = workspace_target(*number, *id)?;
            let window_id = *window_id;
            app.act(&Args::WindowMove { window_id, target }).await
        }
        Command::Float { window_id, switch } => {
            let action = Args::WindowFloating {
                window_id: *window_id,
                enabled: *switch == Switch::On,
            };
            app.act(&action).await
        }
        Command::Close { window_id, yes } => {
            // Spec 17.7: a client asks before it sends `window.close`.
            if !yes {
                return Err(CliError::Usage(
                    "close lets the app close the window and may lose unsaved work: add --yes",
                ));
            }
            let window_id = *window_id;
            app.act(&Args::WindowClose { window_id }).await
        }
        Command::Lock => app.lock().await,
        Command::Awake { switch } => app.keep_awake(keep_awake_args(*switch)).await,
        Command::Volume(volume) => {
            let change = AudioChange::Volume(volume.volume_pct);
            app.audio(volume.device, change).await
        }
        Command::Mute(mute) => {
            let change = AudioChange::Mute(mute.switch == Switch::On);
            app.audio(mute.device, change).await
        }
        Command::Play(play) => app.play_pause(play.player_id).await,
        Command::Launch(launch) => app.launch(launch).await,
        Command::Type(typed) => {
            if !typed.text_arguments.is_empty() {
                return Err(CliError::Usage(
                    "the text is read from stdin: other local users can read arguments",
                ));
            }
            app.type_text(typed.target.window_id).await
        }
        Command::Key(key) => app.press_key(key).await,
        Command::Layout { number, id } => app.layout(*number, *id).await,
        Command::Icon { hash, output } => app.icon(*hash, output).await,
        Command::Raw { payload } => app.raw(payload).await,
    }
}

/// The argument parser lets exactly one of the two through; this does not rely on it.
fn workspace_target(number: Option<u16>, id: Option<u64>) -> Result<WorkspaceTarget, CliError> {
    match (number, id) {
        (Some(number), None) => Ok(WorkspaceTarget::Number(number)),
        (None, Some(id)) => Ok(WorkspaceTarget::Id(id)),
        (Some(_), Some(_)) | (None, None) => {
            Err(CliError::Usage("a workspace is a number or --id, not both"))
        }
    }
}

fn keep_awake_args(switch: AwakeSwitch) -> KeepAwake {
    match switch {
        AwakeSwitch::On { duration_s } => KeepAwake::On { duration_s },
        AwakeSwitch::Off => KeepAwake::Off,
    }
}

struct App<'a> {
    prints_json: bool,
    answer_timeout: Duration,
    context: Context<'a>,
}

impl App<'_> {
    async fn pair(&mut self, device_name: &str) -> Result<(), CliError> {
        if !is_display_name(device_name) {
            return Err(CliError::Usage(
                "--name must be 1 to 64 bytes without control or format characters",
            ));
        }
        let uri = self.read_pairing_uri()?;
        let pairing = self
            .context
            .connector
            .pair(&uri, device_name, Some(PLATFORM));
        let pending = within(self.answer_timeout, pairing).await?;

        // `welcome` is proof-checked, but its text still comes from the server.
        let confirm_code = escape_for_terminal(pending.confirm_code());
        if self.prints_json {
            self.print_json(&json!({ "confirm_code": confirm_code }))?;
        } else {
            self.print(&format!("code: {confirm_code}"))?;
            self.print("Compare the code with the one on the desktop and confirm it there.")?;
        }

        let confirmation_deadline = confirmation_deadline(self.answer_timeout);
        let profile = timeout(confirmation_deadline, pending.confirmed())
            .await
            .map_err(|_elapsed| CliError::NotConfirmed)??;
        self.context.store.save(&profile)?;

        let server_name = escape_for_terminal(&profile.server_name);
        if self.prints_json {
            let device_id = profile.device_id.as_str();
            self.print_json(&json!({ "paired": server_name, "device_id": device_id }))
        } else {
            self.print(&format!("paired: {server_name}"))
        }
    }

    fn read_pairing_uri(&mut self) -> Result<PairingUri, CliError> {
        let mut line = String::new();
        self.context
            .input
            .read_line(&mut line)
            .map_err(|source| CliError::Io {
                action: "read the pairing URI from stdin",
                source,
            })?;
        Ok(PairingUri::parse(line.trim())?)
    }

    async fn info(&mut self) -> Result<(), CliError> {
        let session = self.connect().await?;
        let welcome = session.welcome();
        if self.prints_json {
            self.print_json(&render::info_json(welcome))?;
        } else {
            self.print(&render::info_text(welcome))?;
        }
        finish(session).await;
        Ok(())
    }

    async fn state(&mut self) -> Result<(), CliError> {
        let (session, state) = self.desktop_state().await?;
        let response = json!({ "states": [state.to_json()] });
        if self.prints_json {
            self.print_json(&response)?;
        } else {
            self.print(&format!("{response:#}"))?;
        }
        finish(session).await;
        Ok(())
    }

    async fn workspaces(&mut self) -> Result<(), CliError> {
        let (session, state) = self.desktop_state().await?;
        let desktop = state.desktop();
        if self.prints_json {
            for workspace in &desktop.encode()["workspaces"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                self.print_json(workspace)?;
            }
        } else {
            self.print(&render::workspaces_table(desktop))?;
            self.print_notes(&state)?;
        }
        finish(session).await;
        Ok(())
    }

    async fn windows(&mut self) -> Result<(), CliError> {
        let (session, state) = self.desktop_state().await?;
        let desktop = state.desktop();
        if self.prints_json {
            for window in &render::windows_json(desktop) {
                self.print_json(window)?;
            }
        } else {
            self.print(&render::windows_table(desktop))?;
            self.print_notes(&state)?;
        }
        finish(session).await;
        Ok(())
    }

    async fn layout(&mut self, number: Option<u32>, id: Option<u64>) -> Result<(), CliError> {
        let (session, state) = self.desktop_state().await?;
        finish(session).await;
        let desktop = state.desktop();
        let workspace = layout_map::find_workspace(desktop, number, id)?;
        let layout = match &workspace.layout {
            Some(Some(layout)) => layout,
            Some(None) if desktop.layouts_cut == Some(true) => {
                return Err(CliError::NoLayout("the state was too large"));
            }
            Some(None) => return Err(CliError::NoLayout("it has no geometry for it")),
            None => return Err(CliError::NoLayout("the server is older than layouts")),
        };
        if self.prints_json {
            self.print_json(&layout_map::layout_json(desktop, workspace.id))
        } else {
            self.print(&layout_map::layout_text(
                workspace,
                layout,
                &desktop.windows,
            ))
        }
    }

    /// Writes the PNG only after the hash and the size in its header are checked.
    async fn icon(&mut self, hash: IconHash, file: &Path) -> Result<(), CliError> {
        let mut session = self.connect().await?;
        let request = Args::IconGet { hashes: vec![hash] };
        let result = within(self.answer_timeout, session.request(&request)).await?;
        finish(session).await;
        let png = icon_png(&result, hash)?;
        std::fs::write(file, &png).map_err(|source| CliError::Io {
            action: "write the icon file",
            source,
        })?;
        let file_name = escape_for_terminal(&file.to_string_lossy());
        if self.prints_json {
            let saved = json!({ "hash": hash.to_string(), "bytes": png.len(), "file": file_name });
            self.print_json(&saved)
        } else {
            self.print(&format!("saved {} bytes to {file_name}", png.len()))
        }
    }

    /// Ends only with an error: the server says `bye` or the connection closes.
    async fn watch(&mut self) -> Result<(), CliError> {
        let (mut session, first_state) = self.desktop_state().await?;
        self.print_watch_line(&first_state)?;
        loop {
            let Event::State(topic_state) = session.next_event().await? else {
                continue;
            };
            if let Some(state) = DesktopState::from_topic_state(&topic_state) {
                self.print_watch_line(&state.map_err(CliError::InvalidState)?)?;
            }
        }
    }

    /// Success means that sway accepted the command; the new state is not part of the answer
    /// (spec 7.2).
    async fn act(&mut self, action: &Args) -> Result<(), CliError> {
        let result = self.request(action).await?;
        self.print_result(result, "ok")
    }

    /// Success means that the server started its lock command, or that it found the screen
    /// locked or its lock command still running (spec 17.6).
    async fn lock(&mut self) -> Result<(), CliError> {
        let result = self.request(&Args::SessionLock).await?;
        let lock = LockResult::decode(&result).map_err(CliError::InvalidResult)?;
        let text = if lock.already_locked {
            "already locked"
        } else {
            "lock started"
        };
        self.print_result(result, text)
    }

    /// Prints the time left that the server answered: it may be shorter than asked (17.8).
    async fn keep_awake(&mut self, switch: KeepAwake) -> Result<(), CliError> {
        let result = self.request(&Args::KeepAwakeSet(switch)).await?;
        let keep_awake = KeepAwakeResult::decode(&result).map_err(CliError::InvalidResult)?;
        let text = keep_awake.remaining_ms.map_or_else(
            || "keep awake off".to_owned(),
            |remaining_ms| format!("keep awake on for {}", remaining_text(remaining_ms)),
        );
        self.print_result(result, &text)
    }

    /// One request on its own connection, closed before the caller prints.
    async fn request(&mut self, action: &Args) -> Result<JsonObject, CliError> {
        let session = self.connect().await?;
        self.request_on(session, action).await
    }

    /// The last request of `session`, which is closed before the caller prints.
    async fn request_on(
        &self,
        mut session: Session,
        action: &Args,
    ) -> Result<JsonObject, CliError> {
        let result = within(self.answer_timeout, session.request(action)).await?;
        finish(session).await;
        Ok(result)
    }

    fn print_result(&mut self, result: JsonObject, text: &str) -> Result<(), CliError> {
        if self.prints_json {
            self.print(&render::server_json(&Value::Object(result)))
        } else {
            self.print(text)
        }
    }

    /// Prints every frame of the reply window, of any type. A server that closes the
    /// connection ends the command early and without an error: `bye` is a reply like another.
    async fn raw(&mut self, payload: &str) -> Result<(), CliError> {
        let mut session = self.connect().await?;
        session.send_raw_frame(payload.as_bytes()).await?;
        let window_end = Instant::now() + RAW_REPLY_WINDOW;
        loop {
            match timeout_at(window_end, session.receive_raw_frame()).await {
                Ok(Ok(reply)) => self.print(&render::server_json(&Value::Object(reply)))?,
                Ok(Err(ClientError::Closed)) => return Ok(()),
                Ok(Err(failure)) => return Err(failure.into()),
                Err(_window_over) => break,
            }
        }
        finish(session).await;
        Ok(())
    }

    async fn connect(&mut self) -> Result<Session, CliError> {
        let profile = self.context.store.load()?;
        within(
            self.answer_timeout,
            self.context.connector.connect(&profile),
        )
        .await
    }

    async fn desktop_state(&mut self) -> Result<(Session, DesktopState), CliError> {
        let mut session = self.connect().await?;
        let states = within(self.answer_timeout, session.subscribe(&[TOPIC_DESKTOP])).await?;
        let state = states
            .iter()
            .find_map(DesktopState::from_topic_state)
            .ok_or(CliError::NoDesktopState)?
            .map_err(CliError::InvalidState)?;
        Ok((session, state))
    }

    fn print_watch_line(&mut self, state: &DesktopState) -> Result<(), CliError> {
        if self.prints_json {
            self.print_json(&Value::Object(state.to_json()))
        } else {
            self.print(&render::watch_line(state))
        }
    }

    fn print_notes(&mut self, state: &DesktopState) -> Result<(), CliError> {
        for note in render::desktop_notes(state.desktop()) {
            self.print(note)?;
        }
        Ok(())
    }

    fn print_json(&mut self, value: &Value) -> Result<(), CliError> {
        self.print(&value.to_string())
    }

    /// Flushes, so that a reader on a pipe sees each `watch` line when it happens.
    fn print(&mut self, text: &str) -> Result<(), CliError> {
        let output = &mut self.context.output;
        writeln!(output, "{text}")
            .and_then(|()| output.flush())
            .map_err(|source| CliError::Io {
                action: "write the output",
                source,
            })
    }
}

/// The checked PNG of `hash` from an `icon.get` result (spec 17.5 rule 3).
fn icon_png(result: &JsonObject, hash: IconHash) -> Result<Vec<u8>, CliError> {
    let result = IconGetResult::decode(result)
        .map_err(|InvalidMember(member)| CliError::InvalidIcon(format!("member `{member}`")))?;
    if result.missing.contains(&hash) {
        return Err(CliError::NoIcon);
    }
    let icon = result
        .icons
        .into_iter()
        .find(|icon| icon.hash == hash)
        .ok_or_else(|| CliError::InvalidIcon("the result does not name the hash".to_owned()))?;
    icon.verify()
        .map_err(|rejection| CliError::InvalidIcon(rejection.to_string()))?;
    Ok(icon.png)
}

/// How long `pair` waits for `pairing.confirmed`. The server ends an unconfirmed pairing
/// itself; this deadline is for a server that neither confirms nor closes.
fn confirmation_deadline(answer_timeout: Duration) -> Duration {
    // `--timeout` takes any number of seconds, so the sum can pass the largest duration.
    Duration::from_millis(PAIRING_CONFIRMATION_DEADLINE_MS).saturating_add(answer_timeout)
}

/// A call that is cut off leaves the connection unusable; every caller ends the command then.
async fn within<T>(
    answer_timeout: Duration,
    call: impl Future<Output = Result<T, ClientError>>,
) -> Result<T, CliError> {
    match timeout(answer_timeout, call).await {
        Ok(answer) => Ok(answer?),
        Err(_elapsed) => Err(CliError::Timeout(answer_timeout.as_secs())),
    }
}

/// Says goodbye with `close_notify`. The answer is printed already, so a server that is gone
/// by now does not turn the command into a failure.
async fn finish(session: Session) {
    let _close_failure = session.close().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_confirmation_deadline_is_the_server_deadline_plus_the_answer_timeout() {
        let deadline = confirmation_deadline(Duration::from_secs(10));

        assert_eq!(deadline, Duration::from_secs(70));
    }

    #[test]
    fn the_largest_timeout_does_not_overflow_the_confirmation_deadline() {
        let deadline = confirmation_deadline(Duration::from_secs(u64::MAX));

        assert_eq!(deadline, Duration::MAX);
    }
}
