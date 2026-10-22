//! Public holidays per German federal state via the `holiday_de` crate.
//!
//! Only the glue lives here: the `holidays = "de-bw"` setting, chrono → jiff conversion
//! and localized short names that fit into the calendar sidebar. Other countries can be
//! added behind the same [`Region`] type later.

use chrono::Datelike;
use holiday_de::{GermanHoliday, GermanRegion};
use jiff::civil::{Date, date};
use rust_i18n::t;

use crate::i18n::{self, Lang};

/// German federal states: (code, region, name).
pub const GERMAN_STATES: &[(&str, GermanRegion, &str)] = &[
    ("bw", GermanRegion::BadenWuerttemberg, "Baden-Württemberg"),
    ("by", GermanRegion::Bayern, "Bayern"),
    ("be", GermanRegion::Berlin, "Berlin"),
    ("bb", GermanRegion::Brandenburg, "Brandenburg"),
    ("hb", GermanRegion::Bremen, "Bremen"),
    ("hh", GermanRegion::Hamburg, "Hamburg"),
    ("he", GermanRegion::Hessen, "Hessen"),
    (
        "mv",
        GermanRegion::MechlenburgVorpommern,
        "Mecklenburg-Vorpommern",
    ),
    ("ni", GermanRegion::Niedersachsen, "Niedersachsen"),
    (
        "nw",
        GermanRegion::NordrheinWestfalen,
        "Nordrhein-Westfalen",
    ),
    ("rp", GermanRegion::RheinlandPfalz, "Rheinland-Pfalz"),
    ("sl", GermanRegion::Saarland, "Saarland"),
    ("sn", GermanRegion::Sachsen, "Sachsen"),
    ("st", GermanRegion::SachsenAnhalt, "Sachsen-Anhalt"),
    ("sh", GermanRegion::SchleswigHolstein, "Schleswig-Holstein"),
    ("th", GermanRegion::Thueringen, "Thüringen"),
];

#[derive(Clone, Copy, Debug)]
pub struct Region(GermanRegion);

impl Region {
    /// Parses the `holidays` setting: `de-bw` (also `bw` for compatibility).
    /// An empty value means no holidays and yields `Ok(None)`.
    pub fn parse_setting(value: &str) -> Result<Option<Region>, String> {
        let v = value.trim().to_lowercase();
        if v.is_empty() {
            return Ok(None);
        }
        let state = v.strip_prefix("de-").unwrap_or(&v);
        GERMAN_STATES
            .iter()
            .find(|(c, _, _)| *c == state)
            .map(|(_, r, _)| Some(Region(*r)))
            .ok_or_else(|| value.to_string())
    }

    /// Setting value for a German state code, e.g. `de-bw`.
    #[allow(dead_code)] // used by the upcoming setup wizard
    pub fn setting_for_state(code: &str) -> String {
        format!("de-{code}")
    }

    /// Holidays between `from` and `to` (both inclusive), sorted by date, named in the
    /// active language.
    pub fn between(self, from: Date, to: Date) -> Vec<(Date, String)> {
        self.between_in(from, to, i18n::current())
    }

    fn between_in(self, from: Date, to: Date, lang: Lang) -> Vec<(Date, String)> {
        let mut days: Vec<(Date, String)> = (from.year()..=to.year())
            .flat_map(|y| self.0.holiday_dates_in_year(y as i32))
            .filter_map(|(d, h)| {
                let d = date(d.year() as i16, d.month() as i8, d.day() as i8);
                (d >= from && d <= to).then(|| (d, short_name(h, lang)))
            })
            .collect();
        days.sort();
        days
    }
}

/// Short, localized name for the sidebar (date + name within 24 characters).
fn short_name(h: GermanHoliday, lang: Lang) -> String {
    let key = format!("holiday.{h:?}");
    t!(key.as_str(), locale = lang.code()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bw() -> Region {
        Region::parse_setting("de-bw").unwrap().unwrap()
    }

    #[test]
    fn bw_2026() {
        let days = bw().between_in(date(2026, 1, 1), date(2026, 12, 31), Lang::De);
        assert_eq!(days.len(), 12);
        let get = |name: &str| days.iter().find(|(_, n)| n == name).unwrap().0;
        assert_eq!(get("Karfreitag"), date(2026, 4, 3));
        assert_eq!(get("Ostermontag"), date(2026, 4, 6));
        assert_eq!(get("Himmelfahrt"), date(2026, 5, 14));
        assert_eq!(get("Pfingstmontag"), date(2026, 5, 25));
        assert_eq!(get("Fronleichnam"), date(2026, 6, 4));
        assert_eq!(get("Hl. Drei Könige"), date(2026, 1, 6));
        assert_eq!(get("Allerheiligen"), date(2026, 11, 1));
    }

    #[test]
    fn names_fit_the_sidebar() {
        for lang in Lang::ALL {
            for y in [2026, 2027] {
                for (code, _, _) in GERMAN_STATES {
                    let r = Region::parse_setting(code).unwrap().unwrap();
                    for (_, name) in r.between_in(date(y, 1, 1), date(y, 12, 31), lang) {
                        assert!(name.chars().count() <= 16, "{name:?} ({lang:?})");
                        assert!(!name.starts_with("holiday."), "missing translation {name}");
                    }
                }
            }
        }
    }

    #[test]
    fn range_across_years_in_english() {
        let days = bw().between_in(date(2026, 12, 1), date(2027, 1, 31), Lang::En);
        let names: Vec<&str> = days.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            ["Christmas Day", "Boxing Day", "New Year's Day", "Epiphany"]
        );
    }

    #[test]
    fn settings() {
        assert!(Region::parse_setting("").unwrap().is_none());
        assert!(Region::parse_setting("de-BY").unwrap().is_some());
        assert!(Region::parse_setting("bw").unwrap().is_some());
        assert_eq!(Region::parse_setting("fr").unwrap_err(), "fr");
        assert_eq!(Region::setting_for_state("nw"), "de-nw");
        // Saxony has Buß- und Bettag (the Wednesday before Nov 23)
        let sn = Region::parse_setting("de-sn").unwrap().unwrap();
        let nov = sn.between_in(date(2026, 11, 1), date(2026, 11, 30), Lang::De);
        assert_eq!(nov, [(date(2026, 11, 18), "Buß- u. Bettag".to_string())]);
    }
}
