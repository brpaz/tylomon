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
task ci               # full CI pipeline locally
```

## Architecture

```
src/
  main.rs              # Tokio runtime, event loop, task spawning
  lib.rs               # Public module re-exports (app, capture, cli, docker, error, http, tui)
  app.rs               # App state, Action enum, Pane enum, keyboard handling, filtering
  cli.rs               # Clap CLI args (--project, --network, --log-file, --max-exchanges)
  error.rs             # Custom Error enum (NetworkNotFound, BridgeNotFound, CaptureError, DockerError)
  capture/
    bridge.rs          # bridge_interface_name() — Docker network ID → br-<12chars>
    packet.rs          # parse_packet() — raw bytes → ParsedPacket via etherparse
    stream.rs          # StreamReassembler — TCP flow tracking, sequence reassembly, drain()
    pipeline.rs        # Capture thread (pcap), HTTP extraction, chunked decoding, keep-alive, stale sweep
  docker/
    discovery.rs       # discover_project_networks(), discover_network_by_name() via bollard
    resolver.rs        # ContainerResolver — IP-to-container-name mapping
    task.rs            # Async resolver polling task (5s interval)
  http/
    parser.rs          # parse_request(), parse_response(), get_content_length(), is_chunked()
    types.rs           # HttpExchange, HttpRequest, HttpResponse structs
  tui/
    mod.rs             # Terminal init/restore (crossterm alternate screen)
    event.rs           # Input task — crossterm events → Action channel
    theme.rs           # Theme struct with static Style methods
    views/
      mod.rs           # Main layout — title bar, split panes, status bar
      exchange_list.rs # Left panel — filtered exchange list with container names
      detail.rs        # Right panel — full request/response with headers and body
      status_bar.rs    # Bottom bar — keybindings, filter state, exchange count
tests/
  http_parser_tests.rs # Integration tests for HTTP parsing
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
- Error handling: `color_eyre::eyre::Result` for fallible functions, `Option` for parsing
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
- Views are free functions: `pub fn render(frame: &mut Frame, app: &App, area: Rect, focused: bool)`
- App state is centralized in `App` struct — views are pure renderers with no state
- Keyboard handling: all keys flow through `App::handle_key()`, not the event layer
- Rendering at 30fps via `tokio::time::interval(Duration::from_millis(33))`

### Concurrency Pattern
- Main thread: event loop with `tokio::select!` over action channel + render interval
- Capture: blocking `pcap` in `spawn_blocking` thread, sends `Action::NewExchange` via channel
- Resolver: async polling task, sends `Action::ContainerUpdate` via channel
- Input: async crossterm event stream, sends `Action::KeyPress` via channel

### Git Hooks (lefthook)
- **pre-commit**: `cargo fmt --all -- --check`
- **pre-push**: fmt check + clippy + `cargo test --lib`

### CI (GitHub Actions)
- Check job: fmt, clippy, test
- Build binary: release build, upload artifact
- Docker: build + push to `ghcr.io` on main/tags (multi-arch: amd64 + arm64)
