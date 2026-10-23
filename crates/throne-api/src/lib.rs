//! Client for the Throne API.
//!
//! Throne exposes its engine over a small encrypted TCP protocol (see
//! `docs/throne-api/protocol.md`) and a set of JSON requests (see
//! `docs/throne-api/api.md`).
//!
//! The crate is layered:
//!
//! - [`protocol`] is pure: key derivation, hellos, and frame sealing and
//!   opening. It performs no I/O.
//! - [`client`] is the async TCP client built on tokio. It owns the
//!   connection, reconnects after idle timeouts, and maps responses to
//!   [`Error::Api`].
//! - [`message`] holds the JSON request and response envelopes.
//! - [`types`] holds the typed objects and request/response bodies of `api.md`.
//!
//! Typical use:
//!
//! ```no_run
//! # async fn run() -> throne_api::Result<()> {
//! use throne_api::{ApiKey, Client};
//!
//! let key: ApiKey = "<43-character key>".parse()?;
//! let client = Client::new("127.0.0.1:9095", key);
//! let body = client.get("/v1/status").await?;
//! # let _ = body;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod error;
pub mod key;
pub mod message;
pub mod protocol;
pub mod types;

pub use client::Client;
pub use error::{Error, Result};
pub use key::ApiKey;
pub use message::{ApiErrorBody, Method, Request, Response};
