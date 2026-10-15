//! Cleartext HTTP/2 (h2c with prior knowledge) and gRPC decoder.
//!
//! A connection is recognised by the client connection preface and then followed in both
//! directions: frames are parsed, header blocks are decoded with one HPACK context per
//! direction, and each stream becomes one exchange. Streams whose request has a
//! `application/grpc` content type become [`Protocol::Grpc`] exchanges with their messages
//! decoded schema-less; everything else is a plain HTTP exchange with version `HTTP/2`.

use std::collections::HashMap;

use fluke_hpack::Decoder;

use crate::capture::protobuf::decode_raw;
use crate::capture::stream::FlowKey;
use crate::http::encoding::decode_body;
use crate::http::types::{HttpExchange, HttpRequest, HttpResponse, Protocol};

/// Capture timestamp in microseconds since the Unix epoch.
type Micros = u64;

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const FRAME_HEADER_LEN: usize = 9;
/// Per direction and stream; the rest of a larger body is dropped.
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_STREAMS: usize = 256;
/// Largest dynamic table an encoder may ask for.
const MAX_HPACK_TABLE: usize = 1 << 20;
/// Streams still waiting for response headers after this long are reported unanswered.
const PENDING_TIMEOUT_US: u64 = 30_000_000;
const VERSION: &str = "HTTP/2";

const DATA: u8 = 0;
const HEADERS: u8 = 1;
const RST_STREAM: u8 = 3;
const PUSH_PROMISE: u8 = 5;
const CONTINUATION: u8 = 9;

const FLAG_END_STREAM: u8 = 0x1;
const FLAG_END_HEADERS: u8 = 0x4;
const FLAG_PADDED: u8 = 0x8;
const FLAG_PRIORITY: u8 = 0x20;

type Headers = Vec<(String, String)>;

/// True if `data` starts with the HTTP/2 client connection preface.
fn has_preface(data: &[u8]) -> bool {
    data.starts_with(PREFACE)
}

struct Frame<'a> {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &'a [u8],
}

/// Split the next frame off `data`; `None` while it is incomplete.
fn next_frame(data: &[u8]) -> Option<(Frame<'_>, usize)> {
    let header = data.get(..FRAME_HEADER_LEN)?;
    let len = usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
    let payload = data.get(FRAME_HEADER_LEN..FRAME_HEADER_LEN + len)?;
    Some((
        Frame {
            kind: header[3],
            flags: header[4],
            stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff,
            payload,
        },
        FRAME_HEADER_LEN + len,
    ))
}

/// Remove padding (`PADDED` flag) from a frame payload.
fn unpad(payload: &[u8], flags: u8) -> Option<&[u8]> {
    if flags & FLAG_PADDED == 0 {
        return Some(payload);
    }
    let (&pad, rest) = payload.split_first()?;
    rest.get(..rest.len().checked_sub(usize::from(pad))?)
}

/// A header block spread over HEADERS and CONTINUATION frames.
struct HeaderBlock {
    stream: u32,
    end_stream: bool,
    /// A PUSH_PROMISE block is decoded to keep HPACK in sync and then discarded.
    ignore: bool,
    fragment: Vec<u8>,
}

struct Direction {
    decoder: Decoder<'static>,
    block: Option<HeaderBlock>,
}

impl Direction {
    fn new() -> Self {
        let mut decoder = Decoder::new();
        decoder.set_max_allowed_table_size(MAX_HPACK_TABLE);
        Self {
            decoder,
            block: None,
        }
    }
}

#[derive(Default)]
struct Stream {
    start: Micros,
    request_headers: Headers,
    request_data: Vec<u8>,
    response_headers: Headers,
    response_started: bool,
    trailers: Headers,
    response_data: Vec<u8>,
    reset: Option<u32>,
}

struct Conn {
    client: FlowKey,
    started: Micros,
    preface_seen: bool,
    dead: bool,
    client_dir: Direction,
    server_dir: Direction,
    streams: HashMap<u32, Stream>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Client,
    Server,
}

impl Conn {
    fn new(client: FlowKey, now: Micros) -> Self {
        Self {
            client,
            started: now,
            preface_seen: false,
            dead: false,
            client_dir: Direction::new(),
            server_dir: Direction::new(),
            streams: HashMap::new(),
        }
    }

    fn data(&mut self, side: Side, data: &[u8], now: Micros, out: &mut Vec<HttpExchange>) -> usize {
        let mut offset = 0;
        if side == Side::Client && !self.preface_seen {
            if data.len() < PREFACE.len() {
                return 0;
            }
            if !has_preface(data) {
                self.dead = true;
                return data.len();
            }
            self.preface_seen = true;
            offset = PREFACE.len();
        }

        while !self.dead {
            let Some((frame, total)) = next_frame(&data[offset..]) else {
                break;
            };
            self.frame(side, &frame, now, out);
            offset += total;
        }
        if self.dead { data.len() } else { offset }
    }

    fn frame(&mut self, side: Side, frame: &Frame, now: Micros, out: &mut Vec<HttpExchange>) {
        match frame.kind {
            HEADERS => {
                let Some(mut payload) = unpad(frame.payload, frame.flags) else {
                    return self.dead = true;
                };
                if frame.flags & FLAG_PRIORITY != 0 {
                    let Some(rest) = payload.get(5..) else {
                        return self.dead = true;
                    };
                    payload = rest;
                }
                self.start_block(
                    side,
                    HeaderBlock {
                        stream: frame.stream,
                        end_stream: frame.flags & FLAG_END_STREAM != 0,
                        ignore: false,
                        fragment: payload.to_vec(),
                    },
                    frame.flags & FLAG_END_HEADERS != 0,
                    now,
                    out,
                );
            }
            PUSH_PROMISE => {
                let Some(payload) = unpad(frame.payload, frame.flags).and_then(|p| p.get(4..))
                else {
                    return self.dead = true;
                };
                self.start_block(
                    side,
                    HeaderBlock {
                        stream: frame.stream,
                        end_stream: false,
                        ignore: true,
                        fragment: payload.to_vec(),
                    },
                    frame.flags & FLAG_END_HEADERS != 0,
                    now,
                    out,
                );
            }
            CONTINUATION => {
                let direction = self.direction(side);
                let Some(block) = direction.block.as_mut() else {
                    return self.dead = true;
                };
                block.fragment.extend_from_slice(frame.payload);
                if frame.flags & FLAG_END_HEADERS != 0 {
                    let block = direction.block.take();
                    if let Some(block) = block {
                        self.finish_block(side, block, now, out);
                    }
                }
            }
            DATA => {
                let Some(payload) = unpad(frame.payload, frame.flags) else {
                    return self.dead = true;
                };
                self.stream_data(
                    side,
                    frame.stream,
                    payload,
                    frame.flags & FLAG_END_STREAM != 0,
                    now,
                    out,
                );
            }
            RST_STREAM => {
                let code = frame
                    .payload
                    .get(..4)
                    .map_or(0, |c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
                if let Some(mut stream) = self.streams.remove(&frame.stream) {
                    stream.reset = Some(code);
                    out.push(self.exchange(stream, Some(now)));
                }
            }
            // SETTINGS, PING, GOAWAY, WINDOW_UPDATE and PRIORITY carry nothing to show.
            _ => {}
        }
    }

    fn direction(&mut self, side: Side) -> &mut Direction {
        match side {
            Side::Client => &mut self.client_dir,
            Side::Server => &mut self.server_dir,
        }
    }

    fn start_block(
        &mut self,
        side: Side,
        block: HeaderBlock,
        complete: bool,
        now: Micros,
        out: &mut Vec<HttpExchange>,
    ) {
        if complete {
            self.finish_block(side, block, now, out);
        } else {
            self.direction(side).block = Some(block);
        }
    }

    fn finish_block(
        &mut self,
        side: Side,
        block: HeaderBlock,
        now: Micros,
        out: &mut Vec<HttpExchange>,
    ) {
        // Every header block must go through HPACK, even for ignored streams, or the
        // dynamic table would drift out of sync.
        let decoded = match self.direction(side).decoder.decode(&block.fragment) {
            Ok(decoded) => decoded,
            Err(error) => {
                tracing::debug!(?error, "HPACK decoding failed; ignoring the connection");
                self.dead = true;
                return;
            }
        };
        if block.ignore {
            return;
        }
        let headers: Headers = decoded
            .into_iter()
            .map(|(name, value)| {
                (
                    String::from_utf8_lossy(&name).into_owned(),
                    String::from_utf8_lossy(&value).into_owned(),
                )
            })
            .collect();
        self.headers(side, block.stream, headers, block.end_stream, now, out);
    }

    fn headers(
        &mut self,
        side: Side,
        id: u32,
        headers: Headers,
        end_stream: bool,
        now: Micros,
        out: &mut Vec<HttpExchange>,
    ) {
        match side {
            Side::Client => {
                if self.streams.len() >= MAX_STREAMS && !self.streams.contains_key(&id) {
                    return;
                }
                let stream = self.streams.entry(id).or_insert_with(|| Stream {
                    start: now,
                    ..Stream::default()
                });
                if stream.request_headers.is_empty() {
                    stream.request_headers = headers;
                }
            }
            Side::Server => {
                let Some(stream) = self.streams.get_mut(&id) else {
                    return;
                };
                let informational = headers
                    .iter()
                    .any(|(n, v)| n == ":status" && v.starts_with('1'));
                if informational {
                    return;
                }
                if stream.response_started {
                    stream.trailers = headers;
                } else {
                    stream.response_headers = headers;
                    stream.response_started = true;
                }
                if end_stream && let Some(done) = self.streams.remove(&id) {
                    out.push(self.exchange(done, Some(now)));
                }
            }
        }
    }

    fn stream_data(
        &mut self,
        side: Side,
        id: u32,
        payload: &[u8],
        end_stream: bool,
        now: Micros,
        out: &mut Vec<HttpExchange>,
    ) {
        let Some(stream) = self.streams.get_mut(&id) else {
            return;
        };
        let buffer = match side {
            Side::Client => &mut stream.request_data,
            Side::Server => &mut stream.response_data,
        };
        let room = MAX_BODY_BYTES.saturating_sub(buffer.len());
        buffer.extend_from_slice(&payload[..payload.len().min(room)]);

        if side == Side::Server
            && end_stream
            && let Some(done) = self.streams.remove(&id)
        {
            out.push(self.exchange(done, Some(now)));
        }
    }

    /// Build the exchange for a stream; `end` is `None` when no response arrived.
    fn exchange(&self, stream: Stream, end: Option<Micros>) -> HttpExchange {
        let pseudo = |name: &str| {
            stream
                .request_headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        let grpc = header(&stream.request_headers, "content-type")
            .is_some_and(|ct| ct.starts_with("application/grpc"));

        let mut request_headers: Headers = Vec::new();
        if let Some(authority) = pseudo(":authority") {
            request_headers.push(("host".to_string(), authority));
        }
        request_headers.extend(
            stream
                .request_headers
                .iter()
                .filter(|(n, _)| !n.starts_with(':'))
                .cloned(),
        );

        let method = pseudo(":method").unwrap_or_else(|| "?".to_string());
        let request_body = if grpc {
            grpc_text(
                &stream.request_data,
                header(&stream.request_headers, "grpc-encoding"),
            )
        } else {
            decoded_body(&request_headers, &stream.request_data)
        };

        let response = (end.is_some() && stream.response_started)
            .then(|| self.response(&stream, grpc))
            .or_else(|| {
                // A reset stream without a response is an error rather than a missing one.
                stream.reset.map(|code| HttpResponse {
                    status_code: 500,
                    reason: format!("RST_STREAM {}", reset_name(code)),
                    version: VERSION.to_string(),
                    headers: Vec::new(),
                    body: None,
                })
            });

        HttpExchange {
            id: 0,
            protocol: if grpc { Protocol::Grpc } else { Protocol::Http },
            request: HttpRequest {
                method: if grpc { "gRPC".to_string() } else { method },
                path: pseudo(":path").unwrap_or_else(|| "/".to_string()),
                version: VERSION.to_string(),
                headers: request_headers,
                body: request_body,
            },
            response,
            src_ip: self.client.src_ip.to_string(),
            dst_ip: self.client.dst_ip.to_string(),
            src_port: self.client.src_port,
            dst_port: self.client.dst_port,
            timestamp_us: Some(stream.start),
            duration_ms: end.map(|e| e.saturating_sub(stream.start) / 1000),
        }
    }

    fn response(&self, stream: &Stream, grpc: bool) -> HttpResponse {
        let http_status: u16 = header(&stream.response_headers, ":status")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut headers: Headers = stream
            .response_headers
            .iter()
            .chain(&stream.trailers)
            .filter(|(n, _)| !n.starts_with(':'))
            .cloned()
            .collect();

        if !grpc {
            let body = decoded_body(&headers, &stream.response_data);
            return HttpResponse {
                status_code: http_status,
                reason: reason_phrase(http_status).to_string(),
                version: VERSION.to_string(),
                headers,
                body,
            };
        }

        // gRPC reports its status in the trailers, or in the headers for "trailers-only"
        // responses.
        let grpc_status = header(&headers, "grpc-status").and_then(|s| s.parse::<u32>().ok());
        let (status_code, reason) = match (grpc_status, stream.reset) {
            (Some(0), _) => (200, "OK".to_string()),
            (Some(code), _) => (500, grpc_status_name(code).to_string()),
            (None, Some(code)) => (500, format!("RST_STREAM {}", reset_name(code))),
            (None, None) if http_status != 200 => (500, format!("HTTP {http_status}")),
            (None, None) => (500, "NO GRPC-STATUS".to_string()),
        };
        headers.retain(|(n, _)| n != "content-length");
        HttpResponse {
            status_code,
            reason,
            version: VERSION.to_string(),
            body: grpc_text(&stream.response_data, header(&headers, "grpc-encoding")),
            headers,
        }
    }

    /// Flush streams that never got a response (`all`: also streams still in progress).
    fn flush(&mut self, now: Micros, all: bool) -> Vec<HttpExchange> {
        let expired: Vec<u32> = self
            .streams
            .iter()
            .filter(|(_, s)| {
                all || (!s.response_started && now.saturating_sub(s.start) > PENDING_TIMEOUT_US)
            })
            .map(|(id, _)| *id)
            .collect();
        let mut flushed = Vec::new();
        for id in expired {
            if let Some(stream) = self.streams.remove(&id) {
                let end = (all && stream.response_started).then_some(now);
                flushed.push(self.exchange(stream, end));
            }
        }
        flushed
    }
}

fn header<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn decoded_body(headers: &Headers, data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    Some(decode_body(headers, data).unwrap_or_else(|| data.to_vec()))
}

/// Render the gRPC messages in a stream's DATA as text, one block per message.
fn grpc_text(data: &[u8], encoding: Option<&str>) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let mut messages = Vec::new();
    let mut pos = 0;
    let mut truncated = false;
    while pos < data.len() {
        let Some(prefix) = data.get(pos..pos + 5) else {
            truncated = true;
            break;
        };
        let compressed = prefix[0] != 0;
        let len = u32::from_be_bytes([prefix[1], prefix[2], prefix[3], prefix[4]]) as usize;
        let Some(payload) = data.get(pos + 5..pos + 5 + len) else {
            truncated = true;
            break;
        };
        pos += 5 + len;
        messages.push(render_message(payload, compressed, encoding));
    }

    let mut out = String::new();
    let many = messages.len() > 1;
    for (index, text) in messages.iter().enumerate() {
        if many {
            out.push_str(&format!("# message {}\n", index + 1));
        }
        out.push_str(text);
        out.push('\n');
    }
    if truncated {
        out.push_str("# …message cut off\n");
    }
    out.truncate(out.trim_end().len());
    Some(out.into_bytes())
}

fn render_message(payload: &[u8], compressed: bool, encoding: Option<&str>) -> String {
    if !compressed {
        return decode_raw(payload);
    }
    let label = encoding.unwrap_or("unknown");
    let headers = vec![("content-encoding".to_string(), label.to_string())];
    match decode_body(&headers, payload) {
        Some(plain) => decode_raw(&plain),
        None => format!("(compressed with {label}, {} bytes)", payload.len()),
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

fn grpc_status_name(code: u32) -> &'static str {
    match code {
        0 => "OK",
        1 => "CANCELLED",
        2 => "UNKNOWN",
        3 => "INVALID_ARGUMENT",
        4 => "DEADLINE_EXCEEDED",
        5 => "NOT_FOUND",
        6 => "ALREADY_EXISTS",
        7 => "PERMISSION_DENIED",
        8 => "RESOURCE_EXHAUSTED",
        9 => "FAILED_PRECONDITION",
        10 => "ABORTED",
        11 => "OUT_OF_RANGE",
        12 => "UNIMPLEMENTED",
        13 => "INTERNAL",
        14 => "UNAVAILABLE",
        15 => "DATA_LOSS",
        16 => "UNAUTHENTICATED",
        _ => "UNKNOWN_STATUS",
    }
}

fn reset_name(code: u32) -> &'static str {
    match code {
        0 => "NO_ERROR",
        1 => "PROTOCOL_ERROR",
        2 => "INTERNAL_ERROR",
        3 => "FLOW_CONTROL_ERROR",
        4 => "SETTINGS_TIMEOUT",
        5 => "STREAM_CLOSED",
        6 => "FRAME_SIZE_ERROR",
        7 => "REFUSED_STREAM",
        8 => "CANCEL",
        9 => "COMPRESSION_ERROR",
        10 => "CONNECT_ERROR",
        11 => "ENHANCE_YOUR_CALM",
        12 => "INADEQUATE_SECURITY",
        13 => "HTTP_1_1_REQUIRED",
        _ => "UNKNOWN",
    }
}

/// Follows every cleartext HTTP/2 connection seen on the wire.
#[derive(Default)]
pub struct H2Tracker {
    conns: HashMap<FlowKey, Conn>,
}

impl H2Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle accumulated stream bytes for `flow`. Returns `None` if the flow is not
    /// HTTP/2, otherwise the number of bytes consumed and any finished exchanges.
    pub fn process(
        &mut self,
        flow: &FlowKey,
        data: &[u8],
        now: Micros,
    ) -> Option<(usize, Vec<HttpExchange>)> {
        let mut out = Vec::new();

        if let Some(conn) = self.conns.get_mut(flow) {
            let consumed = conn.data(Side::Client, data, now, &mut out);
            if conn.dead {
                self.conns.remove(flow);
            }
            return Some((consumed, out));
        }
        let reverse = flow.reverse();
        if let Some(conn) = self.conns.get_mut(&reverse) {
            let consumed = conn.data(Side::Server, data, now, &mut out);
            if conn.dead {
                self.conns.remove(&reverse);
            }
            return Some((consumed, out));
        }
        if has_preface(data) {
            let mut conn = Conn::new(flow.clone(), now);
            let consumed = conn.data(Side::Client, data, now, &mut out);
            self.conns.insert(flow.clone(), conn);
            return Some((consumed, out));
        }
        None
    }

    /// Report requests that never got response headers within the timeout.
    pub fn sweep(&mut self, now: Micros) -> Vec<HttpExchange> {
        self.conns
            .values_mut()
            .flat_map(|c| c.flush(now, false))
            .collect()
    }

    /// Report every stream still open, for the end of a recording.
    pub fn flush_all(&mut self, now: Micros) -> Vec<HttpExchange> {
        self.conns
            .values_mut()
            .flat_map(|c| c.flush(now, true))
            .collect()
    }

    /// Drop the oldest connections beyond `max`.
    pub fn cleanup(&mut self, max: usize) {
        while self.conns.len() > max {
            let Some(oldest) = self
                .conns
                .values()
                .min_by_key(|c| c.started)
                .map(|c| c.client.clone())
            else {
                break;
            };
            self.conns.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use fluke_hpack::Encoder;

    use super::*;

    fn flow() -> FlowKey {
        FlowKey {
            src_ip: "10.0.0.2".parse().expect("ip"),
            src_port: 40000,
            dst_ip: "10.0.0.3".parse().expect("ip"),
            dst_port: 9000,
        }
    }

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let len = payload.len() as u32;
        let mut out = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags];
        out.extend(stream.to_be_bytes());
        out.extend(payload);
        out
    }

    fn block(encoder: &mut Encoder, headers: &[(&str, &str)]) -> Vec<u8> {
        encoder.encode(headers.iter().map(|(n, v)| (n.as_bytes(), v.as_bytes())))
    }

    fn headers_frame(stream: u32, flags: u8, block: &[u8]) -> Vec<u8> {
        frame(HEADERS, flags | FLAG_END_HEADERS, stream, block)
    }

    fn settings() -> Vec<u8> {
        frame(4, 0, 0, &[])
    }

    fn grpc_message(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0];
        out.extend((payload.len() as u32).to_be_bytes());
        out.extend(payload);
        out
    }

    /// Protobuf message with `1: "<text>"`.
    fn proto_string(text: &str) -> Vec<u8> {
        let mut out = vec![0x0a, text.len() as u8];
        out.extend(text.as_bytes());
        out
    }

    struct Wire {
        tracker: H2Tracker,
        client_encoder: Encoder<'static>,
        server_encoder: Encoder<'static>,
    }

    impl Wire {
        fn new() -> Self {
            let mut tracker = H2Tracker::new();
            let opening = [PREFACE.to_vec(), settings()].concat();
            let (consumed, out) = tracker.process(&flow(), &opening, 1_000_000).expect("h2");
            assert_eq!(consumed, opening.len());
            assert!(out.is_empty());
            Self {
                tracker,
                client_encoder: Encoder::new(),
                server_encoder: Encoder::new(),
            }
        }

        fn client(&mut self, data: &[u8], at: Micros) -> (usize, Vec<HttpExchange>) {
            self.tracker.process(&flow(), data, at).expect("h2")
        }

        fn server(&mut self, data: &[u8], at: Micros) -> Vec<HttpExchange> {
            self.tracker
                .process(&flow().reverse(), data, at)
                .expect("h2")
                .1
        }
    }

    #[test]
    fn only_the_client_preface_identifies_http2() {
        let mut tracker = H2Tracker::new();
        assert!(
            tracker
                .process(&flow(), b"GET / HTTP/1.1\r\n\r\n", 0)
                .is_none()
        );
        assert!(tracker.process(&flow(), &settings(), 0).is_none());
    }

    #[test]
    fn plain_http2_request_becomes_an_http_exchange() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[
                (":method", "GET"),
                (":scheme", "http"),
                (":authority", "api.local"),
                (":path", "/users?id=1"),
                ("accept", "application/json"),
            ],
        );
        wire.client(&headers_frame(1, FLAG_END_STREAM, &request), 2_000_000);

        let response = block(
            &mut wire.server_encoder,
            &[(":status", "200"), ("content-type", "application/json")],
        );
        let done = wire.server(
            &[
                headers_frame(1, 0, &response),
                frame(DATA, FLAG_END_STREAM, 1, br#"{"ok":true}"#),
            ]
            .concat(),
            2_020_000,
        );

        assert_eq!(done.len(), 1);
        let ex = &done[0];
        assert_eq!(ex.protocol, Protocol::Http);
        assert_eq!(ex.request.method, "GET");
        assert_eq!(ex.request.path, "/users?id=1");
        assert_eq!(ex.request.version, "HTTP/2");
        assert!(
            ex.request
                .headers
                .contains(&("host".into(), "api.local".into()))
        );
        assert!(ex.request.headers.iter().all(|(n, _)| !n.starts_with(':')));
        let response = ex.response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (200, "OK")
        );
        assert_eq!(response.body.as_deref(), Some(br#"{"ok":true}"#.as_slice()));
        assert_eq!(ex.duration_ms, Some(20));
        assert_eq!(ex.dst_port, 9000);
    }

    #[test]
    fn grpc_unary_call_is_decoded_with_its_status() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[
                (":method", "POST"),
                (":scheme", "http"),
                (":authority", "greeter:9000"),
                (":path", "/helloworld.Greeter/SayHello"),
                ("content-type", "application/grpc"),
                ("te", "trailers"),
            ],
        );
        wire.client(
            &[
                headers_frame(1, 0, &request),
                frame(
                    DATA,
                    FLAG_END_STREAM,
                    1,
                    &grpc_message(&proto_string("world")),
                ),
            ]
            .concat(),
            3_000_000,
        );

        let headers = block(
            &mut wire.server_encoder,
            &[(":status", "200"), ("content-type", "application/grpc")],
        );
        let trailers = block(&mut wire.server_encoder, &[("grpc-status", "0")]);
        let done = wire.server(
            &[
                headers_frame(1, 0, &headers),
                frame(DATA, 0, 1, &grpc_message(&proto_string("Hello world"))),
                headers_frame(1, FLAG_END_STREAM, &trailers),
            ]
            .concat(),
            3_004_000,
        );

        assert_eq!(done.len(), 1);
        let ex = &done[0];
        assert_eq!(ex.protocol, Protocol::Grpc);
        assert_eq!(ex.request.method, "gRPC");
        assert_eq!(ex.request.path, "/helloworld.Greeter/SayHello");
        assert_eq!(ex.request.body.as_deref(), Some(b"1: \"world\"".as_slice()));
        let response = ex.response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (200, "OK")
        );
        assert_eq!(
            response.body.as_deref(),
            Some(b"1: \"Hello world\"".as_slice())
        );
        assert!(
            response
                .headers
                .contains(&("grpc-status".into(), "0".into()))
        );
        assert_eq!(ex.duration_ms, Some(4));
    }

    #[test]
    fn grpc_error_in_a_trailers_only_response() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[
                (":method", "POST"),
                (":authority", "svc"),
                (":path", "/pkg.Svc/Get"),
                ("content-type", "application/grpc+proto"),
            ],
        );
        wire.client(&headers_frame(3, 0, &request), 0);
        wire.client(
            &frame(DATA, FLAG_END_STREAM, 3, &grpc_message(&[0x08, 0x01])),
            1,
        );

        let response = block(
            &mut wire.server_encoder,
            &[
                (":status", "200"),
                ("content-type", "application/grpc"),
                ("grpc-status", "5"),
                ("grpc-message", "user not found"),
            ],
        );
        let done = wire.server(&headers_frame(3, FLAG_END_STREAM, &response), 9_000);

        let response = done[0].response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (500, "NOT_FOUND")
        );
        assert!(
            response
                .headers
                .contains(&("grpc-message".into(), "user not found".into()))
        );
    }

    #[test]
    fn hpack_context_is_shared_across_streams() {
        let mut wire = Wire::new();
        for (stream, path) in [(1u32, "/first"), (3, "/second")] {
            let request = block(
                &mut wire.client_encoder,
                &[
                    (":method", "GET"),
                    (":authority", "svc"),
                    (":path", path),
                    ("x-long-custom-header", "same-value-every-time"),
                ],
            );
            wire.client(&headers_frame(stream, FLAG_END_STREAM, &request), 0);
            let response = block(&mut wire.server_encoder, &[(":status", "204")]);
            let done = wire.server(&headers_frame(stream, FLAG_END_STREAM, &response), 1);
            assert_eq!(done.len(), 1);
            assert_eq!(done[0].request.path, path);
            assert!(done[0].request.headers.contains(&(
                "x-long-custom-header".into(),
                "same-value-every-time".into()
            )));
        }
    }

    #[test]
    fn header_block_split_over_continuation_frames() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[
                (":method", "GET"),
                (":authority", "svc"),
                (":path", "/split"),
            ],
        );
        let (first, second) = request.split_at(request.len() / 2);
        let data = [
            frame(HEADERS, FLAG_END_STREAM, 1, first),
            frame(CONTINUATION, FLAG_END_HEADERS, 1, second),
        ]
        .concat();
        wire.client(&data, 0);

        let response = block(&mut wire.server_encoder, &[(":status", "200")]);
        let done = wire.server(&headers_frame(1, FLAG_END_STREAM, &response), 1);
        assert_eq!(done[0].request.path, "/split");
    }

    #[test]
    fn padded_and_prioritised_headers_and_data() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[(":method", "POST"), (":authority", "svc"), (":path", "/up")],
        );
        let mut payload = vec![2]; // pad length
        payload.extend([0, 0, 0, 0, 16]); // priority
        payload.extend(&request);
        payload.extend([0, 0]); // padding
        wire.client(
            &frame(
                HEADERS,
                FLAG_END_HEADERS | FLAG_PADDED | FLAG_PRIORITY,
                1,
                &payload,
            ),
            0,
        );
        let mut data = vec![3];
        data.extend(b"body");
        data.extend([0, 0, 0]);
        wire.client(&frame(DATA, FLAG_PADDED | FLAG_END_STREAM, 1, &data), 1);

        let response = block(&mut wire.server_encoder, &[(":status", "201")]);
        let done = wire.server(&headers_frame(1, FLAG_END_STREAM, &response), 2);
        assert_eq!(done[0].request.body.as_deref(), Some(b"body".as_slice()));
        assert_eq!(
            done[0].response.as_ref().expect("response").reason,
            "Created"
        );
    }

    #[test]
    fn frames_split_across_packets_wait_for_the_rest() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[
                (":method", "GET"),
                (":authority", "svc"),
                (":path", "/slow"),
            ],
        );
        let data = headers_frame(1, FLAG_END_STREAM, &request);
        let (consumed, _) = wire.client(&data[..data.len() - 2], 0);
        assert_eq!(consumed, 0);
        let (consumed, _) = wire.client(&data, 1);
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn reset_stream_is_reported_as_an_error() {
        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[(":method", "POST"), (":authority", "svc"), (":path", "/x")],
        );
        wire.client(&headers_frame(1, 0, &request), 0);
        let done = wire.server(&frame(RST_STREAM, 0, 1, &8u32.to_be_bytes()), 5_000);
        let response = done[0].response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (500, "RST_STREAM CANCEL")
        );
    }

    #[test]
    fn compressed_grpc_messages_are_decompressed() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&proto_string("squeezed")).expect("compress");
        let packed = gz.finish().expect("finish");
        let mut message = vec![1];
        message.extend((packed.len() as u32).to_be_bytes());
        message.extend(packed);

        let text = grpc_text(&message, Some("gzip")).expect("text");
        assert_eq!(text, b"1: \"squeezed\"");
        let unsupported = grpc_text(&message, Some("snappy")).expect("text");
        assert!(String::from_utf8_lossy(&unsupported).starts_with("(compressed with snappy"));
    }

    #[test]
    fn several_grpc_messages_are_numbered() {
        let data = [
            grpc_message(&proto_string("a")),
            grpc_message(&proto_string("b")),
        ]
        .concat();
        let text = String::from_utf8(grpc_text(&data, None).expect("text")).expect("utf8");
        assert_eq!(text, "# message 1\n1: \"a\"\n# message 2\n1: \"b\"");
    }

    #[test]
    fn cut_off_grpc_message_is_flagged() {
        let mut data = grpc_message(&proto_string("hello"));
        data.truncate(data.len() - 2);
        let text = String::from_utf8(grpc_text(&data, None).expect("text")).expect("utf8");
        assert!(text.contains("message cut off"));
    }

    #[test]
    fn gzip_http2_response_body_is_decoded() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"plain text").expect("compress");
        let packed = gz.finish().expect("finish");

        let mut wire = Wire::new();
        let request = block(
            &mut wire.client_encoder,
            &[(":method", "GET"), (":authority", "svc"), (":path", "/z")],
        );
        wire.client(&headers_frame(1, FLAG_END_STREAM, &request), 0);
        let response = block(
            &mut wire.server_encoder,
            &[(":status", "200"), ("content-encoding", "gzip")],
        );
        let done = wire.server(
            &[
                headers_frame(1, 0, &response),
                frame(DATA, FLAG_END_STREAM, 1, &packed),
            ]
            .concat(),
            1,
        );
        assert_eq!(
            done[0].response.as_ref().expect("response").body.as_deref(),
            Some(b"plain text".as_slice())
        );
    }

    #[test]
    fn sweep_reports_unanswered_requests_but_not_streaming_responses() {
        let mut wire = Wire::new();
        let request = |encoder: &mut Encoder, path: &str| {
            block(
                encoder,
                &[(":method", "GET"), (":authority", "svc"), (":path", path)],
            )
        };
        let first = request(&mut wire.client_encoder, "/silent");
        let second = request(&mut wire.client_encoder, "/stream");
        wire.client(&headers_frame(1, FLAG_END_STREAM, &first), 5_000_000);
        wire.client(&headers_frame(3, FLAG_END_STREAM, &second), 5_000_000);
        let response = block(&mut wire.server_encoder, &[(":status", "200")]);
        wire.server(&headers_frame(3, 0, &response), 5_100_000);

        assert!(wire.tracker.sweep(10_000_000).is_empty());
        let stale = wire.tracker.sweep(40_000_000);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].request.path, "/silent");
        assert!(stale[0].response.is_none());

        let rest = wire.tracker.flush_all(50_000_000);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].request.path, "/stream");
        assert!(rest[0].response.is_some());
    }

    #[test]
    fn corrupt_header_block_stops_decoding_without_panicking() {
        let mut wire = Wire::new();
        let (consumed, _) = wire.client(&headers_frame(1, 0, &[0xff, 0xff, 0xff, 0xff]), 0);
        assert_eq!(consumed, 13);
        // The connection is forgotten; later bytes no longer look like HTTP/2.
        assert!(wire.tracker.process(&flow(), &settings(), 1).is_none());
    }

    #[test]
    fn cleanup_drops_the_oldest_connections() {
        let mut tracker = H2Tracker::new();
        for port in 0..5u16 {
            let mut f = flow();
            f.src_port = 40000 + port;
            tracker.process(&f, PREFACE, u64::from(port));
        }
        tracker.cleanup(2);
        assert_eq!(tracker.conns.len(), 2);
        assert!(tracker.conns.values().all(|c| c.started >= 3));
    }
}
