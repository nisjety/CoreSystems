//! The vendored provider read golden, checked against the Rust table.
//!
//! `fixtures/provider-read-operations.json` is a byte-identical copy of the golden
//! that integration-corev2's Go test owns. That test pins the golden's sha256 and
//! checks the executor against it. This test checks that the copy says what
//! `READ_OPERATIONS` says, row for row, and that no write is listed as a read.
//!
//! The sha256 pin is checked here as well as in the Go test `TestReadOperationsGoldenIsPinned`.
//! `READ_GOLDEN_SHA256` must name the same digest as that pin: a change to the golden moves both
//! pins in one reviewed change. A byte-level drift between the two copies is caught by the Go
//! test `TestReadOperationsVendoredCopyIsByteIdentical`.

use mp_contracts::provider_reads::READ_OPERATIONS;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const GOLDEN: &str = include_str!("fixtures/provider-read-operations.json");
/// The digest the Go test pins for the golden: re-pin both when the golden changes.
const READ_GOLDEN_SHA256: &str = "efc6009715aa27e1b0fb916ad6b6ddbaf84882abf845a4c22f5bb47b45499e4e";

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden parses as JSON")
}

fn string_list(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("params is an array")
        .iter()
        .map(|item| item.as_str().expect("param is a string"))
        .collect()
}

#[test]
fn provider_read_golden_bytes_match_the_pinned_sha256() {
    let digest = format!("{:x}", Sha256::digest(GOLDEN.as_bytes()));
    assert_eq!(
        digest, READ_GOLDEN_SHA256,
        "the golden changed: re-pin READ_GOLDEN_SHA256 and the Go pin together"
    );
}

#[test]
fn golden_rows_match_the_contract_table_row_for_row() {
    let golden = golden();
    let rows = golden["operations"]
        .as_array()
        .expect("operations is an array");
    assert_eq!(golden["version"], 1, "golden version");
    assert_eq!(rows.len(), READ_OPERATIONS.len(), "row count");

    for (row, expected) in rows.iter().zip(READ_OPERATIONS) {
        let keys: BTreeSet<&str> = row
            .as_object()
            .expect("row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["provider", "operation", "params", "capability", "sensitive"]),
            "row keys for {}",
            expected.operation
        );
        assert_eq!(row["provider"], expected.provider);
        assert_eq!(row["operation"], expected.operation);
        assert_eq!(
            string_list(&row["params"]),
            expected.params,
            "params of {}",
            expected.operation
        );
        assert_eq!(
            row["capability"], expected.capability,
            "capability of {}",
            expected.operation
        );
        assert_eq!(
            row["sensitive"], expected.sensitive,
            "sensitive of {}",
            expected.operation
        );
    }
}

#[test]
fn excluded_writes_are_never_listed_as_reads() {
    let golden = golden();
    let excluded = golden["excludedWrites"]
        .as_array()
        .expect("excludedWrites is an array");
    assert!(
        !excluded.is_empty(),
        "the writes kept out of the catalogue are named"
    );
    for write in excluded {
        let provider = write["provider"].as_str().expect("provider");
        let operation = write["operation"].as_str().expect("operation");
        assert!(
            !READ_OPERATIONS
                .iter()
                .any(|read| read.provider == provider && read.operation == operation),
            "{provider} {operation} is a write and must not be a read"
        );
    }
}
