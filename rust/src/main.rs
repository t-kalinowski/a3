//! `a3` — CLI tool to validate and inspect A3 amino acid annotation files.
//!
//! Usage: `a3 [OPTIONS] <FILE>`
//! Pass `-` as `<FILE>` to read from stdin.
//!
//! Exit codes: `0` valid, `1` invalid, `2` could not be read or parsed.

use clap::Parser;
use colored::Colorize;
use rtemis_a3::{A3, A3Error, A3Issue, a3_from_json};
use serde_json::{Value, json};
use std::io::{self, IsTerminal, Read};
use std::process;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "a3",
    version,
    about = "Validate and inspect A3 amino acid annotation files"
)]
struct Cli {
    /// Path to the A3 JSON file (use `-` for stdin)
    file: String,

    /// Maximum number of sequence residues to display
    #[arg(short, long, default_value_t = 20)]
    limit: usize,

    /// Suppress all output; use exit code only
    #[arg(short, long)]
    quiet: bool,

    /// Output results in JSON format
    #[arg(short, long)]
    json: bool,

    /// Deprecated and ignored: every run is a full diagnostic, because
    /// validation always accumulates every issue in the failing stage.
    #[arg(short = 'D', long, hide = true)]
    diagnose: bool,
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Word-wrap `text` to `width` columns, returning one string per line.
///
/// Words that individually exceed `width` are placed on their own line
/// unbroken. If `text` fits within `width`, returns a single-element vec.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.chars().count() <= width {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(current.clone());
            current = word.to_string();
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        vec![text.to_string()]
    } else {
        lines
    }
}

/// Build the parenthetical name hint for an annotation row.
///
/// Shows up to 3 names. Appends `…` if there are more than 3 total, or if
/// any name had to be cropped to stay within `available` display columns.
/// `available` is the space for the content *inside* the parentheses.
fn build_hint(names: &[String], available: usize) -> String {
    if names.is_empty() || available < 2 {
        return String::new();
    }
    let more_than_three = names.len() > 3;
    let mut result = String::new();

    for (i, name) in names.iter().take(3).enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        let candidate = format!("{}{}", sep, name);
        let after_cols = result.chars().count() + candidate.chars().count();
        // Reserve 1 display column for "…" unless this is provably the last item.
        let is_last = i + 1 == names.len() && !more_than_three;
        let reserve = if is_last { 0 } else { 1 };

        if after_cols + reserve <= available {
            result.push_str(&candidate);
        } else {
            // Crop: append "…" to whatever we've accumulated so far.
            if result.chars().count() < available {
                result.push('…');
            }
            return result;
        }
    }

    if more_than_three && result.chars().count() < available {
        result.push('…');
    }
    result
}

/// Print the inspection view of a valid document.
fn print_valid(a3: &A3, limit: usize) {
    println!();
    println!(
        "  {} {} {}",
        "✓ valid".green().bold(),
        format!("A3 {}", a3.a3_version())
            .bold()
            .truecolor(71, 156, 255),
        a3.schema().dimmed(),
    );
    println!();

    // --- Sequence ---
    let seq = a3.sequence();
    let char_count = seq.chars().count();
    let preview: String = seq.chars().take(limit).collect();
    let seq_display = if char_count > limit {
        format!("{}… (length = {})", preview, char_count)
    } else {
        format!("{} (length = {})", seq, char_count)
    };
    println!(
        "  {}  {}",
        "Sequence".bold(),
        seq_display.truecolor(220, 150, 86)
    );

    // --- Annotations ---
    println!();
    println!("  {}", "Annotations".bold());

    let ann = a3.annotations();

    // Names in document order — that order is part of the document, so the
    // display should not invent a different one.
    let site_names: Vec<String> = ann.site().keys().cloned().collect();
    let region_names: Vec<String> = ann.region().keys().cloned().collect();
    let ptm_names: Vec<String> = ann.ptm().keys().cloned().collect();
    let proc_names: Vec<String> = ann.processing().keys().cloned().collect();
    let var_names: Vec<String> = ann
        .variant()
        .iter()
        .map(|v| format!("pos {}", v.position()))
        .collect();

    let entries = [
        ("site", ann.site().len(), site_names),
        ("region", ann.region().len(), region_names),
        ("ptm", ann.ptm().len(), ptm_names),
        ("processing", ann.processing().len(), proc_names),
        ("variant", ann.variant().len(), var_names),
    ];
    let last = entries.len() - 1;
    for (i, (name, count, names)) in entries.iter().enumerate() {
        let connector = if i == last { "└──" } else { "├──" };
        let padded = format!("{:<12}", name);
        let count_str = if *count == 0 {
            "—".dimmed().to_string()
        } else {
            count.to_string().truecolor(220, 150, 86).to_string()
        };
        // Columns consumed before the opening paren:
        // 2 (indent) + 3 (connector) + 1 (space) + 12 (padded name) + count digits + 2 (gap) + 1 '('
        let prefix_cols = 21
            + if *count == 0 {
                1
            } else {
                count.to_string().len()
            };
        let available = 90usize.saturating_sub(prefix_cols + 1); // +1 for ')'
        let hint_content = build_hint(names, available);
        let hint = if hint_content.is_empty() {
            String::new()
        } else {
            format!("  {}", format!("({})", hint_content).dimmed())
        };
        println!("  {} {}{}{}", connector.dimmed(), padded, count_str, hint);
    }

    // --- Metadata ---
    println!();
    println!("  {}", "Metadata".bold());

    let meta = a3.metadata();
    let meta_rows: [(&str, &str); 4] = [
        ("UniProt ID", meta.uniprot_id()),
        ("Description", meta.description()),
        ("Reference", meta.reference()),
        ("Organism", meta.organism()),
    ];
    let label_width = meta_rows.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    // 2 (indent) + 3 (connector) + 1 (space) + label_width + 2 (gap)
    let value_col = 8 + label_width;
    let value_width = 90usize.saturating_sub(value_col);
    let last = meta_rows.len() - 1;
    for (i, (label, value)) in meta_rows.iter().enumerate() {
        let is_last = i == last;
        let connector = if is_last { "└──" } else { "├──" };
        // Non-last items get a │ at the connector column to keep the list
        // visually uninterrupted across wrapped value lines.
        let continuation = if is_last {
            " ".repeat(value_col)
        } else {
            format!("  {}{}", "│".dimmed(), " ".repeat(value_col - 3))
        };
        if value.is_empty() {
            println!(
                "  {} {:<label_width$}  {}",
                connector.dimmed(),
                label,
                "—".dimmed(),
                label_width = label_width,
            );
        } else {
            let lines = wrap_words(value, value_width);
            print!(
                "  {} {:<label_width$}  {}",
                connector.dimmed(),
                label,
                lines[0].truecolor(220, 150, 86),
                label_width = label_width,
            );
            for line in &lines[1..] {
                print!("\n{}{}", continuation, line.truecolor(220, 150, 86));
            }
            println!();
        }
    }
    println!();
}

/// Print the issue list of an invalid document.
///
/// Every issue in the list belongs to the same stage — validation stops at a
/// stage boundary — so the stage is reported once, in the header.
fn print_invalid(issues: &[A3Issue]) {
    println!();
    let stage = issues.first().map(A3Issue::stage).unwrap_or(0);
    println!(
        "  {}  {}",
        "✗ invalid".red().bold(),
        format!(
            "{} issue(s) at stage {} ({})",
            issues.len(),
            stage,
            stage_name(stage)
        )
        .dimmed()
    );
    println!();

    let last = issues.len().saturating_sub(1);
    for (i, issue) in issues.iter().enumerate() {
        let connector = if i == last { "└──" } else { "├──" };
        let where_ = if issue.path.is_empty() {
            "<document>"
        } else {
            &issue.path
        };
        println!(
            "  {} {}  {}",
            connector.dimmed(),
            issue.code.code().red().bold(),
            where_.truecolor(220, 150, 86)
        );
        let indent = if i == last { "     " } else { "  │  " };
        println!("  {}{}", indent.dimmed(), issue.message);
    }
    println!();
}

fn stage_name(stage: u8) -> &'static str {
    match stage {
        1 => "envelope",
        2 => "structural",
        3 => "intra-field",
        4 => "contextual",
        _ => "unknown",
    }
}

/// Build the JSON output for a valid document.
fn json_valid(a3: &A3, limit: usize) -> Value {
    let meta = a3.metadata();
    let ann = a3.annotations();
    let seq = a3.sequence();

    json!({
        "valid": true,
        "issues": [],
        "metadata": {
            "uniprot_id": meta.uniprot_id(),
            "description": meta.description(),
            "reference": meta.reference(),
            "organism": meta.organism(),
        },
        "sequence_length": seq.chars().count(),
        "sequence_preview": seq.chars().take(limit).collect::<String>(),
        "annotations": {
            "site": ann.site().len(),
            "region": ann.region().len(),
            "ptm": ann.ptm().len(),
            "processing": ann.processing().len(),
            "variant": ann.variant().len(),
        }
    })
}

/// Build the JSON output for an invalid document.
///
/// Issues serialize as `{code, path, message}`, which makes the CLI usable as
/// the conformance oracle when debugging another implementation.
fn json_invalid(issues: &[A3Issue]) -> Value {
    json!({
        "valid": false,
        "stage": issues.first().map(A3Issue::stage),
        "issues": issues,
    })
}

fn emit(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("output value is always serializable")
    );
}

// ---------------------------------------------------------------------------
// Input reading
// ---------------------------------------------------------------------------

fn read_input(file: &str) -> Result<String, String> {
    if file == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Error reading stdin: {e}"))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(file).map_err(|e| format!("Error reading '{file}': {e}"))
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    // Disable colors when stdout is not a terminal (pipe, redirect, --quiet).
    if !std::io::stdout().is_terminal() {
        colored::control::set_override(false);
    }

    let content = read_input(&cli.file).unwrap_or_else(|e| {
        if !cli.quiet {
            eprintln!("{e}");
        }
        process::exit(2);
    });

    match a3_from_json(&content) {
        Ok(a3) => {
            if !cli.quiet {
                if cli.json {
                    emit(&json_valid(&a3, cli.limit));
                } else {
                    print_valid(&a3, cli.limit);
                }
            }
            process::exit(0);
        }
        Err(A3Error::Validate(issues)) => {
            if !cli.quiet {
                if cli.json {
                    emit(&json_invalid(&issues));
                } else {
                    print_invalid(&issues);
                }
            }
            process::exit(1);
        }
        // Not JSON at all — there is no path to report and nothing to collect.
        Err(e) => {
            if !cli.quiet {
                if cli.json {
                    emit(&json!({"valid": false, "issues": [], "error": e.to_string()}));
                } else {
                    println!("\n  {}", "✗ invalid".red().bold());
                    println!("  {}\n", e.to_string().red());
                }
            }
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_JSON: &str = r#"{
        "$schema": "https://schema.rtemis.org/a3/v1/schema.json",
        "a3_version": "1.0.0",
        "sequence": "MAEPRQ"
    }"#;

    #[test]
    fn json_output_reports_codes_and_paths() {
        let json = r#"{
            "$schema": "https://schema.rtemis.org/a3/v1/schema.json",
            "a3_version": "1.0.0",
            "sequence": "MAEPRQ",
            "annotations": {"site": {"s": {"index": [99]}}}
        }"#;
        let Err(A3Error::Validate(issues)) = a3_from_json(json) else {
            panic!("expected validation issues");
        };
        let v = json_invalid(&issues);
        assert_eq!(v["valid"], false);
        assert_eq!(v["stage"], 4);
        assert_eq!(v["issues"][0]["code"], "A3E_POS_OUT_OF_BOUNDS");
        assert_eq!(v["issues"][0]["path"], "/annotations/site/s/index/0");
        assert!(v["issues"][0]["message"].is_string());
    }

    // Regression test for a panic where --limit landed inside a multibyte
    // character. `chars()` never touches a byte boundary.
    #[test]
    fn sequence_preview_counts_characters_not_bytes() {
        let a3 = a3_from_json(MINIMAL_JSON).unwrap();
        let v = json_valid(&a3, 3);
        assert_eq!(v["sequence_preview"], "MAE");
        assert_eq!(v["sequence_length"], 6);
    }

    #[test]
    fn wraps_long_metadata_values() {
        let lines = wrap_words("one two three four", 9);
        assert_eq!(lines, vec!["one two", "three", "four"]);
    }
}
