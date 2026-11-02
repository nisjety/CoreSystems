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
}
