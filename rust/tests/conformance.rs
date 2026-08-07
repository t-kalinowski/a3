//! Conformance runner: checks this implementation against the shared corpus.
//!
//! The corpus lives at `spec/conformance/` in the repository root and is read
//! by all five A3 implementations. It is never vendored into a language
//! directory — one corpus, five readers, or it drifts.
//!
//! See `spec/conformance/README.md` for the assertion semantics this file
//! implements.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rtemis_a3::{A3Error, A3IssueCode, a3_from_json, a3_to_json};
use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Manifest {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    file: String,
    description: String,
    valid: bool,
    #[serde(default)]
    stage: Option<u8>,
    #[serde(default)]
    issues: Vec<ExpectedIssue>,
    #[serde(default)]
    normalized: Option<String>,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct ExpectedIssue {
    code: String,
    path: String,
}

/// Locate `spec/conformance/` relative to this crate.
///
/// `CARGO_MANIFEST_DIR` is `<repo>/rust`, so the corpus is one level up.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rust crate always has a parent directory")
        .join("spec/conformance")
}

fn load_manifest(dir: &Path) -> Manifest {
    let path = dir.join("manifest.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("could not parse {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// Count each `{code, path}` pair.
///
/// Issues are compared as a **multiset**: a rule can legitimately fire more
/// than once at the same path (two repeated positions in one index produce two
/// `A3E_POS_DUPLICATE` issues), and an implementation that reports one of them
/// has not conformed. Order is not compared.
fn tally<I: IntoIterator<Item = ExpectedIssue>>(issues: I) -> BTreeMap<ExpectedIssue, usize> {
    let mut counts = BTreeMap::new();
    for issue in issues {
        *counts.entry(issue).or_insert(0) += 1;
    }
    counts
}

/// Describe the difference between two issue multisets, or `None` if equal.
fn diff(
    expected: &BTreeMap<ExpectedIssue, usize>,
    actual: &BTreeMap<ExpectedIssue, usize>,
) -> Option<String> {
    if expected == actual {
        return None;
    }
    let mut lines = Vec::new();
    for (issue, want) in expected {
        let got = actual.get(issue).copied().unwrap_or(0);
        if got < *want {
            lines.push(format!(
                "  missing:    {:<34} {}  (expected {want}, got {got})",
                issue.code, issue.path
            ));
        }
    }
    for (issue, got) in actual {
        let want = expected.get(issue).copied().unwrap_or(0);
        if *got > want {
            lines.push(format!(
                "  unexpected: {:<34} {}  (expected {want}, got {got})",
                issue.code, issue.path
            ));
        }
    }
    Some(lines.join("\n"))
}

/// Compare two JSON values, treating object member order as significant.
///
/// `serde_json`'s `preserve_order` feature makes `Map` an `IndexMap`, so `==`
/// already ignores order — hence the explicit key-sequence comparison here.
fn deep_eq_ordered(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.keys().eq(y.keys())
                && x.values()
                    .zip(y.values())
                    .all(|(u, v)| deep_eq_ordered(u, v))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(u, v)| deep_eq_ordered(u, v))
        }
        _ => a == b,
    }
}

// ---------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------

#[test]
fn conformance_corpus() {
    let dir = corpus_dir();
    let manifest = load_manifest(&dir);
    let mut failures: Vec<String> = Vec::new();

    for case in &manifest.cases {
        let path = dir.join(&case.file);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("could not read case {}: {e}", path.display()));

        let result = a3_from_json(&text);

        if case.valid {
            let a3 = match result {
                Ok(a3) => a3,
                Err(e) => {
                    failures.push(format!(
                        "FAIL {}\n  {}\n  expected valid, got:\n{e}",
                        case.file, case.description
                    ));
                    continue;
                }
            };

            let Some(normalized) = &case.normalized else {
                continue;
            };
            let expected_text = fs::read_to_string(dir.join(normalized))
                .unwrap_or_else(|e| panic!("could not read {normalized}: {e}"));
            let expected: Value = serde_json::from_str(&expected_text)
                .unwrap_or_else(|e| panic!("could not parse {normalized}: {e}"));
            let produced: Value = serde_json::from_str(&a3_to_json(&a3, None).unwrap())
                .expect("our own output is always valid JSON");

            if !deep_eq_ordered(&produced, &expected) {
                failures.push(format!(
                    "FAIL {}\n  {}\n  round-trip differs from {normalized}\n  \
                     expected: {}\n  produced: {}",
                    case.file,
                    case.description,
                    serde_json::to_string(&expected).unwrap(),
                    serde_json::to_string(&produced).unwrap(),
                ));
            }
            continue;
        }

        // Invalid case.
        let Err(A3Error::Validate(issues)) = result else {
            failures.push(format!(
                "FAIL {}\n  {}\n  expected {} validation issue(s), got {}",
                case.file,
                case.description,
                case.issues.len(),
                match result {
                    Ok(_) => "a valid document".to_string(),
                    Err(e) => format!("a non-validation error: {e}"),
                }
            ));
            continue;
        };

        let actual = tally(issues.iter().map(|i| ExpectedIssue {
            code: i.code.code().to_string(),
            path: i.path.clone(),
        }));
        let expected = tally(case.issues.iter().cloned());

        if let Some(d) = diff(&expected, &actual) {
            failures.push(format!("FAIL {}\n  {}\n{d}", case.file, case.description));
            continue;
        }

        // Every issue in one error comes from a single stage, and the manifest
        // records which. A mismatch means the staging is wrong even though the
        // issue set happens to agree.
        if let Some(want_stage) = case.stage
            && let Some(first) = issues.first()
            && first.stage() != want_stage
        {
            failures.push(format!(
                "FAIL {}\n  {}\n  expected stage {want_stage}, got {}",
                case.file,
                case.description,
                first.stage()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "\n\n{} of {} conformance case(s) failed:\n\n{}\n",
        failures.len(),
        manifest.cases.len(),
        failures.join("\n\n")
    );
}

/// Every code in the registry must be exercised by at least one corpus case.
///
/// Without this, the registry and the corpus drift within two months: a code is
/// added, no case is written, and nothing notices.
#[test]
fn every_code_has_a_corpus_case() {
    let dir = corpus_dir();
    let manifest = load_manifest(&dir);

    let covered: std::collections::HashSet<&str> = manifest
        .cases
        .iter()
        .flat_map(|c| c.issues.iter())
        .map(|i| i.code.as_str())
        .collect();

    let missing: Vec<&str> = A3IssueCode::ALL
        .iter()
        .map(|c| c.code())
        .filter(|c| !covered.contains(c))
        .collect();

    assert!(
        missing.is_empty(),
        "codes with no corpus case: {}",
        missing.join(", ")
    );

    // And the reverse: a case must not expect a code this crate does not know.
    let known: std::collections::HashSet<&str> =
        A3IssueCode::ALL.iter().map(|c| c.code()).collect();
    let unknown: Vec<&&str> = covered.iter().filter(|c| !known.contains(*c)).collect();
    assert!(
        unknown.is_empty(),
        "corpus expects codes not in the registry: {unknown:?}"
    );
}
