#!/usr/bin/env python3
"""Send keystrokes to the TUI via tmux send-keys."""

import argparse
import subprocess
import sys

SESSION = "tui_session"
WINDOW = "tui"
TARGET = f"{SESSION}:{WINDOW}"


def main():
    parser = argparse.ArgumentParser(
        description="Send keystrokes to the TUI via tmux"
    )
    parser.add_argument(
        "--keys",
        nargs="+",
        required=True,
        help="Space-separated key sequence using tmux key notation",
    )
    args = parser.parse_args()

    keys = []
    for token in args.keys:
        keys.extend(token.split())

    r = subprocess.run(
        ["tmux", "has-session", "-t", SESSION],
        capture_output=True,
    )
    if r.returncode != 0:
        print(f"ERROR: tmux session '{SESSION}' not found")
        return 1

    r = subprocess.run(
        ["tmux", "list-windows", "-t", SESSION],
        capture_output=True, text=True,
    )
    if WINDOW not in r.stdout:
        print(f"ERROR: tmux window '{WINDOW}' not found in session '{SESSION}'")
        return 1

    subprocess.run(
        ["tmux", "send-keys", "-t", TARGET] + keys,
        check=True,
    )

    print(f"Sent: {' '.join(keys)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
