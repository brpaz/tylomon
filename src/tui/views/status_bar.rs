use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, ViewMode};
use crate::tui::theme::Theme;

/// Render the bottom status bar with mode indicators, key hints and counts.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.exchanges.len();
    let shown = app.filtered_exchanges().len();

    let mut spans = Vec::new();
    if app.paused {
        spans.push(Span::styled(" ⏸ PAUSED ", Theme::pause_indicator()));
    }
    if app.zoom {
        spans.push(Span::styled(" ⛶ ZOOM ", Theme::follow_indicator()));
    }
    if app.follow {
        spans.push(Span::styled(" ⏬ FOLLOW ", Theme::follow_indicator()));
    }

    if app.filter_active {
        spans.push(Span::styled(" /", Theme::title()));
        spans.push(Span::raw(app.filter_input.as_str()));
        spans.push(Span::styled("▌", Theme::title()));
        spans.push(Span::styled("  [Enter]apply  [Esc]cancel", Theme::dim()));
    } else {
        // A message replaces the key hints so it is never pushed off the right edge.
        if let Some(message) = &app.flash_message {
            spans.push(Span::styled(
                format!(" ✓ {message} "),
                Theme::flash_indicator(),
            ));
        } else {
            let hints = match (app.view, app.filter_text.is_empty()) {
                (ViewMode::List, _) if app.zoom => {
                    " [q]unzoom  [j/k]nav/scroll  [tab]other pane  [/]filter  [?]help  "
                }
                (ViewMode::Map, _) => {
                    " [m]back  [j/k]nav  [Enter]show requests  [/]filter  [?]help  "
                }
                (ViewMode::List, false) => " [q]clear filter  [/]edit  [s]ort  [m]ap  [?]help  ",
                (ViewMode::List, true) => {
                    " [q]uit  [j/k]nav  [tab]pane  [z]oom  [/]filter  [s]ort  [m]ap  [f]ollow  [?]help  "
                }
            };
            spans.push(Span::raw(hints));
        }
        let counts = if app.filter_text.is_empty() {
            format!("| {total} exchanges")
        } else {
            format!("| filter: \"{}\" ({shown}/{total})", app.filter_text)
        };
        spans.push(Span::styled(counts, Theme::dim()));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Theme::status_bar()),
        area,
    );
}
