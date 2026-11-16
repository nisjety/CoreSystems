//! Room browser run start, the model-gateway side of the Control decision (build plan WS20-S2).
//!
//! The run start (WS20-S3) calls these in order, after the zero-retention refusal and before any
//! thread, run or grant is written:
//!
//! 1. [`check_profile_request`]: a room run takes no profile, no approval hold and no ZDR flag.
//! 2. [`verify_start`]: the rollout switch (404 `feature_unavailable` while off), then the Control
//!    decision and its claims, every check in `mp_contracts::space_browser`.
//! 3. [`check_audience_agreement`]: the goal turn's append decision names the same recipient audience
//!    revision (409 `space_authority_stale`).
//! 4. [`check_capacity`]: fewer than two browser runs are active in this Space (429 `browser_capacity`).
//!    The count is Session Core's (`CountActiveRoomBrowserRuns`, WS20-S3), not this module's.
//! 5. [`derive_room_run_grant`]: the narrowed, ephemeral run grant derived from the starter's session
//!    grant, with the room's short lifetime. The grant names no Space (C1).
//!
//! Errors are `{error:{code,message,details}}`. Every verification failure is one fixed 403 whatever
//! the reason, so a response never says which check failed. The rollout switch is a control, never an
//! authorization input. Nothing here holds state: the policy is read once at start-up, and this
//! module has no route of its own.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use mp_contracts::model_plane::v1::{
    browser_broker_client::BrowserBrokerClient, DeriveRunGrantRequest, DeriveRunGrantResponse,
};
use mp_contracts::space_browser::{
    capacity_admits, check_audience_revisions_agree, profile_request_admitted,
    room_grant_ttl_seconds, space_browser_run_api_enabled, verify_browser_run_decision,
    BrowserRunExpectation, BrowserRunRefusal, ControlDecisionKey, VerifiedBrowserRun,
    CONTROL_KEY_ID_ENV, CONTROL_PUBLIC_KEY_ENV, SPACE_BROWSER_RUN_API_ENV,
};
use serde_json::json;
use tonic::transport::Channel;

use crate::auth::VerifiedBrowserBearer;
use crate::browser_run::authenticated_browser_request;

/// The `DeriveRunGrant` purpose of a room run: an ordinary AI run, not an attach.
const PURPOSE_AI_RUN: &str = "ai_run";
/// The grant kind a run grant reports.
const GRANT_KIND_RUN: &str = "run";
/// The budget claim source: the step and runtime bounds come from the verified Space decision.
const BUDGET_SOURCE_REQUEST: &str = "request";

/// The browser run verifier's configuration, read once at start-up. Off, it holds no key.
#[derive(Clone)]
pub struct SpaceBrowserRunPolicy {
    enabled: bool,
    control_key: Option<ControlDecisionKey>,
}

impl SpaceBrowserRunPolicy {
    /// Off: every start answers 404 `feature_unavailable`, and no key is read.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            enabled: false,
            control_key: None,
        }
    }

    /// The policy the raw values mean. With the switch on, the Control key pair is required. The
    /// error names the variable that is missing or invalid, never its value.
    pub fn from_values(
        api: Option<&str>,
        key_id: Option<&str>,
        public_key: Option<&str>,
    ) -> Result<Self, &'static str> {
        if !space_browser_run_api_enabled(api) {
            return Ok(Self::off());
        }
        let control_key = ControlDecisionKey::from_config(key_id, public_key)?;
        Ok(Self {
            enabled: true,
            control_key: Some(control_key),
        })
    }

    /// The process environment right now. Start-up only.
    pub fn from_env() -> Result<Self, &'static str> {
        Self::from_values(
            std::env::var(SPACE_BROWSER_RUN_API_ENV).ok().as_deref(),
            std::env::var(CONTROL_KEY_ID_ENV).ok().as_deref(),
            std::env::var(CONTROL_PUBLIC_KEY_ENV).ok().as_deref(),
        )
    }

    /// Whether room browser run starts are on in this process.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Start-up refusal: with `SPACE_BROWSER_RUN_API` on, the Control key pair must be configured and
/// valid. Off, nothing else is read and the gateway starts as before.
pub fn validate_startup() -> anyhow::Result<()> {
    SpaceBrowserRunPolicy::from_env().map(|_| ()).map_err(|variable| {
        anyhow::anyhow!(
            "{} is on but {} is missing or invalid",
            SPACE_BROWSER_RUN_API_ENV,
            variable
        )
    })
}

/// Refuses the profile fields a room run must not carry (400 `room_browser_profile_forbidden`).
pub fn check_profile_request(
    profile_id_set: bool,
    require_approval_set: bool,
    zdr: bool,
) -> Result<(), Response> {
    if profile_request_admitted(profile_id_set, require_approval_set, zdr) {
        Ok(())
    } else {
        Err(refusal(
            StatusCode::BAD_REQUEST,
            "room_browser_profile_forbidden",
            "a room browser run takes no profile, approval hold or zero-retention flag",
        ))
    }
}

/// Verifies the start's Control decision and its claims. The rollout switch, then the configured
/// key, then the decision. The refusal is ready to return.
pub fn verify_start(
    policy: &SpaceBrowserRunPolicy,
    decision_token: &str,
    claims_json: &str,
    expect: &BrowserRunExpectation<'_>,
) -> Result<VerifiedBrowserRun, Response> {
    if !policy.enabled() {
        return Err(refusal(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "room browser runs are not enabled",
        ));
    }
    let Some(key) = policy.control_key.as_ref() else {
        return Err(refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_decision_unavailable",
            "the Space decision key is not configured",
        ));
    };
    verify_browser_run_decision(key, decision_token, claims_json, expect).map_err(refusal_for)
}

/// The goal turn's append decision and the browser decision must name one recipient audience
/// revision (409 `space_authority_stale`, so the caller re-mints once).
pub fn check_audience_agreement(
    verified: &VerifiedBrowserRun,
    append_audience_revision: i64,
) -> Result<(), Response> {
    check_audience_revisions_agree(verified.recipient_audience_revision, append_audience_revision)
        .map_err(refusal_for)
}

/// Refuses a start while the Space already has its browser runs in flight (429 `browser_capacity`).
/// The caller supplies the count it read from Session Core.
pub fn check_capacity(active_runs: u32) -> Result<(), Response> {
    if capacity_admits(active_runs) {
        Ok(())
    } else {
        Err(refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "browser_capacity",
            "this Space already has the maximum number of browser runs",
        ))
    }
}

/// The narrowed, ephemeral run grant request. The hosts are the decision's (the broker only ever
/// narrows the session grant, so the derive is never wider than its parent), the purpose is `ai_run`,
/// the lifetime is the room's short TTL, and the step and runtime bounds tighten the claim. It carries
/// no Space field (C1) and no money claim, which the decision does not sign.
#[must_use]
pub fn room_run_derive_request(
    parent_grant_id: &str,
    run_key: &str,
    org_id: &str,
    verified: &VerifiedBrowserRun,
) -> DeriveRunGrantRequest {
    DeriveRunGrantRequest {
        parent_grant_id: parent_grant_id.to_owned(),
        session_key: run_key.to_owned(),
        org_id: org_id.to_owned(),
        allowed_domains: verified.claims.allowed_domains.clone(),
        purpose: PURPOSE_AI_RUN.to_owned(),
        ttl_seconds: i32::try_from(room_grant_ttl_seconds(verified.claims.max_runtime_s))
            .unwrap_or(i32::MAX),
        budget_micro_usd: 0,
        budget_max_steps: i32::try_from(verified.claims.max_steps).unwrap_or(i32::MAX),
        budget_max_runtime_s: i32::try_from(verified.claims.max_runtime_s).unwrap_or(i32::MAX),
        budget_source: BUDGET_SOURCE_REQUEST.to_owned(),
        budget_minted_at_unix: 0,
        resource_id: String::new(),
    }
}

/// Asks the broker for the run grant derived from the starter's session grant. The answer must be a
/// run grant under the same parent whose hosts are a subset of the decision's, or it is refused.
pub async fn derive_room_run_grant(
    broker: &mut BrowserBrokerClient<Channel>,
    bearer: &VerifiedBrowserBearer,
    parent_grant_id: &str,
    run_key: &str,
    org_id: &str,
    verified: &VerifiedBrowserRun,
) -> Result<DeriveRunGrantResponse, Response> {
    let request = authenticated_browser_request(
        room_run_derive_request(parent_grant_id, run_key, org_id, verified),
        bearer,
    )
    .map_err(IntoResponse::into_response)?;
    let derived = broker
        .derive_run_grant(request)
        .await
        .map_err(|status| broker_refusal(&status))?
        .into_inner();
    check_derived_grant(&derived, parent_grant_id, verified)?;
    Ok(derived)
}

fn check_derived_grant(
    derived: &DeriveRunGrantResponse,
    parent_grant_id: &str,
    verified: &VerifiedBrowserRun,
) -> Result<(), Response> {
    let grant_id = derived.grant_id.trim();
    let narrower = !derived.allowed_domains.is_empty()
        && derived
            .allowed_domains
            .iter()
            .all(|host| verified.claims.allowed_domains.contains(host));
    if grant_id.is_empty()
        || grant_id == parent_grant_id
        || derived.kind != GRANT_KIND_RUN
        || !narrower
    {
        return Err(refusal(
            StatusCode::BAD_GATEWAY,
            "browser_grant_unavailable",
            "the browser grant for this run could not be issued",
        ));
    }
    Ok(())
}

/// Maps a broker refusal to the closed codes the browser run routes already use.
fn broker_refusal(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::PermissionDenied => refusal(
            StatusCode::FORBIDDEN,
            "browser_grant_forbidden",
            "the browser grant is not allowed",
        ),
        tonic::Code::ResourceExhausted => refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_browser_grants",
            "too many browser grants are active",
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_broker_unavailable",
            "the browser broker is unavailable",
        ),
        tonic::Code::AlreadyExists => refusal(
            StatusCode::CONFLICT,
            "idempotency_key_conflict",
            "this run key already names a different browser grant",
        ),
        _ => refusal(
            StatusCode::BAD_GATEWAY,
            "browser_grant_unavailable",
            "the browser grant for this run could not be issued",
        ),
    }
}

/// Every verification failure is one fixed 403, except the two that tell the caller to re-mint (409).
fn refusal_for(reason: BrowserRunRefusal) -> Response {
    tracing::debug!(reason = reason.label(), "room browser run refused");
    match reason {
        BrowserRunRefusal::Stale | BrowserRunRefusal::Window => refusal(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "the Space authority changed; retry the request",
        ),
        BrowserRunRefusal::Malformed
        | BrowserRunRefusal::Untrusted
        | BrowserRunRefusal::Mismatch
        | BrowserRunRefusal::Policy => refusal(
            StatusCode::FORBIDDEN,
            "space_browser_not_authorized",
            "Space browser run is not authorized",
        ),
    }
}

fn refusal(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}
