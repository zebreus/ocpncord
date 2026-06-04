#!/usr/bin/env python3
"""Send keystrokes to the TUI via tmux send-keys. Use --help for syntax."""

import subprocess
import sys
import unittest

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

# tmux send-keys uses its own names for some keys. Anything not listed here is
# already a valid tmux key name and is passed through unchanged (Enter, Escape,
# Tab, Space, arrows, Home, End, PageUp, PageDown, F1-F12, C-a..C-z).
TMUX_KEY_NAMES = {
    "Backspace": "BSpace",
    "Delete": "DC",
}


def tmux_key_name(name):
    """Translate a recognized key name to the name tmux send-keys expects."""
    return TMUX_KEY_NAMES.get(name, name)

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
                ["tmux", "send-keys", "-t", target, tmux_key_name(value)],
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


# --- Tests -----------------------------------------------------------------

def fmt(actions):
    parts = []
    for t, v in actions:
        if t == "key":
            parts.append(f"<{v}>")
        else:
            parts.append(repr(v))
    return " + ".join(parts)


class TestParseKeys(unittest.TestCase):

    # --- Literal text (no brackets) ---

    def test_empty_string(self):
        self.assertEqual(parse_keys(""), [])

    def test_literal_simple_text(self):
        self.assertEqual(parse_keys("hello"), [("literal", "hello")])

    def test_literal_with_spaces(self):
        self.assertEqual(parse_keys("hello world"),
                         [("literal", "hello world")])

    def test_literal_standalone_angles_no_escape(self):
        self.assertEqual(parse_keys("<<>>"), [("literal", "<<>>")])

    def test_literal_left_double_not_escape(self):
        self.assertEqual(parse_keys("<<x"), [("literal", "<<x")])

    def test_literal_right_double_not_escape(self):
        self.assertEqual(parse_keys("x>>"), [("literal", "x>>")])

    def test_literal_standalone_right_angle(self):
        self.assertEqual(parse_keys("a>b"), [("literal", "a>b")])

    def test_literal_many_angles_no_keywords(self):
        self.assertEqual(parse_keys("<<<<>>>>"), [("literal", "<<<<>>>>")])

    # --- Single keypress via <...> ---

    def test_key_enter(self):
        self.assertEqual(parse_keys("<Enter>"), [("key", "Enter")])

    def test_key_escape(self):
        self.assertEqual(parse_keys("<Escape>"), [("key", "Escape")])

    def test_key_tab(self):
        self.assertEqual(parse_keys("<Tab>"), [("key", "Tab")])

    def test_key_backspace(self):
        self.assertEqual(parse_keys("<Backspace>"), [("key", "Backspace")])

    def test_key_space(self):
        self.assertEqual(parse_keys("<Space>"), [("key", "Space")])

    def test_key_up(self):
        self.assertEqual(parse_keys("<Up>"), [("key", "Up")])

    def test_key_down(self):
        self.assertEqual(parse_keys("<Down>"), [("key", "Down")])

    def test_key_left(self):
        self.assertEqual(parse_keys("<Left>"), [("key", "Left")])

    def test_key_right(self):
        self.assertEqual(parse_keys("<Right>"), [("key", "Right")])

    def test_key_home(self):
        self.assertEqual(parse_keys("<Home>"), [("key", "Home")])

    def test_key_end(self):
        self.assertEqual(parse_keys("<End>"), [("key", "End")])

    def test_key_pageup(self):
        self.assertEqual(parse_keys("<PageUp>"), [("key", "PageUp")])

    def test_key_pagedown(self):
        self.assertEqual(parse_keys("<PageDown>"), [("key", "PageDown")])

    def test_key_delete(self):
        self.assertEqual(parse_keys("<Delete>"), [("key", "Delete")])

    def test_key_f1(self):
        self.assertEqual(parse_keys("<F1>"), [("key", "F1")])

    def test_key_f12(self):
        self.assertEqual(parse_keys("<F12>"), [("key", "F12")])

    def test_key_ctrl_c(self):
        self.assertEqual(parse_keys("<C-c>"), [("key", "C-c")])

    def test_key_ctrl_x(self):
        self.assertEqual(parse_keys("<C-x>"), [("key", "C-x")])

    def test_key_ctrl_z(self):
        self.assertEqual(parse_keys("<C-z>"), [("key", "C-z")])

    def test_all_ctrl_keys_recognized(self):
        for c in "abcdefghijklmnopqrstuvwxyz":
            name = f"C-{c}"
            self.assertIn(name, RECOGNIZED_KEYS)
            self.assertEqual(parse_keys(f"<{name}>"), [("key", name)])

    # --- Non-recognized key names: typed literally WITH brackets ---

    def test_unrecognized_single_char_x(self):
        self.assertEqual(parse_keys("<x>"), [("literal", "<x>")])

    def test_unrecognized_word(self):
        self.assertEqual(parse_keys("<foobar>"), [("literal", "<foobar>")])

    def test_unrecognized_number(self):
        self.assertEqual(parse_keys("<123>"), [("literal", "<123>")])

    # --- Empty <...> types <> ---

    def test_empty_brackets(self):
        self.assertEqual(parse_keys("<>"), [("literal", "<>")])

    # --- Invalid / malformed angle brackets are typed literally ---

    def test_left_double_then_right(self):
        self.assertEqual(parse_keys("<<>"), [("literal", "<<>")])

    def test_left_then_double_right(self):
        self.assertEqual(parse_keys("<>>"), [("literal", "<>>")])

    def test_double_left_with_space(self):
        self.assertEqual(parse_keys("<< >>"), [("literal", "<< >>")])

    def test_triple_left_then_enter_then_triple_right(self):
        self.assertEqual(
            parse_keys("<<<Enter>>>"),
            [("literal", "<<Enter>>")],
        )

    # --- Mixed literal text and keypresses ---

    def test_text_then_key_then_text(self):
        self.assertEqual(
            parse_keys("hello<Enter>world"),
            [("literal", "hello"), ("key", "Enter"), ("literal", "world")],
        )

    def test_multiple_keys_in_sequence(self):
        self.assertEqual(
            parse_keys("<Up><Down><Left><Right>"),
            [("key", "Up"), ("key", "Down"), ("key", "Left"), ("key", "Right")],
        )

    def test_literal_then_key(self):
        self.assertEqual(
            parse_keys("abc<Enter>"),
            [("literal", "abc"), ("key", "Enter")],
        )

    def test_key_then_literal(self):
        self.assertEqual(
            parse_keys("<Enter>abc"),
            [("key", "Enter"), ("literal", "abc")],
        )

    def test_space_key_expansion(self):
        self.assertEqual(
            parse_keys("hello<Space><Enter><Space>world"),
            [
                ("literal", "hello"),
                ("key", "Space"),
                ("key", "Enter"),
                ("key", "Space"),
                ("literal", "world"),
            ],
        )

    # --- <<KEY>> escape: types <KEY> literally ---

    def test_type_literal_enter(self):
        self.assertEqual(
            parse_keys("<<Enter>>"),
            [("literal", "<Enter>")],
        )

    def test_type_literal_ctrl_c(self):
        self.assertEqual(
            parse_keys("<<C-c>>"),
            [("literal", "<C-c>")],
        )

    # --- Unclosed brackets: typed literally ---

    def test_unclosed_bracket_simple(self):
        self.assertEqual(parse_keys("<x"), [("literal", "<x")])

    def test_unclosed_bracket_empty(self):
        self.assertEqual(parse_keys("<"), [("literal", "<")])

    def test_unclosed_double_left(self):
        self.assertEqual(parse_keys("<< "), [("literal", "<< ")])

    # --- Edge cases ---

    def test_only_left_bracket(self):
        self.assertEqual(parse_keys("<"), [("literal", "<")])

    def test_only_right_bracket(self):
        self.assertEqual(parse_keys(">"), [("literal", ">")])

    def test_double_left_then_double_right(self):
        self.assertEqual(parse_keys("<<>>"), [("literal", "<<>>")])

    def test_invalid_key_typed_with_brackets(self):
        self.assertEqual(parse_keys("<foo>"), [("literal", "<foo>")])

    def test_nested_like_angles(self):
        self.assertEqual(
            parse_keys("<<<Enter>>>"),
            [("literal", "<<Enter>>")],
        )

    def test_multiple_ctrl_keys(self):
        self.assertEqual(
            parse_keys("<C-c><C-x><C-z>"),
            [("key", "C-c"), ("key", "C-x"), ("key", "C-z")],
        )

    def test_complex_interleaved(self):
        result = parse_keys("run <Enter> cd<Space>/tmp<Enter> ls<Enter>")
        self.assertEqual(
            result,
            [
                ("literal", "run "),
                ("key", "Enter"),
                ("literal", " cd"),
                ("key", "Space"),
                ("literal", "/tmp"),
                ("key", "Enter"),
                ("literal", " ls"),
                ("key", "Enter"),
            ],
        )

    def test_very_complex_alternating(self):
        result = parse_keys("<Enter><C-c>hello<Up>world<Down>")
        self.assertEqual(
            result,
            [
                ("key", "Enter"),
                ("key", "C-c"),
                ("literal", "hello"),
                ("key", "Up"),
                ("literal", "world"),
                ("key", "Down"),
            ],
        )

    # --- tmux key-name translation ---

    def test_tmux_name_backspace(self):
        self.assertEqual(tmux_key_name("Backspace"), "BSpace")

    def test_tmux_name_delete(self):
        self.assertEqual(tmux_key_name("Delete"), "DC")

    def test_tmux_name_passthrough(self):
        for name in ("Enter", "Escape", "Tab", "Space", "Up", "Down",
                     "Left", "Right", "Home", "End", "PageUp", "PageDown",
                     "F1", "F12", "C-c", "C-x"):
            self.assertEqual(tmux_key_name(name), name)

    def test_every_recognized_key_maps_to_nonempty(self):
        for name in RECOGNIZED_KEYS:
            self.assertTrue(tmux_key_name(name))

    def test_recognized_key_set_has_all_expected(self):
        expected = {
            "Enter", "Escape", "Tab", "Backspace", "Space",
            "Up", "Down", "Left", "Right",
            "Home", "End", "PageUp", "PageDown",
            "Delete",
        }
        expected.update(f"F{i}" for i in range(1, 13))
        expected.update(f"C-{c}" for c in "abcdefghijklmnopqrstuvwxyz")
        self.assertEqual(RECOGNIZED_KEYS, frozenset(expected))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--test":
        sys.argv.pop(1)
        unittest.main()
    else:
        sys.exit(main())
