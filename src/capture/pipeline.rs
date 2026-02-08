use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::app::Action;
use crate::capture::packet::parse_packet;
use crate::capture::stream::{FlowKey, StreamReassembler};
use crate::http::parser::{get_content_length, is_chunked, parse_request, parse_response};
use crate::http::types::{HttpExchange, HttpRequest};

const PENDING_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STALE_SWEEP_INTERVAL: u64 = 500;

struct PendingRequest {
    request: HttpRequest,
    src_ip: String,
    dst_ip: String,
    src_port: u16,
    dst_port: u16,
    timestamp: Instant,
}

pub fn spawn_capture_task(
    bridge_interface: String,
    action_tx: mpsc::Sender<Action>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(e) = run_capture(&bridge_interface, &action_tx) {
            error!("Capture task failed: {e}");
            let msg = if e.to_string().contains("CAP_NET_RAW") {
                format!(
                    "Capture failed: permission denied. Run with sudo or grant capabilities:\n\
                     sudo setcap cap_net_raw,cap_net_admin=eip {binary}",
                    binary = std::env::current_exe()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "tylomon".to_string())
                )
            } else {
                format!("Capture failed: {e}")
            };
            let _ = action_tx.blocking_send(Action::CaptureError(msg));
        }
    })
}

fn run_capture(
    bridge_interface: &str,
    action_tx: &mpsc::Sender<Action>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cap = pcap::Capture::from_device(bridge_interface)?
        .promisc(true)
        .snaplen(65535)
        .timeout(100)
        .open()?;

    cap.filter("tcp", true)?;

    info!(interface = bridge_interface, "Capture started");

    let mut reassembler = StreamReassembler::new();
    let mut pending_requests: HashMap<FlowKey, PendingRequest> = HashMap::new();
    let mut packet_count: u64 = 0;

    loop {
        if action_tx.is_closed() {
            break;
        }

        let packet_data = match cap.next_packet() {
            Ok(packet) => packet.data.to_vec(),
            Err(pcap::Error::TimeoutExpired) => {
                sweep_stale_requests(&mut pending_requests, action_tx);
                continue;
            }
            Err(e) => {
                warn!("Packet read error: {e}");
                continue;
            }
        };

        packet_count += 1;

        let Some(parsed) = parse_packet(&packet_data) else {
            continue;
        };

        if parsed.payload.is_empty() {
            reassembler.process_packet(&parsed);
            continue;
        }

        let Some(stream_data) = reassembler.process_packet(&parsed) else {
            continue;
        };

        let consumed = process_stream_data(
            &stream_data.flow,
            &stream_data.data,
            &mut pending_requests,
            action_tx,
        );

        if consumed > 0 {
            reassembler.drain(&stream_data.flow, consumed);
        }

        if packet_count.is_multiple_of(STALE_SWEEP_INTERVAL) {
            reassembler.cleanup_stale(1024);
            sweep_stale_requests(&mut pending_requests, action_tx);
        }
    }

    info!("Capture task shutting down");
    Ok(())
}

/// Remove pending requests that have been waiting longer than the timeout,
/// flushing them as no-response exchanges.
fn sweep_stale_requests(
    pending_requests: &mut HashMap<FlowKey, PendingRequest>,
    action_tx: &mpsc::Sender<Action>,
) {
    let stale_keys: Vec<FlowKey> = pending_requests
        .iter()
        .filter(|(_, req)| req.timestamp.elapsed() > PENDING_REQUEST_TIMEOUT)
        .map(|(key, _)| key.clone())
        .collect();

    for key in stale_keys {
        if let Some(pending) = pending_requests.remove(&key) {
            let exchange = HttpExchange {
                request: pending.request,
                response: None,
                src_ip: pending.src_ip,
                dst_ip: pending.dst_ip,
                src_port: pending.src_port,
                dst_port: pending.dst_port,
                timestamp: Some(pending.timestamp),
                duration_ms: None,
            };

            debug!("Flushing stale pending request as no-response exchange");
            let _ = action_tx.blocking_send(Action::NewExchange(Box::new(exchange)));
        }
    }
}

/// Returns the total number of bytes consumed from the buffer.
fn process_stream_data(
    flow: &FlowKey,
    data: &[u8],
    pending_requests: &mut HashMap<FlowKey, PendingRequest>,
    action_tx: &mpsc::Sender<Action>,
) -> usize {
    let mut offset = 0;

    loop {
        let remaining = &data[offset..];
        if remaining.is_empty() {
            break;
        }

        let consumed = try_parse_message(flow, remaining, pending_requests, action_tx);
        if consumed == 0 {
            break;
        }

        offset += consumed;
    }

    offset
}

/// Returns the number of bytes consumed, or 0 if no complete message was found.
fn try_parse_message(
    flow: &FlowKey,
    data: &[u8],
    pending_requests: &mut HashMap<FlowKey, PendingRequest>,
    action_tx: &mpsc::Sender<Action>,
) -> usize {
    if let Some((request, header_len)) = parse_request(data) {
        let (body, body_len) = extract_body(data, header_len, &request.headers);

        let mut request = request;
        request.body = body;

        let total_consumed = header_len + body_len;

        let pending = PendingRequest {
            request,
            src_ip: flow.src_ip.to_string(),
            dst_ip: flow.dst_ip.to_string(),
            src_port: flow.src_port,
            dst_port: flow.dst_port,
            timestamp: Instant::now(),
        };

        pending_requests.insert(flow.clone(), pending);
        debug!(
            src = %flow.src_ip,
            dst = %flow.dst_ip,
            "Parsed HTTP request on flow"
        );
        return total_consumed;
    }

    let reverse_flow = flow.reverse();
    if let Some((response, header_len)) = parse_response(data) {
        let (body, body_len) = extract_body(data, header_len, &response.headers);

        let mut response = response;
        response.body = body;

        let total_consumed = header_len + body_len;

        if let Some(pending) = pending_requests.remove(&reverse_flow) {
            let duration_ms = Some(pending.timestamp.elapsed().as_millis() as u64);

            let exchange = HttpExchange {
                request: pending.request,
                response: Some(response),
                src_ip: pending.src_ip,
                dst_ip: pending.dst_ip,
                src_port: pending.src_port,
                dst_port: pending.dst_port,
                timestamp: Some(pending.timestamp),
                duration_ms,
            };

            let _ = action_tx.blocking_send(Action::NewExchange(Box::new(exchange)));
        }

        return total_consumed;
    }

    0
}

/// Returns `(decoded_body, raw_bytes_consumed)`. For chunked encoding,
/// raw bytes consumed differs from decoded body length.
fn extract_body(
    data: &[u8],
    header_len: usize,
    headers: &[(String, String)],
) -> (Option<Vec<u8>>, usize) {
    let remaining = &data[header_len..];
    if remaining.is_empty() {
        return (None, 0);
    }

    if is_chunked(headers) {
        return decode_chunked(remaining);
    }

    if let Some(content_length) = get_content_length(headers) {
        if remaining.len() >= content_length {
            return (Some(remaining[..content_length].to_vec()), content_length);
        }
        let len = remaining.len();
        return (Some(remaining.to_vec()), len);
    }

    let len = remaining.len();
    (Some(remaining.to_vec()), len)
}

/// Chunked encoding: `{hex_size}\r\n{data}\r\n ... 0\r\n\r\n`
/// Returns `(decoded_body, raw_bytes_consumed)`.
fn decode_chunked(data: &[u8]) -> (Option<Vec<u8>>, usize) {
    let mut decoded = Vec::new();
    let mut pos = 0;

    loop {
        if pos >= data.len() {
            break;
        }

        let Some(crlf_pos) = find_crlf(&data[pos..]) else {
            break;
        };
        let size_end = pos + crlf_pos;

        // Chunk size is hex, optionally followed by extensions after ';'
        let size_str = String::from_utf8_lossy(&data[pos..size_end]);
        let size_hex = size_str.split(';').next().unwrap_or("").trim();
        let Ok(chunk_size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };

        pos = size_end + 2; // past \r\n

        if chunk_size == 0 {
            if data.get(pos..pos + 2) == Some(b"\r\n") {
                pos += 2;
            }
            break;
        }

        if pos + chunk_size > data.len() {
            decoded.extend_from_slice(&data[pos..]);
            pos = data.len();
            break;
        }

        decoded.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size;

        if data.get(pos..pos + 2) == Some(b"\r\n") {
            pos += 2;
        }
    }

    if decoded.is_empty() {
        (None, pos)
    } else {
        (Some(decoded), pos)
    }
}

fn find_crlf(data: &[u8]) -> Option<usize> {
    data.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_chunked_simple() {
        let data = b"5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n";
        let (body, consumed) = decode_chunked(data);
        assert_eq!(body.as_deref(), Some(b"Hello World".as_slice()));
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn test_decode_chunked_single_chunk() {
        let data = b"d\r\nHello, World!\r\n0\r\n\r\n";
        let (body, consumed) = decode_chunked(data);
        assert_eq!(body.as_deref(), Some(b"Hello, World!".as_slice()));
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn test_decode_chunked_with_extensions() {
        let data = b"5;ext=val\r\nHello\r\n0\r\n\r\n";
        let (body, consumed) = decode_chunked(data);
        assert_eq!(body.as_deref(), Some(b"Hello".as_slice()));
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn test_decode_chunked_incomplete() {
        let data = b"5\r\nHel";
        let (body, _consumed) = decode_chunked(data);
        assert_eq!(body.as_deref(), Some(b"Hel".as_slice()));
    }

    #[test]
    fn test_decode_chunked_empty() {
        let data = b"0\r\n\r\n";
        let (body, consumed) = decode_chunked(data);
        assert!(body.is_none());
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn test_extract_body_content_length() {
        let headers = vec![("Content-Length".to_string(), "5".to_string())];
        let data = b"HTTP-HEADER\r\n\r\nHello Extra";
        let header_len = 15;
        let (body, consumed) = extract_body(data, header_len, &headers);
        assert_eq!(body.as_deref(), Some(b"Hello".as_slice()));
        assert_eq!(consumed, 5);
    }

    #[test]
    fn test_extract_body_chunked() {
        let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        let chunked_data = b"5\r\nHello\r\n0\r\n\r\n";
        let mut data = Vec::new();
        data.extend_from_slice(chunked_data);
        let (body, consumed) = extract_body(&data, 0, &headers);
        assert_eq!(body.as_deref(), Some(b"Hello".as_slice()));
        assert_eq!(consumed, chunked_data.len());
    }
}
