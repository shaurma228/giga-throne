//! What the Home and Servers screens show, computed from Throne's objects.
//!
//! Everything here is pure except [`push`], which hands a finished [`Frame`] to
//! the Slint event loop. Texts are Russian; numbers are formatted here so the
//! `.slint` files only lay them out.

use std::time::Duration;

use slint::{ModelRc, VecModel};
use throne_api::types::{ConnectionState, LiveStatsResponse, Profile, Status};

use crate::{AppWindow, ServerItem};

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
    pub code: String,
    pub proto: String,
    /// Last URL test in ms; 0 not tested, negative when the test failed.
    pub latency: i64,
    pub ping_text: String,
    /// Lit signal bars: 3 good, 2 medium, 1 bad, 0 not tested or failed.
    pub bars: i32,
    pub running: bool,
}

/// Everything the UI thread needs to draw one update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub home: HomeView,
    /// The whole server list, present only when the profiles changed.
    pub servers: Option<Vec<ServerRow>>,
    pub import_result: String,
    pub error: String,
    /// Clear the import box (the last import succeeded).
    pub clear_import: bool,
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
pub fn proto_tag(profile_type: &str) -> String {
    match profile_type.trim().to_ascii_lowercase().as_str() {
        "vless" => "VLESS".to_owned(),
        "hysteria" | "hysteria2" => "HY2".to_owned(),
        "tuic" => "TUIC".to_owned(),
        "trojan" => "TROJAN".to_owned(),
        "shadowsocks" => "SS".to_owned(),
        "wireguard" => "WG".to_owned(),
        other => other.to_uppercase(),
    }
}

/// Two-letter country badge, or "··" when Throne has no country.
pub fn country_code(country: &str) -> String {
    let code = country.trim();
    if code.is_empty() {
        "··".to_owned()
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
        code: country_code(&profile.country),
        proto: proto_tag(&profile.r#type),
        latency: profile.latency_ms,
        ping_text: ping_text(profile.latency_ms),
        bars: ping_bars(profile.latency_ms),
        running: profile.running,
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
            country_code(&p.country),
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
    if frame.clear_import {
        window.set_import_text("".into());
    }
    if let Some(rows) = frame.servers {
        let items: Vec<ServerItem> = rows.iter().map(to_item).collect();
        window.set_servers(ModelRc::new(VecModel::from(items)));
    }
}

fn to_item(row: &ServerRow) -> ServerItem {
    ServerItem {
        id: i32::try_from(row.id).unwrap_or(-1),
        name: row.name.as_str().into(),
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
    fn country_falls_back_to_placeholder() {
        assert_eq!(country_code(""), "··");
        assert_eq!(country_code("  "), "··");
        assert_eq!(country_code("nl"), "NL");
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
