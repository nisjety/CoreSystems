//! The attention inbox: the BFF half of WS11-S7.
//!
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

use axum::{
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
    envelope::{error, unwrap_data},
    middleware::AuthenticatedUser,
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
        .route("/api/v1/spaces/attention/items", get(list_items))
        .route(
            "/api/v1/spaces/attention/items/{item_id}/state",
            post(set_item_state),
        )
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/follow",
            get(get_follow).put(put_follow),
        )
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

/// `POST /v1/spaces/attention` through model-gateway: decisions travel in the body only. `None`
/// when the bearers or the upstream are unavailable, which the caller names as a gap.
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
    let session_token =
        crate::domains::chat::shared::required_session_token(state, user, headers)
            .await
            .ok()?;
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
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
    let results = response.get("results").and_then(Value::as_array)?;
    let mut answer = SessionAnswer::default();
    for result in results {
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
