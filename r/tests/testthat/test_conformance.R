# Conformance runner: checks this implementation against the shared corpus.
#
# The corpus lives at spec/conformance/ in the repository root and is read by
# all five A3 implementations. It is never vendored into the package — one
# corpus, five readers, or it drifts. That means it is absent from an installed
# package, so these tests skip gracefully off-repo and run for real in CI.
#
# See spec/conformance/README.md for the assertion semantics implemented here.

corpus_dir <- function() {
  # testthat runs from tests/testthat/, but devtools::test() and R CMD check
  # each use a different working directory, so try the plausible roots.
  candidates <- c(
    file.path("..", "..", "..", "spec", "conformance"),
    file.path("..", "..", "..", "..", "spec", "conformance"),
    file.path(testthat::test_path(), "..", "..", "..", "spec", "conformance")
  )
  for (path in candidates) {
    if (file.exists(file.path(path, "manifest.json"))) {
      return(normalizePath(path))
    }
  }
  NA_character_
}

CORPUS <- corpus_dir()

skip_without_corpus <- function() {
  testthat::skip_if(
    is.na(CORPUS),
    "conformance corpus not found (running outside the repository)"
  )
}

read_corpus_json <- function(rel) {
  jsonlite::fromJSON(
    file.path(CORPUS, rel),
    simplifyVector = FALSE,
    simplifyDataFrame = FALSE,
    simplifyMatrix = FALSE
  )
}

# Count each code/path pair. Issues are compared as a multiset: a rule can
# legitimately fire more than once at the same path (two repeated positions in
# one index produce two A3E_POS_DUPLICATE issues), and an implementation that
# reports one of them has not conformed. Order is not compared.
tally <- function(pairs) {
  if (length(pairs) == 0L) {
    return(table(character(0)))
  }
  table(vapply(pairs, function(p) paste(p[[1L]], p[[2L]]), character(1L)))
}

describe_diff <- function(expected, actual) {
  keys <- union(names(expected), names(actual))
  lines <- character(0)
  for (key in sort(keys)) {
    want <- if (key %in% names(expected)) expected[[key]] else 0L
    got <- if (key %in% names(actual)) actual[[key]] else 0L
    if (got < want) {
      lines <- c(
        lines,
        paste0("  missing:    ", key, " (expected ", want, ", got ", got, ")")
      )
    } else if (got > want) {
      lines <- c(
        lines,
        paste0("  unexpected: ", key, " (expected ", want, ", got ", got, ")")
      )
    }
  }
  paste(lines, collapse = "\n")
}

# Project a parsed JSON value so that identical() treats object member order as
# significant. With simplification off, fromJSON() gives named lists whose order
# is the document's, and identical() already compares names in order — but the
# projection also strips attribute differences that would otherwise mask a real
# match. This is what turns these cases into a normalization test rather than
# merely a parsing test.
ordered_repr <- function(x) {
  if (is.list(x)) {
    nms <- names(x)
    body <- lapply(unname(x), ordered_repr)
    if (is.null(nms)) {
      list(kind = "array", values = body)
    } else {
      list(kind = "object", names = nms, values = body)
    }
  } else if (is.numeric(x) && length(x) == 1L) {
    # An integral 3 and 3.0 are the same JSON number.
    as.numeric(x)
  } else {
    x
  }
}


test_that("every corpus case behaves as the manifest says", {
  skip_without_corpus()
  manifest <- read_corpus_json("manifest.json")
  failures <- character(0)

  for (case in manifest[["cases"]]) {
    file <- case[["file"]]
    description <- case[["description"]]
    result <- validate_a3_raw(read_corpus_json(file))
    issues <- result[["issues"]]

    if (isTRUE(case[["valid"]])) {
      if (length(issues) > 0L) {
        failures <- c(
          failures,
          paste0(
            "FAIL ",
            file,
            "\n  ",
            description,
            "\n  expected valid, got:\n",
            paste(
              vapply(
                issues,
                function(i) {
                  paste0(
                    "    [",
                    i[["code"]],
                    "] ",
                    i[["path"]],
                    ": ",
                    i[["message"]]
                  )
                },
                character(1L)
              ),
              collapse = "\n"
            )
          )
        )
        next
      }
      if (is.null(case[["normalized"]])) {
        next
      }
      produced <- jsonlite::fromJSON(
        to_json(result[["a3"]], pretty = FALSE),
        simplifyVector = FALSE,
        simplifyDataFrame = FALSE,
        simplifyMatrix = FALSE
      )
      expected <- read_corpus_json(case[["normalized"]])
      if (!identical(ordered_repr(produced), ordered_repr(expected))) {
        failures <- c(
          failures,
          paste0(
            "FAIL ",
            file,
            "\n  ",
            description,
            "\n  round-trip differs from ",
            case[["normalized"]]
          )
        )
      }
      next
    }

    # Invalid case.
    expected <- tally(lapply(case[["issues"]], function(i) {
      list(i[["code"]], i[["path"]])
    }))
    actual <- tally(lapply(issues, function(i) list(i[["code"]], i[["path"]])))
    diff <- describe_diff(expected, actual)
    if (nzchar(diff)) {
      failures <- c(
        failures,
        paste0("FAIL ", file, "\n  ", description, "\n", diff)
      )
      next
    }

    # Every issue in one result comes from a single stage, and the manifest
    # records which. A mismatch means the staging is wrong even though the issue
    # set happens to agree.
    if (!is.null(case[["stage"]]) && length(issues) > 0L) {
      got_stage <- .A3_ISSUE_STAGES[[issues[[1L]][["code"]]]]
      if (got_stage != case[["stage"]]) {
        failures <- c(
          failures,
          paste0(
            "FAIL ",
            file,
            "\n  ",
            description,
            "\n  expected stage ",
            case[["stage"]],
            ", got ",
            got_stage
          )
        )
      }
    }
  }

  expect_equal(paste(failures, collapse = "\n\n"), "")
})


test_that("every code in the registry has a corpus case", {
  # Without this, the registry and the corpus drift within two months: a code is
  # added, no case is written, and nothing notices.
  skip_without_corpus()
  manifest <- read_corpus_json("manifest.json")

  covered <- unique(unlist(lapply(manifest[["cases"]], function(case) {
    if (isTRUE(case[["valid"]])) {
      return(character(0))
    }
    vapply(case[["issues"]], function(i) i[["code"]], character(1L))
  })))
  known <- names(.A3_ISSUE_STAGES)

  expect_equal(sort(setdiff(known, covered)), character(0))
  expect_equal(sort(setdiff(covered, known)), character(0))
})


test_that("every code declares a stage in 1-4", {
  expect_true(all(.A3_ISSUE_STAGES >= 1L & .A3_ISSUE_STAGES <= 4L))
  expect_equal(length(.A3_ISSUE_STAGES), 37L)
})


test_that("JSON Pointer tokens escape ~ before /", {
  # "~1" must survive as "~01", not be re-read as an escaped slash.
  expect_equal(.a3_escape_token("a/b"), "a~1b")
  expect_equal(.a3_escape_token("a~b"), "a~0b")
  expect_equal(.a3_escape_token("a~1b"), "a~01b")
  expect_equal(.a3_escape_token("a.b"), "a.b")
  expect_equal(
    .a3_pointer("/annotations/site", "a/b"),
    "/annotations/site/a~1b"
  )
  expect_equal(.a3_pointer("/annotations/site", ""), "/annotations/site/")
  expect_equal(
    .a3_pointer("/annotations/variant", 3L),
    "/annotations/variant/3"
  )
})
