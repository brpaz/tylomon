# Tylomon — Architecture Plan

A Rust CLI/TUI tool that monitors HTTP traffic between Docker containers in a docker-compose setup.

## How It Works

Point it at a compose project. It discovers the bridge network, captures packets on it, reassembles TCP streams, parses HTTP request/response pairs, and displays them in a real-time Ratatui dashboard where you can inspect headers, bodies, and timing.

```
Docker Compose Project
+---------------------------------------------+
|  +---------+    +---------+    +---------+   |
|  |service-a|<-->|service-b|<-->|service-c|   |
|  +----+----+    +----+----+    +----+----+   |
|       |              |              |        |
|  -----+--------------+--------------+-----   |
|              br-<network-id>                 |
|                     ^                        |
+---------------------|-----------------------+
                      |
              +-------+--------+
              |    tylomon     |
              | (pcap capture) |
              +----------------+
```

## Core Decision: Bridge Interface Capture

**Discovery flow:**
1. Use `bollard` (Docker API client) to list compose project networks
2. Get the network ID -> compute bridge interface name: `br-<first 12 chars of network ID>`
3. Open a pcap capture on that interface with a BPF filter for TCP traffic
4. Parse packets in real-time

**Privileges required:** `CAP_NET_RAW` + `CAP_NET_ADMIN` (or root)

```bash
sudo setcap cap_net_raw,cap_net_admin=eip ./tylomon
```

**Limitations:**
- Works for bridge networks only (default for docker-compose) — not overlay/macvlan
- Cannot see TLS-encrypted payload (shows connection metadata: IPs, ports, SNI from ClientHello)
- Cannot see loopback traffic within a single container

### Why This Over Alternatives

| Approach | Complexity | Privileges | Inter-container | HTTP Reconstruction | TLS Plaintext |
|---|---|---|---|---|---|
| **Bridge capture (chosen)** | Low | NET_RAW | Yes | Manual (TCP reassembly + httparse) | No |
| eBPF TC hooks | High | SYS_ADMIN | Yes | Manual | No (without uprobes) |
| eBPF + TLS uprobes | Very high | SYS_ADMIN | Yes | Manual | Yes |
| Transparent proxy | Medium | NET_ADMIN | Yes | Free (proxy gives HTTP objects) | Yes (with CA injection) |
| Per-container nsenter | Medium | SYS_ADMIN + SYS_PTRACE | Yes + loopback | Manual | No |

### Reference Implementations
- [gprossliner/docker-bridgenw-dump](https://github.com/gprossliner/docker-bridgenw-dump) — computes `br-<shortid>` and runs tcpdump
- [siemens/edgeshark](https://github.com/siemens/edgeshark) — namespace-aware capture with dumpcap
- [eldadru/ksniff](https://github.com/eldadru/ksniff) — `--net=container:<id>` and nsenter patterns

---

## Data Pipeline

```
+---------------+     +---------------+     +----------------+     +---------------+
|  pcap capture |---->|  L2-L4 parse  |---->| TCP stream     |---->| HTTP parse    |
|  (pcap crate) |     | (etherparse)  |     | reassembly     |     | (httparse)    |
|               |     |               |     | (custom)       |     |               |
|  br-<netid>   |     | Extract:      |     | Ordered byte   |     | Parse req +   |
|  BPF filter   |     | - src/dst IP  |     | stream per     |     | response      |
|               |     | - src/dst pt  |     | TCP flow       |     | pairs         |
|               |     | - TCP payload |     | (4-tuple key)  |     |               |
+---------------+     +---------------+     +----------------+     +---------------+
       |                                                                  |
       |              +---------------+                                   |
       +------------->| Container     |<----------------------------------+
                      | resolver      |
                      | (bollard)     |
                      |               |
                      | IP -> name    |
                      | mapping       |
                      +---------------+
                             |
                      +------v--------+
                      |  TUI state    |
                      |  (App model)  |
                      |               |
                      | HttpExchange  |
                      | list + detail |
                      +---------------+
```

### TCP Stream Reassembly

No mature Rust crate exists — build a lightweight custom reassembler:

- **Key**: 4-tuple `(src_ip, src_port, dst_ip, dst_port)`
- Track TCP sequence numbers, reorder out-of-order segments
- Detect connection boundaries (SYN/FIN/RST)
- Feed ordered byte stream into httparse
- Handle keep-alive connections (multiple HTTP exchanges per TCP stream)

MVP simplification: assume packets arrive mostly in order (on a local bridge, they almost always do). Buffer by flow, concatenate payloads by sequence number, flush to HTTP parser on complete request/response.

### HTTP Parsing

Use `httparse` push-parser:
- `Content-Length`: read exactly N bytes after headers
- `Transfer-Encoding: chunked`: parse chunk sizes, concatenate
- No body: HEAD responses, 204, 304

**What you CAN'T see (plaintext-only mode):**
- HTTPS payload (encrypted) — but CAN extract TLS SNI from ClientHello
- HTTP/2 over TLS (h2) — opaque
- gRPC (rides on HTTP/2)

---

## Crate Selection

| Layer | Crate | Purpose | Notes |
|---|---|---|---|
| Packet capture | `pcap` | libpcap bindings | Mature, `capture-stream` feature for async |
| Packet parsing | `etherparse` | Zero-copy L2-L4 | Ethernet -> IP -> TCP without allocations |
| HTTP parsing | `httparse` | Push-parser HTTP/1.1 | No-alloc, handles partial parses |
| Docker API | `bollard` | Container/network discovery | List networks, inspect containers, resolve IPs |
| TUI | `ratatui` + `crossterm` | Terminal UI | De facto standard |
| Async runtime | `tokio` | Event loop + channels | Required by bollard; crossterm EventStream |
| Serialization | `serde` + `serde_json` | HTTP body pretty-print | JSON body inspection |
| CLI args | `clap` | Argument parsing | Standard |
| Error handling | `anyhow` | Error reporting | Ergonomic error handling |
| Logging | `tracing` | Structured logging | File-based (can't log to stdout in TUI) |

---

## TUI Architecture: Action/Message Bus

Best fit for high-volume streaming data:
- Decouples event sources (keyboard, network, docker) from state mutations
- Bounded channels provide natural backpressure
- Render throttling prevents UI overwhelm

### Main Loop

```
+-----------------+   +-----------------+   +-----------------+
| Capture Task    |   | Input Task      |   | Docker Task     |
| (tokio::spawn)  |   | (tokio::spawn)  |   | (tokio::spawn)  |
|                 |   |                 |   |                 |
| pcap -> parse   |   | crossterm       |   | bollard poll    |
| -> HttpExchange |   | EventStream     |   | container list  |
|                 |   |                 |   | IP mappings     |
+-------+---------+   +-------+---------+   +-------+---------+
        | bounded              | bounded             | bounded
        v                      v                     v
+---------------------------------------------------------------+
|                     Main Event Loop                           |
|                                                               |
|  tokio::select! {                                             |
|      action = action_rx.recv() => update_state(action),       |
|      _ = render_interval.tick() => draw(&state),              |
|  }                                                            |
+---------------------------------------------------------------+
```

### Key Parameters

| Concern | Decision | Rationale |
|---|---|---|
| Channel type | `tokio::sync::mpsc::channel(256)` bounded | Backpressure on high volume |
| Render rate | 30 FPS (33ms interval) | Smooth scrolling, not wasteful |
| Tick rate | Eliminated — event-driven only | Render on timer, state on actions |
| State location | Single `App` struct on main task | No locking needed |
| Action enum | `NewExchange, KeyPress, ContainerUpdate, Resize, Quit` | Single type |

### TUI Layout

```
+----------------------------------------------------------+
| tylomon - monitoring: myproject_default (br-a1b2c3d4e5f6) |
+--------------------+-------------------------------------+
| HTTP Exchanges     |  Request Detail                     |
|                    |                                     |
| > GET /api/users   |  GET /api/users HTTP/1.1            |
|   200 OK  12ms     |  Host: service-b:8080               |
|   service-a -> b   |  Content-Type: application/json     |
|                    |  Authorization: Bearer xxx...        |
|   POST /api/order  |                                     |
|   201 Created 45ms |  --- Response ---                   |
|   service-b -> c   |  HTTP/1.1 200 OK                    |
|                    |  Content-Type: application/json     |
|   GET /health      |                                     |
|   200 OK  2ms      |  {"users": [{"id": 1, ...}]}       |
|   external -> a    |                                     |
+--------------------+-------------------------------------+
| [q]uit [f]ilter [c]lear [/]search [tab]switch pane       |
+----------------------------------------------------------+
```

---

## Project Structure

```
tylomon/
├── .envrc
├── .github/
│   └── workflows/
│       └── ci.yml
├── src/
│   ├── main.rs                 # CLI args, tokio runtime bootstrap
│   ├── app.rs                  # App state, Action enum, update logic
│   ├── cli.rs                  # clap argument definitions
│   ├── capture/
│   │   ├── mod.rs              # Capture task entry point
│   │   ├── bridge.rs           # Bridge interface discovery
│   │   ├── packet.rs           # etherparse L2-L4 parsing
│   │   └── stream.rs           # TCP stream reassembly
│   ├── docker/
│   │   ├── mod.rs              # Docker task entry point
│   │   ├── discovery.rs        # Network/container discovery via bollard
│   │   └── resolver.rs         # IP -> container name resolution
│   ├── http/
│   │   ├── mod.rs              # HTTP parsing orchestration
│   │   ├── parser.rs           # httparse request/response parsing
│   │   └── types.rs            # HttpExchange, HttpRequest, HttpResponse
│   ├── tui/
│   │   ├── mod.rs              # Terminal setup/teardown, render loop
│   │   ├── event.rs            # Input event task (crossterm EventStream)
│   │   ├── views/
│   │   │   ├── mod.rs
│   │   │   ├── exchange_list.rs  # Left panel: scrollable exchange list
│   │   │   ├── detail.rs         # Right panel: headers + body
│   │   │   └── status_bar.rs     # Bottom: keybindings, stats
│   │   └── theme.rs            # Colors, styles
│   └── error.rs                # Error types
├── tests/
│   ├── capture_tests.rs
│   └── http_parser_tests.rs
├── fixtures/                   # Sample .pcap files for testing
├── devenv.nix
├── devenv.yaml
├── Taskfile.yml
├── lefthook.yml
├── Dockerfile
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
└── LICENSE
```

---

## Tooling Configuration

### devenv.nix
- Pin Rust via `rust-toolchain.toml` using `languages.rust.toolchainFile`
- System deps: `pkg-config`, `libpcap`
- Dev tools: `go-task`, `lefthook`, `cargo-watch`, `cargo-nextest`
- `enterShell`: auto-install lefthook hooks

### Taskfile.yml
Tasks: `fmt`, `fmt:check`, `clippy`, `test`, `build`, `build:release`, `run`, `watch`, `lint`, `ci`, `docker:build`, `docker:run`, `clean`

### lefthook.yml
- pre-commit: `cargo fmt --check`
- pre-push: clippy + fmt check + tests

### GitHub Actions (ci.yml)
- **check** job: fmt, clippy, test (with `Swatinem/rust-cache@v2`)
- **build-binary** job: release build + upload artifact
- **build-docker** job: multi-platform (amd64/arm64) via buildx, push to GHCR on main push + semver tags

### Dockerfile
- Multi-stage: `rust:1-bookworm` builder -> `debian:bookworm-slim` runtime
- Runtime dep: `libpcap0.8`
- Dependency caching via dummy `src/main.rs` trick

### Docker deployment
- `network_mode: host` (to see bridge interfaces)
- `cap_add: NET_RAW, NET_ADMIN`
- Mount Docker socket read-only for container discovery

---

## Technical Risks & Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| TCP reassembly complexity | Missed/corrupt HTTP exchanges | Start "in-order only" (valid for local bridge). Test with sample pcaps. |
| High packet volume | UI lag, memory growth | Bounded channels (256). Ring buffer for exchanges (keep last N). |
| Container IP changes | Stale name resolution | Poll bollard every 5s. Invalidate on Docker events. |
| libpcap cross-compilation | ARM Docker builds may fail | QEMU-emulated buildx. Ensure libpcap-dev for target arch. |
| Keep-alive connections | Multiple HTTP exchanges per stream | Track HTTP state machine per flow: after response complete, expect next request. |

---

## MVP Milestones

1. **M1 — Skeleton**: Project scaffolding (all config files), `cargo build` works, TUI shows static placeholder layout
2. **M2 — Docker Discovery**: bollard discovers compose networks, resolves bridge interface, lists containers with IPs
3. **M3 — Packet Capture**: Capture on bridge interface, parse L2-L4 with etherparse, display raw packet flow in TUI
4. **M4 — TCP Reassembly**: Group packets by flow (4-tuple), reassemble ordered byte streams
5. **M5 — HTTP Parsing**: Parse HTTP/1.1 request/response pairs with httparse, display structured
6. **M6 — Full TUI**: Exchange list + detail view, filtering, search, keybindings
7. **M7 — Polish**: Body pretty-printing (JSON), timing, export, error handling edge cases

---

## Reference Repositories

| Repo | What to Learn | Key Files |
|---|---|---|
| [domcyrus/rustnet](https://github.com/domcyrus/rustnet) | Packet capture -> DPI -> Ratatui pipeline | `src/app.rs`, `src/network/parser.rs`, `src/network/dpi/http.rs` |
| [pythops/oryx](https://github.com/pythops/oryx) | eBPF (aya) + Ratatui. Future eBPF mode reference. | `oryx-tui/src/ebpf.rs`, `src/event.rs` |
| [imsnif/bandwhich](https://github.com/imsnif/bandwhich) | pnet + ratatui network monitoring. Mature structure. | `src/main.rs`, `src/network/sniffer.rs` |
| [Chleba/netscanner](https://github.com/Chleba/netscanner) | pnet sniffing + ratatui multi-component TUI. | `src/components/packetdump.rs`, `src/components/sniff.rs` |
| [mrjackwills/oxker](https://github.com/mrjackwills/oxker) | bollard + Ratatui. Container discovery reference. | Bounded `mpsc::channel(32)` + Docker polling |
| [robertpsoane/ducker](https://github.com/robertpsoane/ducker) | Bounded channel event loop. Clean action architecture. | `src/events/event_loop.rs` |
| [ratatui/crates-tui](https://github.com/ratatui/crates-tui) | StreamMap event aggregation + Action dispatch. | `src/events.rs`, `src/app.rs` |
| [ClementTsang/bottom](https://github.com/ClementTsang/bottom) | Mature system monitor TUI. Render throttling. | Streaming data display patterns |
