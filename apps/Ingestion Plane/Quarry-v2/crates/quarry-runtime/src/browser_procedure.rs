//! Deterministic browser-procedure compilation and replay checks.
//!
//! This is intentionally a compiler/validator, not a second planner. A
//! procedure can only be promoted from receipts whose observations prove the
//! action outcome; unknown effectful actions remain reviewable evidence but
//! cannot become an autonomous replay.

use serde::{Deserialize, Serialize};

use quarry_core::contracts::{ActionOutcomeStatus, AgentAction};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::step_receipts::{StepOutcome, StepReceipt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureStep {
    pub step: u32,
    pub action: AgentActionWire,
    pub expected_outcome: ActionOutcomeStatus,
}

/// JSON-backed action representation keeps this crate independent of a
/// particular action enum version while preserving an exact replay digest.
pub type AgentActionWire = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserProcedure {
    pub procedure_id: String,
    pub version: u32,
    pub source_receipt_ids: Vec<String>,
    pub steps: Vec<ProcedureStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureQualityReport {
    pub procedure_id: String,
    pub step_count: usize,
    pub verified_step_count: usize,
    pub promotion_allowed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureImpactReport {
    pub procedure_id: String,
    pub impacted_steps: Vec<u32>,
    pub quarantine_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayDecision {
    Exact,
    RepairRequired { step: u32, reason: String },
    Refused { step: u32, reason: String },
}

pub fn compile_procedure(
    procedure_id: impl Into<String>,
    receipts: &[StepReceipt],
) -> QuarryResult<BrowserProcedure> {
    if receipts.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "cannot compile a procedure from an empty receipt sequence",
        ));
    }
    let mut steps = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        if !matches!(&receipt.outcome, StepOutcome::Completed { .. }) {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!("receipt {} is not completed", receipt.receipt_id),
            ));
        }
        let Some(observation) = receipt.observation.as_ref() else {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!("receipt {} has no observation proof", receipt.receipt_id),
            ));
        };
        if observation.action_outcome.status != ActionOutcomeStatus::Verified {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!(
                    "receipt {} has an unverified business outcome",
                    receipt.receipt_id
                ),
            ));
        }
        let action = serde_json::to_value(&receipt.action).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("encode procedure action: {error}"),
            )
        })?;
        steps.push(ProcedureStep {
            step: receipt.step,
            action,
            expected_outcome: observation.action_outcome.status.clone(),
        });
    }
    Ok(BrowserProcedure {
        procedure_id: procedure_id.into(),
        version: 1,
        source_receipt_ids: receipts.iter().map(|r| r.receipt_id.clone()).collect(),
        steps,
    })
}

pub fn compare_replay(procedure: &BrowserProcedure, actions: &[AgentAction]) -> ReplayDecision {
    for (index, expected) in procedure.steps.iter().enumerate() {
        let Some(actual) = actions.get(index) else {
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "replay ended before the procedure completed".to_owned(),
            };
        };
        let actual = match serde_json::to_value(actual) {
            Ok(value) => value,
            Err(_) => {
                return ReplayDecision::Refused {
                    step: expected.step,
                    reason: "action could not be canonically encoded".to_owned(),
                }
            }
        };
        if actual != expected.action {
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "exact action changed; no silent business-action substitution".to_owned(),
            };
        }
    }
    if actions.len() > procedure.steps.len() {
        return ReplayDecision::RepairRequired {
            step: procedure
                .steps
                .last()
                .map(|step| step.step + 1)
                .unwrap_or(0),
            reason: "replay contains actions not present in the approved procedure".to_owned(),
        };
    }
    ReplayDecision::Exact
}

/// Deterministic promotion gate for a compiled candidate. This is a quality
/// report only; rollout state and business approval remain outside Quarry.
pub fn assess_quality(procedure: &BrowserProcedure) -> ProcedureQualityReport {
    let mut reasons = Vec::new();
    if procedure.steps.is_empty() {
        reasons.push("procedure has no steps".to_owned());
    }
    let mut previous = None;
    for step in &procedure.steps {
        if step.action.is_null() {
            reasons.push(format!("step {} has no canonical action", step.step));
        }
        if step.expected_outcome != ActionOutcomeStatus::Verified {
            reasons.push(format!("step {} is not verified", step.step));
        }
        if previous.is_some_and(|prior| step.step <= prior) {
            reasons.push("procedure steps are not strictly ordered".to_owned());
        }
        previous = Some(step.step);
    }
    let verified_step_count = procedure
        .steps
        .iter()
        .filter(|step| step.expected_outcome == ActionOutcomeStatus::Verified)
        .count();
    ProcedureQualityReport {
        procedure_id: procedure.procedure_id.clone(),
        step_count: procedure.steps.len(),
        verified_step_count,
        promotion_allowed: reasons.is_empty(),
        reasons,
    }
}

/// Identify deterministic procedure steps whose navigation source changed.
/// This is intentionally conservative: a matching URL quarantines the
/// candidate for review instead of attempting an automatic effectful repair.
pub fn analyze_impact(
    procedure: &BrowserProcedure,
    changed_urls: &[String],
) -> ProcedureImpactReport {
    let changed = changed_urls
        .iter()
        .filter_map(|url| url::Url::parse(url).ok().map(|url| url.to_string()))
        .collect::<std::collections::HashSet<_>>();
    let mut impacted_steps = Vec::new();
    for step in &procedure.steps {
        let Some(url) = step.action.get("url").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Ok(normalized) = url::Url::parse(url).map(|url| url.to_string()) else {
            continue;
        };
        if changed.contains(&normalized) {
            impacted_steps.push(step.step);
        }
    }
    let quarantine_required = !impacted_steps.is_empty();
    ProcedureImpactReport {
        procedure_id: procedure.procedure_id.clone(),
        impacted_steps,
        quarantine_required,
        reasons: quarantine_required
            .then(|| vec!["procedure navigation source changed; replay requires review".to_owned()])
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use quarry_core::contracts::{ActionOutcome, AgentAction, BrowserObservation};
    use quarry_core::ids::kinds::RunKind;
    use quarry_core::ids::Id;

    fn receipt(action: AgentAction, outcome: ActionOutcome) -> StepReceipt {
        let run_id: RunKind = Id::new();
        StepReceipt {
            org_id: Some("org".into()),
            actor_id: Some("user".into()),
            receipt_id: "rcpt_1".into(),
            run_id: run_id.to_string(),
            step: 0,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            action,
            outcome: StepOutcome::Completed { latency_ms: 1 },
            observation: Some(BrowserObservation {
                run_id: Id::new(),
                step: 0,
                url: "https://example.test".into(),
                title: None,
                dom_summary: None,
                screenshot_artifact_id: None,
                visual_observation_artifact_id: None,
                console_summary: vec![],
                network_summary: vec![],
                policy_denials: vec![],
                action_outcome: outcome,
                observation_delta: None,
                challenge: None,
                extraction_profile: None,
                extraction_result: None,
                proof_bundle: None,
                observed_at: Utc::now(),
            }),
            correction_of: None,
            cost_micro_usd: 0,
        }
    }

    #[test]
    fn only_verified_receipts_promote() {
        let action = AgentAction::Navigate {
            url: "https://example.test".into(),
        };
        let p = compile_procedure(
            "proc_test",
            &[receipt(
                action.clone(),
                ActionOutcome::verified("navigation_completed"),
            )],
        )
        .unwrap();
        assert_eq!(compare_replay(&p, &[action]), ReplayDecision::Exact);
    }

    #[test]
    fn unknown_effect_is_not_promoted() {
        let result = compile_procedure(
            "proc_test",
            &[receipt(
                AgentAction::Click {
                    selector: "#buy".into(),
                },
                ActionOutcome::unknown("postcondition_required", "not proved"),
            )],
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
    }

    #[test]
    fn quality_gate_rejects_unverified_or_unordered_steps() {
        let procedure = BrowserProcedure {
            procedure_id: "proc_test".into(),
            version: 1,
            source_receipt_ids: vec![],
            steps: vec![
                ProcedureStep {
                    step: 2,
                    action: serde_json::json!({"type": "click"}),
                    expected_outcome: ActionOutcomeStatus::Unknown,
                },
                ProcedureStep {
                    step: 1,
                    action: serde_json::json!({"type": "click"}),
                    expected_outcome: ActionOutcomeStatus::Verified,
                },
            ],
        };
        let report = assess_quality(&procedure);
        assert!(!report.promotion_allowed);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("not verified")));
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("ordered")));
    }

    #[test]
    fn changed_navigation_source_requires_quarantine() {
        let procedure = BrowserProcedure {
            procedure_id: "proc_test".into(),
            version: 1,
            source_receipt_ids: vec![],
            steps: vec![ProcedureStep {
                step: 0,
                action: serde_json::json!({
                    "type": "navigate",
                    "url": "https://example.test/path"
                }),
                expected_outcome: ActionOutcomeStatus::Verified,
            }],
        };
        let report = analyze_impact(&procedure, &["https://example.test/path".into()]);
        assert!(report.quarantine_required);
        assert_eq!(report.impacted_steps, vec![0]);
    }
}
