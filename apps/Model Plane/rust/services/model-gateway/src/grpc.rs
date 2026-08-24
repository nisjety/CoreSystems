//! gRPC server implementing the `ModelGateway` service on :9090.
//!
//! The gateway orchestrates:
//!  1. `MemoryService.SearchMemory` — best-effort retrieval of relevant
//!     memory entries scoped to the thread/org/query.
//!  2. `InferenceCore.Infer` / `InferStream` — provider routing for the
//!     final LLM call, with memory results injected as system context.

use chrono::Utc;
use mp_contracts::model_plane::v1::{
    model_gateway_server::{ModelGateway, ModelGatewayServer},
    AppendThreadMessageRequest, AppendThreadMessageResponse, ApproveApprovalRequest,
    ApproveApprovalResponse, ChatMessage, CheckPermissionRequest, CheckPermissionResponse,
    CreateTaskRequest, CreateTaskResponse, DenyApprovalRequest, DenyApprovalResponse,
    EnterPlanModeRequest, EnterPlanModeResponse, ExecuteCommandRequest, ExecuteCommandResponse,
    ExitPlanModeRequest, ExitPlanModeResponse, ExportTrajectoriesRequest,
    ExportTrajectoriesResponse, ExtractStructuredRequest, ExtractStructuredResponse, FetchRequest,
    FetchResponse, GetAnalyticsRequest, GetAnalyticsResponse, GetPolicyRequest, GetPolicyResponse,
    GetSkillRequest, GetSkillResponse, HealthRequest, HealthResponse, InferChunk, InferRequest,
    InferResponse, InvokeChunk, InvokeRequest, InvokeResponse, IsPlanModeRequest,
    IsPlanModeResponse, ListCommandsRequest, ListCommandsResponse, ListHooksRequest,
    ListHooksResponse, ListMcpServersRequest, ListMcpServersResponse, ListMcpToolsRequest,
    ListMcpToolsResponse, ListPendingApprovalsRequest, ListPendingApprovalsResponse,
    ListPluginsRequest, ListPluginsResponse, ListSkillsRequest, ListSkillsResponse,
    ListTasksRequest, ListTasksResponse, ListThreadMessagesRequest, ListThreadMessagesResponse,
    ListTrajectoriesRequest, ListTrajectoriesResponse, LspQueryRequest, LspQueryResponse,
    MatchSkillsRequest, MatchSkillsResponse, ProxyMcpToolRequest, ProxyMcpToolResponse,
    RecordTrajectoryRequest, RecordTrajectoryResponse, RegisterHookRequest, RegisterHookResponse,
    RegisterMcpServerRequest, RegisterMcpServerResponse, RegisterPluginRequest,
    RegisterPluginResponse, RemoteTriggerRequest, RemoteTriggerResponse,
    RenderHints as ProtoRenderHints, RequestApprovalRequest, RequestApprovalResponse,
    SearchMemoryRequest, SendMessageRequest, SendMessageResponse, SetPermissionRequest,
    SetPermissionResponse, SetPluginEnabledRequest, SetPluginEnabledResponse, SetPolicyRequest,
    SetPolicyResponse, SleepRequest, SleepResponse, SpeechToTextRequest, SpeechToTextResponse,
    SynthesizeSpeechRequest, SyntheticOutputRequest, SyntheticOutputResponse, TeamCreateRequest,
    TeamCreateResponse, TeamDeleteRequest, TeamDeleteResponse, TeamListRequest, TeamListResponse,
    TextToSpeechRequest, TextToSpeechResponse, TranscribeSpeechRequest, WebSearchRequest,
    WebSearchResponse,
};
use mp_contracts::model_plane::v1::{
    Command, EvaluatePolicyRequest, OrgPolicy, Plugin, TaskRecord,
};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::{
    approvals, coordinator,
    grpc_auth::{self, RpcAccess, VerifiedIdentity},
    lsp,
    quarry::{QuarryError, RenderHints},
    runtime_registries, session_flow, skills,
    state::AppState,
    tools, trajectory,
};

trait TenantScopedRequest {
    fn requested_org_id(&self) -> &str;
}

macro_rules! direct_tenant_requests {
    ($($request:ty),+ $(,)?) => {
        $(
            impl TenantScopedRequest for $request {
                fn requested_org_id(&self) -> &str {
                    &self.org_id
                }
            }
        )+
    };
}

direct_tenant_requests!(
    InvokeRequest,
    WebSearchRequest,
    SleepRequest,
    RemoteTriggerRequest,
    SendMessageRequest,
    SyntheticOutputRequest,
    EnterPlanModeRequest,
    ExitPlanModeRequest,
    IsPlanModeRequest,
    TeamCreateRequest,
    TeamDeleteRequest,
    TeamListRequest,
    LspQueryRequest,
    RequestApprovalRequest,
    ApproveApprovalRequest,
    DenyApprovalRequest,
    ListPendingApprovalsRequest,
    ListTrajectoriesRequest,
    ExportTrajectoriesRequest,
    ListSkillsRequest,
    GetSkillRequest,
    MatchSkillsRequest,
    RegisterMcpServerRequest,
    ListMcpServersRequest,
    ListMcpToolsRequest,
    ProxyMcpToolRequest,
    RegisterPluginRequest,
    ListPluginsRequest,
    SetPluginEnabledRequest,
    ListCommandsRequest,
    ExecuteCommandRequest,
    RegisterHookRequest,
    ListHooksRequest,
    CheckPermissionRequest,
    SetPermissionRequest,
    GetPolicyRequest,
    AppendThreadMessageRequest,
    ListThreadMessagesRequest,
    GetAnalyticsRequest,
    TextToSpeechRequest,
    SpeechToTextRequest,
    CreateTaskRequest,
    ListTasksRequest,
    FetchRequest,
    ExtractStructuredRequest,
);

impl TenantScopedRequest for RecordTrajectoryRequest {
    fn requested_org_id(&self) -> &str {
        self.trajectory
            .as_ref()
            .map_or("", |trajectory| trajectory.org_id.as_str())
    }
}

impl TenantScopedRequest for SetPolicyRequest {
    fn requested_org_id(&self) -> &str {
        self.policy
            .as_ref()
            .map_or("", |policy| policy.org_id.as_str())
    }
}

/// A string field from a capability-core JSON row, or empty.
fn json_str(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Policy fields that still have no owning service.
///
/// The spend/token ceilings now live in Control Plane (`org_quotas`), so only
/// `allowed_models` remains homeless. It is refused by name rather than banked
/// locally: accepting a model allowlist that nothing consults would tell an
/// operator they had restricted models when they had not.
fn unowned_policy_fields(policy: &OrgPolicy) -> Vec<&'static str> {
    let mut unowned = Vec::new();
    if !policy.allowed_models.trim().is_empty() {
        unowned.push("allowed_models");
    }
    unowned
}

/// Map one capability-core task row onto the gateway's wire `TaskRecord`.
/// `created_at` crosses as a unix second count; an unparseable or absent
/// timestamp becomes 0 rather than "now", so a missing value cannot read as a
/// task that was just created.
fn task_from_catalog(row: &serde_json::Value) -> TaskRecord {
    TaskRecord {
        task_id: json_str(row, "id"),
        org_id: json_str(row, "org_id"),
        description: json_str(row, "description"),
        status: json_str(row, "status"),
        parent_run_id: json_str(row, "parent_run_id"),
        cron: json_str(row, "cron"),
        created_at_unix: row
            .get("created_at")
            .and_then(serde_json::Value::as_str)
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map_or(0, |parsed| parsed.timestamp()),
    }
}

/// Map one capability-core command row onto the gateway's wire `Command`.
/// capability-core names the executor `handler`; this contract calls it
/// `tool_name`.
fn command_from_catalog(row: &serde_json::Value) -> Command {
    Command {
        command_id: json_str(row, "id"),
        name: json_str(row, "name"),
        description: json_str(row, "description"),
        tool_name: json_str(row, "handler"),
        // capability-core's catalogue has no remote-URL or default-payload
        // concept; leaving these empty is honest rather than inventing values.
        remote_url: String::new(),
        default_payload_json: String::new(),
    }
}

/// Convert the gateway's JSON arg object into capability-core's flat
/// `map[string]string`.
///
/// # Errors
/// Returns a message naming the offending field when a value is not a string.
/// Stringifying a nested object here would hand capability-core an argument the
/// caller never wrote, so a mismatch is refused instead.
fn command_args(args_json: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let trimmed = args_json.trim();
    if trimmed.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| format!("args_json is not valid JSON: {error}"))?;
    let Some(object) = parsed.as_object() else {
        return Err("args_json must be a JSON object".to_owned());
    };
    object
        .iter()
        .map(|(key, value)| match value {
            serde_json::Value::String(text) => Ok((key.clone(), text.clone())),
            // Numbers and booleans have one unambiguous textual form, so they
            // cross safely; structures do not.
            serde_json::Value::Number(number) => Ok((key.clone(), number.to_string())),
            serde_json::Value::Bool(flag) => Ok((key.clone(), flag.to_string())),
            _ => Err(format!(
                "argument '{key}' must be a string, number, or boolean"
            )),
        })
        .collect()
}

/// Map one capability-core `plugin_packages` row onto the gateway's wire
/// `Plugin`. The manifest URL round-trips inside `manifest_json` because the
/// catalog stores a manifest document while this contract carries a URL.
fn plugin_from_catalog(row: &serde_json::Value) -> Plugin {
    Plugin {
        plugin_id: json_str(row, "id"),
        name: json_str(row, "name"),
        version: json_str(row, "version"),
        kind: json_str(row, "description"),
        manifest_url: row
            .get("manifest_json")
            .and_then(|m| m.get("manifest_url"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        enabled: row
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        status: json_str(row, "rollout_state"),
        installed_at_unix: row
            .get("installed_at_unix")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
    }
}

/// The caller's own `authorization` header, for forwarding to a plane the
/// gateway is proxying to. A BFF acts with the caller's authority, not with an
/// ambient service identity — so an upstream can apply its own tenant checks
/// rather than trusting whatever org the gateway names.
fn forwarded_authorization<T>(request: &Request<T>) -> String {
    request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[allow(clippy::result_large_err)]
fn authorize_rpc<T: TenantScopedRequest>(
    request: &Request<T>,
    access: RpcAccess,
) -> Result<VerifiedIdentity, Status> {
    grpc_auth::authorize_request(request, request.get_ref().requested_org_id(), access)
}

#[allow(clippy::result_large_err)]
fn require_explicit_zdr_contract(
    identity: &VerifiedIdentity,
    operation: &'static str,
) -> Result<(), Status> {
    if identity.effective_zdr(false) {
        return Err(Status::failed_precondition(format!(
            "{operation} is unavailable under zero-retention policy until its downstream contract carries ZDR"
        )));
    }
    Ok(())
}

/// Resolve the caller-selected run against Session Core before a gateway RPC
/// can mutate local run-scoped state or publish a run event. The request body
/// contributes only the run id; tenant and user are always the verified ingress
/// identity, and the dedicated Session Core bearer is forwarded only after it
/// was independently verified by the gateway interceptor.
#[allow(clippy::result_large_err)]
async fn require_durable_run_owner(
    state: &AppState,
    identity: &VerifiedIdentity,
    run_id: &str,
) -> Result<(), Status> {
    let user_id = identity
        .user_id()
        .ok_or_else(|| Status::permission_denied("a verified user must mutate a run"))?;
    session_flow::require_durable_run_owner_with_token(
        state,
        run_id,
        identity.org_id(),
        user_id,
        identity.session_bearer()?,
    )
    .await
}

/// Maximum page content (markdown chars) forwarded to inference-core
/// for `ExtractStructured`. Tuned to stay well below the 128k context
/// floor every in-use model shares, leaving headroom for the schema +
/// system prompt + caller instructions.
const MAX_EXTRACT_CONTENT_CHARS: usize = 80_000;

pub(crate) struct GatewayService {
    state: AppState,
}

fn request_id_or_new(request_id: &str) -> String {
    if request_id.trim().is_empty() {
        new_ulid()
    } else {
        request_id.to_owned()
    }
}

/// Derive the durable managed-start retry identity for both unary and streaming
/// gRPC invocation. A client-provided idempotency key wins over the tracing
/// request id so a response-loss retry cannot create a second provider call.
fn managed_start_key(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    request_id: &str,
    idempotency_key: &str,
    managed_source: &str,
) -> String {
    state.managed_start_keys.derive(
        org_id,
        user_id,
        (!idempotency_key.trim().is_empty()).then_some(idempotency_key),
        request_id,
        managed_source,
    )
}

fn model_or_default(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        "default".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Best-effort memory search. Returns the matched memory contents or an
/// empty `Vec` on any error (memory is advisory context — inference still
/// proceeds when the memory backend is unavailable).
async fn fetch_memory_context(
    state: &AppState,
    identity: &VerifiedIdentity,
    thread_id: &str,
    org_id: &str,
    query: &str,
) -> Vec<String> {
    let mut client = state.memory_client.clone();
    let request = SearchMemoryRequest {
        thread_id: thread_id.to_owned(),
        query: query.to_owned(),
        topic_filter: Vec::new(),
        limit: 5,
        org_id: org_id.to_owned(),
        updated_after: None,
    };
    let Ok(request) = identity.session_request(request) else {
        warn!("session-core credential missing; continuing without memory context");
        return Vec::new();
    };
    match client.search_memory(request).await {
        Ok(resp) => resp
            .into_inner()
            .entries
            .into_iter()
            .map(|e| e.content)
            .filter(|c| !c.trim().is_empty())
            .collect(),
        Err(error) => {
            warn!(%error, "memory search failed; continuing without context");
            Vec::new()
        }
    }
}

fn build_messages(memory_context: &[String], user_content: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(2);
    if !memory_context.is_empty() {
        let joined = memory_context.join("\n---\n");
        messages.push(ChatMessage {
            role: "system".to_owned(),
            content: format!("Relevant memory:\n{joined}"),
            name: String::new(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: user_content.to_owned(),
        name: String::new(),
    });
    messages
}

fn build_infer_request(
    request_id: &str,
    org_id: &str,
    model: &str,
    provider_hint: &str,
    messages: Vec<ChatMessage>,
    req: &InvokeRequest,
) -> Result<InferRequest, Status> {
    // Fail closed at the edge on an unknown tier numeric, mirroring the HTTP
    // normalize() path: a NEWER gRPC caller naming a tier this build does not
    // know must be refused rather than silently read as "no constraint"
    // downstream. UNSPECIFIED (0) passes through unchanged — it imposes no
    // floor, exactly like an absent field.
    mp_contracts::model_plane::v1::PrivacyTier::try_from(req.min_privacy_tier).map_err(|_| {
        Status::invalid_argument(format!(
            "unknown privacy tier value: {}",
            req.min_privacy_tier
        ))
    })?;
    Ok(InferRequest {
        request_id: request_id.to_owned(),
        org_id: org_id.to_owned(),
        model: model.to_owned(),
        provider_hint: provider_hint.to_owned(),
        messages,
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        structured_output_schema: req.structured_output_schema.clone(),
        zdr: req.zdr,
        // Same floor the HTTP transports thread; enforced fail-closed by
        // inference-core's chain selection.
        min_privacy_tier: req.min_privacy_tier,
        ..Default::default()
    })
}

async fn publish_ingress_accepted(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    model: &str,
    content_length: usize,
    user_id: &str,
    zdr: bool,
) {
    if zdr {
        return;
    }
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "INGRESS_ACCEPTED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: request_id.to_owned(),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "content_length": content_length,
            "model": model,
            "transport": "grpc",
        }),
        zdr,
    };

    if let Err(error) = state
        .publisher
        .publish(&subjects::ingress_subject("accepted"), &envelope)
        .await
    {
        tracing::warn!(%error, "failed to publish INGRESS_ACCEPTED");
    }
}

/// Publish the per-request usage envelope (best-effort; publish failures are logged).
async fn publish_usage_envelope(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    infer: &InferResponse,
    latency_ms: u64,
    user_id: &str,
    zdr: bool,
    min_privacy_tier: i32,
) {
    if zdr {
        return;
    }
    let usage_envelope = Envelope {
        event_id: new_ulid(),
        event_type: "USAGE_ENVELOPE".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: format!("{request_id}-USAGE_ENVELOPE"),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "request_id": request_id,
            "org_id": org_id,
            "user_id": user_id,
            "model": infer.model_used.clone(),
            "input_tokens": infer.input_tokens,
            "output_tokens": infer.output_tokens,
            "latency_ms": latency_ms,
            "transport": "grpc",
            // Phase-4 provenance receipt inputs, matching the HTTP envelopes:
            // which deployment processed the content and under what
            // residency / requested privacy floor.
            "provider_used": infer.provider_used.clone(),
            "residency": infer.residency.clone(),
            "min_privacy_tier": min_privacy_tier,
        }),
        zdr,
    };

    if let Err(error) = state
        .publisher
        .publish(&subjects::usage_subject(org_id), &usage_envelope)
        .await
    {
        warn!(%error, "failed to publish grpc USAGE_ENVELOPE");
    }
}

// invoke/invoke_stream are sequential request pipelines (session preamble + cache +
// inference/streaming relay); per-method #[allow] doesn't survive the async_trait
// macro expansion, so the allow lives on the impl block. Both read clearer inline.
impl GatewayService {
    /// Write one org quota to Control Plane.
    ///
    /// Authenticates as a SERVICE (`x-service-id`/`x-service-token`), not with
    /// the caller's token: the gRPC caller holds a Model Plane audience token
    /// that org-core does not accept. The caller's authority is still checked —
    /// `authorize_rpc` gated this RPC before we got here — and org-core applies
    /// its own scope check on the service credential.
    ///
    /// # This write is refused by org-core, and the credential cannot fix it
    ///
    /// `PUT .../quotas/:key` requires `org:settings:write:self`, and org-core
    /// rejects any `:self` scope held by a principal other than
    /// `verevon-gateway` — in `validateServiceCredential`, which runs from
    /// `main.go` BEFORE any listener. Adding that scope to this service's
    /// registry entry would not grant the write; it would stop Control Plane's
    /// org-core from booting at all. This service is therefore registered with
    /// `org:read:any` only, which is exactly what reading and enforcing a
    /// ceiling needs.
    ///
    /// That is not a gap to route around. The route also sits behind org-core's
    /// membership guard and demands a verified v3 HMAC delegation naming the
    /// acting user, because setting an org's spend cap is a governed admin
    /// action, not something a service should do on its own authority. The
    /// Verevon gateway already holds that scope, mints the delegation, and
    /// exposes the write at `PUT /api/v1/orgs/:id/quotas/:key` (its
    /// `domains/orgs/quotas.rs`) — so the capability exists, with an operator
    /// behind it.
    ///
    /// So this arm returns org-core's 403: an honest failure with a working
    /// alternative one plane over. Teaching model-gateway to sign its own
    /// delegation would duplicate Control-Plane-facing authority the gateway
    /// already owns.
    async fn put_org_quota(&self, org_id: &str, key: &str, limit: i64) -> Result<(), Status> {
        if self.state.org_core_base_url.trim().is_empty()
            || self.state.org_core_service_token.trim().is_empty()
        {
            // Refuse rather than no-op: an operator told the cap was accepted
            // while it was never stored is the failure this whole cleanup
            // exists to remove.
            return Err(Status::unavailable(
                "org-core credentials are not configured; the spend ceiling cannot be recorded",
            ));
        }
        let url = format!(
            "{}/api/v1/organizations/{org_id}/quotas/{key}",
            self.state.org_core_base_url.trim_end_matches('/')
        );
        let response = self
            .state
            .http_client
            .put(&url)
            .header("x-service-id", &self.state.org_core_service_id)
            .header("x-service-token", &self.state.org_core_service_token)
            .json(&serde_json::json!({ "limit": limit }))
            .send()
            .await
            .map_err(|error| Status::unavailable(format!("org-core unreachable: {error}")))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(Status::internal(format!(
                "org-core rejected the quota write for '{key}': HTTP {status}"
            )));
        }
        Ok(())
    }

    /// One authenticated call to capability-core's HTTP API, forwarding the
    /// caller's own credential so capability-core applies its own tenant
    /// checks rather than trusting an org this gateway names.
    async fn capability_json(
        &self,
        method: reqwest::Method,
        path: &str,
        bearer: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, Status> {
        if self.state.capability_core_base_url.is_empty() {
            return Err(Status::unavailable("capability registry is unavailable"));
        }
        let url = format!("{}{path}", self.state.capability_core_base_url);
        let mut builder = self
            .state
            .http_client
            .request(method, &url)
            .bearer_auth(bearer.trim_start_matches("Bearer ").trim());
        if let Some(body) = body {
            builder = builder.json(&body);
        }
        let response = builder.send().await.map_err(|error| {
            Status::unavailable(format!("capability registry unreachable: {error}"))
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(Status::internal(format!(
                "capability registry returned HTTP {status}"
            )));
        }
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|error| Status::internal(format!("capability registry sent no JSON: {error}")))
    }
}

#[tonic::async_trait]
#[allow(clippy::too_many_lines)]
impl ModelGateway for GatewayService {
    async fn invoke(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<InvokeResponse>, Status> {
        let started = std::time::Instant::now();
        let identity = authorize_rpc(&request, RpcAccess::Invoke)?;
        let mut req = request.into_inner();
        req.zdr = identity.effective_zdr(req.zdr);
        let content = req.content.trim().to_owned();
        if content.is_empty() {
            return Err(Status::invalid_argument("content must not be empty"));
        }

        let request_id = request_id_or_new(&req.request_id);
        let model = model_or_default(&req.model);
        let org_id = req.org_id.clone();
        let user_id = identity
            .user_id()
            .unwrap_or_else(|| identity.principal_id());
        let start_key = managed_start_key(
            &self.state,
            &org_id,
            user_id,
            &request_id,
            &req.idempotency_key,
            "gateway-direct",
        );
        let session_run = session_flow::prepare_managed_run_with_token(
            &self.state,
            Some(&req.thread_id),
            Some(&req.session_key),
            &org_id,
            user_id,
            &content,
            "model-gateway",
            "execute",
            &start_key,
            mp_contracts::model_plane::v1::ManagedRunSource::GatewayDirect,
            req.zdr,
            identity.session_bearer()?,
        )
        .await
        .map_err(|error| {
            Status::unavailable(format!("session-core managed start failed: {error}"))
        })?;
        if session_run.already_started {
            return Err(Status::already_exists(
                "managed run already exists; observe or resume it instead of dispatching again",
            ));
        }
        session_flow::ensure_direct_inference_run_liveness(&self.state, &session_run)
            .await
            .map_err(|error| {
                warn!(%error, run_id = %session_run.run_id, "initial gRPC direct-inference liveness heartbeat failed");
                Status::unavailable("session-core liveness heartbeat failed")
            })?;

        publish_ingress_accepted(
            &self.state,
            &request_id,
            &org_id,
            &model,
            content.len(),
            user_id,
            req.zdr,
        )
        .await;

        let memory_context = if req.zdr {
            Vec::new()
        } else {
            fetch_memory_context(
                &self.state,
                &identity,
                &session_run.thread_id,
                &org_id,
                &content,
            )
            .await
        };
        let messages = build_messages(&memory_context, &content);
        let infer_req =
            build_infer_request(&request_id, &org_id, &model, &req.provider, messages, &req)?;

        // The gateway used to consult its own response cache here, BEFORE
        // calling inference-core. It was removed rather than repaired.
        //
        // It keyed on `(org_id, user_id, model)` plus the raw last user
        // message — while the two lines above had just built the memory-loaded
        // `messages` and an `infer_req` carrying `structured_output_schema`,
        // `temperature` and `max_tokens`, none of which reached the key. So a
        // structured-output call whose last user message matched an earlier
        // plain-text call from the same (org, user, model) was served that
        // plain-text answer as a normal response with `stop_reason: "end_turn"`
        // and zero tokens; the caller parsed it as JSON and failed with nothing
        // anywhere to explain why. Memory drift had the same shape: the context
        // is in the prompt but not in the key, so a pre-change answer replayed
        // after the user's memory changed. A hit also short-circuited
        // inference-core's ZDR handling, routing and its own cache.
        //
        // inference-core already caches behind this same call and does it
        // correctly: `PromptCache::cache_key` (inference-core/src/cache.rs:67)
        // hashes org, user, provider hint, model, EVERY message role+content,
        // temperature, max_tokens, tools, tool_choice and the structured-output
        // schema, and `get` refuses ZDR requests outright — a strict superset.
        // It is live on the path at provider/fallback.rs:701 and :741.
        //
        // The SSE path keeps its cache: that one keys on the rendered prompt
        // and gates on `langcache::TurnCacheability`, which this path never had.

        let mut client = self.state.inference_client.clone();
        let infer = match client.infer(identity.inference_request(infer_req)?).await {
            Ok(response) => response.into_inner(),
            Err(status) => {
                warn!(%status, "inference-core Infer failed");
                session_flow::terminalize_direct_inference_run_with_token(
                    &self.state,
                    &session_run,
                    session_flow::DirectInferenceTerminal::Failed("inference_unavailable"),
                    identity.session_bearer()?,
                )
                .await
                .map_err(|error| {
                    Status::unavailable(format!("session-core terminalization failed: {error}"))
                })?;
                return Err(Status::internal(format!(
                    "inference failed: {}",
                    status.message()
                )));
            }
        };

        if !req.zdr {
            if let Err(error) = session_flow::append_assistant_message_with_token(
                &self.state,
                &session_run.thread_id,
                &infer.content,
                identity.session_bearer()?,
            )
            .await
            {
                session_flow::terminalize_direct_inference_run_with_token(
                    &self.state,
                    &session_run,
                    session_flow::DirectInferenceTerminal::Failed("assistant_persist_failed"),
                    identity.session_bearer()?,
                )
                .await
                .map_err(|terminal_error| {
                    Status::unavailable(format!(
                        "session-core terminalization failed: {terminal_error}"
                    ))
                })?;
                return Err(Status::internal(format!(
                    "session-core append assistant failed: {error}"
                )));
            }
        }

        session_flow::terminalize_direct_inference_run_with_token(
            &self.state,
            &session_run,
            session_flow::DirectInferenceTerminal::Completed,
            identity.session_bearer()?,
        )
        .await
        .map_err(|error| {
            Status::unavailable(format!("session-core terminalization failed: {error}"))
        })?;

        // No gateway-tier store: inference-core cached this response itself,
        // keyed on the whole request rather than on the raw user message.

        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        publish_usage_envelope(
            &self.state,
            &request_id,
            &org_id,
            &infer,
            latency_ms,
            user_id,
            req.zdr,
            req.min_privacy_tier,
        )
        .await;

        info!(run_id = %session_run.run_id, thread_id = %session_run.thread_id, request_id = %request_id, "gateway invoke completed");

        Ok(Response::new(InvokeResponse {
            request_id,
            content: infer.content,
            model_used: if infer.model_used.is_empty() {
                model
            } else {
                infer.model_used
            },
            stop_reason: if infer.stop_reason.is_empty() {
                "end_turn".to_owned()
            } else {
                infer.stop_reason
            },
            input_tokens: infer.input_tokens,
            output_tokens: infer.output_tokens,
            sources: Vec::new(),
        }))
    }

    type InvokeStreamStream = tokio_stream::wrappers::ReceiverStream<Result<InvokeChunk, Status>>;

    async fn invoke_stream(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<Self::InvokeStreamStream>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Invoke)?;
        let mut req = request.into_inner();
        req.zdr = identity.effective_zdr(req.zdr);
        let content = req.content.trim().to_owned();
        if content.is_empty() {
            return Err(Status::invalid_argument("content must not be empty"));
        }

        let request_id = request_id_or_new(&req.request_id);
        let model = model_or_default(&req.model);
        let org_id = req.org_id.clone();
        let user_id = identity
            .user_id()
            .unwrap_or_else(|| identity.principal_id());
        let start_key = managed_start_key(
            &self.state,
            &org_id,
            user_id,
            &request_id,
            &req.idempotency_key,
            "gateway-direct",
        );
        let session_run = session_flow::prepare_managed_run_with_token(
            &self.state,
            Some(&req.thread_id),
            Some(&req.session_key),
            &org_id,
            user_id,
            &content,
            "model-gateway",
            "execute",
            &start_key,
            mp_contracts::model_plane::v1::ManagedRunSource::GatewayDirect,
            req.zdr,
            identity.session_bearer()?,
        )
        .await
        .map_err(|error| {
            Status::unavailable(format!("session-core managed start failed: {error}"))
        })?;
        if session_run.already_started {
            return Err(Status::already_exists(
                "managed run already exists; observe or resume it instead of dispatching again",
            ));
        }
        session_flow::ensure_direct_inference_run_liveness(&self.state, &session_run)
            .await
            .map_err(|error| {
                warn!(%error, run_id = %session_run.run_id, "initial gRPC direct-stream liveness heartbeat failed");
                Status::unavailable("session-core liveness heartbeat failed")
            })?;

        publish_ingress_accepted(
            &self.state,
            &request_id,
            &org_id,
            &model,
            content.len(),
            user_id,
            req.zdr,
        )
        .await;

        let memory_context = if req.zdr {
            Vec::new()
        } else {
            fetch_memory_context(
                &self.state,
                &identity,
                &session_run.thread_id,
                &org_id,
                &content,
            )
            .await
        };
        let messages = build_messages(&memory_context, &content);
        let infer_req =
            build_infer_request(&request_id, &org_id, &model, &req.provider, messages, &req)?;

        // No gateway-tier cache lookup here — see the `invoke` site. The
        // streaming variant had the same defect and additionally reported a
        // cache hit as a single terminal chunk with zero token counts.

        let mut client = self.state.inference_client.clone();
        let upstream = match client
            .infer_stream(identity.inference_request(infer_req)?)
            .await
        {
            Ok(response) => response,
            Err(status) => {
                warn!(%status, "inference-core InferStream failed");
                session_flow::terminalize_direct_inference_run_with_token(
                    &self.state,
                    &session_run,
                    session_flow::DirectInferenceTerminal::Failed("inference_unavailable"),
                    identity.session_bearer()?,
                )
                .await
                .map_err(|error| {
                    Status::unavailable(format!("session-core terminalization failed: {error}"))
                })?;
                return Err(Status::internal(format!(
                    "inference stream failed: {}",
                    status.message()
                )));
            }
        };
        let mut upstream_stream = upstream.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<InvokeChunk, Status>>(32);
        let fallback_model = model.clone();
        let fallback_request_id = request_id.clone();
        let state = self.state.clone();
        let managed_run = session_run.clone();
        // Captured so the streaming task can decide whether the assembled
        // assistant turn may be persisted. Named for retention, not for the
        // cache that used to live here — the four prompt/scope clones beside
        // it existed only to key that cache and went with it.
        let zdr = req.zdr;
        let session_bearer = identity.session_bearer()?.to_owned();

        tokio::spawn(async move {
            let mut assistant_output = String::new();
            let mut terminal_chunk: Option<InvokeChunk> = None;
            let mut heartbeat = tokio::time::interval(session_flow::MANAGED_RUN_HEARTBEAT_INTERVAL);
            // The request path obtained the initial receipt before contacting
            // inference-core. Consume interval's immediate tick so it does
            // not duplicate that lease write inside the spawned relay.
            heartbeat.tick().await;
            loop {
                let next = tokio::select! {
                    next = upstream_stream.next() => next,
                    _ = heartbeat.tick() => {
                        match session_flow::heartbeat_direct_inference_run(&state, &managed_run).await {
                            Ok(true) => continue,
                            Ok(false) => {
                                let _ = tx.send(Err(Status::failed_precondition(
                                    "managed run is already terminal",
                                ))).await;
                                return;
                            }
                            Err(error) => {
                                warn!(%error, run_id = %managed_run.run_id, "gRPC direct stream liveness heartbeat failed");
                                let _ = tx.send(Err(Status::unavailable(
                                    "session-core liveness heartbeat failed",
                                ))).await;
                                return;
                            }
                        }
                    }
                };
                let Some(next) = next else {
                    break;
                };
                match next {
                    Ok(InferChunk {
                        request_id: chunk_request_id,
                        delta,
                        done,
                        model_used,
                        input_tokens,
                        output_tokens,
                        // Newer contract fields (provider_used/residency) are
                        // deliberately not surfaced on the legacy gRPC chunk
                        // shape; the unary envelope carries the receipt.
                        ..
                    }) => {
                        let rid = if chunk_request_id.is_empty() {
                            fallback_request_id.clone()
                        } else {
                            chunk_request_id
                        };
                        if !delta.is_empty() {
                            assistant_output.push_str(&delta);
                        }
                        let mu = if model_used.is_empty() && done {
                            fallback_model.clone()
                        } else {
                            model_used
                        };
                        let chunk = InvokeChunk {
                            request_id: rid,
                            delta,
                            done,
                            model_used: mu,
                            input_tokens,
                            output_tokens,
                        };
                        if chunk.done {
                            // A terminal chunk is not externally observable
                            // until assistant persistence and the immutable
                            // Session Core receipt have both succeeded.
                            terminal_chunk = Some(chunk);
                            break;
                        }
                        if tx.send(Ok(chunk)).await.is_err() {
                            return;
                        }
                    }
                    Err(status) => {
                        warn!(%status, "inference stream error");
                        let terminal = session_flow::terminalize_direct_inference_run_with_token(
                            &state,
                            &managed_run,
                            session_flow::DirectInferenceTerminal::Failed("inference_unavailable"),
                            &session_bearer,
                        )
                        .await;
                        let error = match terminal {
                            Ok(()) => Status::internal(format!(
                                "inference stream error: {}",
                                status.message()
                            )),
                            Err(error) => Status::unavailable(format!(
                                "session-core terminalization failed: {error}"
                            )),
                        };
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                }
            }

            let Some(terminal_chunk) = terminal_chunk else {
                let terminal = session_flow::terminalize_direct_inference_run_with_token(
                    &state,
                    &managed_run,
                    session_flow::DirectInferenceTerminal::Failed("inference_failed"),
                    &session_bearer,
                )
                .await;
                let error = match terminal {
                    Ok(()) => Status::internal("inference stream ended without a terminal chunk"),
                    Err(error) => {
                        Status::unavailable(format!("session-core terminalization failed: {error}"))
                    }
                };
                let _ = tx.send(Err(error)).await;
                return;
            };

            if !zdr {
                if let Err(error) = session_flow::append_assistant_message_with_token(
                    &state,
                    &managed_run.thread_id,
                    &assistant_output,
                    &session_bearer,
                )
                .await
                {
                    let terminal = session_flow::terminalize_direct_inference_run_with_token(
                        &state,
                        &managed_run,
                        session_flow::DirectInferenceTerminal::Failed("assistant_persist_failed"),
                        &session_bearer,
                    )
                    .await;
                    let result = match terminal {
                        Ok(()) => Status::internal(format!(
                            "session-core append assistant failed: {error}"
                        )),
                        Err(terminal_error) => Status::unavailable(format!(
                            "session-core terminalization failed: {terminal_error}"
                        )),
                    };
                    let _ = tx.send(Err(result)).await;
                    return;
                }
            }

            if let Err(error) = session_flow::terminalize_direct_inference_run_with_token(
                &state,
                &managed_run,
                session_flow::DirectInferenceTerminal::Completed,
                &session_bearer,
            )
            .await
            {
                let _ = tx
                    .send(Err(Status::unavailable(format!(
                        "session-core terminalization failed: {error}"
                    ))))
                    .await;
                return;
            }

            // No gateway-tier response cache on this path: inference-core's
            // `PromptCache` already caches behind the same call, and its key is
            // a strict superset of anything we could key on here. See the
            // `invoke` lookup site for why the gateway copy was removed.

            let _ = tx.send(Ok(terminal_chunk)).await;
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "ok".to_owned(),
        }))
    }

    // ------------------------------------------------------------------
    // Wave 10a — trivial tool RPCs. Implementations live in
    // crate::tools to keep this file readable. Each handler is a thin
    // delegate; no business logic here.
    // ------------------------------------------------------------------

    async fn web_search(
        &self,
        request: Request<WebSearchRequest>,
    ) -> Result<Response<WebSearchResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        let mut req = request.into_inner();
        req.zdr = identity.effective_zdr(req.zdr);
        tools::handle_web_search(&self.state, req)
            .await
            .map(Response::new)
    }

    async fn sleep(
        &self,
        request: Request<SleepRequest>,
    ) -> Result<Response<SleepResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Tool)?;
        tools::handle_sleep(request.into_inner())
            .await
            .map(Response::new)
    }

    async fn remote_trigger(
        &self,
        request: Request<RemoteTriggerRequest>,
    ) -> Result<Response<RemoteTriggerResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Tool)?;
        Err(Status::failed_precondition(
            "remote_trigger is quarantined until hostname DNS rebinding defenses are enforced by Quarry",
        ))
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        require_explicit_zdr_contract(&identity, "send_message")?;
        tools::handle_send_message(&self.state, request.into_inner()).map(Response::new)
    }

    async fn synthetic_output(
        &self,
        request: Request<SyntheticOutputRequest>,
    ) -> Result<Response<SyntheticOutputResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Tool)?;
        tools::handle_synthetic_output(request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10b — plan mode + team workers. Stores live on AppState
    // (DashMap-backed). Tool dispatch middleware checks `is_plan_mode`
    // before allowing write-class tool calls.
    // ------------------------------------------------------------------

    async fn enter_plan_mode(
        &self,
        request: Request<EnterPlanModeRequest>,
    ) -> Result<Response<EnterPlanModeResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Write)?;
        let req = request.into_inner();
        require_durable_run_owner(&self.state, &identity, &req.run_id).await?;
        let run_id = req.run_id.clone();
        let org_id = req.org_id.clone();
        let resp =
            coordinator::handle_enter_plan_mode(&self.state.plan_mode, &*self.state.publisher, req)
                .await?;
        // Durable run-mode write-through (ROADMAP P3 / matrix §4.1): persist
        // mode='plan' on the run so plan mode survives restart. Best-effort —
        // the in-memory plan_mode store is authoritative for the response.
        // org_id is forwarded so session-core can org-scope the UPDATE.
        if !run_id.is_empty() && !org_id.is_empty() {
            let mut client = self.state.session_client.clone();
            if let Err(e) = client
                .set_run_mode(identity.session_request(
                    mp_contracts::model_plane::v1::SetRunModeRequest {
                        run_id,
                        mode: "plan".to_owned(),
                        org_id,
                    },
                )?)
                .await
            {
                tracing::warn!(error = %e, "durable run-mode persist (plan) failed (best-effort)");
            }
        }
        Ok(Response::new(resp))
    }

    async fn exit_plan_mode(
        &self,
        request: Request<ExitPlanModeRequest>,
    ) -> Result<Response<ExitPlanModeResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Write)?;
        let req = request.into_inner();
        require_durable_run_owner(&self.state, &identity, &req.run_id).await?;
        let run_id = req.run_id.clone();
        let org_id = req.org_id.clone();
        let resp =
            coordinator::handle_exit_plan_mode(&self.state.plan_mode, &*self.state.publisher, req)
                .await?;
        // Exiting plan mode returns the run to 'execute' durably. Best-effort.
        // org_id is forwarded so session-core can org-scope the UPDATE.
        if !run_id.is_empty() && !org_id.is_empty() {
            let mut client = self.state.session_client.clone();
            if let Err(e) = client
                .set_run_mode(identity.session_request(
                    mp_contracts::model_plane::v1::SetRunModeRequest {
                        run_id,
                        mode: "execute".to_owned(),
                        org_id,
                    },
                )?)
                .await
            {
                tracing::warn!(error = %e, "durable run-mode persist (execute) failed (best-effort)");
            }
        }
        Ok(Response::new(resp))
    }

    async fn is_plan_mode(
        &self,
        request: Request<IsPlanModeRequest>,
    ) -> Result<Response<IsPlanModeResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        coordinator::handle_is_plan_mode(&self.state.plan_mode, request.into_inner())
            .map(Response::new)
    }

    async fn team_create(
        &self,
        request: Request<TeamCreateRequest>,
    ) -> Result<Response<TeamCreateResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        coordinator::handle_team_create(&self.state.team_workers, request.into_inner())
            .map(Response::new)
    }

    async fn team_delete(
        &self,
        request: Request<TeamDeleteRequest>,
    ) -> Result<Response<TeamDeleteResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        coordinator::handle_team_delete(&self.state.team_workers, request.into_inner())
            .map(Response::new)
    }

    async fn team_list(
        &self,
        request: Request<TeamListRequest>,
    ) -> Result<Response<TeamListResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        coordinator::handle_team_list(&self.state.team_workers, request.into_inner())
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10c — LSP bridge passthrough.
    // ------------------------------------------------------------------

    async fn lsp_query(
        &self,
        request: Request<LspQueryRequest>,
    ) -> Result<Response<LspQueryResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        require_explicit_zdr_contract(&identity, "lsp_query")?;
        lsp::handle_lsp_query(&self.state.lsp, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10e — approval gate.
    // ------------------------------------------------------------------

    async fn request_approval(
        &self,
        request: Request<RequestApprovalRequest>,
    ) -> Result<Response<RequestApprovalResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Write)?;
        let session_bearer = identity.session_bearer()?;
        let actor = identity
            .user_id()
            .ok_or_else(|| Status::permission_denied("a verified user must request approval"))?;
        // Prepare without any cache/event side effect. Session-core first
        // validates the run's tenant+user ownership and durably records the
        // gate; only that acknowledged record may enter the bounded cache or
        // emit APPROVAL_REQUESTED.
        let prepared = approvals::prepare_request_approval(
            &self.state.approvals,
            request.into_inner(),
            actor,
        )?;
        let durable = approvals::persist_approval_request_authenticated(
            &mut self.state.orchestration_client.clone(),
            prepared.approval(),
            session_bearer,
        )
        .await?;
        let resp = approvals::commit_persisted_approval(
            &self.state.approvals,
            &*self.state.publisher,
            prepared.align_with_durable(&durable)?,
            actor,
        )
        .await?;
        Ok(Response::new(resp))
    }

    async fn approve_approval(
        &self,
        request: Request<ApproveApprovalRequest>,
    ) -> Result<Response<ApproveApprovalResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Approval)?;
        let session_bearer = identity.session_bearer()?;
        let actor = identity
            .user_id()
            .ok_or_else(|| Status::permission_denied("a verified user must decide an approval"))?;
        let mut req = request.into_inner();
        req.decided_by = actor.to_owned();
        let preview = self
            .state
            .approvals
            .preview_resolution_for_owner_with_transition(
                &req.approval_id,
                &req.org_id,
                true,
                req.decided_by.clone(),
                req.comment.clone(),
            );
        let preview = match preview {
            Ok(preview) => preview,
            Err(approvals::ResolveError::NotFound) => {
                approvals::read_through_approval_for_owner_authenticated(
                    &self.state.approvals,
                    &mut self.state.orchestration_client.clone(),
                    &req.approval_id,
                    &req.org_id,
                    session_bearer,
                    actor,
                )
                .await?;
                self.state
                    .approvals
                    .preview_resolution_for_owner_with_transition(
                        &req.approval_id,
                        &req.org_id,
                        true,
                        req.decided_by.clone(),
                        req.comment.clone(),
                    )
                    .map_err(approvals::resolve_err_to_status)?
            }
            Err(error) => return Err(approvals::resolve_err_to_status(error)),
        };
        approvals::require_fresh_approval_delivery(preview.transitioned)?;
        if preview.transitioned {
            approvals::persist_approval_decision_authenticated(
                &mut self.state.orchestration_client.clone(),
                &preview.approval,
                session_bearer,
            )
            .await?;
        }
        let outcome =
            approvals::handle_approve_approval(&self.state.approvals, &*self.state.publisher, req)
                .await?;
        // A concurrent identical decision can win between preview and local
        // commit. Without a durable delivery receipt that outcome is still
        // unknown, so fail closed rather than returning a false success.
        approvals::require_fresh_approval_delivery(outcome.transitioned)?;
        let approval = outcome
            .response
            .approval
            .as_ref()
            .ok_or_else(|| Status::data_loss("resolved approval is missing"))?;
        // The grant is durable, but Gateway must not turn it into a generic
        // `ResumeRun` call. There is no immutable continuation descriptor or
        // service-only receipt protocol yet, so this deliberately surfaces an
        // unavailable continuation rather than claiming the agent restarted.
        approvals::quarantine_granted_approval_continuation(approval)?;
        Ok(Response::new(outcome.response))
    }

    async fn deny_approval(
        &self,
        request: Request<DenyApprovalRequest>,
    ) -> Result<Response<DenyApprovalResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Approval)?;
        let session_bearer = identity.session_bearer()?;
        let actor = identity
            .user_id()
            .ok_or_else(|| Status::permission_denied("a verified user must decide an approval"))?;
        let mut req = request.into_inner();
        req.decided_by = actor.to_owned();
        let preview = self.state.approvals.preview_resolution_for_owner(
            &req.approval_id,
            &req.org_id,
            false,
            req.decided_by.clone(),
            req.comment.clone(),
        );
        let preview = match preview {
            Ok(preview) => preview,
            Err(approvals::ResolveError::NotFound) => {
                approvals::read_through_approval_for_owner_authenticated(
                    &self.state.approvals,
                    &mut self.state.orchestration_client.clone(),
                    &req.approval_id,
                    &req.org_id,
                    session_bearer,
                    actor,
                )
                .await?;
                self.state
                    .approvals
                    .preview_resolution_for_owner(
                        &req.approval_id,
                        &req.org_id,
                        false,
                        req.decided_by.clone(),
                        req.comment.clone(),
                    )
                    .map_err(approvals::resolve_err_to_status)?
            }
            Err(error) => return Err(approvals::resolve_err_to_status(error)),
        };
        approvals::persist_approval_decision_authenticated(
            &mut self.state.orchestration_client.clone(),
            &preview,
            session_bearer,
        )
        .await?;
        let resp =
            approvals::handle_deny_approval(&self.state.approvals, &*self.state.publisher, req)
                .await?;
        Ok(Response::new(resp))
    }

    async fn list_pending_approvals(
        &self,
        request: Request<ListPendingApprovalsRequest>,
    ) -> Result<Response<ListPendingApprovalsResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Read)?;
        let actor = identity.user_id().ok_or_else(|| {
            Status::permission_denied("a verified user must list pending approvals")
        })?;
        approvals::handle_list_pending_approvals_authenticated(
            &self.state.approvals,
            &mut self.state.orchestration_client.clone(),
            request.into_inner(),
            identity.session_bearer()?,
            actor,
        )
        .await
        .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10f — trajectory recording + JSONL export.
    // ------------------------------------------------------------------

    async fn record_trajectory(
        &self,
        request: Request<RecordTrajectoryRequest>,
    ) -> Result<Response<RecordTrajectoryResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Write)?;
        let req = request.into_inner();
        let run_id = req
            .trajectory
            .as_ref()
            .map_or("", |trajectory| trajectory.run_id.as_str());
        require_durable_run_owner(&self.state, &identity, run_id).await?;
        trajectory::handle_record_trajectory(&self.state.trajectories, &*self.state.publisher, req)
            .await
            .map(Response::new)
    }

    async fn list_trajectories(
        &self,
        request: Request<ListTrajectoriesRequest>,
    ) -> Result<Response<ListTrajectoriesResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        trajectory::handle_list_trajectories(&self.state.trajectories, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn export_trajectories(
        &self,
        request: Request<ExportTrajectoriesRequest>,
    ) -> Result<Response<ExportTrajectoriesResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        trajectory::handle_export_trajectories(&self.state.trajectories, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10d — skills loader.
    // ------------------------------------------------------------------

    async fn list_skills(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Read)?;
        let caller_user_id = identity.user_id().unwrap_or_default().to_owned();
        skills::handle_list_skills(
            &self.state.skills,
            request.into_inner(),
            &self.state.ownership,
            &caller_user_id,
        )
        .map(Response::new)
    }

    async fn get_skill(
        &self,
        request: Request<GetSkillRequest>,
    ) -> Result<Response<GetSkillResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Read)?;
        let caller_user_id = identity.user_id().unwrap_or_default().to_owned();
        skills::handle_get_skill(
            &self.state.skills,
            request.into_inner(),
            &self.state.ownership,
            &caller_user_id,
        )
        .map(Response::new)
    }

    async fn match_skills(
        &self,
        request: Request<MatchSkillsRequest>,
    ) -> Result<Response<MatchSkillsResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Read)?;
        let caller_user_id = identity.user_id().unwrap_or_default().to_owned();
        let req = request.into_inner();
        // §G7 read path (last mile): lazily pull this org's LEARNED skills from
        // session-core into the match cache — once per org — so learned skills
        // surface in matching, not just disk-loaded ones. Best-effort: a fetch
        // failure leaves the disk-loaded skills intact and is retried next call
        // (the org is only marked loaded on success).
        if !req.org_id.is_empty() && !self.state.skills.is_org_loaded(&req.org_id) {
            let mut client = self.state.session_client.clone();
            match client
                .list_agent_skills(identity.session_request(
                    mp_contracts::model_plane::v1::ListAgentSkillsRequest {
                        org_id: req.org_id.clone(),
                        enabled_only: true,
                    },
                )?)
                .await
            {
                Ok(resp) => {
                    let pulled = resp.into_inner().skills;
                    // Ownership (SKILL-1) rides alongside the match cache: computed
                    // from the same pull, before `pulled` is consumed below.
                    self.state.ownership.replace_org_kind(
                        &req.org_id,
                        crate::ownership::KIND_SKILL,
                        skills::skill_ownership_entries(&pulled),
                    );
                    // Reconcile, don't merely append: a disabled/deleted
                    // learned skill must stop steering MatchSkills results on
                    // the gRPC path just as it does on the SSE path.
                    self.state.skills.replace_learned(
                        &req.org_id,
                        pulled
                            .into_iter()
                            .map(skills::agent_skill_to_skill)
                            .collect(),
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, org_id = %req.org_id, "lazy-load learned skills failed (best-effort)");
                }
            }
        }
        skills::handle_match_skills(
            &self.state.skills,
            req,
            &self.state.ownership,
            &caller_user_id,
        )
        .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10g — MCP server hosting.
    // ------------------------------------------------------------------

    async fn register_mcp_server(
        &self,
        request: Request<RegisterMcpServerRequest>,
    ) -> Result<Response<RegisterMcpServerResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        Err(Status::failed_precondition(
            "MCP registration requires the authoritative capability credential; use the authenticated HTTP registration contract",
        ))
    }

    async fn list_mcp_servers(
        &self,
        request: Request<ListMcpServersRequest>,
    ) -> Result<Response<ListMcpServersResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        runtime_registries::handle_list_mcp_servers(&self.state.mcp, request.into_inner())
            .map(Response::new)
    }

    async fn proxy_mcp_tool(
        &self,
        request: Request<ProxyMcpToolRequest>,
    ) -> Result<Response<ProxyMcpToolResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        require_explicit_zdr_contract(&identity, "MCP execution")?;
        if !identity.is_service() {
            return Err(Status::permission_denied(
                "MCP execution is restricted to the governed execution service",
            ));
        }
        let req = request.into_inner();
        // No live per-user bearer exists on this service-to-service path, so
        // an OAuth-connected server's token can't be forwarded the way every
        // other gateway->capability-core call does it — resolve one via the
        // Model-Plane-local service secret instead. Soft-fails to None (never
        // an error) so servers with no stored OAuth tokens are unaffected.
        let oauth_token = crate::mcp_oauth::resolve_stored_oauth_token(
            &self.state.http_client,
            &self.state.capability_core_base_url,
            &self.state.mcp_oauth_service_token,
            &req.org_id,
            &req.server_id,
        )
        .await;
        runtime_registries::handle_proxy_mcp_tool(
            &self.state.mcp,
            &self.state.ownership,
            req,
            oauth_token.as_deref(),
        )
        .await
        .map(Response::new)
    }

    /// Agent-facing tool defs for the org's enabled MCP servers, namespaced
    /// `mcp__<server_id>__<tool>` — the exposure bridge for the governed agent
    /// loop (execution-core). Inline chat calls the same underlying
    /// `runtime_registries::mcp_tool_defs` directly rather than this RPC.
    async fn list_mcp_tools(
        &self,
        request: Request<ListMcpToolsRequest>,
    ) -> Result<Response<ListMcpToolsResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Read)?;
        if !identity.is_service() {
            identity.authorize_user(&request.get_ref().user_id)?;
        }
        let req = request.into_inner();
        let tools = runtime_registries::mcp_tool_defs(
            &self.state.mcp,
            &self.state.ownership,
            &req.org_id,
            &req.user_id,
            &self.state.http_client,
            &self.state.capability_core_base_url,
            &self.state.mcp_oauth_service_token,
            None,
        )
        .await;
        Ok(Response::new(ListMcpToolsResponse {
            request_id: req.request_id,
            tools,
        }))
    }

    // ------------------------------------------------------------------
    // Wave 10h — plugins.
    // ------------------------------------------------------------------

    async fn register_plugin(
        &self,
        request: Request<RegisterPluginRequest>,
    ) -> Result<Response<RegisterPluginResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        // capability-core's `plugin_packages` is the system of record (its own
        // handler says so). The gateway previously kept a per-replica in-memory
        // copy instead, so a registered plugin never reached the durable
        // catalog and vanished on the next restart.
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let plugin = req
            .plugin
            .ok_or_else(|| Status::invalid_argument("plugin is required"))?;
        if plugin.name.trim().is_empty() || plugin.version.trim().is_empty() {
            return Err(Status::invalid_argument(
                "plugin name and version are required",
            ));
        }
        let created: serde_json::Value = self
            .capability_json(
                reqwest::Method::POST,
                "/api/v1/plugins",
                &bearer,
                Some(serde_json::json!({
                    "name": plugin.name,
                    "version": plugin.version,
                    "description": plugin.kind,
                    // capability-core stores a manifest DOCUMENT; the gateway
                    // contract carries a manifest URL. Keep the URL rather than
                    // fetch-and-inline it here — fetching a caller-supplied URL
                    // from the gateway would be an SSRF surface, and the host
                    // that loads the manifest is the right place to resolve it.
                    "manifest_json": {"manifest_url": plugin.manifest_url},
                })),
            )
            .await?;
        // capability-core deliberately creates plugins DISABLED (safe rollout)
        // and assigns its own id, so report what it stored rather than echoing
        // what the caller asked for.
        Ok(Response::new(RegisterPluginResponse {
            request_id: req.request_id,
            plugin: Some(Plugin {
                plugin_id: json_str(&created, "id"),
                name: json_str(&created, "name"),
                version: json_str(&created, "version"),
                kind: plugin.kind,
                manifest_url: plugin.manifest_url,
                enabled: created
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                status: json_str(&created, "rollout_state"),
                installed_at_unix: created
                    .get("installed_at_unix")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_else(|| Utc::now().timestamp()),
            }),
        }))
    }

    async fn list_plugins(
        &self,
        request: Request<ListPluginsRequest>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let listed: serde_json::Value = self
            .capability_json(reqwest::Method::GET, "/api/v1/plugins", &bearer, None)
            .await?;
        let plugins = listed
            .get("plugins")
            .and_then(serde_json::Value::as_array)
            .map(|rows| rows.iter().map(plugin_from_catalog).collect())
            .unwrap_or_default();
        Ok(Response::new(ListPluginsResponse {
            request_id: req.request_id,
            plugins,
        }))
    }

    async fn set_plugin_enabled(
        &self,
        request: Request<SetPluginEnabledRequest>,
    ) -> Result<Response<SetPluginEnabledResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        if req.plugin_id.trim().is_empty() {
            return Err(Status::invalid_argument("plugin_id is required"));
        }
        let updated: serde_json::Value = self
            .capability_json(
                reqwest::Method::PATCH,
                &format!("/api/v1/plugins/{}", req.plugin_id),
                &bearer,
                Some(serde_json::json!({ "enabled": req.enabled })),
            )
            .await?;
        Ok(Response::new(SetPluginEnabledResponse {
            request_id: req.request_id,
            plugin: Some(plugin_from_catalog(&updated)),
        }))
    }

    // ------------------------------------------------------------------
    // Wave 10i — commands / hooks / permissions / policy.
    // ------------------------------------------------------------------

    async fn list_commands(
        &self,
        request: Request<ListCommandsRequest>,
    ) -> Result<Response<ListCommandsResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        // capability-core owns the slash-command catalogue. The gateway kept a
        // per-replica in-memory copy, so a command registered through the
        // product was invisible here and vice versa.
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let listed = self
            .capability_json(reqwest::Method::GET, "/api/v1/commands", &bearer, None)
            .await?;
        // capability-core returns either a bare array or an envelope depending
        // on the route; accept both rather than assuming one.
        let rows = listed
            .as_array()
            .or_else(|| listed.get("commands").and_then(serde_json::Value::as_array));
        let commands = rows
            .map(|rows| {
                rows.iter()
                    // A disabled command must not be offered: the gateway's
                    // wire `Command` has no enabled flag, so filtering here is
                    // the only way not to advertise something switched off.
                    .filter(|row| {
                        row.get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true)
                    })
                    .map(command_from_catalog)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Response::new(ListCommandsResponse {
            request_id: req.request_id,
            commands,
        }))
    }

    async fn execute_command(
        &self,
        request: Request<ExecuteCommandRequest>,
    ) -> Result<Response<ExecuteCommandResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        require_explicit_zdr_contract(&identity, "command execution")?;
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        // capability-core's exec contract takes flat string args; the gateway's
        // carries a JSON object. Only string-valued members can cross, so a
        // nested or non-string arg is REJECTED rather than stringified — a
        // silently coerced argument would execute a command the caller did not
        // describe.
        let args = match command_args(&req.args_json) {
            Ok(args) => args,
            Err(message) => return Err(Status::invalid_argument(message)),
        };
        let result = self
            .capability_json(
                reqwest::Method::POST,
                "/api/v1/commands/exec",
                &bearer,
                Some(serde_json::json!({
                    "commandName": req.command_name,
                    "args": args,
                    // capability-core re-derives the tenant from the verified
                    // caller; these are context for its audit trail.
                    "orgId": req.org_id,
                    "userId": identity.user_id(),
                })),
            )
            .await?;
        let success = result
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Ok(Response::new(ExecuteCommandResponse {
            request_id: req.request_id,
            output_json: json_str(&result, "output"),
            error_message: if success {
                String::new()
            } else {
                let error = json_str(&result, "error");
                if error.is_empty() {
                    "command execution failed".to_owned()
                } else {
                    error
                }
            },
        }))
    }

    async fn register_hook(
        &self,
        request: Request<RegisterHookRequest>,
    ) -> Result<Response<RegisterHookResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        runtime_registries::handle_register_hook(&self.state.hooks, request.into_inner())
            .map(Response::new)
    }

    async fn list_hooks(
        &self,
        request: Request<ListHooksRequest>,
    ) -> Result<Response<ListHooksResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        runtime_registries::handle_list_hooks(&self.state.hooks, request.into_inner())
            .map(Response::new)
    }

    async fn check_permission(
        &self,
        request: Request<CheckPermissionRequest>,
    ) -> Result<Response<CheckPermissionResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        // capability-core owns tool permission; the gateway proxies to it.
        // It used to answer from its own in-memory map that nothing enforced,
        // defaulting to "allow" — so a caller was told a tool was permitted
        // when nothing had evaluated it.
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let mut upstream = Request::new(EvaluatePolicyRequest {
            capability_id: req.tool_name.clone(),
            run_id: String::new(),
            agent_id: "model-gateway".to_owned(),
            org_id: req.org_id.clone(),
            // Mirrors execution-core's own call (capability_policy.rs) — the
            // two must ask the same question or they can disagree.
            scope: "global".to_owned(),
        });
        if !bearer.is_empty() {
            upstream.metadata_mut().insert(
                "authorization",
                bearer
                    .parse()
                    .map_err(|_| Status::unauthenticated("malformed authorization"))?,
            );
        }
        let decision = self
            .state
            .capability_client
            .clone()
            .evaluate_policy(upstream)
            .await?
            .into_inner();
        // Only an explicit "allow" is permission. "ask" and "fallback" are not
        // denials, but they are not authorization either — reporting them as
        // allowed would be the same false assurance the local store gave.
        Ok(Response::new(CheckPermissionResponse {
            request_id: req.request_id,
            allowed: decision.decision == "allow",
            reason: if decision.reason.is_empty() {
                decision.decision
            } else {
                decision.reason
            },
        }))
    }

    async fn set_permission(
        &self,
        request: Request<SetPermissionRequest>,
    ) -> Result<Response<SetPermissionResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        // Written through to capability-core's scope store, which is the
        // system of record. Previously this landed in a per-replica in-memory
        // map that no dispatch path read and no restart survived.
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let action = match req.verdict.as_str() {
            "allow" => "grant",
            "deny" => "revoke",
            other => {
                return Err(Status::invalid_argument(format!(
                    "verdict must be 'allow' or 'deny', got '{other}'"
                )))
            }
        };
        if self.state.capability_core_base_url.is_empty() {
            return Err(Status::unavailable(
                "capability registry is unavailable; tool permission cannot be recorded",
            ));
        }
        let url = format!(
            "{}/api/v1/capabilities/scopes/{action}",
            self.state.capability_core_base_url
        );
        // `scope_kind: org` with the org from the request; capability-core
        // re-derives the tenant from the verified caller on its own side, so
        // this cannot be used to write into another tenant.
        let response = self
            .state
            .http_client
            .post(&url)
            .bearer_auth(bearer.trim_start_matches("Bearer ").trim())
            .json(&serde_json::json!({
                "capability_id": req.tool_name,
                "scope_kind": "org",
                "scope_value": req.org_id,
            }))
            .send()
            .await
            .map_err(|error| {
                Status::unavailable(format!("capability registry unreachable: {error}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(Status::internal(format!(
                "capability registry rejected the permission write: HTTP {status}"
            )));
        }
        Ok(Response::new(SetPermissionResponse {
            request_id: req.request_id,
        }))
    }

    async fn get_policy(
        &self,
        request: Request<GetPolicyRequest>,
    ) -> Result<Response<GetPolicyResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        let req = request.into_inner();
        // `OrgPolicy` was one gateway-owned blob that nothing enforced. It is
        // now decomposed per owner and read back from those owners, so what an
        // operator sees here is what is actually applied. `allowed_models`
        // stays zero-valued because nothing stores it yet.
        let limits = crate::org_quota::fetch_org_limits(
            &self.state.http_client,
            &self.state.org_core_base_url,
            &req.org_id,
            &self.state.org_core_service_id,
            &self.state.org_core_service_token,
        )
        .await;
        Ok(Response::new(GetPolicyResponse {
            request_id: req.request_id,
            policy: Some(OrgPolicy {
                org_id: req.org_id.clone(),
                rate_limit_rpm: i32::try_from(
                    self.state.rate_limiter.org_rpm(&req.org_id).round() as i64
                )
                .unwrap_or(i32::MAX),
                max_cost_per_run_usd: limits.max_cost_usd.unwrap_or(0.0),
                max_tokens_per_run: limits
                    .max_tokens
                    .map_or(0, |tokens| i32::try_from(tokens).unwrap_or(i32::MAX)),
                ..OrgPolicy::default()
            }),
        }))
    }

    async fn set_policy(
        &self,
        request: Request<SetPolicyRequest>,
    ) -> Result<Response<SetPolicyResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let policy = req
            .policy
            .ok_or_else(|| Status::invalid_argument("policy is required"))?;

        // Refuse BEFORE applying anything, so a partially-applied policy cannot
        // be reported as accepted. These fields have no owner that can store
        // them: cost-core's /budget/check takes the caps as request parameters
        // (it accounts spend, it does not configure limits), and org-core has
        // an `org_quotas` table but a dead `Quota` type and no route. Silently
        // banking them here is exactly the behaviour being removed — an
        // operator would set a spend cap that nothing ever applies.
        let unowned = unowned_policy_fields(&policy);
        if !unowned.is_empty() {
            return Err(Status::unimplemented(format!(
                "no service stores these yet, so they cannot be enforced: {}. Everything else in OrgPolicy now routes to its owner: spend/token caps to Control Plane quotas (org-core org_quotas), denied_tools to capability-core, rate_limit_rpm to this gateway's own limiter.",
                unowned.join(", ")
            )));
        }

        // denied_tools -> capability-core, which owns tool permission (the same
        // service CheckPermission now consults).
        for tool in policy
            .denied_tools
            .split(',')
            .map(str::trim)
            .filter(|tool| !tool.is_empty())
        {
            self.capability_json(
                reqwest::Method::POST,
                "/api/v1/capabilities/scopes/revoke",
                &bearer,
                Some(serde_json::json!({
                    "capability_id": tool,
                    "scope_kind": "org",
                    "scope_value": policy.org_id,
                })),
            )
            .await?;
        }

        // Spend/token ceilings -> Control Plane, which owns quotas. Stored in
        // micro-dollars because `org_quotas.quota_limit` is BIGINT; see
        // `org_quota` for why the key names its unit.
        if policy.max_cost_per_run_usd > 0.0 {
            self.put_org_quota(
                &policy.org_id,
                crate::org_quota::MAX_COST_PER_RUN_USD_MICROS,
                crate::org_quota::usd_to_micros(policy.max_cost_per_run_usd),
            )
            .await?;
        }
        if policy.max_tokens_per_run > 0 {
            self.put_org_quota(
                &policy.org_id,
                crate::org_quota::MAX_TOKENS_PER_RUN,
                i64::from(policy.max_tokens_per_run),
            )
            .await?;
        }

        // rate_limit_rpm stays local: it protects THIS process, so no plane can
        // enforce it for us. 0 clears the override back to the process default.
        self.state.rate_limiter.set_org_rpm(
            &policy.org_id,
            (policy.rate_limit_rpm > 0).then(|| f64::from(policy.rate_limit_rpm)),
        );

        Ok(Response::new(SetPolicyResponse {
            request_id: req.request_id,
            policy: Some(OrgPolicy {
                org_id: policy.org_id.clone(),
                rate_limit_rpm: policy.rate_limit_rpm,
                denied_tools: policy.denied_tools,
                max_cost_per_run_usd: policy.max_cost_per_run_usd,
                max_tokens_per_run: policy.max_tokens_per_run,
                ..OrgPolicy::default()
            }),
        }))
    }

    // ------------------------------------------------------------------
    // Wave 10j — messages, analytics, voice, tasks.
    // ------------------------------------------------------------------

    async fn append_thread_message(
        &self,
        request: Request<AppendThreadMessageRequest>,
    ) -> Result<Response<AppendThreadMessageResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        runtime_registries::handle_append_thread_message(&self.state.messages, request.into_inner())
            .map(Response::new)
    }

    async fn list_thread_messages(
        &self,
        request: Request<ListThreadMessagesRequest>,
    ) -> Result<Response<ListThreadMessagesResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        runtime_registries::handle_list_thread_messages(&self.state.messages, request.into_inner())
            .map(Response::new)
    }

    async fn get_analytics(
        &self,
        request: Request<GetAnalyticsRequest>,
    ) -> Result<Response<GetAnalyticsResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        runtime_registries::handle_get_analytics(&self.state.analytics, request.into_inner())
            .map(Response::new)
    }

    async fn text_to_speech(
        &self,
        request: Request<TextToSpeechRequest>,
    ) -> Result<Response<TextToSpeechResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Invoke)?;
        require_explicit_zdr_contract(&identity, "speech synthesis")?;
        let req = request.into_inner();
        let request_id = request_id_or_new(&req.request_id);
        let resp = self
            .state
            .inference_client
            .clone()
            .synthesize_speech(identity.inference_request(SynthesizeSpeechRequest {
                request_id: request_id.clone(),
                org_id: req.org_id,
                text: req.text,
                voice: req.voice,
                format: req.format,
                model: String::new(),
                provider_hint: String::new(),
                language: String::new(),
            })?)
            .await?
            .into_inner();

        Ok(Response::new(TextToSpeechResponse {
            request_id: resp.request_id,
            audio: resp.audio,
            format: resp.format,
            error_message: String::new(),
        }))
    }

    async fn speech_to_text(
        &self,
        request: Request<SpeechToTextRequest>,
    ) -> Result<Response<SpeechToTextResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Invoke)?;
        require_explicit_zdr_contract(&identity, "speech transcription")?;
        let req = request.into_inner();
        let request_id = request_id_or_new(&req.request_id);
        let resp = self
            .state
            .inference_client
            .clone()
            .transcribe_speech(identity.inference_request(TranscribeSpeechRequest {
                request_id: request_id.clone(),
                org_id: req.org_id,
                audio: req.audio,
                format: req.format,
                model: String::new(),
                provider_hint: String::new(),
                language: req.language,
            })?)
            .await?
            .into_inner();

        Ok(Response::new(SpeechToTextResponse {
            request_id: resp.request_id,
            text: resp.text,
            detected_language: resp.detected_language,
            error_message: String::new(),
        }))
    }

    async fn create_task(
        &self,
        request: Request<CreateTaskRequest>,
    ) -> Result<Response<CreateTaskResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Write)?;
        // capability-core owns tasks, and this gateway's own HTTP routes
        // already proxy there (`create_task_proxy`, http_routes.rs). Only the
        // gRPC surface kept a separate in-memory store, so a task created over
        // gRPC was invisible to the task UI, to capability-core and to
        // Temporal — and gone on the next restart.
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let created = self
            .capability_json(
                reqwest::Method::POST,
                "/api/v1/tasks",
                &bearer,
                Some(serde_json::json!({
                    "org_id": req.org_id,
                    "description": req.description,
                    "parent_run_id": req.parent_run_id,
                    "cron": req.cron,
                })),
            )
            .await?;
        Ok(Response::new(CreateTaskResponse {
            request_id: req.request_id,
            task: Some(task_from_catalog(&created)),
        }))
    }

    async fn list_tasks(
        &self,
        request: Request<ListTasksRequest>,
    ) -> Result<Response<ListTasksResponse>, Status> {
        authorize_rpc(&request, RpcAccess::Read)?;
        let bearer = forwarded_authorization(&request);
        let req = request.into_inner();
        let listed = self
            .capability_json(reqwest::Method::GET, "/api/v1/tasks", &bearer, None)
            .await?;
        let rows = listed
            .as_array()
            .or_else(|| listed.get("tasks").and_then(serde_json::Value::as_array));
        let tasks = rows
            .map(|rows| {
                rows.iter()
                    .filter(|row| {
                        // The wire contract exposes a status filter; apply it
                        // here so a gRPC caller gets the same subset an HTTP
                        // caller would.
                        req.status_filter.is_empty() || json_str(row, "status") == req.status_filter
                    })
                    .map(task_from_catalog)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Response::new(ListTasksResponse {
            request_id: req.request_id,
            tasks,
        }))
    }

    // ------------------------------------------------------------------
    // Wave 9 — Fetch + ExtractStructured.
    //
    // These two RPCs replace the dead Python web_fetch + extract_structured
    // tools (apps/Model Plane v2). Fetch is a pure pass-through to Quarry;
    // ExtractStructured fans out to Quarry for the fetch step and reuses
    // inference-core's existing `structured_output_schema` path for the
    // LLM coercion step. No new RPC on inference-core, no provider
    // changes — the v1 surface already had the field.
    // ------------------------------------------------------------------

    async fn fetch(
        &self,
        request: Request<FetchRequest>,
    ) -> Result<Response<FetchResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Tool)?;
        let mut req = request.into_inner();
        req.zdr = identity.effective_zdr(req.zdr);
        if !self.state.quarry.available() {
            return Err(Status::unimplemented("quarry edge not configured"));
        }
        if req.url.trim().is_empty() {
            return Err(Status::invalid_argument("url is required"));
        }

        let render = render_hints_from_proto(req.render.as_ref());
        let result = self
            .state
            .quarry
            .scrape(
                &req.url,
                &req.org_id,
                render.as_ref(),
                req.prefer_http3,
                req.zdr,
            )
            .await
            .map_err(quarry_err_to_status)?;

        Ok(Response::new(FetchResponse {
            request_id: req.request_id,
            url: result.url,
            final_url: result.final_url,
            status: i32::from(result.status),
            content_type: result.content_type,
            title: result.title,
            markdown: result.markdown,
            text: result.text,
            fingerprint: result.fingerprint,
            language: result.language,
        }))
    }

    async fn extract_structured(
        &self,
        request: Request<ExtractStructuredRequest>,
    ) -> Result<Response<ExtractStructuredResponse>, Status> {
        let identity = authorize_rpc(&request, RpcAccess::Invoke)?;
        let mut req = request.into_inner();
        req.zdr = identity.effective_zdr(req.zdr);
        if !self.state.quarry.available() {
            return Err(Status::unimplemented("quarry edge not configured"));
        }
        if req.url.trim().is_empty() {
            return Err(Status::invalid_argument("url is required"));
        }
        if req.schema_json.trim().is_empty() {
            return Err(Status::invalid_argument("schema_json is required"));
        }

        // 1. Fetch via Quarry.
        let render = render_hints_from_proto(req.render.as_ref());
        let scrape = self
            .state
            .quarry
            .scrape(&req.url, &req.org_id, render.as_ref(), false, req.zdr)
            .await
            .map_err(quarry_err_to_status)?;

        // Prefer markdown; fall back to plain text.
        let mut content = if scrape.markdown.is_empty() {
            scrape.text.clone()
        } else {
            scrape.markdown.clone()
        };
        if content.is_empty() {
            // Fetch succeeded but the page had no extractable text.
            // Return OK with an error_message so the caller sees a
            // structured failure, not a gRPC error.
            return Ok(Response::new(ExtractStructuredResponse {
                request_id: req.request_id,
                url: req.url.clone(),
                final_url: scrape.final_url,
                title: scrape.title,
                source_fingerprint: scrape.fingerprint,
                error_message: "no extractable content from page".to_owned(),
                ..Default::default()
            }));
        }
        if content.len() > MAX_EXTRACT_CONTENT_CHARS {
            content.truncate(safe_char_boundary(&content, MAX_EXTRACT_CONTENT_CHARS));
        }

        // 2. Build the inference call. Single user message embeds the
        // page content + caller instructions; structured_output_schema
        // makes the provider enforce the shape end-to-end.
        let mut system_prompt = String::from(
            "You extract structured data from web pages. \
             Read the supplied page content carefully. \
             For any field where the page does not contain a clear answer, \
             omit the field — do not invent values. \
             Return only a JSON object matching the schema.",
        );
        if !req.instructions.is_empty() {
            system_prompt.push_str("\n\nAdditional caller instructions:\n");
            system_prompt.push_str(&req.instructions);
        }

        let title_header = if scrape.title.is_empty() {
            String::new()
        } else {
            format!("# {}\n\n", scrape.title)
        };
        let user_prompt = format!(
            "URL: {}\n\n--- PAGE CONTENT ---\n{}{}\n--- END PAGE CONTENT ---\n\n\
             Extract the requested fields per the JSON schema.",
            req.url, title_header, content,
        );

        let infer_req = InferRequest {
            request_id: req.request_id.clone(),
            org_id: req.org_id.clone(),
            model: req.model.clone(),
            provider_hint: req.provider.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_owned(),
                    content: system_prompt,
                    name: String::new(),
                },
                ChatMessage {
                    role: "user".to_owned(),
                    content: user_prompt,
                    name: String::new(),
                },
            ],
            temperature: 0.0,
            max_tokens: 2048,
            structured_output_schema: req.schema_json.clone(),
            zdr: req.zdr,
            ..Default::default()
        };

        let mut client = self.state.inference_client.clone();
        match client.infer(identity.inference_request(infer_req)?).await {
            Ok(resp) => {
                let infer = resp.into_inner();
                Ok(Response::new(ExtractStructuredResponse {
                    request_id: req.request_id,
                    url: req.url.clone(),
                    final_url: scrape.final_url,
                    title: scrape.title,
                    source_fingerprint: scrape.fingerprint,
                    extracted_json: infer.content,
                    model_used: infer.model_used,
                    input_tokens: infer.input_tokens,
                    output_tokens: infer.output_tokens,
                    error_message: String::new(),
                }))
            }
            Err(err) => {
                // LLM failure. OK + structured error so the caller
                // still sees the fingerprint and can decide whether to
                // retry against the same page later.
                Ok(Response::new(ExtractStructuredResponse {
                    request_id: req.request_id,
                    url: req.url.clone(),
                    final_url: scrape.final_url,
                    title: scrape.title,
                    source_fingerprint: scrape.fingerprint,
                    error_message: format!("inference failed: {err}"),
                    ..Default::default()
                }))
            }
        }
    }
}

fn render_hints_from_proto(p: Option<&ProtoRenderHints>) -> Option<RenderHints> {
    let p = p?;
    if p.wait_for_selector.is_empty() {
        return None;
    }
    Some(RenderHints {
        wait_for_selector: Some(p.wait_for_selector.clone()),
        wait_for_timeout_ms: if p.wait_for_timeout_ms > 0 {
            #[allow(clippy::cast_sign_loss)]
            Some(p.wait_for_timeout_ms as u32)
        } else {
            None
        },
    })
}

/// Map Quarry typed errors onto gRPC status codes so callers can branch
/// on the canonical kind (`PermissionDenied` vs Unavailable vs
/// `InvalidArgument`) instead of parsing strings.
fn quarry_err_to_status(err: QuarryError) -> Status {
    match err {
        QuarryError::Unavailable => Status::unimplemented("quarry edge not configured"),
        QuarryError::Transport(e) => Status::unavailable(format!("quarry transport: {e}")),
        QuarryError::EmptyEnvelope => Status::internal("quarry: empty envelope"),
        QuarryError::Decode(e) => Status::internal(format!("quarry decode: {e}")),
        // The detailed cause (e.g. missing MODEL_GATEWAY_SERVICE_API_KEY /
        // AUTH_CORE_URL misconfiguration, or Auth Core refusing the
        // service-principal credential) is deliberately not echoed to the
        // caller/model, but must not be silently swallowed either — log it
        // so this is diagnosable from server logs instead of only ever
        // surfacing as an opaque "quarry authentication is unavailable".
        QuarryError::Authentication(detail) => {
            warn!(
                error = %detail,
                "quarry authentication failed; check MODEL_GATEWAY_SERVICE_API_KEY and the Auth Core service-principal credential"
            );
            Status::unavailable("quarry authentication is unavailable")
        }
        QuarryError::Typed { code, message, .. } => match code.as_str() {
            "BAD_REQUEST" | "INVALID_ARGUMENT" => Status::invalid_argument(message),
            "SECURITY_BLOCKED" | "FORBIDDEN" => {
                Status::permission_denied(format!("{code}: {message}"))
            }
            "RATE_LIMITED" => Status::resource_exhausted(message),
            "TIMEOUT" => Status::deadline_exceeded(message),
            "UPSTREAM_BLOCKED" => Status::unavailable(message),
            _ => Status::internal(format!("{code}: {message}")),
        },
    }
}

/// Truncate a UTF-8 string to at most `max` bytes without splitting a
/// codepoint. Falls back to 0 if no valid boundary exists (impossible
/// for non-empty strings).
fn safe_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Start the additive, authenticated gRPC compatibility listener on `:9090`.
/// Auth Core verification material is fetched before bind, so a missing or
/// malformed JWKS cannot leave a partially exposed listener.
///
/// # Errors
/// Returns an error when authentication bootstrap or the server fails.
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    serve_with_readiness(state, crate::readiness::GrpcReadiness::new()).await
}

/// Start the authenticated gRPC listener and update the shared HTTP readiness
/// gate only after the socket bind succeeds.
///
/// # Errors
///
/// Returns an error when authentication bootstrap fails, the listener cannot
/// bind, or the authenticated gRPC server fails while serving.
pub async fn serve_with_readiness(
    state: AppState,
    readiness: crate::readiness::GrpcReadiness,
) -> anyhow::Result<()> {
    let verifier = grpc_auth::JwtVerifier::from_env().await?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await?;
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<ModelGatewayServer<GatewayService>>()
        .await;
    readiness.mark_bound();
    info!("authenticated gRPC listening on :9090");

    let result = tonic::transport::Server::builder()
        .add_service(health_service)
        .add_service(ModelGatewayServer::with_interceptor(
            GatewayService { state },
            verifier,
        ))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await;
    readiness.mark_unbound();
    result?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unowned_policy_fields;
    use super::{command_args, command_from_catalog, json_str, task_from_catalog};

    #[test]
    fn policy_fields_with_no_owning_service_are_named_not_silently_banked() {
        // Spend/token ceilings now have an owner — Control Plane `org_quotas`,
        // written through by SetPolicy — so they are stored rather than
        // refused. Only the model allowlist is still homeless.
        let unowned = unowned_policy_fields(&mp_contracts::model_plane::v1::OrgPolicy {
            org_id: "org".into(),
            max_cost_per_run_usd: 5.0,
            max_tokens_per_run: 1000,
            allowed_models: "gpt-4".into(),
            ..Default::default()
        });
        assert_eq!(unowned, vec!["allowed_models"]);
    }

    #[test]
    fn the_two_fields_that_do_have_owners_are_accepted() {
        // rate_limit_rpm is the gateway's own (it protects THIS process),
        // denied_tools routes to capability-core, and the ceilings go to
        // Control Plane quotas. None of these is "unowned".
        let unowned = unowned_policy_fields(&mp_contracts::model_plane::v1::OrgPolicy {
            org_id: "org".into(),
            rate_limit_rpm: 60,
            denied_tools: "book_shipment".into(),
            max_cost_per_run_usd: 5.0,
            max_tokens_per_run: 1000,
            ..Default::default()
        });
        assert!(unowned.is_empty(), "got: {unowned:?}");
    }

    #[test]
    fn command_args_pass_scalars_and_refuse_structures() {
        let args =
            command_args(r#"{"path":"/tmp","depth":3,"force":true}"#).expect("scalars convert");
        assert_eq!(args.get("path").map(String::as_str), Some("/tmp"));
        // Numbers and booleans have one unambiguous textual form.
        assert_eq!(args.get("depth").map(String::as_str), Some("3"));
        assert_eq!(args.get("force").map(String::as_str), Some("true"));

        // A nested value has no single correct string form. Stringifying it
        // would hand capability-core an argument the caller never wrote, so it
        // is refused and the offending field is named.
        let error = command_args(r#"{"filter":{"kind":"a"}}"#).unwrap_err();
        assert!(error.contains("filter"), "got: {error}");
        assert!(command_args(r#"{"tags":["a"]}"#).is_err());

        // Empty is a legitimate no-args call, not an error.
        assert!(command_args("").expect("empty is fine").is_empty());
        assert!(command_args("not json").is_err());
        assert!(command_args("[1,2]").unwrap_err().contains("object"));
    }

    #[test]
    fn a_catalog_command_maps_handler_onto_tool_name() {
        let row = serde_json::json!({
            "id": "cmd_1", "name": "/plan", "description": "plan it",
            "handler": "planner_tool", "enabled": true
        });
        let command = command_from_catalog(&row);
        assert_eq!(command.command_id, "cmd_1");
        assert_eq!(command.name, "/plan");
        // capability-core calls the executor `handler`; this contract calls it
        // `tool_name`.
        assert_eq!(command.tool_name, "planner_tool");
        // No remote-URL or default-payload concept exists upstream; empty is
        // honest rather than invented.
        assert!(command.remote_url.is_empty());
        assert!(command.default_payload_json.is_empty());
    }

    #[test]
    fn a_missing_task_timestamp_is_zero_not_now() {
        let row = serde_json::json!({
            "id": "task_1", "org_id": "org", "description": "d", "status": "created"
        });
        let task = task_from_catalog(&row);
        assert_eq!(task.task_id, "task_1");
        assert_eq!(task.status, "created");
        // Defaulting to the current time would make an undated task look as if
        // it had just been created.
        assert_eq!(task.created_at_unix, 0);

        let dated = task_from_catalog(&serde_json::json!({
            "id": "t", "created_at": "2026-08-10T12:00:00Z"
        }));
        assert_eq!(dated.created_at_unix, 1_786_363_200);
    }

    #[test]
    fn json_str_never_panics_on_a_wrong_or_missing_field() {
        let row = serde_json::json!({"a": 1, "b": null});
        assert_eq!(json_str(&row, "a"), "");
        assert_eq!(json_str(&row, "b"), "");
        assert_eq!(json_str(&row, "missing"), "");
    }

    use super::*;
    use crate::state::DynPublisher;
    use mp_contracts::model_plane::v1::{
        inference_core_client::InferenceCoreClient,
        inference_core_server::{InferenceCore, InferenceCoreServer},
        managed_run_lifecycle_client::ManagedRunLifecycleClient,
        managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
        run_service_client::RunServiceClient,
        run_service_server::{RunService, RunServiceServer},
        session_core_client::SessionCoreClient,
        session_core_server::{SessionCore, SessionCoreServer},
        AnalyzeDocumentRequest, AnalyzeDocumentResponse, AnalyzeImageRequest, AnalyzeImageResponse,
        AnalyzeLanguageRequest, AnalyzeLanguageResponse, AppendMessageRequest,
        AppendMessageResponse, BatchTranslateTextRequest, BatchTranslateTextResponse,
        CancelRunRequest, CancelRunResponse, CompactNowRequest, CompactNowResponse,
        CompleteStepRequest, CompleteStepResponse, CreateEmbeddingRequest, CreateEmbeddingResponse,
        CreateRealtimeSessionRequest, CreateRealtimeSessionResponse, CreateThreadRequest,
        CreateThreadResponse, CreateVideoGenerationJobRequest, CreateVideoGenerationJobResponse,
        DetectTextLanguageRequest, DetectTextLanguageResponse, Event, ExtractImageTextRequest,
        ExtractImageTextResponse, FinalizeToolActionRequest, FinalizeToolActionResponse,
        GenerateImageRequest, GenerateImageResponse, GeneratedImage, GetContextAssemblyRequest,
        GetContextAssemblyResponse, GetVideoGenerationJobRequest, GetVideoGenerationJobResponse,
        HeartbeatManagedRunRequest, HeartbeatManagedRunResponse, InferChunk, InferResponse,
        LanguageAnalysisResult, ListModelsRequest, ListModelsResponse, ListRunsRequest,
        ListRunsResponse, ListSpeechVoicesRequest, ListSpeechVoicesResponse, ListSystemRunsRequest,
        ListTranslationLanguagesRequest, ListTranslationLanguagesResponse, ManagedRunSource,
        ModelInfo, RecordTerminalOutcomeRequest, RecordTerminalOutcomeResponse,
        ReplayThreadRequest, ReserveToolActionRequest, ReserveToolActionResponse,
        ResolveRunActionAuthorityRequest, ResolveRunActionAuthorityResponse,
        ResolveRunOwnerRequest, ResolveRunOwnerResponse, RunDetail, SaveCheckpointRequest,
        SaveCheckpointResponse, SetAgentSkillEnabledRequest, SetAgentSkillEnabledResponse,
        SpeechVoiceInfo, StartManagedRunRequest, StartManagedRunResponse, StartRunRequest,
        StartRunResponse, StreamVideoGenerationContentRequest,
        StreamVideoGenerationContentResponse, SynthesizeSpeechRequest, SynthesizeSpeechResponse,
        TerminalOutcome, Trajectory, TranscribeSpeechRequest, TranscribeSpeechResponse,
        TranslateTextRequest, TranslateTextResponse, TranslationDetection, TranslationLanguageInfo,
    };
    use mp_events::publisher::InMemoryPublisher;
    use std::{
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::{net::TcpListener, sync::OnceCell};
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Response,
    };
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    type MockInferStream = Pin<Box<dyn futures::Stream<Item = Result<InferChunk, Status>> + Send>>;
    type MockVideoContentStream = Pin<
        Box<
            dyn futures::Stream<Item = Result<StreamVideoGenerationContentResponse, Status>> + Send,
        >,
    >;
    type MockReplayStream = Pin<Box<dyn futures::Stream<Item = Result<Event, Status>> + Send>>;

    struct MockInferenceOk;

    #[tonic::async_trait]
    impl InferenceCore for MockInferenceOk {
        type InferStreamStream = MockInferStream;
        type StreamVideoGenerationContentStream = MockVideoContentStream;

        async fn infer(&self, _: Request<InferRequest>) -> Result<Response<InferResponse>, Status> {
            Ok(Response::new(InferResponse {
                request_id: "req-ok".to_owned(),
                content: "hello".to_owned(),
                model_used: "mock".to_owned(),
                stop_reason: "stop".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
                provider_used: String::new(),
                residency: String::new(),
                tool_calls: Vec::new(),
            }))
        }

        async fn infer_stream(
            &self,
            _: Request<InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Ok(Response::new(Box::pin(futures::stream::iter(vec![
                Ok(InferChunk {
                    request_id: "req-stream".to_owned(),
                    delta: "hel".to_owned(),
                    done: false,
                    model_used: "mock".to_owned(),
                    input_tokens: 0,
                    output_tokens: 0,
                    provider_used: String::new(),
                    residency: String::new(),
                }),
                Ok(InferChunk {
                    request_id: "req-stream".to_owned(),
                    delta: "lo".to_owned(),
                    done: true,
                    model_used: "mock".to_owned(),
                    input_tokens: 2,
                    output_tokens: 3,
                    provider_used: String::new(),
                    residency: String::new(),
                }),
            ]))))
        }

        async fn create_embedding(
            &self,
            _: Request<CreateEmbeddingRequest>,
        ) -> Result<Response<CreateEmbeddingResponse>, Status> {
            Ok(Response::new(CreateEmbeddingResponse {
                request_id: "embed-ok".to_owned(),
                vector: vec![0.1, 0.2, 0.3],
                model_used: "mock-embedding".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_models(
            &self,
            _: Request<ListModelsRequest>,
        ) -> Result<Response<ListModelsResponse>, Status> {
            Ok(Response::new(ListModelsResponse {
                models: vec![ModelInfo {
                    id: "mock-embedding".to_owned(),
                    provider: "mock".to_owned(),
                    modality: "embedding".to_owned(),
                    streaming: false,
                    features: Vec::new(),
                    privacy_tier: 0,
                    residency: String::new(),
                }],
            }))
        }

        async fn synthesize_speech(
            &self,
            _: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
            Ok(Response::new(SynthesizeSpeechResponse {
                request_id: "speech-ok".to_owned(),
                audio: b"audio".to_vec(),
                format: "mp3".to_owned(),
                model_used: "mock-tts".to_owned(),
                provider_used: "mock".to_owned(),
                duration_ms: 0,
            }))
        }

        async fn transcribe_speech(
            &self,
            _: Request<TranscribeSpeechRequest>,
        ) -> Result<Response<TranscribeSpeechResponse>, Status> {
            Ok(Response::new(TranscribeSpeechResponse {
                request_id: "stt-ok".to_owned(),
                text: "hello".to_owned(),
                detected_language: "en".to_owned(),
                confidence: 1.0,
                model_used: "mock-stt".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_speech_voices(
            &self,
            _: Request<ListSpeechVoicesRequest>,
        ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
            Ok(Response::new(ListSpeechVoicesResponse {
                voices: vec![SpeechVoiceInfo {
                    id: "alloy".to_owned(),
                    name: "Alloy".to_owned(),
                    language: "*".to_owned(),
                    gender: "Neutral".to_owned(),
                    provider: "mock".to_owned(),
                }],
            }))
        }

        async fn translate_text(
            &self,
            _: Request<TranslateTextRequest>,
        ) -> Result<Response<TranslateTextResponse>, Status> {
            Ok(Response::new(TranslateTextResponse {
                request_id: "translate-ok".to_owned(),
                translated_text: "hei".to_owned(),
                detected_language: "en".to_owned(),
                confidence: 1.0,
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn batch_translate_text(
            &self,
            _: Request<BatchTranslateTextRequest>,
        ) -> Result<Response<BatchTranslateTextResponse>, Status> {
            Ok(Response::new(BatchTranslateTextResponse {
                request_id: "batch-translate-ok".to_owned(),
                translations: Vec::new(),
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn detect_text_language(
            &self,
            _: Request<DetectTextLanguageRequest>,
        ) -> Result<Response<DetectTextLanguageResponse>, Status> {
            Ok(Response::new(DetectTextLanguageResponse {
                request_id: "detect-ok".to_owned(),
                detections: vec![TranslationDetection {
                    language: "en".to_owned(),
                    confidence: 1.0,
                    is_translation_supported: true,
                }],
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_translation_languages(
            &self,
            _: Request<ListTranslationLanguagesRequest>,
        ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
            Ok(Response::new(ListTranslationLanguagesResponse {
                languages: vec![TranslationLanguageInfo {
                    code: "en".to_owned(),
                    name: "English".to_owned(),
                    native_name: "English".to_owned(),
                    direction: "ltr".to_owned(),
                }],
            }))
        }

        async fn generate_image(
            &self,
            _: Request<GenerateImageRequest>,
        ) -> Result<Response<GenerateImageResponse>, Status> {
            Ok(Response::new(GenerateImageResponse {
                request_id: "image-ok".to_owned(),
                images: vec![GeneratedImage {
                    url: String::new(),
                    b64_json: "aW1hZ2U=".to_owned(),
                    revised_prompt: "mock image".to_owned(),
                }],
                model_used: "mock-image".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_image(
            &self,
            _: Request<AnalyzeImageRequest>,
        ) -> Result<Response<AnalyzeImageResponse>, Status> {
            Ok(Response::new(AnalyzeImageResponse {
                request_id: "vision-ok".to_owned(),
                description: "mock description".to_owned(),
                model_used: "mock-vision".to_owned(),
                provider_used: "mock".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
            }))
        }

        async fn extract_image_text(
            &self,
            _: Request<ExtractImageTextRequest>,
        ) -> Result<Response<ExtractImageTextResponse>, Status> {
            Ok(Response::new(ExtractImageTextResponse {
                request_id: "ocr-ok".to_owned(),
                text: "mock text".to_owned(),
                page_count: 0,
                model_used: "mock-ocr".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_document(
            &self,
            _: Request<AnalyzeDocumentRequest>,
        ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
            Ok(Response::new(AnalyzeDocumentResponse {
                request_id: "document-ok".to_owned(),
                status: "succeeded".to_owned(),
                content: "invoice text".to_owned(),
                fields_json: r#"{"VendorName":"ACME"}"#.to_owned(),
                tables_json: "[]".to_owned(),
                paragraphs: vec!["invoice text".to_owned()],
                raw_json: "{}".to_owned(),
                pages_processed: 1,
                confidence: 0.9,
                model_used: "prebuilt-invoice".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_language(
            &self,
            _: Request<AnalyzeLanguageRequest>,
        ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
            Ok(Response::new(AnalyzeLanguageResponse {
                request_id: "language-ok".to_owned(),
                operation: "sentiment".to_owned(),
                results: vec![LanguageAnalysisResult {
                    id: "1".to_owned(),
                    sentiment: "positive".to_owned(),
                    confidence_scores_json: r#"{"positive":0.9}"#.to_owned(),
                    sentences_json: "[]".to_owned(),
                    entities_json: "[]".to_owned(),
                    key_phrases: Vec::new(),
                    redacted_text: String::new(),
                    content_safety_json: String::new(),
                    detected_language_name: String::new(),
                    detected_language_code: String::new(),
                    confidence: 0.0,
                    summary: String::new(),
                    raw_json: "{}".to_owned(),
                }],
                model_used: "mock-language".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn create_realtime_session(
            &self,
            _: Request<CreateRealtimeSessionRequest>,
        ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
            Ok(Response::new(CreateRealtimeSessionResponse {
                request_id: "realtime-ok".to_owned(),
                session_id: "sess_mock".to_owned(),
                client_secret: "ek_mock".to_owned(),
                websocket_url: "wss://example.test/v1/realtime?model=mock-realtime".to_owned(),
                expires_at: 1234,
                model_used: "mock-realtime".to_owned(),
                provider_used: "mock".to_owned(),
                voice: "alloy".to_owned(),
            }))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<CreateVideoGenerationJobRequest>,
        ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
            Ok(Response::new(CreateVideoGenerationJobResponse {
                request_id: "video-ok".to_owned(),
                job_id: "job_mock".to_owned(),
                status: "queued".to_owned(),
                model_used: "mock-sora".to_owned(),
                provider_used: "mock".to_owned(),
                raw_json: r#"{"id":"job_mock","status":"queued"}"#.to_owned(),
            }))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<GetVideoGenerationJobRequest>,
        ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
            Ok(Response::new(GetVideoGenerationJobResponse {
                request_id: "video-status-ok".to_owned(),
                job_id: "job_mock".to_owned(),
                status: "succeeded".to_owned(),
                generation_id: "gen_mock".to_owned(),
                video_url: "https://example.test/video.mp4".to_owned(),
                error: String::new(),
                model_used: "mock-sora".to_owned(),
                provider_used: "mock".to_owned(),
                raw_json: r#"{"id":"job_mock","status":"succeeded"}"#.to_owned(),
            }))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Ok(Response::new(Box::pin(futures::stream::iter(vec![
                Ok(StreamVideoGenerationContentResponse {
                    request_id: "video-content-ok".to_owned(),
                    generation_id: "gen_mock".to_owned(),
                    data: b"mock-video".to_vec(),
                    done: false,
                    content_type: "video/mp4".to_owned(),
                    content_length: 10,
                    provider_used: "mock".to_owned(),
                }),
                Ok(StreamVideoGenerationContentResponse {
                    request_id: "video-content-ok".to_owned(),
                    generation_id: "gen_mock".to_owned(),
                    data: Vec::new(),
                    done: true,
                    content_type: "video/mp4".to_owned(),
                    content_length: 10,
                    provider_used: "mock".to_owned(),
                }),
            ]))))
        }
    }

    struct MockInferenceDown;

    #[tonic::async_trait]
    impl InferenceCore for MockInferenceDown {
        type InferStreamStream = MockInferStream;
        type StreamVideoGenerationContentStream = MockVideoContentStream;

        async fn infer(&self, _: Request<InferRequest>) -> Result<Response<InferResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn infer_stream(
            &self,
            _: Request<InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_embedding(
            &self,
            _: Request<CreateEmbeddingRequest>,
        ) -> Result<Response<CreateEmbeddingResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_models(
            &self,
            _: Request<ListModelsRequest>,
        ) -> Result<Response<ListModelsResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn synthesize_speech(
            &self,
            _: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn transcribe_speech(
            &self,
            _: Request<TranscribeSpeechRequest>,
        ) -> Result<Response<TranscribeSpeechResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_speech_voices(
            &self,
            _: Request<ListSpeechVoicesRequest>,
        ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn translate_text(
            &self,
            _: Request<TranslateTextRequest>,
        ) -> Result<Response<TranslateTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn batch_translate_text(
            &self,
            _: Request<BatchTranslateTextRequest>,
        ) -> Result<Response<BatchTranslateTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn detect_text_language(
            &self,
            _: Request<DetectTextLanguageRequest>,
        ) -> Result<Response<DetectTextLanguageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_translation_languages(
            &self,
            _: Request<ListTranslationLanguagesRequest>,
        ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn generate_image(
            &self,
            _: Request<GenerateImageRequest>,
        ) -> Result<Response<GenerateImageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_image(
            &self,
            _: Request<AnalyzeImageRequest>,
        ) -> Result<Response<AnalyzeImageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn extract_image_text(
            &self,
            _: Request<ExtractImageTextRequest>,
        ) -> Result<Response<ExtractImageTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_document(
            &self,
            _: Request<AnalyzeDocumentRequest>,
        ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_language(
            &self,
            _: Request<AnalyzeLanguageRequest>,
        ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_realtime_session(
            &self,
            _: Request<CreateRealtimeSessionRequest>,
        ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<CreateVideoGenerationJobRequest>,
        ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<GetVideoGenerationJobRequest>,
        ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Err(Status::unavailable("down"))
        }
    }

    /// The harness previously left `memory_client` pointing at the real
    /// `localhost:9091` default while mocking everything else, so every
    /// non-ZDR `invoke` test reached out of the process for memory context.
    /// That is what hung the suite on a machine where Docker's proxy still
    /// held the port with no container behind it: the TCP connect succeeded,
    /// so there was no fast refusal, and the handshake never completed.
    struct MockMemoryService;

    #[tonic::async_trait]
    impl mp_contracts::model_plane::v1::memory_service_server::MemoryService for MockMemoryService {
        async fn search_memory(
            &self,
            _: Request<mp_contracts::model_plane::v1::SearchMemoryRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::SearchMemoryResponse>, Status> {
            Ok(Response::new(
                mp_contracts::model_plane::v1::SearchMemoryResponse::default(),
            ))
        }

        async fn index_memory(
            &self,
            _: Request<mp_contracts::model_plane::v1::IndexMemoryRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::IndexMemoryResponse>, Status> {
            Err(Status::unimplemented("index_memory not needed in test"))
        }

        async fn list_memory(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListMemoryRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListMemoryResponse>, Status> {
            Err(Status::unimplemented("list_memory not needed in test"))
        }

        async fn delete_memory(
            &self,
            _: Request<mp_contracts::model_plane::v1::DeleteMemoryRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::DeleteMemoryResponse>, Status> {
            Err(Status::unimplemented("delete_memory not needed in test"))
        }

        async fn health(
            &self,
            _: Request<mp_contracts::model_plane::v1::MemoryHealthRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::MemoryHealthResponse>, Status> {
            Ok(Response::new(
                mp_contracts::model_plane::v1::MemoryHealthResponse {
                    status: "ok".to_owned(),
                    ..Default::default()
                },
            ))
        }
    }

    async fn spawn_memory_client(
    ) -> mp_contracts::model_plane::v1::memory_service_client::MemoryServiceClient<
        tonic::transport::Channel,
    > {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind memory");
        let addr = listener.local_addr().expect("memory addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(
                    mp_contracts::model_plane::v1::memory_service_server::MemoryServiceServer::new(
                        MockMemoryService,
                    ),
                )
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("memory endpoint")
            .connect()
            .await
            .expect("connect memory");
        mp_contracts::model_plane::v1::memory_service_client::MemoryServiceClient::new(channel)
    }

    struct MockSessionCore;

    #[tonic::async_trait]
    impl SessionCore for MockSessionCore {
        type ReplayThreadStream = MockReplayStream;

        async fn create_thread(
            &self,
            request: Request<CreateThreadRequest>,
        ) -> Result<Response<CreateThreadResponse>, Status> {
            let req = request.into_inner();
            Ok(Response::new(CreateThreadResponse {
                thread_id: format!("thread-{}", req.session_key),
                created_at: None,
            }))
        }

        async fn append_message(
            &self,
            _: Request<AppendMessageRequest>,
        ) -> Result<Response<AppendMessageResponse>, Status> {
            Ok(Response::new(AppendMessageResponse { sequence: 1 }))
        }

        async fn start_run(
            &self,
            request: Request<StartRunRequest>,
        ) -> Result<Response<StartRunResponse>, Status> {
            let req = request.into_inner();
            Ok(Response::new(StartRunResponse {
                owner_id: String::new(),
                run_id: format!("run-{}", req.thread_id),
                created_at: None,
            }))
        }

        async fn start_scheduled_run(
            &self,
            _: Request<mp_contracts::model_plane::v1::StartScheduledRunRequest>,
        ) -> Result<Response<StartRunResponse>, Status> {
            Err(Status::unimplemented(
                "start_scheduled_run not needed in test",
            ))
        }

        async fn complete_step(
            &self,
            _: Request<CompleteStepRequest>,
        ) -> Result<Response<CompleteStepResponse>, Status> {
            Err(Status::unimplemented("complete_step not needed in test"))
        }

        async fn reserve_tool_action(
            &self,
            _: Request<ReserveToolActionRequest>,
        ) -> Result<Response<ReserveToolActionResponse>, Status> {
            Err(Status::unimplemented(
                "reserve_tool_action not needed in test",
            ))
        }

        async fn finalize_tool_action(
            &self,
            _: Request<FinalizeToolActionRequest>,
        ) -> Result<Response<FinalizeToolActionResponse>, Status> {
            Err(Status::unimplemented(
                "finalize_tool_action not needed in test",
            ))
        }

        async fn save_checkpoint(
            &self,
            _: Request<SaveCheckpointRequest>,
        ) -> Result<Response<SaveCheckpointResponse>, Status> {
            Err(Status::unimplemented("save_checkpoint not needed in test"))
        }

        async fn replay_thread(
            &self,
            _: Request<ReplayThreadRequest>,
        ) -> Result<Response<Self::ReplayThreadStream>, Status> {
            Err(Status::unimplemented("replay_thread not needed in test"))
        }

        async fn get_context_assembly(
            &self,
            _: Request<GetContextAssemblyRequest>,
        ) -> Result<Response<GetContextAssemblyResponse>, Status> {
            Err(Status::unimplemented(
                "get_context_assembly not needed in test",
            ))
        }

        async fn compact_now(
            &self,
            _: Request<CompactNowRequest>,
        ) -> Result<Response<CompactNowResponse>, Status> {
            Err(Status::unimplemented("compact_now not needed in test"))
        }

        async fn set_agent_skill_enabled(
            &self,
            _: Request<SetAgentSkillEnabledRequest>,
        ) -> Result<Response<SetAgentSkillEnabledResponse>, Status> {
            Err(Status::unimplemented(
                "set_agent_skill_enabled not needed in test",
            ))
        }

        async fn upsert_agent_skill(
            &self,
            _: Request<mp_contracts::model_plane::v1::UpsertAgentSkillRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::UpsertAgentSkillResponse>, Status>
        {
            Err(Status::unimplemented(
                "upsert_agent_skill not needed in test",
            ))
        }

        async fn set_run_mode(
            &self,
            _: Request<mp_contracts::model_plane::v1::SetRunModeRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::SetRunModeResponse>, Status> {
            Err(Status::unimplemented("set_run_mode not needed in test"))
        }

        async fn list_agent_skills(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListAgentSkillsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListAgentSkillsResponse>, Status>
        {
            Err(Status::unimplemented(
                "list_agent_skills not needed in test",
            ))
        }

        async fn list_conversation(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListConversationRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListConversationResponse>, Status>
        {
            Err(Status::unimplemented(
                "list_conversation not needed in test",
            ))
        }

        async fn list_threads(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListThreadsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListThreadsResponse>, Status> {
            Ok(Response::new(
                mp_contracts::model_plane::v1::ListThreadsResponse { threads: vec![] },
            ))
        }

        async fn update_thread_presentation(
            &self,
            _: Request<mp_contracts::model_plane::v1::UpdateThreadPresentationRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::UpdateThreadPresentationResponse>, Status>
        {
            Err(Status::unimplemented(
                "update_thread_presentation not needed in test",
            ))
        }

        async fn archive_thread(
            &self,
            _: Request<mp_contracts::model_plane::v1::ArchiveThreadRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ArchiveThreadResponse>, Status>
        {
            Err(Status::unimplemented("archive_thread not needed in test"))
        }

        async fn archive_threads(
            &self,
            _: Request<mp_contracts::model_plane::v1::ArchiveThreadsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ArchiveThreadsResponse>, Status>
        {
            Err(Status::unimplemented("archive_threads not needed in test"))
        }

        // These deliberately remain unimplemented: gateway gRPC unit tests
        // exercise ModelGateway RPCs, while thread erasure is covered at the
        // Session Core/Postgres boundary. They keep this exhaustive mock in
        // lock-step with the generated SessionCore contract so unrelated
        // gateway tests still link after a durable API expansion.
        async fn delete_thread(
            &self,
            _: Request<mp_contracts::model_plane::v1::DeleteThreadRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::DeleteThreadResponse>, Status> {
            Err(Status::unimplemented("delete_thread not needed in test"))
        }

        async fn delete_threads(
            &self,
            _: Request<mp_contracts::model_plane::v1::DeleteThreadsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::DeleteThreadsResponse>, Status>
        {
            Err(Status::unimplemented("delete_threads not needed in test"))
        }

        async fn prepare_scheduled_run_thread(
            &self,
            _: Request<mp_contracts::model_plane::v1::PrepareScheduledRunThreadRequest>,
        ) -> Result<
            Response<mp_contracts::model_plane::v1::PrepareScheduledRunThreadResponse>,
            Status,
        > {
            Err(Status::unimplemented(
                "prepare_scheduled_run_thread not needed in test",
            ))
        }

        async fn claim_scheduled_step(
            &self,
            _: Request<mp_contracts::model_plane::v1::ClaimScheduledStepRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ClaimScheduledStepResponse>, Status>
        {
            Err(Status::unimplemented(
                "claim_scheduled_step not needed in test",
            ))
        }

        async fn record_scheduled_step_receipt(
            &self,
            _: Request<mp_contracts::model_plane::v1::RecordScheduledStepReceiptRequest>,
        ) -> Result<
            Response<mp_contracts::model_plane::v1::RecordScheduledStepReceiptResponse>,
            Status,
        > {
            Err(Status::unimplemented(
                "record_scheduled_step_receipt not needed in test",
            ))
        }

        async fn delete_space_threads(
            &self,
            _: Request<mp_contracts::model_plane::v1::DeleteSpaceThreadsRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::DeleteSpaceThreadsResponse>, Status>
        {
            Err(Status::unimplemented(
                "delete_space_threads not needed in test",
            ))
        }
    }

    #[derive(Clone, Default)]
    struct ManagedLifecycleHandles {
        starts: Arc<Mutex<Vec<StartManagedRunRequest>>>,
        terminal_outcomes: Arc<Mutex<Vec<(String, RecordTerminalOutcomeRequest)>>>,
    }

    #[derive(Clone)]
    struct MockManagedRunLifecycle {
        handles: ManagedLifecycleHandles,
    }

    fn managed_source(source: i32) -> Result<ManagedRunSource, Status> {
        let source = ManagedRunSource::try_from(source)
            .map_err(|_| Status::invalid_argument("invalid managed terminal source"))?;
        if source == ManagedRunSource::Unspecified {
            return Err(Status::invalid_argument(
                "managed terminal source is required",
            ));
        }
        Ok(source)
    }

    fn managed_terminal_step(source: ManagedRunSource) -> &'static str {
        match source {
            ManagedRunSource::GatewayDirect => "model-gateway-direct-inference-final",
            ManagedRunSource::ExecutionAgent => "execution-core-agent-final",
            ManagedRunSource::ExecutionBrowser => "execution-core-browser-final",
            ManagedRunSource::GatewayAgentDispatchRejected => {
                "model-gateway-agent-dispatch-rejected"
            }
            ManagedRunSource::GatewayBrowser => "model-gateway-browser-agent-final",
            ManagedRunSource::Unspecified => "",
        }
    }

    fn authorization<T>(request: &Request<T>) -> String {
        request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    #[tonic::async_trait]
    impl ManagedRunLifecycle for MockManagedRunLifecycle {
        async fn start_managed_run(
            &self,
            request: Request<StartManagedRunRequest>,
        ) -> Result<Response<StartManagedRunResponse>, Status> {
            let request = request.into_inner();
            let source = managed_source(request.terminal_source)?;
            let thread_id = if request.thread_id.is_empty() {
                "metadata-only-thread".to_owned()
            } else {
                request.thread_id.clone()
            };
            self.handles.starts.lock().unwrap().push(request);
            Ok(Response::new(StartManagedRunResponse {
                run_id: format!("managed-run-for-{thread_id}"),
                created_at: None,
                terminal_step_id: managed_terminal_step(source).to_owned(),
                already_started: false,
                thread_id,
            }))
        }

        async fn record_terminal_outcome(
            &self,
            request: Request<RecordTerminalOutcomeRequest>,
        ) -> Result<Response<RecordTerminalOutcomeResponse>, Status> {
            let authorization = authorization(&request);
            let request = request.into_inner();
            let source = managed_source(request.source)?;
            self.handles
                .terminal_outcomes
                .lock()
                .unwrap()
                .push((authorization.clone(), request.clone()));
            if authorization != "Bearer gateway-terminalizer-token" {
                return Err(Status::unauthenticated(
                    "managed terminal receipt requires the scoped service credential",
                ));
            }
            Ok(Response::new(RecordTerminalOutcomeResponse {
                run_id: request.run_id,
                source: source as i32,
                terminal_step_id: managed_terminal_step(source).to_owned(),
                step_index: 1,
                receipt_id: "grpc-managed-receipt".to_owned(),
                applied_at: None,
                already_applied: false,
                reconciliation_required: false,
            }))
        }

        async fn heartbeat_managed_run(
            &self,
            request: Request<HeartbeatManagedRunRequest>,
        ) -> Result<Response<HeartbeatManagedRunResponse>, Status> {
            if authorization(&request) != "Bearer gateway-terminalizer-token" {
                return Err(Status::unauthenticated(
                    "managed heartbeat requires the scoped service credential",
                ));
            }
            let _ = managed_source(request.into_inner().source)?;
            Ok(Response::new(HeartbeatManagedRunResponse {
                renewed_until: None,
                already_terminal: false,
            }))
        }
    }

    /// The only durable run owned by the verified test identity. This mock
    /// deliberately derives the result from the Session Core request, rather
    /// than trusting a gateway-side cache or request field.
    struct MockRunService;

    #[tonic::async_trait]
    impl RunService for MockRunService {
        async fn get_run(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetRunRequest>,
        ) -> Result<Response<RunDetail>, Status> {
            Err(Status::unimplemented("get_run not needed in test"))
        }

        async fn list_runs(
            &self,
            _: Request<ListRunsRequest>,
        ) -> Result<Response<ListRunsResponse>, Status> {
            Err(Status::unimplemented("list_runs not needed in test"))
        }

        async fn list_system_runs(
            &self,
            _: Request<ListSystemRunsRequest>,
        ) -> Result<Response<ListRunsResponse>, Status> {
            Err(Status::unimplemented("list_system_runs not needed in test"))
        }

        async fn cancel_run(
            &self,
            _: Request<CancelRunRequest>,
        ) -> Result<Response<CancelRunResponse>, Status> {
            Err(Status::unimplemented("cancel_run not needed in test"))
        }

        async fn resolve_run_owner(
            &self,
            request: Request<ResolveRunOwnerRequest>,
        ) -> Result<Response<ResolveRunOwnerResponse>, Status> {
            let bearer = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok());
            if bearer != Some("Bearer test-session-bearer") {
                return Err(Status::unauthenticated(
                    "verified session credential required",
                ));
            }
            let request = request.into_inner();
            Ok(Response::new(ResolveRunOwnerResponse {
                authorized: request.run_id == "run-owned"
                    && request.org_id == "org_test"
                    && request.user_id == "user_test",
            }))
        }

        async fn resolve_thread_owner(
            &self,
            _: Request<mp_contracts::model_plane::v1::ResolveThreadOwnerRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ResolveThreadOwnerResponse>, Status>
        {
            Err(Status::unimplemented(
                "resolve_thread_owner not needed in test",
            ))
        }

        async fn resolve_run_action_authority(
            &self,
            _: Request<ResolveRunActionAuthorityRequest>,
        ) -> Result<Response<ResolveRunActionAuthorityResponse>, Status> {
            Err(Status::unimplemented(
                "run action authority not needed in gateway test",
            ))
        }

        async fn get_scheduled_step_context(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetScheduledStepContextRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ScheduledStepContext>, Status> {
            Err(Status::unimplemented(
                "scheduled step context not needed in gateway test",
            ))
        }

        async fn resolve_scheduled_step_authority(
            &self,
            _: Request<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityRequest>,
        ) -> Result<
            Response<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityResponse>,
            Status,
        > {
            Err(Status::unimplemented(
                "scheduled step authority not needed in gateway test",
            ))
        }
    }

    async fn spawn_inference_client<S: InferenceCore>(
        service: S,
    ) -> InferenceCoreClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference");
        let addr = listener.local_addr().expect("inference addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(InferenceCoreServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("inference endpoint")
            .connect()
            .await
            .expect("connect inference");
        InferenceCoreClient::new(channel)
    }

    async fn spawn_session_client<S: SessionCore>(
        service: S,
    ) -> SessionCoreClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind session");
        let addr = listener.local_addr().expect("session addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("session endpoint")
            .connect()
            .await
            .expect("connect session");
        SessionCoreClient::new(channel)
    }

    async fn spawn_managed_lifecycle_client(
        service: MockManagedRunLifecycle,
    ) -> ManagedRunLifecycleClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind managed lifecycle");
        let addr = listener.local_addr().expect("managed lifecycle addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(ManagedRunLifecycleServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("managed lifecycle endpoint")
            .connect()
            .await
            .expect("connect managed lifecycle");
        ManagedRunLifecycleClient::new(channel)
    }

    async fn terminal_auth_core_url() -> String {
        static AUTH_CORE: OnceCell<MockServer> = OnceCell::const_new();
        let auth_core = AUTH_CORE
            .get_or_init(|| async {
                let server = MockServer::start().await;
                Mock::given(method("POST"))
                    .and(path("/api/session-core/internal-token"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "token": "gateway-terminalizer-token",
                        "expiresInSeconds": 300,
                        "audience": "session-core"
                    })))
                    .mount(&server)
                    .await;
                server
            })
            .await;
        auth_core.uri()
    }

    async fn spawn_run_client<S: RunService>(
        service: S,
    ) -> RunServiceClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind runs");
        let addr = listener.local_addr().expect("runs addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(RunServiceServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("runs endpoint")
            .connect()
            .await
            .expect("connect runs");
        RunServiceClient::new(channel)
    }

    async fn test_service_with_lifecycle<S>(
        inference_service: S,
    ) -> (GatewayService, Arc<DynPublisher>, ManagedLifecycleHandles)
    where
        S: InferenceCore,
    {
        let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
        let mut state = AppState::new();
        state.publisher = publisher.clone();
        state.inference_client = spawn_inference_client(inference_service).await;
        state.session_client = spawn_session_client(MockSessionCore).await;
        state.memory_client = spawn_memory_client().await;
        let handles = ManagedLifecycleHandles::default();
        state.managed_run_client = spawn_managed_lifecycle_client(MockManagedRunLifecycle {
            handles: handles.clone(),
        })
        .await;
        state
            .configure_managed_terminalization(
                &terminal_auth_core_url().await,
                "model-gateway",
                "test-model-gateway-service-credential",
            )
            .expect("configure scoped terminalization credential");

        (GatewayService { state }, publisher, handles)
    }

    async fn test_run_ownership_service() -> (GatewayService, Arc<DynPublisher>) {
        let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
        let mut state = AppState::new();
        state.publisher = publisher.clone();
        state.session_client = spawn_session_client(MockSessionCore).await;
        state.run_client = spawn_run_client(MockRunService).await;
        (GatewayService { state }, publisher)
    }

    fn assert_direct_managed_terminal(
        handles: &ManagedLifecycleHandles,
        outcome: TerminalOutcome,
        failure_code: &str,
    ) {
        let starts = handles.starts.lock().unwrap();
        assert_eq!(
            starts.len(),
            1,
            "Gateway must use exactly one managed start"
        );
        assert_eq!(
            starts[0].terminal_source,
            ManagedRunSource::GatewayDirect as i32,
            "Gateway direct inference must bind the direct terminal owner at start"
        );
        let thread_id = starts[0].thread_id.clone();
        drop(starts);

        let receipts = handles.terminal_outcomes.lock().unwrap();
        assert_eq!(receipts.len(), 1, "Gateway must submit exactly one receipt");
        let (authorization, receipt) = &receipts[0];
        assert_eq!(authorization, "Bearer gateway-terminalizer-token");
        assert_eq!(receipt.run_id, format!("managed-run-for-{thread_id}"));
        assert_eq!(receipt.source, ManagedRunSource::GatewayDirect as i32);
        assert_eq!(receipt.outcome, outcome as i32);
        assert_eq!(receipt.failure_code, failure_code);
    }

    fn make_request(content: &str) -> InvokeRequest {
        InvokeRequest {
            request_id: String::new(),
            org_id: "org_test".to_owned(),
            session_key: "sess-1".to_owned(),
            thread_id: String::new(),
            content: content.to_owned(),
            model: String::new(),
            provider: String::new(),
            max_tokens: 128,
            temperature: 0.3,
            stream: false,
            metadata: None,
            structured_output_schema: String::new(),
            zdr: false,
            max_cost_usd: 0.0,
            max_tokens_budget: 0,
            ..Default::default()
        }
    }

    fn authenticated<T>(value: T) -> Request<T> {
        authenticated_as(value, "org_test", "user_test")
    }

    fn authenticated_as<T>(value: T, org_id: &str, user_id: &str) -> Request<T> {
        let mut request = Request::new(value);
        request.extensions_mut().insert(
            crate::grpc_auth::VerifiedIdentity::user_with_downstream_for_test(org_id, user_id),
        );
        request
    }

    fn enter_plan_request(run_id: &str) -> EnterPlanModeRequest {
        EnterPlanModeRequest {
            request_id: "request-enter".to_owned(),
            org_id: "org_test".to_owned(),
            run_id: run_id.to_owned(),
            session_id: "session-test".to_owned(),
            rationale: "review first".to_owned(),
            ttl_seconds: 60,
        }
    }

    fn exit_plan_request(run_id: &str) -> ExitPlanModeRequest {
        ExitPlanModeRequest {
            request_id: "request-exit".to_owned(),
            org_id: "org_test".to_owned(),
            run_id: run_id.to_owned(),
            session_id: "session-test".to_owned(),
        }
    }

    fn record_trajectory_request(run_id: &str) -> RecordTrajectoryRequest {
        RecordTrajectoryRequest {
            request_id: "request-trajectory".to_owned(),
            trajectory: Some(Trajectory {
                trajectory_id: String::new(),
                org_id: "org_test".to_owned(),
                run_id: run_id.to_owned(),
                task_pattern: "security-regression".to_owned(),
                goal: "prove authorization".to_owned(),
                planned_actions: Vec::new(),
                executed_actions: Vec::new(),
                outcome: "success".to_owned(),
                duration_sec: 1.0,
                skills_used: Vec::new(),
                cost_usd: 0.0,
                model: "test".to_owned(),
                created_at_unix: 0,
            }),
        }
    }

    async fn assert_no_run_scoped_side_effects(
        service: &GatewayService,
        publisher: &DynPublisher,
        run_id: &str,
    ) {
        assert!(
            !service.state.plan_mode.is_plan_mode("org_test", run_id).0,
            "an unauthorized run id must not alter plan-mode state"
        );
        let stored = trajectory::handle_list_trajectories(
            &service.state.trajectories,
            ListTrajectoriesRequest {
                request_id: "request-list".to_owned(),
                org_id: "org_test".to_owned(),
                outcome_filter: String::new(),
                pattern_filter: String::new(),
                since_unix: 0,
                limit: 10,
            },
        )
        .await
        .expect("local trajectory inspection");
        assert!(
            stored.trajectories.is_empty(),
            "an unauthorized run id must not enter the trajectory store"
        );
        assert!(
            publisher.drain().is_empty(),
            "an unauthorized run id must not publish a run event"
        );
    }

    #[tokio::test]
    async fn wrong_user_run_id_cannot_mutate_or_publish_plan_or_trajectory_state() {
        let (service, publisher) = test_run_ownership_service().await;
        let attacker = "user-other";
        let run_id = "run-other-user";

        for error in [
            service
                .enter_plan_mode(authenticated_as(
                    enter_plan_request(run_id),
                    "org_test",
                    attacker,
                ))
                .await
                .expect_err("a different user must not enter plan mode for this run"),
            service
                .exit_plan_mode(authenticated_as(
                    exit_plan_request(run_id),
                    "org_test",
                    attacker,
                ))
                .await
                .expect_err("a different user must not exit plan mode for this run"),
            service
                .record_trajectory(authenticated_as(
                    record_trajectory_request(run_id),
                    "org_test",
                    attacker,
                ))
                .await
                .expect_err("a different user must not record this run trajectory"),
        ] {
            assert_eq!(error.code(), tonic::Code::PermissionDenied);
        }

        assert_no_run_scoped_side_effects(&service, &publisher, run_id).await;
    }

    #[tokio::test]
    async fn wrong_org_run_id_cannot_mutate_or_publish_plan_or_trajectory_state() {
        let (service, publisher) = test_run_ownership_service().await;
        // The request still names the caller's authenticated org. The foreign
        // run id is what must be checked against Session Core's durable owner.
        let run_id = "run-other-org";

        for error in [
            service
                .enter_plan_mode(authenticated(enter_plan_request(run_id)))
                .await
                .expect_err("a different org's run must not enter plan mode"),
            service
                .exit_plan_mode(authenticated(exit_plan_request(run_id)))
                .await
                .expect_err("a different org's run must not exit plan mode"),
            service
                .record_trajectory(authenticated(record_trajectory_request(run_id)))
                .await
                .expect_err("a different org's run must not record a trajectory"),
        ] {
            assert_eq!(error.code(), tonic::Code::PermissionDenied);
        }

        assert_no_run_scoped_side_effects(&service, &publisher, run_id).await;
    }

    #[tokio::test]
    async fn durable_run_owner_can_mutate_and_publish_plan_and_trajectory_state() {
        let (service, publisher) = test_run_ownership_service().await;

        service
            .enter_plan_mode(authenticated(enter_plan_request("run-owned")))
            .await
            .expect("durable owner may enter plan mode");
        assert!(
            service
                .state
                .plan_mode
                .is_plan_mode("org_test", "run-owned")
                .0
        );

        let trajectory = service
            .record_trajectory(authenticated(record_trajectory_request("run-owned")))
            .await
            .expect("durable owner may record a trajectory")
            .into_inner();
        assert!(trajectory.published);

        let exited = service
            .exit_plan_mode(authenticated(exit_plan_request("run-owned")))
            .await
            .expect("durable owner may exit plan mode")
            .into_inner();
        assert!(exited.was_active);
        assert!(
            !service
                .state
                .plan_mode
                .is_plan_mode("org_test", "run-owned")
                .0
        );

        let events = publisher.drain();
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|(subject, _)| subject == "mp.v1.run.run-owned.event"));
    }

    #[tokio::test]
    async fn authenticated_send_message_is_quarantined_before_publish() {
        let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
        let mut state = AppState::new();
        state.publisher = publisher.clone();
        let service = GatewayService { state };

        for subject in ["agents.worker-1", "org.org_test.events", "notify.user_test"] {
            let error = service
                .send_message(authenticated(SendMessageRequest {
                    request_id: "request-test".to_owned(),
                    org_id: "org_test".to_owned(),
                    subject: subject.to_owned(),
                    payload_json: r#"{\"message\":\"must not publish\"}"#.to_owned(),
                    idempotency_key: "idempotency-test".to_owned(),
                }))
                .await
                .expect_err("authenticated SendMessage must be quarantined");

            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }

        assert!(
            publisher.drain().is_empty(),
            "the public SendMessage RPC must not publish to an ambient subject"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_empty_content() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let response = service.invoke(authenticated(make_request("   "))).await;
        assert!(response.is_err());
        assert_eq!(
            response.expect_err("must fail").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn invoke_stream_rejects_empty_content() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let response = service.invoke_stream(authenticated(make_request(""))).await;
        assert!(response.is_err());
        assert_eq!(
            response.expect_err("must fail").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let resp = service
            .health(Request::new(HealthRequest {}))
            .await
            .expect("health ok")
            .into_inner();
        assert_eq!(resp.status, "ok");
    }

    #[tokio::test]
    async fn invoke_returns_inference_response_and_emits_events() {
        let (service, publisher, lifecycle) = test_service_with_lifecycle(MockInferenceOk).await;

        let response = service
            .invoke(authenticated(make_request("hello")))
            .await
            .expect("invoke ok")
            .into_inner();

        assert_eq!(response.content, "hello");
        assert_eq!(response.model_used, "mock");

        let drained = publisher.drain();
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.ingress.")));
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.usage.")));
        assert_direct_managed_terminal(&lifecycle, TerminalOutcome::Completed, "");
    }

    #[tokio::test]
    async fn invoke_uses_an_opaque_scoped_idempotency_key_for_its_managed_start() {
        let (service, _, lifecycle) = test_service_with_lifecycle(MockInferenceOk).await;
        let mut request = make_request("hello");
        request.request_id = "request-123".to_owned();
        request.idempotency_key = "retry-key-123".to_owned();

        service
            .invoke(authenticated(request))
            .await
            .expect("invoke with a stable retry key");

        let starts = lifecycle.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert!(starts[0].start_key.starts_with("msk-v1-"));
        assert_ne!(starts[0].start_key, "retry-key-123");
        assert!(
            !starts[0].start_key.contains("retry-key-123"),
            "a durable start key must never echo public client retry data"
        );
    }

    #[tokio::test]
    async fn zdr_invoke_suppresses_gateway_events_and_durable_session_side_effects() {
        let (service, publisher, lifecycle) = test_service_with_lifecycle(MockInferenceOk).await;
        let mut request = make_request("ephemeral prompt");
        request.zdr = true;
        request.idempotency_key = "private-customer-invoice-891".to_owned();

        let response = service
            .invoke(authenticated(request))
            .await
            .expect("ZDR invoke remains usable")
            .into_inner();

        assert_eq!(response.content, "hello");
        assert!(publisher.drain().is_empty(), "ZDR must publish no events");
        let starts = lifecycle.starts.lock().unwrap();
        assert_eq!(
            starts.len(),
            1,
            "ZDR still has one metadata-only managed run"
        );
        assert!(starts[0].goal.is_empty());
        assert!(starts[0].agent_id.is_empty());
        assert!(starts[0].start_key.starts_with("msk-v1-"));
        assert!(
            !starts[0].start_key.contains("private-customer-invoice-891"),
            "raw ZDR idempotency text must not cross the durable StartManagedRun boundary"
        );
    }

    #[tokio::test]
    async fn invoke_returns_internal_when_inference_unavailable() {
        let (service, publisher, lifecycle) = test_service_with_lifecycle(MockInferenceDown).await;

        let error = service
            .invoke(authenticated(make_request("hello")))
            .await
            .expect_err("invoke should fail");
        assert_eq!(error.code(), tonic::Code::Internal);

        let drained = publisher.drain();
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.ingress.")));
        assert!(!drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.usage.")));
        assert_direct_managed_terminal(&lifecycle, TerminalOutcome::Failed, "provider_unavailable");
    }

    #[tokio::test]
    async fn invoke_stream_forwards_chunks_and_done() {
        let (service, _, lifecycle) = test_service_with_lifecycle(MockInferenceOk).await;

        let response = service
            .invoke_stream(authenticated(make_request("hello")))
            .await
            .expect("invoke_stream ok")
            .into_inner();

        let chunks: Vec<_> = response.collect().await;
        assert_eq!(chunks.len(), 2);
        let first = chunks[0].as_ref().expect("first chunk ok");
        assert_eq!(first.delta, "hel");
        assert!(!first.done);
        let last = chunks[1].as_ref().expect("done chunk ok");
        assert_eq!(last.delta, "lo");
        assert!(last.done);
        assert_eq!(last.input_tokens, 2);
        assert_eq!(last.output_tokens, 3);
        assert_direct_managed_terminal(&lifecycle, TerminalOutcome::Completed, "");
    }

    #[tokio::test]
    async fn invoke_stream_returns_internal_when_inference_unavailable() {
        let (service, _, lifecycle) = test_service_with_lifecycle(MockInferenceDown).await;

        let error = service
            .invoke_stream(authenticated(make_request("hello")))
            .await
            .expect_err("invoke_stream should fail");
        assert_eq!(error.code(), tonic::Code::Internal);
        assert_direct_managed_terminal(&lifecycle, TerminalOutcome::Failed, "provider_unavailable");
    }

    #[tokio::test]
    async fn invoke_rejects_forged_tenant_before_downstream_work() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let mut request = make_request("hello");
        request.org_id = "org-other".to_owned();
        let error = service
            .invoke(authenticated(request))
            .await
            .expect_err("cross-tenant invoke must fail");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
    }
}
