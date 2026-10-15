pub mod body;
pub mod detail;
pub mod exchange_list;
pub mod help;
pub mod service_map;
pub mod status_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::{App, Pane, ViewMode};

const MAX_ERROR_ROWS: usize = 6;

pub fn render(frame: &mut Frame, app: &mut App) {
    let error = app.error_banner();
    // Banner height follows its text, capped so it cannot crowd out the panes.
    let error_rows = error
        .as_deref()
        .map_or(0, |e| e.lines().count().clamp(1, MAX_ERROR_ROWS) as u16);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(error_rows),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let title_text = if app.network_label.is_empty() {
        " tylomon - Docker HTTP Traffic Monitor".to_string()
    } else {
        format!(
            " tylomon - monitoring: {} ({})",
            app.network_label, app.bridge_label
        )
    };
    let title = Paragraph::new(title_text).style(crate::tui::theme::Theme::title());
    frame.render_widget(title, chunks[0]);

    if let Some(error) = error {
        let error_style = Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD);
        let error_lines: Vec<Line> = error.lines().map(|l| Line::raw(format!(" {l}"))).collect();
        let error_widget = Paragraph::new(error_lines).style(error_style);
        frame.render_widget(error_widget, chunks[1]);
    }

    if app.view == ViewMode::Map {
        service_map::render(frame, app, chunks[2]);
        app.list_area = Rect::default();
        app.detail_area = Rect::default();
    } else {
        app.map_area = Rect::default();
        let list_focused = app.active_pane == Pane::List;
        let area = chunks[2];

        if app.zoom {
            // Only the focused pane is drawn; the other has no area for mouse hit-testing.
            if list_focused {
                exchange_list::render(frame, app, area, true);
                app.list_area = area;
                app.detail_area = Rect::default();
            } else {
                detail::render(frame, app, area, true);
                app.list_area = Rect::default();
                app.detail_area = area;
            }
        } else {
            let list_width = app.list_pane_width(area.width);
            let [list_area, detail_area] =
                Layout::horizontal([Constraint::Length(list_width), Constraint::Min(0)])
                    .areas(area);

            exchange_list::render(frame, app, list_area, list_focused);
            detail::render(frame, app, detail_area, !list_focused);
            app.list_area = list_area;
            app.detail_area = detail_area;
        }
    }

    status_bar::render(frame, app, chunks[3]);

    if app.show_help {
        help::render(frame, app);
    }
}
