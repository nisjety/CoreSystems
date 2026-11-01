//! Room artifacts aggregate: the BFF half of WS07-S9.
//!
//! `GET /api/v1/spaces/{space_ref}/artifacts?limit=50&cursor=...` lists the artifacts the room's
//! agent turns produced, one entry per `(thread, artifact)` at its latest version, newest first.
//! Session Core owns the query and the audience ceiling (`SpaceRoomReadService.ListSpaceArtifacts`,
//! reached through model-gateway `GET /v1/spaces/:space_id/artifacts`); this module decides
//! nothing and stores nothing. It validates the shape, checks the Space lifecycle, asks Control
//! for the signed shared-read decision under the caller's own delegation, forwards with the
//! caller's own bearers, and reduces the answer to a fixed field set.
//!
//! The response carries summaries only, never artifact content (the viewer reads the thread
//! through the existing transcript route):
//! `200 {data:{artifacts:[{thread_id, message_id, artifact_id, kind, title, version,
//! version_count, size_chars, agent_name?, created_at}], next_cursor, shared_read:{available},
//! retention:{zdr}}}`. `retention.zdr` is `true` or `false` from the org posture and `null` when
//! the org-core lookup fails (an outage is not a posture); a ZDR turn produces no artifacts, so
//! the list for a ZDR org is empty and the SPA says why. `shared_read.available` is always `true`
//! on a 200: a Control decline is a 403, never a silent narrowing to the caller's own turns.
//!
//! Order (each step is a refusal before the next one runs): flag, org, Space reference, query
//! shape, Space lifecycle, Control read decision, bearers, model-gateway, response validation.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_limit` /
//! `invalid_cursor`, 403 `active_membership_required` / `space_read_not_authorized`, 404
//! `space_not_found` / `space_room_artifacts_disabled`, 502 `invalid_model_gateway_response`
//! (also a foreign-space item), 503 `space_lifecycle_unavailable` / `delegated_auth_unavailable`
//! / `space_artifacts_unavailable`. The read decision token is carried in the upstream query
//! string only, never logged here and never put in any response body.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{self, OrgZdrPosture},
};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;
const MAX_SPACE_REF_LEN: usize = 200;
const MAX_CURSOR_LEN: usize = 512;

/// The summary fields that cross to the browser; anything else model-gateway adds is dropped.
const STRING_FIELDS: &[&str] = &["thread_id", "message_id", "artifact_id", "kind", "title", "created_at"];
const NUMBER_FIELDS: &[&str] = &["version", "version_count", "size_chars"];

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/spaces/{space_ref}/artifacts", get(list_room_artifacts))
}

#[derive(Debug, Default, Deserialize)]
struct ArtifactsParams {
    limit: Option<String>,
    cursor: Option<String>,
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

fn invalid(code: &'static str, message: &'static str, field: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(code, message, json!({ "field": field }))),
    )
        .into_response()
}

fn bad_upstream() -> Response {
    refuse(
        StatusCode::BAD_GATEWAY,
        "invalid_model_gateway_response",
        "Model Gateway returned an invalid artifact listing.",
    )
}

/// One validated summary, reduced to the allowed fields. `None` is a malformed item.
fn summary(item: &Value, space_ref: &str) -> Option<Value> {
    let object = item.as_object()?;
    // Session Core filters by Space; an item that names another Space is a contract break, not
    // something to narrow away.
    if let Some(item_space) = object.get("space_id").and_then(Value::as_str) {
        if !item_space.is_empty() && item_space != space_ref {
            return None;
        }
    }
    let mut out = Map::new();
    for key in STRING_FIELDS {
        let value = object.get(*key).and_then(Value::as_str)?;
        out.insert((*key).to_owned(), json!(value));
    }
    for key in NUMBER_FIELDS {
        let value = object.get(*key).and_then(Value::as_u64)?;
        out.insert((*key).to_owned(), json!(value));
    }
    if let Some(name) = object
        .get("agent_name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        out.insert("agent_name".to_owned(), json!(name));
    }
    Some(Value::Object(out))
}

async fn list_room_artifacts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(params): Query<ArtifactsParams>,
) -> Response {
    if !state.space_features.room_artifacts {
        return refuse(
            StatusCode::NOT_FOUND,
            "space_room_artifacts_disabled",
            "Room artifacts are not enabled.",
        );
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to read Space artifacts.",
        );
    }
    let space_ref = space_ref.trim();
    if space_ref.is_empty() || space_ref.len() > MAX_SPACE_REF_LEN {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let limit = match params
        .limit
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
            _ => {
                return invalid(
                    "invalid_limit",
                    "limit must be between 1 and 200.",
                    "limit",
                )
            }
        },
    };
    let cursor = params
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty());
    if let Some(cursor) = cursor {
        if cursor.len() > MAX_CURSOR_LEN || cursor.chars().any(char::is_control) {
            return invalid("invalid_cursor", "The cursor is not valid.", "cursor");
        }
    }

    match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(&state, &user, &org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return response.into_response(),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Reading this Space's artifacts is not authorized.";
        return match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            )
                .into_response(),
            None => refuse(StatusCode::FORBIDDEN, denial::OP_READ, message),
        };
    };

    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => {
                return crate::domains::chat::shared::delegated_auth_unavailable(reason)
                    .into_response()
            }
        };
    let mut url = format!(
        "{}/v1/spaces/{}/artifacts?limit={limit}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    if let Some(cursor) = cursor {
        url.push_str("&cursor=");
        url.push_str(&urlencoding::encode(cursor));
    }
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        // model-gateway's typed envelope (401, 403 `space_read_not_authorized`, 400, 503
        // `space_artifacts_unavailable`) as is; it never echoes the decision.
        return (status, Json(payload)).into_response();
    }
    let Some(items) = payload.get("artifacts").and_then(Value::as_array) else {
        return bad_upstream();
    };
    if items.len() > MAX_LIMIT as usize {
        return bad_upstream();
    }
    let mut artifacts: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        match summary(item, space_ref) {
            Some(entry) => artifacts.push(entry),
            None => return bad_upstream(),
        }
    }
    let next_cursor = match payload.get("next_cursor") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(cursor)) if cursor.is_empty() => Value::Null,
        Some(Value::String(cursor)) if cursor.len() <= MAX_CURSOR_LEN => json!(cursor),
        Some(_) => return bad_upstream(),
    };
    // Org posture for the empty-state copy only. A failed lookup is `null`, not "not ZDR".
    let zdr_posture = match zdr::org_zdr_lookup(&state, &user).await {
        OrgZdrPosture::Enabled => json!(true),
        OrgZdrPosture::Disabled => json!(false),
        OrgZdrPosture::Unknown => Value::Null,
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "data": {
                "artifacts": artifacts,
                "next_cursor": next_cursor,
                "shared_read": { "available": true },
                "retention": { "zdr": zdr_posture },
            }
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    //! WS07-S9.gw tests: the real router with wiremock for auth-core, user-core (Control), org-core,
    //! Application (Convex) and model-gateway. No model is called: model-gateway is a mock.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN_SECRET: &str = "decision-token-secret-xyzxyz";
    const ARTIFACTS_URI: &str = "/api/v1/spaces/space-room/artifacts";

    struct Fx {
        state: AppState,
        _auth: MockServer,
        user_core: MockServer,
        _org_core: MockServer,
        _application: MockServer,
        model_gateway: MockServer,
    }

    fn active_space() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": [
            {"spaceRef": "space-room", "name": "Room", "kind": "room", "lifecycle": "active"}
        ]}))
    }

    fn org_posture(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(
            json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
        )
    }

    async fn fixture(org_core: ResponseTemplate, flag: bool) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "model-plane"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_core)
            .mount(&org)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(active_space())
            .mount(&app)
            .await;
        let model_gateway = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.model_gateway_url = model_gateway.uri();
        state.space_features.room_artifacts = flag;
        Fx {
            state,
            _auth: auth,
            user_core,
            _org_core: org,
            _application: app,
            model_gateway,
        }
    }

    async fn default_fixture() -> Fx {
        fixture(org_posture(false), true).await
    }

    fn grant() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"space_ref": "space-room", "decision_ref": "decision-1"},
            "token": TOKEN_SECRET,
        }}))
    }

    async fn mount_read_decision(fx: &Fx, response: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(response)
            .mount(&fx.user_core)
            .await;
    }

    async fn mount_listing(fx: &Fx, response: ResponseTemplate) {
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/spaces/space-room/artifacts"))
            .respond_with(response)
            .mount(&fx.model_gateway)
            .await;
    }

    fn item(artifact_id: &str) -> Value {
        json!({
            "thread_id": "thread-1",
            "message_id": "message-1",
            "artifact_id": artifact_id,
            "kind": "markdown",
            "title": "Plan",
            "version": 2,
            "version_count": 3,
            "size_chars": 1200,
            "created_at": "2026-10-20T10:00:00Z",
        })
    }

    async fn get(state: &AppState, uri: &str) -> (u16, Value, String, Option<String>) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1")
            .body(Body::empty())
            .unwrap();
        let response = crate::build_router(state.clone())
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status().as_u16();
        let cache = response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let raw = String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned();
        let parsed = serde_json::from_str(&raw).unwrap_or(Value::Null);
        (status, parsed, raw, cache)
    }

    async fn model_urls(fx: &Fx) -> Vec<String> {
        fx.model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|request| request.url.to_string())
            .collect()
    }

    fn assert_no_secret(raw: &str) {
        assert!(!raw.contains(TOKEN_SECRET), "decision token leaked: {raw}");
        assert!(!raw.contains("decision-1"), "decision ref leaked: {raw}");
        assert!(!raw.contains("plane-token"), "bearer leaked: {raw}");
    }

    #[tokio::test]
    async fn flag_off_answers_404_space_room_artifacts_disabled_and_calls_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(org_posture(false), false).await;
        mount_read_decision(&fx, grant()).await;
        let (status, body, _, _) = get(&fx.state, ARTIFACTS_URI).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_room_artifacts_disabled");
        assert!(model_urls(&fx).await.is_empty());
        let control = fx.user_core.received_requests().await.unwrap_or_default();
        assert!(control
            .iter()
            .all(|request| !request.url.path().contains("thread-read-decision")));
    }

    #[tokio::test]
    async fn control_read_decline_is_403_space_read_not_authorized_without_a_token() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(
            &fx,
            ResponseTemplate::new(403).set_body_json(json!({
                "error": "You cannot read this Space.",
                "code": "space_role_insufficient",
            })),
        )
        .await;
        mount_listing(&fx, ResponseTemplate::new(200).set_body_json(json!({"artifacts": []})))
            .await;
        let (status, body, raw, _) = get(&fx.state, ARTIFACTS_URI).await;
        assert_eq!(status, 403, "{raw}");
        assert_eq!(body["error"]["code"], "space_read_not_authorized");
        assert_no_secret(&raw);
        assert!(model_urls(&fx).await.is_empty(), "no read after a decline");
    }

    #[tokio::test]
    async fn invalid_limit_and_cursor_are_400_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(&fx, grant()).await;
        mount_listing(&fx, ResponseTemplate::new(200).set_body_json(json!({"artifacts": []})))
            .await;
        for limit in ["0", "201", "abc"] {
            let (status, body, _, _) =
                get(&fx.state, &format!("{ARTIFACTS_URI}?limit={limit}")).await;
            assert_eq!(status, 400, "limit={limit}");
            assert_eq!(body["error"]["code"], "invalid_limit", "limit={limit}");
            assert_eq!(body["error"]["details"]["field"], "limit");
        }
        let too_long = "c".repeat(MAX_CURSOR_LEN + 1);
        for cursor in ["bad%00cursor", "line%0Abreak", too_long.as_str()] {
            let (status, body, _, _) =
                get(&fx.state, &format!("{ARTIFACTS_URI}?cursor={cursor}")).await;
            assert_eq!(status, 400, "cursor={cursor}");
            assert_eq!(body["error"]["code"], "invalid_cursor", "cursor={cursor}");
            assert_eq!(body["error"]["details"]["field"], "cursor");
        }
        assert!(model_urls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn a_foreign_space_item_is_502_invalid_model_gateway_response() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(&fx, grant()).await;
        let mut foreign = item("artifact-2");
        foreign["space_id"] = json!("space-other");
        mount_listing(
            &fx,
            ResponseTemplate::new(200)
                .set_body_json(json!({"artifacts": [item("artifact-1"), foreign]})),
        )
        .await;
        let (status, body, raw, _) = get(&fx.state, ARTIFACTS_URI).await;
        assert_eq!(status, 502, "{raw}");
        assert_eq!(body["error"]["code"], "invalid_model_gateway_response");
        assert!(!raw.contains("space-other"));
    }

    #[tokio::test]
    async fn unknown_fields_are_dropped_content_is_never_present_and_last_page_cursor_is_null() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(&fx, grant()).await;
        let mut first = item("artifact-1");
        first["content"] = json!("SECRET-ARTIFACT-BODY");
        first["internal_note"] = json!("drop-me");
        first["space_id"] = json!("space-room");
        first["agent_name"] = json!("Analyst");
        mount_listing(
            &fx,
            ResponseTemplate::new(200)
                .set_body_json(json!({"artifacts": [first, item("artifact-2")]})),
        )
        .await;
        let (status, body, raw, cache) = get(&fx.state, &format!("{ARTIFACTS_URI}?limit=2")).await;
        assert_eq!(status, 200, "{raw}");
        assert_eq!(cache.as_deref(), Some("no-store"));
        assert!(!raw.contains("SECRET-ARTIFACT-BODY"));
        assert!(!raw.contains("drop-me"));
        assert_no_secret(&raw);
        let data = &body["data"];
        let artifacts = data["artifacts"].as_array().unwrap();
        assert_eq!(artifacts.len(), 2);
        let keys: std::collections::BTreeSet<_> = artifacts[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "agent_name",
                "artifact_id",
                "created_at",
                "kind",
                "message_id",
                "size_chars",
                "thread_id",
                "title",
                "version",
                "version_count",
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(artifacts[0]["agent_name"], "Analyst");
        assert!(artifacts[1].get("agent_name").is_none());
        assert!(data["next_cursor"].is_null());
        assert_eq!(data["shared_read"], json!({"available": true}));
        // The decision rides the upstream query string only.
        let urls = model_urls(&fx).await;
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("limit=2"));
        assert!(urls[0].contains("space_read_decision_ref=decision-1"));
        assert!(urls[0].contains(TOKEN_SECRET));
    }

    #[tokio::test]
    async fn a_next_cursor_passes_through_and_a_request_cursor_is_forwarded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(&fx, grant()).await;
        mount_listing(
            &fx,
            ResponseTemplate::new(200)
                .set_body_json(json!({"artifacts": [item("artifact-1")], "next_cursor": "cur-2"})),
        )
        .await;
        let (status, body, _, _) = get(&fx.state, &format!("{ARTIFACTS_URI}?cursor=cur-1")).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["next_cursor"], "cur-2");
        let urls = model_urls(&fx).await;
        assert!(urls[0].contains("cursor=cur-1"), "{}", urls[0]);
        assert!(urls[0].contains("limit=50"), "default limit: {}", urls[0]);
    }

    #[tokio::test]
    async fn retention_zdr_is_true_false_or_null_when_org_core_fails() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            (org_posture(true), json!(true)),
            (org_posture(false), json!(false)),
            (ResponseTemplate::new(500), Value::Null),
        ];
        for (posture, expected) in cases {
            let fx = fixture(posture, true).await;
            mount_read_decision(&fx, grant()).await;
            mount_listing(&fx, ResponseTemplate::new(200).set_body_json(json!({"artifacts": []})))
                .await;
            let (status, body, raw, _) = get(&fx.state, ARTIFACTS_URI).await;
            assert_eq!(status, 200, "{raw}");
            assert_eq!(body["data"]["retention"]["zdr"], expected, "{raw}");
            assert_eq!(body["data"]["shared_read"]["available"], true);
            assert_eq!(body["data"]["artifacts"], json!([]));
        }
    }

    #[tokio::test]
    async fn a_model_gateway_503_passes_through_without_the_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_read_decision(&fx, grant()).await;
        mount_listing(
            &fx,
            ResponseTemplate::new(503).set_body_json(json!({"error": {
                "code": "space_artifacts_unavailable",
                "message": "Space artifacts are unavailable.",
                "details": {}
            }})),
        )
        .await;
        let (status, body, raw, _) = get(&fx.state, ARTIFACTS_URI).await;
        assert_eq!(status, 503, "{raw}");
        assert_eq!(body["error"]["code"], "space_artifacts_unavailable");
        assert_no_secret(&raw);
    }
}
