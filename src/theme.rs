//! Themes: semantische Farb-Tokens, eingebaut per `include_str!` oder als eigene
//! TOML-Dateien in `~/.config/shdo/themes/`. Fehlende Tokens kommen vom Default-Theme.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use ratatui::style::Color;

pub const DEFAULT_THEME: &str = "tokyonight";

const BUILTIN: &[(&str, &str)] = &[
    ("tokyonight", include_str!("../themes/tokyonight.toml")),
    ("nord", include_str!("../themes/nord.toml")),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.toml")),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
    ("terminal", include_str!("../themes/terminal.toml")),
];

#[derive(Debug, Clone)]
pub struct Palette {
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub border: Color,
    pub border_active: Color,
    pub title: Color,
    pub accent: Color,
    pub selection: Color,
    pub prio_high: Color,
    pub prio_low: Color,
    pub overdue: Color,
    pub today: Color,
    pub done: Color,
    pub key: Color,
    pub warn: Color,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub c: Palette,
}

#[derive(serde::Deserialize)]
struct ThemeFile {
    name: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

fn parse_color(s: &str) -> Result<Color> {
    Color::from_str(s).map_err(|_| anyhow!("ungültige Farbe {s:?}"))
}

impl Palette {
    fn apply(&mut self, colors: &BTreeMap<String, String>) -> Result<()> {
        for (key, val) in colors {
            let slot = match key.as_str() {
                "bg" => &mut self.bg,
                "fg" => &mut self.fg,
                "dim" => &mut self.dim,
                "border" => &mut self.border,
                "border_active" => &mut self.border_active,
                "title" => &mut self.title,
                "accent" => &mut self.accent,
                "selection" => &mut self.selection,
                "prio_high" => &mut self.prio_high,
                "prio_low" => &mut self.prio_low,
                "overdue" => &mut self.overdue,
                "today" => &mut self.today,
                "done" => &mut self.done,
                "key" => &mut self.key,
                "warn" => &mut self.warn,
                other => return Err(anyhow!("unbekanntes Farb-Token „{other}“")),
            };
            *slot = parse_color(val).with_context(|| format!("Token „{key}“"))?;
        }
        Ok(())
    }
}

fn fallback() -> Palette {
    let mut p = Palette {
        bg: Color::Reset,
        fg: Color::Reset,
        dim: Color::DarkGray,
        border: Color::DarkGray,
        border_active: Color::Green,
        title: Color::Blue,
        accent: Color::Cyan,
        selection: Color::Black,
        prio_high: Color::Red,
        prio_low: Color::DarkGray,
        overdue: Color::Red,
        today: Color::Yellow,
        done: Color::Green,
        key: Color::Magenta,
        warn: Color::Yellow,
    };
    let raw = BUILTIN
        .iter()
        .find(|(id, _)| *id == DEFAULT_THEME)
        .map(|(_, raw)| *raw)
        .expect("Default-Theme ist eingebaut");
    let base: ThemeFile = toml::from_str(raw).expect("Default-Theme ist gültig");
    p.apply(&base.colors).expect("Default-Theme ist gültig");
    p
}

fn parse_theme(id: &str, raw: &str) -> Result<Theme> {
    let file: ThemeFile = toml::from_str(raw)?;
    let mut c = fallback();
    c.apply(&file.colors)?;
    Ok(Theme {
        id: id.into(),
        name: file.name.unwrap_or_else(|| id.into()),
        c,
    })
}

/// Alle Themes alphabetisch nach Name: eingebaute und eigene (gleiche ID überschreibt).
/// Fehlerhafte eigene Themes landen als Warnung im zweiten Wert.
pub fn load_all(custom_dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let mut themes: Vec<Theme> = BUILTIN
        .iter()
        .map(|(id, raw)| parse_theme(id, raw).expect("eingebaute Themes sind gültig"))
        .collect();
    let mut warnings = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(custom_dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|p: &std::path::PathBuf| p.extension().is_some_and(|x| x == "toml"));
    files.sort();
    for path in files {
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let parsed = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|raw| parse_theme(&id, &raw));
        match parsed {
            Ok(t) => match themes.iter_mut().find(|x| x.id == id) {
                Some(slot) => *slot = t,
                None => themes.push(t),
            },
            Err(e) => warnings.push(format!("Theme {}: {e:#}", path.display())),
        }
    }
    themes.sort_by_key(|t| t.name.to_lowercase());
    (themes, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let (themes, warnings) = load_all(tmp.path());
        assert!(warnings.is_empty());
        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted);
        let by_id = |id: &str| themes.iter().find(|t| t.id == id).unwrap();
        assert_eq!(by_id(DEFAULT_THEME).c.bg, Color::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(by_id("nord").c.bg, Color::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(by_id("terminal").c.bg, Color::Reset);
        // Jedes eingebaute Theme setzt alle Tokens selbst (nichts erbt still vom Default).
        for (id, raw) in BUILTIN {
            let file: ThemeFile = toml::from_str(raw).unwrap();
            assert_eq!(file.colors.len(), 15, "{id}");
        }
    }

    #[test]
    fn custom_themes_merge_and_warn() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("mein.toml"), "[colors]\nbg = \"#000000\"\n").unwrap();
        std::fs::write(tmp.path().join("kaputt.toml"), "[colors]\nbg = \"lila\"\n").unwrap();
        std::fs::write(
            tmp.path().join("tippfehler.toml"),
            "[colors]\nbgg = \"red\"\n",
        )
        .unwrap();
        let (themes, warnings) = load_all(tmp.path());
        let mein = themes.iter().find(|t| t.id == "mein").unwrap();
        assert_eq!(mein.name, "mein");
        assert_eq!(mein.c.bg, Color::Rgb(0, 0, 0));
        let default = themes.iter().find(|t| t.id == DEFAULT_THEME).unwrap();
        assert_eq!(mein.c.fg, default.c.fg);
        assert_eq!(warnings.len(), 2);
    }
}
