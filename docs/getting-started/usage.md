# Usage

## Pick a target interactively

Run tylomon without arguments to choose what to monitor:

```bash
cd my-project
sudo tylomon
```

The list contains:

- the Compose project in the current directory, marked `[compose]`. It is found from `compose.yaml`, `compose.yml`, `docker-compose.yaml` or `docker-compose.yml`, or from the files listed in `COMPOSE_FILE` (separated by `:` or `COMPOSE_PATH_SEPARATOR`). The project name comes from `COMPOSE_PROJECT_NAME`, the top-level `name:` key, or the directory name. Projects that are not running are shown dimmed.
- every Docker bridge network that has containers attached, marked `[bridge]`, with its interface name and container count.

Use `j`/`k` or the arrow keys to move, `/` to filter, `Enter` to select and `q`/`Esc` to quit. The previous choice is preselected; it is stored in `$XDG_STATE_HOME/tylomon/last-choice` (default `~/.local/state`). Selecting a Compose project captures on all of its networks.

A sample project is available in the repository's `examples/` directory:

```bash
cd examples
docker compose up -d
sudo tylomon
```

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

## Record and replay traffic

Save a session to a pcap file, then inspect it later without privileges or a running Docker daemon:

```bash
tylomon --project my-compose-project --write session.pcap
tylomon --read session.pcap
```

Recordings are standard pcap files, so they also open in Wireshark.

## Export and scripting

Stream exchanges as JSON Lines for `jq`, `grep` or log shippers:

```bash
tylomon --no-tui --project my-compose-project | jq -c 'select(.duration_ms > 500)'
```

Save a HAR file to open in browser devtools or Postman, either on exit with `--har session.har` or at any time from the TUI with `e`, which exports the exchanges currently listed. See the [CLI reference](../reference/cli.md#-no-tui) for the field list.

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

A plain word matches the HTTP method, request path, status code, or source/destination container. Prefixed terms narrow it down, for example `status:5xx dst:api` or `>500ms -path:/health`. Press `?` at any time for the list of keys. The mouse works too: click an exchange to select it, double-click to open the detail pane, and use the wheel to scroll. Press `f` to follow the newest exchange as traffic arrives; the selection otherwise stays on the exchange you picked even when the list changes.

Drag inside the detail pane to select text; it is copied when you release the mouse button. Double-click a word to copy just that word.

Drag the border between the panes to resize them, or press `<` and `>`. Press `z` to zoom the focused pane to full screen and `q` or `Esc` to return. See the [layout keys](../reference/keybindings.md#layout).

See [Keyboard Shortcuts](../reference/keybindings.md#search-filter) for the full syntax.

Press `s` to sort the list by response time, slowest first.

Press `q` or `Esc` to clear an active filter.

## Navigating the interface

The interface has two panes:

- **Exchange list** (left) — shows all captured HTTP exchanges
- **Detail view** (right) — shows full headers and body for the selected exchange

Press `Tab` to switch focus between panes. When the detail pane is focused, `j`/`k` scrolls the detail content instead of navigating the list.

See [Keyboard Shortcuts](../reference/keybindings.md) for the full list of keybindings.

## Networks that change while running

tylomon re-checks the target every few seconds. If a Compose stack is taken down and brought back up (which gives its networks new bridge interfaces), or a new network joins the project, captures are re-attached automatically and the status bar shows what changed. While a network is missing, tylomon waits for it to return.

Capture failures appear in a red banner at the top, one per distinct message. The banner clears by itself once the failing capture runs again, or when its network goes away. A capture that fails for another reason than missing privileges, such as an interface that is not up yet, is retried every few seconds. Missing privileges are not retried; fix them (see [Packet capture permissions](installation.md#packet-capture-permissions)) and restart tylomon.

## Copy as curl

Select an exchange and press `3` to copy a `curl` command that repeats the request (method, URL from the `Host` header, headers and body). Headers curl derives itself, such as `Content-Length`, are left out.

## Compressed bodies

Request and response bodies with `Content-Encoding: gzip`, `deflate` or `br` are decoded before display, so JSON is readable and pretty-printed. Decoded output is capped at 16 MiB per body, and a body cut short by the capture shows what could be decoded. Other encodings are shown as received.

## Service map

Press `m` to replace the panes with a table of traffic per client and server: request count, 4xx and failures, failure rate and p50/p95 response time. Select a row and press `Enter` to jump back to the list filtered to that connection. Filters entered with `/` also apply to the map.

## Log file

tylomon writes debug logs to a file (default: `tylomon.log` in the current directory). You can change the path with `--log-file`:

```bash
sudo tylomon --project myapp --log-file /var/log/tylomon.log
```

Set the `RUST_LOG` environment variable to control log verbosity:

```bash
sudo RUST_LOG=tylomon=debug tylomon --project myapp
```
