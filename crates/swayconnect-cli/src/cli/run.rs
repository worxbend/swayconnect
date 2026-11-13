//! Runs one command against the server of the stored profile.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{Value, json};
use swayconnect_protocol::limits::PAIRING_CONFIRMATION_DEADLINE_MS;
use swayconnect_protocol::ops::{Args, TOPIC_DESKTOP, WorkspaceTarget};
use swayconnect_protocol::text::{escape_for_terminal, is_display_name};
use swayconnect_protocol::uri::PairingUri;
use tokio::time::{Instant, timeout, timeout_at};

use super::args::{Cli, Command, Switch};
use super::error::CliError;
use super::render;
use super::state::DesktopState;
use super::store::ProfileStore;
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
        Command::Raw { payload } => app.raw(payload).await,
    }
}

/// The argument parser lets exactly one of the two through; this does not rely on it.
fn workspace_target(number: Option<u16>, id: Option<u64>) -> Result<WorkspaceTarget, CliError> {
    match (number, id) {
        (Some(number), None) => Ok(WorkspaceTarget::Number(number)),
        (None, Some(id)) => Ok(WorkspaceTarget::Id(id)),
        (Some(_), Some(_)) | (None, None) => Err(CliError::Usage(
            "workspace takes a number or --id, not both",
        )),
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
            for window in &desktop.encode()["windows"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                self.print_json(window)?;
            }
        } else {
            self.print(&render::windows_table(desktop))?;
            self.print_notes(&state)?;
        }
        finish(session).await;
        Ok(())
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
        let mut session = self.connect().await?;
        let result = within(self.answer_timeout, session.request(action)).await?;
        if self.prints_json {
            self.print(&render::server_json(&Value::Object(result)))?;
        } else {
            self.print("ok")?;
        }
        finish(session).await;
        Ok(())
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
