use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::tui::theme::Theme;

/// Render the bottom status bar with keybindings and stats
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let exchange_count = app.exchanges.len();
    let filtered_count = app.filtered_exchanges().len();

    let pause_indicator = if app.paused {
        Span::styled(" ⏸ PAUSED ", Theme::pause_indicator())
    } else {
        Span::raw("")
    };

    let flash = app
        .flash_message
        .as_ref()
        .map(|msg| Span::styled(format!(" ✓ {msg} "), Theme::flash_indicator()));

    let line = if app.filter_active {
        Line::from(vec![
            pause_indicator,
            Span::styled(" /", Theme::title()),
            Span::raw(&app.filter_input),
            Span::styled("▌", Theme::title()),
            Span::styled("  [Enter]apply  [Esc]cancel", Theme::dim()),
        ])
    } else if !app.filter_text.is_empty() {
        let mut spans = vec![
            pause_indicator,
            Span::raw(
                " [q]clear filter  [/]edit  [tab]pane  [p]ause  [y]copy [1]req [2]res  [c]lear  ",
            ),
            Span::styled(
                format!(
                    "| filter: \"{}\" ({filtered_count}/{exchange_count})",
                    app.filter_text
                ),
                Theme::dim(),
            ),
        ];
        if let Some(f) = flash {
            spans.push(f);
        }
        Line::from(spans)
    } else {
        let mut spans = vec![
            pause_indicator,
            Span::raw(
                " [q]uit  [j/k]nav  [tab]pane  [/]filter  [p]ause  [y]copy [1]req [2]res  [c]lear  ",
            ),
            Span::styled(format!("| {exchange_count} exchanges"), Theme::dim()),
        ];
        if let Some(f) = flash {
            spans.push(f);
        }
        Line::from(spans)
    };

    let status = Paragraph::new(line).style(Theme::status_bar());
    frame.render_widget(status, area);
}
