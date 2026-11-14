//! Operations, their arguments and results, and event payloads (`docs/PROTOCOL.md` 7, 8, 17).

mod decode;
mod topic;

use serde_json::Value;

use crate::apps::EntryId;
use crate::audio::AudioChange;
use crate::code::wire_names;
use crate::icon::IconHash;
use crate::input::{InputTarget, InputText, KeyPress};
use crate::json::{self, InvalidMember, object_of};
use crate::media::MediaCommand;
use crate::message::{JsonObject, Req, Scope};

pub(crate) use decode::has_duplicate;
pub use topic::{TopicState, decode_subscribe_result, encode_subscribe_result};

pub const TOPIC_DESKTOP: &str = "desktop";
/// Spec 17.8.
pub const TOPIC_SESSION: &str = "session";
/// Spec 17.9 to 17.11.
pub const TOPIC_AUDIO: &str = "audio";
pub const TOPIC_MEDIA: &str = "media";
pub const TOPIC_APPS: &str = "apps";
pub const EVENT_STATE_CHANGED: &str = "state.changed";
pub const EVENT_PAIRING_CONFIRMED: &str = "pairing.confirmed";

/// The operation table of spec 7.2, and the operations of section 17 this version knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Ping,
    StateSubscribe,
    /// Spec 17.5. Not an action; it has its own rate class `icons`.
    IconGet,
    WorkspaceFocus,
    WindowFocus,
    WindowFullscreen,
    /// Spec 17.6 to 17.8.
    WindowMove,
    WindowFloating,
    WindowLayout,
    WindowScratchpad,
    ScratchpadShow,
    SessionLock,
    WindowClose,
    KeepAwakeSet,
    /// Spec 17.9 to 17.13.
    AudioSetVolume,
    AudioSetMute,
    AudioSetDefault,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    AppLaunch,
    LauncherOpen,
    InputText,
    InputKey,
}

wire_names!(Operation {
    Ping => "ping",
    StateSubscribe => "state.subscribe",
    IconGet => "icon.get",
    WorkspaceFocus => "workspace.focus",
    WindowFocus => "window.focus",
    WindowFullscreen => "window.fullscreen",
    WindowMove => "window.move",
    WindowFloating => "window.floating",
    WindowLayout => "window.layout",
    WindowScratchpad => "window.scratchpad",
    ScratchpadShow => "scratchpad.show",
    SessionLock => "session.lock",
    WindowClose => "window.close",
    KeepAwakeSet => "keep_awake.set",
    AudioSetVolume => "audio.set_volume",
    AudioSetMute => "audio.set_mute",
    AudioSetDefault => "audio.set_default",
    MediaPlayPause => "media.play_pause",
    MediaNext => "media.next",
    MediaPrevious => "media.previous",
    AppLaunch => "app.launch",
    LauncherOpen => "launcher.open",
    InputText => "input.text",
    InputKey => "input.key",
});

/// The per-device token bucket an operation takes from at step 5 of spec 10.2 (17.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateClass {
    /// Version 1's action bucket: burst 5, refill 2 per second.
    Actions,
    /// `icon.get`, also in read-only mode: burst 4, refill 2 per second.
    Icons,
    /// `audio.set_volume`: burst 10, refill 10 per second.
    Continuous,
    /// `app.launch`, `launcher.open`: burst 3, refill 1 per 5 s.
    Launch,
    /// `input.key`: burst 20, refill 10 per second.
    Input,
    /// `input.text`: burst 4, refill 1 per second, and at most 4096 characters per minute.
    InputText,
}

/// Validated arguments of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Args {
    Ping,
    StateSubscribe {
        topics: Vec<String>,
    },
    /// 1 to 8 distinct hashes.
    IconGet {
        hashes: Vec<IconHash>,
    },
    WorkspaceFocus(WorkspaceTarget),
    WindowFocus {
        window_id: u64,
    },
    WindowFullscreen {
        window_id: u64,
        enabled: bool,
    },
    WindowMove {
        window_id: u64,
        target: WorkspaceTarget,
    },
    WindowFloating {
        window_id: u64,
        enabled: bool,
    },
    WindowLayout {
        window_id: u64,
        layout: ContainerLayout,
    },
    WindowScratchpad {
        window_id: u64,
    },
    /// `window_id` is an id of the state's `scratchpad`, not of `windows`.
    ScratchpadShow {
        window_id: u64,
    },
    SessionLock,
    WindowClose {
        window_id: u64,
    },
    KeepAwakeSet(KeepAwake),
    /// `audio.set_volume`, `audio.set_mute`, `audio.set_default`. `device_id` is an id of the
    /// `audio` state, a sink or a source.
    Audio {
        device_id: u64,
        change: AudioChange,
    },
    /// `media.play_pause`, `media.next`, `media.previous`.
    Media {
        player_id: u64,
        command: MediaCommand,
    },
    /// With `number`, the server focuses that workspace (0 to 999) first.
    AppLaunch {
        entry_id: EntryId,
        number: Option<u16>,
    },
    LauncherOpen,
    InputText {
        target: InputTarget,
        text: InputText,
    },
    InputKey {
        target: InputTarget,
        press: KeyPress,
    },
}

/// Exactly one of the two arguments of `workspace.focus` and `window.move`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTarget {
    Id(u64),
    /// 0 to 999.
    Number(u16),
}

/// Argument `layout` of `window.layout` (17.6). A closed enum, matched exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerLayout {
    SplitH,
    SplitV,
    Tabbed,
    Stacking,
}

wire_names!(ContainerLayout {
    SplitH => "splith",
    SplitV => "splitv",
    Tabbed => "tabbed",
    Stacking => "stacking",
});

/// Arguments of `keep_awake.set` (17.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAwake {
    /// `duration_s` is at least 60; `None` means the server's `default_duration_s`.
    On { duration_s: Option<u64> },
    /// `duration_s` is ignored, so it is not read.
    Off,
}

impl KeepAwake {
    /// The upper end of `duration_s` is the server's `max_duration_s`, which this crate does not
    /// know. A server applies this at step 6 of spec 10.2; an error is a `bad_request`.
    pub fn check_max_duration(self, max_duration_s: u64) -> Result<KeepAwake, InvalidMember> {
        match self {
            KeepAwake::On {
                duration_s: Some(duration_s),
            } if duration_s > max_duration_s => Err(InvalidMember(DURATION_S)),
            _ => Ok(self),
        }
    }
}

const DURATION_S: &str = "duration_s";
const DEVICE_ID: &str = "device_id";
const PLAYER_ID: &str = "player_id";
const ENTRY_ID: &str = "entry_id";

/// The scope a topic needs beside `state.read` (17.2): `audio`, `media` or `launch`. `None` for
/// `desktop`, `session` and every name this version does not know.
pub fn topic_scope(topic: &str) -> Option<Scope> {
    match topic {
        TOPIC_AUDIO => Some(Scope::Audio),
        TOPIC_MEDIA => Some(Scope::Media),
        TOPIC_APPS => Some(Scope::Launch),
        _ => None,
    }
}

impl Operation {
    pub const fn required_scope(self) -> Option<Scope> {
        match self {
            Operation::Ping => None,
            Operation::StateSubscribe | Operation::IconGet => Some(Scope::StateRead),
            Operation::WorkspaceFocus
            | Operation::WindowFocus
            | Operation::WindowFullscreen
            | Operation::WindowMove
            | Operation::WindowFloating
            | Operation::WindowLayout
            | Operation::WindowScratchpad
            | Operation::ScratchpadShow
            | Operation::SessionLock => Some(Scope::Control),
            Operation::WindowClose => Some(Scope::Destructive),
            Operation::KeepAwakeSet => Some(Scope::IdleInhibit),
            Operation::AudioSetVolume | Operation::AudioSetMute | Operation::AudioSetDefault => {
                Some(Scope::Audio)
            }
            Operation::MediaPlayPause | Operation::MediaNext | Operation::MediaPrevious => {
                Some(Scope::Media)
            }
            Operation::AppLaunch | Operation::LauncherOpen => Some(Scope::Launch),
            Operation::InputText => Some(Scope::InputText),
            Operation::InputKey => Some(Scope::InputKeys),
        }
    }

    /// An action changes the desktop.
    pub const fn is_action(self) -> bool {
        !matches!(
            self,
            Operation::Ping | Operation::StateSubscribe | Operation::IconGet
        )
    }

    /// Spec 17.14. `None` for an operation that takes no token.
    pub const fn rate_class(self) -> Option<RateClass> {
        match self {
            Operation::Ping | Operation::StateSubscribe => None,
            Operation::IconGet => Some(RateClass::Icons),
            Operation::WorkspaceFocus
            | Operation::WindowFocus
            | Operation::WindowFullscreen
            | Operation::WindowMove
            | Operation::WindowFloating
            | Operation::WindowLayout
            | Operation::WindowScratchpad
            | Operation::ScratchpadShow
            | Operation::SessionLock
            | Operation::WindowClose
            | Operation::KeepAwakeSet
            | Operation::AudioSetMute
            | Operation::AudioSetDefault
            | Operation::MediaPlayPause
            | Operation::MediaNext
            | Operation::MediaPrevious => Some(RateClass::Actions),
            Operation::AudioSetVolume => Some(RateClass::Continuous),
            Operation::AppLaunch | Operation::LauncherOpen => Some(RateClass::Launch),
            Operation::InputKey => Some(RateClass::Input),
            Operation::InputText => Some(RateClass::InputText),
        }
    }

    /// Step 6 of the check order (spec 10.2). `args` is `req.args`; absent means `{}`.
    /// An error is answered with `bad_request`. Topic names are not looked up here. The server
    /// limits of step 6 are [`KeepAwake::check_max_duration`] and
    /// [`AudioChange::check_max_volume`].
    pub fn decode_args(self, args: Option<&Value>) -> Result<Args, InvalidMember> {
        let empty = JsonObject::new();
        let args = match args {
            None => &empty,
            Some(Value::Object(args)) => args,
            Some(_) => return Err(InvalidMember("args")),
        };
        let window_id = || json::int(args, "window_id");
        let enabled = || json::boolean(args, "enabled");
        let audio = |change| {
            Ok(Args::Audio {
                device_id: json::int(args, DEVICE_ID)?,
                change,
            })
        };
        let media = |command| {
            Ok(Args::Media {
                player_id: json::int(args, PLAYER_ID)?,
                command,
            })
        };
        match self {
            Operation::Ping => Ok(Args::Ping),
            Operation::StateSubscribe => Ok(Args::StateSubscribe {
                topics: decode::decode_topics(args)?,
            }),
            Operation::IconGet => Ok(Args::IconGet {
                hashes: decode::decode_hashes(args)?,
            }),
            Operation::WorkspaceFocus => {
                decode::decode_workspace_target(args).map(Args::WorkspaceFocus)
            }
            Operation::WindowFocus => Ok(Args::WindowFocus {
                window_id: window_id()?,
            }),
            Operation::WindowFullscreen => Ok(Args::WindowFullscreen {
                window_id: window_id()?,
                enabled: enabled()?,
            }),
            Operation::WindowMove => Ok(Args::WindowMove {
                window_id: window_id()?,
                target: decode::decode_workspace_target(args)?,
            }),
            Operation::WindowFloating => Ok(Args::WindowFloating {
                window_id: window_id()?,
                enabled: enabled()?,
            }),
            Operation::WindowLayout => Ok(Args::WindowLayout {
                window_id: window_id()?,
                layout: decode::decode_layout(args)?,
            }),
            Operation::WindowScratchpad => Ok(Args::WindowScratchpad {
                window_id: window_id()?,
            }),
            Operation::ScratchpadShow => Ok(Args::ScratchpadShow {
                window_id: window_id()?,
            }),
            Operation::SessionLock => Ok(Args::SessionLock),
            Operation::WindowClose => Ok(Args::WindowClose {
                window_id: window_id()?,
            }),
            Operation::KeepAwakeSet => decode::decode_keep_awake(args).map(Args::KeepAwakeSet),
            Operation::AudioSetVolume => audio(AudioChange::decode_volume(args)?),
            Operation::AudioSetMute => audio(AudioChange::decode_mute(args)?),
            Operation::AudioSetDefault => audio(AudioChange::Default),
            Operation::MediaPlayPause => media(MediaCommand::PlayPause),
            Operation::MediaNext => media(MediaCommand::Next),
            Operation::MediaPrevious => media(MediaCommand::Previous),
            Operation::AppLaunch => Ok(Args::AppLaunch {
                entry_id: EntryId::decode(args, ENTRY_ID)?,
                number: decode::decode_launch_number(args)?,
            }),
            Operation::LauncherOpen => Ok(Args::LauncherOpen),
            Operation::InputText => Ok(Args::InputText {
                target: InputTarget::decode(args)?,
                text: InputText::decode(args)?,
            }),
            Operation::InputKey => Ok(Args::InputKey {
                target: InputTarget::decode(args)?,
                press: KeyPress::decode(args)?,
            }),
        }
    }
}

impl Args {
    pub const fn operation(&self) -> Operation {
        match self {
            Args::Ping => Operation::Ping,
            Args::StateSubscribe { .. } => Operation::StateSubscribe,
            Args::IconGet { .. } => Operation::IconGet,
            Args::WorkspaceFocus(_) => Operation::WorkspaceFocus,
            Args::WindowFocus { .. } => Operation::WindowFocus,
            Args::WindowFullscreen { .. } => Operation::WindowFullscreen,
            Args::WindowMove { .. } => Operation::WindowMove,
            Args::WindowFloating { .. } => Operation::WindowFloating,
            Args::WindowLayout { .. } => Operation::WindowLayout,
            Args::WindowScratchpad { .. } => Operation::WindowScratchpad,
            Args::ScratchpadShow { .. } => Operation::ScratchpadShow,
            Args::SessionLock => Operation::SessionLock,
            Args::WindowClose { .. } => Operation::WindowClose,
            Args::KeepAwakeSet(_) => Operation::KeepAwakeSet,
            Args::Audio { change, .. } => change.operation(),
            Args::Media { command, .. } => command.operation(),
            Args::AppLaunch { .. } => Operation::AppLaunch,
            Args::LauncherOpen => Operation::LauncherOpen,
            Args::InputText { .. } => Operation::InputText,
            Args::InputKey { .. } => Operation::InputKey,
        }
    }

    /// The request a client sends for these arguments.
    pub fn to_req(&self, id: u64) -> Req {
        Req {
            id,
            op: self.operation().as_str().to_owned(),
            args: self.encode().map(Value::Object),
        }
    }

    fn encode(&self) -> Option<JsonObject> {
        match self {
            Args::Ping | Args::SessionLock | Args::LauncherOpen => None,
            Args::StateSubscribe { topics } => Some(object_of([("topics", topics.clone().into())])),
            Args::IconGet { hashes } => {
                let hashes: Vec<Value> = hashes.iter().map(|hash| hash.encode_value()).collect();
                Some(object_of([("hashes", hashes.into())]))
            }
            Args::WorkspaceFocus(target) => Some(object_of([target.member()])),
            Args::WindowFocus { window_id }
            | Args::WindowScratchpad { window_id }
            | Args::ScratchpadShow { window_id }
            | Args::WindowClose { window_id } => {
                Some(object_of([("window_id", (*window_id).into())]))
            }
            Args::WindowFullscreen { window_id, enabled }
            | Args::WindowFloating { window_id, enabled } => Some(object_of([
                ("window_id", (*window_id).into()),
                ("enabled", (*enabled).into()),
            ])),
            Args::WindowMove { window_id, target } => Some(object_of([
                ("window_id", (*window_id).into()),
                target.member(),
            ])),
            Args::WindowLayout { window_id, layout } => Some(object_of([
                ("window_id", (*window_id).into()),
                ("layout", layout.as_str().into()),
            ])),
            Args::KeepAwakeSet(keep_awake) => Some(keep_awake.encode()),
            Args::Audio { device_id, change } => {
                let mut object = object_of([(DEVICE_ID, (*device_id).into())]);
                if let Some((name, value)) = change.member() {
                    object.insert(name.to_owned(), value);
                }
                Some(object)
            }
            Args::Media { player_id, .. } => Some(object_of([(PLAYER_ID, (*player_id).into())])),
            Args::AppLaunch { entry_id, number } => {
                let mut object = object_of([(ENTRY_ID, entry_id.as_str().into())]);
                json::insert_present(&mut object, "number", number.map(Value::from));
                Some(object)
            }
            Args::InputText { target, text } => {
                Some(object_of([target.member(), ("text", text.as_str().into())]))
            }
            Args::InputKey { target, press } => {
                let mut object = object_of([target.member()]);
                press.encode_into(&mut object);
                Some(object)
            }
        }
    }
}

impl WorkspaceTarget {
    fn member(self) -> (&'static str, Value) {
        match self {
            WorkspaceTarget::Id(id) => ("workspace_id", id.into()),
            WorkspaceTarget::Number(number) => ("number", number.into()),
        }
    }
}

impl KeepAwake {
    fn encode(self) -> JsonObject {
        let mut object = object_of([("enabled", matches!(self, KeepAwake::On { .. }).into())]);
        if let KeepAwake::On {
            duration_s: Some(duration_s),
        } = self
        {
            object.insert(DURATION_S.to_owned(), duration_s.into());
        }
        object
    }
}

#[cfg(test)]
mod tests;
