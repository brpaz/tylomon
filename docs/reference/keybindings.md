# Keyboard Shortcuts

## General

| Key       | Action                        |
|-----------|-------------------------------|
| `?`       | Show / hide the in-app key reference |
| `q` / `Esc` | Step back one level: clear the text selection, then leave zoom, then clear the filter, then quit |
| `Ctrl+C`  | Quit immediately              |
| `c`       | Clear all exchanges           |
| `s`       | Toggle sort: capture order / slowest first |
| `p`       | Pause / resume capture        |
| `Tab`     | Switch focus between panes    |

## Copy and export

| Key       | Action                        |
|-----------|-------------------------------|
| `y`       | Copy the whole exchange (request and response) |
| `1` / `2` | Copy the request / the response |
| `3`       | Copy the request as a `curl` command |
| `e`       | Export the listed (filtered) exchanges to `tylomon-<timestamp>.har` in the current directory |

Dragging in the detail pane copies the selected text, see [Mouse](#mouse). Copy confirmations replace the key hints in the status bar for a few seconds.

## Navigation

| Key        | Action (List pane focused)     | Action (Detail pane focused)   |
|------------|-------------------------------|-------------------------------|
| `j` / `Down` | Select next exchange        | Scroll down                   |
| `k` / `Up`   | Select previous exchange    | Scroll up                     |
| `g`         | Jump to first exchange        | Scroll to top                 |
| `G`         | Jump to last exchange and follow new ones | Scroll to bottom  |
| `PgDn`      | Page down (10 items)          | Scroll down (10 lines)        |
| `PgUp`      | Page up (10 items)            | Scroll up (10 lines)          |

## Mouse

| Action | Effect |
|--------|--------|
| Click an exchange | Select it and focus the list |
| Double-click an exchange | Select it and focus the detail pane |
| Click the detail pane | Focus it |
| Drag inside the detail text | Select text; it is copied to the clipboard when you release the button |
| Double-click a word in the detail text | Select and copy that word (paths, header values, JSON strings) |
| Drag above or below the detail text | Keep selecting while the content scrolls |
| Wheel over the detail pane | Scroll it |
| Wheel over the list | Move the selection |
| Click an edge in the service map | Select it |
| Double-click an edge | Show that edge's requests in the list |
| Wheel over the service map | Move the selection |
| Wheel over the help | Scroll it |
| Click outside the help | Close it |

Selection stays inside the scrollable part of the detail pane (headers and bodies), never the list or borders. Lines that were only wrapped to fit the pane are copied as one line, without the wrap indent. `Esc`, a click elsewhere, or selecting another exchange clears the highlight. The pinned summary at the top is not selectable; use `1`, `2`, `3` or `y` to copy whole messages.

Clicks are ignored while the filter prompt is open. Dragging is ignored while a pane is zoomed.

## Layout

| Key / action | Effect |
|--------------|--------|
| `z` | Zoom the focused pane to full screen. `Tab` switches to the other pane, still full screen; `q` or `Esc` returns to the split |
| `<` / `>` (or `,` / `.`) | Make the list pane narrower / wider |
| `=` | Reset the split to the default |
| Drag the border between the panes | Resize them continuously |
| Double-click the border | Reset the split |

Each pane keeps at least 20 columns. A zoomed list is useful for long paths and the `src→dst` column; a zoomed detail pane gives wide hex dumps (16 bytes per row) and fewer wrapped lines. The split is not saved between runs.

## Follow

| Key | Action |
|-----|--------|
| `f` | Toggle follow: the newest exchange stays selected as traffic arrives. Moving up or `g` turns it off; `G` turns it on. |

The selection tracks the exchange itself, not its row, so it stays put when new exchanges arrive, the sort changes or a filter still lists it.

## Detail pane

The top of the pane stays pinned while the content scrolls: request line (method and path), a coloured status badge with response time and body sizes, the source and destination with the capture time, then request headers, request body, response headers and response body. A position indicator and scrollbar appear when the content is longer than the pane.

- JSON bodies are pretty-printed with coloured keys, strings, numbers and keywords.
- Bodies that are not valid UTF-8 text are shown as a hex dump with an ASCII column (first 8 KB).
- Text bodies over 256 KB are cut off with a notice; copy with `y`, `1` or `2` for the full body.
- Requests that never received a response are marked `NO RESPONSE`.

## Service map

| Key       | Action                                                    |
|-----------|-----------------------------------------------------------|
| `m`       | Toggle the service map (from the list) / return to the list |
| `j` / `k` | Select next / previous edge                               |
| `g` / `G` | First / last edge                                         |
| `Enter`   | Return to the list, filtered to the selected edge (`src:… dst:…`) |
| `q` / `Esc` | Return to the list                                      |

The map groups the filtered exchanges by source, destination and destination port, busiest first. Columns: request count, `4xx` responses, `Failed` (5xx responses plus requests that never got a response), failure percentage, and p50/p95 response time (nearest-rank, answered requests only). Active filters apply, so `/` with `>500ms` shows only the slow traffic per edge. `p`, `/`, `e` and `c` work as in the list.

## Search & filter

| Key       | Action                                        |
|-----------|-----------------------------------------------|
| `/`       | Open search filter                            |
| `Enter`   | Apply filter (in filter mode)                 |
| `Esc`     | Cancel filter input / clear active filter     |
| `Backspace` | Delete last character (in filter mode)      |

Filters are case-insensitive. Separate terms with spaces; all terms must match.

| Term | Matches |
|------|---------|
| `word` | Method, path, status code, or source/destination name or IP |
| `src:api`, `dst:gateway` | Source / destination container name or IP (substring) |
| `method:post`, `path:/users` | Method / path (substring) |
| `status:404`, `status:5xx`, `status:>=400` | Exact status, status class, or comparison (`<`, `<=`, `>`, `>=`) |
| `>500ms`, `<2s`, `>=1.5s` | Response time |
| `!term`, `-term` | Negates any term above |

Examples:

```
status:5xx dst:api         server errors from the api container
>500ms -path:/health       slow requests, ignoring health checks
method:post !status:2xx    failed POSTs
```

Exchanges still waiting for a response never match `status:` or duration terms.

## Response time colours

Response times are shown dimmed, yellow from 300 ms and red from 1 s.
