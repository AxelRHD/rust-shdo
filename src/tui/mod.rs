//! Event loop, terminal setup and teardown.

mod app;
mod calendar;
mod form;
mod ui;

use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::config::AppConfig;
use crate::store::Store;
use app::App;

const TICK: Duration = Duration::from_millis(500);

pub fn run(store: Store, cfg: AppConfig, cfg_path: PathBuf) -> Result<()> {
    let mut app = App::new(store, cfg, cfg_path)?;
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let res = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    res
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(TICK)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.handle_key(key),
                _ => {}
            }
        }
        app.tick();
        if let Some(path) = app.editor_request.take() {
            let res = with_suspended(terminal, || crate::open_editor(&app.editor(), &path));
            app.after_editor(path, res);
        }
    }
    Ok(())
}

/// Leaves the TUI for an external program and restores it afterwards.
fn with_suspended<T>(terminal: &mut DefaultTerminal, f: impl FnOnce() -> T) -> T {
    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let out = f();
    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    let _ = terminal.clear();
    out
}
