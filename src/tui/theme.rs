use ratatui::style::{Color, Modifier, Style};

/// Color scheme for the application
pub struct Theme;

impl Theme {
    pub fn title() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    pub fn selected() -> Style {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }

    pub fn method_get() -> Style {
        Style::default().fg(Color::Green)
    }

    pub fn method_post() -> Style {
        Style::default().fg(Color::Yellow)
    }

    pub fn method_put() -> Style {
        Style::default().fg(Color::Blue)
    }

    pub fn method_delete() -> Style {
        Style::default().fg(Color::Red)
    }

    pub fn status_success() -> Style {
        Style::default().fg(Color::Green)
    }

    pub fn status_redirect() -> Style {
        Style::default().fg(Color::Yellow)
    }

    pub fn status_client_error() -> Style {
        Style::default().fg(Color::Red)
    }

    pub fn status_server_error() -> Style {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    pub fn header_name() -> Style {
        Style::default().fg(Color::Cyan)
    }

    pub fn header_value() -> Style {
        Style::default().fg(Color::White)
    }

    pub fn dim() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    pub fn status_bar() -> Style {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }

    pub fn pause_indicator() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }

    pub fn flash_indicator() -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }
}
