use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A complete HTTP exchange (request + response pair)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchange {
    pub request: HttpRequest,
    pub response: Option<HttpResponse>,
    pub src_ip: String,
    pub dst_ip: String,
    pub src_port: u16,
    pub dst_port: u16,
    #[serde(skip)]
    pub timestamp: Option<Instant>,
    /// Duration from request to response
    pub duration_ms: Option<u64>,
}

/// Parsed HTTP request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// Parsed HTTP response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status_code: u16,
    pub reason: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpExchange {
    /// Get a display-friendly summary line
    pub fn summary(&self) -> String {
        let method = &self.request.method;
        let path = &self.request.path;
        let status = self
            .response
            .as_ref()
            .map(|r| format!("{} {}", r.status_code, r.reason))
            .unwrap_or_else(|| "pending...".to_string());
        let duration = self
            .duration_ms
            .map(|d| format!("{d}ms"))
            .unwrap_or_default();

        format!("{method} {path} -> {status} {duration}")
    }
}
