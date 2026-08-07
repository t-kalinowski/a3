# A3: cross-language conformance and implementation cleanup

**Status:** plan, ready for handoff
**Date:** 2026-08-06
**Goal:** bring the five implementations into demonstrable agreement, so the
*Bioinformatics* Applications Note can claim it rather than assert it.

---

## 0. Read this first

The paper's central claim is *one specification, five conforming
implementations*. Today that is an assertion: each language has its own tests,
there is no shared corpus, and the implementations **provably disagree** (§2).
This plan closes that gap. Everything else here is subordinate to it.

Two decisions are already made and are not open for re-litigation:

- **D1 — Collect-all error reporting** in all five implementations, staged
  (§3.1). Rationale and cost analysis in §3.
- **D2 — Stable error codes + paths** are the conformance contract. Human
  messages are explicitly *not* part of the contract and may differ freely
  between languages (§4).

- **D3 — The JSON Schema is generated**, not hand-maintained: the R class
  declarations become its source of truth via a ported-and-extended `prop_*` /
  `S7_to_JSONSchema()` (§7).

**Context that shapes everything below:** the five implementations were written
by an earlier model and have not had a quality pass since. Assume idiom drift
and missed opportunities, not just the divergences catalogued in §2 — §7A is a
standing audit workstream for exactly this. Release friction is also low: every
package except the CRAN build of `rtemis.a3` can be republished the same day
(§7B). **Breaking changes are therefore cheap, and this plan should not preserve
bug-compatibility with the current behaviour anywhere.** Where the right answer
and the current answer differ, take the right answer.

**Order matters.** §4 (contract) blocks everything. Do not start §6
(per-language work) before §4 is merged, or five people will invent five path
grammars a second time.

---

## 1. Repository hygiene (do first, ~1 hour, unblocks nothing but is embarrassing)

| # | Item | Detail |
|---|---|---|
| H1 | **`specs/` does not exist** | `README.md` and root `AGENTS.md` both advertise `specs/A3.md`, `specs/A3_S7`, `specs/A3_Pydantic`, `specs/A3_Julia`, `specs/A3_Zod`, `specs/A3_Rust`. None of these files exist. Either write them or remove the references. Given §4 creates `spec/`, prefer: create the directory and make it real. |
| H2 | **Licence disagreement** | R `DESCRIPTION` says `GPL (>= 3)`; Python, TypeScript and Rust all say `MPL-2.0`; Julia has a `LICENSE` file but no `license` field in `Project.toml`. Decide one, or document deliberately per-package. The journal checks this. |
| H3 | **Julia `Project.toml` has no licence field** | Add it. |
| H4 | **`examples/mapt.a3.json` modelling** | Disease variants are encoded as a `site` position list (57 entries) and the microtubule repeats as `site` positions rather than `region` ranges. Both validate, but this is the file that will be Figure 1b and Supplementary S4. See the manuscript skeleton §3.1 for the full list. Track separately from this plan. |
| H5 | **Makefile has no `test-conformance` target** | Added in §8. |

---

## 2. Verified divergences (the evidence base)

All of the following were read out of the source on 2026-08-06. They are the
reason this plan exists.

### 2.1 Error reporting model

| Impl | Model | Where |
|---|---|---|
| Rust | **collect-all** — `errors: Vec<String>` accumulated, returned as `A3Error::Validate` | `rust/src/validation.rs` |
| Python | **collect-all** — envelope errors as Pydantic-style dicts in `api.py`; `_bounds_check` accumulates `errors: list[str]` | `python/.../api.py`, `_models.py` |
| TypeScript | **collect-all** — Zod issue list surfaced on `A3ValidationError.issues` | `typescript/src/a3.ts` |
| Julia | **fail-fast** — every check is `throw(A3ValidationError(...))` | `julia/RtemisA3/src/validate.jl` |
| R | **fail-fast** — every check is `cli::cli_abort()`, inside S7 validators | `r/R/a3.R` |

### 2.2 Error path grammar — already divergent

For the *same* violation (a site position out of bounds):

- Rust emits path `annotations.site.{name}`
- Python emits path `annotations.site.{name}.index`
- Julia emits `{path}[{i}]` with an indexed element suffix

Three implementations, three grammars, for one rule. This is exactly what §4
must fix, and it is the single best piece of evidence that the conformance
corpus is necessary rather than ceremonial.

### 2.3 Rule coverage gaps

| Rule | Rust | Python | TS | Julia | R |
|---|---|---|---|---|---|
| `$schema` const | ✅ | ✅ | ✅ | ✅ | ✅ |
| `a3_version` const | ✅ | ✅ | ✅ | ✅ | ✅ |
| sequence ≥ 2 chars | ✅ | ✅ | ✅ | ✅ | ✅ |
| sequence charset | ✅ | ✅ | ✅ | ✅ | ✅ |
| positions ≥ 1 | ✅ | ✅ | ✅ | ✅ | ✅ |
| positions deduped | ✅ | ✅ | ✅ | ✅ | ✅ |
| positions sorted | ✅ | ✅ | ✅ | ✅ | ✅ |
| ranges `start < end` | ✅ | ✅ | ✅ | ✅ | ✅ |
| ranges non-overlapping | ✅ | ? | ? | ? | ✅ |
| bounds vs sequence length | ✅ | ✅ | ? | ✅ | ✅ |
| empty annotation name | ✅ | ? | ? | ? | ✅ |
| `uniprot_id` accession grammar | ❌ | ❌ | ✅ | ❌ | ❌ |
| mixed index homogeneity (ptm/processing) | ✅ | ✅ | ? | ? | ? |
| variant `from` residue matches sequence | ❌ | ❌ | ❌ | ❌ | ❌ |

`?` = not confirmed by reading; **the corpus will resolve every `?` and every
`❌` mechanically.** That is its job — do not spend time filling this table in
by hand first.

Two notes:

- **`uniprot_id` validation exists only in TypeScript.** Either promote it to
  the spec and implement everywhere, or drop it from TypeScript. A rule that
  fires in one language and not the others is worse than no rule.
- **The variant-residue check does not exist anywhere**, but the talk notes
  (`a3-paper/a3_dev.md`) claim it does. Implement it (§6.6) or strike the claim
  from the paper.

---

## 3. D1: the error model

### 3.1 What to build

> **Validate the raw parsed representation, accumulate every issue, and only
> then construct the typed object.**

This is already the Rust architecture (`validate(raw: A3) -> Result<A3, A3Error>`)
and substantially the Python one. It generalises cleanly:

```
JSON text
   │
   ├─ parse ──────────────► parse error (single, fatal, not collectable)
   │
   ▼
raw representation          (Value / dict / Dict / unknown / list)
   │
   ├─ STAGE 1  envelope     $schema, a3_version present and correct
   ├─ STAGE 2  structural   types, shapes, required keys, charset, name non-empty
   ├─ STAGE 3  intra-field  positions ≥1/sorted/deduped, ranges ordered/non-overlapping
   ├─ STAGE 4  contextual   bounds vs sequence length, variant residue vs sequence
   │
   ▼  (only if issues is empty)
typed object                A3 / A3 model / RtemisA3.A3 / S7 A3
```

**Staging is mandatory, and this is the part that is easy to get wrong.** Issues
from a stage are reported only if all earlier stages produced none. Without
this, a document whose `sequence` failed Stage 2 emits several hundred spurious
"out of bounds" errors from Stage 4. Python already does this deliberately (its
`_bounds_check` is commented "Stage 2 contextual validation"); make it explicit
and identical everywhere.

Within a stage, **collect everything**. Across stages, **short-circuit**.

### 3.2 Is collect-all correct? Yes.

A user assembling a 500-annotation MAPT document from four public resources
should not fix one error per run. Collect-all is the difference between a format
you can adopt and one you fight. It is also the precondition for the web app's
live-validation panel, which needs the full issue list to annotate a form, and
for the CLI's `--diagnose`.

### 3.3 Is it feasible? Yes, and cheaper than it looks.

Because the raw-validation architecture already exists in two implementations,
this is not a redesign — it is porting a known-good design to three more.

| Impl | Work | Estimate |
|---|---|---|
| Rust | Add codes to existing collected errors. Reference implementation. | S |
| Python | Add codes; unify envelope and bounds issue shapes. | S |
| TypeScript | Map Zod issues → `{code, path}`; add rules missing from §2.3. | M |
| Julia | `validate.jl` already threads `path` through every check. Mechanically convert `throw(A3ValidationError(...))` → `push!(issues, A3Issue(code, path, msg))`. Largely a find-and-replace with care at early returns. | M |
| R | **The real work.** R has no raw-validation pass at all: validation lives in S7 validators (fail-fast by contract — a validator returns one string) and `read_A3json` aborts on the first envelope problem. Needs a new `validate_a3_raw()` written from scratch, mirroring Rust. | L |

### 3.4 What happens to the existing constructor-level validation?

**Keep it.** S7 validators and Julia inner constructors stay fail-fast and stay
exactly as they are. They are a backstop for programmatic construction
(`create_A3(...)` called directly with bad arguments), where an immediate,
friendly, corrective message is the right behaviour and there is no document to
collect issues across. By the time an object is constructed from a *document*,
the raw pass has already run and the data is known good, so the validators never
fire on that path.

This resolves the apparent tension between collect-all and R/Julia idiom: they
are two different entry points with two different audiences.

- `read_A3json(path)` / `validate()` → raw pass → collect-all
- `create_A3(...)` / `A3(...)` → constructor → fail-fast, `cli_abort`

Document this split explicitly in each README. It is a design feature, not an
inconsistency, but only if it is stated.

---

## 4. D2: the conformance contract  ⟵ **BLOCKS EVERYTHING ELSE**

Create `spec/` at the repo root. Three artifacts, all language-agnostic, all
reviewed and merged before any implementation work begins.

### 4.1 `spec/error-codes.md` — the rule registry

One row per rule. This is the single source of truth; every implementation
references it, no implementation invents a code.

**Naming:** `A3E_<AREA>_<RULE>`, SCREAMING_SNAKE, stable forever. Codes are
never renumbered or reused; a retired rule is marked deprecated in place.

Starting set, derived from the rules that exist today — extend as needed:

| Code | Stage | Rule |
|---|---|---|
| `A3E_ENVELOPE_SCHEMA_MISSING` | 1 | `$schema` absent |
| `A3E_ENVELOPE_SCHEMA_MISMATCH` | 1 | `$schema` ≠ canonical URI |
| `A3E_ENVELOPE_VERSION_MISSING` | 1 | `a3_version` absent |
| `A3E_ENVELOPE_VERSION_MISMATCH` | 1 | `a3_version` ≠ supported version |
| `A3E_SEQ_MISSING` | 2 | `sequence` absent |
| `A3E_SEQ_TYPE` | 2 | `sequence` not a string |
| `A3E_SEQ_TOO_SHORT` | 2 | `sequence` < 2 characters |
| `A3E_SEQ_CHARSET` | 2 | `sequence` contains a character outside `[A-Za-z*]` |
| `A3E_NAME_EMPTY` | 2 | annotation name is the empty string |
| `A3E_INDEX_TYPE` | 2 | index is not an array of the expected shape |
| `A3E_INDEX_MIXED` | 2 | ptm/processing index mixes positions and ranges |
| `A3E_POS_NOT_POSITIVE` | 3 | position < 1 |
| `A3E_POS_DUPLICATE` | 3 | duplicate position within one annotation |
| `A3E_POS_UNSORTED` | 3 | positions not ascending *(see Q3)* |
| `A3E_RANGE_ARITY` | 2 | range is not a 2-element array |
| `A3E_RANGE_NOT_POSITIVE` | 3 | range endpoint < 1 |
| `A3E_RANGE_ORDER` | 3 | `start >= end` |
| `A3E_RANGE_OVERLAP` | 3 | two ranges in one annotation overlap |
| `A3E_RANGE_UNSORTED` | 3 | ranges not ascending by start *(see Q3)* |
| `A3E_POS_OUT_OF_BOUNDS` | 4 | position > `length(sequence)` |
| `A3E_RANGE_OUT_OF_BOUNDS` | 4 | range endpoint > `length(sequence)` |
| `A3E_VARIANT_POSITION_MISSING` | 2 | variant record has no `position` |
| `A3E_VARIANT_POSITION_NOT_POSITIVE` | 3 | variant position < 1 |
| `A3E_VARIANT_OUT_OF_BOUNDS` | 4 | variant position > `length(sequence)` |
| `A3E_VARIANT_RESIDUE_MISMATCH` | 4 | variant `from` ≠ residue at `position` *(new, see §6.6)* |
| `A3E_METADATA_UNIPROT_ID` | 2 | `uniprot_id` fails accession grammar *(see Q2)* |
| `A3E_UNKNOWN_FIELD` | 2 | property not permitted by the schema |

For each code the registry records: **code · stage · trigger condition · JSON
pointer/path emitted · whether the human message must name the offending value.**

### 4.2 `spec/error-paths.md` — the path grammar

The most important file in this plan, and the one most likely to be skimped.
Three implementations already disagree (§2.2).

Pin down, with a worked example for every case:

1. **Dotted or JSON Pointer?** Recommend **dotted**, since all five already
   produce dotted paths and `$schema` complicates pointer escaping. Decide once.
2. **Does the path descend to `index`?** i.e. `annotations.site.foo` (Rust) or
   `annotations.site.foo.index` (Python). **Recommend `.index`** — it names the
   field that is actually wrong, and generalises to a future sibling field.
3. **Does the path carry the element subscript?** i.e.
   `annotations.site.foo.index[3]`. **Recommend yes for per-element rules**
   (`A3E_POS_NOT_POSITIVE`, `A3E_RANGE_ORDER`) and **no for whole-set rules**
   (`A3E_POS_DUPLICATE`, `A3E_RANGE_OVERLAP`, `A3E_POS_UNSORTED`), which are
   properties of the collection, not of one element. State this rule explicitly
   — it is the one a reimplementer will get wrong.
4. **Subscripts are 0-based** (they index the JSON array, not the sequence).
   Say so loudly; the 1-based sequence positions sitting right next to them make
   this a live trap.
5. **Annotation names are inserted raw.** Specify escaping for a name containing
   `.` or `[`. Simplest defensible answer: no escaping, and note the ambiguity
   as known. Alternatively forbid `.` in annotation names — but that is a schema
   change and a breaking one.
6. **Array element order.** Issues are emitted in document order; the corpus
   compares **sets**, not sequences, so ordering is not part of the contract.
   Say that explicitly so nobody sorts defensively.

### 4.3 `spec/conformance/` — the corpus

```
spec/conformance/
  README.md              format description + how to add a case
  manifest.json          index of all cases with expectations
  valid/
    minimal.a3.json
    all-families.a3.json
    mapt.a3.json         symlink or copy of examples/mapt.a3.json
    lowercase-sequence.a3.json
    unsorted-positions.a3.json      ← valid: normalization sorts
    ...
  invalid/
    seq-too-short.a3.json
    seq-bad-charset.a3.json
    pos-out-of-bounds.a3.json
    range-overlap.a3.json
    multi-error.a3.json             ← several issues at once; the key case
    ...
```

`manifest.json`, one entry per case:

```json
{
  "cases": [
    {
      "file": "invalid/multi-error.a3.json",
      "valid": false,
      "issues": [
        { "code": "A3E_POS_OUT_OF_BOUNDS", "path": "annotations.site.actives.index[2]" },
        { "code": "A3E_RANGE_OVERLAP",     "path": "annotations.region.domains.index" }
      ]
    },
    {
      "file": "valid/unsorted-positions.a3.json",
      "valid": true,
      "normalized": "valid/unsorted-positions.normalized.json"
    }
  ]
}
```

**Assertion semantics** (state these in `spec/conformance/README.md`):

- For `valid: false`, the set of `{code, path}` pairs produced must equal the
  expected set **exactly** — no missing, no extra. Human messages are ignored.
- For `valid: true`, validation must succeed, **and** serializing the parsed
  object must equal `normalized` byte-for-byte when that key is present. This is
  how normalization (case folding, sorting, dedup) gets tested cross-language,
  and it is the cheapest possible round-trip guarantee.
- Cases are compared as **sets**; emission order is not contractual.

**Coverage requirement:** every code in `spec/error-codes.md` is exercised by at
least one case, in isolation. Add a meta-test that fails if a code has no case —
otherwise the registry and the corpus drift within two months.

**Minimum size to be credible:** ~15 valid, ~35 invalid, plus 3–5 multi-error
cases. Multi-error cases are where fail-fast implementations get caught, so they
carry most of the value; do not skimp on them.

---

## 5. The conformance runner

Each implementation ships a small runner that reads `manifest.json`, validates
each case, and compares issue sets. Five runners, one contract.

- **Rust** — `rust/tests/conformance.rs`
- **Python** — `python/.../tests/test_conformance.py`
- **TypeScript** — `typescript/tests/conformance.test.ts`
- **Julia** — a `@testset` in `julia/RtemisA3/test/runtests.jl`
- **R** — `r/tests/testthat/test_conformance.R`

Each resolves the corpus by relative path from the repo root. **Do not vendor
copies into each language directory** — one corpus, five readers, or it drifts.
(Note for the R package: `r/tests/` needs the corpus available in the built
package, or the test must skip gracefully off-repo. Use `testthat::skip_if_not()`
on the corpus path existing, and run the real check in CI where the repo is
present.)

**Failure output must name the diff**: which `{code, path}` pairs were expected
and absent, and which were produced and unexpected. A bare "sets differ" wastes
an hour every time it fires.

---

## 6. Per-language workstreams

Do Rust first — it is closest to the target and becomes the reference the other
four are read against.

### 6.1 Rust (reference implementation)

1. Introduce `A3IssueCode` as an enum with a `code()` → `&'static str`, one
   variant per registry entry.
2. Change the accumulator from `Vec<String>` to `Vec<A3Issue>` where
   `A3Issue { code, path, message }`. Keep `Display` producing today's text so
   CLI output is unchanged.
3. Re-key every existing error to the agreed path grammar (§4.2) — Rust
   currently omits `.index` and element subscripts, so this is a real change,
   not a rename.
4. Enforce explicit staging (§3.1); it is currently implicit.
5. Expose issues in `--json` output as `{code, path, message}` objects. This
   makes the CLI usable as the conformance oracle for debugging.
6. Add `rust/tests/conformance.rs`.

**Acceptance:** whole corpus passes; `a3 --json` emits codes; `--diagnose`
reports the stage at which validation stopped.

### 6.2 Python

1. `A3Issue` dataclass; unify the envelope-error dicts in `api.py` and the
   `errors: list[str]` in `_models.py` into one issue list.
2. Re-key paths to §4.2.
3. `A3ValidationError.issues` exposed as the public accessor, matching the
   TypeScript shape.
4. Add `tests/test_conformance.py`.

**Watch:** Pydantic's own `ValidationError` will fire first on shape problems
and produce Pydantic-shaped errors, not A3 issues. Either pre-validate shape in
`api.py` before the model sees the data (preferred — it matches the staged
architecture), or map `ValidationError` → A3 issues. Do not let both escape.

### 6.3 TypeScript

1. Map Zod issues → `A3Issue`. Zod's `issue.path` is an array — join per §4.2.
   Attach codes via each refinement's `params`, so the mapping is declared at
   the rule rather than inferred from the message.
2. Add the rules missing from §2.3 (confirm overlap, bounds, mixed-index
   homogeneity, empty-name).
3. Decide `uniprot_id` (see Q2) — promote or drop.
4. Add `tests/conformance.test.ts`.
5. **Check the browser entry point** (`index-browser.ts`) exports the issue type;
   rtemislive's `issueUtils.ts` consumes it and will need updating in step §9.

### 6.4 Julia

1. `A3Issue` struct; `A3ValidationError` gains an `issues::Vector{A3Issue}` field.
2. Convert `validate.jl` from `throw`-per-check to `push!`-per-check. The `path`
   threading is already there, which is why this is mechanical — but **watch the
   early returns**: several checks currently rely on `throw` to skip subsequent
   work on the same value (e.g. a non-integer that would then be compared to 1).
   Each of those needs an explicit guard.
3. Enforce staging.
4. Add the conformance testset.

### 6.5 R (largest)

1. **New file `r/R/validate.R`** with `validate_a3_raw(x)`, taking the
   `jsonlite::fromJSON()` output and returning a data frame or list of issues.
   Port structure directly from `rust/src/validation.rs` — same stages, same
   order, same codes. Do not try to reuse the S7 validators for this.
2. `read_A3json()` calls `validate_a3_raw()` first, and on a non-empty issue
   list aborts **once** with all issues formatted via `cli::cli_abort()` with a
   bulleted body. Attach the issue list to the condition object so it is
   programmatically reachable — use the rtemis condition-class convention
   (`class = c("a3_validation_error", "rtemis_error", ...)`).
3. Add an exported `validate_A3json(filepath)` that *returns* the issue list
   instead of aborting. rtemislive-equivalent workflows and the paper both need
   a non-throwing entry point.
4. **Leave every S7 validator exactly as it is.** Per §3.4 they are the
   constructor backstop.
5. Add `test_conformance.R`.
6. Minor cleanups while in there:
   - `A3Variant@position` is an `A3Position` (a whole index object) holding a
     single integer, with a validator asserting length 1. Consider a plain
     integer property. Behaviour-neutral, reduces surprise.
   - No `uniprot_id` validation (pending Q2).
   - `A3Annotation`'s validator uses `sapply(...)` over possibly-empty lists;
     `sapply(list(), ...)` returns `list()` and `all(list())` is `TRUE`, so it
     happens to work — but replace with `vapply(..., logical(1L))` guarded on
     length, per the repo's own R conventions.

### 6.6 New rule: `A3E_VARIANT_RESIDUE_MISMATCH` (all five)

Stage 4. If a variant record carries a `from` field and it is a single-character
string, it must equal the residue at `position` in the sequence.

Deliberately narrow: `from` is an *optional, unenforced* field of an open record
(§2.3 of the schema), so the rule fires only when the field is present and
well-shaped. Absence is not an error; a `from` of the wrong shape is not this
rule's problem.

This is worth doing for the paper as well as for users: it is the one rule that
catches a *biological* error rather than a structural one, and it is what makes
the "validation caught a real problem in real public data" sentence in the
manuscript possible (MAPT + ClinVar, isoform coordinate mismatches).

---

## 7. Schema generation: `prop_*` factories in the R implementation

**Committed.** The R class declarations become the source of truth for
`schema/a3.schema.json`, which is generated by `S7_to_JSONSchema()` rather than
hand-maintained. A3-specific factories are added as needed; the ML-specific
spec fields are not carried over.

### 7.1 What is actually there

`rtemis/R/010_Props.R` (3,639 lines) is much more than a set of property
constructors. The relevant pieces:

- `PropertySpec` — an S7 class holding a property's type, bounds, enum,
  container, nullability, defaults, and about a dozen flags, as **data rather
  than closures**.
- `prop_boolean`, `prop_integer`, `prop_float`, `prop_string`, `prop_map`,
  `prop_array`, `prop_matrix`, `prop_factor`, `prop_table`, `prop_struct`,
  `prop_const`, `prop_bag` — factories returning S7 properties with a
  spec-driven validator attached.
- `validate_with_spec()` — one shared validator body for all of them.
- **`S7_to_JSONSchema()`** — walks a class's properties, reads each attached
  spec, and emits a draft 2020-12 JSON Schema mechanically.

### 7.2 Why this is strategically interesting for A3

`S7_to_JSONSchema()` inverts A3's architecture. Today `schema/a3.schema.json` is
hand-maintained and five implementations independently try to conform to it.
With props, the R class declarations would **generate** the schema, and the
schema stays correct by construction.

For the paper that is a materially stronger claim — "the schema is derived from
a typed declaration, not maintained alongside it" — and it is the same argument
the visualization section makes about the visualization model being derived from
the data model. It would make the paper's design story consistent end to end.

### 7.3 What to port, and what to leave behind

Copy into `r/R/000_Props.R` as **internal, unexported** functions. Keep a file
header recording provenance and that this is destined for extraction into a
shared package, so the next reader does not treat it as A3-native code.

**Port:** `PropertySpec`, `spec_fields`, `spec_object`, `spec_r_kind`,
`validate_with_spec` and its helpers, `make_prop`, `spec_validator`,
`get_spec`, `prop_to_schema` / `spec_to_schema`, `S7_to_JSONSchema`, and the
factories A3 actually uses: `prop_boolean`, `prop_integer`, `prop_string`,
`prop_array`, `prop_map`, `prop_struct`, `prop_const`, `prop_bag`.

**Leave behind (strip from `PropertySpec` entirely, not just unused):**
`tunable`, `tune_on_null`, `default_on_null`, `data_bound`, `data_dependent`,
`broadcast`, and the `DATA_BOUNDS` / `NAME_BOUNDS` / `DATA_BOUND_NOUN`
vocabulary. Also drop the `matrix`, `table` and `factor` containers — A3 has no
use for any of them, and each carries a validator branch and a schema branch.
`prop_float`, `prop_matrix`, `prop_table`, `prop_factor` go with them.

Stripping rather than ignoring matters: `tunable` and `broadcast` are the only
things that currently emit `oneOf`, and §7.4 adds a *general* union mechanism.
Leaving both in place would give two overlapping ways to emit the same keyword.

`rtemis.core` is already in the R package's `Imports`, so `rtemis.core::abort()`
calls can stay as-is — but prefer routing them through `cli::cli_abort()` to
match `r/AGENTS.md`.

### 7.4 New factories and spec fields A3 needs

Derived by walking `schema/a3.schema.json` against what the factories currently
express. Each row is a gap that blocks byte-exact regeneration.

| A3 shape | Schema keywords | Status | Work |
|---|---|---|---|
| `$schema`, `a3_version` | `const` | ✅ `prop_const()` | none |
| `position` | `type: integer`, `minimum: 1` | ✅ `prop_integer(min = 1L)` | none |
| `positionIndex` | array of integers ≥1 | ✅ `prop_integer(vector = TRUE, min = 1L)` | none |
| `sequence` | `minLength: 2`, `pattern: ^[A-Za-z*]+$` | ❌ | **extend `prop_string()`** with `min_length`, `max_length`, `pattern` |
| `range` | array, `minItems: 2`, `maxItems: 2` | ❌ | **add `max_items`** to the array container (spec field, validator branch, schema emitter) |
| `rangeIndex` | array of ranges | ✅ `prop_array(items = <range>)` | none |
| `mixedIndex` | `anyOf: [positionIndex, rangeIndex]` | ❌ | **new `prop_union()`** — see below |
| `positionAnnotationEntry` etc. | object, `required: [index]`, `additionalProperties: false` | ✅ `prop_struct()` | none |
| `metadata` | object, **no** required members, `additionalProperties: false` | ⚠️ | `prop_struct()` resolves `required <- required %||% names(members)`, so NULL means *all required*. **Make an explicit `character(0)` mean "none required"** and confirm it survives the `PropertySpec` validator |
| `variantRecord` | `required: [position]`, additional properties **allowed** | ❌ | **add `closed = FALSE`** to `prop_struct()` (emit no `additionalProperties: false`); `prop_bag()` is too loose — it has no required key |
| `annotations.site` etc. | object with `additionalProperties: {$ref}` **and** `propertyNames: {minLength: 1}` | ⚠️ | `prop_map()` covers `additionalProperties`; **add a `key_min_length` / `property_names` argument** for the non-empty-name constraint |
| top-level | `additionalProperties: false` | ✅ | `S7_to_JSONSchema(closed = TRUE)` |

#### `prop_union()` — the one genuinely new factory

```r
prop_union(alternatives, nullable = FALSE, description = "")
```

`alternatives` is an unnamed list of `prop_*` properties. Emits
`anyOf: [<schema of each>]`. Validation succeeds if the value satisfies **any**
alternative.

Three things to get right:

1. **`PropertySpec` needs an `alternatives` field**, stored like `items` and
   `members` — as `class_any`, converted recursively by `spec_fields()` /
   `spec_object()`, because the class cannot reference itself while its own
   `properties` list is being evaluated. Follow exactly what `members` does.
2. **`spec_r_kind()` must resolve.** For `mixedIndex` the alternatives are an
   integer vector (`atomic`) and a list of pairs (`list`). They disagree, so the
   union's R class has to widen to `class_list | class_integer` — or the factory
   requires all alternatives to share an R kind and A3 accepts the widening.
   **Decide this explicitly; it is the subtlest part of the port.** Simplest
   defensible rule: if the alternatives' `spec_r_kind()` values differ, the
   property's S7 class is the union of their base classes.
3. **Error reporting under a union is hard.** When every alternative fails,
   which message does the user get? "matched no alternative" is useless for a
   50-position index. Recommend: report the alternative that got *furthest*, or
   emit `A3E_INDEX_MIXED` directly when the value is an array whose elements are
   not homogeneous — which is the actual failure A3 cares about. Do not let the
   generic union message be the only thing a user sees.

#### Validator parity is the real cost

Every new spec field needs **three** implementations, not one: the schema
emitter, the `validate_with_spec()` branch, and the `PropertySpec` validator
that rejects a nonsensical declaration (e.g. `max_items < min_items`,
`pattern` on a non-string, `closed = FALSE` with no members). rtemis's own
factories are careful about the third — match that standard, since a bad
declaration failing at package load is the property that makes the whole
approach safe.

### 7.5 Acceptance: byte-exact regeneration

The gate for this workstream:

```
S7_to_JSONSchema(A3, id = "https://schema.rtemis.org/a3/v1/schema.json", ...)
  ==  schema/a3.schema.json
```

- Add a test asserting equality, so the two can never drift.
- Add a `make schema` target that regenerates the file, and a CI check that the
  committed file matches regeneration (§8).
- **Key order and whitespace count.** Normalize both sides through the same
  serializer before comparing, and commit the serializer's output as the
  canonical file so the check is stable.

**If regeneration and the committed schema differ, the generated one wins** —
breaking changes are in scope, and a hand-written schema that the typed
declaration disagrees with is precisely the class of bug this change exists to
eliminate. But every such diff must be **recorded in the PR and reflected in the
conformance corpus**, because a schema change is a behaviour change for the
other four implementations. Expected candidates: absent `description` keywords,
integer-vs-number widening, and the `propertyNames` constraint.

### 7.6 Sequencing risk

This workstream and §4 (the conformance contract) both touch what "valid" means,
from opposite ends. Keep them apart:

- §4 defines **validation reporting** — codes, paths, staging. It does not
  change what is accepted.
- §7 changes the **provenance** of the schema. Its acceptance criterion is that
  the accepted set does not change.

If §7 turns out to change the accepted set (see the "generated one wins" case
above), that is a schema change and it must land **before** the corpus is
frozen. Run §7 early and in parallel with §4 for exactly this reason — not
because it is unimportant, but so its surprises arrive before the corpus is
written rather than after.

---

## 7A. Standing workstream: per-language quality audit

The implementations predate the current conventions and have not been reviewed
since. Run one audit pass per language, in the same PR as that language's §6
work — the file is already open, and a separate audit pass never happens.

**This is not a licence to rewrite.** The audit produces a list; anything not
touching correctness, the public API, or the conformance contract goes on the
list and waits. What it must not do is quietly change behaviour the corpus is
about to pin down.

### Audit checklist, per language

1. **Idiom currency.** Does the code use the current idioms of its language and
   of the repo's own `AGENTS.md`? For R specifically, `AGENTS.md` mandates S7
   throughout, `cli::cli_abort()` with corrective messages, `x[["a"]]` over
   `x$a`, and complete roxygen — check compliance rather than assuming it.
2. **Dependency currency.** Verified 2026-08-06 and all current: Rust edition
   2024 / MSRV 1.85, `clap` 4, `thiserror` 2, `serde` 1; TypeScript with Zod 4,
   TS 6, vitest 4, Biome 2; Python with Pydantic ≥2.12.5. **No upgrades appear
   needed** — confirm, don't churn. Re-check at submission (`AGENTS.md` requires
   a web check for current versions).
3. **Are newer language features being left on the table?** e.g. Zod 4 features
   the code predates; Pydantic 2.12 validators; Rust 2024-edition patterns. Only
   act where it removes code or catches a bug.
4. **Public API ergonomics.** Are the five APIs as parallel as the languages
   allow? Today they are close but not audited: R uses `create_A3` /
   `read_A3json`, Python `create_a3` / `read_a3json`, TypeScript `readJSON` /
   `writeJSON`. Naming should be *idiomatic per language* but *structurally
   identical* — same operations, same arity, same return shapes. Document the
   correspondence in one table in the root README; a reader moving between
   languages should not have to guess.
5. **Docs.** `AGENTS.md` says everything documented and current, no exceptions.
   Check each README against the actual API.
6. **Dead code and `__dev/`.** Each language has a `__dev/` directory; confirm
   nothing shipped depends on it.

### Already-found audit items

- **R: `jsonlite` is in `Suggests`, not `Imports`.** There are 13
  `jsonlite::` call sites across `a3.R` and `utils_a3.R`, and **not one
  `requireNamespace()` guard in the package**. `to_json()`, `read_A3json()` and
  `write_A3json()` — the package's primary I/O path — call it unconditionally.
  A user who installs `rtemis.a3` without `jsonlite` gets a runtime failure on
  core functionality. **Move to `Imports`.** A plain `R CMD check` does not
  catch this (which is why the package is currently clean), but
  `_R_CHECK_DEPENDS_ONLY_=true` does, and it violates the CRAN policy that
  Suggests packages be used conditionally. Add the depends-only check to CI so
  the class of bug cannot recur.
- **R: `A3Variant@position`** wraps a single integer in a full `A3Position`
  object (§6.5).
- **R: `sapply()` over possibly-empty lists** in `A3Annotation`'s validator
  (§6.5).
- **TypeScript: `uniprot_id` rule exists nowhere else** (§2.3, Q2).
- **The `specs/` directory is advertised and absent** (H1).

---

## 7B. Release and versioning strategy

Republication is nearly free except for one package, which changes how this work
should be shipped.

| Channel | Latency | Notes |
|---|---|---|
| r-universe (`rtemis.a3`) | immediate | builds from the repo |
| PyPI (`rtemis_a3`) | immediate | |
| npm + JSR (`@rtemis/a3`) | immediate | two registries, publish together |
| crates.io (`rtemis_a3`) | immediate | irreversible: a published version is never replaced |
| Julia General (`RtemisA3`) | ~hours | registry PR + merge |
| **CRAN (`rtemis.a3`)** | **days** | manual review; the only real constraint. **Currently passes with 0 errors / 0 warnings / 0 notes — keep it that way.** A clean check is easy to hold and expensive to recover, and §7 adds a substantial amount of new R code. Run `R CMD check --as-cran` on every R PR, plus `_R_CHECK_DEPENDS_ONLY_=true`. |

### Implications

1. **Do not gate the other four on CRAN.** Ship them as they land. r-universe
   carries R users in the meantime, and it is what the paper's availability
   statement can point at for the current version.
2. **The paper's availability statement must be version-pinned and honest.**
   Give the version of each package the manuscript describes, and state the
   canonical install channel per language. If CRAN lags at submission, cite
   r-universe as primary for R and note CRAN as also available — do not cite a
   CRAN version whose behaviour differs from what the paper describes. Editors
   check these URLs and may install.
3. **Version bump policy for this work.** The path-grammar change (§4.2) alters
   every error path, and the new rule (§6.6) rejects documents that previously
   validated. Both are breaking for anyone parsing error output or relying on
   lenient acceptance. Bump the **minor** version everywhere while pre-1.0, and
   write a `CHANGELOG` entry naming the path change explicitly.
4. **`a3_version` does not change.** None of this alters the *document* format —
   only validation reporting and one added rule. The `1.0.0` const stays. Be
   careful not to conflate package version with `a3_version`; the manuscript
   should state the distinction in one sentence, because a reader will assume
   they track each other.
5. **Freeze before submission.** Cut a release of all five from one commit, tag
   it, mint a Zenodo DOI for that tag, and cite the DOI. "All five conform, in
   the same commit, at the versions cited" is the claim — it needs one commit to
   point at.
6. **Order of operations at release time:** merge → tag → crates.io (irreversible,
   so last among the instant ones is safer) → npm/JSR → PyPI → r-universe picks
   up automatically → Julia registry PR → CRAN submission. Then Zenodo.

---

## 8. CI and Makefile

1. `Makefile`: add `test-conformance` running all five runners, and fold it into
   the existing `test` target.
2. Each of the five CI workflows gains a conformance step. **All five must run
   the same corpus in the same commit** — that is the property the paper claims.
3. Add a workflow (or a step in one) that fails if a code in
   `spec/error-codes.md` has no corresponding corpus case.
4. `Makefile`: add `schema` (regenerate `schema/a3.schema.json` from the R
   classes) and **a CI check that the committed schema equals regeneration**
   (§7.5). Without this, generation is a one-off and the drift returns.
5. R CI: add `R CMD check --as-cran` and a `_R_CHECK_DEPENDS_ONLY_=true` run
   (§7A, §7B).
6. Consider a small `conformance-report` target emitting the coverage matrix
   (codes × implementations) as Markdown — that is Supplementary Table S3
   generated rather than transcribed.

---

## 9. Downstream: rtemislive

Not in this repo, but this plan breaks it if ignored.

`live/src/components/a3doc/issueUtils.ts` consumes `@rtemis/a3` validation
issues. Once §6.3 lands, issues carry a `code`. Update `issueUtils.ts` to key
off `code` rather than parsing messages, and use it to attach each issue to the
right form field via its `path`. This is a strict improvement — the current
message-parsing approach breaks whenever a message is reworded — and it is what
makes Figure 1c (inline validation against a dotted path) worth showing.

---

## 10. Sequencing

```
H1–H3 hygiene ──┐
                │
§7 props port ──┤   run EARLY, in parallel with §4 (see §7.6)
  └─ any schema │   ── if it changes the accepted set, that lands
     diff ──────┤      BEFORE the corpus is frozen
                │
                ├──► §4 CONTRACT (codes + paths + corpus)  ◄── blocks all below
                          │
                          ├──► §6.1 Rust  + audit ──► reference for the rest
                          ├──► §6.2 Python + audit
                          ├──► §6.3 TypeScript + audit ──► §9 rtemislive
                          ├──► §6.4 Julia + audit
                          └──► §6.5 R + audit   ◄── merges with §7's R work
                                   │
                                   └──► §6.6 variant-residue rule (all five)
                                              │
                                              ├──► §8 CI ──► paper §2.3, §3.2
                                              └──► §7B release + Zenodo tag
```

Suggested order of attack: hygiene → **props port and contract in parallel** →
resolve any schema diff the port surfaces → freeze the corpus → Rust → corpus
expansion driven by what Rust reveals → Python + TypeScript → Julia → R → new
rule → CI → release.

Two coupling notes:

- **§7 must run early**, not late. Its risk is that generation disagrees with
  the committed schema; that surprise has to arrive before the corpus encodes
  the old behaviour, not after.
- **§7's R work and §6.5's R work land in the same package** and will conflict
  if run concurrently. Sequence them: props port and schema generation first,
  then `validate_a3_raw()` on top — the raw validator can then be written
  against the generated shape rather than retrofitted to it.

Fold each language's §7A audit into that language's §6 PR.

---

## 11. Acceptance criteria

The plan is done when:

- [ ] `spec/error-codes.md`, `spec/error-paths.md`, `spec/conformance/` exist and are reviewed
- [ ] Every code in the registry has ≥1 isolating corpus case; a meta-test enforces this
- [ ] The corpus has ≥3 multi-error cases and every implementation reports **all** issues in each
- [ ] All five implementations pass the full corpus in CI, in the same commit
- [ ] Every valid case with a `normalized` companion round-trips identically in all five
- [ ] `a3 --json` emits `{code, path, message}`
- [ ] Each README documents the two entry points and their differing error behaviour (§3.4)
- [ ] The §2.3 table has no `?` and no unintended `❌`
- [ ] Licences reconciled; `specs/` references resolved
- [ ] `jsonlite` moved to `Imports` in the R `DESCRIPTION`
- [ ] An audit pass (§7A) is recorded for each of the five, with deferred items listed rather than silently dropped
- [ ] A cross-language API correspondence table exists in the root README
- [ ] All five released from **one tagged commit**, with a Zenodo DOI, and the manuscript's availability statement pins those versions
- [ ] `schema/a3.schema.json` is generated from the R classes; `make schema` is idempotent and CI fails on drift
- [ ] Every diff between the generated and previously-committed schema is recorded in the PR and reflected in the corpus
- [ ] `PropertySpec` in `rtemis.a3` carries **no** ML-specific fields (`tunable`, `data_bound`, `broadcast`, …)
- [ ] `prop_union()`, `max_items`, `prop_string(pattern=, min_length=)`, `prop_struct(closed=)`, and the `prop_map()` key constraint each have all three parts: schema emitter, value validator, and declaration validator
- [ ] `R CMD check --as-cran` still reports 0 errors / 0 warnings / 0 notes, and `_R_CHECK_DEPENDS_ONLY_=true` passes

---

## 12. Open questions

| # | Question | Needs |
|---|---|---|
| Q1 | Dotted paths or JSON Pointer? *(recommend dotted)* | Decide in §4.2 |
| Q2 | Is `uniprot_id` accession-grammar validation part of the spec? It exists only in TypeScript today. Promoting it is a **tightening** — documents that validate now would stop validating. | Decide before §6.3 |
| Q3 | Are unsorted positions an **error** or **normalized silently**? Rust's `normalization.rs` sorts; a sorted-order *error* code would contradict that. The two cannot both be true. Resolve, then keep or drop `A3E_POS_UNSORTED` / `A3E_RANGE_UNSORTED`. **This is the subtlest item in the plan** — normalization and validation are making different promises today. |
| Q4 | Does `a3_version` stay `const`, or move to a supported-range with a migration path? Affects `A3E_ENVELOPE_VERSION_MISMATCH` semantics and the Discussion's limitations paragraph. |
| Q5 | Annotation names containing `.` or `[` — escape in paths, or forbid in the schema? Forbidding is breaking. |
| Q6 | Does the corpus live in this repo, or get published separately so third-party implementations can claim conformance? Publishing it is a stronger paper claim and costs almost nothing extra. |
| Q7 | `prop_union()`: when alternatives have different `spec_r_kind()` (an integer vector vs a list of pairs, as `mixedIndex` does), does the property's S7 class widen to their union, or must alternatives share a kind? *(recommend widen)* — §7.4 |
| Q8 | What does a union report when every alternative fails? A generic "matched no alternative" is useless at A3's scale; recommend emitting `A3E_INDEX_MIXED` directly for the homogeneity case. — §7.4 |
| Q9 | Does the R package become a **build-time** dependency of the schema? If the schema is generated from R classes, regenerating it requires R — which means the Python/TS/Julia/Rust maintainers cannot regenerate it. Acceptable (the artifact is committed), but state it in `spec/` so nobody is surprised. |
| Q10 | Should the generated schema carry `description` keywords? The hand-written one does; `S7_to_JSONSchema()` emits them from each spec's `description`. Populating every A3 factory call with the existing prose keeps the published schema self-documenting — decide before regenerating, since it is the most likely source of diff noise. |
