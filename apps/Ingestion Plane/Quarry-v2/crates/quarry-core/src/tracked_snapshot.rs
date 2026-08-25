//! PromoteTrackedResultToSnapshot — bridge between the versioned
//! change-history surface and the public resource surface.
//!
//! A completed change check produces a [`ChangeRecord`] keyed by
//! `baseline_id` (`bln_<ulid>`). Dashboards and list endpoints consume the
//! public [`Snapshot`] shape instead (`snap_<ulid>`,
//! `"unchanged" | "modified" | "new"`). This module projects one into the
//! other WITHOUT persisting anything — callers decide whether the promoted
//! view lands elsewhere or is served transiently.
//!
//! Two entry points share one projection core:
//! - [`tracked_result_to_snapshot`] — from a completed check result
//!   ([`ChangeRecord`]); status comes from the record.
//! - [`baseline_chain_to_snapshot`] — from a stored baseline plus the
//!   previous baseline's fingerprint (when the caller only has the
//!   persisted chain, e.g. `load_history` output); status is derived by
//!   comparing fingerprints.

use crate::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus};
use crate::error::{ErrorCode, QuarryError, QuarryResult};
use crate::ids::kinds;
use crate::resources::Snapshot;
use ulid::Ulid;

/// Project a completed check result into the public `Snapshot` wire shape.
///
/// Fails closed: an unreachable check (no captured baseline) or a
/// malformed `baseline_id` yields [`ErrorCode::BadRequest`] rather than a
/// fabricated snapshot.
pub fn tracked_result_to_snapshot(record: &ChangeRecord) -> QuarryResult<Snapshot> {
    let baseline = record.new_baseline.as_ref().ok_or_else(|| {
        QuarryError::new(
            ErrorCode::BadRequest,
            "an unreachable check captures no baseline and cannot be promoted to a snapshot",
        )
    })?;
    let word = status_word(record.status)?;
    let prev_fingerprint = record
        .prev_baseline
        .as_ref()
        .map(|p| p.fingerprint.clone());
    project(baseline, word, prev_fingerprint)
}

/// Project the newest link of a stored baseline chain into the public
/// `Snapshot` shape. `prev_fingerprint` is `None` for a first-ever
/// capture; equal fingerprints mean `unchanged`; anything else is
/// `modified`.
pub fn baseline_chain_to_snapshot(
    latest: &BaselineSnapshot,
    prev_fingerprint: Option<&str>,
) -> QuarryResult<Snapshot> {
    let word = match prev_fingerprint {
        None => "new",
        Some(prev) if prev == latest.fingerprint => "unchanged",
        Some(_) => "modified",
    };
    project(latest, word, prev_fingerprint.map(str::to_string))
}

fn status_word(status: ChangeStatus) -> QuarryResult<&'static str> {
    match status {
        ChangeStatus::New => Ok("new"),
        ChangeStatus::Unchanged => Ok("unchanged"),
        ChangeStatus::Changed => Ok("modified"),
        ChangeStatus::Unreachable => Err(QuarryError::new(
            ErrorCode::BadRequest,
            "an unreachable check captures no baseline and cannot be promoted to a snapshot",
        )),
    }
}

/// Shared projection core. Mapping:
/// - `snapshot_id` ← ULID portion of `baseline_id`, re-prefixed `snap_`.
/// - `source_id` stays `None`: baselines are URL-keyed; the durable Source
///   linkage lives in control plane and is not implied here.
fn project(
    baseline: &BaselineSnapshot,
    change_status: &str,
    prev_fingerprint: Option<String>,
) -> QuarryResult<Snapshot> {
    let raw_ulid = baseline
        .baseline_id
        .strip_prefix("bln_")
        .and_then(|rest| Ulid::from_string(rest).ok())
        .ok_or_else(|| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("baseline_id is not a bln_<ulid>: {}", baseline.baseline_id),
            )
        })?;
    Ok(Snapshot {
        snapshot_id: kinds::SnapshotKind::from_ulid(raw_ulid),
        org_id: baseline.org_id.clone(),
        source_id: None,
        url: baseline.source_url.clone(),
        fingerprint: baseline.fingerprint.clone(),
        prev_fingerprint,
        change_status: change_status.to_string(),
        captured_at: baseline.captured_at,
        artifact_id: baseline.artifact_id.clone(),
    })
}

/// Convenience alias mirroring the parity-plan naming.
pub use tracked_result_to_snapshot as promote_tracked_result_to_snapshot;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const GOOD_ULID: &str = "01ARZ3NDeKTSJMdNG7gZ6pvhgp";

    fn baseline(id: &str, fp: &str) -> BaselineSnapshot {
        BaselineSnapshot {
            baseline_id: id.to_string(),
            org_id: "org_a".to_string(),
            source_url: "https://example.com/pricing".to_string(),
            fingerprint: fp.to_string(),
            artifact_id: None,
            prev_baseline_id: None,
            captured_at: Utc::now(),
            run_id: None,
        }
    }

    fn record(
        status: ChangeStatus,
        new: Option<BaselineSnapshot>,
        prev_fp: Option<&str>,
    ) -> ChangeRecord {
        ChangeRecord {
            source_url: "https://example.com/pricing".to_string(),
            org_id: "org_a".to_string(),
            status,
            new_baseline: new,
            prev_baseline: prev_fp
                .map(|fp| baseline("bln_old0000000000000000000000", fp)),
            diff_id: None,
            checked_at: Utc::now(),
        }
    }

    #[test]
    fn chain_variant_derives_new_when_no_prev() {
        let b = baseline(&format!("bln_{GOOD_ULID}"), "blake3:first");
        let snap = baseline_chain_to_snapshot(&b, None).unwrap();
        assert_eq!(snap.change_status, "new");
        assert!(snap.prev_fingerprint.is_none());
    }

    #[test]
    fn chain_variant_derives_unchanged_and_modified_from_fingerprints() {
        let b = baseline(&format!("bln_{GOOD_ULID}"), "blake3:v2");
        let same = baseline_chain_to_snapshot(&b, Some("blake3:v2")).unwrap();
        assert_eq!(same.change_status, "unchanged");
        let changed = baseline_chain_to_snapshot(&b, Some("blake3:v1")).unwrap();
        assert_eq!(changed.change_status, "modified");
        assert_eq!(changed.prev_fingerprint.as_deref(), Some("blake3:v1"));
    }

    #[test]
    fn changed_record_promotes_to_modified_snapshot() {
        let rec = record(
            ChangeStatus::Changed,
            Some(baseline("bln_01ARZ3NDeKTSJMdNG7gZ6pvhgp", "blake3:new")),
            Some("blake3:old"),
        );
        let snap = tracked_result_to_snapshot(&rec).unwrap();
        assert_eq!(snap.change_status, "modified");
        assert_eq!(snap.fingerprint, "blake3:new");
        assert_eq!(snap.prev_fingerprint.as_deref(), Some("blake3:old"));
        assert!(snap.snapshot_id.to_string().starts_with("snap_"));
    }

    #[test]
    fn new_record_promotes_without_prev_fingerprint() {
        let rec = record(
            ChangeStatus::New,
            Some(baseline("bln_01ARZ3NDeKTSJMdNG7gZ6pvhgp", "blake3:first")),
            None,
        );
        let snap = tracked_result_to_snapshot(&rec).unwrap();
        assert_eq!(snap.change_status, "new");
        assert!(snap.prev_fingerprint.is_none());
    }

    #[test]
    fn unchanged_record_keeps_unchanged_status() {
        let rec = record(
            ChangeStatus::Unchanged,
            Some(baseline("bln_01ARZ3NDeKTSJMdNG7gZ6pvhgp", "blake3:same")),
            Some("blake3:same"),
        );
        let snap = tracked_result_to_snapshot(&rec).unwrap();
        assert_eq!(snap.change_status, "unchanged");
    }

    #[test]
    fn unreachable_check_cannot_be_promoted_fail_closed() {
        let mut rec = record(ChangeStatus::Unreachable, None, None);
        rec.new_baseline = None;
        let err = tracked_result_to_snapshot(&rec).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn malformed_baseline_id_is_rejected_not_silently_reminted() {
        let rec = record(
            ChangeStatus::Changed,
            Some(baseline("not-a-ulid", "blake3:x")),
            None,
        );
        let err = tracked_result_to_snapshot(&rec).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn snapshot_id_roundtrips_from_the_baseline_ulid() {
        let rec = record(
            ChangeStatus::Changed,
            Some(baseline("bln_01ARZ3NDeKTSJMdNG7gZ6pvhgp", "blake3:x")),
            None,
        );
        let snap = tracked_result_to_snapshot(&rec).unwrap();
        // Same ULID, different prefix — deterministic across calls.
        let again = tracked_result_to_snapshot(&rec).unwrap();
        assert_eq!(snap.snapshot_id, again.snapshot_id);
        // And it parses back as a typed SnapshotKind.
        let parsed: kinds::SnapshotKind = snap.snapshot_id.to_string().parse().unwrap();
        assert_eq!(parsed, snap.snapshot_id);
    }

    #[test]
    fn wire_shape_pins_public_snapshot_fields() {
        let rec = record(
            ChangeStatus::Changed,
            Some(baseline("bln_01ARZ3NDeKTSJMdNG7gZ6pvhgp", "blake3:x")),
            Some("blake3:y"),
        );
        let s = serde_json::to_string(&tracked_result_to_snapshot(&rec).unwrap()).unwrap();
        assert!(s.contains("\"snapshot_id\":\"snap_"));
        assert!(s.contains("\"change_status\":\"modified\""));
        assert!(s.contains("\"fingerprint\":\"blake3:x\""));
        assert!(s.contains("\"prev_fingerprint\":\"blake3:y\""));
        // source_id omitted when None.
        assert!(!s.contains("source_id"));
    }
}
