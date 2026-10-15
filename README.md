# tylomon

<div align="center">
  
![project logo](./docs/assets/logo.webp)

</div>

> Monitor HTTP traffic between Docker containers in real time, directly from the terminal.

> [!WARNING]  
> This project was fully Vibecoded by Opus 4.6. It was done as a way to experiment with AI workflows and to solve a specific itch of mine. Use it at your own risk!

## About

tylomon captures packets on Docker bridge network interfaces, reassembles TCP streams, parses HTTP request/response pairs, and displays them in an interactive terminal dashboard.

## Features

- Real-time HTTP traffic capture on Docker bridge networks
- Automatic Docker Compose project and network discovery; captures follow networks that are recreated or added while running
- Container name resolution (shows container names instead of IPs)
- Full HTTP header and body inspection
- Detail pane with a pinned summary, syntax-coloured JSON bodies and hex dumps for binary data
- Chunked transfer-encoding decoding and gzip / deflate / brotli body decoding
- Keep-alive connection support
- Structured filters (`status:5xx`, `dst:api`, `>500ms`, negation) and sort by response time
- Record traffic to pcap (`--write`) and replay it later without root or Docker (`--read`)
- JSON Lines streaming (`--no-tui`) and HAR export (`--har`, or `e` in the TUI)
- Service map (`m`): requests, errors and p50/p95 latency per source→destination
- Keyboard and mouse driven TUI: click or double-click to select and open, wheel to scroll, drag to select and copy text in the detail pane

## Getting Started

### Requirements

- Linux (uses bridge interface capture via libpcap)
- Docker daemon running
- `libpcap` installed
- Root or `CAP_NET_RAW` + `CAP_NET_ADMIN` capabilities. Prefer granting the capabilities over `sudo`, which can hide your Wayland/X11 session (clipboard) and Compose environment:

  ```bash
  sudo setcap cap_net_raw,cap_net_admin=eip /usr/local/bin/tylomon
  ```

  Re-run `setcap` after every rebuild. When developing, `task run` does it for you.

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
docker pull ghcr.io/brpaz/tylomon:latest
```

## Usage

### Pick a target interactively

```bash
cd my-project && sudo tylomon
```

Without `--project` or `--network`, tylomon lists the Compose project found in the current directory (`compose.yaml`, `compose.yml`, `docker-compose.yaml` or `docker-compose.yml`, or the files in `COMPOSE_FILE`) plus every Docker bridge network that has containers. Use `j`/`k` and `Enter` to select, `/` to filter. The last choice is preselected next time. Selecting a Compose project captures on all of its networks.

A sample project to try it out lives in [`examples/`](examples/):

```bash
cd examples && docker compose up -d && sudo tylomon
```

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
  ghcr.io/brpaz/tylomon:latest \
  --project my-compose-project
```

### Options

```
Usage: tylomon [OPTIONS]

Options:
  -p, --project <PROJECT>              Docker compose project name to monitor
  -n, --network <NETWORK>              Docker network name to capture on
  -w, --write <FILE>                   Record captured packets to a pcap file
  -r, --read <FILE>                    Replay a pcap file instead of capturing live
      --no-tui                         Write exchanges to stdout as JSON Lines instead of showing the TUI
      --har <FILE>                     Save all captured exchanges to a HAR file on exit
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
| `?`       | Show all keys                                 |
| `f`       | Follow the newest exchange                    |
| `s`       | Toggle sort: capture order / slowest first    |
| `z`       | Zoom the focused pane full screen             |
| `< / >`   | Resize the list pane (or drag the border)     |
| `m`       | Toggle service map                            |
| `3`       | Copy selected request as a `curl` command     |
| `e`       | Export listed exchanges to a HAR file         |
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
task run -- --project myapp  # Build, grant capabilities, run
task watch        # Watch and rebuild on changes
task e2e          # Capture the examples/ stack and assert on the exchanges
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
  export.rs         JSONL and HAR serialisation
  stats.rs          Per-edge traffic aggregation (service map)
  filter.rs         Exchange filter query parsing and matching
  capture/
    bridge.rs       Docker bridge interface discovery
    packet.rs       Ethernet/IP/TCP packet parsing (etherparse)
    stream.rs       TCP stream reassembly
    pipeline.rs     Capture/replay threads, HTTP extraction, chunked decoding, pcap recording
  docker/
    compose.rs      Compose file / COMPOSE_FILE project detection
    discovery.rs    Docker network/container discovery (bollard)
    resolver.rs     Container IP-to-name resolution
    task.rs         Async resolver polling
    watch.rs        Re-attaches captures when networks are recreated or added
  http/
    parser.rs       HTTP request/response parsing (httparse)
    types.rs        HttpExchange, HttpRequest, HttpResponse types
  tui/
    event.rs        Terminal input handling
    picker.rs       Interactive target selection list
    theme.rs        Color scheme
    rows.rs         Detail rows and text selection
    views/          TUI panels (exchange list, detail, body rendering, help, service map, status bar)
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

Full documentation is available at [https://brpaz.github.io/tylomon](https://brpaz.github.io/tylomon).

## License

[MIT](LICENSE)
