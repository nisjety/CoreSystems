//! Metric-label guard for session-core (WS06-S3).
//!
//! Walks this crate's `src` tree and asserts that no `counter!`, `gauge!` or `histogram!`
//! carries a label key that is not on `mp_contracts::telemetry_labels::METRIC_LABELS`. A label
//! whose value is request data (a thread id, a run id, a space id, a raw path) would put that
//! identifier into the metrics store of every scraper.

use std::fs;
use std::path::{Path, PathBuf};

use mp_contracts::telemetry_labels::disallowed_metric_labels;

/// Known label keys committed by WS06-S2 (`mp_session_space_zdr_flag_total{action,enforced}`)
/// that are not yet on `METRIC_LABELS`. Both carry closed values (a `&'static str` action and an
/// enforce/observe label, no ids), so the exemption is narrow: exact (metric, key) pairs only.
/// DEPTH-WS06S3-1: add `action` and `enforced` to the reviewed list, then delete this table.
///
/// The two write-policy refusal counters (WS26-S4, on main) carry `origin`, the closed
/// six-value `write_policy::Origin::label()` (user, model_tool, delegation, workflow,
/// dreaming, background_review): no id reaches it. They are exempted as exact pairs until
/// `origin` is added to the reviewed list in `mp-contracts`.
const KNOWN_EXEMPTIONS: &[(&str, &str)] = &[
    ("mp_session_space_zdr_flag_total", "action"),
    ("mp_session_space_zdr_flag_total", "enforced"),
    ("mp_session_memory_policy_refused_total", "origin"),
    ("mp_session_skill_policy_refused_total", "origin"),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn src_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    files
}

#[test]
fn no_metric_uses_a_label_key_outside_the_closed_list() {
    let files = src_files();
    assert!(!files.is_empty(), "the src walk found no files");
    let mut offences = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read source file");
        for found in disallowed_metric_labels(&source) {
            if KNOWN_EXEMPTIONS.contains(&(found.metric.as_str(), found.label.as_str())) {
                continue;
            }
            offences.push(format!(
                "{}: metric `{}` uses label `{}`",
                file.display(),
                found.metric,
                found.label
            ));
        }
    }
    assert!(
        offences.is_empty(),
        "metric labels outside METRIC_LABELS (add the key to the reviewed list or drop the label):\n{}",
        offences.join("\n")
    );
}

#[test]
fn a_synthetic_thread_id_label_fails_the_scan() {
    let synthetic = r#"counter!("synthetic_total", "thread_id" => id.to_string()).increment(1);"#;
    let found = disallowed_metric_labels(synthetic);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "thread_id");
    assert_eq!(found[0].metric, "synthetic_total");
}
