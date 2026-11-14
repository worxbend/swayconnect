# swayconnect Android UX, v2

Status: proposed, 2026-11-13 (review 2). Normative for the Android app once the owner accepts it.
Related: `docs/DESIGN.md` section 11 (architecture), `docs/PROTOCOL.md` section 17 (wire), `docs/V2_RESEARCH.md` (evidence), `docs/V2_PLAN.md` (slices).

Next action: the owner checks the wireframes of section 5 and the tokens of section 3, then slice A1 (theme) of `docs/V2_PLAN.md` starts.

## 0. Answer first

1. Base concept: "Glance & Flick" (highest judge scores: 8.2, 8.6, 8.0). Swipe on the body, tap to act, motion explains every change.
2. The owner's visual direction wins over the concept where they differ:
   - a floating pill bottom navigation with five areas (Desktop, Audio, Launch, Keys, More), not sheets over one screen;
   - a warm-neutral light theme (reference 2) and a dark theme built like reference 1;
   - the workspace carousel is the row of tall "day cards".
3. Swiping a workspace page **drives the desktop** by default when the phone has `control` (the owner's literal request). A visible "Drive / Browse" pill switches it off. Browse mode keeps a "Show on computer" button.
4. Card **fill** means "focused on the computer". A moving **ring** means "shown on the phone". They are never the same mark.
5. Everything is Compose on material3 1.4.0 (BOM 2026.09.00). No XML layout, no View widget, no WebView, no new library, no dynamic colour by default.
6. M1 builds the shell, theme, carousel, pager, list view, mini-map and icons. M2a fills More (keep awake, lock, scratchpad) and the window actions. M2b fills Audio, Launch and Keys.

## 1. Principles

| # | Principle | Rule |
|---|---|---|
| P1 | Glance before act | A workspace card says enough to decide without swiping: number, name, window count, focus fill, urgent dot. |
| P2 | One gesture, one meaning | Horizontal swipe on a page body or on the card row = change workspace. Tap = act. Long press = more actions. No horizontal drag inside a page. |
| P3 | Two truths, two marks | Fill = focused on the computer. Ring = shown on the phone. Each also has a word in semantics. |
| P4 | Motion explains state | Nothing jumps. Focus fades onto its card and slides in the map, new windows grow, closed windows collapse, removed workspaces shrink out. |
| P5 | Calm success, plain refusal | Success is the state change plus a haptic. Only a refusal gets words: one line, plain, with a `Reject` haptic. |
| P6 | Dense, never small | 48dp targets everywhere. Small map tiles use nearest-tile hit testing or the list. |
| P7 | Server truth | The UI renders server state. Sliders are the one exception, only while a finger is down. |
| P8 | Locked, not hidden | A feature the phone may not use keeps its place, shows a lock and says how the owner turns it on. |
| P9 | Danger looks different | Three tiers: plain, armed (shield), destructive (coral, after a divider, hold to confirm). |

## 2. Information architecture

```
App
├─ Pair flow (no profile, or a swayconnect://pair link)
│    Link ─► Check computer ─► Confirm code ─► What this phone can do ─► Desktop
└─ Shell (paired)
     ├─ Title row: screen title · status line · context pill · round action button
     ├─ Content of the selected area
     ├─ Now-playing capsule (M2b, when a player exists; not on Audio and Keys)
     └─ Floating pill navigation
          ├─ Desktop  workspace cards + pager → Map | List, focused-window card
          ├─ Audio    now playing, outputs, inputs                  (M2b; scope audio, audio.capture, media)
          ├─ Launch   search, recent, all apps, fuzzel              (M2b; scope launch, launch.risky)
          ├─ Keys     target, composer, special keys, shortcuts     (M2b; scope input.text, input.keys)
          └─ More     quick settings (keep awake, lock, mic, ...), settings, computer   (M2a)
Sheets (ModalBottomSheet): Window actions · Move to workspace · Small windows · Tab group ·
                           Scratchpad · Keep-awake duration · Locked feature · Computer · Confirm
```

Rules:

1. Navigation is a plain `when` over `enum class Area { Desktop, Audio, Launch, Keys, More }` in `UiState`. No navigation library.
2. Back closes the top sheet, then returns to Desktop. On Desktop with no sheet the app does not handle Back: with `android:enableOnBackInvokedCallback="true"` the system shows its predictive back-to-home preview before the app leaves.
3. The area is kept per profile. A new app start opens Desktop.
4. In M1 the four other areas exist as locked or "coming later" pages, so the nav bar never changes shape.

## 3. Theme

### 3.1 Colour tokens

Material 3 `ColorScheme` roles are set by hand. Dynamic colour is off. A setting "Use wallpaper colours" is not offered in v2.

| Role / token | Light (reference 2) | Dark (reference 1) | Used for |
|---|---|---|---|
| `background` | `#EDEAE4` greige | `#0E0E12` | screen |
| `surface`, `surfaceContainerLowest` | `#FFFFFF` | `#1B1B23` | cards, map slab, nav pill (light) |
| `surfaceContainer` | `#F4F2EE` | `#24242E` | icon badges, tiles, text fields |
| `surfaceContainerHigh` | `#E4E0D8` | `#2E2E3A` | pressed tiles, slider track |
| `onSurface`, `onBackground` | `#141414` | `#F2F2F5` | text |
| `onSurfaceVariant` | `#66625A` | `#A3A3AE` | secondary text, outline icons |
| `primary` ("ink") | `#141414` | `#FFFFFF` | focused card fill, selected nav pill, round action button, filled pills |
| `onPrimary` | `#FFFFFF` | `#14141C` | text on ink |
| `tertiary` ("accent") | `#E4E46C` lime | `#E4E46C` | accents on dark surfaces only (dark-theme cards and background) |
| `accentOnInk` (custom) | `#E4E46C` lime | `#5C5A00` olive | count line on the focused card, "N min left" on an "on" tile, any accent text on an ink fill. Always on `primary`. |
| `error` ("coral" marks) | `#A8402A` | `#FF9A80` | urgent dot and border, destructive icons and text, refusal marks |
| `errorContainer` ("coral" fill) | `#E8836A` | `#FF9A80` | filled coral pills behind `onErrorContainer` text |
| `onErrorContainer` | `#141414` | `#141414` | text on the coral fill |
| `secondaryContainer` (tinted pill) | `#E4E0D8` | `#2E2E3A` | "Floating", "Tabbed" pills |
| `ok` (custom) | `#3B6E2E` dot on `#DCEBD3` | `#8DBF7A` dot | "Ready", "Default" dots |
| `outline` | `#8A857C` | `#75757F` | dashed cards, map tile borders, dark nav stroke (non-text) |
| `navContainer` (custom) | `#FFFFFF` | `#1B1B26` | floating nav pill |
| `navSelected` (custom) | `#141414` | `#FFFFFF` | selected inner pill |
| `navOnSelected` (custom) | `#FFFFFF` | `#14141C` | its icon and label |
| `navIcon` (custom) | `#66625A` | `#8E8E99` | unselected outline icons |

Custom tokens live in `SwayTokens` provided through `staticCompositionLocalOf`.

Contrast (WCAG, computed 2026-11-13): ink on background 15.3:1, ink on card 18.4:1, `onSurfaceVariant` on background 5.1:1 (light) and 7.7:1 (dark), `accentOnInk` on `primary` 13.7:1 (light, lime on `#141414`) and 7.2:1 (dark, olive on `#FFFFFF`), ink on `errorContainer` 6.9:1, light `error` on white 6.1:1 and on greige 5.1:1, dark `error` on dark card 8.3:1, light `outline` on white 3.7:1 and on greige 3.05:1, dark `outline` on dark card 3.75:1 and on dark background 4.2:1, dark nav icon on dark pill 5.3:1. Lime on white is 1.35:1 and on greige 1.12:1, so lime never sits on a light surface; that is why `accentOnInk` exists.

**Map tile washes.** Eight washes per theme, picked by a stable hash of `app_id` (FNV-1a 32 bit, modulo 8). Light: low-chroma pastels between `#F1E9DD` and `#E3ECEF`, each at least 12:1 with ink text. Dark: muted tones between `#262632` and `#2F2A35`, each at least 10:1 with `#F2F2F5`. Washes sit near the slab colour (about 1.2:1), so every tiled, floating and sliver tile always has a 1dp `outline` border: the edges carry the layout. With the system "high-contrast text" setting the washes drop to `surfaceContainer` and the border becomes 2dp.

### 3.2 Shapes

| Token | Value | Used for |
|---|---|---|
| `extraSmall` | 8dp | map tiles |
| `small` | 12dp | text fields, small cards |
| `medium` | 20dp | window cards, audio cards |
| `large` | 24dp | workspace cards, map slab |
| `extraLarge` | 28dp | sheets (top corners) |
| pill | `CircleShape` | nav, status pills, context pill, buttons |

### 3.3 Type

System font (`FontFamily.Default`). No bundled font. Numbers use tabular figures (`fontFeatureSettings = "tnum"`).

| Style | Size / weight | Used for |
|---|---|---|
| `headlineLarge` | 32sp / Bold | screen title |
| `titleLarge` | 26sp / Bold, tnum | number on a workspace card |
| `titleMedium` | 16sp / SemiBold | card titles |
| `bodyMedium` | 14sp / Regular | secondary lines |
| `labelLarge` | 14sp / Medium | nav label, buttons, pills |
| `labelMedium` | 12sp / Medium | card name line |
| `labelSmall` | 11sp / Medium | count line, pills on tiles |

### 3.4 Icons

About 30 Material Symbols (Rounded, weight 400) written as Kotlin `ImageVector` builders in `ui/theme/SwayIcons.kt` and drawn with `Icon(imageVector = …)`. No XML, no resource ids; `res/drawable` keeps only the launcher icon. Each nav icon has an outline and a filled form. The set: desktop, audio (speaker), launch (apps grid), keyboard, more, plus, swap (drive), visibility (browse), map, list, fullscreen, float, move, close, lock, lock_open, mic, mic_off, volume, volume_off, coffee (keep awake), play, pause, next, previous, search, terminal, shield, scratchpad, moon (screens off), warning, check.

## 4. App shell

### 4.1 Title row (reference 2)

```
 worxbox                       (⇄ Drive)  (●)
 ● Ready   ☕ Awake 52 min                         
```

- Title: the area name, except Desktop, which shows the computer's name (`server.name`). `headlineLarge`, one line, ellipsis.
- The title row is `heightIn(min = 72.dp)` and grows with font scale. The title and the status line stack tight in one column (`weight(1f)`); the context pill and the round button are centred against that column. The status line is a 24dp visual row; Compose widens its touch area to 48dp.
- One gutter: `max(16.dp, system gesture inset)` on each side, computed once in the shell, holds the title, the cards, the page content and the offline line on one edge (in landscape the start gutter is 16dp, since the vertical nav already stands outside the gesture zone).
- Status line (under the title): a dot shape plus a word, then optional state pills. Tap opens the Computer sheet. Only the connection word is a polite live region; pills and counters are outside it (section 15).
- Context pill (48dp tall, `CircleShape`, card colour, icon + label) and round action button (56dp, ink, white icon) per area:

| Area | Context pill | Round button |
|---|---|---|
| Desktop | `⇄ Drive` / `◎ Browse` toggle | `+` open Launch aimed at the shown workspace |
| Audio | none | mute or unmute the default output |
| Launch | `On 4 ▾` target workspace | open the launcher on the computer (fuzzel) |
| Keys | none | Private mode on or off |
| More | none | lock the computer |

Each button follows `FeatureAvailability` (`PROTOCOL.md` 17.3): locked shows its lock glyph and opens the Locked sheet (section 10); read-only shows it disabled; unsupported hides it.

**Status dot shapes**, so colour is never the only sign:

| State | Shape | Word |
|---|---|---|
| Ready | filled circle, `ok` | Ready |
| Connecting / Authenticating | rotating arc | Connecting / Checking key |
| Backoff | hollow circle | Offline · retry in 8 s |
| Sway offline | crossed square | Sway not running |
| Locked (`session.locked == true`) | padlock | Locked |
| Screens off (every active output has `power == false`) | moon | Screens off |
| Read-only | eye | Read-only |
| Not paired / wrong key | triangle, `error` | Not paired / Wrong computer key |

Extra pills on the status line: `☕ Awake 52 min` (keep awake on), `⌨ Typing` (input session armed), `Drive paused` (rate limited). Timed pills and chips last `LocalAccessibilityManager.current.calculateRecommendedTimeoutMillis(base, containsControls = …)` with base 2 s, so TalkBack users get the longer system timeout.

### 4.2 Floating pill navigation (reference 1 and 2)

```
 ╭───────────────────────────────────────────────╮
 │  ▢   ♪   ▦  ( ⌨ Keys )   ⋯                    │   light: white pill, black selected pill
 ╰───────────────────────────────────────────────╯   dark: dark pill, white selected pill
```

- 64dp tall, fully rounded, 16dp from the screen sides, 12dp above the system navigation inset, 8dp shadow (light) or none with a 1dp `outline` stroke (dark).
- Unselected item: 48x48dp, outline icon in `navIcon`.
- Selected item: an inner pill (`navSelected`) 48dp tall, filled icon + label (`labelLarge`) in `navOnSelected`. Width = 16 + 24 + 8 + label + 20 dp.
- Selection change: the old pill shrinks while the new one grows. Width uses the spatial spring (section 13). Label fades in 120ms after the width starts, and out in 90ms. Icon swaps outline to filled with a 150ms crossfade.
- A locked area keeps its slot and shows a 12dp lock badge on the icon.
- Badges: Desktop gets an `error` dot when any workspace is urgent.
- Label fit, at any font scale: the label is measured with `rememberTextMeasurer`; when the selected pill plus the other items would overflow the bar, the selected pill shows the filled icon only, and the label lives in semantics.
- Landscape: the same pill stands vertical, inside the left `WindowInsets.systemGestures` inset (so it never sits in the back-gesture zone), centred vertically.

### 4.3 Now-playing capsule (M2b)

A 52dp floating pill directly above the nav, card colour: app icon badge, title · artist (one line, needs `state.titles`, else "Playing · Chromium"), play/pause and next (48dp each). Shown when a player is `playing` or `paused`, on Desktop, Launch and More. Not on Audio (it has the full card) and not on Keys (the IME needs the room). Tap the text: go to Audio. Long press: a small menu with "Hide until next track"; the same action is a `CustomAccessibilityAction`. No swipe (it would fight the page scroll).

## 5. Desktop

### 5.1 Portrait wireframe, map view (360 x 780dp)

```
┌────────────────────────────────────────┐
│ worxbox                (⇄ Drive)  (+)  │ title row, min 72dp
│ ● Ready   ☕ Awake 52 min              │
│ ╭────╮ ╭────╮ ┏━━━━┓ ╭────╮ ╭────╮      │ workspace cards 96dp row
│ │web │ │chat│ ┃code┃ │term│ │ emu│      │ name (labelMedium)
│ │ 1  │ │ 2 •│ ┃ 3  ┃ │ 4  │ │ 20 │      │ number (titleLarge)
│ │2 wi│ │1 wi│ ┃3 wi┃ │7 wi│ │2 wi│      │ count; filled = focused on computer
│ ╰────╯ ╰────╯ ┗━━━━┛ ╰────╯ ╰────╯      │ ring (not drawn here) = shown on phone
├────────────────────────────────────────┤ ▲ fixed   ▼ the pager: each page is one LazyColumn
│ code · 3 windows              [▦ | ☰] │ page header 40dp, view toggle
│ ╭────────────────────────────────────╮ │
│ │ ╭────────────────╮╭──────────────╮ │ │ map slab (white card, 24dp)
│ │ │ ◉  Chrome      ││ ◉ Alacritty  │ │ │ 16:9 → about 328 x 185dp
│ │ │    Docs        ││   ~/sway  #1 │ │ │ #1 #2 = same-app index
│ │ │  (ink = focus) │╰──────────────╯ │ │
│ │ │                │╭──────────────╮ │ │
│ │ │                ││ ◉ Alacritty#2│ │ │
│ │ ╰────────────────╯╰──────────────╯ │ │
│ ╰────────────────────────────────────╯ │
│ (+2 small windows)  (Floating · 1)      │ chips only when needed
│ ╭────────────────────────────────────╮ │
│ │ (◉) Chrome — Docs                  │ │ focused-window card
│ │     google-chrome  [Focused] [Tiled]│ │
│ │ [⤢ Full] [⧉ Float] [⇥ Move] [⌨] [✕]│ │ 48dp buttons, ✕ `error`, hold
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (♫) Song — Artist        ▶‖   ▶▶   │ │ now capsule (M2b)
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (▢ Desktop)  ♪    ▦    ⌨    ⋯      │ │ floating pill nav
│ ╰────────────────────────────────────╯ │
└────────────────────────────────────────┘
```

Vertical budget: status bar 24, title row 72, cards 96, header 40, map 185, chips 40, focused card 132, capsule 60, nav 88. Total 737dp.

Scroll ownership: the title row, the workspace card row and the nav are fixed. The pager takes the remaining height (`Modifier.weight(1f)`). Each page scrolls vertically on its own: it is one `LazyColumn` whose items are, in map view, the page header, the map slab, the chips and the focused-window card; in list view, the page header, the window cards, the dashed card and the focused-window card. No `verticalScroll` parent wraps the pager, so no lazy list is ever measured with infinite height.

### 5.2 Portrait wireframe, list view

```
│ code · 3 windows              [▦ | ☰] │
│ ╭────────────────────────────────────╮ │
│ │ (◉) Chrome — Docs                  │ │ white card, round icon badge
│ │     google-chrome                  │ │ app id line
│ │ [■ Focused]  [Tiled]               │ │ ink pill = focused on computer
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (◉) Alacritty — ~/sway        #1   │ │ same-app index on the right
│ │     Alacritty                      │ │
│ │ [Tabbed · 2 of 3]                  │ │ tinted pill
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (◉) Signal                         │ │
│ │     signal                         │ │
│ │ [● Urgent]  [Floating]             │ │ coral fill, ink dot + word
│ ╰────────────────────────────────────╯ │
│ ┌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┐ │
│ ╎ +  Open an app on 3                ╎ │ dashed card → Launch, target 3
│ └╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┘ │
```

Window card anatomy:

- Round icon badge 44dp (`surfaceContainer`), app icon 28dp inside. In map tiles a monogram (no icon yet) sits on a `surface` disc, so it reads as a badge on any wash.
- Title `titleMedium`, 2 lines, ellipsis. Without `state.titles`: the app id. Display rule for titles (client only, the wire keeps PROTOCOL 2.4): runs of U+FFFD at the ends are dropped and inner runs become one space, because invisible direction marks (U+200E, U+2068, U+2069) arrive as U+FFFD; a title left blank shows the app id.
- App id line `bodyMedium`, `onSurfaceVariant`.
- State pills, in this order: Focused (ink fill), Urgent (`errorContainer` fill, ink dot + word), Fullscreen, Floating, "Tabbed · 2 of 3" / "Stacked · 1 of 2" (tinted).
- Trailing: same-app index `#2` when two windows on the page share an `app_id`; a spinner while an action on it waits.
- Order: layout tree order. Tabbed and stacked groups are not split into headers; the pill says it.

### 5.3 Landscape wireframe (780 x 360dp)

```
┌──┬─────────────────────────────────────────────────────────────────────────────┐
│╭╮│ worxbox  ● Ready  ☕ 52m     [web 1][chat 2][■code 3]…      (⇄ Drive) (+)    │ title + compact cards
│││├─────────────────────────────────────────────────────────────────────────────┤
│▢││ code · 3 windows                                          [▦ | ☰]          │
│♪││ ╭──────────────────────────────────────────────╮                            │
│▦││ │ map slab fills the height (≈ 520 x 292dp)     │  the page scrolls          │
│⌨││ ╰──────────────────────────────────────────────╯  vertically to the        │
│⋯││ (+2 small) (Floating · 1)                         focused-window card      │
│╰╯│                                                                             │
└──┴─────────────────────────────────────────────────────────────────────────────┘
 vertical pill nav      pager, full width
```

- Landscape when `maxWidth > maxHeight && maxHeight < 480.dp`.
- Workspace cards collapse to one line (name + number) inside the title row: a 40dp visual pill with `Modifier.minimumInteractiveComponentSize()`, so the touch target is 48dp. Fill and ring rules stay.
- The pager spans the width right of the nav; each page is the same `LazyColumn` as in portrait.
- Tablet split, side panel and output groups are later (`DESIGN.md` 11.13).

### 5.4 Workspace cards (the carousel)

**Anatomy** (min 64dp wide, `heightIn(min = 88.dp)` with no maximum, `large` shape, 8dp apart). Edge padding: when every card fits (widths measured from their texts), the row starts on the page gutter like reference 2 and does not scroll; only when the cards overflow does each side get half the free width, so that any card can reach the centre.

1. Name line: the sway name without its `N:` prefix, `labelMedium`, one line, ellipsis. Empty when the name equals the number.
2. Number: `titleLarge`, tabular. A workspace without a number shows its first 2 graphemes.
3. Count line: `"1 window"`, `"3 windows"`, `"empty"`, `labelSmall`. The card grows in height to fit its text.

**Marks:**

| Meaning | Mark | Semantics word |
|---|---|---|
| Focused on the computer | card filled `primary` (ink), text `onPrimary`, count line in `accentOnInk` | "focused on computer" |
| Shown on the phone | 2dp ink ring, 3dp outside the card | "selected" (tab state) |
| Urgent | 14dp `error` dot with "!" at the top right; pulses once (2 cycles of alpha 1→0.4→1, 600ms) when it turns urgent | "needs attention" |
| Pending focus | 16dp spinner replaces the count line | "switching" |

Pages are workspaces only, one per workspace in sway's order (all outputs together). Output groups are later. The scratchpad is not a page and has no card: it is the Scratchpad sheet (More tile, M2a), which lists `desktop.scratchpad` with "Show" (`scratchpad.show`).

**Row behaviour.** The row is a snapping carousel (`rememberSnapFlingBehavior(rowState, SnapPosition.Center)`). A swipe on the row is the owner's "swipe the carousel": when a user drag of the row settles, the centred card becomes the shown page (`pagerState.scrollToPage`) and, in Drive mode, drives the desktop (5.5). When the pager moves, the row scrolls the shown card to the centre; that scroll is not user-initiated and drives nothing. Tapping a card scrolls the pager to it (and drives the desktop when Drive is on). Long press on a card opens the workspace sheet: "Show on computer", "Move focused window here" (M2a).

### 5.5 Pager and swipe semantics

- One page per workspace, keyed by workspace id. The selected **workspace id** is the source of truth. When sway adds or drops workspaces, the page index is re-found by id and set without animation.
- If the shown workspace disappears, its page shrinks and fades (150ms) and the pager lands on the desktop's focused workspace.

**Drive mode (default when `control` is granted).**

1. Only a **user-initiated** settle drives. A settle is user-initiated when, since the last settle, the pager's `interactionSource` saw a `DragInteraction.Start` (a finger swipe), the card row saw a user drag, or a card was tapped. Follow-focus scrolls, re-sync jumps, keep-in-view row scrolls and accessibility scroll actions are not user-initiated. The flag clears after one send.
2. When such a settle holds for 250ms, the app reads the workspace id of the settled page from `pages` at that moment and sends `workspace.focus {workspace_id}`, unless that id equals the confirmed focused id. A fling across four pages sends one request.
3. The ring is on the new card at once (phone view). The ink fill changes only when `state.changed` confirms: the old card's container colour fades to `surface` and the new one's to `primary` (`animateColorAsState`, `defaultEffectsSpec`); the content colour follows. So the change confirms the computer, not the finger.
4. `rate_limited` or `read_only`: `Reject` haptic, status pill "Drive paused" (timed as in 4.1), no retry.
5. A workspace without a usable number (`unsupported_target`): the page stays, one line "This workspace cannot be shown from the phone."
6. Computer locked (`session.locked == true`): Drive pauses; the page header shows "The computer is locked." Swipes change only the phone's view.

**Browse mode** (toggle in the context pill; also when `control` is missing or the daemon is read-only, then the pill is locked):

1. Swiping changes only the phone's view.
2. When the shown page is not focused on the computer, the page header shows a tonal pill button "Show on computer" (48dp). When it is, the header shows "On screen" with an ink dot.

**Follow focus** (both modes). When the computer's focus moves to another workspace (the owner used his keyboard):

- the pager scrolls there if no finger is down, no own request is pending, the pager was not touched in the last 1.5 s, and touch exploration (TalkBack) is off;
- otherwise a chip "Computer moved to 7 →" appears in the page header (base 5 s, timed as in 4.1); tapping it scrolls there.

Loop guard: a follow-focus scroll is not user-initiated, so it never sends `workspace.focus` (5.5 rule 1); a confirmation of the app's own request finds the pager already on that page, so nothing moves. Incoming focus never interrupts a drag.

**Edge insets.** Page content keeps a horizontal margin of `max(16.dp, WindowInsets.systemGestures)` on both sides, so a swipe rarely starts in the system back-gesture zone. No `systemGestureExclusion` on the pager (the system caps it at 200dp).

**Edges.** No wrap-around. The default overscroll stretch shows the end.

**Haptics.** `SegmentTick` on every settled page. `Confirm` when Drive's focus is confirmed. `Reject` on refusal.

### 5.6 Page header

40dp: `<name> · <count>` (`titleMedium`), in Browse mode the "On screen" pill (neutral dot + word) when the page is focused on the computer, then on the right the view toggle (two 48dp icon segments, map and list, content descriptions "Layout view" and "List view"). Directly under it, 4dp apart, the page's one-line states (`bodyMedium`): why the list is forced ("Layout too large to send; showing the list" or "No layout for this workspace; showing the list"), read-only, screens off, the last refusal. Then, when needed, one row of chips: "Show on computer", "Computer moved to 7 →", "Larger text · Show list". "+N small windows", "Floating · N" and "Dense workspace · Show list" sit under the map. Chips and tonal buttons carry a 1dp `outline` edge (3:1 on the background).

The view choice is stored per profile. Default: map.

### 5.7 Focused-window card

In list view the focused window is already one of the window cards: its buttons go into that card, and no second focused-window card follows (a copy directly under its own card read as a bug). Map view keeps the separate card.

A window card (5.2 anatomy) for the window that is focused on the computer, when it is on the shown workspace. Otherwise a dashed card with the hint "Tap a window to focus it".

Buttons (48dp, icon + short label, icon only below 360dp width or above font scale 1.5):

| Button | Op | Scope | Tier |
|---|---|---|---|
| Full / Exit full | `window.fullscreen` | control | plain |
| Float / Tile | `window.floating` (M2a) | control | plain |
| Move | opens Move sheet → `window.move` (M2a) | control | plain |
| Type | goes to Keys with this window as target (M2b) | input.text | armed (shield) |
| Close | `window.close` (M2a), hold 600ms | destructive | destructive, last, after a 1dp divider |

Each button follows `FeatureAvailability` (`PROTOCOL.md` 17.3): locked shows a lock and opens the Locked sheet; read-only shows it disabled; unsupported (not in `known_ops`) hides it.

### 5.8 Window sheet (long press on a tile or card)

```
╭──────────────────────────────────────────╮
│ (◉) Alacritty — ~/sway                   │
│     Alacritty · workspace 4 · tiled      │
├──────────────────────────────────────────┤
│ [Focus] [Fullscreen] [Float] [Scratchpad]│
│ Move to workspace                        │
│ (1) (2) (3) (■4) (20) (+ new)            │ number pills; "+ new" = lowest free number
│ Layout of its container                  │
│ [ Split ↔ | Split ↕ | Tabbed | Stacked ] │ segmented
│ Some tools (autotiling) may change this. │ one-time note
│ ──────────────────────────────────────── │
│ ⌨ Type into this window            🛡    │
│ ✕ Close window… (hold)                   │ `error` text
╰──────────────────────────────────────────╯
```

### 5.9 States of the Desktop area

| State | What shows |
|---|---|
| First connect, no cache | three skeleton cards and a skeleton slab with two tiles. No shimmer under reduce motion. |
| Connecting with cache | last desktop from `filesDir/last_desktop.json`, dimmed (`alpha = 0.6`), status "Connecting". Taps disabled; semantics say "connecting". |
| Offline | same, "Offline · last seen 12 s ago" (ticks each second for a minute, then each minute; outside the live region), "Retry now" in the Computer sheet and as a chip. |
| Sway offline | empty slab of the last output aspect, "Sway is not running on worxbox. The phone reconnects when it starts." Audio, Launch (without "on workspace") and More stay usable. |
| Read-only | status "Read-only". Controls whose op is in `known_ops` stay visible, disabled, so nothing jumps when it ends. One line: "The computer is set to look only." |
| Computer locked (`session.locked == true`, M2a) | status "Locked". The map shows; Drive pauses with the line "The computer is locked." |
| Screens off (every active output `power == false`) | status "Screens off". The map shows; one line "The screens are off." |
| Empty workspace | slab with a dashed inner outline and "Nothing open on 5", plus a dashed card "+ Open an app here" (Launch with target 5). In list view: only the dashed card. |
| Dense workspace | when more than 30 % of tiles are under 24dp and at least 3 tiles are: chip "Dense workspace · Show list" (switches this page only, not stored). Never switches by itself. |
| Layout dropped (`layouts_cut` true and `layout` null) | list view for the page, line "Layout too large to send; showing the list". |
| No layout (`layout` null, `layouts_cut` false: no measurable area) | list view for the page, line "No layout for this workspace; showing the list". |
| Old daemon (no `layout` field) | list view, toggle disabled with "Needs a newer swayconnectd". |
| Many workspaces (10+) | the row scrolls and snaps; the shown card stays centred. |
| Truncated | one line "The computer has more windows than the phone can show." |

## 6. Mini-map rendering rules

### 6.1 Geometry

1. The slab is a white card (`large` shape, 8dp padding). Inside it, the map keeps the aspect `layout.size[0] : layout.size[1]` and is letterboxed.
2. `mapW = min(innerW, innerH * size[0] / size[1])`; `mapH = mapW * size[1] / size[0]`; `offset = ((innerW - mapW) / 2, (innerH - mapH) / 2)`; `tile.x = offset.x + r[0] / 10000 * mapW`, `tile.y = offset.y + r[1] / 10000 * mapH`, `tile.w = r[2] / 10000 * mapW`, `tile.h = r[3] / 10000 * mapH`; then the 2dp inset of rule 3.
3. Each tile is inset 2dp on every side (sway gaps are not sent), shape `extraSmall` (8dp), with a 1dp `outline` border (3.1).
4. Rects outside 0..10000 are clamped. A tile whose window is not in `windows` is skipped.
5. Pure function `MapGeometry.place(layout, mapSize, density, fontScale): List<PlacedTile>` holds all of this and is unit tested.

### 6.2 Size classes (measured in dp after scaling)

| Tile size | Content | Tap |
|---|---|---|
| ≥ 120 × 80 | 24dp icon, title (2 lines, `labelLarge`), app id (`labelSmall`), same-app index | own target |
| ≥ 48 × 48 | 28dp centred icon, same-app index badge (`#2`) in a corner when it fits | own target |
| 24–48 in either side | 16dp icon, drawn in the shared small-tile layer; an invisible placed child of the same rect carries its semantics | the hit rule below |
| < 24 in either side | a sliver in the app wash colour, in the shared layer | not tappable; counted in "+N small windows" |

Hit rule for the shared layer: a tap inside a tile's rect hits that tile; otherwise the tile whose nearest edge is closest, within 24dp. Never the nearest centre (a long thin tile would lose to a neighbour).

Classes are chosen after measuring the title at the current font scale (`rememberTextMeasurer`), so large text falls to icon-only sooner: the large class needs room for the 32dp icon disc, an 8dp gap, 8 characters of `labelLarge` and 16dp of padding; a narrower tile is medium (centred icon and `#N`).

A sliver (< 24dp) is a plain wash bar: no border, no shadow, corner radius at most 2dp; ink when focused, `error` when urgent.

"+N small windows" chip opens the Small windows sheet: a list of those windows with the card anatomy.

### 6.3 Same-app windows

When two or more tiles on a page share an `app_id`, each gets its tree-order index (`#1`, `#2`, …) as a badge (≥ 48dp tiles, whenever the measured `#N` plus 12dp fits the tile width) and in its semantics. Tiles ≥ 120 × 56dp also show the first 12 characters of the title. The owner's workspace of 7 Alacritty windows then reads as 7 distinct tiles.

### 6.4 Focus, urgent, pending

- Focused on the computer: ink fill, `onPrimary` text, icon on a `surface` 32dp disc. One shared "focus" node with its own bounds slides from tile to tile when focus moves (signature motion).
- Urgent: 2dp `error` border + "!" dot. Pulses once when it turns urgent.
- Pending: a 16dp spinner in the tile corner; taps ignored until the answer or the next state.

### 6.5 Tabbed and stacked containers

- One tile for the container. A header strip on top: 20dp tall.
- Tabbed: horizontal segments, one per child, each with the child's 14dp icon. The shown child (`a`) has an ink underline.
- Stacked: thin rows (8dp each, max 4 shown, then "+N").
- The body shows the shown child like a normal tile.
- A segment at least 48dp wide focuses that child on tap. Narrower strips: a tap on the strip opens the Tab group sheet ("Tabbed · 5 windows").
- A tab that holds a split shows the icons of its windows in the segment, max 2.
- Semantics: "Tabbed group, 5 windows, showing Chrome".

### 6.6 Floating windows

- Drawn after the tiled layer, in `floating` order (last on top), white, 1dp `outline` border, 6dp shadow (light) or a 1dp lighter border (dark).
- A floating container with several windows draws each window.
- When floats cover more than 60 % of the map area, the chip "Floating · N" appears; it hides and shows them (not stored).

### 6.7 Fullscreen

- `layout.fullscreen` set: that window's tile covers the whole map, with a "Fullscreen" pill. The tiled layout shows behind it at 30 % alpha.

### 6.8 Multiple outputs

- Each page uses its own workspace's `layout.size`, so a portrait monitor gives a tall map. The page header adds "on HDMI-A-1" when the workspace is not on the focused output.
- Output groups on the card row and an "All outputs" overview are later, not v2.

### 6.9 Motion inside the map

- Pages with at most 25 tiles: each tile in a `LookaheadScope` with `Modifier.animateBounds`, keyed by window id. New windows grow from their rect, closed windows collapse, splits morph.
- Pages with more than 25 tiles: no bounds animation (snap). The focus node still slides.
- Reduce motion: everything snaps.

### 6.10 Gestures in the map

Tap = `window.focus`, in both Drive and Browse mode (P2: a tap always acts). Long press = Window sheet. No horizontal drag: it belongs to the pager. Drag-to-workspace and pinch overview are later.

## 7. Audio (M2b)

### 7.1 Portrait wireframe

```
┌────────────────────────────────────────┐
│ Audio                              (🔇) │
│ ● Ready                                │
│ ╭────────────────────────────────────╮ │
│ │ (◉) Song title                     │ │ now playing card
│ │     Artist · Chromium              │ │
│ │      ⏮       ( ▶‖ )       ⏭        │ │ 72dp play button, shape morph
│ ╰────────────────────────────────────╯ │
│ Output                                 │
│ ╭────────────────────────────────────╮ │
│ │ (🔈) Line out           [■ Default]│ │
│ │      Analog stereo                 │ │
│ │ (🔇)  ━━━━━━━━━●───────────  40 %  │ │ slider only on the default device
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (🎧) Headphones        [Unplugged] │ │ tinted pill, row disabled
│ ╰────────────────────────────────────╯ │
│ Input                                  │
│ ╭────────────────────────────────────╮ │
│ │ (🎙) Analog mic   [● Live] (Mute)  │ │ mic state as word + icon
│ │      ━━━━━━━●──────────────  65 %  │ │ slider needs audio.capture
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ (▢ ) (♪ Audio)  ▦   ⌨   ⋯          │ │
│ ╰────────────────────────────────────╯ │
└────────────────────────────────────────┘
```

### 7.2 Rules

- Tap a device card: `audio.set_default`. The "Default" pill and the slider move to it (`animateContentSize`, spatial spring). `Confirm` haptic. For an input device this needs `audio.capture`; without it the card shows a lock and opens the Locked sheet.
- Slider: the value/onValueChange `Slider` (stable). Local value while dragging, sends at most 10 per second plus the final value in `onValueChangeFinished`, then shows the server value from the next state. Range 0 to `max_volume_pct`. `SegmentFrequentTick` every 5 %, `GestureThresholdActivate` at 0 and at the top.
- Mute: `IconToggleButton` 48dp, `ToggleOn`/`ToggleOff`, the word "Muted" next to the percent.
- Mic: muting is one tap under `audio`. Unmuting, the mic slider and picking the input device need `audio.capture` (the server enforces it; without it they show a lock). With it, **unmuting needs a 600ms hold** (UX only; the server shows a desktop notice). TalkBack: double tap opens a confirm dialog instead.
- Volume keys: while the Audio area is shown, the phone's volume keys step the default sink by 5 %, and a small pill "PC 45 %" shows under the title row for 1.2 s. Other modes are later.
- `available: false`: "Sound server not reachable on worxbox." Media card still works if `media` is granted.
- No player: the now-playing card reads "Nothing playing".
- Without `state.titles`: "Playing · Chromium".
- No seek bar, no per-app streams and no card profiles in v2 (`DESIGN.md` 11.13).

## 8. Launch (M2b)

### 8.1 Portrait wireframe

```
┌────────────────────────────────────────┐
│ Launch                  (On 3 ▾)   (⌕) │ ⌕ = show fuzzel on the computer
│ ● Ready                                │
│ ╭────────────────────────────────────╮ │
│ │ 🔍 Search apps                     │ │ pinned TextField
│ ╰────────────────────────────────────╯ │
│ Recent                                 │
│ (◉)    (◉)    (◉)    (◉)               │
│ Firefox Alacr. Signal Files            │
│ All apps                               │
│ (◉)    (◉)    (◉)    (◉)               │ LazyVerticalGrid Adaptive(80dp)
│ ...                                    │
│ ╭────────────────────────────────────╮ │
│ │ (▢ ) ♪  (▦ Launch)  ⌨   ⋯          │ │
│ ╰────────────────────────────────────╯ │
└────────────────────────────────────────┘
```

### 8.2 Rules

- App cell: 56dp round icon badge, 2-line label (`labelMedium`, centred). Risky apps (terminal or owner-marked) carry a 14dp shield. Without `launch.risky` they show a lock and open the Locked sheet (the server refuses them anyway).
- Tap: `app.launch {entry_id, number}` with the target workspace of the context pill (default: the computer's focused workspace). Risky apps need a 600ms hold (UX only). Then the app **returns to Desktop** on the target page and draws a ghost tile ("Opening Firefox…", icon pulsing at 1 Hz) until any new window id appears on the target workspace or 10 s pass. After 10 s: one line "Firefox did not open a window yet." No toast on success.
- Long press: "Open on workspace…". "Pin to top" (pins stored on the phone per profile) is deferred: not in M2 (`V2_PLAN.md` status, "Not done"). A risky app has no long-press menu; its press is the hold.
- Search: client-side fuzzy match over name, generic name and keywords (`FuzzyMatch`, pure, tested). The grid reflows with `animateItem`. The keyboard opens only when the field is tapped.
- Round button (⌕): `launcher.open`. If `input.text` is granted and `session.launcher.holder` is `this_device`, the app goes to Keys with the target "Launcher (fuzzel)". Otherwise one line: "Fuzzel is open on the computer."
- Owner buttons are later (`DESIGN.md` 11.13).
- Empty search result: dashed card "No app matches 'xyz'".

## 9. Keys (M2b)

### 9.1 Portrait wireframe

```
┌────────────────────────────────────────┐
│ Keys                               (🔒) │ 🔒 = Private mode
│ ● Ready   ⌨ Typing                     │
│ ╭────────────────────────────────────╮ │
│ │ (◉) Alacritty — ~/sway        ws 4 │ │ target card, captured on entry
│ │ [● Terminal: Enter runs commands]  │ │ coral fill pill for known terminals
│ ╰────────────────────────────────────╯ │
│ ╭────────────────────────────────────╮ │
│ │ git status▌                        │ │ TextField(state)
│ ╰────────────────────────────────────╯ │
│        [ Send ]   [ Send + ⏎ (hold) ]  │
│ (Esc)(Tab)(Ctrl)(Alt)(❖)(←)(↑)(↓)(→)(⌫)│ special keys, scrolls, above the IME
│ (Copy)(Paste)(Undo)(Ctrl+C)(Ctrl+L)    │ shortcuts
├──────────────── IME ───────────────────┤
```

The pill nav is hidden while the IME is open; back closes the IME first.

### 9.2 Rules

- **Target.** The focused window when Keys opens (or the one picked from Desktop's "Type"). Every request carries its `window_id`. When `session.launcher.open` is true and `session.launcher.holder` is `this_device`, a second target "Launcher (fuzzel)" is offered.
- **Send (the one mode in v2).** Compose, then Send. Autocorrect, glide and voice work. The field clears after delivery. Text over 1024 bytes is split at character boundaries into requests of at most 1024 bytes (`TextSplit`, pure), sent in order, stopping at the first error. Enter is never in the text: "Send + ⏎" sends the text, then `input.key Return`. For known terminals (alacritty, foot, kitty, wezterm, gnome-terminal, konsole, xterm) "Send + ⏎" needs a 400ms hold. Live typing is later.
- **No IME learning, in every mode.** The composer sits inside `InterceptPlatformTextInput`; its interceptor wraps the `PlatformTextInputMethodRequest` and, in `createInputConnection(outAttrs)`, adds `EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING` and `IME_FLAG_NO_EXTRACT_UI` to `outAttrs.imeOptions`. So the keyboard does not learn or sync commands, host names or passwords typed for the computer.
- **Private mode.** `BasicSecureTextField` with `KeyboardType.Password`, `FLAG_SECURE` on the window, the text dropped on rotation and after Send. Not the default for terminals: the IME flags above already stop learning, and hidden text would make typing commands blind.
- **Special keys.** 48dp tonal chips. Ctrl, Alt, Super are sticky: filled while armed, `ToggleOn`, released after the next key. Super combos carry a shield (they reach sway bindings).
- **Refusals.**
  - `focus_changed`: the target card gets a 2dp `error` border and a warning icon: "Focus moved to Firefox." Buttons "Type there" and "Back to Alacritty" (the second sends `window.focus`). "Sent 16 of 40 characters." The unsent tail stays in the field. `Reject`.
  - `locked`: full-width card "The computer is locked. Typing is off, so nothing reaches the lock screen." Composer disabled.
  - `keyboard_grabbed`: "A menu or launcher on the computer has the keyboard. Close it or type into the launcher."
  - `launcher_closed`: "The launcher on the computer is closed." The target falls back to the focused window; nothing is re-sent.
- `FLAG_SECURE` is set on Keys in every mode.
- Leaving Keys or backgrounding the app ends the phone's input session indicator.

## 10. More (M2a content; M1 shows the Computer card)

M1 has no settings store, so Theme, Default view, Reduce motion and Haptics come with A11 in M2a (`docs/V2_PLAN.md` status); M1 More shows the Computer card and one dashed "later" card.

### 10.1 Portrait wireframe

```
┌────────────────────────────────────────┐
│ More                               (🔒) │ round = lock computer
│ ● Ready                                │
│ ╭─────────────────╮ ╭─────────────────╮│ quick settings, 2 columns
│ │ (☕)            │ │ (🎙)            ││ 88dp tall tiles
│ │ Awake           │ │ Mic             ││
│ │ 52 min left     │ │ Live            ││ ink tile = on; count in accentOnInk
│ ╰─────────────────╯ ╰─────────────────╯│
│ ╭─────────────────╮ ╭─────────────────╮│
│ │ (⇄) Drive       │ │ (▣) Scratchpad  ││
│ │ On              │ │ 2 hidden        ││
│ ╰─────────────────╯ ╰─────────────────╯│
│ ╭─────────────────╮ ╭─────────────────╮│
│ │ (⌕) Launcher    │ │ (🔒) Lock       ││
│ │ on computer     │ │ now             ││
│ ╰─────────────────╯ ╰─────────────────╯│
│ Settings                               │
│  Volume keys · Follow focus · Haptics  │
│  Theme · Default view · Reduce motion  │
│ Computer                               │
│  worxbox · aa51 08ad … · Scopes · Forget│
└────────────────────────────────────────┘
```

### 10.2 Keep awake tile

| State (topic `session`) | Tile | Tap | Long press |
|---|---|---|---|
| off | card colour, outline coffee icon, "Keep awake · Off" | `keep_awake.set {enabled: true}` with the default duration | duration sheet |
| on, this phone | ink fill, filled icon, "Awake", "52 min left" in `accentOnInk` | `keep_awake.set {enabled: false}` | duration sheet with "1 h from now" |
| on, other device | ink fill, "Awake · from another device" | turn off (any device may) | duration sheet |
| cooldown (`limit_reached`) | disabled, "Available in 9 min" | `Reject` + one line | none |
| computer locked (`locked == true`) | as its keep-awake state, second line "Computer is locked" | as its state | as its state |
| scope missing | lock badge | Locked sheet | Locked sheet |

- Duration sheet: segmented 15 min / 30 min / 1 h / 2 h (only values up to `max_duration_s`; above font scale 1.5 a vertical radio list), and two lines: "Turning it on wakes the screens." and "The computer locks as usual when this ends." Each choice replaces the expiry: "1 h from now" sends `duration_s = min(3600, max_duration_s)`.
- The remaining time counts down on the phone from `remaining_ms`, per minute; under one minute per second. The countdown is not a live region; its `contentDescription` ("Awake, about 52 minutes left") changes at most every 5 minutes.
- When `windows_inhibiting > 0`, the tile's second line adds "A window also keeps it awake".
- Status line pill on every area: "☕ Awake 52 min". Tap = duration sheet.
- Haptics `ToggleOn` / `ToggleOff`. TalkBack: "Keep awake, on, 52 minutes left. Double tap to turn off."

### 10.3 Other tiles

- Mic (M2b): mute free; unmute needs `audio.capture` and a hold (7.2).
- Drive: same toggle as the Desktop context pill.
- Scratchpad: opens the Scratchpad sheet: windows of `desktop.scratchpad` with "Show" (`scratchpad.show`).
- Launcher on computer (M2b): `launcher.open`.
- Lock: `session.lock`, no confirm (locking is the safe direction). `Confirm` haptic. `{already_locked: true}`: one line "Already locked."

### 10.4 Locked feature sheet

```
╭──────────────────────────────────────────╮
│ 🔒 Terminals are off for this phone      │
│ It lets the phone start terminals and    │
│ apps marked risky. That is the same as   │
│ running commands.                        │
│ To turn it on, run on the computer:      │
│ ┌──────────────────────────────────────┐ │
│ │ swayconnectd devices grant wMHC      │ │ mono, selectable
│ │   launch.risky                       │ │
│ └──────────────────────────────────────┘ │
│ [ Copy ]   [ Share… ]                    │
│ The phone reconnects by itself after.    │
╰──────────────────────────────────────────╯
```

- One honest risk sentence per scope (table in 10.5).
- A default pairing (`DESIGN.md` 11.8) leaves only `destructive`, `audio.capture` and `launch.risky` off, so those are the usual Locked sheets. Every other scope can still be missing (paired with `--scopes`, removed with `devices deny`, or an older pairing); the sheet then reads the same way. The text never says a scope is "off by default".
- The prefix is the first 4 characters of the device id (more if the daemon lists a clash).
- `swayconnectd devices grant` exists from M1 on, so the sheet text is the same in every milestone.
- On `bye` `scopes_changed` the app reconnects at once. A newly unlocked feature morphs its lock into its icon, `ToggleOn`, one line "Terminals are now on." (the feature's name).

### 10.5 Scope sentences

| Scope | Sentence |
|---|---|
| `control` | "Lets the phone switch workspaces, move, float or fullscreen windows, and lock the computer." |
| `audio` | "Lets the phone change the volume and the output device, and mute the microphone." |
| `audio.capture` | "Lets the phone turn the microphone on, change its level and pick the input device. Someone with this phone could listen to the room." |
| `media` | "Lets the phone play, pause and skip media." |
| `idle.inhibit` | "Lets the phone stop the computer from locking itself when idle, for up to {max_duration} at a time, again after each expiry, up to {max_continuous} in a row." (from topic `session`; without it: "for a limited time") |
| `launch` | "Lets the phone start apps and open the launcher. Terminals and apps marked risky need a second permission." |
| `launch.risky` | "Lets the phone start terminals and apps marked risky. That is the same as running commands." |
| `input.text`, `input.keys` | "Lets the phone type into the focused window. In a terminal that is the same as running commands." |
| `destructive` | "Lets the phone close windows. Unsaved work can be lost." |

### 10.6 Computer sheet

Server name, short fingerprint, device id prefix, granted scopes (ink pills) and known but missing scopes (outline pills with a lock; tap = Locked sheet), read-only flag, "Retry now", "Forget this computer" (confirm dialog).

## 11. Pairing

1. **Welcome card.** "Control your sway desktop from here." Primary: "Paste link". The line "On the computer, run: swayconnectd pair" in a mono pill with Copy.
2. **Target card.** Host, port, fingerprint in four 4-digit groups, the computer name. Device name field (prefilled `Build.MODEL`). "Pair" (ink, full width).
3. **Confirm code.** Six digits, 64sp tabular, split 3 + 3, inside a slowly breathing ring (static under reduce motion). Text: where the code appears, answer no if it differs, keep the app open.
4. **What this phone can do.** After `pairing.confirmed`: ink pills for granted features, outline pills with a lock for the others, "Ask at the computer to turn more on". "Done" goes to Desktop. The pills come from `welcome.scopes`, never from an assumed default. With the default pairing (`DESIGN.md` 11.8) the ink pills include Typing and Launch, so the step adds one line under them: "This phone can type and start apps on the computer. Remove it there if the phone is lost: swayconnectd revoke" (mono pill for the command). The outline pills are then Close windows, Microphone and Terminals.
5. `FLAG_SECURE` on all pair screens (closes DESIGN 5.6 item 12).

## 12. Gestures

| Where | Gesture | Result |
|---|---|---|
| Page body (map or list) | horizontal swipe | next or previous workspace (drives the desktop in Drive mode) |
| Workspace card row | horizontal swipe | snaps; the centred card becomes the shown page (drives the desktop in Drive mode) |
| Workspace card | tap / long press | go to page (and drive) / workspace sheet |
| Map tile, window card | tap / long press | focus / Window sheet |
| Small tiles (24–48dp) | tap inside or near | focus the tile hit by containment, else the nearest edge within 24dp |
| Tab strip segment ≥ 48dp | tap | focus that child |
| Page body | vertical scroll | scrolls the page content |
| Now capsule | long press | menu: hide until next track |
| Slider | drag | volume (optimistic) |
| Hold buttons | press 400 or 600ms | confirm; early release cancels with `Reject` |
| Hardware volume keys | press | desktop volume when enabled |
| Hardware keyboard | Page Up / Page Down | previous / next workspace |

Not in v2: drag a tile onto a workspace card, pinch to overview, swipe to dismiss.

System back gesture: page content keeps the `systemGestures` inset as margin (5.5), and the landscape nav sits inside it (4.2).

## 13. Motion

`SwayMotion : MotionScheme` (custom object; `MotionScheme.expressive()` is internal in 1.4.0).

| Spec | Value | Used for |
|---|---|---|
| `defaultSpatialSpec` | `spring(dampingRatio = 0.8f, stiffness = 380f)` | nav pill width, focus node, tile bounds, pager and row scrolls |
| `fastSpatialSpec` | `spring(0.7f, 800f)` | press scale, chips |
| `slowSpatialSpec` | `spring(0.85f, 200f)` | sheets |
| `defaultEffectsSpec` | `spring(1f, 1600f)` | colour (card fill), alpha, list↔map crossfade |
| `fastEffectsSpec` | `spring(1f, 3800f)` | icon crossfades |
| `slowEffectsSpec` | `spring(1f, 500f)` | dimming of stale state |

| Moment | Motion | Duration (settle) |
|---|---|---|
| Nav selection | old pill shrinks, new pill grows; label fade in 120ms after start, out 90ms | ≈ 300ms |
| Pager ring | ring lerps x and width between the two cards by `currentPageOffsetFraction`; a stretch of `16dp × sin(π·|fraction|)` mid-swipe | follows finger |
| Focus fill (Drive) | old card's container colour fades out, the confirmed card's fades in (no slide) | ≈ 200ms |
| Focus in the map | shared focus node animates bounds | ≈ 300ms |
| Window opens / closes | grow from rect / collapse, neighbours reflow (`animateBounds`) | ≈ 350ms |
| Map ↔ list | `Crossfade` | ≈ 200ms |
| Press | scale 0.96 on tiles and cards | 100ms |
| Refusal | 4dp horizontal shake × 2 on the control | 240ms |
| Urgent appears | dot alpha pulse × 2 | 600ms |
| Stale state | dim to alpha 0.6 | `slowEffectsSpec` |
| Workspace removed | page and card shrink + fade | 150ms |

Reduce motion (system animator scale 0, or the app setting): every spatial spec becomes `snap()`, pulses are dropped, the ring jumps, and scrolls use `pagerState.scrollToPage` and `rowState.scrollToItem` instead of their animate variants (those take their own default springs, not `MotionScheme`).

Performance budget: 60 fps on a mid-range phone with 40 tiles. Transforms through `graphicsLayer` only. No blur. Icons decoded once into an `ImageBitmap` cache. One page beyond the viewport.

## 14. Haptics

Through `LocalHapticFeedback.current.performHapticFeedback`, behind a `Haptics` wrapper that reads the setting (Off / Subtle / Full; Subtle drops the tick types).

| Type | Fires on |
|---|---|
| `SegmentTick` | page settle; list↔map switch |
| `Confirm` | the server confirmed an action |
| `Reject` | a refusal; early release of a hold |
| `ToggleOn` / `ToggleOff` | Drive, mute, keep awake, sticky modifiers, a feature unlocking |
| `SegmentFrequentTick` | slider every 5 % |
| `GestureThresholdActivate` | slider at 0 and max; a hold completes |
| `LongPress` | Window sheet opens |

API 29 fallback: types the platform lacks map to `LongPress` (Confirm, Reject, GestureThresholdActivate) or `TextHandleMove` (ticks). The exact fallback per type is checked on the emulator in slice A1 (UNVERIFIED now). Haptics never carry meaning alone.

## 15. Accessibility

**Targets.** Every control ≥ 48dp, through size or `Modifier.minimumInteractiveComponentSize()`. Map tiles 24–48dp use the hit rule of 6.2 (WCAG 2.5.8 floor 24px) and each has its own semantics node. Nothing under 24dp takes a tap; it is reachable through "+N small windows" and the list.

**Semantics.**

- Workspace card: `Role.Tab`, `selected` = shown on the phone; content description "Workspace 3, code, 3 windows, focused on computer, needs attention". Custom action "Show on computer" (browse mode) and "Next workspace", "Previous workspace" on the row.
- Pager: only the current page is in the semantics tree. Neighbour pages (composed by `beyondViewportPageCount = 1`) get `Modifier.clearAndSetSemantics {}`, so TalkBack linear navigation never walks into the next page. Pages change through the row's custom actions and the card tabs. Accessibility scroll actions are not user-initiated, so they never drive the desktop (5.5).
- Map: a container node with `contentDescription` "Layout, horizontal split, 3 windows" and `isTraversalGroup = true`; no `collectionInfo` (a split tree is not a grid). Each tile, including each 24–48dp tile through its invisible child: `Role.Button`, `traversalIndex` in tree order, "Chrome, Docs, left half, focused on computer", custom actions Focus, Fullscreen, Float, Move to workspace…, Type here, Close (only those the phone may do).
- Window card: same text and actions.
- Pills always carry words. Colour never stands alone: focus = fill + word, urgent = dot with "!" + word, armed = shield, destructive = `error` colour + icon + divider.
- Sliders: `stateDescription` "40 percent, muted", 5 % steps.
- Hold buttons: TalkBack double tap opens a confirm dialog instead of the hold.

**TalkBack order.** Title → status line → context pill → round button → workspace cards → page header → map or list → focused-window card → capsule → nav. Sheets trap focus until closed.

**Announcements.** The connection word of the status line is a polite live region, and only transitions are announced: Ready, Offline, Locked, Screens off, Keep awake on, Keep awake ended. The one other polite live region is the refusal line of an area (the UX 16 text that comes with the `Reject` haptic), so a refusal is never told by the haptic alone (section 14). Countdown pills and "last seen" text sit in their own nodes with no live region. A settled page: "Workspace 4, term, 7 windows" (once per settle). While touch exploration is on, follow focus never moves the page (5.5); the chip appears instead. `focus_changed` while typing: assertive.

**Font scale (to 200 %).**

- Title row and workspace cards have a minimum height and no maximum; they grow to fit. Cards drop the name line above 1.5, keep number + count.
- Tiles fall to icon-only sooner (classes measured with text).
- Focused-window card buttons go icon-only with labels in semantics.
- Nav selected pill shows icon only when the measured label would overflow (4.2), at any scale.
- Segmented choices of four (the keep-awake duration) become a vertical radio list above 1.5.
- Above 1.8 a dismissible chip suggests the list view once.
- No text is cut without the full text in semantics.

**Contrast.** All text pairs of section 3.1 are ≥ 4.5:1, including `accentOnInk` on `primary` in both themes and `error` text on white and greige. Non-text marks (ring, `outline` borders, urgent dot, `error` borders) ≥ 3:1 against the surface they sit on. A JVM test in slice A1 computes every pair of 3.1 from the token values.

**Reduce motion** as in section 13.

## 16. Error and empty states (all areas)

| Situation | Text (one line, plain) | Haptic |
|---|---|---|
| `rate_limited` | "Too many actions. Wait a moment." | Reject |
| `read_only` | "The computer is set to look only." | Reject |
| `not_found` | "That window is gone." | Reject |
| `sway_unavailable` | "Sway is not running." | Reject |
| `timeout` | "No answer from the computer." (spinner turns into a retry glyph) | Reject |
| `backend_unavailable` | "This needs a service that is not running on the computer." | Reject |
| `spawn_failed` | "The computer could not start it." | Reject |
| `limit_reached` | "Available again in 9 min." | Reject |
| `launcher_closed` | "The launcher on the computer is closed." | Reject |
| `forbidden` | opens the Locked sheet for `detail.scope` | Reject |
| unknown error code | "The computer refused this." | Reject |
| Revoked | full screen: "This phone is no longer paired with worxbox." + Pair again | none |
| Wrong fingerprint | full screen: "This is not the computer you paired with." + Pair again; no auto-retry | none |

Empty states: "Nothing open on 5" (workspace), "Nothing playing" (media), "No app matches" (search), "No sound devices" (audio).

## 17. Compose implementation notes

Stack: material3 1.4.0, foundation / ui / animation 1.12.1 (BOM 2026.09.00). No new library. "Stable" below means no opt-in annotation in the bytecode check of `V2_RESEARCH.md` section 9. Each slice re-checks its APIs against the jars before use.

### 17.1 Theme (`ui/theme/`)

- `MaterialTheme(colorScheme = swayLight()/swayDark(), motionScheme = SwayMotion, shapes = SwayShapes, typography = SwayType, content)`. Stable overload.
- `swayLight()` = `lightColorScheme(primary = Ink, onPrimary = White, background = Greige, surface = White, surfaceContainerLowest = White, surfaceContainer = Badge, ..., tertiary = Lime, error = Coral, onError = Ink)`.
- `SwayTokens` data class through `staticCompositionLocalOf` for the nav and map tokens.
- Theme setting: System / Light / Dark. No dynamic colour.
- `LocalReduceMotion`: `Settings.Global.getFloat(contentResolver, ANIMATOR_DURATION_SCALE, 1f) == 0f || setting`.

### 17.2 Shell (`ui/shell/`)

- `Scaffold(contentWindowInsets = WindowInsets.safeDrawing)` with no `bottomBar`; the nav floats in a `Box` over the content with `Modifier.align(BottomCenter).navigationBarsPadding().padding(bottom = 12.dp)`. Content gets bottom padding = nav height + 24dp.
- Back: `BackHandler(enabled = sheet != null || area != Desktop)` (activity-compose 1.13.0). Nothing handles Back on Desktop with no sheet, so the system's predictive back-to-home runs; the manifest sets `android:enableOnBackInvokedCallback="true"`.
- `PillNavBar`: `Surface(shape = CircleShape, color = tokens.navContainer, shadowElevation = 8.dp)` → `Row(Modifier.selectableGroup())`. Label fit: `rememberTextMeasurer().measure(label, labelLarge)` against the bar width minus the other items. Item: `Modifier.selectable(selected, role = Role.Tab, onClick)` + `Modifier.animateContentSize(motion.defaultSpatialSpec())`; inner pill colour with `animateColorAsState`; label in `AnimatedVisibility(selected, enter = fadeIn(tween(120, delayMillis = 60)) + expandHorizontally(), exit = fadeOut(tween(90)) + shrinkHorizontally())`. All stable.
- Not used: `NavigationBar`, `ShortNavigationBar` (their look does not match the reference).
- Vertical variant for landscape: same item composable in a `Column`, padded by `WindowInsets.systemGestures.only(Start)`.

### 17.3 Workspace cards and pager (`ui/desktop/`)

- Layout: `Column { TitleRow(); WorkspaceRow(); HorizontalPager(Modifier.weight(1f)) { page -> LazyColumn { … } } }`. One vertical scroll owner per page; nothing wraps the pager in `verticalScroll`.
- `val pagerState = rememberPagerState { pages.size }`; `HorizontalPager(state = pagerState, key = { pages[it].id }, beyondViewportPageCount = 1, flingBehavior = PagerDefaults.flingBehavior(pagerState))`. Stable. Pages other than `currentPage` wrap their content in `Modifier.clearAndSetSemantics {}`.
- Cards: `LazyRow(state = rowState, flingBehavior = rememberSnapFlingBehavior(rowState, SnapPosition.Center), contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = spacedBy(8.dp), modifier = Modifier.selectableGroup())`, items keyed by workspace id, each `Modifier.selectable(selected = page == shown, role = Role.Tab)`. Card container colour: `animateColorAsState(if (focused) primary else surface, motion.defaultEffectsSpec())`; content colour follows. Stable. Not `PrimaryScrollableTabRow`: its indicator and tab layout fight the card look.
- Ring: drawn in `Modifier.drawWithContent` on the `LazyRow`. It reads `pagerState.currentPage`, `currentPageOffsetFraction` and `rowState.layoutInfo.visibleItemsInfo` in the draw phase (no recomposition per frame) and lerps left and width between the two cards.
- `DriveController` (pure, in `ui/state/`): inputs are `userDrag` (from `pagerState.interactionSource` `DragInteraction.Start`, or a row drag), `cardTap(id)`, `settled(id)` with the id read from `pages` at settle time, and `focusedId`. Output: at most one `workspace.focus` per user-initiated settle after 250ms, never when the id equals `focusedId`. Programmatic scrolls (follow focus, re-sync, keep in view) and accessibility scroll actions never set `userDrag`.
- Row → pager: when a user drag of the row settles, `pagerState.scrollToPage(centredIndex)` and `DriveController.userDrag()`.
- Keep in view: `LaunchedEffect(pagerState.targetPage) { if (reduceMotion) rowState.scrollToItem(i, centreOffset) else rowState.animateScrollToItem(i, centreOffset) }`.
- Follow focus: `LaunchedEffect(focusedWorkspaceId) { if (!touchExploration && !pagerState.isScrollInProgress && state.pending == null && now - lastTouch > 1500) { if (reduceMotion) pagerState.scrollToPage(i) else pagerState.animateScrollToPage(i, animationSpec = motion.defaultSpatialSpec()) } else vm.showMovedChip(id) }`. `lastTouch` from `Modifier.pointerInput { awaitEachGesture { awaitFirstDown(pass = PointerEventPass.Initial) ... } }` without consuming. `touchExploration` from the platform `AccessibilityManager.isTouchExplorationEnabled` (a system service, not a View widget), observed with its listener.
- Re-sync: `LaunchedEffect(pages.map { it.id }) { pagerState.scrollToPage(indexOf(selectedId)) }`.
- Chip timeouts: `LocalAccessibilityManager.current?.calculateRecommendedTimeoutMillis(5000, containsIcons = false, containsText = true, containsControls = true) ?: 5000`.

### 17.4 Map (`ui/map/`)

- `MapGeometry` (pure Kotlin, no Compose import): basis points → dp rects, size classes, same-app indexes, nearest-tile hit test, density ratio for the dense chip. JVM tests.
- `LayoutMap`: a custom `Layout(content)`. Children carry their rect through a `ParentDataModifierNode` (`Modifier.tileRect(rect)`). The measure policy measures each child with `Constraints.fixed(w, h)` and places floats last with `placeRelativeWithLayer(x, y, zIndex = 1f)`.
- Tiles ≥ 48dp: `Surface(onClick, shape = shapes.extraSmall, color = wash, border = …)` keyed with `key(windowId)`.
- Tiles < 48dp: one `Canvas` node draws all of them; `Modifier.pointerInput { detectTapGestures { offset -> geometry.hit(offset, 24.dp)?.let(onTap) } }` on the map (`hit` = containment, then nearest edge). Each 24–48dp tile also gets an invisible placed child of its rect that draws nothing and carries `semantics(mergeDescendants = true) { role = Role.Button; contentDescription; customActions }`.
- Motion: `LookaheadScope { … Modifier.animateBounds(lookaheadScope = this, boundsTransform = BoundsTransform { _, _ -> spring(0.8f, Spring.StiffnessMediumLow) }) }` when tile count ≤ 25. Stable per bytecode check; re-check the exact parameter names in the 1.12.1 jar.
- Semantics: `Modifier.semantics { isTraversalGroup = true; contentDescription = … }` on the map, no `collectionInfo`; `Modifier.semantics(mergeDescendants = true) { role = Role.Button; traversalIndex = treeOrder; stateDescription = …; customActions = listOf(CustomAccessibilityAction("Fullscreen") { … }, …) }` on tiles.
- Long press on a tile: `Modifier.combinedClickable(onClick, onLongClick)` (foundation; was `@ExperimentalFoundationApi` in older versions, check 1.12.1) instead of `Surface(onClick)` for tiles that need both.

### 17.5 List and the switch

- `Crossfade(viewMode, animationSpec = motion.defaultEffectsSpec())` inside the page's `LazyColumn` item for the map slot; in list view the window cards are the column's own items. The shared-element morph is later.
- `LazyColumn { item { PageHeader() }; items(windows, key = { it.id }) { WindowCard(Modifier.animateItem()) }; item { DashedCard() }; item { FocusedWindowCard() } }`.

### 17.6 Icons (`ui/common/AppIcon.kt`, `data/IconCache.kt`)

- `IconCache`: memory `LruCache<String, ImageBitmap>` (android.util, platform) + disk `cacheDir/icons/<hash>.png`, 4 MiB cap, oldest-first eviction.
- Batching: missing hashes go into a set; at most one `icon.get` per 500ms with ≤ 8 hashes; after `rate_limited` wait `detail.retry_after_ms`.
- Decode: `MessageDigest.getInstance("SHA-256")` check → `BitmapFactory.Options().apply { inJustDecodeBounds = true }` → refuse > 256 px or > 65536 bytes → decode → `asImageBitmap()`.
- `AppIcon(appId, hash, size)`: monogram (first grapheme on the wash of `appId`) until the bitmap arrives, then a 120ms crossfade at the same size.

### 17.7 Sheets and dialogs

- `ModalBottomSheet` + `rememberModalBottomSheetState(skipPartiallyExpanded = true)`: **`@ExperimentalMaterial3Api`**, opted in per file (already done in v1). One sheet at a time: `UiState.sheet: Sheet?` sealed (WindowActions, MoveTo, SmallWindows, TabGroup, Scratchpad, KeepAwakeDuration, Locked(scope), Computer, Confirm).
- Confirm dialogs: `AlertDialog` (stable).

### 17.8 Controls

- Sliders: `Slider(value, onValueChange, valueRange, onValueChangeFinished)`, the stable overload. Not the `SliderState` overload (**experimental**).
- Send throttle: a `Channel<Int>(CONFLATED)` consumer that sends, then `delay(100)`.
- Hold button: `Modifier.pointerInput { awaitEachGesture { awaitFirstDown(); … } }` + `Animatable` progress drawn with `drawArc`.
- Search field: `TextField(state = rememberTextFieldState(), lineLimits = TextFieldLineLimits.SingleLine, keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search, showKeyboardOnFocus = true))`. Not `SearchBar` (**experimental**).
- Grid: `LazyVerticalGrid(GridCells.Adaptive(80.dp))`, section headers with `span = { GridItemSpan(maxLineSpan) }`.
- Keys composer: `InterceptPlatformTextInput(interceptor = NoLearningInterceptor) { TextField(state = rememberTextFieldState(), keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send)) }`; Private: the same wrapper around `BasicSecureTextField(state = rememberTextFieldState(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password))`. `NoLearningInterceptor` is a `PlatformTextInputInterceptor` whose `interceptStartInputMethod(request, nextHandler)` passes on a `PlatformTextInputMethodRequest` that calls the original `createInputConnection(outAttrs)` and then sets `outAttrs.imeOptions = outAttrs.imeOptions or IME_FLAG_NO_PERSONALIZED_LEARNING or IME_FLAG_NO_EXTRACT_UI`. No opt-in annotation in ui 1.12.1 (bytecode check 2026-11-13). `Modifier.imePadding()` on the Keys column.
- `FLAG_SECURE`: `DisposableEffect(Unit) { window.addFlags(FLAG_SECURE); onDispose { window.clearFlags(FLAG_SECURE) } }` on Pair and Keys.
- Volume keys: `MainActivity.dispatchKeyEvent` handles `KEYCODE_VOLUME_UP/DOWN/MUTE` while the Audio area is shown and returns `true`. (An Activity override, not a View widget.)
- Stale state: `Modifier.graphicsLayer { alpha = animatedAlpha }` (0.6 when stale). No colour-matrix effect, so no API split.

### 17.9 State (`ui/state/`)

- `UiState` gains: `area`, `sheet`, `viewMode`, `drive`, `followFocusChip`, `session`, `audio`, `media`, `apps`, `keys` (target, private, sticky modifiers), `lastSeenAt`, `pendingLaunch`, `knownScopes`, `knownOps`.
- Pure reducers per area (`DesktopReducer`, `SessionReducer`, `AudioReducer`, …). `FeatureAvailability(op, scopes, ops, knownOps, readOnly)` → `Available | ReadOnly | Locked(scope) | Unsupported`, exactly the table of `PROTOCOL.md` 17.3. The nav, buttons and tiles render from it.
- `DesktopCache`: write the desktop (titles only with `state.titles`) to `filesDir/last_desktop.json` 2 s after each change on `Dispatchers.IO`; read before connecting.

### 17.10 Experimental APIs used (complete list)

`ModalBottomSheet`, `rememberModalBottomSheetState` (`@ExperimentalMaterial3Api`). Nothing else. Any other experimental API needs a note in the slice. Checked as stable in the 1.12.1 jars: `InterceptPlatformTextInput`, `PlatformTextInputInterceptor`, `rememberSnapFlingBehavior(LazyListState, SnapPosition)`, `AccessibilityManager.calculateRecommendedTimeoutMillis`.

## 18. Two-tap audit (acceptance)

From any area, with the scopes granted. A hold or a confirm dialog is protection, not navigation.

| Action | Path | Taps |
|---|---|---|
| Focus a window | Desktop → tile or card | 2 |
| Switch the desktop workspace | Desktop → swipe (Drive) or card | 1–2 |
| Fullscreen / float the focused window | Desktop → card button | 2 |
| Move a window to a workspace | long press → number pill | 2 + long press |
| Close a window | Desktop → ✕ + hold | 2 |
| Volume of the default output | Audio → slider, or volume keys | 2 / 0 |
| Mute the mic | More → Mic, or Audio → Mute | 2 |
| Unmute the mic (`audio.capture`) | More → Mic + hold | 2 |
| Play / pause | capsule | 1 |
| Keep awake on / off | More → tile, or status pill → sheet | 2 |
| Lock the computer | More → round button | 2 |
| Launch an app | Launch → recent icon | 2 |
| Open fuzzel | Launch → round button | 2 |
| Type into the focused window | Keys → type → Send | 2 + typing |
| Esc / Ctrl+C | Keys → chip | 2 |

Checked in slice QA2 on the emulator against the fake sway.
