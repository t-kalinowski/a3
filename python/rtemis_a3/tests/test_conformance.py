"""Conformance runner: checks this implementation against the shared corpus.

The corpus lives at ``spec/conformance/`` in the repository root and is read by
all five A3 implementations. It is never vendored into a language directory —
one corpus, five readers, or it drifts.

See ``spec/conformance/README.md`` for the assertion semantics implemented here.
"""

from __future__ import annotations

import json
from collections import Counter
from pathlib import Path
from typing import Any

import pytest

from rtemis.a3 import A3IssueCode, A3ValidationError, a3_from_json, a3_to_json

# tests/ -> rtemis_a3/ -> python/ -> <repo>
CORPUS = Path(__file__).resolve().parents[3] / "spec" / "conformance"

pytestmark = pytest.mark.skipif(
    not (CORPUS / "manifest.json").is_file(),
    reason=f"conformance corpus not found at {CORPUS}",
)


def _manifest() -> dict[str, Any]:
    return json.loads((CORPUS / "manifest.json").read_text(encoding="utf-8"))


def _cases() -> list[dict[str, Any]]:
    if not (CORPUS / "manifest.json").is_file():
        return []
    return _manifest()["cases"]


def _load(rel: str) -> Any:
    return json.loads((CORPUS / rel).read_text(encoding="utf-8"))


def _tally(pairs) -> Counter:
    """Count each ``(code, path)`` pair.

    Issues are compared as a **multiset**: a rule can legitimately fire more
    than once at the same path (two repeated positions in one index produce two
    ``A3E_POS_DUPLICATE`` issues), and an implementation that reports one of
    them has not conformed. Order is not compared.
    """
    return Counter(pairs)


def _diff(expected: Counter, actual: Counter) -> str:
    """Describe the difference between two issue multisets."""
    lines = []
    for pair, want in sorted(expected.items()):
        got = actual.get(pair, 0)
        if got < want:
            lines.append(
                f"  missing:    {pair[0]:<34} {pair[1]}  (expected {want}, got {got})"
            )
    for pair, got in sorted(actual.items()):
        want = expected.get(pair, 0)
        if got > want:
            lines.append(
                f"  unexpected: {pair[0]:<34} {pair[1]}  (expected {want}, got {got})"
            )
    return "\n".join(lines)


def _ordered_key_repr(value: Any) -> Any:
    """Project a JSON value so that ``==`` treats object key order as significant.

    ``json.loads`` preserves member order in a ``dict``, but ``dict.__eq__``
    ignores it. Comparing a list of items instead makes order part of the
    comparison, which is what turns these cases into a normalization test rather
    than merely a parsing test.
    """
    if isinstance(value, dict):
        return [(k, _ordered_key_repr(v)) for k, v in value.items()]
    if isinstance(value, list):
        return [_ordered_key_repr(v) for v in value]
    return value


def _case_id(case: dict[str, Any]) -> str:
    return case["file"]


@pytest.mark.parametrize("case", _cases(), ids=_case_id)
def test_conformance_case(case: dict[str, Any]) -> None:
    """Each corpus case, as its own test, so failures name the case."""
    text = (CORPUS / case["file"]).read_text(encoding="utf-8")

    if case["valid"]:
        try:
            a3 = a3_from_json(text)
        except A3ValidationError as exc:
            pytest.fail(f"{case['description']}\n  expected valid, got:\n{exc}")

        if "normalized" not in case:
            return

        expected = _load(case["normalized"])
        produced = json.loads(a3_to_json(a3))
        assert _ordered_key_repr(produced) == _ordered_key_repr(expected), (
            f"{case['description']}\n  round-trip differs from {case['normalized']}"
        )
        return

    with pytest.raises(A3ValidationError) as caught:
        a3_from_json(text)
    exc = caught.value

    expected = _tally((i["code"], i["path"]) for i in case["issues"])
    actual = _tally((str(issue.code), issue.path) for issue in exc.issues)
    assert expected == actual, f"{case['description']}\n{_diff(expected, actual)}"

    # Every issue in one error comes from a single stage, and the manifest
    # records which. A mismatch means the staging is wrong even though the issue
    # set happens to agree.
    if "stage" in case:
        assert exc.stage == case["stage"], (
            f"{case['description']}\n  expected stage {case['stage']}, got {exc.stage}"
        )


def test_every_code_has_a_corpus_case() -> None:
    """Every code in the registry must be exercised by at least one case.

    Without this, the registry and the corpus drift within two months: a code is
    added, no case is written, and nothing notices.
    """
    covered = {
        issue["code"]
        for case in _manifest()["cases"]
        if not case["valid"]
        for issue in case["issues"]
    }
    known = {str(code) for code in A3IssueCode}

    assert not (known - covered), (
        f"codes with no corpus case: {sorted(known - covered)}"
    )
    assert not (covered - known), (
        f"corpus expects codes not in the registry: {sorted(covered - known)}"
    )


def test_every_code_has_a_stage() -> None:
    """Every code must declare a stage in 1-4."""
    for code in A3IssueCode:
        assert 1 <= code.stage <= 4, f"{code} has stage {code.stage}"
