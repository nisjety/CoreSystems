//! Human posts in a Space room: the BFF half of WS01 (build plan WS01-S5).
//!
//! - `POST /api/v1/spaces/{space_ref}/threads` creates a room thread whose first row is a human
//!   post (no model run).
//! - `POST /api/v1/spaces/{space_ref}/threads/{thread_id}/messages` appends a human post to an
//!   existing room thread (no model run).
//!
//! Body of both: `{content, client_send_id}`, typed with `deny_unknown_fields`: a forged
//! `role`, `org_id`, `space_context`, decision field or `metadata` is a 400 and is never
//! forwarded. Answer: `{data:{space_ref?,thread_id,message_id,sequence,deduplicated}}`, 201, or 200
//! when the send id was already stored (a retry of the same send).
//!
//! The gateway decides nothing here and stores nothing. Control mints the signed decision
//! (`thread-decision` for a create, `thread-append-decision` with `post_kind:"post"` for a reply,
//! always under the caller's own delegation and bound to the content digest), model-gateway maps
//! the call to Session Core, and Session Core verifies the decision and writes the row. The
//! gateway only validates the shape, refuses Zero Data Retention first, mints, forwards with the
//! caller's own bearers and maps the answer.
//!
//! One send id names the whole send: the thread session key, Control's idempotency key and the
//! stored message id are all `space-post-{client_send_id}`, so a retry lands on the same thread
//! and the same row, and the same id with other text is a 409 `idempotency_key_conflict`.
//!
//! Order (each step is a refusal before the next one runs): flag, org, Zero Data Retention (typed
//! 412 before any Control or model call; an org-core failure is a typed 503, never "your org is
//! ZDR"), body shape, Space lifecycle, bearers, Control decision, model-gateway.
//!
//! Errors are `{error:{code,message,details}}` and ZDR refusals the catalogue shape
//! (`zdr::zdr_refusal_body`): 400 `invalid_post` / `invalid_client_send_id` / `invalid_space`,
//! 403 `active_membership_required` / `space_post_not_authorized`, 404 `space_not_found` /
//! `thread_not_found` / `space_human_posts_disabled`, 409 `idempotency_key_conflict` /
//! `space_authority_stale` (a reply re-mints once) / `thread_state_conflict`, 412
//! `zdr_durable_mutation_forbidden`, 502 `post_not_stored` (the thread exists, the post did not
//! land: retry with the same send id), 503 `zdr_decision_unavailable` /
//! `space_lifecycle_unavailable` / `space_decision_unavailable` / `delegated_auth_unavailable` /
//! `session_unavailable`.

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::denial::{typed_response, OP_POST};
use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        required_session_token,
    },
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::{self, OrgZdrPosture},
};

/// The longest post, in characters. Reported to the SPA as `data.limits.post_max_chars`.
pub(crate) const POST_MAX_CHARS: usize = 8000;
const MAX_THREAD_ID_BYTES: usize = 128;
const SEND_KEY_PREFIX: &str = "space-post-";

type Reply = (StatusCode, Json<Value>);

/// The two post routes. Merged through `module_routes()` behind `require_session`; the
/// `/threads` path shares its URL with the list route (another method), so the method routers
/// merge.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/threads", post(create_post))
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/messages",
            post(reply_in_thread),
        )
}

async fn create_post(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Bytes,
) -> Reply {
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    submit_post(&state, &user, &headers, &space_ref, None, &body).await
}

async fn reply_in_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id)): Path<(String, String)>,
    body: Bytes,
) -> Reply {
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    submit_post(&state, &user, &headers, &space_ref, Some(&thread_id), &body).await
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn invalid_post(field: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_post",
            "The post is not valid.",
            json!({ "field": field, "max_chars": POST_MAX_CHARS }),
        )),
    )
}

fn thread_not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "thread_not_found",
        "The thread was not found.",
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostBody {
    content: String,
    client_send_id: String,
}

struct ValidPost {
    content: String,
    client_send_id: String,
}

/// `^[A-Za-z0-9_-]{16,64}$`.
fn valid_send_id(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_post(body: &Value) -> Result<ValidPost, Reply> {
    let parsed: PostBody = serde_json::from_value(body.clone()).map_err(|_| invalid_post("body"))?;
    if !valid_send_id(&parsed.client_send_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_client_send_id",
                "client_send_id must be 16 to 64 letters, digits, '_' or '-'.",
            )),
        ));
    }
    // Trimmed here, once: the Control decision commits to the digest of exactly these bytes and
    // the same bytes are what model-gateway forwards to Session Core.
    let content = parsed.content.trim();
    if content.is_empty() || content.contains('\0') || content.chars().count() > POST_MAX_CHARS {
        return Err(invalid_post("content"));
    }
    Ok(ValidPost {
        content: content.to_owned(),
        client_send_id: parsed.client_send_id,
    })
}

/// A durable write of free text is refused for a Zero Data Retention caller, typed, before any
/// Control or model call. `Some` is the response to send.
async fn zdr_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &Value,
) -> Option<Reply> {
    let forbidden = || {
        (
            StatusCode::PRECONDITION_FAILED,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)),
        )
    };
    if zdr::request_zdr(headers, Some(body)) {
        return Some(forbidden());
    }
    match zdr::org_zdr_lookup(state, user).await {
        OrgZdrPosture::Enabled => Some(forbidden()),
        // An org-core failure is an outage, not a posture: say so, typed, and let the caller retry.
        OrgZdrPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
        )),
        OrgZdrPosture::Disabled => None,
    }
}

/// The whole post flow, shared by the two routes and the two actions
/// (`spaces.create_post`, `spaces.reply_in_thread`). `body` is the untrusted
/// `{content, client_send_id}` object; `thread_id` is `None` for a new thread.
pub(crate) async fn submit_post(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    thread_id: Option<&str>,
    body: &Value,
) -> Reply {
    if !state.space_features.human_posts_api {
        return refuse(
            StatusCode::NOT_FOUND,
            "space_human_posts_disabled",
            "Human posts are not enabled.",
        );
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to post in a Space.",
        );
    }
    let space_ref = space_ref.trim();
    if space_ref.is_empty() {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let thread_id = match thread_id.map(str::trim) {
        Some(id) if id.is_empty() || id.len() > MAX_THREAD_ID_BYTES => return thread_not_found(),
        other => other,
    };
    if let Some(refusal) = zdr_refusal(state, user, headers, body).await {
        return refusal;
    }
    let post = match parse_post(body) {
        Ok(post) => post,
        Err(reply) => return reply,
    };
    match super::space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
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
    let model_bearer = model_token(state, user, headers).await;
    let session_bearer = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return delegated_auth_unavailable(cause),
    };
    let send_key = format!("{SEND_KEY_PREFIX}{}", post.client_send_id);

    let Some(thread_id) = thread_id else {
        let context = match mint_create_decision(state, user, &org_id, space_ref, &send_key).await {
            Ok(context) => context,
            Err(reply) => return reply,
        };
        let request = json!({
            "space_context": context,
            "session_key": send_key,
            "first_message": {
                "content": post.content,
                "post_kind": "post",
                "client_message_id": send_key,
            },
        });
        let url = format!("{}/v1/threads", state.model_gateway_url);
        let (status, Json(response)) = proxy_model_json_with_session(
            state,
            Method::POST,
            &url,
            Some(request),
            model_bearer.as_deref(),
            Some(&session_bearer),
            user,
        )
        .await;
        return map_model_response(status, response, Some(space_ref));
    };

    let url = format!(
        "{}/v1/threads/{}/messages",
        state.model_gateway_url,
        urlencoding::encode(thread_id)
    );
    // A decision is bound to the audience revision it saw. When the audience moved, Session Core
    // answers 409 `space_authority_stale`: mint a fresh decision once and resend, then surface it.
    for attempt in 0..2 {
        let context =
            match mint_append_decision(state, user, &org_id, space_ref, thread_id, &post, &send_key)
                .await
            {
                Ok(context) => context,
                Err(reply) => return reply,
            };
        let request = json!({
            "content": post.content,
            "post_kind": "post",
            "client_message_id": send_key,
            "space_append_context": context,
        });
        let (status, Json(response)) = proxy_model_json_with_session(
            state,
            Method::POST,
            &url,
            Some(request),
            model_bearer.as_deref(),
            Some(&session_bearer),
            user,
        )
        .await;
        let stale = status == StatusCode::CONFLICT
            && response.pointer("/error/code").and_then(Value::as_str)
                == Some("space_authority_stale");
        if stale && attempt == 0 {
            continue;
        }
        return map_model_response(status, response, None);
    }
    refuse(
        StatusCode::CONFLICT,
        "space_authority_stale",
        "The Space authority changed; try again.",
    )
}

/// model-gateway's answer as the browser envelope. Its own errors are already
/// `{error:{code,message,details}}` (or the typed ZDR catalogue shape) with fixed words, so they
/// pass through with their status; anything else is `session_unavailable`.
fn map_model_response(status: StatusCode, response: Value, space_ref: Option<&str>) -> Reply {
    if !status.is_success() {
        if response
            .pointer("/error/code")
            .and_then(Value::as_str)
            .is_some()
        {
            return (status, Json(response));
        }
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "The conversation service is unavailable.",
        );
    }
    let text = |field: &str| {
        response
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let (Some(thread_id), Some(message_id), Some(sequence)) = (
        text("thread_id"),
        text("message_id"),
        response.get("sequence").and_then(Value::as_u64),
    ) else {
        return refuse(
            StatusCode::BAD_GATEWAY,
            "session_unavailable",
            "The conversation service returned an unreadable answer.",
        );
    };
    let deduplicated = response
        .get("deduplicated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut data = Map::new();
    if let Some(space_ref) = space_ref {
        data.insert("space_ref".to_owned(), json!(space_ref));
    }
    data.insert("thread_id".to_owned(), json!(thread_id));
    data.insert("message_id".to_owned(), json!(message_id));
    data.insert("sequence".to_owned(), json!(sequence));
    data.insert("deduplicated".to_owned(), json!(deduplicated));
    let status = if deduplicated {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (status, Json(json!({ "data": Value::Object(data) })))
}

async fn mint_create_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    send_key: &str,
) -> Result<Value, Reply> {
    // No retrieval decision: a post reads nothing from Data Plane.
    mint_decision(
        state,
        user,
        org_id,
        "thread-decision",
        json!({
            "space_ref": space_ref,
            "session_key": send_key,
            "idempotency_key": send_key,
        }),
    )
    .await
}

async fn mint_append_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
    post: &ValidPost,
    send_key: &str,
) -> Result<Value, Reply> {
    let content_digest = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(post.content.as_bytes()))
    );
    mint_decision(
        state,
        user,
        org_id,
        "thread-append-decision",
        json!({
            "space_ref": space_ref,
            "thread_id": thread_id,
            "content_digest": content_digest,
            "idempotency_key": send_key,
            "post_kind": "post",
        }),
    )
    .await
}

/// Calls Control's decision endpoint under the caller's delegation and returns the 12-field
/// context model-gateway forwards. Nothing about the decision is decided or defaulted here.
async fn mint_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    path: &str,
    request: Value,
) -> Result<Value, Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/internal/spaces/{path}", state.user_core_url);
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(request),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err(control_refusal(state, status, response));
    }
    let data = unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(super::invalid_decision)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if token.trim().is_empty() {
        return Err(super::invalid_decision());
    }
    space_context_from_decision(decision, token)
}

/// Control's refusal as the post envelope. Control gives no reason a browser may rely on, so a 403
/// is always `space_post_not_authorized` (with the typed `details.reason` when Control sent one
/// and the denial flag is on).
fn control_refusal(state: &AppState, status: StatusCode, body: Value) -> Reply {
    match status {
        StatusCode::FORBIDDEN => {
            let (status, Json(mapped)) = typed_response(state, status, body, OP_POST);
            if mapped.pointer("/error/code").and_then(Value::as_str) == Some(OP_POST) {
                (status, Json(mapped))
            } else {
                refuse(
                    StatusCode::FORBIDDEN,
                    OP_POST,
                    "Posting to this Space is not permitted.",
                )
            }
        }
        StatusCode::NOT_FOUND => refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        ),
        StatusCode::BAD_REQUEST => invalid_post("request"),
        StatusCode::CONFLICT => refuse(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "The Space authority changed; try again.",
        ),
        _ => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_decision_unavailable",
            "The Space decision service is unavailable.",
        ),
    }
}

/// The 12-field decision envelope model-gateway and Session Core verify, rebuilt solely from
/// Control's signed response (no retrieval token: a post reads nothing). Same shape as the
/// scoped-chat contexts in `inject_personal_thread_context`.
pub(crate) fn space_context_from_decision(
    decision: &Map<String, Value>,
    token: &str,
) -> Result<Value, Reply> {
    use super::{invalid_decision, required_decision_string};
    Ok(json!({
        "space_id": required_decision_string(decision, "space_ref")?,
        "space_decision_ref": required_decision_string(decision, "decision_ref")?,
        "recipient_audience_ref": required_decision_string(decision, "recipient_audience_ref")?,
        "recipient_audience_revision": decision
            .get("recipient_audience_revision")
            .and_then(Value::as_u64)
            .ok_or_else(invalid_decision)?,
        "recipient_audience_hash": required_decision_string(decision, "recipient_audience_hash")?,
        "privacy_policy_ref": required_decision_string(decision, "privacy_policy_ref")?,
        "authority_revision": decision
            .get("authority_revision")
            .and_then(Value::as_u64)
            .ok_or_else(invalid_decision)?,
        "resource_authorization_ref": required_decision_string(decision, "resource_authorization_ref")?,
        "space_decision_token": token,
        "action_schema_hash": required_decision_string(decision, "action_schema_hash")?,
        "payload_digest": required_decision_string(decision, "payload_digest")?,
        "idempotency_key": required_decision_string(decision, "idempotency_key")?,
    }))
}

/// Where a thread lives, as far as THIS caller can see it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "consumed by Model Plane WS18 (chat projects), not by a Space route")]
pub(crate) enum SpaceThreadScope {
    /// The thread belongs to this Space (`space_ref`).
    Space(String),
    /// The caller owns the thread and it has no Space.
    NoSpace,
    /// The caller cannot see the thread: it does not exist, or it is another member's post that
    /// Session Core does not list to this caller.
    NotVisible,
}

/// Which Space (if any) a thread belongs to. For a BFF route that takes only a `thread_id` and
/// must refuse or re-scope a Space thread (Model Plane WS18). Reads Session Core through
/// model-gateway's owner-bound by-id listing with the caller's own bearers: it widens nothing, so a
/// room post owned by someone else is `NotVisible` here, and a ZDR caller is no different because
/// the read stores nothing. The answer is a routing fact, never an authorization.
///
/// UNVERIFIED shape: the plan names the helper but not its signature (depth file).
#[allow(dead_code, reason = "consumed by Model Plane WS18 (chat projects), not by a Space route")]
pub(crate) async fn space_scope_for_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
) -> Result<SpaceThreadScope, Reply> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() || thread_id.len() > MAX_THREAD_ID_BYTES {
        return Ok(SpaceThreadScope::NotVisible);
    }
    let model_bearer = model_token(state, user, headers).await;
    let session_bearer = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return Err(delegated_auth_unavailable(cause)),
    };
    let url = format!(
        "{}/v1/threads?limit=1&archived=all&thread_id={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id)
    );
    let (status, Json(response)) = proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        model_bearer.as_deref(),
        Some(&session_bearer),
        user,
    )
    .await;
    if !status.is_success() {
        return Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "The conversation service is unavailable.",
        ));
    }
    let found = response
        .get("threads")
        .and_then(Value::as_array)
        .and_then(|threads| {
            threads
                .iter()
                .find(|thread| thread.get("thread_id").and_then(Value::as_str) == Some(thread_id))
        });
    Ok(match found {
        None => SpaceThreadScope::NotVisible,
        Some(thread) => match thread
            .get("space_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|space| !space.is_empty())
        {
            Some(space) => SpaceThreadScope::Space(space.to_owned()),
            None => SpaceThreadScope::NoSpace,
        },
    })
}

#[cfg(test)]
mod tests {
    //! WS01-S5 tests: the real router with wiremock for auth-core, user-core (Control), org-core,
    //! Application (Convex) and model-gateway. No model is called: model-gateway is a mock.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SEND_ID: &str = "send_id_0123456789ab";
    const TOKEN_SECRET: &str = "decision-token-secret-xyzxyz";

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

    /// `org_core`: org-core's answer to the ZDR posture read; `application`: the Convex answer to
    /// the Space lifecycle lookup. Flags: human posts on, everything else off.
    async fn fixture(org_core: ResponseTemplate, application: ResponseTemplate) -> Fx {
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
            .respond_with(application)
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
        state.space_features.human_posts_api = true;
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
        fixture(org_posture(false), active_space()).await
    }

    fn decision_body(path_name: &str) -> Value {
        json!({"data": {
            "decision": {
                "space_ref": "space-room",
                "decision_ref": "decision-1",
                "recipient_audience_ref": "audience-1",
                "recipient_audience_revision": 3,
                "recipient_audience_hash": "hash-1",
                "privacy_policy_ref": "privacy-1",
                "authority_revision": 7,
                "resource_authorization_ref": "resauth-1",
                "action_schema_hash": format!("schema-{path_name}"),
                "payload_digest": "sha256:payload",
                "idempotency_key": format!("space-post-{SEND_ID}"),
            },
            "token": TOKEN_SECRET,
        }})
    }

    async fn mount_decision(fx: &Fx, name: &str) {
        Mock::given(wm_method("POST"))
            .and(wm_path(format!("/api/v1/internal/spaces/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(decision_body(name)))
            .mount(&fx.user_core)
            .await;
    }

    async fn mount_decision_status(fx: &Fx, name: &str, response: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(format!("/api/v1/internal/spaces/{name}")))
            .respond_with(response)
            .mount(&fx.user_core)
            .await;
    }

    fn stored(deduplicated: bool) -> ResponseTemplate {
        ResponseTemplate::new(if deduplicated { 200 } else { 201 }).set_body_json(json!({
            "thread_id": "thread-1",
            "message_id": format!("space-post-{SEND_ID}"),
            "sequence": 1,
            "deduplicated": deduplicated,
        }))
    }

    async fn mount_model(fx: &Fx, route: &str, response: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .respond_with(response)
            .mount(&fx.model_gateway)
            .await;
    }

    async fn send(
        state: &AppState,
        uri: &str,
        extra_header: Option<(&str, &str)>,
        body: Value,
    ) -> (u16, Value, String) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1")
            .header("content-type", "application/json");
        if let Some((name, value)) = extra_header {
            builder = builder.header(name, value);
        }
        let request = builder.body(Body::from(body.to_string())).unwrap();
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

    fn create_uri() -> &'static str {
        "/api/v1/spaces/space-room/threads"
    }

    fn reply_uri() -> &'static str {
        "/api/v1/spaces/space-room/threads/thread-1/messages"
    }

    fn post(content: &str) -> Value {
        json!({"content": content, "client_send_id": SEND_ID})
    }

    /// Control (user-core) requests to the Space decision namespace: (path, JSON body).
    async fn control_calls(fx: &Fx) -> Vec<(String, Value)> {
        fx.user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path().starts_with("/api/v1/internal/spaces/"))
            .map(|r| {
                (
                    r.url.path().to_owned(),
                    serde_json::from_slice(&r.body).unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    async fn model_calls(fx: &Fx) -> Vec<(String, Value)> {
        fx.model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                (
                    r.url.path().to_owned(),
                    serde_json::from_slice(&r.body).unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    fn assert_no_secret(raw: &str) {
        assert!(!raw.contains(TOKEN_SECRET), "decision token leaked: {raw}");
        assert!(!raw.contains("decision-1"), "decision ref leaked: {raw}");
        assert!(!raw.contains("space_decision"), "decision field leaked: {raw}");
        assert!(!raw.contains("plane-token"), "bearer leaked: {raw}");
    }

    #[tokio::test]
    async fn create_post_mints_create_decision_without_retrieval_and_sends_keys() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        let (status, body, raw) = send(&fx.state, create_uri(), None, post("  hello room  ")).await;
        assert_eq!(status, 201, "{raw}");
        assert_eq!(
            body["data"],
            json!({
                "space_ref": "space-room",
                "thread_id": "thread-1",
                "message_id": format!("space-post-{SEND_ID}"),
                "sequence": 1,
                "deduplicated": false,
            })
        );
        assert_no_secret(&raw);

        let control = control_calls(&fx).await;
        assert_eq!(control.len(), 1, "one decision and no retrieval decision: {control:?}");
        assert_eq!(control[0].0, "/api/v1/internal/spaces/thread-decision");
        assert_eq!(control[0].1["space_ref"], "space-room");
        assert_eq!(control[0].1["session_key"], format!("space-post-{SEND_ID}"));
        assert_eq!(control[0].1["idempotency_key"], format!("space-post-{SEND_ID}"));
        assert!(control.iter().all(|(path, _)| !path.contains("retrieval")));

        let model = model_calls(&fx).await;
        assert_eq!(model.len(), 1);
        assert_eq!(model[0].0, "/v1/threads");
        let sent = &model[0].1;
        assert_eq!(sent["session_key"], format!("space-post-{SEND_ID}"));
        assert_eq!(sent["space_context"]["space_id"], "space-room");
        assert_eq!(sent["space_context"]["space_decision_token"], TOKEN_SECRET);
        assert_eq!(sent["first_message"]["post_kind"], "post");
        assert_eq!(
            sent["first_message"]["client_message_id"],
            format!("space-post-{SEND_ID}")
        );
        assert_eq!(sent["first_message"]["content"], "hello room");
    }

    #[tokio::test]
    async fn a_deduplicated_send_answers_200() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(true)).await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["deduplicated"], true);
        assert_eq!(body["data"]["thread_id"], "thread-1");
    }

    #[tokio::test]
    async fn reply_mints_v2_decision_bound_to_content_digest() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored(false)).await;
        let (status, body, raw) = send(&fx.state, reply_uri(), None, post("  reply text \n")).await;
        assert_eq!(status, 201, "{raw}");
        assert!(body["data"].get("space_ref").is_none(), "a reply names no space_ref");
        assert_eq!(body["data"]["thread_id"], "thread-1");
        assert_no_secret(&raw);

        let digest = format!("sha256:{}", hex::encode(Sha256::digest(b"reply text")));
        let control = control_calls(&fx).await;
        assert_eq!(control.len(), 1);
        assert_eq!(control[0].0, "/api/v1/internal/spaces/thread-append-decision");
        assert_eq!(control[0].1["post_kind"], "post");
        assert_eq!(control[0].1["thread_id"], "thread-1");
        assert_eq!(control[0].1["content_digest"], json!(digest));
        assert_eq!(control[0].1["idempotency_key"], format!("space-post-{SEND_ID}"));

        let model = model_calls(&fx).await;
        assert_eq!(model.len(), 1);
        assert_eq!(model[0].1["content"], "reply text", "the digested bytes are the sent bytes");
        assert_eq!(
            hex::encode(Sha256::digest(model[0].1["content"].as_str().unwrap().as_bytes())),
            digest.trim_start_matches("sha256:")
        );
        assert_eq!(model[0].1["post_kind"], "post");
        assert_eq!(model[0].1["client_message_id"], format!("space-post-{SEND_ID}"));
        assert_eq!(model[0].1["space_append_context"]["space_decision_token"], TOKEN_SECRET);
    }

    #[tokio::test]
    async fn forged_authority_fields_are_rejected_not_forwarded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored(false)).await;
        let forged = [
            ("role", json!("owner")),
            ("org_id", json!("org-2")),
            ("space_context", json!({"space_id": "space-other"})),
            ("space_append_context", json!({"space_id": "space-other"})),
            ("metadata", json!({"k": "v"})),
            ("space_decision_token", json!("forged")),
            ("decision_ref", json!("forged")),
            ("content_digest", json!("sha256:forged")),
        ];
        for uri in [create_uri(), reply_uri()] {
            for (field, value) in &forged {
                let mut body = post("hello");
                body[*field] = value.clone();
                let (status, parsed, _) = send(&fx.state, uri, None, body).await;
                assert_eq!(status, 400, "{uri} {field}");
                assert_eq!(parsed["error"]["code"], "invalid_post", "{uri} {field}");
            }
        }
        assert!(control_calls(&fx).await.is_empty(), "no Control call");
        assert!(model_calls(&fx).await.is_empty(), "no model-gateway call");
    }

    #[tokio::test]
    async fn zdr_org_gets_typed_412_before_any_control_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // Org ZDR on.
        let fx = fixture(org_posture(true), active_space()).await;
        mount_decision(&fx, "thread-decision").await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(control_calls(&fx).await.is_empty());
        assert!(model_calls(&fx).await.is_empty());
        let user_core_all = fx.user_core.received_requests().await.unwrap_or_default();
        assert!(
            user_core_all
                .iter()
                .all(|r| !r.url.path().contains("decision")),
            "zero Control decision requests"
        );

        // x-zdr header on a non-ZDR org.
        let fx = default_fixture().await;
        let (status, body, _) = send(&fx.state, reply_uri(), Some(("x-zdr", "true")), post("hello")).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(control_calls(&fx).await.is_empty());

        // zdr:true in the body: refused before the unknown-field check.
        let mut with_zdr = post("hello");
        with_zdr["zdr"] = json!(true);
        let (status, body, _) = send(&fx.state, create_uri(), None, with_zdr).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(control_calls(&fx).await.is_empty());
        assert!(model_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn zdr_refusal_body_matches_spa_golden() {
        let golden: Value = serde_json::from_str(include_str!(
            "../../../../../src/shared/api/contracts/zdr-refusals.json"
        ))
        .expect("golden parses");
        let codes = golden["codes"].as_array().expect("codes");
        for code in [zdr::ZDR_DURABLE_MUTATION_FORBIDDEN, zdr::ZDR_DECISION_UNAVAILABLE] {
            let entry = codes
                .iter()
                .find(|entry| entry["code"] == code)
                .unwrap_or_else(|| panic!("{code} is in the golden"));
            let body = zdr::zdr_refusal_body(code);
            assert_eq!(body["error"]["code"], code);
            assert_eq!(body["error"]["message"], entry["message_en"], "{code}");
            assert_eq!(body["error"]["retryable"], entry["retryable"], "{code}");
            assert_eq!(body["error"]["action"], entry["action"], "{code}");
        }
    }

    #[tokio::test]
    async fn org_core_failure_is_typed_503_not_zdr() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(500), active_space()).await;
        mount_decision(&fx, "thread-decision").await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(body["error"]["retryable"], true);
        assert!(control_calls(&fx).await.is_empty());
        assert!(model_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn flag_off_is_404_before_org_zdr_and_body() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(org_posture(true), active_space()).await;
        fx.state.space_features.human_posts_api = false;
        // Org ZDR on and a malformed body: the flag answers first.
        let (status, body, _) = send(&fx.state, create_uri(), Some(("x-zdr", "true")), json!({"role": "owner"})).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_human_posts_disabled");
        assert!(control_calls(&fx).await.is_empty());
        assert!(model_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn stale_authority_is_retried_once() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // 409 then 201: two mints, success.
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-append-decision").await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/threads/thread-1/messages"))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": {"code": "space_authority_stale", "message": "stale", "details": {}}
            })))
            .up_to_n_times(1)
            .mount(&fx.model_gateway)
            .await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored(false)).await;
        let (status, body, _) = send(&fx.state, reply_uri(), None, post("hello")).await;
        assert_eq!(status, 201);
        assert_eq!(body["data"]["thread_id"], "thread-1");
        assert_eq!(control_calls(&fx).await.len(), 2, "re-minted once");
        assert_eq!(model_calls(&fx).await.len(), 2);

        // 409 twice: the stale error surfaces after exactly two attempts.
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(
            &fx,
            "/v1/threads/thread-1/messages",
            ResponseTemplate::new(409).set_body_json(json!({
                "error": {"code": "space_authority_stale", "message": "stale", "details": {}}
            })),
        )
        .await;
        let (status, body, _) = send(&fx.state, reply_uri(), None, post("hello")).await;
        assert_eq!(status, 409);
        assert_eq!(body["error"]["code"], "space_authority_stale");
        assert_eq!(control_calls(&fx).await.len(), 2);
        assert_eq!(model_calls(&fx).await.len(), 2);
    }

    #[tokio::test]
    async fn control_403_maps_to_space_post_not_authorized() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let denial = json!({
            "error": "Your role cannot post here.",
            "code": "space_role_insufficient",
            "resolvable_by": "space_manager",
            "details": {"required_roles": ["editor"], "secret": "dropped"},
        });
        // Flag off: fixed code and message, no typed reason.
        let fx = default_fixture().await;
        mount_decision_status(&fx, "thread-decision", ResponseTemplate::new(403).set_body_json(denial.clone())).await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_post_not_authorized");
        assert!(body["error"].get("details").is_none());
        assert!(model_calls(&fx).await.is_empty());

        // Flag on and Control sent a reason: typed details.reason, allowlisted details only.
        let mut fx = default_fixture().await;
        fx.state.space_features.denials_typed = true;
        mount_decision_status(&fx, "thread-decision", ResponseTemplate::new(403).set_body_json(denial)).await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_post_not_authorized");
        assert_eq!(body["error"]["details"]["reason"], "space_role_insufficient");
        assert_eq!(body["error"]["details"]["resolvable_by"], "space_manager");
        assert!(body["error"]["details"].get("secret").is_none());

        // Flag on but Control sent no reason: the fixed refusal, never a bare Control body.
        let mut fx = default_fixture().await;
        fx.state.space_features.denials_typed = true;
        mount_decision_status(
            &fx,
            "thread-decision",
            ResponseTemplate::new(403).set_body_json(json!({"error": "forbidden"})),
        )
        .await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_post_not_authorized");
        assert!(body["error"].get("details").is_none());
    }

    #[tokio::test]
    async fn other_control_statuses_map_to_typed_errors() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (control, expected_status, expected_code) in [
            (404_u16, 404_u16, "space_not_found"),
            (400, 400, "invalid_post"),
            (409, 409, "space_authority_stale"),
            (500, 503, "space_decision_unavailable"),
        ] {
            let fx = default_fixture().await;
            mount_decision_status(
                &fx,
                "thread-decision",
                ResponseTemplate::new(control).set_body_json(json!({"error": "nope"})),
            )
            .await;
            let (status, body, raw) = send(&fx.state, create_uri(), None, post("hello")).await;
            assert_eq!(status, expected_status, "control {control}: {raw}");
            assert_eq!(body["error"]["code"], expected_code, "control {control}");
            assert!(model_calls(&fx).await.is_empty(), "control {control}");
        }
    }

    #[tokio::test]
    async fn validation_refuses_bad_content_and_send_ids() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        for content in ["", "   \n\t ", "a\u{0}b"] {
            let (status, body, _) = send(&fx.state, create_uri(), None, post(content)).await;
            assert_eq!(status, 400, "{content:?}");
            assert_eq!(body["error"]["code"], "invalid_post");
        }
        let too_long = "x".repeat(8001);
        let (status, body, _) = send(&fx.state, create_uri(), None, post(&too_long)).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_post");
        assert_eq!(body["error"]["details"]["max_chars"], 8000);
        // 8000 characters is the limit, inclusive.
        let (status, _, _) = send(&fx.state, create_uri(), None, post(&"x".repeat(8000))).await;
        assert_eq!(status, 201);

        for id in ["short", "has space 0123456789", "bad!char0123456789a", &"a".repeat(65)] {
            let (status, body, _) = send(
                &fx.state,
                create_uri(),
                None,
                json!({"content": "hello", "client_send_id": id}),
            )
            .await;
            assert_eq!(status, 400, "{id}");
            assert_eq!(body["error"]["code"], "invalid_client_send_id", "{id}");
        }
        // Missing send id is a body-shape refusal.
        let (status, body, _) = send(&fx.state, create_uri(), None, json!({"content": "hello"})).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_post");
    }

    #[tokio::test]
    async fn blank_thread_id_is_404_thread_not_found() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        let (status, body, _) = send(
            &fx.state,
            "/api/v1/spaces/space-room/threads/%20/messages",
            None,
            post("hello"),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "thread_not_found");
        assert!(control_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn space_lifecycle_none_is_404_and_error_is_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(org_posture(false), ResponseTemplate::new(200).set_body_json(json!({"value": []}))).await;
        mount_decision(&fx, "thread-decision").await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_not_found");
        assert!(control_calls(&fx).await.is_empty());

        let fx = fixture(org_posture(false), ResponseTemplate::new(503)).await;
        mount_decision(&fx, "thread-decision").await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_lifecycle_unavailable");
        assert!(control_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn model_gateway_errors_pass_through_with_status() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases: Vec<(u16, Value, u16, &str)> = vec![
            (
                502,
                json!({"error": {"code": "post_not_stored", "message": "m", "details": {"thread_id": "thread-9"}}}),
                502,
                "post_not_stored",
            ),
            (
                409,
                json!({"error": {"code": "idempotency_key_conflict", "message": "m", "details": {}}}),
                409,
                "idempotency_key_conflict",
            ),
            (
                404,
                json!({"error": {"code": "thread_not_found", "message": "m", "details": {}}}),
                404,
                "thread_not_found",
            ),
            (
                412,
                zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN),
                412,
                "zdr_durable_mutation_forbidden",
            ),
        ];
        for (upstream, upstream_body, expected, code) in cases {
            let fx = default_fixture().await;
            mount_decision(&fx, "thread-decision").await;
            mount_model(&fx, "/v1/threads", ResponseTemplate::new(upstream).set_body_json(upstream_body.clone())).await;
            let (status, body, raw) = send(&fx.state, create_uri(), None, post("hello")).await;
            assert_eq!(status, expected, "{raw}");
            assert_eq!(body["error"]["code"], code);
            assert_eq!(body, upstream_body);
            assert_no_secret(&raw);
        }
        // A non-JSON upstream failure is a typed 503.
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", ResponseTemplate::new(500).set_body_string("boom")).await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "session_unavailable");
    }

    #[tokio::test]
    async fn an_unreadable_success_body_is_a_typed_502() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", ResponseTemplate::new(201).set_body_json(json!({"thread_id": "thread-1"}))).await;
        let (status, body, _) = send(&fx.state, create_uri(), None, post("hello")).await;
        assert_eq!(status, 502);
        assert_eq!(body["error"]["code"], "session_unavailable");
    }

    #[test]
    fn send_id_shape_is_16_to_64_url_safe_characters() {
        assert!(valid_send_id(&"a".repeat(16)));
        assert!(valid_send_id(&"a-_Z9".repeat(12)));
        assert!(!valid_send_id(&"a".repeat(15)));
        assert!(!valid_send_id(&"a".repeat(65)));
        assert!(!valid_send_id("aaaaaaaaaaaaaaa."));
        assert!(!valid_send_id("aaaaaaaaaaaaaaaé"));
    }
}
