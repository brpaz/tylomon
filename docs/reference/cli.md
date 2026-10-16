# CLI Options

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

## `--project` / `-p`

Discover and monitor all networks belonging to a Docker Compose project. tylomon finds networks by querying for the `com.docker.compose.project` label.

```bash
sudo tylomon --project my-app
```

## `--network` / `-n`

Monitor a specific Docker network by name. This overrides project-based discovery and allows monitoring any Docker network, not just Compose-managed ones.

```bash
sudo tylomon --network my-custom-network
```

!!! note
    If neither `--project` nor `--network` is given, tylomon shows an interactive list of the Compose project in the current directory and the available bridge networks. See [Usage](../getting-started/usage.md#pick-a-target-interactively).

## Environment variables

| Variable | Effect |
|----------|--------|
| `COMPOSE_FILE` | Compose files to look for in the current directory, separated by `:` |
| `COMPOSE_PATH_SEPARATOR` | Separator used in `COMPOSE_FILE` |
| `COMPOSE_PROJECT_NAME` | Overrides the detected project name |

## `--write` / `-w`

Record every captured packet to a pcap file while monitoring. With several networks (a Compose project), all bridges are written to the same file. The file can be opened in Wireshark or replayed with `--read`.

```bash
tylomon --project myapp --write session.pcap
```

## `--read` / `-r`

Replay a pcap file through the same pipeline instead of capturing live. No Docker and no capture privileges are needed, and the file is replayed as fast as it can be read. Response times come from the packet timestamps in the file. Container names are not resolved, so addresses are shown as IPs. Requests without a response by the end of the file are listed as unanswered.

```bash
tylomon --read session.pcap
```

`--read` cannot be combined with `--project`, `--network` or `--write`.

## `--no-tui`

Skip the TUI and print each completed exchange to stdout as one JSON object per line (JSON Lines). Logs still go to the log file, so stdout contains only exchanges. Stop with `Ctrl-C`; with `--read` it ends when the file does. A target is required: `--project`, `--network` or `--read`.

```bash
tylomon --no-tui --project myapp | jq 'select(.status >= 500)'
```

Each line has these fields:

| Field | Description |
|-------|-------------|
| `protocol` | `http`, `postgres` or `grpc` |
| `timestamp` | Request time, ISO 8601 UTC |
| `method`, `path`, `http_version` | Request line. For PostgreSQL: statement verb, SQL on one line, `PostgreSQL` |
| `status` | Response status, `null` if no response was seen. For PostgreSQL: 200 on success, 500 on error |
| `duration_ms` | Request to response time, `null` if unanswered |
| `src`, `dst` | Container name, or IP when unresolved |
| `src_ip`, `dst_ip`, `src_port`, `dst_port` | Connection endpoints |
| `request_headers`, `response_headers` | Lists of `{name, value}` |
| `request_body`, `response_body` | `{text, size}` or `null`; non-UTF-8 bodies are base64 with `"encoding": "base64"` |

Names are resolved from Docker every few seconds, so the first exchanges of a live session may show IPs.

## `--har`

Write all captured exchanges to a [HAR 1.2](http://www.softwareishard.com/blog/har-12-spec/) file when tylomon exits. At most `--max-exchanges` of the newest exchanges are kept. Works with and without `--no-tui`. Open the file in browser devtools, Postman or Insomnia.

```bash
tylomon --project myapp --har session.har
```

PostgreSQL exchanges are left out, since HAR describes HTTP. Unanswered requests are exported with status `0` and timings of `-1`. The request URL uses the `Host` header, falling back to the destination address.

## `--log-file`

Path to the debug log file. Defaults to `tylomon.log` in the current directory.

```bash
sudo tylomon --project myapp --log-file /var/log/tylomon.log
```

Control log verbosity with the `RUST_LOG` environment variable:

```bash
sudo RUST_LOG=tylomon=debug tylomon --project myapp
```

## `--max-exchanges`

Maximum number of exchanges (HTTP, PostgreSQL and gRPC) to keep in memory. When the limit is reached, the oldest exchanges are discarded. Defaults to `1000`.

```bash
sudo tylomon --project myapp --max-exchanges 5000
```
