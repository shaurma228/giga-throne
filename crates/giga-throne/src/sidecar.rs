//! Lifecycle of the headless Throne process (sidecar).
//!
//! Launches `Throne --headless --data-dir <data>/throne --api-listen 127.0.0.1:9096
//! --api-key-file <data>/api.key`. The API key is generated on first start with
//! mode 0600. The binary is found via `GIGA_THRONE_SIDECAR`, then `sidecar/Throne/Throne`,
//! then next to our own executable. Exit codes and the watchdog are described in
//! `docs/throne-api/api.md` (section "Headless Throne").

// TODO: spawn, restart and stop the headless process; watchdog.
