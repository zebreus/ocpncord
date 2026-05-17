---
name: test-tui
description: Python toolkit for LLM agents to start, stop, inspect, and interact with the Rust TUI application running in tmux. Covers lifecycle management, keystroke injection, screenshot capture, and log retrieval.
---

# Test TUI Toolkit

A collection of seven Python scripts that let LLM agents drive the `opencode-native` Rust TUI application inside a tmux session. Use these scripts when you need to start/stop the TUI, send keystrokes, capture screenshots, or inspect logs — without touching tmux directly.

## Script reference

| Script | Description | CLI usage | Prints on success | Prints on failure |
|---|---|---|---|---|
| `test-tui-start.py` | Create tmux session, start OpenCode server, then start TUI | `./test-tui-start.py` | `TUI started (PID 12345), OpenCode server started at http://localhost:7774` | `ERROR: ...` (non-zero exit) |
| `test-tui-stop.py` | Kill both processes, kill tmux session, remove all temp files | `./test-tui-stop.py` | `Stopped` | `ERROR: ...` (non-zero exit) |
| `test-tui-status.py` | Check tmux session, TUI PID, and OpenCode HTTP endpoint | `./test-tui-status.py` | `TUI: RUNNING` / `OPENCODE: RUNNING` | Always exits 0 |
| `test-tui-screenshot.py` | Send SIGUSR1 to TUI, print screenshot content to stdout | `./test-tui-screenshot.py` | (screenshot plain-text content) | `ERROR: ...` (non-zero exit) |
| `test-tui-input.py` | Send keystrokes to the TUI via tmux send-keys | `./test-tui-input.py --keys "h e l l o Enter"` | `Sent: h e l l o Enter` | `ERROR: ...` (non-zero exit) |
| `test-tui-logs-opencode.py` | Print `/tmp/opencode.log` to stdout | `./test-tui-logs-opencode.py` | (file contents) | `ERROR: log file not found at /tmp/opencode.log` (non-zero exit) |
| `test-tui-logs-tui.py` | Print `/tmp/opencode-rust-client.log` (TUI debug log) to stdout | `./test-tui-logs-tui.py` | (file contents) | `ERROR: log file not found at /tmp/opencode-rust-client.log` (non-zero exit) |

## tmux key notation for `test-tui-input.py`

Each token passed to `--keys` is forwarded as a separate key name to `tmux send-keys`. Supported key names:

| Category | Tokens |
|---|---|
| Letters & digits | `a` `b` … `z` `0` … `9` |
| Special keys | `Enter` `Escape` `Tab` `Backspace` `Space` `Up` `Down` `Left` `Right` `Home` `End` `PageUp` `PageDown` `Delete` |
| Function keys | `F1` … `F12` |
| Ctrl combos | `C-c` `C-d` `C-x` etc. |

Example: `--keys "C-x q Enter"` sends Ctrl+X, then `q`, then Enter.

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
  - `/tmp/opencode-rust-client.log` — TUI internal debug log (from `log!()` calls in `native/src/main.rs`)
- **Screenshots:** Written as plain-text files to `/tmp/<ms>-screenshot.txt` (e.g. `/tmp/0000123-screenshot.txt`). The format is a rendered `TestBackend` buffer dump with a header showing screen name, dimensions, tick, streaming state, etc.
- **PID files:** `/tmp/tui.pid` and `/tmp/opencode.pid`
- **The TUI binary** is `opencode-native`. The start script uses `cargo run` from the repo root, so compilation happens automatically.
- **TUI depends on SIGUSR1** for screenshots. The `--screenshot-dir /tmp` flag must be present for the signal handler to be installed.
- **`opencode serve`** (no `r` at the end) is the correct subcommand. Do not use `opencode server`.
