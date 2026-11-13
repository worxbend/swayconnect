# swayconnect implementation plan, stage 1

Date: 2026-11-13, after the security, implementability and scope review. Inputs: `docs/DESIGN.md`, `docs/PROTOCOL.md`.

Next action: the owner reviews V2, then runs S12 himself (section 3). After that S13: the debug APK on a real phone over the LAN.

## 0. Status

Written on 2026-11-13, after the security and conformance review of V1 and V2.

| Checkpoint | State |
|---|---|
| V0 | Done. |
| V1 see | Written. `just ci` is green. S11, the read-only live check, is done: daemon in `read_only` mode against the owner's sway, CLI paired, 3 workspaces and 10 windows listed correctly. |
| V2 act | Written (S6d, S7b, S8b). Tested against the fake sway only; `just ci` is green. No action has reached a real sway. S12 is not run: it is the owner's step. |
| V3 phone | S4 and S9 are written and built on this machine since the owner installed the Android SDK (2026-11-13). The local gate is green: `:protocol:test` 39 tests, `:app:testDebugUnitTest` 38 tests, lint 0 errors, debug APK. The app ran on one emulator (API 37): pairing, live state from the real sway through a `read_only` daemon, the four actions against the fake sway, revoke, offline and recovery. What that run covered and what it did not: `docs/DESIGN.md` 9.5. `android.yml` has not run. S13 is not run: a physical phone over the LAN is the owner's step. |

Two gates of section 2 were not kept, at the owner's request: V2 was written before the owner's review of V1, and S9 was written before `:protocol:test` was green in CI. The owner-run steps S12 and S13 keep their gates.

The first local build found one compile error (in a test) and a racing test harness. The first run on the emulator found one defect: a live session could not be stopped from inside the app. The review after that found 31 points; `docs/DESIGN.md` 5.6 items 11 to 15 and the roadmap hold what was deferred.

The review after V2 found ten points. Nine are fixed with tests, one of them in part. One is deferred. What stays open is in `docs/DESIGN.md` 5.6 items 9 and 10.

## 1. Rules for every slice

### 1.1 Hard rules

1. Never send a state-changing command to the owner's live sway. Read-only `swaymsg -t get_*` is allowed. Nothing else. The owner-run steps S12 and S13 are run by the owner, not by an agent.
2. Never start a real, nested or headless sway.
3. Android builds run under the lock of 1.2 with `--max-workers=4`, and at most one headless emulator runs (`AGENTS.md` section 2). Until 2026-11-13 this rule forbade every Gradle run here; the slices S4 and S9 below still say "nothing is run here" for that reason.
4. No `git commit`, no `git push`, no system package install, no `sudo`. The owner commits and pushes.
5. A slice edits only the paths it owns (section 3). A needed change elsewhere goes to the owner of that path.
6. Only S0 edits `Cargo.toml` files, `Cargo.lock`, `.cargo/config.toml`, the `justfile` and `protocol/**`. A slice that needs a new dependency or a contract change asks the S0 owner.
7. The seam files `crates/swayconnect-daemon/src/security.rs` and `sway_handle.rs` are written by S0 and then owned by S6a and S6b. A change to a frozen signature (section 5) needs the S6c owner to agree. `lib.rs` holds only `pub mod` lines and re-exports; S0 writes it and nobody needs to edit it again.
8. A wire change updates `docs/PROTOCOL.md`, `protocol/vectors/`, the Rust protocol crate and the Kotlin `:protocol` module in one change.
9. No code outside the two `main.rs` files reads the process environment (`docs/DESIGN.md` 9.2).

### 1.2 Machine safety

The PC reset during an earlier run of this workflow. What the logs show (read on 2026-11-13, first and fourth row read again during the spec review):

| Fact | Evidence |
|---|---|
| It was a hard reset, not a reboot command | The journal of the previous boot ends on 2026-11-13 at 00:03:52 with an ordinary firewall line. No shutdown sequence. The next boot starts at 00:04:39. journald reports the journal as "corrupted or uncleanly shut down" and fsck reports "Dirty bit is set". |
| Not out of memory | No OOM kill and no memory pressure message. The machine has 62 GiB RAM and 62 GiB zram. |
| No kernel panic text, no thermal message | Nothing in the kernel log. A hard reset often leaves none. |
| A CPU machine check happened earlier in the same boot | Kernel log, 2026-11-12 04:53:19: `mce: [Hardware Error]: Machine check events logged`, `[Hardware Error]: System Fatal error.`, `CPU:1`, status bits `UE` and `PCC` (uncorrected error, processor context corrupt). |
| The machine was under other load | Docker containers with 2 to 4 GiB and tens of CPU minutes each ran from 22:00 to 23:59. Test binaries of another project crashed between 23:47 and 23:54. |
| swayconnect did nothing heavy | The repository had no code. No build ran. The workflow was in its read-only research and design steps. |

Reading: the cause is not proven. The evidence points to a hardware-level reset under sustained load (CPU machine check, power or cooling), not to this workflow and not to memory exhaustion. The workflow stopped because the reset killed it; its scratch files in `/tmp` were lost with the boot.

Not checked: `/sys/fs/pstore` (needs root), temperatures, firmware settings.

Precautions until the owner has cleared the hardware. Each one has a mechanism, not only a sentence:

| # | Precaution | Mechanism |
|---|---|---|
| 1 | At most 4 build jobs and 4 test threads (the machine has 16 threads) | `.cargo/config.toml`: `[build] jobs = 4` and `[env] RUST_TEST_THREADS = "4"`. It applies to every cargo call, also a raw one. |
| 2 | One cargo process at a time. Agents write code in parallel; builds and test runs queue. | Every `justfile` recipe runs cargo under `flock target/.agent.lock` (after `mkdir -p target`). A second agent waits. Agents call cargo through `just` only. |
| 3 | One shared `target/` directory. No per-agent target directories. | no `CARGO_TARGET_DIR` anywhere |
| 4 | No compositor and no container builds from this project. Gradle: one run at a time under the cargo lock, `--max-workers=4`, daemon stopped afterwards. One emulator: 2 cores, 3 GiB, headless, killed afterwards. | hard rules 2 and 3; `flock target/.agent.lock` |
| 5 | No test may call `systemctl`, `loginctl`, `swaylock` or a mutating `swaymsg` | review; the env guard of `docs/DESIGN.md` 9.2 |

### 1.3 Definition of done for every Rust slice

- `just fmt-check`
- `just lint <crate>` (`cargo clippy -p <crate> --all-targets -- -D warnings`)
- `just test <crate>` (`cargo test -p <crate> --locked`)
- Files under about 500 lines. `#![forbid(unsafe_code)]` in each crate root.

The full gate is `just ci` (fmt check, clippy for the workspace, test for the workspace, `cargo deny check`). One integrator runs it at each checkpoint.

## 2. Order

Three vertical checkpoints. Each ends with a check on the real desktop, so a model bug against the real 85 KB sway tree shows up after V1, not after everything is built.

```
S0  contract and skeleton (alone)

V1 "see"   S1 protocol ─┬─ S5 client lib ── S7 CLI (pair, read, watch) ──┐
           S2 sway ─────┼─ S6a daemon: security half ─┐                  │
           S3 testkit   └─ S6b daemon: sway half ─────┴─ S6c wiring ─────┼─ S8 integration ── S11 read-only live check
           S4 Android :protocol (written; green locally since 2026-11-13) │
           S10 docs, packaging, CI ──────────────────────────────────────┘

V2 "act"   S6d daemon actions ─┬─ S8b integration ── S12 owner runs two actions on his desktop
           S7b CLI actions ────┘

V3 "phone" owner pushes ── android.yml :protocol:test green ── S9 Android :app ── owner pushes
           ── android.yml app job green ── S13 owner pairs the phone over the LAN
```

| Checkpoint | Slices | Gate |
|---|---|---|
| V0 | S0 | workspace compiles with stubs; vector files valid; a raw `cargo build` uses 4 jobs |
| V1 see | S1, S2, S3, then S5, S6a, S6b, then S6c, S7, then S8. S4 and S10 alongside. | `just ci` green. Steps 1 to 4 and 7 of the end-to-end test (section 4) pass. Then S11 with the owner. At this point the daemon has no code path to `RUN_COMMAND`. |
| V2 act | S6d, S7b, then S8b | `just ci` green. The whole end-to-end test passes. Owner review. Then S12, run by the owner. |
| V3 phone | owner push, S4 fixes until `:protocol:test` is green, S9, owner push, S13 | `android.yml` green. S13 done by the owner. Stage 1 closes. |

V3 can start as soon as the owner has pushed after V1. It does not wait for V2 to be written, but S13 needs V2.

## 3. Slices

### S0 — Contract and skeleton

Depends on: none. One agent, alone.

Creates:

- `Cargo.toml` (workspace, `[workspace.package]`, `[workspace.dependencies]` as in `docs/DESIGN.md` 7.3), `Cargo.lock`, `.cargo/config.toml` (`[build] jobs = 4`, `[env] RUST_TEST_THREADS = "4"`), `rust-toolchain.toml`, `deny.toml`, `justfile` (every recipe under `flock target/.agent.lock`), `.gitignore`, `.editorconfig`, `LICENSE`
- `crates/*/Cargo.toml` for all five crates, each with `src/lib.rs` (and `src/main.rs` for daemon and cli)
- `crates/swayconnect-protocol/src/{message,desktop,code}.rs`: the public type names and field lists of every message, of `Desktop`, and of the error, reject and bye codes, as compiling stubs. Plus `DeviceId` and `Scope`. S1 fills behaviour only.
- `crates/swayconnect-sway/src/socket.rs` with `SwayTarget` and `EnvSnapshot` (`docs/DESIGN.md` 4.3), bodies `todo!()`
- `crates/swayconnect-daemon/src/lib.rs` with one `pub mod` line per module of `docs/DESIGN.md` 4.2, and an empty stub file for each module
- `crates/swayconnect-daemon/src/security.rs` and `sway_handle.rs` with the frozen signatures of section 5 and `todo!()` bodies. Ownership then passes to S6a and S6b.
- `protocol/vectors/v1/**` with the content of `docs/PROTOCOL.md` section 15, in the formats of section 6 below, and `protocol/vectors/v1/README.md` with that format table

Acceptance:

- `just ci` passes on the stub workspace.
- A raw `cargo build` uses 4 jobs (`cargo build -v` shows `-j4`, or `cargo config get build.jobs` prints 4).
- Every file under `protocol/vectors/v1/` parses as JSON.
- Every value in the vector files equals the value printed in `docs/PROTOCOL.md` section 15: fingerprint, both keys, all proofs, `confirm_code`, every transcript frame, every table line. Checked with a throwaway script, not by eye.
- The test key of `fingerprint.json` matches the certificate (public key comparison), and the second key does not.
- No other slice needs to touch a manifest to start.

### S1 — Protocol crate

Depends on: S0. Owns: `crates/swayconnect-protocol/src/**`, `crates/swayconnect-protocol/tests/**`.

Builds: message behaviour and validation (`message.rs`), `Desktop` model (`desktop.rs`), frame codec without I/O (`frame.rs`), proofs and `confirm_code` (`proof.rs`), fingerprint and base64url (`fingerprint.rs`), pairing URI (`uri.rs`), text rules of spec 2.4 (`text.rs`), limits (`limits.rs`), error, reject and bye codes (`code.rs`).

Acceptance:

- `just test swayconnect-protocol` replays every vector file: fingerprint, all proof values and `confirm_code`, every valid and invalid URI, every frame decoder case, every handshake case (the decode-and-validate step gives `bad_message` where the file says so), every transcript frame (decode, then encode to an equal JSON value).
- The tests read the files with `serde_json` and `env!("CARGO_MANIFEST_DIR")`. `swayconnect-testkit` is not a dev-dependency of this crate.
- `"client":null` and a non-canonical `device_id` are decode errors. serde's `Option<T>` accepts `null` by default; the message types must not.
- `text.rs`: each range of spec 2.4 has a test for `is_control_or_format`, `replace_unsafe` and `escape_for_terminal`.
- The crate has no dependency on tokio or on any sway crate.
- Proof verification uses `ring::hmac::verify`. No hand-written comparison of secrets.

### S2 — Sway crate

Depends on: S0. Owns: `crates/swayconnect-sway/**` except `Cargo.toml`.

Builds: 14-byte framing (`frame.rs`), connection with reader task and 2 s request timeout (`connection.rs`), event subscription (`events.rs`), socket path resolution (`socket.rs`), lenient reply types (`types.rs`), the command enum and its renderer (`command.rs`).

Acceptance:

- Table test: each `Command` variant renders to the exact text in `docs/DESIGN.md` 4.4. No variant holds a string.
- A test asserts every rendered command matches the pattern in `docs/DESIGN.md` 4.4.
- `command.rs` is the only file in the workspace that builds sway command text. No public function in the crate takes a command as `&str` or `String`.
- The frame writer takes an enum of message types 0, 1, 2, 4, 7. A test proves no other type can be written.
- Tests against an in-test Unix socket in a temp directory: two pipelined requests answered in order; an event frame between request and reply is routed to the event side; no reply gives a timeout error within 2 s (paused tokio clock); bad magic gives an error.
- Socket resolution tests pass an `EnvSnapshot` literal that points into a temp directory: `Explicit` wins and never looks elsewhere; a stale `swaysock` falls through to the glob; a file with a dead pid is skipped; a file with another owner is skipped (unit test on the filter function).
- No test and no source file of the crate reads the process environment.

### S3 — Testkit

Depends on: S0. Owns: `crates/swayconnect-testkit/**` except `Cargo.toml`.

Builds: `FakeSway` (own framing code, scripted replies for types 0, 1, 2, 4, 7, event injection, "stop answering", "close", record of every type-0 payload), `TestDirs` (one temp root with config, data and runtime directories and the fake sway socket path; a helper that gives a cleared environment for child processes), default fixtures. No vector loader.

Acceptance:

- `FakeSway::start(dir, forbidden)` panics when its socket path is outside `dir` or equals a path in `forbidden`. A test proves both.
- A raw-socket test reads a `GET_VERSION` reply and checks the header bytes: `69 33 2d 69 70 63`, length, type 7, native byte order.
- Unknown message type gets no reply (sway behaviour).
- Default fixtures are a small tree and workspace list with two workspaces and two windows, titles invented.
- The crate depends on neither `swayconnect-daemon` nor `swayconnect-protocol`, and reads no environment variable.

### S4 — Android `:protocol` module and Gradle scaffold

Depends on: S0. Owns: `clients/android/settings.gradle.kts`, `build.gradle.kts`, `gradle.properties`, `gradlew`, `gradlew.bat`, `.editorconfig`, `gradle/**`, `clients/android/protocol/**`.

Builds: the scaffold copied from the sibling project as listed in `docs/DESIGN.md` 8.1, and the pure Kotlin module of `docs/DESIGN.md` 8.2 with JUnit tests that replay `protocol/vectors/v1`. `settings.gradle.kts` includes `:app` only when `app/build.gradle.kts` exists.

Acceptance (nothing is run here):

- The version catalog holds only entries listed in `docs/DESIGN.md` 8.1, with those versions. The material3 entry has no version.
- `org.jetbrains.kotlin.android` appears nowhere.
- `:protocol` has no `android.*` or `androidx.*` import. `PairingUri.kt` does not use `android.net.Uri`.
- Every `@Serializable` class of the project, including the profile, is in `:protocol`.
- A test class exists for each vector file.
- Real check, in V3: `.github/workflows/android.yml` job `:protocol:test` green after the owner's push. Until then the module is labelled unverified. This slice fixes what CI finds.
- Done locally on 2026-11-13: `:protocol:test` is green on this machine. The CI job has not run.

### S5 — Rust client library

Depends on: S1. Owns: `crates/swayconnect-cli/src/client/**`, `crates/swayconnect-cli/tests/client_*.rs`.

Builds: rustls client config with a custom `ServerCertVerifier` that pins the fingerprint, ALPN, TLS 1.3 only, `resumption = Resumption::disabled()`; check of the selected ALPN; `pair(uri, device_name)` with the pending step; `connect(profile)`; `request(op, args)` with id matching; a raw-frame send for negative tests; event stream; ping timer; profile type. The nonce source is injectable.

The verifier: `verify_server_cert` hashes `end_entity` and compares. `verify_tls13_signature` calls `rustls::crypto::verify_tls13_signature(message, cert, dss, &provider.signature_verification_algorithms)`. `supported_verify_schemes` comes from the same provider. `verify_tls12_signature` returns an error. `HandshakeSignatureValid::assertion()` is never returned without that call.

Acceptance, with an in-test rustls server that presents the vector certificate and key from `fingerprint.json`:

- Wrong fingerprint (the client pins another value): the client fails with a pin error and the test server receives zero application bytes.
- Right certificate, wrong key: a test server built from the pinned certificate DER plus the second key of `fingerprint.json` (through `with_cert_resolver` and `CertifiedKey::new`, because `with_single_cert` refuses a mismatched pair). The client must fail in the TLS handshake and send zero frames (spec 15.13).
- The server selects no ALPN: the client aborts.
- With the T1 nonces injected, the client emits the T1 `pair` payload (equal JSON value), shows `confirm_code` 261988, and stores the profile only after `pairing.confirmed`.
- Transcript T7: after `bye` with `revoked` the client has stored nothing.
- A wrong `server_proof`, a wrong `welcome.device_id` after `auth`, or a wrong `welcome.protocol` makes the client close and store nothing.
- A response arriving after an event, and two responses in swapped order, are matched by id.
- A state with a lower or equal `rev` is dropped, from an event and from a repeated subscribe result.

### S6a — Daemon, security half

Depends on: S1. Owns: `crates/swayconnect-daemon/src/{config,paths,security,identity,devices,pairing,throttle,admin}.rs`.

Acceptance:

- Config: the example in `docs/DESIGN.md` section 6 loads. Unknown key, wildcard listen address, non-loopback listen without `allow_from`, `server_name` with a control character: each is an error. The default listen address is `127.0.0.1:28414` (unit test on the parsed value; nothing binds it).
- Paths: data directory or file with a looser mode, a symlink or another owner is refused. Writes are temp file, fsync, rename. Own uid from the socket-pair method equals the owner of a file the test just made.
- Identity: first run creates the certificate and key; second run loads the same fingerprint. A test parses the created DER and asserts: CN `swayconnectd`, no subjectAltName, the host name appears nowhere, the `server_name` appears nowhere, notBefore is 2000-01-01. The `ServerConfig` has no session storage, zero tickets, zero early data.
- Pairing: a correct proof makes a pending device and closes the window; a yes stores the device and fires `confirmed`; a no, the 60 s deadline (paused clock) or a dropped session deletes it and nothing is written to `devices.json`; second use of the secret fails; expiry fails; a bad proof leaves the window open and the next correct proof succeeds; with no window open one HMAC is still computed; the stored key equals `K` and the code equals `confirm_code` of the spec vectors when fed the vector inputs.
- Authentication: unknown device, pending device and wrong proof return the same result. The unknown-device path still computes one HMAC. Lookup is by exact string.
- Client strings: `verify_pair` never sees an invalid one (S1 refuses them); `devices.json` stores `client` from `pair` only.
- Revoke: the device's `watch` turns `true` under the store lock; `is_active` is false from then on; a receiver created before or after the revoke both see it.
- Throttle: connection bucket per address (burst 5, 1 per 2 s, paused clock); 20 failed handshakes spaced at the refill rate do not block a correct one from the same address (no lockout); loopback is exempt; IPv6 addresses are grouped by /64; the table never holds more than 1024 entries; with 8 connections before `welcome`, a ninth evicts the oldest connection of the address that holds the most of them (a flood from one address never evicts the connection of another); action bucket per device (burst 5, 2 per second) returns the retry delay.
- Admin socket: `XDG_RUNTIME_DIR`-equivalent path unset or relative is an error; a pre-existing directory with a looser mode, another owner (unit test on the check function) or a symlink is refused; directory 0700, socket 0600; a peer with another uid is refused (unit test on the check function); the client refuses a server with another uid; `pair.open` with a yes and with a no, `devices.list`, `devices.revoke` round-trip; closing the connection cancels the window.
- No secret appears in any `Debug` output or log line of these modules (types holding `S` or `K` have a redacting `Debug`). Device strings go through `escape_for_terminal`.

### S6b — Daemon, sway half (state)

Depends on: S1, S2. Owns: `crates/swayconnect-daemon/src/{sway_handle,supervisor,model}.rs`, `crates/swayconnect-daemon/tests/fixtures/**`.

Acceptance:

- `model.rs`: fixture tree and workspaces give the expected `Desktop` JSON. Covers scratchpad skipped, Xwayland class fallback, floating, fullscreen, workspace with `num` -1 gives `number: null`, title cut, list cut with `truncated`.
- Hostile text: a fixture with ESC, U+009B and U+202E in a title, an `app_id` and a workspace name. Each becomes U+FFFD before the length cut.
- Size cut: 512 windows with worst-case titles (200 four-byte scalars) give an encoded state at or under 262144 bytes with `truncated: true`; the kept windows are a prefix of the list.
- Supervisor: publishes only when the snapshot differs; `rev` grows by one per publish; on EOF it publishes `online=false` and reconnects with backoff (paused clock). With `SwayTarget::Explicit` it opens that path only.
- `SwayHandle::act` returns `SwayUnavailable` in this slice's stub form until S6d; no `RUN_COMMAND` is sent in V1.
- Fixtures taken from the live desktop use read-only queries and have every title and pid replaced.

### S6c — Daemon wiring (state path)

Depends on: S6a, S6b. Owns: `crates/swayconnect-daemon/src/{main,daemon,listener,session,dispatch}.rs`.

Acceptance:

- Started through `TestDirs` with `sway.socket` set to a FakeSway path and `listen = ["127.0.0.1:0"]`, as a child process with a cleared environment: the daemon logs the bound address and the fingerprint, and stops cleanly on SIGTERM with exit 0. Nothing binds port 28414 in a test.
- `dispatch.rs` in V1 knows `ping` and `state.subscribe`. The action names are not in `welcome.ops` and get `unknown_op`. It checks in the order of `docs/PROTOCOL.md` 10.2.
- Session unit tests over an in-memory stream: `hello` first; bytes sent before `welcome` beyond the first frame are refused; the 2 s deadline closes with no frame; the frame limit switches from 4096 to 16384 at `welcome`; a `pair` or `auth` frame after `welcome`, and a frame with no `type`, get `bye` with `protocol_error`; an unknown `type` is ignored.
- Pending session: empty scopes, `state.subscribe` gets `forbidden`, `ping` works; yes gives `pairing.confirmed`; no gives `bye` with `revoked`.
- A device without `state.titles` gets every `title` as null.
- Subcommands `pair`, `devices`, `revoke` work against a running daemon and fail clearly without one. `pair` prints name, platform, peer address and code, and asks y/N.
- Guard tests: no file under `crates/*/src/` except the two `main.rs` contains `std::env::var`, `env::var_os` or `env::vars`. No file under `crates/*/src/` except `crates/swayconnect-sway/src/command.rs` matches the regex `\[con_id=|no-auto-back-and-forth`. (Integration tests under `tests/` may spell command text; unit tests under `src/` compare `Command` values.)

### S7 — CLI (read path)

Depends on: S5. Owns: `crates/swayconnect-cli/src/**` except `client/**`, `crates/swayconnect-cli/tests/**` except `client_*.rs`.

Acceptance:

- Subcommands `pair`, `info`, `state`, `workspaces`, `windows`, `watch` of `docs/DESIGN.md` 7.2 exist with the listed flags and exit codes.
- `pair` reads the URI from stdin. `pair <uri>` as an argument exits 2 and connects to nothing.
- Profile files are written with mode 0600 and refused when looser.
- `--json` prints one JSON value per line.
- Table output of a state that holds ESC, U+009B and U+202E (fed through a scripted server that breaks the server guarantee) contains none of them.
- Unit tests cover argument parsing and output formatting. End-to-end tests come in S8.
- The crate's dependencies do not include `swayconnect-sway` or `swayconnect-daemon`. It does not depend on `tracing`.

### S8 — Integration tests (state path)

Depends on: S3, S6c, S7. One agent. Owns: `crates/swayconnect-daemon/tests/*.rs`, `crates/swayconnect-daemon/tests/support/**`, `crates/swayconnect-cli/tests/e2e_*.rs`. May file fixes with the owner of any failing crate.

Acceptance: every case of `docs/DESIGN.md` 9.3 rows "Daemon integration" and "CLI end to end" that does not need an action is a test and passes. Among them:

- Pair with confirmation; pair refused; pair deadline; a pending device can only `ping`; a bad proof leaves the window open.
- Revoke during a stalled client write closes the session (the 10 s write timeout or the next `is_active` check, paused clock).
- Revoke between `auth` and the first request: the first request gets `bye` with `revoked`.
- A flood of connections that never send a frame does not stop a correct `auth` from another address; the oldest idle connection is evicted.
- Steps 1 to 4 and 7 of section 4 pass. `just ci` is green.

### S10 — Docs, packaging, CI

Depends on: S0. Owns: `README.md`, `AGENTS.md`, `CLAUDE.md`, `config.example.toml`, `packaging/**`, `.github/**`.

Acceptance:

- `config.example.toml` equals the example in `docs/DESIGN.md` section 6 and is loaded by a test in S6a.
- `rust.yml`: fmt check, clippy with `-D warnings`, `cargo test --workspace --locked`, `cargo deny check`. Actions pinned by commit SHA, `permissions: contents: read`, path filter.
- `android.yml`: JDK 21, wrapper validation, `setup-android`, `platforms;android-37.0`, path filter `clients/android/**` and `protocol/**`. Job `protocol`: `./gradlew :protocol:test`. Job `app`, only when `clients/android/app/build.gradle.kts` exists: `./gradlew :app:testDebugUnitTest :app:assembleDebug :app:lintDebug`, then upload the debug APK as an artifact.
- README has the security warning, the quick start with the firewall line (`sudo ufw allow from 192.168.1.0/24 to 192.168.1.200 port 28414 proto tcp`, owner-run), the panic switch with an example `bindsym` line, and the "Android unverified" label.
- No `docs/adr/`, no `docs/admin-socket.md`, no `protocol/CHANGELOG.md`. Decisions live in `docs/DESIGN.md` section 2, their alternatives in `docs/RESEARCH.md` section 4.

### S11 — Read-only live check (end of V1)

Depends on: S8. The owner is present. No files.

Steps: `docs/DESIGN.md` 9.4 steps 1 to 4. Acceptance: `swayconnect watch` prints a new line when the owner switches workspace by hand. `workspaces` and `windows` match what the owner sees. No action command exists in this build.

### S6d — Daemon actions (V2)

Depends on: S11. Owns: `crates/swayconnect-daemon/src/actions.rs`; takes over `dispatch.rs`, `sway_handle.rs` and the signal part of `main.rs` for the changes below.

Acceptance:

- `actions.rs`: an id that is in neither list gives `NotFound`; a workspace id sent as `window_id`, and a window id sent as `workspace_id`, give `NotFound`, and the fake sway recorded no command; a workspace with `number` null, with number 1234, or sharing its number with another workspace (`1:web`, `1:mail`) gives `UnsupportedTarget`; an id cut by truncation gives `NotFound`; sway offline gives `SwayUnavailable`.
- Reply check: `[{"success":true}]` is success; an empty array, two elements or `success:false` is `SwayError`. Sway's error text is passed through `replace_unsafe` and cut to 200 characters.
- `dispatch.rs` adds the three actions, the read-only check, the rate limit and `is_active`, in the order of `docs/PROTOCOL.md` 10.2. A unit test proves that in read-only mode no action reaches `actions.rs`. `requests.json` is replayed line by line with its preconditions.
- SIGUSR1 switches a running daemon to read-only: the next action gets `read_only`, the next `welcome` has `read_only: true` and no action in `ops`.

### S7b — CLI actions (V2)

Depends on: S7. Owns: the action subcommands in `crates/swayconnect-cli/src/**`.

Acceptance: `workspace <number>`, `workspace --id <id>`, `focus <window_id>`, `fullscreen <window_id> on|off` and `raw '<json>'` exist with the exit codes of `docs/DESIGN.md` 7.2. `rate_limited` prints the retry delay.

### S8b — Integration tests (actions, V2)

Depends on: S6d, S7b. Same owner and paths as S8.

Acceptance: the remaining cases of `docs/DESIGN.md` 9.3. The whole end-to-end test of section 4 passes. A device that sends 20 `window.focus` requests at once on 4 connections gets at most 5 successes and the rest `rate_limited`; the fake sway recorded at most 5 commands. `just ci` is green.

### S12 — Owner runs two actions (end of V2)

Depends on: S8b and the owner's review. **Run by the owner himself. No agent runs or scripts this step.**

Steps: `docs/DESIGN.md` 9.4 step 5. Acceptance: `swayconnect workspace <n>` and `swayconnect focus <id>` change the focus on the real desktop and exit 0. If the daemon reports `sway_error`, the logged reply shape goes back to the S6d owner.

### S9 — Android `:app` (V3, gated)

Depends on: S4 **and** a green `:protocol:test` in `android.yml` after the owner's push. Owns: `clients/android/app/**`.

Acceptance (nothing is run here):

- Files and screens as in `docs/DESIGN.md` 8.2 and 8.3.
- `PinnedTrustManager`: all three `checkServerTrusted` overloads run the same pin check; all three `checkClientTrusted` overloads throw; `getAcceptedIssuers()` is empty; `@SuppressLint("CustomX509TrustManager")`. No `HostnameVerifier` that returns true exists. No "trust all" code path.
- `TlsTransport`: new `SSLContext` per connection, TLS 1.3 only, selected ALPN checked.
- Manifest: `INTERNET` only, `allowBackup="false"`, no cleartext flag, intent filter with `VIEW` and `DEFAULT` only, no `BROWSABLE`.
- Pair screen: a link only fills the form; nothing connects before the user taps Pair; an existing profile needs a second confirmation and is deleted only after `pairing.confirmed`; the confirm code is shown; the device name is pre-filled with `Build.MODEL`.
- `:app` has no `@Serializable` class and no serialization plugin, and does not set `warningsAsErrors`.
- JVM unit tests (run only in CI): `AesGcmCipher`; `PinnedTrustManager` and `TlsTransport` against a plain JSSE test server with the vector certificate: wrong certificate fails; right certificate with the second vector key fails in the handshake (spec 15.13; the test server needs a custom key manager to present a mismatched pair, not verified here).
- Real check: `android.yml` job `app` green after the owner's push, debug APK uploaded.
- Done locally on 2026-11-13: the gate of `AGENTS.md` section 2 is green and the debug APK ran on an emulator (`docs/DESIGN.md` 9.5). The CI job has not run.

### S13 — Owner pairs the phone over the LAN (end of V3 and of stage 1)

Depends on: S12, and S9 green in CI or in the local gate. **Run by the owner himself.**

Steps: `docs/DESIGN.md` 9.4 step 6. Acceptance: the phone shows the live workspace list and a tap on a workspace and on a window changes the focus on the desktop. If `android.yml` is not green yet, the owner may first do the same from a second LAN host with the CLI; stage 1 still closes only after the phone has done it.

## 4. The stage 1 end-to-end test

In `crates/swayconnect-daemon/tests/`, with the fake sway:

1. Start the fake sway and the daemon in temp directories on `127.0.0.1:0`.
2. Open a pairing window through the admin socket. Pair with the Rust client. The admin side reads the pending device and its code, checks that the code equals the one the client shows, and answers yes. The client reconnects with `auth`.
3. Subscribe to `desktop`. Check the two fixture workspaces.
4. The fake sway changes its tree and emits a `workspace` event. The client receives `state.changed` with a higher `rev` and the new focus.
5. (V2) The client sends `workspace.focus {"number":3}`. The fake sway's record holds exactly `workspace --no-auto-back-and-forth number 3`.
6. (V2) A second device, paired with `--scopes state.read`, sends `workspace.focus`. It gets `forbidden` with `detail.scope` = `control`. The record has no new entry.
7. Revoke the first device through the admin socket. The client receives `bye` with `revoked`. A reconnect gets `reject` with `auth_failed`.

## 5. Seam types frozen by S0

Two files in `crates/swayconnect-daemon/src/`. S0 writes them with `todo!()` bodies. S6a and S6b implement them. S6c uses them. Private fields may be added; public names and meanings stay. `lib.rs` has no types.

```rust
// sway_handle.rs — owned by S6b after S0 (S6d adds the body of `act`)
pub struct Published {
    pub rev: u64,
    pub desktop: swayconnect_protocol::Desktop,
}
pub enum Action {
    FocusWorkspaceNumber(u16),
    FocusWorkspaceId(u64),
    FocusWindow(u64),
    Fullscreen { window_id: u64, enabled: bool },
}
pub enum ActionError { NotFound, UnsupportedTarget, SwayUnavailable, SwayError(String), Timeout }
pub struct SwayHandle { /* private */ }
impl SwayHandle {
    pub fn spawn(target: swayconnect_sway::SwayTarget, shutdown: tokio::sync::watch::Receiver<bool>) -> SwayHandle;
    pub fn state(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<Published>>;
    pub async fn act(&self, action: Action) -> Result<(), ActionError>;
}

// security.rs — owned by S6a after S0
pub struct Security { /* private: identity, device store, pairing window, throttle */ }
pub struct ConnectionPermit { /* private: total-cap slot, slot before `welcome` */ }
pub enum Refused { NotAllowed, Full, RateLimited }
pub struct Handshake { pub server_nonce: [u8; 32], pub peer: std::net::SocketAddr }
pub struct Admitted {
    pub device_id: swayconnect_protocol::DeviceId,
    pub scopes: std::collections::BTreeSet<swayconnect_protocol::Scope>, // empty while pending
    pub server_proof: [u8; 32],
    pub revoked: tokio::sync::watch::Receiver<bool>, // `true` or a closed channel: revoked, or pairing refused
    pub pending: Option<Pending>,                    // Some after `pair`
}
pub struct Pending {
    pub confirm_code: String,
    pub confirmed: tokio::sync::oneshot::Receiver<()>, // fires on the owner's yes; dropping it tells pairing.rs the client is gone
}
impl Security {
    pub fn open(config: &Config, paths: &Paths) -> Result<Security, StartError>;
    pub fn fingerprint(&self) -> [u8; 32];
    pub fn tls_config(&self) -> std::sync::Arc<rustls::ServerConfig>;
    pub fn admit(&self, peer: std::net::IpAddr) -> Result<ConnectionPermit, Refused>;
    pub fn verify_pair(&self, hs: &Handshake, msg: &swayconnect_protocol::Pair) -> Result<Admitted, swayconnect_protocol::RejectCode>;
    pub fn verify_auth(&self, hs: &Handshake, msg: &swayconnect_protocol::Auth) -> Result<Admitted, swayconnect_protocol::RejectCode>;
    pub fn is_active(&self, device: &swayconnect_protocol::DeviceId) -> bool;
    pub fn take_action_token(&self, device: &swayconnect_protocol::DeviceId) -> Result<(), std::time::Duration>; // Err: retry after
}
impl ConnectionPermit {
    pub async fn evicted(&mut self); // resolves when this connection must close to make room before `welcome`
    pub fn welcomed(&mut self);      // leaves the set of connections before `welcome`
}
```

Notes:

- There is no broadcast channel for revocation. A `watch` per device cannot lag and cannot miss a revoke that lands between `verify_auth` and the session loop. `is_active` is the second check, before every request.
- `Admitted` is built under the device store lock, so the `revoked` receiver always belongs to the record that verified.
- Messages arrive in `verify_pair` and `verify_auth` already validated by the protocol crate (spec 5.7 step 1). The session checks the version (step 2). `Security` checks the proof (step 3).
- `Config`, `Paths`, `StartError` are daemon types from `config.rs` and `paths.rs`.

## 6. Vector file formats (written by S0)

S0 writes this table into `protocol/vectors/v1/README.md`. After S0 the README is the only copy and this section shrinks to a link to it.

All files are JSON. Binary values are lowercase hex unless the name ends in `_b64` (standard base64) or `_b64url`.

| File | Shape |
|---|---|
| `fingerprint.json` | `[{"name", "cert_der_b64", "key_pkcs8_b64", "wrong_key_pkcs8_b64", "fp_hex", "fp_b64url"}]` |
| `proofs.json` | `{"inputs": {"s","fp","device_id","pairing":{"server_nonce","client_nonce"},"later":{...}}, "expect": {"pair_proof","k","confirm_mac","confirm_code","server_proof_pairing","client_proof_later","server_proof_later"}}` |
| `pairing-uri.json` | `{"valid": [{"uri","v","host","port","fp_hex","secret_hex","name"}], "invalid": [{"uri","reason"}]}`; `name` is `null` when absent |
| `frames.json` | `[{"name","limit","input_hex": [chunks],"expect": "bad" \| "too_large" \| [payload strings]}]` |
| `requests.json` | `[{"name","scopes","read_only","sway_online","state_ref","actions_left","payload","expect": "ok" \| "ignored" \| {"error": code, "detail"?: object} \| {"bye": code}}]`. `state_ref` is `"t3-rev7"` or `"offline"`. `actions_left` is the number of tokens in the device's action bucket. |
| `handshake.json` | `[{"name","after": "t1" \| "t2","window_open","payload","expect": {"reject": code}}]` |
| `transcripts/*.json` | `{"name","preconditions","frames": [{"from": "S" \| "C","payload": string}]}`; the length prefix is derived from the payload |

## 7. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| The Android app is written blind | build breaks, or a silent pin mistake | pure `:protocol` module with vector tests; `:app` starts only after `:protocol` is green in CI; catalog subset; all three trust-manager overloads specified; JVM test for the wrong-key case; "unverified" label until S13 |
| Real sway differs from the fake (`RUN_COMMAND` reply shape) | an action fails on the real desktop | strict one-result check; no names in commands; owner-run step S12 before stage 1 closes |
| A client keeps the pin check but stubs the handshake signature check | a relay with the public certificate is accepted | spec 3.2 rule 4, case 15.13, S5 and S9 tests with the second vector key |
| rustls, tokio-rustls, rcgen API fit not compiled yet | S5 and S6a lose time | S0 adds all three to the manifests and runs `cargo check` first; field and function names in the docs were read from the rustls 0.23.46 source |
| The proof handshake has a design flaw | authentication bypass | uses only HMAC-SHA256 with fixed labels and channel binding; vectors for every value; relay and replay cases are integration tests; owner confirmation limits what a leaked secret gives; review before any release |
| Hardware instability of this machine | another reset during a build | section 1.2 mechanisms; owner checks the hardware |
| Two agents edit one file | lost work | ownership per slice; `lib.rs` has nothing to edit; seam files have one owner each |
| Contract change after S0 | Rust and Kotlin drift | rule 1.1.8; vectors are shared data files |
| The phone cannot reach the daemon | S13 fails with a timeout | firewall line in README and `pair` output; `allow_from` check logged with the refused address |

## 8. Open questions for the owner

1. The PC reset: see 1.2. Please check `sudo ls /sys/fs/pstore`, CPU temperatures and firmware settings (overclock, curve optimizer, memory profile) before heavy builds. The plan stays at four build jobs until then.
2. Is a pairing link that opens the Android app acceptable for stage 1? Another app on the phone could claim the `swayconnect://` scheme. With the confirmation code such an app can no longer pair silently. Say no and S9 ships paste only.
3. The firewall rule for S13 opens port 28414 to 192.168.1.0/24. Is that the right network, or should it be the phone's address only?
4. The panic switch is SIGUSR1. Do you want the README to suggest a sway key binding for it? You would add the line to your sway config yourself.
5. Stage 1 drops `window.close`, workspaces without a number, `swayconnectd status` and several CLI profiles (all listed for stage 2). Your workspaces are 1, 2, 3 today, so nothing you use is lost. Say so if one of them should come back.
