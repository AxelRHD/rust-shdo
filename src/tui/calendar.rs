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

use super::app::App;
use crate::theme::Palette;

/// Border (2) + padding (1) + week number (3) + 7 days of 3 characters.
pub const WIDTH: u16 = 2 + 1 + 3 + 7 * 3;

const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// Superscript count in the gap after the day; `⁺` from 10 on.
fn superscript(n: usize) -> char {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n < 10 { DIGITS[n] } else { '⁺' }
}

fn first_of_month(d: Date) -> Date {
    d.first_of_month()
}

fn weeks_in_month(first: Date) -> usize {
    let offset = first.weekday().to_monday_zero_offset() as usize;
    (offset + first.days_in_month() as usize).div_ceil(7)
}

/// Rows for one month: title, weekday header, weeks, blank line.
fn month_height(first: Date) -> usize {
    3 + weeks_in_month(first)
}

pub fn draw(f: &mut Frame, app: &App, c: &Palette, area: Rect) {
    let today = crate::dates::today();
    let block = Block::bordered()
        .border_style(Style::new().fg(c.border))
        .title(Span::styled("Kalender", Style::new().fg(c.title)));
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
    .find(|ms| ms.iter().map(|m| month_height(*m)).sum::<usize>() <= height)
    .unwrap_or_else(|| vec![current]);

    let first = months[0];
    let last = months[months.len() - 1];
    let range_end = last.last_of_month();
    let holiday_list = app.region.between(first, range_end);
    let holiday_set: HashMap<Date, &str> = holiday_list.iter().copied().collect();

    let mut lines: Vec<Line> = vec![];
    for m in &months {
        month_lines(
            &mut lines,
            *m,
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
                format!(" {:02}.{:02}. ", d.day(), d.month()),
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
    is_current: bool,
    today: Date,
    due: &HashMap<Date, usize>,
    selected_due: Option<Date>,
    holidays: &HashMap<Date, &str>,
    c: &Palette,
) {
    let title = format!("{} {}", MONTHS[first.month() as usize - 1], first.year());
    let title_style = if is_current {
        Style::new().fg(c.title).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(c.dim)
    };
    lines.push(Line::from(Span::styled(
        format!("{title:^25}"),
        title_style,
    )));
    let mut header = vec![Span::styled(" KW", Style::new().fg(c.dim))];
    for (i, wd) in ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"]
        .iter()
        .enumerate()
    {
        let color = if i >= 5 { c.weekend } else { c.dim };
        header.push(Span::styled(format!(" {wd}"), Style::new().fg(color)));
    }
    lines.push(Line::from(header));

    let offset = first.weekday().to_monday_zero_offset() as i64;
    let mut week_start = first.checked_sub(offset.days()).unwrap_or(first);
    for _ in 0..weeks_in_month(first) {
        let kw = week_start.iso_week_date().week();
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
    let weekend = matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday);
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
        assert_eq!(weeks_in_month(date(2026, 10, 1)), 5);
        // February 2027 starts on a Monday and has 28 days: exactly 4 weeks
        assert_eq!(weeks_in_month(date(2027, 2, 1)), 4);
        // August 2027 starts on a Sunday: 6 week rows
        assert_eq!(weeks_in_month(date(2027, 8, 1)), 6);
        assert_eq!(month_height(date(2026, 10, 1)), 8);
    }

    #[test]
    fn superscripts() {
        assert_eq!(superscript(1), '¹');
        assert_eq!(superscript(9), '⁹');
        assert_eq!(superscript(12), '⁺');
    }
}
