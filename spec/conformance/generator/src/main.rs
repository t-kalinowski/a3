//! Generate the A3 conformance corpus: every case file plus `manifest.json`.
//!
//! This is the single place to edit when adding, removing, or changing a
//! conformance case. Hand-editing the corpus is a mistake: a case file and its
//! manifest entry must agree, and 87 files kept in sync by hand will not stay
//! in sync.
//!
//! ```console
//! $ just corpus
//! ```
//!
//! It rewrites the corpus from scratch and is idempotent; CI fails if running
//! it produces a diff.
//!
//! # What comes from where
//!
//! Two kinds of expectation live in this file, and the difference matters:
//!
//! - **Expected issues are hand-declared.** They are the specification. They
//!   never come from an implementation, or the corpus would be a snapshot of
//!   one implementation's behavior rather than something every implementation
//!   — including that one — can be found wrong against.
//! - **`normalized` companions are *produced* by the Rust implementation.**
//!   Something has to emit them, and re-implementing the three normalization
//!   rules here would create a sixth normalizer whose only qualification is
//!   that it lives next to the corpus. Rust is used because the generator can
//!   link against it directly, not because it outranks the other four.
//!
//! Producing is not defining. The canonical form is specified in prose in
//! `spec/conformance/README.md`, and every generated companion is checked
//! against those rules, stated here independently, before it is written — see
//! [`check_canonical`]. A normalization bug in the producer fails generation
//! rather than being silently baked into the corpus as if it were the spec.

use std::fs;
use std::path::{Path, PathBuf};

use rtemis_a3::{A3IssueCode as C, a3_to_json, validate};
use serde_json::{Map, Value, json};

const URI: &str = "https://schema.rtemis.org/a3/v1/schema.json";
const VER: &str = "1.0.0";

/// M1 A2 E3 P4 R5 Q6 E7 F8 E9 V10 M11 E12 D13 H14 A15 G16 T17 Y18 G19 L20 G21 D22 R23 K24
///
/// Every case shares this sequence so that bounds arithmetic is checkable by
/// eye. Residue 5 is `R`, residue 18 is `Y`.
const SEQ: &str = "MAEPRQEFEVMEDHAGTYGLGDRK";

/// Length of [`SEQ`], for cases that sit on the boundary.
const N: u64 = 24;

const TOP_LEVEL_ORDER: &[&str] = &[
    "$schema",
    "a3_version",
    "sequence",
    "annotations",
    "metadata",
];
const FAMILY_ORDER: &[&str] = &["site", "region", "ptm", "processing", "variant"];
const NAMED_FAMILIES: &[&str] = &["site", "region", "ptm", "processing"];
const METADATA_ORDER: &[&str] = &["uniprot_id", "description", "reference", "organism"];

// ---------------------------------------------------------------------------
// Document builders
// ---------------------------------------------------------------------------

/// A document with the envelope filled in, in schema order.
///
/// Cases state only what they are testing; `sequence` defaults to [`SEQ`] and
/// both optional containers are absent unless given.
fn doc(members: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    m.insert("$schema".into(), json!(URI));
    m.insert("a3_version".into(), json!(VER));
    m.insert(
        "sequence".into(),
        members
            .iter()
            .find(|(k, _)| *k == "sequence")
            .map_or_else(|| json!(SEQ), |(_, v)| v.clone()),
    );
    for (k, v) in members.iter().filter(|(k, _)| *k != "sequence") {
        m.insert((*k).into(), v.clone());
    }
    Value::Object(m)
}

/// A document written member by member, with nothing filled in.
///
/// Used by the cases that are *about* the envelope, where "this member is
/// absent" is the whole point and a helpful default would erase the test.
fn raw(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

/// Shorthand for a document whose only content is an `annotations` object.
fn annotated(annotations: Value) -> Value {
    doc(&[("annotations", annotations)])
}

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

struct Corpus {
    dir: PathBuf,
    cases: Vec<Value>,
}

impl Corpus {
    fn new(dir: PathBuf) -> Self {
        Corpus {
            dir,
            cases: Vec::new(),
        }
    }

    /// A document that must validate.
    fn valid(&mut self, name: &str, description: &str, document: Value) {
        let file = format!("valid/{name}.a3.json");
        self.write(&file, &document);
        self.cases.push(json!({
            "file": file,
            "description": description,
            "valid": true,
        }));
    }

    /// A document that must validate *and* re-serialize to a pinned canonical form.
    fn valid_roundtrip(&mut self, name: &str, description: &str, document: Value) {
        let file = format!("valid/{name}.a3.json");
        let companion = format!("valid/{name}.normalized.json");
        self.write(&file, &document);
        self.write(&companion, &normalized(&document, &file));
        self.cases.push(json!({
            "file": file,
            "description": description,
            "valid": true,
            "normalized": companion,
        }));
    }

    /// A document that must not validate, with the exact issues expected.
    ///
    /// The issue list is never checked against an implementation here — that is
    /// what `cargo test --test conformance` is for, and it has to be able to
    /// fail.
    fn invalid(
        &mut self,
        name: &str,
        description: &str,
        document: Value,
        stage: u8,
        issues: &[(C, &str)],
    ) {
        let file = format!("invalid/{name}.a3.json");
        self.write(&file, &document);
        self.cases.push(json!({
            "file": file,
            "description": description,
            "valid": false,
            "stage": stage,
            "issues": issues
                .iter()
                .map(|(code, path)| json!({"code": code.code(), "path": path}))
                .collect::<Vec<_>>(),
        }));
    }

    fn write(&self, rel: &str, value: &Value) {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("mkdir {}: {e}", parent.display()));
        }
        let mut text = serde_json::to_string_pretty(value).expect("case values are serializable");
        text.push('\n');
        fs::write(&path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    fn finish(self) {
        let n_valid = self.cases.iter().filter(|c| c["valid"] == true).count();
        let n_roundtrip = self
            .cases
            .iter()
            .filter(|c| c.get("normalized").is_some())
            .count();
        let n_invalid = self.cases.len() - n_valid;

        let mut codes: Vec<&str> = self
            .cases
            .iter()
            .filter_map(|c| c["issues"].as_array())
            .flatten()
            .filter_map(|i| i["code"].as_str())
            .collect();
        codes.sort_unstable();
        codes.dedup();

        let total = self.cases.len();
        self.write(
            "manifest.json",
            &json!({
                "$comment": "Generated by spec/conformance/generator — edit that, not this.",
                "schema_uri": URI,
                "a3_version": VER,
                "cases": self.cases,
            }),
        );

        println!(
            "{total} cases: {n_valid} valid ({n_roundtrip} with round-trip), {n_invalid} invalid"
        );
        println!("{} distinct codes exercised", codes.len());

        // Every registry code must have a case. The per-language runners assert
        // this too, but failing here means you learn it while writing the case
        // rather than three commits later.
        let missing: Vec<&str> = C::ALL
            .iter()
            .map(|c| c.code())
            .filter(|c| !codes.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "codes with no corpus case: {}",
            missing.join(", ")
        );
    }
}

// ---------------------------------------------------------------------------
// Normalization oracle
// ---------------------------------------------------------------------------

/// Produce the canonical form of a valid document, using the Rust
/// implementation, and check it against invariants stated here independently.
fn normalized(document: &Value, file: &str) -> Value {
    let a3 = validate(document).unwrap_or_else(|issues| {
        panic!(
            "{file} is registered as valid but the Rust implementation rejected it:\n{}",
            issues
                .iter()
                .map(|i| format!("  {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    let text = a3_to_json(&a3, None).expect("a validated A3 always serializes");
    let value: Value = serde_json::from_str(&text).expect("our own output is valid JSON");
    check_canonical(document, &value, file);
    value
}

/// Assert the canonical form's invariants without consulting the implementation
/// that produced it.
///
/// This is what keeps the producer honest, and what makes it a producer rather
/// than an authority. Normalization is exactly three rules — case-fold the
/// sequence, sort positions, sort ranges — plus a fixed member order and
/// materialized defaults. All of that is checkable from the outside, so a bug
/// in the producer fails generation loudly instead of being written into the
/// corpus as if it were the specification.
fn check_canonical(source: &Value, out: &Value, file: &str) {
    let root = out
        .as_object()
        .unwrap_or_else(|| fail(file, "is not an object"));
    if !keys_are(root, TOP_LEVEL_ORDER) {
        fail(file, "does not list its top-level members in schema order");
    }

    // -- Rule 1: the sequence is case-folded, and nothing else about it changed.
    let sequence = root["sequence"]
        .as_str()
        .unwrap_or_else(|| fail(file, "has a non-string sequence"));
    let source_sequence = source["sequence"].as_str().unwrap_or("");
    if sequence != source_sequence.to_ascii_uppercase() {
        fail(
            file,
            "did something to the sequence other than case-fold it",
        );
    }
    if !sequence.chars().all(|c| c.is_ascii_uppercase() || c == '*') {
        fail(file, "has a sequence outside [A-Z*]");
    }
    let n = sequence.chars().count() as u64;

    // -- Defaults are materialized, in schema order.
    let annotations = root["annotations"]
        .as_object()
        .unwrap_or_else(|| fail(file, "has no annotations object"));
    if !keys_are(annotations, FAMILY_ORDER) {
        fail(
            file,
            "does not list all five annotation families in schema order",
        );
    }
    let metadata = root["metadata"]
        .as_object()
        .unwrap_or_else(|| fail(file, "has no metadata object"));
    if !keys_are(metadata, METADATA_ORDER) {
        fail(
            file,
            "does not list all four metadata members in schema order",
        );
    }

    // -- Author-supplied key order is preserved.
    let source_annotations = source.get("annotations").and_then(Value::as_object);
    for family in NAMED_FAMILIES {
        let entries = annotations[*family]
            .as_object()
            .unwrap_or_else(|| fail(file, "has a non-object annotation family"));

        let source_names: Vec<&String> = source_annotations
            .and_then(|a| a.get(*family))
            .and_then(Value::as_object)
            .map(|f| f.keys().collect())
            .unwrap_or_default();
        if entries.keys().collect::<Vec<_>>() != source_names {
            fail(file, "reordered or lost annotation names");
        }

        for entry in entries.values() {
            let entry = entry
                .as_object()
                .unwrap_or_else(|| fail(file, "has a non-object entry"));
            if !keys_are(entry, &["index", "type"]) {
                fail(file, "does not write an entry as index then type");
            }
            check_index(&entry["index"], n, file);
        }
    }

    // -- Rule 4 of the record format: a variant's members round-trip in order,
    //    with `position` first.
    let variants = annotations["variant"]
        .as_array()
        .unwrap_or_else(|| fail(file, "has a non-array variant list"));
    let source_variants = source_annotations
        .and_then(|a| a.get("variant"))
        .and_then(Value::as_array);
    for (i, variant) in variants.iter().enumerate() {
        let variant = variant
            .as_object()
            .unwrap_or_else(|| fail(file, "has a non-object variant"));
        let mut expected: Vec<&str> = vec!["position"];
        if let Some(src) = source_variants
            .and_then(|v| v.get(i))
            .and_then(Value::as_object)
        {
            expected.extend(src.keys().map(String::as_str).filter(|k| *k != "position"));
        }
        if variant.keys().collect::<Vec<_>>() != expected {
            fail(file, "reordered or lost a variant record's members");
        }
    }
}

/// Rules 2 and 3: positions ascending, ranges ascending by start then end, and
/// every value within the sequence.
fn check_index(index: &Value, n: u64, file: &str) {
    let elements = index
        .as_array()
        .unwrap_or_else(|| fail(file, "has a non-array index"));
    let mut previous: Option<(u64, u64)> = None;
    for element in elements {
        let (start, end) = match element {
            Value::Number(_) => {
                let p = element
                    .as_u64()
                    .unwrap_or_else(|| fail(file, "has a non-integer position"));
                (p, p)
            }
            Value::Array(pair) if pair.len() == 2 => {
                let s = pair[0]
                    .as_u64()
                    .unwrap_or_else(|| fail(file, "has a non-integer range start"));
                let e = pair[1]
                    .as_u64()
                    .unwrap_or_else(|| fail(file, "has a non-integer range end"));
                if s >= e {
                    fail(file, "kept a range whose start is not less than its end");
                }
                (s, e)
            }
            _ => fail(
                file,
                "has an index element that is neither a position nor a range",
            ),
        };
        if start < 1 || end > n {
            fail(file, "kept an index value outside the sequence");
        }
        if let Some((ps, pe)) = previous
            && (start, end) <= (ps, pe)
        {
            fail(file, "left an index unsorted or duplicated");
        }
        previous = Some((start, end));
    }
}

/// Report a canonical-form violation and stop. Generating a corpus from a
/// broken oracle is worse than generating nothing.
fn fail(file: &str, what: &str) -> ! {
    panic!("{file}: canonical form {what}")
}

/// Whether an object's members appear in exactly this order.
fn keys_are(object: &Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && object.keys().zip(expected).all(|(k, e)| k == e)
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

fn main() {
    // <generator>/src/main.rs -> <generator> -> spec/conformance
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the generator crate always has a parent directory")
        .to_path_buf();
    let repo = dir
        .parent()
        .and_then(Path::parent)
        .expect("spec/conformance is always two levels below the repo root")
        .to_path_buf();

    let mut c = Corpus::new(dir);

    // ══ VALID ═══════════════════════════════════════════════════════════════

    c.valid_roundtrip(
        "minimal",
        "Only the three members the schema requires; annotations and metadata default to empty.",
        doc(&[]),
    );

    c.valid_roundtrip(
        "empty-containers",
        "annotations and metadata present but empty.",
        doc(&[("annotations", json!({})), ("metadata", json!({}))]),
    );

    c.valid_roundtrip(
        "all-families",
        "One entry in each of the five annotation families.",
        doc(&[
            (
                "annotations",
                json!({
                    "site": {"Active site": {"index": [5, 17], "type": "active site"}},
                    "region": {"Domain": {"index": [[3, 10], [15, 22]], "type": "repeat"}},
                    "ptm": {"Phosphorylation": {"index": [2, 18], "type": "phosphoserine"}},
                    "processing": {"Signal peptide": {"index": [[1, 6]], "type": "signal"}},
                    "variant": [
                        {"position": 5, "from": "R", "to": "W"},
                        {"position": 18, "from": "Y", "to": "C"},
                    ],
                }),
            ),
            (
                "metadata",
                json!({
                    "uniprot_id": "P10636",
                    "description": "Microtubule-associated protein tau",
                    "reference": "https://example.org/ref",
                    "organism": "Homo sapiens",
                }),
            ),
        ]),
    );

    c.valid_roundtrip(
        "lowercase-sequence",
        "Lowercase sequence is accepted and case-folded to uppercase on parse.",
        doc(&[
            ("sequence", json!(SEQ.to_lowercase())),
            ("annotations", json!({"site": {"s": {"index": [1, 24]}}})),
        ]),
    );

    c.valid_roundtrip(
        "unsorted-positions",
        "Positions out of order are valid; normalization sorts them ascending.",
        annotated(json!({"site": {"s": {"index": [17, 3, 24, 1], "type": ""}}})),
    );

    c.valid_roundtrip(
        "unsorted-ranges",
        "Ranges out of order are valid; normalization sorts them by start, then end.",
        annotated(json!({"region": {"r": {"index": [[15, 22], [3, 10]], "type": ""}}})),
    );

    c.valid_roundtrip(
        "annotation-name-order-preserved",
        "Annotation names keep document order; normalization never reorders authored keys.",
        annotated(json!({
            "site": {
                "zeta": {"index": [1]},
                "alpha": {"index": [2]},
                "Mu": {"index": [3]},
            }
        })),
    );

    c.valid_roundtrip(
        "special-annotation-names",
        "Annotation names containing characters that JSON Pointer escapes, or does not.",
        annotated(json!({
            "site": {
                "a/b": {"index": [1]},
                "a~b": {"index": [2]},
                "a.b": {"index": [3]},
                "Domain (R1, R3, R4)": {"index": [4]},
                "β-sheet": {"index": [5]},
            }
        })),
    );

    c.valid_roundtrip(
        "integral-float-positions",
        "JSON has no integer type distinct from number; 3.0 is the position 3.",
        annotated(json!({
            "site": {"s": {"index": [3.0, 1.0]}},
            "region": {"r": {"index": [[5.0, 9.0]]}},
        })),
    );

    c.valid(
        "stop-codon",
        "'*' is a permitted sequence character.",
        doc(&[("sequence", json!("MAEPRQ*"))]),
    );

    c.valid(
        "min-length-sequence",
        "A two-character sequence is the shortest valid one.",
        doc(&[("sequence", json!("MA"))]),
    );

    c.valid(
        "adjacent-ranges",
        "Ranges that touch but do not overlap are accepted.",
        annotated(json!({"region": {"r": {"index": [[1, 3], [4, 8]]}}})),
    );

    c.valid(
        "boundary-positions",
        "Position 1 and position length(sequence) are both in bounds.",
        annotated(json!({"site": {"s": {"index": [1, N]}}})),
    );

    c.valid(
        "boundary-range",
        "A range spanning the whole sequence is in bounds.",
        annotated(json!({"region": {"r": {"index": [[1, N]]}}})),
    );

    c.valid(
        "ptm-positions",
        "A ptm index may be a position index.",
        annotated(json!({"ptm": {"Phospho": {"index": [2, 18]}}})),
    );

    c.valid(
        "ptm-ranges",
        "A ptm index may be a range index.",
        annotated(json!({"ptm": {"Glycan": {"index": [[2, 6], [18, 20]]}}})),
    );

    c.valid_roundtrip(
        "empty-index",
        "An empty index is valid in every family.",
        annotated(json!({
            "site": {"s": {"index": []}},
            "region": {"r": {"index": []}},
            "ptm": {"p": {"index": []}},
        })),
    );

    c.valid_roundtrip(
        "entry-without-type",
        "The entry 'type' member is optional and defaults to the empty string.",
        annotated(json!({"site": {"s": {"index": [5]}}})),
    );

    c.valid_roundtrip(
        "variant-open-fields",
        "A variant record's members beyond 'position' are open and preserved verbatim.",
        annotated(json!({
            "variant": [{
                "position": 5,
                "from": "R",
                "to": "W",
                "clinicalSignificance": "Pathogenic",
                "evidence": {"pmid": 12345, "confirmed": true},
                "aliases": ["R5W", "p.Arg5Trp"],
                "note": null,
            }]
        })),
    );

    c.valid(
        "variant-from-matches",
        "A variant 'from' residue that matches the sequence, compared case-insensitively.",
        annotated(json!({
            "variant": [
                {"position": 5, "from": "R"},
                {"position": 18, "from": "y"},
                {"position": 3, "from": "Glu"},
                {"position": 7},
            ]
        })),
    );

    c.valid_roundtrip(
        "metadata-partial",
        "Metadata members are individually optional and default to the empty string.",
        doc(&[("metadata", json!({"uniprot_id": "P10636"}))]),
    );

    // ══ INVALID — Stage 1 ═══════════════════════════════════════════════════

    c.invalid(
        "doc-not-object",
        "The document root is a JSON array. The guard rule makes this the only issue.",
        json!([{"$schema": URI, "a3_version": VER, "sequence": SEQ}]),
        1,
        &[(C::DocNotObject, "")],
    );

    c.invalid(
        "envelope-schema-missing",
        "$schema is absent.",
        raw(&[("a3_version", json!(VER)), ("sequence", json!(SEQ))]),
        1,
        &[(C::EnvelopeSchemaMissing, "/$schema")],
    );

    c.invalid(
        "envelope-schema-mismatch",
        "$schema is present but is not the canonical URI.",
        raw(&[
            (
                "$schema",
                json!("https://schema.rtemis.org/a3/v2/schema.json"),
            ),
            ("a3_version", json!(VER)),
            ("sequence", json!(SEQ)),
        ]),
        1,
        &[(C::EnvelopeSchemaMismatch, "/$schema")],
    );

    c.invalid(
        "envelope-version-missing",
        "a3_version is absent.",
        raw(&[("$schema", json!(URI)), ("sequence", json!(SEQ))]),
        1,
        &[(C::EnvelopeVersionMissing, "/a3_version")],
    );

    c.invalid(
        "envelope-version-mismatch",
        "a3_version is a constant; a later minor version is still a mismatch.",
        raw(&[
            ("$schema", json!(URI)),
            ("a3_version", json!("1.1.0")),
            ("sequence", json!(SEQ)),
        ]),
        1,
        &[(C::EnvelopeVersionMismatch, "/a3_version")],
    );

    // ══ INVALID — Stage 2 ═══════════════════════════════════════════════════

    c.invalid(
        "unknown-field-root",
        "An unpermitted top-level property.",
        doc(&[("organism", json!("Homo sapiens"))]),
        2,
        &[(C::UnknownField, "/organism")],
    );

    c.invalid(
        "unknown-field-family",
        "An unpermitted annotation family.",
        annotated(json!({"site": {"s": {"index": [1]}}, "domain": {}})),
        2,
        &[(C::UnknownField, "/annotations/domain")],
    );

    c.invalid(
        "unknown-field-entry",
        "An unpermitted member of an annotation entry.",
        annotated(json!({"site": {"s": {"index": [1], "colour": "red"}}})),
        2,
        &[(C::UnknownField, "/annotations/site/s/colour")],
    );

    c.invalid(
        "unknown-field-metadata",
        "An unpermitted metadata member.",
        doc(&[("metadata", json!({"pdb_id": "1TAU"}))]),
        2,
        &[(C::UnknownField, "/metadata/pdb_id")],
    );

    c.invalid(
        "seq-missing",
        "sequence is absent.",
        raw(&[("$schema", json!(URI)), ("a3_version", json!(VER))]),
        2,
        &[(C::SeqMissing, "/sequence")],
    );

    c.invalid(
        "seq-not-string",
        "sequence is a number.",
        doc(&[("sequence", json!(42))]),
        2,
        &[(C::SeqNotString, "/sequence")],
    );

    c.invalid(
        "seq-too-short",
        "A one-character sequence is too short.",
        doc(&[("sequence", json!("M"))]),
        2,
        &[(C::SeqTooShort, "/sequence")],
    );

    c.invalid(
        "seq-charset",
        "A digit is not a permitted sequence character.",
        doc(&[("sequence", json!("MAEP1Q"))]),
        2,
        &[(C::SeqCharset, "/sequence")],
    );

    c.invalid(
        "annotations-not-object",
        "annotations is an array. The guard rule suppresses everything beneath it.",
        doc(&[("annotations", json!([]))]),
        2,
        &[(C::AnnotationsNotObject, "/annotations")],
    );

    c.invalid(
        "metadata-not-object",
        "metadata is a string.",
        doc(&[("metadata", json!("P10636"))]),
        2,
        &[(C::MetadataNotObject, "/metadata")],
    );

    c.invalid(
        "metadata-field-not-string",
        "A metadata member is a number.",
        doc(&[("metadata", json!({"uniprot_id": 10636}))]),
        2,
        &[(C::MetadataFieldNotString, "/metadata/uniprot_id")],
    );

    c.invalid(
        "family-not-object",
        "An annotation family is an array. The guard rule suppresses everything beneath it.",
        annotated(json!({"site": [{"index": [1]}]})),
        2,
        &[(C::FamilyNotObject, "/annotations/site")],
    );

    c.invalid(
        "variant-list-not-array",
        "variant is an object rather than an array.",
        annotated(json!({"variant": {"0": {"position": 1}}})),
        2,
        &[(C::VariantListNotArray, "/annotations/variant")],
    );

    c.invalid(
        "name-empty",
        "An annotation name is the empty string; the path ends in an empty reference token.",
        annotated(json!({"site": {"": {"index": [1]}}})),
        2,
        &[(C::NameEmpty, "/annotations/site/")],
    );

    c.invalid(
        "entry-not-object",
        "A bare array is not an annotation entry. The guard rule suppresses everything beneath it.",
        annotated(json!({"site": {"s": [1, 2, 3]}})),
        2,
        &[(C::EntryNotObject, "/annotations/site/s")],
    );

    c.invalid(
        "entry-type-not-string",
        "The entry 'type' member is a number.",
        annotated(json!({"site": {"s": {"index": [1], "type": 7}}})),
        2,
        &[(C::EntryTypeNotString, "/annotations/site/s/type")],
    );

    c.invalid(
        "index-missing",
        "An annotation entry with no index.",
        annotated(json!({"site": {"s": {"type": "active site"}}})),
        2,
        &[(C::IndexMissing, "/annotations/site/s/index")],
    );

    c.invalid(
        "index-not-array",
        "index is a number. The guard rule suppresses everything beneath it.",
        annotated(json!({"site": {"s": {"index": 5}}})),
        2,
        &[(C::IndexNotArray, "/annotations/site/s/index")],
    );

    c.invalid(
        "index-element-type",
        "A site index element is a string, and another is a non-integral number.",
        annotated(json!({"site": {"s": {"index": [1, "5", 2.5]}}})),
        2,
        &[
            (C::IndexElementType, "/annotations/site/s/index/1"),
            (C::IndexElementType, "/annotations/site/s/index/2"),
        ],
    );

    c.invalid(
        "index-element-type-region",
        "A region index element is an integer rather than a range.",
        annotated(json!({"region": {"r": {"index": [[1, 3], 5]}}})),
        2,
        &[(C::IndexElementType, "/annotations/region/r/index/1")],
    );

    c.invalid(
        "index-element-type-boolean",
        "A boolean is not an integer, so it is not a position.",
        annotated(json!({"site": {"s": {"index": [true, 2]}}})),
        2,
        &[(C::IndexElementType, "/annotations/site/s/index/0")],
    );

    c.invalid(
        "index-mixed",
        "A ptm index mixes positions and ranges.",
        annotated(json!({"ptm": {"p": {"index": [1, [3, 5], 7]}}})),
        2,
        &[(C::IndexMixed, "/annotations/ptm/p/index")],
    );

    c.invalid(
        "index-mixed-other-wins",
        "An unclassifiable element takes priority over the mixed-geometry rule.",
        annotated(json!({"processing": {"p": {"index": [1, [3, 5], "x"]}}})),
        2,
        &[(C::IndexElementType, "/annotations/processing/p/index/2")],
    );

    c.invalid(
        "range-arity",
        "A range is a three-element array.",
        annotated(json!({"region": {"r": {"index": [[1, 3, 5]]}}})),
        2,
        &[(C::RangeArity, "/annotations/region/r/index/0")],
    );

    c.invalid(
        "range-endpoint-not-integer",
        "A range endpoint is a string.",
        annotated(json!({"region": {"r": {"index": [[1, "3"]]}}})),
        2,
        &[(C::RangeEndpointNotInteger, "/annotations/region/r/index/0")],
    );

    c.invalid(
        "variant-not-object",
        "A variant record is an integer. The guard rule suppresses everything beneath it.",
        annotated(json!({"variant": [5]})),
        2,
        &[(C::VariantNotObject, "/annotations/variant/0")],
    );

    c.invalid(
        "variant-position-missing",
        "A variant record with no position.",
        annotated(json!({"variant": [{"from": "R", "to": "W"}]})),
        2,
        &[(C::VariantPositionMissing, "/annotations/variant/0/position")],
    );

    c.invalid(
        "variant-position-not-integer",
        "A variant position is a string.",
        annotated(json!({"variant": [{"position": "5"}]})),
        2,
        &[(
            C::VariantPositionNotInteger,
            "/annotations/variant/0/position",
        )],
    );

    // ══ INVALID — Stage 3 ═══════════════════════════════════════════════════

    c.invalid(
        "pos-not-positive",
        "Positions are 1-based; zero and negative values are rejected. Subscripts are post-sort.",
        annotated(json!({"site": {"s": {"index": [5, 0, -3]}}})),
        3,
        &[
            (C::PosNotPositive, "/annotations/site/s/index/0"),
            (C::PosNotPositive, "/annotations/site/s/index/1"),
        ],
    );

    c.invalid(
        "pos-duplicate",
        "Duplicate positions are rejected, not deduplicated. One issue per repeated value.",
        annotated(json!({"site": {"s": {"index": [7, 3, 7, 3, 7]}}})),
        3,
        &[
            (C::PosDuplicate, "/annotations/site/s/index"),
            (C::PosDuplicate, "/annotations/site/s/index"),
        ],
    );

    c.invalid(
        "range-endpoint-not-positive",
        "A range endpoint below 1.",
        annotated(json!({"region": {"r": {"index": [[0, 5]]}}})),
        3,
        &[(C::RangeEndpointNotPositive, "/annotations/region/r/index/0")],
    );

    c.invalid(
        "range-order",
        "A degenerate range (start == end) and an inverted one (start > end).",
        annotated(json!({"region": {"r": {"index": [[3, 3], [12, 8]]}}})),
        3,
        &[
            (C::RangeOrder, "/annotations/region/r/index/0"),
            (C::RangeOrder, "/annotations/region/r/index/1"),
        ],
    );

    c.invalid(
        "range-overlap",
        "Two overlapping ranges. One issue per overlapping consecutive pair, after sorting.",
        annotated(json!({"region": {"r": {"index": [[1, 5], [4, 8]]}}})),
        3,
        &[(C::RangeOverlap, "/annotations/region/r/index")],
    );

    c.invalid(
        "variant-position-not-positive",
        "A variant position of zero.",
        annotated(json!({"variant": [{"position": 0}]})),
        3,
        &[(
            C::VariantPositionNotPositive,
            "/annotations/variant/0/position",
        )],
    );

    // ══ INVALID — Stage 4 ═══════════════════════════════════════════════════

    c.invalid(
        "pos-out-of-bounds",
        "A position past the end of a 24-residue sequence.",
        annotated(json!({"site": {"s": {"index": [5, 99]}}})),
        4,
        &[(C::PosOutOfBounds, "/annotations/site/s/index/1")],
    );

    c.invalid(
        "range-out-of-bounds",
        "A range whose end exceeds the sequence length; the path names the range, not the endpoint.",
        annotated(json!({"region": {"r": {"index": [[20, 99]]}}})),
        4,
        &[(C::RangeOutOfBounds, "/annotations/region/r/index/0")],
    );

    c.invalid(
        "variant-out-of-bounds",
        "A variant position past the end of the sequence.",
        annotated(json!({"variant": [{"position": 99}]})),
        4,
        &[(C::VariantOutOfBounds, "/annotations/variant/0/position")],
    );

    c.invalid(
        "variant-residue-mismatch",
        "Residue 5 is R, not K. A 'from' of another shape is not this rule's problem.",
        annotated(json!({
            "variant": [
                {"position": 5, "from": "K", "to": "W"},
                {"position": 18, "from": "Y"},
                {"position": 3, "from": "Glu"},
            ]
        })),
        4,
        &[(C::VariantResidueMismatch, "/annotations/variant/0/from")],
    );

    c.invalid(
        "path-escaping",
        "Annotation names containing '/' and '~' must be escaped as ~1 and ~0 in the path.",
        annotated(json!({
            "site": {
                "a/b": {"index": [99]},
                "a~b": {"index": [98]},
                "a.b": {"index": [97]},
            }
        })),
        4,
        &[
            (C::PosOutOfBounds, "/annotations/site/a~1b/index/0"),
            (C::PosOutOfBounds, "/annotations/site/a~0b/index/0"),
            (C::PosOutOfBounds, "/annotations/site/a.b/index/0"),
        ],
    );

    // ══ INVALID — multi-error ═══════════════════════════════════════════════

    c.invalid(
        "multi-envelope",
        "Both envelope members are wrong. A fail-fast implementation reports only one.",
        raw(&[
            ("$schema", json!("urn:a3")),
            ("a3_version", json!("0.9.0")),
            ("sequence", json!(SEQ)),
        ]),
        1,
        &[
            (C::EnvelopeSchemaMismatch, "/$schema"),
            (C::EnvelopeVersionMismatch, "/a3_version"),
        ],
    );

    c.invalid(
        "multi-structural",
        "Six independent Stage 2 issues across four subtrees.",
        doc(&[
            ("sequence", json!("MAEP1Q")),
            (
                "annotations",
                json!({
                    "site": {"": {"index": [1]}, "s": {"type": "x"}},
                    "region": {"r": {"index": [[1, 3, 5]]}},
                    "variant": [{"from": "R"}],
                }),
            ),
            ("metadata", json!({"uniprot_id": 10636})),
        ]),
        2,
        &[
            (C::SeqCharset, "/sequence"),
            (C::NameEmpty, "/annotations/site/"),
            (C::IndexMissing, "/annotations/site/s/index"),
            (C::RangeArity, "/annotations/region/r/index/0"),
            (C::VariantPositionMissing, "/annotations/variant/0/position"),
            (C::MetadataFieldNotString, "/metadata/uniprot_id"),
        ],
    );

    c.invalid(
        "multi-intra-field",
        "Five Stage 3 issues across three annotations, mixing per-element and whole-set rules.",
        annotated(json!({
            "site": {"s": {"index": [3, 0, 3]}},
            "region": {"r": {"index": [[1, 5], [4, 8]]}, "q": {"index": [[9, 9]]}},
            "variant": [{"position": 0}],
        })),
        3,
        &[
            (C::PosNotPositive, "/annotations/site/s/index/0"),
            (C::PosDuplicate, "/annotations/site/s/index"),
            (C::RangeOverlap, "/annotations/region/r/index"),
            (C::RangeOrder, "/annotations/region/q/index/0"),
            (
                C::VariantPositionNotPositive,
                "/annotations/variant/0/position",
            ),
        ],
    );

    c.invalid(
        "multi-contextual",
        "Five Stage 4 issues, including the biological one.",
        annotated(json!({
            "site": {"s": {"index": [5, 30, 40]}},
            "region": {"r": {"index": [[20, 99]]}},
            "variant": [{"position": 5, "from": "K"}, {"position": 88}],
        })),
        4,
        &[
            (C::PosOutOfBounds, "/annotations/site/s/index/1"),
            (C::PosOutOfBounds, "/annotations/site/s/index/2"),
            (C::RangeOutOfBounds, "/annotations/region/r/index/0"),
            (C::VariantResidueMismatch, "/annotations/variant/0/from"),
            (C::VariantOutOfBounds, "/annotations/variant/1/position"),
        ],
    );

    c.invalid(
        "multi-stage-shadowing",
        "One Stage 2 issue alongside sixteen would-be Stage 4 issues. Only the Stage 2 issue is reported.",
        doc(&[
            ("sequence", json!("MAEP1Q")),
            (
                "annotations",
                json!({
                    "site": {
                        "s": {"index": [30, 31, 32, 33, 34, 35, 36, 37]},
                        "t": {"index": [40, 41, 42, 43, 44, 45, 46, 47]},
                    }
                }),
            ),
        ]),
        2,
        &[(C::SeqCharset, "/sequence")],
    );

    c.invalid(
        "multi-guard-siblings",
        "A guard suppresses only its own subtree; two sibling entries each report their own.",
        annotated(json!({
            "site": {
                "s": [1, 2],
                "t": {"index": 5},
                "u": {"index": [99]},
            }
        })),
        2,
        &[
            (C::EntryNotObject, "/annotations/site/s"),
            (C::IndexNotArray, "/annotations/site/t/index"),
        ],
    );

    // ══ mapt ════════════════════════════════════════════════════════════════

    let mapt_path = repo.join("examples/mapt.a3.json");
    let mapt: Value = serde_json::from_str(
        &fs::read_to_string(&mapt_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", mapt_path.display())),
    )
    .expect("examples/mapt.a3.json is valid JSON");
    c.valid_roundtrip(
        "mapt",
        "The MAPT example from examples/, as a real-world round-trip case.",
        mapt,
    );

    c.finish();
}
