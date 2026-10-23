//! Grouping tasks into columns, bucket filter and sorting.

use std::cmp::Ordering;

use anyhow::Result;
use jiff::Zoned;
use rust_i18n::t;

use crate::store::{BoardConfig, BucketDef, StatusDef, Store, resolve_def};
use crate::task::Task;

/// Filter entry for tasks without a (valid) bucket.
pub const NO_BUCKET: &str = "-";

/// Which tasks are shown: selected buckets (empty = all) and an optional search text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub buckets: Vec<String>,
    /// Case-insensitive search in title, note, tags and ID; empty = no search.
    pub query: String,
    /// Shown tags: a task matches if it has any of them. Empty = no tag filter.
    pub tags: Vec<String>,
    /// Done tasks older than this many days are not shown; `None` = show all.
    pub done_days: Option<u32>,
}

impl Filter {
    pub fn all() -> Filter {
        Filter::default()
    }

    pub fn is_all(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Unknown entries are dropped; if nothing is left or everything is selected, it means "all".
    pub fn normalized(mut self, defs: &[BucketDef]) -> Filter {
        self.buckets
            .retain(|b| b == NO_BUCKET || defs.iter().any(|d| &d.key == b));
        self.buckets.dedup();
        if defs.is_empty() || self.buckets.len() == defs.len() + 1 {
            self.buckets.clear();
        }
        self
    }

    fn matches(&self, bucket: &str) -> bool {
        self.is_all() || self.buckets.iter().any(|b| b == bucket)
    }

    /// Every word of the query must appear in the title, the note or the ID.
    fn matches_query(&self, task: &Task) -> bool {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let tags: Vec<String> = task.tags.iter().map(|t| format!("+{t}")).collect();
        let haystack = format!(
            "{}\n{}\n{}\n{}",
            task.title,
            task.note(),
            task.id,
            tags.join(" ")
        )
        .to_lowercase();
        query.split_whitespace().all(|word| haystack.contains(word))
    }

    fn matches_tags(&self, task: &Task) -> bool {
        self.tags.is_empty() || task.tags.iter().any(|t| self.tags.contains(t))
    }

    /// If exactly one real bucket is selected, new tasks inherit it. "No bucket" does not
    /// count: showing work plus unassigned tasks still means new tasks belong to work.
    pub fn inherited(&self) -> Option<&str> {
        let mut real = self.buckets.iter().filter(|b| *b != NO_BUCKET);
        match (real.next(), real.next()) {
            (Some(one), None) => Some(one),
            _ => None,
        }
    }

    /// Only a single bucket is visible, so showing `@bucket` on every card adds nothing.
    pub fn shows_single_bucket(&self) -> bool {
        matches!(self.buckets.as_slice(), [one] if one != NO_BUCKET)
    }
}

pub struct Column {
    pub def: StatusDef,
    pub tasks: Vec<Task>,
}

pub struct Board {
    pub columns: Vec<Column>,
    pub buckets: Vec<BucketDef>,
    pub filter: Filter,
    /// Open tasks per bucket (order as in `buckets`, then "none"), unfiltered.
    pub bucket_counts: Vec<usize>,
    /// All tags in use with the number of open tasks carrying them, sorted by name.
    pub tag_counts: Vec<(String, usize)>,
    /// Open tasks hidden by the filter.
    pub hidden: usize,
    /// Done tasks (matching the filter) hidden because they are older than `done_days`.
    pub old_done: usize,
    pub warnings: Vec<String>,
}

impl Board {
    pub fn load(store: &Store, filter: &Filter) -> Result<Board> {
        let cfg = store.board_config()?;
        let (tasks, warnings) = store.load_tasks()?;
        Ok(Board::build(cfg, tasks, warnings, filter.clone()))
    }

    /// Tasks with an unknown or missing status land in the first column, tasks with
    /// an unknown bucket count as "no bucket" (both with a warning).
    pub fn build(
        cfg: BoardConfig,
        tasks: Vec<Task>,
        mut warnings: Vec<String>,
        filter: Filter,
    ) -> Board {
        let filter = filter.normalized(&cfg.buckets);
        let today = crate::dates::today();
        let mut old_done = 0;
        let mut columns: Vec<Column> = cfg
            .statuses
            .into_iter()
            .map(|def| Column {
                def,
                tasks: Vec::new(),
            })
            .collect();
        let last = columns.len() - 1;
        let mut bucket_counts = vec![0; cfg.buckets.len() + 1];
        let mut tag_counts: std::collections::BTreeMap<String, usize> = Default::default();
        let mut hidden = 0;
        for task in tasks {
            let idx = match columns.iter().position(|c| c.def.key == task.status) {
                Some(i) => i,
                None => {
                    if task.status.is_empty() {
                        warnings.push(t!("warn.no_status", id = task.id).into_owned());
                    } else {
                        warnings.push(
                            t!("warn.unknown_status", id = task.id, status = task.status)
                                .into_owned(),
                        );
                    }
                    0
                }
            };
            let bucket_idx = match &task.bucket {
                None => None,
                Some(b) => {
                    let i = cfg.buckets.iter().position(|d| &d.key == b);
                    if i.is_none() {
                        warnings
                            .push(t!("warn.unknown_bucket", id = task.id, bucket = b).into_owned());
                    }
                    i
                }
            };
            let key = bucket_idx.map_or(NO_BUCKET, |i| cfg.buckets[i].key.as_str());
            for tag in &task.tags {
                *tag_counts.entry(tag.clone()).or_default() += usize::from(idx != last);
            }
            if idx != last {
                bucket_counts[bucket_idx.unwrap_or(cfg.buckets.len())] += 1;
            }
            if !filter.matches(key) || !filter.matches_tags(&task) || !filter.matches_query(&task) {
                if idx != last {
                    hidden += 1;
                }
                continue;
            }
            if idx == last && is_old_done(&task, filter.done_days, today) {
                old_done += 1;
                continue;
            }
            columns[idx].tasks.push(task);
        }
        for (i, col) in columns.iter_mut().enumerate() {
            sort_tasks(&mut col.tasks, i == last);
        }
        Board {
            columns,
            buckets: cfg.buckets,
            filter,
            bucket_counts,
            tag_counts: tag_counts.into_iter().collect(),
            hidden,
            old_done,
            warnings,
        }
    }

    pub fn last_index(&self) -> usize {
        self.columns.len() - 1
    }

    pub fn open_count(&self) -> usize {
        self.columns[..self.last_index()]
            .iter()
            .map(|c| c.tasks.len())
            .sum()
    }

    pub fn overdue_count(&self, today: jiff::civil::Date) -> usize {
        self.columns[..self.last_index()]
            .iter()
            .flat_map(|c| &c.tasks)
            .filter(|t| t.due.is_some_and(|d| d < today))
            .count()
    }

    pub fn resolve_status(&self, input: &str) -> Option<usize> {
        let defs: Vec<StatusDef> = self.columns.iter().map(|c| c.def.clone()).collect();
        resolve_def(&defs, input)
    }

    pub fn bucket_name<'a>(&'a self, key: &'a str) -> &'a str {
        self.buckets
            .iter()
            .find(|b| b.key == key)
            .map_or(key, |b| b.name.as_str())
    }

    /// Short description of the tag filter, e.g. "+dev +ops".
    pub fn tag_label(&self) -> Option<String> {
        if self.filter.tags.is_empty() {
            return None;
        }
        let tags: Vec<String> = self.filter.tags.iter().map(|t| format!("+{t}")).collect();
        Some(tags.join(" "))
    }

    /// Short description of the filter, e.g. "Work" or "Home, no bucket".
    pub fn filter_label(&self) -> Option<String> {
        if self.filter.is_all() {
            return None;
        }
        let names: Vec<String> = self
            .filter
            .buckets
            .iter()
            .map(|b| {
                if b == NO_BUCKET {
                    t!("bucket.none").into_owned()
                } else {
                    self.bucket_name(b).to_string()
                }
            })
            .collect();
        Some(names.join(", "))
    }

    pub fn find(&self, id: &str) -> Option<(usize, usize)> {
        self.columns
            .iter()
            .enumerate()
            .find_map(|(ci, c)| c.tasks.iter().position(|t| t.id == id).map(|ri| (ci, ri)))
    }
}

/// Where a new task's bucket came from, so the UI can say so.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BucketOrigin {
    /// `@bucket` in the title
    Explicit,
    /// exactly one bucket is shown in the filter
    Filter,
    /// `default_bucket` in config.toml
    Default,
}

/// A done task whose completion (or, without `done`, the file's mtime) lies more than
/// `days` days before `today`. `None` never counts as old.
pub fn is_old_done(task: &Task, days: Option<u32>, today: jiff::civil::Date) -> bool {
    let (Some(days), Some(at)) = (days, task.done.or(task.mtime)) else {
        return false;
    };
    let done_on = crate::dates::local_date(at);
    done_on
        .until(today)
        .map(|span| span.get_days() as i64 > i64::from(days))
        .unwrap_or(false)
}

/// Archives done tasks older than `days` (`None` = all done tasks), across all buckets
/// and tags. Returns the done column's name and the number archived.
pub fn archive_done(store: &Store, days: Option<u32>) -> Result<(String, usize)> {
    let board = Board::load(store, &Filter::all())?;
    let done = &board.columns[board.last_index()];
    let today = crate::dates::today();
    let mut n = 0;
    for task in &done.tasks {
        if days.is_none() || is_old_done(task, days, today) {
            store.archive(task)?;
            n += 1;
        }
    }
    Ok((done.def.name.clone(), n))
}

/// How many done tasks `archive_done` would archive.
pub fn count_archivable(board_all: &Board, days: Option<u32>) -> usize {
    let today = crate::dates::today();
    board_all.columns[board_all.last_index()]
        .tasks
        .iter()
        .filter(|t| days.is_none() || is_old_done(t, days, today))
        .count()
}

/// Bucket for a new task: `@…` in the title, else the filter (exactly one bucket shown),
/// else the per-machine default. An unknown default bucket is ignored.
pub fn bucket_for_new(
    explicit: Option<&str>,
    filter: &Filter,
    buckets: &[BucketDef],
    default: Option<&str>,
) -> Result<Option<(String, BucketOrigin)>> {
    if let Some(raw) = explicit {
        return Ok(Some((
            resolve_bucket(buckets, raw)?,
            BucketOrigin::Explicit,
        )));
    }
    if let Some(b) = filter.inherited() {
        return Ok(Some((b.to_string(), BucketOrigin::Filter)));
    }
    Ok(default
        .filter(|d| buckets.iter().any(|b| b.key == *d))
        .map(|d| (d.to_string(), BucketOrigin::Default)))
}

pub fn resolve_bucket(buckets: &[BucketDef], raw: &str) -> Result<String> {
    let raw = raw.trim_start_matches('@');
    if buckets.is_empty() {
        anyhow::bail!(t!("bucket.none_defined"));
    }
    match resolve_def(buckets, raw) {
        Some(i) => Ok(buckets[i].key.clone()),
        None => {
            let keys: Vec<_> = buckets.iter().map(|b| b.key.as_str()).collect();
            anyhow::bail!(t!("bucket.unknown", bucket = raw, keys = keys.join(", ")))
        }
    }
}

/// Sets status `target` (index) and saves the task.
pub fn move_task(
    task: &mut Task,
    board_defs: &[StatusDef],
    target: usize,
    now: &Zoned,
) -> Result<()> {
    let is_last = target == board_defs.len() - 1;
    task.move_to(&board_defs[target].key, is_last, now);
    task.save()
}

/// Open columns: prio, then due (missing last), then created.
/// Last column: most recently done first (`done`, else the file's mtime).
pub fn sort_tasks(tasks: &mut [Task], done_column: bool) {
    if done_column {
        tasks.sort_by(|a, b| {
            b.done
                .or(b.mtime)
                .cmp(&a.done.or(a.mtime))
                .then_with(|| a.id.cmp(&b.id))
        });
    } else {
        tasks.sort_by(compare_open);
    }
}

fn compare_open(a: &Task, b: &Task) -> Ordering {
    a.prio_or_default()
        .cmp(&b.prio_or_default())
        .then_with(|| match (a.due, b.due) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
        .then_with(|| a.created.cmp(&b.created))
        .then_with(|| a.id.cmp(&b.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::parse_board;
    use jiff::civil::date;
    use std::path::PathBuf;

    fn task(id: &str, extra: &str) -> Task {
        let raw = format!("+++\ntitle = \"{id}\"\nstatus = \"todo\"\n{extra}\n+++\n");
        Task::parse(id, PathBuf::from(format!("{id}.md")), &raw).unwrap()
    }

    fn ids(tasks: &[Task]) -> Vec<&str> {
        tasks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn open_sorting() {
        let mut tasks = vec![
            task("nodue", "prio = 1"),
            task("p3", "prio = 3"),
            task("default", "due = 2026-10-01"),
            task("late", "prio = 1\ndue = 2026-12-01"),
            task("early", "prio = 1\ndue = 2026-11-01"),
            task("old", "prio = 2\ncreated = 2026-01-01T00:00:00Z"),
            task("new", "prio = 2\ncreated = 2026-06-01T00:00:00Z"),
        ];
        sort_tasks(&mut tasks, false);
        assert_eq!(
            ids(&tasks),
            ["early", "late", "nodue", "default", "old", "new", "p3"]
        );
    }

    #[test]
    fn done_sorting_newest_first() {
        let mut tasks = vec![
            task("a", "done = 2026-10-01T10:00:00Z"),
            task("b", "done = 2026-10-03T10:00:00Z"),
            task("c", ""),
        ];
        sort_tasks(&mut tasks, true);
        assert_eq!(ids(&tasks), ["b", "a", "c"]);
    }

    #[test]
    fn unknown_columns_go_first_with_warning() {
        let defs = parse_board("").unwrap();
        let mut odd = task("odd", "");
        odd.status = "weg".into();
        let mut doing = task("d", "");
        doing.status = "doing".into();
        let board = Board::build(defs, vec![odd, doing], vec![], Filter::all());
        assert_eq!(ids(&board.columns[0].tasks), ["odd"]);
        assert_eq!(ids(&board.columns[1].tasks), ["d"]);
        assert_eq!(board.warnings.len(), 1);
        assert!(board.warnings[0].contains("weg"));
    }

    #[test]
    fn counts_and_resolve() {
        let defs = parse_board(&crate::store::default_board(crate::i18n::Lang::De)).unwrap();
        let mut done = task("x", "due = 2020-01-01");
        done.status = "done".into();
        let board = Board::build(
            defs,
            vec![task("a", "due = 2020-01-01"), task("b", ""), done],
            vec![],
            Filter::all(),
        );
        assert_eq!(board.open_count(), 2);
        assert_eq!(board.overdue_count(date(2026, 10, 22)), 1);
        assert_eq!(board.resolve_status("Doing"), Some(1));
        assert_eq!(board.resolve_status("in arbeit"), Some(1));
        assert_eq!(board.resolve_status("wa"), Some(2));
        assert_eq!(board.resolve_status("erl"), Some(3));
        assert_eq!(board.resolve_status("arbeit"), Some(1));
        assert_eq!(board.resolve_status("xyz"), None);
        assert_eq!(board.find("b"), Some((0, 1)));
    }

    #[test]
    fn search_query() {
        let cfg = parse_board("").unwrap();
        let mut noted = task("n1", "");
        noted.body = "\ncall the plumber about the leak\n".into();
        let tasks = || vec![task("Rechnung", ""), task("Steuer", ""), noted.clone()];
        let search = |q: &str| {
            let f = Filter {
                query: q.into(),
                ..Filter::all()
            };
            let b = Board::build(cfg.clone(), tasks(), vec![], f);
            (
                ids(&b.columns[0].tasks)
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
                b.hidden,
            )
        };
        assert_eq!(search("").0.len(), 3);
        assert_eq!(search("RECH"), (vec!["Rechnung".to_string()], 2));
        assert_eq!(search("plumber leak").0, ["n1"]);
        assert_eq!(search("plumber roof").0.len(), 0);
        assert_eq!(search("steu").0, ["Steuer"]);
    }

    fn bucket_task(id: &str, bucket: &str) -> Task {
        task(id, &format!("bucket = \"{bucket}\""))
    }

    #[test]
    fn old_done_tasks_are_hidden() {
        let today = crate::dates::today();
        let at = |days_ago: i64| {
            let d = today.checked_sub(jiff::ToSpan::days(days_ago)).unwrap();
            format!("done = {d}T12:00:00Z")
        };
        let mut recent = task("recent", &at(3));
        recent.status = "done".into();
        let mut old = task("old", &at(20));
        old.status = "done".into();
        let open_old = task("open", &at(20));
        assert!(is_old_done(&old, Some(14), today));
        assert!(!is_old_done(&recent, Some(14), today));
        assert!(!is_old_done(&old, None, today));
        let cfg = parse_board("").unwrap();
        let tasks = || vec![recent.clone(), old.clone(), open_old.clone()];
        let f = Filter {
            done_days: Some(14),
            ..Filter::default()
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], f);
        assert_eq!(ids(&b.columns[3].tasks), ["recent"]);
        assert_eq!(b.old_done, 1);
        // open tasks are never hidden by age
        assert_eq!(ids(&b.columns[0].tasks), ["open"]);
        let b = Board::build(cfg, tasks(), vec![], Filter::all());
        assert_eq!(b.columns[3].tasks.len(), 2);
        assert_eq!(b.old_done, 0);
    }

    #[test]
    fn tag_filter_and_counts() {
        let cfg = parse_board("").unwrap();
        let tasks = || {
            vec![
                task("a", "tags = [\"dev\"]"),
                task("b", "tags = [\"ops\", \"dev\"]"),
                task("c", "tags = [\"lvs\"]"),
                task("d", ""),
            ]
        };
        let all = Board::build(cfg.clone(), tasks(), vec![], Filter::all());
        assert_eq!(
            all.tag_counts,
            [
                ("dev".to_string(), 2),
                ("lvs".to_string(), 1),
                ("ops".to_string(), 1)
            ]
        );
        assert_eq!(all.tag_label(), None);
        let f = Filter {
            tags: vec!["ops".into(), "lvs".into()],
            ..Filter::default()
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], f);
        assert_eq!(ids(&b.columns[0].tasks), ["b", "c"]);
        assert_eq!(b.hidden, 2);
        assert_eq!(b.tag_label().as_deref(), Some("+ops +lvs"));
        // the search also finds tags
        let q = Filter {
            query: "+lvs".into(),
            ..Filter::default()
        };
        assert_eq!(
            ids(&Board::build(cfg, tasks(), vec![], q).columns[0].tasks),
            ["c"]
        );
    }

    #[test]
    fn bucket_for_new_tasks() {
        let cfg = parse_board(
            "[[buckets]]\nkey='home'\nname='Privat'\n[[buckets]]\nkey='work'\nname='Arbeit'",
        )
        .unwrap();
        let all = Filter::all();
        let work = Filter {
            buckets: vec!["work".into()],
            ..Filter::default()
        };
        let pick = |explicit, filter: &Filter, default| {
            bucket_for_new(explicit, filter, &cfg.buckets, default).unwrap()
        };
        assert_eq!(
            pick(Some("hom"), &work, Some("work")),
            Some(("home".into(), BucketOrigin::Explicit))
        );
        assert_eq!(
            pick(None, &work, Some("home")),
            Some(("work".into(), BucketOrigin::Filter))
        );
        assert_eq!(
            pick(None, &all, Some("work")),
            Some(("work".into(), BucketOrigin::Default))
        );
        assert_eq!(pick(None, &all, Some("gone")), None);
        assert_eq!(pick(None, &all, None), None);
        // work plus "no bucket" shown: new tasks still belong to work
        let work_and_none = Filter {
            buckets: vec!["work".into(), NO_BUCKET.into()],
            ..Filter::default()
        };
        assert_eq!(
            pick(None, &work_and_none, None),
            Some(("work".into(), BucketOrigin::Filter))
        );
        assert!(!work_and_none.shows_single_bucket());
        assert!(work.shows_single_bucket());
    }

    #[test]
    fn bucket_filter() {
        let cfg = parse_board(
            "[[buckets]]\nkey='home'\nname='Privat'\n[[buckets]]\nkey='work'\nname='Arbeit'",
        )
        .unwrap();
        let tasks = || {
            vec![
                bucket_task("h", "home"),
                bucket_task("w", "work"),
                bucket_task("x", "weg"),
                task("n", ""),
            ]
        };
        let all = Board::build(cfg.clone(), tasks(), vec![], Filter::all());
        assert_eq!(all.open_count(), 4);
        assert_eq!(all.bucket_counts, [1, 1, 2]);
        assert_eq!(all.warnings.len(), 1);
        assert_eq!(all.filter_label(), None);

        let work = Filter {
            buckets: vec!["work".into()],
            ..Filter::default()
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], work.clone());
        assert_eq!(ids(&b.columns[0].tasks), ["w"]);
        assert_eq!(b.hidden, 3);
        assert_eq!(b.filter.inherited(), Some("work"));
        assert_eq!(b.filter_label().as_deref(), Some("Arbeit"));

        let none = Filter {
            buckets: vec![NO_BUCKET.into()],
            ..Filter::default()
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], none);
        assert_eq!(ids(&b.columns[0].tasks), ["n", "x"]);
        assert_eq!(b.filter.inherited(), None);

        // Everything or only unknown entries selected = all
        let full = Filter {
            buckets: vec!["home".into(), "work".into(), "-".into()],
            ..Filter::default()
        };
        assert!(full.normalized(&cfg.buckets).is_all());
        let stale = Filter {
            buckets: vec!["alt".into()],
            ..Filter::default()
        };
        assert!(stale.normalized(&cfg.buckets).is_all());
        // Without declared buckets nothing is ever filtered
        assert!(work.normalized(&[]).is_all());
    }
}
