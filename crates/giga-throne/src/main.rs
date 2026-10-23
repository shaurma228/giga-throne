//! giga-throne: desktop UI for the Throne engine.

mod app;
mod engine;
mod prefs;
mod sidecar;
mod sysproxy;
mod view;

use std::sync::Arc;

use anyhow::Context;
use sysproxy::SystemProxy;
use tracing::warn;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;

    let data_root = sidecar::default_data_root()?;
    let sysproxy = Arc::new(SystemProxy::new(&data_root));
    // A previous run may have died with our proxy still set.
    match sysproxy.recover() {
        Ok(true) => warn!("restored the system proxy left by a previous run"),
        Ok(false) => {}
        Err(err) => warn!("could not restore the system proxy: {err:#}"),
    }

    let window = AppWindow::new()?;
    let commands = engine::bind_callbacks(&window);
    let engine = app::start_engine(
        &runtime,
        &window,
        commands,
        Arc::clone(&sysproxy),
        data_root,
    );
    let result = window.run();
    // Before the engine stops: the system proxy must not outlive Throne.
    if let Err(err) = sysproxy.disable() {
        warn!("could not restore the system proxy: {err:#}");
    }
    app::stop_engine(&runtime, engine);
    result?;
    Ok(())
}

