#!/usr/bin/env python3
"""Print the OpenCode server log file to stdout."""

import argparse
import sys

LOG_FILE = "/tmp/opencode_reference.log"


def main():
    parser = argparse.ArgumentParser(
        description="Print the OpenCode server log"
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
