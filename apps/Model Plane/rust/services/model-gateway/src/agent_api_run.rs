//! `POST /v1/agent/runs`: an API key starts one Ask turn (Wave 3 EP-12 S7, decisions D2, D5, D6).
//!
//! The key owns the thread and the run (D2): the thread is opened or continued and the managed
//! run started with the key's own `aud=session-core` bearer, so Session Core binds both to the
//! key and stamps the run `origin:"api"` from the verified caller, never from the request. The
//! answer is one model call with the key's own `aud=inference-core` bearer, made in a detached
//! task after the `202`: no tool is offered, and no retrieval, skill, memory or attachment
//! reaches it (D5). The run ends on the gateway's managed terminal path
//! (`GatewayDirect`, the gateway's own service credential), as a direct chat turn does, so a key
//! never holds a terminal RPC.
//!
//! The user-shaped Ask pipeline in `sse.rs` is deliberately not reused: it reads the person's
//! memory, retrieval, skills and connectors, every one of which needs a user-bound bearer the
//! key cannot hold. The few turn steps this route needs are composed here from the small
//! `session_flow` helpers.
//!
//! The body is a closed allow-list (`api/contracts/agent-api-v1.json`): a tool-shaped field is
//! `agent_api_feature_not_supported` naming the feature, any other unknown field is
//! `invalid_request`, both before any upstream call. A ZDR key is refused first (D6). A key runs
//! at most [`LIVE_RUNS_PER_KEY`] Ask turns at once on one replica; the ingress rate-limits too.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::{body::Bytes, extract::State, http::StatusCode, Extension, Json};
use mp_contracts::model_plane::v1::{
    ChatMessage, InferPurpose, InferRequest, InferResponse, ListConversationRequest,
    ManagedRunSource,
};
use mp_contracts::zdr_refusal::Refusal;
use mp_ids::new_ulid;
use serde_json::{json, Map, Value};
use tracing::{info, warn};

use crate::agent_api_routes::{
    admit, api_key_invalid, envelope, internal_error, invalid_request, is_id, upstream_error,
    Missing,
};
use crate::auth::{Claims, VerifiedApiKeyInferenceBearer, VerifiedApiKeySessionBearer};
use crate::http_routes::{authenticated_session_request, HttpJsonError};
use crate::session_flow::{
    self, DirectAnswer, DirectCompletion, DirectCompletionError, DirectInferenceTerminal,
    SessionRun,
};
use crate::state::AppState;

/// The fields a start body may carry (`agent-api-v1.json` `startRun.body_fields`).
const BODY_FIELDS: &[&str] = &["input", "thread_id", "model", "metadata"];

/// The tool-shaped fields a start body may not carry, each with the feature its refusal names
/// (`agent-api-v1.json` `refused_fields`). Ask only in v1 (D5).
const REFUSED_FIELDS: &[(&str, &str)] = &[
    ("tools", "tools"),
    ("tool_choice", "tools"),
    ("attachments", "attachments"),
    ("skill_ids", "skills"),
    ("connector_scope", "connectors"),
    ("retrieval_scope", "retrieval"),
    ("browse_web", "web_search"),
    ("deep_research", "deep_research"),
    ("generate_image", "image_generation"),
    ("plan_mode", "plan_mode"),
    ("execute_granted", "plan_mode"),
    ("approved_plan_id", "plan_mode"),
    ("code_execution", "code_execution"),
    ("browser", "browser"),
    ("mcp_servers", "mcp_servers"),
];

/// The body limits of `agent-api-v1.json`, counted in characters.
const INPUT_MAX_CHARS: usize = 32_000;
const MODEL_MAX_CHARS: usize = 128;
const METADATA_MAX_PAIRS: usize = 16;
const METADATA_KEY_MAX_CHARS: usize = 64;
const METADATA_VALUE_MAX_CHARS: usize = 512;

/// D5: the live Ask runs one key may hold on one replica (`limits.live_runs_per_key`).
pub(crate) const LIVE_RUNS_PER_KEY: usize = 2;

/// What `rate_limited` tells the caller to wait. An Ask turn usually ends well inside it.
const RETRY_AFTER_SECONDS: u64 = 5;

/// The turns of a continued thread the answer sees: the newest page of its active branch.
const HISTORY_MESSAGES: u32 = 40;

/// The roles the answer sees from a continued thread: the turns the key sent and the answers.
const HISTORY_ROLES: &[&str] = &["user", "assistant"];

/// The answer's output budget, the chat transport's default (`sse.rs` `DEFAULT_ANSWER_TOKENS`).
const ANSWER_MAX_TOKENS: i32 = 4096;

/// How long the model may take. Inside the 15 minutes Session Core keeps a managed run alive
/// after the liveness heartbeat, so the terminal receipt still finds the run the gateway owns.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How often the detached turn looks for a key's cancel of its run.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// The managed start key's source label: a key's Ask turn never shares a start key with a
/// person's direct turn.
const START_KEY_SOURCE: &str = "agent-api-direct";

/// The transport the usage envelope names, so a key's spend is told apart on the ledger.
const USAGE_TRANSPORT: &str = "agent_api";

/// The one fixed instruction of an Ask turn: there are no tools to call.
const ASK_SYSTEM_PROMPT: &str = "You answer through the Verevon Agent API. No tools, \
    retrieval or browsing are available in this conversation: answer from the conversation \
    alone, in plain text.";

/// `POST /v1/agent/runs`: start one Ask turn and answer `202 {data:{run_id, thread_id, status}}`
/// at once. The caller follows the run with the read routes.
pub(crate) async fn start_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: Option<Extension<VerifiedApiKeySessionBearer>>,
    inference_bearer: Option<Extension<VerifiedApiKeyInferenceBearer>>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), HttpJsonError> {
    // Key, scope, posture (a ZDR key is refused here, D6) and the session bearer, before anything.
    let session_bearer = admit(&claims, session_bearer)?;
    let inference_bearer = inference_bearer
        .map(|Extension(bearer)| bearer)
        .ok_or_else(api_key_invalid)?;
    let request = StartRequest::parse(&body)?;
    let key_id = claims.api_key_id().ok_or_else(api_key_invalid)?;
    let permit = LIVE_RUNS
        .try_acquire(live_run_key(&claims.org_id, key_id), LIVE_RUNS_PER_KEY)
        .ok_or_else(rate_limited)?;

    // The organisation's model policy, asked before a run exists, as every chat transport does,
    // so a refused model leaves no thread or run behind.
    let admitted = crate::model_access::admit(
        &state.inference_client,
        inference_request((), &inference_bearer).metadata(),
        &claims.org_id,
        request.model.as_deref().unwrap_or_default(),
        "",
    )
    .await
    .map_err(|refusal| model_refused(&refusal))?;
    let model = admitted
        .model
        .or_else(|| request.model.clone())
        .unwrap_or_default();

    // A continued thread is read first, under the key's own bearer: a thread the key does not own
    // is `thread_not_found` before anything is written, and its turns are the answer's history.
    let history = match request.thread_id.as_deref() {
        Some(thread_id) => thread_history(&state, &claims, &session_bearer, thread_id).await?,
        None => Vec::new(),
    };

    let request_id = new_ulid();
    let start_key = state.managed_start_keys.derive(
        &claims.org_id,
        &claims.sub,
        None,
        &request_id,
        START_KEY_SOURCE,
    );
    let run = session_flow::prepare_managed_run_with_token(
        &state,
        request.thread_id.as_deref(),
        None,
        &claims.org_id,
        &claims.sub,
        &request.input,
        "model-gateway",
        "execute",
        &start_key,
        ManagedRunSource::GatewayDirect,
        false,
        session_bearer.as_session_bearer().as_str(),
    )
    .await
    .map_err(|error| start_error(&error, request.thread_id.is_some()))?;
    let accepted = accepted_body(&run);
    // The start key is fresh per request, so a replayed start is never this request's to answer:
    // its first request already dispatched it.
    if !run.already_started {
        let turn = AskTurn {
            request: ask_request(
                &request_id,
                &claims.org_id,
                &run.run_id,
                &model,
                history,
                &request.input,
            ),
            run,
            request_id,
            org_id: claims.org_id.clone(),
            owner: claims.sub.clone(),
            inference_bearer,
            session_bearer,
        };
        tokio::spawn(run_ask_turn(state, turn, permit));
    }
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

/// Stop this replica's live Ask turn for `run_id` after the key's durable cancel, so the model
/// call is dropped instead of answering a cancelled run. Owner-bound: only the key that started
/// the turn reaches it. A turn on another replica sees the cancel when it tries to complete.
pub(crate) fn stop_live_turn(state: &AppState, claims: &Claims, run_id: &str) {
    state
        .cancels
        .cancel_run_for(run_id, &claims.org_id, &claims.sub);
}

/// A start body that passed the allow-list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StartRequest {
    input: String,
    thread_id: Option<String>,
    model: Option<String>,
}

impl StartRequest {
    /// The closed allow-list: a tool-shaped field is refused naming its feature, then any other
    /// unknown field, then each value against its limit.
    fn parse(body: &[u8]) -> Result<Self, HttpJsonError> {
        let Ok(Value::Object(fields)) = serde_json::from_slice::<Value>(body) else {
            return Err(invalid_request("body"));
        };
        if let Some(feature) = fields.keys().find_map(|name| refused_feature(name)) {
            return Err(feature_not_supported(feature));
        }
        if let Some(name) = fields
            .keys()
            .find(|name| !BODY_FIELDS.contains(&name.as_str()))
        {
            return Err(invalid_request(name));
        }
        let input = match fields.get("input") {
            Some(Value::String(input))
                if (1..=INPUT_MAX_CHARS).contains(&input.chars().count())
                    && !input.trim().is_empty() =>
            {
                input.clone()
            }
            _ => return Err(invalid_request("input")),
        };
        let thread_id = match fields.get("thread_id") {
            None => None,
            Some(Value::String(thread_id)) if is_id(thread_id) => Some(thread_id.clone()),
            Some(_) => return Err(invalid_request("thread_id")),
        };
        let model = match fields.get("model") {
            None => None,
            Some(Value::String(model))
                if (1..=MODEL_MAX_CHARS).contains(&model.chars().count())
                    && model.bytes().all(|byte| byte.is_ascii_graphic()) =>
            {
                Some(model.clone())
            }
            Some(_) => return Err(invalid_request("model")),
        };
        // `metadata` is checked against the contract's limits but not kept: Session Core has no
        // field for it yet (depth register, EP-12 S7).
        match fields.get("metadata") {
            None => {}
            Some(Value::Object(pairs)) if valid_metadata(pairs) => {}
            Some(_) => return Err(invalid_request("metadata")),
        }
        Ok(Self {
            input,
            thread_id,
            model,
        })
    }
}

fn refused_feature(field: &str) -> Option<&'static str> {
    REFUSED_FIELDS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, feature)| *feature)
}

fn valid_metadata(pairs: &Map<String, Value>) -> bool {
    pairs.len() <= METADATA_MAX_PAIRS
        && pairs.iter().all(|(key, value)| {
            (1..=METADATA_KEY_MAX_CHARS).contains(&key.chars().count())
                && value
                    .as_str()
                    .is_some_and(|value| value.chars().count() <= METADATA_VALUE_MAX_CHARS)
        })
}

/// One Ask turn handed to the detached task.
struct AskTurn {
    run: SessionRun,
    request: InferRequest,
    request_id: String,
    org_id: String,
    /// The key principal `apikey:<key_id>`, which owns the run.
    owner: String,
    inference_bearer: VerifiedApiKeyInferenceBearer,
    session_bearer: VerifiedApiKeySessionBearer,
}

/// The model request of an Ask turn: the fixed instruction, the thread's turns and the input.
/// No tool, no structured output, no image: a plain answer (D5).
fn ask_request(
    request_id: &str,
    org_id: &str,
    run_id: &str,
    model: &str,
    history: Vec<ChatMessage>,
    input: &str,
) -> InferRequest {
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(chat_message("system", ASK_SYSTEM_PROMPT));
    messages.extend(history);
    messages.push(chat_message("user", input));
    InferRequest {
        purpose: InferPurpose::Answer as i32,
        request_id: request_id.to_owned(),
        org_id: org_id.to_owned(),
        run_id: run_id.to_owned(),
        model: model.to_owned(),
        messages,
        max_tokens: ANSWER_MAX_TOKENS,
        // A ZDR key never gets here (D6); the persistent posture is the token's own.
        zdr: false,
        ..Default::default()
    }
}

fn chat_message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_owned(),
        content: content.to_owned(),
        name: String::new(),
        compaction_summary: String::new(),
    }
}

/// The newest page of a key-owned thread's active branch, as the answer's history.
async fn thread_history(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedApiKeySessionBearer,
    thread_id: &str,
) -> Result<Vec<ChatMessage>, HttpJsonError> {
    let request = authenticated_session_request(
        ListConversationRequest {
            org_id: claims.org_id.clone(),
            thread_id: thread_id.to_owned(),
            include_inactive: false,
            limit: HISTORY_MESSAGES,
            include_human_posts: false,
            ..Default::default()
        },
        &bearer.as_session_bearer(),
    )
    .map_err(|_| internal_error())?;
    let response = state
        .session_client
        .clone()
        .list_conversation(request)
        .await
        .map_err(|error| upstream_error(&error, Missing::Thread))?
        .into_inner();
    Ok(response
        .messages
        .into_iter()
        .filter(|message| {
            HISTORY_ROLES.contains(&message.role.as_str()) && !message.content.trim().is_empty()
        })
        .map(|message| chat_message(&message.role, &message.content))
        .collect())
}

/// The detached Ask turn: the liveness receipt, the model call, then the answer and the
/// Completed receipt in one Session Core transaction, or a Failed receipt. Every exit releases
/// the key's live-run slot and the cancel entry.
async fn run_ask_turn(state: AppState, turn: AskTurn, _permit: LiveRunPermit) {
    let AskTurn {
        run,
        request,
        request_id,
        org_id,
        owner,
        inference_bearer,
        session_bearer,
    } = turn;
    let cancelled = state
        .cancels
        .register_run(&request_id, &org_id, &owner, &run.run_id);
    let _registration = state.cancels.hold(&request_id);

    // Fail closed before the provider call: a run Session Core no longer counts as live is not
    // answered. Its lease then ends and Session Core's recovery closes it.
    if let Err(error) = session_flow::ensure_direct_inference_run_liveness(&state, &run).await {
        warn!(%error, run_id = %run.run_id, "agent api run liveness heartbeat failed");
        return;
    }

    let started = Instant::now();
    let mut client = state.inference_client.clone();
    let call = client.infer(inference_request(request, &inference_bearer));
    let answered = tokio::select! {
        answered = tokio::time::timeout(ANSWER_TIMEOUT, call) => answered,
        () = until_cancelled(&cancelled) => {
            info!(run_id = %run.run_id, "agent api run cancelled during its model call");
            return;
        }
    };
    let infer = match answered {
        Ok(Ok(response)) => response.into_inner(),
        Ok(Err(status)) => {
            warn!(code = ?status.code(), run_id = %run.run_id, "agent api model call failed");
            fail_run(&state, &run, &session_bearer, failure_terminal(&status)).await;
            return;
        }
        Err(_) => {
            warn!(run_id = %run.run_id, "agent api model call timed out");
            fail_run(
                &state,
                &run,
                &session_bearer,
                DirectInferenceTerminal::Failed("provider_timeout"),
            )
            .await;
            return;
        }
    };
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    publish_usage(&state, &request_id, &org_id, &owner, &infer, latency_ms).await;
    if cancelled.load(Ordering::Relaxed) {
        info!(run_id = %run.run_id, "agent api run cancelled before its answer was saved");
        return;
    }

    let answer = DirectAnswer {
        content: &infer.content,
        agent_name: None,
        metadata: None,
    };
    match session_flow::complete_direct_inference_run_with_message(
        &state,
        &run,
        answer,
        &request_id,
        &session_bearer.as_session_bearer(),
        None,
    )
    .await
    {
        Ok(DirectCompletion::Completed) => {
            info!(run_id = %run.run_id, thread_id = %run.thread_id, "agent api run completed");
        }
        Ok(DirectCompletion::AlreadyTerminal) => {
            info!(run_id = %run.run_id, "agent api run was already terminal; answer not saved");
        }
        Err(DirectCompletionError::Persist(error)) => {
            warn!(
                error = %session_flow::session_error_chain(&error),
                run_id = %run.run_id,
                "agent api answer could not be saved"
            );
            fail_run(
                &state,
                &run,
                &session_bearer,
                DirectInferenceTerminal::Failed("assistant_persist_failed"),
            )
            .await;
        }
        Err(DirectCompletionError::Terminal(error)) => {
            warn!(
                error = %session_flow::session_error_chain(&error),
                run_id = %run.run_id,
                "agent api answer saved but its terminal receipt failed"
            );
        }
    }
}

/// Resolves once the key's cancel of the run reached this replica.
async fn until_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// The Failed receipt for a model call that did not answer, in the direct-run vocabulary: an
/// unreachable or overloaded provider, or a request the provider or the policy refused.
fn failure_terminal(status: &tonic::Status) -> DirectInferenceTerminal {
    match status.code() {
        tonic::Code::Unavailable | tonic::Code::ResourceExhausted => {
            DirectInferenceTerminal::Failed("inference_unavailable")
        }
        tonic::Code::DeadlineExceeded => DirectInferenceTerminal::Failed("provider_timeout"),
        _ => DirectInferenceTerminal::Failed("inference_failed"),
    }
}

/// Record a Failed receipt through the managed terminal path (the gateway's own credential).
async fn fail_run(
    state: &AppState,
    run: &SessionRun,
    bearer: &VerifiedApiKeySessionBearer,
    terminal: DirectInferenceTerminal,
) {
    if let Err(error) = session_flow::terminalize_direct_inference_run_authenticated(
        state,
        run,
        terminal,
        &bearer.as_session_bearer(),
    )
    .await
    {
        warn!(%error, run_id = %run.run_id, "agent api run failure receipt could not be recorded");
    }
}

/// The usage envelope cost-core meters, attributed to the key principal. Persistent only: a ZDR
/// key never starts a run.
async fn publish_usage(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    owner: &str,
    infer: &InferResponse,
    latency_ms: u64,
) {
    crate::grpc::publish_usage_envelope_via(
        state,
        request_id,
        org_id,
        infer,
        latency_ms,
        owner,
        false,
        0,
        crate::pricing::CostBasis::Metered.served_by(&infer.provider_used),
        USAGE_TRANSPORT,
    )
    .await;
}

/// An inference-core request carrying the key's own inference bearer.
fn inference_request<T>(value: T, bearer: &VerifiedApiKeyInferenceBearer) -> tonic::Request<T> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", bearer.as_str())
            .parse()
            .expect("a verified compact JWT is valid gRPC metadata"),
    );
    request
}

/// `202`'s body: the run is queued until the detached turn starts the model call.
fn accepted_body(run: &SessionRun) -> Value {
    json!({
        "data": {
            "run_id": run.run_id,
            "thread_id": run.thread_id,
            "status": "queued",
        }
    })
}

/// A failed start in the Agent API's vocabulary: Session Core's status when the chain carries
/// one, otherwise a gateway failure. A not-found, refused or invalid answer names the caller's
/// thread only when the caller named one; without one, Session Core refusing the key's own new
/// thread is the gateway's failure, not the caller's.
fn start_error(error: &anyhow::Error, thread_named: bool) -> HttpJsonError {
    let Some(status) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<tonic::Status>())
    else {
        return internal_error();
    };
    let names_the_thread = matches!(
        status.code(),
        tonic::Code::NotFound | tonic::Code::PermissionDenied | tonic::Code::InvalidArgument
    );
    if names_the_thread && !thread_named && Refusal::from_status(status).is_none() {
        return internal_error();
    }
    upstream_error(status, Missing::Thread)
}

/// The model policy's refusal in the Agent API's envelope, `details` always an object.
fn model_refused(refusal: &mp_contracts::model_entitlement::Refusal) -> HttpJsonError {
    let details = refusal.reason().map_or_else(
        || json!({}),
        |reason| json!({ "model": refusal.model(), "reason": reason.as_str() }),
    );
    envelope(
        StatusCode::from_u16(refusal.http_status()).unwrap_or(StatusCode::FORBIDDEN),
        refusal.code(),
        &refusal.message(),
        details,
    )
}

fn feature_not_supported(feature: &str) -> HttpJsonError {
    envelope(
        StatusCode::UNPROCESSABLE_ENTITY,
        "agent_api_feature_not_supported",
        "the Agent API does not support this feature in version 1",
        json!({ "feature": feature }),
    )
}

fn rate_limited() -> HttpJsonError {
    envelope(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "too many live runs for this API key; try again later",
        json!({ "retry_after_seconds": RETRY_AFTER_SECONDS }),
    )
}

/// The live-run slot of one key in one organisation.
fn live_run_key(org_id: &str, key_id: &str) -> String {
    format!("{org_id}\u{1f}{key_id}")
}

/// The live Ask runs of each key on this replica (D5). In memory on purpose: a durable count
/// would need a store, and the ingress rate limit stands in front of every replica.
#[derive(Default)]
struct LiveRuns(Mutex<HashMap<String, usize>>);

static LIVE_RUNS: LazyLock<Arc<LiveRuns>> = LazyLock::new(Arc::default);

impl LiveRuns {
    /// A slot for `key` while it holds fewer than `cap`, released when the permit is dropped.
    fn try_acquire(self: &Arc<Self>, key: String, cap: usize) -> Option<LiveRunPermit> {
        let mut live = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let count = live.entry(key.clone()).or_insert(0);
        if *count >= cap {
            return None;
        }
        *count += 1;
        Some(LiveRunPermit {
            runs: Arc::clone(self),
            key,
        })
    }

    fn release(&self, key: &str) {
        let mut live = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = live.get_mut(key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                live.remove(key);
            }
        }
    }
}

/// One held live-run slot. Dropped on every exit of the start handler or of the detached turn,
/// a panic included.
#[must_use = "dropping the permit frees the key's live-run slot"]
struct LiveRunPermit {
    runs: Arc<LiveRuns>,
    key: String,
}

impl Drop for LiveRunPermit {
    fn drop(&mut self) {
        self.runs.release(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::API_KEY_SCOPE_AGENT_RUNS;
    use axum::{body::Body, http::Request as HttpRequest, middleware, Router};
    use tower::ServiceExt;

    const CONTRACT: &str = include_str!("../../../../api/contracts/agent-api-v1.json");

    fn contract() -> Value {
        serde_json::from_str(CONTRACT).expect("the Agent API contract parses")
    }

    fn parse(body: Value) -> Result<StartRequest, HttpJsonError> {
        StartRequest::parse(body.to_string().as_bytes())
    }

    fn assert_typed(error: &HttpJsonError, status: StatusCode, code: &str) {
        assert_eq!(error.0, status, "{}", error.1 .0);
        assert_eq!(error.1["error"]["code"], code, "{}", error.1 .0);
        assert!(error.1["error"]["message"].is_string());
        assert!(
            error.1["error"]["details"].is_object(),
            "details is an object"
        );
    }

    fn key_claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "apikey:key-1",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": zdr,
            "principal_type": "api_key",
            "key_id": "key-1",
            "acting_user_id": "user-a",
            "scopes": [API_KEY_SCOPE_AGENT_RUNS],
        }))
        .expect("an api key token decodes")
    }

    #[test]
    fn body_allow_list_is_closed() {
        let golden = contract();
        let start = golden["routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|route| route["operation_id"] == "startRun")
            .expect("the start route is in the contract");
        let fields: Vec<&str> = start["body_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        assert_eq!(fields, BODY_FIELDS, "the allow-list is the contract's");
        let limits = &golden["limits"];
        assert_eq!(limits["input_max_chars"], INPUT_MAX_CHARS);
        assert_eq!(limits["model_max_chars"], MODEL_MAX_CHARS);
        assert_eq!(limits["metadata_max_pairs"], METADATA_MAX_PAIRS);
        assert_eq!(limits["metadata_key_max_chars"], METADATA_KEY_MAX_CHARS);
        assert_eq!(limits["metadata_value_max_chars"], METADATA_VALUE_MAX_CHARS);
        assert_eq!(limits["live_runs_per_key"], LIVE_RUNS_PER_KEY);

        let full = parse(json!({
            "input": "What is the capital of Norway?",
            "thread_id": "thread_01-AB",
            "model": "claude-sonnet-5-5",
            "metadata": { "ticket": "T-1" },
        }))
        .expect("every allowed field");
        assert_eq!(full.input, "What is the capital of Norway?");
        assert_eq!(full.thread_id.as_deref(), Some("thread_01-AB"));
        assert_eq!(full.model.as_deref(), Some("claude-sonnet-5-5"));
        let bare = parse(json!({ "input": "hi" })).expect("input alone");
        assert_eq!((bare.thread_id, bare.model), (None, None));

        // An unknown field, a field the run would trust (its origin), and every bad value.
        let long_input = "x".repeat(INPUT_MAX_CHARS + 1);
        let long_value = "v".repeat(METADATA_VALUE_MAX_CHARS + 1);
        let many_pairs: Map<String, Value> = (0..=METADATA_MAX_PAIRS)
            .map(|index| (format!("k{index}"), json!("v")))
            .collect();
        for (body, field) in [
            (json!({ "input": "hi", "temperature": 0.2 }), "temperature"),
            (json!({ "input": "hi", "origin": "chat" }), "origin"),
            (json!({ "input": "hi", "user_id": "user-b" }), "user_id"),
            (json!({}), "input"),
            (json!({ "input": "" }), "input"),
            (json!({ "input": "  \n " }), "input"),
            (json!({ "input": 7 }), "input"),
            (json!({ "input": long_input }), "input"),
            (json!({ "input": "hi", "thread_id": "a/b" }), "thread_id"),
            (json!({ "input": "hi", "thread_id": null }), "thread_id"),
            (json!({ "input": "hi", "model": "" }), "model"),
            (json!({ "input": "hi", "model": "a model" }), "model"),
            (
                json!({ "input": "hi", "model": "m".repeat(MODEL_MAX_CHARS + 1) }),
                "model",
            ),
            (json!({ "input": "hi", "metadata": ["a"] }), "metadata"),
            (json!({ "input": "hi", "metadata": { "n": 1 } }), "metadata"),
            (
                json!({ "input": "hi", "metadata": { "": "v" } }),
                "metadata",
            ),
            (
                json!({ "input": "hi", "metadata": { "k": long_value } }),
                "metadata",
            ),
            (json!({ "input": "hi", "metadata": many_pairs }), "metadata"),
        ] {
            let error = parse(body.clone()).expect_err(field);
            assert_typed(&error, StatusCode::BAD_REQUEST, "invalid_request");
            assert_eq!(error.1["error"]["details"]["field"], field, "{body}");
        }
        let raws: [&[u8]; 4] = [b"", b"not json", b"[1]", b"\"input\""];
        for raw in raws {
            let error = StartRequest::parse(raw).expect_err("not an object");
            assert_eq!(error.1["error"]["details"]["field"], "body");
        }
        // Exactly at the limits is allowed.
        let at_limit = parse(json!({ "input": "x".repeat(INPUT_MAX_CHARS) }));
        assert!(at_limit.is_ok());
    }

    #[test]
    fn tool_fields_are_refused_with_feature() {
        let golden = contract();
        let refused = golden["refused_fields"].as_object().unwrap();
        assert_eq!(refused.len(), REFUSED_FIELDS.len());
        for (field, feature) in refused {
            assert_eq!(refused_feature(field), feature.as_str(), "{field}");
            let mut body = json!({ "input": "hi" });
            body[field.as_str()] = json!(true);
            let error = parse(body).expect_err("a tool-shaped field is refused");
            assert_typed(
                &error,
                StatusCode::UNPROCESSABLE_ENTITY,
                "agent_api_feature_not_supported",
            );
            assert_eq!(error.1["error"]["details"]["feature"], *feature);
        }
        // The feature refusal wins over an unknown field and a bad value in the same body.
        let error = parse(json!({ "tools": [], "temperature": 1 })).expect_err("tools");
        assert_eq!(error.1["error"]["code"], "agent_api_feature_not_supported");
        // Every feature named is in the contract's closed list.
        let features: Vec<&str> = golden["refused_features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|feature| feature.as_str().unwrap())
            .collect();
        for (_, feature) in REFUSED_FIELDS {
            assert!(features.contains(feature), "{feature}");
        }
    }

    #[test]
    fn no_tools_are_offered() {
        let history = vec![
            chat_message("user", "earlier question"),
            chat_message("assistant", "earlier answer"),
        ];
        let request = ask_request("req-1", "org-a", "run-1", "", history, "the question");
        assert!(request.tools.is_empty(), "no tool is offered");
        assert!(request.tool_choice.is_empty());
        assert!(request.structured_output_schema.is_empty());
        assert!(
            request.image_parts.is_empty(),
            "no attachment reaches the model"
        );
        assert!(request.subscription_connection_id.is_empty());
        assert!(!request.zdr);
        assert_eq!(request.purpose, InferPurpose::Answer as i32);
        assert_eq!(request.run_id, "run-1");
        assert_eq!(request.org_id, "org-a");
        let turns: Vec<(&str, &str)> = request
            .messages
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            turns,
            [
                ("system", ASK_SYSTEM_PROMPT),
                ("user", "earlier question"),
                ("assistant", "earlier answer"),
                ("user", "the question"),
            ]
        );
        assert_eq!(
            golden_tools_offered(),
            0,
            "the contract offers no tool either"
        );
    }

    fn golden_tools_offered() -> usize {
        contract()["policy"]["tools_offered"]
            .as_array()
            .map_or(usize::MAX, Vec::len)
    }

    /// The run's `origin:"api"` stamp is Session Core's, from the verified key bearer the start
    /// carries (EP-12 S4); the gateway forwards no origin of its own, and a body that tries to
    /// name one is refused before any call.
    #[test]
    fn run_is_stamped_api() {
        let error = parse(json!({ "input": "hi", "origin": "chat" })).expect_err("origin");
        assert_eq!(error.1["error"]["details"]["field"], "origin");
        let error = parse(json!({ "input": "hi", "metadata": { "origin": 1 } }))
            .expect_err("a non-string metadata value");
        assert_eq!(error.1["error"]["details"]["field"], "metadata");
        // The start key is the key principal's own: a key's start never shares one with a person.
        assert_ne!(START_KEY_SOURCE, "gateway-direct");
    }

    #[test]
    fn third_live_run_is_rate_limited() {
        let runs = Arc::new(LiveRuns::default());
        let first = runs.try_acquire(live_run_key("org-a", "key-1"), LIVE_RUNS_PER_KEY);
        let second = runs.try_acquire(live_run_key("org-a", "key-1"), LIVE_RUNS_PER_KEY);
        assert!(first.is_some() && second.is_some());
        assert!(
            runs.try_acquire(live_run_key("org-a", "key-1"), LIVE_RUNS_PER_KEY)
                .is_none(),
            "a third live run of one key is refused"
        );
        // Another key, and the same key id in another organisation, have their own slots.
        let other_key = runs.try_acquire(live_run_key("org-a", "key-2"), LIVE_RUNS_PER_KEY);
        let other_org = runs.try_acquire(live_run_key("org-b", "key-1"), LIVE_RUNS_PER_KEY);
        assert!(other_key.is_some() && other_org.is_some());
        // A run that ends frees its slot.
        drop(first);
        let again = runs.try_acquire(live_run_key("org-a", "key-1"), LIVE_RUNS_PER_KEY);
        assert!(again.is_some(), "a freed slot is reusable");
        drop((second, again, other_key, other_org));
        assert!(
            runs.0.lock().unwrap().is_empty(),
            "no slot outlives its runs"
        );

        let error = rate_limited();
        assert_typed(&error, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
        assert_eq!(
            error.1["error"]["details"]["retry_after_seconds"],
            RETRY_AFTER_SECONDS
        );
    }

    /// The route as production layers it, minus the JWT check: `claims` and the key's two
    /// bearers stand in for what `require_auth` inserts. Its clients reach nothing, so a request
    /// that got past the gates would answer `upstream_unavailable`.
    fn app(claims: Claims, with_inference_bearer: bool) -> Router {
        use mp_contracts::model_plane::v1::{
            inference_core_client::InferenceCoreClient, run_service_client::RunServiceClient,
            session_core_client::SessionCoreClient,
        };
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1")
            .connect_timeout(Duration::from_secs(2))
            .connect_lazy();
        let mut state = AppState::new();
        state.run_client = RunServiceClient::new(channel.clone());
        state.session_client = SessionCoreClient::new(channel.clone());
        state.inference_client = InferenceCoreClient::new(channel);
        let router = crate::agent_api_routes::router(true)
            .layer(middleware::from_fn(crate::auth::authorize_principal_route))
            .layer(Extension(VerifiedApiKeySessionBearer::for_test(
                "key-session-token",
            )));
        let router = if with_inference_bearer {
            router.layer(Extension(VerifiedApiKeyInferenceBearer::for_test(
                "key-inference-token",
            )))
        } else {
            router
        };
        router.layer(Extension(claims)).with_state(state)
    }

    async fn post(router: Router, body: Value) -> (StatusCode, Value) {
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/v1/agent/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn zdr_key_refused() {
        std::env::set_var(crate::agent_api_routes::ENABLED_ENV, "true");
        // Even a body that names a tool: the ZDR answer comes first, before any upstream call.
        let (status, body) = post(
            app(key_claims(true), true),
            json!({ "input": "hi", "tools": [] }),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["details"]["feature"], "agent_api");

        // A persistent key without its inference bearer is not a usable key.
        let (status, body) = post(app(key_claims(false), false), json!({ "input": "hi" })).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(body["error"]["code"], "api_key_invalid");

        // A persistent key with a tool-shaped body is refused before any upstream call.
        let (status, body) = post(
            app(key_claims(false), true),
            json!({ "input": "hi", "browser": true }),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["error"]["details"]["feature"], "browser");

        // A thread the key names is read before anything is written: the (absent) Session Core
        // answers, so nothing past it ran.
        let (status, body) = post(
            app(key_claims(false), true),
            json!({ "input": "hi", "thread_id": "thread-1" }),
        )
        .await;
        std::env::remove_var(crate::agent_api_routes::ENABLED_ENV);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"]["code"], "upstream_unavailable");
    }

    /// The run ends on the managed terminal path: a failed model call maps onto the direct-run
    /// failure vocabulary Session Core accepts from `GatewayDirect`, and the start names that
    /// source, so the gateway's own credential (not the key) records the receipt.
    #[test]
    fn run_completes_through_managed_terminal() {
        use tonic::{Code, Status};
        for (code, expected) in [
            (Code::Unavailable, "inference_unavailable"),
            (Code::ResourceExhausted, "inference_unavailable"),
            (Code::DeadlineExceeded, "provider_timeout"),
            (Code::PermissionDenied, "inference_failed"),
            (Code::InvalidArgument, "inference_failed"),
            (Code::Internal, "inference_failed"),
        ] {
            assert_eq!(
                failure_terminal(&Status::new(code, "provider text")),
                DirectInferenceTerminal::Failed(expected),
                "{code:?}"
            );
        }
        let run = SessionRun {
            thread_id: "thread-1".to_owned(),
            run_id: "run-1".to_owned(),
            org_id: "org-a".to_owned(),
            already_started: false,
            terminal_source: ManagedRunSource::GatewayDirect,
        };
        assert_eq!(
            accepted_body(&run),
            json!({ "data": { "run_id": "run-1", "thread_id": "thread-1", "status": "queued" } })
        );
        let golden = contract();
        let accepted: Vec<&str> = golden["response_fields"]["StartRunAccepted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        let body = accepted_body(&run);
        let mut keys: Vec<&str> = body["data"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = accepted.clone();
        expected.sort_unstable();
        assert_eq!(keys, expected, "the contract's StartRunAccepted fields");
    }

    #[test]
    fn start_errors_and_refusals_are_typed_envelopes() {
        let status = anyhow::Error::new(tonic::Status::not_found("upstream text"))
            .context("session-core append_message(user) failed");
        let error = start_error(&status, true);
        assert_typed(&error, StatusCode::NOT_FOUND, "thread_not_found");
        assert!(!error.1 .0.to_string().contains("upstream text"));
        let error = start_error(&status, false);
        assert_typed(&error, StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        let error = start_error(&anyhow::anyhow!("no status"), true);
        assert_typed(&error, StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        let unavailable = anyhow::Error::new(tonic::Status::unavailable("down"));
        let error = start_error(&unavailable, false);
        assert_typed(
            &error,
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_unavailable",
        );
        let zdr = anyhow::Error::new(
            Refusal::feature_not_supported("agent_api")
                .grpc_status(tonic::Code::FailedPrecondition),
        );
        let error = start_error(&zdr, false);
        assert_typed(
            &error,
            StatusCode::PRECONDITION_FAILED,
            "zdr_feature_not_supported",
        );

        use mp_contracts::model_entitlement::{Reason, Refusal as ModelRefusal};
        let error = model_refused(&ModelRefusal::not_entitled(
            "claude-sonnet-5-5",
            Reason::NotInAllowList,
        ));
        assert_typed(&error, StatusCode::FORBIDDEN, "model_not_entitled");
        assert_eq!(error.1["error"]["details"]["reason"], "not_in_allow_list");
        let error = model_refused(&ModelRefusal::unavailable());
        assert_typed(
            &error,
            StatusCode::SERVICE_UNAVAILABLE,
            "model_entitlement_unavailable",
        );
        assert_typed(
            &feature_not_supported("tools"),
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_api_feature_not_supported",
        );
    }

    #[tokio::test]
    async fn a_cancel_reaches_only_the_keys_own_live_turn() {
        let state = AppState::new();
        let flag = state
            .cancels
            .register_run("req-1", "org-a", "apikey:key-1", "run-1");
        let mut person: Claims = key_claims(false);
        person.sub = "user-a".to_owned();
        stop_live_turn(&state, &person, "run-1");
        assert!(!flag.load(Ordering::Relaxed), "another principal's cancel");
        stop_live_turn(&state, &key_claims(false), "run-2");
        assert!(!flag.load(Ordering::Relaxed), "another run");
        stop_live_turn(&state, &key_claims(false), "run-1");
        assert!(flag.load(Ordering::Relaxed), "the key's own run");
        state.cancels.finish("req-1");
    }
}
