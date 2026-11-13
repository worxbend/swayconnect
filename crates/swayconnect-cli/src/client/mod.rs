//! Client library: pinned TLS, handshake, requests, events. Written by slice S5.
//!
//! No timeouts live here except the keepalive: a caller that wants a deadline wraps a call in
//! `tokio::time::timeout` and drops the [`Session`], because a cancelled call leaves the
//! connection in an unknown state (`docs/PROTOCOL.md` 11 rule 4).

mod channel;
mod error;
mod handshake;
mod profile;
mod session;
mod tls;

pub use error::ClientError;
pub use handshake::{Connector, NonceSource, PendingPairing};
pub use profile::{Profile, ProfileError};
pub use session::{Event, PendingRequest, Session};
