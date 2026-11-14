//! Library half of `swayconnectd`. `pub mod` lines and re-exports only: no types, no logic.

#![forbid(unsafe_code)]

pub mod actions;
pub mod admin;
pub mod apps;
pub mod audio;
pub mod audit;
pub mod config;
pub mod daemon;
pub mod devices;
pub mod dispatch;
pub mod icons;
pub mod identity;
pub mod input;
pub mod keep_awake;
pub mod layout;
pub mod listener;
pub mod lockstate;
pub mod media;
pub mod model;
pub mod notify;
pub mod outputs;
pub mod pairing;
pub mod paths;
pub mod proc;
pub mod security;
pub mod session;
pub mod session_topic;
pub mod supervisor;
pub mod sway_handle;
pub mod throttle;
