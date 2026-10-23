//! Throne API wire protocol, version 1.
//!
//! Summary of `docs/throne-api/protocol.md`:
//!
//! - The connection is plain TCP. Both sides send a hello immediately:
//!   the server sends 16 random bytes `S`, the client sends `0x01 || C`
//!   with 16 random bytes `C`.
//! - Two direction keys are derived with HMAC-SHA256 over the shared key,
//!   a label (`"throne-api/1 c2s"` or `"throne-api/1 s2c"`), `S` and `C`.
//! - Each frame is `len` (u32 big-endian) followed by a ChaCha20-Poly1305
//!   sealed message. The nonce is four zero bytes followed by a per-direction
//!   counter starting at 0, and `len` is used as the AAD.
//! - Messages are JSON: requests carry `method`, `path`, optional `query`
//!   and `body`; responses carry `status` and either `body` or `error`.
//! - Requests and responses strictly alternate on one connection.

/// Default TCP port of a regular Throne instance.
pub const DEFAULT_PORT: u16 = 9095;

/// Protocol version byte sent in the client hello.
pub const PROTOCOL_VERSION: u8 = 1;

/// Largest frame length accepted from a client (client to server).
pub const MAX_CLIENT_FRAME: u32 = 1 << 20;

/// Largest frame length accepted from the server (server to client).
pub const MAX_SERVER_FRAME: u32 = 64 << 20;

/// Size of the Poly1305 authentication tag appended to every frame.
pub const TAG_SIZE: usize = 16;
