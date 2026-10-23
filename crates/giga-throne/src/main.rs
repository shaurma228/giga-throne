//! giga-throne: desktop UI for the Throne engine.

mod app;
mod engine;
mod sidecar;
mod view;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;

    let window = AppWindow::new()?;
    let commands = engine::bind_callbacks(&window);
    let engine = app::start_engine(&runtime, &window, commands);
    let result = window.run();
    app::stop_engine(&runtime, engine);
    result?;
    Ok(())
}
