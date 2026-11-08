//! INNH-10: API-key agent runs, the principal fence (dark behind `QUARRY_EDGE__API_KEY_AGENT_RUNS`).
//!
//! An API key never reaches Quarry. Auth Core exchanges it for a short-lived token of the
//! `api_key` principal type (`Claims::is_api_key`), and the BrowserBroker issues an ordinary
//! allow-list session grant stamped with the key's id. This module is the first of Quarry's
//! three gates for such a caller; the other two are the start binding
//! (`agent_routes::api_key_runs`) and the per-step rules (`agent_routes::admission`).
//!
//! The fence is a middleware layered next to `require_service_route_scope`. For an `api_key`
//! principal it answers, in this order, and every refusal is a 403 with `error.details.reason`:
//!
//! 1. the switch is off: `api_key_agent_runs_disabled`, on EVERY route (a key token reaching an
//!    edge that did not opt in does nothing, not even a read);
//! 2. the token lacks `browser:agent`: `api_key_scope_missing`;
//! 3. the method and path are not on [`route_allowed`]'s allow-list: `api_key_route_refused`.
//!
//! The allow-list is closed and written by method and path. Anything not named is refused, so
//! a route added to the router later is refused for a key until it is named here.
//! User and service tokens pass through untouched.

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};

use crate::auth::{Claims, SCOPE_BROWSER_AGENT};

/// The closed list of `error.details.reason` values this feature adds (`docs/ERROR_REASONS.md`).
/// `per_key_run_cap` (a 429) is in the run registry's list, not here.
pub mod reason {
    pub const DISABLED: &str = "api_key_agent_runs_disabled";
    pub const SCOPE_MISSING: &str = "api_key_scope_missing";
    pub const ROUTE_REFUSED: &str = "api_key_route_refused";
    pub const GRANT_REQUIRED: &str = "api_key_grant_required";
    pub const GRANT_KIND_REFUSED: &str = "api_key_grant_kind_refused";
    pub const OPEN_WEB_REFUSED: &str = "api_key_open_web_refused";
    pub const GRANT_OWNER_MISMATCH: &str = "api_key_grant_owner_mismatch";
    pub const GRANT_BUDGET_REQUIRED: &str = "api_key_grant_budget_required";
    pub const GRANT_HOSTS_REQUIRED: &str = "api_key_grant_hosts_required";
    pub const PROFILE_REFUSED: &str = "api_key_profile_refused";
    pub const RESUME_REFUSED: &str = "api_key_resume_refused";
    pub const SENSITIVE_ACTION_REFUSED: &str = "api_key_sensitive_action_refused";
    pub const BUDGET_NOT_ENFORCED: &str = "api_key_budget_not_enforced";

    /// Every reason above, for the docs parity test.
    pub const ALL: [&str; 13] = [
        DISABLED,
        SCOPE_MISSING,
        ROUTE_REFUSED,
        GRANT_REQUIRED,
        GRANT_KIND_REFUSED,
        OPEN_WEB_REFUSED,
        GRANT_OWNER_MISMATCH,
        GRANT_BUDGET_REQUIRED,
        GRANT_HOSTS_REQUIRED,
        PROFILE_REFUSED,
        RESUME_REFUSED,
        SENSITIVE_ACTION_REFUSED,
        BUDGET_NOT_ENFORCED,
    ];
}

/// The 403 for a refused API-key request: a fixed sentence per reason (no caller data) and
/// `details.reason`.
#[must_use]
pub fn refusal_error(reason: &'static str) -> QuarryError {
    let message = match reason {
        reason::DISABLED => "API-key agent runs are not enabled on this edge",
        reason::SCOPE_MISSING => "the API-key token does not carry the browser:agent scope",
        reason::ROUTE_REFUSED => "this route is not available to an API key",
        reason::GRANT_REQUIRED => "an API-key run needs a browser grant_id",
        reason::GRANT_KIND_REFUSED => "this browser grant kind cannot back an API-key run",
        reason::OPEN_WEB_REFUSED => "an API-key run cannot use an open-web or human-browse grant",
        reason::GRANT_OWNER_MISMATCH => "the browser grant was not issued to this API key",
        reason::GRANT_BUDGET_REQUIRED => "an API-key run needs a browser grant with a budget",
        reason::GRANT_HOSTS_REQUIRED => "an API-key run needs a browser grant with an allowed host list",
        reason::PROFILE_REFUSED => "an API-key run cannot use or persist a browser profile",
        reason::RESUME_REFUSED => "an API-key run cannot be resumed",
        reason::SENSITIVE_ACTION_REFUSED => "an API-key run cannot perform a sensitive browser action",
        reason::BUDGET_NOT_ENFORCED => "API-key runs need run budget enforcement to be on",
        _ => "this request is refused for an API key",
    };
    QuarryError::new(ErrorCode::Forbidden, message).with_details(serde_json::json!({ "reason": reason }))
}

fn refusal_response(reason: &'static str) -> Response {
    let request_id = crate::request_id::current();
    (
        StatusCode::FORBIDDEN,
        Json(Envelope::<()>::err(&request_id, refusal_error(reason))),
    )
        .into_response()
}

/// An id segment of a run or tab: non-empty, bounded, and only characters that cannot change
/// the meaning of a path.
fn id_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-' | b'~'))
}

/// The allow-list of an API key: method and path, nothing else.
///
/// `POST /v1/agent/runs`, `POST /v1/agent/runs/:run_id/step`, `GET .../receipts`,
/// `GET .../egress-receipts`, `GET|POST .../tabs`, `POST .../tabs/:tab_id/select`,
/// `DELETE .../tabs/:tab_id`, `GET .../events/stream`, `DELETE /v1/agent/runs/:run_id`.
///
/// Refused by omission: `/interact`, `browser-session*` (control, takeover, timeline),
/// `attach*`, `live-view`, `frame`, `frames/*`, `devtools`, `procedure*`, `tabs/:id/control`,
/// `/v1/profiles*`, `/v1/artifacts*`, and every scrape, crawl, search, extract, job, schedule,
/// change, team and fleet route. A trailing slash, an empty segment and a method outside the
/// table (HEAD included) are refused.
#[must_use]
pub fn route_allowed(method: &Method, path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/v1/agent/runs") else {
        return false;
    };
    if rest.is_empty() {
        return method == Method::POST;
    }
    let Some(rest) = rest.strip_prefix('/') else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [run] if id_segment(run) => method == Method::DELETE,
        [run, "step"] if id_segment(run) => method == Method::POST,
        [run, "receipts"] | [run, "egress-receipts"] if id_segment(run) => method == Method::GET,
        [run, "tabs"] if id_segment(run) => method == Method::GET || method == Method::POST,
        [run, "tabs", tab, "select"] if id_segment(run) && id_segment(tab) => method == Method::POST,
        [run, "tabs", tab] if id_segment(run) && id_segment(tab) => method == Method::DELETE,
        [run, "events", "stream"] if id_segment(run) => method == Method::GET,
        _ => false,
    }
}

/// The fence's decision for an API-key request. Pure, so the table is unit-tested.
/// `Err` is the `details.reason`.
pub fn fence_decision(
    enabled: bool,
    has_scope: bool,
    method: &Method,
    path: &str,
) -> Result<(), &'static str> {
    if !enabled {
        return Err(reason::DISABLED);
    }
    if !has_scope {
        return Err(reason::SCOPE_MISSING);
    }
    if !route_allowed(method, path) {
        return Err(reason::ROUTE_REFUSED);
    }
    Ok(())
}

/// Middleware: layer it after `require_auth`, beside `require_service_route_scope`. A user or
/// service token is not touched; an `api_key` token is held to [`fence_decision`].
pub async fn require_api_key_route_fence(req: Request, next: Next) -> Response {
    let verdict = {
        let Some(claims) = req.extensions().get::<Claims>() else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        if !claims.is_api_key() {
            None
        } else {
            let decision = fence_decision(
                crate::run_limits::api_key_agent_runs_enabled(),
                claims.has_scope(SCOPE_BROWSER_AGENT),
                req.method(),
                req.uri().path(),
            );
            if let Err(reason) = decision {
                tracing::warn!(
                    reason,
                    key_id = claims.key_id().unwrap_or(""),
                    org_id = %claims.org_id,
                    "api key request refused at the route fence"
                );
            }
            Some(decision)
        }
    };
    match verdict {
        None | Some(Ok(())) => next.run(req).await,
        Some(Err(reason)) => refusal_response(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = "run_01J0000000000000000000000";

    #[test]
    fn the_allow_list_is_exactly_the_documented_routes() {
        let allowed = [
            (Method::POST, "/v1/agent/runs".to_owned()),
            (Method::DELETE, format!("/v1/agent/runs/{RUN}")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/step")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/receipts")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/egress-receipts")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/tabs")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/tabs")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/tabs/TAB1/select")),
            (Method::DELETE, format!("/v1/agent/runs/{RUN}/tabs/TAB1")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/events/stream")),
        ];
        for (method, path) in &allowed {
            assert!(route_allowed(method, path), "{method} {path} must be allowed");
        }
        // Every allowed path refuses every other method.
        let methods = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::HEAD,
            Method::OPTIONS,
        ];
        for (allowed_method, path) in &allowed {
            for method in &methods {
                let expected = allowed
                    .iter()
                    .any(|(m, p)| m == method && p == path);
                assert_eq!(
                    route_allowed(method, path),
                    expected,
                    "{method} {path} (allowed method {allowed_method})"
                );
            }
        }
    }

    #[test]
    fn everything_else_a_person_or_service_can_reach_is_refused() {
        let refused = [
            (Method::POST, format!("/v1/agent/runs/{RUN}/interact")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/browser-session")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/browser-session/control")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/browser-session/timeline")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/attach")),
            (Method::DELETE, format!("/v1/agent/runs/{RUN}/attach/ATT1")),
            (Method::POST, format!("/v1/agent/runs/{RUN}/procedure")),
            (Method::POST, "/v1/agent/procedures/replay-check".to_owned()),
            (Method::POST, "/v1/agent/procedures/quality-check".to_owned()),
            (Method::POST, "/v1/agent/procedures/impact-check".to_owned()),
            (Method::POST, format!("/v1/agent/runs/{RUN}/tabs/TAB1/control")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/devtools")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/frames/ws")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/frames/stream")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/frame")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/live-view")),
            (Method::GET, "/v1/agent/runs".to_owned()),
            (Method::GET, "/v1/profiles".to_owned()),
            (Method::POST, "/v1/profiles".to_owned()),
            (Method::GET, "/v1/artifacts".to_owned()),
            (Method::GET, "/v1/artifacts/art_1".to_owned()),
            (Method::POST, "/v1/scrape".to_owned()),
            (Method::POST, "/v1/crawl".to_owned()),
            (Method::POST, "/v1/search".to_owned()),
            (Method::POST, "/v1/extract".to_owned()),
            (Method::GET, "/v1/jobs/job_1".to_owned()),
            (Method::GET, "/v1/team/credit-usage".to_owned()),
            (Method::POST, "/graphql".to_owned()),
            (Method::GET, "/v1/fleets".to_owned()),
            (Method::POST, "/v1/internal/schedules/preflight".to_owned()),
            (Method::POST, "/v1/agent/runs/".to_owned()),
            (Method::POST, format!("/v1/agent/runs/{RUN}/step/")),
            (Method::POST, "/v1/agent/runs//step".to_owned()),
            (Method::POST, "/v1/agent/runsx".to_owned()),
            (Method::DELETE, "/v1/agent/runs/../profiles".to_owned()),
            (Method::DELETE, "/v1/agent/runs/%2e%2e".to_owned()),
            (Method::POST, format!("/v1/agent/runs/{RUN}/step/extra")),
            (Method::GET, format!("/v1/agent/runs/{RUN}/tabs/TAB1/select")),
            (Method::GET, "/".to_owned()),
            (Method::GET, String::new()),
        ];
        for (method, path) in &refused {
            assert!(!route_allowed(method, path), "{method} {path:?} must be refused");
        }
    }

    #[test]
    fn the_decision_checks_the_switch_then_the_scope_then_the_route() {
        let ok = Method::POST;
        // Off: every route, scope or not, allowed or not.
        for path in ["/v1/agent/runs", "/v1/scrape", "/v1/profiles", "/graphql"] {
            for scope in [true, false] {
                assert_eq!(
                    fence_decision(false, scope, &ok, path),
                    Err(reason::DISABLED),
                    "{path} scope={scope}"
                );
            }
        }
        // On, no scope: the scope is checked before the route.
        assert_eq!(
            fence_decision(true, false, &ok, "/v1/agent/runs"),
            Err(reason::SCOPE_MISSING)
        );
        assert_eq!(
            fence_decision(true, false, &ok, "/v1/scrape"),
            Err(reason::SCOPE_MISSING)
        );
        // On, scope: the route decides.
        assert_eq!(fence_decision(true, true, &ok, "/v1/agent/runs"), Ok(()));
        assert_eq!(
            fence_decision(true, true, &ok, "/v1/scrape"),
            Err(reason::ROUTE_REFUSED)
        );
    }

    #[test]
    fn every_reason_has_a_sentence_and_is_documented() {
        let docs = include_str!("../../../docs/ERROR_REASONS.md");
        for reason in reason::ALL {
            let error = refusal_error(reason);
            assert_ne!(
                error.message, "this request is refused for an API key",
                "{reason} has no sentence of its own"
            );
            assert_eq!(error.code.http_status(), 403);
            assert_eq!(error.details.as_ref().unwrap()["reason"], reason);
            assert!(docs.contains(&format!("`{reason}`")), "{reason} is not in docs/ERROR_REASONS.md");
        }
        let unique: std::collections::HashSet<_> = reason::ALL.iter().collect();
        assert_eq!(unique.len(), reason::ALL.len());
    }
}
