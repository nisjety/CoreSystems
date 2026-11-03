//! WS04-S3: people directory search for the room member picker.
//!
//! `GET /api/v1/directory/people?q=&limit=&cursor=&exclude_member_of=` proxies Control's
//! `GET /api/v1/internal/directory/people` under the caller's signed delegation. The organization
//! and the caller come from the session, never from the query string; Control runs the single
//! SQL join of `users` and `user_org_memberships` and decides what the caller may see. This module
//! holds no directory state. It carries ids, names and emails only: no message content, so there
//! is nothing for Zero Data Retention to refuse and no model is called.
//!
//! Gate order: flag (404 `feature_disabled`), live organization membership, per-user limiter
//! (60 per minute, 429 `rate_limited` with `Retry-After`), query shape (400 `invalid_request`),
//! then Control. WS04-S9 adds `kinds=person,agent` (composed people and agents) and
//! `POST /api/v1/directory/resolve`, both behind `SPACE_DIRECTORY_RESOLVER_API` and both marked
//! `meta.presence_only` because the agent half is a presence registry, never an authority. Control's typed 4xx passes through unchanged; a Control outage is a typed 503
//! `directory_unavailable`, never an empty list.

use std::sync::OnceLock;

use axum::{
    extract::{Extension, Query, State},
    http::{header::RETRY_AFTER, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::proxy_json,
};

/// Directory reads per user per minute (plan: 60/min).
const DIRECTORY_READS_PER_MINUTE: f64 = 60.0;
const MAX_QUERY_CHARS: usize = 100;
const MAX_LIMIT: u32 = 50;
const MAX_CURSOR_LEN: usize = 512;
const MAX_SPACE_REF_LEN: usize = 200;

/// Process-wide limiter, built on first use from the gateway's Dragonfly connection (in-process
/// fallback when the cache is off). It is not an `AppState` field so that adding it does not
/// touch every test state literal.
static DIRECTORY_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

#[derive(Debug, Default, Deserialize)]
pub(super) struct PeopleParams {
    q: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    exclude_member_of: Option<String>,
    /// WS04-S9: `person`, `agent` or `person,agent`; absent keeps the S3 people-only shape.
    /// Never forwarded to Control.
    kinds: Option<String>,
}

fn invalid(field: &'static str, reason: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_request",
            "The directory query is invalid.",
            json!({ "field": field, "reason": reason }),
        )),
    )
        .into_response()
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "directory_unavailable",
            "The directory is unavailable.",
        )),
    )
        .into_response()
}

/// Query string for Control, from the validated parameters only (unknown parameters are dropped,
/// so a client cannot smuggle `org_id` or a caller id through).
fn control_query(params: &PeopleParams) -> Result<String, Response> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = params.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        if q.chars().count() > MAX_QUERY_CHARS {
            return Err(invalid("q", "too_long"));
        }
        parts.push(format!("q={}", urlencoding::encode(q)));
    }
    if let Some(raw) = params
        .limit
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => parts.push(format!("limit={limit}")),
            _ => return Err(invalid("limit", "out_of_range")),
        }
    }
    if let Some(cursor) = params
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
    {
        if cursor.len() > MAX_CURSOR_LEN {
            return Err(invalid("cursor", "too_long"));
        }
        parts.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    if let Some(space_ref) = params
        .exclude_member_of
        .as_deref()
        .map(str::trim)
        .filter(|space_ref| !space_ref.is_empty())
    {
        if space_ref.len() > MAX_SPACE_REF_LEN {
            return Err(invalid("exclude_member_of", "too_long"));
        }
        parts.push(format!("exclude_member_of={}", urlencoding::encode(space_ref)));
    }
    Ok(parts.join("&"))
}

/// The four fields the picker needs, nothing else Control might add later.
fn person(value: &Value) -> Option<Value> {
    let subject_id = value.get("subject_id").and_then(Value::as_str)?;
    Some(json!({
        "subject_id": subject_id,
        "display_name": value.get("display_name").and_then(Value::as_str).unwrap_or_default(),
        "email": value.get("email").and_then(Value::as_str).unwrap_or_default(),
        "is_self": value.get("is_self").and_then(Value::as_bool).unwrap_or(false),
    }))
}

/// `GET /api/v1/directory/people`.
///
/// Without `kinds` this is the WS04-S3 people search (`{people, next_cursor}`). With `kinds`
/// (WS04-S9, flag `directory_resolver_api`) it answers the composed directory
/// `{results:[person|agent], next_cursor}` with `meta.presence_only:true`.
pub(super) async fn people_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<PeopleParams>,
) -> Response {
    if !state.space_features.people_search_api {
        return feature_disabled("People search is not enabled.");
    }
    if params.kinds.is_some() && !state.space_features.directory_resolver_api {
        return feature_disabled("The composed directory is not enabled.");
    }
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return membership_required();
    }
    if let Some(response) = rate_limited(&state, &org_id, &user).await {
        return response;
    }
    let query = match control_query(&params) {
        Ok(query) => query,
        Err(response) => return response,
    };
    let kinds = match params.kinds.as_deref().map(parse_kinds).transpose() {
        Ok(kinds) => kinds,
        Err(response) => return response,
    };
    if let Some(kinds) = kinds {
        return composed_search(&state, &user, &org_id, kinds, &params, &query).await;
    }
    let data = match control_people(&state, &user, &org_id, &query).await {
        Ok(data) => data,
        Err(failure) => return failure.into_response(),
    };
    let people: Vec<Value> = data
        .get("people")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(person).collect())
        .unwrap_or_default();
    let mut response = (
        StatusCode::OK,
        Json(json!({ "data": { "people": people, "next_cursor": next_cursor(&data) } })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn feature_disabled(message: &'static str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error("feature_disabled", message)),
    )
        .into_response()
}

fn membership_required() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "active_membership_required",
            "An active organization membership is required.",
        )),
    )
        .into_response()
}

/// Per-user directory read limiter, shared by the search and the resolver (60 per minute).
async fn rate_limited(state: &AppState, org_id: &str, user: &AuthenticatedUser) -> Option<Response> {
    let limiter = DIRECTORY_LIMITER.get_or_init(|| {
        RateLimiter::from_cache_with_rpm(&state.cache, DIRECTORY_READS_PER_MINUTE)
    });
    let retry_after = limiter
        .try_acquire(&format!("space-directory:{}:{}", org_id.trim(), user.user_id))
        .await
        .err()?;
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(error(
            "rate_limited",
            "Too many directory searches. Retry shortly.",
        )),
    )
        .into_response();
    if let Ok(value) = HeaderValue::try_from(retry_after.max(1).to_string()) {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    Some(response)
}

fn next_cursor(data: &Value) -> Value {
    data.get("next_cursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map_or(Value::Null, |cursor| json!(cursor))
}

/// Why a Control directory read produced no data.
enum ControlFailure {
    /// 5xx or unreachable: a typed 503, never an empty list.
    Unavailable,
    /// Control's typed 4xx (invalid_request, member_not_in_organization, delegation) as is.
    Passthrough(StatusCode, Value),
}

impl ControlFailure {
    fn into_response(self) -> Response {
        match self {
            Self::Unavailable => unavailable(),
            Self::Passthrough(status, body) => (status, Json(body)).into_response(),
        }
    }
}

/// One Control directory read under the caller's signed delegation; returns the unwrapped
/// `{people, next_cursor}` data.
async fn control_people(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    query: &str,
) -> Result<Value, ControlFailure> {
    let mut url = format!("{}/api/v1/internal/directory/people", state.user_core_url);
    if !query.is_empty() {
        url.push('?');
        url.push_str(query);
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if status.is_server_error() {
        return Err(ControlFailure::Unavailable);
    }
    if !status.is_success() {
        return Err(ControlFailure::Passthrough(status, control));
    }
    Ok(unwrap_data(&control))
}

// ---------------------------------------------------------------------------------------------
// WS04-S9: composed people and agent directory, and the resolver.
//
// Presence, not authority (ADR-0002): the agent half comes from Application's cross-Space
// registry, which answers "what is installed" for any organization member. Nothing here lets a
// caller act on a person or an agent; acting still goes through Control's room roster. An agent
// row never carries the name of a room (only `space_ref`s of rooms the caller is already in), so
// the directory cannot be used to learn that another room exists or what it is called.
// ---------------------------------------------------------------------------------------------

/// Agents returned on the first page of a composed search.
const MAX_AGENT_RESULTS: usize = 10;
/// Candidates named in an `ambiguous` resolver answer.
const MAX_CANDIDATES: usize = 5;
/// People fetched from Control per resolver read (Control's own cap).
const RESOLVE_PEOPLE_LIMIT: u32 = 50;

#[derive(Clone, Copy)]
struct Kinds {
    person: bool,
    agent: bool,
}

/// `kinds=person,agent` (either, both; case-insensitive; unknown values are a 400).
fn parse_kinds(raw: &str) -> Result<Kinds, Response> {
    let mut kinds = Kinds { person: false, agent: false };
    for part in raw.split(',').map(|part| part.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "person" => kinds.person = true,
            "agent" => kinds.agent = true,
            _ => return Err(invalid("kinds", "unknown_kind")),
        }
    }
    Ok(kinds)
}

/// One org-installed published agent definition, with the rooms the CALLER is in that hold it.
struct AgentRow {
    agent_ref: String,
    name: String,
    description: String,
    /// Rooms (by `space_ref`) among the caller's own that have an active or paused binding.
    installed_in: Vec<String>,
}

/// Published agent definitions of the organization from Application's registry, grouped by
/// definition. `installed_in` is limited to the caller's rooms from Control's actor-filtered
/// index; when that index is unavailable the list degrades to no rooms, never to someone else's.
async fn load_agents(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<Vec<AgentRow>, ()> {
    let value = super::convex_gateway_call(
        state,
        "query",
        "spaceAgents:agentInstallationsForOrgForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await?;
    let bindings = value.as_array().cloned().unwrap_or_default();
    let mut rows: std::collections::BTreeMap<String, (String, String, Vec<String>)> =
        std::collections::BTreeMap::new();
    for binding in &bindings {
        let Some(agent_ref) = binding.get("agentRef").and_then(Value::as_str) else {
            continue;
        };
        // Published definitions only: `active` is the published state of an agent definition.
        if binding.get("definitionStatus").and_then(Value::as_str) != Some("active") {
            continue;
        }
        let entry = rows.entry(agent_ref.to_owned()).or_insert_with(|| {
            (
                binding.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                binding.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
                Vec::new(),
            )
        });
        let live = matches!(
            binding.get("status").and_then(Value::as_str),
            Some("active" | "paused")
        );
        if let (true, Some(space_ref)) = (live, binding.get("spaceRef").and_then(Value::as_str)) {
            if !entry.2.iter().any(|existing| existing == space_ref) {
                entry.2.push(space_ref.to_owned());
            }
        }
    }
    let mut rows: Vec<AgentRow> = rows
        .into_iter()
        .filter(|(_, (name, _, _))| !name.trim().is_empty())
        .map(|(agent_ref, (name, description, installed_in))| AgentRow {
            agent_ref,
            name,
            description,
            installed_in,
        })
        .collect();
    if rows.iter().any(|row| !row.installed_in.is_empty()) {
        let mine: std::collections::HashSet<String> = super::control_space_index(state, user, org_id)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| entry.get("space_ref").and_then(Value::as_str).map(str::to_owned))
            .collect();
        for row in &mut rows {
            row.installed_in.retain(|space_ref| mine.contains(space_ref));
        }
    }
    Ok(rows)
}

fn agent_json(row: &AgentRow) -> Value {
    json!({
        "kind": "agent",
        "agent_ref": row.agent_ref,
        "display_name": row.name,
        "description": row.description,
        "installed_in": row.installed_in,
    })
}

fn person_with_kind(value: &Value) -> Option<Value> {
    let mut person = person(value)?;
    person["kind"] = json!("person");
    Some(person)
}

/// The composed search: people from Control (paged), then published agents (first page only,
/// filtered by `q`, at most [`MAX_AGENT_RESULTS`]).
async fn composed_search(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    kinds: Kinds,
    params: &PeopleParams,
    query: &str,
) -> Response {
    let mut results: Vec<Value> = Vec::new();
    let mut cursor = Value::Null;
    if kinds.person {
        match control_people(state, user, org_id, query).await {
            Ok(data) => {
                results.extend(
                    data.get("people")
                        .and_then(Value::as_array)
                        .map(|rows| rows.iter().filter_map(person_with_kind).collect::<Vec<_>>())
                        .unwrap_or_default(),
                );
                cursor = next_cursor(&data);
            }
            Err(failure) => return failure.into_response(),
        }
    }
    let first_page = params
        .cursor
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty);
    if kinds.agent && first_page {
        let Ok(agents) = load_agents(state, user, org_id).await else {
            return unavailable();
        };
        let needle = params.q.as_deref().map(str::trim).unwrap_or_default().to_lowercase();
        results.extend(
            agents
                .iter()
                .filter(|row| {
                    needle.is_empty()
                        || row.name.to_lowercase().contains(&needle)
                        || row.description.to_lowercase().contains(&needle)
                })
                .take(MAX_AGENT_RESULTS)
                .map(agent_json),
        );
    }
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "data": { "results": results, "next_cursor": cursor },
            "meta": { "presence_only": true },
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

/// A resolver candidate: a person or an agent, with whether it is in the scoping room.
struct Candidate {
    json: Value,
    name: String,
    email: String,
    in_room: bool,
}

impl Candidate {
    fn exact(&self, needle: &str) -> bool {
        self.name.to_lowercase() == needle || (!self.email.is_empty() && self.email.to_lowercase() == needle)
    }

    fn prefix(&self, needle: &str) -> bool {
        self.name.to_lowercase().starts_with(needle)
            || (!self.email.is_empty() && self.email.to_lowercase().starts_with(needle))
    }
}

/// The best non-empty tier: exact email or full name, else prefix of name or email.
fn best_tier<'a>(candidates: &[&'a Candidate], needle: &str) -> Vec<&'a Candidate> {
    let exact: Vec<&Candidate> = candidates.iter().copied().filter(|c| c.exact(needle)).collect();
    if !exact.is_empty() {
        return exact;
    }
    candidates.iter().copied().filter(|c| c.prefix(needle)).collect()
}

fn resolve_response(data: Value) -> Response {
    let mut response = (
        StatusCode::OK,
        Json(json!({ "data": data, "meta": { "presence_only": true } })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn resolve_none(reason: &'static str) -> Response {
    resolve_response(json!({ "status": "none", "reason": reason }))
}

/// `POST /api/v1/directory/resolve` `{query, kind:"person"|"agent"|"any", space_ref?}`.
///
/// Exact (case-insensitive) email or full name, else a unique prefix, is `ok`; several in the
/// best tier is `ambiguous` (at most five candidates); otherwise `none` with a reason. With a
/// `space_ref` the answer is scoped to that room's members, but only when the caller is a member
/// of it (otherwise the room is ignored, so the route cannot probe a room's roster); a match
/// outside the room is then `none` / `not_in_room`. The response is presence, never authority.
pub(super) async fn resolve_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    if !state.space_features.directory_resolver_api {
        return feature_disabled("The directory resolver is not enabled.");
    }
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return membership_required();
    }
    if let Some(response) = rate_limited(&state, &org_id, &user).await {
        return response;
    }
    let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let query = text("query");
    if query.is_empty() {
        return invalid("query", "required");
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return invalid("query", "too_long");
    }
    let kinds = match text("kind") {
        "" | "any" => Kinds { person: true, agent: true },
        "person" => Kinds { person: true, agent: false },
        "agent" => Kinds { person: false, agent: true },
        _ => return invalid("kind", "unknown_kind"),
    };
    let space_ref = text("space_ref");
    if space_ref.len() > MAX_SPACE_REF_LEN {
        return invalid("space_ref", "too_long");
    }
    let needle = query.to_lowercase();

    // The room scopes the answer only when the caller is in it.
    let scope: Option<&str> = if space_ref.is_empty() {
        None
    } else {
        match super::control_space_index(&state, &user, &org_id).await {
            Some(index) => index
                .iter()
                .any(|entry| entry.get("space_ref").and_then(Value::as_str) == Some(space_ref))
                .then_some(space_ref),
            None => return resolve_none("directory_unavailable"),
        }
    };

    let mut candidates: Vec<Candidate> = Vec::new();
    if kinds.person {
        let base = format!(
            "q={}&limit={RESOLVE_PEOPLE_LIMIT}",
            urlencoding::encode(query)
        );
        let everyone = match control_people(&state, &user, &org_id, &base).await {
            Ok(data) => data,
            Err(ControlFailure::Unavailable) => return resolve_none("directory_unavailable"),
            Err(failure) => return failure.into_response(),
        };
        // Same query and order with the room's members removed: whoever is missing from it is
        // in the room (a person ranked within the first 50 outside the room is within the first
        // 50 here too).
        let outside: Option<std::collections::HashSet<String>> = match scope {
            None => None,
            Some(room) => {
                let scoped = format!("{base}&exclude_member_of={}", urlencoding::encode(room));
                match control_people(&state, &user, &org_id, &scoped).await {
                    Ok(data) => Some(
                        data.get("people")
                            .and_then(Value::as_array)
                            .map(|rows| {
                                rows.iter()
                                    .filter_map(|row| row.get("subject_id").and_then(Value::as_str))
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default(),
                    ),
                    Err(ControlFailure::Unavailable) => {
                        return resolve_none("directory_unavailable");
                    }
                    Err(failure) => return failure.into_response(),
                }
            }
        };
        for row in everyone
            .get("people")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(json) = person_with_kind(row) else { continue };
            let subject_id = json["subject_id"].as_str().unwrap_or_default().to_owned();
            candidates.push(Candidate {
                name: json["display_name"].as_str().unwrap_or_default().to_owned(),
                email: json["email"].as_str().unwrap_or_default().to_owned(),
                in_room: outside.as_ref().is_none_or(|set| !set.contains(&subject_id)),
                json,
            });
        }
    }
    if kinds.agent {
        let Ok(agents) = load_agents(&state, &user, &org_id).await else {
            return resolve_none("directory_unavailable");
        };
        for row in &agents {
            candidates.push(Candidate {
                json: agent_json(row),
                name: row.name.clone(),
                email: String::new(),
                in_room: scope.is_none_or(|room| row.installed_in.iter().any(|r| r == room)),
            });
        }
    }

    let inside: Vec<&Candidate> = candidates.iter().filter(|c| c.in_room).collect();
    let matches = best_tier(&inside, &needle);
    match matches.as_slice() {
        [] => {
            let outside: Vec<&Candidate> = candidates.iter().filter(|c| !c.in_room).collect();
            if scope.is_some() && !best_tier(&outside, &needle).is_empty() {
                resolve_none("not_in_room")
            } else {
                resolve_none("no_match")
            }
        }
        [one] => resolve_response(json!({ "status": "ok", "match": one.json })),
        many => resolve_response(json!({
            "status": "ambiguous",
            "candidates": many.iter().take(MAX_CANDIDATES).map(|c| c.json.clone()).collect::<Vec<_>>(),
        })),
    }
}

// ---------------------------------------------------------------------------------------------
// WS04-S8: the room directory (`GET /api/v1/directory/rooms`).
//
// Control decides which rooms the caller may browse (open, managed, active, not already a member,
// organization policy on; absent policy means `enabled:false`). Application only supplies the
// display labels. A room Control lists but Application has no active label for is dropped, so the
// directory never shows a bare ref. The label read is intersected with Control's list, never
// browsed, so a name or topic of a room Control did not list is never returned. No content moves,
// so a Zero Data Retention organization behaves the same.
// ---------------------------------------------------------------------------------------------

/// Rooms per page of the directory.
const ROOMS_PAGE_SIZE: usize = 25;

#[derive(Debug, Default, Deserialize)]
pub(super) struct RoomsParams {
    q: Option<String>,
    cursor: Option<String>,
}

fn encode_rooms_cursor(name: &str, space_ref: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(json!([name, space_ref]).to_string())
}

fn decode_rooms_cursor(raw: &str) -> Option<(String, String)> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes = URL_SAFE_NO_PAD.decode(raw).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let pair = value.as_array()?;
    Some((
        pair.first()?.as_str()?.to_owned(),
        pair.get(1)?.as_str()?.to_owned(),
    ))
}

fn room_ref(room: &Value) -> String {
    room.get("space_ref")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// `GET /api/v1/directory/rooms?q=&cursor=`: `{data:{enabled,rooms:[{space_ref,name,topic,
/// member_count,join_role}],next_cursor},meta:{truncated}}`, sorted by name, 25 per page.
pub(super) async fn rooms_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<RoomsParams>,
) -> Response {
    if !state.space_features.room_directory_api {
        return feature_disabled("The room directory is not enabled.");
    }
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return membership_required();
    }
    if let Some(response) = rate_limited(&state, &org_id, &user).await {
        return response;
    }
    let needle = params
        .q
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_lowercase();
    if needle.chars().count() > MAX_QUERY_CHARS {
        return invalid("q", "too_long");
    }
    let after = match params
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
    {
        None => None,
        Some(raw) if raw.len() > MAX_CURSOR_LEN => return invalid("cursor", "too_long"),
        Some(raw) => match decode_rooms_cursor(raw) {
            Some(key) => Some(key),
            None => return invalid("cursor", "malformed"),
        },
    };
    let data = match control_rooms(&state, &user, &org_id).await {
        Ok(data) => data,
        Err(failure) => return failure.into_response(),
    };
    let enabled = data
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let truncated = data
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let listed: Vec<Value> = data
        .get("rooms")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // (lowercase name, room) pairs; the name is the sort key and half the cursor.
    let mut rooms: Vec<(String, Value)> = Vec::new();
    if enabled && !listed.is_empty() {
        // A failed label read is a 503, never an empty directory.
        let labels = match super::convex_gateway_call(
            &state,
            "query",
            "spaces:spacesForOrgForGateway",
            json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
        )
        .await
        {
            Ok(value) => value.as_array().cloned().unwrap_or_default(),
            Err(()) => return unavailable(),
        };
        for room in &listed {
            let space_ref = room_ref(room);
            if space_ref.is_empty() {
                continue;
            }
            let Some(label) = labels.iter().find(|label| {
                label.get("spaceRef").and_then(Value::as_str) == Some(space_ref.as_str())
                    && label.get("lifecycle").and_then(Value::as_str) == Some("active")
            }) else {
                continue;
            };
            let name = label
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if name.is_empty() {
                continue;
            }
            let topic = label.get("topic").and_then(Value::as_str);
            if !needle.is_empty()
                && !name.to_lowercase().contains(&needle)
                && !topic.is_some_and(|topic| topic.to_lowercase().contains(&needle))
            {
                continue;
            }
            rooms.push((
                name.to_lowercase(),
                json!({
                    "space_ref": space_ref,
                    "name": name,
                    "topic": topic,
                    "member_count": room.get("member_count").and_then(Value::as_i64).unwrap_or(0),
                    "join_role": room.get("join_role").and_then(Value::as_str).unwrap_or("viewer"),
                }),
            ));
        }
    }
    rooms.sort_by(|a, b| (&a.0, room_ref(&a.1)).cmp(&(&b.0, room_ref(&b.1))));
    if let Some((after_name, after_ref)) = after {
        rooms.retain(|(key, room)| (key.clone(), room_ref(room)) > (after_name.clone(), after_ref.clone()));
    }
    let more = rooms.len() > ROOMS_PAGE_SIZE;
    rooms.truncate(ROOMS_PAGE_SIZE);
    let next = if more {
        rooms.last().map_or(Value::Null, |(key, room)| {
            json!(encode_rooms_cursor(key, &room_ref(room)))
        })
    } else {
        Value::Null
    };
    let page: Vec<Value> = rooms.into_iter().map(|(_, room)| room).collect();
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "data": { "enabled": enabled, "rooms": page, "next_cursor": next },
            "meta": { "truncated": truncated },
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

/// One Control read of the browsable rooms under the caller's signed delegation.
async fn control_rooms(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<Value, ControlFailure> {
    let url = format!("{}/api/v1/internal/directory/rooms", state.user_core_url);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if status.is_server_error() {
        return Err(ControlFailure::Unavailable);
    }
    if !status.is_success() {
        return Err(ControlFailure::Passthrough(status, control));
    }
    Ok(unwrap_data(&control))
}

#[cfg(test)]
mod tests {
    use super::{control_query, people_route, PeopleParams};
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{
        body::Body,
        extract::{Extension, Query, State},
        http::Request,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CONTROL_PATH: &str = "/api/v1/internal/directory/people";

    /// Each test uses its own user id: the limiter is process-wide and keyed by org and user.
    fn member(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: format!("{user_id}@example.com"),
            user_name: "Caller".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn state_with(control: &MockServer, flag: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.user_core_url = control.uri();
        state.space_features.people_search_api = flag;
        state
    }

    async fn control(template: ResponseTemplate) -> MockServer {
        let control = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(CONTROL_PATH))
            .respond_with(template)
            .mount(&control)
            .await;
        control
    }

    fn people_ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": data}))
    }

    async fn run(
        state: &AppState,
        user: AuthenticatedUser,
        params: PeopleParams,
    ) -> (u16, axum::http::HeaderMap, Value) {
        let response = people_route(State(state.clone()), Extension(user), Query(params)).await;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, headers, body)
    }

    fn q(text: &str) -> PeopleParams {
        PeopleParams { q: Some(text.to_owned()), ..PeopleParams::default() }
    }

    async fn control_requests(control: &MockServer) -> usize {
        control.received_requests().await.expect("requests").len()
    }

    #[tokio::test]
    async fn flag_off_is_404_feature_disabled_with_zero_control_requests() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let control = control(people_ok(json!({"people": []}))).await;
        let (status, _, body) = run(&state_with(&control, false), member("dir-flag-off"), q("ann")).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert_eq!(control_requests(&control).await, 0);
    }

    #[tokio::test]
    async fn a_user_without_an_organization_membership_is_refused_before_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let control = control(people_ok(json!({"people": []}))).await;
        let mut user = member("dir-no-org");
        user.authorized_membership = None;
        let (status, _, body) = run(&state_with(&control, true), user.clone(), q("ann")).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "active_membership_required");
        // A blank organization id is the same refusal.
        user.authorized_membership =
            Some(AuthorizedMembership { organization_id: "  ".to_owned(), role: "member".to_owned() });
        let (status, _, body) = run(&state_with(&control, true), user, q("ann")).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "active_membership_required");
        assert_eq!(control_requests(&control).await, 0);
    }

    #[tokio::test]
    async fn an_over_long_or_malformed_query_is_a_400_with_field_and_reason_and_no_control_request() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let control = control(people_ok(json!({"people": []}))).await;
        let state = state_with(&control, true);
        let cases: Vec<(PeopleParams, &str, &str)> = vec![
            (q(&"a".repeat(101)), "q", "too_long"),
            (PeopleParams { limit: Some("0".into()), ..Default::default() }, "limit", "out_of_range"),
            (PeopleParams { limit: Some("51".into()), ..Default::default() }, "limit", "out_of_range"),
            (PeopleParams { limit: Some("abc".into()), ..Default::default() }, "limit", "out_of_range"),
            (PeopleParams { limit: Some("-1".into()), ..Default::default() }, "limit", "out_of_range"),
            (PeopleParams { limit: Some("1.5".into()), ..Default::default() }, "limit", "out_of_range"),
            (PeopleParams { cursor: Some("c".repeat(513)), ..Default::default() }, "cursor", "too_long"),
            (PeopleParams { exclude_member_of: Some("s".repeat(201)), ..Default::default() }, "exclude_member_of", "too_long"),
        ];
        for (index, (params, field, reason)) in cases.into_iter().enumerate() {
            let (status, _, body) = run(&state, member(&format!("dir-invalid-{index}")), params).await;
            assert_eq!(status, 400, "{field}");
            assert_eq!(body["error"]["code"], "invalid_request");
            assert_eq!(body["error"]["details"], json!({"field": field, "reason": reason}));
        }
        assert_eq!(control_requests(&control).await, 0);
    }

    #[test]
    fn the_bounds_themselves_are_accepted_and_only_the_four_parameters_are_built() {
        let params = PeopleParams {
            q: Some("a".repeat(100)),
            limit: Some("50".into()),
            cursor: Some("c".repeat(512)),
            exclude_member_of: Some("space-room".into()),
            kinds: None,
        };
        let query = control_query(&params).expect("bounds are valid");
        let keys: Vec<&str> = query.split('&').filter_map(|part| part.split('=').next()).collect();
        assert_eq!(keys, ["q", "limit", "cursor", "exclude_member_of"]);
        assert!(control_query(&PeopleParams { limit: Some("1".into()), ..Default::default() }).is_ok());
        // Blank values are dropped rather than forwarded.
        let blank = control_query(&PeopleParams {
            q: Some("   ".into()),
            limit: Some(" ".into()),
            cursor: Some("".into()),
            exclude_member_of: Some(" ".into()),
            kinds: None,
        })
        .unwrap();
        assert_eq!(blank, "");
    }

    #[test]
    fn q_is_percent_encoded() {
        let query = control_query(&q("caf\u{00e9} & co=1")).unwrap();
        assert_eq!(query, "q=caf%C3%A9%20%26%20co%3D1");
    }

    /// Through the real router: only the four known parameters reach Control, unknown ones such
    /// as `org_id` and `user_id` are dropped, and the delegation carries the session's org and
    /// user, not anything from the query string.
    #[tokio::test]
    async fn the_control_request_carries_only_known_params_and_the_session_delegation() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "dir-router-user", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"dir-router-user", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path(CONTROL_PATH))
            .respond_with(people_ok(json!({"people": [], "next_cursor": ""})))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.space_features.people_search_api = true;
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/directory/people?q=caf%C3%A9%20x&limit=5&cursor=abc&exclude_member_of=space-room&org_id=forged&user_id=forged&subject_id=forged")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let requests = user_core.received_requests().await.unwrap();
        let sent = requests
            .iter()
            .find(|request| request.url.path() == CONTROL_PATH)
            .expect("Control request");
        let mut keys: Vec<String> = sent.url.query_pairs().map(|(key, _)| key.into_owned()).collect();
        keys.sort();
        assert_eq!(keys, ["cursor", "exclude_member_of", "limit", "q"]);
        let q_value = sent.url.query_pairs().find(|(key, _)| key == "q").map(|(_, value)| value.into_owned());
        assert_eq!(q_value.as_deref(), Some("caf\u{00e9} x"));
        assert!(sent.headers.get("x-delegation-signature").is_some(), "a signed delegation");
        let header = |name: &str| {
            sent.headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(header("x-user-id").as_deref(), Some("dir-router-user"));
        assert_eq!(header("x-org-id").as_deref(), Some("org-1"));
        assert!(!sent.url.to_string().contains("forged"));
    }

    #[tokio::test]
    async fn success_maps_the_four_field_person_shape_with_no_store_and_a_null_empty_cursor() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let control = control(people_ok(json!({
            "people": [
                {"subject_id": "u-2", "display_name": "Ann", "email": "ann@example.com", "is_self": false,
                 "role": "owner", "org_id": "org-1", "password_hash": "nope"},
                {"subject_id": "u-3"},
                {"display_name": "No Id", "email": "x@example.com"}
            ],
            "next_cursor": ""
        })))
        .await;
        let (status, headers, body) = run(&state_with(&control, true), member("dir-success"), q("a")).await;
        assert_eq!(status, 200);
        assert_eq!(headers.get("cache-control").and_then(|v| v.to_str().ok()), Some("no-store"));
        assert_eq!(
            body,
            json!({"data": {"people": [
                {"subject_id": "u-2", "display_name": "Ann", "email": "ann@example.com", "is_self": false},
                {"subject_id": "u-3", "display_name": "", "email": "", "is_self": false}
            ], "next_cursor": null}})
        );
        assert!(!body.to_string().contains("password_hash"));
    }

    #[tokio::test]
    async fn a_non_empty_cursor_is_passed_on_and_a_missing_one_is_null() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let with_cursor = control(people_ok(json!({"people": [], "next_cursor": "page-2"}))).await;
        let (_, _, body) = run(&state_with(&with_cursor, true), member("dir-cursor-a"), q("a")).await;
        assert_eq!(body["data"]["next_cursor"], "page-2");
        let without = control(people_ok(json!({"people": []}))).await;
        let (_, _, body) = run(&state_with(&without, true), member("dir-cursor-b"), q("a")).await;
        assert!(body["data"]["next_cursor"].is_null());
        assert_eq!(body["data"]["people"], json!([]));
    }

    #[tokio::test]
    async fn control_typed_4xx_pass_through_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (status, body) in [
            (403, json!({"error": {"code": "member_not_in_organization", "message": "Not a member."}})),
            (400, json!({"error": {"code": "invalid_request", "message": "Bad query.", "details": {"field": "q"}}})),
        ] {
            let control = control(ResponseTemplate::new(status).set_body_json(body.clone())).await;
            let (got_status, _, got) =
                run(&state_with(&control, true), member(&format!("dir-4xx-{status}")), q("a")).await;
            assert_eq!(got_status, status);
            assert_eq!(got, body);
        }
    }

    #[tokio::test]
    async fn a_control_5xx_or_unreachable_control_is_a_503_directory_unavailable_never_an_empty_list() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let down = control(ResponseTemplate::new(500).set_body_json(json!({"error": "boom"}))).await;
        let (status, _, body) = run(&state_with(&down, true), member("dir-5xx"), q("a")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");
        assert!(body.get("data").is_none());

        let mut unreachable = crate::tests::test_state(false);
        unreachable.user_core_url = "http://127.0.0.1:1".to_owned();
        unreachable.space_features.people_search_api = true;
        let (status, _, body) = run(&unreachable, member("dir-unreachable"), q("a")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");
    }

    #[tokio::test]
    async fn the_61st_search_in_a_minute_by_one_user_is_429_with_retry_after() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let control = control(people_ok(json!({"people": []}))).await;
        let state = state_with(&control, true);
        // An over-long query is refused AFTER the limiter, so these spend a token each without
        // a Control round trip: the whole burst fits well inside one refill second.
        for index in 0..60 {
            let (status, _, _) = run(&state, member("dir-limited"), q(&"a".repeat(101))).await;
            assert_eq!(status, 400, "request {index} is inside the limit");
        }
        let (status, headers, body) = run(&state, member("dir-limited"), q("ann")).await;
        assert_eq!(status, 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        let retry: u64 = headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .expect("Retry-After");
        assert!(retry >= 1);
        assert_eq!(control_requests(&control).await, 0);
        // Another user is not throttled by it.
        let (status, _, _) = run(&state, member("dir-other-user"), q("ann")).await;
        assert_eq!(status, 200);
    }

    // -----------------------------------------------------------------------------------------
    // WS04-S9: composed search (`kinds=`) and the resolver.
    // -----------------------------------------------------------------------------------------

    use super::resolve_route;
    use axum::Json;
    use wiremock::matchers::{body_partial_json, query_param};

    const SPACES_PATH: &str = "/api/v1/internal/spaces";

    struct Fx {
        state: AppState,
        control: MockServer,
        app: MockServer,
    }

    fn person_row(id: &str, name: &str, email: &str) -> Value {
        json!({"subject_id": id, "display_name": name, "email": email, "is_self": false})
    }

    fn binding(agent_ref: &str, name: &str, description: &str, def: &str, status: &str, space: &str) -> Value {
        json!({
            "agentRef": agent_ref, "name": name, "description": description,
            "definitionStatus": def, "status": status, "spaceRef": space
        })
    }

    /// Control answers `people` for every directory read, `outside` (when given) for reads that
    /// carry `exclude_member_of`, and `rooms` as the caller's Space index; Application answers the
    /// agent registry with `bindings`. Both flags are on.
    async fn fixture(people: Value, outside: Option<Value>, rooms: &[&str], bindings: Vec<Value>) -> Fx {
        let control = MockServer::start().await;
        if let Some(outside) = outside {
            Mock::given(wm_method("GET"))
                .and(wm_path(CONTROL_PATH))
                .and(query_param("exclude_member_of", "space-room"))
                .respond_with(people_ok(json!({"people": outside})))
                .mount(&control)
                .await;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path(CONTROL_PATH))
            .respond_with(people_ok(json!({"people": people, "next_cursor": "next-1"})))
            .mount(&control)
            .await;
        let spaces: Vec<Value> = rooms.iter().map(|room| json!({"space_ref": room})).collect();
        Mock::given(wm_method("GET"))
            .and(wm_path(SPACES_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"spaces": spaces}})))
            .mount(&control)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaceAgents:agentInstallationsForOrgForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": bindings})))
            .mount(&app)
            .await;
        let mut state = state_with(&control, true);
        state.space_features.directory_resolver_api = true;
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        Fx { state, control, app }
    }

    async fn app_requests(fx: &Fx) -> usize {
        fx.app.received_requests().await.expect("requests").len()
    }

    fn kinds(raw: &str) -> PeopleParams {
        PeopleParams { kinds: Some(raw.to_owned()), ..PeopleParams::default() }
    }

    async fn resolve(state: &AppState, user: &str, body: Value) -> (u16, Value) {
        let response = resolve_route(State(state.clone()), Extension(member(user)), Json(body)).await;
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body)
    }

    #[tokio::test]
    async fn resolver_flag_off_is_404_feature_disabled_on_both_routes_with_zero_upstream_requests() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(json!([]), None, &[], vec![]).await;
        fx.state.space_features.directory_resolver_api = false;
        let (status, body) = resolve(&fx.state, "res-off-a", json!({"query": "ann"})).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        let (status, _, body) = run(&fx.state, member("res-off-b"), kinds("person,agent")).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert_eq!(control_requests(&fx.control).await, 0);
        assert_eq!(app_requests(&fx).await, 0);
        // Without `kinds` the S3 people search keeps working with the resolver flag off.
        let (status, _, body) = run(&fx.state, member("res-off-c"), q("a")).await;
        assert_eq!(status, 200);
        assert!(body["data"]["people"].is_array());
    }

    #[tokio::test]
    async fn an_unknown_kind_is_a_400_and_kinds_is_never_forwarded_to_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(json!([person_row("u-1", "Ann", "ann@example.com")]), None, &[], vec![]).await;
        for (index, raw) in ["bot", "person,bot", "person,,agent", ""].into_iter().enumerate() {
            let (status, _, body) = run(&fx.state, member(&format!("res-kind-{index}")), kinds(raw)).await;
            assert_eq!(status, 400, "{raw:?}");
            assert_eq!(body["error"]["code"], "invalid_request");
            assert_eq!(body["error"]["details"], json!({"field": "kinds", "reason": "unknown_kind"}));
        }
        assert_eq!(control_requests(&fx.control).await, 0);
        // A valid composed read (case-insensitive kinds) reaches Control without `kinds`.
        let (status, _, _) = run(
            &fx.state,
            member("res-kind-ok"),
            PeopleParams { q: Some("ann".into()), kinds: Some(" Person , AGENT ".into()), ..Default::default() },
        )
        .await;
        assert_eq!(status, 200);
        let requests = fx.control.received_requests().await.unwrap();
        let sent = requests.iter().find(|r| r.url.path() == CONTROL_PATH).expect("Control request");
        assert!(sent.url.query_pairs().all(|(key, _)| key != "kinds"));
    }

    #[tokio::test]
    async fn composed_search_returns_people_then_published_agents_with_presence_only_meta() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            json!([person_row("u-2", "Ann", "ann@example.com")]),
            None,
            &["space-room", "space-mine"],
            vec![
                binding("agent-b", "Budget Helper", "Watches budgets", "active", "active", "space-room"),
                // The same definition in a room the caller is NOT in, and a removed binding.
                binding("agent-b", "Budget Helper", "Watches budgets", "active", "active", "space-secret"),
                binding("agent-b", "Budget Helper", "Watches budgets", "active", "removed", "space-mine"),
                binding("agent-c", "Calendar Bot", "Plans days", "active", "paused", "space-mine"),
                binding("agent-d", "Draft Agent", "Unpublished", "draft", "active", "space-room"),
                binding("agent-e", "Old Agent", "Retired", "inactive", "active", "space-room"),
            ],
        )
        .await;
        let (status, headers, body) = run(&fx.state, member("res-composed"), kinds("person,agent")).await;
        assert_eq!(status, 200);
        assert_eq!(headers.get("cache-control").and_then(|v| v.to_str().ok()), Some("no-store"));
        assert_eq!(body["meta"], json!({"presence_only": true}));
        assert_eq!(body["data"]["next_cursor"], "next-1");
        let results = body["data"]["results"].as_array().unwrap();
        assert_eq!(
            results[0],
            json!({"kind": "person", "subject_id": "u-2", "display_name": "Ann",
                   "email": "ann@example.com", "is_self": false})
        );
        assert_eq!(results.len(), 3, "one person then two published agents: {results:?}");
        assert_eq!(
            results[1],
            json!({"kind": "agent", "agent_ref": "agent-b", "display_name": "Budget Helper",
                   "description": "Watches budgets", "installed_in": ["space-room"]})
        );
        assert_eq!(results[2]["agent_ref"], "agent-c");
        assert_eq!(results[2]["installed_in"], json!(["space-mine"]));
        let text = body.to_string();
        assert!(!text.contains("space-secret"), "a room the caller is not in never appears");
        assert!(!text.contains("space_name") && !text.contains("Draft Agent") && !text.contains("Old Agent"));
    }

    #[tokio::test]
    async fn composed_search_agents_are_first_page_only_capped_at_ten_and_filtered_by_q() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let bindings: Vec<Value> = (0..12)
            .map(|i| binding(&format!("agent-{i:02}"), &format!("Helper {i:02}"), "desc", "active", "active", "space-room"))
            .collect();
        let fx = fixture(json!([]), None, &["space-room"], bindings).await;
        let (_, _, body) = run(&fx.state, member("res-cap-a"), kinds("agent")).await;
        assert_eq!(body["data"]["results"].as_array().unwrap().len(), 10);
        // kinds=agent alone never asks Control for people.
        assert_eq!(control_requests(&fx.control).await, 1, "only the Space index read");
        // A cursor means a later page: people continue, agents are not repeated.
        let (_, _, body) = run(
            &fx.state,
            member("res-cap-b"),
            PeopleParams { cursor: Some("page-2".into()), kinds: Some("person,agent".into()), ..Default::default() },
        )
        .await;
        assert!(body["data"]["results"].as_array().unwrap().iter().all(|r| r["kind"] == "person"));
        // `q` filters agents by name or description, case-insensitively.
        let (_, _, body) = run(
            &fx.state,
            member("res-cap-c"),
            PeopleParams { q: Some("HELPER 07".into()), kinds: Some("agent".into()), ..Default::default() },
        )
        .await;
        let results = body["data"]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["agent_ref"], "agent-07");
    }

    #[tokio::test]
    async fn composed_search_control_4xx_passes_through_and_5xx_or_registry_outage_is_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(json!([]), None, &[], vec![]).await;
        let forbidden = json!({"error": {"code": "member_not_in_organization", "message": "Not a member."}});
        let control = control(ResponseTemplate::new(403).set_body_json(forbidden.clone())).await;
        let mut state = fx.state.clone();
        state.user_core_url = control.uri();
        let (status, _, body) = run(&state, member("res-4xx"), kinds("person")).await;
        assert_eq!((status, body), (403, forbidden));
        let down = self::control(ResponseTemplate::new(500).set_body_json(json!({"error": "boom"}))).await;
        state.user_core_url = down.uri();
        let (status, _, body) = run(&state, member("res-5xx"), kinds("person,agent")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");
        // The agent registry failing is a 503 as well, never agents silently missing.
        let mut no_convex = fx.state.clone();
        no_convex.application_convex_url = String::new();
        let (status, _, body) = run(&no_convex, member("res-registry"), kinds("agent")).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");
    }

    fn match_of(body: &Value) -> &Value {
        assert_eq!(body["data"]["status"], "ok", "{body}");
        &body["data"]["match"]
    }

    #[tokio::test]
    async fn resolve_exact_email_exact_full_name_and_a_unique_prefix_are_ok_with_presence_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            json!([
                person_row("u-1", "Annika Berg", "annika@example.com"),
                person_row("u-2", "Bjorn Hansen", "bjorn@example.com")
            ]),
            None,
            &[],
            vec![],
        )
        .await;
        let (status, body) = resolve(&fx.state, "res-ok-email", json!({"query": "BJORN@example.com", "kind": "person"})).await;
        assert_eq!(status, 200);
        assert_eq!(body["meta"], json!({"presence_only": true}));
        assert_eq!(match_of(&body)["subject_id"], "u-2");
        let (_, body) = resolve(&fx.state, "res-ok-name", json!({"query": "annika berg"})).await;
        assert_eq!(match_of(&body)["subject_id"], "u-1");
        assert_eq!(match_of(&body)["kind"], "person");
        let (_, body) = resolve(&fx.state, "res-ok-prefix", json!({"query": "bjo", "kind": "any"})).await;
        assert_eq!(match_of(&body)["subject_id"], "u-2");
    }

    #[tokio::test]
    async fn resolve_two_prefix_matches_are_ambiguous_with_at_most_five_candidates_and_exact_beats_prefix() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let many: Vec<Value> = (0..7)
            .map(|i| person_row(&format!("u-{i}"), &format!("Sam {i}"), &format!("sam{i}@example.com")))
            .collect();
        let fx = fixture(json!(many), None, &[], vec![]).await;
        let (status, body) = resolve(&fx.state, "res-amb", json!({"query": "sam", "kind": "person"})).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["status"], "ambiguous");
        assert_eq!(body["data"]["candidates"].as_array().unwrap().len(), 5);
        assert_eq!(body["meta"]["presence_only"], true);

        let fx = fixture(
            json!([person_row("u-a", "Sam", "sam@example.com"), person_row("u-b", "Samantha", "samantha@example.com")]),
            None,
            &[],
            vec![],
        )
        .await;
        let (_, body) = resolve(&fx.state, "res-exact", json!({"query": "Sam", "kind": "person"})).await;
        assert_eq!(match_of(&body)["subject_id"], "u-a", "the exact tier wins over the prefix tier");
    }

    #[tokio::test]
    async fn resolve_no_match_blank_overlong_and_bad_kind() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(json!([person_row("u-1", "Ann", "ann@example.com")]), None, &[], vec![]).await;
        let (status, body) = resolve(&fx.state, "res-none", json!({"query": "zed"})).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"], json!({"status": "none", "reason": "no_match"}));
        let cases = [
            (json!({"query": "   "}), "query", "required"),
            (json!({}), "query", "required"),
            (json!({"query": "a".repeat(101)}), "query", "too_long"),
            (json!({"query": "ann", "kind": "robot"}), "kind", "unknown_kind"),
            (json!({"query": "ann", "space_ref": "s".repeat(201)}), "space_ref", "too_long"),
        ];
        let before = control_requests(&fx.control).await;
        for (index, (request, field, reason)) in cases.into_iter().enumerate() {
            let (status, body) = resolve(&fx.state, &format!("res-bad-{index}"), request).await;
            assert_eq!(status, 400, "{field}");
            assert_eq!(body["error"]["code"], "invalid_request");
            assert_eq!(body["error"]["details"], json!({"field": field, "reason": reason}));
        }
        assert_eq!(control_requests(&fx.control).await, before, "no upstream call on a bad body");
    }

    #[tokio::test]
    async fn resolve_control_or_convex_outage_is_200_none_directory_unavailable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(json!([]), None, &[], vec![]).await;
        let down = control(ResponseTemplate::new(503).set_body_json(json!({"error": "down"}))).await;
        let mut state = fx.state.clone();
        state.user_core_url = down.uri();
        let (status, body) = resolve(&state, "res-out-a", json!({"query": "ann", "kind": "person"})).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"], json!({"status": "none", "reason": "directory_unavailable"}));
        let mut no_convex = fx.state.clone();
        no_convex.application_convex_url = String::new();
        let (status, body) = resolve(&no_convex, "res-out-b", json!({"query": "ann", "kind": "agent"})).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"], json!({"status": "none", "reason": "directory_unavailable"}));
    }

    #[tokio::test]
    async fn resolve_with_space_ref_scopes_to_members_when_the_caller_is_in_the_room() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // Control's "outside the room" list holds u-b only, so u-a is a member.
        let people = json!([person_row("u-a", "Alice", "alice@example.com"), person_row("u-b", "Alicia", "alicia@example.com")]);
        let fx = fixture(people.clone(), Some(json!([person_row("u-b", "Alicia", "alicia@example.com")])), &["space-room"], vec![]).await;
        let (_, body) = resolve(&fx.state, "res-scope-a", json!({"query": "ali", "kind": "person", "space_ref": "space-room"})).await;
        assert_eq!(match_of(&body)["subject_id"], "u-a", "the org member outside the room is not a candidate");
        let (_, body) = resolve(&fx.state, "res-scope-b", json!({"query": "alicia", "kind": "person", "space_ref": "space-room"})).await;
        assert_eq!(body["data"], json!({"status": "none", "reason": "not_in_room"}));
        let (_, body) = resolve(&fx.state, "res-scope-c", json!({"query": "zed", "kind": "person", "space_ref": "space-room"})).await;
        assert_eq!(body["data"]["reason"], "no_match");

        // The caller is not in the room: space_ref is ignored and nothing says the room exists.
        let fx = fixture(people, Some(json!([person_row("u-b", "Alicia", "alicia@example.com")])), &["space-other"], vec![]).await;
        let (_, body) = resolve(&fx.state, "res-scope-d", json!({"query": "alicia", "kind": "person", "space_ref": "space-room"})).await;
        assert_eq!(match_of(&body)["subject_id"], "u-b");
        let requests = fx.control.received_requests().await.unwrap();
        assert!(
            requests.iter().filter(|r| r.url.path() == CONTROL_PATH).all(|r| r.url.query_pairs().all(|(k, _)| k != "exclude_member_of")),
            "no roster probe for a room the caller is not in"
        );
    }

    #[tokio::test]
    async fn resolve_agents_are_scoped_by_installed_in() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            json!([]),
            None,
            &["space-room", "space-mine"],
            vec![
                binding("agent-in", "Helper In", "x", "active", "active", "space-room"),
                binding("agent-out", "Helper Out", "x", "active", "active", "space-mine"),
            ],
        )
        .await;
        let scoped = |query: &str| json!({"query": query, "kind": "agent", "space_ref": "space-room"});
        let (_, body) = resolve(&fx.state, "res-agent-a", scoped("helper in")).await;
        assert_eq!(match_of(&body)["agent_ref"], "agent-in");
        assert_eq!(match_of(&body)["kind"], "agent");
        let (_, body) = resolve(&fx.state, "res-agent-b", scoped("helper out")).await;
        assert_eq!(body["data"], json!({"status": "none", "reason": "not_in_room"}));
        let (_, body) = resolve(&fx.state, "res-agent-c", json!({"query": "helper", "kind": "agent"})).await;
        assert_eq!(body["data"]["status"], "ambiguous");
    }

    #[tokio::test]
    async fn the_resolver_and_the_search_share_one_60_per_minute_limiter() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(json!([]), None, &[], vec![]).await;
        // A blank query is refused AFTER the limiter, so each call spends a token and no upstream.
        for index in 0..60 {
            let (status, _) = resolve(&fx.state, "res-limited", json!({"query": ""})).await;
            assert_eq!(status, 400, "request {index} is inside the limit");
        }
        let (status, headers, body) = run(&fx.state, member("res-limited"), kinds("person")).await;
        assert_eq!(status, 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(headers.get("retry-after").is_some());
        let (status, body) = resolve(&fx.state, "res-limited", json!({"query": "ann"})).await;
        assert_eq!(status, 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert_eq!(control_requests(&fx.control).await, 0);
    }

    /// `GET /spaces/{ref}/context` reports `features.directory_resolver` as the flag says, and
    /// off by default.
    #[tokio::test]
    async fn space_context_features_directory_resolver_mirrors_the_flag() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for flag in [false, true] {
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
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": {"space_ref": "space-room", "role": "editor"}
                })))
                .mount(&user_core)
                .await;
            let app = MockServer::start().await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/query"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                    {"spaceRef":"space-room","name":"Leveranse","kind":"room","lifecycle":"active"}
                ]})))
                .mount(&app)
                .await;
            let mut state = crate::tests::test_state(false);
            state.application_convex_url = app.uri();
            state.application_convex_service_key = "application-test-key".into();
            state.auth_core_url = auth.uri();
            state.user_core_url = user_core.uri();
            state.space_features.directory_resolver_api = flag;
            let response = crate::build_router(state)
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/spaces/space-room/context")
                        .header("cookie", "better-auth.session_token=session-1")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 200);
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(body["data"]["features"]["directory_resolver"], flag);
        }
        assert!(!crate::config::SpaceFeatureFlags::default().directory_resolver_api);
    }
}

#[cfg(test)]
mod rooms_tests {
    //! WS04-S8.gw: the room directory route. Control lists the browsable rooms, Application only
    //! supplies labels; both are mocks.
    use super::{decode_rooms_cursor, encode_rooms_cursor, rooms_route, RoomsParams};
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::extract::{Extension, Query, State};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ROOMS_PATH: &str = "/api/v1/internal/directory/rooms";

    /// Each test uses its own user id: the limiter is process-wide and keyed by org and user.
    fn member(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: format!("{user_id}@example.com"),
            user_name: "Caller".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    struct Fx {
        state: AppState,
        control: MockServer,
        application: MockServer,
    }

    async fn fixture(control: ResponseTemplate, labels: ResponseTemplate, flag: bool) -> Fx {
        let control_server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(ROOMS_PATH))
            .respond_with(control)
            .mount(&control_server)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(labels)
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = control_server.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.room_directory_api = flag;
        Fx { state, control: control_server, application }
    }

    fn rooms_ok(rooms: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {"enabled": true, "rooms": rooms, "truncated": false}}))
    }

    fn labels(rows: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": rows}))
    }

    fn listed(space_ref: &str) -> Value {
        json!({"space_ref": space_ref, "member_count": 3, "join_role": "editor"})
    }

    fn label(space_ref: &str, name: &str, topic: Option<&str>) -> Value {
        json!({"spaceRef": space_ref, "name": name, "topic": topic, "kind": "room", "lifecycle": "active"})
    }

    async fn run(fx: &Fx, user_id: &str, q: Option<&str>, cursor: Option<&str>) -> (u16, Value) {
        let params = RoomsParams { q: q.map(str::to_owned), cursor: cursor.map(str::to_owned) };
        let response = rooms_route(State(fx.state.clone()), Extension(member(user_id)), Query(params)).await;
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap_or(Value::Null);
        (status, body)
    }

    fn names(body: &Value) -> Vec<String> {
        body["data"]["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|room| room["name"].as_str().unwrap().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn flag_off_is_404_feature_disabled_with_zero_control_requests() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(rooms_ok(json!([listed("r1")])), labels(json!([])), false).await;
        let (status, body) = run(&fx, "rooms-flag-off", None, None).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(fx.control.received_requests().await.unwrap().is_empty());
        assert!(fx.application.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn policy_off_answers_enabled_false_and_no_rooms_without_a_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"enabled": false, "rooms": [listed("r1")]}})),
            labels(json!([label("r1", "Alpha", None)])),
            true,
        )
        .await;
        let (status, body) = run(&fx, "rooms-policy-off", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["enabled"], false);
        assert_eq!(body["data"]["rooms"], json!([]));
        assert!(body["data"]["next_cursor"].is_null());
        assert!(fx.application.received_requests().await.unwrap().is_empty(), "no label read when the policy is off");
    }

    #[tokio::test]
    async fn rooms_without_an_active_label_are_dropped_and_the_rest_sorted_by_name() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            rooms_ok(json!([listed("r-zulu"), listed("r-alpha"), listed("r-archived"), listed("r-nolabel"), listed("r-blank"), listed("")])),
            labels(json!([
                label("r-zulu", "Zulu", Some("last")),
                label("r-alpha", "alpha", None),
                {"spaceRef": "r-archived", "name": "Gone", "lifecycle": "archived"},
                {"spaceRef": "r-blank", "name": "   ", "lifecycle": "active"},
                label("r-unlisted", "Secret", None),
            ])),
            true,
        )
        .await;
        let (status, body) = run(&fx, "rooms-labels", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(names(&body), ["alpha", "Zulu"], "case-insensitive sort; unlabelled, archived, blank and unlisted rooms never appear");
        let first = &body["data"]["rooms"][0];
        assert_eq!(first["space_ref"], "r-alpha");
        assert_eq!(first["member_count"], 3);
        assert_eq!(first["join_role"], "editor");
        assert!(first["topic"].is_null());
        assert_eq!(body["meta"]["truncated"], false);
        // the label read is scoped to the caller and the organization
        let query = &fx.application.received_requests().await.unwrap()[0];
        let sent: Value = serde_json::from_slice(&query.body).unwrap();
        assert_eq!(sent["path"], "spaces:spacesForOrgForGateway");
        assert_eq!(sent["args"]["externalOrgId"], "org-1");
    }

    #[tokio::test]
    async fn q_filters_on_name_and_topic_case_insensitively() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            rooms_ok(json!([listed("r1"), listed("r2"), listed("r3")])),
            labels(json!([
                label("r1", "Leveranse", None),
                label("r2", "Salg", Some("Ukens LEVERING")),
                label("r3", "Okonomi", Some("budsjett")),
            ])),
            true,
        )
        .await;
        let (_, body) = run(&fx, "rooms-q-1", Some("  lever "), None).await;
        assert_eq!(names(&body), ["Leveranse", "Salg"]);
        let (_, body) = run(&fx, "rooms-q-2", Some("BUDSJ"), None).await;
        assert_eq!(names(&body), ["Okonomi"]);
        let (_, body) = run(&fx, "rooms-q-3", Some("nothing"), None).await;
        assert_eq!(names(&body), Vec::<String>::new());
        let (status, body) = run(&fx, "rooms-q-4", Some(&"a".repeat(101)), None).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["details"], json!({"field": "q", "reason": "too_long"}));
    }

    #[tokio::test]
    async fn pages_hold_25_rooms_with_an_opaque_cursor_that_resumes_after_the_last_room() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let refs: Vec<String> = (0..30).map(|i| format!("r{i:02}")).collect();
        let fx = fixture(
            rooms_ok(Value::Array(refs.iter().map(|r| listed(r)).collect())),
            labels(Value::Array(refs.iter().map(|r| label(r, &format!("Room {r}"), None)).collect())),
            true,
        )
        .await;
        let (status, first) = run(&fx, "rooms-page", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(names(&first).len(), 25);
        assert_eq!(names(&first)[0], "Room r00");
        let cursor = first["data"]["next_cursor"].as_str().expect("a next cursor").to_owned();
        assert_eq!(decode_rooms_cursor(&cursor), Some(("room r24".to_owned(), "r24".to_owned())));
        let (_, second) = run(&fx, "rooms-page", None, Some(&cursor)).await;
        assert_eq!(names(&second), ["Room r25", "Room r26", "Room r27", "Room r28", "Room r29"]);
        assert!(second["data"]["next_cursor"].is_null(), "the last page has no cursor");
    }

    #[tokio::test]
    async fn a_malformed_or_oversized_cursor_is_a_400_before_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(rooms_ok(json!([listed("r1")])), labels(json!([label("r1", "A", None)])), true).await;
        for (index, cursor) in ["***not-base64***", "bm90LWpzb24", &encode_rooms_cursor("a", "b").replace('=', "")[..3]]
            .into_iter()
            .enumerate()
        {
            let (status, body) = run(&fx, &format!("rooms-cursor-{index}"), None, Some(cursor)).await;
            assert_eq!(status, 400, "{cursor}");
            assert_eq!(body["error"]["details"], json!({"field": "cursor", "reason": "malformed"}));
        }
        let (status, body) = run(&fx, "rooms-cursor-long", None, Some(&"c".repeat(513))).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["details"], json!({"field": "cursor", "reason": "too_long"}));
        assert!(fx.control.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_control_5xx_or_a_failed_label_read_is_a_503_never_an_empty_directory() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let down = fixture(ResponseTemplate::new(503), labels(json!([])), true).await;
        let (status, body) = run(&down, "rooms-503-control", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");

        let no_labels = fixture(rooms_ok(json!([listed("r1")])), ResponseTemplate::new(500), true).await;
        let (status, body) = run(&no_labels, "rooms-503-labels", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "directory_unavailable");
    }

    #[tokio::test]
    async fn a_control_4xx_passes_through_with_its_status_and_body() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "member_not_in_organization", "message": "no"}})),
            labels(json!([])),
            true,
        )
        .await;
        let (status, body) = run(&fx, "rooms-403", None, None).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "member_not_in_organization");
        assert!(fx.application.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn truncated_is_relayed_in_meta() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"enabled": true, "rooms": [listed("r1")], "truncated": true}})),
            labels(json!([label("r1", "A", None)])),
            true,
        )
        .await;
        let (_, body) = run(&fx, "rooms-truncated", None, None).await;
        assert_eq!(body["meta"]["truncated"], true);
    }

    #[test]
    fn the_cursor_round_trips_name_and_ref() {
        let cursor = encode_rooms_cursor("room a", "ref-1");
        assert_eq!(decode_rooms_cursor(&cursor), Some(("room a".to_owned(), "ref-1".to_owned())));
        assert_eq!(decode_rooms_cursor(""), None);
    }
}
