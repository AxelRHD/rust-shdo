//! UI language. The CLI is always English; the TUI uses the configured language.
//!
//! Texts live in `locales/app.yml`. Code that formats language-dependent output takes an
//! explicit [`Lang`] where tests need determinism, because the global locale is shared.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::De];

    pub fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
        }
    }

    /// Name of the language in that language, for the language picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::De => "Deutsch",
            Lang::En => "English (US)",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_lowercase().as_str() {
            "de" | "de-de" | "de_de" => Some(Lang::De),
            "en" | "en-us" | "en_us" => Some(Lang::En),
            _ => None,
        }
    }

    /// Guess from the usual locale variables (`LC_ALL`, `LC_MESSAGES`, `LANG`).
    pub fn from_env() -> Lang {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty())
            .map_or(Lang::En, |v| {
                if v.to_lowercase().starts_with("de") {
                    Lang::De
                } else {
                    Lang::En
                }
            })
    }
}

pub fn set(lang: Lang) {
    rust_i18n::set_locale(lang.code());
}

/// The currently active language.
pub fn current() -> Lang {
    Lang::from_code(&rust_i18n::locale()).unwrap_or(Lang::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(Lang::from_code("DE"), Some(Lang::De));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_code("fr"), None);
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
    }

    /// Every key referenced in the sources (`t!("…")` or as a quoted `kb.`/`help.`/… key)
    /// must exist in the locale file, otherwise the raw key would show up in the UI.
    #[test]
    fn all_used_keys_are_defined() {
        let root = env!("CARGO_MANIFEST_DIR");
        let yml = std::fs::read_to_string(format!("{root}/locales/app.yml")).unwrap();
        let defined: std::collections::HashSet<&str> = yml
            .lines()
            .filter(|l| !l.starts_with(' ') && !l.starts_with('#') && l.ends_with(':'))
            .map(|l| l.trim_end_matches(':'))
            .collect();
        let prefixes = [
            "kb.",
            "help.",
            "form.",
            "cal.",
            "date.",
            "board.",
            "msg.",
            "bucket.",
            "warn.",
            "detail.",
            "hdr.",
            "col.",
            "input.",
            "filter.",
            "git.",
            "io.",
            "id.",
            "task.",
            "store.",
            "theme.",
            "status.",
            "delete.",
            "editor.",
            "quickadd.",
            "setup.",
            "tagfilter.",
            "settings.",
        ];
        let mut missing = vec![];
        let mut stack = vec![std::path::PathBuf::from(format!("{root}/src"))];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap();
                for piece in src.split('"').skip(1).step_by(2) {
                    let is_key = prefixes.iter().any(|p| piece.starts_with(p))
                        && !piece.ends_with('.')
                        && piece
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
                    if is_key && !defined.contains(piece) {
                        missing.push(format!("{}: {piece}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "undefined keys: {missing:?}");
    }

    #[test]
    fn every_key_has_both_languages() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/app.yml"))
            .unwrap();
        // Light YAML check without a YAML parser: every top-level key must be followed by
        // an `en:` and a `de:` line before the next key.
        let mut key: Option<(&str, bool, bool)> = None;
        let mut missing = vec![];
        for line in raw
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        {
            if !line.starts_with(' ') {
                if let Some((k, en, de)) = key.take()
                    && !(en && de)
                {
                    missing.push(k.to_string());
                }
                if !line.starts_with("_version") {
                    key = Some((line.trim_end_matches(':'), false, false));
                }
            } else if let Some((_, en, de)) = key.as_mut() {
                let l = line.trim_start();
                *en |= l.starts_with("en:");
                *de |= l.starts_with("de:");
            }
        }
        if let Some((k, en, de)) = key
            && !(en && de)
        {
            missing.push(k.to_string());
        }
        assert!(missing.is_empty(), "keys without en/de: {missing:?}");
    }
}
