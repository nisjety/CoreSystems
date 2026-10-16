use super::*;

pub(super) async fn validate_navigation(
    action: &AgentAction,
    security: &dyn SecurityEngine,
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
    if !security.allow_private_hosts() {
        quarry_runtime::dns_guard::guard_url(&parsed).await?;
    }
    Ok(())
}

/// Revalidate the per-run BrowserBroker grant and, in governed mode,
/// prove that the live run is still using the broker's exact canonical
/// host authority. This prevents a caller from presenting a valid grant
/// while retaining a broader client-supplied `allowed_domains` list.
pub(super) async fn validate_run_grant(
    state: &AppState,
    grant_id: &str,
    run_domains: &[String],
) -> QuarryResult<()> {
    let grant = state.grant_validator.validate(grant_id).await?;
    if !grant.is_usable() {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser grant is inactive or expired",
        ));
    }
    if state.require_browser_grants {
        let broker_domains =
            BrowserEgressPolicy::canonical_broker_domains(&grant.allowed_domains)
                .map_err(|reason| QuarryError::new(ErrorCode::SecurityBlocked, reason))?;
        if broker_domains != run_domains {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser run domain policy no longer matches its broker grant",
            ));
        }
    }
    Ok(())
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
    Ok(())
}
