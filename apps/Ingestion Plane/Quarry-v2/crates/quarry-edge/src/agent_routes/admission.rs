use super::*;

/// The run-less navigation check. Every production navigation path (steps, attach,
/// tab open, resume recovery) now goes through `validate_navigation_for_run` (WS8.B),
/// so this is kept for the tests that exercise the engine's own policy.
#[cfg(test)]
pub(super) async fn validate_navigation(
    action: &AgentAction,
    security: &dyn SecurityEngine,
) -> QuarryResult<()> {
    validate_navigation_with(action, security, false).await
}

/// `validate_navigation` for a live run whose navigation authority is known
/// (D2-059). An open-web run keeps the DNS guard on even when the engine allows
/// private hosts: its grant is "any public host", decided by name in gate 11, so
/// only the resolution can refuse a public name that points at a private address.
pub(super) async fn validate_navigation_for_run(
    action: &AgentAction,
    security: &dyn SecurityEngine,
    run_nav: &super::human_browse::RunNavPolicy,
) -> QuarryResult<()> {
    validate_navigation_with(action, security, run_nav.is_open_web()).await
}

async fn validate_navigation_with(
    action: &AgentAction,
    security: &dyn SecurityEngine,
    open_web: bool,
) -> QuarryResult<()> {
    let AgentAction::Navigate { url } = action else {
        return Ok(());
    };
    let parsed = Url::parse(url).map_err(|error| {
        QuarryError::new(
            ErrorCode::BadRequest,
            format!("invalid navigation URL: {error}"),
        )
    })?;
    let verdict = security.preflight(&parsed).await;
    if verdict.decision == Decision::Block {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            verdict.reasons.join("; "),
        ));
    }
    if quarry_runtime::dns_guard::dns_guard_required(security.allow_private_hosts(), open_web) {
        quarry_runtime::dns_guard::guard_url(&parsed).await?;
    }
    Ok(())
}

/// Revalidate the per-run BrowserBroker grant and, in governed mode,
/// prove that the live run is still using the broker's exact canonical
/// host authority. This prevents a caller from presenting a valid grant
/// while retaining a broader client-supplied `allowed_domains` list.
///
/// WS7.K: returns the web-policy snapshot the broker stored on the grant it just
/// revalidated, read as the grant layer of the step (`GrantSnapshot::from_grant`).
pub(super) async fn validate_run_grant(
    state: &AppState,
    org_id: &str,
    grant_id: &str,
    run_key_id: Option<&str>,
    run_domains: &[String],
    run_nav: &super::human_browse::RunNavPolicy,
) -> QuarryResult<crate::agent_policy_gate::GrantSnapshot> {
    let grant = state.grant_validator.validate_for_org(org_id, grant_id).await?;
    if !grant.is_usable() {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser grant is inactive or expired",
        ));
    }
    // INNH-10: a key run stays bound to the key it started under. A grant that answers for
    // another key, or for none, is refused, so a grant swapped under a live run stops it.
    match run_key_id {
        Some(run_key) if grant.key_id != run_key => {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser grant no longer belongs to this key",
            ));
        }
        // And a key's grant never backs a run that is not a key's.
        None if !grant.key_id.is_empty() => {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser grant belongs to an API key",
            ));
        }
        _ => {}
    }
    // A key run always compares the broker's policy, as `require_browser_grants` does.
    if state.require_browser_grants || run_key_id.is_some() {
        // The broker's mode decides the comparison (WS8.B): an allow-list is the
        // exact host list, an open-web grant is the same denied list. Any other
        // pairing, or an unknown mode, is a mismatch.
        let broker = grant
            .nav_policy()
            .map_err(|reason| QuarryError::new(ErrorCode::SecurityBlocked, reason))?;
        if !super::human_browse::broker_matches_run(&broker, run_nav, run_domains) {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser run domain policy no longer matches its broker grant",
            ));
        }
    }
    Ok(crate::agent_policy_gate::GrantSnapshot::from_grant(&grant))
}

pub(super) fn domain_allowed(url: &str, allowed_domains: &[String]) -> bool {
    BrowserEgressPolicy::from_allowed_domains(allowed_domains).allows_url(url)
}

/// Defense-in-depth ZDR guard for `POST /v1/agent/runs`, independent of
/// (and not reliant on) the Verevon gateway's own `effective_profile_scope`
/// check. The gateway already rejects a client-supplied
/// `{zdr: true, profileId: "<real>"}` combination before ever proxying to
/// Quarry-edge (`fix(gateway): close ZDR bypass via explicit ephemeral
/// scope claim`), but Quarry-edge is the layer that actually launches the
/// browser and calls `ProfileStore::save` on release — any other direct
/// caller of this route (a different consumer, a future service, a bug
/// upstream) must not be able to make a ZDR run's cookies/storage durable
/// just by supplying a `profile_id` or `persist_profile: true`. Without
/// this check, `persist_profile = body.persist_profile ||
/// body.profile_id.is_some()` ignored `zdr` entirely, so `close_run` →
/// `agent_driver.release()` → `persist_current_page()` (gated only on
/// `lease.persist_profile`, which has no notion of ZDR at all — see
/// `quarry_core::lease::BrowserLease`) would happily write a ZDR
/// session's cookies into a named profile.
pub(super) fn zdr_forbids_persistent_profile(
    zdr: ZdrMode,
    persist_profile_flag: bool,
    has_profile_id: bool,
) -> bool {
    zdr.is_active() && (persist_profile_flag || has_profile_id)
}

/// An interactive run gives a remote page the ability to initiate further
/// network activity after the initial URL has been admitted. Entry-point
/// URL checks are not sufficient for that authority: the selected driver
/// must prove that every redirect and subresource request remains behind
/// Quarry's isolated egress boundary. Do this before acquiring a session
/// so an unsafe provider cannot create durable browser state merely by
/// starting an agent run.
pub(super) fn require_isolated_agent_egress(state: &AppState) -> QuarryResult<()> {
    let capabilities = state.agent_driver.capabilities();
    if capabilities.isolated_egress && capabilities.security_evidence {
        return Ok(());
    }

    Err(QuarryError::new(
        ErrorCode::Unsupported,
        "agent browser execution requires verified isolated egress and current security evidence",
    ))
}

pub(super) fn experimental_lite_enabled() -> bool {
    std::env::var("QUARRY_LITE_EXPERIMENTAL")
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

/// Enforce the Lite contract independently of whichever renderer is used.
/// It admits anonymous evidence acquisition and bounded snapshot-based DOM
/// interaction only; it cannot carry auth state, transfer files, resolve a
/// host path, or expose a raw page scripting channel.
pub(super) fn require_ephemeral_evidence_action(action: &AgentAction) -> QuarryResult<()> {
    if matches!(
        action,
        AgentAction::Navigate { .. }
            | AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::Wait { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::Scroll { .. }
            | AgentAction::Screenshot { .. }
            | AgentAction::Pdf
            | AgentAction::GetContent
    ) {
        Ok(())
    } else {
        Err(QuarryError::new(
            ErrorCode::Forbidden,
            "action is unavailable in the ephemeral_evidence execution tier",
        ))
    }
}

pub(super) fn require_driver_capabilities(
    requirements: &BrowserDriverRequirements,
    capabilities: BrowserDriverCapabilities,
) -> QuarryResult<()> {
    let mut missing = Vec::new();
    if requirements.persistent_profile && !capabilities.persistent_profile {
        missing.push("persistent_profile");
    }
    if requirements.devtools_trace && !capabilities.devtools_trace {
        missing.push("devtools_trace");
    }
    if requirements.downloads_to_artifacts && !capabilities.downloads_to_artifacts {
        missing.push("downloads_to_artifacts");
    }
    if requirements.uploads_from_artifacts && !capabilities.uploads_from_artifacts {
        missing.push("uploads_from_artifacts");
    }
    if requirements.full_visual_fidelity && !capabilities.full_visual_fidelity {
        missing.push("full_visual_fidelity");
    }
    if requirements.isolated_egress && !capabilities.isolated_egress {
        missing.push("isolated_egress");
    }
    if requirements.security_evidence && !capabilities.security_evidence {
        missing.push("security_evidence");
    }
    if requirements.atomic_target_actions && !capabilities.atomic_target_actions {
        missing.push("atomic_target_actions");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "selected browser driver does not satisfy the requested capabilities",
        )
        .with_details(serde_json::json!({ "missing": missing })))
    }
}

/// Refuse raw page script (`AgentAction::Evaluate`) before anything is
/// consumed: no approval grant, no budget step, no receipt. The script is
/// never logged or echoed.
pub(super) fn deny_raw_script(action: &AgentAction, run_id: &str) -> QuarryResult<()> {
    if matches!(action, AgentAction::Evaluate { .. }) {
        tracing::warn!(
            target: "quarry_edge::agent",
            run_id,
            action = "evaluate",
            "raw page script execution denied"
        );
        return Err(QuarryError::raw_script_denied());
    }
    Ok(())
}

/// Refuse a page reload when the verified initiator is an agent (WS9.B, D2-012): a person's
/// reload goes through, an agent's never does, before anything is consumed (no approval, no
/// budget step, no receipt). The initiator is the one `derive_initiator` gave from the
/// token, so a hint cannot make an agent a person.
pub(super) fn deny_agent_reload(
    action: &AgentAction,
    initiator: StepInitiator,
    run_id: &str,
) -> QuarryResult<()> {
    if initiator == StepInitiator::Agent && matches!(action, AgentAction::Reload { .. }) {
        tracing::warn!(
            target: "quarry_edge::agent",
            run_id,
            action = "reload",
            "page reload by an agent denied"
        );
        return Err(QuarryError::agent_reload_denied());
    }
    Ok(())
}

pub(super) fn require_action_capabilities(
    action: &AgentAction,
    capabilities: BrowserDriverCapabilities,
) -> QuarryResult<()> {
    let uses_snapshot_target = matches!(
        action,
        AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::ClickSemantic { .. }
            | AgentAction::TypeSemantic { .. }
            | AgentAction::SelectSemantic { .. }
            | AgentAction::WaitForSemantic { .. }
            | AgentAction::UploadRef { .. }
            | AgentAction::FrameUploadRef { .. }
            | AgentAction::DownloadRef { .. }
            | AgentAction::FrameDownloadRef { .. }
            | AgentAction::FillOtpRef { .. }
    );
    if uses_snapshot_target && !capabilities.atomic_target_actions {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "selected browser driver cannot execute snapshot targets atomically",
        ));
    }
    if matches!(
        action,
        AgentAction::DownloadRef { .. } | AgentAction::FrameDownloadRef { .. }
    ) && !capabilities.downloads_to_artifacts
    {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "selected browser driver cannot provide governed artifact-only downloads",
        ));
    }
    if matches!(
        action,
        AgentAction::UploadRef { .. } | AgentAction::FrameUploadRef { .. }
    ) && !capabilities.uploads_from_artifacts
    {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "selected browser driver cannot provide governed artifact-only uploads",
        ));
    }
    // E3.1-c2: a one-time-code fill needs a driver that checks the fill
    // preconditions and inserts trusted input (`QUARRY_EDGE__FILL_OTP` on).
    if matches!(action, AgentAction::FillOtpRef { .. }) && !capabilities.otp_fill {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "selected browser driver cannot fill one-time codes",
        ));
    }
    Ok(())
}

/// What an admitted action runs under: who drove it and the control epoch it was
/// admitted in (both stamped on its receipt).
#[derive(Debug, Clone)]
pub(super) struct Admitted {
    pub initiator: StepInitiator,
    /// Which agent, when `initiator` is `Agent`.
    pub agent: Option<AgentIdentity>,
    pub control_epoch: u64,
    /// A person held the browser when the step was admitted. A person's step then never
    /// counts against the run's step budget.
    pub takeover: bool,
    /// Q3-E3.1-h2: for a `fill_otp_ref` step, the mail connection and origin its approval grant
    /// is bound to (checked non-empty and https in `validate_sensitive_approval`). `None` for
    /// every other action. The step puts it, with the run's actor, on the observation context.
    pub otp_binding: Option<OtpGrantBinding>,
    /// WS7.K: the web-policy snapshot of the run grant revalidated at gate 4 (`Absent` for a
    /// run with no grant, and for a tab operation, which revalidates the grant itself).
    pub grant_snapshot: crate::agent_policy_gate::GrantSnapshot,
    /// WS7.K: the org layer of the step caller's own token (`Absent` when the caller was not
    /// passed, or claim mode is `off`).
    pub step_claim: quarry_core::web_policy::OrgInput,
}

impl Admitted {
    /// The step layers this action was admitted under. Every visit check of the same step
    /// (the landed page, a tab's URL) applies the same layers as gate 8 did.
    pub(super) fn step_layers(&self) -> crate::agent_policy_gate::StepLayers<'_> {
        crate::agent_policy_gate::StepLayers {
            grant: &self.grant_snapshot,
            claim: &self.step_claim,
        }
    }
}

pub(super) struct AdmitInput<'a> {
    pub initiator: Initiator,
    pub action: &'a AgentAction,
    /// Refuse (not just log) an agent action while a person holds the browser.
    pub enforce_takeover: bool,
    /// True only after Quarry has admitted the signed delegate for the
    /// owner's specific borrowed tab and checked its current holder.
    pub consented_tab_control: bool,
    /// WS8.H: the last observation of the lane this action runs on (the approval checks read
    /// the dialogs and targets it names). `None` before the lane has observed anything.
    pub last_observation: Option<&'a BrowserObservation>,
    /// WS7.K: the verified claims of the request's caller. Their `web_policy` claim is the
    /// org layer gate 8 checks beside the run's decision; `None` checks no claim layer.
    pub caller: Option<&'a crate::auth::Claims>,
}

/// 409 `USER_TOOK_OVER` with the state that caused it: `control_mode`, `control_epoch`
/// and, when the edge knows it, `taken_over_at` (RFC 3339, when the person took the
/// browser; absent for a run resumed into a takeover, whose start the checkpoint does
/// not carry).
pub(super) fn user_took_over(snapshot: ControlSnapshot, taken_over_at_ms: Option<u64>) -> QuarryError {
    let mut details = serde_json::json!({
        "control_mode": "human_takeover",
        "control_epoch": snapshot.epoch,
    });
    if let Some(at) = taken_over_at_ms
        .and_then(|ms| i64::try_from(ms).ok())
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
    {
        details["taken_over_at"] = serde_json::json!(at.to_rfc3339());
    }
    QuarryError::new(
        ErrorCode::UserTookOver,
        "the person using the browser has taken control; this action was not run",
    )
    .with_details(details)
}

/// The control gate shared by actions and tab operations: while a person holds
/// the browser only a person's request goes through. In observe mode
/// (`QUARRY_EDGE__TAKEOVER_ENFORCEMENT` off) a request that would be refused is
/// logged and let through.
pub(super) fn takeover_gate(
    handle: &RunHandle,
    entry: &RunEntry,
    initiator: StepInitiator,
    enforce: bool,
    what: &str,
) -> Result<ControlSnapshot, QuarryError> {
    let snapshot = handle.live.snapshot();
    if snapshot.mode == BrowserControlMode::HumanTakeover && initiator != StepInitiator::Human {
        if enforce {
            return Err(user_took_over(snapshot, handle.live.taken_over_at_ms()));
        }
        tracing::warn!(
            run_id = %entry.run_id,
            control_epoch = snapshot.epoch,
            what,
            "would_reject: agent request during human takeover (enforcement is off)"
        );
    }
    Ok(snapshot)
}

/// A refused agent action leaves one `skipped` receipt per takeover, so the
/// history shows what the agent tried without growing with every retry. ZDR runs
/// enforce the gate and persist nothing.
async fn record_takeover_skip(
    state: &AppState,
    entry: &mut RunEntry,
    action: &AgentAction,
    initiator: &Initiator,
    snapshot: ControlSnapshot,
) {
    if entry.last_rejected_epoch == Some(snapshot.epoch) {
        return;
    }
    entry.last_rejected_epoch = Some(snapshot.epoch);
    if entry.zdr.is_active() {
        return;
    }
    let receipt = ReceiptBuilder::start(entry.run_id.to_string(), entry.summary.step)
        .org_id(entry.org_id.clone())
        .actor_id(entry.actor_id.clone())
        .initiator(initiator.class, snapshot.epoch)
        .agent(initiator.agent.clone())
        .skip(action.clone(), "user_took_over");
    if let Err(error) = state.receipts.append(receipt).await {
        // zdr-log-ok: a ZDR run returns above, before the receipt is built; only a non-ZDR run reaches this line, and the store error text is what an operator needs
        tracing::warn!(%error, "could not record the skipped receipt for a refused agent action");
    }
}

/// The one ordered chain every browser action goes through, whichever surface it
/// arrives on (REST step, WebSocket, interact). Cheap, local checks first; the
/// gate that consumes state (the one-shot approval) comes last among the policy
/// checks, so a refusal never burns an approval or a budget step.
///
/// Order (pinned by `admission_order_is_pinned`):
/// 1 the run is still live, 2 the caller owns it (done by the caller: it needs the
/// lock-free `RunLive` or the entry), 3 control, 4 grant, 5 driver capabilities,
/// 6 ephemeral tier, 7 raw script (and an agent's reload), 8 web policy (slot for WS7), 9 budget,
/// 10 sensitive approval, 11 navigation domain, 12 navigation security.
pub(super) async fn admit_action(
    state: &AppState,
    handle: &RunHandle,
    entry: &mut RunEntry,
    input: AdmitInput<'_>,
) -> QuarryResult<Admitted> {
    let action = input.action;

    // 1. closed
    if entry.closed {
        return Err(QuarryError::new(ErrorCode::NotFound, "agent run is not live")
            .with_details(serde_json::json!({ "reason": "run_not_live" })));
    }

    // 3. control / takeover
    let snapshot = if input.consented_tab_control {
        // The person's run stays in human mode. The exact borrowed tab, with
        // its own holder, is the agent's only authority in that run.
        handle.live.snapshot()
    } else {
        match takeover_gate(handle, entry, input.initiator.class, input.enforce_takeover, "action") {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let snapshot = handle.live.snapshot();
                record_takeover_skip(state, entry, action, &input.initiator, snapshot).await;
                return Err(error);
            }
        }
    };

    // 4. grant (WS7.K: its web-policy snapshot is the grant layer of gate 8)
    let grant_snapshot = if let Some(grant_id) = entry.grant_id.as_deref() {
        validate_run_grant(
            state,
            &entry.org_id,
            grant_id,
            entry.key_id.as_deref(),
            &entry.constraints.allowed_domains,
            &entry.nav_policy,
        )
        .await?
    } else if state.require_browser_grants || entry.key_id.is_some() {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "browser grant is required for every action",
        ));
    } else {
        crate::agent_policy_gate::GrantSnapshot::Absent
    };

    // INNH-10: an API key takes no sensitive action: there is no person to sign an ask. Refused
    // on the kind of the action, ahead of the driver's capabilities, with nothing consumed.
    // (Gate 10 holds the same line for any action that would still name an approval.)
    if entry.key_id.is_some() && is_sensitive_action(action) {
        return Err(crate::api_key::refusal_error(
            crate::api_key::reason::SENSITIVE_ACTION_REFUSED,
        ));
    }

    // 5. driver capabilities
    require_action_capabilities(action, state.agent_driver.capabilities())?;
    // 6. ephemeral tier
    if entry.execution_tier.is_ephemeral_evidence() {
        require_ephemeral_evidence_action(action)?;
    }
    // 7. raw script
    deny_raw_script(action, &entry.run_id.to_string())?;
    deny_agent_reload(action, input.initiator.class, &entry.run_id.to_string())?;
    // 8. web policy (WS7.D): the run's resolved decision, then (WS7.K) the grant snapshot and
    // the step caller's claim, which only tighten it. Before the budget and the one-shot
    // approval, so a refusal consumes neither.
    let step_claim = input
        .caller
        .map(|caller| crate::agent_policy_gate::step_claim(&state.policy, caller))
        .unwrap_or(quarry_core::web_policy::OrgInput::Absent);
    crate::agent_policy_gate::gate_policy_for_step(
        state,
        &handle.live,
        entry.zdr.is_active(),
        action,
        &crate::agent_policy_gate::StepLayers {
            grant: &grant_snapshot,
            claim: &step_claim,
        },
    )?;

    // 9. budget: the request's own step cap where the gate does not own it, then the
    // gate, whose first refusal per segment is announced before the 402 is returned.
    let takeover = snapshot.mode == BrowserControlMode::HumanTakeover;
    if super::human_browse::counts_against_step_budget(input.initiator.class, takeover, entry.budget.as_ref()) {
        let person_step = input.initiator.class == StepInitiator::Human;
        if crate::budget::legacy_step_cap_applies(entry.budget.as_ref(), crate::budget::current_mode(), person_step) {
            enforce_step_budget(entry)
                .map_err(|message| QuarryError::new(ErrorCode::RateLimited, message))?;
        }
        if let Err(error) = budget_gate(entry, action) {
            announce_budget_refusal(state, entry, &handle.live).await;
            return Err(error);
        }
    }

    // 10. sensitive approval (the only gate that consumes state)
    let sensitive = sensitive_approval_for_action(entry, action, input.last_observation);
    // INNH-10: there is no person to sign an ask for an API key, so a key run takes no
    // sensitive action at all: refused before any broker call. An action that would need an
    // approval but cannot name one (a stale target) is refused the same way.
    if entry.key_id.is_some() && !matches!(sensitive, Ok(None)) {
        return Err(crate::api_key::refusal_error(
            crate::api_key::reason::SENSITIVE_ACTION_REFUSED,
        ));
    }
    let mut otp_binding = None;
    if let Some(mut approval) = sensitive? {
        validate_sensitive_approval(state, &entry.org_id, &mut approval).await?;
        otp_binding = approval.otp_binding.take();
        if !entry
            .used_sensitive_approval_grants
            .insert(approval.grant_id)
        {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                "sensitive browser approval grant has already been used by this run",
            ));
        }
    }

    // 11. navigation domain
    if let AgentAction::Navigate { url } = action {
        if !entry.nav_policy.allows(url, &entry.constraints.allowed_domains) {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser navigation is outside the run's allowed domains",
            ));
        }
    }

    // 12. navigation security (an open-web run is DNS-guarded whatever allow_private_hosts says)
    validate_navigation_for_run(action, state.security.as_ref(), &entry.nav_policy).await?;

    Ok(Admitted {
        initiator: input.initiator.class,
        agent: input.initiator.agent,
        control_epoch: snapshot.epoch,
        takeover,
        otp_binding,
        grant_snapshot,
        step_claim,
    })
}

/// The control gate for tab operations (open, select, close): they move the
/// person's browser too, so an agent cannot do them during a takeover.
pub(super) fn admit_tab_operation(
    handle: &RunHandle,
    entry: &RunEntry,
    initiator: impl Into<Initiator>,
    enforce_takeover: bool,
) -> QuarryResult<Admitted> {
    let initiator = initiator.into();
    if entry.closed {
        return Err(QuarryError::new(ErrorCode::NotFound, "agent run is not live")
            .with_details(serde_json::json!({ "reason": "run_not_live" })));
    }
    let snapshot = takeover_gate(handle, entry, initiator.class, enforce_takeover, "tab")?;
    Ok(Admitted {
        initiator: initiator.class,
        agent: initiator.agent,
        control_epoch: snapshot.epoch,
        takeover: snapshot.mode == BrowserControlMode::HumanTakeover,
        otp_binding: None,
        grant_snapshot: crate::agent_policy_gate::GrantSnapshot::Absent,
        step_claim: quarry_core::web_policy::OrgInput::Absent,
    })
}

/// E3.2-e1: apply a person's per-origin consent to a live run. The origin-extension grant the
/// broker issued (a child of the run grant; contract in `agent_policy_gate`) is validated, the
/// run grant is revalidated as gate 4 does it, any deny beats the approval, and only then is the
/// run's browser egress widened through `configure_egress_policy`. The widened boundary is
/// stored on the lane's context, so every later action of that lane installs it again
/// (`ObservationRunner` reads `ctx.egress_policy`). Nothing is persisted: the boundary is
/// process-local, a resumed run starts from its grant again, and a ZDR run holds a
/// turn-scoped extension only.
///
/// What this does not widen: the navigation gate (gate 11) still holds an agent's own
/// `navigate` to the run's allow list; the extension covers what the page reaches (an SSO
/// redirect, a frame, a CDN, a link the person follows). Flag `QUARRY_EDGE__ORIGIN_CONSENT`,
/// default off: off, the call is refused with 403 before any broker call.
///
/// Q3-E3.2-e1w: `origin_extension::apply_origin_extension` is the route that calls this, and
/// stores the returned extension on the run entry for the per-step revalidation.
pub(super) async fn admit_origin_extension(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    ctx: &mut ObservationContext,
    extension_grant_id: &str,
) -> QuarryResult<crate::agent_policy_gate::OriginExtension> {
    admit_origin_extension_with(
        crate::agent_policy_gate::origin_consent_enabled(),
        state,
        handle,
        entry,
        ctx,
        extension_grant_id,
    )
    .await
}

/// [`admit_origin_extension`] with the flag passed in (tests).
pub(super) async fn admit_origin_extension_with(
    enabled: bool,
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    ctx: &mut ObservationContext,
    extension_grant_id: &str,
) -> QuarryResult<crate::agent_policy_gate::OriginExtension> {
    use crate::agent_policy_gate::{
        check_origin_extension, extended_egress_policy, origin_extension_from_grant,
        ExtensionRun, OriginExtensionRefusal,
    };

    let refuse = |refusal: OriginExtensionRefusal| {
        metrics::counter!("quarry_origin_extension_refused_total", "reason" => refusal.as_str())
            .increment(1);
        tracing::warn!(
            target: "quarry_edge::agent",
            run_id = %entry.run_id,
            reason = refusal.as_str(),
            "origin extension refused"
        );
        refusal.to_error()
    };

    if !enabled {
        return Err(refuse(OriginExtensionRefusal::FlagOff));
    }
    if entry.closed {
        return Err(QuarryError::new(ErrorCode::NotFound, "agent run is not live")
            .with_details(serde_json::json!({ "reason": "run_not_live" })));
    }
    // An API key has no person to consent (INNH-10).
    if entry.key_id.is_some() {
        return Err(refuse(OriginExtensionRefusal::ApiKeyRun));
    }
    let Some(run_grant_id) = entry.grant_id.as_deref() else {
        return Err(refuse(OriginExtensionRefusal::GrantInvalid));
    };

    // The run grant first, exactly as gate 4 checks it, then its snapshot for the org caps and
    // the grant layer.
    validate_run_grant(
        state,
        &entry.org_id,
        run_grant_id,
        entry.key_id.as_deref(),
        &entry.constraints.allowed_domains,
        &entry.nav_policy,
    )
    .await?;
    let run_grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, run_grant_id)
        .await?;

    let extension_grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, extension_grant_id)
        .await?;
    let zdr = entry.zdr.is_active();
    let extension = origin_extension_from_grant(
        &extension_grant,
        &ExtensionRun {
            parent_grant_id: run_grant_id,
            owner_id: &entry.actor_id,
            zdr,
        },
    )
    .map_err(refuse)?;

    // Any deny beats the approval: the org caps, the run's decision, the grant snapshot, and the
    // denied list of an open-web run.
    let run_denied: &[String] = match &entry.nav_policy {
        super::human_browse::RunNavPolicy::OpenWeb { denied } => denied,
        super::human_browse::RunNavPolicy::Allowlist => &[],
    };
    check_origin_extension(state, &handle.live, zdr, run_denied, &run_grant, &extension)
        .map_err(refuse)?;

    // Widen the boundary. An open-web run already reaches every public host that is not denied,
    // and an unconstrained allow list needs no widening; both keep their boundary.
    let egress = super::human_browse::run_egress(
        state,
        &entry.nav_policy,
        extended_egress_policy(
            state,
            &handle.live,
            zdr,
            &entry.constraints.allowed_domains,
            &extension,
        ),
        handle.live.policy(),
    );
    state
        .agent_driver
        .configure_egress_policy(&entry.session, egress.clone())
        .await?;
    ctx.egress_policy = Some(egress);

    metrics::counter!("quarry_origin_extension_applied_total", "scope" => extension.scope.as_str())
        .increment(1);
    // Counts and closed labels only: never a host.
    tracing::info!(
        target: "quarry_edge::agent",
        run_id = %entry.run_id,
        scope = extension.scope.as_str(),
        hosts = extension.hosts.len() + 1,
        "origin extension applied"
    );
    Ok(extension)
}

#[cfg(test)]
mod dns_guard_tests {
    use super::super::human_browse::RunNavPolicy;
    use super::*;

    fn private_navigation() -> AgentAction {
        AgentAction::Navigate {
            url: "http://127.0.0.1/admin".to_owned(),
        }
    }

    #[tokio::test]
    async fn open_web_navigation_is_dns_guarded_under_allow_private_hosts() {
        // D2-059: allow_private_hosts relaxes the allow-list path (local
        // fixtures) but never an open-web grant. A literal keeps this DNS-free.
        let security = quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true);
        let open_web = RunNavPolicy::OpenWeb { denied: Vec::new() };

        let error = validate_navigation_for_run(&private_navigation(), &security, &open_web)
            .await
            .expect_err("an open-web run never reaches a private address");
        assert_eq!(error.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn allowlist_navigation_keeps_the_allow_private_hosts_relaxation() {
        let security = quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true);

        validate_navigation_for_run(&private_navigation(), &security, &RunNavPolicy::Allowlist)
            .await
            .expect("allow-list runs keep the dev/test relaxation unchanged");
        validate_navigation(&private_navigation(), &security)
            .await
            .expect("the run-less entry point is unchanged");
    }

    #[tokio::test]
    async fn strict_engine_guards_both_modes() {
        let security = quarry_security::preflight::DefaultEngine::new();
        for nav in [RunNavPolicy::Allowlist, RunNavPolicy::OpenWeb { denied: Vec::new() }] {
            let error = validate_navigation_for_run(&private_navigation(), &security, &nav)
                .await
                .expect_err("a strict engine refuses a private target in every mode");
            assert_eq!(error.code, ErrorCode::SecurityBlocked);
        }
    }
}
