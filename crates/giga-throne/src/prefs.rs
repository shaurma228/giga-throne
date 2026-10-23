//! User preferences kept in `<data_root>/prefs.json`.
//!
//! A missing or unreadable file gives the defaults, so a broken file never stops
//! the app from starting.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Name of the preferences file in our data root.
const PREFS_FILE: &str = "prefs.json";

/// What the user can change in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// "Системный прокси": route the apps that use the system proxy through the VPN.
    pub system_proxy: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { system_proxy: true }
    }
}

/// Where the preferences file lives.
pub fn path(data_root: &Path) -> PathBuf {
    data_root.join(PREFS_FILE)
}

/// Reads the preferences at `path`. Missing or invalid content gives the defaults.
pub fn load(path: &Path) -> Prefs {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
            warn!("ignoring {}: {err}", path.display());
            Prefs::default()
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Prefs::default(),
        Err(err) => {
            warn!("could not read {}: {err}", path.display());
            Prefs::default()
        }
    }
}

/// Writes the preferences to `path`, creating the directory if needed.
pub fn save(path: &Path, prefs: &Prefs) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
    fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_the_system_proxy_on() {
        assert!(Prefs::default().system_proxy);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(load(&path(dir.path())), Prefs::default());
    }

    #[test]
    fn invalid_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = path(dir.path());
        fs::write(&file, "{ broken").expect("write");
        assert_eq!(load(&file), Prefs::default());
    }

    #[test]
    fn empty_object_gives_defaults_for_missing_keys() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = path(dir.path());
        fs::write(&file, "{}").expect("write");
        assert!(load(&file).system_proxy);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("nested").join(PREFS_FILE);
        save(
            &file,
            &Prefs {
                system_proxy: false,
            },
        )
        .expect("save");
        assert!(!load(&file).system_proxy);
        let text = fs::read_to_string(&file).expect("read");
        assert!(text.contains("\"system_proxy\": false"), "{text}");
    }
}
