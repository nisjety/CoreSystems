//! Deterministic target-fingerprint scoring.
//!
//! This module only ranks read-only repair candidates. It never rewrites an
//! effectful browser action; callers must require an exact selector or an
//! explicit approval boundary before executing one.

use serde::{Deserialize, Serialize};

use quarry_core::contracts::ElementFingerprint;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairCandidate {
    pub fingerprint_id: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairDecision {
    Accepted,
    Rejected,
    RequiresApproval,
}

/// Evidence record for a target repair attempt. A receipt is deliberately
/// independent of the browser action itself: callers can persist the score
/// trace for read-only repair, while effectful actions still require an exact
/// approved target and must not be substituted from this record alone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetRepairReceipt {
    pub original_selector: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_selector: Option<String>,
    pub action_kind: String,
    pub candidates: Vec<RepairCandidate>,
    pub decision: RepairDecision,
    pub reason: String,
}

pub fn receipt(
    action_kind: impl Into<String>,
    original_selector: impl Into<String>,
    selected_selector: Option<String>,
    candidates: Vec<RepairCandidate>,
    decision: RepairDecision,
    reason: impl Into<String>,
) -> TargetRepairReceipt {
    TargetRepairReceipt {
        original_selector: original_selector.into(),
        selected_selector,
        action_kind: action_kind.into(),
        candidates,
        decision,
        reason: reason.into(),
    }
}

/// Weighted similarity used by extraction/read-only navigation repair.
/// Logical identity and tag carry more weight than layout path, which is
/// intentionally treated as a weak signal under wrapper/layout mutations.
pub fn score(expected: &ElementFingerprint, candidate: &ElementFingerprint) -> RepairCandidate {
    let mut total = 0.0;
    let mut reasons = Vec::new();
    if expected.tag.eq_ignore_ascii_case(&candidate.tag) {
        total += 0.20;
        reasons.push("tag".to_owned());
    }
    if !expected.normalized_text.is_empty() && expected.normalized_text == candidate.normalized_text
    {
        total += 0.30;
        reasons.push("text".to_owned());
    }
    if expected.logical_id.is_some() && expected.logical_id == candidate.logical_id {
        total += 0.30;
        reasons.push("logical_id".to_owned());
    }
    let shared_attributes = expected
        .attributes
        .iter()
        .filter(|attribute| candidate.attributes.contains(attribute))
        .count();
    let attribute_denominator = expected.attributes.len().max(1) as f32;
    if shared_attributes > 0 {
        total += 0.15 * (shared_attributes as f32 / attribute_denominator);
        reasons.push("attributes".to_owned());
    }
    if !expected.structural_path.is_empty() && expected.structural_path == candidate.structural_path
    {
        total += 0.05;
        reasons.push("path".to_owned());
    }
    RepairCandidate {
        fingerprint_id: candidate.fingerprint_id.clone(),
        score: total.min(1.0),
        reasons,
    }
}

/// Return candidates at or above the supplied threshold, strongest first.
pub fn rank(
    expected: &ElementFingerprint,
    candidates: impl IntoIterator<Item = ElementFingerprint>,
    threshold: f32,
) -> Vec<RepairCandidate> {
    let mut ranked = candidates
        .into_iter()
        .map(|candidate| score(expected, &candidate))
        .filter(|candidate| candidate.score >= threshold)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(id: &str, text: &str, logical_id: Option<&str>, path: &str) -> ElementFingerprint {
        ElementFingerprint {
            fingerprint_id: id.into(),
            tag: "button".into(),
            normalized_text: text.into(),
            attributes: vec![("role".into(), "button".into())],
            structural_path: path.into(),
            logical_id: logical_id.map(str::to_owned),
        }
    }

    #[test]
    fn ranks_same_semantic_target_above_layout_mutation() {
        let expected = fp("old", "approve invoice", Some("approve"), "button[2]");
        let repaired = fp("new", "approve invoice", Some("approve"), "button[9]");
        let dangerous = fp("danger", "approve and pay", Some("pay"), "button[2]");
        let ranked = rank(&expected, vec![dangerous, repaired], 0.70);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].fingerprint_id, "new");
        assert!(ranked[0].score >= 0.8);
    }

    #[test]
    fn no_candidate_below_threshold_is_auto_repaired() {
        let expected = fp("old", "approve invoice", Some("approve"), "button[2]");
        let candidate = fp("new", "approve and pay", Some("pay"), "button[9]");
        assert!(rank(&expected, vec![candidate], 0.70).is_empty());
    }

    #[test]
    fn repair_receipt_retains_candidate_trace_and_decision() {
        let expected = fp("old", "approve invoice", Some("approve"), "button[2]");
        let candidate = fp("new", "approve invoice", Some("approve"), "button[9]");
        let ranked = rank(&expected, vec![candidate], 0.70);
        let receipt = receipt(
            "click",
            "#old",
            Some("#new".into()),
            ranked,
            RepairDecision::RequiresApproval,
            "effectful target requires an explicit approval boundary",
        );
        assert_eq!(receipt.decision, RepairDecision::RequiresApproval);
        assert_eq!(receipt.candidates.len(), 1);
        assert!(receipt.candidates[0]
            .reasons
            .contains(&"logical_id".to_owned()));
    }
}
