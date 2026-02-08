# Architecture

## Overview

tylomon uses a multi-task architecture built on Tokio. The main thread runs an event loop that receives actions from three concurrent tasks via an `mpsc` channel.

```
┌─────────────┐     ┌──────────────┐     ┌───────────────┐
│  Input Task  │     │ Capture Task │     │ Resolver Task │
│  (crossterm) │     │   (pcap)     │     │  (bollard)    │
└──────┬───────┘     └──────┬───────┘     └───────┬───────┘
       │ KeyPress           │ NewExchange         │ ContainerUpdate
       └────────────┬───────┴─────────────────────┘
                    ▼
            ┌──────────────┐
            │  Event Loop  │
            │  (App state) │
            └──────┬───────┘
                   │ 30fps
                   ▼
            ┌──────────────┐
            │   Ratatui    │
            │  (Terminal)  │
            └──────────────┘
```

## Source tree

```
src/
  main.rs              # Tokio runtime, event loop, task spawning
  lib.rs               # Public module re-exports
  app.rs               # App state, Action/Pane enums, keyboard handling, filtering
  cli.rs               # Clap CLI argument definitions
  error.rs             # Custom Error enum
  capture/
    bridge.rs          # Docker network ID → bridge interface name (br-<12chars>)
    packet.rs          # Raw bytes → ParsedPacket via etherparse
    stream.rs          # TCP stream reassembly with FlowKey tracking
    pipeline.rs        # Capture thread, HTTP extraction, chunked decoding, keep-alive
  docker/
    discovery.rs       # Docker network discovery via bollard
    resolver.rs        # Container IP-to-name resolution
    task.rs            # Async resolver polling task (5s interval)
  http/
    parser.rs          # HTTP request/response parsing via httparse
    types.rs           # HttpExchange, HttpRequest, HttpResponse structs
  tui/
    mod.rs             # Terminal init/restore (crossterm alternate screen)
    event.rs           # Input task — crossterm events → Action channel
    theme.rs           # Color scheme (Theme struct with static Style methods)
    views/
      mod.rs           # Main layout — title bar, split panes, status bar
      exchange_list.rs # Left panel — filtered exchange list with container names
      detail.rs        # Right panel — full request/response with headers and body
      status_bar.rs    # Bottom bar — keybindings, filter state, exchange count
tests/
  http_parser_tests.rs # Integration tests for HTTP parsing
```

## Data pipeline

### 1. Packet capture

The capture task runs pcap on the Docker bridge interface (e.g., `br-a1b2c3d4e5f6`) in a blocking thread via `tokio::task::spawn_blocking`. It filters for TCP traffic only.

### 2. Packet parsing

Each captured packet is parsed using `etherparse` to extract IP addresses, TCP ports, flags, sequence numbers, and payload bytes.

### 3. Stream reassembly

The `StreamReassembler` tracks TCP flows by `(src_ip, src_port, dst_ip, dst_port)` tuples. It accumulates payload bytes per flow direction and handles SYN/FIN/RST lifecycle events.

### 4. HTTP extraction

The pipeline processes reassembled stream data to find HTTP request/response pairs:

- Parses HTTP headers using `httparse`
- Handles `Content-Length` and `Transfer-Encoding: chunked` for body extraction
- Supports keep-alive connections (multiple exchanges per TCP stream)
- Sweeps stale pending requests after 30 seconds

### 5. Container resolution

A separate async task polls the Docker API every 5 seconds to maintain an IP-to-container-name mapping. This runs independently so the capture pipeline is never blocked by Docker API calls.

### 6. Rendering

The main event loop renders the TUI at 30fps using `tokio::time::interval`. Views are pure functions that read from `App` state — they hold no state of their own.

## Key dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Terminal input/output |
| `tokio` | Async runtime |
| `pcap` | Packet capture (libpcap bindings) |
| `etherparse` | Ethernet/IP/TCP packet parsing |
| `httparse` | HTTP request/response parsing |
| `bollard` | Docker Engine API client |
| `clap` | CLI argument parsing |
| `color-eyre` | Error handling and reporting |
| `tracing` | Structured logging |
| `serde` / `serde_json` | Serialization and JSON pretty-printing |
