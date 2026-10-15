use std::path::{Path, PathBuf};

use etherparse::PacketBuilder;
use pcap::{Capture, Linktype, Packet, PacketHeader};
use tokio::sync::mpsc;

use tylomon::app::Action;
use tylomon::capture::pipeline::spawn_replay_task;
use tylomon::http::types::HttpExchange;

const CLIENT: [u8; 4] = [10, 0, 0, 2];
const SERVER: [u8; 4] = [10, 0, 0, 3];

struct PcapWriter {
    file: pcap::Savefile,
}

impl PcapWriter {
    fn create(path: &Path) -> Self {
        let file = Capture::dead(Linktype::ETHERNET)
            .expect("dead capture")
            .savefile(path)
            .expect("savefile");
        Self { file }
    }

    /// Write a TCP segment at `micros` after the epoch.
    #[allow(clippy::too_many_arguments)]
    fn segment(
        &mut self,
        micros: u64,
        (src, sport): ([u8; 4], u16),
        (dst, dport): ([u8; 4], u16),
        seq: u32,
        syn: bool,
        payload: &[u8],
    ) {
        let mut builder = PacketBuilder::ethernet2([2; 6], [4; 6])
            .ipv4(src, dst, 64)
            .tcp(sport, dport, seq, 4096);
        if syn {
            builder = builder.syn();
        }
        let mut data = Vec::new();
        builder.write(&mut data, payload).expect("build packet");

        let header = PacketHeader {
            ts: libc::timeval {
                tv_sec: (micros / 1_000_000) as _,
                tv_usec: (micros % 1_000_000) as _,
            },
            caplen: data.len() as u32,
            len: data.len() as u32,
        };
        self.file.write(&Packet::new(&header, &data));
    }

    fn finish(mut self) {
        self.file.flush().expect("flush");
    }
}

fn temp_pcap(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tylomon-replay-{tag}-{}.pcap", std::process::id()))
}

fn replay(path: &Path) -> Vec<HttpExchange> {
    let (tx, mut rx) = mpsc::channel::<Action>(64);
    let handle = spawn_replay_task(path, tx).expect("open pcap");
    handle.join().expect("replay thread");
    rx.close();

    let mut exchanges = Vec::new();
    while let Ok(action) = rx.try_recv() {
        if let Action::NewExchange(ex) = action {
            exchanges.push(*ex);
        }
    }
    exchanges
}

#[test]
fn replay_pairs_requests_with_responses_and_measures_capture_time() {
    let path = temp_pcap("pair");
    let mut w = PcapWriter::create(&path);
    let client = (CLIENT, 40000);
    let server = (SERVER, 80);

    w.segment(1_000_000, client, server, 100, true, b"");
    w.segment(1_100_000, server, client, 500, true, b"");
    w.segment(
        2_000_000,
        client,
        server,
        101,
        false,
        b"GET /health HTTP/1.1\r\nHost: api\r\n\r\n",
    );
    w.segment(
        2_250_000,
        server,
        client,
        501,
        false,
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exchanges.len(), 1);
    let ex = &exchanges[0];
    assert_eq!(ex.request.method, "GET");
    assert_eq!(ex.request.path, "/health");
    assert_eq!(ex.response.as_ref().map(|r| r.status_code), Some(200));
    assert_eq!(ex.src_ip, "10.0.0.2");
    assert_eq!(ex.dst_ip, "10.0.0.3");
    assert_eq!(ex.duration_ms, Some(250));
}

#[test]
fn replay_flushes_unanswered_requests_at_end_of_file() {
    let path = temp_pcap("unanswered");
    let mut w = PcapWriter::create(&path);

    w.segment(
        1_000_000,
        (CLIENT, 40001),
        (SERVER, 80),
        1,
        false,
        b"POST /slow HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].request.path, "/slow");
    assert!(exchanges[0].response.is_none());
    assert_eq!(exchanges[0].duration_ms, None);
}

#[test]
fn replay_of_missing_file_fails_up_front() {
    let (tx, _rx) = mpsc::channel::<Action>(1);
    let missing = std::env::temp_dir().join("tylomon-does-not-exist.pcap");
    assert!(spawn_replay_task(&missing, tx).is_err());
}

#[test]
fn replay_ignores_non_http_traffic() {
    let path = temp_pcap("noise");
    let mut w = PcapWriter::create(&path);
    w.segment(
        1_000_000,
        (CLIENT, 40002),
        (SERVER, 5432),
        1,
        false,
        b"\x00\x00\x00\x08not http",
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);
    assert!(exchanges.is_empty());
}

#[test]
fn replay_decodes_gzip_response_bodies() {
    use std::io::Write;

    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(br#"{"status":"healthy"}"#).expect("compress");
    let compressed = enc.finish().expect("finish");

    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
        compressed.len()
    )
    .into_bytes();
    response.extend_from_slice(&compressed);

    let path = temp_pcap("gzip");
    let mut w = PcapWriter::create(&path);
    w.segment(
        1_000_000,
        (CLIENT, 40003),
        (SERVER, 80),
        1,
        false,
        b"GET /health HTTP/1.1\r\nAccept-Encoding: gzip\r\n\r\n",
    );
    w.segment(
        1_010_000,
        (SERVER, 80),
        (CLIENT, 40003),
        1,
        false,
        &response,
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exchanges.len(), 1);
    let body = exchanges[0]
        .response
        .as_ref()
        .and_then(|r| r.body.as_deref())
        .expect("response body");
    assert_eq!(body, br#"{"status":"healthy"}"#);
}
