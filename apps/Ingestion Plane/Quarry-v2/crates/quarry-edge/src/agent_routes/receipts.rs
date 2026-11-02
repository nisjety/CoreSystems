use super::*;

/// A receipt as a reader may see it (WS12.S3, D2-063). Every receipt passes the secret masking
/// first (`redact_for_read`, idempotent). A receipt read under ZDR then keeps no page URL beyond
/// its host: a ZDR run stores no receipts today (`receipt_plan` is `Skip`), so this is the read
/// side guard for a row written before the run was ZDR, a floor raised since, and any future
/// metadata record. A non-ZDR receipt keeps its URL as the masking left it (secret query and
/// fragment values and userinfo removed). The page URL is the one a reload, a navigation or any
/// other step left on its observation.
pub(super) fn receipt_for_read(mut receipt: StepReceipt, zdr_active: bool) -> StepReceipt {
    receipt.redact_for_read();
    if zdr_active {
        if let Some(observation) = receipt.observation.as_mut() {
            observation.url = host_only_url(&observation.url);
        }
    }
    receipt
}

/// The page URL reduced to scheme and host: no userinfo, path, query or fragment. A tab with no
/// page yet (`about:blank`) names no host and is kept. Anything that does not parse, or has no
/// host, is replaced whole, never kept, so a malformed URL cannot leak through.
pub(super) fn host_only_url(raw: &str) -> String {
    if super::human_browse::is_blank_tab(raw) {
        return raw.to_owned();
    }
    match url::Url::parse(raw) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) if matches!(parsed.scheme(), "http" | "https") => {
                let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
                format!("{}://{host}{port}/", parsed.scheme())
            }
            _ => "[redacted]".to_owned(),
        },
        Err(_) => "[redacted]".to_owned(),
    }
}

/// `GET /v1/agent/runs/{run_id}/receipts` — immutable action history for
/// replay, audit, and human review. The run's verified tenant owns the
/// receipt stream; no caller-supplied org id is accepted.
pub async fn list_receipts(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<Vec<StepReceipt>>>, ApiErr> {
    let request_id = crate::request_id::current();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    // D2-063: the posture the page URL on a receipt is read under. A live run answers with its
    // own; a run read from the durable store answers with the caller's floor.
    let mut zdr_active = claims.zdr_floor();
    let receipts = if let Some(entry_arc) = entry_arc {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        zdr_active |= entry.zdr.is_active();
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
    // WS12.S3: stored evidence is never rewritten, so a receipt written before the masking
    // existed (or while it was not enforcing) may hold raw text. Every read masks it; a
    // receipt already stamped with the current rule version passes through untouched.
    let receipts = receipts
        .into_iter()
        .map(|receipt| receipt_for_read(receipt, zdr_active))
        .collect::<Vec<_>>();
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
    let request_id = crate::request_id::current();
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
    // A legacy row that holds raw typed text must not become a procedure step: mask on read first.
    let receipts = receipts
        .into_iter()
        .map(|mut receipt| {
            receipt.redact_for_read();
            receipt
        })
        .collect::<Vec<_>>();
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
    let request_id = crate::request_id::current();
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
    let request_id = crate::request_id::current();
    Json(Envelope::ok(request_id, assess_quality(&procedure)))
}

/// `POST /v1/agent/procedures/impact-check` — conservative change impact
/// analysis. A changed source URL quarantines affected navigation steps;
/// Quarry does not silently replay or self-heal effectful procedures.
pub async fn procedure_impact_check(
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(body): Json<ProcedureImpactBody>,
) -> Json<Envelope<ProcedureImpactReport>> {
    let request_id = crate::request_id::current();
    Json(Envelope::ok(
        request_id,
        analyze_impact(&body.procedure, &body.changed_urls),
    ))
}

#[cfg(test)]
mod receipt_url_tests {
    use super::*;
    use quarry_core::contracts::{ActionOutcome, BrowserObservation, BrowserTelemetry};
    use quarry_core::redaction::Mode;

    // Assembled at run time so no secret-shaped literal exists in the repository.
    fn jwt() -> String {
        format!(
            "{}.{}.{}",
            ["eyJ", "hbGciOiJIUzI1NiJ9"].concat(),
            ["eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0"].concat(),
            ["SflKxwRJSM", "eKKF2QT4fwpMeJf36POk6yJV_adQssw5c"].concat()
        )
    }

    fn observation(url: &str) -> BrowserObservation {
        BrowserObservation {
            run_id: quarry_core::ids::Id::new(),
            step: 1,
            url: url.to_owned(),
            title: None,
            snapshot: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            visual_observation_artifact_id: None,
            evidence_delta_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            egress_receipts: vec![],
            dialogs: vec![],
            policy_denials: vec![],
            action_outcome: ActionOutcome::verified("reloaded"),
            observation_delta: None,
            challenge: None,
            extraction_profile: None,
            extraction_result: None,
            proof_bundle: None,
            target_resolution: None,
            telemetry: BrowserTelemetry::default(),
            redaction: None,
            observed_at: chrono::Utc::now(),
        }
    }

    fn reload_receipt(url: &str) -> StepReceipt {
        ReceiptBuilder::start("run_x", 3)
            .redaction_mode(Mode::Enforce)
            .complete(
                AgentAction::Reload { ignore_cache: false },
                Some(observation(url)),
                0.0,
            )
    }

    fn page_url() -> String {
        format!(
            "https://user:pw@shop.example.test:8443/orders/42?access_token={}&page=2#frag",
            jwt()
        )
    }

    /// D2-063: a ZDR read keeps the host of the page a reload landed on and nothing after it.
    #[test]
    fn a_zdr_reload_receipt_carries_the_host_only() {
        let receipt = receipt_for_read(reload_receipt(&page_url()), true);
        let url = receipt.observation.as_ref().expect("observation kept").url.clone();
        assert_eq!(url, "https://shop.example.test:8443/");
        let stored = serde_json::to_string(&receipt).unwrap();
        for leaked in ["/orders/42", "page=2", "frag", "user:pw", "eyJ", "access_token"] {
            assert!(!stored.contains(leaked), "{leaked} leaked into {stored}");
        }
        assert!(matches!(receipt.action, AgentAction::Reload { ignore_cache: false }));
    }

    /// D2-063: a non-ZDR receipt keeps its page URL, with the secrets masked out.
    #[test]
    fn a_non_zdr_reload_receipt_carries_the_redacted_url() {
        let receipt = receipt_for_read(reload_receipt(&page_url()), false);
        let url = receipt.observation.as_ref().expect("observation kept").url.clone();
        assert!(url.contains("shop.example.test:8443/orders/42"), "{url}");
        assert!(url.contains("page=2"), "benign query kept: {url}");
        let stored = serde_json::to_string(&receipt).unwrap();
        assert!(!stored.contains("eyJ"), "token masked: {stored}");
        assert!(!stored.contains("user:pw"), "userinfo masked: {stored}");
    }

    /// A receipt with no observation (a failure, a metadata-only row) passes through under ZDR.
    #[test]
    fn a_receipt_without_an_observation_is_unchanged_by_the_zdr_view() {
        let receipt = ReceiptBuilder::start("run_x", 4)
            .redaction_mode(Mode::Enforce)
            .complete_metadata_only(AgentAction::Reload { ignore_cache: true }, 0.0);
        assert!(receipt.observation.is_none());
        let viewed = receipt_for_read(receipt.clone(), true);
        assert!(viewed.observation.is_none());
        assert_eq!(viewed.receipt_id, receipt.receipt_id);
    }

    #[test]
    fn host_only_url_keeps_the_scheme_and_host_and_replaces_what_it_cannot_parse() {
        assert_eq!(host_only_url("https://a.test/p?q=1#f"), "https://a.test/");
        assert_eq!(host_only_url("http://a.test:8080/p"), "http://a.test:8080/");
        assert_eq!(host_only_url("about:blank"), "about:blank");
        for odd in ["", "not a url", "data:text/html,secret", "about:srcdoc", "javascript:alert(1)"] {
            assert_eq!(host_only_url(odd), "[redacted]", "{odd}");
        }
    }
}
