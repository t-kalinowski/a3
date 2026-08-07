"""Tests for the issue registry and JSON Pointer construction."""

from __future__ import annotations

import pytest

from rtemis.a3 import A3IssueCode
from rtemis.a3.issues import A3Issue, escape_token, pointer


class TestEscapeToken:
    """RFC 6901 escapes exactly two characters, in a specific order."""

    def test_escapes_the_two_reserved_characters(self):
        assert escape_token("a/b") == "a~1b"
        assert escape_token("a~b") == "a~0b"

    def test_replaces_tilde_before_slash(self):
        # Replacing in the other order would turn "a/b" into "a~01b": the "~1"
        # the slash produced would itself be re-escaped.
        assert escape_token("a/b") == "a~1b"
        assert escape_token("~/") == "~0~1"

    @pytest.mark.parametrize(
        "name", ["a.b", "$schema", "Domain (R1, R3, R4)", "β-sheet", "a[0]", ""]
    )
    def test_leaves_every_other_character_alone(self, name):
        assert escape_token(name) == name


class TestPointer:
    def test_escapes_the_token_it_is_given(self):
        assert pointer("/annotations/site", "a/b") == "/annotations/site/a~1b"
        assert pointer("/annotations/site", "a~b") == "/annotations/site/a~0b"
        assert pointer("", "$schema") == "/$schema"

    def test_takes_a_raw_member_name_not_a_pre_escaped_one(self):
        # The trap this pins: `pointer` escapes internally, so handing it an
        # already-escaped token escapes it a second time. Callers pass raw names.
        assert pointer("", "a~1b") == "/a~01b"
        assert pointer("", "a~0b") == "/a~00b"

    def test_does_not_escape_an_array_subscript(self):
        assert pointer("/annotations/variant", 3) == "/annotations/variant/3"
        assert pointer("/annotations/site/s/index", 0) == "/annotations/site/s/index/0"

    def test_renders_an_empty_member_name_as_a_trailing_empty_token(self):
        # A pointer ending in an empty reference token denotes the member named
        # "", which is exactly what A3E_NAME_EMPTY reports.
        assert pointer("/annotations/site", "") == "/annotations/site/"

    def test_a_bool_takes_the_subscript_branch(self):
        # `bool` is a subclass of `int` in Python, so a bool takes the subscript
        # branch and formats as "True" rather than "1". Nonsense either way —
        # tokens are annotation names (str) or loop indices (int) — but pinned
        # so the behaviour is known rather than discovered.
        assert pointer("", True) == "/True"


class TestRegistry:
    def test_every_code_has_a_stage_in_1_to_4(self):
        for code in A3IssueCode:
            assert 1 <= code.stage <= 4, code

    def test_has_37_codes(self):
        assert len(list(A3IssueCode)) == 37

    def test_a_code_compares_equal_to_its_wire_string(self):
        # StrEnum, so consumers can match without an explicit conversion.
        assert A3IssueCode.SEQ_CHARSET == "A3E_SEQ_CHARSET"
        assert str(A3IssueCode.POS_OUT_OF_BOUNDS) == "A3E_POS_OUT_OF_BOUNDS"


class TestA3Issue:
    def test_reports_its_stage_from_its_code(self):
        issue = A3Issue(
            A3IssueCode.POS_OUT_OF_BOUNDS, "/annotations/site/s/index/0", "…"
        )
        assert issue.stage == 4

    def test_names_the_whole_document_rather_than_printing_an_empty_path(self):
        # The empty pointer denotes the whole document; writing it as "/" would
        # be wrong, since that is the member named "".
        issue = A3Issue(A3IssueCode.DOC_NOT_OBJECT, "", "must be a JSON object")
        assert "<document>" in str(issue)
        assert str(issue).startswith("[A3E_DOC_NOT_OBJECT]")
