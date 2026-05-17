#!/usr/bin/env python3
"""Print the TUI debug log file to stdout."""

import argparse
import sys

LOG_FILE = "/tmp/ocpncord.log"


def main():
    parser = argparse.ArgumentParser(
        description="Print the TUI debug log"
    )
    parser.parse_args()

    try:
        with open(LOG_FILE) as f:
            sys.stdout.write(f.read())
    except FileNotFoundError:
        print(f"ERROR: log file not found at {LOG_FILE}", file=sys.stderr)
        return 1
    except OSError as e:
        print(f"ERROR: cannot read {LOG_FILE}: {e}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
