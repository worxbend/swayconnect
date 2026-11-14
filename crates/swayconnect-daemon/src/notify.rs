//! Desktop notices (`docs/DESIGN.md` 11.8; 11.3 rule 11) through `[notify] notify_send`.
//! A notice is one fixed text plus the device short id. No client string reaches it, and no
//! text holds markup.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use swayconnect_protocol::{DeviceId, Scope};

use crate::devices::MIN_PREFIX_CHARS;
use crate::proc::{ChildCommand, Exit, ProcError, ProgramPath, Runner};

/// notify-send ends at once; a longer run means the notification server hangs.
pub const NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_NOTIFY_SEND: &str = "/usr/bin/notify-send";
const APP_NAME_ARG: &str = "--app-name=swayconnect";
const SUMMARY: &str = "swayconnect";

/// Config section `[notify]` (`docs/DESIGN.md` 11.9). `config.rs` embeds it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyConfig {
    #[serde(default = "default_notify_send")]
    pub notify_send: ProgramPath,
}

impl Default for NotifyConfig {
    fn default() -> NotifyConfig {
        NotifyConfig {
            notify_send: default_notify_send(),
        }
    }
}

fn default_notify_send() -> ProgramPath {
    ProgramPath::new(PathBuf::from(DEFAULT_NOTIFY_SEND)).expect("the default path is absolute")
}

/// The tier 2 scopes of `docs/DESIGN.md` 11.8: holding one makes a connect a notice.
const TIER_2_SCOPES: [Scope; 7] = [
    Scope::IdleInhibit,
    Scope::Launch,
    Scope::InputKeys,
    Scope::InputText,
    Scope::Destructive,
    Scope::AudioCapture,
    Scope::LaunchRisky,
];

/// `true` when `scopes` hold a tier 2 scope.
pub fn holds_tier_2<'a>(scopes: impl IntoIterator<Item = &'a Scope>) -> bool {
    scopes
        .into_iter()
        .any(|scope| TIER_2_SCOPES.contains(scope))
}

/// Every notice the daemon shows (`docs/DESIGN.md` 11.8, "Desktop notices").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// A device that holds a tier 2 scope connected.
    RiskyDeviceConnected,
    /// The first input of a connection armed a remote keyboard session.
    RemoteKeyboardActive,
    KeepAwakeOn,
    /// A device unmuted an audio source.
    MicrophoneUnmuted,
}

impl Notice {
    pub const ALL: [Notice; 4] = [
        Notice::RiskyDeviceConnected,
        Notice::RemoteKeyboardActive,
        Notice::KeepAwakeOn,
        Notice::MicrophoneUnmuted,
    ];

    fn text(self) -> &'static str {
        match self {
            Notice::RiskyDeviceConnected => "Device with remote control rights connected",
            Notice::RemoteKeyboardActive => "Remote keyboard active",
            Notice::KeepAwakeOn => "Keep awake on",
            Notice::MicrophoneUnmuted => "Microphone unmuted remotely",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error(transparent)]
    Proc(#[from] ProcError),
    #[error("notify-send ended with {0:?}")]
    Failed(Exit),
}

/// Shows notices. Cheap to clone.
#[derive(Clone)]
pub struct Notifier {
    runner: Arc<dyn Runner>,
    program: ProgramPath,
}

impl Notifier {
    pub fn new(runner: Arc<dyn Runner>, config: &NotifyConfig) -> Notifier {
        Notifier {
            runner,
            program: config.notify_send.clone(),
        }
    }

    /// Runs notify-send to its end, at most [`NOTIFY_TIMEOUT`].
    pub async fn show(&self, notice: Notice, device: &DeviceId) -> Result<(), NotifyError> {
        let finished = self
            .runner
            .run(self.command(notice, device), NOTIFY_TIMEOUT)
            .await?;
        if finished.exit.success() {
            Ok(())
        } else {
            Err(NotifyError::Failed(finished.exit))
        }
    }

    /// Shows `notice` off the caller's task; a failure is only logged, because a notice
    /// never holds up the request or connection that caused it.
    pub fn spawn_show(&self, notice: Notice, device: DeviceId) {
        let notifier = self.clone();
        tokio::spawn(async move {
            if let Err(error) = notifier.show(notice, &device).await {
                tracing::warn!(%error, ?notice, "desktop notice not shown");
            }
        });
    }

    fn command(&self, notice: Notice, device: &DeviceId) -> ChildCommand {
        ChildCommand::new(&self.program)
            .arg(APP_NAME_ARG)
            .arg("--")
            .arg(SUMMARY)
            .arg(body(notice, device))
    }
}

/// The short id is a base64url prefix, so the body holds no markup character.
fn body(notice: Notice, device: &DeviceId) -> String {
    let short_id: String = device.as_str().chars().take(MIN_PREFIX_CHARS).collect();
    format!("{} (device {short_id})", notice.text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::fake::FakeRunner;
    use crate::proc::{Captured, Finished};

    fn device() -> DeviceId {
        DeviceId::from_bytes([0xfb; 16])
    }

    fn notifier(runner: &FakeRunner) -> Notifier {
        let config: NotifyConfig =
            toml::from_str(r#"notify_send = "/fake/notify-send""#).expect("config");
        Notifier::new(Arc::new(runner.clone()), &config)
    }

    #[test]
    fn only_a_tier_2_scope_makes_a_connect_a_notice() {
        let steer = [Scope::StateRead, Scope::StateTitles, Scope::Control];
        assert!(!holds_tier_2(&steer));
        assert!(!holds_tier_2(&[Scope::Audio, Scope::Media]));
        for risky in TIER_2_SCOPES {
            assert!(holds_tier_2(&[Scope::StateRead, risky]), "{risky:?}");
        }
    }

    #[tokio::test]
    async fn a_notice_runs_the_configured_program_with_fixed_text() {
        let runner = FakeRunner::default();
        notifier(&runner)
            .show(Notice::KeepAwakeOn, &device())
            .await
            .expect("shown");
        assert_eq!(
            runner.argvs(),
            [[
                "/fake/notify-send",
                "--app-name=swayconnect",
                "--",
                "swayconnect",
                "Keep awake on (device -_v7)",
            ]]
        );
    }

    #[test]
    fn every_text_is_free_of_markup_and_control_characters() {
        let allowed =
            |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '(' | ')' | '-' | '_');
        for notice in Notice::ALL {
            let text = body(notice, &device());
            assert!(text.chars().all(allowed), "{text:?}");
            assert!(!text.starts_with('-'), "{text:?}");
        }
    }

    #[tokio::test]
    async fn a_failing_notify_send_is_an_error() {
        let runner = FakeRunner::default();
        runner.reply_next_run(Ok(Finished {
            exit: Exit::Code(1),
            stdout: Captured::default(),
            stderr: Captured::default(),
        }));
        let result = notifier(&runner)
            .show(Notice::RemoteKeyboardActive, &device())
            .await;
        assert!(matches!(result, Err(NotifyError::Failed(Exit::Code(1)))));

        runner.reply_next_run(Err(ProcError::TimedOut(NOTIFY_TIMEOUT)));
        let result = notifier(&runner)
            .show(Notice::RemoteKeyboardActive, &device())
            .await;
        assert!(matches!(result, Err(NotifyError::Proc(_))));
    }

    #[test]
    fn the_default_program_is_the_system_notify_send() {
        let config: NotifyConfig = toml::from_str("").expect("config");
        assert_eq!(config, NotifyConfig::default());
        assert_eq!(
            config.notify_send.as_path(),
            std::path::Path::new(DEFAULT_NOTIFY_SEND)
        );
        assert!(toml::from_str::<NotifyConfig>(r#"notify_send = "notify-send""#).is_err());
        assert!(toml::from_str::<NotifyConfig>("extra = 1").is_err());
    }
}
