use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};

use crate::export;
use crate::filter::Filter;
use crate::http::types::HttpExchange;
use crate::stats::{self, EdgeStats};
use crate::tui::rows::{Pos, Row, Selection, selected_text, word_at};

const DOUBLE_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Debug, Clone)]
pub enum Action {
    NewExchange(Box<HttpExchange>),
    ContainerUpdate {
        ip: String,
        name: String,
    },
    /// A capture on `interface` could not run.
    CaptureError {
        interface: String,
        message: String,
    },
    /// A capture on `interface` is running; clears any earlier error for it.
    CaptureStarted(String),
    /// Transient message shown in the status bar.
    Notice(String),
    /// Current `(network name, bridge interface)` pairs being captured.
    Networks(Vec<(String, String)>),
    KeyPress(crossterm::event::KeyEvent),
    MouseScroll {
        column: u16,
        row: u16,
        delta: i16,
    },
    MouseClick {
        column: u16,
        row: u16,
    },
    MouseDrag {
        column: u16,
        row: u16,
    },
    MouseRelease,
    Resize(u16, u16),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pane {
    List,
    Detail,
}

/// Ordering of the exchange list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    /// Capture order.
    Time,
    /// Slowest first; pending exchanges last.
    Duration,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Time => "time",
            SortMode::Duration => "slowest",
        }
    }
}

/// Main screen content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Exchange list and detail panes.
    List,
    /// Per-edge traffic summary.
    Map,
}

/// Default share of the screen taken by the list pane, in percent.
const DEFAULT_LIST_PERCENT: u16 = 40;
/// Narrowest either pane may become when resizing.
const MIN_PANE_WIDTH: u16 = 20;
/// Columns moved per resize key press.
const RESIZE_STEP: u16 = 4;

/// Rendered scrollable detail content for one exchange at one width.
pub struct DetailCache {
    pub id: u64,
    pub width: usize,
    pub rows: Vec<Row>,
}

pub struct App {
    pub exchanges: Vec<HttpExchange>,
    pub max_exchanges: usize,
    /// Id of the selected exchange; selection follows the exchange, not its row.
    pub selected_id: Option<u64>,
    next_id: u64,
    /// Keep the newest exchange selected as new ones arrive.
    pub follow: bool,
    pub show_help: bool,
    pub help_scroll: u16,
    pub detail_cache: Option<DetailCache>,
    /// Text selected with the mouse in the detail pane.
    pub selection: Option<Selection>,
    selecting: bool,
    selection_moved: bool,
    /// Screen area of the detail pane's scrollable text, for mapping the mouse to characters.
    pub detail_text_area: Rect,
    /// Widget state kept across frames so clicks can be mapped to rows.
    pub list_state: ListState,
    pub table_state: TableState,
    pub map_area: Rect,
    pub help_area: Rect,
    /// Width of the list pane in columns; `None` uses the default share of the screen.
    pub list_width: Option<u16>,
    /// Show only the focused pane, full screen.
    pub zoom: bool,
    dragging: bool,
    last_click: Option<(Instant, u16, u16)>,
    pub should_quit: bool,
    pub container_names: std::collections::HashMap<String, String>,
    pub detail_scroll: u16,
    pub network_label: String,
    pub bridge_label: String,
    pub active_pane: Pane,
    pub filter_text: String,
    filter: Filter,
    pub sort: SortMode,
    pub view: ViewMode,
    pub map_selected: usize,
    pub filter_input: String,
    pub filter_active: bool,
    /// Capture failures by interface, shown in a banner until the capture runs again or
    /// its network goes away.
    capture_errors: std::collections::BTreeMap<String, String>,
    pub paused: bool,
    pub detail_area: Rect,
    pub list_area: Rect,
    pub flash_message: Option<String>,
    pub flash_expiry: Option<Instant>,
    clipboard: Option<arboard::Clipboard>,
}

impl App {
    pub fn new(max_exchanges: usize) -> Self {
        Self {
            exchanges: Vec::new(),
            max_exchanges,
            selected_id: None,
            next_id: 0,
            follow: false,
            show_help: false,
            help_scroll: 0,
            detail_cache: None,
            selection: None,
            selecting: false,
            selection_moved: false,
            detail_text_area: Rect::default(),
            list_state: ListState::default(),
            table_state: TableState::default(),
            map_area: Rect::default(),
            help_area: Rect::default(),
            list_width: None,
            zoom: false,
            dragging: false,
            last_click: None,
            should_quit: false,
            container_names: std::collections::HashMap::new(),
            detail_scroll: 0,
            network_label: String::new(),
            bridge_label: String::new(),
            active_pane: Pane::List,
            filter_text: String::new(),
            filter: Filter::default(),
            sort: SortMode::Time,
            view: ViewMode::List,
            map_selected: 0,
            filter_input: String::new(),
            filter_active: false,
            capture_errors: std::collections::BTreeMap::new(),
            paused: false,
            detail_area: Rect::default(),
            list_area: Rect::default(),
            flash_message: None,
            flash_expiry: None,
            clipboard: arboard::Clipboard::new()
                .inspect_err(|e| tracing::warn!("Clipboard init failed: {e}"))
                .ok(),
        }
    }

    pub fn update(&mut self, action: Action) {
        match action {
            Action::NewExchange(exchange) => {
                if self.paused {
                    return;
                }
                let mut exchange = *exchange;
                exchange.id = self.next_id;
                self.next_id += 1;
                self.exchanges.push(exchange);
                if self.exchanges.len() > self.max_exchanges {
                    self.exchanges.remove(0);
                }
                self.ensure_selection();
                if self.follow {
                    self.select_last();
                }
            }
            Action::ContainerUpdate { ip, name } => {
                self.container_names.insert(ip, name);
            }
            Action::CaptureError { interface, message } => {
                self.capture_errors.insert(interface, message);
            }
            Action::CaptureStarted(interface) => {
                self.capture_errors.remove(&interface);
            }
            Action::Notice(msg) => self.flash(&msg, 5),
            Action::Networks(networks) => {
                self.capture_errors
                    .retain(|interface, _| networks.iter().any(|(_, bridge)| bridge == interface));
                self.network_label = networks
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.bridge_label = networks
                    .iter()
                    .map(|(_, b)| b.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            Action::KeyPress(key) => {
                self.handle_key(key);
            }
            Action::MouseScroll { column, row, delta } => {
                self.handle_mouse_scroll(column, row, delta);
            }
            Action::MouseClick { column, row } => {
                self.handle_mouse_click(column, row);
            }
            Action::MouseDrag { column, row } => self.handle_mouse_drag(column, row),
            Action::MouseRelease => {
                self.dragging = false;
                self.finish_selection();
            }
            Action::Resize(_, _) => {}
            Action::Quit => {
                self.should_quit = true;
            }
        }
    }

    pub fn tick(&mut self) {
        if let Some(expiry) = self.flash_expiry
            && Instant::now() >= expiry
        {
            self.flash_message = None;
            self.flash_expiry = None;
        }
    }

    /// Exchanges matching the active filter in display order, paired with their index in
    /// `exchanges`.
    pub fn filtered_exchanges(&self) -> Vec<(usize, &HttpExchange)> {
        let mut rows: Vec<_> = self
            .exchanges
            .iter()
            .enumerate()
            .filter(|(_, ex)| self.filter.matches(ex, &self.container_names))
            .collect();
        if self.sort == SortMode::Duration {
            rows.sort_by_key(|(_, ex)| std::cmp::Reverse(ex.duration_ms));
        }
        rows
    }

    /// List pane width for a screen `total` columns wide, honouring a user-set width but
    /// keeping both panes usable.
    pub fn list_pane_width(&self, total: u16) -> u16 {
        let wanted = self
            .list_width
            .unwrap_or_else(|| total * DEFAULT_LIST_PERCENT / 100);
        Self::clamp_pane_width(wanted, total)
    }

    fn clamp_pane_width(wanted: u16, total: u16) -> u16 {
        let min = MIN_PANE_WIDTH.min(total / 2);
        wanted.clamp(min, total.saturating_sub(min).max(min))
    }

    /// Resize the list pane by `delta` columns from its current width.
    fn resize_list(&mut self, delta: i16) {
        let total = self.list_area.width + self.detail_area.width;
        if total == 0 {
            return;
        }
        let current = self.list_area.width as i16;
        self.list_width = Some(Self::clamp_pane_width(
            (current + delta).max(0) as u16,
            total,
        ));
    }

    /// True if `column` is on one of the two border columns shared by the panes.
    fn on_divider(&self, column: u16, row: u16) -> bool {
        let area = self.list_area;
        area.width > 0
            && self.detail_area.width > 0
            && row >= area.y
            && row < area.y + area.height
            && (column == area.right().saturating_sub(1) || column == self.detail_area.x)
    }

    fn handle_mouse_drag(&mut self, column: u16, row: u16) {
        if self.dragging {
            let total = self.list_area.width + self.detail_area.width;
            let wanted = column.saturating_sub(self.list_area.x) + 1;
            self.list_width = Some(Self::clamp_pane_width(wanted, total));
        } else if self.selecting {
            self.extend_selection(column, row);
        }
    }

    /// Character position in the detail rows under a screen cell. Cells outside the text area
    /// map to the nearest edge row, and columns past the end of a row to its last character.
    fn text_pos(&self, column: u16, row: u16) -> Option<(u64, Pos)> {
        let area = self.detail_text_area;
        let cache = self.detail_cache.as_ref()?;
        if cache.rows.is_empty() || area.height == 0 {
            return None;
        }
        let screen_row = row.clamp(area.y, area.bottom() - 1) - area.y;
        let line = (self.detail_scroll as usize + screen_row as usize).min(cache.rows.len() - 1);
        let col =
            (column.saturating_sub(area.x) as usize).min(cache.rows[line].len().saturating_sub(1));
        Some((cache.id, Pos { line, col }))
    }

    fn begin_selection(&mut self, column: u16, row: u16, double: bool) {
        let position = ratatui::layout::Position { x: column, y: row };
        if !self.detail_text_area.contains(position) {
            return;
        }
        let Some((id, pos)) = self.text_pos(column, row) else {
            return;
        };

        if double
            && let Some(cache) = &self.detail_cache
            && let Some((anchor, cursor)) = word_at(&cache.rows, pos)
        {
            self.selection = Some(Selection { id, anchor, cursor });
            self.copy_selection();
            return;
        }
        self.selection = Some(Selection {
            id,
            anchor: pos,
            cursor: pos,
        });
        self.selecting = true;
        self.selection_moved = false;
    }

    fn extend_selection(&mut self, column: u16, row: u16) {
        // Dragging past the top or bottom edge scrolls the content.
        let area = self.detail_text_area;
        if row < area.y {
            self.detail_scroll = self.detail_scroll.saturating_sub(1);
        } else if row >= area.bottom() {
            self.detail_scroll = self.detail_scroll.saturating_add(1);
        }
        if let Some((_, pos)) = self.text_pos(column, row)
            && let Some(selection) = self.selection.as_mut()
        {
            selection.cursor = pos;
            self.selection_moved |= selection.cursor != selection.anchor;
        }
    }

    /// Copy the finished selection; a click without dragging selects nothing.
    fn finish_selection(&mut self) {
        if !self.selecting {
            return;
        }
        self.selecting = false;
        if self.selection_moved {
            self.copy_selection();
        } else {
            self.selection = None;
        }
    }

    fn copy_selection(&mut self) {
        let text = match (&self.selection, &self.detail_cache) {
            (Some(selection), Some(cache)) if selection.id == cache.id => {
                selected_text(&cache.rows, selection)
            }
            _ => return,
        };
        let chars = text.chars().count();
        self.set_clipboard(text, &format!("selection ({chars} chars)"));
    }

    /// Text of the error banner, one line per distinct failure, or `None` when all captures
    /// are healthy.
    pub fn error_banner(&self) -> Option<String> {
        let mut messages: Vec<&str> = Vec::new();
        for message in self.capture_errors.values() {
            if !messages.contains(&message.as_str()) {
                messages.push(message);
            }
        }
        (!messages.is_empty()).then(|| messages.join("\n"))
    }

    /// Row of the selected exchange in the filtered, sorted list.
    pub fn selected_row(&self) -> Option<usize> {
        let id = self.selected_id?;
        self.filtered_exchanges()
            .iter()
            .position(|(_, ex)| ex.id == id)
    }

    pub fn selected_exchange(&self) -> Option<&HttpExchange> {
        let id = self.selected_id?;
        self.filtered_exchanges()
            .into_iter()
            .map(|(_, ex)| ex)
            .find(|ex| ex.id == id)
    }

    /// Keep the selection if its exchange is still listed, otherwise select the first row.
    fn ensure_selection(&mut self) {
        let rows = self.filtered_exchanges();
        let still_listed = self
            .selected_id
            .is_some_and(|id| rows.iter().any(|(_, ex)| ex.id == id));
        if !still_listed {
            self.selected_id = rows.first().map(|(_, ex)| ex.id);
            self.detail_scroll = 0;
        }
    }

    fn select_row(&mut self, row: usize) {
        let id = self.filtered_exchanges().get(row).map(|(_, ex)| ex.id);
        if id.is_some() && id != self.selected_id {
            self.selected_id = id;
            self.detail_scroll = 0;
        }
    }

    fn select_first(&mut self) {
        self.select_row(0);
    }

    fn select_last(&mut self) {
        if let Some(last) = self.visible_count().checked_sub(1) {
            self.select_row(last);
        }
    }

    /// Traffic summary per client/server edge over the filtered exchanges.
    pub fn service_edges(&self) -> Vec<EdgeStats> {
        stats::service_map(
            self.filtered_exchanges().into_iter().map(|(_, ex)| ex),
            &self.container_names,
        )
    }

    /// Handle keys specific to the service map. Returns true if the key was consumed.
    fn handle_map_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let last = self.service_edges().len().saturating_sub(1);
        match key.code {
            KeyCode::Char('m') | KeyCode::Char('q') | KeyCode::Esc => self.view = ViewMode::List,
            KeyCode::Down | KeyCode::Char('j') => {
                self.map_selected = (self.map_selected + 1).min(last)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.map_selected = self.map_selected.saturating_sub(1)
            }
            KeyCode::Home | KeyCode::Char('g') => self.map_selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.map_selected = last,
            KeyCode::Enter => self.drill_into_edge(),
            KeyCode::Tab | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Char('s') => {}
            _ => return false,
        }
        true
    }

    /// Leave the service map, filtered to the selected edge.
    fn drill_into_edge(&mut self) {
        let edges = self.service_edges();
        let last = edges.len().saturating_sub(1);
        if let Some(edge) = edges.get(self.map_selected.min(last)) {
            self.set_filter(format!("src:{} dst:{}", edge.src, edge.dst));
            self.view = ViewMode::List;
            self.select_first();
        }
    }

    fn set_filter(&mut self, text: String) {
        self.filter = Filter::parse(&text);
        self.filter_text = text;
        self.ensure_selection();
    }

    fn visible_count(&self) -> usize {
        self.filtered_exchanges().len()
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.show_help {
            self.handle_help_key(key);
            return;
        }

        if self.filter_active {
            self.handle_filter_key(key);
            return;
        }

        if self.view == ViewMode::Map && self.handle_map_key(key) {
            return;
        }

        match key.code {
            KeyCode::Char('?') => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('m') => {
                self.view = ViewMode::Map;
                self.map_selected = 0;
            }
            KeyCode::Char('z') => {
                if self.view == ViewMode::List {
                    self.zoom = !self.zoom;
                }
            }
            KeyCode::Char('<') | KeyCode::Char(',') => self.resize_list(-(RESIZE_STEP as i16)),
            KeyCode::Char('>') | KeyCode::Char('.') => self.resize_list(RESIZE_STEP as i16),
            KeyCode::Char('=') => self.list_width = None,
            KeyCode::Char('f') => {
                self.follow = !self.follow;
                if self.follow {
                    self.select_last();
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.selection.is_some() {
                    self.selection = None;
                } else if self.zoom {
                    self.zoom = false;
                } else if !self.filter_text.is_empty() {
                    self.set_filter(String::new());
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('/') => {
                self.filter_active = true;
                self.filter_input.clear();
            }
            KeyCode::Tab => {
                self.active_pane = match self.active_pane {
                    Pane::List => Pane::Detail,
                    Pane::Detail => Pane::List,
                };
            }
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                Pane::List => self.select_next(),
                Pane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                }
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                Pane::List => self.select_previous(),
                Pane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                }
            },
            KeyCode::Home | KeyCode::Char('g') => match self.active_pane {
                Pane::List => {
                    self.follow = false;
                    self.select_first();
                }
                Pane::Detail => self.detail_scroll = 0,
            },
            KeyCode::End | KeyCode::Char('G') => match self.active_pane {
                Pane::List => {
                    self.follow = true;
                    self.select_last();
                }
                // Clamped to the content length when rendered.
                Pane::Detail => self.detail_scroll = u16::MAX,
            },
            KeyCode::PageDown => match self.active_pane {
                Pane::List => {
                    for _ in 0..10 {
                        self.select_next();
                    }
                }
                Pane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_add(10);
                }
            },
            KeyCode::PageUp => match self.active_pane {
                Pane::List => {
                    for _ in 0..10 {
                        self.select_previous();
                    }
                }
                Pane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(10);
                }
            },
            KeyCode::Char('c') => {
                self.exchanges.clear();
                self.selected_id = None;
                self.detail_scroll = 0;
                self.set_filter(String::new());
            }
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    SortMode::Time => SortMode::Duration,
                    SortMode::Duration => SortMode::Time,
                };
            }
            KeyCode::Char('p') => {
                self.paused = !self.paused;
            }
            KeyCode::Char('e') => {
                self.export_har();
            }
            KeyCode::Char('y') => {
                self.copy_to_clipboard(CopyTarget::Full);
            }
            KeyCode::Char('1') => {
                self.copy_to_clipboard(CopyTarget::Request);
            }
            KeyCode::Char('2') => {
                self.copy_to_clipboard(CopyTarget::Response);
            }
            KeyCode::Char('3') => {
                self.copy_to_clipboard(CopyTarget::Curl);
            }
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.set_filter(self.filter_input.clone());
                self.filter_active = false;
                self.select_first();
            }
            KeyCode::Esc => {
                self.filter_active = false;
                self.filter_input.clear();
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
            }
            KeyCode::Char(c) => {
                self.filter_input.push(c);
            }
            _ => {}
        }
    }

    fn select_next(&mut self) {
        let last = self.visible_count().saturating_sub(1);
        let row = self.selected_row().map_or(0, |r| (r + 1).min(last));
        self.select_row(row);
    }

    fn select_previous(&mut self) {
        self.follow = false;
        let row = self.selected_row().map_or(0, |r| r.saturating_sub(1));
        self.select_row(row);
    }

    fn handle_help_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => {
                self.show_help = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.help_scroll = self.help_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => self.help_scroll = self.help_scroll.saturating_add(10),
            KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(10),
            _ => {}
        }
    }

    /// True if this click repeats the previous one closely in time and place.
    fn is_double_click(&mut self, column: u16, row: u16) -> bool {
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(at, c, r)| {
            c == column && r == row && now.duration_since(at) <= DOUBLE_CLICK_WINDOW
        });
        // A third click starts a new sequence rather than counting as another double.
        self.last_click = (!double).then_some((now, column, row));
        double
    }

    /// Index into a bordered list or table of the item under `row`, given the widget's area,
    /// scroll offset and the number of header rows above the items.
    fn item_at(area: Rect, offset: usize, header_rows: u16, row: u16) -> Option<usize> {
        let first = area.y + 1 + header_rows;
        let last = (area.y + area.height).checked_sub(1)?;
        (row >= first && row < last).then(|| offset + (row - first) as usize)
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16) {
        let pos = ratatui::layout::Position { x: column, y: row };

        if self.show_help {
            if !self.help_area.contains(pos) {
                self.show_help = false;
            }
            return;
        }
        if self.filter_active {
            return;
        }
        self.dragging = false;
        self.selecting = false;
        self.selection = None;
        let double = self.is_double_click(column, row);

        if self.view == ViewMode::List && !self.zoom && self.on_divider(column, row) {
            if double {
                self.list_width = None;
            } else {
                self.dragging = true;
            }
            return;
        }

        if self.view == ViewMode::Map {
            if self.map_area.contains(pos)
                && let Some(index) = Self::item_at(self.map_area, self.table_state.offset(), 1, row)
                && index < self.service_edges().len()
            {
                self.map_selected = index;
                if double {
                    self.drill_into_edge();
                }
            }
            return;
        }

        if self.list_area.contains(pos) {
            self.active_pane = Pane::List;
            if let Some(index) = Self::item_at(self.list_area, self.list_state.offset(), 0, row)
                && index < self.visible_count()
            {
                self.follow = false;
                self.select_row(index);
                if double {
                    self.active_pane = Pane::Detail;
                }
            }
        } else if self.detail_area.contains(pos) {
            self.active_pane = Pane::Detail;
            self.begin_selection(column, row, double);
        }
    }

    fn handle_mouse_scroll(&mut self, column: u16, row: u16, delta: i16) {
        let pos = ratatui::layout::Position { x: column, y: row };
        let steps = delta.unsigned_abs();

        if self.show_help {
            self.help_scroll = if delta > 0 {
                self.help_scroll.saturating_add(steps)
            } else {
                self.help_scroll.saturating_sub(steps)
            };
            return;
        }

        if self.view == ViewMode::Map {
            if self.map_area.contains(pos) {
                let last = self.service_edges().len().saturating_sub(1);
                self.map_selected = if delta > 0 {
                    (self.map_selected + steps as usize).min(last)
                } else {
                    self.map_selected.saturating_sub(steps as usize)
                };
            }
            return;
        }

        if self.detail_area.contains(pos) {
            self.detail_scroll = if delta > 0 {
                self.detail_scroll.saturating_add(steps)
            } else {
                self.detail_scroll.saturating_sub(steps)
            };
        } else if self.list_area.contains(pos) {
            for _ in 0..steps {
                if delta > 0 {
                    self.select_next();
                } else {
                    self.select_previous();
                }
            }
        }
    }

    /// Save the exchanges currently listed (after filtering) to a timestamped HAR file in the
    /// working directory.
    fn export_har(&mut self) {
        let visible = self.filtered_exchanges();
        if visible.is_empty() {
            self.flash("Nothing to export", 2);
            return;
        }
        let stamp = export::iso8601(export::now_micros()).replace(':', "-");
        let path = std::path::PathBuf::from(format!("tylomon-{stamp}.har"));
        let result = export::write_har(&path, visible.iter().map(|(_, ex)| *ex));
        match result {
            Ok(()) => self.flash(
                &format!("Exported {} to {}", visible.len(), path.display()),
                4,
            ),
            Err(e) => self.flash(&format!("Export failed: {e}"), 4),
        }
    }

    fn flash(&mut self, message: &str, seconds: u64) {
        self.flash_message = Some(message.to_string());
        self.flash_expiry = Some(Instant::now() + std::time::Duration::from_secs(seconds));
    }

    fn copy_to_clipboard(&mut self, target: CopyTarget) {
        let Some(exchange) = self.selected_exchange().cloned() else {
            return;
        };

        let (text, label) = match target {
            CopyTarget::Full => (self.format_exchange_text(&exchange), "exchange"),
            CopyTarget::Curl => (export::curl_command(&exchange), "curl command"),
            CopyTarget::Request => (self.format_request_text(&exchange), "request"),
            CopyTarget::Response => match self.format_response_text(&exchange) {
                Some(t) => (t, "response"),
                None => {
                    self.flash_message = Some("No response yet".to_string());
                    self.flash_expiry = Some(Instant::now() + std::time::Duration::from_secs(2));
                    return;
                }
            },
        };

        self.set_clipboard(text, label);
    }

    fn set_clipboard(&mut self, text: String, label: &str) {
        match self.clipboard.as_mut() {
            Some(cb) => match cb.set_text(text) {
                Ok(()) => self.flash(&format!("Copied {label} to clipboard"), 2),
                Err(e) => self.flash(&format!("Clipboard error: {e}"), 3),
            },
            None => self.flash("Clipboard unavailable", 3),
        }
    }

    fn format_request_text(&self, exchange: &HttpExchange) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "{} {} {}\n",
            exchange.request.method, exchange.request.path, exchange.request.version
        ));

        let src_label = self
            .container_names
            .get(&exchange.src_ip)
            .cloned()
            .unwrap_or_else(|| exchange.src_ip.clone());
        let dst_label = self
            .container_names
            .get(&exchange.dst_ip)
            .cloned()
            .unwrap_or_else(|| exchange.dst_ip.clone());
        out.push_str(&format!(
            "From: {}:{}  To: {}:{}\n",
            src_label, exchange.src_port, dst_label, exchange.dst_port
        ));

        for (name, value) in &exchange.request.headers {
            out.push_str(&format!("{name}: {value}\n"));
        }

        if let Some(body) = &exchange.request.body {
            out.push('\n');
            out.push_str(&format_body(body));
        }

        out
    }

    fn format_response_text(&self, exchange: &HttpExchange) -> Option<String> {
        let response = exchange.response.as_ref()?;
        let mut out = String::new();

        out.push_str(&format!(
            "{} {} {}\n",
            response.version, response.status_code, response.reason
        ));

        if let Some(duration) = exchange.duration_ms {
            out.push_str(&format!("Duration: {duration}ms\n"));
        }

        for (name, value) in &response.headers {
            out.push_str(&format!("{name}: {value}\n"));
        }

        if let Some(body) = &response.body {
            out.push('\n');
            out.push_str(&format_body(body));
        }

        Some(out)
    }

    fn format_exchange_text(&self, exchange: &HttpExchange) -> String {
        let mut out = self.format_request_text(exchange);

        if let Some(response_text) = self.format_response_text(exchange) {
            out.push('\n');
            out.push_str(&response_text);
        }

        out
    }
}

enum CopyTarget {
    Full,
    Curl,
    Request,
    Response,
}

fn format_body(body: &[u8]) -> String {
    let body_str = String::from_utf8_lossy(body);
    serde_json::from_str::<serde_json::Value>(&body_str)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| body_str.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::HttpRequest;

    fn exchange(path: &str, ms: Option<u64>) -> HttpExchange {
        HttpExchange {
            request: HttpRequest {
                method: "GET".into(),
                path: path.into(),
                version: "HTTP/1.1".into(),
                headers: vec![],
                body: None,
            },
            response: None,
            src_ip: "10.0.0.2".into(),
            dst_ip: "10.0.0.3".into(),
            src_port: 1,
            dst_port: 80,
            id: 0,
            timestamp_us: None,
            duration_ms: ms,
        }
    }

    fn key(c: char) -> Action {
        Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        ))
    }

    fn app_with(exchanges: Vec<HttpExchange>) -> App {
        let mut app = App::new(100);
        for ex in exchanges {
            app.update(Action::NewExchange(Box::new(ex)));
        }
        app
    }

    fn paths(app: &App) -> Vec<String> {
        app.filtered_exchanges()
            .iter()
            .map(|(_, ex)| ex.request.path.clone())
            .collect()
    }

    #[test]
    fn sort_key_orders_slowest_first_with_pending_last() {
        let mut app = app_with(vec![
            exchange("/pending", None),
            exchange("/fast", Some(5)),
            exchange("/slow", Some(900)),
        ]);
        app.update(key('s'));
        assert_eq!(paths(&app), ["/slow", "/fast", "/pending"]);
        app.update(key('s'));
        assert_eq!(paths(&app), ["/pending", "/fast", "/slow"]);
    }

    #[test]
    fn structured_filter_applies_to_list() {
        let mut app = app_with(vec![exchange("/a", Some(5)), exchange("/b", Some(900))]);
        app.update(key('/'));
        for c in ">500ms".chars() {
            app.update(key(c));
        }
        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(paths(&app), ["/b"]);
        app.update(key('q'));
        assert_eq!(paths(&app).len(), 2);
    }

    #[test]
    fn map_view_toggles_and_enter_filters_to_edge() {
        let mut other = exchange("/b", Some(5));
        other.src_ip = "10.0.0.9".into();
        let mut app = app_with(vec![
            exchange("/a", Some(5)),
            exchange("/a2", Some(6)),
            other,
        ]);

        app.update(key('m'));
        assert_eq!(app.view, ViewMode::Map);
        // Busiest edge (two requests) is first.
        assert_eq!(app.service_edges()[0].requests, 2);

        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.view, ViewMode::List);
        assert_eq!(paths(&app), ["/a", "/a2"]);
    }

    #[test]
    fn map_navigation_is_clamped_and_q_returns_to_list() {
        let mut app = app_with(vec![exchange("/a", Some(5))]);
        app.update(key('m'));
        app.update(key('j'));
        app.update(key('j'));
        assert_eq!(app.map_selected, 0);
        app.update(key('q'));
        assert_eq!(app.view, ViewMode::List);
        assert!(!app.should_quit);
    }

    fn selected_path(app: &App) -> Option<String> {
        app.selected_exchange().map(|ex| ex.request.path.clone())
    }

    #[test]
    fn selection_follows_the_exchange_when_sort_changes() {
        let mut app = app_with(vec![
            exchange("/fast", Some(5)),
            exchange("/slow", Some(900)),
            exchange("/mid", Some(50)),
        ]);
        app.update(key('j'));
        assert_eq!(selected_path(&app).as_deref(), Some("/slow"));

        app.update(key('s'));
        assert_eq!(selected_path(&app).as_deref(), Some("/slow"));
        assert_eq!(app.selected_row(), Some(0));
    }

    #[test]
    fn selection_survives_new_exchanges_arriving() {
        let mut app = app_with(vec![exchange("/a", Some(1)), exchange("/b", Some(1))]);
        app.update(key('j'));
        app.update(Action::NewExchange(Box::new(exchange("/c", Some(1)))));
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
    }

    #[test]
    fn selection_moves_on_when_selected_exchange_is_evicted() {
        let mut app = App::new(2);
        for path in ["/a", "/b", "/c"] {
            app.update(Action::NewExchange(Box::new(exchange(path, Some(1)))));
        }
        // "/a" was selected first and has been evicted.
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
    }

    #[test]
    fn follow_keeps_newest_selected_until_navigating_up() {
        let mut app = app_with(vec![exchange("/a", Some(1))]);
        app.update(key('f'));
        assert!(app.follow);
        app.update(Action::NewExchange(Box::new(exchange("/b", Some(1)))));
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));

        app.update(key('k'));
        assert!(!app.follow);
        app.update(Action::NewExchange(Box::new(exchange("/c", Some(1)))));
        assert_eq!(selected_path(&app).as_deref(), Some("/a"));
    }

    #[test]
    fn capital_g_selects_last_and_enables_follow() {
        let mut app = app_with(vec![exchange("/a", Some(1)), exchange("/b", Some(1))]);
        app.update(key('G'));
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
        assert!(app.follow);
        app.update(key('g'));
        assert_eq!(selected_path(&app).as_deref(), Some("/a"));
        assert!(!app.follow);
    }

    #[test]
    fn filter_keeps_selection_when_still_listed() {
        let mut app = app_with(vec![exchange("/a", Some(1)), exchange("/b", Some(1))]);
        app.update(key('j'));
        app.set_filter("path:/b".into());
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
        app.set_filter("path:/a".into());
        assert_eq!(selected_path(&app).as_deref(), Some("/a"));
    }

    #[test]
    fn help_toggles_and_swallows_other_keys() {
        let mut app = app_with(vec![exchange("/a", Some(1))]);
        app.update(key('?'));
        assert!(app.show_help);
        app.update(key('p'));
        assert!(!app.paused);
        app.update(key('q'));
        assert!(!app.show_help);
        assert!(!app.should_quit);
    }

    #[test]
    fn detail_g_scrolls_instead_of_changing_selection() {
        let mut app = app_with(vec![exchange("/a", Some(1)), exchange("/b", Some(1))]);
        app.update(key('j'));
        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::NONE,
        )));
        app.detail_scroll = 7;
        app.update(key('g'));
        assert_eq!(app.detail_scroll, 0);
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
    }

    fn click(app: &mut App, column: u16, row: u16) {
        app.update(Action::MouseClick { column, row });
    }

    fn app_with_layout(paths: &[&str]) -> App {
        let mut app = app_with(paths.iter().map(|p| exchange(p, Some(5))).collect());
        // Bordered list at rows 1..=10: items start at row 2.
        app.list_area = Rect::new(0, 1, 40, 10);
        app.detail_area = Rect::new(40, 1, 60, 10);
        app
    }

    #[test]
    fn clicking_a_row_selects_it_and_focuses_the_list() {
        let mut app = app_with_layout(&["/a", "/b", "/c"]);
        app.active_pane = Pane::Detail;
        app.follow = true;
        click(&mut app, 5, 4);
        assert_eq!(selected_path(&app).as_deref(), Some("/c"));
        assert_eq!(app.active_pane, Pane::List);
        assert!(!app.follow);
    }

    #[test]
    fn clicking_border_or_empty_space_keeps_selection() {
        let mut app = app_with_layout(&["/a", "/b"]);
        click(&mut app, 5, 3);
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
        // Top border row, and a row below the last exchange.
        click(&mut app, 5, 1);
        click(&mut app, 5, 8);
        assert_eq!(selected_path(&app).as_deref(), Some("/b"));
    }

    #[test]
    fn click_accounts_for_scroll_offset() {
        let mut app = app_with_layout(&["/a", "/b", "/c", "/d"]);
        *app.list_state.offset_mut() = 2;
        click(&mut app, 5, 2);
        assert_eq!(selected_path(&app).as_deref(), Some("/c"));
    }

    #[test]
    fn double_click_moves_focus_to_detail() {
        let mut app = app_with_layout(&["/a", "/b"]);
        click(&mut app, 5, 3);
        assert_eq!(app.active_pane, Pane::List);
        click(&mut app, 5, 3);
        assert_eq!(app.active_pane, Pane::Detail);
    }

    #[test]
    fn clicks_on_different_rows_are_not_a_double_click() {
        let mut app = app_with_layout(&["/a", "/b"]);
        click(&mut app, 5, 2);
        click(&mut app, 5, 3);
        assert_eq!(app.active_pane, Pane::List);
    }

    #[test]
    fn clicking_detail_focuses_it_without_changing_selection() {
        let mut app = app_with_layout(&["/a", "/b"]);
        click(&mut app, 50, 5);
        assert_eq!(app.active_pane, Pane::Detail);
        assert_eq!(selected_path(&app).as_deref(), Some("/a"));
    }

    #[test]
    fn wheel_scrolls_detail_and_moves_list_selection() {
        let mut app = app_with_layout(&["/a", "/b", "/c", "/d"]);
        app.update(Action::MouseScroll {
            column: 50,
            row: 5,
            delta: 3,
        });
        assert_eq!(app.detail_scroll, 3);
        app.update(Action::MouseScroll {
            column: 5,
            row: 5,
            delta: 3,
        });
        assert_eq!(selected_path(&app).as_deref(), Some("/d"));
        app.update(Action::MouseScroll {
            column: 5,
            row: 5,
            delta: -1,
        });
        assert_eq!(selected_path(&app).as_deref(), Some("/c"));
    }

    #[test]
    fn map_click_selects_edge_and_double_click_drills_in() {
        let mut other = exchange("/other", Some(5));
        other.src_ip = "10.0.0.9".into();
        let mut app = app_with(vec![
            exchange("/a", Some(5)),
            exchange("/a2", Some(5)),
            other,
        ]);
        app.update(key('m'));
        // Header row sits under the top border: edges start at row 3.
        app.map_area = Rect::new(0, 1, 100, 10);

        click(&mut app, 5, 4);
        assert_eq!(app.view, ViewMode::Map);
        assert_eq!(app.map_selected, 1);

        click(&mut app, 5, 4);
        assert_eq!(app.view, ViewMode::List);
        assert_eq!(paths(&app), ["/other"]);
    }

    #[test]
    fn map_wheel_moves_selection() {
        let mut other = exchange("/other", Some(5));
        other.src_ip = "10.0.0.9".into();
        let mut app = app_with(vec![exchange("/a", Some(5)), other]);
        app.update(key('m'));
        app.map_area = Rect::new(0, 1, 100, 10);
        app.update(Action::MouseScroll {
            column: 5,
            row: 5,
            delta: 3,
        });
        assert_eq!(app.map_selected, 1);
    }

    #[test]
    fn help_closes_on_outside_click_and_scrolls_with_wheel() {
        let mut app = app_with_layout(&["/a"]);
        app.update(key('?'));
        app.help_area = Rect::new(10, 5, 50, 20);

        click(&mut app, 20, 10);
        assert!(app.show_help);

        app.update(Action::MouseScroll {
            column: 20,
            row: 10,
            delta: 3,
        });
        assert_eq!(app.help_scroll, 3);

        click(&mut app, 1, 1);
        assert!(!app.show_help);
    }

    fn drag(app: &mut App, column: u16) {
        app.update(Action::MouseDrag { column, row: 5 });
    }

    /// Panes laid out as the renderer would for a 100 column screen: borders at 39 and 40.
    fn app_with_split() -> App {
        let mut app = app_with_layout(&["/a", "/b"]);
        app.list_area = Rect::new(0, 1, 40, 10);
        app.detail_area = Rect::new(40, 1, 60, 10);
        app
    }

    #[test]
    fn default_list_width_is_forty_percent() {
        let app = App::new(10);
        assert_eq!(app.list_pane_width(100), 40);
    }

    #[test]
    fn dragging_the_border_resizes_the_list() {
        let mut app = app_with_split();
        click(&mut app, 39, 5);
        drag(&mut app, 59);
        assert_eq!(app.list_width, Some(60));
        app.update(Action::MouseRelease);
        drag(&mut app, 30);
        assert_eq!(app.list_width, Some(60), "no resize after release");
    }

    #[test]
    fn dragging_from_the_detail_side_of_the_border_also_works() {
        let mut app = app_with_split();
        click(&mut app, 40, 5);
        drag(&mut app, 50);
        assert_eq!(app.list_width, Some(51));
    }

    #[test]
    fn drag_is_clamped_so_both_panes_stay_usable() {
        let mut app = app_with_split();
        click(&mut app, 39, 5);
        drag(&mut app, 2);
        assert_eq!(app.list_width, Some(MIN_PANE_WIDTH));
        drag(&mut app, 99);
        assert_eq!(app.list_width, Some(100 - MIN_PANE_WIDTH));
    }

    #[test]
    fn clicking_elsewhere_does_not_start_a_drag() {
        let mut app = app_with_split();
        click(&mut app, 10, 5);
        drag(&mut app, 70);
        assert_eq!(app.list_width, None);
    }

    #[test]
    fn double_click_on_the_border_resets_the_split() {
        let mut app = app_with_split();
        app.list_width = Some(70);
        click(&mut app, 39, 5);
        click(&mut app, 39, 5);
        assert_eq!(app.list_width, None);
    }

    #[test]
    fn resize_keys_step_and_reset() {
        let mut app = app_with_split();
        app.update(key('>'));
        assert_eq!(app.list_width, Some(40 + RESIZE_STEP));
        app.list_area = Rect::new(0, 1, 44, 10);
        app.detail_area = Rect::new(44, 1, 56, 10);
        app.update(key('<'));
        assert_eq!(app.list_width, Some(40));
        app.update(key('='));
        assert_eq!(app.list_width, None);
    }

    #[test]
    fn zoom_toggles_and_escape_leaves_it_before_anything_else() {
        let mut app = app_with_split();
        app.set_filter("path:/a".into());
        app.update(key('z'));
        assert!(app.zoom);

        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        assert!(!app.zoom);
        assert_eq!(app.filter_text, "path:/a", "filter untouched");
        assert!(!app.should_quit);
    }

    #[test]
    fn zoom_follows_tab_and_ignores_the_divider() {
        let mut app = app_with_split();
        app.update(key('z'));
        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.active_pane, Pane::Detail);
        assert!(app.zoom);

        app.list_area = Rect::new(0, 1, 100, 10);
        app.detail_area = Rect::default();
        click(&mut app, 39, 5);
        drag(&mut app, 70);
        assert_eq!(app.list_width, None);
    }

    #[test]
    fn zoom_is_ignored_in_the_service_map() {
        let mut app = app_with_split();
        app.update(key('m'));
        app.update(key('z'));
        assert!(!app.zoom);
    }

    /// App with a rendered detail pane: text area at columns 40.., rows 3..=12.
    fn app_with_detail(lines: &[&str]) -> App {
        let mut app = app_with_layout(&["/a"]);
        app.clipboard = None;
        let id = app.selected_id.expect("selected");
        app.detail_cache = Some(DetailCache {
            id,
            width: 58,
            rows: lines
                .iter()
                .map(|l| Row::new(ratatui::text::Line::raw(l.to_string())))
                .collect(),
        });
        app.detail_area = Rect::new(40, 1, 60, 12);
        app.detail_text_area = Rect::new(41, 3, 58, 10);
        app
    }

    fn drag_to(app: &mut App, column: u16, row: u16) {
        app.update(Action::MouseDrag { column, row });
    }

    fn release(app: &mut App) {
        app.update(Action::MouseRelease);
    }

    fn selection_text(app: &App) -> String {
        let selection = app.selection.as_ref().expect("selection");
        selected_text(&app.detail_cache.as_ref().expect("cache").rows, selection)
    }

    #[test]
    fn dragging_in_the_detail_pane_selects_text() {
        let mut app = app_with_detail(&["hello world", "second line"]);
        click(&mut app, 41, 3);
        drag_to(&mut app, 45, 3);
        assert_eq!(selection_text(&app), "hello");
        release(&mut app);
        assert!(app.selection.is_some(), "selection stays highlighted");
        assert_eq!(app.flash_message.as_deref(), Some("Clipboard unavailable"));
    }

    #[test]
    fn dragging_across_lines_selects_multiple_lines() {
        let mut app = app_with_detail(&["hello world", "second line"]);
        click(&mut app, 47, 3);
        drag_to(&mut app, 46, 4);
        assert_eq!(selection_text(&app), "world\nsecond");
    }

    #[test]
    fn plain_click_selects_nothing() {
        let mut app = app_with_detail(&["hello world"]);
        click(&mut app, 43, 3);
        release(&mut app);
        assert!(app.selection.is_none());
        assert_eq!(app.flash_message, None);
    }

    #[test]
    fn selection_accounts_for_scroll() {
        let mut app = app_with_detail(&["line zero", "line one", "line two"]);
        app.detail_scroll = 1;
        click(&mut app, 41, 3);
        drag_to(&mut app, 44, 3);
        assert_eq!(selection_text(&app), "line");
        assert_eq!(app.selection.as_ref().map(|s| s.anchor.line), Some(1));
    }

    #[test]
    fn dragging_past_the_bottom_scrolls() {
        let lines: Vec<String> = (0..30).map(|i| format!("row {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut app = app_with_detail(&refs);
        click(&mut app, 41, 3);
        drag_to(&mut app, 45, 20);
        assert_eq!(app.detail_scroll, 1);
    }

    #[test]
    fn double_click_selects_the_word_under_the_cursor() {
        let mut app = app_with_detail(&["GET /api/users?id=7 HTTP"]);
        click(&mut app, 46, 3);
        release(&mut app);
        click(&mut app, 46, 3);
        assert_eq!(selection_text(&app), "/api/users");
    }

    #[test]
    fn escape_clears_the_selection_before_anything_else() {
        let mut app = app_with_detail(&["hello world"]);
        app.zoom = true;
        click(&mut app, 41, 3);
        drag_to(&mut app, 45, 3);
        release(&mut app);
        app.update(Action::KeyPress(crossterm::event::KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        assert!(app.selection.is_none());
        assert!(app.zoom, "zoom is left for the next Esc");
    }

    #[test]
    fn clicking_elsewhere_clears_the_selection() {
        let mut app = app_with_detail(&["hello world"]);
        click(&mut app, 41, 3);
        drag_to(&mut app, 45, 3);
        release(&mut app);
        click(&mut app, 5, 4);
        assert!(app.selection.is_none());
    }

    #[test]
    fn clicks_outside_the_text_area_do_not_start_a_selection() {
        let mut app = app_with_detail(&["hello world"]);
        // The pinned summary sits above the scrollable text.
        click(&mut app, 50, 2);
        drag_to(&mut app, 55, 2);
        assert!(app.selection.is_none());
        assert_eq!(app.active_pane, Pane::Detail);
    }

    fn capture_error(interface: &str, message: &str) -> Action {
        Action::CaptureError {
            interface: interface.into(),
            message: message.into(),
        }
    }

    #[test]
    fn error_banner_appears_and_clears_when_the_capture_starts() {
        let mut app = App::new(10);
        assert_eq!(app.error_banner(), None);

        app.update(capture_error("br-1", "Capture failed: no such device"));
        assert_eq!(
            app.error_banner().as_deref(),
            Some("Capture failed: no such device")
        );

        app.update(Action::CaptureStarted("br-1".into()));
        assert_eq!(app.error_banner(), None);
    }

    #[test]
    fn one_healthy_capture_does_not_hide_another_one_failing() {
        let mut app = App::new(10);
        app.update(capture_error("br-1", "first"));
        app.update(capture_error("br-2", "second"));
        app.update(Action::CaptureStarted("br-1".into()));
        assert_eq!(app.error_banner().as_deref(), Some("second"));
    }

    #[test]
    fn identical_failures_are_shown_once() {
        let mut app = App::new(10);
        app.update(capture_error("br-1", "permission denied"));
        app.update(capture_error("br-2", "permission denied"));
        assert_eq!(app.error_banner().as_deref(), Some("permission denied"));
    }

    #[test]
    fn error_for_a_network_that_went_away_is_dropped() {
        let mut app = App::new(10);
        app.update(capture_error("br-old", "Capture failed"));
        app.update(Action::Networks(vec![("net".into(), "br-new".into())]));
        assert_eq!(app.error_banner(), None);
    }

    #[test]
    fn error_for_a_still_tracked_network_survives_a_network_update() {
        let mut app = App::new(10);
        app.update(capture_error("br-1", "Capture failed"));
        app.update(Action::Networks(vec![("net".into(), "br-1".into())]));
        assert_eq!(app.error_banner().as_deref(), Some("Capture failed"));
    }
}
