# A3 conformance contract

This directory is the language-agnostic contract that the R, Python, Julia,
TypeScript, and Rust implementations of A3 are held to. It is normative: where
an implementation and this directory disagree, this directory is right.

| File | What it fixes |
|---|---|
| [`error-codes.md`](error-codes.md) | The rule registry — one stable code per validation rule, its stage, and the value it points at |
| [`error-paths.md`](error-paths.md) | The path grammar — every issue carries an RFC 6901 JSON Pointer |
| [`conformance/`](conformance/) | The shared corpus every implementation is tested against |

The JSON Schema itself lives at [`../schema/a3.schema.json`](../schema/a3.schema.json)
and defines which documents are *valid*. This directory defines what an
implementation must *say* when a document is not.

## What is contractual

Contractual — a conforming implementation must agree exactly:

- the set of `{code, path}` pairs produced for a given document
- which stage each rule belongs to, and therefore which issues are suppressed
  by an earlier-stage failure
- the normalized serialization of a valid document

Not contractual — implementations may differ freely:

- human-readable messages, their wording, punctuation, and language
- the order in which issues are emitted (issue sets are compared as sets)
- the type used to carry an issue (struct, dataclass, list, S7 object)
- how issues are surfaced (thrown, returned, printed)

Human messages are deliberately excluded. Tying five languages to one wording
would freeze every message the first time the corpus is written and make
improving a message a cross-language breaking change. Codes are stable so that
programs can rely on them; messages are free so that humans can be served well.

## Who owns what

Authority is split three ways, and no implementation is the specification.

| Artifact | Authority |
|---|---|
| What is **valid** | [`../schema/a3.schema.json`](../schema/a3.schema.json). A planned change makes the R S7 class declarations generate it, so **R owns the data model** — see "Provenance of the schema" below. |
| What an implementation must **say** | This directory. Language-agnostic; no implementation privileged. |
| The **canonical form** of a valid document | [`conformance/README.md`](conformance/README.md), in prose, machine-checked by the corpus generator. |
| Human-readable **messages** | Nobody. Deliberately unowned, so improving one is not a cross-language breaking change. |

A3 began in R, with S7. The Rust implementation later acquired a collect-all
pass over raw JSON because the `a3` CLI needed a `--diagnose` mode that reports
everything wrong with a file in one run — and that made it, by accident of the
CLI's requirements, the implementation closest to the architecture this contract
asks of all five. It is used as the corpus generator's *producer* of canonical
forms for that reason and no other. Producing is not defining: every generated
companion is checked against the rules stated above before it is written.

(`--diagnose` is now deprecated, because every run in every implementation is a
full diagnostic. The property that made Rust convenient became universal.)

## Two entry points, two error behaviours

Every implementation exposes two ways to get an A3 object, and they behave
differently on purpose:

| Entry point | Audience | On error |
|---|---|---|
| Reading a **document** (`read_A3json`, `a3_from_json`, `A3.fromJSONText`, `A3(::AbstractDict)`, `validate`) | someone handed a file assembled from public resources | **collect-all** — every issue in the earliest failing stage, reported at once |
| Calling a **constructor** (`create_A3`, `create_a3`, `new A3(...)`, `A3(seq, ann, meta)`) | a programmer building an object in code | **fail-fast** — the first problem, with a corrective message |

The reasoning: a 500-annotation document assembled from four databases should
not be fixed one error per run, so document validation accumulates. A
programmer who passed a bad argument wants an immediate, specific, corrective
message, and there is no document to accumulate across. By the time an object
is constructed from a document the raw pass has already run, so the
constructor's checks never fire on that path — they are a backstop, not a
duplicate.

Each implementation's README states this split. It is a design feature, not an
inconsistency, but only if it is stated.

## Validation is not linting

Validation answers exactly one question: *is this a legal A3 document?* Its
sole authority is the JSON Schema. If a rule is not expressible in the schema,
it does not belong in validation.

There is a category of check that is useful but is not about legality — is this
`uniprot_id` a well-formed accession, does this `organism` look like a species
name, is this `reference` resolvable. Those belong in a separate, opt-in **lint**
layer, so that "valid" keeps meaning "conforms to the published schema" and
nothing more. No lint layer exists today; this note records where it goes if one
is built, so the checks do not accrete into validation instead.

Two exceptions are already in the registry and are worth naming, because they
are the only rules not directly expressible in JSON Schema:

- the bounds rules (Stage 4), which compare an index against `sequence` length
- `A3E_VARIANT_RESIDUE_MISMATCH`, which compares a variant's `from` residue
  against the sequence

Both are *cross-field* constraints on data the schema already declares, not
opinions about the content of a free-text field. That is the line.

## Provenance of the schema

`schema/a3.schema.json` is committed and is the artifact every implementation
conforms to. A planned change (plan §7) makes it *generated* from the R S7 class
declarations rather than hand-maintained. When that lands, regenerating the
schema will require R — the Python, TypeScript, Julia, and Rust maintainers will
not be able to regenerate it locally. That is acceptable because the generated
artifact is committed and CI fails on drift, but it is stated here so nobody is
surprised by it.
