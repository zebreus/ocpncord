#!/usr/bin/env python3
"""Trigger a TUI screenshot via SIGUSR1 and retrieve the file."""

import argparse
import glob
import os
import signal
import sys
import time

PID_FILE_TUI = "/tmp/tui.pid"
SCREENSHOT_GLOB = "/tmp/*-screenshot.txt"


def main():
    parser = argparse.ArgumentParser(
        description="Trigger a TUI screenshot and retrieve the file"
    )
    parser.parse_args()

    if not os.path.isfile(PID_FILE_TUI):
        print("ERROR: TUI PID file not found (is the TUI running?)")
        return 1

    try:
        with open(PID_FILE_TUI) as f:
            pid = int(f.read().strip())
    except (ValueError, OSError) as e:
        print(f"ERROR: cannot read TUI PID: {e}")
        return 1

    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        print("ERROR: TUI process is not running")
        return 1

    before = set(glob.glob(SCREENSHOT_GLOB))

    try:
        os.kill(pid, signal.SIGUSR1)
    except ProcessLookupError:
        print("ERROR: TUI process died while sending signal")
        return 1
    except PermissionError:
        print("ERROR: permission denied sending SIGUSR1")
        return 1

    new_file = None
    for _ in range(50):
        time.sleep(0.1)
        after = set(glob.glob(SCREENSHOT_GLOB))
        diff = after - before
        if diff:
            new_file = sorted(diff)[-1]
            break

    if new_file is None:
        print("ERROR: screenshot file did not appear within 5 seconds")
        return 1

    with open(new_file) as f:
        sys.stdout.write(f.read())
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
