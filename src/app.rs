use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;

use crate::http::types::HttpExchange;

#[derive(Debug, Clone)]
pub enum Action {
    NewExchange(Box<HttpExchange>),
    ContainerUpdate { ip: String, name: String },
    CaptureError(String),
    KeyPress(crossterm::event::KeyEvent),
    MouseScroll { column: u16, row: u16, delta: i16 },
    MouseClick { column: u16, row: u16 },
    Resize(u16, u16),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pane {
    List,
    Detail,
}

pub struct App {
    pub exchanges: Vec<HttpExchange>,
    pub max_exchanges: usize,
    pub selected_index: Option<usize>,
    pub should_quit: bool,
    pub container_names: std::collections::HashMap<String, String>,
    pub detail_scroll: u16,
    pub network_label: String,
    pub bridge_label: String,
    pub active_pane: Pane,
    pub filter_text: String,
    pub filter_input: String,
    pub filter_active: bool,
    pub error_message: Option<String>,
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
            selected_index: None,
            should_quit: false,
            container_names: std::collections::HashMap::new(),
            detail_scroll: 0,
            network_label: String::new(),
            bridge_label: String::new(),
            active_pane: Pane::List,
            filter_text: String::new(),
            filter_input: String::new(),
            filter_active: false,
            error_message: None,
            paused: false,
            detail_area: Rect::default(),
            list_area: Rect::default(),
            flash_message: None,
            flash_expiry: None,
            clipboard: arboard::Clipboard::new().ok(),
        }
    }

    pub fn update(&mut self, action: Action) {
        match action {
            Action::NewExchange(exchange) => {
                if self.paused {
                    return;
                }
                self.exchanges.push(*exchange);
                if self.exchanges.len() > self.max_exchanges {
                    self.exchanges.remove(0);
                    if let Some(idx) = self.selected_index {
                        self.selected_index = idx.checked_sub(1);
                    }
                }
                if self.selected_index.is_none() && !self.exchanges.is_empty() {
                    self.selected_index = Some(0);
                }
            }
            Action::ContainerUpdate { ip, name } => {
                self.container_names.insert(ip, name);
            }
            Action::CaptureError(msg) => {
                self.error_message = Some(msg);
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

    pub fn filtered_exchanges(&self) -> Vec<(usize, &HttpExchange)> {
        if self.filter_text.is_empty() {
            return self.exchanges.iter().enumerate().collect();
        }
        let filter_lower = self.filter_text.to_lowercase();
        self.exchanges
            .iter()
            .enumerate()
            .filter(|(_, ex)| {
                let method = ex.request.method.to_lowercase();
                let path = ex.request.path.to_lowercase();
                let status = ex
                    .response
                    .as_ref()
                    .map(|r| r.status_code.to_string())
                    .unwrap_or_default();
                method.contains(&filter_lower)
                    || path.contains(&filter_lower)
                    || status.contains(&filter_lower)
            })
            .collect()
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.filter_active {
            self.handle_filter_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if !self.filter_text.is_empty() {
                    self.filter_text.clear();
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
            KeyCode::Home | KeyCode::Char('g') => {
                if !self.exchanges.is_empty() {
                    self.selected_index = Some(0);
                    self.detail_scroll = 0;
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.exchanges.is_empty() {
                    self.selected_index = Some(self.exchanges.len() - 1);
                    self.detail_scroll = 0;
                }
            }
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
                self.selected_index = None;
                self.detail_scroll = 0;
                self.filter_text.clear();
            }
            KeyCode::Char('p') => {
                self.paused = !self.paused;
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
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.filter_text = self.filter_input.clone();
                self.filter_active = false;
                self.selected_index = if self.filtered_exchanges().is_empty() {
                    None
                } else {
                    Some(0)
                };
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
        let filtered = self.filtered_exchanges();
        if filtered.is_empty() {
            return;
        }
        let max = filtered.len() - 1;
        self.selected_index = Some(match self.selected_index {
            Some(i) if i < max => i + 1,
            Some(i) => i,
            None => 0,
        });
        self.detail_scroll = 0;
    }

    fn select_previous(&mut self) {
        let filtered = self.filtered_exchanges();
        if filtered.is_empty() {
            return;
        }
        self.selected_index = Some(match self.selected_index {
            Some(i) if i > 0 => i - 1,
            Some(i) => i,
            None => 0,
        });
        self.detail_scroll = 0;
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16) {
        let pos = ratatui::layout::Position { x: column, y: row };
        if self.list_area.contains(pos) {
            self.active_pane = Pane::List;
        } else if self.detail_area.contains(pos) {
            self.active_pane = Pane::Detail;
        }
    }

    fn handle_mouse_scroll(&mut self, column: u16, row: u16, delta: i16) {
        if self
            .detail_area
            .contains(ratatui::layout::Position { x: column, y: row })
        {
            if delta > 0 {
                self.detail_scroll = self.detail_scroll.saturating_add(delta as u16);
            } else {
                self.detail_scroll = self.detail_scroll.saturating_sub((-delta) as u16);
            }
        } else if self
            .list_area
            .contains(ratatui::layout::Position { x: column, y: row })
        {
            if delta > 0 {
                for _ in 0..delta {
                    self.select_next();
                }
            } else {
                for _ in 0..(-delta) {
                    self.select_previous();
                }
            }
        }
    }

    fn copy_to_clipboard(&mut self, target: CopyTarget) {
        let filtered = self.filtered_exchanges();
        let selected = self
            .selected_index
            .and_then(|i| filtered.get(i).map(|(_, ex)| (*ex).clone()));

        let Some(exchange) = selected else {
            return;
        };

        let (text, label) = match target {
            CopyTarget::Full => (self.format_exchange_text(&exchange), "exchange"),
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

        match self.clipboard.as_mut() {
            Some(cb) => match cb.set_text(text) {
                Ok(()) => {
                    self.flash_message = Some(format!("Copied {label} to clipboard"));
                    self.flash_expiry = Some(Instant::now() + std::time::Duration::from_secs(2));
                }
                Err(e) => {
                    self.flash_message = Some(format!("Clipboard error: {e}"));
                    self.flash_expiry = Some(Instant::now() + std::time::Duration::from_secs(3));
                }
            },
            None => {
                self.flash_message = Some("Clipboard unavailable".to_string());
                self.flash_expiry = Some(Instant::now() + std::time::Duration::from_secs(3));
            }
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
