//! Calendar sidebar: previous, current and next month with the number of tasks due per day.
//! Display only – no focus, no filtering.

use std::collections::HashMap;

use jiff::ToSpan;
use jiff::civil::{Date, Weekday};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rust_i18n::t;

use super::app::App;
use crate::i18n::{self, Lang};
use crate::theme::Palette;

/// Border (2) + padding (1) + week number (3) + 7 days of 3 characters.
pub const WIDTH: u16 = 2 + 1 + 3 + 7 * 3;

fn is_weekend(wd: Weekday) -> bool {
    matches!(wd, Weekday::Saturday | Weekday::Sunday)
}

/// Superscript count in the gap after the day; `⁺` from 10 on.
fn superscript(n: usize) -> char {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n < 10 { DIGITS[n] } else { '⁺' }
}

fn first_of_month(d: Date) -> Date {
    d.first_of_month()
}

/// Weeks start on Sunday in US English, on Monday otherwise.
fn sunday_first(lang: Lang) -> bool {
    lang == Lang::En
}

/// Position of `d` within its week row.
fn week_offset(d: Date, sunday_first: bool) -> usize {
    if sunday_first {
        d.weekday().to_sunday_zero_offset() as usize
    } else {
        d.weekday().to_monday_zero_offset() as usize
    }
}

fn weeks_in_month(first: Date, sunday_first: bool) -> usize {
    (week_offset(first, sunday_first) + first.days_in_month() as usize).div_ceil(7)
}

/// Rows for one month: title, weekday header, weeks, blank line.
fn month_height(first: Date, sunday_first: bool) -> usize {
    3 + weeks_in_month(first, sunday_first)
}

fn month_name(month: i8) -> String {
    let key = format!("cal.month.{month}");
    t!(key.as_str()).into_owned()
}

/// Two-letter weekday for the header.
fn weekday_header(wd: Weekday) -> String {
    let key = match wd {
        Weekday::Monday => "cal.wd.mon",
        Weekday::Tuesday => "cal.wd.tue",
        Weekday::Wednesday => "cal.wd.wed",
        Weekday::Thursday => "cal.wd.thu",
        Weekday::Friday => "cal.wd.fri",
        Weekday::Saturday => "cal.wd.sat",
        Weekday::Sunday => "cal.wd.sun",
    };
    t!(key).into_owned()
}

pub fn draw(f: &mut Frame, app: &App, c: &Palette, area: Rect) {
    let today = crate::dates::today();
    let block = Block::bordered()
        .border_style(Style::new().fg(c.border))
        .title(Span::styled(
            t!("cal.title").into_owned(),
            Style::new().fg(c.title),
        ));
    let sunday_first = sunday_first(i18n::current());
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Open, visible tasks per due date (follows the bucket filter, excludes done)
    let mut due: HashMap<Date, usize> = HashMap::new();
    for col in &app.board.columns[..app.board.last_index()] {
        for t in &col.tasks {
            if let Some(d) = t.due {
                *due.entry(d).or_default() += 1;
            }
        }
    }
    let selected_due = app.selected().and_then(|t| t.due);

    let current = first_of_month(today);
    let prev = current.checked_sub(1.month()).unwrap_or(current);
    let next = current.checked_add(1.month()).unwrap_or(current);
    // As many months as fit: all three, else current + next, else only the current one
    let height = inner.height as usize;
    let months: Vec<Date> = [
        vec![prev, current, next],
        vec![current, next],
        vec![current],
    ]
    .into_iter()
    .find(|ms| {
        ms.iter()
            .map(|m| month_height(*m, sunday_first))
            .sum::<usize>()
            <= height
    })
    .unwrap_or_else(|| vec![current]);

    let first = months[0];
    let last = months[months.len() - 1];
    let range_end = last.last_of_month();
    let holiday_list = app
        .region
        .map(|r| r.between(first, range_end))
        .unwrap_or_default();
    let holiday_set: HashMap<Date, &str> =
        holiday_list.iter().map(|(d, n)| (*d, n.as_str())).collect();

    let mut lines: Vec<Line> = vec![];
    for m in &months {
        month_lines(
            &mut lines,
            *m,
            sunday_first,
            *m == current,
            today,
            &due,
            selected_due,
            &holiday_set,
            c,
        );
    }
    // Holidays of the shown months, as far as space allows
    for (d, name) in holiday_list {
        if lines.len() >= height {
            break;
        }
        let style = if d < today {
            Style::new().fg(c.dim)
        } else {
            Style::new().fg(c.fg)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", crate::dates::fmt_day_month(d)),
                Style::new().fg(c.holiday),
            ),
            Span::styled(name, style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn month_lines(
    lines: &mut Vec<Line<'static>>,
    first: Date,
    sunday_first: bool,
    is_current: bool,
    today: Date,
    due: &HashMap<Date, usize>,
    selected_due: Option<Date>,
    holidays: &HashMap<Date, &str>,
    c: &Palette,
) {
    let title = format!("{} {}", month_name(first.month()), first.year());
    let title_style = if is_current {
        Style::new().fg(c.title).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(c.dim)
    };
    lines.push(Line::from(Span::styled(
        format!("{title:^25}"),
        title_style,
    )));
    let mut header = vec![Span::styled(
        format!("{:>3}", t!("cal.week")),
        Style::new().fg(c.dim),
    )];
    let offset = week_offset(first, sunday_first) as i64;
    let mut week_start = first.checked_sub(offset.days()).unwrap_or(first);
    for i in 0..7 {
        let wd = week_start
            .checked_add((i as i64).days())
            .unwrap_or(week_start)
            .weekday();
        let color = if is_weekend(wd) { c.weekend } else { c.dim };
        header.push(Span::styled(
            format!(" {}", weekday_header(wd)),
            Style::new().fg(color),
        ));
    }
    lines.push(Line::from(header));

    for _ in 0..weeks_in_month(first, sunday_first) {
        // ISO week of the row's Monday (the second day when weeks start on Sunday)
        let monday = week_start
            .checked_add((if sunday_first { 1 } else { 0 }).days())
            .unwrap_or(week_start);
        let kw = monday.iso_week_date().week();
        let mut spans = vec![Span::styled(format!(" {kw:>2}"), Style::new().fg(c.dim))];
        for i in 0..7 {
            let d = week_start
                .checked_add((i as i64).days())
                .unwrap_or(week_start);
            if d.month() != first.month() {
                spans.push(Span::raw("   "));
                continue;
            }
            // Gap before the day: nothing here, the count goes after the day
            spans.push(Span::raw(if i == 0 { " " } else { "" }));
            spans.push(Span::styled(
                format!("{:>2}", d.day()),
                day_style(d, today, selected_due, holidays, c),
            ));
            let marker = match due.get(&d) {
                Some(&n) => {
                    let color = if d < today { c.overdue } else { c.accent };
                    Span::styled(superscript(n).to_string(), Style::new().fg(color).bold())
                }
                None => Span::raw(" "),
            };
            spans.push(marker);
        }
        lines.push(Line::from(spans));
        week_start = week_start.checked_add(7.days()).unwrap_or(week_start);
    }
    lines.push(Line::default());
}

fn day_style(
    d: Date,
    today: Date,
    selected_due: Option<Date>,
    holidays: &HashMap<Date, &str>,
    c: &Palette,
) -> Style {
    let weekend = is_weekend(d.weekday());
    let mut style = if holidays.contains_key(&d) {
        Style::new().fg(c.holiday).add_modifier(Modifier::BOLD)
    } else if weekend {
        Style::new().fg(c.weekend)
    } else if d < today {
        Style::new().fg(c.dim)
    } else {
        Style::new().fg(c.fg)
    };
    if d == today {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    if selected_due == Some(d) {
        style = style
            .fg(c.border_active)
            .add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn month_geometry() {
        // October 2026 starts on a Thursday: 5 week rows
        assert_eq!(weeks_in_month(date(2026, 10, 1), false), 5);
        // February 2027 starts on a Monday and has 28 days: exactly 4 weeks
        assert_eq!(weeks_in_month(date(2027, 2, 1), false), 4);
        // Sunday-first: February 2027 then spans 5 rows
        assert_eq!(weeks_in_month(date(2027, 2, 1), true), 5);
        // August 2027 starts on a Sunday: 6 week rows
        assert_eq!(weeks_in_month(date(2027, 8, 1), false), 6);
        assert_eq!(weeks_in_month(date(2027, 8, 1), true), 5);
        assert_eq!(month_height(date(2026, 10, 1), false), 8);
    }

    #[test]
    fn superscripts() {
        assert_eq!(superscript(1), '¹');
        assert_eq!(superscript(9), '⁹');
        assert_eq!(superscript(12), '⁺');
    }
}
