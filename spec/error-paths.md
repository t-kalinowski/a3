# A3 error paths

Every validation issue carries a `path`: an **RFC 6901 JSON Pointer** into the
document being validated. This file is normative.

## Why JSON Pointer

- It is the locator JSON Schema itself uses. Draft 2020-12's standard output
  format gives every error an `instanceLocation`, which is a JSON Pointer. A3
  issue paths are the same thing, so A3's output and an off-the-shelf validator's
  output are directly comparable.
- Escaping is decided by the RFC rather than by us. Annotation names containing
  `.`, `[`, spaces, parentheses, or commas need no special handling and no schema
  restriction.
- Array subscripts are 0-based by definition of the standard, which removes the
  live trap of 1-based sequence positions sitting next to 0-based array indices.
- Constructing one is three lines in every language, and the rules cannot drift
  because they are not ours.

## The grammar

A pointer is a sequence of reference tokens, each preceded by `/`:

```
pointer   = *( "/" token )
token     = *( unescaped / escaped )
escaped   = "~0"    ; a literal ~
          / "~1"    ; a literal /
```

Exactly two characters are escaped, and only these two:

| Literal in the key | Written in the pointer |
|---|---|
| `~` | `~0` |
| `/` | `~1` |

Nothing else is escaped. `$`, `.`, `[`, `]`, spaces, quotes, commas, and
non-ASCII characters are all written verbatim. In particular `$schema` is
written `/$schema` — `$` has no special meaning in a JSON Pointer.

The **empty pointer** `""` denotes the whole document. It is the path of
`A3E_DOC_NOT_OBJECT` and of nothing else.

Array subscripts are decimal, 0-based, without leading zeros: `/annotations/variant/0`.

## Worked examples

```
""                                                  the whole document
/$schema                                            the $schema member
/a3_version
/sequence
/annotations
/annotations/site                                   the site family
/annotations/site/Active site                       one named site annotation
/annotations/site/Active site/index                 its index, as a whole
/annotations/site/Active site/index/2               the 3rd element of that index
/annotations/site/Active site/type
/annotations/region/Microtubule Binding Domain (R1, R3, R4)/index/0
/annotations/ptm/Phosphorylation/index
/annotations/variant/3                              the 4th variant record
/annotations/variant/3/position
/annotations/variant/3/from
/metadata
/metadata/uniprot_id
```

Escaping in practice:

| Annotation name | Pointer to its `index` |
|---|---|
| `Active site` | `/annotations/site/Active site/index` |
| `Domain (R1, R3, R4)` | `/annotations/site/Domain (R1, R3, R4)/index` |
| `a.b` | `/annotations/site/a.b/index` |
| `a/b` | `/annotations/site/a~1b/index` |
| `a~b` | `/annotations/site/a~0b/index` |
| `` (empty string) | `/annotations/site/` |

The last row is not a typo. A pointer whose final reference token is empty
denotes the member named `""`, which is exactly what `A3E_NAME_EMPTY` reports.

## Rule 1 — missing members point at where the member would be

A missing required member is reported at the pointer the member *would* occupy,
not at its containing object:

```
A3E_SEQ_MISSING                 →  /sequence                      (not "")
A3E_INDEX_MISSING               →  /annotations/site/foo/index    (not /annotations/site/foo)
A3E_VARIANT_POSITION_MISSING    →  /annotations/variant/0/position
A3E_ENVELOPE_SCHEMA_MISSING     →  /$schema
```

This is a **deliberate divergence** from JSON Schema's output format, where a
`required` failure reports the containing object as `instanceLocation`. A3
diverges because the pointer's job here is to tell an editor or form which field
to attach the message to, and "the document root" is not an answer a user
interface can use. RFC 6901 defines a pointer as a locator; it does not require
that the locator resolve to an existing value, and failure to resolve is
precisely the "this is missing" signal.

## Rule 2 — element subscripts appear only for per-element rules

A rule that is a property of **one element** carries the element's subscript. A
rule that is a property of the **whole collection** stops at the collection.

| Rule | Kind | Path |
|---|---|---|
| `A3E_POS_NOT_POSITIVE` | per-element | `/annotations/site/foo/index/0` |
| `A3E_POS_OUT_OF_BOUNDS` | per-element | `/annotations/site/foo/index/2` |
| `A3E_INDEX_ELEMENT_TYPE` | per-element | `/annotations/site/foo/index/1` |
| `A3E_RANGE_ARITY` | per-element | `/annotations/region/foo/index/1` |
| `A3E_RANGE_ORDER` | per-element | `/annotations/region/foo/index/0` |
| `A3E_RANGE_OUT_OF_BOUNDS` | per-element | `/annotations/region/foo/index/1` |
| `A3E_RANGE_ENDPOINT_NOT_INTEGER` | per-element | `/annotations/region/foo/index/0` |
| `A3E_RANGE_ENDPOINT_NOT_POSITIVE` | per-element | `/annotations/region/foo/index/0` |
| `A3E_POS_DUPLICATE` | whole-set | `/annotations/site/foo/index` |
| `A3E_RANGE_OVERLAP` | whole-set | `/annotations/region/foo/index` |
| `A3E_INDEX_MIXED` | whole-set | `/annotations/ptm/foo/index` |

A duplicate is not a property of either of the two positions involved, and an
overlap is not a property of either range — both are properties of the set. This
is the rule a reimplementer is most likely to get wrong, so the registry marks
every code with its kind.

## Rule 3 — subscripts index the document, not the sequence

`/annotations/site/foo/index/2` is the **third** element of the index array.
If that element holds the value `2`, then residue **2** of the sequence is what
the message talks about, and array element **2** is what the path talks about.
They are different numbers naming different things and they will sit next to
each other in the same message. Sequence positions are 1-based; array subscripts
are 0-based.

## Rule 4 — a subscript names the array as it stands in that stage

Sorting happens **between Stage 2 and Stage 3**. An index cannot be sorted until
its elements are known to be sortable, and it must be sorted before overlap and
duplication can be checked. So:

| Stage | Subscripts name | Because |
|---|---|---|
| 2 | the array **as written in the document** | it has not been sorted yet, and may not be sortable |
| 3, 4 | the array **after sorting** | positions are ascending, ranges are ordered by start |

Concretely, for `"index": [5, 0, -3]`:

```
Stage 2 order:  [5, 0, -3]      ← nothing fires here
Stage 3 order:  [-3, 0, 5]      ← A3E_POS_NOT_POSITIVE at /index/0 and /index/1
```

The alternative — carrying original subscripts through the sort — costs
bookkeeping in five languages to produce a locator pointing at a value the user
would have to re-find anyway. Consumers that need to highlight the original text
should match on the value, not the subscript.

## Rule 5 — path ranges over the document, not the object model

Paths describe the **JSON document**, so an implementation whose in-memory model
differs from the wire format still emits wire-format paths. R stores a variant's
extra fields in a nested `info` list and Rust stores them in a flattened `extra`
map, but both report `/annotations/variant/0/from`, because that is where the
value lives in the file.

## Rule 6 — order is not contractual

Issues are emitted in document order where that is natural, but the corpus
compares issue **sets**. Two implementations that produce the same
`{code, path}` pairs in different orders both conform. Do not sort defensively,
and do not write a test that depends on order.

## Constructing a pointer

The whole of it, in any language:

```
escape(token) = token.replace("~", "~0").replace("/", "~1")
pointer(tokens) = concat("/" + escape(t) for t in tokens)
```

Replace `~` before `/`, or a key containing `/` becomes `~01` instead of `~1`.
