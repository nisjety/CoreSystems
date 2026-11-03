//! The member's personal Spaces sidebar layout: the BFF half of WS16-S8.
//!
//! `GET` and `PUT /api/v1/spaces/sidebar` read and save one small document of references (starred
//! rooms, hidden rooms, named sections of room refs and closed group ids; see Application's
//! `spaceSidebarRules.ts` for the shape and caps). Application (Convex `spaceSidebar`) stores it,
//! compare-and-set on a revision. This module holds no Space state, mints no Control decision and
//! makes no Model Plane call: which rooms a person can open is still Control's answer, and nothing
//! in an authority path reads this document.
//!
//! The organization comes from the verified session membership and the user from the session; the
//! body can name neither. Keys other than `base_revision` and `doc` are refused. The document is
//! validated in Application (one rule set, one copy); the gateway checks only the envelope and
//! the 64 KiB body cap, so an oversized body never reaches Convex.
//!
//! Zero Data Retention split (SD-12 option A): labels in the document are user-authored text, so a
//! ZDR organization does not get a synced sidebar. `GET` answers `mode: "zdr_local"` with no
//! document and the SPA keeps a refs-and-labels-only copy in the browser; `PUT` is refused with
//! the typed 412 before any Application call. An org-core outage is a 503 on both verbs, never a
//! silent flip to `zdr_local` and never a guess toward "not ZDR".
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_sidebar_doc` (`details.reason`), 403
//! `active_membership_required`, 404 `space_sidebar_disabled`, 409 `space_sidebar_conflict`
//! (`details:{revision, doc}`), 412 `zdr_durable_mutation_forbidden`, 413 `sidebar_too_large`, 503
//! `space_sidebar_unavailable` / `zdr_decision_unavailable`.

use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, DefaultBodyLimit, Extension, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{durable_write_refusal, org_zdr_lookup, request_zdr, OrgZdrPosture},
};

type Reply = (StatusCode, Json<Value>);

/// The body cap of the `PUT` route. Application's own cap on the document is 32 KiB; this is the
/// transport ceiling above it (the JSON envelope and whitespace), so nothing larger is buffered.
const BODY_LIMIT_BYTES: usize = 64 * 1024;

/// The caps the SPA shows, in the plan's names. Mirrors Application's `SIDEBAR_LIMITS`
/// (`test/fixtures/space-sidebar-limits.json`); the golden parity test pins both copies.
fn limits() -> Value {
    json!({
        "sections": 30,
        "refs_per_section": 200,
        "starred": 100,
        "hidden": 500,
        "name": 40,
    })
}

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/sidebar",
        get(get_sidebar)
            .put(put_sidebar)
            .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
    )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_sidebar_disabled",
        "The Spaces sidebar layout is not enabled.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_sidebar_unavailable",
        "The sidebar layout is unavailable. Nothing was changed.",
    )
}

fn invalid(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_sidebar_doc",
            "The sidebar layout is not valid.",
            json!({ "reason": reason }),
        )),
    )
}

/// Flag, then the verified organization. Everything both verbs share before the ZDR split.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.sidebar {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to use the sidebar layout.",
        ));
    }
    Ok(org_id)
}

/// One Application call. `Ok` is the function's value; `Err(Some(message))` is a thrown error
/// (Convex answers a non-success status or `status: "error"` with an `errorMessage`);
/// `Err(None)` is every other failure (not configured, unreachable, unreadable).
pub(super) async fn convex_call(
    state: &AppState,
    operation: &str,
    path: &str,
    mut args: Value,
) -> Result<Value, Option<String>> {
    let convex_url = state.application_convex_url.clone();
    let service_key = state.application_convex_service_key.clone();
    if convex_url.trim().is_empty() || service_key.trim().is_empty() {
        return Err(None);
    }
    args.as_object_mut()
        .ok_or(None)?
        .insert("serviceKey".to_owned(), Value::String(service_key));
    let response = crate::upstream::send_with_retry(
        state
            .client
            .post(format!(
                "{}/api/{operation}",
                convex_url.trim_end_matches('/')
            ))
            .json(&json!({ "path": path, "args": args })),
    )
    .await
    .map_err(|_| None)?;
    let http_ok = response.status().is_success();
    let body = response.json::<Value>().await.map_err(|_| None)?;
    if http_ok && body.get("status").and_then(Value::as_str) != Some("error") {
        return body.get("value").cloned().ok_or(None);
    }
    Err(body
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// The validation token out of an `invalid_sidebar_doc:<reason>` Application error, lowercase
/// letters, digits and underscores only (nothing else from the message is ever returned). An
/// argument-validator failure (a key or type the table does not accept) is `doc_shape`.
fn invalid_reason(message: &str) -> Option<String> {
    const PREFIX: &str = "invalid_sidebar_doc:";
    if let Some(start) = message.find(PREFIX) {
        let token: String = message[start + PREFIX.len()..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .take(64)
            .collect();
        return Some(if token.is_empty() {
            "doc_shape".to_owned()
        } else {
            token
        });
    }
    if message.contains("ArgumentValidationError") || message.contains("Validator error") {
        return Some("doc_shape".to_owned());
    }
    None
}

/// `GET /api/v1/spaces/sidebar`.
async fn get_sidebar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    // A request-level ZDR assertion counts as ZDR; the org posture can only raise it further.
    let zdr_local = if request_zdr(&headers, None) {
        true
    } else {
        match org_zdr_lookup(&state, &user).await {
            OrgZdrPosture::Enabled => true,
            OrgZdrPosture::Disabled => false,
            OrgZdrPosture::Unknown => return unavailable(),
        }
    };
    if zdr_local {
        return (
            StatusCode::OK,
            Json(json!({"data": {
                "mode": "zdr_local",
                "revision": 0,
                "doc": Value::Null,
                "limits": limits(),
            }})),
        );
    }
    match convex_call(
        &state,
        "query",
        "spaceSidebar:getForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await
    {
        Ok(Value::Null) => (
            StatusCode::OK,
            Json(json!({"data": {
                "mode": "server",
                "revision": 0,
                "doc": Value::Null,
                "limits": limits(),
            }})),
        ),
        Ok(row) => match (row.get("revision").and_then(Value::as_u64), row.get("doc")) {
            (Some(revision), Some(doc)) if doc.is_object() => (
                StatusCode::OK,
                Json(json!({"data": {
                    "mode": "server",
                    "revision": revision,
                    "doc": doc,
                    "limits": limits(),
                }})),
            ),
            _ => unavailable(),
        },
        Err(_) => unavailable(),
    }
}

/// `PUT /api/v1/spaces/sidebar {"base_revision": n, "doc": {...}}`.
async fn put_sidebar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Reply {
    // The body is read lazily so the flag, organization and ZDR refusals come first.
    match gate(&state, &user).await {
        Ok(_) => {}
        Err(reply) => return reply,
    }
    if let Some(refusal) = durable_write_refusal(&state, &user, Some(&headers), None).await {
        return refusal;
    }
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return refuse(
                StatusCode::PAYLOAD_TOO_LARGE,
                "sidebar_too_large",
                "The sidebar layout is too large to save.",
            )
        }
        Err(_) => return invalid("body_unreadable"),
    };
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return invalid("body_not_json");
    };
    save_sidebar(&state, &user, &headers, &body).await
}

/// The whole save, shared with the `spaces.update_sidebar` action: flag, organization, ZDR, the
/// envelope, then the compare-and-set in Application. `body` is the untrusted
/// `{base_revision, doc}` object.
pub(crate) async fn save_sidebar(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    if let Some(refusal) = durable_write_refusal(state, user, Some(headers), Some(body)).await {
        return refusal;
    }
    let Some(object) = body.as_object() else {
        return invalid("body_not_object");
    };
    if object.keys().any(|key| key != "base_revision" && key != "doc") {
        return invalid("unknown_key");
    }
    let Some(base_revision) = object.get("base_revision").and_then(Value::as_u64) else {
        return invalid("bad_base_revision");
    };
    let Some(doc) = object.get("doc").filter(|doc| doc.is_object()) else {
        return invalid("doc_not_object");
    };
    // A second cheap bound for the action path, which has no body extractor in front of it.
    if doc.to_string().len() > BODY_LIMIT_BYTES {
        return refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "sidebar_too_large",
            "The sidebar layout is too large to save.",
        );
    }
    match convex_call(
        state,
        "mutation",
        "spaceSidebar:putForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "baseRevision": base_revision,
            "doc": doc,
        }),
    )
    .await
    {
        Ok(result) => match result.get("status").and_then(Value::as_str) {
            Some("ok") => match result.get("revision").and_then(Value::as_u64) {
                Some(revision) => (
                    StatusCode::OK,
                    Json(json!({"data": { "revision": revision, "doc": doc }})),
                ),
                None => unavailable(),
            },
            Some("conflict") => {
                let mut details = Map::new();
                details.insert(
                    "revision".to_owned(),
                    result.get("revision").cloned().unwrap_or(Value::Null),
                );
                details.insert(
                    "doc".to_owned(),
                    result.get("doc").cloned().unwrap_or(Value::Null),
                );
                (
                    StatusCode::CONFLICT,
                    Json(error_with_details(
                        "space_sidebar_conflict",
                        "The sidebar layout changed somewhere else. Reload it and apply your change again.",
                        Value::Object(details),
                    )),
                )
            }
            _ => unavailable(),
        },
        Err(Some(message)) => match invalid_reason(&message) {
            Some(reason) => invalid(&reason),
            None => unavailable(),
        },
        Err(None) => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::{get_sidebar, invalid_reason};
    use crate::config::AppState;
    use crate::middleware::AuthenticatedUser;
    use axum::{
        body::Body,
        extract::{Extension, State},
        http::{HeaderMap, Request},
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Clone, Copy, PartialEq)]
    enum Zdr {
        Off,
        On,
        OrgCoreDown,
    }

    struct Fx {
        state: AppState,
        app: MockServer,
        _servers: Vec<MockServer>,
    }

    /// A signed-in member of `org-1` through the real router. Application answers `get_reply` to
    /// `spaceSidebar:getForGateway` and `put_reply` to `spaceSidebar:putForGateway` (an
    /// unanswered function is a 404 from the mock, which the gateway treats as unavailable).
    /// The flag is on.
    async fn fixture(
        zdr: Zdr,
        get_reply: Option<ResponseTemplate>,
        put_reply: Option<ResponseTemplate>,
    ) -> Fx {
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
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        let org_reply = match zdr {
            Zdr::Off => ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": false}}}}),
            ),
            Zdr::On => ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": true}}}}),
            ),
            Zdr::OrgCoreDown => ResponseTemplate::new(500).set_body_json(json!({"error": "down"})),
        };
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_reply)
            .mount(&org)
            .await;
        let app = MockServer::start().await;
        if let Some(reply) = get_reply {
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/query"))
                .and(body_partial_json(json!({"path": "spaceSidebar:getForGateway"})))
                .respond_with(reply)
                .mount(&app)
                .await;
        }
        if let Some(reply) = put_reply {
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/mutation"))
                .and(body_partial_json(json!({"path": "spaceSidebar:putForGateway"})))
                .respond_with(reply)
                .mount(&app)
                .await;
        }
        let mut state = crate::tests::test_state(false);
        state.space_features.sidebar = true;
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        Fx { state, app, _servers: vec![auth, user_core, org] }
    }

    async fn call(
        state: &AppState,
        verb: &str,
        body: Option<Vec<u8>>,
        extra: Option<(&str, &str)>,
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .method(verb)
            .uri("/api/v1/spaces/sidebar")
            .header("cookie", "better-auth.session_token=session-1")
            .header("content-type", "application/json");
        if let Some((name, value)) = extra {
            request = request.header(name, value);
        }
        let response = crate::build_router(state.clone())
            .oneshot(request.body(Body::from(body.unwrap_or_default())).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn put_body(base_revision: Value, doc: Value) -> Option<Vec<u8>> {
        Some(serde_json::to_vec(&json!({"base_revision": base_revision, "doc": doc})).unwrap())
    }

    async fn convex_requests(fx: &Fx) -> Vec<Value> {
        fx.app
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    fn ok_put(revision: u64) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"status": "success", "value": {"status": "ok", "revision": revision}}))
    }

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    fn doc() -> Value {
        json!({"starred": ["space-a"], "hidden": [], "sections": [], "closed_groups": []})
    }

    #[tokio::test]
    async fn flag_off_is_404_space_sidebar_disabled_on_get_and_put_before_org_zdr_and_body() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(Zdr::On, None, None).await;
        fx.state.space_features.sidebar = false;
        for (verb, body) in [("GET", None), ("PUT", Some(b"{not json".to_vec()))] {
            let (status, body) = call(&fx.state, verb, body, None).await;
            assert_eq!(status, 404, "{verb}");
            assert_eq!(body["error"]["code"], "space_sidebar_disabled");
        }
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn get_returns_server_mode_with_null_doc_revision_0_for_a_new_member() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            Zdr::Off,
            Some(ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": null}))),
            None,
        )
        .await;
        let (status, body) = call(&fx.state, "GET", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["mode"], "server");
        assert_eq!(body["data"]["revision"], 0);
        assert!(body["data"]["doc"].is_null());
        assert_eq!(body["data"]["limits"]["sections"], 30);
        assert_eq!(body["data"]["limits"]["name"], 40);
        let sent = convex_requests(&fx).await;
        assert_eq!(sent[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(sent[0]["args"]["externalOrgId"], "org-1");
    }

    #[tokio::test]
    async fn get_returns_the_stored_row_revision_and_doc() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            Zdr::Off,
            Some(ResponseTemplate::new(200).set_body_json(
                json!({"status": "success", "value": {"revision": 7, "doc": doc(), "userId": "leak"}}),
            )),
            None,
        )
        .await;
        let (status, body) = call(&fx.state, "GET", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["mode"], "server");
        assert_eq!(body["data"]["revision"], 7);
        assert_eq!(body["data"]["doc"], doc());
        assert!(!body.to_string().contains("leak"));
        // A row without a usable revision or doc is unavailable, never a made-up document.
        let fx = fixture(
            Zdr::Off,
            Some(ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {"revision": 1}}))),
            None,
        )
        .await;
        let (status, body) = call(&fx.state, "GET", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_sidebar_unavailable");
    }

    #[tokio::test]
    async fn get_for_a_zdr_org_or_the_x_zdr_header_answers_zdr_local_without_a_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::On, None, None).await;
        let (status, body) = call(&fx.state, "GET", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["mode"], "zdr_local");
        assert_eq!(body["data"]["revision"], 0);
        assert!(body["data"]["doc"].is_null());
        assert!(body["data"]["limits"].is_object());
        let fx2 = fixture(Zdr::Off, None, None).await;
        let (status, body) = call(&fx2.state, "GET", None, Some(("x-zdr", "true"))).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["mode"], "zdr_local");
        assert!(convex_requests(&fx).await.is_empty());
        assert!(convex_requests(&fx2).await.is_empty());
    }

    #[tokio::test]
    async fn get_with_org_core_down_is_503_space_sidebar_unavailable_not_zdr_local() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::OrgCoreDown, None, None).await;
        let (status, body) = call(&fx.state, "GET", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_sidebar_unavailable");
        assert!(body["data"].is_null());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn get_with_no_authorized_org_is_403_active_membership_required() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::Off, None, None).await;
        let user = AuthenticatedUser {
            user_id: "user-1".into(),
            user_email: "user@example.com".into(),
            user_name: "User".into(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".into()),
            active_org_id: None,
            authorized_membership: None,
        };
        let response = get_sidebar(State(fx.state.clone()), Extension(user), HeaderMap::new()).await;
        assert_eq!(response.0.as_u16(), 403);
        assert_eq!(response.1 .0["error"]["code"], "active_membership_required");
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn put_for_a_zdr_org_is_412_catalogue_shape_with_zero_convex_calls() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::On, None, Some(ok_put(1))).await;
        let (status, body) = call(&fx.state, "PUT", put_body(json!(0), doc()), None).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["error"]["action"], "none");
        assert!(body["error"]["message"].is_string());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn put_with_org_core_down_is_503_zdr_decision_unavailable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::OrgCoreDown, None, Some(ok_put(1))).await;
        let (status, body) = call(&fx.state, "PUT", put_body(json!(0), doc()), None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(body["error"]["retryable"], true);
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn put_ok_takes_org_and_user_from_the_session_and_returns_revision_and_doc() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::Off, None, Some(ok_put(4))).await;
        let (status, body) = call(&fx.state, "PUT", put_body(json!(3), doc()), None).await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({"data": {"revision": 4, "doc": doc()}}));
        let sent = convex_requests(&fx).await;
        let args = &sent[0]["args"];
        assert_eq!(args["externalAuthId"], "user-1");
        assert_eq!(args["externalOrgId"], "org-1");
        assert_eq!(args["baseRevision"], 3);
        assert_eq!(args["doc"], doc());
        // The body cannot name an org or a user: any extra key is refused before Convex.
        let before = sent.len();
        let forged = serde_json::to_vec(&json!({
            "base_revision": 3, "doc": doc(), "org_id": "org-2", "user_id": "user-2"
        }))
        .unwrap();
        let (status, body) = call(&fx.state, "PUT", Some(forged), None).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_sidebar_doc");
        assert_eq!(body["error"]["details"]["reason"], "unknown_key");
        assert_eq!(convex_requests(&fx).await.len(), before);
    }

    #[tokio::test]
    async fn put_conflict_is_409_with_the_current_revision_and_doc() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let current = json!({"starred": [], "hidden": ["space-z"], "sections": [], "closed_groups": []});
        let fx = fixture(
            Zdr::Off,
            None,
            Some(ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
                "status": "conflict", "revision": 9, "doc": current
            }}))),
        )
        .await;
        let (status, body) = call(&fx.state, "PUT", put_body(json!(2), doc()), None).await;
        assert_eq!(status, 409);
        assert_eq!(body["error"]["code"], "space_sidebar_conflict");
        assert_eq!(body["error"]["details"]["revision"], 9);
        assert_eq!(body["error"]["details"]["doc"], current);
    }

    #[tokio::test]
    async fn put_envelope_errors_are_400_with_details_reason_and_no_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::Off, None, Some(ok_put(1))).await;
        let raw = |value: Value| Some(serde_json::to_vec(&value).unwrap());
        let cases: Vec<(Option<Vec<u8>>, &str)> = vec![
            (raw(json!({"base_revision": -1, "doc": doc()})), "bad_base_revision"),
            (raw(json!({"base_revision": 1.5, "doc": doc()})), "bad_base_revision"),
            (raw(json!({"base_revision": "1", "doc": doc()})), "bad_base_revision"),
            (raw(json!({"doc": doc()})), "bad_base_revision"),
            (raw(json!({"base_revision": 1, "doc": []})), "doc_not_object"),
            (raw(json!({"base_revision": 1, "doc": "x"})), "doc_not_object"),
            (raw(json!({"base_revision": 1})), "doc_not_object"),
            (raw(json!([1, 2])), "body_not_object"),
            (Some(b"{not json".to_vec()), "body_not_json"),
        ];
        for (index, (body, reason)) in cases.into_iter().enumerate() {
            let (status, body) = call(&fx.state, "PUT", body, None).await;
            assert_eq!(status, 400, "case {index}");
            assert_eq!(body["error"]["code"], "invalid_sidebar_doc");
            assert_eq!(body["error"]["details"]["reason"], reason, "case {index}");
        }
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn convex_validation_errors_map_to_400_with_the_reason_and_everything_else_to_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (message, reason) in [
            ("Uncaught Error: invalid_sidebar_doc:too_many_sections at handler", "too_many_sections"),
            ("[Request ID: x] Server Error ArgumentValidationError: Object contains extra field", "doc_shape"),
            ("invalid_sidebar_doc:", "doc_shape"),
        ] {
            let fx = fixture(Zdr::Off, None, Some(convex_error(message))).await;
            let (status, body) = call(&fx.state, "PUT", put_body(json!(0), doc()), None).await;
            assert_eq!(status, 400, "{message}");
            assert_eq!(body["error"]["code"], "invalid_sidebar_doc");
            assert_eq!(body["error"]["details"]["reason"], reason);
            assert!(!body.to_string().contains("Uncaught"), "no upstream text leaks");
        }
        for reply in [
            convex_error("something else broke"),
            ResponseTemplate::new(500).set_body_string("boom"),
            ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {"status": "weird"}})),
        ] {
            let fx = fixture(Zdr::Off, None, Some(reply)).await;
            let (status, body) = call(&fx.state, "PUT", put_body(json!(0), doc()), None).await;
            assert_eq!(status, 503);
            assert_eq!(body["error"]["code"], "space_sidebar_unavailable");
        }
        // Convex not configured at all.
        let mut fx = fixture(Zdr::Off, None, Some(ok_put(1))).await;
        fx.state.application_convex_url = String::new();
        let (status, body) = call(&fx.state, "PUT", put_body(json!(0), doc()), None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_sidebar_unavailable");
    }

    #[test]
    fn invalid_reason_returns_a_bounded_lowercase_token_only() {
        assert_eq!(invalid_reason("x invalid_sidebar_doc:name_too_long, more").as_deref(), Some("name_too_long"));
        assert_eq!(invalid_reason("invalid_sidebar_doc:UPPER").as_deref(), Some("doc_shape"));
        assert_eq!(invalid_reason("Validator error: bad").as_deref(), Some("doc_shape"));
        assert_eq!(invalid_reason("nothing relevant"), None);
        let long = format!("invalid_sidebar_doc:{}", "a".repeat(200));
        assert_eq!(invalid_reason(&long).unwrap().len(), 64);
    }

    #[tokio::test]
    async fn a_body_over_64_kib_is_413_sidebar_too_large_typed_with_no_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(Zdr::Off, None, Some(ok_put(1))).await;
        let big = json!({"base_revision": 0, "doc": {"starred": ["x".repeat(70 * 1024)]}});
        let (status, body) = call(&fx.state, "PUT", Some(serde_json::to_vec(&big).unwrap()), None).await;
        assert_eq!(status, 413);
        assert_eq!(body["error"]["code"], "sidebar_too_large");
        assert!(convex_requests(&fx).await.is_empty());
    }
}
