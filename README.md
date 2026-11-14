# swayconnect

Remote control for a [sway](https://swaywm.org/) desktop. A daemon on the computer (`swayconnectd`) shows paired devices the live workspaces and windows and lets them act on them: focus, move, float, close, keep the desktop awake, lock, set the volume, control media, start apps and type. Clients are a Rust command line tool (`swayconnect`) and an Android app.

## Status

**Stage 1 is written. It is not closed and nothing is released.**

| Checkpoint | Content | State |
|---|---|---|
| V1 "see" | pair, read state, watch live changes | Written. Tested against a fake sway (`just ci`). The read-only live check on the owner's real sway is done. |
| V2 "act" | focus a workspace, focus a window, fullscreen on or off | Written. Tested against a fake sway only. No action has reached a real sway: the owner has not run step S12 yet. |
| V3 "phone" | Android app | Builds and passes its JVM tests locally. Ran on one emulator. The owner has not run step S13 yet. |
| v2 M1 "UI/UX" | layout map, app icons, new Android shell and desktop | Written. `just ci` and the Android gate are green. Seen on one emulator against a `read_only` daemon on the real sway; actions only against a fake sway. The owner has not run gate G-M1 yet. What was and was not checked: `docs/V2_PLAN.md`, Status. |
| v2 M2a "desktop controls" | move, float, layout, scratchpad, close; keep awake; lock; audit log | Written. `just ci` and the Android gate are green. Tested against a fake sway and the testkit's `fake-proc` only. No real `systemd-inhibit`, lock script or window action has run. The owner has not run gates G-WIN, G-KA, G-LOCK. |
| v2 M2b "devices and typing" | audio, media, app launch and launcher, remote keyboard | Written. `just ci`, `just real` and the Android gate are green. Since M2c these run against the real programs in a sealed test session (headless sway, private PipeWire and D-Bus): real `pactl`, an MPRIS player on a real bus, real `systemd-run --user --scope`, fuzzel, and the virtual keyboard typing into foot. The app drove all four on an emulator against that session. Nothing ran on the owner's live desktop. The owner has not run gates G-AUDIO, G-MEDIA, G-LAUNCH, G-INPUT, G-PHONE. |

What v2 adds, and what is proven so far:

| Feature | Command line | Android | Proven against | Owner gate still to run |
|---|---|---|---|---|
| Layout map and app icons | `layout`, `icon` | Desktop map, list | real sway, read-only | G-M1 |
| Move, float, layout, scratchpad, close a window | `move`, `float`, `close --yes`; `raw` for layout and scratchpad | Desktop card buttons and sheets; close needs a hold | fake sway | G-WIN |
| Keep awake (logind idle inhibitor, 1 h default, 2 h max) | `awake on [--for 90m]`, `awake off` | More: first tile, duration sheet | fake `systemd-inhibit` | G-KA |
| Lock the screen | `lock` | More: round button | fake lock script | G-LOCK |
| Volume, mute, default device | `volume`, `mute`, `audio devices`, `audio default` | Audio area, volume keys, More: Mic | real `pactl` on a sealed PipeWire with null sinks | G-AUDIO: speakers and wob OSD |
| Play/pause, next, previous | `play`, `media next`, `media previous`, `media players` | Audio card, now-playing capsule | a test MPRIS player on a sealed D-Bus | G-MEDIA: a browser tab, Spotify |
| Start an app, open the launcher | `launch`, `apps`, `launcher open` | Launch area, More: Launcher | real `systemd-run --user --scope` (test units, stopped), real fuzzel in a headless sway | G-LAUNCH: his own apps, a daemon restart |
| Type text, press keys | `type` (stdin), `key`, `--launcher` | Keys area | the daemon's virtual keyboard typing into foot and fuzzel in a headless sway | G-INPUT: his keyboard layout, locked screen |
| Audit log of tier 2 actions | `swayconnectd audit` | none | tests | none |

Every gate is in `docs/V2_PLAN.md` section 5. Only the owner runs them. For keep awake, lock and window actions, the gate is the first real run. For audio, media, launch and typing, `just real` already runs the real programs in a sealed session (`docs/M2C_PLAN.md`), so their gates check only what needs the live desktop: speakers and the wob OSD, a browser or Spotify player, his own app list and keyboard layout, the lock screen, and a real phone over the LAN (G-PHONE).

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
- **A paired device can do a lot by default.** `swayconnectd pair` grants `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys` and `input.text`. So a paired phone can read your window titles (URLs, document names, chat partners), steer focus, start apps, open the launcher and type into the focused window. Fuzzel plus typing reaches a terminal even without `launch.risky`: a stolen, unlocked phone can run code as you (`docs/DESIGN.md` D30, the owner's accepted risk). What limits it: typing is refused while the screen is locked or a keyboard grabber runs and stops when focus moves; a desktop notice shows when a remote keyboard session starts and when such a device connects; every tier 2 action is in the audit log; revoke and the panic switch end it. Pair only devices you own. Narrow the scopes with `swayconnectd pair --scopes state.read,control` or `swayconnectd devices deny <prefix> <scope>`.
- **Off by default, granted only by name:** `destructive` (close windows), `audio.capture` (unmute a microphone) and `launch.risky` (terminals and the apps in `[apps] risky`).
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

6. v2 controls (none has run against the owner's live desktop yet; the owner gates of `docs/V2_PLAN.md` section 5 come first):

   ```
   swayconnect move <window_id> 3          # or --id <workspace_id>
   swayconnect float <window_id> on
   swayconnect close <window_id> --yes     # needs the destructive scope
   swayconnect awake on --for 90m          # awake off
   swayconnect lock                        # needs [session] lock in the config
   swayconnect audio devices               # sinks and sources, with their ids
   swayconnect volume 30                   # mute on|off; --device <id>
   swayconnect audio default <device_id>   # a source needs audio.capture
   swayconnect media players
   swayconnect play                        # media next|previous; --player <id>
   swayconnect apps                        # entry ids for launch
   swayconnect launch firefox              # desktop entry id, without .desktop
   swayconnect launcher open               # then type or key with --launcher
   echo hello | swayconnect type --window <window_id>   # or --launcher
   swayconnect key --window <window_id> Escape          # or --launcher
   swayconnectd audit -n 20
   ```

   `audio devices`, `media players` and `apps` print a table, or with `--json` one JSON value per row. Exit codes: 1 server refusal or no target in the state, 2 usage (also a risky app without `--yes`), 3 invalid state or result.

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

`SIGUSR1` switches the running daemon to read-only mode until it is restarted. Paired devices can still read state. Every action is refused with `read_only`. It also ends a running keep-awake, closes a launcher the daemon opened and releases every remote key.

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
| `icons.aliases`, `icons.theme` | empty, hicolor only | app id to desktop entry fallbacks; icon theme searched first |
| `session.lock` | not set | argv of the lock command; without it `lock` is not offered |
| `session.lock_processes` | `["swaylock"]` | process names that mean "locked" |
| `session.keyboard_grabbers` | fuzzel, wofi, bemenu, tofi, swaylock | typing is refused while one runs |
| `keep_awake.*` | `systemd-inhibit`, `sleep`; 3600, 7200, 14400, 900 s | inhibitor programs; default, max, continuous cap, cooldown |
| `notify.notify_send` | `/usr/bin/notify-send` | desktop notices |
| `audio.pactl`, `audio.max_volume_pct` | `/usr/bin/pactl`, 100 | sound server tool; highest volume (100 to 150) |
| `apps.systemd_run`, `apps.deny`, `apps.risky`, `apps.terminal` | `/usr/bin/systemd-run`, none, gparted and KernelManager, alacritty `-e` | how apps start; hidden entries; entries that need `launch.risky` |
| `launcher.argv` | `["/usr/bin/fuzzel"]` | what `launcher.open` runs |
| `input.seat`, `wayland.socket` | `seat0`, discover | the virtual keyboard's seat and compositor socket |

## Repository layout

| Path | Content |
|---|---|
| `docs/PROTOCOL.md` | the wire protocol, normative |
| `docs/DESIGN.md` | architecture, security model, decisions |
| `docs/RESEARCH.md` | why, with the alternatives |
| `docs/IMPLEMENTATION_PLAN.md` | stage 1 slices and order of work |
| `docs/UX.md`, `docs/V2_PLAN.md` | v2 screens and v2 slices, status and owner gates |
| `protocol/vectors/v1/` | conformance vectors shared by every client |
| `crates/swayconnect-protocol` | messages, frames, proofs, pairing URI |
| `crates/swayconnect-sway` | sway IPC client and the only command builder |
| `crates/swayconnect-daemon` | `swayconnectd` |
| `crates/swayconnect-cli` | `swayconnect` and the Rust client library |
| `crates/swayconnect-testkit` | fake sway server, `fake-proc` and test configs for tests |
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
| `just real` | audio, media, launch and typing against the real programs in a sealed session; not part of `ci` |

`just ci` tests use a fake sway on a temporary Unix socket, and the testkit's `fake-proc` for every program the daemon starts. `just real` needs sway, foot, fuzzel, grim, PipeWire, WirePlumber, `pactl` and `dbus-daemon` installed. It starts its own headless sway, private D-Bus and PipeWire on a temporary runtime directory. Its one contact with your session is owner decision D1: test apps start as short `app-swayconnect-test-*` scopes in your user systemd, and each test stops its own. No test connects to your sway, sound server, session bus or compositor.

Android, from `clients/android`, with an Android SDK (`platforms;android-37.0`) and a JDK (Temurin 25 is tested; `android.yml` pins 21, untested): `./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug`. Rules for contributors and coding agents are in [`AGENTS.md`](AGENTS.md).

## Releases

Each release on the [Releases page](https://github.com/worxbend/swayconnect/releases) carries a signed `swayconnect-vX.Y.Z.apk` and its SHA-256. To install it on the phone:

1. Open the release page in the phone's browser.
2. Download the APK.
3. Allow your browser to install unknown apps when Android asks.

A release-signed APK cannot update a debug build of the app. Uninstall the debug build first. You will then need to pair again.

To publish a release, push a tag: `git tag v0.2.0 && git push origin v0.2.0`. `.github/workflows/release.yml` then tests the app, builds the APK, signs it and creates the release.

The signing key is in the repository secrets `SWAYCONNECT_KEYSTORE_B64`, `SWAYCONNECT_KEYSTORE_PASSWORD`, `SWAYCONNECT_KEY_ALIAS` and `SWAYCONNECT_KEY_PASSWORD`. To build a signed APK locally, export the same values, with `SWAYCONNECT_KEYSTORE` pointing to the `.jks` file, then run `./gradlew :app:assembleRelease`.

Keep the keystore backed up. If it is lost, the phone cannot install newer releases over the old one.

## License

MIT. See [`LICENSE`](LICENSE).
