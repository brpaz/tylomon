# tylomon

Monitor HTTP traffic between Docker containers in real time.

tylomon captures packets on Docker bridge network interfaces, reassembles TCP streams, parses HTTP request/response pairs, and displays them in an interactive terminal dashboard.

!!! warning "AI disclosure"
    tylomon is vibe coded: the code, tests and documentation were written by AI coding assistants under human direction. The first version was written with Claude Opus 4.6; later work, including most features, was done with Claude Sonnet 5.5 in Claude Code. There are automated tests and an end-to-end check against a sample Docker stack, but no independent code review or security audit. tylomon captures network traffic and needs elevated capabilities (`CAP_NET_RAW`, `CAP_NET_ADMIN`), so read the code before running it on a machine or network you care about. Use it at your own risk.

## Features

- **Real-time capture** — monitors HTTP traffic on Docker bridge networks as it happens
- **Target picker** — run `tylomon` in a Compose project directory (`compose.yaml`, `docker-compose.yml` or `COMPOSE_FILE`) and choose the project or any bridge network from a list
- **Follows your stack** — captures every network of a project and re-attaches when networks are recreated or added
- **Container names** — resolves container IPs to human-readable names
- **Full inspection** — complete HTTP headers and bodies for requests and responses, in a detail pane with a pinned summary
- **Readable bodies** — pretty-printed, syntax-coloured JSON, decoded gzip/deflate/brotli and chunked bodies, and hex dumps for binary data
- **Filters and sorting** — `status:5xx dst:api >500ms -path:/health`, and sort by response time
- **Service map** — requests, errors and p50/p95 latency per source and destination
- **Record and replay** — save traffic to a pcap file and open it later without root or Docker
- **Export and scripting** — JSON Lines on stdout (`--no-tui`), HAR files (`--har`), copy as `curl`
- **Keyboard and mouse** — click, double-click, wheel, drag to resize panes, drag to select and copy text, zoom a pane full screen

## Quick start

```bash
# From a directory with a compose file: pick a target from the list
sudo tylomon

# Or name it
sudo tylomon --project my-compose-project
```

Try it with the sample stack in the repository:

```bash
cd examples && docker compose up -d && sudo tylomon
```

Or via Docker:

```bash
docker run --rm -it \
  --cap-add=NET_RAW --cap-add=NET_ADMIN \
  --network host \
  ghcr.io/brpaz/tylomon:latest \
  --project my-compose-project
```

## Requirements

- Linux (uses bridge interface capture via libpcap)
- Docker daemon running
- `libpcap` installed
- Root or `CAP_NET_RAW` + `CAP_NET_ADMIN` capabilities for live capture ([how to grant them](getting-started/installation.md#packet-capture-permissions)). Replaying a pcap file needs neither

## Limitations

- **Plaintext HTTP/1.x only** — TLS/HTTPS and HTTP/2 traffic between containers is not decoded
- **Linux only** — relies on Docker bridge network interfaces
- **Requires elevated privileges** — needs `CAP_NET_RAW` and `CAP_NET_ADMIN` for live capture
