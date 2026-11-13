//! The numbers of `docs/PROTOCOL.md` section 14, plus the fixed identifiers of the protocol.

/// ALPN protocol id of the `tls-frames` binding (spec 3.1).
pub const ALPN_ID: &[u8] = b"swayconnect/1";
/// The only message-layer version this crate speaks (spec 5.6).
pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 28414;

/// Largest value of the type `int` (spec 2.2): 2^53 - 1.
pub const MAX_INT: u64 = 9_007_199_254_740_991;

pub const MAX_FRAME_C2S_BEFORE_WELCOME_BYTES: u32 = 4096;
pub const MAX_FRAME_C2S_BYTES: u32 = 16_384;
/// Server to client, up to and including `welcome`.
pub const MAX_FRAME_S2C_HANDSHAKE_BYTES: u32 = 65_536;
pub const MAX_FRAME_S2C_BYTES: u32 = 1_048_576;

pub const TLS_HANDSHAKE_DEADLINE_MS: u64 = 5_000;
/// Deadline for `pair` or `auth`, from `hello`.
pub const FIRST_FRAME_DEADLINE_MS: u64 = 2_000;
pub const IDLE_TIMEOUT_MS: u64 = 90_000;
pub const CLIENT_KEEPALIVE_INTERVAL_MS: u64 = 30_000;
pub const CLIENT_REQUEST_TIMEOUT_MS: u64 = 10_000;
pub const SWAY_ANSWER_TIMEOUT_MS: u64 = 2_000;
pub const PAIRING_WINDOW_LIFETIME_S: u64 = 300;
pub const PAIRING_CONFIRMATION_DEADLINE_MS: u64 = 60_000;

pub const MAX_PENDING_REQUESTS: usize = 8;
pub const MAX_TOPICS_PER_SUBSCRIBE: usize = 16;
pub const MAX_CONNECTIONS: usize = 16;
pub const MAX_CONNECTIONS_BEFORE_WELCOME: usize = 8;

/// New connections per peer address: token bucket.
pub const CONNECTION_BURST: u32 = 5;
pub const CONNECTION_REFILL_INTERVAL_MS: u64 = 2_000;
/// Actions per device: token bucket.
pub const ACTION_BURST: u32 = 5;
pub const ACTION_REFILL_PER_SECOND: u32 = 2;

pub const MAX_WORKSPACES: usize = 256;
pub const MAX_WINDOWS: usize = 512;
pub const MAX_DESKTOP_BYTES: usize = 262_144;

/// `device_name`, `server.name`, `client` and the URI `name` (spec 4, 5.2).
pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_PLATFORM_BYTES: usize = 32;
/// Window titles (spec 9.3) and the text of `sway_error` (spec 10.1), in Unicode scalar values.
pub const MAX_TITLE_SCALARS: usize = 200;
/// Workspace `name`, `output` and `app_id` (spec 9.2, 9.3), in Unicode scalar values.
pub const MAX_LABEL_SCALARS: usize = 128;
pub const MAX_WORKSPACE_NUMBER: u64 = 2_147_483_647;
/// Largest `number` argument of `workspace.focus` (spec 7.2).
pub const MAX_FOCUS_NUMBER: u16 = 999;

/// Version 1 additive extensions (spec 17.14, 17.17).
///
/// `icon.get`: hashes per request.
pub const MAX_ICON_HASHES: usize = 8;
/// Largest PNG a server sends, before encoding.
pub const MAX_ICON_PNG_BYTES: usize = 65_536;
/// Largest PNG side a server sends, in pixels.
pub const MAX_ICON_SIDE_PX: u32 = 128;
/// Largest PNG side a client accepts, in pixels (17.5 rule 3).
pub const MAX_CLIENT_ICON_SIDE_PX: u32 = 256;
/// Rate class `icons` per device: token bucket.
pub const ICON_BURST: u32 = 4;
pub const ICON_REFILL_PER_SECOND: u32 = 2;
/// Outputs in topic `desktop`.
pub const MAX_OUTPUTS: usize = 16;
/// Container levels of a `Layout`.
pub const MAX_LAYOUT_DEPTH: usize = 16;
/// Upper end of a basis-point value of `Tile.r`.
pub const MAX_BASIS_POINTS: u16 = 10_000;
