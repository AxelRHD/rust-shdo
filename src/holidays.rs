//! Gesetzliche Feiertage je Bundesland über das Crate `holiday_de`.
//!
//! Hier nur die Anbindung: Länderkürzel aus der Config, Umrechnung chrono → jiff und
//! Kurznamen, die in die Kalenderleiste passen.

use chrono::Datelike;
use holiday_de::{GermanHoliday, GermanRegion};
use jiff::civil::{Date, date};

pub const DEFAULT_REGION: &str = "bw";

const REGIONS: &[(&str, GermanRegion)] = &[
    ("bw", GermanRegion::BadenWuerttemberg),
    ("by", GermanRegion::Bayern),
    ("be", GermanRegion::Berlin),
    ("bb", GermanRegion::Brandenburg),
    ("hb", GermanRegion::Bremen),
    ("hh", GermanRegion::Hamburg),
    ("he", GermanRegion::Hessen),
    ("mv", GermanRegion::MechlenburgVorpommern),
    ("ni", GermanRegion::Niedersachsen),
    ("nw", GermanRegion::NordrheinWestfalen),
    ("rp", GermanRegion::RheinlandPfalz),
    ("sl", GermanRegion::Saarland),
    ("sn", GermanRegion::Sachsen),
    ("st", GermanRegion::SachsenAnhalt),
    ("sh", GermanRegion::SchleswigHolstein),
    ("th", GermanRegion::Thueringen),
];

#[derive(Clone, Copy)]
pub struct Region(GermanRegion);

impl Region {
    /// Länderkürzel wie `bw`, `by`, `nw` (Groß-/Kleinschreibung egal).
    pub fn from_code(code: &str) -> Option<Region> {
        let code = code.trim().to_lowercase();
        REGIONS
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, r)| Region(*r))
    }

    pub fn codes() -> impl Iterator<Item = &'static str> {
        REGIONS.iter().map(|(c, _)| *c)
    }

    /// Feiertage zwischen `from` und `to` (jeweils einschließlich), nach Datum sortiert.
    pub fn between(self, from: Date, to: Date) -> Vec<(Date, &'static str)> {
        let mut days: Vec<(Date, &'static str)> = (from.year()..=to.year())
            .flat_map(|y| self.0.holiday_dates_in_year(y as i32))
            .filter_map(|(d, h)| {
                let d = date(d.year() as i16, d.month() as i8, d.day() as i8);
                (d >= from && d <= to).then_some((d, short_name(h)))
            })
            .collect();
        days.sort();
        days
    }
}

impl Default for Region {
    fn default() -> Region {
        Region::from_code(DEFAULT_REGION).expect("Default-Region existiert")
    }
}

/// Kurzname für die Leiste (Datum + Name in 24 Zeichen).
fn short_name(h: GermanHoliday) -> &'static str {
    use GermanHoliday::*;
    match h {
        HeiligeDreiKoenige => "Hl. Drei Könige",
        ErsterMai => "Tag der Arbeit",
        ChristiHimmelfahrt => "Himmelfahrt",
        TagDerDeutschenEinheit => "Dt. Einheit",
        MariaeHimmelfahrt => "Mariä Himmelf.",
        TagDerBefreiung => "Tag d. Befreiung",
        BussUndBettag => "Buß- u. Bettag",
        ErsterWeihnachtsfeiertag => "1. Weihnachtstag",
        ZweiterWeihnachtsfeiertag => "2. Weihnachtstag",
        AugsburgerFriedensfest => "Friedensfest",
        other => other.description(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bw_2026() {
        let bw = Region::default();
        let days = bw.between(date(2026, 1, 1), date(2026, 12, 31));
        assert_eq!(days.len(), 12);
        let get = |name: &str| days.iter().find(|(_, n)| *n == name).unwrap().0;
        assert_eq!(get("Karfreitag"), date(2026, 4, 3));
        assert_eq!(get("Ostermontag"), date(2026, 4, 6));
        assert_eq!(get("Himmelfahrt"), date(2026, 5, 14));
        assert_eq!(get("Pfingstmontag"), date(2026, 5, 25));
        assert_eq!(get("Fronleichnam"), date(2026, 6, 4));
        assert_eq!(get("Hl. Drei Könige"), date(2026, 1, 6));
        assert_eq!(get("Allerheiligen"), date(2026, 11, 1));
        assert!(days.iter().all(|(_, n)| n.chars().count() <= 16));
    }

    #[test]
    fn range_across_years() {
        let days = Region::default().between(date(2026, 12, 1), date(2027, 1, 31));
        let names: Vec<&str> = days.iter().map(|(_, n)| *n).collect();
        assert_eq!(
            names,
            [
                "1. Weihnachtstag",
                "2. Weihnachtstag",
                "Neujahr",
                "Hl. Drei Könige"
            ]
        );
    }

    #[test]
    fn regions() {
        assert!(Region::from_code("BY").is_some());
        assert!(Region::from_code("xx").is_none());
        assert_eq!(Region::codes().count(), 16);
        // Sachsen hat den Buß- und Bettag (Mittwoch vor dem 23.11.)
        let sn = Region::from_code("sn").unwrap();
        let nov = sn.between(date(2026, 11, 1), date(2026, 11, 30));
        assert_eq!(nov, [(date(2026, 11, 18), "Buß- u. Bettag")]);
    }
}
