//! Schnellerfassungs-Syntax: `Rechnung bezahlen P1 due:fr @work`.
//! Gilt für die CLI und für das Eingabefeld der TUI.

use anyhow::{Result, bail};
use jiff::civil::Date;

use crate::dates;

#[derive(Debug, PartialEq)]
pub struct QuickAdd {
    pub title: String,
    pub prio: Option<u8>,
    pub due: Option<Date>,
    /// Roh-Eingabe nach `@`; aufgelöst wird gegen die Buckets in `shdo.toml`.
    pub bucket: Option<String>,
}

pub fn parse(words: &[String], today: Date) -> Result<QuickAdd> {
    let mut title = Vec::new();
    let mut prio = None;
    let mut due = None;
    let mut bucket = None;
    for word in words.iter().flat_map(|w| w.split_whitespace()) {
        if let Some(p) = parse_prio(word) {
            prio = Some(p);
        } else if let Some(d) = word.strip_prefix("due:") {
            due = Some(dates::parse_date(d, today)?);
        } else if let Some(b) = word.strip_prefix('@').filter(|b| !b.is_empty()) {
            bucket = Some(b.to_string());
        } else {
            title.push(word);
        }
    }
    if title.is_empty() {
        bail!("Titel fehlt");
    }
    Ok(QuickAdd {
        title: title.join(" "),
        prio,
        due,
        bucket,
    })
}

pub fn parse_str(input: &str, today: Date) -> Result<QuickAdd> {
    parse(&[input.to_string()], today)
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
                bucket: None
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
    fn errors() {
        assert!(parse(&words("P1 due:fr"), TODAY).is_err());
        assert!(parse(&words("x due:bald"), TODAY).is_err());
        assert!(parse_str("   ", TODAY).is_err());
    }
}
