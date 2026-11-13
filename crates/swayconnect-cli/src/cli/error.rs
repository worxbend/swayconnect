//! Why a command failed, and the exit code of `docs/DESIGN.md` 7.2 for it.

use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_protocol::uri::UriError;
use swayconnect_protocol::{ByeCode, ErrorBody, ErrorCode, InvalidMember};

use super::store::StoreError;
use crate::client::ClientError;

const EXIT_SERVER_ERROR: u8 = 1;
const EXIT_USAGE_OR_PROFILE: u8 = 2;
const EXIT_CONNECTION: u8 = 3;
const EXIT_REJECTED: u8 = 4;

/// The `Display` text is safe for a terminal: server text in it is escaped.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Usage(&'static str),
    #[error("the pairing URI is not valid: {0}")]
    Uri(#[from] UriError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{}", client_text(.0))]
    Client(#[from] ClientError),
    #[error("no answer from the server within {0} s")]
    Timeout(u64),
    #[error("the pairing was not confirmed in time")]
    NotConfirmed,
    #[error("the server sent no state of topic `desktop`")]
    NoDesktopState,
    #[error("the server sent an invalid desktop state: member `{}`", .0.0)]
    InvalidState(InvalidMember),
    /// The state has no workspace with this number or id.
    #[error("no workspace {0} in the desktop state")]
    NoWorkspace(String),
    #[error("several workspaces have number {0}: choose one with --id")]
    SeveralWorkspaces(u32),
    #[error("the server sends no layout for this workspace: {0}")]
    NoLayout(&'static str),
    #[error("the server has no icon for this hash")]
    NoIcon,
    #[error("the server sent an invalid icon: {0}")]
    InvalidIcon(String),
    #[error("cannot {action}: {source}")]
    Io {
        action: &'static str,
        source: std::io::Error,
    },
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Usage(_) | CliError::Uri(_) | CliError::Store(_) => EXIT_USAGE_OR_PROFILE,
            CliError::Client(error) => client_exit_code(error),
            CliError::NotConfirmed => EXIT_REJECTED,
            // Like the server's `not_found`: the command reached the server, its target is not
            // there.
            CliError::NoWorkspace(_)
            | CliError::SeveralWorkspaces(_)
            | CliError::NoLayout(_)
            | CliError::NoIcon => EXIT_SERVER_ERROR,
            CliError::Timeout(_)
            | CliError::NoDesktopState
            | CliError::InvalidState(_)
            | CliError::InvalidIcon(_)
            | CliError::Io { .. } => EXIT_CONNECTION,
        }
    }
}

fn client_exit_code(error: &ClientError) -> u8 {
    match error {
        // An operation the server did not offer would be answered with `forbidden`.
        ClientError::Failed(_) | ClientError::NotOffered(_) => EXIT_SERVER_ERROR,
        ClientError::Rejected { .. }
        | ClientError::BadWelcome(_)
        | ClientError::Bye {
            code: ByeCode::Revoked,
            ..
        } => EXIT_REJECTED,
        ClientError::Io(_)
        | ClientError::PinMismatch
        | ClientError::Tls(_)
        | ClientError::NoAlpn
        | ClientError::Random(_)
        | ClientError::Frame(_)
        | ClientError::Encode(_)
        | ClientError::Protocol(_)
        | ClientError::NoCommonVersion { .. }
        | ClientError::Bye { .. }
        | ClientError::Closed
        | ClientError::NoAnswer
        | ClientError::TooManyPending => EXIT_CONNECTION,
    }
}

/// `ClientError` keeps server text out of its own `Display`; it is added here, escaped.
fn client_text(error: &ClientError) -> String {
    let (server_message, retry_hint) = match error {
        ClientError::Rejected { message, .. } | ClientError::Bye { message, .. } => {
            (message, String::new())
        }
        ClientError::Failed(body) => (&body.message, retry_hint(body)),
        _without_server_text => return error.to_string(),
    };
    let server_message = escape_for_terminal(server_message);
    format!("{error}: {server_message}{retry_hint}")
}

/// `rate_limited` says when an action can succeed again (spec 10.1). Empty for every other
/// error, and for a server that left the delay out.
fn retry_hint(body: &ErrorBody) -> String {
    let retry_after_ms = body
        .detail
        .as_ref()
        .filter(|_| body.code == ErrorCode::RateLimited)
        .and_then(|detail| detail.get("retry_after_ms"))
        .and_then(serde_json::Value::as_u64);
    retry_after_ms.map_or_else(String::new, |delay| format!(" (retry after {delay} ms)"))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use swayconnect_protocol::RejectCode;

    use super::*;

    fn bye(code: ByeCode) -> CliError {
        let message = String::new();
        CliError::Client(ClientError::Bye { code, message })
    }

    fn failed(message: &str) -> CliError {
        CliError::Client(ClientError::Failed(ErrorBody {
            code: ErrorCode::Forbidden,
            message: message.to_owned(),
            detail: None,
        }))
    }

    #[test]
    fn exit_codes_follow_the_design_table() {
        let rejected = ClientError::Rejected {
            code: RejectCode::AuthFailed,
            message: String::new(),
        };
        let cases = [
            (failed("no"), 1),
            (
                CliError::Client(ClientError::NotOffered("state.subscribe")),
                1,
            ),
            (CliError::Usage("bad"), 2),
            (CliError::Uri(UriError::NotPairingUri), 2),
            (CliError::Store(StoreError::NoHome), 2),
            (CliError::Client(ClientError::PinMismatch), 3),
            (CliError::Client(ClientError::Closed), 3),
            (CliError::Client(ClientError::NoAnswer), 3),
            (CliError::Client(ClientError::NoAlpn), 3),
            (CliError::Timeout(10), 3),
            (bye(ByeCode::Shutdown), 3),
            (CliError::Client(rejected), 4),
            (CliError::Client(ClientError::BadWelcome("server_proof")), 4),
            (bye(ByeCode::Revoked), 4),
            (CliError::NotConfirmed, 4),
            (CliError::NoWorkspace("3".to_owned()), 1),
            (CliError::SeveralWorkspaces(3), 1),
            (CliError::NoLayout("dropped"), 1),
            (CliError::NoIcon, 1),
            (CliError::InvalidIcon("hash".to_owned()), 3),
        ];
        for (error, exit_code) in cases {
            assert_eq!(error.exit_code(), exit_code, "{error}");
        }
    }

    fn failed_with_detail(code: ErrorCode, detail: Value) -> String {
        let Value::Object(detail) = detail else {
            panic!("not an object");
        };
        CliError::Client(ClientError::Failed(ErrorBody {
            code,
            message: "slow down".to_owned(),
            detail: Some(detail),
        }))
        .to_string()
    }

    #[test]
    fn rate_limited_prints_the_retry_delay() {
        let text = failed_with_detail(ErrorCode::RateLimited, json!({"retry_after_ms": 250}));

        assert_eq!(
            text,
            "the request failed: rate_limited: slow down (retry after 250 ms)"
        );
    }

    #[test]
    fn a_delay_of_the_wrong_type_or_on_another_error_is_not_printed() {
        let expected = "the request failed: rate_limited: slow down";
        for detail in [
            json!({}),
            json!({"retry_after_ms": "\u{1b}[2J"}),
            json!({"retry_after_ms": -5}),
        ] {
            assert_eq!(failed_with_detail(ErrorCode::RateLimited, detail), expected);
        }
        let other = failed_with_detail(ErrorCode::Forbidden, json!({"retry_after_ms": 250}));
        assert_eq!(other, "the request failed: forbidden: slow down");
    }

    #[test]
    fn a_server_error_prints_its_code_and_the_escaped_message() {
        let text = failed("no \u{1b}[31mscope\u{202e}").to_string();

        assert_eq!(
            text,
            "the request failed: forbidden: no \\u{1b}[31mscope\\u{202e}"
        );
    }
}
