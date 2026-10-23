<p align="center">
  <img src="https://raw.githubusercontent.com/AxelRHD/rust-shdo/main/logo.svg" alt="shdo logo" width="424">
</p>

<p align="center">
  <strong>A kanban board for your terminal. Every task is a Markdown file in a git repository.</strong><br>
  <a href="https://github.com/AxelRHD/rust-shdo/actions/workflows/ci.yml"><img src="https://github.com/AxelRHD/rust-shdo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
</p>

shdo opens straight into a board, shows every available key at the bottom of the
screen and syncs between machines with plain git. No server, no database, no account –
just files you can read, edit and diff.

<p align="center">
  <img src="https://raw.githubusercontent.com/AxelRHD/rust-shdo/main/docs/demo.gif" alt="shdo demo: quick capture, moving a card, the task form, the bucket filter and the theme picker" width="900">
</p>

## Why shdo?

- **A program, not a pile of commands.** Start `shdo` and you are on your board, with
  vim-style navigation and a key bar that always tells you what you can do next.
- **Quick capture from the shell.** `shdo Pay invoice P1 due:fri @work` adds a task
  without opening anything.
- **Your data stays yours.** One Markdown file per task with a small TOML header. Edit
  it in any editor, grep it, keep notes and checklists in it.
- **Sync without conflicts.** Tasks live in a git repository; `s` (or `shdo sync`)
  commits, pulls and pushes. One file per task means merge conflicts practically never
  happen.
- **Deliberately small.** Statuses, priorities, due dates, buckets and notes. No
  assignees, projects, tags or story points.

## Features

- Kanban columns per status (Todo, Doing, Waiting, Done – configurable), sorted
  automatically by priority, due date and age
- Form for new and edited tasks with a live preview of recognized dates
- **Buckets** such as *home* and *work*, with a per-machine filter: show work tasks on
  the work laptop and home tasks at home, from the same repository
- **Tags** such as `+dev`, `+ops` across buckets, with their own filter
- **Calendar sidebar** with the number of tasks due per day and public holidays
  (Germany, all federal states)
- Markdown notes with checklists (`☐ 1/3` on the card)
- 7 built-in themes – Tokyo Night, Nord, Gruvbox Dark/Light, Catppuccin Mocha/Latte,
  Terminal – plus your own
- English and German interface
- Shell completion, with task IDs, statuses and buckets for fish

## Installation

shdo runs on Linux and macOS and needs `git`.

**Prebuilt binary** (Linux and macOS, x86_64 and ARM):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/AxelRHD/rust-shdo/releases/latest/download/shdo-installer.sh | sh
```

**With Cargo** (Rust 1.89 or newer):

```sh
cargo install shdo
```

**From source:**

```sh
git clone https://github.com/AxelRHD/rust-shdo
cd rust-shdo
cargo install --path .
```

## Getting started

Run `shdo`. On the first start a short setup asks for

1. the language of the board (English or German),
2. the data directory (default `~/.local/share/shdo`; an existing shdo directory is
   picked up as is),
3. optionally public holidays for the calendar.

You can change this later with `shdo setup`. To sync between machines, add a remote to
the data directory once:

```sh
cd ~/.local/share/shdo
git remote add origin git@github.com:you/tasks.git
```

On another machine, clone that repository and point the setup at it.

## The board

The key bar at the bottom always shows the keys for the current view; `?` shows all of
them.

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` / arrows | move between columns and cards |
| `H` / `L` | move the card to the previous / next status |
| `n` | new task (form) |
| `e` / `E` | edit in the form / open the file in your editor |
| `x` | mark as done / reopen |
| `1` `2` `3` | priority high / normal / low |
| `D`, `r`, `b`, `d` | due date, rename, bucket, delete |
| `⏎` | details with notes |
| `/` | search titles, notes and IDs (live; `Esc` clears) |
| `f` | bucket filter; create, rename and delete buckets |
| `+` | tag filter: show tasks with any of the checked tags |
| `v` / `c` | show or hide the done column / the calendar |
| `s` | sync with the git remote |
| `t` | choose a theme |
| `,` | open the settings (config.toml) in your editor |
| `q` | quit |

## Command line

The command line is always English and meant for quick capture and scripts.

```sh
shdo Deploy check +ops due:fri @work  # new task; P1–P3, due:, @bucket, +tag anywhere
shdo -- ls is broken                # title that starts with a command name
shdo ls                             # board as text (honours the bucket filter, -a for all)
shdo done a3f                       # IDs can be shortened while unique
shdo mv a3f doing                   # status by key or name
shdo bucket a3f home                # set a bucket, "-" removes it
shdo tag a3f dev ops                # set tags, "-" removes them
shdo ls @work +dev                  # show only these, once
shdo edit a3f                       # open the file in $EDITOR
shdo sync                           # git add, commit, pull --rebase, push
shdo archive                        # move done tasks to archive/
```

Dates for `due:` work in English and German: `2026-10-24`, `10/24`, `24.10.`, `+3d`,
`+2w`, `+1m`, `today`, `tomorrow`, `mon` … `sun` (the next one, never today).

## Files

A task is a Markdown file named after its ID:

```markdown
+++
title = "Quarterly report"
status = "todo"
prio = 1
due = 2026-10-26
bucket = "work"
tags = ["finance"]
created = 2026-10-22T10:41:00+02:00
moved = 2026-10-22T10:41:00+02:00
+++

Numbers are in the shared drive.

- [x] collect numbers
- [ ] write summary
- [ ] send to team
```

The data directory contains `shdo.toml` with the statuses and buckets, `tasks/` and
`archive/`. Statuses and buckets have a stable `key` used in the files and a `name` that
is only displayed, so renaming never touches your tasks. Comments and unknown fields in
these files are preserved when shdo writes them.

## Configuration

Per-machine settings live in `~/.config/shdo/config.toml` and are written by the setup,
the theme picker and the filter:

```toml
language = "en"          # "en" or "de"
dir = "~/.local/share/shdo"
editor = "hx"            # defaults to $VISUAL, then $EDITOR
theme = "nord"
buckets = ["work"]       # shown buckets; empty = all
default_bucket = "work"  # bucket for new tasks when the filter does not decide
tags = ["dev", "ops"]    # shown tags; empty = no tag filter
holidays = "de-bw"       # empty = none
show_done = true
show_calendar = true
auto_sync = false        # sync when the board opens and before it closes
```

With `auto_sync = true` the board syncs once in the background when it opens and once
when you quit – only if there is something to send. It never syncs on every change;
press `s` to sync in between. If the sync on quit fails, the board stays open with the
error, and pressing `q` again quits without syncing.

### Themes

Custom themes go into `~/.config/shdo/themes/<name>.toml` and appear in the theme picker.
Missing colors fall back to the default theme:

```toml
name = "My theme"

[colors]
bg = "#1a1b26"
fg = "#c0caf5"
border_active = "#9ece6a"
```

Available tokens: `bg`, `fg`, `dim`, `border`, `border_active`, `title`, `accent`,
`selection`, `prio_high`, `prio_low`, `overdue`, `today`, `done`, `key`, `warn`,
`weekend`, `holiday`. Colors are `#rrggbb` or ANSI names such as `red` or `reset`.

### Shell completion

```sh
shdo completions fish > ~/.config/fish/completions/shdo.fish
```

Run `shdo completions` without arguments for bash and zsh. Fish also completes task IDs
(with titles), statuses, buckets and `due:` values.

## License

MIT
