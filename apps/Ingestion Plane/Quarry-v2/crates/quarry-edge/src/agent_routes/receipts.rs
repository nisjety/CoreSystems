use super::*;

/// `GET /v1/agent/runs/{run_id}/receipts` — immutable action history for
/// replay, audit, and human review. The run's verified tenant owns the
/// receipt stream; no caller-supplied org id is accepted.
pub async fn list_receipts(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<Vec<StepReceipt>>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
    };
    let receipts = if let Some(entry_arc) = entry_arc {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        state
            .receipts
            .list_for_actor(&entry.org_id, &entry.actor_id, &entry.run_id)
            .await
            .map_err(|e| driver_err(&request_id, e))?
    } else {
        // Receipts are durable independently of the live Chromium
        // session. An actor-scoped predicate is required even after an
        // edge restart; legacy rows without this binding remain hidden.
        let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
            status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found")
        })?;
        let receipts = state
            .receipts
            .list_for_actor(&claims.org_id, claims.actor_id(), &parsed_run_id)
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        if receipts.is_empty() {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        receipts
    };
    Ok(Json(Envelope::ok(request_id, receipts)))
}

/// `POST /v1/agent/runs/{run_id}/procedure` — compile a deterministic
/// replay candidate from a tenant-owned verified receipt stream. Unknown,
/// failed, or unobserved effects are rejected by the runtime compiler;
/// Model Plane remains the planner and policy owner.
pub async fn compile_run_procedure(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<CompileProcedureBody>,
) -> Result<Json<Envelope<BrowserProcedure>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let parsed_run_id = run_id
        .parse::<RunKind>()
        .map_err(|_| status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found"))?;
    let receipts = state
        .receipts
        .list_for_actor(&claims.org_id, claims.actor_id(), &parsed_run_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    if receipts.is_empty() {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run has no durable receipts",
        ));
    }
    let procedure_id = body
        .procedure_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("proc_{run_id}"));
    let procedure = compile_procedure(procedure_id, &receipts)
        .map_err(|error| driver_err(&request_id, error))?;
    Ok(Json(Envelope::ok(request_id, procedure)))
}

/// `POST /v1/agent/procedures/replay-check` — compare a proposed action
/// sequence with a compiled procedure without executing browser effects.
/// Any mismatch is an explicit repair/refusal decision, never a silent
/// selector or business-action substitution.
pub async fn replay_check(
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(body): Json<ReplayCheckBody>,
) -> Json<Envelope<ReplayDecision>> {
    let request_id = RequestKind::new().to_string();
    Json(Envelope::ok(
        request_id,
        compare_replay(&body.procedure, &body.actions),
    ))
}

/// `POST /v1/agent/procedures/quality-check` — deterministic promotion
/// report. This never changes rollout state; it supplies evidence for the
/// owning release/Model workflow.
pub async fn procedure_quality_check(
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(procedure): Json<BrowserProcedure>,
) -> Json<Envelope<ProcedureQualityReport>> {
    let request_id = RequestKind::new().to_string();
    Json(Envelope::ok(request_id, assess_quality(&procedure)))
}

/// `POST /v1/agent/procedures/impact-check` — conservative change impact
/// analysis. A changed source URL quarantines affected navigation steps;
/// Quarry does not silently replay or self-heal effectful procedures.
pub async fn procedure_impact_check(
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(body): Json<ProcedureImpactBody>,
) -> Json<Envelope<ProcedureImpactReport>> {
    let request_id = RequestKind::new().to_string();
    Json(Envelope::ok(
        request_id,
        analyze_impact(&body.procedure, &body.changed_urls),
    ))
}
