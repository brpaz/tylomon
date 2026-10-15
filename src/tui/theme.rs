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

    /// Style for a response time: dim, yellow from 300 ms, red from 1 s.
    pub fn duration(ms: Option<u64>) -> Style {
        match ms {
            Some(d) if d >= 1000 => Self::duration_slow(),
            Some(d) if d >= 300 => Self::duration_warn(),
            _ => Self::dim(),
        }
    }

    pub fn duration_warn() -> Style {
        Style::default().fg(Color::Yellow)
    }

    pub fn duration_slow() -> Style {
        Style::default().fg(Color::Red)
    }

    pub fn section_title() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    pub fn follow_indicator() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// Badge for a response status: filled background keyed by status class.
    pub fn status_badge(status: Option<u16>) -> Style {
        let bg = match status {
            Some(code) if code < 300 => Color::Green,
            Some(code) if code < 400 => Color::Yellow,
            Some(_) => Color::Red,
            None => Color::Red,
        };
        Style::default()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn method(method: &str) -> Style {
        match method {
            "GET" | "SELECT" => Self::method_get(),
            "gRPC" => Style::default().fg(Color::Magenta),
            "POST" | "INSERT" | "UPDATE" | "MERGE" => Self::method_post(),
            "PUT" | "PATCH" | "CREATE" | "ALTER" => Self::method_put(),
            "DELETE" | "DROP" | "TRUNCATE" => Self::method_delete(),
            _ => Self::dim(),
        }
    }

    pub fn json_key() -> Style {
        Style::default().fg(Color::Cyan)
    }

    pub fn json_string() -> Style {
        Style::default().fg(Color::Green)
    }

    pub fn json_number() -> Style {
        Style::default().fg(Color::Yellow)
    }

    pub fn json_keyword() -> Style {
        Style::default().fg(Color::Magenta)
    }

    pub fn json_punctuation() -> Style {
        Style::default().fg(Color::Gray)
    }

    pub fn hex_offset() -> Style {
        Self::dim()
    }

    pub fn hex_ascii() -> Style {
        Style::default().fg(Color::Yellow)
    }

    /// Text selected with the mouse in the detail pane.
    pub fn selection() -> Style {
        Style::default().bg(Color::LightBlue).fg(Color::Black)
    }
}
