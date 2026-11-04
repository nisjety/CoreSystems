//! The org agent policy binds at the gateway's authority points (build plan WS25-S9): a cap binds a turn the
//! person did not choose a mode for, a choice cannot loosen the cap, a plan grant is held to the rung cap, and
//! the wiring that carries each decision into the request is in place.

use std::time::Instant;

use model_gateway::org_agent_policy::{
    clamp_rung, dispatch_rung_cap, held_revision, held_rung_cap, held_web_caps, resolve_turn,
    stamp_web_policy, OrgPolicy, OrgPolicyCache, PolicyMode, WebCaps,
};
use model_gateway::profile::ApprovalPosture;
use mp_contracts::agent_mode::AgentMode;
use mp_contracts::model_plane::v1::AutonomyRung;
use serde_json::{json, Map, Value};

fn document(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// A cache that holds one org's policy, read now, as the admission step would have left it.
fn held(org_id: &str, settings: Value) -> OrgPolicyCache {
    let cache = OrgPolicyCache::new();
    cache.store(
        org_id,
        OrgPolicy::from_settings(1, "sha256:1", &document(settings)),
        Instant::now(),
    );
    cache
}

#[test]
fn a_cap_binds_a_turn_the_person_did_not_choose_a_mode_for() {
    let resolved = resolve_turn(ApprovalPosture::Auto, None, Some(AgentMode::ReadOnly))
        .expect("the cap binds the turn");
    assert_eq!(resolved.mode, AgentMode::ReadOnly);
    assert!(
        resolved.preset.plan_gate,
        "read_only holds the turn to plan mode"
    );
}

#[test]
fn a_person_cannot_loosen_an_org_cap_by_choosing_a_looser_mode() {
    for choice in AgentMode::ALL {
        let resolved = resolve_turn(
            ApprovalPosture::Auto,
            Some(choice),
            Some(AgentMode::ReadOnly),
        )
        .expect("the cap binds the turn");
        assert!(
            resolved.mode.strictness() >= AgentMode::ReadOnly.strictness(),
            "{choice:?} loosened a read_only cap to {:?}",
            resolved.mode
        );
    }
}

#[test]
fn an_org_at_the_default_cap_leaves_a_turn_without_a_choice_unbound() {
    assert_eq!(
        resolve_turn(ApprovalPosture::Auto, None, Some(AgentMode::Auto)),
        None
    );
}

#[test]
fn a_lockdown_cap_refuses_a_do_run_whatever_mode_the_person_chose() {
    for choice in [None, Some(AgentMode::Auto), Some(AgentMode::AskFirst)] {
        let resolved = resolve_turn(ApprovalPosture::Auto, choice, Some(AgentMode::Lockdown))
            .expect("the cap binds the turn");
        assert!(
            !resolved.preset.do_runs,
            "{choice:?} allowed a Do run under lockdown"
        );
    }
}

#[test]
fn a_plan_grant_above_the_rung_cap_is_held_to_the_cap() {
    let cache = held(
        "org-policy-it-grant",
        json!({"rung_cap": "workspace_write"}),
    );
    let cap = held_rung_cap(PolicyMode::Enforce, &cache, "org-policy-it-grant");
    assert_eq!(
        clamp_rung(Some(AutonomyRung::DangerFullAccess), cap),
        Some(AutonomyRung::WorkspaceWrite)
    );
}

#[test]
fn a_run_with_no_grant_states_the_cap_and_no_cap_states_nothing_new() {
    let capped = held("org-policy-it-capped", json!({"rung_cap": "read_only"}));
    let cap = dispatch_rung_cap(PolicyMode::Enforce, &capped, "org-policy-it-capped");
    assert_eq!(clamp_rung(None, cap), Some(AutonomyRung::ReadOnly));

    let uncapped = held("org-policy-it-uncapped", json!({}));
    let no_cap = dispatch_rung_cap(PolicyMode::Enforce, &uncapped, "org-policy-it-uncapped");
    assert_eq!(
        clamp_rung(None, no_cap),
        None,
        "a default cap leaves the request as it was"
    );
}

#[test]
fn an_enforced_org_the_cache_does_not_hold_is_held_to_read_only_and_off_holds_nothing() {
    let cache = OrgPolicyCache::new();
    assert_eq!(
        dispatch_rung_cap(PolicyMode::Enforce, &cache, "org-policy-it-unknown"),
        Some(AutonomyRung::ReadOnly)
    );
    assert_eq!(
        dispatch_rung_cap(PolicyMode::Off, &cache, "org-policy-it-unknown"),
        None
    );
    assert_eq!(
        dispatch_rung_cap(PolicyMode::Shadow, &cache, "org-policy-it-unknown"),
        None
    );
}

#[test]
fn a_run_is_attributed_to_the_held_revision_only_under_enforcement() {
    let cache = held("org-policy-it-revision", json!({"mode_cap": "read_only"}));
    assert_eq!(
        held_revision(PolicyMode::Enforce, &cache, "org-policy-it-revision"),
        Some((1, "sha256:1".to_owned()))
    );
    assert_eq!(
        held_revision(PolicyMode::Shadow, &cache, "org-policy-it-revision"),
        None
    );
}

#[test]
fn an_org_that_turns_web_access_off_is_held_to_it_and_an_unread_one_is_not() {
    let cache = held(
        "org-policy-it-web",
        json!({"web.mode": "off", "web.paid_providers": "off"}),
    );
    // The inline loop reads the switches the admission left warm; they bind only under enforcement.
    assert_eq!(
        held_web_caps(PolicyMode::Enforce, &cache, "org-policy-it-web"),
        Some(WebCaps {
            live: false,
            paid: false
        })
    );
    assert_eq!(
        held_web_caps(PolicyMode::Shadow, &cache, "org-policy-it-web"),
        None
    );
    // A Do run is stamped with the switches it was admitted under.
    let mut stamped = None;
    stamp_web_policy(PolicyMode::Enforce, &cache, "org-policy-it-web", &mut stamped);
    let web = stamped
        .as_ref()
        .and_then(|context| context.web.as_ref())
        .expect("the run carries its web switches");
    assert_eq!(web.mode, "off");
    assert!(!web.paid_providers);
    // An enforced org the cache does not hold: the Do run is held to the fail-closed switches, and an inline
    // turn keeps its web access, because an outage on Ask must not silently break an answer.
    let empty = OrgPolicyCache::new();
    let mut unread = None;
    stamp_web_policy(PolicyMode::Enforce, &empty, "org-policy-it-unread", &mut unread);
    assert_eq!(
        unread
            .as_ref()
            .and_then(|context| context.web.as_ref())
            .map(|web| web.mode.as_str()),
        Some("off")
    );
    assert_eq!(
        held_web_caps(PolicyMode::Enforce, &empty, "org-policy-it-unread"),
        None
    );
}

/// The decisions above reach the request only through the call sites that make it. These guards read the
/// gateway source for each call, in the same style as the other contract tests of this crate.
#[test]
fn the_admission_the_dispatch_and_the_grant_all_call_the_policy() {
    let sse = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/sse.rs"))
        .expect("sse.rs is readable");
    for call in [
        "org_agent_policy::admit_turn(",
        "org_agent_policy::resolve_turn(",
        "org_agent_policy::clamp_rung(",
        "org_agent_policy::dispatch_rung_cap(",
        "org_agent_policy::stamp_policy_revision(",
    ] {
        assert!(sse.contains(call), "sse.rs must make the call {call}");
    }
    let coordinator =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/coordinator.rs"))
            .expect("coordinator.rs is readable");
    assert!(
        coordinator.contains("org_agent_policy::held_rung_cap("),
        "record_grant must hold the grant to the org's rung cap"
    );
}

/// The org's web switches (Wave 3 EP-9 W2) reach both loops: the inline turn withholds the public web tools and the
/// paid grant, and a Do run is stamped for execution-core. Each is a call site this crate makes, so the guard reads
/// the source for it.
#[test]
fn the_web_switches_reach_both_loops() {
    let sse = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/sse.rs"))
        .expect("sse.rs is readable");
    for call in [
        "org_agent_policy::held_web_caps(",
        "org_agent_policy::stamp_web_policy(",
        "tool_loop::is_public_web_tool",
    ] {
        assert!(sse.contains(call), "sse.rs must make the call {call}");
    }
    let tool_loop = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/tool_loop.rs"))
        .expect("tool_loop.rs is readable");
    assert!(
        tool_loop.contains("org_agent_policy::org_allows_paid_providers("),
        "the paid search grant must be read with the org's web.paid_providers switch"
    );
}
