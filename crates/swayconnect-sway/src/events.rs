//! The event connection: subscribe once, then only read (`docs/DESIGN.md` 4.3 rule 3).

use std::path::Path;

use crate::connection::{Connection, Error, parse};
use crate::frame::MessageType;
use crate::types::Outcome;

const SUBSCRIPTION: &[u8] = br#"["workspace","window","output","shutdown"]"#;

/// A trigger from sway. The payload is not kept: any event means "state may have changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Workspace,
    Output,
    Window,
    /// sway is exiting. The socket closes next.
    Shutdown,
    /// An event type nobody subscribed to, with its frame type.
    Other(u32),
}

impl Event {
    pub(crate) fn from_code(code: u32) -> Event {
        match code {
            0x8000_0000 => Event::Workspace,
            0x8000_0001 => Event::Output,
            0x8000_0003 => Event::Window,
            0x8000_0006 => Event::Shutdown,
            other => Event::Other(other),
        }
    }
}

/// A subscribed connection that only reads.
#[derive(Debug)]
pub struct EventStream {
    connection: Connection,
}

impl EventStream {
    /// Connects to `socket` and subscribes to workspace, window, output and shutdown events.
    pub async fn open(socket: &Path) -> Result<EventStream, Error> {
        let connection = Connection::connect(socket).await?;
        let reply = connection
            .request(MessageType::Subscribe, SUBSCRIPTION)
            .await?;
        if !parse::<Outcome>(&reply)?.success {
            return Err(Error::SubscribeRefused);
        }
        Ok(EventStream { connection })
    }

    /// The next event. An error means the connection is gone and must be rebuilt.
    ///
    /// When the caller falls far behind, events are dropped; the ones already queued still
    /// tell it to refresh.
    pub async fn next(&mut self) -> Result<Event, Error> {
        self.connection.next_event().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_socket::TestSocket;

    #[test]
    fn frame_types_map_to_events() {
        assert_eq!(Event::from_code(0x8000_0000), Event::Workspace);
        assert_eq!(Event::from_code(0x8000_0001), Event::Output);
        assert_eq!(Event::from_code(0x8000_0003), Event::Window);
        assert_eq!(Event::from_code(0x8000_0006), Event::Shutdown);
        assert_eq!(Event::from_code(0x8000_0007), Event::Other(0x8000_0007));
    }

    #[tokio::test]
    async fn open_subscribes_to_the_four_event_types_and_then_reads_events() {
        let socket = TestSocket::bind();
        let sway = async {
            let mut peer = socket.accept().await;
            let request = peer.read_request().await;
            peer.send(2, br#"{"success":true}"#).await;
            peer.send(0x8000_0003, br#"{"change":"focus"}"#).await;
            peer.send(0x8000_0006, br#"{"change":"exit"}"#).await;
            (request, peer)
        };
        let client = async {
            let mut events = EventStream::open(socket.path()).await.expect("subscribe");
            let first = events.next().await.expect("first event");
            let second = events.next().await.expect("second event");
            (first, second, events)
        };
        let ((request, peer), (first, second, mut events)) = tokio::join!(sway, client);

        assert_eq!(request.0, 2);
        assert_eq!(request.1, br#"["workspace","window","output","shutdown"]"#);
        assert_eq!((first, second), (Event::Window, Event::Shutdown));

        drop(peer);
        let error = events.next().await.expect_err("sway closed the socket");
        assert!(matches!(error, Error::Io(_)), "{error}");
        let error = events.next().await.expect_err("still closed");
        assert!(matches!(error, Error::Closed), "{error}");
    }

    #[tokio::test]
    async fn a_refused_subscription_is_an_error() {
        let socket = TestSocket::bind();
        let sway = async {
            let mut peer = socket.accept().await;
            peer.read_request().await;
            peer.send(2, br#"{"success":false}"#).await;
            peer
        };
        let (_peer, opened) = tokio::join!(sway, EventStream::open(socket.path()));
        let error = opened.expect_err("refused");
        assert!(matches!(error, Error::SubscribeRefused), "{error}");
    }
}
