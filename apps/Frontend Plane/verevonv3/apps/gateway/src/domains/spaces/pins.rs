//! Room pins: the BFF half of WS13-S8.
//!
//! `PUT` and `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}` pin and unpin a post for the
//! whole room, and `list_pins` is the `pins` section of the thread listing. Application (Convex
//! `spacePins`) stores identifiers and a timestamp only; this module decides nothing about
//! content and stores nothing itself. Session Core's owner-only `pinned_at` is a different,
//! personal fact and is untouched.
//!
//! Order of refusals for a pin (each one stops before the next runs): flag, organization, ids,
//! Space lifecycle, the caller's LIVE Control role (editor, manager or owner), the Control
//! shared-read decision, then proof that the thread exists in this Space and is readable by the
//! pinner (a one-turn read through model-gateway under the caller's own bearers), and only then
//! the Convex write. An unpin skips the read proof on purpose: a pin whose thread was archived or
//! erased must stay removable. The pin stores no title and no content, so a Zero Data Retention
//! organization has nothing to withhold; reading a pinned post still goes through Session Core
//! under each viewer's own decision.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_thread`, 403
//! `active_membership_required` / `room_role_forbidden` / `space_read_not_authorized`, 404
//! `feature_disabled` / `space_not_found` / `thread_not_found`, 409 `room_pins_limit`, 503
//! `space_lifecycle_unavailable` / `delegated_auth_unavailable` / `room_pins_unavailable`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::put,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{
    convex_gateway_call, denial, room_admin::convex_mutation_message,
    shared_thread_read_decision_for_typed, space_lifecycle_by_ref,
};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

type Reply = (StatusCode, Json<Value>);

/// Control roles that may pin or unpin. A viewer reads the pins, never changes them.
const PIN_ROLES: [&str; 3] = ["editor", "manager", "owner"];
const MAX_ID_LEN: usize = 200;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/pins/{thread_id}",
        put(pin_thread).delete(unpin_thread),
    )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "feature_disabled",
        "Room pins are not enabled.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "room_pins_unavailable",
        "The pin could not be changed. Nothing was changed.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control)
}

/// Organization, trimmed Space ref and thread id, with the Space lifecycle and the caller's live
/// role already checked. Everything both verbs share.
async fn authorize_change(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    thread_id: &str,
) -> Result<(String, String, String), Reply> {
    if !state.space_features.room_pins {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to pin a post.",
        ));
    }
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    let thread_id = thread_id.trim();
    if !valid_id(thread_id) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        ));
    }
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Err(refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            ))
        }
        Err(()) => {
            return Err(refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            ))
        }
    }
    // The live role decides, from Control, before any Application request.
    match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if PIN_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return Err(refuse(
                StatusCode::FORBIDDEN,
                "room_role_forbidden",
                "Only a room editor, manager or owner can pin a post.",
            ))
        }
        Err(refusal) => return Err(refusal),
    }
    Ok((org_id, space_ref.to_owned(), thread_id.to_owned()))
}

/// Proof that `thread_id` is a post in this Space the pinner may read: a one-turn read through
/// model-gateway under the pinner's own shared-read decision and bearers. A thread of another
/// Space, a missing one and one the pinner cannot read all answer 404 `thread_not_found`.
async fn prove_thread_in_space(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
) -> Result<(), Reply> {
    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(state, user, org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return Err(response),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Reading this Space's posts is not authorized.";
        return Err(match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            ),
            None => refuse(StatusCode::FORBIDDEN, "space_read_not_authorized", message),
        });
    };
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(reason) => return Err(crate::domains::chat::shared::delegated_auth_unavailable(reason)),
        };
    let url = format!(
        "{}/v1/threads/{}/messages?space_id={}&limit=1&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let (status, Json(_)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    match status {
        status if status.is_success() => Ok(()),
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN => Err(refuse(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "That post is not available in this room.",
        )),
        _ => Err(unavailable()),
    }
}

/// `PUT /api/v1/spaces/{space_ref}/pins/{thread_id}`. Idempotent: pinning a pinned post answers
/// 200 with the original pin time.
async fn pin_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> Reply {
    let (org_id, space_ref, thread_id) =
        match authorize_change(&state, &user, &space_ref, &thread_id).await {
            Ok(scope) => scope,
            Err(reply) => return reply,
        };
    if let Err(reply) =
        prove_thread_in_space(&state, &user, &headers, &org_id, &space_ref, &thread_id).await
    {
        return reply;
    }
    match convex_mutation_message(
        &state,
        "spacePins:setForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": space_ref,
                "thread_id": thread_id,
                "pinned": true,
                "pinned_at": value.get("pinnedAt").cloned().unwrap_or(Value::Null),
            }})),
        ),
        Err(Some(message)) if message.contains("room_pins_limit") => refuse(
            StatusCode::CONFLICT,
            "room_pins_limit",
            "This room has reached its pin limit. Unpin a post first.",
        ),
        Err(Some(message)) if message.contains("pin_invalid") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "The space or thread id is not valid.",
        ),
        Err(_) => unavailable(),
    }
}

/// `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}`. Idempotent: an absent pin answers 200.
async fn unpin_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> Reply {
    let (org_id, space_ref, thread_id) =
        match authorize_change(&state, &user, &space_ref, &thread_id).await {
            Ok(scope) => scope,
            Err(reply) => return reply,
        };
    match convex_mutation_message(
        &state,
        "spacePins:clearForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": space_ref,
                "thread_id": thread_id,
                "pinned": false,
            }})),
        ),
        Err(Some(message)) if message.contains("pin_invalid") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "The space or thread id is not valid.",
        ),
        Err(_) => unavailable(),
    }
}

/// The room's pins for the thread listing: `[{thread_id, pinned_by, pinned_at}]`, newest first.
/// `None` when the flag is off or Application could not be asked (the listing then names the gap
/// instead of failing), distinct from a room with no pins, which is `Some([])`.
pub(super) async fn list_pins(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Option<Value> {
    let result = convex_gateway_call(
        state,
        "query",
        "spacePins:listForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .ok()?;
    let value = result.get("value").unwrap_or(&result);
    let rows = value.get("pins")?.as_array()?;
    let pins: Vec<Value> = rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "thread_id": row.get("threadId")?.as_str()?,
                "pinned_by": row.get("pinnedByExternalAuthId")?.as_str()?,
                "pinned_at": row.get("pinnedAt")?.as_u64()?,
            }))
        })
        .collect();
    Some(Value::Array(pins))
}

#[cfg(test)]
mod tests {
    //! WS13-S8: the route's refusal order, idempotency and the listing's `pins` section, driven
    //! through the real router against mock Auth, Control, Application (Convex) and Model Gateway.
    use crate::config::AppState;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PIN: &str = "/api/v1/spaces/space-room/pins/thread-1";

    struct Fx {
        state: AppState,
        user_core: MockServer,
        application: MockServer,
        model_gateway: MockServer,
        _auth: MockServer,
    }

    /// The caller holds `role` in `space-room`. Control grants the shared read, the Space is active
    /// in Application, Model Gateway reads `thread-1` and Convex accepts pin writes, unless a test
    /// replaces a piece (a later mount does not override an earlier one, so those are built here
    /// from the options).
    struct Opts {
        role: &'static str,
        read_decision: u16,
        thread_read: u16,
        pin_mutation: ResponseTemplate,
        list_query: ResponseTemplate,
    }

    impl Opts {
        fn new(role: &'static str) -> Self {
            Self {
                role,
                read_decision: 200,
                thread_read: 200,
                pin_mutation: ResponseTemplate::new(200).set_body_json(json!({
                    "value": {"threadId": "thread-1", "pinnedAt": 1_760_000_000_000_u64, "changed": true}
                })),
                list_query: ResponseTemplate::new(200).set_body_json(json!({"value": {"pins": [
                    {"threadId": "thread-2", "pinnedByExternalAuthId": "user-9", "pinnedAt": 20},
                    {"threadId": "thread-1", "pinnedByExternalAuthId": "user-1", "pinnedAt": 10}
                ]}})),
            }
        }
    }

    async fn fx(opts: Opts, pins_on: bool) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "model-gateway", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})))
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": opts.role, "subject_id": "user-1"}
            })))
            .mount(&user_core)
            .await;
        let decision = if opts.read_decision == 200 {
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {"decision": {"decision_ref": "decision-1"}, "token": "decision-token"}
            }))
        } else {
            ResponseTemplate::new(opts.read_decision).set_body_json(json!({"error": {"code": "forbidden"}}))
        };
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(decision)
            .mount(&user_core)
            .await;

        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spacePins:listForGateway"})))
            .respond_with(opts.list_query)
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaceReadMarkers:spaceReadMarkerForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {"lastReadAt": 1}})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active"}
            ]})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(opts.pin_mutation)
            .mount(&application)
            .await;

        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/threads/thread-1/messages"))
            .respond_with(ResponseTemplate::new(opts.thread_read).set_body_json(json!({"messages": []})))
            .mount(&model_gateway)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/threads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"threads": [
                {"thread_id": "thread-1", "space_id": "space-room", "title": "Innkjop",
                 "updated_at": "2026-09-07T11:00:00Z", "owner_subject_id": "user-1"}
            ]})))
            .mount(&model_gateway)
            .await;

        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        state.space_features.room_pins = pins_on;
        Fx { state, user_core, application, model_gateway, _auth: auth }
    }

    async fn call(fx: &Fx, method: &str, uri: &str) -> (u16, Value) {
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body)
    }

    /// Convex mutations the application mock saw, as `{path, args}`.
    async fn mutations(fx: &Fx) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/api/mutation")
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    async fn control_paths(fx: &Fx) -> Vec<String> {
        fx.user_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.path().to_owned())
            .collect()
    }

    async fn model_gateway_reads(fx: &Fx) -> usize {
        fx.model_gateway.received_requests().await.unwrap().len()
    }

    #[tokio::test]
    async fn flag_off_answers_404_feature_disabled_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Opts::new("editor"), false).await;
        for method in ["PUT", "DELETE"] {
            let (status, body) = call(&fx, method, PIN).await;
            assert_eq!(status, 404, "{method}");
            assert_eq!(body["error"]["code"], "feature_disabled");
        }
        assert!(mutations(&fx).await.is_empty());
        assert!(!control_paths(&fx).await.iter().any(|path| path.ends_with("/membership")));
        assert_eq!(model_gateway_reads(&fx).await, 0);
    }

    #[tokio::test]
    async fn a_viewer_gets_403_room_role_forbidden_and_nothing_downstream_runs() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for method in ["PUT", "DELETE"] {
            let fx = fx(Opts::new("viewer"), true).await;
            let (status, body) = call(&fx, method, PIN).await;
            assert_eq!(status, 403, "{method}");
            assert_eq!(body["error"]["code"], "room_role_forbidden");
            assert!(mutations(&fx).await.is_empty(), "{method}: no Convex write");
            assert!(
                !control_paths(&fx).await.iter().any(|path| path.ends_with("thread-read-decision")),
                "{method}: no read decision is requested for a viewer"
            );
            assert_eq!(model_gateway_reads(&fx).await, 0);
        }
    }

    #[tokio::test]
    async fn a_declined_read_decision_is_403_and_nothing_is_written() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("editor");
        opts.read_decision = 403;
        let fx = fx(opts, true).await;
        let (status, body) = call(&fx, "PUT", PIN).await;
        assert_eq!(status, 403);
        assert!(body["error"]["code"].is_string());
        assert!(mutations(&fx).await.is_empty());
        assert_eq!(model_gateway_reads(&fx).await, 0);
    }

    #[tokio::test]
    async fn a_thread_that_is_not_in_the_space_is_404_thread_not_found_with_no_write() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for upstream in [404, 403] {
            let mut opts = Opts::new("editor");
            opts.thread_read = upstream;
            let fx = fx(opts, true).await;
            let (status, body) = call(&fx, "PUT", PIN).await;
            assert_eq!(status, 404, "model gateway {upstream}");
            assert_eq!(body["error"]["code"], "thread_not_found");
            assert!(mutations(&fx).await.is_empty());
        }
    }

    #[tokio::test]
    async fn a_model_gateway_failure_is_503_room_pins_unavailable_with_no_write() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("editor");
        opts.thread_read = 500;
        let fx = fx(opts, true).await;
        let (status, body) = call(&fx, "PUT", PIN).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "room_pins_unavailable");
        assert!(mutations(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn pinning_is_idempotent_and_writes_with_the_session_identity_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("editor");
        // The second call sees an already pinned post: Convex answers changed:false, the route 200.
        opts.pin_mutation = ResponseTemplate::new(200).set_body_json(json!({
            "value": {"threadId": "thread-1", "pinnedAt": 1_760_000_000_000_u64, "changed": false}
        }));
        let fx = fx(opts, true).await;
        for _ in 0..2 {
            let (status, body) = call(&fx, "PUT", PIN).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["pinned"], true);
            assert_eq!(body["data"]["thread_id"], "thread-1");
            assert_eq!(body["data"]["pinned_at"], 1_760_000_000_000_u64);
        }
        let writes = mutations(&fx).await;
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0]["path"], "spacePins:setForGateway");
        let args = &writes[0]["args"];
        assert_eq!(args["externalAuthId"], "user-1");
        assert_eq!(args["externalOrgId"], "org-1");
        assert_eq!(args["spaceRef"], "space-room");
        assert_eq!(args["threadId"], "thread-1");
    }

    #[tokio::test]
    async fn the_pin_limit_is_409_room_pins_limit() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("manager");
        opts.pin_mutation = ResponseTemplate::new(200)
            .set_body_json(json!({"status": "error", "errorMessage": "Uncaught Error: room_pins_limit"}));
        let fx = fx(opts, true).await;
        let (status, body) = call(&fx, "PUT", PIN).await;
        assert_eq!(status, 409);
        assert_eq!(body["error"]["code"], "room_pins_limit");
    }

    #[tokio::test]
    async fn a_convex_failure_hides_the_raw_error_text() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("owner");
        opts.pin_mutation = ResponseTemplate::new(200).set_body_json(
            json!({"status": "error", "errorMessage": "Server Error at secret-internal-host:3210 stack"}),
        );
        let fx = fx(opts, true).await;
        let (status, body) = call(&fx, "PUT", PIN).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "room_pins_unavailable");
        assert!(!body.to_string().contains("secret-internal-host"));
    }

    #[tokio::test]
    async fn unpinning_is_idempotent_and_skips_the_read_proof() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("editor");
        // Even a thread that is gone or unreadable stays removable.
        opts.thread_read = 404;
        opts.pin_mutation = ResponseTemplate::new(200)
            .set_body_json(json!({"value": {"threadId": "thread-1", "changed": false}}));
        let fx = fx(opts, true).await;
        for _ in 0..2 {
            let (status, body) = call(&fx, "DELETE", PIN).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["pinned"], false);
        }
        let writes = mutations(&fx).await;
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0]["path"], "spacePins:clearForGateway");
        assert_eq!(model_gateway_reads(&fx).await, 0, "an unpin proves nothing about the thread");
        assert!(!control_paths(&fx).await.iter().any(|path| path.ends_with("thread-read-decision")));
    }

    #[tokio::test]
    async fn the_thread_listing_carries_pins_when_on_and_an_empty_list_when_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let on = fx(Opts::new("editor"), true).await;
        let (status, body) = call(&on, "GET", "/api/v1/spaces/space-room/threads").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"]["pins"],
            json!([
                {"thread_id": "thread-2", "pinned_by": "user-9", "pinned_at": 20},
                {"thread_id": "thread-1", "pinned_by": "user-1", "pinned_at": 10}
            ])
        );
        assert!(!body["data"]["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["section"] == "pins"));

        let off = fx(Opts::new("editor"), false).await;
        let (status, body) = call(&off, "GET", "/api/v1/spaces/space-room/threads").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["pins"], json!([]));
        assert!(!body["data"]["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["section"] == "pins"));
    }

    #[tokio::test]
    async fn the_thread_listing_names_a_pins_gap_when_convex_fails_and_still_lists_threads() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut opts = Opts::new("editor");
        opts.list_query = ResponseTemplate::new(500).set_body_string("boom");
        let fx = fx(opts, true).await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/space-room/threads").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["pins"], json!([]));
        assert_eq!(body["data"]["threads"].as_array().unwrap().len(), 1);
        let gap = body["data"]["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .find(|gap| gap["section"] == "pins")
            .expect("a named pins gap");
        assert_eq!(gap["code"], "pins_unavailable");
    }
}
