/**
 * Validation issues: stable codes, RFC 6901 paths, and human messages.
 *
 * The contract is defined in `spec/error-codes.md` and `spec/error-paths.md` at
 * the repository root, and is shared by all five A3 implementations. An issue's
 * `code` and `path` are contractual; its `message` is not and may be reworded
 * freely.
 *
 * @module
 */

/**
 * Every A3 validation issue code, mapped to the stage it belongs to.
 *
 * Codes are never renumbered and never reused. See `spec/error-codes.md` for
 * the full registry, including each code's trigger condition and the path it
 * reports.
 */
export const A3_ISSUE_STAGES = {
  // ── Stage 1 — Envelope ──────────────────────────────────────────────────
  A3E_DOC_NOT_OBJECT: 1,
  A3E_ENVELOPE_SCHEMA_MISSING: 1,
  A3E_ENVELOPE_SCHEMA_MISMATCH: 1,
  A3E_ENVELOPE_VERSION_MISSING: 1,
  A3E_ENVELOPE_VERSION_MISMATCH: 1,

  // ── Stage 2 — Structural ────────────────────────────────────────────────
  A3E_UNKNOWN_FIELD: 2,
  A3E_SEQ_MISSING: 2,
  A3E_SEQ_NOT_STRING: 2,
  A3E_SEQ_TOO_SHORT: 2,
  A3E_SEQ_CHARSET: 2,
  A3E_ANNOTATIONS_NOT_OBJECT: 2,
  A3E_METADATA_NOT_OBJECT: 2,
  A3E_METADATA_FIELD_NOT_STRING: 2,
  A3E_FAMILY_NOT_OBJECT: 2,
  A3E_VARIANT_LIST_NOT_ARRAY: 2,
  A3E_NAME_EMPTY: 2,
  A3E_ENTRY_NOT_OBJECT: 2,
  A3E_ENTRY_TYPE_NOT_STRING: 2,
  A3E_INDEX_MISSING: 2,
  A3E_INDEX_NOT_ARRAY: 2,
  A3E_INDEX_ELEMENT_TYPE: 2,
  A3E_INDEX_MIXED: 2,
  A3E_RANGE_ARITY: 2,
  A3E_RANGE_ENDPOINT_NOT_INTEGER: 2,
  A3E_VARIANT_NOT_OBJECT: 2,
  A3E_VARIANT_POSITION_MISSING: 2,
  A3E_VARIANT_POSITION_NOT_INTEGER: 2,

  // ── Stage 3 — Intra-field ───────────────────────────────────────────────
  A3E_POS_NOT_POSITIVE: 3,
  A3E_POS_DUPLICATE: 3,
  A3E_RANGE_ENDPOINT_NOT_POSITIVE: 3,
  A3E_RANGE_ORDER: 3,
  A3E_RANGE_OVERLAP: 3,
  A3E_VARIANT_POSITION_NOT_POSITIVE: 3,

  // ── Stage 4 — Contextual ────────────────────────────────────────────────
  A3E_POS_OUT_OF_BOUNDS: 4,
  A3E_RANGE_OUT_OF_BOUNDS: 4,
  A3E_VARIANT_OUT_OF_BOUNDS: 4,
  A3E_VARIANT_RESIDUE_MISMATCH: 4,
} as const satisfies Record<string, 1 | 2 | 3 | 4>;

/** A stable, machine-readable validation issue code. */
export type A3IssueCode = keyof typeof A3_ISSUE_STAGES;

/** The validation stage a code belongs to. */
export type A3Stage = (typeof A3_ISSUE_STAGES)[A3IssueCode];

/** Every code in the registry, for coverage checks. */
export const A3_ISSUE_CODES = Object.keys(A3_ISSUE_STAGES) as A3IssueCode[];

/** A single validation issue. */
export interface A3Issue {
  /** Stable machine-readable code. Contractual. */
  readonly code: A3IssueCode;
  /** RFC 6901 JSON Pointer to the offending value. Contractual. */
  readonly path: string;
  /** Human-readable explanation. Not contractual; may be reworded freely. */
  readonly message: string;
}

/** The validation stage an issue belongs to. */
export function stageOf(issue: A3Issue): A3Stage {
  return A3_ISSUE_STAGES[issue.code];
}

/**
 * Escape one reference token for inclusion in a JSON Pointer.
 *
 * RFC 6901 escapes exactly two characters. `~` must be replaced before `/`, or
 * a key containing `/` would come out as `~01` instead of `~1`.
 */
export function escapeToken(token: string): string {
  return token.replaceAll("~", "~0").replaceAll("/", "~1");
}

/**
 * Append one reference token or array subscript to a JSON Pointer.
 *
 * @param base - The pointer to extend. The empty string denotes the document root.
 * @param token - A **raw** object member name, which this function escapes, or a
 *   0-based array subscript, which needs no escaping. Do not pre-escape a name:
 *   `pointer(b, "a~1b")` yields `a~01b`, because the `~` is escaped a second time.
 *
 * @example
 * ```ts
 * pointer("/annotations/site", "a/b"); // "/annotations/site/a~1b"  (escaped)
 * pointer("", "$schema");              // "/$schema"                (nothing to escape)
 * pointer("/annotations/variant", 3);  // "/annotations/variant/3"  (subscript)
 * ```
 */
export function pointer(base: string, token: string | number): string {
  return typeof token === "number" ? `${base}/${token}` : `${base}/${escapeToken(token)}`;
}

/**
 * Resolve a JSON Pointer against a parsed document.
 *
 * Returns `undefined` when the pointer does not resolve — which for a
 * missing-member issue is the expected outcome, not a failure. See
 * `spec/error-paths.md`, Rule 1.
 */
export function resolvePointer(document: unknown, path: string): unknown {
  if (path === "") return document;
  let current: unknown = document;
  for (const raw of path.slice(1).split("/")) {
    const token = raw.replaceAll("~1", "/").replaceAll("~0", "~");
    if (Array.isArray(current)) {
      current = current[Number(token)];
    } else if (current !== null && typeof current === "object") {
      current = (current as Record<string, unknown>)[token];
    } else {
      return undefined;
    }
    if (current === undefined) return undefined;
  }
  return current;
}
