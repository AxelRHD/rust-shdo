//! Quick-add syntax: `Pay invoice P1 due:fr @work +finance`.
//! Shared by the CLI and the TUI form.

use anyhow::{Result, bail};
use jiff::civil::Date;
use rust_i18n::t;

use crate::dates;

#[derive(Debug, PartialEq)]
pub struct QuickAdd {
    pub title: String,
    pub prio: Option<u8>,
    pub due: Option<Date>,
    /// Raw input after `@`; resolved against the buckets in `shdo.toml`.
    pub bucket: Option<String>,
    /// Words after `+`, normalized.
    pub tags: Vec<String>,
}

pub fn parse(words: &[String], today: Date) -> Result<QuickAdd> {
    let mut title = Vec::new();
    let mut prio = None;
    let mut due = None;
    let mut bucket = None;
    let mut tags = Vec::new();
    for word in words.iter().flat_map(|w| w.split_whitespace()) {
        if let Some(p) = parse_prio(word) {
            prio = Some(p);
        } else if let Some(d) = word.strip_prefix("due:") {
            due = Some(dates::parse_date(d, today)?);
        } else if let Some(b) = word.strip_prefix('@').filter(|b| !b.is_empty()) {
            bucket = Some(b.to_string());
        } else if let Some(tag) = word.strip_prefix('+').filter(|t| is_tag(t)) {
            tags.push(tag);
        } else {
            title.push(word);
        }
    }
    if title.is_empty() {
        bail!(t!("quickadd.title_missing"));
    }
    Ok(QuickAdd {
        title: title.join(" "),
        prio,
        due,
        bucket,
        tags: crate::task::normalize_tags(tags.into_iter()),
    })
}

pub fn parse_str(input: &str, today: Date) -> Result<QuickAdd> {
    parse(&[input.to_string()], today)
}

/// A tag starts with a letter, so `+3d` or a lone `+` stay in the title.
fn is_tag(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_alphabetic)
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn parse_prio(word: &str) -> Option<u8> {
    match word {
        "P1" | "p1" => Some(1),
        "P2" | "p2" => Some(2),
        "P3" | "p3" => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    const TODAY: Date = date(2026, 10, 22);

    fn words(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn plain_title() {
        let q = parse(&words("Rechnung bezahlen"), TODAY).unwrap();
        assert_eq!(
            q,
            QuickAdd {
                title: "Rechnung bezahlen".into(),
                prio: None,
                due: None,
                bucket: None,
                tags: vec![]
            }
        );
    }

    #[test]
    fn tokens_anywhere() {
        let q = parse(&words("P1 Rechnung due:fr bezahlen"), TODAY).unwrap();
        assert_eq!(q.title, "Rechnung bezahlen");
        assert_eq!(q.prio, Some(1));
        assert_eq!(q.due, Some(date(2026, 10, 23)));
        let q = parse(&words("Zahnarzt p3 due:+2w"), TODAY).unwrap();
        assert_eq!(q.prio, Some(3));
        assert_eq!(q.due, Some(date(2026, 11, 5)));
    }

    #[test]
    fn similar_words_stay_in_title() {
        let q = parse(&words("P4 P1-Team Meeting due"), TODAY).unwrap();
        assert_eq!(q.title, "P4 P1-Team Meeting due");
        assert_eq!(q.prio, None);
    }

    #[test]
    fn quoted_single_argument() {
        let q = parse(&["Rechnung bezahlen P2".to_string()], TODAY).unwrap();
        assert_eq!(q.title, "Rechnung bezahlen");
        assert_eq!(q.prio, Some(2));
    }

    #[test]
    fn bucket_token() {
        let q = parse(&words("Angebot @work schreiben"), TODAY).unwrap();
        assert_eq!(q.title, "Angebot schreiben");
        assert_eq!(q.bucket.as_deref(), Some("work"));
        let q = parse(&words("Mail an x @ y"), TODAY).unwrap();
        assert_eq!(q.title, "Mail an x @ y");
        assert_eq!(q.bucket, None);
    }

    #[test]
    fn tag_tokens() {
        let q = parse(&words("Deploy fixen +ops +Dev @work"), TODAY).unwrap();
        assert_eq!(q.title, "Deploy fixen");
        assert_eq!(q.tags, ["ops", "dev"]);
        let q = parse(&words("Plus +3d und + bleiben"), TODAY).unwrap();
        assert_eq!(q.title, "Plus +3d und + bleiben");
        assert!(q.tags.is_empty());
    }

    #[test]
    fn errors() {
        assert!(parse(&words("P1 due:fr"), TODAY).is_err());
        assert!(parse(&words("x due:bald"), TODAY).is_err());
        assert!(parse_str("   ", TODAY).is_err());
    }
}
