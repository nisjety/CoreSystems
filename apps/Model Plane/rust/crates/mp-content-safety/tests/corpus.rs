//! Corpus gate for the shared scanner.
//!
//! Every attack line must be a hit under the normalised scan, and not one
//! benign line may be. Both files are synthetic and public: no customer text
//! belongs in them. A failing line prints its id and kind, never its text.

use std::path::PathBuf;

use mp_content_safety::rules::MARKERS;
use mp_content_safety::scan::{scan_in, ScanMode};

struct Row {
    id: String,
    kind: String,
    text: String,
}

fn corpus(file: &str) -> Vec<Row> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value =
                serde_json::from_str(line).expect("each corpus line is a JSON object");
            let field = |name: &str| {
                value[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("corpus row lacks string field `{name}`"))
                    .to_owned()
            };
            Row {
                id: field("id"),
                kind: field("kind"),
                text: field("text"),
            }
        })
        .collect()
}

fn attacks() -> Vec<Row> {
    corpus("attacks.jsonl")
}

fn benign() -> Vec<Row> {
    corpus("benign.jsonl")
}

#[test]
fn corpus_meets_the_minimum_sizes() {
    assert!(attacks().len() >= 150, "attacks.jsonl has {} rows, need >= 150", attacks().len());
    assert!(benign().len() >= 300, "benign.jsonl has {} rows, need >= 300", benign().len());
}

#[test]
fn every_attack_line_is_a_deterministic_hit() {
    let misses: Vec<String> = attacks()
        .iter()
        .filter(|row| !scan_in(ScanMode::Normalized, &row.text).is_hit())
        .map(|row| format!("{} ({})", row.id, row.kind))
        .collect();
    assert!(misses.is_empty(), "attack lines not flagged: {misses:?}");
}

#[test]
fn no_benign_line_is_a_hit() {
    let false_positives: Vec<String> = benign()
        .iter()
        .filter_map(|row| {
            let report = scan_in(ScanMode::Normalized, &row.text);
            report
                .is_hit()
                .then(|| format!("{} ({}): {:?}", row.id, row.kind, report.rule_ids))
        })
        .collect();
    assert!(false_positives.is_empty(), "benign lines flagged: {false_positives:?}");
}

#[test]
fn the_sixteen_original_phrases_are_hits_in_both_modes() {
    for marker in MARKERS.iter().filter(|marker| marker.legacy) {
        assert!(scan_in(ScanMode::Legacy, marker.phrase).is_hit(), "legacy misses {}", marker.rule_id);
        assert!(scan_in(ScanMode::Normalized, marker.phrase).is_hit(), "normalized misses {}", marker.rule_id);
    }
}

#[test]
fn normalisation_only_ever_adds_flags() {
    for row in attacks().iter().chain(benign().iter()) {
        if scan_in(ScanMode::Legacy, &row.text).is_hit() {
            assert!(
                scan_in(ScanMode::Normalized, &row.text).is_hit(),
                "{} was a legacy hit and is not a normalised hit",
                row.id
            );
        }
    }
}

#[test]
fn rule_ids_are_plain_identifiers() {
    for row in attacks().iter() {
        for rule in scan_in(ScanMode::Normalized, &row.text).rule_ids {
            assert!(
                rule.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
                "rule id {rule:?} is not a plain identifier"
            );
        }
    }
}

/// A 4 MiB benign payload must scan in under 250 ms. Only meaningful with
/// optimisations, so debug builds check correctness alone.
#[test]
fn a_four_mib_benign_payload_scans_within_budget() {
    let paragraph = benign()
        .into_iter()
        .map(|row| row.text)
        .collect::<Vec<_>>()
        .join(" ");
    let mut payload = String::with_capacity(4 * 1024 * 1024 + paragraph.len());
    while payload.len() < 4 * 1024 * 1024 {
        payload.push_str(&paragraph);
        payload.push(' ');
    }

    let started = std::time::Instant::now();
    let report = scan_in(ScanMode::Normalized, &payload);
    let elapsed = started.elapsed();

    assert!(!report.is_hit(), "the benign payload must stay clean");
    if !cfg!(debug_assertions) {
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "4 MiB benign scan took {elapsed:?}, budget is 250 ms"
        );
    }
}
