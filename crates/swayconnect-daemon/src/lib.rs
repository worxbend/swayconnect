//! Library half of `swayconnectd`. `pub mod` lines and re-exports only: no types, no logic.

#![forbid(unsafe_code)]

pub mod actions;
pub mod admin;
pub mod config;
pub mod daemon;
pub mod devices;
pub mod dispatch;
pub mod identity;
pub mod listener;
pub mod model;
pub mod pairing;
pub mod paths;
pub mod security;
pub mod session;
pub mod supervisor;
pub mod sway_handle;
pub mod throttle;
