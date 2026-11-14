# swayconnect design, stage 1

Status: accepted for stage 1, after the security, implementability and scope review. Date: 2026-11-13.
Related: `docs/PROTOCOL.md` (wire contract, normative), `docs/RESEARCH.md` (why), `docs/IMPLEMENTATION_PLAN.md` (how and in what order).
Version 2 (UI/UX and desktop controls): section 11, with `docs/UX.md`, `docs/V2_RESEARCH.md` and `docs/V2_PLAN.md`. Sections 1 to 10 stay the record of stage 1; section 11 says where v2 changes them.

Next action: the owner reviews V2 and runs step 5 of 9.4 himself. Status of the work: `docs/IMPLEMENTATION_PLAN.md` section 0.

## 1. What stage 1 is

One working path, end to end:

1. The owner pairs a client with the daemon and confirms it at the desktop.
2. The client shows live workspaces and windows.
3. The client focuses a workspace or a window. Sway does it.

Stage 1 is done when all four are true:

| # | Proof | Who |
|---|---|---|
| a | The automated tests pass (`just ci`). They use a fake sway only. | agents |
| b | The read-only live check passes (9.4 steps 1 to 4). | owner present |
| c | The owner himself, at his own keyboard, has run `swayconnect workspace <n>` and `swayconnect focus <id>` once against his real sway with `read_only = false` (9.4 step 5). | owner only |
| d | The same two actions worked from the phone over the LAN, with a LAN `listen` address, `allow_from` and the firewall rule (9.4 step 6). The phone runs the debug APK, built by `android.yml` or on this machine. | owner only |

Agents never send a state-changing command to the real sway. Steps c and d are the owner's.
If the Android build is not green yet, a second LAN host with the CLI may prove the LAN path first. Stage 1 stays open until the phone has done step d.

### 1.1 In scope

| # | Item |
|---|---|
| 1 | Daemon `swayconnectd`: sway IPC client, TLS 1.3 listener, pairing with owner confirmation, per-device keys, scopes, typed actions, live state |
| 2 | Protocol v1: spec in `docs/PROTOCOL.md`, conformance vectors in `protocol/vectors/v1/` |
| 3 | State: one topic `desktop` with workspaces and windows, pushed as full snapshots |
| 4 | Actions: `workspace.focus`, `window.focus`, `window.fullscreen` |
| 5 | Rust CLI `swayconnect`: pair, read, watch, act, raw frames for negative tests |
| 6 | Android app: pair by link or paste, workspace and window lists, tap to act. Built and run on an emulator on this machine since 2026-11-13 (section 8). |
| 7 | Local admin commands: `pair` (with confirmation), `devices`, `revoke` |
| 8 | Read-only daemon mode, and a panic switch that turns it on without typing focus |

### 1.2 Out of scope (do not build)

- Any way to send a sway command string. No `exec`. No command buttons. No `GET_CONFIG`.
- Close window (`window.close`, scope `destructive`). Workspaces addressed by name. Both are stage 2.
- Pointer, keyboard, media, volume, lock, power, screenshot, clipboard, notifications, brightness.
- Move window, layouts, outputs list, binding modes, scratchpad, marks.
- mDNS discovery, WebSocket, browser client, client certificates, JSON Schema files, state deltas, unsubscribe, a one-shot `state.get`.
- In-app QR camera scanner, more than one paired computer in the Android app, more than one profile in the CLI.
- Any switch that turns off authentication or encryption. None exists and none is planned.

## 2. Decisions and defaults

Each row is final for stage 1. The reason is one line. Details are in `docs/RESEARCH.md` section 4.

| # | Decision | Reason |
|---|---|---|
| D1 | Raw TCP, TLS 1.3 only, ALPN `swayconnect/1`, no session resumption | No HTTP parser in front of authentication. A browser page cannot speak it. Platform TLS on every client. Every handshake shows the certificate. |
| D2 | Frames: 4-byte big-endian length, then one UTF-8 JSON object | Size is known before the payload is read. No escaping rules. Room for binary frames later. |
| D3 | Spec split into message layer and transport binding. The ALPN id versions the binding, the handshake and the proofs. `protocol` versions the messages after `welcome`. | A WebSocket binding can be added later with no message change. A version 2 can be negotiated cleanly. |
| D4 | Server identity = SHA-256 of the whole leaf certificate DER. Key is ECDSA P-256. | Every language can compute it with the standard library. No X.509 parsing. |
| D5 | Pairing and authentication by HMAC-SHA256 proofs bound to the certificate fingerprint and two nonces | No secret crosses the wire, not even at pairing. A relay with another certificate fails even when a client compares the pin wrongly. A client must still check the TLS handshake signature; nothing replaces that. |
| D6 | Pairing secret: 256 bits in a QR code and URI, single use, 300 s. The device key is derived from it. | Short eyeball codes were brute-forced in KDE Connect. A PAKE has no good Android library. |
| D7 | Server sends `hello` first, with nothing but version range and nonce | One round trip. Nothing about the host leaks before authentication. |
| D8 | Typed operations only. Targets are numeric ids that must exist in the daemon's own snapshot. No string ever enters a sway command. | A command string surface is remote code execution. A blocklist cannot be made safe. |
| D9 | Scopes from version 1: `state.read`, `state.titles`, `control` | Splitting an existing scope later would break paired devices; adding one would not. Limits what a stolen phone can do. |
| D10 | State sync: full snapshot, latest wins, `rev` per connection | No delta bugs. A tokio `watch` channel is the whole backpressure design. |
| D11 | Sway IPC: hand-written framing on tokio, own lenient types, two connections | The `swayipc` crates have no timeouts, no explicit socket path and strict enums that fail on new sway values. |
| D12 | Admin channel: owner-only Unix socket with peer uid check on both ends | Pairing secret stays in memory. Revoke is immediate. The daemon is the only writer of its files. |
| D13 | Bind `127.0.0.1` by default. Wildcard addresses refused. LAN bind needs an explicit address **and** an explicit `allow_from` list. | This machine has a `docker0` bridge. Containers can reach a LAN address of the host. |
| D14 | Tests use a fake sway IPC server only. No agent or CI job starts a real, nested or headless sway. No library code reads the process environment. | The owner's desktop must not be touched. A real compositor is not needed to test the daemon. |
| D15 | Five Rust crates: protocol, sway, daemon, cli (with the client library inside), testkit | The sway crate isolates the command builder. The protocol crate is the Rust form of the contract. The client library has one consumer. |
| D16 | Android: two modules, platform `SSLSocket` and `javax.crypto`, no network or DI library | The app was written before this machine could build it. Less code and fewer plugins meant fewer blind mistakes. |
| D17 | A new pairing is pending until the owner confirms a 6-digit code at the desktop | The secret can leak (QR photo, another app that takes the link). Without confirmation the first presenter wins. |
| D18 | No lockout anywhere. Limits are caps, deadlines and token buckets. | Keys are 256 bits, so counting failures protects nothing. A lockout lets any LAN host or local process lock the owner out. |
| D19 | Actions are rate limited per device. `SIGUSR1` switches the daemon to read-only. | A paired device could otherwise move focus faster than the owner can type `revoke`. |

Defaults chosen without asking the owner. Change them in config or in a later commit:

| Default | Value | Where |
|---|---|---|
| TCP port | 28414 (not in `/etc/services`; IANA registry not checked) | config `listen` |
| Scopes given at pairing | `state.read`, `state.titles`, `control` (v1; the v2 default is in 11.8) | `swayconnectd pair --scopes` |
| Pairing window | 300 s (30 to 600) | `swayconnectd pair --ttl` |
| Pairing confirmation deadline | 60 s | fixed (`docs/PROTOCOL.md` section 14) |
| Server label | system hostname | config `server_name` |
| Log level | info | `RUST_LOG` |
| License | MIT (same as sibling repos `prismcast`, `swaylingo`) | `LICENSE`, `Cargo.toml` |
| Rust edition and toolchain | 2024, pinned 1.99.1 | `rust-toolchain.toml` |
| Android ids | namespace `com.swayconnect.android`, applicationId `com.worxbend.swayconnect` | `clients/android/app/build.gradle.kts` |
| Android SDK levels | compileSdk 37, targetSdk 36, minSdk 29 | same |

`state.titles` is on by default because a window list without titles cannot tell three terminals apart. The channel is encrypted and the phone is the owner's. Titles are never logged. An owner who wants less passes `--scopes state.read,control`.

## 3. Monorepo layout

```
swayconnect/
├── Cargo.toml                      workspace: members, [workspace.package], [workspace.dependencies]
├── Cargo.lock
├── .cargo/config.toml              [build] jobs = 4, [env] RUST_TEST_THREADS = "4" (machine safety, plan 1.2)
├── rust-toolchain.toml             channel 1.99.1, components rustfmt + clippy
├── deny.toml                       license allowlist, unknown registries denied
├── justfile                        fmt, fmt-check, lint, test, deny, ci; every recipe takes one lock
├── README.md                       what it is, quick start, firewall line, security warning, what was verified for Android
├── AGENTS.md                       layer rules, commands, protocol-change rule
├── CLAUDE.md                       points to AGENTS.md
├── LICENSE
├── .gitignore
├── .editorconfig
├── config.example.toml             every daemon option with its default
├── docs/
│   ├── RESEARCH.md
│   ├── DESIGN.md
│   ├── PROTOCOL.md                 normative protocol spec
│   └── IMPLEMENTATION_PLAN.md
├── protocol/                       machine-readable contract, owned by no client
│   └── vectors/v1/
│       ├── README.md               file formats (the only copy after S0)
│       ├── fingerprint.json
│       ├── proofs.json
│       ├── pairing-uri.json
│       ├── frames.json
│       ├── requests.json
│       ├── handshake.json
│       └── transcripts/
│           ├── t1-pair-ok.json
│           ├── t2-auth-ok.json
│           ├── t3-session.json
│           ├── t4-auth-failed.json
│           ├── t5-version.json
│           ├── t6-sway-offline.json
│           └── t7-pair-refused.json
├── crates/
│   ├── swayconnect-protocol/       messages, frame codec, proofs, pairing URI, limits, codes, text rules. No tokio, no sway.
│   ├── swayconnect-sway/           sway IPC framing, connection, socket resolution, raw types, the only command builder
│   ├── swayconnect-daemon/         lib + bin `swayconnectd`
│   ├── swayconnect-cli/            lib (module `client`: pinned TLS, handshake, requests, events) + bin `swayconnect`
│   └── swayconnect-testkit/        publish = false: fake sway IPC server, temp directories, fixtures
├── clients/
│   └── android/                    self-contained Gradle project
│       ├── settings.gradle.kts  build.gradle.kts  gradle.properties  gradlew  gradlew.bat  .editorconfig
│       ├── gradle/libs.versions.toml
│       ├── gradle/wrapper/         gradle-wrapper.jar, gradle-wrapper.properties (9.8.0)
│       ├── protocol/               module :protocol (pure Kotlin/JVM)
│       └── app/                    module :app (Android, Compose)
├── packaging/
│   └── systemd/swayconnectd.service
└── .github/workflows/
    ├── rust.yml                    path-filtered
    └── android.yml                 path-filtered; the same Gradle tasks as a local build; uploads the debug APK
```

Layer rules (to be written into `AGENTS.md`):

1. Only `swayconnect-sway` builds sway command text. Only `swayconnect-daemon` depends on `swayconnect-sway`.
2. `swayconnect-cli` speaks only the wire protocol. It must not depend on `swayconnect-sway` or `swayconnect-daemon`.
3. `swayconnect-protocol` has no async runtime and no I/O. `swayconnect-testkit` is not a dependency of it, not even a dev-dependency.
4. `swayconnect-testkit` depends on tokio and serde only. It does not depend on the daemon, so the daemon can dev-depend on it without a cycle.
5. No code outside the two `main.rs` files reads the process environment (9.2).
6. A wire change updates `docs/PROTOCOL.md`, `protocol/vectors/`, the Rust protocol crate and every client in one change.
7. A future client is a new directory under `clients/`. It depends on `docs/PROTOCOL.md` and `protocol/vectors/` only.

## 4. Daemon architecture

### 4.1 Process and tasks

One process, tokio multi-thread runtime, `#![forbid(unsafe_code)]` in every crate.

| Task | Owns | Talks through |
|---|---|---|
| main | config, identity, shutdown signal (`watch<bool>`), signal handling (SIGINT, SIGTERM, SIGUSR1) | spawns the rest |
| sway supervisor | both sway connections, the current snapshot | in: `mpsc<ActionRequest>` (32). out: `watch<Arc<Published>>` |
| listener (one per listen address) | `TcpListener`, peer filter | asks `Security::admit`, spawns sessions |
| session (one per client) | TLS stream, handshake state, subscriptions | reads `watch`, sends `ActionRequest`, watches revoke, eviction and shutdown |
| admin | Unix socket `admin.sock` | calls pairing and device store |

### 4.2 Source modules

```
crates/swayconnect-daemon/src/
  main.rs         clap entry and subcommands; the only file that reads the environment
  lib.rs          `pub mod` lines and re-exports only. No types, no logic.
  daemon.rs       Daemon::start(config, Paths, SwayTarget), DaemonHandle
  config.rs       TOML config, validation
  paths.rs        Paths value, file mode and owner checks, atomic write, own uid
  security.rs     seam type Security and its handshake types (plan section 5)
  identity.rs     certificate and key: create, load, fingerprint, rustls ServerConfig
  devices.rs      device store (devices.json), one revoke signal per device
  pairing.rs      pairing window, pending device, confirmation
  throttle.rs     connection bucket per address, slots before `welcome`, action bucket per device
  admin.rs        admin socket server and client
  listener.rs     accept loop, peer filter, TLS accept
  session.rs      handshake and per-connection loop
  dispatch.rs     operation table, scope check, read-only check, rate limit, argument validation
  sway_handle.rs  seam type SwayHandle, Published, Action, ActionError (plan section 5)
  supervisor.rs   sway connect, subscribe, refresh, reconnect
  model.rs        pure function: sway tree + workspaces + version to Desktop
  actions.rs      typed action to validated sway command
```

`rustls::ServerConfig` in `identity.rs`: TLS 1.3 only, ALPN `swayconnect/1`, `session_storage = Arc::new(NoServerSessionStorage {})`, `send_tls13_tickets = 0`, `max_early_data_size = 0`. Field names read from the rustls 0.23.46 source; not compiled yet.

### 4.3 Sway IPC (`swayconnect-sway`)

1. Framing is written by hand on `tokio::net::UnixStream`: `"i3-ipc"`, `u32` length, `u32` type, native byte order. A dedicated reader task reads with `read_exact`, so no read is cancelled half way.
2. Own serde types hold only the fields the daemon uses. Every field has `#[serde(default)]`. Enum-like values are kept as `String`. A new sway value degrades. It never fails the whole message.
3. Two connections. The command connection serves `GET_VERSION`, `GET_WORKSPACES`, `GET_TREE`, `RUN_COMMAND`, one request at a time. The event connection sends `SUBSCRIBE ["workspace","window","output","shutdown"]` and then only reads.
4. Message types sent: 0, 1, 2, 4, 7. Nothing else. The frame writer takes an enum with exactly these five variants. Sway never answers an unknown type.
5. Every request has a 2 s timeout. On timeout the connection is dropped and rebuilt, because the stream may be out of step.
6. Events are triggers only. Their payload is not parsed beyond the type. Any event sets a dirty flag.
7. Refresh: 50 ms after the first dirty flag, the supervisor runs `GET_TREE` and `GET_WORKSPACES` (the tree lacks `visible`; checked on sway 1.12), builds a `Desktop`, and publishes it if it differs from the last one. `rev` grows by one per publish.
8. The event reader never waits for a TCP client. Sway can drop a slow IPC reader.

Socket path. The daemon is given a `SwayTarget`:

```rust
pub enum SwayTarget { Explicit(PathBuf), Discover(EnvSnapshot) }
pub struct EnvSnapshot { pub swaysock: Option<PathBuf>, pub xdg_runtime_dir: Option<PathBuf>, pub uid: u32 }
pub fn resolve(target: &SwayTarget) -> Option<PathBuf>   // socket.rs; pure except for file system reads
```

Resolved again on every connect attempt:

1. `Explicit(path)`: that path and nothing else. Config key `sway.socket` gives this.
2. `Discover`: `swaysock`, if set and connectable.
3. `Discover`: glob `<xdg_runtime_dir>/sway-ipc.<uid>.*.sock`. Keep entries owned by `uid` whose pid part is alive (`/proc/<pid>` exists). Take the newest.

`resolve` never reads the environment. Only `main.rs` builds an `EnvSnapshot` from the real environment (9.2). `I3SOCK` is ignored. The socket path contains the sway pid, so a sway restart changes it. Step 3 finds the new one.

Reconnect: on EOF, I/O error, timeout or `shutdown` event, publish a snapshot with `sway.online = false` and empty lists. Retry after 250 ms, doubling to 5 s. Then subscribe and take a full snapshot. The daemon keeps running while sway is away.

### 4.4 Actions and the command builder

This is the core safety property. No function in the code base takes a sway command as text from outside `swayconnect-sway`. No string of any origin is placed into a command: only integers and one boolean.

| Protocol operation | Check in `actions.rs` | Command text from `swayconnect-sway` |
|---|---|---|
| `workspace.focus {number}` | integer 0 to 999 | `workspace --no-auto-back-and-forth number <n>` |
| `workspace.focus {workspace_id}` | id equals `id` of an entry in `workspaces`, else `not_found`. The entry's number is 0 to 999 and no other workspace in the snapshot has that number, else `unsupported_target`. | `workspace --no-auto-back-and-forth number <n>` |
| `window.focus {window_id}` | id equals `id` of an entry in `windows`, else `not_found` | `[con_id=<id>] focus` |
| `window.fullscreen {window_id, enabled}` | id equals `id` of an entry in `windows`, else `not_found` | `[con_id=<id>] fullscreen enable` or `[con_id=<id>] fullscreen disable` |

Rules:

1. One command per `RUN_COMMAND`. No `;`, no `,`.
2. No workspace name is ever placed in a command. A workspace without a usable number cannot be focused by id in stage 1 (`unsupported_target`). The owner's desktop uses numbered workspaces only (checked read-only on 2026-11-13).
3. Window ids and workspace ids are one `con_id` space in sway. `actions.rs` keeps two separate id sets. A workspace id sent as `window_id` is `not_found`. Without this, `[con_id=<workspace id>] fullscreen` would hit a whole workspace.
4. `--no-auto-back-and-forth` makes the command idempotent even when the owner has `workspace_auto_back_and_forth` on. The flag is in `man 5 sway` of sway 1.12.
5. Reply check: the reply array must have exactly one element with `success: true`. Anything else is `sway_error`. This is defensive: the reply shape is first seen on a real sway in the owner-run step 9.4 step 5.
6. A unit test asserts every rendered command matches this pattern and nothing else:
   `^(workspace --no-auto-back-and-forth number \d{1,3}|\[con_id=\d+\] (focus|fullscreen (enable|disable)))$`

### 4.5 State model

`model.rs` is a pure function. Input: tree JSON, workspaces JSON, version. Output: `Desktop` as defined in `docs/PROTOCOL.md` section 9.

- Skips output `__i3` and workspace `__i3_scratch`.
- A window is a tree node of type `con` or `floating_con` that has `pid` or `app_id` or `window` set and has no child nodes.
- `app_id`: sway `app_id`, else `window_properties.class`, else null.
- `number`: sway `num`. A value below 0 becomes `null` (sway reports -1 for a workspace with no number, `man 7 sway-ipc`).
- `fullscreen`: `fullscreen_mode != 0`. `floating`: the node, or a container above it, is in a `floating_nodes` list or has the type `floating_con`. Sway gives that type only to the top container of a floating group; the windows of a floating split have the type `con`.
- Text: in `title`, `app_id`, workspace `name` and `output`, every control or format character (`docs/PROTOCOL.md` 2.4) is replaced by U+FFFD. Then the length is cut: title to 200 scalars, the others to 128. Applications set these strings. A web page sets a browser title.
- Count cuts: 512 windows, 256 workspaces.
- Size cut: if the `Desktop` encoded with titles is over 262144 bytes, drop windows from the end, then workspaces from the end, until it fits.
- `truncated` is set when any list was cut.
- No pids, no rectangles, no marks. They are not needed and they are extra data on the LAN.

The published snapshot is the cut one. `actions.rs` checks ids against it, so an id that was cut is `not_found`.

`Published` holds `rev` and the `Desktop`. A session serialises it per send. For a device without `state.titles` it sets every `title` to null first. At most 16 clients exist, so a shared pre-serialised copy is not worth a frozen seam.

The text functions live in `swayconnect-protocol` (`text.rs`): `is_control_or_format(char)`, `replace_unsafe(&str) -> String`, `escape_for_terminal(&str) -> String`. The daemon and the CLI both use them.

### 4.6 Sessions

1. TCP accept. `Security::admit(peer)` checks `allow_from`, the total cap and the connection bucket of the peer address. On refusal close at once, before TLS. It then takes a slot in the "before `welcome`" set (8 slots). If the set is full, one connection in it is told to close: the oldest one of the peer address that holds the most slots, the newcomer counted. So a flood from one address evicts its own connections, not a handshake from another address. All loopback peers are one address here.
2. TLS accept, 5 s deadline. Check ALPN.
3. Send `hello`. Read one frame, 2 s deadline, 4096-byte limit.
4. Verify `pair` or `auth` in the order of `docs/PROTOCOL.md` 5.7. Failure: `reject`, close, log with the peer address. Nothing is counted. No lockout exists.
5. Send `welcome`. The connection leaves the "before `welcome`" set.
6. After `pair` the session is pending: empty scope set. It waits for the owner's answer: on yes it sends `pairing.confirmed`; on no, on the 60 s deadline or when the admin side goes away it sends `bye` with `revoked` and closes.
7. Loop over: next client frame, snapshot changed, revoke signal of this device, pairing answer, shutdown, idle timer.
8. Requests of one connection run one at a time, in order. Before each request `dispatch.rs` calls `Security::is_active(device)`; a revoked device gets `bye` with `revoked`. Each action takes one token from the device's action bucket (burst 5, refill 2 per second, shared by all connections of the device); an empty bucket is `rate_limited`.
9. Every write has a 10 s timeout. A timeout closes the session without `bye`.
10. A slow client only skips snapshots, because `watch` keeps the newest value.

Revoke signal: one `tokio::sync::watch<bool>` per device, kept in the device store. `devices.revoke` sets it to `true` under the same lock that deletes the record. A session gets its receiver inside `Admitted`, from the same lock that verified the proof, so no revoke can fall between authentication and subscription. `true` or a closed channel both mean revoked. A `watch` cannot lag and cannot lose a value. The `is_active` check in step 8 covers a session that is stuck in a write.

`throttle.rs` state is bounded: the connection bucket table holds at most 1024 addresses and evicts the oldest; action buckets exist only for stored devices.

### 4.7 Signals, shutdown and exit codes

On SIGINT or SIGTERM: stop accepting, send `bye` with `shutdown` to each session, wait up to 2 s, remove `admin.sock`, exit 0.

On SIGUSR1 (panic switch): the daemon enters read-only mode until it is restarted. Open sessions stay; their actions now return `read_only`. The switch needs no typing focus: the owner can bind it to a key in his own sway config (`bindsym ... exec pkill -USR1 -x swayconnectd`) or run `systemctl --user kill -s USR1 swayconnectd`. Sway key bindings work wherever the focus is. Agents never edit the owner's sway config.

| Exit code | Meaning |
|---|---|
| 0 | clean stop |
| 1 | runtime failure, or a listen address that cannot be bound. systemd restarts the unit. |
| 2 | config error, bad file mode or owner, refused bind address, `XDG_RUNTIME_DIR` unusable, or another daemon already listens on `admin.sock`. systemd does not restart (`RestartPreventExitStatus=2`). |

### 4.8 Logging

`tracing` to stderr. Under systemd, journald keeps it. Level from `RUST_LOG`, default info.

Logged at info: start, bound addresses, fingerprint, a firewall hint when a non-loopback address is bound, pairing window opened, pending, confirmed, refused or expired, authentication success and failure with peer address, every action with device id, rate-limit hits, revoke, read-only switch, sway connect and disconnect.

Lines that a peer can cause as often as it likes are limited to one line per 10 seconds, with the count of the lines held back (`held_back`): refused connections (one budget for the daemon), refused handshakes (one budget for the daemon), rate-limit hits (one budget per device). Without the limit any host that reaches the port could fill the journal and push the audit lines out of it.

Never logged: `S`, `K`, proofs, nonces, the pairing URI, window titles.

Every client-supplied string (`device_name`, `platform`, `client`) and every sway-supplied string is logged and printed through `escape_for_terminal`. It renders each control or format character as `\u{..}`. The protocol already refuses such characters in client strings; the escape is the second line.

## 5. Security model

### 5.1 Assets

1. A shell as the owner. Sway IPC reaches `exec`, `bindsym`, `for_window`, `include`, `reload`, `exit`.
2. Control of the desktop: focus, fullscreen. Focus decides where the owner's keystrokes go.
3. What the owner is doing: window titles (URLs, document names, chat partners) and the live activity stream.

### 5.2 Attackers and answers

| Attacker | Answer |
|---|---|
| Any LAN host, not paired | Nothing but `hello` before a valid proof. 256-bit keys. Caps, deadlines and a connection bucket per address bound the work. No lockout exists that the host could turn against the owner. |
| Passive sniffer | TLS 1.3, forward secret. No session tickets. |
| Active man in the middle | The client pins the certificate fingerprint and checks the TLS handshake signature. Proofs are bound to the fingerprint the client saw, so a relay with another certificate fails even when a client compares the pin wrongly. A relay that replays the real certificate fails at the handshake signature; a client that stubs that check is not protected (`docs/PROTOCOL.md` 3.2, case 15.13). |
| Web page in the owner's browser | Not HTTP. A browser cannot produce TLS with a custom ALPN, binary framing and HMAC proofs toward a self-signed peer. |
| Containers on `docker0`, VPN peers, tethered devices | Loopback bind by default. Wildcard bind refused. Source address must match `allow_from` when a LAN address is bound. |
| Paired but stolen or malicious phone | Typed operations only, closed command grammar, scopes, action rate limit, immediate revoke, panic switch, audit log. With default scopes the worst case is: change focus, toggle fullscreen, read titles. Focus control lets the device steer where the owner's keystrokes go and can disturb the desktop within the action rate limit. No code execution path exists in version 1. |
| Someone photographs the pairing QR, or another phone app takes the pairing link | The pairing stays pending until the owner confirms the 6-digit code at the desktop (`docs/PROTOCOL.md` 5.4). A stranger who pairs first shows up as a pending device whose code is not on the owner's phone. The owner answers no. A pending device can only `ping`. |
| Another LAN host spams bad `pair` attempts | Bad attempts cannot close the window. The per-address connection bucket bounds the work. |
| Another local user on the desktop | The admin socket directory is checked on both ends (5.4). The CLI reads the pairing URI from stdin, never from the argument list. A loopback peer still needs a valid proof. |
| Hostile text (window title set by a web page, device name, client name) | Control and format characters are refused in client strings and replaced in sway strings. Every print path escapes again (4.5, 4.8, 7.2). |
| Code already running as the owner | Out of scope. It already has `SWAYSOCK`. |

### 5.3 Rules that must never be broken

Amended for v2 by 11.3 (approved by the owner on 2026-11-14). The v1 text stays here for reference; 11.3 and `AGENTS.md` section 5 hold the rules in force.

1. No operation accepts a sway command, a shell command or a file path.
2. No string is placed into a sway command. Integers and one boolean only.
3. Authentication cannot be turned off. No config key, no flag, no environment variable.
4. Pairing, pairing confirmation, revoke and scope assignment are reachable only through the admin socket. Never over TCP.
5. Only sway message types 0, 1, 2, 4, 7 are ever sent. A unit test on the frame writer proves that no other type can be built. `GET_CONFIG` in particular is never sent.
6. Secrets are never logged and never sent.
7. A pairing never becomes active without the owner's yes.

### 5.4 Admin socket

- `XDG_RUNTIME_DIR` MUST be set, absolute, owned by the daemon's uid and mode 0700. Otherwise exit 2. There is no fallback path.
- Directory: `$XDG_RUNTIME_DIR/swayconnect/`, created with mode 0700. It is refused (exit 2) if it exists as a symlink, with another owner or with a looser mode.
- Socket: `admin.sock` in that directory, mode 0600. A stale `admin.sock` is unlinked only if it is a socket owned by the daemon's uid and nothing listens on it. The daemon tries to connect first: when a daemon answers, the start fails with "already running" (exit 2) and the socket stays, so the running daemon can still be told to revoke. The daemon binds `admin.sock` after its TCP listeners, so a start that fails on a listen address leaves no socket file behind.
- The server checks the peer uid (`UnixStream::peer_cred`) against its own uid.
- The admin client (`swayconnectd pair`, `devices`, `revoke`) applies the same directory checks and checks the server's peer uid before it sends a request. A planted socket cannot hand the owner a pairing URI for another server.
- One JSON object per line. Internal and unversioned. Daemon and subcommands ship in the same binary, so they always match.
- Operations: `pair.open`, `devices.list`, `devices.revoke`.
- `pair.open` returns the URI. The connection stays open. When a device is pending, the daemon sends its `device_name`, `platform`, `client`, peer address and `confirm_code` on the same connection and reads the owner's yes or no from it. Then it sends the result.
- Closing the connection cancels an open window and refuses a pending device.

### 5.5 Files and storage

Config: `$XDG_CONFIG_HOME/swayconnect/config.toml` (default `~/.config/swayconnect/`).
Data: `$XDG_DATA_HOME/swayconnect/` (default `~/.local/share/swayconnect/`), mode 0700.

| File | Mode | Content |
|---|---|---|
| `identity.cert.der` | 0600 | self-signed ECDSA P-256 certificate, created at first run with `rcgen` |
| `identity.key.der` | 0600 | its private key, PKCS#8 |
| `devices.json` | 0600 | `{"version":1,"devices":[{"id","name","platform","client","key","scopes","created_at"}]}` |

Certificate content. The certificate goes to every unauthenticated peer, so it says nothing about the host: subject and issuer CN = `swayconnectd`, no subjectAltName, no host name, notBefore = 2000-01-01T00:00:00Z, notAfter = 9999-12-31T00:00:00Z, random 128-bit serial. The creation time is not in it.

Rules:

1. Writes go to a temp file in the same directory, then `fsync`, then rename.
2. At start the daemon refuses (exit 2) a data directory or file that is a symlink, has another owner, or has a looser mode.
3. The daemon is the only writer. Admin commands go through the socket.
4. `key` is the device key `K` in base64url. It is stored in the clear because HMAC needs the key itself. Someone who can read this file is already the owner.
5. Deleting the two identity files unpairs every device.
6. Only confirmed devices are written. A pending device lives in memory.
7. `client` is stored from `pair` only. `auth.client` and "last seen" are kept in memory and shown by `swayconnectd devices`.

Paths are a value, `Paths { config_file, data_dir, runtime_dir }`. `main.rs` builds it from the XDG variables by hand; tests build it from temp directories (9.2). No `directories` crate.

OS facts without a new crate and without `unsafe`:

| Fact | Method |
|---|---|
| own uid | `tokio::net::UnixStream::pair()`, then `peer_cred()?.uid()` on one end |
| file owner and mode | `std::os::unix::fs::MetadataExt` |
| sway pid alive | `/proc/<pid>` exists |
| hostname | read `/proc/sys/kernel/hostname`, trim, `replace_unsafe`, cut to 64 bytes |

### 5.6 Known limits (honest list)

1. The proof handshake is small and uses only HMAC-SHA256, but it is written for this project and not reviewed by a third party. Vectors in the spec pin every value.
2. The Android pairing link uses a custom URI scheme. Another app on the phone could register the same scheme and receive the pairing secret. Its pairing would then be pending with a code that the owner's app does not show, and the owner answers no (D17). Paste is still the safer path.
3. Android stores `K` encrypted under an AndroidKeyStore AES key. On a rooted phone the app's own process can still be abused. A non-exportable Keystore HMAC key is on the roadmap.
4. `state.titles` exposes window titles to the paired device. Pair with `--scopes state.read,control` to withhold them.
5. The `RUN_COMMAND` reply shape is not verified on a real sway until the owner-run step 9.4 step 5. The daemon treats every unexpected reply as failure.
6. The Android app, with its pin check, was built and run on one emulator only (API 37, Android's Conscrypt TLS stack; see 9.5). No physical phone, no LAN address and no Android below API 37 was tried. It stays unverified on a real phone until the owner has done step d of section 1.
7. A paired device with `control` can move focus up to the action rate limit. It can so redirect keystrokes for a moment. The answers are the rate limit, the panic switch and revoke.
8. A LAN attacker can still deny service (flood within the caps, ARP tricks). Stage 1 protects confidentiality and control, not availability against the LAN.
9. The 8 slots before `welcome` are shared fairly between peer addresses, not between processes. A local process of another user can open loopback connections faster than a loopback client finishes its handshake, and so keep `swayconnect` on the same machine out. A LAN attacker with many source addresses inside `allow_from` can do the same to a phone. TCP gives the daemon nothing to tell such peers apart before the proof. Not fixed in stage 1.
10. The total cap of 16 connections has no share per device. One paired device can hold all 16 with authenticated sessions; every other device is then refused at accept. The answer in stage 1 is `swayconnectd revoke`, which uses the admin socket and so still works. A cap per device needs its own `bye` or `reject` code, which is a wire change; it is on the roadmap.
11. Android 10 (API 29, the `minSdk`) still has the task-affinity flaw known as StrandHogg: another installed app can put an activity of its own into the task of this app and show a false pair screen. Android 11 and later are patched. The platform's answer is `minSdk` 30; raising it is the owner's decision. Until then, check the pair screen on an Android 10 phone with care.
12. The pair screen shows the pairing link, with its secret, as readable text until the attempt ends. The keyboard is told not to learn it (password input type). Since v2 M1 the pair screen sets `FLAG_SECURE` while it is shown, so a screenshot, a screen share or the recents thumbnail cannot hold the link (checked on the emulator: a capture is black). The owner's code check (D17) still stands between a leaked link and a paired device.
13. A pairing does not survive a switch to another app on current Android. The connection of a pairing attempt lives in the view model and outlives a rotation or a screen timeout, but Android closes the sockets of an app a few seconds after it left the foreground (seen on API 37 for a session; the pairing connection was not tried). The confirm screen says to keep the app open. A foreground service would fix it and is out of scope.
14. A QR scanner that starts the link with category `BROWSABLE` finds no handler, because the intent filter leaves that category out on purpose (8.2). No scanner was tried. Paste always works.
15. The link field reads its text back from the view model. Fast typing can lose a character there (the device name field keeps its text in the composition for that reason). Paste and the link intent are not affected, and nobody types 150 characters of base64 by hand.

## 6. Configuration

File: `$XDG_CONFIG_HOME/swayconnect/config.toml`. All keys are optional. An unknown key is a start error (exit 2), so a typo cannot silently weaken a setting. `SWAYCONNECT_CONFIG` overrides the path; it must be absolute.

```toml
# Label shown to clients after authentication. Default: system hostname.
# 1 to 64 bytes, no control or format characters.
server_name = "desk"

# Addresses to listen on. Default: ["127.0.0.1:28414"].
# "0.0.0.0", "::" and any other wildcard are refused.
listen = ["127.0.0.1:28414", "192.168.1.200:28414"]

# Source networks allowed to connect to non-loopback listen addresses.
# Required as soon as "listen" has a non-loopback address. Loopback peers are always allowed on loopback.
allow_from = ["192.168.1.0/24"]

# true: every action is refused with error "read_only". State can still be read.
read_only = false

[sway]
# Empty: discover (SWAYSOCK, then glob in XDG_RUNTIME_DIR). Set: use only this path.
socket = ""
```

That is the whole config. Pairing lifetime and scopes are flags of `swayconnectd pair`. The log level is `RUST_LOG`. The connection cap is fixed at 16.

Validation at start: every listen address parses and is not a wildcard; `allow_from` entries are valid CIDR; `allow_from` is not empty when a non-loopback address is listed; `server_name` is within its limits. A listen address that cannot be bound (for example the LAN address is not up yet at login) is exit 1; systemd starts the daemon again.

Firewall. A host firewall must also allow the port. This machine runs UFW and drops inbound LAN TCP (seen in the journal). Without a rule the phone only times out, and nothing tells the user why. The owner runs this once; agents do not:

```
sudo ufw allow from 192.168.1.0/24 to 192.168.1.200 port 28414 proto tcp
```

The daemon logs a hint with the port when it binds a non-loopback address, and `swayconnectd pair` prints the same hint when the URI host is not loopback.

systemd user unit (`packaging/systemd/swayconnectd.service`): `Type=simple`, `ExecStart=%h/.cargo/bin/swayconnectd run`, `Restart=on-failure`, `RestartSec=5`, `RestartPreventExitStatus=2`, `StartLimitIntervalSec=0`, `NoNewPrivileges=yes`, `WantedBy=default.target`. No start limit, because the LAN address can come up long after login. No `SWAYSOCK` import is needed, because the daemon finds the socket by glob.

## 7. Command line tools

### 7.1 Daemon binary `swayconnectd`

| Command | Does |
|---|---|
| `swayconnectd run` (default) | runs the daemon in the foreground |
| `swayconnectd pair [--scopes a,b] [--ttl SECONDS] [--host HOST]` | opens a pairing window and prints a terminal QR code and the URI. When a device presents the secret it prints `pending: <name> (<platform>) from <address>` and `code: <6 digits>`, then asks `Confirm this device? [y/N]` (60 s, default no). Only a line typed after the device was shown is an answer: a `y` typed ahead confirms nothing, because whoever presents the secret first would get it. It ends with `paired: <name> id <id> scopes <...>`, `refused` or `window expired`. |
| `swayconnectd devices` | lists devices: id, name, platform, client, scopes, created, last seen, live connections |
| `swayconnectd revoke <device_id>` | deletes the device and drops its live sessions |

`pair` picks the URI host this way: `--host` if given, else the first non-loopback listen address, else `127.0.0.1`. `--ttl` is 30 to 600, default 300. All but `run` talk to the running daemon over the admin socket and fail with a clear message when it is not running.

The fingerprint is logged at start and is inside the pairing URI. There is no `status` or `fingerprint` command in stage 1; the journal shows bound addresses, the sway connection and the read-only flag.

Every device string is printed through `escape_for_terminal` (4.8).

Panic switch: SIGUSR1, see 4.7.

### 7.2 Client binary `swayconnect`

The reference client and the test tool. It speaks only the wire protocol.

| Command | Operation |
|---|---|
| `pair [--name TEXT]` | reads the pairing URI as one line from stdin. Runs the pairing flow, prints the confirm code, waits for `pairing.confirmed`, saves the profile, replacing an older one. A URI given as an argument is refused with exit 2, because other local users can read process arguments. |
| `info` | prints the `welcome` data: server, scopes, ops, topics, limits |
| `state` | `state.subscribe` for `desktop`, prints the response in full, closes |
| `workspaces`, `windows` | table views of the same state |
| `workspace <number>` or `workspace --id <id>` | `workspace.focus` |
| `focus <window_id>` | `window.focus` |
| `fullscreen <window_id> on\|off` | `window.fullscreen` |
| `watch` | `state.subscribe`, one line per `state.changed`, sends `ping` every 30 s. A `ping` with no response after 10 s ends the command with exit 3. |
| `raw '<json>'` | authenticates, sends the text as one frame, prints every reply for 2 s. For negative tests by hand. |

Global flags: `--json`, `--timeout SECONDS` (default 10).

Profile file: `$XDG_DATA_HOME/swayconnect/client/default.json`, mode 0600, content `{host, port, fp, device_id, key, server_name}`. One profile. `SWAYCONNECT_CLIENT_DIR` overrides the directory; it must be absolute. Tests use it to hold several devices.

Output safety: the client replaces control or format characters again when it decodes a state (`replace_unsafe`), so the table views and `--json` are both clean. Any other server text (`server.name`, error messages) is printed through `escape_for_terminal`.

| Exit code | Meaning |
|---|---|
| 0 | success |
| 1 | the server answered with an error. The code is printed. |
| 2 | usage or profile error |
| 3 | connection failure or fingerprint mismatch |
| 4 | authentication or pairing rejected, or pairing not confirmed |

### 7.3 Rust dependencies

Versions from the crates.io check of 2026-11-12 (`docs/RESEARCH.md`). The root `Cargo.toml` declares them once in `[workspace.dependencies]`.

| Crate | Version | Used by | Note |
|---|---|---|---|
| tokio | 1.54 | sway, daemon, cli, testkit | features: `rt-multi-thread`, `macros`, `net`, `io-util`, `time`, `sync`, `signal` |
| rustls | 0.23.46 | cli, daemon | `default-features = false`, features `ring`, `std`, `logging`. No `tls12`, no `brotli`, no `aws-lc-rs`. |
| tokio-rustls | 0.26.6 | cli, daemon | `default-features = false`, feature `ring` |
| rcgen | 0.14.10 | daemon | certificate creation. Tests use the certificate and keys of the vectors instead. |
| ring | 0.17.14 | protocol | HMAC-SHA256, SHA-256, random, constant-time verify. Already in the tree through rustls. |
| base64 | 0.23 | protocol | `URL_SAFE_NO_PAD`; it rejects non-zero trailing bits by default |
| serde, serde_json | 1.0 | all | no `raw_value` feature |
| toml | 1.2 | daemon | config |
| clap | 4.6 | daemon, cli | derive |
| thiserror | 2.0 | library crates | typed errors |
| anyhow | 1.0 | `main.rs` of the two binaries only | |
| tracing, tracing-subscriber | 0.1, 0.3 | daemon | |
| qrcode | 0.14.1 | daemon | terminal QR for `pair` |
| tempfile, assert_cmd, predicates | 3, 2, 3 | dev | |

Not used: `swayipc*`, `tokio-tungstenite`, `tokio-util`, `snow`, `mdns-sd`, `directories`, `rustix`, `futures-util`, `hmac`, `sha2`, `subtle`, `rand`, any Unicode table crate (the character list of `docs/PROTOCOL.md` 2.4 is a fixed set of ranges).

`cargo deny check` runs in CI. rustls had five advisories in September and October 2026; which release fixes each was not confirmed, so the newest 0.23.x is pinned and `cargo deny` must pass before any release.

## 8. Android app (`clients/android`)

The app was written before this machine had an Android SDK. Since 2026-11-13 it is built here (Temurin 25, Gradle 9.8.0, `platforms;android-37.0`) and was run on an emulator; 9.5 says what that covered. Design goal: most logic in a pure JVM module that plain JUnit can test, and the thinnest possible Android layer.

Local gate, from `clients/android`, under the lock of the plan's section 1.2: `./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug --max-workers=4`.

### 8.1 Build setup

Copy these files unchanged from the sibling project `scenedeck-android`: `gradlew`, `gradlew.bat`, `gradle/wrapper/*` (Gradle 9.8.0), `gradle.properties`, `.editorconfig`, and the `pluginManagement` and `dependencyResolutionManagement` blocks of `settings.gradle.kts`.

The version catalog is a subset of that sibling's catalog. Nothing is bumped. One entry differs: `androidx-compose-material3 = { group = "androidx.compose.material3", name = "material3" }` with no version, so the BOM decides, as sibling `airgradient-android` does. The `scenedeck-android` pin `1.5.0-alpha29` is **not** copied.

| Item | Version | Source |
|---|---|---|
| Gradle wrapper | 9.8.0 | sibling wrapper file |
| Android Gradle Plugin | 9.4.1 | sibling catalog |
| Kotlin (`kotlin.jvm`, `kotlin.plugin.compose`, `kotlin.plugin.serialization`) | 2.4.20 | sibling catalog |
| Compose BOM | 2026.09.00 | sibling catalog |
| androidx.activity:activity-compose | 1.13.0 | sibling catalog |
| androidx.lifecycle (runtime-compose, viewmodel-compose) | 2.11.0 | sibling catalog |
| androidx.core:core-ktx | 1.19.1 | sibling catalog |
| kotlinx-coroutines (core, android, test) | 1.11.0 | sibling catalog |
| kotlinx-serialization-json | 1.11.0 | sibling catalog |
| JUnit | 4.13.2 | sibling catalog |
| compileSdk / targetSdk / minSdk | 37 / 36 / 29 | see below |
| JVM target / CI JDK | 17 / 21 (Temurin) | sibling |

- minSdk 29: TLS 1.3 is on by default from API 29, and `SSLParameters.setApplicationProtocols` and `SSLSocket.getApplicationProtocol` (ALPN) exist from API 29.
- targetSdk 36: apps that target 37 need the runtime permission `ACCESS_LOCAL_NETWORK` for any LAN connection. That is debt for the later bump to 37.
- AGP 9 has built-in Kotlin. `org.jetbrains.kotlin.android` must **not** be applied.
- Lint: `:app` does **not** set `warningsAsErrors` in stage 1 (the sibling does). Lint errors fail the build. Three warnings are left on purpose: `OldTargetApi` (targetSdk 36, see above), `MissingApplicationIcon` (no launcher icon yet) and `UseKtx`.

Not used in stage 1: OkHttp, Ktor, Hilt, KSP, Room, DataStore, Navigation, CameraX, ML Kit, zxing, security-crypto, Turbine, ktfmt, detekt, Roborazzi. Each one is a plugin, a library or a code generator that stage 1 does not need.

### 8.2 Modules

`:protocol` — plugins `java-library`, `kotlin.jvm`, `kotlin.plugin.serialization`. No Android import. Package `com.swayconnect.protocol`.

| File | Content |
|---|---|
| `Messages.kt` | `@Serializable` message classes. `Json { ignoreUnknownKeys = true; explicitNulls = true }` |
| `Desktop.kt` | `Desktop`, `Workspace`, `Window` |
| `Profile.kt` | `@Serializable` profile: host, port, fp, device id, server name. Every serializable class of the project is in this module. |
| `FrameCodec.kt` | length-prefixed read and write over `InputStream` and `OutputStream`, limits enforced before reading |
| `Proofs.kt` | the proof functions and `confirm_code`. They take the key as `ByteArray`. |
| `PairingUri.kt` | parser with every rule of the spec. It does not use `android.net.Uri` (that class turns `+` into a space). |
| `Fingerprint.kt` | SHA-256 of DER, base64url helpers with the canonical check `encode(decode(x)) == x` |
| `Text.kt` | the character list of spec 2.4, `replaceUnsafe` |
| `Limits.kt` | constants from spec section 14 |
| `Transport.kt` | interface: `open(host, port, pinnedFp): Connection` giving streams and the observed fingerprint |
| `SwayConnectClient.kt` | coroutines: handshake state machine with the pending step, one reader coroutine, request map by id, `StateFlow<Desktop?>`, ping timer, a 10 s deadline for `hello` and `welcome`. It refuses a `welcome` whose `max_frame_s2c` is above 1048576. Nonce source and the three waits are injectable for the tests. |

Its JUnit tests replay `protocol/vectors/v1` through a test resource directory that points at `../../../protocol/vectors`.

`:app` — plugins `android.application`, `kotlin.plugin.compose`. It declares no `@Serializable` class, so it does not need the serialization plugin. Package `com.swayconnect.android`.

| File | Content |
|---|---|
| `MainActivity.kt` | single activity, handles the `swayconnect://pair` intent |
| `AppContainer.kt` | manual dependency wiring |
| `net/PinnedTrustManager.kt` | `X509ExtendedTrustManager`. All three `checkServerTrusted` overloads (`(chain, authType)`, `(chain, authType, Socket)`, `(chain, authType, SSLEngine)`) run the same pin check: SHA-256 of `chain[0].encoded`, compared with `MessageDigest.isEqual`. They throw `CertificateException` on mismatch or an empty chain. All three `checkClientTrusted` overloads throw. `getAcceptedIssuers()` returns an empty array. Annotated `@SuppressLint("CustomX509TrustManager")` with a comment that names the pin. |
| `net/TlsTransport.kt` | a new `SSLContext` for `TLSv1.3` per connection (so no session is resumed), `enabledProtocols = ["TLSv1.3"]`, ALPN `swayconnect/1`, check that `applicationProtocol` equals `swayconnect/1` after the handshake, no hostname check, socket work on `Dispatchers.IO`. The platform checks the handshake signature; no code here touches that. |
| `data/SecretsStore.kt`, `data/KeystoreSecretsStore.kt`, `data/AesGcmCipher.kt` | sibling pattern: AES-256-GCM, key in AndroidKeyStore, cipher class free of Keystore so JUnit can test it |
| `data/ProfileStore.kt` | one profile in a private JSON file, encoded with the `:protocol` class; `K` through `SecretsStore` |
| `ui/MainViewModel.kt`, `ui/UiState.kt` | one `ViewModel`, immutable `UiState` as `StateFlow` |
| `ui/PairScreen.kt`, `ui/DesktopScreen.kt`, `ui/theme/Theme.kt` | Compose screens |

On Android (Conscrypt) an `SSLSocket` with an `X509ExtendedTrustManager` calls the three-argument `checkServerTrusted(chain, authType, Socket)`. A class that pins only in the two-argument form and throws in the rest fails every connection. That is why all three run the check.

Manifest: permission `INTERNET` only. `android:allowBackup="false"`. No cleartext flag. One intent filter for scheme `swayconnect`, host `pair`, with action `VIEW` and category `DEFAULT` only. No `BROWSABLE`, so a web page cannot fire it.

### 8.3 Screens

1. **Pair.** Shown when no profile exists or a `swayconnect://pair` link arrives. Paste field for the URI. Shows host, port and a short form of the fingerprint. Device name field, pre-filled with `Build.MODEL`. Button "Pair". One error line.
   - Supported transfer: scan the terminal QR with the system camera or any QR reader, then either follow the link (opens the app) or copy the text and paste it. Whether a given scanner opens a custom scheme is not verified; paste always works.
   - A link only fills the form. Nothing connects until the user taps Pair.
   - The help text names the command: "On the computer, run: swayconnectd pair".
   - A text that is not a whole link gives one plain error line. The parser's own message stays out of the UI.
   - After `welcome` the screen shows the confirm code in large digits. The text below says where the code appears on the computer, what to do when the codes differ (answer no, tap Cancel) and to keep the app open until the computer answers. It waits for `pairing.confirmed`.
   - If a profile exists, the screen shows "This replaces <server name>" and needs a second confirmation. A link that arrives while that dialog is open closes it: the confirmation was for the link of that moment. The old profile and key are deleted only after `pairing.confirmed` of the new pairing. The system back gesture returns to the desktop screen.
2. **Desktop.** Home screen. A connection banner on top: Connecting, Authenticating, Ready, Sway offline, Reconnecting. Workspace chips with focused, visible and urgent marks; tap focuses. Below, the windows of the selected workspace; tap focuses. Long press opens a sheet: fullscreen on or off. A control is shown only when its operation is in `welcome.ops`. An overflow menu shows server name, short fingerprint, granted scopes, the read-only flag and "Forget this computer"; a revoked device has no menu.
   - When the connection offers no action at all, one neutral line under the banner says why: read-only mode, or a device without the scope `control`. The banner list stays as it is.
   - Marks: the focused window has a tinted row and a heavier title, an urgent one a red dot; a focused or urgent workspace chip has a dot. The words stay next to the marks. The fill of a chip means "its windows are listed", not "focused".
   - Empty states have a line each: no windows on the workspace, sway not running, waiting for the next attempt (with "Retry now").
   - The line about the last action stands under the list, so that it moves no tap target. A new connection clears it.

Navigation is a plain `when` over an enum in `UiState`. No navigation library.

### 8.4 Behaviour

- Connection states: `Disconnected`, `Connecting`, `Authenticating`, `Ready`, `Backoff` (1 s doubling to 30 s).
- The view model owns the one connection. The session of the stored pairing needs a started activity (`repeatOnLifecycle(STARTED)` counts them): background means disconnect. A pairing attempt does not need one: its connection stays open over a rotation or a screen timeout, because the first proof uses up the pairing window on the computer. No foreground service, so Android itself ends a pairing when the app leaves the foreground for more than a few seconds (5.6 item 13).
- A client must not be silent for more than 30 s (`docs/PROTOCOL.md` section 11 rule 3). The app pings every 20 s, also while a pairing waits for the owner's answer.
- `hello` and `welcome` must arrive within 10 s. After that the app closes the connection and treats it like a lost one. The socket has no read timeout after TLS, and the ping and request timers start only after `welcome`.
- The UI renders server state only. A tapped item shows a spinner until its answer or the next `state.changed`. A tap on an item that still waits sends nothing.
- "Retry now" on the waiting screen ends the backoff wait.
- 10 s request deadline. No automatic retry of an action. `rate_limited` and `read_only` are shown as one line of text.
- `bye` with `revoked` or `reject` with `auth_failed`: stop reconnecting, show "This device is no longer paired", offer to pair again.
- A fingerprint mismatch is shown as its own error, not as a network failure.
- A connection that is closed before TLS (cap or bucket) is retried with the normal backoff. It never leads to a lockout on the server.

## 9. Test strategy

**Rule: no test and no agent changes the owner's sway session. Automated tests never connect to it at all.**

### 9.1 Fake sway is the only automated backend

`swayconnect-testkit` has a fake sway IPC server on a Unix socket in a temp directory. It speaks the 14-byte `i3-ipc` framing with its own, separate implementation. It serves scripted replies, injects events on demand, can stop answering, can close the connection, and records every `RUN_COMMAND` payload.

Why a fake and not a real sway:

1. It is deterministic and fast and runs in CI.
2. Tests assert the exact command text. That is the injection test.
3. It needs no compositor process, no seat and no GPU. It cannot reach the real desktop.

Cost: real sway behaviour for `RUN_COMMAND` replies stays unverified by automation. The daemon is defensive there (4.4). The owner-run step 9.4 step 5 is the real check.

### 9.2 Guards in code

1. No code reads the process environment outside `main.rs` of the two binaries. `Daemon::start(config, Paths, SwayTarget)` takes explicit values. `socket::resolve` takes an `EnvSnapshot` value. Only `main.rs` builds `Paths` and `EnvSnapshot` from the real environment. A test scans `crates/*/src/**/*.rs` except the two `main.rs` files and fails on `std::env::var`, `env::var_os` or `env::vars`. (Edition 2024 makes `std::env::remove_var` unsafe and every crate forbids unsafe code, so "scrub the environment in the test process" could not be written anyway.)
2. In-process tests get their inputs from `TestDirs`: a `Paths` under one temp directory and a `SwayTarget::Explicit` that points at the fake sway. The testkit offers no way to build `SwayTarget::Discover`. Discovery itself is tested in `swayconnect-sway` with `EnvSnapshot` literals that point into a temp directory.
3. `FakeSway::start(dir, forbidden)` takes the socket directory and a list of forbidden paths as arguments. It panics if its socket path is outside `dir` or equals a forbidden path. The test harness, not library code, passes the forbidden list.
4. Child processes (`assert_cmd`) start with `env_clear()`, then get temp `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_RUNTIME_DIR` and, for the daemon, a config file whose `sway.socket` is the fake sway path. No `SWAYSOCK`, `I3SOCK` or `WAYLAND_DISPLAY` exists in such a child.
5. Listeners bind `127.0.0.1:0`. No test binds the default port. The default port 28414 is asserted by a config unit test.
6. No sleep for readiness. Tests wait on the `watch` channel or a ready signal.

### 9.3 Layers

| Layer | Where | Covers |
|---|---|---|
| Vectors | `swayconnect-protocol/tests/`, Kotlin `:protocol` tests | the same files in both languages: fingerprint, proofs with `confirm_code`, URIs, frames, requests, handshake cases, transcripts. Read with `serde_json` and `env!("CARGO_MANIFEST_DIR")`; no testkit. |
| Text rules | `swayconnect-protocol` unit tests | every range of spec 2.4 is refused in `device_name`, replaced by `replace_unsafe`, escaped by `escape_for_terminal` |
| Command builder | `swayconnect-sway` unit tests | every action to exact text; the pattern of 4.4; the frame writer cannot build a type outside 0, 1, 2, 4, 7 |
| Sway client | `swayconnect-sway` tests against an in-test socket | pipelining, event between request and reply, timeout when no reply, bad magic, socket resolution with `EnvSnapshot` literals |
| Model | `swayconnect-daemon` unit tests | sway JSON fixtures to expected `Desktop`. Fixtures come from read-only `swaymsg -t get_tree` and `get_workspaces`, with titles replaced. Hostile titles, `num` -1, size cut. |
| Stores | `swayconnect-daemon` unit tests | loose file mode refused, symlink refused, atomic write, admin directory checks, certificate content |
| Client library | `swayconnect-cli` tests with an in-test rustls server that presents the vector certificate and key | wrong fingerprint, right certificate with the wrong key, T1 with injected nonces, wrong `server_proof`, id matching, `rev` rule |
| Daemon integration | `swayconnect-daemon/tests/` with testkit and the `swayconnect-cli` client library | pair with confirmation, pair refused, pair deadline, pending device can only `ping`, auth, wrong proof, wrong fingerprint, relay case, replayed proof, expired window, second use of a secret, bad proof leaves the window open, scope denial per operation, read-only mode, SIGUSR1-equivalent switch, rate limit, revoke drops the session, revoke during a stalled write, revoke between auth and first request, oversize frame, slow handshake, eviction of the oldest connection before `welcome`, every `handshake.json` case with its reject code, the server frames of the seven transcripts (without the members a daemon draws at random, and without `rev`), one log line per flood of refusals, connection bucket without lockout, `allow_from`, sway reconnect, sway hang, stale window id, workspace id as window id, snapshot coalescing under a stalled client |
| CLI end to end | `swayconnect-cli/tests/` with `assert_cmd` | every subcommand against a daemon child process and the fake sway, with a cleared environment |

### 9.4 Live checks on the owner's desktop

Steps 1 to 4: owner present, read-only. This is proof b of section 1.

1. Start the daemon with `read_only = true` and `listen = ["127.0.0.1:28414"]`.
2. Run `swayconnectd pair` in one terminal. Run `swayconnect pair` in a second one and paste the URI. Compare the code and answer yes.
3. Run only `info`, `state`, `workspaces`, `windows`, `watch`.
4. The owner switches workspaces by hand and watches lines appear in `watch`.

`dispatch.rs` refuses actions in read-only mode before `actions.rs` is reached. A unit test pins that order. The daemon sends only `GET_*` and `SUBSCRIBE` to sway in this mode.

Steps 5 and 6: the owner runs them himself. Agents do not. These are proofs c and d of section 1.

5. Set `read_only = false`, restart the daemon. Run `swayconnect workspace <n>` and `swayconnect focus <id>`. This is the first `RUN_COMMAND` that reaches a real sway. If the daemon reports `sway_error`, the owner sends the logged reply shape back and 4.4 rule 5 is fixed.
6. Set `listen = ["127.0.0.1:28414", "<lan-ip>:28414"]` and `allow_from`, add the firewall rule of section 6, restart. Install the debug APK from the `android.yml` run or from a local `:app:assembleDebug`. Run `swayconnectd pair`, scan the QR with the phone (follow the link or paste the text), compare the code, answer yes. Tap a workspace and a window on the phone.

### 9.5 Not automated

- Android on a device. The JVM tests (`:protocol:test`, `:app:testDebugUnitTest`) run locally and in `android.yml`; they cover the protocol vectors, the view model over fake transports, and `PinnedTrustManager` and `TlsTransport` against a plain JSSE server, including case 15.13 of the spec. They do not run on Android's own TLS stack and they draw no screen.
- Checked by hand on 2026-11-13, emulator `Medium_Phone_API_37.0` (API 37, x86_64), daemon on host loopback (`10.0.2.2`): pairing by link and by paste, the code on both sides, state from the owner's real sway through a `read_only` daemon, the four actions against the fake sway of the testkit (`crates/swayconnect-testkit/examples/fake_sway.rs`), revoke, daemon stop and restart. After the review fixes, the same run again plus: the read-only line, "Retry now" during backoff, a link that closes the open "Replace?" dialog, a link intent that the system sends again from the recents list (ignored), dark mode, and logcat without the pairing secret from the app. No agent sent an action to the real sway.
- Not checked anywhere: a physical phone, a LAN address with the firewall rule, a QR scanner, Android below API 37, a release build, TalkBack. `android.yml` has never run. Step 6 of 9.4 is the owner's.

## 10. Roadmap

| Stage | Content |
|---|---|
| 1 (this document) | daemon, protocol v1, CLI, Android app, vectors |
| 2 | Command buttons defined in daemon config and called by id, with no client arguments (covers media, volume, lock, launchers). Close window under its own scope `destructive`, off by default, with a client-side confirmation. Workspaces without a number. Move window to workspace or output. Outputs and binding mode in state. `swayconnectd status`. Several CLI profiles. Opt-in mDNS `_swayconnect._tcp` as an address hint. In-app QR scanner. Several paired computers. Device key as a non-exportable Keystore HMAC key. A cap on sessions per device, with its own `bye` code (5.6 item 10). JSON Schema generated from the Rust types. ktfmt and detekt for Android. Android: `minSdk` 30 or another answer to 5.6 item 11, a decision on `FLAG_SECURE` for the pair screen, a way to pair with another computer without a link while one is paired, scrolling the selected workspace chip into view, a launcher icon, a mark to tell windows with equal titles apart. |
| 3 | Touchpad through `zwlr_virtual_pointer_v1` under a new scope. Urgent-window notification. Context-aware button pages. WebSocket binding if a web client is wanted. targetSdk 37 with `ACCESS_LOCAL_NETWORK`. |
| Maybe | Keyboard input (equal to code execution; own scope, off by default). Output power. Thumbnails. |
| Never | Raw sway command passthrough. `exec` with client text. Remote unlock. A switch that disables authentication. Cloud relay. |

## 11. Version 2 architecture

Status: proposed, 2026-11-13 (review 2). Not implemented. Wire contract: `docs/PROTOCOL.md` section 17. UX: `docs/UX.md`. Evidence: `docs/V2_RESEARCH.md`. Order and ownership: `docs/V2_PLAN.md`.

Next action: M2a starts with C2a. The owner answered `docs/V2_PLAN.md` section 0 on 2026-11-14: 11.3 approved, no step-up, typing on at pairing (11.8, D30), deferrals kept.

### 11.1 What v2 is

Three steps. The owner asked for UI/UX first.

| Milestone | Content | Wire change |
|---|---|---|
| M1 "UI/UX" | new Android design (pill navigation, snapping workspace card carousel synced with a pager, layout mini-map, list view, app icons), daemon layout export and icon service, scope grants at the desktop (`devices grant`) | `desktop` additions (layout, rects, outputs, scratchpad, icon hash), op `icon.get`, `welcome.known_scopes` and `known_ops`, `bye scopes_changed` |
| M2a "desktop controls" | window ops, close window, lock, keep awake, audit log | 17.6, 17.7, 17.8 (topic `session`) |
| M2b "devices and typing" | audio (sinks and sources), media (play/pause, next, previous), app launch, fuzzel, remote keyboard | 17.9 to 17.13 |

Deferred to 11.13: owner buttons, per-app streams, card profiles, media seek, Live typing, presets, Android Keystore key hardening, audit rotation, `status.json`, multi-output UI groups, tablet split, map↔list morph.

### 11.2 Decisions

| # | Decision | Reason |
|---|---|---|
| D20 | Protocol stays version 1. Every v2 item is additive (13.1). | Old clients keep working. No breaking change was needed. |
| D21 | Layout travels inside topic `desktop`, as basis-point rects per workspace. | One `rev` keeps window ids and geometry consistent. 730 bytes for the live desktop. |
| D22 | Output facts come from output nodes of `GET_TREE`. `GET_OUTPUTS` stays unsent. | The tree has `rect`, `scale`, `transform`, `power`. Allowed message types stay 0, 1, 2, 4, 7. |
| D23 | While a client subscribes to `desktop`, the supervisor re-reads the tree every 2 s and on `binding` events. | Sway sends no resize event. |
| D24 | Icons: own desktop-entry and icon-theme resolver; resvg for SVG in a resource-limited child process; served by content hash only. | MPL-2.0 crates fail `deny.toml`. A hash-only op cannot be turned into a file read. Icon files are not owner-trusted (flatpak exports). |
| D25 | Audio through `pactl` (JSON snapshots, a long-lived `pactl subscribe`, argv writes with `--`). | Pure Rust side, light build, easy fake. pipewire-rs needs bindgen and clang. |
| D26 | Media through zbus (MPRIS). No album art in v2. | Typed D-Bus without libdbus. Art fetching is SSRF or arbitrary file read. |
| D27 | Keep awake: a `systemd-inhibit --what=idle --mode=block ... sleep N` child. | swayidle 1.9.1 honours logind block inhibitors. The child expires by itself even if the daemon hangs. `V2_RESEARCH.md` section 7. |
| D28 | Apps start from the daemon's own desktop-entry index under `systemd-run --user --scope`. Never through sway `exec`. | No client text in a shell. The app gets its own cgroup and survives a daemon restart. |
| D29 | Text input through a daemon-owned `zwp_virtual_keyboard_v1` client with a generated keymap. | Sway IPC cannot type. No injector is installed. Works for every app and both layouts. |
| D30 | Fine-grained scopes, granted at the desktop only. No client scope request. Owner's choice (2026-11-14): `swayconnectd pair` gives by default `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys`, `input.text`; `destructive`, `audio.capture` and `launch.risky` stay off and are granted only by name. Named presets stay deferred (11.13). | D9. A request op would be a social-engineering channel. The owner wants a new phone to type and launch without a second step. **Risk, stated plainly:** with the default set, anyone holding the unlocked, paired phone can run any command as the owner: `launcher.open` plus `input.text` types a terminal's name into fuzzel, then types commands into it. `launch.risky` off does not stop that path. What still stands: typing is refused while the screen is locked or a grabber runs, focus is checked before each 16-character part, a desktop notice shows when a remote keyboard session starts, every input session is in the audit log, and `devices deny`, `revoke` and SIGUSR1 end it. An owner who wants less pairs with `--scopes` or runs `devices deny`. |
| D31 | `K` stays the device credential ("API key"), stored as in v1. | Nothing secret crosses the wire already. Per-scope keys or tokens add nothing on one TLS connection. Keystore hardening is deferred (11.13). |
| D32 | No biometric step-up in v2 (owner, 2026-11-14, `V2_PLAN.md` question 1). | New unreviewed crypto flow. Input is granted at the desktop only (at pairing or by name). The cost is in D30. |
| D33 | Android stays on material3 1.4.0 (BOM pin) and adds no library. Expressive look is built by hand. | The catalog rule "nothing is bumped". |
| D34 | Every UI piece is Compose. No XML layouts, no View widgets, no WebView. Icons are Kotlin `ImageVector`s. One exception is allowed only if the owner approves: none is planned. | Owner direction. Private typing uses `BasicSecureTextField`, not an `EditText`. |

### 11.3 Changes to the security rules of 5.3

5.3 is amended, not worked around. **Approved by the owner on 2026-11-14** (`V2_PLAN.md` questions 2a and 2b). `AGENTS.md` section 5 carries the text. Rules 2, 8, 9 (first sentence) and 11 apply from M2a; the `input.text` exception of rule 9 and rule 10 from M2b.

| Rule | v1 text | v2 text |
|---|---|---|
| 1 | No operation accepts a sway command, a shell command or a file path. | Unchanged. |
| 2 | No string is placed into a sway command. Integers and one boolean only. | No client string is placed into a sway command. A command holds only integers, booleans and members of closed enums defined in `swayconnect-sway` (layout kinds). The one fixed command `output * power on` holds no variable at all. |
| 5 | Only sway message types 0, 1, 2, 4, 7. | Unchanged. The event subscription adds `binding`. |
| 8 (new) | | Child processes: argv only, no shell. The program is an absolute path from config (every program has a config key with a default). Arguments are integers, closed enums and daemon-owned strings (pactl names from the daemon's own snapshot, desktop entry ids and Exec tokens from the daemon's own index, config values). A daemon-owned string that comes from another program (a pactl name) goes after `--` and never starts with `-`. Environment from an allowlist. stdin null. stdout and stderr read with a cap and never sent to a client. A timeout and kill. |
| 9 (new) | | A client string is allowed in a request only when it is an id the server issued (entry id) or a closed enum, matched exactly. The one exception is `input.text`, which goes only to the virtual keyboard, never to a process, a file or sway. |
| 10 (new) | | Remote input fails closed: refused while the screen is locked or lock state is unknown, while a keyboard-grabbing surface runs, or when focus, read from sway just before each part, is not the target. |
| 11 (new) | | No client-chosen string reaches a desktop notification. Notices use fixed text and the device short id. |

The stage 1 claim "no code execution path exists" (5.2) ends with M2b: `launch`, `launch.risky` and `input.*` are code-execution equivalent. They are tier 2 and granted only at the desktop. `launch`, `input.keys` and `input.text` are in the default of `swayconnectd pair` (11.8, D30); `launch.risky` is granted only by name.

### 11.4 Daemon modules

New and changed files in `crates/swayconnect-daemon/src/`. Every module with logic is a pure function or sits behind a trait with a fake. Each feature module defines its own config section type (`#[derive(Deserialize)] #[serde(deny_unknown_fields)]`) in its own file; `config.rs` only embeds it.

```
model.rs            unchanged role; calls layout.rs and outputs; cut rule of PROTOCOL 17.4.5        M1
layout.rs           NEW pure: sway Node -> Layout (basis points, collapse, depth cap, active child)  M1
outputs.rs          NEW pure: output nodes -> Output[], scratchpad windows                           M1
supervisor.rs       + 2 s poll while subscribed, + binding events, + idle-inhibit view count         M1
icons/
  mod.rs            IconService: app_id -> Option<IconHash>; hash -> PNG bytes                       M1
  entries.rs        desktop entry parser and index (shared with apps)                                M1
  theme.rs          Icon Theme Spec lookup: Inherits, hicolor, pixmaps, size choice                  M1
  raster.rs         Rasteriser trait; ChildRasteriser runs `swayconnectd --raster` (11.5)           M1
  store.rs          produced-PNG map, disk cache in $XDG_CACHE_HOME/swayconnect/icons/              M1
devices.rs          + scope grant/deny, scopes_changed signal per device                             M1
admin.rs            + devices.grant, devices.deny                                                    M1
proc.rs             NEW Runner trait: spawn argv with env allowlist, timeout, output cap; FakeRunner M2a
notify.rs           NEW notify-send through proc.rs, fixed texts                                     M2a
lockstate.rs        NEW /proc scan: lock processes and keyboard grabbers of the own uid              M2a
keep_awake.rs       NEW KeepAwake: child handle, expiry, continuous cap with breaks, holder          M2a
session_topic.rs    NEW per-device `session` state; 2 s lockstate poll while subscribed              M2a
audit.rs            NEW audit.jsonl writer, 0600, append                                             M2a
audio/
  mod.rs            AudioBackend trait, AudioService (id issue, snapshot, debounce, caps)            M2b
  pactl.rs          JSON parse (lenient), argv builders for writes                                   M2b
  watch.rs          `pactl subscribe` child, respawn with backoff                                    M2b
media/
  mod.rs            MediaBackend trait, MediaService                                                 M2b
  mpris.rs          zbus: names, PropertiesChanged, NameOwnerChanged                                 M2b
apps/
  mod.rs            AppsService: apps topic, recent list, cut rule                                   M2b
  exec.rs           Exec split by the Desktop Entry quoting rules, field codes removed               M2b
  launch.rs         systemd-run --user --scope argv; launcher.open (fuzzel), one instance, holder    M2b
input/
  mod.rs            InputService: chunking, guard calls, release-all, session arming                 M2b
  keymap.rs         pure: text -> XKB text keymap + key events (wtype method)                        M2b
  guard.rs          lock, launcher, grabber, synchronous focus checks                                M2b
  vkbd.rs           Wayland thread: zwp_virtual_keyboard_v1, memfd keymap, one device per run        M2b
dispatch.rs         op table grows; rate class per op; extra scope checks 8a/8b
config.rs           + sections of 11.9
throttle.rs         + rate classes of PROTOCOL 17.14
```

`swayconnect-sway` gains: `Rect`, `layout`, `focus`, `scratchpad_state`, `inhibit_idle`, output `power`/`scale`/`transform`/`make`/`model` on `Node` (all `#[serde(default)]`); `binding` in the subscription (M1); command variants `MoveToWorkspace`, `Floating`, `Layout(LayoutKind)`, `MoveScratchpad`, `ScratchpadShow`, `Kill`, `OutputsPowerOn` (M2a). The command pattern test grows to:

```
^(workspace --no-auto-back-and-forth number \d{1,3}
 |\[con_id=\d+\] (focus|fullscreen (enable|disable)|floating (enable|disable)
   |move container to workspace number \d{1,3}|layout (splith|splitv|tabbed|stacking)
   |move scratchpad|scratchpad show|kill)
 |output \* power on)$
```

A new `WaylandTarget` mirrors `SwayTarget`: `Explicit(path)` from config `[wayland] socket`, or `Discover(EnvSnapshot)` with `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY`. The session bus address for zbus is built the same way from `EnvSnapshot` (`DBUS_SESSION_BUS_ADDRESS`, else `unix:path=$XDG_RUNTIME_DIR/bus`) and passed to `zbus::connection::Builder::address`; zbus never reads the environment itself. Only `main.rs` builds the snapshot. `EnvSnapshot` also gains `XDG_DATA_DIRS` (icon and entry roots). A daemon with temp XDG dirs (agents) gets no Wayland socket and no bus, so input and media are `backend_unavailable` there by construction.

`Paths` gains `cache_dir`. `main.rs` gains the `--raster` entry (11.5) and passes the raster program path into `Daemon::start`.

### 11.5 Layout export and icons

- `layout.rs` follows `PROTOCOL.md` 17.4.3. Rects are normalised against the workspace rect. The workspace node's `fullscreen_mode` is ignored. The active tab is the first `focus` id in `nodes`. Floating containers keep sway's list order (bottom first).
- The supervisor keeps "subscribed to desktop" as a count from the sessions. Above 0 it polls `GET_TREE` + `GET_WORKSPACES` every 2 s. A publish happens only when the built `Desktop` differs.
- Icon resolution runs off the async runtime, one job at a time. Results are cached by `(app_id key)` with a 30 s negative cache. The desktop-entry index is rebuilt when an unknown key appears, at most once per 30 s.
- Absolute `Icon=` paths are accepted only under the icon roots, `/usr/share/pixmaps` and the flatpak exports, after canonicalising. Extension png or svg only. Source file at most 1 MiB.
- **Rasterising runs in a short-lived child**: `swayconnectd --raster`, spawned by `raster.rs` itself (`tokio::process::Command`, `env_clear()`, stdin and stdout piped, stderr null; program path passed in by `main.rs`; tests use `env!("CARGO_BIN_EXE_swayconnectd")` or a fake `Rasteriser`). The child first sets its own limits with `rustix::process::setrlimit` (`RLIMIT_AS` 256 MiB, `RLIMIT_CPU` 2 s), then reads the source bytes from stdin and writes the PNG to stdout. The daemon kills it after 3 s of wall time and caps its output at 65536 bytes. Before rendering, the child refuses: a PNG whose IHDR declares more than 1024 x 1024; an SVG whose usvg tree has more than 10000 nodes; an SVG that uses filters. Any failure puts the key in the negative cache. One child runs at a time; a killed child never blocks the next icon.
- An owner alias map in config fixes misses: `[icons.aliases] Emulator = "jetbrains-studio"`.
- The published `Window.icon` is set only for keys that already have a produced PNG. A new key triggers resolution and a later publish. So a publish never waits for rasterising.
- `icon.get` serves at most 8 hashes per request (about 700 KB encoded, under `max_frame_s2c`) under the `icons` rate class.

### 11.6 Controls

**Window ops (M2a).** Typed commands from 11.4, checked like v1 actions: the id must be in the snapshot of the right kind. `window.move` uses workspace numbers only (4.4 rule 2).

**Keep awake (M2a).** `keep_awake.rs` holds at most one `systemd-inhibit` child. `on`: kill any old child, spawn `[keep_awake] inhibit` `--what=idle --mode=block --who=swayconnect --why=<fixed text>` `[keep_awake] sleep` `<n>`, where `n` is `duration_s` shortened to the remaining continuous budget; record holder and expiry; then send `output * power on` if an active snapshot output has `power: false`; send a desktop notice with the device short id. `off`: kill. Child exit: clear the state and publish `session`. Continuous cap (`PROTOCOL.md` 17.8 rule 5): on-time accumulates across off periods shorter than `cooldown_s`; only an off period of at least `cooldown_s` resets it; a replace does not. Config check at start: `cooldown_s` ≥ 60 and `max_continuous_s` ≥ `max_duration_s` (exit 2 otherwise); the owner keeps `cooldown_s` above his longest swayidle timeout (default 900 > 600). Released on revoke of the holder, SIGUSR1 and shutdown. Not released on disconnect.

**Lock (M2a).** `session.lock` runs config `[session] lock = ["/home/<owner>/.config/sway/scripts/lock.sh"]` through `proc.rs`. Without config the op is not advertised. At most one lock child at a time; when `lockstate` says locked or the last lock child still runs, the op returns `{already_locked: true}` without spawning. `loginctl lock-session` is not used: swayidle has no `lock` hook here.

**Session topic (M2a).** Built per device. While at least one client subscribes to `session`, `session_topic.rs` re-runs the `lockstate` scan every 2 s and publishes on change.

**Audio (M2b).** Snapshot = `pactl -f json list sinks`, `sources`, plus `get-default-sink` and `get-default-source`. A `pactl subscribe` child triggers a fresh snapshot 100 ms after the first event (client events ignored). Nodes whose name starts with `-` are dropped from the snapshot. Ids on the wire are small integers issued by the daemon and mapped to pactl names in the daemon; a name that vanished is `not_found`. Writes, always with `--` before the positional arguments: `set-sink-volume -- <name> <N>%`, `set-sink-mute -- <name> 0|1`, `set-default-sink -- <name>`, and the source equivalents. Volume is clamped to `max_volume_pct` (default 100). Source writes other than mute-on need `audio.capture`; an unmute of a source sends a desktop notice with the device short id. Caps: 64 sinks, 64 sources. The binary path comes from config, default `/usr/bin/pactl`.

**Media (M2b).** zbus on the session bus (explicit address, 11.4). Players are `org.mpris.MediaPlayer2.*` names except `playerctld`, at most 16. Metadata text is cleaned (2.4) and cut. Titles are withheld from devices without `state.titles`. Ops: play/pause, next, previous.

**Apps and launcher (M2b).** `apps/` reuses `icons/entries.rs`. Filters: `NoDisplay`, `Hidden`, `OnlyShowIn`/`NotShowIn` against `sway`, `TryExec`, URL-only handlers, ids outside `[A-Za-z0-9._-]`. Config `[apps] deny = [...]` hides entries; `risky = [...]` marks them (terminals are always risky: `Terminal=true` entries and terminal emulators, `TerminalEmulator` in `Categories`). A risky entry needs scope `launch.risky` on the server; the phone's hold is UX only. Launch argv: `[apps] systemd_run` `--user --scope --collect --unit=app-swayconnect-<sanitised id>-<random hex> -- <Exec tokens>`, with an env allowlist (`WAYLAND_DISPLAY`, `DISPLAY`, `SWAYSOCK`, `XDG_RUNTIME_DIR`, `XDG_CURRENT_DESKTOP`, `XDG_DATA_DIRS`, `DBUS_SESSION_BUS_ADDRESS`, `DBUS_SYSTEM_BUS_ADDRESS`, `PATH`, `HOME`, `LANG`) taken from the real session at start, not from the daemon's own temp dirs. The same list applies to every child of the daemon (`pactl`, `notify-send`, `systemd-inhibit`, the lock script, the launcher). `DBUS_SYSTEM_BUS_ADDRESS` is on it so the `unix:path=/nonexistent` of the test guard (11.12) reaches a test daemon's children; on the desktop it is normally unset, and then a child gets nothing. `Terminal=true` wraps with config `[apps] terminal = ["/usr/bin/alacritty","-e"]`. The topic keeps to `PROTOCOL.md` 17.11 caps and cut rule. `launcher.open` runs config `[launcher] argv` (default `["/usr/bin/fuzzel"]`) as a tracked child, one at a time, and records the holder device and the window focused at open time.

**Close window (M2a).** `[con_id=N] kill` under `destructive`.

### 11.7 Text input and its TOCTOU protection (M2b)

1. `vkbd.rs` owns one Wayland connection on a dedicated std thread. It binds `zwp_virtual_keyboard_manager_v1` and creates one keyboard on the configured seat (`seat0`) for the life of the connection. It reconnects when sway restarts. Commands arrive over a channel; each delivery ends with `wl_display.roundtrip`, so the compositor has processed the keys before the next check.
2. `keymap.rs` (pure) maps each distinct character of a part to its own keycode in a generated XKB keymap: Latin-1 directly, other code points as keysym `0x01000000 | cp`. Named keys and modifiers use their real evdev codes, each with one level only. Text is never mapped onto Escape, BackSpace, Tab, Return or modifier codes. No key of a generated keymap resolves to `XF86Switch_VT_1`..`12`, `XF86Ungrab` or `Terminate_Server` under any modifier; `ctrl`+`alt`+`F1..F12` is refused at the protocol layer as well. The keymap is uploaded through a sealed memfd and changed only between complete press and release pairs.
3. A request is cut into parts of at most 16 characters (a key press is one part). Before each part, `guard.rs` checks in this order, all synchronously:
   1. Lock: a process of the own uid whose `comm` is in config `lock_processes` (default `swaylock`) means locked. An unreadable `/proc` means unknown. Both refuse with `locked`.
   2. Launcher (target `launcher` only): the request's device is the launcher holder, and the launcher child is still running (its exit as the reaper published it, read through a probe of the child handle, not the topic view). Else `launcher_closed`.
   3. Grab: a process in `keyboard_grabbers` (default `fuzzel`, `wofi`, `bemenu`, `tofi`, `swaylock`) means keys may go elsewhere. Refuse with `keyboard_grabbed`. Exception: target `launcher` and the only grabber is the launcher child the daemon started.
   4. Focus: one `GET_TREE` on the command IPC connection, sent after the previous part's roundtrip. Sway answers IPC in order in its main loop, so the reply shows focus at that moment. The focused leaf must be `window_id`; for target `launcher` it must be the window recorded at `launcher.open` (the window that gets the keys back if the launcher closes). Else `focus_changed`. No event-based shortcut exists.
4. Parts are paced: check, deliver, roundtrip, then the next check. Nothing is pipelined. A part carries the time of its check; the keyboard thread refuses it (`backend_unavailable`) when its caller has given up or the check is older than 250 ms, also after a slow first connect, so a part queued behind a stuck delivery is never written late.
5. After each request and on any error or disconnect: release every key and modifier.
6. Session arming: the first input of a connection sets `session.input.active` and sends a desktop notice "Remote keyboard active (device <short id>)". The session ends after 10 min without input or on disconnect.
7. Residual risk, written into 5.6: keys processed between one IPC reply and the delivery of at most one part (16 characters) can reach a window that took focus in that gap, or the previously focused window when the launcher exits in that gap. A locker that is not in `lock_processes` defeats the lock check. Shortcuts with `super` reach sway bindings.
8. Logs and the audit record lengths only, never text.

### 11.8 Security model changes

**Scopes and tiers** (`PROTOCOL.md` 17.2):

| Tier | Scopes | Default at pairing |
|---|---|---|
| 0 observe | `state.read`, `state.titles` | on (unchanged) |
| 1 steer | `control`, `audio`, `media` | on |
| 2 risky | `idle.inhibit`, `launch`, `input.keys`, `input.text` | on (owner, 2026-11-14; risk in D30) |
| 2 risky | `destructive`, `audio.capture`, `launch.risky` | off; only by name |

`swayconnectd pair` gives by default: `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys`, `input.text` (owner's answer of 2026-11-14). `destructive`, `audio.capture` and `launch.risky` are granted only by name: `swayconnectd pair --scopes ...` or `swayconnectd devices grant`. `--scopes` replaces the default, so an owner can pair with less. Devices already in `devices.json` keep their scopes. Named presets stay deferred (11.13).

**Granting (M1).** New admin operations `devices.grant` and `devices.deny` (`swayconnectd devices grant <id-prefix> <scope>...`). A device is named by a unique prefix of its base64url id, at least 4 characters. A change rewrites `devices.json`, then sends `bye` `scopes_changed` to every live session of that device. An unknown scope name in `devices.json` is a start error (exit 2), like the strict config.

**Rate limits.** Per device and class, `PROTOCOL.md` 17.14. `icon.get` has its own class, also in read-only mode.

**Audit log (M2a).** `$XDG_DATA_HOME/swayconnect/audit.jsonl`, 0600, append. One line per tier 2 action, per keep-awake change, per scope change and per input session start: time, device id, peer address, op, target ids, result. Never titles, text or secrets. `swayconnectd audit [-n N]` prints it through `escape_for_terminal`. Rotation is deferred (11.13).

**Desktop notices** (notify-send, fixed text, device short id): connect of a device that holds a tier 2 scope (each authenticated connect, not a pending pairing, and not while the daemon is read-only, when no tier 2 op is offered), input session start, keep-awake on, remote unmute of a source.

**Panic.** SIGUSR1 also ends keep-awake, aborts input and kills the launcher. The launcher and keep-awake then refuse every later start with `read_only` until the daemon restarts, so a request that passed the read-only check before the switch starts nothing after it; `app.launch` reads the flag again just before its start.

**Android.** `K` stays stored as in v1 (AES-GCM under a Keystore key). Moving it to a non-exportable Keystore HMAC key is deferred: it needs a `DeviceKeyMac` interface in `:protocol` (`Proofs.kt` takes `ByteArray` today). `FLAG_SECURE` on the pair screens and the Keys screen.

**New attacker rows for 5.2:**

| Attacker | Answer |
|---|---|
| Stolen, unlocked phone with the default scopes | Can run code as the owner: fuzzel + typing reaches a terminal even without `launch.risky` (D30, the owner's choice). Answers: lock refusal, keyboard-grabber refusal, focus check per part, desktop notice at input session start and at connect of a device with a tier 2 scope, audit, `devices deny`, revoke, panic. No step-up in v2 (D32). |
| Stolen phone that re-arms keep-awake | `idle.inhibit` is on at pairing (D30). On-time is counted across short breaks: off/on toggling still hits the 4 h cap, and only an off period of at least `cooldown_s` (15 min, longer than swayidle's 600 s timeout) resets it. A notice shows each start. |
| Hostile paired phone that unmutes the microphone | Plain `audio` can only mute sources. Unmute, source volume and default source need tier 2 `audio.capture`; each remote unmute shows a desktop notice. |
| Patched client that skips the "risky" hold | The server refuses risky entries without `launch.risky`. |
| App that claims another app's id | Gets its icon. The UI never treats an icon as identity; the Keys target always shows title and workspace. |
| Hostile `.desktop` or icon file (flatpak export, `~/.local`) | Entries already run as the owner when launched; the phone cannot add arguments. Icons render in a child with memory, CPU and time limits, after size and node-count checks. |
| Local program that names a PipeWire node like an option | Names starting with `-` are dropped; every pactl argv puts `--` before names. |

**New known limits for 5.6:** lock detection is a process heuristic; the input TOCTOU window of 11.7 item 7; Super shortcuts reach sway bindings; keep-awake outlives a phone disconnect by design; the icon resolver misses apps with no desktop entry (the emulator).

### 11.9 Configuration additions

All optional, strict as before. Every child program has a path key, so tests point all of them at the testkit's `fake-proc`.

```toml
[icons]
aliases = { Emulator = "jetbrains-studio" }   # tried last, after every other match (V2_RESEARCH 3)
theme = "Papirus"                              # optional; searched before hicolor. Default: hicolor only

[session]
lock = ["/home/OWNER/.config/sway/scripts/lock.sh"]
lock_processes = ["swaylock"]
keyboard_grabbers = ["fuzzel", "wofi", "bemenu", "tofi", "swaylock"]

[keep_awake]
inhibit = "/usr/bin/systemd-inhibit"
sleep = "/usr/bin/sleep"
default_duration_s = 3600
max_duration_s = 7200
max_continuous_s = 14400
cooldown_s = 900

[notify]
notify_send = "/usr/bin/notify-send"

[audio]
pactl = "/usr/bin/pactl"
max_volume_pct = 100

[apps]
systemd_run = "/usr/bin/systemd-run"
deny = []
risky = ["gparted", "org.cachyos.KernelManager"]
terminal = ["/usr/bin/alacritty", "-e"]

[launcher]
argv = ["/usr/bin/fuzzel"]

[input]
seat = "seat0"

[wayland]
socket = ""
```

### 11.10 Dependencies

Versions from the crates.io API on 2026-11-13. Only the manifest slice edits `Cargo.toml` and `deny.toml`.

| Crate | Version | Milestone | Features / note |
|---|---|---|---|
| resvg | 0.50.0 | M1 | `default-features = false` (no text, fonts, raster-images, svgz) |
| tiny-skia | 0.12.0 | M1 | `png-format`; brings `png` 0.18 |
| rustix | 1.1.6 | M1 (`process`), M2b (`fs`) | `setrlimit` in the raster child; `memfd_create` and seals for the keymap |
| zbus | 6.3.0 | M2b | `default-features = false, features = ["tokio", "proxy"]` (`proxy` gates `#[proxy]` and the fdo proxies) |
| wayland-client | 0.31.15 | M2b | pure Rust backend |
| wayland-protocols-misc | 0.3.12 | M2b | feature `client`; `zwp_virtual_keyboard_v1` |
| xkeysym | 0.2.1 | M2b | keysym names and values |

`deny.toml`: add `BSD-2-Clause` (arrayref, via tiny-skia). zbus and the Wayland crates were checked on crates.io metadata only; `just deny` decides in the manifest slice. Not added: `image`, freedesktop-* crates (MPL), pipewire, libpulse-binding, mpris, xkbcommon, ddc-hi, evdev.

Android: no new library. Icons are about 30 Material Symbols written as Kotlin `ImageVector` builders (`ui/theme/SwayIcons.kt`). `BitmapFactory` decodes icon PNGs.

### 11.11 Android architecture

```
app/src/main/java/com/swayconnect/android/
  ui/theme/       Theme.kt, Tokens.kt (colours, shapes, type), Motion.kt (MotionScheme), Haptics.kt, SwayIcons.kt
  ui/shell/       AppShell.kt, PillNavBar.kt, TitleRow.kt, StatusPill.kt, NowCapsule.kt
  ui/desktop/     DesktopScreen.kt, WorkspaceCarousel.kt, WorkspacePager.kt, WindowCards.kt, FocusDock.kt, WindowSheet.kt
  ui/map/         LayoutMap.kt, MapGeometry.kt (pure), TileContent.kt
  ui/audio/       AudioScreen.kt
  ui/launch/      LaunchScreen.kt, FuzzyMatch.kt (pure)
  ui/keys/        KeysScreen.kt, SpecialKeys.kt, TextSplit.kt (pure)
  ui/more/        MoreScreen.kt, KeepAwakeTile.kt, LockedFeatureSheet.kt, ComputerSheet.kt, ScratchpadSheet.kt
  ui/common/      AppIcon.kt, StatePill.kt, HoldButton.kt, DashedCard.kt
  ui/state/       UiState.kt, reducers per area, FeatureAvailability.kt (pure), DriveController.kt (pure)
  data/           IconCache.kt, DesktopCache.kt
```

`:protocol` gains `Layout.kt`, `Session.kt`, `Audio.kt`, `Media.kt`, `Apps.kt`, `Input.kt` and the new ops in `SwayConnectClient`. One `MainViewModel` stays; pure reducers per area keep it testable on the JVM.

### 11.12 Test strategy

The v1 rule stands: no test and no agent changes the owner's desktop. New backends sit behind traits with fakes in `swayconnect-testkit` or in test modules.

| Area | Fake | What tests cover |
|---|---|---|
| Layout | fixtures from the live tree (titles removed) | basis points, collapse, depth cap, tabbed active child, floating order, fullscreen, cut rule |
| Icons | temp dirs with `.desktop` files and small PNG/SVG | id matching order, Inherits, pixmaps fallback, path canonicalisation, size caps, hash; a blur-bomb SVG, a `<use>` fan-out SVG and a huge-IHDR PNG each end within 3 s while other icons are still served |
| Processes | `FakeRunner` records argv and env; `fake-proc` binary for child-process tests | every argv builder: pactl (with `--`, and a node named `--server=x`), systemd-inhibit, systemd-run, launcher, lock, notify-send |
| Keep awake | `FakeRunner` + paused tokio time | expiry, replace, cap, off/on every 60 s still hits `limit_reached` at 4 h, cooldown, release on revoke and SIGUSR1, power-on command when an output is off |
| Audio | `FakeAudio` with pactl JSON fixtures from this machine | parse, id mapping, clamps, `not_found` after removal, capture rule, caps |
| Media | `FakeMedia` | status, title withholding, caps |
| Apps | temp desktop entries | filters, Exec splitting, field-code removal, quoting edge cases, risky refusal, cut rule |
| Input | `FakeKeyboard` (records key events) + `FakeProcScan` + fake sway | keymap generation, chunking, lock and grab refusal; focus changed by the fake sway **without** an event: the second part is refused; launcher holder and exit; release-all; no newline accepted |
| Security | integration tests with the testkit | scope per new op, grant/deny with `scopes_changed`, rate classes, audit lines without text |
| Android | JVM tests | reducers, `MapGeometry`, `FuzzyMatch`, `TextSplit`, `FeatureAvailability`, icon cache hash check, `DriveController` (follow focus and re-sync send nothing) |

Tests never call `pactl`, `playerctl`, `systemd-run`, `systemd-inhibit`, `notify-send`, `fuzzel`, the lock script or a Wayland socket. The test-environment guard of 9.2 extends: child processes in tests start with `env_clear()` and get temp `XDG_DATA_DIRS` (one `.desktop` file and one small PNG, so icon hashes are deterministic), `DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent`, no `WAYLAND_DISPLAY` and no `DBUS_SESSION_BUS_ADDRESS`. Every program path in a test config points at `fake-proc` in a temp dir; a guard test fails when a test config names a program under `/usr`.

v1 transcripts stay byte-identical as client-side vectors. The daemon's replay test (`tests/transcripts.rs`) compares only v1 members: it strips members that v1 does not define from server frames, and checks `welcome.ops` and `topics` as supersets.

The first real action of every new control runs by the owner's hand (`docs/V2_PLAN.md` gates).

### 11.13 Roadmap after v2

Later: owner buttons (`commands`), per-app stream volume, card profiles (switching to the monitor's HDMI speakers), media seek and position, Live typing mode, presets, Android Keystore HMAC key (with `DeviceKeyMac` in `:protocol`), audit rotation, `status.json` for a waybar indicator, multi-output card groups, tablet split layout, map↔list shared-element morph, touchpad (`input.pointer`), biometric step-up (not in v2, owner 2026-11-14), presentation mode, output power control, URL share, clipboard to desktop, Quick Settings tiles, media notification with a foreground service, urgent alerts, Wake-on-LAN, power menu (suspend, reboot, power off), album art, input-method "safe text" mode, `GET_BINDING_STATE` for the binding mode.
Never (unchanged and extended): raw sway commands, `exec` with client text, remote unlock, cliphist access, screen streaming, keystroke reading, cloud relay.
