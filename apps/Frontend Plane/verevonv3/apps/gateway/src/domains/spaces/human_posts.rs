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
//! Order (each step is a refusal before the next one runs): flag, org, per-user post ceiling
//! (typed 429 `rate_limited` with `details.retry_after_seconds`, before anything else is called),
//! Zero Data Retention (typed 412 before any Control or model call; an org-core failure is a
//! typed 503, never "your org is ZDR"), body shape, Space lifecycle, bearers, Control decision,
//! model-gateway.
//!
//! Errors are `{error:{code,message,details}}` and ZDR refusals the catalogue shape
//! (`zdr::zdr_refusal_body`): 400 `invalid_post` / `invalid_client_send_id` / `invalid_space`,
//! 403 `active_membership_required` / `space_post_not_authorized`, 404 `space_not_found` /
//! `thread_not_found` / `space_human_posts_disabled`, 409 `idempotency_key_conflict` /
//! `space_authority_stale` (a reply re-mints once) / `room_archived` / `thread_state_conflict`, 412
//! `zdr_durable_mutation_forbidden`, 429 `rate_limited`, 502 `post_not_stored` (the thread exists,
//! the post did not land: retry with the same send id), 503 `zdr_decision_unavailable` /
//! `space_lifecycle_unavailable` / `space_decision_unavailable` / `delegated_auth_unavailable` /
//! `session_unavailable`.
//!
//! A model-gateway error is not relayed as it came: it keeps its status and its code and is
//! rebuilt with a message of at most 300 characters and only the scalar entries of `details`
//! (at most 16). The codes model-gateway answers are `invalid_post`, `invalid_client_message_id`,
//! `space_post_not_authorized`, `thread_not_found`, `idempotency_key_conflict`,
//! `space_authority_stale`, `thread_state_conflict`, `post_not_stored`, `session_unavailable` and
//! the ZDR pair (rebuilt from the catalogue); a body without a well-formed code becomes the
//! generic `session_unavailable` 503, and so does its 401 `unauthorized` (the gateway already
//! verified the browser session, and the SPA reads a 401 `unauthorized` as a signed-out user).
//! `room_archived` comes from Control, not from here.

use std::sync::OnceLock;

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
use super::mentions;
use super::room_admin::{is_error_code, MAX_RELAYED_DETAILS, MAX_RELAYED_MESSAGE_CHARS};
use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        required_session_token,
    },
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::{authorized_org_id, proxy_json},
    zdr,
};

/// The longest post, in characters. Reported to the SPA as `data.limits.post_max_chars`.
pub(crate) const POST_MAX_CHARS: usize = 8000;
const MAX_THREAD_ID_BYTES: usize = 128;
const SEND_KEY_PREFIX: &str = "space-post-";
/// Human posts per user per organization per minute, creates and replies together. A test run
/// shares one process-wide limiter and one user, so the ceiling is lifted there; the limiter
/// itself is tested directly with its own small bucket.
const POSTS_PER_MINUTE: f64 = if cfg!(test) { 100_000.0 } else { 60.0 };

/// Process-wide limiter, built on first use from the gateway's Dragonfly connection (in-process
/// fallback when the cache is off). Not an `AppState` field, like `room_admin`'s member limiter,
/// so no test state literal changes.
static POST_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

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

/// One post token for the caller, or the typed 429. The Retry-After seconds travel in
/// `details.retry_after_seconds` because the flow answers a bare `(status, json)`.
async fn check_post_ceiling(limiter: &RateLimiter, org_id: &str, user_id: &str) -> Result<(), Reply> {
    match limiter
        .try_acquire(&format!("space-post:{}:{user_id}", org_id.trim()))
        .await
    {
        Ok(()) => Ok(()),
        Err(retry_after) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(error_with_details(
                "rate_limited",
                "Too many posts. Retry shortly.",
                json!({ "retry_after_seconds": retry_after.max(1) }),
            )),
        )),
    }
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
    /// WS08-S4: `{people:[...], agents:[...]}`. Shape and meaning are checked by `plan_mentions`;
    /// with `SPACE_MENTION_RECORDS_API` off it is ignored and the post stands.
    #[serde(default)]
    mentions: Option<Value>,
}

struct ValidPost {
    content: String,
    client_send_id: String,
    mentions: Option<Value>,
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
        mentions: parsed.mentions.filter(|mentions| !mentions.is_null()),
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
    zdr::durable_write_refusal(state, user, Some(headers), Some(body)).await
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
    let limiter = POST_LIMITER
        .get_or_init(|| RateLimiter::from_cache_with_rpm(&state.cache, POSTS_PER_MINUTE));
    submit_post_limited(state, limiter, user, headers, space_ref, thread_id, body).await
}

/// `submit_post` with the post ceiling passed in, so a test can use a small bucket of its own.
async fn submit_post_limited(
    state: &AppState,
    limiter: &RateLimiter,
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
    if let Err(reply) = check_post_ceiling(limiter, &org_id, &user.user_id).await {
        return reply;
    }
    if let Some(refusal) = zdr_refusal(state, user, headers, body).await {
        return refusal;
    }
    let post = match parse_post(body) {
        Ok(post) => post,
        Err(reply) => return reply,
    };
    // WS12-S3: a post accepted in a direct message clears every viewer's hide.
    let dm_space: bool;
    match super::space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(space)) => {
            dm_space = state.space_features.dm_api
                && super::dm::is_dm_kind(
                    space.get("kind").and_then(Value::as_str).unwrap_or_default(),
                );
        }
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
    let planned = match plan_post_mentions(state, user, headers, &org_id, space_ref, thread_id, &post).await
    {
        Ok(planned) => planned,
        Err(reply) => return reply,
    };

    let Some(thread_id) = thread_id else {
        let context = match mint_create_decision(state, user, &org_id, space_ref, &send_key).await {
            Ok(context) => context,
            Err(reply) => return reply,
        };
        let mut request = json!({
            "space_context": context,
            "session_key": send_key,
            "first_message": {
                "content": post.content,
                "post_kind": "post",
                "client_message_id": send_key,
            },
        });
        if let Some(planned) = planned.as_ref().filter(|planned| !planned.entries.is_empty()) {
            request["first_message"]["mentions"] = Value::Array(planned.entries.clone());
        }
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
        let recorded = mentions::recorded_from_receipt(&response);
        let reply = attach_mentions(
            map_model_response(status, response, Some(space_ref)),
            planned.as_ref(),
            recorded,
        );
        if dm_space && reply.0.is_success() {
            super::dm::resurface(state, user, &org_id, space_ref).await;
        }
        return reply;
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
        let mut request = json!({
            "content": post.content,
            "post_kind": "post",
            "client_message_id": send_key,
            "space_append_context": context,
        });
        if let Some(planned) = planned.as_ref().filter(|planned| !planned.entries.is_empty()) {
            request["mentions"] = Value::Array(planned.entries.clone());
        }
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
        let recorded = mentions::recorded_from_receipt(&response);
        let reply = attach_mentions(
            map_model_response(status, response, None),
            planned.as_ref(),
            recorded,
        );
        if dm_space && reply.0.is_success() {
            super::dm::resurface(state, user, &org_id, space_ref).await;
        }
        return reply;
    }
    refuse(
        StatusCode::CONFLICT,
        "space_authority_stale",
        "The Space authority changed; try again.",
    )
}

/// The mention plan of one post, ready to forward and to report (WS08-S4).
struct PlannedMentions {
    /// The `mentions` entries forwarded to model-gateway (Session Core's `MentionInput`).
    entries: Vec<Value>,
    /// `data.mentions.rejected`.
    rejected: Vec<Value>,
}

/// Plans the post's `mentions` (WS08-S4). `Ok(None)` when the post carries none or the flag is
/// off. A malformed set is the one hard refusal (400 `invalid_mentions`); every other problem is a
/// soft per-ref `rejected` entry, because a mention never fails a post. When the roster or the
/// poster's role cannot be read the post still goes, with no mentions, and each requested ref is
/// reported `mention_unavailable` (DEVIATION: the code is outside the plan's closed set).
async fn plan_post_mentions(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    thread_id: Option<&str>,
    post: &ValidPost,
) -> Result<Option<PlannedMentions>, Reply> {
    let Some(raw) = post.mentions.as_ref() else {
        return Ok(None);
    };
    if !state.space_features.mention_records_api {
        return Ok(None);
    }
    let input = mentions::MentionsInput::parse(raw).map_err(invalid_mentions)?;
    if input.is_empty() {
        return Ok(None);
    }
    let unavailable = || PlannedMentions {
        entries: Vec::new(),
        rejected: input
            .people
            .iter()
            .map(|person| person.subject_id.as_str())
            .chain(input.agents.iter().map(|agent| agent.subject_id.as_str()))
            .map(|subject_id| json!({ "subject_id": subject_id, "code": "mention_unavailable" }))
            .collect(),
    };
    let Ok(members) = super::fetch_space_roster_members(state, user, org_id, space_ref).await else {
        return Ok(Some(unavailable()));
    };
    let Ok(role) = super::resolve_space_role(state, user, org_id, space_ref).await else {
        return Ok(Some(unavailable()));
    };
    let agents = if input.agents.is_empty() {
        Vec::new()
    } else {
        super::compose_space_agents_from_members(state, user, org_id, space_ref, &members).await
    };
    // A new thread is the poster's own. In an existing one only the owner's runs can start, and the
    // owner-bound by-id read shows the thread to its owner alone: not visible reads as "not theirs".
    let thread_owner = match thread_id {
        None => mentions::ThreadOwner::Invoker,
        Some(_) if input.agents.is_empty() => mentions::ThreadOwner::Unknown,
        Some(thread_id) => match space_scope_for_thread(state, user, headers, thread_id).await {
            Ok(SpaceThreadScope::NotVisible) => mentions::ThreadOwner::Other,
            Ok(_) => mentions::ThreadOwner::Invoker,
            Err(_) => mentions::ThreadOwner::Unknown,
        },
    };
    let invoker = mentions::Invoker {
        subject_id: &user.user_id,
        role: &role,
    };
    let plan = mentions::plan_mentions(&post.content, &input, &members, &agents, &invoker, thread_owner)
        .map_err(invalid_mentions)?;
    Ok(Some(PlannedMentions {
        entries: plan.session_entries(),
        rejected: plan.rejected_json(),
    }))
}

/// 400 `invalid_mentions` with the closed `details.reason`.
fn invalid_mentions(reason: mentions::InvalidMentions) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_mentions",
            "The mentions of this post are not valid.",
            json!({ "reason": reason.reason() }),
        )),
    )
}

/// Adds `data.mentions:{recorded, rejected}` to a successful post answer when the post carried
/// mentions. `recorded` is what Session Core says it recorded (never the plan), so a switch that is
/// off reports nothing as recorded.
fn attach_mentions(mut reply: Reply, planned: Option<&PlannedMentions>, recorded: Vec<Value>) -> Reply {
    let Some(planned) = planned else {
        return reply;
    };
    if !reply.0.is_success() {
        return reply;
    }
    if let Some(data) = reply.1 .0.get_mut("data").and_then(Value::as_object_mut) {
        data.insert(
            "mentions".to_owned(),
            json!({ "recorded": recorded, "rejected": planned.rejected }),
        );
    }
    reply
}

/// A model-gateway failure body as the browser envelope. A typed body
/// (`{error:{code,message,details}}`) keeps its code and is rebuilt from a bounded `message` and
/// the scalar entries of `details` only, so nothing else an upstream puts in a body (extra keys,
/// nested values, text of another service) reaches the browser. A ZDR catalogue code is rebuilt
/// from the catalogue itself. `None` when the body has no well-formed code.
pub(super) fn filter_model_error(response: &Value) -> Option<Value> {
    let error = response.get("error").and_then(Value::as_object)?;
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| is_error_code(code))?;
    if code == zdr::ZDR_DURABLE_MUTATION_FORBIDDEN || code == zdr::ZDR_DECISION_UNAVAILABLE {
        return Some(zdr::zdr_refusal_body(code));
    }
    let message: String = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(MAX_RELAYED_MESSAGE_CHARS)
        .collect();
    let details: Map<String, Value> = error
        .get("details")
        .and_then(Value::as_object)
        .map(|details| {
            details
                .iter()
                .filter(|(_, value)| !value.is_array() && !value.is_object())
                .take(MAX_RELAYED_DETAILS)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    Some(json!({ "error": { "code": code, "message": message, "details": details } }))
}

/// model-gateway's answer as the browser envelope. Its own errors keep their status and code and
/// are bounded by `filter_model_error`; anything else is `session_unavailable`.
fn map_model_response(status: StatusCode, response: Value, space_ref: Option<&str>) -> Reply {
    if !status.is_success() {
        // A 401 here is the delegated bearer being refused, not the browser session; relayed, the
        // SPA would sign the user out. It is an outage, like room_admin's typed 401.
        if status != StatusCode::UNAUTHORIZED {
            if let Some(body) = filter_model_error(&response) {
                return (status, Json(body));
            }
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
        // An archived room is final until someone unarchives it, so it must not read as "try again".
        StatusCode::CONFLICT
            if body.pointer("/error/code").and_then(Value::as_str) == Some("room_archived") =>
        {
            refuse(
                StatusCode::CONFLICT,
                "room_archived",
                "The room is archived; posting is closed.",
            )
        }
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
    async fn an_archived_room_refusal_stays_room_archived_on_create_and_reply() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (decision, uri) in [
            ("thread-decision", create_uri()),
            ("thread-append-decision", reply_uri()),
        ] {
            let fx = default_fixture().await;
            mount_decision_status(
                &fx,
                decision,
                ResponseTemplate::new(409).set_body_json(json!({
                    "error": {"code": "room_archived", "message": "archived", "details": {}}
                })),
            )
            .await;
            let (status, body, raw) = send(&fx.state, uri, None, post("hello")).await;
            assert_eq!(status, 409, "{decision}: {raw}");
            assert_eq!(body["error"]["code"], "room_archived", "{decision}");
            assert!(model_calls(&fx).await.is_empty(), "{decision}");
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

    /// The caller as the router builds it, for the tests that drive `submit_post_limited` with a
    /// limiter of their own.
    fn caller(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn session_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            "better-auth.session_token=session-1".parse().unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn the_post_ceiling_is_per_user_and_per_org_and_answers_a_typed_429() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let state = AppState::for_tests();
        let limiter = RateLimiter::from_cache_with_rpm(&state.cache, 2.0);
        assert!(check_post_ceiling(&limiter, "org-1", "user-1").await.is_ok());
        assert!(check_post_ceiling(&limiter, "org-1", "user-1").await.is_ok());
        let (status, Json(body)) = check_post_ceiling(&limiter, "org-1", "user-1")
            .await
            .expect_err("the third post in the minute is refused");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        // Another user, and the same user in another organization, have their own bucket.
        assert!(check_post_ceiling(&limiter, "org-1", "user-2").await.is_ok());
        assert!(check_post_ceiling(&limiter, "org-2", "user-1").await.is_ok());
    }

    #[tokio::test]
    async fn a_refused_post_calls_neither_control_nor_model_gateway() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored(false)).await;
        let user = caller("user-ceiling");
        let headers = session_headers();
        // One post an hour is the whole bucket: the first lands, creates and replies share it.
        let limiter = RateLimiter::from_cache_with_rpm(&fx.state.cache, 1.0);
        let (status, Json(body)) = submit_post_limited(
            &fx.state, &limiter, &user, &headers, "space-room", None, &post("hello"),
        )
        .await;
        assert_eq!(status.as_u16(), 201, "{body}");
        let control_before = control_calls(&fx).await.len();
        let model_before = model_calls(&fx).await.len();
        assert!(control_before >= 1 && model_before == 1);
        for thread in [None, Some("thread-1")] {
            let (status, Json(body)) = submit_post_limited(
                &fx.state, &limiter, &user, &headers, "space-room", thread, &post("again"),
            )
            .await;
            assert_eq!(status.as_u16(), 429, "{body}");
            assert_eq!(body["error"]["code"], "rate_limited");
            assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        }
        assert_eq!(control_calls(&fx).await.len(), control_before, "no Control call");
        assert_eq!(model_calls(&fx).await.len(), model_before, "no model-gateway call");
    }

    // The Dragonfly path of the post ceiling. The tests above run the in-process fallback (the
    // test state has no cache). These need a real server and are `#[ignore]`d by default:
    //
    //   docker run --rm -d --name space-leftovers-dragonfly -p 127.0.0.1:<port>:6379 \
    //     docker.dragonflydb.io/dragonflydb/dragonfly      (redis:7-alpine works too)
    //   SPACE_LIMITER_REDIS_URL=redis://127.0.0.1:<port> \
    //     cargo test -- --ignored post_ceiling_redis --test-threads=1
    //
    // Every test names its own org and user, so a run never needs a flush and never meets a
    // bucket another run left behind.

    const REDIS_URL_ENV: &str = "SPACE_LIMITER_REDIS_URL";

    /// `(host:port, userinfo-with-@-or-empty)` of the server under test.
    fn redis_endpoint() -> (String, String) {
        let url = std::env::var(REDIS_URL_ENV)
            .unwrap_or_else(|_| panic!("set {REDIS_URL_ENV} to a throwaway Dragonfly or Redis"));
        let rest = url.trim().split_once("://").map_or(url.trim(), |(_, rest)| rest);
        let rest = rest.split('/').next().unwrap_or(rest);
        match rest.rsplit_once('@') {
            Some((userinfo, host)) => (host.to_owned(), format!("{userinfo}@")),
            None => (rest.to_owned(), String::new()),
        }
    }

    async fn redis_cache(host: &str, userinfo: &str) -> crate::cache::ResultCache {
        let cache = crate::cache::ResultCache::connect(Some(&format!("redis://{userinfo}{host}"))).await;
        assert!(
            cache.connection().is_some(),
            "no connection to {host}: the Dragonfly path would silently not run"
        );
        cache
    }

    /// An org id and a user id nobody else has used.
    fn fresh_ids(tag: &str) -> (String, String) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let unique = format!("{tag}-{}-{nanos}", std::process::id());
        (format!("org-{unique}"), format!("user-{unique}"))
    }

    /// Whether the limiter's bucket for this caller exists in the server: proof the answer came
    /// from Dragonfly and not from the in-process fallback.
    async fn bucket_in_server(cache: &crate::cache::ResultCache, org: &str, user: &str) -> bool {
        let mut conn = cache.connection().expect("connected");
        let key = format!("verevon:gw:ratelimit:space-post:{org}:{user}");
        redis::cmd("EXISTS")
            .arg(key)
            .query_async::<i64>(&mut conn)
            .await
            .expect("EXISTS")
            == 1
    }

    #[tokio::test]
    #[ignore = "needs SPACE_LIMITER_REDIS_URL (a throwaway Dragonfly or Redis)"]
    async fn post_ceiling_redis_is_one_bucket_across_create_and_reply_then_a_typed_429() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (host, userinfo) = redis_endpoint();
        let cache = redis_cache(&host, &userinfo).await;
        let fx = default_fixture().await;
        mount_decision(&fx, "thread-decision").await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored(false)).await;
        // The fixture's session-context answers org-1, so the bucket is `space-post:org-1:<user>`.
        let (_, user_id) = fresh_ids("shared");
        let user = caller(&user_id);
        let headers = session_headers();
        // Two posts a minute: one create and one reply spend the same two tokens.
        let limiter = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        let (status, Json(body)) = submit_post_limited(
            &fx.state, &limiter, &user, &headers, "space-room", None, &post("first"),
        )
        .await;
        assert_eq!(status.as_u16(), 201, "{body}");
        let (status, Json(body)) = submit_post_limited(
            &fx.state, &limiter, &user, &headers, "space-room", Some("thread-1"), &post("second"),
        )
        .await;
        assert_eq!(status.as_u16(), 201, "{body}");
        assert!(bucket_in_server(&cache, "org-1", &user_id).await, "bucket lives in the server");
        let control_before = control_calls(&fx).await.len();
        let model_before = model_calls(&fx).await.len();
        for thread in [None, Some("thread-1")] {
            let (status, Json(body)) = submit_post_limited(
                &fx.state, &limiter, &user, &headers, "space-room", thread, &post("third"),
            )
            .await;
            assert_eq!(status.as_u16(), 429, "{body}");
            assert_eq!(body["error"]["code"], "rate_limited");
            assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        }
        assert_eq!(control_calls(&fx).await.len(), control_before, "no Control call");
        assert_eq!(model_calls(&fx).await.len(), model_before, "no model-gateway call");
    }

    #[tokio::test]
    #[ignore = "needs SPACE_LIMITER_REDIS_URL (a throwaway Dragonfly or Redis)"]
    async fn post_ceiling_redis_keeps_a_bucket_per_user_and_per_org_across_replicas() {
        let (host, userinfo) = redis_endpoint();
        let cache = redis_cache(&host, &userinfo).await;
        // Two limiters on one server are two gateway replicas: the ceiling is fleet-wide.
        let replica_a = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        let replica_b = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        let (org_1, user_1) = fresh_ids("iso-a");
        let (org_2, user_2) = fresh_ids("iso-b");
        assert!(check_post_ceiling(&replica_a, &org_1, &user_1).await.is_ok());
        assert!(check_post_ceiling(&replica_b, &org_1, &user_1).await.is_ok());
        let (status, Json(body)) = check_post_ceiling(&replica_a, &org_1, &user_1)
            .await
            .expect_err("two replicas spend one bucket");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        assert!(check_post_ceiling(&replica_b, &org_1, &user_1).await.is_err());
        // Another user in the same org, and the same user in another org, are untouched.
        assert!(check_post_ceiling(&replica_a, &org_1, &user_2).await.is_ok());
        assert!(check_post_ceiling(&replica_a, &org_2, &user_1).await.is_ok());
        for (org, user) in [(&org_1, &user_1), (&org_1, &user_2), (&org_2, &user_1)] {
            assert!(bucket_in_server(&cache, org, user).await, "{org}/{user}");
        }
        // The org id is trimmed into the key, as the route sends it.
        assert!(check_post_ceiling(&replica_a, &format!(" {org_1} "), &user_1).await.is_err());
    }

    /// A TCP forwarder to the server under test that can be cut, standing in for "Redis goes
    /// down" without touching the container.
    struct CutProxy {
        addr: std::net::SocketAddr,
        accept: tokio::task::JoinHandle<()>,
        live: std::sync::Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    }

    impl CutProxy {
        async fn start(upstream: String) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            let live = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let tracked = live.clone();
            let accept = tokio::spawn(async move {
                while let Ok((mut inbound, _)) = listener.accept().await {
                    let upstream = upstream.clone();
                    let pipe = tokio::spawn(async move {
                        if let Ok(mut outbound) = tokio::net::TcpStream::connect(&upstream).await {
                            let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                        }
                    });
                    tracked.lock().unwrap().push(pipe);
                }
            });
            Self { addr, accept, live }
        }

        /// Close the listener and every open connection: the server is now unreachable.
        fn cut(&self) {
            self.accept.abort();
            for pipe in self.live.lock().unwrap().drain(..) {
                pipe.abort();
            }
        }
    }

    #[tokio::test]
    #[ignore = "needs SPACE_LIMITER_REDIS_URL (a throwaway Dragonfly or Redis)"]
    async fn post_ceiling_redis_outage_falls_back_to_the_in_process_bucket_not_to_no_ceiling() {
        let (host, userinfo) = redis_endpoint();
        let proxy = CutProxy::start(host).await;
        let cache = redis_cache(&proxy.addr.to_string(), &userinfo).await;
        let limiter = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        let (org, user) = fresh_ids("outage");
        // Up: the server decides.
        assert!(check_post_ceiling(&limiter, &org, &user).await.is_ok());
        assert!(check_post_ceiling(&limiter, &org, &user).await.is_ok());
        assert!(check_post_ceiling(&limiter, &org, &user).await.is_err());
        assert!(bucket_in_server(&cache, &org, &user).await);
        // Down: no 5xx and no stall (each call gives up after the limiter's own 500 ms deadline),
        // and the ceiling is still a ceiling. The in-process bucket starts full, so the first two
        // posts of the outage land and the third is the typed 429. This is `rate_limit.rs`'s
        // documented degrade: a Dragonfly blip does not reject posts, and it does not lift the cap.
        proxy.cut();
        let started = std::time::Instant::now();
        assert!(check_post_ceiling(&limiter, &org, &user).await.is_ok());
        assert!(check_post_ceiling(&limiter, &org, &user).await.is_ok());
        let (status, Json(body)) = check_post_ceiling(&limiter, &org, &user)
            .await
            .expect_err("the in-process bucket still refuses the third post");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "an outage must not hold a post open: {:?}",
            started.elapsed()
        );
        // Another caller is still on its own bucket during the outage.
        let (other_org, other_user) = fresh_ids("outage-other");
        assert!(check_post_ceiling(&limiter, &other_org, &other_user).await.is_ok());
    }

    /// A RESP peer that answers every command with an error, like a Redis that is up but refusing
    /// work (LOADING, OOM, READONLY). Returns its address and the command names it has seen.
    async fn erroring_redis() -> (std::net::SocketAddr, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let record = record.clone();
                tokio::spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut read = tokio::io::BufReader::new(read);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let Some(count) =
                            line.trim_end().strip_prefix('*').and_then(|n| n.parse::<usize>().ok())
                        else {
                            return;
                        };
                        let mut name = String::new();
                        for index in 0..count {
                            line.clear();
                            if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                                return;
                            }
                            let Some(len) =
                                line.trim_end().strip_prefix('$').and_then(|n| n.parse::<usize>().ok())
                            else {
                                return;
                            };
                            let mut data = vec![0u8; len + 2];
                            if read.read_exact(&mut data).await.is_err() {
                                return;
                            }
                            if index == 0 {
                                name = String::from_utf8_lossy(&data[..len]).to_uppercase();
                            }
                        }
                        record.lock().unwrap().push(name);
                        if write.write_all(b"-ERR refusing work\r\n").await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn post_ceiling_with_a_redis_that_answers_errors_is_the_in_process_bucket() {
        // Connected, but every command fails: the limiter's `Ok(Err)` branch, not the timeout and
        // not the no-connection one. The ceiling must still hold, as a typed 429, with no 5xx.
        let (addr, seen) = erroring_redis().await;
        let cache = crate::cache::ResultCache::connect(Some(&format!("redis://{addr}"))).await;
        assert!(cache.connection().is_some(), "the erroring peer must still count as connected");
        let limiter = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        let started = std::time::Instant::now();
        assert!(check_post_ceiling(&limiter, "org-1", "user-errors").await.is_ok());
        assert!(check_post_ceiling(&limiter, "org-1", "user-errors").await.is_ok());
        let (status, Json(body)) = check_post_ceiling(&limiter, "org-1", "user-errors")
            .await
            .expect_err("a Redis that errors is not a Redis that lifts the cap");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(
            seen.lock().unwrap().iter().any(|name| name.starts_with("EVAL")),
            "the limiter must have tried the server first: {:?}",
            seen.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn post_ceiling_with_an_unreachable_cache_is_the_in_process_bucket() {
        // Startup with the cache down: `ResultCache::connect` yields no connection (nothing
        // listens on port 1), so the limiter is purely in-process and still caps the user.
        let cache = crate::cache::ResultCache::connect(Some("redis://127.0.0.1:1")).await;
        assert!(cache.connection().is_none());
        let limiter = RateLimiter::from_cache_with_rpm(&cache, 2.0);
        assert!(check_post_ceiling(&limiter, "org-1", "user-down").await.is_ok());
        assert!(check_post_ceiling(&limiter, "org-1", "user-down").await.is_ok());
        let (status, Json(body)) = check_post_ceiling(&limiter, "org-1", "user-down")
            .await
            .expect_err("no cache is not no ceiling");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
    }

    #[test]
    fn a_model_gateway_error_is_bounded_before_it_reaches_the_browser() {
        let long = "m".repeat(1000);
        let mut details: Map<String, Value> =
            (0..40).map(|i| (format!("k{i:02}"), json!(i))).collect();
        details.insert("nested".to_owned(), json!({"internal": "http://session-core:8080"}));
        details.insert("list".to_owned(), json!(["a", "b"]));
        let (status, Json(body)) = map_model_response(
            StatusCode::CONFLICT,
            json!({"error": {"code": "thread_state_conflict", "message": long, "details": details,
                "stack": "at session-core/src/append.rs:10"}, "trace": "x"}),
            None,
        );
        assert_eq!(status.as_u16(), 409);
        assert_eq!(body["error"]["code"], "thread_state_conflict");
        assert_eq!(body["error"]["message"].as_str().unwrap().chars().count(), 300);
        let kept = body["error"]["details"].as_object().unwrap();
        assert_eq!(kept.len(), 16);
        assert!(!kept.contains_key("nested") && !kept.contains_key("list"));
        assert!(body.get("trace").is_none() && body["error"].get("stack").is_none());

        // Nested values are dropped even when few details are sent.
        let (_, Json(body)) = map_model_response(
            StatusCode::BAD_GATEWAY,
            json!({"error": {"code": "post_not_stored", "message": "m",
                "details": {"thread_id": "thread-9", "cause": {"url": "http://x"}}}}),
            None,
        );
        assert_eq!(body["error"]["details"], json!({"thread_id": "thread-9"}));

        // No details in, an empty details object out: the envelope keeps its shape.
        let (_, Json(body)) = map_model_response(
            StatusCode::NOT_FOUND,
            json!({"error": {"code": "thread_not_found"}}),
            None,
        );
        assert_eq!(body["error"]["details"], json!({}));
        assert_eq!(body["error"]["message"], "");
    }

    #[test]
    fn an_untyped_model_gateway_error_maps_to_the_generic_session_unavailable() {
        for response in [
            json!({"error": "boom"}),
            json!({"error": {"message": "no code"}}),
            json!({"error": {"code": "Not A Code!", "message": "m"}}),
            json!({"error": {"code": 500, "message": "m"}}),
            json!({"detail": "something"}),
            Value::Null,
        ] {
            let (status, Json(body)) =
                map_model_response(StatusCode::INTERNAL_SERVER_ERROR, response, None);
            assert_eq!(status.as_u16(), 503);
            assert_eq!(body["error"]["code"], "session_unavailable");
            assert_eq!(
                body["error"]["message"],
                "The conversation service is unavailable."
            );
        }
    }

    #[test]
    fn a_model_gateway_401_never_reaches_the_browser_as_a_signed_out_user() {
        let (status, Json(body)) = map_model_response(
            StatusCode::UNAUTHORIZED,
            json!({"error": {"code": "unauthorized", "message": "authentication required",
                "details": {}}}),
            None,
        );
        assert_eq!(status.as_u16(), 503);
        assert_eq!(body["error"]["code"], "session_unavailable");
    }

    #[test]
    fn a_zdr_code_from_model_gateway_is_rebuilt_from_the_catalogue() {
        let (status, Json(body)) = map_model_response(
            StatusCode::PRECONDITION_FAILED,
            json!({"error": {"code": "zdr_durable_mutation_forbidden", "message": "x".repeat(900),
                "details": {"internal": {"k": "v"}}}}),
            None,
        );
        assert_eq!(status.as_u16(), 412);
        assert_eq!(
            body,
            zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)
        );
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

    // ---- WS08-S4: mentions on the two post routes -----------------------------------------------

    const ROSTER_PATH: &str = "/api/v1/internal/spaces/space-room/roster";

    async fn mount_roster(fx: &Fx, template: ResponseTemplate) {
        Mock::given(wm_method("GET"))
            .and(wm_path(ROSTER_PATH))
            .respond_with(template)
            .mount(&fx.user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "editor"}
            })))
            .mount(&fx.user_core)
            .await;
    }

    fn roster_with(ids: &[&str]) -> ResponseTemplate {
        let members: Vec<Value> = ids
            .iter()
            .map(|id| json!({"subject_type": "user", "subject_id": id, "role": "editor", "status": "active"}))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"data": {"members": members, "count": members.len()}}))
    }

    fn stored_with_mentions(mentions: Value) -> ResponseTemplate {
        ResponseTemplate::new(201).set_body_json(json!({
            "thread_id": "thread-1",
            "message_id": format!("space-post-{SEND_ID}"),
            "sequence": 1,
            "deduplicated": false,
            "mentions": mentions,
        }))
    }

    fn post_with_mentions(content: &str, mentions: Value) -> Value {
        json!({"content": content, "client_send_id": SEND_ID, "mentions": mentions})
    }

    async fn roster_calls(fx: &Fx) -> usize {
        fx.user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == ROSTER_PATH)
            .count()
    }

    #[tokio::test]
    async fn a_post_with_mentions_forwards_the_entries_and_returns_recorded_and_rejected() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = default_fixture().await;
        fx.state.space_features.mention_records_api = true;
        mount_roster(&fx, roster_with(&["u-ada"])).await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(
            &fx,
            "/v1/threads",
            stored_with_mentions(json!([{"subject_id": "u-ada", "kind": "user", "ordinal": 0}])),
        )
        .await;
        let mentions = json!({"people": [
            {"subject_id": "u-ada", "label": "Ada"},
            {"subject_id": "u-gone", "label": "Gone"}
        ]});
        let (status, body, raw) =
            send(&fx.state, create_uri(), None, post_with_mentions("hello @Ada and @Gone", mentions)).await;
        assert_eq!(status, 201, "{raw}");
        assert_eq!(
            body["data"]["mentions"],
            json!({
                "recorded": [{"subject_id": "u-ada", "kind": "user", "ordinal": 0}],
                "rejected": [{"subject_id": "u-gone", "code": "person_not_in_room"}]
            })
        );
        assert_no_secret(&raw);
        let model = model_calls(&fx).await;
        assert_eq!(model.len(), 1);
        assert_eq!(
            model[0].1["first_message"]["mentions"],
            json!([{"subject_id": "u-ada", "kind": "user", "source": "typed", "ordinal": 0}]),
            "ids and closed words only"
        );
        assert!(model[0].1["first_message"]["content"].as_str().unwrap().contains("@Ada"));
    }

    #[tokio::test]
    async fn a_reply_forwards_the_mentions_at_the_top_level_of_the_append() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = default_fixture().await;
        fx.state.space_features.mention_records_api = true;
        mount_roster(&fx, roster_with(&["u-ada"])).await;
        mount_decision(&fx, "thread-append-decision").await;
        mount_model(&fx, "/v1/threads/thread-1/messages", stored_with_mentions(json!([]))).await;
        let mentions = json!({"people": [{"subject_id": "u-ada", "label": "ada"}]});
        let (status, body, raw) =
            send(&fx.state, reply_uri(), None, post_with_mentions("thanks @ADA", mentions)).await;
        assert_eq!(status, 201, "{raw}");
        let model = model_calls(&fx).await;
        assert_eq!(model[0].1["mentions"][0]["subject_id"], "u-ada");
        // the receipt, not the plan, says what was recorded: a receipt with none reports none
        assert_eq!(body["data"]["mentions"]["recorded"], json!([]));
    }

    #[tokio::test]
    async fn with_the_flag_off_mentions_are_ignored_and_nothing_is_read() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture().await;
        mount_roster(&fx, roster_with(&["u-ada"])).await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        let mentions = json!({"people": [{"subject_id": "u-ada", "label": "Ada"}]});
        let (status, body, raw) = send(&fx.state, create_uri(), None, post_with_mentions("hi @Ada", mentions)).await;
        assert_eq!(status, 201, "{raw}");
        assert!(body["data"].get("mentions").is_none());
        assert_eq!(roster_calls(&fx).await, 0, "no roster read");
        assert!(model_calls(&fx).await[0].1["first_message"].get("mentions").is_none());
        // even a malformed object is ignored while the switch is off
        let (status, _, raw) = send(&fx.state, create_uri(), None, post_with_mentions("hi", json!("junk"))).await;
        assert_eq!(status, 201, "{raw}");
    }

    #[tokio::test]
    async fn a_malformed_mentions_object_is_a_400_invalid_mentions_before_control_and_model() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = default_fixture().await;
        fx.state.space_features.mention_records_api = true;
        mount_roster(&fx, roster_with(&["u-ada"])).await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        for (mentions, reason) in [
            (json!("junk"), "malformed"),
            (json!({"people": [], "email": "x"}), "malformed"),
            (json!({"people": [{"subject_id": "u-ada", "label": "Ada"}, {"subject_id": "u-ada", "label": "Ada"}]}), "duplicate"),
            (json!({"people": [{"subject_id": "bad id", "label": "Ada"}]}), "bad_subject_id"),
            (json!({"agents": [{"subject_id": "a1"}, {"subject_id": "a2"}, {"subject_id": "a3"}, {"subject_id": "a4"}]}), "too_many_agents"),
        ] {
            let (status, body, _) = send(&fx.state, create_uri(), None, post_with_mentions("hi @Ada", mentions.clone())).await;
            assert_eq!(status, 400, "{mentions}");
            assert_eq!(body["error"]["code"], "invalid_mentions");
            assert_eq!(body["error"]["details"]["reason"], reason, "{mentions}");
        }
        assert!(
            control_calls(&fx).await.iter().all(|(path, _)| !path.contains("decision")),
            "no decision was minted"
        );
        assert!(model_calls(&fx).await.is_empty(), "the post never reached model-gateway");
    }

    #[tokio::test]
    async fn a_roster_outage_reports_mention_unavailable_and_the_post_stands() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = default_fixture().await;
        fx.state.space_features.mention_records_api = true;
        mount_roster(&fx, ResponseTemplate::new(503)).await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        let mentions = json!({"people": [{"subject_id": "u-ada", "label": "Ada"}]});
        let (status, body, raw) = send(&fx.state, create_uri(), None, post_with_mentions("hi @Ada", mentions)).await;
        assert_eq!(status, 201, "the post stands: {raw}");
        assert_eq!(
            body["data"]["mentions"],
            json!({"recorded": [], "rejected": [{"subject_id": "u-ada", "code": "mention_unavailable"}]})
        );
        assert!(model_calls(&fx).await[0].1["first_message"].get("mentions").is_none(), "no unplanned entries are sent");
    }

    #[tokio::test]
    async fn a_zdr_post_with_mentions_is_412_before_the_roster_or_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(org_posture(true), active_space()).await;
        fx.state.space_features.mention_records_api = true;
        mount_roster(&fx, roster_with(&["u-ada"])).await;
        mount_decision(&fx, "thread-decision").await;
        mount_model(&fx, "/v1/threads", stored(false)).await;
        let mentions = json!({"people": [{"subject_id": "u-ada", "label": "Ada"}]});
        let (status, body, _) = send(&fx.state, create_uri(), None, post_with_mentions("hi @Ada", mentions)).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(roster_calls(&fx).await, 0);
        assert!(control_calls(&fx).await.is_empty());
        assert!(model_calls(&fx).await.is_empty());
    }
}
