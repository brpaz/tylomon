use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::tui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let border_style = if focused {
        Theme::title()
    } else {
        Style::default()
    };
    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner_width = area.width.saturating_sub(2) as usize;

    let filtered = app.filtered_exchanges();
    let selected = app
        .selected_index
        .and_then(|i| filtered.get(i).map(|(_, ex)| *ex));

    let content = match selected {
        Some(exchange) => {
            let mut lines: Vec<Line> = Vec::new();

            lines.push(Line::from(vec![Span::styled(
                "--- Request ---",
                Theme::title(),
            )]));
            wrap_plain(
                &mut lines,
                &format!(
                    "{} {} {}",
                    exchange.request.method, exchange.request.path, exchange.request.version
                ),
                inner_width,
            );

            let src_label = app
                .container_names
                .get(&exchange.src_ip)
                .cloned()
                .unwrap_or_else(|| exchange.src_ip.clone());
            let dst_label = app
                .container_names
                .get(&exchange.dst_ip)
                .cloned()
                .unwrap_or_else(|| exchange.dst_ip.clone());
            lines.push(Line::from(vec![
                Span::styled("From: ", Theme::header_name()),
                Span::styled(
                    format!("{src_label}:{}", exchange.src_port),
                    Theme::header_value(),
                ),
                Span::raw("  "),
                Span::styled("To: ", Theme::header_name()),
                Span::styled(
                    format!("{dst_label}:{}", exchange.dst_port),
                    Theme::header_value(),
                ),
            ]));

            lines.push(Line::raw(""));
            for (name, value) in &exchange.request.headers {
                wrap_header(&mut lines, name, value, inner_width);
            }

            if let Some(body) = &exchange.request.body {
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![Span::styled(
                    "--- Request Body ---",
                    Theme::title(),
                )]));
                append_body_lines(&mut lines, body, inner_width);
            }

            if let Some(response) = &exchange.response {
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![Span::styled(
                    "--- Response ---",
                    Theme::title(),
                )]));
                wrap_plain(
                    &mut lines,
                    &format!(
                        "{} {} {}",
                        response.version, response.status_code, response.reason
                    ),
                    inner_width,
                );

                if let Some(duration) = exchange.duration_ms {
                    lines.push(Line::from(vec![
                        Span::styled("Duration: ", Theme::header_name()),
                        Span::styled(format!("{duration}ms"), Theme::header_value()),
                    ]));
                }

                lines.push(Line::raw(""));
                for (name, value) in &response.headers {
                    wrap_header(&mut lines, name, value, inner_width);
                }

                if let Some(body) = &response.body {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![Span::styled(
                        "--- Response Body ---",
                        Theme::title(),
                    )]));
                    append_body_lines(&mut lines, body, inner_width);
                }
            } else {
                lines.push(Line::raw(""));
                lines.push(Line::styled("  Awaiting response...", Theme::dim()));
            }

            Text::from(lines)
        }
        None => Text::styled("  Select an exchange to view details", Theme::dim()),
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn wrap_plain<'a>(lines: &mut Vec<Line<'a>>, text: &str, max_width: usize) {
    if max_width == 0 {
        lines.push(Line::raw(text.to_string()));
        return;
    }
    for line in text.lines() {
        for chunk in hard_wrap(line, max_width) {
            lines.push(Line::raw(chunk));
        }
    }
}

fn wrap_header<'a>(lines: &mut Vec<Line<'a>>, name: &str, value: &str, max_width: usize) {
    let prefix = format!("{name}: ");
    let prefix_len = prefix.len();

    if max_width == 0 || prefix_len + value.len() <= max_width {
        lines.push(Line::from(vec![
            Span::styled(prefix, Theme::header_name()),
            Span::styled(value.to_string(), Theme::header_value()),
        ]));
        return;
    }

    let value_width = max_width.saturating_sub(prefix_len);
    if value_width == 0 {
        lines.push(Line::from(vec![
            Span::styled(prefix, Theme::header_name()),
            Span::styled(value.to_string(), Theme::header_value()),
        ]));
        return;
    }

    let chunks = hard_wrap(value, value_width);
    for (i, chunk) in chunks.into_iter().enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled(prefix.clone(), Theme::header_name()),
                Span::styled(chunk, Theme::header_value()),
            ]));
        } else {
            let indent = " ".repeat(prefix_len);
            lines.push(Line::from(vec![
                Span::raw(indent),
                Span::styled(chunk, Theme::header_value()),
            ]));
        }
    }
}

fn append_body_lines<'a>(lines: &mut Vec<Line<'a>>, body: &[u8], max_width: usize) {
    let body_str = String::from_utf8_lossy(body);
    let formatted = serde_json::from_str::<serde_json::Value>(&body_str)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| body_str.to_string());

    for line in formatted.lines() {
        if max_width == 0 || line.len() <= max_width {
            lines.push(Line::raw(line.to_string()));
        } else {
            for chunk in hard_wrap(line, max_width) {
                lines.push(Line::raw(chunk));
            }
        }
    }
}

fn hard_wrap(s: &str, width: usize) -> Vec<String> {
    if width == 0 || s.len() <= width {
        return vec![s.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = s;
    while !remaining.is_empty() {
        let end = remaining
            .char_indices()
            .nth(width)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        chunks.push(remaining[..end].to_string());
        remaining = &remaining[end..];
    }
    chunks
}
