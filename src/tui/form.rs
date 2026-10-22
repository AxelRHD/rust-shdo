//! Eingabefelder und das Formular für neue bzw. bearbeitete Tasks.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::board::Board;
use crate::task::{PRIO_DEFAULT, Task};

#[derive(Default, Clone)]
pub struct Input {
    pub text: String,
    /// Cursor-Position in Zeichen (nicht Bytes).
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
    Note,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Titel",
            Field::Status => "Status",
            Field::Prio => "Prio",
            Field::Due => "Fällig",
            Field::Bucket => "Bucket",
            Field::Note => "Notiz",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(self, Field::Title | Field::Due | Field::Note)
    }
}

pub const PRIO_NAMES: [&str; 3] = ["hoch", "normal", "niedrig"];

pub struct Form {
    /// `None` = neuer Task, sonst ID des bearbeiteten Tasks.
    pub editing: Option<String>,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub title: Input,
    pub status: usize,
    pub status_names: Vec<String>,
    pub prio: u8,
    pub due: Input,
    /// 0 = ohne Bucket, sonst Index + 1 in `bucket_names`.
    pub bucket: usize,
    pub bucket_names: Vec<String>,
    pub bucket_keys: Vec<String>,
    /// Bucket wurde aus dem Filter übernommen und noch nicht angefasst.
    pub inherited: bool,
    pub note: Input,
    /// Mehrzeilige Notizen lassen sich nur im Editor bearbeiten.
    pub note_editable: bool,
}

impl Form {
    fn base(board: &Board) -> Form {
        let mut fields = vec![Field::Title, Field::Status, Field::Prio, Field::Due];
        if !board.buckets.is_empty() {
            fields.push(Field::Bucket);
        }
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
            inherited: false,
            note: Input::default(),
            note_editable: true,
        }
    }

    pub fn new_task(board: &Board, status: usize) -> Form {
        let mut f = Form::base(board);
        f.status = status;
        if let Some(b) = board.filter.inherited() {
            f.bucket = f
                .bucket_keys
                .iter()
                .position(|k| k == b)
                .map_or(0, |i| i + 1);
            f.inherited = f.bucket > 0;
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

    pub fn bucket_label(&self) -> &str {
        match self.bucket.checked_sub(1) {
            Some(i) => &self.bucket_names[i],
            None => "ohne Bucket",
        }
    }

    pub fn input_mut(&mut self) -> Option<&mut Input> {
        match self.focused() {
            Field::Title => Some(&mut self.title),
            Field::Due => Some(&mut self.due),
            Field::Note if self.note_editable => Some(&mut self.note),
            _ => None,
        }
    }

    fn cycle(&mut self, delta: isize) {
        let wrap = |v: usize, n: usize| ((v as isize + delta).rem_euclid(n as isize)) as usize;
        match self.focused() {
            Field::Status => self.status = wrap(self.status, self.status_names.len()),
            Field::Prio => self.prio = wrap(self.prio as usize - 1, 3) as u8 + 1,
            Field::Bucket => {
                self.bucket = wrap(self.bucket, self.bucket_names.len() + 1);
                self.inherited = false;
            }
            _ => {}
        }
    }

    /// Verarbeitet alles außer Enter/Esc.
    pub fn handle(&mut self, key: KeyEvent) {
        let n = self.fields.len();
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus = (self.focus + 1) % n,
            KeyCode::BackTab | KeyCode::Up => self.focus = (self.focus + n - 1) % n,
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
        };
        Board::build(cfg, vec![], vec![], filter)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    #[test]
    fn new_task_inherits_single_bucket() {
        let f = Form::new_task(&board(&["work"]), 1);
        assert_eq!(f.bucket_key(), Some("work"));
        assert!(f.inherited);
        assert_eq!(f.status, 1);
        let f = Form::new_task(&board(&["work", "home"]), 0);
        assert_eq!(f.bucket_key(), None);
        assert!(!f.inherited);
    }

    #[test]
    fn navigation_and_choices() {
        let mut f = Form::new_task(&board(&["work"]), 0);
        assert_eq!(f.fields.len(), 6);
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
        assert!(!f.inherited);
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
        let f = Form::new_task(&b, 0);
        assert!(!f.fields.contains(&Field::Bucket));
    }
}
