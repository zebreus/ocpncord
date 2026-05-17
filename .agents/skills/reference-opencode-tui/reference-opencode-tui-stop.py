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

TEMP_FILES = [
    "/tmp/reference_tui.pid",
    "/tmp/opencode_reference.pid",
    "/tmp/opencode_reference.log",
    "/tmp/opencode_reference_workdir",
]


def kill_pid(pidfile):
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
    except ProcessLookupError:
        pass
    except OSError:
        pass


def main():
    parser = argparse.ArgumentParser(
        description="Stop reference TUI and OpenCode server, clean up"
    )
    parser.parse_args()

    kill_pid(PID_FILE_TUI)
    kill_pid(PID_FILE_OPENCODE)

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
