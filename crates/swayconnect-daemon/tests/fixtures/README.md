# Sway reply fixtures

Input for the unit tests of `src/model.rs`, `src/layout.rs` and `src/outputs.rs`.

The node shapes (wrapper containers, the `__i3` output with `__i3_scratch`, workspace
`fullscreen_mode` 1) follow read-only `swaymsg -t get_tree`, `get_workspaces` and
`get_version` output of sway 1.12. In the `desktop_*` and `hostile_*` files every title,
application id, pid and id is invented.
Only the fields the daemon reads are kept, plus `marks` to show it is ignored. `desktop_tree.json`
has no workspace rect, so it also shows `rect` and `layout` as `null`.

| File | Content |
|---|---|
| `desktop_tree.json`, `desktop_workspaces.json` | scratchpad, split container, Xwayland window, floating, fullscreen, workspace without a number |
| `desktop_expected.json` | the `desktop` state object those two give with version `1.12`, with the v2 members of `docs/PROTOCOL.md` 17.4 |
| `hostile_tree.json`, `hostile_workspaces.json` | ESC, U+009B and U+202E in a title, an `app_id`, a workspace name and an output name |

## `layout/`

From a read-only `swaymsg -t get_tree` and `get_workspaces` of the owner's sway 1.12 on
2026-11-13. A script kept only the fields `swayconnect_sway::Node` reads and replaced every
window title with `Title <id>`, every pid with `1000 + id` and every X11 window id with
`4194304 + id`. Node ids, rects, layouts, focus lists, app ids and the monitor make and model
are the live values.

| File | Content |
|---|---|
| `live_tree.json`, `live_workspaces.json` | the whole live desktop: one output `DP-1` at scale 1.25, four workspaces with gaps, deep one-child wrapper containers, an Xwayland window and a floating one. No tabbed, stacked, fullscreen or scratchpad case: the live desktop had none (the testkit's `fixtures::layout_tree` covers those). |
| `example_tree.json`, `example_workspaces.json` | live workspace 20 alone, put back into the state of the `desktop` example of `docs/PROTOCOL.md` 17.18: shown as workspace `3` and focused, the 1Password window and its container removed (so the right column is window 29 alone), the floating window moved to the example's place, the example's titles. |
| `example_expected.json` | the state those two give, with an icon for `google-chrome` only |

`example_expected.json` differs from the 17.18 example in two members, both on purpose: `model`
is `"Dell Inc. DELL P3223QE"` (sway's make and model joined; the example's `"Dell P3223QE"`
cannot be derived from them), and `layouts_cut` is present as `false` (the example leaves it out).
