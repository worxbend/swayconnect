# pactl fixtures

Used by `crates/swayconnect-daemon/src/audio/` tests (D-AUDIO, `docs/DESIGN.md` 11.12).

| File | Origin |
|---|---|
| `sinks.json`, `sources.json` | `pactl -f json list sinks` and `sources`, captured read-only on the owner's machine on 2026-11-14 (pactl 17.0 against pipewire-pulse). Not edited. |
| `default-sink.txt`, `default-source.txt` | `pactl get-default-sink` and `get-default-source`, same capture. |
| `hostile-sinks.json`, `hostile-sources.json` | Written by hand: names that start with `-`, look like options, are empty, are not strings or hold a control character; monitor sources in both spellings (`device.class`, `monitor_of_sink`); control and format characters and a 200-character description; three channels; an absurd volume; a member of the wrong type; entries that are not objects; one sink and source per `kind` rule. |

The captured machine has one sink and one real source, so the hand-written files carry the cases it lacks. No test runs pactl.
