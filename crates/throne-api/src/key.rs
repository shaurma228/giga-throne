//! API key handling.
//!
//! The key is 32 random bytes, shared between Throne and the client. Its text
//! form is base64url without padding (43 characters).

/// A 32-byte Throne API key.
// TODO: parsing from base64url text, loading from a key file, and
// constant-time comparison.
// The field has no readers until the protocol code lands, so dead_code is allowed here only.
#[allow(dead_code)]
pub struct ApiKey([u8; 32]);
