# swayconnect v2 research

Status: input to `docs/UX.md`, `docs/DESIGN.md` section 11 and `docs/V2_PLAN.md`. Date: 2026-11-13 (review 2: sections 3, 7.3, 8, 9, 10 updated).
All machine checks were read-only. No build ran. No state-changing command reached sway, PipeWire, logind or any app.

Next action: the owner reads section 7 (idle inhibitor) and section 8 (ranked list), then answers the questions in `docs/V2_PLAN.md` section 0.

## 1. Machine facts (read-only, 2026-11-13)

| Area | Fact | How checked |
|---|---|---|
| sway | 1.12 (`sway 1:1.12-4.1`), wlroots 0.20.2, seat `seat0`, one output DP-1 (Dell P3223QE, 3840x2160, scale 1.25, logical 3072x1728) | `pacman -Q`, `swaymsg -t get_outputs`, `get_tree` |
| Tree | `get_tree` has `rect` (logical px, global), `layout`, `focus`, `nodes`, `floating_nodes`, `deco_rect`, `window_rect`, `percent`, `inhibit_idle`, `idle_inhibitors{application,user}`. Output nodes carry `power`, `dpms`, `scale`, `transform`, `make`, `model`, `rect`. | `swaymsg -t get_tree` |
| Workspace rect | `2,28 3068x1698`: bar and `gaps inner 2` are already removed. Gaps are not exposed. | same |
| Fullscreen | Both workspace nodes report `fullscreen_mode: 1` with nothing fullscreen. Read it on leaf nodes only. | same |
| Events | No sway event for a resize. A layout-mode change most likely has none either (UNVERIFIED). | `man 7 sway-ipc` |
| Audio | PipeWire 1.6.11 + WirePlumber + pipewire-pulse (protocol 35). One sink `alsa_output.pci-0000_2b_00.4.analog-stereo` at 40 %, one source, a `.monitor` source. The HDMI card is on profile `off`: switching to monitor speakers is a card profile change. | `wpctl status`, `pactl -f json list ...` |
| Media | `playerctl -l`: no players now. `org.mpris.MediaPlayer2.playerctld` is D-Bus activatable. | `playerctl -l`, `busctl --user list` |
| Launcher | fuzzel 1.15.0 (with resvg). 50 + 9 desktop entries (task text said 52 + 11; the count differs by filter), 82 flatpak entries. 19 `NoDisplay`, 3 `Terminal=true`, 0 `DBusActivatable`. | file reads |
| Icons | Theme Adwaita (inherits AdwaitaLegacy, hicolor). Icon= resolved for 93 of 94 displayable entries. app_id to entry matched 2 of 3 live ids (8 of 10 windows). Miss: the Android emulator (`Emulator`/`qemu-system-x86_64`). | prototype `proto/icons.py` in the research scratchpad |
| Text input | wtype, ydotool, dotool are not installed. `/dev/uinput` is root only. libwlroots 0.20.2 contains `zwp_virtual_keyboard_manager_v1`, `zwp_input_method_manager_v2`, `zwlr_virtual_pointer_manager_v1`. That sway 1.12 advertises them is likely but UNVERIFIED (no `wayland-info`). No IME daemon runs. Layouts: English (US) + Ukrainian. | `strings`, `swaymsg -t get_inputs` |
| Lock | `lock.sh` runs `swaylock -f`. logind `LockedHint=no` while locked, so logind cannot tell lock state. Sway IPC cannot either. | `loginctl show-session` |
| Clipboard | `wl-paste --watch cliphist store` runs. Anything put on the clipboard is stored on disk. | `pgrep -a` |
| Brightness | No backlight device. DDC/CI needs `ddcutil` and `i2c-dev`, both absent. | `/sys/class/backlight`, `brightnessctl -l` |
| Session env | The user manager has `WAYLAND_DISPLAY=wayland-1`, `SWAYSOCK`, `DISPLAY=:0`. The dev daemon (tmux) has no `WAYLAND_DISPLAY` and a temp `XDG_RUNTIME_DIR`. | `systemctl --user show-environment`, `/proc/<pid>/environ` |
| systemd unit | `swayconnectd.service` has no `KillMode`, so a plain child app dies with the daemon. | file read |

## 2. Layout geometry (mini-map)

1. Everything the map needs is in `get_tree`. `model.rs` drops it today.
2. Compaction: rects relative to the workspace in basis points (0 to 10000), kinds `h|v|t|s`, active child index for tabbed/stacked (first `focus` id that is in `nodes`), floating list in z-order, fullscreen window id, one-child splits collapsed, depth cap 16.
3. Size: the live desktop (2 workspaces, 10 windows) is 730 bytes. Worst case (512 windows, each wrapped) is about 42 KiB. Both fit the 256 KiB desktop cap. The cut rule drops layouts before windows.
4. Freshness: poll `GET_TREE` every 2 s while a client subscribes to `desktop`, and subscribe to `binding` events. Publish only on change.
5. Output geometry comes from output nodes inside `get_tree`. `GET_OUTPUTS` (type 3) is not needed, so the allowed sway message types stay 0, 1, 2, 4, 7.

## 3. Icons

1. Pipeline: app_id (or X11 class) to desktop id (sandbox app id, exact id, case-insensitive id, `StartupWMClass`, unique reverse-DNS suffix, Xwayland instance, `/proc/<pid>` flatpak hints, owner alias map), then `Icon=` through the Icon Theme Spec (Inherits, hicolor, `/usr/share/pixmaps`), then rasterise to a 128 px PNG.
2. Crates: freedesktop-desktop-entry, freedesktop-icons, linicon and freedesktop_entry_parser fail `deny.toml` (MPL-2.0 directly or via `option-ext`). Decision: a hand-written resolver. resvg/usvg 0.50.0 (`default-features = false`) + tiny-skia 0.12.0 (`png-format`) need `BSD-2-Clause` added to `deny.toml` (arrayref).
3. Transport: content hash per icon. `Window.icon` carries the hash. `icon.get` serves only PNGs the daemon produced. No path or name is ever accepted.
4. The icon is a hint, not an identity. Any local app can claim another app's id.
5. Icon files are not owner-trusted: flatpak exports are app-authored. A small SVG (huge blur, deep `<use>` fan-out) or PNG (huge IHDR) can cost gigabytes or minutes in a renderer. So rendering runs in a child process with `RLIMIT_AS` 256 MiB, `RLIMIT_CPU` 2 s and a 3 s kill, after size and node-count checks (`DESIGN.md` 11.5).

## 4. Audio, media, brightness

| Option | Verdict | Why |
|---|---|---|
| `pactl -f json list` + long-lived `pactl subscribe`, writes through `pactl` argv | **chosen** | One tool, stable JSON, pure Rust side, easy fake. Matches the owner's `volume.sh`. |
| libpulse-binding | rejected | C callback main loop, awkward with tokio. |
| pipewire-rs 0.10.1 | rejected | bindgen + clang: a heavy build on a machine that resets under load. Setting a default means writing WirePlumber metadata. |
| `pw-dump -m` | rejected | complete but noisy |
| MPRIS through zbus 6.3.0 (features `tokio`, `proxy`) | **chosen** | Typed D-Bus, no C library. The `mpris` crate pulls libdbus. playerctl output is hard to parse safely when titles hold separators. |
| Album art | **deferred** | `http(s)` art from the daemon is SSRF. `file://` art is an arbitrary-read risk. v2 shows the player's app icon. |
| Brightness | **never on this machine for now** | No backlight. DDC needs system changes the owner must make. |

## 5. Launching apps and fuzzel

1. Index desktop entries by hand (XDG_DATA_DIRS order, first id wins, `Hidden` deletes). Filter `NoDisplay`, `Hidden`, `OnlyShowIn`/`NotShowIn` against `sway`, `TryExec`. Hide entries whose Exec needs `%u`/`%U` only (URL handlers) by default.
2. Spawn: Exec split by the Desktop Entry quoting rules, every field code removed, run as `systemd-run --user --scope --unit=app-swayconnect-<id>-<rand>.scope -- <argv>` with an explicit environment allowlist. The app gets its own cgroup and survives a daemon restart.
3. Never `swaymsg exec`: sway runs it through `/bin/sh -c`.
4. fuzzel is a layer-shell surface. It has no sway window id. While it is open, keys go to fuzzel although `get_tree` still names the old window as focused.

## 6. Text input

| Option | Verdict | Why |
|---|---|---|
| Daemon-owned `zwp_virtual_keyboard_v1` client (wayland-client 0.31.15, wayland-protocols-misc 0.3.12, rustix 1.1.6 memfd, xkeysym 0.2.1) with a generated XKB keymap like wtype | **chosen** | Works for every Wayland and Xwayland client, any Unicode text, independent of the US/UA layout. Pure Rust, light build. |
| input-method-v2 `commit_string` | later, as a "safe text" mode | only apps with text-input-v3; one IM per seat; no shortcuts |
| ydotool / uinput | rejected | needs a system-wide `/dev/uinput` grant |
| wtype CLI | rejected | not installed; text in argv is readable in `/proc` |
| clipboard + paste | rejected | cliphist stores the text on disk |

Risks that shape the design: typing into a terminal is code execution. Key events also trigger sway bindings. A virtual keyboard probably reaches swaylock, which would be remote unlock. Any layer surface with exclusive keyboard (fuzzel, the power-menu, swaylock) takes the keys while the tree still shows the old focused window. Sway issue #8143: recreating the virtual keyboard can loop focus, so keep one device per daemon run.

## 7. Idle inhibitor ("keep awake")

The owner runs:
`swayidle -w timeout 300 lock.sh timeout 600 'swaymsg "output * power off"' resume 'swaymsg "output * power on"' before-sleep lock.sh` (pid 920).

### 7.1 What was checked

| Check | Result | Source |
|---|---|---|
| swayidle version | 1.9.1 | `pacman -Q swayidle` |
| Built with systemd | yes, links `libsystemd.so.0` | `ldd` |
| Honours logind idle inhibitors | **yes**. `get_logind_idle_inhibit()` reads `org.freedesktop.login1.Manager.BlockInhibited` and checks for `idle`. A `PropertiesChanged` match on `/org/freedesktop/login1` calls `disable_timeouts()` or `enable_timeouts()`. `enable_timeouts()` refuses while the inhibitor exists ("Not enabling timeouts: idle inhibitor found"). | swayidle v1.9.1 `main.c` lines 252-280, 340-380, 600-635 (fetched with `gh api`); same strings in the installed binary |
| Weak inhibitors | swayidle reads `BlockInhibited` only. systemd 262 also has `BlockWeakInhibited`. So the mode must be `block`, not `block-weak`. | `busctl introspect org.freedesktop.login1 ...` |
| logind signals the change | `BlockInhibited` is `emits-change`. Current value `""`. | same |
| Permission | polkit `org.freedesktop.login1.inhibit-block-idle`: implicit any, inactive and active are all `yes`. A user service needs no prompt. | `pkaction --verbose` |
| Side effects in logind | `IdleAction` is commented out (default `ignore`). An idle block changes nothing else here. | `/etc/systemd/logind.conf` |
| Honours Wayland inhibitors | yes. Timeouts are registered with `obey_inhibitors = true`, which uses `ext_idle_notifier_v1.get_idle_notification`. | `main.c` lines 578-597, 615 |
| Pitfall | `disable_timeouts()` destroys the idle notifications without running the `resume` command. If the outputs were already powered off by the 600 s timeout, taking the inhibitor leaves them off: no `resumed` event ever comes. | `main.c` lines 571-576, 620-635, 645-660 |
| Output power state | Output nodes in `get_tree` carry `power` (live: `true`). | `get_tree` |
| Sway `inhibit_idle` | exists (`man 5 sway`). The tree shows `inhibit_idle` and `idle_inhibitors` per view. Live: none of the views inhibit. The owner's sway config has no `inhibit_idle` rule. | `man 5 sway`, `get_tree`, `grep` |

Not checked live, by rule: that holding the inhibitor really stops the 300 s lock on this desktop. Owner check in `docs/V2_PLAN.md` gate G-KA.

### 7.2 Candidates

| Candidate | Stops swayidle here? | Fails safe if the daemon dies? | Verdict |
|---|---|---|---|
| logind `Inhibit("idle", ..., "block")` held by a `systemd-inhibit ... sleep <N>` child | yes (7.1) | **yes**. The child ends after N seconds even if the daemon hangs. A unit restart kills it at once. | **chosen** |
| logind inhibitor held through zbus inside the daemon | yes | yes on crash, but a hung daemon holds it forever | rejected: the child gives the expiry for free |
| sway `[con_id=N] inhibit_idle open` | yes (obeyed Wayland inhibitor) | **no**. It stays until the window closes. A crash leaves it on. It can also clobber an owner `for_window ... inhibit_idle` rule. Needs a window. | rejected |
| `zwp_idle_inhibit_manager_v1` on a daemon surface | probably (sway treats an inhibitor without a view as visible; UNVERIFIED on 1.12) | yes on crash | rejected for now: needs a Wayland surface; the logind path is simpler |
| `SIGSTOP` to swayidle | yes | **no**. A frozen Wayland client can be disconnected by sway; `before-sleep` locking stops too; a lost `SIGCONT` means the desktop never locks | **rejected as unsafe** |

### 7.3 Chosen mechanism

1. On `keep_awake.set {enabled: true, duration_s}` the daemon spawns `/usr/bin/systemd-inhibit --what=idle --mode=block --who=swayconnect --why=<fixed text> /usr/bin/sleep <n>` (both paths from config). Argv only. `n` is an integer.
2. If any active output in the snapshot has `power: false`, the daemon then sends the fixed command `output * power on`. Reason: the pitfall in 7.1.
3. Off = kill the child. A new `on` replaces the child (new expiry).
4. Expiry: default 1 h, max 2 h per request (config). A continuous cap of 4 h, counted across short breaks: swayidle restarts its timeouts from zero when the inhibitor goes, so a 1 s off period would never let it lock. Only an off period of at least `cooldown_s` (default 15 min, longer than the 600 s timeout) resets the count; after the cap, `true` is refused for that long. So a stolen phone cannot keep an unattended desktop unlocked for ever by toggling.
5. Released on: expiry, `off`, revoke of the holding device, `SIGUSR1`, daemon stop. Not released on phone disconnect: the couch use case needs it to outlive a phone screen-off.
6. Scope `idle.inhibit` is tier 2 and off by default: it defeats the owner's lock policy for a while, so it is granted by name only.

## 8. Ranked use cases

V = value, R = risk if a paired phone is stolen or hostile (1 to 5).

### 8.1 v2 (M2), in build order

| # | Use case | V | R | Scope | Mechanism |
|---|---|---|---|---|---|
| 1 | Window ops: move to workspace, floating, layout, scratchpad | 4 | 1 | `control` | typed sway commands |
| 2 | Keep awake with expiry | 4 | 3 | `idle.inhibit` | section 7 |
| 3 | Lock the computer | 4 | 1 | `control` | config argv (`lock.sh`) |
| 4 | Close window | 3 | 3 | `destructive` | `[con_id] kill` |
| 5 | Volume, mute, default sink; mute the mic | 5 | 1 | `audio` | pactl |
| 6 | Unmute the mic, mic level, default source | 3 | 4 (listening in) | `audio.capture` | pactl |
| 7 | Phone volume keys drive the desktop (while Audio is shown) | 4 | 1 | `audio` | phone only |
| 8 | Now playing: play/pause, next, previous | 5 | 1 | `media` | zbus MPRIS |
| 9 | App launcher with icons and search | 4 | 3 | `launch` (`launch.risky` for terminals) | own index + systemd-run |
| 10 | Open fuzzel on the computer, type into it | 3 | 3 | `launch` + `input.text` | config argv + virtual keyboard |
| 11 | Remote keyboard: text, special keys, shortcuts | 5 | 5 | `input.text`, `input.keys` | virtual keyboard |

Rows 1 to 4 are M2a, rows 5 to 11 are M2b.

### 8.2 Later

Owner buttons (`commands`: screenshot, record, night light, power menu), per-app streams, HDMI card profile, media seek, Live typing mode, presets, Keystore key hardening, touchpad (`input.pointer`, wlr virtual pointer), presentation mode, output power on/off as its own control, URL share to the desktop, clipboard phone to desktop, Quick Settings tiles, media notification with a foreground service, urgent-window alerts, Wake-on-LAN, keyboard layout switch, context button pages, suspend/reboot/power off, biometric step-up (unless the owner wants it before typing ships), album art, input-method "safe text" mode.

### 8.3 Never

Arbitrary commands or sway `exec` with client text. Raw sway commands. Remote unlock, including typing into swaylock. Reading cliphist history. Live screen streaming (use wayvnc). Reading the desktop's keystrokes. Cloud relay. Wear OS.

## 9. UI toolkit facts (bytecode of the BOM jars)

- Compose BOM 2026.09.00 pins material3 1.4.0, foundation/ui/animation 1.12.1.
- Stable: `HorizontalPager`, `rememberPagerState`, `PagerState.settledPage/currentPageOffsetFraction`, `SingleChoiceSegmentedButtonRow`, `ListItem`, `LazyVerticalGrid`, `Modifier.animateItem`, `SharedTransitionLayout`, `Modifier.sharedBounds`, `LookaheadScope`, `Modifier.animateBounds`, `MaterialTheme(colorScheme, motionScheme, shapes, typography, content)`, `MotionScheme` interface, `rememberTextFieldState`, `BasicSecureTextField`, `KeyboardOptions(showKeyboardOnFocus, ...)`, `Modifier.minimumInteractiveComponentSize()`.
- `@ExperimentalMaterial3Api`: `ModalBottomSheet`, `SearchBar`, the `SliderState` slider, carousels, `TooltipBox`.
- Not in 1.4.0: `ButtonGroup`, `FloatingToolbar`, `LoadingIndicator`, `MaterialShapes`. `MotionScheme.standard()/expressive()` are internal.
- `InterceptPlatformTextInput(interceptor, content)` and `PlatformTextInputMethodRequest.createInputConnection(EditorInfo)` are in ui 1.12.1 with no opt-in annotation (bytecode check 2026-11-13). Wrapping a text field in it lets the app add `EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING` and `IME_FLAG_NO_EXTRACT_UI` to `imeOptions`. The earlier claim that no Compose path can set the flag was wrong.
- Also stable in 1.12.1: `rememberSnapFlingBehavior(LazyListState, SnapPosition)`, `SnapPosition.Center`, `LocalAccessibilityManager` with `calculateRecommendedTimeoutMillis`. activity-compose 1.13.0 has `BackHandler` and `PredictiveBackHandler`.
- To verify before coding: `ShortNavigationBar`, `SecureTextField`, the newer `HapticFeedbackType` constants and their API 29 fallback.

## 10. Security model research, condensed

1. The per-device key `K` stays the "API key". No per-scope keys, no session tokens, no attestation. Later hardening on Android: import as a non-exportable AndroidKeyStore `HmacSHA256` key, `setUnlockedDeviceRequired(true)`, StrongBox when present. Deferred from v2: `Proofs.kt` takes the key as `ByteArray`, so it first needs a `DeviceKeyMac` interface in `:protocol`.
2. Fine-grained scopes now, because splitting a scope later breaks paired devices (D9).
3. Grants only at the desktop (admin socket). No client scope-request op.
4. Biometric step-up (server-verified ECDSA P-256 signature) is the only client-side gate a patched client cannot skip. It costs a new crypto flow and enrolment UX. Deferred unless the owner wants it before typing ships. API facts: on API 29, per-use auth (`CryptoObject`, validity 0) needs a Class 3 biometric; a time-bound key (`setUserAuthenticationValidityDurationSeconds(30)`) accepts PIN, pattern or password through `BiometricPrompt.Builder.setDeviceCredentialAllowed(true)` (a system dialog, so it fits the all-Compose rule). On API 30+: `setUserAuthenticationParameters(30, AUTH_BIOMETRIC_STRONG or AUTH_DEVICE_CREDENTIAL)`. So a time-bound step-up key works on `minSdk` 29 without fingerprint hardware.
5. Audit log, markup-safe desktop notifications (mako renders Pango markup, so never a client-chosen string), panic switch extended to drop keep-awake and input. A status file for a waybar indicator is deferred.
6. Server-side gates for what a patched client could skip: `audio.capture` for opening the microphone, `launch.risky` for terminals and owner-marked apps. Client holds are UX only.
7. PipeWire node names are chosen by whatever created the node. `pactl` parses options with `getopt_long`, which permutes arguments, so a node named `--server=...` would be read as an option. Every pactl argv puts `--` before names, and names starting with `-` are dropped.

## 11. Sources

- Machine: `swaymsg -t get_tree/get_workspaces/get_outputs/get_seats/get_inputs`, `wpctl status`, `pactl -f json list`, `pw-dump`, `playerctl -l`, `busctl`, `systemd-inhibit --list`, `pkaction`, `pacman -Q`, `ldd`, `strings`, reads of `~/.config/sway`.
- `man 5 sway`, `man 5 sway-input`, `man 7 sway-ipc`, `man swayidle`.
- swayidle v1.9.1 `main.c`: https://github.com/swaywm/swayidle/blob/v1.9.1/main.c
- wtype: https://github.com/atx/wtype/blob/master/main.c
- Virtual keyboard protocol: https://wayland.app/protocols/virtual-keyboard-unstable-v1 ; sway issue #8143.
- Material tabs swipe rules: https://m2.material.io/go/design-tabs ; Compose pager: https://developer.android.com/develop/ui/compose/layouts/pager
- Android 16 output switcher: https://androidauthority.com/android-16-media-output-switcher-3547363
- KDE Connect: https://kdeconnect.kde.org/ , advisory https://kde.org/info/security/advisory-20250418-2.txt
- Touch targets: https://support.google.com/accessibility/android/answer/7101858
- crates.io API, checked 2026-11-13, for every crate version named here.
