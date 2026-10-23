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

# Bucket for new tasks when neither @bucket nor the filter decides (key, e.g. "work")
# default_bucket = "work"

# Show done tasks of the last N days (0 = all); `shdo archive` archives older ones
# done_days = 14

# Sync with the git remote when the board opens and before it closes
# (never on every change); `s` syncs in between
# auto_sync = false

# Public holidays in the calendar: country-state, e.g. "de-bw"; empty = none
# (states: bw, by, be, bb, hb, hh, he, mv, ni, nw, rp, sl, sn, st, sh, th)
# holidays = "de-bw"

# What the board remembers by itself (filters, shown columns, calendar) is not kept
# here but in ~/.local/state/shdo/state.toml.
"#;

#[derive(Debug, Default, serde::Deserialize)]
pub struct AppConfig {
    pub language: Option<String>,
    pub dir: Option<String>,
    pub editor: Option<String>,
    pub theme: Option<String>,
    pub auto_sync: Option<bool>,
    pub default_bucket: Option<String>,
    pub done_days: Option<u32>,
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

/// Default for `done_days`.
pub const DONE_DAYS_DEFAULT: u32 = 14;

impl AppConfig {
    /// Days of done tasks to show; `None` = all (`done_days = 0`).
    pub fn done_days(&self) -> Option<u32> {
        match self.done_days.unwrap_or(DONE_DAYS_DEFAULT) {
            0 => None,
            n => Some(n),
        }
    }

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

/// Creates the config file from the commented template if it does not exist yet,
/// so it can be opened in an editor.
pub fn ensure_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, DEFAULT_CONFIG)
        .with_context(|| t!("io.write", path = path.display()).into_owned())
}

/// Sets a string value in the config file; comments and other values are preserved.
pub fn set_value(path: &Path, key: &str, val: &str) -> Result<()> {
    update(path, |doc| doc[key] = value(val))
}

pub fn set_int(path: &Path, key: &str, val: i64) -> Result<()> {
    update(path, |doc| doc[key] = value(val))
}

pub fn remove_key(path: &Path, key: &str) -> Result<()> {
    update(path, |doc| {
        doc.remove(key);
    })
}

pub fn set_bool(path: &Path, key: &str, val: bool) -> Result<()> {
    update(path, |doc| doc[key] = value(val))
}

/// Result of the setup dialog.
pub struct Setup {
    pub language: Lang,
    /// Data directory as entered (may start with `~`).
    pub dir: String,
    /// `holidays` value such as `de-bw`; `None` = no holidays.
    pub holidays: Option<String>,
}

/// Writes the setup choices; other settings and comments in an existing file stay.
pub fn write_setup(path: &Path, setup: &Setup) -> Result<()> {
    update(path, |doc| {
        doc["language"] = value(setup.language.code());
        doc["dir"] = value(setup.dir.as_str());
        match &setup.holidays {
            Some(h) => doc["holidays"] = value(h.as_str()),
            None => {
                doc.remove("holidays");
            }
        }
    })
}

/// `/home/me/x` → `~/x`, for display and for storing paths in the config.
pub fn contract_tilde(p: &Path) -> String {
    let home = home();
    match p.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn update(path: &Path, f: impl FnOnce(&mut DocumentMut)) -> Result<()> {
    update_file(path, DEFAULT_CONFIG, f)
}

/// Edits a TOML file in place (keeping comments); `template` is used if it does not exist.
pub(crate) fn update_file(
    path: &Path,
    template: &str,
    f: impl FnOnce(&mut DocumentMut),
) -> Result<()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => template.into(),
        Err(e) => return Err(e).with_context(|| t!("io.read", path = path.display()).into_owned()),
    };
    let mut doc: DocumentMut = raw
        .parse()
        .with_context(|| t!("io.invalid", path = path.display()).into_owned())?;
    let before: Vec<String> = doc.iter().map(|(k, _)| k.to_string()).collect();
    f(&mut doc);
    let out = place_new_keys(doc, &before);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, out).with_context(|| t!("io.write", path = path.display()).into_owned())
}

/// toml_edit puts new top-level keys at the start of the file. If the file has a
/// commented example for such a key (`# theme = "tokyonight"`), the value replaces that
/// line instead, so it sits next to its explanation.
fn place_new_keys(mut doc: DocumentMut, before: &[String]) -> String {
    let new_keys: Vec<String> = doc
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| !before.contains(k))
        .collect();
    let mut replacements = vec![];
    for key in new_keys {
        let example = format!("# {key} =");
        if !doc.to_string().lines().any(|l| l.starts_with(&example)) {
            continue;
        }
        if let Some(item) = doc.remove(&key)
            && let Some(v) = item.as_value()
        {
            replacements.push((example, format!("{key} = {}", v.to_string().trim())));
        }
    }
    let mut out = doc.to_string();
    for (example, line) in replacements {
        let mut done = false;
        out = out
            .lines()
            .map(|l| {
                if !done && l.starts_with(&example) {
                    done = true;
                    line.clone()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
    }
    out
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
    fn new_values_replace_their_commented_example() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        set_value(&path, "theme", "nord").unwrap();
        set_bool(&path, "auto_sync", true).unwrap();
        set_value(&path, "theme", "gruvbox-dark").unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("# shdo"), "header stays first:\n{raw}");
        assert!(raw.contains("# Theme, also set with `t` in the board\ntheme = \"gruvbox-dark\""));
        assert!(!raw.contains("# theme ="));
        assert!(raw.contains("auto_sync = true"));
        // a key without a commented example still works
        set_value(&path, "custom", "x").unwrap();
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.theme.as_deref(), Some("gruvbox-dark"));
        assert_eq!(cfg.auto_sync, Some(true));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("custom = \"x\"")
        );
    }

    #[test]
    fn setup_is_written_and_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut setup = Setup {
            language: Lang::De,
            dir: "~/notes/tasks".into(),
            holidays: Some("de-by".into()),
        };
        write_setup(&path, &setup).unwrap();
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.lang(), Lang::De);
        assert_eq!(cfg.data_dir(None), home().join("notes/tasks"));
        assert_eq!(cfg.holidays.as_deref(), Some("de-by"));
        setup.holidays = None;
        write_setup(&path, &setup).unwrap();
        assert!(AppConfig::load(&path).unwrap().holidays.is_none());
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("# Data directory")
        );
        assert_eq!(contract_tilde(&home().join("a/b")), "~/a/b");
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
        set_value(&path, "default_bucket", "work").unwrap();
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.default_bucket.as_deref(), Some("work"));
        assert_eq!(cfg.theme.as_deref(), Some("gruvbox"));
    }
}
