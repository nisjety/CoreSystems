//! WS03-S4: the org-admin Space effect-policy routes (phase 1).
//!
//! user-core owns the policy row, the canonical-role check (auth-core), the write
//! switch, the audit rows and every rule about which entitlement may be on. This
//! module is only the Frontend Plane's door to it: flag, session role (defence in
//! depth), structure check, then a proxy that signs the v1 delegation. It evaluates
//! no policy and holds no state, and Control's typed body and status pass through.
//!
//! Order on every route: flag (404 `space_policy_admin_disabled`), `require_org_admin`,
//! shape check, `proxy_json`. The flag is a rollout control, never an authorization input.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

const CODE_INVALID: &str = "space_policy_invalid_request";
const MAX_ENTITLEMENT_KEYS: usize = 32;
const MAX_KEY_LENGTH: usize = 48;
const MAX_PRESET_ID_LENGTH: usize = 48;
const IDEMPOTENCY_MIN: usize = 8;
const IDEMPOTENCY_MAX: usize = 128;
const EVENTS_MAX_LIMIT: u32 = 100;
const MAX_CURSOR_DIGITS: usize = 19;

/// The only fields a PUT body may carry. Org, actor and role come from the signed
/// delegation, never from the body, so a stray one is refused rather than forwarded.
const PUT_FIELDS: [&str; 4] = [
    "idempotency_key",
    "expected_entitlement_revision",
    "preset_id",
    "entitlements",
];

fn disabled() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "space_policy_admin_disabled",
            "Space policy administration is not enabled.",
        )),
    )
}

fn invalid(field: &str, message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            CODE_INVALID,
            message,
            json!({ "field": field }),
        )),
    )
}

fn control_url(state: &AppState, suffix: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/org-policy{suffix}",
        state.user_core_url
    )
}

/// `^[a-z_]{1,48}$`. Control owns the closed key set, so a new catalogue row needs
/// no gateway edit; this only keeps the shape sane.
fn is_entitlement_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LENGTH
        && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Structure check of a PUT body. Returns the body rebuilt from the allowed fields,
/// or the refusal. Never generates an idempotency key: a retry must reuse the
/// caller's, so the gateway cannot mint a fresh one per attempt.
fn validate_put_body(body: &Value) -> Result<Value, GatewayJsonResponse> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body", "The request body must be a JSON object."));
    };
    if object.keys().any(|key| !PUT_FIELDS.contains(&key.as_str())) {
        return Err(invalid("body", "The request body has an unsupported field."));
    }
    let key = object
        .get("idempotency_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if key.len() < IDEMPOTENCY_MIN || key.len() > IDEMPOTENCY_MAX {
        return Err(invalid(
            "idempotency_key",
            "idempotency_key must be 8 to 128 characters.",
        ));
    }
    let Some(revision) = object
        .get("expected_entitlement_revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
    else {
        return Err(invalid(
            "expected_entitlement_revision",
            "expected_entitlement_revision must be a non-negative integer.",
        ));
    };
    let preset = object.get("preset_id").filter(|value| !value.is_null());
    let entitlements = object.get("entitlements").filter(|value| !value.is_null());
    let mut forwarded = Map::new();
    forwarded.insert("idempotency_key".to_owned(), json!(key));
    forwarded.insert("expected_entitlement_revision".to_owned(), json!(revision));
    match (preset, entitlements) {
        (Some(preset), None) => {
            let Some(preset_id) = preset
                .as_str()
                .map(str::trim)
                .filter(|id| !id.is_empty() && id.len() <= MAX_PRESET_ID_LENGTH)
            else {
                return Err(invalid("preset_id", "preset_id must be a short string."));
            };
            forwarded.insert("preset_id".to_owned(), json!(preset_id));
        }
        (None, Some(entitlements)) => {
            let Some(map) = entitlements.as_object() else {
                return Err(invalid(
                    "entitlements",
                    "entitlements must be an object of booleans.",
                ));
            };
            if map.len() > MAX_ENTITLEMENT_KEYS
                || !map
                    .iter()
                    .all(|(key, value)| is_entitlement_key(key) && value.is_boolean())
            {
                return Err(invalid(
                    "entitlements",
                    "entitlements must hold at most 32 boolean flags with lowercase names.",
                ));
            }
            forwarded.insert("entitlements".to_owned(), entitlements.clone());
        }
        _ => {
            return Err(invalid(
                "preset_id",
                "Send exactly one of preset_id or entitlements.",
            ));
        }
    }
    Ok(Value::Object(forwarded))
}

/// `?limit=` 1..=100 and `?cursor=` (Control's cursor is a decimal event id). Anything
/// else is refused here so a malformed value never reaches the signed URL.
fn events_query(params: &HashMap<String, String>) -> Result<String, GatewayJsonResponse> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(raw) = params.get("limit").map(|value| value.trim()).filter(|v| !v.is_empty()) {
        match raw.parse::<u32>() {
            Ok(limit) if (1..=EVENTS_MAX_LIMIT).contains(&limit) => {
                parts.push(format!("limit={limit}"));
            }
            _ => return Err(invalid("limit", "limit must be 1 to 100.")),
        }
    }
    if let Some(raw) = params.get("cursor").map(|value| value.trim()).filter(|v| !v.is_empty()) {
        if raw.len() > MAX_CURSOR_DIGITS || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("cursor", "cursor is not valid."));
        }
        parts.push(format!("cursor={raw}"));
    }
    Ok(if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    })
}

/// GET /api/v1/orgs/:id/space-policy: the org's effective Space entitlements.
pub(crate) async fn get_space_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/space-policy/entitlements: apply a preset or the eight keys.
pub(crate) async fn put_space_policy_entitlements(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    apply_space_policy_entitlements(&state, &user, &id, &body).await
}

/// Shared by the route above and the `spaces.set_effect_entitlements` /
/// `spaces.apply_effect_preset` action dispatchers, so both ride the same flag,
/// admin check and shape check.
pub(crate) async fn apply_space_policy_entitlements(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: &Value,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(state, user, org_id).await {
        return response;
    }
    let forwarded = match validate_put_body(body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    proxy_json(
        state,
        Method::PUT,
        &control_url(state, "/entitlements"),
        Some(forwarded),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/space-policy/events: the change history, newest first.
pub(crate) async fn list_space_policy_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> GatewayJsonResponse {
    if !state.space_features.policy_admin_api {
        return disabled();
    }
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let query = match events_query(&params) {
        Ok(query) => query,
        Err(response) => return response,
    };
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, &format!("/events{query}")),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    //! WS03-S4 tests: the real router with wiremock for auth-core and user-core (Control). No model
    //! and no database is involved.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "idem-key-12345";
    const POLICY_PATH: &str = "/api/v1/internal/spaces/org-policy";

    struct Fx {
        state: AppState,
        _auth: MockServer,
        control: MockServer,
    }

    /// `role` is the caller's live organization role (Control's session-context answer). The
    /// `policy_admin_api` flag is ON; `control_status` / `control_body` is what every
    /// org-policy path answers.
    async fn fixture(role: &str, control_status: u16, control_body: Value) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let control = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": role,
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&control)
            .await;
        for method in ["GET", "PUT"] {
            for suffix in ["", "/entitlements", "/events"] {
                Mock::given(wm_method(method))
                    .and(wm_path(format!("{POLICY_PATH}{suffix}")))
                    .respond_with(
                        ResponseTemplate::new(control_status).set_body_json(control_body.clone()),
                    )
                    .mount(&control)
                    .await;
            }
        }
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = control.uri();
        state.space_features.policy_admin_api = true;
        Fx {
            state,
            _auth: auth,
            control,
        }
    }

    async fn call(state: &AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value, String) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder
            .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap();
        let response = crate::build_router(state.clone())
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status().as_u16();
        let raw = String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned();
        let parsed = serde_json::from_str(&raw).unwrap_or(Value::Null);
        (status, parsed, raw)
    }

    /// Requests Control received on the org-policy namespace.
    async fn policy_calls(fx: &Fx) -> Vec<wiremock::Request> {
        fx.control
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path().starts_with(POLICY_PATH))
            .collect()
    }

    fn put_uri() -> &'static str {
        "/api/v1/orgs/org-1/space-policy/entitlements"
    }

    fn valid_put() -> Value {
        json!({
            "idempotency_key": KEY,
            "expected_entitlement_revision": 4,
            "entitlements": {"effects_enabled": true, "external_send": false},
        })
    }

    #[tokio::test]
    async fn member_and_viewer_get_403_and_control_receives_no_calls() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["member", "viewer"] {
            let fx = fixture(role, 200, json!({"data": {}})).await;
            for (method, uri, body) in [
                ("GET", "/api/v1/orgs/org-1/space-policy", None),
                ("PUT", put_uri(), Some(valid_put())),
                ("GET", "/api/v1/orgs/org-1/space-policy/events", None),
            ] {
                let (status, parsed, _) = call(&fx.state, method, uri, body).await;
                assert_eq!(status, 403, "{role} {method} {uri}");
                assert_eq!(parsed["error"]["code"], "forbidden");
            }
            assert!(policy_calls(&fx).await.is_empty(), "{role}: zero Control calls");
        }
    }

    #[tokio::test]
    async fn admin_calls_reach_control_with_a_signed_v1_delegation_and_the_org_id() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("admin", 200, json!({"data": {"entitlement_revision": 5}})).await;
        let (status, parsed, _) = call(&fx.state, "GET", "/api/v1/orgs/org-1/space-policy", None).await;
        assert_eq!(status, 200);
        assert_eq!(parsed["data"]["entitlement_revision"], 5);
        let (status, _, _) = call(&fx.state, "PUT", put_uri(), Some(valid_put())).await;
        assert_eq!(status, 200);
        let (status, _, _) = call(
            &fx.state,
            "GET",
            "/api/v1/orgs/org-1/space-policy/events?limit=25&cursor=1234",
            None,
        )
        .await;
        assert_eq!(status, 200);

        let calls = policy_calls(&fx).await;
        assert_eq!(calls.len(), 3);
        let paths: Vec<&str> = calls.iter().map(|r| r.url.path()).collect();
        assert_eq!(
            paths,
            vec![
                POLICY_PATH,
                "/api/v1/internal/spaces/org-policy/entitlements",
                "/api/v1/internal/spaces/org-policy/events",
            ]
        );
        assert_eq!(calls[1].method.as_str(), "PUT");
        assert_eq!(calls[2].url.query(), Some("limit=25&cursor=1234"));
        for request in &calls {
            let header = |name: &str| {
                request
                    .headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned()
            };
            assert_eq!(header("x-delegation-version"), "v1");
            assert!(!header("x-delegation-signature").is_empty(), "signed");
            assert_eq!(header("x-org-id"), "org-1");
            assert_eq!(header("x-user-id"), "user-1");
        }
        // The PUT body is rebuilt from the allowed fields.
        let sent: Value = serde_json::from_slice(&calls[1].body).unwrap();
        assert_eq!(sent["idempotency_key"], KEY);
        assert_eq!(sent["expected_entitlement_revision"], 4);
        assert_eq!(sent["entitlements"]["effects_enabled"], true);
    }

    #[tokio::test]
    async fn owner_is_an_admin_too() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("owner", 200, json!({"data": {}})).await;
        let (status, _, _) = call(&fx.state, "GET", "/api/v1/orgs/org-1/space-policy", None).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn flag_off_gives_404_on_all_three_routes_before_any_role_check() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // A plain member: a role check would answer 403, so a 404 proves the flag is first.
        let mut fx = fixture("member", 200, json!({"data": {}})).await;
        fx.state.space_features.policy_admin_api = false;
        for (method, uri, body) in [
            ("GET", "/api/v1/orgs/org-1/space-policy", None),
            ("PUT", put_uri(), Some(valid_put())),
            ("GET", "/api/v1/orgs/org-1/space-policy/events", None),
        ] {
            let (status, parsed, _) = call(&fx.state, method, uri, body).await;
            assert_eq!(status, 404, "{method} {uri}");
            assert_eq!(parsed["error"]["code"], "space_policy_admin_disabled");
        }
        assert!(policy_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn put_shape_errors_are_400_and_never_reach_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("admin", 200, json!({"data": {}})).await;
        // 33 distinct valid keys (letters and underscores only).
        let mut thirty_three = serde_json::Map::new();
        for i in 0..33_u8 {
            let key: String = format!(
                "k_{}{}",
                (b'a' + (i / 26)) as char,
                (b'a' + (i % 26)) as char
            );
            thirty_three.insert(key, json!(true));
        }
        assert_eq!(thirty_three.len(), 33);
        let cases: Vec<(&str, Value, &str)> = vec![
            (
                "both preset and entitlements",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "preset_id": "balanced", "entitlements": {"a": true}}),
                "preset_id",
            ),
            (
                "neither preset nor entitlements",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1}),
                "preset_id",
            ),
            (
                "missing idempotency key",
                json!({"expected_entitlement_revision": 1, "preset_id": "balanced"}),
                "idempotency_key",
            ),
            (
                "short idempotency key",
                json!({"idempotency_key": "1234567", "expected_entitlement_revision": 1,
                       "preset_id": "balanced"}),
                "idempotency_key",
            ),
            (
                "long idempotency key",
                json!({"idempotency_key": "k".repeat(129), "expected_entitlement_revision": 1,
                       "preset_id": "balanced"}),
                "idempotency_key",
            ),
            (
                "missing expected revision",
                json!({"idempotency_key": KEY, "preset_id": "balanced"}),
                "expected_entitlement_revision",
            ),
            (
                "negative expected revision",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": -1,
                       "preset_id": "balanced"}),
                "expected_entitlement_revision",
            ),
            (
                "unknown field org_id",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "preset_id": "balanced", "org_id": "org-2"}),
                "body",
            ),
            (
                "unknown field role",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "preset_id": "balanced", "role": "owner"}),
                "body",
            ),
            (
                "unknown field actor",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "preset_id": "balanced", "actor": "someone"}),
                "body",
            ),
            (
                "non-boolean entitlement",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "entitlements": {"effects_enabled": "yes"}}),
                "entitlements",
            ),
            (
                "bad entitlement key",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "entitlements": {"Effects-Enabled": true}}),
                "entitlements",
            ),
            (
                "entitlement key too long",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "entitlements": {"a".repeat(49): true}}),
                "entitlements",
            ),
            (
                "more than 32 entitlement keys",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "entitlements": Value::Object(thirty_three)}),
                "entitlements",
            ),
            (
                "entitlements not an object",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "entitlements": ["a"]}),
                "entitlements",
            ),
            (
                "blank preset",
                json!({"idempotency_key": KEY, "expected_entitlement_revision": 1,
                       "preset_id": "  "}),
                "preset_id",
            ),
        ];
        for (name, body, field) in cases {
            let (status, parsed, raw) = call(&fx.state, "PUT", put_uri(), Some(body)).await;
            assert_eq!(status, 400, "{name}: {raw}");
            assert_eq!(parsed["error"]["code"], "space_policy_invalid_request", "{name}");
            assert_eq!(parsed["error"]["details"]["field"], field, "{name}");
        }
        assert!(policy_calls(&fx).await.is_empty(), "no shape error reached Control");
    }

    #[tokio::test]
    async fn the_idempotency_key_is_never_generated() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("admin", 200, json!({"data": {}})).await;
        let (status, _, _) = call(
            &fx.state,
            "PUT",
            put_uri(),
            Some(json!({"expected_entitlement_revision": 1, "preset_id": "balanced"})),
        )
        .await;
        assert_eq!(status, 400);
        assert!(policy_calls(&fx).await.is_empty());
        // A valid key is forwarded unchanged, so a retry reuses it.
        for _ in 0..2 {
            let (status, _, _) = call(&fx.state, "PUT", put_uri(), Some(valid_put())).await;
            assert_eq!(status, 200);
        }
        let calls = policy_calls(&fx).await;
        assert_eq!(calls.len(), 2);
        for request in calls {
            let sent: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(sent["idempotency_key"], KEY);
        }
    }

    #[tokio::test]
    async fn an_org_other_than_the_active_one_is_403() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("admin", 200, json!({"data": {}})).await;
        for (method, uri, body) in [
            ("GET", "/api/v1/orgs/org-2/space-policy", None),
            ("PUT", "/api/v1/orgs/org-2/space-policy/entitlements", Some(valid_put())),
            ("GET", "/api/v1/orgs/org-2/space-policy/events", None),
        ] {
            let (status, _, _) = call(&fx.state, method, uri, body).await;
            assert_eq!(status, 403, "{method} {uri}");
        }
        assert!(policy_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn events_query_values_are_validated() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture("admin", 200, json!({"data": []})).await;
        for query in [
            "limit=0",
            "limit=101",
            "limit=abc",
            "limit=-1",
            "cursor=abc",
            "cursor=12x",
            "cursor=1234567890123456789012",
        ] {
            let (status, parsed, _) = call(
                &fx.state,
                "GET",
                &format!("/api/v1/orgs/org-1/space-policy/events?{query}"),
                None,
            )
            .await;
            assert_eq!(status, 400, "{query}");
            assert_eq!(parsed["error"]["code"], "space_policy_invalid_request", "{query}");
        }
        assert!(policy_calls(&fx).await.is_empty());
        for (query, forwarded) in [("limit=100", "limit=100"), ("limit=1&cursor=9", "limit=1&cursor=9"), ("", "")] {
            let (status, _, _) = call(
                &fx.state,
                "GET",
                &format!("/api/v1/orgs/org-1/space-policy/events?{query}"),
                None,
            )
            .await;
            assert_eq!(status, 200, "{query}");
            let calls = policy_calls(&fx).await;
            let last = calls.last().unwrap();
            assert_eq!(last.url.query().unwrap_or(""), forwarded);
        }
    }

    #[tokio::test]
    async fn control_409_and_422_pass_through_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // Keys are in sorted order and the JSON is compact, so the serialised bytes are stable.
        let conflict = json!({
            "code": "space_policy_revision_conflict",
            "details": {"current_entitlement_revision": 9},
            "error": "The policy changed.",
        });
        let fx = fixture("admin", 409, conflict.clone()).await;
        let (status, parsed, raw) = call(&fx.state, "PUT", put_uri(), Some(valid_put())).await;
        assert_eq!(status, 409);
        assert_eq!(parsed, conflict);
        assert_eq!(parsed["details"]["current_entitlement_revision"], 9);
        assert_eq!(raw, conflict.to_string());

        let unprocessable = json!({
            "code": "space_policy_not_permitted",
            "details": {"entitlement": "external_send"},
            "error": "Not permitted.",
        });
        let fx = fixture("admin", 422, unprocessable.clone()).await;
        let (status, parsed, raw) = call(&fx.state, "PUT", put_uri(), Some(valid_put())).await;
        assert_eq!(status, 422);
        assert_eq!(parsed, unprocessable);
        assert_eq!(raw, unprocessable.to_string());
    }

    #[test]
    fn entitlement_key_shape_is_lowercase_letters_and_underscores_up_to_48() {
        assert!(is_entitlement_key("effects_enabled"));
        assert!(is_entitlement_key(&"a".repeat(48)));
        assert!(!is_entitlement_key(""));
        assert!(!is_entitlement_key(&"a".repeat(49)));
        assert!(!is_entitlement_key("Effects"));
        assert!(!is_entitlement_key("effects-enabled"));
        assert!(!is_entitlement_key("effects1"));
    }
}
