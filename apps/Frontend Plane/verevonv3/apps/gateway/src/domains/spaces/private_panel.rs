//! WS17-S6: the owner's private panel, the gateway side.
//!
//! Routes (merged by `module_routes()` in `spaces/mod.rs`, behind the session):
//! `GET /api/v1/spaces/{space_ref}/private-panel` reads the caller's panel pointer for one room and
//! `POST` opens it (get or create). The action `spaces.open_private_panel` runs the same open core
//! (`dispatchers_spaces/ws17.rs`), so the flag, the ZDR refusal, the membership check and the typed
//! errors exist once.
//!
//! The gateway stores nothing. The pointer (ids only: the panel's thread id and the agent subject) lives
//! in Convex (`convex/spacePrivatePanels.ts`, read through `getSpacePrivatePanelForGateway`). Room
//! membership is Control's fact (`/api/v1/internal/spaces/{ref}/membership`) under the caller's own
//! delegation, and it is checked before any Convex call. Nothing here reads a message, and the browser
//! never holds a token. No flag is an authorization input.
//!
//! Creation (W3-FIX-9): a room with no pointer gets its owner thread from model-gateway
//! (`POST /v1/chat/conversation-threads`), created with `source_scope` conversation (SD-22 v1). session-core
//! locks that scope at insert and refuses, never returns, a workspace thread under the same key. The key is a
//! digest of the room reference, so a retry or a race finds the same thread. Only the returned id is recorded
//! in Convex (first writer wins). A failure before the record writes no pointer and answers a typed refusal.
//!
//! Order of refusals on every route (each stops the next): flag (404 `feature_disabled`), organization
//! (403 `active_membership_required`), Space reference (400 `invalid_space`), Zero Data Retention (typed
//! 412 `zdr_durable_mutation_forbidden`, before any Control or Convex call), Control membership (403
//! `active_membership_required`; a Control outage is 503 `space_decision_unavailable`), the Convex
//! pointer read (503 `private_panel_unavailable`), then creation (503 `private_panel_unavailable`, or the
//! typed ZDR refusal labelled with this surface).
//! Errors are `{error:{code,message,details}}`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        required_session_token,
    },
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::durable_write_refusal,
};

use super::{control_membership_url, convex_gateway_call, denial};

type Reply = (StatusCode, Json<Value>);

/// Convex's id bound (`requireId`): the gateway refuses a longer Space reference first.
const MAX_SPACE_REF_CHARS: usize = 200;
/// The surface a ZDR refusal names (the catalogue code is feature-less).
const PRIVATE_PANEL_SURFACE: &str = "private_panel";
const POINTER_QUERY: &str = "spacePrivatePanels:getSpacePrivatePanelForGateway";
/// The Convex record of the pointer, first writer wins: the thread id is the one model-gateway returned.
const RECORD_MUTATION: &str = "spacePrivatePanels:recordSpacePrivatePanelForGateway";
/// model-gateway's create of the owner's conversation-scope thread (W3-FIX-9).
const CONVERSATION_THREAD_PATH: &str = "/v1/chat/conversation-threads";
/// The session-key prefix of a room's private thread. session-core keys a thread by (organization, owner,
/// session key), so one key is one owner's panel in one room.
const PRIVATE_SESSION_PREFIX: &str = "private-panel-";

/// Which half of the surface a request is. Membership is read differently: a read keeps an archived
/// room's own pointer visible, a write needs a live room.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Open,
}

/// The WS17-S6 routes, merged by `module_routes()` in `spaces/mod.rs`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/private-panel",
        get(get_private_panel).post(open_private_panel),
    )
}

/// `GET .../private-panel`: the caller's pointer for one room, or `thread_id: null` when there is none.
async fn get_private_panel(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Reply {
    gated(&state, &user, &headers, &space_ref, Access::Read).await
}

/// `POST .../private-panel`: get or create the caller's pointer for one room.
async fn open_private_panel(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Reply {
    gated(&state, &user, &headers, &space_ref, Access::Open).await
}

/// `spaces.open_private_panel`: the open core, with the request headers the ZDR check reads.
pub(crate) async fn open_private_panel_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
) -> Reply {
    gated(state, user, headers, space_ref, Access::Open).await
}

async fn gated(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    access: Access,
) -> Reply {
    if !state.space_features.private_panel_ui {
        return feature_disabled();
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_membership();
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return invalid_space();
    }
    if let Some((status, Json(body))) = durable_write_refusal(state, user, Some(headers), None).await {
        return label_zdr(status, body);
    }
    if let Err(reply) = membership(state, user, &org_id, space_ref, access == Access::Read).await {
        return reply;
    }
    let pointer = match pointer_for(state, user, &org_id, space_ref).await {
        Ok(pointer) => pointer,
        Err(reply) => return reply,
    };
    match (pointer, access) {
        (Some(pointer), _) => ok(panel_body(space_ref, Some(&pointer))),
        (None, Access::Read) => ok(panel_body(space_ref, None)),
        (None, Access::Open) => create_pointer(state, user, headers, &org_id, space_ref).await,
    }
}

/// Control's membership fact for the caller, under their own delegation. A Control refusal is the
/// caller's lack of membership (403); a Control outage is 503. Nothing is decided here.
async fn membership(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    readable: bool,
) -> Result<(), Reply> {
    let url = control_membership_url(state, space_ref, readable);
    let actor = actor_for(user);
    let (status, Json(body)) =
        proxy_json(state, Method::GET, &url, None, Some(org_id), Some(&actor), None).await;
    if status.is_success() {
        return Ok(());
    }
    if denial::refusal_status(status, &body).is_server_error() {
        return Err(decision_unavailable());
    }
    Err(no_membership())
}

/// The caller's pointer row from Convex, or `None` when there is none. Read only after membership.
async fn pointer_for(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Option<Value>, Reply> {
    let value = convex_gateway_call(
        state,
        "query",
        POINTER_QUERY,
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .map_err(|()| unavailable())?;
    Ok(value.is_object().then_some(value))
}

/// `POST` with no pointer: model-gateway gets or creates the owner's conversation-scope thread for this room,
/// then its id is recorded in Convex. This runs only after Control's membership check and the ZDR refusal, and
/// only the recorded pointer is ever answered. A failure before the record writes no pointer and answers a typed
/// refusal. No path returns a workspace thread.
async fn create_pointer(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
) -> Reply {
    let model_bearer = model_token(state, user, headers).await;
    let session_bearer = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return delegated_auth_unavailable(cause),
    };
    let url = format!("{}{CONVERSATION_THREAD_PATH}", state.model_gateway_url);
    let (status, Json(body)) = proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({ "session_key": private_session_key(space_ref) })),
        model_bearer.as_deref(),
        Some(&session_bearer),
        user,
    )
    .await;
    if !status.is_success() {
        return creation_refused(status, body);
    }
    let Some(thread_id) = body
        .get("thread_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    else {
        return unavailable();
    };
    match convex_gateway_call(
        state,
        "mutation",
        RECORD_MUTATION,
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(recorded) if recorded.is_object() => ok(panel_body(space_ref, Some(&recorded))),
        _ => unavailable(),
    }
}

/// The session key of the room's private thread: a digest of the room reference, so no key carries the raw
/// reference and every key is a valid session key.
fn private_session_key(space_ref: &str) -> String {
    format!("{PRIVATE_SESSION_PREFIX}{}", hex::encode(Sha256::digest(space_ref.as_bytes())))
}

/// The answer for a refused create. The typed ZDR refusal keeps its catalogue code under this surface; anything
/// else is `private_panel_unavailable`. No pointer is written in either case.
fn creation_refused(status: StatusCode, body: Value) -> Reply {
    if status == StatusCode::PRECONDITION_FAILED {
        return label_zdr(status, body);
    }
    unavailable()
}

/// The wire body both routes answer with: ids only. An absent pointer or field is null.
fn panel_body(space_ref: &str, pointer: Option<&Value>) -> Value {
    let field = |name: &str| {
        pointer
            .and_then(|pointer| pointer.get(name))
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "space_ref": space_ref,
        "thread_id": field("threadId"),
        "agent_subject_id": field("agentSubjectId"),
    })
}

fn ok(data: Value) -> Reply {
    (StatusCode::OK, Json(json!({ "data": data })))
}

/// The actor the gateway signs for at Control, exactly as the existing room routes do.
fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn valid_space_ref(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_SPACE_REF_CHARS
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '\\'))
}

/// The typed ZDR refusal, labelled with the private panel surface.
fn label_zdr(status: StatusCode, mut body: Value) -> Reply {
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert("details".to_owned(), json!({ "surface": PRIVATE_PANEL_SURFACE }));
        }
    }
    (status, Json(body))
}

fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "The private panel is not enabled in this environment.",
        )),
    )
}

fn no_membership() -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "active_membership_required",
            "An active membership is required for this room.",
        )),
    )
}

fn invalid_space() -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_space", "A Space reference is required.")),
    )
}

fn decision_unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "space_decision_unavailable",
            "Room authority could not be confirmed right now. Nothing was changed.",
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "private_panel_unavailable",
            "Your private assistant is temporarily unavailable. Try again; nothing was changed.",
        )),
    )
}

#[cfg(test)]
mod tests {
    //! WS17-S6b (the refusal order, the read and the open answers) and W3-FIX-9 (creation through model-gateway,
    //! then the pointer record), through the real router. The session, Control, org-core, Convex and model-gateway
    //! are doubles, and every "no call" is checked by path or by the Convex operation named in the request body.
    //!
    //! The brief's WS17-S6b case "POST with no pointer gives 501 private_panel_creation_unavailable" is not written:
    //! the handler no longer has that answer. W3-FIX-9 made the create real, so the code answers 200 (see the
    //! `post_with_no_pointer_...` test below).
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const ROUTE: &str = "/api/v1/spaces/space-room/private-panel";
    const MEMBERSHIP: &str = "/api/v1/internal/spaces/space-room/membership";
    const ORG_PATH: &str = "/api/v1/organizations/org-1";
    const SPACE: &str = "space-room";

    struct Fixture {
        state: AppState,
        auth: MockServer,
        user_core: MockServer,
        org_core: MockServer,
        convex: MockServer,
        model: MockServer,
    }

    async fn fixture() -> Fixture {
        let auth = MockServer::start().await;
        let user_core = MockServer::start().await;
        let org_core = MockServer::start().await;
        let convex = MockServer::start().await;
        let model = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = convex.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.model_gateway_url = model.uri();
        state.space_features.private_panel_ui = true;
        Fixture {
            state,
            auth,
            user_core,
            org_core,
            convex,
            model,
        }
    }

    async fn answer(server: &MockServer, verb: &str, route: &str, response: ResponseTemplate) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(response)
            .mount(server)
            .await;
    }

    /// A Convex answer for one named operation, whichever endpoint (`query` or `mutation`) carries it.
    async fn convex(f: &Fixture, operation: &str, response: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(body_partial_json(json!({ "path": operation })))
            .respond_with(response)
            .mount(&f.convex)
            .await;
    }

    fn convex_value(value: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({ "value": value }))
    }

    fn org_record(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}))
    }

    fn membership_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {"membership": "active"}}))
    }

    /// The session the router resolves for the caller: org-1, role member, and the session-core token the
    /// create path mints.
    async fn session(f: &Fixture) {
        answer(
            &f.auth,
            "GET",
            "/api/auth/get-session",
            ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })),
        )
        .await;
        for slug in ["session-core", "capability-core"] {
            answer(
                &f.auth,
                "GET",
                &format!("/api/{slug}/token"),
                ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
            )
            .await;
        }
        answer(
            &f.user_core,
            "GET",
            "/api/v1/me/session-context",
            ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })),
        )
        .await;
    }

    /// The session, the organisation posture and Control's membership answer for org-1 and space-room.
    async fn base(f: &Fixture, zdr: bool, membership: ResponseTemplate) {
        session(f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(zdr)).await;
        answer(&f.user_core, "GET", MEMBERSHIP, membership).await;
    }

    /// One request through the real router with the session cookie: the status and the JSON body.
    async fn call(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder
            .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
            .expect("a request");
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn calls_to(server: &MockServer, part: &str) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path().contains(part))
            .collect()
    }

    /// Every Convex operation a server received, in arrival order, by the `path` in each body.
    async fn convex_ops(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter_map(|body| body.get("path").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    /// The `args` of every call to one Convex operation.
    async fn convex_args(server: &MockServer, operation: &str) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body.get("path").and_then(Value::as_str) == Some(operation))
            .map(|body| body.get("args").cloned().unwrap_or(Value::Null))
            .collect()
    }

    // ---- WS17-S6b: the refusal order on both routes ----------------------------------------------

    /// The flag is the first refusal: 404 `feature_disabled` on both routes, and nothing upstream is called.
    #[tokio::test]
    async fn flag_off_is_404_feature_disabled_for_get_and_post_with_no_upstream_call() {
        let mut f = fixture().await;
        f.state.space_features.private_panel_ui = false;
        base(&f, false, membership_ok()).await;
        convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
        for method in ["GET", "POST"] {
            let (status, body) = call(f.state.clone(), method, ROUTE, None).await;
            assert_eq!(status, 404, "{method}");
            assert_eq!(body["error"]["code"], "feature_disabled", "{method}");
        }
        assert!(calls_to(&f.user_core, "/membership").await.is_empty(), "no Control call");
        assert!(calls_to(&f.org_core, ORG_PATH).await.is_empty(), "no org-core call");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex call");
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "no model call");
    }

    /// A ZDR org is refused on both routes with the typed 412 labelled `private_panel`, before any Control or Convex
    /// call. The model is not reached either.
    #[tokio::test]
    async fn a_zdr_org_is_412_on_read_and_open_before_any_control_or_convex_call() {
        let f = fixture().await;
        base(&f, true, membership_ok()).await;
        for method in ["GET", "POST"] {
            let (status, body) = call(f.state.clone(), method, ROUTE, None).await;
            assert_eq!(status, 412, "{method} {body}");
            assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden", "{method}");
            assert_eq!(body["error"]["details"]["surface"], "private_panel", "{method}");
        }
        assert!(calls_to(&f.user_core, "/membership").await.is_empty(), "no Control call");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex call");
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "no model call");
    }

    /// Control's 403 is the caller's lack of room membership: 403 `active_membership_required`, and no Convex read.
    #[tokio::test]
    async fn a_control_refusal_is_403_active_membership_required_with_no_convex_call() {
        let f = fixture().await;
        base(
            &f,
            false,
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "forbidden"}})),
        )
        .await;
        for method in ["GET", "POST"] {
            let (status, body) = call(f.state.clone(), method, ROUTE, None).await;
            assert_eq!(status, 403, "{method}");
            assert_eq!(body["error"]["code"], "active_membership_required", "{method}");
        }
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex call");
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "no model call");
    }

    /// A Control outage (500 or 503) is 503 `space_decision_unavailable`, and no Convex read is made.
    #[tokio::test]
    async fn a_control_outage_is_503_space_decision_unavailable_with_no_convex_call() {
        for outage in [500_u16, 503] {
            let f = fixture().await;
            base(
                &f,
                false,
                ResponseTemplate::new(outage).set_body_json(json!({"error": {"code": "unavailable"}})),
            )
            .await;
            for method in ["GET", "POST"] {
                let (status, body) = call(f.state.clone(), method, ROUTE, None).await;
                assert_eq!(status, 503, "control {outage} {method}");
                assert_eq!(body["error"]["code"], "space_decision_unavailable", "control {outage} {method}");
            }
            assert!(convex_ops(&f.convex).await.is_empty(), "control {outage}: no Convex call");
        }
    }

    /// A blank, a tab-only or a control-character Space reference is 400 `invalid_space` on both routes, before any
    /// Control, Convex or model call.
    #[tokio::test]
    async fn a_blank_or_control_character_space_ref_is_400_invalid_space_before_any_upstream() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        for segment in ["%20", "%09", "%01room"] {
            let uri = format!("/api/v1/spaces/{segment}/private-panel");
            for method in ["GET", "POST"] {
                let (status, body) = call(f.state.clone(), method, &uri, None).await;
                assert_eq!(status, 400, "{method} {uri}");
                assert_eq!(body["error"]["code"], "invalid_space", "{method} {uri}");
            }
        }
        assert!(calls_to(&f.user_core, "/membership").await.is_empty(), "no Control call");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex call");
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "no model call");
    }

    // ---- WS17-S6b and W3-FIX-9: the read and the open answers ------------------------------------

    /// GET with no pointer answers `thread_id: null` and never creates a thread or a pointer.
    #[tokio::test]
    async fn get_with_no_pointer_answers_thread_id_null() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
        let (status, body) = call(f.state.clone(), "GET", ROUTE, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({"space_ref": SPACE, "thread_id": null, "agent_subject_id": null})
        );
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "a read never creates");
        assert!(convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await.is_empty());
    }

    /// POST with an existing pointer answers it and never creates: no model call and no pointer write.
    #[tokio::test]
    async fn post_with_an_existing_pointer_answers_it_and_never_creates() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        convex(
            &f,
            "spacePrivatePanels:getSpacePrivatePanelForGateway",
            convex_value(json!({"threadId": "thread-7", "agentSubjectId": "agent-9"})),
        )
        .await;
        let (status, body) = call(f.state.clone(), "POST", ROUTE, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({"space_ref": SPACE, "thread_id": "thread-7", "agent_subject_id": "agent-9"})
        );
        assert!(calls_to(&f.model, "/v1/chat/conversation-threads").await.is_empty(), "no model call");
        assert!(convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await.is_empty());
    }

    /// W3-FIX-9: POST with no pointer creates the owner's thread through model-gateway under a `private-panel-` key
    /// (a digest of the room reference), records the returned id, and answers 200 with it. This is the code as it
    /// stands; it replaces the WS17-S6b 501 case.
    #[tokio::test]
    async fn post_with_no_pointer_creates_the_thread_through_the_model_and_records_it() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
        answer(
            &f.model,
            "POST",
            "/v1/chat/conversation-threads",
            ResponseTemplate::new(200).set_body_json(json!({"thread_id": "thread-new"})),
        )
        .await;
        convex(
            &f,
            "spacePrivatePanels:recordSpacePrivatePanelForGateway",
            convex_value(json!({"threadId": "thread-new", "agentSubjectId": null})),
        )
        .await;
        let (status, body) = call(f.state.clone(), "POST", ROUTE, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["thread_id"], "thread-new");

        let sent = calls_to(&f.model, "/v1/chat/conversation-threads").await;
        assert_eq!(sent.len(), 1, "one model create");
        let sent_body: Value = serde_json::from_slice(&sent[0].body).expect("a JSON body");
        let digest = hex::encode(Sha256::digest(SPACE.as_bytes()));
        assert_eq!(sent_body["session_key"], format!("{PRIVATE_SESSION_PREFIX}{digest}"));

        let recorded = convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await;
        assert_eq!(recorded.len(), 1, "one pointer write");
        assert_eq!(recorded[0]["threadId"], "thread-new", "the recorded id is the one the model returned");
        assert_eq!(recorded[0]["spaceRef"], SPACE);
        assert_eq!(recorded[0]["externalOrgId"], "org-1");
    }

    /// A model 412 keeps its catalogue code and is labelled `private_panel`; no pointer is written.
    #[tokio::test]
    async fn a_model_412_keeps_its_code_and_is_labelled_private_panel() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
        answer(
            &f.model,
            "POST",
            "/v1/chat/conversation-threads",
            ResponseTemplate::new(412).set_body_json(json!({"error": {
                "code": "zdr_durable_mutation_forbidden",
                "message": "Zero Data Retention credentials cannot create, change, or delete durable state.",
                "retryable": false,
                "action": "none"
            }})),
        )
        .await;
        let (status, body) = call(f.state.clone(), "POST", ROUTE, None).await;
        assert_eq!(status, 412, "{body}");
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(body["error"]["details"]["surface"], "private_panel");
        assert!(convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await.is_empty());
    }

    /// A model outage, or a reply with no usable thread id (missing, or blank), is 503 `private_panel_unavailable`,
    /// and no pointer is recorded.
    #[tokio::test]
    async fn a_model_outage_or_a_reply_without_a_thread_id_is_503_and_records_nothing() {
        let replies = [
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal", "message": "secret"}})),
            ResponseTemplate::new(200).set_body_json(json!({})),
            ResponseTemplate::new(200).set_body_json(json!({"thread_id": "   "})),
        ];
        for reply in replies {
            let f = fixture().await;
            base(&f, false, membership_ok()).await;
            convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
            answer(&f.model, "POST", "/v1/chat/conversation-threads", reply).await;
            let (status, body) = call(f.state.clone(), "POST", ROUTE, None).await;
            assert_eq!(status, 503, "{body}");
            assert_eq!(body["error"]["code"], "private_panel_unavailable");
            assert!(!body.to_string().contains("secret"), "upstream text never crosses: {body}");
            assert!(convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await.is_empty());
        }
    }

    /// A failed pointer record is 503 `private_panel_unavailable`: one attempt, no retry, and the answer carries no
    /// pointer. The thread the model made is not handed to the browser as a pointer.
    #[tokio::test]
    async fn a_failed_record_is_503_and_the_answer_carries_no_pointer() {
        let f = fixture().await;
        base(&f, false, membership_ok()).await;
        convex(&f, "spacePrivatePanels:getSpacePrivatePanelForGateway", convex_value(Value::Null)).await;
        answer(
            &f.model,
            "POST",
            "/v1/chat/conversation-threads",
            ResponseTemplate::new(200).set_body_json(json!({"thread_id": "thread-new"})),
        )
        .await;
        convex(
            &f,
            "spacePrivatePanels:recordSpacePrivatePanelForGateway",
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        let (status, body) = call(f.state.clone(), "POST", ROUTE, None).await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "private_panel_unavailable");
        assert!(body["data"].is_null(), "no pointer in the answer: {body}");
        assert!(!body.to_string().contains("thread-new"), "{body}");
        assert_eq!(
            convex_args(&f.convex, "spacePrivatePanels:recordSpacePrivatePanelForGateway").await.len(),
            1,
            "one record attempt"
        );
    }
}
