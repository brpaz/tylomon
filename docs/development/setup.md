# Development Setup

How to set up the environment and run the checks. Reporting issues and the pull request and release process are in [Contributing](contributing.md).

## Prerequisites

The project uses [devenv.sh](https://devenv.sh/) to manage an isolated, reproducible development environment. Install devenv first, then:

```bash
devenv shell
```

This provides:

- Rust stable toolchain (with rustfmt and clippy)
- `libpcap` development headers
- [Task](https://taskfile.dev/) — task runner
- [Lefthook](https://github.com/evilmartians/lefthook) — git hooks
- [cargo-watch](https://github.com/watchexec/cargo-watch) — rebuild on file changes
- [cargo-nextest](https://nexte.st/) — fast test runner
- [uv](https://docs.astral.sh/uv/) — runs the documentation tooling

Git hooks are installed automatically when entering the devenv shell.

## Common tasks

```bash
task fmt            # Format code
task lint           # Run fmt check + clippy
task test           # Run tests (via nextest)
task build          # Build debug binary
task build:release  # Build release binary
task install        # Build release, install to ~/.local/bin and grant capabilities
task uninstall      # Remove the installed binary
task setcap         # Build, then grant packet capture capabilities to the debug binary
task run -- --project myapp  # Same, then run with args (no sudo needed at runtime)
task e2e            # Capture the examples/ stack and assert on the exchanges
task watch          # Watch and rebuild on changes
task ci             # Run fmt, clippy, tests and a release build locally
task docker:build   # Build Docker image locally
task docker:run -- --project myapp  # Run Docker image
task docs:serve     # Preview these docs with live reload (Zensical)
task docs:build     # Build the docs with warnings as errors
task clean          # Clean build artifacts
```

## Documentation

The docs in `docs/` are built with [Zensical](https://zensical.org/), configured in `zensical.toml`. The version is pinned in `requirements-docs.txt` and the tasks run it through `uv`, so nothing needs installing by hand. The `Docs` workflow publishes to GitHub Pages when `docs/`, `zensical.toml` or `requirements-docs.txt` change on `main`.

## Running tests

```bash
# All tests
cargo test --all-features

# Single test by name
cargo test test_parse_simple_get_request

# Tests matching a substring
cargo test decode_chunked

# Tests in a specific module
cargo test http::parser::tests

# Integration tests only
cargo test --test http_parser_tests
cargo test --test replay_tests
```

`tests/replay_tests.rs` builds small pcap files with real Ethernet/IP/TCP packets and replays them through the capture pipeline, so it needs neither privileges nor Docker.

## End-to-end check

`task e2e` (or `scripts/e2e.sh [binary]`) starts the stack in `examples/`, captures it for 15 seconds with `--no-tui --har --write`, and asserts on the JSON Lines output, the HAR file and a replay of the recording. It needs Docker, `jq` and `sudo` (to run `setcap` on the binary the first time).

Debug builds lose their capabilities on every rebuild; `task run` and `task setcap` grant them again. To run by hand:

```bash
cd examples && docker compose up -d
sudo setcap cap_net_raw,cap_net_admin=eip ../target/debug/tylomon
../target/debug/tylomon
```

## Git hooks

[Lefthook](https://github.com/evilmartians/lefthook) is configured with:

- **pre-commit**: `cargo fmt --all -- --check`
- **pre-push**: format check, clippy, and `cargo test --lib`

## CI pipeline

GitHub Actions runs `CI` on pushes to `main`, on pull requests and when a release is published:

1. **Check & Test** — formatting, clippy, tests
2. **End-to-end** — builds the debug binary and runs `scripts/e2e.sh` against the example stack
3. **Build Binary** — release build, uploaded as artifact and attached to published releases
4. **Build & Push Docker** — multi-arch image (amd64 + arm64) pushed to `ghcr.io`, for published releases only

Two more workflows run on pushes to `main`: `Docs` builds and deploys this site when the docs change, and `Draft release` updates the draft release notes from the Conventional Commits since the last release.

Run the full pipeline locally:

```bash
task ci
```
