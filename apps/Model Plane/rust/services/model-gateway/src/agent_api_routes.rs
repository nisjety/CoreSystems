//! `/v1/agent/*`: the Agent API read and control routes (Wave 3 EP-12 S5, decisions D4, D6, D12).
//!
//! An API key starts (EP-12 S7, `agent_api_run`), reads and controls its own runs here: one run,
//! its live events, a cancel, and a key-owned thread's runs and messages. Every route forwards
//! the key's own `aud=session-core` bearer (`VerifiedApiKeySessionBearer`, verified by
//! `auth::require_auth`) and never a header the caller chose; Session Core binds every call to
//! the threads and runs the key owns (EP-12 S4), so a run or thread of anyone else reads as not
//! found.
//!
//! The routes are for keys only (D4: people use the chat routes). The route policy admits a key
//! here only while `MODEL_GATEWAY_AGENT_API` is on and only with scope `agent:runs`; the handlers
//! check both again, and refuse a zero-retention key before any upstream call (D6), because a ZDR
//! run keeps no durable row to read. While the switch is off the routes are not merged at all, so
//! every answer is what it was before.
//!
//! Every success is `{data}` (a list adds cursor `meta` and `links`); every error is the typed
//! envelope `{error:{code,message,details}}` with `details` an object, in the closed vocabulary of
//! `api/contracts/agent-api-v1.json`.

use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    CancelRunRequest, GetRunRequest, ListConversationRequest, ListRunsRequest, RunDetail,
};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::{json, Map, Value};

use crate::auth::{Claims, VerifiedApiKeySessionBearer, API_KEY_SCOPE_AGENT_RUNS};
use crate::http_routes::{
    authenticated_session_request, run_detail_value, timestamp_to_rfc3339, HttpJsonError,
};
use crate::state::AppState;
use crate::zdr_posture::{self, Route, TurnPosture};

/// The gateway's switch for the Agent API. Opt-in: only the word `true` enables it. The route
/// policy reads it at each call; the router reads it once, when it is built.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_AGENT_API";

/// The ZDR feature key a refusal names (`zdr-refusals.json`, EP-12 D6).
const ZDR_FEATURE: &str = "agent_api";

/// The page size when a list names none, and the most it may name (`agent-api-v1.json` limits).
const PAGE_LIMIT_DEFAULT: u32 = 20;
const PAGE_LIMIT_MAX: u32 = 100;

/// The longest run id, thread id or cursor a caller may send (`agent-api-v1.json` limits).
const MAX_ID_CHARS: usize = 128;

/// What a key's cancel records as its reason on the durable `RUN_CANCELLED` event.
const CANCEL_REASON: &str = "api_key_requested";

/// The only roles a key reads in a thread: the turns it sent and the answers.
const MESSAGE_ROLES: &[&str] = &["user", "assistant"];

/// Whether the Agent API is on in this process.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The six routes, merged into the authed router only while `enabled` (the switch) is on.
pub(crate) fn router(enabled: bool) -> Router<AppState> {
    if !enabled {
        return Router::new();
    }
    Router::new()
        // EP-12 S7: start an Ask run.
        .route("/v1/agent/runs", post(crate::agent_api_run::start_run))
        .route("/v1/agent/runs/:run_id", get(get_run))
        .route("/v1/agent/runs/:run_id/events", get(run_events))
        .route("/v1/agent/runs/:run_id/cancel", post(cancel_run))
        .route("/v1/agent/threads/:thread_id/runs", get(list_thread_runs))
        .route(
            "/v1/agent/threads/:thread_id/messages",
            get(list_thread_messages),
        )
}

/// The refusal of a key without `agent:runs`. The route policy answers it before any handler,
/// and the handlers answer it again.
pub(crate) fn scope_missing() -> HttpJsonError {
    envelope(
        StatusCode::FORBIDDEN,
        "agent_api_scope_missing",
        "the API key does not hold the permission this route needs",
        json!({ "scope": API_KEY_SCOPE_AGENT_RUNS }),
    )
}

/// `GET /v1/agent/runs/:run_id`: one run the key owns, as `{data:{run}}`.
async fn get_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let bearer = admit(&claims, bearer)?;
    let run_id = path_id(&run_id, "run_id")?;
    let run = fetch_run(&state, &bearer, &run_id).await?;
    Ok(Json(json!({ "data": { "run": run_value(&run) } })))
}

/// `GET /v1/agent/runs/:run_id/events`: the run's live events, from the run-events stream the
/// chat routes use. The run is read first, under the key's own bearer, so a run the key does not
/// own is a typed `run_not_found` and no stream is opened for it. The stream itself carries the
/// same bearer, and Session Core checks the owner again when it opens the stream.
async fn run_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpJsonError> {
    let bearer = admit(&claims, bearer)?;
    let run_id = path_id(&run_id, "run_id")?;
    fetch_run(&state, &bearer, &run_id).await?;
    Ok(crate::sse::run_events_sse(
        State(state),
        Path(run_id),
        headers,
        Extension(claims),
        bearer.as_session_bearer(),
    )
    .await
    .into_response())
}

/// `POST /v1/agent/runs/:run_id/cancel`: Session Core's authoritative cancel. Its transaction
/// decides ownership and terminal state, and its `RUN_CANCELLED` event id is the receipt. A run
/// this replica is still answering (EP-12 S7) is then told to stop, so a cancelled run spends no
/// more on its model call.
async fn cancel_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let bearer = admit(&claims, bearer)?;
    let run_id = path_id(&run_id, "run_id")?;
    let cancellation = state
        .run_client
        .clone()
        .cancel_run(session_request(
            CancelRunRequest {
                run_id: run_id.clone(),
                reason: CANCEL_REASON.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| upstream_error(&error, Missing::Run))?
        .into_inner();
    if cancellation.cancelled {
        crate::agent_api_run::stop_live_turn(&state, &claims, &run_id);
    }
    let status = if cancellation.cancelled {
        "cancelled"
    } else {
        "already_terminal"
    };
    Ok(Json(json!({
        "data": {
            "run_id": run_id,
            "status": status,
            "cancelled": cancellation.cancelled,
            "receipt_id": cancellation.receipt_id,
        }
    })))
}

/// `GET /v1/agent/threads/:thread_id/runs?after&limit`: one page of a key-owned thread's runs,
/// through the unchanged `ListRuns` (D12). The cursor is the last run id of the page.
async fn list_thread_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    Path(thread_id): Path<String>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<Json<Value>, HttpJsonError> {
    let bearer = admit(&claims, bearer)?;
    let thread_id = path_id(&thread_id, "thread_id")?;
    let page = PageQuery::parse(query, "after")?;
    let response = state
        .run_client
        .clone()
        .list_runs(session_request(
            ListRunsRequest {
                thread_id: thread_id.clone(),
                after_run_id: page.cursor.clone().unwrap_or_default(),
                limit: page.limit,
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|error| upstream_error(&error, Missing::Thread))?
        .into_inner();
    let next_cursor = response
        .has_more
        .then(|| response.runs.last().map(|run| run.run_id.clone()))
        .flatten();
    let runs = response.runs.iter().map(run_value).collect::<Vec<_>>();
    Ok(Json(page_body(
        runs,
        response.has_more,
        next_cursor.as_deref(),
        &format!("/threads/{thread_id}/runs"),
        "after",
        page.limit,
    )))
}

/// `GET /v1/agent/threads/:thread_id/messages?before&limit`: one page of a key-owned thread's
/// active branch, newest page first, user and assistant turns only. No human post is asked for.
/// The cursor is the oldest turn of the page as Session Core returned it, so a turn of another
/// role at the page edge never makes the next page skip or repeat a turn.
async fn list_thread_messages(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    Path(thread_id): Path<String>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<Json<Value>, HttpJsonError> {
    let bearer = admit(&claims, bearer)?;
    let thread_id = path_id(&thread_id, "thread_id")?;
    let page = PageQuery::parse(query, "before")?;
    let response = state
        .session_client
        .clone()
        .list_conversation(session_request(
            ListConversationRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.clone(),
                include_inactive: false,
                limit: page.limit,
                before_message_id: page.cursor.clone().unwrap_or_default(),
                include_human_posts: false,
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|error| upstream_error(&error, Missing::Thread))?
        .into_inner();
    let next_cursor = response
        .has_more
        .then(|| {
            response
                .messages
                .first()
                .map(|message| message.message_id.clone())
        })
        .flatten();
    let messages = response
        .messages
        .into_iter()
        .filter(|message| MESSAGE_ROLES.contains(&message.role.as_str()))
        .map(|message| {
            let created_at = Some(timestamp_to_rfc3339(message.created_at))
                .filter(|at| !at.is_empty())
                .map_or(Value::Null, Value::String);
            json!({
                "message_id": message.message_id,
                "role": message.role,
                "content": message.content,
                "created_at": created_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(page_body(
        messages,
        response.has_more,
        next_cursor.as_deref(),
        &format!("/threads/{thread_id}/messages"),
        "before",
        page.limit,
    )))
}

/// The gate every route passes before anything else: the caller is a key (D4), it holds
/// `agent:runs`, its retention posture is persistent (D6), and its own session bearer came with
/// it. The ZDR refusal comes before the bearer check, so no upstream call is ever made for a ZDR
/// key whatever else it sent.
pub(crate) fn admit(
    claims: &Claims,
    bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
) -> Result<VerifiedApiKeySessionBearer, HttpJsonError> {
    if claims.api_key_id().is_none() {
        return Err(api_key_invalid());
    }
    if !claims.has_scope(API_KEY_SCOPE_AGENT_RUNS) {
        return Err(scope_missing());
    }
    zdr_gate(claims)?;
    bearer
        .map(|Extension(bearer)| bearer)
        .ok_or_else(api_key_invalid)
}

/// D6: only a persistent posture is admitted. A ZDR key, and a posture the resolver cannot decide
/// or refuses, is the `agent_api` feature refusal (fail closed).
fn zdr_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    match zdr_posture::decide_for(claims, &Route::plain(false)) {
        Ok(TurnPosture::Persistent) => Ok(()),
        _ => Err(zdr_refused()),
    }
}

/// The ZDR refusal in the Agent API's envelope: the catalogue code, status and sentence, with the
/// feature under `details` as the public contract has it.
fn zdr_refused() -> HttpJsonError {
    let refusal = Refusal::feature_not_supported(ZDR_FEATURE);
    envelope(
        zdr_posture::http_status(&refusal),
        refusal.code(),
        &refusal.message(),
        json!({ "feature": ZDR_FEATURE }),
    )
}

/// A credential that is not a usable key: not a key at all, or a key without its session bearer.
pub(crate) fn api_key_invalid() -> HttpJsonError {
    envelope(
        StatusCode::UNAUTHORIZED,
        "api_key_invalid",
        "a valid API key is required",
        json!({}),
    )
}

/// A path id as Session Core writes them: 1 to 128 ASCII letters, digits, `-` or `_`. Anything
/// else is refused here, so it never reaches Session Core or a `links.next` value.
fn path_id(raw: &str, field: &str) -> Result<String, HttpJsonError> {
    if is_id(raw) {
        Ok(raw.to_owned())
    } else {
        Err(invalid_request(field))
    }
}

pub(crate) fn is_id(raw: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The query of a list route: the cursor under its one name and `limit`. The names are a closed
/// set: an unknown or repeated name, a malformed query, a cursor that is not an id or a limit
/// outside 1 to 100 is `invalid_request` naming the value.
#[derive(Debug, PartialEq, Eq)]
struct PageQuery {
    cursor: Option<String>,
    limit: u32,
}

impl PageQuery {
    fn parse(
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        cursor_name: &str,
    ) -> Result<Self, HttpJsonError> {
        let Ok(Query(pairs)) = query else {
            return Err(invalid_request("query"));
        };
        let mut cursor = None;
        let mut limit = None;
        for (name, value) in pairs {
            if name == cursor_name && cursor.is_none() {
                if !is_id(&value) {
                    return Err(invalid_request(cursor_name));
                }
                cursor = Some(value);
            } else if name == "limit" && limit.is_none() {
                let parsed = value
                    .parse::<u32>()
                    .ok()
                    .filter(|limit| (1..=PAGE_LIMIT_MAX).contains(limit))
                    .ok_or_else(|| invalid_request("limit"))?;
                limit = Some(parsed);
            } else {
                return Err(invalid_request(&name));
            }
        }
        Ok(Self {
            cursor,
            limit: limit.unwrap_or(PAGE_LIMIT_DEFAULT),
        })
    }
}

/// A list body: `{data, meta:{has_more,next_cursor}, links:{next}}`. `links.next` is the same
/// request with the next cursor, relative to the API root, and null on the last page. The path
/// and cursor were checked as ids, so they need no escaping.
fn page_body(
    data: Vec<Value>,
    has_more: bool,
    next_cursor: Option<&str>,
    path: &str,
    cursor_name: &str,
    limit: u32,
) -> Value {
    let next = next_cursor.map(|cursor| format!("{path}?{cursor_name}={cursor}&limit={limit}"));
    json!({
        "data": data,
        "meta": { "has_more": has_more, "next_cursor": next_cursor },
        "links": { "next": next },
    })
}

/// The public run: the fields of `agent-api-v1.json`'s `Run`, taken from the projection the chat
/// routes serve (`run_detail_value`), and of its metadata only `origin` and `api_key_id`. Every
/// other key the run read model carries stays inside the platform.
fn run_value(run: &RunDetail) -> Value {
    const RUN_FIELDS: &[&str] = &[
        "run_id",
        "thread_id",
        "parent_run_id",
        "status",
        "error",
        "input_tokens",
        "output_tokens",
        "created_at",
        "updated_at",
    ];
    const METADATA_FIELDS: &[&str] = &["origin", "api_key_id"];
    let full = run_detail_value(run);
    let mut public = Map::new();
    for field in RUN_FIELDS {
        public.insert((*field).to_owned(), full[*field].clone());
    }
    let metadata = METADATA_FIELDS
        .iter()
        .filter_map(|field| {
            full["metadata"][*field]
                .as_str()
                .map(|value| ((*field).to_owned(), json!(value)))
        })
        .collect::<Map<_, _>>();
    public.insert("metadata".to_owned(), Value::Object(metadata));
    Value::Object(public)
}

/// The run read under the key's own bearer.
async fn fetch_run(
    state: &AppState,
    bearer: &VerifiedApiKeySessionBearer,
    run_id: &str,
) -> Result<RunDetail, HttpJsonError> {
    state
        .run_client
        .clone()
        .get_run(session_request(
            GetRunRequest {
                run_id: run_id.to_owned(),
            },
            bearer,
        )?)
        .await
        .map(tonic::Response::into_inner)
        .map_err(|error| upstream_error(&error, Missing::Run))
}

/// A Session Core request carrying the key's session bearer.
fn session_request<T>(
    value: T,
    bearer: &VerifiedApiKeySessionBearer,
) -> Result<tonic::Request<T>, HttpJsonError> {
    authenticated_session_request(value, &bearer.as_session_bearer()).map_err(|_| internal_error())
}

/// Which id a not-found answer names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Missing {
    Run,
    Thread,
}

/// Session Core's refusals in the Agent API's vocabulary. A run or thread the key does not own
/// reads as one that does not exist. A ZDR refusal from upstream is the `agent_api` refusal.
pub(crate) fn upstream_error(error: &tonic::Status, missing: Missing) -> HttpJsonError {
    if Refusal::from_status(error).is_some() {
        return zdr_refused();
    }
    match error.code() {
        tonic::Code::NotFound | tonic::Code::PermissionDenied => match missing {
            Missing::Run => envelope(
                StatusCode::NOT_FOUND,
                "run_not_found",
                "run not found",
                json!({}),
            ),
            Missing::Thread => envelope(
                StatusCode::NOT_FOUND,
                "thread_not_found",
                "thread not found",
                json!({}),
            ),
        },
        tonic::Code::InvalidArgument => invalid_request(match missing {
            Missing::Run => "run_id",
            Missing::Thread => "thread_id",
        }),
        tonic::Code::Unauthenticated => api_key_invalid(),
        tonic::Code::Unavailable
        | tonic::Code::DeadlineExceeded
        | tonic::Code::ResourceExhausted => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_unavailable",
            "a dependency is unavailable; try again",
            json!({}),
        ),
        _ => internal_error(),
    }
}

pub(crate) fn invalid_request(field: &str) -> HttpJsonError {
    envelope(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "the request is not valid",
        json!({ "field": field }),
    )
}

pub(crate) fn internal_error() -> HttpJsonError {
    envelope(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "the gateway failed unexpectedly",
        json!({}),
    )
}

/// A typed error body: `{error:{code,message,details}}`, with `details` always an object.
pub(crate) fn envelope(
    status: StatusCode,
    code: &str,
    message: &str,
    details: Value,
) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest, middleware};
    use tower::ServiceExt;

    fn key_claims(scopes: &[&str], zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "apikey:key-1",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": zdr,
            "principal_type": "api_key",
            "key_id": "key-1",
            "acting_user_id": "user-a",
            "scopes": scopes,
        }))
        .expect("an api key token decodes")
    }

    fn user_claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a",
            "user_id": "user-a",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": false,
            "scopes": [API_KEY_SCOPE_AGENT_RUNS],
        }))
        .expect("a user token decodes")
    }

    fn key_bearer() -> Option<Extension<VerifiedApiKeySessionBearer>> {
        Some(Extension(VerifiedApiKeySessionBearer::for_test(
            "key-session-token",
        )))
    }

    /// Every route's concrete path and method, as a caller sends them.
    const ROUTES: &[(&str, &str)] = &[
        ("POST", "/v1/agent/runs"),
        ("GET", "/v1/agent/runs/run-1"),
        ("GET", "/v1/agent/runs/run-1/events"),
        ("POST", "/v1/agent/runs/run-1/cancel"),
        ("GET", "/v1/agent/threads/thread-1/runs"),
        ("GET", "/v1/agent/threads/thread-1/messages"),
    ];

    /// A state whose Session Core clients point at a closed local port (not the default, where a
    /// developer's Session Core may be listening), so a request that reached Session Core answers
    /// `upstream_unavailable`.
    fn unreachable_state() -> AppState {
        use mp_contracts::model_plane::v1::{
            run_service_client::RunServiceClient, session_core_client::SessionCoreClient,
        };
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1")
            .connect_timeout(std::time::Duration::from_secs(2))
            .connect_lazy();
        let mut state = AppState::new();
        state.run_client = RunServiceClient::new(channel.clone());
        state.session_client = SessionCoreClient::new(channel);
        state
    }

    /// The router as production layers it, minus the JWT check: `claims` and the key's session
    /// bearer stand in for what `require_auth` inserts.
    fn app(enabled: bool, claims: Claims, with_bearer: bool) -> Router {
        let router =
            router(enabled).layer(middleware::from_fn(crate::auth::authorize_principal_route));
        let router = if with_bearer {
            router.layer(Extension(VerifiedApiKeySessionBearer::for_test(
                "key-session-token",
            )))
        } else {
            router
        };
        router
            .layer(Extension(claims))
            .with_state(unreachable_state())
    }

    async fn send(router: Router, method: &str, path: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    fn assert_typed(body: &Value, code: &str) {
        assert_eq!(body["error"]["code"], code, "{body}");
        assert!(body["error"]["message"].is_string(), "{body}");
        assert!(
            body["error"]["details"].is_object(),
            "details is an object: {body}"
        );
    }

    #[tokio::test]
    async fn switch_off_routes_are_absent() {
        for (method, path) in ROUTES {
            let (status, _) = send(
                app(false, key_claims(&[API_KEY_SCOPE_AGENT_RUNS], false), true),
                method,
                path,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        }
    }

    #[test]
    fn api_key_needs_agent_runs_scope() {
        let error = admit(&key_claims(&["browser:agent"], false), key_bearer())
            .expect_err("a key without agent:runs is refused");
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        assert_typed(&error.1, "agent_api_scope_missing");
        assert_eq!(
            error.1["error"]["details"]["scope"],
            API_KEY_SCOPE_AGENT_RUNS
        );
        assert!(admit(
            &key_claims(&[API_KEY_SCOPE_AGENT_RUNS], false),
            key_bearer()
        )
        .is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn zdr_key_is_refused_before_upstream() {
        // The route policy reads the process switch, so it is set for this test. The handlers'
        // clients reach nothing: a request that got past the gate would answer
        // `upstream_unavailable`, so the 412 proves no upstream call was made.
        std::env::set_var(ENABLED_ENV, "true");
        for (method, path) in ROUTES {
            let (status, body) = send(
                app(true, key_claims(&[API_KEY_SCOPE_AGENT_RUNS], true), true),
                method,
                path,
            )
            .await;
            assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{method} {path}");
            assert_typed(&body, "zdr_feature_not_supported");
        }
        // The same key, persistent: past the gate, so it reaches the (absent) upstream.
        let (status, body) = send(
            app(true, key_claims(&[API_KEY_SCOPE_AGENT_RUNS], false), true),
            "GET",
            "/v1/agent/runs/run-1",
        )
        .await;
        std::env::remove_var(ENABLED_ENV);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_typed(&body, "upstream_unavailable");
        let error = admit(&key_claims(&[API_KEY_SCOPE_AGENT_RUNS], true), key_bearer())
            .expect_err("a ZDR key is refused");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        assert_typed(&error.1, "zdr_feature_not_supported");
        assert_eq!(error.1["error"]["details"]["feature"], ZDR_FEATURE);
        // Even without its session bearer, the ZDR answer comes first.
        let error = admit(&key_claims(&[API_KEY_SCOPE_AGENT_RUNS], true), None)
            .expect_err("a ZDR key is refused");
        assert_typed(&error.1, "zdr_feature_not_supported");
    }

    #[test]
    fn session_bearer_must_match_the_key() {
        // `require_auth` inserts the key's session bearer only after binding it to the same key,
        // org and posture (auth::tests); a key that arrives without one is refused here.
        let error = admit(&key_claims(&[API_KEY_SCOPE_AGENT_RUNS], false), None)
            .expect_err("no key session bearer");
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);
        assert_typed(&error.1, "api_key_invalid");
    }

    #[tokio::test]
    async fn user_principal_cannot_use_agent_routes() {
        let error = admit(&user_claims(), key_bearer()).expect_err("a person is not a key");
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);
        assert_typed(&error.1, "api_key_invalid");
        // Through the router too: a person passes the route policy and the handler refuses it
        // before any upstream call.
        for (method, path) in ROUTES {
            let (status, body) = send(app(true, user_claims(), false), method, path).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
            assert_typed(&body, "api_key_invalid");
        }
    }

    #[test]
    fn errors_are_typed_envelopes() {
        let status = |code: tonic::Code| tonic::Status::new(code, "upstream text");
        use tonic::Code;
        use Missing::{Run, Thread};
        for (code, missing, http, typed) in [
            (Code::NotFound, Run, StatusCode::NOT_FOUND, "run_not_found"),
            (
                Code::PermissionDenied,
                Run,
                StatusCode::NOT_FOUND,
                "run_not_found",
            ),
            (
                Code::NotFound,
                Thread,
                StatusCode::NOT_FOUND,
                "thread_not_found",
            ),
            (
                Code::PermissionDenied,
                Thread,
                StatusCode::NOT_FOUND,
                "thread_not_found",
            ),
            (
                Code::InvalidArgument,
                Run,
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                Code::Unauthenticated,
                Run,
                StatusCode::UNAUTHORIZED,
                "api_key_invalid",
            ),
            (
                Code::Unavailable,
                Thread,
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream_unavailable",
            ),
            (
                Code::DeadlineExceeded,
                Run,
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream_unavailable",
            ),
            (
                Code::Internal,
                Run,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            ),
        ] {
            let (got, body) = upstream_error(&status(code), missing);
            assert_eq!(got, http, "{code:?}");
            assert_typed(&body, typed);
            assert!(
                !body.to_string().contains("upstream text"),
                "an upstream message never reaches the caller"
            );
        }
        let refusal = Refusal::feature_not_supported(ZDR_FEATURE)
            .grpc_status(tonic::Code::FailedPrecondition);
        let (got, body) = upstream_error(&refusal, Missing::Run);
        assert_eq!(got, StatusCode::PRECONDITION_FAILED);
        assert_typed(&body, "zdr_feature_not_supported");

        // The local refusals as well.
        for (error, code) in [
            (scope_missing(), "agent_api_scope_missing"),
            (api_key_invalid(), "api_key_invalid"),
            (zdr_refused(), "zdr_feature_not_supported"),
            (invalid_request("limit"), "invalid_request"),
            (internal_error(), "internal_error"),
        ] {
            assert_typed(&error.1, code);
        }
    }

    #[test]
    fn path_ids_and_page_queries_are_closed() {
        assert_eq!(path_id("run_01-AB", "run_id").unwrap(), "run_01-AB");
        for bad in ["", "run.1", "run 1", "a/b", "run%2F1", &"r".repeat(129)] {
            let error = path_id(bad, "run_id").expect_err(bad);
            assert_typed(&error.1, "invalid_request");
            assert_eq!(error.1["error"]["details"]["field"], "run_id");
        }

        let parse = |pairs: &[(&str, &str)]| {
            PageQuery::parse(
                Ok(Query(
                    pairs
                        .iter()
                        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                        .collect(),
                )),
                "after",
            )
        };
        assert_eq!(
            parse(&[]).unwrap(),
            PageQuery {
                cursor: None,
                limit: PAGE_LIMIT_DEFAULT
            }
        );
        assert_eq!(
            parse(&[("after", "run-9"), ("limit", "100")]).unwrap(),
            PageQuery {
                cursor: Some("run-9".to_owned()),
                limit: 100
            }
        );
        for (pairs, field) in [
            (vec![("limit", "0")], "limit"),
            (vec![("limit", "101")], "limit"),
            (vec![("limit", "ten")], "limit"),
            (vec![("after", "a.b")], "after"),
            (vec![("before", "run-9")], "before"),
            (vec![("status", "running")], "status"),
            (vec![("after", "a"), ("after", "b")], "after"),
            (vec![("limit", "5"), ("limit", "6")], "limit"),
        ] {
            let error = parse(&pairs).expect_err(field);
            assert_typed(&error.1, "invalid_request");
            assert_eq!(error.1["error"]["details"]["field"], field, "{pairs:?}");
        }
    }

    #[test]
    fn page_bodies_carry_meta_and_links() {
        let last = page_body(vec![json!(1)], false, None, "/threads/t1/runs", "after", 20);
        assert_eq!(
            last["meta"],
            json!({ "has_more": false, "next_cursor": null })
        );
        assert_eq!(last["links"], json!({ "next": null }));
        let more = page_body(
            Vec::new(),
            true,
            Some("m-1"),
            "/threads/t1/messages",
            "before",
            5,
        );
        assert_eq!(more["data"], json!([]));
        assert_eq!(more["meta"]["next_cursor"], "m-1");
        assert_eq!(
            more["links"]["next"],
            "/threads/t1/messages?before=m-1&limit=5"
        );
    }

    #[test]
    fn run_projection_matches_run_detail_value() {
        let run = RunDetail {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            status: "completed".to_owned(),
            goal: "a goal the public run does not carry".to_owned(),
            final_output: "the answer is read from the thread".to_owned(),
            error: String::new(),
            input_tokens: 12,
            output_tokens: 34,
            created_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
            metadata: Some(prost_types::Struct {
                fields: [
                    ("origin", "api"),
                    ("api_key_id", "key-1"),
                    ("api_key_owner", "user-a"),
                    ("retention_receipt", "r-1"),
                ]
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.to_owned(),
                        prost_types::Value {
                            kind: Some(prost_types::value::Kind::StringValue(value.to_owned())),
                        },
                    )
                })
                .collect(),
            }),
            ..Default::default()
        };
        let full = run_detail_value(&run);
        let public = run_value(&run);
        let keys: Vec<&String> = public.as_object().unwrap().keys().collect();
        assert_eq!(
            keys.len(),
            10,
            "the contract's Run fields and nothing else: {keys:?}"
        );
        for field in [
            "run_id",
            "thread_id",
            "parent_run_id",
            "status",
            "error",
            "input_tokens",
            "output_tokens",
            "created_at",
            "updated_at",
        ] {
            assert_eq!(public[field], full[field], "{field}");
        }
        assert_eq!(
            public["metadata"],
            json!({ "origin": "api", "api_key_id": "key-1" }),
            "the key owner and every other stamp stay inside"
        );
        assert!(public.get("goal").is_none() && public.get("final_output").is_none());

        let bare = run_value(&RunDetail::default());
        assert_eq!(
            bare["metadata"],
            json!({}),
            "no metadata is an empty object, not null"
        );
    }
}
