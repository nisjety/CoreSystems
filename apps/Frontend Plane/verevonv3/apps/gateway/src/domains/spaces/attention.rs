//! The attention surface: the BFF half of WS11-S3 (aggregate, read-all, notify level, mark unread)
//! and WS11-S7 (inbox, item state, thread follow).
//!
//! * `GET /api/v1/spaces/attention?refs=` is the polled cross-room aggregate with the one badge
//!   per room (`room_badge`); `POST /api/v1/spaces/attention/read-all`, `GET|PUT
//!   /api/v1/spaces/{space_ref}/notify` and `POST /api/v1/spaces/{space_ref}/unread` follow the
//!   same gate (flag off is 404 `space_attention_disabled`) and write only the caller's own rows.
//! * `GET /api/v1/spaces/attention/items?state=open|snoozed|done&kind=&limit=&cursor=` lists the
//!   caller's own Application inbox rows (`spaceAttentionItems`: ids, a closed kind word and
//!   timestamps, no text), newest first, and hydrates them for display.
//! * `POST /api/v1/spaces/attention/items/{item_id}/state` marks one of the caller's own rows
//!   `done`, `open` or `snoozed`.
//! * `GET|PUT /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` reads and sets the caller's
//!   follow of one thread (`following`, `muted`, `none`). A follow is durable, so under Zero Data
//!   Retention setting one is the typed 412; clearing one never is.
//!
//! Hydration. A row stores no title. The BFF drops every row whose Space is absent from Control's
//! actor-filtered index, asks Control for one `model.thread.read` decision per remaining Space in a
//! single bulk request (`thread-read-decisions`, at most 60), and forwards those decisions to
//! Session Core (through model-gateway, decisions in the body) which verifies each one and returns
//! a display title only for a thread that passes the decision's audience ceiling. A title is
//! therefore re-read at render under the viewer's CURRENT access and is never stored here. Per row
//! `headline_state` is `ok` (a title, or a kind that has no thread), `hidden` (the viewer chose
//! `hide_preview`) or `restricted` (no decision, a denied Space, Zero Data Retention, or a thread
//! the viewer can no longer read). The first page of `state=open` also carries `needs_you`: the
//! approvals waiting on the caller, derived live by Session Core and never stored as items.
//! Every upstream source that fails becomes a named entry in `gaps`, never a 5xx for the list.
//!
//! Authority is not here: the rows are the caller's own (the Convex functions take the verified
//! user and org and touch nothing else), the Space set is Control's, and the titles are Session
//! Core's after verifying Control's decisions. Flag off gives 404 `space_attention_disabled`. The
//! flag is a rollout control, never an authorization input.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_state` /
//! `invalid_kind` / `invalid_limit` / `invalid_cursor` / `invalid_snooze` / `invalid_space` /
//! `invalid_thread` / `invalid_follow_state`, 403 `active_membership_required`, 404
//! `space_attention_disabled` / `attention_item_not_found`, 412 `zdr_durable_mutation_forbidden`,
//! 503 `space_index_unavailable` / `attention_unavailable`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use axum::{
    body::Bytes,
    extract::{rejection::JsonRejection, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{
    control_space_index, convex_gateway_call, organization_space_labels,
    room_admin::convex_mutation_message, resolve_space_role, unix_nanos,
};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::{authorized_org_id, proxy_json},
};

type Reply = (StatusCode, Json<Value>);

/// The closed kind words (the Application `spaceAttentionItems.kind` union).
const KINDS: [&str; 6] = ["mention", "reply", "invite", "agent_done", "agent_failed", "reminder"];
const STATES: [&str; 3] = ["open", "snoozed", "done"];
const FOLLOW_STATES: [&str; 3] = ["following", "muted", "none"];
/// Control's bulk decision route and Session Core's aggregate both cap a request at 60 Spaces.
const MAX_SPACES: usize = 60;
/// Session Core returns at most 20 titles per Space.
const HEADLINES_PER_SPACE: usize = 20;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
const MAX_ID_LEN: usize = 200;
const MAX_CURSOR_LEN: usize = 2048;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        // Static segments: matchit ranks them above `{space_ref}`, so a room can never shadow them.
        .route("/api/v1/spaces/attention", get(get_attention))
        .route("/api/v1/spaces/attention/read-all", post(read_all))
        .route("/api/v1/spaces/attention/items", get(list_items))
        .route(
            "/api/v1/spaces/attention/items/{item_id}/state",
            post(set_item_state),
        )
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/follow",
            get(get_follow).put(put_follow),
        )
        .route(
            "/api/v1/spaces/{space_ref}/notify",
            get(get_notify).put(put_notify),
        )
        .route("/api/v1/spaces/{space_ref}/unread", post(post_unread))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_attention_disabled",
        "Space attention is not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization membership is required to read attention.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "attention_unavailable",
        "Attention is unavailable right now. Nothing was changed.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request body is not valid.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control)
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// Flag, then organization. Everything every route shares.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.attention_api {
        return Err(disabled());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    Ok(org_id)
}

// ── the list ─────────────────────────────────────────────────────────────────────────────────────

struct ListQuery {
    state: String,
    kind: Option<String>,
    limit: u32,
    cursor: Option<String>,
}

fn parse_query(params: &HashMap<String, String>) -> Result<ListQuery, Reply> {
    let state = params
        .get("state")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("open");
    if !STATES.contains(&state) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_state",
            "state must be open, snoozed or done.",
        ));
    }
    let kind = match params.get("kind").map(|value| value.trim()) {
        None | Some("") => None,
        Some(kind) if KINDS.contains(&kind) => Some(kind.to_owned()),
        Some(_) => {
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                "invalid_kind",
                "kind is not a known attention kind.",
            ))
        }
    };
    let limit = match params.get("limit").map(|value| value.trim()) {
        None | Some("") => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
            _ => {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_limit",
                    "limit must be between 1 and 100.",
                ))
            }
        },
    };
    let cursor = match params.get("cursor").map(String::as_str) {
        None | Some("") => None,
        Some(cursor) if cursor.len() <= MAX_CURSOR_LEN && !cursor.chars().any(char::is_control) => {
            Some(cursor.to_owned())
        }
        Some(_) => {
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not valid.",
            ))
        }
    };
    Ok(ListQuery {
        state: state.to_owned(),
        kind,
        limit,
        cursor,
    })
}

/// One Control `model.thread.read` decision per Space, in a single request. A Space the caller may
/// not read, or any failure, is simply absent from the answer: its titles then read `restricted`.
/// `Err(())` is Control being unreachable or answering something unreadable.
async fn bulk_read_decisions(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_refs: &[String],
) -> Result<HashMap<String, (String, String)>, ()> {
    if space_refs.is_empty() {
        return Ok(HashMap::new());
    }
    let stamp = unix_nanos();
    let items: Vec<Value> = space_refs
        .iter()
        .enumerate()
        .map(|(index, space_ref)| {
            json!({
                "space_ref": space_ref,
                "idempotency_key": format!("attn-{stamp}-{index}"),
            })
        })
        .collect();
    let url = format!(
        "{}/api/v1/internal/spaces/thread-read-decisions",
        state.user_core_url
    );
    let actor = actor_of(user);
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "items": items })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err(());
    }
    let data = unwrap_data(&response);
    let results = data.get("results").and_then(Value::as_array).ok_or(())?;
    let mut decisions = HashMap::new();
    for result in results {
        if result.get("status").and_then(Value::as_str) != Some("issued") {
            continue;
        }
        let space_ref = result.get("space_ref").and_then(Value::as_str).unwrap_or_default();
        let decision_ref = result
            .get("decision")
            .and_then(|decision| decision.get("decision_ref"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let token = result.get("token").and_then(Value::as_str).unwrap_or_default();
        if !space_ref.is_empty() && !decision_ref.is_empty() && !token.is_empty() {
            decisions.insert(
                space_ref.to_owned(),
                (decision_ref.to_owned(), token.to_owned()),
            );
        }
    }
    Ok(decisions)
}

/// What Session Core said for the Spaces asked about: titles by (space, thread) and approvals
/// waiting on the caller, both only for Spaces whose status was `ok`.
#[derive(Default)]
struct SessionAnswer {
    titles: HashMap<(String, String), String>,
    pending: Vec<Value>,
    ok_spaces: HashSet<String>,
}

/// `POST /v1/spaces/attention` through model-gateway with the spaces already built: decisions
/// travel in the body only. The `results` array as Session Core answered it, or `None` when the
/// bearers or the upstream are unavailable, which the caller names as a gap.
async fn post_session_attention(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    spaces: Vec<Value>,
) -> Option<Vec<Value>> {
    let session_token =
        crate::domains::chat::shared::required_session_token(state, user, headers)
            .await
            .ok()?;
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!("{}/v1/spaces/attention", state.model_gateway_url);
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({ "rule": "last_author", "spaces": spaces })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if !status.is_success() {
        tracing::info!(status = status.as_u16(), "attention headlines refused upstream");
        return None;
    }
    response.get("results").and_then(Value::as_array).cloned()
}

/// Titles and approvals for the Spaces named in `decisions`. `None` when Session Core could not
/// be asked, which the caller names as a gap.
async fn session_attention(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    decisions: &HashMap<String, (String, String)>,
    headline_ids: &HashMap<String, Vec<String>>,
) -> Option<SessionAnswer> {
    if decisions.is_empty() {
        return Some(SessionAnswer::default());
    }
    let spaces: Vec<Value> = decisions
        .iter()
        .map(|(space_ref, (decision_ref, token))| {
            json!({
                "space_id": space_ref,
                "decision_ref": decision_ref,
                "decision_token": token,
                // Titles and approvals only: this call counts nothing, so no marker is sent.
                "since_ms": 0,
                "headline_thread_ids": headline_ids.get(space_ref).cloned().unwrap_or_default(),
            })
        })
        .collect();
    let results = post_session_attention(state, user, headers, spaces).await?;
    let mut answer = SessionAnswer::default();
    for result in &results {
        // A result for a Space this request did not name is dropped, never trusted.
        let Some(space_ref) = result.get("space_id").and_then(Value::as_str) else {
            continue;
        };
        if !decisions.contains_key(space_ref)
            || result.get("status").and_then(Value::as_str) != Some("ok")
        {
            continue;
        }
        answer.ok_spaces.insert(space_ref.to_owned());
        for headline in result
            .get("headlines")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let thread_id = headline.get("thread_id").and_then(Value::as_str);
            let title = headline.get("title").and_then(Value::as_str);
            if let (Some(thread_id), Some(title)) = (thread_id, title) {
                answer
                    .titles
                    .insert((space_ref.to_owned(), thread_id.to_owned()), title.to_owned());
            }
        }
        for approval in result
            .get("pending")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            answer.pending.push(json!({
                "approval_id": approval.get("approval_id").cloned().unwrap_or(Value::Null),
                "space_ref": space_ref,
                "thread_id": approval.get("thread_id").cloned().unwrap_or(Value::Null),
                "run_id": approval.get("run_id").cloned().unwrap_or(Value::Null),
                "requested_at": approval.get("requested_at_ms").cloned().unwrap_or(Value::Null),
                "approval_kind": approval.get("kind").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    Some(answer)
}

fn str_field<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

fn actor_json(row: &Value) -> Value {
    let kind = str_field(row, "actorKind").unwrap_or("system");
    let actor_ref = str_field(row, "actorRef");
    match (kind, actor_ref) {
        ("user", Some(subject)) => json!({ "kind": "user", "subject_id": subject }),
        ("agent", Some(agent)) => json!({ "kind": "agent", "agent_ref": agent }),
        (kind, _) => json!({ "kind": kind }),
    }
}

/// The listing route's whole work, after the gate. Split from the handler so its pieces read in
/// order: query, Control index, Application rows, hydration, answer.
async fn list_items(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let query = match parse_query(&params) {
        Ok(query) => query,
        Err(reply) => return reply,
    };
    // Control's actor-filtered index decides which Spaces exist for this caller at all.
    let Some(index) = control_space_index(&state, &user, &org_id).await else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_index_unavailable",
            "Your rooms could not be listed right now.",
        );
    };
    let member_refs: Vec<String> = index
        .iter()
        .filter_map(|entry| str_field(entry, "space_ref").map(str::to_owned))
        .collect();
    let member_set: HashSet<&str> = member_refs.iter().map(String::as_str).collect();

    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("state".to_owned(), json!(query.state));
    args.insert("limit".to_owned(), json!(query.limit));
    if let Some(kind) = &query.kind {
        args.insert("kind".to_owned(), json!(kind));
    }
    if let Some(cursor) = &query.cursor {
        args.insert("cursor".to_owned(), json!(cursor));
    }
    let Ok(listed) = convex_gateway_call(
        &state,
        "query",
        "spaceAttention:listItemsForGateway",
        Value::Object(args),
    )
    .await
    else {
        return unavailable();
    };
    let hide_preview = listed.get("hidePreview").and_then(Value::as_bool) == Some(true);
    let next_cursor = listed.get("nextCursor").cloned().unwrap_or(Value::Null);
    // A row whose Space left the caller's index is dropped, not shown with a placeholder.
    let rows: Vec<&Value> = listed
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| str_field(row, "spaceRef").is_some_and(|space| member_set.contains(space)))
        .collect();

    let first_open_page = query.state == "open" && query.cursor.is_none();
    let zdr = crate::zdr::org_zdr_lookup(&state, &user).await.treat_as_zdr();
    let mut gaps: Vec<&'static str> = Vec::new();

    // Which Spaces need a decision: those holding rows, and on the first open page every room (for
    // the live approvals), at most 60, row Spaces first.
    let mut wanted: Vec<String> = Vec::new();
    for row in &rows {
        if let Some(space_ref) = str_field(row, "spaceRef") {
            if !wanted.iter().any(|seen| seen == space_ref) {
                wanted.push(space_ref.to_owned());
            }
        }
    }
    if first_open_page {
        for space_ref in &member_refs {
            if !wanted.contains(space_ref) {
                wanted.push(space_ref.clone());
            }
        }
    }
    wanted.truncate(MAX_SPACES);

    let mut headline_ids: HashMap<String, Vec<String>> = HashMap::new();
    if !hide_preview {
        for row in &rows {
            let (Some(space_ref), Some(thread_id)) =
                (str_field(row, "spaceRef"), str_field(row, "threadId"))
            else {
                continue;
            };
            let ids = headline_ids.entry(space_ref.to_owned()).or_default();
            if ids.len() < HEADLINES_PER_SPACE && !ids.iter().any(|seen| seen == thread_id) {
                ids.push(thread_id.to_owned());
            }
        }
    }

    // Zero Data Retention: no decision is minted and Session Core is not asked. Rows are refs-only
    // and are still listed; their titles read `restricted` and there are no live approvals.
    let mut answer = SessionAnswer::default();
    let mut decisions: HashMap<String, (String, String)> = HashMap::new();
    if !zdr && !wanted.is_empty() {
        match bulk_read_decisions(&state, &user, &org_id, &wanted).await {
            Ok(minted) => decisions = minted,
            Err(()) => gaps.push("space_decisions"),
        }
        if !decisions.is_empty() {
            match session_attention(&state, &user, &headers, &decisions, &headline_ids).await {
                Some(found) => answer = found,
                None => gaps.push("session_core"),
            }
        }
    }

    let names: HashMap<String, String> = organization_space_labels(&state, &user, &org_id)
        .await
        .iter()
        .filter_map(|space| {
            Some((
                str_field(space, "spaceRef")?.to_owned(),
                str_field(space, "name")?.to_owned(),
            ))
        })
        .collect();
    let name_of = |space_ref: &str| names.get(space_ref).map_or(Value::Null, |name| json!(name));

    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            let space_ref = str_field(row, "spaceRef").unwrap_or_default();
            let thread_id = str_field(row, "threadId");
            let (headline, headline_state) = match thread_id {
                None => (Value::Null, "ok"),
                Some(_) if hide_preview => (Value::Null, "hidden"),
                Some(thread_id) => match answer
                    .titles
                    .get(&(space_ref.to_owned(), thread_id.to_owned()))
                {
                    Some(title) => (json!(title), "ok"),
                    None => (Value::Null, "restricted"),
                },
            };
            json!({
                "item_id": row.get("itemId").cloned().unwrap_or(Value::Null),
                "kind": row.get("kind").cloned().unwrap_or(Value::Null),
                "space_ref": space_ref,
                "space_name": name_of(space_ref),
                "thread_id": thread_id,
                "message_id": str_field(row, "messageId"),
                "run_id": str_field(row, "runId"),
                "actor": actor_json(row),
                "count": row.get("count").cloned().unwrap_or(json!(1)),
                "state": row.get("state").cloned().unwrap_or(Value::Null),
                "snoozed_until": row.get("snoozedUntil").cloned().unwrap_or(Value::Null),
                "created_at": row.get("createdAt").cloned().unwrap_or(Value::Null),
                "updated_at": row.get("updatedAt").cloned().unwrap_or(Value::Null),
                "headline": headline,
                "headline_state": headline_state,
            })
        })
        .collect();

    // Live approvals waiting on the caller: first open page only, newest request last (Session
    // Core sends them oldest first per Space). Their titles take one more call under the same
    // decisions, only for the threads they name.
    let mut needs_you: Vec<Value> = Vec::new();
    if first_open_page && !zdr {
        let mut pending = answer.pending.clone();
        pending.sort_by_key(|approval| {
            approval
                .get("requested_at")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        });
        let mut extra_titles: HashMap<(String, String), String> = HashMap::new();
        if !hide_preview && !pending.is_empty() {
            let mut ids: HashMap<String, Vec<String>> = HashMap::new();
            for approval in &pending {
                let (Some(space_ref), Some(thread_id)) =
                    (str_field(approval, "space_ref"), str_field(approval, "thread_id"))
                else {
                    continue;
                };
                let list = ids.entry(space_ref.to_owned()).or_default();
                if list.len() < HEADLINES_PER_SPACE && !list.iter().any(|seen| seen == thread_id) {
                    list.push(thread_id.to_owned());
                }
            }
            let narrowed: HashMap<String, (String, String)> = decisions
                .iter()
                .filter(|(space_ref, _)| ids.contains_key(space_ref.as_str()))
                .map(|(space_ref, decision)| (space_ref.clone(), decision.clone()))
                .collect();
            match session_attention(&state, &user, &headers, &narrowed, &ids).await {
                Some(found) => extra_titles = found.titles,
                None => {
                    if !gaps.contains(&"session_core") {
                        gaps.push("session_core");
                    }
                }
            }
        }
        for approval in pending.into_iter().take(50) {
            let space_ref = str_field(&approval, "space_ref").unwrap_or_default().to_owned();
            let thread_id = str_field(&approval, "thread_id").map(str::to_owned);
            let (headline, headline_state) = match &thread_id {
                None => (Value::Null, "ok"),
                Some(_) if hide_preview => (Value::Null, "hidden"),
                Some(thread_id) => match extra_titles.get(&(space_ref.clone(), thread_id.clone())) {
                    Some(title) => (json!(title), "ok"),
                    None => (Value::Null, "restricted"),
                },
            };
            let mut entry = approval;
            if let Some(object) = entry.as_object_mut() {
                object.insert("space_name".to_owned(), name_of(&space_ref));
                object.insert("headline".to_owned(), headline);
                object.insert("headline_state".to_owned(), json!(headline_state));
            }
            needs_you.push(entry);
        }
    }

    tracing::info!(
        items = items.len(),
        needs_you = needs_you.len(),
        gaps = gaps.len(),
        "attention items served"
    );
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "items": items,
                "needs_you": needs_you,
                "mode": if zdr { "zdr_refs_only" } else { "live" },
                "gaps": gaps,
                // Mirrored from `meta`: the SPA transport unwraps `data` and drops `meta`.
                "next_cursor": next_cursor,
            },
            "meta": { "next_cursor": next_cursor },
        })),
    )
}

// ── done, reopen, snooze ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemStateBody {
    state: String,
    snooze_until: Option<i64>,
    /// Accepted for the action contract; the write is idempotent by itself.
    #[allow(dead_code, reason = "accepted and ignored: the state write is idempotent")]
    idempotency_key: Option<String>,
}

async fn set_item_state(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(item_id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    set_item_state_core(&state, &user, &item_id, &body).await
}

/// The state write, shared by the route and the `spaces.set_attention_item_state` action.
pub(crate) async fn set_item_state_core(
    state: &AppState,
    user: &AuthenticatedUser,
    item_id: &str,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<ItemStateBody>(body.clone()) else {
        return invalid_request();
    };
    let item_id = item_id.trim();
    if !valid_id(item_id) {
        return refuse(
            StatusCode::NOT_FOUND,
            "attention_item_not_found",
            "That item is not available.",
        );
    }
    if !STATES.contains(&body.state.as_str()) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_state",
            "state must be open, snoozed or done.",
        );
    }
    if body.state != "snoozed" && body.snooze_until.is_some() {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_snooze",
            "snooze_until applies only to a snoozed item.",
        );
    }
    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("itemId".to_owned(), json!(item_id));
    args.insert("state".to_owned(), json!(body.state));
    if let Some(until) = body.snooze_until {
        args.insert("snoozeUntil".to_owned(), json!(until));
    }
    match convex_mutation_message(
        state,
        "spaceAttention:setItemStateForGateway",
        Value::Object(args),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "item_id": value.get("itemId").cloned().unwrap_or_else(|| json!(item_id)),
                "state": value.get("state").cloned().unwrap_or_else(|| json!(body.state)),
                "snoozed_until": value.get("snoozedUntil").cloned().unwrap_or(Value::Null),
            }})),
        ),
        Err(Some(message)) if message.contains("attention_item_not_found") => refuse(
            StatusCode::NOT_FOUND,
            "attention_item_not_found",
            "That item is not available.",
        ),
        Err(Some(message)) if message.contains("invalid_snooze") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_snooze",
            "Snooze for at least a minute and at most 30 days.",
        ),
        Err(Some(message)) if message.contains("invalid_item_state") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_state",
            "state must be open, snoozed or done.",
        ),
        Err(_) => unavailable(),
    }
}

// ── thread follow ────────────────────────────────────────────────────────────────────────────────

/// Organization, ids and the caller's LIVE membership of the Space (Control), in that order.
async fn follow_scope(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    thread_id: &str,
) -> Result<(String, String, String), Reply> {
    let org_id = gate(state, user).await?;
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
    match resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if !role.trim().is_empty() => {}
        Ok(_) => return Err(membership_required()),
        Err(reply) => return Err(reply),
    }
    Ok((org_id, space_ref.to_owned(), thread_id.to_owned()))
}

async fn get_follow(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> Reply {
    let (org_id, _space_ref, thread_id) =
        match follow_scope(&state, &user, &space_ref, &thread_id).await {
            Ok(scope) => scope,
            Err(reply) => return reply,
        };
    match convex_gateway_call(
        &state,
        "query",
        "spaceAttention:getThreadFollowForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "state": value.get("state").cloned().unwrap_or_else(|| json!("none")),
                "reason": value.get("reason").cloned().unwrap_or(Value::Null),
            }})),
        ),
        Err(()) => unavailable(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FollowBody {
    state: String,
}

async fn put_follow(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id)): Path<(String, String)>,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    put_follow_core(&state, &user, Some(&headers), &space_ref, &thread_id, &body).await
}

/// The follow write, shared by the route and the `spaces.set_thread_follow` action.
pub(crate) async fn put_follow_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    space_ref: &str,
    thread_id: &str,
    body: &Value,
) -> Reply {
    let (org_id, space_ref, thread_id) = match follow_scope(state, user, space_ref, thread_id).await
    {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<FollowBody>(body.clone()) else {
        return invalid_request();
    };
    if !FOLLOW_STATES.contains(&body.state.as_str()) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_follow_state",
            "state must be following, muted or none.",
        );
    }
    // A follow is a durable row. Setting one is refused typed for a Zero Data Retention org before
    // any Application call; clearing one removes data and is always allowed.
    if body.state != "none" {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, None).await {
            return refusal;
        }
    }
    match convex_mutation_message(
        state,
        "spaceAttention:setThreadFollowForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
            "state": body.state,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "state": value.get("state").cloned().unwrap_or_else(|| json!(body.state)),
                "reason": value.get("reason").cloned().unwrap_or(Value::Null),
            }})),
        ),
        Err(Some(message)) if message.contains("invalid_follow_state") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_follow_state",
            "state must be following, muted or none.",
        ),
        Err(Some(message)) if message.contains("invalid_thread_id") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        ),
        Err(_) => unavailable(),
    }
}

// ── the cross-room aggregate (WS11-S3) ───────────────────────────────────────────────────────────
//
// `GET /api/v1/spaces/attention?refs=a,b` answers, per room the caller belongs to, what needs
// them: unread threads and the room's latest activity and working runs (Session Core, verifying
// one Control `model.thread.read` decision per room), approvals waiting on them (Session Core,
// live; never stored), and the caller's own read marker, level and open mentions (Application).
// The ONE badge per room is computed here, in `room_badge`, and nowhere else: the SPA draws it
// as given. A source that fails becomes a named gap on the rooms it would have informed and an
// entry of `meta.stale_sources`; it never turns the poll into a 5xx. Only Control's room index
// failing is a 503, because without it nobody knows which rooms exist.

/// Polls per user per minute (plan: 6). A test run shares one process-wide limiter and one user,
/// so the ceiling is lifted there.
const AGGREGATE_PER_MINUTE: f64 = if cfg!(test) { 100_000.0 } else { 6.0 };

/// Process-wide limiter, built on first use from the gateway's cache connection (in-process
/// fallback when it is off). Not an `AppState` field, like the human-post limiter.
static AGGREGATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

const LEVELS: [&str; 3] = ["all", "mentions", "off"];
/// What "Mark all read" may resolve. `needs_you` is live-derived and never touched.
const READ_ALL_KINDS: [&str; 2] = ["mention", "reply"];
const DEFAULT_LEVEL: &str = "mentions";

/// The one badge rule. `n = needs_you + mentions`; any `n` is a count at every level. Otherwise
/// unread decides by level: `all` a muted count, `mentions` a dot, `off` nothing. Unread is never
/// added to `n`, so one thread cannot count twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Badge {
    Count(u32),
    MutedCount(u32),
    Dot,
    Nothing,
}

fn room_badge(level: &str, needs_you: u32, mentions: u32, unread_threads: u32) -> Badge {
    let n = needs_you.saturating_add(mentions);
    if n > 0 {
        return Badge::Count(n);
    }
    if unread_threads == 0 {
        return Badge::Nothing;
    }
    match level {
        "all" => Badge::MutedCount(unread_threads),
        "mentions" => Badge::Dot,
        _ => Badge::Nothing,
    }
}

fn badge_json(badge: Badge) -> Value {
    match badge {
        Badge::Count(n) => json!({ "kind": "count", "n": n }),
        Badge::MutedCount(n) => json!({ "kind": "count", "n": n, "muted": true }),
        Badge::Dot => json!({ "kind": "dot", "n": 0 }),
        Badge::Nothing => json!({ "kind": "none", "n": 0 }),
    }
}

fn count_of(row: &Value, key: &str) -> u32 {
    row.get(key)
        .and_then(Value::as_u64)
        .map_or(0, |n| n.min(u64::from(u32::MAX)) as u32)
}

fn now_ms() -> i64 {
    i64::try_from(unix_nanos() / 1_000_000).unwrap_or(i64::MAX)
}

/// `?refs=a,b` as a deduplicated list; `None` when absent or empty. More than 60 is refused.
fn parse_refs(params: &HashMap<String, String>) -> Result<Option<Vec<String>>, Reply> {
    let Some(raw) = params.get("refs") else {
        return Ok(None);
    };
    let mut refs: Vec<String> = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if !valid_id(part) {
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                "invalid_refs",
                "refs must be Space references separated by commas.",
            ));
        }
        if !refs.iter().any(|seen| seen == part) {
            refs.push(part.to_owned());
        }
    }
    if refs.len() > MAX_SPACES {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_refs",
            "At most 60 Spaces can be asked about at once.",
        ));
    }
    Ok(if refs.is_empty() { None } else { Some(refs) })
}

/// The Space references of Control's actor-filtered index, personal Spaces excluded (they carry
/// no cross-room attention), in Control's order.
fn indexed_refs(index: &[Value]) -> Vec<String> {
    index
        .iter()
        .filter(|entry| str_field(entry, "kind") != Some("personal"))
        .filter_map(|entry| str_field(entry, "space_ref").map(str::to_owned))
        .collect()
}

/// What Session Core counted for one room whose decision verified.
#[derive(Default, Clone)]
struct RoomCounts {
    unread_threads: u32,
    unread_capped: bool,
    latest_activity_ms: i64,
    working: u32,
    needs_you: u32,
}

fn session_counts(
    results: &[Value],
    asked: &HashMap<String, (String, String)>,
) -> HashMap<String, RoomCounts> {
    let mut counts = HashMap::new();
    for result in results {
        // A result for a Space this request did not name is dropped, never trusted.
        let Some(space_ref) = result.get("space_id").and_then(Value::as_str) else {
            continue;
        };
        if !asked.contains_key(space_ref)
            || result.get("status").and_then(Value::as_str) != Some("ok")
        {
            continue;
        }
        counts.insert(
            space_ref.to_owned(),
            RoomCounts {
                unread_threads: count_of(result, "unread_threads"),
                unread_capped: result.get("unread_capped").and_then(Value::as_bool) == Some(true),
                latest_activity_ms: result
                    .get("latest_activity_ms")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                working: count_of(result, "working_runs"),
                needs_you: u32::try_from(
                    result
                        .get("pending")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len),
                )
                .unwrap_or(u32::MAX),
            },
        );
    }
    counts
}

async fn get_attention(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let limiter = AGGREGATE_LIMITER
        .get_or_init(|| RateLimiter::from_cache_with_rpm(&state.cache, AGGREGATE_PER_MINUTE));
    if let Err(retry_after) = limiter
        .try_acquire(&format!("space-attention:{}:{}", org_id.trim(), user.user_id))
        .await
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error_with_details(
                "rate_limited",
                "Attention was asked for too often. Retry shortly.",
                json!({ "retry_after_seconds": retry_after.max(1) }),
            )),
        );
    }
    let requested = match parse_refs(&params) {
        Ok(requested) => requested,
        Err(reply) => return reply,
    };
    let now = now_ms();

    // Zero Data Retention (or a posture that cannot be confirmed, which counts as it): the org
    // keeps nothing durable, so there is nothing to count and no upstream is called.
    if crate::zdr::org_zdr_lookup(&state, &user).await.treat_as_zdr() {
        return (
            StatusCode::OK,
            Json(json!({
                "data": {
                    "generated_at": now,
                    "mode": "zdr_empty",
                    "rooms": [],
                    "totals": { "badge": 0, "unread_rooms": 0, "other_open": 0 },
                    "truncated": false,
                    "stale_sources": [],
                },
                "meta": { "stale_sources": [] },
            })),
        );
    }

    // Control's index decides which rooms exist for this caller; `refs` can only narrow it.
    let Some(index) = control_space_index(&state, &user, &org_id).await else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_index_unavailable",
            "Your rooms could not be listed right now.",
        );
    };
    let member_refs = indexed_refs(&index);
    let mut refs: Vec<String> = match requested {
        Some(wanted) => wanted
            .into_iter()
            .filter(|space_ref| member_refs.contains(space_ref))
            .collect(),
        None => member_refs,
    };
    let truncated = refs.len() > MAX_SPACES;
    refs.truncate(MAX_SPACES);

    // Application: the caller's own marker, level and open mentions.
    let application = if refs.is_empty() {
        Ok(json!({ "rooms": [], "otherOpen": 0 }))
    } else {
        convex_gateway_call(
            &state,
            "query",
            "spaceAttention:attentionForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "spaceRefs": refs,
            }),
        )
        .await
    };
    let application_ok = application.is_ok();
    let application = application.unwrap_or_else(|()| json!({ "rooms": [], "otherOpen": 0 }));
    let mut rows: HashMap<String, &Value> = HashMap::new();
    for row in application
        .get("rooms")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(space_ref) = str_field(row, "spaceRef") {
            rows.insert(space_ref.to_owned(), row);
        }
    }

    // Control: one read decision per room; a room the caller is not entitled to simply has none.
    let (decisions, decisions_ok) = if refs.is_empty() {
        (HashMap::new(), true)
    } else {
        match bulk_read_decisions(&state, &user, &org_id, &refs).await {
            Ok(decisions) => (decisions, true),
            Err(()) => (HashMap::new(), false),
        }
    };

    // Session Core: counts since each room's effective marker (none means nothing is unread).
    let session = if decisions.is_empty() {
        Some(Vec::new())
    } else {
        let spaces: Vec<Value> = refs
            .iter()
            .filter_map(|space_ref| {
                let (decision_ref, token) = decisions.get(space_ref)?;
                let since = rows
                    .get(space_ref)
                    .and_then(|row| row.get("lastReadAt"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .max(0);
                Some(json!({
                    "space_id": space_ref,
                    "decision_ref": decision_ref,
                    "decision_token": token,
                    "since_ms": since,
                    "headline_thread_ids": [],
                }))
            })
            .collect();
        post_session_attention(&state, &user, &headers, spaces).await
    };
    let session_ok = session.is_some();
    let counts = session_counts(session.as_deref().unwrap_or_default(), &decisions);

    let mut rooms: Vec<Value> = Vec::with_capacity(refs.len());
    let mut total_badge: u64 = 0;
    let mut unread_rooms: u32 = 0;
    let mut other_open: u64 = u64::from(count_of(&application, "otherOpen"));
    let mut session_gap = false;
    for space_ref in &refs {
        let row = rows.get(space_ref).copied();
        let level = row
            .and_then(|row| str_field(row, "level"))
            .filter(|level| LEVELS.contains(level))
            .unwrap_or(DEFAULT_LEVEL);
        let mentions = row.map_or(0, |row| count_of(row, "mentions"));
        let counted = counts.get(space_ref);
        let mut gaps: Vec<&'static str> = Vec::new();
        if !application_ok {
            gaps.push("application");
        }
        if !decisions_ok {
            gaps.push("space_decisions");
        } else if !decisions.contains_key(space_ref) {
            // Not entitled to read this room's threads (or no longer): the room is listed
            // without counts and says so.
            gaps.push("read_decision");
        } else if counted.is_none() {
            gaps.push("session_core");
            session_gap = true;
        }
        let counted = counted.cloned().unwrap_or_default();
        let badge = room_badge(level, counted.needs_you, mentions, counted.unread_threads);
        total_badge += u64::from(counted.needs_you) + u64::from(mentions);
        if counted.unread_threads > 0 {
            unread_rooms += 1;
        }
        // Everything open that is not a mention stays out of the room badge and the total.
        if let Some(row) = row {
            other_open += ["replies", "invites", "agentDone", "agentFailed", "reminders"]
                .iter()
                .map(|key| u64::from(count_of(row, key)))
                .sum::<u64>();
        }
        rooms.push(json!({
            "space_ref": space_ref,
            "unread_threads": counted.unread_threads,
            "unread_capped": counted.unread_capped,
            "mentions": mentions,
            "needs_you": counted.needs_you,
            "working": counted.working,
            "last_activity_at": if counted.latest_activity_ms > 0 { json!(counted.latest_activity_ms) } else { Value::Null },
            "last_read_at": row.and_then(|row| row.get("lastReadAt")).cloned().unwrap_or(Value::Null),
            "manual_unread": row.and_then(|row| row.get("manualUnread")).and_then(Value::as_bool) == Some(true),
            "level": level,
            "badge": badge_json(badge),
            "gaps": gaps,
        }));
    }

    let mut stale: Vec<&'static str> = Vec::new();
    if !application_ok {
        stale.push("application");
    }
    if !decisions_ok {
        stale.push("space_decisions");
    }
    if !session_ok || session_gap {
        stale.push("session_core");
    }
    tracing::info!(
        rooms = rooms.len(),
        stale = stale.len(),
        "attention aggregate served"
    );
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "generated_at": now,
                "mode": "live",
                "rooms": rooms,
                "totals": {
                    "badge": total_badge,
                    "unread_rooms": unread_rooms,
                    "other_open": other_open,
                },
                "truncated": truncated,
                // Mirrored from `meta`: the SPA transport unwraps `data` and drops `meta`.
                "stale_sources": stale,
            },
            "meta": { "stale_sources": stale },
        })),
    )
}

// ── read-all ─────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadAllBody {
    space_refs: Option<Vec<String>>,
    kinds: Option<Vec<String>>,
}

async fn read_all(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Bytes,
) -> Reply {
    // An empty body is `{}`: every room, the default kinds.
    let body = if body.iter().all(u8::is_ascii_whitespace) {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => value,
            Err(_) => return invalid_request(),
        }
    };
    read_all_core(&state, &user, &body).await
}

/// "Mark all read", shared by the route and the `spaces.mark_all_read` action. Advances the
/// caller's EXISTING markers (never creates one), clears manual floors and resolves the caller's
/// open mention and reply items in the named rooms. Never touches `needs_you` (derived live).
/// Rooms are Control's index intersected with the request; a marker write is the same one the
/// room makes on every visit, so Zero Data Retention does not change it.
pub(crate) async fn read_all_core(
    state: &AppState,
    user: &AuthenticatedUser,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<ReadAllBody>(body.clone()) else {
        return invalid_request();
    };
    if let Some(kinds) = &body.kinds {
        if kinds
            .iter()
            .any(|kind| !READ_ALL_KINDS.contains(&kind.as_str()))
        {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_kind",
                "kinds can only be mention or reply.",
            );
        }
    }
    if let Some(space_refs) = &body.space_refs {
        if space_refs.len() > MAX_SPACES
            || space_refs
                .iter()
                .any(|space_ref| !valid_id(space_ref.trim()))
        {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_refs",
                "space_refs must hold at most 60 Space references.",
            );
        }
    }
    let Some(index) = control_space_index(state, user, &org_id).await else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_index_unavailable",
            "Your rooms could not be listed right now.",
        );
    };
    let member_refs: Vec<String> = index
        .iter()
        .filter_map(|entry| str_field(entry, "space_ref").map(str::to_owned))
        .collect();
    let target: Vec<String> = match &body.space_refs {
        Some(wanted) => {
            let mut chosen: Vec<String> = Vec::new();
            for space_ref in wanted {
                let space_ref = space_ref.trim();
                if member_refs.iter().any(|member| member == space_ref)
                    && !chosen.iter().any(|seen| seen == space_ref)
                {
                    chosen.push(space_ref.to_owned());
                }
            }
            chosen
        }
        None => member_refs,
    };
    let mut rooms_marked: u64 = 0;
    let mut items_resolved: u64 = 0;
    let mut marked_at: Option<i64> = None;
    for chunk in target.chunks(MAX_SPACES) {
        let mut args = Map::new();
        args.insert("externalAuthId".to_owned(), json!(user.user_id));
        args.insert("externalOrgId".to_owned(), json!(org_id));
        args.insert("spaceRefs".to_owned(), json!(chunk));
        if let Some(kinds) = &body.kinds {
            args.insert("kinds".to_owned(), json!(kinds));
        }
        match convex_mutation_message(
            state,
            "spaceAttention:markAllReadForGateway",
            Value::Object(args),
        )
        .await
        {
            Ok(value) => {
                rooms_marked += u64::from(count_of(&value, "roomsMarked"));
                items_resolved += u64::from(count_of(&value, "itemsResolved"));
                let at = value.get("markedAt").and_then(Value::as_i64);
                marked_at = marked_at.max(at);
            }
            Err(Some(message)) if message.contains("invalid_kind") => {
                return refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_kind",
                    "kinds can only be mention or reply.",
                )
            }
            Err(_) => return unavailable(),
        }
    }
    (
        StatusCode::OK,
        Json(json!({"data": {
            "rooms_marked": rooms_marked,
            "items_resolved": items_resolved,
            "marked_at": marked_at.unwrap_or_else(now_ms),
        }})),
    )
}

// ── per-room notification level ──────────────────────────────────────────────────────────────────

/// Organization and the caller's LIVE membership of the Space (Control), in that order.
async fn room_scope(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    let org_id = gate(state, user).await?;
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    match resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if !role.trim().is_empty() => {}
        Ok(_) => return Err(membership_required()),
        Err(reply) => return Err(reply),
    }
    Ok((org_id, space_ref.to_owned()))
}

fn notify_reply(value: &Value, fallback_level: Option<&str>) -> Reply {
    (
        StatusCode::OK,
        Json(json!({"data": {
            "level": value
                .get("level")
                .cloned()
                .unwrap_or_else(|| json!(fallback_level.unwrap_or(DEFAULT_LEVEL))),
            "hide_preview": value.get("hidePreview").and_then(Value::as_bool) == Some(true),
        }})),
    )
}

async fn get_notify(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> Reply {
    let (org_id, space_ref) = match room_scope(&state, &user, &space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    match convex_gateway_call(
        &state,
        "query",
        "spaceAttention:getRoomLevelForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    {
        Ok(value) => notify_reply(&value, None),
        Err(()) => unavailable(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyBody {
    level: Option<String>,
    hide_preview: Option<bool>,
}

async fn put_notify(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    put_notify_core(&state, &user, &space_ref, &body).await
}

/// The level write, shared by the route and the `spaces.set_notify_level` action. A closed
/// vocabulary and a boolean: no content, so it is allowed under Zero Data Retention.
pub(crate) async fn put_notify_core(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    let (org_id, space_ref) = match room_scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<NotifyBody>(body.clone()) else {
        return invalid_request();
    };
    if body.level.is_none() && body.hide_preview.is_none() {
        return invalid_request();
    }
    if let Some(level) = &body.level {
        if !LEVELS.contains(&level.as_str()) {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_level",
                "level must be all, mentions or off.",
            );
        }
    }
    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("spaceRef".to_owned(), json!(space_ref));
    if let Some(level) = &body.level {
        args.insert("level".to_owned(), json!(level));
    }
    if let Some(hide_preview) = body.hide_preview {
        args.insert("hidePreview".to_owned(), json!(hide_preview));
    }
    match convex_mutation_message(
        state,
        "spaceAttention:setRoomLevelForGateway",
        Value::Object(args),
    )
    .await
    {
        Ok(value) => notify_reply(&value, body.level.as_deref()),
        Err(Some(message)) if message.contains("invalid_level") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_level",
            "level must be all, mentions or off.",
        ),
        Err(_) => unavailable(),
    }
}

// ── mark unread ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnreadBody {
    before_ms: i64,
    /// Accepted for the action contract; the write is idempotent by itself.
    #[allow(dead_code, reason = "accepted and ignored: the floor write is idempotent")]
    idempotency_key: Option<String>,
}

async fn post_unread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    mark_unread_core(&state, &user, &space_ref, &body).await
}

/// "Mark unread", shared by the route and the `spaces.mark_unread` action. Sets the caller's own
/// manual floor so everything newer than `before_ms` reads as unread again. It only ever moves the
/// caller's position backwards, and Application ignores it when the caller has no marker or
/// `before_ms` is not earlier than the effective marker (the answer then shows the unchanged
/// marker). Marker rows hold no content, so Zero Data Retention does not change it.
pub(crate) async fn mark_unread_core(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    let (org_id, space_ref) = match room_scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<UnreadBody>(body.clone()) else {
        return invalid_request();
    };
    if body.before_ms <= 0 {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_before_ms",
            "before_ms must be a time in epoch milliseconds.",
        );
    }
    match convex_mutation_message(
        state,
        "spaceReadMarkers:markUnreadForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "beforeMs": body.before_ms,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "last_read_at": value.get("lastReadAt").cloned().unwrap_or(Value::Null),
                "manual_unread": value.get("manualUnread").and_then(Value::as_bool) == Some(true),
            }})),
        ),
        Err(_) => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    //! WS11-S3.gw tests: the aggregate, read-all, notify level and mark-unread routes through the
    //! real router, with wiremock for auth-core, Control (user-core), org-core, Application
    //! (Convex) and model-gateway. No model is called.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Fx {
        state: AppState,
        _auth: MockServer,
        user_core: MockServer,
        _org: MockServer,
        application: MockServer,
        model_gateway: MockServer,
    }

    /// `zdr`: org-core's answer (`None` is an outage, so an unknown posture). `role`: what Control
    /// says the caller holds in `room-a` (empty is "not a member"). The attention flag is on.
    async fn fx(zdr: Option<bool>, role: &str) -> Fx {
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
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})))
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/room-a/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "room-a", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(match zdr {
                Some(zdr) => ResponseTemplate::new(200).set_body_json(
                    json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
                ),
                None => ResponseTemplate::new(500),
            })
            .mount(&org)
            .await;
        let application = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.model_gateway_url = model_gateway.uri();
        state.space_features.attention_api = true;
        Fx {
            state,
            _auth: auth,
            user_core,
            _org: org,
            application,
            model_gateway,
        }
    }

    async fn mount_index(fx: &Fx, rooms: &[&str]) {
        let mut spaces: Vec<Value> = rooms
            .iter()
            .map(|space_ref| json!({"space_ref": space_ref, "kind": "room"}))
            .collect();
        spaces.push(json!({"space_ref": "personal-1", "kind": "personal"}));
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"spaces": spaces}})))
            .mount(&fx.user_core)
            .await;
    }

    async fn mount_decisions(fx: &Fx, issued: &[&str]) {
        let results: Vec<Value> = issued
            .iter()
            .map(|space_ref| {
                json!({"space_ref": space_ref, "status": "issued",
                       "decision": {"decision_ref": format!("d-{space_ref}")}, "token": format!("t-{space_ref}")})
            })
            .collect();
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decisions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"results": results}})))
            .mount(&fx.user_core)
            .await;
    }

    async fn mount_convex(fx: &Fx, route: &str, function: &str, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .and(body_partial_json(json!({"path": function})))
            .respond_with(template)
            .mount(&fx.application)
            .await;
    }

    fn value(value: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": value}))
    }

    async fn call(fx: &Fx, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload)
    }

    /// Bodies of the Convex calls to one function.
    async fn convex_calls(fx: &Fx, function: &str) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body["path"] == function)
            .collect()
    }

    async fn paths_seen(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.path().to_owned())
            .collect()
    }

    fn room<'a>(body: &'a Value, space_ref: &str) -> &'a Value {
        body["data"]["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|room| room["space_ref"] == space_ref)
            .unwrap_or_else(|| panic!("room {space_ref} in {body}"))
    }

    // ---- the badge rule --------------------------------------------------------------------------

    #[test]
    fn attention_badge_rule_table() {
        // n = needs_you + mentions is a count at EVERY level; unread never joins it
        for level in ["all", "mentions", "off", "unheard-of"] {
            assert_eq!(room_badge(level, 1, 2, 9), Badge::Count(3), "{level}");
            assert_eq!(room_badge(level, 0, 4, 0), Badge::Count(4), "{level}");
            assert_eq!(room_badge(level, 2, 0, 5), Badge::Count(2), "{level}");
            assert_eq!(room_badge(level, 0, 0, 0), Badge::Nothing, "{level}");
        }
        // otherwise unread decides by level
        assert_eq!(room_badge("all", 0, 0, 3), Badge::MutedCount(3));
        assert_eq!(room_badge("mentions", 0, 0, 3), Badge::Dot);
        assert_eq!(room_badge("off", 0, 0, 3), Badge::Nothing);
        assert_eq!(room_badge("unheard-of", 0, 0, 3), Badge::Nothing);
        assert_eq!(room_badge("all", u32::MAX, 5, 0), Badge::Count(u32::MAX), "saturates");
        assert_eq!(badge_json(Badge::Count(3)), json!({"kind": "count", "n": 3}));
        assert_eq!(badge_json(Badge::MutedCount(3)), json!({"kind": "count", "n": 3, "muted": true}));
        assert_eq!(badge_json(Badge::Dot), json!({"kind": "dot", "n": 0}));
        assert_eq!(badge_json(Badge::Nothing), json!({"kind": "none", "n": 0}));
    }

    #[tokio::test]
    async fn the_aggregate_applies_the_badge_rule_and_never_adds_unread_to_the_total() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a", "room-b"]).await;
        mount_decisions(&fx, &["room-a", "room-b"]).await;
        mount_convex(
            &fx,
            "/api/query",
            "spaceAttention:attentionForGateway",
            value(json!({"rooms": [
                {"spaceRef": "room-a", "level": "mentions", "mentions": 1, "lastReadAt": 100, "replies": 4},
                {"spaceRef": "room-b", "level": "all"}
            ], "otherOpen": 2})),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/attention"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [
                {"space_id": "room-a", "status": "ok", "unread_threads": 2, "pending": [{"approval_id": "ap-1"}],
                 "latest_activity_ms": 5, "working_runs": 1},
                {"space_id": "room-b", "status": "ok", "unread_threads": 3, "pending": []},
                {"space_id": "room-not-asked", "status": "ok", "unread_threads": 99, "pending": [{"approval_id": "x"}]}
            ]})))
            .mount(&fx.model_gateway)
            .await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["mode"], "live");
        assert_eq!(room(&body, "room-a")["badge"], json!({"kind": "count", "n": 2}), "needs_you 1 + mentions 1");
        assert_eq!(room(&body, "room-b")["badge"], json!({"kind": "count", "n": 3, "muted": true}));
        assert_eq!(room(&body, "room-a")["working"], 1);
        assert_eq!(room(&body, "room-a")["last_read_at"], 100);
        assert_eq!(body["data"]["totals"]["badge"], 2, "unread (2 + 3) is never in the badge total");
        assert_eq!(body["data"]["totals"]["unread_rooms"], 2);
        assert_eq!(body["data"]["totals"]["other_open"], 6, "otherOpen 2 + room-a replies 4");
        assert!(body["data"]["rooms"].as_array().unwrap().iter().all(|r| r["space_ref"] != "room-not-asked"));
        assert_eq!(body["meta"]["stale_sources"], json!([]));
    }

    // ---- rooms: the Control index, the refs and the cap -------------------------------------------

    #[tokio::test]
    async fn attention_rooms_intersect_control_index_and_cap_60() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a", "room-b"]).await;
        mount_decisions(&fx, &[]).await;
        mount_convex(&fx, "/api/query", "spaceAttention:attentionForGateway", value(json!({"rooms": [], "otherOpen": 0}))).await;
        // refs can only narrow: an unknown ref and a personal Space are dropped
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention?refs=room-a,personal-1,zz,room-a", None).await;
        assert_eq!(status, 200, "{body}");
        let asked = convex_calls(&fx, "spaceAttention:attentionForGateway").await;
        assert_eq!(asked[0]["args"]["spaceRefs"], json!(["room-a"]));
        assert_eq!(body["data"]["rooms"].as_array().unwrap().len(), 1);
        // no refs: the whole index minus personal Spaces, in Control's order
        let (_, body) = call(&fx, "GET", "/api/v1/spaces/attention", None).await;
        let asked = convex_calls(&fx, "spaceAttention:attentionForGateway").await;
        assert_eq!(asked[1]["args"]["spaceRefs"], json!(["room-a", "room-b"]));
        assert_eq!(body["data"]["truncated"], false);
        // more than 60 refs is a 400 before any upstream read of the rooms
        let many: Vec<String> = (0..61).map(|i| format!("r{i}")).collect();
        let (status, body) = call(&fx, "GET", &format!("/api/v1/spaces/attention?refs={}", many.join(",")), None).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_refs");
        let (status, body) = call(&fx, "GET", &format!("/api/v1/spaces/attention?refs={}", "x".repeat(201)), None).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_refs");
    }

    #[tokio::test]
    async fn an_index_of_more_than_60_rooms_is_cut_to_60_and_says_truncated() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        let names: Vec<String> = (0..61).map(|i| format!("room-{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        mount_index(&fx, &refs).await;
        mount_decisions(&fx, &[]).await;
        mount_convex(&fx, "/api/query", "spaceAttention:attentionForGateway", value(json!({"rooms": [], "otherOpen": 0}))).await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["truncated"], true);
        assert_eq!(body["data"]["rooms"].as_array().unwrap().len(), 60);
        let asked = convex_calls(&fx, "spaceAttention:attentionForGateway").await;
        assert_eq!(asked[0]["args"]["spaceRefs"].as_array().unwrap().len(), 60);
        let decisions = fx
            .user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("thread-read-decisions"))
            .expect("a bulk decision request");
        let sent: Value = serde_json::from_slice(&decisions.body).unwrap();
        assert_eq!(sent["items"].as_array().unwrap().len(), 60);
    }

    // ---- degraded sources ---------------------------------------------------------------------------

    #[tokio::test]
    async fn attention_degrades_to_gaps_when_session_core_down() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a", "room-b"]).await;
        mount_decisions(&fx, &["room-a"]).await;
        mount_convex(&fx, "/api/query", "spaceAttention:attentionForGateway", value(json!({"rooms": [{"spaceRef": "room-a", "mentions": 2}], "otherOpen": 0}))).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/attention"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&fx.model_gateway)
            .await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 200, "a failed source is a named gap, never a 5xx: {body}");
        assert_eq!(room(&body, "room-a")["gaps"], json!(["session_core"]));
        assert_eq!(room(&body, "room-b")["gaps"], json!(["read_decision"]), "no decision, no counts");
        assert_eq!(room(&body, "room-a")["mentions"], 2, "Application's own counts still show");
        assert_eq!(body["data"]["stale_sources"], json!(["session_core"]));
        assert_eq!(body["meta"]["stale_sources"], json!(["session_core"]));
    }

    #[tokio::test]
    async fn application_and_control_decision_failures_are_named_gaps_and_the_index_failure_is_a_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx_gaps = fx(Some(false), "editor").await;
        mount_index(&fx_gaps, &["room-a"]).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decisions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&fx_gaps.user_core)
            .await;
        mount_convex(&fx_gaps, "/api/query", "spaceAttention:attentionForGateway", ResponseTemplate::new(500)).await;
        let (status, body) = call(&fx_gaps, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 200);
        let gaps = room(&body, "room-a")["gaps"].clone();
        assert_eq!(gaps, json!(["application", "space_decisions"]));
        assert_eq!(body["meta"]["stale_sources"], json!(["application", "space_decisions"]));

        let down = fx(Some(false), "editor").await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&down.user_core)
            .await;
        let (status, body) = call(&down, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_index_unavailable");
        assert!(paths_seen(&down.model_gateway).await.is_empty());
    }

    // ---- Zero Data Retention -----------------------------------------------------------------------

    #[tokio::test]
    async fn attention_zdr_org_makes_no_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // org-core says ZDR; org-core unreachable is an unknown posture, which counts as ZDR
        for zdr in [Some(true), None] {
            let fx = fx(zdr, "editor").await;
            mount_index(&fx, &["room-a"]).await;
            mount_decisions(&fx, &["room-a"]).await;
            let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention?refs=room-a", None).await;
            assert_eq!(status, 200, "{zdr:?}: {body}");
            assert_eq!(body["data"]["mode"], "zdr_empty");
            assert_eq!(body["data"]["rooms"], json!([]));
            assert_eq!(body["data"]["totals"], json!({"badge": 0, "unread_rooms": 0, "other_open": 0}));
            let control = paths_seen(&fx.user_core).await;
            assert!(!control.iter().any(|path| path.starts_with("/api/v1/internal/spaces")), "{zdr:?}: Control untouched: {control:?}");
            assert!(paths_seen(&fx.application).await.is_empty(), "{zdr:?}: no Convex request");
            assert!(paths_seen(&fx.model_gateway).await.is_empty(), "{zdr:?}: no model-gateway request");
        }
    }

    // ---- no titles without a decision --------------------------------------------------------------

    #[tokio::test]
    async fn attention_never_returns_titles_without_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a", "room-b"]).await;
        mount_decisions(&fx, &["room-a"]).await;
        mount_convex(&fx, "/api/query", "spaceAttention:attentionForGateway", value(json!({"rooms": [], "otherOpen": 0}))).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/attention"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [
                {"space_id": "room-a", "status": "ok", "unread_threads": 1, "pending": [],
                 "headlines": [{"thread_id": "t-1", "title": "A secret plan"}]}
            ]})))
            .mount(&fx.model_gateway)
            .await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/attention", None).await;
        assert_eq!(status, 200);
        let sent = fx.model_gateway.received_requests().await.unwrap();
        assert_eq!(sent.len(), 1);
        let sent: Value = serde_json::from_slice(&sent[0].body).unwrap();
        let spaces = sent["spaces"].as_array().unwrap();
        assert_eq!(spaces.len(), 1, "only the room with a decision is asked about");
        assert_eq!(spaces[0]["space_id"], "room-a");
        assert_eq!(spaces[0]["headline_thread_ids"], json!([]), "the aggregate asks for no titles");
        assert_eq!(spaces[0]["decision_ref"], "d-room-a");
        assert!(!body.to_string().contains("A secret plan"), "no title in the aggregate answer");
        assert!(!body.to_string().contains("title"));
        assert_eq!(room(&body, "room-b")["gaps"], json!(["read_decision"]));
    }

    // ---- the rate limit ----------------------------------------------------------------------------

    /// The aggregate ceiling is lifted under `cfg!(test)` (one process-wide bucket and one test
    /// user), so the 429 envelope itself cannot be driven here without changing production code.
    /// This pins the limiter the route uses at the plan's six polls a minute and its Retry-After.
    #[tokio::test]
    async fn attention_rate_limits_six_per_minute_with_retry_after() {
        let state = crate::tests::test_state(false);
        let limiter = RateLimiter::from_cache_with_rpm(&state.cache, 6.0);
        for poll in 0..6 {
            assert!(limiter.try_acquire("space-attention:org-1:user-1").await.is_ok(), "poll {poll}");
        }
        let retry_after = limiter.try_acquire("space-attention:org-1:user-1").await.expect_err("the seventh poll");
        assert!(retry_after >= 1);
        assert!(limiter.try_acquire("space-attention:org-1:user-2").await.is_ok(), "own bucket per user");
        assert_eq!(MAX_SPACES, 60);
    }

    // ---- read-all ----------------------------------------------------------------------------------

    #[tokio::test]
    async fn read_all_never_creates_markers_or_touches_needs_you() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a", "room-b"]).await;
        mount_convex(&fx, "/api/mutation", "spaceAttention:markAllReadForGateway", value(json!({"roomsMarked": 1, "itemsResolved": 3, "markedAt": 777}))).await;
        // only the index-intersected refs reach the mutation (an unknown ref is dropped, a duplicate collapses)
        let (status, body) = call(
            &fx,
            "POST",
            "/api/v1/spaces/attention/read-all",
            Some(json!({"space_refs": ["room-a", "personal-1", "zz", "room-a"], "kinds": ["mention"]})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"], json!({"rooms_marked": 1, "items_resolved": 3, "marked_at": 777}));
        let calls = convex_calls(&fx, "spaceAttention:markAllReadForGateway").await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["args"]["spaceRefs"], json!(["room-a", "personal-1"]));
        assert_eq!(calls[0]["args"]["kinds"], json!(["mention"]));
        assert_eq!(calls[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(calls[0]["args"]["externalOrgId"], "org-1");
        // an empty body means every room of the index (membership of the caller), default kinds
        let (status, _) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", None).await;
        assert_eq!(status, 200);
        let calls = convex_calls(&fx, "spaceAttention:markAllReadForGateway").await;
        assert_eq!(calls[1]["args"]["spaceRefs"], json!(["room-a", "room-b", "personal-1"]));
        assert!(calls[1]["args"].get("kinds").is_none());
        // nothing else was read: no decision, no model-gateway request, no aggregate query
        assert!(paths_seen(&fx.model_gateway).await.is_empty());
        assert!(!paths_seen(&fx.user_core).await.iter().any(|path| path.ends_with("thread-read-decisions")));
        assert!(paths_seen(&fx.application).await.iter().all(|path| path == "/api/mutation"));
    }

    #[tokio::test]
    async fn read_all_sends_chunks_of_60_and_sums_the_answers() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        let names: Vec<String> = (0..130).map(|i| format!("room-{i:03}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        mount_index(&fx, &refs).await;
        mount_convex(&fx, "/api/mutation", "spaceAttention:markAllReadForGateway", value(json!({"roomsMarked": 2, "itemsResolved": 1, "markedAt": 50}))).await;
        let (status, body) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", Some(json!({}))).await;
        assert_eq!(status, 200, "{body}");
        let lens: Vec<usize> = convex_calls(&fx, "spaceAttention:markAllReadForGateway")
            .await
            .iter()
            .map(|call| call["args"]["spaceRefs"].as_array().unwrap().len())
            .collect();
        assert_eq!(lens, [60, 60, 11], "130 rooms and the personal Space");
        assert_eq!(body["data"]["rooms_marked"], 6);
        assert_eq!(body["data"]["items_resolved"], 3);
        assert_eq!(body["data"]["marked_at"], 50);
    }

    #[tokio::test]
    async fn read_all_refuses_other_kinds_bad_bodies_and_the_flag_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "editor").await;
        mount_index(&fx, &["room-a"]).await;
        mount_convex(&fx, "/api/mutation", "spaceAttention:markAllReadForGateway", value(json!({"roomsMarked": 0, "itemsResolved": 0}))).await;
        for kinds in [json!(["invite"]), json!(["mention", "reminder"]), json!(["needs_you"])] {
            let (status, body) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", Some(json!({"kinds": kinds}))).await;
            assert_eq!(status, 400, "{kinds}");
            assert_eq!(body["error"]["code"], "invalid_kind");
        }
        let (status, body) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", Some(json!({"org_id": "forged"}))).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_request");
        let (status, body) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", Some(json!({"space_refs": ["x".repeat(201)]}))).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_refs");
        let too_many: Vec<String> = (0..61).map(|i| format!("r{i}")).collect();
        let (status, _) = call(&fx, "POST", "/api/v1/spaces/attention/read-all", Some(json!({"space_refs": too_many}))).await;
        assert_eq!(status, 400);
        assert!(convex_calls(&fx, "spaceAttention:markAllReadForGateway").await.is_empty());

        let mut off = fx;
        off.state.space_features.attention_api = false;
        let (status, body) = call(&off, "POST", "/api/v1/spaces/attention/read-all", Some(json!({}))).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_attention_disabled");
        for (method, uri) in [("GET", "/api/v1/spaces/attention"), ("GET", "/api/v1/spaces/room-a/notify"), ("POST", "/api/v1/spaces/room-a/unread")] {
            let (status, body) = call(&off, method, uri, if method == "POST" { Some(json!({"before_ms": 5})) } else { None }).await;
            assert_eq!(status, 404, "{uri}");
            assert_eq!(body["error"]["code"], "space_attention_disabled");
        }
    }

    // ---- notify level ------------------------------------------------------------------------------

    #[tokio::test]
    async fn notify_get_put_and_invalid_level() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "viewer").await;
        mount_convex(&fx, "/api/query", "spaceAttention:getRoomLevelForGateway", value(json!({"level": "all", "hidePreview": true}))).await;
        mount_convex(&fx, "/api/mutation", "spaceAttention:setRoomLevelForGateway", value(json!({"level": "off", "hidePreview": false}))).await;
        let (status, body) = call(&fx, "GET", "/api/v1/spaces/room-a/notify", None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"], json!({"level": "all", "hide_preview": true}));
        let (status, body) = call(&fx, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"level": "off"}))).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"], json!({"level": "off", "hide_preview": false}));
        let writes = convex_calls(&fx, "spaceAttention:setRoomLevelForGateway").await;
        assert_eq!(writes[0]["args"]["level"], "off");
        assert_eq!(writes[0]["args"]["spaceRef"], "room-a");
        assert_eq!(writes[0]["args"]["externalAuthId"], "user-1");
        assert!(writes[0]["args"].get("hidePreview").is_none());
        let (_, _) = call(&fx, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"hide_preview": true}))).await;
        let writes = convex_calls(&fx, "spaceAttention:setRoomLevelForGateway").await;
        assert_eq!(writes[1]["args"]["hidePreview"], true);
        assert!(writes[1]["args"].get("level").is_none());

        for (body, code) in [
            (json!({"level": "loud"}), "invalid_level"),
            (json!({"level": ""}), "invalid_level"),
            (json!({}), "invalid_request"),
            (json!({"level": "all", "org_id": "forged"}), "invalid_request"),
            (json!({"level": 3}), "invalid_request"),
        ] {
            let (status, got) = call(&fx, "PUT", "/api/v1/spaces/room-a/notify", Some(body.clone())).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], code, "{body}");
        }
        assert_eq!(convex_calls(&fx, "spaceAttention:setRoomLevelForGateway").await.len(), 2, "no refused write reached Convex");
    }

    #[tokio::test]
    async fn notify_needs_a_live_membership_and_is_allowed_under_zdr() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let stranger = fx(Some(false), "").await;
        mount_convex(&stranger, "/api/query", "spaceAttention:getRoomLevelForGateway", value(json!({"level": "all"}))).await;
        mount_convex(&stranger, "/api/mutation", "spaceAttention:setRoomLevelForGateway", value(json!({"level": "all"}))).await;
        let (status, body) = call(&stranger, "GET", "/api/v1/spaces/room-a/notify", None).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "active_membership_required");
        let (status, _) = call(&stranger, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"level": "all"}))).await;
        assert_eq!(status, 403);
        let (status, _) = call(&stranger, "POST", "/api/v1/spaces/room-a/unread", Some(json!({"before_ms": 5}))).await;
        assert_eq!(status, 403);
        assert!(paths_seen(&stranger.application).await.is_empty(), "a non-member reaches no Convex function");

        // a level is a closed word and a boolean: no content, so a ZDR organization may write it
        let zdr = fx(Some(true), "viewer").await;
        mount_convex(&zdr, "/api/mutation", "spaceAttention:setRoomLevelForGateway", value(json!({"level": "mentions", "hidePreview": false}))).await;
        mount_convex(&zdr, "/api/mutation", "spaceReadMarkers:markUnreadForGateway", value(json!({"lastReadAt": 1, "manualUnread": true}))).await;
        let (status, body) = call(&zdr, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"level": "mentions"}))).await;
        assert_eq!(status, 200, "{body}");
        let (status, _) = call(&zdr, "POST", "/api/v1/spaces/room-a/unread", Some(json!({"before_ms": 5}))).await;
        assert_eq!(status, 200);
    }

    // ---- mark unread -------------------------------------------------------------------------------

    #[tokio::test]
    async fn unread_posts_floor_and_rejects_bad_before_ms() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "viewer").await;
        mount_convex(&fx, "/api/mutation", "spaceReadMarkers:markUnreadForGateway", value(json!({"lastReadAt": 1_699_999_999_000_i64, "manualUnread": true}))).await;
        let (status, body) = call(
            &fx,
            "POST",
            "/api/v1/spaces/room-a/unread",
            Some(json!({"before_ms": 1_700_000_000_000_i64, "idempotency_key": "k-1"})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"], json!({"last_read_at": 1_699_999_999_000_i64, "manual_unread": true}));
        let writes = convex_calls(&fx, "spaceReadMarkers:markUnreadForGateway").await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["args"]["beforeMs"], 1_700_000_000_000_i64);
        assert_eq!(writes[0]["args"]["spaceRef"], "room-a");
        assert_eq!(writes[0]["args"]["externalOrgId"], "org-1");
        assert!(writes[0]["args"].get("idempotencyKey").is_none(), "the key is accepted and ignored");

        for before_ms in [json!(0), json!(-5)] {
            let (status, got) = call(&fx, "POST", "/api/v1/spaces/room-a/unread", Some(json!({"before_ms": before_ms}))).await;
            assert_eq!(status, 400, "{before_ms}");
            assert_eq!(got["error"]["code"], "invalid_before_ms");
        }
        for body in [json!({}), json!({"before_ms": "soon"}), json!({"before_ms": 5, "org_id": "forged"}), json!({"before_ms": 5, "last_read_at": 1})] {
            let (status, got) = call(&fx, "POST", "/api/v1/spaces/room-a/unread", Some(body.clone())).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], "invalid_request", "forged fields are rejected by deny_unknown_fields: {body}");
        }
        assert_eq!(convex_calls(&fx, "spaceReadMarkers:markUnreadForGateway").await.len(), 1);
    }

    #[tokio::test]
    async fn a_convex_invalid_level_message_is_a_400_and_any_other_failure_a_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let bad = fx(Some(false), "viewer").await;
        mount_convex(&bad, "/api/mutation", "spaceAttention:setRoomLevelForGateway", ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": "Uncaught Error: invalid_level"}))).await;
        let (status, body) = call(&bad, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"level": "all"}))).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_level");
        let down = fx(Some(false), "viewer").await;
        mount_convex(&down, "/api/mutation", "spaceAttention:setRoomLevelForGateway", ResponseTemplate::new(500)).await;
        let (status, body) = call(&down, "PUT", "/api/v1/spaces/room-a/notify", Some(json!({"level": "all"}))).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "attention_unavailable");
    }
}
