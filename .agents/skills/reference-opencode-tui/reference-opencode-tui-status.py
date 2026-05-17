#!/usr/bin/env python3
"""Check if reference TUI and OpenCode server are running."""

import argparse
import os
import subprocess
import sys
import socket

SESSION = "reference_tui_session"
PID_FILE_TUI = "/tmp/reference_tui.pid"
PID_FILE_OPENCODE = "/tmp/opencode_reference.pid"
URL = "http://localhost:7775"


def _cmd_matches(pid, pattern):
    try:
        with open(f"/proc/{pid}/cmdline") as f:
            cmd = f.read().replace("\0", " ")
            return pattern in cmd
    except (OSError, IOError):
        r = subprocess.run(
            ["pgrep", "-f", pattern, "-n"],
            capture_output=True, text=True,
        )
        return r.returncode == 0 and str(pid) in r.stdout.strip().split()


def _process_running(pidfile, cmd_pattern):
    if not os.path.isfile(pidfile):
        return False
    try:
        with open(pidfile) as f:
            pid = int(f.read().strip())
    except (ValueError, OSError):
        return False
    try:
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return False
    return cmd_pattern is None or _cmd_matches(pid, cmd_pattern)


def is_tmux_alive():
    r = subprocess.run(
        ["tmux", "has-session", "-t", SESSION],
        capture_output=True,
    )
    return r.returncode == 0


def is_opencode_reachable():
    try:
        with socket.create_connection((socket.gethostbyname("localhost"), 7775), timeout=2):
            return True
    except OSError:
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Check reference TUI and OpenCode server status"
    )
    parser.parse_args()

    tui_running = is_tmux_alive() and _process_running(PID_FILE_TUI, "opencode attach")
    opencode_running = _process_running(PID_FILE_OPENCODE, "opencode serve") and is_opencode_reachable()

    print("Running" if tui_running and opencode_running else "Not Running")
    return 0


if __name__ == "__main__":
    sys.exit(main())
