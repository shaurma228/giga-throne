//! Application state: links the Slint UI to the Throne API.
//!
//! Network calls run on tokio; results reach the UI through
//! `slint::invoke_from_event_loop`.

// TODO: connection state, status and statistics polling, UI callbacks.
