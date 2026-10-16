use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::tui::theme::Theme;

const KEY_COLUMN: usize = 16;

/// Every key binding by section. Keep in sync with `App::handle_key`.
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "General",
        &[
            ("?", "Toggle this help"),
            ("q / Esc", "Clear the filter, or quit"),
            ("Ctrl+C", "Quit immediately"),
            ("Tab", "Switch focus between list and detail"),
            ("z", "Zoom the focused pane full screen (q / Esc leaves)"),
            ("< / >", "Narrow / widen the list pane (also , and .)"),
            ("=", "Reset the pane split"),
            ("p", "Pause / resume capture"),
            ("c", "Clear all exchanges"),
        ],
    ),
    (
        "List",
        &[
            ("j / k", "Select next / previous exchange"),
            ("g / G", "First exchange / last exchange and follow"),
            ("PgUp / PgDn", "Move 10 rows"),
            ("f", "Follow: keep the newest exchange selected"),
            ("s", "Sort by capture order / slowest first"),
            ("m", "Service map"),
        ],
    ),
    (
        "Detail",
        &[
            ("j / k", "Scroll"),
            ("g / G", "Top / bottom"),
            ("PgUp / PgDn", "Scroll 10 lines"),
        ],
    ),
    (
        "Filter",
        &[
            ("/", "Edit the filter (Enter applies, Esc cancels)"),
            ("word", "Method, path, status, source or destination"),
            ("src:api dst:db", "Container name or IP"),
            ("status:5xx", "Also status:404, status:>=400"),
            (">500ms", "Response time; also <2s, >=1s"),
            (
                "proto:pg",
                "PostgreSQL queries; also proto:grpc, proto:http",
            ),
            ("!term  -term", "Negate a term"),
        ],
    ),
    (
        "Copy & export",
        &[
            ("y", "Copy the whole exchange"),
            ("1 / 2", "Copy the request / response"),
            ("3", "Copy as a curl command"),
            ("e", "Export the listed exchanges to a HAR file"),
        ],
    ),
    (
        "Mouse",
        &[
            ("Click", "Select an exchange / edge, focus a pane"),
            (
                "Double-click",
                "Open the detail pane / show an edge's requests",
            ),
            (
                "Wheel",
                "Scroll the detail pane, move the selection, scroll help",
            ),
            ("Drag in detail", "Select text; it is copied on release"),
            (
                "Double-click word",
                "In the detail pane: select and copy the word",
            ),
            ("Esc", "Clear the text selection"),
            ("Drag border", "Resize the list and detail panes"),
            ("Double-click border", "Reset the pane split"),
            ("Click outside", "Close this help"),
        ],
    ),
    (
        "Service map",
        &[
            ("j / k", "Select an edge"),
            ("Enter", "Show that edge's requests"),
            ("m / q / Esc", "Back to the list"),
        ],
    ),
];

fn lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, (title, bindings)) in SECTIONS.iter().enumerate() {
        if i > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(*title, Theme::section_title()));
        for (key, description) in *bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key:<KEY_COLUMN$}"), Theme::header_name()),
                Span::styled(*description, Theme::header_value()),
            ]));
        }
    }
    lines
}

/// Centered popup listing all key bindings; scrolls when the terminal is short.
pub fn render(frame: &mut Frame, app: &mut App) {
    let content = lines();
    let screen = frame.area();

    let [popup] = Layout::horizontal([Constraint::Length(72.min(screen.width))])
        .flex(Flex::Center)
        .areas(screen);
    let [popup] = Layout::vertical([Constraint::Length(
        (content.len() as u16 + 2).min(screen.height),
    )])
    .flex(Flex::Center)
    .areas(popup);

    app.help_area = popup;
    let viewport = popup.height.saturating_sub(2) as usize;
    let max_scroll = content.len().saturating_sub(viewport);
    app.help_scroll = app.help_scroll.min(max_scroll as u16);

    let mut block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Theme::title());
    if max_scroll > 0 {
        block = block.title_bottom(Line::styled(" j/k scroll · ? close ", Theme::dim()).centered());
    } else {
        block = block.title_bottom(Line::styled(" ? close ", Theme::dim()).centered());
    }

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(content)
            .block(block)
            .scroll((app.help_scroll, 0)),
        popup,
    );
}
