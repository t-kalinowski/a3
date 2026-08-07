# Normalization — the three transformations A3 applies to a valid document.
#
# Every function here succeeds for every input, because deciding whether an
# input is *legal* is validate.jl's job. The complete list is:
#
#   1. the sequence is case-folded to uppercase
#   2. positions are sorted ascending
#   3. ranges are sorted by start, then by end
#
# Nothing else changes. In particular, author-supplied key order — annotation
# names, and a variant record's extra members — is preserved exactly.

"""
    sort_ranges(v) -> Vector{Tuple{Int,Int}}

Sort ranges by start, then by end. Overlapping and inverted ranges are
preserved; both are rejected by validation after this sort has run.
"""
sort_ranges(v::Vector{Tuple{Int,Int}}) = sort(v; by = r -> (r[1], r[2]))

"""
    is_json_compatible(v) -> Bool

Whether a value can be represented in JSON.

Used on the construction path only: no incompatible value can come out of
`JSON.parse`, so this is not a validation rule and has no issue code.
"""
function is_json_compatible(v)::Bool
    v === nothing && return true
    v isa Bool && return true
    v isa Number && return true
    v isa AbstractString && return true
    v isa AbstractVector && return all(is_json_compatible, v)
    if v isa AbstractDict
        return all(k isa AbstractString && is_json_compatible(val) for (k, val) in v)
    end
    return false
end
