# A3 conformance corpus

One corpus. Five readers. Every implementation resolves this directory by
relative path from the repo root — **do not vendor a copy into a language
directory**, or the five stop testing the same thing.

```
spec/conformance/
  README.md          this file
  generator/         the single source of truth for every case below
  manifest.json      the index: one entry per case, with its expectation
  valid/             documents that must validate
  invalid/           documents that must not, with the exact issues expected
```

Current size: **72 cases** — 22 valid (14 with a round-trip expectation) and 50
invalid, of which 10 expect more than one issue and 6 are deliberate
multi-error cases. Every one of the 37 codes in
[`../error-codes.md`](../error-codes.md) has a case that exercises it in
isolation.

## The manifest

```json
{
  "cases": [
    {
      "file": "invalid/multi-contextual.a3.json",
      "description": "Five Stage 4 issues, including the biological one.",
      "valid": false,
      "stage": 4,
      "issues": [
        { "code": "A3E_POS_OUT_OF_BOUNDS",        "path": "/annotations/site/s/index/1" },
        { "code": "A3E_VARIANT_RESIDUE_MISMATCH", "path": "/annotations/variant/0/from" }
      ]
    },
    {
      "file": "valid/unsorted-positions.a3.json",
      "description": "Positions out of order are valid; normalization sorts them ascending.",
      "valid": true,
      "normalized": "valid/unsorted-positions.normalized.json"
    }
  ]
}
```

| Key | Meaning |
|---|---|
| `file` | path to the case document, relative to this directory |
| `description` | one line, for legible failure output |
| `valid` | whether the document must validate |
| `stage` | invalid cases only: the stage the reported issues belong to |
| `issues` | invalid cases only: the exact set of `{code, path}` pairs expected |
| `normalized` | valid cases only, optional: the expected normalized document |

## Assertion semantics

**For `valid: false`** — the set of `{code, path}` pairs produced must **equal**
the expected set. Not a subset, not a superset. Human messages are ignored
entirely.

The comparison is over **multisets**, not sets, because a rule can legitimately
fire more than once at the same path: `pos-duplicate` expects two
`A3E_POS_DUPLICATE` issues at `/annotations/site/s/index`, one per repeated
value. An implementation that reports one has not conformed.

Order is **not** compared. Two implementations that emit the same pairs in
different orders both conform.

**For `valid: true`** — validation must succeed. If the case has a `normalized`
key, then parsing the case document and re-serializing it must produce a
document **structurally equal** to the `normalized` file, where structural
equality means:

- the same values throughout;
- arrays in the same order;
- **object members in the same order.**

Object member order is significant. That is what makes these cases test
normalization rather than merely test parsing.

Not byte-for-byte: that would additionally pin indentation, line endings and
number formatting across five serializers that do not agree and are not all
configurable, for no guarantee that ordered structural comparison does not
already give.

## The canonical form

Re-serializing a parsed document must produce all defaults, materialized:

- all five top-level members, in schema order: `$schema`, `a3_version`,
  `sequence`, `annotations`, `metadata`
- all five annotation families, in schema order: `site`, `region`, `ptm`,
  `processing`, `variant` — present even when empty
- all four metadata members, in schema order — present even when empty, as `""`
- every annotation entry's `index` then `type`, with `type` present as `""` when
  it was absent

Normalization is exactly three transformations and nothing else:

1. `sequence` is case-folded to uppercase
2. positions are sorted ascending
3. ranges are sorted by start, then by end

**Author-supplied key order is preserved.** Annotation names appear in the order
they appear in the document, and a variant record's extra members appear in the
order they appear in the record. Normalization never reorders anything a human
wrote. `valid/annotation-name-order-preserved.a3.json` is the case that pins
this; it exists because `HashMap` in Rust and `Dict` in Julia would otherwise
make the output order nondeterministic run to run.

Variant records are open: every member beyond `position` round-trips verbatim,
including `null`, nested objects, and arrays. `valid/variant-open-fields.a3.json`
covers all three.

## Adding a case

Edit [`generator/src/main.rs`](generator/src/main.rs) and re-run it:

```sh
just corpus     # cargo run --manifest-path spec/conformance/generator/Cargo.toml
```

Do not hand-edit the case files or `manifest.json`. A case file and its manifest
entry have to agree, and 87 files kept in sync by hand will not stay in sync.

CI runs the generator and fails if it produces a diff, so the committed corpus
and the generator can never drift.

### Where each expectation comes from

- **Expected issues are hand-declared.** They are the specification, so they
  never come from an implementation — otherwise the corpus would be a snapshot
  of one implementation rather than something it can be found wrong against.
- **`normalized` companions are produced by the Rust implementation**, because
  re-implementing the three normalization rules in the generator would make a
  sixth normalizer. Producing is not defining: every companion is checked
  against the canonical form above — member order, materialized defaults,
  sequence differing only by case, indices sorted and in bounds, authored key
  order intact — before it is written. A normalization bug fails generation
  instead of entering the corpus as if it were the spec.

See [`../README.md`](../README.md), "Who owns what".

### What a good case looks like

- **One rule per case**, wherever possible. A case that exercises three codes
  cannot tell you which of the three an implementation got wrong. The
  `multi-*` cases deliberately break this rule; everything else should not.
- **A `description` that says what is being asserted**, not what the file
  contains. "Duplicate positions are rejected, not deduplicated" is useful in a
  failure report; "site index with duplicates" is not.
- **The smallest document that triggers it.** Every case shares the same
  24-residue sequence `MAEPRQEFEVMEDHAGTYGLGDRK` so that bounds arithmetic is
  checkable by eye. Residue 5 is `R`, residue 18 is `Y`.

## Writing a runner

Each implementation ships one runner. It reads `manifest.json`, validates every
case, and compares issue multisets.

**Failure output must name the diff.** For every failing case, print which
`{code, path}` pairs were expected and absent, and which were produced and
unexpected. A bare "sets differ" costs an hour every time it fires.

```
FAIL invalid/multi-contextual.a3.json
  Five Stage 4 issues, including the biological one.
  missing:    A3E_VARIANT_RESIDUE_MISMATCH  /annotations/variant/0/from
  unexpected: A3E_VARIANT_RESIDUE_MISMATCH  /annotations/variant/0
```

The runners:

| Implementation | Runner |
|---|---|
| Rust | `rust/tests/conformance.rs` |
| Python | `python/rtemis_a3/tests/test_conformance.py` |
| TypeScript | `typescript/tests/conformance.test.ts` |
| Julia | a `@testset` in `julia/RtemisA3/test/runtests.jl` |
| R | `r/tests/testthat/test_conformance.R` |

The R package's tests must survive being run off-repo, where the corpus is not
installed. Use `testthat::skip_if_not()` on the corpus path existing, and rely
on CI — where the repo is present — to run the real check.
