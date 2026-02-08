# Keyboard Shortcuts

## General

| Key       | Action                        |
|-----------|-------------------------------|
| `q`       | Clear active filter, or quit  |
| `Ctrl+C`  | Quit immediately              |
| `c`       | Clear all exchanges           |
| `Tab`     | Switch focus between panes    |

## Navigation

| Key        | Action (List pane focused)     | Action (Detail pane focused)   |
|------------|-------------------------------|-------------------------------|
| `j` / `Down` | Select next exchange        | Scroll down                   |
| `k` / `Up`   | Select previous exchange    | Scroll up                     |
| `g`         | Jump to first exchange        | —                             |
| `G`         | Jump to last exchange         | —                             |
| `PgDn`      | Page down (10 items)          | Scroll down (10 lines)        |
| `PgUp`      | Page up (10 items)            | Scroll up (10 lines)          |

## Search & filter

| Key       | Action                                        |
|-----------|-----------------------------------------------|
| `/`       | Open search filter                            |
| `Enter`   | Apply filter (in filter mode)                 |
| `Esc`     | Cancel filter input / clear active filter     |
| `Backspace` | Delete last character (in filter mode)      |

The filter matches case-insensitively against:

- HTTP method (`GET`, `POST`, `PUT`, `DELETE`, etc.)
- Request path (`/api/users`, `/health`, etc.)
- Response status code (`200`, `404`, `500`, etc.)
