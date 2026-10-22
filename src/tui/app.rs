//! TUI application state and actions.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use anyhow::Result;
use jiff::Zoned;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_i18n::t;

use super::form::{Form, Input};
use crate::board::{self, Board, Filter, NO_BUCKET};
use crate::config::{self, AppConfig};
use crate::git::Git;
use crate::holidays::Region;
use crate::store::{StatusDef, Store};
use crate::task::{PRIO_DEFAULT, Task};
use crate::theme::{self, Theme};
use crate::{dates, quickadd};

#[derive(Clone, Copy, PartialEq)]
pub enum InputKind {
    Rename,
    Due,
}

impl InputKind {
    pub fn label(self) -> String {
        match self {
            InputKind::Rename => t!("input.title").into_owned(),
            InputKind::Due => t!("input.due").into_owned(),
        }
    }
}

pub enum Mode {
    Normal,
    /// Live search; the query is applied while typing.
    Search {
        input: Input,
    },
    Input {
        kind: InputKind,
        input: Input,
    },
    Form(Box<Form>),
    ConfirmDelete,
    Help {
        scroll: u16,
    },
    Themes {
        idx: usize,
        original: usize,
    },
    Detail {
        scroll: u16,
    },
    /// Entries: buckets in `shdo.toml` order, then "no bucket".
    Filter {
        idx: usize,
        checked: Vec<bool>,
        original: Filter,
    },
    /// 0 = no bucket, otherwise index + 1.
    PickBucket {
        idx: usize,
    },
    BucketNew {
        name: Input,
        key: Input,
        /// key edited by hand – stop deriving it from the name
        key_edited: bool,
        focus_key: bool,
    },
    BucketRename {
        key: String,
        input: Input,
    },
    BucketDelete {
        key: String,
        open: usize,
        done: usize,
        idx: usize,
    },
    /// Target for the tasks of a bucket being deleted (other buckets only).
    BucketMove {
        key: String,
        idx: usize,
    },
}

#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Ok,
    Error,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DeleteChoice {
    Delete,
    Unassign,
    Move,
    ArchiveDone,
    Cancel,
}

/// What happens to a bucket's tasks when the bucket is deleted.
enum Retarget {
    /// The bucket is empty
    Keep,
    /// Replace the tasks' bucket (None = no bucket)
    To(Option<String>),
    /// Archive all (done) tasks
    Archive,
}

pub struct App {
    pub store: Store,
    cfg: AppConfig,
    cfg_path: PathBuf,
    pub board: Board,
    filter: Filter,
    pub col: usize,
    pub rows: Vec<usize>,
    /// Scroll offset per column (in cards), updated while rendering.
    pub offsets: Vec<usize>,
    /// First visible column when not all columns fit side by side.
    pub col_offset: usize,
    pub mode: Mode,
    /// Show the last column (done); stored per machine.
    pub show_done: bool,
    /// Show the calendar sidebar; stored per machine.
    pub show_calendar: bool,
    /// Region for public holidays; `None` = no holidays.
    pub region: Option<Region>,
    pub status: Option<(Level, String)>,
    pub themes: Vec<Theme>,
    pub theme: usize,
    pub theme_warnings: Vec<String>,
    fingerprint: u64,
    sync_rx: Option<Receiver<Result<String>>>,
    pub quit: bool,
    /// Sync once on start and once on quit (config `auto_sync`).
    auto_sync: bool,
    /// Quit as soon as the running sync has finished successfully.
    quit_after_sync: bool,
    /// The sync on quit failed: the next quit leaves without syncing.
    skip_sync_on_quit: bool,
    pub editor_request: Option<PathBuf>,
    /// Open config.toml in the editor and apply it afterwards.
    pub config_request: bool,
}

impl App {
    pub fn new(store: Store, cfg: AppConfig, cfg_path: PathBuf) -> Result<App> {
        let (themes, theme_warnings) = theme::load_all(&config::config_dir().join("themes"));
        let wanted = cfg.theme.as_deref().unwrap_or(theme::DEFAULT_THEME);
        let mut theme_warnings = theme_warnings;
        let theme = themes
            .iter()
            .position(|t| t.id == wanted)
            .unwrap_or_else(|| {
                theme_warnings.push(t!("warn.theme_not_found", theme = wanted).into_owned());
                themes
                    .iter()
                    .position(|t| t.id == theme::DEFAULT_THEME)
                    .unwrap_or(0)
            });
        let filter = Filter {
            buckets: cfg.buckets.clone(),
            ..Filter::default()
        };
        let show_done = cfg.show_done.unwrap_or(true);
        let show_calendar = cfg.show_calendar.unwrap_or(true);
        let auto_sync = cfg.auto_sync.unwrap_or(false);
        let region = match Region::parse_setting(cfg.holidays.as_deref().unwrap_or("")) {
            Ok(r) => r,
            Err(value) => {
                theme_warnings.push(t!("warn.bad_holidays", value = value).into_owned());
                None
            }
        };
        let board = Board::load(&store, &filter)?;
        let n = board.columns.len();
        Ok(App {
            fingerprint: store.fingerprint(),
            store,
            cfg,
            cfg_path,
            board,
            filter,
            col: 0,
            rows: vec![0; n],
            offsets: vec![0; n],
            col_offset: 0,
            mode: Mode::Normal,
            show_done,
            show_calendar,
            auto_sync,
            quit_after_sync: false,
            skip_sync_on_quit: false,
            region,
            status: None,
            themes,
            theme,
            theme_warnings,
            sync_rx: None,
            quit: false,
            editor_request: None,
            config_request: false,
        })
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme]
    }

    pub fn cfg_path(&self) -> &std::path::Path {
        &self.cfg_path
    }

    /// Re-reads config.toml after it was edited and applies it to the running board.
    /// A changed data directory only takes effect on the next start.
    pub fn reload_config(&mut self) {
        let cfg = match AppConfig::load(&self.cfg_path) {
            Ok(cfg) => cfg,
            Err(e) => {
                self.set_status(Level::Error, format!("{e:#}"));
                return;
            }
        };
        crate::i18n::set(cfg.lang());
        let dir_changed = cfg.dir != self.cfg.dir;
        let keep = self.selected_id();
        let store = Store::new(self.store.dir.clone());
        match App::new(store, cfg, self.cfg_path.clone()) {
            Ok(mut fresh) => {
                fresh.col = self.col;
                fresh.rows = std::mem::take(&mut self.rows);
                fresh.filter.query = std::mem::take(&mut self.filter.query);
                fresh.sync_rx = self.sync_rx.take();
                *self = fresh;
                self.reload(keep);
                let mut msg = t!("msg.settings_applied").into_owned();
                if dir_changed {
                    msg.push_str(&format!(" – {}", t!("msg.dir_next_start")));
                }
                self.set_status(Level::Ok, msg);
            }
            Err(e) => self.set_status(Level::Error, format!("{e:#}")),
        }
    }

    pub fn editor(&self) -> String {
        self.cfg.editor()
    }

    pub fn warnings(&self) -> impl Iterator<Item = &String> {
        self.board.warnings.iter().chain(&self.theme_warnings)
    }

    pub fn syncing(&self) -> bool {
        self.sync_rx.is_some()
    }

    pub fn selected(&self) -> Option<&Task> {
        self.board
            .columns
            .get(self.col)?
            .tasks
            .get(self.rows[self.col])
    }

    /// Number of displayed columns (without "done" if hidden).
    pub fn visible_columns(&self) -> usize {
        let n = self.board.columns.len();
        if self.show_done || n == 1 { n } else { n - 1 }
    }

    fn toggle_show_done(&mut self) {
        self.show_done = !self.show_done;
        self.col = self.col.min(self.visible_columns() - 1);
        let res = config::set_bool(&self.cfg_path, "show_done", self.show_done);
        let name = self.board.columns[self.board.last_index()].def.name.clone();
        let msg = if self.show_done {
            t!("msg.column_shown", name = name).into_owned()
        } else {
            t!("msg.column_hidden", name = name).into_owned()
        };
        self.report(res, msg);
    }

    fn toggle_calendar(&mut self) {
        self.show_calendar = !self.show_calendar;
        let res = config::set_bool(&self.cfg_path, "show_calendar", self.show_calendar);
        let msg = if self.show_calendar {
            t!("msg.calendar_shown")
        } else {
            t!("msg.calendar_hidden")
        };
        self.report(res, msg);
    }

    fn selected_id(&self) -> Option<String> {
        self.selected().map(|t| t.id.clone())
    }

    fn defs(&self) -> Vec<StatusDef> {
        self.board.columns.iter().map(|c| c.def.clone()).collect()
    }

    fn set_status(&mut self, level: Level, msg: impl Into<String>) {
        self.status = Some((level, msg.into()));
    }

    fn report(&mut self, res: Result<()>, ok: impl Into<String>) {
        match res {
            Ok(()) => self.set_status(Level::Ok, ok),
            Err(e) => self.set_status(Level::Error, format!("{e:#}")),
        }
    }

    /// Reloads the board; `follow` selects that card afterwards (switching column if needed).
    pub fn reload(&mut self, follow: Option<String>) {
        match Board::load(&self.store, &self.filter) {
            Ok(b) => self.board = b,
            Err(e) => {
                self.set_status(Level::Error, format!("{e:#}"));
                return;
            }
        }
        self.fingerprint = self.store.fingerprint();
        let n = self.board.columns.len();
        self.rows.resize(n, 0);
        self.offsets.resize(n, 0);
        self.col = self.col.min(n - 1);
        if let Some((c, r)) = follow
            .and_then(|id| self.board.find(&id))
            .filter(|&(c, _)| c < self.visible_columns())
        {
            self.col = c;
            self.rows[c] = r;
        }
        for (c, col) in self.board.columns.iter().enumerate() {
            self.rows[c] = self.rows[c].min(col.tasks.len().saturating_sub(1));
        }
        self.col = self.col.min(self.visible_columns() - 1);
    }

    /// Called periodically: collects the sync result and detects external changes.
    pub fn tick(&mut self) {
        if let Some(rx) = &self.sync_rx
            && let Ok(res) = rx.try_recv()
        {
            self.sync_rx = None;
            let keep = self.selected_id();
            self.reload(keep);
            let ok = res.is_ok();
            match res {
                Ok(msg) => self.set_status(Level::Ok, format!("Sync: {msg}")),
                Err(e) => self.set_status(Level::Error, format!("Sync: {e:#}")),
            }
            if self.quit_after_sync {
                self.quit_after_sync = false;
                if ok {
                    // Changes made while a sync was running still need to go out
                    self.request_quit();
                } else {
                    self.skip_sync_on_quit = true;
                    if let Some((_, msg)) = &self.status {
                        // Hint first: long git errors get cut off at the end of the line
                        let msg = format!("{} – {msg}", t!("msg.quit_without_sync"));
                        self.set_status(Level::Error, msg);
                    }
                }
            }
        }
        if matches!(
            self.mode,
            Mode::Normal | Mode::Help { .. } | Mode::Themes { .. }
        ) && self.store.fingerprint() != self.fingerprint
        {
            let keep = self.selected_id();
            self.reload(keep);
        }
    }

    pub fn after_editor(&mut self, path: PathBuf, res: Result<()>) {
        let keep = self.selected_id();
        self.reload(keep);
        match res.and_then(|()| Task::load(&path).map(|_| ())) {
            Ok(()) => self.set_status(Level::Ok, t!("msg.saved")),
            Err(e) => self.set_status(Level::Error, format!("{}: {e:#}", path.display())),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key),
            Mode::Search { mut input } => match key.code {
                KeyCode::Esc => self.set_query(String::new()),
                KeyCode::Enter => {}
                _ => {
                    input.handle(key);
                    self.set_query(input.text.clone());
                    self.mode = Mode::Search { input };
                }
            },
            Mode::Input { kind, mut input } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if !self.submit_input(kind, &input.text) {
                        self.mode = Mode::Input { kind, input };
                    }
                }
                _ => {
                    input.handle(key);
                    self.mode = Mode::Input { kind, input };
                }
            },
            Mode::Form(mut form) => match key.code {
                KeyCode::Esc => self.set_status(Level::Info, t!("msg.discarded")),
                KeyCode::Enter => {
                    if let Err(e) = self.submit_form(&form) {
                        self.set_status(Level::Error, format!("{e:#}"));
                        self.mode = Mode::Form(form);
                    }
                }
                _ => {
                    self.status = None;
                    form.handle(key);
                    self.mode = Mode::Form(form);
                }
            },
            Mode::ConfirmDelete => {
                if matches!(key.code, KeyCode::Char('j' | 'J' | 'y' | 'Y')) {
                    self.delete();
                } else {
                    self.set_status(Level::Info, t!("msg.not_deleted"));
                }
            }
            Mode::Help { scroll } => match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.mode = Mode::Help { scroll: scroll + 1 },
                KeyCode::Char('k') | KeyCode::Up => {
                    self.mode = Mode::Help {
                        scroll: scroll.saturating_sub(1),
                    }
                }
                _ => {}
            },
            Mode::Themes { idx, original } => self.themes_key(key, idx, original),
            Mode::Detail { scroll } => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.mode = Mode::Detail { scroll: scroll + 1 }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.mode = Mode::Detail {
                        scroll: scroll.saturating_sub(1),
                    }
                }
                KeyCode::Char('e') => self.open_edit_form(),
                KeyCode::Char('E') => self.request_editor(),
                _ => {}
            },
            Mode::Filter {
                idx,
                checked,
                original,
            } => self.filter_key(key, idx, checked, original),
            Mode::PickBucket { idx } => self.pick_bucket_key(key, idx),
            Mode::BucketNew {
                name,
                key: key_input,
                key_edited,
                focus_key,
            } => self.bucket_new_key(key, name, key_input, key_edited, focus_key),
            Mode::BucketRename { key: bucket, input } => self.bucket_rename_key(key, bucket, input),
            Mode::BucketDelete {
                key: bucket,
                open,
                done,
                idx,
            } => self.bucket_delete_key(key, bucket, open, done, idx),
            Mode::BucketMove { key: bucket, idx } => self.bucket_move_key(key, bucket, idx),
        }
    }

    fn normal_key(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        self.status = None;
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            // Esc first clears an active search, then quits
            KeyCode::Esc if !self.filter.query.is_empty() => self.set_query(String::new()),
            KeyCode::Esc => self.request_quit(),
            KeyCode::Char('/') => {
                self.mode = Mode::Search {
                    input: Input::with(&self.filter.query),
                }
            }
            KeyCode::Char('H') => self.move_selected(-1),
            KeyCode::Char('L') => self.move_selected(1),
            KeyCode::Left if shift => self.move_selected(-1),
            KeyCode::Right if shift => self.move_selected(1),
            KeyCode::Char('h') | KeyCode::Left => self.col = self.col.saturating_sub(1),
            KeyCode::Char('l') | KeyCode::Right => {
                self.col = (self.col + 1).min(self.visible_columns() - 1)
            }
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Char('g') | KeyCode::Home => self.rows[self.col] = 0,
            KeyCode::Char('G') | KeyCode::End => self.step(isize::MAX),
            KeyCode::Char('n') => {
                self.mode = Mode::Form(Box::new(Form::new_task(&self.board, self.col)))
            }
            KeyCode::Char('e') => self.open_edit_form(),
            KeyCode::Char('r') => {
                if let Some(t) = self.selected() {
                    let input = Input::with(&t.title);
                    self.mode = Mode::Input {
                        kind: InputKind::Rename,
                        input,
                    };
                }
            }
            KeyCode::Char('D') => {
                if let Some(t) = self.selected() {
                    let input = Input::with(&t.due.map(|d| d.to_string()).unwrap_or_default());
                    self.mode = Mode::Input {
                        kind: InputKind::Due,
                        input,
                    };
                }
            }
            KeyCode::Char(c @ '1'..='3') => self.set_prio(c as u8 - b'0'),
            KeyCode::Char('x') => self.toggle_done(),
            KeyCode::Char('d') => {
                if self.selected().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
            }
            KeyCode::Char('E') => self.request_editor(),
            KeyCode::Enter => {
                if self.selected().is_some() {
                    self.mode = Mode::Detail { scroll: 0 };
                }
            }
            KeyCode::Char('f') => self.open_filter(),
            KeyCode::Char('v') => self.toggle_show_done(),
            KeyCode::Char('c') => self.toggle_calendar(),
            KeyCode::Char('b') => self.open_pick_bucket(),
            KeyCode::Char('s') => self.start_sync(),
            KeyCode::Char('R') => {
                let keep = self.selected_id();
                self.reload(keep);
                self.set_status(Level::Info, t!("msg.reloaded"));
            }
            KeyCode::Char('t') => {
                self.mode = Mode::Themes {
                    idx: self.theme,
                    original: self.theme,
                }
            }
            KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
            KeyCode::Char(',') => self.config_request = true,
            _ => {}
        }
    }

    fn step(&mut self, delta: isize) {
        let len = self.board.columns[self.col].tasks.len();
        if len == 0 {
            return;
        }
        let row = self.rows[self.col] as isize;
        self.rows[self.col] = row.saturating_add(delta).clamp(0, len as isize - 1) as usize;
    }

    /// Applies `f` to the selected card, saves and reloads.
    fn edit_selected(&mut self, ok: &str, f: impl FnOnce(&mut Task)) {
        let Some(mut task) = self.selected().cloned() else {
            return;
        };
        f(&mut task);
        let res = task.save();
        self.reload(Some(task.id.clone()));
        self.report(res, ok);
    }

    fn move_selected(&mut self, delta: isize) {
        let target = self.col as isize + delta;
        if target < 0 || target as usize >= self.board.columns.len() {
            return;
        }
        let Some(mut task) = self.selected().cloned() else {
            return;
        };
        let defs = self.defs();
        let res = board::move_task(&mut task, &defs, target as usize, &Zoned::now());
        self.reload(Some(task.id.clone()));
        let mut msg = format!("→ {}", defs[target as usize].name);
        if target as usize >= self.visible_columns() {
            msg.push_str(&format!(" ({})", t!("msg.into_hidden_column")));
        }
        self.report(res, msg);
    }

    /// Done or reopened (first column); the selection stays in the column.
    fn toggle_done(&mut self) {
        let Some(mut task) = self.selected().cloned() else {
            return;
        };
        let defs = self.defs();
        let last = defs.len() - 1;
        let target = if self.col == last { 0 } else { last };
        let res = board::move_task(&mut task, &defs, target, &Zoned::now());
        self.reload(None);
        self.report(res, format!("{} → {}", task.title, defs[target].name));
    }

    fn set_prio(&mut self, prio: u8) {
        self.edit_selected(&format!("Prio {prio}"), |t| t.set_prio(Some(prio)));
    }

    fn delete(&mut self) {
        let Some(task) = self.selected().cloned() else {
            return;
        };
        let res = self.store.delete(&task);
        self.reload(None);
        self.report(res, t!("msg.deleted", title = task.title));
    }

    /// Returns `false` if the input is invalid and should stay open.
    fn submit_input(&mut self, kind: InputKind, text: &str) -> bool {
        let today = dates::today();
        match kind {
            InputKind::Rename => {
                let title = text.trim().to_string();
                if title.is_empty() {
                    self.set_status(Level::Error, t!("msg.title_empty"));
                    return false;
                }
                self.edit_selected(&t!("msg.renamed"), |t| t.set_title(&title));
            }
            InputKind::Due => {
                let due = if text.trim().is_empty() {
                    None
                } else {
                    match dates::parse_date(text, today) {
                        Ok(d) => Some(d),
                        Err(e) => {
                            self.set_status(Level::Error, format!("{e:#}"));
                            return false;
                        }
                    }
                };
                let msg = match due {
                    Some(d) => t!("msg.due_set", due = dates::fmt_due(d, today)).into_owned(),
                    None => t!("msg.due_removed").into_owned(),
                };
                self.edit_selected(&msg, |t| t.set_due(due));
            }
        }
        true
    }

    fn open_edit_form(&mut self) {
        if let Some(t) = self.selected() {
            self.mode = Mode::Form(Box::new(Form::edit_task(&self.board, t)));
        }
    }

    /// Saves the form. Tokens in the title (`P1`, `due:…`, `@…`) take precedence over the fields.
    fn submit_form(&mut self, form: &Form) -> Result<()> {
        let today = dates::today();
        let q = quickadd::parse_str(&form.title.text, today)?;
        let due = match q.due {
            Some(d) => Some(d),
            None if form.due.text.trim().is_empty() => None,
            None => Some(dates::parse_date(&form.due.text, today)?),
        };
        let bucket = match &q.bucket {
            Some(raw) => Some(board::resolve_bucket(&self.board.buckets, raw)?),
            None => form.bucket_key().map(String::from),
        };
        let prio = q.prio.unwrap_or(form.prio);
        let defs = self.defs();
        let now = Zoned::now();

        let (mut task, is_new) = match &form.editing {
            None => (self.store.create(&q.title, &defs[0].key, &now), true),
            Some(id) => (self.store.find(id)?, false),
        };
        task.set_title(&q.title);
        // Only write normal priority if it was already set explicitly.
        if prio != PRIO_DEFAULT || task.prio.is_some() {
            task.set_prio(Some(prio));
        }
        task.set_due(due);
        task.set_bucket(bucket.as_deref());
        if form.note_editable {
            task.set_note(&form.note.text);
        }
        task.move_to(&defs[form.status].key, form.status == defs.len() - 1, &now);
        task.save()?;

        let id = task.id.clone();
        self.reload(Some(id.clone()));
        let mut msg = if is_new {
            t!("msg.created", title = task.title).into_owned()
        } else {
            t!("msg.saved_task", title = task.title).into_owned()
        };
        if let Some(b) = &bucket {
            msg.push_str(&format!(" @{b}"));
            if form.inherited && q.bucket.is_none() {
                msg.push_str(&format!(" ({})", t!("msg.from_filter")));
            }
        }
        if self.board.find(&id).is_none() {
            msg.push_str(&format!(" – {}", t!("msg.hidden_by_filter")));
        }
        self.set_status(Level::Ok, msg);
        Ok(())
    }

    fn request_editor(&mut self) {
        if let Some(t) = self.selected() {
            self.editor_request = Some(t.path.clone());
        }
    }

    /// Called once when the board opens.
    pub fn on_start(&mut self) {
        if self.auto_sync {
            self.start_sync();
        }
    }

    /// Quit, with auto-sync first syncing pending changes. A failed sync keeps the board
    /// open with the error; quitting again then leaves without syncing.
    fn request_quit(&mut self) {
        if !self.auto_sync || self.skip_sync_on_quit {
            self.quit = true;
            return;
        }
        if self.syncing() {
            self.quit_after_sync = true;
            self.set_status(Level::Info, t!("msg.sync_before_quit"));
            return;
        }
        let pending = Git::new(&self.store.dir, true)
            .has_local_changes()
            .unwrap_or(true);
        if !pending {
            self.quit = true;
            return;
        }
        self.start_sync();
        self.quit_after_sync = true;
        self.set_status(Level::Info, t!("msg.sync_before_quit"));
    }

    fn start_sync(&mut self) {
        if self.syncing() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.store.dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Git::new(&dir, true).sync());
        });
        self.sync_rx = Some(rx);
        self.set_status(Level::Info, t!("msg.sync_running"));
    }

    fn no_buckets(&mut self) -> bool {
        if self.board.buckets.is_empty() {
            self.set_status(Level::Info, t!("bucket.none_defined"));
            return true;
        }
        false
    }

    fn open_filter(&mut self) {
        self.open_filter_at(0);
    }

    /// Opens (or reopens) the filter popup with the cursor on row `idx`.
    fn open_filter_at(&mut self, idx: usize) {
        let f = &self.board.filter;
        let mut checked: Vec<bool> = self
            .board
            .buckets
            .iter()
            .map(|b| f.is_all() || f.buckets.contains(&b.key))
            .collect();
        checked.push(f.is_all() || f.buckets.iter().any(|b| b == NO_BUCKET));
        let idx = idx.min(checked.len() - 1);
        self.mode = Mode::Filter {
            idx,
            checked,
            original: self.filter.clone(),
        };
    }

    fn filter_from(&self, checked: &[bool]) -> Filter {
        let n = self.board.buckets.len();
        let buckets = checked
            .iter()
            .enumerate()
            .filter(|&(_, &on)| on)
            .map(|(i, _)| {
                if i == n {
                    NO_BUCKET.to_string()
                } else {
                    self.board.buckets[i].key.clone()
                }
            })
            .collect();
        Filter {
            buckets,
            ..Filter::default()
        }
        .normalized(&self.board.buckets)
    }

    /// Applies the filter immediately (live preview); keeps the selected card if still visible.
    /// Sets the search text and reloads, keeping the selected card if it still matches.
    fn set_query(&mut self, query: String) {
        self.filter.query = query;
        let keep = self.selected_id();
        self.reload(keep);
    }

    /// Number of tasks currently shown (all columns).
    pub fn shown_count(&self) -> usize {
        self.board.columns.iter().map(|c| c.tasks.len()).sum()
    }

    pub fn query(&self) -> &str {
        &self.filter.query
    }

    fn apply_filter(&mut self, filter: Filter) {
        // The bucket filter never touches an active search
        self.filter = Filter {
            query: std::mem::take(&mut self.filter.query),
            ..filter
        };
        let keep = self.selected_id();
        self.reload(keep);
    }

    fn filter_key(&mut self, key: KeyEvent, idx: usize, mut checked: Vec<bool>, original: Filter) {
        let n = checked.len();
        let mut idx = idx;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => idx = (idx + 1) % n,
            KeyCode::Char('k') | KeyCode::Up => idx = (idx + n - 1) % n,
            KeyCode::Char(' ') | KeyCode::Char('x') => {
                checked[idx] = !checked[idx];
                if checked.iter().any(|&c| c) {
                    let f = self.filter_from(&checked);
                    self.apply_filter(f);
                }
            }
            KeyCode::Char('a') => {
                let on = !checked.iter().all(|&c| c);
                checked.iter_mut().for_each(|c| *c = on);
                if on {
                    self.apply_filter(Filter::all());
                }
            }
            KeyCode::Char('o') => {
                // only this one
                checked
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, c)| *c = i == idx);
                let f = self.filter_from(&checked);
                self.apply_filter(f);
            }
            KeyCode::Enter => {
                let f = if checked.iter().any(|&c| c) {
                    self.filter_from(&checked)
                } else {
                    Filter::all()
                };
                self.apply_filter(f.clone());
                let res = config::set_list(&self.cfg_path, "buckets", &f.buckets);
                let label = self
                    .board
                    .filter_label()
                    .unwrap_or_else(|| t!("msg.all").into_owned());
                self.report(res, t!("msg.filter", label = label));
                return;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.apply_filter(original);
                return;
            }
            KeyCode::Char('n') => {
                self.mode = Mode::BucketNew {
                    name: Input::default(),
                    key: Input::default(),
                    key_edited: false,
                    focus_key: false,
                };
                return;
            }
            KeyCode::Char('r') if idx < self.board.buckets.len() => {
                let b = &self.board.buckets[idx];
                self.mode = Mode::BucketRename {
                    key: b.key.clone(),
                    input: Input::with(&b.name),
                };
                return;
            }
            KeyCode::Char('d') if idx < self.board.buckets.len() => {
                let key = self.board.buckets[idx].key.clone();
                match self.bucket_counts(&key) {
                    Ok((open, done)) => {
                        self.mode = Mode::BucketDelete {
                            key,
                            open,
                            done,
                            idx: 0,
                        };
                        return;
                    }
                    Err(e) => self.set_status(Level::Error, format!("{e:#}")),
                }
            }
            _ => {}
        }
        self.mode = Mode::Filter {
            idx,
            checked,
            original,
        };
    }

    fn bucket_index(&self, key: &str) -> usize {
        self.board
            .buckets
            .iter()
            .position(|b| b.key == key)
            .unwrap_or(0)
    }

    /// (open, done) counts of all tasks in the bucket, regardless of the filter.
    fn bucket_counts(&self, key: &str) -> Result<(usize, usize)> {
        let done_key = &self.board.columns[self.board.last_index()].def.key;
        let tasks = self.store.tasks_in_bucket(key)?;
        let done = tasks.iter().filter(|t| &t.status == done_key).count();
        Ok((tasks.len() - done, done))
    }

    fn bucket_new_key(
        &mut self,
        key: KeyEvent,
        mut name: Input,
        mut key_input: Input,
        mut key_edited: bool,
        mut focus_key: bool,
    ) {
        match key.code {
            KeyCode::Esc => return self.open_filter_at(0),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => focus_key = !focus_key,
            KeyCode::Enter => {
                let res = self.store.add_bucket(&key_input.text, &name.text);
                match res {
                    Ok(()) => {
                        let new_key = key_input.text.trim().to_string();
                        let keep = self.selected_id();
                        self.reload(keep);
                        let idx = self.bucket_index(&new_key);
                        self.open_filter_at(idx);
                        self.set_status(
                            Level::Ok,
                            t!("msg.bucket_created", name = name.text.trim(), key = new_key),
                        );
                        return;
                    }
                    Err(e) => self.set_status(Level::Error, format!("{e:#}")),
                }
            }
            _ if focus_key => {
                key_input.handle(key);
                key_edited = true;
            }
            _ => {
                name.handle(key);
                if !key_edited {
                    key_input = Input::with(&crate::store::key_from_name(&name.text));
                }
            }
        }
        self.mode = Mode::BucketNew {
            name,
            key: key_input,
            key_edited,
            focus_key,
        };
    }

    fn bucket_rename_key(&mut self, key: KeyEvent, bucket: String, mut input: Input) {
        match key.code {
            KeyCode::Esc => self.open_filter_at(self.bucket_index(&bucket)),
            KeyCode::Enter => match self.store.rename_bucket(&bucket, &input.text) {
                Ok(()) => {
                    let keep = self.selected_id();
                    self.reload(keep);
                    self.open_filter_at(self.bucket_index(&bucket));
                    self.set_status(
                        Level::Ok,
                        t!("msg.bucket_renamed", name = input.text.trim()),
                    );
                }
                Err(e) => {
                    self.set_status(Level::Error, format!("{e:#}"));
                    self.mode = Mode::BucketRename { key: bucket, input };
                }
            },
            _ => {
                input.handle(key);
                self.mode = Mode::BucketRename { key: bucket, input };
            }
        }
    }

    /// Choices when deleting: (action, allowed, label).
    pub fn delete_choices(&self, open: usize, done: usize) -> Vec<(DeleteChoice, bool, String)> {
        if open + done == 0 {
            return vec![
                (
                    DeleteChoice::Delete,
                    true,
                    t!("bucket.choice_delete").into_owned(),
                ),
                (
                    DeleteChoice::Cancel,
                    true,
                    t!("bucket.choice_cancel").into_owned(),
                ),
            ];
        }
        let others = self.board.buckets.len() > 1;
        let archive_text = if open > 0 {
            t!("bucket.choice_archive_blocked", open = open).into_owned()
        } else {
            t!("bucket.choice_archive").into_owned()
        };
        vec![
            (
                DeleteChoice::Unassign,
                true,
                t!("bucket.choice_unassign").into_owned(),
            ),
            (
                DeleteChoice::Move,
                others,
                t!("bucket.choice_move").into_owned(),
            ),
            (DeleteChoice::ArchiveDone, open == 0, archive_text),
            (
                DeleteChoice::Cancel,
                true,
                t!("bucket.choice_cancel").into_owned(),
            ),
        ]
    }

    fn bucket_delete_key(
        &mut self,
        key: KeyEvent,
        bucket: String,
        open: usize,
        done: usize,
        idx: usize,
    ) {
        let choices = self.delete_choices(open, done);
        let n = choices.len();
        let back = self.bucket_index(&bucket);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = Mode::BucketDelete {
                    key: bucket,
                    open,
                    done,
                    idx: (idx + 1) % n,
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = Mode::BucketDelete {
                    key: bucket,
                    open,
                    done,
                    idx: (idx + n - 1) % n,
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => self.open_filter_at(back),
            KeyCode::Enter => {
                let (choice, enabled, _) = &choices[idx];
                if !enabled {
                    self.set_status(Level::Info, t!("msg.not_possible"));
                    self.mode = Mode::BucketDelete {
                        key: bucket,
                        open,
                        done,
                        idx,
                    };
                    return;
                }
                match choice {
                    DeleteChoice::Cancel => self.open_filter_at(back),
                    DeleteChoice::Move => {
                        self.mode = Mode::BucketMove {
                            key: bucket,
                            idx: 0,
                        }
                    }
                    DeleteChoice::Delete => self.finish_bucket_delete(&bucket, Retarget::Keep),
                    DeleteChoice::Unassign => {
                        self.finish_bucket_delete(&bucket, Retarget::To(None))
                    }
                    DeleteChoice::ArchiveDone => {
                        self.finish_bucket_delete(&bucket, Retarget::Archive)
                    }
                }
            }
            _ => {
                self.mode = Mode::BucketDelete {
                    key: bucket,
                    open,
                    done,
                    idx,
                }
            }
        }
    }

    /// Move targets: all other buckets.
    pub fn move_targets(&self, bucket: &str) -> Vec<(String, String)> {
        self.board
            .buckets
            .iter()
            .filter(|b| b.key != bucket)
            .map(|b| (b.key.clone(), b.name.clone()))
            .collect()
    }

    fn bucket_move_key(&mut self, key: KeyEvent, bucket: String, idx: usize) {
        let targets = self.move_targets(&bucket);
        let n = targets.len().max(1);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = Mode::BucketMove {
                    key: bucket,
                    idx: (idx + 1) % n,
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = Mode::BucketMove {
                    key: bucket,
                    idx: (idx + n - 1) % n,
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => self.open_filter_at(self.bucket_index(&bucket)),
            KeyCode::Enter => match targets.get(idx) {
                Some((target, _)) => {
                    let target = target.clone();
                    self.finish_bucket_delete(&bucket, Retarget::To(Some(target)));
                }
                None => self.open_filter_at(0),
            },
            _ => self.mode = Mode::BucketMove { key: bucket, idx },
        }
    }

    fn finish_bucket_delete(&mut self, bucket: &str, retarget: Retarget) {
        let name = self.board.bucket_name(bucket).to_string();
        let res = (|| -> Result<String> {
            let tasks = self.store.tasks_in_bucket(bucket)?;
            let n = tasks.len();
            let what = match &retarget {
                Retarget::Keep => String::new(),
                Retarget::To(None) => format!(", {}", t!("msg.tasks_unassigned", n = n)),
                Retarget::To(Some(target)) => format!(
                    ", {}",
                    t!(
                        "msg.tasks_moved",
                        n = n,
                        name = self.board.bucket_name(target)
                    )
                ),
                Retarget::Archive => format!(", {}", t!("msg.tasks_archived", n = n)),
            };
            for mut t in tasks {
                match &retarget {
                    Retarget::Keep => {}
                    Retarget::To(target) => {
                        t.set_bucket(target.as_deref());
                        t.save()?;
                    }
                    Retarget::Archive => self.store.archive(&t)?,
                }
            }
            self.store.remove_bucket(bucket)?;
            // Drop it from the saved filter, otherwise a dead entry remains
            if self.filter.buckets.iter().any(|b| b == bucket) {
                self.filter.buckets.retain(|b| b != bucket);
                config::set_list(&self.cfg_path, "buckets", &self.filter.buckets)?;
            }
            Ok(what)
        })();
        let keep = self.selected_id();
        self.reload(keep);
        self.open_filter_at(0);
        match res {
            Ok(what) => self.set_status(
                Level::Ok,
                format!("{}{what}", t!("msg.bucket_deleted", name = name)),
            ),
            Err(e) => self.set_status(Level::Error, format!("{e:#}")),
        }
    }

    fn open_pick_bucket(&mut self) {
        if self.no_buckets() {
            return;
        }
        let Some(t) = self.selected() else { return };
        let idx = t
            .bucket
            .as_ref()
            .and_then(|b| self.board.buckets.iter().position(|d| &d.key == b))
            .map_or(0, |i| i + 1);
        self.mode = Mode::PickBucket { idx };
    }

    fn pick_bucket_key(&mut self, key: KeyEvent, idx: usize) {
        let n = self.board.buckets.len() + 1;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = Mode::PickBucket { idx: (idx + 1) % n }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = Mode::PickBucket {
                    idx: (idx + n - 1) % n,
                }
            }
            KeyCode::Enter => {
                let bucket = idx
                    .checked_sub(1)
                    .map(|i| self.board.buckets[i].key.clone());
                let msg = match &bucket {
                    Some(b) => t!("msg.bucket_set", name = self.board.bucket_name(b)).into_owned(),
                    None => t!("bucket.none").into_owned(),
                };
                let id = self.selected_id();
                self.edit_selected(&msg, |t| t.set_bucket(bucket.as_deref()));
                if let Some(id) = id
                    && self.board.find(&id).is_none()
                {
                    self.set_status(Level::Ok, format!("{msg} – {}", t!("msg.hidden_by_filter")));
                }
            }
            _ => {}
        }
    }

    fn themes_key(&mut self, key: KeyEvent, idx: usize, original: usize) {
        let n = self.themes.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let idx = (idx + 1) % n;
                self.theme = idx;
                self.mode = Mode::Themes { idx, original };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let idx = (idx + n - 1) % n;
                self.theme = idx;
                self.mode = Mode::Themes { idx, original };
            }
            KeyCode::Enter => {
                self.theme = idx;
                let id = self.themes[idx].id.clone();
                let res = config::set_value(&self.cfg_path, "theme", &id);
                let name = self.themes[idx].name.clone();
                self.report(res, format!("Theme: {name}"));
            }
            KeyCode::Esc | KeyCode::Char('q') => self.theme = original,
            _ => self.mode = Mode::Themes { idx, original },
        }
    }
}
