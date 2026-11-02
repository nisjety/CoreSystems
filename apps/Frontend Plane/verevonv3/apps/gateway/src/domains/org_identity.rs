//! Organization identity administration (WS05-S6): the owner/admin routes for
//! DNS-proven company domains, proxied to org-core.
//!
//! org-core owns `organization_domains` and the proof. This module is only the
//! edge: it gates on the live owner/admin membership of the session's active
//! organization, rebuilds the upstream path and body from allow-lists (a client
//! `org_id` or role never travels), and rate-limits verification attempts per
//! domain. Every route answers 404 `feature_disabled` unless
//! `ORG_IDENTITY_ADMIN_API` is on; the flag is a rollout control and never an
//! authorization input.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::{authorized_org_id, proxy_json},
};

const ADMIN_ROLES: &[&str] = &["owner", "admin"];
const MAX_BODY_BYTES: usize = 4 << 10;
const MAX_DOMAIN_LEN: usize = 253;
const MAX_ORG_ID_LEN: usize = 128;

/// Verification attempts allowed per organization and domain in a window.
const VERIFY_ATTEMPTS: usize = 6;
const VERIFY_WINDOW: Duration = Duration::from_secs(3600);

/// Attempt times per `org|domain`. Per gateway process: a fleet of replicas
/// multiplies the allowance, and org-core's own lookup timeout bounds the cost
/// of each attempt.
static VERIFY_ATTEMPT_LOG: LazyLock<Mutex<HashMap<String, VecDeque<Instant>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/orgs/{id}/domains",
            get(list_domains).post(add_domain),
        )
        .route("/api/v1/orgs/{id}/domains/{domain}", delete(revoke_domain))
        .route("/api/v1/orgs/{id}/domains/{domain}/verify", post(verify_domain))
        // SSO policy and providers (WS05-S7), same flag and authority.
        .route(
            "/api/v1/orgs/{id}/sso/policy",
            get(get_sso_policy).put(put_sso_policy),
        )
        .route("/api/v1/orgs/{id}/sso/providers", post(add_sso_provider))
        .route(
            "/api/v1/orgs/{id}/sso/providers/{provider_id}",
            delete(remove_sso_provider),
        )
        // `rate_limit_middleware` reads the `AuthenticatedUser` that
        // `require_session` inserts, so it is written first (inner).
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn refusal(status: StatusCode, code: &'static str, message: &str) -> Response {
    (status, Json(error(code, message))).into_response()
}

struct AdminCaller {
    org_id: String,
    actor: ActionActor,
}

/// The caller as org-core must see them: the authorized org (which must be the
/// one in the path) and the live membership role.
async fn admin_caller(
    state: &AppState,
    user: &AuthenticatedUser,
    path_org_id: &str,
) -> Result<AdminCaller, Response> {
    if !state.space_features.org_identity_admin_api {
        return Err(refusal(
            StatusCode::NOT_FOUND,
            "feature_disabled",
            "Identity administration is not enabled.",
        ));
    }
    if !has_authorized_org_role(user, ADMIN_ROLES) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "identity_admin_required",
            "Identity administration requires an organization owner or admin role.",
        ));
    }
    let org_id = authorized_org_id(state, user).await;
    let role = user
        .authorized_membership
        .as_ref()
        .map(|membership| membership.role.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if org_id.trim().is_empty()
        || role.is_empty()
        || org_id.len() > MAX_ORG_ID_LEN
        || org_id.trim() != path_org_id.trim()
    {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "forbidden",
            "The requested organization is not authorized for this session.",
        ));
    }
    Ok(AdminCaller {
        org_id,
        actor: ActionActor {
            user_id: user.user_id.clone(),
            user_email: user.user_email.clone(),
            user_name: user.user_name.clone(),
            user_role: role,
        },
    })
}

/// A domain path segment is a hostname shape only; org-core normalizes and
/// validates it for real, this keeps junk out of the upstream URL.
fn valid_domain_segment(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= MAX_DOMAIN_LEN
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

fn domains_url(state: &AppState, org_id: &str, domain: Option<&str>, verify: bool) -> String {
    let base = format!(
        "{}/orgs/{}/domains",
        state.org_core_url,
        urlencoding::encode(org_id)
    );
    match (domain, verify) {
        (Some(domain), true) => format!("{base}/{}/verify", urlencoding::encode(domain)),
        (Some(domain), false) => format!("{base}/{}", urlencoding::encode(domain)),
        (None, _) => base,
    }
}

/// Whether one more verification of this domain fits the window. The attempt
/// is recorded when allowed. `Err` carries the seconds until the oldest
/// attempt leaves the window.
fn verify_attempt_allowed(org_id: &str, domain: &str, now: Instant) -> Result<(), u64> {
    let key = format!("{org_id}|{}", domain.to_ascii_lowercase());
    let mut log = VERIFY_ATTEMPT_LOG.lock().unwrap_or_else(|e| e.into_inner());
    let attempts = log.entry(key).or_default();
    while attempts
        .front()
        .is_some_and(|at| now.duration_since(*at) >= VERIFY_WINDOW)
    {
        attempts.pop_front();
    }
    if attempts.len() >= VERIFY_ATTEMPTS {
        let oldest = attempts.front().copied().unwrap_or(now);
        let wait = VERIFY_WINDOW.saturating_sub(now.duration_since(oldest));
        return Err(wait.as_secs().max(1));
    }
    attempts.push_back(now);
    if log.len() > 4096 {
        log.retain(|_, v| v.back().is_some_and(|at| now.duration_since(*at) < VERIFY_WINDOW));
    }
    Ok(())
}

async fn forward(
    state: &AppState,
    caller: &AdminCaller,
    method: Method,
    url: String,
    body: Option<Value>,
) -> Response {
    let content_type = body.as_ref().map(|_| "application/json");
    let (status, body) = proxy_json(
        state,
        method,
        &url,
        body,
        Some(&caller.org_id),
        Some(&caller.actor),
        content_type,
    )
    .await;
    (status, body).into_response()
}

async fn list_domains(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let url = domains_url(&state, &caller.org_id, None, false);
    forward(&state, &caller, Method::GET, url, None).await
}

async fn add_domain(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let invalid = || {
        refusal(
            StatusCode::BAD_REQUEST,
            "invalid_domain",
            "The request body must be a JSON object with a domain.",
        )
    };
    if body.len() > MAX_BODY_BYTES {
        return invalid();
    }
    let domain = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(object)) => match object.get("domain").and_then(Value::as_str) {
            Some(domain) if !domain.trim().is_empty() && domain.len() <= 512 => {
                domain.trim().to_owned()
            }
            _ => return invalid(),
        },
        _ => return invalid(),
    };
    // Only `domain` is forwarded; org-core refuses unknown fields.
    let mut forwarded = Map::new();
    forwarded.insert("domain".to_owned(), json!(domain));
    let url = domains_url(&state, &caller.org_id, None, false);
    forward(&state, &caller, Method::POST, url, Some(Value::Object(forwarded))).await
}

async fn verify_domain(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, domain)): Path<(String, String)>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if !valid_domain_segment(&domain) {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_domain",
            "The domain is not valid.",
        );
    }
    if let Err(retry_after) = verify_attempt_allowed(&caller.org_id, &domain, Instant::now()) {
        let mut response = refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many verification attempts for this domain. Try again later.",
        );
        if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return response;
    }
    let url = domains_url(&state, &caller.org_id, Some(&domain), true);
    forward(&state, &caller, Method::POST, url, None).await
}

async fn revoke_domain(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, domain)): Path<(String, String)>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if !valid_domain_segment(&domain) {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_domain",
            "The domain is not valid.",
        );
    }
    let url = domains_url(&state, &caller.org_id, Some(&domain), false);
    forward(&state, &caller, Method::DELETE, url, None).await
}

// --- SSO policy and providers (WS05-S7) ---
//
// Same edge rules as the domain routes: flag, live owner/admin membership, the
// path organization must be the authorized one, and the upstream body is rebuilt
// from an allow-list. The client secret of a provider is write-only: it is
// forwarded once and never logged or echoed by this module.

const MAX_PROVIDER_ID_LEN: usize = 64;
const MAX_FIELD_LEN: usize = 512;
const MAX_ISSUER_LEN: usize = 2048;
const MAX_BREAK_GLASS: usize = 2;

fn sso_url(state: &AppState, org_id: &str, tail: &str) -> String {
    format!(
        "{}/orgs/{}/sso/{tail}",
        state.org_core_url,
        urlencoding::encode(org_id)
    )
}

fn valid_provider_segment(provider_id: &str) -> bool {
    !provider_id.is_empty()
        && provider_id.len() <= MAX_PROVIDER_ID_LEN
        && provider_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn invalid_request(message: &str) -> Response {
    refusal(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn bounded_str<'a>(object: &'a Map<String, Value>, key: &str, max: usize) -> Option<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= max)
}

/// The allow-listed PUT policy body, or the refusal to return.
fn sso_policy_body(body: &Bytes) -> Result<Value, Response> {
    if body.len() > MAX_BODY_BYTES {
        return Err(invalid_request("The request body is too large."));
    }
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(body) else {
        return Err(invalid_request("The request body must be a JSON object."));
    };
    let mode = match bounded_str(&object, "mode", 16) {
        Some(mode @ ("off" | "report" | "enforce")) => mode.to_owned(),
        _ => return Err(invalid_request("mode must be off, report or enforce.")),
    };
    let Some(expected_revision) = object.get("expected_revision").and_then(Value::as_u64) else {
        return Err(invalid_request("expected_revision is required."));
    };
    let mut break_glass = Vec::new();
    match object.get("break_glass_user_ids") {
        None | Some(Value::Null) => {}
        Some(Value::Array(ids)) if ids.len() <= MAX_BREAK_GLASS => {
            for id in ids {
                match id.as_str().map(str::trim) {
                    Some(id) if !id.is_empty() && id.len() <= MAX_ORG_ID_LEN => {
                        break_glass.push(json!(id));
                    }
                    _ => return Err(invalid_request("break_glass_user_ids is not valid.")),
                }
            }
        }
        _ => return Err(invalid_request("break_glass_user_ids is not valid.")),
    }
    let mut forwarded = Map::new();
    forwarded.insert("mode".to_owned(), json!(mode));
    forwarded.insert("break_glass_user_ids".to_owned(), Value::Array(break_glass));
    forwarded.insert("expected_revision".to_owned(), json!(expected_revision));
    Ok(Value::Object(forwarded))
}

/// The allow-listed POST provider body, or the refusal to return.
fn sso_provider_body(body: &Bytes) -> Result<Value, Response> {
    if body.len() > MAX_BODY_BYTES {
        return Err(invalid_request("The request body is too large."));
    }
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(body) else {
        return Err(invalid_request("The request body must be a JSON object."));
    };
    let mut forwarded = Map::new();
    for (key, max) in [
        ("provider_id", MAX_PROVIDER_ID_LEN),
        ("domain", MAX_DOMAIN_LEN),
        ("type", 16),
        ("issuer", MAX_ISSUER_LEN),
        ("client_id", MAX_FIELD_LEN),
        ("client_secret", MAX_FIELD_LEN),
    ] {
        let Some(value) = bounded_str(&object, key, max) else {
            return Err(invalid_request("A provider field is missing or not valid."));
        };
        forwarded.insert(key.to_owned(), json!(value));
    }
    Ok(Value::Object(forwarded))
}

async fn get_sso_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let url = sso_url(&state, &caller.org_id, "policy");
    forward(&state, &caller, Method::GET, url, None).await
}

async fn put_sso_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let forwarded = match sso_policy_body(&body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    let url = sso_url(&state, &caller.org_id, "policy");
    forward(&state, &caller, Method::PUT, url, Some(forwarded)).await
}

async fn add_sso_provider(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let forwarded = match sso_provider_body(&body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    let url = sso_url(&state, &caller.org_id, "providers");
    forward(&state, &caller, Method::POST, url, Some(forwarded)).await
}

async fn remove_sso_provider(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, provider_id)): Path<(String, String)>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if !valid_provider_segment(&provider_id) {
        return invalid_request("The provider id is not valid.");
    }
    let url = sso_url(
        &state,
        &caller.org_id,
        &format!("providers/{}", urlencoding::encode(&provider_id)),
    );
    forward(&state, &caller, Method::DELETE, url, None).await
}
