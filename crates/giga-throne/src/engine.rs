//! Engine controller: keeps the UI in step with the Throne engine and runs the
//! commands the user triggers.
//!
//! Two tasks share one [`Engine`]:
//! - the poll loop reads `GET /v1/status`, `/v1/stats/live` and `/v1/profiles`
//!   and publishes a [`Frame`] every second;
//! - the command loop runs [`Command`]s one at a time, so two commands never
//!   overlap. The client serializes requests on its own connection, so the poll
//!   loop may wait for a long command to finish, but never breaks it.
//!
//! Nothing runs until the sidecar reports [`SidecarState::Ready`]. Every
//! transition to Ready re-checks the ports Throne listens on.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use throne_api::types::{
    ConnectionState, GetSettingsResponse, ImportResponse, InboundStatus, LiveStatsResponse,
    Profile, ProfilesResponse, Route, SettingsMap, Status,
};
use throne_api::{Client, Error, Method};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{watch, Notify};
use tokio::time::{interval_at, sleep};
use tracing::{debug, info, warn};

use crate::prefs::{self, Prefs};
use crate::presets::{self, ImportKind, PresetPlan};
use crate::sidecar::SidecarState;
use crate::sysproxy::{proxy_host, SystemProxy};
use crate::view::{
    home_view, routes_view, server_row, Frame, HomeView, Pending, RouteBusy, RouteNote, RoutesView,
    ServerRow,
};
use crate::AppWindow;

/// Interval of the status and live-stats poll.
const POLL_EVERY: Duration = Duration::from_secs(1);

/// Interval of the profile list refresh, in addition to refreshes after changes.
const PROFILES_EVERY: Duration = Duration::from_secs(15);

/// How long an error text stays on screen.
const ERROR_SHOWN_FOR: Duration = Duration::from_secs(6);

/// Seconds Throne may take to connect or disconnect before answering 202.
const CONNECTION_WAIT_SECS: u64 = 30;

/// Seconds Throne may take to import before answering 202.
const IMPORT_WAIT_SECS: u64 = 60;

/// Shown while the system proxy is wanted but Throne's local proxy is switched off.
const INBOUND_OFF_TEXT: &str = "Локальный прокси выключен в настройках движка";

/// Shown when the engine has no routing API: a stock Throne, not our fork.
const ROUTES_API_MISSING_TEXT: &str =
    "Этой версии движка не хватает API маршрутов — нужен Throne из форка (feat/route-api)";

/// Shown by the route editor placeholders until the editor exists.
const EDITOR_SOON_TEXT: &str = "Редактор маршрутов будет в следующей версии";

/// Shown when a route command arrives before the engine is ready.
const ENGINE_NOT_READY_TEXT: &str = "Движок ещё не готов";

/// Settings that name a loopback port Throne binds.
const PORT_SETTINGS: [&str; 4] = [
    "inbound_port",
    "dns_in_port",
    "clash_api_port",
    "singbox_api_port",
];

/// Where finished frames go. The app posts them to the Slint event loop.
pub type Sink = Box<dyn Fn(Frame) + Send + Sync>;

/// A user action from the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Connect with `select: last`, or disconnect when running.
    Toggle,
    /// Connect to this profile.
    Connect(i64),
    /// Import what the user pasted.
    Import(String),
    /// Flip the "Системный прокси" preference.
    ToggleSystemProxy,
    /// Make the ready-made mode with this id the active one, adding its list first if needed.
    SelectPreset(String),
    /// Make this route the active one.
    ActivateRoute(i64),
    /// Add what the Routes import box holds: a route link or remote lists.
    ImportRoute(String),
    /// Delete this route.
    DeleteRoute(i64),
    /// Show another region's presets and remember the choice.
    SetRegion(String),
    /// "Свой маршрут": the editor is not there yet.
    CreateRoute,
    /// "Изменить": the editor is not there yet.
    EditRoute(i64),
}

/// Connects the UI callbacks of `window` to commands. The receiver goes to [`run`].
pub fn bind_callbacks(window: &AppWindow) -> UnboundedReceiver<Command> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let toggle = tx.clone();
    window.on_toggle(move || {
        let _ = toggle.send(Command::Toggle);
    });
    let pick = tx.clone();
    window.on_pick(move |id| {
        let _ = pick.send(Command::Connect(i64::from(id)));
    });
    let sysproxy = tx.clone();
    window.on_toggle_sysproxy(move || {
        let _ = sysproxy.send(Command::ToggleSystemProxy);
    });
    let select = tx.clone();
    window.on_select_preset(move |id| {
        let _ = select.send(Command::SelectPreset(id.to_string()));
    });
    let activate = tx.clone();
    window.on_activate_route(move |id| {
        let _ = activate.send(Command::ActivateRoute(i64::from(id)));
    });
    let import_route = tx.clone();
    window.on_import_route(move |text| {
        let _ = import_route.send(Command::ImportRoute(text.to_string()));
    });
    let delete = tx.clone();
    window.on_delete_route(move |id| {
        let _ = delete.send(Command::DeleteRoute(i64::from(id)));
    });
    let region = tx.clone();
    window.on_set_region(move |id| {
        let _ = region.send(Command::SetRegion(id.to_string()));
    });
    let create = tx.clone();
    window.on_create_route(move || {
        let _ = create.send(Command::CreateRoute);
    });
    let edit = tx.clone();
    window.on_edit_route(move |id| {
        let _ = edit.send(Command::EditRoute(i64::from(id)));
    });
    window.on_import_link(move |text| {
        let _ = tx.send(Command::Import(text.to_string()));
    });
    rx
}

/// Runs the poll loop and the command loop until the sidecar stops.
pub async fn run(
    engine: Arc<Engine>,
    state: watch::Receiver<SidecarState>,
    commands: UnboundedReceiver<Command>,
) {
    let commands_task = tokio::spawn(command_loop(Arc::clone(&engine), state.clone(), commands));
    engine.poll_loop(state).await;
    commands_task.abort();
}

/// The controller. Cheap to share; all state sits behind [`Shared`].
pub struct Engine {
    client: Arc<Client>,
    /// Our own API port, which Throne must not take for itself.
    api_port: u16,
    shared: Arc<Shared>,
    /// Wakes the poll loop after a command, so the lists refresh at once.
    wake: Notify,
    /// The desktop proxy. Shared with the app, which restores it on exit.
    sysproxy: Arc<SystemProxy>,
    /// Where the preferences are saved.
    prefs_path: PathBuf,
    proxy_track: Mutex<ProxyTrack>,
}

impl Engine {
    pub fn new(
        client: Arc<Client>,
        api_port: u16,
        sink: Sink,
        sysproxy: Arc<SystemProxy>,
        prefs_path: PathBuf,
    ) -> Arc<Self> {
        let prefs = prefs::load(&prefs_path);
        Arc::new(Self {
            client,
            api_port,
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    prefs,
                    ..State::default()
                }),
                sink,
            }),
            wake: Notify::new(),
            sysproxy,
            prefs_path,
            proxy_track: Mutex::new(ProxyTrack::default()),
        })
    }

    /// Polls the engine for as long as it is Ready.
    async fn poll_loop(&self, mut state: watch::Receiver<SidecarState>) {
        loop {
            if state.wait_for(is_ready).await.is_err() {
                return;
            }
            if let Err(err) = self.fix_ports().await {
                warn!("could not check Throne's ports: {err}");
            }

            let mut tick = interval_at(tokio::time::Instant::now() + POLL_EVERY, POLL_EVERY);
            let mut profiles_at: Option<Instant> = None;
            loop {
                let with_profiles = profiles_at.is_none_or(|at| at.elapsed() >= PROFILES_EVERY);
                self.refresh(with_profiles).await;
                if with_profiles {
                    profiles_at = Some(Instant::now());
                }
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = self.wake.notified() => profiles_at = None,
                    changed = state.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        // Left Ready or came back: release the proxy if Throne is down,
                        // re-check the ports and restart the loop.
                        if !is_ready(&state.borrow()) {
                            self.sync_system_proxy(None);
                        }
                        break;
                    }
                }
            }
        }
    }

    /// Reads the status, the live stats while running and, if asked, the profiles.
    async fn refresh(&self, with_profiles: bool) {
        match self
            .client
            .request_json::<Status>(Method::Get, "/v1/status", None, None)
            .await
        {
            Ok(status) => {
                let stats = if status.state == ConnectionState::Running {
                    self.live_stats().await
                } else {
                    None
                };
                self.sync_system_proxy(Some(&status));
                self.shared.update(|st| {
                    st.set_status(status);
                    st.stats = stats;
                });
            }
            Err(err) => debug!("status poll failed: {err}"),
        }
        if with_profiles {
            match self
                .client
                .request_json::<ProfilesResponse>(Method::Get, "/v1/profiles", None, None)
                .await
            {
                Ok(list) => self.shared.update(|st| st.set_profiles(list.profiles)),
                Err(err) => debug!("profiles poll failed: {err}"),
            }
            match self.client.routes().await {
                Ok(routes) => self.shared.update(|st| st.routes = routes),
                Err(err) => debug!("routes poll failed: {err}"),
            }
        }
    }

    async fn live_stats(&self) -> Option<LiveStatsResponse> {
        self.client
            .request_json(Method::Get, "/v1/stats/live", None, None)
            .await
            .map_err(|err| debug!("live stats poll failed: {err}"))
            .ok()
    }

    /// Runs one command to its end and reports its error, if any.
    pub async fn handle(&self, command: Command) {
        match command {
            Command::Toggle => self.report_connection(self.toggle().await),
            Command::Connect(id) => self.report_connection(self.connect(id).await),
            Command::Import(text) => self.import(&text).await,
            Command::ToggleSystemProxy => self.toggle_system_proxy(),
            Command::SelectPreset(id) => {
                let work = self.select_preset(&id);
                let result = self.while_busy(RouteBusy::Preset(id.clone()), work).await;
                self.report_route(result);
            }
            Command::ActivateRoute(id) => {
                let work = self.activate_route(id);
                let result = self.while_busy(RouteBusy::Route(id), work).await;
                self.report_route(result);
            }
            Command::ImportRoute(text) => {
                let work = self.import_route(&text);
                let result = self.while_busy(RouteBusy::Import, work).await;
                self.report_route(result);
            }
            Command::DeleteRoute(id) => {
                let work = self.delete_route(id);
                let result = self.while_busy(RouteBusy::Route(id), work).await;
                self.report_route(result);
            }
            Command::SetRegion(region) => self.set_region(&region),
            Command::CreateRoute => self.report_route(Ok(EDITOR_SOON_TEXT.to_owned())),
            Command::EditRoute(_) => self.report_route(Ok(EDITOR_SOON_TEXT.to_owned())),
        }
        let status = self.shared.read(|st| st.status.clone());
        self.sync_system_proxy(status.as_ref());
        self.wake.notify_one();
    }

    /// Flips the "Системный прокси" preference and saves it. The caller syncs the proxy.
    pub fn toggle_system_proxy(&self) {
        let prefs = self.shared.read(|st| Prefs {
            system_proxy: !st.prefs.system_proxy,
            ..st.prefs.clone()
        });
        self.save_prefs(prefs);
    }

    /// Saves the preferences and makes them the current ones.
    fn save_prefs(&self, prefs: Prefs) {
        if let Err(err) = prefs::save(&self.prefs_path, &prefs) {
            warn!("could not save preferences: {err}");
        }
        self.shared.update(|st| st.prefs = prefs);
    }

    /// Remembers the region of the Routes screen. Unknown regions are ignored.
    fn set_region(&self, region: &str) {
        if !presets::is_region(region) {
            return;
        }
        let prefs = self.shared.read(|st| Prefs {
            route_region: region.to_owned(),
            ..st.prefs.clone()
        });
        self.save_prefs(prefs);
    }

    /// Shows `busy` on the Routes screen while `work` runs.
    async fn while_busy<T>(&self, busy: RouteBusy, work: impl Future<Output = T>) -> T {
        self.shared.update(|st| st.route_busy = Some(busy));
        let result = work.await;
        self.shared.update(|st| st.route_busy = None);
        result
    }

    /// Shows the result of a routing command under the Routes screen.
    fn report_route(&self, result: Result<String, String>) {
        match result {
            Ok(text) => flash(&self.shared, Flash::Routes { bad: false }, text),
            Err(text) => {
                warn!("routing command failed: {text}");
                flash(&self.shared, Flash::Routes { bad: true }, text);
            }
        }
    }

    /// Chooses a ready-made mode. Activates its route, or adds its list first.
    ///
    /// Returns the text for the Routes screen: empty on success, the error otherwise.
    async fn select_preset(&self, id: &str) -> Result<String, String> {
        let Some(preset) = presets::find(id) else {
            return Err("Неизвестный режим".to_owned());
        };
        let routes = self.shared.read(|st| st.routes.clone());
        match presets::plan_preset(&routes, preset) {
            PresetPlan::Activate(route_id) => self.activate_route(route_id).await,
            PresetPlan::AddThenActivate(url) => {
                let link =
                    presets::preset_link(preset).ok_or_else(|| NO_DEFAULT_ROUTE_TEXT.to_owned())?;
                let added = self
                    .client
                    .add_remote_routes(&link, true)
                    .await
                    .map_err(|err| route_error_text(&err, true))?;
                let route_id = presets::pick_added_route(&added, &url)?;
                self.activate_route(route_id).await
            }
            PresetPlan::Unavailable => Err(NO_DEFAULT_ROUTE_TEXT.to_owned()),
        }
    }

    async fn activate_route(&self, id: i64) -> Result<String, String> {
        match self.client.set_active_route(id).await {
            Ok(status) => {
                self.shared.update(|st| st.set_status(status));
                Ok(String::new())
            }
            Err(err) => Err(route_error_text(&err, false)),
        }
    }

    /// Adds what the import box holds: a route link, or remote lists.
    async fn import_route(&self, text: &str) -> Result<String, String> {
        let text = text.trim();
        match presets::classify_import(text) {
            ImportKind::Link => self
                .client
                .add_route_link(text)
                .await
                .map(|added| format!("Добавлено: {}", added.routes.len()))
                .map_err(|err| route_error_text(&err, true)),
            ImportKind::Remote => self
                .client
                .add_remote_routes(text, true)
                .await
                .map(|added| {
                    let failed = added.results.iter().filter(|result| !result.ok).count();
                    route_summary(added.routes.len(), failed)
                })
                .map_err(|err| route_error_text(&err, true)),
            ImportKind::Invalid => Err("Это не похоже на ссылку маршрута".to_owned()),
        }
    }

    async fn delete_route(&self, id: i64) -> Result<String, String> {
        // Only a route we know exists can make a 404 mean a missing endpoint.
        let known = self.shared.read(|st| st.routes.iter().any(|r| r.id == id));
        match self.client.delete_route(id).await {
            Ok(()) => Ok("Маршрут удалён".to_owned()),
            Err(err) => Err(route_error_text(&err, known)),
        }
    }

    /// Makes the desktop proxy match the preference and the engine state.
    ///
    /// `status` is the engine's status, or `None` when the engine is not Ready. A
    /// failing enable is shown once and not retried every second, only when the
    /// wanted endpoint changes. Never blocks on the network.
    pub fn sync_system_proxy(&self, status: Option<&Status>) {
        let pref = self.shared.read(|st| st.prefs.system_proxy);
        let state = status.map(|s| s.state);
        let inbound = status.map(|s| &s.inbound);
        let applied = self.sysproxy.applied();
        let applied = applied.as_ref().map(|(host, port)| (host.as_str(), *port));
        let action = proxy_action(state, pref, inbound, applied);

        let mut track = self
            .proxy_track
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if inbound_blocked(state, pref, inbound) {
            if !track.blocked {
                flash_error(&self.shared, INBOUND_OFF_TEXT.to_owned());
            }
            track.blocked = true;
        } else {
            track.blocked = false;
        }

        let enabling = matches!(action, ProxyAction::Enable { .. });
        let result = match &action {
            ProxyAction::Nothing => {
                track.failed = None;
                return;
            }
            ProxyAction::Enable { .. } if track.failed.as_ref() == Some(&action) => return,
            ProxyAction::Enable { host, port } => self.sysproxy.enable(host, *port),
            ProxyAction::Disable => self.sysproxy.disable(),
        };
        match result {
            Ok(()) => track.failed = None,
            Err(err) => {
                warn!("system proxy change failed: {err:#}");
                let verb = if enabling {
                    "включить"
                } else {
                    "выключить"
                };
                flash_error(
                    &self.shared,
                    format!("Не удалось {verb} системный прокси: {err:#}"),
                );
                track.failed = Some(action);
            }
        }
    }

    fn report_connection(&self, result: Result<(), Error>) {
        if let Err(err) = result {
            warn!("connection command failed: {err}");
            flash_error(&self.shared, error_text(&err));
        }
    }

    /// Starts the last profile when idle, stops it when running. Other states are busy.
    async fn toggle(&self) -> Result<(), Error> {
        let state = self.shared.read(|st| st.status.as_ref().map(|s| s.state));
        match state {
            Some(ConnectionState::Running) => {
                self.change_connection(
                    Pending::Disconnecting,
                    Method::Delete,
                    json!({ "timeout": CONNECTION_WAIT_SECS }),
                )
                .await
            }
            Some(ConnectionState::Connecting | ConnectionState::Stopping) => Ok(()),
            _ => {
                self.change_connection(
                    Pending::Connecting,
                    Method::Put,
                    json!({ "select": "last", "timeout": CONNECTION_WAIT_SECS }),
                )
                .await
            }
        }
    }

    async fn connect(&self, id: i64) -> Result<(), Error> {
        let already = self.shared.read(|st| {
            st.status.as_ref().is_some_and(|s| {
                s.state == ConnectionState::Running
                    && s.profile.as_ref().is_some_and(|p| p.id == id)
            })
        });
        if already {
            return Ok(());
        }
        self.change_connection(
            Pending::Connecting,
            Method::Put,
            json!({ "profile": id, "timeout": CONNECTION_WAIT_SECS }),
        )
        .await
    }

    /// Sends a connection request and shows `pending` until it answers.
    async fn change_connection(
        &self,
        pending: Pending,
        method: Method,
        body: Value,
    ) -> Result<(), Error> {
        self.shared.update(|st| st.pending = Some(pending));
        let result = self
            .client
            .request_json::<Status>(method, "/v1/connection", None, Some(body))
            .await;
        match result {
            Ok(status) => {
                self.shared.update(|st| {
                    st.pending = None;
                    st.set_status(status);
                });
                Ok(())
            }
            Err(err) => {
                self.shared.update(|st| st.pending = None);
                Err(err)
            }
        }
    }

    async fn import(&self, text: &str) {
        let Some(body) = import_request(text) else {
            return;
        };
        let result = self
            .client
            .request_json::<ImportResponse>(Method::Post, "/v1/import", None, Some(body))
            .await;
        match result {
            Ok(response) => {
                let summary = import_summary(&response);
                self.shared.update(|st| {
                    st.import_result = summary;
                    st.clear_import = true;
                });
            }
            Err(err) => {
                warn!("import failed: {err}");
                flash_import_error(&self.shared, error_text(&err));
            }
        }
    }

    /// Moves Throne's ports away from the ones another program holds on this machine.
    ///
    /// Only ports above zero are checked. Changed keys are sent in one `PUT /v1/settings`.
    /// Skipped unless Throne is idle: a resumed profile holds its own inbound port, which
    /// the bind check would take for a foreign one and move on every restart.
    pub async fn fix_ports(&self) -> Result<(), Error> {
        let status: Status = self
            .client
            .request_json(Method::Get, "/v1/status", None, None)
            .await?;
        if status.state != ConnectionState::Idle {
            debug!("port check skipped: Throne is {:?}", status.state);
            return Ok(());
        }
        let settings: GetSettingsResponse<SettingsMap> = self
            .client
            .request_json(Method::Get, "/v1/settings", None, None)
            .await?;
        let ports: Vec<(&'static str, u16)> = PORT_SETTINGS
            .iter()
            .filter_map(|key| {
                let value = settings.settings.get(*key)?.as_u64()?;
                let port = u16::try_from(value).ok()?;
                (port > 0).then_some((*key, port))
            })
            .collect();
        let moves = plan_port_moves(&ports, self.api_port, port_is_free);
        if moves.is_empty() {
            return Ok(());
        }
        let changes: SettingsMap = moves
            .iter()
            .map(|(key, port)| ((*key).to_owned(), json!(port)))
            .collect();
        self.client
            .request(
                Method::Put,
                "/v1/settings",
                None,
                Some(json!({ "settings": changes })),
            )
            .await?;
        for (key, port) in &moves {
            info!("Throne port {key} was busy, moved to {port}");
        }
        Ok(())
    }
}

fn is_ready(state: &SidecarState) -> bool {
    matches!(state, SidecarState::Ready { .. })
}

/// Drains commands one by one. Each command runs to its end before the next starts.
async fn command_loop(
    engine: Arc<Engine>,
    state: watch::Receiver<SidecarState>,
    mut commands: UnboundedReceiver<Command>,
) {
    while let Some(command) = commands.recv().await {
        let ready = is_ready(&state.borrow());
        if !ready {
            match command {
                // Local settings: they do not need the engine.
                Command::ToggleSystemProxy => {
                    // With no engine running, the proxy must be off.
                    engine.toggle_system_proxy();
                    engine.sync_system_proxy(None);
                }
                Command::SetRegion(region) => engine.set_region(&region),
                Command::Toggle | Command::Connect(_) | Command::Import(_) => {
                    flash_error(&engine.shared, ENGINE_NOT_READY_TEXT.to_owned());
                }
                _ => engine.report_route(Err(ENGINE_NOT_READY_TEXT.to_owned())),
            }
            continue;
        }
        engine.handle(command).await;
    }
}

/// Which text a flashed message goes to.
#[derive(Clone, Copy)]
enum Flash {
    /// The error line on the Home screen.
    Home,
    /// The line under the import box.
    Import,
    /// The info or error line of the Routes screen.
    Routes { bad: bool },
}

/// Shows an error text for [`ERROR_SHOWN_FOR`], unless a newer one replaces it.
fn flash_error(shared: &Arc<Shared>, text: String) {
    flash(shared, Flash::Home, text);
}

/// Shows an error text in the import box for [`ERROR_SHOWN_FOR`].
fn flash_import_error(shared: &Arc<Shared>, text: String) {
    flash(shared, Flash::Import, text);
}

fn flash(shared: &Arc<Shared>, target: Flash, text: String) {
    let mut seq = 0;
    shared.update(|st| match target {
        Flash::Home => {
            st.error_seq += 1;
            seq = st.error_seq;
            st.error = text;
        }
        Flash::Import => {
            st.import_seq += 1;
            seq = st.import_seq;
            st.import_result = text;
        }
        Flash::Routes { bad } => {
            st.route_seq += 1;
            seq = st.route_seq;
            st.route_note = RouteNote { text, bad };
        }
    });
    let shared = Arc::clone(shared);
    tokio::spawn(async move {
        sleep(ERROR_SHOWN_FOR).await;
        shared.update(|st| match target {
            Flash::Home if st.error_seq == seq => st.error.clear(),
            Flash::Import if st.import_seq == seq => st.import_result.clear(),
            Flash::Routes { .. } if st.route_seq == seq => st.route_note = RouteNote::default(),
            _ => {}
        });
    });
}

/// What the desktop proxy should be, given the engine state and the preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyAction {
    /// Point the desktop proxy at this endpoint, or move it there.
    Enable { host: String, port: u16 },
    /// Restore the desktop proxy.
    Disable,
    /// Leave it as it is.
    Nothing,
}

/// Decides the desktop proxy from the engine state, the preference, Throne's
/// inbound and the endpoint that is applied now.
///
/// The proxy is wanted only while Throne runs, the preference is on and the
/// inbound is enabled. A different endpoint, such as a new port, is applied again.
pub fn proxy_action(
    state: Option<ConnectionState>,
    pref: bool,
    inbound: Option<&InboundStatus>,
    applied: Option<(&str, u16)>,
) -> ProxyAction {
    match (wanted_endpoint(state, pref, inbound), applied) {
        (Some((host, port)), Some((applied_host, applied_port)))
            if host == applied_host && port == applied_port =>
        {
            ProxyAction::Nothing
        }
        (Some((host, port)), _) => ProxyAction::Enable { host, port },
        (None, Some(_)) => ProxyAction::Disable,
        (None, None) => ProxyAction::Nothing,
    }
}

/// The endpoint the system proxy should use, when it is wanted at all.
fn wanted_endpoint(
    state: Option<ConnectionState>,
    pref: bool,
    inbound: Option<&InboundStatus>,
) -> Option<(String, u16)> {
    if !pref || state != Some(ConnectionState::Running) {
        return None;
    }
    let inbound = inbound.filter(|inbound| inbound.enabled)?;
    Some((proxy_host(&inbound.address), inbound.port))
}

/// Whether the proxy is wanted but Throne's local proxy is switched off.
pub fn inbound_blocked(
    state: Option<ConnectionState>,
    pref: bool,
    inbound: Option<&InboundStatus>,
) -> bool {
    pref && state == Some(ConnectionState::Running) && inbound.is_some_and(|i| !i.enabled)
}

/// What the last system proxy change did, so that a failure is not retried every second.
#[derive(Default)]
struct ProxyTrack {
    /// The change that failed, retried only when the wanted endpoint changes.
    failed: Option<ProxyAction>,
    /// The "inbound is off" error is already on screen.
    blocked: bool,
}

/// Engine state the UI shows. Guarded by a mutex; never held across an await.
#[derive(Default)]
struct State {
    status: Option<Status>,
    stats: Option<LiveStatsResponse>,
    profiles: Vec<Profile>,
    routes: Vec<Route>,
    /// The Routes screen as last sent to the UI, to send it again only when it changes.
    routes_sent: Option<RoutesView>,
    route_busy: Option<RouteBusy>,
    route_note: RouteNote,
    route_seq: u64,
    pending: Option<Pending>,
    prefs: Prefs,
    /// Profile id and start time of the current session, while running.
    running_since: Option<(i64, Instant)>,
    error: String,
    error_seq: u64,
    import_result: String,
    import_seq: u64,
    clear_import: bool,
    profiles_rev: u64,
    servers_rev: u64,
}

impl State {
    fn set_status(&mut self, status: Status) {
        let profile_id = status.profile.as_ref().map_or(-1, |p| p.id);
        self.running_since = match status.state {
            ConnectionState::Running => match self.running_since {
                Some((id, at)) if id == profile_id => Some((id, at)),
                _ => Some((profile_id, Instant::now())),
            },
            _ => None,
        };
        self.status = Some(status);
    }

    fn set_profiles(&mut self, profiles: Vec<Profile>) {
        if self.profiles != profiles {
            self.profiles = profiles;
            self.profiles_rev += 1;
        }
    }

    /// The Routes screen, when it differs from the one last sent.
    fn routes_frame(&mut self) -> Option<RoutesView> {
        let active = self
            .status
            .as_ref()
            .and_then(|s| s.route.as_ref())
            .map(|route| route.id)
            .or_else(|| self.routes.iter().find(|r| r.active).map(|r| r.id));
        let view = routes_view(
            &self.routes,
            active,
            &self.prefs.route_region,
            self.route_busy.as_ref(),
            &self.route_note,
            now_secs(),
        );
        if self.routes_sent.as_ref() == Some(&view) {
            return None;
        }
        self.routes_sent = Some(view.clone());
        Some(view)
    }

    /// Builds the frame for the UI. The server list is sent only when it changed.
    fn frame(&mut self) -> Frame {
        let timer = self
            .running_since
            .map(|(_, at)| crate::view::format_duration(at.elapsed()))
            .unwrap_or_default();
        let home: HomeView = home_view(
            self.status.as_ref(),
            self.stats.as_ref(),
            &self.profiles,
            self.pending,
            timer,
        );
        let servers = (self.servers_rev != self.profiles_rev).then(|| {
            self.servers_rev = self.profiles_rev;
            self.profiles
                .iter()
                .map(server_row)
                .collect::<Vec<ServerRow>>()
        });
        let routes = self.routes_frame();
        let clear_import = std::mem::take(&mut self.clear_import);
        Frame {
            home,
            servers,
            routes,
            import_result: self.import_result.clone(),
            error: self.error.clone(),
            clear_import,
            system_proxy: self.prefs.system_proxy,
        }
    }
}

/// [`State`] plus the sink that shows its frames.
struct Shared {
    state: Mutex<State>,
    sink: Sink,
}

impl Shared {
    /// Applies `change`, then sends the resulting frame. The lock is held while
    /// sending so frames reach the UI in the order they were made.
    fn update(&self, change: impl FnOnce(&mut State)) {
        let mut st = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        change(&mut st);
        let frame = st.frame();
        (self.sink)(frame);
    }

    fn read<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        let st = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&st)
    }
}

/// Seconds since the Unix epoch, for the "N мин назад" texts.
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0))
}

/// Text under the import box after adding `added` profiles, `failed` of them with errors.
fn route_summary(added: usize, failed: usize) -> String {
    if failed > 0 {
        format!("Добавлено: {added}, с ошибками: {failed}")
    } else {
        format!("Добавлено: {added}")
    }
}

fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Chooses new ports for the ones that are busy.
///
/// `ports` are `(setting, port)` pairs. A busy port moves to the first port from
/// `port + 10` upward that is free, is not one of `ports`, and is not `reserved`
/// (our own API port). Returns only the settings that change.
pub fn plan_port_moves(
    ports: &[(&'static str, u16)],
    reserved: u16,
    is_free: impl Fn(u16) -> bool,
) -> Vec<(&'static str, u16)> {
    let mut taken: Vec<u16> = ports.iter().map(|&(_, port)| port).collect();
    taken.push(reserved);
    let mut moves = Vec::new();
    for (index, &(key, port)) in ports.iter().enumerate() {
        if is_free(port) {
            continue;
        }
        let Some(start) = port.checked_add(10) else {
            warn!("no free port for {key} near {port}");
            continue;
        };
        match (start..=u16::MAX).find(|&c| !taken.contains(&c) && is_free(c)) {
            Some(candidate) => {
                taken[index] = candidate;
                moves.push((key, candidate));
            }
            None => warn!("no free port for {key} near {port}"),
        }
    }
    moves
}

/// Body of `POST /v1/import` for what the user pasted, or `None` when it is empty.
///
/// A single line that is an http(s) URL is downloaded (`url`); anything else is
/// parsed as share links or a document (`profiles`).
pub fn import_request(text: &str) -> Option<Value> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let single_line = !text.contains(['\n', '\r']) && !text.contains(char::is_whitespace);
    let is_url = text.starts_with("http://") || text.starts_with("https://");
    if single_line && is_url {
        Some(json!({ "as": "url", "url": text, "timeout": IMPORT_WAIT_SECS }))
    } else {
        Some(json!({ "as": "profiles", "text": text, "timeout": IMPORT_WAIT_SECS }))
    }
}

/// Text under the import box after a successful import.
pub fn import_summary(response: &ImportResponse) -> String {
    if response.state.is_some() {
        return "Импорт продолжается в фоне".to_owned();
    }
    let imported = response.imported.unwrap_or(0);
    let failed = response.failed.unwrap_or(0);
    if failed > 0 {
        format!("Добавлено: {imported}, с ошибками: {failed}")
    } else {
        format!("Добавлено: {imported}")
    }
}

/// Shown when the "Всё через VPN" mode has no "Default" profile to activate.
const NO_DEFAULT_ROUTE_TEXT: &str = "Нет профиля Default";

/// Russian text for a failed routing request.
///
/// `endpoint_missing` says that a 404 means the routing endpoint itself is not
/// there, so the engine is not our fork. Other errors use [`error_text`].
pub fn route_error_text(err: &Error, endpoint_missing: bool) -> String {
    match err {
        Error::Api { status: 404, .. } if endpoint_missing => ROUTES_API_MISSING_TEXT.to_owned(),
        Error::Api { code, .. } => match code.as_str() {
            "not_found" => "Маршрут не найден".to_owned(),
            "route_active" => "Сначала выбери другой маршрут".to_owned(),
            "last_route" => "Нельзя удалить последний маршрут".to_owned(),
            "remote_route" => "Этот маршрут обновляется по ссылке и не редактируется".to_owned(),
            "not_remote" => "Этот маршрут не загружается по ссылке".to_owned(),
            "editing" => "Маршруты открыты в редакторе Throne".to_owned(),
            _ => error_text(err),
        },
        _ => error_text(err),
    }
}

/// Russian text for an API error.
pub fn error_text(err: &Error) -> String {
    match err {
        Error::Api { code, message, .. } => match code.as_str() {
            "no_profiles" => "Сначала добавь сервер".to_owned(),
            "no_working_profile" => "Ни один сервер не отвечает".to_owned(),
            "busy" => "Подожди, движок занят".to_owned(),
            "not_found" => "Сервер не найден".to_owned(),
            "kill_switch_inactive" | "kill_switch_blocked" => {
                "Kill switch блокирует подключение".to_owned()
            }
            "tun_failed" => "Не удалось поднять TUN".to_owned(),
            "start_failed" | "build_failed" => {
                format!("Не удалось запустить сервер: {message}")
            }
            "core_unavailable" => "Ядро не запущено".to_owned(),
            "nothing_imported" => format!("Ничего не импортировано: {message}"),
            "fetch_failed" => "Не удалось скачать список".to_owned(),
            "geo_assets_missing" => "Нет geo-файлов".to_owned(),
            _ => format!("Ошибка: {message}"),
        },
        Error::Timeout => "Движок не ответил вовремя".to_owned(),
        Error::Io(_) | Error::Closed => "Нет связи с движком".to_owned(),
        other => format!("Ошибка: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use throne_api::types::ImportResponse;

    fn api_error(code: &str, message: &str) -> Error {
        Error::Api {
            status: 409,
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn error_codes_have_russian_texts() {
        assert_eq!(
            error_text(&api_error("no_profiles", "")),
            "Сначала добавь сервер"
        );
        assert_eq!(
            error_text(&api_error("kill_switch_blocked", "")),
            "Kill switch блокирует подключение"
        );
        assert_eq!(
            error_text(&api_error("start_failed", "bad outbound")),
            "Не удалось запустить сервер: bad outbound"
        );
        assert_eq!(
            error_text(&api_error("nothing_imported", "2 failed")),
            "Ничего не импортировано: 2 failed"
        );
        assert_eq!(error_text(&api_error("weird", "boom")), "Ошибка: boom");
    }

    #[test]
    fn transport_errors_have_russian_texts() {
        assert_eq!(error_text(&Error::Timeout), "Движок не ответил вовремя");
        assert_eq!(error_text(&Error::Closed), "Нет связи с движком");
        let io = std::io::Error::other("refused");
        assert_eq!(error_text(&Error::Io(io)), "Нет связи с движком");
    }

    #[test]
    fn busy_ports_move_to_the_first_free_slot_above_ten() {
        let ports = [("inbound_port", 2080), ("dns_in_port", 6450)];
        let moves = plan_port_moves(&ports, 9096, |p| p != 2080);
        assert_eq!(moves, vec![("inbound_port", 2090)]);
    }

    #[test]
    fn free_ports_are_left_alone() {
        let ports = [("inbound_port", 2080), ("clash_api_port", 9090)];
        assert!(plan_port_moves(&ports, 9096, |_| true).is_empty());
    }

    #[test]
    fn candidates_avoid_other_ports_and_our_api_port() {
        // 2090 is our API port, so inbound takes 2091. dns_in_port is busy too and
        // moves past its own 2100 + 10.
        let ports = [("inbound_port", 2080), ("dns_in_port", 2100)];
        let moves = plan_port_moves(&ports, 2090, |p| p != 2080 && p != 2100);
        assert_eq!(moves, vec![("inbound_port", 2091), ("dns_in_port", 2110)]);
    }

    #[test]
    fn two_busy_ports_get_different_slots() {
        let ports = [("inbound_port", 2080), ("clash_api_port", 2090)];
        // 2090 is held by the other listed port, so inbound takes 2091 and clash 2100.
        let moves = plan_port_moves(&ports, 9096, |p| p != 2080 && p != 2090);
        assert_eq!(
            moves,
            vec![("inbound_port", 2091), ("clash_api_port", 2100)]
        );
    }

    #[test]
    fn import_body_is_url_only_for_a_single_http_line() {
        assert_eq!(
            import_request("  https://example.com/list.txt \n").unwrap(),
            json!({"as": "url", "url": "https://example.com/list.txt", "timeout": 60})
        );
        assert_eq!(
            import_request("http://127.0.0.1:9#e2e").unwrap()["as"],
            "url"
        );
        assert_eq!(
            import_request("vless://a\nhttps://example.com").unwrap()["as"],
            "profiles"
        );
        assert_eq!(
            import_request("vless://uuid@host:443#x").unwrap(),
            json!({"as": "profiles", "text": "vless://uuid@host:443#x", "timeout": 60})
        );
        assert!(import_request("   \n").is_none());
    }

    fn inbound(enabled: bool, address: &str, port: u16) -> InboundStatus {
        InboundStatus {
            enabled,
            address: address.to_owned(),
            port,
        }
    }

    #[test]
    fn running_with_the_preference_wants_the_proxy_at_the_inbound() {
        let on = inbound(true, "0.0.0.0", 2080);
        assert_eq!(
            proxy_action(Some(ConnectionState::Running), true, Some(&on), None),
            ProxyAction::Enable {
                host: "127.0.0.1".to_owned(),
                port: 2080
            }
        );
    }

    #[test]
    fn the_applied_endpoint_is_left_alone_and_a_new_port_moves_it() {
        let on = inbound(true, "127.0.0.1", 2080);
        let running = Some(ConnectionState::Running);
        assert_eq!(
            proxy_action(running, true, Some(&on), Some(("127.0.0.1", 2080))),
            ProxyAction::Nothing
        );
        assert_eq!(
            proxy_action(running, true, Some(&on), Some(("127.0.0.1", 1080))),
            ProxyAction::Enable {
                host: "127.0.0.1".to_owned(),
                port: 2080
            }
        );
    }

    #[test]
    fn leaving_running_or_turning_the_preference_off_restores_the_proxy() {
        let on = inbound(true, "127.0.0.1", 2080);
        let applied = Some(("127.0.0.1", 2080));
        for state in [
            Some(ConnectionState::Idle),
            Some(ConnectionState::Stopping),
            None,
        ] {
            assert_eq!(
                proxy_action(state, true, Some(&on), applied),
                ProxyAction::Disable
            );
        }
        assert_eq!(
            proxy_action(Some(ConnectionState::Running), false, Some(&on), applied),
            ProxyAction::Disable
        );
    }

    #[test]
    fn nothing_is_done_when_nothing_is_applied_and_nothing_is_wanted() {
        let on = inbound(true, "127.0.0.1", 2080);
        assert_eq!(
            proxy_action(Some(ConnectionState::Idle), true, Some(&on), None),
            ProxyAction::Nothing
        );
        assert_eq!(
            proxy_action(Some(ConnectionState::Running), false, Some(&on), None),
            ProxyAction::Nothing
        );
        assert_eq!(proxy_action(None, true, None, None), ProxyAction::Nothing);
    }

    #[test]
    fn a_disabled_inbound_is_never_enabled_and_is_reported() {
        let off = inbound(false, "127.0.0.1", 2080);
        let running = Some(ConnectionState::Running);
        assert_eq!(
            proxy_action(running, true, Some(&off), None),
            ProxyAction::Nothing
        );
        assert!(inbound_blocked(running, true, Some(&off)));
        assert!(!inbound_blocked(running, false, Some(&off)));
        assert!(!inbound_blocked(
            Some(ConnectionState::Idle),
            true,
            Some(&off)
        ));
        // Already applied and now the inbound is off: the proxy must not point at it.
        assert_eq!(
            proxy_action(running, true, Some(&off), Some(("127.0.0.1", 2080))),
            ProxyAction::Disable
        );
    }

    #[test]
    fn import_summary_counts_failures() {
        let mut response = ImportResponse {
            group: 1,
            state: None,
            imported: Some(3),
            profile_ids: vec![],
            failed: None,
            warned: None,
            issues: vec![],
            errors: vec![],
        };
        assert_eq!(import_summary(&response), "Добавлено: 3");
        response.failed = Some(2);
        assert_eq!(import_summary(&response), "Добавлено: 3, с ошибками: 2");
        response.state = Some("queued".to_owned());
        assert_eq!(import_summary(&response), "Импорт продолжается в фоне");
    }

    /// Full controller run against the real Throne from `sidecar/`: port fix,
    /// profile import, connect, live stats, disconnect, shutdown.
    #[tokio::test]
    #[ignore = "needs the real Throne binary in sidecar/Throne"]
    async fn engine_e2e_real_throne() {
        use crate::sidecar::{Sidecar, SidecarConfig, SidecarState};
        use std::path::PathBuf;

        let binary = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sidecar/Throne/Throne"
        ));
        let dir = tempfile::tempdir().expect("temp dir");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("local addr")
            .port();
        let config = SidecarConfig {
            binary,
            data_root: dir.path().to_path_buf(),
            port,
        };

        let handle = Sidecar::start(config).await.expect("start sidecar");
        let mut state = handle.state();
        state.wait_for(is_ready).await.expect("state channel open");
        println!("engine Ready");

        // A fake runner: the real desktop proxy must not change during this test.
        let sysproxy = SystemProxy::with_runner(
            dir.path(),
            Arc::new(crate::sysproxy::runner::FakeRunner::new()),
        );
        let engine = Engine::new(
            handle.client(),
            port,
            Box::new(|_| {}),
            Arc::new(sysproxy),
            dir.path().join("prefs.json"),
        );

        engine.fix_ports().await.expect("port check");
        let settings: GetSettingsResponse<SettingsMap> = engine
            .client
            .request_json(Method::Get, "/v1/settings", None, None)
            .await
            .expect("GET /v1/settings");
        println!(
            "inbound_port = {:?}, dns_in_port = {:?}, clash_api_port = {:?}",
            settings.settings.get("inbound_port"),
            settings.settings.get("dns_in_port"),
            settings.settings.get("clash_api_port"),
        );

        // A line with only an http URL is an HTTP proxy profile in profiles mode.
        // Command::Import would download it instead, so the raw request is used here.
        let mut imported = None;
        for line in ["http://127.0.0.1:9#e2e", "socks5://127.0.0.1:9#e2e"] {
            let body = json!({"as": "profiles", "text": line});
            match engine
                .client
                .request_json::<ImportResponse>(Method::Post, "/v1/import", None, Some(body))
                .await
            {
                Ok(response) => {
                    println!("import {line:?} -> {}", import_summary(&response));
                    imported = Some(response);
                    break;
                }
                Err(err) => println!("import {line:?} failed: {}", error_text(&err)),
            }
        }
        let imported = imported.expect("Throne imports a proxy profile");
        assert!(imported.imported.unwrap_or(0) >= 1, "{imported:?}");

        engine.refresh(true).await;
        let profile_id = engine
            .shared
            .read(|st| st.profiles.iter().find(|p| p.name == "e2e").map(|p| p.id));
        let profile_id = profile_id.expect("the imported profile is listed");
        println!("profile id = {profile_id}");

        engine.handle(Command::Connect(profile_id)).await;
        let (running, error) = engine
            .shared
            .read(|st| (st.status.as_ref().map(|s| s.state), st.error.clone()));
        println!("after connect: state = {running:?}, error = {error:?}");
        assert_eq!(running, Some(ConnectionState::Running));

        engine.refresh(false).await;
        let stats = engine.shared.read(|st| st.stats.clone());
        println!("live stats: {stats:?}");
        assert!(stats.is_some(), "GET /v1/stats/live parsed while running");

        engine.handle(Command::Toggle).await;
        let stopped = engine.shared.read(|st| st.status.as_ref().map(|s| s.state));
        println!("after toggle: state = {stopped:?}");
        assert_eq!(stopped, Some(ConnectionState::Idle));

        handle.shutdown().await;
        state
            .wait_for(|s| *s == SidecarState::Stopped)
            .await
            .expect("state channel open");
    }
}
