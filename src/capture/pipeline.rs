use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pcap::{Capture, Linktype, Packet, Savefile};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::app::Action;
use crate::capture::packet::parse_packet;
use crate::capture::postgres::PgTracker;
use crate::capture::stream::{FlowKey, StreamReassembler};
use crate::export::now_micros;
use crate::http::encoding::decode_body;
use crate::http::parser::{get_content_length, is_chunked, parse_request, parse_response};
use crate::http::types::{HttpExchange, HttpRequest, Protocol};

const PENDING_REQUEST_TIMEOUT_US: u64 = 30_000_000;
const STALE_SWEEP_INTERVAL: u64 = 500;
/// Packets between flushes of a recording, bounding data lost if the process is killed.
const RECORD_FLUSH_INTERVAL: u64 = 100;

/// Capture timestamp in microseconds since the Unix epoch.
type Micros = u64;

struct PendingRequest {
    request: HttpRequest,
    src_ip: String,
    dst_ip: String,
    src_port: u16,
    dst_port: u16,
    timestamp_us: Micros,
}

/// Shared destination for packets recorded from every capture thread.
#[derive(Clone)]
pub struct Recorder {
    file: Arc<Mutex<Savefile>>,
}

impl Recorder {
    /// Create a pcap file at `path`. Bridge interfaces deliver Ethernet frames.
    pub fn create(path: &Path) -> Result<Self, pcap::Error> {
        let file = Capture::dead(Linktype::ETHERNET)?.savefile(path)?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    fn write(&self, packet: &Packet<'_>, flush: bool) {
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        file.write(packet);
        if flush && let Err(e) = file.flush() {
            warn!("Failed to flush recording: {e}");
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

fn packet_time(packet: &Packet<'_>) -> Micros {
    packet.header.ts.tv_sec as u64 * 1_000_000 + packet.header.ts.tv_usec as u64
}

/// Turns raw packets into HTTP exchanges.
struct Pipeline {
    reassembler: StreamReassembler,
    pending_requests: HashMap<FlowKey, PendingRequest>,
    postgres: PgTracker,
    packet_count: u64,
}

impl Pipeline {
    fn new() -> Self {
        Self {
            reassembler: StreamReassembler::new(),
            pending_requests: HashMap::new(),
            postgres: PgTracker::new(),
            packet_count: 0,
        }
    }

    fn handle_packet(&mut self, data: &[u8], now: Micros, action_tx: &mpsc::Sender<Action>) {
        self.packet_count += 1;

        if let Some(parsed) = parse_packet(data)
            && let Some(stream_data) = self.reassembler.process_packet(&parsed)
            && !parsed.payload.is_empty()
        {
            let consumed = match self
                .postgres
                .process(&stream_data.flow, &stream_data.data, now)
            {
                Some((consumed, exchanges)) => {
                    for exchange in exchanges {
                        let _ = action_tx.blocking_send(Action::NewExchange(Box::new(exchange)));
                    }
                    consumed
                }
                None => process_stream_data(
                    &stream_data.flow,
                    &stream_data.data,
                    now,
                    &mut self.pending_requests,
                    action_tx,
                ),
            };
            if consumed > 0 {
                self.reassembler.drain(&stream_data.flow, consumed);
            }
        }

        if self.packet_count.is_multiple_of(STALE_SWEEP_INTERVAL) {
            self.reassembler.cleanup_stale(1024);
            self.postgres.cleanup(1024);
            self.sweep(now, action_tx);
        }
    }

    /// Flush pending requests that have waited longer than the timeout as no-response
    /// exchanges.
    fn sweep(&mut self, now: Micros, action_tx: &mpsc::Sender<Action>) {
        for exchange in self.postgres.sweep(now) {
            let _ = action_tx.blocking_send(Action::NewExchange(Box::new(exchange)));
        }
        let stale_keys: Vec<FlowKey> = self
            .pending_requests
            .iter()
            .filter(|(_, req)| now.saturating_sub(req.timestamp_us) > PENDING_REQUEST_TIMEOUT_US)
            .map(|(key, _)| key.clone())
            .collect();

        for key in stale_keys {
            if let Some(pending) = self.pending_requests.remove(&key) {
                debug!("Flushing stale pending request as no-response exchange");
                let _ = action_tx
                    .blocking_send(Action::NewExchange(Box::new(unanswered_exchange(pending))));
            }
        }
    }
}

fn unanswered_exchange(pending: PendingRequest) -> HttpExchange {
    HttpExchange {
        request: pending.request,
        response: None,
        src_ip: pending.src_ip,
        dst_ip: pending.dst_ip,
        src_port: pending.src_port,
        dst_port: pending.dst_port,
        id: 0,
        protocol: Protocol::Http,
        timestamp_us: Some(pending.timestamp_us),
        duration_ms: None,
    }
}

/// Consecutive read errors after which a capture is considered dead (e.g. the interface
/// was removed).
/// Pause between reads while no packets are waiting.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(10);

const MAX_CONSECUTIVE_READ_ERRORS: u32 = 10;

/// A running live capture thread.
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    /// Set when the capture could not start; retrying would fail the same way.
    fatal: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl CaptureHandle {
    /// True once the thread ended on its own (e.g. its interface vanished or was not up yet),
    /// meaning a new capture on the same network could succeed.
    pub fn needs_restart(&self) -> bool {
        self.thread.is_finished() && !self.fatal.load(Ordering::Relaxed)
    }

    /// Ask the thread to stop and wait for it, which also closes any recording handle.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

pub fn spawn_capture_task(
    bridge_interface: String,
    action_tx: mpsc::Sender<Action>,
    recorder: Option<Recorder>,
) -> CaptureHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let fatal = Arc::new(AtomicBool::new(false));
    let thread = {
        let (stop, fatal) = (stop.clone(), fatal.clone());
        std::thread::spawn(move || {
            if let Err(e) = run_capture(&bridge_interface, &action_tx, recorder.as_ref(), &stop) {
                error!("Capture task failed: {e}");
                let permission_denied = e.to_string().contains("CAP_NET_RAW");
                // Missing privileges will not fix themselves; other failures (such as an
                // interface that is not up yet) are retried by the network watcher.
                fatal.store(permission_denied, Ordering::Relaxed);
                let message = if permission_denied {
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
                let _ = action_tx.blocking_send(Action::CaptureError {
                    interface: bridge_interface,
                    message,
                });
            }
        })
    };
    CaptureHandle {
        stop,
        fatal,
        thread,
    }
}

/// Replay a pcap file through the pipeline as fast as it can be read. The file is opened
/// before the thread starts so that a bad path is reported to the caller.
pub fn spawn_replay_task(
    path: &Path,
    action_tx: mpsc::Sender<Action>,
) -> Result<std::thread::JoinHandle<()>, pcap::Error> {
    let capture = Capture::from_file(path)?;
    Ok(std::thread::spawn(move || {
        let count = replay(capture, &action_tx);
        info!(packets = count, "Replay finished");
    }))
}

fn replay(mut capture: Capture<pcap::Offline>, action_tx: &mpsc::Sender<Action>) -> u64 {
    let mut pipeline = Pipeline::new();
    let mut last_packet_time = 0;

    loop {
        match capture.next_packet() {
            Ok(packet) => {
                last_packet_time = packet_time(&packet);
                pipeline.handle_packet(packet.data, last_packet_time, action_tx);
            }
            Err(pcap::Error::NoMorePackets) => break,
            Err(e) => {
                warn!("Packet read error: {e}");
                break;
            }
        }
        if action_tx.is_closed() {
            return pipeline.packet_count;
        }
    }

    // Anything still unanswered at the end of the recording never got a response.
    pipeline.sweep(
        last_packet_time.saturating_add(PENDING_REQUEST_TIMEOUT_US + 1),
        action_tx,
    );
    pipeline.packet_count
}

fn run_capture(
    bridge_interface: &str,
    action_tx: &mpsc::Sender<Action>,
    recorder: Option<&Recorder>,
    stop: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cap = Capture::from_device(bridge_interface)?
        .promisc(true)
        .snaplen(65535)
        .timeout(100)
        .open()?
        // A blocking read on an idle interface may never time out, which would make the
        // thread impossible to stop; poll instead.
        .setnonblock()?;

    cap.filter("tcp", true)?;

    info!(interface = bridge_interface, "Capture started");
    let _ = action_tx.blocking_send(Action::CaptureStarted(bridge_interface.to_string()));

    let mut pipeline = Pipeline::new();
    let mut read_errors = 0;

    loop {
        if action_tx.is_closed() || stop.load(Ordering::Relaxed) {
            break;
        }

        match cap.next_packet() {
            Ok(packet) => {
                read_errors = 0;
                if let Some(recorder) = recorder {
                    let flush = (pipeline.packet_count + 1).is_multiple_of(RECORD_FLUSH_INTERVAL);
                    recorder.write(&packet, flush);
                }
                pipeline.handle_packet(packet.data, packet_time(&packet), action_tx);
            }
            Err(pcap::Error::TimeoutExpired) => {
                read_errors = 0;
                if let Some(recorder) = recorder {
                    recorder.flush();
                }
                pipeline.sweep(now_micros(), action_tx);
                std::thread::sleep(IDLE_POLL_INTERVAL);
            }
            Err(e) => {
                warn!("Packet read error: {e}");
                read_errors += 1;
                if read_errors >= MAX_CONSECUTIVE_READ_ERRORS {
                    warn!(
                        interface = bridge_interface,
                        "Giving up on capture after repeated read errors"
                    );
                    break;
                }
            }
        }
    }

    info!("Capture task shutting down");
    Ok(())
}

/// Returns the total number of bytes consumed from the buffer.
fn process_stream_data(
    flow: &FlowKey,
    data: &[u8],
    now: Micros,
    pending_requests: &mut HashMap<FlowKey, PendingRequest>,
    action_tx: &mpsc::Sender<Action>,
) -> usize {
    let mut offset = 0;

    loop {
        let remaining = &data[offset..];
        if remaining.is_empty() {
            break;
        }

        let consumed = try_parse_message(flow, remaining, now, pending_requests, action_tx);
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
    now: Micros,
    pending_requests: &mut HashMap<FlowKey, PendingRequest>,
    action_tx: &mpsc::Sender<Action>,
) -> usize {
    if let Some((request, header_len)) = parse_request(data) {
        let (body, body_len) = extract_body(data, header_len, &request.headers);

        let mut request = request;
        request.body = decode_if_encoded(&request.headers, body);

        let total_consumed = header_len + body_len;

        let pending = PendingRequest {
            request,
            src_ip: flow.src_ip.to_string(),
            dst_ip: flow.dst_ip.to_string(),
            src_port: flow.src_port,
            dst_port: flow.dst_port,
            timestamp_us: now,
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
        response.body = decode_if_encoded(&response.headers, body);

        let total_consumed = header_len + body_len;

        if let Some(pending) = pending_requests.remove(&reverse_flow) {
            let duration_ms = Some(now.saturating_sub(pending.timestamp_us) / 1000);

            let exchange = HttpExchange {
                request: pending.request,
                response: Some(response),
                src_ip: pending.src_ip,
                dst_ip: pending.dst_ip,
                src_port: pending.src_port,
                dst_port: pending.dst_port,
                id: 0,
                protocol: Protocol::Http,
                timestamp_us: Some(pending.timestamp_us),
                duration_ms,
            };

            let _ = action_tx.blocking_send(Action::NewExchange(Box::new(exchange)));
        }

        return total_consumed;
    }

    0
}

/// Replace a compressed body with its decoded content so every view shows readable text.
fn decode_if_encoded(headers: &[(String, String)], body: Option<Vec<u8>>) -> Option<Vec<u8>> {
    body.map(|raw| decode_body(headers, &raw).unwrap_or(raw))
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
