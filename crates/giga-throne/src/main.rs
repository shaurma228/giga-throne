//! giga-throne: desktop UI for the Throne engine.

mod app;
mod sidecar;

use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let window = AppWindow::new()?;
    window.run()?;
    Ok(())
}
