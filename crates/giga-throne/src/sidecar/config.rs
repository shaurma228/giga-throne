//! Paths, port and executable of the headless Throne, and the API key file.

use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use throne_api::ApiKey;

/// Loopback port of the Throne API when `GIGA_THRONE_PORT` is not set.
pub const DEFAULT_PORT: u16 = 9096;

/// Where the sidecar keeps its files and which Throne it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarConfig {
    /// The Throne executable.
    pub binary: PathBuf,
    /// Our data root. Holds `throne/` (Throne's data directory) and `api.key`.
    pub data_root: PathBuf,
    /// Port of the Throne API on 127.0.0.1.
    pub port: u16,
}

impl SidecarConfig {
    /// Reads the configuration from the environment and the platform directories.
    ///
    /// Honours `GIGA_THRONE_SIDECAR` (path to Throne) and `GIGA_THRONE_PORT`.
    pub fn from_env() -> Result<Self> {
        let binary = resolve_binary(
            std::env::var_os("GIGA_THRONE_SIDECAR"),
            current_exe_dir().as_deref(),
            dev_candidate().as_deref(),
        )?;
        let port = parse_port(std::env::var("GIGA_THRONE_PORT").ok().as_deref())?;
        Ok(Self {
            binary,
            data_root: default_data_root()?,
            port,
        })
    }

    /// Throne's data directory (`--data-dir`).
    pub fn throne_data_dir(&self) -> PathBuf {
        self.data_root.join("throne")
    }

    /// The API key file (`--api-key-file`).
    pub fn key_file(&self) -> PathBuf {
        self.data_root.join("api.key")
    }

    /// The loopback address Throne listens on (`--api-listen`).
    pub fn listen_addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// Our data root: the platform's data directory for giga-throne.
pub fn default_data_root() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "giga-throne")
        .context("cannot find the user's data directory")?;
    Ok(dirs.data_dir().to_path_buf())
}

/// Directory of our executable, if it is known.
fn current_exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

/// The debug-only path to the Throne checked out in `sidecar/`.
fn dev_candidate() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../sidecar/Throne/Throne"
        )))
    } else {
        None
    }
}

/// Picks the Throne executable: the first existing file among the env override,
/// the debug path, and `Throne/Throne` and `Throne` next to our executable.
///
/// Takes its inputs as arguments so that it can be tested without touching the
/// process environment.
pub fn resolve_binary(
    env_override: Option<OsString>,
    exe_dir: Option<&Path>,
    dev: Option<&Path>,
) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(value) = env_override.filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(value));
    }
    if let Some(path) = dev {
        candidates.push(path.to_path_buf());
    }
    if let Some(dir) = exe_dir {
        candidates.push(dir.join("Throne").join("Throne"));
        candidates.push(dir.join("Throne"));
    }
    let candidates: Vec<PathBuf> = candidates.into_iter().map(with_exe_suffix).collect();

    if let Some(found) = candidates.iter().find(|path| path.is_file()) {
        return Ok(found.clone());
    }
    let tried: Vec<String> = candidates
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect();
    bail!(
        "не найден исполняемый файл Throne; проверены пути:\n{}\nукажите его в GIGA_THRONE_SIDECAR",
        tried.join("\n")
    )
}

fn with_exe_suffix(path: PathBuf) -> PathBuf {
    if cfg!(windows) && path.extension() != Some(OsStr::new("exe")) {
        let mut name = path.into_os_string();
        name.push(".exe");
        PathBuf::from(name)
    } else {
        path
    }
}

/// Parses `GIGA_THRONE_PORT`. Unset or empty means [`DEFAULT_PORT`].
pub fn parse_port(value: Option<&str>) -> Result<u16> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(DEFAULT_PORT),
        Some(text) => text
            .parse::<u16>()
            .with_context(|| format!("GIGA_THRONE_PORT is not a port number: {text:?}")),
    }
}

/// Reads the API key from `path`, or creates a new one there if the file is absent.
///
/// An existing file that does not hold a valid key is an error: it is never
/// overwritten. A new file is created with mode 0600 on unix.
pub fn load_or_create_key(path: &Path) -> Result<ApiKey> {
    match std::fs::read_to_string(path) {
        Ok(text) => ApiKey::from_text(&text).map_err(|_| {
            anyhow!(
                "файл ключа {} повреждён; переименуйте или удалите его, чтобы создать новый ключ",
                path.display()
            )
        }),
        Err(err) if err.kind() == ErrorKind::NotFound => create_key(path),
        Err(err) => Err(err).with_context(|| format!("чтение ключа {}", path.display())),
    }
}

fn create_key(path: &Path) -> Result<ApiKey> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("создание каталога {}", parent.display()))?;
    }
    let key = ApiKey::generate().context("генерация ключа API")?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("создание файла ключа {}", path.display()))?;
    file.write_all(format!("{}\n", key.to_text()).as_bytes())
        .with_context(|| format!("запись ключа в {}", path.display()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::write(path, b"x").expect("write fake executable");
    }

    #[test]
    fn env_override_wins_over_the_dev_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let from_env = dir.path().join("from-env");
        let dev = dir.path().join("dev");
        touch(&from_env);
        touch(&dev);
        let found = resolve_binary(Some(from_env.clone().into_os_string()), None, Some(&dev))
            .expect("env override exists");
        assert_eq!(found, from_env);
    }

    #[test]
    fn missing_env_override_falls_through_to_the_dev_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dev = dir.path().join("dev");
        touch(&dev);
        let missing = dir.path().join("missing");
        let found = resolve_binary(Some(missing.into_os_string()), None, Some(&dev))
            .expect("dev path exists");
        assert_eq!(found, dev);
    }

    #[test]
    fn exe_dir_layouts_are_searched_and_directories_skipped() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A directory named like the binary must not be picked.
        std::fs::create_dir_all(dir.path().join("Throne")).expect("mkdir");
        let nested = dir.path().join("Throne").join("Throne");
        std::fs::create_dir_all(nested.parent().expect("parent")).expect("mkdir");
        touch(&nested);
        let found = resolve_binary(None, Some(dir.path()), None).expect("nested layout");
        assert_eq!(found, nested);
    }

    #[test]
    fn nothing_found_lists_the_tried_paths() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("nope");
        let err = resolve_binary(Some(missing.clone().into_os_string()), None, None)
            .expect_err("nothing exists");
        let text = err.to_string();
        assert!(text.contains(&missing.display().to_string()), "{text}");
    }

    #[test]
    fn port_defaults_and_validates() {
        assert_eq!(parse_port(None).expect("default"), DEFAULT_PORT);
        assert_eq!(parse_port(Some("  ")).expect("empty"), DEFAULT_PORT);
        assert_eq!(parse_port(Some("9100")).expect("port"), 9100);
        assert!(parse_port(Some("abc")).is_err());
        assert!(parse_port(Some("70000")).is_err());
    }

    #[test]
    fn key_is_created_once_with_private_perms_and_reused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("sub").join("api.key");
        let first = load_or_create_key(&path).expect("created");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(text.ends_with('\n'));
        assert_eq!(text.trim_end().len(), 43);
        let second = load_or_create_key(&path).expect("reused");
        assert_eq!(first.as_bytes(), second.as_bytes());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn invalid_key_file_is_an_error_and_is_left_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("api.key");
        std::fs::write(&path, "not a key\n").expect("write");
        assert!(load_or_create_key(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "not a key\n");
    }
}
