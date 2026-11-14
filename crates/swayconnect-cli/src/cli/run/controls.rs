//! The M2b commands (`docs/PROTOCOL.md` 17.9 to 17.13). A command that needs a server id it
//! was not given reads it from the topic state on the connection that then sends the request.

use std::io::Read;
use std::time::Duration;

use serde_json::json;
use swayconnect_protocol::limits::{INPUT_TEXT_SCALARS_PER_MINUTE, MAX_INPUT_TEXT_BYTES};
use swayconnect_protocol::ops::{Args, TOPIC_APPS, TOPIC_AUDIO, TOPIC_MEDIA};
use swayconnect_protocol::{
    Apps, Audio, AudioChange, ErrorBody, ErrorCode, InputRefusal, InputTarget, InputText,
    InputTextResult, InvalidMember, JsonObject, KeyPress, Media, MediaCommand,
};

use super::super::control_args::{DeviceArg, KeyArgs, LaunchArgs};
use super::super::error::retry_after_ms;
use super::{App, CliError, within};
use crate::client::{ClientError, Session};

/// More could not be typed within one minute anyway (spec 17.14, class `input_text`).
const MAX_TYPED_BYTES: usize = INPUT_TEXT_SCALARS_PER_MINUTE;
const MAX_RATE_LIMITED_RETRIES: usize = 4;
const BAD_TEXT: &str = "the text has a control or format character, such as a newline or a tab: \
                        send Enter with `swayconnect key Return`";

impl App<'_> {
    /// Without `--device` the change goes to the default sink of the `audio` state.
    pub(super) async fn audio(
        &mut self,
        device: DeviceArg,
        change: AudioChange,
    ) -> Result<(), CliError> {
        let mut session = self.connect().await?;
        let device_id = match device.device_id {
            Some(device_id) => device_id,
            None => default_sink(
                &self
                    .read_state(&mut session, TOPIC_AUDIO, Audio::decode)
                    .await?,
            )?,
        };
        let result = self
            .request_on(session, &Args::Audio { device_id, change })
            .await?;
        self.print_result(result, "ok")
    }

    /// Without `--player` the command goes to the active player of the `media` state.
    pub(super) async fn play_pause(&mut self, player_id: Option<u64>) -> Result<(), CliError> {
        let mut session = self.connect().await?;
        let player_id = match player_id {
            Some(player_id) => player_id,
            None => self
                .read_state(&mut session, TOPIC_MEDIA, Media::decode)
                .await?
                .active_player_id
                .ok_or(CliError::NoActivePlayer)?,
        };
        let command = MediaCommand::PlayPause;
        let result = self
            .request_on(session, &Args::Media { player_id, command })
            .await?;
        self.print_result(result, "ok")
    }

    /// Asks for `--yes` before a risky app (spec 17.19 rule 18). `ok` means that the server
    /// started it, not that a window appeared.
    pub(super) async fn launch(&mut self, launch: &LaunchArgs) -> Result<(), CliError> {
        let mut session = self.connect().await?;
        let apps = self
            .read_state(&mut session, TOPIC_APPS, Apps::decode)
            .await?;
        let entry_id = &launch.entry_id;
        let app = apps
            .apps
            .iter()
            .find(|app| app.entry_id == entry_id.as_str())
            .ok_or_else(|| CliError::NoApp(entry_id.to_string()))?;
        if app.risky && !launch.yes {
            return Err(CliError::Usage(
                "this app is risky, a terminal or marked by the owner: add --yes",
            ));
        }
        let action = Args::AppLaunch {
            entry_id: entry_id.clone(),
            number: launch.number,
        };
        let result = self.request_on(session, &action).await?;
        self.print_result(result, "started")
    }

    /// Text comes from stdin only. Every request is checked before the first is sent, and
    /// typing stops at the first error (spec 17.13).
    pub(super) async fn type_text(&mut self, window_id: u64) -> Result<(), CliError> {
        let text = self.read_typed_text()?;
        let requests = input_requests(&text)?;
        let target = InputTarget::Window(window_id);
        let mut session = self.connect().await?;
        let mut typed_chars = 0;
        for text in requests {
            let action = Args::InputText { target, text };
            let result = match self.request_paced(&mut session, &action).await {
                Ok(result) => result,
                Err(CliError::Client(ClientError::Failed(body))) => {
                    return Err(typing_stopped(body, typed_chars));
                }
                Err(failure) => return Err(failure),
            };
            let delivered = InputTextResult::decode(&result).map_err(CliError::InvalidResult)?;
            typed_chars += delivered.delivered_chars;
        }
        super::finish(session).await;
        if self.prints_json {
            self.print_json(&json!({ "delivered_chars": typed_chars }))
        } else {
            self.print(&format!("typed {typed_chars} characters"))
        }
    }

    pub(super) async fn press_key(&mut self, key: &KeyArgs) -> Result<(), CliError> {
        let press = KeyPress::new(key.key, key.modifiers.clone(), key.repeat).map_err(|_| {
            CliError::Usage(
                "modifiers must be distinct, and ctrl with alt and F1 to F12 is refused: \
                 it switches the virtual terminal",
            )
        })?;
        let target = InputTarget::Window(key.target.window_id);
        let result = self.request(&Args::InputKey { target, press }).await?;
        self.print_result(result, "ok")
    }

    async fn read_state<T>(
        &self,
        session: &mut Session,
        topic: &'static str,
        decode: fn(&JsonObject) -> Result<T, InvalidMember>,
    ) -> Result<T, CliError> {
        let states = within(self.answer_timeout, session.subscribe(&[topic])).await?;
        let state = states
            .into_iter()
            .find(|state| state.topic == topic)
            .ok_or(CliError::NoState(topic))?;
        decode(&state.data).map_err(|member| CliError::InvalidTopicState { topic, member })
    }

    /// `rate_limited` comes before any part is typed (spec 10.2 step 5), so the same text
    /// goes again after the delay, when the delay fits the answer timeout.
    async fn request_paced(
        &self,
        session: &mut Session,
        action: &Args,
    ) -> Result<JsonObject, CliError> {
        for _retry in 0..MAX_RATE_LIMITED_RETRIES {
            match within(self.answer_timeout, session.request(action)).await {
                Err(CliError::Client(ClientError::Failed(body)))
                    if body.code == ErrorCode::RateLimited =>
                {
                    let delay = retry_after_ms(&body)
                        .map(Duration::from_millis)
                        .filter(|delay| *delay <= self.answer_timeout);
                    let Some(delay) = delay else {
                        return Err(ClientError::Failed(body).into());
                    };
                    tokio::time::sleep(delay).await;
                }
                answer => return answer,
            }
        }
        within(self.answer_timeout, session.request(action)).await
    }

    /// One final newline is dropped, so `echo text |` types `text`.
    fn read_typed_text(&mut self) -> Result<String, CliError> {
        let mut bytes = Vec::new();
        let read_limit = MAX_TYPED_BYTES as u64 + 2;
        (&mut *self.context.input)
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|source| CliError::Io {
                action: "read the text from stdin",
                source,
            })?;
        let mut text =
            String::from_utf8(bytes).map_err(|_| CliError::Usage("the text is not UTF-8"))?;
        if text.ends_with('\n') {
            text.pop();
        }
        if text.len() > MAX_TYPED_BYTES {
            return Err(CliError::Usage(
                "the text is longer than 4096 bytes, more than the server types in a minute",
            ));
        }
        Ok(text)
    }
}

fn default_sink(audio: &Audio) -> Result<u64, CliError> {
    if !audio.available {
        return Err(CliError::NoDefaultSink(
            "the sound server cannot be reached",
        ));
    }
    audio
        .default_sink
        .ok_or(CliError::NoDefaultSink("choose a device with --device"))
}

/// Requests of at most 1024 bytes, cut at character boundaries (spec 17.13 `input.text`).
fn input_requests(text: &str) -> Result<Vec<InputText>, CliError> {
    if text.is_empty() {
        return Err(CliError::Usage("stdin holds no text to type"));
    }
    let mut requests = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(MAX_INPUT_TEXT_BYTES);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (request, tail) = rest.split_at(end);
        requests.push(InputText::parse(request).ok_or(CliError::Usage(BAD_TEXT))?);
        rest = tail;
    }
    Ok(requests)
}

/// A refusal of spec 17.13 rule 2 with the count of every request so far; any other error
/// as it came.
fn typing_stopped(body: ErrorBody, typed_before: u64) -> CliError {
    match InputRefusal::decode(&body) {
        Ok(Some(refusal)) => CliError::TypingStopped {
            refusal,
            typed_chars: typed_before.saturating_add(refusal.delivered_chars()),
            message: body.message,
        },
        Ok(None) => ClientError::Failed(body).into(),
        Err(member) => CliError::InvalidResult(member),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn texts(requests: &[InputText]) -> Vec<&str> {
        requests.iter().map(InputText::as_str).collect()
    }

    #[test]
    fn text_up_to_1024_bytes_is_one_request() {
        let full = "a".repeat(MAX_INPUT_TEXT_BYTES);
        for text in ["a", "Hello, мир 👩‍💻", full.as_str()] {
            let requests = input_requests(text).expect("valid text");
            assert_eq!(texts(&requests), [text]);
        }
    }

    #[test]
    fn longer_text_is_cut_at_character_boundaries() {
        let ascii = "a".repeat(MAX_INPUT_TEXT_BYTES + 1);
        let requests = input_requests(&ascii).expect("valid text");
        assert_eq!(texts(&requests), [&ascii[..MAX_INPUT_TEXT_BYTES], "a"]);

        // 342 three-byte characters: the 1024-byte cut falls inside the last one.
        let wide = "€".repeat(342);
        let requests = input_requests(&wide).expect("valid text");
        let lengths: Vec<usize> = texts(&requests).iter().map(|text| text.len()).collect();
        assert_eq!(lengths, [1023, 3]);
        assert_eq!(texts(&requests).concat(), wide);
    }

    #[test]
    fn a_bad_character_anywhere_refuses_every_request() {
        let late_newline = format!("{}\n", "a".repeat(2000));
        for text in [
            "",
            "line\nline",
            "tab\t",
            "\u{202e}",
            "e\u{e0041}",
            &late_newline,
        ] {
            let refused = input_requests(text).expect_err("refused");
            assert_eq!(refused.exit_code(), 2, "{text:?}");
        }
    }

    fn error_body(code: ErrorCode, detail: Option<Value>) -> ErrorBody {
        let detail = detail.map(|detail| match detail {
            Value::Object(detail) => detail,
            _ => panic!("not an object"),
        });
        let message = "stopped".to_owned();
        ErrorBody {
            code,
            message,
            detail,
        }
    }

    #[test]
    fn a_refusal_adds_its_delivered_chars_to_the_earlier_requests() {
        let detail = json!({"delivered_chars": 16, "focused_window_id": 7});
        let body = error_body(ErrorCode::FocusChanged, Some(detail));

        let CliError::TypingStopped {
            refusal,
            typed_chars,
            ..
        } = typing_stopped(body, 1024)
        else {
            panic!("not a refusal");
        };

        assert_eq!(typed_chars, 1040);
        let focused_window_id = Some(7);
        let expected = InputRefusal::FocusChanged {
            delivered_chars: 16,
            focused_window_id,
        };
        assert_eq!(refusal, expected);
    }

    #[test]
    fn another_error_stays_as_it_came_and_a_bad_detail_is_invalid() {
        let other = typing_stopped(error_body(ErrorCode::BackendUnavailable, None), 0);
        assert!(matches!(other, CliError::Client(ClientError::Failed(_))));

        let bad = typing_stopped(error_body(ErrorCode::Locked, Some(json!({}))), 0);
        assert!(matches!(bad, CliError::InvalidResult(_)), "{bad}");
    }
}
