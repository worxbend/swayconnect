//! [`MediaService`] against [`FakeMedia`] with paused time, and the step-8 part of the
//! `requests` of `protocol/vectors/v1/ext/media.json`.

use serde_json::Value;
use swayconnect_protocol::ops::{Args, Operation};
use swayconnect_protocol::{ErrorCode, Message, Player, PlayerStatus};

use super::*;
use crate::media::PlayerReport;
use crate::media::fake::{FakeAnswer, FakeMedia};

const MEDIA_VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../protocol/vectors/v1/ext/media.json"
);

fn report(bus_suffix: &str, status: &str, can_go_next: bool) -> PlayerReport {
    PlayerReport {
        bus_name: format!("org.mpris.MediaPlayer2.{bus_suffix}"),
        identity: Some(bus_suffix.to_owned()),
        desktop_entry: Some(bus_suffix.to_owned()),
        status: status.to_owned(),
        title: Some("Song".to_owned()),
        artist: Some("Band".to_owned()),
        album: None,
        can_play: true,
        can_pause: true,
        can_go_next,
        can_go_previous: false,
    }
}

struct Running {
    fake: Arc<FakeMedia>,
    service: MediaService,
    _stop: watch::Sender<bool>,
}

fn start(players: Vec<PlayerReport>) -> Running {
    let fake = Arc::new(FakeMedia::default());
    fake.set_players(Ok(players));
    let (stop, shutdown) = watch::channel(false);
    let backend: Arc<dyn MediaBackend> = fake.clone();
    let service = MediaService::spawn(backend, None, shutdown);
    Running {
        fake,
        service,
        _stop: stop,
    }
}

async fn published_with(service: &MediaService, count: usize) -> Arc<PublishedMedia> {
    let mut state = service.state();
    let published = state
        .wait_for(|published| published.facts.players.len() == count)
        .await
        .expect("the topic task runs");
    Arc::clone(&published)
}

#[tokio::test(start_paused = true)]
async fn the_players_of_the_backend_are_published_with_titles_only_for_state_titles() {
    let running = start(vec![report("spotify", "Playing", true)]);
    let published = published_with(&running.service, 1).await;
    let player: &Player = &published.facts.state_for(true).players[0];
    assert_eq!(player.name, "spotify");
    assert_eq!(player.status, PlayerStatus::Playing);
    assert_eq!(player.title.as_deref(), Some("Song"));
    assert_eq!(published.facts.active_player_id, Some(player.id));
    let without_titles = published.facts.state_for(false);
    assert_eq!(without_titles.players[0].title, None);
    assert_eq!(without_titles.players[0].artist, None);
}

#[tokio::test(start_paused = true)]
async fn a_change_is_republished_with_a_higher_rev_and_the_same_ids() {
    let running = start(vec![report("a", "Paused", true)]);
    let first = published_with(&running.service, 1).await;
    let id_a = first.facts.players[0].player.id;

    running.fake.set_players(Ok(vec![
        report("a", "Paused", true),
        report("b", "Paused", true),
    ]));
    let second = published_with(&running.service, 2).await;
    assert!(second.rev > first.rev);
    assert_eq!(second.facts.players[0].player.id, id_a);
    assert_ne!(second.facts.players[1].player.id, id_a);
}

#[test]
fn equal_facts_publish_nothing_and_a_change_raises_rev_by_one() {
    let first = PublishedMedia {
        rev: 1,
        facts: MediaFacts::default(),
    };
    let (publisher, state) = watch::channel(Arc::new(first));
    publish(&publisher, MediaFacts::default());
    assert_eq!(state.borrow().rev, 1);
    assert!(!state.has_changed().expect("open"));

    let changed = MediaFacts {
        truncated: true,
        ..MediaFacts::default()
    };
    publish(&publisher, changed.clone());
    assert_eq!(state.borrow().rev, 2);
    assert_eq!(state.borrow().facts, changed);
}

#[tokio::test(start_paused = true)]
async fn a_player_keeps_its_id_across_a_failed_read() {
    let running = start(vec![report("a", "Paused", true)]);
    let first = published_with(&running.service, 1).await;
    let id = first.facts.players[0].player.id;
    running
        .fake
        .set_players(Err(BackendError::Unavailable("bus down".into())));
    published_with(&running.service, 0).await;
    running
        .fake
        .set_players(Ok(vec![report("a", "Paused", true)]));
    let back = published_with(&running.service, 1).await;
    assert_eq!(back.facts.players[0].player.id, id);
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_publishes_no_players_and_their_ids_are_not_found() {
    let running = start(vec![report("a", "Paused", true)]);
    let first = published_with(&running.service, 1).await;
    let id = first.facts.players[0].player.id;
    running
        .fake
        .set_players(Err(BackendError::Unavailable("bus down".into())));
    published_with(&running.service, 0).await;
    assert_eq!(
        running.service.command(id, MediaCommand::PlayPause).await,
        Err(MediaError::NotFound)
    );
    assert!(running.fake.sent().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_command_goes_to_the_bus_name_of_its_player() {
    let running = start(vec![
        report("a", "Paused", false),
        report("b", "Paused", true),
    ]);
    let published = published_with(&running.service, 2).await;
    let id_b = published.facts.players[1].player.id;
    running
        .service
        .command(id_b, MediaCommand::Next)
        .await
        .expect("sent");
    assert_eq!(
        running.fake.sent(),
        [("org.mpris.MediaPlayer2.b".to_owned(), MediaCommand::Next)]
    );
}

#[tokio::test(start_paused = true)]
async fn a_hanging_player_is_a_timeout_and_a_failing_one_is_unavailable() {
    let running = start(vec![report("a", "Paused", true)]);
    let id = published_with(&running.service, 1).await.facts.players[0]
        .player
        .id;
    running.fake.set_answer(FakeAnswer::Hangs);
    let error = running.service.command(id, MediaCommand::PlayPause).await;
    assert_eq!(error.map_err(|error| error.code()), Err(ErrorCode::Timeout));
    running.fake.set_answer(FakeAnswer::Fails);
    let error = running.service.command(id, MediaCommand::PlayPause).await;
    assert_eq!(
        error.map_err(|error| error.code()),
        Err(ErrorCode::BackendUnavailable)
    );
}

#[tokio::test(start_paused = true)]
async fn the_task_ends_on_shutdown() {
    let fake = Arc::new(FakeMedia::default());
    let (stop, shutdown) = watch::channel(false);
    let service = MediaService::spawn(fake, None, shutdown);
    let mut state = service.state();
    stop.send(true).expect("the task listens");
    // The publisher is dropped when the task returns.
    while state.changed().await.is_ok() {}
}

/// Codes this service decides; the others belong to steps 2 to 6 (dispatch).
const STEP_8_CODES: [&str; 4] = [
    "not_found",
    "unsupported_target",
    "backend_unavailable",
    "timeout",
];

fn server_state_reports(state: &Value) -> Vec<PlayerReport> {
    let buses = state["buses"].as_object().expect("buses");
    state["state"]["players"]
        .as_array()
        .expect("players")
        .iter()
        .map(|player| {
            let id = player["id"].as_u64().expect("id").to_string();
            let status = match player["status"].as_str() {
                Some("playing") => "Playing",
                Some("paused") => "Paused",
                _ => "Stopped",
            };
            let text = |key: &str| player[key].as_str().map(str::to_owned);
            let flag = |key: &str| player[key].as_bool().expect("flag");
            PlayerReport {
                bus_name: buses[&id].as_str().expect("bus").to_owned(),
                identity: text("name"),
                desktop_entry: text("app_id"),
                status: status.to_owned(),
                title: text("title"),
                artist: text("artist"),
                album: text("album"),
                can_play: flag("can_play"),
                can_pause: flag("can_pause"),
                can_go_next: flag("can_next"),
                can_go_previous: flag("can_previous"),
            }
        })
        .collect()
}

fn media_args(payload: &str) -> Option<(u64, MediaCommand)> {
    let object: serde_json::Map<String, Value> = serde_json::from_str(payload).ok()?;
    let Ok(Message::Req(req)) = Message::decode(&object) else {
        return None;
    };
    match Operation::from_wire(&req.op)?.decode_args(req.args.as_ref()) {
        Ok(Args::Media { player_id, command }) => Some((player_id, command)),
        _ => None,
    }
}

#[tokio::test(start_paused = true)]
async fn the_step_8_request_vectors_are_answered_as_the_spec_says() {
    let text = std::fs::read_to_string(MEDIA_VECTORS).expect("media vectors");
    let file: Value = serde_json::from_str(&text).expect("JSON");
    let mut replayed = 0;
    for case in file["requests"].as_array().expect("requests") {
        let name = case["name"].as_str().expect("name");
        let expected = case["expect"]["error"].as_str();
        let allowed = case["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .any(|scope| scope == "media");
        let decided_here = expected.is_none_or(|code| STEP_8_CODES.contains(&code));
        let Some((player_id, command)) = media_args(case["payload"].as_str().expect("payload"))
        else {
            continue;
        };
        if !allowed || case["read_only"] == true || !decided_here {
            continue;
        }
        let state = &file["server_states"][case["media"].as_str().expect("media state")];
        let reports = server_state_reports(state);
        let running = start(reports.clone());
        published_with(&running.service, reports.len()).await;
        running.fake.set_answer(match case["backend"].as_str() {
            Some("fails") => FakeAnswer::Fails,
            Some("hangs") => FakeAnswer::Hangs,
            _ => FakeAnswer::Ok,
        });

        let result = running.service.command(player_id, command).await;
        let code = result.err().map(|error| error.code().as_str().to_owned());
        assert_eq!(code.as_deref(), expected, "{name}: answer");
        let calls: Vec<Value> = running
            .fake
            .sent()
            .into_iter()
            .map(|(bus, command)| {
                serde_json::json!([bus, crate::media::mpris::method_name(command)])
            })
            .collect();
        assert_eq!(Value::Array(calls), case["mpris"], "{name}: MPRIS calls");
        replayed += 1;
    }
    assert!(replayed >= 13, "replayed {replayed} cases");
}
