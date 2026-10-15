# Installation

## From source

### Prerequisites

- Rust toolchain (stable)
- `libpcap` development headers
- `pkg-config`

### Install system dependencies

=== "Debian / Ubuntu"

    ```bash
    sudo apt-get install libpcap-dev pkg-config
    ```

=== "Fedora / RHEL"

    ```bash
    sudo dnf install libpcap-devel pkg-config
    ```

=== "Arch Linux"

    ```bash
    sudo pacman -S libpcap pkgconf
    ```

### Build and install

With [Task](https://taskfile.dev/):

```bash
git clone https://github.com/brpaz/tylomon.git
cd tylomon
task install
```

This builds the release binary, installs it to `~/.local/bin` and grants the [packet capture capabilities](#packet-capture-permissions). Make sure that directory is on your `PATH`. Options:

| Variable | Default | Effect |
|----------|---------|--------|
| `INSTALL_DIR` | `~/.local/bin` | Install somewhere else, e.g. `task install INSTALL_DIR=/usr/local/bin` |
| `SETCAP` | `true` | `task install SETCAP=false` skips granting the capabilities |

`sudo` is used only when the install directory is not writable and for `setcap`. `task uninstall` removes the binary.

Without Task:

```bash
git clone https://github.com/brpaz/tylomon.git
cd tylomon
cargo build --release
sudo cp target/release/tylomon /usr/local/bin/
sudo setcap cap_net_raw,cap_net_admin=eip /usr/local/bin/tylomon
```

## Docker

Pull the pre-built image from GitHub Container Registry:

```bash
docker pull ghcr.io/brpaz/tylomon:latest
```

Multi-architecture images are available for `linux/amd64` and `linux/arm64`.

### Build locally

```bash
docker build -t tylomon:local .
```

## Packet capture permissions

Capturing needs `CAP_NET_RAW` and `CAP_NET_ADMIN`. Grant them to the binary instead of running under `sudo`, which drops your desktop session (clipboard) and Compose environment variables:

```bash
sudo setcap cap_net_raw,cap_net_admin=eip /usr/local/bin/tylomon
```

Re-run the command after every rebuild or upgrade; `task install` does it for you. The user running tylomon also needs access to the Docker socket.
