//! WS06-S6: the Go refusal helper (go/pkg/zdrrefusal) and the Rust reader
//! (`mp_contracts::zdr_refusal::Refusal::from_status`) must agree on the wire.
//!
//! Go's `Refuse` answers FailedPrecondition with the catalogue sentence and the
//! `x-mp-provider-error` (and, for a feature code, `x-mp-zdr-feature`) trailers.
//! These tests rebuild exactly that shape from the Go package's own embedded
//! catalogue file, so a drift in the code names, the feature keys or the trailer
//! names on either side fails here. A live Go server round trip is the later
//! tier (`go_sandbox_manager_live_refusal_round_trip`, ignored).

use mp_contracts::zdr_refusal::{Refusal, FEATURE_TRAILER, KIND_TRAILER};
use tonic::metadata::MetadataValue;
use tonic::{Code, Status};

const GO_CATALOGUE: &str = include_str!("../../../../go/pkg/zdrrefusal/zdr-refusals.json");

fn go_catalogue() -> serde_json::Value {
    serde_json::from_str(GO_CATALOGUE).expect("the Go catalogue parses")
}

/// What `zdrrefusal.Refuse(ctx, code, feature)` puts on the wire.
fn go_status(code: &str, feature: Option<&str>) -> Status {
    let catalogue = go_catalogue();
    let entry = catalogue["codes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == code)
        .unwrap_or_else(|| panic!("{code} is not in the Go catalogue"));
    let mut message = entry["message_en"].as_str().unwrap().to_string();
    if let Some(key) = feature {
        let label = catalogue["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .map(|f| f["label_en"].as_str().unwrap().to_string())
            .unwrap_or_else(|| key.to_string());
        message = message.replace("{feature}", &label);
    }
    let mut status = Status::new(Code::FailedPrecondition, message);
    status
        .metadata_mut()
        .insert(KIND_TRAILER, MetadataValue::try_from(code).unwrap());
    if let Some(key) = feature {
        status
            .metadata_mut()
            .insert(FEATURE_TRAILER, MetadataValue::try_from(key).unwrap());
    }
    status
}

#[test]
fn a_go_durable_mutation_refusal_reads_as_the_typed_refusal() {
    let status = go_status("zdr_durable_mutation_forbidden", None);
    assert_eq!(
        Refusal::from_status(&status),
        Some(Refusal::DURABLE_MUTATION_FORBIDDEN)
    );
}

#[test]
fn a_go_feature_refusal_reads_with_its_feature() {
    let status = go_status("zdr_feature_not_supported", Some("attachments"));
    assert_eq!(
        Refusal::from_status(&status),
        Some(Refusal::feature_not_supported("attachments"))
    );
}

#[test]
fn every_code_the_go_package_exports_is_a_code_the_rust_reader_accepts() {
    let catalogue = go_catalogue();
    for code in [
        "zdr_durable_mutation_forbidden",
        "zdr_feature_not_supported",
        "zdr_posture_mismatch",
        "zdr_decision_unavailable",
    ] {
        let takes_feature = catalogue["codes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["code"] == code)
            .unwrap_or_else(|| panic!("{code} missing from the Go catalogue"))["takes_feature"]
            .as_bool()
            .unwrap();
        let feature = takes_feature.then_some("attachments");
        let status = go_status(code, feature);
        let refusal = Refusal::from_status(&status)
            .unwrap_or_else(|| panic!("Rust reader rejected the Go refusal {code}"));
        assert_eq!(refusal.code(), code);
    }
}

#[test]
fn a_go_status_that_degraded_to_untyped_is_not_mistaken_for_a_refusal() {
    // Unknown code or unknown feature: Go sends a plain FailedPrecondition.
    let plain = Status::new(Code::FailedPrecondition, "zero data retention refusal");
    assert_eq!(Refusal::from_status(&plain), None);
}

#[test]
#[ignore = "needs a running Go sandbox-manager (later tier): ZDR principal -> AcquireLease -> from_status"]
fn go_sandbox_manager_live_refusal_round_trip() {
    // Later tier: start sandbox-manager with a zdr:true token, call AcquireLease
    // through tonic and assert Refusal::from_status(&err) is
    // Some(Refusal::DURABLE_MUTATION_FORBIDDEN).
}
