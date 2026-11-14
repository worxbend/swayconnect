# Conformance vectors, protocol version 1

Machine-readable form of `docs/PROTOCOL.md` section 15 (and the URI examples of section 4).
Every client replays these files in its tests. If a file and the spec differ, that is a bug.

A change here is a wire change: update `docs/PROTOCOL.md`, these files, the Rust protocol crate
and every client in one change.

## Formats

All files are JSON, UTF-8. Binary values are lowercase hex unless the name ends in `_b64`
(standard base64, with padding) or `_b64url` (base64url, no padding).

| File | Shape |
|---|---|
| `fingerprint.json` | `[{"name", "cert_der_b64", "key_pkcs8_b64", "wrong_key_pkcs8_b64", "fp_hex", "fp_b64url"}]` |
| `proofs.json` | `{"inputs": {"s","fp","device_id","pairing":{"server_nonce","client_nonce"},"later":{...}}, "expect": {"pair_proof","k","confirm_mac","confirm_code","server_proof_pairing","client_proof_later","server_proof_later"}}` |
| `pairing-uri.json` | `{"valid": [{"uri","v","host","port","fp_hex","secret_hex","name"}], "invalid": [{"uri","reason"}]}`; `name` is `null` when absent |
| `frames.json` | `[{"name","limit","input_hex": [chunks],"expect": "bad" \| "too_large" \| [payload strings]}]` |
| `requests.json` | `[{"name","scopes","read_only","sway_online","state_ref","actions_left","payload","expect": "ok" \| "ignored" \| {"error": code, "detail"?: object} \| {"bye": code}}]`. `state_ref` is `"t3-rev7"` or `"offline"`. `actions_left` is the number of tokens in the device's action bucket. |
| `handshake.json` | `[{"name","after": "t1" \| "t2","window_open","payload","expect": {"reject": code}}]` |
| `transcripts/*.json` | `{"name","preconditions","frames": [{"from": "S" \| "C","payload": string}]}`; the length prefix is derived from the payload |

## Reading rules

1. `payload` is always a string. It holds the exact frame payload: encode it as UTF-8 to get the
   bytes on the wire. The 4-byte length prefix is not stored.
2. `device_id` in `proofs.json` is the id string itself, not hex. The proofs use its ASCII bytes.
3. `confirm_code` is a string of six digits.
4. `frames.json`: each entry of `input_hex` is one read. Feed them to the decoder in order.
   `expect` lists the payloads of the frames the decoder must give after the last read.
   For `too_large` the input ends after the length: the decoder must fail without the payload.
5. `fingerprint.json`: `key_pkcs8_b64` is the private key of the certificate.
   `wrong_key_pkcs8_b64` is a P-256 key that does not belong to it (spec 15.13).
   Both are public test material. Never use them outside tests.
6. `requests.json`: one frame from the client in a `READY` session with the given preconditions.
   - `"ok"`: a `res` with `ok: true`. `"ignored"`: no answer, the connection stays open.
   - `{"error": code}`: a `res` with that `error.code`. When `detail` is given, each member of it
     must equal the member of `error.detail`. One exception: `retry_after_ms` is given as
     `{"min", "max"}` and the value must be in that range, both ends included.
     When `detail` is not given, `error.detail` is not checked.
   - `{"bye": code}`: a `bye` with that code, then the server closes.
   - `state_ref` `"t3-rev7"`: the `desktop` state in the `res` of `transcripts/t3-session.json`
     (workspaces 4 and 9, windows 5 and 12). `"offline"`: the state of
     `transcripts/t6-sway-offline.json`.
   - `actions_left`: 5 is a full bucket, 0 is an empty one.
7. `handshake.json`: `payload` is the first client frame after the `hello` of transcript `after`.
   `window_open` says whether a pairing window with the `s` of `proofs.json` is open.
   Some payloads hold JSON escapes such as `\u001b`. They are part of the payload text.
8. `transcripts/*.json`: `preconditions` is the text of the spec, for people.
   A decoder must accept every `payload`. An encoder passes when its output decodes to an equal
   JSON value; member order and whitespace are free.
   `t4`, `t5` and `t6` start in the middle of a connection, as the spec prints them.
   A server under test draws its own nonces, ids and secrets and counts `rev` itself. Compare
   its frames with the transcript without `nonce`, `device_id`, `server_proof`, `confirm_code`,
   `rev` and `server.version`. Every other member must be equal, the `message` texts too.

## Extensions (`docs/PROTOCOL.md` section 17)

The files under `ext/` cover the additive extensions of section 17. They follow every rule above.
Protocol and ALPN stay version 1. The files above stay byte-identical.

| File | Milestone | Shape |
|---|---|---|
| `ext/desktop.json` | M1 | `[{"name","payload","expect": "ok" \| "bad" \| {"normalized": payload}}]` |
| `ext/icons.json` | M1 | `{"store": [{"hash","png_b64url"}], "requests": [...], "results": [...]}` |
| `ext/welcome.json` | M1 | `{"op_table", "frames", "availability", "server_ops", "bye"}` |
| `ext/requests.json` | M2a | `{"config", "requests": [...]}` |
| `ext/session.json` | M2a | `{"states": [...], "results": [...]}` |
| `ext/audio.json` | M2b | `{"config", "server_states", "states", "requests", "results", "errors"}` |
| `ext/media.json` | M2b | `{"server_states", "states", "server", "requests", "results"}` |
| `ext/apps.json` | M2b | `{"server_states", "states", "cut", "requests", "results"}` |
| `ext/input.json` | M2b | `{"requests", "results", "errors"}` |

### `ext/desktop.json`

Each `payload` is a `state.changed` event of topic `desktop` (17.4).

- `"ok"`: the decoder accepts it. Re-encoding the decoded state gives an equal JSON value.
- `"bad"`: the decoder rejects the state (wrong type, missing required member, `null` where the
  type has no `or null`, wrong array length, an empty `c`, a number with a fraction or exponent,
  a `sint` out of range).
- `{"normalized": payload}`: the decoder accepts it, then applies the client rules of 17.4.2 and
  17.4.3: unknown `transform` is `normal`, unknown `k` is `h`, absent or out-of-range `a` is 0,
  a tile whose `w` is not in `windows` is skipped, each `r` value above 10000 is 10000, a tile with
  member `w` is a window tile (its `k` and `c` are ignored), unknown members are ignored.
  The result must equal the decoded `normalized` payload, which is already normal.
- Case `01` is the `desktop` example of 17.18 with a real icon hash (`ext/icons.json` store entry 1)
  in place of the placeholder `q83vEjRWeJCrze8SNFZ4kA`.

### `ext/icons.json`

- `store`: the PNGs the server under test holds, and no others. `hash` is base64url of the first
  16 bytes of `SHA-256(png)` (17.5 rule 1).
- `requests`: `[{"name","scopes","read_only","sway_online","icons_left","payload","expect"}]`.
  One client frame in a `READY` session, as `requests.json`. `icons_left` is the number of tokens
  in the device's `icons` bucket (17.14): 4 is full, 0 is empty. `expect` is
  `{"result": {"icons": [hash], "missing": [hash]}}`, where each listed icon's `png` decodes to the
  bytes of that `store` entry, or `{"error": code, "detail"?}` read as in `requests.json` rule 6.
  A string that is not a canonical 22-character base64url value is `bad_request`; a well-formed hash
  the server did not issue is listed in `missing`.
- `results`: `[{"name","requested","payload","expect"}]`. `payload` is a `res` the client gets for
  an `icon.get` of the `requested` hashes. `expect` is `"bad"` (the client rejects the result) or
  `{"accepted": [hash], "dropped": [{"hash","reason"}], "missing": [hash]}`. The client keeps an icon
  only when `SHA-256(png)` matches the hash and the PNG bounds, read before decoding, are at most
  256 x 256 (17.5 rule 3). `reason` is `hash_mismatch`, `not_png` or `bounds`; it is informative.
  Case `06` holds 8 PNGs of exactly 65536 bytes, 128 x 128 pixels; its frame is 699466 bytes and
  fits `max_frame_s2c`.
- The PNGs are deterministic: pixel bytes are the SHA-256 counter stream of a fixed seed, the big
  ones are stored without compression and padded to 65536 bytes with a `tEXt` chunk.

### `ext/welcome.json`

- `op_table`: per operation, the scope that grants it (`null` for `ping`) and whether it is an
  action (refused in read-only mode). From 7.2, 17.2 and 17.6 to 17.13.
- `frames`: `[{"name","payload","expect"}]` with a `welcome` payload. `expect` is `"bad"` or
  `{"fields": {"scopes","ops","known_scopes","known_ops","read_only"}}`, the decoded values;
  `null` there means the member is absent. Unknown names in these lists are kept by the decoder and
  ignored by the client (13.2 rule 6). Case `01` is the `welcome` example of 17.18 with the
  `server_proof` of `transcripts/t2-auth-ok.json` in place of `"..."`.
- `availability`: `[{"name","op","scopes","ops","known_scopes","known_ops","read_only","expect","scope"?}]`.
  The client rule of 17.3 (`FeatureAvailability`). `expect` is `available`, `read_only`, `locked`
  (then `scope` names the scope to show) or `unsupported`. `known_ops` `null` means absent.
- `server_ops`: `[{"name","known_ops","scopes","read_only","expect_ops"}]`. A server lists in
  `welcome.ops` each op of `known_ops` whose scope the device holds (or that needs none), less the
  actions when `read_only` is true. Compare as sets.
- `bye`: `[{"name","payload","expect"}]`. `expect` is the client reaction: `reconnect_now`
  (`scopes_changed`, 17.15), `reconnect_backoff` (`shutdown`, and any unknown code) or `forget`
  (`revoked`).

### `ext/requests.json`

Window ops, `window.close`, `session.lock`, `keep_awake.set` and `state.subscribe` of topic `session`
(17.6 to 17.8), read as `requests.json` (rule 6), with these additions:

- `config`: the `[keep_awake]` values of the server under test (`docs/DESIGN.md` 11.9 defaults).
- `state_ref` also takes `"ext-desktop-03"`: the state of `ext/desktop.json` case `03` (workspaces
  30, 31 and 32, which has no number; windows 41 to 48; scratchpad windows 50 and 51; output
  `HDMI-A-1` is active with `power: false`).
- `session`: what the server holds when the frame arrives. `locked` is its lock belief (`true`,
  `false` or `null`), `lock_command` whether the owner configured one, `lock_running` whether the
  lock command it started last still runs, `spawn` (`"ok"` or `"fails"`) whether a child the
  request makes it start (lock command, inhibitor) starts. `keep_awake` gives `active`,
  `remaining_ms`, `on_time_s` (the on-time count of 17.8 rule 5 at that moment) and
  `cooldown_remaining_ms` (not `null` while `limit_reached` applies).
- `expect` may also be `{"result": object}`: a `res` with `ok: true` whose `result` has each listed
  member with an equal value. A member given as `{"min", "max"}` is a number in that range, both
  ends included; this holds for `detail` too. Times are exact with a paused clock; the range allows
  1 s of a live one.
- `sway` (present when `sway_online` is true): the exact sway commands the reference server sends,
  in order; `[]` means none. They follow `docs/DESIGN.md` 11.4. A server that renders commands
  differently checks only that the list is empty or not.
- Case `108` expects `unknown_op` because the server has no lock command (17.6 rule 1), not because
  the name is unknown. Cases `64` and `114` check 10.2 step 5 before step 6 and before the result.
- Cases `70` and `101` hold the `keep_awake.set` and `session.lock` requests of 17.18.

### `ext/session.json`

- `states`: `[{"name","payload","expect"}]`, a `state.changed` event of topic `session` (17.8), read as
  `ext/desktop.json`. The client rules: an unknown `holder` string (`keep_awake`, `input`,
  `launcher`) is `other_device`; unknown members are ignored. Case `01` is the 17.18 example.
- `results`: `[{"name","op","payload","expect"}]`, a `res` the client gets for `op`. `expect` is
  `"ok"` (the client accepts the result) or `"bad"` (it rejects it: a result member missing, of
  the wrong type, or `null` where the type has no `or null`). Cases `01` and `04` are the 17.18
  examples.

### M2b files: shared rules

`ext/audio.json`, `ext/media.json`, `ext/apps.json` and `ext/input.json` cover 17.9 to 17.13.

- `states`: a `state.changed` event of the file's topic, read as `ext/desktop.json` (`"ok"`, `"bad"`,
  `{"normalized"}`). `"bad"` also covers an `int` outside the range its table states (2.2: an `int`
  is 0 to 2^53 - 1 unless a narrower range is stated), for example `volume_pct` 1001 or
  `max_volume_pct` 151. A client does not check string contents: the server cleans them (17.1
  rule 7), and a case with U+FFFD in a string is `"ok"`.
- `results`: `[{"name","op","payload","expect"}]`, read as `ext/session.json` `results`.
- `errors`: `[{"name","op","payload","expect"}]`, an error `res` the client decodes. `expect` lists
  `code` and the `detail` members the client reads; a member not listed is not checked.
- `requests`: read as `ext/requests.json` (rule 6 and its additions), with these changes:
  - `buckets` replaces `actions_left`: tokens left per rate class of 17.14 (`actions`,
    `continuous`, `launch`, `input`, `input_text`). A full bucket holds the class burst. A
    `rate_limited` `retry_after_ms` range ends at one refill interval of the op's class.
  - `server_states` (audio, media, apps) holds what the server has when the frame arrives, by name;
    the case names it in `audio`, `media` or `apps`. `state` is the topic state the server
    publishes; `names` (audio) maps each device id to its pactl name and `buses` (media) each
    player id to its MPRIS bus name. Both are server-internal and never on the wire.
  - `backend` (audio, media): `"ok"`, `"fails"` (the program or player is missing or reports
    failure: `backend_unavailable`) or `"hangs"` (no answer in time: `timeout`). `spawn` (apps):
    `"ok"` or `"fails"` (`spawn_failed`).
  - `state.subscribe` cases check the topic scope of 17.2 (`audio`, `media`, `launch`) after the
    op scope `state.read` of 7.2. Subscribing is not an action: read-only mode and empty buckets
    do not refuse it.
  - Every case has `sway_online: true`. What an M2b op does while sway is offline is not covered.

### `ext/audio.json`

- `config`: `[audio] max_volume_pct` of the server under test.
- `pactl`: the argv the reference server runs after the `pactl` program path, in order (`docs/DESIGN.md`
  11.6), `[]` when it runs nothing. A server that talks to the sound server differently checks only
  that the list is empty or not. `notice`: whether a desktop notice is shown (17.9: unmute of a
  source).
- Device ids 2 and 4 are sources. An unknown id is `not_found` before the capture rule (17.16 step
  8a after step 8). Cases `01`, `02` and `33` hold the requests of 17.18 with their ids; `results`
  `01` and `errors` hold its responses.

### `ext/media.json`

- `mpris`: the `[bus name, method]` calls the reference server makes, in order; `[]` for none.
- `unsupported_target` follows the player's `can_*` flags: `media.next` needs `can_next`,
  `media.previous` needs `can_previous`, `media.play_pause` is refused when the player allows
  neither play nor pause.
- `server`: `[{"name","scopes","players","expect"}]`. `players` is what the server's media backend
  reports, in its order: `bus_name`, `identity`, `desktop_entry`, MPRIS `status` (`Playing`,
  `Paused`, `Stopped`), `title`, `artist`, `album` (string or `null`), `can_play`, `can_pause`,
  `can_go_next`, `can_go_previous`, and `last_playing` (a counter of when it last played, higher is
  later, `null` never). `expect` is the `media` state the server publishes for a device with
  `scopes`: `players` without `id` and `icon` (server-issued), `active` (index of the
  `active_player_id` player in `expect.players`, or `null`) and `truncated`. Rules: the
  `playerctld` name is skipped; `name` comes from `identity`, `app_id` from `desktop_entry`; each
  control or format character (2.4) becomes U+FFFD before a string is cut to its length in Unicode
  scalar values, so U+200D in a title is replaced too; `title`, `artist` and `album` are `null`
  without `state.titles`; the active player is the one with the highest `last_playing`; at 17
  players and more, players are dropped from the end, never the active one.
- Case `server` `01` is the `album` with U+202E and ESC.

### `ext/apps.json`

- `started`: what the request makes the server start, `{"app": entry_id}`, `{"launcher": true}` or
  `null`. A case with `spawn: "fails"` names the attempt. `sway`: the exact sway commands before the
  start (`app.launch` with `number`), as in `ext/requests.json`. `launcher`: the launcher the server
  runs when the frame arrives (`open`, `holder`); `launcher_after` (`launcher.open` cases): the same
  after the request.
- A request `entry_id` that is not 1 to 255 bytes of `A-Z a-z 0-9 . _ -` is `bad_request` (step 6);
  a well-formed id that is not in the `apps` state is `not_found` (step 8), before the
  `launch.risky` check (step 8b). The match is exact: no case folding, no `.desktop` suffix.
- `cut`: `[{"name","stage","input","input_bytes","expect","expect_bytes"}]`, the cut rule of 17.11.
  `input` is the `apps` state a server would build before any cut, as a compact JSON string;
  `input_bytes` is its UTF-8 length, the size 17.1 rule 9 measures (compact JSON, UTF-8 not escaped).
  `expect` is the state the server publishes, or `null` when it publishes `input` unchanged.
  `stage` names the last step the cut needed: `none`, `count` (more than 512 apps), `keywords`,
  `generic_name` or `apps`. Order: drop apps beyond 512; then, while the state is above 262144
  bytes, set every `keywords` to `[]`, then every `generic_name` to `null`, then drop apps from the
  end one at a time. Ids of dropped apps leave `recent`, which keeps its order. A cut state carries
  `truncated: true`, counted in its size. Case `04` is exactly 262144 bytes and is not cut; case
  `05` is one byte more.

### `ext/input.json`

- `state_ref` is `"ext-desktop-03"` (focused window 42) or `"ext-desktop-01"` (focused window 29;
  the 17.18 example state). Cases `001`, `100` and `101` hold the requests of 17.18 with their ids;
  `errors` `01` to `03` hold its responses.
- `guard`: what the checks of 17.13 rule 2 see before each part. `locked` is `true`, `false` or
  `null` (unknown). `grabber` is `"none"`, `"launcher"` (the only keyboard grabber running is the
  launcher the server started) or `"other"`. `focus` is the focused window id a synchronous sway
  read returns, or `null` for none. `launcher` is the launcher the server runs: `open`, `holder`
  and `focus_window_id` (the window focused when `launcher.open` ran). `locked`, `grabber`,
  `focus` and `launcher.open` may be a list: entry `i` is what the check before part `i` sees, and
  the last entry holds for later parts.
- `delivered`: for `input.text`, the parts the server hands to the keyboard, in order; for
  `input.key`, the number of key presses. A part is at most 16 characters; a character is one
  Unicode scalar value, so a part may end inside an emoji sequence. `delivered_chars` counts scalar
  values.
- Text rule: 1 to 1024 bytes of UTF-8, no control or format character (2.4) except U+200C and
  U+200D. One bad character refuses the whole request with `bad_request` before anything is
  typed.
