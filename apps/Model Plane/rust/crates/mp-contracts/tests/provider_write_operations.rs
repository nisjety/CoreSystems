//! The vendored provider write golden, checked against the Rust table (EP-10-S2).
//!
//! `fixtures/provider-write-operations.json` is a byte-identical copy of the golden that
//! integration-corev2's Go test owns. The digest pinned here is the one that test pins, so a change
//! to the golden moves both pins in one reviewed change. The Go test
//! `TestWriteOperationsVendoredCopyIsByteIdentical` catches any drift between the two copies.

use mp_contracts::provider_reads::READ_OPERATIONS;
use mp_contracts::provider_writes::{editable_kind, EditableKind, WRITE_OPERATIONS};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const GOLDEN: &str = include_str!("fixtures/provider-write-operations.json");
/// The digest integration-corev2's Go test pins for the golden. Re-pin both when the golden changes.
const GOLDEN_SHA256: &str = "5e69423b54f6856fc8f70c6d2beafc5ed29653c431a80a1d215c2fcf994e7eaf";

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden parses as JSON")
}

fn string_list(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("aliases is an array")
        .iter()
        .map(|item| item.as_str().expect("alias is a string"))
        .collect()
}

#[test]
fn provider_write_golden_bytes_match_the_pinned_sha256() {
    let digest = format!("{:x}", Sha256::digest(GOLDEN.as_bytes()));
    assert_eq!(
        digest, GOLDEN_SHA256,
        "the golden changed: re-pin GOLDEN_SHA256 and the Go pin together"
    );
}

#[test]
fn provider_write_rows_match_the_table_row_for_row() {
    let golden = golden();
    assert_eq!(golden["version"], 1, "golden version");
    let rows = golden["operations"]
        .as_array()
        .expect("operations is an array");
    assert_eq!(rows.len(), WRITE_OPERATIONS.len(), "row count");

    for (row, expected) in rows.iter().zip(WRITE_OPERATIONS) {
        let keys: BTreeSet<&str> = row
            .as_object()
            .expect("row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "provider",
                "operation",
                "aliases",
                "approval_required",
                "direction",
                "editable_kind",
            ]),
            "row keys for {}",
            expected.operation
        );
        assert_eq!(row["provider"], expected.provider);
        assert_eq!(row["operation"], expected.operation);
        assert_eq!(
            string_list(&row["aliases"]),
            expected.aliases,
            "aliases of {}",
            expected.operation
        );
        assert_eq!(
            row["approval_required"], true,
            "{} is approval-gated",
            expected.operation
        );
        assert_eq!(
            row["direction"], "write",
            "direction of {}",
            expected.operation
        );
        assert_eq!(
            row["editable_kind"],
            expected.editable_kind.as_str(),
            "editable_kind of {}",
            expected.operation
        );
    }
}

#[test]
fn provider_write_excluded_writes_are_neither_catalogued_nor_reads() {
    let golden = golden();
    let excluded = golden["excludedWrites"]
        .as_array()
        .expect("excludedWrites is an array");
    assert!(
        !excluded.is_empty(),
        "the gated writes execution-core does not catalogue are named"
    );
    for write in excluded {
        let provider = write["provider"].as_str().expect("provider");
        let operation = write["operation"].as_str().expect("operation");
        assert!(
            !WRITE_OPERATIONS
                .iter()
                .any(|row| row.provider == provider && row.operation == operation),
            "{provider} {operation} is excluded, so it must not also be an operation"
        );
        assert!(
            !READ_OPERATIONS
                .iter()
                .any(|read| read.provider == provider && read.operation == operation),
            "{provider} {operation} is a write and must not be a read"
        );
    }
}

#[test]
fn provider_write_operations_are_never_listed_as_reads() {
    for write in WRITE_OPERATIONS {
        assert!(
            !READ_OPERATIONS
                .iter()
                .any(|read| read.provider == write.provider && read.operation == write.operation),
            "{} {} is a write and must not be a read",
            write.provider,
            write.operation
        );
    }
}

#[test]
fn provider_write_editable_kind_follows_the_table_and_its_aliases() {
    assert_eq!(
        editable_kind("google", "gmail.send"),
        Some(EditableKind::MailSend)
    );
    assert_eq!(
        editable_kind("google", " google.gmail.send "),
        Some(EditableKind::MailSend)
    );
    assert_eq!(
        editable_kind("microsoft", "mail.send"),
        Some(EditableKind::MailSend)
    );
    assert_eq!(
        editable_kind("meta", "pages.post"),
        Some(EditableKind::NotEditable)
    );
    assert_eq!(
        editable_kind("meta", "facebook.page.post"),
        Some(EditableKind::NotEditable)
    );
    // A read is not a write, and a write belongs to its own provider only.
    assert_eq!(editable_kind("microsoft", "mail.messages"), None);
    assert_eq!(editable_kind("google", "mail.send"), None);
    // A gated write that execution-core does not catalogue has no edit kind here.
    assert_eq!(editable_kind("meta", "live.create"), None);
}
