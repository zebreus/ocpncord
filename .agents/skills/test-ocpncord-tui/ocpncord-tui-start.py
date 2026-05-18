#!/usr/bin/env python3
"""Start OpenCode server and TUI in a tmux session."""

import argparse
import os
import subprocess
import sys
import time
import socket

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, "..", "..", ".."))
PID_FILE_TUI = "/tmp/tui.pid"
PID_FILE_OPENCODE = "/tmp/opencode.pid"
LOG_OPENCODE = "/tmp/opencode.log"
LOG_TUI_INTERNAL = "/tmp/ocpncord.log"
WORKDIR_FILE = "/tmp/opencode_workdir"
SESSION = "tui_session"
URL = "http://localhost:7774"

# Resolve absolute path to avoid tmux-shell PATH issues
_which = subprocess.run(
    ["which", "opencode"], capture_output=True, text=True
)
if _which.returncode != 0 or not _which.stdout.strip():
    print("ERROR: opencode binary not found on PATH", file=sys.stderr)
    sys.exit(1)
OPENCODE_BIN = os.path.realpath(_which.stdout.strip())


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


def _process_running(pidfile, cmd_pattern=None):
    if not os.path.isfile(pidfile):
        return None
    try:
        with open(pidfile) as f:
            pid = int(f.read().strip())
    except (ValueError, OSError):
        return None
    try:
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return None
    if cmd_pattern is not None and not _cmd_matches(pid, cmd_pattern):
        return None
    return pid


def _kill_procs(pattern):
    r = subprocess.run(
        ["pgrep", "-f", pattern, "-n"],
        capture_output=True, text=True,
    )
    if r.returncode != 0 or not r.stdout.strip():
        return
    for pid in r.stdout.strip().split():
        try:
            os.kill(int(pid), 15)
        except (ValueError, OSError, ProcessLookupError):
            pass


def read_pid(pidfile):
    if not os.path.isfile(pidfile):
        return None
    with open(pidfile) as f:
        return int(f.read().strip())


def cleanup():
    subprocess.run(["tmux", "kill-session", "-t", SESSION],
                   capture_output=True)
    _kill_procs("opencode serve --port 7774")
    _kill_procs("ocpncord-native")
    TEMP_FILES = [
        PID_FILE_TUI,
        PID_FILE_OPENCODE,
        LOG_OPENCODE,
        LOG_TUI_INTERNAL,
        WORKDIR_FILE,
    ]
    for f in TEMP_FILES:
        if os.path.isfile(f):
            os.remove(f)
    if os.path.isfile(WORKDIR_FILE):
        with open(WORKDIR_FILE) as f:
            workdir = f.read().strip()
        if workdir and os.path.isdir(workdir):
            subprocess.run(["rm", "-rf", workdir], capture_output=True)
        os.remove(WORKDIR_FILE)


def main():
    parser = argparse.ArgumentParser(
        description="Start OpenCode server and TUI in tmux"
    )
    args = parser.parse_args()

    svr = _process_running(PID_FILE_OPENCODE, "opencode serve --port 7774")
    tui = _process_running(PID_FILE_TUI, "ocpncord-native")
    if svr is not None and tui is not None:
        print("Already Running")
        return 0

    cleanup()

    workdir = subprocess.run(
        ["mktemp", "-d", "-p", "/tmp", "test-tui-XXXXXX"],
        capture_output=True, text=True, check=True
    ).stdout.strip()
    with open(WORKDIR_FILE, "w") as f:
        f.write(workdir)

    subprocess.run(
        ["tmux", "new-session", "-d", "-s", SESSION, "-n", "opencode"],
        check=True,
    )
    subprocess.run(
        ["tmux", "set-window-option", "-t", f"{SESSION}:opencode", "window-size", "manual"],
        capture_output=True,
    )

    opencode_cmd = (
        f"cd {workdir}"
        f" && {OPENCODE_BIN} serve --port 7774"
        f" --print-logs --log-level INFO 2>&1"
        f" | while IFS= read -r line; do"
        f" echo \"[$(date '+%H:%M:%S.%4N') OPENCODE] $line\"; done"
        f" > {LOG_OPENCODE}"
    )
    subprocess.run(
        ["tmux", "send-keys", "-t", f"{SESSION}:opencode", opencode_cmd, "Enter"],
        check=True,
    )

    opencode_pid = None
    for _ in range(15):
        time.sleep(0.3)
        result = subprocess.run(
            ["pgrep", "-f", f"opencode serve --port 7774", "-n"],
            capture_output=True, text=True,
        )
        if result.returncode == 0 and result.stdout.strip():
            cand = result.stdout.strip().split('\n')[-1]
            try:
                os.kill(int(cand), 0)
                opencode_pid = cand
                break
            except (ValueError, OSError, ProcessLookupError):
                continue
    if opencode_pid is None:
        print("ERROR: could not determine OpenCode server PID")
        cleanup()
        return 1
    with open(PID_FILE_OPENCODE, "w") as f:
        f.write(opencode_pid)

    server_up = False
    for _ in range(25):
        try:
            with socket.create_connection((socket.gethostbyname("localhost"), 7774), timeout=1):
                server_up = True
                break
        except OSError:
            time.sleep(0.2)
    if not server_up:
        print(f"ERROR: OpenCode server not reachable at {URL}")
        cleanup()
        return 1

    subprocess.run(
        ["tmux", "new-window", "-t", SESSION, "-n", "tui"],
        check=True,
    )
    subprocess.run(
        ["tmux", "set-window-option", "-t", f"{SESSION}:tui", "window-size", "manual"],
        capture_output=True,
    )

    tui_cmd = (
        f"cd {REPO_ROOT} && cargo run -- --url {URL}"
    )
    subprocess.run(
        ["tmux", "send-keys", "-t", f"{SESSION}:tui", tui_cmd, "Enter"],
        check=True,
    )

    tui_pid = None
    for _ in range(100):
        result = subprocess.run(
            ["pgrep", "-f", "ocpncord-native", "-n"],
            capture_output=True, text=True,
        )
        if result.returncode == 0 and result.stdout.strip():
            tui_pid = result.stdout.strip()
            with open(PID_FILE_TUI, "w") as f:
                f.write(tui_pid)
            break
        time.sleep(0.1)
    if tui_pid is None:
        print("ERROR: TUI process did not start within 10 seconds")
        cleanup()
        return 1

    if _process_running(PID_FILE_TUI, "ocpncord-native"):
        print("Started")
        return 0
    else:
        print("ERROR: TUI process died shortly after starting")
        cleanup()
        return 1


if __name__ == "__main__":
    sys.exit(main())
