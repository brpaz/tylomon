# CLI Options

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

!!! warning
    You must provide either `--project` or `--network`. If neither is specified, tylomon will exit with an error.

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

Maximum number of HTTP exchanges to keep in memory. When the limit is reached, the oldest exchanges are discarded. Defaults to `1000`.

```bash
sudo tylomon --project myapp --max-exchanges 5000
```
