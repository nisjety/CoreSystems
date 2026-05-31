//! Wiki maintenance agent surfaces.
//!
//! Provides synchronous tool entry points the runtime loop can dispatch to:
//!   * `wiki_propose_edit` — emit a structured proposal envelope that the
//!     orchestrator submits to Data Plane WikiService.SubmitProposal.
//!   * `wiki_lint` — run cheap heuristics over a candidate page to surface
//!     contradiction / stale / orphan signals as proposal seeds.
//!
//! The actual durable write (`SubmitProposal` RPC) lives at the model-gateway
//! `/v1/wiki/proposals` route. This module produces deterministic envelopes
//! so the agent loop and tests can reason about wiki maintenance without
//! coupling tool execution to an async gRPC channel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ProposeEditInput {
    pub page_id: String,
    pub patch: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProposalEnvelope {
    pub proposal_id: String,
    pub page_id: String,
    pub patch: String,
    pub source_refs: Vec<String>,
    pub reason: String,
    pub run_id: String,
    pub org_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LintInput {
    pub page_id: String,
    pub content: String,
    #[serde(default)]
    pub backlink_count: i32,
    #[serde(default)]
    pub last_updated_days: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LintIssue {
    Contradiction { phrase: String },
    Stale { days: i32 },
    Orphan,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LintReport {
    pub page_id: String,
    pub issues: Vec<LintIssue>,
    /// True if `content` exceeded `MAX_LINT_CONTENT_BYTES` and was truncated
    /// before scanning. Stale and orphan checks are unaffected — they read
    /// metadata, not content.
    #[serde(default, skip_serializing_if = "is_false")]
    pub content_truncated: bool,
}

// serde's `skip_serializing_if` requires a `fn(&T) -> bool`, so the reference
// is mandated by the framework, not a style choice.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

const STALE_THRESHOLD_DAYS: i32 = 180;
/// Maximum byte length of `LintInput.content` scanned for contradiction
/// markers. Caps worst-case work per lint call so an attacker (or noisy
/// upstream) can't pin a worker thread by submitting a 100MB page body.
/// Larger inputs are truncated and `LintReport.content_truncated` is set
/// so the caller can decide whether to fan out a more careful scan.
pub const MAX_LINT_CONTENT_BYTES: usize = 64 * 1024;
const CONTRADICTION_MARKERS: &[&str] =
    &["however", "contradicts", "but in fact", "on the contrary"];

/// Build a wiki-edit [`ProposalEnvelope`] from `input`.
///
/// # Errors
/// Returns `Err` with a human-readable reason if `page_id` or `patch` is empty.
pub fn propose_edit(input: ProposeEditInput) -> Result<ProposalEnvelope, String> {
    if input.page_id.is_empty() {
        return Err("page_id required".to_owned());
    }
    if input.patch.is_empty() {
        return Err("patch required".to_owned());
    }
    Ok(ProposalEnvelope {
        proposal_id: mp_ids::new_ulid(),
        page_id: input.page_id,
        patch: input.patch,
        source_refs: input.source_refs,
        reason: input.reason,
        run_id: input.run_id.unwrap_or_default(),
        org_id: input.org_id.unwrap_or_default(),
        status: "pending".to_owned(),
    })
}

pub fn lint(input: LintInput) -> LintReport {
    let mut issues = Vec::new();

    let (scan_slice, content_truncated) = if input.content.len() > MAX_LINT_CONTENT_BYTES {
        // Truncate at a UTF-8 char boundary at or before the byte cap so we
        // never split a multi-byte sequence. `floor_char_boundary` is
        // unstable, so do it by hand.
        let mut cut = MAX_LINT_CONTENT_BYTES;
        while cut > 0 && !input.content.is_char_boundary(cut) {
            cut -= 1;
        }
        (&input.content[..cut], true)
    } else {
        (input.content.as_str(), false)
    };

    let lower = scan_slice.to_lowercase();
    for marker in CONTRADICTION_MARKERS {
        if lower.contains(marker) {
            issues.push(LintIssue::Contradiction {
                phrase: (*marker).to_owned(),
            });
        }
    }

    if input.last_updated_days > STALE_THRESHOLD_DAYS {
        issues.push(LintIssue::Stale {
            days: input.last_updated_days,
        });
    }

    if input.backlink_count == 0 {
        issues.push(LintIssue::Orphan);
    }

    LintReport {
        page_id: input.page_id,
        issues,
        content_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propose_edit_rejects_empty_page_id() {
        let err = propose_edit(ProposeEditInput {
            page_id: String::new(),
            patch: "x".into(),
            source_refs: vec![],
            reason: String::new(),
            run_id: None,
            org_id: None,
        })
        .unwrap_err();
        assert_eq!(err, "page_id required");
    }

    #[test]
    fn propose_edit_rejects_empty_patch() {
        let err = propose_edit(ProposeEditInput {
            page_id: "p1".into(),
            patch: String::new(),
            source_refs: vec![],
            reason: String::new(),
            run_id: None,
            org_id: None,
        })
        .unwrap_err();
        assert_eq!(err, "patch required");
    }

    #[test]
    fn propose_edit_emits_envelope_with_status_pending() {
        let env = propose_edit(ProposeEditInput {
            page_id: "p1".into(),
            patch: "diff".into(),
            source_refs: vec!["s1".into()],
            reason: "freshness".into(),
            run_id: Some("run-42".into()),
            org_id: Some("org-x".into()),
        })
        .unwrap();
        assert_eq!(env.page_id, "p1");
        assert_eq!(env.patch, "diff");
        assert_eq!(env.source_refs, vec!["s1".to_owned()]);
        assert_eq!(env.reason, "freshness");
        assert_eq!(env.run_id, "run-42");
        assert_eq!(env.org_id, "org-x");
        assert_eq!(env.status, "pending");
        assert!(!env.proposal_id.is_empty());
    }

    #[test]
    fn lint_flags_contradiction_marker() {
        let report = lint(LintInput {
            page_id: "p1".into(),
            content: "This is true. However, the data shows otherwise.".into(),
            backlink_count: 3,
            last_updated_days: 10,
        });
        assert_eq!(report.page_id, "p1");
        assert!(matches!(
            report.issues.first(),
            Some(LintIssue::Contradiction { phrase }) if phrase == "however"
        ));
    }

    #[test]
    fn lint_flags_stale_when_over_threshold() {
        let report = lint(LintInput {
            page_id: "p1".into(),
            content: "Some current content.".into(),
            backlink_count: 5,
            last_updated_days: 365,
        });
        assert!(report
            .issues
            .iter()
            .any(|i| matches!(i, LintIssue::Stale { days } if *days == 365)));
    }

    #[test]
    fn lint_flags_orphan_when_no_backlinks() {
        let report = lint(LintInput {
            page_id: "p1".into(),
            content: "Body".into(),
            backlink_count: 0,
            last_updated_days: 1,
        });
        assert!(report.issues.iter().any(|i| matches!(i, LintIssue::Orphan)));
    }

    #[test]
    fn lint_clean_page_has_no_issues() {
        let report = lint(LintInput {
            page_id: "p1".into(),
            content: "Plain factual content.".into(),
            backlink_count: 4,
            last_updated_days: 10,
        });
        assert!(report.issues.is_empty());
        assert!(!report.content_truncated);
    }

    #[test]
    fn lint_truncates_oversized_content_and_flags_it() {
        // Build content that's well past the cap but holds the marker
        // *only* in the truncated suffix — so a correct truncation skips
        // the marker, and we get content_truncated = true.
        let mut content = "x".repeat(MAX_LINT_CONTENT_BYTES + 1024);
        content.push_str(" however this should be skipped");
        let report = lint(LintInput {
            page_id: "p1".into(),
            content,
            backlink_count: 5,
            last_updated_days: 5,
        });
        assert!(report.content_truncated);
        assert!(
            !report
                .issues
                .iter()
                .any(|i| matches!(i, LintIssue::Contradiction { .. })),
            "marker in truncated suffix should not be flagged"
        );
    }

    #[test]
    fn lint_truncation_respects_utf8_char_boundaries() {
        // A multi-byte char ("é" = 2 bytes) crossing the cap must not be
        // split — we'd panic on slicing if the boundary check were buggy.
        let prefix_size = MAX_LINT_CONTENT_BYTES - 1;
        let mut content = "a".repeat(prefix_size);
        content.push('é'); // straddles the cap (1 byte before, 2 bytes total)
        let _report = lint(LintInput {
            page_id: "p1".into(),
            content,
            backlink_count: 1,
            last_updated_days: 1,
        });
        // No panic = success.
    }
}
