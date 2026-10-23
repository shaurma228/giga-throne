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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use throne_api::types::{
    ConnectionState, GetSettingsResponse, ImportResponse, LiveStatsResponse, Profile,
    ProfilesResponse, SettingsMap, Status,
};
use throne_api::{Client, Error, Method};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{watch, Notify};
use tokio::time::{interval_at, sleep};
use tracing::{debug, info, warn};

use crate::sidecar::SidecarState;
use crate::view::{home_view, server_row, Frame, HomeView, Pending, ServerRow};
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
}

impl Engine {
    pub fn new(client: Arc<Client>, api_port: u16, sink: Sink) -> Arc<Self> {
        Arc::new(Self {
            client,
            api_port,
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                sink,
            }),
            wake: Notify::new(),
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
                        // Left Ready or came back: re-check the ports and restart the loop.
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
        }
        self.wake.notify_one();
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
            flash_error(&engine.shared, "Движок ещё не готов".to_owned());
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
    });
    let shared = Arc::clone(shared);
    tokio::spawn(async move {
        sleep(ERROR_SHOWN_FOR).await;
        shared.update(|st| match target {
            Flash::Home if st.error_seq == seq => st.error.clear(),
            Flash::Import if st.import_seq == seq => st.import_result.clear(),
            _ => {}
        });
    });
}

/// Engine state the UI shows. Guarded by a mutex; never held across an await.
#[derive(Default)]
struct State {
    status: Option<Status>,
    stats: Option<LiveStatsResponse>,
    profiles: Vec<Profile>,
    pending: Option<Pending>,
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
        let clear_import = std::mem::take(&mut self.clear_import);
        Frame {
            home,
            servers,
            import_result: self.import_result.clone(),
            error: self.error.clone(),
            clear_import,
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

        let engine = Engine::new(handle.client(), port, Box::new(|_| {}));

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
