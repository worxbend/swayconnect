//! A decoded `desktop` state whose text is safe to print.

use swayconnect_protocol::ops::{TOPIC_DESKTOP, TopicState};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{Desktop, InvalidMember, JsonObject};

/// Exists only with every control or format character replaced, so nothing that prints it
/// can pass a hostile title to the terminal (spec 9.3 rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopState {
    rev: u64,
    desktop: Desktop,
}

impl DesktopState {
    /// `None` for a state of another topic.
    pub fn from_topic_state(state: &TopicState) -> Option<Result<DesktopState, InvalidMember>> {
        (state.topic == TOPIC_DESKTOP).then(|| {
            let desktop = Desktop::decode(&state.data)?;
            Ok(DesktopState {
                rev: state.rev,
                desktop: without_unsafe_text(desktop),
            })
        })
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn desktop(&self) -> &Desktop {
        &self.desktop
    }

    /// The wire form `{topic, rev, data}`, with the replaced text.
    pub fn to_json(&self) -> JsonObject {
        let state = TopicState {
            topic: TOPIC_DESKTOP.to_owned(),
            rev: self.rev,
            data: self.desktop.encode(),
        };
        state.encode()
    }
}

fn without_unsafe_text(mut desktop: Desktop) -> Desktop {
    replace_in(&mut desktop.sway.version);
    for workspace in &mut desktop.workspaces {
        workspace.name = replace_unsafe(&workspace.name);
        replace_in(&mut workspace.output);
    }
    for window in &mut desktop.windows {
        replace_in(&mut window.app_id);
        replace_in(&mut window.title);
    }
    desktop
}

fn replace_in(text: &mut Option<String>) {
    if let Some(text) = text {
        *text = replace_unsafe(text);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use swayconnect_protocol::text::is_control_or_format;

    use super::*;

    const HOSTILE: &str = "a\u{1b}[2Jb\u{9b}c\u{202e}d";
    const REPLACED: &str = "a\u{fffd}[2Jb\u{fffd}c\u{fffd}d";

    fn topic_state(topic: &str) -> TopicState {
        let data = json!({
            "sway": {"online": true, "version": HOSTILE},
            "workspaces": [{
                "id": 4, "number": 1, "name": HOSTILE, "output": HOSTILE,
                "focused": true, "visible": true, "urgent": false,
            }],
            "windows": [{
                "id": 9, "workspace_id": 4, "app_id": HOSTILE, "title": HOSTILE,
                "focused": true, "urgent": false, "floating": false, "fullscreen": false,
            }],
            "truncated": false,
        });
        let Value::Object(data) = data else {
            panic!("not an object");
        };
        TopicState {
            topic: topic.to_owned(),
            rev: 7,
            data,
        }
    }

    #[test]
    fn every_text_member_loses_its_control_and_format_characters() {
        let state = DesktopState::from_topic_state(&topic_state("desktop"))
            .expect("desktop topic")
            .expect("valid state");

        let desktop = state.desktop();
        assert_eq!(desktop.sway.version.as_deref(), Some(REPLACED));
        assert_eq!(desktop.workspaces[0].name, REPLACED);
        assert_eq!(desktop.workspaces[0].output.as_deref(), Some(REPLACED));
        assert_eq!(desktop.windows[0].app_id.as_deref(), Some(REPLACED));
        assert_eq!(desktop.windows[0].title.as_deref(), Some(REPLACED));
        let json_text = Value::Object(state.to_json()).to_string();
        assert!(!json_text.chars().any(is_control_or_format));
        assert_eq!(state.to_json()["topic"], "desktop");
        assert_eq!(state.to_json()["rev"], 7);
    }

    #[test]
    fn a_state_of_another_topic_is_not_a_desktop_state() {
        assert_eq!(DesktopState::from_topic_state(&topic_state("audio")), None);
    }

    #[test]
    fn a_state_with_a_wrong_member_type_is_an_error() {
        let mut state = topic_state("desktop");
        state.data.insert("truncated".to_owned(), json!("no"));

        let decoded = DesktopState::from_topic_state(&state).expect("desktop topic");

        assert_eq!(decoded, Err(InvalidMember("truncated")));
    }
}
