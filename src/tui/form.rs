//! Input fields and the form for new or edited tasks.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_i18n::t;

use crate::board::{Board, BucketOrigin, bucket_for_new};
use crate::task::{PRIO_DEFAULT, Task};

#[derive(Default, Clone)]
pub struct Input {
    pub text: String,
    /// Cursor position in characters (not bytes).
    pub cursor: usize,
}

impl Input {
    pub fn with(text: &str) -> Input {
        Input {
            text: text.into(),
            cursor: text.chars().count(),
        }
    }

    fn byte_pos(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map_or(self.text.len(), |(i, _)| i)
    }

    pub fn handle(&mut self, key: KeyEvent) {
        let len = self.text.chars().count();
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                *self = Input::default();
            }
            KeyCode::Char(c) => {
                let at = self.byte_pos(self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte_pos(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Delete if self.cursor < len => {
                let at = self.byte_pos(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Title,
    Status,
    Prio,
    Due,
    Bucket,
    Tags,
    Note,
}

impl Field {
    pub fn label(self) -> String {
        let key = match self {
            Field::Title => "form.field.title",
            Field::Status => "form.field.status",
            Field::Prio => "form.field.prio",
            Field::Due => "form.field.due",
            Field::Bucket => "form.field.bucket",
            Field::Tags => "form.field.tags",
            Field::Note => "form.field.note",
        };
        t!(key).into_owned()
    }

    pub fn is_text(self) -> bool {
        matches!(self, Field::Title | Field::Due | Field::Tags | Field::Note)
    }
}

/// Localized name of a priority (1 = high … 3 = low).
pub fn prio_name(prio: u8) -> String {
    let key = match prio {
        1 => "form.prio.high",
        3 => "form.prio.low",
        _ => "form.prio.normal",
    };
    t!(key).into_owned()
}

/// Tags of all tasks, most used (open tasks) first, then by name.
fn known_tags(board: &Board) -> Vec<String> {
    let mut tags = board.tag_counts.clone();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tags.into_iter().map(|(tag, _)| tag).collect()
}

pub struct Form {
    /// `None` = new task, otherwise the ID of the edited task.
    pub editing: Option<String>,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub title: Input,
    pub status: usize,
    pub status_names: Vec<String>,
    pub prio: u8,
    pub due: Input,
    /// 0 = no bucket, otherwise index + 1 into `bucket_names`.
    pub bucket: usize,
    pub bucket_names: Vec<String>,
    pub bucket_keys: Vec<String>,
    /// The bucket was taken over from the filter and not touched yet.
    pub origin: Option<BucketOrigin>,
    /// Space-separated, `+` optional.
    pub tags: Input,
    pub note: Input,
    /// Multi-line notes can only be edited in the editor.
    pub note_editable: bool,
    /// Known tags, most used first – suggested while typing a tag.
    pub tag_names: Vec<String>,
}

impl Form {
    fn base(board: &Board) -> Form {
        let mut fields = vec![Field::Title, Field::Status, Field::Prio, Field::Due];
        if !board.buckets.is_empty() {
            fields.push(Field::Bucket);
        }
        fields.push(Field::Tags);
        fields.push(Field::Note);
        Form {
            editing: None,
            fields,
            focus: 0,
            title: Input::default(),
            status: 0,
            status_names: board.columns.iter().map(|c| c.def.name.clone()).collect(),
            prio: PRIO_DEFAULT,
            due: Input::default(),
            bucket: 0,
            bucket_names: board.buckets.iter().map(|b| b.name.clone()).collect(),
            bucket_keys: board.buckets.iter().map(|b| b.key.clone()).collect(),
            origin: None,
            tags: Input::default(),
            note: Input::default(),
            note_editable: true,
            tag_names: known_tags(board),
        }
    }

    /// `default_bucket` from the config preselects the bucket unless the filter decides.
    pub fn new_task(board: &Board, status: usize, default_bucket: Option<&str>) -> Form {
        let mut f = Form::base(board);
        f.status = status;
        let picked = bucket_for_new(None, &board.filter, &board.buckets, default_bucket)
            .ok()
            .flatten();
        if let Some((b, origin)) = picked {
            f.bucket = f
                .bucket_keys
                .iter()
                .position(|k| *k == b)
                .map_or(0, |i| i + 1);
            f.origin = (f.bucket > 0).then_some(origin);
        }
        f
    }

    pub fn edit_task(board: &Board, task: &Task) -> Form {
        let mut f = Form::base(board);
        f.editing = Some(task.id.clone());
        f.title = Input::with(&task.title);
        f.status = board
            .columns
            .iter()
            .position(|c| c.def.key == task.status)
            .unwrap_or(0);
        f.prio = task.prio_or_default();
        f.due = Input::with(&task.due.map(|d| d.to_string()).unwrap_or_default());
        f.bucket = task
            .bucket
            .as_ref()
            .and_then(|b| f.bucket_keys.iter().position(|k| k == b))
            .map_or(0, |i| i + 1);
        f.tags = Input::with(&task.tags.join(" "));
        f.note_editable = task.note_is_single_line();
        if f.note_editable {
            f.note = Input::with(task.note());
        }
        f
    }

    pub fn focused(&self) -> Field {
        self.fields[self.focus]
    }

    pub fn bucket_key(&self) -> Option<&str> {
        self.bucket
            .checked_sub(1)
            .map(|i| self.bucket_keys[i].as_str())
    }

    pub fn bucket_label(&self) -> String {
        match self.bucket.checked_sub(1) {
            Some(i) => self.bucket_names[i].clone(),
            None => t!("bucket.none").into_owned(),
        }
    }

    pub fn input_mut(&mut self) -> Option<&mut Input> {
        match self.focused() {
            Field::Title => Some(&mut self.title),
            Field::Due => Some(&mut self.due),
            Field::Tags => Some(&mut self.tags),
            Field::Note if self.note_editable => Some(&mut self.note),
            _ => None,
        }
    }

    /// The rest of a known tag (or, in the title, a bucket after `@`) for the word that
    /// ends at the cursor – shown greyed out and taken over with `→`.
    pub fn suggestion(&self) -> Option<&str> {
        let (input, in_title) = match self.focused() {
            Field::Title => (&self.title, true),
            Field::Tags => (&self.tags, false),
            _ => return None,
        };
        let (before, after) = input.text.split_at(input.byte_pos(input.cursor));
        if after.chars().next().is_some_and(|c| !c.is_whitespace()) {
            return None;
        }
        let word = before.rsplit(char::is_whitespace).next().unwrap_or("");
        let (prefix, candidates) = match (in_title, word.chars().next()) {
            (true, Some('+')) | (false, Some('+')) => (&word[1..], &self.tag_names),
            (true, Some('@')) => (&word[1..], &self.bucket_keys),
            (false, Some(_)) => (word, &self.tag_names),
            _ => return None,
        };
        if prefix.is_empty() {
            return None;
        }
        let words: Vec<&str> = input.text.split_whitespace().collect();
        let prefix = prefix.to_lowercase();
        candidates
            .iter()
            .filter(|c| c.len() > prefix.len() && c.starts_with(&prefix))
            // a tag that is already there is not suggested again
            .find(|c| {
                !words
                    .iter()
                    .any(|w| w.trim_start_matches('+') == c.as_str())
            })
            .map(|c| &c[prefix.len()..])
    }

    fn accept_suggestion(&mut self) -> bool {
        let Some(rest) = self.suggestion().map(str::to_owned) else {
            return false;
        };
        let Some(input) = self.input_mut() else {
            return false;
        };
        let at = input.byte_pos(input.cursor);
        let space = !input.text[at..].starts_with(' ');
        input
            .text
            .insert_str(at, &format!("{rest}{}", if space { " " } else { "" }));
        input.cursor += rest.chars().count() + 1;
        true
    }

    fn cycle(&mut self, delta: isize) {
        let wrap = |v: usize, n: usize| ((v as isize + delta).rem_euclid(n as isize)) as usize;
        match self.focused() {
            Field::Status => self.status = wrap(self.status, self.status_names.len()),
            Field::Prio => self.prio = wrap(self.prio as usize - 1, 3) as u8 + 1,
            Field::Bucket => {
                self.bucket = wrap(self.bucket, self.bucket_names.len() + 1);
                self.origin = None;
            }
            _ => {}
        }
    }

    /// Handles everything except Enter/Esc.
    pub fn handle(&mut self, key: KeyEvent) {
        let n = self.fields.len();
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus = (self.focus + 1) % n,
            KeyCode::BackTab | KeyCode::Up => self.focus = (self.focus + n - 1) % n,
            KeyCode::Right if self.accept_suggestion() => {}
            _ if self.focused().is_text() => {
                if let Some(input) = self.input_mut() {
                    input.handle(key);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.cycle(1),
            KeyCode::Char(c @ '1'..='3') if self.focused() == Field::Prio => {
                self.prio = c as u8 - b'0'
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Filter;
    use crate::store::parse_board;
    use ratatui::crossterm::event::KeyEvent;

    fn board(filter: &[&str]) -> Board {
        let cfg = parse_board(
            "[[buckets]]\nkey='home'\nname='Privat'\n[[buckets]]\nkey='work'\nname='Arbeit'",
        )
        .unwrap();
        let filter = Filter {
            buckets: filter.iter().map(|s| s.to_string()).collect(),
            ..Filter::default()
        };
        Board::build(cfg, vec![], vec![], filter)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    #[test]
    fn new_task_inherits_single_bucket() {
        let f = Form::new_task(&board(&["work"]), 1, None);
        assert_eq!(f.bucket_key(), Some("work"));
        assert_eq!(f.origin, Some(BucketOrigin::Filter));
        assert_eq!(f.status, 1);
        let f = Form::new_task(&board(&["work", "home"]), 0, None);
        assert_eq!(f.bucket_key(), None);
        assert_eq!(f.origin, None);
        let f = Form::new_task(&board(&["work", "home"]), 0, Some("home"));
        assert_eq!(f.bucket_key(), Some("home"));
        assert_eq!(f.origin, Some(BucketOrigin::Default));
    }

    fn typed(f: &mut Form, text: &str) {
        for ch in text.chars() {
            f.handle(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn suggests_known_tags_and_buckets() {
        let mut f = Form::new_task(&board(&[]), 0, None);
        f.tag_names = vec!["reporting".into(), "dev".into(), "deploy".into()];
        typed(&mut f, "Report +rep");
        assert_eq!(f.suggestion(), Some("orting"));
        f.handle(key(KeyCode::Right));
        assert_eq!(f.title.text, "Report +reporting ");
        assert_eq!(f.suggestion(), None);
        // most used first, already used tags are skipped
        typed(&mut f, "+de");
        assert_eq!(f.suggestion(), Some("v"));
        typed(&mut f, "v +d");
        assert_eq!(f.suggestion(), Some("eploy"));
        // buckets after @ in the title
        typed(&mut f, " @wo");
        assert_eq!(f.suggestion(), Some("rk"));
        // nothing for plain words, a complete tag or the cursor inside a word
        typed(&mut f, "rk");
        assert_eq!(f.suggestion(), None);
        f.handle(key(KeyCode::Left));
        assert_eq!(f.suggestion(), None);
        // without a suggestion → still moves the cursor
        f.handle(key(KeyCode::Right));
        assert_eq!(f.title.cursor, f.title.text.chars().count());
        // tags field: + is optional
        while f.focused() != Field::Tags {
            f.handle(key(KeyCode::Tab));
        }
        typed(&mut f, "dev re");
        assert_eq!(f.suggestion(), Some("porting"));
        f.handle(key(KeyCode::Right));
        assert_eq!(f.tags.text, "dev reporting ");
    }

    #[test]
    fn navigation_and_choices() {
        let mut f = Form::new_task(&board(&["work"]), 0, None);
        assert_eq!(f.fields.len(), 7);
        for ch in "Hallo h".chars() {
            f.handle(key(KeyCode::Char(ch)));
        }
        assert_eq!(f.title.text, "Hallo h");
        f.handle(key(KeyCode::Tab));
        f.handle(key(KeyCode::Tab));
        assert_eq!(f.focused(), Field::Prio);
        f.handle(key(KeyCode::Char('h')));
        assert_eq!(f.prio, 1);
        f.handle(key(KeyCode::Left));
        assert_eq!(f.prio, 3);
        f.handle(key(KeyCode::Char('2')));
        assert_eq!(f.prio, 2);
        f.handle(key(KeyCode::Down));
        f.handle(key(KeyCode::Down));
        assert_eq!(f.focused(), Field::Bucket);
        f.handle(key(KeyCode::Right));
        assert_eq!(f.bucket_key(), None);
        assert_eq!(f.origin, None);
        f.handle(key(KeyCode::BackTab));
        f.handle(key(KeyCode::Up));
        f.handle(key(KeyCode::Up));
        f.handle(key(KeyCode::Up));
        f.handle(key(KeyCode::Up));
        assert_eq!(f.focused(), Field::Note);
    }

    #[test]
    fn no_bucket_field_without_buckets() {
        let b = Board::build(parse_board("").unwrap(), vec![], vec![], Filter::all());
        let f = Form::new_task(&b, 0, None);
        assert!(!f.fields.contains(&Field::Bucket));
    }
}
