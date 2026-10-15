# AGENTS.md — tylomon

## Project Overview

tylomon is a Rust CLI/TUI tool that monitors plaintext HTTP traffic between Docker containers. It captures packets on Docker bridge interfaces using libpcap, reassembles TCP streams, parses HTTP request/response pairs, and renders them in a Ratatui terminal dashboard.

## Build & Run Commands

```bash
# Development environment (provides Rust, libpcap, task, lefthook, cargo-nextest)
devenv shell

# Build
cargo build                        # debug
cargo build --release --locked     # release

# Run
cargo run -- --project myapp       # monitor compose project
cargo run -- --network mynet       # monitor specific network

# Lint
cargo fmt --all --check            # formatting check
cargo clippy --all-targets --all-features -- -D warnings  # clippy (deny warnings)

# Test — all tests
cargo test --all-features
cargo nextest run                  # via nextest (preferred in CI)

# Test — single test by name
cargo test test_parse_simple_get_request
cargo test decode_chunked          # runs all tests matching substring

# Test — single module
cargo test http::parser::tests
cargo test capture::pipeline::tests

# Test — integration tests only
cargo test --test http_parser_tests

# Docker
docker build -t tylomon:local .
docker run --rm -it --cap-add=NET_RAW --cap-add=NET_ADMIN --network host tylomon:local --project myapp

# Task runner shortcuts
task fmt              # format
task lint             # fmt check + clippy
task test             # nextest
task build:release    # release build
task install          # release build + install (INSTALL_DIR=, SETCAP=false)
task e2e              # capture examples/ stack, assert on exchanges (Docker, jq, sudo)
task ci               # full CI pipeline locally
```

## Architecture

```
src/
  main.rs              # Tokio runtime, target discovery, event loop, headless JSONL mode
  lib.rs               # Public module re-exports
  app.rs               # App state, Action enum, keyboard/mouse handling, selection, filtering
  cli.rs               # Clap CLI args (--project, --network, --write, --read, --no-tui, --har, ...)
  error.rs             # Custom Error enum
  export.rs            # JSONL, HAR and curl serialisation
  filter.rs            # Filter query parser and matcher
  stats.rs             # Per-edge aggregation (service map)
  state.rs             # Remembered picker choice
  capture/
    bridge.rs          # bridge_interface_name() — Docker network ID → br-<12chars>
    packet.rs          # parse_packet() — raw bytes → ParsedPacket via etherparse
    stream.rs          # StreamReassembler — TCP flow tracking, sequence reassembly, drain()
    pipeline.rs        # Capture/replay threads, HTTP extraction, pcap Recorder, stale sweep
    postgres.rs        # PostgreSQL wire protocol decoder (PgTracker), emits Protocol::Postgres exchanges
  docker/
    compose.rs         # Compose file / COMPOSE_FILE project detection
    discovery.rs       # Target resolution, network discovery via bollard
    resolver.rs        # ContainerResolver — IP-to-container-name mapping
    task.rs            # Async resolver polling task (5s interval)
    watch.rs           # NetworkWatcher — one capture per network, follows changes
  http/
    parser.rs          # parse_request(), parse_response(), get_content_length(), is_chunked()
    encoding.rs        # gzip / deflate / brotli decoding
    types.rs           # HttpExchange, HttpRequest, HttpResponse structs
  tui/
    mod.rs             # Terminal init/restore (alternate screen, mouse capture)
    event.rs           # Input task — crossterm events → Action channel
    picker.rs          # Interactive target selection
    rows.rs            # Detail rows and text selection
    theme.rs           # Theme struct with static Style methods
    views/             # exchange_list, detail, body, service_map, help, status_bar, layout in mod.rs
tests/
  http_parser_tests.rs # Integration tests for HTTP parsing
  replay_tests.rs      # Pipeline tests over generated pcap files
scripts/
  e2e.sh               # End-to-end check against examples/ (Docker, jq, sudo)
```

## Code Style

### Rust Edition & Toolchain
- Edition: **2024** (Cargo.toml)
- Toolchain: **stable** (rust-toolchain.toml)
- Components: rustfmt, clippy

### Formatting
- Standard `cargo fmt` (no rustfmt.toml overrides)
- Clippy with `-D warnings` — zero warnings policy

### Imports
- Group by: std → external crates → crate-internal (`use crate::...`)
- Blank line between groups
- Use block imports only when grouping from same parent: `use crossterm::{execute, terminal::{...}};`
- Scoped imports allowed inside function bodies for rarely-used types (see `discovery.rs` line 21)

### Naming
- Types: `PascalCase` — `FlowKey`, `HttpExchange`, `StreamReassembler`
- Functions/methods: `snake_case` — `parse_packet`, `bridge_interface_name`
- Constants: `SCREAMING_SNAKE_CASE` — `PENDING_REQUEST_TIMEOUT`, `STALE_SWEEP_INTERVAL`
- Enum variants: `PascalCase` — `Action::NewExchange`, `Pane::Detail`

### Types & Patterns
- Error handling: `anyhow::Result` for fallible functions, `Option` for parsing
- Custom errors: `Error` enum in `error.rs` with `Display` and `std::error::Error` impls
- Ad-hoc errors: `eyre!("message")` for one-off failures
- Async: `tokio` runtime, `mpsc::channel` for inter-task communication
- Logging: `tracing` macros (`info!`, `warn!`, `debug!`) — logs to file, not stdout
- Serialization: `serde` derive on data types, `serde_json` for body pretty-printing
- No `unwrap()` in production code — use `?`, `.ok()`, or `unwrap_or_else`
- No `as any`, `@ts-ignore` equivalents — no type suppression

### Struct Style
- All fields `pub` on data structs (HttpExchange, ParsedPacket, FlowKey, etc.)
- Doc comments (`///`) on public types and functions
- Derive order: `Debug, Clone` minimum; add `Serialize, Deserialize` for data types; `Hash, Eq, PartialEq` for keys

### Testing
- Unit tests: `#[cfg(test)] mod tests` inside the source file
- Integration tests: `tests/` directory, import via `use tylomon::...`
- Assertions: `assert_eq!`, `assert!`, `.expect("should parse")` for Option/Result unwrapping in tests
- Dev dependency: `pretty_assertions` available but not required

### TUI Patterns
- `Theme` struct with static methods returning `Style` — no constants
- Views are free functions: `pub fn render(frame: &mut Frame, app: &mut App, area: Rect, focused: bool)`
- App state is centralized in `App` struct — views render from it and only write back layout facts for mouse hit-testing and the detail row cache
- Keyboard handling: all keys flow through `App::handle_key()`, not the event layer; mouse goes through `App::update()`
- New key bindings must be added to `views/help.rs`, `docs/reference/keybindings.md` and the README
- Rendering at 30fps via `tokio::time::interval(Duration::from_millis(33))`

### Concurrency Pattern
- Main thread: event loop with `tokio::select!` over action channel + render interval
- Capture: one non-blocking `pcap` thread per bridge, owned by `NetworkWatcher`, sends `Action::NewExchange` via channel
- Replay (`--read`): one thread reads a pcap file through the same pipeline
- Resolver: async polling task, sends `Action::ContainerUpdate` via channel
- Input: async crossterm event stream, sends `Action::KeyPress` via channel

### Git Hooks (lefthook)
- **pre-commit**: `cargo fmt --all -- --check`
- **pre-push**: fmt check + clippy + `cargo test --lib`

### CI (GitHub Actions)
- Check job: fmt, clippy, test
- Build binary: release build, upload artifact
- Docker: build + push to `ghcr.io` on main/tags (multi-arch: amd64 + arm64)
