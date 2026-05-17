#!/usr/bin/env python3
"""Capture tmux pane content as a screenshot."""

import subprocess
import sys

SESSION = "tui_session"
WINDOW = "tui"
TARGET = f"{SESSION}:{WINDOW}"


def main():
    result = subprocess.run(
        ["tmux", "capture-pane", "-t", TARGET, "-p"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        print("ERROR: tmux capture-pane failed")
        return 1
    sys.stdout.write(result.stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main())
