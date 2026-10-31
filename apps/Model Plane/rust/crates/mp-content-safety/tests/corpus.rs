//! Corpus gate for the shared scanner (WS26-S2, extended by WS26-S8).
//!
//! Deterministic rows must decide the scan: every `needs: deterministic`
//! attack is a hit, and no benign row is a hit. Semantic rows must be left
//! clean by the scanner, so only the live classifier can flag them. Their
//! numbers come from the eval in `model-gateway/tests/content_safety_eval.rs`.
//! The corpus is synthetic and public. A failing line prints its id and kind,
//! never its text.

#[allow(dead_code)]
#[path = "corpus/rows.rs"]
mod rows;

use std::collections::BTreeSet;

use mp_content_safety::rules::MARKERS;
use mp_content_safety::scan::{scan_in, ScanMode};
use rows::{Expect, Needs, Row};

#[test]
fn corpus_meets_the_minimum_sizes() {
    let sizes = [
        ("attacks.jsonl", rows::attacks().len(), 150),
        ("attacks_semantic.jsonl", rows::attacks_semantic().len(), 100),
        ("benign.jsonl", rows::benign().len(), 300),
        ("benign_semantic.jsonl", rows::benign_semantic().len(), 60),
    ];
    for (file, have, need) in sizes {
        assert!(have >= need, "{file} has {have} rows, need >= {need}");
    }
}

#[test]
fn each_file_declares_one_expectation_and_one_layer() {
    let files: [(&str, Vec<Row>, Expect, Needs); 4] = [
        ("attacks.jsonl", rows::attacks(), Expect::Flag, Needs::Deterministic),
        ("attacks_semantic.jsonl", rows::attacks_semantic(), Expect::Flag, Needs::Semantic),
        ("benign.jsonl", rows::benign(), Expect::Clean, Needs::Deterministic),
        ("benign_semantic.jsonl", rows::benign_semantic(), Expect::Clean, Needs::Semantic),
    ];
    for (file, rows, expect, needs) in files {
        let stray: Vec<&str> = rows
            .iter()
            .filter(|row| row.expect != expect || row.needs != needs)
            .map(|row| row.id.as_str())
            .collect();
        assert!(stray.is_empty(), "{file} has rows outside its expect and needs: {stray:?}");
    }
}

#[test]
fn every_deterministic_attack_is_a_hit() {
    let misses: Vec<String> = rows::attacks()
        .iter()
        .filter(|row| !scan_in(ScanMode::Normalized, &row.text).is_hit())
        .map(|row| format!("{} ({})", row.id, row.kind))
        .collect();
    assert!(misses.is_empty(), "attack lines not flagged: {misses:?}");
}

#[test]
fn no_benign_line_is_a_hit() {
    let false_positives: Vec<String> = rows::benign()
        .into_iter()
        .chain(rows::benign_semantic())
        .filter_map(|row| {
            let report = scan_in(ScanMode::Normalized, &row.text);
            report
                .is_hit()
                .then(|| format!("{} ({}): {:?}", row.id, row.kind, report.rule_ids))
        })
        .collect();
    assert!(false_positives.is_empty(), "benign lines flagged: {false_positives:?}");
}

/// A semantic row the scanner already flags would test the scanner, and the live
/// numbers would stop measuring the classifier.
#[test]
fn semantic_attacks_are_left_to_the_classifier() {
    let already_flagged: Vec<String> = rows::attacks_semantic()
        .iter()
        .filter(|row| scan_in(ScanMode::Normalized, &row.text).is_hit())
        .map(|row| format!("{} ({})", row.id, row.kind))
        .collect();
    assert!(
        already_flagged.is_empty(),
        "semantic attack lines the scanner already flags: {already_flagged:?}"
    );
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
    for row in rows::all() {
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
    for row in rows::attacks() {
        for rule in scan_in(ScanMode::Normalized, &row.text).rule_ids {
            assert!(
                rule.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
                "rule id {rule:?} is not a plain identifier"
            );
        }
    }
}

#[test]
fn ids_are_unique_across_the_corpus() {
    let mut seen = BTreeSet::new();
    for row in rows::all() {
        assert!(seen.insert(row.id.clone()), "duplicate corpus id {}", row.id);
    }
}

/// A 4 MiB benign payload must scan in under 250 ms. Only meaningful with
/// optimisations, so debug builds check correctness alone. The payload is the
/// S2 benign set, so the number stays comparable with the S2 baseline.
/// `benign_semantic.jsonl` scans markedly slower in release, and that is an open
/// finding for the owner, not something this payload should hide.
#[test]
fn a_four_mib_benign_payload_scans_within_budget() {
    let paragraph = rows::benign()
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
