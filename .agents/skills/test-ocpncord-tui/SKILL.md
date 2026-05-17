---
name: ocpncord-tui
description: Python toolkit for LLM agents to start, stop, inspect, and interact with the Rust TUI application running in tmux. Covers lifecycle management, keystroke injection, screenshot capture, and log retrieval.
---

# ocpncord TUI Toolkit

A collection of seven Python scripts that let LLM agents drive the `ocpncord-native` Rust TUI application inside a tmux session. Use these scripts when you need to start/stop the TUI, send keystrokes, capture screenshots, or inspect logs — without touching tmux directly.

## Script reference

| Script | Description | CLI usage | Prints on success | Prints on failure |
|---|---|---|---|---|
| `ocpncord-tui-start.py` | Create tmux session, start OpenCode server, then start TUI | `./ocpncord-tui-start.py` or `./ocpncord-tui-start.py --force` to restart | `Started` / `Already Running` | `ERROR: ...` (non-zero exit) |
| `ocpncord-tui-stop.py` | Kill both processes, kill tmux session, remove all temp files | `./ocpncord-tui-stop.py` | `Stopped` | `ERROR: ...` (non-zero exit) |
| `ocpncord-tui-status.py` | Check tmux session, TUI PID, and OpenCode HTTP endpoint | `./ocpncord-tui-status.py` | `Running` / `Not Running` | Always exits 0 |
| `ocpncord-tui-screenshot.py` | Capture tmux pane content as screenshot | `./ocpncord-tui-screenshot.py` | (pane text content) | `ERROR: ...` (non-zero exit) |
| `ocpncord-tui-input.py` | Send keystrokes to the TUI via tmux send-keys | `./ocpncord-tui-input.py --keys "hello<Enter>"` | `Sent: hello<Enter>` | `ERROR: ...` (non-zero exit) |
| `ocpncord-tui-input.py` (tests) | Run parser unit tests | `./ocpncord-tui-input.py --test` | `OK` (55 tests) | `FAILED` |
| `ocpncord-tui-logs-opencode.py` | Print server log to stdout | `./ocpncord-tui-logs-opencode.py` | (file contents) | `ERROR: log file not found` (non-zero exit) |
| `ocpncord-tui-logs-tui.py` | Print TUI debug log to stdout | `./ocpncord-tui-logs-tui.py` | (file contents) | `ERROR: log file not found` (non-zero exit) |

## key syntax for `ocpncord-tui-input.py`

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

1. `ocpncord-tui-start.py` — start both services
2. `ocpncord-tui-status.py` — confirm everything is running
3. `ocpncord-tui-screenshot.py` — baseline screenshot
4. `ocpncord-tui-input.py --keys "..."` — interact with the TUI
5. `ocpncord-tui-screenshot.py` — compare after-interaction state
6. `ocpncord-tui-stop.py` — shut down and clean up

## Important notes

- **OpenCode server** always starts in a fresh temporary directory at `/tmp/test-tui-XXXXXX`. The path is stored in `/tmp/opencode_workdir`.
- **All scripts are idempotent** where applicable. Starting already-running services prints `Already Running` (exit 0). Stopping when nothing is running prints `Stopped` (exit 0).
- **Agents must never interact with tmux directly.** Use these scripts as the sole interface.
- **Log files:**
  - `/tmp/opencode.log` — OpenCode server stdout/stderr
  - `/tmp/ocpncord.log` — TUI internal debug log
- **Screenshots:** Uses `tmux capture-pane` to capture the visible pane content as plain text.
- **PID files:** `/tmp/tui.pid` and `/tmp/opencode.pid`
- **The TUI binary** is `ocpncord-native`. The start script uses `cargo run` from the repo root, so compilation happens automatically.
- **`opencode serve`** (no `r` at the end) is the correct subcommand. Do not use `opencode server`.
