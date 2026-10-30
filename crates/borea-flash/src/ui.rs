//! The look every screen shares: one palette, a header that shows where you are
//! in the journey (image → USB stick → write → verify), a centred column for the
//! content, a progress bar, and key hints that wrap instead of being cut off.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

/// Borea's dark palette, for terminals.
pub const ACCENT: Color = Color::Rgb(0xa2, 0xc4, 0xa8);
pub const SUB: Color = Color::Rgb(0x8a, 0x96, 0x8c);
pub const WARN: Color = Color::Rgb(0xe3, 0x9b, 0x84);
pub const LINE: Color = Color::Rgb(0x39, 0x44, 0x3b);
pub const SELECTED: Color = Color::Rgb(0x2d, 0x3b, 0x30);

/// The widest the content column gets, so long lines stay readable.
const COLUMN: u16 = 88;

/// Where the user is in getting Borea Live onto a stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Image,
    Stick,
    Write,
    Verify,
    Done,
}

const STEPS: [(Step, &str); 4] = [
    (Step::Image, "Image"),
    (Step::Stick, "USB stick"),
    (Step::Write, "Write"),
    (Step::Verify, "Verify"),
];

/// A line under the content: a warning, or a confirmation when it starts with ✓.
pub fn note_style(text: &str) -> Style {
    if text.starts_with('✓') { Style::new().fg(ACCENT) } else { Style::new().fg(WARN) }
}

/// Draw the header and footer for `step`, and return the content column.
/// `note` sits just above the key hints; both wrap to the column's width.
pub fn screen(frame: &mut Frame, step: Step, note: Option<&str>, keys: &[(&str, &str)]) -> Rect {
    let area = column(frame.area());
    let note_lines: Vec<Line> = note
        .map(|text| {
            wrap(text, area.width)
                .into_iter()
                .map(|l| Line::from(Span::styled(l, note_style(text))))
                .collect()
        })
        .unwrap_or_default();
    let key_lines = key_lines(keys, area.width);
    let footer = (note_lines.len() + key_lines.len()) as u16 + 1;
    let [header, body, _, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(footer),
    ])
    .areas(area);
    render_header(frame, header, step);
    let mut lines = note_lines;
    lines.extend(key_lines);
    frame.render_widget(Paragraph::new(lines), footer);
    body
}

/// The frame's area less a 2-column margin, at most [`COLUMN`] wide, centred.
fn column(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(COLUMN);
    let [column] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(area);
    column
}

fn render_header(frame: &mut Frame, area: Rect, step: Step) {
    let mut journey = Vec::new();
    for (i, (this, name)) in STEPS.iter().enumerate() {
        if i > 0 {
            journey.push(Span::styled("  ›  ", Style::new().fg(LINE)));
        }
        journey.extend(match (*this).cmp(&step) {
            std::cmp::Ordering::Less => {
                [Span::styled("✓ ", ACCENT), Span::styled(*name, Style::new().fg(SUB))]
            }
            std::cmp::Ordering::Equal => {
                [Span::styled("● ", ACCENT), Span::styled(*name, Style::new().bold())]
            }
            std::cmp::Ordering::Greater => [
                Span::styled("○ ", Style::new().fg(LINE)),
                Span::styled(*name, Style::new().fg(SUB).add_modifier(Modifier::DIM)),
            ],
        });
    }
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Borea").bold(),
            Span::styled("  rataTUI", SUB),
            Span::styled(
                "   ∼ go wherever the wind takes you",
                Style::new().fg(ACCENT).add_modifier(Modifier::DIM),
            ),
        ]),
        Line::from(journey),
        Line::from(Span::styled("─".repeat(area.width as usize), Style::new().fg(LINE))),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

/// Key hints as "key action" pairs, wrapped onto as many lines as they need.
fn key_lines(keys: &[(&str, &str)], width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut line: Vec<Span> = Vec::new();
    let mut used = 0usize;
    for (key, action) in keys {
        let len = key.chars().count() + 1 + action.chars().count();
        let gap = if line.is_empty() { 0 } else { 3 };
        if !line.is_empty() && used + gap + len > width as usize {
            lines.push(Line::from(std::mem::take(&mut line)));
            used = 0;
        } else if gap > 0 {
            line.push(Span::raw("   "));
        }
        line.push(Span::styled(key.to_string(), Style::new().fg(ACCENT).bold()));
        line.push(Span::styled(format!(" {action}"), SUB));
        used += if used == 0 { len } else { gap + len };
    }
    if !line.is_empty() {
        lines.push(Line::from(line));
    }
    lines
}

/// Greedy word wrap to `width` columns; long words are split.
pub fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = (width as usize).max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_owned();
            loop {
                let needed = if line.is_empty() { 0 } else { line.chars().count() + 1 };
                if needed + word.chars().count() <= width {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    continue;
                }
                let head: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                lines.push(head);
                if word.is_empty() {
                    break;
                }
            }
        }
        lines.push(line);
    }
    lines
}

/// Wrapped lines of `text` in `style`.
pub fn text(text: &str, width: u16, style: Style) -> Vec<Line<'static>> {
    wrap(text, width).into_iter().map(|l| Line::from(Span::styled(l, style))).collect()
}

/// A progress bar the full width of the column.
pub fn bar(ratio: f64, width: u16) -> Line<'static> {
    let filled = (ratio.clamp(0.0, 1.0) * width as f64).round() as usize;
    Line::from(vec![
        Span::styled("━".repeat(filled), ACCENT),
        Span::styled("━".repeat(width as usize - filled), LINE),
    ])
}

/// A rounded panel with a quiet title.
pub fn panel(title: &str) -> Block<'_> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(LINE))
        .title(Span::styled(title, SUB))
        .padding(Padding::horizontal(1))
}

/// Bytes as the size printed on the stick: decimal units.
pub fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b if b >= 1_000_000 => format!("{:.0} MB", b as f64 / 1e6),
        b if b >= 1_000 => format!("{:.0} KB", b as f64 / 1e3),
        b => format!("{b} B"),
    }
}

/// Time left, rounded the way people say it.
pub fn eta(seconds: f64) -> String {
    let s = seconds.round() as u64;
    match s {
        s if s >= 3600 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

/// Elapsed time as h:mm:ss, or m:ss under an hour.
pub fn clock(seconds: u64) -> String {
    match seconds {
        s if s >= 3600 => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        s => format!("{}:{:02}", s / 60, s % 60),
    }
}

/// "1 package", "3 packages".
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_and_splits_long_ones() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\nb", 10), ["a", "b"]);
    }

    #[test]
    fn key_hints_wrap_instead_of_being_cut() {
        let keys = [("↑↓", "choose"), ("Enter", "write"), ("u", "unmount"), ("q", "quit")];
        assert_eq!(key_lines(&keys, 80).len(), 1);
        let narrow = key_lines(&keys, 24);
        assert!(narrow.len() > 1);
        assert!(narrow.iter().all(|l| l.width() <= 24), "{narrow:?}");
    }

    #[test]
    fn sizes_and_times_read_naturally() {
        assert_eq!(human(2_100_000_000), "2.1 GB");
        assert_eq!(human(700_000_000), "700 MB");
        assert_eq!(eta(3725.0), "1h 02m");
        assert_eq!(clock(83), "1:23");
        assert_eq!(clock(3_725), "1:02:05");
        assert_eq!(count(1, "package", "packages"), "1 package");
    }
}
