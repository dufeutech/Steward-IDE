"""The keystroke spec and crop geometry — the parts that can be wrong without a window.

Everything else in this tool is a call into user32, where the only honest test is driving
a real window. These are the rules a typo in a spec runs into first.
"""

from __future__ import annotations

import pytest

from appdrive.core import Box, Chord, SpecError, parse


def test_plain_text_is_one_chord_per_character():
    assert parse("cls") == (Chord("c"), Chord("l"), Chord("s"))


def test_a_named_key_is_one_chord():
    assert parse("{ENTER}") == (Chord("ENTER", named=True),)


def test_named_keys_are_case_insensitive():
    assert parse("{enter}") == parse("{ENTER}")


def test_a_modifier_binds_only_to_the_next_key():
    assert parse("^ab") == (Chord("a", ctrl=True), Chord("b"))


def test_modifiers_stack():
    # The chord that shows and hides the terminal panel.
    assert parse("^+`") == (Chord("`", ctrl=True, shift=True),)


def test_a_modifier_binds_to_a_named_key_too():
    assert parse("%{TAB}") == (Chord("TAB", named=True, alt=True),)


def test_a_single_character_in_braces_is_that_character():
    # How a spec types a modifier sign without meaning the modifier.
    assert parse("{^}{+}{%}") == (Chord("^"), Chord("+"), Chord("%"))


def test_braces_can_be_typed_literally():
    assert parse("{{}") == (Chord("{"),)
    assert parse("{}}") == (Chord("}"),)


def test_a_dangling_modifier_is_refused():
    # Caught before anything is sent, so a typo cannot half-type a chord into a live window.
    with pytest.raises(SpecError, match="holds nothing"):
        parse("cls^")


def test_an_unknown_named_key_is_refused_with_the_alternatives():
    with pytest.raises(SpecError, match="ENTER"):
        parse("{RETURN}")


def test_an_unclosed_brace_is_refused():
    with pytest.raises(SpecError, match="unclosed"):
        parse("{ENTER")


def test_a_stray_closing_brace_is_refused():
    with pytest.raises(SpecError, match="unbalanced"):
        parse("a}b")


def test_a_crop_inside_the_image_is_unchanged():
    assert Box(10, 20, 30, 40).clamped((100, 100)) == Box(10, 20, 30, 40)


def test_a_crop_running_off_the_edge_is_trimmed_not_refused():
    # The normal case when a region is read back from a window that has since been resized.
    assert Box(90, 90, 50, 50).clamped((100, 100)) == Box(90, 90, 10, 10)


def test_a_crop_entirely_outside_the_image_is_an_error():
    with pytest.raises(ValueError, match="outside"):
        Box(200, 200, 10, 10).clamped((100, 100))
