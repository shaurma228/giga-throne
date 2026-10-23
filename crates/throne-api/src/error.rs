//! Error types returned by the Throne API client.

use thiserror::Error;

/// Errors produced by the Throne API client.
#[derive(Debug, Error)]
pub enum Error {
    /// Underlying I/O failure on the TCP connection or a key file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The API key is malformed: wrong base64url text or not 32 bytes.
    #[error("invalid API key")]
    InvalidKey,

    /// The peer violated the wire protocol (bad hello, bad frame, bad JSON).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// A frame failed authentication. The connection must be closed.
    #[error("frame failed authentication")]
    FrameAuth,

    /// A frame length prefix is larger than the direction allows.
    #[error("frame of {len} bytes exceeds the limit of {max} bytes")]
    FrameTooLarge {
        /// Length announced by the frame or required by the message.
        len: usize,
        /// Largest length allowed in this direction.
        max: u32,
    },

    /// A frame length prefix is smaller than the authentication tag.
    #[error("frame of {len} bytes is shorter than the authentication tag")]
    FrameTooShort {
        /// Length announced by the frame.
        len: u32,
    },

    /// A per-direction frame counter reached its last value. The connection
    /// must be closed rather than let the counter wrap.
    #[error("frame counter exhausted")]
    CounterExhausted,

    /// A cryptographic operation failed, for example the OS RNG.
    #[error("cryptographic failure: {0}")]
    Crypto(String),

    /// A request path does not start with `/` or contains a query or fragment.
    #[error("invalid request path: {0}")]
    InvalidPath(String),

    /// A message could not be encoded or decoded as JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The connection or a request did not finish in time.
    #[error("timed out")]
    Timeout,

    /// The peer closed the connection.
    #[error("connection closed by Throne")]
    Closed,

    /// Throne answered the request with an error status.
    #[error("Throne API error {status} ({code}): {message}")]
    Api {
        /// HTTP-like status code returned by Throne.
        status: u16,
        /// Stable, machine-readable error code.
        code: String,
        /// Human-readable message, may change between versions.
        message: String,
    },
}

/// Convenience alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;
