use std::collections::HashMap;
use std::io;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;

use crate::http::types::HttpExchange;

/// Map of IP address to container name used to label exchanges.
pub type Names = HashMap<String, String>;

#[derive(Serialize)]
struct Header {
    name: String,
    value: String,
}

/// Body text, base64-encoded when the bytes are not valid UTF-8.
#[derive(Serialize)]
struct Body {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<&'static str>,
    size: usize,
}

impl Body {
    fn new(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Body {
                text: text.to_string(),
                encoding: None,
                size: bytes.len(),
            },
            Err(_) => Body {
                text: BASE64.encode(bytes),
                encoding: Some("base64"),
                size: bytes.len(),
            },
        }
    }
}

fn headers(pairs: &[(String, String)]) -> Vec<Header> {
    pairs
        .iter()
        .map(|(name, value)| Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect()
}

fn header_value<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn label(names: &Names, ip: &str) -> String {
    names.get(ip).cloned().unwrap_or_else(|| ip.to_string())
}

/// Current time in microseconds since the Unix epoch.
pub fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as u64)
}

/// Format microseconds since the Unix epoch as an ISO 8601 UTC timestamp.
pub fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let millis = (micros % 1_000_000) / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);

    // Days since 1970-01-01 to a proleptic Gregorian date.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[derive(Serialize)]
struct JsonlRecord {
    timestamp: Option<String>,
    method: String,
    path: String,
    http_version: String,
    status: Option<u16>,
    duration_ms: Option<u64>,
    src: String,
    dst: String,
    src_ip: String,
    dst_ip: String,
    src_port: u16,
    dst_port: u16,
    request_headers: Vec<Header>,
    request_body: Option<Body>,
    response_headers: Vec<Header>,
    response_body: Option<Body>,
}

/// Serialise an exchange as a single-line JSON object.
pub fn jsonl_line(ex: &HttpExchange, names: &Names) -> serde_json::Result<String> {
    let response = ex.response.as_ref();
    serde_json::to_string(&JsonlRecord {
        timestamp: ex.timestamp_us.map(iso8601),
        method: ex.request.method.clone(),
        path: ex.request.path.clone(),
        http_version: ex.request.version.clone(),
        status: response.map(|r| r.status_code),
        duration_ms: ex.duration_ms,
        src: label(names, &ex.src_ip),
        dst: label(names, &ex.dst_ip),
        src_ip: ex.src_ip.clone(),
        dst_ip: ex.dst_ip.clone(),
        src_port: ex.src_port,
        dst_port: ex.dst_port,
        request_headers: headers(&ex.request.headers),
        request_body: ex.request.body.as_deref().map(Body::new),
        response_headers: response.map(|r| headers(&r.headers)).unwrap_or_default(),
        response_body: response.and_then(|r| r.body.as_deref()).map(Body::new),
    })
}

#[derive(Serialize)]
struct Har {
    log: HarLog,
}

#[derive(Serialize)]
struct HarLog {
    version: &'static str,
    creator: HarCreator,
    entries: Vec<HarEntry>,
}

#[derive(Serialize)]
struct HarCreator {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarEntry {
    started_date_time: String,
    time: i64,
    request: HarRequest,
    response: HarResponse,
    cache: serde_json::Value,
    timings: HarTimings,
    #[serde(rename = "serverIPAddress")]
    server_ip_address: String,
    connection: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarRequest {
    method: String,
    url: String,
    http_version: String,
    cookies: Vec<Header>,
    headers: Vec<Header>,
    query_string: Vec<Header>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_data: Option<HarPostData>,
    headers_size: i64,
    body_size: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarPostData {
    mime_type: String,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarResponse {
    status: u16,
    status_text: String,
    http_version: String,
    cookies: Vec<Header>,
    headers: Vec<Header>,
    content: HarContent,
    #[serde(rename = "redirectURL")]
    redirect_url: String,
    headers_size: i64,
    body_size: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarContent {
    size: usize,
    mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<&'static str>,
}

#[derive(Serialize)]
struct HarTimings {
    send: i64,
    wait: i64,
    receive: i64,
}

fn query_string(path: &str) -> Vec<Header> {
    let Some((_, query)) = path.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Header {
                name: name.to_string(),
                value: value.to_string(),
            }
        })
        .collect()
}

/// Absolute URL of the request, using the `Host` header and falling back to the destination
/// address.
fn request_url(ex: &HttpExchange) -> String {
    let host = header_value(&ex.request.headers, "host")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}:{}", ex.dst_ip, ex.dst_port));
    format!("http://{host}{}", ex.request.path)
}

/// Headers curl derives itself from the URL, body or connection.
const CURL_SKIPPED_HEADERS: [&str; 4] =
    ["host", "content-length", "connection", "transfer-encoding"];

/// Quote a string for POSIX shells.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Quote arbitrary bytes for bash using ANSI-C `$'...'` escapes.
fn shell_quote_bytes(bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\x{b:02x}")).collect();
    format!("$'{escaped}'")
}

/// Build a `curl` command that repeats the request.
pub fn curl_command(ex: &HttpExchange) -> String {
    let mut parts = vec!["curl".to_string()];
    match ex.request.method.as_str() {
        "GET" => {}
        "HEAD" => parts.push("--head".to_string()),
        method => parts.push(format!("-X {}", shell_quote(method))),
    }
    parts.push(shell_quote(&request_url(ex)));

    for (name, value) in &ex.request.headers {
        if CURL_SKIPPED_HEADERS
            .iter()
            .any(|skipped| name.eq_ignore_ascii_case(skipped))
        {
            continue;
        }
        parts.push(format!("-H {}", shell_quote(&format!("{name}: {value}"))));
    }

    if let Some(body) = ex.request.body.as_deref().filter(|b| !b.is_empty()) {
        let quoted = match std::str::from_utf8(body) {
            Ok(text) => shell_quote(text),
            Err(_) => shell_quote_bytes(body),
        };
        parts.push(format!("--data-raw {quoted}"));
    }

    parts.join(" \\\n  ")
}

fn har_entry(ex: &HttpExchange) -> HarEntry {
    let request_headers = &ex.request.headers;
    let wait = ex.duration_ms.map_or(-1, |d| d as i64);
    let request_body = ex.request.body.as_deref();

    let response = match &ex.response {
        Some(r) => {
            let content = r.body.as_deref().map(Body::new);
            let mime_type = header_value(&r.headers, "content-type")
                .unwrap_or("x-unknown")
                .to_string();
            HarResponse {
                status: r.status_code,
                status_text: r.reason.clone(),
                http_version: r.version.clone(),
                cookies: Vec::new(),
                headers: headers(&r.headers),
                content: HarContent {
                    size: content.as_ref().map_or(0, |b| b.size),
                    mime_type,
                    encoding: content.as_ref().and_then(|b| b.encoding),
                    text: content.map(|b| b.text),
                },
                redirect_url: header_value(&r.headers, "location")
                    .unwrap_or_default()
                    .to_string(),
                headers_size: -1,
                body_size: r.body.as_ref().map_or(0, |b| b.len() as i64),
            }
        }
        // HAR has no notion of a missing response; status 0 is the convention for it.
        None => HarResponse {
            status: 0,
            status_text: String::new(),
            http_version: ex.request.version.clone(),
            cookies: Vec::new(),
            headers: Vec::new(),
            content: HarContent {
                size: 0,
                mime_type: "x-unknown".to_string(),
                text: None,
                encoding: None,
            },
            redirect_url: String::new(),
            headers_size: -1,
            body_size: -1,
        },
    };

    HarEntry {
        started_date_time: iso8601(ex.timestamp_us.unwrap_or(0)),
        time: wait,
        request: HarRequest {
            method: ex.request.method.clone(),
            url: request_url(ex),
            http_version: ex.request.version.clone(),
            cookies: Vec::new(),
            headers: headers(request_headers),
            query_string: query_string(&ex.request.path),
            post_data: request_body.map(|body| HarPostData {
                mime_type: header_value(request_headers, "content-type")
                    .unwrap_or_default()
                    .to_string(),
                text: String::from_utf8_lossy(body).into_owned(),
            }),
            headers_size: -1,
            body_size: request_body.map_or(0, |b| b.len() as i64),
        },
        response,
        cache: serde_json::json!({}),
        timings: HarTimings {
            send: 0,
            wait,
            receive: 0,
        },
        server_ip_address: ex.dst_ip.clone(),
        connection: ex.src_port.to_string(),
    }
}

/// Render exchanges as a HAR 1.2 document.
pub fn har_document<'a>(
    exchanges: impl IntoIterator<Item = &'a HttpExchange>,
) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&Har {
        log: HarLog {
            version: "1.2",
            creator: HarCreator {
                name: env!("CARGO_PKG_NAME"),
                version: env!("CARGO_PKG_VERSION"),
            },
            entries: exchanges.into_iter().map(har_entry).collect(),
        },
    })
}

/// Write exchanges to `path` as a HAR file.
pub fn write_har<'a>(
    path: &Path,
    exchanges: impl IntoIterator<Item = &'a HttpExchange>,
) -> io::Result<()> {
    let document = har_document(exchanges).map_err(io::Error::other)?;
    std::fs::write(path, document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpRequest, HttpResponse};

    fn exchange() -> HttpExchange {
        HttpExchange {
            request: HttpRequest {
                method: "POST".into(),
                path: "/echo?a=1&b".into(),
                version: "HTTP/1.1".into(),
                headers: vec![
                    ("Host".into(), "gateway".into()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body: Some(br#"{"x":1}"#.to_vec()),
            },
            response: Some(HttpResponse {
                status_code: 201,
                reason: "Created".into(),
                version: "HTTP/1.1".into(),
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: Some(b"ok".to_vec()),
            }),
            src_ip: "10.0.0.2".into(),
            dst_ip: "10.0.0.3".into(),
            src_port: 40000,
            dst_port: 80,
            id: 0,
            timestamp_us: Some(1_700_000_000_123_456),
            duration_ms: Some(42),
        }
    }

    fn names() -> Names {
        Names::from([("10.0.0.2".into(), "poller".into())])
    }

    #[test]
    fn iso8601_formats_epoch_and_known_instant() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(1_700_000_000_123_456), "2023-11-14T22:13:20.123Z");
    }

    #[test]
    fn iso8601_handles_leap_day() {
        // 2024-02-29T12:00:00Z
        assert_eq!(iso8601(1_709_208_000_000_000), "2024-02-29T12:00:00.000Z");
    }

    #[test]
    fn jsonl_line_is_single_line_with_names() {
        let line = jsonl_line(&exchange(), &names()).expect("serialise");
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(v["method"], "POST");
        assert_eq!(v["status"], 201);
        assert_eq!(v["duration_ms"], 42);
        assert_eq!(v["src"], "poller");
        assert_eq!(v["dst"], "10.0.0.3");
        assert_eq!(v["timestamp"], "2023-11-14T22:13:20.123Z");
        assert_eq!(v["request_body"]["text"], r#"{"x":1}"#);
        assert_eq!(v["response_headers"][0]["name"], "Content-Type");
    }

    #[test]
    fn jsonl_unanswered_has_null_status() {
        let mut ex = exchange();
        ex.response = None;
        ex.duration_ms = None;
        let v: serde_json::Value =
            serde_json::from_str(&jsonl_line(&ex, &Names::new()).expect("serialise"))
                .expect("valid json");
        assert!(v["status"].is_null());
        assert!(v["response_body"].is_null());
        assert_eq!(v["response_headers"], serde_json::json!([]));
    }

    #[test]
    fn binary_body_is_base64_encoded() {
        let mut ex = exchange();
        ex.response.as_mut().expect("response").body = Some(vec![0xff, 0xfe, 0x00]);
        let v: serde_json::Value =
            serde_json::from_str(&jsonl_line(&ex, &Names::new()).expect("serialise"))
                .expect("valid json");
        assert_eq!(v["response_body"]["encoding"], "base64");
        assert_eq!(v["response_body"]["text"], "//4A");
    }

    #[test]
    fn har_document_matches_spec_shape() {
        let ex = exchange();
        let doc = har_document([&ex]).expect("serialise");
        let v: serde_json::Value = serde_json::from_str(&doc).expect("valid json");
        let log = &v["log"];
        assert_eq!(log["version"], "1.2");
        assert_eq!(log["creator"]["name"], "tylomon");

        let entry = &log["entries"][0];
        assert_eq!(entry["startedDateTime"], "2023-11-14T22:13:20.123Z");
        assert_eq!(entry["time"], 42);
        assert_eq!(entry["serverIPAddress"], "10.0.0.3");
        assert_eq!(entry["request"]["url"], "http://gateway/echo?a=1&b");
        assert_eq!(entry["request"]["queryString"][0]["name"], "a");
        assert_eq!(entry["request"]["queryString"][1]["value"], "");
        assert_eq!(entry["request"]["postData"]["mimeType"], "application/json");
        assert_eq!(entry["request"]["bodySize"], 7);
        assert_eq!(entry["response"]["status"], 201);
        assert_eq!(entry["response"]["statusText"], "Created");
        assert_eq!(entry["response"]["content"]["text"], "ok");
        assert_eq!(entry["response"]["content"]["mimeType"], "text/plain");
        assert_eq!(entry["timings"]["wait"], 42);
    }

    #[test]
    fn har_unanswered_uses_status_zero_and_unknown_timing() {
        let mut ex = exchange();
        ex.response = None;
        ex.duration_ms = None;
        let v: serde_json::Value =
            serde_json::from_str(&har_document([&ex]).expect("serialise")).expect("valid json");
        let entry = &v["log"]["entries"][0];
        assert_eq!(entry["response"]["status"], 0);
        assert_eq!(entry["time"], -1);
        assert_eq!(entry["timings"]["wait"], -1);
    }

    #[test]
    fn curl_command_repeats_request() {
        let cmd = curl_command(&exchange());
        assert_eq!(
            cmd,
            "curl \\\n  -X 'POST' \\\n  'http://gateway/echo?a=1&b' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"x\":1}'"
        );
    }

    #[test]
    fn curl_get_has_no_method_flag_and_skips_derived_headers() {
        let mut ex = exchange();
        ex.request.method = "GET".into();
        ex.request.body = None;
        ex.request
            .headers
            .push(("Content-Length".into(), "7".into()));
        ex.request
            .headers
            .push(("Connection".into(), "keep-alive".into()));
        let cmd = curl_command(&ex);
        assert!(!cmd.contains("-X"));
        assert!(!cmd.contains("Content-Length"));
        assert!(!cmd.contains("Connection"));
        assert!(!cmd.contains("Host:"));
        assert!(cmd.contains("-H 'Content-Type: application/json'"));
    }

    #[test]
    fn curl_head_uses_head_flag() {
        let mut ex = exchange();
        ex.request.method = "HEAD".into();
        ex.request.body = None;
        assert!(curl_command(&ex).contains("--head"));
    }

    #[test]
    fn curl_escapes_single_quotes() {
        let mut ex = exchange();
        ex.request.body = Some(b"it's".to_vec());
        assert!(curl_command(&ex).contains("--data-raw 'it'\\''s'"));
    }

    #[test]
    fn curl_binary_body_uses_ansi_c_quoting() {
        let mut ex = exchange();
        ex.request.body = Some(vec![0xff, 0x00]);
        assert!(curl_command(&ex).contains("--data-raw $'\\xff\\x00'"));
    }

    #[test]
    fn har_url_falls_back_to_destination_address() {
        let mut ex = exchange();
        ex.request.headers.clear();
        ex.request.path = "/x".into();
        let v: serde_json::Value =
            serde_json::from_str(&har_document([&ex]).expect("serialise")).expect("valid json");
        assert_eq!(
            v["log"]["entries"][0]["request"]["url"],
            "http://10.0.0.3:80/x"
        );
    }
}
