# swayconnect research and brainstorm

Date: 2026-11-13. Condensed from four research passes and three design proposals. Updated the same day after the security, implementability and scope review.
This file explains **why**. The decisions themselves are in `docs/DESIGN.md`. The wire contract is `docs/PROTOCOL.md`.

Marks used: **verified** = checked on this machine or in a primary source during research. **UNVERIFIED** = from memory or a secondary source.

Only read-only sway queries were run (`swaymsg -t get_*`, a raw-socket probe with `GET_*` and `SUBSCRIBE ["tick"]`). `RUN_COMMAND` and `SEND_TICK` were never sent.

## 1. Sway IPC facts

Sources: `man 7 sway-ipc`, `man 5 sway`, `man 5 sway-input` (sway 1.12, installed), crate sources `swayipc-4.0.0` and `swayipc-types-2.0.3`, live read-only probes.

### 1.1 Wire format (verified live)

- UNIX stream socket. Frame: `"i3-ipc"` (6 bytes), `u32` payload length, `u32` type, payload. Integers are in **native** byte order.
- Replies come in request order. Pipelining works.
- An unknown message type gets **no reply at all**. A client without a timeout hangs.
- Bad magic: sway closes the connection.
- The socket has no authentication. Mode `srwxr-xr-x`, owner connects.

### 1.2 Message types

| # | Name | Mutates | Used by swayconnect |
|---|---|---|---|
| 0 | RUN_COMMAND | yes: reaches `exec`, `exit`, `reload`, `bindsym`, `input`, `output` | yes, with daemon-built text only |
| 1 | GET_WORKSPACES | no | yes |
| 2 | SUBSCRIBE | connection state only | yes |
| 4 | GET_TREE | no | yes |
| 7 | GET_VERSION | no | yes |
| 3, 5, 6, 8, 12, 100, 101 | GET_OUTPUTS, GET_MARKS, GET_BAR_CONFIG, GET_BINDING_MODES, GET_BINDING_STATE, GET_INPUTS, GET_SEATS | no | not in stage 1 |
| 9 | GET_CONFIG | no, but leaks the whole config | never |
| 10 | SEND_TICK | side effect on other IPC clients | never |
| 11 | SYNC | always fails on sway | never |

### 1.3 Events and subscriptions (verified live unless marked)

- Events: `workspace`, `output`, `mode`, `window`, `barconfig_update`, `binding`, `shutdown`, `tick`, `bar_state_update`, `input`. Bit 31 of the type marks an event.
- A subscribed connection still answers requests, so events can arrive between a request and its reply.
- No unsubscribe, no replay, no sequence numbers. After a reconnect, take a full snapshot.
- `output` events carry only `{change: "unspecified"}`. Workspace events do not carry a reliable subtree. So events are best used as "something changed, query again".
- UNVERIFIED: sway drops an IPC client whose write buffer grows too large. The event reader must never wait on a TCP client.

### 1.4 Payload facts that shaped the model (verified live)

- `get_tree` is about 85 KB on this desktop. `get_workspaces` is about 4 KB.
- Workspace nodes inside `get_tree` have `num` and `output` but **not** `visible`. `get_workspaces` has all three, with empty `nodes`. Both queries are needed.
- A window node has `id` (target with `[con_id=<id>]`), `app_id`, `name` (title), `pid`, `focused`, `urgent`, `fullscreen_mode`, `type` (`con` or `floating_con`). Xwayland windows add `window_properties.class`.
- Output `__i3` holds workspace `__i3_scratch` (the scratchpad).
- A workspace with no number reports `num: -1` (`man 7 sway-ipc`). The model maps it to `null`.
- Workspace ids and window ids are one `con_id` space. `[con_id=<workspace id>]` addresses the whole workspace, so the daemon must keep the two id sets apart.
- Sway adds fields per release and returns enum values the man page does not list (`subpixel_hinting: "unknown"`). A strict parser breaks.
- `man 5 sway` line 909: `workspace [--no-auto-back-and-forth] [number] <[num:]name>`.

### 1.5 RUN_COMMAND results (UNVERIFIED by design)

- Reply: array with one object per parsed command, `{success, error?, parse_error?}`.
- Commands split on `;` and `,`. A client string placed in a command can chain `exec`.
- An unknown command may stop processing, so the array can be shorter than the command list.
- `exec` reports success when the fork succeeds.

### 1.6 Socket path

- `SWAYSOCK=/run/user/<uid>/sway-ipc.<uid>.<sway pid>.sock`. A sway restart gives a new path. A long-lived daemon holds a stale variable.
- `reload` keeps the process and the socket.
- Fix: resolve on every connect, with a glob fallback and a live-pid check.

### 1.7 Rust crates for sway IPC

| Crate | Version | Finding |
|---|---|---|
| `swayipc` | 4.0.0 | sync. Parses sway 1.12 replies (verified). `subscribe` consumes the connection, so two connections are needed. |
| `swayipc-async` | 3.0.0 | built on `async-io`, not tokio. Adds a second reactor thread. |
| `swayipc-types` | 2.0.3 | strict serde enums: a new `change` value fails the whole message. `input_code` is `u8`. |

Shared problems: no timeouts, no explicit socket path (prefers `I3SOCK`, may spawn `i3`), single maintainer. Hand-written framing is about 30 lines.

### 1.8 What sway IPC cannot do

| Feature | Mechanism outside IPC |
|---|---|
| Key or text input | `zwp_virtual_keyboard_v1`, or `wtype` (not installed) |
| Pointer | `seat <seat> cursor move/press/release` exists in IPC but is deprecated; proper way is `zwlr_virtual_pointer_v1` |
| Volume, media | `wpctl`, `playerctl` (both installed) |
| Screenshot | `grim` |
| Lock, suspend | `swaylock`, `loginctl`, `systemctl` |

## 2. Prior art

| Project | What it teaches | Source |
|---|---|---|
| KDE Connect, GSConnect, Valent | Capability lists in the identity packet are a good extension mechanism. No request ids, so a client cannot tell failure from loss. Run-commands are predefined on the desktop and called by key. | https://valent.andyholmes.ca/documentation/protocol.html , https://userbase.kde.org/KDE_Connect/Tutorials/Adding_commands |
| KDE Connect CVE-2025-66270 | Identity sent twice and not cross-checked: impersonation of a paired device. An identity must be bound to the secure session. | https://kde.org/info/security/advisory-20251128-1.txt |
| KDE Connect CVE-2025-32898 | 8-character eyeball pairing code was brute-forced. Use a high-entropy secret or a PAKE. Keep no downgrade path. | https://kde.org/info/security/advisory-20250418-3.txt |
| KDE Connect 2018 run-command arguments | Client-supplied arguments allowed shell injection. | https://mail.kde.org/pipermail/kdeconnect/2018-October/004605.html |
| KDE Connect on sway | Remote input does not work on wlroots (needs the RemoteDesktop portal). A sway-native tool has no competitor. | https://bugs.kde.org/show_bug.cgi?id=448604 , https://gitlab.freedesktop.org/libinput/libei/-/issues/1 |
| Unified Remote CVE-2021-47891, CVE-2022-3229 | Authentication optional and off by default, plus a second admin port: unauthenticated remote code execution. | https://www.vulncheck.com/advisories/unified-remote-remote-code-execution , https://packetstormsecurity.com/files/168456/Unified-Remote-Authentication-Bypass-Code-Execution.html |
| lan-mouse, wayvnc, Sunshine | Use `wlr-virtual-pointer` and `virtual-keyboard` on sway. | https://docs.rs/crate/lan-mouse/0.11.0/source/README.md |
| StreamController | Valued pattern: button pages that follow the focused window, and state shown on the button. | https://flathub.org/apps/com.core447.StreamController |
| sway or i3 phone remotes | Three web searches found none. Only local IPC libraries exist. Absence is not proof. | — |

Why a phone remote fits a tiling compositor: the state is a small tree, not pixels. Every action is already a named command. Live events make the phone a second status bar.

## 3. Feature candidates by tier

Danger scale: LOW annoyance, MED disrupts or leaks, HIGH data loss, CRIT code execution.

### MVP (stage 1)

| Feature | Backed by | Danger |
|---|---|---|
| Version negotiation, capability list | daemon | LOW |
| Mandatory pairing, authenticated encrypted session | daemon | a flaw here is CRIT |
| Workspace list, window list (titles under their own scope) | GET_WORKSPACES, GET_TREE | MED (titles leak) |
| Live updates | SUBSCRIBE + re-query | MED (activity stream) |
| Focus workspace, focus window, fullscreen | RUN_COMMAND, daemon-built | LOW alone; MED because focus decides where keystrokes go, so actions are rate limited |
| Keepalive, typed errors, revoke, read-only mode | daemon | none |

### Next (stage 2)

Command buttons defined in daemon config and called by id only. Close window under its own scope, off by default (`[con_id=N] kill`, HIGH: unsaved work; cut from stage 1 in review because the stage 1 path does not need it). Workspaces without a number. Media and volume. Move window to workspace or output. Outputs list. Binding mode display. Scratchpad. Lock (never unlock). mDNS discovery as an address hint. In-app QR scanner. Several paired computers.

### Later or maybe

Touchpad (HIGH: can click into a terminal). Keyboard (CRIT: typing equals code execution). Output power (can black out every screen). Notifications to the phone (HIGH for privacy). Thumbnails. Brightness. Marks, swap, resize. Access beyond the LAN by documenting WireGuard or Tailscale.

### Not doing

| Not doing | Why |
|---|---|
| Raw sway command passthrough, raw IPC proxy | includes `exec`; equals remote code execution |
| `exec` with client text or arguments | KDE Connect injection precedent |
| `exit`, `reload`, `bindsym`, `input` from a client | session kill, persistent rebinding |
| `GET_CONFIG` exposure | config may hold secrets |
| Any switch that disables authentication | Unified Remote CVEs |
| Remote unlock | turns a stolen phone into desktop access |
| Clipboard sync, file transfer, SMS | KDE Connect scope, not compositor control |
| Screen streaming | wayvnc exists |
| Cloud relay, accounts | LAN only |
| KDE Connect protocol compatibility | complex; had a Critical cross-implementation bug |

## 4. Options considered and the choices

### 4.1 Transport

| Option | For | Against | Verdict |
|---|---|---|---|
| WebSocket over TLS | browser can speak it; OkHttp and Swift have it; ping, close codes, subprotocol built in | HTTP upgrade parser in front of `exec`; a dependency on both sides; a browser cannot pin a self-signed certificate, so the browser benefit is mostly an illusion | rejected for v1; possible later as a second binding |
| gRPC | codegen, streaming | heavy on Android, awkward pinning, browser needs a proxy | rejected |
| Line-delimited JSON over TLS | easiest to debug by hand; `readline` everywhere | length is unknown until the newline; with HMAC proofs nobody types it by hand anyway | rejected, close second |
| **Length-prefixed JSON over TLS 1.3** | size checked before reading; no escaping rule; zero libraries on Android, Python, Go, Swift; binary frames possible later | every client writes ten lines of framing | **chosen** |

The research pass recommended WebSocket. Its own counter-argument won: the main reason was a browser client, and a browser cannot pin. All three proposals dropped WebSocket.

### 4.2 Encoding and envelope

- JSON chosen. Every language has it. Largest message measured is 85 KB on a LAN, so size is no constraint. CBOR, MessagePack, protobuf, postcard rejected: not readable, or a build step per client, or Rust-only in practice.
- Own envelope (`req`, `res`, `evt`, string error codes) chosen over JSON-RPC 2.0. JSON-RPC brings integer error codes, batches and a version member that add nothing here. Request ids and explicit responses are kept: KDE Connect has neither.
- Handshake messages have their own types (`hello`, `pair`, `auth`, `welcome`, `reject`). The state machine is then visible in the message names.
- The daemon sends its own flat, stable state model. Raw `get_tree` is 85 KB, changes per sway release and holds pids.

### 4.3 Secure channel

| Option | Verdict |
|---|---|
| Plain TCP with a token | rejected: a sniffer gets the token and then the desktop |
| Pre-shared key with hand-written encryption | rejected: that is designing a secure channel |
| Noise (`snow`) | sound, but every client language needs a third-party Noise library, and the JVM ones are stale |
| TLS 1.3 PSK | UNVERIFIED support on Android (Conscrypt); rejected |
| **TLS 1.3, self-signed server certificate, pinned fingerprint** | **chosen**: platform TLS in every language |

Two facts found in review (both verified in the rustls 0.23.46 source on this machine):

- A pin is only as good as the handshake signature check. The certificate is public. In rustls a custom `ServerCertVerifier` must implement `verify_tls13_signature` itself; the common copy-paste returns `HandshakeSignatureValid::assertion()`, which accepts a relay that replays the certificate. The spec has a conformance case for it (15.13) and publishes a second test key.
- rustls `ServerConfig` defaults to a 256-entry session cache and 2 TLS 1.3 tickets. A resumed handshake carries no certificate, so the pin check cannot run. Resumption is off on both sides.

Fingerprint of the whole certificate was chosen over the SPKI hash. The SPKI hash survives a certificate renewal with the same key, but this certificate is created once and never renewed, and the SPKI needs X.509 parsing in some languages.

### 4.4 Device authentication

| Option | For | Against | Verdict |
|---|---|---|---|
| Bearer token inside TLS | simplest; well trodden | the token and the pairing secret cross the wire; a client with a broken pin check hands them to a man in the middle, who then owns the desktop session | rejected |
| TLS client certificates | strongest against credential theft | doubles the spec; hard for some client platforms | deferred |
| PAKE with a short code (`spake2`) | short code is safe | no well-kept Android library | rejected |
| **HMAC-SHA256 challenge-response bound to the certificate fingerprint and both nonces** | no secret on the wire, even at pairing; a relay with another certificate fails even when a client forgets to compare the pin; needs only HMAC and SHA-256, which every platform has | written for this project; the daemon must store device keys unhashed; does not help a client that skips the TLS handshake signature check | **chosen** |

This matters because the protocol is a contract for clients that other people will write. The design must stay safe when a client author gets the pin comparison wrong. It cannot stay safe when a client turns off the handshake signature check; the spec says so and tests for it. The construction is the classic challenge-response with channel binding (compare `tls-server-end-point`, RFC 5929). Fixed vectors for every value are in the spec.

### 4.5 Pairing

- Chosen: the owner runs a local command. The daemon shows a QR code and a `swayconnect://pair?...` URI with host, port, fingerprint and a single-use 256-bit secret. 300 s window.
- Added in review: the pairing is pending until the owner confirms a 6-digit code at the desktop. The secret can leak three ways (QR photo, another app that takes the link, a process argument list), and without confirmation the first presenter wins.
- Dropped in review: every failure counter. "Five failures close the window" and "five failed handshakes ban the address for 300 s" protect nothing with 256-bit keys, and both let an unauthenticated host (or any local process on loopback) lock the owner out. Limits are caps, deadlines and token buckets.
- The fingerprint travels out of band (screen to camera). That beats "trust on first use, accept on both ends".
- The CLI consumes the same URI, so one flow serves every client. It reads it from stdin: a process argument is readable by every local user through `/proc/<pid>/cmdline`.
- Android input: link (`swayconnect://`) plus paste. An in-app scanner was rejected for stage 1 because it needs a library that the sibling builds never compiled. Cost: another app could claim the scheme; the confirmation code turns that from a silent pairing into a pending device the owner refuses. Recorded as a known limit. How a given QR scanner handles a custom scheme is UNVERIFIED; paste always works.

### 4.6 State sync

Chosen: full snapshot, latest wins, one topic `desktop`, `rev` per connection. Rejected: deltas with sequence numbers and gap recovery (more code in every client, for 4 to 90 KB on a LAN), per-client queues with an overflow message (a `watch` channel does the same with no queue), an `epoch` field (a client starts fresh on every connection anyway).

### 4.7 Admin channel

Chosen: Unix socket with peer uid check. Rejected: daemon subcommands that edit files while the daemon polls them. The file design is smaller, but it puts the pairing secret on disk, delays revoke by the poll interval and has two writers for one file.

### 4.8 Discovery

Not in stage 1. The pairing URI carries host and port. Cost: a DHCP address change breaks reconnect until the host is edited or the QR is scanned again. mDNS is planned as opt-in in stage 2, as an address hint only; the pinned fingerprint still decides identity. Android `NsdManager` is unreliable on some devices, so manual entry stays.

## 5. Stack facts

### 5.1 Rust (crates.io, checked 2026-11-12)

rustls 0.23.46, tokio-rustls 0.26.6, rcgen 0.14.10, ring 0.17.14, tokio 1.54.1, tokio-util 0.7.20 (not used after review; `tokio::sync::watch` covers shutdown and revoke), serde 1.0.229, serde_json 1.0.154, clap 4.6.7, thiserror 2.0.21, anyhow 1.0.104, tracing 0.1.44, tracing-subscriber 0.3.23, toml 1.2.1, base64 0.23.1, qrcode 0.14.1, tempfile 3.27.0, assert_cmd 2.2.2.

- rustls defaults to the `aws-lc-rs` provider (C build). Use `default-features = false` with `ring`.
- rustls had five advisories in 2026-09 and 2026-10 (RUSTSEC-2026-0285, -0346, -0347, -0348, -0349). Which release fixes each is UNVERIFIED. -0348 concerns brotli certificate decompression: never enable `brotli`.
- Not compiled yet: the fit of rustls 0.23, tokio-rustls 0.26 and rcgen 0.14 is assumed from their feature lists.
- `rustls::ConfigBuilder::with_single_cert` refuses a key that does not match the certificate (verified in source). A test server for the wrong-key case uses `with_cert_resolver` and `CertifiedKey::new`.
- Edition 2024 makes `std::env::remove_var` unsafe. With `#![forbid(unsafe_code)]` no test can scrub its own environment, so library code takes paths and environment values as arguments.
- std has no safe call for the own uid, the hostname or a Unicode general category. Chosen without a new crate: uid from `peer_cred()` on a `UnixStream::pair()`, hostname from `/proc/sys/kernel/hostname`, pid check through `/proc/<pid>`, and a fixed list of character ranges in the spec instead of Unicode categories (a category changes with the Unicode version; two implementations would disagree).

### 5.2 House conventions (sibling repos)

- `prismcast`: the closest template. Cargo workspace with `crates/<name>`, a protocol crate with serde types only, golden wire tests, an ADR that refuses a non-loopback bind without TLS.
- `obsctl-rs`: only the daemon talks to the backend; CLI is a thin client. `thiserror` in libraries, `anyhow` at binary edges. Fake backend in tests. No sleep-based readiness.
- `swaylingo`: sway user-unit pattern, drops `I3SOCK`.
- Gate everywhere: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo deny check`.

### 5.3 Android

- Sibling `scenedeck-android` catalog (read from `gradle/libs.versions.toml` on 2026-11-13): AGP 9.4.1, Kotlin 2.4.20, Compose BOM 2026.09.00, lifecycle 2.11.0, activity-compose 1.13.0, androidx-core 1.19.1, kotlinx-coroutines 1.11.0, kotlinx-serialization 1.11.0, JUnit 4.13.2, Turbine 1.2.1, Gradle wrapper 9.8.0.
- AGP 9 has built-in Kotlin: do not apply `org.jetbrains.kotlin.android`.
- TLS 1.3 is default from API 29. https://developer.android.com/reference/javax/net/ssl/SSLSocket
- Android 17: apps that target SDK 37 need runtime permission `ACCESS_LOCAL_NETWORK` for any LAN TCP connection; a denial looks like a timeout. https://developer.android.com/privacy-and-security/local-network-permission
- OkHttp `CertificatePinner` cannot pin a self-signed certificate; a custom trust manager is needed either way.
- Secrets in the sibling: AES-GCM with an AndroidKeyStore key behind a `SecretsStore` interface.
- Sibling `scenedeck-android` pins material3 with `version.ref = "material3"` (1.5.0-alpha29), applies `kotlin.plugin.serialization` in `:app` and sets lint `warningsAsErrors = true` (verified 2026-11-13). Sibling `airgradient-android` has the BOM-managed material3 entry with no version.
- UNVERIFIED: on Android (Conscrypt) an `SSLSocket` with an `X509ExtendedTrustManager` calls `checkServerTrusted(chain, authType, Socket)`, not the two-argument form. The design runs the pin check in all three overloads.
- UNVERIFIED: `android.net.Uri.getQueryParameter` decodes `+` as a space. The spec says `+` is a plus sign, so the parser is hand-written in the pure Kotlin module.
- No Android SDK on this machine. Local JDK is 25; sibling CI uses JDK 21.

### 5.4 This machine

- sway 1.12, one output `DP-1`, two keyboard layouts, 15 input devices.
- Interfaces: `enp37s0 192.168.1.200/24` and `docker0 172.17.0.1/16`. A wildcard bind would expose the daemon to containers.
- Firewall: UFW is active and drops inbound LAN TCP (verified in the journal: `[UFW BLOCK] IN=enp37s0 ... SRC=192.168.1.131 DST=192.168.1.200 ... PROTO=TCP ... DPT=8099`). A LAN listen address needs an `ufw allow` rule for the port, or the phone only times out.
- Workspaces on this desktop are `1`, `2`, `3`: numbered, name equal to number (read-only `get_workspaces`, 2026-11-13). Addressing by number is enough for the owner.
- 16 CPU threads, 62 GiB RAM, 62 GiB zram swap.

## 6. Reference monorepos

`mullvad/mullvadvpn-app` (root Cargo workspace, daemon, CLI, `android/`, `ios/`), `signalapp/libsignal` (language-named top-level directories), `bitwarden/sdk-internal` (top-level `protocols/` and `test-vectors/`). Taken from them: Rust workspace at the root, one directory per client platform, shared vectors as data files outside any client.

## 7. Still unverified

1. `RUN_COMMAND` reply shape on a real sway. First seen in the owner-run step of the plan (S12). Workspace-name quoting is no longer needed: no name enters a command.
2. Sway behaviour across `reload` and restart; the write-buffer disconnect limit.
3. Android: custom trust manager on a raw `SSLSocket` (which `checkServerTrusted` overload is called), ALPN through `SSLParameters`, the AGP 9 build, Gradle 9.8.0 on JDK 25, whether a camera app opens a `swayconnect://` link without `BROWSABLE`, a JSSE test server that presents a certificate with a foreign key.
4. Port 28414 against the IANA registry (it is absent from `/etc/services` here).
5. Which rustls release fixes each 2026 advisory.
6. That no sway phone remote exists.
