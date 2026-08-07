"""A3 error classes.

A3 distinguishes two failure modes, and they are not interchangeable:

- :class:`A3ParseError` — the input was not JSON, or could not be read. There
  is no path to report and nothing further to collect, so it carries a single
  message.
- :class:`A3ValidationError` — the input parsed but violates A3 rules. It
  carries every issue found in the earliest failing validation stage, each with
  a stable code and an RFC 6901 JSON Pointer.
"""

from __future__ import annotations

from .issues import A3Issue


class A3ValidationError(Exception):
    """Raised when input fails A3 validation.

    Parameters
    ----------
    issues : list[A3Issue]
        Every issue found in the earliest validation stage that produced one.
        Validation short-circuits at a stage boundary, so all issues in one
        error come from the same stage.

    Attributes
    ----------
    issues : list[A3Issue]
        The full issue list, for programmatic inspection. Each issue's ``code``
        and ``path`` are contractual and identical across all five A3
        implementations; its ``message`` is not.

    Examples
    --------
    >>> from rtemis.a3 import a3_from_json, A3ValidationError
    >>> try:
    ...     a3_from_json('{"$schema": "x", "a3_version": "1.0.0", "sequence": "MA"}')
    ... except A3ValidationError as exc:
    ...     [(i.code, i.path) for i in exc.issues]
    [(<A3IssueCode.ENVELOPE_SCHEMA_MISMATCH: 'A3E_ENVELOPE_SCHEMA_MISMATCH'>, '/$schema')]
    """

    def __init__(self, issues: list[A3Issue]):
        self.issues: list[A3Issue] = issues
        summary = f"A3 validation failed with {len(issues)} issue(s):\n" + "\n".join(
            f"  {issue}" for issue in issues
        )
        super().__init__(summary)

    @property
    def stage(self) -> int | None:
        """The validation stage the reported issues belong to, or ``None``."""
        return self.issues[0].stage if self.issues else None

    @property
    def messages(self) -> list[str]:
        """Individual human-readable messages, one per issue."""
        return [issue.message for issue in self.issues]


class A3ParseError(Exception):
    """Raised when JSON parsing or file I/O fails."""
