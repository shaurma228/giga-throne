//! What the Home and Servers screens show, computed from Throne's objects.
//!
//! Everything here is pure except [`push`], which hands a finished [`Frame`] to
//! the Slint event loop. Texts are Russian; numbers are formatted here so the
//! `.slint` files only lay them out.

use std::time::Duration;

use slint::{ModelRc, VecModel};
use throne_api::types::{ConnectionState, LiveStatsResponse, Profile, Route, Status};

use crate::editor_ui::{self, EditorEvent};
use crate::presets::{self, Preset};
use crate::servers::{self, ServersBusy, ServersView};
use crate::{AppWindow, GroupTabItem, PresetItem, RouteItem, ServerItem, ServersData};

/// A command that is in flight on the engine side, shown until it finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    Connecting,
    Disconnecting,
}

/// Texts and states of the Home screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeView {
    /// "off", "busy" or "on".
    pub conn: &'static str,
    pub status_text: String,
    pub timer: String,
    pub hint: String,
    pub server_name: String,
    pub server_code: String,
    pub server_proto: String,
    pub server_ping: String,
    pub down_rate: String,
    pub up_rate: String,
    pub inbound_text: String,
}

/// One row of the server list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    pub id: i64,
    pub name: String,
    /// The address Throne shows, on the Servers page card.
    pub address: String,
    pub code: String,
    pub proto: String,
    /// Last URL test in ms; 0 not tested, negative when the test failed.
    pub latency: i64,
    pub ping_text: String,
    /// Lit signal bars: 3 good, 2 medium, 1 bad, 0 not tested or failed.
    pub bars: i32,
    pub running: bool,
}

/// A routing command in flight, shown on its card or row until it finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteBusy {
    /// Choosing the preset with this id.
    Preset(String),
    /// Activating or deleting the route with this id.
    Route(i64),
    /// Adding what the import box holds.
    Import,
}

/// The info or error line under the Routes screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteNote {
    pub text: String,
    /// Shown in the error color instead of the muted one.
    pub bad: bool,
}

/// One ready-made mode card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetCard {
    pub id: String,
    pub title: String,
    pub desc: String,
    pub source: String,
    pub recommended: bool,
    /// The active route is this mode.
    pub selected: bool,
    /// Can be chosen now. Only the "Всё через VPN" mode needs a route of its own.
    pub available: bool,
    /// Replaces the source line when the card is not available.
    pub note: String,
}

/// One row of "Мои маршруты".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRow {
    pub id: i64,
    pub name: String,
    pub meta: String,
    pub active: bool,
}

/// Everything the Routes screen shows. Busy values use `""` and `-1` for none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutesView {
    /// Region id of the chip that is on.
    pub region: String,
    pub presets: Vec<PresetCard>,
    pub rows: Vec<RouteRow>,
    pub busy_preset: String,
    pub busy_route: i64,
    pub busy_import: bool,
    pub note: RouteNote,
}

/// Everything the UI thread needs to draw one update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// What the route editor has to do, once, after the command that made it.
    pub editor: Option<EditorEvent>,
    pub home: HomeView,
    /// The whole server list, present only when the profiles changed.
    pub servers: Option<Vec<ServerRow>>,
    /// The whole Routes screen, present only when something on it changed.
    pub routes: Option<RoutesView>,
    pub import_result: String,
    pub error: String,
    /// Clear the import box (the last import succeeded).
    pub clear_import: bool,
    /// The "Системный прокси" preference, for its switch.
    pub system_proxy: bool,
    /// The whole Servers page, present only when something on it changed.
    pub servers_page: Option<ServersView>,
    /// Clear the "Ссылки" box (the last import succeeded).
    pub clear_servers_links: bool,
    /// Clear the subscription fields (the last subscription was added).
    pub clear_servers_sub: bool,
}

/// Formats a transfer rate in bytes per second, 1024-based: "512 Б/с", "12.3 КБ/с".
pub fn format_rate(bytes_per_sec: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = bytes_per_sec as f64;
    if bytes_per_sec < 1024 {
        format!("{bytes_per_sec} Б/с")
    } else if value < KIB * KIB {
        format!("{:.1} КБ/с", value / KIB)
    } else if value < KIB * KIB * KIB {
        format!("{:.1} МБ/с", value / (KIB * KIB))
    } else {
        format!("{:.1} ГБ/с", value / (KIB * KIB * KIB))
    }
}

/// Formats an elapsed time as "HH:MM:SS".
pub fn format_duration(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Protocol tag shown next to a server, from Throne's profile type.
/// xray-core types come as "xray<kind>": "xrayvless" is VLESS, the bundled-config kinds are XRAY.
pub fn proto_tag(profile_type: &str) -> String {
    let lower = profile_type.trim().to_ascii_lowercase();
    let (xray, kind) = match lower.strip_prefix("xray") {
        Some(rest) => (true, rest),
        None => (false, lower.as_str()),
    };
    match kind {
        "vless" => "VLESS".to_owned(),
        "hysteria" | "hysteria2" => "HY2".to_owned(),
        "tuic" => "TUIC".to_owned(),
        "trojan" => "TROJAN".to_owned(),
        "shadowsocks" => "SS".to_owned(),
        "wireguard" => "WG".to_owned(),
        "" | "full" | "fullconfig" | "outbound" if xray => "XRAY".to_owned(),
        other => other.to_uppercase(),
    }
}

/// Two ASCII letters from the first flag emoji (a pair of regional indicators) in `name`.
fn flag_code(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let letter = |c: char| match c as u32 {
        v @ 0x1F1E6..=0x1F1FF => char::from_u32(v - 0x1F1E6 + 'A' as u32),
        _ => None,
    };
    chars
        .windows(2)
        .find_map(|pair| Some(format!("{}{}", letter(pair[0])?, letter(pair[1])?)))
}

/// Two-letter country badge. Throne's country wins; without one, a flag emoji in the
/// profile name is used, and "··" when neither is there.
pub fn country_code(country: &str, name: &str) -> String {
    let code = country.trim();
    if code.is_empty() {
        flag_code(name).unwrap_or_else(|| "··".to_owned())
    } else {
        code.to_uppercase()
    }
}

/// Latency text: "38 мс", "—" when not tested, "×" when the test failed.
pub fn ping_text(latency: i64) -> String {
    match latency {
        l if l > 0 => format!("{l} мс"),
        0 => "—".to_owned(),
        _ => "×".to_owned(),
    }
}

/// Number of lit signal bars for a latency.
pub fn ping_bars(latency: i64) -> i32 {
    match latency {
        l if l <= 0 => 0,
        l if l < 60 => 3,
        l if l < 150 => 2,
        _ => 1,
    }
}

/// Maps a profile to a server-list row.
pub fn server_row(profile: &Profile) -> ServerRow {
    ServerRow {
        id: profile.id,
        name: profile.name.clone(),
        address: profile.address.clone(),
        code: country_code(&profile.country, &profile.name),
        proto: proto_tag(&profile.r#type),
        latency: profile.latency_ms,
        ping_text: ping_text(profile.latency_ms),
        bars: ping_bars(profile.latency_ms),
        running: profile.running,
    }
}

/// Russian plural of "правило": "1 правило", "3 правила", "7 правил".
pub fn rules_text(count: u64) -> String {
    let word = match (count % 10, count % 100) {
        (1, n) if n != 11 => "правило",
        (2..=4, n) if !(12..=14).contains(&n) => "правила",
        _ => "правил",
    };
    format!("{count} {word}")
}

/// Time since `then` as "только что", "N мин назад", "N ч назад" or "N дн назад".
/// `now` and `then` are epoch seconds; a time in the future counts as just now.
pub fn ago_text(now: i64, then: i64) -> String {
    let secs = now - then;
    match secs {
        s if s < 60 => "только что".to_owned(),
        s if s < 3_600 => format!("{} мин назад", s / 60),
        s if s < 86_400 => format!("{} ч назад", s / 3_600),
        s => format!("{} дн назад", s / 86_400),
    }
}

/// Second line of a route row: "свой · 7 правил", "по ссылке · обновлён 2 ч назад"
/// or "по ссылке · ещё не загружен".
pub fn route_meta(route: &Route, now: i64) -> String {
    if route.remote {
        if route.updated_at > 0 {
            format!("по ссылке · обновлён {}", ago_text(now, route.updated_at))
        } else {
            "по ссылке · ещё не загружен".to_owned()
        }
    } else if route.raw {
        "свой · JSON".to_owned()
    } else {
        format!("свой · {}", rules_text(route.rule_count))
    }
}

/// Maps a route to a row of "Мои маршруты".
pub fn route_row(route: &Route, active: Option<i64>, now: i64) -> RouteRow {
    RouteRow {
        id: route.id,
        name: route.name.clone(),
        meta: route_meta(route, now),
        active: active == Some(route.id),
    }
}

/// Maps a preset to its card. `active` is the id of the active route.
pub fn preset_card(preset: &Preset, routes: &[Route], active: Option<i64>) -> PresetCard {
    let selected = active.is_some_and(|id| {
        routes
            .iter()
            .any(|route| route.id == id && presets::route_matches(route, preset))
    });
    let available = preset.file.is_some() || routes.iter().any(presets::is_default_route);
    PresetCard {
        id: preset.id.to_owned(),
        title: preset.title.to_owned(),
        desc: preset.desc.to_owned(),
        source: preset.source.to_owned(),
        recommended: preset.recommended,
        selected,
        available,
        note: if available {
            String::new()
        } else {
            "Нет профиля Default".to_owned()
        },
    }
}

/// Computes the Routes screen from the routes, the active id and the region.
pub fn routes_view(
    routes: &[Route],
    active: Option<i64>,
    region: &str,
    busy: Option<&RouteBusy>,
    note: &RouteNote,
    now: i64,
) -> RoutesView {
    let region = presets::region_or_default(region);
    RoutesView {
        region: region.to_owned(),
        presets: presets::in_region(region)
            .map(|preset| preset_card(preset, routes, active))
            .collect(),
        rows: presets::my_routes(routes)
            .into_iter()
            .map(|route| route_row(route, active, now))
            .collect(),
        busy_preset: match busy {
            Some(RouteBusy::Preset(id)) => id.clone(),
            _ => String::new(),
        },
        busy_route: match busy {
            Some(RouteBusy::Route(id)) => *id,
            _ => -1,
        },
        busy_import: busy == Some(&RouteBusy::Import),
        note: note.clone(),
    }
}

/// Computes the Home screen from the last engine status, stats and profiles.
///
/// `timer` is the session time, already formatted; it is shown only while running.
pub fn home_view(
    status: Option<&Status>,
    stats: Option<&LiveStatsResponse>,
    profiles: &[Profile],
    pending: Option<Pending>,
    timer: String,
) -> HomeView {
    let state = status.map(|s| s.state);
    let activity = pending.or(match state {
        Some(ConnectionState::Connecting | ConnectionState::Starting) => Some(Pending::Connecting),
        Some(ConnectionState::Stopping) => Some(Pending::Disconnecting),
        _ => None,
    });
    let running = activity.is_none() && state == Some(ConnectionState::Running);
    let (conn, status_text) = match activity {
        Some(Pending::Connecting) => ("busy", "Подключаемся…"),
        Some(Pending::Disconnecting) => ("busy", "Отключаемся…"),
        None if running => ("on", "Защищено"),
        None => ("off", "Не защищено"),
    };

    let current = status
        .and_then(|s| s.profile.clone())
        .or_else(|| {
            status.and_then(|s| profiles.iter().find(|p| p.id == s.last_profile_id).cloned())
        })
        .or_else(|| profiles.first().cloned());
    let (server_name, server_code, server_proto, server_ping) = match &current {
        Some(p) => (
            p.name.clone(),
            country_code(&p.country, &p.name),
            proto_tag(&p.r#type),
            ping_text(p.latency_ms),
        ),
        None => (
            "Нет серверов".to_owned(),
            "··".to_owned(),
            String::new(),
            "—".to_owned(),
        ),
    };

    let hint = if profiles.is_empty() {
        "Вставь ссылку справа"
    } else {
        match conn {
            "on" => "в сети без цензуры",
            "busy" => "рукопожатие с сервером",
            _ => "нажми на кнопку — и погнали",
        }
    };

    let rate = |pick: fn(&LiveStatsResponse) -> u64| match (running, stats) {
        (true, Some(s)) => format_rate(pick(s)),
        _ => "—".to_owned(),
    };

    let inbound_text = match status {
        Some(s) if s.inbound.enabled => {
            format!("Прокси: {}:{}", s.inbound.address, s.inbound.port)
        }
        Some(_) => "Прокси выключен".to_owned(),
        None => "—".to_owned(),
    };

    HomeView {
        conn,
        status_text: status_text.to_owned(),
        timer: if running {
            timer
        } else {
            "00:00:00".to_owned()
        },
        hint: hint.to_owned(),
        server_name,
        server_code,
        server_proto,
        server_ping,
        down_rate: rate(|s| s.proxy.down),
        up_rate: rate(|s| s.proxy.up),
        inbound_text,
    }
}

fn preset_item(card: &PresetCard) -> PresetItem {
    PresetItem {
        id: card.id.as_str().into(),
        title: card.title.as_str().into(),
        desc: card.desc.as_str().into(),
        source: card.source.as_str().into(),
        note: card.note.as_str().into(),
        recommended: card.recommended,
        selected: card.selected,
        available: card.available,
    }
}

fn route_item(row: &RouteRow) -> RouteItem {
    RouteItem {
        id: i32::try_from(row.id).unwrap_or(-1),
        name: row.name.as_str().into(),
        meta: row.meta.as_str().into(),
        active: row.active,
    }
}

fn apply_routes(window: &AppWindow, view: RoutesView) {
    window.set_route_region(view.region.as_str().into());
    let cards: Vec<PresetItem> = view.presets.iter().map(preset_item).collect();
    window.set_route_presets(ModelRc::new(VecModel::from(cards)));
    let rows: Vec<RouteItem> = view.rows.iter().map(route_item).collect();
    window.set_route_rows(ModelRc::new(VecModel::from(rows)));
    window.set_busy_preset(view.busy_preset.as_str().into());
    window.set_busy_route(i32::try_from(view.busy_route).unwrap_or(-1));
    window.set_busy_import(view.busy_import);
    window.set_route_note(view.note.text.as_str().into());
    window.set_route_note_bad(view.note.bad);
}

/// Hands a frame to the Slint event loop. Never blocks.
pub fn push(weak: &slint::Weak<AppWindow>, frame: Frame) {
    // Fails only after the event loop has exited, when nobody is left to show it.
    let _ = weak.upgrade_in_event_loop(move |window| apply(&window, frame));
}

fn apply(window: &AppWindow, frame: Frame) {
    let home = frame.home;
    window.set_conn_state(home.conn.into());
    window.set_status_text(home.status_text.into());
    window.set_timer(home.timer.into());
    window.set_hint(home.hint.into());
    window.set_server_name(home.server_name.into());
    window.set_server_code(home.server_code.into());
    window.set_server_proto(home.server_proto.into());
    window.set_server_ping(home.server_ping.into());
    window.set_down_rate(home.down_rate.into());
    window.set_up_rate(home.up_rate.into());
    window.set_inbound_text(home.inbound_text.into());
    window.set_error_text(frame.error.into());
    window.set_import_result(frame.import_result.into());
    window.set_sysproxy_on(frame.system_proxy);
    if frame.clear_import {
        window.set_import_text("".into());
    }
    if let Some(rows) = frame.servers {
        let items: Vec<ServerItem> = rows.iter().map(to_item).collect();
        window.set_servers(ModelRc::new(VecModel::from(items)));
    }
    if let Some(routes) = frame.routes {
        apply_routes(window, routes);
    }
    if let Some(page) = frame.servers_page {
        apply_servers(window, page);
    }
    if frame.clear_servers_links {
        window.set_servers_links("".into());
    }
    if frame.clear_servers_sub {
        window.set_servers_sub_url("".into());
        window.set_servers_sub_name("".into());
    }
    if let Some(event) = frame.editor {
        editor_ui::on_event(event);
    }
}

/// Sets the Servers page. Counts and indexes are `i32` in Slint; out of range ones saturate.
fn apply_servers(window: &AppWindow, view: ServersView) {
    let groups: Vec<GroupTabItem> = view
        .groups
        .iter()
        .map(|tab| GroupTabItem {
            id: to_i32(tab.id),
            label: tab.label.as_str().into(),
            count: tab.count.to_string().into(),
            subscription: tab.subscription,
        })
        .collect();
    let cards: Vec<ServerItem> = view.cards.iter().map(to_item).collect();
    let (busy_update, busy_delete, busy_import, busy_add) = match &view.busy {
        Some(ServersBusy::Update(id)) => (to_i32(*id), false, false, false),
        Some(ServersBusy::DeleteGroup) => (-1, true, false, false),
        Some(ServersBusy::Import) => (-1, false, true, false),
        Some(ServersBusy::AddSubscription) => (-1, false, false, true),
        None => (-1, false, false, false),
    };
    let subscription = view
        .subscription
        .clone()
        .unwrap_or(servers::SubscriptionCard {
            name: String::new(),
            meta: String::new(),
            count: String::new(),
        });
    let plain = view.plain.clone().unwrap_or(servers::PlainGroup {
        name: String::new(),
        count: String::new(),
    });
    let test_text = view
        .test
        .map(|(tested, total)| servers::test_progress_text(tested, total))
        .unwrap_or_default();
    let data = ServersData {
        groups: ModelRc::new(VecModel::from(groups)),
        cards: ModelRc::new(VecModel::from(cards)),
        selected: to_i32(view.selected),
        can_delete: view.can_delete,
        sub_visible: view.subscription.is_some(),
        sub_name: subscription.name.as_str().into(),
        sub_meta: subscription.meta.as_str().into(),
        sub_count: subscription.count.as_str().into(),
        plain_visible: view.plain.is_some(),
        plain_name: plain.name.as_str().into(),
        plain_count: plain.count.as_str().into(),
        by_ping: view.by_ping,
        empty_text: view.empty_text.as_str().into(),
        test_running: view.test.is_some(),
        test_text: test_text.as_str().into(),
        busy_update,
        busy_delete,
        busy_import,
        busy_add,
        import_target: to_i32(view.import_target),
        note: view.note.text.as_str().into(),
        note_bad: view.note.bad,
    };
    window.set_servers_data(data);
}

fn to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(-1)
}

fn to_item(row: &ServerRow) -> ServerItem {
    ServerItem {
        id: i32::try_from(row.id).unwrap_or(-1),
        name: row.name.as_str().into(),
        address: row.address.as_str().into(),
        code: row.code.as_str().into(),
        proto: row.proto.as_str().into(),
        ping: i32::try_from(row.latency).unwrap_or(i32::MAX),
        ping_text: row.ping_text.as_str().into(),
        bars: row.bars,
        running: row.running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: i64) -> Profile {
        Profile {
            id,
            name: format!("server {id}"),
            r#type: "vless".to_owned(),
            ..Profile::default()
        }
    }

    #[test]
    fn rates_use_1024_and_one_decimal() {
        assert_eq!(format_rate(0), "0 Б/с");
        assert_eq!(format_rate(512), "512 Б/с");
        assert_eq!(format_rate(1023), "1023 Б/с");
        assert_eq!(format_rate(1024), "1.0 КБ/с");
        assert_eq!(format_rate(12_595), "12.3 КБ/с");
        assert_eq!(format_rate(26_000_000), "24.8 МБ/с");
        assert_eq!(format_rate(1_288_490_189), "1.2 ГБ/с");
    }

    #[test]
    fn durations_are_hh_mm_ss() {
        assert_eq!(format_duration(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_duration(Duration::from_secs(59)), "00:00:59");
        assert_eq!(format_duration(Duration::from_secs(2563)), "00:42:43");
        assert_eq!(format_duration(Duration::from_secs(90_061)), "25:01:01");
    }

    #[test]
    fn protocol_tags_follow_throne_types() {
        assert_eq!(proto_tag("vless"), "VLESS");
        assert_eq!(proto_tag("hysteria2"), "HY2");
        assert_eq!(proto_tag("hysteria"), "HY2");
        assert_eq!(proto_tag("shadowsocks"), "SS");
        assert_eq!(proto_tag("wireguard"), "WG");
        assert_eq!(proto_tag("chain"), "CHAIN");
    }

    #[test]
    fn xray_types_strip_the_xray_prefix() {
        assert_eq!(proto_tag("xrayvless"), "VLESS");
        assert_eq!(proto_tag("xrayvmess"), "VMESS");
        assert_eq!(proto_tag("vmess"), "VMESS");
        assert_eq!(proto_tag("xrayfull"), "XRAY");
        assert_eq!(proto_tag("xrayfullconfig"), "XRAY");
        assert_eq!(proto_tag("xrayoutbound"), "XRAY");
        assert_eq!(proto_tag("xray"), "XRAY");
        assert_eq!(proto_tag(""), "");
    }

    #[test]
    fn country_falls_back_to_placeholder() {
        assert_eq!(country_code("", ""), "··");
        assert_eq!(country_code("  ", "server 1"), "··");
        assert_eq!(country_code("nl", ""), "NL");
    }

    #[test]
    fn country_falls_back_to_flag_in_name() {
        assert_eq!(country_code("", "🇩🇪 Германия"), "DE");
        assert_eq!(country_code("  ", "Тест 🇫🇷"), "FR");
        assert_eq!(country_code("", "🇺🇸🇩🇪 two flags"), "US");
        assert_eq!(country_code("", "🇩 single"), "··");
        assert_eq!(country_code("", "no flag here"), "··");
    }

    #[test]
    fn country_from_throne_wins_over_flag_in_name() {
        assert_eq!(country_code("nl", "🇩🇪 Германия"), "NL");
    }

    #[test]
    fn ping_text_and_bars_cover_every_latency_class() {
        assert_eq!(ping_text(0), "—");
        assert_eq!(ping_text(-1), "×");
        assert_eq!(ping_text(-2), "×");
        assert_eq!(ping_text(38), "38 мс");
        assert_eq!((ping_bars(0), ping_bars(-1)), (0, 0));
        assert_eq!(ping_bars(59), 3);
        assert_eq!(ping_bars(60), 2);
        assert_eq!(ping_bars(149), 2);
        assert_eq!(ping_bars(150), 1);
        assert_eq!(ping_bars(214), 1);
    }

    #[test]
    fn profile_maps_to_server_row() {
        let mut p = profile(7);
        p.country = "de".to_owned();
        p.latency_ms = 44;
        p.running = true;
        let row = server_row(&p);
        assert_eq!(row.id, 7);
        assert_eq!(row.code, "DE");
        assert_eq!(row.proto, "VLESS");
        assert_eq!(row.ping_text, "44 мс");
        assert_eq!(row.bars, 3);
        assert!(row.running);

        let untested = server_row(&profile(8));
        assert_eq!(untested.code, "··");
        assert_eq!(untested.ping_text, "—");
        assert_eq!(untested.bars, 0);
    }

    #[test]
    fn rules_are_counted_in_russian() {
        assert_eq!(rules_text(0), "0 правил");
        assert_eq!(rules_text(1), "1 правило");
        assert_eq!(rules_text(3), "3 правила");
        assert_eq!(rules_text(7), "7 правил");
        assert_eq!(rules_text(11), "11 правил");
        assert_eq!(rules_text(22), "22 правила");
        assert_eq!(rules_text(112), "112 правил");
    }

    #[test]
    fn time_ago_has_four_steps() {
        let now = 1_000_000;
        assert_eq!(ago_text(now, now), "только что");
        assert_eq!(ago_text(now, now - 59), "только что");
        assert_eq!(ago_text(now, now - 60), "1 мин назад");
        assert_eq!(ago_text(now, now - 3_599), "59 мин назад");
        assert_eq!(ago_text(now, now - 7_200), "2 ч назад");
        assert_eq!(ago_text(now, now - 86_399), "23 ч назад");
        assert_eq!(ago_text(now, now - 172_800), "2 дн назад");
        assert_eq!(ago_text(now, now + 500), "только что");
    }

    fn route(id: i64, name: &str) -> Route {
        Route {
            id,
            name: name.to_owned(),
            active: false,
            remote: false,
            remote_url: String::new(),
            auto_update: false,
            updated_at: 0,
            raw: false,
            rule_count: 0,
        }
    }

    #[test]
    fn route_meta_tells_local_from_remote() {
        let mut local = route(1, "Работа");
        local.rule_count = 7;
        assert_eq!(route_meta(&local, 0), "свой · 7 правил");

        let mut remote = route(2, "Списки");
        remote.remote = true;
        remote.remote_url = "https://example.com/x".to_owned();
        assert_eq!(route_meta(&remote, 0), "по ссылке · ещё не загружен");
        remote.updated_at = 1_000;
        assert_eq!(
            route_meta(&remote, 1_000 + 7_300),
            "по ссылке · обновлён 2 ч назад"
        );
    }

    #[test]
    fn routes_view_partitions_and_marks_the_active_route() {
        let mut blocked = route(5, "Proxy");
        blocked.remote = true;
        blocked.remote_url =
            presets::preset_url(presets::find("ru-blocked").expect("preset")).expect("url");
        let routes = vec![route(1, "Default"), blocked, route(2, "Работа")];
        let view = routes_view(&routes, Some(5), "ru", None, &RouteNote::default(), 0);
        assert_eq!(view.region, "ru");
        assert_eq!(view.presets.len(), 5);
        assert!(
            view.presets
                .iter()
                .find(|p| p.id == "ru-blocked")
                .expect("card")
                .selected
        );
        assert!(
            !view
                .presets
                .iter()
                .find(|p| p.id == "ru-all")
                .expect("card")
                .selected
        );
        let rows: Vec<&str> = view.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(rows, vec!["Работа"]);
        assert!(!view.rows[0].active);
    }

    #[test]
    fn all_mode_is_unavailable_without_default_and_busy_is_reported() {
        let routes = vec![route(2, "Работа")];
        let busy = RouteBusy::Preset("ru-refilter".to_owned());
        let view = routes_view(&routes, None, "xx", Some(&busy), &RouteNote::default(), 0);
        assert_eq!(view.region, "ru");
        let all = &view.presets[0];
        assert!(!all.available);
        assert_eq!(all.note, "Нет профиля Default");
        assert_eq!(view.busy_preset, "ru-refilter");
        assert_eq!(view.busy_route, -1);
        assert!(!view.busy_import);

        let busy = RouteBusy::Route(2);
        let view = routes_view(&routes, None, "ru", Some(&busy), &RouteNote::default(), 0);
        assert_eq!(view.busy_route, 2);
        assert!(view.busy_preset.is_empty());
    }

    #[test]
    fn home_without_profiles_asks_for_a_link() {
        let view = home_view(None, None, &[], None, "00:00:00".to_owned());
        assert_eq!(view.conn, "off");
        assert_eq!(view.status_text, "Не защищено");
        assert_eq!(view.server_name, "Нет серверов");
        assert_eq!(view.hint, "Вставь ссылку справа");
        assert_eq!(view.down_rate, "—");
    }

    #[test]
    fn pending_command_overrides_the_polled_state() {
        let view = home_view(
            None,
            None,
            &[profile(1)],
            Some(Pending::Connecting),
            "x".into(),
        );
        assert_eq!(view.conn, "busy");
        assert_eq!(view.status_text, "Подключаемся…");
        assert_eq!(view.timer, "00:00:00");
        let view = home_view(
            None,
            None,
            &[profile(1)],
            Some(Pending::Disconnecting),
            "x".into(),
        );
        assert_eq!(view.status_text, "Отключаемся…");
        assert_eq!(view.server_name, "server 1");
    }
}
