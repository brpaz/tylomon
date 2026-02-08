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

```bash
git clone https://github.com/brunopaz/tylomon.git
cd tylomon
cargo build --release
sudo cp target/release/tylomon /usr/local/bin/
```

## Docker

Pull the pre-built image from GitHub Container Registry:

```bash
docker pull ghcr.io/brunopaz/tylomon:latest
```

Multi-architecture images are available for `linux/amd64` and `linux/arm64`.

### Build locally

```bash
docker build -t tylomon:local .
```
