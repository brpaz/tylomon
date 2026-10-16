use serde::{Deserialize, Serialize};

/// Wire protocol an exchange was decoded from.
///
/// Non-HTTP protocols reuse the HTTP structures: the method is the verb, the path the
/// operation, and the status code is 200 on success and 500 on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Http,
    Postgres,
    Grpc,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Postgres => "postgres",
            Protocol::Grpc => "grpc",
        }
    }
}

/// A complete request/response exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchange {
    /// Unique per stored exchange, assigned by the app so selection survives reordering.
    #[serde(skip)]
    pub id: u64,
    #[serde(default)]
    pub protocol: Protocol,
    pub request: HttpRequest,
    pub response: Option<HttpResponse>,
    pub src_ip: String,
    pub dst_ip: String,
    pub src_port: u16,
    pub dst_port: u16,
    /// Request capture time in microseconds since the Unix epoch
    pub timestamp_us: Option<u64>,
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
