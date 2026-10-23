//! Application state: links the Slint UI to the Throne API.
//!
//! Network calls run on tokio; results reach the UI through
//! `slint::Weak::upgrade_in_event_loop`, so the UI thread never waits on them.

use std::time::Duration;

use anyhow::Result;
use slint::ComponentHandle;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{oneshot, watch};
use tokio::time::timeout;
use tracing::warn;

use crate::engine::{self, Command, Engine};
use crate::sidecar::{Sidecar, SidecarConfig, SidecarHandle, SidecarState};
use crate::view;
use crate::AppWindow;

/// Upper bound for stopping the engine when the window closes.
const ENGINE_STOP_TIMEOUT: Duration = Duration::from_secs(7);

/// Text of the engine status line for a sidecar state.
pub fn engine_status_text(state: &SidecarState) -> String {
    match state {
        SidecarState::Starting => "Движок запускается…".to_owned(),
        SidecarState::Ready { version } => format!("Движок готов · Throne {version}"),
        SidecarState::Restarting { attempt: 0, .. } => "Перезапуск движка…".to_owned(),
        SidecarState::Restarting { attempt, .. } => format!("Перезапуск движка ({attempt})…"),
        SidecarState::Failed { reason } => format!("Ошибка движка: {reason}"),
        SidecarState::Stopped => "Движок остановлен".to_owned(),
    }
}

/// Starts the engine on `runtime` without blocking the caller, and keeps the
/// engine status line of `window` up to date.
///
/// Once the sidecar exists, the engine controller runs `commands` against it.
/// The receiver yields the handle once the engine has been started, so that
/// [`stop_engine`] can shut it down. If the start fails, the error is shown in
/// the status line and the receiver is closed.
pub fn start_engine(
    runtime: &tokio::runtime::Runtime,
    window: &AppWindow,
    commands: UnboundedReceiver<Command>,
) -> oneshot::Receiver<SidecarHandle> {
    let (handle_tx, handle_rx) = oneshot::channel();
    let weak = window.as_weak();
    runtime.spawn(async move {
        match launch().await {
            Ok((handle, api_port)) => {
                tokio::spawn(watch_status(weak.clone(), handle.state()));
                let sink_weak = weak.clone();
                let engine = Engine::new(
                    handle.client(),
                    api_port,
                    Box::new(move |frame| view::push(&sink_weak, frame)),
                );
                tokio::spawn(engine::run(engine, handle.state(), commands));
                // Only fails if `stop_engine` is already gone; the handle is then
                // dropped, which stops the engine too.
                let _ = handle_tx.send(handle);
            }
            Err(err) => {
                warn!("engine did not start: {err:#}");
                set_status(&weak, format!("Ошибка движка: {err:#}"));
            }
        }
    });
    handle_rx
}

/// Stops the engine started by [`start_engine`], waiting at most [`ENGINE_STOP_TIMEOUT`].
pub fn stop_engine(runtime: &tokio::runtime::Runtime, handle_rx: oneshot::Receiver<SidecarHandle>) {
    // `timeout` must be created inside the runtime: it registers with its timer.
    let stopped = runtime.block_on(async {
        timeout(ENGINE_STOP_TIMEOUT, async {
            if let Ok(handle) = handle_rx.await {
                handle.shutdown().await;
            }
        })
        .await
    });
    if stopped.is_err() {
        warn!("engine did not stop within {ENGINE_STOP_TIMEOUT:?}");
    }
}

/// Starts the sidecar. Returns the handle and our API port.
async fn launch() -> Result<(SidecarHandle, u16)> {
    let config = SidecarConfig::from_env()?;
    let port = config.port;
    let handle = Sidecar::start(config).await?;
    Ok((handle, port))
}

/// Mirrors every state change into the status line until the engine is dropped.
async fn watch_status(weak: slint::Weak<AppWindow>, mut state: watch::Receiver<SidecarState>) {
    loop {
        let text = engine_status_text(&state.borrow_and_update());
        set_status(&weak, text);
        if state.changed().await.is_err() {
            break;
        }
    }
}

fn set_status(weak: &slint::Weak<AppWindow>, text: String) {
    // Fails only after the event loop has exited, when nobody is left to show it.
    let _ = weak.upgrade_in_event_loop(move |window| window.set_engine_status(text.into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_covers_every_state() {
        assert_eq!(
            engine_status_text(&SidecarState::Starting),
            "Движок запускается…"
        );
        assert_eq!(
            engine_status_text(&SidecarState::Ready {
                version: "1.4.0-beta.4".into()
            }),
            "Движок готов · Throne 1.4.0-beta.4"
        );
        assert_eq!(
            engine_status_text(&SidecarState::Restarting {
                attempt: 0,
                reason: String::new()
            }),
            "Перезапуск движка…"
        );
        assert_eq!(
            engine_status_text(&SidecarState::Restarting {
                attempt: 2,
                reason: String::new()
            }),
            "Перезапуск движка (2)…"
        );
        assert_eq!(
            engine_status_text(&SidecarState::Failed { reason: "x".into() }),
            "Ошибка движка: x"
        );
        assert_eq!(
            engine_status_text(&SidecarState::Stopped),
            "Движок остановлен"
        );
    }
}
