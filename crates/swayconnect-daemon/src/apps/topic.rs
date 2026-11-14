//! The state object of topic `apps` and its cut rule (`docs/PROTOCOL.md` 17.11, 17.1 rule 9).

use swayconnect_protocol::limits::{MAX_APPS, MAX_TOPIC_STATE_BYTES};
use swayconnect_protocol::{App, Apps, IconHash};

use super::index::IndexedApp;

/// The state for `apps` (sorted already) with `recent` kept to listed ids, then cut.
pub fn build_state(
    apps: &[IndexedApp],
    recent: &[String],
    icon_of: impl Fn(&IndexedApp) -> Option<IconHash>,
) -> Apps {
    let apps: Vec<App> = apps
        .iter()
        .map(|app| App {
            entry_id: app.entry_id.clone(),
            name: app.name.clone(),
            generic_name: app.generic_name.clone(),
            keywords: app.keywords.clone(),
            icon: icon_of(app),
            terminal: app.terminal,
            risky: app.risky,
        })
        .collect();
    let mut state = Apps {
        apps,
        recent: recent.to_vec(),
        truncated: false,
    };
    keep_listed_recent(&mut state);
    cut(state)
}

/// Drops apps beyond 512; then, while the encoded state is above 262144 bytes, clears every
/// `keywords`, then every `generic_name`, then drops apps from the end. A cut state carries
/// `truncated: true`, counted in its size.
pub fn cut(mut state: Apps) -> Apps {
    if state.apps.len() > MAX_APPS {
        state.apps.truncate(MAX_APPS);
        state.truncated = true;
        keep_listed_recent(&mut state);
    }
    if fits(&state) {
        return state;
    }
    state.truncated = true;
    state.apps.iter_mut().for_each(|app| app.keywords.clear());
    if fits(&state) {
        return state;
    }
    state
        .apps
        .iter_mut()
        .for_each(|app| app.generic_name = None);
    drop_from_end(state)
}

/// One app at a time; each drop subtracts that app's bytes and its comma instead of
/// encoding the whole state again, except when the app was in `recent`.
fn drop_from_end(mut state: Apps) -> Apps {
    let mut len = encoded_len(&state);
    while len > MAX_TOPIC_STATE_BYTES {
        let Some(app) = state.apps.pop() else {
            break;
        };
        if state.recent.contains(&app.entry_id) {
            state.recent.retain(|id| *id != app.entry_id);
            len = encoded_len(&state);
        } else {
            let comma = usize::from(!state.apps.is_empty());
            len = len.saturating_sub(app_len(app) + comma);
        }
    }
    state
}

/// The bytes one app takes inside the `apps` array, without a separating comma.
fn app_len(app: App) -> usize {
    let empty = Apps {
        apps: Vec::new(),
        recent: Vec::new(),
        truncated: true,
    };
    let empty_len = encoded_len(&empty);
    let alone = Apps {
        apps: vec![app],
        ..empty
    };
    encoded_len(&alone).saturating_sub(empty_len)
}

fn keep_listed_recent(state: &mut Apps) {
    let Apps { apps, recent, .. } = state;
    recent.retain(|id| apps.iter().any(|app| app.entry_id == *id));
}

fn fits(state: &Apps) -> bool {
    encoded_len(state) <= MAX_TOPIC_STATE_BYTES
}

/// Compact JSON, UTF-8 not escaped: the size 17.1 rule 9 measures.
pub fn encoded_len(state: &Apps) -> usize {
    // Encoding a JSON object cannot fail; if it did, the state would count as too large.
    serde_json::to_vec(&state.encode()).map_or(usize::MAX, |encoded| encoded.len())
}

#[cfg(test)]
#[path = "topic_tests.rs"]
mod tests;
