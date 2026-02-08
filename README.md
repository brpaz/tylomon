# tylomon

![project logo](./docs/assets/logo.webp)

> Monitor HTTP traffic between Docker containers in real time, directly from the terminal.

> [!WARNING]  
> This project was done to solve a personal itch and it was completely Vibe Coded using Claude Opus 4.6. Use it at your own risk.

## About

tylomon captures packets on Docker bridge network interfaces, reassembles TCP streams, parses HTTP request/response pairs, and displays them in an interactive terminal dashboard.

## Features

- Real-time HTTP traffic capture on Docker bridge networks
- Automatic Docker Compose project and network discovery
- Container name resolution (shows container names instead of IPs)
- Full HTTP header and body inspection
- JSON body pretty-printing
- Chunked transfer-encoding decoding
- Keep-alive connection support
- Search and filter by method, path, or status code
- Keyboard-driven TUI with split-pane navigation

## Getting Started

### Requirements

- Linux (uses bridge interface capture via libpcap)
- Docker daemon running
- `libpcap` installed
- Root or `CAP_NET_RAW` + `CAP_NET_ADMIN` capabilities

### Installation

#### From source

```bash
# Install system dependency
sudo apt-get install libpcap-dev   # Debian/Ubuntu
sudo dnf install libpcap-devel     # Fedora/RHEL

# Build
cargo build --release

# Install
sudo cp target/release/tylomon /usr/local/bin/
```

#### Docker

```bash
docker pull ghcr.io/brunopaz/tylomon:latest
```

## Usage

### Monitor a Docker Compose project

```bash
sudo tylomon --project my-compose-project
```

### Monitor a specific Docker network

```bash
sudo tylomon --network my-network-name
```

### Run via Docker

```bash
docker run --rm -it \
  --cap-add=NET_RAW --cap-add=NET_ADMIN \
  --network host \
  ghcr.io/brunopaz/tylomon:latest \
  --project my-compose-project
```

### Options

```
Usage: tylomon [OPTIONS]

Options:
  -p, --project <PROJECT>              Docker compose project name to monitor
  -n, --network <NETWORK>              Docker network name to capture on
      --log-file <LOG_FILE>            Log file path [default: tylomon.log]
      --max-exchanges <MAX_EXCHANGES>  Maximum exchanges to keep in memory [default: 1000]
  -h, --help                           Print help
  -V, --version                        Print version
```

## Keyboard shortcuts

| Key       | Action                                        |
|-----------|-----------------------------------------------|
| `j` / `k` | Navigate exchange list (or scroll detail pane) |
| `Tab`     | Switch focus between list and detail panes    |
| `/`       | Open search filter                            |
| `Enter`   | Apply filter (in filter mode)                 |
| `Esc`     | Cancel filter / clear active filter           |
| `q`       | Clear active filter, or quit                  |
| `c`       | Clear all exchanges                           |
| `g` / `G` | Jump to first / last exchange                 |
| `PgUp/Dn` | Page through list or detail                  |
| `Ctrl+C`  | Quit                                          |

## Development

### Prerequisites

The project uses [devenv.sh](https://devenv.sh/) to manage the development environment. With devenv installed:

```bash
devenv shell
```

This provides Rust toolchain, libpcap, [Task](https://taskfile.dev/), [Lefthook](https://github.com/evilmartians/lefthook), cargo-watch, and cargo-nextest.

### Common tasks

```bash
task fmt          # Format code
task lint         # Run fmt check + clippy
task test         # Run tests
task build        # Build debug binary
task build:release # Build release binary
task run -- --project myapp  # Run in dev mode
task watch        # Watch and rebuild on changes
task ci           # Run full CI pipeline locally
task docker:build # Build Docker image locally
```

### Git hooks

[Lefthook](https://github.com/evilmartians/lefthook) is configured with:

- **pre-commit**: format check
- **pre-push**: format check, clippy, tests

Hooks are installed automatically when entering the devenv shell.

## Architecture

```
src/
  main.rs           Entry point, event loop
  app.rs            Application state, keyboard handling, filtering
  cli.rs            CLI argument parsing (clap)
  capture/
    bridge.rs       Docker bridge interface discovery
    packet.rs       Ethernet/IP/TCP packet parsing (etherparse)
    stream.rs       TCP stream reassembly
    pipeline.rs     Capture thread, HTTP extraction, chunked decoding
  docker/
    discovery.rs    Docker network/container discovery (bollard)
    resolver.rs     Container IP-to-name resolution
    task.rs         Async resolver polling
  http/
    parser.rs       HTTP request/response parsing (httparse)
    types.rs        HttpExchange, HttpRequest, HttpResponse types
  tui/
    event.rs        Terminal input handling
    theme.rs        Color scheme
    views/          TUI panels (exchange list, detail, status bar)
```

## How it works

1. Discovers the Docker bridge network interface for the target compose project or network name
2. Opens a pcap capture on that bridge interface filtering TCP traffic
3. Reassembles TCP streams and parses HTTP request/response pairs
4. Resolves container IPs to names via the Docker API
5. Displays everything in a Ratatui terminal dashboard at 30fps

## Limitations

- **Plaintext HTTP only** -- TLS/HTTPS traffic between containers is not intercepted
- **Linux only** -- relies on Docker bridge network interfaces
- **Requires elevated privileges** -- needs `CAP_NET_RAW` and `CAP_NET_ADMIN` for packet capture

## Documentation

Full documentation is available at [https://brunopaz.github.io/tylomon](https://brunopaz.github.io/tylomon).

## License

[MIT](LICENSE)
