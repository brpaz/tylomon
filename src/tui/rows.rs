use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme::Theme;

/// A rendered row of the detail pane, with enough context to copy text back out of it.
#[derive(Debug, Clone)]
pub struct Row {
    pub line: Line<'static>,
    /// Indent width of a row that continues the previous one after soft wrapping; zero for
    /// the start of a real line.
    pub continuation: usize,
}

impl Row {
    pub fn new(line: Line<'static>) -> Self {
        Self {
            line,
            continuation: 0,
        }
    }

    pub fn text(&self) -> String {
        self.line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Length in characters.
    pub fn len(&self) -> usize {
        self.line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl From<Line<'static>> for Row {
    fn from(line: Line<'static>) -> Self {
        Self::new(line)
    }
}

/// A character position in the rows: line index and character column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// A text selection over the rows of one exchange. Both ends are inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Exchange the rows belong to.
    pub id: u64,
    pub anchor: Pos,
    pub cursor: Pos,
}

impl Selection {
    /// Ends ordered from first to last in reading order.
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Half-open column range selected on `line`, which is `len` characters long.
    pub fn columns_on(&self, line: usize, len: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        if line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line { end.col + 1 } else { len };
        let (from, to) = (from.min(len), to.min(len));
        (from < to).then_some((from, to))
    }
}

/// The selected text. Rows that continue a soft-wrapped line are joined without a newline
/// and without their wrap indent, so a long line copies as it was received.
pub fn selected_text(rows: &[Row], selection: &Selection) -> String {
    let (start, end) = selection.ordered();
    let mut out = String::new();

    for index in start.line..=end.line.min(rows.len().saturating_sub(1)) {
        let Some(row) = rows.get(index) else { break };
        let chars: Vec<char> = row.text().chars().collect();

        let mut from = if index == start.line { start.col } else { 0 };
        let to = if index == end.line {
            (end.col + 1).min(chars.len())
        } else {
            chars.len()
        };

        if index > start.line {
            if row.continuation > 0 {
                let indent = chars
                    .iter()
                    .take(row.continuation)
                    .take_while(|c| c.is_whitespace())
                    .count();
                from = from.max(indent);
            } else {
                out.push('\n');
            }
        }
        if from < to {
            out.extend(&chars[from..to]);
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || "_-.:/@%+~#".contains(c)
}

/// The word under `pos` as an inclusive selection range, if `pos` is on one.
pub fn word_at(rows: &[Row], pos: Pos) -> Option<(Pos, Pos)> {
    let chars: Vec<char> = rows.get(pos.line)?.text().chars().collect();
    if !is_word_char(*chars.get(pos.col)?) {
        return None;
    }
    let mut start = pos.col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = pos.col;
    while end + 1 < chars.len() && is_word_char(chars[end + 1]) {
        end += 1;
    }
    Some((
        Pos {
            line: pos.line,
            col: start,
        },
        Pos {
            line: pos.line,
            col: end,
        },
    ))
}

/// Copy of `line` with the characters in `[start, end)` highlighted as selected.
pub fn highlight_line(line: &Line<'static>, start: usize, end: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut offset = 0;

    for span in &line.spans {
        let chars: Vec<char> = span.content.chars().collect();
        let (span_start, span_end) = (offset, offset + chars.len());
        offset = span_end;

        let (lo, hi) = (start.max(span_start), end.min(span_end));
        if lo >= hi {
            spans.push(span.clone());
            continue;
        }
        let part = |from: usize, to: usize, style: Style| {
            Span::styled(
                chars[from - span_start..to - span_start]
                    .iter()
                    .collect::<String>(),
                style,
            )
        };
        if lo > span_start {
            spans.push(part(span_start, lo, span.style));
        }
        spans.push(part(lo, hi, span.style.patch(Theme::selection())));
        if hi < span_end {
            spans.push(part(hi, span_end, span.style));
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    fn rows(lines: &[&str]) -> Vec<Row> {
        lines
            .iter()
            .map(|l| Row::new(Line::raw(l.to_string())))
            .collect()
    }

    fn pos(line: usize, col: usize) -> Pos {
        Pos { line, col }
    }

    fn sel(anchor: Pos, cursor: Pos) -> Selection {
        Selection {
            id: 1,
            anchor,
            cursor,
        }
    }

    #[test]
    fn single_line_selection_is_inclusive_of_both_ends() {
        let rows = rows(&["hello world"]);
        assert_eq!(selected_text(&rows, &sel(pos(0, 0), pos(0, 4))), "hello");
        assert_eq!(selected_text(&rows, &sel(pos(0, 6), pos(0, 10))), "world");
    }

    #[test]
    fn selection_in_either_direction_gives_the_same_text() {
        let rows = rows(&["hello world"]);
        assert_eq!(selected_text(&rows, &sel(pos(0, 4), pos(0, 0))), "hello");
    }

    #[test]
    fn multi_line_selection_keeps_newlines_and_partial_ends() {
        let rows = rows(&["first line", "second", "third line"]);
        let text = selected_text(&rows, &sel(pos(0, 6), pos(2, 4)));
        assert_eq!(text, "line\nsecond\nthird");
    }

    #[test]
    fn blank_lines_inside_a_selection_are_kept() {
        let rows = rows(&["a", "", "b"]);
        assert_eq!(selected_text(&rows, &sel(pos(0, 0), pos(2, 0))), "a\n\nb");
    }

    #[test]
    fn soft_wrapped_rows_rejoin_without_newline_or_indent() {
        let mut rows = rows(&["abcdef", "  ghij", "next"]);
        rows[1].continuation = 2;
        let text = selected_text(&rows, &sel(pos(0, 0), pos(2, 3)));
        assert_eq!(text, "abcdefghij\nnext");
    }

    #[test]
    fn selection_ending_past_the_text_is_clamped() {
        let rows = rows(&["abc"]);
        assert_eq!(selected_text(&rows, &sel(pos(0, 1), pos(0, 50))), "bc");
    }

    #[test]
    fn columns_on_covers_partial_first_and_last_lines() {
        let s = sel(pos(1, 3), pos(3, 2));
        assert_eq!(s.columns_on(0, 10), None);
        assert_eq!(s.columns_on(1, 10), Some((3, 10)));
        assert_eq!(s.columns_on(2, 10), Some((0, 10)));
        assert_eq!(s.columns_on(3, 10), Some((0, 3)));
        assert_eq!(s.columns_on(4, 10), None);
        assert_eq!(s.columns_on(2, 0), None);
    }

    #[test]
    fn word_at_expands_over_word_characters() {
        let rows = rows(&["GET /api/users?id=7 HTTP"]);
        let (a, b) = word_at(&rows, pos(0, 6)).expect("word");
        assert_eq!((a.col, b.col), (4, 13));
        assert_eq!(selected_text(&rows, &sel(a, b)), "/api/users");
    }

    #[test]
    fn word_at_on_a_separator_is_none() {
        let rows = rows(&["a b"]);
        assert_eq!(word_at(&rows, pos(0, 1)), None);
        assert_eq!(word_at(&rows, pos(0, 99)), None);
        assert_eq!(word_at(&rows, pos(5, 0)), None);
    }

    #[test]
    fn highlight_splits_spans_and_preserves_other_styles() {
        let line = Line::from(vec![
            Span::styled("abc", Style::default().fg(Color::Green)),
            Span::raw("def"),
        ]);
        let out = highlight_line(&line, 2, 4);
        let parts: Vec<(String, bool)> = out
            .spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.bg == Theme::selection().bg))
            .collect();
        assert_eq!(
            parts,
            [
                ("ab".to_string(), false),
                ("c".to_string(), true),
                ("d".to_string(), true),
                ("ef".to_string(), false),
            ]
        );
        assert_eq!(out.spans[0].style.fg, Some(Color::Green));
        assert_eq!(out.spans[1].style.fg, Theme::selection().fg);
    }
}
