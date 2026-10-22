//! Rechnerlokale App-Konfiguration: `~/.config/shdo/config.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, value};

pub const DEFAULT_CONFIG: &str = r#"# shdo – lokale Einstellungen (werden nicht synchronisiert)

# Datenverzeichnis (Git-Repo mit den Tasks)
# dir = "~/tasks"

# Editor für `e` / `shdo edit`; ohne Angabe gilt $VISUAL, dann $EDITOR
# editor = "hx"

# Theme, wird auch über `t` in der TUI gesetzt
# theme = "tokyonight"

# Angezeigte Buckets (leer = alle, "-" = ohne Bucket), wird über `f` in der TUI gesetzt
# buckets = ["work"]
"#;

#[derive(Debug, Default, serde::Deserialize)]
pub struct AppConfig {
    pub dir: Option<String>,
    pub editor: Option<String>,
    pub theme: Option<String>,
    #[serde(default)]
    pub buckets: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x).join("shdo"),
        None => home().join(".config").join("shdo"),
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
            Ok(raw) => toml::from_str(&raw).with_context(|| format!("{} ungültig", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
            Err(e) => Err(e).with_context(|| format!("{} lesen", path.display())),
        }
    }

    /// Reihenfolge: `--dir`, dann `dir` aus der Config, dann `~/tasks`.
    pub fn data_dir(&self, cli: Option<&Path>) -> PathBuf {
        match (cli, &self.dir) {
            (Some(p), _) => p.to_path_buf(),
            (None, Some(d)) => expand_tilde(d),
            (None, None) => home().join("tasks"),
        }
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

/// Setzt einen String-Wert in der Config-Datei; Kommentare und andere Werte bleiben erhalten.
pub fn set_value(path: &Path, key: &str, val: &str) -> Result<()> {
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
        Err(e) => return Err(e).with_context(|| format!("{} lesen", path.display())),
    };
    let mut doc: DocumentMut = raw
        .parse()
        .with_context(|| format!("{} ungültig", path.display()))?;
    f(&mut doc);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, doc.to_string()).with_context(|| format!("{} schreiben", path.display()))
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
        assert!(raw.contains("# Datenverzeichnis"));
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
