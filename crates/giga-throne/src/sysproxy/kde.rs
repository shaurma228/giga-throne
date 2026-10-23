//! KDE Plasma backend: the proxy settings in `kioslaverc`, written with the
//! `kwriteconfig6` tools (or the `5` ones on Plasma 5).

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::runner::Runner;

const FILE: &str = "kioslaverc";
const GROUP: &str = "Proxy Settings";
const NO_PROXY_FOR: &str = "localhost,127.0.0.0/8,::1";

/// Proxy settings of KDE as they were before we changed them.
///
/// `major` is the Plasma major version whose tools were used, so that the
/// restore goes through the same tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub major: u8,
    pub proxy_type: String,
    pub http_proxy: String,
    pub https_proxy: String,
    pub socks_proxy: String,
    pub no_proxy_for: String,
}

/// The newest Plasma tools present: `Some(6)`, else `Some(5)`, else `None`.
pub fn detect(runner: &dyn Runner) -> Option<u8> {
    [6, 5].into_iter().find(|&major| {
        runner
            .run(&format!("kreadconfig{major}"), &["--help"])
            .is_ok_and(|out| out.status_ok)
    })
}

/// Reads the five keys that [`apply`] changes.
pub fn snapshot(runner: &dyn Runner, major: u8) -> Result<Snapshot> {
    Ok(Snapshot {
        major,
        proxy_type: read(runner, major, "ProxyType")?,
        http_proxy: read(runner, major, "httpProxy")?,
        https_proxy: read(runner, major, "httpsProxy")?,
        socks_proxy: read(runner, major, "socksProxy")?,
        no_proxy_for: read(runner, major, "NoProxyFor")?,
    })
}

/// Switches KDE to manual proxy at `host:port` and tells running apps about it.
pub fn apply(runner: &dyn Runner, major: u8, host: &str, port: u16) -> Result<()> {
    let endpoint = format!("{host} {port}");
    write(runner, major, "ProxyType", "1")?;
    write(runner, major, "httpProxy", &format!("http://{endpoint}"))?;
    write(runner, major, "httpsProxy", &format!("http://{endpoint}"))?;
    write(runner, major, "socksProxy", &format!("socks://{endpoint}"))?;
    write(runner, major, "NoProxyFor", NO_PROXY_FOR)?;
    notify(runner);
    Ok(())
}

/// Writes the values of `snapshot` back.
pub fn restore(runner: &dyn Runner, snapshot: &Snapshot) -> Result<()> {
    let major = snapshot.major;
    write(runner, major, "ProxyType", &snapshot.proxy_type)?;
    write(runner, major, "httpProxy", &snapshot.http_proxy)?;
    write(runner, major, "httpsProxy", &snapshot.https_proxy)?;
    write(runner, major, "socksProxy", &snapshot.socks_proxy)?;
    write(runner, major, "NoProxyFor", &snapshot.no_proxy_for)?;
    notify(runner);
    Ok(())
}

fn read(runner: &dyn Runner, major: u8, key: &str) -> Result<String> {
    let program = format!("kreadconfig{major}");
    let out = runner
        .run(&program, &["--file", FILE, "--group", GROUP, "--key", key])
        .with_context(|| format!("running {program} for {key}"))?;
    ensure!(out.status_ok, "{program} failed for {key}");
    Ok(out.stdout.trim_end_matches('\n').to_owned())
}

fn write(runner: &dyn Runner, major: u8, key: &str, value: &str) -> Result<()> {
    let program = format!("kwriteconfig{major}");
    let out = runner
        .run(
            &program,
            &["--file", FILE, "--group", GROUP, "--key", key, value],
        )
        .with_context(|| format!("running {program} for {key}"))?;
    ensure!(out.status_ok, "{program} failed to write {key}");
    Ok(())
}

/// Asks running KDE apps to reread their proxy configuration. Failure is ignored:
/// the settings are saved either way, and apps pick them up on their own later.
fn notify(runner: &dyn Runner) {
    let args = [
        "--type=signal",
        "/KIO/Scheduler",
        "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
        "string:",
    ];
    if let Err(err) = runner.run("dbus-send", &args) {
        debug!("could not notify KDE apps about the proxy: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysproxy::runner::FakeRunner;

    const READ6: &str = "kreadconfig6 --file kioslaverc --group Proxy Settings --key";

    fn sample() -> Snapshot {
        Snapshot {
            major: 6,
            proxy_type: "0".to_owned(),
            http_proxy: "http://127.0.0.1 2080".to_owned(),
            https_proxy: "http://127.0.0.1 2080".to_owned(),
            socks_proxy: "socks://127.0.0.1 2080".to_owned(),
            no_proxy_for: String::new(),
        }
    }

    #[test]
    fn detect_prefers_plasma_six_and_falls_back_to_five() {
        let fake = FakeRunner::new();
        assert_eq!(detect(&fake), Some(6));
        assert_eq!(fake.call_lines(), ["kreadconfig6 --help"]);

        let fake = FakeRunner::new();
        fake.missing("kreadconfig6 --help");
        assert_eq!(detect(&fake), Some(5));

        let fake = FakeRunner::new();
        fake.missing("kreadconfig6 --help")
            .failing("kreadconfig5 --help");
        assert_eq!(detect(&fake), None);
    }

    #[test]
    fn snapshot_reads_the_five_keys_in_order() {
        let fake = FakeRunner::new();
        fake.answer(&format!("{READ6} ProxyType"), "1\n")
            .answer(&format!("{READ6} httpProxy"), "http://h 1\n")
            .answer(&format!("{READ6} httpsProxy"), "http://h 1\n")
            .answer(&format!("{READ6} socksProxy"), "socks://h 1\n")
            .answer(&format!("{READ6} NoProxyFor"), "a,b\n");
        let snap = snapshot(&fake, 6).expect("snapshot");
        assert_eq!(snap.proxy_type, "1");
        assert_eq!(snap.http_proxy, "http://h 1");
        assert_eq!(snap.no_proxy_for, "a,b");
        assert_eq!(
            fake.call_lines(),
            [
                format!("{READ6} ProxyType"),
                format!("{READ6} httpProxy"),
                format!("{READ6} httpsProxy"),
                format!("{READ6} socksProxy"),
                format!("{READ6} NoProxyFor"),
            ]
        );
    }

    #[test]
    fn snapshot_fails_when_a_read_fails() {
        let fake = FakeRunner::new();
        fake.failing(&format!("{READ6} httpProxy"));
        assert!(snapshot(&fake, 6).is_err());
    }

    #[test]
    fn apply_writes_manual_proxy_and_notifies() {
        let fake = FakeRunner::new();
        apply(&fake, 6, "127.0.0.1", 65000).expect("apply");
        assert_eq!(
            fake.call_lines(),
            [
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 1",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://127.0.0.1 65000",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpsProxy http://127.0.0.1 65000",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key socksProxy socks://127.0.0.1 65000",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key NoProxyFor localhost,127.0.0.0/8,::1",
                "dbus-send --type=signal /KIO/Scheduler org.kde.KIO.Scheduler.reparseSlaveConfiguration string:",
            ]
        );
    }

    #[test]
    fn apply_on_plasma_five_uses_the_five_tools() {
        let fake = FakeRunner::new();
        apply(&fake, 5, "127.0.0.1", 1080).expect("apply");
        assert!(fake
            .call_lines()
            .iter()
            .all(|line| !line.contains("kwriteconfig6")));
        assert!(fake.call_lines().contains(
            &"kwriteconfig5 --file kioslaverc --group Proxy Settings --key ProxyType 1".to_owned()
        ));
    }

    #[test]
    fn restore_writes_the_snapshot_back_with_its_tools() {
        let fake = FakeRunner::new();
        restore(&fake, &sample()).expect("restore");
        assert_eq!(
            fake.call_lines(),
            [
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 0",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://127.0.0.1 2080",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpsProxy http://127.0.0.1 2080",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key socksProxy socks://127.0.0.1 2080",
                "kwriteconfig6 --file kioslaverc --group Proxy Settings --key NoProxyFor ",
                "dbus-send --type=signal /KIO/Scheduler org.kde.KIO.Scheduler.reparseSlaveConfiguration string:",
            ]
        );
    }

    #[test]
    fn failed_write_is_an_error() {
        let fake = FakeRunner::new();
        fake.failing("kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 1");
        assert!(apply(&fake, 6, "127.0.0.1", 1080).is_err());
    }

    #[test]
    fn failed_notify_is_ignored() {
        const DBUS: &str = "dbus-send --type=signal /KIO/Scheduler org.kde.KIO.Scheduler.reparseSlaveConfiguration string:";
        let missing = FakeRunner::new();
        missing.missing(DBUS);
        apply(&missing, 6, "127.0.0.1", 1080).expect("apply succeeds without dbus-send");

        let failing = FakeRunner::new();
        failing.failing(DBUS);
        restore(&failing, &sample()).expect("restore succeeds when dbus-send fails");
    }
}
