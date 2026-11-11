//! Q3-E3.2-e1w: applying a person's per-origin consent to a live run.
//!
//! `POST /v1/agent/runs/{run_id}/origin-extension` carries the origin-extension grant the broker
//! issued (a child of the run grant; contract in `agent_policy_gate`). `admit_origin_extension`
//! validates it (the grant, the org's caps, deny beats approval) and widens the run's egress; this
//! file is the route around it and the rules that keep the widening honest afterwards:
//!
//! * the accepted extension is kept on the run entry (`RunEntry.origin_extension`);
//! * before every step of either lane, and before a tab is opened or an attachment started, the
//!   edge reads the extension grant again. When it is no longer usable (revoked, expired, a
//!   changed grant, a cap or deny that now applies, the flag turned off) the run's own egress
//!   policy is restored and the extension is dropped (fail closed);
//! * a tab opened later and the agent tab of an attachment start from the widened boundary while
//!   the extension stands.
//!
//! Dark behind `QUARRY_EDGE__ORIGIN_CONSENT` (default off): off, the route answers 403
//! `origin_consent_disabled` before any broker call. Nothing here is persisted, and the only
//! detail a log or a counter carries is a count or a closed label, never a host.

use super::*;

/// The longest grant id the route accepts. A grant id is an opaque broker id; anything longer is a
/// malformed request, refused before any broker call.
const MAX_EXTENSION_GRANT_ID_LEN: usize = 256;

/// `POST .../origin-extension` body.
#[derive(Debug, Deserialize)]
pub struct OriginExtensionBody {
    /// The origin-extension grant the broker issued for this run (a child of the run grant).
    #[serde(alias = "extensionGrantId")]
    pub extension_grant_id: String,
}

/// What an applied extension answers. A scope and a count: never a host name.
#[derive(Debug, Serialize)]
pub struct OriginExtensionData {
    /// `turn`, `conversation` or `always`.
    pub scope: &'static str,
    /// The hosts the run may now also reach, the start host included.
    pub host_count: usize,
}

/// `POST /v1/agent/runs/{run_id}/origin-extension` — apply a person's origin-extension grant to a
/// live run.
///
/// The caller must own the run or be the service principal that started it (the same test as a
/// step); anyone else gets the 404 of an unknown run. The person's lane and the run entry are
/// taken in the documented order (lane, then entry), so the widened boundary lands on the lane's
/// context between two of its actions.
pub async fn apply_origin_extension(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<OriginExtensionBody>,
) -> Result<Json<Envelope<OriginExtensionData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };
    // The ownership check needs no lock, so a stranger is refused before it counts as in flight.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    let grant_id = body.extension_grant_id.trim();
    if grant_id.is_empty() || grant_id.len() > MAX_EXTENSION_GRANT_ID_LEN {
        return Err(status_err(
            StatusCode::BAD_REQUEST,
            &request_id,
            "extension_grant_id must be 1 to 256 characters",
        ));
    }

    // Counts as in flight from before it waits for its lane, so the sweeper never releases a
    // run this call is queued on.
    let _in_flight = handle.live.in_flight_guard();
    // The lane first, then the entry (the order `LaneState` documents).
    let lane_ref = super::step::human_lane_of(&handle).await;
    let mut lane = lane_ref.0.lock().await;
    let mut entry = handle.entry.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // The run was closed while this call waited for its lane.
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }

    let extension = admit_origin_extension(&state, &handle, &entry, &mut lane.ctx, grant_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let data = OriginExtensionData {
        scope: extension.scope.as_str(),
        host_count: extension.all_hosts().len(),
    };
    // The boundary is now on the session and on the lane's context. A later grant replaces this
    // one: the widest boundary a run holds is always the last extension that passed the gate.
    entry.origin_extension = Some(extension);
    entry.origin_extension_lane_stale = false;
    Ok(Json(Envelope::ok(request_id, data)))
}

/// The run's own egress boundary, with no extension: the one it started under (its allow list
/// narrowed by the web policy, the open-web policy and the deny list).
fn base_run_egress(state: &AppState, handle: &RunHandle, entry: &RunEntry) -> BrowserEgressPolicy {
    super::human_browse::run_egress(
        state,
        &entry.nav_policy,
        crate::agent_policy_gate::live_egress_policy(
            state,
            &handle.live,
            entry.zdr.is_active(),
            &entry.constraints.allowed_domains,
        ),
        handle.live.policy(),
    )
}

/// The egress boundary the run holds now: the extended one while an extension stands, the run's
/// own otherwise.
fn current_run_egress(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
) -> BrowserEgressPolicy {
    match entry.origin_extension.as_ref() {
        Some(extension) => super::human_browse::run_egress(
            state,
            &entry.nav_policy,
            crate::agent_policy_gate::extended_egress_policy(
                state,
                &handle.live,
                entry.zdr.is_active(),
                &entry.constraints.allowed_domains,
                extension,
            ),
            handle.live.policy(),
        ),
        None => base_run_egress(state, handle, entry),
    }
}

/// The egress policy for a tab the run opens (`tabs::new_tab`). Without an extension this is the
/// policy the tab route always installed; with one, the widened boundary.
pub(super) fn tab_egress_policy(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
) -> BrowserEgressPolicy {
    if entry.origin_extension.is_some() {
        return current_run_egress(state, handle, entry);
    }
    // E3.8.7: a new tab carries the host reputation hook too (nothing when the mode is off).
    crate::agent_policy_gate::attach_reputation(
        state,
        crate::agent_policy_gate::live_egress_policy(
            state,
            &handle.live,
            entry.zdr.is_active(),
            &entry.constraints.allowed_domains,
        ),
        handle.live.policy(),
    )
}

/// The hosts the agent tab of an attachment may also reach while the run's extension stands (none
/// otherwise). The attachment's own allow list stays the delegate's navigation and landing
/// boundary; these hosts widen only what the tab's page may load or be redirected through.
pub(super) fn attach_extension_hosts(entry: &RunEntry) -> Vec<String> {
    entry
        .origin_extension
        .as_ref()
        .map(crate::agent_policy_gate::OriginExtension::all_hosts)
        .unwrap_or_default()
}

/// Whether the run's stored extension is still usable: the flag, the run (a person's, with a run
/// grant), the run grant and the extension grant read again from the broker, the same extension
/// (hosts and scope unchanged), and the org caps and deny-beats-approval check. Anything else,
/// including a broker that cannot be reached, is "no longer usable" (fail closed).
async fn extension_still_usable(state: &AppState, handle: &RunHandle, entry: &RunEntry) -> bool {
    use crate::agent_policy_gate::{
        check_origin_extension, origin_consent_enabled, origin_extension_from_grant, ExtensionRun,
    };
    let Some(stored) = entry.origin_extension.as_ref() else {
        return false;
    };
    if !origin_consent_enabled() || entry.closed || entry.key_id.is_some() {
        return false;
    }
    let Some(run_grant_id) = entry.grant_id.as_deref() else {
        return false;
    };
    let Ok(extension_grant) = state
        .grant_validator
        .validate_for_org(&entry.org_id, &stored.grant_id)
        .await
    else {
        return false;
    };
    let Ok(run_grant) = state
        .grant_validator
        .validate_for_org(&entry.org_id, run_grant_id)
        .await
    else {
        return false;
    };
    let zdr = entry.zdr.is_active();
    let Ok(current) = origin_extension_from_grant(
        &extension_grant,
        &ExtensionRun {
            parent_grant_id: run_grant_id,
            owner_id: &entry.actor_id,
            zdr,
        },
    ) else {
        return false;
    };
    // A grant that changed under the same id is not the extension the person approved here.
    if current != *stored {
        return false;
    }
    let run_denied: &[String] = match &entry.nav_policy {
        super::human_browse::RunNavPolicy::OpenWeb { denied } => denied,
        super::human_browse::RunNavPolicy::Allowlist => &[],
    };
    check_origin_extension(state, &handle.live, zdr, run_denied, &run_grant, &current).is_ok()
}

/// Drop the run's extension: the session goes back to the run's own egress policy, the agent tab
/// of a live attachment goes back to its attach grant's hosts, and only then is the extension
/// forgotten. When a restore fails the extension stays, so the next call tries again and the
/// caller's action is refused meanwhile (fail closed).
async fn drop_origin_extension(
    state: &AppState,
    handle: &RunHandle,
    entry: &mut RunEntry,
    reason: &'static str,
) -> QuarryResult<()> {
    let base = base_run_egress(state, handle, entry);
    state
        .agent_driver
        .configure_egress_policy(&entry.session, base)
        .await?;
    // The agent tab an attachment opened carries its own policy; take the extension off it too.
    let narrow = handle
        .live
        .attaches()
        .live()
        .filter(|attach| attach.borrowed.is_none() && attach.policy_ask.is_none())
        .map(|attach| (attach.tab_id.clone(), attach.allowed_domains.clone()));
    if let Some((tab_id, allowed_domains)) = narrow {
        state
            .agent_driver
            .swap_tab_egress_policy(
                &entry.session,
                &tab_id,
                Some(crate::agent_policy_gate::attach_reputation(
                    state,
                    BrowserEgressPolicy::from_allowed_domains(&allowed_domains)
                        .with_public_assets(),
                    handle.live.policy(),
                )),
            )
            .await?;
    }
    entry.origin_extension = None;
    // The person's lane still holds the widened policy on its context; its next action rebuilds
    // the boundary from the run's own policy before it runs.
    entry.origin_extension_lane_stale = true;
    metrics::counter!("quarry_origin_extension_revoked_total", "reason" => reason).increment(1);
    tracing::warn!(
        target: "quarry_edge::agent",
        run_id = %entry.run_id,
        reason,
        "origin extension dropped"
    );
    Ok(())
}

/// Revalidate the run's origin extension before an action, and keep the person's lane in step with
/// it. The caller holds the run entry; it also passes the person's lane context when it holds that
/// lane (a step of the person's lane), so a dropped extension is taken off the lane's stored
/// boundary before the action installs it. A lane that is not held is rebuilt at its next action.
///
/// A run with no extension and a lane that is in step costs nothing: no broker call.
pub(super) async fn revalidate_origin_extension(
    state: &AppState,
    handle: &RunHandle,
    entry: &mut RunEntry,
    lane_ctx: Option<&mut ObservationContext>,
) -> QuarryResult<()> {
    if entry.origin_extension.is_some() && !extension_still_usable(state, handle, entry).await {
        drop_origin_extension(state, handle, entry, "grant_not_usable").await?;
    }
    if entry.origin_extension_lane_stale {
        if let Some(ctx) = lane_ctx {
            ctx.egress_policy = Some(current_run_egress(state, handle, entry));
            entry.origin_extension_lane_stale = false;
        }
    }
    Ok(())
}
