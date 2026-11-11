//! Deterministic browser-procedure compilation and replay checks.
//!
//! This is intentionally a compiler/validator, not a second planner. A
//! procedure can only be promoted from receipts whose observations prove the
//! action outcome; unknown effectful actions remain reviewable evidence but
//! cannot become an autonomous replay.

use serde::{Deserialize, Serialize};

use quarry_core::contracts::{
    ActionOutcomeStatus, AgentAction, BrowserObservation, ProofBundle, SemanticLocator,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::step_expect::{ExpectVerdict, StepExpectation};

use crate::step_receipts::{StepOutcome, StepReceipt};

/// The switch that turns canonical (version 2) procedures on. The edge checks it
/// with [`procedure_canonical_enabled`] before it compiles or replays one.
pub const PROCEDURE_CANONICAL_FLAG: &str = "QUARRY_EDGE__PROCEDURE_CANONICAL";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureStep {
    pub step: u32,
    pub action: AgentActionWire,
    pub expected_outcome: ActionOutcomeStatus,
    /// The action carries a redaction marker: what was typed or selected was
    /// masked when the receipt was written, so the replay must supply it. Such a
    /// step is never reported as an exact replay (WS12.S3). In a version 2
    /// procedure a typed value is a slot the replay supplies, so this flag only
    /// records the mask there; a masked step outside a slot is still never exact.
    #[serde(default)]
    pub redacted_input: bool,
    /// The semantic target a ref or semantic step acts on. Version 2 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<StepTarget>,
    /// The name of the slot this step's typed text is bound to. Version 2 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// The step's postcondition, carried only when Quarry verified it as met.
    /// Version 2 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<StepExpectation>,
}

/// The element a semantic step acts on: the exact locator Quarry resolved it to,
/// and the fingerprint identity it matched. The fingerprint's normalized text is
/// never copied, because it can carry page content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepTarget {
    /// The semantic locator the step replays against.
    pub locator: SemanticLocator,
    /// The identity of the fingerprint Quarry matched when it resolved the target.
    pub fingerprint_id: String,
    /// The HTML tag of the matched element.
    pub tag: String,
}

/// Whether a slot's bound value is secret. A secret slot is never shown or logged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    /// Ordinary typed text, such as a search term.
    Text,
    /// A password, secret or one-time code: the captured text carried a marker.
    Secret,
}

/// A named input a version 2 procedure leaves open. The step types into it at
/// replay; the value is supplied then and is never stored in the procedure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureSlot {
    /// The slot's name, `slot_<step>`, unique within the procedure.
    pub name: String,
    /// The step number that types into the slot.
    pub step: u32,
    /// Whether the value a replay supplies is secret.
    pub kind: SlotKind,
}

/// Does this action JSON carry a redaction marker (`[redacted:password]`, or the
/// `[redacted]` of a metadata-only receipt) in any string?
fn carries_redaction_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains("[redacted:") || s == "[redacted]",
        serde_json::Value::Array(items) => items.iter().any(carries_redaction_marker),
        serde_json::Value::Object(map) => map.values().any(carries_redaction_marker),
        _ => false,
    }
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
    /// The typed-input slots of a version 2 procedure. Empty for version 1, and
    /// omitted from the wire then, so a stored version 1 procedure keeps its shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<ProcedureSlot>,
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
        let observation = verified_observation(receipt)?;
        let action = serde_json::to_value(&receipt.action).map_err(encode_error)?;
        let redacted_input = carries_redaction_marker(&action);
        steps.push(ProcedureStep {
            step: receipt.step,
            action,
            expected_outcome: observation.action_outcome.status.clone(),
            redacted_input,
            target: None,
            slot: None,
            expect: None,
        });
    }
    Ok(BrowserProcedure {
        procedure_id: procedure_id.into(),
        version: 1,
        source_receipt_ids: receipts.iter().map(|r| r.receipt_id.clone()).collect(),
        steps,
        slots: Vec::new(),
    })
}

/// The observation a receipt must carry before it can be compiled: the step
/// completed, an observation proves it, and the business outcome was verified.
/// Any other receipt is a conflict, so an unverified effect is never promoted.
fn verified_observation(receipt: &StepReceipt) -> QuarryResult<&BrowserObservation> {
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
    Ok(observation)
}

/// Encodes a receipt's action for a procedure. The message names the failure and
/// never the action.
fn encode_error(error: serde_json::Error) -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        format!("encode procedure action: {error}"),
    )
}

/// Whether `QUARRY_EDGE__PROCEDURE_CANONICAL` is on in this process (`1`, `true`,
/// `yes`, `on`). Read per call; default off.
pub fn procedure_canonical_enabled() -> bool {
    std::env::var(PROCEDURE_CANONICAL_FLAG)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Compiles a version 2 (canonical) procedure from verified receipts. The receipt
/// rules are those of [`compile_procedure`]. A ref becomes a semantic step bound to
/// the locator and fingerprint Quarry resolved. Typed text becomes a named slot
/// whose stored value is empty. A key or option value carrying a redaction marker is
/// refused. Only the closed vocabulary is kept: every other action is refused as
/// `UNSUPPORTED`, so an approval grant or a typed value never reaches a recipe.
pub fn compile_procedure_v2(
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
    let mut slots = Vec::new();
    for receipt in receipts {
        let observation = verified_observation(receipt)?;
        let proof = observation.proof_bundle.as_ref();
        let original = serde_json::to_value(&receipt.action).map_err(encode_error)?;
        let compiled = compile_v2_action(receipt.step, &receipt.action, proof)?;
        let action = serde_json::to_value(&compiled.action).map_err(encode_error)?;
        let slot = compiled.slot.map(|kind| ProcedureSlot {
            name: format!("slot_{}", receipt.step),
            step: receipt.step,
            kind,
        });
        steps.push(ProcedureStep {
            step: receipt.step,
            action,
            expected_outcome: observation.action_outcome.status.clone(),
            redacted_input: carries_redaction_marker(&original),
            target: compiled.target,
            slot: slot.as_ref().map(|slot| slot.name.clone()),
            expect: proof
                .and_then(|proof| proof.expectation.as_ref())
                .filter(|report| report.verdict == ExpectVerdict::Met)
                .map(|report| report.expect.clone()),
        });
        slots.extend(slot);
    }
    Ok(BrowserProcedure {
        procedure_id: procedure_id.into(),
        version: 2,
        source_receipt_ids: receipts.iter().map(|r| r.receipt_id.clone()).collect(),
        steps,
        slots,
    })
}

/// One receipt's action under the version 2 rules, before it is encoded.
struct CompiledAction {
    /// The action as stored. A semantic form has an empty snapshot and generation 0,
    /// and typed text is empty.
    action: AgentAction,
    /// The semantic target of a ref or semantic step.
    target: Option<StepTarget>,
    /// The kind of slot the step's typed text became, when it typed.
    slot: Option<SlotKind>,
}

/// Maps a receipt's action onto the closed version 2 vocabulary. Any action outside
/// it is refused as `UNSUPPORTED`, so a new action type is refused by default.
fn compile_v2_action(
    step: u32,
    action: &AgentAction,
    proof: Option<&ProofBundle>,
) -> QuarryResult<CompiledAction> {
    let kept = |action: AgentAction| CompiledAction {
        action,
        target: None,
        slot: None,
    };
    match action {
        AgentAction::Navigate { .. }
        | AgentAction::Click { .. }
        | AgentAction::Scroll { .. }
        | AgentAction::Wait { .. }
        | AgentAction::Back
        | AgentAction::GetContent => Ok(kept(action.clone())),
        AgentAction::Press { key } => {
            refuse_marked_value(step, key, "key")?;
            Ok(kept(action.clone()))
        }
        AgentAction::Type { selector, text } => Ok(CompiledAction {
            action: AgentAction::Type {
                selector: selector.clone(),
                text: String::new(),
            },
            target: None,
            slot: Some(slot_kind_for(text)),
        }),
        AgentAction::ClickRef { .. } | AgentAction::ClickSemantic { .. } => {
            let target = semantic_target(step, proof)?;
            Ok(CompiledAction {
                action: AgentAction::ClickSemantic {
                    snapshot_id: String::new(),
                    generation: 0,
                    locator: target.locator.clone(),
                },
                target: Some(target),
                slot: None,
            })
        }
        AgentAction::TypeRef { text, .. } | AgentAction::TypeSemantic { text, .. } => {
            let target = semantic_target(step, proof)?;
            Ok(CompiledAction {
                action: AgentAction::TypeSemantic {
                    snapshot_id: String::new(),
                    generation: 0,
                    locator: target.locator.clone(),
                    text: String::new(),
                },
                target: Some(target),
                slot: Some(slot_kind_for(text)),
            })
        }
        AgentAction::SelectRef { value, .. } | AgentAction::SelectSemantic { value, .. } => {
            refuse_marked_value(step, value, "option value")?;
            let target = semantic_target(step, proof)?;
            Ok(CompiledAction {
                action: AgentAction::SelectSemantic {
                    snapshot_id: String::new(),
                    generation: 0,
                    locator: target.locator.clone(),
                    value: value.clone(),
                },
                target: Some(target),
                slot: None,
            })
        }
        AgentAction::WaitForRef { timeout_ms, .. }
        | AgentAction::WaitForSemantic { timeout_ms, .. } => {
            let target = semantic_target(step, proof)?;
            Ok(CompiledAction {
                action: AgentAction::WaitForSemantic {
                    snapshot_id: String::new(),
                    generation: 0,
                    locator: target.locator.clone(),
                    timeout_ms: *timeout_ms,
                },
                target: Some(target),
                slot: None,
            })
        }
        other => Err(QuarryError::unsupported_action(&action_kind(other))
            .with_details(serde_json::json!({ "step": step }))),
    }
}

/// The wire type of an action (`evaluate`, `upload_ref`, ...), used in a refusal. It
/// never carries the action's arguments.
fn action_kind(action: &AgentAction) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Refuses a key or option value that carries a redaction marker. The value was
/// masked when the receipt was written, so no recipe could reproduce it. The message
/// names the field and never the value.
fn refuse_marked_value(step: u32, value: &str, field: &str) -> QuarryResult<()> {
    if !value.contains("[redacted") {
        return Ok(());
    }
    Err(QuarryError::new(
        ErrorCode::Unsupported,
        format!("step {step} carries a redaction marker in its {field}; it cannot be recorded"),
    )
    .with_details(serde_json::json!({ "step": step, "field": field })))
}

/// The slot typed text binds under: `Secret` when the captured text carried a
/// password, one-time-code, card, secret, token, key or credential marker (every
/// `quarry_core::redaction` marker kind except phone), and `Text` otherwise. A
/// replay prompts for a secret slot as a masked input, so a card number or token
/// never shows in the clear.
fn slot_kind_for(text: &str) -> SlotKind {
    const SECRET_MARKERS: [&str; 7] = [
        "[redacted:password]",
        "[redacted:otp]",
        "[redacted:card]",
        "[redacted:secret]",
        "[redacted:token]",
        "[redacted:key]",
        "[redacted:credential]",
    ];
    if SECRET_MARKERS.iter().any(|marker| text.contains(marker)) {
        SlotKind::Secret
    } else {
        SlotKind::Text
    }
}

/// The target of a semantic step: the locator Quarry resolved the element to and the
/// fingerprint identity it matched. The canonical locator is preferred, then the
/// proof's own locator. A positional locator, or one that could carry personal data,
/// is never stored. Without a usable locator the step fails closed.
fn semantic_target(step: u32, proof: Option<&ProofBundle>) -> QuarryResult<StepTarget> {
    let resolution = proof
        .and_then(|proof| proof.target_resolution.as_ref())
        .ok_or_else(|| target_repair_required(step))?;
    let locator = resolution
        .canonical_locator
        .iter()
        .chain(resolution.locator.iter())
        .find(|locator| locator_is_storable(locator))
        .cloned()
        .ok_or_else(|| target_repair_required(step))?;
    Ok(StepTarget {
        locator,
        fingerprint_id: resolution.fingerprint.fingerprint_id.clone(),
        tag: resolution.fingerprint.tag.clone(),
    })
}

/// The refusal for a semantic step with no usable locator. Its details name the step.
fn target_repair_required(step: u32) -> QuarryError {
    QuarryError::new(
        ErrorCode::TargetRepairRequired,
        "the step has no stable semantic target; observe the page again and record the step",
    )
    .with_details(serde_json::json!({ "step": step }))
}

/// Whether a locator may be stored in a recipe: never positional (`nth`), and every
/// value it names passes [`storable_text`]. These are the rules the observation
/// layer applies to a canonical locator.
fn locator_is_storable(locator: &SemanticLocator) -> bool {
    match locator {
        SemanticLocator::Role { role, name, .. } => {
            storable_text(role) && name.as_deref().into_iter().all(storable_text)
        }
        SemanticLocator::Text { text, .. } => storable_text(text),
        SemanticLocator::Label { label, .. } => storable_text(label),
        SemanticLocator::Placeholder { placeholder, .. } => storable_text(placeholder),
        SemanticLocator::TestId { test_id, .. } => storable_text(test_id),
        SemanticLocator::Nth { .. } => false,
    }
}

/// Longest page-authored locator value a recipe stores, in bytes. The observation
/// layer uses the same cap for a canonical locator.
const STORED_LOCATOR_MAX_BYTES: usize = 120;

/// One locator value: non-empty, at most [`STORED_LOCATOR_MAX_BYTES`], with no
/// redaction marker, no `@` and no run of six digits.
fn storable_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= STORED_LOCATOR_MAX_BYTES
        && !value.to_ascii_lowercase().contains("[redacted")
        && !value.contains('@')
        && !has_digit_run(value, 6)
}

/// Whether `value` holds `run` ASCII digits in a row.
fn has_digit_run(value: &str, run: usize) -> bool {
    let mut digits = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            if digits >= run {
                return true;
            }
        } else {
            digits = 0;
        }
    }
    false
}

pub fn compare_replay(procedure: &BrowserProcedure, actions: &[AgentAction]) -> ReplayDecision {
    if procedure.version >= 2 {
        return compare_replay_canonical(procedure, actions, false);
    }
    for (index, expected) in procedure.steps.iter().enumerate() {
        let Some(actual) = actions.get(index) else {
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "replay ended before the procedure completed".to_owned(),
            };
        };
        if expected.redacted_input {
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "typed input was redacted at capture; the replay must supply it"
                    .to_owned(),
            };
        }
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

/// Compares a replay with a version 2 procedure. The action at each position must
/// match its stored step after two sets of fields are set aside: a `*_semantic`
/// action drops its `snapshot_id` and `generation`, which are handles scoped to one
/// run, and a slot step drops its `text`, which the replay binds. Every other field
/// must match exactly. With `prefix`, the replay may stop early, so its actions may
/// be a leading part of the procedure. Actions beyond the procedure are always
/// refused.
pub fn compare_replay_canonical(
    procedure: &BrowserProcedure,
    actions: &[AgentAction],
    prefix: bool,
) -> ReplayDecision {
    for (index, expected) in procedure.steps.iter().enumerate() {
        let Some(actual) = actions.get(index) else {
            if prefix {
                break;
            }
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "replay ended before the procedure completed".to_owned(),
            };
        };
        if expected.redacted_input && expected.slot.is_none() {
            return ReplayDecision::RepairRequired {
                step: expected.step,
                reason: "redacted at capture; the replay cannot reproduce it".to_owned(),
            };
        }
        let actual = match serde_json::to_value(actual) {
            Ok(value) => value,
            Err(_) => {
                return ReplayDecision::Refused {
                    step: expected.step,
                    reason: "action could not be canonically encoded".to_owned(),
                }
            }
        };
        let slot = expected.slot.is_some();
        if canonical_json(&actual, slot) != canonical_json(&expected.action, slot) {
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

/// The wire types of the semantic forms a version 2 procedure holds.
const SEMANTIC_KINDS: [&str; 4] = [
    "click_semantic",
    "type_semantic",
    "select_semantic",
    "wait_for_semantic",
];

/// Whether an action JSON is one of the semantic forms.
fn is_semantic_action(action: &serde_json::Value) -> bool {
    action
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| SEMANTIC_KINDS.contains(&kind))
}

/// The form of one action that a canonical comparison checks: the JSON without the
/// run-scoped handles of a semantic action, and without the `text` of a slot step.
fn canonical_json(action: &serde_json::Value, slot: bool) -> serde_json::Value {
    let mut value = action.clone();
    let semantic = is_semantic_action(&value);
    if let Some(map) = value.as_object_mut() {
        if semantic {
            map.remove("snapshot_id");
            map.remove("generation");
        }
        if slot {
            map.remove("text");
        }
    }
    value
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
    // A version 2 semantic step must name its target, since a replay resolves its
    // locator. Version 1 has no targets, so its verdict does not change.
    if procedure.version >= 2 {
        for step in &procedure.steps {
            if is_semantic_action(&step.action) && step.target.is_none() {
                reasons.push(format!("step {} has no target locator", step.step));
            }
        }
    }
    let mut slot_names = std::collections::HashSet::new();
    for slot in &procedure.slots {
        if !slot_names.insert(slot.name.as_str()) {
            reasons.push(format!("slot {} is not unique", slot.name));
        }
        let referenced = procedure
            .steps
            .iter()
            .filter(|step| step.slot.as_deref() == Some(slot.name.as_str()))
            .count();
        if referenced != 1 {
            reasons.push(format!(
                "slot {} is referenced by {referenced} steps, not one",
                slot.name
            ));
        }
    }
    for step in &procedure.steps {
        let Some(name) = step.slot.as_deref() else {
            continue;
        };
        if !procedure.slots.iter().any(|slot| slot.name == name) {
            reasons.push(format!("step {} binds an undeclared slot", step.step));
        }
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
        reasons: if quarantine_required {
            vec!["procedure navigation source changed; replay requires review".to_owned()]
        } else {
            Vec::new()
        },
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
            cost_source: None,
            redaction_rule_version: None,
            tab_id: None,
            attach_id: None,
            observation: Some(BrowserObservation {
                run_id: Id::new(),
                step: 0,
                url: "https://example.test".into(),
                title: None,
                snapshot: None,
                dom_summary: None,
                screenshot_artifact_id: None,
                visual_observation_artifact_id: None,
                pdf_artifact_id: None,
                evidence_delta_artifact_id: None,
                console_summary: vec![],
                network_summary: vec![],
                egress_receipts: vec![],
                dialogs: vec![],
                policy_denials: vec![],
                action_outcome: outcome,
                observation_delta: None,
                challenge: None,
                extraction_profile: None,
                extraction_result: None,
                proof_bundle: None,
                target_resolution: None,
                telemetry: quarry_core::contracts::BrowserTelemetry::default(),
                redaction: None,
                warnings: Vec::new(),
                settle_ms: None,
                observed_at: Utc::now(),
            }),
            correction_of: None,
            cost_micro_usd: 0,
            initiated_by: None,
            control_epoch: None,
            agent: None,
        }
    }

    #[test]
    fn compile_procedure_flags_redacted_input() {
        let typed = AgentAction::TypeRef {
            snapshot_id: "snap_x".into(),
            generation: 1,
            ref_id: "@e1".into(),
            text: "[redacted:password]".into(),
        };
        let click = AgentAction::Click {
            selector: "#go".into(),
        };
        let p = compile_procedure(
            "proc_test",
            &[
                receipt(typed, ActionOutcome::verified("typed")),
                receipt(click, ActionOutcome::verified("clicked")),
            ],
        )
        .unwrap();
        assert!(p.steps[0].redacted_input);
        assert!(!p.steps[1].redacted_input);
    }

    #[test]
    fn compare_replay_returns_repair_required_for_redacted_input() {
        let typed = AgentAction::TypeRef {
            snapshot_id: "snap_x".into(),
            generation: 1,
            ref_id: "@e1".into(),
            text: "[redacted:password]".into(),
        };
        let p = compile_procedure(
            "proc_test",
            &[receipt(typed.clone(), ActionOutcome::verified("typed"))],
        )
        .unwrap();
        // Even the very same marker text is not an exact replay: nothing real was typed.
        match compare_replay(&p, &[typed]) {
            ReplayDecision::RepairRequired { step, reason } => {
                assert_eq!(step, 0);
                assert!(reason.contains("redacted at capture"));
            }
            other => panic!("expected RepairRequired, got {other:?}"),
        }
    }

    #[test]
    fn a_procedure_stored_before_the_flag_existed_still_reads() {
        let old = serde_json::json!({
            "step": 3,
            "action": {"type": "click", "selector": "#go"},
            "expected_outcome": "verified"
        });
        let step: ProcedureStep = serde_json::from_value(old).unwrap();
        assert!(!step.redacted_input);
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
                    redacted_input: false,
                    target: None,
                    slot: None,
                    expect: None,
                },
                ProcedureStep {
                    step: 1,
                    action: serde_json::json!({"type": "click"}),
                    expected_outcome: ActionOutcomeStatus::Verified,
                    redacted_input: false,
                    target: None,
                    slot: None,
                    expect: None,
                },
            ],
            slots: vec![],
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
                redacted_input: false,
                target: None,
                slot: None,
                expect: None,
            }],
            slots: vec![],
        };
        let report = analyze_impact(&procedure, &["https://example.test/path".into()]);
        assert!(report.quarantine_required);
        assert_eq!(report.impacted_steps, vec![0]);
    }
}
