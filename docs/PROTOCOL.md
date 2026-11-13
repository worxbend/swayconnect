# swayconnect protocol, version 1

Status: draft for stage 1. Normative.
Protocol version: `1`. Document revision: 2026-11-13, after the security, implementability and scope review.

This file is the contract between the daemon (`swayconnectd`) and every client.
A client author needs this file and the vector files in `protocol/vectors/v1/`. Nothing else.

The key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are used as in RFC 2119 and RFC 8174.

Contents:

1. Overview
2. Terms and data types
3. Transport binding `tls-frames`
4. Pairing URI
5. Handshake: hello, pairing, authentication
6. Session messages
7. Operations
8. Events and state sync
9. Topic `desktop`
10. Errors
11. Keepalive, timeouts, close
12. Ordering guarantees
13. Versioning and forward compatibility
14. Limits (all numbers in one place)
15. Conformance vectors
16. Client implementation checklist
17. Version 1 additive extensions (v2 features). Draft, optional capabilities; see 17.1.

---

## 1. Overview

1. The daemon runs on the desktop. It talks to sway over the local sway IPC socket.
2. A client connects over TCP, then TLS 1.3. The daemon has a self-signed certificate. The client pins it by fingerprint.
3. The daemon sends `hello`. The client answers with `pair` (first time) or `auth` (every later time). Both carry an HMAC proof. No secret is ever sent.
4. A new pairing is **pending** until the owner confirms it on the desktop. A pending device can do nothing but `ping`.
5. After `welcome`, the client sends requests (`req`) and receives responses (`res`) and events (`evt`).
6. The protocol carries typed operations only. It has no operation that takes a sway command string.

The spec has two layers:

- The **message layer** (sections 5 to 13): JSON messages. It does not depend on the transport.
- The **transport binding** (section 3): how messages travel. Version 1 defines one binding, `tls-frames`. A later binding (for example WebSocket) can be added without changing any message.

Roles: the **server** is the daemon. The **client** is anything else. The **owner** is the person who runs the daemon.

---

## 2. Terms and data types

### 2.1 JSON

- Every message is one JSON object (RFC 8259), encoded as UTF-8 without a byte order mark.
- Senders MUST NOT emit duplicate member names in one object.
- Senders MUST NOT emit unpaired surrogates in strings.
- Whitespace between JSON tokens is allowed. Member order has no meaning.
- Receivers MUST ignore object members they do not know (section 13).

### 2.2 Types used in field tables

| Type | Meaning |
|---|---|
| `string` | JSON string |
| `int` | JSON number with no fraction and no exponent, in the range 0 to 9007199254740991 (2^53 - 1) unless a narrower range is stated |
| `bool` | JSON `true` or `false` |
| `object` | JSON object |
| `T[]` | JSON array of `T` |
| `T or null` | the member is present; its value is `T` or JSON `null` |
| `bytes32` | `string`: base64url of exactly 32 bytes, 43 characters |
| `bytes16` | `string`: base64url of exactly 16 bytes, 22 characters |

**base64url** means RFC 4648 section 5, alphabet `A-Z a-z 0-9 - _`, **without** padding.
Receivers MUST reject a value that has padding, a character outside the alphabet, or the wrong decoded length.
Receivers MUST also reject a value whose unused trailing bits are not zero (a 43-character value has 2 such bits, a 22-character value has 4).
The decoders of Java and Python do not check this. A portable check is `encode(decode(x)) == x`.

"Required: yes" means the member MUST be present. "Required: no" means the sender MAY leave it out.
An optional member is either absent or has the stated type. JSON `null` is allowed only where the type says `or null`. A receiver treats `null` anywhere else as a wrong type.

### 2.3 Byte strings

`||` is concatenation. `0x00` is one zero byte. Text labels are ASCII bytes with no terminator.
`SHA-256` is FIPS 180-4. `HMAC-SHA256(key, msg)` is RFC 2104 with SHA-256. Output is 32 bytes.

### 2.4 Control and format characters

Some strings end up on a screen or in a terminal. This document calls the following Unicode scalar values **control or format characters**:

| Range | What |
|---|---|
| U+0000 to U+001F, U+007F to U+009F | C0 controls, DEL, C1 controls (includes ESC and CSI) |
| U+00AD, U+061C, U+180E | soft hyphen, Arabic letter mark, Mongolian vowel separator |
| U+200B to U+200F | zero-width characters and direction marks |
| U+2028 to U+202E | line and paragraph separator, bidirectional embeddings and overrides |
| U+2060 to U+206F | word joiner, invisible operators, bidirectional isolates |
| U+FEFF, U+FFF9 to U+FFFB | zero-width no-break space, interlinear annotation marks |
| U+E0000 to U+E007F | tag characters |

The list is fixed for protocol version 1. It does not follow later Unicode versions, so two implementations always agree.

---

## 3. Transport binding `tls-frames`

### 3.1 TCP and TLS

1. The client opens a TCP connection to the server's host and port. The default port is **28414**.
2. The client starts a TLS handshake at once. There is no plaintext mode and no STARTTLS.
3. Both sides MUST use TLS 1.3 (RFC 8446). They MUST NOT negotiate any earlier version.
4. The client MUST offer the ALPN protocol `swayconnect/1`. The server MUST select it. If the client does not offer it, the server MUST close the connection.
5. The client MUST NOT send early data (0-RTT). The server MUST NOT accept it.
6. The server does not request a client certificate.
7. The client MAY omit SNI. The server ignores SNI.
8. TLS compression and certificate compression MUST NOT be used.
9. After the handshake the client MUST check that the server selected `swayconnect/1`, and MUST abort otherwise. Common TLS stacks continue when the peer selects nothing.
10. Every connection is a full handshake. The server MUST NOT send a NewSessionTicket and MUST NOT accept a pre-shared key. The client MUST NOT offer session resumption. A resumed handshake carries no certificate, so the pin check of 3.2 could not run.

The ALPN id names this transport binding, the `hello` format and the proof construction of 5.3. Section 13.1 says when it changes.

### 3.2 Server identity and pinning

The server certificate is self-signed. Its chain, names and validity dates carry no meaning.

The certificate key MUST be ECDSA on curve P-256. The handshake signature scheme is `ecdsa_secp256r1_sha256`.
Both sides MUST support the cipher suite `TLS_AES_128_GCM_SHA256` and the key exchange group `secp256r1`. Group `x25519` is RECOMMENDED.

The server identity is the **fingerprint**:

```
fp = SHA-256(DER encoding of the server's leaf certificate)      32 bytes
```

The leaf certificate is the first certificate in the server's TLS Certificate message.

The client:

1. MUST compute `fp` from the certificate presented on **this** connection.
2. MUST compare it with the pinned fingerprint (from the pairing URI, or stored at pairing).
3. On mismatch, MUST abort the connection and MUST NOT send any application data.
4. MUST verify the TLS handshake signature (CertificateVerify) with the public key of that certificate. A client that turns this check off, or replaces it with a function that always succeeds, is not conformant (15.13).
5. MUST NOT accept the connection because a certificate authority, a hostname check or a date check passed. Only the fingerprint decides.
6. MUST use the `fp` value it computed in step 1 as input to the proofs in section 5.3.

Why rule 4 matters: the certificate is public. Any host on the LAN gets it with one TLS handshake. A relay that replays the certificate without owning the key has the right fingerprint. Only the handshake signature shows that the peer holds the private key.

Rule 6 defeats a relay that presents a different certificate, even when the comparison of rule 2 is broken in a client. It does not help a client that skips rule 4.

### 3.3 Framing

Inside TLS, each direction is a stream of frames.

```
frame = length (4 bytes, unsigned, big-endian) || payload (length bytes)
```

- The payload is exactly one JSON object as defined in 2.1.
- `length` counts payload bytes only. It MUST be at least 2 and at most the limit for the direction and state (section 14).
- A receiver MUST check `length` against the limit **before** it reads or buffers the payload.
- A frame boundary has no relation to TLS record or TCP segment boundaries. A receiver MUST handle a frame split over many reads and many frames in one read.

Example. The request `{"type":"req","id":5,"op":"ping"}` is 33 bytes. On the wire inside TLS:

```
00 00 00 21 7b 22 74 79 70 65 22 3a 22 72 65 71 22 2c 22 69 64 22 3a 35 2c 22 6f 70 22 3a 22 70 69 6e 67 22 7d
```

### 3.4 Frame errors

A frame is **bad** when `length` is under 2, the payload is not valid UTF-8, is not valid JSON, or is not a JSON object.
A frame is **too large** when `length` is over the limit.

| Where | Receiver | Action |
|---|---|---|
| Before `welcome` | server | send `reject` with code `bad_message`, then close |
| After `welcome` | server | send `bye` with code `bad_frame` or `too_large`, then close |
| Any time | client | close the connection |

After a bad or too-large frame the stream position is unknown. The receiver MUST NOT read further frames.

---

## 4. Pairing URI

The owner starts pairing on the desktop. The daemon prints a QR code and the same content as text. The content is one URI:

```
swayconnect://pair?v=1&host=<host>&port=<port>&fp=<fp>&secret=<secret>&name=<name>
```

| Parameter | Required | Value |
|---|---|---|
| `v` | yes | URI format version. MUST be `1`. A client MUST refuse any other value. |
| `host` | yes | IPv4 literal, IPv6 literal without brackets and without zone, or DNS name |
| `port` | yes | decimal TCP port, 1 to 65535, matching `[1-9][0-9]{0,4}` |
| `fp` | yes | `bytes32`: the server fingerprint (3.2) |
| `secret` | yes | `bytes32`: the pairing secret `S` |
| `name` | no | server label for display. 1 to 64 bytes of UTF-8. No control or format character (2.4). |

Rules:

1. The scheme is `swayconnect` and the authority is `pair`. Both are compared case-insensitively, for the ASCII letters only. A parser MUST NOT use Unicode case folding here: it takes U+017F for `s` and U+0131 for `i`.
2. Parameter values are percent-encoded per RFC 3986. A parser MUST percent-decode them.
3. Parameters may come in any order. A parser MUST ignore parameters it does not know.
4. A parser MUST reject a URI where a known parameter appears twice, is missing when required, or has an invalid value.
5. The pairing secret is single use and short lived. A client MUST NOT store it after the pairing attempt ends. A client MUST NOT log the URI. A client SHOULD NOT accept the URI as a process argument, because other local users can read process arguments.
6. `+` in a value is a literal plus sign. It is not a space.
7. Parameter names are case-sensitive. `V=1` is an unknown parameter.
8. The path MUST be empty and a fragment MUST be absent. Every parameter has the form `name=value`.
9. `port` has no sign and no leading zero.
10. A percent-decoded value MUST be valid UTF-8. A bad percent sequence (for example `%zz`) makes the URI invalid. An empty `name` is invalid.

Valid examples (also in `protocol/vectors/v1/pairing-uri.json`):

```
swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=desk
```

Parsed: `v=1`, `host=192.168.1.200`, `port=28414`, `fp` = hex `aa5108ad019ade397fbe42244fba3d963644da7c753c29a774e684275c9fc0b8`, `secret` = bytes `00 01 02 ... 1f`, `name=desk`.

| URI | Parsed difference |
|---|---|
| `swayconnect://pair?v=1&host=fd00%3A%3A1&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `host` = `fd00::1`, no `name` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=my%20desk%2B1` | `name` = `my desk+1` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=a+b` | `name` = `a+b` |
| `SwayConnect://PAIR?name=desk&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&port=28414&host=192.168.1.200&v=1&x=9` | same as the first example; order, case of scheme and authority, and the unknown `x` do not matter |

Invalid examples. A parser MUST reject each one:

| URI | Reason |
|---|---|
| `swayconnect://pair?v=2&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | unknown `v` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg` | `secret` missing |
| `swayconnect://pair?v=1&host=192.168.1.200&port=0&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | port out of range |
| `swayconnect://pair?v=1&host=192.168.1.200&port=028414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | leading zero in `port` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `fp` is not 32 bytes |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg=&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `fp` has padding |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLh&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `fp` has non-zero trailing bits (2.2) |
| `swayconnect://pair?v=1&host=a&host=b&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `host` twice |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=` | empty `name` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=a%1Bb` | control character in `name` |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&name=%ff` | `name` is not UTF-8 |
| `swayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8#x` | fragment |
| `swayconnect://pair/x?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | path not empty |
| `swayconnect://pair?V=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | `v` missing (`V` is another name) |
| `https://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | wrong scheme |
| `ſwayconnect://pair?v=1&host=192.168.1.200&port=28414&fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` | the scheme starts with U+017F (long s), not with `s` (rule 1) |

---

## 5. Handshake: hello, pairing, authentication

### 5.1 State machine

```
client                                          server
  |---- TCP connect, TLS 1.3, ALPN, pin check ---->|
  |                                                |  state: HELLO_SENT
  |<------------------- hello ---------------------|
  |                                                |
  |---- pair   (first time, has pairing URI) ----->|
  |  or auth   (has device_id and device key) ---->|
  |                                                |  verify proof
  |<------------------ welcome --------------------|  state: READY after auth,
  |  or reject, then the server closes ------------|         PENDING after pair
  |                                                |
  |  verify server_proof; on failure close         |
  |==== req / res / evt / bye (section 6) =========|
```

Server states:

| State | Entered when | Accepts from client | Leaves on |
|---|---|---|---|
| `HELLO_SENT` | TLS handshake done and `hello` sent | exactly one frame: `pair` or `auth` | valid `auth`: `READY`. Valid `pair`: `PENDING`. Anything else: `reject`, `CLOSED`. Deadline (section 14): `CLOSED` with no frame. |
| `PENDING` | `welcome` sent after `pair` | `req`. The device has no scopes, so only `ping` succeeds. | owner refuses or the confirmation deadline passes: `bye` with `revoked`, `CLOSED`. Client closes: `CLOSED`. The state does not change to `READY` on this connection. |
| `READY` | `welcome` sent after `auth` | `req` | `bye` sent, client closed, or error: `CLOSED` |
| `CLOSED` | connection closed | nothing | |

Rules:

1. The server sends `hello` as the first frame. The client MUST NOT send any frame before it has received `hello`.
2. The client sends exactly one of `pair` or `auth`.
3. The server answers with exactly one of `welcome` or `reject`. After `reject` the server closes the connection.
4. The client MUST verify `server_proof` in `welcome` before it trusts anything else in that message. It computes the proof with the `device_id` string from `welcome` (after `pair`) or with its stored `device_id` (after `auth`). After `auth` it MUST also check that `welcome.device_id` equals the stored one. In both flows it MUST check that `welcome.protocol` equals the version it sent. On any failure it MUST close the connection and MUST NOT store any credential.
5. Before `welcome` the server reveals nothing about the host, the desktop, the paired devices or the state of pairing.
6. The client MUST NOT send any frame after `pair` or `auth` until it has received `welcome` and verified `server_proof`. When the server finds more client bytes before it has sent `welcome`, it MAY answer `reject` with `bad_message`. The 4096-byte limit applies to everything the client sends before `welcome`.

### 5.2 Messages

#### `hello` (server to client)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"hello"` |
| `protocol` | object | yes | protocol versions the server speaks |
| `protocol.min` | int | yes | lowest version |
| `protocol.max` | int | yes | highest version, `min <= max` |
| `nonce` | bytes32 | yes | `server_nonce`: 32 fresh random bytes for this connection |

#### `pair` (client to server)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"pair"` |
| `protocol` | int | yes | the version the client chose, within the server's range |
| `nonce` | bytes32 | yes | `client_nonce`: 32 fresh random bytes for this connection |
| `proof` | bytes32 | yes | `pair_proof` (5.3) |
| `device_name` | string | yes | display name chosen by the user. 1 to 64 bytes of UTF-8. No control or format character (2.4). |
| `platform` | string | no | informational, for example `"android"`, `"linux"`. 1 to 32 bytes. Only the characters `A-Z a-z 0-9 . _ / -` and space. |
| `client` | string | no | informational client name and version, for example `"swayconnect-cli/0.1.0"`. 1 to 64 bytes. Same characters as `platform`. |

A value outside these limits is answered with `reject` and `bad_message` (5.7). The owner reads these strings in a terminal. That is why the character sets are closed.

A client cannot ask for scopes. The owner fixes them when he opens the pairing window.

#### `auth` (client to server)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"auth"` |
| `protocol` | int | yes | the version the client chose, within the server's range |
| `device_id` | bytes16 | yes | the id received at pairing |
| `nonce` | bytes32 | yes | `client_nonce`: 32 fresh random bytes for this connection |
| `proof` | bytes32 | yes | `client_proof` (5.3) |
| `client` | string | no | as in `pair` |

#### `welcome` (server to client)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"welcome"` |
| `protocol` | int | yes | the version in use. Equals the client's choice. |
| `device_id` | bytes16 | yes | this device's id. New after `pair`. Echoed after `auth`. |
| `server_proof` | bytes32 | yes | `server_proof` (5.3) |
| `confirm_code` | string | after `pair` only | six decimal digits (5.3). The client shows them to the user. Absent after `auth`. |
| `scopes` | string[] | yes | scopes granted to this device (7.1). Empty after `pair`. |
| `ops` | string[] | yes | operations this device may call now. Already filtered by scopes and by read-only mode. `["ping"]` after `pair`. |
| `topics` | string[] | yes | state topics this device may read. Empty after `pair`. |
| `read_only` | bool | yes | `true` when the daemon refuses every action |
| `server` | object | yes | |
| `server.name` | string | yes | display label of the server. 1 to 64 bytes of UTF-8. No control or format character (2.4). |
| `server.version` | string | yes | daemon software version, informational |
| `limits` | object | yes | |
| `limits.max_frame_c2s` | int | yes | largest payload in bytes the client may send from now on |
| `limits.max_frame_s2c` | int | yes | largest payload in bytes the server will send from now on |
| `limits.idle_timeout_ms` | int | yes | the server closes the connection after this long with no frame from the client |

`scopes`, `ops` and `topics` do not change during a connection.

#### `reject` (server to client)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"reject"` |
| `code` | string | yes | one of the codes below |
| `message` | string | yes | human text. Not for parsing. |

| `code` | Meaning | Client reaction |
|---|---|---|
| `bad_message` | the first frame was bad or too large, had an unknown `type`, lacked a required member, or had a member with a wrong type or a value outside its stated limits | fix the client |
| `unsupported_version` | the client's `protocol` is outside the server's range | do not retry with the same version |
| `auth_failed` | unknown `device_id` or wrong proof. The two cases are not told apart. | the device may be revoked. Offer to pair again. Do not loop. |
| `pairing_failed` | no pairing window is open, it expired, it was used already, or the proof is wrong. The cases are not told apart. | ask the owner to start pairing again |

### 5.3 Proofs

Inputs:

| Name | Size | Source |
|---|---|---|
| `fp` | 32 bytes | the client uses the value it computed from this connection's certificate (3.2). The server uses the fingerprint of the certificate it serves. |
| `server_nonce` | 32 bytes | decoded `hello.nonce` |
| `client_nonce` | 32 bytes | decoded `nonce` of `pair` or `auth` |
| `S` | 32 bytes | the pairing secret from the pairing URI |
| `K` | 32 bytes | the device key. Derived at pairing. Stored by both sides. |
| `device_id` | 22 bytes | the ASCII bytes of the `device_id` string as it appears in JSON (not the decoded 16 bytes) |

Definitions:

```
ctx          = fp || server_nonce || client_nonce                                   (96 bytes)

pair_proof   = HMAC-SHA256(S, "swayconnect/1 pair"    || 0x00 || ctx)
K            = HMAC-SHA256(S, "swayconnect/1 key"     || 0x00 || ctx)
confirm_mac  = HMAC-SHA256(K, "swayconnect/1 confirm" || 0x00 || ctx)
client_proof = HMAC-SHA256(K, "swayconnect/1 client"  || 0x00 || ctx || device_id)
server_proof = HMAC-SHA256(K, "swayconnect/1 server"  || 0x00 || ctx || device_id)

confirm_code = the first 4 bytes of confirm_mac as an unsigned big-endian integer,
               modulo 1000000, written as 6 decimal digits with leading zeros
```

Rules:

1. Every proof comparison MUST run in constant time.
2. Both nonces MUST come from a cryptographically secure random source. A nonce MUST NOT be reused.
3. `S` and `K` MUST NOT appear in any message, log line or error text.
4. `ctx` in pairing uses the nonces of the pairing connection. `ctx` in authentication uses the nonces of that later connection. `K` itself is computed once, at pairing, and never changes.
5. `confirm_code` is not a secret. The server computes it and sends it in `welcome`. It exists so that the owner can match the pending device on the desktop with the device in his hand.
6. The labels start with `swayconnect/1`, the ALPN id. They are tied to the ALPN id, not to the `protocol` number (13.1).

This is HMAC challenge-response with channel binding to the server certificate. It is the same idea as the `tls-server-end-point` binding of RFC 5929.

### 5.4 Pairing flow

1. The owner opens a pairing window with a local command on the desktop. The daemon creates `S`, fixes the scopes for the new device and shows the pairing URI. The first correct proof uses the window up. Otherwise it closes after its lifetime (section 14).
2. The client parses the URI (section 4), connects to `host:port`, and checks the certificate fingerprint against `fp` from the URI (3.2).
3. The client receives `hello`. It picks a protocol version, creates `client_nonce`, computes `pair_proof` and sends `pair`.
4. The server checks the frame in the order of 5.7. When no window is open or the proof is wrong it sends `reject` with `pairing_failed`. A wrong proof does not close the window. When no window is open the server MUST still compute one HMAC with a dummy key before it answers, so that both cases take the same time.
5. On a correct proof the server closes the window, computes `K`, creates a random `device_id` and keeps the device as **pending**. It sends `welcome` with `server_proof`, `confirm_code`, `scopes: []`, `ops: ["ping"]` and `topics: []`.
6. The client computes `K` and verifies `server_proof` (5.1 rule 4). It shows `confirm_code` to the user. It keeps `K` and `device_id` in memory only.
7. The server shows the owner, on the desktop, the `device_name`, the `platform`, the peer address and `confirm_code`. The owner compares the code with the one on the client and answers yes or no.
8. Yes: the server stores the device with its scopes and sends the event `pairing.confirmed` (8.3). From now on the device can authenticate.
9. No, or no answer before the confirmation deadline (section 14), or the connection closes first: the server deletes the pending device. When it can still write, it sends `bye` with `revoked`.
10. On `pairing.confirmed` the client stores `host`, `port`, `fp`, `device_id` and `K`. It MUST store `K` in the most protected storage the platform offers. It discards `S`. It then closes this connection and connects again with `auth` (5.5). The granted scopes and operations come from that `welcome`.
11. When the connection ends without `pairing.confirmed`, the client MUST discard `K` and `device_id`. It stores nothing.

A `welcome` after `pair` does not mean "paired". A client MUST NOT store a credential before `pairing.confirmed`.

A pending device is unknown to authentication: `auth` with its `device_id` gets `auth_failed`.

Why step 7 exists: the pairing secret can leak (a photo of the QR code, another app that receives the link, a process list). Without the owner's answer, whoever presents `S` first would be paired. The client chooses `device_name`, so the name alone proves nothing. The code does: only the device that completed this handshake can show it.

If the connection breaks after step 8 and before the client has read `pairing.confirmed`, the server has a device that the client does not know. The owner removes it with the local revoke command and pairs again.

### 5.5 Authentication flow

1. The client connects and checks the certificate fingerprint against the stored `fp` (3.2).
2. The client receives `hello`, creates `client_nonce`, computes `client_proof` with the stored `K` and `device_id`, and sends `auth`.
3. The server checks the frame in the order of 5.7. It looks up `device_id` by exact string comparison and verifies `client_proof`. For an unknown `device_id` it MUST still run one HMAC with a dummy key, then send the same `reject` as for a wrong proof.
4. On success the server computes `server_proof` and sends `welcome`.
5. The client verifies `server_proof` and the other checks of 5.1 rule 4.

### 5.6 Version negotiation

1. `hello.protocol` gives the server's range.
2. The client picks the highest version that both sides speak and sends it in `pair` or `auth`.
3. If there is no common version, the client closes the connection without sending a frame.
4. If the client sends a version outside the range, the server sends `reject` with `unsupported_version`.
5. This document defines version `1` only.
6. The `protocol` value never changes the handshake. It selects the message layer after `welcome` (sections 6 to 12). Framing, `hello`, `pair`, `auth`, `welcome`, `reject` and the proofs belong to the ALPN id (13.1).

### 5.7 Check order for `pair` and `auth`

The server checks the first client frame in this order and reports the first failure:

1. The frame is not bad and not too large (3.4). `type` is `pair` or `auth`. Every required member is present. Every present member that this document defines has the stated type and is within its stated length, range and character set. Else `bad_message`.
2. `protocol` is within the server's range. Else `unsupported_version`.
3. The proof. Else `auth_failed` for `auth`, `pairing_failed` for `pair`.

Example: a frame with `protocol` 2 and a wrong proof gets `unsupported_version`.

---

## 6. Session messages

After `welcome`, four message types exist.

### 6.1 `req` (client to server)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"req"` |
| `id` | int | yes | chosen by the client. MUST be unique among this connection's requests that have no response yet. |
| `op` | string | yes | operation name (section 7) |
| `args` | object | no | operation arguments. Absent means `{}`. |

When `args` is present but is not an object, the server answers with a `res` that carries `bad_request`.

### 6.2 `res` (server to client)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"res"` |
| `id` | int | yes | the `id` of the request |
| `ok` | bool | yes | `true` on success |
| `result` | object | when `ok` is `true` | operation result. MUST be absent when `ok` is `false`. |
| `error` | object | when `ok` is `false` | MUST be absent when `ok` is `true` |
| `error.code` | string | yes | error code (section 10). Clients switch on this. |
| `error.message` | string | yes | human text. Not for parsing. May change between releases. |
| `error.detail` | object | no | extra data defined per code |

The server sends exactly one `res` for every `req` it accepts, unless the connection closes first.

### 6.3 `evt` (server to client)

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"evt"` |
| `event` | string | yes | event name (section 8) |
| `data` | object | yes | event payload |

Events have no id and get no answer.

### 6.4 `bye` (server to client)

Sent before a deliberate close, when the server can still write. The server closes the connection right after it.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | string | yes | `"bye"` |
| `code` | string | yes | one of the codes below |
| `message` | string | yes | human text |

| `code` | Meaning | Client reaction |
|---|---|---|
| `shutdown` | the daemon is stopping | reconnect with backoff |
| `revoked` | the owner revoked this device, or refused or did not confirm its pairing | forget the credential. Do not reconnect. |
| `idle` | no frame from the client within `idle_timeout_ms` | reconnect when needed |
| `bad_frame` | the client sent a bad frame (3.4) | fix the client |
| `too_large` | the client sent a frame over `max_frame_c2s` | fix the client |
| `protocol_error` | the client sent a `req` without a valid `id` or `op`, a frame whose `type` is missing or is not a string, or a frame of any known type other than `req` (`pair`, `auth`, `hello`, `welcome`, `reject`, `res`, `evt`, `bye`) | fix the client |

A connection can also end without `bye` (crash, network loss, write timeout). Clients MUST handle that.

### 6.5 What each side may send

| Sender | Before `welcome` | After `welcome` |
|---|---|---|
| server | `hello`, `welcome`, `reject` | `res`, `evt`, `bye` |
| client | `pair`, `auth` | `req` |

The server never sends `req`. The client never sends `res`, `evt` or `bye`. To leave, the client closes the connection.

---

## 7. Operations

### 7.1 Scopes

A device holds a set of scopes. The owner sets them at pairing. The client cannot change them.

| Scope | Grants |
|---|---|
| `state.read` | subscribe to topic `desktop`, without window titles |
| `state.titles` | window titles inside topic `desktop`. Has effect only together with `state.read`. |
| `control` | `workspace.focus`, `window.focus`, `window.fullscreen` |

### 7.2 Operation table

| `op` | Scope | Changes the desktop | Idempotent |
|---|---|---|---|
| `ping` | none | no | yes |
| `state.subscribe` | `state.read` | no | yes |
| `workspace.focus` | `control` | yes | yes |
| `window.focus` | `control` | yes | yes |
| `window.fullscreen` | `control` | yes | yes |

Every version 1 operation is idempotent. Sending the same request twice gives the same end state.

For an operation that changes the desktop ("action"):

- `ok: true` means sway accepted the command. It does not carry the new state.
- The new state arrives as a `state.changed` event if the client is subscribed.
- When the daemon is in read-only mode, every action returns error `read_only` and the action names are absent from `welcome.ops`.
- The owner can switch the daemon to read-only mode while a session is open. Actions then return `read_only`, whatever `welcome.ops` said.
- Actions are rate limited per device (section 14). Over the limit an action returns `rate_limited`.
- The target ids come from topic `desktop`. A `window_id` MUST equal the `id` of an entry of `windows`. A `workspace_id` MUST equal the `id` of an entry of `workspaces`. The server MUST refuse any other id with `not_found`. That includes an id of the other kind and an id of an entry that the server dropped from the state (9.1).

#### `ping`

Args: none. Result: `{}`.

#### `state.subscribe`

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `topics` | string[] | yes | 1 to 16 topic names, no duplicates |

| Result field | Type | Meaning |
|---|---|---|
| `states` | object[] | one entry per requested topic, in request order |
| `states[].topic` | string | |
| `states[].rev` | int | revision of this state (section 8) |
| `states[].data` | object | the full topic state at subscribe time (section 9) |

After this response the server sends a `state.changed` event each time a subscribed topic changes.
A subscription lasts until the connection closes. There is no unsubscribe in version 1.
Subscribing again to the same topic is allowed. It returns the current state again and does not create a second subscription.
If any requested topic is unknown or forbidden, the whole request fails and no subscription is added.
A client that wants the state once subscribes, reads the response and closes.

Errors: `unknown_topic`, `forbidden`, `bad_request`.

#### `workspace.focus`

Exactly one of the two arguments MUST be present.

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `workspace_id` | int | one of | `id` of a workspace in topic `desktop` |
| `number` | int, 0 to 999 | one of | workspace number. If no workspace has this number, sway creates it. |

Result: `{}`.

Errors: `bad_request` (none or both arguments, or out of range), `not_found` (`workspace_id` is not the id of a workspace), `unsupported_target` (the workspace has no number, has a number above 999, or shares its number with another workspace), `forbidden`, `read_only`, `rate_limited`, `sway_unavailable`, `sway_error`, `timeout`.

The server addresses a workspace by number only. It never puts a workspace name into a sway command.

#### `window.focus`

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `window_id` | int | yes | `id` of a window in topic `desktop` |

Result: `{}`. Errors: `bad_request`, `not_found`, `forbidden`, `read_only`, `rate_limited`, `sway_unavailable`, `sway_error`, `timeout`.

#### `window.fullscreen`

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `window_id` | int | yes | `id` of a window in topic `desktop` |
| `enabled` | bool | yes | `true` makes the window fullscreen. `false` ends fullscreen. |

Result: `{}`. Errors: as `window.focus`.

There is no toggle. A client reads the current value from the state and sends the opposite.

---

## 8. Events and state sync

### 8.1 Event `state.changed`

| `data` field | Type | Required | Meaning |
|---|---|---|---|
| `topic` | string | yes | topic name |
| `rev` | int | yes | revision of this state |
| `data` | object | yes | the **full** topic state. It replaces the previous one. |

### 8.2 Sync rules

1. State is always sent whole. There are no deltas.
2. `rev` belongs to one topic on one connection. It only grows on that connection.
3. The client keeps, per topic, the highest `rev` it has applied on this connection.
4. Every state the client receives for a topic carries `rev`: each entry of a `state.subscribe` result and each `state.changed` event. The client MUST apply a state when its `rev` is higher than the applied one, or when nothing is applied yet, and MUST drop it otherwise.
5. The client MUST NOT expect consecutive revisions. The server skips revisions when changes come faster than the client reads. Only the newest state matters.
6. When the connection closes, the client MUST discard the applied revisions. On the next connection it subscribes again and starts from the state in that response. It MUST NOT compare `rev` across connections.
7. The server MAY send a `state.changed` whose `data` equals the previous one.
8. A client that is not subscribed gets no `state.changed` events.

### 8.3 Event `pairing.confirmed`

Sent once, on a pairing connection (state `PENDING`), when the owner has confirmed the device. `data` is `{}`.

The client reacts as in 5.4 step 10: it stores the credential, closes the connection and connects again with `auth`.

---

## 9. Topic `desktop`

Required scope: `state.read`.

### 9.1 State object

| Field | Type | Required | Meaning |
|---|---|---|---|
| `sway` | object | yes | |
| `sway.online` | bool | yes | `true` when the daemon is connected to sway |
| `sway.version` | string or null | yes | sway version text, for example `"1.12"`. `null` when offline. |
| `workspaces` | Workspace[] | yes | in sway's own workspace order |
| `windows` | Window[] | yes | in layout tree order: per workspace, tiled windows first, then floating ones |
| `truncated` | bool | yes | `true` when the server dropped entries to respect the limits of section 14 |

When `sway.online` is `false`, both arrays are empty and every action returns `sway_unavailable`.

Size rule: the server MUST keep this object, as it encodes it, at or under 262144 bytes. When the count limits of section 14 are not enough for that, the server drops windows from the end of `windows`, then workspaces from the end of `workspaces`, until the object fits. It sets `truncated`.

### 9.2 `Workspace`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | int | yes | workspace id. Stable while the workspace exists. Not stable across a sway restart. |
| `number` | int or null | yes | workspace number, 0 to 2147483647. `null` for a workspace with no number. |
| `name` | string | yes | workspace name, at most 128 Unicode scalar values |
| `output` | string or null | yes | name of the output that shows the workspace, for example `"DP-1"`. At most 128 Unicode scalar values. |
| `focused` | bool | yes | this workspace has the keyboard focus. At most one workspace is focused. |
| `visible` | bool | yes | this workspace is shown on its output |
| `urgent` | bool | yes | a window on this workspace asks for attention |

### 9.3 `Window`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | int | yes | window id. Stable while the window exists. Not stable across a sway restart. |
| `workspace_id` | int | yes | `id` of the workspace that holds the window |
| `app_id` | string or null | yes | application id. For X11 windows the window class. At most 128 Unicode scalar values. |
| `title` | string or null | yes | window title, at most 200 Unicode scalar values. `null` when the device lacks scope `state.titles` or the window has no title. |
| `focused` | bool | yes | this window has the keyboard focus. At most one window is focused. |
| `urgent` | bool | yes | the window asks for attention |
| `floating` | bool | yes | the window floats |
| `fullscreen` | bool | yes | the window is fullscreen |

Rules:

1. Scratchpad windows and sway-internal workspaces are not listed.
2. Titles, application ids and workspace names come from applications. A web page sets a browser title. They are untrusted text. A client MUST treat them as plain text and MUST NOT interpret them as markup, a URL to open, or a command.
3. Server guarantee: `title`, `app_id`, `name` and `output` contain no control or format character (2.4). The server replaces each one with U+FFFD before it cuts the length.
4. Defence in depth: a client that writes these strings to a terminal MUST still escape every control or format character. It must not rely on rule 3 alone.
5. When `truncated` is `false`, every `workspace_id` refers to an entry in `workspaces`.
6. Workspace ids and window ids come from one id space in sway. A client MUST NOT use one kind where the other is asked for (7.2).

---

## 10. Errors

### 10.1 Error codes in `res.error.code`

| Code | Meaning | `detail` | Retry |
|---|---|---|---|
| `bad_request` | `args` is not an object, lacks a required field, has a wrong type or a value out of range | none | no, fix the request |
| `unknown_op` | the server does not know `op` | none | no |
| `unknown_topic` | the server does not offer the topic | `{"topic": string}` | no |
| `forbidden` | the device lacks the scope | `{"scope": string}` | no |
| `read_only` | the daemon is in read-only mode and refuses actions | none | no |
| `rate_limited` | too many actions from this device | `{"retry_after_ms": int}` | yes, after `retry_after_ms` |
| `not_found` | the id is not the id of a window (or workspace) in the server's current state | none | after a state refresh, with a current id |
| `unsupported_target` | the target exists but cannot be addressed safely | none | no |
| `sway_unavailable` | the daemon is not connected to sway | none | later |
| `sway_error` | sway refused the command. `message` holds sway's text, at most 200 characters, with control or format characters (2.4) replaced by U+FFFD. | none | no |
| `timeout` | sway did not answer in time. The action may or may not have happened. | none | yes |
| `internal` | daemon bug | none | later |

The connection stays open after every error in this table.

### 10.2 Check order

The server checks a request in this order and reports the first failure:

1. `id` and `op` valid (else `bye` with `protocol_error`). A frame that is not a `req` never reaches this list: 6.4 and 13.2 rule 2 decide.
2. `op` known (`unknown_op`).
3. scope (`forbidden`).
4. read-only mode, for actions (`read_only`).
5. rate limit, for actions (`rate_limited`). Every action that reaches this step uses one token, whatever happens later.
6. `args` shape and ranges (`bad_request`), topic names (`unknown_topic`).
7. sway online, for actions (`sway_unavailable`).
8. target exists in current state (`not_found`), target addressable (`unsupported_target`).
9. sway's answer (`sway_error`, `timeout`).

---

## 11. Keepalive, timeouts, close

1. The server counts idle time per connection. Every complete frame from the client resets it.
2. The server closes a connection after `idle_timeout_ms` of idle time. It sends `bye` with code `idle` first.
3. A client that wants to stay connected MUST send a frame at least every 30 seconds. `ping` is the frame to use when it has nothing else to send. This also holds while a pairing is pending.
4. A client SHOULD treat a request with no response after 10 seconds as a dead connection and close it.
5. A client MUST NOT retry an action automatically after a client-side timeout on a broken connection. It reconnects, reads the state, and lets the user decide.
6. Either side closes with a TLS `close_notify` and then closes TCP. Half-closed connections are not supported.
7. After an unexpected close a client SHOULD reconnect with backoff: 1 second, doubling, capped at 30 seconds.
8. When the server closes the TCP connection before any TLS data, the client is being refused (connection cap or connection rate limit, section 14). The client MUST back off as in rule 7.

---

## 12. Ordering guarantees

1. Frames in one direction arrive in the order sent.
2. Before `welcome`, the order is fixed: `hello`, then `pair` or `auth`, then `welcome` or `reject`.
3. A client MAY send a request before it has the response to an earlier one. It SHOULD keep at most 8 requests without a response.
4. A client MUST match responses to requests by `id`. It MUST NOT rely on response order.
5. Events may arrive between a request and its response.
6. The response to `state.subscribe` is sent before any `state.changed` event caused by that subscription.
7. For one topic on one connection, `rev` values of `state.changed` events only grow, and none is lower than the `rev` in the subscribe response.
8. The state change caused by an action is sent after the `res` of that action, if it is sent at all. If the action changes nothing, no event follows.
9. Nothing is guaranteed across connections.
10. All `rev` values for one topic on one connection come from one counter, whichever message carries them. A repeated `state.subscribe` never returns a `rev` lower than one already sent on that connection.

---

## 13. Versioning and forward compatibility

### 13.1 What can change, and where the version lives

Two identifiers exist. They cover different things.

| Identifier | Covers | Changes when |
|---|---|---|
| ALPN id `swayconnect/1` | the transport binding (3), the handshake messages `hello`, `pair`, `auth`, `welcome`, `reject` (5), the proofs and their labels (5.3) | any of these changes incompatibly. The next id is `swayconnect/2`, with new proof labels. A server may offer both ids. |
| `protocol` number `1` | the message layer after `welcome` (sections 6 to 12) | a message-layer change that is not in the allowed list below. It is negotiated in `hello` (5.6). |

Allowed without a new `protocol` number and without a new ALPN id:

- new optional fields in any object
- new operations, topics, events
- new error codes, `reject` codes, `bye` codes
- new scope names

Not allowed inside version 1: removing a field, changing a field's type or meaning, making an optional field required. In the message layer that needs `protocol` 2. In framing, `hello`, the other handshake messages or the proofs that needs a new ALPN id.

### 13.2 Receiver rules

Every receiver, client and server:

1. MUST ignore unknown members in any object.
2. MUST ignore a frame whose `type` it does not know, after `welcome`. (Before `welcome`, the server answers `reject` with `bad_message`. A known type that the sender may not send is a `protocol_error`, 6.4.)

Every client:

3. MUST ignore events with an unknown `event` name and `state.changed` events for a topic it did not subscribe to.
4. MUST treat an unknown `error.code` as a failed request that it should not retry.
5. MUST treat an unknown `bye.code` like `shutdown`, and an unknown `reject.code` like `bad_message`.
6. MUST ignore unknown strings in `scopes`, `ops` and `topics`.
7. MUST use `welcome.ops` and `welcome.topics` to decide what to offer. It MUST NOT call an operation that is not listed.

The server:

8. Answers an unknown `op` with `unknown_op` and keeps the connection.

### 13.3 Capabilities

`welcome.ops` and `welcome.topics` are the capability list. They describe what **this device** can do on **this daemon** at connect time.
A client shows a control only when its operation is listed.

---

## 14. Limits

| Limit | Value |
|---|---|
| Default TCP port | 28414 |
| Frame payload, client to server, before `welcome` | 4096 bytes |
| Frame payload, client to server, after `welcome` | `limits.max_frame_c2s`; 16384 in this version |
| Frame payload, server to client, up to and including `welcome` | 65536 bytes |
| Frame payload, server to client, after `welcome` | `limits.max_frame_s2c`; 1048576 in this version |
| TLS handshake deadline, from TCP accept | 5 s |
| Deadline for `pair` or `auth`, from `hello` | 2 s |
| Idle timeout | `limits.idle_timeout_ms`; 90000 in this version |
| Client keepalive interval | at most 30 s between client frames |
| Client request timeout (recommended) | 10 s |
| Sway answer timeout inside the daemon | 2 s |
| Requests without a response per connection (recommended) | 8 |
| Topics per `state.subscribe` | 16 |
| Connections in total | 16 |
| Connections that have not reached `welcome` | 8. When the cap is full, the server closes one of them and takes the new one. It closes the oldest connection of the peer address that holds the most of the 8, the new connection counted. Between addresses that hold equally many, the oldest connection goes. |
| New connections per peer address | token bucket: burst 5, refill 1 token per 2 s. Loopback peers are exempt. |
| Actions per device, over all its connections | token bucket: burst 5, refill 2 tokens per second |
| Pairing window lifetime | 300 s by default |
| Pairing confirmation deadline, from `welcome` | 60 s |
| Workspaces in `desktop` | 256 |
| Windows in `desktop` | 512 |
| Encoded `desktop` state object | 262144 bytes |

Notes:

1. A client MUST be able to receive a frame of `limits.max_frame_s2c` bytes.
2. "Peer address" is the IPv4 address or the IPv6 /64 prefix.
3. The total cap and the connection bucket are applied at TCP accept. The server closes the TCP connection before the TLS handshake. No frame is sent.
4. There is no lockout. A failed or refused connection never blocks a later one beyond the bucket. Keys are 256 bits, so counting failures would protect nothing and would let any host lock the owner out.
5. Failed `pair` attempts are not counted and do not close the pairing window.
6. Loopback peers share one address, so a per-address bucket would let one local process block the others. The cap on connections before `welcome` still applies to them, and for that cap all loopback peers are one peer address.
7. The rule for the cap before `welcome` keeps one peer address from pushing the others out: a flood from one address loses its own connections. It does not help against a flood from the client's own address (every local process, for a loopback client) or from many addresses at once.

---

## 15. Conformance vectors

All values below are also in `protocol/vectors/v1/` as JSON files. The files are the machine-readable form. If this section and the files differ, that is a bug in the repository.

In the transcripts, each frame is shown as two lines. The first line has the sender (`S:` server, `C:` client) and the 4 length bytes in hex. The second line is the payload: exactly those UTF-8 bytes, with no line terminator.
Member order and the absence of whitespace are how the reference implementation writes JSON. They are not required of other senders. A decoder MUST accept these exact bytes. An encoder passes when its output decodes to an equal JSON value.

### 15.1 Fingerprint and test key

Certificate, DER, 392 bytes, shown in standard base64 (with padding):

```
MIIBhDCCASugAwIBAgIUZ4XSOS1jkR7kbwc7gQpAIDRQJ/gwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMc3dheWNvbm5lY3RkMCAXDTI2MTExMzA3NDYwN1oYDzIxMjYxMDIwMDc0NjA3WjAXMRUwEwYDVQQDDAxzd2F5Y29ubmVjdGQwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASzDw/9fxVP7HRyPhXHrGuqEqmIKaTeKrxt1G/UI9tc8L9mfNBKU8A3tk1Xi/XOk+Qtfsl615H/3nEPpSh481N7o1MwUTAdBgNVHQ4EFgQUR8uja7And/Vaz0m1GRjgJAUPuy8wHwYDVR0jBBgwFoAUR8uja7And/Vaz0m1GRjgJAUPuy8wDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNHADBEAiA2dF6JJpOBig7ulsRmJ7NKKmSucgDergNcgGh2v5I0SwIgSvDOgvij2Lm01EVda09AUhrVEKT6p+xNUmwlagZAMTc=
```

```
fp (hex)       aa5108ad019ade397fbe42244fba3d963644da7c753c29a774e684275c9fc0b8
fp (base64url) qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg
```

The private key of this certificate, PKCS#8 DER, standard base64. With it a test server can present the certificate over real TLS, so a client can be checked end to end against the transcripts:

```
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgJkQEVjnF0lpIwuWMEJqv0CDvkddvfRDgOMLM5Xm9NRKhRANCAASzDw/9fxVP7HRyPhXHrGuqEqmIKaTeKrxt1G/UI9tc8L9mfNBKU8A3tk1Xi/XOk+Qtfsl615H/3nEPpSh481N7
```

A second P-256 key that does **not** belong to the certificate, for case 15.13:

```
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgC9MSjNPFGa6T+oH5THcXLEPir0UaD7e7XtQluSr4MtGhRANCAAQlv+KYAr0zvM5+eWp/YAnbNHT0tOxT3X6wwVJxLJn2OeEaQdtrxG0///N9OaZYqN9b+WV6uKJ9MYlx9J8Vsku8
```

Both keys are public test material. A server MUST NOT use them outside tests.

### 15.2 Proofs

Inputs:

```
S               000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
fp              aa5108ad019ade397fbe42244fba3d963644da7c753c29a774e684275c9fc0b8
device_id       "wMHCw8TFxsfIycrLzM3Ozw"   (ASCII bytes 774d484377385446787366497963724c7a4d334f7a77)

pairing connection:
server_nonce    202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
client_nonce    404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f

later connection:
server_nonce    808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f
client_nonce    a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf
```

The full HMAC message for `pair_proof` (18 label bytes, one zero byte, 96 bytes of `ctx`; 115 bytes in total):

```
73776179636f6e6e6563742f31207061697200
aa5108ad019ade397fbe42244fba3d963644da7c753c29a774e684275c9fc0b8
202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f
```

Expected outputs:

| Value | Connection | Hex | base64url |
|---|---|---|---|
| `pair_proof` | pairing | `bf820433f8976b07bb3b15a0c4b31d5cc4b98d94087f1ce8695f0ce923a92b86` | `v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y` |
| `K` | pairing | `aad828da69eb2a990f8ed5c9db325602e983c5a7e15e830d452c6971c9d57da0` | `qtgo2mnrKpkPjtXJ2zJWAumDxafhXoMNRSxpccnVfaA` |
| `confirm_mac` | pairing | `9e6299a47c2432caa32045cbca26f7550759b9d12c45b4d40ab007f232b5e074` | |
| `server_proof` | pairing | `a16e1a57eb895093f750feb329c488b2cc71b06b50dd886700b9570484fc54b4` | `oW4aV-uJUJP3UP6zKcSIssxxsGtQ3YhnALlXBIT8VLQ` |
| `client_proof` | later | `a350156d73989973ddb90297a7afcd24fae53a75fc8c809843ece9f725a152e4` | `o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ` |
| `server_proof` | later | `e09f3dc2d9e26f5913882e15e1f0b2696f29a27314f331ecc80edd9050c4401a` | `4J89wtnib1kTiC4V4fCyaW8ponMU8zHsyA7dkFDEQBo` |

`confirm_code`: the first 4 bytes of `confirm_mac` are the integer 2657261988. Modulo 1000000 and padded to 6 digits that is `261988`.

### 15.3 Transcript T1: pairing succeeds

Uses the pairing URI of section 4 and the pairing connection values of 15.2. The owner answers yes.

```
S: 00 00 00 63
{"type":"hello","protocol":{"min":1,"max":1},"nonce":"ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8"}
C: 00 00 00 da
{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":"Pixel 9","platform":"android","client":"swayconnect-android/0.1.0"}
S: 00 00 01 4e
{"type":"welcome","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","server_proof":"oW4aV-uJUJP3UP6zKcSIssxxsGtQ3YhnALlXBIT8VLQ","confirm_code":"261988","scopes":[],"ops":["ping"],"topics":[],"read_only":false,"server":{"name":"desk","version":"0.1.0"},"limits":{"max_frame_c2s":16384,"max_frame_s2c":1048576,"idle_timeout_ms":90000}}
S: 00 00 00 34
{"type":"evt","event":"pairing.confirmed","data":{}}
```

The client then stores `K` = `qtgo2mnrKpkPjtXJ2zJWAumDxafhXoMNRSxpccnVfaA` and `device_id` = `wMHCw8TFxsfIycrLzM3Ozw`, closes the connection and continues with T2.

### 15.4 Transcript T2: authentication succeeds

Uses the later connection values of 15.2.

```
S: 00 00 00 63
{"type":"hello","protocol":{"min":1,"max":1},"nonce":"gIGCg4SFhoeIiYqLjI2Oj5CRkpOUlZaXmJmam5ydnp8"}
C: 00 00 00 ce
{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ","client":"swayconnect-cli/0.1.0"}
S: 00 00 01 ab
{"type":"welcome","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","server_proof":"4J89wtnib1kTiC4V4fCyaW8ponMU8zHsyA7dkFDEQBo","scopes":["state.read","state.titles","control"],"ops":["ping","state.subscribe","workspace.focus","window.focus","window.fullscreen"],"topics":["desktop"],"read_only":false,"server":{"name":"desk","version":"0.1.0"},"limits":{"max_frame_c2s":16384,"max_frame_s2c":1048576,"idle_timeout_ms":90000}}
```

### 15.5 Transcript T3: a session

Continues after T2. The device has scopes `state.read`, `state.titles`, `control`.
Note the title with one non-ASCII character (U+2014, three bytes in UTF-8): the frame length counts bytes, not characters.

```
C: 00 00 00 4a
{"type":"req","id":1,"op":"state.subscribe","args":{"topics":["desktop"]}}
S: 00 00 02 86
{"type":"res","id":1,"ok":true,"result":{"states":[{"topic":"desktop","rev":7,"data":{"sway":{"online":true,"version":"1.12"},"workspaces":[{"id":4,"number":1,"name":"1","output":"DP-1","focused":true,"visible":true,"urgent":false},{"id":9,"number":2,"name":"2","output":"DP-1","focused":false,"visible":false,"urgent":false}],"windows":[{"id":5,"workspace_id":4,"app_id":"Alacritty","title":"zsh","focused":true,"urgent":false,"floating":false,"fullscreen":false},{"id":12,"workspace_id":9,"app_id":"firefox","title":"Example Domain — Mozilla Firefox","focused":false,"urgent":false,"floating":false,"fullscreen":false}],"truncated":false}}]}}
C: 00 00 00 40
{"type":"req","id":2,"op":"workspace.focus","args":{"number":2}}
S: 00 00 00 2b
{"type":"res","id":2,"ok":true,"result":{}}
S: 00 00 02 7e
{"type":"evt","event":"state.changed","data":{"topic":"desktop","rev":8,"data":{"sway":{"online":true,"version":"1.12"},"workspaces":[{"id":4,"number":1,"name":"1","output":"DP-1","focused":false,"visible":false,"urgent":false},{"id":9,"number":2,"name":"2","output":"DP-1","focused":true,"visible":true,"urgent":false}],"windows":[{"id":5,"workspace_id":4,"app_id":"Alacritty","title":"zsh","focused":false,"urgent":false,"floating":false,"fullscreen":false},{"id":12,"workspace_id":9,"app_id":"firefox","title":"Example Domain — Mozilla Firefox","focused":true,"urgent":false,"floating":false,"fullscreen":false}],"truncated":false}}}
C: 00 00 00 42
{"type":"req","id":3,"op":"window.focus","args":{"window_id":999}}
S: 00 00 00 5f
{"type":"res","id":3,"ok":false,"error":{"code":"not_found","message":"no window with id 999"}}
C: 00 00 00 21
{"type":"req","id":4,"op":"ping"}
S: 00 00 00 2b
{"type":"res","id":4,"ok":true,"result":{}}
S: 00 00 00 44
{"type":"bye","code":"shutdown","message":"daemon is shutting down"}
```

### 15.6 Transcript T4: authentication fails

After the `hello` of T2. The proof is 32 zero bytes.

```
C: 00 00 00 ad
{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}
S: 00 00 00 48
{"type":"reject","code":"auth_failed","message":"authentication failed"}
```

The server then closes the connection.

### 15.7 Transcript T5: version not supported

After the `hello` of T2. The client wrongly sends version 2.

```
C: 00 00 00 ad
{"type":"auth","protocol":2,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ"}
S: 00 00 00 5e
{"type":"reject","code":"unsupported_version","message":"supported protocol versions: 1 to 1"}
```

### 15.8 Transcript T6: sway goes offline

During a session with a `desktop` subscription.

```
S: 00 00 00 a8
{"type":"evt","event":"state.changed","data":{"topic":"desktop","rev":9,"data":{"sway":{"online":false,"version":null},"workspaces":[],"windows":[],"truncated":false}}}
C: 00 00 00 40
{"type":"req","id":6,"op":"window.focus","args":{"window_id":5}}
S: 00 00 00 60
{"type":"res","id":6,"ok":false,"error":{"code":"sway_unavailable","message":"sway is offline"}}
```

### 15.9 Transcript T7: the owner does not confirm the pairing

Same start as T1. The owner answers no, or the confirmation deadline passes.

```
S: 00 00 00 63
{"type":"hello","protocol":{"min":1,"max":1},"nonce":"ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8"}
C: 00 00 00 da
{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":"Pixel 9","platform":"android","client":"swayconnect-android/0.1.0"}
S: 00 00 01 4e
{"type":"welcome","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","server_proof":"oW4aV-uJUJP3UP6zKcSIssxxsGtQ3YhnALlXBIT8VLQ","confirm_code":"261988","scopes":[],"ops":["ping"],"topics":[],"read_only":false,"server":{"name":"desk","version":"0.1.0"},"limits":{"max_frame_c2s":16384,"max_frame_s2c":1048576,"idle_timeout_ms":90000}}
S: 00 00 00 45
{"type":"bye","code":"revoked","message":"pairing was not confirmed"}
```

The client stores nothing. A later `auth` with this `device_id` gets `auth_failed`.

### 15.10 Wrong fingerprint (client-side case)

The client holds pinned `fp` = `qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg`. The server presents any other certificate.
Expected: the client aborts during or right after the TLS handshake. It sends zero frames. It reports a pin failure, not a network error.

### 15.11 Relay with another certificate (server-side case)

A client computes its proof with a fingerprint that differs from the server's own. Take T2 and replace `fp` with 32 zero bytes when computing `client_proof`.
Expected: the server answers `reject` with `auth_failed`.

### 15.12 Frame decoder cases

Limit in each case: 4096 bytes.

| Input bytes (hex) | Expected |
|---|---|
| `00 00 00 02 7b 7d` | one frame, payload `{}` |
| `00 00 00 21 7b 22 74 79 70 65 22 3a 22 72 65 71 22 2c 22 69 64 22 3a 35 2c 22 6f 70 22 3a 22 70 69 6e 67 22 7d` | one frame, the `ping` request |
| the previous line twice, back to back | two equal frames |
| the first 10 bytes of the `ping` line, then the rest in a second read | one frame, after the second read |
| `00 00 00 00` | bad frame |
| `00 00 00 01 7b` | bad frame (length under 2) |
| `00 00 10 01` followed by anything | too large. The decoder MUST fail without reading the payload. |
| `00 00 00 09 7b 6e 6f 74 20 6a 73 6f 6e` | bad frame (not JSON) |
| `00 00 00 02 5b 5d` | bad frame (JSON, but not an object) |
| `00 00 00 03 7b ff 7d` | bad frame (not UTF-8) |

### 15.13 Pinned certificate, wrong private key (client-side case)

The test server presents the certificate of 15.1 and signs the handshake with the second key of 15.1.
The fingerprint is right. The handshake signature is wrong.
Expected: the client MUST abort the TLS handshake. It sends zero frames.
A client that passes 15.10 and fails this case has a signature check that always succeeds (3.2 rule 4).

### 15.14 Request validation cases

Each line is a client frame payload in a `READY` session. Preconditions for every line, unless the last column says otherwise: the device has scopes `state.read` and `control`; `read_only` is false; sway is online; the `desktop` state is the `rev` 7 state of T3 (workspaces 4 and 9, windows 5 and 12); the action bucket of the device is full.

| # | Payload | Expected | Changed precondition |
|---|---|---|---|
| 1 | `{"type":"req","id":1,"op":"state.subscribe","args":{"topics":["desktop"]}}` | success | |
| 2 | `{"type":"req","id":2,"op":"window.fullscreen","args":{"window_id":5,"enabled":true}}` | success | |
| 3 | `{"type":"req","id":3,"op":"state.subscribe","args":{"topics":["media"]}}` | `unknown_topic` | |
| 4 | `{"type":"req","id":4,"op":"sway.command","args":{"command":"exec true"}}` | `unknown_op` | |
| 5 | `{"type":"req","id":5,"op":"workspace.focus","args":{}}` | `bad_request` | |
| 6 | `{"type":"req","id":6,"op":"workspace.focus","args":{"number":2,"workspace_id":9}}` | `bad_request` | |
| 7 | `{"type":"req","id":7,"op":"workspace.focus","args":{"number":1000}}` | `bad_request` | |
| 8 | `{"type":"req","id":8,"op":"workspace.focus","args":{"number":"2; exec true"}}` | `bad_request` | |
| 9 | `{"type":"req","id":9,"op":"window.focus","args":{"window_id":5,"extra":1}}` | success (unknown member ignored) | |
| 10 | `{"type":"req","id":10,"op":"workspace.focus","args":{"workspace_id":9}}` | success | |
| 11 | `{"type":"req","id":11,"op":"window.focus","args":{"window_id":4}}` | `not_found` (4 is a workspace id) | |
| 12 | `{"type":"req","id":12,"op":"workspace.focus","args":{"workspace_id":5}}` | `not_found` (5 is a window id) | |
| 13 | `{"type":"req","id":13,"op":"ping","args":5}` | `bad_request` | |
| 14 | `{"type":"req","id":14,"op":"ping","args":null}` | `bad_request` | |
| 15 | `{"type":"req","id":15,"op":"workspace.focus","args":{"number":2}}` | `forbidden` with `detail.scope` = `control` | scopes: `state.read` only |
| 16 | `{"type":"req","id":16,"op":"state.subscribe","args":{"topics":["desktop"]}}` | `forbidden` with `detail.scope` = `state.read` | scopes: `control` only |
| 17 | `{"type":"req","id":17,"op":"window.focus","args":{"window_id":5}}` | `read_only` | `read_only` is true |
| 18 | `{"type":"req","id":18,"op":"workspace.focus","args":{"number":2}}` | `forbidden` (scope is checked before read-only mode) | `read_only` is true; scopes: `state.read` only |
| 19 | `{"type":"req","id":19,"op":"window.focus","args":{"window_id":5}}` | `rate_limited` with `detail.retry_after_ms` from 1 to 500 | the action bucket is empty |
| 20 | `{"type":"req","id":20,"op":"window.focus","args":{"window_id":5}}` | `sway_unavailable` | sway is offline; state as in T6 |
| 21 | `{"type":"req","op":"ping"}` | `bye` with `protocol_error` | |
| 22 | `{"id":22,"op":"ping"}` | `bye` with `protocol_error` (no `type`) | |
| 23 | `{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ"}` | `bye` with `protocol_error` (handshake frame after `welcome`) | |
| 24 | `{"type":"note","text":"hi"}` | ignored, no answer | |

### 15.15 Handshake validation cases

Each line is the first client frame after the `hello` of the named transcript. For the T1 lines a pairing window is open with the `S` of 15.2. Expected `reject` code on the right. Non-ASCII and control characters are written as JSON `\u` escapes here.

| After `hello` of | Payload | Expected |
|---|---|---|
| T2 | `{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozx","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ"}` | `bad_message` (`device_id` is not canonical base64url) |
| T2 | `{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ","client":null}` | `bad_message` (`null` in an optional member) |
| T2 | `{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ","client":"cli\u001b[2J"}` | `bad_message` (character not allowed in `client`) |
| T2 | `{"type":"auth","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","proof":"o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ"}` | `bad_message` (required member missing) |
| T2 | `{"type":"auth","protocol":2,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","nonce":"oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8","proof":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}` | `unsupported_version` (checked before the proof) |
| T2 | `{"type":"login"}` | `bad_message` (unknown `type`) |
| T1 | `{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":""}` | `bad_message` (empty `device_name`) |
| T1 | `{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":"Pixel\u202e9"}` | `bad_message` (U+202E in `device_name`) |
| T1 | `{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}` | `bad_message` (`device_name` over 64 bytes) |
| T1 | `{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"v4IEM_iXawe7OxWgxLMdXMS5jZQIfxzoaV8M6SOpK4Y","device_name":"Pixel 9","platform":"android\n"}` | `bad_message` (character not allowed in `platform`) |
| T1 | `{"type":"pair","protocol":1,"nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","proof":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","device_name":"Pixel 9"}` | `pairing_failed`. The window stays open. |

---

## 16. Client implementation checklist

A client is conformant when it does all of this:

1. Parses the pairing URI, accepts every valid example and rejects every invalid example in section 4.
2. Connects with TLS 1.3 only, offers ALPN `swayconnect/1`, checks that the server selected it, and never resumes a session.
3. Computes `fp` from the presented leaf certificate and aborts on mismatch before sending a frame (15.10).
4. Keeps the handshake signature check and aborts in case 15.13.
5. Reproduces every value in 15.2.
6. Verifies `server_proof`, `welcome.device_id` and `welcome.protocol`, and closes when one is wrong.
7. After `pair`: shows `confirm_code`, stores the credential only on `pairing.confirmed`, then reconnects with `auth`.
8. Decodes every case in 15.12.
9. Matches responses by `id` and handles events between request and response.
10. Applies states by the rules of 8.2.
11. Ignores unknown members, message types, events, codes, scopes, ops and topics (13.2).
12. Sends a frame at least every 30 seconds while it wants the connection.
13. Offers only operations listed in `welcome.ops`.
14. Stores `K` in protected storage and never logs `S`, `K`, proofs or the pairing URI.
15. Escapes control or format characters (2.4) before it writes server text to a terminal.

How to get the leaf certificate DER and keep the signature check in common stacks (informative; only the Rust row was read against library source, none was run here):

| Stack | Call |
|---|---|
| Rust, rustls 0.23 | `ServerCertVerifier::verify_server_cert` receives `end_entity`; hash its bytes. `verify_tls13_signature` MUST call `rustls::crypto::verify_tls13_signature(message, cert, dss, &provider.signature_verification_algorithms)`. `supported_verify_schemes` comes from the same provider. `verify_tls12_signature` returns an error. Never return `HandshakeSignatureValid::assertion()` without that call. Set `ClientConfig.resumption = Resumption::disabled()`. |
| Java, Android | `X509ExtendedTrustManager.checkServerTrusted(chain, authType, socket)`: `chain[0].getEncoded()`. Android calls this three-argument form for an `SSLSocket`, so all three overloads must run the same check. The platform checks the handshake signature itself. Use a new `SSLContext` per connection so that no session is resumed. Read the selected ALPN with `SSLSocket.getApplicationProtocol()`. |
| Python | `ssl.SSLSocket.getpeercert(binary_form=True)` |
| Go | `tls.ConnectionState.PeerCertificates[0].Raw` |
| Swift | `SecTrustCopyCertificateChain`, then `SecCertificateCopyData` on the first entry |

---

## 17. Version 1 additive extensions (v2 features)

Status: **draft**. Revision 2026-11-13 (review 2). Written for `docs/V2_PLAN.md`. Nothing in this section is implemented yet.

Every item here is an addition allowed by 13.1: new optional fields, new operations, new topics, new scopes, new error and `bye` codes. Protocol stays `1`. ALPN stays `swayconnect/1`. No existing field, message or rule of sections 1 to 16 changes meaning.

Milestones (informative): **M1** = 17.3, 17.4, 17.5 and `bye` `scopes_changed` (17.15). **M2a** = 17.6, 17.7, 17.8. **M2b** = 17.9 to 17.13.

### 17.1 Rules for this section

1. Every operation and topic of this section is an optional capability. A server that does not implement one leaves it out of `welcome.ops`, `welcome.known_ops` and `welcome.topics` and answers it with `unknown_op` or `unknown_topic`. Such a server is still conformant.
2. A client MUST NOT call an operation of this section unless `welcome.ops` lists it (13.2 rule 7). It MUST NOT subscribe to a topic unless `welcome.topics` lists it.
3. A client MUST ignore an unknown value in an enum-like string of this section and use the fallback given in its table.
4. Every new field added to an object of section 9 is optional. Old clients ignore it (13.2 rule 1).
5. Every string a client sends in this section is either a closed enum defined here or an id that the server itself issued in a topic. The server matches it exactly against its own current state. It never interprets it, never puts it into a shell and never puts it into a sway command. The one exception is `input.text` (17.13), whose text goes only to the server's virtual keyboard.
6. One new type is used: `sint`, a JSON number with no fraction and no exponent, in the range -9007199254740991 to 9007199254740991.
7. Every string the server sends in a topic or result of this section follows 9.3 rules 2 to 4: no control or format character (2.4), cut to the stated length, untrusted. A string with no stated length is cut to 128 Unicode scalar values.
8. Binary data (icons) uses base64url without padding (2.2), with the same strictness rules.
9. Size rule for every topic of this section: the server MUST keep the state object, as it encodes it, at or under 262144 bytes. Each topic below states its count caps and its cut rule. A state that was cut carries `truncated: true`.

### 17.2 New scopes

All new scopes are off unless the owner grants them by name at the desktop (`swayconnectd pair --scopes`, or `swayconnectd devices grant`). No preset and no default pairing holds any of them. A client cannot ask for a scope (7.1).

| Scope | Grants | Risk tier | Milestone |
|---|---|---|---|
| `idle.inhibit` | op `keep_awake.set` | 2 (stops the auto-lock for a limited time) | M2a |
| `destructive` | op `window.close` | 2 | M2a |
| `audio` | topic `audio`; ops `audio.set_volume`, `audio.set_mute`, `audio.set_default` on sinks; `audio.set_mute` with `muted: true` on sources | 1 | M2b |
| `audio.capture` | together with `audio`: every write to a source other than muting it (unmute, volume, set default source) | 2 (can open the microphone) | M2b |
| `media` | topic `media`, ops `media.*` | 1 | M2b |
| `launch` | topic `apps`, op `launcher.open`, op `app.launch` for entries with `risky: false` | 2 (starts programs) | M2b |
| `launch.risky` | together with `launch`: `app.launch` for entries with `risky: true` | 2 (terminals: code execution) | M2b |
| `input.keys` | op `input.key` | 2 (code-execution equivalent) | M2b |
| `input.text` | op `input.text` | 2 (code-execution equivalent) | M2b |
| `commands`, `input.pointer` | reserved, no operation in this revision | | |

Existing scope `control` additionally grants `window.move`, `window.floating`, `window.layout`, `window.scratchpad`, `scratchpad.show` and `session.lock` (M2a). Existing scope `state.read` additionally grants `icon.get` (M1) and topic `session` (M2a).

A scope that a check after 10.2 step 3 needs (`audio.capture`, `launch.risky`) is checked at 17.16. Its refusal is `forbidden` with `detail.scope` naming it.

### 17.3 `welcome` additions

| Field | Type | Required | Meaning |
|---|---|---|---|
| `known_scopes` | string[] | no | every scope name this server knows, granted or not |
| `known_ops` | string[] | no | every operation this server implements and is configured for, as it would list them in `ops` for a device with every scope and with read-only mode off |

A client MUST NOT treat `known_scopes` or `known_ops` as granted. Granted scopes are `scopes` only, callable operations are `ops` only.

Client rule for one operation `op` (the basis of the Android `FeatureAvailability`):

| Condition | State |
|---|---|
| `op` in `ops` | available |
| `op` in `known_ops`, its scope in `scopes`, `read_only` is `true` | read-only (show disabled) |
| `op` in `known_ops`, its scope not in `scopes` | locked (show the scope name) |
| `op` not in `known_ops` | unsupported (hide) |
| `known_ops` absent (older server) | unsupported, unless `op` is in `ops` |

### 17.4 Topic `desktop` additions

Required scope: `state.read`, as before.

#### 17.4.1 New fields

| Object | Field | Type | Required | Meaning |
|---|---|---|---|---|
| state object (9.1) | `outputs` | Output[] | no | the outputs, in sway's tree order. Sway-internal outputs are not listed. At most 16. |
| state object | `scratchpad` | ScratchpadWindow[] | no | windows hidden in the scratchpad. A scratchpad window that is shown is a normal entry of `windows`. |
| state object | `layouts_cut` | bool | no | `true` when the server set every `Workspace.layout` to `null` to respect the size limit (17.4.5) |
| `Workspace` (9.2) | `rect` | Rect or null | no | the workspace area in global logical pixels, without the bar. `null` when unknown. |
| `Workspace` | `layout` | Layout or null | no | the tiling layout (17.4.3). `null` when the server dropped it (then `layouts_cut` is true), or when the workspace has no measurable area (a missing or 0 × 0 rect, for example on a disabled output; `layouts_cut` stays false). |
| `Window` (9.3) | `icon` | string or null | no | content hash of the app icon (17.5). `null` when the server has no icon. Equal for windows of the same app. |

A `Window` keeps every rule of 9.3. Scratchpad windows stay out of `windows` (9.3 rule 1) and appear only in `scratchpad`.

#### 17.4.2 `Rect`, `Output`, `ScratchpadWindow`

`Rect`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `x`, `y` | sint | yes | top left corner, global logical pixels |
| `w`, `h` | int | yes | size in logical pixels |

`Output`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | yes | output name, for example `"DP-1"`. At most 128 Unicode scalar values. Equals `Workspace.output` of its workspaces. |
| `model` | string or null | yes | monitor make and model text, for display only. At most 128 scalar values. |
| `rect` | Rect | yes | output area in global logical pixels |
| `scale_milli` | int | yes | scale times 1000, for example `1250` |
| `transform` | string | yes | `normal`, `90`, `180`, `270`, `flipped`, `flipped-90`, `flipped-180`, `flipped-270`. Unknown value: treat as `normal`. |
| `active` | bool | yes | the output is enabled |
| `power` | bool | yes | the output is powered on |
| `current_workspace_id` | int or null | yes | `id` of the workspace it shows |

`ScratchpadWindow`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | int | yes | window id, same id space as 9.3 |
| `app_id` | string or null | yes | as in 9.3 |
| `title` | string or null | yes | as in 9.3, `null` without `state.titles` |
| `icon` | string or null | yes | as `Window.icon` |

#### 17.4.3 `Layout` and `Tile`

`Layout`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `size` | int[] | yes | exactly 2 values: workspace width and height in logical pixels. Gives the aspect ratio. |
| `fullscreen` | int or null | yes | `id` of the window that is fullscreen on this workspace, else `null` |
| `tiled` | Tile or null | yes | the root of the tiled tree. `null` when the workspace has no tiled window. |
| `floating` | Tile[] | yes | floating windows and floating containers, bottom first, top last |

`Tile` is one of two shapes. A receiver tells them apart by the member `w`.

Window tile:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `w` | int | yes | window `id`. It is the `id` of an entry of `windows` with this workspace's `id` as `workspace_id`. |
| `r` | int[] | yes | exactly 4 values `[x, y, w, h]` in basis points of the workspace (0 to 10000), relative to `Workspace.rect`. Clamped to 0..10000. |

Container tile:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `k` | string | yes | `h` split horizontal, `v` split vertical, `t` tabbed, `s` stacked. Unknown value: treat as `h`. |
| `r` | int[] | yes | as above |
| `c` | Tile[] | yes | children in sway order, 1 or more |
| `a` | int | no | for `t` and `s`: index into `c` of the child shown. Absent or out of range: treat as 0. |

Server rules:

1. A split container with exactly one child is replaced by that child, except the root.
2. Depth is at most 16 container levels. Below that, the server puts the remaining window tiles directly into the deepest container. Every tile still has its own `r`, so no geometry is lost.
3. A window id appears at most once in all layouts together.
4. The workspace's own `fullscreen_mode` is ignored. `fullscreen` names a window whose own fullscreen mode is not 0.
5. A window in `windows` may be missing from the layout (for example after a cut). A client lists it beside the map.
6. `a` is the position in `c` of the first id of the container's sway `focus` list that is one of its children.

Client rules:

1. A client MUST tolerate a `w` that is not in `windows` (skip the tile) and an `r` outside 0..10000 (clamp).
2. A client MUST NOT treat the layout as a reason to call an operation. It is for display.

#### 17.4.4 Freshness (informative)

Sway sends no event for a resize. While at least one client subscribes to `desktop`, the server re-reads the tree every 2 s and on sway `binding` events, and publishes only when the state differs. A layout can lag a resize by up to about 2 s.

#### 17.4.5 Size rule

9.1's limit of 262144 bytes still holds. When the state is too large, the server first sets every `Workspace.layout` to `null` and sets `layouts_cut`. Then it drops `scratchpad` entries from the end. Then it applies 9.1 (windows, then workspaces) and sets `truncated`.

### 17.5 Icons

#### `icon.get`

Scope: `state.read`. Not an action: allowed in read-only mode. It has its own rate class `icons` (17.14), which applies in read-only mode too.

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `hashes` | string[] | yes | 1 to 8 distinct icon hashes from `Window.icon`, `ScratchpadWindow.icon`, `Player.icon` or `App.icon` |

| Result field | Type | Meaning |
|---|---|---|
| `icons` | object[] | one entry per hash the server has, in request order |
| `icons[].hash` | string | the hash |
| `icons[].png` | string | base64url of a PNG image, at most 128 x 128 pixels, at most 65536 bytes before encoding |
| `missing` | string[] | hashes the server does not have |

Rules:

1. A hash is base64url of the first 16 bytes of SHA-256 over the PNG bytes: 22 characters.
2. The server answers only from PNGs it produced itself. The operation takes no path and no name.
3. A client MUST check `SHA-256(png)` against the hash and drop a mismatch. It MUST decode the PNG as untrusted input: read the bounds first, refuse more than 256 x 256 pixels.
4. A client MAY cache icons by hash across connections and servers.
5. Size (informative): 8 icons of 65536 bytes encode to 8 x 87382 characters, about 700 KB with JSON. The result always fits `max_frame_s2c`.

Errors: `bad_request`, `forbidden`, `rate_limited`.

### 17.6 Window and session operations (scope `control`)

All are actions (7.2): rate limited, refused in read-only mode, `ok` means sway (or the lock command) accepted.

| `op` | Args | Sway command (server side, informative) | Errors beyond 7.2's usual set |
|---|---|---|---|
| `window.move` | `window_id` int, and exactly one of `workspace_id` int or `number` int 0..999 | `[con_id=<id>] move container to workspace number <n>` | `unsupported_target` (workspace without a usable number, as in `workspace.focus`) |
| `window.floating` | `window_id` int, `enabled` bool | `[con_id=<id>] floating enable` or `disable` | |
| `window.layout` | `window_id` int, `layout` one of `splith`, `splitv`, `tabbed`, `stacking` | `[con_id=<id>] layout <layout>`. It changes the window's parent container. | |
| `window.scratchpad` | `window_id` int | `[con_id=<id>] move scratchpad` | |
| `scratchpad.show` | `window_id` int, an `id` from `scratchpad` | `[con_id=<id>] scratchpad show` | `not_found` when the id is not in `scratchpad` |
| `session.lock` | none | none: the server runs its configured lock command | `backend_unavailable`, `spawn_failed` |

Result of each: `{}`, except `session.lock`: `{already_locked: bool}`.

`session.lock` rules:

1. It is in `known_ops` and `ops` only when the owner configured a lock command.
2. When topic `session` says `locked: true`, or the lock command the server started last is still running, the server starts nothing and returns `{already_locked: true}`.
3. At most one lock command runs at a time. When lock state is unknown (`locked: null`) and none runs, the server starts it (locking is the safe direction).

Note for clients: the owner may run tools that change the layout again (for example autotiling). A layout change may look undone.

### 17.7 `window.close` (scope `destructive`)

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `window_id` | int | yes | `id` of an entry of `windows` |

Action. Result `{}`. Sway asks the application to close. The application may refuse or ask to save.
A client MUST ask the user to confirm before it sends this.

### 17.8 Keep awake (scope `idle.inhibit`)

#### `keep_awake.set`

Action. The server holds at most one keep-awake, shared by all devices. The last `true` sets the expiry.

| Arg | Type | Required | Meaning |
|---|---|---|---|
| `enabled` | bool | yes | `true` starts it or replaces the expiry, `false` ends it |
| `duration_s` | int, 60 to `max_duration_s` | no | how long from now. Default `default_duration_s` of topic `session`. Ignored when `enabled` is `false`. |

| Result field | Type | Meaning |
|---|---|---|
| `remaining_ms` | int or null | time left, `null` when off |

Rules:

1. On `true` the server holds a logind idle inhibitor (mode `block`) that ends by itself at the expiry, even if the server stops. The expiry is now + `duration_s`, shortened so that rule 5 holds. A replace sets a new expiry; it does not reset the count of rule 5.
2. If an active output has `power` `false`, the server turns all outputs on after it takes the inhibitor.
3. `false` ends it, whichever device started it.
4. The holder is the device that sent the last `true`.
5. Continuous cap. The server counts on-time since the last break. A break is an off period of at least `cooldown_s`. A shorter off period does not reset the count. When the count reaches `max_continuous_s`, the server ends keep-awake and refuses `true` with `limit_reached` until it has been off for `cooldown_s`. The server config MUST give `cooldown_s` larger than the longest idle timeout of the desktop (default 900 s against swayidle's 300 s and 600 s here).
6. It also ends on revoke of the holder, on read-only mode (SIGUSR1) and on daemon stop. It does not end when the client disconnects.
7. Every `true` that starts keep-awake shows a desktop notice with the holder's short device id.

Errors: `bad_request`, `forbidden`, `read_only`, `rate_limited`, `limit_reached` (`detail.retry_after_ms`), `backend_unavailable`, `spawn_failed`.

#### Topic `session`

Scope: `state.read`. Full snapshots with `rev`, as section 8.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `keep_awake` | object | yes | |
| `keep_awake.active` | bool | yes | |
| `keep_awake.remaining_ms` | int or null | yes | at the moment the state was built. The client counts down locally. |
| `keep_awake.holder` | string or null | yes | `this_device`, `other_device`, or `null` when off. Unknown value: `other_device`. |
| `keep_awake.default_duration_s` | int | yes | |
| `keep_awake.max_duration_s` | int | yes | |
| `keep_awake.max_continuous_s` | int | yes | the cap of rule 5 |
| `keep_awake.cooldown_remaining_ms` | int or null | yes | set while `limit_reached` applies |
| `windows_inhibiting` | int | yes | number of windows that keep the desktop awake by themselves (sway `inhibit_idle`), for example a fullscreen video |
| `locked` | bool or null | yes | `true` when the server believes the screen is locked, `null` when it cannot tell. Re-checked at least every 2 s while a client subscribes. |
| `input` | object | no | present when the server offers `input.*` (17.13) |
| `input.active` | bool | yes | a remote keyboard session is armed on the server |
| `input.holder` | string or null | yes | as `keep_awake.holder` |
| `launcher` | object | no | present when the server offers `launcher.open` (17.11) |
| `launcher.open` | bool | yes | a launcher started by the server is running |
| `launcher.holder` | string or null | yes | the device that opened it, as `keep_awake.holder` |

Because `holder` depends on the device, the server builds this state per device.

### 17.9 Audio (scope `audio`)

#### Topic `audio`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `available` | bool | yes | `false` when the sound server cannot be reached. Then both lists are empty. |
| `max_volume_pct` | int | yes | highest value a client may send, 100 to 150 |
| `default_sink` | int or null | yes | `id` of the default output device |
| `default_source` | int or null | yes | `id` of the default input device |
| `sinks` | Device[] | yes | output devices, at most 64 |
| `sources` | Device[] | yes | input devices, at most 64. Monitor sources are not listed. |
| `truncated` | bool | yes | `true` when the server dropped devices for a cap |

`Device`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | int | yes | issued by the server. Stable while the server runs and the device exists. |
| `description` | string | yes | human name, at most 128 scalar values |
| `kind` | string | yes | `speaker`, `headphones`, `hdmi`, `bluetooth`, `usb`, `microphone`, `other`. Unknown value: `other`. |
| `volume_pct` | int | yes | current volume, highest channel, 0 to 1000 |
| `muted` | bool | yes | |
| `available` | bool | yes | `false` when its port is unplugged |

Size (informative): 128 devices of about 700 bytes each stay far under 17.1 rule 9. Cut rule: drop devices from the end of `sources`, then of `sinks`.

#### Audio operations

All are actions. `audio.set_volume` uses the `continuous` rate class (17.14), the others the `actions` class.

| `op` | Args | Result |
|---|---|---|
| `audio.set_volume` | `device_id` int, `volume_pct` int 0 to `max_volume_pct` | `{}` |
| `audio.set_mute` | `device_id` int, `muted` bool | `{}` |
| `audio.set_default` | `device_id` int (a sink or a source) | `{}` |

Capture rule: when `device_id` is a source, every op except `audio.set_mute` with `muted: true` also needs `audio.capture` (17.16 step 8a). When a source is unmuted from a device, the server shows a desktop notice with the device's short id.

Errors: `bad_request` (out of range), `not_found` (id not in the current `audio` state), `forbidden`, `read_only`, `rate_limited`, `backend_unavailable`, `timeout`.

### 17.10 Media (scope `media`)

#### Topic `media`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `players` | Player[] | yes | MPRIS players, at most 16 |
| `active_player_id` | int or null | yes | the player a simple client controls: the last one that played |
| `truncated` | bool | yes | `true` when the server dropped players for the cap |

`Player`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | int | yes | issued by the server |
| `name` | string | yes | player identity, for example `"Chromium"`. At most 64 scalar values. |
| `app_id` | string or null | yes | icon hint, at most 128 scalar values |
| `icon` | string or null | yes | icon hash |
| `status` | string | yes | `playing`, `paused`, `stopped`. Unknown: `stopped`. |
| `title`, `artist`, `album` | string or null | yes | at most 200 scalar values each. `null` without scope `state.titles`. |
| `can_play`, `can_pause`, `can_next`, `can_previous` | bool | yes | |

No album art, no position and no seek in this revision. Cut rule: drop players from the end, never the active one.

#### Media operations

Actions, `actions` rate class. Arg `player_id` int on each. Result `{}`.

| `op` | Extra args |
|---|---|
| `media.play_pause` | none |
| `media.next` | none |
| `media.previous` | none |

Errors: `bad_request`, `not_found`, `unsupported_target` (the player does not allow it), `forbidden`, `read_only`, `rate_limited`, `backend_unavailable`, `timeout`.

### 17.11 Apps and launcher (scope `launch`)

#### Topic `apps`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `apps` | App[] | yes | sorted by `name`, at most 512 |
| `recent` | string[] | yes | up to 8 `entry_id`s, most recent first |
| `truncated` | bool | yes | `true` when the server cut the state (rule below) |

`App`:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `entry_id` | string | yes | desktop entry id, 1 to 255 bytes of `A-Z a-z 0-9 . _ -` |
| `name` | string | yes | at most 128 scalar values |
| `generic_name` | string or null | yes | at most 128 scalar values |
| `keywords` | string[] | yes | at most 16, each at most 64 scalar values, together at most 512 bytes of UTF-8 |
| `icon` | string or null | yes | icon hash |
| `terminal` | bool | yes | runs in a terminal |
| `risky` | bool | yes | the owner marked it, or it is a terminal. Launching it needs `launch.risky`. |

The topic never carries the Exec line or a file path.

Cut rule (17.1 rule 9): first set every `keywords` to `[]`, then every `generic_name` to `null`, then drop apps from the end of `apps` (and their ids from `recent`), until the state fits. Set `truncated`.

#### Launch operations

Actions in the `launch` rate class.

| `op` | Args | Result | Notes |
|---|---|---|---|
| `app.launch` | `entry_id` string (required), `number` int 0..999 (optional) | `{}` | `entry_id` MUST be in the current `apps` state. A `risky` entry also needs `launch.risky` (17.16 step 8b). With `number`, the server focuses that workspace first. `ok` means started, not that a window appeared. |
| `launcher.open` | none | `{already_open: bool}` | runs the configured launcher (for example fuzzel). At most one runs. The caller becomes `session.launcher.holder` unless one was already open. |

There is no close operation: the launcher closes on Escape at the desktop, or by `input.key` `Escape` with target `launcher`.

Errors: `bad_request`, `not_found`, `forbidden`, `read_only`, `rate_limited`, `backend_unavailable`, `spawn_failed`.

### 17.12 Owner buttons

Not in this revision. Scope name `commands` is reserved for it.

### 17.13 Remote keyboard (scopes `input.text`, `input.keys`)

Both are actions in the `input` rate classes.

Target: exactly one of these args:

| Arg | Type | Meaning |
|---|---|---|
| `window_id` | int | the window the user saw as focused. It MUST be the focused window when each part of the input is delivered. |
| `launcher` | bool, only `true` | the launcher this device opened (`session.launcher.holder` is `this_device`) |

#### `input.text`

| Arg | Type | Required | Meaning |
|---|---|---|---|
| target | see above | yes | |
| `text` | string | yes | 1 to 1024 bytes of UTF-8. No control or format character (2.4), except U+200C and U+200D (needed by emoji sequences and some scripts). So no newline, no tab. |

Result `{delivered_chars: int}`. Enter is never part of text; it is `input.key` `Return`. A client with longer text splits it at character boundaries into requests of at most 1024 bytes, sends them in order and stops at the first error.

#### `input.key`

| Arg | Type | Required | Meaning |
|---|---|---|---|
| target | see above | yes | |
| `key` | string | yes | one of `Escape`, `Tab`, `Return`, `BackSpace`, `Delete`, `Insert`, `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `Page_Up`, `Page_Down`, `space`, `F1` to `F12`, or one ASCII letter `a`-`z` or digit `0`-`9` |
| `modifiers` | string[] | no | distinct values from `ctrl`, `alt`, `shift`, `super`. `ctrl` and `alt` together with `F1` to `F12` is refused with `bad_request` (VT switch). |
| `repeat` | int 1 to 32 | no | default 1 |

Result `{}`.

Server rules (both ops):

1. The server cuts text into parts of at most 16 characters. A key press is one part.
2. Before each part the server checks, in order: the screen is not locked (else `locked`; unknown lock state also refuses with `locked`); for target `launcher`, the launcher this device opened is still running (else `launcher_closed`); no keyboard-grabbing surface runs other than that launcher (else `keyboard_grabbed`); the focused window, read from sway at that moment, is the target `window_id`, or for target `launcher` the window that was focused when `launcher.open` ran (else `focus_changed`).
3. The focus read of rule 2 is a synchronous request to sway made after the previous part was fully processed by the compositor. The server does not reuse an earlier answer.
4. It stops at the first failed check and reports how much was delivered.
5. It releases every key and modifier after each request and when the connection closes.
6. The first input of a connection arms an input session and shows a notice on the desktop.

New errors with `detail`:

| Code | `detail` |
|---|---|
| `focus_changed` | `{"delivered_chars": int, "focused_window_id": int or null}` |
| `locked` | `{"delivered_chars": int}` |
| `keyboard_grabbed` | `{"delivered_chars": int}` |
| `launcher_closed` | `{"delivered_chars": int}` |

### 17.14 Rate classes

Each device has one bucket per class, shared by its connections. Version 1's action bucket is the `actions` class.

| Class | Ops | Bucket |
|---|---|---|
| `actions` | `workspace.focus`, `window.*`, `scratchpad.show`, `session.lock`, `keep_awake.set`, `audio.set_mute`, `audio.set_default`, `media.*` | burst 5, refill 2 per second (unchanged) |
| `continuous` | `audio.set_volume` | burst 10, refill 10 per second |
| `launch` | `app.launch`, `launcher.open` | burst 3, refill 1 per 5 s |
| `input` | `input.key` | burst 20, refill 10 per second |
| `input_text` | `input.text` | burst 4, refill 1 per second, and at most 4096 characters per minute |
| `icons` | `icon.get` (also in read-only mode) | burst 4, refill 2 per second |

`rate_limited` keeps its `detail.retry_after_ms`.

### 17.15 New error and `bye` codes

| Code | Where | Meaning | Retry |
|---|---|---|---|
| `focus_changed` | `res.error` | the focused window is not the target | no; show the user the new focus |
| `locked` | `res.error` | the screen is locked, or lock state is unknown | no |
| `keyboard_grabbed` | `res.error` | another surface takes the keyboard (launcher, menu, lock screen) | no |
| `launcher_closed` | `res.error` | no launcher this device opened is running | no |
| `limit_reached` | `res.error` | a server cap applies; `detail.retry_after_ms` | after the delay |
| `backend_unavailable` | `res.error` | the system service behind the op is missing or down | later |
| `spawn_failed` | `res.error` | the server could not start the program | no |
| `scopes_changed` | `bye` | the owner changed this device's scopes | reconnect at once, without backoff; `welcome` lists the new scopes |

Old clients treat `scopes_changed` like `shutdown` (13.2 rule 5) and unknown error codes as non-retryable failures (13.2 rule 4).

### 17.16 Check order additions

10.2 holds. Step 5 uses the op's rate class and also applies to `icon.get`. After step 8:

8a. audio ops on a source, except `audio.set_mute` with `muted: true`: scope `audio.capture` (`forbidden`).
8b. `app.launch` of a `risky` entry: scope `launch.risky` (`forbidden`).
8c. `input.*`: the checks of 17.13 rule 2, in its order (`locked`, `launcher_closed`, `keyboard_grabbed`, `focus_changed`).

### 17.17 Limits additions

| Limit | Value |
|---|---|
| Hashes per `icon.get` | 8 |
| PNG per icon | 128 x 128 px, 65536 bytes |
| Encoded state of every topic of section 17 | 262144 bytes |
| Outputs in `desktop` | 16 |
| `input.text` length | 1024 bytes |
| Characters per input part | 16 |
| Layout depth | 16 container levels |
| `keep_awake.set` `duration_s` | 60 to `max_duration_s` (default 7200) |
| Keep-awake continuous cap and cooldown | server config; defaults 14400 s and 900 s |
| Sinks, sources in `audio` | 64 each |
| Players in `media` | 16 |
| Apps in `apps` | 512 |
| Keywords per app | 16, together 512 bytes |

### 17.18 Example frames (informative)

Payloads only; the 4-byte length prefix is left out. Normative vectors land in `protocol/vectors/v1/ext/` with each slice.

`welcome` after `auth`, for a device that was granted `idle.inhibit` and `audio` by name:

```
{"type":"welcome","protocol":1,"device_id":"wMHCw8TFxsfIycrLzM3Ozw","server_proof":"...","scopes":["state.read","state.titles","control","idle.inhibit","audio"],"ops":["ping","state.subscribe","icon.get","workspace.focus","window.focus","window.fullscreen","window.move","window.floating","window.layout","window.scratchpad","scratchpad.show","session.lock","keep_awake.set","audio.set_volume","audio.set_mute","audio.set_default"],"topics":["desktop","session","audio"],"known_scopes":["state.read","state.titles","control","idle.inhibit","destructive","audio","audio.capture","media","launch","launch.risky","input.keys","input.text"],"known_ops":["ping","state.subscribe","icon.get","workspace.focus","window.focus","window.fullscreen","window.move","window.floating","window.layout","window.scratchpad","scratchpad.show","session.lock","window.close","keep_awake.set","audio.set_volume","audio.set_mute","audio.set_default","media.play_pause","media.next","media.previous","app.launch","launcher.open","input.text","input.key"],"read_only":false,"server":{"name":"worxbox","version":"0.2.0"},"limits":{"max_frame_c2s":16384,"max_frame_s2c":1048576,"idle_timeout_ms":90000}}
```

A `desktop` state with a layout (one workspace of the live desktop, titles shortened):

```
{"sway":{"online":true,"version":"1.12"},"outputs":[{"name":"DP-1","model":"Dell P3223QE","rect":{"x":0,"y":0,"w":3072,"h":1728},"scale_milli":1250,"transform":"normal","active":true,"power":true,"current_workspace_id":20}],"workspaces":[{"id":20,"number":3,"name":"3","output":"DP-1","focused":true,"visible":true,"urgent":false,"rect":{"x":2,"y":28,"w":3068,"h":1698},"layout":{"size":[3068,1698],"fullscreen":null,"tiled":{"k":"h","r":[0,0,10000,10000],"c":[{"w":24,"r":[0,0,4997,10000]},{"w":29,"r":[5003,0,4997,10000]}]},"floating":[{"w":23,"r":[4876,3510,244,2980]}]}}],"windows":[{"id":24,"workspace_id":20,"app_id":"Emulator","title":"Android Emulator","focused":false,"urgent":false,"floating":false,"fullscreen":false,"icon":null},{"id":29,"workspace_id":20,"app_id":"google-chrome","title":"Docs","focused":true,"urgent":false,"floating":false,"fullscreen":false,"icon":"q83vEjRWeJCrze8SNFZ4kA"},{"id":23,"workspace_id":20,"app_id":"Emulator","title":"Extended controls","focused":false,"urgent":false,"floating":true,"fullscreen":false,"icon":null}],"scratchpad":[],"truncated":false}
```

`icon.get`:

```
C: {"type":"req","id":7,"op":"icon.get","args":{"hashes":["q83vEjRWeJCrze8SNFZ4kA","AAAAAAAAAAAAAAAAAAAAAA"]}}
S: {"type":"res","id":7,"ok":true,"result":{"icons":[{"hash":"q83vEjRWeJCrze8SNFZ4kA","png":"iVBORw0KGgo..."}],"missing":["AAAAAAAAAAAAAAAAAAAAAA"]}}
```

Keep awake for one hour, then the `session` state (a server without input and launcher ops):

```
C: {"type":"req","id":8,"op":"keep_awake.set","args":{"enabled":true,"duration_s":3600}}
S: {"type":"res","id":8,"ok":true,"result":{"remaining_ms":3600000}}
S: {"type":"evt","event":"state.changed","data":{"topic":"session","rev":3,"data":{"keep_awake":{"active":true,"remaining_ms":3600000,"holder":"this_device","default_duration_s":3600,"max_duration_s":7200,"max_continuous_s":14400,"cooldown_remaining_ms":null},"windows_inhibiting":0,"locked":false}}}
```

Lock while already locked:

```
C: {"type":"req","id":9,"op":"session.lock","args":{}}
S: {"type":"res","id":9,"ok":true,"result":{"already_locked":true}}
```

Volume while dragging, then a refusal, then a mic unmute without `audio.capture`:

```
C: {"type":"req","id":10,"op":"audio.set_volume","args":{"device_id":1,"volume_pct":55}}
S: {"type":"res","id":10,"ok":true,"result":{}}
C: {"type":"req","id":11,"op":"audio.set_volume","args":{"device_id":1,"volume_pct":180}}
S: {"type":"res","id":11,"ok":false,"error":{"code":"bad_request","message":"volume_pct above max_volume_pct"}}
C: {"type":"req","id":12,"op":"audio.set_mute","args":{"device_id":2,"muted":false}}
S: {"type":"res","id":12,"ok":false,"error":{"code":"forbidden","message":"needs audio.capture","detail":{"scope":"audio.capture"}}}
```

Typing cut short by a focus change:

```
C: {"type":"req","id":13,"op":"input.text","args":{"window_id":29,"text":"git status --short"}}
S: {"type":"res","id":13,"ok":false,"error":{"code":"focus_changed","message":"focus moved","detail":{"delivered_chars":16,"focused_window_id":24}}}
```

Typing while locked:

```
C: {"type":"req","id":14,"op":"input.key","args":{"window_id":29,"key":"Return"}}
S: {"type":"res","id":14,"ok":false,"error":{"code":"locked","message":"screen is locked","detail":{"delivered_chars":0}}}
```

Enter for a launcher that already closed:

```
C: {"type":"req","id":15,"op":"input.key","args":{"launcher":true,"key":"Return"}}
S: {"type":"res","id":15,"ok":false,"error":{"code":"launcher_closed","message":"launcher is not running","detail":{"delivered_chars":0}}}
```

Scopes changed at the desktop:

```
S: {"type":"bye","code":"scopes_changed","message":"scopes of this device changed"}
```

### 17.19 Client checklist additions

16. Calls an operation of section 17 only when `welcome.ops` lists it. Shows the others by the table of 17.3: read-only, locked with the scope name, or hidden.
17. Checks the hash of every icon and decodes it as untrusted. Asks for at most 8 icons per request and waits `retry_after_ms` after `rate_limited`.
18. Asks before `window.close` and before `app.launch` of a `risky` app.
19. Never puts Enter into `input.text`. Splits text above 1024 bytes. Shows `delivered_chars` after `focus_changed`, `locked`, `keyboard_grabbed` or `launcher_closed`. Offers target `launcher` only when `session.launcher.holder` is `this_device`.
20. Reconnects at once on `bye` `scopes_changed`.
