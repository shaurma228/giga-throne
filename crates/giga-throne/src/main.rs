//! giga-throne: desktop UI for the Throne engine.

mod app;
mod dev_aid;
mod editor_ui;
mod engine;
mod prefs;
mod presets;
mod route_editor;
mod servers;
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
    dev_aid::apply_startup(&window);
    let engine = app::start_engine(
        &runtime,
        &window,
        commands,
        Arc::clone(&sysproxy),
        data_root,
    );
    // Ctrl+C or a session logout would otherwise kill us with the system proxy still set.
    runtime.spawn(quit_on_signal());
    let result = window.run();
    // Before the engine stops: the system proxy must not outlive Throne.
    if let Err(err) = sysproxy.disable() {
        warn!("could not restore the system proxy: {err:#}");
    }
    app::stop_engine(&runtime, engine);
    result?;
    Ok(())
}

/// Leaves the event loop on SIGINT, SIGTERM or SIGHUP, so the normal shutdown path runs.
#[cfg(unix)]
async fn quit_on_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let (mut int, mut term, mut hup) = match (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) {
        (Ok(int), Ok(term), Ok(hup)) => (int, term, hup),
        _ => {
            warn!("could not listen for shutdown signals");
            return;
        }
    };
    tokio::select! {
        _ = int.recv() => {}
        _ = term.recv() => {}
        _ = hup.recv() => {}
    }
    warn!("signal received, shutting down");
    if let Err(err) = slint::quit_event_loop() {
        warn!("could not stop the event loop: {err}");
    }
}

#[cfg(not(unix))]
async fn quit_on_signal() {
    if tokio::signal::ctrl_c().await.is_ok() {
        if let Err(err) = slint::quit_event_loop() {
            warn!("could not stop the event loop: {err}");
        }
    }
}
