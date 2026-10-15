# tylomon

Monitor HTTP traffic between Docker containers in real time.

tylomon captures packets on Docker bridge network interfaces, reassembles TCP streams, parses HTTP request/response pairs, and displays them in an interactive terminal dashboard.

## Features

- **Real-time capture** — monitors HTTP traffic on Docker bridge networks as it happens
- **Automatic discovery** — finds Docker Compose project networks automatically
- **Container names** — resolves container IPs to human-readable names
- **Full inspection** — view complete HTTP headers and bodies for both requests and responses
- **Readable bodies** — pretty-printed, syntax-coloured JSON, decoded gzip/brotli, and hex dumps for binary data
- **Chunked decoding** — handles chunked transfer-encoding transparently
- **Keep-alive support** — correctly tracks multiple exchanges on persistent connections
- **Search & filter** — filter exchanges by method, path, or status code
- **Split-pane TUI** — keyboard-driven interface with exchange list and detail view

## Quick start

```bash
# Install and run
cargo install tylomon
sudo tylomon --project my-compose-project
```

Or via Docker:

```bash
docker run --rm -it \
  --cap-add=NET_RAW --cap-add=NET_ADMIN \
  --network host \
  ghcr.io/brunopaz/tylomon:latest \
  --project my-compose-project
```

## Requirements

- Linux (uses bridge interface capture via libpcap)
- Docker daemon running
- `libpcap` installed
- Root or `CAP_NET_RAW` + `CAP_NET_ADMIN` capabilities

## Limitations

- **Plaintext HTTP only** — TLS/HTTPS traffic between containers is not intercepted
- **Linux only** — relies on Docker bridge network interfaces
- **Requires elevated privileges** — needs `CAP_NET_RAW` and `CAP_NET_ADMIN` for packet capture
