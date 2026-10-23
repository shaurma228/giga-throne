//! Lifecycle of the headless Throne process (the "sidecar").
//!
//! Runs `Throne --headless --data-dir <root>/throne --api-listen 127.0.0.1:<port>
//! --api-key-file <root>/api.key --restart-mode exit --log-format plain`, restarts it
//! according to its exit code and stops it on request. Exit codes are described in
//! `docs/throne-api/api.md`, section "Exit statuses".

mod config;
mod policy;
mod supervisor;

pub use config::{default_data_root, SidecarConfig};
pub use supervisor::{Sidecar, SidecarHandle, SidecarState};
