# A3 conformance contract

Normative. Where an implementation and this directory disagree, this directory
is right.

| File | What it fixes |
|---|---|
| [`error-codes.md`](error-codes.md) | one stable code per rule, its stage, the path it reports |
| [`error-paths.md`](error-paths.md) | every issue carries an RFC 6901 JSON Pointer |
| [`conformance/`](conformance/) | 72 cases every implementation is tested against |

[`../schema/a3.schema.json`](../schema/a3.schema.json) defines which documents
are *valid*. This directory defines what an implementation must *say* when one
is not.

## Contractual

A conforming implementation must agree exactly on:

- the set of `{code, path}` pairs a document produces
- each rule's stage, and therefore which issues an earlier failure suppresses
- the normalized serialization of a valid document

Free to differ: message wording, emission order, the type carrying an issue, and
how it is surfaced. Messages are excluded so that improving one is not a
cross-language breaking change.

## Who owns what

No implementation is the specification.

| Artifact | Authority |
|---|---|
| What is valid | the JSON Schema — and R owns the data model, since a planned change generates the schema from its S7 classes |
| What to say | this directory |
| The canonical form | [`conformance/README.md`](conformance/README.md), machine-checked by the corpus generator |
| Messages | nobody, deliberately |

A3 began in R. Rust later grew a collect-all pass over raw JSON because the CLI
needed `--diagnose`, which made it the implementation closest to what this
contract asks of all five — so the corpus generator uses it to *produce*
canonical forms. Producing is not defining: every companion is checked against
the rules above before it is written.

## Invariants that are easy to get wrong

- **Within a stage collect everything; across stages short-circuit.** A stage
  runs only if every earlier one was clean. A bad `sequence` has no known
  length, so Stage 4 would otherwise emit one spurious issue per position.
- **A guard suppresses its own subtree only** — not its siblings.
- **Per-element rules carry a subscript; whole-set rules do not.** A duplicate
  is a property of the set, not of either position.
- **Sorting is normalization; duplication is a rule.** There is no
  `A3E_POS_UNSORTED`.
- **Subscripts are 0-based; sequence positions are 1-based.**
- **`3.0` is the position 3**; `2.5` is not a position, and `true` never is.

## Two entry points

| Entry point | On error |
|---|---|
| reading a document — `read_A3json`, `a3_from_json`, `A3.fromJSONText`, `A3(::AbstractDict)` | **collect-all**: every issue in the earliest failing stage |
| calling a constructor — `create_A3`, `create_a3`, `new A3(...)` | **fail-fast**: one corrective message |

A 500-annotation document assembled from four databases should not be fixed one
error per run. A programmer who passed a bad argument wants one specific
message. Each README states this; keep it stated.

## Validation is not linting

Validation answers one question: *is this a legal A3 document?* Its sole
authority is the schema. A check not expressible there does not belong in
validation — so metadata members are free text, and A3 checks only that each is
a string. Accession grammars and the like belong in an opt-in lint layer, which
does not exist yet.

Two rules are not directly expressible in JSON Schema and are in the registry
anyway: the Stage 4 bounds rules, and `A3E_VARIANT_RESIDUE_MISMATCH`. Both are
cross-field constraints on data the schema already declares, not opinions about
free text. That is the line.

## Changing a rule

1. Add or amend the code in [`error-codes.md`](error-codes.md), with its stage
   and path.
2. Add a case to `conformance/generator/` exercising it in isolation, then
   `just corpus`.
3. Implement it in **all five**. A rule that fires in one language and not the
   others is worse than no rule.
4. `just conformance` must pass in all five before merging.

A meta-test fails if a registry code has no case, so step 2 cannot be skipped.
Never change a code or a path without changing this directory and the corpus in
the same commit.

## Known limitations

- **JavaScript reorders integer-like keys.** An annotation named `"5"` is moved
  ahead of other keys by JavaScript's object property-order rules, so
  `@rtemis/a3` cannot round-trip it in document order. The other four can. Do
  not "fix" this by sorting everywhere.
- **Regenerating the schema will require R** once it is generated from the S7
  classes. The artifact stays committed and CI fails on drift, so only
  maintainers regenerating it are affected.
