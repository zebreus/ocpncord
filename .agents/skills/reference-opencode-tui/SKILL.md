---
name: reference-opencode-tui
description: Python toolkit for LLM agents to start, stop, inspect, and interact with the reference opencode TUI (opencode attach) running in tmux. Covers lifecycle management, keystroke injection, screenshot capture, and log retrieval.
---

# Reference OpenCode TUI Toolkit

A collection of six Python scripts that let LLM agents drive the reference `opencode attach` Go/Bubble Tea TUI inside a tmux session. Use these scripts when you need to start/stop the TUI, send keystrokes, capture screenshots, or inspect logs — without touching tmux directly.

## Script reference

| Script | Description | CLI usage | Prints on success | Prints on failure |
|---|---|---|---|---|
| `reference-opencode-tui-start.py` | Create tmux session, start OpenCode server, then start reference TUI | `./reference-opencode-tui-start.py` | `Started` | `ERROR: ...` (non-zero exit) |
| `reference-opencode-tui-stop.py` | Kill both processes, kill tmux session, remove all temp files | `./reference-opencode-tui-stop.py` | `Stopped` | `ERROR: ...` (non-zero exit) |
| `reference-opencode-tui-status.py` | Check tmux session, TUI PID, and OpenCode HTTP endpoint | `./reference-opencode-tui-status.py` | `Running` / `Not Running` | Always exits 0 |
| `reference-opencode-tui-screenshot.py` | Capture tmux pane content as screenshot | `./reference-opencode-tui-screenshot.py` | (pane text content) | `ERROR: ...` (non-zero exit) |
| `reference-opencode-tui-input.py` | Send keystrokes to the TUI via tmux send-keys | `./reference-opencode-tui-input.py --keys "hello<Enter>"` | `Sent: hello<Enter>` | `ERROR: ...` (non-zero exit) |
| `reference-opencode-tui-input.py` (tests) | Run parser unit tests | `./reference-opencode-tui-input.py --test` | `OK` (55 tests) | `FAILED` |
| `reference-opencode-tui-logs-opencode.py` | Print `/tmp/opencode_reference.log` to stdout | `./reference-opencode-tui-logs-opencode.py` | (file contents) | `ERROR: log file not found at ...` (non-zero exit) |

## key syntax for `reference-opencode-tui-input.py`

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

1. `reference-opencode-tui-start.py` — start both services
2. `reference-opencode-tui-status.py` — confirm everything is running
3. `reference-opencode-tui-screenshot.py` — baseline screenshot
4. `reference-opencode-tui-input.py --keys "..."` — interact with the TUI
5. `reference-opencode-tui-screenshot.py` — compare after-interaction state
6. `reference-opencode-tui-stop.py` — shut down and clean up

## Important notes

- **Uses port 7775** (separate from the ocpncord TUI skill which uses 7774).
- **OpenCode server** always starts in a fresh temporary directory created by `mktemp -d`. The path is stored in `/tmp/opencode_reference_workdir`.
- **All scripts are idempotent** where applicable. Starting already-running services is a no-op (exit 0). Stopping when nothing is running is a no-op (exit 0).
- **Agents must never interact with tmux directly.** Use these scripts as the sole interface.
- **Log files:**
  - `/tmp/opencode_reference.log` — OpenCode server stdout/stderr
- **Screenshots:** Uses `tmux capture-pane` to capture the visible pane content as plain text.
- **PID files:** `/tmp/reference_tui.pid` and `/tmp/opencode_reference.pid`
- **The TUI binary** is `opencode attach` (the reference Go/Bubble Tea client).
- **`opencode serve`** (no `r` at the end) is the correct subcommand. Do not use `opencode server`.
