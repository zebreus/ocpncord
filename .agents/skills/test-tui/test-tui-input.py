#!/usr/bin/env python3
"""Send keystrokes to the TUI via tmux send-keys. Use --help for syntax."""

import subprocess
import sys

SESSION = "tui_session"
WINDOW = "tui"
TARGET = f"{SESSION}:{WINDOW}"

RECOGNIZED_KEYS = frozenset({
    "Enter", "Escape", "Tab", "Backspace", "Space",
    "Up", "Down", "Left", "Right",
    "Home", "End", "PageUp", "PageDown",
    "Delete",
    *[f"F{i}" for i in range(1, 13)],
    *[f"C-{c}" for c in "abcdefghijklmnopqrstuvwxyz"],
})

DESCRIPTION = """Send keystrokes to the TUI via tmux send-keys.

Recognized key names for <...>:

  Special:              Enter, Escape, Tab, Backspace, Space
  Arrow keys:           Up, Down, Left, Right
  Navigation:           Home, End, PageUp, PageDown
  Editing:              Delete
  Function keys:        F1 through F12
  Ctrl combos:          C-a through C-z (e.g. <C-c>, <C-x>)

Outside <...> text is typed literally.
Inside <...>:
  <name> sends a keypress if name is a recognized key.
  <<name>> types "<name>" literally (escape).
  Any other <...> is typed literally, brackets included.

Examples:
  Type hello then press Enter:             --keys "hello<Enter>"
  Press Ctrl+X then h for help:            --keys "<C-x>h"
  Type "<Enter>" as literal text:          --keys "<<Enter>>"
  Press Ctrl+C:                            --keys "<C-c>"
  Type "x", press Down, type "y":          --keys "x<Down>y"
"""


def parse_keys(input_str):
    """Parse ``input_str`` into a list of actions.

    Returns ``[("literal", str), ("key", str), ...]``.

    ``<key>`` sends a keypress if ``key`` is recognized.
    ``<<key>>`` types ``<key>`` literally.
    Any other ``<...>`` is typed literally, brackets included.
    """
    result = []
    i = 0
    while i < len(input_str):
        if input_str[i] == '<':
            match = None
            for j in range(i + 1, len(input_str)):
                if input_str[j] == '>':
                    content = input_str[i + 1:j]
                    if content in RECOGNIZED_KEYS:
                        match = ("key", content, j + 1)
                        break
                    if content.startswith('<') and content.endswith('>') and content[1:-1] in RECOGNIZED_KEYS:
                        match = ("literal", "<" + content[1:-1] + ">", j + 1)
                        break
            if match:
                result.append((match[0], match[1]))
                i = match[2]
            else:
                result.append(("literal", input_str[i]))
                i += 1
        else:
            result.append(("literal", input_str[i]))
            i += 1
    return _merge_literals(result)


def _merge_literals(actions):
    merged = []
    for action in actions:
        if action[0] == "literal" and merged and merged[-1][0] == "literal":
            merged[-1] = ("literal", merged[-1][1] + action[1])
        else:
            merged.append(action)
    return merged


def send_actions(actions, target):
    """Send a sequence of actions to a tmux pane."""
    for action_type, value in actions:
        if action_type == "key":
            subprocess.run(
                ["tmux", "send-keys", "-t", target, value],
                check=True,
                capture_output=True,
            )
        else:
            subprocess.run(
                ["tmux", "send-keys", "-l", "-t", target, value],
                check=True,
                capture_output=True,
            )


def main():
    argv = sys.argv[1:]

    if "--help" in argv or "-h" in argv:
        print(DESCRIPTION.strip())
        return 0

    if not argv or argv[0] != "--keys":
        print("ERROR: --keys argument is required", file=sys.stderr)
        print(DESCRIPTION.strip(), file=sys.stderr)
        return 1

    input_str = " ".join(argv[1:])

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

    actions = parse_keys(input_str)
    send_actions(actions, TARGET)

    print(f"Sent: {input_str}")
    return 0


def parse_keys_test(input_str):
    """Wrapper for tests: returns a human-readable description of the parsed actions."""
    actions = parse_keys(input_str)
    parts = []
    for t, v in actions:
        if t == "key":
            parts.append(f"<{v}>")
        else:
            parts.append(repr(v))
    return " + ".join(parts)


if __name__ == "__main__":
    sys.exit(main())
