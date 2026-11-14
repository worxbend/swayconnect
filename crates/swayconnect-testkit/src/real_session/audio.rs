//! The sound part of the sealed session: one PipeWire that also serves the PulseAudio
//! protocol, WirePlumber with no hardware monitor, two null sinks and a null source.

use std::time::Instant;

use super::child::{Lines, Watcher};
use super::{
    PACTL, PIPEWIRE, PIPEWIRE_CONF, READY_TIMEOUT, RealSession, SEALED_SINKS, SEALED_SOURCE,
    SUBSCRIPTION_PROBE, WIREPLUMBER,
};

/// WirePlumber reads its configs only here: no drop-in of `/etc/wireplumber` or
/// `~/.config` can add a hardware monitor.
const WIREPLUMBER_CONFIG_DIR: &str = "/usr/share/wireplumber";

impl RealSession {
    /// WirePlumber runs with its `policy` profile: default nodes and links, and no
    /// hardware monitor at all. Without it the null sinks exist, but
    /// `pactl get-default-sink` answers `@DEFAULT_SINK@` and `set-default-sink` is lost.
    pub(super) fn start_audio(&mut self) {
        let config = self.write_etc("sealed-pipewire.conf", PIPEWIRE_CONF);
        // PipeWire reads drop-ins of its configs only from here, never from /etc/pipewire.
        let config_dir = self.root.path().join("etc");
        let mut command = self.command(PIPEWIRE);
        command
            .env("PIPEWIRE_CONFIG_DIR", &config_dir)
            .arg("-c")
            .arg(config);
        let lines = self.spawn_service(command, "pipewire");
        if lines
            .wait_for(READY_TIMEOUT, |line| line == "pipewire-ready")
            .is_none()
        {
            self.not_ready("pipewire");
        }
        let mut command = self.command(WIREPLUMBER);
        command
            .env("PIPEWIRE_CONFIG_DIR", &config_dir)
            .env("WIREPLUMBER_CONFIG_DIR", WIREPLUMBER_CONFIG_DIR)
            .args(["--profile", "policy"]);
        self.spawn_service(command, "wireplumber");
        self.add_null_devices();
    }

    /// Loads the null devices and waits until WirePlumber has picked the defaults.
    fn add_null_devices(&self) {
        let mut command = self.command(PACTL);
        command.arg("subscribe");
        self.seal(&mut command);
        let events = Watcher::spawn(command, Lines::read);
        self.wait_for_subscription(&events);
        self.pactl(&[
            "load-module",
            "module-null-sink",
            &sink_name(SEALED_SINKS[0]),
        ]);
        self.pactl(&[
            "load-module",
            "module-null-sink",
            &sink_name(SEALED_SINKS[1]),
        ]);
        self.pactl(&[
            "load-module",
            "module-null-sink",
            &sink_name(SEALED_SOURCE),
            "media.class=Audio/Source/Virtual",
        ]);
        let deadline = Instant::now() + READY_TIMEOUT;
        while !self.audio_defaults_ready() {
            let left = deadline.saturating_duration_since(Instant::now());
            if events
                .lines
                .wait_for(left, |event| event.contains("on server"))
                .is_none()
            {
                self.not_ready("wireplumber");
            }
        }
    }

    /// `pactl subscribe` prints nothing once it listens. Every other pactl run is a new
    /// client, so the first client event it reports proves that no later event is missed.
    fn wait_for_subscription(&self, events: &Watcher) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            self.pactl(&["info"]);
            let seen = events
                .lines
                .wait_for(SUBSCRIPTION_PROBE, |event| event.contains("on client"));
            if seen.is_some() {
                return;
            }
        }
        self.not_ready("pactl subscribe");
    }

    fn audio_defaults_ready(&self) -> bool {
        let sink = self.pactl(&["get-default-sink"]);
        let source = self.pactl(&["get-default-source"]);
        SEALED_SINKS.contains(&sink.trim()) && source.trim() == SEALED_SOURCE
    }
}

fn sink_name(name: &str) -> String {
    format!("sink_name={name}")
}
