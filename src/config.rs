//! Per-machine app configuration: `~/.config/shdo/config.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rust_i18n::t;
use toml_edit::{DocumentMut, value};

use crate::i18n::Lang;

pub const DEFAULT_CONFIG: &str = r#"# shdo – settings for this machine (not synced)

# UI language of the board: "en" or "de" (the CLI is always English)
# language = "en"

# Data directory (git repository with the tasks), default ~/.local/share/shdo
# dir = "~/tasks"

# Editor for `E` / `shdo edit`; defaults to $VISUAL, then $EDITOR
# editor = "hx"

# Theme, also set with `t` in the board
# theme = "tokyonight"

# Shown buckets (empty = all, "-" = no bucket), also set with `f` in the board
# buckets = ["work"]

# Show the done column, also toggled with `v` in the board
# show_done = true

# Show the calendar sidebar, also toggled with `c` in the board
# show_calendar = true

# Public holidays in the calendar: country-state, e.g. "de-bw"; empty = none
# (states: bw, by, be, bb, hb, hh, he, mv, ni, nw, rp, sl, sn, st, sh, th)
# holidays = "de-bw"
"#;

#[derive(Debug, Default, serde::Deserialize)]
pub struct AppConfig {
    pub language: Option<String>,
    pub dir: Option<String>,
    pub editor: Option<String>,
    pub theme: Option<String>,
    #[serde(default)]
    pub buckets: Vec<String>,
    pub show_done: Option<bool>,
    pub show_calendar: Option<bool>,
    pub holidays: Option<String>,
}

/// `$SHDO_CONFIG_DIR`, else `$XDG_CONFIG_HOME/shdo`, else `~/.config/shdo`.
///
/// `SHDO_CONFIG_DIR` only redirects shdo; overriding `XDG_CONFIG_HOME` would also start
/// the editor (e.g. hx) without its config.
pub fn config_dir() -> PathBuf {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var("SHDO_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    match var("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x).join("shdo"),
        None => home().join(".config").join("shdo"),
    }
}

/// `$XDG_DATA_HOME/shdo`, else `~/.local/share/shdo`.
pub fn default_data_dir() -> PathBuf {
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x).join("shdo"),
        None => home().join(".local").join("share").join("shdo"),
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<AppConfig> {
        match fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw)
                .with_context(|| t!("io.invalid", path = path.display()).into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
            Err(e) => Err(e).with_context(|| t!("io.read", path = path.display()).into_owned()),
        }
    }

    /// Order: `--dir`, then `dir` from the config, then [`default_data_dir`].
    pub fn data_dir(&self, cli: Option<&Path>) -> PathBuf {
        match (cli, &self.dir) {
            (Some(p), _) => p.to_path_buf(),
            (None, Some(d)) => expand_tilde(d),
            (None, None) => default_data_dir(),
        }
    }

    /// Configured UI language, otherwise guessed from the environment.
    pub fn lang(&self) -> Lang {
        self.language
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or_else(Lang::from_env)
    }

    pub fn editor(&self) -> String {
        self.editor
            .clone()
            .or_else(|| std::env::var("VISUAL").ok())
            .or_else(|| std::env::var("EDITOR").ok())
            .filter(|e| !e.trim().is_empty())
            .unwrap_or_else(|| "vi".into())
    }
}

/// Sets a string value in the config file; comments and other values are preserved.
pub fn set_value(path: &Path, key: &str, val: &str) -> Result<()> {
    update(path, |doc| doc[key] = value(val))
}

pub fn set_bool(path: &Path, key: &str, val: bool) -> Result<()> {
    update(path, |doc| doc[key] = value(val))
}

pub fn set_list(path: &Path, key: &str, vals: &[String]) -> Result<()> {
    update(path, |doc| {
        doc[key] = value(
            vals.iter()
                .map(String::as_str)
                .collect::<toml_edit::Array>(),
        )
    })
}

fn update(path: &Path, f: impl FnOnce(&mut DocumentMut)) -> Result<()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_CONFIG.into(),
        Err(e) => return Err(e).with_context(|| t!("io.read", path = path.display()).into_owned()),
    };
    let mut doc: DocumentMut = raw
        .parse()
        .with_context(|| t!("io.invalid", path = path.display()).into_owned())?;
    f(&mut doc);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, doc.to_string())
        .with_context(|| t!("io.write", path = path.display()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(&tmp.path().join("nope.toml")).unwrap();
        assert!(cfg.dir.is_none());
        assert_eq!(cfg.data_dir(Some(Path::new("/x"))), PathBuf::from("/x"));
    }

    #[test]
    fn set_value_keeps_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub/config.toml");
        set_value(&path, "theme", "nord").unwrap();
        fs::write(
            &path,
            fs::read_to_string(&path).unwrap() + "dir = \"~/t\" # mein Repo\n",
        )
        .unwrap();
        set_value(&path, "theme", "gruvbox").unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# Data directory"));
        assert!(raw.contains("# mein Repo"));
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.theme.as_deref(), Some("gruvbox"));
        assert_eq!(cfg.data_dir(None), home().join("t"));
        set_list(&path, "buckets", &["work".into(), "-".into()]).unwrap();
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.buckets, ["work", "-"]);
        assert_eq!(cfg.theme.as_deref(), Some("gruvbox"));
    }
}
