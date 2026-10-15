use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::rows::Row;
use crate::tui::theme::Theme;

/// Largest text body rendered; the rest is cut off to keep redraws cheap.
const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Largest binary body rendered as a hex dump.
const MAX_HEX_BYTES: usize = 8 * 1024;
/// Terminal width needed for 16 bytes per hex dump row.
const WIDE_HEX_WIDTH: usize = 78;

/// A body that is not valid UTF-8 text.
pub fn is_binary(body: &[u8]) -> bool {
    body.contains(&0) || std::str::from_utf8(body).is_err()
}

/// Human-readable size such as `512 B` or `1.5 KB`.
pub fn format_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}

/// Render a body for the detail pane: coloured pretty JSON, plain text, or a hex dump for
/// binary data, wrapped to `width` columns.
pub fn body_lines(body: &[u8], width: usize) -> Vec<Row> {
    if is_binary(body) {
        return hex_lines(body, width);
    }

    let shown = &body[..body.len().min(MAX_TEXT_BYTES)];
    let text = String::from_utf8_lossy(shown);
    let truncated = body.len() > shown.len();

    let mut lines = match (!truncated)
        .then(|| serde_json::from_str::<serde_json::Value>(&text).ok())
        .flatten()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
    {
        Some(pretty) => pretty
            .lines()
            .flat_map(|line| wrap_segments(&json_segments(line), width, indent_of(line)))
            .collect(),
        None => text
            .lines()
            .flat_map(|line| {
                wrap_segments(
                    &[(line.to_string(), Style::default())],
                    width,
                    indent_of(line),
                )
            })
            .collect::<Vec<_>>(),
    };

    if truncated {
        lines.push(notice(format!(
            "… {} more not shown (copy with y/1/2 for the full body)",
            format_size(body.len() - shown.len())
        )));
    }
    lines
}

fn notice(text: String) -> Row {
    Row::new(Line::styled(text, Theme::dim()))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Split a pretty-printed JSON line into styled tokens.
fn json_segments(line: &str) -> Vec<(String, Style)> {
    let chars: Vec<char> = line.chars().collect();
    let mut segments = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let style = match c {
            c if c.is_whitespace() => {
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                Style::default()
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                i = (i + 1).min(chars.len());
                let is_key = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
                if is_key {
                    Theme::json_key()
                } else {
                    Theme::json_string()
                }
            }
            '{' | '}' | '[' | ']' | ',' | ':' => {
                i += 1;
                Theme::json_punctuation()
            }
            _ => {
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], ',' | '}' | ']')
                {
                    i += 1;
                }
                match chars[start..i].iter().collect::<String>().as_str() {
                    "true" | "false" | "null" => Theme::json_keyword(),
                    _ => Theme::json_number(),
                }
            }
        };
        segments.push((chars[start..i].iter().collect(), style));
    }
    segments
}

/// Wrap styled segments to `width` columns. Continuation rows are indented by `indent + 2`
/// (capped so narrow panes still show content).
fn wrap_segments(segments: &[(String, Style)], width: usize, indent: usize) -> Vec<Row> {
    if width == 0 {
        return vec![Row::new(Line::from(
            segments
                .iter()
                .map(|(t, s)| Span::styled(t.clone(), *s))
                .collect::<Vec<_>>(),
        ))];
    }

    // Never zero: a non-zero value marks a row as a soft-wrapped continuation.
    let continuation = (indent + 2).min(width / 2).max(1);
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0;
    let mut row_continuation = 0;

    for (text, style) in segments {
        let mut rest: Vec<char> = text.chars().collect();
        while !rest.is_empty() {
            if col >= width {
                lines.push(Row {
                    line: Line::from(std::mem::take(&mut spans)),
                    continuation: row_continuation,
                });
                row_continuation = continuation;
                spans.push(Span::raw(" ".repeat(continuation)));
                col = continuation;
            }
            let take = (width - col).min(rest.len());
            let chunk: String = rest.drain(..take).collect();
            col += take;
            spans.push(Span::styled(chunk, *style));
        }
    }
    lines.push(Row {
        line: Line::from(spans),
        continuation: row_continuation,
    });
    lines
}

/// One row of a hex dump: offset, hex bytes (grouped in eights) and printable characters.
fn hex_row(offset: usize, chunk: &[u8], per_line: usize) -> (String, String, String) {
    let mut hex = String::new();
    for i in 0..per_line {
        if i > 0 && i % 8 == 0 {
            hex.push(' ');
        }
        match chunk.get(i) {
            Some(b) => hex.push_str(&format!("{b:02x} ")),
            None => hex.push_str("   "),
        }
    }
    let ascii = chunk
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    (format!("{offset:08x}"), hex, ascii)
}

fn hex_lines(body: &[u8], width: usize) -> Vec<Row> {
    let per_line = if width >= WIDE_HEX_WIDTH { 16 } else { 8 };
    let shown = &body[..body.len().min(MAX_HEX_BYTES)];

    let mut lines = vec![notice(format!("binary data, {}", format_size(body.len())))];
    for (n, chunk) in shown.chunks(per_line).enumerate() {
        let (offset, hex, ascii) = hex_row(n * per_line, chunk, per_line);
        lines.push(Row::new(Line::from(vec![
            Span::styled(offset, Theme::hex_offset()),
            Span::raw("  "),
            Span::styled(hex, Style::default()),
            Span::raw(" "),
            Span::styled(format!("|{ascii}|"), Theme::hex_ascii()),
        ])));
    }
    if body.len() > shown.len() {
        lines.push(notice(format!(
            "… {} more not shown",
            format_size(body.len() - shown.len())
        )));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(row: &Row) -> String {
        row.text()
    }

    #[test]
    fn detects_binary_bodies() {
        assert!(!is_binary(b"{\"a\":1}"));
        assert!(!is_binary("héllo".as_bytes()));
        assert!(is_binary(&[0xff, 0xfe, 0x01]));
        assert!(is_binary(b"abc\0def"));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn json_segments_classify_tokens() {
        let segs = json_segments(r#"  "name": "x", "#);
        let styled: Vec<(&str, Style)> = segs.iter().map(|(t, s)| (t.as_str(), *s)).collect();
        assert!(styled.contains(&("\"name\"", Theme::json_key())));
        assert!(styled.contains(&("\"x\"", Theme::json_string())));
        assert!(styled.contains(&(":", Theme::json_punctuation())));

        let segs = json_segments("\"a\": 12,");
        assert!(
            segs.iter()
                .any(|(t, s)| t == "12" && *s == Theme::json_number())
        );
        let segs = json_segments("\"a\": null");
        assert!(
            segs.iter()
                .any(|(t, s)| t == "null" && *s == Theme::json_keyword())
        );
        let segs = json_segments("true");
        assert_eq!(segs[0].1, Theme::json_keyword());
    }

    #[test]
    fn json_string_with_escaped_quote_and_colon_is_a_value() {
        let segs = json_segments(r#""k": "a \" b: c""#);
        let strings: Vec<_> = segs
            .iter()
            .filter(|(_, s)| *s == Theme::json_string())
            .collect();
        assert_eq!(strings.len(), 1);
        assert_eq!(strings[0].0, r#""a \" b: c""#);
    }

    #[test]
    fn json_body_is_pretty_printed() {
        let lines = body_lines(br#"{"a":1,"b":[true]}"#, 80);
        let rendered: Vec<String> = lines.iter().map(text).collect();
        assert_eq!(rendered[0], "{");
        assert_eq!(rendered[1], "  \"a\": 1,");
        assert!(rendered.contains(&"    true".to_string()));
    }

    #[test]
    fn plain_text_is_kept_and_wrapped() {
        let lines = body_lines(b"abcdefghij", 4);
        let rendered: Vec<String> = lines.iter().map(text).collect();
        assert_eq!(rendered, ["abcd", "  ef", "  gh", "  ij"]);
    }

    #[test]
    fn wrapped_rows_are_marked_as_continuations() {
        let rows = body_lines(
            b"abcdefghij
next",
            4,
        );
        let marks: Vec<usize> = rows.iter().map(|r| r.continuation).collect();
        assert_eq!(marks, [0, 2, 2, 2, 0]);
    }

    #[test]
    fn wrapping_counts_characters_not_bytes() {
        let lines = body_lines("ééééé".as_bytes(), 5);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn binary_body_renders_hex_dump() {
        let body: Vec<u8> = (0..20).collect();
        let lines = body_lines(&body, 100);
        let rendered: Vec<String> = lines.iter().map(text).collect();
        assert_eq!(rendered[0], "binary data, 20 B");
        assert!(rendered[1].starts_with("00000000  00 01 02 03 04 05 06 07  08 09"));
        assert!(rendered[1].ends_with("|................|"));
        assert!(rendered[2].starts_with("00000010  10 11 12 13"));
    }

    #[test]
    fn hex_dump_uses_narrow_rows_in_small_panes() {
        let body = b"\xffHello\x00World!!!";
        let lines = body_lines(body, 40);
        assert!(text(&lines[1]).starts_with("00000000  ff 48 65 6c 6c 6f 00 57"));
        assert!(text(&lines[1]).ends_with("|.Hello.W|"));
    }

    #[test]
    fn large_binary_body_is_truncated_with_notice() {
        let body = vec![0u8; MAX_HEX_BYTES + 100];
        let lines = body_lines(&body, 100);
        assert!(text(lines.last().expect("line")).contains("100 B more not shown"));
    }

    #[test]
    fn large_text_body_is_truncated_with_notice() {
        let body = vec![b'a'; MAX_TEXT_BYTES + 10];
        let lines = body_lines(&body, 1000);
        assert!(text(lines.last().expect("line")).contains("10 B more not shown"));
    }
}
