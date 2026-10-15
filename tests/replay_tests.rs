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

fn pg_message(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut message = vec![kind];
    message.extend((payload.len() as u32 + 4).to_be_bytes());
    message.extend(payload);
    message
}

fn pg_cstr(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

#[test]
fn replay_decodes_a_postgres_session() {
    let mut ssl_request = 8u32.to_be_bytes().to_vec();
    ssl_request.extend(80_877_103u32.to_be_bytes());

    let mut startup_body = 196_608u32.to_be_bytes().to_vec();
    for text in ["user", "app", "database", "shop", ""] {
        startup_body.extend(if text.is_empty() {
            vec![0]
        } else {
            pg_cstr(text)
        });
    }
    let mut startup = (startup_body.len() as u32 + 4).to_be_bytes().to_vec();
    startup.extend(startup_body);

    let mut row_description = 1i16.to_be_bytes().to_vec();
    row_description.extend(pg_cstr("name"));
    row_description.extend([0; 4]);
    row_description.extend([0; 2]);
    row_description.extend(25i32.to_be_bytes());
    row_description.extend((-1i16).to_be_bytes());
    row_description.extend((-1i32).to_be_bytes());
    row_description.extend([0; 2]);

    let mut data_row = 1i16.to_be_bytes().to_vec();
    data_row.extend(3i32.to_be_bytes());
    data_row.extend(b"ana");

    let path = temp_pcap("postgres");
    let mut w = PcapWriter::create(&path);
    let client = (CLIENT, 40010);
    let server = (SERVER, 5432);

    w.segment(1_000_000, client, server, 1, false, &ssl_request);
    w.segment(1_001_000, server, client, 1, false, b"N");
    w.segment(1_002_000, client, server, 9, false, &startup);
    w.segment(
        1_030_000,
        server,
        client,
        2,
        false,
        &[
            pg_message(b'R', &0i32.to_be_bytes()),
            pg_message(b'Z', b"I"),
        ]
        .concat(),
    );
    w.segment(
        2_000_000,
        client,
        server,
        100,
        false,
        &pg_message(b'Q', &pg_cstr("SELECT name FROM users")),
    );
    w.segment(
        2_007_000,
        server,
        client,
        50,
        false,
        &[
            pg_message(b'T', &row_description),
            pg_message(b'D', &data_row),
            pg_message(b'C', &pg_cstr("SELECT 1")),
            pg_message(b'Z', b"I"),
        ]
        .concat(),
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exchanges.len(), 2);
    assert_eq!(exchanges[0].request.method, "CONNECT");
    assert_eq!(exchanges[0].request.path, "app@shop");

    let query = &exchanges[1];
    assert_eq!(query.protocol, tylomon::http::types::Protocol::Postgres);
    assert_eq!(query.request.method, "SELECT");
    assert_eq!(query.request.path, "SELECT name FROM users");
    assert_eq!(query.dst_port, 5432);
    assert_eq!(query.duration_ms, Some(7));
    let response = query.response.as_ref().expect("response");
    assert_eq!(response.reason, "SELECT 1");
    let table = String::from_utf8(response.body.clone().expect("body")).expect("utf8");
    assert!(table.contains("ana"));
}

fn h2_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut frame = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags];
    frame.extend(stream.to_be_bytes());
    frame.extend(payload);
    frame
}

fn h2_headers(encoder: &mut fluke_hpack::Encoder, headers: &[(&str, &str)]) -> Vec<u8> {
    encoder.encode(headers.iter().map(|(n, v)| (n.as_bytes(), v.as_bytes())))
}

fn grpc_message(payload: &[u8]) -> Vec<u8> {
    let mut message = vec![0];
    message.extend((payload.len() as u32).to_be_bytes());
    message.extend(payload);
    message
}

#[test]
fn replay_decodes_a_grpc_call() {
    const END_STREAM: u8 = 1;
    const END_HEADERS: u8 = 4;

    let path = temp_pcap("grpc");
    let mut w = PcapWriter::create(&path);
    let client = (CLIENT, 40020);
    let server = (SERVER, 9000);
    let mut client_encoder = fluke_hpack::Encoder::new();
    let mut server_encoder = fluke_hpack::Encoder::new();

    // The preface is often sent in a segment of its own.
    w.segment(
        1_000_000,
        client,
        server,
        1,
        false,
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
    );
    w.segment(
        1_000_100,
        client,
        server,
        25,
        false,
        &h2_frame(4, 0, 0, &[]),
    );
    w.segment(1_000_200, server, client, 1, false, &h2_frame(4, 0, 0, &[]));

    let request = h2_headers(
        &mut client_encoder,
        &[
            (":method", "POST"),
            (":scheme", "http"),
            (":authority", "greeter:9000"),
            (":path", "/helloworld.Greeter/SayHello"),
            ("content-type", "application/grpc"),
            ("te", "trailers"),
        ],
    );
    // Message: field 1 = "world".
    let message = grpc_message(&[0x0a, 5, b'w', b'o', b'r', b'l', b'd']);
    w.segment(
        2_000_000,
        client,
        server,
        34,
        false,
        &[
            h2_frame(1, END_HEADERS, 1, &request),
            h2_frame(0, END_STREAM, 1, &message),
        ]
        .concat(),
    );

    let headers = h2_headers(
        &mut server_encoder,
        &[(":status", "200"), ("content-type", "application/grpc")],
    );
    let trailers = h2_headers(&mut server_encoder, &[("grpc-status", "0")]);
    let reply = grpc_message(&[
        0x0a, 11, b'H', b'e', b'l', b'l', b'o', b' ', b'w', b'o', b'r', b'l', b'd',
    ]);
    w.segment(
        2_006_000,
        server,
        client,
        10,
        false,
        &[
            h2_frame(1, END_HEADERS, 1, &headers),
            h2_frame(0, 0, 1, &reply),
            h2_frame(1, END_HEADERS | END_STREAM, 1, &trailers),
        ]
        .concat(),
    );
    w.finish();

    let exchanges = replay(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(exchanges.len(), 1);
    let call = &exchanges[0];
    assert_eq!(call.protocol, tylomon::http::types::Protocol::Grpc);
    assert_eq!(call.request.path, "/helloworld.Greeter/SayHello");
    assert_eq!(
        call.request.body.as_deref(),
        Some(b"1: \"world\"".as_slice())
    );
    let response = call.response.as_ref().expect("response");
    assert_eq!(
        (response.status_code, response.reason.as_str()),
        (200, "OK")
    );
    assert_eq!(
        response.body.as_deref(),
        Some(b"1: \"Hello world\"".as_slice())
    );
    assert_eq!(call.duration_ms, Some(6));
}
