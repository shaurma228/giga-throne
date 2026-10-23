//! Starts, watches, restarts and stops the headless Throne process.
//!
//! [`Sidecar::start`] adopts a Throne that already answers on our port, or spawns
//! one. A background task owns the child, publishes [`SidecarState`] and applies
//! the restart policy from [`super::policy`]. [`SidecarHandle`] is the cheap,
//! cloneable way to reach the API client, watch the state and stop the engine.

use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use throne_api::types::Status;
use throne_api::{Client, Method};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::watch;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};

use super::config::{load_or_create_key, SidecarConfig};
use super::policy::{backoff, on_exit, ExitDecision, FailureWindow, STABLE_AFTER};

/// Connect timeout of the one-off probe that looks for a running Throne.
const ADOPT_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Request timeout of the readiness and liveness probes.
const PROBE_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a started Throne may take to answer its status request.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// Interval between readiness probes.
const READY_POLL: Duration = Duration::from_millis(100);

/// How long Throne may take to exit after a shutdown request before it is killed.
const GRACE: Duration = Duration::from_secs(5);

/// How long [`SidecarHandle::shutdown`] waits for the supervisor to finish.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(6);

/// How long the shutdown request itself may take.
const SHUTDOWN_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Lifecycle of the engine as the rest of the app sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarState {
    /// Spawned or adopted, not answering the API yet.
    Starting,
    /// Answers the API. `version` is Throne's version string.
    Ready { version: String },
    /// Waiting before another start. `attempt` counts failed starts in the
    /// current window; it is 0 for a restart that Throne asked for.
    Restarting { attempt: u32, reason: String },
    /// Given up. The text is user-readable.
    Failed { reason: String },
    /// Stopped on request, or after an unexpected stop of an adopted instance.
    Stopped,
}

/// Entry point of the sidecar.
pub struct Sidecar;

impl Sidecar {
    /// Makes sure the API key exists, adopts a running Throne or spawns one,
    /// and starts supervising it in the background.
    ///
    /// Returns once the child is spawned or the adoption probe has finished.
    /// Readiness is reported through [`SidecarHandle::state`].
    pub async fn start(config: SidecarConfig) -> Result<SidecarHandle> {
        let key = load_or_create_key(&config.key_file())?;
        std::fs::create_dir_all(config.throne_data_dir())
            .with_context(|| format!("создание каталога {}", config.throne_data_dir().display()))?;

        let addr = config.listen_addr();
        let probe = Client::new(addr.clone(), key.clone())
            .connect_timeout(ADOPT_CONNECT_TIMEOUT)
            .request_timeout(PROBE_REQUEST_TIMEOUT);
        let adopted = is_our_throne(&probe).await;
        let first_child = if adopted {
            info!("adopting Throne already listening on {addr}");
            None
        } else {
            Some(spawn(&config)?)
        };

        let (state_tx, state_rx) = watch::channel(SidecarState::Starting);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (done_tx, done_rx) = watch::channel(false);
        let supervisor = Supervisor {
            config,
            probe,
            state: state_tx,
            stop: stop_rx,
            adopted,
        };
        tokio::spawn(supervisor.run(first_child, done_tx));

        Ok(SidecarHandle {
            inner: Arc::new(Inner {
                client: Arc::new(Client::new(addr, key)),
                state: state_rx,
                stop: stop_tx,
                done: done_rx,
            }),
        })
    }
}

/// Handle to the running engine. Cheap to clone; all clones share one engine.
#[derive(Clone)]
pub struct SidecarHandle {
    inner: Arc<Inner>,
}

struct Inner {
    client: Arc<Client>,
    state: watch::Receiver<SidecarState>,
    /// Setting this to `true` requests a stop. Dropping every handle also stops.
    stop: watch::Sender<bool>,
    /// Becomes `true` when the supervisor task has finished.
    done: watch::Receiver<bool>,
}

impl SidecarHandle {
    /// The shared API client, configured with our port and key.
    #[allow(dead_code)] // used once the app layer calls the API
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.inner.client)
    }

    /// Receiver of the engine state. Clone it to watch independently.
    pub fn state(&self) -> watch::Receiver<SidecarState> {
        self.inner.state.clone()
    }

    /// Asks Throne to exit, waits for it, and kills it after a grace period.
    ///
    /// Idempotent: a second call only waits for the same shutdown. Returns
    /// once the supervisor has finished, or after [`SHUTDOWN_WAIT`].
    pub async fn shutdown(&self) {
        let already_requested = self.inner.stop.send_replace(true);
        if !already_requested {
            let request = self
                .inner
                .client
                .request(Method::Post, "/v1/shutdown", None, None);
            match timeout(SHUTDOWN_REQUEST_TIMEOUT, request).await {
                Ok(Ok(_)) => info!("shutdown requested from Throne"),
                Ok(Err(err)) => warn!("shutdown request failed: {err}"),
                Err(_) => warn!("shutdown request timed out"),
            }
        }

        let mut done = self.inner.done.clone();
        let finished = timeout(SHUTDOWN_WAIT, async {
            while !*done.borrow_and_update() {
                if done.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        if finished.is_err() {
            warn!("engine did not stop within {SHUTDOWN_WAIT:?}");
        }
    }
}

/// Outcome of one run of a Throne instance.
enum InstanceEnd {
    /// A stop was requested and the instance is gone.
    Stopped,
    /// The child exited on its own.
    Exited {
        code: Option<i32>,
        /// How long it had been ready, `None` if it never became ready.
        ready_for: Option<Duration>,
    },
    /// The instance did not answer within [`READY_TIMEOUT`]. It has been stopped.
    NotReady,
}

enum Event {
    Exited(std::io::Result<ExitStatus>),
    Stop,
    Tick,
}

struct Supervisor {
    config: SidecarConfig,
    /// Short-timeout client for readiness probes.
    probe: Client,
    state: watch::Sender<SidecarState>,
    stop: watch::Receiver<bool>,
    /// True when the instance was adopted and is not our child.
    adopted: bool,
}

impl Supervisor {
    async fn run(mut self, first_child: Option<Child>, done: watch::Sender<bool>) {
        let final_state = self.supervise(first_child).await;
        self.state.send_replace(final_state);
        done.send_replace(true);
    }

    /// The restart loop. Returns the state to leave behind.
    async fn supervise(&mut self, mut pending: Option<Child>) -> SidecarState {
        let mut window = FailureWindow::default();
        loop {
            if *self.stop.borrow() {
                return SidecarState::Stopped;
            }
            self.state.send_replace(SidecarState::Starting);

            let end = if self.adopted {
                self.run_adopted().await
            } else {
                let child = match pending.take() {
                    Some(child) => child,
                    None => match spawn(&self.config) {
                        Ok(child) => child,
                        Err(err) => {
                            return SidecarState::Failed {
                                reason: format!("не удалось запустить Throne: {err:#}"),
                            }
                        }
                    },
                };
                self.run_child(child).await
            };

            let (reason, ready_for) = match end {
                InstanceEnd::Stopped => return SidecarState::Stopped,
                InstanceEnd::NotReady => ("движок не ответил за 15 секунд".to_owned(), None),
                InstanceEnd::Exited { code, ready_for } => {
                    if *self.stop.borrow() {
                        return SidecarState::Stopped;
                    }
                    match on_exit(code) {
                        ExitDecision::RestartNow => {
                            info!("Throne asked for a restart");
                            self.state.send_replace(SidecarState::Restarting {
                                attempt: window.count() as u32,
                                reason: "перезапуск по запросу движка".to_owned(),
                            });
                            continue;
                        }
                        ExitDecision::Fail(reason) => {
                            return SidecarState::Failed { reason };
                        }
                        ExitDecision::Backoff => (
                            format!("движок завершился: {}", describe_exit(code)),
                            ready_for,
                        ),
                    }
                }
            };

            if self.adopted {
                // Nothing to restart: we do not own the adopted process.
                return SidecarState::Failed { reason };
            }
            if ready_for.is_some_and(|ran| ran > STABLE_AFTER) {
                window.reset();
            }
            let failed = window.record(Instant::now());
            if window.exhausted() {
                return SidecarState::Failed {
                    reason: format!("{failed} неудачных запусков за 2 минуты: {reason}"),
                };
            }

            let delay = backoff(failed as u32 - 1);
            warn!("{reason}; restarting in {delay:?} (failed start {failed})");
            self.state.send_replace(SidecarState::Restarting {
                attempt: failed as u32,
                reason,
            });
            let stopped = tokio::select! {
                _ = wait_stop(&mut self.stop) => true,
                _ = sleep(delay) => false,
            };
            if stopped {
                return SidecarState::Stopped;
            }
        }
    }

    /// Watches a child we spawned until it exits, is stopped, or misses readiness.
    async fn run_child(&mut self, mut child: Child) -> InstanceEnd {
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut ready_at: Option<Instant> = None;
        loop {
            let tick = if ready_at.is_some() {
                Duration::from_secs(1)
            } else {
                READY_POLL
            };
            let event = tokio::select! {
                status = child.wait() => Event::Exited(status),
                _ = wait_stop(&mut self.stop) => Event::Stop,
                _ = sleep(tick) => Event::Tick,
            };
            match event {
                Event::Exited(status) => {
                    return InstanceEnd::Exited {
                        code: status.ok().and_then(|s| s.code()),
                        ready_for: ready_at.map(|at| at.elapsed()),
                    };
                }
                Event::Stop => {
                    terminate(&mut child).await;
                    return InstanceEnd::Stopped;
                }
                Event::Tick => {}
            }

            if ready_at.is_none() {
                if let Some(version) = self.probe_ready().await {
                    info!("Throne {version} is ready");
                    self.state.send_replace(SidecarState::Ready { version });
                    ready_at = Some(Instant::now());
                } else if Instant::now() >= deadline {
                    warn!("Throne did not answer within {READY_TIMEOUT:?}");
                    terminate(&mut child).await;
                    return InstanceEnd::NotReady;
                }
            }
        }
    }

    /// Waits for an adopted instance to answer, then for a stop, then for it to go away.
    async fn run_adopted(&mut self) -> InstanceEnd {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(version) = self.probe_ready().await {
                self.state.send_replace(SidecarState::Ready { version });
                break;
            }
            if Instant::now() >= deadline {
                return InstanceEnd::NotReady;
            }
            let stopped = tokio::select! {
                _ = wait_stop(&mut self.stop) => true,
                _ = sleep(READY_POLL) => false,
            };
            if stopped {
                return InstanceEnd::Stopped;
            }
        }

        wait_stop(&mut self.stop).await;
        // The shutdown request was sent by the handle; wait until the port closes.
        let gone = timeout(GRACE, async {
            while self.probe.get("/v1/status").await.is_ok() {
                sleep(READY_POLL).await;
            }
        })
        .await;
        if gone.is_err() {
            warn!("adopted Throne is still answering after {GRACE:?}");
        }
        InstanceEnd::Stopped
    }

    /// Status of the instance if it is ready, `None` while it is not.
    async fn probe_ready(&self) -> Option<String> {
        let status: Status = self
            .probe
            .request_json(Method::Get, "/v1/status", None, None)
            .await
            .ok()?;
        Some(status.version)
    }
}

/// Starts Throne with the flags of the sidecar contract.
fn spawn(config: &SidecarConfig) -> Result<Child> {
    let mut command = Command::new(&config.binary);
    command
        .arg("--headless")
        .arg("--data-dir")
        .arg(config.throne_data_dir())
        .arg("--api-listen")
        .arg(config.listen_addr())
        .arg("--api-key-file")
        .arg(config.key_file())
        .args(["--restart-mode", "exit", "--log-format", "plain"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("запуск {}", config.binary.display()))?;
    if let Some(stderr) = child.stderr.take() {
        forward_stderr(stderr);
    }
    info!("started Throne {}", config.binary.display());
    Ok(child)
}

/// Copies Throne's stderr into `tracing`. Lines marked `[ERR]` are warnings.
fn forward_stderr(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("[ERR]") {
                warn!(target: "throne", "{line}");
            } else {
                info!(target: "throne", "{line}");
            }
        }
    });
}

/// Asks nothing and waits for the child to exit on its own, then kills it after [`GRACE`].
async fn terminate(child: &mut Child) {
    if timeout(GRACE, child.wait()).await.is_err() {
        warn!("Throne did not exit within {GRACE:?}; killing it");
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

/// True when a Throne answers on our port with our key, ready or still starting.
async fn is_our_throne(probe: &Client) -> bool {
    match probe.get("/v1/status").await {
        Ok(_) => true,
        // 503 `starting`: the key matched, so this is our Throne.
        Err(throne_api::Error::Api { status: 503, .. }) => true,
        Err(_) => false,
    }
}

/// Completes when a stop has been requested or every handle is gone.
async fn wait_stop(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow_and_update() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

fn describe_exit(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("код {code}"),
        None => "завершён сигналом".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;
    use std::path::{Path, PathBuf};

    use super::*;

    /// Picks a port that is free right now.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("local addr").port()
    }

    /// Full round trip with the real Throne from `sidecar/`.
    #[tokio::test]
    #[ignore = "needs the real Throne binary in sidecar/Throne"]
    async fn real_throne_roundtrip() {
        let binary = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sidecar/Throne/Throne"
        ));
        assert!(Path::new(&binary).is_file(), "missing {}", binary.display());
        let dir = tempfile::tempdir().expect("temp dir");
        let port = free_port();
        let config = SidecarConfig {
            binary,
            data_root: dir.path().to_path_buf(),
            port,
        };

        let handle = Sidecar::start(config).await.expect("start sidecar");
        let mut state = handle.state();
        let ready = timeout(
            Duration::from_secs(20),
            state.wait_for(|s| !matches!(s, SidecarState::Starting)),
        )
        .await
        .expect("Throne became ready within 20 s")
        .expect("state channel open")
        .clone();
        let version = match ready {
            SidecarState::Ready { version } => version,
            other => panic!("expected Ready, got {other:?}"),
        };
        println!("Throne version: {version}");

        let status: Status = handle
            .client()
            .request_json(Method::Get, "/v1/status", None, None)
            .await
            .expect("GET /v1/status parses as Status");
        assert_eq!(status.state, throne_api::types::ConnectionState::Idle);
        println!("status.state = {:?}", status.state);

        handle.shutdown().await;
        handle.shutdown().await; // idempotent
        let stopped = state
            .wait_for(|s| *s == SidecarState::Stopped)
            .await
            .expect("state channel open")
            .clone();
        assert_eq!(stopped, SidecarState::Stopped);
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "Throne still listens after shutdown"
        );
    }
}
