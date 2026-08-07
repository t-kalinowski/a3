"""Functional API for A3 construction, parsing, and querying.

Users interact with A3 objects exclusively through these functions.
Pydantic models are never constructed directly.
"""

from __future__ import annotations

import json
from typing import Any

from pydantic import ValidationError

from ._models import A3, VariantRecord, _BoundsErrors
from ._validate import (
    A3_SCHEMA_URI as _A3_SCHEMA_URI,
    A3_VERSION as _A3_VERSION,
    validate,
)
from .errors import A3ParseError, A3ValidationError
from .issues import A3Issue


def create_a3(
    sequence: str,
    *,
    site: dict[str, dict[str, Any]] | None = None,
    region: dict[str, dict[str, Any]] | None = None,
    ptm: dict[str, dict[str, Any]] | None = None,
    processing: dict[str, dict[str, Any]] | None = None,
    variant: list[dict[str, Any]] | None = None,
    metadata: dict[str, str] | None = None,
) -> A3:
    """Create an A3 object from raw components.

    Parameters
    ----------
    sequence : str
        Amino acid sequence (minimum 2 characters, ``[A-Za-z*]``).
    site : dict, optional
        Named site annotations. Each value: ``{"index": [...], "type": "..."}``.
    region : dict, optional
        Named region annotations. Each value:
        ``{"index": [[s, e], ...], "type": "..."}``.
    ptm : dict, optional
        Named PTM annotations. Index may be positions or ranges.
    processing : dict, optional
        Named processing annotations. Index may be positions or ranges.
    variant : list of dict, optional
        Variant records. Each must have ``"position"``; other fields are open.
    metadata : dict, optional
        Metadata fields: ``uniprot_id``, ``description``, ``reference``,
        ``organism``.

    Returns
    -------
    A3
        Validated, immutable A3 object.

    Raises
    ------
    A3ValidationError
        If input fails structural or contextual validation.
    """
    data: dict[str, Any] = {"sequence": sequence}
    annotations: dict[str, Any] = {}
    if site is not None:
        annotations["site"] = site
    if region is not None:
        annotations["region"] = region
    if ptm is not None:
        annotations["ptm"] = ptm
    if processing is not None:
        annotations["processing"] = processing
    if variant is not None:
        annotations["variant"] = variant
    if annotations:
        data["annotations"] = annotations
    if metadata is not None:
        data["metadata"] = metadata

    data["$schema"] = _A3_SCHEMA_URI
    data["a3_version"] = _A3_VERSION
    return _validated(data)


def a3_from_json(text: str) -> A3:
    """Parse a JSON string into an A3 object.

    Parameters
    ----------
    text : str
        JSON string conforming to the A3 wire format.

    Returns
    -------
    A3
        Validated, immutable A3 object.

    Raises
    ------
    A3ParseError
        If JSON parsing fails.
    A3ValidationError
        If parsed data fails validation.
    """
    try:
        data = json.loads(text)
    except (json.JSONDecodeError, TypeError) as exc:
        raise A3ParseError(f"invalid JSON: {exc}") from exc

    return _validated(data)


def _validated(raw: Any) -> A3:
    """Run the staged validator, then build the typed model.

    Splitting these two steps is what makes collect-all reporting possible.
    Pydantic fails on the first shape problem and reports it in Pydantic's own
    vocabulary; the staged validator in :mod:`._validate` reports every problem
    in the failing stage, in A3's. By the time the model sees the data it is
    known good, so the model's own validators act only as a backstop for the
    constructor path.

    Parameters
    ----------
    raw : Any
        The output of :func:`json.loads`, or a dict assembled by
        :func:`create_a3`.

    Returns
    -------
    A3
        Validated, immutable A3 object.

    Raises
    ------
    A3ValidationError
        If validation produced any issue.
    """
    normalized, issues = validate(raw)
    if issues:
        raise A3ValidationError(issues)
    assert normalized is not None  # noqa: S101 — guaranteed when issues is empty

    try:
        return A3.model_validate(normalized)
    except (_BoundsErrors, ValidationError) as exc:  # pragma: no cover
        # Unreachable: the staged validator has already established every
        # invariant the model checks. If this fires, the two have diverged.
        raise AssertionError(
            f"staged validation passed but the model rejected the result: {exc}"
        ) from exc


def validate_a3(raw: Any) -> list[A3Issue]:
    """Validate a parsed value without raising.

    The non-throwing counterpart to :func:`a3_from_json`, for forms, reports,
    and live-validation panels that need the full issue list rather than an
    exception.

    Parameters
    ----------
    raw : Any
        A parsed A3 document, typically the result of :func:`json.loads`.

    Returns
    -------
    list[A3Issue]
        Every issue found in the earliest failing validation stage. Empty when
        the document is valid.

    Examples
    --------
    >>> import json
    >>> from rtemis.a3 import validate_a3
    >>> doc = json.loads(
    ...     '{"$schema": "https://schema.rtemis.org/a3/v1/schema.json",'
    ...     ' "a3_version": "1.0.0", "sequence": "MAEPRQ"}'
    ... )
    >>> validate_a3(doc)
    []
    """
    return validate(raw)[1]


def a3_to_json(a3: A3, *, indent: int | None = None) -> str:
    """Serialize an A3 object to a canonical JSON string.

    Parameters
    ----------
    a3 : A3
        A3 object to serialize.
    indent : int, optional
        JSON indentation level. ``None`` for compact output.

    Returns
    -------
    str
        JSON string.
    """
    data: dict[str, Any] = {
        "$schema": _A3_SCHEMA_URI,
        "a3_version": _A3_VERSION,
        **a3.model_dump(mode="json"),
    }
    return json.dumps(data, indent=indent, ensure_ascii=False)


def residue_at(a3: A3, position: int) -> str:
    """Return the residue at a 1-based position.

    Parameters
    ----------
    a3 : A3
        A3 object.
    position : int
        1-based position in the sequence.

    Returns
    -------
    str
        Single character at the given position.

    Raises
    ------
    ValueError
        If position is out of bounds.
    """
    if position < 1 or position > len(a3.sequence):
        raise ValueError(
            f"position {position} is out of bounds for sequence of length "
            f"{len(a3.sequence)} (must be 1-{len(a3.sequence)})"
        )
    return a3.sequence[position - 1]


def variants_at(a3: A3, position: int) -> list[VariantRecord]:
    """Return all variant records at the given 1-based position.

    Parameters
    ----------
    a3 : A3
        A3 object.
    position : int
        1-based position in the sequence.

    Returns
    -------
    list[VariantRecord]
        Variant records matching the position. May be empty.
    """
    return [v for v in a3.annotations.variant if v.position == position]
