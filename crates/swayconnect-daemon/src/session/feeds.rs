//! The topics a session follows (`docs/PROTOCOL.md` 8, 17.4, 17.8 to 17.11): the watch of
//! each, its state as one device may see it, and its revisions.
//!
//! `desktop` carries the supervisor's revision. Every other topic counts its own revisions
//! on the connection (spec 8 rules 2 and 10): `session` also changes with the input and
//! launcher holders, and `audio` has no revision of its own.

use std::collections::BTreeSet;
use std::sync::Arc;

use swayconnect_protocol::ops::TopicState;
use swayconnect_protocol::{DeviceId, Held, Holder, Scope};
use tokio::sync::watch;
use tokio::sync::watch::error::RecvError;
use tokio::time::Instant;

use crate::apps::{OpenLauncher, PublishedApps, held_for};
use crate::audio::Snapshot;
use crate::daemon::services::Services;
use crate::dispatch::Topic;
use crate::media::PublishedMedia;
use crate::session_topic::{PublishedSession, SessionInterest, SessionTopic};
use crate::sway_handle::{DesktopInterest, Published, SwayHandle};

/// Where the topics come from.
pub(super) struct Sources {
    pub(super) desktop: watch::Receiver<Arc<Published>>,
    pub(super) sway: Arc<SwayHandle>,
    pub(super) session_topic: Arc<SessionTopic>,
    pub(super) services: Services,
}

/// Who looks: the per-device members and the titles depend on it.
pub(super) struct Viewer<'a> {
    pub(super) device: &'a DeviceId,
    pub(super) scopes: &'a BTreeSet<Scope>,
}

impl Viewer<'_> {
    fn sees_titles(&self) -> bool {
        self.scopes.contains(&Scope::StateTitles)
    }
}

/// The topics one session subscribed to. A topic is followed from its first subscription
/// until the connection ends.
pub(super) struct Feeds {
    sources: Sources,
    desktop: Option<DesktopFeed>,
    session: Option<SessionFeed>,
    audio: Option<Feed<Arc<Snapshot>>>,
    media: Option<Feed<Arc<PublishedMedia>>>,
    apps: Option<Feed<Arc<PublishedApps>>>,
}

/// While it lives, the supervisor polls sway.
struct DesktopFeed {
    _interest: DesktopInterest,
    state: watch::Receiver<Arc<Published>>,
}

/// A topic with this connection's revision counter.
struct Feed<T> {
    state: watch::Receiver<T>,
    rev: u64,
}

impl<T: Clone> Feed<T> {
    fn new(state: watch::Receiver<T>) -> Feed<T> {
        Feed { state, rev: 0 }
    }

    /// The newest value, marked as seen, with the next revision of this connection.
    fn next(&mut self) -> (u64, T) {
        self.rev += 1;
        (self.rev, self.state.borrow_and_update().clone())
    }
}

/// Topic `session`: the shared facts, the input holder and the launcher holder. While it
/// lives, lock state is re-checked.
struct SessionFeed {
    _interest: SessionInterest,
    facts: Feed<Arc<PublishedSession>>,
    input: watch::Receiver<Option<DeviceId>>,
    launcher: watch::Receiver<Option<OpenLauncher>>,
}

impl Feeds {
    pub(super) fn new(sources: Sources) -> Feeds {
        Feeds {
            sources,
            desktop: None,
            session: None,
            audio: None,
            media: None,
            apps: None,
        }
    }

    /// Starts following `topic` unless it is followed already, and returns its state.
    pub(super) fn subscribe(&mut self, topic: Topic, viewer: &Viewer<'_>) -> TopicState {
        let sources = &self.sources;
        let services = &sources.services;
        match topic {
            Topic::Desktop => {
                let feed = self.desktop.get_or_insert_with(|| DesktopFeed {
                    _interest: sources.sway.watch_desktop(),
                    state: sources.desktop.clone(),
                });
                desktop_state(feed, viewer)
            }
            Topic::Session => {
                let feed = self.session.get_or_insert_with(|| SessionFeed {
                    _interest: sources.session_topic.watch_session(),
                    facts: Feed::new(sources.session_topic.state()),
                    input: services.input.view(),
                    launcher: services.launcher.view(),
                });
                session_state(feed, viewer)
            }
            Topic::Audio => audio_state(
                self.audio
                    .get_or_insert_with(|| Feed::new(services.audio.snapshot())),
            ),
            Topic::Media => {
                let feed = self
                    .media
                    .get_or_insert_with(|| Feed::new(services.media.state()));
                media_state(feed, viewer)
            }
            Topic::Apps => apps_state(
                self.apps
                    .get_or_insert_with(|| Feed::new(services.apps.subscribe())),
            ),
        }
    }

    /// The newest state of `topic`, marked as seen. `None` when it is not followed.
    pub(super) fn state(&mut self, topic: Topic, viewer: &Viewer<'_>) -> Option<TopicState> {
        match topic {
            Topic::Desktop => self
                .desktop
                .as_mut()
                .map(|feed| desktop_state(feed, viewer)),
            Topic::Session => self
                .session
                .as_mut()
                .map(|feed| session_state(feed, viewer)),
            Topic::Audio => self.audio.as_mut().map(audio_state),
            Topic::Media => self.media.as_mut().map(|feed| media_state(feed, viewer)),
            Topic::Apps => self.apps.as_mut().map(apps_state),
        }
    }

    /// Resolves with a followed topic that has a newer state; never without one. `Err`: its
    /// source has ended, so the daemon is stopping. Cancel safe.
    pub(super) async fn changed(&mut self) -> Result<Topic, RecvError> {
        tokio::select! {
            changed = desktop_changed(self.desktop.as_mut()) => changed.map(|()| Topic::Desktop),
            changed = session_changed(self.session.as_mut()) => changed.map(|()| Topic::Session),
            changed = feed_changed(self.audio.as_mut()) => changed.map(|()| Topic::Audio),
            changed = feed_changed(self.media.as_mut()) => changed.map(|()| Topic::Media),
            changed = feed_changed(self.apps.as_mut()) => changed.map(|()| Topic::Apps),
        }
    }
}

async fn desktop_changed(feed: Option<&mut DesktopFeed>) -> Result<(), RecvError> {
    match feed {
        Some(feed) => feed.state.changed().await,
        None => std::future::pending().await,
    }
}

async fn session_changed(feed: Option<&mut SessionFeed>) -> Result<(), RecvError> {
    let Some(feed) = feed else {
        return std::future::pending().await;
    };
    tokio::select! {
        changed = feed.facts.state.changed() => changed,
        changed = feed.input.changed() => changed,
        changed = feed.launcher.changed() => changed,
    }
}

async fn feed_changed<T>(feed: Option<&mut Feed<T>>) -> Result<(), RecvError> {
    match feed {
        Some(feed) => feed.state.changed().await,
        None => std::future::pending().await,
    }
}

/// The newest snapshot, without titles unless the viewer holds `state.titles`.
fn desktop_state(feed: &mut DesktopFeed, viewer: &Viewer<'_>) -> TopicState {
    let published = Arc::clone(&feed.state.borrow_and_update());
    let data = if viewer.sees_titles() {
        published.desktop.encode()
    } else {
        let mut desktop = published.desktop.clone();
        for window in &mut desktop.windows {
            window.title = None;
        }
        for window in desktop.scratchpad.iter_mut().flatten() {
            window.title = None;
        }
        desktop.encode()
    };
    TopicState {
        topic: Topic::Desktop.as_str().to_owned(),
        rev: published.rev,
        data,
    }
}

/// Topic `session` for the viewer. `remaining_ms` counts from now.
fn session_state(feed: &mut SessionFeed, viewer: &Viewer<'_>) -> TopicState {
    let (rev, published) = feed.facts.next();
    let mut session = published.facts.state_for(viewer.device, Instant::now());
    let input_holder = feed.input.borrow_and_update().clone();
    session.input = Some(Held {
        running: input_holder.is_some(),
        holder: input_holder.map(|holder| holder_for(&holder, viewer.device)),
    });
    session.launcher = Some(held_for(
        feed.launcher.borrow_and_update().as_ref(),
        viewer.device,
    ));
    TopicState {
        topic: Topic::Session.as_str().to_owned(),
        rev,
        data: session.encode(),
    }
}

fn holder_for(holder: &DeviceId, viewer: &DeviceId) -> Holder {
    if holder == viewer {
        Holder::ThisDevice
    } else {
        Holder::OtherDevice
    }
}

fn audio_state(feed: &mut Feed<Arc<Snapshot>>) -> TopicState {
    let (rev, snapshot) = feed.next();
    TopicState {
        topic: Topic::Audio.as_str().to_owned(),
        rev,
        data: snapshot.audio().encode(),
    }
}

/// Track texts are withheld unless the viewer holds `state.titles`.
fn media_state(feed: &mut Feed<Arc<PublishedMedia>>, viewer: &Viewer<'_>) -> TopicState {
    let (rev, published) = feed.next();
    TopicState {
        topic: Topic::Media.as_str().to_owned(),
        rev,
        data: published.facts.state_for(viewer.sees_titles()).encode(),
    }
}

fn apps_state(feed: &mut Feed<Arc<PublishedApps>>) -> TopicState {
    let (rev, published) = feed.next();
    TopicState {
        topic: Topic::Apps.as_str().to_owned(),
        rev,
        data: published.state.encode(),
    }
}
