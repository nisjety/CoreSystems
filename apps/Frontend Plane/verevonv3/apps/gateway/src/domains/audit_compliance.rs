//! Audit compliance surface (WS06-S6): the owner/admin read, verify and export
//! routes over audit-core's compliance API, and the SIEM destination proxies
//! over org-core.
//!
//! This is deliberately not `audit.rs`: that route is session-only and feeds the
//! Trust Center for every member, while these routes expose the whole org log and
//! its tamper evidence, so every one of them is gated to the live owner/admin
//! membership before anything is forwarded.
//!
//! The org is the caller's authorized org (never a client value) and the role
//! sent upstream is the live membership role, not the platform role, because
//! audit-core and org-core re-check it from the signed delegation. Query strings
//! and bodies are rebuilt from an allow-list per route, so a client-supplied
//! `org_id` (or anything else the route does not define) is dropped here.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Extension, Path, RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{Map, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::{authorized_org_id, proxy_json},
};

const COMPLIANCE_BASE: &str = "/v1/compliance/audit";
const ADMIN_ROLES: &[&str] = &["owner", "admin"];

const EVENTS_QUERY: &[&str] = &[
    "event", "user_id", "plane", "outcome", "since", "until", "limit", "cursor",
];
const MANIFEST_QUERY: &[&str] = &["since", "until"];
const NO_QUERY: &[&str] = &[];
const VERIFY_BODY: &[&str] = &["from_seq", "from_hash", "max_rows"];

/// Bounds the URL a client can make the gateway build; audit-core validates the
/// values themselves.
const MAX_QUERY_VALUE_LEN: usize = 512;
/// A verify body is three scalars and a SIEM body a URL, a secret and a filter.
const MAX_VERIFY_BODY_BYTES: usize = 4 << 10;
const MAX_SIEM_BODY_BYTES: usize = 32 << 10;
const MAX_DESTINATION_ID_LEN: usize = 64;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/audit/compliance/events", get(events))
        .route("/api/v1/audit/compliance/manifest", get(manifest))
        .route("/api/v1/audit/compliance/keys", get(keys))
        .route("/api/v1/audit/compliance/export-status", get(export_status))
        .route("/api/v1/audit/compliance/verify", post(verify))
        .route(
            "/api/v1/audit/siem-destinations",
            get(list_destinations).post(create_destination),
        )
        .route(
            "/api/v1/audit/siem-destinations/{destination_id}",
            patch(update_destination).delete(delete_destination),
        )
        // `rate_limit_middleware` reads the `AuthenticatedUser` that
        // `require_session` inserts, so it is written first (inner).
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// The caller as the upstream cores must see them: the authorized org and the
/// live membership role.
struct AdminCaller {
    org_id: String,
    actor: ActionActor,
}

fn refusal(status: StatusCode, code: &'static str, message: &str) -> Response {
    (status, Json(error(code, message))).into_response()
}

async fn admin_caller(state: &AppState, user: &AuthenticatedUser) -> Result<AdminCaller, Response> {
    if !has_authorized_org_role(user, ADMIN_ROLES) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Audit compliance requires an organization owner or admin role.",
        ));
    }
    let org_id = authorized_org_id(state, user).await;
    let role = user
        .authorized_membership
        .as_ref()
        .map(|membership| membership.role.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if org_id.trim().is_empty() || role.is_empty() {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "forbidden",
            "No authorized organization for this session.",
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

/// The query string the upstream sees: the caller's org first, then only the
/// allow-listed parameters, first occurrence each, blanks dropped.
fn upstream_query(org_id: &str, raw: Option<&str>, allowed: &[&str]) -> Result<String, Response> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for (name, value) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        if !allowed.contains(&name.as_ref()) || value.trim().is_empty() {
            continue;
        }
        if value.len() > MAX_QUERY_VALUE_LEN {
            return Err(refusal(
                StatusCode::BAD_REQUEST,
                "invalid_query",
                "A query parameter is too long.",
            ));
        }
        seen.entry(name.into_owned())
            .or_insert_with(|| value.into_owned());
    }
    let mut pairs = vec![format!("org_id={}", urlencoding::encode(org_id))];
    // Allow-list order, so the signed request target is deterministic.
    for name in allowed {
        if let Some(value) = seen.get(*name) {
            pairs.push(format!("{name}={}", urlencoding::encode(value)));
        }
    }
    Ok(pairs.join("&"))
}

async fn compliance_get(
    state: AppState,
    user: AuthenticatedUser,
    raw_query: Option<String>,
    route: &str,
    allowed: &[&str],
) -> Response {
    let caller = match admin_caller(&state, &user).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let query = match upstream_query(&caller.org_id, raw_query.as_deref(), allowed) {
        Ok(query) => query,
        Err(response) => return response,
    };
    let url = format!("{}{COMPLIANCE_BASE}/{route}?{query}", state.audit_core_url);
    let (status, body) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&caller.org_id),
        Some(&caller.actor),
        None,
    )
    .await;
    (status, body).into_response()
}

async fn events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    compliance_get(state, user, raw_query, "events", EVENTS_QUERY).await
}

async fn manifest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    compliance_get(state, user, raw_query, "manifest", MANIFEST_QUERY).await
}

async fn keys(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    compliance_get(state, user, raw_query, "keys", NO_QUERY).await
}

async fn export_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    compliance_get(state, user, raw_query, "export-status", NO_QUERY).await
}

/// A request body as a JSON object. An empty body is an empty object; anything
/// else that is not an object, or is over the cap, is refused before it leaves.
fn object_body(raw: &Bytes, cap: usize) -> Result<Map<String, Value>, Response> {
    let invalid = |message: &str| refusal(StatusCode::BAD_REQUEST, "invalid_body", message);
    if raw.len() > cap {
        return Err(invalid("The request body is too large."));
    }
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(invalid("The request body must be a JSON object.")),
    }
}

async fn verify(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Bytes,
) -> Response {
    let caller = match admin_caller(&state, &user).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    let mut object = match object_body(&body, MAX_VERIFY_BODY_BYTES) {
        Ok(object) => object,
        Err(response) => return response,
    };
    object.retain(|name, _| VERIFY_BODY.contains(&name.as_str()));
    let query = match upstream_query(&caller.org_id, None, NO_QUERY) {
        Ok(query) => query,
        Err(response) => return response,
    };
    let url = format!("{}{COMPLIANCE_BASE}/verify?{query}", state.audit_core_url);
    let (status, body) = proxy_json(
        &state,
        Method::POST,
        &url,
        Some(Value::Object(object)),
        Some(&caller.org_id),
        Some(&caller.actor),
        Some("application/json"),
    )
    .await;
    (status, body).into_response()
}

fn destinations_url(state: &AppState, org_id: &str, destination_id: Option<&str>) -> String {
    let base = format!(
        "{}/orgs/{}/siem-destinations",
        state.org_core_url,
        urlencoding::encode(org_id)
    );
    match destination_id {
        Some(id) => format!("{base}/{}", urlencoding::encode(id)),
        None => base,
    }
}

fn valid_destination_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DESTINATION_ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Forwards one SIEM destination call. The secret in a create or patch body is
/// write-only: it is passed through to org-core, which seals it, and is never
/// logged or echoed here.
async fn destinations_call(
    state: AppState,
    user: AuthenticatedUser,
    method: Method,
    destination_id: Option<String>,
    body: Option<Bytes>,
) -> Response {
    let caller = match admin_caller(&state, &user).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if let Some(id) = destination_id.as_deref() {
        if !valid_destination_id(id) {
            return refusal(
                StatusCode::BAD_REQUEST,
                "invalid_destination_id",
                "The destination id is not valid.",
            );
        }
    }
    let payload = match body {
        None => None,
        Some(raw) => match object_body(&raw, MAX_SIEM_BODY_BYTES) {
            Ok(mut object) => {
                // The org is the session's; org-core refuses unknown fields, so
                // a forged one is removed rather than turned into an error.
                object.remove("org_id");
                object.remove("orgId");
                Some(Value::Object(object))
            }
            Err(response) => return response,
        },
    };
    let content_type = payload.as_ref().map(|_| "application/json");
    let url = destinations_url(&state, &caller.org_id, destination_id.as_deref());
    let (status, body) = proxy_json(
        &state,
        method,
        &url,
        payload,
        Some(&caller.org_id),
        Some(&caller.actor),
        content_type,
    )
    .await;
    (status, body).into_response()
}

async fn list_destinations(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    destinations_call(state, user, Method::GET, None, None).await
}

async fn create_destination(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Bytes,
) -> Response {
    destinations_call(state, user, Method::POST, None, Some(body)).await
}

async fn update_destination(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(destination_id): Path<String>,
    body: Bytes,
) -> Response {
    destinations_call(state, user, Method::PATCH, Some(destination_id), Some(body)).await
}

async fn delete_destination(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(destination_id): Path<String>,
) -> Response {
    destinations_call(state, user, Method::DELETE, Some(destination_id), None).await
}

#[cfg(test)]
mod tests {
    //! Driven through the REAL router: the real session middleware decides the
    //! caller's org role (get-session + session-context), and the upstream
    //! doubles answer only a request that carries the gateway's v3 delegation
    //! for that org and role, so a route that forgot the identity would see a
    //! 404 from the double rather than a pass.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_json as wm_body_json, header as wm_header, method as wm_method, path as wm_path,
        query_param as wm_query,
    };
    use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

    use super::*;

    const COOKIE: &str = "better-auth.session_token=session-1";

    struct Harness {
        auth: MockServer,
        user_core: MockServer,
        audit_core: MockServer,
        org_core: MockServer,
    }

    impl Harness {
        async fn start(org_role: &str) -> Self {
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
            Self {
                auth,
                user_core,
                audit_core: MockServer::start().await,
                org_core: MockServer::start().await,
            }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.audit_core_url = self.audit_core.uri();
            state.org_core_url = self.org_core.uri();
            state
        }

        async fn audit_core_requests(&self) -> usize {
            self.audit_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }

        async fn org_core_requests(&self) -> usize {
            self.org_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }

        async fn send(&self, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
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
            let response = crate::build_router(self.state())
                .oneshot(request.body(body).unwrap())
                .await
                .unwrap();
            let status = response.status().as_u16();
            let parsed: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap_or(Value::Null);
            (status, parsed)
        }
    }

    fn delegated(role: &str, route_method: &str, route_path: &str) -> MockBuilder {
        Mock::given(wm_method(route_method))
            .and(wm_path(route_path))
            .and(wm_header("x-service-id", "verevon-gateway"))
            .and(wm_header("x-delegation-version", "v3"))
            .and(wm_header("x-org-id", "org-1"))
            .and(wm_header("x-user-id", "user-1"))
            .and(wm_header("x-user-role", role))
    }

    #[tokio::test]
    async fn a_member_is_refused_every_route_and_no_upstream_is_called() {
        let harness = Harness::start("member").await;
        let calls: [(&str, &str, Option<Value>); 8] = [
            ("GET", "/api/v1/audit/compliance/events", None),
            ("GET", "/api/v1/audit/compliance/manifest", None),
            ("GET", "/api/v1/audit/compliance/keys", None),
            ("GET", "/api/v1/audit/compliance/export-status", None),
            ("POST", "/api/v1/audit/compliance/verify", Some(json!({}))),
            ("GET", "/api/v1/audit/siem-destinations", None),
            (
                "POST",
                "/api/v1/audit/siem-destinations",
                Some(json!({"kind": "ndjson_webhook"})),
            ),
            ("DELETE", "/api/v1/audit/siem-destinations/d-1", None),
        ];
        for (method, uri, body) in calls {
            let (status, payload) = harness.send(method, uri, body).await;
            assert_eq!(status, 403, "{method} {uri}");
            assert_eq!(payload["error"]["code"], "forbidden", "{method} {uri}");
        }
        assert_eq!(harness.audit_core_requests().await, 0);
        assert_eq!(harness.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn an_admin_reads_events_with_only_the_allowed_filters_and_the_session_org() {
        let harness = Harness::start("admin").await;
        delegated("admin", "GET", "/v1/compliance/audit/events")
            .and(wm_query("org_id", "org-1"))
            .and(wm_query("event", "tool_action"))
            .and(wm_query("limit", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [], "meta": {"count": 0, "head_seq": 7}, "error": null
            })))
            .expect(1)
            .mount(&harness.audit_core)
            .await;

        // A forged org, a repeated filter and an unknown parameter are all
        // dropped; the first `event` wins.
        let (status, body) = harness
            .send(
                "GET",
                "/api/v1/audit/compliance/events?org_id=org-2&event=tool_action&event=other&limit=50&bogus=1&since=",
                None,
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["meta"]["head_seq"], 7);

        let received = harness.audit_core.received_requests().await.unwrap();
        assert_eq!(
            received[0].url.query(),
            Some("org_id=org-1&event=tool_action&limit=50")
        );
    }

    #[tokio::test]
    async fn the_owner_role_is_forwarded_and_manifest_keys_and_export_status_route_through() {
        let harness = Harness::start("owner").await;
        for route in ["manifest", "keys", "export-status"] {
            delegated(
                "owner",
                "GET",
                &format!("/v1/compliance/audit/{route}"),
            )
            .and(wm_query("org_id", "org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": route})))
            .expect(1)
            .mount(&harness.audit_core)
            .await;
            let (status, body) = harness
                .send(
                    "GET",
                    &format!("/api/v1/audit/compliance/{route}?org_id=org-2&since=2026-01-01T00:00:00Z"),
                    None,
                )
                .await;
            assert_eq!(status, 200, "{route}: {body}");
            assert_eq!(body["data"], route);
        }
        let received = harness.audit_core.received_requests().await.unwrap();
        // since is a manifest filter only.
        assert_eq!(
            received[0].url.query(),
            Some("org_id=org-1&since=2026-01-01T00%3A00%3A00Z")
        );
        assert_eq!(received[1].url.query(), Some("org_id=org-1"));
        assert_eq!(received[2].url.query(), Some("org_id=org-1"));
    }

    #[tokio::test]
    async fn verify_forwards_only_its_three_fields() {
        let harness = Harness::start("owner").await;
        delegated("owner", "POST", "/v1/compliance/audit/verify")
            .and(wm_query("org_id", "org-1"))
            .and(wm_body_json(json!({"from_seq": 10, "max_rows": 500})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"status": "verified"}, "error": null
            })))
            .expect(1)
            .mount(&harness.audit_core)
            .await;
        let (status, body) = harness
            .send(
                "POST",
                "/api/v1/audit/compliance/verify?org_id=org-2",
                Some(json!({"from_seq": 10, "max_rows": 500, "org_id": "org-2", "extra": true})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["status"], "verified");
    }

    #[tokio::test]
    async fn verify_refuses_a_body_that_is_not_an_object_before_it_leaves() {
        let harness = Harness::start("owner").await;
        let response = crate::build_router(harness.state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/compliance/verify")
                    .header("cookie", COOKIE)
                    .header("content-type", "application/json")
                    .body(Body::from("[1,2]"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 400);
        assert_eq!(harness.audit_core_requests().await, 0);
    }

    #[tokio::test]
    async fn a_compliance_refusal_from_audit_core_passes_through_with_its_status() {
        let harness = Harness::start("admin").await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/compliance/audit/keys"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "data": null, "meta": null,
                "error": {"code": "compliance_role_required", "message": "owner or admin only"}
            })))
            .mount(&harness.audit_core)
            .await;
        let (status, body) = harness
            .send("GET", "/api/v1/audit/compliance/keys", None)
            .await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "compliance_role_required");
    }

    #[tokio::test]
    async fn an_admin_lists_creates_updates_and_deletes_destinations_for_the_session_org() {
        let harness = Harness::start("admin").await;
        delegated("admin", "GET", "/orgs/org-1/siem-destinations")
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "d-1", "secret_set": true}]
            })))
            .expect(1)
            .mount(&harness.org_core)
            .await;
        // The forged org field is stripped: org-core refuses unknown fields.
        delegated("admin", "POST", "/orgs/org-1/siem-destinations")
            .and(wm_body_json(json!({
                "kind": "ndjson_webhook",
                "url": "https://siem.example.com/hook",
                "secret": "s3cret"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "data": {"id": "d-1", "secret_set": true}
            })))
            .expect(1)
            .mount(&harness.org_core)
            .await;
        delegated("admin", "PATCH", "/orgs/org-1/siem-destinations/d-1")
            .and(wm_body_json(json!({"expected_revision": 1, "enabled": false})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"id": "d-1", "enabled": false}
            })))
            .expect(1)
            .mount(&harness.org_core)
            .await;
        delegated("admin", "DELETE", "/orgs/org-1/siem-destinations/d-1")
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&harness.org_core)
            .await;

        let (status, body) = harness
            .send("GET", "/api/v1/audit/siem-destinations?org_id=org-2", None)
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"][0]["secret_set"], true);

        let (status, body) = harness
            .send(
                "POST",
                "/api/v1/audit/siem-destinations",
                Some(json!({
                    "kind": "ndjson_webhook",
                    "url": "https://siem.example.com/hook",
                    "secret": "s3cret",
                    "org_id": "org-2"
                })),
            )
            .await;
        assert_eq!(status, 201, "{body}");

        let (status, body) = harness
            .send(
                "PATCH",
                "/api/v1/audit/siem-destinations/d-1",
                Some(json!({"expected_revision": 1, "enabled": false})),
            )
            .await;
        assert_eq!(status, 200, "{body}");

        let (status, body) = harness
            .send("DELETE", "/api/v1/audit/siem-destinations/d-1", None)
            .await;
        assert_eq!(status, 204, "{body}");
        assert_eq!(harness.audit_core_requests().await, 0);
    }

    #[tokio::test]
    async fn a_malformed_destination_id_never_reaches_org_core() {
        let harness = Harness::start("owner").await;
        let (status, body) = harness
            .send("DELETE", "/api/v1/audit/siem-destinations/d.1%3Fx", None)
            .await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"]["code"], "invalid_destination_id");
        assert_eq!(harness.org_core_requests().await, 0);
    }

    #[test]
    fn the_upstream_query_keeps_the_session_org_and_only_allow_listed_names() {
        let query = upstream_query(
            "org 1",
            Some("limit=5&org_id=evil&cursor=a%20b&user_id=&unknown=x&limit=9"),
            EVENTS_QUERY,
        )
        .unwrap();
        assert_eq!(query, "org_id=org%201&limit=5&cursor=a%20b");
        assert_eq!(upstream_query("o", Some("since=x"), NO_QUERY).unwrap(), "org_id=o");
    }

    #[test]
    fn an_oversized_query_value_is_refused() {
        let raw = format!("cursor={}", "a".repeat(MAX_QUERY_VALUE_LEN + 1));
        let refused = upstream_query("o", Some(&raw), EVENTS_QUERY).unwrap_err();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    }
}
