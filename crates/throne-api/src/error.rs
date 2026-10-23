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
