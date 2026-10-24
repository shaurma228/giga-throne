//! Ready-made routing modes: the preset table, matching of routes to presets,
//! the plan for choosing one, and the classification of pasted import text.
//!
//! Everything here is pure. The engine runs the plan against the API.

use throne_api::types::{AddRouteResponse, Route};

/// Throne's built-in routing profile, the "Всё через VPN" mode.
const DEFAULT_ROUTE_NAME: &str = "Default";

/// Where the routeprofiles lists are published.
const LIST_URL_PREFIX: &str = "https://raw.githubusercontent.com/throneproj/routeprofiles/profile/";

/// Region ids, in the order of the region chips.
pub const REGIONS: [&str; 3] = ["ru", "cn", "ir"];

/// Region used when the preferences name none or an unknown one.
pub const DEFAULT_REGION: &str = "ru";

/// One ready-made routing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Unique id, used by `SelectPreset`.
    pub id: &'static str,
    /// Region the card is shown in.
    pub region: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    /// Name of the list, shown in mono under the card.
    pub source: &'static str,
    /// File name in the routeprofiles repository. `None` for Throne's built-in
    /// "Default" profile, which is not downloaded.
    pub file: Option<&'static str>,
    pub recommended: bool,
}

/// Every preset, grouped by region in the order the cards are shown.
pub const PRESETS: [Preset; 10] = [
    Preset {
        id: "ru-all",
        region: "ru",
        title: "Всё через VPN",
        desc: "Каждый сайт и приложение идут через сервер.",
        source: "Default",
        file: None,
        recommended: false,
    },
    Preset {
        id: "ru-blocked",
        region: "ru",
        title: "Только заблокированное",
        desc: "VPN только для сайтов, заблокированных в РФ. Остальное — напрямую и быстро.",
        source: "Proxy Russia Blocked",
        file: Some("Proxy_Russia_Blocked"),
        recommended: true,
    },
    Preset {
        id: "ru-bypass",
        region: "ru",
        title: "Всё, кроме России",
        desc: "Российские сайты, банки и госуслуги — напрямую, остальное через VPN.",
        source: "Bypass Russia",
        file: Some("Bypass_Russia"),
        recommended: false,
    },
    Preset {
        id: "ru-antizapret",
        region: "ru",
        title: "Антизапрет",
        desc: "Список сообщества Antizapret: блокировки и замедления.",
        source: "Proxy Antizapret",
        file: Some("Proxy_Antizapret"),
        recommended: false,
    },
    Preset {
        id: "ru-refilter",
        region: "ru",
        title: "Re:filter",
        desc: "Список Re:filter: заблокированные домены и IP.",
        source: "Proxy Refilter",
        file: Some("Proxy_Refilter"),
        recommended: false,
    },
    Preset {
        id: "cn-all",
        region: "cn",
        title: "Всё через VPN",
        desc: "Каждый сайт и приложение идут через сервер.",
        source: "Default",
        file: None,
        recommended: false,
    },
    Preset {
        id: "cn-blocked",
        region: "cn",
        title: "Только заблокированное",
        desc: "VPN только для сайтов за Великим файрволом.",
        source: "Proxy China Blocked",
        file: Some("Proxy_China_Blocked"),
        recommended: true,
    },
    Preset {
        id: "cn-bypass",
        region: "cn",
        title: "Всё, кроме Китая",
        desc: "Китайские сайты — напрямую, остальное через VPN.",
        source: "Bypass China",
        file: Some("Bypass_China"),
        recommended: false,
    },
    Preset {
        id: "ir-all",
        region: "ir",
        title: "Всё через VPN",
        desc: "Каждый сайт и приложение идут через сервер.",
        source: "Default",
        file: None,
        recommended: false,
    },
    Preset {
        id: "ir-bypass",
        region: "ir",
        title: "Всё, кроме Ирана",
        desc: "Иранские сайты — напрямую, остальное через VPN.",
        source: "Bypass Iran",
        file: Some("Bypass_Iran"),
        recommended: true,
    },
];

/// Looks a preset up by id.
pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// The presets of one region, in card order.
pub fn in_region(region: &str) -> impl Iterator<Item = &'static Preset> + '_ {
    PRESETS.iter().filter(move |preset| preset.region == region)
}

/// Whether `region` is one of [`REGIONS`].
pub fn is_region(region: &str) -> bool {
    REGIONS.contains(&region)
}

/// The region to show: `region` when known, otherwise [`DEFAULT_REGION`].
pub fn region_or_default(region: &str) -> &str {
    if is_region(region) {
        region
    } else {
        DEFAULT_REGION
    }
}

/// The list URL of a preset, or `None` for the built-in "Default" profile.
pub fn preset_url(preset: &Preset) -> Option<String> {
    preset.file.map(|file| format!("{LIST_URL_PREFIX}{file}"))
}

/// Canonical form of a URL for comparing: surrounding and trailing slashes
/// dropped, scheme and host lower-cased. The path keeps its case.
pub fn normalize_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (host, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        host.to_ascii_lowercase(),
        path.trim_end_matches('/')
    )
}

/// Whether `route` is Throne's built-in "Default" profile.
pub fn is_default_route(route: &Route) -> bool {
    route.name == DEFAULT_ROUTE_NAME
}

/// Whether `route` is the route of `preset`: the same remote URL, or the
/// "Default" profile for the "Всё через VPN" mode.
pub fn route_matches(route: &Route, preset: &Preset) -> bool {
    match preset_url(preset) {
        Some(url) => {
            !route.remote_url.is_empty() && normalize_url(&route.remote_url) == normalize_url(&url)
        }
        None => is_default_route(route),
    }
}

/// The preset a route stands for, if any.
pub fn preset_of(route: &Route) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| route_matches(route, preset))
}

/// The routes shown under "Мои маршруты": neither "Default" nor a preset.
pub fn my_routes(routes: &[Route]) -> Vec<&Route> {
    routes
        .iter()
        .filter(|route| !is_default_route(route) && preset_of(route).is_none())
        .collect()
}

/// What choosing a preset has to do, given the routes that exist now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetPlan {
    /// The route exists: make it active.
    Activate(i64),
    /// Add the remote list from this URL, then activate the new route.
    AddThenActivate(String),
    /// Nothing can be done (no "Default" profile).
    Unavailable,
}

/// Plans the activation of `preset` against `routes`.
pub fn plan_preset(routes: &[Route], preset: &Preset) -> PresetPlan {
    if let Some(route) = routes.iter().find(|route| route_matches(route, preset)) {
        return PresetPlan::Activate(route.id);
    }
    match preset_url(preset) {
        Some(url) => PresetPlan::AddThenActivate(url),
        None => PresetPlan::Unavailable,
    }
}

/// Which of the routes the preset's list was added as, and whether it can be
/// activated. `response` is the answer to adding `url`.
///
/// A failed first fetch keeps an empty profile, so it is never activated: the
/// error text is returned instead.
pub fn pick_added_route(response: &AddRouteResponse, url: &str) -> Result<i64, String> {
    let wanted = normalize_url(url);
    let route = response
        .routes
        .iter()
        .find(|route| normalize_url(&route.remote_url) == wanted)
        .ok_or_else(|| "Движок не вернул маршрут".to_owned())?;
    match response.results.iter().find(|result| result.id == route.id) {
        Some(result) if !result.ok => Err(format!(
            "Не удалось скачать список: {}",
            result.error.as_deref().unwrap_or("нет ответа")
        )),
        Some(_) => Ok(route.id),
        None if route.updated_at > 0 => Ok(route.id),
        None => Err("Список ещё загружается, попробуй через минуту".to_owned()),
    }
}

/// What the text pasted into the import box is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// A `throne://route/` link: one profile.
    Link,
    /// A `throne://remoteroute/` link or a single http(s) URL: remote profiles.
    Remote,
    /// Anything else.
    Invalid,
}

/// Classifies the text of the import box. Surrounding spaces are ignored.
pub fn classify_import(text: &str) -> ImportKind {
    let text = text.trim();
    let single_line_url = !text.contains(char::is_whitespace)
        && (starts_with_ignore_case(text, "https://") || starts_with_ignore_case(text, "http://"));
    if starts_with_ignore_case(text, "throne://route/") {
        ImportKind::Link
    } else if starts_with_ignore_case(text, "throne://remoteroute/") || single_line_url {
        ImportKind::Remote
    } else {
        ImportKind::Invalid
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use throne_api::types::RouteFetchResult;

    use super::*;

    fn route(id: i64, name: &str, remote_url: &str) -> Route {
        Route {
            id,
            name: name.to_owned(),
            active: false,
            remote: !remote_url.is_empty(),
            remote_url: remote_url.to_owned(),
            auto_update: !remote_url.is_empty(),
            updated_at: 0,
            raw: false,
            rule_count: 0,
        }
    }

    fn url_of(file: &str) -> String {
        format!("{LIST_URL_PREFIX}{file}")
    }

    #[test]
    fn preset_ids_are_unique_and_every_region_has_a_default_first() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        for region in REGIONS {
            let first = in_region(region).next().expect("region has presets");
            assert_eq!(first.file, None, "{region} starts with the Default mode");
            assert_eq!(first.title, "Всё через VPN");
            assert_eq!(in_region(region).filter(|p| p.recommended).count(), 1);
        }
    }

    #[test]
    fn russia_has_five_modes_and_the_others_three_or_two() {
        assert_eq!(in_region("ru").count(), 5);
        assert_eq!(in_region("cn").count(), 3);
        assert_eq!(in_region("ir").count(), 2);
    }

    #[test]
    fn preset_urls_follow_the_routeprofiles_layout() {
        let blocked = find("ru-blocked").expect("preset exists");
        assert_eq!(
            preset_url(blocked).as_deref(),
            Some("https://raw.githubusercontent.com/throneproj/routeprofiles/profile/Proxy_Russia_Blocked")
        );
        assert_eq!(preset_url(find("ru-all").expect("preset")), None);
        assert!(find("nope").is_none());
    }

    #[test]
    fn region_falls_back_to_russia() {
        assert_eq!(region_or_default("cn"), "cn");
        assert_eq!(region_or_default("xx"), "ru");
        assert_eq!(region_or_default(""), "ru");
    }

    #[test]
    fn urls_match_ignoring_trailing_slash_and_host_case() {
        let want = url_of("Bypass_Russia");
        assert_eq!(
            normalize_url(&format!("{want}/")),
            normalize_url(&want.replace("raw.githubusercontent", "RAW.GitHubUserContent"))
        );
        assert_eq!(
            normalize_url("  https://Example.COM/A/ "),
            "https://example.com/A"
        );
        // The path keeps its case.
        assert_ne!(
            normalize_url("https://e.com/A"),
            normalize_url("https://e.com/a")
        );
    }

    #[test]
    fn a_route_is_a_preset_by_its_remote_url() {
        let bypass = find("ru-bypass").expect("preset");
        let mine = route(2, "Почта", "https://example.com/list");
        assert_eq!(preset_of(&mine), None);
        let same = route(3, "Bypass", &format!("{}/", url_of("Bypass_Russia")));
        assert_eq!(preset_of(&same).map(|p| p.id), Some(bypass.id));
    }

    #[test]
    fn default_route_is_the_all_mode() {
        let all = find("cn-all").expect("preset");
        assert!(route_matches(&route(1, "Default", ""), all));
        assert!(!route_matches(&route(1, "Other", ""), all));
    }

    #[test]
    fn my_routes_are_neither_default_nor_presets() {
        let routes = vec![
            route(1, "Default", ""),
            route(2, "Работа", ""),
            route(3, "Антизапрет", &url_of("Proxy_Antizapret")),
            route(4, "Игры", "https://example.com/games"),
        ];
        let ids: Vec<i64> = my_routes(&routes).iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![2, 4]);
    }

    #[test]
    fn plan_activates_an_existing_route_or_adds_the_list() {
        let blocked = find("ru-blocked").expect("preset");
        let routes = vec![
            route(1, "Default", ""),
            route(5, "x", &url_of("Proxy_Russia_Blocked")),
        ];
        assert_eq!(plan_preset(&routes, blocked), PresetPlan::Activate(5));
        assert_eq!(
            plan_preset(&routes[..1], blocked),
            PresetPlan::AddThenActivate(url_of("Proxy_Russia_Blocked"))
        );
    }

    #[test]
    fn plan_for_the_all_mode_needs_the_default_profile() {
        let all = find("ru-all").expect("preset");
        assert_eq!(
            plan_preset(&[route(1, "Default", "")], all),
            PresetPlan::Activate(1)
        );
        assert_eq!(
            plan_preset(&[route(2, "Работа", "")], all),
            PresetPlan::Unavailable
        );
    }

    fn added(routes: Vec<Route>, results: Vec<RouteFetchResult>) -> AddRouteResponse {
        AddRouteResponse {
            routes,
            results,
            warnings: String::new(),
        }
    }

    #[test]
    fn added_list_is_activated_only_when_its_fetch_worked() {
        let url = url_of("Proxy_Refilter");
        let new = route(7, "Re:filter", &url);
        let ok = added(
            vec![new.clone()],
            vec![RouteFetchResult {
                id: 7,
                ok: true,
                error: None,
                updated_at: Some(100),
            }],
        );
        assert_eq!(pick_added_route(&ok, &url), Ok(7));

        let failed = added(
            vec![new.clone()],
            vec![RouteFetchResult {
                id: 7,
                ok: false,
                error: Some("404".to_owned()),
                updated_at: None,
            }],
        );
        assert_eq!(
            pick_added_route(&failed, &url),
            Err("Не удалось скачать список: 404".to_owned())
        );

        let waiting = added(vec![new], Vec::new());
        assert!(pick_added_route(&waiting, &url).is_err());
        assert!(pick_added_route(&added(Vec::new(), Vec::new()), &url).is_err());
    }

    #[test]
    fn import_text_is_classified() {
        assert_eq!(classify_import("  throne://route/eyJ "), ImportKind::Link);
        assert_eq!(classify_import("THRONE://route/eyJ"), ImportKind::Link);
        assert_eq!(
            classify_import("throne://remoteroute/aHR0"),
            ImportKind::Remote
        );
        assert_eq!(
            classify_import("https://example.com/list"),
            ImportKind::Remote
        );
        assert_eq!(
            classify_import("http://example.com/x\n"),
            ImportKind::Remote
        );
        assert_eq!(
            classify_import("https://a.com https://b.com"),
            ImportKind::Invalid
        );
        assert_eq!(
            classify_import("vless://uuid@host:443#x"),
            ImportKind::Invalid
        );
        assert_eq!(classify_import("   "), ImportKind::Invalid);
        assert_eq!(classify_import("ftp://example.com"), ImportKind::Invalid);
    }
}

#[cfg(test)]
mod e2e {
    //! Tests against the real Throne from `sidecar/`, one Throne per test.
    //!
    //! Run with `cargo test -p giga-throne e2e -- --ignored --nocapture`. The
    //! remote tests need network access to raw.githubusercontent.com.

    use std::future::Future;
    use std::net::{TcpListener, TcpStream};
    use std::panic::resume_unwind;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::{json, Value};
    use throne_api::types::{Route, RouteRule, RouteShare, Status, UpdateRouteRequest};
    use throne_api::{decode_route_link, Client, Error, Method};
    use tokio::sync::watch;
    use tokio::time::timeout;

    use crate::engine::{Command, Engine};
    use crate::sidecar::{Sidecar, SidecarConfig, SidecarHandle, SidecarState};
    use crate::sysproxy::runner::FakeRunner;
    use crate::sysproxy::SystemProxy;

    use super::*;

    /// Time allowed for Throne to answer the API after the start.
    const READY_TIMEOUT: Duration = Duration::from_secs(30);

    /// Hard limit of one test, start and shutdown included.
    const TEST_TIMEOUT: Duration = Duration::from_secs(90);

    /// Bypass Russia, from a real `throne://route/` link.
    const BYPASS_RUSSIA_LINK: &str = "throne://route/eyJkZWZhdWx0X291dGJvdW5kIjoicHJveHkiLCJraW5kIjoidGhyb25lLXJvdXRlLXByb2ZpbGUiLCJuYW1lIjoiQnlwYXNzIFJ1c3NpYSIsInJ1bGVzIjpbeyJhY3Rpb24iOiJoaWphY2stZG5zIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzEiLCJwcm90b2NvbCI6ImRucyIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwiaXBfaXNfcHJpdmF0ZSI6dHJ1ZSwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzIiLCJvdXRib3VuZCI6ImRpcmVjdCIsInR5cGUiOiJjdXN0b20ifSx7ImFjdGlvbiI6InJvdXRlIiwibmFtZSI6ImltcG9ydGVkIHJ1bGUgIzMiLCJvdXRib3VuZCI6ImRpcmVjdCIsInJ1bGVfc2V0IjpbImdlb2lwLXJ1IiwiZ2Vvc2l0ZS1ydSIsImdlb3NpdGUtY2F0ZWdvcnktcnUiLCJnZW9zaXRlLXJ1LWF2YWlsYWJsZS1vbmx5LWluc2lkZSJdLCJ0eXBlIjoiY3VzdG9tIn1dLCJ2IjoxfQ";
    /// Four Russia lists, from a real `throne://remoteroute/` link.
    const REMOTE_ROUTES_LINK: &str = "throne://remoteroute/aHR0cHM6Ly9yYXcuZ2l0aHVidXNlcmNvbnRlbnQuY29tL3Rocm9uZXByb2ovcm91dGVwcm9maWxlcy9wcm9maWxlL0J5cGFzc19SdXNzaWEjQnlwYXNzIFJ1c3NpYQpodHRwczovL3Jhdy5naXRodWJ1c2VyY29udGVudC5jb20vdGhyb25lcHJvai9yb3V0ZXByb2ZpbGVzL3Byb2ZpbGUvUHJveHlfUnVzc2lhX0Jsb2NrZWQjUHJveHkgUnVzc2lhIEJsb2NrZWQKaHR0cHM6Ly9yYXcuZ2l0aHVidXNlcmNvbnRlbnQuY29tL3Rocm9uZXByb2ovcm91dGVwcm9maWxlcy9wcm9maWxlL1Byb3h5X0FudGl6YXByZXQjUHJveHkgQW50aXphcHJldApodHRwczovL3Jhdy5naXRodWJ1c2VyY29udGVudC5jb20vdGhyb25lcHJvai9yb3V0ZXByb2ZpbGVzL3Byb2ZpbGUvUHJveHlfUmVmaWx0ZXIjUHJveHkgUmVmaWx0ZXI";

    /// What a test body gets: the API client, our API port and the data root.
    struct Throne {
        client: Arc<Client>,
        port: u16,
        data: PathBuf,
    }

    /// Picks a port that is free right now.
    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("local addr").port()
    }

    /// Starts a fresh Throne, runs `body` against it and shuts Throne down.
    ///
    /// The body runs in its own task. A panic in it, or a timeout, still
    /// reaches the shutdown before the panic is re-raised.
    async fn with_throne<F, Fut>(body: F)
    where
        F: FnOnce(Throne) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let binary = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sidecar/Throne/Throne"
        ));
        assert!(binary.is_file(), "missing {}", binary.display());
        let dir = tempfile::tempdir().expect("temp dir");
        let port = free_port();
        let config = SidecarConfig {
            binary,
            data_root: dir.path().to_path_buf(),
            port,
        };

        let handle: SidecarHandle = Sidecar::start(config).await.expect("start sidecar");
        let mut state = handle.state();
        let throne = Throne {
            client: handle.client(),
            port,
            data: dir.path().to_path_buf(),
        };
        let client = Arc::clone(&throne.client);
        let mut task = tokio::spawn(async move {
            wait_ready(&mut state).await;
            move_ports(&client).await;
            body(throne).await;
        });

        let finished = timeout(TEST_TIMEOUT, &mut task).await;
        if finished.is_err() {
            task.abort();
        }
        handle.shutdown().await;
        let mut state = handle.state();
        let stopped = state
            .wait_for(|s| *s == SidecarState::Stopped)
            .await
            .expect("state channel open")
            .clone();
        assert_eq!(stopped, SidecarState::Stopped);

        match finished {
            Err(_) => panic!("the test did not finish within {TEST_TIMEOUT:?}"),
            Ok(Err(join)) if join.is_panic() => resume_unwind(join.into_panic()),
            Ok(Err(join)) => panic!("the test task was cancelled: {join}"),
            Ok(Ok(())) => {}
        }
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "Throne still listens after shutdown"
        );
    }

    /// Waits until Throne answers the API, or fails the test with a clear message.
    async fn wait_ready(state: &mut watch::Receiver<SidecarState>) {
        let ready = timeout(
            READY_TIMEOUT,
            state.wait_for(|s| !matches!(s, SidecarState::Starting)),
        )
        .await
        .unwrap_or_else(|_| panic!("Throne did not become ready within {READY_TIMEOUT:?}"))
        .expect("state channel open")
        .clone();
        match ready {
            SidecarState::Ready { version } => println!("Throne {version} is ready"),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    /// Gives this Throne its own inbound, DNS and Clash API ports. Tests run in
    /// parallel, and the default inbound port 2080 is often taken by another
    /// Throne, which makes the remote list fetches fail.
    async fn move_ports(client: &Client) {
        let ports = json!({
            "inbound_port": free_port(),
            "dns_in_port": free_port(),
            "clash_api_port": free_port(),
        });
        client
            .request(
                Method::Put,
                "/v1/settings",
                None,
                Some(json!({ "settings": ports })),
            )
            .await
            .expect("PUT /v1/settings with free ports");
    }

    /// The error code of an API error, or a panic for any other error.
    fn api_code(err: Error) -> String {
        match err {
            Error::Api { code, .. } => code,
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    fn route_named<'a>(routes: &'a [Route], name: &str) -> &'a Route {
        routes
            .iter()
            .find(|route| route.name == name)
            .unwrap_or_else(|| panic!("no route named {name:?} in {routes:?}"))
    }

    /// The rule-set lists of the rules of a share, in rule order.
    fn rule_sets_of(share: &RouteShare) -> Vec<Option<Vec<String>>> {
        share
            .rules
            .iter()
            .map(|rule| rule.rule_set.clone())
            .collect()
    }

    /// A custom rule that sends `example.com` through the proxy.
    fn custom_rule() -> RouteRule {
        serde_json::from_value(json!({
            "name": "Example",
            "type": "custom",
            "action": "route",
            "outbound": "proxy",
            "domain_suffix": ["example.com"]
        }))
        .expect("rule parses")
    }

    /// The names of the routes, in list order.
    fn route_list_names(routes: &[Route]) -> Vec<&str> {
        routes.iter().map(|route| route.name.as_str()).collect()
    }

    /// Lists the routes, failing the test on error.
    async fn routes_of(client: &Client) -> Vec<Route> {
        client.routes().await.expect("GET /v1/routes")
    }

    #[tokio::test]
    #[ignore = "needs the real Throne binary in sidecar/Throne"]
    async fn routes_api_real_throne() {
        with_throne(|throne| async move {
            let client = throne.client;

            // Listing: Default is there, and every route carries the new fields.
            let raw: Value = client
                .request_json(Method::Get, "/v1/routes", None, None)
                .await
                .expect("GET /v1/routes as JSON");
            for route in raw["routes"].as_array().expect("routes array") {
                for key in [
                    "remote",
                    "remote_url",
                    "auto_update",
                    "updated_at",
                    "raw",
                    "rule_count",
                ] {
                    assert!(route.get(key).is_some(), "{key} missing in {route}");
                }
            }
            let routes = routes_of(&client).await;
            println!("routes: {:?}", route_list_names(&routes));
            let default = route_named(&routes, "Default").clone();

            // Rule sets: sorted, and the Russia list is among them.
            let sets = client
                .rule_sets()
                .await
                .expect("GET /v1/rule-sets")
                .rule_sets;
            println!("rule sets: {} names", sets.len());
            assert!(!sets.is_empty(), "no rule sets");
            assert!(
                sets.windows(2).all(|pair| pair[0] <= pair[1]),
                "rule sets are not sorted"
            );
            assert!(
                sets.iter().any(|set| set == "geosite-ru-blocked"),
                "{sets:?}"
            );

            // Adding a link: three rules, the same rule sets as the link.
            let added = client
                .add_route_link(BYPASS_RUSSIA_LINK)
                .await
                .expect("POST /v1/routes with the Bypass Russia link");
            let id = route_named(&added.routes, "Bypass Russia").id;
            let got = client.route(id).await.expect("GET /v1/routes/{id}");
            let fixture = decode_route_link(BYPASS_RUSSIA_LINK).expect("fixture link decodes");
            assert_eq!(got.route.id, id);
            assert_eq!(got.profile.rules.len(), 3, "{:?}", got.profile);
            assert_eq!(rule_sets_of(&got.profile), rule_sets_of(&fixture));
            assert_eq!(got.route.rule_count, 3);

            // A rule with a domain suffix is added and comes back exactly.
            let mut share = got.profile.clone();
            share.rules.push(custom_rule());
            let updated = client
                .update_route_profile(id, &share)
                .await
                .expect("PUT /v1/routes/{id} with four rules");
            assert_eq!(updated.route.id, id);
            let after = client.route(id).await.expect("GET /v1/routes/{id}");
            assert_eq!(after.profile.rules.len(), 4, "{:?}", after.profile);
            assert_eq!(after.profile.rules[3], custom_rule());
            assert_eq!(&after.profile.rules[..3], &got.profile.rules[..]);
            assert_eq!(after.route.rule_count, 4);

            // Rename.
            let renamed = client.rename_route(id, "Bypass RU").await.expect("rename");
            assert_eq!(renamed.route.name, "Bypass RU");
            assert_eq!(after.profile.name, "Bypass Russia");

            // Activation, then the guard against deleting the active route.
            let status: Status = client.set_active_route(id).await.expect("activate");
            assert_eq!(status.route.as_ref().map(|route| route.id), Some(id));
            let err = client
                .delete_route(id)
                .await
                .expect_err("active route is kept");
            assert_eq!(api_code(err), "route_active");

            // Activating Default lets the route go.
            let status = client
                .set_active_route(default.id)
                .await
                .expect("activate Default");
            assert_eq!(
                status.route.as_ref().map(|route| route.id),
                Some(default.id)
            );
            client
                .delete_route(id)
                .await
                .expect("inactive route deleted");
            let err = client.route(id).await.expect_err("deleted route is gone");
            assert_eq!(api_code(err), "not_found");
            let routes = routes_of(&client).await;
            let names = route_list_names(&routes);
            assert!(!names.contains(&"Bypass RU"), "{names:?}");

            // Default is the last route left and it is the active one. Throne checks
            // the active route first, so the answer is route_active, not last_route.
            let err = client
                .delete_route(default.id)
                .await
                .expect_err("the last route is kept");
            assert_eq!(api_code(err), "route_active");
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "needs the real Throne binary in sidecar/Throne and network access to GitHub"]
    async fn remote_presets_real_throne() {
        with_throne(|throne| async move {
            let client = throne.client;

            // Four Russia lists, fetched once each.
            let added = client
                .add_remote_routes(REMOTE_ROUTES_LINK, true)
                .await
                .expect("POST /v1/routes with the remoteroute link");
            println!("results: {:?}", added.results);
            assert_eq!(added.routes.len(), 4, "{:?}", added.routes);
            if added.results.len() != 4 || added.results.iter().any(|result| !result.ok) {
                panic!(
                    "the list fetch from GitHub failed or did not finish; is \
                     raw.githubusercontent.com reachable? results: {:?}, warnings: {:?}",
                    added.results, added.warnings
                );
            }

            let routes = routes_of(&client).await;
            println!("routes: {:?}", route_list_names(&routes));
            for route in &routes {
                if route.name == "Default" {
                    continue;
                }
                assert!(route.remote, "{route:?}");
                assert!(route.auto_update, "{route:?}");
                assert!(route.remote_url.starts_with(LIST_URL_PREFIX), "{route:?}");
                assert!(route.rule_count > 0, "{route:?}");
                let fetched = client.route(route.id).await.expect("GET /v1/routes/{id}");
                assert!(!fetched.profile.rules.is_empty(), "{route:?}");
                println!("{}: {} rules", route.name, fetched.profile.rules.len());
            }
            for name in [
                "Proxy Russia Blocked",
                "Proxy Antizapret",
                "Proxy Refilter",
                "Bypass Russia",
            ] {
                assert!(route_named(&routes, name).remote, "{name}");
            }

            // Refresh: the timestamp does not go back.
            let blocked = route_named(&routes, "Proxy Russia Blocked").clone();
            let refreshed = client
                .refresh_route(blocked.id)
                .await
                .expect("POST /v1/routes/{id}/update");
            println!("refresh: {refreshed:?}");
            assert!(refreshed.ok, "{refreshed:?}");
            let now = route_named(&routes_of(&client).await, "Proxy Russia Blocked").updated_at;
            assert!(
                now >= blocked.updated_at,
                "updated_at went back: {now} < {}",
                blocked.updated_at
            );
            assert!(refreshed.updated_at.unwrap_or(0) >= blocked.updated_at);

            // A remote profile takes no profile edit.
            let share = client
                .route(blocked.id)
                .await
                .expect("GET /v1/routes/{id}")
                .profile;
            let err = client
                .update_route_profile(blocked.id, &share)
                .await
                .expect_err("remote profile refuses a profile edit");
            assert_eq!(api_code(err), "remote_route");

            // The planner finds the list we just added.
            let preset = find("ru-blocked").expect("preset exists");
            assert_eq!(
                plan_preset(&routes, preset),
                PresetPlan::Activate(blocked.id)
            );

            // Activation.
            let status: Status = client
                .set_active_route(blocked.id)
                .await
                .expect("activate Proxy Russia Blocked");
            assert_eq!(
                status.route.as_ref().map(|route| route.id),
                Some(blocked.id)
            );
            assert!(
                status.profile.is_none(),
                "a profile runs: {:?}",
                status.profile
            );

            // Turning the active remote profile into a local copy.
            let mut local = client
                .route(blocked.id)
                .await
                .expect("GET /v1/routes/{id}")
                .profile;
            local.rules.push(custom_rule());
            let turned = client
                .update_route(
                    blocked.id,
                    &UpdateRouteRequest {
                        profile: Some(local.clone()),
                        remote: Some(false),
                        ..Default::default()
                    },
                )
                .await
                .expect("PUT with remote false and a profile");
            println!("local copy: {:?}", turned.route);
            assert!(!turned.route.remote, "{:?}", turned.route);
            assert_eq!(turned.route.remote_url, "");
            assert_eq!(turned.route.rule_count, local.rules.len() as u64);
            let again = client.route(blocked.id).await.expect("GET /v1/routes/{id}");
            assert!(!again.route.remote, "{:?}", again.route);
            assert_eq!(again.profile.rules.len(), local.rules.len());
            assert_eq!(again.profile.rules.last(), Some(&custom_rule()));
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "needs the real Throne binary in sidecar/Throne and network access to GitHub"]
    async fn engine_select_preset_real_throne() {
        with_throne(|throne| async move {
            let client = Arc::clone(&throne.client);
            let sysproxy = SystemProxy::with_runner(&throne.data, Arc::new(FakeRunner::new()));
            let engine = Engine::new(
                Arc::clone(&client),
                throne.port,
                Box::new(|_| {}),
                Arc::new(sysproxy),
                throne.data.join("prefs.json"),
            );

            // A fresh Throne: only Default exists, so the preset adds its list.
            assert_eq!(route_list_names(&routes_of(&client).await), ["Default"]);
            engine
                .handle(Command::SelectPreset("ru-blocked".to_owned()))
                .await;

            let routes = routes_of(&client).await;
            println!("routes after the preset: {:?}", route_list_names(&routes));
            let added = routes
                .iter()
                .find(|route| preset_of(route).map(|p| p.id) == Some("ru-blocked"))
                .unwrap_or_else(|| panic!("the preset's route was not added: {routes:?}"));
            let status: Status = client
                .request_json(Method::Get, "/v1/status", None, None)
                .await
                .expect("GET /v1/status");
            assert_eq!(
                status.route.as_ref().map(|route| route.id),
                Some(added.id),
                "the preset's route is not active: {status:?}"
            );
            assert!(added.remote && added.rule_count > 0, "{added:?}");
        })
        .await;
    }
}
