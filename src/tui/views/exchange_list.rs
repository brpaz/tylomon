use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::app::App;
use crate::tui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let border_style = if focused {
        Theme::title()
    } else {
        Style::default()
    };
    let block = Block::default()
        .title(" Exchanges ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let filtered = app.filtered_exchanges();

    if filtered.is_empty() {
        let msg = if app.filter_text.is_empty() {
            "  No exchanges captured yet..."
        } else {
            "  No exchanges match filter"
        };
        let empty = ratatui::widgets::Paragraph::new(msg)
            .style(Theme::dim())
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|(_, exchange)| {
            let method_style = match exchange.request.method.as_str() {
                "GET" => Theme::method_get(),
                "POST" => Theme::method_post(),
                "PUT" | "PATCH" => Theme::method_put(),
                "DELETE" => Theme::method_delete(),
                _ => Theme::dim(),
            };

            let status_style = match exchange.response.as_ref().map(|r| r.status_code) {
                Some(code) if code < 300 => Theme::status_success(),
                Some(code) if code < 400 => Theme::status_redirect(),
                Some(code) if code < 500 => Theme::status_client_error(),
                Some(_) => Theme::status_server_error(),
                None => Theme::dim(),
            };

            let status_text = exchange
                .response
                .as_ref()
                .map(|r| format!("{}", r.status_code))
                .unwrap_or_else(|| "...".to_string());

            let duration_text = exchange
                .duration_ms
                .map(|d| format!(" {d}ms"))
                .unwrap_or_default();

            let src_name = app
                .container_names
                .get(&exchange.src_ip)
                .map(|s| s.as_str())
                .unwrap_or(&exchange.src_ip);
            let dst_name = app
                .container_names
                .get(&exchange.dst_ip)
                .map(|s| s.as_str())
                .unwrap_or(&exchange.dst_ip);

            let line = Line::from(vec![
                Span::styled(format!("{:<7}", exchange.request.method), method_style),
                Span::raw(&exchange.request.path),
                Span::raw(" "),
                Span::styled(status_text, status_style),
                Span::styled(duration_text, Theme::dim()),
                Span::styled(format!(" {src_name}\u{2192}{dst_name}"), Theme::dim()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(app.selected_index);

    let list = List::new(items)
        .block(block)
        .highlight_style(Theme::selected());

    frame.render_stateful_widget(list, area, &mut state);
}
