"""Four-stage A3 validation over the raw parsed JSON.

Validation runs against the plain ``dict`` that :func:`json.loads` produces,
not against a Pydantic model. That is deliberate: Pydantic's own
``ValidationError`` fires on shape problems and produces Pydantic-shaped
errors, not A3 issues, so letting the model see unvalidated input would mean
two error vocabularies escaping from one function.

**Within a stage, collect everything. Across stages, short-circuit.**

.. code-block:: text

    JSON text
       |
       +- json.loads ----------> A3ParseError (single, fatal, not collectable)
       v
    dict / list / scalar
       +- STAGE 1  envelope     document is an object, declares itself A3 1.0.0
       +- STAGE 2  structural   types, shapes, required members, charset, names
       +- STAGE 3  intra-field  positions >=1/deduped, ranges ordered/disjoint
       +- STAGE 4  contextual   bounds vs sequence length, variant `from` residue
       v  (only if no stage produced an issue)
    normalized dict -> A3 model

Short-circuiting is a correctness requirement, not an optimization. A document
whose ``sequence`` fails Stage 2 has no known length, so Stage 4 would emit one
spurious out-of-bounds issue per annotated position.

The full contract lives in ``spec/error-codes.md`` and ``spec/error-paths.md``.
"""

from __future__ import annotations

from typing import Any

from .issues import A3Issue, A3IssueCode as C, pointer

A3_SCHEMA_URI = "https://schema.rtemis.org/a3/v1/schema.json"
A3_VERSION = "1.0.0"

TOP_LEVEL_KEYS = ("$schema", "a3_version", "sequence", "annotations", "metadata")
FAMILIES = ("site", "region", "ptm", "processing", "variant")
NAMED_FAMILIES = ("site", "region", "ptm", "processing")
ENTRY_KEYS = ("index", "type")
METADATA_KEYS = ("uniprot_id", "description", "reference", "organism")

_ALLOWED_RESIDUES = frozenset("ABCDEFGHIJKLMNOPQRSTUVWXYZ*")


# ---------------------------------------------------------------------------
# Public entry point
# ---------------------------------------------------------------------------


def validate(raw: Any) -> tuple[dict[str, Any] | None, list[A3Issue]]:
    """Validate a raw parsed JSON value as an A3 document.

    Parameters
    ----------
    raw : Any
        The output of :func:`json.loads`. Any value, not necessarily a dict.

    Returns
    -------
    tuple[dict or None, list[A3Issue]]
        On success, the normalized document and an empty issue list. On
        failure, ``None`` and every issue found in the earliest stage that
        produced one.
    """
    issues: list[A3Issue] = []

    # ── Stage 1 ───────────────────────────────────────────────────────────
    _stage1_envelope(raw, issues)
    if issues:
        return None, issues
    obj: dict[str, Any] = raw

    # ── Stage 2 ───────────────────────────────────────────────────────────
    draft = _stage2_structural(obj, issues)
    if issues:
        return None, issues

    # ── Stage 3 ───────────────────────────────────────────────────────────
    _stage3_intra_field(draft, issues)
    if issues:
        return None, issues

    # ── Stage 4 ───────────────────────────────────────────────────────────
    _stage4_contextual(draft, issues)
    if issues:
        return None, issues

    return _build(draft), []


# ---------------------------------------------------------------------------
# Stage 1 — Envelope
# ---------------------------------------------------------------------------


def _stage1_envelope(raw: Any, issues: list[A3Issue]) -> None:
    """Check that the document is an object declaring itself to be A3 1.0.0.

    When the root is not an object, ``A3E_DOC_NOT_OBJECT`` is the only issue
    reported: nothing below it can be located.
    """
    if not isinstance(raw, dict):
        issues.append(
            A3Issue(
                C.DOC_NOT_OBJECT,
                "",
                f"an A3 document must be a JSON object, got {_render(raw)}",
            )
        )
        return

    _check_const(
        raw,
        "$schema",
        A3_SCHEMA_URI,
        C.ENVELOPE_SCHEMA_MISSING,
        C.ENVELOPE_SCHEMA_MISMATCH,
        issues,
    )
    _check_const(
        raw,
        "a3_version",
        A3_VERSION,
        C.ENVELOPE_VERSION_MISSING,
        C.ENVELOPE_VERSION_MISMATCH,
        issues,
    )


def _check_const(
    obj: dict[str, Any],
    key: str,
    expected: str,
    missing: C,
    mismatch: C,
    issues: list[A3Issue],
) -> None:
    """Check one envelope member against its required constant value.

    A wrong type and a wrong value are the same failure: the schema declares
    both members with ``const``, and a ``const`` failure is one failure.
    """
    path = pointer("", key)
    if key not in obj:
        issues.append(
            A3Issue(missing, path, f"'{key}' is required and must be '{expected}'")
        )
    elif obj[key] != expected:
        issues.append(
            A3Issue(
                mismatch, path, f"'{key}' must be '{expected}', got {_render(obj[key])}"
            )
        )


# ---------------------------------------------------------------------------
# Stage 2 — Structural
# ---------------------------------------------------------------------------


def _stage2_structural(obj: dict[str, Any], issues: list[A3Issue]) -> dict[str, Any]:
    """Check every value's type and shape, and extract a draft document.

    The draft carries values as plain Python ints, not yet range-checked, plus
    the JSON Pointer each one will be reported at.
    """
    _unknown_fields(obj, TOP_LEVEL_KEYS, "", issues)

    draft: dict[str, Any] = {
        "sequence": _stage2_sequence(obj, issues) or "",
        "site": {},
        "region": {},
        "ptm": {},
        "processing": {},
        "variant": [],
        "metadata": dict.fromkeys(METADATA_KEYS, ""),
    }

    # `annotations` and `metadata` are optional; both default to empty.
    annotations = obj.get("annotations")
    if annotations is not None:
        if not isinstance(annotations, dict):
            issues.append(
                A3Issue(
                    C.ANNOTATIONS_NOT_OBJECT,
                    "/annotations",
                    f"'annotations' must be an object, got {_render(annotations)}",
                )
            )
        else:
            _stage2_annotations(annotations, draft, issues)

    metadata = obj.get("metadata")
    if metadata is not None:
        if not isinstance(metadata, dict):
            issues.append(
                A3Issue(
                    C.METADATA_NOT_OBJECT,
                    "/metadata",
                    f"'metadata' must be an object, got {_render(metadata)}",
                )
            )
        else:
            draft["metadata"] = _stage2_metadata(metadata, issues)

    return draft


def _stage2_sequence(obj: dict[str, Any], issues: list[A3Issue]) -> str | None:
    """Extract and case-fold the sequence, checking its type, length, charset."""
    if "sequence" not in obj:
        issues.append(A3Issue(C.SEQ_MISSING, "/sequence", "'sequence' is required"))
        return None
    raw = obj["sequence"]
    if not isinstance(raw, str):
        issues.append(
            A3Issue(
                C.SEQ_NOT_STRING,
                "/sequence",
                f"'sequence' must be a string, got {_render(raw)}",
            )
        )
        return None

    upper = raw.upper()
    if len(upper) < 2:
        issues.append(
            A3Issue(
                C.SEQ_TOO_SHORT,
                "/sequence",
                f"'sequence' must be at least 2 characters, got {len(upper)} ({raw!r})",
            )
        )
    bad = next((c for c in upper if c not in _ALLOWED_RESIDUES), None)
    if bad is not None:
        issues.append(
            A3Issue(
                C.SEQ_CHARSET,
                "/sequence",
                f"'sequence' contains {bad!r}; only A-Z (IUPAC amino acid codes) "
                f"and '*' (stop codon) are permitted",
            )
        )
    return upper


def _stage2_annotations(
    annotations: dict[str, Any], draft: dict[str, Any], issues: list[A3Issue]
) -> None:
    """Parse the five annotation families."""
    _unknown_fields(annotations, FAMILIES, "/annotations", issues)

    for family in NAMED_FAMILIES:
        entries = annotations.get(family)
        if entries is None:
            continue
        family_path = pointer("/annotations", family)
        if not isinstance(entries, dict):
            issues.append(
                A3Issue(
                    C.FAMILY_NOT_OBJECT,
                    family_path,
                    f"'annotations.{family}' must be an object, got {_render(entries)}",
                )
            )
            continue
        draft[family] = _stage2_family(entries, family, family_path, issues)

    records = annotations.get("variant")
    if records is not None:
        if not isinstance(records, list):
            issues.append(
                A3Issue(
                    C.VARIANT_LIST_NOT_ARRAY,
                    "/annotations/variant",
                    f"'annotations.variant' must be an array, got {_render(records)}",
                )
            )
        else:
            draft["variant"] = _stage2_variants(records, issues)


def _stage2_family(
    entries: dict[str, Any], family: str, family_path: str, issues: list[A3Issue]
) -> dict[str, dict[str, Any]]:
    """Parse one named annotation family.

    ``family`` selects the permitted index geometry: ``site`` takes positions,
    ``region`` takes ranges, and ``ptm`` / ``processing`` take either,
    homogeneously.
    """
    out: dict[str, dict[str, Any]] = {}

    for name, raw in entries.items():
        entry_path = pointer(family_path, name)

        if not name:
            issues.append(
                A3Issue(
                    C.NAME_EMPTY,
                    entry_path,
                    f"'annotations.{family}' has an annotation named ''; "
                    f"names must not be empty",
                )
            )
            continue

        if not isinstance(raw, dict):
            issues.append(
                A3Issue(
                    C.ENTRY_NOT_OBJECT,
                    entry_path,
                    f"annotation {name!r} must be an object with an 'index' member, "
                    f"got {_render(raw)}",
                )
            )
            continue

        _unknown_fields(raw, ENTRY_KEYS, entry_path, issues)

        label = raw.get("type")
        if label is None:
            label = ""
        elif not isinstance(label, str):
            issues.append(
                A3Issue(
                    C.ENTRY_TYPE_NOT_STRING,
                    pointer(entry_path, "type"),
                    f"'type' must be a string, got {_render(label)}",
                )
            )
            label = ""

        index_path = pointer(entry_path, "index")
        if "index" not in raw:
            issues.append(
                A3Issue(
                    C.INDEX_MISSING,
                    index_path,
                    f"annotation {name!r} is missing its required 'index' member",
                )
            )
            continue
        elements = raw["index"]
        if not isinstance(elements, list):
            issues.append(
                A3Issue(
                    C.INDEX_NOT_ARRAY,
                    index_path,
                    f"'index' must be an array, got {_render(elements)}",
                )
            )
            continue

        parsed = _parse_index(elements, family, index_path, issues)
        if parsed is None:
            continue
        geometry, values = parsed
        out[name] = {
            "index_path": index_path,
            "geometry": geometry,
            "index": values,
            "type": label,
        }

    return out


def _parse_index(
    elements: list[Any], family: str, index_path: str, issues: list[A3Issue]
) -> tuple[str, list[Any]] | None:
    """Settle an index's geometry and extract its values.

    Returns ``None`` when the index could not be interpreted; issues have
    already been recorded in that case.
    """
    flexible = family in ("ptm", "processing")

    # Classify every element first. Doing this up front is what makes the
    # mixed-geometry rule expressible: it is a property of the whole index.
    kinds = [_classify(v) for v in elements]

    bad = False
    for i, kind in enumerate(kinds):
        if kind == "other":
            bad = True
            issues.append(
                A3Issue(
                    C.INDEX_ELEMENT_TYPE,
                    pointer(index_path, i),
                    f"index element must be {_expected_shape(family)}, "
                    f"got {_render(elements[i])}",
                )
            )
    if bad:
        return None

    n_int = kinds.count("integer")
    n_arr = kinds.count("array")

    if flexible:
        if n_int and n_arr:
            issues.append(
                A3Issue(
                    C.INDEX_MIXED,
                    index_path,
                    f"index mixes {n_int} position(s) and {n_arr} range(s); a ptm or "
                    f"processing index must be all positions or all ranges",
                )
            )
            return None
        geometry = "ranges" if n_arr else "positions"
    else:
        geometry = "ranges" if family == "region" else "positions"
        # A well-formed element of the *other* geometry is still wrong for a
        # family whose geometry the schema fixes.
        want = "array" if geometry == "ranges" else "integer"
        wrong = False
        for i, kind in enumerate(kinds):
            if kind != want:
                wrong = True
                issues.append(
                    A3Issue(
                        C.INDEX_ELEMENT_TYPE,
                        pointer(index_path, i),
                        f"index element must be {_expected_shape(family)}, "
                        f"got {_render(elements[i])}",
                    )
                )
        if wrong:
            return None

    if geometry == "positions":
        return "positions", [_as_integer(v) for v in elements]

    ranges: list[list[int]] = []
    ok = True
    for i, pair in enumerate(elements):
        elem_path = pointer(index_path, i)
        if len(pair) != 2:
            issues.append(
                A3Issue(
                    C.RANGE_ARITY,
                    elem_path,
                    f"a range must be a 2-element [start, end] array, "
                    f"got {len(pair)} element(s)",
                )
            )
            ok = False
            continue
        start, end = _as_integer(pair[0]), _as_integer(pair[1])
        if start is None or end is None:
            issues.append(
                A3Issue(
                    C.RANGE_ENDPOINT_NOT_INTEGER,
                    elem_path,
                    f"range endpoints must be integers, got "
                    f"[{_render(pair[0])}, {_render(pair[1])}]",
                )
            )
            ok = False
            continue
        ranges.append([start, end])
    return ("ranges", ranges) if ok else None


def _stage2_variants(records: list[Any], issues: list[A3Issue]) -> list[dict[str, Any]]:
    """Parse the variant list. Every member beyond ``position`` is open."""
    out: list[dict[str, Any]] = []

    for i, raw in enumerate(records):
        path = pointer("/annotations/variant", i)

        if not isinstance(raw, dict):
            issues.append(
                A3Issue(
                    C.VARIANT_NOT_OBJECT,
                    path,
                    f"a variant record must be an object, got {_render(raw)}",
                )
            )
            continue

        position_path = pointer(path, "position")
        if "position" not in raw:
            issues.append(
                A3Issue(
                    C.VARIANT_POSITION_MISSING,
                    position_path,
                    "a variant record is missing its required 'position' member",
                )
            )
            continue
        position = _as_integer(raw["position"])
        if position is None:
            issues.append(
                A3Issue(
                    C.VARIANT_POSITION_NOT_INTEGER,
                    position_path,
                    f"'position' must be an integer, got {_render(raw['position'])}",
                )
            )
            continue

        # A variant record is open: every other member is preserved verbatim, in
        # document order, and is not type-checked.
        extra = {k: v for k, v in raw.items() if k != "position"}
        out.append({"path": path, "position": position, "extra": extra})

    return out


def _stage2_metadata(metadata: dict[str, Any], issues: list[A3Issue]) -> dict[str, str]:
    """Extract the four metadata members, checking only that each is a string."""
    _unknown_fields(metadata, METADATA_KEYS, "/metadata", issues)

    out: dict[str, str] = {}
    for key in METADATA_KEYS:
        value = metadata.get(key)
        if value is None:
            out[key] = ""
        elif isinstance(value, str):
            out[key] = value
        else:
            issues.append(
                A3Issue(
                    C.METADATA_FIELD_NOT_STRING,
                    pointer("/metadata", key),
                    f"'metadata.{key}' must be a string, got {_render(value)}",
                )
            )
            out[key] = ""
    return out


# ---------------------------------------------------------------------------
# Stage 3 — Intra-field
# ---------------------------------------------------------------------------


def _stage3_intra_field(draft: dict[str, Any], issues: list[A3Issue]) -> None:
    """Sort every index, then check that each is internally consistent.

    Sorting happens here rather than in Stage 2 because an index cannot be
    sorted until its elements are known to be sortable, and it must be sorted
    before duplication and overlap can be checked. Every subscript reported by
    this stage and by Stage 4 therefore names the *sorted* array.
    """
    for entry in _entries(draft):
        if entry["geometry"] == "positions":
            entry["index"] = sorted(entry["index"])
        else:
            entry["index"] = sorted(entry["index"], key=lambda r: (r[0], r[1]))

    for entry in _entries(draft):
        index_path = entry["index_path"]
        if entry["geometry"] == "positions":
            positions: list[int] = entry["index"]
            for i, p in enumerate(positions):
                if p < 1:
                    issues.append(
                        A3Issue(
                            C.POS_NOT_POSITIVE,
                            pointer(index_path, i),
                            f"position {p} must be >= 1; positions are 1-based",
                        )
                    )
            # One issue per distinct repeated value, not per excess occurrence:
            # [7, 7, 7] is one mistake, not two.
            seen: set[int] = set()
            for a, b in zip(positions, positions[1:]):
                if a == b and a not in seen:
                    seen.add(a)
                    issues.append(
                        A3Issue(
                            C.POS_DUPLICATE,
                            index_path,
                            f"position {a} appears more than once; duplicates are "
                            f"rejected, not merged",
                        )
                    )
        else:
            ranges: list[list[int]] = entry["index"]
            for i, (start, end) in enumerate(ranges):
                elem_path = pointer(index_path, i)
                if start < 1 or end < 1:
                    issues.append(
                        A3Issue(
                            C.RANGE_ENDPOINT_NOT_POSITIVE,
                            elem_path,
                            f"range [{start}, {end}] has an endpoint below 1; "
                            f"positions are 1-based",
                        )
                    )
                if start >= end:
                    issues.append(
                        A3Issue(
                            C.RANGE_ORDER,
                            elem_path,
                            f"range [{start}, {end}] must satisfy start < end; a single "
                            f"residue belongs in a position-indexed family",
                        )
                    )
            for prev, curr in zip(ranges, ranges[1:]):
                # Sorted by start, so they overlap exactly when the second
                # starts at or before the first ends.
                if curr[0] <= prev[1]:
                    issues.append(
                        A3Issue(
                            C.RANGE_OVERLAP,
                            index_path,
                            f"ranges [{prev[0]}, {prev[1]}] and [{curr[0]}, {curr[1]}] "
                            f"overlap; adjacent ranges are permitted but overlapping "
                            f"ones are not",
                        )
                    )

    for variant in draft["variant"]:
        if variant["position"] < 1:
            issues.append(
                A3Issue(
                    C.VARIANT_POSITION_NOT_POSITIVE,
                    pointer(variant["path"], "position"),
                    f"position {variant['position']} must be >= 1; positions are 1-based",
                )
            )


# ---------------------------------------------------------------------------
# Stage 4 — Contextual
# ---------------------------------------------------------------------------


def _stage4_contextual(draft: dict[str, Any], issues: list[A3Issue]) -> None:
    """Check every value against the sequence, whose length is known only
    because Stages 1-3 all passed."""
    sequence: str = draft["sequence"]
    n = len(sequence)

    for entry in _entries(draft):
        index_path = entry["index_path"]
        if entry["geometry"] == "positions":
            for i, p in enumerate(entry["index"]):
                if p > n:
                    issues.append(
                        A3Issue(
                            C.POS_OUT_OF_BOUNDS,
                            pointer(index_path, i),
                            f"position {p} is out of bounds for a sequence of length "
                            f"{n} (must be 1-{n})",
                        )
                    )
        else:
            # Stage 3 established start < end, so only `end` can exceed `n`.
            for i, (start, end) in enumerate(entry["index"]):
                if end > n:
                    issues.append(
                        A3Issue(
                            C.RANGE_OUT_OF_BOUNDS,
                            pointer(index_path, i),
                            f"range [{start}, {end}] is out of bounds for a sequence of "
                            f"length {n} (must be within 1-{n})",
                        )
                    )

    for variant in draft["variant"]:
        position = variant["position"]
        if position > n:
            issues.append(
                A3Issue(
                    C.VARIANT_OUT_OF_BOUNDS,
                    pointer(variant["path"], "position"),
                    f"position {position} is out of bounds for a sequence of length "
                    f"{n} (must be 1-{n})",
                )
            )
            continue
        # The residue rule is deliberately narrow: it fires only when `from` is
        # present *and* is a single character. `from` is an optional member of an
        # open record, so any other shape is not this rule's problem.
        frm = variant["extra"].get("from")
        if not isinstance(frm, str) or len(frm) != 1:
            continue
        actual = sequence[position - 1]
        # Case-insensitive because `sequence` is case-folded during
        # normalization and `from` is not.
        if frm.upper() != actual:
            issues.append(
                A3Issue(
                    C.VARIANT_RESIDUE_MISMATCH,
                    pointer(variant["path"], "from"),
                    f"'from' is {frm!r} but residue {position} of the sequence is "
                    f"{actual!r}; the variant list may be annotated against a "
                    f"different isoform",
                )
            )


# ---------------------------------------------------------------------------
# Construction
# ---------------------------------------------------------------------------


def _build(draft: dict[str, Any]) -> dict[str, Any]:
    """Assemble the normalized document that the Pydantic model is built from.

    Every invariant this relies on was established by the four stages.
    """

    def family(name: str) -> dict[str, Any]:
        return {
            key: {
                "index": (
                    entry["index"]
                    if entry["geometry"] == "positions"
                    else [tuple(r) for r in entry["index"]]
                ),
                "type": entry["type"],
            }
            for key, entry in draft[name].items()
        }

    return {
        "sequence": draft["sequence"],
        "annotations": {
            "site": family("site"),
            "region": family("region"),
            "ptm": family("ptm"),
            "processing": family("processing"),
            "variant": [
                {"position": v["position"], **v["extra"]} for v in draft["variant"]
            ],
        },
        "metadata": draft["metadata"],
    }


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _entries(draft: dict[str, Any]):
    """Every named entry across the four named families, in family order."""
    for family in NAMED_FAMILIES:
        yield from draft[family].values()


def _classify(value: Any) -> str:
    """Classify an index element as ``integer``, ``array``, or ``other``."""
    if isinstance(value, list):
        return "array"
    if _as_integer(value) is not None:
        return "integer"
    return "other"


def _as_integer(value: Any) -> int | None:
    """Interpret a JSON value as an integer.

    JSON has no integer type distinct from number, and JSON Schema's
    ``type: "integer"`` accepts any number with a zero fractional part. ``3.0``
    is therefore the position 3, while ``2.5`` is not a position at all.

    ``bool`` is a subclass of ``int`` in Python but is a distinct type in JSON,
    so ``True`` is never a position.
    """
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def _expected_shape(family: str) -> str:
    if family == "site":
        return "an integer position"
    if family == "region":
        return "a [start, end] range"
    return "an integer position or a [start, end] range"


def _unknown_fields(
    obj: dict[str, Any], allowed: tuple[str, ...], base: str, issues: list[A3Issue]
) -> None:
    """Report one ``A3E_UNKNOWN_FIELD`` per property outside ``allowed``."""
    for key in obj:
        if key not in allowed:
            issues.append(
                A3Issue(
                    C.UNKNOWN_FIELD,
                    pointer(base, key),
                    f"{key!r} is not a permitted member here; expected one of: "
                    f"{', '.join(allowed)}",
                )
            )


def _render(value: Any) -> str:
    """Render a JSON value compactly for a human message, truncating long ones."""
    text = "null" if value is None else repr(value)
    return f"{text[:39]}…" if len(text) > 40 else text
