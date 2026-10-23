//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use rust_i18n::t;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Level, Mode, SETTINGS_ROWS, SettingsRow};
use super::calendar;
use super::form::{Field, Form, Input, prio_name};
use crate::board::BucketOrigin;
use crate::dates;
use crate::task::Task;
use crate::theme::Palette;

const MIN_COLUMN_WIDTH: u16 = 24;
const CARD_HEIGHT: u16 = 2;

/// Help sections: (section title key, [(keys, description key)]).
const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "help.nav",
        &[
            ("h / ←", "help.nav.left"),
            ("l / →", "help.nav.right"),
            ("j / ↓", "help.nav.down"),
            ("k / ↑", "help.nav.up"),
            ("g / G", "help.nav.first_last"),
            ("⏎", "help.nav.details"),
        ],
    ),
    (
        "help.cards",
        &[
            ("n", "help.cards.new"),
            ("e", "help.cards.edit"),
            ("E", "help.cards.editor"),
            ("H / ⇧←", "help.cards.move_left"),
            ("L / ⇧→", "help.cards.move_right"),
            ("x", "help.cards.done"),
            ("r", "help.cards.rename"),
            ("1 2 3", "help.cards.prio"),
            ("D", "help.cards.due"),
            ("b", "help.cards.bucket"),
            ("d", "help.cards.delete"),
        ],
    ),
    (
        "help.form",
        &[
            ("Tab ↓ / ⇧Tab ↑", "help.form.fields"),
            ("← → Space", "help.form.choice"),
            ("⏎", "help.form.save"),
            ("Esc", "help.form.discard"),
            ("P1 due: @", "help.form.tokens"),
            ("→", "help.form.complete"),
            ("⌫", "help.form.remove_tag"),
        ],
    ),
    (
        "help.buckets",
        &[
            ("f", "help.buckets.filter"),
            ("n / r / d", "help.buckets.manage"),
            ("Space / x", "help.buckets.toggle"),
            ("o", "help.buckets.only"),
            ("a", "help.buckets.all"),
            ("⏎ / Esc", "help.buckets.save"),
            ("", "help.buckets.inherit"),
        ],
    ),
    (
        "help.tags",
        &[
            ("+tag", "help.tags.add"),
            ("f ←/→", "help.tags.filter"),
            ("Space / o / a", "help.tags.keys"),
            ("/+dev", "help.tags.search"),
        ],
    ),
    (
        "help.general",
        &[
            ("/", "help.general.search"),
            ("s", "help.general.sync"),
            ("R", "help.general.reload"),
            ("v", "help.general.done_column"),
            ("c", "help.general.calendar"),
            ("t", "help.general.theme"),
            (",", "help.general.settings"),
            ("?", "help.general.help"),
            ("q / Esc", "help.general.quit"),
            ("Ctrl+C", "help.general.ctrl_c"),
            ("", "help.general.esc_search"),
        ],
    ),
    (
        "help.dates",
        &[
            ("2026-10-24", "help.dates.iso"),
            ("24.10. 10/24", "help.dates.day_month"),
            ("+3d +2w +1m", "help.dates.relative"),
            ("today tomorrow", "help.dates.words"),
            ("mon … sun", "help.dates.weekdays"),
        ],
    ),
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let c = app.theme().c.clone();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(c.bg).fg(c.fg)), area);

    let keys = keybar_lines(app, &c, area.width);
    let [header, body, status, keybar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(keys.len() as u16),
    ])
    .areas(area);

    draw_header(f, app, &c, header);
    // Calendar on the left as long as two board columns still fit next to it
    let min_board = MIN_COLUMN_WIDTH * 2.min(app.visible_columns() as u16);
    let body = if app.show_calendar && body.width >= calendar::WIDTH + min_board {
        let [cal, rest] =
            Layout::horizontal([Constraint::Length(calendar::WIDTH), Constraint::Min(0)])
                .areas(body);
        calendar::draw(f, app, &c, cal);
        rest
    } else {
        body
    };
    draw_board(f, app, &c, body);
    draw_status(f, app, &c, status);
    f.render_widget(Paragraph::new(keys), keybar);

    match &app.mode {
        Mode::Help { scroll } => draw_help(f, app, &c, *scroll),
        Mode::Themes { idx, .. } => draw_themes(f, app, &c, *idx),
        Mode::Detail { scroll } => draw_detail(f, app, &c, *scroll),
        Mode::ConfirmDelete => draw_confirm(f, app, &c),
        Mode::Form(form) => draw_form(f, app, &c, form),
        Mode::Filter { idx, checked, .. } => draw_filter(f, app, &c, *idx, checked),
        Mode::PickBucket { idx } => draw_pick_bucket(f, app, &c, *idx),
        Mode::TagFilter { idx, checked, .. } => draw_tag_filter(f, app, &c, *idx, checked),
        Mode::Settings {
            idx,
            remote,
            archivable,
        } => draw_settings(f, app, &c, *idx, remote.as_deref(), *archivable),
        Mode::ConfirmArchive { count, .. } => draw_confirm_archive(f, app, &c, *count),
        Mode::BucketNew {
            name,
            key,
            focus_key,
            ..
        } => draw_bucket_new(f, &c, name, key, *focus_key),
        Mode::BucketRename { key, input } => draw_bucket_rename(f, &c, key, input),
        Mode::BucketDelete {
            key,
            open,
            done,
            idx,
        } => draw_bucket_delete(f, app, &c, key, *open, *done, *idx),
        Mode::BucketMove { key, idx } => draw_bucket_move(f, app, &c, key, *idx),
        _ => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, c: &Palette, area: Rect) {
    let today = dates::today();
    let mut parts = vec![t!("hdr.open", n = app.board.open_count()).into_owned()];
    let overdue = app.board.overdue_count(today);
    if overdue > 0 {
        parts.push(t!("hdr.overdue", n = overdue).into_owned());
    }
    let last = &app.board.columns[app.board.last_index()];
    // Only worth mentioning if the hidden column actually holds something
    if !app.show_done && !last.tasks.is_empty() {
        parts.push(
            t!(
                "hdr.done_hidden",
                n = last.tasks.len(),
                name = last.def.name
            )
            .into_owned(),
        );
    }
    let filter = app.board.filter_label().map(|label| {
        let mut s = format!("⧩ {label}");
        if app.board.hidden > 0 {
            s.push_str(&format!(" · {}", t!("hdr.hidden", n = app.board.hidden)));
        }
        s
    });
    let left = Line::from(vec![
        Span::styled(" shdo ", Style::new().fg(c.title).bold()),
        Span::styled(
            truncate(
                &tilde_path(&app.store.dir),
                (area.width as usize).saturating_sub(40),
            ),
            Style::new().fg(c.dim),
        ),
    ]);
    let mut right = vec![Span::styled("[ ", Style::new().fg(c.dim))];
    if !app.query().is_empty() {
        right.push(Span::styled(
            format!("/ {}", truncate(app.query(), 24)),
            Style::new().fg(c.accent).bold(),
        ));
        right.push(Span::styled(" · ", Style::new().fg(c.dim)));
    }
    if let Some(filter) = filter {
        right.push(Span::styled(filter, Style::new().fg(c.key).bold()));
        right.push(Span::styled(" · ", Style::new().fg(c.dim)));
    }
    if let Some(tags) = app.board.tag_label() {
        right.push(Span::styled(tags, Style::new().fg(c.accent).bold()));
        right.push(Span::styled(" · ", Style::new().fg(c.dim)));
    }
    right.push(Span::styled(
        parts.join(" · "),
        Style::new().fg(if overdue > 0 { c.overdue } else { c.accent }),
    ));
    right.push(Span::styled(" ] ", Style::new().fg(c.dim)));
    let right = Line::from(right);
    let right_width = right.width() as u16;
    let left_area = Rect {
        width: area.width.saturating_sub(right_width + 1),
        ..area
    };
    f.render_widget(Paragraph::new(left), left_area);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn tilde_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

fn draw_board(f: &mut Frame, app: &mut App, c: &Palette, area: Rect) {
    let n = app.visible_columns();
    let visible = ((area.width / MIN_COLUMN_WIDTH) as usize).clamp(1, n);
    if app.col < app.col_offset {
        app.col_offset = app.col;
    } else if app.col >= app.col_offset + visible {
        app.col_offset = app.col + 1 - visible;
    }
    app.col_offset = app.col_offset.min(n - visible);

    let areas = Layout::horizontal(vec![Constraint::Ratio(1, visible as u32); visible]).split(area);
    for (slot, ci) in (app.col_offset..app.col_offset + visible).enumerate() {
        let more_left = slot == 0 && ci > 0;
        let more_right = slot == visible - 1 && ci < n - 1;
        draw_column(f, app, c, areas[slot], ci, more_left, more_right);
    }
}

fn draw_column(
    f: &mut Frame,
    app: &mut App,
    c: &Palette,
    area: Rect,
    ci: usize,
    more_left: bool,
    more_right: bool,
) {
    let active = ci == app.col;
    let is_last = ci == app.board.last_index();
    let column = &app.board.columns[ci];
    let len = column.tasks.len();
    let row = app.rows[ci];

    let title_style = if active {
        Style::new().fg(c.border_active).bold()
    } else {
        Style::new().fg(c.title)
    };
    let mut title = vec![];
    if more_left {
        title.push(Span::styled("◂ ", Style::new().fg(c.dim)));
    }
    title.push(Span::styled(column.def.name.clone(), title_style));
    // The done column says how far back it shows
    if is_last && let Some(days) = app.board.filter.done_days {
        title.push(Span::styled(
            format!(" · {}", t!("col.done_days", n = days)),
            Style::new().fg(c.dim),
        ));
    }
    let counter = if active && len > 0 {
        t!("col.position", n = row + 1, total = len).into_owned()
    } else {
        len.to_string()
    };
    let mut bottom = vec![];
    if is_last && app.board.old_done > 0 {
        bottom.push(Span::styled(
            format!("{} · ", t!("col.older", n = app.board.old_done)),
            Style::new().fg(c.dim),
        ));
    }
    bottom.push(Span::styled(
        counter,
        Style::new().fg(if active { c.fg } else { c.dim }),
    ));
    if more_right {
        bottom.push(Span::styled(" ▸", Style::new().fg(c.dim)));
    }
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(if active { c.border_active } else { c.border }))
        .title(Line::from(title))
        .title_bottom(Line::from(bottom).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);

    if len == 0 {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {}", t!("col.empty")),
                Style::new().fg(c.dim),
            )),
            inner,
        );
        return;
    }

    let fit = (inner.height / CARD_HEIGHT).max(1) as usize;
    let off = &mut app.offsets[ci];
    if row < *off {
        *off = row;
    } else if row >= *off + fit {
        *off = row + 1 - fit;
    }
    *off = (*off).min(len.saturating_sub(fit));
    let off = *off;

    let ctx = CardCtx {
        first: ci == 0,
        last: is_last,
        today: dates::today(),
        width: inner.width,
        show_bucket: !app.board.buckets.is_empty() && !app.board.filter.shows_single_bucket(),
    };
    let column = &app.board.columns[ci];
    for (i, task) in column.tasks.iter().enumerate().skip(off).take(fit) {
        let y = inner.y + ((i - off) as u16) * CARD_HEIGHT;
        let rect = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: CARD_HEIGHT.min(inner.bottom() - y),
        };
        let selected = active && i == row;
        if selected {
            f.buffer_mut().set_style(rect, Style::new().bg(c.selection));
        }
        let lines = card_lines(task, c, selected, &ctx);
        f.render_widget(Paragraph::new(lines), rect);
    }
}

struct CardCtx {
    first: bool,
    last: bool,
    today: jiff::civil::Date,
    width: u16,
    show_bucket: bool,
}

fn card_lines<'a>(t: &'a Task, c: &Palette, selected: bool, ctx: &CardCtx) -> Vec<Line<'a>> {
    let CardCtx {
        first,
        last,
        today,
        width,
        ..
    } = *ctx;
    let marker = if selected {
        Span::styled("→ ", Style::new().fg(c.border_active).bold())
    } else {
        Span::raw("  ")
    };
    let badge = if last {
        Span::styled("✓  ", Style::new().fg(c.done))
    } else {
        let p = t.prio_or_default();
        let style = match p {
            1 => Style::new().fg(c.prio_high).bold(),
            3 => Style::new().fg(c.prio_low),
            _ => Style::new().fg(c.fg),
        };
        Span::styled(format!("P{p} "), style)
    };
    let title_width = (width as usize).saturating_sub(5);
    let mut title_style = Style::new().fg(if last { c.dim } else { c.fg });
    if selected {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    let line1 = Line::from(vec![
        marker,
        badge,
        Span::styled(truncate(&t.title, title_width), title_style),
    ]);

    // Parts with priority: if the line does not fit, the least important go first.
    let mut parts: Vec<(u8, Span)> = vec![];
    if last {
        if let Some(ts) = t.done.or(t.mtime) {
            parts.push((
                0,
                Span::styled(dates::fmt_ago(ts, today), Style::new().fg(c.dim)),
            ));
        }
    } else {
        if let Some(d) = t.due {
            let style = if d < today {
                Style::new().fg(c.overdue).bold()
            } else if d == today {
                Style::new().fg(c.today).bold()
            } else {
                Style::new().fg(c.accent)
            };
            parts.push((0, Span::styled(dates::fmt_due(d, today), style)));
        }
        if !first && let Some(m) = t.moved {
            parts.push((
                2,
                Span::styled(dates::fmt_since(m, today), Style::new().fg(c.dim)),
            ));
        }
    }
    if let Some((done, total)) = t.checklist() {
        let color = if done == total { c.done } else { c.dim };
        parts.push((
            1,
            Span::styled(format!("☐ {done}/{total}"), Style::new().fg(color)),
        ));
    } else if !t.note().is_empty() {
        parts.push((3, Span::styled("≡", Style::new().fg(c.dim))));
    }
    if ctx.show_bucket
        && let Some(b) = &t.bucket
    {
        parts.push((1, Span::styled(format!("@{b}"), Style::new().fg(c.key))));
    }
    if !t.tags.is_empty() {
        let tags: Vec<String> = t.tags.iter().map(|tag| format!("+{tag}")).collect();
        parts.push((2, Span::styled(tags.join(" "), Style::new().fg(c.accent))));
    }
    const INDENT: &str = "     ";
    let avail = (width as usize).saturating_sub(INDENT.len());
    let total = |parts: &[(u8, Span)]| {
        parts.iter().map(|(_, s)| s.width()).sum::<usize>() + parts.len().saturating_sub(1) * 3
    };
    while parts.len() > 1 && total(&parts) > avail {
        let worst = (0..parts.len()).max_by_key(|&i| parts[i].0).unwrap_or(0);
        parts.remove(worst);
    }
    let mut meta = vec![Span::raw(INDENT)];
    for (i, (_, span)) in parts.into_iter().enumerate() {
        if i > 0 {
            meta.push(Span::styled(" · ", Style::new().fg(c.dim)));
        }
        meta.push(span);
    }
    vec![line1, Line::from(meta)]
}

fn truncate(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn draw_status(f: &mut Frame, app: &App, c: &Palette, area: Rect) {
    if let Mode::Search { input } = &app.mode {
        let label = " / ";
        let hits = format!("   {}", t!("search.hits", n = app.shown_count()));
        let line = Line::from(vec![
            Span::styled(label, Style::new().fg(c.key).bold()),
            Span::styled(input.text.clone(), Style::new().fg(c.fg)),
            Span::styled(hits, Style::new().fg(c.dim)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        let before: String = input.text.chars().take(input.cursor).collect();
        let x = area.x + (label.width() + before.width()) as u16;
        f.set_cursor_position(Position {
            x: x.min(area.right().saturating_sub(1)),
            y: area.y,
        });
        return;
    }
    if let Mode::Input { kind, input } = &app.mode {
        let label = format!(" {}: ", kind.label());
        let hint = match kind {
            super::app::InputKind::Due => t!("input.due_hint"),
            super::app::InputKind::Rename => "".into(),
        };
        let line = Line::from(vec![
            Span::styled(label.clone(), Style::new().fg(c.key).bold()),
            Span::styled(input.text.clone(), Style::new().fg(c.fg)),
            Span::styled(hint, Style::new().fg(c.dim)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        let before: String = input.text.chars().take(input.cursor).collect();
        let x = area.x + (label.width() + before.width()) as u16;
        f.set_cursor_position(Position {
            x: x.min(area.right().saturating_sub(1)),
            y: area.y,
        });
        return;
    }
    let line = if let Some((level, msg)) = &app.status {
        let color = match level {
            Level::Info => c.accent,
            Level::Ok => c.done,
            Level::Error => c.overdue,
        };
        let first = msg
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ⏎ ");
        Line::from(Span::styled(format!(" {first}"), Style::new().fg(color)))
    } else {
        let n = app.warnings().count();
        if n > 0 {
            Line::from(Span::styled(
                format!(" ⚠ {}", t!("status.warnings", n = n)),
                Style::new().fg(c.warn),
            ))
        } else {
            Line::default()
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/// Key bar; wraps onto a second line if one is not enough.
fn keybar_lines(app: &App, c: &Palette, width: u16) -> Vec<Line<'static>> {
    let has_buckets = !app.board.buckets.is_empty();
    let keys: Vec<(&str, &str)> = match &app.mode {
        Mode::Normal => {
            let mut k = vec![
                ("h/l", "kb.column"),
                ("j/k", "kb.card"),
                ("H/L", "kb.move"),
                ("n", "kb.new"),
                ("e", "kb.edit"),
                ("E", "kb.editor"),
                ("x", "kb.done"),
                ("⏎", "kb.details"),
            ];
            k.push((
                "f",
                if has_buckets {
                    "kb.filter"
                } else {
                    "kb.buckets"
                },
            ));
            if has_buckets {
                k.push(("b", "kb.bucket"));
            }
            k.push(("v", "kb.toggle_done"));
            k.push(("c", "kb.calendar"));
            k.push(("/", "kb.search"));
            k.extend([
                ("s", "kb.sync"),
                ("t", "kb.theme"),
                (",", "kb.settings"),
                ("?", "kb.help"),
                ("q", "kb.quit"),
            ]);
            k
        }
        Mode::Input { .. } => vec![("⏎", "kb.save"), ("Esc", "kb.cancel"), ("^U", "kb.clear")],
        Mode::Search { .. } => vec![("⏎", "kb.apply"), ("Esc", "kb.clear_search")],
        Mode::Form(form) => {
            let mut k = vec![("Tab/↓", "kb.next_field"), ("⇧Tab/↑", "kb.prev_field")];
            if !form.focused().is_text() {
                k.push(("←/→", "kb.change"));
            } else if form.suggestion().is_some() {
                k.push(("→", "kb.complete"));
            }
            k.extend([("⏎", "kb.save"), ("Esc", "kb.discard")]);
            k
        }
        Mode::ConfirmDelete => vec![("y/j", "kb.delete"), ("n/Esc", "kb.cancel")],
        Mode::Help { .. } => vec![("j/k", "kb.scroll"), ("Esc", "kb.close")],
        Mode::Themes { .. } => vec![
            ("j/k", "kb.select"),
            ("⏎", "kb.apply"),
            ("Esc", "kb.cancel"),
        ],
        Mode::Detail { .. } => vec![
            ("j/k", "kb.scroll"),
            ("e", "kb.edit"),
            ("E", "kb.editor"),
            ("Esc", "kb.close"),
        ],
        Mode::Filter { .. } => vec![
            ("←/→", "kb.tab"),
            ("j/k", "kb.select"),
            ("Space", "kb.toggle"),
            ("o", "kb.only"),
            ("a", "kb.all"),
            ("n", "kb.new_bucket"),
            ("r", "kb.rename"),
            ("d", "kb.delete"),
            ("⏎", "kb.save"),
            ("Esc", "kb.discard"),
        ],
        Mode::BucketNew { .. } => vec![
            ("Tab", "kb.name_key"),
            ("⏎", "kb.create"),
            ("Esc", "kb.cancel"),
        ],
        Mode::BucketRename { .. } => vec![("⏎", "kb.save"), ("Esc", "kb.cancel")],
        Mode::BucketDelete { .. } | Mode::BucketMove { .. } => {
            vec![("j/k", "kb.select"), ("⏎", "kb.run"), ("Esc", "kb.cancel")]
        }
        Mode::Settings { .. } => vec![
            ("j/k", "kb.select"),
            ("←/→", "kb.change"),
            ("⏎", "kb.run"),
            ("Esc", "kb.close"),
        ],
        Mode::ConfirmArchive { .. } => vec![("y/j", "kb.archive"), ("n/Esc", "kb.cancel")],
        Mode::TagFilter { .. } => vec![
            ("←/→", "kb.tab"),
            ("j/k", "kb.select"),
            ("Space", "kb.toggle"),
            ("o", "kb.only"),
            ("a", "kb.no_tag_filter"),
            ("⏎", "kb.save"),
            ("Esc", "kb.discard"),
        ],
        Mode::PickBucket { .. } => {
            vec![("j/k", "kb.select"), ("⏎", "kb.set"), ("Esc", "kb.cancel")]
        }
    };
    key_hints(&keys, c, width)
}

/// Renders `[key → label]` hints (labels are translation keys), wrapping onto a second
/// line when one is not enough.
pub(super) fn key_hints(keys: &[(&str, &str)], c: &Palette, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![vec![Span::raw(" ")]];
    let mut used = 1;
    for &(k, label) in keys {
        let label = t!(label);
        let item_width = k.width() + label.width() + 6;
        if used + item_width > width as usize && used > 1 && lines.len() < 2 {
            lines.push(vec![Span::raw(" ")]);
            used = 1;
        }
        let line = lines.last_mut().expect("at least one line");
        line.push(Span::styled("[", Style::new().fg(c.dim)));
        line.push(Span::styled(k.to_string(), Style::new().fg(c.key)));
        line.push(Span::styled(format!(" → {label}]"), Style::new().fg(c.dim)));
        line.push(Span::raw(" "));
        used += item_width;
    }
    lines.into_iter().map(Line::from).collect()
}

pub(super) fn popup(f: &mut Frame, c: &Palette, title: &str, width: u16, height: u16) -> Rect {
    let area = f.area();
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(c.border_active))
        .style(Style::new().bg(c.bg).fg(c.fg))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::new().fg(c.border_active).bold(),
        )));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

fn draw_help(f: &mut Frame, app: &App, c: &Palette, scroll: u16) {
    let mut lines = vec![];
    for (section, keys) in HELP {
        lines.push(Line::from(Span::styled(
            t!(*section).into_owned(),
            Style::new().fg(c.title).bold(),
        )));
        for (k, text) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<16}"), Style::new().fg(c.key)),
                Span::raw(t!(*text).into_owned()),
            ]));
        }
        lines.push(Line::default());
    }
    // Where things live, with the real paths of this machine
    lines.push(Line::from(Span::styled(
        t!("help.files").into_owned(),
        Style::new().fg(c.title).bold(),
    )));
    let files = [
        ("help.files.config", tilde_path(app.cfg_path())),
        ("help.files.state", tilde_path(&crate::state::path())),
        ("help.files.data", tilde_path(&app.store.dir)),
        (
            "help.files.themes",
            tilde_path(&crate::config::config_dir().join("themes")),
        ),
    ];
    for (k, path) in files {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<16}", t!(k)), Style::new().fg(c.key)),
            Span::raw(path),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<16}", "shdo setup"), Style::new().fg(c.key)),
        Span::raw(t!("help.files.setup").into_owned()),
    ]));
    lines.push(Line::default());
    let warnings: Vec<_> = app.warnings().collect();
    if !warnings.is_empty() {
        lines.push(Line::from(Span::styled(
            t!("help.warnings").into_owned(),
            Style::new().fg(c.warn).bold(),
        )));
        for w in warnings {
            lines.push(Line::from(Span::styled(
                format!("  {w}"),
                Style::new().fg(c.warn),
            )));
        }
    }
    let inner = popup(f, c, &t!("help.title"), 72, lines.len() as u16 + 2);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn draw_themes(f: &mut Frame, app: &App, c: &Palette, idx: usize) {
    let lines: Vec<Line> = app
        .themes
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let sel = i == idx;
            let swatch: Vec<Span> = [
                t.c.title,
                t.c.border_active,
                t.c.prio_high,
                t.c.today,
                t.c.key,
            ]
            .into_iter()
            .map(|col| Span::styled("●", Style::new().fg(col)))
            .collect();
            let mut spans = vec![
                Span::styled(
                    if sel { "→ " } else { "  " },
                    Style::new().fg(c.border_active),
                ),
                Span::styled(
                    format!("{:<20}", t.name),
                    if sel {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                ),
            ];
            spans.extend(swatch);
            Line::from(spans)
        })
        .collect();
    let inner = popup(f, c, &t!("theme.title"), 36, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_detail(f: &mut Frame, app: &App, c: &Palette, scroll: u16) {
    let Some(t) = app.selected() else { return };
    let today = dates::today();
    let column = app
        .board
        .columns
        .iter()
        .find(|col| col.def.key == t.status)
        .map_or(t.status.as_str(), |col| col.def.name.as_str());
    let field = |k: &str, v: String| {
        Line::from(vec![
            Span::styled(format!("{k:<14}"), Style::new().fg(c.dim)),
            Span::raw(v),
        ])
    };
    let mut lines = vec![
        Line::from(Span::styled(
            t.title.clone(),
            Style::new().fg(c.title).bold(),
        )),
        Line::default(),
        field("ID", t.id.clone()),
        field(&t!("detail.status"), column.to_string()),
        field(&t!("detail.prio"), format!("P{}", t.prio_or_default())),
    ];
    if let Some(b) = &t.bucket {
        lines.push(field(
            &t!("detail.bucket"),
            format!("{} (@{b})", app.board.bucket_name(b)),
        ));
    }
    if !t.tags.is_empty() {
        let tags: Vec<String> = t.tags.iter().map(|tag| format!("+{tag}")).collect();
        lines.push(field(&t!("detail.tags"), tags.join(" ")));
    }
    if let Some(d) = t.due {
        lines.push(field(
            &t!("detail.due"),
            format!("{} ({})", dates::fmt_date(d), dates::fmt_due(d, today)),
        ));
    }
    for (k, v) in [
        (t!("detail.created"), t.created),
        (t!("detail.moved"), t.moved),
        (t!("detail.done"), t.done),
    ] {
        if let Some(ts) = v {
            lines.push(field(&k, dates::fmt_timestamp(ts)));
        }
    }
    if !t.note().is_empty() {
        lines.push(Line::default());
        for l in t.note().lines() {
            let style = if l.trim_start().starts_with('#') {
                Style::new().fg(c.accent).bold()
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(l.to_string(), style)));
        }
    }
    let height = (lines.len() as u16 + 2).max(10);
    let inner = popup(f, c, &t!("detail.title"), 72, height);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn draw_confirm(f: &mut Frame, app: &App, c: &Palette) {
    let Some(t) = app.selected() else { return };
    let inner = popup(f, c, &t!("delete.title"), 50, 5);
    let lines = vec![
        Line::from(truncate(&t.title, inner.width as usize)),
        Line::default(),
        Line::from(vec![
            Span::styled(
                t!("kb.yes_key").into_owned(),
                Style::new().fg(c.overdue).bold(),
            ),
            Span::raw(format!(" {}   ", t!("kb.delete"))),
            Span::styled("n", Style::new().fg(c.key)),
            Span::raw(format!(" {}", t!("kb.cancel"))),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

/// Visible slice of an input field and the cursor column within it.
pub(super) fn input_view(input: &Input, width: usize) -> (String, usize) {
    let chars: Vec<char> = input.text.chars().collect();
    let width = width.max(2);
    let mut start = 0;
    let cursor_w = |start: usize| -> usize {
        chars[start..input.cursor]
            .iter()
            .map(|ch| unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0))
            .sum()
    };
    while cursor_w(start) >= width {
        start += 1;
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in &chars[start..] {
        let cw = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(*ch);
        w += cw;
    }
    (out, cursor_w(start))
}

/// Like [`input_view`], with `ghost` shown greyed out at the cursor (a suggestion).
fn input_spans(
    input: &Input,
    ghost: Option<&str>,
    width: usize,
    c: &Palette,
) -> (Vec<Span<'static>>, usize) {
    let Some(ghost) = ghost else {
        let (visible, cx) = input_view(input, width);
        return (vec![Span::styled(visible, Style::new().fg(c.fg))], cx);
    };
    let at = input
        .text
        .char_indices()
        .nth(input.cursor)
        .map_or(input.text.len(), |(i, _)| i);
    let mut joined = input.text.clone();
    joined.insert_str(at, ghost);
    let shown = Input {
        text: joined,
        cursor: input.cursor,
    };
    let (visible, cx) = input_view(&shown, width);
    // `visible` starts where the cursor still fits; find the ghost inside it
    let mut spans = vec![];
    let (mut w, mut part, mut in_ghost) = (0, String::new(), false);
    let ghost_w = unicode_width::UnicodeWidthStr::width(ghost);
    for ch in visible.chars() {
        let now_ghost = w >= cx && w < cx + ghost_w;
        if now_ghost != in_ghost && !part.is_empty() {
            let fg = if in_ghost { c.dim } else { c.fg };
            spans.push(Span::styled(std::mem::take(&mut part), Style::new().fg(fg)));
        }
        in_ghost = now_ghost;
        part.push(ch);
        w += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    let fg = if in_ghost { c.dim } else { c.fg };
    spans.push(Span::styled(part, Style::new().fg(fg)));
    (spans, cx)
}

/// Tags field: finished tags as pills (unknown ones in the warning colour), then the word
/// being typed with its suggestion. `None` when the cursor is not at the end or the pills
/// do not fit – the field is then shown as plain text.
fn tag_pills(form: &Form, width: usize, c: &Palette) -> Option<(Vec<Span<'static>>, usize)> {
    let input = &form.tags;
    if input.cursor != input.text.chars().count() {
        return None;
    }
    let (done, typing) = match input.text.rfind(char::is_whitespace) {
        Some(i) => input.text.split_at(i + 1),
        None => ("", input.text.as_str()),
    };
    let mut spans = vec![];
    let mut w = 0;
    for tag in done.split_whitespace() {
        let fg = if form.is_known_tag(tag) {
            c.accent
        } else {
            c.warn
        };
        let pill = format!(" {} ", tag.trim_start_matches('+'));
        w += unicode_width::UnicodeWidthStr::width(pill.as_str()) + 1;
        spans.push(Span::styled(pill, Style::new().fg(fg).bg(c.selection)));
        spans.push(Span::raw(" "));
    }
    let typed = unicode_width::UnicodeWidthStr::width(typing);
    let ghost = form.suggestion().unwrap_or("");
    if w + typed + unicode_width::UnicodeWidthStr::width(ghost) >= width {
        return None;
    }
    spans.push(Span::styled(typing.to_string(), Style::new().fg(c.fg)));
    if !ghost.is_empty() {
        spans.push(Span::styled(ghost.to_string(), Style::new().fg(c.dim)));
    }
    Some((spans, w + typed))
}

fn draw_form(f: &mut Frame, app: &App, c: &Palette, form: &Form) {
    const LABEL: usize = 10;
    let today = dates::today();
    let title = match &form.editing {
        None => t!("form.title_new").into_owned(),
        Some(id) => t!("form.title_edit", id = id).into_owned(),
    };
    // Title, hint line, remaining fields, border
    let inner = popup(f, c, &title, 70, form.fields.len() as u16 + 3);
    let value_width = (inner.width as usize).saturating_sub(LABEL + 2);
    let mut lines: Vec<Line> = vec![];
    let mut cursor: Option<Position> = None;
    // Title tokens override the fields on save – show that next to each field.
    let tokens = crate::quickadd::parse_str(&form.title.text, today).ok();
    let overrides = |c: &Palette, text: String| {
        Span::styled(
            format!("  ← {}", t!("form.from_title", value = text)),
            Style::new().fg(c.accent),
        )
    };

    let choice = |text: String, style: Style, focused: bool| -> Vec<Span<'static>> {
        let arrow = Style::new().fg(if focused { c.border_active } else { c.dim });
        vec![
            Span::styled("‹ ", arrow),
            Span::styled(text, style),
            Span::styled(" ›", arrow),
        ]
    };

    for (i, field) in form.fields.iter().enumerate() {
        let focused = i == form.focus;
        let label_style = if focused {
            Style::new().fg(c.key).bold()
        } else {
            Style::new().fg(c.dim)
        };
        let mut spans = vec![
            Span::styled(if focused { "▸ " } else { "  " }, label_style),
            Span::styled(format!("{:<LABEL$}", field.label()), label_style),
        ];
        let y = inner.y + lines.len() as u16;
        let x0 = inner.x + (LABEL + 2) as u16;
        let ghost = if focused { form.suggestion() } else { None };
        let mut text_field = |input: &Input, spans: &mut Vec<Span<'static>>, width: usize| {
            let (parts, cx) = input_spans(input, ghost, width, c);
            spans.extend(parts);
            if focused {
                cursor = Some(Position {
                    x: x0 + cx as u16,
                    y,
                });
            }
        };
        match field {
            Field::Title => text_field(&form.title, &mut spans, value_width),
            Field::Due => {
                let from_title = tokens.as_ref().and_then(|q| q.due);
                let preview = if let Some(d) = from_title {
                    overrides(c, dates::fmt_date(d))
                } else if form.due.text.trim().is_empty() {
                    Span::styled(
                        format!("  {}", t!("form.due_empty")),
                        Style::new().fg(c.dim),
                    )
                } else {
                    match dates::parse_date(&form.due.text, today) {
                        Ok(d) => Span::styled(
                            format!("  → {} ({})", dates::fmt_date(d), dates::fmt_due(d, today)),
                            Style::new().fg(c.accent),
                        ),
                        Err(_) => Span::styled(
                            format!("  ✗ {}", t!("form.due_unknown")),
                            Style::new().fg(c.overdue),
                        ),
                    }
                };
                let w = value_width.saturating_sub(preview.width()).max(8);
                text_field(&form.due, &mut spans, w);
                let used: usize = spans[2..].iter().map(|s| s.width()).sum();
                spans.push(Span::raw(" ".repeat(w.saturating_sub(used))));
                spans.push(preview);
            }
            Field::Tags => {
                let width = value_width.saturating_sub(24);
                match tag_pills(form, width, c) {
                    Some((parts, cx)) => {
                        spans.extend(parts);
                        if focused {
                            cursor = Some(Position {
                                x: x0 + cx as u16,
                                y,
                            });
                        }
                    }
                    None => text_field(&form.tags, &mut spans, width),
                }
                let from_title = tokens.as_ref().map(|q| q.tags.clone()).unwrap_or_default();
                if !from_title.is_empty() {
                    let tags: Vec<String> = from_title.iter().map(|t| format!("+{t}")).collect();
                    spans.push(overrides(c, tags.join(" ")));
                } else {
                    // finished tags no task uses yet – likely a typo
                    let text = &form.tags.text;
                    let done = &text[..text.rfind(char::is_whitespace).map_or(0, |i| i + 1)];
                    let new: Vec<&str> = done
                        .split_whitespace()
                        .filter(|t| !form.is_known_tag(t))
                        .map(|t| t.trim_start_matches('+'))
                        .collect();
                    if !new.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", t!("form.new_tags", tags = new.join(" "))),
                            Style::new().fg(c.warn),
                        ));
                    }
                }
            }
            Field::Note if form.note_editable => text_field(&form.note, &mut spans, value_width),
            Field::Note => spans.push(Span::styled(
                t!("form.note_multiline").into_owned(),
                Style::new().fg(c.dim),
            )),
            Field::Status => spans.extend(choice(
                form.status_names[form.status].clone(),
                Style::new().fg(c.fg),
                focused,
            )),
            Field::Prio => {
                let style = match form.prio {
                    1 => Style::new().fg(c.prio_high).bold(),
                    3 => Style::new().fg(c.prio_low),
                    _ => Style::new().fg(c.fg),
                };
                spans.extend(choice(
                    format!("P{} {}", form.prio, prio_name(form.prio)),
                    style,
                    focused,
                ));
                if let Some(p) = tokens.as_ref().and_then(|q| q.prio) {
                    spans.push(overrides(c, format!("P{p}")));
                }
            }
            Field::Bucket => {
                let style = if form.bucket == 0 {
                    Style::new().fg(c.dim)
                } else {
                    Style::new().fg(c.key).bold()
                };
                spans.extend(choice(form.bucket_label(), style, focused));
                if let Some(b) = tokens.as_ref().and_then(|q| q.bucket.as_ref()) {
                    spans.push(overrides(c, format!("@{b}")));
                } else if let Some(origin) = form.origin {
                    let key = match origin {
                        BucketOrigin::Default => "form.default_bucket",
                        _ => "form.from_filter",
                    };
                    spans.push(Span::styled(
                        format!("  {}", t!(key)),
                        Style::new().fg(c.key),
                    ));
                }
            }
        }
        lines.push(Line::from(spans));
        if *field == Field::Title {
            lines.push(title_hint(app, c, &form.title.text, today, LABEL + 2));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(pos) = cursor {
        f.set_cursor_position(pos);
    }
}

/// Shows what the title tokens do (`P1`, `due:…`, `@…`).
fn title_hint(
    app: &App,
    c: &Palette,
    text: &str,
    today: jiff::civil::Date,
    indent: usize,
) -> Line<'static> {
    let pad = Span::raw(" ".repeat(indent));
    let has_token = text.split_whitespace().any(|w| {
        w.starts_with("due:")
            || (w.starts_with('@') && w.len() > 1)
            || (w.starts_with('+') && w[1..].starts_with(char::is_alphabetic))
            || matches!(w, "P1" | "P2" | "P3" | "p1" | "p2" | "p3")
    });
    if !has_token {
        return Line::from(vec![
            pad,
            Span::styled(
                // Only mention @bucket when there are buckets to pick from
                if app.board.buckets.is_empty() {
                    t!("form.tokens_hint_no_buckets").into_owned()
                } else {
                    t!("form.tokens_hint").into_owned()
                },
                Style::new().fg(c.dim),
            ),
        ]);
    }
    let q = match crate::quickadd::parse_str(text, today) {
        Ok(q) => q,
        Err(e) => {
            return Line::from(vec![
                pad,
                Span::styled(format!("✗ {e:#}"), Style::new().fg(c.overdue)),
            ]);
        }
    };
    let mut parts = vec![];
    if let Some(p) = q.prio {
        parts.push(format!("P{p}"));
    }
    if let Some(d) = q.due {
        parts.push(dates::fmt_due(d, today));
    }
    if let Some(b) = &q.bucket {
        match crate::board::resolve_bucket(&app.board.buckets, b) {
            Ok(key) => parts.push(format!("@{key}")),
            Err(e) => {
                return Line::from(vec![
                    pad,
                    Span::styled(format!("✗ {e:#}"), Style::new().fg(c.overdue)),
                ]);
            }
        }
    }
    for tag in &q.tags {
        parts.push(format!("+{tag}"));
    }
    Line::from(vec![
        pad,
        Span::styled(
            t!("form.recognized", parts = parts.join(" · ")).into_owned(),
            Style::new().fg(c.accent),
        ),
    ])
}

/// Tab line of the filter popup: Buckets | Tags, switched with ←/→.
fn filter_tabs(c: &Palette, tags_active: bool) -> Line<'static> {
    let tab = |label: String, active: bool| {
        if active {
            Span::styled(
                format!(" {label} "),
                Style::new().fg(c.bg).bg(c.border_active).bold(),
            )
        } else {
            Span::styled(format!(" {label} "), Style::new().fg(c.dim))
        }
    };
    Line::from(vec![
        Span::raw(" "),
        tab(t!("filter.tab_buckets").into_owned(), !tags_active),
        Span::styled(" │ ", Style::new().fg(c.dim)),
        tab(t!("filter.tab_tags").into_owned(), tags_active),
    ])
}

fn draw_filter(f: &mut Frame, app: &App, c: &Palette, idx: usize, checked: &[bool]) {
    let mut names: Vec<String> = app.board.buckets.iter().map(|b| b.name.clone()).collect();
    names.push(format!("({})", t!("bucket.none")));
    let lines: Vec<Line> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let sel = i == idx;
            let on = checked[i];
            Line::from(vec![
                Span::styled(
                    if sel { "→ " } else { "  " },
                    Style::new().fg(c.border_active),
                ),
                Span::styled(
                    if on { "[x] " } else { "[ ] " },
                    Style::new().fg(if on { c.done } else { c.dim }),
                ),
                Span::styled(
                    format!("{:<20}", truncate(name, 20)),
                    if sel {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                ),
                Span::styled(
                    format!("{:>4}", app.board.bucket_counts[i]),
                    Style::new().fg(c.dim),
                ),
            ])
        })
        .collect();
    let mut lines = lines;
    if app.board.buckets.is_empty() {
        lines.insert(
            0,
            Line::from(Span::styled(
                format!("  {}", t!("filter.no_buckets")),
                Style::new().fg(c.dim),
            )),
        );
    }
    lines.insert(0, filter_tabs(c, false));
    lines.insert(1, Line::default());
    let inner = popup(f, c, &t!("filter.title"), 42, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

/// Labelled input field line; also returns the cursor position when focused.
fn field_line(
    c: &Palette,
    at: Position,
    width: u16,
    label: &str,
    input: &Input,
    focused: bool,
    hint: &str,
) -> (Line<'static>, Option<Position>) {
    const LABEL: usize = 8;
    let style = if focused {
        Style::new().fg(c.key).bold()
    } else {
        Style::new().fg(c.dim)
    };
    let avail = (width as usize).saturating_sub(LABEL + 2 + hint.width());
    let (visible, cx) = input_view(input, avail.max(4));
    let cursor = focused.then(|| Position {
        x: at.x + (LABEL + 2 + cx) as u16,
        y: at.y,
    });
    let line = Line::from(vec![
        Span::styled(if focused { "▸ " } else { "  " }, style),
        Span::styled(format!("{label:<LABEL$}"), style),
        Span::styled(visible, Style::new().fg(c.fg)),
        Span::styled(hint.to_string(), Style::new().fg(c.dim)),
    ]);
    (line, cursor)
}

fn row(area: Rect, n: u16) -> Position {
    Position {
        x: area.x,
        y: area.y + n,
    }
}

fn draw_bucket_new(f: &mut Frame, c: &Palette, name: &Input, key: &Input, focus_key: bool) {
    let inner = popup(f, c, &t!("bucket.new_title"), 48, 5);
    let hint = if key.text.is_empty() {
        String::new()
    } else {
        format!("  → @{}", key.text)
    };
    let (name_line, c1) = field_line(
        c,
        row(inner, 0),
        inner.width,
        &t!("bucket.field_name"),
        name,
        !focus_key,
        "",
    );
    let (key_line, c2) = field_line(
        c,
        row(inner, 1),
        inner.width,
        &t!("bucket.field_key"),
        key,
        focus_key,
        &hint,
    );
    let lines = vec![
        name_line,
        key_line,
        Line::from(Span::styled(
            format!("  {}", t!("bucket.key_hint")),
            Style::new().fg(c.dim),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(pos) = c1.or(c2) {
        f.set_cursor_position(pos);
    }
}

fn draw_bucket_rename(f: &mut Frame, c: &Palette, key: &str, input: &Input) {
    let inner = popup(f, c, &t!("bucket.rename_title", key = key), 48, 4);
    let (line, cursor) = field_line(
        c,
        row(inner, 0),
        inner.width,
        &t!("bucket.field_name"),
        input,
        true,
        "",
    );
    let lines = vec![
        line,
        Line::from(Span::styled(
            format!("  {}", t!("bucket.rename_hint")),
            Style::new().fg(c.dim),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(pos) = cursor {
        f.set_cursor_position(pos);
    }
}

fn draw_bucket_delete(
    f: &mut Frame,
    app: &App,
    c: &Palette,
    key: &str,
    open: usize,
    done: usize,
    idx: usize,
) {
    let name = app.board.bucket_name(key).to_string();
    let choices = app.delete_choices(open, done);
    let summary = if open + done == 0 {
        t!("bucket.delete_empty").into_owned()
    } else {
        t!(
            "bucket.delete_summary",
            n = open + done,
            open = open,
            done = done
        )
        .into_owned()
    };
    let mut lines = vec![
        Line::from(Span::styled(summary, Style::new().fg(c.warn))),
        Line::default(),
    ];
    for (i, (_, enabled, text)) in choices.iter().enumerate() {
        let sel = i == idx;
        let style = match (enabled, sel) {
            (false, _) => Style::new().fg(c.dim),
            (true, true) => Style::new().bold(),
            (true, false) => Style::new(),
        };
        lines.push(Line::from(vec![
            Span::styled(
                if sel { "→ " } else { "  " },
                Style::new().fg(c.border_active),
            ),
            Span::styled(text.clone(), style),
        ]));
    }
    let inner = popup(
        f,
        c,
        &t!("bucket.delete_title", name = name),
        56,
        lines.len() as u16 + 2,
    );
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_bucket_move(f: &mut Frame, app: &App, c: &Palette, key: &str, idx: usize) {
    let lines: Vec<Line> = app
        .move_targets(key)
        .into_iter()
        .enumerate()
        .map(|(i, (k, name))| {
            let sel = i == idx;
            Line::from(vec![
                Span::styled(
                    if sel { "→ " } else { "  " },
                    Style::new().fg(c.border_active),
                ),
                Span::styled(
                    format!("{name}  @{k}"),
                    if sel {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                ),
            ])
        })
        .collect();
    let title = t!("bucket.move_title", name = app.board.bucket_name(key)).into_owned();
    let inner = popup(f, c, &title, 48, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

/// Value shown for a settings row; `None` for action rows.
fn setting_value(app: &App, row: SettingsRow) -> Option<String> {
    let cfg = app.cfg();
    Some(match row {
        SettingsRow::Language => cfg.lang().native_name().to_string(),
        SettingsRow::DefaultBucket => match cfg
            .default_bucket
            .as_deref()
            .filter(|k| app.board.buckets.iter().any(|b| b.key == *k))
        {
            Some(k) => format!("{} @{k}", app.board.bucket_name(k)),
            None => t!("settings.none").into_owned(),
        },
        SettingsRow::DoneDays => match cfg.done_days() {
            Some(n) => t!("settings.days", n = n).into_owned(),
            None => t!("settings.all").into_owned(),
        },
        SettingsRow::AutoSync => {
            if cfg.auto_sync.unwrap_or(false) {
                t!("settings.on").into_owned()
            } else {
                t!("settings.off").into_owned()
            }
        }
        SettingsRow::Holidays => match app.region {
            Some(_) => {
                let code = cfg.holidays.as_deref().unwrap_or("").trim().to_lowercase();
                let code = code.strip_prefix("de-").unwrap_or(&code).to_string();
                crate::holidays::GERMAN_STATES
                    .iter()
                    .find(|(c, _, _)| *c == code)
                    .map_or(code.clone(), |(_, _, name)| name.to_string())
            }
            None => t!("settings.none").into_owned(),
        },
        SettingsRow::Archive | SettingsRow::EditBoard | SettingsRow::OpenFile => return None,
    })
}

fn draw_settings(
    f: &mut Frame,
    app: &App,
    c: &Palette,
    idx: usize,
    remote: Option<&str>,
    archivable: usize,
) {
    let mut lines = vec![];
    for (i, row) in SETTINGS_ROWS.iter().enumerate() {
        let sel = i == idx;
        let marker = Span::styled(
            if sel { "→ " } else { "  " },
            Style::new().fg(c.border_active),
        );
        let style = if sel {
            Style::new().bold()
        } else {
            Style::new()
        };
        if *row == SettingsRow::Archive {
            // Actions get a blank line above them
            lines.push(Line::default());
        }
        let line = match setting_value(app, *row) {
            Some(value) => {
                let label = match row {
                    SettingsRow::Language => t!("settings.language"),
                    SettingsRow::DefaultBucket => t!("settings.default_bucket"),
                    SettingsRow::DoneDays => t!("settings.done_days"),
                    SettingsRow::AutoSync => t!("settings.auto_sync"),
                    _ => t!("settings.holidays"),
                };
                let arrow = Style::new().fg(if sel { c.border_active } else { c.dim });
                Line::from(vec![
                    marker,
                    Span::styled(format!("{:<20}", label), style),
                    Span::styled("‹ ", arrow),
                    Span::styled(value, Style::new().fg(c.fg)),
                    Span::styled(" ›", arrow),
                ])
            }
            None => {
                let text = match (row, app.cfg().done_days()) {
                    (SettingsRow::Archive, Some(d)) => {
                        t!("settings.archive_older", n = d, count = archivable)
                    }
                    (SettingsRow::Archive, None) => t!("settings.archive_all", count = archivable),
                    (SettingsRow::EditBoard, _) => t!("settings.edit_board"),
                    _ => t!("settings.open_file"),
                };
                Line::from(vec![marker, Span::styled(text.into_owned(), style)])
            }
        };
        lines.push(line);
    }
    lines.push(Line::default());
    let info = |label: String, value: String, dim: bool| {
        Line::from(vec![
            Span::styled(format!("  {label:<20}"), Style::new().fg(c.dim)),
            Span::styled(value, Style::new().fg(if dim { c.dim } else { c.fg })),
        ])
    };
    lines.push(info(
        t!("settings.data_dir").into_owned(),
        format!("{}  (shdo setup)", tilde_path(&app.store.dir)),
        false,
    ));
    lines.push(match remote {
        Some(url) => info(t!("settings.remote").into_owned(), url.to_string(), false),
        None => info(
            t!("settings.remote").into_owned(),
            t!("settings.no_remote").into_owned(),
            true,
        ),
    });
    let inner = popup(f, c, &t!("settings.title"), 72, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_confirm_archive(f: &mut Frame, app: &App, c: &Palette, count: usize) {
    let text = match app.cfg().done_days() {
        Some(d) => t!("settings.confirm_archive_older", n = d, count = count),
        None => t!("settings.confirm_archive_all", count = count),
    };
    let inner = popup(f, c, &t!("settings.archive_title"), 56, 5);
    let lines = vec![
        Line::from(text.into_owned()),
        Line::default(),
        Line::from(vec![
            Span::styled(
                t!("kb.yes_key").into_owned(),
                Style::new().fg(c.warn).bold(),
            ),
            Span::raw(format!(" {}   ", t!("kb.archive"))),
            Span::styled("n", Style::new().fg(c.key)),
            Span::raw(format!(" {}", t!("kb.cancel"))),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_tag_filter(f: &mut Frame, app: &App, c: &Palette, idx: usize, checked: &[bool]) {
    let mut lines: Vec<Line> = app
        .board
        .tag_counts
        .iter()
        .enumerate()
        .map(|(i, (tag, n))| {
            let sel = i == idx;
            let on = checked.get(i).copied().unwrap_or(false);
            Line::from(vec![
                Span::styled(
                    if sel { "→ " } else { "  " },
                    Style::new().fg(c.border_active),
                ),
                Span::styled(
                    if on { "[x] " } else { "[ ] " },
                    Style::new().fg(if on { c.done } else { c.dim }),
                ),
                Span::styled(
                    format!("{:<20}", truncate(&format!("+{tag}"), 20)),
                    if sel {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                ),
                Span::styled(format!("{n:>4}"), Style::new().fg(c.dim)),
            ])
        })
        .collect();
    let hint = if lines.is_empty() {
        t!("tagfilter.no_tags")
    } else {
        t!("tagfilter.hint")
    };
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!("  {hint}"),
        Style::new().fg(c.dim),
    )));
    lines.insert(0, filter_tabs(c, true));
    lines.insert(1, Line::default());
    let inner = popup(f, c, &t!("filter.title"), 42, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_pick_bucket(f: &mut Frame, app: &App, c: &Palette, idx: usize) {
    let mut names = vec![format!("({})", t!("bucket.none"))];
    names.extend(
        app.board
            .buckets
            .iter()
            .map(|b| format!("{}  @{}", b.name, b.key)),
    );
    let lines: Vec<Line> = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let sel = i == idx;
            Line::from(vec![
                Span::styled(
                    if sel { "→ " } else { "  " },
                    Style::new().fg(c.border_active),
                ),
                Span::styled(
                    name,
                    if sel {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                ),
            ])
        })
        .collect();
    let inner = popup(f, c, &t!("bucket.pick_title"), 32, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation() {
        assert_eq!(truncate("kurz", 10), "kurz");
        assert_eq!(truncate("Rechnung bezahlen", 9), "Rechnung…");
        assert_eq!(truncate("äöü", 3), "äöü");
    }
}
