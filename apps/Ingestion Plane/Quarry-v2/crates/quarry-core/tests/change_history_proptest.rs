//! Property tests (WS4) for the change-tracking wire types.
//!
//! These pin cross-plane serde invariants that example-based tests
//! sample only: arbitrary `BaselineSnapshot` / `ChangeRecord` values
//! survive a JSON round-trip unchanged, optional fields stay absent on
//! the wire when `None` (so Go decoders never see explicit nulls), and
//! `ChangeStatus` keeps its pinned snake-case spelling.

use proptest::prelude::*;
use quarry_core::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus};

fn arb_org_id() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,31}"
}

fn arb_url() -> impl Strategy<Value = String> {
    ("https", "[a-z0-9-]{1,20}", "[a-z]{2,6}", "/[a-z0-9/_.~-]{0,64}").prop_map(
        |(scheme, host, tld, path)| format!("{scheme}://{host}.{tld}{path}"),
    )
}

fn arb_baseline(org: impl Strategy<Value = String>) -> impl Strategy<Value = BaselineSnapshot> {
    (org, arb_url(), "[a-zA-Z0-9:_-]{0,48}", any::<u64>()).prop_map(
        |(org_id, source_url, fingerprint_suffix, seed)| BaselineSnapshot {
            baseline_id: format!("bln_{seed:032x}"),
            org_id,
            source_url,
            fingerprint: format!("blake3:{fingerprint_suffix}"),
            artifact_id: None,
            prev_baseline_id: None,
            captured_at: chrono::Utc::now() + chrono::Duration::seconds(seed as i64 % 1_000_000),
            run_id: None,
        },
    )
}

fn arb_record(status: ChangeStatus) -> impl Strategy<Value = ChangeRecord> {
    (
        arb_org_id(),
        arb_url(),
        arb_baseline(arb_org_id()),
        prop::option::of("[a-z0-9_-]{1,32}"),
        any::<u64>(),
    )
        .prop_map(
            move |(org_id, source_url, new_baseline, prev_baseline_id, seed)| {
                let diff_id = if matches!(status, ChangeStatus::Changed) {
                    Some(format!("diff_{seed:016x}"))
                } else {
                    None
                };
                let prev_baseline = prev_baseline_id.map(|id| BaselineSnapshot {
                    baseline_id: format!("bln_{id}"),
                    org_id: org_id.clone(),
                    source_url: source_url.clone(),
                    fingerprint: format!("blake3:prev-{seed}"),
                    artifact_id: None,
                    prev_baseline_id: None,
                    captured_at: chrono::Utc::now(),
                    run_id: None,
                });
                ChangeRecord {
                    source_url,
                    org_id,
                    status,
                    new_baseline: Some(new_baseline),
                    prev_baseline,
                    diff_id,
                    checked_at: chrono::Utc::now(),
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn baselines_roundtrip_through_json(baseline in arb_baseline(arb_org_id())) {
        let json = serde_json::to_value(&baseline).unwrap();
        let back: BaselineSnapshot = serde_json::from_value(json).unwrap();
        prop_assert_eq!(back.baseline_id, baseline.baseline_id);
        prop_assert_eq!(back.org_id, baseline.org_id);
        prop_assert_eq!(back.source_url, baseline.source_url);
        prop_assert_eq!(back.fingerprint, baseline.fingerprint);
        prop_assert_eq!(back.captured_at, baseline.captured_at);
    }

    #[test]
    fn records_roundtrip_through_json(record in arb_record(ChangeStatus::Changed)) {
        let json = serde_json::to_value(&record).unwrap();
        let back: ChangeRecord = serde_json::from_value(json).unwrap();
        // Field-by-field: the wire structs do not derive PartialEq.
        prop_assert_eq!(&back.source_url, &record.source_url);
        prop_assert_eq!(&back.org_id, &record.org_id);
        prop_assert_eq!(back.status, record.status);
        prop_assert_eq!(&back.diff_id, &record.diff_id);
        prop_assert_eq!(back.checked_at, record.checked_at);
        let new_back = back.new_baseline.as_ref().unwrap();
        let new_orig = record.new_baseline.as_ref().unwrap();
        prop_assert_eq!(&new_back.baseline_id, &new_orig.baseline_id);
        prop_assert_eq!(&new_back.fingerprint, &new_orig.fingerprint);
        if let (Some(prev), Some(prev_orig)) = (&back.prev_baseline, &record.prev_baseline) {
            prop_assert_eq!(&prev.baseline_id, &prev_orig.baseline_id);
            prop_assert_eq!(&prev.org_id, &prev_orig.org_id);
        } else {
            prop_assert!(back.prev_baseline.is_none() && record.prev_baseline.is_none());
        }
    }

    #[test]
    fn none_optionals_are_absent_not_null_on_the_wire(
        record in arb_record(ChangeStatus::Unchanged),
    ) {
        let mut rec = record;
        rec.prev_baseline = None;
        rec.diff_id = None;
        let json = serde_json::to_value(&rec).unwrap();
        prop_assert!(!json.as_object().unwrap().contains_key("prev_baseline"));
        prop_assert!(!json.as_object().unwrap().contains_key("diff_id"));
        // And it still decodes.
        let back: ChangeRecord = serde_json::from_value(json).unwrap();
        prop_assert_eq!(back.status, ChangeStatus::Unchanged);
    }

    #[test]
    fn status_json_is_pinned_snake_case(status in prop_oneof![
        Just(ChangeStatus::New),
        Just(ChangeStatus::Unchanged),
        Just(ChangeStatus::Changed),
        Just(ChangeStatus::Unreachable),
    ]) {
        let json = serde_json::to_string(&status).unwrap();
        let expected = match status {
            ChangeStatus::New => "\"new\"",
            ChangeStatus::Unchanged => "\"unchanged\"",
            ChangeStatus::Changed => "\"changed\"",
            ChangeStatus::Unreachable => "\"unreachable\"",
        };
        prop_assert_eq!(json, expected);
    }
}
