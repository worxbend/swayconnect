//! The message layer after `welcome`: requests, responses, events (`docs/PROTOCOL.md` 6 to 12).

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use swayconnect_protocol::limits::{
    CLIENT_KEEPALIVE_INTERVAL_MS, CLIENT_REQUEST_TIMEOUT_MS, MAX_PENDING_REQUESTS,
};
use swayconnect_protocol::ops::{
    Args, EVENT_PAIRING_CONFIRMED, EVENT_STATE_CHANGED, TopicState, decode_subscribe_result,
};
use swayconnect_protocol::{DecodeError, Evt, JsonObject, Message, Res, Welcome};
use tokio::time::{Instant, sleep_until};

use super::channel::Channel;
use super::error::ClientError;

const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(CLIENT_KEEPALIVE_INTERVAL_MS);
const KEEPALIVE_ANSWER_TIMEOUT: Duration = Duration::from_millis(CLIENT_REQUEST_TIMEOUT_MS);

/// What the server sends without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A newer state of a subscribed topic. Its text is as the server sent it.
    State(TopicState),
    PairingConfirmed,
}

/// A request that was sent and whose response was not collected yet.
#[derive(Debug)]
pub struct PendingRequest {
    id: u64,
}

enum Slot {
    /// Holds the topics that count as subscribed once the response says `ok`.
    Waiting(Vec<String>),
    Arrived(Res),
    /// A `ping` of the keepalive timer. Its response only ends the wait for it.
    Keepalive,
}

/// One authenticated connection. Dropping it closes the connection without `close_notify`.
pub struct Session {
    channel: Channel,
    welcome: Welcome,
    next_id: u64,
    requests: HashMap<u64, Slot>,
    events: VecDeque<Event>,
    subscribed_topics: HashSet<String>,
    /// Highest applied `rev` per topic, on this connection only (spec 8.2 rules 3 and 6).
    applied_revs: HashMap<String, u64>,
    last_sent: Instant,
    /// `Some` while a keepalive `ping` has no response: when the connection counts as dead.
    keepalive_answer_due: Option<Instant>,
}

impl Session {
    pub(super) fn new(mut channel: Channel, welcome: Welcome) -> Session {
        channel.apply_limits(&welcome.limits);
        Session {
            channel,
            welcome,
            next_id: 1,
            requests: HashMap::new(),
            events: VecDeque::new(),
            subscribed_topics: HashSet::new(),
            applied_revs: HashMap::new(),
            last_sent: Instant::now(),
            keepalive_answer_due: None,
        }
    }

    /// The verified `welcome`. Its strings are server text.
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Sends one request and waits for its result object.
    pub async fn request(&mut self, args: &Args) -> Result<JsonObject, ClientError> {
        let pending = self.send(args).await?;
        self.response(pending).await
    }

    /// Sends one request without waiting, so that several can be in flight (spec 12 rule 3).
    pub async fn send(&mut self, args: &Args) -> Result<PendingRequest, ClientError> {
        let operation = args.operation().as_str();
        if !self.welcome.ops.iter().any(|offered| offered == operation) {
            return Err(ClientError::NotOffered(operation));
        }
        if self.requests.len() >= MAX_PENDING_REQUESTS {
            return Err(ClientError::TooManyPending);
        }
        let subscribes_to = match args {
            Args::StateSubscribe { topics } => topics.clone(),
            _other_operation => Vec::new(),
        };
        let id = self.send_req(args, Slot::Waiting(subscribes_to)).await?;
        Ok(PendingRequest { id })
    }

    /// Waits for the response with the id of `pending`, whatever arrives before it.
    pub async fn response(&mut self, pending: PendingRequest) -> Result<JsonObject, ClientError> {
        while !matches!(self.requests.get(&pending.id), Some(Slot::Arrived(_))) {
            self.receive().await?;
        }
        let Some(Slot::Arrived(res)) = self.requests.remove(&pending.id) else {
            return Err(ClientError::Protocol("a response went missing"));
        };
        match (res.result, res.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(ClientError::Failed(error)),
            _ => Err(ClientError::Protocol("`res` needs one of result and error")),
        }
    }

    /// `state.subscribe`. Returns the states that are newer than the applied ones; a state
    /// with a lower or equal `rev` is left out (spec 8.2 rule 4).
    pub async fn subscribe(&mut self, topics: &[&str]) -> Result<Vec<TopicState>, ClientError> {
        let topics = topics.iter().map(|&topic| topic.to_owned()).collect();
        let result = self.request(&Args::StateSubscribe { topics }).await?;
        let mut states = decode_subscribe_result(&result)
            .map_err(|_| ClientError::Protocol("invalid `state.subscribe` result"))?;
        states.retain(|state| self.apply_rev(state));
        Ok(states)
    }

    /// The next event. Sends `ping` when nothing was sent for 30 seconds (spec 11 rule 3).
    /// A `ping` with no response after 10 seconds means a dead connection (rule 4).
    pub async fn next_event(&mut self) -> Result<Event, ClientError> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Ok(event);
            }
            // The answer is due before the next `ping` would be.
            let keepalive_at = self.last_sent + KEEPALIVE_INTERVAL;
            let timer = self.keepalive_answer_due.unwrap_or(keepalive_at);
            tokio::select! {
                received = self.receive() => received?,
                () = sleep_until(timer) => self.keep_alive().await?,
            }
        }
    }

    /// The keepalive timer fired: gives up on an unanswered `ping`, or sends one.
    async fn keep_alive(&mut self) -> Result<(), ClientError> {
        if self.keepalive_answer_due.is_some() {
            return Err(ClientError::NoAnswer);
        }
        self.send_req(&Args::Ping, Slot::Keepalive).await?;
        self.keepalive_answer_due = Some(self.last_sent + KEEPALIVE_ANSWER_TIMEOUT);
        Ok(())
    }

    /// For negative tests: sends `payload` as one frame. Nothing checks it and no response is
    /// matched to it; read the answers with [`Session::receive_raw_frame`].
    pub async fn send_raw_frame(&mut self, payload: &[u8]) -> Result<(), ClientError> {
        self.channel.write_raw_frame(payload).await?;
        self.last_sent = Instant::now();
        Ok(())
    }

    /// For negative tests: the next frame as the server sent it, of any type.
    pub async fn receive_raw_frame(&mut self) -> Result<JsonObject, ClientError> {
        self.channel.read_frame().await
    }

    /// Closes with TLS `close_notify` (spec 11 rule 6).
    pub async fn close(self) -> Result<(), ClientError> {
        self.channel.close().await
    }

    async fn send_req(&mut self, args: &Args, slot: Slot) -> Result<u64, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        self.channel
            .write_message(&Message::Req(args.to_req(id)))
            .await?;
        self.last_sent = Instant::now();
        self.requests.insert(id, slot);
        Ok(id)
    }

    /// Reads one frame and files it. Cancel safe.
    async fn receive(&mut self) -> Result<(), ClientError> {
        let frame = self.channel.read_frame().await?;
        match Message::decode(&frame) {
            Ok(Message::Res(res)) => self.file_response(res),
            Ok(Message::Evt(evt)) => self.file_event(&evt),
            Ok(Message::Bye(bye)) => Err(ClientError::Bye {
                code: bye.code,
                message: bye.message,
            }),
            Ok(_) => Err(ClientError::Protocol("a message type a server never sends")),
            Err(DecodeError::UnknownType) => Ok(()),
            Err(_) => Err(ClientError::Protocol("a frame that is not a valid message")),
        }
    }

    fn file_response(&mut self, res: Res) -> Result<(), ClientError> {
        match self.requests.remove(&res.id) {
            Some(Slot::Waiting(topics)) => {
                if res.ok {
                    self.subscribed_topics.extend(topics);
                }
                self.requests.insert(res.id, Slot::Arrived(res));
            }
            // The frame reset the server's idle timer; its answer shows a live connection.
            Some(Slot::Keepalive) => self.keepalive_answer_due = None,
            Some(Slot::Arrived(_)) | None => {
                return Err(ClientError::Protocol("a response to no open request"));
            }
        }
        Ok(())
    }

    fn file_event(&mut self, evt: &Evt) -> Result<(), ClientError> {
        match evt.event.as_str() {
            EVENT_STATE_CHANGED => {
                let state = TopicState::decode(&evt.data)
                    .map_err(|_| ClientError::Protocol("invalid `state.changed` event"))?;
                if self.subscribed_topics.contains(&state.topic) && self.apply_rev(&state) {
                    self.events.push_back(Event::State(state));
                }
            }
            EVENT_PAIRING_CONFIRMED => self.events.push_back(Event::PairingConfirmed),
            _unknown_event => {}
        }
        Ok(())
    }

    /// Records the `rev` of `state` when it is newer than the applied one, and says whether
    /// the state is to be applied.
    fn apply_rev(&mut self, state: &TopicState) -> bool {
        let is_newer = self
            .applied_revs
            .get(&state.topic)
            .is_none_or(|applied| state.rev > *applied);
        if is_newer {
            self.applied_revs.insert(state.topic.clone(), state.rev);
        }
        is_newer
    }
}
