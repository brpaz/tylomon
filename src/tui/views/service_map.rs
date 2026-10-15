use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::tui::theme::Theme;

fn ms(value: Option<u64>) -> Cell<'static> {
    match value {
        Some(v) => Cell::from(format!("{v}ms")).style(Theme::duration(Some(v))),
        None => Cell::from("-").style(Theme::dim()),
    }
}

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    app.map_area = area;
    let block = Block::default()
        .title(" Service map ")
        .borders(Borders::ALL)
        .border_style(Theme::title());

    let edges = app.service_edges();
    if edges.is_empty() {
        let msg = if app.filter_text.is_empty() {
            "  No exchanges captured yet..."
        } else {
            "  No exchanges match filter"
        };
        frame.render_widget(Paragraph::new(msg).style(Theme::dim()).block(block), area);
        return;
    }

    let header = Row::new([
        "Source → Destination",
        "Requests",
        "4xx",
        "Failed",
        "Fail%",
        "p50",
        "p95",
    ])
    .style(Theme::header_name());

    let rows: Vec<Row> = edges
        .iter()
        .map(|edge| {
            let failed_style = if edge.failures > 0 {
                Theme::status_server_error()
            } else {
                Theme::dim()
            };
            let client_style = if edge.client_errors > 0 {
                Theme::status_client_error()
            } else {
                Theme::dim()
            };
            Row::new(vec![
                Cell::from(Line::from(vec![
                    Span::raw(edge.src.clone()),
                    Span::styled(" → ", Theme::dim()),
                    Span::raw(format!("{}:{}", edge.dst, edge.dst_port)),
                ])),
                Cell::from(edge.requests.to_string()),
                Cell::from(edge.client_errors.to_string()).style(client_style),
                Cell::from(edge.failures.to_string()).style(failed_style),
                Cell::from(format!("{:.1}%", edge.failure_rate())).style(failed_style),
                ms(edge.p50_ms),
                ms(edge.p95_ms),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(30),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(9),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Theme::selected())
        .highlight_symbol("▶ ")
        .style(Style::default());

    let mut state = std::mem::take(&mut app.table_state);
    state.select(Some(app.map_selected.min(edges.len() - 1)));
    frame.render_stateful_widget(table, area, &mut state);
    app.table_state = state;
}
