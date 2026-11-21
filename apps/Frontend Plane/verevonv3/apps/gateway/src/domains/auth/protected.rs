use axum::{
    body::Body,
    extract::{Extension, State},
    http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details, ok, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{
        browser_origin, proxy_auth, proxy_json, proxy_session_json, user_core_delegation_headers,
    },
};

use super::shared::{actor_for, cookie_header};

pub(super) async fn session_current(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let (status, Json(body)) = proxy_session_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/sessions/current", state.session_core_url),
        None,
        Some(&actor_for(&user)),
        user.active_org_id.as_deref(),
    )
    .await;
    let control = unwrap_data(&body);
    if !status.is_success()
        || control.pointer("/user/id").and_then(Value::as_str) != Some(user.user_id.as_str())
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "control_session_unavailable",
                "Control session is temporarily unavailable.",
            )),
        )
            .into_response();
    }
    // user-core's session-context, scoped to the session's active org (role,
    // onboardingStatus for the org the user is acting as); cached per
    // (user, active-org) in `upstream` so this is round-trip-free when paired with
    // /me/session-context on bootstrap.
    let ctx = crate::upstream::resolve_session_context(&state, &user).await;

    // Current org = the session's active org (what the user is acting as, set via
    // org-switch) when present, else the primary-org membership. The display name is
    // resolved from org-core for whichever org that is. `role` is the membership role
    // for that same org, so the permission hints below match the active org.
    let org_id = user
        .active_org_id
        .clone()
        .or_else(|| ctx.get("orgId").and_then(Value::as_str).map(str::to_owned));
    let role = ctx
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let permissions = permission_hints(user.auth_role.as_deref(), role.as_str());
    // Never use another organization's snapshot after an active-org switch.
    let control_matches_org =
        control.pointer("/organization/id").and_then(Value::as_str) == org_id.as_deref();
    let onboarding_status = if control_matches_org {
        control.get("onboardingStatus")
    } else {
        None
    }
    .or_else(|| ctx.get("onboardingStatus"))
    .and_then(Value::as_str)
    .unwrap_or("CREATED");
    let org = match org_id {
        Some(id) if !id.is_empty() => {
            let name = resolve_org_name(&state, &user, &id).await;
            Some(json!({ "id": id, "name": name, "role": role }))
        }
        _ => None,
    };

    Json(ok(json!({
        "user": {
            "id": user.user_id,
            "email": user.user_email,
            "name": user.user_name,
            "emailVerified": user.email_verified,
            "image": user.user_image,
            "role": user.auth_role,
        },
        "org": org,
        "permissions": permissions,
        "onboardingStatus": onboarding_status,
        "status": "authenticated",
    })))
    .into_response()
}

/// Resolve an organization's display name from org-core. Returns an empty string
/// on any failure so session bootstrap never breaks on a name lookup. Also reused
/// by the chat path (see `domains::chat::shared`) to give the model verified
/// org/user identity context — never trust a client-supplied name for that.
pub(crate) async fn resolve_org_name(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> String {
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return String::new();
    }
    unwrap_data(&body)
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub(super) async fn get_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let url = format!("{}/api/v1/users/me", state.user_core_url);
    let avatar = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let actor = actor_for(&user);
    let headers = user_core_delegation_headers(
        &state.user_core_service_token,
        &Method::GET,
        &url,
        &[],
        &actor,
        user.active_org_id.as_deref(),
        avatar,
        chrono::Utc::now(),
    );
    let mut request = state.client.get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }

    match request.send().await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
            if status.is_success() {
                merge_session_user_profile(&mut body, &user);
            }
            (status, Json(body))
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        ),
    }
}

pub(super) async fn patch_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    proxy_json(
        &state,
        Method::PATCH,
        &format!("{}/api/v1/users/me", state.user_core_url),
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn get_session_context(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // Cached per-user in `upstream`; the SPA hits this on bootstrap alongside
    // /session/current, so the second call is served from cache.
    let ctx = crate::upstream::resolve_session_context(&state, &user).await;

    // Current org = the session's active org (if switched) else the primary-org
    // membership — matching the org the gateway scopes operations to.
    let org_id = user
        .active_org_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .or_else(|| ctx.get("orgId").and_then(Value::as_str));
    let role = ctx.get("role").and_then(Value::as_str).unwrap_or_default();
    let permissions = permission_hints(user.auth_role.as_deref(), role);
    let onboarding_status = ctx
        .get("onboardingStatus")
        .and_then(Value::as_str)
        .unwrap_or("CREATED");

    // user-core returns a single primary org; the SPA reads `orgs[0]`. Emit a
    // superset: flat fields for existing readers plus an `orgs` array for the SPA.
    let orgs = match org_id {
        Some(id) => {
            let name = resolve_org_name(&state, &user, id).await;
            json!([{ "id": id, "name": name, "role": role }])
        }
        None => json!([]),
    };

    // resolve_session_context returns Null on failure → the fields below fall back
    // to safe defaults, so this endpoint always answers 200 (as it did before).
    (
        StatusCode::OK,
        Json(ok(json!({
            "userId": ctx
                .get("userId")
                .and_then(Value::as_str)
                .unwrap_or(user.user_id.as_str()),
            "email": user.user_email,
            "name": user.user_name,
            "image": user.user_image,
            "orgId": org_id,
            "role": role,
            "permissions": permissions,
            "onboardingStatus": onboarding_status,
            "orgs": orgs,
        }))),
    )
}

// --- Two-factor (TOTP) enrollment ---------------------------------------
//
// Enrollment is session-bound: the user is already authenticated (require_session
// runs first) and Better Auth's two-factor plugin endpoints operate on the live
// session. `proxy_auth` forwards the session cookie, returns any Set-Cookie, and
// strips session tokens from the response body — so the QR/secret and backup codes
// reach the SPA but the session secret never leaks into JS-readable JSON.

/// Begin TOTP enrollment: returns `{ totpURI, backupCodes }` for the user to
/// scan. Requires the account password (re-auth) per Better Auth's plugin.
pub(super) async fn two_factor_enable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(&state, &headers, "/api/auth/two-factor/enable", Some(body)).await
}

/// Fetch the otpauth:// TOTP URI for the (already-enabled, unverified) factor.
/// Used to (re)render the QR / manual key without re-running enable.
pub(super) async fn two_factor_get_totp_uri(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/get-totp-uri",
        Some(body),
    )
    .await
}

/// Confirm enrollment by verifying a TOTP code from the authenticator app.
pub(super) async fn two_factor_verify_totp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/verify-totp",
        Some(body),
    )
    .await
}

/// (Re)generate single-use backup codes. Returns `{ backupCodes }`.
pub(super) async fn two_factor_generate_backup_codes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/generate-backup-codes",
        Some(body),
    )
    .await
}

async fn proxy_two_factor(
    state: &AppState,
    headers: &HeaderMap,
    path: &str,
    body: Option<Value>,
) -> Response {
    let url = format!("{}{}", state.auth_core_url, path);
    let cookie = cookie_header(headers);
    let origin = browser_origin(headers);
    proxy_auth(
        state,
        Method::POST,
        &url,
        body,
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

/// The most body `POST /api/v1/auth/step-up` reads: one password field and its JSON (F-f03-15).
const STEP_UP_BODY_LIMIT_BYTES: usize = 2 * 1024;
/// The longest password the step-up route forwards, in bytes. Auth Core refuses a longer one too.
const STEP_UP_PASSWORD_MAX_BYTES: usize = 1024;

/// `POST /api/v1/auth/step-up` (F-f03-15): the signed-in person re-confirms their password before a high-risk
/// approval. The gateway checks only the shape of the body. Auth Core re-verifies the password, records the proof for
/// the session and answers, and that answer is relayed with its status, as it came. Nothing here verifies, stores or
/// logs the password or the answer, and every answer is `Cache-Control: no-store`.
pub(super) async fn step_up(
    State(state): State<AppState>,
    Extension(_user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Ok(bytes) = axum::body::to_bytes(body, STEP_UP_BODY_LIMIT_BYTES).await else {
        return step_up_refused("body_too_large");
    };
    let password = match step_up_password(&bytes) {
        Ok(password) => password,
        Err(reason) => return step_up_refused(reason),
    };
    let mut request = state
        .client
        .post(format!("{}/api/v2/auth/step-up", state.auth_core_url))
        .json(&json!({ "password": password }));
    let cookie = cookie_header(&headers);
    if !cookie.is_empty() {
        request = request.header("cookie", cookie);
    }
    if let Some(origin) = browser_origin(&headers) {
        request = request.header("origin", origin);
    }
    let upstream = match request.send().await {
        Ok(upstream) => upstream,
        Err(failure) => {
            tracing::warn!(
                error_class = crate::zdr_log::class_reqwest(&failure),
                "step-up: auth-core unreachable"
            );
            return step_up_unavailable();
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !relays_step_up_status(status) {
        return step_up_unavailable();
    }
    let Ok(answer) = upstream.json::<Value>().await else {
        return step_up_unavailable();
    };
    no_store((status, Json(answer)).into_response())
}

/// The password of a step-up body, when the body is exactly one `password` string of 1 to 1024 bytes. Otherwise the
/// closed reason it was refused for, which never echoes the body.
fn step_up_password(bytes: &[u8]) -> Result<String, &'static str> {
    let body: Value = serde_json::from_slice(bytes).map_err(|_| "body_not_json")?;
    let Some(fields) = body.as_object() else {
        return Err("body_not_an_object");
    };
    if fields.keys().any(|key| key != "password") {
        return Err("unexpected_field");
    }
    let Some(password) = fields.get("password") else {
        return Err("password_missing");
    };
    let Some(password) = password.as_str() else {
        return Err("password_not_a_string");
    };
    if password.is_empty() || password.len() > STEP_UP_PASSWORD_MAX_BYTES {
        return Err("password_length");
    }
    Ok(password.to_owned())
}

/// The typed refusal of a step-up body that is not exactly one password (400). `reason` is a closed word, never the
/// body.
fn step_up_refused(reason: &'static str) -> Response {
    no_store(
        (
            StatusCode::BAD_REQUEST,
            Json(error_with_details(
                "invalid_request",
                "Send the password to confirm, and nothing else.",
                json!({ "reason": reason }),
            )),
        )
            .into_response(),
    )
}

/// The refusal when Auth Core cannot be asked, or answers with something this route does not relay (502).
fn step_up_unavailable() -> Response {
    no_store(
        (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "step_up_unavailable",
                "The password confirmation is unavailable. Try again shortly.",
            )),
        )
            .into_response(),
    )
}

/// The statuses relayed from Auth Core: its success, its typed refusals (400 validation, 401 wrong password or no
/// session, 403 `STEP_UP_METHOD_UNAVAILABLE`, 429 `RATE_LIMITED`) and its 503 `STEP_UP_UNAVAILABLE`.
fn relays_step_up_status(status: StatusCode) -> bool {
    status.is_success()
        || matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::SERVICE_UNAVAILABLE
        )
}

/// Marks an answer `Cache-Control: no-store`: a proof, and a refusal of one, is never cached.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn merge_session_user_profile(body: &mut Value, user: &AuthenticatedUser) {
    if let Some(data) = body.get_mut("data") {
        merge_session_user_profile(data, user);
        return;
    }

    if let Some(profile) = body.get_mut("user").and_then(Value::as_object_mut) {
        merge_profile_object(profile, user);
        return;
    }

    if let Some(profile) = body.as_object_mut() {
        merge_profile_object(profile, user);
    }
}

fn merge_profile_object(profile: &mut serde_json::Map<String, Value>, user: &AuthenticatedUser) {
    set_string_if_missing(profile, "id", &user.user_id);
    set_string_if_missing(profile, "email", &user.user_email);
    set_string_if_missing(profile, "name", &user.user_name);
    set_string_if_missing(profile, "display_name", &user.user_name);
    set_bool_if_missing(profile, "email_verified", user.email_verified);
    set_bool_if_missing(profile, "emailVerified", user.email_verified);

    if let Some(image) = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        set_string_if_missing(profile, "avatarUrl", image);
        set_string_if_missing(profile, "avatar", image);
        set_string_if_missing(profile, "image", image);
    }
}

fn set_string_if_missing(profile: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let has_value = profile
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|current| !current.is_empty())
        .unwrap_or(false);
    if !has_value {
        profile.insert(key.to_owned(), Value::String(trimmed.to_owned()));
    }
}

fn set_bool_if_missing(profile: &mut serde_json::Map<String, Value>, key: &str, value: bool) {
    if !profile.get(key).is_some_and(Value::is_boolean) {
        profile.insert(key.to_owned(), Value::Bool(value));
    }
}

fn permission_hints(auth_role: Option<&str>, org_role: &str) -> Vec<&'static str> {
    let mut permissions = vec!["profile:read", "profile:update"];
    if has_any_role(org_role, &["owner", "admin"]) {
        permissions.extend(["org:read", "org:members:read", "org:members:update"]);
    }
    if has_any_role(auth_role.unwrap_or_default(), &["admin", "superadmin"]) {
        permissions.extend(["admin:users:read", "admin:users:update"]);
    }
    permissions
}

fn has_any_role(value: &str, allowed: &[&str]) -> bool {
    value.split(',').map(str::trim).any(|role| {
        allowed
            .iter()
            .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
    })
}

/// `GET /api/v1/admin/users` — cross-org user directory for platform super
/// admins. Unlike `/api/v1/orgs/{id}/members` (scoped to one org), this lists
/// EVERY user in the deployment by proxying Better Auth's admin `list-users`,
/// which is not org-scoped. Gated here on the top-level Better Auth role
/// (`admin`/`superadmin`); auth-core re-checks the same role via its admin
/// plugin, so this is defense-in-depth, not the sole gate.
pub(super) async fn admin_list_users(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    if !has_any_role(
        user.auth_role.as_deref().unwrap_or_default(),
        &["admin", "superadmin"],
    ) {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Cross-org user administration requires a platform admin role.",
            )),
        )
            .into_response();
    }

    // Forward the client's paging/search query (limit, offset, searchValue,
    // sortBy, …) verbatim to Better Auth's admin list-users.
    let query = uri.query().filter(|q| !q.is_empty());
    let url = match query {
        Some(q) => format!("{}/api/auth/admin/list-users?{q}", state.auth_core_url),
        None => format!(
            "{}/api/auth/admin/list-users?limit=200",
            state.auth_core_url
        ),
    };

    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie_header(&headers)),
        browser_origin(&headers).as_deref(),
    )
    .await
}

/// F-f03-15: the step-up route relays Auth Core's password re-check, and never keeps, logs or re-verifies the
/// password. The handler is driven directly with a wiremock Auth Core, and the router with the switch on and off.
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        body::Body,
        extract::{Extension, State},
        http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, Request, StatusCode},
        response::Response,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::{step_up, STEP_UP_PASSWORD_MAX_BYTES};
    use crate::{
        config::AppState,
        middleware::{AuthenticatedUser, AuthorizedMembership},
    };

    const SESSION_COOKIE: &str = "better-auth.session_token=session-1";
    /// The password every test sends: distinctive, so a log that carried it is easy to recognise.
    const PASSWORD: &str = "pw-canary-7f3a9c-correct";
    /// The two tests that reach the step-up warning hold this lock. tracing computes a callsite's interest from the
    /// thread that registers it, so a test without a capture that registers the warning first would hide it from
    /// the capturing test.
    static WARNING_CAPTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "Chat User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn cookied() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static(SESSION_COOKIE),
        );
        headers
    }

    fn password_body() -> String {
        json!({ "password": PASSWORD }).to_string()
    }

    /// An Auth Core whose step-up route answers `answer` to the call a test makes.
    async fn auth_core(answer: ResponseTemplate) -> MockServer {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/step-up"))
            .respond_with(answer)
            .mount(&auth)
            .await;
        auth
    }

    fn state_with(auth: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state
    }

    /// Calls the handler as the session middleware leaves it: a verified caller, the browser's headers and a body.
    async fn call(
        state: AppState,
        headers: HeaderMap,
        body: &str,
    ) -> (StatusCode, HeaderMap, Value) {
        let response: Response = step_up(
            State(state),
            Extension(member()),
            headers,
            Body::from(body.to_owned()),
        )
        .await;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn auth_calls(auth: &MockServer) -> usize {
        auth.received_requests().await.expect("recording on").len()
    }

    #[tokio::test]
    async fn a_proof_is_relayed_with_auth_cores_status_and_is_never_cached() {
        let answer = json!({ "data": {
            "method": "password",
            "stepUpAt": "2026-11-20T10:00:00.000Z",
            "expiresAt": "2026-11-20T10:02:00.000Z",
            "expiresInSeconds": 120,
        }});
        // The mock answers only the exact body under the caller's cookie, so a different request fails the test.
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/step-up"))
            .and(header("cookie", SESSION_COOKIE))
            .and(body_json(json!({ "password": PASSWORD })))
            .respond_with(ResponseTemplate::new(201).set_body_json(answer.clone()))
            .expect(1)
            .mount(&auth)
            .await;

        let (status, headers, body) = call(state_with(&auth), cookied(), &password_body()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body, answer);
        assert_eq!(
            headers
                .get(CACHE_CONTROL)
                .expect("cache-control")
                .to_str()
                .expect("ascii"),
            "no-store"
        );
    }

    #[tokio::test]
    async fn auth_cores_refusals_are_relayed_with_their_status_and_body() {
        for (status, code) in [
            (400, "VALIDATION_ERROR"),
            (401, "INVALID_PASSWORD"),
            (403, "STEP_UP_METHOD_UNAVAILABLE"),
            (429, "RATE_LIMITED"),
            (503, "STEP_UP_UNAVAILABLE"),
        ] {
            let refusal = json!({ "code": code, "message": "Auth Core's words." });
            let auth =
                auth_core(ResponseTemplate::new(status).set_body_json(refusal.clone())).await;
            let (relayed, _, body) = call(state_with(&auth), cookied(), &password_body()).await;
            assert_eq!(relayed.as_u16(), status, "{code}");
            assert_eq!(body, refusal, "{code}");
        }
    }

    #[tokio::test]
    async fn an_unrelayed_answer_and_an_unreachable_auth_core_are_step_up_unavailable() {
        let _capture = WARNING_CAPTURE.lock().await;
        let auth = auth_core(ResponseTemplate::new(500).set_body_string("boom")).await;
        let (status, headers, body) = call(state_with(&auth), cookied(), &password_body()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "step_up_unavailable");
        assert_eq!(
            headers
                .get(CACHE_CONTROL)
                .expect("cache-control")
                .to_str()
                .expect("ascii"),
            "no-store"
        );

        let mut unreachable = crate::tests::test_state(false);
        unreachable.auth_core_url = "http://127.0.0.1:1".to_owned();
        let (status, _, body) = call(unreachable, cookied(), &password_body()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "step_up_unavailable");
    }

    #[tokio::test]
    async fn a_body_that_is_not_exactly_one_password_is_refused_before_auth_core() {
        let too_long =
            json!({ "password": "x".repeat(STEP_UP_PASSWORD_MAX_BYTES + 1) }).to_string();
        let oversized = json!({ "password": "x".repeat(3 * 1024) }).to_string();
        for (body, reason) in [
            ("not json", "body_not_json"),
            ("[\"password\"]", "body_not_an_object"),
            ("{}", "password_missing"),
            ("{\"password\":5}", "password_not_a_string"),
            ("{\"password\":\"\"}", "password_length"),
            (too_long.as_str(), "password_length"),
            ("{\"password\":\"a\",\"remember\":true}", "unexpected_field"),
            ("{\"pass\":\"a\"}", "unexpected_field"),
            (oversized.as_str(), "body_too_large"),
        ] {
            let auth = auth_core(ResponseTemplate::new(201).set_body_json(json!({}))).await;
            let (status, _, answer) = call(state_with(&auth), cookied(), body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{reason}");
            assert_eq!(answer["error"]["code"], "invalid_request", "{reason}");
            assert_eq!(answer["error"]["details"]["reason"], reason);
            assert_eq!(auth_calls(&auth).await, 0, "{reason}");
        }
    }

    #[tokio::test]
    async fn a_password_of_exactly_the_longest_allowed_length_is_forwarded() {
        let longest = "x".repeat(STEP_UP_PASSWORD_MAX_BYTES);
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/step-up"))
            .and(body_json(json!({ "password": longest.clone() })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "data": {} })))
            .expect(1)
            .mount(&auth)
            .await;
        let body = json!({ "password": longest }).to_string();
        let (status, _, _) = call(state_with(&auth), cookied(), &body).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn the_password_is_never_logged_on_any_path() {
        let _capture = WARNING_CAPTURE.lock().await;
        #[derive(Clone, Default)]
        struct LogBuffer(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for LogBuffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        // Callsites other tests hit while no capture was set keep a cached "never"; rebuild so every warning here reaches
        // this buffer, and probe so the assertions below can tell a dead capture from a clean log.
        tracing::callsite::rebuild_interest_cache();
        tracing::warn!(probe = "capture", "step-up-capture-probe");

        // A relayed refusal, a body refused before any call, and a transport failure.
        let auth = auth_core(
            ResponseTemplate::new(401).set_body_json(json!({ "code": "INVALID_PASSWORD" })),
        )
        .await;
        let _ = call(state_with(&auth), cookied(), &password_body()).await;
        let refused_body = format!("{{\"password\":\"{PASSWORD}\",\"extra\":1}}");
        let _ = call(state_with(&auth), cookied(), &refused_body).await;
        let mut unreachable = crate::tests::test_state(false);
        unreachable.auth_core_url = "http://127.0.0.1:1".to_owned();
        let _ = call(unreachable, cookied(), &password_body()).await;

        let logged = String::from_utf8_lossy(&buffer.0.lock().unwrap()).into_owned();
        assert!(
            logged.contains("step-up-capture-probe"),
            "the capture is not live: {logged}"
        );
        assert!(
            logged.contains("step-up: auth-core unreachable"),
            "{logged}"
        );
        assert!(
            !logged.contains(PASSWORD),
            "the password was logged: {logged}"
        );
    }

    #[tokio::test]
    async fn with_the_switch_off_the_route_does_not_exist() {
        let auth = auth_core(ResponseTemplate::new(201).set_body_json(json!({}))).await;
        let state = state_with(&auth);
        assert!(!state.auth_step_up_route_enabled);
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/step-up")
                    .header("cookie", SESSION_COOKIE)
                    .header("content-type", "application/json")
                    .body(Body::from(password_body()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(auth_calls(&auth).await, 0);
    }

    #[tokio::test]
    async fn with_the_switch_on_the_route_needs_a_signed_in_session() {
        let auth = auth_core(ResponseTemplate::new(201).set_body_json(json!({}))).await;
        let mut state = state_with(&auth);
        state.auth_step_up_route_enabled = true;
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/step-up")
                    .header("content-type", "application/json")
                    .body(Body::from(password_body()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(auth_calls(&auth).await, 0);
    }

    #[tokio::test]
    async fn with_the_switch_on_a_signed_in_person_reaches_auth_core_through_the_session() {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"},
            })))
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/step-up"))
            .and(header("cookie", SESSION_COOKIE))
            .and(body_json(json!({ "password": PASSWORD })))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(json!({ "data": { "method": "password" } })),
            )
            .expect(1)
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "owner", "onboardingStatus": "COMPLETED",
            })))
            .mount(&user_core)
            .await;
        let mut state = state_with(&auth);
        state.user_core_url = user_core.uri();
        state.auth_step_up_route_enabled = true;

        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/step-up")
                    .header("cookie", SESSION_COOKIE)
                    .header("content-type", "application/json")
                    .body(Body::from(password_body()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[test]
    fn the_step_up_switch_is_on_only_for_the_exact_word_true() {
        assert!(crate::config::auth_step_up_route_enabled_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("TRUE"),
            Some("yes"),
            Some("false"),
        ] {
            assert!(
                !crate::config::auth_step_up_route_enabled_from(value),
                "{value:?}"
            );
        }
    }
}
