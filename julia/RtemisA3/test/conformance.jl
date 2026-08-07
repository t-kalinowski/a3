# Conformance runner: checks this implementation against the shared corpus.
#
# The corpus lives at spec/conformance/ in the repository root and is read by
# all five A3 implementations. It is never vendored into a language directory —
# one corpus, five readers, or it drifts.
#
# See spec/conformance/README.md for the assertion semantics implemented here.

# test/ -> RtemisA3/ -> julia/ -> <repo>
const CORPUS = normpath(joinpath(@__DIR__, "..", "..", "..", "spec", "conformance"))

"""
Count each `(code, path)` pair.

Issues are compared as a **multiset**: a rule can legitimately fire more than
once at the same path (two repeated positions in one index produce two
`A3E_POS_DUPLICATE` issues), and an implementation that reports one of them has
not conformed. Order is not compared.
"""
function tally(pairs)
    counts = Dict{Tuple{String,String},Int}()
    for p in pairs
        counts[p] = get(counts, p, 0) + 1
    end
    counts
end

"""Describe the difference between two issue multisets, or `""` if equal."""
function issue_diff(expected::Dict, actual::Dict)
    lines = String[]
    for (pair, want) in sort(collect(expected); by = first)
        got = get(actual, pair, 0)
        got < want &&
            push!(lines, "  missing:    $(pair[1])  $(pair[2])  (expected $want, got $got)")
    end
    for (pair, got) in sort(collect(actual); by = first)
        want = get(expected, pair, 0)
        got > want &&
            push!(lines, "  unexpected: $(pair[1])  $(pair[2])  (expected $want, got $got)")
    end
    join(lines, "\n")
end

"""
Project a parsed JSON value so that `==` treats object key order as significant.

`OrderedDict` preserves member order, but `==` on two OrderedDicts ignores it.
Comparing a vector of pairs instead makes order part of the comparison, which is
what turns these cases into a normalization test rather than a parsing test.
"""
ordered(v::AbstractDict) = [(string(k), ordered(val)) for (k, val) in v]
ordered(v::AbstractVector) = [ordered(x) for x in v]
ordered(v) = v

if !isfile(joinpath(CORPUS, "manifest.json"))
    @warn "Conformance corpus not found, skipping" CORPUS
else
    manifest =
        JSON.parse(read(joinpath(CORPUS, "manifest.json"), String); dicttype = OrderedDict)

    @testset "conformance corpus" begin
        for case in manifest["cases"]
            file = case["file"]
            description = case["description"]
            @testset "$file" begin
                text = read(joinpath(CORPUS, file), String)
                raw = JSON.parse(text; dicttype = OrderedDict)
                a3, issues = RtemisA3.validate(raw)

                if case["valid"]
                    @test a3 !== nothing
                    a3 === nothing && (@info description issues; continue)

                    haskey(case, "normalized") || continue
                    expected = JSON.parse(
                        read(joinpath(CORPUS, case["normalized"]), String);
                        dicttype = OrderedDict,
                    )
                    produced = JSON.parse(a3_to_json(a3); dicttype = OrderedDict)
                    @test ordered(produced) == ordered(expected)
                else
                    want = tally([(i["code"], i["path"]) for i in case["issues"]])
                    got = tally([(i.code, i.path) for i in issues])
                    diff = issue_diff(want, got)
                    isempty(diff) || @info "$file — $description\n$diff"
                    @test isempty(diff)

                    # Every issue in one result comes from a single stage, and
                    # the manifest records which. A mismatch means the staging is
                    # wrong even though the issue set happens to agree.
                    if haskey(case, "stage") && !isempty(issues)
                        @test stage(issues[1]) == case["stage"]
                    end
                end
            end
        end
    end

    @testset "registry coverage" begin
        # Without this, the registry and the corpus drift within two months: a
        # code is added, no case is written, and nothing notices.
        covered = Set{String}()
        for case in manifest["cases"]
            case["valid"] && continue
            for issue in case["issues"]
                push!(covered, issue["code"])
            end
        end
        known = Set(A3_ISSUE_CODES)

        missing_cases = sort(collect(setdiff(known, covered)))
        isempty(missing_cases) || @info "codes with no corpus case" missing_cases
        @test isempty(missing_cases)

        unknown_codes = sort(collect(setdiff(covered, known)))
        isempty(unknown_codes) || @info "corpus expects unregistered codes" unknown_codes
        @test isempty(unknown_codes)

        # And every code must declare a stage in 1-4.
        @test all(1 <= A3_ISSUE_STAGES[c] <= 4 for c in A3_ISSUE_CODES)
    end
end
