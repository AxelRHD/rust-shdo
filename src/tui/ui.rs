//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::app::{App, Level, Mode};
use super::calendar;
use super::form::{Field, Form, Input, PRIO_NAMES};
use crate::dates;
use crate::task::Task;
use crate::theme::Palette;

const MIN_COLUMN_WIDTH: u16 = 24;
const CARD_HEIGHT: u16 = 2;

pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Navigation",
        &[
            ("h / ←", "Spalte links"),
            ("l / →", "Spalte rechts"),
            ("j / ↓", "nächste Karte"),
            ("k / ↑", "vorherige Karte"),
            ("g / G", "erste / letzte Karte"),
            ("⏎", "Details anzeigen"),
        ],
    ),
    (
        "Karten",
        &[
            ("n", "neue Karte (Formular, Status der Spalte vorbelegt)"),
            ("e", "Karte im Formular bearbeiten"),
            ("E", "Task-Datei im Editor öffnen"),
            ("H / ⇧←", "Karte nach links (vorheriger Status)"),
            ("L / ⇧→", "Karte nach rechts (nächster Status)"),
            ("x", "erledigt / wieder offen"),
            ("r", "Titel umbenennen"),
            ("1 2 3", "Prio hoch / normal / niedrig"),
            ("D", "Fälligkeit setzen (leer = entfernen)"),
            ("b", "Bucket setzen"),
            ("d", "löschen"),
        ],
    ),
    (
        "Formular",
        &[
            ("Tab ↓ / ⇧Tab ↑", "nächstes / voriges Feld"),
            ("← → Space", "Auswahl ändern (Status, Prio, Bucket)"),
            ("⏎", "speichern (aus jedem Feld)"),
            ("Esc", "verwerfen"),
            ("Titel", "P1–P3, due:fr, @work direkt im Titel gehen auch"),
        ],
    ),
    (
        "Buckets",
        &[
            ("f", "Filter: welche Buckets angezeigt werden"),
            (
                "n / r / d",
                "im Filter: Bucket anlegen / umbenennen / löschen",
            ),
            ("Space / x", "im Filter: an/aus (wirkt sofort)"),
            ("o", "im Filter: nur dieser"),
            ("a", "im Filter: alle an/aus"),
            ("⏎ / Esc", "im Filter: speichern / verwerfen"),
            ("", "genau ein Bucket aktiv → neue Tasks erben ihn"),
        ],
    ),
    (
        "Allgemein",
        &[
            ("s", "Sync mit Git-Remote"),
            ("R", "neu laden"),
            ("v", "Spalte „Erledigt“ ein-/ausblenden"),
            (
                "c",
                "Kalender ein-/ausblenden (Anzahl fälliger Tasks je Tag)",
            ),
            ("t", "Theme wählen"),
            ("?", "diese Hilfe"),
            ("q / Esc", "beenden bzw. abbrechen"),
        ],
    ),
    (
        "Datumsangaben",
        &[
            ("2026-10-24", "ISO"),
            ("24.10.", "nächstes Vorkommen, auch 24.10.2026"),
            ("+3d +2w +1m", "relativ: Tage, Wochen, Monate"),
            ("heute morgen", "auch übermorgen"),
            ("mo … so", "nächster Wochentag (nicht heute)"),
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
    let mut parts = vec![format!("{} offen", app.board.open_count())];
    let overdue = app.board.overdue_count(today);
    if overdue > 0 {
        parts.push(format!("{overdue} überfällig"));
    }
    if !app.show_done {
        let last = &app.board.columns[app.board.last_index()];
        parts.push(format!(
            "{} {} ausgeblendet",
            last.tasks.len(),
            last.def.name
        ));
    }
    let filter = app.board.filter_label().map(|label| {
        let mut s = format!("⧩ {label}");
        if app.board.hidden > 0 {
            s.push_str(&format!(" · {} ausgeblendet", app.board.hidden));
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
    if let Some(filter) = filter {
        right.push(Span::styled(filter, Style::new().fg(c.key).bold()));
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
    let counter = if active && len > 0 {
        format!("{} von {}", row + 1, len)
    } else {
        len.to_string()
    };
    let mut bottom = vec![Span::styled(
        counter,
        Style::new().fg(if active { c.fg } else { c.dim }),
    )];
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
            Paragraph::new(Span::styled("  (leer)", Style::new().fg(c.dim))),
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
        show_bucket: !app.board.buckets.is_empty() && app.board.filter.inherited().is_none(),
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
    if let Mode::Input { kind, input } = &app.mode {
        let label = format!(" {}: ", kind.label());
        let hint = match kind {
            super::app::InputKind::Due => "  leer = entfernen",
            super::app::InputKind::Rename => "",
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
                format!(" ⚠ {n} Warnung(en) – ? zeigt Details"),
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
                ("h/l", "Spalte"),
                ("j/k", "Karte"),
                ("H/L", "verschieben"),
                ("n", "neu"),
                ("e", "bearbeiten"),
                ("E", "Editor"),
                ("x", "erledigt"),
                ("⏎", "Details"),
            ];
            k.push(("f", if has_buckets { "Filter" } else { "Buckets" }));
            if has_buckets {
                k.push(("b", "Bucket"));
            }
            k.push(("v", "Erledigt ein/aus"));
            k.push(("c", "Kalender"));
            k.extend([("s", "Sync"), ("t", "Theme"), ("?", "Hilfe"), ("q", "Ende")]);
            k
        }
        Mode::Input { .. } => vec![("⏎", "speichern"), ("Esc", "abbrechen"), ("^U", "leeren")],
        Mode::Form(form) => {
            let mut k = vec![("Tab/↓", "nächstes Feld"), ("⇧Tab/↑", "voriges")];
            if !form.focused().is_text() {
                k.push(("←/→", "ändern"));
            }
            k.extend([("⏎", "speichern"), ("Esc", "verwerfen")]);
            k
        }
        Mode::ConfirmDelete => vec![("j", "löschen"), ("n/Esc", "abbrechen")],
        Mode::Help { .. } => vec![("j/k", "scrollen"), ("Esc", "schließen")],
        Mode::Themes { .. } => vec![
            ("j/k", "auswählen"),
            ("⏎", "übernehmen"),
            ("Esc", "abbrechen"),
        ],
        Mode::Detail { .. } => vec![
            ("j/k", "scrollen"),
            ("e", "bearbeiten"),
            ("E", "Editor"),
            ("Esc", "schließen"),
        ],
        Mode::Filter { .. } => vec![
            ("j/k", "auswählen"),
            ("Space", "an/aus"),
            ("o", "nur dieser"),
            ("a", "alle"),
            ("n", "neuer Bucket"),
            ("r", "umbenennen"),
            ("d", "löschen"),
            ("⏎", "speichern"),
            ("Esc", "verwerfen"),
        ],
        Mode::BucketNew { .. } => vec![("Tab", "Name/Key"), ("⏎", "anlegen"), ("Esc", "abbrechen")],
        Mode::BucketRename { .. } => vec![("⏎", "speichern"), ("Esc", "abbrechen")],
        Mode::BucketDelete { .. } | Mode::BucketMove { .. } => {
            vec![
                ("j/k", "auswählen"),
                ("⏎", "ausführen"),
                ("Esc", "abbrechen"),
            ]
        }
        Mode::PickBucket { .. } => {
            vec![("j/k", "auswählen"), ("⏎", "setzen"), ("Esc", "abbrechen")]
        }
    };
    let mut lines = vec![vec![Span::raw(" ")]];
    let mut used = 1;
    for (k, label) in keys {
        let item_width = k.width() + label.width() + 6;
        if used + item_width > width as usize && used > 1 && lines.len() < 2 {
            lines.push(vec![Span::raw(" ")]);
            used = 1;
        }
        let line = lines.last_mut().expect("mindestens eine Zeile");
        line.push(Span::styled("[", Style::new().fg(c.dim)));
        line.push(Span::styled(k.to_string(), Style::new().fg(c.key)));
        line.push(Span::styled(format!(" → {label}]"), Style::new().fg(c.dim)));
        line.push(Span::raw(" "));
        used += item_width;
    }
    lines.into_iter().map(Line::from).collect()
}

fn popup(f: &mut Frame, c: &Palette, title: &str, width: u16, height: u16) -> Rect {
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
            *section,
            Style::new().fg(c.title).bold(),
        )));
        for (k, text) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<14}"), Style::new().fg(c.key)),
                Span::raw(*text),
            ]));
        }
        lines.push(Line::default());
    }
    let warnings: Vec<_> = app.warnings().collect();
    if !warnings.is_empty() {
        lines.push(Line::from(Span::styled(
            "Warnungen",
            Style::new().fg(c.warn).bold(),
        )));
        for w in warnings {
            lines.push(Line::from(Span::styled(
                format!("  {w}"),
                Style::new().fg(c.warn),
            )));
        }
    }
    let inner = popup(f, c, "Hilfe", 72, lines.len() as u16 + 2);
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
    let inner = popup(f, c, "Theme", 36, lines.len() as u16 + 2);
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
            Span::styled(format!("{k:<12}"), Style::new().fg(c.dim)),
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
        field("Status", column.to_string()),
        field("Prio", format!("P{}", t.prio_or_default())),
    ];
    if let Some(b) = &t.bucket {
        lines.push(field(
            "Bucket",
            format!("{} (@{b})", app.board.bucket_name(b)),
        ));
    }
    if let Some(d) = t.due {
        lines.push(field(
            "Fällig",
            format!("{} ({})", d.strftime("%d.%m.%Y"), dates::fmt_due(d, today)),
        ));
    }
    for (k, v) in [
        ("Erstellt", t.created),
        ("Verschoben", t.moved),
        ("Erledigt", t.done),
    ] {
        if let Some(ts) = v {
            lines.push(field(k, dates::fmt_timestamp(ts)));
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
    let inner = popup(f, c, "Details", 72, height);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn draw_confirm(f: &mut Frame, app: &App, c: &Palette) {
    let Some(t) = app.selected() else { return };
    let inner = popup(f, c, "Löschen?", 50, 5);
    let lines = vec![
        Line::from(truncate(&t.title, inner.width as usize)),
        Line::default(),
        Line::from(vec![
            Span::styled("j", Style::new().fg(c.overdue).bold()),
            Span::raw(" löschen   "),
            Span::styled("n", Style::new().fg(c.key)),
            Span::raw(" abbrechen"),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

/// Visible slice of an input field and the cursor column within it.
fn input_view(input: &Input, width: usize) -> (String, usize) {
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

fn draw_form(f: &mut Frame, app: &App, c: &Palette, form: &Form) {
    const LABEL: usize = 10;
    let today = dates::today();
    let title = match &form.editing {
        None => "Neuer Task".to_string(),
        Some(id) => format!("Task bearbeiten · {id}"),
    };
    // Title, hint line, remaining fields, border
    let inner = popup(f, c, &title, 70, form.fields.len() as u16 + 3);
    let value_width = (inner.width as usize).saturating_sub(LABEL + 2);
    let mut lines: Vec<Line> = vec![];
    let mut cursor: Option<Position> = None;
    // Title tokens override the fields on save – show that next to each field.
    let tokens = crate::quickadd::parse_str(&form.title.text, today).ok();
    let overrides = |c: &Palette, text: String| {
        Span::styled(format!("  ← Titel: {text}"), Style::new().fg(c.accent))
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
        let mut text_field = |input: &Input, spans: &mut Vec<Span<'static>>, width: usize| {
            let (visible, cx) = input_view(input, width);
            spans.push(Span::styled(visible, Style::new().fg(c.fg)));
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
                    overrides(c, d.strftime("%d.%m.%Y").to_string())
                } else if form.due.text.trim().is_empty() {
                    Span::styled("  leer = keine Fälligkeit", Style::new().fg(c.dim))
                } else {
                    match dates::parse_date(&form.due.text, today) {
                        Ok(d) => Span::styled(
                            format!(
                                "  → {} ({})",
                                d.strftime("%d.%m.%Y"),
                                dates::fmt_due(d, today)
                            ),
                            Style::new().fg(c.accent),
                        ),
                        Err(_) => Span::styled("  ✗ unbekanntes Datum", Style::new().fg(c.overdue)),
                    }
                };
                let w = value_width.saturating_sub(preview.width()).max(8);
                text_field(&form.due, &mut spans, w);
                let used: usize = spans[2..].iter().map(|s| s.width()).sum();
                spans.push(Span::raw(" ".repeat(w.saturating_sub(used))));
                spans.push(preview);
            }
            Field::Note if form.note_editable => text_field(&form.note, &mut spans, value_width),
            Field::Note => spans.push(Span::styled(
                "mehrzeilig – im Editor bearbeiten (E)",
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
                    format!("P{} {}", form.prio, PRIO_NAMES[form.prio as usize - 1]),
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
                spans.extend(choice(form.bucket_label().to_string(), style, focused));
                if let Some(b) = tokens.as_ref().and_then(|q| q.bucket.as_ref()) {
                    spans.push(overrides(c, format!("@{b}")));
                } else if form.inherited {
                    spans.push(Span::styled(
                        "  aus Filter übernommen",
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
            || matches!(w, "P1" | "P2" | "P3" | "p1" | "p2" | "p3")
    });
    if !has_token {
        return Line::from(vec![
            pad,
            Span::styled(
                "P1–P3 · due:fr · @bucket im Titel gehen auch",
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
    Line::from(vec![
        pad,
        Span::styled(
            format!("erkannt: {}", parts.join(" · ")),
            Style::new().fg(c.accent),
        ),
    ])
}

fn draw_filter(f: &mut Frame, app: &App, c: &Palette, idx: usize, checked: &[bool]) {
    let mut names: Vec<String> = app.board.buckets.iter().map(|b| b.name.clone()).collect();
    names.push("(ohne Bucket)".into());
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
                "  noch keine Buckets – n legt einen an",
                Style::new().fg(c.dim),
            )),
        );
    }
    let inner = popup(f, c, "Filter · Buckets", 42, lines.len() as u16 + 2);
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
    let inner = popup(f, c, "Neuer Bucket", 48, 5);
    let hint = if key.text.is_empty() {
        String::new()
    } else {
        format!("  → @{}", key.text)
    };
    let (name_line, c1) = field_line(c, row(inner, 0), inner.width, "Name", name, !focus_key, "");
    let (key_line, c2) = field_line(c, row(inner, 1), inner.width, "Key", key, focus_key, &hint);
    let lines = vec![
        name_line,
        key_line,
        Line::from(Span::styled(
            "  Key steht in den Task-Dateien, a–z 0–9 - _",
            Style::new().fg(c.dim),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(pos) = c1.or(c2) {
        f.set_cursor_position(pos);
    }
}

fn draw_bucket_rename(f: &mut Frame, c: &Palette, key: &str, input: &Input) {
    let inner = popup(f, c, &format!("Bucket umbenennen · @{key}"), 48, 4);
    let (line, cursor) = field_line(c, row(inner, 0), inner.width, "Name", input, true, "");
    let lines = vec![
        line,
        Line::from(Span::styled(
            "  nur der Anzeigename, Tasks bleiben unverändert",
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
        "Der Bucket ist leer.".to_string()
    } else {
        format!("{} Task(s): {open} offen, {done} erledigt", open + done)
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
        &format!("Bucket „{name}“ löschen?"),
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
    let title = format!(
        "Tasks von „{}“ verschieben nach",
        app.board.bucket_name(key)
    );
    let inner = popup(f, c, &title, 48, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_pick_bucket(f: &mut Frame, app: &App, c: &Palette, idx: usize) {
    let mut names = vec!["(ohne Bucket)".to_string()];
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
    let inner = popup(f, c, "Bucket", 32, lines.len() as u16 + 2);
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
