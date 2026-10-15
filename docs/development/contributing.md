# Contributing

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

## Commits and pull requests

1. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `chore(deps):` …)
2. Run `task ci`, and `task e2e` when you touch capture, export or the example stack
3. Keep changes focused — one feature or fix per PR
4. Add tests for new parsing or logic changes
5. Update the docs and README for user-visible changes
