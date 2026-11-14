# M2c plan: audio, media, launching and typing against real programs

Status: done 2026-11-14; review findings fixed 2026-11-14 (section 7). Every slice of section 4 is written; checkpoint by INT passed (section 7): `just ci`, `just real` twice and the Android gate green, R5 seen on the emulator, live session unchanged. Owner answered D1 and D2 on 2026-11-14 (section 6). What only the live desktop can show stays in the owner gates (`docs/V2_PLAN.md` section 5).

## 1. Goal

M2 implemented audio, media, launching and typing, but agents tested them only against fakes
(fake-proc, FakeKeyboard, the fake media backend). The first real run would be the owner gates
on the live desktop.

M2c runs them against the real programs inside a sealed test session: a headless sway, a
private D-Bus bus and a private PipeWire. The live desktop stays untouched. After M2c the owner
gates shrink to what only the real desktop can show: speakers and the wob OSD, a browser's
MPRIS player, a real phone over the LAN, keep awake and lock.

M2c also closes the gaps that M2 left in these four features (section 4, slice G).

## 2. What runs for real, and what is still not real

| Feature | Real in M2c | Still not real |
|---|---|---|
| Typing | headless sway 1.12, the daemon's zwp_virtual_keyboard_v1, foot as the target window, real fuzzel as the launcher target | the owner's keyboard layout and window set |
| Audio | private PipeWire + WirePlumber + pipewire-pulse with null sinks and a null source, real `pactl` | speakers, the wob OSD |
| Media | a private D-Bus session bus, the real zbus/MPRIS client code, a test MPRIS player (testkit binary; mpv-mpris is not installed) | a browser or Spotify player |
| Launch | real desktop-entry index, real `systemd-run --user --scope` (D1), foot started by a test entry and shown in the headless sway | the owner's app list |

Keep awake and lock stay owner gates. They act on the real logind session.

## 3. The sealed session (slice R0)

Built 2026-11-14: `swayconnect_testkit::RealSession` (`crates/swayconnect-testkit/src/real_session.rs`,
configs beside it in `real_session/`). Each real-backend test starts and stops its own session:

1. One temp root `/tmp/swayconnect-real.XXXXXX` with `rt` (`XDG_RUNTIME_DIR`, mode 0700),
   `home`, `config`, `cache`, `data`, `state`, `share` (empty XDG dirs), `etc` (configs) and
   `log` (stderr of every program). The short path keeps every socket far below the 108 bytes
   of a Unix socket address. Every child gets `env_clear()` plus only `RealSession::env()`
   (every start path rebuilds the environment from the variables set on the command, so even
   a bare `Command` inherits nothing of the test process):
   `XDG_RUNTIME_DIR`, `HOME`, the XDG dirs, `PATH=/usr/bin:/bin`, `LANG=C.UTF-8`,
   `WAYLAND_DISPLAY` (the name sway chose, relative to `rt`, so fuzzel's lock file works),
   `SWAYSOCK`, `DBUS_SESSION_BUS_ADDRESS`, `DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent`
   and `PULSE_SERVER=unix:<rt>/pulse/native`.
2. A supervisor leads the process group: `sh -c 'cat >/dev/null; kill -KILL 0'`, whose
   stdin is a pipe only the test process writes (close-on-exec, so no other child holds it).
   When the test process dies, also by SIGKILL, the pipe closes, `cat` ends and the whole
   group is killed. Then a private bus: `dbus-daemon --nofork --print-address=1` with its own config (listen on
   `<rt>/bus`, no service directory, so nothing is activated). Ready = the printed address.
3. Headless sway: `WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
   sway -c <etc>/sway.conf`. The config: no Xwayland, no swaybg, no swaynag, one output
   `HEADLESS-1` at 1280x720, no bar, no idle, no include. Ready = an `exec printf` line with
   `WAYLAND_DISPLAY` and `SWAYSOCK` on sway's stdout.
4. Audio: one `pipewire -c <etc>/sealed-pipewire.conf` that also loads the PulseAudio
   protocol module (what pipewire-pulse is), with no device and no realtime module. Ready = a
   `context.exec` line on its stdout. Then `wireplumber --profile policy`: default nodes and
   links, no hardware monitor (no ALSA, Bluetooth, V4L2 or libcamera). Neither reads a host
   drop-in: PipeWire gets `PIPEWIRE_CONFIG_DIR=<root>/etc`, WirePlumber also
   `WIREPLUMBER_CONFIG_DIR=/usr/share/wireplumber`, so nothing in `/etc/pipewire`,
   `/etc/wireplumber` or `~/.config` can add a device; `pw-dump` of the session shows no
   device object and no `api.*` node. Proven on 2026-11-14:
   without WirePlumber the null sinks work, but `pactl get-default-sink` answers
   `@DEFAULT_SINK@` and `set-default-sink` is lost, so R1 needs it. Then `pactl load-module
   module-null-sink` gives `sealed_sink_1` and `sealed_sink_2`, and `module-null-sink
   media.class=Audio/Source/Virtual` gives `sealed_source` (pipewire-pulse has no
   `module-null-source`). Ready = WirePlumber picked a sealed sink and `sealed_source` as
   defaults, seen through `pactl subscribe` server events.
5. Guard (`LiveEnv`, built by the test from its own environment; the live runtime dir is
   `XDG_RUNTIME_DIR`, else `/run/user/<uid>`, and `/run/user/<uid>/` is always refused):
   `start` panics when `rt` is, holds or lies in the live runtime dir; every start of a
   program panics when its environment names the live runtime dir, `WAYLAND_DISPLAY` (also
   a relative name resolved against the child's runtime dir), `SWAYSOCK`/`I3SOCK`, session
   bus address or path, or sound server (`PULSE_SERVER`, `<live rt>/pulse/native`,
   `pipewire-0`), also through a symlink (paths are resolved before they are compared), or
   when a symlink in `rt` or one level below resolves into the live runtime dir, except the
   one D1 link once `allow_user_systemd` ran. `checked_env` runs the same check on the
   environment a test hands to the daemon's `RealRunner` (pactl, fuzzel, systemd-run). Unit
   tests cover each case; `just real` proves it trips on a leaked SWAYSOCK and that a bare
   command inherits nothing.
6. Teardown on drop, also on panic: SIGKILL to the session's process group (led by the
   supervisor, never reaped earlier), reap of every child, a bounded wait until no live
   process of the group is left, then removal of the root. After a panic the logs move to
   `<root>.log` first, and the path is printed. A failing `swaymsg` or `pactl` panics with
   the tail of the sway or PipeWire log.
7. Helpers: `command`/`spawn` (foot, fuzzel) and `output` (a short command to its end,
   such as `systemctl --user`), both sealed and guarded on the final environment,
   `checked_env`, `swaymsg`, `pactl`, `screenshot(png)` (`grim -o HEADLESS-1`, non-empty),
   `wait_for_window(app_id)` (window subscription plus tick, so no event is missed),
   `live_processes_in_group`, `processes_naming`/`leftovers_naming` (processes that name `rt`,
   also outside the group, such as the daemon's children), and D1: `allow_user_systemd()`
   makes `<rt>/systemd/private` a symlink to the live user manager's private socket, the
   only path `systemd-run --user --scope` needs, and tells the guard to allow exactly that
   link; the bus, compositor and sound server stay sealed.
8. Manual walks: `just sealed-session [--allow-user-systemd]` (testkit example) starts the
   same `RealSession`, prints its environment as `export` lines and tears it down when its
   stdin ends. Only the build takes the lock. Killed by a signal, its supervisor still kills
   the group, but the temp root stays in `/tmp`. A daemon started by hand from those lines is
   outside the guard: give it exactly those variables (`env -i`).

Proven by `just real` (`crates/swayconnect-testkit/tests/real_session.rs`, 7 tests): 10 start/stop rounds with no process left in the group or naming `rt`, `rt` removed, a
non-empty grim capture each round, and the live sink list unchanged; foot maps a window and
fuzzel draws on headless+pixman (screenshots in `target/tmp/real-session/`), so no testkit
Wayland client is needed; a panicking test still tears down and keeps its logs; a bare
command, run or spawned, inherits nothing of the test process; the sealed sound server has
no device object; `systemd-run --user --scope` fails while sealed and runs in
`app-swayconnect-test-r0-<pid>.scope` after `allow_user_systemd()`.

Not covered: a test process killed with SIGKILL still leaves its temp root in `/tmp` and
its D1 scope until the scope's foot dies with sway (`--collect` then removes the unit); the
daemon's children outside the group (`pactl subscribe`, fuzzel) end only when they notice
that the sound server or compositor is gone. The SIGKILL case was not provoked in a test.

Every recipe takes the agent lock with `flock -o`, so no program a test starts inherits the
lock file: a sealed program left running can never block the next `just` or Gradle run.

The real-backend tests do not run in `just ci`. Each test file `tests/real_*.rs` starts with
`#![cfg(feature = "real-backends")]` (feature on testkit and daemon). `just real` runs clippy
and those tests under the same lock, with 1 test thread. A GitHub job for them is a later
decision.

## 4. Slices

Order: R0 alone first. Then at most two slices at a time, because each test starts its own
session (machine safety, AGENTS.md section 2). G-CLI and G-APP need no session and can run
beside one R slice.

| Slice | Owns | Done when |
|---|---|---|
| R0 harness | testkit `real_session.rs`, sway/pipewire test configs, `just real` | a smoke test starts and stops the session 10 times with no leftover process; the guard trips on a leaked SWAYSOCK; `grim` captures the output |
| R1 audio | daemon `tests/real_audio.rs` | the daemon lists 2 sinks + 1 source from real pactl; set_volume, set_mute, set_default show in `pactl` read back by the test; a change made with `pactl` outside the daemon reaches the audio topic (subscribe); mic unmute refused without audio.capture, allowed with it plus the notice |
| R2 media | testkit `mpris-test-player` bin (zbus), daemon `tests/real_media.rs` | the player appears in the media topic with title and status; play_pause, next and previous reach it (the player logs each call); the player quitting removes it from the topic; a second player and focus between them |
| R3 typing | daemon `tests/real_input.rs` | text typed into foot (`foot sh -c 'cat > out'`) equals the input, including non-ASCII and a 200-character text over several parts; Enter, Esc and Ctrl+C keys; a focus change mid-text stops it with focus_changed and the rest is not typed; launcher.open starts real fuzzel and typing reaches it; Escape gives launcher_closed |
| R4 launch | daemon `tests/real_launch.rs`, desktop-entry fixture | a test entry (Exec = foot with a fixed title) starts through real `systemd-run --user --scope`; the window appears in the headless sway on the target workspace; the unit is `app-swayconnect-*.scope`; the test stops the unit at the end; a terminal entry is refused without launch.risky |
| R5 emulator | screenshots only | the app, paired with a read_only=false daemon on the sealed session, drives Audio, Media, Launch and Keys; `grim` shots of the headless output show the effect (window opened, text typed) beside the phone screenshots |
| G-CLI | crates/swayconnect-cli | `audio devices`, `media players`, `apps` listing commands; `launcher open`; `--launcher` target for type and key; `media next/previous`; `audio default <id>` |
| G-APP | Android More area | Mic and Launcher tiles (UX 10.1) |
| G-FIX | daemon | the fake-sway daemon listed no apps from the desktop-entry fixture (found at the M2 checkpoint); the connect-notice behaviour once the owner decides (section 6, D2) |

Then review, fix and gate, as in M2. The gate also checks that no real-backend test touched
the live session: the live sink volume, the live sway tree and `systemctl --user list-units
'app-swayconnect-*'` are the same before and after.

## 5. Risks

1. The lock check reads the real `/proc` for swaylock. If the owner's screen is locked during
   `just real`, typing tests are refused. R3 makes the lock-process scan root configurable,
   or the harness checks the live lock state first and skips with a clear message.
2. Headless sway with the pixman renderer may not run foot or fuzzel. R0 proves this first;
   if it fails, a small testkit Wayland client that records keys replaces foot.
3. PipeWire null sinks may report different `pactl -f json` fields than a real card. R1 keeps
   the M2 fixture tests and adds the real output as a second fixture.
4. Each session is about 5 processes. Two at a time is the cap.
5. If the GitHub runner gets these tests later, it needs sway, PipeWire, foot and fuzzel
   installed (apt), and a few minutes more per run.

## 6. Owner decisions

Answered 2026-11-14: **D1 allow** (real `systemd-run --user`, test scopes stopped by the test). **D2 once per device per 12 h**.


- **D1, launch through the real user systemd.** R4 needs real `systemd-run --user`, which
  creates a short transient scope in the owner's own user manager (`app-swayconnect-test-*`,
  stopped by the test). The window opens only in the headless sway. Choices: allow it, or keep
  launching as an owner gate and run R4 with a fake `systemd-run`.
- **D2, connect notice.** Today every authenticated reconnect of a default-paired phone shows
  a notice (9 times in 12 minutes during testing). Keep it, limit it per device (for example
  once per 12 h), or drop it from DESIGN 11.8.

## 7. Checkpoint (INT, 2026-11-14)

Results and the R5 walk: `docs/V2_PLAN.md`, "Status: checkpoint M2c". Bugs the real programs showed, each fixed at the root with a test that fails on the old code:

| Found by | Bug | Fix |
|---|---|---|
| R1 | every audio write failed: pactl takes `--` after its command word as the device name | `-- set-sink-volume <name> <N>%` (DESIGN 11.6) |
| R3 | launcher typing was always refused: under fuzzel, sway marks no node focused | the launcher check follows the `focus` chain (DESIGN 11.7) |
| INT (R5) | the app showed "Nothing playing" for a paused player that had not played since the daemon started | the card falls back to the first paused player (UX 7.2) |
| INT (R5) | after Esc closed fuzzel, Keys had no target: the `session` state came before the `desktop` state with the focus back | the next focused window becomes the target (UX 9.2) |
| review | launcher typing passed the focus check before fuzzel held the keyboard: the recorded window, still focused, got the keys (`launcher open && type --launcher` could type into a terminal) | the launcher target needs no focused node; the guard waits up to 1 s for fuzzel's grab (DESIGN 11.7, PROTOCOL 17.13, vector `088`); `real_input` types right after `launcher.open` |
| review | `swayconnect play` without `--player` failed for a paused player that had not played | the CLI uses the app's order: active, else first playing, else first paused (PROTOCOL 17.10) |

Still open from the slices:

1. A test process killed with SIGKILL: since the review fixes the supervisor kills the session's group (section 3 item 2). The daemon's `pactl subscribe` and `fuzzel` then end on their own, the D1 foot dies with sway; the temp root stays. Not provoked in a test. If a unit is left, stop it by hand (`systemctl --user stop 'app-swayconnect-test-foot-*'`).
2. A seat with no keyboard drops the first keys after the daemon's virtual keyboard appears, and the daemon reports them delivered. Only the headless sway has no keyboard; `real_input` arms the seat first. A daemon fix needs a decision (create the keyboard at start, or hold the first delivery after a seat capability change).
3. Not run for real: source volume and set-default on a source, Xwayland clients, `super` and the ctrl+alt+F-key refusal, MPRIS players that time out, the 16-player cap.
4. No startup warning when no data dir has `applications/` (G-FIX proposal, about 3 lines).
5. fuzzel logs to the user journal, also from the sealed session.
6. No GitHub job for `just real`.
7. Closed by the review fixes: WirePlumber and PipeWire read no host drop-in (section 3 item 4).
8. Seen once, not reproduced: the sealed sway died on `swaymsg input type:keyboard xkb_layout de` (R3). A failing `swaymsg` now shows the tail of `sway.err`, and a panicking test keeps its logs in `<root>.log`.
9. Unexplained: the live sink volume moved from 30 % to 5 % during R1, before INT's baseline. Ruled out: the daemon's `ChildEnv` drops `PULSE_SERVER` of the test process and sets the sealed `XDG_RUNTIME_DIR`, so its pactl reached only the sealed server; the tests' own pactl runs come from `RealSession` (sealed). Not ruled out: an agent's hand-run command, or the owner. Since the review fixes every real-backend rig (audio, input, launch, media) compares the live sinks and sources (name, mute, volume) and both defaults before and after, read only.
