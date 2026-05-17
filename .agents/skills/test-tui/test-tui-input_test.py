"""Unit tests for test-tui-input.py parser.

Run:  python3 test-tui-input_test.py
"""

import importlib.util
import unittest
import sys
import os

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    "test_tui_input", os.path.join(_here, "test-tui-input.py")
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
parse_keys = _mod.parse_keys
RECOGNIZED_KEYS = _mod.RECOGNIZED_KEYS
fmt = _mod.parse_keys_test


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
    unittest.main()
