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
        // SCIM token management (WS05-S8), same flag and authority.
        .route(
            "/api/v1/orgs/{id}/scim/tokens",
            get(list_scim_tokens).post(create_scim_token),
        )
        .route(
            "/api/v1/orgs/{id}/scim/tokens/{token_id}",
            delete(revoke_scim_token),
        )
        .route("/api/v1/orgs/{id}/scim/status", get(get_scim_status))
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

// --- SCIM token management (WS05-S8) ---
//
// Same edge rules as the domain routes. The bearer token the IdP will use is
// returned by org-core exactly once, in the create response; this module
// forwards it to the admin's browser and never logs or stores it.

const MAX_TOKEN_NAME_LEN: usize = 80;

fn scim_url(state: &AppState, org_id: &str, tail: &str) -> String {
    format!(
        "{}/orgs/{}/scim/{tail}",
        state.org_core_url,
        urlencoding::encode(org_id)
    )
}

fn valid_token_id_segment(token_id: &str) -> bool {
    !token_id.is_empty()
        && token_id.len() <= 36
        && token_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// The allow-listed create body: a name and a lifetime in days.
fn scim_token_body(body: &Bytes) -> Result<Value, Response> {
    if body.len() > MAX_BODY_BYTES {
        return Err(invalid_request("The request body is too large."));
    }
    let object = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(object)) => object,
        _ => return Err(invalid_request("The request body must be a JSON object.")),
    };
    let name = bounded_str(&object, "name", MAX_TOKEN_NAME_LEN)
        .ok_or_else(|| invalid_request("A token name is required."))?;
    let days = object
        .get("expires_in_days")
        .and_then(Value::as_i64)
        .filter(|days| (1..=365).contains(days))
        .ok_or_else(|| invalid_request("expires_in_days must be between 1 and 365."))?;
    let mut forwarded = Map::new();
    forwarded.insert("name".to_owned(), json!(name));
    forwarded.insert("expires_in_days".to_owned(), json!(days));
    Ok(Value::Object(forwarded))
}

async fn list_scim_tokens(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let url = scim_url(&state, &caller.org_id, "tokens");
    forward(&state, &caller, Method::GET, url, None).await
}

async fn create_scim_token(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let forwarded = match scim_token_body(&body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    let url = scim_url(&state, &caller.org_id, "tokens");
    let mut response = forward(&state, &caller, Method::POST, url, Some(forwarded)).await;
    // The response carries the one-time bearer token.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn revoke_scim_token(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, token_id)): Path<(String, String)>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if !valid_token_id_segment(&token_id) {
        return invalid_request("The token id is not valid.");
    }
    let url = scim_url(
        &state,
        &caller.org_id,
        &format!("tokens/{}", urlencoding::encode(&token_id)),
    );
    forward(&state, &caller, Method::DELETE, url, None).await
}

async fn get_scim_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let caller = match admin_caller(&state, &user, &id).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let url = scim_url(&state, &caller.org_id, "status");
    forward(&state, &caller, Method::GET, url, None).await
}

#[cfg(test)]
mod tests {
    //! WS05-S6 / WS05-S7. The handlers are driven through the REAL router with the real
    //! session middleware; auth-core, user-core and org-core are wiremock doubles that
    //! record what the gateway sent.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const COOKIE: &str = "better-auth.session_token=session-1";

    struct Harness {
        auth: MockServer,
        user_core: MockServer,
        org_core: MockServer,
        enabled: bool,
    }

    impl Harness {
        async fn start(org_role: &str, enabled: bool) -> Self {
            let auth = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/get-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                    "session": {"activeOrganizationId": "org-1"}
                })))
                .mount(&auth)
                .await;
            let user_core = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "userId": "user-1", "orgId": "org-1", "role": org_role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            let org_core = MockServer::start().await;
            Mock::given(wiremock::matchers::any())
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"ok": true}})))
                .mount(&org_core)
                .await;
            Self { auth, user_core, org_core, enabled }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.org_core_url = self.org_core.uri();
            state.space_features.org_identity_admin_api = self.enabled;
            state
        }

        async fn send(&self, method: &str, uri: &str, body: Option<&str>) -> (u16, axum::http::HeaderMap, Value) {
            let mut builder = Request::builder().method(method).uri(uri).header("cookie", COOKIE);
            if body.is_some() {
                builder = builder.header("content-type", "application/json");
            }
            let response = crate::build_router(self.state())
                .oneshot(builder.body(Body::from(body.unwrap_or("").to_owned())).unwrap())
                .await
                .unwrap();
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let parsed: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap_or(Value::Null);
            (status, headers, parsed)
        }

        async fn upstream(&self) -> Vec<wiremock::Request> {
            self.org_core.received_requests().await.expect("recording")
        }

        async fn upstream_count(&self) -> usize {
            self.upstream().await.len()
        }
    }

    fn upstream_body(request: &wiremock::Request) -> Value {
        serde_json::from_slice(&request.body).unwrap_or(Value::Null)
    }

    fn code(body: &Value) -> &str {
        body["error"]["code"].as_str().unwrap_or_default()
    }

    /// Every identity route the module mounts for org `org-1`, with a valid body where needed.
    fn all_routes() -> Vec<(&'static str, String, Option<String>)> {
        let policy = r#"{"mode":"report","expected_revision":1}"#.to_owned();
        let provider = json!({
            "provider_id": "acme-oidc", "domain": "example.com", "type": "oidc",
            "issuer": "https://idp.example.com", "client_id": "client-1", "client_secret": "secret-1"
        })
        .to_string();
        vec![
            ("GET", "/api/v1/orgs/org-1/domains".into(), None),
            ("POST", "/api/v1/orgs/org-1/domains".into(), Some(r#"{"domain":"example.com"}"#.into())),
            ("POST", "/api/v1/orgs/org-1/domains/example.com/verify".into(), None),
            ("DELETE", "/api/v1/orgs/org-1/domains/example.com".into(), None),
            ("GET", "/api/v1/orgs/org-1/sso/policy".into(), None),
            ("PUT", "/api/v1/orgs/org-1/sso/policy".into(), Some(policy)),
            ("POST", "/api/v1/orgs/org-1/sso/providers".into(), Some(provider)),
            ("DELETE", "/api/v1/orgs/org-1/sso/providers/acme-oidc".into(), None),
        ]
    }

    // --- pure helpers ---

    #[test]
    fn domain_segments_are_hostname_shaped_only() {
        for ok in ["example.com", "sub-domain.example.co.uk", "xn--bcher-kva.example", "A1.b-2.org"] {
            assert!(valid_domain_segment(ok), "{ok}");
        }
        for bad in ["", "exa mple.com", "a/b.com", "a_b.com", "a:443", "a@b.com", "a?x=1", "a#b", "ex%20ample", "bücher.example"] {
            assert!(!valid_domain_segment(bad), "{bad}");
        }
        assert!(valid_domain_segment(&"a".repeat(MAX_DOMAIN_LEN)));
        assert!(!valid_domain_segment(&"a".repeat(MAX_DOMAIN_LEN + 1)));
    }

    #[test]
    fn provider_segments_are_lowercase_ids_only() {
        for ok in ["acme-oidc", "abc", "a1-b2", "0abc"] {
            assert!(valid_provider_segment(ok), "{ok}");
        }
        for bad in ["", "Acme", "acme_oidc", "acme oidc", "a/b", "../x", "acme.oidc"] {
            assert!(!valid_provider_segment(bad), "{bad}");
        }
        assert!(valid_provider_segment(&"a".repeat(MAX_PROVIDER_ID_LEN)));
        assert!(!valid_provider_segment(&"a".repeat(MAX_PROVIDER_ID_LEN + 1)));
    }

    #[test]
    fn verification_attempts_are_limited_per_org_and_domain_per_window() {
        let now = Instant::now();
        let org = "org-rate-unit";
        let domain = "Rate-Unit.Example.com";
        for attempt in 0..VERIFY_ATTEMPTS {
            assert!(verify_attempt_allowed(org, domain, now).is_ok(), "attempt {attempt}");
        }
        let wait = verify_attempt_allowed(org, "rate-unit.example.com", now)
            .expect_err("the seventh attempt in the window is refused, and the domain is case-insensitive");
        assert!((1..=VERIFY_WINDOW.as_secs()).contains(&wait), "retry after {wait}s");
        // Another domain or another org has its own allowance.
        assert!(verify_attempt_allowed(org, "other-rate-unit.example.com", now).is_ok());
        assert!(verify_attempt_allowed("org-rate-unit-2", domain, now).is_ok());
        // Once the window has passed the allowance returns.
        let later = now + VERIFY_WINDOW + Duration::from_secs(1);
        assert!(verify_attempt_allowed(org, domain, later).is_ok());
    }

    // --- flag, role and organization gates ---

    #[tokio::test]
    async fn flag_off_is_404_feature_disabled_on_every_route_with_no_upstream_call() {
        let harness = Harness::start("owner", false).await;
        for (method, uri, body) in all_routes() {
            let (status, _, parsed) = harness.send(method, &uri, body.as_deref()).await;
            assert_eq!(status, 404, "{method} {uri}: {parsed}");
            assert_eq!(code(&parsed), "feature_disabled", "{method} {uri}");
        }
        assert_eq!(harness.upstream_count().await, 0);
    }

    #[tokio::test]
    async fn a_member_gets_403_identity_admin_required_and_no_upstream_call() {
        for role in ["member", "viewer"] {
            let harness = Harness::start(role, true).await;
            for (method, uri, body) in all_routes() {
                let (status, _, parsed) = harness.send(method, &uri, body.as_deref()).await;
                assert_eq!(status, 403, "{role} {method} {uri}: {parsed}");
                assert_eq!(code(&parsed), "identity_admin_required", "{role} {method} {uri}");
            }
            assert_eq!(harness.upstream_count().await, 0, "{role}");
        }
    }

    #[tokio::test]
    async fn the_path_organization_must_be_the_authorized_one() {
        let harness = Harness::start("owner", true).await;
        for (method, uri, body) in all_routes() {
            let foreign = uri.replace("/orgs/org-1/", "/orgs/org-2/");
            let (status, _, parsed) = harness.send(method, &foreign, body.as_deref()).await;
            assert_eq!(status, 403, "{method} {foreign}: {parsed}");
            assert_eq!(code(&parsed), "forbidden", "{method} {foreign}");
        }
        assert_eq!(harness.upstream_count().await, 0);
    }

    // --- domains (WS05-S6) ---

    #[tokio::test]
    async fn an_admin_reaches_org_core_with_the_session_org_and_role() {
        let harness = Harness::start("admin", true).await;
        let (status, _, parsed) = harness.send("GET", "/api/v1/orgs/org-1/domains", None).await;
        assert_eq!(status, 200, "{parsed}");
        let (status, _, _) = harness.send("DELETE", "/api/v1/orgs/org-1/domains/example.com", None).await;
        assert_eq!(status, 200);
        let requests = harness.upstream().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method.as_str(), "GET");
        assert_eq!(requests[0].url.path(), "/orgs/org-1/domains");
        assert_eq!(requests[1].method.as_str(), "DELETE");
        assert_eq!(requests[1].url.path(), "/orgs/org-1/domains/example.com");
        for request in &requests {
            let header = |name: &str| request.headers.get(name).map(|v| v.to_str().unwrap_or_default().to_owned());
            assert_eq!(header("x-service-id").as_deref(), Some("verevon-gateway"));
            assert_eq!(header("x-org-id").as_deref(), Some("org-1"));
            assert_eq!(header("x-user-id").as_deref(), Some("user-1"));
            assert_eq!(header("x-user-role").as_deref(), Some("admin"));
            assert_eq!(header("x-delegation-version").as_deref(), Some("v3"));
        }
    }

    #[tokio::test]
    async fn only_the_domain_is_forwarded_on_add() {
        let harness = Harness::start("owner", true).await;
        let (status, _, parsed) = harness
            .send(
                "POST",
                "/api/v1/orgs/org-1/domains",
                Some(r#"{"domain":"  Example.COM  ","org_id":"org-2","role":"owner","status":"verified"}"#),
            )
            .await;
        assert_eq!(status, 200, "{parsed}");
        let requests = harness.upstream().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/orgs/org-1/domains");
        assert_eq!(upstream_body(&requests[0]), json!({"domain": "Example.COM"}));
    }

    #[tokio::test]
    async fn a_bad_add_body_is_400_invalid_domain_before_org_core() {
        let harness = Harness::start("owner", true).await;
        let too_long = format!(r#"{{"domain":"{}.com"}}"#, "a".repeat(600));
        let huge = format!(r#"{{"domain":"a.com","pad":"{}"}}"#, "x".repeat(5000));
        for (name, body) in [
            ("not json", "domain=example.com".to_owned()),
            ("array", r#"["example.com"]"#.to_owned()),
            ("no domain", r#"{}"#.to_owned()),
            ("blank", r#"{"domain":"   "}"#.to_owned()),
            ("not a string", r#"{"domain":42}"#.to_owned()),
            ("too long", too_long),
            ("oversized body", huge),
        ] {
            let (status, _, parsed) = harness.send("POST", "/api/v1/orgs/org-1/domains", Some(&body)).await;
            assert_eq!(status, 400, "{name}: {parsed}");
            assert_eq!(code(&parsed), "invalid_domain", "{name}");
        }
        assert_eq!(harness.upstream_count().await, 0);
    }

    #[tokio::test]
    async fn an_invalid_domain_segment_is_400_before_org_core() {
        let harness = Harness::start("owner", true).await;
        for segment in ["under_score.com", "has%20space.com", "a%2Fb.com", "a%3Ab.com"] {
            for (method, suffix) in [("POST", "/verify"), ("DELETE", "")] {
                let uri = format!("/api/v1/orgs/org-1/domains/{segment}{suffix}");
                let (status, _, parsed) = harness.send(method, &uri, None).await;
                assert_eq!(status, 400, "{method} {uri}: {parsed}");
                assert_eq!(code(&parsed), "invalid_domain", "{method} {uri}");
            }
        }
        assert_eq!(harness.upstream_count().await, 0);
    }

    #[tokio::test]
    async fn the_seventh_verify_in_an_hour_is_429_with_retry_after() {
        let harness = Harness::start("owner", true).await;
        // A name no other test uses: the attempt log is process-wide.
        let uri = "/api/v1/orgs/org-1/domains/seventh-verify-ws05s6.example.com/verify";
        for attempt in 1..=VERIFY_ATTEMPTS {
            let (status, _, parsed) = harness.send("POST", uri, None).await;
            assert_eq!(status, 200, "attempt {attempt}: {parsed}");
        }
        let (status, headers, parsed) = harness.send("POST", uri, None).await;
        assert_eq!(status, 429, "{parsed}");
        assert_eq!(code(&parsed), "rate_limited");
        let retry_after: u64 = headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .expect("a numeric Retry-After header");
        assert!((1..=3600).contains(&retry_after), "retry-after {retry_after}");
        assert_eq!(harness.upstream_count().await, VERIFY_ATTEMPTS, "the refused attempt must not reach org-core");

        // The allowance is per domain: another one still goes through.
        let (status, _, _) = harness
            .send("POST", "/api/v1/orgs/org-1/domains/other-verify-ws05s6.example.com/verify", None)
            .await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn an_org_core_refusal_passes_through_with_its_status_and_body() {
        let harness = Harness::start("owner", true).await;
        harness.org_core.reset().await;
        Mock::given(wm_method("DELETE"))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": {"code": "domain_in_use", "message": "in use", "details": {}}
            })))
            .mount(&harness.org_core)
            .await;
        let (status, _, parsed) = harness.send("DELETE", "/api/v1/orgs/org-1/domains/example.com", None).await;
        assert_eq!(status, 409, "{parsed}");
        assert_eq!(code(&parsed), "domain_in_use");
    }

    // --- SSO policy and providers (WS05-S7) ---

    #[tokio::test]
    async fn the_policy_body_is_rebuilt_from_an_allow_list() {
        let harness = Harness::start("owner", true).await;
        let (status, _, parsed) = harness
            .send(
                "PUT",
                "/api/v1/orgs/org-1/sso/policy",
                Some(r#"{"mode":" enforce ","expected_revision":3,"break_glass_user_ids":["u1"," u2 "],"org_id":"org-2","updated_by":"x"}"#),
            )
            .await;
        assert_eq!(status, 200, "{parsed}");
        let requests = harness.upstream().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "PUT");
        assert_eq!(requests[0].url.path(), "/orgs/org-1/sso/policy");
        assert_eq!(
            upstream_body(&requests[0]),
            json!({"mode": "enforce", "break_glass_user_ids": ["u1", "u2"], "expected_revision": 3})
        );

        // Break-glass users are optional.
        let (status, _, _) = harness
            .send("PUT", "/api/v1/orgs/org-1/sso/policy", Some(r#"{"mode":"off","expected_revision":0}"#))
            .await;
        assert_eq!(status, 200);
        assert_eq!(
            upstream_body(&harness.upstream().await[1]),
            json!({"mode": "off", "break_glass_user_ids": [], "expected_revision": 0})
        );
    }

    #[tokio::test]
    async fn a_bad_policy_body_is_400_invalid_request_before_org_core() {
        let harness = Harness::start("owner", true).await;
        let huge = format!(r#"{{"mode":"off","expected_revision":1,"pad":"{}"}}"#, "x".repeat(5000));
        for (name, body) in [
            ("not json", "mode=off".to_owned()),
            ("array", "[]".to_owned()),
            ("unknown mode", r#"{"mode":"block","expected_revision":1}"#.to_owned()),
            ("upper mode", r#"{"mode":"ENFORCE","expected_revision":1}"#.to_owned()),
            ("no mode", r#"{"expected_revision":1}"#.to_owned()),
            ("no revision", r#"{"mode":"off"}"#.to_owned()),
            ("string revision", r#"{"mode":"off","expected_revision":"1"}"#.to_owned()),
            ("negative revision", r#"{"mode":"off","expected_revision":-1}"#.to_owned()),
            ("three break glass", r#"{"mode":"off","expected_revision":1,"break_glass_user_ids":["a","b","c"]}"#.to_owned()),
            ("non-string break glass", r#"{"mode":"off","expected_revision":1,"break_glass_user_ids":[1]}"#.to_owned()),
            ("blank break glass", r#"{"mode":"off","expected_revision":1,"break_glass_user_ids":[" "]}"#.to_owned()),
            ("break glass not a list", r#"{"mode":"off","expected_revision":1,"break_glass_user_ids":"a"}"#.to_owned()),
            ("oversized", huge),
        ] {
            let (status, _, parsed) = harness.send("PUT", "/api/v1/orgs/org-1/sso/policy", Some(&body)).await;
            assert_eq!(status, 400, "{name}: {parsed}");
            assert_eq!(code(&parsed), "invalid_request", "{name}");
        }
        assert_eq!(harness.upstream_count().await, 0);
    }

    #[tokio::test]
    async fn the_provider_body_is_rebuilt_from_an_allow_list() {
        let harness = Harness::start("admin", true).await;
        let (status, _, parsed) = harness
            .send(
                "POST",
                "/api/v1/orgs/org-1/sso/providers",
                Some(
                    &json!({
                        "provider_id": "acme-oidc", "domain": "example.com", "type": "oidc",
                        "issuer": "https://idp.example.com", "client_id": " client-1 ",
                        "client_secret": "secret-1", "org_id": "org-2", "enabled": true
                    })
                    .to_string(),
                ),
            )
            .await;
        assert_eq!(status, 200, "{parsed}");
        let requests = harness.upstream().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/orgs/org-1/sso/providers");
        assert_eq!(
            upstream_body(&requests[0]),
            json!({
                "provider_id": "acme-oidc", "domain": "example.com", "type": "oidc",
                "issuer": "https://idp.example.com", "client_id": "client-1", "client_secret": "secret-1"
            })
        );
        // The client secret is write-only: nothing in the gateway answer repeats it.
        assert!(!parsed.to_string().contains("secret-1"));
    }

    #[tokio::test]
    async fn a_provider_body_missing_a_field_or_over_a_bound_is_400() {
        let harness = Harness::start("owner", true).await;
        let full = json!({
            "provider_id": "acme-oidc", "domain": "example.com", "type": "oidc",
            "issuer": "https://idp.example.com", "client_id": "client-1", "client_secret": "secret-1"
        });
        for field in ["provider_id", "domain", "type", "issuer", "client_id", "client_secret"] {
            let mut body = full.clone();
            body.as_object_mut().unwrap().remove(field);
            let (status, _, parsed) = harness
                .send("POST", "/api/v1/orgs/org-1/sso/providers", Some(&body.to_string()))
                .await;
            assert_eq!(status, 400, "missing {field}: {parsed}");
            assert_eq!(code(&parsed), "invalid_request", "missing {field}");
            let mut blank = full.clone();
            blank[field] = json!("  ");
            let (status, _, _) = harness
                .send("POST", "/api/v1/orgs/org-1/sso/providers", Some(&blank.to_string()))
                .await;
            assert_eq!(status, 400, "blank {field}");
        }
        for (field, len) in [("client_secret", MAX_FIELD_LEN + 1), ("issuer", MAX_ISSUER_LEN + 1), ("provider_id", MAX_PROVIDER_ID_LEN + 1)] {
            let mut body = full.clone();
            body[field] = json!("a".repeat(len));
            let (status, _, _) = harness
                .send("POST", "/api/v1/orgs/org-1/sso/providers", Some(&body.to_string()))
                .await;
            assert_eq!(status, 400, "over-long {field}");
        }
        let (status, _, _) = harness.send("POST", "/api/v1/orgs/org-1/sso/providers", Some("not json")).await;
        assert_eq!(status, 400);
        assert_eq!(harness.upstream_count().await, 0);
    }

    #[tokio::test]
    async fn the_provider_segment_is_validated_and_encoded() {
        let harness = Harness::start("owner", true).await;
        for segment in ["Acme", "acme_oidc", "acme.oidc", "a%2Fb", "a%20b"] {
            let uri = format!("/api/v1/orgs/org-1/sso/providers/{segment}");
            let (status, _, parsed) = harness.send("DELETE", &uri, None).await;
            assert_eq!(status, 400, "{uri}: {parsed}");
            assert_eq!(code(&parsed), "invalid_request", "{uri}");
        }
        assert_eq!(harness.upstream_count().await, 0);

        let (status, _, parsed) = harness.send("DELETE", "/api/v1/orgs/org-1/sso/providers/acme-oidc", None).await;
        assert_eq!(status, 200, "{parsed}");
        let requests = harness.upstream().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "DELETE");
        assert_eq!(requests[0].url.path(), "/orgs/org-1/sso/providers/acme-oidc");
    }

    #[tokio::test]
    async fn get_policy_is_forwarded_for_an_admin() {
        let harness = Harness::start("owner", true).await;
        let (status, _, _) = harness.send("GET", "/api/v1/orgs/org-1/sso/policy", None).await;
        assert_eq!(status, 200);
        let requests = harness.upstream().await;
        assert_eq!(requests[0].method.as_str(), "GET");
        assert_eq!(requests[0].url.path(), "/orgs/org-1/sso/policy");
    }

    /// `ORG_IDENTITY_ADMIN_API` (config.rs) defaults OFF, is OFF under `APP_ENV=production` when
    /// unset, ON in development, and an explicit value wins in both. The compose files set it the
    /// same way (dev on, production false), so a production build never exposes the routes by
    /// accident. Lives here, not in config.rs, so it stays beside the routes it gates.
    #[test]
    fn org_identity_admin_api_is_dev_on_and_production_off() {
        use crate::config::{SpaceFeatureFlags, TEST_ENV_LOCK};

        assert!(!SpaceFeatureFlags::default().org_identity_admin_api);
        let _guard = TEST_ENV_LOCK.blocking_lock();
        let saved: Vec<(&str, Option<String>)> = ["APP_ENV", "ORG_IDENTITY_ADMIN_API"]
            .into_iter()
            .map(|key| (key, std::env::var(key).ok()))
            .collect();
        let read = |app_env: &str, value: Option<&str>| {
            std::env::set_var("APP_ENV", app_env);
            match value {
                Some(value) => std::env::set_var("ORG_IDENTITY_ADMIN_API", value),
                None => std::env::remove_var("ORG_IDENTITY_ADMIN_API"),
            }
            SpaceFeatureFlags::from_env().org_identity_admin_api
        };
        let observed = [
            read("production", None),
            read("prod", None),
            read("development", None),
            read("production", Some("true")),
            read("production", Some("false")),
            read("development", Some("false")),
            read("development", Some("0")),
        ];
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        assert_eq!(observed, [false, false, true, true, false, false, false]);
    }
}
