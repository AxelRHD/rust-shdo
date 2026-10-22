//! Task model: a Markdown file with TOML frontmatter between `+++` lines.
//!
//! The frontmatter is kept as a `toml_edit` document so comments, formatting
//! and unknown fields survive writes. The body is kept as is.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use jiff::civil::Date;
use jiff::{Timestamp, Zoned};
use toml_edit::{DocumentMut, Item, value};

pub const DELIM: &str = "+++";
pub const PRIO_DEFAULT: u8 = 2;
const LEGACY_STATUS: &str = "column";

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub path: PathBuf,
    doc: DocumentMut,
    pub body: String,
    pub title: String,
    /// Status key; empty if not set in the frontmatter.
    pub status: String,
    pub bucket: Option<String>,
    pub prio: Option<u8>,
    pub due: Option<Date>,
    pub created: Option<Timestamp>,
    pub moved: Option<Timestamp>,
    pub done: Option<Timestamp>,
    /// File modification time, used when `done` is missing.
    pub mtime: Option<Timestamp>,
}

impl Task {
    pub fn new(id: &str, path: PathBuf, title: &str, status: &str, now: &Zoned) -> Task {
        let mut doc = DocumentMut::new();
        doc["title"] = value(title);
        doc["status"] = value(status);
        doc["created"] = datetime_item(now);
        doc["moved"] = datetime_item(now);
        let ts = now.timestamp();
        Task {
            id: id.into(),
            path,
            doc,
            body: String::new(),
            title: title.into(),
            status: status.into(),
            prio: None,
            bucket: None,
            due: None,
            created: Some(ts),
            moved: Some(ts),
            done: None,
            mtime: None,
        }
    }

    pub fn load(path: &Path) -> Result<Task> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("{} nicht lesbar", path.display()))?;
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("ungültiger Dateiname {}", path.display()))?
            .to_string();
        let mut task = Task::parse(&id, path.to_path_buf(), &raw)?;
        task.mtime = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| Timestamp::try_from(t).ok());
        Ok(task)
    }

    pub fn parse(id: &str, path: PathBuf, raw: &str) -> Result<Task> {
        let (front, body) = split_frontmatter(raw)?;
        let doc: DocumentMut = front.parse().context("ungültiges TOML im Frontmatter")?;
        let title = doc
            .get("title")
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| anyhow!("Feld „title“ fehlt"))?
            .to_string();
        // `column` is the field's former name and is still accepted.
        let status = doc
            .get("status")
            .or_else(|| doc.get(LEGACY_STATUS))
            .and_then(Item::as_str)
            .unwrap_or("")
            .to_string();
        let prio = doc
            .get("prio")
            .and_then(Item::as_integer)
            .map(|p| p.clamp(1, 3) as u8);
        Ok(Task {
            id: id.into(),
            path,
            title,
            status,
            bucket: doc
                .get("bucket")
                .and_then(Item::as_str)
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map(String::from),
            prio,
            due: get_date(&doc, "due")?,
            created: get_timestamp(&doc, "created")?,
            moved: get_timestamp(&doc, "moved")?,
            done: get_timestamp(&doc, "done")?,
            mtime: None,
            body: body.to_string(),
            doc,
        })
    }

    pub fn render(&self) -> String {
        let mut front = self.doc.to_string();
        if !front.ends_with('\n') {
            front.push('\n');
        }
        format!("{DELIM}\n{front}{DELIM}\n{}", self.body)
    }

    pub fn save(&self) -> Result<()> {
        // Write to a temporary file first, then rename: never a half-written file.
        let tmp = self.path.with_extension("md.tmp");
        fs::write(&tmp, self.render())
            .with_context(|| format!("{} nicht schreibbar", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("{} nicht schreibbar", self.path.display()))?;
        Ok(())
    }

    pub fn prio_or_default(&self) -> u8 {
        self.prio.unwrap_or(PRIO_DEFAULT)
    }

    pub fn set_bucket(&mut self, bucket: Option<&str>) {
        match bucket {
            Some(b) => self.doc["bucket"] = value(b),
            None => {
                self.doc.remove("bucket");
            }
        }
        self.bucket = bucket.map(String::from);
    }

    /// Sets a single-line note (form). Multi-line notes only via the editor.
    pub fn set_note(&mut self, note: &str) {
        let note = note.trim();
        self.body = if note.is_empty() {
            String::new()
        } else {
            format!("\n{note}\n")
        };
    }

    /// Can the note be edited in the single-line form field?
    pub fn note_is_single_line(&self) -> bool {
        !self.note().contains('\n')
    }

    pub fn set_title(&mut self, title: &str) {
        self.doc["title"] = value(title);
        self.title = title.into();
    }

    pub fn set_prio(&mut self, prio: Option<u8>) {
        match prio {
            Some(p) => self.doc["prio"] = value(p as i64),
            None => {
                self.doc.remove("prio");
            }
        }
        self.prio = prio;
    }

    pub fn set_due(&mut self, due: Option<Date>) {
        match due {
            Some(d) => {
                let dt: toml_edit::Datetime =
                    d.to_string().parse().expect("ISO-Datum ist gültiges TOML");
                self.doc["due"] = value(dt);
            }
            None => {
                self.doc.remove("due");
            }
        }
        self.due = due;
    }

    /// Sets a new status and maintains `moved` and `done`.
    pub fn move_to(&mut self, status: &str, is_done: bool, now: &Zoned) {
        if self.status == status {
            return;
        }
        self.doc.remove(LEGACY_STATUS);
        self.doc["status"] = value(status);
        self.status = status.into();
        self.doc["moved"] = datetime_item(now);
        self.moved = Some(now.timestamp());
        if is_done {
            self.doc["done"] = datetime_item(now);
            self.done = Some(now.timestamp());
        } else {
            self.doc.remove("done");
            self.done = None;
        }
    }

    /// (checked, total) Markdown checkboxes in the body.
    pub fn checklist(&self) -> Option<(usize, usize)> {
        let mut done = 0;
        let mut total = 0;
        for line in self.body.lines() {
            let l = line.trim_start();
            let rest = l.strip_prefix("- ").or_else(|| l.strip_prefix("* "));
            match rest.map(|r| r.get(..3)) {
                Some(Some("[ ]")) => total += 1,
                Some(Some("[x]" | "[X]")) => {
                    total += 1;
                    done += 1;
                }
                _ => {}
            }
        }
        (total > 0).then_some((done, total))
    }

    pub fn note(&self) -> &str {
        self.body.trim()
    }
}

fn datetime_item(now: &Zoned) -> Item {
    let s = now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
    let dt: toml_edit::Datetime = s.parse().expect("RFC 3339 ist gültiges TOML");
    value(dt)
}

/// Accepts TOML date values and, for hand-edited files, strings too.
fn raw_datetime(doc: &DocumentMut, key: &str) -> Option<String> {
    let v = doc.get(key)?;
    v.as_datetime()
        .map(|d| d.to_string())
        .or_else(|| v.as_str().map(str::to_string))
}

fn get_date(doc: &DocumentMut, key: &str) -> Result<Option<Date>> {
    let Some(s) = raw_datetime(doc, key) else {
        return Ok(None);
    };
    let date_part = s.get(..10).unwrap_or(&s);
    date_part
        .parse::<Date>()
        .map(Some)
        .map_err(|_| anyhow!("Feld „{key}“: ungültiges Datum {s:?}"))
}

fn get_timestamp(doc: &DocumentMut, key: &str) -> Result<Option<Timestamp>> {
    let Some(s) = raw_datetime(doc, key) else {
        return Ok(None);
    };
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(Some(ts));
    }
    // No offset (local time) or date only: interpret in the system time zone.
    let tz = jiff::tz::TimeZone::system();
    if let Ok(dt) = s.parse::<jiff::civil::DateTime>() {
        return Ok(dt.to_zoned(tz).ok().map(|z| z.timestamp()));
    }
    if let Ok(d) = s.parse::<Date>() {
        return Ok(d.to_zoned(tz).ok().map(|z| z.timestamp()));
    }
    bail!("Feld „{key}“: ungültiger Zeitstempel {s:?}")
}

fn split_frontmatter(raw: &str) -> Result<(&str, &str)> {
    let rest = raw
        .strip_prefix("\u{feff}")
        .unwrap_or(raw)
        .strip_prefix(DELIM)
        .and_then(|r| r.strip_prefix("\r\n").or_else(|| r.strip_prefix('\n')))
        .ok_or_else(|| anyhow!("Datei beginnt nicht mit „{DELIM}“"))?;
    let mut pos = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == DELIM {
            return Ok((&rest[..pos], &rest[pos + line.len()..]));
        }
        pos += line.len();
    }
    bail!("schließendes „{DELIM}“ fehlt")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "+++
# Kommentar bleibt
title = \"Rechnung bezahlen\"
status = \"todo\"
prio = 1
due = 2026-10-24
created = 2026-10-22T10:41:00+02:00
custom = \"unbekannt\"
+++

Kundennr. 4711

- [x] raussuchen
- [ ] überweisen
  * [X] eingerückt
";

    fn now() -> Zoned {
        "2026-10-23T09:00:00+02:00[Europe/Berlin]".parse().unwrap()
    }

    fn sample() -> Task {
        Task::parse("a3f9", PathBuf::from("a3f9.md"), SAMPLE).unwrap()
    }

    #[test]
    fn parses_fields() {
        let t = sample();
        assert_eq!(t.title, "Rechnung bezahlen");
        assert_eq!(t.status, "todo");
        assert_eq!(t.prio, Some(1));
        assert_eq!(t.due, Some(jiff::civil::date(2026, 10, 24)));
        assert_eq!(t.created.unwrap().to_string(), "2026-10-22T08:41:00Z");
        assert_eq!(t.checklist(), Some((2, 3)));
        assert!(t.note().starts_with("Kundennr. 4711"));
    }

    #[test]
    fn roundtrip_is_lossless() {
        assert_eq!(sample().render(), SAMPLE);
    }

    #[test]
    fn edits_keep_comments_and_unknown_fields() {
        let mut t = sample();
        t.set_title("Neu");
        t.set_prio(None);
        t.set_due(Some(jiff::civil::date(2026, 11, 1)));
        let out = t.render();
        assert!(out.contains("# Kommentar bleibt"));
        assert!(out.contains("custom = \"unbekannt\""));
        assert!(out.contains("title = \"Neu\""));
        assert!(out.contains("due = 2026-11-01\n"));
        assert!(!out.contains("prio"));
        assert!(out.ends_with("  * [X] eingerückt\n"));
        let back = Task::parse("a3f9", PathBuf::from("a3f9.md"), &out).unwrap();
        assert_eq!(back.title, "Neu");
        assert_eq!(back.prio, None);
    }

    #[test]
    fn bucket_and_note() {
        let mut t = sample();
        assert_eq!(t.bucket, None);
        assert!(!t.note_is_single_line());
        t.set_bucket(Some("work"));
        t.set_note("  kurz  ");
        let back = Task::parse("a3f9", PathBuf::from("a3f9.md"), &t.render()).unwrap();
        assert_eq!(back.bucket.as_deref(), Some("work"));
        assert_eq!(back.note(), "kurz");
        assert!(back.note_is_single_line());
        t.set_bucket(None);
        t.set_note("");
        assert!(!t.render().contains("bucket"));
        assert!(t.render().ends_with("+++\n"));
    }

    #[test]
    fn move_sets_and_clears_done() {
        let mut t = sample();
        t.move_to("done", true, &now());
        assert_eq!(t.done, Some(now().timestamp()));
        assert!(t.render().contains("done = 2026-10-23T09:00:00+02:00"));
        t.move_to("doing", false, &now());
        assert_eq!(t.done, None);
        assert!(!t.render().contains("done ="));
        assert!(t.render().contains("moved = 2026-10-23T09:00:00+02:00"));
    }

    #[test]
    fn legacy_column_field_is_read_and_migrated() {
        let raw = "+++\ntitle = \"alt\"\ncolumn = \"doing\"\n+++\n";
        let mut t = Task::parse("x", PathBuf::new(), raw).unwrap();
        assert_eq!(t.status, "doing");
        t.move_to("todo", false, &now());
        let out = t.render();
        assert!(out.contains("status = \"todo\""));
        assert!(!out.contains("column"));
    }

    #[test]
    fn new_task_renders_and_parses() {
        let t = Task::new(
            "b71c",
            PathBuf::from("b71c.md"),
            "Test \"quotes\"",
            "todo",
            &now(),
        );
        let back = Task::parse("b71c", PathBuf::from("b71c.md"), &t.render()).unwrap();
        assert_eq!(back.title, "Test \"quotes\"");
        assert_eq!(back.created, Some(now().timestamp()));
        assert_eq!(back.checklist(), None);
    }

    #[test]
    fn lenient_values() {
        let raw = "+++\ntitle = \"x\"\nprio = 9\ndue = \"2026-10-24\"\ncreated = 2026-10-22\n+++\n";
        let t = Task::parse("x", PathBuf::new(), raw).unwrap();
        assert_eq!(t.prio, Some(3));
        assert_eq!(t.due, Some(jiff::civil::date(2026, 10, 24)));
        assert!(t.created.is_some());
        assert_eq!(t.status, "");
    }

    #[test]
    fn invalid_files() {
        for raw in [
            "title = \"x\"",
            "+++\ntitle = \"x\"\n",
            "+++\ntitle = \n+++\n",
            "+++\nstatus = \"todo\"\n+++\n",
            "+++\ntitle = \"x\"\ndue = \"bald\"\n+++\n",
        ] {
            assert!(Task::parse("x", PathBuf::new(), raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn crlf_frontmatter() {
        let t = Task::parse("x", PathBuf::new(), "+++\r\ntitle = \"x\"\r\n+++\r\nbody").unwrap();
        assert_eq!(t.title, "x");
        assert_eq!(t.body, "body");
    }
}
