use crate::http::types::{HttpRequest, HttpResponse};

/// Attempt to parse an HTTP request from a byte buffer.
/// Returns the parsed request and the number of bytes consumed (headers only),
/// or None if the data is incomplete or not HTTP.
pub fn parse_request(data: &[u8]) -> Option<(HttpRequest, usize)> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);

    match req.parse(data) {
        Ok(httparse::Status::Complete(body_offset)) => {
            let method = req.method?.to_string();
            let path = req.path?.to_string();
            let version = match req.version? {
                0 => "HTTP/1.0".to_string(),
                1 => "HTTP/1.1".to_string(),
                v => format!("HTTP/1.{v}"),
            };

            let headers: Vec<(String, String)> = req
                .headers
                .iter()
                .map(|h| {
                    (
                        h.name.to_string(),
                        String::from_utf8_lossy(h.value).to_string(),
                    )
                })
                .collect();

            Some((
                HttpRequest {
                    method,
                    path,
                    version,
                    headers,
                    body: None, // Body parsing handled separately based on Content-Length/chunked
                },
                body_offset,
            ))
        }
        Ok(httparse::Status::Partial) => None, // Need more data
        Err(_) => None,                        // Not HTTP
    }
}

/// Attempt to parse an HTTP response from a byte buffer.
/// Returns the parsed response and the number of bytes consumed (headers only),
/// or None if the data is incomplete or not HTTP.
pub fn parse_response(data: &[u8]) -> Option<(HttpResponse, usize)> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut resp = httparse::Response::new(&mut headers);

    match resp.parse(data) {
        Ok(httparse::Status::Complete(body_offset)) => {
            let status_code = resp.code?;
            let reason = resp.reason?.to_string();
            let version = match resp.version? {
                0 => "HTTP/1.0".to_string(),
                1 => "HTTP/1.1".to_string(),
                v => format!("HTTP/1.{v}"),
            };

            let headers: Vec<(String, String)> = resp
                .headers
                .iter()
                .map(|h| {
                    (
                        h.name.to_string(),
                        String::from_utf8_lossy(h.value).to_string(),
                    )
                })
                .collect();

            Some((
                HttpResponse {
                    status_code,
                    reason,
                    version,
                    headers,
                    body: None,
                },
                body_offset,
            ))
        }
        Ok(httparse::Status::Partial) => None,
        Err(_) => None,
    }
}

/// Extract the Content-Length value from a list of headers
pub fn get_content_length(headers: &[(String, String)]) -> Option<usize> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse().ok())
}

/// Check if the transfer encoding is chunked
pub fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.to_lowercase().contains("chunked")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get_request() {
        let data =
            b"GET /api/users HTTP/1.1\r\nHost: service-b:8080\r\nAccept: application/json\r\n\r\n";

        let (req, offset) = parse_request(data).expect("should parse");
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/api/users");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(req.headers.len(), 2);
        assert_eq!(offset, data.len());
    }

    #[test]
    fn test_parse_simple_response() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"status\":\"ok\"}";

        let (resp, offset) = parse_response(data).expect("should parse");
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.reason, "OK");
        assert_eq!(resp.headers.len(), 2);
        assert!(offset < data.len()); // Body follows
    }

    #[test]
    fn test_parse_incomplete_request() {
        let data = b"GET /api/users HTTP/1.1\r\nHost: service";
        assert!(parse_request(data).is_none());
    }

    #[test]
    fn test_content_length_extraction() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), "42".to_string()),
        ];
        assert_eq!(get_content_length(&headers), Some(42));
    }

    #[test]
    fn test_chunked_detection() {
        let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        assert!(is_chunked(&headers));
    }
}
