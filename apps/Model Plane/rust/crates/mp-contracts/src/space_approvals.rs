//! Addressee routing and member decide for Space approvals (build plan WS10-S6).
//!
//! An approval raised in a Space run is addressed to someone: the requester (the run owner, the
//! default and the only form before this slice), one named member, any editor, or any manager.
//! A member who is not the run owner decides it with a Control-signed `model.approval.decide`
//! decision; the approved action still runs as the requester (the approver is recorded as
//! `decided_by`, never as the actor). Moving a pending approval to another addressee needs a
//! `model.approval.reroute` decision. The rollout switch below is a control, never an
//! authorization input: Control's signed decision and Session Core's verifier decide.
//!
//! # Payload digests (Control's twins must produce the same bytes)
//!
//! SHA-256 over a header, then each field as `name \0 uint64be(len(value)) value`; the result is
//! `sha256:` + lowercase hex.
//!
//! `model.approval.decide`: header `model.approval.decide\0v1\0`; fields in order `org_id`,
//! `user_id` (the CALLER, the approver), `approval_id`, `space_id`, `space_decision_ref`,
//! `decision` (`granted` or `denied`), `scope` (`once`, `thread` or `run`), `action_schema_hash`.
//! The decision carries exactly the permission `approval:decide`.
//!
//! `model.approval.reroute`: header `model.approval.reroute\0v1\0`; fields in order `org_id`,
//! `user_id` (the caller), `approval_id`, `space_id`, `space_decision_ref`, `addressee_kind`
//! (`requester`, `named`, `any_editor` or `any_manager`), `addressee_subject_id` (empty unless
//! `named`), `action_schema_hash`. The decision carries exactly the permission `approval:reroute`.

use sha2::{Digest, Sha256};

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: accepts a member decide and a reroute, and stores a stated addressee.
pub const MEMBER_DECIDE_CORE_ENV: &str = "SESSION_CORE_APPROVAL_MEMBER_DECIDE";

/// The typed refusal reason while the switch is off.
pub const MEMBER_DECIDE_DISABLED: &str = "approval_member_decide_disabled";

/// Pure form of the switch: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether this Session Core process accepts member decides.
#[must_use]
pub fn member_decide_core_enabled() -> bool {
    enabled_from(std::env::var(MEMBER_DECIDE_CORE_ENV).ok().as_deref())
}

/// Control decision identity for a member decide (audience `model-plane`).
pub const DECIDE_ACTION_ID: &str = "model.approval.decide";
pub const DECIDE_SCHEMA: &str = "sha256:approval-decide-v1";
/// The only permission a member-decide decision carries; disjoint from the thread permissions.
pub const PERMISSION_DECIDE: &str = "approval:decide";

/// Control decision identity for a reroute (audience `model-plane`).
pub const REROUTE_ACTION_ID: &str = "model.approval.reroute";
pub const REROUTE_SCHEMA: &str = "sha256:approval-reroute-v1";
/// The only permission a reroute decision carries.
pub const PERMISSION_REROUTE: &str = "approval:reroute";

/// `approvals.addressee_kind` values (migration 0074).
pub const KIND_REQUESTER: &str = "requester";
pub const KIND_NAMED: &str = "named";
pub const KIND_ANY_EDITOR: &str = "any_editor";
pub const KIND_ANY_MANAGER: &str = "any_manager";
/// The closed set of addressee kinds.
pub const ADDRESSEE_KINDS: [&str; 4] = [KIND_REQUESTER, KIND_NAMED, KIND_ANY_EDITOR, KIND_ANY_MANAGER];

/// Most rows a Space-wide approvals read returns.
pub const MAX_SPACE_APPROVALS: i64 = 100;

fn write_field(digest: &mut Sha256, name: &str, value: &str) {
    digest.update(name.as_bytes());
    digest.update([0]);
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

/// The payload digest a `model.approval.decide` decision commits to.
#[must_use]
pub fn decide_payload_digest(
    org_id: &str,
    user_id: &str,
    approval_id: &str,
    space_id: &str,
    space_decision_ref: &str,
    decision: &str,
    scope: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.approval.decide\0v1\0");
    for (name, value) in [
        ("org_id", org_id),
        ("user_id", user_id),
        ("approval_id", approval_id),
        ("space_id", space_id),
        ("space_decision_ref", space_decision_ref),
        ("decision", decision),
        ("scope", scope),
        ("action_schema_hash", DECIDE_SCHEMA),
    ] {
        write_field(&mut digest, name, value);
    }
    format!("sha256:{:x}", digest.finalize())
}

/// The payload digest a `model.approval.reroute` decision commits to.
#[must_use]
pub fn reroute_payload_digest(
    org_id: &str,
    user_id: &str,
    approval_id: &str,
    space_id: &str,
    space_decision_ref: &str,
    addressee_kind: &str,
    addressee_subject_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.approval.reroute\0v1\0");
    for (name, value) in [
        ("org_id", org_id),
        ("user_id", user_id),
        ("approval_id", approval_id),
        ("space_id", space_id),
        ("space_decision_ref", space_decision_ref),
        ("addressee_kind", addressee_kind),
        ("addressee_subject_id", addressee_subject_id),
        ("action_schema_hash", REROUTE_SCHEMA),
    ] {
        write_field(&mut digest, name, value);
    }
    format!("sha256:{:x}", digest.finalize())
}
