# swayconnect

Remote control for a [sway](https://swaywm.org/) desktop. A daemon on the computer (`swayconnectd`) shows paired devices the live workspaces and windows and lets them change focus. Clients are a Rust command line tool (`swayconnect`) and an Android app.

## Status

**Stage 1 is written. It is not closed and nothing is released.**

| Checkpoint | Content | State |
|---|---|---|
| V1 "see" | pair, read state, watch live changes | Written. Tested against a fake sway (`just ci`). The read-only live check on the owner's real sway is done. |
| V2 "act" | focus a workspace, focus a window, fullscreen on or off | Written. Tested against a fake sway only. No action has reached a real sway: the owner has not run step S12 yet. |
| V3 "phone" | Android app | Builds and passes its JVM tests locally. Ran on one emulator. The owner has not run step S13 yet. |

Read it this way:

- The read commands (`info`, `state`, `workspaces`, `windows`, `watch`) were seen working on a real sway.
- The action commands (`workspace`, `focus`, `fullscreen`) pass every automated test, but those tests use a fake sway. How a real sway answers them is not verified until S12.
- Stage 1 closes when the owner has run S12 and S13 himself. Both steps, and the full status, are in [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).

**Android: not yet run on a real phone.** The client under `clients/android` builds on the owner's machine since 2026-11-13: `:protocol:test` and `:app:testDebugUnitTest` pass, lint has no errors, and the debug APK installs.

On one emulator (API 37, daemon on the host's loopback) these worked: pairing by link and by paste with the same code on both sides, the live desktop of the real sway through a `read_only` daemon, the four actions against a fake sway, revoke, daemon stop and recovery.

Not verified: a physical phone, a LAN address with the firewall rule, a QR scanner, Android below API 37, a release build. The GitHub workflow `android.yml` has never run. Treat the app, and its certificate pin check on a real network, as unverified until a phone has paired over the LAN (S13). Details: `docs/DESIGN.md` 9.5 and 5.6.

## Security warning

Read this before you run the daemon.

- **Sway IPC is a shell.** Whoever controls sway can run programs as you. swayconnect never forwards a command string: it accepts three typed actions whose targets are numeric ids. This design is new and has had no outside security review.
- **A paired device can steer your keyboard focus.** With the default scopes it can also read your window titles (URLs, document names, chat partners). Pair only devices you own. Use `swayconnectd pair --scopes state.read,control` to withhold titles.
- **The pairing handshake is written for this project.** It uses TLS 1.3 with a pinned certificate and HMAC-SHA256 proofs. It is not a reviewed standard protocol.
- **Do not expose the port to the internet.** The daemon listens on `127.0.0.1` by default. It refuses wildcard addresses. A LAN address needs an explicit `allow_from` list.
- **Authentication and encryption cannot be turned off.** There is no flag for it and none is planned.
- **Not protected:** availability against hosts on your LAN, against other users of this computer and against a paired device that floods the daemon, and anything that already runs as your user.

The full threat model and the list of known limits are in [`docs/DESIGN.md`](docs/DESIGN.md) section 5.

## Quick start

Needs Linux with sway, and a Rust toolchain through `rustup` (the version is pinned in `rust-toolchain.toml`).

1. Install both binaries into `~/.cargo/bin`:

   ```
   cargo install --locked --path crates/swayconnect-daemon
   cargo install --locked --path crates/swayconnect-cli
   ```

2. Start the daemon. With no config file it listens on `127.0.0.1:28414` only.

   ```
   swayconnectd run
   ```

   Or use the systemd user unit:

   ```
   install -Dm644 packaging/systemd/swayconnectd.service ~/.config/systemd/user/swayconnectd.service
   systemctl --user enable --now swayconnectd
   ```

3. Pair a client. In one terminal:

   ```
   swayconnectd pair
   ```

   It prints a QR code and a pairing URI. In a second terminal, run the client and paste the URI on its standard input (never as an argument: other local users can read process arguments):

   ```
   swayconnect pair
   ```

   Both sides show a 6-digit code. Compare them. Answer `y` in the `swayconnectd pair` terminal only if they match. Answer after the device is shown: a `y` typed ahead is ignored.

4. Read the desktop:

   ```
   swayconnect workspaces
   swayconnect windows
   swayconnect watch
   ```

5. Act (checkpoint V2; not yet run against a real sway, see Status):

   ```
   swayconnect workspace 2
   swayconnect focus <window_id>
   swayconnect fullscreen <window_id> on
   ```

### Reach the daemon from the LAN (phone)

1. Copy `config.example.toml` to `~/.config/swayconnect/config.toml`. The example already lists a LAN address, so edit it first: put your computer's own address in `listen` and your network in `allow_from`.

   ```toml
   listen = ["127.0.0.1:28414", "192.168.1.200:28414"]
   allow_from = ["192.168.1.0/24"]
   ```

   An unknown key is a start error. A typo cannot silently weaken a setting.

2. Open the port in the host firewall. **You run this yourself, once.** Without it the phone only times out. With UFW, for the addresses above:

   ```
   sudo ufw allow from 192.168.1.0/24 to 192.168.1.200 port 28414 proto tcp
   ```

3. Restart the daemon, run `swayconnectd pair`, and scan the QR code with the phone.

## Panic switch

`SIGUSR1` switches the running daemon to read-only mode until it is restarted. Paired devices can still read state. Every action is refused with `read_only`.

```
pkill -USR1 -x swayconnectd
systemctl --user kill -s USR1 swayconnectd
```

Bind it to a key in your own sway config, so it works wherever the focus is. Choose a free key:

```
bindsym $mod+Shift+F12 exec pkill -USR1 -x swayconnectd
```

To cut one device off for good: `swayconnectd devices`, then `swayconnectd revoke <device_id>`. To start read-only from the beginning, set `read_only = true` in the config.

## Configuration

File: `~/.config/swayconnect/config.toml`. Every key is optional. [`config.example.toml`](config.example.toml) shows all of them. Details: [`docs/DESIGN.md`](docs/DESIGN.md) section 6.

| Key | Default | Meaning |
|---|---|---|
| `server_name` | system hostname | label shown to paired clients |
| `listen` | `["127.0.0.1:28414"]` | addresses to listen on; wildcards are refused |
| `allow_from` | none | source networks allowed on non-loopback addresses; required with a LAN address |
| `read_only` | `false` | refuse every action |
| `sway.socket` | empty (discover) | sway IPC socket path |

## Repository layout

| Path | Content |
|---|---|
| `docs/PROTOCOL.md` | the wire protocol, normative |
| `docs/DESIGN.md` | architecture, security model, decisions |
| `docs/RESEARCH.md` | why, with the alternatives |
| `docs/IMPLEMENTATION_PLAN.md` | slices and order of work |
| `protocol/vectors/v1/` | conformance vectors shared by every client |
| `crates/swayconnect-protocol` | messages, frames, proofs, pairing URI |
| `crates/swayconnect-sway` | sway IPC client and the only command builder |
| `crates/swayconnect-daemon` | `swayconnectd` |
| `crates/swayconnect-cli` | `swayconnect` and the Rust client library |
| `crates/swayconnect-testkit` | fake sway server for tests |
| `clients/android` | Android app (emulator only, see Status) |
| `packaging/systemd` | user unit |

## Development

Needs [`just`](https://github.com/casey/just) and [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny).

| Command | Does |
|---|---|
| `just check [crate]` | type-check |
| `just lint [crate]` | clippy, warnings are errors |
| `just test [crate]` | tests |
| `just fmt-check` | formatting check |
| `just deny` | licenses and advisories |
| `just ci` | the full gate |

Tests use a fake sway on a temporary Unix socket. No test connects to a real sway session.

Android, from `clients/android`, with an Android SDK (`platforms;android-37.0`) and a JDK (Temurin 25 is tested; `android.yml` pins 21, untested): `./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug`. Rules for contributors and coding agents are in [`AGENTS.md`](AGENTS.md).

## License

MIT. See [`LICENSE`](LICENSE).
