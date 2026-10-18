//! The JSON Schema of `ChallengeSignal`, pinned as a golden.
//!
//! `docs/schemas/challenge-signal.schema.json` is derived from the Rust types, so
//! it cannot drift from what the edge serializes: this test fails when
//! `ChallengeSignal`, `ChallengeKind`, `ChallengeStrength`, `BlockReason` or
//! `ChallengeRecovery` change without the golden, and `UPDATE_SCHEMAS=1` rewrites
//! it. `scripts/tests/openapi-challenge-parity.test.mjs` checks `docs/openapi.yaml`
//! against the golden, the way `openapi-scrape-parity.test.mjs` does for the
//! scrape options.
//!
//! Run with `cargo test -p quarry-core --features jsonschema --test challenge_schema`.

use quarry_core::contracts::{BlockReason, ChallengeSignal};

fn golden_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schemas").join(name)
}

fn rendered() -> String {
    serde_json::to_string_pretty(&schemars::schema_for!(ChallengeSignal)).expect("schema serializes") + "\n"
}

#[test]
fn the_challenge_signal_schema_matches_its_golden() {
    let rendered = rendered();
    let path = golden_path("challenge-signal.schema.json");
    if std::env::var("UPDATE_SCHEMAS").as_deref() == Ok("1") {
        std::fs::write(&path, &rendered).expect("write golden");
        return;
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing: run with UPDATE_SCHEMAS=1", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        golden, rendered,
        "challenge-signal.schema.json no longer matches the Rust types: rerun with UPDATE_SCHEMAS=1 and review the diff"
    );
}

/// The wire names in the schema are the names `BlockReason::as_str` hands to
/// log counters and error details, so a counter label cannot differ from the
/// value a client reads.
#[test]
fn block_reason_as_str_is_the_wire_name_in_the_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(ChallengeSignal)).expect("schema");
    let def = &schema["$defs"]["BlockReason"];
    let names: Vec<String> = match def["oneOf"].as_array() {
        Some(entries) => entries
            .iter()
            .map(|entry| entry["const"].as_str().expect("const").to_owned())
            .collect(),
        None => def["enum"]
            .as_array()
            .expect("an enum")
            .iter()
            .map(|value| value.as_str().expect("string").to_owned())
            .collect(),
    };
    assert_eq!(names.len(), 4, "{names:?}");
    for name in names {
        let reason: BlockReason = serde_json::from_value(serde_json::Value::String(name.clone())).expect("a reason");
        assert_eq!(reason.as_str(), name);
    }
}
