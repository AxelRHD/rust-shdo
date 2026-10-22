# Changelog

## 0.3.1 - 2026-10-22

- New values in config.toml (from the setup, the theme picker or the toggles) replace
  their commented example instead of landing above the file header

## 0.3.0 - 2026-10-22

- `,` opens the settings (config.toml) in the editor; changes apply when the editor
  closes (the data directory on the next start)
- The help lists where settings, tasks and custom themes live, `shdo setup`, and
  `Ctrl+C`
- The setup dialog puts the cursor on the input line when the text above wraps

## 0.2.0 - 2026-10-22

- Live search with `/` in titles, notes and IDs; works together with the bucket filter
  and the calendar counts, `Esc` clears it
- Optional auto-sync (`auto_sync = true`): once when the board opens and once before it
  closes if there is something to send, never on every change; a failed sync on quit
  keeps the board open
- Releases publish to crates.io via trusted publishing

## 0.1.0 - 2026-10-22

First release.

### Board

- Kanban columns per status (Todo, Doing, Waiting, Done – configurable in `shdo.toml`),
  sorted by priority, due date and age; the done column lists the most recent first
- Permanent, context-aware key bar and vim-style navigation
- Form for creating and editing tasks with a live preview of recognized dates and
  quick-add tokens in the title
- Buckets (e.g. home/work) with a per-machine filter; new tasks inherit the bucket when
  exactly one is shown; buckets can be created, renamed and deleted from the filter
- Calendar sidebar with the number of tasks due per day and German public holidays
- Detail view with Markdown notes; checklists are counted on the card
- Seven built-in themes (Tokyo Night, Nord, Gruvbox Dark/Light, Catppuccin
  Mocha/Latte, Terminal) and custom themes
- English and German interface; first-run setup for language, data directory and
  holidays (`shdo setup` to run it again)
- Auto-reload when task files change outside the board

### Command line

- Quick capture: `shdo Pay invoice P1 due:fri @work`
- `ls`, `done`, `mv`, `rm`, `edit`, `bucket`, `sync`, `archive`, `init`, `setup`
- ID prefixes, a guard against typos of command names
- Shell completion for fish, bash, zsh, elvish and PowerShell; fish also completes
  task IDs, statuses, buckets and `due:` values

### Data

- One Markdown file with TOML frontmatter per task in a git repository; comments and
  unknown fields are preserved
- `shdo sync`: add, commit, pull --rebase, push
