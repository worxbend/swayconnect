# swayconnect v2 implementation plan

Date: 2026-11-13 (review 2). Inputs: `docs/UX.md`, `docs/DESIGN.md` section 11, `docs/PROTOCOL.md` section 17, `docs/V2_RESEARCH.md`.
Three milestones, in the owner's order: **M1 "UI/UX"**, then **M2a "desktop controls"** (window ops, lock, keep awake), then **M2b "devices and typing"** (audio, media, launch, keyboard).

Next action: the owner runs gates G-M1, then G-WIN, G-KA, G-LOCK, then G-AUDIO, G-MEDIA, G-LAUNCH, G-INPUT, G-PHONE (section 5). Since M2c, audio, media, launch and typing also ran against the real programs in a sealed session (`docs/M2C_PLAN.md`); the gates keep only what the live desktop can show.

## Status: M2c review fixes on 2026-11-14

All 18 review findings are fixed (none rejected, none deferred). Details in `docs/M2C_PLAN.md` 3 and 7.

| Check | Result |
|---|---|
| `just ci` | green: 970 Rust tests, fmt-check, clippy `-D warnings`, `cargo deny` |
| `just real` | green: 17 tests (testkit `real_session` 7, daemon `real_audio` 3, `real_input` 3, `real_launch` 2, `real_media` 2) |
| Android gate | green: `:protocol:test` 78 (with the new vector `088`), `:app:testDebugUnitTest` 299; lint 0 errors, the same 3 warnings. No Android code changed, so no new screenshots. |
| Live session | read only before and after: sinks (volume included), sources, sway windows, `app-swayconnect-*` units identical |
| By hand | `just sealed-session`: SIGKILL of the session process left no process in its group and none naming its runtime dir (the temp root stays, as documented); end of stdin with `--allow-user-systemd` removed everything |

Fixed: the launcher target now needs the launcher to hold the keyboard (no focused node), with a 1 s wait after `launcher.open` (DESIGN 11.7, PROTOCOL 17.13, vectors README `keyboard`); the CLI picks the player as the app does (PROTOCOL 17.10); the harness seals every start path (bare commands inherit nothing; `output`, `checked_env`), resolves symlinks, refuses links into the live runtime dir except the D1 link, falls back to `/run/user/<uid>`, kills its group through a supervisor when the test process dies, keeps logs after a panic, reads no host PipeWire/WirePlumber drop-in, and kills a timed-out helper without a pid race; `flock -o` keeps the agent lock out of every child (justfile, AGENTS.md); the D1 cleanup stops only units of the fixture entries; every real rig checks the live sound server and leftover processes outside the group; `real_input` waits on sway state, not screenshots; DESIGN 11.8 states the D2 limit in the stolen-phone row.

Not verified: the SIGKILL path inside a real test with a running daemon (only the bare session by hand); the guard wait under a slow fuzzel start (the real run passed, its duration was not measured).

## Status: checkpoint M2c by INT on 2026-11-14

Every M2c slice (R0 to R4, G-CLI, G-APP, G-FIX) is written. INT checked the whole tree:

| Check | Result |
|---|---|
| `just ci` | green: 961 Rust tests, fmt-check, clippy `-D warnings`, `cargo deny` |
| `just real` | green twice in a row: 15 tests (testkit `real_session` 5, daemon `real_audio` 3, `real_input` 3, `real_launch` 2, `real_media` 2), about 10 s of tests each run; no session, process or `app-swayconnect-test-*` unit left after either run |
| Android gate | green: `:protocol:test` 78, `:app:testDebugUnitTest` 299, 0 failures; lint 0 errors, the same 3 warnings |
| Live session | read-only before and after: sinks, sources, both defaults, the sway tree (window ids and app ids), `app-swayconnect-*` units and the PipeWire node count (7) were identical |

R5, emulator against the real programs: a sealed session like `RealSession` (private `XDG_RUNTIME_DIR` under `/tmp`, own `dbus-daemon`, PipeWire + WirePlumber `policy` with two null sinks and a null source, headless sway on pixman), started by a scratch script with `env -i` and sealed values only, and in it `swayconnectd` with `read_only = false` on `127.0.0.1:28541`, `pactl`, `systemd-run` (D1, through `<rt>/systemd/private` only) and `fuzzel` real, notices, keep awake and lock on `fake-proc`, desktop entries from `tests/fixtures/real-launch`, plus `mpris-test-player`, a `foot` running `tee` as the typing target and two read-back windows (pactl and playerctl state, once a second). API 37 emulator, separate Android user, paired by link (host rewritten to `10.0.2.2`, codes compared). Screenshots in `~/.cache/swayconnect-m2c/` (`NN-*-desktop.png` from `grim`, `NN-*-phone.png`; Keys is `FLAG_SECURE` and captures black, so its text is in `*.uiautomator.txt`):

| Step on the phone | Seen on the sealed desktop |
|---|---|
| Audio: slider tap, then drag | `sealed_sink_1` 36 %, then 66 % (`01`) |
| Audio: mute; tap the second sink | `Mute: yes`; default sink `sealed_sink_2` (`02`, `03`) |
| Media: Play, Next, Previous, Pause | player log `PlayPause Next Previous PlayPause`, status and title follow (`04` to `06`) |
| Launch: Swayconnect Test Foot | real `app-swayconnect-test-foot-16c379b9.scope`, window `swayconnect-test-r4` in the headless sway (`07`); INT stopped the unit through the sealed `systemctl --user`, window gone (`08`) |
| Launch: the terminal entry | Locked sheet with `swayconnectd devices grant 6xWT launch.risky`, no unit (`09`) |
| Desktop: window card tap | `FocusWindow(5)`, sway focus moved to `r5-typing` |
| Keys: Esc, then `hello from the phone 42` + Send ⏎ | `typed.txt` = `ESC hello from the phone 42\n` (`10`); the first key was not lost here |
| Launch round button, type `term` | real `fuzzel` (the daemon's child, sealed env only) filters to Swayconnect Test Terminal (`11`, `12`) |
| Keys: Esc to fuzzel | fuzzel gone, the target back on `r5-typing`; `back in foot` + Send ⏎ arrives (`13`, `15`) |
| More: Mic tap; grant `audio.capture`; Mic hold | muted with the lock badge; a tap does not unmute, the 600 ms hold does; notice "Microphone unmuted remotely (device 6xWT)" (`16` to `18`) |
| More: Launcher tile | fuzzel opens, Keys shows "Launcher (fuzzel)"; Esc closes it (`19`) |

Notices (fake `notify-send` log): one connect notice for the whole walk, across two app reinstalls and a scope change (D2), two "Remote keyboard active", one "Microphone unmuted remotely".

### Fixed by INT (M2c)

1. Android, media: the now-playing card showed "Nothing playing" for a paused player. The server sends `active_player_id: null` until a player has played during its run (wire vector `ext/media.json` allows it), and the app took only the active player or one that plays. A paused Spotify or browser tab could not be started from the phone. `MediaView.activePlayer` now falls back to the first paused player. Test `a paused player that has not played since the server started is the active one` (red before). UX 7.2 updated.
2. Android, Keys: after Esc closed fuzzel, the card read "Nothing had the focus on the computer". While fuzzel holds the keyboard, sway 1.12 marks no window focused, and the `session` state (launcher closed) reached the app before the `desktop` state with the focus back, so the fallback target was null. `KeysView.awaitingFocus` now hands the target to the next focused window (`withKeysFollowingDesktop`, called on each desktop state); a target the user picks is kept. Test `a launcher that closes before sway shows the focus again hands the target to the next focused window`. UX 9.2 updated. Seen fixed on the emulator.
3. Docs: DESIGN 11.6 pactl argv (`-- set-sink-volume <name> <N>%`, R1's fix) and 11.7 launcher focus rule (R3's fix) written in.

### Not done, not verified, open (M2c)

- The CLI `play` without `--player` still fails with "no active player" when no player has played yet. Fixed in the review round: the CLI uses the app's order (PROTOCOL 17.10).
- After an app update, opening the app shows the pairing link form again ("This replaces sealed-r5"): the update drops the saved state, and the task's first intent was the link. Back returns to the computer, and the pairing window has long closed. Not fixed.
- A slider drag that starts on the right padding, past the thumb, did not move the value; a drag from the track and a tap did. Not investigated.
- R5 used a scratch script, not `RealSession` (a Rust test cannot stay up for a manual walk). It mirrors `RealSession` but has no guard of its own: every program got `env -i` plus sealed values, checked afterwards by reading `/proc/<pid>/environ`. Only the supervising bash shells carried the live environment. Since the review round a walk starts the session with `just sealed-session`, which has the guard and teardown of `RealSession`.
- `launcher_closed` itself was not provoked from the phone (the session state always came first); `real_input` covers it.
- Window ops (G-WIN), keep awake and lock were not part of M2c; only window focus ran on the sealed sway.
- Open slice items stay as the slices reported them in `docs/M2C_PLAN.md` 7.

## Status: checkpoints M2a and M2b by INT on 2026-11-14; owner gates open

Every M2a and M2b slice of section 3 is written. INT checked the whole tree on 2026-11-14:

| Check | Result |
|---|---|
| `just ci` | green: fmt-check, clippy `-D warnings`, 908 Rust tests, `cargo deny` (warnings only: duplicate `getrandom`, `miniz_oxide`, `syn`) |
| Android gate | green: `:protocol:test` 78 tests, `:app:testDebugUnitTest` 282 tests, 0 failures; lint 0 errors, the same 3 warnings (`OldTargetApi`, `MissingApplicationIcon`, `UseKtx`) |
| Default pairing | `swayconnectd pair` on a real binary gave exactly `state.read, state.titles, control, idle.inhibit, audio, media, launch, input.keys, input.text`, to the app and to the CLI. |
| Emulator, live sway, read-only | API 37 emulator, separate Android user, paired by link with a `read_only` daemon on the live sway (temp XDG under `/tmp/claude-1000/sc.bXLh`, `127.0.0.1:28521`; every program but `pactl` pointed at `fake-proc`; no Wayland socket). Seen with real data: the owner's workspace and window with its icon; Audio with the real sink (15 %) and source (100 %) from `pactl -f json list`, both defaults; media "Nothing playing" (`playerctl -l` also lists none); Launch with the owner's real app list and icons, the risky shield on btop++ and Kernel Manager; More with keep awake Off (topic `session`: not locked, no inhibitor), Scratchpad Empty, Lock; the Computer sheet with the nine default scopes and the three by-name scopes off. Every action control is disabled with "The computer is set to look only.". Server side, through a CLI paired with the same daemon: `window.floating`, `window.move`, `scratchpad.show`, `keep_awake.set`, `session.lock`, `audio.set_mute`, `audio.set_volume`, `app.launch`, `launcher.open`, `input.text`, `input.key` each answered `read_only`, no fake program was started, and the live sway got no command. Light and dark. |
| Emulator, fake sway | A second daemon (`read_only = false`, `127.0.0.1:28522`) on the testkit `fake_sway` example, every program path at `fake-proc`, desktop entries from `tests/fixtures/desktop-entries` (the fixture root, which lists nothing: the data dirs are `<fixture>/high:<fixture>/low`, found by G-FIX in M2c), no session bus, no Wayland socket. The fake sway recorded, in order: Float → `[con_id=6] floating enable`; Move sheet pill 2 → `[con_id=6] move container to workspace number 2`; window sheet Tabbed → `[con_id=6] layout tabbed`; Scratchpad → `[con_id=6] move scratchpad`; Close hold (after `devices grant … destructive`) → `[con_id=6] kill`, with one `window.close` audit line; Drive card tap → `workspace --no-auto-back-and-forth number 2`; window card tap → `[con_id=7] focus`. Fake programs: Audio card tap → `pactl set-default-sink -- alsa_output.pci-0000_2b_00.4.analog-stereo`; Launch Firefox → `systemd-run --user --scope --collect --unit=app-swayconnect-firefox-ab65c6f2 -- /usr/lib/firefox/firefox` and the ghost tile "Opening Firefox…"; Launch round button → fake `fuzzel` started and the app moved to Keys with the target "Launcher (fuzzel)". Keep-awake tile → fake `systemd-inhibit --what=idle --mode=block --who=swayconnect --why=… <fake sleep> 3600`, fake notice "Keep awake on (device 4o4t)", tile "Awake · 60 min left"; long press → duration sheet (15 min, 30 min, 1 h, 2 h, "1 h from now"), 30 min → a new inhibitor with `1800`; tap → off; three `keep_awake.set` audit lines. Lock round button → fake lock script. Keys Esc chip → `input.key`, fake notice "Remote keyboard active (device 4o4t)", then "This needs a service that is not running on the computer." (no Wayland socket: `backend_unavailable`); typed "hi" + Send → "Sent 0 of 2 characters.", the text stays; audit lines carry `text_len` 2, never the text. Light and dark. |

Screenshots: `~/.cache/swayconnect-m2/` (`ro-*` read-only live, `fake-*` fake sway). Keys and Pair capture black: `FLAG_SECURE` works; they were read with `uiautomator dump`.

### UX 18 two-tap audit (QA2)

| Action | Path seen | Result |
|---|---|---|
| Focus a window | Desktop → card | 2, `[con_id=7] focus` |
| Switch the desktop workspace | Desktop → card (Drive) | 2, `workspace … number 2` |
| Fullscreen / float the focused window | Desktop → card button | 2, `floating enable` |
| Move a window to a workspace | long press → number pill; or Move → pill | long press + 1, or 2 |
| Close a window | Desktop → Close + hold | 2, `kill` (needs `destructive`) |
| Volume of the default output | Audio → slider, or volume keys | not exercised: the single-stdout fake `pactl` cannot report a default device, so no slider shows; the live read-only run showed the slider disabled |
| Mute the mic | Audio → Mute | not exercised for the same reason; More has no Mic tile (open, see below) |
| Unmute the mic (`audio.capture`) | More → Mic + hold | not possible: no Mic tile |
| Play / pause | capsule | not exercised: the daemon binary has no fake media backend, and the live session had no player |
| Keep awake on / off | More → tile | 2, fake `systemd-inhibit`; the status-line pill is not wired (A3.2a note) |
| Lock the computer | More → round button | 2, fake lock script |
| Launch an app | Launch → recent icon | 2, fake `systemd-run`; Firefox showed under Recent after the first launch |
| Open fuzzel | Launch → round button | 2, fake `fuzzel` |
| Type into the focused window | Keys → type → Send | 2 + typing, reaches the daemon; `backend_unavailable` because the binary has no fake keyboard |
| Esc / Ctrl+C | Keys → chip | 2, same |

### Fixed by INT

1. `tests/limits.rs`: the two "closed after N s" tests measured on a paused clock. The M2b audio watcher retries a missing `pactl` on a 1, 2, 4 … s backoff, and the paused clock jumped along those timers while the accept or close was still in the kernel, so the tests read 7, 15 or 31 s (2 of 5 runs). They now measure real time (5 s added to the run).
2. `proc/tests.rs` (D-PROC flake reported by four slices): `spawn` returns when close-on-exec fires, before the kernel has set up the child's environment, so `/proc/<pid>/environ` sometimes read empty. The test now waits for the fake's own call line through a FIFO log.
3. Pair step 4 lacked the UX 11 line for the default pairing: it now says "This phone can type and start apps on the computer. Remove it there if the phone is lost:" with `swayconnectd revoke` in mono (`PairCapabilities.canRunCode`, unit tested; seen on the emulator).
4. Pair "Done" kept the area shown before pairing (More); it now opens the Desktop area (UX 11 step 4; view-model test extended; seen).
5. Focused window card: the 1dp divider before Close stayed alone at the end of the first row when Close wrapped. Divider and Close are now one item (seen).
6. Audio card: an unplugged default microphone read "Microphone · Live". It now reads "Microphone · Unplugged" (seen with the live source, whose ports are all "not available").

### Not done, not verified, open

- More has no Mic and Launcher tiles (UX 10.1, M2b): UX 18 "More → Mic" cannot be done; "Audio → Mute" and "Launch → round button" can. No settings rows (no settings store; A11 note).
- Emulator: the host ran other heavy jobs (load 50 to 80) for part of the walk. The emulator then reported 93 % CPU pressure, and the app, the Pixel launcher, Play services and `system` all showed "isn't responding" dialogs; INT rebooted the emulator once. The app used 1.6 % CPU in that ANR report and its main thread was in a normal recomposition; the same steps ran without a dialog once the load fell. Read as starvation, not an app hang, but not proven. Not seen on the device: `scratchpad.show` (the fixture scratchpad is empty), the capsule, the "PC 45 %" pill, font scale 2.0, landscape, TalkBack as a service.
- Daemon binary has no test mode for the fake media backend or the fake keyboard, and one fake `pactl` prints the same text for every call; so audio volume and mute, media and typing were exercised only in the integration tests (`tests/v2_m2b.rs`, `crates/swayconnect-cli/tests/e2e_m2b.rs`), not on the device.
- No real program ran: `systemd-inhibit`, `notify-send`, the lock script, `pactl` writes, `systemd-run`, `fuzzel`, MPRIS commands, the virtual keyboard. Those are the owner gates.
- Launch on the live data: the Blanket entry shows an empty disc, no icon and no monogram. Not investigated.
- Spec gaps the slices reported and not yet written into `PROTOCOL.md`, `DESIGN.md` or `UX.md`: sway offline for `session.lock`, `keep_awake.set` and every M2b op; `backend_unavailable` with no producing state for keep-awake and lock; `duration_s` ignored when `enabled` is false; no Required column in the 17.6 and 17.8 result tables; `limit_reached` retry equal to `cooldown_remaining_ms`; power-on only after the inhibitor started; scratchpad ids `not_found` for `window.*`; audio `available:false` with a stale id; "character" in 17.13 (scalar values); `input.key` repeat failing part-way; the `input_text` per-minute budget error; `entry_id` format (no `.desktop`) and collation of `apps`; media order, active player with none played, artist join; `play_pause` and `CanPause`; the step of the topic scope in `state.subscribe`; DESIGN 11.6 volume clamp versus PROTOCOL `bad_request`; audit line member names and which refusals are audited; notice texts; `[keep_awake]` config bounds; DESIGN 11.4 variant shapes (`Layout { con_id, kind }`, `Floating { con_id, enabled }`); `devices deny … idle.inhibit` not ending a running keep-awake (17.8 rule 6 names only revoke; owner call); UX 16 `not_found` text outside windows; UX 10.2 "about 50" rounding; the launcher name in UX texts; (the Awake and Typing status pills of UX 4.1 are now done, see below).
- File size: `MainViewModel.kt` is about 1250 lines, `SwayConnectClient.kt` 522, `admin.rs` 541, CLI `args.rs` 521 and `run.rs` 504, over the about-500 guide.
- `cargo run` was run once by INT outside `just` (an example binary that was already built); not repeated.

### Review fixes after checkpoints M2a and M2b (2026-11-14)

Nineteen review findings, each checked first. All were real. 18 are fixed with a test or an emulator check (the two panic-switch findings share one change, item 5), and 1 is deferred. `just ci` green (921 Rust tests). Android gate green (`:protocol:test` 78, `:app:testDebugUnitTest` 287, 0 failures; lint 0 errors and the same 3 warnings).

Fixed, daemon:

1. Terminal emulators (`TerminalEmulator` in `Categories`: Alacritty, foot, footclient, kitty) are risky. Before this, a phone with the default pairing could start them without `launch.risky`. Index test and `prepare_launch` test added; PROTOCOL 17.11 and DESIGN 11.6 updated.
2. The tier 2 connect notice of DESIGN 11.8 is wired. It shows after each authenticated connect of a device that holds a tier 2 scope. It does not show for a pending pairing, or while the daemon is read-only, since then no tier 2 op is offered. Test expectations updated in `v2_m2a.rs`, `v2_m2b.rs` and `e2e_controls.rs`. With the default pairing every phone reconnect showed a notice (9 in about 12 minutes on the emulator). Owner answer D2 (2026-11-14): at most once per device per 12 h, in memory per daemon run (DESIGN 11.8; done in M2c G-FIX).
3. Virtual keyboard: a part carries the time of its check. The keyboard thread refuses it when the caller has gone or the check is older than 250 ms, so a part queued behind a stuck delivery is never written late (DESIGN 11.7 item 4).
4. The launcher check reads the child's exit through an `ExitProbe` of the child handle, not the topic view. A launcher that just ended counts as closed at once.
5. Panic switch: `Launcher::close` and the new `KeepAwake::close` refuse every later start (`read_only`), so an `open` or `set` that passed the read-only check before the switch starts nothing after it. `app.launch` reads the flag again just before its start. The two closes run side by side (`tokio::join!`). Tests: `after_close_no_launcher_opens`, `after_the_panic_switch_a_queued_or_later_on_is_read_only`, and the daemon test that now also checks the refusals. Not unit-tested: the `app.launch` re-check after the workspace switch, because the session harness has no sway that answers.
6. `DBUS_SYSTEM_BUS_ADDRESS` is written into the DESIGN 11.6 env list with its reason. The list covers every child.
7. Keep-awake waits 500 ms (`START_CHECK`) for the new inhibitor. If it ends in that time, the answer is `backend_unavailable`, no notice shows, and a replace keeps the old inhibitor and its expiry (PROTOCOL 17.8 errors). FakeRunner test for a first start and for a replace.
8. Media: a failed bus read publishes no players but keeps every id and mark. A player whose own read failed stays known through the name list (`Players::unread`). Tracker and service tests added.
9. Audio watcher: a test now covers the backoff reset after a healthy run of `pactl subscribe`.

Fixed, Android (emulator, API 37, separate user, read-only daemon on the live sway at `127.0.0.1:28521` and a fake-sway daemon at `28522`; screenshots `*-fixed*.png` in `~/.cache/swayconnect-m2/`):

10. Keep-awake duration buttons keep their unit ("15 min"). The padding is 8dp, labels never wrap, and the row stacks when the widest label does not fit. Seen at font scale 1.0 and 1.3, and at 360 dp width.
11. Status line: `Locked` (padlock) after "Sway not running" and before "Screens off", inside the polite live region. The `Awake N min` pill (tap opens the duration sheet) and the `Typing` pill both show. Unit tests added; both pills seen.
12. A read-only app grid dims its cells to 0.38 (`AppCell.looksDisabled`, tested). A stale grid keeps its own 0.6.
13. The search pill has no underline.
14. The refusal line moved above the cards in Audio. In every area it is a polite live region (shared `StateLine`). UX 15 now names it as the one other announcement.
15. Keys: the key rows stay below a scrolling part (`weight(1f, fill = false)`), so they sit directly above the IME. `uiautomator` at 360x780 dp with the keyboard up, font scale 1.0 and 1.3, also with the Sent line and a refusal line shown: the Copy row ended at 1242 px with the IME top at 1263 px.
16. Font scale: More tiles take one column above 1.2. At 1.3, "Scratchpad" broke in two columns. The Audio card stacks its pill under the name above 1.5, and its state line may wrap. The Launch grid cell grows with the font scale ("Alacritty" broke mid-word at 2.0). Audio, More and Launch were seen at 2.0.
17. Typing: after `rate_limited`, a part waits `detail.retry_after_ms` once (at most 60 s) and is sent again under the same spinner. View-model test with 5 parts.

Deferred:

18. "Pin to top" (UX 8.2) is not in M2. UX 8.2 now says so.

Environment: the owner's daemon (pid 243449) and the windowed emulator were gone when this work resumed (uptime 2 h, likely a reset). As AGENTS.md 2 allows, the AVD was started headless, used, and stopped again. The test user is removed, and font scale and display size are back to their defaults. Installing for the test user also replaced the APK of user 0 with this debug build; its data stays.

## Status: checkpoint M1 passed by INT on 2026-11-13; owner gate G-M1 open

Every M1 slice of section 3 is written. INT checked the whole tree on 2026-11-13:

| Check | Result |
|---|---|
| `just ci` | green: fmt-check, clippy `-D warnings`, 546 Rust tests, `cargo deny` (warnings only, among them the duplicate `miniz_oxide` from resvg) |
| Android gate | green: `:protocol:test` 55 tests, `:app:testDebugUnitTest` 136 tests, 0 failures; lint 0 errors, 3 warnings (`OldTargetApi`, `MissingApplicationIcon`, `UseKtx`). `MainViewModelTest` ran 5 more times, all green. |
| Emulator, live sway | API 37 emulator, a separate Android user, paired by link with a `read_only` daemon on the live sway (temp XDG, `127.0.0.1:28511`). Seen: the owner's 4 real workspaces as cards; map and list; real app icons through `icon.get` (Alacritty, Chrome, Brave, 1Password, Telegram, Firefox, Vesktop) and from the disk cache after a restart; the 7-window workspace as a map; pager swipe and card-row swipe in Browse (the daemon is read-only, so Drive is off and "Show on computer" is disabled); window sheet; Computer sheet; Audio and More placeholders; light and dark; font scale 1.0 and 2.0; portrait and landscape; live focus changes from the owner's keyboard; offline with "last seen" and Retry; cold start with the cached snapshot; `devices deny`/`grant state.titles` ending the session with `scopes_changed` and the app showing app ids, then titles again; revoke ending in "Not paired". Pair step 4 ("What this phone can do") was reached against a real daemon. |
| Emulator, fake sway | A second daemon (`read_only = false`, `127.0.0.1:28512`) on the testkit `fake_sway` example. The fake recorded exactly 5 commands, in this order: Drive pager swipe → `workspace --no-auto-back-and-forth number 2`; Drive card tap → the same; window card tap → `[con_id=7] focus`; Full → `[con_id=6] fullscreen enable`; Browse "Show on computer" → `workspace --no-auto-back-and-forth number 2`. A Browse swipe, a Drive swipe back to the focused workspace and an app restart sent nothing. |

Screenshots: `~/.cache/swayconnect-m1/` (29 files). The pair screen shows black there: `FLAG_SECURE` works.

INT fixed four integration defects:

1. `MainViewModel`: the icon cache was never run or attached, so only monograms showed. It now runs for the view model's life and fetches through the live session when `icon.get` is in `ops`.
2. `MainViewModel.pair()`: `PairPhase.Confirmed` was published before the new server, so the test "a confirmed pairing shows what the phone can do until done" failed about 2 runs in 3. Both now change in one state update.
3. `AppIcon`: the monogram letter inherited the body line height and dropped below small discs (A5 note). Its line height now equals its font size.
4. `WorkspacePage`: the "Layout too large" line sat inside the chip `FlowRow` and was drawn beside "On screen". It is now its own line.

Not checked, or found and left open:

- TalkBack as a running service, reduce motion, haptics on API 29, a physical phone, the 40-tile frame rate (A5 measured p90 48 ms).
- Tabbed, stacked, floating and fullscreen maps on live data: the live desktop had none. They are covered by JVM tests and fixtures only.
- Landscape: the title row, status line and the separate compact card row take about half the height, so the map starts below the fold (UX 5.3 wants the cards inside the title row). Still open after the review fixes below: deferred, see there.
- The testkit `fake_sway` example serves the v1 fixtures without rects, so Drive was checked in list view; the map against the fake sway was not seen.
- View mode, Drive choice and theme are in memory only; no per-profile settings store has an owner yet (A3 note).
- Spec gaps the slices reported (nesting depth limit, `icon.get` hash typing, 17.3 missing row, Output.model join rule, `layouts_cut` in the 17.18 example, UX 13 `MotionScheme` being internal, light `onError`) are not yet written into `PROTOCOL.md` or `UX.md`.

## Review fixes after checkpoint M1 (2026-11-13)

A design, accessibility and code review of the checkpoint raised 33 findings. One writer fixed them; `just ci` green (549 Rust tests), Android gate green (`:protocol:test` 55, `:app:testDebugUnitTest` 149, 0 failures; lint 0 errors, the same 3 warnings). Emulator re-check against a `read_only` daemon on the live sway (temp XDG under `/tmp/claude-1000/sc.TLr1`, `127.0.0.1:28511`, a separate Android user, removed after). Screenshots with a `-fixed` suffix in `~/.cache/swayconnect-m1/`.

Fixed:

1. Card row starts on the page gutter when the cards fit (widths measured from their texts) and does not scroll then; the centring padding only when they overflow (UX 5.4).
2. Title and status line stack tight; "On screen" moved into the page header; the page's state lines sit 4dp under the header (UX 4.1, 5.6). The map now starts at y 742 of 2400 instead of 1047 (screenshot 01).
3. `#N` badge shows whenever its measured text plus 12dp fits (the 70dp tiles of the 7-Alacritty workspace now read #2, #3); at a large font scale the index follows the title fragment instead of overlapping it (UX 6.3).
4. List view: the focused window's buttons go into its own card; no second copy follows (UX 5.1, 5.7).
5. Large size class needs room for 8 measured characters of `labelLarge`; at font scale 2.0 narrow tiles show the centred icon, not "Alacri…" (UX 6.2).
6. Titles: runs of U+FFFD at the ends dropped, inner runs one space (client display rule, option b; the wire rule of PROTOCOL 2.4 is unchanged). Telegram titles show clean (screenshot 05).
7. Read-only or missing `control`: the context pill shows Browse with a 12dp lock at full contrast; a tap opens the Computer sheet (read-only) or the Locked sheet (scope). The pill is one TalkBack node.
8. Workspace card: one TalkBack node with `Role.Tab`, `selected`, the description and the "Show on computer" action (checked with `uiautomator dump`; no Compose UI test library, so no automated semantics test).
9. Wrong certificate: `SessionOutcome.WrongKey`, no retry, status "Wrong computer key" (triangle), full-screen "This is not the computer you paired with." with Pair again. Revoked is full screen too and keeps the computer's name.
10. Refusals follow UX 16 (`Refusal.of`, unit tested); `forbidden` opens the Locked sheet for a plain scope name; never a raw wire code. The strings stay in Kotlin next to the view model's other notices (it has no `Context`), not in `strings_desktop.xml`.
11. Daemon icon key: built from the whole window node (`app_id`, Xwayland class and instance, `sandbox_app_id`) and the pid for the flatpak cgroup hint (`IconKey::of_window`). `swayconnect-sway` parses `window_properties.instance` and `sandbox_app_id`. Flatpak Brave, Telegram and Vesktop got their icons on the live check.
12. Minor: one gutter for title, cards, banner and pages; slivers are plain wash bars, and the dense chip needs 3 slivers as well as 30 %; map monograms sit on a `surface` disc; Audio, Launch and Keys show a dashed "later" card, More a Computer card; "Drive paused" really pauses drive; a drag that springs back to its start page sends nothing (card taps still do); TalkBack order title → status → pill → button; chips and tonal buttons have a 1dp `outline` edge (pairs already in `ThemeContrastTest`); the pane title is frozen per settle; "Larger text · Show list" above font scale 1.8; Locked sheet prints `swayconnectd devices grant <4-char prefix> <scope>`; icon files open with `O_NONBLOCK` and are type-checked on the handle (FIFO test); the icon key map is capped at 1024 and the job queue at 64; a `null` layout without `layouts_cut` reads "No layout for this workspace" (PROTOCOL 17.4.3 row updated); `AppIcon` recomposes per hash and keeps a shown bitmap; the map's shared tap path reads current callbacks.

Manifest: `rustix` gained the `fs` feature (root `Cargo.toml`) for the non-blocking open.

Deferred or not verified:

- Landscape single title row of UX 5.3 (title, status, compact cards and context pill in one 56dp row): needs the pager state hoisted into the shell. Owner of the work: A2 with A4, M2a.
- Owner question: the page line "The computer is set to look only." repeats the status word "Read-only" on every page. It stays (UX 5.9 asks for it) until the owner says it can go.
- No D-INT test in `tests/v2_m1.rs` for an instance-only or flatpak-only window through FakeSway; unit tests in `icons/service_tests.rs` cover key building, instance matching and the cgroup hint.
- TalkBack was still not run as a service: announcements (pane title once per settle) and the traversal order are checked in the accessibility tree only.
- M1 Settings (Theme, Default view, Reduce motion, Haptics) have no store; they move to A11 in M2a (UX 10 heading corrected).

## 0. Questions for the owner

**Answered by the owner on 2026-11-14 (final):**

- **Q1: no step-up in v2.** Step-up stays in `DESIGN.md` 11.13.
- **Q2a and Q2b: approved.** The amended rules 2, 8, 9 (with the `input.text` exception), 10 and 11 of `DESIGN.md` 11.3 hold; `AGENTS.md` section 5 carries them. M2a and M2b are both unblocked.
- **Typing "on at pairing" (owner addition).** `swayconnectd pair` now gives by default: `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys`, `input.text`. Still off at pairing and granted only by name: `destructive`, `audio.capture`, `launch.risky`. Every other safeguard of `DESIGN.md` 11 stays: lock refusal, focus check per part, desktop notice when a remote keyboard session starts, audit, revoke, panic switch. Only new pairings get the new default; `devices.json` entries keep their scopes. Who does it: D-WIRE adds each milestone's scopes to the `pair` default in its step .2a (`idle.inhibit`) and .2b (`audio`, `media`, `launch`, `input.keys`, `input.text`), so the default never names a scope the daemon does not know yet. D-INT.2a and D-INT.2b test that a default pairing holds exactly that set and none of the three by-name scopes. The risk is in `DESIGN.md` D30.
- **Q3: deferrals kept** (section 6 item 9).

The questions as they were asked:

1. **Biometric step-up before remote typing ships?** With it, `input.*` (and optionally `launch.risky`) need a user check on the phone, verified by the daemon (a second Keystore key that signs a server challenge, an enrol flow confirmed at the desktop, about two more slices). API facts: on API 29, per-use auth needs a strong biometric; a time-bound key (for example 30 s) accepts PIN, pattern or password through `BiometricPrompt.Builder.setDeviceCredentialAllowed(true)`, a system dialog. On API 30+: `setUserAuthenticationParameters(30, AUTH_BIOMETRIC_STRONG or AUTH_DEVICE_CREDENTIAL)`. So the time-bound ECDSA step-up key works on `minSdk` 29 without fingerprint hardware. Without step-up, typing relies on: off by default, granted only at the desktop, lock refusal, desktop notice, audit, revoke, panic switch. **Default if unanswered: no step-up in v2.**
2. **Approve the amended security rules of `docs/DESIGN.md` 11.3.** `AGENTS.md` section 5 calls these rules "must never be broken", so no agent edits them without your yes. Two parts, asked separately:
   - **2a (gates M2a):** rule 2 (closed-enum sway commands and the fixed `output * power on`), rule 8 (child processes by argv, every program path from config, `--` before names), rule 9 first sentence (client strings only as server-issued ids or closed enums), rule 11 (fixed-text notices).
   - **2b (gates M2b):** the `input.text` exception of rule 9 and rule 10 (fail-closed input).
   **M1 needs neither.**
3. **Deferrals (section 6 item 9).** This plan cuts owner buttons, per-app streams, card profiles (switching to the monitor's HDMI speakers), media seek, Live typing, presets and Keystore key hardening out of v2 to keep it small. **Default if unanswered: deferred.** Say which one you want back.

Everything else is decided (section 6).

## 1. Rules for every slice

### 1.1 Hard rules (unchanged from stage 1, extended)

1. Never send a state-changing command to the live sway. Read-only `swaymsg -t get_*` only.
2. Never run a state-changing desktop tool: no `pactl set-*`, `wpctl set-*`, `playerctl` control, `systemd-inhibit`, `systemd-run`, `gtk-launch`, `fuzzel`, `notify-send`, `wl-copy`, `loginctl`, `swaylock`, `lock.sh`, and no Wayland client that creates a virtual keyboard. Read-only queries (`pactl -f json list`, `pactl subscribe`, `wpctl status`, `playerctl -l`, `busctl get-property`, `systemd-inhibit --list`) are allowed.
3. A daemon an agent runs against the real desktop has `read_only = true`, temp XDG directories, a loopback address and a port other than 28414. Read-only mode refuses every action, including the new ones (keep awake, audio, launch, input).
4. **The owner runs the first real action of every new control himself** (gates of section 5). Agents test against fakes only. Every test config points every program path (11.9) at the testkit's `fake-proc`.
5. No `git commit`, no `git push`, no `sudo`, no package install. Never edit the owner's sway config, swayidle setup, waybar, firewall or systemd units.
6. A slice edits only the paths it owns (section 4). A needed change elsewhere goes to that path's owner.
7. A wire change updates `docs/PROTOCOL.md`, `protocol/vectors/`, `swayconnect-protocol` and the Kotlin `:protocol` module together (`AGENTS.md` section 4). Slices C, P-RS and P-KT land as one change per step.
8. Every UI piece is Compose (D34). No XML layout, no View widget, no WebView, no vector drawable.

### 1.2 Machine safety (the PC hard-resets under heavy load)

| # | Rule | Mechanism |
|---|---|---|
| 1 | One heavy process at a time: cargo, Gradle and the emulator boot never overlap. | Every `just` recipe takes `flock -o target/.agent.lock`. Gradle takes the same lock by hand: `flock -o ../../target/.agent.lock ./gradlew <tasks> --max-workers=4`, then `./gradlew --stop`. |
| 2 | Cargo only through `just`. 4 jobs, 4 test threads. | `.cargo/config.toml`. No `CARGO_TARGET_DIR`. |
| 3 | Gradle with `--max-workers=4`. No change to `org.gradle.jvmargs`. | command line |
| 4 | One headless emulator, 2 cores, 3 GiB: `-no-window -no-audio -no-boot-anim -no-snapshot-save -cores 2 -memory 3072`. No build while it boots. `adb emu kill` at the end. | `AGENTS.md` section 2 |
| 5 | Heavy new crates (resvg, zbus, wayland-client) are added by MAN alone, one milestone at a time, with a `just check` of the touched crate before anything else builds. | slice MAN |
| 6 | No background process left running. | review |

### 1.3 Done for a slice

- Rust: `just fmt-check`, `just lint <crate>`, `just test <crate>`. Files under about 500 lines. `#![forbid(unsafe_code)]`.
- Android: `flock -o ../../target/.agent.lock ./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug --max-workers=4`, 0 lint errors. A screen change is looked at on the emulator (light and dark, font scale 1.0 and 2.0, TalkBack order once per screen).
- The integrator (INT) runs `just ci` and the Android gate at each checkpoint.

## 2. Order

```
M1 "UI/UX"
  C1 ─┬─ P-RS.1 ─┬─ SW.1 ─┬─ D-LAYOUT ─┐
      │          │  TK.1 ─┤            │
      │          │        ├─ D-POLL ───┤
      │          │ MAN.1 ─┴─ D-ICON ───┼─ D-WIRE.1 ─ D-INT.1 ─ CLI.1 ─┐
      │          └─ D-SEC.1 ───────────┘                               │
      └─ P-KT.1 ─┴─ A1 theme ─ A2 shell ─ A3 state ─┬─ A4 desktop ─────┤
                                    A6 common/icons ┤                  │
                                    A7 pair ────────┘  A5 map ─────────┤
                                                                       ▼
                         checkpoint M1 (INT): ci + emulator against a read-only daemon on the live sway
                         owner gate G-M1 (look and feel)
M2a "desktop controls"  (after question 2a)
  C2a ─ P-RS.2a ─ P-KT.2a ─ SW.2 ─ TK.2 ─ D-PROC ─┬─ D-KA ──────┬─ D-WIRE.2a ─ D-INT.2a ─ CLI.2a
                                                  └─ D-SEC.2 ───┘
  A3.2a ─┬─ A11 more (keep awake first, lock, scratchpad sheet)
         └─ A4.2 desktop actions, A6.2 hold button
  checkpoint M2a (INT), then owner gates G-WIN, G-KA, G-LOCK
M2b "devices and typing"  (after question 2b)
  C2b ─ P-RS.2b ─ P-KT.2b ─ MAN.2
    ├─ D-AUDIO ─┐
    ├─ D-MEDIA ─┤
    ├─ D-APPS ──┼─ D-INPUT ─ D-WIRE.2b ─ D-INT.2b ─ CLI.2b
  A3.2b ─ A8 audio ─ A9 launch ─ A10 keys ─ A2.2 capsule ─ QA2
  checkpoint M2b (INT), then owner gates G-AUDIO, G-MEDIA, G-LAUNCH, G-INPUT
```

Slices on the same line after a split can be written in parallel. Their builds still queue on the lock.

## 3. Slices

Format: **owns** (exclusive paths), **needs** (slices that must be done), **acceptance**.
A slice with steps `.1`, `.2`, `.2a`, `.2b` is one slice that owns its paths in every milestone.

### 3.1 Shared slices

**INT — integrator and docs.**
Owns: `AGENTS.md`, `docs/**`, `README.md`, `config.example.toml`.
Does: checkpoints, status sections, `AGENTS.md` section 5 text after question 2a and again after 2b, `config.example.toml` keys from 11.9.
Acceptance: `just ci` green and the Android gate green at each checkpoint; docs say what was and was not checked.

**MAN — manifests.**
Owns: every `Cargo.toml`, `Cargo.lock`, `deny.toml`, `.cargo/config.toml`, `justfile`, `clients/android/gradle/libs.versions.toml`, `clients/android/*/build.gradle.kts`.
Step .1: add resvg 0.50.0 (`default-features = false`), tiny-skia 0.12.0 (`png-format`; resvg also turns on its defaults `std` and `simd`), rustix 1.1.6 (`process`) to the daemon; `BSD-2-Clause` to `deny.toml`. (`fake-proc` needs no manifest entry: Cargo finds `src/bin/fake-proc.rs`.)
Step .2 (M2b): add `zbus = { version = "6.3.0", default-features = false, features = ["tokio", "proxy"] }`, wayland-client 0.31.15, wayland-protocols-misc 0.3.12 (`client`), rustix feature `fs`, xkeysym 0.2.1.
Acceptance: `just check swayconnect-daemon` then `just deny` pass after each step. No Android dependency added.

**C1 / C2a / C2b — contract vectors.**
C1 owns `protocol/vectors/v1/ext/desktop.json`, `ext/icons.json`, `ext/welcome.json`, and `protocol/vectors/v1/README.md`. `icons.json` includes a result with 8 icons of 65536 bytes (fits `max_frame_s2c`) and a 9-hash request (`bad_request`). `welcome.json` has `known_scopes` and `known_ops` with the `FeatureAvailability` table of 17.3.
C2a owns `ext/session.json` and `ext/requests.json` (window ops, `session.lock` with `already_locked`, `keep_awake.set`).
C2b owns `ext/audio.json`, `ext/media.json`, `ext/apps.json`, `ext/input.json`. `media.json` has an `album` with U+202E and ESC (server output must have U+FFFD). `apps.json` has an oversize state and its cut result with `truncated`. `input.json` has a ZWJ emoji text (allowed), a tag-character text (refused), `ctrl`+`alt`+`F2` (refused), and the `launcher_closed` case.
Needs: nothing (C2a: question 2a; C2b: question 2b).
Acceptance: every example of `PROTOCOL.md` 17.18 is a vector; request cases (bad ranges, unknown enum, `null` where not allowed, client string not issued by the server) with expected errors; `PROTOCOL.md` 17 corrected where a vector found a gap (sent to INT). The v1 vectors and transcripts stay byte-identical.

**P-RS — Rust protocol crate.**
Owns: `crates/swayconnect-protocol/**`.
Step .1: `Rect`, `Output`, `ScratchpadWindow`, `Layout`, `Tile`, `Window.icon`, `Workspace.rect/layout`, `known_scopes`, `known_ops`, op `icon.get`, `bye scopes_changed`, `sint` checks.
Step .2a: scopes `idle.inhibit`, `destructive`; window and session ops; topic `session`; `limit_reached`, `backend_unavailable`, `spawn_failed`; rate classes.
Step .2b: scopes, topics and ops of 17.9 to 17.13; input errors; the `input.text` character rule with the U+200C/U+200D exception; the `ctrl`+`alt`+Fn refusal.
Needs: the C step of the same milestone.
Acceptance: vectors decode and re-encode; unknown enum values map to the fallbacks; old v1 vectors still pass unchanged.

**P-KT — Kotlin `:protocol` module.**
Owns: `clients/android/protocol/**`.
Steps as P-RS. `SwayConnectClient` gains `iconGet`, the new ops and the topic flows.
Needs: the C step of the same milestone.
Acceptance: `:protocol:test` replays the same vectors; old tests unchanged.

**SW — sway crate.**
Owns: `crates/swayconnect-sway/**`.
Step .1: `Node` fields of 11.4 (`rect`, `layout`, `focus`, `scratchpad_state`, `inhibit_idle`, output `power`/`scale`/`transform`/`make`/`model`), `binding` subscription.
Step .2 (M2a): command variants of 11.4 and the new pattern test.
Needs: P-RS.1 (types only).
Acceptance: the frame writer still builds only types 0, 1, 2, 4, 7; every command renders to the 11.4 pattern; a live tree fixture (titles removed) parses.

**TK — testkit.**
Owns: `crates/swayconnect-testkit/**`.
Step .1: FakeSway serves tree fixtures with rects, outputs, tabbed and floating cases, `binding` events; a temp `XDG_DATA_DIRS` with one `.desktop` file and one small PNG; the child-env helper sets `DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent` and that `XDG_DATA_DIRS`.
Step .2 (M2a): records and replies to the new commands; can change focus in the tree without sending an event; `fake-proc` binary, copied into a temp dir under each program's name (it appends its argv to `<own path>.log`, then sleeps or exits as `<own path>.mode` says; it needs no environment) and a config builder that points every program key at it.
Acceptance: used by D-INT and CLI; a guard test fails when a test config names a program under `/usr` (step .2, with the config builder).

**D-WIRE — daemon wiring.**
Owns: `crates/swayconnect-daemon/src/{dispatch,session,daemon,main,config,throttle,lib,security,pairing,actions,paths,identity,listener}.rs`.
Step .1: `icon.get` with the `icons` rate class, `known_scopes`, `known_ops`, subscriber count to the supervisor, `daemon::IconSources` (search dirs from `XDG_DATA_HOME`/`XDG_DATA_DIRS`, cache dir, raster program) built in `main.rs` and passed to `Daemon::start_with_icons`, the `--raster` entry.
Step .2a: window and session ops, `session.lock` rules, `keep_awake.set`, per-device `session` topic, scope table and rate classes for M2a, SIGUSR1 ending keep-awake.
Step .2b: ops of 17.9 to 17.13, extra scope checks 8a/8b, `WaylandTarget` and the session bus address in `main.rs`, SIGUSR1 aborting input and the launcher.
Each step embeds the config section types the feature slices define; it does not write their fields.
Needs: the feature slices of its step.
Acceptance: op table test (op → scope → rate class); read-only refuses every new action and still serves `icon.get`; unknown config key still exits 2.

**D-SEC — scopes, grants, audit.**
Owns: `crates/swayconnect-daemon/src/{devices,admin,audit}.rs`.
Step .1 (M1): `devices grant/deny` by id prefix (≥ 4 chars, clash refused); unknown scope in `devices.json` exits 2; `scopes_changed` sent to live sessions (hook called from D-WIRE.1).
Step .2 (M2a): audit lines carry lengths only; `swayconnectd audit` output escaped. No rotation.
Acceptance: as listed, through integration tests in D-INT.

**CLI — client binary.**
Owns: `crates/swayconnect-cli/**`.
Step .1: `windows --json` prints the new fields; `layout <workspace>` draws an ASCII map; `icon <hash> -o FILE`.
Step .2a: `awake on [--for 1h]|off`, `lock`, `move`, `float`, `close`.
Step .2b: `volume`, `mute`, `play`, `launch <entry>`, `type` (text from stdin only, never argv), `key <name>`.
Acceptance: end-to-end tests against a daemon child, the fake sway and `fake-proc` for every program; `type` refuses text in arguments; no test starts a real system program.

### 3.2 M1 daemon slices

**D-LAYOUT — layout export.**
Owns: `crates/swayconnect-daemon/src/{layout,outputs,model}.rs`, `crates/swayconnect-daemon/tests/fixtures/layout/**`, `tests/fixtures/desktop_expected.json`, `tests/fixtures/README.md`.
Needs: SW.1, P-RS.1.
Acceptance: fixtures from the live tree give the 17.18 example; one-child collapse; depth cap 16; tabbed active child from `focus`; floating order; fullscreen from leaf only; workspace `fullscreen_mode` ignored; cut rule drops layouts before windows and sets `layouts_cut`; a 512-window synthetic tree stays under 262144 bytes; `desktop_expected.json` updated for the new fields.

**D-POLL — supervisor freshness.**
Owns: `crates/swayconnect-daemon/src/{supervisor,sway_handle}.rs`.
Needs: SW.1.
Does: 2 s poll while subscribers exist, `binding` events, publish only on change, count of windows with `inhibit_idle`.
Acceptance: paused-time tests: no poll with zero subscribers; one publish per real change; no publish when the tree is equal.

**D-ICON — icon service.**
Owns: `crates/swayconnect-daemon/src/icons/**`, `crates/swayconnect-daemon/tests/icons.rs`.
Needs: MAN.1, P-RS.1.
Acceptance: id matching order of `V2_RESEARCH.md` 3; Inherits, hicolor, pixmaps; absolute paths outside the roots refused after canonicalising; 1 MiB source cap; PNG ≤ 128 px and ≤ 65536 bytes; hash = first 16 bytes of SHA-256; rasterising in the `--raster` child with `RLIMIT_AS` 256 MiB, `RLIMIT_CPU` 2 s, 3 s kill; IHDR > 1024 px, > 10000 SVG nodes and SVG filters refused before rendering; a blur-bomb SVG, a `<use>` fan-out SVG and a huge-IHDR PNG each end within 3 s while another icon is still served; negative cache 30 s. No test reads the real `/usr/share`.

**D-INT.1 — integration tests M1.**
Owns: `crates/swayconnect-daemon/tests/v2_m1.rs`, `crates/swayconnect-daemon/tests/transcripts.rs`.
Acceptance: subscribe → layout present; resize via fixture swap → new state within one poll; `icon.get` with a known hash (from the TK temp data dir) and an unknown hash; `icon.get` allowed in read-only mode and without `control`, rate limited by its class; `devices grant` → `bye scopes_changed` → reconnect sees the new `ops`. `transcripts.rs` compares v1 members only: it strips members that v1 does not define from server frames and checks `welcome.ops` and `topics` as supersets; the v1 transcript files stay byte-identical.

### 3.3 M1 Android slices

Each Android slice owns its own string file: `res/values/strings_<area>.xml` (`shell`, `desktop`, `map`, `common`, `pair`, `audio`, `launch`, `keys`, `more`). `res/values/strings.xml` keeps the app name and shared words (A2).

**A1 — theme and icons.**
Owns: `ui/theme/**` (incl. `SwayIcons.kt`), `res/values/themes.xml`, `res/values-night/themes.xml`.
Acceptance: tokens of `UX.md` 3; contrast pairs re-computed in a JVM test from the token values (≥ 4.5 text, ≥ 3 marks), including `accentOnInk` on `primary` in both themes, `error` on white and greige, the urgent dot, dark `outline` on card and background; `SwayMotion`; haptic fallbacks on API 29 recorded (UX 14); about 30 `ImageVector` glyphs in outline and filled pairs. No new file in `res/drawable`.

**A2 — shell.**
Owns: `ui/shell/**`, `MainActivity.kt`, `res/values/strings.xml`, `res/values/strings_shell.xml`, `AndroidManifest.xml`.
Step .1: title row, status line with dot shapes and its live-region split, context pill and round button slots, floating pill nav (horizontal and vertical) with measured label drop, area switching, back rules with `android:enableOnBackInvokedCallback="true"`.
Step .2 (M2b): now-playing capsule; volume keys in `dispatchKeyEvent` while Audio is shown.
Needs: A1, A3.
Acceptance: nav animation per UX 4.2; reduce motion snaps; TalkBack reads the nav as tabs; font scale 1.3 at 360dp and 2.0 keep 48dp targets and no overflow; status announcements only on transitions.

**A3 — state.**
Owns: `ui/state/**`, `ui/MainViewModel.kt`, `ui/UiState.kt` (moved into `ui/state/`), `AppContainer.kt` (incl. `IconCache` wiring), `data/DesktopCache.kt`, `data/ProfileStore.kt`.
Step .1: `Area`, `Sheet`, `viewMode`, `drive`, desktop reducer with id re-sync, `DriveController` (user-initiated settles only), follow-focus guard, `FeatureAvailability` from `known_ops`, cached snapshot.
Step .2a: session reducer, keep-awake countdown.
Step .2b: audio, media, apps, keys reducers; slider throttle; pending launch.
Needs: P-KT.
Acceptance: JVM tests: re-sync after a dropped workspace; a workspace removed before the current page sends nothing; follow-focus scroll sends nothing; Drive sends one request after a user fling; no send when the settled id equals the focused id; `FeatureAvailability` for every row of `PROTOCOL.md` 17.3; cache without titles when `state.titles` is missing.

**A4 — desktop area.**
Owns: `ui/desktop/**`, `ui/DesktopScreen.kt` (deleted after the move), `res/values/strings_desktop.xml`.
Step .1: snapping workspace card row with colour fill, ring and marks; pager; Drive and Browse; follow-focus chip; page header; one `LazyColumn` per page for map and list; window cards; focused-window card; Window sheet (Focus, Fullscreen); states of UX 5.9; basic landscape.
Step .2a: Float, Move, Close buttons and sheet rows; window layout segmented row.
Step .2b: Type button; ghost tile for launches.
Needs: A2, A3, A6 (A5 for the map slot).
Acceptance: emulator against a read-only daemon on the live sway shows the owner's real workspaces; actions against the fake sway; ring follows the finger; fill changes only on confirmation; list view inside a page does not crash at font scale 2.0; TalkBack cannot move into a neighbour page.

**A5 — mini-map.**
Owns: `ui/map/**`, `res/values/strings_map.xml`.
Needs: A1, A6, P-KT.1.
Acceptance: `MapGeometry` JVM tests (the formula of UX 6.1 with exact values, size classes, hit test by containment then nearest edge within 24dp, a touch inside a thin tile next to a small tile, same-app indexes, dense ratio); every 24–48dp tile has its own semantics node; the live layout renders; tabbed, stacked, floating and fullscreen from fixtures; ≤ 25 tiles animate, more snap; 40 tiles hold 60 fps on the emulator profile (frame stats noted, not a hard gate on an emulator).

**A6 — common components and icon cache.**
Owns: `ui/common/**`, `data/IconCache.kt`, `res/values/strings_common.xml`.
Step .1: `AppIcon` with monogram, `StatePill`, `DashedCard`, `WindowCard`, icon batching (≤ 8 hashes, at most one request per 500 ms, waits on `rate_limited`) and hash check.
Step .2a: `HoldButton`, `LockedFeatureSheet` content.
Acceptance: hash mismatch dropped; > 256 px refused before decode; disk cap; monogram colour stable per app id.

**A7 — pair flow restyle.**
Owns: `ui/PairScreen.kt` (moved to `ui/pair/**`), `res/values/strings_pair.xml`.
Acceptance: UX 11 steps; `FLAG_SECURE` on; "What this phone can do" from `scopes` and `known_scopes`.

**Checkpoint M1 (INT).** `just ci`, Android gate, emulator run against a read-only daemon on the live sway (allowed: it only reads) and the fake sway for actions. Then **G-M1**: the owner looks at it.

### 3.4 M2a daemon slices

**D-PROC — process runner and desktop facts.**
Owns: `crates/swayconnect-daemon/src/{proc,notify,lockstate}.rs`.
Needs: TK.2.
Acceptance: `Runner` trait + `FakeRunner`; env allowlist; timeout and kill; output cap; notify texts fixed and markup-free, program path from `[notify]`; `/proc` scan for lock processes and grabbers of the own uid, unknown on error.

**D-KA — keep awake, lock and the session topic.**
Owns: `crates/swayconnect-daemon/src/{keep_awake,session_topic}.rs`.
Needs: D-PROC, SW.2 (`output * power on`). Not MAN.2.
Acceptance (paused time, FakeRunner): exact argv from `[keep_awake] inhibit` and `sleep`; replace on a second `on` keeps the continuous count; kill on `off`; expiry clears state; off/on toggling every 60 s still hits `limit_reached` at 4 h; only an off period ≥ `cooldown_s` resets the count; expiry shortened to the remaining budget; config check `cooldown_s` ≥ 60 and `max_continuous_s` ≥ `max_duration_s`; release on holder revoke, SIGUSR1, shutdown; not on disconnect; power-on command only when an active output has `power: false`; notice on each start; holder per device; 2 s lockstate poll only while `session` has subscribers; `session.lock` returns `already_locked` while locked or while its child runs, one child at a time.

**D-INT.2a — integration tests M2a.**
Owns: `crates/swayconnect-daemon/tests/v2_m2a.rs`.
Acceptance: scope denial per new op; rate classes; read-only refuses every new action; keep-awake survives a client disconnect; audit line per keep-awake change; every program is `fake-proc`.

### 3.5 M2b daemon slices

**D-AUDIO.**
Owns: `crates/swayconnect-daemon/src/audio/**`, `crates/swayconnect-daemon/tests/fixtures/pactl/**`.
Needs: D-PROC.
Acceptance: fixtures captured read-only on this machine parse; monitor sources hidden; nodes named `-…` dropped; every argv has `--` before the name (a node named `--server=x` stays a positional argument); ids stable across snapshots; clamp to `max_volume_pct`; capture rule (source writes other than mute-on need `audio.capture`); notice on remote unmute; caps 64/64; a vanished device is `not_found`; subscribe respawn with backoff.

**D-MEDIA.**
Owns: `crates/swayconnect-daemon/src/media/**`.
Needs: MAN.2.
Acceptance: `MediaBackend` trait + fake; `playerctld` skipped; titles withheld without `state.titles`; every string cleaned (an `album` with U+202E and ESC comes out with U+FFFD); cap 16 players. No test touches a real bus; the bus address comes from `EnvSnapshot`.

**D-APPS — apps and launcher.**
Owns: `crates/swayconnect-daemon/src/apps/**`, `crates/swayconnect-daemon/tests/fixtures/desktop-entries/**`.
Needs: D-PROC, D-ICON (entry index).
Acceptance: filters; Exec quoting cases from the spec; every field code removed; `systemd-run` argv exact from `[apps] systemd_run`; terminal wrap; risky entry refused without `launch.risky`; length caps and cut rule with `truncated`; one launcher instance with holder and the focused window recorded; no client string reaches argv except an exact index match.

**D-INPUT — remote keyboard.**
Owns: `crates/swayconnect-daemon/src/input/**`.
Needs: MAN.2, D-PROC (lockstate), D-APPS (launcher handle). Question 2b answered.
Acceptance: keymap generation for ASCII, Latin-1, Cyrillic, single-code-point emoji and ZWJ emoji sequences; never maps text onto Escape/BackSpace/Tab/Return/modifier codes; no key resolves to `XF86Switch_VT_*`, `XF86Ungrab` or `Terminate_Server` under any modifier; 16-character parts; refusal order locked → launcher_closed → grabbed → focus; a synchronous focus read before every part (the fake sway changes focus without an event: the second part is refused with the right `delivered_chars`); launcher target only for the holder and only while the child lives; release-all after every request and on disconnect; text never logged (log capture test). The Wayland thread is behind a `KeyboardSink` trait; tests use `FakeKeyboard` only.

**D-INT.2b — integration tests M2b.**
Owns: `crates/swayconnect-daemon/tests/v2_m2b.rs`.
Acceptance: scope denial per new op, including 8a and 8b; rate classes; read-only refuses every new action; input refused while the fake proc scan reports swaylock; every program is `fake-proc`.

### 3.6 M2 Android slices

**A11 — More area (M2a; keep awake first).**
Owns: `ui/more/**`, `res/values/strings_more.xml`.
Acceptance: keep-awake tile states of UX 10.2 from `session` fixtures, including locked; countdown semantics without per-second announcements; duration choice limited by `max_duration_s`, "1 h from now"; Locked sheet text per scope from 10.5; Computer sheet; Scratchpad sheet with Show; Lock tile with `already_locked`.

**A8 — Audio area (M2b).**
Owns: `ui/audio/**`, `res/values/strings_audio.xml`.
Acceptance: UX 7; slider throttle ≤ 10/s and final value; mic unmute locked without `audio.capture` and held with it; titles withheld state.

**A9 — Launch area (M2b).**
Owns: `ui/launch/**`, `res/values/strings_launch.xml`.
Acceptance: `FuzzyMatch` JVM tests; risky apps locked without `launch.risky`, held with it; return to Desktop with ghost tile; fuzzel button flow to Keys.

**A10 — Keys area (M2b).**
Owns: `ui/keys/**`, `res/values/strings_keys.xml`.
Acceptance: `TextSplit` JVM tests (1024-byte UTF-8 boundaries, stop at first error); Enter never in text; terminal hold; sticky modifiers; refusals of UX 9.2 including `launcher_closed`; IME flags set through `InterceptPlatformTextInput` in every mode; `FLAG_SECURE` on; Private text dropped on rotation.

**QA2 — two-tap audit.**
Owns: no source files; writes its report to INT.
Acceptance: every row of `UX.md` 18 done on the emulator against the fake sway and fake backends.

**Checkpoints M2a and M2b (INT)**, then the owner gates.

## 4. Ownership index

No path appears twice. Paths under `clients/android/app/src/main/java/com/swayconnect/android/` are shortened.

| Path | Slice |
|---|---|
| `AGENTS.md`, `docs/**`, `README.md`, `config.example.toml` | INT |
| `**/Cargo.toml`, `Cargo.lock`, `deny.toml`, `.cargo/config.toml`, `justfile`, Gradle catalog and build files | MAN |
| `protocol/vectors/v1/README.md`, `ext/desktop.json`, `ext/icons.json`, `ext/welcome.json` | C1 |
| `protocol/vectors/v1/ext/{session,requests}.json` | C2a |
| `protocol/vectors/v1/ext/{audio,media,apps,input}.json` | C2b |
| `protocol/vectors/v1/*.json`, `protocol/vectors/v1/transcripts/**` | nobody: stay byte-identical |
| `crates/swayconnect-protocol/**` | P-RS |
| `crates/swayconnect-sway/**` | SW |
| `crates/swayconnect-testkit/**` (incl. `fake-proc`) | TK |
| `crates/swayconnect-cli/**` | CLI |
| daemon `dispatch, session, daemon, main, config, throttle, lib, security, pairing, actions, paths, identity, listener` | D-WIRE |
| daemon `devices, admin, audit` | D-SEC |
| daemon `layout, outputs, model`, `tests/fixtures/layout/**`, `tests/fixtures/desktop_expected.json`, `tests/fixtures/README.md` | D-LAYOUT |
| daemon `supervisor, sway_handle` | D-POLL |
| daemon `icons/**`, `tests/icons.rs` | D-ICON |
| daemon `tests/v2_m1.rs`, `tests/transcripts.rs` | D-INT.1 |
| daemon `proc, notify, lockstate` | D-PROC |
| daemon `keep_awake, session_topic` | D-KA |
| daemon `tests/v2_m2a.rs` | D-INT.2a |
| daemon `audio/**`, `tests/fixtures/pactl/**` | D-AUDIO |
| daemon `media/**` | D-MEDIA |
| daemon `apps/**`, `tests/fixtures/desktop-entries/**` | D-APPS |
| daemon `input/**` | D-INPUT |
| daemon `tests/v2_m2b.rs` | D-INT.2b |
| `clients/android/protocol/**` | P-KT |
| `ui/theme/**`, `res/values*/themes.xml` | A1 |
| `ui/shell/**`, `MainActivity.kt`, `AndroidManifest.xml`, `res/values/strings.xml`, `strings_shell.xml` | A2 |
| `ui/state/**`, `ui/MainViewModel.kt`, `ui/UiState.kt`, `AppContainer.kt`, `data/DesktopCache.kt`, `data/ProfileStore.kt` | A3 |
| `ui/desktop/**`, `ui/DesktopScreen.kt`, `strings_desktop.xml` | A4 |
| `ui/map/**`, `strings_map.xml` | A5 |
| `ui/common/**`, `data/IconCache.kt`, `strings_common.xml` | A6 |
| `ui/PairScreen.kt`, `ui/pair/**`, `strings_pair.xml` | A7 |
| `ui/audio/**`, `strings_audio.xml` | A8 |
| `ui/launch/**`, `strings_launch.xml` | A9 |
| `ui/keys/**`, `strings_keys.xml` | A10 |
| `ui/more/**`, `strings_more.xml` | A11 |
| `net/**`, `data/{KeystoreSecretsStore,SecretsStore,AesGcmCipher}.kt`, `res/drawable/**` | no change planned |

Each slice also owns the test files that mirror its source paths (`src/test/...` and `tests/...` under the same names).

## 5. Owner gates (agents never run these)

What M2c proved in a sealed session (`docs/M2C_PLAN.md`) is not repeated here: the gates keep only what the live desktop shows (speakers, wob, real players, his app list and layout, keep awake, lock, a real phone).

Each gate: the owner sets `read_only = false` on his own daemon, grants the scope by name (`swayconnectd devices grant <prefix> <scope>`), runs the step at his own keyboard, and reports the result. If a step fails, the logged reply goes back to the slice that owns it.

| Gate | After | The owner does | Pass |
|---|---|---|---|
| G-M1 | checkpoint M1 | looks at the app on the emulator or his phone against his live desktop; tries swipe in Drive mode once, on the page and on the card row | carousel, map and list match his screen; one swipe switches his workspace |
| G-WIN | D-INT.2a | `swayconnect` move a window, float it, change a layout, send to scratchpad and show it, close a throw-away window | each sway reply is `success` |
| G-KA | D-KA | first `systemd-inhibit --what=idle --mode=block --who=test --why=test sleep 420` by hand, no input for 6 min; then `swayconnect awake on --for 5m`, wait; then `off` | no lock while held; lock 300 s after release; `windows_inhibiting` and `session` match; a notice showed at start |
| G-LOCK | D-WIRE.2a | `swayconnect lock`, then `swayconnect lock` again | screen locks; the second call says already locked and starts no second `lock.sh` |
| G-AUDIO | D-AUDIO | volume to 30 % from the phone, mute, unmute | the speakers follow; the wob OSD reacts. (pactl writes, default device and the mic capture rule passed against a real PipeWire in M2c.) |
| G-MEDIA | D-MEDIA | play/pause and next on a browser tab, then on Spotify; once with the player paused since the daemon started | each player reacts; a paused player shows with Play. (The MPRIS client passed against a test player on a real bus in M2c.) |
| G-LAUNCH | D-APPS | launch a harmless app of his own list; restart the daemon | the app opens on the live desktop in its own `app-swayconnect-*.scope` and survives the restart. (Real `systemd-run`, the terminal refusal and fuzzel passed in a headless sway in M2c.) |
| G-INPUT | D-INPUT | type text with non-ASCII letters into a scratch terminal under his own keyboard layout; lock the screen and try again | text arrives as typed; locked refuses; the notice appears. (Focus change, Esc, Enter, Ctrl+C and the launcher target passed in a headless sway in M2c.) |
| G-PHONE | S13 | pair his phone over the LAN (QR code), then one volume change and one launch | pairing pins the certificate; both actions work over Wi-Fi |

## 6. Decisions taken in this plan (no owner answer needed)

1. Base concept Glance & Flick, with the owner's visual direction (pill nav areas, warm-neutral light theme, dark theme like reference 1).
2. Swipe drives the desktop by default when `control` is granted; Browse is one tap away. Both the page body and the card row (snapping to the centre card) change the workspace.
3. Card fill = focused on the computer; ring = shown on the phone.
4. Map in M1 needs only additive protocol fields; no breaking change exists in v2.
5. Keep awake = logind block inhibitor held by an expiring `systemd-inhibit` child; outputs powered on if off; 1 h default, 2 h max, 4 h continuous cap counted across breaks shorter than 15 min, 15 min cooldown; survives phone disconnect; tier 2, on at pairing (owner, 2026-11-14), removable with `devices deny`.
6. Audio through pactl; media through zbus; no album art; launch through `systemd-run --user --scope`; text through a daemon virtual keyboard.
7. No named presets (deferred). The `pair` default is one fixed set (owner, 2026-11-14): `state.read`, `state.titles`, `control`, `audio`, `media`, `idle.inhibit`, `launch`, `input.keys`, `input.text`. `destructive`, microphone opening (`audio.capture`) and risky launches (`launch.risky`) are their own tier 2 scopes, off at pairing, granted by name and enforced by the server.
8. material3 stays 1.4.0; no new Android library; no dynamic colour.
9. Deferred to `DESIGN.md` 11.13 (question 3 can bring one back): owner buttons, per-app streams, card profiles, media seek, Live typing, presets, Keystore key hardening, audit rotation, `status.json`, multi-output card groups, tablet split, landscape side panel, map↔list shared-element morph (a crossfade instead), first-reveal cascade, desaturation effect (an alpha dim instead), drag-to-workspace, pinch overview, touchpad, step-up (question 1: no), power menu, album art.

## 7. Risks

| Risk | Answer |
|---|---|
| Tiny tiles on a phone (the 7-Alacritty workspace) | same-app index, title fragments, tap by containment then nearest edge, semantics per small tile, dense chip, list view one tap away |
| Map lags a resize by up to 2 s | poll + binding events; motion makes the late update read as a change |
| zbus or Wayland crates fail `just deny` | MAN.2 finds it first; fallback for media is `playerctl` with fixed args, for input none (typing waits) |
| Virtual keyboard side effects with the owner's per-window layout tool (skbd) | one device per daemon run; G-INPUT checks both layouts |
| Lock detection misses a different locker | config `lock_processes`; documented limit |
| Keep awake while the screen is off | power-on command after taking the inhibitor (swayidle drops its pending resume) |
| A stolen, unlocked phone with the default scopes can run code as the owner (open fuzzel, start a terminal by name, type) | owner's choice (2026-11-14, `DESIGN.md` D30); lock refusal, focus check per part, notices, audit, revoke, panic; no step-up (question 1) |
| Hostile icon files | raster child with memory, CPU and time limits |
| Compose APIs named here differ in the 1.12.1 jars | each slice checks its APIs against the jars before use; `UX.md` 17 lists the ones to re-check |
