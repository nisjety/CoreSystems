//! Guards the autonomy ladder against the two ways it silently stops working.
//!
//! The ladder crosses three services: model-gateway validates and records a
//! grant, session-core persists it on the run, and execution-core enforces it
//! per call. `mp_contracts::autonomy` is the one shared implementation, so the
//! ordering itself cannot drift — but two things it cannot hold are call-shape
//! properties that a compile is perfectly happy with:
//!
//! 1. **The check must be applied per call, with the arguments.** `DeepSeek`'s
//!    reason for this is exact: "schemas are registry-global while the effective
//!    mode is per-call truth". If the rung were ever baked into a tool
//!    definition, `execute_provider_action` — a read on one call and a write on
//!    the next — would carry one answer for both.
//! 2. **An unstated rung must stay unconstrained in the loop.** `UNSPECIFIED` is
//!    correctly the narrowest rung in `mp_contracts::autonomy::permits`, and
//!    correctly *no constraint* in the loop's gate. Wiring the loop to the
//!    former would refuse every write on every run that predates the ladder —
//!    which typechecks, passes every unit test that states a rung, and breaks
//!    production.
//!
//! Read as source text for the same reason `tool_retry_contract.rs` and
//! `context_recovery_contract.rs` do: these are properties of where a call sits,
//! not of any value a test can observe.

use std::path::{Path, PathBuf};

fn source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do \
             not delete it — it is what keeps the autonomy ladder from becoming \
             decorative.",
            path.display()
        )
    })
}

fn function_body(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is gone; re-point this test"));
    let open = text[start..]
        .find('{')
        .expect("a signature is followed by a body")
        + start;
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text[open..=open + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after `{signature}`");
}

/// The rung a call needs is decided from the call's ARGUMENTS. A name-only
/// classification would give `execute_provider_action` one answer for a read and
/// a write alike, which is exactly the schema-vs-per-call confusion the ladder
/// exists to avoid.
#[test]
fn the_required_rung_is_decided_from_the_call_not_the_tool_name() {
    let text = source("src/permission/mod.rs");
    let body = function_body(&text, "pub fn rung_required_for(");
    assert!(
        body.contains("tool_input"),
        "rung_required_for must inspect the arguments, not just the name: {body}"
    );
    assert!(
        body.contains("is_risky_call("),
        "it must reuse the argument-aware classifier (`is_risky_call`), not the \
         name-only `is_risky_tool` — the two disagree exactly where it matters: {body}"
    );
}

/// The gate is called from the loop, before dispatch, with the live call. If it
/// ever moves into `offered_tool_defs` it becomes a schema property.
#[test]
fn the_rung_gate_runs_in_the_loop_and_never_in_the_tool_catalogue() {
    let text = source("src/runtime_loop/agent.rs");
    let catalogue = function_body(&text, "fn offered_tool_defs()");
    assert!(
        !catalogue.contains("autonomy_rung") && !catalogue.contains("check_autonomy_rung"),
        "the tool catalogue must carry no rung: a schema is registry-global while \
         the effective rung is per-call truth"
    );
    let call_site = text
        .split("check_autonomy_rung(")
        .nth(1)
        .expect("the loop no longer checks the autonomy rung at all");
    let args = call_site.split(')').next().unwrap_or_default();
    assert!(
        args.contains("call.arguments_json"),
        "the gate must be handed the live call's arguments: {args}"
    );
}

/// An unstated rung stays unconstrained in the loop, and stays the narrowest in
/// the shared contract. Both are correct answers to different questions, and
/// wiring one to the other breaks something either way.
#[test]
fn an_unstated_rung_is_unconstrained_in_the_loop_and_narrowest_in_the_contract() {
    let gate = function_body(
        &source("src/permission/mod.rs"),
        "pub fn check_autonomy_rung(",
    );
    assert!(
        gate.contains("AutonomyRung::Unspecified") && gate.contains("return Ok(())"),
        "the loop's gate must return Ok for an unstated rung, or every run that \
         predates the ladder loses its writes: {gate}"
    );

    let contract =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/mp-contracts/src/autonomy.rs");
    let contract = std::fs::read_to_string(&contract)
        .unwrap_or_else(|error| panic!("cannot read the shared ladder ({error})"));
    let rank = function_body(&contract, "pub fn rank(");
    assert!(
        rank.contains("AutonomyRung::Unspecified | AutonomyRung::ReadOnly"),
        "the shared contract must rank an unstated rung with the NARROWEST, so an \
         unset field is never read as a grant: {rank}"
    );
}

/// The grant is refused when it is malformed, on BOTH sides. model-gateway is
/// the caller that should have checked; a server that trusts its caller to have
/// checked has no rule at all.
#[test]
fn both_the_gateway_and_session_core_validate_the_grant() {
    let services = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("execution-core's parent is rust/services");
    for (relative, signature) in [
        (
            "model-gateway/src/coordinator.rs",
            "pub async fn handle_exit_plan_mode",
        ),
        ("session-core/src/grpc.rs", "async fn set_run_mode"),
    ] {
        let text = std::fs::read_to_string(services.join(relative))
            .unwrap_or_else(|error| panic!("cannot read {relative} ({error})"));
        let body = function_body(&text, signature);
        assert!(
            body.contains("AutonomyEscalation::request("),
            "{relative} must validate the grant through the shared contract, not \
             accept it as given"
        );
    }
}
