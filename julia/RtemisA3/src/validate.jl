# Four-stage A3 validation over the raw parsed JSON.
#
# Validation runs against the `OrderedDict` / `Vector` / scalar tree that
# `JSON.parse` produces, not against the typed `A3` structs. That is what makes
# collect-all reporting possible: constructing a typed value fails at the first
# problem, while validating the raw tree lets every problem in a stage be
# recorded with a code and a path.
#
#   Within a stage, collect everything. Across stages, short-circuit.
#
#     JSON text
#        |
#        +- JSON.parse ----------> A3ParseError (single, fatal, not collectable)
#        v
#     OrderedDict / Vector / scalar
#        +- STAGE 1  envelope     document is an object, declares itself A3 1.0.0
#        +- STAGE 2  structural   types, shapes, required members, charset, names
#        +- STAGE 3  intra-field  positions >=1/deduped, ranges ordered/disjoint
#        +- STAGE 4  contextual   bounds vs sequence length, variant `from` residue
#        v  (only if no stage produced an issue)
#     A3
#
# Short-circuiting is a correctness requirement, not an optimization. A document
# whose `sequence` fails Stage 2 has no known length, so Stage 4 would emit one
# spurious out-of-bounds issue per annotated position.
#
# The full contract lives in spec/error-codes.md and spec/error-paths.md.

const _A3_SCHEMA_URI = "https://schema.rtemis.org/a3/v1/schema.json"
const _A3_VERSION = "1.0.0"

const _TOP_LEVEL_KEYS = ("\$schema", "a3_version", "sequence", "annotations", "metadata")
const _FAMILIES = ("site", "region", "ptm", "processing", "variant")
const _NAMED_FAMILIES = ("site", "region", "ptm", "processing")
const _ENTRY_KEYS = ("index", "type")
const _METADATA_KEYS = ("uniprot_id", "description", "reference", "organism")

# ─── Draft ────────────────────────────────────────────────────────────────────

# An index whose geometry is settled but whose values are not yet checked.
# Values are kept as `Int` but not range-checked, so a negative position
# survives Stage 2 intact and Stage 3 can report it.
mutable struct DraftEntry
    index_path::String        # JSON Pointer to this entry's `index`
    geometry::Symbol          # :positions or :ranges
    positions::Vector{Int}
    ranges::Vector{Tuple{Int,Int}}
    type::String
end

struct DraftVariant
    path::String              # JSON Pointer to this variant record
    position::Int
    extra::OrderedDict{String,Any}
end

mutable struct Draft
    sequence::String
    site::OrderedDict{String,DraftEntry}
    region::OrderedDict{String,DraftEntry}
    ptm::OrderedDict{String,DraftEntry}
    processing::OrderedDict{String,DraftEntry}
    variant::Vector{DraftVariant}
    metadata::A3Metadata
end

_new_draft() = Draft(
    "",
    OrderedDict{String,DraftEntry}(),
    OrderedDict{String,DraftEntry}(),
    OrderedDict{String,DraftEntry}(),
    OrderedDict{String,DraftEntry}(),
    DraftVariant[],
    A3Metadata("", "", "", ""),
)

# Every named entry across the four named families, in family order.
_entries(d::Draft) = Iterators.flatten((
    values(d.site),
    values(d.region),
    values(d.ptm),
    values(d.processing),
))

# ─── Public entry point ───────────────────────────────────────────────────────

"""
    validate(raw) -> Tuple{Union{A3,Nothing}, Vector{A3Issue}}

Validate a raw parsed JSON value as an A3 document.

Returns the normalized [`A3`] and an empty issue list on success, or `nothing`
and every issue found in the earliest stage that produced one. This is the
non-throwing entry point; `A3(raw)` wraps it and throws.
"""
function validate(raw)
    issues = A3Issue[]

    # ── Stage 1 ───────────────────────────────────────────────────────────
    _stage1_envelope(raw, issues)
    isempty(issues) || return (nothing, issues)
    obj = raw::AbstractDict

    # ── Stage 2 ───────────────────────────────────────────────────────────
    draft = _stage2_structural(obj, issues)
    isempty(issues) || return (nothing, issues)

    # ── Stage 3 ───────────────────────────────────────────────────────────
    _stage3_intra_field(draft, issues)
    isempty(issues) || return (nothing, issues)

    # ── Stage 4 ───────────────────────────────────────────────────────────
    _stage4_contextual(draft, issues)
    isempty(issues) || return (nothing, issues)

    return (_build(draft), issues)
end

# ─── Stage 1 — Envelope ───────────────────────────────────────────────────────

# When the root is not an object, A3E_DOC_NOT_OBJECT is the only issue reported:
# nothing below it can be located.
function _stage1_envelope(raw, issues::Vector{A3Issue})
    if !(raw isa AbstractDict)
        push!(
            issues,
            A3Issue(
                "A3E_DOC_NOT_OBJECT",
                "",
                "an A3 document must be a JSON object, got $(_render(raw))",
            ),
        )
        return
    end
    _check_const(
        raw,
        "\$schema",
        _A3_SCHEMA_URI,
        "A3E_ENVELOPE_SCHEMA_MISSING",
        "A3E_ENVELOPE_SCHEMA_MISMATCH",
        issues,
    )
    _check_const(
        raw,
        "a3_version",
        _A3_VERSION,
        "A3E_ENVELOPE_VERSION_MISSING",
        "A3E_ENVELOPE_VERSION_MISMATCH",
        issues,
    )
end

# A wrong type and a wrong value are the same failure: the schema declares both
# envelope members with `const`, and a `const` failure is one failure.
function _check_const(
    obj::AbstractDict,
    key::String,
    expected::String,
    missing_code::String,
    mismatch_code::String,
    issues::Vector{A3Issue},
)
    path = pointer("", key)
    if !haskey(obj, key)
        push!(
            issues,
            A3Issue(missing_code, path, "'$key' is required and must be '$expected'"),
        )
    elseif obj[key] != expected
        push!(
            issues,
            A3Issue(
                mismatch_code,
                path,
                "'$key' must be '$expected', got $(_render(obj[key]))",
            ),
        )
    end
end

# ─── Stage 2 — Structural ─────────────────────────────────────────────────────

function _stage2_structural(obj::AbstractDict, issues::Vector{A3Issue})::Draft
    _unknown_fields(obj, _TOP_LEVEL_KEYS, "", issues)

    draft = _new_draft()
    seq = _stage2_sequence(obj, issues)
    seq === nothing || (draft.sequence = seq)

    # `annotations` and `metadata` are optional; both default to empty.
    annotations = get(obj, "annotations", nothing)
    if annotations !== nothing
        if !(annotations isa AbstractDict)
            push!(
                issues,
                A3Issue(
                    "A3E_ANNOTATIONS_NOT_OBJECT",
                    "/annotations",
                    "'annotations' must be an object, got $(_render(annotations))",
                ),
            )
        else
            _stage2_annotations(annotations, draft, issues)
        end
    end

    metadata = get(obj, "metadata", nothing)
    if metadata !== nothing
        if !(metadata isa AbstractDict)
            push!(
                issues,
                A3Issue(
                    "A3E_METADATA_NOT_OBJECT",
                    "/metadata",
                    "'metadata' must be an object, got $(_render(metadata))",
                ),
            )
        else
            draft.metadata = _stage2_metadata(metadata, issues)
        end
    end

    draft
end

function _stage2_sequence(obj::AbstractDict, issues::Vector{A3Issue})::Union{String,Nothing}
    if !haskey(obj, "sequence")
        push!(issues, A3Issue("A3E_SEQ_MISSING", "/sequence", "'sequence' is required"))
        return nothing
    end
    raw = obj["sequence"]
    if !(raw isa AbstractString)
        push!(
            issues,
            A3Issue(
                "A3E_SEQ_NOT_STRING",
                "/sequence",
                "'sequence' must be a string, got $(_render(raw))",
            ),
        )
        return nothing
    end

    upper = uppercase(String(raw))
    if length(upper) < 2
        push!(
            issues,
            A3Issue(
                "A3E_SEQ_TOO_SHORT",
                "/sequence",
                "'sequence' must be at least 2 characters, got $(length(upper)) (\"$raw\")",
            ),
        )
    end
    bad = findfirst(c -> !(c in 'A':'Z' || c == '*'), upper)
    if bad !== nothing
        push!(
            issues,
            A3Issue(
                "A3E_SEQ_CHARSET",
                "/sequence",
                "'sequence' contains '$(upper[bad])'; only A-Z (IUPAC amino acid codes) " *
                "and '*' (stop codon) are permitted",
            ),
        )
    end
    upper
end

function _stage2_annotations(
    annotations::AbstractDict,
    draft::Draft,
    issues::Vector{A3Issue},
)
    _unknown_fields(annotations, _FAMILIES, "/annotations", issues)

    for family in _NAMED_FAMILIES
        entries = get(annotations, family, nothing)
        entries === nothing && continue
        family_path = pointer("/annotations", family)
        if !(entries isa AbstractDict)
            push!(
                issues,
                A3Issue(
                    "A3E_FAMILY_NOT_OBJECT",
                    family_path,
                    "'annotations.$family' must be an object, got $(_render(entries))",
                ),
            )
            continue
        end
        parsed = _stage2_family(entries, family, family_path, issues)
        family == "site" ? (draft.site = parsed) :
        family == "region" ? (draft.region = parsed) :
        family == "ptm" ? (draft.ptm = parsed) : (draft.processing = parsed)
    end

    records = get(annotations, "variant", nothing)
    if records !== nothing
        if !(records isa AbstractVector)
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_LIST_NOT_ARRAY",
                    "/annotations/variant",
                    "'annotations.variant' must be an array, got $(_render(records))",
                ),
            )
        else
            draft.variant = _stage2_variants(records, issues)
        end
    end
end

# `family` selects the permitted index geometry: `site` takes positions,
# `region` takes ranges, `ptm` / `processing` take either, homogeneously.
function _stage2_family(
    entries::AbstractDict,
    family::String,
    family_path::String,
    issues::Vector{A3Issue},
)::OrderedDict{String,DraftEntry}
    out = OrderedDict{String,DraftEntry}()

    for (name, raw) in entries
        entry_path = pointer(family_path, name)

        if isempty(name)
            push!(
                issues,
                A3Issue(
                    "A3E_NAME_EMPTY",
                    entry_path,
                    "'annotations.$family' has an annotation named \"\"; names must not be empty",
                ),
            )
            continue
        end

        if !(raw isa AbstractDict)
            push!(
                issues,
                A3Issue(
                    "A3E_ENTRY_NOT_OBJECT",
                    entry_path,
                    "annotation \"$name\" must be an object with an 'index' member, " *
                    "got $(_render(raw))",
                ),
            )
            continue
        end

        _unknown_fields(raw, _ENTRY_KEYS, entry_path, issues)

        label = get(raw, "type", nothing)
        type_str = ""
        if label !== nothing
            if label isa AbstractString
                type_str = String(label)
            else
                push!(
                    issues,
                    A3Issue(
                        "A3E_ENTRY_TYPE_NOT_STRING",
                        pointer(entry_path, "type"),
                        "'type' must be a string, got $(_render(label))",
                    ),
                )
            end
        end

        index_path = pointer(entry_path, "index")
        if !haskey(raw, "index")
            push!(
                issues,
                A3Issue(
                    "A3E_INDEX_MISSING",
                    index_path,
                    "annotation \"$name\" is missing its required 'index' member",
                ),
            )
            continue
        end
        elements = raw["index"]
        if !(elements isa AbstractVector)
            push!(
                issues,
                A3Issue(
                    "A3E_INDEX_NOT_ARRAY",
                    index_path,
                    "'index' must be an array, got $(_render(elements))",
                ),
            )
            continue
        end

        parsed = _parse_index(elements, family, index_path, issues)
        parsed === nothing && continue
        geometry, positions, ranges = parsed
        out[String(name)] = DraftEntry(index_path, geometry, positions, ranges, type_str)
    end

    out
end

# Settle an index's geometry and extract its values. Returns `nothing` when the
# index could not be interpreted; issues have already been recorded.
function _parse_index(
    elements::AbstractVector,
    family::String,
    index_path::String,
    issues::Vector{A3Issue},
)
    flexible = family == "ptm" || family == "processing"

    # Classify every element first. Doing this up front is what makes the
    # mixed-geometry rule expressible: it is a property of the whole index.
    kinds = map(_classify, elements)

    bad = false
    for (i, kind) in enumerate(kinds)
        if kind === :other
            bad = true
            push!(
                issues,
                A3Issue(
                    "A3E_INDEX_ELEMENT_TYPE",
                    pointer(index_path, i - 1),
                    "index element must be $(_expected_shape(family)), " *
                    "got $(_render(elements[i]))",
                ),
            )
        end
    end
    bad && return nothing

    n_int = count(==(:integer), kinds)
    n_arr = count(==(:array), kinds)

    local geometry::Symbol
    if flexible
        if n_int > 0 && n_arr > 0
            push!(
                issues,
                A3Issue(
                    "A3E_INDEX_MIXED",
                    index_path,
                    "index mixes $n_int position(s) and $n_arr range(s); a ptm or processing " *
                    "index must be all positions or all ranges",
                ),
            )
            return nothing
        end
        geometry = n_arr > 0 ? :ranges : :positions
    else
        geometry = family == "region" ? :ranges : :positions
        # A well-formed element of the *other* geometry is still wrong for a
        # family whose geometry the schema fixes.
        want = geometry === :ranges ? :array : :integer
        wrong = false
        for (i, kind) in enumerate(kinds)
            if kind !== want
                wrong = true
                push!(
                    issues,
                    A3Issue(
                        "A3E_INDEX_ELEMENT_TYPE",
                        pointer(index_path, i - 1),
                        "index element must be $(_expected_shape(family)), " *
                        "got $(_render(elements[i]))",
                    ),
                )
            end
        end
        wrong && return nothing
    end

    if geometry === :positions
        return (geometry, Int[_as_integer(v)::Int for v in elements], Tuple{Int,Int}[])
    end

    ranges = Tuple{Int,Int}[]
    ok = true
    for (i, element) in enumerate(elements)
        pair = element::AbstractVector
        elem_path = pointer(index_path, i - 1)
        if length(pair) != 2
            push!(
                issues,
                A3Issue(
                    "A3E_RANGE_ARITY",
                    elem_path,
                    "a range must be a 2-element [start, end] array, got $(length(pair)) element(s)",
                ),
            )
            ok = false
            continue
        end
        s, e = _as_integer(pair[1]), _as_integer(pair[2])
        if s === nothing || e === nothing
            push!(
                issues,
                A3Issue(
                    "A3E_RANGE_ENDPOINT_NOT_INTEGER",
                    elem_path,
                    "range endpoints must be integers, got " *
                    "[$(_render(pair[1])), $(_render(pair[2]))]",
                ),
            )
            ok = false
            continue
        end
        push!(ranges, (s, e))
    end
    ok ? (geometry, Int[], ranges) : nothing
end

function _stage2_variants(
    records::AbstractVector,
    issues::Vector{A3Issue},
)::Vector{DraftVariant}
    out = DraftVariant[]

    for (i, raw) in enumerate(records)
        path = pointer("/annotations/variant", i - 1)

        if !(raw isa AbstractDict)
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_NOT_OBJECT",
                    path,
                    "a variant record must be an object, got $(_render(raw))",
                ),
            )
            continue
        end

        position_path = pointer(path, "position")
        if !haskey(raw, "position")
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_POSITION_MISSING",
                    position_path,
                    "a variant record is missing its required 'position' member",
                ),
            )
            continue
        end
        position = _as_integer(raw["position"])
        if position === nothing
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_POSITION_NOT_INTEGER",
                    position_path,
                    "'position' must be an integer, got $(_render(raw["position"]))",
                ),
            )
            continue
        end

        # A variant record is open: every other member is preserved verbatim, in
        # document order, and is not type-checked.
        extra = OrderedDict{String,Any}()
        for (k, v) in raw
            k == "position" && continue
            extra[String(k)] = v
        end
        push!(out, DraftVariant(path, position, extra))
    end

    out
end

function _stage2_metadata(metadata::AbstractDict, issues::Vector{A3Issue})::A3Metadata
    _unknown_fields(metadata, _METADATA_KEYS, "/metadata", issues)

    function get_str(key::String)::String
        v = get(metadata, key, nothing)
        v === nothing && return ""
        v isa AbstractString && return String(v)
        push!(
            issues,
            A3Issue(
                "A3E_METADATA_FIELD_NOT_STRING",
                pointer("/metadata", key),
                "'metadata.$key' must be a string, got $(_render(v))",
            ),
        )
        ""
    end

    A3Metadata(
        get_str("uniprot_id"),
        get_str("description"),
        get_str("reference"),
        get_str("organism"),
    )
end

# ─── Stage 3 — Intra-field ────────────────────────────────────────────────────

# Sorting happens here rather than in Stage 2 because an index cannot be sorted
# until its elements are known to be sortable, and it must be sorted before
# duplication and overlap can be checked. Every subscript reported by this stage
# and by Stage 4 therefore names the *sorted* array.
function _stage3_intra_field(draft::Draft, issues::Vector{A3Issue})
    for entry in _entries(draft)
        if entry.geometry === :positions
            sort!(entry.positions)
        else
            entry.ranges = sort_ranges(entry.ranges)
        end
    end

    for entry in _entries(draft)
        if entry.geometry === :positions
            for (i, p) in enumerate(entry.positions)
                p < 1 && push!(
                    issues,
                    A3Issue(
                        "A3E_POS_NOT_POSITIVE",
                        pointer(entry.index_path, i - 1),
                        "position $p must be >= 1; positions are 1-based",
                    ),
                )
            end
            # One issue per distinct repeated value, not per excess occurrence:
            # [7, 7, 7] is one mistake, not two.
            seen = Set{Int}()
            for i = 2:length(entry.positions)
                v = entry.positions[i]
                if v == entry.positions[i-1] && !(v in seen)
                    push!(seen, v)
                    push!(
                        issues,
                        A3Issue(
                            "A3E_POS_DUPLICATE",
                            entry.index_path,
                            "position $v appears more than once; duplicates are rejected, " *
                            "not merged",
                        ),
                    )
                end
            end
        else
            for (i, (s, e)) in enumerate(entry.ranges)
                elem_path = pointer(entry.index_path, i - 1)
                (s < 1 || e < 1) && push!(
                    issues,
                    A3Issue(
                        "A3E_RANGE_ENDPOINT_NOT_POSITIVE",
                        elem_path,
                        "range [$s, $e] has an endpoint below 1; positions are 1-based",
                    ),
                )
                s >= e && push!(
                    issues,
                    A3Issue(
                        "A3E_RANGE_ORDER",
                        elem_path,
                        "range [$s, $e] must satisfy start < end; a single residue belongs " *
                        "in a position-indexed family",
                    ),
                )
            end
            for i = 2:length(entry.ranges)
                prev, curr = entry.ranges[i-1], entry.ranges[i]
                # Sorted by start, so they overlap exactly when the second starts
                # at or before the first ends.
                curr[1] <= prev[2] && push!(
                    issues,
                    A3Issue(
                        "A3E_RANGE_OVERLAP",
                        entry.index_path,
                        "ranges [$(prev[1]), $(prev[2])] and [$(curr[1]), $(curr[2])] overlap; " *
                        "adjacent ranges are permitted but overlapping ones are not",
                    ),
                )
            end
        end
    end

    for v in draft.variant
        v.position < 1 && push!(
            issues,
            A3Issue(
                "A3E_VARIANT_POSITION_NOT_POSITIVE",
                pointer(v.path, "position"),
                "position $(v.position) must be >= 1; positions are 1-based",
            ),
        )
    end
end

# ─── Stage 4 — Contextual ─────────────────────────────────────────────────────

function _stage4_contextual(draft::Draft, issues::Vector{A3Issue})
    residues = collect(draft.sequence)
    n = length(residues)

    for entry in _entries(draft)
        if entry.geometry === :positions
            for (i, p) in enumerate(entry.positions)
                p > n && push!(
                    issues,
                    A3Issue(
                        "A3E_POS_OUT_OF_BOUNDS",
                        pointer(entry.index_path, i - 1),
                        "position $p is out of bounds for a sequence of length $n (must be 1-$n)",
                    ),
                )
            end
        else
            # Stage 3 established start < end, so only `end` can exceed `n`.
            for (i, (s, e)) in enumerate(entry.ranges)
                e > n && push!(
                    issues,
                    A3Issue(
                        "A3E_RANGE_OUT_OF_BOUNDS",
                        pointer(entry.index_path, i - 1),
                        "range [$s, $e] is out of bounds for a sequence of length $n " *
                        "(must be within 1-$n)",
                    ),
                )
            end
        end
    end

    for v in draft.variant
        if v.position > n
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_OUT_OF_BOUNDS",
                    pointer(v.path, "position"),
                    "position $(v.position) is out of bounds for a sequence of length $n " *
                    "(must be 1-$n)",
                ),
            )
            continue
        end
        # The residue rule is deliberately narrow: it fires only when `from` is
        # present *and* is a single character. `from` is an optional member of an
        # open record, so any other shape is not this rule's problem.
        frm = get(v.extra, "from", nothing)
        (frm isa AbstractString && length(frm) == 1) || continue
        actual = residues[v.position]
        # Case-insensitive because `sequence` is case-folded during normalization
        # and `from` is not.
        if uppercase(first(frm)) != actual
            push!(
                issues,
                A3Issue(
                    "A3E_VARIANT_RESIDUE_MISMATCH",
                    pointer(v.path, "from"),
                    "'from' is '$frm' but residue $(v.position) of the sequence is '$actual'; " *
                    "the variant list may be annotated against a different isoform",
                ),
            )
        end
    end
end

# ─── Construction ─────────────────────────────────────────────────────────────

# Every invariant this relies on was established by the four stages.
function _build(draft::Draft)::A3
    to_index(e::DraftEntry) =
        e.geometry === :positions ? A3Position(e.positions, e.type) :
        A3Range(e.ranges, e.type)

    site = OrderedDict{String,A3Position}(
        k => A3Position(e.positions, e.type) for (k, e) in draft.site
    )
    region = OrderedDict{String,A3Range}(
        k => A3Range(e.ranges, e.type) for (k, e) in draft.region
    )
    ptm = OrderedDict{String,A3Index}(k => to_index(e) for (k, e) in draft.ptm)
    processing =
        OrderedDict{String,A3Index}(k => to_index(e) for (k, e) in draft.processing)
    variant = VariantRecord[VariantRecord(v.position, v.extra) for v in draft.variant]

    A3(
        draft.sequence,
        A3Annotations(site, region, ptm, processing, variant),
        draft.metadata,
    )
end

# ─── A3 outer constructor ─────────────────────────────────────────────────────

"""
    A3(raw::AbstractDict) -> A3

Validate a raw parsed JSON object as an A3 document.

Throws [`A3ValidationError`](@ref) carrying every issue found in the earliest
failing validation stage. Use [`validate`](@ref) for a non-throwing result.
"""
function A3(raw::AbstractDict)
    a3, issues = validate(raw)
    a3 === nothing && throw(A3ValidationError(issues))
    _assert_variants_json_compatible(a3)
    a3
end

# Reject variant members that cannot be represented in JSON.
#
# A variant record is open, so its members are preserved verbatim — but a
# function or a struct cannot be serialized, producing a document that does not
# round-trip. This is a construction-path guard, not a validation rule: no such
# value can come out of `JSON.parse`, so it has no A3 issue code and cannot
# appear in the conformance corpus. It exists because `create_a3` lets a caller
# hand live Julia values straight in.
function _assert_variants_json_compatible(a3::A3)
    for (i, record) in enumerate(a3.annotations.variant)
        for (key, value) in record.extra
            is_json_compatible(value) || throw(
                ArgumentError(
                    "annotations.variant[$(i - 1)].$key is not JSON-compatible: " *
                    "$(typeof(value)). A variant record's members must be JSON values.",
                ),
            )
        end
    end
end

# ─── Helpers ──────────────────────────────────────────────────────────────────

# Classify an index element as :integer, :array, or :other.
function _classify(v)::Symbol
    v isa AbstractVector && return :array
    _as_integer(v) === nothing ? :other : :integer
end

# Interpret a JSON value as an integer.
#
# JSON has no integer type distinct from number, and JSON Schema's
# `type: "integer"` accepts any number with a zero fractional part, so `3.0` is
# the position 3 while `2.5` is not a position at all.
#
# `Bool <: Integer` in Julia, but a JSON boolean is not a number, so `true` must
# be excluded explicitly or it would be read as the position 1.
function _as_integer(v)::Union{Int,Nothing}
    v isa Bool && return nothing
    v isa Integer && return Int(v)
    (v isa AbstractFloat && isinteger(v)) && return Int(v)
    nothing
end

_expected_shape(family::String) =
    family == "site" ? "an integer position" :
    family == "region" ? "a [start, end] range" :
    "an integer position or a [start, end] range"

# Report one A3E_UNKNOWN_FIELD per property outside `allowed`.
function _unknown_fields(
    obj::AbstractDict,
    allowed::NTuple{N,String},
    base::String,
    issues::Vector{A3Issue},
) where {N}
    for key in keys(obj)
        if !(key in allowed)
            push!(
                issues,
                A3Issue(
                    "A3E_UNKNOWN_FIELD",
                    pointer(base, key),
                    "'$key' is not a permitted member here; expected one of: " *
                    join(allowed, ", "),
                ),
            )
        end
    end
end

# Render a JSON value compactly for a human message, truncating long ones.
function _render(v)
    text = v === nothing ? "null" : v isa AbstractString ? "\"$v\"" : string(v)
    length(text) > 40 ? string(first(text, 39), "…") : text
end
