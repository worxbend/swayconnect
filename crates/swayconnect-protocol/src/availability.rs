//! What a client shows for one operation (`docs/PROTOCOL.md` 17.3, client rule).

use crate::message::{Scope, Welcome};
use crate::ops::Operation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// `welcome.ops` lists it: the client may call it.
    Available,
    /// The device holds the scope, but the server is in read-only mode: show it disabled.
    ReadOnly,
    /// The device lacks this scope: show the control locked, with the scope name.
    Locked(Scope),
    /// The server does not implement it, or does not say: hide it.
    Unsupported,
}

impl Welcome {
    /// The client rule of 17.3. Only `ops` makes an operation callable; `known_ops` and
    /// `known_scopes` never grant anything. A server that knows an operation, grants its scope,
    /// is not read-only and still leaves it out of `ops` has no row in the table; the client
    /// cannot call it, so it is [`Availability::Unsupported`].
    pub fn availability(&self, operation: Operation) -> Availability {
        let name = operation.as_str();
        if self.ops.iter().any(|op| op == name) {
            return Availability::Available;
        }
        let is_known = self
            .known_ops
            .as_ref()
            .is_some_and(|known| known.iter().any(|op| op == name));
        if !is_known {
            return Availability::Unsupported;
        }
        match operation.required_scope() {
            Some(scope) if !self.scopes.contains(&scope) => Availability::Locked(scope),
            _ if self.read_only => Availability::ReadOnly,
            _ => Availability::Unsupported,
        }
    }
}
