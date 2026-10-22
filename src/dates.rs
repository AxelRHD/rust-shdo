//! Parsen von Datumseingaben (`due:`) und deutsche Kurzdarstellung von Daten.

use anyhow::{Result, bail};
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("mo", Weekday::Monday),
    ("di", Weekday::Tuesday),
    ("mi", Weekday::Wednesday),
    ("do", Weekday::Thursday),
    ("fr", Weekday::Friday),
    ("sa", Weekday::Saturday),
    ("so", Weekday::Sunday),
];

pub fn today() -> Date {
    jiff::Zoned::now().date()
}

/// Parst eine Datumseingabe relativ zu `today`.
///
/// Unterstützt: `2026-10-24`, `24.10.`, `24.10.2026`, `+3d`, `+2w`, `+1m`,
/// `heute`, `morgen`, `übermorgen` und Wochentage (`mo`–`so`, nächstes Vorkommen).
pub fn parse_date(input: &str, today: Date) -> Result<Date> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        bail!("leeres Datum");
    }
    match s.as_str() {
        "heute" => return Ok(today),
        "morgen" => return Ok(today.tomorrow()?),
        "übermorgen" | "uebermorgen" => return Ok(today.checked_add(2.days())?),
        _ => {}
    }
    if let Some((_, wd)) = WEEKDAYS.iter().find(|(name, _)| *name == s) {
        let mut d = today.tomorrow()?;
        while d.weekday() != *wd {
            d = d.tomorrow()?;
        }
        return Ok(d);
    }
    if let Some(rel) = s.strip_prefix('+') {
        return parse_relative(rel, today).ok_or_else(|| invalid(input));
    }
    if let Ok(d) = s.parse::<Date>() {
        return Ok(d);
    }
    parse_german(&s, today).ok_or_else(|| invalid(input))
}

fn invalid(input: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "ungültiges Datum „{input}“ (erlaubt: 2026-10-24, 24.10., +3d, +2w, +1m, heute, morgen, mo–so)"
    )
}

fn parse_relative(rel: &str, today: Date) -> Option<Date> {
    let unit = rel.chars().last()?;
    let n: i64 = rel[..rel.len() - unit.len_utf8()].parse().ok()?;
    let span = match unit {
        'd' | 't' => n.days(),
        'w' => n.weeks(),
        'm' => n.months(),
        _ => return None,
    };
    today.checked_add(span).ok()
}

/// `24.10.` (nächstes Vorkommen) oder `24.10.2026`.
fn parse_german(s: &str, today: Date) -> Option<Date> {
    let parts: Vec<&str> = s.split('.').collect();
    let (day, month, year) = match parts.as_slice() {
        [d, m] | [d, m, ""] => (d.parse().ok()?, m.parse().ok()?, None),
        [d, m, y] => (
            d.parse().ok()?,
            m.parse().ok()?,
            Some(y.parse::<i16>().ok()?),
        ),
        _ => return None,
    };
    match year {
        Some(y) => {
            let y = if y < 100 { 2000 + y } else { y };
            Date::new(y, month, day).ok()
        }
        None => {
            let d = Date::new(today.year(), month, day).ok()?;
            if d < today {
                Date::new(today.year() + 1, month, day).ok()
            } else {
                Some(d)
            }
        }
    }
}

fn weekday_abbr(d: Date) -> &'static str {
    WEEKDAYS
        .iter()
        .find(|(_, wd)| *wd == d.weekday())
        .map(|(n, _)| *n)
        .unwrap_or("")
}

fn days_between(from: Date, to: Date) -> i64 {
    from.until(to).map(|s| s.get_days() as i64).unwrap_or(0)
}

fn short(d: Date, today: Date) -> String {
    if d.year() == today.year() {
        format!("{:02}.{:02}.", d.day(), d.month())
    } else {
        format!("{:02}.{:02}.{:02}", d.day(), d.month(), d.year() % 100)
    }
}

/// Kurzform für Fälligkeiten: `heute`, `morgen`, `gestern`, `fr 24.10.`, `24.10.`.
pub fn fmt_due(due: Date, today: Date) -> String {
    match days_between(today, due) {
        0 => "heute".into(),
        1 => "morgen".into(),
        -1 => "gestern".into(),
        2..=6 => format!("{} {}", weekday_abbr(due), short(due, today)),
        _ => short(due, today),
    }
}

pub fn local_date(ts: Timestamp) -> Date {
    ts.to_zoned(TimeZone::system()).date()
}

/// „seit heute“, „seit gestern“, „seit 3 Tagen“.
pub fn fmt_since(ts: Timestamp, today: Date) -> String {
    match days_between(local_date(ts), today) {
        i64::MIN..=0 => "seit heute".into(),
        1 => "seit gestern".into(),
        n => format!("seit {n} Tagen"),
    }
}

/// „heute“, „gestern“, „vor 3 Tagen“, ab einer Woche das Datum.
pub fn fmt_ago(ts: Timestamp, today: Date) -> String {
    let d = local_date(ts);
    match days_between(d, today) {
        i64::MIN..=0 => "heute".into(),
        1 => "gestern".into(),
        n @ 2..=6 => format!("vor {n} Tagen"),
        _ => short(d, today),
    }
}

/// Vollständige lokale Darstellung für die Detailansicht.
pub fn fmt_timestamp(ts: Timestamp) -> String {
    ts.to_zoned(TimeZone::system())
        .strftime("%d.%m.%Y %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    // Donnerstag
    const TODAY: Date = date(2026, 10, 22);

    #[test]
    fn iso_and_keywords() {
        assert_eq!(parse_date("2026-10-24", TODAY).unwrap(), date(2026, 10, 24));
        assert_eq!(parse_date("heute", TODAY).unwrap(), TODAY);
        assert_eq!(parse_date("Morgen", TODAY).unwrap(), date(2026, 10, 23));
        assert_eq!(parse_date("übermorgen", TODAY).unwrap(), date(2026, 10, 24));
    }

    #[test]
    fn relative() {
        assert_eq!(parse_date("+3d", TODAY).unwrap(), date(2026, 10, 25));
        assert_eq!(parse_date("+2w", TODAY).unwrap(), date(2026, 11, 5));
        assert_eq!(parse_date("+1m", TODAY).unwrap(), date(2026, 11, 22));
        assert_eq!(parse_date("+0d", TODAY).unwrap(), TODAY);
    }

    #[test]
    fn weekdays_next_occurrence_not_today() {
        assert_eq!(parse_date("fr", TODAY).unwrap(), date(2026, 10, 23));
        assert_eq!(parse_date("do", TODAY).unwrap(), date(2026, 10, 29));
        assert_eq!(parse_date("mo", TODAY).unwrap(), date(2026, 10, 26));
        assert_eq!(parse_date("so", TODAY).unwrap(), date(2026, 10, 25));
    }

    #[test]
    fn german_format() {
        assert_eq!(parse_date("24.10.", TODAY).unwrap(), date(2026, 10, 24));
        assert_eq!(parse_date("1.3.", TODAY).unwrap(), date(2027, 3, 1));
        assert_eq!(parse_date("24.12.2027", TODAY).unwrap(), date(2027, 12, 24));
        assert_eq!(parse_date("24.12.27", TODAY).unwrap(), date(2027, 12, 24));
    }

    #[test]
    fn errors() {
        for bad in [
            "",
            "gestern",
            "+3x",
            "+d",
            "32.10.",
            "2026-13-01",
            "freitag",
            "fr.",
        ] {
            assert!(
                parse_date(bad, TODAY).is_err(),
                "{bad:?} sollte fehlschlagen"
            );
        }
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_due(TODAY, TODAY), "heute");
        assert_eq!(fmt_due(date(2026, 10, 23), TODAY), "morgen");
        assert_eq!(fmt_due(date(2026, 10, 21), TODAY), "gestern");
        assert_eq!(fmt_due(date(2026, 10, 24), TODAY), "sa 24.10.");
        assert_eq!(fmt_due(date(2026, 11, 24), TODAY), "24.11.");
        assert_eq!(fmt_due(date(2027, 1, 5), TODAY), "05.01.27");
    }
}
