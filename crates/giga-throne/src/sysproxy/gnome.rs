//! GNOME and GTK backend: the `org.gnome.system.proxy` schemas, written with
//! `gsettings`. Values are kept in gsettings' own text form, so a restore writes
//! back exactly what was read.

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use super::runner::Runner;

const PROXY: &str = "org.gnome.system.proxy";
const HTTP: &str = "org.gnome.system.proxy.http";
const HTTPS: &str = "org.gnome.system.proxy.https";
const SOCKS: &str = "org.gnome.system.proxy.socks";
const IGNORE_HOSTS: &str = "['localhost', '127.0.0.0/8', '::1']";

/// Proxy settings of GNOME as they were before we changed them. Each field is
/// the raw output of `gsettings get`, such as `'none'` or `2080`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub mode: String,
    pub http_host: String,
    pub http_port: String,
    pub https_host: String,
    pub https_port: String,
    pub socks_host: String,
    pub socks_port: String,
    pub ignore_hosts: String,
}

/// Whether `gsettings` is installed.
pub fn detect(runner: &dyn Runner) -> bool {
    runner
        .run("gsettings", &["--version"])
        .is_ok_and(|out| out.status_ok)
}

/// Reads every key that [`apply`] changes.
pub fn snapshot(runner: &dyn Runner) -> Result<Snapshot> {
    Ok(Snapshot {
        mode: get(runner, PROXY, "mode")?,
        http_host: get(runner, HTTP, "host")?,
        http_port: get(runner, HTTP, "port")?,
        https_host: get(runner, HTTPS, "host")?,
        https_port: get(runner, HTTPS, "port")?,
        socks_host: get(runner, SOCKS, "host")?,
        socks_port: get(runner, SOCKS, "port")?,
        ignore_hosts: get(runner, PROXY, "ignore-hosts")?,
    })
}

/// Switches GNOME to manual proxy at `host:port` for HTTP, HTTPS and SOCKS.
pub fn apply(runner: &dyn Runner, host: &str, port: u16) -> Result<()> {
    let port = port.to_string();
    let host = format!("'{host}'");
    set(runner, PROXY, "mode", "'manual'")?;
    set(runner, HTTP, "host", &host)?;
    set(runner, HTTP, "port", &port)?;
    set(runner, HTTPS, "host", &host)?;
    set(runner, HTTPS, "port", &port)?;
    set(runner, SOCKS, "host", &host)?;
    set(runner, SOCKS, "port", &port)?;
    set(runner, PROXY, "ignore-hosts", IGNORE_HOSTS)
}

/// Writes the values of `snapshot` back.
pub fn restore(runner: &dyn Runner, snapshot: &Snapshot) -> Result<()> {
    set(runner, PROXY, "mode", &snapshot.mode)?;
    set(runner, HTTP, "host", &snapshot.http_host)?;
    set(runner, HTTP, "port", &snapshot.http_port)?;
    set(runner, HTTPS, "host", &snapshot.https_host)?;
    set(runner, HTTPS, "port", &snapshot.https_port)?;
    set(runner, SOCKS, "host", &snapshot.socks_host)?;
    set(runner, SOCKS, "port", &snapshot.socks_port)?;
    set(runner, PROXY, "ignore-hosts", &snapshot.ignore_hosts)
}

fn get(runner: &dyn Runner, schema: &str, key: &str) -> Result<String> {
    let out = runner
        .run("gsettings", &["get", schema, key])
        .with_context(|| format!("running gsettings get {schema} {key}"))?;
    ensure!(out.status_ok, "gsettings get {schema} {key} failed");
    Ok(out.stdout.trim_end_matches('\n').to_owned())
}

fn set(runner: &dyn Runner, schema: &str, key: &str, value: &str) -> Result<()> {
    let out = runner
        .run("gsettings", &["set", schema, key, value])
        .with_context(|| format!("running gsettings set {schema} {key}"))?;
    ensure!(out.status_ok, "gsettings set {schema} {key} failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysproxy::runner::FakeRunner;

    fn sample() -> Snapshot {
        Snapshot {
            mode: "'none'".to_owned(),
            http_host: "'127.0.0.1'".to_owned(),
            http_port: "2080".to_owned(),
            https_host: "'127.0.0.1'".to_owned(),
            https_port: "2080".to_owned(),
            socks_host: "'127.0.0.1'".to_owned(),
            socks_port: "2080".to_owned(),
            ignore_hosts: "['localhost', '127.0.0.0/8', '::1']".to_owned(),
        }
    }

    #[test]
    fn snapshot_reads_gvariant_text_as_is() {
        let fake = FakeRunner::new();
        fake.answer("gsettings get org.gnome.system.proxy mode", "'none'\n")
            .answer("gsettings get org.gnome.system.proxy.http port", "2080\n");
        let snap = snapshot(&fake).expect("snapshot");
        assert_eq!(snap.mode, "'none'");
        assert_eq!(snap.http_port, "2080");
        assert_eq!(fake.calls().len(), 8);
        assert_eq!(
            fake.call_lines()[0],
            "gsettings get org.gnome.system.proxy mode"
        );
        assert_eq!(
            fake.call_lines()[7],
            "gsettings get org.gnome.system.proxy ignore-hosts"
        );
    }

    #[test]
    fn apply_sets_manual_mode_for_all_three_protocols() {
        let fake = FakeRunner::new();
        apply(&fake, "127.0.0.1", 65000).expect("apply");
        assert_eq!(
            fake.call_lines(),
            [
                "gsettings set org.gnome.system.proxy mode 'manual'",
                "gsettings set org.gnome.system.proxy.http host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.http port 65000",
                "gsettings set org.gnome.system.proxy.https host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.https port 65000",
                "gsettings set org.gnome.system.proxy.socks host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.socks port 65000",
                "gsettings set org.gnome.system.proxy ignore-hosts ['localhost', '127.0.0.0/8', '::1']",
            ]
        );
    }

    #[test]
    fn restore_writes_the_raw_snapshot_back() {
        let fake = FakeRunner::new();
        restore(&fake, &sample()).expect("restore");
        assert_eq!(
            fake.call_lines(),
            [
                "gsettings set org.gnome.system.proxy mode 'none'",
                "gsettings set org.gnome.system.proxy.http host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.http port 2080",
                "gsettings set org.gnome.system.proxy.https host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.https port 2080",
                "gsettings set org.gnome.system.proxy.socks host '127.0.0.1'",
                "gsettings set org.gnome.system.proxy.socks port 2080",
                "gsettings set org.gnome.system.proxy ignore-hosts ['localhost', '127.0.0.0/8', '::1']",
            ]
        );
    }

    #[test]
    fn detect_needs_a_working_gsettings() {
        let fake = FakeRunner::new();
        assert!(detect(&fake));
        let fake = FakeRunner::new();
        fake.missing("gsettings --version");
        assert!(!detect(&fake));
    }

    #[test]
    fn failed_set_is_an_error() {
        let fake = FakeRunner::new();
        fake.failing("gsettings set org.gnome.system.proxy mode 'manual'");
        assert!(apply(&fake, "127.0.0.1", 1080).is_err());
    }
}
