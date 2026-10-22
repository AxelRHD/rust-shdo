//! Parsing of date input (`due:`) and localized short date formatting.
//!
//! Input is accepted in German and English regardless of the UI language; output follows
//! the active language (`FR 24.10.` vs. `FRI 10/24`).

use anyhow::Result;
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use rust_i18n::t;

use crate::i18n::{self, Lang};

/// Weekday names accepted as input, German and English.
const WEEKDAY_INPUT: [(&str, Weekday); 14] = [
    ("mo", Weekday::Monday),
    ("di", Weekday::Tuesday),
    ("mi", Weekday::Wednesday),
    ("do", Weekday::Thursday),
    ("fr", Weekday::Friday),
    ("sa", Weekday::Saturday),
    ("so", Weekday::Sunday),
    ("mon", Weekday::Monday),
    ("tue", Weekday::Tuesday),
    ("wed", Weekday::Wednesday),
    ("thu", Weekday::Thursday),
    ("fri", Weekday::Friday),
    ("sat", Weekday::Saturday),
    ("sun", Weekday::Sunday),
];

pub fn today() -> Date {
    jiff::Zoned::now().date()
}

/// Parses a date input relative to `today`.
///
/// Supports `2026-10-24`, `24.10.`, `24.10.2026`, `10/24`, `10/24/2026`, `+3d`, `+2w`,
/// `+1m`, `heute`/`today`, `morgen`/`tomorrow`, `übermorgen` and weekdays
/// (`mo`–`so`, `mon`–`sun`, next occurrence, never today).
pub fn parse_date(input: &str, today: Date) -> Result<Date> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        anyhow::bail!(t!("date.empty"));
    }
    match s.as_str() {
        "heute" | "today" => return Ok(today),
        "morgen" | "tomorrow" => return Ok(today.tomorrow()?),
        "übermorgen" | "uebermorgen" => return Ok(today.checked_add(2.days())?),
        _ => {}
    }
    if let Some((_, wd)) = WEEKDAY_INPUT.iter().find(|(name, _)| *name == s) {
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
    parse_german(&s, today)
        .or_else(|| parse_us(&s, today))
        .ok_or_else(|| invalid(input))
}

fn invalid(input: &str) -> anyhow::Error {
    anyhow::anyhow!(t!("date.invalid", input = input.trim()))
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

/// Without a year: the next occurrence (this year, or next year if already past).
fn day_and_month(day: i8, month: i8, year: Option<i16>, today: Date) -> Option<Date> {
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

/// `24.10.` (next occurrence) or `24.10.2026`.
fn parse_german(s: &str, today: Date) -> Option<Date> {
    let parts: Vec<&str> = s.split('.').collect();
    let (day, month, year) = match parts.as_slice() {
        [d, m] | [d, m, ""] => (d.parse().ok()?, m.parse().ok()?, None),
        [d, m, y] => (d.parse().ok()?, m.parse().ok()?, Some(y.parse().ok()?)),
        _ => return None,
    };
    day_and_month(day, month, year, today)
}

/// `10/24` (next occurrence) or `10/24/2026`.
fn parse_us(s: &str, today: Date) -> Option<Date> {
    let parts: Vec<&str> = s.split('/').collect();
    let (month, day, year) = match parts.as_slice() {
        [m, d] => (m.parse().ok()?, d.parse().ok()?, None),
        [m, d, y] => (m.parse().ok()?, d.parse().ok()?, Some(y.parse().ok()?)),
        _ => return None,
    };
    day_and_month(day, month, year, today)
}

/// Localized weekday abbreviation: `FR` / `FRI`.
pub fn weekday_abbr(wd: Weekday, lang: Lang) -> String {
    let key = match wd {
        Weekday::Monday => "date.wd.mon",
        Weekday::Tuesday => "date.wd.tue",
        Weekday::Wednesday => "date.wd.wed",
        Weekday::Thursday => "date.wd.thu",
        Weekday::Friday => "date.wd.fri",
        Weekday::Saturday => "date.wd.sat",
        Weekday::Sunday => "date.wd.sun",
    };
    t!(key, locale = lang.code()).into_owned()
}

fn days_between(from: Date, to: Date) -> i64 {
    from.until(to).map(|s| s.get_days() as i64).unwrap_or(0)
}

/// `24.10.` / `10/24`, with a two-digit year if not the current year.
fn short(d: Date, today: Date, lang: Lang) -> String {
    let same_year = d.year() == today.year();
    let yy = d.year() % 100;
    match (lang, same_year) {
        (Lang::De, true) => format!("{:02}.{:02}.", d.day(), d.month()),
        (Lang::De, false) => format!("{:02}.{:02}.{yy:02}", d.day(), d.month()),
        (Lang::En, true) => format!("{:02}/{:02}", d.month(), d.day()),
        (Lang::En, false) => format!("{:02}/{:02}/{yy:02}", d.month(), d.day()),
    }
}

/// Day and month only: `03.10.` / `10/03`.
pub fn fmt_day_month(d: Date) -> String {
    match i18n::current() {
        Lang::De => format!("{:02}.{:02}.", d.day(), d.month()),
        Lang::En => format!("{:02}/{:02}", d.month(), d.day()),
    }
}

/// Full date without weekday: `23.10.2026` / `10/23/2026`.
pub fn fmt_date(d: Date) -> String {
    fmt_date_in(d, i18n::current())
}

fn fmt_date_in(d: Date, lang: Lang) -> String {
    match lang {
        Lang::De => format!("{:02}.{:02}.{}", d.day(), d.month(), d.year()),
        Lang::En => format!("{:02}/{:02}/{}", d.month(), d.day(), d.year()),
    }
}

/// Weekday and full date: `FR 23.10.2026` / `FRI 10/23/2026`.
pub fn fmt_long(d: Date) -> String {
    let lang = i18n::current();
    format!(
        "{} {}",
        weekday_abbr(d.weekday(), lang),
        fmt_date_in(d, lang)
    )
}

/// Short form for due dates: today, tomorrow, yesterday, `FR 24.10.` within a week, else the date.
pub fn fmt_due(due: Date, today: Date) -> String {
    fmt_due_in(due, today, i18n::current())
}

fn fmt_due_in(due: Date, today: Date, lang: Lang) -> String {
    let l = lang.code();
    match days_between(today, due) {
        0 => t!("date.today", locale = l).into_owned(),
        1 => t!("date.tomorrow", locale = l).into_owned(),
        -1 => t!("date.yesterday", locale = l).into_owned(),
        2..=6 => format!(
            "{} {}",
            weekday_abbr(due.weekday(), lang),
            short(due, today, lang)
        ),
        _ => short(due, today, lang),
    }
}

pub fn local_date(ts: Timestamp) -> Date {
    ts.to_zoned(TimeZone::system()).date()
}

/// How long a task has been in its status: "since today", "since yesterday", "for 3 days".
pub fn fmt_since(ts: Timestamp, today: Date) -> String {
    fmt_since_in(local_date(ts), today, i18n::current())
}

fn fmt_since_in(d: Date, today: Date, lang: Lang) -> String {
    let l = lang.code();
    match days_between(d, today) {
        i64::MIN..=0 => t!("date.since_today", locale = l).into_owned(),
        1 => t!("date.since_yesterday", locale = l).into_owned(),
        n => t!("date.since_days", locale = l, n = n).into_owned(),
    }
}

/// When something happened: "today", "yesterday", "3 days ago", the date from one week on.
pub fn fmt_ago(ts: Timestamp, today: Date) -> String {
    fmt_ago_in(local_date(ts), today, i18n::current())
}

fn fmt_ago_in(d: Date, today: Date, lang: Lang) -> String {
    let l = lang.code();
    match days_between(d, today) {
        i64::MIN..=0 => t!("date.today", locale = l).into_owned(),
        1 => t!("date.yesterday", locale = l).into_owned(),
        n @ 2..=6 => t!("date.days_ago", locale = l, n = n).into_owned(),
        _ => short(d, today, lang),
    }
}

/// Full local timestamp for the detail view.
pub fn fmt_timestamp(ts: Timestamp) -> String {
    let zoned = ts.to_zoned(TimeZone::system());
    let pattern = match i18n::current() {
        Lang::De => "%d.%m.%Y %H:%M",
        Lang::En => "%m/%d/%Y %-I:%M %p",
    };
    zoned.strftime(pattern).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    // Thursday
    const TODAY: Date = date(2026, 10, 22);

    #[test]
    fn iso_and_keywords() {
        assert_eq!(parse_date("2026-10-24", TODAY).unwrap(), date(2026, 10, 24));
        assert_eq!(parse_date("heute", TODAY).unwrap(), TODAY);
        assert_eq!(parse_date("Today", TODAY).unwrap(), TODAY);
        assert_eq!(parse_date("Morgen", TODAY).unwrap(), date(2026, 10, 23));
        assert_eq!(parse_date("tomorrow", TODAY).unwrap(), date(2026, 10, 23));
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
        assert_eq!(parse_date("Fri", TODAY).unwrap(), date(2026, 10, 23));
        assert_eq!(parse_date("thu", TODAY).unwrap(), date(2026, 10, 29));
    }

    #[test]
    fn day_month_formats() {
        assert_eq!(parse_date("24.10.", TODAY).unwrap(), date(2026, 10, 24));
        assert_eq!(parse_date("1.3.", TODAY).unwrap(), date(2027, 3, 1));
        assert_eq!(parse_date("24.12.2027", TODAY).unwrap(), date(2027, 12, 24));
        assert_eq!(parse_date("24.12.27", TODAY).unwrap(), date(2027, 12, 24));
        assert_eq!(parse_date("10/24", TODAY).unwrap(), date(2026, 10, 24));
        assert_eq!(parse_date("3/1", TODAY).unwrap(), date(2027, 3, 1));
        assert_eq!(parse_date("12/24/2027", TODAY).unwrap(), date(2027, 12, 24));
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
            "13/1",
        ] {
            assert!(parse_date(bad, TODAY).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn formatting_de() {
        let f = |d| fmt_due_in(d, TODAY, Lang::De);
        assert_eq!(f(TODAY), "heute");
        assert_eq!(f(date(2026, 10, 23)), "morgen");
        assert_eq!(f(date(2026, 10, 21)), "gestern");
        assert_eq!(f(date(2026, 10, 24)), "SA 24.10.");
        assert_eq!(f(date(2026, 11, 24)), "24.11.");
        assert_eq!(f(date(2027, 1, 5)), "05.01.27");
        assert_eq!(
            fmt_since_in(date(2026, 10, 19), TODAY, Lang::De),
            "seit 3 Tagen"
        );
        assert_eq!(
            fmt_ago_in(date(2026, 10, 19), TODAY, Lang::De),
            "vor 3 Tagen"
        );
    }

    #[test]
    fn formatting_en() {
        let f = |d| fmt_due_in(d, TODAY, Lang::En);
        assert_eq!(f(TODAY), "today");
        assert_eq!(f(date(2026, 10, 23)), "tomorrow");
        assert_eq!(f(date(2026, 10, 24)), "SAT 10/24");
        assert_eq!(f(date(2027, 1, 5)), "01/05/27");
        assert_eq!(
            fmt_since_in(date(2026, 10, 19), TODAY, Lang::En),
            "for 3 days"
        );
        assert_eq!(fmt_ago_in(date(2026, 10, 21), TODAY, Lang::En), "yesterday");
        assert_eq!(fmt_date_in(date(2026, 10, 3), Lang::En), "10/03/2026");
    }
}
