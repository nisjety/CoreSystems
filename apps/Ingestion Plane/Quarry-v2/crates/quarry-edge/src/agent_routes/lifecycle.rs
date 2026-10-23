use super::*;

/// Binds a validated broker grant to the run being started (WS1.B).
///
/// A user principal may only start a run under a grant issued to that user
/// (`browser grant belongs to another user`). A service principal acts for
/// the run's owner, so the owner rule does not apply to it. Quarry does NOT check
/// the grant's organization for either kind of caller: the broker's
/// `ValidateGrantResponse` carries no `org_id`, so the organization scoping is the
/// broker's own. Every check is made with a service token minted for the verified
/// run's organization (`validate_for_org`), and the broker resolves the grant
/// within that organization, so another organization's grant is not found (an
/// inactive grant, a 403). The service token is not bound to the grant's owner:
/// that gap stays pinned by the known-gap test
/// `known_gap_a_service_principal_is_not_bound_to_the_grants_owner`.
/// An empty `owner_id` (a broker that predates owners, or the no-op
/// validator) is refused only when grants are required.
///
/// Returns the broker's canonical hosts when they replace the caller's
/// domains: whenever grants are required, and in every mode when the grant
/// carries a host policy. `None` keeps the caller's constraints.
pub(super) fn bind_validated_grant(
    grant: &quarry_runtime::GrantValidation,
    claims: &crate::auth::Claims,
    require_browser_grants: bool,
) -> Result<Option<Vec<String>>, String> {
    if !claims.is_service() {
        let owner = grant.owner_id.trim();
        if owner.is_empty() {
            if require_browser_grants {
                return Err("browser grant belongs to another user".to_owned());
            }
        } else if owner != claims.actor_id() {
            return Err("browser grant belongs to another user".to_owned());
        }
    }
    if require_browser_grants || !grant.allowed_domains.is_empty() {
        return BrowserEgressPolicy::canonical_broker_domains(&grant.allowed_domains).map(Some);
    }
    Ok(None)
}

/// [`status_err`] for a ZDR refusal that was enforced before WS4.H: the same status, the
/// same `BAD_REQUEST` code and the same message, plus the shared
/// `details.zdr_refusal { code, action }` (`crate::zdr_refusal::refusal_details`). A refusal
/// that had its own machine-readable token before WS4.H passes it as `legacy_code`; it is
/// added to the details beside `zdr_refusal` (the plan's WS4.H detail), so a client that
/// matched the old token in the message can read it from a field.
fn zdr_status_err(
    status: StatusCode,
    request_id: &str,
    msg: &str,
    action: &str,
    legacy_code: Option<&str>,
) -> ApiErr {
    let mut details = crate::zdr_refusal::refusal_details(action);
    if let (Some(legacy), Some(map)) = (legacy_code, details.as_object_mut()) {
        map.insert("legacy_code".to_owned(), serde_json::json!(legacy));
    }
    (
        status,
        Json(Envelope::<()>::err(
            request_id,
            QuarryError::new(ErrorCode::BadRequest, msg).with_details(details),
        )),
    )
}

/// A ZDR grant may only back a ZDR run: a run that is not ZDR would persist
/// what the grant's owner asked never to be kept. A non-ZDR grant may back
/// either (the run's own posture can only raise ZDR).
pub(super) fn zdr_grant_may_back_run(grant_zdr: bool, run_zdr: bool) -> bool {
    !grant_zdr || run_zdr
}

/// The class a run is swept and capped under. A resumed run keeps the stricter of the
/// class it was checkpointed with and the one this request derives, so a person who
/// resumes an agent's run without the `initiated_by: "agent"` hint does not turn it
/// into an interactive session with the longer interactive limits (the budget ledger
/// already takes the same rule, `RunBudget::start`).
///
/// The stored class is the budget ledger's, so a run checkpointed while budget
/// enforcement was `off` carries none and is classed from the request alone.
pub(super) fn resumed_run_class(
    checkpoint: Option<&AgentRunCheckpoint>,
    derived: crate::budget::RunClass,
) -> crate::budget::RunClass {
    checkpoint
        .and_then(|checkpoint| checkpoint.budget.as_ref())
        .and_then(|budget| crate::budget::RunClass::from_label(&budget.class))
        .map_or(derived, |stored| stored.stricter(derived))
}

/// `POST /v1/agent/runs` — acquire a leased browser session for a new run.
pub async fn start_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    headers: axum::http::HeaderMap,
    Json(body): Json<StartRunBody>,
) -> Result<Json<Envelope<StartRunData>>, ApiErr> {
    let request_id = crate::request_id::current();
    if let Some(error) = crate::drain::refusal() {
        return Err(driver_err(&request_id, error));
    }
    let org_id = claims.org_id.clone();
    let actor_id = claims.actor_id().to_owned();
    let execution_tier = body.execution_tier;
    // WS6.C-e: the surface of this start (or resume) is the surface of its usage segment.
    let surface = crate::usage_attrs::surface_for(
        &claims,
        headers
            .get(crate::usage_attrs::SURFACE_HEADER)
            .and_then(|value| value.to_str().ok()),
    );

    if execution_tier.is_ephemeral_evidence() && !experimental_lite_enabled() {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "ephemeral_evidence is disabled; set QUARRY_LITE_EXPERIMENTAL=1 for the lab",
        ));
    }
    if execution_tier.is_ephemeral_evidence()
        && (body.persist_profile || body.profile_id.is_some() || body.resume_run_id.is_some())
    {
        return Err(status_err(
            StatusCode::BAD_REQUEST,
            &request_id,
            "ephemeral_evidence forbids profiles, persistence, and resume",
        ));
    }

    // Do not treat a provider's top-level navigation guard as proof that
    // page-controlled redirects, frames, fetch/XHR, or DNS rebinding are
    // contained. Until a driver can prove the stronger boundary, agent
    // execution remains unavailable rather than silently unsafe.
    require_isolated_agent_egress(&state).map_err(|error| driver_err(&request_id, error))?;
    require_driver_capabilities(&body.driver_requirements, state.agent_driver.capabilities())
        .map_err(|error| driver_err(&request_id, error))?;

    let resume_run_id = match body.resume_run_id.as_deref() {
        Some(raw) => match raw.parse::<RunKind>() {
            Ok(run_id) => Some(run_id),
            Err(_) => {
                return Err(status_err(
                    StatusCode::BAD_REQUEST,
                    &request_id,
                    "invalid resume_run_id",
                ));
            }
        },
        None => None,
    };
    let resume_checkpoint = if let Some(run_id) = resume_run_id.as_ref() {
        let checkpoint = state
            .receipts
            .load_run_checkpoint(&org_id, run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        let Some(checkpoint) = checkpoint else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "no durable checkpoint exists for resume_run_id",
            ));
        };
        if checkpoint.run_id != run_id.to_string()
            || !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims)
        {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        if checkpoint.status == AgentRunStatus::Closed {
            return Err(status_err(
                StatusCode::CONFLICT,
                &request_id,
                "agent run is already closed",
            ));
        }
        if checkpoint.zdr.is_active() {
            return Err(zdr_status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "ZDR runs cannot be resumed from durable state",
                "resume",
                // The resume refusal never had a token of its own (its message carries none),
                // so there is no legacy code to repeat.
                None,
            ));
        }
        Some(checkpoint)
    } else {
        None
    };

    // A resumed run's persisted constraints are authoritative. This
    // prevents a caller from widening a budget or allowed-domain set after
    // an edge restart.
    let mut constraints = resume_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.constraints.clone())
        .unwrap_or_else(|| body.constraints.clone());

    // W4: max_cost_usd is now supported via the flat action-cost table
    // and enforcement in AgentLoop. The blanket reject is removed.
    let grant_id = body.grant_id.clone().or_else(|| {
        resume_checkpoint
            .as_ref()
            .and_then(|cp| cp.grant_id.clone())
    });
    if state.require_browser_grants && grant_id.is_none() {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "browser grant_id is required",
        ));
    }
    let mut grant_is_zdr = false;
    // What the grant says about the run's budget (WS6.E). Snapshotted here, at
    // start, and never read again: per-action grant revalidation does not
    // touch it, so a later validate cannot widen the run.
    let mut grant_budget: Option<quarry_runtime::grant_validator::GrantBudget> = None;
    let mut grant_is_run_kind = false;
    // A person's `human_browse` grant, admitted once the run's ZDR posture is known (WS8.B).
    let mut human_grant: Option<quarry_runtime::GrantValidation> = None;
    if let Some(grant_id) = grant_id.as_deref() {
        let grant = state
            .grant_validator
            .validate_for_org(&org_id, grant_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        if !grant.is_usable() {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser grant is inactive or expired",
            ));
        }
        grant_is_zdr = grant.zdr;
        grant_budget = grant.budget.clone().filter(|claim| claim.is_set());
        grant_is_run_kind = grant.kind == "run";
        if grant.kind == super::human_browse::GRANT_KIND_HUMAN_BROWSE {
            human_grant = Some(grant.clone());
        } else if let Some(broker_domains) =
            bind_validated_grant(&grant, &claims, state.require_browser_grants)
                .map_err(|reason| status_err(StatusCode::FORBIDDEN, &request_id, &reason))?
        {
            if resume_checkpoint.is_some() && constraints.allowed_domains != broker_domains {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "resumed run domain policy no longer matches its broker grant",
                ));
            }
            // At first acquisition, authority is strictly broker-owned:
            // never use domains carried in the Model/client request.
            constraints.allowed_domains = broker_domains;
        }
    }

    let run_id: RunKind = resume_run_id.unwrap_or_else(Id::new);
    let lease_id: LeaseKind = Id::new();
    // Reuse a caller-supplied profile (cookie/session continuity across
    // runs) when provided & parseable; otherwise mint a fresh one.
    let profile_id: ProfileKind = if let Some(checkpoint) = resume_checkpoint.as_ref() {
        checkpoint.profile_id.parse().map_err(|_| {
            status_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_id,
                "durable checkpoint has an invalid profile id",
            )
        })?
    } else {
        body.profile_id
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Id::new)
    };
    // The token's ZDR floor (a user in a ZDR org) can only raise the run's
    // posture, a service caller's posture is its body flag (see
    // `Claims::zdr_floor`). A resumed run keeps its persisted posture, which
    // is never ZDR here (ZDR checkpoints are refused above), and is raised by
    // the floor too: an org that turned ZDR on since the run was
    // checkpointed does not get durable state written for it any more, and
    // a resume that needs a persistent profile is refused below.
    let zdr = match resume_checkpoint.as_ref() {
        Some(checkpoint) => ZdrMode::from(checkpoint.zdr.is_active() || claims.zdr_floor()),
        None => {
            claims.effective_zdr_mode(Some(body.zdr || execution_tier.is_ephemeral_evidence()))
        }
    };
    zdr.mark_current_span();
    if !zdr_grant_may_back_run(grant_is_zdr, zdr.is_active()) {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "a ZDR browser grant cannot back a non-ZDR run",
        ));
    }

    // WS8.B: a person's open-web run. Its grant is admitted here (switch, person, owner,
    // ZDR posture, open-web policy). The run takes no domains from the caller and no step,
    // time or cost bound from the request; the broker's policy is its only host authority.
    let mut nav_policy = super::human_browse::RunNavPolicy::Allowlist;
    if let Some(human) = human_grant.as_ref() {
        nav_policy = super::human_browse::admit_start(
            crate::run_limits::human_browse_enabled(),
            claims.is_service(),
            claims.actor_id(),
            human,
            zdr.is_active(),
        )
        .map_err(|reason| status_err(StatusCode::FORBIDDEN, &request_id, reason))?;
        constraints.allowed_domains = Vec::new();
        constraints.max_steps = 0;
        constraints.max_runtime_s = None;
        constraints.max_cost_usd = None;
    }
    // A resumed run keeps the navigation mode it started with. An unknown stored mode fails closed.
    if let Some(checkpoint) = resume_checkpoint.as_ref() {
        if !super::human_browse::resume_policy_matches(checkpoint.policy_mode.as_deref(), &nav_policy) {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "resumed run policy no longer matches its broker grant",
            ));
        }
    }
    let ttl_s = constraints.max_runtime_s.unwrap_or(120);
    let viewport = normalize_viewport(
        resume_checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.viewport)
            .or(body.viewport),
    )
    .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg.as_str()))?;

    let persist_profile = resume_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.persist_profile)
        .unwrap_or(
            !execution_tier.is_ephemeral_evidence()
                && (body.persist_profile || body.profile_id.is_some()),
        );
    let profile_scope = resume_checkpoint.as_ref().map_or(
        if execution_tier.is_ephemeral_evidence() {
            BrowserProfileScope::Ephemeral
        } else {
            body.profile_scope
        },
        |checkpoint| checkpoint.profile_scope,
    );
    if zdr_forbids_persistent_profile(zdr, persist_profile, body.profile_id.is_some()) {
        return Err(zdr_status_err(
            StatusCode::BAD_REQUEST,
            &request_id,
            "zdr_persistent_profile_forbidden: a ZDR run cannot request a persistent profile or profile_id",
            "profile",
            Some("zdr_persistent_profile_forbidden"),
        ));
    }
    // WS7.D: the run's web policy, resolved once from the caller's principal, the ZDR posture,
    // the operator baseline and the request's `policy` ask. A refusal here consumes nothing.
    let web_decision = crate::agent_policy_gate::resolve_for_start(
        &state.policy,
        &claims,
        zdr.is_active(),
        body.policy.as_ref(),
        persist_profile,
        body.driver_requirements.uploads_from_artifacts,
        body.driver_requirements.downloads_to_artifacts,
    )
    .map_err(|error| driver_err(&request_id, error))?;
    if persist_profile && !state.agent_driver.capabilities().persistent_profile {
        return Err(driver_err(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot provide a persistent profile",
            ),
        ));
    }

    let lease = BrowserLease {
        lease_id: lease_id.clone(),
        profile_id,
        session_affinity_key: run_id.to_string(),
        // W1 — per-run dedicated proxy identity. The `host` for the
        // sticky derivation is the lease's `session_affinity_key`
        // (the run id), so the same agent run on a different target
        // host keeps the same egress identity for the run's lifetime.
        proxy_affinity: quarry_runtime::proxy_affinity::derive(
            &org_id,
            &run_id.to_string(),
            &run_id.to_string(),
            &PrivacyPolicy::default(),
            &state.proxy_pool_name,
        ),
        ttl_s,
        capabilities: vec![Capability::Actions, Capability::Js, Capability::Screenshots],
        artifact_bucket: String::new(),
        persist_profile,
        viewport,
        org_id: org_id.clone(),
    };
    let profile_id = lease.profile_id.to_string();

    // The run caps come first (R-26: run cap, then the driver's own cap). The slot
    // counts against them from here and frees itself on every early return below.
    // With claims enforced, a run grant is minted for an AI run (R-8), so a
    // person's token holding one is not an uncapped interactive run. Without
    // `enforce` the class is what it always was.
    let enforce_claim = crate::budget::current_claim_mode() == crate::budget::ClaimMode::Enforce;
    let run_class = resumed_run_class(
        resume_checkpoint.as_ref(),
        crate::budget::classify(
            claims.is_service() || (enforce_claim && grant_is_run_kind),
            body.initiated_by.as_deref(),
        ),
    );
    // WS6.D-s: what the organization has left of its agent browser seconds. A spent
    // allowance refuses the start (402 `org_allowance`, once enforcement is `on`; `shadow`
    // records it and serves); what is left becomes a runtime ceiling below. A person's
    // session is never limited here (human seconds are metered, never capped). The static
    // source configures no allowance, so nothing changes until a source reports one.
    let agent_seconds = if run_class == crate::budget::RunClass::Agent {
        let org_limits = state
            .limits
            .org_limits(&org_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        if org_limits.browser_seconds_agent.is_exhausted() {
            match crate::budget::current_mode() {
                crate::budget::BudgetMode::On => {
                    crate::metrics::record_budget_exhausted(
                        crate::limits::DIMENSION_ORG_ALLOWANCE,
                        run_class.label(),
                        "on",
                    );
                    return Err(driver_err(
                        &request_id,
                        crate::limits::AllowanceExhausted {
                            metric: quarry_runtime::usage_metrics::BROWSER_SECOND_AGENT,
                        }
                        .into_error(),
                    ));
                }
                crate::budget::BudgetMode::Shadow => {
                    crate::metrics::record_budget_exhausted(
                        crate::limits::DIMENSION_ORG_ALLOWANCE,
                        run_class.label(),
                        "shadow",
                    );
                    tracing::warn!(
                        class = run_class.label(),
                        "agent run allowance would be exhausted (shadow: not enforced)"
                    );
                }
                crate::budget::BudgetMode::Off => {}
            }
        }
        org_limits.browser_seconds_agent
    } else {
        crate::limits::Allowance::Unconfigured
    };
    let slot = state
        .agent_runs
        .try_reserve(&org_id, run_class)
        .map_err(|denied| driver_err(&request_id, denied.into_error()))?;

    let session = state
        .agent_driver
        .acquire(&lease)
        .await
        .map_err(|e| driver_err(&request_id, e))?;

    // Recovery navigation happens before the first ObservationRunner
    // action. Install the broker-derived boundary immediately after lease
    // acquisition so a resumed checkpoint cannot create its first CDP
    // context with the proxy's default, unconstrained policy.
    if let Err(error) = state
        .agent_driver
        .configure_egress_policy(
            &session,
            super::human_browse::open_web_egress(
                &nav_policy,
                crate::agent_policy_gate::egress_policy(&web_decision, &constraints.allowed_domains),
            ),
        )
        .await
    {
        let _ = state.agent_driver.release(session).await;
        return Err(driver_err(&request_id, error));
    }

    // Rehydrate the last known location when resuming. This is a recovery
    // navigation, not a claimed business action; the next explicit step
    // still receives the first new receipt and observation.
    if let Some(checkpoint) = resume_checkpoint.as_ref() {
        if !checkpoint.current_url.is_empty() {
            let recovery_action = AgentAction::Navigate {
                url: checkpoint.current_url.clone(),
            };
            if !nav_policy.allows(&checkpoint.current_url, &constraints.allowed_domains) {
                let _ = state.agent_driver.release(session).await;
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "durable checkpoint URL is outside the persisted allowed domains",
                ));
            }
            if let Err(error) = crate::agent_policy_gate::gate_url(&web_decision, &checkpoint.current_url) {
                let _ = state.agent_driver.release(session).await;
                return Err(driver_err(&request_id, error));
            }
            if let Err(error) =
                validate_navigation(&recovery_action, state.security.as_ref()).await
            {
                let _ = state.agent_driver.release(session).await;
                return Err(driver_err(&request_id, error));
            }
            if let Err(error) = state
                .agent_driver
                .goto(&session, &checkpoint.current_url)
                .await
            {
                let _ = state.agent_driver.release(session).await;
                return Err(driver_err(&request_id, error));
            }
        }
    }

    let budget_mode = crate::budget::current_mode();
    let budget = (budget_mode != crate::budget::BudgetMode::Off).then(|| {
        crate::budget::RunBudget::start_with_claim(
            crate::budget::BudgetLimits::cached(),
            run_class,
            &constraints,
            grant_budget.as_ref().filter(|_| enforce_claim),
            resume_checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.budget.as_ref()),
            std::time::Instant::now(),
        )
        .with_entitlement(agent_seconds)
        .with_surface(surface)
    });
    let budget_view = budget
        .as_ref()
        .map(|budget| (budget.effective_constraints(budget_mode), budget.clamped()));

    let entry = RunEntry {
        run_id: run_id.clone(),
        org_id: org_id.clone(),
        actor_id,
        session,
        ctx: ObservationContext {
            step: resume_checkpoint.as_ref().map_or(0, |cp| cp.step),
            current_url: resume_checkpoint
                .as_ref()
                .map_or_else(String::new, |cp| cp.current_url.clone()),
            page_hash: resume_checkpoint
                .as_ref()
                .map_or_else(String::new, |cp| cp.page_hash.clone()),
            previous_screenshot: None,
            // A resumed run continues diffing against the checkpointed page
            // identity instead of pretending the recovery navigation is a
            // fresh baseline.
            previous_observation: resume_checkpoint
                .as_ref()
                .filter(|cp| !cp.current_url.is_empty() || !cp.page_hash.is_empty())
                .map(|cp| {
                    ObservationSnapshot::capture(
                        &cp.current_url,
                        None,
                        None,
                        (!cp.page_hash.is_empty()).then_some(cp.page_hash.as_str()),
                    )
                }),
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
        },
        lease,
        constraints,
        zdr,
        grant_id,
        control_mode: resume_checkpoint
            .as_ref()
            .map_or(BrowserControlMode::AgentControl, |checkpoint| {
                checkpoint.control_mode
            }),
        control_epoch: resume_checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.control_epoch),
        profile_scope,
        execution_tier,
        last_observation: None,
        used_sensitive_approval_grants: HashSet::new(),
        budget,
        closed: false,
        last_rejected_epoch: None,
        tab_origins: HashMap::new(),
        nav_policy,
    };
    let live = RunLive::new(
        entry.org_id.clone(),
        entry.actor_id.clone(),
        entry.control_mode,
        entry.control_epoch,
    )
    // The lifetime is the run's effective one: a resumed run's stored constraints
    // are authoritative (see above), so the resume request cannot lengthen it.
    .with_run(run_class, entry.constraints.max_runtime_s.map(u64::from))
    .with_policy(web_decision);

    if !zdr.is_active() {
        let checkpoint = checkpoint_for_entry(&entry);
        if let Err(error) = state.receipts.save_run_checkpoint(checkpoint).await {
            let _ = state.agent_driver.release(entry.session).await;
            return Err(driver_err(&request_id, error));
        }
        let event = BrowserTimelineEvent::new(
            entry.org_id.clone(),
            entry.actor_id.clone(),
            entry.run_id.to_string(),
            BrowserTimelineEventKind::Lifecycle {
                state: BrowserTimelineLifecycle::Started,
                initiated_by: None,
            },
        );
        if let Err(error) = state.receipts.append_browser_timeline_event(event).await {
            let _ = state.agent_driver.release(entry.session).await;
            return Err(driver_err(&request_id, error));
        }
    }

    slot.commit(run_id.to_string(), RunHandle::new(entry, live));

    // WS10.S3: the run's event trace starts here, numbered from seq 1, so the
    // AgentStarted below is the first event of the run's replay ring.
    state.event_sink.open_run(&run_id, 1);

    state
        .event_sink
        .emit_for_zdr(
            zdr,
            run_id.clone(),
            EventType::AgentStarted,
            serde_json::json!({ "org_id": org_id }),
            format!("{run_id}:agent_started"),
        )
        .await;

    Ok(Json(Envelope::ok(
        request_id,
        StartRunData {
            run_id: run_id.to_string(),
            lease_id: lease_id.to_string(),
            profile_id,
            driver_capabilities: state.agent_driver.capabilities(),
            execution_tier,
            execution_engine: execution_tier.engine_label(),
            constraints: budget_view.as_ref().map(|(view, _)| view.clone()),
            clamped: budget_view.map(|(_, clamped)| clamped).unwrap_or_default(),
        },
    )))
}
