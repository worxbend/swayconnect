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
