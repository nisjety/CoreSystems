use axum::{
    body::Body,
    http::{header::SET_COOKIE, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_with_defaults,
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
};

/// Per-user TTL for the cached session-context lookup. Short enough that an org
/// switch reflects quickly, long enough to collapse the repeated user-core round
/// trips that billing / ingestions / session bootstrap would otherwise each make.
const SESSION_CONTEXT_TTL_SECS: u64 = 60;

/// The org the session context is scoped to: the session's active organization
/// (the org the user is currently acting as, from the validated Better Auth
/// session — never a client header) when present, else `None` so user-core
/// resolves the user's primary membership. Pure, so the cache-key and `x-org-id`
/// derivation below is unit-testable.
pub(crate) fn scope_org_id(user: &AuthenticatedUser) -> Option<&str> {
    user.active_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Resolve user-core's session-context for the authenticated user, cached per
/// `(user_id, active_org)` for [`SESSION_CONTEXT_TTL_SECS`]. Returns the unwrapped
/// context object (`{ userId, orgId, role, onboardingStatus, ... }`) or
/// `Value::Null` on failure.
///
/// Scoped to the session's active organization (forwarded to user-core as
/// `x-org-id`) so `role`/`onboardingStatus` reflect the org the user is currently
/// acting as rather than always the primary membership. The cache key includes
/// the active org, so switching orgs never serves a stale primary-org role; it
/// also includes the validated `user_id`, so there is no IDOR surface (a user
/// only reads their own context) and a disabled cache falls back to a live fetch.
pub(crate) async fn resolve_session_context(state: &AppState, user: &AuthenticatedUser) -> Value {
    let scope_org = scope_org_id(user);
    let key = crate::cache::cache_key(
        "session-context",
        &[user.user_id.as_str(), scope_org.unwrap_or("")],
    );
    if let Some(cached) = state
        .cache
        .lookup_within(&key, SESSION_CONTEXT_TTL_SECS)
        .await
    {
        return cached;
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/me/session-context", state.user_core_url);
    // Forward the active org as x-org-id; user-core returns the membership role for
    // THAT org, falling back to the primary membership when `scope_org` is None.
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        scope_org,
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Value::Null;
    }

    let context = crate::envelope::unwrap_data(&body);
    state.cache.store(&key, &context).await;
    context
}

/// The authenticated user's authoritative org id, from the validated session /
/// cached session-context (never a client-supplied header). Empty string when
/// there is none / on failure.
pub(crate) async fn authorized_org_id(state: &AppState, user: &AuthenticatedUser) -> String {
    // The org the user is currently acting as: the session's active organization
    // (already in the validated session — zero extra round-trip), falling back to the
    // primary-org membership from the cached session-context before any org switch.
    if let Some(active) = scope_org_id(user) {
        return active.to_owned();
    }
    resolve_session_context(state, user)
        .await
        .get("orgId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Send a request, retrying ONCE on a connection/send-level error. Such errors
/// mean the request never reached the upstream (e.g. a pooled keep-alive socket
/// that died while idle — the root cause of intermittent "error sending request"
/// 502s), so a retry is safe even for non-idempotent methods; reqwest opens a
/// fresh connection on the second attempt. Streaming bodies that can't be cloned
/// simply skip the retry.
async fn send_with_retry(
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    let retry = builder.try_clone();
    match builder.send().await {
        Ok(resp) => Ok(resp),
        Err(err) => match retry {
            Some(retry_builder) if err.is_connect() || err.is_request() => {
                retry_builder.send().await
            }
            _ => Err(err),
        },
    }
}

pub(crate) async fn proxy_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: Option<&ActionActor>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let actor = actor_with_defaults(actor);
    let mut request = state.client.request(method, url);
    request = request.header("x-internal-api-key", &state.internal_api_key);
    request = request.header("x-user-id", actor.user_id);
    if !actor.user_email.is_empty() {
        request = request.header("x-user-email", actor.user_email);
    }
    if !actor.user_name.is_empty() {
        request = request.header("x-user-name", actor.user_name);
    }
    if !actor.user_role.is_empty() {
        request = request.header("x-user-role", actor.user_role);
    }
    if let Some(org_id) = org_id.filter(|value| !value.trim().is_empty()) {
        request = request.header("x-org-id", org_id.trim());
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }

    match send_with_retry(request).await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            // On a non-success status with an empty / non-JSON upstream body, synthesize
            // a proper {error:{code,message}} envelope instead of forwarding `{}` with an
            // error status — otherwise the SPA gets a failing status it cannot explain.
            let needs_envelope = !status.is_success()
                && (body.is_null() || body.as_object().map(|o| o.is_empty()).unwrap_or(false));
            if needs_envelope {
                (
                    status,
                    Json(error(
                        "upstream_error",
                        format!("Upstream returned {}", status.as_u16()),
                    )),
                )
            } else {
                (status, Json(if body.is_null() { json!({}) } else { body }))
            }
        }
        Err(request_error) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", request_error.to_string())),
        ),
    }
}

/// Proxy an auth request to auth-core, forwarding session cookies and returning Set-Cookie headers.
/// Session tokens are stripped from the response body — they live only in HttpOnly cookies.
pub(crate) async fn proxy_auth(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    cookie_header: Option<&str>,
) -> Response {
    let mut req = state.client.request(method, url);

    if let Some(cookie) = cookie_header.filter(|c| !c.is_empty()) {
        req = req.header("cookie", cookie);
    }

    if let Some(body) = body {
        req = req.json(&body);
    }

    match req.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

            let set_cookies: Vec<_> = upstream
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .cloned()
                .collect();

            let mut body = upstream.json::<Value>().await.unwrap_or_else(|_| json!({}));

            // Strip session tokens from auth response bodies — they belong only in
            // HttpOnly cookies. Better Auth's get-session nests the live token under
            // `session.token`, so strip the top-level keys (sign-in/up shape) AND the
            // nested ones; otherwise the same secret in the HttpOnly cookie is readable
            // by any page script (XSS exfiltration / session replay).
            strip_session_tokens(&mut body);

            let mut response = (status, Json(body)).into_response();
            for cookie_val in set_cookies {
                response.headers_mut().append(SET_COOKIE, cookie_val);
            }
            response
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", e.to_string())),
        )
            .into_response(),
    }
}

/// Remove session tokens from an auth response body in place — both top-level
/// `token`/`sessionToken` (sign-in/up shape) and nested `session.token`/
/// `session.sessionToken` (Better Auth get-session shape). Non-secret session
/// metadata (e.g. `expiresAt`) is preserved.
fn strip_session_tokens(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.remove("token");
    obj.remove("sessionToken");
    if let Some(session) = obj.get_mut("session").and_then(Value::as_object_mut) {
        session.remove("token");
        session.remove("sessionToken");
    }
}

/// Proxy a request to integration-corev2 and normalize its `{success, data|error}` envelope
/// to the gateway's standard `{data}` / `{error:{code,message}}` shape.
/// Also maps 402/403 plan-gate errors to `PLAN_REQUIRED` error code.
pub(crate) async fn proxy_integration_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    actor: Option<&ActionActor>,
    org_id: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let (status, Json(raw)) = proxy_json(state, method, url, body, org_id, actor, None).await;

    if let Some(success) = raw.get("success").and_then(|v| v.as_bool()) {
        if success {
            let data = raw.get("data").cloned().unwrap_or_else(|| raw.clone());
            return (status, Json(ok(data)));
        }

        let msg = raw
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Integration service error")
            .to_owned();
        let code: &'static str = if status.as_u16() == 402
            || status.as_u16() == 403
            || msg.to_ascii_lowercase().contains("plan")
        {
            "PLAN_REQUIRED"
        } else {
            "integration_error"
        };
        return (status, Json(error(code, msg)));
    }

    (status, Json(raw))
}

/// Proxy a JSON request with a Bearer token and x-user-id. Used for quarry-edge and similar
/// services that accept JWT auth rather than internal API key auth.
pub(crate) async fn proxy_bearer_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user_id: &str,
) -> (StatusCode, Json<Value>) {
    let mut req = state
        .client
        .request(method, url)
        .header("x-user-id", user_id);

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match send_with_retry(req).await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let b = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(b))
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", e.to_string())),
        ),
    }
}

/// Proxy an SSE stream from model-gateway to the browser without buffering.
/// Injects a Bearer token and optional `Last-Event-ID` / ZDR headers before forwarding.
/// Returns a proper JSON error envelope if the upstream returns a non-2xx status.
pub(crate) async fn proxy_sse_stream(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    last_event_id: Option<&str>,
    zdr: bool,
) -> Response {
    let mut req = state.streaming_client.request(method, url);

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }

    if let Some(lei) = last_event_id.filter(|v| !v.is_empty()) {
        req = req.header("last-event-id", lei);
    }

    if zdr {
        req = req.header("x-zdr", "true");
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match req.send().await {
        Ok(resp) => {
            let status_u16 = resp.status().as_u16();
            if status_u16 >= 400 {
                let code = if status_u16 == 401 {
                    "unauthorized"
                } else {
                    "model_error"
                };
                let msg = format!("Model gateway returned {status_u16}");
                return (
                    StatusCode::from_u16(status_u16).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(error(code, msg)),
                )
                    .into_response();
            }

            // Forward upstream SSE bytes verbatim, interleaving a `: keep-alive`
            // comment every 15s of idle so intermediaries (nginx, the Docker bridge)
            // don't reap an idle stream. Comment lines (leading `:`) are ignored by
            // EventSource and by the SPA's sse.ts parser.
            let upstream = resp.bytes_stream();
            let body = Body::from_stream(async_stream::stream! {
                futures_util::pin_mut!(upstream);
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await; // discard the immediate first tick
                loop {
                    tokio::select! {
                        chunk = upstream.next() => match chunk {
                            Some(Ok(bytes)) => yield Ok(bytes),
                            Some(Err(err)) => {
                                yield Err(err);
                                break;
                            }
                            None => break,
                        },
                        _ = ticker.tick() => {
                            yield Ok(bytes::Bytes::from_static(b": keep-alive\n\n"));
                        }
                    }
                }
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("x-accel-buffering", "no")
                .body(body)
                .expect("infallible static headers")
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", e.to_string())),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthenticatedUser;

    fn user_with_active_org(active: Option<&str>) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "u@example.com".to_owned(),
            user_name: "U".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: active.map(str::to_owned),
        }
    }

    #[test]
    fn scope_org_id_uses_active_org_when_set() {
        assert_eq!(
            scope_org_id(&user_with_active_org(Some("org-active"))),
            Some("org-active")
        );
    }

    #[test]
    fn scope_org_id_trims_and_treats_blank_as_none() {
        assert_eq!(
            scope_org_id(&user_with_active_org(Some("  org-x  "))),
            Some("org-x")
        );
        assert_eq!(scope_org_id(&user_with_active_org(Some("   "))), None);
        assert_eq!(scope_org_id(&user_with_active_org(None)), None);
    }

    #[test]
    fn session_context_cache_key_differs_by_active_org() {
        // Role/onboarding now differ per active org, so the cache must not collapse
        // two different active orgs (or active vs primary) onto one entry — that was
        // the stale-role bug. This mirrors the key built in resolve_session_context.
        let key = |user: &AuthenticatedUser| {
            crate::cache::cache_key(
                "session-context",
                &[user.user_id.as_str(), scope_org_id(user).unwrap_or("")],
            )
        };
        let primary = key(&user_with_active_org(None));
        let org_a = key(&user_with_active_org(Some("org-a")));
        let org_b = key(&user_with_active_org(Some("org-b")));
        assert_ne!(primary, org_a);
        assert_ne!(org_a, org_b);
        // A blank active org collapses onto the primary (None) key.
        assert_eq!(primary, key(&user_with_active_org(Some("  "))));
    }
}
