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
  ghcr.io/brpaz/tylomon:latest \
  --project my-compose-project
```

!!! note
    The `--network host` flag is required so tylomon can access the Docker bridge interfaces on the host.

## Filtering traffic

Press `/` in the TUI to open the filter, type a query and press `Enter`. A plain word matches the HTTP method, request path, status code, or source/destination container. Prefixed terms narrow it down, for example `status:5xx dst:api` or `>500ms -path:/health`. `q` or `Esc` clears an active filter. See [Keyboard Shortcuts](../reference/keybindings.md#search-filter) for the full syntax.

Press `s` to sort the list by response time, slowest first, and `f` to follow the newest exchange as traffic arrives. The selection stays on the exchange you picked even when the list changes.

## Navigating the interface

The interface has two panes:

- **Exchange list** (left) shows all captured HTTP exchanges
- **Detail view** (right) shows a pinned summary, then the headers and bodies of the selected exchange

Press `Tab` to switch focus between panes. When the detail pane is focused, `j`/`k` scrolls its content instead of moving the selection. Press `?` at any time for the list of keys.

With the mouse, click an exchange to select it, double-click to open the detail pane, and use the wheel to scroll. Drag inside the detail pane to select text; it is copied when you release the mouse button, and a double-click copies a single word.

Drag the border between the panes to resize them, or press `<` and `>`. Press `z` to zoom the focused pane to full screen and `q` or `Esc` to return. See the [layout keys](../reference/keybindings.md#layout) and the [full keybinding list](../reference/keybindings.md).

To copy a whole message, press `1` for the request, `2` for the response, `y` for both, or `3` for a `curl` command that repeats the request.

## Networks that change while running

tylomon re-checks the target every few seconds. If a Compose stack is taken down and brought back up (which gives its networks new bridge interfaces), or a new network joins the project, captures are re-attached automatically and the status bar shows what changed. While a network is missing, tylomon waits for it to return.

Capture failures appear in a red banner at the top, one per distinct message. The banner clears by itself once the failing capture runs again, or when its network goes away. A capture that fails for another reason than missing privileges, such as an interface that is not up yet, is retried every few seconds. Missing privileges are not retried; fix them (see [Packet capture permissions](installation.md#packet-capture-permissions)) and restart tylomon.

## Copy as curl

The `curl` command repeats the request with its method, URL (from the `Host` header), headers and body. Headers curl derives itself, such as `Content-Length`, are left out.

## PostgreSQL

tylomon recognises PostgreSQL connections by their startup packet and decodes both directions. Each query becomes an entry in the list next to the HTTP exchanges:

- the list shows the statement's verb (`SELECT`, `INSERT`, ...) and the SQL on one line, with `OK` or `ERR`, the duration and the endpoints
- the detail pane shows the user, database and application, the full SQL, bound parameters for prepared statements (`-- $1 = 42`), the command tag (`SELECT 3`, `INSERT 0 1`), the row count and a table with the first 50 rows
- errors are shown with the SQLSTATE, message, detail and hint, and match `status:5xx`
- each connection also appears once as a `CONNECT user@database` entry with the time the login took, or the reason it failed

Use `proto:pg` to show only queries, `proto:http` to hide them, and the usual filters for the rest: `proto:pg >100ms`, `proto:pg path:users`, `proto:pg status:5xx`. The service map includes database edges (`app → db:5432`).

Limits:

- Connections that negotiate TLS (`sslmode=require` or a server with SSL enabled) are shown as a single `CONNECT … (TLS encrypted)` entry; their queries cannot be decoded. Clients using `sslmode=prefer` against a server without SSL work, which is the usual case between containers.
- The connection must be captured from its start. Connections opened before tylomon started are not recognised.
- Parameters and result values are shown as sent. Passwords are never read or stored, but query parameters and rows can contain sensitive data.
- Copy as `curl` and HAR export apply to HTTP exchanges only. JSON Lines output includes PostgreSQL entries with `"protocol": "postgres"`.

## gRPC and HTTP/2

Connections that start with the HTTP/2 client preface (cleartext HTTP/2 with prior knowledge, which is how gRPC runs between containers) are decoded. Calls with an `application/grpc` content type are shown as `gRPC /pkg.Service/Method` with the gRPC status (`OK`, `NOT_FOUND`, ...) and the messages of each direction rendered schema-less, like `protoc --decode_raw`: `1: "hello"`, nested messages in braces, and unknown binary as hex. Compressed messages (gzip) are decompressed. Other HTTP/2 requests appear as normal HTTP exchanges with version `HTTP/2`; copy as `curl` adds `--http2-prior-knowledge`.

Use `proto:grpc` to show only gRPC calls. Failed calls match `status:5xx`. A sample is in `examples/grpc/`.

Limits: TLS-encrypted HTTP/2 cannot be decoded, the connection must be captured from its start (HPACK needs the whole header history), and field names are unknown without the `.proto` files.

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
