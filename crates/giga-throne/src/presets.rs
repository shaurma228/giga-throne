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
