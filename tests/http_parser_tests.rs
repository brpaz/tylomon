use tylomon::http::parser::{get_content_length, is_chunked, parse_request, parse_response};

#[test]
fn test_parse_post_with_body() {
    let data = b"POST /api/orders HTTP/1.1\r\nHost: service-c:3000\r\nContent-Type: application/json\r\nContent-Length: 27\r\n\r\n{\"item\":\"widget\",\"qty\":5}";

    let (req, offset) = parse_request(data).expect("should parse");
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/api/orders");
    assert_eq!(req.headers.len(), 3);

    let content_length = get_content_length(&req.headers);
    assert_eq!(content_length, Some(27));

    // Verify body starts at offset
    let body = &data[offset..];
    assert_eq!(body, b"{\"item\":\"widget\",\"qty\":5}");
}

#[test]
fn test_parse_response_with_chunked() {
    let data = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n";

    let (resp, _offset) = parse_response(data).expect("should parse");
    assert_eq!(resp.status_code, 200);
    assert!(is_chunked(&resp.headers));
}
