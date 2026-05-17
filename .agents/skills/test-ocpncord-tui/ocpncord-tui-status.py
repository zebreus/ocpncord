#!/usr/bin/env python3
"""Check if TUI and OpenCode server are running."""

import argparse
import os
import subprocess
import sys
import socket

SESSION = "tui_session"
PID_FILE_TUI = "/tmp/tui.pid"
PID_FILE_OPENCODE = "/tmp/opencode.pid"
URL = "http://localhost:7774"


def is_tmux_alive():
    r = subprocess.run(
        ["tmux", "has-session", "-t", SESSION],
        capture_output=True,
    )
    return r.returncode == 0


def is_pid_alive(pidfile):
    if not os.path.isfile(pidfile):
        return False
    try:
        with open(pidfile) as f:
            pid = int(f.read().strip())
        os.kill(pid, 0)
        return True
    except (ValueError, OSError, ProcessLookupError):
        return False


def is_opencode_reachable():
    try:
        with socket.create_connection((socket.gethostbyname("localhost"), 7774), timeout=2):
            return True
    except OSError:
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Check TUI and OpenCode server status"
    )
    parser.parse_args()

    tui_running = is_tmux_alive() and is_pid_alive(PID_FILE_TUI)
    opencode_running = is_pid_alive(PID_FILE_OPENCODE) and is_opencode_reachable()

    print("Running" if tui_running and opencode_running else "Not Running")
    return 0


if __name__ == "__main__":
    sys.exit(main())
