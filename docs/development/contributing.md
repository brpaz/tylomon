# Contributing

This page covers how the project is built and released, and its code style. How to report issues and what a good pull request looks like are in [CONTRIBUTING.md](https://github.com/brpaz/tylomon/blob/main/CONTRIBUTING.md); the environment, commands and tests are on the [Development Setup](setup.md) page, and the [architecture page](../reference/architecture.md) explains the data pipeline.

Documentation lives in `docs/` and is built with [Zensical](https://zensical.org/). Update it, the README and the `?` help overlay in the same change as any user-visible behaviour, including new key bindings.

## Project process

### Continuous integration

GitHub Actions runs on every push to `main` and on pull requests:

| Job | What it does |
|-----|--------------|
| **Check & Test** | `cargo fmt --check`, clippy with `-D warnings`, `cargo test` |
| **End-to-end** | Builds the binary and runs `scripts/e2e.sh` against the example stack |
| **Build Binary** | Release build; uploaded as an artifact, and attached to the release when one is published |
| **Build & Push Docker Image** | Multi-arch image (amd64, arm64) pushed to `ghcr.io`; runs for published releases only |
| **Docs** | Builds the site with `--strict` and deploys it to GitHub Pages when `docs/`, `zensical.toml` or `requirements-docs.txt` change on `main` |
| **Draft release** | On pushes to `main`, [draftsman](https://github.com/brpaz/draftsman) updates the draft release notes from the Conventional Commits since the last release |

### Dependencies

[Renovate](https://docs.renovatebot.com/) opens pull requests for dependency, GitHub Action and Docker base image updates. Versions are pinned exactly (and Actions by commit SHA), updates wait three days after release, and vulnerability alerts are raised straight away. Minor and patch updates of actions are grouped; Rust crate and Docker updates are grouped and need a maintainer to merge. A lock-file refresh runs monthly.

Do not bump versions by hand unless a change needs it. If you add a dependency, pin it with `=` in `Cargo.toml`.

### Releases

Releases follow [semantic versioning](https://semver.org/).

1. [draftsman](https://github.com/brpaz/draftsman) maintains a draft release. On every push to `main` it reads the Conventional Commits since the last release, groups them into sections by type (`feat` is Features, `fix` is Bug Fixes, `chore`, `build` and `ci` are Maintenance, `docs` is Documentation, anything else is Other), and picks the next version itself: a breaking change (`!` or a `BREAKING CHANGE:` footer) is a major bump, `feat` a minor one, anything else a patch. The sections are configured in [`.draftsman.yml`](https://github.com/brpaz/tylomon/blob/main/.draftsman.yml).
2. A maintainer checks the draft and publishes it (from the GitHub release page, or with `draftsman publish`), which creates the `vX.Y.Z` tag.
3. Publishing triggers CI to set the version in `Cargo.toml` from the tag, build the release binary and attach it to the release, and build and push the multi-arch Docker image to `ghcr.io/brpaz/tylomon`.

The release notes are built from commit messages, not labels, so the squash-merge title of a pull request (which becomes the commit on `main`) must be a clear Conventional Commit. To keep a commit out of the release notes, add `[skip changelog]` anywhere in its message or a `Skip-Changelog: true` footer.

## Code style

### Formatting

- Standard `cargo fmt` — no custom rustfmt configuration
- Clippy with `-D warnings` — zero warnings policy

### Imports

Group imports in this order, separated by blank lines:

1. `std` library
2. External crates
3. Crate-internal (`use crate::...`)

```rust
use std::collections::HashMap;
use std::net::IpAddr;

use bollard::Docker;
use anyhow::Result;

use crate::capture::bridge::bridge_interface_name;
```

### Error handling

- Use `anyhow::Result` for fallible functions
- Use `Option` for parsing functions that may not find valid input
- Use `anyhow!("message")` for ad-hoc errors
- Custom errors live in `src/error.rs`
- No `unwrap()` in production code — use `?`, `.ok()`, or `unwrap_or_else`

### Naming conventions

- Types: `PascalCase` — `FlowKey`, `HttpExchange`
- Functions: `snake_case` — `parse_packet`, `bridge_interface_name`
- Constants: `SCREAMING_SNAKE_CASE` — `PENDING_REQUEST_TIMEOUT`
- Enum variants: `PascalCase` — `Action::NewExchange`

### Struct style

- All fields `pub` on data structs
- Doc comments (`///`) on public types and functions
- Derive order: `Debug, Clone` minimum; add `Serialize, Deserialize` for data types; `Hash, Eq, PartialEq` for key types

### Testing

- Unit tests go inside the source file in a `#[cfg(test)] mod tests` block
- Integration tests go in the `tests/` directory. Pipeline behaviour is tested by generating a pcap and replaying it (see `tests/replay_tests.rs`)
- UI logic is tested through `App` (keys, clicks, drags) with layout areas set by hand
- Use `.expect("description")` instead of `.unwrap()` in tests

## TUI guidelines

- Views are free functions: `pub fn render(frame, app, area, focused)`
- All application state lives in `App`. Views read from it; the only things they write back are layout facts needed for mouse hit-testing (pane areas, scroll offsets) and the detail row cache
- All keyboard input flows through `App::handle_key()` and mouse input through `App::update()`, so behaviour is testable without a terminal
- Colors and styles are defined in `Theme` as static methods
- Keep `Row`s in the detail pane one screen line each (pre-wrapped), so mouse positions map straight onto text
- Every key binding belongs in `views/help.rs`, `docs/reference/keybindings.md` and the README essentials table
