---
name: test-tui
description: Python toolkit for LLM agents to start, stop, inspect, and interact with the Rust TUI application running in tmux. Covers lifecycle management, keystroke injection, screenshot capture, and log retrieval.
---

# Test TUI Toolkit

A collection of seven Python scripts that let LLM agents drive the `ocpncord-native` Rust TUI application inside a tmux session. Use these scripts when you need to start/stop the TUI, send keystrokes, capture screenshots, or inspect logs — without touching tmux directly.

## Script reference

| Script | Description | CLI usage | Prints on success | Prints on failure |
|---|---|---|---|---|
| `test-tui-start.py` | Create tmux session, start OpenCode server, then start TUI | `./test-tui-start.py` | `TUI started (PID 12345), OpenCode server started at http://localhost:7774` | `ERROR: ...` (non-zero exit) |
| `test-tui-stop.py` | Kill both processes, kill tmux session, remove all temp files | `./test-tui-stop.py` | `Stopped` | `ERROR: ...` (non-zero exit) |
| `test-tui-status.py` | Check tmux session, TUI PID, and OpenCode HTTP endpoint | `./test-tui-status.py` | `TUI: RUNNING` / `OPENCODE: RUNNING` | Always exits 0 |
| `test-tui-screenshot.py` | Capture tmux pane content as screenshot | `./test-tui-screenshot.py` | (pane text content) | `ERROR: ...` (non-zero exit) |
| `test-tui-input.py` | Send keystrokes to the TUI via tmux send-keys | `./test-tui-input.py --keys "hello<Enter>"` | `Sent: hello<Enter>` | `ERROR: ...` (non-zero exit) |
| `test-tui-logs-opencode.py` | Print `/tmp/opencode.log` to stdout | `./test-tui-logs-opencode.py` | (file contents) | `ERROR: log file not found at /tmp/opencode.log` (non-zero exit) |
| `test-tui-logs-tui.py` | Print `/tmp/ocpncord.log` (TUI debug log) to stdout | `./test-tui-logs-tui.py` | (file contents) | `ERROR: log file not found at /tmp/ocpncord.log` (non-zero exit) |

## key syntax for `test-tui-input.py`

The `--keys` argument uses angle-bracket notation. Text outside `<...>` is typed literally. Inside `<...>`, recognized key names are sent as keystrokes:

| Category | Names |
|---|---|
| Special keys | `Enter`, `Escape`, `Tab`, `Backspace`, `Space` |
| Arrow keys | `Up`, `Down`, `Left`, `Right` |
| Navigation | `Home`, `End`, `PageUp`, `PageDown` |
| Editing | `Delete` |
| Function keys | `F1` … `F12` |
| Ctrl combos | `C-a` through `C-z` (e.g. `<C-c>`, `<C-x>`) |

`<<KEY>>` types `<KEY>` literally.

Examples: `--keys "hello<Enter>"` types "hello" and presses Enter. `--keys "<C-x>h"` opens the help modal. `--keys "<<Enter>>"` types `<Enter>` literally.

## Recommended usage sequence

1. `test-tui-start.py` — start both services
2. `test-tui-status.py` — confirm everything is running
3. `test-tui-screenshot.py` — baseline screenshot
4. `test-tui-input.py --keys "..."` — interact with the TUI
5. `test-tui-screenshot.py` — compare after-interaction state
6. `test-tui-stop.py` — shut down and clean up

## Important notes

- **OpenCode server** always starts in a fresh temporary directory created by `mktemp -d`. The path is stored in `/tmp/opencode_workdir`.
- **All scripts are idempotent** where applicable. Starting already-running services is a no-op (exit 0). Stopping when nothing is running is a no-op (exit 0).
- **Agents must never interact with tmux directly.** Use these scripts as the sole interface.
- **Log files:**
  - `/tmp/opencode.log` — OpenCode server stdout/stderr
  - `/tmp/ocpncord.log` — TUI internal debug log (from `log!()` calls in `native/src/main.rs`)
- **Screenshots:** Uses `tmux capture-pane` to capture the visible pane content as plain text.
- **PID files:** `/tmp/tui.pid` and `/tmp/opencode.pid`
- **The TUI binary** is `ocpncord-native`. The start script uses `cargo run` from the repo root, so compilation happens automatically.
- **`opencode serve`** (no `r` at the end) is the correct subcommand. Do not use `opencode server`.
