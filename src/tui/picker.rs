use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::theme::Theme;

/// A selectable row in the picker.
#[derive(Debug, Clone)]
pub struct PickerItem {
    /// Stable identifier used to preselect the row remembered from a previous run.
    pub key: String,
    pub label: String,
    pub detail: String,
    /// Disabled rows are shown dimmed and cannot be chosen.
    pub enabled: bool,
}

impl PickerItem {
    fn matches(&self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        self.label.to_lowercase().contains(&needle) || self.detail.to_lowercase().contains(&needle)
    }
}

/// Result of feeding a key to the picker.
#[derive(Debug, PartialEq, Eq)]
pub enum PickerAction {
    None,
    /// Index into the original item list.
    Select(usize),
    Quit,
}

/// List selection state, independent of rendering and terminal I/O.
pub struct Picker {
    title: String,
    items: Vec<PickerItem>,
    /// Indices into `items` that match the current filter.
    visible: Vec<usize>,
    filter: String,
    filtering: bool,
    state: ListState,
}

impl Picker {
    pub fn new(title: impl Into<String>, items: Vec<PickerItem>, preselect: Option<&str>) -> Self {
        let mut picker = Self {
            title: title.into(),
            visible: (0..items.len()).collect(),
            items,
            filter: String::new(),
            filtering: false,
            state: ListState::default(),
        };
        let start = preselect
            .and_then(|key| {
                picker
                    .visible
                    .iter()
                    .position(|&i| picker.items[i].key == key && picker.items[i].enabled)
            })
            .or_else(|| picker.visible.iter().position(|&i| picker.items[i].enabled))
            .or((!picker.visible.is_empty()).then_some(0));
        picker.state.select(start);
        picker
    }

    pub fn selected(&self) -> Option<usize> {
        self.state.selected().map(|row| self.visible[row])
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PickerAction {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return PickerAction::Quit;
        }
        if self.filtering {
            return self.handle_filter_key(key);
        }
        match key.code {
            KeyCode::Char('q') => PickerAction::Quit,
            KeyCode::Esc if self.filter.is_empty() => PickerAction::Quit,
            KeyCode::Esc => {
                self.set_filter(String::new());
                PickerAction::None
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                PickerAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_by(1);
                PickerAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_by(-1);
                PickerAction::None
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.state.select((!self.visible.is_empty()).then_some(0));
                PickerAction::None
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.state.select(self.visible.len().checked_sub(1));
                PickerAction::None
            }
            KeyCode::Enter => self.select_current(),
            _ => PickerAction::None,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> PickerAction {
        match key.code {
            KeyCode::Esc => {
                self.filtering = false;
                self.set_filter(String::new());
            }
            KeyCode::Enter => {
                self.filtering = false;
                return self.select_current();
            }
            KeyCode::Backspace => {
                let mut filter = self.filter.clone();
                filter.pop();
                self.set_filter(filter);
            }
            KeyCode::Down => self.move_by(1),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Char(c) => {
                let filter = format!("{}{c}", self.filter);
                self.set_filter(filter);
            }
            _ => {}
        }
        PickerAction::None
    }

    fn select_current(&self) -> PickerAction {
        match self.selected() {
            Some(i) if self.items[i].enabled => PickerAction::Select(i),
            _ => PickerAction::None,
        }
    }

    fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.visible = (0..self.items.len())
            .filter(|&i| self.items[i].matches(&self.filter))
            .collect();
        let first = self
            .visible
            .iter()
            .position(|&i| self.items[i].enabled)
            .or((!self.visible.is_empty()).then_some(0));
        self.state.select(first);
    }

    fn move_by(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let len = self.visible.len() as isize;
        let current = self.state.selected().unwrap_or(0) as isize;
        self.state
            .select(Some((current + delta).rem_euclid(len) as usize));
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let [list_area, help_area] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());

        let rows: Vec<ListItem> = self
            .visible
            .iter()
            .map(|&i| {
                let item = &self.items[i];
                let label_style = if item.enabled {
                    Theme::header_value()
                } else {
                    Theme::dim()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(item.label.clone(), label_style),
                    Span::raw("  "),
                    Span::styled(item.detail.clone(), Theme::dim()),
                ]))
            })
            .collect();

        let list = List::new(rows)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(format!(" {} ", self.title), Theme::title())),
            )
            .highlight_style(Theme::selected())
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, list_area, &mut self.state);

        let help = if self.filtering {
            format!("/{}█  Enter: select  Esc: clear", self.filter)
        } else if self.filter.is_empty() {
            "j/k: move  /: filter  Enter: select  q/Esc: quit".to_string()
        } else {
            format!(
                "filter: {}  /: edit  Esc: clear  Enter: select",
                self.filter
            )
        };
        frame.render_widget(Paragraph::new(help).style(Theme::status_bar()), help_area);
    }
}

/// Show a blocking selection screen and return the chosen item index, or `None` if cancelled.
/// The row whose key equals `preselect` starts highlighted.
pub fn pick(title: &str, items: Vec<PickerItem>, preselect: Option<&str>) -> Result<Option<usize>> {
    let mut terminal = super::init()?;
    let mut picker = Picker::new(title, items, preselect);

    let outcome = loop {
        if let Err(e) = terminal.draw(|frame| picker.render(frame)) {
            break Err(e.into());
        }
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                match picker.handle_key(key) {
                    PickerAction::None => {}
                    PickerAction::Select(i) => break Ok(Some(i)),
                    PickerAction::Quit => break Ok(None),
                }
            }
            Ok(_) => {}
            Err(e) => break Err(e.into()),
        }
    };

    super::restore()?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, enabled: bool) -> PickerItem {
        PickerItem {
            key: label.into(),
            label: label.into(),
            detail: String::new(),
            enabled,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_str(picker: &mut Picker, text: &str) {
        for c in text.chars() {
            picker.handle_key(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn starts_on_first_enabled_item() {
        let picker = Picker::new("t", vec![item("a", false), item("b", true)], None);
        assert_eq!(picker.selected(), Some(1));
    }

    #[test]
    fn preselects_remembered_key() {
        let picker = Picker::new(
            "t",
            vec![item("a", true), item("b", true), item("c", true)],
            Some("c"),
        );
        assert_eq!(picker.selected(), Some(2));
    }

    #[test]
    fn unknown_preselect_falls_back_to_first_enabled() {
        let picker = Picker::new("t", vec![item("a", true)], Some("gone"));
        assert_eq!(picker.selected(), Some(0));
    }

    #[test]
    fn navigation_wraps() {
        let mut picker = Picker::new("t", vec![item("a", true), item("b", true)], None);
        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.selected(), Some(1));
        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.selected(), Some(0));
    }

    #[test]
    fn enter_selects_enabled_item() {
        let mut picker = Picker::new("t", vec![item("a", true), item("b", true)], None);
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(
            picker.handle_key(key(KeyCode::Enter)),
            PickerAction::Select(1)
        );
    }

    #[test]
    fn enter_ignores_disabled_item() {
        let mut picker = Picker::new("t", vec![item("a", false)], None);
        assert_eq!(picker.handle_key(key(KeyCode::Enter)), PickerAction::None);
    }

    #[test]
    fn quit_keys() {
        let mut picker = Picker::new("t", vec![item("a", true)], None);
        assert_eq!(
            picker.handle_key(key(KeyCode::Char('q'))),
            PickerAction::Quit
        );
        assert_eq!(picker.handle_key(key(KeyCode::Esc)), PickerAction::Quit);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(picker.handle_key(ctrl_c), PickerAction::Quit);
    }

    #[test]
    fn empty_list_is_safe() {
        let mut picker = Picker::new("t", vec![], None);
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.handle_key(key(KeyCode::Enter)), PickerAction::None);
    }

    #[test]
    fn filter_narrows_and_select_returns_original_index() {
        let mut picker = Picker::new(
            "t",
            vec![item("alpha", true), item("beta", true), item("gamma", true)],
            None,
        );
        picker.handle_key(key(KeyCode::Char('/')));
        type_str(&mut picker, "BET");
        assert_eq!(
            picker.handle_key(key(KeyCode::Enter)),
            PickerAction::Select(1)
        );
    }

    #[test]
    fn typing_q_in_filter_mode_does_not_quit() {
        let mut picker = Picker::new("t", vec![item("quux", true)], None);
        picker.handle_key(key(KeyCode::Char('/')));
        assert_eq!(
            picker.handle_key(key(KeyCode::Char('q'))),
            PickerAction::None
        );
        assert_eq!(picker.selected(), Some(0));
    }

    #[test]
    fn esc_clears_filter_before_quitting() {
        let mut picker = Picker::new("t", vec![item("a", true), item("b", true)], None);
        picker.handle_key(key(KeyCode::Char('/')));
        type_str(&mut picker, "b");
        picker.handle_key(key(KeyCode::Enter));
        // Enter selected; filter stays active in normal mode.
        assert_eq!(picker.handle_key(key(KeyCode::Esc)), PickerAction::None);
        assert_eq!(picker.handle_key(key(KeyCode::Esc)), PickerAction::Quit);
    }

    #[test]
    fn filter_with_no_match_selects_nothing() {
        let mut picker = Picker::new("t", vec![item("a", true)], None);
        picker.handle_key(key(KeyCode::Char('/')));
        type_str(&mut picker, "zzz");
        assert_eq!(picker.selected(), None);
        assert_eq!(picker.handle_key(key(KeyCode::Enter)), PickerAction::None);
    }
}
