# Architecture

## Overview

tylomon is a Tokio application with a single event loop. Everything that produces data runs in its own task or thread and sends `Action` values to the loop over an `mpsc` channel. The loop updates the `App` state and redraws the terminal at 30fps.

```
┌────────────┐  ┌─────────────────────────┐  ┌───────────────┐  ┌────────────┐
│ Input task │  │ Network watcher         │  │ Resolver task │  │  Replay    │
│ (crossterm)│  │  └ capture thread / bridge│  │  (bollard)    │  │  thread    │
└─────┬──────┘  │    (pcap)               │  └───────┬───────┘  │ (--read)   │
      │         └────────────┬────────────┘          │          └─────┬──────┘
      │ KeyPress, Mouse*     │ NewExchange,          │ ContainerUpdate │ NewExchange
      │                      │ CaptureStarted/Error  │                 │
      └──────────────┬───────┴───────────────────────┴─────────────────┘
                     ▼
             ┌──────────────┐        ┌────────────────────────────┐
             │  Event loop  │ ─────▶ │ --no-tui: JSON Lines, HAR   │
             │  (App state) │        └────────────────────────────┘
             └──────┬───────┘
                    │ 30fps
                    ▼
             ┌──────────────┐
             │   Ratatui    │
             └──────────────┘
```

With `--read` there is no watcher, resolver or Docker connection: only the replay thread feeds the loop. With `--no-tui` the input task is not started and the loop prints each exchange instead of drawing.

## Source tree

```
src/
  main.rs              # Runtime, target discovery, event loop, headless JSONL mode
  lib.rs               # Public module re-exports
  app.rs               # App state, Action enum, keyboard and mouse handling
  cli.rs               # Clap CLI argument definitions
  error.rs             # Custom Error enum
  export.rs            # JSONL, HAR and curl serialisation
  filter.rs            # Filter query parser and matcher
  stats.rs             # Per-edge aggregation for the service map
  state.rs             # Remembered picker choice (XDG state directory)
  capture/
    bridge.rs          # Docker network ID → bridge interface name (br-<12chars>)
    packet.rs          # Raw bytes → ParsedPacket via etherparse
    stream.rs          # TCP stream reassembly with FlowKey tracking
    pipeline.rs        # Capture/replay threads, HTTP extraction, pcap recording
    postgres.rs        # PostgreSQL wire protocol decoder (PgTracker)
  docker/
    compose.rs         # Compose file and COMPOSE_FILE project detection
    discovery.rs       # Network discovery, Target resolution (bollard)
    resolver.rs        # Container IP-to-name resolution
    task.rs            # Async resolver polling task (5s interval)
    watch.rs           # NetworkWatcher: one capture per network, follows changes
  http/
    parser.rs          # HTTP request/response parsing via httparse
    encoding.rs        # gzip / deflate / brotli decoding
    types.rs           # HttpExchange, HttpRequest, HttpResponse structs
  tui/
    mod.rs             # Terminal init/restore (alternate screen, mouse capture)
    event.rs           # Input task — crossterm events → Action channel
    picker.rs          # Interactive target selection
    rows.rs            # Detail rows, text selection, highlighting
    theme.rs           # Colour scheme (static Style methods)
    views/
      mod.rs           # Layout — title, error banner, panes, status bar, help
      exchange_list.rs # Left pane — filtered, sorted exchange list
      detail.rs        # Right pane — pinned summary, headers, bodies
      body.rs          # Body rendering: coloured JSON, text, hex dump
      service_map.rs   # Per-edge table
      help.rs          # Key reference popup
      status_bar.rs    # Bottom bar — mode indicators, hints, counts
tests/
  http_parser_tests.rs # HTTP parsing
  replay_tests.rs      # Pipeline tests over generated pcap files
scripts/
  e2e.sh               # End-to-end check against examples/
```

## Data pipeline

### 1. Target resolution

A `Target` (a Compose project or a single network) is resolved to Docker networks. It comes from `--project`, `--network`, or the picker, which lists the Compose project found in the working directory plus bridge networks that have containers. Each network maps to a Linux bridge interface (`br-<first 12 chars of the ID>`, or the name in `com.docker.network.bridge.name`, such as `docker0`).

### 2. Capture

The `NetworkWatcher` starts one capture thread per network. Each thread opens the bridge with libpcap in non-blocking mode, filters TCP, and polls every 10 ms so it can always be stopped. Every 5 seconds the watcher re-resolves the target: networks that were recreated get a new capture on their new bridge, removed networks are dropped, and new networks of a project are added. Captures that ended on their own are restarted, except those that failed for lack of privileges.

With `--write`, every thread also writes its packets to one shared pcap file. With `--read`, a replay thread reads a pcap file through the same pipeline instead.

### 3. Packet parsing and reassembly

`etherparse` extracts IP addresses, TCP ports, flags, sequence numbers and payload. The `StreamReassembler` accumulates payload bytes per flow direction and handles the SYN/FIN/RST lifecycle.

### 4. HTTP extraction

The pipeline finds request/response pairs in the reassembled data:

- Parses headers with `httparse`
- Reads bodies by `Content-Length` or `Transfer-Encoding: chunked`
- Decodes `Content-Encoding` gzip, deflate and brotli (output capped at 16 MiB)
- Supports keep-alive connections (several exchanges per stream)
- Measures response time from packet timestamps, so live capture and replay agree
- Flushes requests with no response after 30 seconds (or at the end of a replayed file) as unanswered

### 5. Other protocols

Before trying HTTP, the pipeline asks the `PgTracker` whether a stream is PostgreSQL. A flow is recognised by its first bytes (a protocol 3.0 startup, SSL, GSSAPI or cancel request) and then followed in both directions as a connection: the tracker parses typed messages, remembers prepared statements and portals, and pairs each `Query` or `Execute` with the server messages that answer it (`RowDescription`, `DataRow`, `CommandComplete`, `ErrorResponse`, `ReadyForQuery`). Finished queries are emitted as exchanges with `Protocol::Postgres`, reusing the HTTP structures: the verb is the method, the SQL is the path and body, and the status is 200 or 500. Connections that switch to TLS are reported once and not decoded. Unanswered queries are flushed after 30 seconds like HTTP requests.

### 6. Container resolution

A separate async task re-resolves the target and polls the Docker API every 5 seconds to maintain an IP-to-container-name mapping, so the capture threads are never blocked by Docker calls.

### 7. State and rendering

`App` owns all state: exchanges (each with a unique id), the selected id, filter, sort, view mode, split width, zoom, text selection, and the cached detail rows for the selected exchange. Selection is tracked by exchange id, so it survives new traffic, sorting and filtering.

Views render from `App` at 30fps. The few things they write back are layout facts needed for mouse hit-testing (pane areas, scroll offsets) and the detail cache.

Capture failures are kept per interface in `App` and shown as a banner until that capture runs again or its network goes away.

### 8. Output

In the TUI, `e` writes the listed exchanges to a HAR file and `3` copies a `curl` command. In headless mode each completed exchange is written to stdout as one JSON object, and `--har` writes everything captured on exit.

## Key dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Terminal input/output and mouse events |
| `tokio` | Async runtime |
| `pcap` | Packet capture, recording and replay (libpcap bindings) |
| `etherparse` | Ethernet/IP/TCP packet parsing |
| `httparse` | HTTP request/response parsing |
| `flate2`, `brotli` | Body decompression |
| `bollard` | Docker Engine API client |
| `clap` | CLI argument parsing |
| `serde`, `serde_json`, `serde_yaml_ng` | Serialisation, JSON bodies, Compose `name:` |
| `base64` | Binary bodies in JSONL and HAR |
| `arboard` | Clipboard (X11 and Wayland) |
| `anyhow` | Error handling |
| `tracing` | Structured logging to a file |
