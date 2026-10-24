//! Debug-only startup shortcuts for checking screens by hand and in screenshots.
//!
//! `GIGA_THRONE_PAGE` moves the app to a page once at startup: `0` Home (the
//! default), `1` Routes, `2` the editor of a new route (the same path as
//! "Свой маршрут"), `2:<id>` the editor of route `<id>`. The editor needs the
//! engine, so the editor pages wait until it reports ready.
//! `GIGA_THRONE_EDITOR_MODE=advanced` starts that first editor in "Продвинуто".
//! Release builds ignore both variables.

use std::cell::Cell;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Weak};
use tracing::warn;

use crate::AppWindow;

/// How long the startup page waits for the engine before it gives up.
const ENGINE_WAIT: Duration = Duration::from_secs(60);
/// Pause between two checks of the engine status.
const POLL: Duration = Duration::from_millis(100);

thread_local! {
    /// Set at startup, taken by the first editor that opens.
    static START_ADVANCED: Cell<bool> = const { Cell::new(false) };
}

/// A page from `GIGA_THRONE_PAGE` that is not Home.
#[derive(Debug, PartialEq)]
enum StartPage {
    Routes,
    NewRoute,
    Route(i32),
}

/// Applies `GIGA_THRONE_PAGE` and `GIGA_THRONE_EDITOR_MODE` to a debug build's
/// first screen. Call it on the UI thread before the event loop runs.
pub fn apply_startup(window: &AppWindow) {
    if !cfg!(debug_assertions) {
        return;
    }
    let advanced = std::env::var("GIGA_THRONE_EDITOR_MODE").is_ok_and(|mode| mode == "advanced");
    START_ADVANCED.with(|flag| flag.set(advanced));

    let Ok(value) = std::env::var("GIGA_THRONE_PAGE") else {
        return;
    };
    match parse_start_page(&value) {
        Ok(None) => {}
        Ok(Some(StartPage::Routes)) => window.set_page(1),
        Ok(Some(start)) => open_when_ready(window.as_weak(), start, Instant::now()),
        Err(err) => warn!("GIGA_THRONE_PAGE ignored: {err}"),
    }
}

/// Whether the editor that opens now should start in "Продвинуто". True only once,
/// for the editor requested by [`apply_startup`].
pub fn take_start_advanced() -> bool {
    START_ADVANCED.with(|flag| flag.replace(false))
}

/// Reads the value of `GIGA_THRONE_PAGE`: `None` for Home, otherwise the page.
fn parse_start_page(value: &str) -> Result<Option<StartPage>, String> {
    match value {
        "0" => Ok(None),
        "1" => Ok(Some(StartPage::Routes)),
        "2" => Ok(Some(StartPage::NewRoute)),
        _ => {
            let id = value
                .strip_prefix("2:")
                .ok_or_else(|| format!("unknown page {value:?}, expected 0, 1, 2 or 2:<id>"))?;
            let id = id
                .parse::<i32>()
                .map_err(|_| format!("bad route id {id:?}"))?;
            Ok(Some(StartPage::Route(id)))
        }
    }
}

/// Opens the editor once the engine status line says it is ready. The status line
/// is the only readiness signal on the UI side (see `app::engine_status_text`).
fn open_when_ready(weak: Weak<AppWindow>, start: StartPage, started: Instant) {
    slint::Timer::single_shot(POLL, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !window.get_engine_status().starts_with("Движок готов") {
            if started.elapsed() < ENGINE_WAIT {
                open_when_ready(weak, start, started);
            } else {
                warn!("GIGA_THRONE_PAGE: the engine was not ready in time, staying on Home");
            }
            return;
        }
        match start {
            StartPage::NewRoute => window.invoke_create_route(),
            StartPage::Route(id) => window.invoke_edit_route(id),
            StartPage::Routes => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_page_values_parse() {
        assert_eq!(parse_start_page("0"), Ok(None));
        assert_eq!(parse_start_page("1"), Ok(Some(StartPage::Routes)));
        assert_eq!(parse_start_page("2"), Ok(Some(StartPage::NewRoute)));
        assert_eq!(parse_start_page("2:7"), Ok(Some(StartPage::Route(7))));
    }

    #[test]
    fn bad_start_page_values_are_refused() {
        assert!(parse_start_page("3").is_err());
        assert!(parse_start_page("2:").is_err());
        assert!(parse_start_page("2:x").is_err());
        assert!(parse_start_page("").is_err());
    }
}
