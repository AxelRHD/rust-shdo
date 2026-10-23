//! What the board remembers by itself – filters, done column, calendar – kept apart
//! from the settings: `$XDG_STATE_HOME/shdo/state.toml` (usually ~/.local/state/shdo).
//!
//! Earlier versions stored these values in config.toml; [`migrate`] moves them once.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rust_i18n::t;
use toml_edit::{DocumentMut, value};

use crate::config;

const TEMPLATE: &str = "# shdo – remembered view state (filters, shown columns); safe to delete\n";

/// Keys that live here; older versions kept them in config.toml.
const KEYS: [&str; 4] = ["show_done", "show_calendar", "buckets", "tags"];

#[derive(Debug, Default, serde::Deserialize)]
pub struct ViewState {
    pub show_done: Option<bool>,
    pub show_calendar: Option<bool>,
    /// Shown buckets (empty = all, "-" = no bucket)
    #[serde(default)]
    pub buckets: Vec<String>,
    /// Shown tags (empty = no tag filter)
    #[serde(default)]
    pub tags: Vec<String>,
}

/// With `SHDO_CONFIG_DIR` (e.g. the demo) the state sits next to that config, so a
/// redirected config never mixes with the real state. Otherwise the XDG state dir.
pub fn path() -> PathBuf {
    if let Some(dir) = std::env::var_os("SHDO_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir).join("state.toml");
    }
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => config::expand_tilde("~/.local/state"),
    };
    base.join("shdo").join("state.toml")
}

impl ViewState {
    pub fn load(path: &Path) -> Result<ViewState> {
        match fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw)
                .with_context(|| t!("io.invalid", path = path.display()).into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ViewState::default()),
            Err(e) => Err(e).with_context(|| t!("io.read", path = path.display()).into_owned()),
        }
    }
}

/// Like `config::update_file`, but a new file starts with the header comment (toml_edit
/// would otherwise put the first keys above a comment-only template).
fn update(path: &Path, f: impl FnOnce(&mut DocumentMut)) -> Result<()> {
    if path.exists() {
        return config::update_file(path, TEMPLATE, f);
    }
    let mut doc = DocumentMut::new();
    f(&mut doc);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, format!("{TEMPLATE}{doc}"))
        .with_context(|| t!("io.write", path = path.display()).into_owned())
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

/// Moves view state left in config.toml by earlier versions into the state file.
/// Values already in the state file win; the old keys are removed from the config.
pub fn migrate(cfg_path: &Path, state_path: &Path) -> Result<()> {
    let Ok(raw) = fs::read_to_string(cfg_path) else {
        return Ok(());
    };
    let cfg: DocumentMut = raw
        .parse()
        .with_context(|| t!("io.invalid", path = cfg_path.display()).into_owned())?;
    let found: Vec<(&str, toml_edit::Item)> = KEYS
        .iter()
        .filter_map(|k| cfg.get(k).map(|item| (*k, item.clone())))
        .collect();
    if found.is_empty() {
        return Ok(());
    }
    update(state_path, |doc| {
        for (key, item) in &found {
            if !doc.contains_key(key) {
                doc[*key] = item.clone();
            }
        }
    })?;
    config::update_file(cfg_path, config::DEFAULT_CONFIG, |doc| {
        for (key, _) in &found {
            doc.remove(key);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state/state.toml");
        assert!(ViewState::load(&path).unwrap().show_done.is_none());
        set_bool(&path, "show_done", false).unwrap();
        set_list(&path, "buckets", &["work".into(), "-".into()]).unwrap();
        set_list(&path, "tags", &["dev".into()]).unwrap();
        let s = ViewState::load(&path).unwrap();
        assert_eq!(s.show_done, Some(false));
        assert_eq!(s.buckets, ["work", "-"]);
        assert_eq!(s.tags, ["dev"]);
        assert!(fs::read_to_string(&path).unwrap().starts_with("# shdo"));
    }

    #[test]
    fn migrates_old_config_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg_path = tmp.path().join("config.toml");
        let state_path = tmp.path().join("state.toml");
        fs::write(
            &cfg_path,
            "language = \"de\"\nshow_done = false\nbuckets = [\"work\"]\ntheme = \"nord\" # mine\n",
        )
        .unwrap();
        // a value already in the state file wins
        set_list(&state_path, "buckets", &["home".into()]).unwrap();
        migrate(&cfg_path, &state_path).unwrap();
        let raw = fs::read_to_string(&cfg_path).unwrap();
        assert!(
            !raw.contains("show_done") && !raw.contains("buckets"),
            "{raw}"
        );
        assert!(raw.contains("theme = \"nord\" # mine"));
        let s = ViewState::load(&state_path).unwrap();
        assert_eq!(s.show_done, Some(false));
        assert_eq!(s.buckets, ["home"]);
        // nothing left to move: config stays untouched
        let before = fs::read_to_string(&cfg_path).unwrap();
        migrate(&cfg_path, &state_path).unwrap();
        assert_eq!(fs::read_to_string(&cfg_path).unwrap(), before);
    }
}
