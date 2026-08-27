//! Property tests (WS4) for the change-webhook wire contract.
//!
//! These pin the cross-plane invariants that unit tests sample only:
//! 1. `change_webhook_payload` is total and well-formed over arbitrary
//!    `Changed` records — the body always decodes back as a
//!    `ChangeRecord` carrying `subject`, `emitted_at`, and an `org_id`
//!    equal to the record's.
//! 2. Only `Changed` records emit; every other status stays silent.
//! 3. `sign_change_webhook` produces an HMAC that verifies over the
//!    exact wire bytes, for arbitrary org ids and secrets — and fails
//!    closed on blank org ids.

use proptest::prelude::*;
use quarry_core::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus};

fn arb_status() -> impl Strategy<Value = ChangeStatus> {
    prop_oneof![
        Just(ChangeStatus::New),
        Just(ChangeStatus::Unchanged),
        Just(ChangeStatus::Changed),
        Just(ChangeStatus::Unreachable),
    ]
}

fn arb_org_id() -> impl Strategy<Value = String> {
    // Org ids are opaque tenant handles on the wire; keep them printable,
    // non-empty, and free of whitespace so URL embedding stays canonical.
    "[a-z][a-z0-9-]{0,31}"
}

fn arb_url() -> impl Strategy<Value = String> {
    ("https", "[a-z0-9-]{1,20}", "[a-z]{2,6}", "/[a-z0-9/_.~-]{0,64}").prop_map(
        |(scheme, host, tld, path)| format!("{scheme}://{host}.{tld}{path}"),
    )
}

fn arb_record(status: ChangeStatus) -> impl Strategy<Value = ChangeRecord> {
    (arb_org_id(), arb_url(), "[a-zA-Z0-9:_-]{0,40}", any::<u64>()).prop_map(
        move |(org_id, source_url, fingerprint_suffix, seed)| {
            let now = chrono::Utc::now() + chrono::Duration::seconds(seed as i64 % 1_000_000);
            let diff_id = if matches!(status, ChangeStatus::Changed) {
                Some(format!("diff_{seed:016x}"))
            } else {
                None
            };
            ChangeRecord {
                source_url,
                org_id,
                status,
                new_baseline: Some(BaselineSnapshot {
                    baseline_id: format!("bln_{seed:032x}"),
                    org_id: "irrelevant".into(),
                    source_url: "https://baseline.example/".into(),
                    fingerprint: format!("blake3:{fingerprint_suffix}"),
                    artifact_id: None,
                    prev_baseline_id: None,
                    captured_at: now,
                    run_id: None,
                }),
                prev_baseline: None,
                diff_id,
                checked_at: now,
            }
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn changed_records_roundtrip_through_the_webhook_body(record in arb_record(ChangeStatus::Changed)) {
        let body = quarry_edge::change_webhook::change_webhook_payload(&record)
            .expect("Changed records always emit");
        prop_assert_eq!(&body["subject"], "quarry.change.detected");
        prop_assert!(body.get("emitted_at").is_some());
        // The Go receiver decodes the body DIRECTLY as ChangeRecord.
        let decoded: ChangeRecord =
            serde_json::from_value(body.clone()).expect("body must decode as ChangeRecord");
        prop_assert_eq!(&decoded.org_id, &record.org_id);
        prop_assert_eq!(&decoded.source_url, &record.source_url);
        prop_assert_eq!(decoded.status, ChangeStatus::Changed);
        // Body org_id must equal what will go into the query string.
        prop_assert_eq!(&body["org_id"], &serde_json::json!(record.org_id));
    }

    #[test]
    fn non_changed_statuses_never_emit(
        status in arb_status().prop_filter("only non-changed", |s| !matches!(s, ChangeStatus::Changed)),
        record in arb_record(ChangeStatus::Unchanged),
    ) {
        let mut rec = record;
        rec.status = status;
        prop_assert!(quarry_edge::change_webhook::change_webhook_payload(&rec).is_none());
    }

    #[test]
    fn signatures_verify_over_exact_wire_bytes(
        secret_hex in "[0-9a-f]{64}",
        record in arb_record(ChangeStatus::Changed),
    ) {
        // The producer always signs with the record's own org id, so derive
        // it from the record instead of an independent strategy.
        let org_id = record.org_id.clone();
        let signer = quarry_edge::internal_auth::InternalSigner::new(&secret_hex)
            .map_err(|_| TestCaseError::reject("bad secret strategy"))?;
        let payload = quarry_edge::change_webhook::change_webhook_payload(&record).unwrap();
        let (path_q, bytes, headers) =
            quarry_edge::change_webhook::sign_change_webhook(&signer, &record.org_id, &payload)
                .unwrap();

        prop_assert_eq!(&path_q, &format!("/v1/webhooks/change?org_id={org_id}"));
        let ts: i64 = headers.timestamp.parse().map_err(|_| TestCaseError::reject("ts parse"))?;
        let digest = quarry_edge::internal_auth::InternalSigner::body_digest(&bytes);
        let expected = signer.sign_raw("POST", &path_q, &digest, ts, &headers.nonce);
        prop_assert_eq!(
            quarry_edge::internal_auth::InternalSigner::header_value(&expected),
            headers.signature.clone()
        );
        // The signed bytes still decode to the same org the query names.
        let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        prop_assert_eq!(&decoded["org_id"], &serde_json::json!(record.org_id));
    }

    #[test]
    fn signing_fails_closed_on_blank_org_ids(
        blank in prop_oneof![Just(String::new()), Just("   ".to_string()), "\t\n\r {0,5}"],
        secret_hex in "[0-9a-f]{64}",
    ) {
        let signer = quarry_edge::internal_auth::InternalSigner::new(&secret_hex)
            .map_err(|_| TestCaseError::reject("bad secret strategy"))?;
        let err = quarry_edge::change_webhook::sign_change_webhook(
            &signer,
            &blank,
            &serde_json::json!({}),
        )
        .expect_err("blank org ids must be rejected");
        prop_assert_eq!(err.code, quarry_core::error::ErrorCode::BadRequest);
    }
}
