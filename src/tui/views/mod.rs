pub mod detail;
pub mod exchange_list;
pub mod status_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::{App, Pane};

pub fn render(frame: &mut Frame, app: &mut App) {
    let has_error = app.error_message.is_some();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_error {
            vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Length(1),
                Constraint::Length(0),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
        })
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

    if let Some(ref error) = app.error_message {
        let error_style = Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD);
        let error_lines: Vec<Line> = error.lines().map(|l| Line::raw(format!(" {l}"))).collect();
        let error_widget = Paragraph::new(error_lines).style(error_style);
        frame.render_widget(error_widget, chunks[1]);
    }

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[2]);

    exchange_list::render(frame, app, main_chunks[0], app.active_pane == Pane::List);
    detail::render(frame, app, main_chunks[1], app.active_pane == Pane::Detail);

    app.list_area = main_chunks[0];
    app.detail_area = main_chunks[1];

    status_bar::render(frame, app, chunks[3]);
}
