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

/// The class-driven half decides the same way: from the call, never the name alone.
/// `TOOL_CLASS_GATE` only chooses which of the two answers decides, so the choice
/// itself must go through the class lookup, and the class answer must still read
/// the arguments (the provider-operation refinement is per call).
#[test]
fn the_declared_class_rung_is_also_decided_from_the_call() {
    let text = source("src/permission/mod.rs");
    let gated = function_body(&text, "pub fn rung_required_for_with(");
    assert!(
        gated.contains("rung_required_for_class(") && gated.contains("tool_input"),
        "the gated rung must reach the class lookup with the call's arguments: {gated}"
    );
    let declared = function_body(&text, "pub fn rung_required_for_class(");
    assert!(
        declared.contains("tool_input"),
        "the declared-class rung must still read the call's arguments: {declared}"
    );
}

/// No call site names a heuristic directly any more: the loop, the step gate and
/// the retry policy ask the run's classifier, so one switch moves them together
/// and none can be left reading names while the others read classes.
#[test]
fn every_call_site_asks_the_runs_classifier_not_a_name_heuristic() {
    fn code(text: &str) -> String {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    let agent = code(&source("src/runtime_loop/agent.rs"));
    for direct in [
        "crate::permission::is_risky_call(",
        "crate::permission::is_restricted_context_write(",
        "crate::permission::check_autonomy_rung(",
        "crate::permission::evaluate_call(",
    ] {
        assert!(
            !agent.contains(direct),
            "agent.rs calls `{direct}` directly: use the run's `RunGate`"
        );
    }
    let step = code(&source("src/runtime_loop/mod.rs"));
    assert!(
        step.contains("RunGate::for_run(org_id, run_id)"),
        "the step gate must classify through the run's gate"
    );
    assert!(
        !step.contains("permission::evaluate_call("),
        "the step gate must not call the name heuristic directly"
    );
    let retry = function_body(
        &source("src/runtime_loop/retry.rs"),
        "pub fn tool_call_is_retryable(",
    );
    assert!(
        retry.contains("gate.is_risky_call("),
        "the retry policy must classify through the run's gate: {retry}"
    );
}

/// The effect ledger and the retry policy read one classifier. If the ledger kept the name
/// heuristic while the retry policy asks the run's gate, a read-declared MCP tool would be
/// replayed under one External reservation: the duplicated call the ledger exists to fence.
/// And whatever the classifiers say, a call that holds an effectful reservation is never
/// dispatched twice.
#[test]
fn the_ledger_classifies_through_the_runs_gate_and_fences_a_retry() {
    fn code(text: &str) -> String {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("
")
    }
    let agent = code(&source("src/runtime_loop/agent.rs"));
    assert!(
        agent.contains("effect::effect_class_for_run(") && agent.contains("effect::entry_for_run_call("),
        "the loop must classify its ledger entries through the run's gate"
    );
    for name_based in ["effect::effect_class_of(", "effect::entry_for_call("] {
        assert!(
            !agent.contains(name_based),
            "agent.rs classifies the ledger with `{name_based}`, the name heuristic the retry policy no longer uses"
        );
    }
    let effect = source("src/runtime_loop/effect.rs");
    let for_run = function_body(&effect, "pub(crate) fn effect_class_for_run(");
    assert!(
        for_run.contains("gate.rung_required_for("),
        "the ledger class must come from the run's gate: {for_run}"
    );
    let retry_site = agent
        .split("runtime_loop::retry::run_attempts(")
        .nth(1)
        .expect("the retry decision is gone")
        .split("|failed, retry|")
        .next()
        .unwrap_or_default()
        .to_owned();
    assert!(
        retry_site.contains("effect::ledger_fences_retry(") && retry_site.contains("ledger_entry"),
        "the retry decision must ask what the ledger holds for the call: {retry_site}"
    );
}

/// A governed MCP call is judged against the class its own signed policy decision carries,
/// not only the snapshot the offer took: a registry row tightened since, with its digest
/// unchanged, would otherwise run under the looser view. The comparison comes before the
/// mode check, which reads the same snapshot, and before any credential or network call.
#[test]
fn a_governed_mcp_call_is_compared_with_its_signed_class_before_anything_else() {
    let text = source("src/runtime_loop/mod.rs");
    let body = function_body(&text, "async fn execute_governed_mcp(");
    let compared = body
        .find("signed.differs_from(")
        .expect("execute_governed_mcp no longer compares the signed class");
    for later in ["mode.allows(", "GovernedMcp::shared(", ".proxy("] {
        let at = body
            .find(later)
            .unwrap_or_else(|| panic!("execute_governed_mcp no longer calls `{later}`"));
        assert!(compared < at, "the class comparison must come before `{later}`: {body}");
    }
    assert!(
        body.contains("CLASS_CHANGED"),
        "the refusal must be the typed `class_changed` one: {body}"
    );
    let step = function_body(&text, "async fn execute_step_inner(");
    assert!(
        step.contains("SignedToolClass::new(") && step.contains("&signed_class"),
        "the step must hand the dispatch the class its policy decision carried"
    );
    // Every arm that reaches the dispatch captured it; none can leave it at its default.
    assert!(
        !step.contains("SignedToolClass::default()"),
        "a dispatch whose signed class was left at the default would fail every governed call closed"
    );
}

/// A run that keeps nothing (ZDR, or a waived route) is offered no MCP tool and nothing is
/// asked on its behalf: the waiver reaches the offer, not only the dispatch refusal.
#[test]
fn the_offer_is_made_with_the_routes_waiver() {
    let text = source("src/runtime_loop/agent.rs");
    let run_agent = function_body(&text, "pub(crate) async fn run_agent(");
    assert!(
        run_agent.contains("offer_is_waived(&req, caller)")
            && run_agent.contains("offered_tools(&req, waived,"),
        "run_agent must make its offer knowing whether the route is waived: {run_agent}"
    );
    let offered = function_body(&text, "async fn offered_tools(");
    assert!(
        offered.contains("req.zdr || waived"),
        "a waived route must be treated as zero-retention by the offer: {offered}"
    );
}

/// A retention-bound run skips the MCP half of the offer only. The block that learns the
/// BUILT-IN classes is not behind the retention flag, or a ZDR run would be classified by the
/// legacy name heuristic, which is looser than the org's registry when it tightened a tool.
#[test]
fn a_retention_bound_run_still_learns_the_built_in_classes() {
    let text = source("src/runtime_loop/agent.rs");
    let body = function_body(&text, "async fn merged_tool_defs_governed(");
    assert!(
        body.contains("if governed_mode != McpDoTools::Off || gate != ToolClassGate::Off {"),
        "the resolve block must run for any run whose gate reads classes: {body}"
    );
    assert!(
        !body.contains("!zdr && (governed_mode"),
        "the resolve block must not be skipped for a retention-bound run: {body}"
    );
    // The MCP side stays off for it: governed mode is forced Off before the block.
    let forced_off = body
        .find("} else if zdr {")
        .expect("a ZDR run must have its governed mode forced off");
    let block = body
        .find("if governed_mode != McpDoTools::Off || gate")
        .expect("resolve block");
    assert!(forced_off < block, "governed mode must be decided before the block");
}

/// An MCP call carries the run's content to a third-party server, so retention is
/// refused before anything that could reach the network: before the name is parsed,
/// before a gateway client or a credential exists.
#[test]
fn an_mcp_call_is_refused_under_retention_before_any_network_call() {
    let text = source("src/runtime_loop/mod.rs");
    let body = function_body(&text, "async fn execute_mcp(");
    let refusal = body
        .find("retention_waived")
        .expect("execute_mcp lost its retention check");
    for reached_later in [
        "parse_mcp_tool_name(",
        "McpGatewayClient::from_env(",
        "execute_governed_mcp(",
    ] {
        let at = body
            .find(reached_later)
            .unwrap_or_else(|| panic!("execute_mcp no longer calls `{reached_later}`"));
        assert!(
            refusal < at,
            "the retention refusal must come before `{reached_later}`: {body}"
        );
    }
    assert!(
        body.contains("feature_not_supported(\"mcp_execution\")"),
        "the refusal must be the typed `mcp_execution` one: {body}"
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
        "pub fn check_autonomy_rung_with(",
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
            "async fn exit_plan_mode<P",
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

/// The org's rung cap (WS25-S9) is applied where a fresh run's rung is decided, before the request states it. A
/// resumed run still states no rung of its own, so the cap never widens a continuation.
#[test]
fn an_org_rung_cap_is_applied_before_a_fresh_run_states_its_rung() {
    let services = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("execution-core's parent is rust/services");
    let text = std::fs::read_to_string(services.join("model-gateway/src/sse.rs"))
        .expect("model-gateway's sse.rs is readable");
    let dispatch = function_body(&text, "fn spawn_run_dispatch(");
    let clamp = dispatch
        .find("org_agent_policy::clamp_rung(")
        .expect("the dispatch holds the granted rung to the org cap");
    let built = dispatch
        .find("resolved_run_agent_request(")
        .expect("the dispatch builds the request");
    assert!(clamp < built, "the cap must bind the grant before the request states it");
    let request = function_body(&text, "fn resolved_run_agent_request(");
    assert!(
        request.contains("dispatch_autonomy_rung("),
        "a resumed run states no rung of its own: the rung is decided by dispatch_autonomy_rung"
    );
}
