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
