//! EP-9 C1: the org retention window and legal hold routes.
//!
//! org-core stores the window and the holds, checks every rule and answers the
//! retention sweeper; this module is only the Frontend Plane's door to it. An owner
//! or admin sets the window; an owner or a compliance member places and releases a
//! hold. It decides nothing about what is kept.
//!
//! Two role gates guard every call, on purpose: this gateway's session role and
//! org-core's own membership table, because the role this gateway forwards is a
//! hint and never authority. When they disagree the stricter one wins and
//! org-core's refusal is passed through.
//!
//! The feature switch is org-core's `ORG_RETENTION_POLICY_ENABLED`: off, org-core
//! has no such route and this module answers a typed 404, so the SPA hides the
//! section without the gateway holding a second switch.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{Map, Value};

use crate::{
    config::AppState,
    envelope::error,
    middleware::{has_authorized_org_role, AuthenticatedUser},
    upstream::proxy_json,
};

use super::shared::{actor_for, require_active_org, require_org_admin, GatewayJsonResponse};

/// Limits a body must stay inside before the delegation is signed. They mirror
/// org-core's `retention` package; org-core stays the owner of every rule.
const MAX_HOLD_REF_CHARS: usize = 200;
const MAX_THREAD_ID_CHARS: usize = 128;
const PUT_FIELDS: [&str; 2] = ["expected_revision", "thread_days"];
const HOLD_FIELDS: [&str; 3] = ["scope", "thread_id", "hold_ref"];

/// Who may read holds, and who may place or release one. An admin sets the
/// window but cannot lift a hold that could concern them.
const HOLD_READ_ROLES: [&str; 3] = ["owner", "admin", "compliance"];
const HOLD_WRITE_ROLES: [&str; 2] = ["owner", "compliance"];

fn not_enabled() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error("not_found", "Retention policy is not enabled.")),
    )
}

fn bad_request(message: impl Into<String>) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error("validation_error", message)),
    )
}

fn org_core_url(state: &AppState, org_id: &str, suffix: &str) -> String {
    format!(
        "{}/api/v1/organizations/{}{suffix}",
        state.org_core_url,
        urlencoding::encode(org_id)
    )
}

/// The caller's session role must be one of `allowed` in the requested org.
fn require_org_role(
    user: &AuthenticatedUser,
    org_id: &str,
    allowed: &[&str],
) -> Result<(), GatewayJsonResponse> {
    require_active_org(user, org_id)?;
    if has_authorized_org_role(user, allowed) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Your organization role does not allow this legal hold action.",
            )),
        ))
    }
}

/// Forward one call on behalf of `user`. A 404 org-core synthesised no envelope
/// for is the unmounted route of a switched-off feature, so it reads as "not
/// enabled"; org-core's own typed 404s (an unknown hold) pass through.
async fn forward(
    state: &AppState,
    user: &AuthenticatedUser,
    method: Method,
    org_id: &str,
    suffix: &str,
    body: Option<Value>,
) -> GatewayJsonResponse {
    let (status, Json(answer)) = proxy_json(
        state,
        method,
        &org_core_url(state, org_id, suffix),
        body,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if status == StatusCode::NOT_FOUND
        && answer.pointer("/error/code").and_then(Value::as_str) == Some("upstream_error")
    {
        return not_enabled();
    }
    (status, Json(answer))
}

/// GET /api/v1/orgs/:id/retention: the stored window, owner or admin.
pub(crate) async fn get_retention(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    forward(&state, &user, Method::GET, &id, "/retention", None).await
}

/// PUT /api/v1/orgs/:id/retention: replace the window. The body carries the
/// revision it was edited from; org-core answers 409 to a stale editor.
pub(crate) async fn put_retention(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    if let Err(message) = validate_put_body(&body) {
        return bad_request(message);
    }
    forward(&state, &user, Method::PUT, &id, "/retention", Some(body)).await
}

/// GET /api/v1/orgs/:id/legal-holds: active holds, then released ones.
pub(crate) async fn list_legal_holds(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_role(&user, &id, &HOLD_READ_ROLES) {
        return response;
    }
    forward(&state, &user, Method::GET, &id, "/legal-holds", None).await
}

/// POST /api/v1/orgs/:id/legal-holds: place an org or thread hold. A thread whose
/// content was never retained is refused by org-core with
/// `legal_hold_nothing_retained`, passed through as it came.
pub(crate) async fn place_legal_hold(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_role(&user, &id, &HOLD_WRITE_ROLES) {
        return response;
    }
    if let Err(message) = validate_hold_body(&body) {
        return bad_request(message);
    }
    forward(&state, &user, Method::POST, &id, "/legal-holds", Some(body)).await
}

/// POST /api/v1/orgs/:id/legal-holds/:hold_id/release.
pub(crate) async fn release_legal_hold(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, hold_id)): Path<(String, String)>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_role(&user, &id, &HOLD_WRITE_ROLES) {
        return response;
    }
    if !valid_hold_id(&hold_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(error("legal_hold_not_found", "Legal hold not found.")),
        );
    }
    let suffix = format!("/legal-holds/{hold_id}/release");
    forward(&state, &user, Method::POST, &id, &suffix, None).await
}

fn reject_unknown_fields(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(unknown) => Err(format!("Unknown field '{unknown}' in the request body.")),
        None => Ok(()),
    }
}

/// Structural check of a window write: shape, unknown fields, an integer revision
/// and an integer or null window. org-core checks the 30..3650 bounds.
fn validate_put_body(body: &Value) -> Result<(), String> {
    let Some(fields) = body.as_object() else {
        return Err("The request body must be a JSON object.".to_owned());
    };
    reject_unknown_fields(fields, &PUT_FIELDS)?;
    if !fields
        .get("expected_revision")
        .and_then(Value::as_i64)
        .is_some_and(|revision| revision >= 0)
    {
        return Err(
            "expected_revision is required and must be an integer, zero or greater.".to_owned(),
        );
    }
    match fields.get("thread_days") {
        None | Some(Value::Null) => Ok(()),
        Some(days) if days.as_i64().is_some() => Ok(()),
        Some(_) => Err("thread_days must be a whole number of days or null (keep).".to_owned()),
    }
}

/// Structural check of a hold placement. The reference names a matter and must be
/// short; org-core trims it and checks every other rule.
fn validate_hold_body(body: &Value) -> Result<(), String> {
    let Some(fields) = body.as_object() else {
        return Err("The request body must be a JSON object.".to_owned());
    };
    reject_unknown_fields(fields, &HOLD_FIELDS)?;
    match fields.get("scope").and_then(Value::as_str) {
        Some("org" | "thread") => {}
        _ => return Err("scope must be \"org\" or \"thread\".".to_owned()),
    }
    match fields.get("thread_id") {
        None | Some(Value::Null) => {}
        Some(Value::String(id)) if id.chars().count() <= MAX_THREAD_ID_CHARS => {}
        Some(_) => {
            return Err(format!(
                "thread_id must be a string of at most {MAX_THREAD_ID_CHARS} characters."
            ))
        }
    }
    match fields.get("hold_ref").and_then(Value::as_str) {
        Some(reference)
            if !reference.trim().is_empty() && reference.chars().count() <= MAX_HOLD_REF_CHARS =>
        {
            Ok(())
        }
        _ => Err(format!(
            "hold_ref is required: a matter or case reference of at most {MAX_HOLD_REF_CHARS} characters."
        )),
    }
}

/// A hold id is `lh_` and 32 lower-case hex characters, the form org-core issues;
/// anything else never reaches a URL.
fn valid_hold_id(id: &str) -> bool {
    id.len() == 35
        && id.starts_with("lh_")
        && id[3..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    //! The routes are driven through the REAL router: the real session middleware
    //! decides the caller's org role, and org-core answers only a request that
    //! carries the gateway's delegated-service identity for this org and the exact
    //! body. An upstream that must receive nothing is asserted to.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_json as wm_body_json, header as wm_header, method as wm_method, path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::AppState;

    use super::{valid_hold_id, validate_hold_body, validate_put_body};

    const COOKIE: &str = "better-auth.session_token=session-1";
    const ORG_CORE_RETENTION: &str = "/api/v1/organizations/org-1/retention";
    const ORG_CORE_HOLDS: &str = "/api/v1/organizations/org-1/legal-holds";
    const HOLD_ID: &str = "lh_0123456789abcdef0123456789abcdef";

    struct Upstreams {
        auth: MockServer,
        user_core: MockServer,
        org_core: MockServer,
    }

    impl Upstreams {
        /// A signed-in user of org-1 whose session role is `role`.
        async fn start(role: &str) -> Self {
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
                    "userId": "user-1", "orgId": "org-1", "role": role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            Self {
                auth,
                user_core,
                org_core: MockServer::start().await,
            }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.org_core_url = self.org_core.uri();
            state
        }

        /// org-core answering only the gateway's delegated identity for THIS org and
        /// user (and the exact body, when given).
        async fn org_core(
            &self,
            method: &str,
            path: &str,
            body: Option<Value>,
            response: ResponseTemplate,
        ) {
            let mut mock = Mock::given(wm_method(method))
                .and(wm_path(path))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-delegation-version", "v3"))
                .and(wm_header("x-org-id", "org-1"))
                .and(wm_header("x-user-id", "user-1"));
            if let Some(body) = body {
                mock = mock.and(wm_body_json(body));
            }
            mock.respond_with(response).mount(&self.org_core).await;
        }

        async fn org_core_requests(&self) -> usize {
            self.org_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }
    }

    async fn send(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", COOKIE);
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = crate::build_router(state)
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    fn thread_hold() -> Value {
        json!({"scope": "thread", "thread_id": "thr-1", "hold_ref": "Matter 2026-17"})
    }

    #[tokio::test]
    async fn an_owner_sets_the_window_through_org_core() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        let put = json!({"expected_revision": 0, "thread_days": 90});
        up.org_core(
            "PUT",
            ORG_CORE_RETENTION,
            Some(put.clone()),
            ResponseTemplate::new(200).set_body_json(
                json!({"organization_id": "org-1", "thread_days": 90, "revision": 1}),
            ),
        )
        .await;
        let (status, body) =
            send(up.state(), "PUT", "/api/v1/orgs/org-1/retention", Some(put)).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["revision"], 1);
    }

    #[tokio::test]
    async fn a_member_and_a_compliance_member_cannot_set_the_window() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["member", "compliance"] {
            let up = Upstreams::start(role).await;
            let (status, body) = send(
                up.state(),
                "PUT",
                "/api/v1/orgs/org-1/retention",
                Some(json!({"expected_revision": 0, "thread_days": 90})),
            )
            .await;
            assert_eq!(status, 403, "{role}: {body}");
            assert_eq!(up.org_core_requests().await, 0);
        }
    }

    #[tokio::test]
    async fn with_org_cores_switch_off_the_routes_read_as_not_enabled() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core(
            "GET",
            ORG_CORE_RETENTION,
            None,
            ResponseTemplate::new(404).set_body_string("404 page not found"),
        )
        .await;
        let (status, body) = send(up.state(), "GET", "/api/v1/orgs/org-1/retention", None).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "not_found", "{body}");
    }

    #[tokio::test]
    async fn compliance_places_a_hold_and_an_admin_cannot() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let admin = Upstreams::start("admin").await;
        let (status, _) = send(
            admin.state(),
            "POST",
            "/api/v1/orgs/org-1/legal-holds",
            Some(thread_hold()),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(admin.org_core_requests().await, 0);

        let compliance = Upstreams::start("compliance").await;
        compliance
            .org_core(
                "POST",
                ORG_CORE_HOLDS,
                Some(thread_hold()),
                ResponseTemplate::new(201).set_body_json(json!({
                    "hold": {"id": HOLD_ID, "scope": "thread", "target_ref": "thr-1"},
                    "existed": false
                })),
            )
            .await;
        let (status, body) = send(
            compliance.state(),
            "POST",
            "/api/v1/orgs/org-1/legal-holds",
            Some(thread_hold()),
        )
        .await;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["hold"]["id"], HOLD_ID);
    }

    #[tokio::test]
    async fn the_nothing_retained_refusal_reaches_the_caller_as_org_core_typed_it() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        let refusal = json!({"error": {
            "code": "legal_hold_nothing_retained",
            "message": "Nothing is kept for this thread under zero data retention, so there is nothing to hold.",
            "details": {"feature": "legal_hold"}
        }});
        up.org_core(
            "POST",
            ORG_CORE_HOLDS,
            None,
            ResponseTemplate::new(422).set_body_json(refusal.clone()),
        )
        .await;
        let (status, body) = send(
            up.state(),
            "POST",
            "/api/v1/orgs/org-1/legal-holds",
            Some(thread_hold()),
        )
        .await;
        assert_eq!((status, body), (422, refusal));
    }

    #[tokio::test]
    async fn a_release_names_a_well_formed_hold_or_never_leaves_the_gateway() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("owner").await;
        up.org_core(
            "POST",
            &format!("{ORG_CORE_HOLDS}/{HOLD_ID}/release"),
            None,
            ResponseTemplate::new(200).set_body_json(json!({"id": HOLD_ID})),
        )
        .await;
        let (status, _) = send(
            up.state(),
            "POST",
            &format!("/api/v1/orgs/org-1/legal-holds/{HOLD_ID}/release"),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, _) = send(
            up.state(),
            "POST",
            "/api/v1/orgs/org-1/legal-holds/not-a-hold/release",
            None,
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(up.org_core_requests().await, 1);
    }

    #[test]
    fn bodies_are_checked_for_shape_before_signing() {
        assert!(validate_put_body(&json!({"expected_revision": 0, "thread_days": null})).is_ok());
        assert!(validate_put_body(&json!({"expected_revision": 2, "thread_days": 365})).is_ok());
        assert!(validate_put_body(&json!({"thread_days": 365})).is_err());
        assert!(validate_put_body(&json!({"expected_revision": 0, "thread_days": "90"})).is_err());
        assert!(validate_put_body(&json!({"expected_revision": 0, "days": 90})).is_err());

        assert!(validate_hold_body(&json!({"scope": "org", "hold_ref": "M-1"})).is_ok());
        assert!(validate_hold_body(&thread_hold()).is_ok());
        assert!(validate_hold_body(&json!({"scope": "space", "hold_ref": "M-1"})).is_err());
        assert!(validate_hold_body(&json!({"scope": "org", "hold_ref": "  "})).is_err());
        assert!(validate_hold_body(&json!({"scope": "org", "hold_ref": "r".repeat(201)})).is_err());
        assert!(
            validate_hold_body(&json!({"scope": "org", "hold_ref": "M", "note": "x"})).is_err()
        );

        assert!(valid_hold_id(HOLD_ID));
        assert!(!valid_hold_id("lh_0123456789ABCDEF0123456789abcdef"));
        assert!(!valid_hold_id("lh_short"));
    }
}
