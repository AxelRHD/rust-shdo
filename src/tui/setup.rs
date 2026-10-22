//! First-run setup: language, data directory and public holidays.

use std::path::PathBuf;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use rust_i18n::t;

use super::form::Input;
use super::ui::{input_view, key_hints, popup};
use crate::config::{self, AppConfig};
use crate::holidays::{GERMAN_STATES, Region};
use crate::i18n::{self, Lang};
use crate::store::{DirState, Store};
use crate::theme::Palette;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Language,
    Dir,
    Country,
    State,
    Summary,
}

struct Setup {
    step: Step,
    lang: usize,
    dir: Input,
    /// The user saw the warning about a non-empty directory and wants it anyway.
    foreign_confirmed: bool,
    /// 0 = no holidays, 1 = Germany
    country: usize,
    state: usize,
    error: Option<String>,
}

enum Outcome {
    Continue,
    Abort,
    Done,
}

impl Setup {
    fn new(cfg: &AppConfig, cli_dir: Option<&std::path::Path>) -> Setup {
        let lang = cfg.lang();
        let dir = config::contract_tilde(&cfg.data_dir(cli_dir));
        let (country, state) = match cfg.holidays.as_deref() {
            Some(h) => {
                let code = h.trim().to_lowercase();
                let code = code.strip_prefix("de-").unwrap_or(&code).to_string();
                match GERMAN_STATES.iter().position(|(c, _, _)| *c == code) {
                    Some(i) => (1, i),
                    None => (0, 0),
                }
            }
            // No choice yet: German users most likely want German holidays
            None => (usize::from(lang == Lang::De), 0),
        };
        Setup {
            step: Step::Language,
            lang: Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0),
            dir: Input::with(&dir),
            foreign_confirmed: false,
            country,
            state,
            error: None,
        }
    }

    fn lang(&self) -> Lang {
        Lang::ALL[self.lang]
    }

    fn store(&self) -> Store {
        Store::new(config::expand_tilde(self.dir.text.trim()))
    }

    fn holidays(&self) -> Option<String> {
        (self.country == 1).then(|| Region::setting_for_state(GERMAN_STATES[self.state].0))
    }

    /// Steps shown to the user (the state only matters for Germany).
    fn steps(&self) -> Vec<Step> {
        let mut steps = vec![Step::Language, Step::Dir, Step::Country];
        if self.country == 1 {
            steps.push(Step::State);
        }
        steps.push(Step::Summary);
        steps
    }

    fn go(&mut self, delta: isize) {
        let steps = self.steps();
        let i = steps.iter().position(|s| *s == self.step).unwrap_or(0) as isize + delta;
        self.step = steps[i.clamp(0, steps.len() as isize - 1) as usize];
        self.error = None;
    }

    fn handle(&mut self, key: KeyEvent) -> Outcome {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Outcome::Abort;
        }
        let step_list = |len: usize, idx: &mut usize, code: KeyCode| match code {
            KeyCode::Char('j') | KeyCode::Down => *idx = (*idx + 1) % len,
            KeyCode::Char('k') | KeyCode::Up => *idx = (*idx + len - 1) % len,
            _ => {}
        };
        match (self.step, key.code) {
            (Step::Language, KeyCode::Esc) => return Outcome::Abort,
            (Step::Summary, KeyCode::Enter) => return Outcome::Done,
            (Step::Dir, KeyCode::Enter) => self.confirm_dir(),
            (_, KeyCode::Enter) => self.go(1),
            (_, KeyCode::Esc) => self.go(-1),
            (Step::Language, code) => {
                step_list(Lang::ALL.len(), &mut self.lang, code);
                // Switch right away so the following steps are in the chosen language
                i18n::set(self.lang());
            }
            (Step::Dir, _) => {
                self.dir.handle(key);
                self.foreign_confirmed = false;
                self.error = None;
            }
            (Step::Country, code) => step_list(2, &mut self.country, code),
            (Step::State, code) => step_list(GERMAN_STATES.len(), &mut self.state, code),
            (Step::Summary, _) => {}
        }
        Outcome::Continue
    }

    fn confirm_dir(&mut self) {
        if self.dir.text.trim().is_empty() {
            self.error = Some(t!("setup.dir.empty_input").into_owned());
            return;
        }
        match self.store().dir_state() {
            DirState::NotADir => self.error = Some(t!("setup.dir.not_a_dir").into_owned()),
            DirState::Foreign { .. } if !self.foreign_confirmed => self.foreign_confirmed = true,
            _ => self.go(1),
        }
    }

    fn result(&self) -> config::Setup {
        config::Setup {
            language: self.lang(),
            dir: self.dir.text.trim().to_string(),
            holidays: self.holidays(),
        }
    }
}

/// Describes the chosen directory: (text, is a warning).
fn dir_note(state: &DirState) -> (String, bool) {
    match state {
        DirState::Missing => (t!("setup.dir.missing").into_owned(), false),
        DirState::Empty => (t!("setup.dir.empty").into_owned(), false),
        DirState::Shdo => (t!("setup.dir.shdo").into_owned(), false),
        DirState::Foreign { entries, git: true } => {
            (t!("setup.dir.foreign_git", n = entries).into_owned(), true)
        }
        DirState::Foreign {
            entries,
            git: false,
        } => (t!("setup.dir.foreign", n = entries).into_owned(), true),
        DirState::NotADir => (t!("setup.dir.not_a_dir").into_owned(), true),
    }
}

/// Runs the setup dialog in the given terminal. Returns the choices, or `None` if aborted.
/// On success the config is written and the data directory is initialized.
pub fn run(
    terminal: &mut DefaultTerminal,
    cfg: &AppConfig,
    cfg_path: &std::path::Path,
    cli_dir: Option<PathBuf>,
    palette: &Palette,
) -> Result<Option<config::Setup>> {
    let mut setup = Setup::new(cfg, cli_dir.as_deref());
    i18n::set(setup.lang());
    loop {
        terminal.draw(|f| draw(f, &setup, palette, cfg_path))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match setup.handle(key) {
            Outcome::Continue => {}
            Outcome::Abort => return Ok(None),
            Outcome::Done => break,
        }
    }
    let result = setup.result();
    config::write_setup(cfg_path, &result)?;
    let store = setup.store();
    store.init(result.language)?;
    let git = crate::git::Git::new(&store.dir, true);
    if !git.is_repo() {
        git.init()?;
    }
    Ok(Some(result))
}

fn draw(f: &mut ratatui::Frame, s: &Setup, c: &Palette, cfg_path: &std::path::Path) {
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(c.bg).fg(c.fg)), area);
    let keys: Vec<(&str, &str)> = match s.step {
        Step::Language => vec![("j/k", "kb.select"), ("⏎", "kb.next"), ("Esc", "kb.quit")],
        Step::Dir => vec![("⏎", "kb.next"), ("Esc", "kb.back"), ("^U", "kb.clear")],
        Step::Country | Step::State => {
            vec![("j/k", "kb.select"), ("⏎", "kb.next"), ("Esc", "kb.back")]
        }
        Step::Summary => vec![("⏎", "kb.finish"), ("Esc", "kb.back")],
    };
    let hints = key_hints(&keys, c, area.width);
    let [_, bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(hints.len() as u16)]).areas(area);
    f.render_widget(Paragraph::new(hints), bar);

    let steps = s.steps();
    let n = steps.iter().position(|x| *x == s.step).unwrap_or(0) + 1;
    let title = format!(
        "{} · {}",
        t!("setup.title"),
        t!("setup.step", n = n, total = steps.len())
    );
    let (heading, hint) = match s.step {
        Step::Language => ("setup.lang.title", "setup.lang.hint"),
        Step::Dir => ("setup.dir.title", "setup.dir.hint"),
        Step::Country => ("setup.country.title", "setup.country.hint"),
        Step::State => ("setup.state.title", "setup.state.hint"),
        Step::Summary => ("setup.summary.title", "setup.summary.hint"),
    };
    let mut lines = vec![
        Line::from(Span::styled(
            t!(heading).into_owned(),
            Style::new().fg(c.title).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            t!(hint, path = config::contract_tilde(cfg_path)).into_owned(),
            Style::new().fg(c.dim),
        )),
        Line::default(),
    ];
    let choice = |sel: bool, text: String| {
        Line::from(vec![
            Span::styled(
                if sel { "→ " } else { "  " },
                Style::new().fg(c.border_active),
            ),
            Span::styled(
                text,
                if sel {
                    Style::new().add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                },
            ),
        ])
    };
    let mut cursor = None;
    match s.step {
        Step::Language => {
            for (i, l) in Lang::ALL.iter().enumerate() {
                lines.push(choice(i == s.lang, l.native_name().to_string()));
            }
        }
        Step::Dir => {
            let (visible, cx) = input_view(&s.dir, 52);
            cursor = Some((lines.len(), cx + 2));
            lines.push(Line::from(vec![
                Span::styled("▸ ", Style::new().fg(c.key)),
                Span::raw(visible),
            ]));
            lines.push(Line::default());
            let (note, warn) = dir_note(&s.store().dir_state());
            lines.push(Line::from(Span::styled(
                note,
                Style::new().fg(if warn { c.warn } else { c.done }),
            )));
            if s.foreign_confirmed {
                lines.push(Line::from(Span::styled(
                    t!("setup.dir.confirm").into_owned(),
                    Style::new().fg(c.warn).add_modifier(Modifier::BOLD),
                )));
            }
        }
        Step::Country => {
            lines.push(choice(
                s.country == 0,
                t!("setup.country.none").into_owned(),
            ));
            lines.push(choice(s.country == 1, t!("setup.country.de").into_owned()));
        }
        Step::State => {
            // Show a window around the selection so the list fits small terminals
            let visible = 8usize;
            let start = s
                .state
                .saturating_sub(visible / 2)
                .min(GERMAN_STATES.len() - visible);
            for (i, (_, _, name)) in GERMAN_STATES.iter().enumerate().skip(start).take(visible) {
                lines.push(choice(i == s.state, name.to_string()));
            }
        }
        Step::Summary => {
            let row = |k: &str, v: String| {
                Line::from(vec![
                    Span::styled(format!("{:<16}", t!(k)), Style::new().fg(c.dim)),
                    Span::raw(v),
                ])
            };
            lines.push(row("setup.summary.language", s.lang().native_name().into()));
            lines.push(row("setup.summary.dir", s.dir.text.trim().to_string()));
            let holidays = match s.holidays() {
                Some(_) => format!("{}, {}", t!("setup.country.de"), GERMAN_STATES[s.state].2),
                None => t!("setup.country.none").into_owned(),
            };
            lines.push(row("setup.summary.holidays", holidays));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                t!("setup.summary.later").into_owned(),
                Style::new().fg(c.dim),
            )));
        }
    }
    if let Some(e) = &s.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::new().fg(c.overdue),
        )));
    }
    // Height including wrapped lines: popup width 64 minus border and padding
    let text_width = 64u16
        .min(area.width.saturating_sub(2))
        .saturating_sub(4)
        .max(1);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let rows = paragraph.line_count(text_width) as u16;
    let inner = popup(f, c, &title, 64, rows + 2);
    f.render_widget(paragraph, inner);
    if let Some((row, col)) = cursor {
        f.set_cursor_position(Position {
            x: inner.x + col as u16,
            y: inner.y + row as u16,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    fn cfg(language: &str) -> AppConfig {
        toml::from_str(&format!("language = \"{language}\"")).unwrap()
    }

    #[test]
    fn german_preselects_german_holidays() {
        assert_eq!(Setup::new(&cfg("de"), None).country, 1);
        assert_eq!(Setup::new(&cfg("en"), None).country, 0);
    }

    #[test]
    fn walks_through_the_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("data");
        let mut s = Setup::new(&cfg("en"), Some(&dir));
        assert_eq!(s.step, Step::Language);
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Dir);
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Country);
        // Germany adds the state step
        s.handle(key(KeyCode::Down));
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::State);
        s.handle(key(KeyCode::Down));
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Summary);
        assert_eq!(s.holidays().as_deref(), Some("de-by"));
        assert!(matches!(s.handle(key(KeyCode::Enter)), Outcome::Done));
        // back to the country step and choose none: the state step disappears
        s.handle(key(KeyCode::Esc));
        s.handle(key(KeyCode::Esc));
        s.handle(key(KeyCode::Up));
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Summary);
        assert_eq!(s.holidays(), None);
    }

    #[test]
    fn foreign_directory_needs_confirmation() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "x").unwrap();
        let mut s = Setup::new(&cfg("en"), Some(tmp.path()));
        s.step = Step::Dir;
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Dir);
        assert!(s.foreign_confirmed);
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Country);
    }

    #[test]
    fn file_is_rejected_as_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let mut s = Setup::new(&cfg("en"), Some(&file));
        s.step = Step::Dir;
        s.handle(key(KeyCode::Enter));
        assert_eq!(s.step, Step::Dir);
        assert!(s.error.is_some());
    }
}
