# ─── Serialization ────────────────────────────────────────────────────────────

# Output is in canonical form: schema-defined members in schema order,
# author-supplied keys in document order. OrderedDict is what makes the second
# half of that true.
_to_dict(e::A3Position) = OrderedDict{String,Any}("index" => e.index, "type" => e.type)
_to_dict(e::A3Range) =
    OrderedDict{String,Any}("index" => [collect(r) for r in e.index], "type" => e.type)

function _to_dict(v::VariantRecord)
    d = OrderedDict{String,Any}("position" => v.position)
    merge!(d, v.extra)
    d
end

function to_dict(a3::A3)::OrderedDict{String,Any}
    fam(d) = OrderedDict{String,Any}(k => _to_dict(v) for (k, v) in d)
    OrderedDict{String,Any}(
        "\$schema" => _A3_SCHEMA_URI,
        "a3_version" => _A3_VERSION,
        "sequence" => a3.sequence,
        "annotations" => OrderedDict{String,Any}(
            "site" => fam(a3.annotations.site),
            "region" => fam(a3.annotations.region),
            "ptm" => fam(a3.annotations.ptm),
            "processing" => fam(a3.annotations.processing),
            "variant" => [_to_dict(v) for v in a3.annotations.variant],
        ),
        "metadata" => OrderedDict{String,Any}(
            "uniprot_id" => a3.metadata.uniprot_id,
            "description" => a3.metadata.description,
            "reference" => a3.metadata.reference,
            "organism" => a3.metadata.organism,
        ),
    )
end

# ─── JSON ─────────────────────────────────────────────────────────────────────

"""
    a3_from_json(text) -> A3

Parse an A3 object from a JSON string.
"""
function a3_from_json(text::AbstractString)::A3
    # `dicttype = OrderedDict` keeps author-supplied key order, which is part of
    # the document: annotation names and a variant record's extra members must
    # round-trip in the order they were written.
    raw = try
        JSON.parse(text; dicttype = OrderedDict)
    catch e
        throw(A3ParseError("JSON parse error: $e"))
    end
    # A non-object root is a validation issue (A3E_DOC_NOT_OBJECT), not a parse
    # failure: the input was perfectly good JSON.
    if !(raw isa AbstractDict)
        throw(A3ValidationError(last(validate(raw))))
    end
    A3(raw)
end

"""
    a3_to_json(a3; indent=nothing) -> String

Serialize an A3 object to a JSON string. Pass `indent` for pretty-printing.
"""
function a3_to_json(a3::A3; indent::Union{Int,Nothing} = nothing)::String
    d = to_dict(a3)
    indent === nothing ? JSON.json(d) : JSON.json(d, indent)
end

# ─── File I/O ─────────────────────────────────────────────────────────────────

"""
    read_a3json(path) -> A3

Read an A3 object from a JSON file on disk.
"""
function read_a3json(path::AbstractString)::A3
    text = try
        read(path, String)
    catch e
        throw(A3ParseError("failed to read '$path': $e"))
    end
    a3_from_json(text)
end

"""
    write_a3json(a3, path; indent=2)

Write an A3 object to a JSON file on disk.
"""
function write_a3json(a3::A3, path::AbstractString; indent::Int = 2)
    text = a3_to_json(a3; indent = indent)
    try
        Base.write(path, text)
    catch e
        throw(A3ParseError("failed to write '$path': $e"))
    end
    nothing
end
