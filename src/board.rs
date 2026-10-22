//! Grouping tasks into columns, bucket filter and sorting.

use std::cmp::Ordering;

use anyhow::Result;
use jiff::Zoned;

use crate::store::{BoardConfig, BucketDef, StatusDef, Store, resolve_def};
use crate::task::Task;

/// Filter entry for tasks without a (valid) bucket.
pub const NO_BUCKET: &str = "-";

/// Which buckets are shown. Empty = all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub buckets: Vec<String>,
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

    /// If exactly one real bucket is selected, new tasks inherit it.
    pub fn inherited(&self) -> Option<&str> {
        match self.buckets.as_slice() {
            [one] if one != NO_BUCKET => Some(one),
            _ => None,
        }
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
    /// Open tasks hidden by the filter.
    pub hidden: usize,
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
        let mut hidden = 0;
        for task in tasks {
            let idx = match columns.iter().position(|c| c.def.key == task.status) {
                Some(i) => i,
                None => {
                    if task.status.is_empty() {
                        warnings.push(format!("{}: kein Status gesetzt", task.id));
                    } else {
                        warnings.push(format!("{}: unbekannter Status „{}“", task.id, task.status));
                    }
                    0
                }
            };
            let bucket_idx = match &task.bucket {
                None => None,
                Some(b) => {
                    let i = cfg.buckets.iter().position(|d| &d.key == b);
                    if i.is_none() {
                        warnings.push(format!("{}: unbekannter Bucket „{b}“", task.id));
                    }
                    i
                }
            };
            let key = bucket_idx.map_or(NO_BUCKET, |i| cfg.buckets[i].key.as_str());
            if idx != last {
                bucket_counts[bucket_idx.unwrap_or(cfg.buckets.len())] += 1;
            }
            if !filter.matches(key) {
                if idx != last {
                    hidden += 1;
                }
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
            hidden,
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

    /// Short description of the filter, e.g. "Work" or "Home, no bucket".
    pub fn filter_label(&self) -> Option<String> {
        if self.filter.is_all() {
            return None;
        }
        let names: Vec<&str> = self
            .filter
            .buckets
            .iter()
            .map(|b| {
                if b == NO_BUCKET {
                    "ohne Bucket"
                } else {
                    self.bucket_name(b)
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

/// Bucket for a new task: explicit via `@…`, otherwise inherited from the filter.
/// Returns (bucket key, inherited?).
pub fn bucket_for_new(
    explicit: Option<&str>,
    filter: &Filter,
    buckets: &[BucketDef],
) -> Result<(Option<String>, bool)> {
    match explicit {
        Some(raw) => Ok((Some(resolve_bucket(buckets, raw)?), false)),
        None => Ok(match filter.inherited() {
            Some(b) => (Some(b.to_string()), true),
            None => (None, false),
        }),
    }
}

pub fn resolve_bucket(buckets: &[BucketDef], raw: &str) -> Result<String> {
    let raw = raw.trim_start_matches('@');
    if buckets.is_empty() {
        anyhow::bail!("keine Buckets definiert (siehe [[buckets]] in shdo.toml)");
    }
    match resolve_def(buckets, raw) {
        Some(i) => Ok(buckets[i].key.clone()),
        None => {
            let keys: Vec<_> = buckets.iter().map(|b| b.key.as_str()).collect();
            anyhow::bail!(
                "unbekannter Bucket „{raw}“ (vorhanden: {})",
                keys.join(", ")
            )
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
        assert!(board.warnings[0].contains("„weg“"));
    }

    #[test]
    fn counts_and_resolve() {
        let defs = parse_board("").unwrap();
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

    fn bucket_task(id: &str, bucket: &str) -> Task {
        task(id, &format!("bucket = \"{bucket}\""))
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
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], work.clone());
        assert_eq!(ids(&b.columns[0].tasks), ["w"]);
        assert_eq!(b.hidden, 3);
        assert_eq!(b.filter.inherited(), Some("work"));
        assert_eq!(b.filter_label().as_deref(), Some("Arbeit"));

        let none = Filter {
            buckets: vec![NO_BUCKET.into()],
        };
        let b = Board::build(cfg.clone(), tasks(), vec![], none);
        assert_eq!(ids(&b.columns[0].tasks), ["n", "x"]);
        assert_eq!(b.filter.inherited(), None);

        // Everything or only unknown entries selected = all
        let full = Filter {
            buckets: vec!["home".into(), "work".into(), "-".into()],
        };
        assert!(full.normalized(&cfg.buckets).is_all());
        let stale = Filter {
            buckets: vec!["alt".into()],
        };
        assert!(stale.normalized(&cfg.buckets).is_all());
        // Without declared buckets nothing is ever filtered
        assert!(work.normalized(&[]).is_all());
    }
}
