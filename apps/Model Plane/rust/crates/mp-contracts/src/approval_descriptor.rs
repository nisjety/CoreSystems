//! The fingerprinted scope of an approval continuation descriptor (EP-10-S3).
//!
//! execution-core binds a paused tool call to its approval with `action_fingerprint`: a blake3 of
//! the compact JSON of this scope. The fold keeps the fingerprint, session-core stores it beside the
//! encrypted descriptor, and a resume or a delivery matches on it. An edit (EP-10-S4) rebuilds the
//! scope with the edited input in session-core, so both planes must build the same bytes: the
//! function lives here and execution-core calls it.
//!
//! Moved byte-for-byte from execution-core's `continuation_descriptor`. Any change to the keys,
//! their values or the encoding changes the fingerprint of every new pause and orphans every
//! approval still pending, so `tests::the_fingerprint_of_a_fixed_scope_is_pinned` pins one.

use serde_json::{json, Value};

/// The descriptor version this scope describes.
pub const DESCRIPTOR_VERSION: u32 = 1;

/// The part of a continuation descriptor that the fingerprint covers: who, which run and step,
/// which tool with which input, under which permission mode. Ids that sit outside the fingerprint
/// (the Space, a ticket binding) are added to the descriptor by its builder, never here.
#[must_use]
pub fn descriptor_scope(
    run_id: &str,
    org_id: &str,
    user_id: &str,
    step_id: &str,
    tool_name: &str,
    input: &Value,
    permission_mode: &str,
) -> Value {
    json!({
        "version": DESCRIPTOR_VERSION,
        "run_id": run_id,
        "org_id": org_id,
        "user_id": user_id,
        "step_id": step_id,
        "action_kind": "tool_call",
        "tool_name": tool_name,
        "input": input.clone(),
        "permission_mode": permission_mode,
    })
}

/// The 64-hex blake3 of the scope's compact JSON. The caller maps the error, which a `Value` does
/// not produce in practice, to its own transport.
pub fn action_fingerprint(scope: &Value) -> Result<String, serde_json::Error> {
    let canonical = serde_json::to_string(scope)?;
    Ok(blake3::hash(canonical.as_bytes()).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compact JSON and the fingerprint of the fixed scope below, as execution-core computed
    /// them before the move. A drift here orphans every pending approval.
    const PINNED_CANONICAL: &str = r#"{"action_kind":"tool_call","input":{"body":{"bodyText":"Hei","subject":"Status","to":["alice@example.com"]},"connection_id":"conn-1","operation":"gmail.send"},"org_id":"org-1","permission_mode":"ask","run_id":"run-1","step_id":"step-1","tool_name":"execute_provider_action","user_id":"user-1","version":1}"#;
    const PINNED_FINGERPRINT: &str =
        "fb6e264cffc1d147d1c1307b38b33f873bffa1854049bfe6c204fcf6a41887f1";

    fn fixed_input() -> Value {
        json!({
            "connection_id": "conn-1",
            "operation": "gmail.send",
            "body": {
                "to": ["alice@example.com"],
                "subject": "Status",
                "bodyText": "Hei",
            },
        })
    }

    fn fixed_scope() -> Value {
        descriptor_scope(
            "run-1",
            "org-1",
            "user-1",
            "step-1",
            "execute_provider_action",
            &fixed_input(),
            "ask",
        )
    }

    #[test]
    fn the_fingerprint_of_a_fixed_scope_is_pinned() {
        let scope = fixed_scope();
        assert_eq!(
            serde_json::to_string(&scope).expect("scope encodes"),
            PINNED_CANONICAL
        );
        assert_eq!(
            action_fingerprint(&scope).expect("fingerprint"),
            PINNED_FINGERPRINT
        );
    }

    #[test]
    fn the_scope_matches_the_pre_move_construction() {
        // The construction execution-core used inline before the move, kept here as the reference.
        let input = fixed_input();
        let legacy = json!({
            "version": 1,
            "run_id": "run-1",
            "org_id": "org-1",
            "user_id": "user-1",
            "step_id": "step-1",
            "action_kind": "tool_call",
            "tool_name": "execute_provider_action",
            "input": input.clone(),
            "permission_mode": "ask",
        });
        let legacy_canonical = serde_json::to_string(&legacy).expect("legacy encodes");
        let legacy_fingerprint = blake3::hash(legacy_canonical.as_bytes())
            .to_hex()
            .to_string();
        assert_eq!(fixed_scope(), legacy);
        assert_eq!(
            action_fingerprint(&fixed_scope()).expect("fingerprint"),
            legacy_fingerprint
        );
    }

    #[test]
    fn every_scope_field_moves_the_fingerprint() {
        let base = action_fingerprint(&fixed_scope()).expect("fingerprint");
        let mut edited = fixed_input();
        edited["body"]["subject"] = json!("Status 2");
        let tool = "execute_provider_action";
        let input = fixed_input();
        let variants = [
            descriptor_scope("run-2", "org-1", "user-1", "step-1", tool, &input, "ask"),
            descriptor_scope("run-1", "org-2", "user-1", "step-1", tool, &input, "ask"),
            descriptor_scope("run-1", "org-1", "user-2", "step-1", tool, &input, "ask"),
            descriptor_scope("run-1", "org-1", "user-1", "step-2", tool, &input, "ask"),
            descriptor_scope("run-1", "org-1", "user-1", "step-1", "other_tool", &input, "ask"),
            descriptor_scope("run-1", "org-1", "user-1", "step-1", tool, &edited, "ask"),
            descriptor_scope("run-1", "org-1", "user-1", "step-1", tool, &input, "auto"),
        ];
        for variant in variants {
            let fingerprint = action_fingerprint(&variant).expect("fingerprint");
            assert_eq!(fingerprint.len(), 64);
            assert_ne!(fingerprint, base, "{variant}");
        }
    }
}
