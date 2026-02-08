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
use color_eyre::eyre::Result;

use crate::capture::bridge::bridge_interface_name;
```

### Error handling

- Use `color_eyre::eyre::Result` for fallible functions
- Use `Option` for parsing functions that may not find valid input
- Use `eyre!("message")` for ad-hoc errors
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
- Integration tests go in the `tests/` directory
- Use `.expect("description")` instead of `.unwrap()` in tests

## TUI guidelines

- Views are stateless free functions: `pub fn render(frame, app, area, focused)`
- All application state lives in `App` — views only read from it
- All keyboard input flows through `App::handle_key()`
- Colors and styles are defined in `Theme` as static methods

## Pull requests

1. Run `task ci` before submitting
2. Keep changes focused — one feature or fix per PR
3. Add tests for new parsing or logic changes
