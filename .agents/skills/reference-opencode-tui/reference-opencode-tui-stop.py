#!/usr/bin/env python3
"""Stop reference TUI and OpenCode server, clean up temp files."""

import argparse
import os
import signal
import subprocess
import sys
import time

SESSION = "reference_tui_session"
PID_FILE_TUI = "/tmp/reference_tui.pid"
PID_FILE_OPENCODE = "/tmp/opencode_reference.pid"
WORKDIR_FILE = "/tmp/opencode_reference_workdir"
URL = "http://localhost:7775"

TEMP_FILES = [
    "/tmp/reference_tui.pid",
    "/tmp/opencode_reference.pid",
    "/tmp/opencode_reference.log",
    "/tmp/opencode_reference_workdir",
]


def _kill_by_pidfile(pidfile):
    if not os.path.isfile(pidfile):
        return
    try:
        with open(pidfile) as f:
            pid = int(f.read().strip())
    except (ValueError, OSError):
        return

    try:
        os.kill(pid, signal.SIGTERM)
        for _ in range(30):
            time.sleep(0.1)
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
        os.kill(pid, signal.SIGKILL)
    except (OSError, ProcessLookupError):
        pass


def _kill_by_pattern(pattern):
    r = subprocess.run(
        ["pgrep", "-f", pattern, "-n"],
        capture_output=True, text=True,
    )
    if r.returncode != 0 or not r.stdout.strip():
        return
    for pid in r.stdout.strip().split():
        try:
            os.kill(int(pid), signal.SIGTERM)
        except (ValueError, OSError, ProcessLookupError):
            pass


def main():
    parser = argparse.ArgumentParser(
        description="Stop reference TUI and OpenCode server, clean up"
    )
    parser.parse_args()

    _kill_by_pattern(f"opencode attach {URL}")
    _kill_by_pattern(f"opencode serve --port 7775")
    _kill_by_pidfile(PID_FILE_TUI)
    _kill_by_pidfile(PID_FILE_OPENCODE)

    subprocess.run(["tmux", "kill-session", "-t", SESSION],
                   capture_output=True)

    if os.path.isfile(WORKDIR_FILE):
        with open(WORKDIR_FILE) as f:
            workdir = f.read().strip()
        if workdir and os.path.isdir(workdir):
            subprocess.run(["rm", "-rf", workdir], capture_output=True)

    for f in TEMP_FILES:
        if os.path.isfile(f):
            os.remove(f)

    print("Stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
