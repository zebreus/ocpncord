---
name: test-ocpncord-tui
description: Python toolkit for LLM agents to start, stop, inspect, and interact with the Rust TUI application running in tmux. Covers lifecycle management, keystroke injection, screenshot capture, and log retrieval.
---

# Test TUI Toolkit

A collection of seven Python scripts that let LLM agents drive the `ocpncord-native` Rust TUI application inside a tmux session. Use these scripts when you need to start/stop the TUI, send keystrokes, capture screenshots, or inspect logs — without touching tmux directly.

## Script reference

| Script | Description | CLI usage | Prints on success | Prints on failure |
|---|---|---|---|---|
| `test-ocpncord-tui-start.py` | Create tmux session, start OpenCode server, then start TUI | `./test-ocpncord-tui-start.py` | `TUI started (PID 12345), OpenCode server started at http://localhost:7774` | `ERROR: ...` (non-zero exit) |
| `test-ocpncord-tui-stop.py` | Kill both processes, kill tmux session, remove all temp files | `./test-ocpncord-tui-stop.py` | `Stopped` | `ERROR: ...` (non-zero exit) |
| `test-ocpncord-tui-status.py` | Check tmux session, TUI PID, and OpenCode HTTP endpoint | `./test-ocpncord-tui-status.py` | `TUI: RUNNING` / `OPENCODE: RUNNING` | Always exits 0 |
| `test-ocpncord-tui-screenshot.py` | Capture tmux pane content as screenshot | `./test-ocpncord-tui-screenshot.py` | (pane text content) | `ERROR: ...` (non-zero exit) |
| `test-ocpncord-tui-input.py` | Send keystrokes to the TUI via tmux send-keys | `./test-ocpncord-tui-input.py --keys "hello<Enter>"` | `Sent: hello<Enter>` | `ERROR: ...` (non-zero exit) |
| `test-ocpncord-tui-input.py` (tests) | Run parser unit tests | `./test-ocpncord-tui-input.py --test` | `OK` (55 tests) | `FAILED` |
| `test-ocpncord-tui-logs-opencode.py` | Print `/tmp/opencode.log` to stdout | `./test-ocpncord-tui-logs-opencode.py` | (file contents) | `ERROR: log file not found at /tmp/opencode.log` (non-zero exit) |
| `test-ocpncord-tui-logs-tui.py` | Print `/tmp/ocpncord.log` (TUI debug log) to stdout | `./test-ocpncord-tui-logs-tui.py` | (file contents) | `ERROR: log file not found at /tmp/ocpncord.log` (non-zero exit) |

## key syntax for `test-ocpncord-tui-input.py`

Recognized key names for `<...>`:

  Special:              Enter, Escape, Tab, Backspace, Space
  Arrow keys:           Up, Down, Left, Right
  Navigation:           Home, End, PageUp, PageDown
  Editing:              Delete
  Function keys:        F1 through F12
  Ctrl combos:          C-a through C-z (e.g. <C-c>, <C-x>)

Outside `<...>` text is typed literally.
Inside `<...>`:
  `<name>` sends a keypress if name is a recognized key.
  `<<name>>` types `<name>` literally (escape).
  Any other `<...>` is typed literally, brackets included.

Examples:
  `--keys "hello<Enter>"`   types "hello" and presses Enter.
  `--keys "<C-x>h"`         opens the help modal.
  `--keys "<<Enter>>"`      types `<Enter>` literally.
  `--keys "<C-c>"`          presses Ctrl+C.
  `--keys "x<Down>y"`       types "x", presses Down, types "y".

## Recommended usage sequence

1. `test-ocpncord-tui-start.py` — start both services
2. `test-ocpncord-tui-status.py` — confirm everything is running
3. `test-ocpncord-tui-screenshot.py` — baseline screenshot
4. `test-ocpncord-tui-input.py --keys "..."` — interact with the TUI
5. `test-ocpncord-tui-screenshot.py` — compare after-interaction state
6. `test-ocpncord-tui-stop.py` — shut down and clean up

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
