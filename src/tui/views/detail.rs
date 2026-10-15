use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use super::body::{body_lines, format_size};
use crate::app::{App, DetailCache};
use crate::export::iso8601;
use crate::http::types::{HttpExchange, Protocol};
use crate::tui::rows::{Row, highlight_line};
use crate::tui::theme::Theme;

/// Rows of the request path shown in the pinned summary before it is cut off.
const MAX_PATH_ROWS: usize = 3;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let border_style = if focused {
        Theme::title()
    } else {
        Style::default()
    };
    let mut block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let Some(exchange) = app.selected_exchange().cloned() else {
        let empty = Paragraph::new("  Select an exchange to view details")
            .style(Theme::dim())
            .block(block);
        frame.render_widget(empty, area);
        return;
    };

    let inner = block.inner(area);
    // One column is reserved for the scrollbar so content does not reflow when it appears.
    let width = inner.width.saturating_sub(1) as usize;

    let summary = summary_lines(app, &exchange, width);
    let [summary_area, content_area] = Layout::vertical([
        Constraint::Length((summary.len() as u16).min(inner.height.saturating_sub(1))),
        Constraint::Min(0),
    ])
    .areas(inner);

    if app
        .detail_cache
        .as_ref()
        .is_none_or(|c| c.id != exchange.id || c.width != width)
    {
        app.selection = None;
        app.detail_cache = Some(DetailCache {
            id: exchange.id,
            width,
            rows: content_lines(&exchange, width),
        });
    }
    let lines = app
        .detail_cache
        .as_ref()
        .map(|c| c.rows.as_slice())
        .unwrap_or_default();

    let viewport = content_area.height as usize;
    let max_scroll = lines.len().saturating_sub(viewport);
    app.detail_scroll = app
        .detail_scroll
        .min(max_scroll.min(u16::MAX as usize) as u16);
    let scroll = app.detail_scroll as usize;

    if max_scroll > 0 {
        let last_visible = (scroll + viewport).min(lines.len());
        block = block.title_bottom(
            Line::styled(
                format!(" {}-{}/{} ", scroll + 1, last_visible, lines.len()),
                Theme::dim(),
            )
            .right_aligned(),
        );
    }
    frame.render_widget(block, area);

    frame.render_widget(Paragraph::new(summary), summary_area);

    let text_area = Rect {
        width: content_area.width.saturating_sub(1),
        ..content_area
    };
    app.detail_text_area = text_area;
    let selection = app.selection.as_ref();
    let visible: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(viewport)
        .map(
            |(index, row)| match selection.and_then(|s| s.columns_on(index, row.len())) {
                Some((from, to)) => highlight_line(&row.line, from, to),
                None => row.line.clone(),
            },
        )
        .collect();
    // Rows are already wrapped to the width, so one row is exactly one screen line and mouse
    // positions map straight onto them.
    frame.render_widget(Paragraph::new(visible), text_area);

    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            content_area,
            &mut state,
        );
    }
}

fn label(app: &App, ip: &str) -> String {
    app.container_names
        .get(ip)
        .cloned()
        .unwrap_or_else(|| ip.to_string())
}

/// Always-visible header: request line, outcome and endpoints.
fn summary_lines(app: &App, ex: &HttpExchange, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Request line, with the path cut off after a few rows.
    let method = format!("{} ", ex.request.method);
    let path_width = width.saturating_sub(method.chars().count()).max(1);
    let path: Vec<char> = ex.request.path.chars().collect();
    let rows: Vec<&[char]> = path.chunks(path_width).collect();
    for (i, row) in rows.iter().take(MAX_PATH_ROWS).enumerate() {
        let mut text: String = row.iter().collect();
        if i + 1 == MAX_PATH_ROWS && rows.len() > MAX_PATH_ROWS {
            text.pop();
            text.push('…');
        }
        let lead = if i == 0 {
            Span::styled(method.clone(), Theme::method(&ex.request.method))
        } else {
            Span::raw(" ".repeat(method.chars().count()))
        };
        lines.push(Line::from(vec![
            lead,
            Span::styled(
                text,
                Theme::header_value().add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            method.clone(),
            Theme::method(&ex.request.method),
        )));
    }

    // Outcome: status badge, timing and body sizes.
    let status = ex.response.as_ref().map(|r| r.status_code);
    let badge = match (&ex.response, ex.protocol) {
        (Some(r), Protocol::Postgres) => format!(" {} ", r.reason),
        (Some(r), Protocol::Http) => format!(" {} {} ", r.status_code, r.reason),
        (None, _) => " NO RESPONSE ".to_string(),
    };
    let mut outcome = vec![Span::styled(badge, Theme::status_badge(status))];
    if let Some(ms) = ex.duration_ms {
        outcome.push(Span::styled(
            format!("  {ms} ms"),
            Theme::duration(Some(ms)),
        ));
    }
    let detail = match ex.protocol {
        Protocol::Http => {
            let up = ex.request.body.as_ref().map_or(0, Vec::len);
            let down = ex
                .response
                .as_ref()
                .and_then(|r| r.body.as_ref())
                .map_or(0, Vec::len);
            format!("  ↑ {}  ↓ {}", format_size(up), format_size(down))
        }
        Protocol::Postgres => ex
            .response
            .as_ref()
            .and_then(|r| header_value(&r.headers, "rows"))
            .map_or(String::new(), |rows| format!("  {rows} rows")),
    };
    outcome.push(Span::styled(detail, Theme::dim()));
    lines.push(Line::from(outcome));

    // Endpoints and capture time.
    let mut endpoints = vec![
        Span::styled(
            format!("{}:{}", label(app, &ex.src_ip), ex.src_port),
            Theme::header_value(),
        ),
        Span::styled(" → ", Theme::dim()),
        Span::styled(
            format!("{}:{}", label(app, &ex.dst_ip), ex.dst_port),
            Theme::header_value(),
        ),
    ];
    if let Some(t) = ex.timestamp_us {
        // "YYYY-MM-DDTHH:MM:SS.mmmZ" -> "HH:MM:SS.mmmZ"
        endpoints.push(Span::styled(
            format!("  {}", &iso8601(t)[11..]),
            Theme::dim(),
        ));
    }
    lines.push(Line::from(endpoints));

    lines.push(Line::styled("─".repeat(width), Theme::dim()));
    lines
}

fn section(title: String) -> Row {
    Row::new(Line::from(vec![
        Span::styled("── ", Theme::dim()),
        Span::styled(title, Theme::section_title()),
        Span::styled(" ──", Theme::dim()),
    ]))
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn header_lines(lines: &mut Vec<Row>, headers: &[(String, String)], width: usize) {
    for (name, value) in headers {
        let prefix = format!("{name}: ");
        let indent = prefix.chars().count();
        let value_width = width.saturating_sub(indent).max(1);

        let chars: Vec<char> = value.chars().collect();
        let mut rows = chars.chunks(value_width);
        let first: String = rows.next().unwrap_or_default().iter().collect();
        lines.push(Row::new(Line::from(vec![
            Span::styled(prefix, Theme::header_name()),
            Span::styled(first, Theme::header_value()),
        ])));
        // Never zero: a non-zero value marks a row as a soft-wrapped continuation.
        let continuation = indent.min(width / 2).max(1);
        for row in rows {
            lines.push(Row {
                line: Line::from(vec![
                    Span::raw(" ".repeat(continuation)),
                    Span::styled(row.iter().collect::<String>(), Theme::header_value()),
                ]),
                continuation,
            });
        }
    }
}

fn body_section(
    lines: &mut Vec<Row>,
    label: &str,
    headers: &[(String, String)],
    body: &[u8],
    width: usize,
) {
    let mut title = format!("{label} · {}", format_size(body.len()));
    if let Some(content_type) = header_value(headers, "content-type") {
        title = format!("{label} · {content_type} · {}", format_size(body.len()));
    }
    lines.push(Row::new(Line::raw("")));
    lines.push(section(title));
    lines.extend(body_lines(body, width));
}

/// Section names for the request and response halves of an exchange.
struct Labels {
    request_headers: &'static str,
    request_body: &'static str,
    response_headers: &'static str,
    response_body: &'static str,
}

fn labels(protocol: Protocol) -> Labels {
    match protocol {
        Protocol::Http => Labels {
            request_headers: "Request Headers",
            request_body: "Request Body",
            response_headers: "Response Headers",
            response_body: "Response Body",
        },
        Protocol::Postgres => Labels {
            request_headers: "Query Info",
            request_body: "SQL",
            response_headers: "Result Info",
            response_body: "Result",
        },
    }
}

/// Scrollable part of the detail pane: headers and bodies of both directions.
fn content_lines(ex: &HttpExchange, width: usize) -> Vec<Row> {
    let names = labels(ex.protocol);
    let mut lines = Vec::new();

    lines.push(section(format!(
        "{} ({}) · {}",
        names.request_headers,
        ex.request.headers.len(),
        ex.request.version
    )));
    header_lines(&mut lines, &ex.request.headers, width);
    if let Some(body) = ex.request.body.as_deref().filter(|b| !b.is_empty()) {
        body_section(
            &mut lines,
            names.request_body,
            &ex.request.headers,
            body,
            width,
        );
    }

    lines.push(Row::new(Line::raw("")));
    match &ex.response {
        Some(response) => {
            lines.push(section(format!(
                "{} ({}) · {}",
                names.response_headers,
                response.headers.len(),
                response.version
            )));
            header_lines(&mut lines, &response.headers, width);
            if let Some(body) = response.body.as_deref().filter(|b| !b.is_empty()) {
                body_section(
                    &mut lines,
                    names.response_body,
                    &response.headers,
                    body,
                    width,
                );
            }
        }
        None => lines.push(Row::new(Line::styled(
            "No response was received for this request",
            Theme::status_client_error(),
        ))),
    }
    lines
}
