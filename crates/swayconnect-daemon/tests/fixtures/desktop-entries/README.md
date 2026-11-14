# Desktop entry fixtures

Input for the unit tests of `src/apps/index.rs`. Two data dirs: `high` comes first in the
search order, so its `firefox` and `hidden-here` shadow the ones in `low`. Every entry is
invented; no `Exec` program exists on purpose.

| Entry | Case |
|---|---|
| `firefox`, `mpv`, `gparted`, `htop`, `only-sway`, `sub/tool` (id `sub-tool`) | listed |
| `quoted` | quoted `Exec` with an escaped quote, `--file=%f` and `%%` |
| `htop` | `Terminal=true`, so risky |
| `hidden-here` | `Hidden=true` in `high` hides the visible copy in `low` |
| `nodisplay`, `only-kde`, `not-sway`, `link` | `NoDisplay`, `OnlyShowIn`, `NotShowIn`, `Type=Link` |
| `geo-handler` | URL-only handler: `%u` and only `x-scheme-handler/*` types |
| `tryexec-missing`, `tryexec-path` | `TryExec` absolute and missing; relative, found only when the test puts it on the path |
| `bad-exec`, `no-name`, `no-exec`, `bad id` | unquoted `;`, no `Name`, no `Exec`, an id with a space |
| `denied` | hidden by `[apps] deny` in the test config |
