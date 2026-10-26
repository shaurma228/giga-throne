//! System tray icon: the StatusNotifierItem with the connection status and the
//! main actions.
//!
//! The pure parts (state, menu model, tooltip, icon pixels) build everywhere and
//! are unit-tested here. The D-Bus service exists on Linux only (ksni); other
//! systems get a stub that returns no tray yet.
//!
//! Menu actions go through the window's own callbacks (`invoke_toggle` and
//! friends), so they take the same path as the buttons in the window.
//!
//! The engine's frames reach the tray through [`TrayFeed::observe`], which runs
//! next to the frame sink. The tray is updated only when its state changes.

// Only the Linux service uses the menu and tray types; elsewhere they are compiled for the tests.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::Mutex;

use slint::ComponentHandle;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::view::Frame;
use crate::AppWindow;

/// Profiles listed in the "Сервер" submenu.
pub const MAX_SERVERS: usize = 15;

/// Icon sizes handed to the StatusNotifierItem, in pixels.
pub const ICON_SIZES: [u32; 2] = [64, 32];

/// Status text of a disconnect in progress; the same text as `view::home_view` shows.
const DISCONNECTING_TEXT: &str = "Отключаемся…";

const ACCENT: [u8; 3] = [0xC8, 0xFF, 0x4D];
const MUTED: [u8; 3] = [0x8A, 0x8A, 0x96];
const DARK: [u8; 3] = [0x0E, 0x0E, 0x12];

/// Connection state as the tray shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Off,
    /// Connecting or disconnecting; no command may start now.
    Busy,
    On,
}

impl Link {
    /// Maps the "off", "busy" or "on" of `view::HomeView::conn`.
    pub fn from_conn(conn: &str) -> Self {
        match conn {
            "on" => Link::On,
            "busy" => Link::Busy,
            _ => Link::Off,
        }
    }
}

/// One profile as the "Сервер" submenu lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub id: i64,
    pub name: String,
    /// The profile Throne runs now.
    pub running: bool,
    /// The profile the Home screen shows; it gets the check mark.
    pub current: bool,
}

/// What the tray shows. Compared with the last one, so unchanged state costs nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayState {
    pub link: Link,
    /// The engine status line: "Защищено", "Не защищено", "Подключаемся…".
    pub status_text: String,
    /// The current server's name, or "Нет серверов".
    pub server_name: String,
    pub servers: Vec<ServerEntry>,
    /// The "Системный прокси" preference.
    pub system_proxy: bool,
}

impl TrayState {
    /// The state before the first frame arrives.
    pub fn initial() -> Self {
        Self {
            link: Link::Off,
            status_text: "Не защищено".to_owned(),
            server_name: String::new(),
            servers: Vec::new(),
            system_proxy: false,
        }
    }

    /// "Защищено · <server>" while connected, otherwise the status text.
    pub fn summary(&self) -> String {
        match self.link {
            Link::On if !self.server_name.is_empty() => {
                format!("{} · {}", self.status_text, self.server_name)
            }
            _ => self.status_text.clone(),
        }
    }

    /// The tooltip title: "giga-throne — <summary>".
    pub fn tooltip(&self) -> String {
        format!("giga-throne — {}", self.summary())
    }
}

/// One entry of the "Сервер" submenu. `id` is `None` for the disabled placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMenuItem {
    pub id: Option<i32>,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

/// The tray menu in plain data, so that its labels and flags can be tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Disabled first line: the summary of the status.
    pub header: String,
    pub toggle_label: &'static str,
    pub toggle_enabled: bool,
    pub servers: Vec<ServerMenuItem>,
    pub system_proxy: bool,
}

/// Builds the menu from the state.
pub fn menu_model(state: &TrayState) -> MenuModel {
    // A disconnect in progress keeps the "Отключить" label, disabled, like the button on Home.
    let stopping = state.link == Link::Busy && state.status_text == DISCONNECTING_TEXT;
    let toggle_is_disconnect = state.link == Link::On || stopping;
    let mut servers: Vec<ServerMenuItem> = state
        .servers
        .iter()
        .take(MAX_SERVERS)
        .filter_map(|server| {
            Some(ServerMenuItem {
                id: Some(i32::try_from(server.id).ok()?),
                label: server.name.clone(),
                checked: server.current,
                enabled: true,
            })
        })
        .collect();
    if servers.is_empty() {
        servers.push(ServerMenuItem {
            id: None,
            label: "Нет серверов".to_owned(),
            checked: false,
            enabled: false,
        });
    }
    MenuModel {
        header: state.summary(),
        toggle_label: if toggle_is_disconnect {
            "Отключить"
        } else {
            "Подключить"
        },
        toggle_enabled: state.link != Link::Busy,
        servers,
        system_proxy: state.system_proxy,
    }
}

/// Size of the icon in bytes: ARGB32 with four bytes per pixel.
pub fn icon_len(size: u32) -> usize {
    (size as usize) * (size as usize) * 4
}

/// Renders the tray icon as ARGB32 in network byte order (alpha, red, green, blue per pixel),
/// the layout StatusNotifierItem expects.
///
/// A lime rounded square with a dark power symbol when connected or connecting,
/// a muted gray one when disconnected. Edges are anti-aliased by supersampling.
pub fn icon_pixels(size: u32, link: Link) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let fill = if link == Link::Off { MUTED } else { ACCENT };
    let total = (SAMPLES * SAMPLES) as f32;
    let mut out = Vec::with_capacity(icon_len(size));
    for y in 0..size {
        for x in 0..size {
            let mut inside = 0u32;
            let mut glyph = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    if in_rounded_square(u, v) {
                        inside += 1;
                        if in_power_glyph(u, v) {
                            glyph += 1;
                        }
                    }
                }
            }
            let alpha = inside as f32 / total;
            let ink = if inside == 0 {
                0.0
            } else {
                glyph as f32 / inside as f32
            };
            let rgb = [0, 1, 2].map(|i| mix(fill[i], DARK[i], ink));
            out.extend_from_slice(&[(alpha * 255.0).round() as u8, rgb[0], rgb[1], rgb[2]]);
        }
    }
    out
}

/// Point in the unit square, inside the rounded square that fills the icon.
fn in_rounded_square(u: f32, v: f32) -> bool {
    const RADIUS: f32 = 0.22;
    let qx = (u - 0.5).abs() - (0.5 - RADIUS);
    let qy = (v - 0.5).abs() - (0.5 - RADIUS);
    !(qx > 0.0 && qy > 0.0 && qx * qx + qy * qy > RADIUS * RADIUS)
}

/// Point of the power symbol: a ring open at the top, and a bar through the gap.
fn in_power_glyph(u: f32, v: f32) -> bool {
    const CX: f32 = 0.5;
    const CY: f32 = 0.55;
    const RING_RADIUS: f32 = 0.27;
    const HALF_STROKE: f32 = 0.055;
    const GAP: f32 = 0.75;
    let (dx, dy) = (u - CX, v - CY);
    let dist = (dx * dx + dy * dy).sqrt();
    // Angle from the top of the ring, clockwise positive.
    let from_top = dx.atan2(-dy).abs();
    let ring = (dist - RING_RADIUS).abs() <= HALF_STROKE && from_top >= GAP;
    let bar = (u - 0.5).abs() <= HALF_STROKE && (0.18..=0.55).contains(&v);
    ring || bar
}

fn mix(from: u8, to: u8, amount: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * amount).round() as u8
}

/// Shared with the window: the tray keeps the last frame's server list and sends
/// the state on when it changes.
pub struct TrayFeed {
    state: watch::Sender<TrayState>,
    servers: Mutex<Vec<ServerEntry>>,
}

impl TrayFeed {
    fn new() -> (Self, watch::Receiver<TrayState>) {
        let (state, rx) = watch::channel(TrayState::initial());
        (
            Self {
                state,
                servers: Mutex::new(Vec::new()),
            },
            rx,
        )
    }

    /// Takes what the tray needs from a frame. Called for every frame; cheap
    /// because the server list is copied only when the frame carries one.
    pub fn observe(&self, frame: &Frame) {
        let mut known = self
            .servers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(rows) = &frame.servers {
            *known = rows
                .iter()
                .map(|row| ServerEntry {
                    id: row.id,
                    name: row.name.clone(),
                    running: row.running,
                    current: row.current,
                })
                .collect();
        }
        let next = TrayState {
            link: Link::from_conn(frame.home.conn),
            status_text: frame.home.status_text.clone(),
            server_name: frame.home.server_name.clone(),
            servers: known.clone(),
            system_proxy: frame.system_proxy,
        };
        self.state.send_if_modified(|current| {
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
    }
}

/// Runs `action` on the window in the Slint event loop, if the window still exists.
fn on_window(window: &slint::Weak<AppWindow>, action: impl FnOnce(AppWindow) + Send + 'static) {
    // Fails only after the event loop has exited, when nobody is left to act on.
    let _ = window.upgrade_in_event_loop(action);
}

/// Shows the window if it is hidden, hides it if it is visible.
fn toggle_window(window: &slint::Weak<AppWindow>) {
    on_window(window, |app| {
        let win = app.window();
        let result = if win.is_visible() {
            win.hide()
        } else {
            win.show()
        };
        if let Err(err) = result {
            warn!("could not toggle the window: {err}");
        }
    });
}

/// Starts the tray icon on `runtime`. Returns the feed for the engine's frames,
/// or `None` when no tray is available; the window then keeps closing as before.
#[cfg(target_os = "linux")]
pub fn start(
    runtime: &tokio::runtime::Runtime,
    window: slint::Weak<AppWindow>,
) -> Option<TrayFeed> {
    use ksni::TrayMethods;

    let (feed, mut rx) = TrayFeed::new();
    let tray = linux::GigaTray {
        window,
        state: rx.borrow().clone(),
    };
    // Registering needs the session bus; `block_on` is fine before the engine starts.
    let handle = match runtime.block_on(tray.spawn()) {
        Ok(handle) => handle,
        Err(err) => {
            warn!("system tray unavailable, closing the window will quit: {err}");
            return None;
        }
    };
    info!("system tray registered");
    runtime.spawn(async move {
        while rx.changed().await.is_ok() {
            let next = rx.borrow_and_update().clone();
            if handle.update(move |tray| tray.state = next).await.is_none() {
                break;
            }
        }
    });
    Some(feed)
}

/// Other systems have no tray yet.
#[cfg(not(target_os = "linux"))]
pub fn start(
    _runtime: &tokio::runtime::Runtime,
    _window: slint::Weak<AppWindow>,
) -> Option<TrayFeed> {
    info!("system tray is not implemented on this platform yet");
    None
}

#[cfg(target_os = "linux")]
mod linux {
    use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
    use ksni::MenuItem;
    use slint::ComponentHandle;
    use tracing::warn;

    use super::{
        icon_pixels, menu_model, on_window, toggle_window, MenuModel, TrayState, ICON_SIZES,
    };
    use crate::AppWindow;

    /// The StatusNotifierItem. Its state is replaced by the feed's updates.
    pub struct GigaTray {
        pub window: slint::Weak<AppWindow>,
        pub state: TrayState,
    }

    impl ksni::Tray for GigaTray {
        fn id(&self) -> String {
            "giga-throne".to_owned()
        }

        fn title(&self) -> String {
            "giga-throne".to_owned()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            ICON_SIZES
                .iter()
                .map(|&size| ksni::Icon {
                    width: size as i32,
                    height: size as i32,
                    data: icon_pixels(size, self.state.link),
                })
                .collect()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: self.state.tooltip(),
                ..ksni::ToolTip::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            toggle_window(&self.window);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            menu_items(&menu_model(&self.state))
        }
    }

    fn menu_items(model: &MenuModel) -> Vec<MenuItem<GigaTray>> {
        let servers: Vec<MenuItem<GigaTray>> = model
            .servers
            .iter()
            .map(|server| match server.id {
                Some(id) => CheckmarkItem {
                    label: server.label.clone(),
                    enabled: server.enabled,
                    checked: server.checked,
                    activate: Box::new(move |tray: &mut GigaTray| {
                        on_window(&tray.window, move |app| app.invoke_pick(id));
                    }),
                    ..CheckmarkItem::default()
                }
                .into(),
                None => StandardItem {
                    label: server.label.clone(),
                    enabled: false,
                    ..StandardItem::default()
                }
                .into(),
            })
            .collect();
        vec![
            StandardItem {
                label: model.header.clone(),
                enabled: false,
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: model.toggle_label.to_owned(),
                enabled: model.toggle_enabled,
                activate: Box::new(|tray: &mut GigaTray| {
                    on_window(&tray.window, |app| app.invoke_toggle());
                }),
                ..StandardItem::default()
            }
            .into(),
            SubMenu {
                label: "Сервер".to_owned(),
                submenu: servers,
                ..SubMenu::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Системный прокси".to_owned(),
                checked: model.system_proxy,
                activate: Box::new(|tray: &mut GigaTray| {
                    on_window(&tray.window, |app| app.invoke_toggle_sysproxy());
                }),
                ..CheckmarkItem::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Открыть giga-throne".to_owned(),
                activate: Box::new(|tray: &mut GigaTray| {
                    on_window(&tray.window, |app| {
                        if let Err(err) = app.window().show() {
                            warn!("could not show the window: {err}");
                        }
                    });
                }),
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: "Выход".to_owned(),
                // Leaves the event loop; main then runs the normal shutdown.
                activate: Box::new(|_: &mut GigaTray| {
                    let _ = slint::quit_event_loop();
                }),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: i64, name: &str, running: bool) -> ServerEntry {
        ServerEntry {
            id,
            name: name.to_owned(),
            running,
            current: running,
        }
    }

    fn state(link: Link, status: &str, servers: Vec<ServerEntry>) -> TrayState {
        TrayState {
            link,
            status_text: status.to_owned(),
            server_name: servers
                .iter()
                .find(|s| s.running)
                .map_or_else(String::new, |s| s.name.clone()),
            servers,
            system_proxy: true,
        }
    }

    #[test]
    fn tooltip_names_the_server_only_when_connected() {
        let on = state(Link::On, "Защищено", vec![server(1, "Берлин", true)]);
        assert_eq!(on.tooltip(), "giga-throne — Защищено · Берлин");
        let off = state(Link::Off, "Не защищено", vec![server(1, "Берлин", false)]);
        assert_eq!(off.tooltip(), "giga-throne — Не защищено");
        let busy = state(Link::Busy, "Подключаемся…", vec![]);
        assert_eq!(busy.tooltip(), "giga-throne — Подключаемся…");
    }

    #[test]
    fn link_follows_the_home_screen_codes() {
        assert_eq!(Link::from_conn("on"), Link::On);
        assert_eq!(Link::from_conn("busy"), Link::Busy);
        assert_eq!(Link::from_conn("off"), Link::Off);
        assert_eq!(Link::from_conn("???"), Link::Off);
    }

    #[test]
    fn menu_toggles_and_disables_while_busy() {
        let off = menu_model(&state(Link::Off, "Не защищено", vec![]));
        assert_eq!(off.header, "Не защищено");
        assert_eq!(off.toggle_label, "Подключить");
        assert!(off.toggle_enabled);

        let on = menu_model(&state(Link::On, "Защищено", vec![server(1, "A", true)]));
        assert_eq!(on.header, "Защищено · A");
        assert_eq!(on.toggle_label, "Отключить");
        assert!(on.toggle_enabled);

        let connecting = menu_model(&state(Link::Busy, "Подключаемся…", vec![]));
        assert_eq!(connecting.toggle_label, "Подключить");
        assert!(!connecting.toggle_enabled);

        let stopping = menu_model(&state(Link::Busy, DISCONNECTING_TEXT, vec![]));
        assert_eq!(stopping.toggle_label, "Отключить");
        assert!(!stopping.toggle_enabled);
    }

    #[test]
    fn server_submenu_checks_the_current_profile_and_caps_the_list() {
        let servers = vec![server(1, "A", false), server(2, "B", true)];
        let model = menu_model(&state(Link::On, "Защищено", servers));
        assert_eq!(model.servers.len(), 2);
        assert_eq!(model.servers[0].id, Some(1));
        assert!(!model.servers[0].checked);
        assert_eq!(model.servers[1].label, "B");
        assert!(model.servers[1].checked);
        assert!(model.servers.iter().all(|s| s.enabled));

        // Picked while idle: checked, though nothing runs.
        let mut picked = server(3, "C", false);
        picked.current = true;
        let model = menu_model(&state(Link::Off, "Не защищено", vec![picked]));
        assert!(model.servers[0].checked);

        let many: Vec<ServerEntry> = (1..=20).map(|i| server(i, "x", false)).collect();
        let model = menu_model(&state(Link::Off, "Не защищено", many));
        assert_eq!(model.servers.len(), MAX_SERVERS);
    }

    #[test]
    fn empty_server_list_shows_a_disabled_placeholder() {
        let model = menu_model(&state(Link::Off, "Не защищено", vec![]));
        assert_eq!(model.servers.len(), 1);
        assert_eq!(model.servers[0].id, None);
        assert_eq!(model.servers[0].label, "Нет серверов");
        assert!(!model.servers[0].enabled);
    }

    #[test]
    fn system_proxy_flag_is_copied() {
        let mut st = state(Link::Off, "Не защищено", vec![]);
        assert!(menu_model(&st).system_proxy);
        st.system_proxy = false;
        assert!(!menu_model(&st).system_proxy);
    }

    fn pixel(buf: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        [buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]
    }

    #[test]
    fn icon_buffer_is_argb32_of_the_right_size() {
        for size in ICON_SIZES {
            for link in [Link::On, Link::Busy, Link::Off] {
                assert_eq!(icon_pixels(size, link).len(), icon_len(size));
            }
        }
        assert_eq!(icon_len(64), 64 * 64 * 4);
    }

    #[test]
    fn icon_corners_are_transparent_and_body_is_opaque() {
        let buf = icon_pixels(64, Link::On);
        assert_eq!(pixel(&buf, 64, 0, 0)[0], 0);
        assert_eq!(pixel(&buf, 64, 63, 63)[0], 0);
        // Just below the power symbol, on the plain body: accent color, fully opaque.
        assert_eq!(pixel(&buf, 64, 32, 59), [255, 0xC8, 0xFF, 0x4D]);
    }

    #[test]
    fn icon_disconnected_is_gray_and_connected_is_lime() {
        let off = icon_pixels(32, Link::Off);
        assert_eq!(pixel(&off, 32, 16, 29), [255, 0x8A, 0x8A, 0x96]);
        let on = icon_pixels(32, Link::On);
        assert_eq!(pixel(&on, 32, 16, 29), [255, 0xC8, 0xFF, 0x4D]);
    }

    #[test]
    fn power_glyph_has_ring_and_bar_but_gap_at_top() {
        assert!(in_power_glyph(0.23, 0.55), "left side of the ring");
        assert!(in_power_glyph(0.5, 0.3), "bar through the gap");
        assert!(!in_power_glyph(0.5, 0.9), "below the ring");
        assert!(!in_power_glyph(0.65, 0.3255), "ring is open at the top");
    }

    /// Needs a session bus with a StatusNotifierWatcher (KDE, GNOME with an extension).
    /// Run with `cargo test -p giga-throne -- --ignored tray_registers`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a session bus and a StatusNotifierWatcher"]
    fn tray_registers_on_the_session_bus() {
        use ksni::TrayMethods;

        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no session bus (DBUS_SESSION_BUS_ADDRESS is not set)");
            return;
        }
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let tray = linux::GigaTray {
            window: slint::Weak::default(),
            state: TrayState::initial(),
        };
        match runtime.block_on(tray.spawn()) {
            Ok(handle) => {
                runtime.block_on(handle.shutdown());
            }
            Err(err) => eprintln!("skipped: no tray host: {err}"),
        }
    }
}
