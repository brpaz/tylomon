# Development Setup

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

Git hooks are installed automatically when entering the devenv shell.

## Common tasks

```bash
task fmt            # Format code
task lint           # Run fmt check + clippy
task test           # Run tests (via nextest)
task build          # Build debug binary
task build:release  # Build release binary
task run -- --project myapp  # Run in dev mode with args
task watch          # Watch and rebuild on changes
task ci             # Run full CI pipeline locally
task docker:build   # Build Docker image locally
task docker:run -- --project myapp  # Run Docker image
task clean          # Clean build artifacts
```

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
```

## Git hooks

[Lefthook](https://github.com/evilmartians/lefthook) is configured with:

- **pre-commit**: `cargo fmt --all -- --check`
- **pre-push**: format check, clippy, and `cargo test --lib`

## CI pipeline

GitHub Actions runs on push to `main`, tags, and pull requests:

1. **Check & Test** — formatting, clippy, tests
2. **Build Binary** — release build, uploaded as artifact
3. **Build & Push Docker** — multi-arch image (amd64 + arm64) pushed to `ghcr.io` on main/tags

Run the full pipeline locally:

```bash
task ci
```
