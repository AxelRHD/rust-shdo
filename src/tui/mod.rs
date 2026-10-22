//! Event loop, terminal setup and teardown.

mod app;
mod calendar;
mod form;
mod setup;
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

/// Opens the board. With `setup`, the setup dialog runs first (first start or `shdo setup`).
pub fn run(cli_dir: Option<PathBuf>, cfg: AppConfig, cfg_path: PathBuf, setup: bool) -> Result<()> {
    crate::i18n::set(cfg.lang());
    // ratatui::init also installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let res = run_in(&mut terminal, cli_dir, cfg, cfg_path, setup);
    ratatui::restore();
    res
}

fn run_in(
    terminal: &mut DefaultTerminal,
    cli_dir: Option<PathBuf>,
    mut cfg: AppConfig,
    cfg_path: PathBuf,
    setup: bool,
) -> Result<()> {
    if setup {
        let palette = theme_for(&cfg);
        if setup::run(terminal, &cfg, &cfg_path, cli_dir.clone(), &palette)?.is_none() {
            return Ok(());
        }
        cfg = AppConfig::load(&cfg_path)?;
    }
    let store = Store::new(cfg.data_dir(cli_dir.as_deref()));
    store.ensure_initialized()?;
    let mut app = App::new(store, cfg, cfg_path)?;
    event_loop(terminal, &mut app)
}

/// Colors for the setup dialog: the configured theme or the default.
fn theme_for(cfg: &AppConfig) -> crate::theme::Palette {
    let (themes, _) = crate::theme::load_all(&crate::config::config_dir().join("themes"));
    let wanted = cfg.theme.as_deref().unwrap_or(crate::theme::DEFAULT_THEME);
    themes
        .iter()
        .find(|t| t.id == wanted)
        .or_else(|| themes.iter().find(|t| t.id == crate::theme::DEFAULT_THEME))
        .map(|t| t.c.clone())
        .expect("default theme exists")
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
