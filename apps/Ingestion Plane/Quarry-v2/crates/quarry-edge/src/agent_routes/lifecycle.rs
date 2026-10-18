use super::*;

/// `POST /v1/agent/runs` — acquire a leased browser session for a new run.
pub async fn start_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(body): Json<StartRunBody>,
) -> Result<Json<Envelope<StartRunData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    if let Some(error) = crate::drain::refusal() {
        return Err(driver_err(&request_id, error));
    }
    let org_id = claims.org_id.clone();
    let actor_id = claims.actor_id().to_owned();
    let execution_tier = body.execution_tier;

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
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "ZDR runs cannot be resumed from durable state",
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
    if let Some(grant_id) = grant_id.as_deref() {
        let grant = state
            .grant_validator
            .validate(grant_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        if !grant.is_usable() {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser grant is inactive or expired",
            ));
        }
        if state.require_browser_grants {
            let broker_domains = BrowserEgressPolicy::canonical_broker_domains(
                &grant.allowed_domains,
            )
            .map_err(|reason| status_err(StatusCode::FORBIDDEN, &request_id, &reason))?;
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
        return Err(status_err(
            StatusCode::BAD_REQUEST,
            &request_id,
            "zdr_persistent_profile_forbidden: a ZDR run cannot request a persistent profile or profile_id",
        ));
    }
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
    let run_class = crate::budget::classify(claims.is_service(), body.initiated_by.as_deref());
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
            BrowserEgressPolicy::from_allowed_domains(&constraints.allowed_domains),
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
            if !domain_allowed(&checkpoint.current_url, &constraints.allowed_domains) {
                let _ = state.agent_driver.release(session).await;
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "durable checkpoint URL is outside the persisted allowed domains",
                ));
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
        crate::budget::RunBudget::start(
            crate::budget::BudgetLimits::cached(),
            crate::budget::classify(claims.is_service(), body.initiated_by.as_deref()),
            &constraints,
            resume_checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.budget.as_ref()),
            std::time::Instant::now(),
        )
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
    };
    let live = RunLive::new(
        entry.org_id.clone(),
        entry.actor_id.clone(),
        entry.control_mode,
        entry.control_epoch,
    )
    .with_run(run_class, body.constraints.max_runtime_s.map(u64::from));

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
            },
        );
        if let Err(error) = state.receipts.append_browser_timeline_event(event).await {
            let _ = state.agent_driver.release(entry.session).await;
            return Err(driver_err(&request_id, error));
        }
    }

    slot.commit(run_id.to_string(), RunHandle::new(entry, live));

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

/// `DELETE /v1/agent/runs/{run_id}` — release the leased session.
pub async fn close_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<serde_json::Value>>, ApiErr> {
    let request_id = RequestKind::new().to_string();

    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    // Ownership first, before touching shared state.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }

    finish_run(&state, &run_id, handle, CloseReason::Explicit)
        .await
        .map_err(|error| driver_err(&request_id, error))?;

    Ok(Json(Envelope::ok(
        request_id,
        serde_json::json!({ "closed": true }),
    )))
}

/// Why a run ended.
pub(super) enum CloseReason {
    /// Its owner closed it.
    Explicit,
    /// The sweeper released it.
    Expired(ExpireVerdict),
}

/// The one path that ends a run, for the owner's `DELETE` and for the sweeper. It
/// writes the durable record first (a failure leaves the run registered and
/// untouched), then marks the run closed so an action that was waiting for the lock
/// is refused, takes it out of the registry and always releases the browser session,
/// even while another request still holds a reference to the run.
///
/// An explicit close also closes the checkpoint; an expiry leaves it `active`, so a
/// non-ZDR run can be resumed. A ZDR run leaves nothing durable either way.
pub(super) async fn finish_run(
    state: &AppState,
    run_id: &str,
    handle: RunHandle,
    reason: CloseReason,
) -> QuarryResult<()> {
    // Waits for an action that is still running.
    let mut entry = handle.entry.lock().await;
    if entry.closed {
        return Ok(());
    }
    let zdr = entry.zdr;
    let rid = entry.run_id.clone();
    if !zdr.is_active() {
        let lifecycle = match reason {
            CloseReason::Explicit => BrowserTimelineLifecycle::Closed,
            CloseReason::Expired(_) => BrowserTimelineLifecycle::Expired,
        };
        state
            .receipts
            .append_browser_timeline_event(BrowserTimelineEvent::new(
                entry.org_id.clone(),
                entry.actor_id.clone(),
                rid.to_string(),
                BrowserTimelineEventKind::Lifecycle { state: lifecycle },
            ))
            .await?;
        if matches!(reason, CloseReason::Explicit) {
            state
                .receipts
                .close_run_checkpoint(&entry.org_id, &rid)
                .await?;
        }
    }

    entry.closed = true;
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .remove(run_id);
    let session = entry.session.clone();
    drop(entry);
    if let Err(error) = state.agent_driver.release(session).await {
        tracing::warn!(run_id, %error, "releasing a run's browser session failed");
    }

    match reason {
        CloseReason::Explicit => {
            state
                .event_sink
                .emit_for_zdr(
                    zdr,
                    rid.clone(),
                    EventType::AgentCompleted,
                    serde_json::json!({}),
                    format!("{rid}:agent_completed"),
                )
                .await;
        }
        CloseReason::Expired(verdict) => {
            state
                .event_sink
                .emit_for_zdr(
                    zdr,
                    rid.clone(),
                    EventType::AgentExpired,
                    serde_json::json!({
                        "reason": verdict.reason.as_str(),
                        "idle_s": verdict.idle_s,
                        "age_s": verdict.age_s,
                    }),
                    format!("{rid}:agent_expired"),
                )
                .await;
        }
    }
    Ok(())
}

/// Release every live browser session. Runs concurrently (8 at a time) with
/// `per_run_timeout` on each release so one hung Chromium cannot hold up the
/// rest. A non-ZDR run's checkpoint is left `Active`, so it can be resumed
/// with `resume_run_id` after the restart; nothing is written for a ZDR run.
pub async fn drain_runs(
    runs: &AgentRuns,
    driver: &Arc<dyn quarry_browser::BrowserDriver>,
    per_run_timeout: std::time::Duration,
) -> DrainReport {
    use futures_util::StreamExt;
    let snapshot: Vec<(String, Arc<TokioMutex<RunEntry>>)> = runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .drain()
        .map(|(run_id, handle)| (run_id, handle.entry))
        .collect();
    let outcomes: Vec<Option<bool>> = futures_util::stream::iter(snapshot)
        .map(|(run_id, entry)| {
            let driver = driver.clone();
            async move {
                match Arc::try_unwrap(entry) {
                    Ok(mutex) => {
                        let entry = mutex.into_inner();
                        match tokio::time::timeout(
                            per_run_timeout,
                            driver.release(entry.session),
                        )
                        .await
                        {
                            Ok(Ok(())) => Some(true),
                            Ok(Err(error)) => {
                                tracing::warn!(%run_id, %error, "drain: release failed");
                                Some(false)
                            }
                            Err(_) => {
                                tracing::warn!(%run_id, "drain: release timed out");
                                Some(false)
                            }
                        }
                    }
                    Err(_) => {
                        tracing::warn!(%run_id, "drain: a request still holds this run");
                        None
                    }
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    let mut report = DrainReport::default();
    for outcome in outcomes {
        match outcome {
            Some(true) => report.released += 1,
            Some(false) => report.failed += 1,
            None => report.in_use += 1,
        }
    }
    report
}
