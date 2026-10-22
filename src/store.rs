//! Data directory: `shdo.toml`, `tasks/`, `archive/`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::Zoned;

use crate::task::Task;

pub const BOARD_FILE: &str = "shdo.toml";
const ID_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

pub const DEFAULT_BOARD: &str = r#"# Status der Tasks, je einer als Spalte im Board. Der erste gilt für neue Tasks,
# der letzte als „erledigt“. Der key steht in den Task-Dateien,
# der name ist nur die Anzeige und darf jederzeit geändert werden.

[[status]]
key = "todo"
name = "Offen"

[[status]]
key = "doing"
name = "In Arbeit"

[[status]]
key = "wait"
name = "Wartet"

[[status]]
key = "done"
name = "Erledigt"

# Optionale Buckets, z. B. Privat/Arbeit. Jeder Task liegt in höchstens einem.
# Welche Buckets angezeigt werden, wählt jeder Rechner selbst (Taste f).
#
# [[buckets]]
# key = "home"
# name = "Privat"
#
# [[buckets]]
# key = "work"
# name = "Arbeit"
"#;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StatusDef {
    pub key: String,
    pub name: String,
}

/// Buckets have the same shape as statuses.
pub type BucketDef = StatusDef;

#[derive(Debug, Clone)]
pub struct BoardConfig {
    pub statuses: Vec<StatusDef>,
    pub buckets: Vec<BucketDef>,
}

#[derive(Debug, serde::Deserialize)]
struct BoardFile {
    /// `columns` is the former name and is still accepted.
    #[serde(alias = "columns")]
    status: Option<Vec<StatusDef>>,
    #[serde(default)]
    buckets: Vec<BucketDef>,
}

pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.dir.join("tasks")
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.dir.join("archive")
    }

    pub fn ensure_initialized(&self) -> Result<()> {
        if !self.tasks_dir().is_dir() {
            bail!(
                "{} ist kein shdo-Verzeichnis – zuerst `shdo init` ausführen",
                self.dir.display()
            );
        }
        Ok(())
    }

    /// Creates the directory layout. Returns what was newly created.
    pub fn init(&self) -> Result<Vec<String>> {
        let mut created = Vec::new();
        for d in [self.dir.clone(), self.tasks_dir(), self.archive_dir()] {
            if !d.is_dir() {
                fs::create_dir_all(&d).with_context(|| format!("{} anlegen", d.display()))?;
                created.push(format!("{}/", d.display()));
            }
            // Git does not track empty directories.
            let keep = d.join(".gitkeep");
            if d != self.dir && !keep.exists() {
                fs::write(&keep, "")?;
            }
        }
        let board = self.dir.join(BOARD_FILE);
        if !board.exists() {
            fs::write(&board, DEFAULT_BOARD)?;
            created.push(board.display().to_string());
        }
        Ok(created)
    }

    /// Statuses and buckets from `shdo.toml`; default statuses if the file or field is missing.
    pub fn board_config(&self) -> Result<BoardConfig> {
        let path = self.dir.join(BOARD_FILE);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_BOARD.into(),
            Err(e) => return Err(e).with_context(|| format!("{} lesen", path.display())),
        };
        parse_board(&raw).with_context(|| format!("{} ungültig", path.display()))
    }

    fn board_doc(&self) -> Result<toml_edit::DocumentMut> {
        let path = self.dir.join(BOARD_FILE);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_BOARD.into(),
            Err(e) => return Err(e).with_context(|| format!("{} lesen", path.display())),
        };
        raw.parse()
            .with_context(|| format!("{} ungültig", path.display()))
    }

    fn save_board_doc(&self, doc: &toml_edit::DocumentMut) -> Result<()> {
        let raw = doc.to_string();
        parse_board(&raw).context("Änderung würde shdo.toml ungültig machen")?;
        let path = self.dir.join(BOARD_FILE);
        fs::write(&path, raw).with_context(|| format!("{} schreiben", path.display()))
    }

    /// Appends a new bucket to `shdo.toml`; comments are preserved.
    pub fn add_bucket(&self, key: &str, name: &str) -> Result<()> {
        let key = key.trim();
        let name = name.trim();
        if name.is_empty() {
            bail!("Name fehlt");
        }
        validate_bucket_key(key)?;
        let cfg = self.board_config()?;
        if cfg.buckets.iter().any(|b| b.key == key) {
            bail!("Bucket-key „{key}“ gibt es schon");
        }
        let mut doc = self.board_doc()?;
        let mut table = toml_edit::Table::new();
        table["key"] = toml_edit::value(key);
        table["name"] = toml_edit::value(name);
        match doc
            .get_mut("buckets")
            .and_then(|i| i.as_array_of_tables_mut())
        {
            Some(arr) => arr.push(table),
            None => {
                let mut arr = toml_edit::ArrayOfTables::new();
                arr.push(table);
                doc["buckets"] = toml_edit::Item::ArrayOfTables(arr);
            }
        }
        self.save_board_doc(&doc)
    }

    /// Changes a bucket's display name; the key (and thus the tasks) stays.
    pub fn rename_bucket(&self, key: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Name fehlt");
        }
        let mut doc = self.board_doc()?;
        let table = bucket_tables(&mut doc)
            .find(|t| t.get("key").and_then(|k| k.as_str()) == Some(key))
            .with_context(|| format!("Bucket „{key}“ nicht gefunden"))?;
        table["name"] = toml_edit::value(name);
        self.save_board_doc(&doc)
    }

    /// Removes a bucket from `shdo.toml`. The caller decides what happens to its tasks.
    pub fn remove_bucket(&self, key: &str) -> Result<()> {
        let mut doc = self.board_doc()?;
        let arr = doc
            .get_mut("buckets")
            .and_then(|i| i.as_array_of_tables_mut())
            .with_context(|| format!("Bucket „{key}“ nicht gefunden"))?;
        let idx = arr
            .iter()
            .position(|t| t.get("key").and_then(|k| k.as_str()) == Some(key))
            .with_context(|| format!("Bucket „{key}“ nicht gefunden"))?;
        arr.remove(idx);
        if arr.is_empty() {
            doc.remove("buckets");
        }
        self.save_board_doc(&doc)
    }

    /// All tasks in a bucket (excluding the archive).
    pub fn tasks_in_bucket(&self, key: &str) -> Result<Vec<Task>> {
        let (tasks, _) = self.load_tasks()?;
        Ok(tasks
            .into_iter()
            .filter(|t| t.bucket.as_deref() == Some(key))
            .collect())
    }

    /// Loads all tasks from `tasks/`. Invalid files are returned as warnings.
    pub fn load_tasks(&self) -> Result<(Vec<Task>, Vec<String>)> {
        let mut tasks = Vec::new();
        let mut warnings = Vec::new();
        for path in md_files(&self.tasks_dir())? {
            match Task::load(&path) {
                Ok(t) => tasks.push(t),
                Err(e) => warnings.push(format!(
                    "{}: {e:#}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }
        Ok((tasks, warnings))
    }

    fn id_taken(&self, id: &str) -> bool {
        let file = format!("{id}.md");
        self.tasks_dir().join(&file).exists() || self.archive_dir().join(&file).exists()
    }

    pub fn new_id(&self) -> String {
        let mut len = 4;
        loop {
            for _ in 0..50 {
                let id: String = (0..len)
                    .map(|_| ID_CHARS[fastrand::usize(..ID_CHARS.len())] as char)
                    .collect();
                if !self.id_taken(&id) {
                    return id;
                }
            }
            len += 1;
        }
    }

    pub fn create(&self, title: &str, column: &str, now: &Zoned) -> Task {
        let id = self.new_id();
        let path = self.tasks_dir().join(format!("{id}.md"));
        Task::new(&id, path, title, column, now)
    }

    /// Finds a task in `tasks/` by a unique ID prefix.
    pub fn find(&self, prefix: &str) -> Result<Task> {
        let prefix = prefix.trim().to_lowercase();
        if prefix.is_empty() {
            bail!("keine ID angegeben");
        }
        let matches: Vec<PathBuf> = md_files(&self.tasks_dir())?
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with(&prefix))
            })
            .collect();
        match matches.as_slice() {
            [] => bail!("kein Task mit ID „{prefix}“"),
            [one] => Task::load(one),
            many => {
                let ids: Vec<_> = many
                    .iter()
                    .filter_map(|p| p.file_stem()?.to_str())
                    .collect();
                bail!("ID „{prefix}“ ist nicht eindeutig: {}", ids.join(", "))
            }
        }
    }

    pub fn delete(&self, task: &Task) -> Result<()> {
        fs::remove_file(&task.path).with_context(|| format!("{} löschen", task.path.display()))
    }

    pub fn archive(&self, task: &Task) -> Result<()> {
        let target = self.archive_dir().join(format!("{}.md", task.id));
        fs::create_dir_all(self.archive_dir())?;
        fs::rename(&task.path, &target)
            .with_context(|| format!("{} archivieren", task.path.display()))
    }

    /// Fingerprint of `tasks/` and `shdo.toml` to detect external changes.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        let mut stamp = |p: &Path| {
            p.hash(&mut h);
            if let Ok(m) = fs::metadata(p) {
                m.modified().ok().hash(&mut h);
                m.len().hash(&mut h);
            }
        };
        stamp(&self.dir.join(BOARD_FILE));
        if let Ok(files) = md_files(&self.tasks_dir()) {
            for f in files {
                stamp(&f);
            }
        }
        h.finish()
    }
}

pub fn parse_board(raw: &str) -> Result<BoardConfig> {
    let file: BoardFile = toml::from_str(raw)?;
    let statuses = match file.status {
        Some(c) => c,
        None => toml::from_str::<BoardFile>(DEFAULT_BOARD)?
            .status
            .expect("Default hat Status"),
    };
    if statuses.is_empty() {
        bail!("mindestens ein Status nötig");
    }
    validate_keys(&statuses, "Status")?;
    validate_keys(&file.buckets, "Bucket")?;
    if let Some(b) = file
        .buckets
        .iter()
        .find(|b| b.key == crate::board::NO_BUCKET)
    {
        bail!("Bucket-key „{}“ ist reserviert", b.key);
    }
    Ok(BoardConfig {
        statuses,
        buckets: file.buckets,
    })
}

fn validate_keys(defs: &[StatusDef], what: &str) -> Result<()> {
    for (i, c) in defs.iter().enumerate() {
        if c.key.trim().is_empty() {
            bail!("{what} {} hat keinen key", i + 1);
        }
        if defs[..i].iter().any(|o| o.key == c.key) {
            bail!("{what}-key „{}“ ist doppelt", c.key);
        }
    }
    Ok(())
}

/// Finds an entry by key or name (case-insensitive),
/// otherwise by a unique prefix of the key or of a word in the name.
fn bucket_tables(doc: &mut toml_edit::DocumentMut) -> impl Iterator<Item = &mut toml_edit::Table> {
    doc.get_mut("buckets")
        .and_then(|i| i.as_array_of_tables_mut())
        .into_iter()
        .flat_map(|arr| arr.iter_mut())
}

fn validate_bucket_key(key: &str) -> Result<()> {
    if key.is_empty() {
        bail!("key fehlt");
    }
    if key == crate::board::NO_BUCKET {
        bail!("„{key}“ ist reserviert");
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        bail!("key „{key}“: nur a–z, 0–9, - und _ (wird als @{key} getippt)");
    }
    Ok(())
}

/// Suggests a key from a name: "Bücher & Co." → `buecher-co`.
pub fn key_from_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().to_lowercase().chars() {
        match ch {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ if !out.ends_with('-') && !out.is_empty() => out.push('-'),
            _ => {}
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn resolve_def(defs: &[StatusDef], input: &str) -> Option<usize> {
    let q = input.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let exact = defs
        .iter()
        .position(|c| c.key.to_lowercase() == q || c.name.to_lowercase() == q);
    exact.or_else(|| {
        let hits: Vec<usize> = (0..defs.len())
            .filter(|&i| {
                let c = &defs[i];
                c.key.to_lowercase().starts_with(&q)
                    || c.name
                        .to_lowercase()
                        .split_whitespace()
                        .any(|w| w.starts_with(&q))
            })
            .collect();
        (hits.len() == 1).then(|| hits[0])
    })
}

fn md_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("{} lesen", dir.display())),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.is_file())
        .collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::new(tmp.path().join("tasks-repo"));
        s.init().unwrap();
        (tmp, s)
    }

    #[test]
    fn init_is_idempotent_and_has_default_statuses() {
        let (_tmp, s) = store();
        assert!(s.init().unwrap().is_empty());
        let cfg = s.board_config().unwrap();
        assert!(cfg.buckets.is_empty());
        let keys: Vec<_> = cfg.statuses.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["todo", "doing", "wait", "done"]);
    }

    #[test]
    fn board_validation() {
        assert_eq!(parse_board("").unwrap().statuses.len(), 4);
        assert!(parse_board("status = []").is_err());
        assert!(
            parse_board("[[status]]\nkey='a'\nname='A'\n[[status]]\nkey='a'\nname='B'").is_err()
        );
        let cfg = parse_board(
            "[[buckets]]\nkey='home'\nname='Privat'\n[[buckets]]\nkey='work'\nname='Arbeit'",
        )
        .unwrap();
        assert_eq!(cfg.statuses.len(), 4);
        // former name
        let legacy = parse_board("[[columns]]\nkey='a'\nname='A'").unwrap();
        assert_eq!(legacy.statuses[0].key, "a");
        assert_eq!(cfg.buckets.len(), 2);
        assert!(parse_board("[[buckets]]\nkey='-'\nname='x'").is_err());
        assert_eq!(resolve_def(&cfg.buckets, "arb"), Some(1));
        assert_eq!(resolve_def(&cfg.buckets, "HOME"), Some(0));
        assert_eq!(resolve_def(&cfg.buckets, ""), None);
    }

    #[test]
    fn bucket_management_keeps_comments() {
        let (_tmp, s) = store();
        s.add_bucket("home", "Privat").unwrap();
        s.add_bucket("work", "Arbeit").unwrap();
        assert!(s.add_bucket("work", "Doppelt").is_err());
        assert!(s.add_bucket("Gross", "x").is_err());
        assert!(s.add_bucket("-", "x").is_err());
        assert!(s.add_bucket("ok", " ").is_err());
        s.rename_bucket("work", "Job").unwrap();
        let cfg = s.board_config().unwrap();
        let names: Vec<_> = cfg.buckets.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Privat", "Job"]);
        s.remove_bucket("home").unwrap();
        s.remove_bucket("work").unwrap();
        assert!(s.remove_bucket("work").is_err());
        assert!(s.board_config().unwrap().buckets.is_empty());
        let raw = fs::read_to_string(s.dir.join(BOARD_FILE)).unwrap();
        assert!(raw.contains("# Optionale Buckets"), "{raw}");
    }

    #[test]
    fn key_suggestions() {
        assert_eq!(key_from_name("Verein"), "verein");
        assert_eq!(key_from_name("Bücher & Co."), "buecher-co");
        assert_eq!(key_from_name("  Große Straße 2 "), "grosse-strasse-2");
    }

    #[test]
    fn create_find_delete() {
        let (_tmp, s) = store();
        let now = Zoned::now();
        let t = s.create("Eins", "todo", &now);
        t.save().unwrap();
        assert_eq!(t.id.len(), 4);
        assert_eq!(s.find(&t.id[..3]).unwrap().title, "Eins");
        assert!(s.find("zzzzzz").is_err());
        s.delete(&t).unwrap();
        assert!(s.find(&t.id).is_err());
    }

    #[test]
    fn ambiguous_prefix() {
        let (_tmp, s) = store();
        for id in ["ab12", "ab34"] {
            let t = Task::new(
                id,
                s.tasks_dir().join(format!("{id}.md")),
                id,
                "todo",
                &Zoned::now(),
            );
            t.save().unwrap();
        }
        let err = s.find("ab").unwrap_err().to_string();
        assert!(err.contains("nicht eindeutig"), "{err}");
        assert_eq!(s.find("ab3").unwrap().id, "ab34");
    }

    #[test]
    fn ids_are_unique_including_archive() {
        let (_tmp, s) = store();
        let mut seen = std::collections::HashSet::new();
        for i in 0..300 {
            let t = s.create("x", "todo", &Zoned::now());
            assert!(seen.insert(t.id.clone()));
            t.save().unwrap();
            if i % 2 == 0 {
                s.archive(&t).unwrap();
            }
        }
        assert!(!s.id_taken("ungültig"));
    }

    #[test]
    fn invalid_files_become_warnings() {
        let (_tmp, s) = store();
        s.create("ok", "todo", &Zoned::now()).save().unwrap();
        fs::write(s.tasks_dir().join("kaputt.md"), "kein frontmatter").unwrap();
        fs::write(s.tasks_dir().join("notiz.txt"), "ignoriert").unwrap();
        let (tasks, warnings) = s.load_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("kaputt.md"));
    }
}
