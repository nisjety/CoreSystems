//! The attention gate against the ZDR posture matrix (build plan WS16 S1).
//!
//! `MarkThreadSeen` is permitted only for a caller the matrix decides as `persistent`: a zero-retention
//! or waived caller keeps its read state on the client. The matrix is the one authority for postures
//! (`api/contracts/zdr-posture-matrix.json`). This test resolves every enforcement row's decision
//! through the matrix's retention-class vocabulary, so a row added later is covered without a new test.

use serde_json::Value;

const MATRIX: &str = include_str!("../../../../api/contracts/zdr-posture-matrix.json");
const ATTENTION_SOURCE: &str = include_str!("../src/attention.rs");

#[test]
fn the_crate_gate_permits_only_the_persistent_class() {
    // The crate's gate is `attention::mark_seen_permitted`. A binary crate's test cannot call into it,
    // so this pins the gate's expression: changing it has to change this pin too.
    assert!(
        ATTENTION_SOURCE.contains("class == RetentionClass::Persistent"),
        "mark_seen_permitted must permit exactly RetentionClass::Persistent"
    );
}

#[test]
fn enforcement_rows_resolve_to_retention_classes_and_only_persistent_is_permitted() {
    let matrix: Value = serde_json::from_str(MATRIX).expect("the posture matrix parses");

    let classes = &matrix["vocabulary"]["retention_classes"];
    assert_eq!(classes["persistent"], "persistent");
    assert_eq!(classes["zdr"], "zdr");
    assert_eq!(classes["waived"], "zdr_waived");

    let decisions: Vec<&str> = matrix["vocabulary"]["decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .map(|value| value.as_str().expect("a decision is a string"))
        .collect();
    assert_eq!(decisions, ["zdr", "persistent", "waived", "refuse"]);

    let rows = matrix["enforcement_rows"]
        .as_array()
        .expect("enforcement rows");
    assert!(!rows.is_empty(), "the matrix has enforcement rows");
    let (mut permitted_rows, mut refused_rows) = (0, 0);
    for row in rows {
        let id = row["id"].as_str().expect("row id");
        let decision = row["expect"]["decision"]
            .as_str()
            .expect("expected decision");
        assert!(
            decisions.contains(&decision),
            "{id} decides {decision}, which is not in the vocabulary"
        );
        if decision == "refuse" {
            refused_rows += 1;
            continue;
        }
        let class = classes[decision]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: a {decision} decision names no retention class"));
        // Only the persistent class passes the gate, so a waived or zero-retention row is refused.
        if class == "persistent" {
            permitted_rows += 1;
        } else {
            refused_rows += 1;
        }
    }
    assert!(
        permitted_rows > 0,
        "at least one row exercises the permitted case"
    );
    assert!(
        refused_rows > 0,
        "at least one row exercises the refused case"
    );
}
