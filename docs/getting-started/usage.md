# Usage

## Monitor a Docker Compose project

The most common usage is monitoring traffic within a Docker Compose project:

```bash
sudo tylomon --project my-compose-project
```

tylomon will automatically discover all networks created by the compose project and start capturing on the bridge interface.

## Monitor a specific Docker network

If you want to target a specific Docker network directly:

```bash
sudo tylomon --network my-network-name
```

## Run via Docker

When running as a Docker container, you need host networking and packet capture capabilities:

```bash
docker run --rm -it \
  --cap-add=NET_RAW --cap-add=NET_ADMIN \
  --network host \
  ghcr.io/brunopaz/tylomon:latest \
  --project my-compose-project
```

!!! note
    The `--network host` flag is required so tylomon can access the Docker bridge interfaces on the host.

## Filtering traffic

Once the TUI is running, press `/` to open the search filter. Type a search term and press `Enter` to filter exchanges.

The filter matches against:

- HTTP method (e.g., `GET`, `POST`)
- Request path (e.g., `/api/users`)
- Response status code (e.g., `200`, `404`)

Matching is case-insensitive and substring-based.

Press `q` or `Esc` to clear an active filter.

## Navigating the interface

The interface has two panes:

- **Exchange list** (left) — shows all captured HTTP exchanges
- **Detail view** (right) — shows full headers and body for the selected exchange

Press `Tab` to switch focus between panes. When the detail pane is focused, `j`/`k` scrolls the detail content instead of navigating the list.

See [Keyboard Shortcuts](../reference/keybindings.md) for the full list of keybindings.

## Log file

tylomon writes debug logs to a file (default: `tylomon.log` in the current directory). You can change the path with `--log-file`:

```bash
sudo tylomon --project myapp --log-file /var/log/tylomon.log
```

Set the `RUST_LOG` environment variable to control log verbosity:

```bash
sudo RUST_LOG=tylomon=debug tylomon --project myapp
```
