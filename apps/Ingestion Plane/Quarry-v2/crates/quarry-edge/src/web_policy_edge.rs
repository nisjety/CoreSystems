//! WS7.C: the edge side of web policy on the scrape family. It decides per request,
//! gates each URL before any I/O or cache read, shapes the refusals, and counts them.
//!
//! What a denial carries: the closed capability and decision-source labels, the
//! reason, and (only when ZDR is off) the host, as the core's `PolicyDenial` decides.
//! Never a URL, a path, a query or page text. The counters and the log lines use the
//! same closed labels only.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::web_policy::{LayerError, PolicyDenial, Principal, RequestAsk};
use serde_json::json;

use crate::auth::Claims;
use crate::policy_baseline::{Decision, PolicyBaseline};

/// A request names capabilities; an echoed unknown name is cut to this length.
const ECHO_LIMIT: usize = 64;

/// The principal a verified token is: a service token is autonomous, a user is human.
pub fn principal(claims: &Claims) -> Principal {
    if claims.is_service() {
        Principal::Autonomous
    } else {
        Principal::Human
    }
}

/// Decide the policy for one request. The ask's shape is checked first, in every
/// enforcement mode (a malformed ask is a 400, not a policy decision); then the
/// baseline's mode decides what the ask and the baseline do.
pub fn decide(
    baseline: &PolicyBaseline,
    principal: Principal,
    zdr: bool,
    ask: Option<&RequestAsk>,
) -> Result<Decision, QuarryError> {
    let request = match ask {
        Some(ask) => Some(ask.into_layer().map_err(ask_error)?),
        None => None,
    };
    baseline
        .decide(principal, zdr, request)
        .map_err(|denial| {
            record_denial(&denial, true);
            quarry_error(&denial)
        })
}

/// Gate one URL of the request. Under `observe` the shadow decision is checked first
/// and counted, and the run's own decision (which never denies) decides. Under
/// `enforce` a denied URL is a 403 whose details name the layer that decided.
pub fn gate_url(decision: &Decision, url: &str) -> Result<(), QuarryError> {
    if let Some(shadow) = decision.shadow() {
        if let Err(denial) = shadow.check_url(url) {
            record_denial(&denial, false);
        }
    }
    match decision.policy().check_url(url) {
        Ok(()) => Ok(()),
        Err(denial) => {
            record_denial(&denial, true);
            Err(quarry_error(&denial))
        }
    }
}

/// Whether the decision is the catalogue default for its principal and ZDR posture.
/// Only a default run may read or write the page cache, which holds default results.
pub fn is_default(decision: &Decision) -> bool {
    quarry_runtime::RunPolicy::from(decision.policy())
        .web_policy
        .is_none()
}

/// A malformed ask. An unknown capability is named (capped); anything else is a
/// generic invalid policy with no echo of the caller's value.
fn ask_error(error: LayerError) -> QuarryError {
    match error {
        LayerError::UnknownCapability(raw) => {
            let capability: String = raw.chars().take(ECHO_LIMIT).collect();
            QuarryError::new(
                ErrorCode::BadRequest,
                "the policy names a capability the catalogue does not have",
            )
            .with_details(json!({
                "reason": "unknown_capability",
                "capability": capability,
                "retryable": false,
            }))
        }
        _ => QuarryError::new(
            ErrorCode::BadRequest,
            "the policy asks for something a request may not set",
        )
        .with_details(json!({
            "reason": "invalid_policy",
            "retryable": false,
        })),
    }
}

/// A denial as the error envelope carries it. A domain denial names its capability
/// (`domains.deny` or `domains.allow`), which the core's details do not carry.
pub fn quarry_error(denial: &PolicyDenial) -> QuarryError {
    let mut details = denial.details();
    if let Some(capability) = domain_capability(denial) {
        details["capability"] = json!(capability);
    }
    QuarryError::new(denial.error_code(), denial.message()).with_details(details)
}

fn domain_capability(denial: &PolicyDenial) -> Option<&'static str> {
    match denial {
        PolicyDenial::DomainDenied { .. } => Some("domains.deny"),
        PolicyDenial::DomainNotAllowed { .. } => Some("domains.allow"),
        _ => None,
    }
}

/// The closed (capability, decision source) pair of a denial, for metrics and logs.
fn labels(denial: &PolicyDenial) -> (&'static str, &'static str) {
    match denial {
        PolicyDenial::PolicyDenied {
            capability,
            decision_source,
            ..
        } => (capability.as_str(), decision_source.as_str()),
        PolicyDenial::RawScriptDenied => ("browser.act.script", "invariant"),
        PolicyDenial::PolicyUnreadable => ("org_policy", "org_policy"),
        PolicyDenial::UnknownCapability { .. } => ("unknown", "request"),
        PolicyDenial::CeilingExceeded { capability, .. } => (capability.as_str(), "request"),
        PolicyDenial::DomainDenied {
            decision_source, ..
        } => ("domains.deny", decision_source.as_str()),
        PolicyDenial::DomainNotAllowed {
            decision_source, ..
        } => ("domains.allow", decision_source.as_str()),
    }
}

/// Count and log one denial. `enforced` denials count as `web_policy_denied_total`;
/// the would-be denials of `observe` count as `web_policy_would_deny_total`.
pub fn record_denial(denial: &PolicyDenial, enforced: bool) {
    let (capability, source) = labels(denial);
    let reason = denial.code();
    if enforced {
        metrics::counter!(
            "web_policy_denied_total",
            "capability" => capability,
            "decision_source" => source
        )
        .increment(1);
        tracing::info!(
            target: "quarry.web_policy",
            capability,
            decision_source = source,
            reason,
            "policy.denied"
        );
    } else {
        metrics::counter!(
            "web_policy_would_deny_total",
            "capability" => capability,
            "decision_source" => source
        )
        .increment(1);
        tracing::info!(
            target: "quarry.web_policy",
            capability,
            decision_source = source,
            reason,
            "policy.would_deny"
        );
    }
}

/// The error response of a scrape-family handler, in the shape those handlers return.
pub fn error_response(request_id: &str, error: QuarryError) -> (StatusCode, Response) {
    let status = StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::FORBIDDEN);
    (
        status,
        Json(Envelope::<()>::err(request_id, error)).into_response(),
    )
}

/// The error response of a route that returns `(StatusCode, Json<Envelope<()>>)`.
pub fn envelope_error(request_id: &str, error: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    let status = StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::FORBIDDEN);
    (status, Json(Envelope::<()>::err(request_id, error)))
}

/// The data of the SSE `error` event that carries a refusal on the stream route.
pub fn stream_error_data(request_id: &str, error: QuarryError) -> String {
    serde_json::to_string(&Envelope::<()>::err(request_id, error)).unwrap_or_default()
}
