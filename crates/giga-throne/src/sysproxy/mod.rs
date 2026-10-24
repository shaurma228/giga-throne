//! System proxy: while Throne runs, points the desktop's proxy settings at
//! Throne's mixed inbound, so that apps which use the system proxy go through the
//! VPN. Headless Throne never does this itself (`PUT /v1/modes` answers
//! `system_proxy_unsupported`).
//!
//! Only Linux is supported: KDE (`kioslaverc`, via `kwriteconfig6` or `5`) and
//! GNOME/GTK (`gsettings`). Both are changed when both are present.
//!
//! The values we replace are saved to `<data_root>/sysproxy-backup.json` before
//! anything is written, and the file is removed once they are back. If the app
//! died in between, [`SystemProxy::recover`] restores them at the next start.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod gnome;
mod kde;
pub(crate) mod runner;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub use runner::{ProcessRunner, Runner};

/// Name of the backup file in our data root.
const BACKUP_FILE: &str = "sysproxy-backup.json";

/// Host used when the inbound listens on every address.
const LOOPBACK: &str = "127.0.0.1";

/// The settings as they were before we changed them. A backend is `Some` when
/// it was present and has been changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Saved {
    pub kde: Option<kde::Snapshot>,
    pub gnome: Option<gnome::Snapshot>,
}

impl Saved {
    fn is_empty(&self) -> bool {
        self.kde.is_none() && self.gnome.is_none()
    }
}

/// The address a client should connect to for an inbound that listens on `address`.
pub fn proxy_host(address: &str) -> String {
    match address.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => LOOPBACK.to_owned(),
        other => other.to_owned(),
    }
}

/// Where the backup of the settings lives.
pub fn backup_path(data_root: &Path) -> PathBuf {
    data_root.join(BACKUP_FILE)
}

/// What is applied right now.
struct Active {
    saved: Saved,
    host: String,
    port: u16,
}

/// Sets and restores the desktop proxy. Holds the snapshot while it is enabled.
pub struct SystemProxy {
    runner: Arc<dyn Runner>,
    backup: PathBuf,
    active: Mutex<Option<Active>>,
}

impl SystemProxy {
    /// A proxy controller that runs the real tools.
    pub fn new(data_root: &Path) -> Self {
        Self::with_runner(data_root, Arc::new(ProcessRunner))
    }

    /// A proxy controller that runs its tools through `runner`.
    pub fn with_runner(data_root: &Path, runner: Arc<dyn Runner>) -> Self {
        Self {
            runner,
            backup: backup_path(data_root),
            active: Mutex::new(None),
        }
    }

    /// Whether we have changed the desktop proxy and not restored it yet.
    #[allow(dead_code)] // Part of the façade; the engine works from `applied`.
    pub fn is_enabled(&self) -> bool {
        self.lock().is_some()
    }

    /// Host and port that are applied now, if any.
    pub fn applied(&self) -> Option<(String, u16)> {
        self.lock()
            .as_ref()
            .map(|active| (active.host.clone(), active.port))
    }

    /// Points the desktop proxy at `host:port`.
    ///
    /// The first call takes a snapshot and saves it. Later calls change only the
    /// endpoint, and only when it differs. An unspecified host (`0.0.0.0`, `::`)
    /// is replaced by `127.0.0.1`.
    #[cfg(target_os = "linux")]
    pub fn enable(&self, host: &str, port: u16) -> Result<()> {
        let host = proxy_host(host);
        let mut active = self.lock();
        if let Some(current) = active.as_mut() {
            if current.host != host || current.port != port {
                apply(self.runner.as_ref(), &current.saved, &host, port)?;
                current.host = host;
                current.port = port;
            }
            return Ok(());
        }

        let saved = snapshot(self.runner.as_ref())?;
        if saved.is_empty() {
            bail!("не найден ни kwriteconfig6 (KDE), ни gsettings (GNOME)");
        }
        write_backup(&self.backup, &saved)?;
        if let Err(err) = apply(self.runner.as_ref(), &saved, &host, port) {
            // Put back whatever was changed. If that fails too, the backup stays
            // and the next start restores it.
            if restore(self.runner.as_ref(), &saved).is_ok() {
                remove_backup(&self.backup)?;
            }
            return Err(err);
        }
        *active = Some(Active { saved, host, port });
        Ok(())
    }

    /// Only Linux has a system proxy to set.
    #[cfg(not(target_os = "linux"))]
    pub fn enable(&self, _host: &str, _port: u16) -> Result<()> {
        bail!("системный прокси поддерживается только в Linux")
    }

    /// Restores the snapshot taken by [`SystemProxy::enable`]. Does nothing when
    /// nothing is enabled. If restoring fails, the snapshot is kept so that a
    /// later call can try again.
    pub fn disable(&self) -> Result<()> {
        let mut active = self.lock();
        let Some(current) = active.as_ref() else {
            return Ok(());
        };
        restore(self.runner.as_ref(), &current.saved)?;
        *active = None;
        remove_backup(&self.backup)
    }

    /// Restores the settings from a backup left by a previous run that ended
    /// before [`SystemProxy::disable`]. Returns whether there was one.
    ///
    /// A backup that cannot be read is an error and is kept.
    pub fn recover(&self) -> Result<bool> {
        let text = match fs::read_to_string(&self.backup) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", self.backup.display()))
            }
        };
        let saved: Saved = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", self.backup.display()))?;
        restore(self.runner.as_ref(), &saved)?;
        remove_backup(&self.backup)?;
        Ok(true)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Active>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Snapshots every backend that is present.
#[cfg(target_os = "linux")]
fn snapshot(runner: &dyn Runner) -> Result<Saved> {
    let kde = match kde::detect(runner) {
        Some(major) => Some(kde::snapshot(runner, major)?),
        None => None,
    };
    let gnome = if gnome::detect(runner) {
        Some(gnome::snapshot(runner)?)
    } else {
        None
    };
    Ok(Saved { kde, gnome })
}

/// Points every backend in `saved` at `host:port`.
fn apply(runner: &dyn Runner, saved: &Saved, host: &str, port: u16) -> Result<()> {
    if let Some(kde) = &saved.kde {
        kde::apply(runner, kde.major, host, port)?;
    }
    if saved.gnome.is_some() {
        gnome::apply(runner, host, port)?;
    }
    Ok(())
}

/// Writes the snapshot back to every backend in `saved`.
fn restore(runner: &dyn Runner, saved: &Saved) -> Result<()> {
    if let Some(kde) = &saved.kde {
        kde::restore(runner, kde)?;
    }
    if let Some(gnome) = &saved.gnome {
        gnome::restore(runner, gnome)?;
    }
    Ok(())
}

fn write_backup(path: &Path, saved: &Saved) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(saved).context("encoding the proxy backup")?;
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn remove_backup(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::sysproxy::runner::FakeRunner;
    use std::sync::atomic::{AtomicBool, Ordering};

    const READ6: &str = "kreadconfig6 --file kioslaverc --group Proxy Settings --key";
    const WRITE_HTTP: &str =
        "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy";

    /// KDE on Plasma 6 with `ProxyType` 0 and a known http proxy, GNOME with "none".
    fn desktop() -> Arc<FakeRunner> {
        let fake = FakeRunner::new();
        fake.answer(&format!("{READ6} ProxyType"), "0\n")
            .answer(&format!("{READ6} httpProxy"), "http://old 3128\n")
            .answer(&format!("{READ6} httpsProxy"), "\n")
            .answer(&format!("{READ6} socksProxy"), "\n")
            .answer(&format!("{READ6} NoProxyFor"), "\n")
            .answer("gsettings get org.gnome.system.proxy mode", "'none'\n")
            .answer("gsettings get org.gnome.system.proxy ignore-hosts", "[]\n");
        Arc::new(fake)
    }

    fn controller(dir: &Path, fake: &Arc<FakeRunner>) -> SystemProxy {
        SystemProxy::with_runner(dir, Arc::clone(fake) as Arc<dyn Runner>)
    }

    /// Checks that the backup exists before the first write.
    struct Spy {
        inner: Arc<FakeRunner>,
        backup: PathBuf,
        write_without_backup: AtomicBool,
    }

    impl Runner for Spy {
        fn run(&self, program: &str, args: &[&str]) -> std::io::Result<runner::Output> {
            let is_write = program.starts_with("kwriteconfig")
                || (program == "gsettings" && args.first() == Some(&"set"));
            if is_write && !self.backup.exists() {
                self.write_without_backup.store(true, Ordering::SeqCst);
            }
            self.inner.run(program, args)
        }
    }

    #[test]
    fn enable_saves_the_backup_before_the_first_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spy = Arc::new(Spy {
            inner: desktop(),
            backup: backup_path(dir.path()),
            write_without_backup: AtomicBool::new(false),
        });
        let proxy = SystemProxy::with_runner(dir.path(), Arc::clone(&spy) as Arc<dyn Runner>);
        proxy.enable("127.0.0.1", 65000).expect("enable");
        assert!(!spy.write_without_backup.load(Ordering::SeqCst));
        assert!(backup_path(dir.path()).is_file());
    }

    #[test]
    fn enable_then_disable_restores_the_snapshot_and_removes_the_backup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);

        proxy.enable("127.0.0.1", 65000).expect("enable");
        assert!(proxy.is_enabled());
        assert_eq!(proxy.applied(), Some(("127.0.0.1".to_owned(), 65000)));
        assert!(backup_path(dir.path()).is_file());

        let before = fake.calls().len();
        proxy.disable().expect("disable");
        assert!(!proxy.is_enabled());
        assert!(!backup_path(dir.path()).exists());
        let restored = fake.call_lines()[before..].to_vec();
        assert_eq!(
            restored[0],
            "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 0"
        );
        assert_eq!(
            restored[1],
            "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://old 3128"
        );
        assert!(restored.contains(&"gsettings set org.gnome.system.proxy mode 'none'".to_owned()));
    }

    #[test]
    fn disable_without_enable_does_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        proxy.disable().expect("disable");
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn enable_twice_takes_one_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        proxy.enable("127.0.0.1", 65000).expect("first");
        let reads = fake
            .call_lines()
            .iter()
            .filter(|c| c.contains("kreadconfig"))
            .count();
        proxy.enable("127.0.0.1", 65000).expect("second");
        let again = fake
            .call_lines()
            .iter()
            .filter(|c| c.contains("kreadconfig"))
            .count();
        assert_eq!(reads, again);
        let writes = fake
            .call_lines()
            .iter()
            .filter(|c| c.starts_with("kwriteconfig6"))
            .count();
        assert_eq!(writes, 5);
    }

    #[test]
    fn a_new_port_is_applied_without_a_new_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        proxy.enable("127.0.0.1", 65000).expect("first");
        let before = fake.calls().len();
        proxy.enable("127.0.0.1", 65001).expect("new port");
        assert_eq!(proxy.applied(), Some(("127.0.0.1".to_owned(), 65001)));
        let later = fake.call_lines()[before..].to_vec();
        assert!(later.iter().all(|line| !line.contains("kreadconfig")));
        assert!(later.contains(
            &"kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://127.0.0.1 65001"
                .to_owned()
        ));
    }

    #[test]
    fn unspecified_host_is_replaced_by_loopback() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        proxy.enable("0.0.0.0", 1080).expect("enable");
        assert_eq!(proxy.applied(), Some(("127.0.0.1".to_owned(), 1080)));
        assert!(fake
            .call_lines()
            .contains(&"gsettings set org.gnome.system.proxy.http host '127.0.0.1'".to_owned()));
    }

    #[test]
    fn crash_recovery_restores_from_an_existing_backup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let saved = Saved {
            kde: Some(kde::Snapshot {
                major: 5,
                proxy_type: "2".to_owned(),
                http_proxy: "http://pac".to_owned(),
                https_proxy: String::new(),
                socks_proxy: String::new(),
                no_proxy_for: String::new(),
            }),
            gnome: None,
        };
        write_backup(&backup_path(dir.path()), &saved).expect("write backup");

        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        assert!(proxy.recover().expect("recover"));
        assert!(!backup_path(dir.path()).exists());
        assert!(fake.call_lines().contains(
            &"kwriteconfig5 --file kioslaverc --group Proxy Settings --key ProxyType 2".to_owned()
        ));
        assert!(!proxy.is_enabled());
    }

    #[test]
    fn recover_without_a_backup_does_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        assert!(!controller(dir.path(), &fake).recover().expect("recover"));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_corrupt_backup_is_an_error_and_is_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(backup_path(dir.path()), "{not json").expect("write");
        let fake = desktop();
        assert!(controller(dir.path(), &fake).recover().is_err());
        assert!(backup_path(dir.path()).exists());
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn no_desktop_tools_means_an_error_and_no_backup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = FakeRunner::new();
        fake.missing("kreadconfig6 --help")
            .missing("kreadconfig5 --help")
            .missing("gsettings --version");
        let fake = Arc::new(fake);
        let proxy = controller(dir.path(), &fake);
        assert!(proxy.enable("127.0.0.1", 1080).is_err());
        assert!(!proxy.is_enabled());
        assert!(!backup_path(dir.path()).exists());
    }

    #[test]
    fn a_failed_apply_puts_the_settings_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        fake.failing(&format!("{WRITE_HTTP} http://127.0.0.1 65000"));
        let proxy = controller(dir.path(), &fake);
        assert!(proxy.enable("127.0.0.1", 65000).is_err());
        assert!(!proxy.is_enabled());
        assert!(!backup_path(dir.path()).exists());
        assert!(fake.call_lines().contains(
            &"kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://old 3128"
                .to_owned()
        ));
    }

    #[test]
    fn failed_restore_keeps_the_backup_for_the_next_try() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fake = desktop();
        let proxy = controller(dir.path(), &fake);
        proxy.enable("127.0.0.1", 65000).expect("enable");
        fake.failing("kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 0");
        assert!(proxy.disable().is_err());
        assert!(proxy.is_enabled());
        assert!(backup_path(dir.path()).exists());
    }

    /// Changes the real KDE and GNOME proxy settings, so it is ignored by default.
    /// It also returns early unless the env var `GIGA_THRONE_TEST_DESKTOP=1` is set:
    /// the other real-Throne tests run in parallel with `-- --ignored` and would
    /// fetch through the bogus proxy while this one is enabled.
    /// Run with `GIGA_THRONE_TEST_DESKTOP=1 cargo test -p giga-throne real_kde_roundtrip -- --ignored --nocapture`.
    #[test]
    #[ignore = "changes the real desktop proxy settings"]
    fn real_kde_roundtrip() {
        if std::env::var("GIGA_THRONE_TEST_DESKTOP").as_deref() != Ok("1") {
            eprintln!(
                "skipped: real_kde_roundtrip changes the desktop proxy; \
                 run it with GIGA_THRONE_TEST_DESKTOP=1 cargo test -p giga-throne real_kde_roundtrip -- --ignored --nocapture"
            );
            return;
        }
        let runner = ProcessRunner;
        let dir = tempfile::tempdir().expect("temp dir");
        let proxy = SystemProxy::new(dir.path());
        let major = kde::detect(&runner).expect("kreadconfig6 or kreadconfig5 is installed");
        let before_kde = kde::snapshot(&runner, major).expect("read KDE proxy");
        let before_gnome = gnome::snapshot(&runner).expect("read GNOME proxy");
        println!("before: {before_kde:?}\n        {before_gnome:?}");

        let enabled = proxy.enable("127.0.0.1", 65000);
        let during_kde = kde::snapshot(&runner, major);
        let during_gnome = gnome::snapshot(&runner);
        // Restore even when the checks below fail.
        let restored = proxy.disable();
        let after_kde = kde::snapshot(&runner, major).expect("read KDE proxy");
        let after_gnome = gnome::snapshot(&runner).expect("read GNOME proxy");
        println!("during: {during_kde:?}\n        {during_gnome:?}");
        println!("after:  {after_kde:?}\n        {after_gnome:?}");

        enabled.expect("enable");
        restored.expect("disable");
        let during_kde = during_kde.expect("read KDE proxy while enabled");
        assert_eq!(during_kde.proxy_type, "1");
        assert_eq!(during_kde.http_proxy, "http://127.0.0.1 65000");
        assert_eq!(during_gnome.expect("read GNOME proxy").mode, "'manual'");
        assert_eq!(after_kde, before_kde);
        assert_eq!(after_gnome, before_gnome);
        assert!(!backup_path(dir.path()).exists());
    }
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    #[test]
    fn unspecified_addresses_map_to_loopback() {
        assert_eq!(proxy_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(proxy_host("::"), "127.0.0.1");
        assert_eq!(proxy_host(" 0.0.0.0 "), "127.0.0.1");
        assert_eq!(proxy_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(proxy_host("192.168.1.5"), "192.168.1.5");
    }

    #[test]
    fn backup_lives_in_the_data_root() {
        assert_eq!(
            backup_path(Path::new("/data/giga")),
            Path::new("/data/giga/sysproxy-backup.json")
        );
    }

    #[test]
    fn saved_round_trips_through_json() {
        let saved = Saved {
            kde: None,
            gnome: Some(gnome::Snapshot {
                mode: "'none'".to_owned(),
                http_host: "''".to_owned(),
                http_port: "0".to_owned(),
                https_host: "''".to_owned(),
                https_port: "0".to_owned(),
                socks_host: "''".to_owned(),
                socks_port: "0".to_owned(),
                ignore_hosts: "[]".to_owned(),
            }),
        };
        let text = serde_json::to_string(&saved).expect("encode");
        let back: Saved = serde_json::from_str(&text).expect("decode");
        assert_eq!(back, saved);
    }
}
