# Sway reply fixtures

Input for the unit tests of `src/model.rs`.

The node shapes (wrapper containers, the `__i3` output with `__i3_scratch`, workspace
`fullscreen_mode` 1) follow read-only `swaymsg -t get_tree`, `get_workspaces` and
`get_version` output of sway 1.12. Every title, application id, pid and id is invented.
Only the fields the daemon reads are kept, plus `rect` and `marks` to show they are ignored.

| File | Content |
|---|---|
| `desktop_tree.json`, `desktop_workspaces.json` | scratchpad, split container, Xwayland window, floating, fullscreen, workspace without a number |
| `desktop_expected.json` | the `desktop` state object those two give with version `1.12` |
| `hostile_tree.json`, `hostile_workspaces.json` | ESC, U+009B and U+202E in a title, an `app_id`, a workspace name and an output name |
