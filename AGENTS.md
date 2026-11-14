# Rules for agents

Next action: read section 1, then the plan of the slice you work on: `docs/V2_PLAN.md` for v2 (its section 1 adds rules), `docs/IMPLEMENTATION_PLAN.md` for stage 1.

`docs/PROTOCOL.md` is normative. If code and spec disagree, the spec wins. If the spec looks wrong, report it; do not diverge silently.

## 1. Hard rules

1. The owner's desktop runs sway. Never send it a state-changing command. Read-only `swaymsg -t get_*` is the only allowed contact. A daemon that an agent connects to the real sway has `read_only = true`, temp XDG directories, a loopback address and a port that is not 28414. Actions are tried against the fake sway only.
2. Never start a real, nested or headless sway. Tests use the fake sway of `swayconnect-testkit` on a temp Unix socket.
3. Android builds are allowed since the owner installed the SDK (2026-11-13), under the limits of section 2: one heavy process at a time, Gradle with `--max-workers=4`, at most one headless emulator. Never start Android Studio, a second AVD or an SDK download.
4. No `git commit`, no `git push`, no `sudo`, no system package install. The owner does these.
5. Never edit the owner's sway config, firewall or systemd units.

## 2. Commands

Run cargo through `just` only. Each recipe takes one lock, so builds of parallel agents queue. `.cargo/config.toml` caps build jobs and test threads at 4. Why: `docs/IMPLEMENTATION_PLAN.md` section 1.2.

| Command | Does |
|---|---|
| `just check [crate]` | `cargo check --all-targets` |
| `just lint [crate]` | `cargo clippy --all-targets -- -D warnings` |
| `just test [crate] [args]` | `cargo test --locked` |
| `just fmt-check` | formatting check, whole workspace |
| `just fmt` | formats the whole workspace |
| `just deny` | `cargo deny check` |
| `just ci` | fmt-check, lint, test, deny |

No crate argument means the workspace.

- Do not set `CARGO_TARGET_DIR`. Do not raise the job count.
- When other agents edit the tree, do not run `just fmt`. Format your own files: `rustfmt --edition 2024 <files>`.
- Do not leave a background process running.

Done for a Rust change: `just fmt-check`, `just lint <crate>`, `just test <crate>` pass. Files stay under about 500 lines. Each crate root has `#![forbid(unsafe_code)]`.

Android, from `clients/android`. Gradle has no `just` recipe, so take the same lock by hand:

```
flock ../../target/.agent.lock ./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug --max-workers=4
```

- Run `./gradlew --stop` when you finish. Do not raise `org.gradle.jvmargs`.
- Emulator: reuse a running one (`adb devices`). Else start the one AVD (`Medium_Phone_API_37.0`) headless: `-no-window -no-audio -no-boot-anim -no-snapshot-save -cores 2 -memory 3072`. No Gradle or cargo run while it boots. End with `adb emu kill`.
- The emulator reaches the host's loopback at `10.0.2.2`. The daemon never needs a LAN address for it.
- `adb install --user <id> -r` replaces the APK for every user that has the app, user 0 included; its data stays. `cmd uimode night yes|no` is device-wide: set it back at the end.
- The emulator app may hold the owner's own pairing. Never pair over it. To pair with a test daemon, use a separate Android user: `adb shell pm create-user <name>`, `adb install --user <id> -r <apk>`, `adb shell am switch-user <id>`. At the end `am switch-user 0` and `pm remove-user <id>`. Screens with `FLAG_SECURE` (pair) capture black; read them with `uiautomator dump`.

Done for an Android change: the gate above passes with 0 lint errors. A change to a screen is looked at on the emulator.

## 3. Layer rules

1. Only `swayconnect-sway` builds sway command text. Only `swayconnect-daemon` depends on `swayconnect-sway`.
2. `swayconnect-cli` speaks only the wire protocol. It must not depend on `swayconnect-sway` or `swayconnect-daemon`.
3. `swayconnect-protocol` has no async runtime and no I/O. It never depends on `swayconnect-testkit`, not even as a dev-dependency.
4. `swayconnect-testkit` does not depend on the daemon, so the daemon can dev-depend on it without a cycle.
5. No code outside the two `main.rs` files reads the process environment. Library code takes explicit values (`Paths`, `SwayTarget`, `EnvSnapshot`).
6. A future client is a new directory under `clients/`. It depends on `docs/PROTOCOL.md` and `protocol/vectors/` only.

## 4. Protocol-change rule

A wire change updates all four in one change:

1. `docs/PROTOCOL.md`
2. `protocol/vectors/`
3. the Rust crate `swayconnect-protocol`
4. every client: `crates/swayconnect-cli` and the Kotlin `:protocol` module in `clients/android`

Vector file formats: `protocol/vectors/v1/README.md`.

## 5. Security rules that must never be broken

From `docs/DESIGN.md` 5.3 as amended by 11.3 (approved by the owner on 2026-11-14):

1. No operation accepts a sway command, a shell command or a file path.
2. No client string is placed into a sway command. A command holds only integers, booleans and members of closed enums defined in `swayconnect-sway` (layout kinds). The one fixed command `output * power on` holds no variable at all.
3. Authentication cannot be turned off. No config key, no flag, no environment variable.
4. Pairing, pairing confirmation, revoke and scope assignment go through the admin socket only. Never over TCP.
5. Only sway message types 0, 1, 2, 4, 7 are ever sent. `GET_CONFIG` is never sent. The event subscription adds `binding`.
6. Secrets are never logged and never sent: `S`, `K`, proofs, nonces, the pairing URI. Window titles are never logged. Typed text is never logged; logs and the audit record lengths only.
7. A pairing never becomes active without the owner's yes.
8. Child processes: argv only, no shell. The program is an absolute path from config (every program has a config key with a default). Arguments are integers, closed enums and daemon-owned strings (pactl names from the daemon's own snapshot, desktop entry ids and Exec tokens from the daemon's own index, config values). A daemon-owned string that comes from another program (a pactl name) goes after `--` and never starts with `-`. Environment from an allowlist. stdin null. stdout and stderr read with a cap and never sent to a client. A timeout and kill.
9. A client string is allowed in a request only when it is an id the server issued (entry id) or a closed enum, matched exactly. The one exception is `input.text`, which goes only to the virtual keyboard, never to a process, a file or sway.
10. Remote input fails closed: refused while the screen is locked or lock state is unknown, while a keyboard-grabbing surface runs, or when focus, read from sway just before each part, is not the target.
11. No client-chosen string reaches a desktop notification. Notices use fixed text and the device short id.

Default pairing scopes (`docs/DESIGN.md` 11.8, D30): `swayconnectd pair` gives `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys`, `input.text`. `destructive`, `audio.capture` and `launch.risky` are granted only by name. Do not add a scope to the default, or remove a safeguard of `docs/DESIGN.md` 11 (lock refusal, focus check per part, desktop notices, audit, revoke, panic switch), without the owner's yes.

## 6. Tests

- Nothing binds TCP port 28414. Listeners use `127.0.0.1:0`.
- No test calls `systemctl`, `loginctl`, `swaylock` or a mutating `swaymsg`.
- No sleep for readiness. Wait on a channel or a ready signal. For a `fake-proc` child, make its `<program>.log` a FIFO and read the line it writes from its own `main` (`tests/v2_m2a/rig.rs` `Gate`). `spawn` returning is not that signal: `/proc/<pid>/environ` can still read empty then.
- Every test config points every program path at the testkit's `fake-proc` (`TestConfig`); a hand-written config calls `assert_no_system_programs`. A test daemon never reaches the real sound server, session bus, compositor or system bus.
- A paused tokio clock jumps to the next timer whenever the runtime waits, also while a socket event is still in the kernel. The daemon always has timers running (audio watcher backoff, polls), so a deadline measured over real I/O uses the real clock (`tests/limits.rs`).
- Child processes start with a cleared environment and temp XDG directories (`docs/DESIGN.md` 9.2).
- Do not weaken, ignore or delete a test to get green.

## 7. Where things are

| Need | Read |
|---|---|
| Wire contract | `docs/PROTOCOL.md` |
| Architecture, decisions (section 2), security model (section 5) | `docs/DESIGN.md` |
| Alternatives behind the decisions | `docs/RESEARCH.md` section 4 |
| Slices, ownership, acceptance | `docs/IMPLEMENTATION_PLAN.md` (stage 1), `docs/V2_PLAN.md` (v2, with status and owner gates) |
| Screens and texts of the Android app | `docs/UX.md` |
| Dependencies and versions | `docs/DESIGN.md` 7.3; only the manifest owner edits `Cargo.toml` files |
