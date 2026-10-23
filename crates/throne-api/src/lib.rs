//! Client for the Throne API.
//!
//! Throne exposes its engine over a small encrypted TCP protocol (see
//! `docs/throne-api/protocol.md`) and a set of JSON requests (see
//! `docs/throne-api/api.md`). This crate provides the transport, the typed
//! request and response surface, and the error type shared by both.

pub mod client;
pub mod error;
pub mod key;
pub mod protocol;

pub use client::Client;
pub use error::{Error, Result};
pub use key::ApiKey;
