//! Debug-only startup shortcuts for checking screens by hand and in screenshots.
//!
//! `GIGA_THRONE_PAGE` moves the app to a page once at startup: `0` Home (the
//! default), `1` Routes, `3` Servers, `2` the editor of a new route (the same path as
//! "Свой маршрут"), `2:<id>` the editor of route `<id>`. The editor needs the
//! engine, so the editor pages wait until it reports ready.
//! `GIGA_THRONE_EDITOR_MODE=advanced` starts that first editor in "Продвинуто".
//! `GIGA_THRONE_EDITOR_RULE=<index>` selects that rule of the first editor's list.
//! `GIGA_THRONE_DEMO_SERVERS=1` fills an engine with no servers with nine dummy
//! profiles in the group "Демо", once per run, to look at the Servers grid.
//! Release builds ignore these variables.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Weak};
use throne_api::types::{CreateGroupRequest, CreateGroupResponse, GroupsResponse, ImportResponse};
use throne_api::{Client, Method};
use tracing::{info, warn};

use crate::AppWindow;

/// Name of the group that `GIGA_THRONE_DEMO_SERVERS` fills.
const DEMO_GROUP: &str = "Демо";

/// Cities of the dummy profiles. Each is an HTTP proxy to a closed port: it never works.
const DEMO_CITIES: [&str; 9] = [
    "Амстердам",
    "Франкфурт",
    "Хельсинки",
    "Стамбул",
    "Варшава",
    "Нью-Йорк",
    "Токио",
    "Стокгольм",
    "Лондон",
];

/// Set once the demo servers were tried in this run.
static DEMO_TRIED: AtomicBool = AtomicBool::new(false);

/// How long the startup page waits for the engine before it gives up.
const ENGINE_WAIT: Duration = Duration::from_secs(60);
/// Pause between two checks of the engine status.
const POLL: Duration = Duration::from_millis(100);

thread_local! {
    /// Set at startup, taken by the first editor that opens.
    static START_ADVANCED: Cell<bool> = const { Cell::new(false) };
    /// Rule to select in that same editor, from `GIGA_THRONE_EDITOR_RULE`.
    static START_RULE: Cell<Option<usize>> = const { Cell::new(None) };
}

/// A page from `GIGA_THRONE_PAGE` that is not Home.
#[derive(Debug, PartialEq)]
enum StartPage {
    Routes,
    Servers,
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
    if let Ok(value) = std::env::var("GIGA_THRONE_EDITOR_RULE") {
        match value.parse::<usize>() {
            Ok(index) => START_RULE.with(|rule| rule.set(Some(index))),
            Err(_) => warn!("GIGA_THRONE_EDITOR_RULE ignored: {value:?} is not an index"),
        }
    }

    let Ok(value) = std::env::var("GIGA_THRONE_PAGE") else {
        return;
    };
    match parse_start_page(&value) {
        Ok(None) => {}
        Ok(Some(StartPage::Routes)) => window.set_page(1),
        Ok(Some(StartPage::Servers)) => window.set_page(3),
        Ok(Some(start)) => open_when_ready(window.as_weak(), start, Instant::now()),
        Err(err) => warn!("GIGA_THRONE_PAGE ignored: {err}"),
    }
}

/// Whether the editor that opens now should start in "Продвинуто". True only once,
/// for the editor requested by [`apply_startup`].
pub fn take_start_advanced() -> bool {
    START_ADVANCED.with(|flag| flag.replace(false))
}

/// The rule index from `GIGA_THRONE_EDITOR_RULE`, once: for the editor that opens
/// now. `None` when the variable is unset or the editor is not the first one.
pub fn take_start_rule() -> Option<usize> {
    START_RULE.with(|rule| rule.take())
}

/// Reads the value of `GIGA_THRONE_PAGE`: `None` for Home, otherwise the page.
fn parse_start_page(value: &str) -> Result<Option<StartPage>, String> {
    match value {
        "0" => Ok(None),
        "1" => Ok(Some(StartPage::Routes)),
        "3" => Ok(Some(StartPage::Servers)),
        "2" => Ok(Some(StartPage::NewRoute)),
        _ => {
            let id = value
                .strip_prefix("2:")
                .ok_or_else(|| format!("unknown page {value:?}, expected 0, 1, 2, 2:<id> or 3"))?;
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
            StartPage::Routes | StartPage::Servers => {}
        }
    });
}

/// Debug only: with `GIGA_THRONE_DEMO_SERVERS=1`, fills an engine that has no
/// servers with nine dummy profiles in the group "Демо". Runs once per process and
/// never when a "Демо" group already exists. Errors are logged, not returned.
pub async fn seed_demo_servers(client: &Client) {
    if !cfg!(debug_assertions)
        || std::env::var("GIGA_THRONE_DEMO_SERVERS").as_deref() != Ok("1")
        || DEMO_TRIED.swap(true, Ordering::Relaxed)
    {
        return;
    }
    if let Err(err) = seed(client).await {
        warn!("demo servers were not added: {err}");
    }
}

async fn seed(client: &Client) -> throne_api::Result<()> {
    let groups: GroupsResponse = client
        .request_json(Method::Get, "/v1/groups", None, None)
        .await?;
    let has_demo = groups.groups.iter().any(|group| group.name == DEMO_GROUP);
    let servers: u64 = groups.groups.iter().map(|group| group.profile_count).sum();
    if has_demo || servers > 0 {
        return Ok(());
    }
    let body = serde_json::to_value(CreateGroupRequest {
        name: DEMO_GROUP.to_owned(),
        url: None,
        auto_update: None,
        update: None,
        wait: None,
        timeout: None,
    })?;
    let created: CreateGroupResponse = client
        .request_json(Method::Post, "/v1/groups", None, Some(body))
        .await?;
    let text = DEMO_CITIES
        .iter()
        .map(|city| format!("http://127.0.0.1:9#{city}"))
        .collect::<Vec<_>>()
        .join("\n");
    let imported: ImportResponse = client
        .request_json(
            Method::Post,
            "/v1/import",
            None,
            Some(serde_json::json!({
                "as": "profiles",
                "text": text,
                "group": created.group.id,
            })),
        )
        .await?;
    info!(
        "added {} demo servers to {DEMO_GROUP:?}",
        imported.imported.unwrap_or(0)
    );
    Ok(())
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
        assert_eq!(parse_start_page("3"), Ok(Some(StartPage::Servers)));
    }

    #[test]
    fn bad_start_page_values_are_refused() {
        assert!(parse_start_page("4").is_err());
        assert!(parse_start_page("2:").is_err());
        assert!(parse_start_page("2:x").is_err());
        assert!(parse_start_page("").is_err());
    }
}
