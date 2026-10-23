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
//!
//! This module is pure: it does no I/O. The async client in [`crate::client`]
//! feeds it bytes read from and written to the socket.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{Error, Result};
use crate::key::ApiKey;

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

/// Size of the random values `S` and `C` exchanged in the hellos.
pub const RANDOM_SIZE: usize = 16;

/// Size of the frame length prefix.
pub const FRAME_HEADER_SIZE: usize = 4;

/// Size of an API key in bytes.
pub const KEY_SIZE: usize = 32;

/// Size of the server hello (`S`).
pub const SERVER_HELLO_SIZE: usize = RANDOM_SIZE;

/// Size of the client hello (`0x01 || C`).
pub const CLIENT_HELLO_SIZE: usize = 1 + RANDOM_SIZE;

const LABEL_C2S: &[u8] = b"throne-api/1 c2s";
const LABEL_S2C: &[u8] = b"throne-api/1 s2c";

/// The two direction keys of one connection.
#[derive(Clone)]
pub struct SessionKeys {
    /// Key that protects frames sent by the client.
    pub c2s: [u8; KEY_SIZE],
    /// Key that protects frames sent by the server.
    pub s2c: [u8; KEY_SIZE],
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionKeys(<redacted>)")
    }
}

/// Direction of a frame on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Frames sent by the client to the server.
    ClientToServer,
    /// Frames sent by the server to the client.
    ServerToClient,
}

impl Direction {
    /// Largest `len` a receiver accepts for frames in this direction.
    pub const fn max_frame_len(self) -> u32 {
        match self {
            Direction::ClientToServer => MAX_CLIENT_FRAME,
            Direction::ServerToClient => MAX_SERVER_FRAME,
        }
    }
}

/// Fills an array with bytes from the operating system's RNG.
pub fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|e| Error::Crypto(format!("OS RNG failed: {e}")))?;
    Ok(out)
}

/// Derives the two direction keys for one connection.
///
/// `k_c2s = HMAC-SHA256(K, "throne-api/1 c2s" || S || C)`, and `k_s2c` is the
/// same with the `s2c` label.
pub fn derive_keys(
    key: &ApiKey,
    server_random: &[u8; RANDOM_SIZE],
    client_random: &[u8; RANDOM_SIZE],
) -> SessionKeys {
    SessionKeys {
        c2s: derive_key(key, LABEL_C2S, server_random, client_random),
        s2c: derive_key(key, LABEL_S2C, server_random, client_random),
    }
}

fn derive_key(
    key: &ApiKey,
    label: &[u8],
    server_random: &[u8; RANDOM_SIZE],
    client_random: &[u8; RANDOM_SIZE],
) -> [u8; KEY_SIZE] {
    // HMAC accepts keys of any length, so this constructor cannot fail.
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(label);
    mac.update(server_random);
    mac.update(client_random);
    mac.finalize().into_bytes().into()
}

/// Builds the client hello `0x01 || C`.
pub fn client_hello(client_random: &[u8; RANDOM_SIZE]) -> [u8; CLIENT_HELLO_SIZE] {
    let mut hello = [0u8; CLIENT_HELLO_SIZE];
    hello[0] = PROTOCOL_VERSION;
    hello[1..].copy_from_slice(client_random);
    hello
}

/// Checks a client hello received by a server and returns `C`.
///
/// Fails when the version byte is not [`PROTOCOL_VERSION`].
pub fn check_client_hello(hello: &[u8; CLIENT_HELLO_SIZE]) -> Result<[u8; RANDOM_SIZE]> {
    if hello[0] != PROTOCOL_VERSION {
        return Err(Error::Protocol(format!(
            "unsupported protocol version {}",
            hello[0]
        )));
    }
    let mut client_random = [0u8; RANDOM_SIZE];
    client_random.copy_from_slice(&hello[1..]);
    Ok(client_random)
}

/// Checks a frame length against the lower bound and the direction's limit.
///
/// Must run before the body of the frame is read.
pub fn check_length(len: u32, dir: Direction) -> Result<()> {
    if len < TAG_SIZE as u32 {
        return Err(Error::FrameTooShort { len });
    }
    if len > dir.max_frame_len() {
        return Err(Error::FrameTooLarge {
            len: len as usize,
            max: dir.max_frame_len(),
        });
    }
    Ok(())
}

fn nonce(counter: u64) -> Nonce {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    nonce.into()
}

fn cipher(key: &[u8; KEY_SIZE]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(key.into())
}

/// Seals one message into a full frame: `len || ciphertext || tag`.
///
/// Does not apply the direction limits; [`FrameSealer`] does.
pub fn seal_frame(dir_key: &[u8; KEY_SIZE], counter: u64, plaintext: &[u8]) -> Result<Vec<u8>> {
    let len = frame_len_for(plaintext.len());
    let header = len.to_be_bytes();
    let sealed = cipher(dir_key)
        .encrypt(
            &nonce(counter),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| Error::Crypto("sealing a frame failed".to_owned()))?;
    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + sealed.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&sealed);
    Ok(frame)
}

/// `len` for a plaintext of the given size. Saturates at `u32::MAX`, which
/// every direction limit rejects.
fn frame_len_for(plaintext_len: usize) -> u32 {
    let len = plaintext_len.saturating_add(TAG_SIZE);
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// Opens a frame body whose length prefix is `header`.
///
/// Checks that `header` matches the body size, then authenticates and decrypts
/// with the AEAD's own decryption. On failure no plaintext is returned.
/// Direction limits are not applied here; [`FrameOpener`] applies them.
pub fn open_frame(
    dir_key: &[u8; KEY_SIZE],
    counter: u64,
    header: [u8; FRAME_HEADER_SIZE],
    sealed: &[u8],
) -> Result<Vec<u8>> {
    let len = u32::from_be_bytes(header);
    if len < TAG_SIZE as u32 {
        return Err(Error::FrameTooShort { len });
    }
    if len as usize != sealed.len() {
        return Err(Error::Protocol(
            "frame length does not match the frame body".to_owned(),
        ));
    }
    cipher(dir_key)
        .decrypt(
            &nonce(counter),
            Payload {
                msg: sealed,
                aad: &header,
            },
        )
        .map_err(|_| Error::FrameAuth)
}

/// Seals outgoing frames for one direction, advancing its counter.
pub struct FrameSealer {
    dir: Direction,
    key: [u8; KEY_SIZE],
    counter: u64,
}

impl FrameSealer {
    /// Creates a sealer whose first frame uses counter 0.
    pub fn new(dir: Direction, key: [u8; KEY_SIZE]) -> Self {
        Self::with_counter(dir, key, 0)
    }

    /// Creates a sealer whose next frame uses `counter`.
    pub fn with_counter(dir: Direction, key: [u8; KEY_SIZE], counter: u64) -> Self {
        Self { dir, key, counter }
    }

    /// Seals `plaintext` into a frame and advances the counter.
    ///
    /// Fails without advancing when the frame would exceed the direction's
    /// limit, or when the counter is exhausted.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        if self.counter == u64::MAX {
            return Err(Error::CounterExhausted);
        }
        check_length(frame_len_for(plaintext.len()), self.dir)?;
        let frame = seal_frame(&self.key, self.counter, plaintext)?;
        self.counter = self.counter.checked_add(1).ok_or(Error::CounterExhausted)?;
        Ok(frame)
    }
}

impl std::fmt::Debug for FrameSealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSealer")
            .field("dir", &self.dir)
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

/// Opens incoming frames for one direction, advancing its counter.
pub struct FrameOpener {
    dir: Direction,
    key: [u8; KEY_SIZE],
    counter: u64,
}

impl FrameOpener {
    /// Creates an opener that expects counter 0 first.
    pub fn new(dir: Direction, key: [u8; KEY_SIZE]) -> Self {
        Self::with_counter(dir, key, 0)
    }

    /// Creates an opener that expects `counter` next.
    pub fn with_counter(dir: Direction, key: [u8; KEY_SIZE], counter: u64) -> Self {
        Self { dir, key, counter }
    }

    /// Checks a length prefix against the limits of this direction and
    /// returns the body length. Call it before reading the body.
    pub fn check_header(&self, header: [u8; FRAME_HEADER_SIZE]) -> Result<u32> {
        let len = u32::from_be_bytes(header);
        check_length(len, self.dir)?;
        Ok(len)
    }

    /// Opens a frame body and advances the counter.
    ///
    /// Any error means the connection must be closed: the counter is not
    /// advanced, so the caller should not try to read further frames.
    pub fn open(&mut self, header: [u8; FRAME_HEADER_SIZE], sealed: &[u8]) -> Result<Vec<u8>> {
        self.check_header(header)?;
        if self.counter == u64::MAX {
            return Err(Error::CounterExhausted);
        }
        let plaintext = open_frame(&self.key, self.counter, header, sealed)?;
        self.counter = self.counter.checked_add(1).ok_or(Error::CounterExhausted)?;
        Ok(plaintext)
    }
}

impl std::fmt::Debug for FrameOpener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameOpener")
            .field("dir", &self.dir)
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_SIZE] = [9; KEY_SIZE];

    #[test]
    fn check_length_bounds() {
        assert!(check_length(15, Direction::ClientToServer).is_err());
        assert!(check_length(16, Direction::ClientToServer).is_ok());
        assert!(check_length(MAX_CLIENT_FRAME, Direction::ClientToServer).is_ok());
        assert!(check_length(MAX_CLIENT_FRAME + 1, Direction::ClientToServer).is_err());
        assert!(check_length(MAX_SERVER_FRAME, Direction::ServerToClient).is_ok());
        assert!(check_length(MAX_SERVER_FRAME + 1, Direction::ServerToClient).is_err());
    }

    #[test]
    fn seal_then_open_roundtrip_advances_counters() {
        let mut sealer = FrameSealer::new(Direction::ClientToServer, KEY);
        let mut opener = FrameOpener::new(Direction::ClientToServer, KEY);
        for msg in [&b"first"[..], b"", b"third message"] {
            let frame = sealer.seal(msg).expect("seal");
            let header: [u8; 4] = frame[..4].try_into().expect("header");
            assert_eq!(
                opener.check_header(header).expect("len"),
                (frame.len() - 4) as u32
            );
            assert_eq!(opener.open(header, &frame[4..]).expect("open"), msg);
        }
    }

    #[test]
    fn replayed_frame_is_rejected() {
        let mut sealer = FrameSealer::new(Direction::ClientToServer, KEY);
        let mut opener = FrameOpener::new(Direction::ClientToServer, KEY);
        let frame = sealer.seal(b"hello").expect("seal");
        let header: [u8; 4] = frame[..4].try_into().expect("header");
        assert!(opener.open(header, &frame[4..]).is_ok());
        assert!(matches!(
            opener.open(header, &frame[4..]),
            Err(Error::FrameAuth)
        ));
    }

    #[test]
    fn tampered_frame_yields_no_plaintext() {
        let frame = seal_frame(&KEY, 0, b"secret").expect("seal");
        let header: [u8; 4] = frame[..4].try_into().expect("header");
        let mut body = frame[4..].to_vec();
        body[0] ^= 1;
        assert!(open_frame(&KEY, 0, header, &body).is_err());
    }

    #[test]
    fn counter_exhaustion_is_an_error() {
        let mut sealer = FrameSealer::with_counter(Direction::ServerToClient, KEY, u64::MAX);
        assert!(matches!(sealer.seal(b"x"), Err(Error::CounterExhausted)));
    }

    #[test]
    fn oversized_plaintext_is_rejected_by_sealer() {
        let mut sealer = FrameSealer::new(Direction::ClientToServer, KEY);
        let big = vec![0u8; MAX_CLIENT_FRAME as usize];
        assert!(matches!(
            sealer.seal(&big),
            Err(Error::FrameTooLarge { .. })
        ));
        // Nothing was sent, so the counter did not move.
        assert!(sealer.seal(b"ok").is_ok());
    }

    #[test]
    fn client_hello_checks_version() {
        let hello = client_hello(&[3; RANDOM_SIZE]);
        assert_eq!(hello[0], PROTOCOL_VERSION);
        assert_eq!(check_client_hello(&hello).expect("v1"), [3; RANDOM_SIZE]);
        let mut bad = hello;
        bad[0] = 2;
        assert!(check_client_hello(&bad).is_err());
    }
}
