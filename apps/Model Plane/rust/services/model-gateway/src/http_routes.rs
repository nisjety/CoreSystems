//! HTTP routes for model-gateway on :8080.
//!
//! Public routes: /healthz, /readyz, /version, /metrics
//! Auth-gated routes: invoke endpoints, orchestration read endpoints, run-event SSE

use std::{convert::Infallible, env, fmt::Write as _};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{
        sse::{Event as SseEvent, Sse},
        IntoResponse, Redirect, Response,
    },
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use metrics_exporter_prometheus::PrometheusHandle;
use mp_contracts::model_plane::v1::InferPurpose;
use mp_contracts::model_plane::v1::{
    AcquireLeaseRequest, DeleteThreadWorkspaceRequest, GetThreadWorkspaceRequest,
    AnalyzeDocumentRequest, AnalyzeImageRequest, AnalyzeLanguageRequest, AnswerRunQuestionRequest,
    Approval, ApprovalCause, ApprovalDecisionSource, ApprovalKind, ApprovalProof, ApprovalScope,
    ApprovalState, BatchTranslateTextRequest,
    CancelRunRequest, ListRunQuestionsRequest, QuestionAnswerItem, QuestionState, RunQuestion,
    ContinuationExecution, CreateEmbeddingRequest, CreateRealtimeSessionRequest,
    CreateVideoGenerationJobRequest, DecideApprovalRequest, DetectTextLanguageRequest, Event,
    EventType, ExtractImageTextRequest, GenerateImageRequest, GetApprovalRequest, GetPlanRequest,
    GetRunProofBundleRequest, GetRunRequest, GetSubagentLineageRequest, GetTodoRequest,
    GetVerificationMetricsRequest, GetVideoGenerationJobRequest, ListApprovalsRequest,
    ListMcpServersRequest, ListModelsRequest, ListPlansRequest, ListRunsRequest,
    ListSpeechVoicesRequest, ListSystemRunsRequest, ListTodosRequest,
    ListProcessesRequest, ListTranslationLanguagesRequest, McpServer, Plan, PlanState, PlanStep,
    PlanStepEdit, PlanStepState, Process as MpProcess, ProcessState as MpProcessState,
    ProcessStream as MpProcessStream, ReadProcessOutputRequest,
    RegisterMcpServerRequest, ReplayThreadRequest, ResumeRunRequest, ResumeRunResponse, RunDetail,
    RunProofBundle, StreamVideoGenerationContentRequest, SubagentLineage, SubagentRole,
    SynthesizeSpeechRequest, Todo, TodoPriority, TodoState, TranscribeSpeechRequest,
    TransitionPlanRequest, TransitionTodoRequest, TranslateTextRequest, TranslationInput,
    VerificationMetrics, VerificationStatus,
};
use mp_contracts::approval_cause::{self, CauseCode, SCOPE_ONCE, SCOPE_RUN, SCOPE_THREAD};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};

use crate::{
    auth::{
        self, Claims, StepUpClaim, VerifiedCapabilityBearer, VerifiedCostBearer,
        VerifiedDataPlaneBearer as VerifiedBearer, VerifiedExecutionBearer,
        VerifiedInferenceBearer, VerifiedSandboxBearer,
        VerifiedSessionBearer as VerifiedModelBearer, VerifiedStepUp,
    },
    gateway_metrics, normalize, rate_limit,
    readiness::GrpcReadiness,
    run_control::{CancelRoute, QueueRoute, RunControlStatus},
    session_flow, sse,
    state::AppState,
};

/// Start the HTTP server on :8080.
///
/// `shutdown` is the graceful-stop trigger. In production it is the signal plus
/// the drain (see [`crate::drain`]), so it only resolves once in-flight streams
/// have finished or the drain window ran out: the listener stays open, and
/// resume, stop and queued input keep working, until then.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(
    state: AppState,
    prom_handle: Option<PrometheusHandle>,
    grpc_readiness: GrpcReadiness,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = build_router_with_readiness(state, prom_handle, grpc_readiness);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("HTTP listening on :8080");
    serve_on(listener, app, shutdown).await
}

/// Serve `app` on an already-bound `listener` until `shutdown` resolves, then
/// stop accepting, close idle connections, and return once the connections that
/// are still open have finished (a streamed answer flushes its last frames
/// before its connection closes). Split out of [`serve`] so the shutdown
/// behaviour is testable on an ephemeral port.
///
/// # Errors
///
/// Returns an error if the server fails while serving.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// Room for the conversation around the attachments in one request body.
const STREAM_BODY_HEADROOM_BYTES: usize = 4 * 1024 * 1024;

/// The body limit of `/v1/invoke/stream`: the explicit limit, raised when the
/// declared attachment total as base64 plus the conversation would not fit, so a
/// raised `CHAT_ATTACHMENT_MAX_TOTAL_BYTES` never fails as a bare 413 in this hop.
/// With the attachment contract off nothing is declared and nothing is derived.
fn stream_body_limit_bytes() -> usize {
    derived_stream_body_limit(
        crate::attachment_policy::stream_body_limit_bytes(),
        crate::attachment_policy::AttachmentPolicy::from_env()
            .declared(false)
            .map(|policy| policy.max_total_bytes),
    )
}

fn derived_stream_body_limit(explicit: usize, declared_total: Option<usize>) -> usize {
    declared_total.map_or(explicit, |total| {
        explicit.max(
            total
                .div_ceil(3)
                .saturating_mul(4)
                .saturating_add(STREAM_BODY_HEADROOM_BYTES),
        )
    })
}

#[cfg(test)]
mod attention_state_tests {
    use super::{attention_reason_name, attention_state_name};

    /// A replica with the attention switch off returns the unspecified state, and the wire omits
    /// it, so a flag-off thread listing is unchanged. The named states are the closed set.
    #[test]
    fn unspecified_is_omitted_and_every_named_state_has_a_wire_name() {
        assert_eq!(attention_state_name(0), None);
        assert_eq!(attention_state_name(1), Some("needs_approval"));
        assert_eq!(attention_state_name(2), Some("needs_input"));
        assert_eq!(attention_state_name(3), Some("plan_ready"));
        assert_eq!(attention_state_name(4), Some("working"));
        assert_eq!(attention_state_name(5), Some("completed"));
        assert_eq!(attention_state_name(6), Some("failed"));
        assert_eq!(attention_state_name(7), Some("archived"));
        assert_eq!(attention_state_name(8), Some("idle"));
        // A number this build does not name reads as absent, never as a guessed state.
        assert_eq!(attention_state_name(9), None);
    }

    /// A reason names the pending row behind a waiting state. Tag 4 belongs to WS03's parked loop
    /// and reads as absent until that slice names it.
    #[test]
    fn reason_names_the_pending_row_and_leaves_the_rest_absent() {
        assert_eq!(attention_reason_name(0), None);
        assert_eq!(attention_reason_name(1), Some("approval"));
        assert_eq!(attention_reason_name(2), Some("plan"));
        assert_eq!(attention_reason_name(3), Some("input"));
        assert_eq!(attention_reason_name(4), None);
    }
}

#[cfg(test)]
mod stream_body_limit_tests {
    use super::{derived_stream_body_limit, STREAM_BODY_HEADROOM_BYTES};
    use crate::attachment_policy::DEFAULT_STREAM_BODY_LIMIT_BYTES as DEFAULT;

    #[test]
    fn todays_declared_total_fits_under_the_default_limit() {
        assert_eq!(derived_stream_body_limit(DEFAULT, Some(3_145_728)), DEFAULT);
        assert_eq!(derived_stream_body_limit(DEFAULT, None), DEFAULT);
    }

    #[test]
    fn a_raised_total_raises_the_limit_to_hold_it_as_base64() {
        let total = 24 * 1024 * 1024;
        let limit = derived_stream_body_limit(DEFAULT, Some(total));
        assert_eq!(limit, total / 3 * 4 + STREAM_BODY_HEADROOM_BYTES);
        assert!(limit > DEFAULT);
    }

    #[test]
    fn an_explicit_limit_above_the_derived_one_wins() {
        let explicit = 64 * 1024 * 1024;
        assert_eq!(
            derived_stream_body_limit(explicit, Some(3_145_728)),
            explicit
        );
    }
}

/// Build the axum router without binding a socket. Useful for tests.
pub fn build_router(state: AppState, prom_handle: Option<PrometheusHandle>) -> Router {
    let grpc_readiness = GrpcReadiness::new();
    grpc_readiness.mark_bound();
    build_router_with_readiness(state, prom_handle, grpc_readiness)
}

/// Build the axum router with the production gRPC-listener readiness gate.
pub fn build_router_with_readiness(
    state: AppState,
    prom_handle: Option<PrometheusHandle>,
    grpc_readiness: GrpcReadiness,
) -> Router {
    let rate_limiter = state.rate_limiter.clone();

    // Public routes — no auth required
    let public = if let Some(handle) = prom_handle {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route("/version", get(version))
            .route(
                "/metrics",
                get(gateway_metrics::metrics_handler).with_state(handle),
            )
    } else {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route("/version", get(version))
            .route("/metrics", get(metrics_placeholder))
    };

    // Auth-gated routes with rate limiting
    let authed = authed_routes()
        .layer(middleware::from_fn(rate_limit::rate_limit_middleware))
        .layer(middleware::from_fn(auth::authorize_principal_route))
        .layer(middleware::from_fn(auth::require_auth))
        .layer(axum::Extension(rate_limiter));

    Router::new()
        .merge(public)
        .merge(authed)
        .layer(Extension(grpc_readiness))
        .layer(middleware::from_fn(gateway_metrics::metrics_middleware))
        .with_state(state)
}

/// Every auth-gated route, without its layers. `build_router_with_readiness` wraps it in
/// `require_auth`, `authorize_principal_route` and the rate limiter; a test wraps it in the route
/// policy alone, to prove every route here answers to it (INNH-10 MP-3).
fn authed_routes() -> Router<AppState> {
    Router::new()
        .route("/v1/invoke", post(invoke))
        // Attachments ride the body as base64, so the 2 MiB default would refuse two
        // images the attachment policy admits (a 413 with no typed reason).
        .route(
            "/v1/invoke/stream",
            post(sse::invoke_stream_sse).layer(DefaultBodyLimit::max(stream_body_limit_bytes())),
        )
        .route("/v1/invoke/resume/:request_id", get(sse::invoke_resume_sse))
        // chat-parity §4: cooperative stop/cancel of an in-flight stream.
        .route("/v1/invoke/:request_id/cancel", post(invoke_cancel))
        // Mid-run user input, delivered at the next tool-round boundary.
        .route("/v1/invoke/:request_id/queue", post(invoke_queue_input))
        // chat-parity §1: reload a thread's conversation (cross-device resume).
        .route("/v1/threads", get(list_threads).delete(delete_threads))
        // Conversation search over the owner's own threads (build plan WS17 S3).
        .route("/v1/threads/search", get(crate::thread_search_routes::search_threads))
        // Archiving remains available as a non-destructive administrative
        // operation; DELETE is reserved for the durable erase contract below.
        .route("/v1/threads/archive", post(archive_threads))
        .route(
            "/v1/threads/:thread_id/presentation",
            post(update_thread_presentation),
        )
        // The attention state of the caller's threads (build plan WS16 S1). It is off by
        // default, and the response says so rather than failing.
        .route("/v1/attention", get(list_attention))
        // The per-person live view of the same state (WS16 S5), relayed as server-sent events.
        .route("/v1/attention/stream", get(stream_attention))
        // Marks one thread read (WS16 S1). A zero-retention caller's read state stays on the
        // client, so Session Core refuses that caller here.
        .route("/v1/threads/:thread_id/seen", post(mark_thread_seen))
        .route("/v1/threads/:thread_id/archive", post(archive_thread))
        // Context inspector (plan item 3.5): what is actually in the model's
        // window for this thread, itemized. session-core has computed this all
        // along (`GetContextAssembly` returns per-segment token estimates) and
        // the gateway already calls it to BUILD prompts — it was simply never
        // exposed, so the one surface that could answer "why did it answer from
        // that?" was unreachable.
        .route("/v1/threads/:thread_id/context", get(get_thread_context))
        .route("/v1/threads/:thread_id", delete(delete_thread))
        .route(
            "/v1/threads/:thread_id/workspace",
            get(get_thread_workspace).delete(delete_thread_workspace),
        )
        // Control-authorized internal deletion adapter. This is deliberately
        // outside the browser/BFF API and is allowed only for the exact
        // deletion coordinator service by auth::authorize_principal_route.
        .route(
            "/v1/internal/space-deletion/threads",
            post(delete_space_threads_for_deletion_coordinator),
        )
        // Delegated-identity design, Gap B (§2.4(ii)): the internal HTTP→gRPC
        // bridge the gateway BFF (Frontend Plane, no tonic/mp-contracts
        // dependency) reaches to call session-core's PrepareUserOwnedThread.
        // Allowed only for `service:verevon-gateway` by
        // `auth::authorize_principal_route`.
        .route(
            "/v1/internal/mail-task/prepare-thread",
            post(prepare_mail_task_thread_for_gateway_bff),
        )
        .route("/v1/threads/:thread_id/messages", get(list_thread_messages))
        // Move a thread's active branch: a version switch or a rewind (build plan WS17 S8).
        .route("/v1/threads/:thread_id/branch", post(branch_thread))
        // Start a new thread from a copy of this one, up to an assistant turn (build plan WS17 S6).
        .route("/v1/threads/:thread_id/fork", post(fork_thread))
        // The thread's goal (Wave 3 EP-7 S1); 404 `thread_goals_disabled` while its switch is off.
        .merge(crate::thread_goal_routes::router())
        // The person's edit and restore of a thread artifact (Wave 3 EP-3 S4); 404 while editing is off.
        .merge(crate::artifact_edit_routes::router())
        // Read-only replay of Session Core's canonical event log. The route
        // exposes envelope metadata only; Any payload bytes remain inside the
        // owning plane and cannot become an accidental browser data channel.
        .route("/v1/threads/:thread_id/events", get(replay_thread_events))
        // Memory management ("what do you remember about me") — user-scoped,
        // backed by session-core's MemoryService.ListMemory/DeleteMemory/
        // IndexMemory. Named `/v1/memories` (plural) to avoid colliding with
        // the unrelated `/v1/memory` capability-core proxy registered in
        // proxy_routes() below.
        //
        // POST reuses IndexMemory — the same write path the Dreaming
        // extractor itself indexes memories through — so the BFF's "correct a
        // memory" flow (create the fix, then delete the original by id) has a
        // real write to call instead of inventing new persistence here.
        .route("/v1/memories", get(list_memories).post(create_memory))
        // The user's own memory state (on / paused / off) and the reset. Static
        // segments win over `:memory_id`, so these never reach the delete route.
        .route(
            "/v1/memories/controls",
            get(get_memory_controls).put(set_memory_controls),
        )
        .route("/v1/memories/reset", post(reset_memory))
        // The standing profile (EP-8 S4): the user bearer only, org and user from
        // the claims. session-core refuses all three while its switch is off.
        .route(
            "/v1/memories/profile",
            get(get_user_profile).put(set_user_profile),
        )
        .route("/v1/memories/profile/rebuild", post(rebuild_user_profile))
        .route("/v1/memories/:memory_id", delete(delete_memory_entry))
        // Space memory for the Space's owner or admin (EP-8 S7). POST so the Control
        // `model.space.memory.manage` decision rides in the body, never a URL; Session Core
        // verifies it and is the authority.
        .route("/v1/spaces/:space_id/memory/list", post(list_space_memories))
        .route("/v1/spaces/:space_id/memory/delete", post(delete_space_memory_entry))
        // Learning governance (WS07-S9): the owner's decisions on learned skills and
        // the ledger of what was learned. The user bearer only; the owner is the
        // verified caller on every hop, and session-core re-checks ownership.
        .route("/v1/learning/ledger", get(list_learning_ledger))
        .route(
            "/v1/learning/proposals/:skill_id/decision",
            post(decide_learning_proposal),
        )
        .route(
            "/v1/learning/ledger/:entry_id/undo",
            post(undo_learning_entry),
        )
        // chat-parity §2: list models + per-model feature families for the picker.
        .route("/v1/models", get(list_models))
        // The Frontend BFF's zero-retention write guard asks what a draft model policy
        // would leave the caller's organisation (see `post_model_access`).
        .route("/v1/models/access", post(post_model_access))
        // chat-parity §2: upload a document into Data Plane (→ retrievable via RAG).
        // Chat-namespaced to avoid colliding with the Data Plane document CRUD
        // route (`POST /v1/documents` in `dataplane_routes`).
        .route("/v1/chat/documents", post(create_document))
        // Orchestration read + mutations
        .merge(orchestration_routes())
        // Operator feedback → skill-promotion signal (HARNESS_PHASE1 §6).
        .route("/v1/feedback", post(ingest_feedback))
        // Run read model — list a thread's runs + a single run's detail (runs
        // history UI). Org-scoped via the verified claims; backed by
        // session-core's RunService.
        .route("/v1/runs", get(list_runs))
        // Sits beside /v1/runs rather than under it: that route is thread-scoped
        // and a system run lives in a thread its own workload owns, so no
        // thread_id a person can supply will ever reach one.
        .route("/v1/runs/system", get(list_system_runs))
        .route("/v1/runs/:run_id", get(get_run))
        // Approving a plan IS the autonomy grant — see `plan_approval`.
        .route("/v1/runs/:run_id/plan-approval", post(plan_approval))
        // Read-only durable browser-event projection used when a Work canvas
        // returns after reload. The live SSE route remains the sole tail
        // transport; this route only reads Session Core's canonical log.
        .route("/v1/runs/:run_id/events/replay", get(replay_run_events))
        // WS10.S6: the run's durable trace (Session Core EventLog), owner only, paged by cursor.
        .route("/v1/runs/:run_id/trace", get(get_run_trace))
        // EP-14 S5: the run's durable, content-free timing and tool-call times, owner only.
        .route("/v1/runs/:run_id/timing", get(get_run_timing))
        // EP-14 S7: one model call's request snapshot, owner only, D3-filtered by Session Core.
        .route("/v1/runs/:run_id/requests/:call_seq", get(get_run_request))
        // Run event SSE
        .route("/v1/runs/:run_id/events", get(sse::run_events_sse))
        // Browser reasoning: Model Plane proposes one safe browser action from
        // Quarry evidence; Quarry remains the only executor/capture layer.
        .route("/v1/browser/suggest-action", post(browser_suggest_action))
        // Wave 3 EP-2 S8: what a wide-research run would look like before it starts. Pure compute;
        // 404 while WIDE_RESEARCH_ENTRY_ENABLED is off.
        .route(
            "/v1/research/wide/estimate",
            post(wide_research_estimate_route),
        )
        // Durable browser-agent run (Phase 2) — start/pause/resume/stop a
        // bounded multi-step browser-agent loop; progress streams over the
        // existing `/v1/runs/:run_id/events` route above.
        .merge(crate::browser_run::router())
        // A signed-in user's browser grant (issue / release); the broker mints it.
        .merge(crate::browser_grants::router())
        // Space room routes /v1/spaces/* (one file per feature under space_routes/).
        .merge(crate::space_routes::router())
        // AI modality routes  /v1/ai/*
        .merge(ai_routes())
        // App-Plane proxies (capabilities/tasks/cron/memory/skills)
        .merge(proxy_routes())
        // --- Data Plane v2 ---
        .merge(dataplane_routes())
        // TOON compact-context encoding
        .route("/v1/toon/encode", post(toon_encode))
        // Wave 7 — fine-tuning lifecycle (gap-model.md §14.9 PAR-30/31/32 v1).
        // POST is admin-gated; GETs are open within the org.
        .route(
            "/v1/finetune/jobs",
            post(crate::finetune_routes::create_job).get(crate::finetune_routes::list_jobs),
        )
        // Wave 7 slice 2e — multipart upload variant of create_job for files
        // larger than what fits comfortably in a JSON body.
        .route(
            "/v1/finetune/jobs/upload",
            post(crate::finetune_routes::create_job_multipart),
        )
        .route(
            "/v1/finetune/jobs/:job_id",
            get(crate::finetune_routes::get_job).delete(crate::finetune_routes::cancel_job),
        )
        // Promote a succeeded job's deployment to a hosting tier (admin-gated).
        // Body { "tier": "production" | "developer" }.
        .route(
            "/v1/finetune/jobs/:job_id/deploy",
            post(crate::finetune_routes::deploy_job),
        )
        // Wave 3 EP-3 S3: artifact revision history and the composer's capability read.
        .merge(crate::artifact_revision_routes::router())
}

/// `/v1/orchestration/*` read + mutation routes. Merged into the auth-gated router.
fn orchestration_routes() -> Router<AppState> {
    Router::new()
        // read
        .route("/v1/orchestration/runs/:run_id/plans", get(list_plans))
        .route("/v1/orchestration/plans/:plan_id", get(get_plan))
        .route(
            "/v1/orchestration/threads/:thread_id/todos",
            get(list_todos),
        )
        .route("/v1/orchestration/todos/:todo_id", get(get_todo))
        .route(
            "/v1/orchestration/runs/:run_id/approvals",
            get(list_approvals),
        )
        .route(
            "/v1/orchestration/runs/:run_id/questions",
            get(list_run_questions),
        )
        .route(
            "/v1/orchestration/runs/:run_id/proof-bundle",
            get(get_run_proof_bundle),
        )
        .route(
            "/v1/orchestration/verification-metrics",
            get(get_verification_metrics),
        )
        .route(
            "/v1/orchestration/approvals/:approval_id",
            get(get_approval),
        )
        .route(
            "/v1/orchestration/approvals/:approval_id/quarantine-preview",
            get(get_quarantine_preview),
        )
        .route(
            "/v1/orchestration/approvals/:approval_id/preview",
            get(get_approval_preview),
        )
        .route(
            "/v1/orchestration/threads/:thread_id/lineage",
            get(get_subagent_lineage),
        )
        // mutations
        .route(
            "/v1/orchestration/plans/:plan_id/approve",
            post(approve_plan),
        )
        .route("/v1/orchestration/plans/:plan_id/reject", post(reject_plan))
        .route(
            "/v1/orchestration/todos/:todo_id/status",
            post(update_todo_status),
        )
        .route(
            "/v1/orchestration/approvals/:approval_id/decide",
            post(decide_approval),
        )
        .route(
            "/v1/orchestration/questions/:question_id/answer",
            post(answer_run_question),
        )
        .route("/v1/orchestration/runs/:run_id/cancel", post(cancel_run))
        .route("/v1/orchestration/runs/:run_id/resume", post(resume_run))
        .route(
            "/v1/orchestration/runs/:run_id/continue",
            post(crate::sse::continue_run),
        )
        // The org's brand deck templates for the picker (Wave 3 EP-5 S7); 404 while
        // SKILL_DECK_TEMPLATES_ENABLED is off.
        .route("/v1/deck-templates", get(list_deck_templates))
}

/// App-Plane proxy routes (`/v1/capabilities`, `/v1/tasks`, `/v1/cron`,
/// `/v1/memory`, `/v1/skills`). Merged into the auth-gated router.
fn proxy_routes() -> Router<AppState> {
    Router::new()
        // Capabilities
        .route("/v1/capabilities", get(list_capabilities_proxy))
        .route("/v1/capabilities/:id", get(get_capability_proxy))
        // MCP servers — add/list/remove an MCP server (and thus its tools) over
        // HTTP. The gateway exposes registered tools to the model automatically
        // (see runtime_registries::mcp_tool_defs).
        .route("/v1/mcp/servers", get(mcp_list).post(mcp_register))
        .route("/v1/mcp/servers/:server_id", delete(mcp_delete))
        .route("/v1/mcp/servers/:server_id/share", post(mcp_share))
        // OAuth 2.1 + Dynamic Client Registration for MCP servers that require
        // real user auth (Visma Net, etc.) — connect with nothing but the
        // server's URL, no pre-registered app.
        .route("/v1/mcp/servers/oauth/start", post(mcp_oauth_start))
        .route("/v1/mcp/servers/oauth/callback", get(mcp_oauth_callback))
        // The single auto-detecting entry point: caller supplies only name +
        // url (+ optional token/allowlist/scope) and the gateway itself
        // discovers whether OAuth is needed — no transport or auth-kind
        // choice is ever asked for. Preferred over the two routes above,
        // which remain for direct/explicit use.
        .route("/v1/mcp/servers/connect", post(mcp_connect))
        // What an MCP server's tools are, and a person's review of one whose
        // definition changed (WS05-S2); capability-core decides who may approve.
        .route(
            "/v1/mcp/servers/:server_id/tools",
            get(list_mcp_server_tools_proxy),
        )
        .route(
            "/v1/mcp/servers/:server_id/tools/:tool/approve",
            post(approve_mcp_tool_proxy),
        )
        // Per-tool allow / ask / never policies, for the caller and (admins) the
        // org (WS05-S5). capability-core owns the rules and the audit.
        .route("/v1/tool-policies", get(list_tool_policies_proxy))
        .route(
            "/v1/tool-policies/:capability_id",
            put(set_tool_policy_proxy).delete(revoke_tool_policy_proxy),
        )
        // The org agent policy and its durable hook rules (WS25-S10). capability-core owns both and
        // decides who may write them from the signed principal; the org comes from the verified token,
        // never from a body.
        .route(
            "/v1/org/agent-policy",
            get(get_org_agent_policy_proxy).put(set_org_agent_policy_proxy),
        )
        .route(
            "/v1/org/agent-policy/effective",
            get(get_org_agent_policy_effective_proxy),
        )
        .route(
            "/v1/org/hooks",
            get(list_org_hooks_proxy).post(create_org_hook_proxy),
        )
        .route(
            "/v1/org/hooks/:hook_id",
            put(update_org_hook_proxy).delete(delete_org_hook_proxy),
        )
        // Tasks
        .route("/v1/tasks", get(list_tasks_proxy).post(create_task_proxy))
        .route("/v1/tasks/:id", get(get_task_proxy).patch(patch_task_proxy))
        .route("/v1/tasks/:id/cancel", post(cancel_task_proxy))
        // Cron
        .route("/v1/processes", get(list_processes))
        .route("/v1/processes/:id/output", get(read_process_output))
        .route("/v1/cron", get(list_cron_proxy).post(create_cron_proxy))
        .route(
            "/v1/cron/:id",
            get(get_cron_proxy)
                .patch(patch_cron_proxy)
                .delete(delete_cron_proxy),
        )
        .route("/v1/cron/:id/fires", get(list_cron_fires_proxy))
        .route("/v1/cron/:id/run", post(run_cron_now_proxy))
        // Memory
        .route(
            "/v1/memory",
            get(list_memory_proxy).post(create_memory_proxy),
        )
        .route(
            "/v1/memory/:id",
            get(get_memory_proxy)
                .patch(patch_memory_proxy)
                .delete(delete_memory_proxy),
        )
        // Skills
        .route(
            "/v1/skills",
            get(list_skills_proxy).post(create_skill_proxy),
        )
        .route(
            "/v1/skills/:id",
            get(get_skill_proxy)
                .patch(patch_skill_proxy)
                .delete(delete_skill_proxy),
        )
        // Plugins
        .route(
            "/v1/plugins",
            get(list_plugins_proxy).post(create_plugin_proxy),
        )
        .route(
            "/v1/plugins/:id",
            get(get_plugin_proxy)
                .patch(patch_plugin_proxy)
                .delete(delete_plugin_proxy),
        )
        // Run watchers (AUTO-2): "notify me when this run finishes". Backed
        // by capability-core's run_watch_subscriptions; org_id/user_id are
        // never in this proxy's control, only the verified capability bearer
        // capability-core's own principal derives them from.
        .route(
            "/v1/runs/:run_id/watchers",
            get(get_run_watchers_proxy)
                .post(create_run_watcher_proxy)
                .delete(delete_run_watcher_proxy),
        )
}

// ---------------------------------------------------------------------------
// Wave 3 EP-5 S7: `GET /v1/deck-templates` - the org's brand deck templates for the picker.
// ---------------------------------------------------------------------------

/// `GET /v1/deck-templates` - see [`deck_templates_listing`].
async fn list_deck_templates(
    state: State<AppState>,
    claims: Extension<Claims>,
    bearer: Option<Extension<VerifiedCapabilityBearer>>,
) -> Result<Json<Value>, HttpJsonError> {
    deck_templates_listing(state, claims, bearer, crate::deck_templates::enabled()).await
}

/// The deck templates the caller may pick, `{data: {templates: [{skill_id, name, description, colors,
/// fonts}]}}`, display fields only (`crate::deck_templates`). With the switch off the route is a 404 as if
/// it did not exist, decided before the bearer is looked at. A zero-retention bearer is refused before
/// anything is read, with feature `skills`: a ZDR turn never mounts or applies a template, so there is
/// nothing to pick. capability-core takes the org from the caller's verified capability bearer; no org
/// from a header, query or body is read here.
async fn deck_templates_listing(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: Option<Extension<VerifiedCapabilityBearer>>,
    enabled: bool,
) -> Result<Json<Value>, HttpJsonError> {
    if !enabled {
        return Err(run_trace_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such route",
        ));
    }
    if claims.zdr {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                mp_contracts::zdr_refusal::Refusal::feature_not_supported(
                    crate::deck_templates::ZDR_FEATURE,
                )
                .json_body(),
            ),
        ));
    }
    let Some(Extension(bearer)) = bearer.filter(|Extension(bearer)| !bearer.as_str().trim().is_empty())
    else {
        return Err(run_trace_error(
            StatusCode::UNAUTHORIZED,
            "capability_credential_required",
            "listing deck templates needs a verified capability credential",
        ));
    };
    let templates = crate::deck_templates::list_for_caller(
        &state,
        &claims.org_id,
        &claims.user_id,
        bearer.as_str(),
    )
    .await
    .map_err(|status| grpc_status_to_http(&status))?;
    Ok(Json(json!({ "data": { "templates": templates } })))
}

/// Request body cap for the two audio routes, `/v1/ai/dictate` and
/// `/v1/ai/speech`. Their audio arrives base64-encoded inside JSON, so axum's
/// 2 MiB default held a recording to about 1.5 MiB. 8 MiB matches the BFF's
/// limit on the same routes; every other route keeps the default.
const AI_AUDIO_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// `/v1/ai/*` modality routes (chat, embeddings, images, speech, translate,
/// documents, language, realtime, video). Merged into the auth-gated router.
fn ai_routes() -> Router<AppState> {
    Router::new()
        .route("/v1/ai/chat", post(ai_chat))
        .route("/v1/recommend/plan", post(recommend_plan))
        .route("/v1/ai/embeddings", post(ai_embeddings))
        .route("/v1/ai/models", get(ai_models))
        .route("/v1/ai/images", post(ai_images))
        .route("/v1/ai/images/analyze", post(ai_images_analyze))
        .route("/v1/ai/images/ocr", post(ai_images_ocr))
        .route(
            "/v1/ai/speech",
            post(ai_speech).layer(DefaultBodyLimit::max(AI_AUDIO_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/ai/dictate",
            post(ai_dictate).layer(DefaultBodyLimit::max(AI_AUDIO_BODY_LIMIT_BYTES)),
        )
        .route("/v1/ai/speech/voices", get(ai_speech_voices))
        .route("/v1/ai/translate", post(ai_translate))
        .route("/v1/ai/translate/detect", post(ai_translate_detect))
        .route("/v1/ai/translate/languages", get(ai_translate_languages))
        .route("/v1/ai/documents", post(ai_documents))
        .route("/v1/ai/documents/analyze", post(ai_documents_analyze))
        .route("/v1/ai/documents/layout", post(ai_documents_layout))
        .route("/v1/ai/documents/forms", post(ai_documents_forms))
        .route("/v1/ai/documents/receipts", post(ai_documents_receipts))
        .route("/v1/ai/documents/invoices", post(ai_documents_invoices))
        .route("/v1/ai/language", post(ai_language))
        .route("/v1/ai/language/sentiment", post(ai_language_sentiment))
        .route("/v1/ai/language/entities", post(ai_language_entities))
        .route("/v1/ai/language/key-phrases", post(ai_language_key_phrases))
        .route("/v1/ai/language/pii", post(ai_language_pii))
        .route("/v1/ai/language/detect", post(ai_language_detect))
        .route(
            "/v1/ai/language/summary/text",
            post(ai_language_summary_text),
        )
        .route("/v1/ai/realtime", post(ai_realtime))
        .route("/v1/ai/realtime/session", post(ai_realtime))
        .route("/v1/ai/realtime/models", get(ai_realtime_models))
        .route("/v1/ai/video/generate", post(ai_video_generate))
        .route("/v1/ai/video/jobs/:job_id", get(ai_video_job))
        .route(
            "/v1/ai/video/generations/:generation_id/content",
            get(ai_video_content),
        )
        .route("/v1/ai/video/models", get(ai_video_models))
}

/// Data Plane v2 routes (documents, retrieval, knowledge, graph, wiki). Merged
/// into the auth-gated router.
fn dataplane_routes() -> Router<AppState> {
    Router::new()
        // Documents
        .route(
            "/v1/documents",
            get(crate::dataplane::list_documents).post(crate::dataplane::create_document),
        )
        .route("/v1/documents/bulk", post(crate::dataplane::bulk_ingest))
        .route(
            "/v1/documents/:id",
            get(crate::dataplane::get_document).delete(crate::dataplane::delete_document),
        )
        .route(
            "/v1/documents/:id/index-status",
            get(crate::dataplane::get_document_index_status),
        )
        // Retrieval
        .route("/v1/retrieval", post(crate::dataplane::retrieve))
        .route(
            "/v1/retrieval/traces/:trace_id",
            get(crate::dataplane::get_retrieval_trace),
        )
        .route("/v1/retrieval/sources", post(crate::dataplane::get_sources))
        .route("/v1/retrieval/chunks", post(crate::dataplane::get_chunks))
        .route("/v1/retrieval/pack", post(crate::dataplane::pack_context))
        // Knowledge
        .route(
            "/v1/knowledge/:document_id/units",
            get(crate::dataplane::get_knowledge_units),
        )
        .route(
            "/v1/knowledge/:document_id/permissions",
            get(crate::dataplane::check_permissions),
        )
        // Graph
        .route("/v1/graph/entities", get(crate::dataplane::list_entities))
        .route(
            "/v1/graph/entities/:entity_id",
            get(crate::dataplane::get_entity),
        )
        .route(
            "/v1/graph/entities/:entity_id/relationships",
            get(crate::dataplane::get_relationships),
        )
        .route(
            "/v1/graph/entities/:entity_id/claims",
            get(crate::dataplane::get_claims),
        )
        .route("/v1/graph/expand", post(crate::dataplane::expand_graph))
        .route(
            "/v1/graph/contradictions",
            get(crate::dataplane::get_contradictions),
        )
        // Wiki
        .route("/v1/wiki/pages", post(crate::dataplane::create_wiki_page))
        .route(
            "/v1/wiki/pages/by-path",
            get(crate::dataplane::get_wiki_page_by_path),
        )
        .route(
            "/v1/wiki/pages/:page_id",
            get(crate::dataplane::get_wiki_page).post(crate::dataplane::update_wiki_page),
        )
        .route(
            "/v1/wiki/pages/:page_id/versions",
            get(crate::dataplane::list_wiki_page_versions),
        )
        .route(
            "/v1/wiki/pages/:page_id/sources",
            get(crate::dataplane::get_wiki_page_sources),
        )
        .route(
            "/v1/wiki/pages/:page_id/backlinks",
            get(crate::dataplane::get_wiki_backlinks),
        )
        .route(
            "/v1/wiki/maintenance",
            get(crate::dataplane::list_wiki_maintenance_issues),
        )
        .route(
            "/v1/wiki/proposals",
            post(crate::dataplane::submit_wiki_proposal),
        )
        .route(
            "/v1/wiki/proposals/:proposal_id/review",
            post(crate::dataplane::review_wiki_proposal),
        )
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(
    State(state): State<AppState>,
    Extension(grpc_readiness): Extension<GrpcReadiness>,
) -> Response {
    // A draining process is finishing the streams it has and must get no new
    // ones, so it reports not ready whatever its listeners are doing.
    if grpc_readiness.is_draining() {
        (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response()
    } else if grpc_readiness.is_bound() {
        // A degraded run-control bus (the NATS ACL without its subjects, a broker that would
        // not connect) behaves as off, so the gateway is still ready: the state is told, not
        // enforced. With the bus off the answer is the one it always was.
        match state.run_control.status() {
            RunControlStatus::Off => (StatusCode::OK, "ok".to_owned()).into_response(),
            status => (StatusCode::OK, format!("ok run_control={}", status.as_str()))
                .into_response(),
        }
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "grpc listener not bound").into_response()
    }
}

async fn metrics_placeholder() -> impl IntoResponse {
    (StatusCode::OK, "# HELP model_gateway_requests_total\n")
}

/// Reports what is actually running in this container: the git revision and
/// build timestamp baked into the image (see `Dockerfile`'s `SOURCE_REVISION`
/// / `BUILD_DATE` build args, re-exposed as runtime `ENV` so no Docker/registry
/// access is needed to answer "what SHA is deployed here"). Falls back to the
/// same `unverified`/`unknown` defaults the image LABELs use when unset.
async fn version() -> Json<Value> {
    Json(json!({
        "service": "model-gateway",
        "revision": env::var("SOURCE_REVISION").unwrap_or_else(|_| "unverified".to_string()),
        "build_date": env::var("BUILD_DATE").unwrap_or_else(|_| "unknown".to_string()),
        "cargo_version": env!("CARGO_PKG_VERSION"),
    }))
}

pub(crate) type HttpJsonError = (StatusCode, Json<Value>);

/// `/v1/invoke`'s error: a status and JSON body as elsewhere, plus a
/// `Retry-After` header for a subscription failure whose wait is known.
pub(crate) struct InvokeError {
    status: StatusCode,
    body: Json<Value>,
    retry_after_secs: Option<u64>,
}

impl From<HttpJsonError> for InvokeError {
    fn from((status, body): HttpJsonError) -> Self {
        Self {
            status,
            body,
            retry_after_secs: None,
        }
    }
}

impl IntoResponse for InvokeError {
    fn into_response(self) -> Response {
        let mut response = (self.status, self.body).into_response();
        if let Some(seconds) = self.retry_after_secs {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
        }
        response
    }
}

/// The unary answer to a failed inference call.
///
/// A subscription failure is worded and statused by its code, with its
/// content-free timing in the body and a `Retry-After` header when known.
/// The body also carries `retry_at`, the absolute instant the wait ends,
/// because a proxy may not pass the header on. `/v1/invoke` sends one message,
/// so it is worded as a request rather than a thread. Anything else is a 502
/// carrying the status message alone: never the tonic `Display`, which prints
/// the trailers too.
fn invoke_inference_error(
    error: &tonic::Status,
    subscription: Option<&crate::subscription_failure::SubscriptionFailure>,
) -> InvokeError {
    let Some(failure) = subscription else {
        // A model the caller's organisation does not allow, refused by
        // inference-core's own check, keeps its catalogue code and status.
        if let Some(refusal) = mp_contracts::model_entitlement::Refusal::from_status(error) {
            return crate::model_access::http(&refusal).into();
        }
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": error.message() })),
        )
            .into();
    };
    let now = Utc::now();
    let mut body = json!({
        "error": crate::subscription_failure::message_at(
            failure,
            crate::subscription_failure::FailureContext::Request,
            now,
        ),
        "code": failure.code,
        "retryable": crate::subscription_failure::retryable(failure.code),
    });
    if let Some(resets_at) = failure.resets_at_label() {
        body["resets_at"] = json!(resets_at);
    }
    if let Some(retry_after_ms) = failure.retry_after_ms {
        body["retry_after_ms"] = json!(retry_after_ms);
    }
    if let Some(retry_at) = failure.retry_at_label(now) {
        body["retry_at"] = json!(retry_at);
    }
    InvokeError {
        status: crate::subscription_failure::http_status(failure.code),
        body: Json(body),
        retry_after_secs: crate::subscription_failure::retry_after_header_secs(failure, now),
    }
}

/// Attach the separately verified `aud=inference-core` credential to one
/// downstream RPC. A verified compact JWT uses only metadata-safe characters;
/// validation happens in the auth middleware before this type can exist.
fn authenticated_inference_request<T>(
    value: T,
    bearer: &VerifiedInferenceBearer,
) -> tonic::Request<T> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", bearer.as_str())
            .parse()
            .expect("a verified compact JWT is valid gRPC metadata"),
    );
    request
}

pub(crate) fn authenticated_session_request<T>(
    value: T,
    bearer: &VerifiedModelBearer,
) -> Result<tonic::Request<T>, HttpJsonError> {
    let mut request = tonic::Request::new(value);
    let authorization = format!("Bearer {}", bearer.as_str()).parse().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "verified session credential is not forwardable"})),
        )
    })?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// Session Core is the authoritative owner of a run. HTTP handlers may use a
/// path/body run id only after this verified, tenant- and user-bound lookup;
/// NATS publication is never an authorization boundary.
async fn require_durable_run_owner(
    state: &AppState,
    claims: &Claims,
    run_id: &str,
    session_bearer: &VerifiedModelBearer,
) -> Result<(), HttpJsonError> {
    session_flow::require_durable_run_owner_with_token(
        state,
        run_id,
        &claims.org_id,
        &claims.user_id,
        session_bearer.as_str(),
    )
    .await
    .map_err(|error| grpc_status_to_http(&error))
}

fn authenticated_execution_request<T>(
    value: T,
    execution_bearer: &VerifiedExecutionBearer,
    session_bearer: &VerifiedModelBearer,
) -> Result<tonic::Request<T>, HttpJsonError> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", execution_bearer.as_str())
            .parse()
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "verified execution credential is not forwardable"})),
                )
            })?,
    );
    request.metadata_mut().insert(
        "x-session-authorization",
        format!("Bearer {}", session_bearer.as_str())
            .parse()
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "verified session credential is not forwardable"})),
                )
            })?,
    );
    Ok(request)
}

fn require_execution_resume_ack(
    response: ResumeRunResponse,
) -> Result<ResumeRunResponse, HttpJsonError> {
    if response.resumed {
        return Ok(response);
    }
    Err((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": {
                "code": "execution_resume_unavailable",
                "message": "Execution did not acknowledge the run resume"
            }
        })),
    ))
}

fn should_resume_granted_approval(
    prior: Option<&Approval>,
    decided: &Approval,
) -> Result<bool, HttpJsonError> {
    let Some(prior) = prior else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "approval_delivery_unknown",
                    "message": "Approval state is unavailable; execution was not resumed"
                }
            })),
        ));
    };
    if prior.id.is_empty()
        || prior.id != decided.id
        || prior.run_id != decided.run_id
        || prior.org_id != decided.org_id
        || decided.state != ApprovalState::Granted as i32
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "code": "approval_identity_mismatch",
                    "message": "Approval identity changed during decision; execution was not resumed"
                }
            })),
        ));
    }
    if prior.state == ApprovalState::Requested as i32 {
        return Ok(true);
    }
    if prior.state == ApprovalState::Granted as i32 {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "approval_delivery_unknown",
                    "message": "Approval is granted but execution delivery is unknown"
                }
            })),
        ));
    }
    Err((
        StatusCode::CONFLICT,
        Json(json!({
            "error": {
                "code": "approval_not_resumable",
                "message": "Approval is not in a resumable state"
            }
        })),
    ))
}

pub(crate) fn require_non_zdr_durable_mutation(claims: &Claims) -> Result<(), HttpJsonError> {
    let zdr = crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false))?;
    if !zdr {
        return Ok(());
    }
    Err((
        StatusCode::PRECONDITION_FAILED,
        Json(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.json_body()),
    ))
}

#[derive(Debug, Default, Deserialize)]
struct TodosQuery {
    run_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ApprovalsQuery {
    step_id: Option<String>,
}

/// Query params for `GET /v1/runs` (runs-history list). `thread_id` scopes the
/// list to one conversation; the rest are optional filter/pagination knobs.
#[derive(Debug, Default, Deserialize)]
struct RunsQuery {
    thread_id: Option<String>,
    status: Option<String>,
    after: Option<String>,
    limit: Option<u32>,
    /// Control's shared-Space read authority, relayed verbatim. This gateway
    /// neither mints nor inspects it — session-core is the verifier, and a
    /// gateway that could interpret it would be a second opinion about who may
    /// read a room's work.
    space_id: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

/// Query params for `GET /v1/runs/system`. Deliberately has NO `org_id`: the
/// tenant comes from the verified claims, so it cannot be chosen by the caller.
#[derive(Debug, Default, Deserialize)]
struct SystemRunsQuery {
    status: Option<String>,
    after: Option<String>,
    limit: Option<u32>,
}

async fn list_plans(
    State(state): State<AppState>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_plans(authenticated_session_request(
            ListPlansRequest {
                run_id,
                thread_id: String::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "plans": response.plans.iter().map(plan_value).collect::<Vec<_>>(),
    })))
}

async fn get_plan(
    State(state): State<AppState>,
    bearer: VerifiedModelBearer,
    Path(plan_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_plan(authenticated_session_request(
            GetPlanRequest { plan_id },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let plan = response.plan.ok_or_else(|| not_found("plan not found"))?;
    Ok(Json(json!({ "plan": plan_value(&plan) })))
}

async fn list_todos(
    State(state): State<AppState>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Query(query): Query<TodosQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_todos(authenticated_session_request(
            ListTodosRequest {
                thread_id,
                run_id: query.run_id.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "todos": response.todos.iter().map(todo_value).collect::<Vec<_>>(),
    })))
}

async fn get_todo(
    State(state): State<AppState>,
    bearer: VerifiedModelBearer,
    Path(todo_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_todo(authenticated_session_request(
            GetTodoRequest { todo_id },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let todo = response.todo.ok_or_else(|| not_found("todo not found"))?;
    Ok(Json(json!({ "todo": todo_value(&todo) })))
}

async fn list_approvals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
    Query(query): Query<ApprovalsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_approvals(authenticated_session_request(
            ListApprovalsRequest {
                run_id,
                step_id: query.step_id.unwrap_or_default(),
                // Cross-org IDOR fix (Phase 6): scope to the caller's verified
                // JWT org, never a client-suppliable value.
                org_id: claims.org_id.clone(),
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "approvals": response.approvals.iter().map(approval_value).collect::<Vec<_>>(),
    })))
}

/// The caller's own questions for a run (build plan WS14 S3). A question is a durable store of its
/// own, so a ZDR or waived caller is refused here and never reaches it; Session Core answers only
/// for the run's owner and scopes the org to the verified credential.
async fn list_run_questions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
    let response = state
        .orchestration_client
        .clone()
        .list_run_questions(authenticated_session_request(
            ListRunQuestionsRequest {
                run_id,
                org_id: claims.org_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| question_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "questions": response.questions.iter().map(run_question_value).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct AnswerQuestionItemBody {
    item_id: String,
    #[serde(default)]
    option_ids: Vec<String>,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Default, Deserialize)]
struct AnswerQuestionBody {
    #[serde(default)]
    answers: Vec<AnswerQuestionItemBody>,
    #[serde(default)]
    skip: bool,
}

/// Answer, or skip, one of the caller's questions once (build plan WS14 S3). An answer is
/// information, never an authority grant: this route cannot decide an approval and shares no state
/// with one. A ZDR or waived caller is refused before anything is read.
async fn answer_run_question(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(question_id): Path<String>,
    Json(body): Json<AnswerQuestionBody>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
    let response = state
        .orchestration_client
        .clone()
        .answer_run_question(authenticated_session_request(
            AnswerRunQuestionRequest {
                question_id,
                org_id: claims.org_id.clone(),
                answers: body
                    .answers
                    .into_iter()
                    .map(|answer| QuestionAnswerItem {
                        item_id: answer.item_id,
                        option_ids: answer.option_ids,
                        text: answer.text,
                    })
                    .collect(),
                skip: body.skip,
                // The message that carries an answer is recorded by the server when the answer
                // arrives on a turn; this route never takes it from the caller.
                answered_message_id: String::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| question_status_to_http(&e))?
        .into_inner();
    let question = response
        .question
        .ok_or_else(|| question_error(StatusCode::NOT_FOUND, "question_not_found", json!({})))?;
    Ok(Json(json!({ "question": run_question_value(&question) })))
}

/// A typed question error body: `{ "error": { "code", "message", "details" } }`.
fn question_error(status: StatusCode, code: &str, details: Value) -> HttpJsonError {
    let message = match code {
        "question_not_found" => "The question was not found.",
        "question_expired" => "The question has expired and can no longer be answered.",
        "question_already_answered" => "The question was already answered.",
        _ => "The answer does not fit the question.",
    };
    let mut body = json!({ "error": { "code": code, "message": message } });
    body["error"]["details"] = details;
    (status, Json(body))
}

/// Map Session Core's question statuses to their typed HTTP errors: `question_not_found` 404,
/// `question_expired` 410, `question_already_answered` 409 and `invalid_answer` 422 (with the
/// refused field and reason). Anything else, a typed ZDR refusal included, takes the general map.
fn question_status_to_http(error: &tonic::Status) -> HttpJsonError {
    match (error.code(), error.message()) {
        (tonic::Code::NotFound, "question_not_found") => {
            question_error(StatusCode::NOT_FOUND, "question_not_found", json!({}))
        }
        (tonic::Code::FailedPrecondition, "question_expired") => {
            question_error(StatusCode::GONE, "question_expired", json!({}))
        }
        (tonic::Code::AlreadyExists, "question_already_answered") => {
            question_error(StatusCode::CONFLICT, "question_already_answered", json!({}))
        }
        (tonic::Code::InvalidArgument, message) if message.starts_with("invalid_answer: ") => {
            let problem = &message["invalid_answer: ".len()..];
            let (field, reason) = problem.rsplit_once(": ").unwrap_or(("answers", problem));
            question_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_answer",
                json!({ "field": field, "reason": reason }),
            )
        }
        _ => grpc_status_to_http(error),
    }
}

fn question_state_token(state: i32) -> &'static str {
    match QuestionState::try_from(state).unwrap_or(QuestionState::Unspecified) {
        QuestionState::Requested => "requested",
        QuestionState::Answered => "answered",
        QuestionState::Skipped => "skipped",
        QuestionState::Expired => "expired",
        QuestionState::Cancelled => "cancelled",
        QuestionState::Unspecified => "unspecified",
    }
}

/// The question as the browser reads it. The owner's own org and user ids stay off the wire.
pub(crate) fn run_question_value(question: &RunQuestion) -> Value {
    json!({
        "id": question.id,
        "run_id": question.run_id,
        "thread_id": question.thread_id,
        "step_id": empty_to_null(&question.step_id),
        "state": question_state_token(question.state),
        "delivery": question.delivery,
        "items": question.items.iter().map(|item| json!({
            "id": item.id,
            "kind": item.kind,
            "prompt": item.prompt,
            "options": item.options.iter().map(|option| json!({
                "id": option.id,
                "label": option.label,
                "description": empty_to_null(&option.description),
            })).collect::<Vec<_>>(),
            "allow_other": item.allow_other,
        })).collect::<Vec<_>>(),
        "requested_at": question.requested_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "expires_at": question.expires_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "answered_at": question.answered_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "answers": question.answers.iter().map(|answer| json!({
            "item_id": answer.item_id,
            "option_ids": answer.option_ids,
            "text": answer.text,
        })).collect::<Vec<_>>(),
        "answered_message_id": empty_to_null(&question.answered_message_id),
    })
}

/// The Verevon Proof Bundle for one run (roadmap P1.2). Read-only; the org is
/// taken from the caller's verified JWT, never from a client-suppliable value.
async fn get_run_proof_bundle(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_run_proof_bundle(authenticated_session_request(
            GetRunProofBundleRequest {
                run_id,
                org_id: claims.org_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let bundle = response
        .bundle
        .as_ref()
        .map(proof_bundle_value)
        .unwrap_or(Value::Null);
    Ok(Json(json!({ "bundle": bundle })))
}

/// `since`, an RFC 3339 timestamp, windows the metrics to continuations
/// finalized (or approvals requested) at or after that instant. Omitted means
/// all-time. Deliberately a plain timestamp rather than a `days=7|30|90`
/// enum: the caller (today, the verevonv3 gateway; eventually a dashboard)
/// computes the boundary itself, so this endpoint carries no opinion about
/// which windows are meaningful.
#[derive(Debug, Default, Deserialize)]
struct VerificationMetricsQuery {
    since: Option<String>,
}

async fn get_verification_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<VerificationMetricsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let since = query
        .since
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .map(|parsed| prost_types::Timestamp {
                    seconds: parsed.timestamp(),
                    nanos: parsed.timestamp_subsec_nanos() as i32,
                })
                .map_err(|_| -> HttpJsonError {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "since must be an RFC 3339 timestamp" })),
                    )
                })
        })
        .transpose()?;

    let response = state
        .orchestration_client
        .clone()
        .get_verification_metrics(authenticated_session_request(
            GetVerificationMetricsRequest {
                org_id: claims.org_id.clone(),
                since,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let metrics = response
        .metrics
        .as_ref()
        .map(verification_metrics_value)
        .unwrap_or(Value::Null);
    Ok(Json(json!({ "metrics": metrics })))
}

async fn get_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(approval_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_approval(authenticated_session_request(
            GetApprovalRequest {
                approval_id,
                // Cross-org IDOR fix (Phase 6): scope to the caller's verified
                // JWT org, never a client-suppliable value.
                org_id: claims.org_id.clone(),
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let approval = response
        .approval
        .ok_or_else(|| not_found("approval not found"))?;
    Ok(Json(json!({ "approval": approval_value(&approval) })))
}

/// `GET /v1/orchestration/approvals/{id}/quarantine-preview` — the inert preview (plain text, at
/// most 1 KiB) of output held for review (build plan WS10 S8). Session Core answers only the run's
/// owner while the approval is pending or granted-and-unspent; every other case is the same 404.
/// The org is the verified JWT org, never a caller value. The text is never markup: show it as text.
async fn get_quarantine_preview(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(approval_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_quarantine_preview(authenticated_session_request(
            mp_contracts::model_plane::v1::GetQuarantinePreviewRequest {
                org_id: claims.org_id.clone(),
                approval_id,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    Ok(Json(json!({
        "preview": {
            "text": response.preview,
            "truncated": response.truncated,
            "tool_key": response.tool_key,
            "trust_class": response.trust_class,
            "format": "plain_text",
        }
    })))
}

/// The full-draft preview switch (EP-10-S1). Off: the route answers 404 `preview_unavailable` and
/// nothing reaches Session Core. Only the literal `true` enables it.
const APPROVAL_WRITE_PREVIEW_ENV: &str = "APPROVAL_WRITE_PREVIEW_ENABLED";

fn approval_write_preview_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(APPROVAL_WRITE_PREVIEW_ENV).ok().as_deref())
}

/// `GET /v1/orchestration/approvals/{id}/preview` - see [`approval_preview`].
async fn get_approval_preview(
    state: State<AppState>,
    claims: Extension<Claims>,
    bearer: VerifiedModelBearer,
    path: Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    approval_preview(state, claims, bearer, path, approval_write_preview_enabled()).await
}

/// The full draft a pending approval would release, `{data: {preview}}` (EP-10-S1, decision D4).
/// Session Core decrypts the descriptor for the run's owner only and shapes it; the gateway adds
/// nothing and logs nothing of it. A zero-retention bearer is refused before anything is read
/// (`continuation_gate`, as for a decision), and Session Core refuses a waived one. The org is the
/// verified JWT org. Every way of not seeing a draft reads as the same 404.
async fn approval_preview(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(approval_id): Path<String>,
    enabled: bool,
) -> Result<Json<Value>, HttpJsonError> {
    if !enabled {
        return Err(run_trace_error(
            StatusCode::NOT_FOUND,
            "preview_unavailable",
            "the approval preview is not available",
        ));
    }
    crate::zdr_posture::continuation_gate(&claims)?;
    let response = state
        .orchestration_client
        .clone()
        .get_approval_preview(authenticated_session_request(
            mp_contracts::model_plane::v1::GetApprovalPreviewRequest {
                approval_id,
                org_id: claims.org_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|status| approval_preview_status_to_http(&status))?
        .into_inner();
    let preview = response
        .preview
        .as_ref()
        .ok_or_else(|| approval_preview_status_to_http(&tonic::Status::not_found("no preview")))?;
    let mut value = approval_preview_value(preview);
    value["can_edit"] = json!(preview_can_edit(preview, approval_edit_enabled()));
    Ok(Json(json!({ "data": { "preview": value } })))
}

/// Whether the card may offer edit mode (EP-10-S6): the edit switch is on and the draft is a mail
/// send that Session Core shaped with fields a person may change. A decision with an edit is
/// refused while the switch is off, so the card never offers one then.
fn preview_can_edit(
    preview: &mp_contracts::model_plane::v1::ApprovalPreview,
    edit_enabled: bool,
) -> bool {
    edit_enabled
        && !preview.editable_fields.is_empty()
        && !preview.raw_only
        && preview.kind == "mail_send"
}

/// Session Core's answer to a preview read. A typed ZDR refusal keeps its catalogue body; another
/// owner's approval, a decided or expired one and an older Session Core all read as one 404.
fn approval_preview_status_to_http(status: &tonic::Status) -> HttpJsonError {
    if mp_contracts::zdr_refusal::Refusal::from_status(status).is_some() {
        return grpc_status_to_http(status);
    }
    match status.code() {
        tonic::Code::NotFound | tonic::Code::PermissionDenied | tonic::Code::Unimplemented => {
            run_trace_error(
                StatusCode::NOT_FOUND,
                "approval_not_found",
                "no such pending approval",
            )
        }
        tonic::Code::InvalidArgument => run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval_preview",
            "the approval id was refused",
        ),
        _ => grpc_status_to_http(status),
    }
}

fn approval_preview_value(preview: &mp_contracts::model_plane::v1::ApprovalPreview) -> Value {
    json!({
        "kind": preview.kind,
        "provider": preview.provider,
        "operation": preview.operation,
        "connection_id": preview.connection_id,
        "to": preview.to,
        "subject": preview.subject,
        "body_text": preview.body_text,
        "has_html": preview.has_html,
        "is_reply": preview.is_reply,
        "raw_only": preview.raw_only,
        "editable_fields": preview.editable_fields,
        "action_fingerprint": preview.action_fingerprint,
        "revision": preview.revision,
        "format": "plain_text",
    })
}

async fn get_subagent_lineage(
    State(state): State<AppState>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_subagent_lineage(authenticated_session_request(
            GetSubagentLineageRequest { thread_id },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let lineage = response
        .lineage
        .ok_or_else(|| not_found("subagent lineage not found"))?;
    Ok(Json(json!({ "lineage": lineage_value(&lineage) })))
}

// ============================================================================
// Run read model (runs-history UI) — session-core RunService
// ============================================================================

/// `GET /v1/runs?thread_id&status&after&limit` — list a thread's runs,
/// newest-first, for the runs-history rail. `status` filters by run status,
/// `after` is a ULID cursor, `limit` bounds the page. The verified claims gate
/// access; session-core owns the run metadata.
///
/// `thread_id` is required UNLESS a Space read decision
/// (`space_id` + `space_read_decision_ref` + `space_read_decision_token`) is
/// supplied, which lists the runs of every thread that decision admits — the
/// room's own work rather than one conversation's. Session-core decides whether
/// the decision authorizes anything; this only refuses the request that names
/// neither a thread nor a Space, which could not be answered either way.
async fn list_runs(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let thread_id = query.thread_id.unwrap_or_default();
    let space_id = query.space_id.unwrap_or_default();
    if thread_id.is_empty() && space_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "thread_id or space_id is required" })),
        ));
    }

    let response = state
        .run_client
        .clone()
        .list_runs(authenticated_session_request(
            ListRunsRequest {
                thread_id,
                status_filter: query.status.unwrap_or_default(),
                after_run_id: query.after.unwrap_or_default(),
                limit: query.limit.unwrap_or(0),
                space_id,
                space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
                space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "runs": response.runs.iter().map(run_detail_value).collect::<Vec<_>>(),
        "has_more": response.has_more,
    })))
}

/// `GET /v1/runs/system?status&after&limit` — the org's runs with no human owner.
///
/// Cron-fired workflows own their runs themselves (session-core's
/// `SYSTEM_RUN_OWNERS`), and those runs live in threads the same workload owns, so
/// the thread-scoped list above cannot reach them: they would exist, be fully
/// authorized, and be invisible. This is the only way to see them.
///
/// The org is taken from the VERIFIED claims and never from the query string —
/// session-core enforces an exact match, so accepting one from the caller would
/// only turn a cross-tenant attempt into a confusing permission error instead of
/// refusing it here. Read-only: system runs are org-readable but mutable solely by
/// the workload that owns them.
async fn list_system_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<SystemRunsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .run_client
        .clone()
        .list_system_runs(authenticated_session_request(
            ListSystemRunsRequest {
                org_id: claims.org_id.clone(),
                status_filter: query.status.unwrap_or_default(),
                after_run_id: query.after.unwrap_or_default(),
                limit: query.limit.unwrap_or(0),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "runs": response.runs.iter().map(run_detail_value).collect::<Vec<_>>(),
        "has_more": response.has_more,
    })))
}

/// `GET /v1/runs/{run_id}` — one run's full detail for the telemetry panel.
async fn get_run(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let detail = state
        .run_client
        .clone()
        .get_run(authenticated_session_request(
            GetRunRequest { run_id },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({ "run": run_detail_value(&detail) })))
}

#[derive(Debug, Deserialize)]
struct ReplayRunEventsQuery {
    /// Resume after this canonical event id (exclusive).
    #[serde(default)]
    after_event_id: Option<String>,
    /// Bounded page size. The value applies to the underlying canonical log,
    /// before the browser-only projection is filtered.
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DurableRunEventProjection {
    event_id: String,
    event_type: String,
    at: Value,
    payload: Value,
}

#[derive(Debug, Serialize)]
struct ReplayRunEventsResponse {
    run_id: String,
    events: Vec<DurableRunEventProjection>,
    /// Cursor for the underlying canonical page, not merely the last browser
    /// event. This lets a client make progress through pages containing only
    /// unrelated thread events.
    next_event_id: Option<String>,
    truncated: bool,
}

/// Read the persisted, safe browser-agent event projection for one run, plus
/// the run's terminal marker.
///
/// The run lookup first proves ownership and gives us the owning thread. The
/// canonical ReplayThread RPC then supplies envelope + JSON payloads; see
/// [`project_run_replay_event`] for what is selected and what leaves this
/// boundary. A terminal entry (`terminal: true`) lets a client stop polling.
async fn replay_run_events(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
    Query(query): Query<ReplayRunEventsQuery>,
) -> Result<Json<ReplayRunEventsResponse>, HttpJsonError> {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "run_id is required" })),
        ));
    }
    const DEFAULT_LIMIT: u32 = 160;
    const MAX_LIMIT: u32 = 500;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let after_event_id = query.after_event_id.unwrap_or_default().trim().to_owned();

    // `GetRun` is the run-level owner check. Do not rely on a caller-supplied
    // thread id, and do not turn a missing run into an empty successful page.
    let detail = state
        .run_client
        .clone()
        .get_run(authenticated_session_request(
            GetRunRequest {
                run_id: trimmed.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    if detail.thread_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "run has no owning thread" })),
        ));
    }

    let response = state
        .session_client
        .clone()
        .replay_thread(authenticated_session_request(
            ReplayThreadRequest {
                thread_id: detail.thread_id,
                after_event_id,
                limit,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core replay_thread failed", &e))?;

    let mut stream = response.into_inner();
    let mut events = Vec::new();
    let mut raw_count = 0usize;
    let mut next_event_id = None;
    while let Some(event) = stream
        .message()
        .await
        .map_err(|e| session_thread_error("session-core replay_thread failed", &e))?
    {
        raw_count += 1;
        next_event_id = Some(event.event_id.clone());
        if let Some(projection) = project_run_replay_event(trimmed, event) {
            events.push(projection);
        }
    }

    Ok(Json(ReplayRunEventsResponse {
        run_id: trimmed.to_owned(),
        events,
        next_event_id,
        truncated: raw_count >= limit as usize,
    }))
}

// ---------------------------------------------------------------------------
// WS10.S6: `GET /v1/runs/:run_id/trace` â€” a run's durable trace from Session Core's EventLog.
// ---------------------------------------------------------------------------

/// Default and bounds of one trace page. Session Core clamps the same way (0 reads 100).
const RUN_TRACE_DEFAULT_LIMIT: u32 = 100;
const RUN_TRACE_MAX_LIMIT: u32 = 500;
const RUN_TRACE_MAX_CURSOR_LEN: usize = 512;
const RUN_TRACE_MAX_RUN_ID_LEN: usize = 64;
const RUN_TRACE_MAX_TYPES: usize = 32;
/// The `type_url` Session Core gives a trace row's payload; its `value` is the JSON object Quarry
/// sent (`eventlog_grpc.rs` `TRACE_PAYLOAD_TYPE_URL`).
const RUN_TRACE_PAYLOAD_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.BrowserTraceEvent";

#[derive(Debug, Deserialize)]
struct RunTraceQuery {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    /// Comma list of `EventType` names without the `EVENT_TYPE_` prefix.
    #[serde(default)]
    types: Option<String>,
}

fn run_trace_error(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

/// A run id as Session Core's trace store keeps it: one `[A-Za-z0-9_-]` token of at most 64.
fn valid_trace_run_id(run_id: &str) -> bool {
    (1..=RUN_TRACE_MAX_RUN_ID_LEN).contains(&run_id.len())
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// `types=A,B` to EventType numbers. Blank items are skipped; an unknown name is a 400 (a typo
/// must not read as "no events"); `UNSPECIFIED` is not a filter.
fn parse_trace_types(raw: Option<&str>) -> Result<Vec<i32>, HttpJsonError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut types = Vec::new();
    for name in raw.split(',').map(str::trim).filter(|name| !name.is_empty()) {
        let kind = EventType::from_str_name(&format!("EVENT_TYPE_{name}"))
            .filter(|kind| *kind != EventType::Unspecified)
            .ok_or_else(|| {
                run_trace_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_trace_types",
                    "types must be EventType names without the EVENT_TYPE_ prefix",
                )
            })?;
        if !types.contains(&(kind as i32)) {
            types.push(kind as i32);
        }
    }
    if types.len() > RUN_TRACE_MAX_TYPES {
        return Err(run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_trace_types",
            "too many event types",
        ));
    }
    Ok(types)
}

/// One trace event in the shape of the run-trace envelope golden
/// (`api/contracts/run-trace-envelope.golden.json`): names without their enum prefixes, the
/// payload as the JSON object Quarry sent. A payload of any other type, or one that is not a JSON
/// object, is reported by its type only: raw bytes never leave the gateway.
fn trace_event_json(event: &Event) -> Value {
    let event_type = EventType::try_from(event.event_type).map_or("UNSPECIFIED", |kind| {
        kind.as_str_name().trim_start_matches("EVENT_TYPE_")
    });
    let origin = mp_contracts::model_plane::v1::RunOrigin::try_from(event.origin)
        .ok()
        .filter(|origin| *origin != mp_contracts::model_plane::v1::RunOrigin::Unspecified)
        .map(|origin| origin.as_str_name().trim_start_matches("RUN_ORIGIN_").to_owned());
    let ts = event.ts.as_ref().and_then(|ts| {
        let nanos = u32::try_from(ts.nanos).ok()?;
        chrono::DateTime::from_timestamp(ts.seconds, nanos)
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    });
    let payload = event.payload.as_ref().map(|any| {
        let object = (any.type_url == RUN_TRACE_PAYLOAD_TYPE_URL)
            .then(|| serde_json::from_slice::<Value>(&any.value).ok())
            .flatten()
            .filter(Value::is_object);
        object.unwrap_or_else(|| json!({ "type_url": any.type_url }))
    });
    let agent = event.agent.as_ref().map(|agent| {
        json!({ "id": agent.id, "role": agent.role, "parent_id": agent.parent_id })
    });
    json!({
        "event_id": event.event_id,
        "event_type": event_type,
        "schema_version": event.schema_version,
        "ts": ts,
        "producer": event.producer,
        "correlation_id": event.correlation_id,
        "causation_id": event.causation_id,
        "org_id": event.org_id,
        "user_id": event.user_id,
        "resource_ref": event.resource_ref,
        "payload": payload,
        "zdr": event.zdr,
        "producer_seq": event.producer_seq,
        "agent": agent,
        "origin": origin,
        "run_id": event.run_id,
        "request_id": event.request_id,
    })
}

/// Session Core's answer to a trace read. A non-owner and an unknown run look the same (404); a
/// Session Core that does not serve the trace yet is a 501 the caller can tell from an outage.
fn run_trace_status_to_http(status: &tonic::Status) -> HttpJsonError {
    match status.code() {
        tonic::Code::NotFound | tonic::Code::PermissionDenied => {
            run_trace_error(StatusCode::NOT_FOUND, "run_not_found", "no such run")
        }
        tonic::Code::Unimplemented => run_trace_error(
            StatusCode::NOT_IMPLEMENTED,
            "trace_unavailable",
            "the run trace is not available",
        ),
        tonic::Code::InvalidArgument => run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_trace_query",
            "the trace query was refused",
        ),
        _ => grpc_status_to_http(status),
    }
}

/// `GET /v1/runs/:run_id/trace?cursor=&limit=&types=` â€” one page of the run's trace, oldest first.
/// The org is the token's; Session Core checks that the caller owns the run (the user behind the
/// session bearer), so this route adds no ownership rule of its own.
async fn get_run_trace(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
    Query(query): Query<RunTraceQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let run_id = run_id.trim();
    if !valid_trace_run_id(run_id) {
        return Err(run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_run_id",
            "run_id must be 1 to 64 characters of [A-Za-z0-9_-]",
        ));
    }
    let limit = query
        .limit
        .unwrap_or(RUN_TRACE_DEFAULT_LIMIT)
        .clamp(1, RUN_TRACE_MAX_LIMIT);
    let cursor = query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty());
    if cursor.is_some_and(|cursor| cursor.len() > RUN_TRACE_MAX_CURSOR_LEN) {
        return Err(run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "the cursor is too long",
        ));
    }
    let event_types = parse_trace_types(query.types.as_deref())?;
    let request = mp_contracts::model_plane::v1::QueryEventsRequest {
        filter: Some(mp_contracts::model_plane::v1::EventFilter {
            org_id: claims.org_id.clone(),
            run_id: run_id.to_owned(),
            event_types,
            ..Default::default()
        }),
        cursor: cursor.map(|value| mp_contracts::model_plane::v1::Cursor {
            value: value.to_owned(),
        }),
        limit,
        reverse: false,
    };
    let page = state
        .event_log_client
        .clone()
        .query_events(authenticated_session_request(request, &bearer)?)
        .await
        .map_err(|status| run_trace_status_to_http(&status))?
        .into_inner();
    let events: Vec<Value> = page.events.iter().map(trace_event_json).collect();
    let next_cursor = page
        .next_cursor
        .map(|cursor| cursor.value)
        .filter(|value| !value.is_empty());
    Ok(Json(json!({
        "data": { "events": events },
        "meta": { "next_cursor": next_cursor, "has_more": page.has_more },
    })))
}

// ---------------------------------------------------------------------------
// EP-14 S5: `GET /v1/runs/:run_id/timing` - a run's durable, content-free timing.
// ---------------------------------------------------------------------------

/// A stored timestamp as RFC 3339 UTC to the millisecond, the shape the trace route uses.
fn timing_rfc3339(ts: Option<&prost_types::Timestamp>) -> Option<String> {
    let ts = ts?;
    let nanos = u32::try_from(ts.nanos).ok()?;
    chrono::DateTime::from_timestamp(ts.seconds, nanos)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// One timing record as the SPA reads it: the report as the JSON object it was stored as. A report
/// that does not parse is reported as absent, never as its raw text.
fn run_timing_json(timing: &mp_contracts::model_plane::v1::RunTimingRecorded) -> Value {
    let report = serde_json::from_str::<Value>(&timing.report_json)
        .ok()
        .filter(Value::is_object);
    json!({
        "run_id": timing.run_id,
        "request_id": timing.request_id,
        "schema": timing.schema,
        "report": report,
        "phases_dropped": timing.phases_dropped,
        "recorded_at": timing_rfc3339(timing.recorded_at.as_ref()),
    })
}

fn run_action_timing_json(action: &mp_contracts::model_plane::v1::RunActionTiming) -> Value {
    json!({
        "action_id": action.action_id,
        "tool": action.tool,
        "status": action.status,
        "reserved_at": timing_rfc3339(action.reserved_at.as_ref()),
        "finalized_at": timing_rfc3339(action.finalized_at.as_ref()),
        "effect_class": (!action.effect_class.is_empty()).then_some(action.effect_class.as_str()),
    })
}

/// Session Core's answer to a timing read. A non-owner, an unknown run and a Session Core with the read
/// switched off all read as 404, so the route says nothing about a run the caller cannot see.
fn run_timing_status_to_http(status: &tonic::Status) -> HttpJsonError {
    match status.code() {
        tonic::Code::NotFound | tonic::Code::PermissionDenied => {
            run_trace_error(StatusCode::NOT_FOUND, "run_not_found", "no such run")
        }
        tonic::Code::Unimplemented => run_trace_error(
            StatusCode::NOT_FOUND,
            "run_timing_unavailable",
            "run timing is not available",
        ),
        tonic::Code::InvalidArgument => run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_run_id",
            "the run id was refused",
        ),
        _ => grpc_status_to_http(status),
    }
}

/// `GET /v1/runs/:run_id/timing` - the run's timing records and its tool calls' reserve and finalize
/// times, `{data: {timings, actions, truncated}}`. Session Core checks that the caller owns the run (the
/// user behind the session bearer). A zero-retention bearer is refused before anything is read, with
/// feature `run_inspection`: there is nothing of a ZDR run to inspect, and the refusal says so.
async fn get_run_timing(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    if claims.zdr {
        return Err((
            StatusCode::FORBIDDEN,
            Json(mp_contracts::zdr_refusal::Refusal::feature_not_supported("run_inspection").json_body()),
        ));
    }
    let run_id = run_id.trim();
    if !valid_trace_run_id(run_id) {
        return Err(run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_run_id",
            "run_id must be 1 to 64 characters of [A-Za-z0-9_-]",
        ));
    }
    let response = state
        .run_ledger_client
        .clone()
        .get_run_timing(authenticated_session_request(
            mp_contracts::model_plane::v1::GetRunTimingRequest {
                run_id: run_id.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|status| run_timing_status_to_http(&status))?
        .into_inner();
    let timings: Vec<Value> = response.timings.iter().map(run_timing_json).collect();
    let actions: Vec<Value> = response.actions.iter().map(run_action_timing_json).collect();
    Ok(Json(json!({
        "data": { "timings": timings, "actions": actions, "truncated": response.truncated },
    })))
}

// ---------------------------------------------------------------------------
// EP-14 S7: `GET /v1/runs/:run_id/requests/:call_seq` - the request inspector.
// ---------------------------------------------------------------------------

/// The inspector switch. Off: the route answers 404 as if it did not exist. Only the literal `true` enables it.
const REQUEST_INSPECTOR_ENV: &str = "MODEL_GATEWAY_REQUEST_INSPECTOR";

fn request_inspector_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(REQUEST_INSPECTOR_ENV).ok().as_deref())
}

/// A stored JSON text as the value it holds; text that does not parse is reported as absent, never raw.
fn stored_json(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw).ok()
}

/// Session Core's answer to an inspector read. Every way of not seeing a call (another user's run, an org
/// admin, no such call, the read switched off) reads as 404, so the route confirms nothing.
fn run_request_status_to_http(status: &tonic::Status) -> HttpJsonError {
    match status.code() {
        tonic::Code::NotFound | tonic::Code::PermissionDenied | tonic::Code::Unimplemented => {
            run_trace_error(
                StatusCode::NOT_FOUND,
                "run_request_not_found",
                "no such request",
            )
        }
        tonic::Code::InvalidArgument => run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_run_request",
            "the run id or call number was refused",
        ),
        _ => grpc_status_to_http(status),
    }
}

/// `GET /v1/runs/:run_id/requests/:call_seq` - see [`run_request_inspection`].
async fn get_run_request(
    state: State<AppState>,
    claims: Extension<Claims>,
    bearer: VerifiedModelBearer,
    path: Path<(String, String)>,
) -> Result<Json<Value>, HttpJsonError> {
    run_request_inspection(state, claims, bearer, path, request_inspector_enabled()).await
}

/// One model call as the inspector shows it: `{data: {call_seq, call_purpose, snapshot, system_sections,
/// tools, tools_truncated, system_truncated, dropped}}`. Session Core checks that the caller is the user who
/// owns the run and applies decision D3 (platform sections carry no text); the gateway adds nothing to it. With
/// the switch off the route is a 404; a zero-retention bearer is refused before anything is read, with
/// feature `run_inspection`.
async fn run_request_inspection(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path((run_id, call_seq)): Path<(String, String)>,
    enabled: bool,
) -> Result<Json<Value>, HttpJsonError> {
    if !enabled {
        return Err(run_trace_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such route",
        ));
    }
    if claims.zdr {
        return Err((
            StatusCode::FORBIDDEN,
            Json(mp_contracts::zdr_refusal::Refusal::feature_not_supported("run_inspection").json_body()),
        ));
    }
    let run_id = run_id.trim();
    let call_seq = call_seq.trim().parse::<u32>().ok().filter(|seq| *seq > 0);
    let (true, Some(call_seq)) = (valid_trace_run_id(run_id), call_seq) else {
        return Err(run_trace_error(
            StatusCode::BAD_REQUEST,
            "invalid_run_request",
            "run_id must be 1 to 64 characters of [A-Za-z0-9_-] and call_seq a number from 1",
        ));
    };
    let response = state
        .run_ledger_client
        .clone()
        .get_request_snapshot(authenticated_session_request(
            mp_contracts::model_plane::v1::GetRequestSnapshotRequest {
                run_id: run_id.to_owned(),
                call_seq,
            },
            &bearer,
        )?)
        .await
        .map_err(|status| run_request_status_to_http(&status))?
        .into_inner();
    let sections: Vec<Value> = response
        .system_sections
        .iter()
        .map(|section| {
            json!({
                "label": section.label,
                "source": section.source,
                "digest": section.digest,
                "chars": section.chars,
                "text": (!section.text.is_empty()).then_some(section.text.as_str()),
            })
        })
        .collect();
    Ok(Json(json!({
        "data": {
            "run_id": run_id,
            "call_seq": call_seq,
            "call_purpose": response.call_purpose,
            "snapshot": stored_json(&response.snapshot_json),
            "system_sections": sections,
            "tools": (!response.tools_json.is_empty()).then(|| stored_json(&response.tools_json)).flatten(),
            "tools_truncated": response.tools_truncated,
            "system_truncated": response.system_truncated,
            "dropped": response.dropped,
        },
    })))
}

/// `type_url` of Session Core's `RUN_CANCELLED` row. Session Core has no enum
/// value for that row, so it replays as `EVENT_TYPE_UNSPECIFIED` and only the
/// payload discriminator identifies it (as in `durable_thread_event_type`).
const RUN_CANCELLED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunCancelled";

/// The terminal status an event records for its run, if it is a terminal event.
fn run_terminal_status(event_type: &str, event: &Event) -> Option<&'static str> {
    let type_url = event
        .payload
        .as_ref()
        .map(|payload| payload.type_url.as_str())
        .unwrap_or_default();
    if type_url == RUN_CANCELLED_TYPE_URL || event_type == "EVENT_TYPE_RUN_CANCELLED" {
        return Some("cancelled");
    }
    match event_type {
        "EVENT_TYPE_RUN_COMPLETED" => Some("completed"),
        "EVENT_TYPE_RUN_FAILED" => Some("failed"),
        _ => None,
    }
}

/// A durable failure code fit to leave this boundary: `^[a-z][a-z0-9_]{0,63}$`.
fn is_failure_code_slug(code: &str) -> bool {
    let mut bytes = code.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && code.len() <= 64
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Project one replayed thread event for `run_id`, or `None` when it does not
/// belong in the run's replay.
///
/// - Browser events: only the browser event types persisted by Session Core,
///   and only when the payload names the requested run.
/// - The run's terminal event (`RUN_COMPLETED`, `RUN_FAILED`, `RUN_CANCELLED`):
///   only when the envelope names the run (`resource_ref` `run:{id}` or the
///   `correlation_id`; the managed `RUN_FAILED` payload carries no run id).
///   The raw payload never leaves: it can hold `error` text, a cancellation
///   `reason` and `cancelled_by`. The projection is
///   `{run_id, terminal: true, status, failure_code?}`, with `failure_code`
///   only when it is a slug.
fn project_run_replay_event(run_id: &str, event: Event) -> Option<DurableRunEventProjection> {
    let event_type = EventType::try_from(event.event_type)
        .map(|kind| kind.as_str_name().to_owned())
        .unwrap_or_else(|_| "EVENT_TYPE_UNSPECIFIED".to_owned());
    if let Some(status) = run_terminal_status(&event_type, &event) {
        if event.resource_ref != format!("run:{run_id}") && event.correlation_id != run_id {
            return None;
        }
        let failure_code = event
            .payload
            .as_ref()
            .and_then(|value| serde_json::from_slice::<Value>(&value.value).ok())
            .and_then(|payload| {
                payload
                    .get("failure_code")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|code| is_failure_code_slug(code));
        let mut payload = json!({
            "run_id": run_id,
            "terminal": true,
            "status": status,
        });
        if let Some(code) = failure_code {
            payload["failure_code"] = Value::String(code);
        }
        let event_type = match status {
            "completed" => "EVENT_TYPE_RUN_COMPLETED",
            "failed" => "EVENT_TYPE_RUN_FAILED",
            _ => "EVENT_TYPE_RUN_CANCELLED",
        };
        return Some(DurableRunEventProjection {
            at: event.ts.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
            event_id: event.event_id,
            event_type: event_type.to_owned(),
            payload,
        });
    }
    if !matches!(
        event_type.as_str(),
        "EVENT_TYPE_BROWSER_ACTION_DISPATCHED"
            | "EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED"
            | "EVENT_TYPE_BROWSER_RUN_PAUSED"
            | "EVENT_TYPE_BROWSER_RUN_RESUMED"
            // WS8.G: ids only (run, plan, attach, session run, tab), never a URL or a title.
            | "EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR"
            | "EVENT_TYPE_BROWSER_ACTION_APPROVAL_REQUIRED"
            | "EVENT_TYPE_BROWSER_ACTION_DECIDED"
            | "EVENT_TYPE_APPROVAL_CONTINUATION_VERIFIED"
    ) {
        return None;
    }
    let payload = event
        .payload
        .as_ref()
        .and_then(|value| serde_json::from_slice::<Value>(&value.value).ok())?;
    if payload.get("run_id").and_then(Value::as_str) != Some(run_id) {
        return None;
    }
    Some(DurableRunEventProjection {
        at: event.ts.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        event_id: event.event_id,
        event_type,
        payload,
    })
}

// ============================================================================
// Orchestration mutation handlers
// ============================================================================

#[derive(Debug, Deserialize)]
struct TransitionPlanBody {
    target_state: String,
    #[serde(default)]
    reason: String,
}

async fn approve_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(plan_id): Path<String>,
    Json(body): Json<TransitionPlanBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: approving a plan advances stored work. A ZDR credential is refused here, before
    // anything runs; a waiver is checked against the plan's run receipt in Session Core.
    crate::zdr_posture::continuation_gate(&claims)?;
    let target = parse_plan_state(&body.target_state)?;
    let resp = state
        .orchestration_client
        .clone()
        .transition_plan(authenticated_session_request(
            TransitionPlanRequest {
                plan_id,
                target_state: target as i32,
                actor: claims.user_id.clone(),
                reason: body.reason,
                edited_steps: Vec::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let plan = resp.plan.ok_or_else(|| not_found("plan not found"))?;
    Ok(Json(json!({ "plan": plan_value(&plan) })))
}

async fn reject_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(plan_id): Path<String>,
    Json(body): Json<TransitionPlanBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: rejecting a plan advances stored work; see approve_plan.
    crate::zdr_posture::continuation_gate(&claims)?;
    let resp = state
        .orchestration_client
        .clone()
        .transition_plan(authenticated_session_request(
            TransitionPlanRequest {
                plan_id,
                target_state: PlanState::Rejected as i32,
                actor: claims.user_id.clone(),
                reason: body.reason,
                edited_steps: Vec::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let plan = resp.plan.ok_or_else(|| not_found("plan not found"))?;
    Ok(Json(json!({ "plan": plan_value(&plan) })))
}

#[derive(Debug, Deserialize)]
struct TransitionTodoBody {
    status: String,
    #[serde(default)]
    reason: String,
}

async fn update_todo_status(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(todo_id): Path<String>,
    Json(body): Json<TransitionTodoBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: a todo update advances stored work; see approve_plan.
    crate::zdr_posture::continuation_gate(&claims)?;
    let target = parse_todo_state(&body.status)?;
    let resp = state
        .orchestration_client
        .clone()
        .transition_todo(authenticated_session_request(
            TransitionTodoRequest {
                todo_id,
                target_state: target as i32,
                actor: claims.user_id.clone(),
                reason: body.reason,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let todo = resp.todo.ok_or_else(|| not_found("todo not found"))?;
    Ok(Json(json!({ "todo": todo_value(&todo) })))
}

#[derive(Debug, Deserialize)]
struct DecideApprovalBody {
    decision: String, // "approve" | "reject"
    #[serde(default)]
    reason: String,
    /// How far the decision reaches (build plan WS13 S2). Empty means `once`. Standing
    /// scopes need the grant store (S6), so any other value is refused until it exists.
    #[serde(default)]
    scope: String,
    /// "Always" (build plan WS13 S7): only the literal `true` asks for a standing rule proposed
    /// from this grant. Any other value is ignored, as an unknown key always was, so a body that
    /// carries one reads exactly as it did before the switch existed.
    #[serde(default)]
    promote: Value,
    /// "Send with my edits" (EP-10-S4): the person's changes to a pending mail send, granted with
    /// the approval in one act. Refused while APPROVAL_EDIT_ENABLED is off; absent, the decision is
    /// exactly what it was before edits existed.
    #[serde(default)]
    edit: Option<DecideEditBody>,
}

/// The edit a decision may carry (EP-10-S4). `patch` holds only `to`, `subject` and `bodyText`;
/// Session Core refuses any other key, so this gateway forwards it as the person wrote it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideEditBody {
    patch: Value,
    base_fingerprint: String,
}

/// The approve-with-edits switch (EP-10-S4). Off: a decision carrying an edit is refused with 403
/// `approval_edit_disabled` before anything reaches Session Core. Only the literal `true` enables
/// it.
const APPROVAL_EDIT_ENV: &str = "APPROVAL_EDIT_ENABLED";

fn approval_edit_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(env::var(APPROVAL_EDIT_ENV).ok().as_deref())
}

/// The largest patch an edit may carry: the 48 KiB body cap plus room for recipients and subject.
const MAX_EDIT_PATCH_BYTES: usize = 64 * 1024;

fn edit_refusal(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

/// Check a decision's edit before any upstream call (EP-10-S4, decision D1): the switch, a grant,
/// the `once` scope with no "Always", a JSON object patch and a 64-hex base fingerprint. `None`
/// when the body carries no edit, whatever the switch says.
fn checked_decision_edit(
    edit: Option<&DecideEditBody>,
    body_scope: &str,
    promote: bool,
    target: ApprovalState,
    enabled: bool,
) -> Result<Option<mp_contracts::model_plane::v1::ApprovalEdit>, HttpJsonError> {
    let Some(edit) = edit else {
        return Ok(None);
    };
    if !enabled {
        return Err(edit_refusal(
            StatusCode::FORBIDDEN,
            "approval_edit_disabled",
            "Editing before approving is not enabled on this deployment.",
        ));
    }
    if !matches!(target, ApprovalState::Granted) {
        return Err(edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "edit_requires_grant",
            "An edit is sent with an approval only.",
        ));
    }
    if !matches!(body_scope, "" | SCOPE_ONCE) || promote {
        return Err(edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "edit_requires_once",
            "An edited action is approved for this one time only.",
        ));
    }
    if !edit.patch.is_object() {
        return Err(edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_patch",
            "The edit is not in the expected shape.",
        ));
    }
    let base = edit.base_fingerprint.trim();
    if base.len() != 64 || !base.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_base_fingerprint",
            "The edit does not say which draft it changes.",
        ));
    }
    let patch_json = edit.patch.to_string();
    if patch_json.len() > MAX_EDIT_PATCH_BYTES {
        return Err(edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "edit_too_large",
            "The edited message is too long.",
        ));
    }
    Ok(Some(mp_contracts::model_plane::v1::ApprovalEdit {
        patch_json,
        base_fingerprint: base.to_owned(),
    }))
}

/// The HTTP form of a decision that carried an edit (EP-10-S4). Session Core sends the closed code
/// as the status message: a changed approval is 409, a refused patch 422, a switch or owner refusal
/// 403. A typed ZDR refusal and every other status keep the shared mapping.
fn edit_decision_status_to_http(error: &tonic::Status) -> HttpJsonError {
    if mp_contracts::zdr_refusal::Refusal::from_status(error).is_some() {
        return grpc_status_to_http(error);
    }
    let code = error.message();
    let contract_code = mp_contracts::approval_edit::EditError::ALL
        .iter()
        .any(|known| known.code() == code);
    match (error.code(), code) {
        (tonic::Code::Aborted, "approval_changed") => edit_refusal(
            StatusCode::CONFLICT,
            "approval_changed",
            "This approval changed since you opened it. Review it again.",
        ),
        (tonic::Code::FailedPrecondition, "revision_limit") => edit_refusal(
            StatusCode::CONFLICT,
            "revision_limit",
            "This action was edited too many times.",
        ),
        (tonic::Code::FailedPrecondition, "not_editable") => edit_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_editable",
            "This action cannot be edited; approve or reject it as drafted.",
        ),
        (tonic::Code::InvalidArgument, code)
            if contract_code
                || matches!(code, "invalid_patch" | "edit_requires_once" | "edit_requires_grant") =>
        {
            edit_refusal(
                StatusCode::UNPROCESSABLE_ENTITY,
                code,
                "The edit was not accepted.",
            )
        }
        (tonic::Code::FailedPrecondition, "approval_edit_disabled")
        | (tonic::Code::PermissionDenied, "edit_owner_only") => edit_refusal(
            StatusCode::FORBIDDEN,
            code,
            "Editing this approval is not available.",
        ),
        _ => grpc_status_to_http(error),
    }
}

/// The scope a decision may carry (build plan WS13 S6). `once` is always available. A standing
/// scope needs the switch, a granting decision, and an approval whose cause offers that scope: the
/// server says what is offered, and a client never picks a scope it was not offered.
fn checked_decision_scope(
    requested: &str,
    target: ApprovalState,
    offered: &[String],
) -> Result<&'static str, HttpJsonError> {
    checked_decision_scope_with(
        requested,
        target,
        offered,
        approval_cause::scope_grants_enabled(),
    )
}

fn checked_decision_scope_with(
    requested: &str,
    target: ApprovalState,
    offered: &[String],
    grants_enabled: bool,
) -> Result<&'static str, HttpJsonError> {
    let scope = match requested {
        "" | SCOPE_ONCE => return Ok(SCOPE_ONCE),
        SCOPE_THREAD => SCOPE_THREAD,
        SCOPE_RUN => SCOPE_RUN,
        _ => {
            return Err(scope_refusal(
                StatusCode::UNPROCESSABLE_ENTITY,
                "scope_not_offered",
                "That scope is not offered for this approval.",
            ))
        }
    };
    if !matches!(target, ApprovalState::Granted) {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "scope_not_applicable",
            "A scope applies to a grant only.",
        ));
    }
    if !grants_enabled {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "scope_not_enabled",
            "Standing scopes are not enabled on this deployment.",
        ));
    }
    if !offered.iter().any(|candidate| candidate == scope) {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "scope_not_offered",
            "That scope is not offered for this approval.",
        ));
    }
    Ok(scope)
}

fn scope_refusal(status: StatusCode, code: &'static str, message: &'static str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

/// The proto scope a checked decision scope name stands for.
fn approval_scope_code(scope: &str) -> i32 {
    match scope {
        SCOPE_THREAD => ApprovalScope::Thread as i32,
        SCOPE_RUN => ApprovalScope::Run as i32,
        _ => ApprovalScope::Once as i32,
    }
}

/// The environment switch for "Always" as a reviewed rule (build plan WS13 S7). Default off; only
/// the literal `true` enables it, read through the one strict parser the other switches share.
const RULE_PROPOSALS_ENV: &str = "APPROVAL_RULE_PROPOSALS_ENABLED";

fn rule_proposals_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

fn rule_proposals_enabled() -> bool {
    rule_proposals_from(env::var(RULE_PROPOSALS_ENV).ok().as_deref())
}

/// Whether a decision is "Always" (build plan WS13 S7): a standing rule proposed from this grant,
/// for the person to confirm in settings. Returns `true` only for a valid ask under the switch; with
/// the switch off `promote` is ignored. A rule is proposed from a grant of this thread on an MCP
/// tool's approval and from nothing else, so every other ask is refused with its typed reason.
fn checked_rule_proposal(
    promote: bool,
    scope: &str,
    target: ApprovalState,
    cause: Option<&ApprovalCause>,
    enabled: bool,
) -> Result<bool, HttpJsonError> {
    if !enabled || !promote {
        return Ok(false);
    }
    if !matches!(target, ApprovalState::Granted) {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "rule_not_applicable",
            "A rule is proposed from a grant only.",
        ));
    }
    if scope != SCOPE_THREAD {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "rule_requires_thread_scope",
            "Always applies to a decision for this thread.",
        ));
    }
    if !cause.is_some_and(approval_cause::rule_offered) {
        return Err(scope_refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "rule_not_offered",
            "Always is offered only for an MCP tool that you may relax.",
        ));
    }
    Ok(true)
}

/// The approval step-up switch (build plan WS13 S5). `off` changes nothing; `shadow` counts a grant
/// that lacks a fresh re-authentication and lets it through; `enforce` refuses it. Only the literal
/// values `shadow` and `enforce` turn anything on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalStepUpGate {
    Off,
    Shadow,
    Enforce,
}

impl ApprovalStepUpGate {
    const ENV: &'static str = "APPROVAL_STEP_UP_ENFORCED";

    fn from_env() -> Self {
        Self::from_raw(std::env::var(Self::ENV).ok().as_deref())
    }

    fn from_raw(raw: Option<&str>) -> Self {
        match raw {
            Some("shadow") => Self::Shadow,
            Some("enforce") => Self::Enforce,
            _ => Self::Off,
        }
    }
}

/// A step-up claim is fresh while it is at most [`STEP_UP_MAX_AGE_SECS`] old. A claim dated a
/// little ahead of this clock is tolerated for [`STEP_UP_CLOCK_SKEW_SECS`] (build plan WS13 S5).
const STEP_UP_MAX_AGE_SECS: i64 = 120;
const STEP_UP_CLOCK_SKEW_SECS: i64 = 30;

fn step_up_is_fresh(claim: Option<&StepUpClaim>, now: i64) -> bool {
    claim.is_some_and(|claim| {
        let age = now - claim.at;
        (-STEP_UP_CLOCK_SKEW_SECS..=STEP_UP_MAX_AGE_SECS).contains(&age)
    })
}

/// Whether a decision is a grant its approval's cause makes conditional on a fresh
/// re-authentication, and the caller brought none. A denial never needs one: refusing a call is
/// always within the caller's authority.
fn step_up_missing(
    target_state: ApprovalState,
    prior: Option<&Approval>,
    claim: Option<&StepUpClaim>,
    now: i64,
) -> bool {
    matches!(target_state, ApprovalState::Granted)
        && prior
            .and_then(|approval| approval.cause.as_ref())
            .is_some_and(|cause| cause.step_up_required)
        && !step_up_is_fresh(claim, now)
}

/// Whether a client is asked for a password confirmation before granting this cause. Only while
/// the gate enforces it: under `off` and `shadow` the grant goes through as it always did, so no
/// client prompts for a confirmation the gateway would not check (build plan WS13 S5).
fn step_up_projected(cause: &ApprovalCause, gate: ApprovalStepUpGate) -> bool {
    cause.step_up_required && gate == ApprovalStepUpGate::Enforce
}

/// The typed refusal of a grant that lacks a fresh re-authentication. The method names the proof
/// the client should ask for.
fn step_up_required_error() -> HttpJsonError {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": {
            "code": "step_up_required",
            "message": "Confirm your password before approving this.",
            "details": { "method": "password" },
        } })),
    )
}

/// The durable decision this gateway records for a decision (build plan WS13 S3). The source is
/// always USER: the body has no field for it, so a client-supplied value cannot reach the record.
/// `rule_proposed` is set only for an "Always" this gateway accepted (build plan WS13 S7).
fn user_decision_request(
    approval_id: String,
    target_state: ApprovalState,
    user_id: &str,
    org_id: &str,
    reason: String,
    scope: &str,
    rule_proposed: bool,
) -> DecideApprovalRequest {
    DecideApprovalRequest {
        approval_id,
        decision: target_state as i32,
        decided_by: user_id.to_owned(),
        decision_reason: reason,
        // Cross-org IDOR fix (Phase 6): scope to the caller's verified
        // JWT org, never a client-suppliable value.
        org_id: org_id.to_owned(),
        decision_source: ApprovalDecisionSource::User as i32,
        scope: approval_scope_code(scope),
        rule_proposed,
        ..Default::default()
    }
}

async fn decide_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: VerifiedModelBearer,
    step_up: Option<Extension<VerifiedStepUp>>,
    Path(approval_id): Path<String>,
    Json(body): Json<DecideApprovalBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: deciding an approval advances the run it belongs to. See approve_plan.
    crate::zdr_posture::continuation_gate(&claims)?;
    let target_state: ApprovalState = match body.decision.as_str() {
        "approve" => ApprovalState::Granted,
        "reject" => ApprovalState::Denied,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown decision: {other}") })),
            ))
        }
    };
    // Approve with edits (EP-10-S4): refused before any upstream call unless it is a `once` grant
    // under the switch. A body without `edit` takes exactly the path it always did.
    let edit = checked_decision_edit(
        body.edit.as_ref(),
        &body.scope,
        body.promote == Value::Bool(true),
        target_state,
        approval_edit_enabled(),
    )?;
    let prior_approval = if matches!(target_state, ApprovalState::Granted) {
        state
            .orchestration_client
            .clone()
            .get_approval(authenticated_session_request(
                GetApprovalRequest {
                    approval_id: approval_id.clone(),
                    org_id: claims.org_id.clone(),
                    ..Default::default()
                },
                &session_bearer,
            )?)
            .await
            .map_err(|error| grpc_status_to_http(&error))?
            .into_inner()
            .approval
    } else {
        None
    };
    // The scope is checked against what this approval's cause offers, once the approval is known.
    let offered = prior_approval
        .as_ref()
        .and_then(|approval| approval.cause.as_ref())
        .map(|cause| cause.scope_options.clone())
        .unwrap_or_default();
    let scope = checked_decision_scope(&body.scope, target_state, &offered)?;
    let asks_rule = checked_rule_proposal(
        body.promote == Value::Bool(true),
        scope,
        target_state,
        prior_approval
            .as_ref()
            .and_then(|approval| approval.cause.as_ref()),
        rule_proposals_enabled(),
    )?;
    let step_up_claim = step_up.and_then(|Extension(VerifiedStepUp(claim))| claim);
    if step_up_missing(
        target_state,
        prior_approval.as_ref(),
        step_up_claim.as_ref(),
        Utc::now().timestamp(),
    ) {
        match ApprovalStepUpGate::from_env() {
            ApprovalStepUpGate::Off => {}
            ApprovalStepUpGate::Shadow => {
                let cause = prior_approval
                    .as_ref()
                    .and_then(|approval| approval.cause.as_ref())
                    .map_or("", |cause| CauseCode::from_wire(&cause.code).as_str());
                metrics::counter!("approval_step_up_missing_total", "cause" => cause).increment(1);
                info!(approval_id = %approval_id, "a grant lacks a fresh step-up (shadow)");
            }
            ApprovalStepUpGate::Enforce => {
                info!(approval_id = %approval_id, "a grant lacks a fresh step-up; refused");
                return Err(step_up_required_error());
            }
        }
    }
    let edited = edit.is_some();
    let mut decision = user_decision_request(
        approval_id,
        target_state,
        &claims.user_id,
        &claims.org_id,
        body.reason,
        scope,
        asks_rule,
    );
    decision.edit = edit;
    let resp = state
        .orchestration_client
        .clone()
        .decide_approval(authenticated_session_request(decision, &session_bearer)?)
        .await
        .map_err(|e| {
            if edited {
                edit_decision_status_to_http(&e)
            } else {
                grpc_status_to_http(&e)
            }
        })?
        .into_inner();
    let approval = resp
        .approval
        .ok_or_else(|| not_found("approval not found"))?;
    // A grant is durable authority, but not a restartable continuation: this
    // path never invokes a generic `ResumeRun`. What happens next is read from
    // the approval's derived `continuation` (Session Core joins the delivery
    // outbox, start receipt and outcome), never assumed here: the old
    // always-failing quarantine call answered "pending" for kinds the worker
    // would refuse (build plan WS10 S1). The decision itself was already
    // durably recorded above, so this is informational and never a 502.
    let mut continuation_delivery: Option<&'static str> = None;
    if matches!(target_state, ApprovalState::Granted)
        && should_resume_granted_approval(prior_approval.as_ref(), &approval)?
    {
        continuation_delivery = Some(continuation_delivery_word(&approval));
    }

    let mut body = json!({ "approval": approval_value(&approval) });
    if let Some(status) = continuation_delivery {
        body["continuation_delivery"] = json!(status);
    }
    Ok(Json(body))
}

/// The one-word `continuation_delivery` of a decided approval, kept for older clients next to the
/// typed `approval.continuation`: `queued`, `running`, `completed`, `failed`, `not_resumable`
/// (the approval only lets the agent retry the exact action once), `live` (a browser action
/// polled by the running loop), `awaiting_continue` (a loop-owned approval) or `unknown` when
/// Session Core could not derive the state.
fn continuation_delivery_word(approval: &Approval) -> &'static str {
    use mp_contracts::model_plane::v1::ContinuationState as State;
    match continuation_state_name(approval) {
        Some(State::Queued) => "queued",
        Some(State::Running) => "running",
        Some(State::Completed) => "completed",
        Some(State::Failed) => "failed",
        Some(State::NotResumable) => "not_resumable",
        Some(State::Live) => "live",
        Some(State::AwaitingContinue) => "awaiting_continue",
        Some(State::Unspecified) | None => "unknown",
    }
}

fn continuation_state_name(
    approval: &Approval,
) -> Option<mp_contracts::model_plane::v1::ContinuationState> {
    approval
        .continuation
        .as_ref()
        .and_then(|continuation| {
            mp_contracts::model_plane::v1::ContinuationState::try_from(continuation.state).ok()
        })
}

/// The resume class Session Core stored on the approval's card (`context.card.resume_class`),
/// restricted to the closed vocabulary.
fn card_resume_class(approval: &Approval) -> Option<String> {
    use prost_types::value::Kind;
    let context = approval.context.as_ref()?;
    let Some(Kind::StructValue(card)) = context.fields.get("card")?.kind.as_ref() else {
        return None;
    };
    let Some(Kind::StringValue(class)) = card.fields.get("resume_class")?.kind.as_ref() else {
        return None;
    };
    matches!(class.as_str(), "action_continuation" | "live_poll" | "none").then(|| class.clone())
}

/// The typed continuation a browser may read; `null` until the approval is granted.
fn continuation_value(approval: &Approval) -> Value {
    use mp_contracts::model_plane::v1::{ContinuationState as State, VerificationStatus};
    let Some(continuation) = approval.continuation.as_ref() else {
        return Value::Null;
    };
    let state = match continuation_state_name(approval) {
        Some(State::Queued) => "queued",
        Some(State::Running) => "running",
        Some(State::Completed) => "completed",
        Some(State::Failed) => "failed",
        Some(State::NotResumable) => "not_resumable",
        Some(State::Live) => "live",
        Some(State::AwaitingContinue) => "awaiting_continue",
        Some(State::Unspecified) | None => return Value::Null,
    };
    let verification = match VerificationStatus::try_from(continuation.verification) {
        Ok(VerificationStatus::VerifiedSuccess) => json!("verified_success"),
        Ok(VerificationStatus::VerifiedFailure) => json!("verified_failure"),
        Ok(VerificationStatus::PartiallyVerified) => json!("partially_verified"),
        Ok(VerificationStatus::Unknown) => json!("unknown"),
        _ => Value::Null,
    };
    json!({
        "state": state,
        "failure_code": empty_to_null(&continuation.failure_code),
        "verification": verification,
        "provider_receipt_id": empty_to_null(&continuation.provider_receipt_id),
    })
}

/// Cancel a run through Session Core's authoritative `RunService`.
///
/// The `RunService` transaction flips the status and returns the durable
/// `RUN_CANCELLED` event id as the receipt; that is the source of truth. The
/// gateway also still publishes the legacy `RUN_CANCEL_REQUESTED` notification
/// on the run subject, but nothing in the repository acts on it today: the
/// `mp.v1.orchestration.>` subscribers in session-core and orchestrator-core
/// receive it and do not decode it as an orchestration event.
///
/// A successful cancel also stops this caller's chat stream for the run when
/// it is live in this process (`CancelRegistry::cancel_run_for`), so the
/// stream ends as `stopped` instead of saving onto the cancelled run.
///
/// Every call writes one info line with the run id, the receipt id when there
/// is one, and the outcome (`zdr_rejected`, `owner_rejected`, `cancelled`,
/// `already_terminal`, or `error` with the gRPC code). Ids and codes only. The
/// browser drops this route's errors, so without the line a Stop that never
/// reached the durable cancel leaves no trace.
async fn cancel_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: a cancel is a change to a stored run. See approve_plan.
    if let Err(rejection) = crate::zdr_posture::continuation_gate(&claims) {
        info!(run_id = %run_id, outcome = "zdr_rejected", "run cancel");
        return Err(rejection);
    }
    if let Err(error) = session_flow::require_durable_run_owner_with_token(
        &state,
        &run_id,
        &claims.org_id,
        &claims.user_id,
        session_bearer.as_str(),
    )
    .await
    {
        if error.code() == tonic::Code::PermissionDenied {
            info!(run_id = %run_id, outcome = "owner_rejected", "run cancel");
        } else {
            info!(run_id = %run_id, outcome = "error", code = ?error.code(), stage = "owner", "run cancel");
        }
        return Err(grpc_status_to_http(&error));
    }

    let request = authenticated_session_request(
        CancelRunRequest {
            run_id: run_id.clone(),
            reason: "user_requested".to_owned(),
        },
        &session_bearer,
    )
    .inspect_err(|_| {
        info!(run_id = %run_id, outcome = "error", stage = "credential", "run cancel");
    })?;
    let cancellation = match state.run_client.clone().cancel_run(request).await {
        Ok(response) => response.into_inner(),
        Err(error) => {
            info!(run_id = %run_id, outcome = "error", code = ?error.code(), stage = "cancel", "run cancel");
            return Err(grpc_status_to_http(&error));
        }
    };

    // A stream still producing this run must stop as well. The durable cancel
    // alone left it to save its answer onto the cancelled run and then fail to
    // terminalize it, with a replay buffer that never finished. Owner-scoped
    // like the stream's own cancel route. A stream on another replica is asked
    // over the run-control bus when it is on; the run is cancelled whatever the
    // bus says, so an unreachable owner is logged and never an error here (its
    // liveness check before the save is the backstop).
    let route = if cancellation.cancelled {
        Some(
            state
                .run_control
                .cancel_run(&state.cancels, &run_id, &claims.org_id, &claims.user_id)
                .await,
        )
    } else {
        None
    };
    let live_stream = matches!(route, Some(CancelRoute::Local | CancelRoute::Peer));
    let via = match route {
        Some(CancelRoute::Local) => "local",
        Some(CancelRoute::Peer) => "peer",
        Some(CancelRoute::Unavailable) => "unavailable",
        Some(CancelRoute::NoOwner) | None => "none",
    };

    // Keep the legacy notification on the run subject (nothing acts on it
    // today, see above), but never turn a successful durable cancellation into
    // a 500 merely because an optional fan-out is unavailable.
    if cancellation.cancelled {
        let envelope = mp_events::envelope::Envelope {
            event_id: new_ulid(),
            event_type: "RUN_CANCEL_REQUESTED".to_owned(),
            schema_version: 1,
            ts: Utc::now(),
            producer: "model-gateway".to_owned(),
            correlation_id: run_id.clone(),
            causation_id: cancellation.receipt_id.clone(),
            idempotency_key: format!("cancel_{run_id}"),
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
            resource_ref: format!("run/{run_id}"),
            payload: serde_json::json!({
                "run_id": run_id,
                "receipt_id": cancellation.receipt_id,
            }),
            // The guard above answered every ZDR posture with a refusal, so a
            // run-control event that reaches this line is persistent.
            zdr: false,
            ..Default::default()
        };
        if let Err(error) = state
            .publisher
            .publish(mp_events::subjects::SUBJECT_RUN, &envelope)
            .await
        {
            warn!(%error, run_id = %run_id, "durable cancellation recorded but notification fan-out failed");
        }
    }

    let status = if cancellation.cancelled {
        "cancelled"
    } else {
        "already_terminal"
    };
    info!(run_id = %run_id, receipt_id = %cancellation.receipt_id, outcome = status, live_stream, via, "run cancel");
    return Ok(Json(json!({
        "run_id": run_id,
        "status": status,
        "cancelled": cancellation.cancelled,
        "receipt_id": cancellation.receipt_id,
    })));
}

/// Resume a cancelled/paused run — flips execution-core's run state back to
/// Running and records a `RUN_RESUME_REQUESTED` event for any downstream
/// consumers. The direct execution acknowledgement is required so callers are
/// never told a run resumed while it remains gated.
async fn resume_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    execution_bearer: VerifiedExecutionBearer,
    session_bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: resuming a stored run is a continuation. See approve_plan.
    crate::zdr_posture::continuation_gate(&claims)?;
    require_durable_run_owner(&state, &claims, &run_id, &session_bearer).await?;
    // Direct, immediate unblock only for a user-paused run. Approval-gated
    // work has no bare-resume path and remains in the descriptor quarantine.
    let resume = state
        .execution_client
        .clone()
        .resume_run(authenticated_execution_request(
            ResumeRunRequest {
                run_id: run_id.clone(),
                checkpoint_id: String::new(),
                org_id: claims.org_id.clone(),
                approval_id: String::new(),
                steer_text: String::new(),
            },
            &execution_bearer,
            &session_bearer,
        )?)
        .await
        .map_err(|error| grpc_status_to_http(&error))?
        .into_inner();
    let resume = require_execution_resume_ack(resume)?;

    let envelope = mp_events::envelope::Envelope {
        event_id: new_ulid(),
        event_type: "RUN_RESUME_REQUESTED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: run_id.clone(),
        causation_id: String::new(),
        idempotency_key: format!("resume_{run_id}"),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("run/{run_id}"),
        payload: serde_json::json!({ "run_id": run_id }),
        // The guard above answered every ZDR posture with a refusal, so a
        // run-control event that reaches this line is persistent.
        zdr: false,
        ..Default::default()
    };
    state
        .publisher
        .publish(mp_events::subjects::SUBJECT_RUN, &envelope)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(
        json!({ "run_id": run_id, "status": "resumed", "resumed": resume.resumed }),
    ))
}

fn parse_plan_state(s: &str) -> Result<PlanState, HttpJsonError> {
    match s {
        "DRAFT" | "draft" => Ok(PlanState::Draft),
        "PROPOSED" | "proposed" => Ok(PlanState::Proposed),
        "APPROVED" | "approved" => Ok(PlanState::Approved),
        "EXECUTING" | "executing" => Ok(PlanState::Executing),
        "COMPLETED" | "completed" => Ok(PlanState::Completed),
        "FAILED" | "failed" => Ok(PlanState::Failed),
        "REJECTED" | "rejected" => Ok(PlanState::Rejected),
        "SUPERSEDED" | "superseded" => Ok(PlanState::Superseded),
        "ARCHIVED" | "archived" => Ok(PlanState::Archived),
        other => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown plan state: {other}") })),
        )),
    }
}

fn parse_todo_state(s: &str) -> Result<TodoState, HttpJsonError> {
    match s {
        "PENDING" | "pending" => Ok(TodoState::Pending),
        "IN_PROGRESS" | "in_progress" => Ok(TodoState::InProgress),
        "BLOCKED" | "blocked" => Ok(TodoState::Blocked),
        "COMPLETED" | "completed" => Ok(TodoState::Completed),
        "CANCELLED" | "cancelled" => Ok(TodoState::Cancelled),
        other => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown todo state: {other}") })),
        )),
    }
}

// ============================================================================
// AI modality stub handlers  /v1/ai/*
// All forward to inference-core via the existing gRPC client.
// ============================================================================

#[derive(Debug, Deserialize)]
struct AiChatRequest {
    messages: Vec<serde_json::Value>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider: String,
    #[serde(default, alias = "subscriptionConnectionId")]
    subscription_connection_id: String,
    #[serde(default)]
    #[allow(dead_code)] // accepted on the wire but not yet acted upon
    stream: bool,
    #[serde(default)]
    structured_output_schema: Option<String>,
    #[serde(default)]
    zdr: bool,
    /// Sampling temperature, 0.0 to 1.0 (WS16.S1). Absent keeps the 0.7 every caller had before
    /// this field existed. Read as a plain number and range-checked by hand so a value out of
    /// range answers 400 with a reason, not a deserialiser 422.
    #[serde(default)]
    temperature: Option<f64>,
    /// Output token ceiling, 1 to 8192. Absent keeps the 4096 every caller had before.
    #[serde(default)]
    max_tokens: Option<i64>,
    /// Why a machine caller makes this call. A closed set (`quarry.extract`), and only the
    /// `service:quarry-edge` principal may name it; it rides on the usage envelope so cost-core
    /// can attribute the spend.
    #[serde(default)]
    purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiEmbeddingRequest {
    input: String,
    model: String,
    provider: Option<String>,
    #[serde(default)]
    zdr: bool,
}

#[derive(Debug, Deserialize)]
struct AiModelsQuery {
    modality: Option<String>,
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiImagesRequest {
    operation: Option<String>,
    prompt: Option<String>,
    input: Option<String>,
    image_url: Option<String>,
    url: Option<String>,
    content_base64: Option<String>,
    image_base64: Option<String>,
    mime_type: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    size: Option<String>,
    quality: Option<String>,
    n: Option<u32>,
    max_tokens: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
struct BrowserSuggestActionRequest {
    #[serde(default)]
    goal: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    observation: Value,
    #[serde(default)]
    visual_observation: Value,
    #[serde(default)]
    visual_observation_artifact_id: Option<String>,
    #[serde(default)]
    screenshot_base64: Option<String>,
    #[serde(default)]
    screenshot_mime_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiSpeechRequest {
    operation: Option<String>,
    input: Option<String>,
    text: Option<String>,
    audio_base64: Option<String>,
    voice: Option<String>,
    format: Option<String>,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    /// Zero Data Retention for this call (Temporary chat). Combined with the
    /// token's own claim and sent on to inference-core, which refuses ZDR
    /// speech until a speech deployment is attested.
    #[serde(default)]
    zdr: bool,
}

#[derive(Debug, Deserialize)]
struct AiSpeechVoicesQuery {
    provider: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiTranslateItem {
    id: Option<String>,
    text: String,
}

#[derive(Debug, Deserialize)]
struct AiTranslateRequest {
    operation: Option<String>,
    text: Option<String>,
    items: Option<Vec<AiTranslateItem>>,
    source_language: Option<String>,
    target_language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiTranslateLanguagesQuery {
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiDocumentIntelRequest {
    url: Option<String>,
    document_url: Option<String>,
    content_base64: Option<String>,
    document_base64: Option<String>,
    content_type: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    pages: Option<String>,
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiLanguageRequest {
    operation: Option<String>,
    text: Option<String>,
    texts: Option<Vec<String>>,
    language: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    sentence_count: Option<u32>,
    kind: Option<String>,
    summary_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiRealtimeSessionRequest {
    model: Option<String>,
    provider: Option<String>,
    voice: Option<String>,
    instructions: Option<String>,
    input_audio_format: Option<String>,
    output_audio_format: Option<String>,
    turn_detection_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiVideoGenerateRequest {
    prompt: String,
    width: Option<u32>,
    height: Option<u32>,
    duration_seconds: Option<u32>,
    n_variants: Option<u32>,
    model: Option<String>,
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiVideoJobQuery {
    provider: Option<String>,
    model: Option<String>,
}

const BROWSER_SCREENSHOT_MAX_BYTES: usize = 6 * 1024 * 1024;
const BROWSER_OBSERVATION_PROMPT_CHARS: usize = 16_000;
const BROWSER_VISUAL_PROMPT_CHARS: usize = 8_000;

const BROWSER_ACTION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["navigate", "click", "type", "press", "scroll", "select", "wait", "wait_for", "screenshot", "done"]
    },
    "url": { "type": "string" },
    "selector": { "type": "string" },
    "text": { "type": "string" },
    "value": { "type": "string" },
    "key": { "type": "string" },
    "target": { "type": "string" },
    "ms": { "type": "integer" },
    "timeout_ms": { "type": "integer" },
    "full_page": { "type": "boolean" },
    "reason": { "type": "string" },
    "confidence": { "type": "number" }
  },
  "required": ["action", "reason", "confidence"]
}"#;

const BROWSER_SUGGEST_SYSTEM_PROMPT: &str = concat!(
    "You are the Model Plane browser planner for Verevon. Quarry-v2 captures browser evidence; ",
    "you only decide the next browser action. Return exactly one JSON object matching the schema. ",
    "Do not request raw JavaScript evaluation, anti-bot bypass, credential entry, CAPTCHA solving, ",
    "or actions outside the current user goal. Prefer low-risk actions that reveal useful page evidence. ",
    "Use action=\"done\" when the page is already ready for evidence capture or no safe action is needed. ",
    "Allowed action meanings: navigate uses url; click/type/select/wait_for use selector; type uses text; ",
    "press uses key; scroll uses target; wait uses ms; screenshot uses full_page."
);

/// What a browser planner does with a bot wall (Quarry WS3.J). execution-core's
/// loop planner carries the same text as its own `BROWSER_CHALLENGE_RULE`; the
/// two are separate services, so
/// `execution-core/tests/browser_challenge_rule_contract.rs` keeps them
/// identical by reading both sources. Edit them together. The rule is one line,
/// so the text is the same whatever the file's line endings.
const BROWSER_CHALLENGE_RULE: &str = r"Bot-wall rule. An observation may carry a challenge line of typed fields (kind, strength, block_reason, recovery, confidence_per_mille). Only strength=strong says the site refuses automation. Never solve a captcha, never change identity (another profile, account, proxy or user agent), and never look for a way around the wall. When recovery=available you may take ONE recovery step: a single wait of up to 5 seconds, or a single reload of the same page. When recovery is exhausted or not_applicable, or the signal is still strong after that one step, stop: report the task as blocked (action=done, with a reason that says the site refuses automation) and ask the user to take over the browser or to use another source. A login page, a consent banner, a paywall, a rate limit and a bare 403 are not bot blocks, carry no strong signal, and this rule does not apply to them.";

/// The planner's system prompt: its fixed instructions, then the bot-wall rule.
fn browser_suggest_system_prompt() -> String {
    format!("{BROWSER_SUGGEST_SYSTEM_PROMPT} {BROWSER_CHALLENGE_RULE}")
}

async fn browser_suggest_action(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<BrowserSuggestActionRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

    let request_id = new_ulid();
    // A waived token names one connection, so it is refused here as a route mismatch (see `zdr_posture`).
    crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, false),
    )?;
    let visual_summary =
        browser_visual_summary(&state, &claims, &req, &request_id, &inference_bearer).await?;
    let evidence = browser_suggestion_prompt(&req, visual_summary.as_deref());

    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                purpose: InferPurpose::BrowserPlanner as i32,
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: req.model.clone().unwrap_or_default(),
                provider_hint: req.provider.clone().unwrap_or_default(),
                messages: vec![
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "system".to_owned(),
                        content: browser_suggest_system_prompt(),
                        name: String::new(),
                    },
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "user".to_owned(),
                        content: evidence,
                        name: String::new(),
                    },
                ],
                temperature: 0.2,
                max_tokens: 700,
                structured_output_schema: BROWSER_ACTION_SCHEMA.to_owned(),
                zdr: true,
                ..Default::default()
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let parsed: Value = serde_json::from_str(resp.content.trim()).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("browser planner returned invalid JSON: {e}")
            })),
        )
    })?;
    let suggestion = browser_suggestion_value(&parsed);

    Ok(Json(json!({
        "id": request_id,
        "object": "browser.action_suggestion",
        "suggestion": suggestion,
        "visual_summary": visual_summary,
        "model_used": resp.model_used,
        "usage": {
            "input_tokens": resp.input_tokens,
            "output_tokens": resp.output_tokens
        }
    })))
}

async fn browser_visual_summary(
    state: &AppState,
    claims: &Claims,
    req: &BrowserSuggestActionRequest,
    request_id: &str,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Option<String>, HttpJsonError> {
    // The screenshot is analysed only as a persistent turn: AnalyzeImageRequest carries no ZDR flag,
    // so a ZDR or waived posture skips the visual summary rather than send the image to a platform
    // provider. The suggestion itself still runs (see `zdr_posture`).
    match crate::zdr_posture::decide_for(claims, &crate::zdr_posture::Route::named(req.provider.as_deref(), None, false)) {
        Ok(posture) if !posture.zdr() => {}
        _ => return Ok(None),
    }
    let Some(raw_base64) = req
        .screenshot_base64
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let encoded = raw_base64
        .split_once(',')
        .map_or(raw_base64, |(_, payload)| payload)
        .trim();
    let image_data = STANDARD.decode(encoded).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "screenshot_base64 must be valid base64" })),
        )
    })?;
    if image_data.len() > BROWSER_SCREENSHOT_MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "screenshot_base64 is too large" })),
        ));
    }

    let prompt = concat!(
        "Describe the visible browser page for action planning. Focus on clickable controls, forms, ",
        "visible blockers, selected/active state, and whether the page is ready for evidence capture. ",
        "Do not infer hidden data or solve CAPTCHAs."
    );
    match state
        .inference_client
        .clone()
        .analyze_image(authenticated_inference_request(
            AnalyzeImageRequest {
                request_id: format!("{request_id}-vision"),
                org_id: claims.org_id.clone(),
                image_url: String::new(),
                image_data,
                mime_type: req
                    .screenshot_mime_type
                    .clone()
                    .unwrap_or_else(|| "image/png".to_owned()),
                prompt: prompt.to_owned(),
                model: req.model.clone().unwrap_or_default(),
                provider_hint: req.provider.clone().unwrap_or_default(),
                max_tokens: 700,
            },
            inference_bearer,
        ))
        .await
    {
        Ok(resp) => Ok(Some(resp.into_inner().description)),
        Err(error) => {
            warn!(error = %error, "browser screenshot vision analysis failed; planning from structured evidence only");
            Ok(None)
        }
    }
}

fn browser_suggestion_prompt(
    req: &BrowserSuggestActionRequest,
    visual_summary: Option<&str>,
) -> String {
    let goal = truncate_for_prompt(
        if req.goal.trim().is_empty() {
            "Decide whether one more browser action is needed before evidence capture."
        } else {
            req.goal.trim()
        },
        1_200,
    );
    // WS12.S4: the page is untrusted and may hold a credential the user typed or the page
    // echoed. Credential-shaped strings are masked HERE, on the way into the prompt, before
    // any cut (`compact_json_for_prompt`, `truncate_for_prompt`) can split one into a
    // fragment the pattern no longer matches. The request itself is not changed.
    let observation = compact_json_for_prompt(
        &scrubbed_live_json(&req.observation),
        BROWSER_OBSERVATION_PROMPT_CHARS,
    );
    let visual_observation = compact_json_for_prompt(
        &scrubbed_live_json(&req.visual_observation),
        BROWSER_VISUAL_PROMPT_CHARS,
    );
    let visual_summary = visual_summary.map_or_else(
        || "not provided".to_owned(),
        |summary| {
            truncate_for_prompt(&mp_scrub::scrub_live(summary), BROWSER_VISUAL_PROMPT_CHARS)
        },
    );
    let visual_artifact = req
        .visual_observation_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("not provided");

    format!(
        "Goal:\n{goal}\n\nCurrent URL: {}\nCurrent title: {}\n\nQuarry observation JSON:\n{observation}\n\nOpenCV visual observation artifact id: {visual_artifact}\nOpenCV visual observation JSON:\n{visual_observation}\n\nVLM screenshot summary:\n{visual_summary}\n\nReturn the single next browser action JSON only.",
        truncate_for_prompt(&mp_scrub::scrub_url(&req.url), 1_000),
        truncate_for_prompt(&mp_scrub::scrub_live(&req.title), 500),
    )
}

/// A copy of `value` with every credential-shaped string masked (`mp_scrub::scrub_live_json`).
fn scrubbed_live_json(value: &Value) -> Value {
    let mut copy = value.clone();
    mp_scrub::scrub_live_json(&mut copy);
    copy
}

fn browser_suggestion_value(parsed: &Value) -> Value {
    let action_kind = parsed
        .get("action")
        .and_then(Value::as_str)
        .map_or("done", str::trim);
    let action = browser_action_value(action_kind, parsed);
    json!({
        "done": action_kind == "done",
        "action": action,
        "reason": string_value(parsed, "reason", 500),
        "confidence": confidence_value(parsed),
    })
}

fn browser_action_value(action_kind: &str, parsed: &Value) -> Value {
    match action_kind {
        "navigate" => {
            let url = string_value(parsed, "url", 2_000);
            if url.is_empty() {
                Value::Null
            } else {
                json!({ "type": "navigate", "url": url })
            }
        }
        "click" => selector_action(parsed, "click"),
        "type" => {
            let selector = string_value(parsed, "selector", 1_000);
            let text = string_value(parsed, "text", 4_000);
            if selector.is_empty() || text.is_empty() {
                Value::Null
            } else {
                json!({ "type": "type", "selector": selector, "text": text })
            }
        }
        "press" => {
            let key = string_value(parsed, "key", 64);
            json!({ "type": "press", "key": if key.is_empty() { "Enter".to_owned() } else { key } })
        }
        "scroll" => {
            let target = string_value(parsed, "target", 1_000);
            json!({ "type": "scroll", "target": if target.is_empty() { "viewport".to_owned() } else { target } })
        }
        "select" => {
            let selector = string_value(parsed, "selector", 1_000);
            let value = string_value(parsed, "value", 1_000);
            if selector.is_empty() || value.is_empty() {
                Value::Null
            } else {
                json!({ "type": "select", "selector": selector, "value": value })
            }
        }
        "wait" => json!({ "type": "wait", "ms": integer_value(parsed, "ms", 250, 10_000, 1_000) }),
        "wait_for" => {
            let selector = string_value(parsed, "selector", 1_000);
            if selector.is_empty() {
                Value::Null
            } else {
                json!({
                    "type": "wait_for",
                    "selector": selector,
                    "timeout_ms": integer_value(parsed, "timeout_ms", 250, 15_000, 5_000)
                })
            }
        }
        "screenshot" => json!({
            "type": "screenshot",
            "full_page": parsed.get("full_page").and_then(Value::as_bool).unwrap_or(false)
        }),
        _ => Value::Null,
    }
}

fn selector_action(parsed: &Value, action_type: &str) -> Value {
    let selector = string_value(parsed, "selector", 1_000);
    if selector.is_empty() {
        Value::Null
    } else {
        json!({ "type": action_type, "selector": selector })
    }
}

fn string_value(parsed: &Value, key: &str, max_chars: usize) -> String {
    parsed
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_for_prompt(value, max_chars))
        .unwrap_or_default()
}

fn integer_value(parsed: &Value, key: &str, min: i64, max: i64, fallback: i64) -> i64 {
    parsed
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(fallback)
        .clamp(min, max)
}

fn confidence_value(parsed: &Value) -> f64 {
    parsed
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn compact_json_for_prompt(value: &Value, max_chars: usize) -> String {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned());
    truncate_for_prompt(&serialized, max_chars)
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut out = value.chars().take(max_chars).collect::<String>();
    out.push_str("\n...[truncated]");
    out
}

/// `AiChatRequest::temperature` when the caller sends none.
const AI_CHAT_DEFAULT_TEMPERATURE: f32 = 0.7;
/// `AiChatRequest::max_tokens` when the caller sends none.
const AI_CHAT_DEFAULT_MAX_TOKENS: i32 = 4096;
/// The ceiling a caller may ask for.
const AI_CHAT_MAX_TOKENS_CEILING: i64 = 8192;
/// The one `purpose` value, and the one principal allowed to name it.
const AI_CHAT_PURPOSE_QUARRY_EXTRACT: &str = "quarry.extract";
const AI_CHAT_PURPOSE_CALLER: &str = "service:quarry-edge";

/// `MODEL_GATEWAY_AI_CHAT_METERING`: whose `/v1/ai/chat` calls publish a `USAGE_ENVELOPE`.
///
/// The build plan's default is `service`; the owner rule is that new behaviour starts off, so the
/// default here is `off` (assumed decision, recorded in the progress file). Rolling it out is
/// `MODEL_GATEWAY_AI_CHAT_METERING=service`. `all` also meters user callers, which changes user
/// accounting and is therefore not a default of anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AiChatMetering {
    #[default]
    Off,
    Service,
    All,
}

impl AiChatMetering {
    /// Anything other than the literal `service` or `all` is `off`.
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
            Some("service") => Self::Service,
            Some("all") => Self::All,
            _ => Self::Off,
        }
    }

    const fn meters(self, service_caller: bool) -> bool {
        match self {
            Self::Off => false,
            Self::Service => service_caller,
            Self::All => true,
        }
    }
}

/// The switch, read once per process.
fn ai_chat_metering() -> AiChatMetering {
    static MODE: std::sync::OnceLock<AiChatMetering> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        AiChatMetering::parse(env::var("MODEL_GATEWAY_AI_CHAT_METERING").ok().as_deref())
    })
}

/// The validated sampling parameters of one `/v1/ai/chat` call.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AiChatParams {
    temperature: f32,
    max_tokens: i32,
    purpose: Option<&'static str>,
}

fn ai_chat_bad_request(error: &str) -> HttpJsonError {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error })))
}

/// Validate the additive fields. Absent fields keep today's values exactly.
fn ai_chat_params(req: &AiChatRequest, claims: &Claims) -> Result<AiChatParams, HttpJsonError> {
    let temperature = match req.temperature {
        None => AI_CHAT_DEFAULT_TEMPERATURE,
        Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => {
            // The range check above makes the narrowing safe for a sampling knob.
            #[allow(clippy::cast_possible_truncation)]
            let narrowed = value as f32;
            narrowed
        }
        Some(_) => return Err(ai_chat_bad_request("temperature must be between 0.0 and 1.0")),
    };
    let max_tokens = match req.max_tokens {
        None => AI_CHAT_DEFAULT_MAX_TOKENS,
        Some(value) if (1..=AI_CHAT_MAX_TOKENS_CEILING).contains(&value) => {
            i32::try_from(value).unwrap_or(AI_CHAT_DEFAULT_MAX_TOKENS)
        }
        Some(_) => return Err(ai_chat_bad_request("max_tokens must be between 1 and 8192")),
    };
    let purpose = match req.purpose.as_deref() {
        None => None,
        Some(purpose) => {
            // Service callers only, and only the one named service. A user token that sends a
            // purpose is refused rather than ignored, so the field can never read as accepted.
            if claims.service_principal_id().is_none() || claims.sub != AI_CHAT_PURPOSE_CALLER {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "purpose is not permitted for this caller" })),
                ));
            }
            if purpose != AI_CHAT_PURPOSE_QUARRY_EXTRACT {
                return Err(ai_chat_bad_request("unknown purpose"));
            }
            Some(AI_CHAT_PURPOSE_QUARRY_EXTRACT)
        }
    };
    Ok(AiChatParams {
        temperature,
        max_tokens,
        purpose,
    })
}

/// The `USAGE_ENVELOPE` of one metered `/v1/ai/chat` call: the shape `grpc.rs` and `invoke`
/// publish, plus `route` and `purpose` so cost-core can tell this lane from chat.
fn ai_chat_usage_envelope(
    request_id: &str,
    claims: &Claims,
    infer: &mp_contracts::model_plane::v1::InferResponse,
    latency_ms: u64,
    purpose: Option<&str>,
    cost_basis: crate::pricing::CostBasis,
) -> Envelope {
    // A service is attributed by its subject, a user by their id.
    let user_id = if claims.service_principal_id().is_some() {
        claims.sub.clone()
    } else {
        claims.user_id.clone()
    };
    Envelope {
        event_id: new_ulid(),
        event_type: "USAGE_ENVELOPE".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: format!("{request_id}-USAGE_ENVELOPE"),
        org_id: claims.org_id.clone(),
        user_id: user_id.clone(),
        resource_ref: format!("request/{request_id}"),
        payload: json!({
            "request_id": request_id,
            "org_id": claims.org_id,
            "user_id": user_id,
            "model": infer.model_used,
            "input_tokens": infer.input_tokens,
            "output_tokens": infer.output_tokens,
            "latency_ms": latency_ms,
            "transport": "http",
            "route": "ai_chat",
            "purpose": purpose,
            "provider_used": infer.provider_used,
            "residency": infer.residency,
            "cache_read_input_tokens": infer.cache_read_input_tokens,
            "cache_creation_input_tokens": infer.cache_creation_input_tokens,
            "cost_basis": cost_basis.as_str(),
        }),
        zdr: false,
        ..Default::default()
    }
}

async fn ai_chat(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiChatRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    ai_chat_with(state, claims, inference_bearer, req, ai_chat_metering()).await
}

async fn ai_chat_with(
    state: AppState,
    claims: Claims,
    inference_bearer: VerifiedInferenceBearer,
    req: AiChatRequest,
    metering: AiChatMetering,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
    // Bad sampling fields and a purpose the caller may not name are refused before anything else.
    let params = ai_chat_params(&req, &claims)?;
    // The posture of this turn, through the resolver: a ZDR turn that names the caller's own
    // connection is refused here, typed, before any model call, and a waiver is honoured only on
    // the connection it names (see `zdr_posture`). Without this, a connection id with no provider
    // hint reached inference-core as "any provider", and the ZDR chain served the turn in place of
    // the connection the caller selected.
    let route = crate::zdr_posture::Route::named(
        Some(req.provider.as_str()),
        Some(req.subscription_connection_id.as_str()),
        req.zdr,
    );
    let zdr = crate::zdr_posture::zdr_or_refuse(&claims, &route)?;
    let messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .map(|m| ChatMessage {
            compaction_summary: String::new(),
            role: m["role"].as_str().unwrap_or("user").to_owned(),
            content: m["content"].as_str().unwrap_or("").to_owned(),
            name: String::new(),
        })
        .collect();
    let request_id = new_ulid();
    let (provider_hint, connection_id) =
        (req.provider.clone(), req.subscription_connection_id.clone());
    let started = std::time::Instant::now();
    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                purpose: InferPurpose::Answer as i32,
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: req.model,
                provider_hint: req.provider,
                subscription_connection_id: req.subscription_connection_id,
                messages,
                temperature: params.temperature,
                max_tokens: params.max_tokens,
                structured_output_schema: req.structured_output_schema.unwrap_or_default(),
                zdr,
                ..Default::default()
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    // A subscription call is never priced from API list prices.
    let cost_basis = crate::pricing::CostBasis::of_route(&provider_hint, &connection_id)
        .served_by(&resp.provider_used);
    // Under ZDR nothing about the call is priced, metered or ledgered: the cost is null.
    let cost_usd = if zdr {
        None
    } else {
        state
            .pricing
            .cost_usd_for(
                cost_basis,
                &resp.model_used,
                i64::from(resp.input_tokens.max(0)),
                i64::from(resp.output_tokens.max(0)),
                i64::from(resp.cache_read_input_tokens.max(0)),
                i64::from(resp.cache_creation_input_tokens.max(0)),
            )
            .await
    };
    if !zdr && metering.meters(claims.service_principal_id().is_some()) {
        let envelope = ai_chat_usage_envelope(
            &request_id,
            &claims,
            &resp,
            latency_ms,
            params.purpose,
            cost_basis,
        );
        if let Err(error) = state
            .publisher
            .publish(&subjects::usage_subject(&claims.org_id), &envelope)
            .await
        {
            warn!(%error, "failed to publish ai_chat USAGE_ENVELOPE");
        }
    }
    Ok(Json(json!({
        "id": request_id,
        "content": resp.content,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "usage": {
            "input_tokens": resp.input_tokens,
            "output_tokens": resp.output_tokens,
            "cost_usd": cost_usd,
            "latency_ms": latency_ms,
        },
    })))
}

/// Versioned system prompt for the onboarding plan recommender. Bumping the
/// version string changes the inference-core prompt-cache key.
const RECOMMEND_PLAN_MODEL_VERSION: &str = "recommend-plan-v9";

const RECOMMEND_PLAN_SYSTEM_PROMPT: &str = concat!(
    "Verevon product context: Verevon is both the product name and the AI worker at the center of the product. ",
    "The software exists to configure, feed, govern, deploy, and measure Verevon for each company. ",
    "Customers are not merely installing a helpdesk with an AI add-on; they are giving Verevon the company's ",
    "website, knowledge, integrations, rules, and goals so Verevon can become their source-grounded support worker. ",
    "Verevon is an AI-native competitor to Intercom, Chatbase, Gorgias, Mimir, and Zendesk: it combines ",
    "customer chat, support inbox, knowledge base, integrations, retrieval, graph context, workflow routing, ",
    "automation, analytics, and AI agent capabilities in one workspace. ",
    "It ingests a customer's public website and connected work systems into a Data Plane with documents, ",
    "knowledge units, vector retrieval, and graph entities/relationships. The Model Plane uses that data for ",
    "GraphRAG-style answers, scope analysis, routing suggestions, and knowledge-gap discovery. ",
    "Verevon can power a customer-facing chatbot, shared/team inbox workflows, source-grounded answers, ",
    "handoff/routing rules, automation ideas, SLA/reporting views, and dashboard insights about missing answers ",
    "or next sources to connect. Verevon improves support by reducing repeated manual answers, making responses ",
    "consistent across website and internal sources, surfacing gaps before launch, and suggesting the first ",
    "automations a team should validate. ",
    "Do not overpromise exact savings, guaranteed resolution rates, autonomous changes in third-party systems, ",
    "or private model training unless the input explicitly supports it. Treat expected outcomes as directional ",
    "launch estimates. ",
    "Recommendation task: you are Verevon's senior onboarding consultant writing a live AI recommendation ",
    "for a customer who just connected their website and tools. Recommend exactly one Verevon plan. ",
    "Plans (id -> name and terms): ",
    "trial -> Free, 0 NOK/month, 14-day trial, no card, upgrade later; ",
    "hobby -> Essential, 299 NOK/month, 4 NOK per AI-resolved inquiry, chatbot + shared inbox, website and knowledge sources, small-team/simple chatbot validation; ",
    "standard -> Advanced, 999 NOK/month, 3.50 NOK per AI-resolved inquiry, automation and routing, multiple team inboxes, 20 Lite seats, multiple sources/inboxes; ",
    "pro -> Expert, 1499 NOK/month, 2.90 NOK per AI-resolved inquiry, SSO and identity controls, SLA/reporting/multibrand, 50 Lite seats, larger support teams; ",
    "enterprise -> Custom, volume pricing per AI answer, custom terms, extended onboarding, dedicated success team, governance/volume. ",
    "Heuristics: more employees, more connected sources, and intent signals like ",
    "automation/SLA/SSO/governance push toward higher tiers; little or no signal -> trial. ",
    "Write like a thoughtful product specialist, not a pricing template. ",
    "Use the actual organization name, employee count if provided, website host, and connected systems. ",
    "Counts are authoritative: context.connectedSourceCount/context.sourceSummary.connectedSourceCount is the number of ",
    "connected source streams to call 'tilkoblede kilder'; context.sourceCount includes those connected streams plus ",
    "the website as one source. Never invent a smaller source count or reuse an older count. ",
    "If context.websiteContent is present, it holds real title+excerpt snippets Verevon just crawled from the ",
    "customer's site; read them to state concretely what the company does, sells, or serves, and reference that ",
    "in the reason/summary so the recommendation is visibly grounded in their own site — never invent facts not ",
    "present in those snippets. If context.industry is present, use it to frame the company's sector. ",
    "If context.dataPlane is present, use its graph counts, groups, sample nodes and sample edges as evidence; ",
    "do not invent document contents that are not in the JSON. ",
    "Paraphrase the user's goal and correct obvious spelling/grammar mistakes; never quote raw user input. ",
    "Explain why this plan fits now, what Verevon already appears to understand, and what the customer can expect ",
    "in the first launch window. Expected outcomes must be rough directional estimates, not guarantees. ",
    "Avoid generic phrases such as 'select this plan', 'static FAQ', or 'you can change later'. ",
    "Be terse: prefer the fewest words that stay grounded and specific; no filler. ",
    "Reply with ONLY a JSON object: {\"planId\": one of trial|hobby|standard|pro|enterprise, ",
    "\"reason\": ONE short sentence addressed to the user, \"summary\": exactly ONE tight sentence, ",
    "\"proofPoints\": at most 3 short bullets (max ~8 words each, not full sentences), ",
    "\"scopeSignals\": at most 3 short bullets (max ~8 words each), ",
    "\"opportunities\": at most 2 short bullets (max ~8 words each), ",
    "\"expectedOutcomes\": 2-3 objects with {label,value,detail}, ",
    "\"confidence\": number 0..1}. Write all user-facing text in the requested locale ",
    "(nb = natural Norwegian Bokmål, en = English)."
);

const RECOMMEND_PLAN_SCHEMA: &str = concat!(
    "{\"type\":\"object\",\"properties\":{",
    "\"planId\":{\"type\":\"string\",\"enum\":[\"trial\",\"hobby\",\"standard\",\"pro\",\"enterprise\"]},",
    "\"reason\":{\"type\":\"string\"},\"summary\":{\"type\":\"string\"},",
    "\"proofPoints\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}},",
    "\"scopeSignals\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}},",
    "\"opportunities\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}},",
    "\"expectedOutcomes\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{",
    "\"label\":{\"type\":\"string\"},\"value\":{\"type\":\"string\"},\"detail\":{\"type\":\"string\"}},",
    "\"required\":[\"label\",\"value\"]}},",
    "\"confidence\":{\"type\":\"number\"}},",
    "\"required\":[\"planId\",\"reason\",\"summary\",\"proofPoints\",\"scopeSignals\",\"opportunities\",\"expectedOutcomes\"]}"
);

#[derive(Debug, Deserialize)]
struct RecommendPlanRequest {
    /// Opaque onboarding-context object assembled by the frontend BFF.
    #[serde(default)]
    context: Value,
    #[serde(default)]
    locale: String,
}

fn requested_recommendation_zdr(context: &Value) -> bool {
    context
        .get("zeroDataRetention")
        .and_then(Value::as_bool)
        .or_else(|| context.get("zdr").and_then(Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod recommendation_retention_tests {
    use super::*;

    #[test]
    fn recommendation_defaults_to_normal_retention_when_toggle_is_absent() {
        assert!(
            !requested_recommendation_zdr(&json!({})),
            "missing toggle must not force ZDR"
        );
    }

    #[test]
    fn recommendation_uses_the_explicit_toggle() {
        assert!(!requested_recommendation_zdr(
            &json!({ "zeroDataRetention": false })
        ));
        assert!(requested_recommendation_zdr(
            &json!({ "zeroDataRetention": true })
        ));
    }

    #[test]
    fn recommendation_accepts_legacy_zdr_alias() {
        assert!(requested_recommendation_zdr(&json!({ "zdr": true })));
    }
}

/// Recommend an onboarding plan from accumulated onboarding signals. Wraps
/// inference-core with a versioned prompt + JSON-schema structured output and
/// runs with the retention posture selected during onboarding. The frontend
/// renders an instant local recommendation first and swaps in this authoritative
/// result; it also falls back to its local engine if this endpoint is
/// unavailable, so this handler favors always returning a valid plan id.
async fn recommend_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<RecommendPlanRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

    let locale = if req.locale == "en" { "en" } else { "nb" };
    let zdr = crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::plain(requested_recommendation_zdr(&req.context)),
    )?;
    let context_json = serde_json::to_string(&req.context).unwrap_or_else(|_| "{}".to_owned());
    let messages = vec![
        ChatMessage {
            compaction_summary: String::new(),
            role: "system".to_owned(),
            content: RECOMMEND_PLAN_SYSTEM_PROMPT.to_owned(),
            name: String::new(),
        },
        ChatMessage {
            compaction_summary: String::new(),
            role: "user".to_owned(),
            content: format!("Locale: {locale}\nOnboarding signals (JSON):\n{context_json}"),
            name: String::new(),
        },
    ];

    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                purpose: InferPurpose::Answer as i32,
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: String::new(),
                provider_hint: String::new(),
                messages,
                temperature: 0.55,
                max_tokens: 1100,
                structured_output_schema: RECOMMEND_PLAN_SCHEMA.to_owned(),
                zdr,
                ..Default::default()
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let parsed: Value = serde_json::from_str(resp.content.trim()).unwrap_or(Value::Null);
    let plan_id = parsed
        .get("planId")
        .and_then(Value::as_str)
        .filter(|p| matches!(*p, "trial" | "hobby" | "standard" | "pro" | "enterprise"))
        .unwrap_or("trial");
    let reason = parsed
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let summary = parsed
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let proof_points = string_array_field(&parsed, "proofPoints", 3);
    let scope_signals = string_array_field(&parsed, "scopeSignals", 3);
    let opportunities = string_array_field(&parsed, "opportunities", 2);
    let expected_outcomes = expected_outcomes_field(&parsed, 3);
    let confidence = parsed.get("confidence").and_then(Value::as_f64);

    Ok(Json(json!({
        "recommendation": {
            "planId": plan_id,
            "reason": reason,
            "summary": summary,
            "proofPoints": proof_points,
            "scopeSignals": scope_signals,
            "opportunities": opportunities,
            "expectedOutcomes": expected_outcomes,
            "confidence": confidence,
            "modelVersion": RECOMMEND_PLAN_MODEL_VERSION,
            "model_used": resp.model_used,
            "request_id": request_id,
        }
    })))
}

fn string_array_field(parsed: &Value, key: &str, limit: usize) -> Vec<String> {
    parsed
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .take(limit)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn expected_outcomes_field(parsed: &Value, limit: usize) -> Vec<Value> {
    parsed
        .get("expectedOutcomes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let label = item.get("label").and_then(Value::as_str)?.trim();
                    let value = item.get("value").and_then(Value::as_str)?.trim();
                    if label.is_empty() || value.is_empty() {
                        return None;
                    }
                    let detail = item
                        .get("detail")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty());
                    Some(json!({
                        "label": label,
                        "value": value,
                        "detail": detail,
                    }))
                })
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

/// Embedding generation via inference-core. Data Plane owns vector storage and
/// retrieval; this route only exposes the provider-facing primitive.
async fn ai_embeddings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiEmbeddingRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let zdr = crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, req.zdr),
    )?;
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_embedding(authenticated_inference_request(
            CreateEmbeddingRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                text: req.input,
                model: req.model,
                provider_hint: req.provider.unwrap_or_default(),
                zdr,
                // No region preference expressed at this primitive — inference-core
                // uses its configured EU deployment (deny-by-default for non-EU).
                region: String::new(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "embedding",
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "embedding": resp.vector,
    })))
}

fn default_mcp_transport() -> String {
    "http".to_owned()
}
const fn default_true() -> bool {
    true
}

/// Body for `POST /v1/mcp/servers`. Mirrors the gRPC `RegisterMcpServer` RPC so
/// an MCP server (hence its tools) can be added with a plain POST — the
/// ergonomic "add a new tool" entry point. `org_id` is taken from the caller's
/// session, never the body.
fn default_mcp_scope() -> String {
    "user".to_owned()
}

#[derive(Debug, Deserialize)]
struct McpRegisterBody {
    name: String,
    /// Public HTTPS MCP bridge base URL. User-supplied process transports are
    /// quarantined in the secure MVP.
    url: String,
    #[serde(default = "default_mcp_transport")]
    transport: String,
    #[serde(default)]
    token: String,
    /// Exact tool names the server may expose. Empty fails closed.
    #[serde(default)]
    tool_allowlist: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    /// Optional explicit id; omit to let the gateway assign a ULID.
    #[serde(default)]
    server_id: String,
    /// "user" (private to the creator, default) or "org" (admin-only, visible to
    /// all org members). A non-admin can never create an "org" server.
    #[serde(default = "default_mcp_scope")]
    scope: String,
}

#[derive(Debug, Deserialize)]
struct McpShareBody {
    /// Full replacement set of user ids the server is shared with.
    #[serde(default)]
    user_ids: Vec<String>,
}

/// Admin authority comes only from signed token scopes. Forwarded role headers
/// are context hints and cannot grant access.
fn req_is_admin(claims: &Claims, headers: &HeaderMap) -> bool {
    let _ = headers;
    crate::ownership::is_admin_claim(&claims.scopes, None)
}

/// Project an [`McpServer`] + its ownership for an API response, deliberately
/// omitting `token` (an operational secret — it never leaves the gateway). A
/// server without an ownership record is quarantined by list/use filters; the
/// projection's empty owner is diagnostic only.
fn mcp_server_json(s: &McpServer, ownership: Option<&crate::ownership::Ownership>) -> Value {
    let (scope, owner, shared) = ownership.map_or_else(
        || ("org", String::new(), Vec::new()),
        |o| {
            (
                o.scope.as_wire(),
                o.owner_user_id.clone(),
                o.shared_with.clone(),
            )
        },
    );
    json!({
        "server_id": s.server_id,
        "name": s.name,
        "url": s.url,
        "transport": s.transport,
        "tool_allowlist": s.tool_allowlist,
        "enabled": s.enabled,
        "scope": scope,
        "owner_user_id": owner,
        "shared_with": shared,
    })
}

/// `POST /v1/mcp/servers` — register (or upsert) an MCP server for the caller.
/// Default scope is user-private; `scope:"org"` requires an admin scope in the
/// verified token (the BFF also checks for clearer UX). Records ownership and
/// writes through to capability-core atomically from the caller's perspective:
/// a catalog failure rolls back the provisional gateway entry.
async fn mcp_register(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    headers: HeaderMap,
    Json(body): Json<McpRegisterBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // The runtime registry is durable gateway state as well as a Capability
    // Core record, so reject ZDR before either registry is touched.
    require_non_zdr_durable_mutation(&claims)?;
    if body.name.trim().is_empty() || body.url.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name and url are required" })),
        ));
    }
    if !body.server_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "server_id is assigned by the gateway on create" })),
        ));
    }
    register_plain_mcp_server(
        &state,
        &claims,
        &headers,
        &capability_bearer,
        body.name,
        body.url,
        body.transport,
        body.token,
        body.tool_allowlist,
        body.enabled,
        body.scope,
    )
    .await
}

/// Resolve the effective tool allowlist for a new MCP server: if the caller
/// specified one explicitly, honor it verbatim (still the way to
/// deliberately narrow scope). Otherwise auto-discover the server's
/// `tools/list` and allow everything it reports — the rest of this connect
/// flow already auto-detects transport and auth, so the caller shouldn't
/// have to separately enumerate tool names by hand either. Falls back to
/// the (empty) input unchanged if discovery fails, so the existing
/// "tool_allowlist must contain 1 to 64 exact tool names" validation still
/// surfaces a clear error rather than silently registering a toolless
/// server.
async fn resolve_tool_allowlist(url: &str, token: &str, requested: Vec<String>) -> Vec<String> {
    if !requested.is_empty() {
        return requested;
    }
    match crate::runtime_registries::http_list_tools(url, token).await {
        Ok(tools) if !tools.is_empty() => tools.into_iter().map(|tool| tool.name).collect(),
        Ok(_) => {
            debug!(%url, "mcp auto-discovery: server reported zero tools");
            requested
        }
        Err(error) => {
            debug!(%url, %error, "mcp auto-discovery: tools/list failed, leaving allowlist as given");
            requested
        }
    }
}

/// Register a plain (non-OAuth) MCP server — static token or none — and
/// write through to capability-core. Shared by `mcp_register` (direct,
/// explicit static-token registration) and `mcp_connect` (the
/// auto-detecting entry point, once discovery finds no OAuth metadata).
///
/// `enabled` is honored rather than assumed: a disabled server keeps its
/// registry row but is filtered out of the agent-facing tool definitions and
/// refused at call time (see `runtime_registries`), so registering with
/// `enabled: false` has to actually land as disabled.
#[allow(clippy::too_many_arguments)]
async fn register_plain_mcp_server(
    state: &AppState,
    claims: &Claims,
    headers: &HeaderMap,
    capability_bearer: &VerifiedCapabilityBearer,
    name: String,
    url: String,
    transport: String,
    token: String,
    tool_allowlist: Vec<String>,
    enabled: bool,
    scope: String,
) -> Result<Json<Value>, HttpJsonError> {
    let org_id = claims.org_id.clone();
    let scope = crate::ownership::Scope::from_wire(&scope);
    if scope == crate::ownership::Scope::Org && !req_is_admin(claims, headers) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "only an admin can create an org-wide MCP server" })),
        ));
    }
    let ownership = match scope {
        crate::ownership::Scope::Org => crate::ownership::Ownership::org(),
        crate::ownership::Scope::User => crate::ownership::Ownership::user(claims.user_id.clone()),
    };
    let tool_allowlist = if transport == "http" {
        resolve_tool_allowlist(&url, &token, tool_allowlist).await
    } else {
        tool_allowlist
    };
    let server = McpServer {
        server_id: String::new(),
        name,
        url,
        transport,
        token,
        tool_allowlist,
        enabled,
    };
    let resp = crate::runtime_registries::handle_register_mcp_server(
        &state.mcp,
        RegisterMcpServerRequest {
            request_id: new_ulid(),
            org_id: org_id.clone(),
            server: Some(server),
        },
    )
    .map_err(|e| grpc_status_to_http(&e))?;

    // Record ownership for the assigned server id (the source of truth for the
    // exposure + visibility filters).
    if let Some(server) = resp.server.as_ref() {
        state.ownership.set(
            &org_id,
            crate::ownership::KIND_MCP,
            &server.server_id,
            ownership.clone(),
        );
    }

    // capability-core is the registry system of record. A create is not
    // successful until the authenticated catalog write succeeds; on failure we
    // remove the provisional gateway cache entry so the two catalogs cannot
    // silently diverge.
    let Some(registered) = resp.server.as_ref() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "MCP registration returned no server"})),
        ));
    };
    if state.capability_core_base_url.is_empty() {
        state.mcp.remove(&org_id, &registered.server_id);
        state
            .ownership
            .remove(&org_id, crate::ownership::KIND_MCP, &registered.server_id);
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "capability registry is unavailable"})),
        ));
    }
    let auth_kind = if registered.token.is_empty() {
        "none"
    } else {
        "bearer"
    };
    let payload = crate::runtime_registries::mcp_capability_payload(
        &org_id, registered, &ownership, auth_kind,
    );
    let url = format!("{}/api/v1/mcp", state.capability_core_base_url);
    let catalog_result = state
        .http_client
        .post(url)
        .bearer_auth(capability_bearer.as_str())
        .json(&payload)
        .send()
        .await;
    let catalog_ok = catalog_result
        .as_ref()
        .is_ok_and(|response| response.status().is_success());
    if !catalog_ok {
        state.mcp.remove(&org_id, &registered.server_id);
        state
            .ownership
            .remove(&org_id, crate::ownership::KIND_MCP, &registered.server_id);
        match catalog_result {
            Ok(response) => warn!(status = %response.status(), "MCP catalog registration rejected"),
            Err(error) => warn!(error = %error, "MCP catalog registration unavailable"),
        }
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "capability registry rejected MCP registration"})),
        ));
    }

    let owned = resp
        .server
        .as_ref()
        .map(|s| mcp_server_json(s, Some(&ownership)));
    Ok(Json(json!({ "data": owned })))
}

/// `GET /v1/mcp/servers` — list the MCP servers the caller may SEE: org-wide,
/// their own, ones shared with them, and (if admin) shared ones for governance.
/// Tokens are never returned.
async fn mcp_list(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpJsonError> {
    if let Err(error) = crate::runtime_registries::hydrate_mcp_registry(
        &state.mcp,
        &state.ownership,
        &state.http_client,
        &state.capability_core_base_url,
        &claims.org_id,
        capability_bearer.as_str(),
    )
    .await
    {
        warn!(org_id = %claims.org_id, %error, "MCP catalog hydration failed for list");
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "capability registry is unavailable"})),
        ));
    }
    let is_admin = req_is_admin(&claims, &headers);
    let resp = crate::runtime_registries::handle_list_mcp_servers(
        &state.mcp,
        ListMcpServersRequest {
            request_id: new_ulid(),
            org_id: claims.org_id.clone(),
        },
    )
    .map_err(|e| grpc_status_to_http(&e))?;
    let servers: Vec<Value> = resp
        .servers
        .iter()
        .filter(|s| {
            state.ownership.visible(
                &claims.org_id,
                crate::ownership::KIND_MCP,
                &s.server_id,
                &claims.user_id,
                is_admin,
            )
        })
        .map(|s| {
            let own = state
                .ownership
                .get(&claims.org_id, crate::ownership::KIND_MCP, &s.server_id);
            mcp_server_json(s, own.as_ref())
        })
        .collect();
    Ok(Json(json!({ "data": { "servers": servers } })))
}

/// `DELETE /v1/mcp/servers/:server_id` — remove a server. Only the owner (user
/// resource) or an admin (org resource) may delete; an admin cannot delete a
/// user's private resource (they cannot even see it).
async fn mcp_delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    // Owner decision 17: removing a server is the owner's housekeeping under every posture. It sends no
    // content, so a ZDR principal may remove one. The ownership checks below still decide who may.
    let is_admin = req_is_admin(&claims, &headers);
    if !state.ownership.can_modify(
        &claims.org_id,
        crate::ownership::KIND_MCP,
        &server_id,
        &claims.user_id,
        is_admin,
    ) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "not allowed to remove this MCP server" })),
        ));
    }
    if state.capability_core_base_url.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "capability registry is unavailable"})),
        ));
    }
    let catalog_url = format!("{}/api/v1/mcp/{server_id}", state.capability_core_base_url);
    let catalog_response = state
        .http_client
        .delete(catalog_url)
        .bearer_auth(capability_bearer.as_str())
        .send()
        .await
        .map_err(|error| {
            warn!(error = %error, "MCP catalog delete unavailable");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "capability registry is unavailable"})),
            )
        })?;
    if !catalog_response.status().is_success()
        && catalog_response.status() != reqwest::StatusCode::NOT_FOUND
    {
        warn!(status = %catalog_response.status(), "MCP catalog delete rejected");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "capability registry rejected MCP deletion"})),
        ));
    }
    let removed = state.mcp.remove(&claims.org_id, &server_id);
    state
        .ownership
        .remove(&claims.org_id, crate::ownership::KIND_MCP, &server_id);
    Ok(Json(
        json!({ "data": { "removed": removed, "server_id": server_id } }),
    ))
}

/// `POST /v1/mcp/servers/:server_id/share` — replace the set of users a
/// user-owned server is shared with. Owner only; a user may share with specific
/// users but can NEVER make the server org-wide (that requires admin re-create).
async fn mcp_share(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    Path(server_id): Path<String>,
    Json(body): Json<McpShareBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Sharing changes durable tool exposure policy and is never an ephemeral
    // operation, regardless of any caller-supplied body fields.
    require_non_zdr_durable_mutation(&claims)?;
    // Rehydrate first so a fresh gateway replica cannot make a share decision
    // from an empty sidecar. A failed read is a safe refusal, not a local-only
    // grant that would disappear on the next replica.
    crate::runtime_registries::hydrate_mcp_registry(
        &state.mcp,
        &state.ownership,
        &state.http_client,
        &state.capability_core_base_url,
        &claims.org_id,
        capability_bearer.as_str(),
    )
    .await
    .map_err(|error| {
        warn!(org_id = %claims.org_id, %error, "MCP catalog hydration failed before share");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "capability registry is unavailable"})),
        )
    })?;
    let prior = state
        .ownership
        .get(&claims.org_id, crate::ownership::KIND_MCP, &server_id);
    let updated = state
        .ownership
        .set_shares(
            &claims.org_id,
            crate::ownership::KIND_MCP,
            &server_id,
            &claims.user_id,
            body.user_ids,
        )
        .map_err(|e| (StatusCode::FORBIDDEN, Json(json!({ "error": e }))))?;
    let catalog_url = format!("{}/api/v1/mcp/{server_id}", state.capability_core_base_url);
    let catalog_response = state
        .http_client
        .patch(catalog_url)
        .bearer_auth(capability_bearer.as_str())
        .json(&json!({ "shared_with": updated.shared_with }))
        .send()
        .await;
    let catalog_ok = catalog_response
        .as_ref()
        .is_ok_and(|response| response.status().is_success());
    if !catalog_ok {
        if let Some(previous) = prior {
            state.ownership.set(
                &claims.org_id,
                crate::ownership::KIND_MCP,
                &server_id,
                previous,
            );
        } else {
            state
                .ownership
                .remove(&claims.org_id, crate::ownership::KIND_MCP, &server_id);
        }
        match catalog_response {
            Ok(response) => warn!(status = %response.status(), "MCP catalog share update rejected"),
            Err(error) => warn!(error = %error, "MCP catalog share update unavailable"),
        }
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "capability registry rejected MCP sharing"})),
        ));
    }
    Ok(Json(json!({ "data": {
        "server_id": server_id,
        "scope": updated.scope.as_wire(),
        "owner_user_id": updated.owner_user_id,
        "shared_with": updated.shared_with,
    } })))
}

#[derive(Debug, Deserialize)]
struct McpOAuthStartBody {
    name: String,
    /// The MCP server's base URL — must be public HTTPS. Discovery reads its
    /// `.well-known/oauth-protected-resource` document from here.
    url: String,
    #[serde(default)]
    tool_allowlist: Vec<String>,
    #[serde(default = "default_mcp_scope")]
    scope: String,
}

/// `POST /v1/mcp/servers/oauth/start` — begin connecting an MCP server that
/// requires real OAuth 2.1 login (Visma Net, etc). Discovers the server's
/// protected-resource + authorization-server metadata, dynamically registers
/// a client (RFC 7591 — no pre-existing app needed), and returns the
/// `authorization_url` the browser should be redirected to next.
async fn mcp_oauth_start(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<McpOAuthStartBody>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
    if body.name.trim().is_empty() || body.url.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name and url are required" })),
        ));
    }

    let resource_metadata = crate::mcp_oauth::discover_protected_resource(&body.url)
        .await
        .map_err(|e| {
            warn!(error = %e, "mcp oauth: protected-resource discovery failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "could not discover the MCP server's authorization metadata" })),
            )
        })?;
    start_oauth_connection(
        &state,
        &claims,
        body.name,
        body.url,
        body.tool_allowlist,
        body.scope,
        resource_metadata,
    )
    .await
}

/// Continue the OAuth 2.1 + DCR handshake given already-discovered
/// protected-resource metadata: discover the authorization server, register
/// a client (RFC 7591), stash a pending PKCE attempt, and return the
/// `authorization_url` the browser should navigate to next. Shared by
/// `mcp_oauth_start` (explicit OAuth connect) and `mcp_connect` (the
/// auto-detecting entry point, once discovery confirms the server needs it).
async fn start_oauth_connection(
    state: &AppState,
    claims: &Claims,
    name: String,
    url: String,
    tool_allowlist: Vec<String>,
    scope: String,
    resource_metadata: crate::mcp_oauth::ProtectedResourceMetadata,
) -> Result<Json<Value>, HttpJsonError> {
    let Some(authorization_server) = resource_metadata.authorization_servers.first() else {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "MCP server did not name an authorization server" })),
        ));
    };
    let auth_server_metadata = crate::mcp_oauth::discover_authorization_server(
        authorization_server,
    )
    .await
    .map_err(|e| {
        warn!(error = %e, "mcp oauth: authorization-server discovery failed");
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "could not discover the authorization server's metadata" })),
        )
    })?;
    let Some(registration_endpoint) = auth_server_metadata.registration_endpoint.as_deref() else {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                json!({ "error": "authorization server does not support dynamic client registration" }),
            ),
        ));
    };
    let redirect_uri = format!(
        "{}/api/v1/mcp/servers/oauth/callback",
        state.verevon_public_origin.trim_end_matches('/')
    );
    let registration = crate::mcp_oauth::register_client(registration_endpoint, &redirect_uri)
        .await
        .map_err(|e| {
            warn!(error = %e, "mcp oauth: dynamic client registration failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "dynamic client registration was rejected" })),
            )
        })?;

    let pkce = crate::mcp_oauth::generate_pkce();
    let state_value = crate::mcp_oauth::generate_state();
    let authorization_url = crate::mcp_oauth::build_authorization_url(
        &auth_server_metadata.authorization_endpoint,
        &registration.client_id,
        &redirect_uri,
        &state_value,
        &pkce.challenge,
        &resource_metadata.scopes_supported,
    );

    state.mcp.start_oauth(
        state_value,
        crate::runtime_registries::PendingMcpOAuth::new(
            claims.org_id.clone(),
            claims.user_id.clone(),
            name,
            url,
            tool_allowlist,
            scope,
            pkce.verifier,
            registration.client_id,
            auth_server_metadata.token_endpoint,
            redirect_uri,
            resource_metadata.scopes_supported,
        ),
    );

    Ok(Json(json!({
        "data": { "needs_oauth": true, "authorization_url": authorization_url }
    })))
}

#[derive(Debug, Deserialize)]
struct McpConnectBody {
    name: String,
    /// Public HTTPS MCP server URL. Verevon auto-detects whether it needs
    /// OAuth 2.1 login (RFC 9728 `.well-known/oauth-protected-resource`) or
    /// works directly — the caller never picks a transport or auth mode.
    url: String,
    /// Only used if the server turns out NOT to need OAuth and still expects
    /// a static bearer token. Ignored (and unnecessary) for OAuth servers.
    #[serde(default)]
    token: String,
    #[serde(default)]
    tool_allowlist: Vec<String>,
    #[serde(default = "default_mcp_scope")]
    scope: String,
}

/// `POST /v1/mcp/servers/connect` — the single, auto-detecting entry point:
/// the caller supplies only a name and URL (plus optional scope/allowlist/
/// token). Discovers whether the server is an OAuth 2.1 protected resource
/// and either starts the OAuth 2.1 + DCR handshake (`needs_oauth: true` +
/// `authorization_url` to redirect the browser to) or registers it directly
/// as a plain HTTP MCP server (using `token` if the caller supplied one, and
/// returning the registered server like `mcp_register` does). No transport
/// or auth-kind choice is ever asked for: the secure-MVP dispatch path only
/// ever speaks HTTPS, and OAuth-vs-static is a fact about the target
/// server, not a choice Verevon's users should have to make.
async fn mcp_connect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    headers: HeaderMap,
    Json(body): Json<McpConnectBody>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
    let name = body.name.trim().to_owned();
    let url = body.url.trim().to_owned();
    if name.is_empty() || url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name and url are required" })),
        ));
    }
    if !url.starts_with("https://") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "url must be a public https:// server" })),
        ));
    }

    match crate::mcp_oauth::discover_protected_resource(&url).await {
        Ok(resource_metadata) => {
            start_oauth_connection(
                &state,
                &claims,
                name,
                url,
                body.tool_allowlist,
                body.scope,
                resource_metadata,
            )
            .await
        }
        Err(error) => {
            // Expected for the common case (most MCP servers aren't OAuth
            // protected resources) — not a real error, so this stays at
            // debug rather than warn. Falls straight through to a plain
            // registration using whatever token (possibly none) was given.
            debug!(%error, "mcp connect: no OAuth protected-resource metadata, registering directly");
            register_plain_mcp_server(
                &state,
                &claims,
                &headers,
                &capability_bearer,
                name,
                url,
                default_mcp_transport(),
                body.token,
                body.tool_allowlist,
                // The connect flow has no `enabled` knob — auto-detected
                // servers are registered live, as they always have been.
                true,
                body.scope,
            )
            .await
        }
    }
}

#[derive(Debug, Deserialize)]
struct McpOAuthCallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// `GET /v1/mcp/servers/oauth/callback` — the browser lands here after the
/// user completes (or cancels) consent at the authorization server. Never
/// renders anything itself: redirects back to the SPA's settings page with a
/// `mcp_oauth=connected|error` query flag. Failure detail is logged
/// server-side only — never echoed into the redirect URL.
async fn mcp_oauth_callback(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: VerifiedCapabilityBearer,
    Query(query): Query<McpOAuthCallbackQuery>,
) -> Response {
    let settings_url = format!(
        "{}/settings/mcp",
        state.verevon_public_origin.trim_end_matches('/')
    );
    let fail = |reason: &str| -> Response {
        warn!(reason, "mcp oauth callback failed");
        Redirect::to(&format!("{settings_url}?mcp_oauth=error")).into_response()
    };

    if let Some(error) = query.error.filter(|e| !e.is_empty()) {
        return fail(&format!("authorization server returned error: {error}"));
    }
    let (Some(code), Some(state_param)) = (query.code, query.state) else {
        return fail("missing code or state");
    };
    let Some(pending) = state.mcp.take_oauth(&state_param) else {
        return fail("connection attempt expired or was already used");
    };
    if pending.org_id != claims.org_id {
        return fail("organization mismatch between start and callback");
    }

    let tokens = match crate::mcp_oauth::exchange_code(
        &pending.token_endpoint,
        &pending.client_id,
        &code,
        &pending.redirect_uri,
        &pending.code_verifier,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error) => return fail(&format!("token exchange failed: {error}")),
    };

    let ownership = match crate::ownership::Scope::from_wire(&pending.scope_wire) {
        crate::ownership::Scope::Org => crate::ownership::Ownership::org(),
        crate::ownership::Scope::User => crate::ownership::Ownership::user(pending.user_id.clone()),
    };
    // The user picked scope/visibility, never individual tool names — same
    // auto-discovery register_plain_mcp_server uses for the non-OAuth path.
    let tool_allowlist = resolve_tool_allowlist(
        &pending.server_url,
        &tokens.access_token,
        pending.tool_allowlist,
    )
    .await;
    let server = McpServer {
        server_id: String::new(),
        name: pending.server_name,
        url: pending.server_url,
        transport: "http".to_owned(),
        token: String::new(),
        tool_allowlist,
        enabled: true,
    };
    let resp = match crate::runtime_registries::handle_register_mcp_server(
        &state.mcp,
        RegisterMcpServerRequest {
            request_id: new_ulid(),
            org_id: pending.org_id.clone(),
            server: Some(server),
        },
    ) {
        Ok(resp) => resp,
        Err(error) => return fail(&format!("server registration failed: {}", error.message())),
    };
    let Some(registered) = resp.server else {
        return fail("server registration returned no server");
    };

    if state.capability_core_base_url.is_empty() {
        state.mcp.remove(&pending.org_id, &registered.server_id);
        return fail("capability registry is unavailable");
    }
    let payload = crate::runtime_registries::mcp_capability_payload(
        &pending.org_id,
        &registered,
        &ownership,
        "oauth",
    );
    let catalog_url = format!("{}/api/v1/mcp", state.capability_core_base_url);
    let catalog_response = state
        .http_client
        .post(&catalog_url)
        .bearer_auth(capability_bearer.as_str())
        .json(&payload)
        .send()
        .await;
    let persisted_id = match catalog_response {
        Ok(response) if response.status().is_success() => {
            // capability-core upserts by (org_id, name): a name collision with
            // an EARLIER registration of this same server (e.g. reconnecting
            // after this gateway's ephemeral cache was wiped by a restart, so
            // it proposed a brand-new id for what is durably the same server)
            // keeps the ORIGINAL id, not the one this call proposed — every
            // other durable record scoped to this server (mcp_oauth_tokens'
            // FK) points at that original id, so it — not registered.server_id
            // — is what every call from here on must address.
            response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body.get("id").and_then(|v| v.as_str()).map(str::to_owned))
        }
        Ok(response) => {
            state.mcp.remove(&pending.org_id, &registered.server_id);
            warn!(status = %response.status(), "MCP catalog registration rejected");
            return fail("capability registry rejected MCP registration");
        }
        Err(error) => {
            state.mcp.remove(&pending.org_id, &registered.server_id);
            warn!(error = %error, "MCP catalog registration unavailable");
            return fail("capability registry rejected MCP registration");
        }
    };
    let server_id = match persisted_id {
        Some(id) if id == registered.server_id => registered.server_id.clone(),
        Some(id) => {
            // The catalog kept a different id than this ephemeral entry —
            // drop the orphaned one and re-insert under the id capability-core
            // actually persisted, so this cache and the durable catalog agree.
            state.mcp.remove(&pending.org_id, &registered.server_id);
            let mut corrected = registered.clone();
            corrected.server_id = id.clone();
            if let Err(error) = crate::runtime_registries::handle_register_mcp_server(
                &state.mcp,
                RegisterMcpServerRequest {
                    request_id: new_ulid(),
                    org_id: pending.org_id.clone(),
                    server: Some(corrected),
                },
            ) {
                warn!(error = %error, "mcp oauth: could not re-key cache to the persisted id");
            }
            id
        }
        None => {
            warn!("MCP catalog registration response had no id");
            registered.server_id.clone()
        }
    };
    state.ownership.set(
        &pending.org_id,
        crate::ownership::KIND_MCP,
        &server_id,
        ownership,
    );

    // Tokens are stored separately from the server record, encrypted at rest
    // by capability-core — never in the gateway's own (ephemeral, plaintext)
    // in-memory cache.
    let tokens_url = format!(
        "{}/api/v1/mcp/{}/oauth-tokens",
        state.capability_core_base_url, server_id
    );
    let tokens_payload = json!({
        "access_token": tokens.access_token,
        "refresh_token": tokens.refresh_token.unwrap_or_default(),
        "token_type": "Bearer",
        "scope": tokens.scope.unwrap_or_default(),
        "expires_in_seconds": tokens.expires_in,
        "token_endpoint": pending.token_endpoint,
        "client_id": pending.client_id,
    });
    let tokens_result = state
        .http_client
        .put(&tokens_url)
        .bearer_auth(capability_bearer.as_str())
        .json(&tokens_payload)
        .send()
        .await;
    let tokens_ok = tokens_result
        .as_ref()
        .is_ok_and(|response| response.status().is_success());
    if !tokens_ok {
        // The server record itself is durably registered; only the tokens
        // failed to save. Surface this distinctly so the user knows to
        // reconnect rather than assume the connection is fully live.
        return fail("connected, but token storage failed — reconnect to retry");
    }

    Redirect::to(&format!(
        "{settings_url}?mcp_oauth=connected&server_id={server_id}"
    ))
    .into_response()
}

/// Active inference model catalogue.
async fn ai_models(
    State(state): State<AppState>,
    inference_bearer: VerifiedInferenceBearer,
    Query(query): Query<AiModelsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(authenticated_inference_request(
            ListModelsRequest {
                modality: query.modality.unwrap_or_default(),
                provider: query.provider.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let models: Vec<Value> = resp
        .models
        .into_iter()
        .map(|model| {
            json!({
                "id": model.id,
                "provider": model.provider,
                "modality": model.modality,
                "streaming": model.streaming,
            })
        })
        .collect();

    Ok(Json(json!({ "models": models })))
}

/// Image generation, vision analysis, and OCR via inference-core vision providers.
async fn ai_images(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiImagesRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let operation = req
        .operation
        .as_deref()
        .unwrap_or(
            if req.image_url.is_some()
                || req.url.is_some()
                || req.content_base64.is_some()
                || req.image_base64.is_some()
            {
                "analyze"
            } else {
                "generate"
            },
        )
        .to_ascii_lowercase();

    match operation.as_str() {
        "generate" | "image" | "text_to_image" => {
            generate_image(state, claims, req, &inference_bearer).await
        }
        "analyze" | "vision" => analyze_image(state, claims, req, &inference_bearer).await,
        "ocr" | "extract_text" => extract_image_text(state, claims, req, &inference_bearer).await,
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "operation must be one of generate, analyze, or ocr"
            })),
        )),
    }
}

async fn ai_images_analyze(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiImagesRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_image(state, claims, req, &inference_bearer).await
}

async fn ai_images_ocr(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiImagesRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    extract_image_text(state, claims, req, &inference_bearer).await
}

async fn generate_image(
    state: AppState,
    claims: Claims,
    req: AiImagesRequest,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    // Image generation is not a ZDR feature: a ZDR posture is refused before any model call, and a
    // waived token (which names one connection) is refused as a route mismatch.
    if crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, false),
    )? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(mp_contracts::zdr_refusal::Refusal::feature_not_supported("image_generation").json_body()),
        ));
    }
    let prompt = req.prompt.or(req.input).unwrap_or_default();
    if prompt.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "prompt or input is required" })),
        ));
    }

    let resp = state
        .inference_client
        .clone()
        .generate_image(authenticated_inference_request(
            GenerateImageRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                prompt,
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                size: req.size.unwrap_or_default(),
                quality: req.quality.unwrap_or_default(),
                n: req.n.unwrap_or(1),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let images: Vec<Value> = resp
        .images
        .into_iter()
        .map(|image| {
            json!({
                "url": image.url,
                "b64_json": image.b64_json,
                "revised_prompt": image.revised_prompt,
            })
        })
        .collect();

    Ok(Json(json!({
        "id": request_id,
        "object": "image.generation",
        "images": images,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

async fn analyze_image(
    state: AppState,
    claims: Claims,
    req: AiImagesRequest,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    // AnalyzeImageRequest carries no ZDR flag: a ZDR posture is refused as an attachment, and a
    // waived token (one connection) as a route mismatch (see `zdr_posture`).
    if crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, false),
    )? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(mp_contracts::zdr_refusal::Refusal::feature_not_supported("attachments").json_body()),
        ));
    }
    let prompt = req
        .prompt
        .or(req.input)
        .unwrap_or_else(|| "Describe this image in detail.".to_owned());
    let (image_url, image_data) =
        image_input(req.url, req.image_url, req.content_base64, req.image_base64)?;

    let resp = state
        .inference_client
        .clone()
        .analyze_image(authenticated_inference_request(
            AnalyzeImageRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                image_url,
                image_data,
                mime_type: req.mime_type.unwrap_or_default(),
                prompt,
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                max_tokens: req.max_tokens.unwrap_or(1024),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "image.analysis",
        "description": resp.description,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "input_tokens": resp.input_tokens,
        "output_tokens": resp.output_tokens,
    })))
}

async fn extract_image_text(
    state: AppState,
    claims: Claims,
    req: AiImagesRequest,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    // ExtractImageTextRequest carries no ZDR flag: a ZDR posture is refused as an attachment, and a
    // waived token (one connection) as a route mismatch (see `zdr_posture`).
    if crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, false),
    )? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(mp_contracts::zdr_refusal::Refusal::feature_not_supported("attachments").json_body()),
        ));
    }
    let request_id = new_ulid();
    let (image_url, image_data) =
        image_input(req.url, req.image_url, req.content_base64, req.image_base64)?;

    let resp = state
        .inference_client
        .clone()
        .extract_image_text(authenticated_inference_request(
            ExtractImageTextRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                image_url,
                image_data,
                mime_type: req.mime_type.unwrap_or_default(),
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "image.ocr",
        "text": resp.text,
        "page_count": resp.page_count,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

fn image_input(
    url: Option<String>,
    image_url: Option<String>,
    content_base64: Option<String>,
    image_base64: Option<String>,
) -> Result<(String, Vec<u8>), HttpJsonError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let url = image_url.or(url).unwrap_or_default();
    let content_base64 = content_base64.or(image_base64).unwrap_or_default();
    if url.trim().is_empty() == content_base64.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "exactly one of image_url/url or content_base64/image_base64 is required"
            })),
        ));
    }
    if content_base64.trim().is_empty() {
        return Ok((url, Vec::new()));
    }
    let image_data = STANDARD.decode(content_base64).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content_base64/image_base64 must be valid base64" })),
        )
    })?;
    Ok((String::new(), image_data))
}

/// Speech synthesis/transcription via inference-core speech providers.
/// Verevon Flow dictation request — browser mic audio in, polished text out.
#[derive(serde::Deserialize)]
pub struct AiDictateRequest {
    /// Base64 audio from the client recorder (`MediaRecorder` webm/opus typical).
    pub audio_base64: String,
    /// Container format: "webm" | "ogg" | "wav" | "mp3" | "m4a". Defaults to
    /// "webm" — the browser `MediaRecorder` default this route exists to serve.
    #[serde(default)]
    pub format: Option<String>,
    /// BCP-47 language hint for the STT leg; empty → auto-detect.
    #[serde(default)]
    pub language: Option<String>,
    /// Optional destination hint used to lightly adapt tone
    /// (e.g. "chat message", "email", "document").
    #[serde(default)]
    pub context: Option<String>,
    /// Cleanup-pass model override. Empty → `MODEL_GATEWAY_DICTATE_MODEL`
    /// env, then "gpt-4o-mini" (fast non-reasoning tier — dictation is
    /// latency-sensitive).
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub zdr: bool,
}

/// Versioned system prompt for the dictation cleanup pass. The version prefix
/// keys inference-core's prompt cache, mirroring `RECOMMEND_PLAN_MODEL_VERSION`.
const DICTATE_SYSTEM_PROMPT: &str = concat!(
    "verevon-flow-dictate-v1: You are Verevon Flow, a dictation cleanup engine. ",
    "The user message is a raw speech-to-text transcript of the user dictating. ",
    "Return ONLY the cleaned transcript text — no preamble, no quotes, no commentary. ",
    "Rules: remove filler words and false starts (um, uh, eh, hmm, altså, liksom, ",
    "'you know' and 'like' when used as filler); fix punctuation, capitalization, ",
    "and obvious speech-recognition errors; apply the speaker's own corrections ",
    "(e.g. 'no wait, I meant X' becomes X); keep the speaker's language ",
    "(Norwegian or English), wording, meaning, and grammatical person exactly — ",
    "never answer questions in the transcript, never add, translate, or summarize ",
    "content; format clearly enumerated items as a list.",
);

/// Matches inference-core's `MAX_STT_AUDIO_BYTES` so oversize audio gets a
/// clean HTTP 400 here instead of a gRPC invalid-argument after upload.
const MAX_DICTATE_AUDIO_BYTES: usize = 25 * 1024 * 1024;

/// Verevon Flow dictation: one round trip from mic audio to polished text.
/// Chains inference-core `TranscribeSpeech` (STT) and a cleanup `Infer` pass
/// (strip fillers, punctuate, apply self-corrections). Fail-soft on the
/// cleanup leg: a cleanup failure returns the raw transcript (`cleaned:false`)
/// rather than dropping the user's dictation.
#[allow(clippy::too_many_lines)]
async fn ai_dictate(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDictateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

    let request_id = new_ulid();
    let zdr = crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::plain(req.zdr),
    )?;
    let audio = STANDARD.decode(&req.audio_base64).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "audio_base64 must be valid base64" })),
        )
    })?;
    if audio.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "audio_base64 is required for dictation" })),
        ));
    }
    if audio.len() > MAX_DICTATE_AUDIO_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "dictation audio exceeds the 25 MB limit" })),
        ));
    }
    let format = req
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("webm")
        .to_ascii_lowercase();
    // azure-speech's short-audio REST only decodes WAV/PCM (it maps unknown
    // formats to audio/wav and returns empty text), while the azure-openai
    // whisper deployment accepts the browser MediaRecorder containers. Route
    // compressed formats straight to whisper; keep WAV on the default chain
    // (azure-speech first — the only provider with a real confidence score).
    let stt_hint = if format == "wav" { "" } else { "azure-openai" };
    let language = req.language.unwrap_or_default();

    let stt = |hint: &str, audio: Vec<u8>| {
        let mut client = state.inference_client.clone();
        let request = authenticated_inference_request(
            TranscribeSpeechRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                audio,
                format: format.clone(),
                model: String::new(),
                provider_hint: hint.to_owned(),
                language: language.clone(),
                // The audio is under the same retention posture as the
                // clean-up pass below; inference-core refuses it (412) while
                // no speech deployment is attested for zero retention.
                zdr,
            },
            &inference_bearer,
        );
        async move { client.transcribe_speech(request).await }
    };

    let mut transcript = stt(stt_hint, audio.clone())
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    // A silent empty transcript from the default (azure-speech) path usually
    // means an undecodable container, not silence — retry once via whisper.
    if transcript.text.trim().is_empty() && stt_hint.is_empty() {
        if let Ok(retry) = stt("azure-openai", audio).await {
            transcript = retry.into_inner();
        }
    }
    let raw_text = transcript.text.trim().to_owned();
    if raw_text.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "no speech detected in the audio",
                "code": "no_speech_detected",
            })),
        ));
    }

    // Cleanup pass — fail-soft: dictation must never be lost to a cleanup
    // hiccup, so any Infer failure (or empty result) returns the raw text.
    let cleanup_model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("MODEL_GATEWAY_DICTATE_MODEL")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "gpt-4o-mini".to_owned());
    let mut system_prompt = DICTATE_SYSTEM_PROMPT.to_owned();
    if let Some(context) = req
        .context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let _ = write!(
            system_prompt,
            " The text will be used as: {context}. Match that tone lightly."
        );
    }
    let cleanup = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                purpose: InferPurpose::DictationCleanup as i32,
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: cleanup_model,
                provider_hint: String::new(),
                messages: vec![
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "system".to_owned(),
                        content: system_prompt,
                        name: String::new(),
                    },
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "user".to_owned(),
                        content: raw_text.clone(),
                        name: String::new(),
                    },
                ],
                temperature: 0.2,
                max_tokens: 2048,
                structured_output_schema: String::new(),
                zdr,
                ..Default::default()
            },
            &inference_bearer,
        ))
        .await;

    let (text, cleaned, model_used, usage) = match cleanup {
        Ok(resp) => {
            let resp = resp.into_inner();
            let cleaned_text = resp.content.trim().to_owned();
            if cleaned_text.is_empty() {
                (raw_text.clone(), false, resp.model_used, json!(null))
            } else {
                (
                    cleaned_text,
                    true,
                    resp.model_used,
                    json!({ "input_tokens": resp.input_tokens, "output_tokens": resp.output_tokens }),
                )
            }
        }
        Err(error) => {
            tracing::warn!(%error, request_id = %request_id, "dictate cleanup inference failed; returning raw transcript");
            (raw_text.clone(), false, String::new(), json!(null))
        }
    };

    Ok(Json(json!({
        "id": request_id,
        "object": "speech.dictation",
        "text": text,
        "raw_text": raw_text,
        "cleaned": cleaned,
        "detected_language": transcript.detected_language,
        "stt_model_used": transcript.model_used,
        "stt_provider_used": transcript.provider_used,
        "model_used": model_used,
        "usage": usage,
    })))
}

async fn ai_speech(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiSpeechRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let request_id = new_ulid();
    let zdr = crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::named(req.provider.as_deref(), None, req.zdr),
    )?;
    let operation = req
        .operation
        .as_deref()
        .unwrap_or(if req.audio_base64.is_some() {
            "stt"
        } else {
            "tts"
        })
        .to_ascii_lowercase();

    if matches!(operation.as_str(), "stt" | "transcribe" | "speech_to_text") {
        let Some(audio_base64) = req.audio_base64 else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "audio_base64 is required for speech transcription" })),
            ));
        };
        let audio = STANDARD.decode(audio_base64).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "audio_base64 must be valid base64" })),
            )
        })?;
        let resp = state
            .inference_client
            .clone()
            .transcribe_speech(authenticated_inference_request(
                TranscribeSpeechRequest {
                    request_id: request_id.clone(),
                    org_id: claims.org_id,
                    audio,
                    format: req.format.unwrap_or_else(|| "mp3".to_owned()),
                    model: req.model.unwrap_or_default(),
                    provider_hint: req.provider.unwrap_or_default(),
                    language: req.language.unwrap_or_default(),
                    zdr,
                },
                &inference_bearer,
            ))
            .await
            .map_err(|e| grpc_status_to_http(&e))?
            .into_inner();

        return Ok(Json(json!({
            "id": request_id,
            "object": "speech.transcription",
            "text": resp.text,
            "detected_language": resp.detected_language,
            "confidence": resp.confidence,
            "model_used": resp.model_used,
            "provider_used": resp.provider_used,
        })));
    }

    if !matches!(operation.as_str(), "tts" | "synthesize" | "text_to_speech") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "operation must be one of tts, synthesize, stt, or transcribe" }),
            ),
        ));
    }

    let text = req.text.or(req.input).unwrap_or_default();
    if text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "input or text is required for speech synthesis" })),
        ));
    }

    let resp = state
        .inference_client
        .clone()
        .synthesize_speech(authenticated_inference_request(
            SynthesizeSpeechRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                text,
                voice: req.voice.unwrap_or_default(),
                format: req.format.unwrap_or_else(|| "mp3".to_owned()),
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                language: req.language.unwrap_or_default(),
                zdr,
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "speech.audio",
        "audio_base64": STANDARD.encode(resp.audio),
        "format": resp.format,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "duration_ms": resp.duration_ms,
    })))
}

/// Provider voice catalogue for speech clients.
async fn ai_speech_voices(
    State(state): State<AppState>,
    inference_bearer: VerifiedInferenceBearer,
    Query(query): Query<AiSpeechVoicesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_speech_voices(authenticated_inference_request(
            ListSpeechVoicesRequest {
                provider: query.provider.unwrap_or_default(),
                language: query.language.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let voices: Vec<Value> = resp
        .voices
        .into_iter()
        .map(|voice| {
            json!({
                "id": voice.id,
                "name": voice.name,
                "language": voice.language,
                "gender": voice.gender,
                "provider": voice.provider,
            })
        })
        .collect();

    Ok(Json(json!({ "voices": voices })))
}

/// Batch-translate branch of [`ai_translate`].
///
/// # Errors
///
/// Returns a `400` if `items` is absent, or maps an upstream gRPC failure to an `HttpJsonError`.
async fn batch_translate(
    state: &AppState,
    org_id: String,
    req: AiTranslateRequest,
    request_id: &str,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let Some(items) = req.items else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "items are required for batch translation" })),
        ));
    };
    let resp = state
        .inference_client
        .clone()
        .batch_translate_text(authenticated_inference_request(
            BatchTranslateTextRequest {
                request_id: request_id.to_owned(),
                org_id,
                items: items
                    .into_iter()
                    .map(|item| TranslationInput {
                        id: item.id.unwrap_or_default(),
                        text: item.text,
                    })
                    .collect(),
                source_language: req.source_language.unwrap_or_default(),
                target_language: req.target_language.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                model: req.model.unwrap_or_default(),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let translations: Vec<Value> = resp
        .translations
        .into_iter()
        .map(|item| {
            json!({
                "id": item.id,
                "original_text": item.original_text,
                "translated_text": item.translated_text,
                "detected_language": item.detected_language,
                "confidence": item.confidence,
            })
        })
        .collect();

    Ok(Json(json!({
        "id": request_id,
        "object": "translation.batch",
        "translations": translations,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

/// Translation via inference-core translation providers.
async fn ai_translate(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiTranslateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let operation = req
        .operation
        .as_deref()
        .unwrap_or(if req.items.is_some() {
            "batch"
        } else {
            "translate"
        })
        .to_ascii_lowercase();

    // Every operation but the language list sends the caller's text to a provider. Its posture is
    // decided here, before any call, from the verified claims and the provider the request names.
    if !matches!(operation.as_str(), "languages" | "list_languages") {
        crate::zdr_posture::decide_content_route(&claims, req.provider.as_deref(), "translation")?;
    }

    if matches!(operation.as_str(), "detect" | "detect_language") {
        return detect_text_language(
            state,
            claims.org_id,
            req.text.unwrap_or_default(),
            req.provider.unwrap_or_default(),
            req.model.unwrap_or_default(),
            &inference_bearer,
        )
        .await;
    }

    if matches!(operation.as_str(), "languages" | "list_languages") {
        return list_translation_languages(
            state,
            req.provider.unwrap_or_default(),
            &inference_bearer,
        )
        .await;
    }

    let request_id = new_ulid();
    if matches!(operation.as_str(), "batch" | "batch_translate") {
        return batch_translate(&state, claims.org_id, req, &request_id, &inference_bearer).await;
    }

    if !matches!(operation.as_str(), "translate" | "text") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "operation must be translate, batch, detect, or languages" })),
        ));
    }

    let resp = state
        .inference_client
        .clone()
        .translate_text(authenticated_inference_request(
            TranslateTextRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                text: req.text.unwrap_or_default(),
                source_language: req.source_language.unwrap_or_default(),
                target_language: req.target_language.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                model: req.model.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "translation",
        "translated_text": resp.translated_text,
        "detected_language": resp.detected_language,
        "confidence": resp.confidence,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

/// Detect text language via inference-core translation providers.
async fn ai_translate_detect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiTranslateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    crate::zdr_posture::decide_content_route(&claims, req.provider.as_deref(), "translation")?;
    detect_text_language(
        state,
        claims.org_id,
        req.text.unwrap_or_default(),
        req.provider.unwrap_or_default(),
        req.model.unwrap_or_default(),
        &inference_bearer,
    )
    .await
}

/// Supported translation languages.
async fn ai_translate_languages(
    State(state): State<AppState>,
    inference_bearer: VerifiedInferenceBearer,
    Query(query): Query<AiTranslateLanguagesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    list_translation_languages(state, query.provider.unwrap_or_default(), &inference_bearer).await
}

async fn detect_text_language(
    state: AppState,
    org_id: String,
    text: String,
    provider_hint: String,
    model: String,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .detect_text_language(authenticated_inference_request(
            DetectTextLanguageRequest {
                request_id: request_id.clone(),
                org_id,
                text,
                provider_hint,
                model,
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let detections: Vec<Value> = resp
        .detections
        .into_iter()
        .map(|detection| {
            json!({
                "language": detection.language,
                "confidence": detection.confidence,
                "is_translation_supported": detection.is_translation_supported,
            })
        })
        .collect();

    Ok(Json(json!({
        "id": request_id,
        "object": "translation.language_detection",
        "detections": detections,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

async fn list_translation_languages(
    state: AppState,
    provider: String,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_translation_languages(authenticated_inference_request(
            ListTranslationLanguagesRequest { provider },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let languages: Vec<Value> = resp
        .languages
        .into_iter()
        .map(|language| {
            json!({
                "code": language.code,
                "name": language.name,
                "native_name": language.native_name,
                "direction": language.direction,
            })
        })
        .collect();

    Ok(Json(json!({ "languages": languages })))
}

async fn ai_documents_analyze(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-document", &inference_bearer).await
}

async fn ai_documents_layout(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-layout", &inference_bearer).await
}

async fn ai_documents_forms(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-document", &inference_bearer).await
}

async fn ai_documents_receipts(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-receipt", &inference_bearer).await
}

async fn ai_documents_invoices(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-invoice", &inference_bearer).await
}

async fn analyze_document_with_model(
    state: AppState,
    claims: Claims,
    req: AiDocumentIntelRequest,
    default_model: &str,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    crate::zdr_posture::decide_content_route(
        &claims,
        req.provider.as_deref(),
        "document_analysis",
    )?;
    let request_id = new_ulid();
    let (document_url, document_data) = document_input(
        req.url,
        req.document_url,
        req.content_base64,
        req.document_base64,
    )?;
    let resp = state
        .inference_client
        .clone()
        .analyze_document(authenticated_inference_request(
            AnalyzeDocumentRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                document_url,
                document_data,
                content_type: req.content_type.unwrap_or_default(),
                model: req.model.unwrap_or_else(|| default_model.to_owned()),
                provider_hint: req.provider.unwrap_or_default(),
                pages: req.pages.unwrap_or_default(),
                locale: req.locale.unwrap_or_default(),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "document.analysis",
        "status": resp.status,
        "content": resp.content,
        "fields": parse_json_value(&resp.fields_json),
        "tables": parse_json_value(&resp.tables_json),
        "paragraphs": resp.paragraphs,
        "raw": parse_json_value(&resp.raw_json),
        "pages_processed": resp.pages_processed,
        "confidence": resp.confidence,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

fn document_input(
    url: Option<String>,
    document_url: Option<String>,
    content_base64: Option<String>,
    document_base64: Option<String>,
) -> Result<(String, Vec<u8>), HttpJsonError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let url = document_url.or(url).unwrap_or_default();
    let content_base64 = content_base64.or(document_base64).unwrap_or_default();
    if url.trim().is_empty() == content_base64.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "exactly one of document_url/url or content_base64/document_base64 is required"
            })),
        ));
    }
    if content_base64.trim().is_empty() {
        return Ok((url, Vec::new()));
    }
    let document_data = STANDARD.decode(content_base64).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content_base64/document_base64 must be valid base64" })),
        )
    })?;
    Ok((String::new(), document_data))
}

fn parse_json_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

async fn ai_language(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let operation = req
        .operation
        .clone()
        .unwrap_or_else(|| "sentiment".to_owned());
    analyze_language_with_operation(state, claims, req, &operation, &inference_bearer).await
}

async fn ai_language_sentiment(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "sentiment", &inference_bearer).await
}

async fn ai_language_entities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "entities", &inference_bearer).await
}

async fn ai_language_key_phrases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "key_phrases", &inference_bearer).await
}

async fn ai_language_pii(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "pii", &inference_bearer).await
}

async fn ai_language_detect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "detect", &inference_bearer).await
}

async fn ai_language_summary_text(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "summary", &inference_bearer).await
}

async fn analyze_language_with_operation(
    state: AppState,
    claims: Claims,
    req: AiLanguageRequest,
    operation: &str,
    inference_bearer: &VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    crate::zdr_posture::decide_content_route(
        &claims,
        req.provider.as_deref(),
        "language_analysis",
    )?;
    let request_id = new_ulid();
    let texts = language_texts(req.text, req.texts)?;
    let resp = state
        .inference_client
        .clone()
        .analyze_language(authenticated_inference_request(
            AnalyzeLanguageRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                operation: operation.to_owned(),
                texts,
                language: req.language.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                model: req.model.unwrap_or_default(),
                sentence_count: req.sentence_count.unwrap_or(3),
                summary_kind: req
                    .summary_kind
                    .or(req.kind)
                    .unwrap_or_else(|| "AbstractiveSummarization".to_owned()),
            },
            inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let results: Vec<Value> = resp.results.iter().map(language_result_value).collect();

    Ok(Json(json!({
        "id": request_id,
        "object": format!("language.{}", resp.operation),
        "results": results,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
    })))
}

fn language_texts(
    text: Option<String>,
    texts: Option<Vec<String>>,
) -> Result<Vec<String>, HttpJsonError> {
    let texts = texts.unwrap_or_else(|| text.into_iter().collect());
    if texts.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "text or texts is required" })),
        ));
    }
    Ok(texts)
}

fn language_result_value(result: &mp_contracts::model_plane::v1::LanguageAnalysisResult) -> Value {
    json!({
        "id": result.id,
        "sentiment": result.sentiment,
        "confidence_scores": parse_json_value(&result.confidence_scores_json),
        "sentences": parse_json_value(&result.sentences_json),
        "entities": parse_json_value(&result.entities_json),
        "key_phrases": result.key_phrases,
        "redacted_text": result.redacted_text,
        "detected_language": {
            "name": result.detected_language_name,
            "iso_code": result.detected_language_code,
            "confidence": result.confidence,
        },
        "summary": result.summary,
        "raw": parse_json_value(&result.raw_json),
    })
}

/// Ingest a document via Data Plane v2 `DocumentService`.
async fn ai_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedBearer,
    Json(req): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::dataplane::documents_v2::CreateDocumentRequest;
    if crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::plain(req["zdr"].as_bool().unwrap_or(false)),
    )?
        || crate::dataplane::document_zdr_requested(
            false,
            req["zdr_classification"].as_str().unwrap_or(""),
        )
    {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.json_body()),
        ));
    }
    let request = crate::retrieval::authorize(
        tonic::Request::new(CreateDocumentRequest {
            org_id: claims.org_id,
            source: req["url"].as_str().unwrap_or("").to_owned(),
            r#type: req["type"].as_str().unwrap_or("document").to_owned(),
            title: req["title"].as_str().unwrap_or("").to_owned(),
            content: req["content"].as_str().unwrap_or("").to_owned(),
            metadata: None,
            zdr_classification: req["zdr_classification"].as_str().unwrap_or("").to_owned(),
            ingest_policy: None,
        }),
        &bearer,
    )
    .map_err(|status| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": status.message() })),
        )
    })?;
    let resp = state
        .document_client
        .clone()
        .create_document(request)
        .await
        .map_err(|e| {
            let code = match e.code() {
                tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
                tonic::Code::Unavailable => StatusCode::BAD_GATEWAY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (code, Json(json!({ "error": e.message() })))
        })?
        .into_inner();
    let doc_json = resp.document.as_ref().map(crate::dataplane::document_value);
    Ok(Json(json!({ "document": doc_json })))
}

async fn ai_realtime(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiRealtimeSessionRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    crate::zdr_posture::decide_content_route(&claims, req.provider.as_deref(), "realtime_voice")?;
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_realtime_session(authenticated_inference_request(
            CreateRealtimeSessionRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                voice: req.voice.unwrap_or_else(|| "alloy".to_owned()),
                instructions: req.instructions.unwrap_or_default(),
                input_audio_format: req.input_audio_format.unwrap_or_else(|| "pcm16".to_owned()),
                output_audio_format: req
                    .output_audio_format
                    .unwrap_or_else(|| "pcm16".to_owned()),
                turn_detection_type: req
                    .turn_detection_type
                    .unwrap_or_else(|| "server_vad".to_owned()),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "realtime.session",
        "session_id": resp.session_id,
        "client_secret": resp.client_secret,
        "websocket_url": resp.websocket_url,
        "expires_at": resp.expires_at,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "voice": resp.voice,
    })))
}

async fn ai_realtime_models(
    State(state): State<AppState>,
    inference_bearer: VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(authenticated_inference_request(
            ListModelsRequest {
                modality: "realtime".to_owned(),
                provider: String::new(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let models: Vec<Value> = resp
        .models
        .into_iter()
        .map(|model| {
            json!({
                "id": model.id,
                "provider": model.provider,
                "modality": model.modality,
                "streaming": model.streaming,
            })
        })
        .collect();

    Ok(Json(json!({ "models": models })))
}

async fn ai_video_generate(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiVideoGenerateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    crate::zdr_posture::decide_content_route(&claims, req.provider.as_deref(), "video_generation")?;
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_video_generation_job(authenticated_inference_request(
            CreateVideoGenerationJobRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                prompt: req.prompt,
                width: req.width.unwrap_or(1280),
                height: req.height.unwrap_or(720),
                duration_seconds: req.duration_seconds.unwrap_or(5),
                n_variants: req.n_variants.unwrap_or(1),
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "video.generation.job",
        "job_id": resp.job_id,
        "status": resp.status,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "raw": parse_json_value(&resp.raw_json),
    })))
}

async fn ai_video_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Path(job_id): Path<String>,
    Query(query): Query<AiVideoJobQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .get_video_generation_job(authenticated_inference_request(
            GetVideoGenerationJobRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                job_id,
                provider_hint: query.provider.unwrap_or_default(),
                model: query.model.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "video.generation.job",
        "job_id": resp.job_id,
        "status": resp.status,
        "generation_id": resp.generation_id,
        "video_url": resp.video_url,
        "error": resp.error,
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "raw": parse_json_value(&resp.raw_json),
    })))
}

async fn ai_video_content(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Path(generation_id): Path<String>,
    Query(query): Query<AiVideoJobQuery>,
) -> Result<Response, HttpJsonError> {
    let request_id = new_ulid();
    let stream = state
        .inference_client
        .clone()
        .stream_video_generation_content(authenticated_inference_request(
            StreamVideoGenerationContentRequest {
                request_id,
                org_id: claims.org_id,
                generation_id,
                provider_hint: query.provider.unwrap_or_default(),
                model: query.model.unwrap_or_default(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let body_stream = futures::stream::unfold(stream, |mut stream| async move {
        match stream.message().await {
            Ok(Some(chunk)) if chunk.done => None,
            Ok(Some(chunk)) => Some((Ok::<Bytes, std::io::Error>(Bytes::from(chunk.data)), stream)),
            Ok(None) => None,
            Err(status) => Some((Err(std::io::Error::other(status.to_string())), stream)),
        }
    });

    let mut response = Body::from_stream(body_stream).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    Ok(response)
}

async fn ai_video_models(
    State(state): State<AppState>,
    inference_bearer: VerifiedInferenceBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(authenticated_inference_request(
            ListModelsRequest {
                modality: "video".to_owned(),
                provider: String::new(),
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    let models: Vec<Value> = resp
        .models
        .into_iter()
        .map(|model| {
            json!({
                "id": model.id,
                "provider": model.provider,
                "modality": model.modality,
                "streaming": model.streaming,
            })
        })
        .collect();

    Ok(Json(json!({ "models": models })))
}

// ============================================================================
// Capability proxy handlers  /v1/capabilities/*
// ============================================================================

fn capability_contract_value(
    capability: &mp_contracts::model_plane::v1::CapabilityDetail,
) -> Value {
    use mp_contracts::tool_class::{effective_floor, ApprovalFloor, SideEffectClass};
    let state = capability.state.as_str();
    let execution_mode = capability.execution_mode.as_str();
    let cost_class = capability.cost_class.as_str();
    let complete = !capability.reason_code.is_empty()
        && matches!(cost_class, "unknown" | "bounded" | "variable")
        && match state {
            "available" => {
                matches!(execution_mode, "direct_read" | "agentic")
                    && !capability.health_checked_at.is_empty()
            }
            "approval_required" => {
                execution_mode == "agentic"
                    && capability.requires_approval
                    && !capability.health_checked_at.is_empty()
            }
            "disabled" | "unavailable" => execution_mode == "unavailable",
            "unhealthy" | "not_configured" => {
                execution_mode == "unavailable" && !capability.health_checked_at.is_empty()
            }
            _ => false,
        };
    let (state, reason_code, reason, execution_mode, cost_class, health_checked_at) = if complete {
        (
            capability.state.as_str(),
            capability.reason_code.as_str(),
            capability.reason.as_str(),
            capability.execution_mode.as_str(),
            capability.cost_class.as_str(),
            capability.health_checked_at.as_str(),
        )
    } else {
        (
            "unavailable",
            "availability_contract_missing",
            "Capability source did not provide a complete availability contract.",
            "unavailable",
            "unknown",
            "",
        )
    };
    // The declared class and approval floor ride on every public capability. An
    // empty or unrecognised value is the strictest (`unclassified`, and
    // `durable_owner` for the floor), never omitted and never a weaker answer, so
    // a mixed-version capability-core cannot make a tool look like a read. The
    // floor is also raised to what the class implies by itself (an unclassified
    // class implies at least `ask`).
    let side_effect_class = SideEffectClass::from_wire(&capability.side_effect_class);
    let approval_floor = effective_floor(
        side_effect_class,
        ApprovalFloor::from_wire(&capability.approval_floor),
    );
    json!({
        "id": capability.capability_id,
        "name": capability.name,
        "kind": capability.kind,
        "version": capability.version,
        "description": capability.description,
        "risk_level": capability.risk_level,
        "lazy_load": capability.lazy_load,
        "scope": capability.scope,
        "state": state,
        "reason_code": reason_code,
        "reason": reason,
        "requires_approval": capability.requires_approval,
        "execution_mode": execution_mode,
        "cost_class": cost_class,
        "health_checked_at": health_checked_at,
        "side_effect_class": side_effect_class.as_str(),
        "approval_floor": approval_floor.as_str(),
    })
}

async fn list_capabilities_proxy(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListCapabilitiesRequest;
    let resp = state
        .capability_client
        .clone()
        .list_capabilities(capability_grpc_request(
            ListCapabilitiesRequest {
                kind_filter: q.get("kind").cloned().unwrap_or_default(),
                query: q.get("q").cloned().unwrap_or_default(),
                after_id: q.get("after_id").cloned().unwrap_or_default(),
                limit: q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    Ok(Json(json!({
        "capabilities": resp.capabilities.iter().map(capability_contract_value).collect::<Vec<_>>(),
        "has_more": resp.has_more,
    })))
}

async fn get_capability_proxy(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::GetCapabilityRequest;
    let c = state
        .capability_client
        .clone()
        .get_capability(capability_grpc_request(
            GetCapabilityRequest {
                capability_id: id,
                version_constraint: String::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    Ok(Json(capability_contract_value(&c)))
}

// ============================================================================
// Tasks / cron / memory / skills  — proxy to capability-core HTTP API
// The capability-core service exposes these on :8085 and we forward from the
// public gateway so clients have a single origin.
// ============================================================================

/// Proxy GET/POST to capability-core's /api/v1/{path}.
async fn proxy_to_capability_core(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedCapabilityBearer,
    path: &str,
    method: &str,
    body: Option<&Value>,
) -> Result<Json<Value>, HttpJsonError> {
    // Refuse before delegating: the durable-mutation inventory checks this entry point on its own.
    if method != "GET" {
        require_non_zdr_durable_mutation(claims)?;
    }
    proxy_to_capability_core_with_precondition(state, claims, bearer, path, method, body, None)
        .await
}

/// [`proxy_to_capability_core`] with an optional `If-Match` revision on the forwarded request: a write
/// capability-core applies only at the revision its author read.
async fn proxy_to_capability_core_with_precondition(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedCapabilityBearer,
    path: &str,
    method: &str,
    body: Option<&Value>,
    if_match: Option<&str>,
) -> Result<Json<Value>, HttpJsonError> {
    // Capability Core remains authoritative, but stop an obvious issuer-ZDR
    // mutation at the public boundary before forwarding any body or credential.
    if method != "GET" {
        require_non_zdr_durable_mutation(claims)?;
    }
    let base = &state.capability_core_base_url;
    let url = format!("{base}/api/v1/{path}");
    let client = &state.http_client;
    let builder = match method {
        "GET" => client.get(&url),
        "POST" => {
            let b = body.cloned().unwrap_or(json!({}));
            client.post(&url).json(&b)
        }
        "PATCH" => {
            let b = body.cloned().unwrap_or(json!({}));
            client.patch(&url).json(&b)
        }
        "PUT" => {
            let b = body.cloned().unwrap_or(json!({}));
            client.put(&url).json(&b)
        }
        "DELETE" => client.delete(&url),
        _ => {
            return Err((
                StatusCode::METHOD_NOT_ALLOWED,
                Json(json!({"error":"method not allowed"})),
            ))
        }
    }
    .bearer_auth(bearer.as_str());
    let builder = match if_match {
        Some(revision) => builder.header(reqwest::header::IF_MATCH, revision),
        None => builder,
    };
    let resp = builder.send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    let status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = resp.json().await.unwrap_or(json!({}));
    if status.is_success() {
        Ok(Json(body))
    } else {
        Err((status, Json(body)))
    }
}

fn capability_grpc_request<T>(
    value: T,
    bearer: &VerifiedCapabilityBearer,
) -> Result<tonic::Request<T>, HttpJsonError> {
    let mut request = tonic::Request::new(value);
    let authorization = format!("Bearer {}", bearer.as_str()).parse().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "verified capability credential is not forwardable"})),
        )
    })?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// `resource?query` for [`proxy_to_capability_core`], each value encoded as a
/// query component. A caller's value is data: a raw `&` or `=` in it would
/// otherwise become another parameter, such as a second `org_id`.
fn capability_query_path(resource: &str, pairs: &[(&str, &str)]) -> String {
    // `reqwest::Url` is the encoder this crate already uses for query strings;
    // the base is a placeholder and only the encoded query is kept.
    let mut url =
        reqwest::Url::parse("http://capability-core.invalid/").expect("a static base URL parses");
    url.query_pairs_mut().extend_pairs(pairs);
    match url.query() {
        Some(query) if !query.is_empty() => format!("{resource}?{query}"),
        _ => resource.to_owned(),
    }
}

async fn list_tasks_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let status = q.get("status").map(String::as_str).unwrap_or_default();
    let path = capability_query_path(
        "tasks",
        &[("org_id", c.org_id.as_str()), ("status", status)],
    );
    proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await
}
async fn create_task_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, &c, &bearer, "tasks", "POST", Some(&b)).await
}
async fn get_task_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("tasks/{id}"), "GET", None).await
}
async fn patch_task_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("tasks/{id}"), "PATCH", Some(&b)).await
}
async fn cancel_task_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("tasks/{id}/cancel"), "POST", None).await
}

/// Whether `value` may stand as one path segment of a capability-core URL: the
/// registry's own identifier alphabet. Axum hands a handler the decoded segment,
/// so without this a `%2F..%2F` in an id would walk the forwarded request to
/// another capability-core route under the caller's bearer.
fn is_registry_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 200
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

fn require_registry_identifier(value: &str, what: &str) -> Result<(), HttpJsonError> {
    if is_registry_identifier(value) {
        return Ok(());
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("{what} is not a valid identifier") })),
    ))
}

/// `GET /v1/mcp/servers/:server_id/tools` — the registry rows of one server's
/// tools (class, floor, review state, digests; never a description or schema).
async fn list_mcp_server_tools_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(server_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    require_registry_identifier(&server_id, "server_id")?;
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("mcp/{server_id}/tools"),
        "GET",
        None,
    )
    .await
}

/// `POST /v1/mcp/servers/:server_id/tools/:tool/approve` — a person accepts the
/// definition of a tool they reviewed. The body names the digest they saw; whether
/// they may approve (server owner, or an admin for a shared server) is
/// capability-core's decision on the signed principal, not this route's.
async fn approve_mcp_tool_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path((server_id, tool)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    require_registry_identifier(&server_id, "server_id")?;
    require_registry_identifier(&tool, "tool")?;
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("mcp/{server_id}/tools/{tool}/approve"),
        "POST",
        Some(&body),
    )
    .await
}

/// `GET /v1/tool-policies?capability_id=` — the org's policies and the caller's own.
async fn list_tool_policies_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let capability_id = q
        .get("capability_id")
        .map(String::as_str)
        .unwrap_or_default();
    if !capability_id.is_empty() {
        require_registry_identifier(capability_id, "capability_id")?;
    }
    let path = capability_query_path("tool-policies", &[("capability_id", capability_id)]);
    proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await
}

/// `PUT /v1/tool-policies/:capability_id` — set the caller's (or, for an admin,
/// the org's) allow / ask / never policy. The body is forwarded as sent:
/// capability-core decodes it strictly and checks the scope, the admin claim, the
/// tool's class and the digest. A ZDR caller is refused here before anything is
/// forwarded.
async fn set_tool_policy_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(capability_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    require_registry_identifier(&capability_id, "capability_id")?;
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("tool-policies/{capability_id}"),
        "PUT",
        Some(&body),
    )
    .await
}

/// `DELETE /v1/tool-policies/:capability_id?scope=user|org` — revoke the caller's
/// (or, for an admin, the org's) policy.
async fn revoke_tool_policy_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(capability_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    require_registry_identifier(&capability_id, "capability_id")?;
    let scope = q.get("scope").map(String::as_str).unwrap_or_default();
    let path = capability_query_path(
        &format!("tool-policies/{capability_id}"),
        &[("scope", scope)],
    );
    proxy_to_capability_core(&s, &c, &bearer, &path, "DELETE", None).await
}

/// Takes the revision a policy write was made against out of its body. capability-core reads it from
/// `If-Match`, and this proxy forwards only the session's headers, so the SPA names it in the body. A body
/// without one forwards with no precondition, which capability-core refuses with 428.
fn take_policy_revision(body: &mut Value) -> Result<Option<String>, HttpJsonError> {
    let Some(raw) = body
        .as_object_mut()
        .and_then(|object| object.remove("revision"))
    else {
        return Ok(None);
    };
    match raw.as_u64() {
        Some(revision) if i64::try_from(revision).is_ok() => Ok(Some(revision.to_string())),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "revision must be a non-negative integer"})),
        )),
    }
}

/// `GET /v1/org/agent-policy` — the org's agent policy in force. Any member of the org reads it.
async fn get_org_agent_policy_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, "org-agent-policy", "GET", None).await
}

/// `PUT /v1/org/agent-policy` — an org admin replaces the policy at the revision the body names. The rest
/// of the body is forwarded as sent: capability-core decodes it strictly and decides the admin claim, the
/// write scope and the ZDR posture from the signed principal.
async fn set_org_agent_policy_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&c)?;
    let revision = take_policy_revision(&mut body)?;
    proxy_to_capability_core_with_precondition(
        &s,
        &c,
        &bearer,
        "org-agent-policy",
        "PUT",
        Some(&body),
        revision.as_deref(),
    )
    .await
}

/// `GET /v1/org/agent-policy/effective` — each setting with the value in force for the caller's org. Only
/// the org layer exists yet, and capability-core accepts only `user=me`, so the read always asks for that.
async fn get_org_agent_policy_effective_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    let path = capability_query_path("org-agent-policy/effective", &[("user", "me")]);
    proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await
}

/// `GET /v1/org/hooks` — the org's active hook rules and the policy revision they are at.
async fn list_org_hooks_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, "org-hooks", "GET", None).await
}

/// `POST /v1/org/hooks` — an org admin adds a rule. The body is forwarded as sent, so a field that
/// capability-core does not know, the org id included, is refused rather than dropped.
async fn create_org_hook_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&c)?;
    proxy_to_capability_core(&s, &c, &bearer, "org-hooks", "POST", Some(&body)).await
}

/// `PUT /v1/org/hooks/:hook_id` — an org admin replaces the content of an active rule.
async fn update_org_hook_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(hook_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&c)?;
    require_registry_identifier(&hook_id, "hook_id")?;
    let path = format!("org-hooks/{hook_id}");
    proxy_to_capability_core(&s, &c, &bearer, &path, "PUT", Some(&body)).await
}

/// `DELETE /v1/org/hooks/:hook_id` — an org admin retires an active rule; the row stays as history.
async fn delete_org_hook_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(hook_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&c)?;
    require_registry_identifier(&hook_id, "hook_id")?;
    let path = format!("org-hooks/{hook_id}");
    proxy_to_capability_core(&s, &c, &bearer, &path, "DELETE", None).await
}

/// Optional `space_ref` narrows the org's schedules to one room's.
///
/// Forwarded verbatim; capability-core applies it as a filter over rows that
/// already carry the column, so this can only ever return a subset of what the
/// unfiltered call returns. The org still comes from the verified claims.
#[derive(Debug, Default, Deserialize)]
struct CronListQuery {
    space_ref: Option<String>,
    /// `self` narrows the list to the caller's own schedules (EP-6-S2). Any
    /// other value is dropped, never forwarded.
    creator: Option<String>,
}

/// Query for both process routes. `space_ref` and the two decision fields
/// travel together: sandbox-manager verifies the decision against the Space
/// the CALLER names, never against whatever the row says, so omitting the
/// Space would leave the verifier comparing a token to itself.
#[derive(Debug, Deserialize)]
struct ProcessesQuery {
    space_ref: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
    include_finished: Option<bool>,
    limit: Option<i32>,
    after: Option<String>,
    after_seq: Option<i64>,
    max_bytes: Option<i64>,
}

/// `GET /v1/processes?space_ref=&space_read_decision_ref=&space_read_decision_token=`
/// — a Space's background processes (S4.2 §7).
///
/// This is the FIRST caller of `state.sandbox_client`, which has been
/// constructed and unused since `state.rs:122`; S3.3's §3.5 amendment flagged
/// it and S4.2 is what finally gives it a job.
///
/// The bearer is the user-bound `aud=sandbox-manager` credential the V3
/// gateway mints per `/work` request — not the chat path's `sandbox_token`
/// helper, which only fires inside a Control-injected turn, and the Work tab
/// is not a turn. It is forwarded upstream as plain `authorization`, exactly
/// as `list_runs` forwards its session bearer.
async fn list_processes(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedSandboxBearer,
    Query(query): Query<ProcessesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let space_ref = require_url_safe_space_ref(query.space_ref.as_deref())?;
    let response = state
        .sandbox_client
        .clone()
        .list_processes(authenticated_sandbox_request(
            ListProcessesRequest {
                space_id: space_ref,
                include_terminal: query.include_finished.unwrap_or(false),
                limit: query.limit.unwrap_or(0),
                after_process_id: query.after.unwrap_or_default(),
                space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
                space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "processes": response.processes.iter().map(process_value).collect::<Vec<_>>(),
        "has_more": response.has_more,
    })))
}

/// `GET /v1/processes/{id}/output?space_ref=&after_seq=&max_bytes=` — one page
/// of a process's durable output.
///
/// `gap_before` is forwarded rather than smoothed over: output that was
/// produced and then trimmed by retention is not the same as output that never
/// existed, and a reader summarising a log needs to know it is looking at a
/// hole.
async fn read_process_output(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedSandboxBearer,
    Path(process_id): Path<String>,
    Query(query): Query<ProcessesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let space_ref = require_url_safe_space_ref(query.space_ref.as_deref())?;
    let response = state
        .sandbox_client
        .clone()
        .read_process_output(authenticated_sandbox_request(
            ReadProcessOutputRequest {
                process_id,
                after_seq: query.after_seq.unwrap_or(0),
                max_bytes: query.max_bytes.unwrap_or(0),
                space_id: space_ref,
                space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
                space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "state": process_state_name(response.state),
        "exit_code": response.exit_code,
        "chunks": response.chunks.iter().map(|chunk| json!({
            "seq": chunk.seq,
            "stream": process_stream_name(chunk.stream),
            "text": String::from_utf8_lossy(&chunk.content),
            "ends_with_newline": chunk.ends_with_newline,
        })).collect::<Vec<_>>(),
        "next_cursor": response.next_cursor,
        "gap_before": response.gap_before,
        "retained_from_seq": response.retained_from_seq,
    })))
}

/// Lifetime of the thread lease a workspace read acquires. Execution-core's own
/// thread lease uses the same hour, and every acquire slides it, so a read never
/// shortens a lease a live run holds.
const THREAD_WORKSPACE_LEASE_TTL_SECS: i64 = 3_600;

/// `GET /v1/threads/{thread_id}/workspace`: the files one thread keeps across runs
/// (WS22-S5, DEC-23), each with its content hash and size, never its content.
///
/// The workspace is read through a lease, so this acquires the caller's thread
/// lease first. Acquire is idempotent for the same user and thread, and it carries
/// the caller's ZDR posture, so a ZDR caller is refused before any lease row is
/// written. The bearer is the user-bound `aud=sandbox-manager` credential the BFF
/// attaches per request, and sandbox-manager binds every row to that user.
async fn get_thread_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSandboxBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let thread_id = require_url_safe_thread_id(&thread_id)?;
    let zdr = crate::zdr_posture::zdr_or_refuse(&claims, &crate::zdr_posture::Route::plain(false))?;
    let mut sandbox = state.sandbox_client.clone();
    let lease = sandbox
        .acquire_lease(authenticated_sandbox_request(
            AcquireLeaseRequest {
                org_id: claims.org_id.clone(),
                scope_type: "thread".to_owned(),
                scope_id: thread_id.clone(),
                ttl: Some(prost_types::Duration {
                    seconds: THREAD_WORKSPACE_LEASE_TTL_SECS,
                    nanos: 0,
                }),
                zdr,
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let response = sandbox
        .get_thread_workspace(authenticated_sandbox_request(
            GetThreadWorkspaceRequest {
                lease_id: lease.lease_id,
                backend_id: lease.backend_id,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "thread_id": thread_id,
        "revision": response.revision,
        "files": response.entries.iter().map(|entry| json!({
            "path": entry.path,
            "content_hash": entry.content_hash,
            "size_bytes": entry.size_bytes,
        })).collect::<Vec<_>>(),
    })))
}

/// `DELETE /v1/threads/{thread_id}/workspace`: the user's reset of one thread's
/// files (WS22-S5). Sandbox-manager removes the thread's rows and then its blobs,
/// and only the caller's own thread. A thread that holds nothing is a successful
/// no-op, so a retry finishes an erase whose blob removal failed the first time.
/// Erasure is not refused under ZDR, because a ZDR thread holds nothing to erase.
async fn delete_thread_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSandboxBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let thread_id = require_url_safe_thread_id(&thread_id)?;
    let response = state
        .sandbox_client
        .clone()
        .delete_thread_workspace(authenticated_sandbox_request(
            DeleteThreadWorkspaceRequest {
                thread_id: thread_id.clone(),
                org_id: claims.org_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();

    Ok(Json(json!({
        "thread_id": thread_id,
        "files_deleted": response.files_deleted,
        "object_prefix": response.object_prefix,
    })))
}

/// A thread id the workspace path accepts: the plain key segment the object store
/// keys blobs by (letters, digits, `_` and `-`, at most 128 bytes). It is refused
/// here, before any RPC, so a crafted id never reaches a blob prefix.
fn require_url_safe_thread_id(raw: &str) -> Result<String, HttpJsonError> {
    let thread_id = raw.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "thread_id is required" })),
        ));
    }
    if thread_id.len() > 128
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "thread_id must be a plain key segment" })),
        ));
    }
    Ok(thread_id.to_owned())
}

#[cfg(test)]
mod thread_workspace_route_tests {
    use super::*;

    #[test]
    fn a_thread_id_is_a_plain_key_segment_or_it_is_refused_before_any_rpc() {
        assert_eq!(
            require_url_safe_thread_id(" thr_01J9-abc ").unwrap(),
            "thr_01J9-abc"
        );
        let long = "a".repeat(129);
        for bad in [
            "",
            "   ",
            "thread/1",
            "../thr",
            "thr.1",
            "thr 1",
            "thr%2F1",
            long.as_str(),
        ] {
            let (status, _) = require_url_safe_thread_id(bad).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
        }
    }
}

/// The same URL-safety rule `/v1/cron` applies, and for the same reason: this
/// crate has no URL-encoder, and half of one is worse than none — a
/// `space_ref` carrying `&` or `=` would silently become different parameters.
/// Space refs are Convex ids, so refusing anything outside the URL-safe set
/// costs nothing real.
fn require_url_safe_space_ref(raw: Option<&str>) -> Result<String, HttpJsonError> {
    let space_ref = raw.unwrap_or_default().trim();
    if space_ref.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "space_ref is required" })),
        ));
    }
    if !space_ref
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "space_ref must be URL-safe" })),
        ));
    }
    Ok(space_ref.to_owned())
}

fn authenticated_sandbox_request<T>(
    value: T,
    bearer: &VerifiedSandboxBearer,
) -> Result<tonic::Request<T>, HttpJsonError> {
    let mut request = tonic::Request::new(value);
    let authorization = format!("Bearer {}", bearer.as_str()).parse().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "verified sandbox credential is not forwardable"})),
        )
    })?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// One process, as the Work tab sees it. The command is the REDACTED argv the
/// registry stored — the host scrubs before it registers, so no unredacted
/// form exists here to leak by mistake.
fn process_value(process: &MpProcess) -> Value {
    json!({
        "process_id": process.process_id,
        "space_ref": process.space_id,
        "state": process_state_name(process.state),
        "exit_code": process.exit_code,
        "end_reason": process.end_reason,
        "command": process.command.as_ref().map(|command| {
            let mut parts = vec![command.program.clone()];
            parts.extend(command.args.iter().cloned());
            parts.join(" ")
        }),
        "started_at": process.started_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "ended_at": process.ended_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "expires_at": process.expires_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "next_seq": process.next_seq,
    })
}

fn process_state_name(state: i32) -> &'static str {
    match MpProcessState::try_from(state) {
        Ok(MpProcessState::Starting) => "STARTING",
        Ok(MpProcessState::Running) => "RUNNING",
        Ok(MpProcessState::Exited) => "EXITED",
        Ok(MpProcessState::Killed) => "KILLED",
        Ok(MpProcessState::Expired) => "EXPIRED",
        Ok(MpProcessState::Lost) => "LOST",
        _ => "UNSPECIFIED",
    }
}

fn process_stream_name(stream: i32) -> &'static str {
    match MpProcessStream::try_from(stream) {
        Ok(MpProcessStream::Stdout) => "stdout",
        Ok(MpProcessStream::Stderr) => "stderr",
        _ => "unspecified",
    }
}

async fn list_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(query): Query<CronListQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let space_ref = query.space_ref.unwrap_or_default();
    let space_ref = space_ref.trim();
    // This crate has no URL-encoder, and half of one is worse than none: a
    // `space_ref` carrying `&` or `=` would silently become different query
    // parameters. Space refs are Convex ids, so refusing anything outside the
    // URL-safe set costs nothing real and cannot mangle a request into a
    // different one.
    if !space_ref.is_empty()
        && !space_ref
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "space_ref must be URL-safe" })),
        ));
    }
    let path = cron_list_path(&c.org_id, space_ref, query.creator.as_deref());
    proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await
}

/// The upstream list path. Only the literal `creator=self` is forwarded:
/// capability-core compares it to the verified actor, so it can only narrow.
fn cron_list_path(org_id: &str, space_ref: &str, creator: Option<&str>) -> String {
    let mut path = if space_ref.is_empty() {
        format!("cron?org_id={org_id}")
    } else {
        format!("cron?org_id={org_id}&space_ref={space_ref}")
    };
    if creator == Some("self") {
        path.push_str("&creator=self");
    }
    path
}
async fn create_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    // A template naming a thread goal is a goal-bound schedule (EP-7 S6): admitted first, since
    // session-core binds it to the goal at insert.
    if crate::goal_unattended::names_goal(&b) {
        crate::goal_unattended::admit(&c, &b)?;
    }
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, &c, &bearer, "cron", "POST", Some(&b)).await
}
async fn get_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("cron/{id}"), "GET", None).await
}
async fn patch_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("cron/{id}"), "PATCH", Some(&b)).await
}
async fn delete_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("cron/{id}"), "DELETE", None).await
}

#[derive(Debug, Default, Deserialize)]
struct CronFiresQuery {
    limit: Option<u32>,
}

/// The recent fires of one schedule. `limit` is typed, so only a number ever
/// reaches the upstream path; capability-core applies the bounds.
async fn list_cron_fires_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Query(query): Query<CronFiresQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &cron_fires_path(&id, query.limit),
        "GET",
        None,
    )
    .await
}

fn cron_fires_path(id: &str, limit: Option<u32>) -> String {
    match limit {
        Some(limit) => format!("cron/{id}/fires?limit={limit}"),
        None => format!("cron/{id}/fires"),
    }
}

#[cfg(test)]
mod cron_fires_path_tests {
    use super::cron_fires_path;

    #[test]
    fn fires_path_forwards_only_a_numeric_limit() {
        assert_eq!(
            cron_fires_path("cron_1", Some(5)),
            "cron/cron_1/fires?limit=5"
        );
        assert_eq!(cron_fires_path("cron_1", None), "cron/cron_1/fires");
    }
}

/// Run now (EP-6-S2): one manual fire of a schedule. Only the caller's
/// `request_id` is forwarded; capability-core checks the creator, asks Control
/// for a fresh fire decision and answers 404 while `CRON_RUN_NOW_ENABLED` is off.
async fn run_cron_now_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    let path = cron_run_path(&id)?;
    let body = json!({ "request_id": b.get("request_id").cloned().unwrap_or(Value::Null) });
    proxy_to_capability_core(&s, &c, &bearer, &path, "POST", Some(&body)).await
}

/// The upstream run path. The id is decoded from the request path, so an
/// encoded `/` or `?` would otherwise address a different upstream route.
fn cron_run_path(id: &str) -> Result<String, HttpJsonError> {
    let url_safe = !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'));
    if !url_safe {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {
                "code": "invalid_cron_id",
                "message": "cron id must be URL-safe",
                "details": {}
            }})),
        ));
    }
    Ok(format!("cron/{id}/run"))
}

#[cfg(test)]
mod cron_run_now_path_tests {
    use super::{cron_list_path, cron_run_path};

    #[test]
    fn cron_run_path_forwards_only_the_id() {
        assert_eq!(cron_run_path("cron_1").unwrap(), "cron/cron_1/run");
        for bad in ["", ".", "..", "cron_1/fires", "cron_1?creator=x", "cron 1", "cron_1#x"] {
            let (status, _) = cron_run_path(bad).unwrap_err();
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{bad:?}");
        }
    }

    #[test]
    fn cron_list_forwards_only_creator_self() {
        assert_eq!(cron_list_path("org-1", "", None), "cron?org_id=org-1");
        assert_eq!(
            cron_list_path("org-1", "", Some("self")),
            "cron?org_id=org-1&creator=self"
        );
        assert_eq!(
            cron_list_path("org-1", "space-1", Some("self")),
            "cron?org_id=org-1&space_ref=space-1&creator=self"
        );
        for other in ["someone-else", "SELF", "self&org_id=org-2", ""] {
            assert_eq!(
                cron_list_path("org-1", "space-1", Some(other)),
                "cron?org_id=org-1&space_ref=space-1"
            );
        }
    }
}

async fn list_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let scope = q.get("scope").map(String::as_str).unwrap_or_default();
    let path = capability_query_path("memory", &[("org_id", c.org_id.as_str()), ("scope", scope)]);
    proxy_to_capability_core(&s, &c, &bearer, &path, "GET", None).await
}
async fn create_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, &c, &bearer, "memory", "POST", Some(&b)).await
}
async fn get_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("memory/{id}"), "GET", None).await
}
async fn patch_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("memory/{id}"), "PATCH", Some(&b)).await
}
async fn delete_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("memory/{id}"), "DELETE", None).await
}

async fn list_skills_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("skills?org_id={}", c.org_id),
        "GET",
        None,
    )
    .await
}
async fn create_skill_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, &c, &bearer, "skills", "POST", Some(&b)).await
}
async fn get_skill_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("skills/{id}"), "GET", None).await
}
async fn patch_skill_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("skills/{id}"), "PATCH", Some(&b)).await
}
async fn delete_skill_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("skills/{id}"), "DELETE", None).await
}

/// `GET /v1/runs/:run_id/watchers` — the caller's own watch status on this
/// run. capability-core derives org_id/user_id from the forwarded
/// capability bearer alone; no query/body identity to inject here.
async fn get_run_watchers_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("runs/{run_id}/watchers"),
        "GET",
        None,
    )
    .await
}

/// `POST /v1/runs/:run_id/watchers` — register "notify me" for this run.
/// No request body: the recipient is always the calling user, never
/// caller-supplied.
async fn create_run_watcher_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("runs/{run_id}/watchers"),
        "POST",
        None,
    )
    .await
}

/// `DELETE /v1/runs/:run_id/watchers` — cancel the caller's own watch.
async fn delete_run_watcher_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("runs/{run_id}/watchers"),
        "DELETE",
        None,
    )
    .await
}

async fn list_plugins_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("plugins?org_id={}", c.org_id),
        "GET",
        None,
    )
    .await
}
async fn create_plugin_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, &c, &bearer, "plugins", "POST", Some(&b)).await
}
async fn get_plugin_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("plugins/{id}"), "GET", None).await
}
async fn patch_plugin_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("plugins/{id}"), "PATCH", Some(&b)).await
}
async fn delete_plugin_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &c, &bearer, &format!("plugins/{id}"), "DELETE", None).await
}

pub(crate) fn grpc_status_to_http(error: &tonic::Status) -> HttpJsonError {
    let status = match error.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        tonic::Code::Unavailable => StatusCode::BAD_GATEWAY,
        // A throttle or an exhausted quota: the caller may retry later, which
        // a bare 500 did not say.
        tonic::Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        other => {
            // F-11 (chat-parity audit §3.4, §3.10): every code not explicitly
            // mapped above used to fall silently into a bare 500, which is how
            // an unimplemented upstream RPC (F-15 — orchestrator-core's
            // `GetRunProofBundle` returning `codes.Unimplemented`) became a
            // "mystery 500" that took a cross-service log search to diagnose.
            // Logging the real code and message here means the *next* one is
            // diagnosable from model-gateway's own logs alone.
            tracing::warn!(
                grpc_code = ?other,
                grpc_message = %error.message(),
                "grpc_status_to_http: unmapped gRPC code, returning 500"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };

    // A typed ZDR refusal (inference-core's kind trailer, or an execution-core
    // one) keeps its catalogue code, action and feature. Every other status keeps
    // the flat `{"error": message}` body it always had.
    if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(error) {
        return (status, Json(refusal.json_body()));
    }
    // A model-entitlement refusal (a different kind trailer, on PERMISSION_DENIED
    // or UNAVAILABLE) keeps its catalogue code and takes the catalogue's status:
    // 503 for an unreadable policy, where UNAVAILABLE alone would read 502.
    if let Some(refusal) = mp_contracts::model_entitlement::Refusal::from_status(error) {
        return crate::model_access::http(&refusal);
    }
    (status, Json(json!({ "error": error.message() })))
}

#[cfg(test)]
mod grpc_status_to_http_tests {
    use super::*;

    #[test]
    fn explicitly_mapped_codes_keep_their_specific_status() {
        let cases = [
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST),
            (tonic::Code::NotFound, StatusCode::NOT_FOUND),
            (
                tonic::Code::FailedPrecondition,
                StatusCode::PRECONDITION_FAILED,
            ),
            (tonic::Code::Unauthenticated, StatusCode::UNAUTHORIZED),
            (tonic::Code::PermissionDenied, StatusCode::FORBIDDEN),
            (tonic::Code::DeadlineExceeded, StatusCode::GATEWAY_TIMEOUT),
            (tonic::Code::Unavailable, StatusCode::BAD_GATEWAY),
            (tonic::Code::ResourceExhausted, StatusCode::TOO_MANY_REQUESTS),
        ];
        for (code, expected) in cases {
            let status = tonic::Status::new(code, "detail");
            let (http_status, _) = grpc_status_to_http(&status);
            assert_eq!(http_status, expected, "{code:?} should map to {expected}");
        }
    }

    /// F-11 / F-15 regression: an unmapped code — `Unimplemented` is exactly
    /// what orchestrator-core returns for the never-implemented
    /// `GetRunProofBundle` RPC — must still degrade to 500 (unchanged client
    /// contract) but is now the case the added `tracing::warn!` covers, so a
    /// future one is diagnosable from model-gateway's own logs. This test
    /// cannot assert on the log line itself (no tracing subscriber is
    /// installed in unit tests), so it locks the still-visible half of the
    /// contract: the status code and the upstream message reaching the client.
    #[test]
    fn unmapped_code_falls_through_to_500_and_preserves_the_message() {
        let status = tonic::Status::new(tonic::Code::Unimplemented, "GetRunProofBundle unimplemented");
        let (http_status, Json(body)) = grpc_status_to_http(&status);
        assert_eq!(http_status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "GetRunProofBundle unimplemented");
    }

    #[test]
    fn internal_code_also_falls_through_to_500() {
        let status = tonic::Status::new(tonic::Code::Internal, "boom");
        let (http_status, _) = grpc_status_to_http(&status);
        assert_eq!(http_status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}

fn not_found(message: &str) -> HttpJsonError {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

/// Map a session-core gRPC error from the chat thread history surfaces to an
/// HTTP error.
///
/// A stale or deleted `thread_id` surfaces as `tonic::Code::NotFound` from
/// session-core (`authorize_thread_owner` returns `not_found` when the row is
/// gone). Blanket-mapping every gRPC error to `502 Bad Gateway` made that
/// ordinary "the thread no longer exists" state look like an upstream outage,
/// so verevonv3 chat could not tell a gone thread apart from a broken
/// model-gateway and could not self-heal by dropping the thread. Map `NotFound`
/// to `404` with a stable `thread_not_found` code so the SPA can evict the
/// thread from its list, and `PermissionDenied` (cross-org / cross-user read)
/// to `403`. Invalid input and ZDR policy denials preserve their client-visible
/// semantics. Every other code — including transport failures (`Unavailable`,
/// `Internal`, …) — stays `502`, preserving the prior "upstream is unhappy"
/// default for these routes.
pub(crate) fn session_thread_error(context: &str, error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::InvalidArgument => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_thread_request",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::FailedPrecondition => (
            StatusCode::PRECONDITION_FAILED,
            Json(json!({
                "error": {
                    "code": "thread_persistence_not_permitted",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "thread_not_found",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::PermissionDenied => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": {
                    "code": "forbidden",
                    "message": error.message(),
                }
            })),
        ),
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("{context}: {}", error.message()),
            })),
        ),
    }
}

/// Map a session-core gRPC error from the memory-management surfaces
/// (`list_memory`, `delete_memory`) to an HTTP error. Mirrors
/// `session_thread_error`'s reasoning: `NotFound` is an ordinary "no such
/// memory" state (already deleted, or a stale id), not an upstream outage.
fn session_memory_error(context: &str, error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "memory_not_found",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::PermissionDenied => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": {
                    "code": "forbidden",
                    "message": error.message(),
                }
            })),
        ),
        // The memory gate's typed refusals (`memory_not_enabled:<reason>`) and the
        // ZDR refusal of a state change: the caller can act on them, so they are
        // not an upstream outage.
        tonic::Code::FailedPrecondition => {
            let (code, reason) = error
                .message()
                .split_once(':')
                .unwrap_or((error.message(), ""));
            (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": {
                        "code": code,
                        "message": error.message(),
                        "details": { "reason": reason },
                    }
                })),
            )
        }
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("{context}: {}", error.message()),
            })),
        ),
    }
}

/// Map a session-core gRPC error from the learning surfaces to an HTTP error.
/// Unlike `session_memory_error`, `NotFound` has its own code: a proposal or ledger
/// entry that is not the owner's, or is gone. A failed precondition carries the
/// refusal's code as its whole message (`not_proposed`, `already_undone`,
/// `entry_superseded`, `memory_undo_via_delete`, ...), which the owner's surface can
/// show as it is.
fn session_learning_error(context: &str, error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::NotFound => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "learning_not_found",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::PermissionDenied => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": {
                    "code": "forbidden",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::InvalidArgument => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_argument",
                    "message": error.message(),
                }
            })),
        ),
        tonic::Code::FailedPrecondition => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": {
                    "code": error.message(),
                    "message": error.message(),
                }
            })),
        ),
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("{context}: {}", error.message()),
            })),
        ),
    }
}

/// One ledger entry for the owner's surface. The snapshots stay the JSON text
/// session-core stored, which the SPA parses. The actor's id is left out: the owner's
/// surface needs only whether a person or Verevon made the change.
fn learning_entry_json(entry: &mp_contracts::model_plane::v1::LearningLedgerEntry) -> Value {
    json!({
        "entry_id": entry.entry_id,
        "kind": entry.kind,
        "action": entry.action,
        "skill_id": entry.skill_id,
        "skill_name": entry.skill_name,
        "revision": entry.revision,
        "actor_kind": entry.actor_kind,
        "thread_id": entry.thread_id,
        "reason": entry.reason,
        "before_json": entry.before_json,
        "after_json": entry.after_json,
        "undone": entry.undone,
        "undoable": entry.undoable,
        "created_at": entry.created_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
    })
}

/// Only the two literal words; anything else is refused rather than defaulted,
/// because a typo must never accept a proposal the owner did not mean to.
fn parse_skill_decision(word: &str) -> Option<mp_contracts::model_plane::v1::SkillDecision> {
    use mp_contracts::model_plane::v1::SkillDecision;
    match word {
        "accept" => Some(SkillDecision::Accept),
        "reject" => Some(SkillDecision::Reject),
        _ => None,
    }
}

/// Serialize a `RunDetail` for the runs-history UI. `Snake_case` wire keys match
/// the rest of the orchestration surface; the SPA client normalizes to
/// camelCase. Timestamps are emitted as RFC3339 strings (null when unset) and
/// the arbitrary `metadata` Struct is flattened to plain JSON.
fn run_detail_value(run: &RunDetail) -> Value {
    let mut value = json!({
        "run_id": run.run_id,
        "thread_id": run.thread_id,
        "parent_run_id": empty_to_null(&run.parent_run_id),
        "agent_id": run.agent_id,
        "status": run.status,
        "mode": run.mode,
        "goal": run.goal,
        "final_output": empty_to_null(&run.final_output),
        "error": empty_to_null(&run.error),
        "checkpoint_index": run.checkpoint_index,
        "steps_completed": run.steps_completed,
        "input_tokens": run.input_tokens,
        "output_tokens": run.output_tokens,
        "created_at": run.created_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "updated_at": run.updated_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "metadata": run.metadata.as_ref().map_or(Value::Null, prost_struct_to_json),
    });
    // Set by Session Core only when the run was read through a Space read decision (WS09-S4);
    // absent otherwise, so an owner-path projection is byte-for-byte what it was.
    if !run.owner_subject_id.is_empty() {
        value["owner_subject_id"] = json!(run.owner_subject_id);
    }
    value
}

#[cfg(test)]
mod run_detail_value_tests {
    use super::*;

    /// `RunDetail` carries two fields about a paused run that stay between the services:
    /// `resumable` (the executor's hint that a fold can be resumed in place) and
    /// `park_reason`. The run projection the browser reads is an allowlist, so neither
    /// reaches it, in either casing, and nor does the value of either.
    #[test]
    fn run_detail_value_never_carries_resumable_or_park_reason() {
        let run = RunDetail {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            status: "awaiting_approval".to_owned(),
            goal: "a goal".to_owned(),
            resumable: true,
            park_reason: "a_closed_park_reason".to_owned(),
            ..Default::default()
        };
        let value = run_detail_value(&run);
        let object = value.as_object().expect("a run is a JSON object");
        for key in ["resumable", "park_reason", "parkReason"] {
            assert!(!object.contains_key(key), "{key} reached the browser projection: {value}");
        }
        let text = value.to_string();
        assert!(!text.contains("a_closed_park_reason"), "{text}");
        assert!(!text.contains("resumable"), "{text}");
        // The projection still says what it always said.
        assert_eq!(value["run_id"], "run-1");
        assert_eq!(value["status"], "awaiting_approval");
    }
}

/// Convert a prost `Timestamp` to an RFC3339 JSON string. An out-of-range value
/// (it cannot occur for stored Postgres timestamps) falls back to null.
fn prost_ts_to_rfc3339(ts: &prost_types::Timestamp) -> Value {
    let nanos = u32::try_from(ts.nanos).unwrap_or(0);
    match chrono::DateTime::from_timestamp(ts.seconds, nanos) {
        Some(dt) => Value::String(dt.to_rfc3339()),
        None => Value::Null,
    }
}

/// The RFC3339 time of a transcript turn, or `None` when session-core sent none (or one that
/// cannot be represented), so the field is left off rather than invented.
fn message_created_at(ts: Option<&prost_types::Timestamp>) -> Option<String> {
    match prost_ts_to_rfc3339(ts?) {
        Value::String(rfc3339) => Some(rfc3339),
        _ => None,
    }
}

/// Flatten a prost `Struct` into plain JSON for the wire envelope.
fn prost_struct_to_json(s: &prost_types::Struct) -> Value {
    let map = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
        .collect();
    Value::Object(map)
}

fn prost_value_to_json(v: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_)) | None => Value::Null,
        Some(Kind::NumberValue(n)) => {
            serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
        }
        Some(Kind::StringValue(s)) => Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => Value::Bool(*b),
        Some(Kind::StructValue(inner)) => prost_struct_to_json(inner),
        Some(Kind::ListValue(list)) => {
            Value::Array(list.values.iter().map(prost_value_to_json).collect())
        }
    }
}

fn plan_value(plan: &Plan) -> Value {
    json!({
        "id": plan.id,
        "run_id": plan.run_id,
        "thread_id": plan.thread_id,
        "author": plan.author,
        "state": enum_name(PlanState::try_from(plan.state).ok().as_ref()),
        "summary": plan.summary,
        "steps": plan.steps.iter().map(plan_step_value).collect::<Vec<_>>(),
        "supersedes": empty_to_null(&plan.supersedes),
    })
}

fn plan_step_value(step: &PlanStep) -> Value {
    json!({
        "id": step.id,
        "title": step.title,
        "operation": empty_to_null(&step.operation),
        "state": enum_name(PlanStepState::try_from(step.state).ok().as_ref()),
        // Why a step ended the way it did. Without this the plan panel could
        // only repeat the step's operation slug, so a run that failed every
        // step on a policy denial looked identical to one that succeeded.
        "detail": empty_to_null(&step.detail),
    })
}

fn todo_value(todo: &Todo) -> Value {
    json!({
        "id": todo.id,
        "thread_id": todo.thread_id,
        "run_id": empty_to_null(&todo.run_id),
        "assignee": todo.assignee,
        "title": todo.title,
        "description": empty_to_null(&todo.description),
        "state": enum_name(TodoState::try_from(todo.state).ok().as_ref()),
        "priority": enum_name(TodoPriority::try_from(todo.priority).ok().as_ref()),
        "blocked_by": todo.blocked_by,
    })
}

pub(crate) fn approval_value(approval: &Approval) -> Value {
    let mut value = json!({
        "id": approval.id,
        "run_id": approval.run_id,
        "step_id": empty_to_null(&approval.step_id),
        "kind": enum_name(ApprovalKind::try_from(approval.kind).ok().as_ref()),
        "state": enum_name(ApprovalState::try_from(approval.state).ok().as_ref()),
        "requested_of": approval.requested_of,
        "decided_by": empty_to_null(&approval.decided_by),
        "decision_reason": empty_to_null(&approval.decision_reason),
        // Why the pause was raised (build plan WS13 S2); null when the creator stated none.
        "cause": approval.cause.as_ref().map_or(Value::Null, approval_cause_value),
        // What the card says it is for (build plan WS10 S1): allowlisted by the creator and
        // re-validated by Session Core, never raw arguments.
        "tool_key": empty_to_null(&approval.tool_key),
        "action_label": empty_to_null(&approval.action_label),
        // What approving does with the paused work, stated by Session Core from its own rows:
        // `action_continuation`, `live_poll` or `none`. Null on an approval created before the
        // card context existed, so a surface says nothing rather than guessing.
        "resume_class": card_resume_class(approval).map_or(Value::Null, Value::String),
        // Where the approved action stands, derived from the delivery rows; null until granted.
        "continuation": continuation_value(approval),
        // Who is asked to decide it and who raised it (build plan WS10 S6): identifiers and kinds
        // only. The requester is the subject the approved action runs as.
        "requester_subject_id": empty_to_null(&approval.requester_subject_id),
        "addressee": approval.addressee.as_ref().map(|a| json!({
            "kind": match a.kind {2 => "named", 3 => "any_editor", 4 => "any_manager", _ => "requester"},
            "subject_id": empty_to_null(&a.subject_id),
        })),
        "grant_ref": empty_to_null(&approval.grant_ref),
        "turn_origin": empty_to_null(&approval.turn_origin),
    });
    // EP-10-S4: present only for an approval the person edited before granting, so every other
    // approval projects exactly as before.
    if approval.revision > 0 {
        value["revision"] = json!(approval.revision);
    }
    value
}

/// The scopes this process offers a person (build plan WS13 S6): `once` alone while standing scopes
/// are off, so a card never offers a scope the decide route would refuse.
fn offered_scope_options(cause: &ApprovalCause) -> Vec<String> {
    if approval_cause::scope_grants_enabled() {
        cause.scope_options.clone()
    } else {
        vec![SCOPE_ONCE.to_owned()]
    }
}

/// The part of an approval's cause a browser may read. An allowlist: the stored cause also
/// holds the call digest and the action key, which are for the gate and the audit trail, and
/// the approval's `context` (tool arguments) never reaches this projection at all. The code
/// is read through the closed vocabulary so a stored value this build does not know renders
/// as `unclassified_gate`, never as a string the copy has no words for.
fn approval_cause_value(cause: &ApprovalCause) -> Value {
    json!({
        "code": CauseCode::from_wire(&cause.code).as_str(),
        "tool": empty_to_null(&cause.tool),
        "effect_class": empty_to_null(&cause.effect_class),
        "risk_category": empty_to_null(&cause.risk_category),
        "rule_id": empty_to_null(&cause.rule_id),
        "assist_kind": empty_to_null(&cause.assist_kind),
        "assist_origin": empty_to_null(&cause.assist_origin),
        "host": empty_to_null(&cause.host),
        "scope_options": offered_scope_options(cause),
        "step_up_required": step_up_projected(cause, ApprovalStepUpGate::from_env()),
    })
}

#[cfg(test)]
mod approval_step_up_tests {
    use super::*;
    use std::collections::BTreeSet;

    const NOW: i64 = 1_800_000_000;

    fn approval_needing_step_up() -> Approval {
        Approval {
            id: "appr_1".to_owned(),
            cause: Some(ApprovalCause {
                code: "owner_effect_reserved".to_owned(),
                step_up_required: true,
                ..ApprovalCause::default()
            }),
            ..Approval::default()
        }
    }

    fn claim_at(at: i64) -> StepUpClaim {
        StepUpClaim {
            at,
            method: "password".to_owned(),
        }
    }

    #[test]
    fn a_grant_needing_step_up_without_the_claim_is_refused() {
        assert!(step_up_missing(
            ApprovalState::Granted,
            Some(&approval_needing_step_up()),
            None,
            NOW,
        ));
    }

    #[test]
    fn a_denial_never_needs_step_up() {
        assert!(!step_up_missing(
            ApprovalState::Denied,
            Some(&approval_needing_step_up()),
            None,
            NOW,
        ));
    }

    #[test]
    fn a_stale_step_up_claim_is_refused() {
        let approval = approval_needing_step_up();
        assert!(step_up_missing(
            ApprovalState::Granted,
            Some(&approval),
            Some(&claim_at(NOW - STEP_UP_MAX_AGE_SECS - 1)),
            NOW,
        ));
        assert!(!step_up_missing(
            ApprovalState::Granted,
            Some(&approval),
            Some(&claim_at(NOW - STEP_UP_MAX_AGE_SECS)),
            NOW,
        ));
    }

    #[test]
    fn a_claim_dated_a_little_ahead_is_tolerated_and_a_far_future_one_is_not() {
        let approval = approval_needing_step_up();
        assert!(!step_up_missing(
            ApprovalState::Granted,
            Some(&approval),
            Some(&claim_at(NOW + STEP_UP_CLOCK_SKEW_SECS)),
            NOW,
        ));
        assert!(step_up_missing(
            ApprovalState::Granted,
            Some(&approval),
            Some(&claim_at(NOW + STEP_UP_CLOCK_SKEW_SECS + 1)),
            NOW,
        ));
    }

    #[test]
    fn a_grant_whose_cause_asks_no_step_up_needs_none() {
        let plain = Approval {
            cause: Some(ApprovalCause {
                code: "hook_ask".to_owned(),
                ..ApprovalCause::default()
            }),
            ..Approval::default()
        };
        assert!(!step_up_missing(
            ApprovalState::Granted,
            Some(&plain),
            None,
            NOW
        ));
        assert!(!step_up_missing(ApprovalState::Granted, None, None, NOW));
    }

    #[test]
    fn step_up_off_changes_nothing() {
        assert_eq!(ApprovalStepUpGate::from_raw(None), ApprovalStepUpGate::Off);
        for raw in ["off", "true", "ENFORCE", " enforce", "enforced", ""] {
            assert_eq!(
                ApprovalStepUpGate::from_raw(Some(raw)),
                ApprovalStepUpGate::Off,
                "{raw:?} must not enable the gate"
            );
        }
        assert_eq!(
            ApprovalStepUpGate::from_raw(Some("shadow")),
            ApprovalStepUpGate::Shadow
        );
        assert_eq!(
            ApprovalStepUpGate::from_raw(Some("enforce")),
            ApprovalStepUpGate::Enforce
        );
    }

    #[test]
    fn the_step_up_prompt_is_offered_only_while_enforced() {
        let asked = ApprovalCause {
            step_up_required: true,
            ..ApprovalCause::default()
        };
        assert!(!step_up_projected(&asked, ApprovalStepUpGate::Off));
        assert!(!step_up_projected(&asked, ApprovalStepUpGate::Shadow));
        assert!(step_up_projected(&asked, ApprovalStepUpGate::Enforce));
        assert!(!step_up_projected(
            &ApprovalCause::default(),
            ApprovalStepUpGate::Enforce
        ));
    }

    #[test]
    fn a_refused_grant_is_the_typed_step_up_error() {
        let (status, Json(body)) = step_up_required_error();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "step_up_required");
        assert_eq!(body["error"]["details"]["method"], "password");
    }

    /// Every production caller of session-core's `DecideApproval` in the Model Plane, reviewed
    /// (build plan WS13 S5, K3-22). A new caller fails here until it is listed with its reason, so
    /// a path that can record a grant outside the HTTP step-up gate cannot appear unnoticed.
    #[test]
    fn only_the_gateway_calls_decide_approval() {
        let services = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut callers = BTreeSet::new();
        for entry in std::fs::read_dir(&services)
            .expect("the services directory is readable")
            .flatten()
        {
            collect_decide_callers(&entry.path().join("src"), &services, &mut callers);
        }
        let expected: BTreeSet<&str> = [
            // The browser gate records a denial when its run is cancelled and a timeout when its
            // wait budget runs out. Neither is a grant.
            "execution-core/src/browser_events.rs",
            // The chat posture records the interactive user's decision for them (POSTURE_AUTO),
            // a grant that never passes the HTTP gate. Tracked as a depth finding.
            "execution-core/src/runtime_loop/mod.rs",
            // The gateway's durable persist of a decision it has already resolved.
            "model-gateway/src/approvals.rs",
            // The gRPC approval RPCs persist a grant through the same helper, outside the HTTP
            // gate. Tracked as a depth finding.
            "model-gateway/src/grpc.rs",
            // The HTTP decide route, where the step-up gate sits.
            "model-gateway/src/http_routes.rs",
            // Main's Space member decide route (WS10 S6), merged 2026-11-02. It carries a Control
            // decision, not the step-up gate; whether a member grant needs step-up is a depth item.
            "model-gateway/src/space_routes/approvals.rs",
        ]
        .into_iter()
        .collect();
        let found: BTreeSet<&str> = callers.iter().map(String::as_str).collect();
        assert_eq!(found, expected);
    }

    fn collect_decide_callers(
        dir: &std::path::Path,
        services: &std::path::Path,
        callers: &mut BTreeSet<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_decide_callers(&path, services, callers);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap_or_default();
                let calls_decide = source.lines().any(|line| {
                    !line.contains("fn ")
                        && (line.contains(".decide_approval(")
                            || line.contains("persist_approval_decision"))
                });
                if calls_decide {
                    let relative = path.strip_prefix(services).unwrap_or(&path);
                    callers.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
}

#[cfg(test)]
mod approval_rule_tests {
    use super::*;

    /// The digest the fixture tool's definition is pinned to.
    const PINNED_DIGEST: &str = "2b6a1f0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a";

    /// An MCP tool's approval as the server states it once the tool is pinned: the given code, the
    /// declared class, the pinned digest, and the scopes the server computes from them.
    fn mcp_cause(code: &str, effect_class: &str) -> ApprovalCause {
        approval_cause::normalized(ApprovalCause {
            code: code.to_owned(),
            tool: "mcp__fixture__create_note".to_owned(),
            effect_class: effect_class.to_owned(),
            action_key: "mcp__fixture__create_note".to_owned(),
            call_digest: "digest-of-the-exact-call".to_owned(),
            tool_digest: PINNED_DIGEST.to_owned(),
            ..ApprovalCause::default()
        })
        .0
    }

    fn ask(
        cause: Option<&ApprovalCause>,
        scope: &str,
        target: ApprovalState,
    ) -> Result<bool, HttpJsonError> {
        checked_rule_proposal(true, scope, target, cause, true)
    }

    fn refusal_code(result: Result<bool, HttpJsonError>) -> String {
        let (status, Json(body)) = result.expect_err("the ask is refused");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        body["error"]["code"]
            .as_str()
            .expect("a typed refusal code")
            .to_owned()
    }

    #[test]
    fn an_always_choice_before_confirmation_is_a_thread_grant() {
        let cause = mcp_cause("risky_effect_under_ask", "external_write");
        let scope = checked_decision_scope_with(
            "thread",
            ApprovalState::Granted,
            &cause.scope_options,
            true,
        )
        .expect("the thread scope is offered");
        assert_eq!(scope, SCOPE_THREAD);
        assert!(ask(Some(&cause), scope, ApprovalState::Granted).expect("an MCP grant may ask"));
        // The grant is the decision's own: it does not wait for the rule to be confirmed. The rule
        // is proposed beside it and is in force only once the person confirms it.
        let request = user_decision_request(
            "appr-1".to_owned(),
            ApprovalState::Granted,
            "user-1",
            "org-1",
            String::new(),
            scope,
            true,
        );
        assert_eq!(request.scope, ApprovalScope::Thread as i32);
        assert_eq!(request.decision_source, ApprovalDecisionSource::User as i32);
        assert!(
            request.rule_proposed,
            "the proposal travels with the decision"
        );
    }

    #[test]
    fn a_rule_is_proposed_only_from_an_mcp_grant_the_person_may_relax() {
        let grant = mcp_cause("risky_effect_under_ask", "external_write");
        let mut built_in = grant.clone();
        built_in.tool = "write_file".to_owned();
        assert_eq!(
            refusal_code(ask(Some(&built_in), SCOPE_THREAD, ApprovalState::Granted)),
            "rule_not_offered"
        );
        for (code, class) in [
            ("risky_effect_under_ask", "destructive"),
            ("risky_effect_under_ask", "owner_effect"),
            ("risky_effect_under_ask", ""),
            ("hook_ask", "external_write"),
            ("capability_policy_ask", "external_write"),
        ] {
            let cause = mcp_cause(code, class);
            assert_eq!(
                refusal_code(ask(Some(&cause), SCOPE_THREAD, ApprovalState::Granted)),
                "rule_not_offered",
                "{code} with class {class:?}"
            );
        }
        assert_eq!(
            refusal_code(ask(None, SCOPE_THREAD, ApprovalState::Granted)),
            "rule_not_offered"
        );
        let mut unpinned = grant.clone();
        unpinned.tool_digest.clear();
        assert_eq!(
            refusal_code(ask(
                Some(&approval_cause::normalized(unpinned).0),
                SCOPE_THREAD,
                ApprovalState::Granted
            )),
            "rule_not_offered",
            "an MCP tool whose definition is not pinned has no rule"
        );
        assert_eq!(
            refusal_code(ask(Some(&grant), SCOPE_ONCE, ApprovalState::Granted)),
            "rule_requires_thread_scope"
        );
        assert_eq!(
            refusal_code(ask(Some(&grant), SCOPE_THREAD, ApprovalState::Denied)),
            "rule_not_applicable"
        );
        for class in ["read", "sandbox_compute", "context_write", "external_write"] {
            let cause = mcp_cause("consent_to_disclose", class);
            assert!(
                ask(Some(&cause), SCOPE_THREAD, ApprovalState::Granted).expect(class),
                "{class} may be relaxed"
            );
        }
        // With the switch off nothing is asked, however invalid the ask, and nothing is refused.
        assert!(!checked_rule_proposal(
            true,
            SCOPE_ONCE,
            ApprovalState::Denied,
            Some(&built_in),
            false,
        )
        .expect("ignored while the switch is off"));
        assert!(!checked_rule_proposal(
            false,
            SCOPE_THREAD,
            ApprovalState::Granted,
            Some(&grant),
            true,
        )
        .expect("an absent ask is not an ask"));
    }

    #[test]
    fn promote_is_ignored_while_the_switch_is_off_and_only_the_literal_true_asks() {
        assert!(!rule_proposals_from(None));
        assert!(!rule_proposals_from(Some("1")));
        assert!(!rule_proposals_from(Some("yes")));
        assert!(rule_proposals_from(Some(" TRUE ")));
        let literal: DecideApprovalBody = serde_json::from_value(json!({
            "decision": "approve",
            "promote": true,
        }))
        .expect("a boolean promote parses");
        assert_eq!(literal.promote, Value::Bool(true));
        for other in [json!("yes"), json!(1), json!(null)] {
            let body: DecideApprovalBody = serde_json::from_value(json!({
                "decision": "approve",
                "promote": other,
            }))
            .expect("any promote value parses, as an unknown key always did");
            assert_ne!(body.promote, Value::Bool(true));
        }
    }
}

#[cfg(test)]
mod approval_edit_route_tests {
    //! EP-10-S4: the gateway half of approve-with-edits. Every check runs before an upstream call;
    //! the switch is off by default and a body without `edit` is untouched by it.
    use super::*;

    const BASE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn edit_body() -> DecideEditBody {
        DecideEditBody {
            patch: json!({ "subject": "New subject" }),
            base_fingerprint: BASE.to_owned(),
        }
    }

    fn code_of(error: &HttpJsonError) -> (StatusCode, String) {
        (error.0, error.1 .0["error"]["code"].as_str().unwrap_or_default().to_owned())
    }

    #[test]
    fn approval_edit_switch_is_off_by_default_and_strict() {
        assert_eq!(APPROVAL_EDIT_ENV, "APPROVAL_EDIT_ENABLED");
        for value in [None, Some("1"), Some("yes"), Some("on"), Some("")] {
            assert!(!mp_contracts::loop_fold::switch_from(value), "{value:?}");
        }
    }

    #[test]
    fn edit_off_by_default_is_forbidden() {
        let refused =
            checked_decision_edit(Some(&edit_body()), "", false, ApprovalState::Granted, false)
                .expect_err("off");
        assert_eq!(code_of(&refused), (StatusCode::FORBIDDEN, "approval_edit_disabled".to_owned()));
        assert!(refused.1 .0["error"]["details"].is_object(), "details is an object");
    }

    #[test]
    fn a_decision_without_an_edit_is_unchanged_by_the_switch() {
        for enabled in [false, true] {
            assert!(checked_decision_edit(None, "thread", true, ApprovalState::Denied, enabled)
                .expect("no edit")
                .is_none());
        }
    }

    #[test]
    fn edit_with_scope_thread_is_refused() {
        for (scope, promote) in [("thread", false), ("run", false), ("", true)] {
            let refused =
                checked_decision_edit(Some(&edit_body()), scope, promote, ApprovalState::Granted, true)
                    .expect_err("wide scope");
            assert_eq!(
                code_of(&refused),
                (StatusCode::UNPROCESSABLE_ENTITY, "edit_requires_once".to_owned()),
                "{scope} {promote}"
            );
        }
    }

    #[test]
    fn a_rejection_with_an_edit_is_refused() {
        let refused =
            checked_decision_edit(Some(&edit_body()), "", false, ApprovalState::Denied, true)
                .expect_err("rejection");
        assert_eq!(code_of(&refused).1, "edit_requires_grant");
    }

    #[test]
    fn an_edit_is_forwarded_as_its_patch_and_base() {
        let edit = checked_decision_edit(Some(&edit_body()), "once", false, ApprovalState::Granted, true)
            .expect("accepted")
            .expect("an edit");
        assert_eq!(edit.base_fingerprint, BASE);
        let patch: Value = serde_json::from_str(&edit.patch_json).expect("json");
        assert_eq!(patch, json!({ "subject": "New subject" }));
    }

    #[test]
    fn a_malformed_edit_is_refused_before_the_upstream() {
        let mut not_object = edit_body();
        not_object.patch = json!(["subject"]);
        let refused = checked_decision_edit(Some(&not_object), "", false, ApprovalState::Granted, true)
            .expect_err("array patch");
        assert_eq!(code_of(&refused).1, "invalid_patch");

        let mut short = edit_body();
        short.base_fingerprint = "abc".to_owned();
        let refused = checked_decision_edit(Some(&short), "", false, ApprovalState::Granted, true)
            .expect_err("short base");
        assert_eq!(code_of(&refused).1, "invalid_base_fingerprint");

        let mut large = edit_body();
        large.patch = json!({ "bodyText": "x".repeat(MAX_EDIT_PATCH_BYTES) });
        let refused = checked_decision_edit(Some(&large), "", false, ApprovalState::Granted, true)
            .expect_err("large");
        assert_eq!(code_of(&refused), (StatusCode::UNPROCESSABLE_ENTITY, "edit_too_large".to_owned()));
    }

    #[test]
    fn decide_body_parses_an_edit_and_refuses_unknown_edit_keys() {
        let body: DecideApprovalBody = serde_json::from_value(json!({
            "decision": "approve",
            "edit": { "patch": { "to": ["a@example.com"] }, "base_fingerprint": BASE },
        }))
        .expect("an edit parses");
        assert!(body.edit.is_some());
        let plain: DecideApprovalBody =
            serde_json::from_value(json!({ "decision": "approve" })).expect("no edit");
        assert!(plain.edit.is_none());
        assert!(serde_json::from_value::<DecideApprovalBody>(json!({
            "decision": "approve",
            "edit": { "patch": {}, "base_fingerprint": BASE, "connection_id": "conn-2" },
        }))
        .is_err());
    }

    #[test]
    fn approval_changed_is_409() {
        let refused = edit_decision_status_to_http(&tonic::Status::aborted("approval_changed"));
        assert_eq!(code_of(&refused), (StatusCode::CONFLICT, "approval_changed".to_owned()));
        assert!(refused.1 .0["error"]["details"].is_object());
    }

    #[test]
    fn edit_refusals_map_to_typed_statuses() {
        for error in mp_contracts::approval_edit::EditError::ALL {
            let refused =
                edit_decision_status_to_http(&tonic::Status::invalid_argument(error.code()));
            assert_eq!(code_of(&refused), (StatusCode::UNPROCESSABLE_ENTITY, error.code().to_owned()));
        }
        let cases = [
            (tonic::Status::failed_precondition("not_editable"), StatusCode::UNPROCESSABLE_ENTITY),
            (tonic::Status::failed_precondition("revision_limit"), StatusCode::CONFLICT),
            (tonic::Status::failed_precondition("approval_edit_disabled"), StatusCode::FORBIDDEN),
            (tonic::Status::permission_denied("edit_owner_only"), StatusCode::FORBIDDEN),
        ];
        for (status, http) in cases {
            let refused = edit_decision_status_to_http(&status);
            assert_eq!(code_of(&refused), (http, status.message().to_owned()));
        }
        let other = edit_decision_status_to_http(&tonic::Status::not_found("approval not found"));
        assert_eq!(other.0, StatusCode::NOT_FOUND, "everything else keeps the shared mapping");
    }

    #[test]
    fn an_edited_approval_projects_its_revision_and_an_unedited_one_does_not() {
        let mut approval = Approval {
            id: "appr_1".to_owned(),
            ..Approval::default()
        };
        assert!(approval_value(&approval).get("revision").is_none());
        approval.revision = 1;
        assert_eq!(approval_value(&approval)["revision"], 1);
    }
}

#[cfg(test)]
mod approval_value_tests {
    use super::*;

    fn approval_with_cause() -> Approval {
        Approval {
            id: "appr_1".to_owned(),
            run_id: "run_1".to_owned(),
            kind: ApprovalKind::Destructive as i32,
            state: ApprovalState::Requested as i32,
            requested_of: "user_1".to_owned(),
            context: Some(prost_types::Struct {
                fields: [(
                    "tool_input".to_owned(),
                    prost_types::Value {
                        kind: Some(prost_types::value::Kind::StringValue(
                            "secret-argument-text".to_owned(),
                        )),
                    },
                )]
                .into_iter()
                .collect(),
            }),
            cause: Some(ApprovalCause {
                code: "provider_write".to_owned(),
                tool: "execute_provider_action".to_owned(),
                effect_class: "external_write".to_owned(),
                scope_options: vec!["once".to_owned()],
                action_key: "execute_provider_action:message.send".to_owned(),
                call_digest: "digest-of-the-exact-call".to_owned(),
                ..ApprovalCause::default()
            }),
            ..Approval::default()
        }
    }

    #[test]
    fn approval_value_projects_cause_and_never_the_raw_context() {
        let value = approval_value(&approval_with_cause());
        assert_eq!(value["cause"]["code"], "provider_write");
        assert_eq!(value["cause"]["tool"], "execute_provider_action");
        assert_eq!(value["cause"]["effect_class"], "external_write");
        assert_eq!(value["cause"]["scope_options"], json!(["once"]));
        assert_eq!(value["cause"]["step_up_required"], false);
        assert!(value["cause"]["risk_category"].is_null());
        assert!(value["cause"]["rule_id"].is_null());
        let object = value.as_object().expect("an approval is a JSON object");
        assert!(!object.contains_key("context"), "{value}");
        assert!(
            !value.to_string().contains("secret-argument-text"),
            "{value}"
        );
    }

    #[test]
    fn decide_ignores_client_supplied_decision_source() {
        let body: DecideApprovalBody = serde_json::from_value(json!({
            "decision": "approve",
            "reason": "ok",
            "decisionSource": "SYSTEM",
            "decision_source": 5,
        }))
        .expect("the body parses; unknown keys are ignored");
        let request = user_decision_request(
            "appr-1".to_owned(),
            ApprovalState::Granted,
            "user-1",
            "org-1",
            body.reason,
            "once",
            false,
        );
        assert_eq!(request.decision_source, ApprovalDecisionSource::User as i32);
        assert_eq!(request.scope, ApprovalScope::Once as i32);
        assert_eq!(request.decided_by, "user-1");
    }

    #[test]
    fn approval_value_omits_action_key() {
        let text = approval_value(&approval_with_cause()).to_string();
        for leaked in [
            "action_key",
            "actionKey",
            "call_digest",
            "callDigest",
            "message.send",
            "digest-of-the-exact-call",
        ] {
            assert!(
                !text.contains(leaked),
                "{leaked} reached the browser projection: {text}"
            );
        }
    }

    #[test]
    fn a_decision_scope_other_than_once_is_refused_while_grants_are_off() {
        let offered = vec![SCOPE_ONCE.to_owned(), SCOPE_THREAD.to_owned()];
        assert_eq!(
            checked_decision_scope_with("", ApprovalState::Granted, &offered, false).expect("once"),
            SCOPE_ONCE
        );
        for scope in ["thread", "run"] {
            let (status, Json(body)) = checked_decision_scope_with(
                scope,
                ApprovalState::Granted,
                &offered,
                false,
            )
            .expect_err("a standing scope is refused while grants are off");
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{scope}");
            assert_eq!(body["error"]["code"], "scope_not_enabled", "{scope}");
        }
    }

    #[test]
    fn decide_refuses_a_scope_the_server_did_not_offer() {
        let once_only = vec![SCOPE_ONCE.to_owned()];
        let (status, Json(body)) = checked_decision_scope_with(
            "thread",
            ApprovalState::Granted,
            &once_only,
            true,
        )
        .expect_err("a scope the cause does not offer is refused");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "scope_not_offered");

        let offered = vec![
            SCOPE_ONCE.to_owned(),
            SCOPE_THREAD.to_owned(),
            SCOPE_RUN.to_owned(),
        ];
        assert_eq!(
            checked_decision_scope_with("thread", ApprovalState::Granted, &offered, true)
                .expect("offered"),
            SCOPE_THREAD
        );
        assert_eq!(
            checked_decision_scope_with("run", ApprovalState::Granted, &offered, true)
                .expect("offered"),
            SCOPE_RUN
        );
        let (_, Json(body)) = checked_decision_scope_with(
            "forever",
            ApprovalState::Granted,
            &offered,
            true,
        )
        .expect_err("an unknown scope is refused");
        assert_eq!(body["error"]["code"], "scope_not_offered");
        let (_, Json(body)) = checked_decision_scope_with(
            "thread",
            ApprovalState::Denied,
            &offered,
            true,
        )
        .expect_err("a denial carries no scope");
        assert_eq!(body["error"]["code"], "scope_not_applicable");
    }

    #[test]
    fn an_approval_without_a_cause_projects_null_and_an_unknown_code_degrades() {
        assert!(approval_value(&Approval::default())["cause"].is_null());
        let mut approval = approval_with_cause();
        approval.cause.as_mut().expect("cause").code = "a_code_from_a_newer_executor".to_owned();
        assert_eq!(
            approval_value(&approval)["cause"]["code"],
            "unclassified_gate"
        );
    }
}

/// Serialize a Verevon Proof Bundle. Every stage that did not happen stays
/// `null` rather than becoming an empty object — a reader must never mistake
/// "no execution recorded" for "executed with blank fields".
///
/// `effect_class` is derived only from the durable evidence already contained
/// in this bundle. It is intentionally not inferred from free-form model text:
/// an execution receipt proves an attempted effect, a provider receipt proves
/// an external boundary acknowledged it, an approval without execution proves
/// only a proposal, and no approval-chain evidence is read-only for this
/// proof surface.
fn proof_bundle_value(bundle: &RunProofBundle) -> Value {
    json!({
        "bundle_version": bundle.bundle_version,
        "run_id": bundle.run_id,
        "org_id": bundle.org_id,
        "effect_class": proof_effect_class(bundle),
        "generated_at": bundle.generated_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "run": bundle.run.as_ref().map_or(Value::Null, |run| json!({
            "goal": run.goal,
            "agent_id": run.agent_id,
            "status": run.status,
            "created_at": run.created_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        })),
        "approvals": bundle.approvals.iter().map(proof_approval_value).collect::<Vec<_>>(),
        "unavailable": bundle.unavailable.iter().map(|section| json!({
            "section": section.section,
            "reason": section.reason,
        })).collect::<Vec<_>>(),
    })
}

fn proof_effect_class(bundle: &RunProofBundle) -> &'static str {
    let mut has_approval = false;
    let mut has_execution = false;
    for approval in &bundle.approvals {
        has_approval = true;
        let Some(execution) = approval.execution.as_ref() else {
            continue;
        };
        has_execution = true;
        if execution
            .outcome
            .as_ref()
            .is_some_and(|outcome| !outcome.provider_receipt_id.trim().is_empty())
        {
            return "external_receipt";
        }
    }
    if has_execution {
        return "effectful";
    }
    if has_approval {
        return "proposed_effect";
    }
    if bundle.run.is_some() {
        "read_only"
    } else {
        "unknown"
    }
}

/// Render `VerificationMetrics` on the wire. Every count is passed through
/// verbatim from the proto - no rate/percentage is pre-computed here, so a
/// reader always sees the raw numerator and denominator together rather than
/// a ratio that could be misread once either side is zero.
fn verification_metrics_value(metrics: &VerificationMetrics) -> Value {
    json!({
        "org_id": metrics.org_id,
        "since": metrics.since.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "generated_at": metrics.generated_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "total_completed": metrics.total_completed,
        "total_failed": metrics.total_failed,
        "total_cancelled": metrics.total_cancelled,
        "verified_success_count": metrics.verified_success_count,
        "false_success_count": metrics.false_success_count,
        "partially_verified_count": metrics.partially_verified_count,
        "unverified_count": metrics.unverified_count,
        "unknown_verification_count": metrics.unknown_verification_count,
        "structural_method_count": metrics.structural_method_count,
        "postcondition_method_count": metrics.postcondition_method_count,
        "approvals_requested": metrics.approvals_requested,
        "approvals_granted": metrics.approvals_granted,
        "approvals_denied": metrics.approvals_denied,
        "median_seconds_receipt_to_outcome": metrics.median_seconds_receipt_to_outcome,
        "median_seconds_approval_decision": metrics.median_seconds_approval_decision,
        "unavailable": metrics.unavailable.iter().map(|section| json!({
            "section": section.section,
            "reason": section.reason,
        })).collect::<Vec<_>>(),
    })
}

fn proof_approval_value(proof: &ApprovalProof) -> Value {
    json!({
        "approval_id": proof.approval_id,
        "kind": proof.kind,
        "status": proof.status,
        "requested_by": empty_to_null(&proof.requested_by),
        "decided_by": empty_to_null(&proof.decided_by),
        "decision_reason": empty_to_null(&proof.decision_reason),
        "requested_at": proof.requested_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "decided_at": proof.decided_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "execution": proof.execution.as_ref().map_or(Value::Null, proof_execution_value),
    })
}

/// Render the shared verification vocabulary on the wire. Deliberately the
/// same lowercase strings session-core persists and `run-console-client.ts`
/// already documents, so the value means one thing across the whole chain.
/// `UNSPECIFIED` and any unknown value render as `"unknown"` — never as a
/// success variant, and never as `null` (which a reader could mistake for
/// "no verification was attempted").
fn verification_status_name(status: i32) -> &'static str {
    match VerificationStatus::try_from(status) {
        Ok(VerificationStatus::VerifiedSuccess) => "verified_success",
        Ok(VerificationStatus::VerifiedFailure) => "verified_failure",
        Ok(VerificationStatus::PartiallyVerified) => "partially_verified",
        _ => "unknown",
    }
}

fn proof_execution_value(execution: &ContinuationExecution) -> Value {
    json!({
        "receipt_id": execution.receipt_id,
        "delivery_id": execution.delivery_id,
        "action_fingerprint": execution.action_fingerprint,
        "execution_service_id": execution.execution_service_id,
        "descriptor_version": execution.descriptor_version,
        "started_at": execution.started_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        "outcome": execution.outcome.as_ref().map_or(Value::Null, |outcome| json!({
            "outcome": outcome.outcome,
            "provider_receipt_id": empty_to_null(&outcome.provider_receipt_id),
            "failure_code": empty_to_null(&outcome.failure_code),
            "finalized_at": outcome.finalized_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
            "verification": outcome.verification.as_ref().map_or(Value::Null, |v| json!({
                "status": verification_status_name(v.status),
                "method": empty_to_null(&v.method),
                "reason": empty_to_null(&v.reason),
                "verified_at": v.verified_at.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
            })),
        })),
    })
}

fn lineage_value(lineage: &SubagentLineage) -> Value {
    json!({
        "thread_id": lineage.thread_id,
        "max_depth": lineage.max_depth,
        "edges": lineage.edges.iter().map(lineage_edge_value).collect::<Vec<_>>(),
    })
}

/// One lineage edge. The goal, batch and edge status (Wave 3 EP-2 S9) are added only when
/// session-core returned them, so an edge without them serialises exactly as it did before: a
/// reloaded wide-research panel groups its cards by `batch_id` and titles them by `goal_summary`.
fn lineage_edge_value(edge: &mp_contracts::model_plane::v1::LineageEdge) -> Value {
    let mut value = json!({
        "parent_run_id": edge.parent_run_id,
        "child_run_id": edge.child_run_id,
        "role": enum_name(SubagentRole::try_from(edge.role).ok().as_ref()),
    });
    if let Some(object) = value.as_object_mut() {
        for (key, field) in [
            ("goal_summary", &edge.goal_summary),
            ("batch_id", &edge.batch_id),
            ("status", &edge.status),
        ] {
            if !field.is_empty() {
                object.insert(key.to_owned(), Value::String(field.clone()));
            }
        }
    }
    value
}

fn empty_to_null(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_owned())
    }
}

fn enum_name<E>(value: Option<&E>) -> &'static str
where
    E: EnumName,
{
    value.map_or("UNSPECIFIED", EnumName::as_str_name)
}

trait EnumName {
    fn as_str_name(&self) -> &'static str;
}

impl EnumName for PlanState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for PlanStepState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for TodoState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for TodoPriority {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for ApprovalKind {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for ApprovalState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for SubagentRole {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

/// The ONE place the feedback rating vocabulary is normalised.
///
/// Two vocabularies exist and both must keep working:
///   - chat clients send a **polarity** (`positive` | `negative`) — a thumb;
///   - operator/agent surfaces send a **grade** (`good` | `acceptable` |
///     `poor`) — the canonical vocabulary carried on `mp.v1.feedback.rated`
///     and counted by orchestrator-core.
///
/// Everything downstream sees only the canonical grade, so no consumer has to
/// know about thumbs. Mapping a thumbs-**up** to anything other than `good` is
/// the bug this type exists to make impossible: `positive` previously reached
/// orchestrator-core verbatim, incremented the sample total, matched no
/// `rating == "good"` branch, and therefore *lowered* the skill's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedbackRating {
    Good,
    Acceptable,
    Poor,
}

impl FeedbackRating {
    /// Accepted wire tokens → canonical grade. Case- and whitespace-insensitive.
    /// Unknown tokens are rejected rather than coerced: silently grading an
    /// unrecognised value is how a rating turns into the wrong signal.
    fn from_wire(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            // Chat polarity.
            "positive" | "up" | "good" => Some(Self::Good),
            "negative" | "down" | "poor" => Some(Self::Poor),
            // Operator grade with no polarity equivalent.
            "acceptable" | "neutral" => Some(Self::Acceptable),
            _ => None,
        }
    }

    /// Canonical grade token published on the envelope.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Acceptable => "acceptable",
            Self::Poor => "poor",
        }
    }
}

/// Operator feedback published as `mp.v1.feedback.rated`, consumed by
/// orchestrator-core's durable feedback store (`AGENT_QUALITY_PLAN_2026-07-29`
/// §1.2).
///
/// Accepts both callers of this endpoint:
///   - **chat** (`verevonv3` `chat-client.ts`): `{requestId, rating, note}` — the
///     turn is resolved server-side to its durable run id and injected skills;
///   - **operator/agent surfaces**: `{run_id, skill_id?, from_scope?, to_scope?,
///     rating}` — unchanged.
#[derive(Debug, Deserialize)]
struct FeedbackBody {
    /// Durable Session Core run id. Supplied directly by operator surfaces.
    #[serde(default, alias = "runId")]
    run_id: Option<String>,
    /// Client-visible chat turn id (the SSE `request_id`). Resolved server-side
    /// — see [`resolve_feedback_target`].
    #[serde(default, alias = "requestId")]
    request_id: Option<String>,
    /// Skill (or agent acting as a skill) the rating applies to. Optional: a
    /// chat turn has no client-known skill, and skills are resolved server-side.
    #[serde(default, alias = "skillId")]
    skill_id: Option<String>,
    #[serde(default = "default_from_scope", alias = "fromScope")]
    from_scope: String,
    #[serde(default = "default_to_scope", alias = "toScope")]
    to_scope: String,
    /// `positive` | `negative` (chat) or `good` | `acceptable` | `poor`
    /// (operator). Normalised by [`FeedbackRating::from_wire`].
    rating: String,
    /// Optional free-text comment from the rating UI.
    #[serde(default)]
    note: Option<String>,
}

fn default_from_scope() -> String {
    "agent".to_owned()
}
fn default_to_scope() -> String {
    "workspace".to_owned()
}

/// Hand-written so an in-code default matches what serde produces for an absent
/// field — a derived `Default` would give empty scopes and silently diverge from
/// the wire defaults.
impl Default for FeedbackBody {
    fn default() -> Self {
        Self {
            run_id: None,
            request_id: None,
            skill_id: None,
            from_scope: default_from_scope(),
            to_scope: default_to_scope(),
            rating: String::new(),
            note: None,
        }
    }
}

/// Notes are caller-authored free text persisted downstream. Bound it.
const MAX_FEEDBACK_NOTE_BYTES: usize = 1_000;

/// What a rating actually attaches to, after server-side resolution.
#[derive(Debug, PartialEq, Eq)]
struct ResolvedFeedbackTarget {
    /// The durable run whose ownership is verified and which the rating scores.
    run_id: String,
    /// Skills injected into that turn, resolved server-side. Empty is normal
    /// (no skill matched, or the turn predates this gateway process) and must
    /// degrade to a run-only rating rather than an error.
    skill_ids: Vec<String>,
}

fn feedback_error(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

/// Resolve `{run_id | requestId}` into the durable run the rating scores, plus
/// the skill ids that steered it.
///
/// Trust rules, in order:
///   - An explicit `run_id` names a run. That is safe because
///     [`require_durable_run_owner`] still verifies ownership against Session
///     Core before anything is published — naming a run you do not own fails.
///   - Skill ids are **never** taken from the request body when a chat
///     `requestId` resolved: the gateway's own record of what it injected wins,
///     because nothing can verify a client's claim about which skill steered an
///     answer.
///   - An unresolvable `requestId` fails closed. We cannot map it to a durable
///     run, so we cannot authorize it — and accepting it unverified would let
///     any caller write ratings against another tenant's turn.
fn resolve_feedback_target(
    registry: &crate::chat_turn_registry::ChatTurnRegistry,
    claims: &Claims,
    body: &FeedbackBody,
) -> Result<ResolvedFeedbackTarget, HttpJsonError> {
    let explicit_run = body
        .run_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let chat_turn = body
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(request_id) = chat_turn {
        if let Some(turn) = registry.lookup(request_id) {
            // Defence in depth. The authoritative gate is Session Core's owner
            // lookup below; this rejects a cross-tenant id before we even ask.
            if turn.org_id != claims.org_id || turn.user_id != claims.user_id {
                return Err(feedback_error(
                    StatusCode::FORBIDDEN,
                    "turn_not_owned",
                    "This chat turn belongs to another user.",
                ));
            }
            return Ok(ResolvedFeedbackTarget {
                run_id: turn.run_id,
                skill_ids: turn.skill_ids,
            });
        }
        if explicit_run.is_none() {
            return Err(feedback_error(
                StatusCode::CONFLICT,
                "turn_not_rateable",
                "This turn can no longer be rated (the gateway no longer holds its run reference).",
            ));
        }
    }

    let Some(run_id) = explicit_run else {
        return Err(feedback_error(
            StatusCode::BAD_REQUEST,
            "run_reference_required",
            "Either requestId (a chat turn) or run_id (a durable run) is required.",
        ));
    };
    // Operator path: only an explicitly named skill applies.
    let skill_ids = body
        .skill_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_owned()])
        .unwrap_or_default();
    Ok(ResolvedFeedbackTarget {
        run_id: run_id.to_owned(),
        skill_ids,
    })
}

/// Publish an operator rating as a feedback envelope, which orchestrator-core
/// folds into its durable feedback store.
async fn ingest_feedback(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: VerifiedModelBearer,
    Json(body): Json<FeedbackBody>,
) -> Result<Json<Value>, HttpJsonError> {
    // Feedback is a durable promotion signal. Reject the issuer-ZDR caller
    // before the ownership lookup or publisher so neither downstream can retain
    // request-derived state.
    require_non_zdr_durable_mutation(&claims)?;
    let rating = FeedbackRating::from_wire(&body.rating).ok_or_else(|| {
        feedback_error(
            StatusCode::BAD_REQUEST,
            "unknown_rating",
            "rating must be positive|negative or good|acceptable|poor.",
        )
    })?;
    // A turn that streamed on another replica is not in this one's map. With the
    // shared record on, adopt the caller's own record before resolving, so the
    // turn is rateable here instead of `turn_not_rateable`. The guard above has
    // already refused ZDR and waived callers.
    if crate::chat_turn_registry::shared_turn_record_enabled() {
        if let Some(request_id) = body.request_id.as_deref() {
            crate::chat_turn_registry::global()
                .adopt_shared(
                    &state.stream_buffers,
                    &claims.org_id,
                    &claims.user_id,
                    request_id,
                )
                .await;
        }
    }
    let target = resolve_feedback_target(crate::chat_turn_registry::global(), &claims, &body)?;
    require_durable_run_owner(&state, &claims, &target.run_id, &session_bearer).await?;
    // The rating is the label this turn's confidence inputs were missing.
    // Emitted after ownership is proven, so a row can only ever describe a
    // turn its own tenant rated, and only for a chat turn (an operator grading
    // a run by id names no scored answer).
    if let Some(request_id) = body
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        crate::calibration::emit_sample(request_id, rating.as_str());
    }
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let mut end = value.len().min(MAX_FEEDBACK_NOTE_BYTES);
            while end > 0 && !value.is_char_boundary(end) {
                end -= 1;
            }
            value[..end].to_owned()
        });
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "FEEDBACK_RATED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: target.run_id.clone(),
        causation_id: String::new(),
        // A re-rating of the same turn by the same user must REPLACE, not
        // accumulate — otherwise a user who clicks up then down counts twice.
        idempotency_key: format!("feedback_{}_{}", target.run_id, claims.user_id),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("run/{}", target.run_id),
        payload: json!({
            "run_id": target.run_id,
            // Back-compat single-skill field for any existing consumer.
            "skill_id": target.skill_ids.first().cloned().unwrap_or_default(),
            "skill_ids": target.skill_ids,
            "from_scope": body.from_scope,
            "to_scope": body.to_scope,
            "rating": rating.as_str(),
            "note": note,
        }),
        // The guard at the top of this handler answered every ZDR posture with a refusal,
        // so the feedback event that reaches this line is persistent.
        zdr: false,
        ..Default::default()
    };
    state
        .publisher
        .publish("mp.v1.feedback.rated", &envelope)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({
        "accepted": true,
        "rating": rating.as_str(),
        "skill_ids": target.skill_ids,
    })))
}

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub content: String,
    pub model: Option<String>,
    /// Provider selection. `openai-codex-subscription` requires the opaque
    /// `subscription_connection_id` below; neither field contains user auth.
    #[serde(default)]
    pub provider: Option<String>,
    /// Opaque Integration Core connection selected by the user. Integration
    /// Core verifies that it belongs to the authenticated org/user on every
    /// invocation. Browser cookies and OAuth tokens are never accepted here.
    #[serde(default, alias = "subscriptionConnectionId")]
    pub subscription_connection_id: Option<String>,
    pub session_key: Option<String>,
    pub thread_id: Option<String>,
    /// Server-injected, fresh Control decision for a message appended to an
    /// existing scoped thread. Session Core verifies its signature and exact
    /// content commitment before the message is durable.
    pub space_append_context: Option<crate::session_flow::ThreadSpaceContext>,
    /// Complete Control authority for a newly-created scoped thread. The V3
    /// BFF injects this after calling Control; raw browser scope fields are not
    /// authority and Session Core independently verifies this bearer.
    #[serde(default)]
    pub space_context: Option<crate::session_flow::ThreadSpaceContext>,
    /// Server-injected Control `model.run.wake` decision and the stored post and agent it binds
    /// (WS08-S3). With it the stream starts one run from that post and appends no user turn. The
    /// BFF strips any browser value and injects this after Control signs; Session Core verifies it.
    #[serde(default)]
    pub wake_context: Option<crate::session_flow::WakeContext>,
    #[serde(default)]
    pub structured_output_schema: Option<String>,
    #[serde(default)]
    pub zdr: bool,
    /// Caller-selected minimum privacy tier (Venice model_spec.privacy
    /// equivalent). Wire value is the numeric of the shared `PrivacyTier` enum:
    /// absent/0 = no constraint (today's behavior); 1–4 raise the floor.
    /// Unknown numerics fail closed at normalization rather than being honored
    /// as "no constraint" downstream.
    #[serde(default, alias = "minPrivacyTier")]
    pub min_privacy_tier: Option<i32>,
    /// Explicit public-web search intent from chat clients. This duplicates the
    /// `web_search` tool definition as a durable request flag so a UI Search
    /// toggle cannot be lost by tool normalization or client/BFF drift.
    #[serde(default)]
    pub browse_web: bool,
    /// Skills the member picked explicitly for this turn — the composer's `/`
    /// picker. Resolved server-side against the org's catalogue and the SKILL-1
    /// ownership rule (`skills::resolve_requested_skills`); an unknown or
    /// not-usable id is dropped, never trusted. Keyword matching still runs
    /// alongside, so a turn that names no skill behaves exactly as before.
    #[serde(default, alias = "skillIds")]
    pub skill_ids: Vec<String>,
    /// Deep-research intent from the composer's "Dyp research" button.
    ///
    /// The button has existed since the chat surface shipped, but the client
    /// folded it into `browseWeb` (`tools.includes('research') → browseWeb:
    /// true`), so "deep research" was one `web_search` call with a different
    /// icon — exactly the `planMode` failure mode one field above: a control
    /// with no field behind it. With this field set the turn runs
    /// `crate::deep_research`'s plan → search → read → synthesize pipeline and
    /// produces a cited report artifact instead.
    ///
    /// Implies web availability: a research turn always gets `web_search` in
    /// its tool set, whether or not the Search toggle is also on.
    #[serde(default, alias = "deepResearch")]
    pub deep_research: bool,
    /// Wave 3 EP-2 S5 — which turn of a two-turn deep research this is: `"plan"` ends with an
    /// editable plan and searches nothing, `"run"` runs the plan in `research_plan`. Absent is
    /// today's one-shot. Kept as raw JSON and read only on a deep-research turn while
    /// `DEEP_RESEARCH_PLAN_TURN` is on (`deep_research::research_phase`), so a body carrying it is
    /// parsed exactly as before with the switch off.
    #[serde(default, alias = "researchPhase")]
    pub research_phase: Option<serde_json::Value>,
    /// Wave 3 EP-2 S5 — the edited plan a run turn carries:
    /// `{sub_queries: [string], answers: [{question, answer}]}`. Untrusted: the gateway re-clamps
    /// every sub-query and bounds the answers before anything is searched.
    #[serde(default, alias = "researchPlan")]
    pub research_plan: Option<serde_json::Value>,
    /// Per-turn retrieval-source filter (Perplexity-Focus style). Absent/unknown
    /// → "auto" (today's behavior, byte-identical). Meaningless and ignored
    /// when `conversation_only` is in effect — that mode already excludes both
    /// organization knowledge and the public web.
    #[serde(default, alias = "retrievalScope")]
    pub retrieval_scope: Option<String>,
    /// The connected accounts the person selected for this turn (build plan WS23 S4): Integration Core
    /// connection ids, at most eight. Absent or empty selects none. A selection is refused, typed, under
    /// zero retention, in a Space, under a knowledge-only or web-only source filter, in a conversation-only
    /// turn and when an id is malformed (`connector_selection` in the chat stream module).
    #[serde(default, alias = "connectorScope")]
    pub connector_scope: Option<Vec<String>>,
    /// The client asked to regenerate the previous answer rather than send a new
    /// message.
    ///
    /// Client-declared because the server cannot infer it: a regenerate arrives
    /// as an ordinary turn carrying the same question. It is only ever WEAK
    /// evidence of dissatisfaction (a regenerate often means "another style"),
    /// and it is weighted accordingly downstream — so a client asserting it
    /// cannot do more than nudge one skill's score.
    #[serde(default, alias = "regenerate")]
    pub regenerated: bool,
    /// Durable ids of earlier messages in this thread that the user pinned into
    /// context.
    ///
    /// A SELECTOR, not content: each id names a message that already exists in
    /// the durable thread, and the server resolves it against what session-core
    /// returns. An id matching nothing is ignored, so a client cannot use this to
    /// inject text it invented as though the user had said it earlier, and a
    /// locally-created turn that is not persisted yet simply has no effect until
    /// it is.
    ///
    /// Bounded server-side (`compaction::MAX_PINNED_MESSAGES`,
    /// `MAX_PINNED_CHARS`): the mechanism protects messages from being shed for
    /// length, so an unbounded list would crowd out the live conversation
    /// through the very path meant to prevent that.
    #[serde(default, alias = "pinnedMessageIds")]
    pub pinned_message_ids: Vec<String>,
    /// The client resubmitted an EDITED version of the previous question.
    ///
    /// Also client-declared, and for the same reason: the edit happened in the
    /// composer and never reached the server as a distinct action.
    #[serde(default, alias = "editResubmit")]
    pub edited_resubmit: bool,
    /// The turn regenerates the answer with this message id (WS17 S9): the thread's active branch moves to
    /// that answer's question, and the answer is produced again beside the old one, with no user turn
    /// appended. Honoured only while `MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED` is on, and refused under zero
    /// retention and for an agentic turn. A blank id is absent.
    #[serde(default, alias = "regenerateOfMessageId")]
    pub regenerate_of_message_id: Option<String>,
    /// The turn is an edit of the user turn with this message id (WS17 S9): the active branch moves to that
    /// turn's parent, and the edited text is appended beside the original. The rules of
    /// `regenerate_of_message_id` apply.
    #[serde(default, alias = "editOfMessageId")]
    pub edit_of_message_id: Option<String>,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Harness profile ("chat" | "`deployed_agent`"). Drives the approval
    /// posture (`HARNESS_PHASE1` §1). Absent → "chat" (auto, non-gating).
    #[serde(default)]
    pub profile: Option<String>,
    /// The agent mode the person chose for this turn (WS25-S6): `lockdown`, `read_only`, `ask_first`
    /// or `auto`. Read only while `AGENT_MODES` is on, and then it can only tighten the posture the
    /// platform already sets; an id this build does not know is refused typed (`agent_mode_unknown`).
    /// Absent or blank is no choice, and the turn is the one it was before the field existed.
    #[serde(default, alias = "agentMode")]
    pub agent_mode: Option<String>,
    /// Response-style / verbosity profile (token-efficiency layer):
    /// `concise` | `detailed` | `minimal` | `normal`. `normal`/unset/unknown
    /// injects no directive. See `crate::verbosity`.
    #[serde(default)]
    pub verbosity: Option<String>,
    /// Extended-thinking effort dial: `quick` | `standard` | `deep`.
    /// `standard`/unset/unknown requests no thinking, so the default turn is
    /// unchanged. The token budget is derived server-side from the profile —
    /// a client selects an effort, never a raw budget. See `crate::thinking`.
    #[serde(default)]
    pub effort: Option<String>,
    /// Opt-in rich SSE event families the client understands (chat-parity §2:
    /// "reasoning", "tools", "citations", "artifacts", "steps", "usage"). EMPTY
    /// → plain stream (connected/chunk/done/error only); protects profile:"chat".
    #[serde(default)]
    pub features: Vec<String>,
    /// chat-parity §1 — optional client idempotency key. When set, a duplicate
    /// `/v1/invoke` (double-submit, regenerate retry, network replay) returns
    /// the original response without re-running inference or re-charging budget.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// chat-parity §2 — multimodal attachments. An image attachment routes the
    /// turn through inference-core `AnalyzeImage` (vision) on the stream path.
    #[serde(default)]
    pub attachments: Vec<crate::vision::InvokeAttachment>,
    /// chat-parity §2 — explicit image-generation intent. When true, the prompt
    /// is routed to inference-core `GenerateImage` and the result is emitted as
    /// an `artifact` event (deterministic trigger — no intent guessing).
    #[serde(default)]
    pub generate_image: bool,
    /// Plan-mode intent from the composer's Planmodus toggle.
    ///
    /// This field's ABSENCE was a live bug: the frontend has always sent
    /// `planMode`, but with no matching field here serde discarded it silently
    /// (no `deny_unknown_fields` anywhere in this service), so the toggle
    /// changed the button's colour and nothing else. Meanwhile
    /// `coordinator::handle_enter_plan_mode` and session-core's durable
    /// `SetRunMode` both existed with zero callers on the HTTP path. Accepting
    /// it here is what connects the switch to the machinery.
    #[serde(default, alias = "planMode")]
    pub plan_mode: bool,
    /// Run this turn as an EXECUTING run under the thread's live plan grant (WS14 S2a, "run
    /// the approved plan"). Fail closed: without a live grant for the verified org and the
    /// run's own thread the turn is refused, typed (`plan_grant_unavailable`), and never runs
    /// with no stated rung. It needs the `agentic` feature, no `plan_mode` and a `thread_id`
    /// (`sse::execute_request_problem`), and the whole feature sits behind
    /// `MODEL_GATEWAY_PLAN_EXECUTE_ENABLED` (typed `plan_execute_disabled` when off). It
    /// carries NO rung: the rung is the one `plan_approval` recorded, and a rung supplied by a
    /// client is never read. A ZDR token is refused through the `agentic` row.
    #[serde(default, alias = "executeGranted")]
    pub execute_granted: bool,
    /// The plan an approve-then-execute turn runs (WS14 S2): the plan a plan approval answered
    /// `continuation: execute_now` for. It is read only while `MODEL_GATEWAY_PLAN_EXECUTE_ON_APPROVE`
    /// is on, and only together with `execute_granted`. The turn is refused typed
    /// (`plan_grant_unavailable`) unless the thread's grant names this plan, and the run then binds
    /// that grant itself (`sse::approve_then_execute_grant`). It carries no rung.
    #[serde(default, alias = "approvedPlanId")]
    pub approved_plan_id: Option<String>,
    /// chat-parity §2 function-calling: tool definitions the model may call.
    /// Empty → no tools. With the `tools` feature opted in, the stream path runs
    /// the tool loop (`tool_call`/`tool_result` events) before the final answer.
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// Display name of the org the caller is acting as, stamped by the BFF
    /// gateway from the verified session (never client-suppliable — the raw
    /// browser doesn't reach this endpoint directly). Framing text only: used
    /// to tell the model who "we"/"us"/"our" refers to. Never used for
    /// authorization or retrieval scoping — that stays keyed off the verified
    /// `org_id` claim regardless of what this string says.
    #[serde(default)]
    pub org_name: Option<String>,
    /// Display name of the signed-in user, same provenance/trust notes as
    /// `org_name`. Used to tell the model who "I"/"me"/"my" refers to.
    #[serde(default)]
    pub user_name: Option<String>,
    /// Present only when this turn was addressed to a Space agent by `@`
    /// mention. Stamped by the BFF gateway after it re-resolved the mention
    /// against Control's live Space membership and the Application binding —
    /// same provenance/trust notes as `org_name`: the raw browser cannot reach
    /// this endpoint directly, and a mention that failed that check never
    /// reaches here at all. Framing text only; a Space agent is not a security
    /// boundary and this field grants nothing by itself
    /// (`docs/space-defenition.md`, "Security model").
    #[serde(default)]
    pub agent_name: Option<String>,
    /// The mentioned agent's own instructions, resolved server-side from its
    /// definition. Never sent by, or shown to, the browser — the mention UI
    /// only ever carries the agent's `subject_id` ref.
    #[serde(default)]
    pub agent_system_prompt: Option<String>,
    /// ADR-0003 — the org layer of the authored-instruction hierarchy,
    /// resolved server-side (Convex `organizations.instructions`) by the BFF
    /// gateway on every turn with a verified org. Same provenance/trust notes
    /// as `org_name`: never client-suppliable, framing text only.
    #[serde(default)]
    pub org_instructions: Option<String>,
    /// ADR-0003 — the Space layer of the authored-instruction hierarchy,
    /// resolved server-side (Convex `spaces.instructions`) by the BFF gateway
    /// when the turn carries a Space reference. Same provenance/trust notes
    /// as `agent_name`.
    #[serde(default)]
    pub space_instructions: Option<String>,
    /// Persona spec — the user layer of the authored-instruction hierarchy,
    /// resolved server-side (Convex `users.personaInstructions`) by the BFF
    /// gateway on every turn, gated on the caller's global persona switch.
    /// Same provenance/trust notes as `org_instructions`: never
    /// client-suppliable, framing text only. The PER-THREAD override is a
    /// separate bit (`persona_enabled` on the context-assembly response),
    /// not carried here — this field is the text alone.
    #[serde(default)]
    pub persona_instructions: Option<String>,
    /// The reply tone the person picked (EP-8-S3), resolved server-side from user-core's AI settings
    /// by the BFF gateway and never client-suppliable, like `persona_instructions`. Only the closed
    /// names `professional`, `friendly`, `direct`, `warm` and `neutral` mean anything: `default` and any
    /// other value add nothing. Read only while `MODEL_GATEWAY_TONE_PRESETS` is on.
    #[serde(default)]
    pub persona_tone: Option<String>,
    /// WS16-S6 — the viewer's interface language, stamped by the BFF gateway from a closed set
    /// (`nb-NO` or `en-US`; flag `CHAT_LOCALE_HINT`). A framing hint for the reply language,
    /// request-scoped: never stored, grants nothing, and `Honored` under ZDR. See
    /// [`crate::locale_hint`]. Absent on agent wake-ups.
    #[serde(default)]
    pub locale: Option<String>,
    /// The browser's IANA zone name (`Europe/Oslo`), sent on every turn (EP-8-S1). The BFF gateway
    /// keeps it only in that closed form; it is checked again here, because it is printed into the
    /// prompt. Read only while `MODEL_GATEWAY_USER_LOCALE_CONTEXT` is on; never stored.
    #[serde(default)]
    pub client_timezone: Option<String>,
    /// The browser's UTC offset in minutes, east positive (EP-8-S1). Dates and times are computed from
    /// it, so a value outside the range a browser can report gives no local clock at all.
    #[serde(default)]
    pub client_utc_offset_minutes: Option<i32>,
    /// The app language the browser shows, `no` or `en` (EP-8-S1). Distinct from `locale` above, which
    /// is the WS16-S6 hint with its own switch and vocabulary (`nb-NO` / `en-US`).
    #[serde(default)]
    pub ui_locale: Option<String>,
    /// EP-6 S10 — the published agent definitions this turn's `subagent.task` may name as a
    /// role, resolved server-side by the BFF gateway (the Space's bound agents, each at its
    /// pinned or published version) and never client-suppliable, like `agent_system_prompt`.
    /// Kept as raw JSON: it is read, bounded and typed only while
    /// `MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE` is on (`sse::compose_agent_roles`), so a body
    /// carrying it is parsed exactly as before with the switch off. Grants nothing: a role
    /// only narrows a delegated child's tools.
    #[serde(default)]
    pub agent_roles: Option<serde_json::Value>,
    /// EP-6 S9 — the mentioned agent's definition id (Convex), stamped by the BFF gateway with
    /// `AGENT_VERSION_PINNING` on, from the binding it re-resolved; any client copy is stripped
    /// there first. The five `agent_*` pin fields below are kept as raw JSON for the same reason
    /// as `agent_roles`: they are read and typed only while `MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST`
    /// is on (`sse::agent_pin`), so a body carrying them parses exactly as before with it off.
    #[serde(default)]
    pub agent_ref: Option<serde_json::Value>,
    /// EP-6 S9 — the definition version this turn runs (the binding's pin, else the published
    /// one), from 1.
    #[serde(default)]
    pub agent_version: Option<serde_json::Value>,
    /// EP-6 S9 — `sha256:` digest of that version's canonical body, so a log line names exactly
    /// what answered.
    #[serde(default)]
    pub agent_digest: Option<serde_json::Value>,
    /// EP-6 S9 — the version's tool allowlist (WS05 capability ids). Only ever narrows the turn's
    /// tools; absent means the definition states none.
    #[serde(default)]
    pub agent_tool_capability_ids: Option<serde_json::Value>,
    /// EP-6 S9 — the version's autonomy cap (`read_only`, `ask_first` or `auto`), one more input
    /// to the WS25-S6 strictest-wins mode resolve, so it can only lower the result.
    #[serde(default)]
    pub agent_autonomy_cap: Option<serde_json::Value>,
    /// Wave 3 EP-2 S8 — a wide-research run from the composer's "Bred forskning" entry:
    /// `{item_cap, items}`, or a one-item retry `{retry_of_child_run_id, batch_id}` (EP-2 S9).
    /// Kept as raw JSON and read only on an agentic turn while
    /// `WIDE_RESEARCH_ENTRY_ENABLED` is on ([`wide_research_request`]), so a body carrying it is
    /// parsed exactly as before with the switch off. It only narrows a run: the gateway clamps
    /// `item_cap` to the org's `research.max_fan_out_items`.
    #[serde(default, alias = "wideResearch")]
    pub wide_research: Option<serde_json::Value>,
    /// Wave 3 EP-3 S5 — the typed context references the composer attached: `[{kind, id, label,
    /// selection?}]`, selectors only, never content. Kept as raw JSON and read only while
    /// `MODEL_GATEWAY_CONTEXT_REFS_ENABLED` is on ([`crate::context_refs::resolve`]), so a body
    /// carrying it parses exactly as before with the switch off. A ZDR turn carrying any is refused
    /// with the `context_references` feature whatever the switch says.
    #[serde(default, alias = "contextRefs")]
    pub context_refs: Option<serde_json::Value>,
    /// Wave 3 EP-5 S7 — the brand deck template the person picked: a skill id, never a name the
    /// model chose (decision D5). Kept as raw JSON and read only while `SKILL_DECK_TEMPLATES_ENABLED`
    /// is on ([`crate::deck_templates::requested`]), so a body carrying it parses exactly as before
    /// with the switch off. The turn checks it with [`crate::deck_templates::resolve`] before use; a
    /// ZDR turn carrying one is refused with the `skills` feature.
    #[serde(default, alias = "deckTemplate")]
    pub deck_template: Option<serde_json::Value>,
    /// Wave 3 EP-7 S1 — the thread goal a Continue turn works toward (`goal_...`). Read only on a
    /// Do turn while `MODEL_GATEWAY_THREAD_GOALS_ENABLED` is on
    /// ([`crate::thread_goal_routes::turn_goal_recitation`]): a goal that is not the thread's
    /// active goal is refused `thread_goal_mismatch` before any run exists. A ZDR turn carrying one
    /// is refused with the `thread_goal` feature whatever the switch says.
    #[serde(default, alias = "goalId")]
    pub goal_id: Option<String>,
    /// Wave 3 EP-7 S4 — the turn was sent by the goal bar's "Keep going" pump after its countdown,
    /// not typed. It grants nothing: it marks the run for audit (`runs.metadata.goal_auto`, through
    /// [`crate::goal_auto::mark`]) and is refused `goal_auto_disabled` before any run exists unless
    /// `MODEL_GATEWAY_GOAL_AUTO_CONTINUE` is on ([`crate::goal_auto::refusal`]). Every goal turn is
    /// still claimed by session-core, which refuses while a decision waits on the owner.
    #[serde(default, alias = "goalAuto")]
    pub goal_auto: bool,
}

/// The switch of the wide-research entry (Wave 3 EP-2 S8). Opt-in: only the literal `true` enables
/// it. Off, `POST /v1/research/wide/estimate` answers 404 and `InvokeRequest.wide_research` is never
/// read, so a run is exactly what it was before the switch existed.
pub const WIDE_RESEARCH_ENTRY_ENV: &str = "WIDE_RESEARCH_ENTRY_ENABLED";

/// Whether the wide-research entry is on in this process.
#[must_use]
pub fn wide_research_entry_enabled() -> bool {
    wide_research_entry_enabled_with(&|key| std::env::var(key).ok())
}

fn wide_research_entry_enabled_with(get: &dyn Fn(&str) -> Option<String>) -> bool {
    get(WIDE_RESEARCH_ENTRY_ENV).is_some_and(|value| value.trim() == "true")
}

/// The most items one wide-research run fans out: the hard maximum of the org policy key and of
/// execution-core's `subagent::MAX_WIDE_RESEARCH_ITEMS`.
pub const MAX_WIDE_RESEARCH_ITEMS: u32 = mp_contracts::org_agent_policy::MAX_FAN_OUT_ITEMS;

/// Most items a wide-research request may carry, kept and dropped together (execution-core reads
/// no more than this many either).
const MAX_WIDE_RESEARCH_REQUEST_ITEMS: usize = 64;

/// Longest item forwarded; execution-core cuts at the same length.
const MAX_WIDE_RESEARCH_ITEM_CHARS: usize = 500;

/// Longest child run id a retry may name (EP-2 S9); run ids are ULID-based and far shorter.
const MAX_WIDE_RESEARCH_RUN_ID_CHARS: usize = 128;

/// Longest batch id a retry may name (EP-2 S9): a batch id is the fan-out call's step id.
const MAX_WIDE_RESEARCH_BATCH_ID_CHARS: usize = 256;

/// Whether a retry's run or batch id is one execution-core may look up: non-empty, at most `max`
/// characters, printable ASCII with no whitespace. Anything else is not an id this platform minted.
fn wide_research_retry_id_ok(id: &str, max: usize) -> bool {
    !id.is_empty() && id.len() <= max && id.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Items in flight at once in one fan-out: execution-core's `WIDE_RESEARCH_FAN_OUT_CONCURRENCY`
/// default. The gateway cannot read that executor's environment, so the estimate states this.
const WIDE_RESEARCH_CONCURRENCY: u32 = 4;

/// The org's wide-research item cap and where it came from: the org's own policy, or the key's
/// platform default when the policy switch does not enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideResearchCap {
    pub items: u32,
    pub source: &'static str,
}

impl WideResearchCap {
    /// The cap a run is held to: the org's value when the policy is enforced (`Some`), else the
    /// key's default, never above [`MAX_WIDE_RESEARCH_ITEMS`].
    #[must_use]
    pub fn of(org_cap: Option<u32>) -> Self {
        match org_cap {
            Some(items) => Self {
                items: items.min(MAX_WIDE_RESEARCH_ITEMS),
                source: "org_policy",
            },
            None => Self {
                items: mp_contracts::org_agent_policy::DEFAULT_MAX_FAN_OUT_ITEMS,
                source: "platform_default",
            },
        }
    }
}

/// The wide-research request a Do run carries (Wave 3 EP-2 S8), from the turn's raw JSON.
///
/// `None` with the switch off, for a ZDR or waived turn, and for a body that is not
/// `{item_cap?, items?}` or a retry `{retry_of_child_run_id, batch_id?}` (EP-2 S9). Items are trimmed, blank ones skipped, each cut to 500 characters and at
/// most 64 kept; `item_cap` is the person's ask (the item count when absent) clamped to the org
/// cap, so a request can only narrow. An org cap of 0 is sent as 0, and execution-core refuses the
/// run: wide research is off for that org.
#[must_use]
pub fn wide_research_request(
    enabled: bool,
    raw: Option<&Value>,
    zdr_or_waived: bool,
    cap: WideResearchCap,
) -> Option<mp_contracts::model_plane::v1::WideResearchRequest> {
    #[derive(Deserialize)]
    struct Input {
        #[serde(default)]
        item_cap: Option<u32>,
        #[serde(default)]
        items: Vec<String>,
        #[serde(default)]
        retry_of_child_run_id: Option<String>,
        #[serde(default)]
        batch_id: Option<String>,
    }

    if !enabled || zdr_or_waived {
        return None;
    }
    let input = raw
        .filter(|raw| raw.is_object())
        .and_then(|raw| serde_json::from_value::<Input>(raw.clone()).ok())?;
    // A one-item retry (EP-2 S9): execution-core reads the item back from the child run, owner
    // checked, so the body's items are not forwarded; the cap is one item, and an org with wide
    // research off is still sent 0. A malformed id is no request at all, never a fresh fan-out.
    if let Some(child_run_id) = input.retry_of_child_run_id.as_deref() {
        let batch_id = input.batch_id.as_deref().unwrap_or_default();
        if !wide_research_retry_id_ok(child_run_id, MAX_WIDE_RESEARCH_RUN_ID_CHARS)
            || !(batch_id.is_empty()
                || wide_research_retry_id_ok(batch_id, MAX_WIDE_RESEARCH_BATCH_ID_CHARS))
        {
            return None;
        }
        return Some(mp_contracts::model_plane::v1::WideResearchRequest {
            item_cap: cap.items.min(1),
            items: Vec::new(),
            batch_id: batch_id.to_owned(),
            retry_of_child_run_id: child_run_id.to_owned(),
        });
    }
    let items: Vec<String> = input
        .items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(MAX_WIDE_RESEARCH_REQUEST_ITEMS)
        .map(|item| item.chars().take(MAX_WIDE_RESEARCH_ITEM_CHARS).collect())
        .collect();
    let asked = input
        .item_cap
        .filter(|&count| count > 0)
        .or_else(|| u32::try_from(items.len()).ok().filter(|&count| count > 0))
        .unwrap_or(cap.items);
    Some(mp_contracts::model_plane::v1::WideResearchRequest {
        item_cap: asked.min(cap.items).min(MAX_WIDE_RESEARCH_ITEMS),
        items,
        ..Default::default()
    })
}

/// `POST /v1/research/wide/estimate` body: how many items the person means to research, and on
/// which model and route.
#[derive(Debug, Deserialize)]
pub struct WideResearchEstimateRequest {
    pub item_count: u32,
    #[serde(default)]
    pub model: Option<String>,
    /// The route the run would be pinned to, as the composer sends it on the turn; a subscription
    /// route has no per-token price.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub subscription_connection_id: Option<String>,
}

/// What one research round is assumed to read and write, for the USD range only: a researcher's
/// round reads a page or a result list and writes a short note. A range, never a promise.
const ESTIMATE_INPUT_TOKENS_PER_ROUND: (u32, u32) = (6_000, 18_000);
const ESTIMATE_OUTPUT_TOKENS_PER_ROUND: (u32, u32) = (500, 1_500);

/// Seconds one item round is assumed to take, low and high, for the wall-clock range.
const ESTIMATE_SECONDS_PER_ROUND: (u64, u64) = (30, 90);

/// What a wide-research run of some items would look like before it starts (Wave 3 EP-2 S8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideResearchEstimate {
    pub items_requested: u32,
    pub items_after_cap: u32,
    pub concurrency: u32,
    pub rounds_per_item: u32,
    pub minutes_low: u64,
    pub minutes_high: u64,
    pub cap: WideResearchCap,
}

impl WideResearchEstimate {
    /// The pure part of the estimate. `max_rounds` is the run's round budget and `deadline_secs`
    /// its wall clock, resolved server-side exactly as a real run's are.
    #[must_use]
    pub fn compute(
        item_count: u32,
        cap: WideResearchCap,
        max_rounds: u32,
        deadline_secs: u64,
    ) -> Self {
        let items = item_count.min(cap.items).min(MAX_WIDE_RESEARCH_ITEMS);
        let concurrency = WIDE_RESEARCH_CONCURRENCY.min(items.max(1));
        // The parent spends its first round calling the fan-out and keeps one to answer with
        // (execution-core's `PARENT_SYNTHESIS_RESERVE`); the rest is divided between the items.
        let rounds_per_item = if items == 0 {
            0
        } else {
            max_rounds.saturating_sub(2) / items
        };
        let waves = u64::from(items.div_ceil(concurrency));
        let rounds = u64::from(rounds_per_item.max(1));
        let deadline_minutes = deadline_secs.div_ceil(60);
        Self {
            items_requested: item_count,
            items_after_cap: items,
            concurrency,
            rounds_per_item,
            minutes_low: (waves * ESTIMATE_SECONDS_PER_ROUND.0)
                .div_ceil(60)
                .min(deadline_minutes),
            minutes_high: (waves * rounds * ESTIMATE_SECONDS_PER_ROUND.1)
                .div_ceil(60)
                .min(deadline_minutes),
            cap,
        }
    }

    /// The USD range on a metered, priced route; `None` (null on the wire) on a subscription
    /// route or for a model the catalogue does not name, never a borrowed figure.
    #[must_use]
    pub fn usd(&self, list: Option<&crate::pricing::ListPrice>) -> Option<(f64, f64)> {
        let list = list?;
        let rounds = f64::from(self.items_after_cap) * f64::from(self.rounds_per_item.max(1));
        let cost = |input: u32, output: u32| {
            rounds
                * (f64::from(input) * list.input_per_million
                    + f64::from(output) * list.output_per_million)
                / 1_000_000.0
        };
        Some((
            cost(
                ESTIMATE_INPUT_TOKENS_PER_ROUND.0,
                ESTIMATE_OUTPUT_TOKENS_PER_ROUND.0,
            ),
            cost(
                ESTIMATE_INPUT_TOKENS_PER_ROUND.1,
                ESTIMATE_OUTPUT_TOKENS_PER_ROUND.1,
            ),
        ))
    }

    /// The `data` of the estimate answer.
    #[must_use]
    pub fn json(&self, usd: Option<(f64, f64)>, basis: crate::pricing::CostBasis) -> Value {
        json!({
            "items_requested": self.items_requested,
            "items_after_cap": self.items_after_cap,
            "concurrency": self.concurrency,
            "rounds_per_item": self.rounds_per_item,
            "minutes_low": self.minutes_low,
            "minutes_high": self.minutes_high,
            "usd_low": usd.map(|(low, _)| low),
            "usd_high": usd.map(|(_, high)| high),
            "cost_basis": basis.as_str(),
            "cap": self.cap.items,
            "cap_source": self.cap.source,
        })
    }
}

/// A typed error of the estimate route; `details` is always an object.
fn wide_research_error(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

/// `POST /v1/research/wide/estimate` (Wave 3 EP-2 S8): what a wide-research run of `item_count`
/// items would look like, before it is started. Pure compute: nothing is stored and nothing is
/// started. 404 while `WIDE_RESEARCH_ENTRY_ENABLED` is off. A zero-retention caller is refused with
/// the typed `agentic` refusal, because the run it estimates is refused under ZDR. The org's cap is
/// read as a Do run's is; a policy that is enforced and cannot be read answers
/// `org_policy_unavailable`. USD is null unless the route is metered and the model priced.
async fn wide_research_estimate_route(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    capability_bearer: Option<Extension<VerifiedCapabilityBearer>>,
    Json(req): Json<WideResearchEstimateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    if !wide_research_entry_enabled() {
        return Err(wide_research_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "not found",
        ));
    }
    wide_research_estimate_posture(&claims)?;
    let org_cap = wide_research_org_cap(&state, &claims.org_id, capability_bearer.as_deref())
        .await
        .map_err(|code| {
            wide_research_error(
                StatusCode::SERVICE_UNAVAILABLE,
                code,
                crate::org_agent_policy::POLICY_UNAVAILABLE_MESSAGE,
            )
        })?;
    let basis = crate::pricing::CostBasis::of_route(
        req.provider.as_deref().unwrap_or_default(),
        req.subscription_connection_id.as_deref().unwrap_or_default(),
    );
    let pinned = basis == crate::pricing::CostBasis::Subscription;
    let estimate = WideResearchEstimate::compute(
        req.item_count,
        WideResearchCap::of(org_cap),
        crate::run_budget::run_budget_from_env(pinned).max_rounds,
        crate::sse::agentic_run_deadline_secs(pinned),
    );
    let model = req.model.as_deref().map(str::trim).unwrap_or_default();
    let list = if model.is_empty() {
        None
    } else {
        state.pricing.snapshot().await.list_price(basis, model)
    };
    Ok(Json(json!({
        "data": estimate.json(estimate.usd(list.as_ref()), basis)
    })))
}

/// The estimate's ZDR gate: a zero-retention caller is refused with the `agentic` feature (the run
/// it estimates is refused the same way); a waiver, which names one chat connection, is answered as
/// a Do run of its org would be, since the estimate names no connection and stores nothing.
///
/// # Errors
///
/// The typed refusal as an HTTP answer.
fn wide_research_estimate_posture(claims: &Claims) -> Result<(), HttpJsonError> {
    match crate::zdr_posture::decide_for(claims, &crate::zdr_posture::Route::plain(false)) {
        Ok(posture) if posture.zdr() => Err(crate::zdr_posture::http(
            &mp_contracts::zdr_refusal::Refusal::feature_not_supported("agentic"),
        )),
        Ok(_) => Ok(()),
        Err(refusal) if refusal.code() == mp_contracts::zdr_refusal::code::WAIVER_ROUTE_MISMATCH => {
            Ok(())
        }
        Err(refusal) => Err(crate::zdr_posture::http(&refusal)),
    }
}

/// The org's `research.max_fan_out_items` as the policy switch reads it: `None` when the switch
/// does not enforce (the platform default applies), the org's value when it does.
///
/// # Errors
///
/// [`crate::org_agent_policy::POLICY_UNAVAILABLE_CODE`] when the policy is enforced and cannot be
/// read inside its bound, as a Do run would be refused.
pub(crate) async fn wide_research_org_cap(
    state: &AppState,
    org_id: &str,
    capability_bearer: Option<&VerifiedCapabilityBearer>,
) -> Result<Option<u32>, &'static str> {
    use crate::org_agent_policy::{policy_mode, shared_cache, PolicyMode};

    if policy_mode() != PolicyMode::Enforce {
        return Ok(None);
    }
    crate::org_agent_policy::ensure(
        shared_cache(),
        &state.http_client,
        &state.capability_core_base_url,
        org_id,
        capability_bearer.map_or("", VerifiedCapabilityBearer::as_str),
    )
    .await
    .map(|policy| Some(policy.max_fan_out_items))
    .map_err(|error| {
        tracing::warn!(%org_id, %error, "org agent policy cannot be read for a wide-research estimate");
        crate::org_agent_policy::POLICY_UNAVAILABLE_CODE
    })
}

#[cfg(test)]
mod wide_research_tests {
    use super::*;

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-1",
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": "org-1",
            "user_id": "user-1",
            "principal_type": "user",
            "zdr": zdr,
            "org_zdr": zdr,
        }))
        .expect("a minted token's payload is valid Claims")
    }

    fn org(items: u32) -> WideResearchCap {
        WideResearchCap::of(Some(items))
    }

    #[test]
    fn wide_research_entry_switch_is_only_the_literal_true() {
        for (value, on) in [
            (None, false),
            (Some("false"), false),
            (Some("1"), false),
            (Some("TRUE"), false),
            (Some("true"), true),
        ] {
            let get = |_: &str| value.map(str::to_owned);
            assert_eq!(wide_research_entry_enabled_with(&get), on, "{value:?}");
        }
    }

    #[test]
    fn wide_research_request_is_absent_with_the_switch_off_and_under_zdr() {
        let body = json!({"item_cap": 5, "items": ["a"]});
        assert_eq!(wide_research_request(false, Some(&body), false, org(8)), None);
        assert_eq!(wide_research_request(true, Some(&body), true, org(8)), None);
        assert_eq!(wide_research_request(true, None, false, org(8)), None);
        assert_eq!(
            wide_research_request(true, Some(&json!(["a"])), false, org(8)),
            None
        );
    }

    #[test]
    fn wide_research_request_clamps_the_ask_to_the_org_cap() {
        // Org cap 5, ten items asked: the run is held to 5 and carries all ten items, so
        // execution-core can name the five it drops.
        let items: Vec<String> = (0..10).map(|n| format!("item {n}")).collect();
        let request = wide_research_request(true, Some(&json!({"items": items})), false, org(5))
            .expect("the switch is on");
        assert_eq!(request.item_cap, 5);
        assert_eq!(request.items.len(), 10);
        // A count preset with no list asks for that many, never more than the org allows.
        let preset = wide_research_request(true, Some(&json!({"item_cap": 16})), false, org(8))
            .expect("the switch is on");
        assert_eq!(preset.item_cap, 8);
        assert!(preset.items.is_empty());
        // A narrower ask stays narrower.
        let narrow =
            wide_research_request(true, Some(&json!({"item_cap": 3})), false, org(8)).expect("on");
        assert_eq!(narrow.item_cap, 3);
        // An org with wide research off is sent 0, and execution-core refuses the run.
        let off = wide_research_request(true, Some(&json!({"items": ["a"]})), false, org(0))
            .expect("on");
        assert_eq!(off.item_cap, 0);
    }

    #[test]
    fn wide_research_request_bounds_and_cleans_items() {
        let long = "y".repeat(MAX_WIDE_RESEARCH_ITEM_CHARS + 10);
        let many: Vec<String> = (0..80).map(|n| format!("q{n}")).collect();
        let request = wide_research_request(
            true,
            Some(&json!({"items": ["  a  ", "", "   ", long]})),
            false,
            org(16),
        )
        .expect("on");
        assert_eq!(request.items[0], "a");
        assert_eq!(request.items.len(), 2);
        assert_eq!(request.items[1].chars().count(), MAX_WIDE_RESEARCH_ITEM_CHARS);
        let flood = wide_research_request(true, Some(&json!({"items": many})), false, org(16))
            .expect("on");
        assert_eq!(flood.items.len(), MAX_WIDE_RESEARCH_REQUEST_ITEMS);
    }

    #[test]
    fn a_wide_research_retry_is_one_item_named_by_its_child_run() {
        let body = json!({
            "retry_of_child_run_id": "run-01jchild",
            "batch_id": "run-1:tool_1_fan-1",
            "items": ["ignored"],
            "item_cap": 9,
        });
        let retry = wide_research_request(true, Some(&body), false, org(8)).expect("on");
        assert_eq!(retry.item_cap, 1);
        assert!(retry.items.is_empty(), "execution-core reads the item back itself");
        assert_eq!(retry.retry_of_child_run_id, "run-01jchild");
        assert_eq!(retry.batch_id, "run-1:tool_1_fan-1");
        // An org with wide research off is still refused by execution-core.
        let off = wide_research_request(true, Some(&body), false, org(0)).expect("on");
        assert_eq!(off.item_cap, 0);
        // A retry is a non-ZDR, switched-on request like any other.
        assert_eq!(wide_research_request(false, Some(&body), false, org(8)), None);
        assert_eq!(wide_research_request(true, Some(&body), true, org(8)), None);
    }

    #[test]
    fn a_wide_research_retry_with_a_malformed_id_is_no_request() {
        for body in [
            json!({"retry_of_child_run_id": ""}),
            json!({"retry_of_child_run_id": "run 1"}),
            json!({"retry_of_child_run_id": "r".repeat(MAX_WIDE_RESEARCH_RUN_ID_CHARS + 1)}),
            json!({"retry_of_child_run_id": "run-1", "batch_id": "a\nb"}),
        ] {
            assert_eq!(
                wide_research_request(true, Some(&body), false, org(8)),
                None,
                "{body}"
            );
        }
    }

    #[test]
    fn a_lineage_edge_adds_goal_batch_and_status_only_when_stored() {
        let lineage = SubagentLineage {
            thread_id: "thread-1".to_owned(),
            max_depth: 1,
            edges: vec![
                mp_contracts::model_plane::v1::LineageEdge {
                    parent_run_id: "run-p".to_owned(),
                    child_run_id: "run-old".to_owned(),
                    role: SubagentRole::Generic as i32,
                    ..Default::default()
                },
                mp_contracts::model_plane::v1::LineageEdge {
                    parent_run_id: "run-p".to_owned(),
                    child_run_id: "run-item".to_owned(),
                    role: SubagentRole::Researcher as i32,
                    goal_summary: "look up carrier docs".to_owned(),
                    batch_id: "run-p:tool_1_fan-1".to_owned(),
                    status: "attached".to_owned(),
                    ..Default::default()
                },
            ],
        };
        let value = lineage_value(&lineage);
        // An edge without the readback fields serialises exactly as before them.
        assert_eq!(
            value["edges"][0],
            json!({
                "parent_run_id": "run-p",
                "child_run_id": "run-old",
                "role": SubagentRole::Generic.as_str_name(),
            })
        );
        assert_eq!(value["edges"][1]["goal_summary"], "look up carrier docs");
        assert_eq!(value["edges"][1]["batch_id"], "run-p:tool_1_fan-1");
        assert_eq!(value["edges"][1]["status"], "attached");
    }

    #[test]
    fn wide_research_cap_without_an_enforced_policy_is_the_platform_default() {
        let platform = WideResearchCap::of(None);
        assert_eq!(platform.items, 8);
        assert_eq!(platform.source, "platform_default");
        let held = WideResearchCap::of(Some(40));
        assert_eq!(held.items, MAX_WIDE_RESEARCH_ITEMS);
        assert_eq!(held.source, "org_policy");
    }

    #[test]
    fn wide_research_estimate_clamps_to_the_org_cap_and_bounds_minutes_by_the_deadline() {
        let estimate = WideResearchEstimate::compute(10, org(5), 12, 600);
        assert_eq!(estimate.items_requested, 10);
        assert_eq!(estimate.items_after_cap, 5);
        assert_eq!(estimate.concurrency, 4);
        assert_eq!(estimate.rounds_per_item, 2, "(12 - 2) rounds over 5 items");
        assert!(estimate.minutes_low <= estimate.minutes_high);
        assert!(estimate.minutes_high <= 10, "never past the 600 s deadline");
        let off = WideResearchEstimate::compute(10, org(0), 12, 600);
        assert_eq!(off.items_after_cap, 0);
        assert_eq!(off.rounds_per_item, 0);
    }

    #[test]
    fn wide_research_estimate_usd_is_null_without_a_list_price() {
        let estimate = WideResearchEstimate::compute(4, org(8), 12, 600);
        assert_eq!(estimate.usd(None), None);
        let body = estimate.json(None, crate::pricing::CostBasis::Subscription);
        assert!(body["usd_low"].is_null() && body["usd_high"].is_null(), "{body}");
        assert_eq!(body["cost_basis"], "subscription");
        assert_eq!(body["cap_source"], "org_policy");

        let list = crate::pricing::ListPrice {
            input_per_million: 1.0,
            output_per_million: 2.0,
            currency: "USD".to_owned(),
            source: crate::pricing::PriceSource::Exact,
        };
        let (low, high) = estimate.usd(Some(&list)).expect("a priced, metered route");
        assert!(low > 0.0 && low < high, "{low} {high}");
    }

    /// The golden case `e_zdr_token_wide_research_estimate_refused` (EP-2 S8): a feature refusal,
    /// so it lives here and in the ZDR feature table, not as a posture-matrix row.
    #[test]
    fn wide_research_estimate_refuses_a_zdr_caller_with_the_agentic_feature() {
        let (status, Json(body)) =
            wide_research_estimate_posture(&claims(true)).expect_err("a ZDR caller is refused");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["feature"], "agentic", "{body}");
        assert!(wide_research_estimate_posture(&claims(false)).is_ok());
    }
}

/// A tool/function definition supplied by the client (chat-parity §2).
#[derive(Debug, Clone, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema (as a JSON string) for the tool's parameters.
    #[serde(default)]
    pub parameters_json: String,
}

#[derive(Debug, Serialize)]
pub struct InvokeResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
    pub usage: Option<InvokeUsage>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct InvokeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: Option<f64>,
    pub latency_ms: u64,
    pub confidence: Option<f64>,
}

const DIRECT_INVOKE_MAX_TOKENS: u32 = 4096;

fn invoke_usage(
    content: &str,
    input_tokens: i32,
    output_tokens: i32,
    cost_usd: Option<f64>,
    latency_ms: u64,
) -> InvokeUsage {
    let input_tokens = u32::try_from(input_tokens).unwrap_or_default();
    let output_tokens = u32::try_from(output_tokens).unwrap_or_default();
    InvokeUsage {
        input_tokens,
        output_tokens,
        cost_usd,
        latency_ms,
        confidence: crate::confidence::score(
            content,
            output_tokens,
            DIRECT_INVOKE_MAX_TOKENS,
            crate::confidence::Evidence::default(),
        ),
    }
}

fn cached_invoke_usage(
    usage: Option<crate::idempotency_registry::CachedInvokeUsage>,
) -> Option<InvokeUsage> {
    usage.map(|usage| InvokeUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cost_usd: usage.cost_usd,
        latency_ms: usage.latency_ms,
        confidence: usage.confidence,
    })
}

#[cfg(test)]
mod invoke_response_contract_tests {
    use super::*;

    #[test]
    fn invoke_response_serializes_server_reported_usage() {
        let response = InvokeResponse {
            request_id: "req-1".to_owned(),
            content: "A bounded answer.".to_owned(),
            model_used: "verevon-balance".to_owned(),
            usage: Some(InvokeUsage {
                input_tokens: 42,
                output_tokens: 18,
                cost_usd: Some(0.00042),
                latency_ms: 321,
                confidence: Some(0.78),
            }),
        };

        let value = serde_json::to_value(response).expect("serialize invoke response");
        assert_eq!(value["usage"]["input_tokens"], 42);
        assert_eq!(value["usage"]["output_tokens"], 18);
        assert_eq!(value["usage"]["cost_usd"], 0.00042);
        assert_eq!(value["usage"]["latency_ms"], 321);
        assert_eq!(value["usage"]["confidence"], 0.78);
    }

    #[test]
    fn invoke_usage_is_bounded_and_labels_only_measured_values() {
        let usage = invoke_usage("A bounded answer.", -1, 18, Some(0.00042), 321);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 18);
        assert_eq!(usage.cost_usd, Some(0.00042));
        assert_eq!(usage.latency_ms, 321);
        assert!(usage
            .confidence
            .is_some_and(|value| (0.0..=1.0).contains(&value)));
    }
}

/// chat-parity §4 — cooperatively cancel an in-flight `/v1/invoke/stream`.
/// Flips the registered cancel flag; the SSE loop emits a terminal `stopped`
/// event and closes. With the run-control bus on, a stream another replica owns
/// is asked to stop (202, `via: "peer"`). 404 when no active stream matches the
/// id, 503 `run_control_unavailable` when the bus could not tell.
async fn invoke_cancel(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(request_id): Path<String>,
) -> Response {
    // The stream registry binds request ids to the authenticated tenant/user at
    // registration time. A mismatched owner gets the same 404 as an inactive
    // stream, avoiding unauthorized cancellation and an existence oracle.
    // A ZDR turn never registers a stream, so its Stop stays here and puts
    // nothing on the bus.
    let zdr = matches!(
        crate::zdr_posture::decide_for(&claims, &crate::zdr_posture::Route::plain(false)),
        Ok(crate::zdr_posture::TurnPosture::Zdr { .. })
    );
    let route = if !zdr {
        state
            .run_control
            .cancel_stream(&state.cancels, &request_id, &claims.org_id, &claims.user_id)
            .await
    } else if state
        .cancels
        .cancel_for(&request_id, &claims.org_id, &claims.user_id)
    {
        CancelRoute::Local
    } else {
        CancelRoute::NoOwner
    };
    match route {
        CancelRoute::Local => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "request_id": request_id, "cancelled": true })),
        )
            .into_response(),
        CancelRoute::Peer => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "request_id": request_id,
                "cancelled": true,
                "via": "peer",
            })),
        )
            .into_response(),
        CancelRoute::NoOwner => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "request_id": request_id,
                "cancelled": false,
                "error": "no active stream",
            })),
        )
            .into_response(),
        // Neither a stop nor a missing stream is known: say so, so the client retries.
        CancelRoute::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "request_id": request_id,
                "cancelled": false,
                "error": "run_control_unavailable",
                "retryable": true,
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PlanApprovalRequest {
    /// `read_only` | `workspace_write` | `danger_full_access`. A keyword rather
    /// than a number on the HTTP edge: a browser sending `2` and meaning
    /// something else is a silent mis-grant, and an unknown keyword is refused
    /// rather than guessed at. Required for `decision: "approve"` (the
    /// default); ignored for `"reject"`, which grants nothing.
    #[serde(default)]
    granted_rung: String,
    /// Why. Required and required to be substantive for an approve — see
    /// `mp_contracts::autonomy`. Also carried as the plan transition's
    /// `reason` for both decisions.
    justification: String,
    /// The proposed plan being approved or rejected (plan-gate §2.3). Empty
    /// when this turn never proposed a structured plan — the request then
    /// degrades to today's pure rung grant, unchanged, so a stale frontend
    /// talking to a rebuilt backend (or vice versa) keeps working.
    #[serde(default)]
    plan_id: String,
    /// The user's edit set (reorder/rewrite/add/remove), applied atomically
    /// before the plan transitions. Empty means the plan is
    /// approved/rejected exactly as proposed.
    #[serde(default)]
    edited_steps: Vec<PlanStepEditWire>,
    /// `"approve"` (default — every caller that predates plan-gate never set
    /// this field and always meant approve) or `"reject"`.
    #[serde(default = "default_plan_approval_decision")]
    decision: String,
}

fn default_plan_approval_decision() -> String {
    "approve".to_owned()
}

#[derive(Debug, Deserialize)]
struct PlanStepEditWire {
    /// Existing step id being edited. Empty means this entry is a NEW step
    /// inserted at this position.
    #[serde(default)]
    step_id: String,
    title: String,
    #[serde(default)]
    operation: String,
    /// True when the user removed this step (marked skipped server-side,
    /// never deleted).
    #[serde(default)]
    removed: bool,
}

impl From<PlanStepEditWire> for PlanStepEdit {
    fn from(wire: PlanStepEditWire) -> Self {
        PlanStepEdit {
            step_id: wire.step_id,
            title: wire.title,
            operation: wire.operation,
            removed: wire.removed,
        }
    }
}

#[cfg(test)]
mod plan_approval_wire_tests {
    use super::{PlanApprovalRequest, PlanStepEdit, PlanStepEditWire};

    #[test]
    fn decision_defaults_to_approve_for_every_caller_that_predates_plan_gate() {
        // Every request body plan-gate shipped before this field existed
        // omits it entirely; the default must keep behaving as a pure rung
        // grant with no plan involved.
        let body: PlanApprovalRequest = serde_json::from_str(
            r#"{"granted_rung":"workspace_write","justification":"writes its report"}"#,
        )
        .expect("legacy body without decision/plan_id/edited_steps must still parse");
        assert_eq!(body.decision, "approve");
        assert_eq!(body.plan_id, "");
        assert!(body.edited_steps.is_empty());
    }

    #[test]
    fn full_wire_body_round_trips_every_field() {
        let body: PlanApprovalRequest = serde_json::from_str(
            r#"{
                "granted_rung": "workspace_write",
                "justification": "writes its report",
                "plan_id": "plan_abc",
                "decision": "reject",
                "edited_steps": [
                    {"step_id": "step_1", "title": "Renamed step", "operation": "web_search", "removed": false},
                    {"title": "New step", "removed": true}
                ]
            }"#,
        )
        .expect("full wire body must parse");
        assert_eq!(body.decision, "reject");
        assert_eq!(body.plan_id, "plan_abc");
        assert_eq!(body.edited_steps.len(), 2);
        assert_eq!(body.edited_steps[0].step_id, "step_1");
        assert!(!body.edited_steps[0].removed);
        // A new entry's step_id defaults to empty ("" means "insert"), and
        // `removed` on a brand-new entry is carried through even though the
        // UI is not expected to produce one — the wire type stays honest
        // about what the proto itself allows.
        assert_eq!(body.edited_steps[1].step_id, "");
        assert!(body.edited_steps[1].removed);
    }

    #[test]
    fn wire_edit_maps_field_for_field_onto_the_proto_message() {
        let wire = PlanStepEditWire {
            step_id: "step_9".to_owned(),
            title: "Send the invoice".to_owned(),
            operation: "email_send".to_owned(),
            removed: true,
        };
        let proto: PlanStepEdit = wire.into();
        assert_eq!(proto.step_id, "step_9");
        assert_eq!(proto.title, "Send the invoice");
        assert_eq!(proto.operation, "email_send");
        assert!(proto.removed);
    }
}

/// Approve OR reject a plan (plan-gate §2.3). Approving takes the run out of
/// plan mode at a named autonomy rung, with a stated reason; rejecting grants
/// nothing. When the turn proposed a structured plan (`plan_id` set), the
/// user's edits are applied and the plan transitions BEFORE any authority is
/// granted — an edited plan IS what the rung is being granted to run.
///
/// # Why this route exists at all
///
/// `ExitPlanMode` was reachable only over gRPC and had **no production caller**.
/// Plan mode could be entered from the composer and never left, so the one
/// control that decides how much a run may do had no way to be exercised — and
/// the grant it makes was neither named nor justified anywhere.
///
/// Both halves are validated through the shared ladder and both are persisted
/// onto the run, so the next `RunAgentRequest` carries the granted rung and
/// execution-core enforces it per call.
async fn plan_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
    Json(body): Json<PlanApprovalRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::autonomy::{label, rung_from_keyword, AutonomyEscalation};
    // Owner decision 17: a plan approval advances the run. See approve_plan.
    crate::zdr_posture::continuation_gate(&claims)?;
    use mp_contracts::model_plane::v1::{AutonomyRung, ExitPlanModeRequest};

    let bad_request = |message: String| -> HttpJsonError {
        (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
    };

    // Only the run's own owner may act on it. Without this, a run id would be
    // enough to decide someone else's run.
    crate::session_flow::require_durable_run_owner_with_token(
        &state,
        &run_id,
        &claims.org_id,
        &claims.user_id,
        bearer.as_str(),
    )
    .await
    .map_err(|error| grpc_status_to_http(&error))?;

    if body.decision != "approve" && body.decision != "reject" {
        return Err(bad_request(format!(
            "unknown decision '{}': expected approve or reject",
            body.decision
        )));
    }

    // Edits are edits OF a plan. A body that carries them without naming the
    // plan would be approved with the edits silently dropped: the person
    // believes their rewrite was applied, and the run is granted authority
    // against the plan exactly as the model wrote it.
    if body.plan_id.is_empty() && !body.edited_steps.is_empty() {
        return Err(bad_request(
            "edited_steps requires plan_id: there is no plan to edit".to_owned(),
        ));
    }

    // Resolve the run's THREAD before anything is decided. Two reasons, both
    // about not leaving a half-made decision behind:
    //
    // * the grant has to outlive this run — the work it authorizes happens on
    //   the NEXT run of the same conversation — so `PlanModeStore` keys grants
    //   by thread, and the thread is filled here rather than trusted from the
    //   client so a caller cannot attach a grant to a thread it does not own
    //   (the ownership check above is on the RUN, and session-core resolves
    //   the thread from that same run);
    // * a failure here must leave the plan PROPOSED. This lookup used to run
    //   AFTER the plan had already been moved to APPROVED, so a transient
    //   error at that point left an approved plan with no grant, and — an
    //   approval being single-use — no way to retry it.
    let run_detail = state
        .run_client
        .clone()
        .get_run(authenticated_session_request(
            mp_contracts::model_plane::v1::GetRunRequest {
                run_id: run_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| grpc_status_to_http(&error))?
        .into_inner();
    // Whether the run is in plan mode is read from the durable row here, not from the memory
    // of this replica: the flag was set by whichever replica started the run (WS04 S4).
    let run_was_in_plan_mode = run_detail.mode == "plan";
    let thread_id = run_detail.thread_id;

    // The plan being decided must be a plan OF THIS run's conversation. The
    // route is keyed by run and the body names the plan, and session-core
    // only checks that the caller owns the plan — so without this, a run's
    // approval could decide any other plan the same person owns, and the
    // rung it grants would be recorded against a conversation whose plan was
    // never reviewed.
    if !body.plan_id.is_empty() {
        let plan = state
            .orchestration_client
            .clone()
            .get_plan(authenticated_session_request(
                GetPlanRequest {
                    plan_id: body.plan_id.clone(),
                },
                &bearer,
            )?)
            .await
            .map_err(|e| grpc_status_to_http(&e))?
            .into_inner()
            .plan
            .ok_or_else(|| not_found("plan not found"))?;
        if plan.thread_id != thread_id {
            return Err(bad_request(
                "plan_id is not a plan of this run's conversation".to_owned(),
            ));
        }
    }

    let edited_steps: Vec<PlanStepEdit> = body
        .edited_steps
        .into_iter()
        .map(PlanStepEdit::from)
        .collect();

    // --- Reject: grants nothing. No rung escalation, no set_run_mode — the
    // BFF route stays the single POST .../plan-approval, but a reject has no
    // authority to hand back, only a plan to close out. ---
    if body.decision == "reject" {
        if body.plan_id.is_empty() {
            return Err(bad_request(
                "decision \"reject\" requires plan_id: there is no plan to reject".to_owned(),
            ));
        }
        let resp = state
            .orchestration_client
            .clone()
            .transition_plan(authenticated_session_request(
                TransitionPlanRequest {
                    plan_id: body.plan_id.clone(),
                    target_state: PlanState::Rejected as i32,
                    actor: claims.user_id.clone(),
                    reason: body.justification.clone(),
                    edited_steps,
                },
                &bearer,
            )?)
            .await
            .map_err(|e| grpc_status_to_http(&e))?
            .into_inner();
        let plan = resp.plan.ok_or_else(|| not_found("plan not found"))?;
        return Ok(Json(json!({
            "run_id": run_id,
            "decision": "reject",
            "plan": plan_value(&plan),
        })));
    }
    // --- Approve: unchanged rung/justification validation. ---
    // A keyword on the HTTP edge rather than a number: a browser sending `2` and
    // meaning something else is a silent mis-grant, and an unknown keyword is
    // refused rather than guessed at — guessing turns a typo into a grant.
    let Some(requested) = rung_from_keyword(&body.granted_rung) else {
        return Err(bad_request(format!(
            "unknown autonomy rung '{}': expected read_only, workspace_write or \
             danger_full_access",
            body.granted_rung
        )));
    };
    let escalation =
        AutonomyEscalation::request(AutonomyRung::ReadOnly, requested, &body.justification)
            .map_err(|refusal| bad_request(refusal.message()))?;

    // plan-gate §2.3: approve the plan's CONTENT before granting any
    // authority — an autonomy grant with no corresponding approved plan is
    // exactly the "rubber stamp" this control exists to prevent. Refuse the
    // whole request (400, via `grpc_status_to_http`) rather than granting a
    // rung to a plan that failed to transition.
    let approved_plan = if body.plan_id.is_empty() {
        None
    } else {
        let resp = state
            .orchestration_client
            .clone()
            .transition_plan(authenticated_session_request(
                TransitionPlanRequest {
                    plan_id: body.plan_id.clone(),
                    target_state: PlanState::Approved as i32,
                    actor: claims.user_id.clone(),
                    reason: body.justification.clone(),
                    edited_steps,
                },
                &bearer,
            )?)
            .await
            .map_err(|e| grpc_status_to_http(&e))?
            .into_inner()
            .plan;
        Some(resp.ok_or_else(|| not_found("plan not found"))?)
    };

    // In-process: this IS the gateway, so calling the coordinator directly
    // rather than dialling our own gRPC surface keeps one code path and one
    // validation. `thread_id` was resolved above, before the plan moved. (An
    // earlier revision of this call sent an empty `session_id`, which the
    // store correctly refused, making the whole grant path a no-op.)
    //
    // With the durable grant switch on (WS04 S4) the grant is Session Core's to record, in the
    // same transaction as the run update below, and this replica's memory holds no copy of it;
    // the lifecycle event follows the run's durable mode rather than this replica's flag.
    let durable_grant = state.plan_mode.durable_grant_enabled();
    let exit_request = ExitPlanModeRequest {
        request_id: format!("plan-approval-{run_id}"),
        org_id: claims.org_id.clone(),
        run_id: run_id.clone(),
        session_id: thread_id.clone(),
        granted_rung: escalation.to() as i32,
        justification: escalation.justification().to_owned(),
    };
    let exited = if durable_grant {
        crate::coordinator::handle_exit_plan_mode_durable(
            &state.plan_mode,
            &*state.publisher,
            exit_request,
            &body.plan_id,
            run_was_in_plan_mode,
        )
        .await
    } else {
        crate::coordinator::handle_exit_plan_mode(
            &state.plan_mode,
            &*state.publisher,
            exit_request,
            &body.plan_id,
        )
        .await
    }
    .map_err(|error| grpc_status_to_http(&error))?;

    // The durable half. Reported rather than swallowed: a grant the run does not
    // carry is a grant the next request will not honour, and the caller should
    // learn that here instead of by watching the agent refuse work it was just
    // authorized to do.
    //
    // With the durable switch on, the plan above is already APPROVED and an approval is
    // single-use, so this write is the only place the grant can still be made durable: it is
    // retried with a bounded backoff on a transient failure, and if it still fails the grant is
    // kept in THIS replica's memory so the approving replica keeps honouring what the human
    // approved (`durable_thread_grant` reads it back when Session Core holds none). The answer
    // stays honest (`persisted: false`); the plan is not rolled back.
    //
    // A WAIVED credential (zero retention waived for one connection) may not write a durable
    // grant: Session Core refuses a `SetRunMode` that states one. The run's mode change is not
    // a grant and is not refused, so the request carries the mode alone (no rung, no
    // justification, no plan id: everything that only feeds the thread's durable copy is
    // stripped) and the run leaves plan mode exactly as it did before durable grants existed.
    // The answer then says `persisted: false`, and this replica keeps the grant in memory.
    // Read through the resolver's own adapter, as every retention claim is. This is not the
    // posture decision (the gateway has refused a ZDR credential above and Session Core decides
    // the waiver's receipt): it only chooses what the request states.
    let waived = crate::zdr_posture::claims_view(&claims).waiver.is_some();
    let set_run_mode_request = || {
        authenticated_session_request(
            set_run_mode_request_for(
                &run_id,
                &claims.org_id,
                &escalation,
                if durable_grant { &body.plan_id } else { "" },
                waived,
            ),
            &bearer,
        )
        .map_err(|_| {
            tonic::Status::invalid_argument("verified session credential is not forwardable")
        })
    };
    let persisted = if durable_grant {
        retry_set_run_mode(&SET_RUN_MODE_RETRY_BACKOFF, || async {
            state
                .session_client
                .clone()
                .set_run_mode(set_run_mode_request()?)
                .await
        })
        .await
    } else {
        state
            .session_client
            .clone()
            .set_run_mode(set_run_mode_request().map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "verified session credential is not forwardable"})),
                )
            })?)
            .await
    };
    // The grant is on the thread in Session Core only when the write succeeded AND it stated the
    // grant (a waived credential's did not).
    let durably_recorded = persisted.is_ok() && !waived;
    // A grant Session Core now holds supersedes the copy this replica kept for an earlier approval
    // whose write failed: that copy is only a fallback (`with_local_fallback` in sse.rs), and it must
    // not keep answering for the thread when it is the narrower of the two.
    if durable_grant && durably_recorded {
        state.plan_mode.clear_grant(&claims.org_id, &thread_id);
    }
    if let Err(error) = &persisted {
        if durable_grant {
            // The status code only: the message can carry upstream text.
            tracing::warn!(
                code = ?error.code(),
                %run_id,
                "plan approval granted but the run's mode change was not recorded"
            );
        } else {
            tracing::warn!(%error, %run_id, "plan approval granted but not durably recorded");
        }
    }
    // This replica keeps the grant in memory (keyed on the run's OWN thread, `thread_id` from
    // GetRun above, never on a caller-supplied one) when nothing durable holds it and the
    // reason is not a refusal: a waived credential, whose grant is never durable, or a
    // transient failure of the write. A refusal (the caller may not, the request is invalid,
    // the run is gone) leaves no copy: a grant Session Core refused must not be honoured here.
    if durable_grant && !durably_recorded {
        let keep = waived
            || persisted
                .as_ref()
                .err()
                .is_some_and(|error| crate::coordinator::set_run_mode_failure_is_transient(error.code()));
        if keep {
            tracing::warn!(%run_id, "plan approval not durably recorded; this replica keeps the grant in memory");
            state.plan_mode.record_grant(
                &claims.org_id,
                &thread_id,
                escalation.to(),
                &body.plan_id,
            );
        }
    }
    // When the grant lapses. Durable: as Session Core recorded it, read back after the write
    // (an approval is rare, so one read is cheap and the answer is the stored one, not a
    // guess). Otherwise the in-memory store's.
    let grant_expires_at_unix = if durable_grant {
        if durably_recorded {
            crate::session_flow::thread_grant(
                &state,
                &claims.org_id,
                &thread_id,
                bearer.as_str(),
            )
            .await
            .ok()
            .and_then(|grant| grant.expires_at_unix)
        } else {
            // Not recorded in Session Core: the expiry of the copy this replica kept.
            state.plan_mode.grant_expires_at(&claims.org_id, &thread_id)
        }
    } else {
        state.plan_mode.grant_expires_at(&claims.org_id, &thread_id)
    };

    // WS14 S2: what the client does next. `execute_now` needs the grant recorded durably for this
    // plan (the durable switch on, the write landed, not waived) and an execute turn accepted here
    // (the execute switch and the approval-resume switch both on). Otherwise the grant waits for the
    // person's own next turn, as it always has.
    let continuation = crate::sse::approval_continuation(
        crate::sse::plan_execute_on_approve_enabled(),
        durable_grant && durably_recorded && !body.plan_id.is_empty(),
        state.plan_mode.execute_enabled() && state.plan_mode.approval_resume_enabled(),
    );
    // WS25-S9: the rung the run will actually hold. An enforced org rung cap clamps the grant at dispatch (and the
    // in-memory copy at record), so the answer states the clamped rung rather than the one asked for.
    let effective_rung = crate::org_agent_policy::clamp_rung(
        Some(escalation.to()),
        crate::org_agent_policy::held_rung_cap(
            crate::org_agent_policy::policy_mode(),
            crate::org_agent_policy::shared_cache(),
            &claims.org_id,
        ),
    )
    .unwrap_or(escalation.to());
    Ok(Json(json!({
        "run_id": run_id,
        "was_in_plan_mode": exited.was_active,
        "granted_rung": label(effective_rung),
        "justification": escalation.justification(),
        "persisted": durably_recorded,
        "plan": approved_plan.map(|plan| plan_value(&plan)),
        // WS14 S2a, additive: when the grant just recorded lapses (unix seconds; the
        // gateway keeps it in memory for this long and no longer), so the client can say
        // so and refuse to offer a rung that has run out. `null` when no grant is held
        // (a rung the store does not record).
        "grant_expires_at_unix": grant_expires_at_unix,
        // WS14 S2, additive: `execute_now` or `next_message` (see above).
        "continuation": continuation,
    })))
}

/// The `SetRunMode` an approval sends. For an ordinary credential it states the grant (rung,
/// justification and, when grants are durable, the plan id). For a WAIVED credential it is the
/// mode change alone: Session Core refuses a waived caller that states a grant, and that refusal
/// would otherwise take the run's mode update down with it, so every field that only feeds the
/// durable grant is left out here and the update proceeds as it did before durable grants.
fn set_run_mode_request_for(
    run_id: &str,
    org_id: &str,
    escalation: &mp_contracts::autonomy::AutonomyEscalation,
    approved_plan_id: &str,
    waived: bool,
) -> mp_contracts::model_plane::v1::SetRunModeRequest {
    if waived {
        return mp_contracts::model_plane::v1::SetRunModeRequest {
            run_id: run_id.to_owned(),
            mode: "execute".to_owned(),
            org_id: org_id.to_owned(),
            granted_rung: mp_contracts::model_plane::v1::AutonomyRung::Unspecified as i32,
            justification: String::new(),
            approved_plan_id: String::new(),
        };
    }
    mp_contracts::model_plane::v1::SetRunModeRequest {
        run_id: run_id.to_owned(),
        mode: "execute".to_owned(),
        org_id: org_id.to_owned(),
        granted_rung: escalation.to() as i32,
        justification: escalation.justification().to_owned(),
        // Named only when the grant is durable: off, the request is the one it always was.
        approved_plan_id: approved_plan_id.to_owned(),
    }
}

/// The wait before each retry of the approval's durable write (WS04 S4): the first attempt is
/// made at once, then up to three retries after about 100, 300 and 900 ms, so a Session Core
/// that restarts or sheds load for a moment does not strand a plan that is already APPROVED.
/// Bounded at 1.3 s of waiting in all.
const SET_RUN_MODE_RETRY_BACKOFF: [std::time::Duration; 3] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(300),
    std::time::Duration::from_millis(900),
];

/// Whether a failed `SetRunMode` is worth another attempt: a transient condition of Session
/// Core, never a refusal. A refusal (the caller may not, the request is invalid, the run is
/// gone) would only be refused again, and repeating it is noise.
fn set_run_mode_retryable(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Unavailable
            | tonic::Code::DeadlineExceeded
            | tonic::Code::ResourceExhausted
            | tonic::Code::Aborted
            | tonic::Code::Internal
            | tonic::Code::Unknown
    )
}

/// Run `attempt`, and on a transient failure run it again after each wait in `backoff`
/// (one retry per entry). Returns the first success, or the last failure.
async fn retry_set_run_mode<T, F, Fut>(
    backoff: &[std::time::Duration],
    mut attempt: F,
) -> Result<T, tonic::Status>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, tonic::Status>>,
{
    let mut waits = backoff.iter();
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(status) => match waits.next() {
                Some(wait) if set_run_mode_retryable(status.code()) => {
                    tokio::time::sleep(*wait).await;
                }
                _ => return Err(status),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct QueueInputRequest {
    content: String,
    /// The fresh, content-bound `thread:append` decision the BFF minted for THIS
    /// message. Present only for a Space-scoped thread — an ordinary thread's
    /// append carries none, exactly as the normal send path does.
    #[serde(default)]
    space_append_context: Option<crate::session_flow::ThreadSpaceContext>,
    /// The thread the BFF minted the append decision against, when it minted
    /// one. Checked against the stream's own thread rather than used as the
    /// append target: a decision issued for a different conversation must not be
    /// spent on this one, even though both belong to the same user.
    #[serde(default)]
    thread_id: Option<String>,
    /// The id the SPA minted for this message (the send receipt). Read only with
    /// `MODEL_GATEWAY_SEND_RECEIPTS` on, where it names the message to the transcript and to the
    /// owner's queue; the SPA sends it whatever the switch says, so this is ignored when off.
    #[serde(default, alias = "clientMessageId")]
    client_message_id: Option<String>,
}

/// The answer to a queued message that was not accepted, for every outcome but `Queued`.
///
/// One mapping for both orders of the handler, so a refusal reads the same with
/// `MODEL_GATEWAY_SEND_RECEIPTS` on and off.
fn queue_refusal(
    request_id: &str,
    outcome: &crate::queued_input::EnqueueOutcome,
    claimed_thread_id: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    use crate::queued_input::{EnqueueOutcome, MAX_QUEUED_CHARS, MAX_QUEUED_MESSAGES};

    match outcome {
        EnqueueOutcome::Queued { .. } => unreachable!("an accepted message is not a refusal"),
        EnqueueOutcome::ThreadMismatch { streaming_thread_id } => {
            tracing::warn!(
                %request_id,
                streaming = %streaming_thread_id,
                claimed = claimed_thread_id.unwrap_or_default(),
                "queued message named a thread other than the stream's; refusing"
            );
            (
                StatusCode::CONFLICT,
                serde_json::json!({
                    "request_id": request_id,
                    "queued": false,
                    "error": "this run is not streaming that thread",
                }),
            )
        }
        EnqueueOutcome::NoActiveStream => (
            StatusCode::NOT_FOUND,
            serde_json::json!({
                "request_id": request_id,
                "queued": false,
                "error": "no active stream",
                // Names the client's next move: this is the case where the
                // run already finished, and an ordinary send is correct.
                "resend_as_new_turn": true,
            }),
        ),
        EnqueueOutcome::Full { pending } => (
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({
                "request_id": request_id,
                "queued": false,
                "error": "too many messages already waiting for this run",
                "pending": pending,
                "max_pending": MAX_QUEUED_MESSAGES,
            }),
        ),
        EnqueueOutcome::TooLong { chars } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            serde_json::json!({
                "request_id": request_id,
                "queued": false,
                "error": "message is too long to deliver mid-run",
                "chars": chars,
                "max_chars": MAX_QUEUED_CHARS,
            }),
        ),
        EnqueueOutcome::Empty => (
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "request_id": request_id,
                "queued": false,
                "error": "content is required",
            }),
        ),
    }
}

/// The 503 for a bus that could not say whether the message was queued: retryable, and nothing is
/// claimed either way.
fn run_control_unavailable(request_id: &str) -> (StatusCode, serde_json::Value) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        serde_json::json!({
            "request_id": request_id,
            "queued": false,
            "error": "run_control_unavailable",
            "retryable": true,
        }),
    )
}

/// Deliver a message the user typed WHILE a run was streaming.
///
/// Two things happen:
///
/// 1. The message is queued for the run's next tool-round boundary, where it arrives with the
///    classify-and-continue pause (`queued_input`). With the run-control bus on, a run that is
///    streaming on another replica is asked to queue it, so the message lands wherever the user
///    reached the gateway.
/// 2. It is persisted to the thread the *stream* is on, so the transcript holds what the user
///    said whether or not the run lives long enough to read it.
///
/// The ledger (2026-08-24) says persist first; this handler has always queued first, and with
/// `MODEL_GATEWAY_SEND_RECEIPTS` off it still does: a message that is queued and then fails to
/// persist says so (`persisted: false`). With the switch on the order is the ledger's, and the
/// message carries a receipt: see [`queue_input_with_receipts`] (WS04-S6, decision D3).
///
/// The thread id comes from the stream's own registration, never from the request: a client that
/// could name the thread could aim a message at a conversation it is not streaming. A thread the
/// client does claim is only compared with it, and refused before anything is queued.
///
/// Nothing leaves this replica before the ZDR and waived gate: `bus_message` is the only way the
/// text reaches the bus, and it refuses those tokens again.
async fn invoke_queue_input(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(request_id): Path<String>,
    Json(body): Json<QueueInputRequest>,
) -> Response {
    use crate::queued_input::EnqueueOutcome;

    // Queued input changes a run's durable state: a ZDR or a waived token cannot do that.
    if let Err(rejection) = require_non_zdr_durable_mutation(&claims) {
        return rejection.into_response();
    }

    if crate::queued_input::send_receipts_enabled() {
        let persist = |thread_id: String, conversation_only: bool, client_message_id: String| {
            let (state, bearer, body) = (&state, &bearer, &body);
            async move {
                crate::session_flow::append_queued_user_message(
                    state,
                    &thread_id,
                    body.content.trim(),
                    bearer,
                    body.space_append_context.as_ref(),
                    conversation_only,
                    Some(&client_message_id),
                )
                .await
            }
        };
        return queue_input_with_receipts(&state, &claims, &request_id, &body, persist).await;
    }

    // One id for this attempt: a request repeated because its reply was lost is recognised by
    // the owner and queues nothing twice.
    let op_id = format!("{:016x}", rand::random::<u64>());
    let msg = match crate::queued_input::bus_message(
        &claims,
        &request_id,
        &body.content,
        body.thread_id.as_deref(),
        &op_id,
    ) {
        Ok(msg) => msg,
        Err(refusal) => return crate::zdr_posture::http(&refusal).into_response(),
    };

    // Ownership, the thread claim and the bounds are all decided before anything is written, so
    // a refused message leaves no trace and a mismatched owner learns nothing about which runs
    // exist (same 404 as an inactive stream — see `enqueue_checked`).
    let outcome = match state.run_control.queue_input(&state.queued_inputs, &msg).await {
        QueueRoute::Local(outcome) | QueueRoute::Peer(outcome) => outcome,
        QueueRoute::NoOwner => EnqueueOutcome::NoActiveStream,
        // Neither queued nor refused is known, so say so and persist nothing: the client retries.
        QueueRoute::Unavailable => {
            let (status, body) = run_control_unavailable(&request_id);
            return (status, Json(body)).into_response();
        }
    };
    // `retrieval_scope` rides on `EnqueueOutcome::Queued` for parity with
    // `conversation_only` (chat-wave2 scope spec §2.3's queued_input.rs
    // note), but is not threaded into `append_queued_user_message` here: the
    // in-flight stream already enforces it from its OWN `retrieval_scope`
    // (computed once at stream start, see `sse.rs`), and a queued message's
    // durable metadata is not part of this item's "shown in the receipt /
    // reflected in citations" surface the way the turn's own persistence is.
    let (pending, thread_id, conversation_only) = match outcome {
        EnqueueOutcome::Queued { pending, thread_id, conversation_only, .. } => (pending, thread_id, conversation_only),
        refusal => {
            let (status, body) = queue_refusal(&request_id, &refusal, body.thread_id.as_deref());
            return (status, Json(body)).into_response();
        }
    };

    // Accepted and queued. The append failing now is a durability problem, not
    // a delivery one: the run will still read the message, so say both plainly
    // rather than pretending either half succeeded.
    let persisted = crate::session_flow::append_queued_user_message(
        &state,
        &thread_id,
        body.content.trim(),
        &bearer,
        body.space_append_context.as_ref(),
        conversation_only,
        None,
    )
    .await;
    if let Err(error) = &persisted {
        tracing::warn!(
            %error,
            %request_id,
            %thread_id,
            "mid-run message queued but not durably recorded"
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "request_id": request_id,
            "queued": true,
            "pending": pending,
            "persisted": persisted.is_ok(),
        })),
    )
        .into_response()
}

/// [`invoke_queue_input`] with durable send receipts (`MODEL_GATEWAY_SEND_RECEIPTS` on, WS04 S6).
///
/// The ledger's order, persist then deliver, which closes decision D3: the message is written to the
/// transcript under its `client_message_id` before it can reach the model, so a message the run
/// read is never missing from the thread, and a message that cannot be recorded is not delivered.
/// Every answer names the message (`client_message_id`, and `message_id` once it is stored) and
/// says whether it is stored (`persisted`), which is what the SPA's tray reports against.
///
/// 1. The id is the SPA's, or the gateway's own for a client from before receipts; the bounds are
///    checked before anything is written, so a message refused for its shape leaves no trace.
/// 2. The thread comes from the *stream's* registration, on this replica or (bus on) from its owner,
///    never from the request. No live stream is a 404 with `resend_as_new_turn` and nothing written:
///    the resend is the one send, and carries the same id.
/// 3. The message is stored. A repeat of an id the thread holds answers with the first row
///    (`deduplicated`) and writes nothing; the same id with other content is a 409.
/// 4. The message is queued, with the id as the owner's `op_id`, so a repeat after a lost reply is
///    queued once. A run that ended between the store and the queue answers 404
///    `resend_as_new_turn` with `persisted: true`: the resend carries the id and finds its row.
///
/// Nothing leaves this replica before the ZDR and waived gate, which the caller ran and
/// `bus_message` runs again.
// One ordered sequence of refusals and effects; splitting it would scatter the order it exists to state.
#[allow(clippy::too_many_lines)]
async fn queue_input_with_receipts<P, Fut>(
    state: &AppState,
    claims: &Claims,
    request_id: &str,
    body: &QueueInputRequest,
    persist: P,
) -> Response
where
    P: Fn(String, bool, String) -> Fut,
    Fut: std::future::Future<Output = Result<crate::session_flow::AppendReceipt, tonic::Status>>,
{
    use crate::queued_input::{bounds_refusal, stream_thread, EnqueueOutcome, StreamThread};

    let client_message_id = match body
        .client_message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) if crate::queued_input::valid_client_message_id(id) => id.to_owned(),
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "request_id": request_id,
                    "queued": false,
                    "error": "client_message_id is not a valid message id",
                })),
            )
                .into_response();
        }
        // A client from before receipts names nothing: the gateway names the message, so the
        // answer carries an id either way and the owner's repeat check has one.
        None => format!("msg-{:032x}", rand::random::<u128>()),
    };
    let msg = match crate::queued_input::bus_message(
        claims,
        request_id,
        &body.content,
        body.thread_id.as_deref(),
        &client_message_id,
    ) {
        Ok(msg) => msg,
        Err(refusal) => return crate::zdr_posture::http(&refusal).into_response(),
    };
    // Every answer past this point names the message and says whether it is stored.
    let answer = |(status, mut answer): (StatusCode, serde_json::Value),
                  stored: Option<&crate::session_flow::AppendReceipt>| {
        answer["client_message_id"] = serde_json::json!(client_message_id);
        answer["persisted"] = serde_json::json!(stored.is_some());
        if let Some(stored) = stored {
            answer["message_id"] = serde_json::json!(stored.message_id);
            answer["deduplicated"] = serde_json::json!(stored.deduplicated);
        }
        (status, Json(answer)).into_response()
    };

    if let Some(refusal) = bounds_refusal(&body.content) {
        return answer(queue_refusal(request_id, &refusal, None), None);
    }
    let (thread_id, known_scope) = match stream_thread(
        &state.queued_inputs,
        &state.run_control,
        request_id,
        &claims.org_id,
        &claims.user_id,
    )
    .await
    {
        StreamThread::Here { thread_id, conversation_only } => (thread_id, Some(conversation_only)),
        StreamThread::Peer { thread_id, conversation_only } => (thread_id, conversation_only),
        StreamThread::Gone => {
            return answer(queue_refusal(request_id, &EnqueueOutcome::NoActiveStream, None), None);
        }
        // The stream may be live: neither stored nor refused is known, so say so and write nothing.
        StreamThread::Unavailable => return answer(run_control_unavailable(request_id), None),
    };
    if let Some(claimed) = body.thread_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        if claimed != thread_id {
            let mismatch = EnqueueOutcome::ThreadMismatch { streaming_thread_id: thread_id };
            return answer(queue_refusal(request_id, &mismatch, Some(claimed)), None);
        }
    }

    // A stream on this replica states its own source scope, and so does its owner's probe answer on
    // another replica. Only an owner that predates the field leaves the scope unknown: its message is
    // then tried as an ordinary one, and a conversation-only thread's refusal (the one precondition a
    // wrong guess can fail) is answered by the same message under the conversation-only scope.
    let mut stored = persist(thread_id.clone(), known_scope.unwrap_or(false), client_message_id.clone()).await;
    if known_scope.is_none()
        && matches!(&stored, Err(status) if status.code() == tonic::Code::FailedPrecondition)
    {
        stored = persist(thread_id.clone(), true, client_message_id.clone()).await;
    }
    let stored = match stored {
        Ok(stored) => stored,
        Err(status) => {
            tracing::warn!(
                code = ?status.code(),
                %request_id,
                %thread_id,
                "mid-run message could not be recorded; not delivered"
            );
            let (code, error, retryable) = match status.code() {
                tonic::Code::AlreadyExists => (StatusCode::CONFLICT, "client_message_id_conflict", false),
                tonic::Code::PermissionDenied | tonic::Code::FailedPrecondition => {
                    (StatusCode::FORBIDDEN, "message_not_recorded", false)
                }
                tonic::Code::InvalidArgument => (StatusCode::BAD_REQUEST, "message_not_recorded", false),
                _ => (StatusCode::SERVICE_UNAVAILABLE, "message_not_recorded", true),
            };
            return answer(
                (
                    code,
                    serde_json::json!({
                        "request_id": request_id,
                        "queued": false,
                        "error": error,
                        "retryable": retryable,
                    }),
                ),
                None,
            );
        }
    };

    match state.run_control.queue_input(&state.queued_inputs, &msg).await {
        QueueRoute::Local(EnqueueOutcome::Queued { pending, .. })
        | QueueRoute::Peer(EnqueueOutcome::Queued { pending, .. }) => answer(
            (
                StatusCode::ACCEPTED,
                serde_json::json!({
                    "request_id": request_id,
                    "queued": true,
                    "pending": pending,
                }),
            ),
            Some(&stored),
        ),
        QueueRoute::Local(refusal) | QueueRoute::Peer(refusal) => {
            answer(queue_refusal(request_id, &refusal, body.thread_id.as_deref()), Some(&stored))
        }
        // The run ended between the store and the queue: the message is in the thread, and the
        // resend carries its id.
        QueueRoute::NoOwner => answer(
            queue_refusal(request_id, &EnqueueOutcome::NoActiveStream, None),
            Some(&stored),
        ),
        // Stored, and the bus could not say whether it was queued: a retry with the same id stores
        // nothing new and is queued once.
        QueueRoute::Unavailable => answer(run_control_unavailable(request_id), Some(&stored)),
    }
}

#[derive(Debug, Serialize)]
struct ModelDescriptor {
    id: String,
    provider: String,
    modality: String,
    streaming: bool,
    features: Vec<String>,
    /// Strongest privacy tier this model's serving provider can honor
    /// (`unspecified|global|eu_resident|zdr_contractual|sovereign`).
    privacy_tier: &'static str,
    /// Declared residency label ("eu"/"norway"); empty when the provider makes
    /// no residency commitment so absence never renders as a guarantee.
    residency: String,
    /// Reasoning-effort levels the model accepts, weakest first, as canonical
    /// names. Left out when the model advertises none, so a deploy with the
    /// ladder off serves the same JSON as before.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    effort_levels: Vec<String>,
    /// The context window in tokens from the declared record. Left out when no sourced
    /// row names the model, so an unknown model serves the JSON it served before.
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window_tokens: Option<u32>,
    /// The synchronous output ceiling in tokens this model is served at. Left out when
    /// unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    /// `ga`, `preview` or `deprecated`. Left out when no sourced row names the model.
    #[serde(skip_serializing_if = "String::is_empty")]
    lifecycle: String,
    /// The catalogue's list price for a chat model, behind
    /// `MODEL_GATEWAY_CATALOGUE_PRICES`. Left out when there is none, so a deploy
    /// with the switch off serves the same JSON as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<crate::catalogue_prices::ModelPrice>,
    /// What the provider states about this model's request data (`data_use` on inference-core's
    /// `ModelInfo`). Left out when nothing is stated, so a deploy without an attested deployment
    /// serves the JSON it served before (EP-9 C5).
    #[serde(skip_serializing_if = "Option::is_none")]
    data_use: Option<DataUseDescriptor>,
}

/// The data-use statements one model's provider makes (`data_use` on `/v1/models`). A field is
/// left out unless a source states it: an absent field is "not stated", never "no".
#[derive(Debug, Serialize)]
struct DataUseDescriptor {
    #[serde(skip_serializing_if = "Option::is_none")]
    trains_on_inputs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retention_days: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zdr_attested: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    statement_ref: String,
}

/// Wire-numeric → tier label. Mirrors inference-core's `PrivacyTier::label`;
/// unknown numerics degrade to "unspecified" here because ListModels is pure
/// disclosure — inference-core remains the fail-closed enforcement point.
pub(crate) fn privacy_tier_label(value: i32) -> &'static str {
    match mp_contracts::model_plane::v1::PrivacyTier::try_from(value) {
        Ok(mp_contracts::model_plane::v1::PrivacyTier::Unspecified) => "unspecified",
        Ok(mp_contracts::model_plane::v1::PrivacyTier::Global) => "global",
        Ok(mp_contracts::model_plane::v1::PrivacyTier::EuResident) => "eu_resident",
        Ok(mp_contracts::model_plane::v1::PrivacyTier::ZdrContractual) => "zdr_contractual",
        Ok(mp_contracts::model_plane::v1::PrivacyTier::Sovereign) => "sovereign",
        Err(_) => "unspecified",
    }
}

/// A token count the record declares, or `None` when it declares none, so the descriptor
/// leaves the field out rather than stating zero tokens.
fn known_tokens(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

#[derive(Debug, Serialize)]
struct ListModelsHttpResponse {
    models: Vec<ModelDescriptor>,
    /// What the organisation's model policy did to this catalogue. Left out
    /// when no policy is applied, so a deploy with entitlement off serves the
    /// same JSON as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlement: Option<ModelEntitlementMeta>,
    /// Whether side tasks ask the platform role table (`MODEL_GATEWAY_ROLE_ROUTING`, EP-13 S4).
    /// Left out while off, so a deploy with the switch off serves the same JSON as before.
    #[serde(skip_serializing_if = "is_false")]
    role_routing: bool,
    /// What a chat turn may attach, enforced by the stream route. Left out when
    /// the attachment contract is off, so the client keeps its own constants.
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_policy: Option<crate::attachment_policy::DeclaredAttachmentPolicy>,
}

/// The serde skip predicate for a flag that is left out while it is off.
fn is_false(value: &bool) -> bool {
    !*value
}

/// `ModelAccessMeta` as the catalogue's sibling field: counts, a revision and
/// policy defaults, never a model list or any content.
#[derive(Debug, Serialize)]
struct ModelEntitlementMeta {
    /// `none`, `org`, `role` or `member_not_found`.
    state: String,
    enforced: bool,
    revision: i64,
    restricted: bool,
    /// Chat models the policy removed from `models`.
    hidden_count: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    default_model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    default_effort: String,
    /// The highest reasoning effort allowed, a canonical name.
    #[serde(skip_serializing_if = "String::is_empty")]
    max_effort: String,
}

/// chat-parity §2 — list available models with their per-model feature
/// families so the client can gate the opt-in `features[]` (reasoning, tools,
/// vision, image, …) per selected model. Proxies inference-core `ListModels`
/// (the capability owner) — no model catalog is duplicated in the gateway.
async fn list_models(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
) -> Result<Json<ListModelsHttpResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::ListModelsRequest;
    let resp = state
        .inference_client
        .clone()
        .list_models(authenticated_inference_request(
            ListModelsRequest::default(),
            &inference_bearer,
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("inference-core list_models failed: {}", e.message()),
                })),
            )
        })?
        .into_inner();

    // One catalogue snapshot prices the whole listing, and none is taken while the switch is off.
    let rates = crate::catalogue_prices::snapshot_for_listing(&state.pricing).await;
    let models: Vec<ModelDescriptor> = resp
        .models
        .into_iter()
        .map(|m| {
            let price = rates.as_ref().and_then(|rates| {
                crate::catalogue_prices::model_price(rates, &m.id, &m.provider, &m.modality)
            });
            ModelDescriptor {
                id: m.id,
                provider: m.provider,
                modality: m.modality,
                streaming: m.streaming,
                features: m.features,
                privacy_tier: privacy_tier_label(m.privacy_tier),
                residency: m.residency,
                effort_levels: m.effort_levels,
                context_window_tokens: known_tokens(m.context_window_tokens),
                max_output_tokens: known_tokens(m.max_output_tokens),
                lifecycle: m.lifecycle,
                price,
                data_use: m.data_use.map(|data_use| DataUseDescriptor {
                    trains_on_inputs: data_use.trains_on_inputs,
                    retention_days: data_use.retention_days,
                    zdr_attested: data_use.zdr_attested,
                    statement_ref: data_use.statement_ref,
                }),
            }
        })
        .collect();
    let entitlement = resp.access.map(|access| ModelEntitlementMeta {
        state: access.state,
        enforced: access.enforced,
        revision: access.revision,
        restricted: access.restricted,
        hidden_count: access.hidden_count,
        default_model: access.default_model,
        default_effort: access.default_effort,
        max_effort: access.max_effort,
    });

    let attachment_policy = crate::attachment_policy::AttachmentPolicy::from_env().declared(
        models
            .iter()
            .any(|model| model.features.iter().any(|feature| feature == "vision")),
    );

    Ok(Json(ListModelsHttpResponse {
        models,
        entitlement,
        attachment_policy,
        role_routing: crate::role_routing::role_routing_enabled(),
    }))
}

/// `POST /v1/models/access`: what the caller's organisation allows, for the Frontend
/// BFF's zero-retention write guard. The body asks about a draft policy and, with
/// `include_catalogue`, one verdict per chat model. The organisation is the verified
/// caller's, never the body's. inference-core decides; this route only maps the
/// question from JSON and the answer back to it.
async fn post_model_access(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let question = crate::model_access::question_from_json(&body, &claims.org_id)?;
    let answer = state
        .inference_client
        .clone()
        .get_model_access(authenticated_inference_request(question, &inference_bearer))
        .await
        .map_err(|status| crate::model_access::route_error(&status))?
        .into_inner();
    Ok(Json(crate::model_access::answer_json(&answer)))
}

#[derive(Debug, Deserialize)]
struct CreateDocumentHttpRequest {
    title: String,
    content: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default, rename = "type")]
    doc_type: Option<String>,
    #[serde(default)]
    zdr: bool,
}

#[derive(Debug, Serialize)]
struct CreateDocumentHttpResponse {
    document_id: String,
    status: String,
}

/// chat-parity §2 — upload a document into Data Plane v2 (the document/ingest
/// owner) so it becomes retrievable by the RAG path. Org scope comes from the
/// authenticated claims. No document store is duplicated in the gateway.
async fn create_document(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedBearer,
    Json(req): Json<CreateDocumentHttpRequest>,
) -> Result<Json<CreateDocumentHttpResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::dataplane::documents_v2::CreateDocumentRequest;

    if req.content.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "content is required"})),
        ));
    }
    if crate::zdr_posture::zdr_or_refuse(&claims, &crate::zdr_posture::Route::plain(req.zdr))? {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.json_body()),
        ));
    }

    let request = crate::retrieval::authorize(
        tonic::Request::new(CreateDocumentRequest {
            org_id: claims.org_id.clone(),
            source: req.source.unwrap_or_else(|| "chat-upload".to_owned()),
            r#type: req.doc_type.unwrap_or_else(|| "text".to_owned()),
            title: req.title,
            content: req.content,
            ..Default::default()
        }),
        &bearer,
    )
    .map_err(|error| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": error.message() })),
        )
    })?;
    let resp = state
        .document_client
        .clone()
        .create_document(request)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("data-plane create_document failed: {}", e.message()),
                })),
            )
        })?
        .into_inner();

    let document = resp.document.unwrap_or_default();
    Ok(Json(CreateDocumentHttpResponse {
        document_id: document.document_id,
        status: document.status,
    }))
}

#[derive(Debug, Serialize)]
struct ThreadMessage {
    /// Durable id of this message, straight from session-core.
    ///
    /// Exposed so a client can name one earlier turn: message pinning sends
    /// ids, and a positional index would point at a different message as soon
    /// as the thread grew. Omitted for rows written before the conversation
    /// read returned ids, which is why the field is skipped when empty rather
    /// than sent as "".
    #[serde(skip_serializing_if = "String::is_empty")]
    message_id: String,
    role: String,
    content: String,
    /// The persona this turn answered as, when it had one. Identity history
    /// from session-core's record — never an authority claim.
    #[serde(skip_serializing_if = "String::is_empty")]
    agent_name: String,
    /// Which subject wrote this turn, from session-core's at-the-time record.
    /// Present on user turns in a shared Space; omitted when empty so a reader
    /// can tell "nobody recorded an author" from "the author is the empty
    /// string". Presentation identity, never an authority claim.
    #[serde(skip_serializing_if = "String::is_empty")]
    author_subject_id: String,
    /// `post` for a human message written to the room; omitted for a conversation turn
    /// (`turn`, or a row from before post kinds), so a turn keeps the shape it always had.
    #[serde(skip_serializing_if = "String::is_empty")]
    post_kind: String,
    /// When session-core stored this turn, as RFC3339. A room shows a time beside each
    /// message. Omitted when session-core sent none, so a reader never sees a made-up time.
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    /// The turn's persisted evidence, flattened so its keys (today
    /// `grounding`) sit directly on the message the way the live SSE path
    /// delivers them. Omitted entirely for turns recorded before metadata was
    /// persisted, or for paths that ground nothing.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Map<String, Value>>,
    /// Where this turn sits among the versions of its position (WS17 S8). Present
    /// only on a versioned thread, so every other transcript keeps its shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<ThreadMessageVersion>,
    /// The typed room entry of a human post (WS15): its own field, never flattened with the free
    /// metadata. An unknown future kind arrives as `{kind, version}` only. Omitted when none.
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<ThreadMessageEntry>,
    /// The mentions recorded on a post (WS08), with the wake state derived from the newest run per
    /// (post, agent). Ids and closed words only; omitted when the row has none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mentions: Vec<ThreadMessageMention>,
    /// The latest applied edit or deletion of this message (WS12-S6), flattened so its keys sit on
    /// the turn. Omitted for a message that was never revised, so every other transcript keeps its
    /// shape. `content` is already the effective text (empty for a deleted turn).
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    revision: Option<ThreadMessageRevision>,
}

/// The revision state of a turn as the transcript serialises it (WS12-S6). Ids, closed words and
/// a time: never the original text.
#[derive(Debug, Serialize)]
struct ThreadMessageRevision {
    /// `edited` or `deleted`.
    revision_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    revised_at: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    revised_by_subject_id: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    revision_no: u32,
    /// Set only on a deleted turn that an agent run answered before the deletion.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    agent_followed_up: bool,
}

/// The revision fields of a conversation row, or `None` when it was never revised.
fn thread_message_revision(
    m: &mp_contracts::model_plane::v1::SessionMessage,
) -> Option<ThreadMessageRevision> {
    if m.revision_state.is_empty() {
        return None;
    }
    Some(ThreadMessageRevision {
        revision_state: m.revision_state.clone(),
        revised_at: message_created_at(m.revised_at.as_ref()),
        revised_by_subject_id: m.revised_by_subject_id.clone(),
        revision_no: m.revision_no,
        agent_followed_up: m.agent_followed_up,
    })
}

/// One recorded mention of a post as the transcript serialises it (WS08).
#[derive(Debug, Serialize)]
struct ThreadMessageMention {
    subject_id: String,
    kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    wake_state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
    ordinal: u32,
}

/// A typed room entry as the transcript serialises it (WS15-S6; stored and validated by WS15-S9).
#[derive(Debug, Serialize)]
struct ThreadMessageEntry {
    kind: String,
    version: u32,
    /// Present only for the `link_previews` kind.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    link_previews: Vec<ThreadMessageLinkPreview>,
}

#[derive(Debug, Serialize)]
struct ThreadMessageLinkPreview {
    url: String,
    final_url: String,
    host: String,
    title: String,
    description: String,
    site_name: String,
    fetched_at: String,
}

fn thread_message_entry(
    entry: Option<mp_contracts::model_plane::v1::MessageEntry>,
) -> Option<ThreadMessageEntry> {
    let entry = entry.filter(|entry| !entry.kind.is_empty())?;
    Some(ThreadMessageEntry {
        kind: entry.kind,
        version: entry.version,
        link_previews: entry
            .link_previews
            .into_iter()
            .map(|preview| ThreadMessageLinkPreview {
                url: preview.url,
                final_url: preview.final_url,
                host: preview.host,
                title: preview.title,
                description: preview.description,
                site_name: preview.site_name,
                fetched_at: timestamp_to_rfc3339(preview.fetched_at),
            })
            .collect(),
    })
}

/// A turn's place in the message tree, as the version switcher reads it.
#[derive(Debug, Serialize)]
struct ThreadMessageVersion {
    /// The turn this one continues. Empty for the first turn of a thread.
    #[serde(skip_serializing_if = "String::is_empty")]
    parent_message_id: String,
    /// 1-based place among the turns that share the parent.
    sibling_ordinal: u32,
    /// How many turns share the parent, this one included.
    sibling_count: u32,
    /// Whether this turn is on the active path. Only a read with `include_inactive`
    /// (or a versioned thread) ever says false.
    on_active_path: bool,
    /// The ids of the turns that share this turn's parent, in order: the versions a switch can move to (WS17
    /// S9). Present only on a versioned transcript read with versions on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    siblings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ListThreadMessagesResponse {
    thread_id: String,
    messages: Vec<ThreadMessage>,
    /// Set only on a paged read (WS09 S8): whether older turns exist before this
    /// page. Absent on the unpaged read, which stays byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    has_more: Option<bool>,
    /// Set only on a read that asked for the first turn (`include_first`, WS15-S6) while
    /// `SPACE_TIMELINE_API` is on: how many readable turns lie between the first turn and the tail
    /// window. When positive, `messages[0]` is the first turn and the rest is the contiguous tail.
    #[serde(skip_serializing_if = "Option::is_none")]
    omitted_before: Option<u32>,
    /// Set only when this gateway runs with message versions on (WS17 S8), so the
    /// client can offer the version switcher. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    versions_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListThreadsQuery {
    limit: Option<u32>,
    space_id: Option<String>,
    /// See session-core's `ListThreadsRequest.origin` — filters to threads a
    /// surface itself created (e.g. "chat"), rather than every unscoped
    /// thread regardless of which surface created it.
    origin: Option<String>,
    /// Control's shared-Space read authority, relayed verbatim. The gateway
    /// does not inspect or mint it: session-core is the verifier, and a
    /// gateway that could interpret this token would be a second opinion about
    /// who may read a room.
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
    /// Keyset cursor from a previous page's `next_cursor`. Opaque here:
    /// session-core decodes it and refuses one it did not issue.
    cursor: Option<String>,
    /// Lists one thread by id, owner-bound like the rest of the listing.
    thread_id: Option<String>,
    /// `active` (the default), `archived` or `all`.
    archived: Option<String>,
    /// Lists only the owner's starred threads. Session Core applies the filter
    /// inside the owner-bound query, so a starred listing pages like any other.
    starred_only: Option<bool>,
}

/// The wire name of a derived attention state (WS16 S1). `None` for the unspecified value, which
/// is what a replica with the switch off returns, so the field is omitted and the response is
/// unchanged. A number this build does not name reads the same way.
fn attention_state_name(state: i32) -> Option<&'static str> {
    use mp_contracts::model_plane::v1::AttentionState;
    match AttentionState::try_from(state).ok()? {
        AttentionState::Unspecified => None,
        AttentionState::NeedsApproval => Some("needs_approval"),
        AttentionState::NeedsInput => Some("needs_input"),
        AttentionState::PlanReady => Some("plan_ready"),
        AttentionState::Working => Some("working"),
        AttentionState::Completed => Some("completed"),
        AttentionState::Failed => Some("failed"),
        AttentionState::Archived => Some("archived"),
        AttentionState::Idle => Some("idle"),
    }
}

/// The wire name of an attention reason (WS16 S1): the pending row behind a waiting state, or
/// `goal` for a thread goal that stopped (EP-7 S5). Tag 4 is reserved for the parked loop of WS03,
/// so this build names nothing for it.
fn attention_reason_name(reason: i32) -> Option<&'static str> {
    use mp_contracts::model_plane::v1::AttentionReason;
    match AttentionReason::try_from(reason).ok()? {
        AttentionReason::Unspecified => None,
        AttentionReason::Approval => Some("approval"),
        AttentionReason::Plan => Some("plan"),
        AttentionReason::Input => Some("input"),
        AttentionReason::Goal => Some("goal"),
    }
}

#[derive(Debug, Deserialize)]
struct ListAttentionQuery {
    /// Page size, 1 to 200. Absent or out of range reads as 200 in Session Core.
    limit: Option<i32>,
}

#[derive(Debug, Serialize)]
struct AttentionItemResponse {
    thread_id: String,
    /// Omitted for a state this build does not name, rather than guessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    /// When the state began, RFC 3339. Omitted when the state has no start.
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    /// Whether the thread holds an assistant reply the owner has not read.
    unseen: bool,
    /// The thread's latest run, for a deep link. Omitted when the thread has no run.
    #[serde(skip_serializing_if = "String::is_empty")]
    run_id: String,
}

/// The counts behind the attention badge.
#[derive(Debug, Serialize)]
struct AttentionCountsResponse {
    needs_you: i32,
    working: i32,
    unseen: i32,
}

#[derive(Debug, Serialize)]
struct ListAttentionResponse {
    /// False while the attention switch is off; the items and counts are then empty.
    enabled: bool,
    /// Whether the live stream is served (WS16 S5). Omitted while it is off, so the listing reads as
    /// it did before the stream existed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    items: Vec<AttentionItemResponse>,
    counts: AttentionCountsResponse,
}

#[derive(Debug, Serialize)]
struct MarkThreadSeenResponse {
    thread_id: String,
}

#[derive(Debug, Serialize)]
struct ThreadSummaryResponse {
    thread_id: String,
    session_key: String,
    title: String,
    preview: String,
    created_at: String,
    updated_at: String,
    pinned: bool,
    space_id: String,
    /// Who owns this thread — in a room, who started the post. Sent so a
    /// shared listing can attribute a post before its transcript is fetched.
    #[serde(skip_serializing_if = "String::is_empty")]
    owner_subject_id: String,
    latest_run_id: String,
    latest_run_status: String,
    latest_run_updated_at: Option<String>,
    /// Which surface created this thread — see session-core's
    /// `ThreadSummary.origin`. Relayed so a caller can defensively re-check
    /// scope on the response rather than trusting the request-side filter alone.
    origin: String,
    /// Persona spec — the per-thread override of the global persona switch,
    /// list-level so the sidebar/composer can show current state without
    /// opening the context inspector. `optional bool` on the wire
    /// (`ThreadSummary.persona_enabled` in `sessions.proto`) defaults to
    /// `true` here for a pre-persona session-core replica during a rolling
    /// deploy — fail open, never silently disable persona fleet-wide.
    persona_enabled: bool,
    /// The thread this one was forked from; omitted for a thread that was not forked.
    #[serde(skip_serializing_if = "String::is_empty")]
    forked_from_thread_id: String,
    /// Set when the owner archived the thread; omitted for an active thread.
    #[serde(skip_serializing_if = "String::is_empty")]
    archived_at: String,
    /// Whether the owner starred the thread.
    starred: bool,
    /// Whether the thread has a reply the owner has not read yet. This is the one
    /// unread definition (build plan DEC-11); the sidebar dot reads it as is.
    unread: bool,
    /// The derived attention state (WS16 S1), as its wire name. Omitted while the attention
    /// switch is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    attention_state: Option<&'static str>,
    /// When the attention state began, RFC 3339. Omitted when the state has no start.
    #[serde(skip_serializing_if = "Option::is_none")]
    attention_since: Option<String>,
    /// Whether the thread has an unread reply. Present only with an attention state.
    #[serde(skip_serializing_if = "Option::is_none")]
    attention_unseen: Option<bool>,
    /// Replies after the first message, under the reader's audience ceiling (Session Core's
    /// count); omitted when zero so a thread without replies keeps its shape.
    #[serde(skip_serializing_if = "is_zero_u32")]
    reply_count: u32,
    /// Who wrote the latest human row; omitted when Session Core names none.
    #[serde(skip_serializing_if = "String::is_empty")]
    last_author_subject_id: String,
    /// Thread activity summary (WS15-S6): present only on a timeline window, omitted from every
    /// other listing so those keep their shape. `last_message_seq` is the window's keyset position.
    #[serde(skip_serializing_if = "is_zero_u64")]
    last_message_seq: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    recent_author_subject_ids: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    last_role: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    turn_count: u32,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Serialize)]
struct ListThreadsResponse {
    threads: Vec<ThreadSummaryResponse>,
    /// Omitted on the last page, so a caller that never pages sees the shape it
    /// always did.
    #[serde(skip_serializing_if = "String::is_empty")]
    next_cursor: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateThreadPresentationBody {
    title: Option<String>,
    preview: Option<String>,
    pinned: Option<bool>,
    /// Persona spec — the per-thread override. `None` means "leave as is",
    /// same tri-state contract as `pinned`.
    #[serde(default)]
    persona_enabled: Option<bool>,
    /// Restore an archived thread (`false`). Session Core refuses `true`:
    /// archiving goes through `POST /v1/threads/{id}/archive`.
    #[serde(default)]
    archived: Option<bool>,
    /// Star state. `None` leaves it as it is.
    #[serde(default)]
    starred: Option<bool>,
    /// Unread state: `false` marks the thread read, `true` marks it unread.
    /// `None` leaves it as it is.
    #[serde(default)]
    unread: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateThreadPresentationResponse {
    thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveThreadResponse {
    thread_id: String,
    archived_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveThreadsResponse {
    archived_count: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteThreadResponse {
    thread_id: String,
    deleted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteThreadsResponse {
    deleted_count: u32,
}

#[derive(Debug, Deserialize)]
struct DeleteSpaceThreadsRequestBody {
    org_id: String,
    space_id: String,
    owner_principal_id: String,
    deletion_request_id: String,
}

#[derive(Debug, Serialize)]
struct DeleteSpaceThreadsResponseBody {
    request_id: String,
    owner_plane: &'static str,
    owner_outcome: &'static str,
    deleted_thread_count: u32,
    semantic_memory_attempted_count: u32,
    semantic_memory_unconfirmed_count: u32,
    remaining_work: Vec<&'static str>,
}

#[derive(Debug, Deserialize)]
struct DeleteSpaceSchedulesResponseBody {
    request_id: String,
    owner_plane: String,
    outcome: String,
}

/// One segment of the assembled context window.
#[derive(Debug, serde::Serialize)]
struct ContextSegmentResponse {
    kind: String,
    /// Segment text. Present so the inspector can show WHAT was included, not
    /// only how much — "1,200 tokens of grounding" does not answer "why did it
    /// say that?".
    content: String,
    estimated_tokens: u32,
}

#[derive(Debug, serde::Serialize)]
struct ThreadContextResponse {
    thread_id: String,
    /// session-core's own total. Reported rather than summed from the segments:
    /// if the two ever disagree, the inspector should show what the assembler
    /// believes, not a number this route computed.
    estimated_tokens: u32,
    /// The budget the segments were assembled against, so a reader can see how
    /// full the window is rather than guessing.
    budget_tokens: u32,
    segments: Vec<ContextSegmentResponse>,
    /// Persona spec — the per-thread override, relayed from session-core's
    /// `GetContextAssemblyResponse.persona_enabled` (an `optional bool`; see
    /// that field's own doc comment for the proto3 zero-value rationale).
    /// Defaults `true` when the RPC left it unset (an old session-core
    /// binary during a rolling deploy) — fail OPEN for display, never fail
    /// closed and hide a persona the user turned on.
    persona_enabled: bool,
}

#[derive(Debug, serde::Deserialize)]
struct ThreadContextQuery {
    /// Optional run scope. Empty asks for the thread's current assembly.
    #[serde(default)]
    run_id: Option<String>,
}

/// The itemized context window for a thread — the read half of the context
/// inspector.
///
/// Deliberately NOT delegating a Data Plane credential, unlike the prompt-build
/// path: this is a read-only introspection view of the durable segments, and
/// forwarding a retrieval credential here would let an inspector request trigger
/// live grounding fan-out (and bill for it) every time someone opened a panel.
/// Grounding segments already assembled into the thread are still reported.
async fn get_thread_context(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Query(query): Query<ThreadContextQuery>,
) -> Result<Json<ThreadContextResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::GetContextAssemblyRequest;

    let budget = crate::sse::context_assembly_budget();
    let response = state
        .session_client
        .clone()
        .get_context_assembly(authenticated_session_request(
            GetContextAssemblyRequest {
                thread_id: thread_id.clone(),
                run_id: query.run_id.unwrap_or_default(),
                max_tokens: budget,
                policy_id: String::new(),
                workspace_id: String::new(),
                agent_id: String::new(),
                // This endpoint deliberately forwards no Data Plane bearer (see
                // the fn doc above), so no live retrieval this posture would
                // govern is ever triggered here. Still resolved from the
                // caller's own signed claim, rather than hardcoded, so the
                // field says something true if that ever changes.
                sovereign_required: claims.effective_sovereign_required(None),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core get_context_assembly failed", &e))?
        .into_inner();

    Ok(Json(ThreadContextResponse {
        thread_id,
        estimated_tokens: response.estimated_tokens,
        budget_tokens: budget,
        persona_enabled: response.persona_enabled.unwrap_or(true),
        segments: response
            .segments
            .into_iter()
            .map(|segment| ContextSegmentResponse {
                kind: segment.kind,
                content: segment.content,
                estimated_tokens: segment.estimated_tokens,
            })
            .collect(),
    }))
}

async fn list_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<ListThreadsQuery>,
) -> Result<Json<ListThreadsResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::{ArchiveFilter, ListThreadsRequest};

    let archived = match query.archived.as_deref().unwrap_or("active") {
        "active" => ArchiveFilter::Unspecified,
        "archived" => ArchiveFilter::Archived,
        "all" => ArchiveFilter::All,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "archived must be active, archived or all" })),
            ))
        }
    };

    let response = state
        .session_client
        .clone()
        .list_threads(authenticated_session_request(
            ListThreadsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: query.limit.unwrap_or(80),
                space_id: query.space_id.unwrap_or_default(),
                origin: query.origin.unwrap_or_default(),
                space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
                space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
                cursor: query.cursor.unwrap_or_default(),
                thread_id: query.thread_id.unwrap_or_default(),
                archived: archived as i32,
                starred_only: query.starred_only.unwrap_or(false),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_threads failed", &e))?
        .into_inner();

    let next_cursor = response.next_cursor;
    let threads = response
        .threads
        .into_iter()
        .map(|thread| ThreadSummaryResponse {
            thread_id: thread.thread_id,
            session_key: thread.session_key,
            title: thread.title,
            preview: thread.preview,
            created_at: timestamp_to_rfc3339(thread.created_at),
            updated_at: timestamp_to_rfc3339(thread.updated_at),
            pinned: thread.pinned,
            space_id: thread.space_id,
            forked_from_thread_id: thread.forked_from_thread_id,
            latest_run_id: thread.latest_run_id,
            latest_run_status: thread.latest_run_status,
            latest_run_updated_at: thread
                .latest_run_updated_at
                .map(|value| timestamp_to_rfc3339(Some(value))),
            origin: thread.origin,
            owner_subject_id: thread.owner_subject_id,
            persona_enabled: thread.persona_enabled,
            archived_at: timestamp_to_rfc3339(thread.archived_at),
            starred: thread.starred,
            unread: thread.unread,
            attention_state: attention_state_name(thread.attention_state),
            attention_since: thread
                .attention_since
                .map(|value| timestamp_to_rfc3339(Some(value))),
            attention_unseen: attention_state_name(thread.attention_state)
                .map(|_| thread.attention_unseen),
            reply_count: thread.reply_count,
            last_author_subject_id: thread.last_author_subject_id,
            last_message_seq: thread.last_message_seq,
            recent_author_subject_ids: thread.recent_author_subject_ids,
            last_role: thread.last_role,
            turn_count: thread.turn_count,
        })
        .collect();

    Ok(Json(ListThreadsResponse {
        threads,
        next_cursor,
    }))
}

/// Persist user-facing title, preview, and pin state through Session Core.
/// The Model Gateway derives tenant/user scope exclusively from verified claims;
/// callers cannot select another user's thread presentation.
async fn update_thread_presentation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Json(body): Json<UpdateThreadPresentationBody>,
) -> Result<Json<UpdateThreadPresentationResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::UpdateThreadPresentationRequest;

    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }

    let response = state
        .session_client
        .clone()
        .update_thread_presentation(authenticated_session_request(
            UpdateThreadPresentationRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
                title: body.title,
                preview: body.preview,
                pinned: body.pinned,
                persona_enabled: body.persona_enabled,
                archived: body.archived,
                starred: body.starred,
                unread: body.unread,
                memory_mode: None,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| {
            session_thread_error("session-core update_thread_presentation failed", &error)
        })?
        .into_inner();

    Ok(Json(UpdateThreadPresentationResponse {
        thread_id: response.thread_id,
    }))
}

/// The caller's threads that need attention or are working (WS16 S1). The scope comes from the
/// verified claims, so a query naming another user changes nothing, and the answer carries
/// identifiers and the closed state only.
async fn list_attention(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<ListAttentionQuery>,
) -> Result<Json<ListAttentionResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListAttentionRequest;

    let response = state
        .attention_client
        .clone()
        .list_attention(authenticated_session_request(
            ListAttentionRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: query.limit.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core list_attention failed", &error))?
        .into_inner();

    Ok(Json(ListAttentionResponse {
        enabled: response.enabled,
        stream: response.stream_enabled,
        items: response
            .items
            .into_iter()
            .map(attention_item_response)
            .collect(),
        counts: attention_counts_response(response.counts.unwrap_or_default()),
    }))
}

/// `GET /v1/attention/stream`: the caller's attention stream as server-sent events (WS16 S5). Each frame
/// is one event named for its kind, with `<epoch>:<seq>` as its id. Session Core refuses the stream
/// before its first frame (switch off, cap reached, zero-retention caller), and the refusal reaches the
/// browser as Session Core sent it.
async fn stream_attention(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
) -> Result<Sse<ReceiverStream<Result<SseEvent, Infallible>>>, HttpJsonError> {
    use mp_contracts::model_plane::v1::StreamAttentionRequest;

    let mut frames = state
        .attention_client
        .clone()
        .stream_attention(authenticated_session_request(
            StreamAttentionRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core stream_attention failed", &error))?
        .into_inner();

    // The relay ends when Session Core ends the stream or the browser leaves. Dropping `frames` then
    // releases Session Core's side of the stream.
    let (tx, rx) = tokio::sync::mpsc::channel(ATTENTION_STREAM_BUFFER);
    tokio::spawn(async move {
        while let Ok(Some(frame)) = frames.message().await {
            let Some(event) = attention_stream_event(frame) else {
                continue;
            };
            if tx.send(Ok(event)).await.is_err() {
                break;
            }
        }
    });
    Ok(Sse::new(ReceiverStream::new(rx)))
}

/// The frames a stream holds while the browser reads them.
const ATTENTION_STREAM_BUFFER: usize = 16;

/// One frame of the attention stream as a server-sent event. The name is the frame's kind; the id is
/// `<epoch>:<seq>`, so a restarted replica reads as a new epoch rather than as a gap in the old sequence.
fn attention_stream_event(
    frame: mp_contracts::model_plane::v1::StreamAttentionResponse,
) -> Option<SseEvent> {
    use mp_contracts::model_plane::v1::stream_attention_response::Frame;

    let id = format!("{}:{}", frame.epoch, frame.seq);
    let (name, data) = match frame.frame? {
        Frame::Snapshot(snapshot) => (
            "attention_snapshot",
            json!({
                "items": snapshot.items.into_iter().map(attention_item_response).collect::<Vec<_>>(),
                "counts": attention_counts_response(snapshot.counts.unwrap_or_default()),
            }),
        ),
        Frame::ThreadChanged(changed) => (
            "attention_changed",
            json!({
                "item": attention_item_response(changed.item.unwrap_or_default()),
                "counts": attention_counts_response(changed.counts.unwrap_or_default()),
            }),
        ),
        Frame::Resync(_) => ("attention_resync", json!({})),
        Frame::Heartbeat(_) => ("attention_heartbeat", json!({})),
    };
    Some(
        SseEvent::default()
            .event(name)
            .id(id)
            .data(data.to_string()),
    )
}

/// One listed thread as the browser reads it (WS16 S1, S5).
fn attention_item_response(
    item: mp_contracts::model_plane::v1::AttentionItem,
) -> AttentionItemResponse {
    AttentionItemResponse {
        thread_id: item.thread_id,
        state: attention_state_name(item.state),
        reason: attention_reason_name(item.reason),
        since: item.since.map(|value| timestamp_to_rfc3339(Some(value))),
        unseen: item.unseen,
        run_id: item.run_id,
    }
}

/// The counts behind the badge, as the browser reads them (WS16 S1, S5).
fn attention_counts_response(
    counts: mp_contracts::model_plane::v1::AttentionCounts,
) -> AttentionCountsResponse {
    AttentionCountsResponse {
        needs_you: counts.needs_you,
        working: counts.working,
        unseen: counts.unseen,
    }
}

/// Marks one of the caller's threads read (WS16 S1). Session Core refuses a zero-retention or
/// waived caller, and answers 412 while the attention switch is off.
async fn mark_thread_seen(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<MarkThreadSeenResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::MarkThreadSeenRequest;

    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }

    let response = state
        .attention_client
        .clone()
        .mark_thread_seen(authenticated_session_request(
            MarkThreadSeenRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                thread_id: thread_id.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core mark_thread_seen failed", &error))?
        .into_inner();

    Ok(Json(MarkThreadSeenResponse {
        thread_id: response.thread_id,
    }))
}

/// Archive one history item without erasing its Model-Plane record or audit
/// evidence. A separately authorized subject-erasure workflow owns deletion.
async fn archive_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<ArchiveThreadResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ArchiveThreadRequest;

    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }
    let response = state
        .session_client
        .clone()
        .archive_thread(authenticated_session_request(
            ArchiveThreadRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core archive_thread failed", &error))?
        .into_inner();

    Ok(Json(ArchiveThreadResponse {
        thread_id: response.thread_id,
        archived_at: timestamp_to_rfc3339(response.archived_at),
    }))
}

/// Archive every currently visible thread belonging to the authenticated user.
async fn archive_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
) -> Result<Json<ArchiveThreadsResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ArchiveThreadsRequest;

    let response = state
        .session_client
        .clone()
        .archive_threads(authenticated_session_request(
            ArchiveThreadsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core archive_threads failed", &error))?
        .into_inner();

    Ok(Json(ArchiveThreadsResponse {
        archived_count: response.archived_count,
    }))
}

/// Permanently erase one thread in Session Core before any edge presentation
/// cache is allowed to disappear. The BFF's DELETE endpoint delegates here;
/// this route is deliberately owner-bound by Session Core, not by a gateway
/// index or caller-supplied user id.
async fn delete_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<DeleteThreadResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DeleteThreadRequest;

    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }
    let response = state
        .session_client
        .clone()
        .delete_thread(authenticated_session_request(
            DeleteThreadRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core delete_thread failed", &error))?
        .into_inner();

    Ok(Json(DeleteThreadResponse {
        thread_id: response.thread_id,
        deleted: response.deleted,
    }))
}

/// Permanently erase all threads for the authenticated user in Session Core.
/// Session Core performs the owner check and transaction; this handler only
/// forwards verified claims and returns the durable receipt.
async fn delete_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
) -> Result<Json<DeleteThreadsResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DeleteThreadsRequest;

    let response = state
        .session_client
        .clone()
        .delete_threads(authenticated_session_request(
            DeleteThreadsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core delete_threads failed", &error))?
        .into_inner();

    Ok(Json(DeleteThreadsResponse {
        deleted_count: response.deleted_count,
    }))
}

/// Control-authorized Model Plane adapter for one owner's threads in one
/// canonical Space. It is intentionally not a generic service/user deletion:
/// auth middleware admits only `service:control-space-deletion` with an
/// independently verified `aud=session-core` bearer carrying
/// `session:space-delete`; Session Core repeats the exact service check.
async fn delete_space_threads_for_deletion_coordinator(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    capability_bearer: Option<Extension<VerifiedCapabilityBearer>>,
    Json(body): Json<DeleteSpaceThreadsRequestBody>,
) -> Result<Json<DeleteSpaceThreadsResponseBody>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DeleteSpaceThreadsRequest;

    if body.org_id.trim().is_empty()
        || body.space_id.trim().is_empty()
        || body.owner_principal_id.trim().is_empty()
        || body.deletion_request_id.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": "org_id, space_id, owner_principal_id, and deletion_request_id are required"}),
            ),
        ));
    }
    if claims.org_id != body.org_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                json!({"error": "deletion request tenant does not match verified service identity"}),
            ),
        ));
    }
    let Some(Extension(capability_bearer)) = capability_bearer else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "independently verified capability deletion bearer is required"})),
        ));
    };

    let response = state
        .session_client
        .clone()
        .delete_space_threads(authenticated_session_request(
            DeleteSpaceThreadsRequest {
                org_id: body.org_id.clone(),
                space_id: body.space_id.clone(),
                owner_principal_id: body.owner_principal_id.clone(),
                deletion_request_id: body.deletion_request_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core delete_space_threads failed", &error))?
        .into_inner();

    let cron_url = format!(
        "{}/api/v1/internal/space-deletion/cron",
        state.capability_core_base_url.trim_end_matches('/')
    );
    let cron_response = state
        .http_client
        .post(cron_url)
        .bearer_auth(capability_bearer.as_str())
        .json(&json!({
            "org_id": body.org_id,
            "space_ref": body.space_id,
            "owner_principal_id": body.owner_principal_id,
            "deletion_request_id": body.deletion_request_id,
        }))
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "Capability Core Space schedule deletion unavailable"})),
            )
        })?;
    if !cron_response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "Capability Core Space schedule deletion rejected"})),
        ));
    }
    let cron_receipt: DeleteSpaceSchedulesResponseBody = cron_response.json().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "Capability Core Space schedule deletion returned an invalid receipt"})),
        )
    })?;
    if cron_receipt.request_id != body.deletion_request_id
        || cron_receipt.owner_plane != "model"
        || cron_receipt.outcome != "partial"
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"error": "Capability Core Space schedule deletion receipt does not match request"}),
            ),
        ));
    }

    // Session Core fully removes its local thread/run/transcript rows. A
    // correlated semantic delete is reported separately; historical records
    // remain uncorrelated and therefore keep the Model owner partial.
    let mut remaining_work = vec!["historical_uncorrelated_letta_memory"];
    if response.semantic_memory_unconfirmed_count > 0 {
        remaining_work.push("unconfirmed_correlated_letta_memory_deletion");
    }
    Ok(Json(DeleteSpaceThreadsResponseBody {
        request_id: body.deletion_request_id,
        owner_plane: "model",
        owner_outcome: "partial",
        deleted_thread_count: response.deleted_count,
        semantic_memory_attempted_count: response.semantic_memory_attempted_count,
        semantic_memory_unconfirmed_count: response.semantic_memory_unconfirmed_count,
        remaining_work,
    }))
}

#[derive(Debug, Deserialize)]
struct PrepareMailTaskThreadRequestBody {
    org_id: String,
    user_id: String,
    origin: String,
    session_key: String,
    /// Forwarded opaquely all the way from user-core's
    /// `IssueMailTaskThreadDecision` through integration-corev2 and the
    /// gateway BFF — this handler never reads or interprets it, only passes
    /// it to session-core's `PrepareUserOwnedThreadRequest.control_decision_token`
    /// unmodified (delegated-identity design, Gap B §2.4(ii)).
    control_decision_token: String,
}

#[derive(Debug, Serialize)]
struct PrepareMailTaskThreadResponseBody {
    thread_id: String,
}

fn authenticated_session_request_with_bearer<T>(
    value: T,
    bearer: &str,
) -> Result<tonic::Request<T>, HttpJsonError> {
    let mut request = tonic::Request::new(value);
    let authorization = format!("Bearer {bearer}").parse().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "mail-task bridge credential is not forwardable"})),
        )
    })?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// Delegated-identity design, Gap B (§2.4(ii)) — the internal HTTP→gRPC
/// bridge the gateway BFF (Frontend Plane, which has no `tonic`/
/// `mp-contracts` dependency) reaches to call session-core's
/// `PrepareUserOwnedThread` for an inbound mail-task message. Authenticated
/// via the existing HTTP `Claims`/`service_id` middleware
/// (`auth::authorize_principal_route`), gated to exactly
/// `service:verevon-gateway`; mints/reuses model-gateway's OWN, distinct
/// `aud=session-core` credential (`mail_task_bridge_auth`) rather than
/// trusting anything the BFF presents as proof of a session-core identity.
async fn prepare_mail_task_thread_for_gateway_bff(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PrepareMailTaskThreadRequestBody>,
) -> Result<Json<PrepareMailTaskThreadResponseBody>, HttpJsonError> {
    use mp_contracts::model_plane::v1::PrepareUserOwnedThreadRequest;

    if body.org_id.trim().is_empty()
        || body.user_id.trim().is_empty()
        || body.origin.trim().is_empty()
        || body.session_key.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "org_id, user_id, origin and session_key are required"})),
        ));
    }
    // This route exists for exactly one origin. The bridge's session-core
    // principal is allow-listed to prepare a thread for ANY user of the org
    // and only the `mail_task` origin is gated on a signed Control decision,
    // so a caller-chosen origin here would let a compromised BFF credential
    // mint an ordinary `chat` thread for any user with no decision at all.
    // Pinned here and again in session-core for that principal.
    if body.origin.trim() != "mail_task" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "origin must be \"mail_task\" for the mail-task thread bridge"})),
        ));
    }
    // Same tenant-fencing shape as delete_space_threads_for_deletion_coordinator
    // above: the verified service credential's own org_id must match the
    // request it is presenting, so a bridge credential minted for one org
    // can never be replayed to prepare a thread in a different one.
    if claims.org_id != body.org_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "mail-task bridge request tenant does not match verified service identity"})),
        ));
    }

    let Some(tokens) = state.mail_task_bridge_tokens.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "mail-task thread bridge is not configured"})),
        ));
    };
    let bearer = tokens.token(&body.org_id).await.map_err(|error| {
        tracing::error!(%error, "mail-task bridge session-core credential mint failed");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "mail-task thread bridge credential is unavailable"})),
        )
    })?;

    let response = state
        .session_client
        .clone()
        .prepare_user_owned_thread(authenticated_session_request_with_bearer(
            PrepareUserOwnedThreadRequest {
                org_id: body.org_id,
                user_id: body.user_id,
                origin: body.origin,
                session_key: body.session_key,
                control_decision_token: body.control_decision_token,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| session_thread_error("session-core prepare_user_owned_thread failed", &error))?
        .into_inner();

    Ok(Json(PrepareMailTaskThreadResponseBody {
        thread_id: response.thread_id,
    }))
}

pub(crate) fn timestamp_to_rfc3339(value: Option<prost_types::Timestamp>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let nanos = u32::try_from(value.nanos).unwrap_or_default();
    chrono::DateTime::from_timestamp(value.seconds, nanos)
        .map(|ts| ts.to_rfc3339())
        .unwrap_or_default()
}

/// chat-parity §1 — reload a thread's conversation history (cross-device
/// resume). Reads from session-core's `ListConversation` (the canonical
/// conversation store); org scope comes from the authenticated claims so a
/// caller can never read another org's thread.
/// Query for a shared-Space transcript read. Absent parameters keep the
/// historical owner-bound behaviour; session-core, not this handler, decides
/// whether a supplied decision authorizes anything.
#[derive(Debug, Deserialize)]
struct ThreadMessagesQuery {
    #[serde(default)]
    space_id: Option<String>,
    #[serde(default)]
    space_read_decision_ref: Option<String>,
    #[serde(default)]
    space_read_decision_token: Option<String>,
    /// Tail-first page size (WS09 S8). Absent or zero is the unpaged read every
    /// caller has always had; a positive value is capped at this gateway's ceiling.
    #[serde(default)]
    limit: Option<u32>,
    /// The id of the oldest turn the caller holds. The page is the turns strictly
    /// before it, so it is read only together with a positive `limit`.
    #[serde(default)]
    before: Option<String>,
    /// `1` or `true`: also return the thread's first readable turn ahead of the newest page
    /// (WS15-S6). Honoured only while `SPACE_TIMELINE_API` is on and only without `before`.
    #[serde(default)]
    include_first: Option<String>,
}

/// Caps the tail page `GET /v1/threads/{id}/messages` serves. Operators set
/// `MODEL_GATEWAY_THREAD_MESSAGES_MAX_LIMIT`; anything that is not a whole number
/// from 1 to 200 keeps the default, so a typo never changes the page size.
/// The gateway's copy of the message-version switch (WS17 S8). Session Core enforces it; the gateway
/// reads its own copy to report `versions_enabled` and to refuse a branch move before it crosses the
/// wire. The deploy sets the two together. Read by the strict parser the other switches share.
const MESSAGE_VERSIONS_ENV: &str = "MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED";

pub(crate) fn message_versions_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(MESSAGE_VERSIONS_ENV).ok().as_deref())
}

const THREAD_MESSAGES_MAX_LIMIT_ENV: &str = "MODEL_GATEWAY_THREAD_MESSAGES_MAX_LIMIT";
const THREAD_MESSAGES_DEFAULT_MAX_LIMIT: u32 = 200;

fn thread_messages_page_ceiling(raw: Option<&str>) -> u32 {
    raw.and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| (1..=THREAD_MESSAGES_DEFAULT_MAX_LIMIT).contains(value))
        .unwrap_or(THREAD_MESSAGES_DEFAULT_MAX_LIMIT)
}

/// Whether `raw` can name a turn to page before: the id shape session-core
/// writes, bounded so that nothing else can ride in on the query string.
fn is_thread_message_cursor(raw: &str) -> bool {
    (1..=128).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn list_thread_messages(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Query(query): Query<ThreadMessagesQuery>,
) -> Result<Json<ListThreadMessagesResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::ListConversationRequest;

    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "thread_id is required"})),
        ));
    }

    let before = query.before.unwrap_or_default().trim().to_owned();
    if !before.is_empty() && !is_thread_message_cursor(&before) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "invalid_thread_request",
                    "message": "before must be a message id",
                }
            })),
        ));
    }
    let limit = query.limit.unwrap_or(0).min(thread_messages_page_ceiling(
        std::env::var(THREAD_MESSAGES_MAX_LIMIT_ENV).ok().as_deref(),
    ));
    let include_first = limit > 0
        && before.is_empty()
        && mp_contracts::space_timeline::api_enabled()
        && query
            .include_first
            .as_deref()
            .is_some_and(|raw| matches!(raw.trim(), "1" | "true"));
    // A shared-Space read (it carries a Control decision) sees only the
    // allowlisted metadata keys (WS07-S9a); the owner's Chat read is unchanged.
    let space_read_decision_ref = query.space_read_decision_ref.unwrap_or_default();
    let legacy_metadata_passthrough = crate::room_metadata::legacy_passthrough(
        std::env::var(crate::room_metadata::LEGACY_PASSTHROUGH_ENV)
            .ok()
            .as_deref(),
    );

    let response = state
        .session_client
        .clone()
        .list_conversation(authenticated_session_request(
            ListConversationRequest {
                org_id: claims.org_id.clone(),
                thread_id: trimmed.to_owned(),
                space_id: query.space_id.unwrap_or_default(),
                space_read_decision_ref: space_read_decision_ref.clone(),
                space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
                // The transcript a client shows is the active path; turns on other
                // branches are not read through this route.
                include_inactive: false,
                limit,
                before_message_id: before,
                // The UI transcript route is the only reader that shows human posts. Every
                // other caller keeps the default (false), so a post never reaches model
                // context or reattach_context.
                include_human_posts: true,
                post_context_cap: 0,
                include_first,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_conversation failed", &e))?
        .into_inner();

    // A version switch moves the branch to a sibling, and the siblings sit on branches the transcript does not
    // show. A versioned transcript therefore takes one more unpaged read, and each versioned turn lists the
    // turns sharing its parent, in order (WS17 S9).
    let sibling_ids = if message_versions_enabled()
        && response.messages.iter().any(|m| m.sibling_count > 1)
    {
        let all = state
            .session_client
            .clone()
            .list_conversation(authenticated_session_request(
                ListConversationRequest {
                    org_id: claims.org_id.clone(),
                    thread_id: trimmed.to_owned(),
                    // A versioned thread is a personal one: a Space keeps no message tree.
                    space_id: String::new(),
                    space_read_decision_ref: String::new(),
                    space_read_decision_token: String::new(),
                    include_inactive: true,
                    limit: 0,
                    before_message_id: String::new(),
                    // A personal thread holds no human posts; the internal read keeps them out.
                    ..Default::default()
                },
                &bearer,
            )?)
            .await
            .map_err(|e| session_thread_error("session-core list_conversation failed", &e))?
            .into_inner();
        sibling_ids_by_parent(&all.messages)
    } else {
        std::collections::HashMap::new()
    };

    // Only a paged read says whether more exists, so an unpaged body keeps the
    // exact shape every caller already parses.
    let has_more = (limit > 0).then_some(response.has_more);
    let omitted_before = include_first.then_some(response.omitted_before);
    let messages =
        response
            .messages
            .into_iter()
            .map(|m| ThreadMessage {
                revision: thread_message_revision(&m),
                message_id: m.message_id,
                role: m.role,
                content: m.content,
                agent_name: m.agent_name,
                author_subject_id: m.author_subject_id,
                post_kind: if m.post_kind == "post" { m.post_kind } else { String::new() },
                created_at: message_created_at(m.created_at.as_ref()),
                entry: thread_message_entry(m.entry),
                mentions: m
                    .mentions
                    .into_iter()
                    .map(|r| ThreadMessageMention {
                        subject_id: r.subject_id,
                        kind: r.kind,
                        wake_state: r.wake_state,
                        reason: r.reason,
                        source: r.source,
                        ordinal: r.ordinal,
                    })
                    .collect(),
                metadata: crate::room_metadata::visible_metadata(
                    m.metadata.as_ref().map(prost_struct_to_json).and_then(
                        |value| match value {
                            // `entry` is the typed field above; a stored free key of that name
                            // must never produce a second `entry` in the body.
                            Value::Object(mut map) => {
                                map.remove("entry");
                                (!map.is_empty()).then_some(map)
                            }
                            _ => None,
                        },
                    ),
                    &space_read_decision_ref,
                    legacy_metadata_passthrough,
                ),
                version: (m.sibling_count > 0).then(|| ThreadMessageVersion {
                    siblings: sibling_ids
                        .get(&m.parent_message_id)
                        .cloned()
                        .unwrap_or_default(),
                    parent_message_id: m.parent_message_id,
                    sibling_ordinal: m.sibling_ordinal,
                    sibling_count: m.sibling_count,
                    on_active_path: m.on_active_path,
                }),
            })
            .collect();

    Ok(Json(ListThreadMessagesResponse {
        thread_id: trimmed.to_owned(),
        messages,
        has_more,
        omitted_before,
        versions_enabled: message_versions_enabled().then_some(true),
    }))
}

/// The ids of the turns that share each parent, in sibling order: what a versioned turn lists as the
/// versions a switch can move to (WS17 S9).
fn sibling_ids_by_parent(
    turns: &[mp_contracts::model_plane::v1::SessionMessage],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut by_parent: std::collections::HashMap<String, Vec<(u32, String)>> =
        std::collections::HashMap::new();
    for turn in turns {
        by_parent
            .entry(turn.parent_message_id.clone())
            .or_default()
            .push((turn.sibling_ordinal, turn.message_id.clone()));
    }
    by_parent
        .into_iter()
        .map(|(parent, mut versions)| {
            versions.sort();
            (parent, versions.into_iter().map(|(_, id)| id).collect())
        })
        .collect()
}

/// The body of a branch move: the turn the user picked and the leaf they last saw.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BranchThreadBody {
    /// On the active path this is a rewind; anywhere else a version switch.
    target_message_id: String,
    /// The leaf the client last displayed. A move against a newer leaf is refused with
    /// 409 `active_leaf_stale`, so the client reloads before it moves again.
    expected_leaf_message_id: String,
}

#[derive(Debug, Serialize)]
struct BranchThreadResponse {
    thread_id: String,
    active_leaf_message_id: String,
    active_path: Vec<String>,
}

/// Move a thread's active branch (WS17 S8). A ZDR caller is refused before anything is
/// written, and Session Core decides the rest: the leaf moves only while the expected
/// leaf is still the leaf, and a Space thread, a waived thread or a thread with a run in
/// flight keeps no versions.
async fn branch_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Json(body): Json<BranchThreadBody>,
) -> Result<Json<BranchThreadResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::SetActiveLeafRequest;

    require_non_zdr_durable_mutation(&claims)?;
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }
    if !message_versions_enabled() {
        return Err(history_versions_disabled());
    }
    let response = state
        .message_tree_client
        .clone()
        .set_active_leaf(authenticated_session_request(
            SetActiveLeafRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
                target_message_id: body.target_message_id,
                expected_leaf_message_id: body.expected_leaf_message_id,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| branch_thread_error(&error))?
        .into_inner();

    Ok(Json(BranchThreadResponse {
        thread_id: thread_id.to_owned(),
        active_leaf_message_id: response.active_leaf_message_id,
        active_path: response.active_path,
    }))
}

/// Session Core's refusals of a branch move, as the typed codes the client acts on. A stale
/// leaf is a conflict to reload from; any other refusal keeps the session error mapping.
pub(crate) fn branch_thread_error(error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::Aborted => (
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "code": "active_leaf_stale",
                "message": "the thread moved on; reload it and try again",
            }})),
        ),
        tonic::Code::FailedPrecondition => (
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "code": "history_branch_refused",
                "message": error.message(),
            }})),
        ),
        tonic::Code::Unavailable if error.message() == "message_versions_disabled" => {
            history_versions_disabled()
        }
        _ => session_thread_error("session-core set_active_leaf failed", error),
    }
}

pub(crate) fn history_versions_disabled() -> HttpJsonError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": {
            "code": "history_versions_disabled",
            "message": "message versions are not enabled on this deployment",
        }})),
    )
}

/// Body of `POST /v1/threads/{id}/fork`: the assistant turn the copy ends with, and an optional
/// title for the new thread.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ForkThreadBody {
    from_message_id: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Serialize)]
struct ForkThreadResponse {
    thread_id: String,
    forked_from_thread_id: String,
    copied_message_count: u32,
}

/// Start a new thread from a copy of this one, up to an assistant turn (build plan WS17 S6). A
/// zero-retention caller is refused before anything is written. Session Core decides the rest: the
/// switch, the owner, a Space or waived source, and whether the turn lies on the active path.
async fn fork_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Json(body): Json<ForkThreadBody>,
) -> Result<Json<ForkThreadResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ForkThreadRequest;

    require_non_zdr_durable_mutation(&claims)?;
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }
    let response = state
        .thread_fork_client
        .clone()
        .fork_thread(authenticated_session_request(
            ForkThreadRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
                from_message_id: body.from_message_id,
                title: body.title,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| fork_thread_error(&error))?
        .into_inner();

    Ok(Json(ForkThreadResponse {
        thread_id: response.new_thread_id,
        forked_from_thread_id: thread_id.to_owned(),
        copied_message_count: response.copied_message_count,
    }))
}

/// Session Core's refusals of a fork, as the typed codes the client acts on. A switch that is off
/// answers 503 `history_fork_disabled`. A source that cannot be forked answers 409
/// `history_fork_refused`, carrying Session Core's reason.
pub(crate) fn fork_thread_error(error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::Unavailable if error.message() == "thread_fork_disabled" => {
            history_fork_disabled()
        }
        tonic::Code::FailedPrecondition => (
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "code": "history_fork_refused",
                "message": error.message(),
            }})),
        ),
        _ => session_thread_error("session-core fork_thread failed", error),
    }
}

pub(crate) fn history_fork_disabled() -> HttpJsonError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": {
            "code": "history_fork_disabled",
            "message": "forking is not enabled on this deployment",
        }})),
    )
}

#[derive(Debug, Deserialize)]
struct ReplayThreadEventsQuery {
    /// Resume after this event id (exclusive). Empty means from the beginning.
    #[serde(default)]
    after_event_id: Option<String>,
    /// Server-side bound. Session Core treats zero as unlimited; the HTTP
    /// surface always supplies a bounded value so a Trace panel cannot pull an
    /// unbounded tenant history into one response.
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DurableThreadEventResponse {
    event_id: String,
    event_type: String,
    schema_version: u32,
    at: Value,
    producer: String,
    correlation_id: String,
    causation_id: String,
    resource_ref: String,
}

#[derive(Debug, Serialize)]
struct ReplayThreadEventsResponse {
    thread_id: String,
    events: Vec<DurableThreadEventResponse>,
    /// True only when the bounded request was filled. Session Core streams do
    /// not carry a total count, so clients must use the last event id as a
    /// cursor and treat this as a continuation hint rather than a count claim.
    truncated: bool,
}

/// Replay the canonical, append-only Session Core event log for one thread.
///
/// This is deliberately an envelope-only read model. Event payloads are
/// protobuf `Any` values owned by the Model Plane and may contain prompt,
/// provider, or connector data that is not suitable for a browser Trace. The
/// chat UI combines these safe labels with the existing plan/approval/proof
/// endpoints for detail.
async fn replay_thread_events(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
    Query(query): Query<ReplayThreadEventsQuery>,
) -> Result<Json<ReplayThreadEventsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "thread_id is required"})),
        ));
    }

    const DEFAULT_LIMIT: u32 = 160;
    const MAX_LIMIT: u32 = 500;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let after_event_id = query.after_event_id.unwrap_or_default().trim().to_owned();

    let response = state
        .session_client
        .clone()
        .replay_thread(authenticated_session_request(
            ReplayThreadRequest {
                thread_id: trimmed.to_owned(),
                after_event_id,
                limit,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core replay_thread failed", &e))?;

    let mut stream = response.into_inner();
    let mut events = Vec::with_capacity(limit as usize);
    while let Some(event) = stream
        .message()
        .await
        .map_err(|e| session_thread_error("session-core replay_thread failed", &e))?
    {
        events.push(durable_thread_event(event));
    }

    let truncated = events.len() >= limit as usize;
    Ok(Json(ReplayThreadEventsResponse {
        thread_id: trimmed.to_owned(),
        events,
        truncated,
    }))
}

/// Resolve the public lifecycle label for a replayed event.
///
/// Session Core's event table predates the protobuf `EventType` taxonomy for
/// thread/message records, so those rows carry the correct discriminator in
/// `Any.type_url` while their legacy string is converted to a numeric value
/// that collides with plan events.  The type URL is the owning producer's
/// authoritative discriminator; use it for those records and retain the
/// envelope enum for the events that are already represented there.  Payload
/// bytes are never decoded or returned to the browser.
fn durable_thread_event_type(event: &Event) -> String {
    let type_url = event
        .payload
        .as_ref()
        .map(|payload| payload.type_url.as_str())
        .unwrap_or_default();

    let typed_label = match type_url {
        "type.googleapis.com/model_plane.v1.ThreadCreated" => Some("THREAD_CREATED"),
        "type.googleapis.com/model_plane.v1.MessageAppended" => Some("MESSAGE_APPENDED"),
        "type.googleapis.com/model_plane.v1.StepCompleted" => Some("STEP_COMPLETED"),
        "type.googleapis.com/model_plane.v1.RunStarted" => Some("RUN_STARTED"),
        "type.googleapis.com/model_plane.v1.RunCancelled" => Some("RUN_CANCELLED"),
        "type.googleapis.com/model_plane.v1.CheckpointSaved" => Some("CHECKPOINT_SAVED"),
        "type.googleapis.com/model_plane.v1.ThreadPresentationUpdated" => {
            Some("THREAD_PRESENTATION_UPDATED")
        }
        "type.googleapis.com/model_plane.v1.ThreadArchived" => Some("THREAD_ARCHIVED"),
        "type.googleapis.com/model_plane.v1.ThreadForked" => Some("THREAD_FORKED"),
        _ => None,
    };

    typed_label.map_or_else(
        || {
            EventType::try_from(event.event_type)
                .map(|kind| kind.as_str_name().to_owned())
                .unwrap_or_else(|_| "EVENT_TYPE_UNSPECIFIED".to_owned())
        },
        str::to_owned,
    )
}

fn durable_thread_event(event: Event) -> DurableThreadEventResponse {
    let event_type = durable_thread_event_type(&event);
    DurableThreadEventResponse {
        event_id: event.event_id,
        event_type,
        schema_version: event.schema_version,
        at: event.ts.as_ref().map_or(Value::Null, prost_ts_to_rfc3339),
        producer: event.producer,
        correlation_id: event.correlation_id,
        causation_id: event.causation_id,
        resource_ref: event.resource_ref,
    }
}

#[derive(Debug, Deserialize)]
struct ListMemoriesQuery {
    limit: Option<u32>,
}

/// Render session-core's provenance enum as a stable public string.
///
/// An UNRECOGNISED tag maps to "unknown", not to a default of either real value:
/// a newer session-core adding a third provenance must degrade to "we are not
/// sure" rather than silently claiming the user said something.
fn memory_provenance_label(tag: i32) -> &'static str {
    match mp_contracts::model_plane::v1::MemoryProvenance::try_from(tag) {
        Ok(mp_contracts::model_plane::v1::MemoryProvenance::Stated) => "stated",
        Ok(mp_contracts::model_plane::v1::MemoryProvenance::Inferred) => "inferred",
        _ => "unknown",
    }
}

#[derive(Debug, Serialize)]
struct MemorySummaryResponse {
    memory_id: String,
    topic: String,
    content: String,
    updated_at: String,
    /// "stated" | "inferred" | "unknown" — how this memory came to exist.
    ///
    /// A string rather than the raw enum tag so the SPA never has to know
    /// protobuf numbering, and three-valued rather than a bool so a row written
    /// before provenance was recorded is not presented as either. Naming an
    /// unknown row "stated" would be the one wrong answer: it is the claim a
    /// user would rely on when deciding whether to keep it.
    provenance: &'static str,
    /// Closed vocabulary; omitted for an ordinary row. "room_origin_unverified" marks a
    /// row learned in a shared room before containment: listed to its owner, never
    /// recalled (WS17-S2).
    #[serde(skip_serializing_if = "String::is_empty")]
    withheld_reason: String,
    /// Space the withheld row was learned in (an id, never content); omitted when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    origin_space_ref: String,
}

#[derive(Debug, Serialize)]
struct ListMemoriesResponse {
    memories: Vec<MemorySummaryResponse>,
    degraded: bool,
    degradation_reason: String,
}

/// "What do you remember about me" — lists durable memory entries owned by
/// the signed-in user, across every thread. Never thread-scoped: session-core's
/// `ListMemory` is deliberately user-scoped, so this reads the same regardless
/// of which conversation the caller opens it from. Note: this is distinct from
/// the `/v1/memory` route above, which proxies capability-core's unrelated
/// agent-tool memory resource.
async fn list_memories(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<ListMemoriesQuery>,
) -> Result<Json<ListMemoriesResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListMemoryRequest;

    let response = state
        .memory_client
        .clone()
        .list_memory(authenticated_session_request(
            ListMemoryRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: query.limit.unwrap_or(100),
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core list_memory failed", &e))?
        .into_inner();

    let memories = response
        .entries
        .into_iter()
        .map(|entry| MemorySummaryResponse {
            memory_id: entry.memory_id,
            topic: entry.topic,
            content: entry.content,
            updated_at: timestamp_to_rfc3339(entry.updated_at),
            provenance: memory_provenance_label(entry.provenance),
            withheld_reason: entry.withheld_reason,
            origin_space_ref: entry.origin_space_ref,
        })
        .collect();

    Ok(Json(ListMemoriesResponse {
        memories,
        degraded: response.degraded,
        degradation_reason: response.degradation_reason,
    }))
}

#[derive(Debug, Deserialize)]
struct CreateMemoryBody {
    thread_id: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct CreateMemoryHttpResponse {
    memory_id: String,
    degraded: bool,
    degradation_reason: String,
}

/// Indexes a new personal ("USER"-topic) memory entry through session-core's
/// `IndexMemory` RPC — the same write path `dreaming::index_agent_memory`
/// already serves for the Dreaming extractor's own writes.
///
/// Topic is fixed to "USER" rather than caller-supplied: this route exists so
/// the "what do you remember about me" surface can save a corrected
/// replacement for an entry a reader is editing, never to let a client write
/// an arbitrary org/workspace/policy fact. `thread_id` only proves the caller
/// owns some real thread in this org (`IndexMemory` resolves the owning user
/// from it); it is not retained on a "USER"-topic row, which is why the BFF's
/// caller may pass whichever thread the edit happened to be read from.
async fn create_memory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Json(body): Json<CreateMemoryBody>,
) -> Result<Json<CreateMemoryHttpResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::IndexMemoryRequest;
    require_non_zdr_durable_mutation(&claims)?;

    let thread_id = body.thread_id.trim();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        ));
    }
    let content = body.content.trim();
    if content.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "content is required"})),
        ));
    }

    let response = state
        .memory_client
        .clone()
        .index_memory(authenticated_session_request(
            IndexMemoryRequest {
                thread_id: thread_id.to_owned(),
                topic: "USER".to_owned(),
                content: content.to_owned(),
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                memory_id: String::new(),
                // The person's own write behind verified user claims (WS26-S4).
                origin: "user".to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core index_memory failed", &e))?
        .into_inner();

    if response.memory_id.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "memory_not_found",
                    "message": "thread not found for this org",
                }
            })),
        ));
    }

    Ok(Json(CreateMemoryHttpResponse {
        memory_id: response.memory_id,
        degraded: response.degraded,
        degradation_reason: response.degradation_reason,
    }))
}

#[derive(Debug, Serialize)]
struct DeleteMemoryHttpResponse {
    deleted: bool,
    degraded: bool,
    degradation_reason: String,
}

/// Deletes a single memory entry by id, scoped to the signed-in user by
/// session-core's own `DeleteMemory` RPC (never trusts a client-selected
/// user). Destructive and non-recoverable — the frontend must confirm before
/// calling this.
async fn delete_memory_entry(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(memory_id): Path<String>,
) -> Result<Json<DeleteMemoryHttpResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DeleteMemoryRequest;

    let trimmed = memory_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "memory_id is required"})),
        ));
    }

    let response = state
        .memory_client
        .clone()
        .delete_memory(authenticated_session_request(
            DeleteMemoryRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                memory_id: trimmed.to_owned(),
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core delete_memory failed", &e))?
        .into_inner();

    Ok(Json(DeleteMemoryHttpResponse {
        deleted: response.deleted,
        degraded: response.degraded,
        degradation_reason: response.degradation_reason,
    }))
}

/// Upper bounds of the Space memory routes' body fields (EP-8 S7), the run-control route's own.
const SPACE_MEMORY_MAX_ID_BYTES: usize = 200;
const SPACE_MEMORY_MAX_DECISION_REF_BYTES: usize = 512;
const SPACE_MEMORY_MAX_DECISION_TOKEN_BYTES: usize = 8 * 1024;

/// `POST /v1/spaces/:space_id/memory/{list,delete}` body. The Control `model.space.memory.manage`
/// decision rides in the body; `memory_id` only on a delete.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceMemoryBody {
    #[serde(default)]
    space_control_decision_ref: String,
    #[serde(default)]
    space_control_decision_token: String,
    #[serde(default)]
    memory_id: String,
    #[serde(default)]
    limit: Option<u32>,
}

/// The decision and ids of a Space memory call, bounded; `None` is a malformed request.
fn space_memory_fields(space_id: &str, body: &SpaceMemoryBody) -> Option<(String, String, String)> {
    let space_id = space_id.trim();
    let decision_ref = body.space_control_decision_ref.trim();
    let token = body.space_control_decision_token.trim();
    let bounded = !space_id.is_empty()
        && space_id.len() <= SPACE_MEMORY_MAX_ID_BYTES
        && decision_ref.len() <= SPACE_MEMORY_MAX_DECISION_REF_BYTES
        && token.len() <= SPACE_MEMORY_MAX_DECISION_TOKEN_BYTES
        && body.memory_id.trim().len() <= SPACE_MEMORY_MAX_ID_BYTES;
    bounded.then(|| (space_id.to_owned(), decision_ref.to_owned(), token.to_owned()))
}

/// Session Core's refusal of a Space memory call in fixed words. A permission refusal is one code
/// whatever its cause (no decision, another role, a revoked member), so nothing about the Space leaks.
fn space_memory_error(status: &tonic::Status) -> HttpJsonError {
    let (code, http, message) = match status.code() {
        tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => (
            "space_memory_not_authorized",
            StatusCode::FORBIDDEN,
            "managing this Space's memory is not permitted",
        ),
        tonic::Code::NotFound => (
            "memory_not_found",
            StatusCode::NOT_FOUND,
            "the memory is not available",
        ),
        tonic::Code::InvalidArgument => (
            "invalid_request",
            StatusCode::BAD_REQUEST,
            "the Space memory request is invalid",
        ),
        _ => (
            "space_memory_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "Space memory is unavailable",
        ),
    };
    (
        http,
        Json(json!({"error": {"code": code, "message": message, "details": {}}})),
    )
}

#[derive(Debug, Serialize)]
struct SpaceMemorySummaryResponse {
    memory_id: String,
    content: String,
    updated_at: String,
    provenance: &'static str,
    /// `room_statement` or `member_save`.
    source_label: String,
    /// The thread the row was learned in, for the source link.
    thread_id: String,
    /// `audience_changed` when the room's current audience does not recall the row; omitted otherwise.
    #[serde(skip_serializing_if = "String::is_empty")]
    withheld_reason: String,
}

/// The memory of one Space, for its owner or admin (EP-8 S7). Session Core verifies the Control
/// Space memory decision; this hop checks the shape only. A read: a zero-retention caller is
/// answered empty by Session Core, as the personal list is.
async fn list_space_memories(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(space_id): Path<String>,
    Json(body): Json<SpaceMemoryBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListMemoryRequest;
    let (space_id, decision_ref, token) = space_memory_fields(&space_id, &body).ok_or_else(|| {
        space_memory_error(&tonic::Status::invalid_argument("invalid Space memory request"))
    })?;
    let response = state
        .memory_client
        .clone()
        .list_memory(authenticated_session_request(
            ListMemoryRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: body.limit.unwrap_or(100).clamp(1, 200),
                space_id,
                space_control_decision_ref: decision_ref,
                space_control_decision_token: token,
            },
            &bearer,
        )?)
        .await
        .map_err(|status| space_memory_error(&status))?
        .into_inner();
    let withheld = response
        .entries
        .iter()
        .filter(|entry| !entry.withheld_reason.is_empty())
        .count();
    let memories: Vec<SpaceMemorySummaryResponse> = response
        .entries
        .into_iter()
        .map(|entry| SpaceMemorySummaryResponse {
            memory_id: entry.memory_id,
            content: entry.content,
            updated_at: timestamp_to_rfc3339(entry.updated_at),
            provenance: memory_provenance_label(entry.provenance),
            source_label: entry.source_label,
            thread_id: entry.thread_id,
            withheld_reason: entry.withheld_reason,
        })
        .collect();
    Ok(Json(json!({
        "memories": memories,
        "withheld": withheld,
        "degraded": response.degraded,
        "degradation_reason": response.degradation_reason,
    })))
}

/// Deletes one row of a Space's memory under a decision issued for that row (EP-8 S7). Allowed
/// under every posture, as every delete is.
async fn delete_space_memory_entry(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(space_id): Path<String>,
    Json(body): Json<SpaceMemoryBody>,
) -> Result<Json<DeleteMemoryHttpResponse>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DeleteMemoryRequest;
    let memory_id = body.memory_id.trim().to_owned();
    let fields = space_memory_fields(&space_id, &body).filter(|_| !memory_id.is_empty());
    let (space_id, decision_ref, token) = fields.ok_or_else(|| {
        space_memory_error(&tonic::Status::invalid_argument("invalid Space memory request"))
    })?;
    let response = state
        .memory_client
        .clone()
        .delete_memory(authenticated_session_request(
            DeleteMemoryRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                memory_id,
                space_id,
                space_control_decision_ref: decision_ref,
                space_control_decision_token: token,
            },
            &bearer,
        )?)
        .await
        .map_err(|status| space_memory_error(&status))?
        .into_inner();
    Ok(Json(DeleteMemoryHttpResponse {
        deleted: response.deleted,
        degraded: response.degraded,
        degradation_reason: response.degradation_reason,
    }))
}

#[cfg(test)]
mod space_memory_route_tests {
    use super::*;

    fn body(decision_ref: &str, token: &str, memory_id: &str) -> SpaceMemoryBody {
        SpaceMemoryBody {
            space_control_decision_ref: decision_ref.to_owned(),
            space_control_decision_token: token.to_owned(),
            memory_id: memory_id.to_owned(),
            limit: None,
        }
    }

    #[test]
    fn space_memory_fields_are_bounded_and_trimmed() {
        assert_eq!(
            space_memory_fields(" space-1 ", &body(" ref ", " tok ", "")),
            Some(("space-1".to_owned(), "ref".to_owned(), "tok".to_owned()))
        );
        assert_eq!(space_memory_fields("  ", &body("ref", "tok", "")), None);
        assert_eq!(space_memory_fields(&"s".repeat(201), &body("ref", "tok", "")), None);
        assert_eq!(space_memory_fields("space-1", &body("ref", &"t".repeat(8 * 1024 + 1), "")), None);
        assert_eq!(space_memory_fields("space-1", &body("ref", "tok", &"m".repeat(201))), None);
    }

    #[test]
    fn space_memory_refusals_use_fixed_words_and_object_details() {
        let (status, Json(body)) =
            space_memory_error(&tonic::Status::permission_denied("Space membership required for space-1"));
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "space_memory_not_authorized");
        assert!(body["error"]["details"].is_object());
        assert!(!body.to_string().contains("space-1"), "upstream text never crosses");
        let (status, _) = space_memory_error(&tonic::Status::internal("db down"));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}

fn memory_state_label(tag: i32) -> &'static str {
    use mp_contracts::model_plane::v1::MemoryState;
    match MemoryState::try_from(tag) {
        Ok(MemoryState::On) => "on",
        Ok(MemoryState::Paused) => "paused",
        Ok(MemoryState::Off) => "off",
        _ => "unspecified",
    }
}

/// Only the three literal words; anything else is refused rather than defaulted,
/// because a typo must never silently turn memory back on.
fn parse_memory_state(word: &str) -> Option<mp_contracts::model_plane::v1::MemoryState> {
    use mp_contracts::model_plane::v1::MemoryState;
    match word {
        "on" => Some(MemoryState::On),
        "paused" => Some(MemoryState::Paused),
        "off" => Some(MemoryState::Off),
        _ => None,
    }
}

/// Content-free: the stored choice, the decision taken from it and why.
fn memory_controls_json(controls: &mp_contracts::model_plane::v1::MemoryControls) -> Value {
    json!({
        "state": memory_state_label(controls.state),
        "org_memory_policy": controls.org_memory_policy,
        "org_learning_policy": controls.org_learning_policy,
        "use_allowed": controls.use_allowed,
        "generation_allowed": controls.generation_allowed,
        "learning_allowed": controls.learning_allowed,
        "reason": controls.reason,
        "hidden_rows": controls.hidden_rows,
        "updated_at": controls
            .updated_at
            .as_ref()
            .map_or(Value::Null, prost_ts_to_rfc3339),
    })
}

/// The signed-in user's memory state and the decision taken from it. A ZDR
/// caller is answered `reason=zdr` by session-core, so there is no ZDR branch
/// here.
async fn get_memory_controls(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::GetMemoryControlsRequest;

    let response = state
        .memory_client
        .clone()
        .get_memory_controls(authenticated_session_request(
            GetMemoryControlsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core get_memory_controls failed", &e))?
        .into_inner();

    Ok(Json(
        response
            .controls
            .as_ref()
            .map_or(Value::Null, memory_controls_json),
    ))
}

#[derive(Debug, Deserialize)]
struct SetMemoryControlsBody {
    state: String,
}

/// Sets the signed-in user's own state. Org and user come from the verified
/// claims, never the body; session-core refuses a service, ZDR or waived
/// credential.
async fn set_memory_controls(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Json(body): Json<SetMemoryControlsBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::SetMemoryControlsRequest;

    let Some(requested) = parse_memory_state(body.state.trim()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_memory_state",
                    "message": "state must be one of: on, paused, off",
                }
            })),
        ));
    };

    let response = state
        .memory_client
        .clone()
        .set_memory_controls(authenticated_session_request(
            SetMemoryControlsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                state: requested as i32,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core set_memory_controls failed", &e))?
        .into_inner();

    Ok(Json(
        response
            .controls
            .as_ref()
            .map_or(Value::Null, memory_controls_json),
    ))
}

#[derive(Debug, Deserialize)]
struct ResetMemoryBody {
    #[serde(default)]
    confirm: bool,
}

/// Erases everything the signed-in user's memory holds. Not blocked for a ZDR
/// caller: erasure is allowed because rows from before ZDR may exist.
/// Irreversible, so an absent `confirm` is a refusal here as well as in
/// session-core.
async fn reset_memory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Json(body): Json<ResetMemoryBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ResetMemoryRequest;

    if !body.confirm {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "confirm_required",
                    "message": "resetting memory cannot be undone; send confirm: true",
                }
            })),
        ));
    }

    let response = state
        .memory_client
        .clone()
        .reset_memory(authenticated_session_request(
            ResetMemoryRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                confirm: true,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_memory_error("session-core reset_memory failed", &e))?
        .into_inner();

    Ok(Json(json!({
        "durable_deleted": response.durable_deleted,
        "semantic_unconfirmed": response.semantic_unconfirmed,
        "degraded": response.degraded,
        "degradation_reason": response.degradation_reason,
    })))
}

/// The profile as JSON: its text, the ids of the memories behind it (never their
/// content), how it was made, and why it is not used when it is not.
fn user_profile_json(response: &mp_contracts::model_plane::v1::UserProfileResponse) -> Value {
    json!({
        "profile": response.profile.as_ref().map_or(Value::Null, |profile| json!({
            "content": profile.content,
            "source_memory_ids": profile.source_memory_ids,
            "origin": profile.origin,
            "revision": profile.revision,
            "updated_at": profile
                .updated_at
                .as_ref()
                .map_or(Value::Null, prost_ts_to_rfc3339),
            "sources_deleted_since_edit": profile.sources_deleted_since_edit,
        })),
        "skip_reason": response.skip_reason,
    })
}

/// A profile write refused for a ZDR or waived credential keeps its catalogue
/// code and feature; every other refusal (`profile_not_enabled`,
/// `profile_revision_stale`, `memory_not_enabled:<reason>`) is mapped as the
/// other memory routes map theirs.
fn session_profile_error(context: &str, error: &tonic::Status) -> HttpJsonError {
    if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(error) {
        return (StatusCode::CONFLICT, Json(refusal.json_body()));
    }
    session_memory_error(context, error)
}

/// The signed-in user's standing profile. A ZDR caller is answered with no
/// profile and `skip_reason=zdr` by session-core, so there is no ZDR branch here.
async fn get_user_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::GetUserProfileRequest;

    let response = state
        .memory_client
        .clone()
        .get_user_profile(authenticated_session_request(
            GetUserProfileRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_profile_error("session-core get_user_profile failed", &e))?
        .into_inner();
    Ok(Json(user_profile_json(&response)))
}

#[derive(Debug, Deserialize)]
struct SetUserProfileBody {
    content: String,
    /// The revision the edit was made against; 0 (the default) when there was none.
    #[serde(default)]
    expected_revision: u64,
}

/// Stores the signed-in user's own edit of their profile. Org and user come from
/// the verified claims, never the body.
async fn set_user_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Json(body): Json<SetUserProfileBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::SetUserProfileRequest;

    let response = state
        .memory_client
        .clone()
        .set_user_profile(authenticated_session_request(
            SetUserProfileRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                content: body.content,
                expected_revision: body.expected_revision,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_profile_error("session-core set_user_profile failed", &e))?
        .into_inner();
    Ok(Json(user_profile_json(&response)))
}

#[derive(Debug, Default, Deserialize)]
struct RebuildUserProfileBody {
    /// Replace an edited profile too; without it the edit is kept.
    #[serde(default)]
    force: bool,
}

/// Rebuilds the signed-in user's profile from their accepted memories.
async fn rebuild_user_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Json(body): Json<RebuildUserProfileBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::RebuildUserProfileRequest;

    let response = state
        .memory_client
        .clone()
        .rebuild_user_profile(authenticated_session_request(
            RebuildUserProfileRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                force: body.force,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_profile_error("session-core rebuild_user_profile failed", &e))?
        .into_inner();
    Ok(Json(user_profile_json(&response)))
}

#[derive(Debug, Deserialize)]
struct LearningLedgerQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

/// "What Verevon learned": the signed-in user's ledger of skill changes and learned
/// memories, newest first, one page at a time. Whose ledger it is comes from the
/// verified claims, never from the query.
async fn list_learning_ledger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<LearningLedgerQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListLearningLedgerRequest;

    let response = state
        .learning_client
        .clone()
        .list_learning_ledger(authenticated_session_request(
            ListLearningLedgerRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                cursor: query.cursor.unwrap_or_default(),
                limit: query.limit.unwrap_or_default(),
                kinds: Vec::new(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_learning_error("session-core list_learning_ledger failed", &e))?
        .into_inner();

    Ok(Json(json!({
        "entries": response.entries.iter().map(learning_entry_json).collect::<Vec<_>>(),
        "next_cursor": response.next_cursor,
        "memory_skip_reason": response.memory_skip_reason,
    })))
}

#[derive(Debug, Deserialize)]
struct LearningDecisionBody {
    decision: String,
}

/// Accepts or rejects one learned-skill proposal. The owner is the verified caller,
/// and session-core refuses anyone else. A ZDR caller is refused before the hop: the
/// decision writes a revision that holds skill text.
async fn decide_learning_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(skill_id): Path<String>,
    Json(body): Json<LearningDecisionBody>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::DecideSkillProposalRequest;
    require_non_zdr_durable_mutation(&claims)?;

    let trimmed = skill_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "skill_id is required"})),
        ));
    }
    let Some(decision) = parse_skill_decision(body.decision.trim()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_skill_decision",
                    "message": "decision must be one of: accept, reject",
                }
            })),
        ));
    };

    let response = state
        .learning_client
        .clone()
        .decide_skill_proposal(authenticated_session_request(
            DecideSkillProposalRequest {
                org_id: claims.org_id.clone(),
                skill_id: trimmed.to_owned(),
                decision: decision as i32,
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_learning_error("session-core decide_skill_proposal failed", &e))?
        .into_inner();

    Ok(Json(json!({
        "review_state": response.review_state,
        "enabled": response.enabled,
        "revision": response.revision,
    })))
}

/// Undoes the newest change to one of the owner's learned skills: the snapshot taken
/// before it is restored, or the skill is removed if the change created it. A learned
/// memory is not undone here (session-core refuses it with `memory_undo_via_delete`),
/// and a ZDR caller is refused before the hop, as for a decision.
async fn undo_learning_entry(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(entry_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::UndoLearningEntryRequest;
    require_non_zdr_durable_mutation(&claims)?;

    let trimmed = entry_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "entry_id is required"})),
        ));
    }

    let response = state
        .learning_client
        .clone()
        .undo_learning_entry(authenticated_session_request(
            UndoLearningEntryRequest {
                org_id: claims.org_id.clone(),
                entry_id: trimmed.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_learning_error("session-core undo_learning_entry failed", &e))?
        .into_inner();

    Ok(Json(json!({
        "action": response.action,
        "skill_id": response.skill_id,
        "revision": response.revision,
    })))
}

#[allow(clippy::too_many_lines)]
async fn invoke(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    model_bearer: VerifiedModelBearer,
    inference_bearer: VerifiedInferenceBearer,
    capability_bearer: Option<Extension<VerifiedCapabilityBearer>>,
    cost_bearer: Option<Extension<VerifiedCostBearer>>,
    Json(mut req): Json<InvokeRequest>,
) -> Result<Json<InvokeResponse>, InvokeError> {
    if req.features.iter().any(|f| f == "conversation_only") {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": {
            "code": "source_scope_requires_stream", "message": "Conversation-only tasks require the streaming chat endpoint."
        }}))).into());
    }
    // An agent mode this build does not know is refused before anything runs (WS25-S6). The unary
    // endpoint runs no agent loop, so a known mode changes nothing here and is not applied.
    if crate::profile::chosen_agent_mode(
        crate::profile::agent_modes_enabled(),
        req.agent_mode.as_deref(),
    )
    .is_err()
    {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": {
            "code": mp_contracts::agent_mode::UNKNOWN_MODE_CODE, "message": "The agent mode is not one this server knows."
        }}))).into());
    }
    let start = std::time::Instant::now();

    // The turn's posture, decided once through the resolver before anything else: a ZDR token on
    // the user's own connection is refused, typed, with no inference call; a waiver is honoured
    // only on the connection it names; every other combination follows the posture matrix (see
    // `zdr_posture`). The refusal is typed instead of the generic 502 the connection used to get.
    let route = crate::zdr_posture::Route::named(
        req.provider.as_deref(),
        req.subscription_connection_id.as_deref(),
        req.zdr,
    );
    let posture = match crate::zdr_posture::decide_for(&claims, &route) {
        Ok(posture) => posture,
        Err(refusal) => return Err(crate::zdr_posture::http(&refusal).into()),
    };
    if posture.is_waived() && (req.deep_research || req.generate_image) {
        return Err(crate::zdr_posture::http(&mp_contracts::zdr_refusal::Refusal::WAIVER_ROUTE_MISMATCH).into());
    }

    // Normalize and validate the request
    let mut normalized = normalize::normalize(&req)?;
    // A regenerate or an edit needs the message tree. With its switch off it is refused before any thread,
    // run or branch is touched (WS17 S9).
    let version_turn = normalize::version_turn(&req)?;
    if version_turn.is_some() && !message_versions_enabled() {
        return Err(history_versions_disabled().into());
    }
    // The organisation's model policy, asked of inference-core before anything
    // durable exists: a refused turn leaves no thread, message or run behind, and
    // the ZDR branch below is under the same policy as the durable one.
    let admitted = crate::model_access::admit(
        &state.inference_client,
        authenticated_inference_request((), &inference_bearer).metadata(),
        &claims.org_id,
        &normalized.model,
        &normalized.provider_hint,
    )
    .await
    .map_err(|refusal| InvokeError::from(crate::model_access::http(&refusal)))?;
    if let Some(model) = admitted.model.clone() {
        normalized.model = model;
    }
    admitted.clamp_effort(&mut req.effort, crate::thinking::ladder_enabled());
    let effective_zdr = posture.zdr();
    let persistence = invoke_persistence_plan(effective_zdr);
    let capability_bearer = capability_bearer.map(|Extension(bearer)| bearer);
    let pii_redaction_required = crate::moderation::pii_redaction_required(
        &req.features,
        &state.http_client,
        &state.capability_core_base_url,
        capability_bearer
            .as_ref()
            .map(VerifiedCapabilityBearer::as_str),
    )
    .await;

    // ZDR is a separate, deliberately narrow path. It must branch before the
    // idempotency registry, budget/session clients, event publisher, and any
    // response cache so request or response content cannot become durable.
    if !persistence.all_durable_effects_allowed() {
        // What the persistence-free path cannot do is refused here, typed, before
        // any model call (`zdr_features`); what it can do is honored below, with
        // the same settings as the streaming path.
        if let Some(refusal) = crate::zdr_features::refusal(&req) {
            return Err(crate::zdr_precheck::http(&refusal).into());
        }
        let settings = crate::sse::zdr_turn_settings(&state, &req);
        let request_id = new_ulid();
        let user_content = if pii_redaction_required {
            crate::moderation::redact_pii(&normalized.content).0
        } else {
            normalized.content.clone()
        };
        let infer_resp = state
            .inference_client
            .clone()
            .infer(authenticated_inference_request(
                mp_contracts::model_plane::v1::InferRequest {
                    purpose: InferPurpose::Answer as i32,
                    request_id: request_id.clone(),
                    org_id: claims.org_id.clone(),
                    model: normalized.model.clone(),
                    provider_hint: normalized.provider_hint.clone(),
                    subscription_connection_id: normalized.subscription_connection_id.clone(),
                    messages: settings.messages(&user_content),
                    temperature: 0.7,
                    max_tokens: settings.max_tokens,
                    thinking_budget_tokens: settings.thinking.budget_tokens,
                    reasoning_effort: settings.thinking.effort_wire(),
                    structured_output_schema: normalized
                        .structured_output_schema
                        .clone()
                        .unwrap_or_default(),
                    zdr: true,
                    // Privacy tier applies on the persistence-free path too:
                    // retention and geography are independent axes, so an
                    // EU-resident ephemeral turn still gets the residency gate.
                    min_privacy_tier: crate::normalize::min_privacy_tier_wire(&normalized),
                    ..Default::default()
                },
                &inference_bearer,
            ))
            .await
            .map_err(|error| {
                // No run was prepared on this path, so there is nothing to
                // terminalize. A typed ZDR refusal from inference-core (the
                // attestation gate) keeps its catalogue code and the 412 every
                // ZDR precondition answers with. Without this it became a flat
                // 502 that a client cannot match on.
                if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(&error) {
                    let answer: InvokeError = crate::zdr_precheck::http(&refusal).into();
                    return answer;
                }
                let subscription = crate::subscription_failure::on_route(
                    &error,
                    !normalized.subscription_connection_id.trim().is_empty(),
                );
                invoke_inference_error(&error, subscription.as_ref())
            })?
            .into_inner();

        return Ok(Json(InvokeResponse {
            request_id,
            usage: Some(invoke_usage(
                &infer_resp.content,
                infer_resp.input_tokens,
                infer_resp.output_tokens,
                None,
                u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            )),
            content: infer_resp.content,
            model_used: infer_resp.model_used,
        }));
    }

    // chat-parity §1 — idempotent regenerate. A duplicate `/v1/invoke` carrying
    // the same `idempotency_key` returns the original response (no second
    // inference run, no second budget charge); a concurrent duplicate is 409.
    // The guard releases its claim on any early-return below (Drop), so an
    // errored request does not wedge the key.
    let idem_guard = match req
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        Some(key) => match state
            .idempotency
            .claim(&claims.org_id, &claims.user_id, key)
        {
            crate::idempotency_registry::Claim::Cached(v) => {
                return Ok(Json(InvokeResponse {
                    request_id: v.request_id,
                    content: v.content,
                    model_used: v.model_used,
                    usage: cached_invoke_usage(v.usage),
                }));
            }
            crate::idempotency_registry::Claim::InFlight => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": "duplicate request in flight for this idempotency_key",
                    })),
                ).into());
            }
            crate::idempotency_registry::Claim::Rejected(
                crate::idempotency_registry::ClaimRejection::InvalidKey,
            ) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "idempotency_key is invalid or too large",
                    })),
                ).into());
            }
            crate::idempotency_registry::Claim::Rejected(
                crate::idempotency_registry::ClaimRejection::CapacityExceeded,
            ) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "idempotency protection is temporarily unavailable",
                    })),
                ).into());
            }
            crate::idempotency_registry::Claim::Proceed(guard) => Some(guard),
        },
        None => None,
    };

    // Pre-flight budget check against cost-core
    let verified_bearer = cost_bearer
        .as_ref()
        .map_or("", |Extension(bearer)| bearer.as_str());
    // Same org ceilings the streaming path applies, so the two entry points
    // cannot disagree about what an org is allowed to spend.
    let org_limits = crate::org_quota::fetch_org_limits(
        &state.http_client,
        &state.org_core_base_url,
        &claims.org_id,
        &state.org_core_service_id,
        &state.org_core_service_token,
    )
    .await;
    crate::budget::check_budget(
        &state.http_client,
        &claims.org_id,
        &claims.user_id,
        verified_bearer,
        &normalized,
        org_limits,
    )
    .await?;

    let request_id = new_ulid();
    // `start_key` is durable in Session Core. Transform public client retry
    // data before it crosses that boundary so ZDR never persists raw request
    // or idempotency text, while keeping same-org/user retries deterministic.
    let managed_start_key = state.managed_start_keys.derive(
        &claims.org_id,
        &claims.user_id,
        req.idempotency_key.as_deref(),
        &request_id,
        "gateway-direct",
    );
    // A regenerate or an edit moves the thread's branch before the run that answers it starts (WS17 S9). A
    // refused move starts nothing, so the thread keeps the branch it had.
    let version_thread = match version_turn {
        Some(turn) => {
            let thread_id = normalized.thread_id.clone().unwrap_or_default();
            let version_move = session_flow::move_active_leaf(
                &state,
                &claims.org_id,
                &thread_id,
                &turn,
                &model_bearer,
            )
            .await
            .map_err(|error| branch_thread_error(&error))?;
            if let Some(goal) = version_move.goal {
                normalized.content = goal;
            }
            Some((thread_id, turn))
        }
        None => None,
    };
    let started = match version_thread.as_ref() {
        Some((thread_id, turn)) => {
            session_flow::prepare_version_run_authenticated(
                &state,
                thread_id,
                &claims.org_id,
                &claims.user_id,
                &normalized.content,
                turn,
                &managed_start_key,
                &model_bearer,
            )
            .await
        }
        None => {
            session_flow::prepare_managed_run_authenticated_with_receipt(
                &state,
                normalized.thread_id.as_deref(),
                normalized.session_key.as_deref(),
                &claims.org_id,
                &claims.user_id,
                &normalized.content,
                "model-gateway",
                "execute",
                &managed_start_key,
                mp_contracts::model_plane::v1::ManagedRunSource::GatewayDirect,
                effective_zdr,
                &model_bearer,
                normalized.space_context.as_ref(),
                normalized.space_append_context.as_ref(),
                false,
                None,
                // The prompt is stored under the send's own id, so a retry of it lands as one row.
                session_flow::send_receipt_id(
                    crate::queued_input::send_receipts_enabled(),
                    effective_zdr,
                    req.regenerated,
                    req.edited_resubmit,
                    req.idempotency_key.as_deref(),
                ),
                None,
            )
            .await
        }
    };
    let session_run = started.map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(
                serde_json::json!({"error": format!("session-core managed start failed: {error}")}),
            ),
        )
    })?;
    if session_run.already_started {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "managed run already exists; observe or resume the existing run",
                "run_id": session_run.run_id,
            })),
        ).into());
    }
    // Do not contact a provider until Session Core has accepted a
    // GatewayDirect heartbeat using the Gateway's fixed-scope workload token.
    // In particular, the verified user bearer above is not an authority for
    // this managed-run lease.
    session_flow::ensure_direct_inference_run_liveness(&state, &session_run)
        .await
        .map_err(|error| {
            warn!(%error, run_id = %session_run.run_id, "initial direct inference liveness heartbeat failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "session-core liveness heartbeat failed"})),
            )
        })?;

    // Emit ingress.accepted envelope
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "INGRESS_ACCEPTED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.clone(),
        causation_id: String::new(),
        idempotency_key: request_id.clone(),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "content_length": normalized.content.len(),
            "model": normalized.model,
        }),
        zdr: effective_zdr,
        ..Default::default()
    };

    if let Err(error) = state
        .publisher
        .publish(&subjects::ingress_subject("accepted"), &envelope)
        .await
    {
        warn!(%error, "failed to publish INGRESS_ACCEPTED");
        if let Err(terminal_error) = session_flow::terminalize_direct_inference_run_authenticated(
            &state,
            &session_run,
            session_flow::DirectInferenceTerminal::Failed("ingress_publish_failed"),
            &model_bearer,
        )
        .await
        {
            warn!(%terminal_error, run_id = %session_run.run_id, "failed to terminalize aborted direct inference run");
        }
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal server error"})),
        ).into());
    }

    // Safety policy is resolved from capability-core per request; client
    // features can add PII redaction but cannot disable the org's policy.
    let user_content = if pii_redaction_required {
        crate::moderation::redact_pii(&normalized.content).0
    } else {
        normalized.content.clone()
    };

    // Call inference-core
    let infer_resp = {
        use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
        let response = state
            .inference_client
            .clone()
            .infer(authenticated_inference_request(
                InferRequest {
                    purpose: InferPurpose::Answer as i32,
                    request_id: request_id.clone(),
                    org_id: claims.org_id.clone(),
                    model: normalized.model.clone(),
                    provider_hint: normalized.provider_hint.clone(),
                    subscription_connection_id: normalized.subscription_connection_id.clone(),
                    messages: vec![ChatMessage {
                        compaction_summary: String::new(),
                        role: "user".to_owned(),
                        content: user_content,
                        name: String::new(),
                    }],
                    temperature: 0.7,
                    max_tokens: 4096,
                    structured_output_schema: normalized
                        .structured_output_schema
                        .clone()
                        .unwrap_or_default(),
                    zdr: effective_zdr,
                    min_privacy_tier: crate::normalize::min_privacy_tier_wire(&normalized),
                    ..Default::default()
                },
                &inference_bearer,
            ))
            .await;
        match response {
            Ok(response) => response.into_inner(),
            Err(error) => {
                // On the subscription route the reason is typed, an overflow
                // is `prompt_too_long` (not retried, closed as the request's
                // own failure), and anything else is the generic route
                // failure; the run records its code.
                let subscription_route = !normalized.subscription_connection_id.trim().is_empty();
                let subscription_failure =
                    crate::subscription_failure::on_route_or_generic(&error, subscription_route);
                let terminal_code = subscription_failure
                    .as_ref()
                    .map_or("inference_unavailable", |failure| failure.code);
                if let Err(terminal_error) =
                    session_flow::terminalize_direct_inference_run_authenticated(
                        &state,
                        &session_run,
                        session_flow::DirectInferenceTerminal::Failed(terminal_code),
                        &model_bearer,
                    )
                    .await
                {
                    warn!(%terminal_error, run_id = %session_run.run_id, "failed to terminalize failed direct inference run");
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({"error": "session-core terminalization failed"})),
                    ).into());
                }
                return Err(invoke_inference_error(&error, subscription_failure.as_ref()));
            }
        }
    };

    if !effective_zdr {
        if let Err(error) = session_flow::append_assistant_message_authenticated(
            &state,
            &session_run.thread_id,
            &infer_resp.content,
            &model_bearer,
            req.agent_name.as_deref(),
            // The unary invoke path does no retrieval of its own, so there is
            // no grounding to record. `InvokeResponse.sources` is a separate,
            // response-only field this append never sees.
            None,
        )
        .await
        {
            if let Err(terminal_error) =
                session_flow::terminalize_direct_inference_run_authenticated(
                    &state,
                    &session_run,
                    session_flow::DirectInferenceTerminal::Failed("assistant_persist_failed"),
                    &model_bearer,
                )
                .await
            {
                warn!(%terminal_error, run_id = %session_run.run_id, "failed to terminalize assistant persistence failure");
            }
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(
                    serde_json::json!({"error": format!("session-core append assistant failed: {error}")}),
                ),
            ).into());
        }
    }

    session_flow::terminalize_direct_inference_run_authenticated(
        &state,
        &session_run,
        session_flow::DirectInferenceTerminal::Completed,
        &model_bearer,
    )
    .await
    .map_err(|error| {
        warn!(%error, run_id = %session_run.run_id, "failed to terminalize completed direct inference run");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "session-core terminalization failed"})),
        )
    })?;

    info!(run_id = %session_run.run_id, thread_id = %session_run.thread_id, request_id = %request_id, "http invoke completed");

    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    // A subscription call is never priced from API list prices.
    let cost_basis = crate::pricing::CostBasis::of_route(
        &normalized.provider_hint,
        &normalized.subscription_connection_id,
    )
    .served_by(&infer_resp.provider_used);
    let usage = invoke_usage(
        &infer_resp.content,
        infer_resp.input_tokens,
        infer_resp.output_tokens,
        state
            .pricing
            .cost_usd_for(
                cost_basis,
                &infer_resp.model_used,
                i64::from(infer_resp.input_tokens.max(0)),
                i64::from(infer_resp.output_tokens.max(0)),
                i64::from(infer_resp.cache_read_input_tokens.max(0)),
                i64::from(infer_resp.cache_creation_input_tokens.max(0)),
            )
            .await,
        latency_ms,
    );

    // Emit usage envelope
    let usage_envelope = Envelope {
        event_id: new_ulid(),
        event_type: "USAGE_ENVELOPE".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.clone(),
        causation_id: String::new(),
        idempotency_key: format!("{request_id}-USAGE_ENVELOPE"),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "request_id": request_id,
            "org_id": claims.org_id,
            "user_id": claims.user_id,
            "model": infer_resp.model_used,
            "input_tokens": infer_resp.input_tokens,
            "output_tokens": infer_resp.output_tokens,
            "latency_ms": latency_ms,
            // Phase-4 provenance receipt inputs, stamped beside the zdr flag:
            // which deployment processed the content and under what residency.
            "provider_used": infer_resp.provider_used,
            "residency": infer_resp.residency,
            "min_privacy_tier": crate::normalize::min_privacy_tier_wire(&normalized),
            // Cache-token telemetry (native-compaction migration
            // prerequisite): already folded into `input_tokens` above, but
            // broken out so cache-hit rate and savings are queryable off the
            // durable ledger. 0 (never absent) when this turn used no cache.
            "cache_read_input_tokens": infer_resp.cache_read_input_tokens,
            "cache_creation_input_tokens": infer_resp.cache_creation_input_tokens,
            "cost_basis": cost_basis.as_str(),
        }),
        zdr: effective_zdr,
        ..Default::default()
    };

    if let Err(e) = state
        .publisher
        .publish(&subjects::usage_subject(&claims.org_id), &usage_envelope)
        .await
    {
        warn!(error = %e, "failed to publish USAGE_ENVELOPE");
    }

    // Cache the completed result so a later duplicate with this key replays it
    // verbatim. No-op when the client supplied no key.
    if let Some(guard) = idem_guard {
        guard.commit(crate::idempotency_registry::CachedInvoke {
            request_id: request_id.clone(),
            content: infer_resp.content.clone(),
            model_used: infer_resp.model_used.clone(),
            usage: Some(crate::idempotency_registry::CachedInvokeUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cost_usd: usage.cost_usd,
                latency_ms: usage.latency_ms,
                confidence: usage.confidence,
            }),
        });
    }

    Ok(Json(InvokeResponse {
        request_id,
        content: infer_resp.content,
        model_used: infer_resp.model_used,
        usage: Some(usage),
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvokePersistencePlan {
    Durable,
    SuppressAllForZdr,
}

impl InvokePersistencePlan {
    fn all_durable_effects_allowed(self) -> bool {
        self == Self::Durable
    }
}

fn invoke_persistence_plan(zdr: bool) -> InvokePersistencePlan {
    if zdr {
        InvokePersistencePlan::SuppressAllForZdr
    } else {
        InvokePersistencePlan::Durable
    }
}


#[derive(Deserialize)]
struct ToonEncodeRequest {
    payload: Value,
}

#[derive(Serialize)]
struct ToonEncodeResponse {
    toon: String,
    chars: usize,
    estimated_tokens: u32,
}

/// Encode an arbitrary JSON payload as TOON for compact prompt/tool I/O.
/// Returns the encoded string plus a token estimate so callers can decide
/// whether the savings clear their threshold.
async fn toon_encode(
    Extension(claims): Extension<Claims>,
    Json(req): Json<ToonEncodeRequest>,
) -> Result<Json<ToonEncodeResponse>, (StatusCode, Json<Value>)> {
    let _ = claims;
    let toon = mp_toon::encode(&req.payload);
    let chars = toon.len();
    let estimated_tokens = mp_toon::estimate_tokens(&toon);
    Ok(Json(ToonEncodeResponse {
        toon,
        chars,
        estimated_tokens,
    }))
}

#[cfg(test)]
mod browser_suggestion_tests {
    use super::*;

    #[test]
    fn browser_suggestion_maps_click_to_client_action_shape() {
        let suggestion = browser_suggestion_value(&json!({
            "action": "click",
            "selector": "button.submit",
            "reason": "Submit button is visible.",
            "confidence": 0.82
        }));

        assert_eq!(suggestion["done"], false);
        assert_eq!(
            suggestion["action"],
            json!({
                "type": "click",
                "selector": "button.submit"
            })
        );
        assert_eq!(suggestion["reason"], "Submit button is visible.");
        assert_eq!(suggestion["confidence"], 0.82);
    }

    #[test]
    fn browser_suggestion_maps_done_without_action() {
        let suggestion = browser_suggestion_value(&json!({
            "action": "done",
            "reason": "Evidence is already visible.",
            "confidence": 0.91
        }));

        assert_eq!(suggestion["done"], true);
        assert_eq!(suggestion["action"], Value::Null);
    }

    #[test]
    fn browser_suggestion_drops_incomplete_selector_actions() {
        let suggestion = browser_suggestion_value(&json!({
            "action": "click",
            "reason": "No selector.",
            "confidence": 0.4
        }));

        assert_eq!(suggestion["done"], false);
        assert_eq!(suggestion["action"], Value::Null);
    }

    #[test]
    fn browser_suggestion_clamps_wait_time_and_confidence() {
        let suggestion = browser_suggestion_value(&json!({
            "action": "wait_for",
            "selector": "#ready",
            "timeout_ms": 120_000,
            "reason": "Wait for dynamic content.",
            "confidence": 2.0
        }));

        assert_eq!(
            suggestion["action"],
            json!({
                "type": "wait_for",
                "selector": "#ready",
                "timeout_ms": 15000
            })
        );
        assert_eq!(suggestion["confidence"], 1.0);
    }

    #[test]
    fn browser_planner_prompt_carries_the_bot_wall_rule_after_its_instructions() {
        let prompt = browser_suggest_system_prompt();
        assert!(prompt.starts_with(BROWSER_SUGGEST_SYSTEM_PROMPT));
        assert!(prompt.ends_with(BROWSER_CHALLENGE_RULE));
        assert!(BROWSER_CHALLENGE_RULE.contains("Never solve a captcha"));
        assert!(BROWSER_CHALLENGE_RULE.contains("up to 5 seconds"));
        assert!(BROWSER_CHALLENGE_RULE.contains("bare 403"));
    }

    #[test]
    fn browser_prompt_truncates_large_observations() {
        let req = BrowserSuggestActionRequest {
            goal: "Inspect page".to_owned(),
            observation: json!({ "text": "x".repeat(BROWSER_OBSERVATION_PROMPT_CHARS + 100) }),
            ..Default::default()
        };

        let prompt = browser_suggestion_prompt(&req, None);

        assert!(prompt.contains("...[truncated]"));
        assert!(prompt.contains("Inspect page"));
    }

    /// WS12.S4: a credential in the page (an accessibility value the user typed, a title,
    /// a URL query, the vision model's reading of the screenshot) never reaches the
    /// planner prompt; ordinary prose and ordinary URLs come through untouched. Secrets
    /// are built from fragments so no secret-shaped literal sits in the source.
    #[test]
    fn browser_suggestion_prompt_masks_ax_value_and_visual_summary() {
        let jwt = [
            "eyJhbGciOiJI",
            "UzI1NiIsInR5cCI6IkpXVCJ9",
            ".eyJzdWIiOiIxMjM0NTY3ODkw",
            ".SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        ]
        .concat();
        let key = ["sk-proj-", "Ab12Cd34Ef56", "Gh78Ij90Kl12"].concat();
        let pat = ["ghp_", "aB3dE6gH9jK2mN5pQ8sT1uV4wX7yZ0"].concat();
        let req = BrowserSuggestActionRequest {
            goal: "Log in".to_owned(),
            url: format!("https://app.example.com/cb?page=2&access_token={pat}&limit=5"),
            title: format!("Welcome back {key}"),
            observation: json!({
                "extracted_text": "the bearer of the message arrived",
                "page_url": format!("https://svc:hunter2-pw@h.example/p?token={pat}"),
                "ax": [{ "role": "textbox", "value": jwt, "name": "Token" }],
            }),
            visual_observation: json!({ "ocr": [format!("key {key}")] }),
            ..Default::default()
        };

        let prompt = browser_suggestion_prompt(
            &req,
            Some(&format!("The page shows an API key field containing {key}.")),
        );

        for secret in [&jwt, &key, &pat, &"hunter2-pw".to_owned()] {
            assert!(!prompt.contains(secret.as_str()), "secret leaked: {prompt}");
        }
        assert!(prompt.contains("access_token=[REDACTED]&limit=5"));
        assert!(prompt.contains("the bearer of the message arrived"));
        assert!(prompt.contains("Welcome back [REDACTED]"));
        assert!(prompt.contains("h.example/p?token=[REDACTED]"));
        // The request is read, never rewritten.
        assert!(req.observation.to_string().contains("eyJhbGciOiJI"));
    }

    #[test]
    fn browser_suggestion_prompt_leaves_an_ordinary_page_alone() {
        let req = BrowserSuggestActionRequest {
            goal: "Inspect page".to_owned(),
            url: "https://example.com/search?q=rust&page=2#top".to_owned(),
            title: "Search results".to_owned(),
            observation: json!({ "extracted_text": "max_tokens=4096 status=completed" }),
            ..Default::default()
        };
        let prompt = browser_suggestion_prompt(&req, Some("a login form"));
        assert!(prompt.contains("https://example.com/search?q=rust&page=2#top"));
        assert!(prompt.contains("max_tokens=4096 status=completed"));
        assert!(prompt.contains("a login form"));
        assert!(!prompt.contains("[REDACTED]"));
    }
}

#[cfg(test)]
mod invoke_zdr_tests {
    use super::*;

    #[test]
    fn zdr_invoke_plan_suppresses_every_durable_gateway_effect() {
        let plan = invoke_persistence_plan(true);
        assert_eq!(plan, InvokePersistencePlan::SuppressAllForZdr);
        assert!(!plan.all_durable_effects_allowed());

        let ordinary = invoke_persistence_plan(false);
        assert_eq!(ordinary, InvokePersistencePlan::Durable);
        assert!(ordinary.all_durable_effects_allowed());
    }

    #[test]
    fn issuer_enforced_zdr_cannot_be_downgraded_by_the_request_body() {
        let claims = Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: true,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        };

        assert!(crate::zdr_posture::decide_for(&claims, &crate::zdr_posture::Route::plain(false))
            .expect("a ZDR token decides")
            .zdr());
        assert_eq!(
            invoke_persistence_plan(true),
            InvokePersistencePlan::SuppressAllForZdr
        );
    }
}

#[cfg(test)]
mod approval_resume_auth_tests {
    use super::*;
    use crate::auth::VerifiedExecutionBearer;

    #[test]
    fn approval_resume_uses_execution_ingress_and_separate_session_delegation() {
        let request = authenticated_execution_request(
            ResumeRunRequest::default(),
            &VerifiedExecutionBearer::for_test("execution-core-token"),
            &VerifiedModelBearer::for_test("session-core-token"),
        )
        .expect("verified downstream credentials must be forwardable");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer execution-core-token")
        );
        assert_eq!(
            request
                .metadata()
                .get("x-session-authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer session-core-token")
        );
    }

    #[test]
    fn execution_resume_acknowledgement_fails_closed() {
        let error = require_execution_resume_ack(ResumeRunResponse {
            resumed: false,
            step_index: 7,
        })
        .expect_err("a non-resume acknowledgement must not be reported as success");
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);

        let acknowledged = require_execution_resume_ack(ResumeRunResponse {
            resumed: true,
            step_index: 7,
        })
        .expect("execution acknowledgement");
        assert!(acknowledged.resumed);
    }

    #[test]
    fn identical_granted_decision_retry_does_not_request_another_resume() {
        let mut prior = Approval {
            id: "approval-1".to_owned(),
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            state: ApprovalState::Requested as i32,
            ..Default::default()
        };
        let decided = Approval {
            state: ApprovalState::Granted as i32,
            ..prior.clone()
        };

        assert!(should_resume_granted_approval(Some(&prior), &decided)
            .expect("a fresh requested-to-granted transition is resumable"));
        prior.state = ApprovalState::Granted as i32;
        let retry_error = should_resume_granted_approval(Some(&prior), &decided)
            .expect_err("granted retry has unknown execution delivery");
        assert_eq!(retry_error.0, StatusCode::SERVICE_UNAVAILABLE);

        prior.state = ApprovalState::Requested as i32;
        let wrong_run = Approval {
            run_id: "run-other".to_owned(),
            ..decided.clone()
        };
        assert!(should_resume_granted_approval(Some(&prior), &wrong_run).is_err());
    }

    #[test]
    fn issuer_zdr_blocks_run_lifecycle_persistence() {
        let claims = Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: true,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        };
        let error = require_non_zdr_durable_mutation(&claims)
            .expect_err("issuer ZDR must block cancellation/resume/approval events");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
    }
}

#[cfg(test)]
mod capability_contract_tests {
    use super::*;
    use crate::auth::VerifiedCapabilityBearer;
    use mp_contracts::model_plane::v1::CapabilityDetail;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::net::TcpListener;

    fn proxy_claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    async fn proxy_counter() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new().fallback(axum::routing::any({
            let calls = Arc::clone(&calls);
            move || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Json(json!({}))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind capability proxy counter");
        let address = listener
            .local_addr()
            .expect("capability proxy counter address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve capability proxy counter");
        });
        (format!("http://{address}"), calls, task)
    }

    #[test]
    fn gateway_preserves_authoritative_capability_availability_semantics() {
        let value = capability_contract_value(&CapabilityDetail {
            capability_id: "cap.shipping.quote".to_owned(),
            name: "Shipping quote".to_owned(),
            kind: "tool".to_owned(),
            version: "1".to_owned(),
            description: "Read-only quote".to_owned(),
            risk_level: "medium".to_owned(),
            lazy_load: true,
            scope: "tenant".to_owned(),
            state: "approval_required".to_owned(),
            reason_code: "approval_required".to_owned(),
            reason: "A governed run is required.".to_owned(),
            requires_approval: true,
            execution_mode: "agentic".to_owned(),
            cost_class: "variable".to_owned(),
            health_checked_at: "2026-07-13T15:00:00Z".to_owned(),
            privacy_tier: 0,
            residency: String::new(),
            side_effect_class: String::new(),
            approval_floor: String::new(),
        });

        assert_eq!(value["state"], "approval_required");
        assert_eq!(value["reason_code"], "approval_required");
        assert_eq!(value["requires_approval"], true);
        assert_eq!(value["execution_mode"], "agentic");
        assert_eq!(value["cost_class"], "variable");
        assert_eq!(value["health_checked_at"], "2026-07-13T15:00:00Z");
    }

    #[test]
    fn mixed_version_capability_response_fails_closed() {
        let value = capability_contract_value(&CapabilityDetail {
            capability_id: "cap.legacy".to_owned(),
            ..CapabilityDetail::default()
        });

        assert_eq!(value["state"], "unavailable");
        assert_eq!(value["reason_code"], "availability_contract_missing");
        assert_eq!(value["execution_mode"], "unavailable");
        assert_eq!(value["cost_class"], "unknown");
    }

    #[test]
    fn public_capability_json_carries_the_declared_class_and_floor() {
        for (class, floor, want_class, want_floor) in [
            ("read", "none", "read", "none"),
            ("read", "ask", "read", "ask"),
            ("context_write", "none", "context_write", "none"),
            ("external_write", "none", "external_write", "none"),
            (
                "owner_effect",
                "durable_owner",
                "owner_effect",
                "durable_owner",
            ),
            // An owner effect is never reported below its durable owner floor.
            ("owner_effect", "none", "owner_effect", "durable_owner"),
            // An unclassified class is never reported below an ask.
            ("unclassified", "none", "unclassified", "ask"),
        ] {
            let value = capability_contract_value(&CapabilityDetail {
                capability_id: "cap.x".to_owned(),
                side_effect_class: class.to_owned(),
                approval_floor: floor.to_owned(),
                ..CapabilityDetail::default()
            });
            assert_eq!(value["side_effect_class"], want_class, "{class}/{floor}");
            assert_eq!(value["approval_floor"], want_floor, "{class}/{floor}");
        }
    }

    #[test]
    fn an_empty_or_unknown_class_and_floor_are_emitted_as_the_strictest_never_omitted() {
        // (class, floor, class reported): every floor here is empty or unknown,
        // so the floor reported is the strictest whatever the class is.
        for (class, floor, want_class) in [
            ("", "", "unclassified"),
            ("readonly", "required", "unclassified"),
            ("READ", "NONE", "unclassified"),
            ("read", "", "read"),
            ("destructive", "unknown", "destructive"),
        ] {
            let value = capability_contract_value(&CapabilityDetail {
                capability_id: "cap.x".to_owned(),
                side_effect_class: class.to_owned(),
                approval_floor: floor.to_owned(),
                ..CapabilityDetail::default()
            });
            let object = value.as_object().expect("capability JSON is an object");
            assert!(
                object.contains_key("side_effect_class"),
                "{class:?}/{floor:?}"
            );
            assert!(object.contains_key("approval_floor"), "{class:?}/{floor:?}");
            assert_eq!(
                value["side_effect_class"], want_class,
                "{class:?}/{floor:?}"
            );
            assert_eq!(
                value["approval_floor"], "durable_owner",
                "{class:?}/{floor:?}"
            );
        }
        // An empty class with a stored floor of none is an unclassified class,
        // which is never reported below an ask.
        let value = capability_contract_value(&CapabilityDetail {
            capability_id: "cap.x".to_owned(),
            approval_floor: "none".to_owned(),
            ..CapabilityDetail::default()
        });
        assert_eq!(value["side_effect_class"], "unclassified");
        assert_eq!(value["approval_floor"], "ask");
    }

    #[tokio::test]
    async fn issuer_zdr_capability_mutations_do_not_forward_or_touch_mcp_registry() {
        let (base_url, calls, task) = proxy_counter().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let zdr = proxy_claims(true);
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");

        let mutation = proxy_to_capability_core(
            &state,
            &zdr,
            &bearer,
            "memory",
            "POST",
            Some(&json!({"zdr": false, "content": "must not forward"})),
        )
        .await
        .expect_err("issuer-ZDR durable capability mutation must fail locally");
        assert_eq!(mutation.0, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "ZDR mutation was forwarded"
        );

        let mcp = mcp_register(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            HeaderMap::new(),
            Json(McpRegisterBody {
                name: "must-not-register".to_owned(),
                url: "https://mcp.example.test".to_owned(),
                transport: "http".to_owned(),
                token: String::new(),
                tool_allowlist: vec!["records.read".to_owned()],
                enabled: true,
                server_id: String::new(),
                scope: "user".to_owned(),
            }),
        )
        .await
        .expect_err("issuer-ZDR MCP registration must fail locally");
        assert_eq!(mcp.0, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "ZDR MCP registration was forwarded"
        );
        let listed = mcp_list(
            State(state.clone()),
            Extension(zdr),
            bearer.clone(),
            HeaderMap::new(),
        )
        .await
        .expect("read-only MCP listing remains allowed")
        .0;
        assert_eq!(listed["data"]["servers"], json!([]));

        let non_zdr = proxy_claims(false);
        let _ = proxy_to_capability_core(
            &state,
            &non_zdr,
            &bearer,
            "memory",
            "POST",
            Some(&json!({"content": "explicitly allowed"})),
        )
        .await
        .expect("explicit non-ZDR mutation should forward to Capability Core");
        // The authenticated MCP listing performs one durable catalog read;
        // the non-ZDR capability mutation is the second upstream call.
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let _ =
            proxy_to_capability_core(&state, &proxy_claims(true), &bearer, "memory", "GET", None)
                .await
                .expect("ZDR read should preserve normal capability discovery");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        task.abort();
    }

    #[test]
    fn capability_query_values_are_encoded_as_data() {
        assert_eq!(
            capability_query_path("tasks", &[("org_id", "org-a"), ("status", "open")]),
            "tasks?org_id=org-a&status=open",
            "an ordinary value must reach Capability Core unchanged"
        );
        assert_eq!(
            capability_query_path("tasks", &[("org_id", "org-a"), ("status", "")]),
            "tasks?org_id=org-a&status=",
            "no status still means every status"
        );
        assert_eq!(
            capability_query_path(
                "tasks",
                &[
                    ("org_id", "org-a"),
                    ("status", "open&org_id=org-b&limit=100000")
                ]
            ),
            "tasks?org_id=org-a&status=open%26org_id%3Dorg-b%26limit%3D100000"
        );
    }

    /// The caller's `status` used to be spliced into the upstream query raw, so
    /// `&limit=` inside it reached Capability Core as its own parameter.
    #[tokio::test]
    async fn task_list_status_reaches_capability_core_as_one_value() {
        let seen = Arc::new(std::sync::Mutex::new(None::<String>));
        let app = Router::new().fallback(axum::routing::any({
            let seen = Arc::clone(&seen);
            move |uri: axum::http::Uri| {
                let seen = Arc::clone(&seen);
                async move {
                    *seen.lock().expect("query capture lock") = uri.query().map(str::to_owned);
                    Json(json!({ "tasks": [] }))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind capability task list");
        let address = listener.local_addr().expect("capability task list address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve capability task list");
        });
        let mut state = AppState::new();
        state.capability_core_base_url = format!("http://{address}");

        let query = std::collections::HashMap::from([(
            "status".to_owned(),
            "open&org_id=org-b&limit=100000".to_owned(),
        )]);
        let _ = list_tasks_proxy(
            State(state),
            Extension(proxy_claims(false)),
            VerifiedCapabilityBearer::for_test("capability-test-bearer"),
            Query(query),
        )
        .await
        .expect("task listing forwards to Capability Core");
        task.abort();

        let raw = seen
            .lock()
            .expect("query capture lock")
            .clone()
            .expect("Capability Core received a query string");
        let parsed = reqwest::Url::parse(&format!("http://upstream.test/tasks?{raw}"))
            .expect("the forwarded query parses");
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("org_id".to_owned(), "org-a".to_owned()),
                (
                    "status".to_owned(),
                    "open&org_id=org-b&limit=100000".to_owned()
                ),
            ]
        );
    }

    /// The raw query string Capability Core receives for one
    /// `GET /v1/memory` whose caller query is `query`.
    async fn memory_list_upstream_query(
        query: std::collections::HashMap<String, String>,
    ) -> String {
        let seen = Arc::new(std::sync::Mutex::new(None::<String>));
        let app = Router::new().fallback(axum::routing::any({
            let seen = Arc::clone(&seen);
            move |uri: axum::http::Uri| {
                let seen = Arc::clone(&seen);
                async move {
                    *seen.lock().expect("query capture lock") = uri.query().map(str::to_owned);
                    Json(json!({ "memory": [] }))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind capability memory list");
        let address = listener
            .local_addr()
            .expect("capability memory list address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve capability memory list");
        });
        let mut state = AppState::new();
        state.capability_core_base_url = format!("http://{address}");

        let _ = list_memory_proxy(
            State(state),
            Extension(proxy_claims(false)),
            VerifiedCapabilityBearer::for_test("capability-test-bearer"),
            Query(query),
        )
        .await
        .expect("memory listing forwards to Capability Core");
        task.abort();

        let raw = seen.lock().expect("query capture lock").clone();
        raw.expect("Capability Core received a query string")
    }

    /// The caller's `scope` used to be spliced into the upstream query raw, so
    /// `&org_id=` inside it reached Capability Core as its own parameter.
    #[tokio::test]
    async fn memory_list_scope_reaches_capability_core_as_one_value() {
        let raw = memory_list_upstream_query(std::collections::HashMap::from([(
            "scope".to_owned(),
            "user&org_id=org-b&limit=100000".to_owned(),
        )]))
        .await;
        let parsed = reqwest::Url::parse(&format!("http://upstream.test/memory?{raw}"))
            .expect("the forwarded query parses");
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("org_id".to_owned(), "org-a".to_owned()),
                (
                    "scope".to_owned(),
                    "user&org_id=org-b&limit=100000".to_owned()
                ),
            ]
        );

        // Ordinary scopes, and no scope at all, reach Capability Core exactly
        // as the old format! string sent them.
        assert_eq!(
            memory_list_upstream_query(std::collections::HashMap::from([(
                "scope".to_owned(),
                "user".to_owned(),
            )]))
            .await,
            "org_id=org-a&scope=user"
        );
        assert_eq!(
            memory_list_upstream_query(std::collections::HashMap::new()).await,
            "org_id=org-a&scope="
        );
    }

    fn mail_task_bridge_claims(org_id: &str) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "service:verevon-gateway".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: org_id.to_owned(),
            user_id: String::new(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: vec!["mail-task:prepare-thread".to_owned()],
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("service".to_owned()),
            service_id: Some("service:verevon-gateway".to_owned()),
            reason: Some("prepare a mail-task-origin thread".to_owned()),
        }
    }

    fn mail_task_bridge_body(org_id: &str, origin: &str) -> PrepareMailTaskThreadRequestBody {
        PrepareMailTaskThreadRequestBody {
            org_id: org_id.to_owned(),
            user_id: "user-a".to_owned(),
            origin: origin.to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: "mail-task-v1.k.p.s".to_owned(),
        }
    }

    /// The bridge exists for the mail_task origin only. A caller-chosen origin
    /// would reach session-core under the bridge principal's allow-list entry,
    /// which is what lets that principal prepare a thread for any user in the
    /// org -- and only mail_task is gated on a signed Control decision. With
    /// no bridge credential configured a request that got PAST the origin guard
    /// would answer 503, so a 400 here proves the guard fired first.
    #[tokio::test]
    async fn mail_task_bridge_refuses_any_origin_but_mail_task() {
        for origin in ["chat", "agent_run", "support", "system", "space", "", "MAIL_TASK"] {
            let error = prepare_mail_task_thread_for_gateway_bff(
                State(AppState::new()),
                Extension(mail_task_bridge_claims("org-a")),
                Json(mail_task_bridge_body("org-a", origin)),
            )
            .await
            .expect_err("a non-mail_task origin must never reach session-core");
            assert_eq!(error.0, StatusCode::BAD_REQUEST, "origin {origin:?}");
        }
    }

    #[tokio::test]
    async fn mail_task_bridge_refuses_a_request_for_another_orgs_tenant() {
        let error = prepare_mail_task_thread_for_gateway_bff(
            State(AppState::new()),
            Extension(mail_task_bridge_claims("org-attacker")),
            Json(mail_task_bridge_body("org-a", "mail_task")),
        )
        .await
        .expect_err("a credential minted for one org must not prepare a thread in another");
        assert_eq!(error.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn mail_task_bridge_answers_503_until_its_session_core_credential_is_configured() {
        let error = prepare_mail_task_thread_for_gateway_bff(
            State(AppState::new()),
            Extension(mail_task_bridge_claims("org-a")),
            Json(mail_task_bridge_body("org-a", "mail_task")),
        )
        .await
        .expect_err("an unconfigured bridge fails closed");
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn space_deletion_requires_capability_bearer_before_session_core_call() {
        let error = delete_space_threads_for_deletion_coordinator(
            State(AppState::new()),
            Extension(proxy_claims(false)),
            crate::auth::VerifiedSessionBearer::for_test("session-core-bearer"),
            None,
            Json(DeleteSpaceThreadsRequestBody {
                org_id: "org-a".to_owned(),
                space_id: "space-a".to_owned(),
                owner_principal_id: "user-a".to_owned(),
                deletion_request_id: "delete-a".to_owned(),
            }),
        )
        .await
        .expect_err("a Session Core bearer must not substitute for capability-core");
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        assert!(error.1 .0["error"]
            .as_str()
            .unwrap_or_default()
            .contains("capability deletion bearer"));
    }
}

#[cfg(test)]
mod grpc_readiness_tests {
    use super::*;
    use crate::readiness::GrpcReadiness;
    use axum::Extension;

    #[tokio::test]
    async fn readyz_fails_until_grpc_listener_is_bound() {
        let readiness = GrpcReadiness::new();

        let before_bind = readyz(State(AppState::new()), Extension(readiness.clone()))
            .await
            .into_response();
        assert_eq!(before_bind.status(), StatusCode::SERVICE_UNAVAILABLE);

        readiness.mark_bound();
        let after_bind = readyz(State(AppState::new()), Extension(readiness))
            .await
            .into_response();
        assert_eq!(after_bind.status(), StatusCode::OK);
        let body = axum::body::to_bytes(after_bind.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"ok", "the bus off leaves the answer as it always was");
    }

    /// The run-control bus state is told on `/readyz`, and a degraded bus (it behaves as off)
    /// does not make the gateway unready.
    #[tokio::test]
    async fn readyz_reports_the_run_control_bus_and_stays_ready_while_it_is_degraded() {
        use crate::run_control::{LoopbackHub, RunControl};

        let readiness = GrpcReadiness::new();
        readiness.mark_bound();
        let answer = |state: AppState| {
            let readiness = readiness.clone();
            async move {
                let response = readyz(State(state), Extension(readiness))
                    .await
                    .into_response();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap();
                (status, String::from_utf8(body.to_vec()).unwrap())
            }
        };

        let mut on = AppState::new();
        on.run_control =
            RunControl::loopback(LoopbackHub::new(), std::time::Duration::from_millis(200));
        assert_eq!(
            answer(on).await,
            (StatusCode::OK, "ok run_control=ok".to_owned())
        );

        let mut degraded = AppState::new();
        degraded.run_control = RunControl::degraded();
        assert_eq!(
            answer(degraded).await,
            (StatusCode::OK, "ok run_control=degraded".to_owned())
        );
    }

    /// I3: a draining gateway is finishing its streams and must get no new
    /// traffic, so `/readyz` flips to 503 the moment the drain starts, while the
    /// listeners are still bound and `/healthz` (liveness) still answers.
    #[tokio::test]
    async fn readyz_fails_while_draining_even_though_the_listeners_are_bound() {
        let readiness = GrpcReadiness::new();
        readiness.mark_bound();
        let before = readyz(State(AppState::new()), Extension(readiness.clone()))
            .await
            .into_response();
        assert_eq!(before.status(), StatusCode::OK);

        readiness.begin_drain();
        let draining = readyz(State(AppState::new()), Extension(readiness.clone()))
            .await
            .into_response();
        assert_eq!(draining.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(draining.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"draining", "the reason is told, not guessed");

        assert_eq!(healthz().await, "ok", "liveness is not readiness");
        assert!(readiness.is_bound(), "draining does not unbind the listener");
    }
}

#[cfg(test)]
mod run_owner_publish_tests {
    use super::*;
    use crate::{
        auth::{VerifiedExecutionBearer, VerifiedSessionBearer},
        state::DynPublisher,
    };
    use mp_contracts::model_plane::v1::{
        run_service_client::RunServiceClient,
        run_service_server::{RunService, RunServiceServer},
        CancelRunRequest, CancelRunResponse, GetRunRequest, ListRunsRequest, ListRunsResponse,
        ListSystemRunsRequest, ResolveRunActionAuthorityRequest, ResolveRunActionAuthorityResponse,
        ResolveRunOwnerRequest, ResolveRunOwnerResponse, RunDetail,
    };
    use mp_events::publisher::InMemoryPublisher;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Request as TonicRequest, Response as TonicResponse, Status,
    };

    struct OwnerResolver;

    #[tonic::async_trait]
    impl RunService for OwnerResolver {
        // Main's RPCs (merged 2026-11-02); this double does not serve them.
        async fn resolve_approval_authority(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::ResolveApprovalAuthorityRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ResolveApprovalAuthorityResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("resolve_approval_authority is not served by this double"))
        }

        async fn get_run(
            &self,
            _: TonicRequest<GetRunRequest>,
        ) -> Result<TonicResponse<RunDetail>, Status> {
            Err(Status::unimplemented("get_run not needed in test"))
        }

        async fn list_runs(
            &self,
            _: TonicRequest<ListRunsRequest>,
        ) -> Result<TonicResponse<ListRunsResponse>, Status> {
            Err(Status::unimplemented("list_runs not needed in test"))
        }

        async fn list_system_runs(
            &self,
            _: TonicRequest<ListSystemRunsRequest>,
        ) -> Result<TonicResponse<ListRunsResponse>, Status> {
            Err(Status::unimplemented("list_system_runs not needed in test"))
        }

        async fn cancel_run(
            &self,
            request: TonicRequest<CancelRunRequest>,
        ) -> Result<TonicResponse<CancelRunResponse>, Status> {
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
            match request.run_id.as_str() {
                "run-owned" => Ok(TonicResponse::new(CancelRunResponse {
                    cancelled: true,
                    receipt_id: "receipt-cancel-1".to_owned(),
                })),
                // Already finished: Session Core returns the existing receipt.
                "run-finished" => Ok(TonicResponse::new(CancelRunResponse {
                    cancelled: false,
                    receipt_id: "receipt-existing".to_owned(),
                })),
                _ => Err(Status::not_found("run not found")),
            }
        }

        async fn resolve_run_owner(
            &self,
            request: TonicRequest<ResolveRunOwnerRequest>,
        ) -> Result<TonicResponse<ResolveRunOwnerResponse>, Status> {
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
            // `run-vanished` is owned but unknown to `CancelRun`, so a cancel
            // of it exercises the route's gRPC error path.
            Ok(TonicResponse::new(ResolveRunOwnerResponse {
                authorized: matches!(
                    request.run_id.as_str(),
                    "run-owned" | "run-finished" | "run-vanished"
                ) && request.org_id == "org-owner"
                    && request.user_id == "user-owner",
            }))
        }

        async fn resolve_thread_owner(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ResolveThreadOwnerRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::ResolveThreadOwnerResponse>, Status>
        {
            Err(Status::unimplemented(
                "resolve_thread_owner not needed in gateway test",
            ))
        }

        async fn resolve_run_action_authority(
            &self,
            _: TonicRequest<ResolveRunActionAuthorityRequest>,
        ) -> Result<TonicResponse<ResolveRunActionAuthorityResponse>, Status> {
            Err(Status::unimplemented(
                "run action authority not needed in route test",
            ))
        }

        async fn get_scheduled_step_context(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::GetScheduledStepContextRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::ScheduledStepContext>, Status>
        {
            Err(Status::unimplemented(
                "scheduled step context not needed in route test",
            ))
        }

        async fn resolve_scheduled_step_authority(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityRequest>,
        ) -> Result<
            TonicResponse<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityResponse>,
            Status,
        > {
            Err(Status::unimplemented(
                "scheduled step authority not needed in route test",
            ))
        }

        async fn authorize_room_run_attach(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachResponse>, Status> {
            Err(Status::unimplemented("room run attach not needed in this test"))
        }

        async fn cancel_run_as_room_manager(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::CancelRunAsRoomManagerRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::CancelRunAsRoomManagerResponse>, Status> {
            Err(Status::unimplemented("room run stop not needed in this test"))
        }
    }

    async fn owner_resolver_client() -> RunServiceClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind run ownership mock");
        let addr = listener.local_addr().expect("run ownership mock addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(RunServiceServer::new(OwnerResolver))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("run ownership mock endpoint")
            .connect()
            .await
            .expect("connect run ownership mock");
        RunServiceClient::new(channel)
    }

    async fn test_state() -> (AppState, Arc<DynPublisher>) {
        let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
        let mut state = AppState::new();
        state.publisher = publisher.clone();
        state.run_client = owner_resolver_client().await;
        (state, publisher)
    }

    fn descriptor(effort_levels: &[&str]) -> ModelDescriptor {
        ModelDescriptor {
            id: "gpt-5-mini".to_owned(),
            provider: "azure".to_owned(),
            modality: "chat".to_owned(),
            streaming: true,
            features: Vec::new(),
            privacy_tier: "unspecified",
            residency: String::new(),
            effort_levels: effort_levels.iter().map(|level| (*level).to_owned()).collect(),
            context_window_tokens: None,
            max_output_tokens: None,
            lifecycle: String::new(),
            price: None,
            data_use: None,
        }
    }

    #[test]
    fn model_descriptor_lists_effort_levels_only_when_there_are_some() {
        let with = serde_json::to_value(descriptor(&["low", "medium", "high"])).unwrap();
        assert_eq!(with["effort_levels"], serde_json::json!(["low", "medium", "high"]));
        // A model with no effort control serves the JSON it served before.
        let without = serde_json::to_value(descriptor(&[])).unwrap();
        assert!(without.get("effort_levels").is_none(), "{without}");
    }

    #[test]
    fn list_models_descriptor_carries_a_price_only_when_there_is_one() {
        let unpriced = serde_json::to_value(descriptor(&[])).unwrap();
        assert!(unpriced.get("price").is_none(), "{unpriced}");
        let mut priced = descriptor(&[]);
        priced.price = Some(crate::catalogue_prices::ModelPrice {
            input_per_million: 3.0,
            output_per_million: 15.0,
            currency: "USD".to_owned(),
            source: "prefix",
        });
        let json = serde_json::to_value(priced).unwrap();
        assert_eq!(json["price"]["source"], "prefix");
        assert_eq!(json["id"], "gpt-5-mini", "the price sits beside the model's own fields");
    }

    fn claims(org_id: &str, user_id: &str) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: user_id.to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn feedback(run_id: &str) -> FeedbackBody {
        FeedbackBody {
            run_id: Some(run_id.to_owned()),
            request_id: None,
            skill_id: Some("skill-test".to_owned()),
            from_scope: "agent".to_owned(),
            to_scope: "workspace".to_owned(),
            rating: "good".to_owned(),
            note: None,
        }
    }

    /// A thumbs-UP must map to the canonical grade that *raises* a score, and a
    /// thumbs-DOWN to one that does not. This is the subtlest of the four
    /// feedback-wire breaks: `positive` used to reach orchestrator-core verbatim,
    /// increment the sample total, match no `good` branch, and so make a
    /// thumbs-up *lower* the skill's promotion score.
    #[test]
    fn thumbs_up_never_maps_to_a_non_good_grade() {
        assert_eq!(
            FeedbackRating::from_wire("positive"),
            Some(FeedbackRating::Good)
        );
        assert_eq!(
            FeedbackRating::from_wire("positive").map(|r| r.as_str()),
            Some("good")
        );
        assert_ne!(
            FeedbackRating::from_wire("positive"),
            Some(FeedbackRating::Poor)
        );
        assert_ne!(
            FeedbackRating::from_wire("positive"),
            Some(FeedbackRating::Acceptable)
        );
        assert_eq!(
            FeedbackRating::from_wire("negative"),
            Some(FeedbackRating::Poor)
        );
        assert_ne!(
            FeedbackRating::from_wire("negative"),
            Some(FeedbackRating::Good)
        );
    }

    /// Pins the vocabulary map in both directions: every accepted wire token
    /// grades as expected, and every canonical grade is itself an accepted wire
    /// token that round-trips to the same variant.
    #[test]
    fn rating_vocabulary_maps_in_both_directions() {
        for (wire, expected) in [
            ("positive", FeedbackRating::Good),
            ("up", FeedbackRating::Good),
            ("good", FeedbackRating::Good),
            ("GOOD", FeedbackRating::Good),
            ("  Positive  ", FeedbackRating::Good),
            ("negative", FeedbackRating::Poor),
            ("down", FeedbackRating::Poor),
            ("poor", FeedbackRating::Poor),
            ("acceptable", FeedbackRating::Acceptable),
            ("neutral", FeedbackRating::Acceptable),
        ] {
            assert_eq!(
                FeedbackRating::from_wire(wire),
                Some(expected),
                "wire token {wire:?} graded wrong"
            );
        }
        for grade in [
            FeedbackRating::Good,
            FeedbackRating::Acceptable,
            FeedbackRating::Poor,
        ] {
            assert_eq!(
                FeedbackRating::from_wire(grade.as_str()),
                Some(grade),
                "canonical grade {:?} must round-trip",
                grade.as_str()
            );
        }
        // Unknown tokens are refused, never coerced into a grade.
        for unknown in ["", "  ", "great", "1", "thumbs_up", "bad"] {
            assert_eq!(
                FeedbackRating::from_wire(unknown),
                None,
                "unknown token {unknown:?} must not be graded"
            );
        }
    }

    fn chat_feedback(request_id: &str, rating: &str) -> FeedbackBody {
        FeedbackBody {
            request_id: Some(request_id.to_owned()),
            rating: rating.to_owned(),
            ..FeedbackBody::default()
        }
    }

    /// The chat wire shape (`{requestId, rating, note}`) must deserialize — it
    /// used to 422 on the missing `run_id` / `skill_id` before any logic ran.
    #[test]
    fn chat_wire_body_deserializes_without_run_id_or_skill_id() {
        let body: FeedbackBody = serde_json::from_str(
            r#"{"requestId":"req-1","rating":"positive","note":"nyttig svar"}"#,
        )
        .expect("the chat client's body must be accepted");
        assert_eq!(body.request_id.as_deref(), Some("req-1"));
        assert_eq!(body.run_id, None);
        assert_eq!(body.skill_id, None);
        assert_eq!(body.note.as_deref(), Some("nyttig svar"));
        // Scope defaults still apply so downstream keys stay stable.
        assert_eq!(body.from_scope, "agent");
        assert_eq!(body.to_scope, "workspace");
    }

    #[test]
    fn chat_turn_resolves_to_its_durable_run_and_injected_skills() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        registry.record(
            "req-1",
            "org-owner",
            "user-owner",
            "run-owned",
            vec!["skill.a".to_owned(), "skill.b".to_owned()],
        );
        let resolved = resolve_feedback_target(
            &registry,
            &claims("org-owner", "user-owner"),
            &chat_feedback("req-1", "negative"),
        )
        .expect("the turn's owner may rate it");
        assert_eq!(resolved.run_id, "run-owned");
        assert_eq!(resolved.skill_ids, vec!["skill.a", "skill.b"]);
    }

    #[test]
    fn client_supplied_skill_ids_never_override_what_the_gateway_injected() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        registry.record(
            "req-1",
            "org-owner",
            "user-owner",
            "run-owned",
            vec!["skill.actually-injected".to_owned()],
        );
        let mut body = chat_feedback("req-1", "negative");
        body.skill_id = Some("skill.someone-elses".to_owned());
        let resolved =
            resolve_feedback_target(&registry, &claims("org-owner", "user-owner"), &body)
                .expect("resolves");
        assert_eq!(
            resolved.skill_ids,
            vec!["skill.actually-injected"],
            "a client must not be able to aim a rating at a skill it names"
        );
    }

    #[test]
    fn another_users_chat_turn_is_refused_before_the_owner_lookup() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        registry.record("req-1", "org-owner", "user-owner", "run-owned", Vec::new());
        let foreign_user = resolve_feedback_target(
            &registry,
            &claims("org-owner", "user-other"),
            &chat_feedback("req-1", "positive"),
        )
        .expect_err("another user in the org must not rate this turn");
        assert_eq!(foreign_user.0, StatusCode::FORBIDDEN);
        let foreign_org = resolve_feedback_target(
            &registry,
            &claims("org-other", "user-owner"),
            &chat_feedback("req-1", "positive"),
        )
        .expect_err("another tenant must not rate this turn");
        assert_eq!(foreign_org.0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn unresolvable_chat_turn_fails_closed_instead_of_trusting_the_client() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        let error = resolve_feedback_target(
            &registry,
            &claims("org-owner", "user-owner"),
            &chat_feedback("req-evicted", "positive"),
        )
        .expect_err("an unknown turn cannot be authorized");
        assert_eq!(error.0, StatusCode::CONFLICT);
    }

    #[test]
    fn missing_run_reference_is_a_client_error_not_a_silent_accept() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        let mut body = FeedbackBody::default();
        body.rating = "positive".to_owned();
        let error = resolve_feedback_target(&registry, &claims("org-owner", "user-owner"), &body)
            .expect_err("neither requestId nor run_id supplied");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
    }

    /// A gateway restart loses the turn map. When the caller still supplies a
    /// durable run id the rating must degrade to run-only, not error.
    #[test]
    fn lost_turn_record_degrades_to_a_run_only_rating() {
        let registry = crate::chat_turn_registry::ChatTurnRegistry::new();
        let mut body = chat_feedback("req-evicted", "negative");
        body.run_id = Some("run-owned".to_owned());
        let resolved =
            resolve_feedback_target(&registry, &claims("org-owner", "user-owner"), &body)
                .expect("a durable run id still authorizes a run-only rating");
        assert_eq!(resolved.run_id, "run-owned");
        assert!(
            resolved.skill_ids.is_empty(),
            "no skill attribution survives the restart, and that is not an error"
        );
    }

    /// A turn that streamed on replica A is rateable on replica B once B adopts the
    /// caller's shared record: it resolves to the run and the injected skills
    /// instead of `turn_not_rateable`.
    #[tokio::test]
    async fn a_turn_from_another_replica_resolves_through_the_shared_record() {
        let shared = crate::chat_turn_registry::fake::FakeSharedStore::default();
        let streamed_on_a = crate::chat_turn_registry::ChatTurnRegistry::new();
        streamed_on_a.record(
            "req-1",
            "org-owner",
            "user-owner",
            "run-owned",
            vec!["skill.a".to_owned()],
        );
        let turn = streamed_on_a.lookup("req-1").expect("recorded on A");
        crate::chat_turn_registry::mirror_turn(&shared, "req-1", &turn).await;

        let replica_b = crate::chat_turn_registry::ChatTurnRegistry::new();
        let body = chat_feedback("req-1", "negative");
        let owner = claims("org-owner", "user-owner");
        let before = resolve_feedback_target(&replica_b, &owner, &body)
            .expect_err("B has never seen the turn");
        assert_eq!(before.0, StatusCode::CONFLICT);

        replica_b
            .adopt_shared(&shared, &owner.org_id, &owner.user_id, "req-1")
            .await;
        let resolved = resolve_feedback_target(&replica_b, &owner, &body)
            .expect("the shared record makes the turn rateable on B");
        assert_eq!(resolved.run_id, "run-owned");
        assert_eq!(resolved.skill_ids, vec!["skill.a"]);
    }

    /// The shared key is the access control: a rating from another user or tenant
    /// finds nothing to adopt and is refused as an unknown turn.
    #[tokio::test]
    async fn another_callers_rating_cannot_resolve_the_shared_record() {
        let shared = crate::chat_turn_registry::fake::FakeSharedStore::default();
        let turn = crate::chat_turn_registry::ChatTurnRecord {
            org_id: "org-owner".to_owned(),
            user_id: "user-owner".to_owned(),
            run_id: "run-owned".to_owned(),
            skill_ids: Vec::new(),
        };
        crate::chat_turn_registry::mirror_turn(&shared, "req-1", &turn).await;
        let intruder = claims("org-owner", "user-other");
        let replica_b = crate::chat_turn_registry::ChatTurnRegistry::new();
        replica_b
            .adopt_shared(&shared, &intruder.org_id, &intruder.user_id, "req-1")
            .await;
        let refused = resolve_feedback_target(
            &replica_b,
            &intruder,
            &chat_feedback("req-1", "positive"),
        )
        .expect_err("another user must not resolve the owner's turn");
        assert_eq!(refused.0, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn thumbs_up_from_chat_publishes_the_canonical_good_grade() {
        let (state, publisher) = test_state().await;
        crate::chat_turn_registry::global().record(
            "req-canonical-good",
            "org-owner",
            "user-owner",
            "run-owned",
            vec!["skill.injected".to_owned()],
        );
        let mut body = chat_feedback("req-canonical-good", "positive");
        body.note = Some("bra".to_owned());
        // The response body is not the assertion here -- what matters is the
        // envelope that actually reaches the promotion scorer, checked below.
        let _accepted = ingest_feedback(
            State(state),
            Extension(claims("org-owner", "user-owner")),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Json(body),
        )
        .await
        .expect("the turn owner may rate it");

        let events = publisher.drain();
        assert_eq!(events.len(), 1);
        let payload = &events[0].1.payload;
        assert_eq!(payload["rating"], "good", "a thumbs-up must publish `good`");
        assert_eq!(payload["run_id"], "run-owned");
        assert_eq!(payload["skill_id"], "skill.injected");
        assert_eq!(payload["skill_ids"][0], "skill.injected");
        assert_eq!(payload["note"], "bra");
    }

    #[tokio::test]
    async fn unknown_rating_is_rejected_before_the_owner_lookup_or_publish() {
        let (state, publisher) = test_state().await;
        let error = ingest_feedback(
            State(state),
            Extension(claims("org-owner", "user-owner")),
            // A bad downstream credential proves the vocabulary check runs first.
            VerifiedSessionBearer::for_test("must-not-be-used"),
            Json(chat_feedback("req-1", "wonderful")),
        )
        .await
        .expect_err("an ungradeable rating must be refused");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(
            publisher.drain().is_empty(),
            "an ungradeable rating published an event"
        );
    }

    #[tokio::test]
    #[serial_test::serial(run_cancel_log)]
    async fn foreign_run_ids_are_rejected_before_http_run_event_publishes() {
        let (state, publisher) = test_state().await;
        let foreign_user = claims("org-owner", "user-other");
        let foreign_org = claims("org-other", "user-other");

        let cancel = cancel_run(
            State(state.clone()),
            Extension(foreign_user.clone()),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Path("run-owned".to_owned()),
        )
        .await;
        let feedback = ingest_feedback(
            State(state.clone()),
            Extension(foreign_user),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Json(feedback("run-owned")),
        )
        .await;
        let resume = resume_run(
            State(state),
            Extension(foreign_org),
            VerifiedExecutionBearer::for_test("test-execution-bearer"),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Path("run-owned".to_owned()),
        )
        .await;

        let statuses = [cancel, feedback, resume].map(|result| {
            result
                .as_ref()
                .err()
                .map(|error| error.0)
                .expect("foreign run must be rejected")
        });
        assert_eq!(statuses, [StatusCode::FORBIDDEN; 3]);
        assert!(
            publisher.drain().is_empty(),
            "foreign run IDs must not publish cancellation, resume, or feedback events"
        );
    }

    #[tokio::test]
    async fn issuer_zdr_feedback_is_rejected_before_owner_lookup_or_publish() {
        let (state, publisher) = test_state().await;
        let mut owner = claims("org-owner", "user-owner");
        owner.zdr = true;

        let result = ingest_feedback(
            State(state),
            Extension(owner),
            // A bad downstream credential proves the ZDR decision happens
            // before the owner lookup. The publisher proves no event escaped.
            VerifiedSessionBearer::for_test("must-not-be-used"),
            Json(feedback("run-owned")),
        )
        .await;

        let error = result.expect_err("issuer-ZDR feedback must be rejected");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        assert!(
            publisher.drain().is_empty(),
            "ZDR feedback published an event"
        );
    }

    #[tokio::test]
    #[serial_test::serial(run_cancel_log)]
    async fn durable_http_run_owner_can_publish_cancel_and_feedback_events() {
        let (state, publisher) = test_state().await;
        let owner = claims("org-owner", "user-owner");

        let _ = cancel_run(
            State(state.clone()),
            Extension(owner.clone()),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Path("run-owned".to_owned()),
        )
        .await
        .expect("durable owner may request cancellation");
        let _ = ingest_feedback(
            State(state),
            Extension(owner),
            VerifiedSessionBearer::for_test("test-session-bearer"),
            Json(feedback("run-owned")),
        )
        .await
        .expect("durable owner may publish feedback");

        let events = publisher.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1.event_type, "RUN_CANCEL_REQUESTED");
        assert_eq!(events[1].1.event_type, "FEEDBACK_RATED");
        assert!(events.iter().all(|(subject, _)| {
            subject == mp_events::subjects::SUBJECT_RUN || subject == "mp.v1.feedback.rated"
        }));
    }

    /// Q15: a durable cancel that won the race against its own stream left the
    /// stream to save the answer onto the cancelled run and then fail to
    /// terminalize it. A successful cancel now also stops the owner's stream
    /// for that run in this process — and nobody else's.
    #[tokio::test]
    #[serial_test::serial(run_cancel_log)]
    async fn a_durable_cancel_stops_the_owners_live_stream_for_the_run() {
        use std::sync::atomic::Ordering;
        let (state, _publisher) = test_state().await;
        let register = |request: &str, user: &str, run: &str| {
            state.cancels.register_run(request, "org-owner", user, run)
        };
        let stream = register("req-owned", "user-owner", "run-owned");
        let colleague = register("req-colleague", "user-other", "run-owned");
        let finished = register("req-finished", "user-owner", "run-finished");

        for run_id in ["run-finished", "run-owned"] {
            let _ = cancel_run(
                State(state.clone()),
                Extension(claims("org-owner", "user-owner")),
                VerifiedSessionBearer::for_test("test-session-bearer"),
                Path(run_id.to_owned()),
            )
            .await
            .expect("the owner may cancel");
        }

        assert!(stream.load(Ordering::Relaxed), "the stream stops");
        assert!(
            !colleague.load(Ordering::Relaxed),
            "another user's stream is never reached"
        );
        assert!(
            !finished.load(Ordering::Relaxed),
            "an already-terminal run was not cancelled by this call"
        );
    }

    /// A durable cancel on a replica that does not own the run's stream still stops it, over the
    /// run-control bus, and says so in its log line (`via`). The run is cancelled whatever the
    /// bus answers: an owner that does not answer is logged, never an error.
    #[tokio::test]
    #[serial_test::serial(run_cancel_log)]
    async fn a_durable_cancel_reaches_a_stream_on_another_replica_over_the_bus() {
        use crate::run_control::{LoopbackHub, RunControl};
        use std::sync::atomic::Ordering;
        let hub = LoopbackHub::new();
        let (mut owner, _publisher) = test_state().await;
        let (mut other, _publisher) = test_state().await;
        for state in [&mut owner, &mut other] {
            state.run_control = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
            state.cancels = crate::cancel_registry::CancelRegistry::new();
        }
        let flag = owner
            .cancels
            .register_run("req-owned", "org-owner", "user-owner", "run-owned");
        owner
            .run_control
            .attach(&owner.cancels, &owner.queued_inputs, "req-owned", "run-owned")
            .await;

        let lines = CancelLines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());
        let cancel = |state: AppState| async move {
            cancel_run(
                State(state),
                Extension(claims("org-owner", "user-owner")),
                VerifiedSessionBearer::for_test("test-session-bearer"),
                Path("run-owned".to_owned()),
            )
            .await
            .expect("the owner may cancel")
        };

        // The owner hangs: the durable cancel still answers, and the log says what happened.
        hub.mute(true);
        let _ = cancel(other.clone()).await;
        assert!(!flag.load(Ordering::Relaxed));
        hub.mute(false);
        let _ = cancel(other.clone()).await;
        assert!(flag.load(Ordering::Relaxed), "the stream on the other replica stops");
        let _ = cancel(owner.clone()).await;

        let via: Vec<String> = lines
            .0
            .lock()
            .expect("log capture lock")
            .iter()
            .map(|line| {
                line.iter()
                    .find(|(key, _)| key == "via")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(via, ["unavailable", "peer", "local"]);
    }

    // ---- durable send receipts (WS04 S6): `queue_input_with_receipts` ----

    /// What the fake transcript was asked to store: `(thread, conversation_only, client_message_id)`,
    /// and how many messages sat in the stream's queue at that moment (the proof of the order).
    #[derive(Clone, Default)]
    struct FakeTranscript {
        rows: Arc<std::sync::Mutex<Vec<(String, bool, String)>>>,
        queue_depth_at_store: Arc<std::sync::Mutex<Vec<usize>>>,
        /// Answered instead of storing.
        fail_with: Option<tonic::Code>,
        /// A thread this transcript holds as conversation-only: a store that does not say so is
        /// refused, as session-core refuses it.
        conversation_only: bool,
        /// Run just after the store, to stand in for the run ending in the middle of a send.
        after_store: Option<Arc<dyn Fn() + Send + Sync>>,
        /// Leave the queue alone at the store (by default the store takes what waits, to show
        /// what was queued before it).
        leave_queue: bool,
    }

    impl FakeTranscript {
        fn persist(
            &self,
            queued: &crate::queued_input::QueuedInputRegistry,
            request_id: &str,
        ) -> impl Fn(
            String,
            bool,
            String,
        ) -> std::future::Ready<Result<crate::session_flow::AppendReceipt, tonic::Status>> {
            let (this, queued, request_id) = (self.clone(), queued.clone(), request_id.to_owned());
            move |thread: String, conversation_only: bool, id: String| {
                // Taking everything leaves the queue empty, so the later queue is what puts the
                // message there: a queue-first order would see it here.
                let depth = if this.leave_queue { 0 } else { queued.drain(&request_id).len() };
                this.queue_depth_at_store.lock().unwrap().push(depth);
                if let Some(code) = this.fail_with {
                    return std::future::ready(Err(tonic::Status::new(code, "scripted")));
                }
                if this.conversation_only && !conversation_only {
                    return std::future::ready(Err(tonic::Status::failed_precondition(
                        "This conversation requires conversation-only source scope.",
                    )));
                }
                let mut rows = this.rows.lock().unwrap();
                let held = rows.iter().position(|(_, _, held)| *held == id);
                let deduplicated = held.is_some();
                let index = held.unwrap_or_else(|| {
                    rows.push((thread, conversation_only, id));
                    rows.len() - 1
                });
                drop(rows);
                if let Some(after) = &this.after_store {
                    after();
                }
                std::future::ready(Ok(crate::session_flow::AppendReceipt {
                    message_id: format!("message-{index}"),
                    deduplicated,
                }))
            }
        }

        fn rows(&self) -> Vec<(String, bool, String)> {
            self.rows.lock().unwrap().clone()
        }
    }

    fn queue_body(content: &str, id: Option<&str>) -> QueueInputRequest {
        QueueInputRequest {
            content: content.to_owned(),
            space_append_context: None,
            thread_id: None,
            client_message_id: id.map(str::to_owned),
        }
    }

    async fn send_queued(
        state: &AppState,
        who: &Claims,
        transcript: &FakeTranscript,
        body: &QueueInputRequest,
    ) -> (StatusCode, serde_json::Value) {
        let response = queue_input_with_receipts(
            state,
            who,
            "req-1",
            body,
            transcript.persist(&state.queued_inputs, "req-1"),
        )
        .await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).expect("a JSON answer"))
    }

    /// A state whose stream `req-1` of `org-a/user-a` is running on this replica.
    async fn streaming(conversation_only: bool) -> AppState {
        let (state, _publisher) = test_state().await;
        state.queued_inputs.register(
            "req-1",
            "org-a",
            "user-a",
            "thread-1",
            conversation_only,
            crate::normalize::RetrievalScope::Auto,
        );
        state
    }

    #[tokio::test]
    async fn a_message_is_stored_under_its_id_before_it_is_queued_and_the_answer_is_the_receipt() {
        let state = streaming(false).await;
        let transcript = FakeTranscript::default();
        let (status, answer) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body(" use EUR ", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(answer["queued"], true);
        assert_eq!(answer["persisted"], true);
        assert_eq!(answer["client_message_id"], "msg-1");
        assert_eq!(answer["message_id"], "message-0");
        assert_eq!(answer["deduplicated"], false);
        assert_eq!(answer["pending"], 1);
        assert_eq!(transcript.rows(), [("thread-1".to_owned(), false, "msg-1".to_owned())]);
        assert_eq!(
            *transcript.queue_depth_at_store.lock().unwrap(),
            [0],
            "the message was not in the queue yet when it was stored"
        );
        let delivered = state.queued_inputs.drain_receipted("req-1");
        assert_eq!(
            delivered,
            [crate::queued_input::QueuedMessage { text: "use EUR".into(), client_message_id: "msg-1".into() }],
            "the loop takes the message with its id"
        );
    }

    /// The plan's retry case: the reply was lost, the client asks again with the same id.
    #[tokio::test]
    async fn a_repeat_after_a_lost_reply_is_stored_once_and_queued_once() {
        let state = streaming(false).await;
        let transcript = FakeTranscript { leave_queue: true, ..FakeTranscript::default() };
        let who = claims("org-a", "user-a");
        let body = queue_body("use EUR", Some("msg-1"));
        let first = send_queued(&state, &who, &transcript, &body).await;
        let again = send_queued(&state, &who, &transcript, &body).await;

        assert_eq!(first.0, StatusCode::ACCEPTED);
        assert_eq!(again.0, StatusCode::ACCEPTED);
        assert_eq!(again.1["deduplicated"], true);
        assert_eq!(again.1["message_id"], first.1["message_id"]);
        assert_eq!(transcript.rows().len(), 1, "one transcript row");
        assert_eq!(
            state.queued_inputs.drain_receipted("req-1").len(),
            1,
            "the repeat queued nothing a second time"
        );
    }

    #[tokio::test]
    async fn an_id_that_names_a_different_message_is_a_conflict_and_nothing_is_delivered() {
        let state = streaming(false).await;
        let transcript = FakeTranscript { fail_with: Some(tonic::Code::AlreadyExists), ..FakeTranscript::default() };
        let (status, answer) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body("hi", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(answer["error"], "client_message_id_conflict");
        assert_eq!((answer["queued"].clone(), answer["persisted"].clone()), (false.into(), false.into()));
        assert!(state.queued_inputs.drain("req-1").is_empty());
    }

    /// A message that cannot be recorded is not delivered: the model never reads a message the
    /// thread does not hold.
    #[tokio::test]
    async fn a_message_that_cannot_be_recorded_is_not_delivered() {
        let state = streaming(false).await;
        let who = claims("org-a", "user-a");
        for (code, expected, retryable) in [
            (tonic::Code::Unavailable, StatusCode::SERVICE_UNAVAILABLE, true),
            (tonic::Code::Internal, StatusCode::SERVICE_UNAVAILABLE, true),
            (tonic::Code::PermissionDenied, StatusCode::FORBIDDEN, false),
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST, false),
        ] {
            let transcript = FakeTranscript { fail_with: Some(code), ..FakeTranscript::default() };
            let (status, answer) =
                send_queued(&state, &who, &transcript, &queue_body("hi", Some("msg-1"))).await;
            assert_eq!(status, expected, "{code:?}");
            assert_eq!(answer["retryable"], retryable, "{code:?}");
            assert_eq!(answer["persisted"], false);
            assert!(state.queued_inputs.drain("req-1").is_empty(), "{code:?}");
        }
    }

    #[tokio::test]
    async fn no_live_stream_is_a_404_that_stores_nothing_and_names_the_resend() {
        let (state, _publisher) = test_state().await;
        let transcript = FakeTranscript::default();
        let (status, answer) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body("hi", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(answer["resend_as_new_turn"], true);
        assert_eq!(answer["persisted"], false);
        assert_eq!(answer["client_message_id"], "msg-1", "the resend carries the same id");
        assert!(transcript.rows().is_empty());
    }

    /// Somebody else's stream answers exactly as a missing one does, and stores nothing.
    #[tokio::test]
    async fn a_strangers_message_is_a_404_and_stores_nothing() {
        let state = streaming(false).await;
        for (org, user) in [("org-b", "user-a"), ("org-a", "user-b")] {
            let transcript = FakeTranscript::default();
            let (status, answer) =
                send_queued(&state, &claims(org, user), &transcript, &queue_body("hi", Some("msg-1"))).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{org}/{user}");
            assert_eq!(answer["persisted"], false);
            assert!(transcript.rows().is_empty(), "{org}/{user}");
        }
        assert!(state.queued_inputs.drain("req-1").is_empty());
    }

    /// The run ends between the store and the queue: the message is in the thread, the answer says
    /// so, and the resend (same id) finds its row instead of writing a second.
    #[tokio::test]
    async fn a_run_that_ends_between_the_store_and_the_queue_leaves_one_row_and_names_the_resend() {
        let state = streaming(false).await;
        let ending = state.queued_inputs.clone();
        let transcript = FakeTranscript {
            after_store: Some(Arc::new(move || {
                ending.finish("req-1");
            })),
            ..FakeTranscript::default()
        };
        let (status, answer) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body("hi", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(answer["resend_as_new_turn"], true);
        assert_eq!(answer["persisted"], true, "the message is in the thread");
        assert_eq!(answer["message_id"], "message-0");
        // The resend: the same id, now an ordinary send.
        let resend = transcript.persist(&state.queued_inputs, "req-1")("thread-1".into(), false, "msg-1".into()).await;
        assert!(resend.expect("stored").deduplicated);
        assert_eq!(transcript.rows().len(), 1, "exactly one transcript row");
    }

    #[tokio::test]
    async fn a_message_refused_for_its_shape_or_its_id_writes_nothing() {
        let state = streaming(false).await;
        let who = claims("org-a", "user-a");
        let long = "x".repeat(crate::queued_input::MAX_QUEUED_CHARS + 1);
        for (body, expected) in [
            (queue_body("   ", Some("msg-1")), StatusCode::BAD_REQUEST),
            (queue_body(&long, Some("msg-1")), StatusCode::PAYLOAD_TOO_LARGE),
            (queue_body("hi", Some("not a valid id")), StatusCode::BAD_REQUEST),
        ] {
            let transcript = FakeTranscript::default();
            let (status, _) = send_queued(&state, &who, &transcript, &body).await;
            assert_eq!(status, expected);
            assert!(transcript.rows().is_empty());
        }
        assert!(state.queued_inputs.drain("req-1").is_empty());
    }

    #[tokio::test]
    async fn a_thread_the_stream_is_not_on_is_refused_before_anything_is_stored() {
        let state = streaming(false).await;
        let transcript = FakeTranscript::default();
        let mut body = queue_body("hi", Some("msg-1"));
        body.thread_id = Some("thread-9".to_owned());
        let (status, _) = send_queued(&state, &claims("org-a", "user-a"), &transcript, &body).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(transcript.rows().is_empty());
        assert!(state.queued_inputs.drain("req-1").is_empty());
    }

    /// A client from before receipts sends no id: the gateway names the message, and the answer
    /// carries the id either way.
    #[tokio::test]
    async fn a_message_sent_without_an_id_is_named_by_the_gateway() {
        let state = streaming(false).await;
        let transcript = FakeTranscript::default();
        let (status, answer) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body("hi", None)).await;

        assert_eq!(status, StatusCode::ACCEPTED);
        let id = answer["client_message_id"].as_str().expect("an id");
        assert!(crate::queued_input::valid_client_message_id(id), "{id}");
        assert_eq!(transcript.rows()[0].2, id);
    }

    /// ZDR keeps nothing: neither a store nor a bus message is ever built for it.
    #[tokio::test]
    async fn a_zdr_message_is_refused_and_stores_and_queues_nothing() {
        let state = streaming(false).await;
        let mut zdr = claims("org-a", "user-a");
        zdr.zdr = true;
        let transcript = FakeTranscript::default();
        let (status, _) = send_queued(&state, &zdr, &transcript, &queue_body("secret", Some("msg-1"))).await;

        assert!(status.is_client_error(), "{status}");
        assert!(transcript.rows().is_empty());
        assert!(transcript.queue_depth_at_store.lock().unwrap().is_empty(), "the store was never reached");
        assert!(state.queued_inputs.drain("req-1").is_empty());
    }

    /// A conversation-only stream on this replica stores its message under that scope, which the
    /// thread requires.
    #[tokio::test]
    async fn a_conversation_only_stream_stores_under_its_own_scope() {
        let state = streaming(true).await;
        let transcript = FakeTranscript { conversation_only: true, ..FakeTranscript::default() };
        let (status, _) =
            send_queued(&state, &claims("org-a", "user-a"), &transcript, &queue_body("hi", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(transcript.rows(), [("thread-1".to_owned(), true, "msg-1".to_owned())]);
    }

    /// Two replicas over one bus: the message is stored from the replica the user reached, with the
    /// thread the owner reported, and queued on the owner, once, with its id.
    #[tokio::test]
    async fn a_message_sent_to_another_replica_is_stored_there_and_queued_on_the_owner() {
        use crate::run_control::{LoopbackHub, RunControl};
        let hub = LoopbackHub::new();
        let owner = streaming(true).await;
        let (mut peer, _publisher) = test_state().await;
        let mut owner = owner;
        for state in [&mut owner, &mut peer] {
            state.run_control = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
        }
        let _flag = owner.cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(owner.run_control.attach(&owner.cancels, &owner.queued_inputs, "req-1", "run-1").await);

        // The owner's thread is conversation-only, and its probe answer says so: the one store is
        // already under that scope, so nothing is refused and retried.
        let transcript = FakeTranscript { conversation_only: true, ..FakeTranscript::default() };
        let who = claims("org-a", "user-a");
        let (status, answer) = send_queued(&peer, &who, &transcript, &queue_body("use EUR", Some("msg-1"))).await;

        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(answer["persisted"], true);
        assert_eq!(transcript.rows(), [("thread-1".to_owned(), true, "msg-1".to_owned())]);
        assert!(peer.queued_inputs.drain("req-1").is_empty(), "nothing is queued on the replica that was reached");
        assert_eq!(
            owner.queued_inputs.drain_receipted("req-1"),
            [crate::queued_input::QueuedMessage { text: "use EUR".into(), client_message_id: "msg-1".into() }]
        );

        // The retry after a lost reply is the same id: stored once, queued once.
        let again = send_queued(&peer, &who, &transcript, &queue_body("use EUR", Some("msg-1"))).await;
        assert_eq!(again.1["deduplicated"], true);
        assert_eq!(transcript.rows().len(), 1);
        assert_eq!(
            owner.queued_inputs.drain_receipted("req-1").len(),
            0,
            "the owner remembers the id it already took: the retry is answered and queues nothing"
        );

        // An owner that does not answer: nothing is stored or claimed.
        hub.mute(true);
        let silent = FakeTranscript::default();
        let (status, answer) = send_queued(&peer, &who, &silent, &queue_body("hi", Some("msg-2"))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(answer["error"], "run_control_unavailable");
        assert_eq!(answer["persisted"], false);
        assert!(silent.rows().is_empty());
    }

    /// A message sent to another replica for a conversation-only stream is stored once, under the scope
    /// the owner states in its probe answer: the first store is not guessed, refused and retried.
    #[tokio::test]
    async fn a_message_to_a_peer_stream_is_stored_once_under_the_scope_the_owner_states() {
        use crate::run_control::{LoopbackHub, RunControl};
        let hub = LoopbackHub::new();
        let mut owner = streaming(true).await;
        let (mut peer, _publisher) = test_state().await;
        for state in [&mut owner, &mut peer] {
            state.run_control = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
        }
        let _flag = owner.cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(owner.run_control.attach(&owner.cancels, &owner.queued_inputs, "req-1", "run-1").await);

        // Each store records the scope it was asked for, and a conversation-only thread refuses any
        // other, so a guessed first attempt would show up as a second entry here.
        let asked = Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let persist = {
            let asked = asked.clone();
            move |_thread: String, conversation_only: bool, _id: String| {
                asked.lock().unwrap().push(conversation_only);
                let stored = if conversation_only {
                    Ok(crate::session_flow::AppendReceipt { message_id: "message-0".to_owned(), deduplicated: false })
                } else {
                    Err(tonic::Status::failed_precondition("conversation-only source scope required"))
                };
                std::future::ready(stored)
            }
        };
        let response = queue_input_with_receipts(
            &peer,
            &claims("org-a", "user-a"),
            "req-1",
            &queue_body("use EUR", Some("msg-1")),
            &persist,
        )
        .await;

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(*asked.lock().unwrap(), [true], "one store, under the scope the owner stated");
        assert_eq!(owner.queued_inputs.drain_receipted("req-1").len(), 1, "and queued once");
    }

    /// An owner that predates the scope field answers a probe with the thread alone: the message is
    /// first tried as an ordinary one, and retried as conversation-only on the one refusal a wrong
    /// guess can get, as it was before the field existed.
    #[tokio::test]
    async fn an_owner_that_does_not_state_its_scope_keeps_the_guess_and_retry() {
        use crate::run_control::{LoopbackHub, RunControl, RunControlOp, RunControlReply, RunControlSubject};
        let hub = LoopbackHub::new();
        let (mut peer, _publisher) = test_state().await;
        peer.run_control = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
        let old_owner = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
        let _listener = old_owner
            .listen(&[RunControlSubject::request_any_op("req-1").expect("subject")], |msg| match msg.op {
                RunControlOp::Probe => RunControlReply {
                    thread_id: Some("thread-1".to_owned()),
                    ..RunControlReply::found()
                },
                _ => RunControlReply::not_found(),
            })
            .await
            .expect("listen");

        let asked = Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let persist = {
            let asked = asked.clone();
            move |_thread: String, conversation_only: bool, _id: String| {
                asked.lock().unwrap().push(conversation_only);
                let stored = if conversation_only {
                    Ok(crate::session_flow::AppendReceipt { message_id: "message-0".to_owned(), deduplicated: false })
                } else {
                    Err(tonic::Status::failed_precondition("conversation-only source scope required"))
                };
                std::future::ready(stored)
            }
        };
        // The old owner does not queue the message, so the answer is not checked: the stores are.
        let _ = queue_input_with_receipts(
            &peer,
            &claims("org-a", "user-a"),
            "req-1",
            &queue_body("use EUR", Some("msg-1")),
            &persist,
        )
        .await;

        assert_eq!(*asked.lock().unwrap(), [false, true], "guessed as ordinary, then retried");
    }

    /// One id, one queued delivery, even when the retry comes while the first copy still waits.
    #[tokio::test]
    async fn a_retry_while_the_first_copy_still_waits_queues_once() {
        use crate::run_control::{LoopbackHub, RunControl};
        let hub = LoopbackHub::new();
        let mut owner = streaming(false).await;
        let (mut peer, _publisher) = test_state().await;
        for state in [&mut owner, &mut peer] {
            state.run_control = RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200));
        }
        let _flag = owner.cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(owner.run_control.attach(&owner.cancels, &owner.queued_inputs, "req-1", "run-1").await);

        // A transcript that leaves the queue alone, so the first copy is still waiting.
        let rows = Arc::new(std::sync::Mutex::new(Vec::new()));
        let who = claims("org-a", "user-a");
        let body = queue_body("use EUR", Some("msg-1"));
        let persist = {
            let rows = rows.clone();
            move |thread: String, _scope: bool, id: String| {
                let mut rows = rows.lock().unwrap();
                let deduplicated = rows.iter().any(|(_, held): &(String, String)| *held == id);
                if !deduplicated {
                    rows.push((thread, id));
                }
                std::future::ready(Ok(crate::session_flow::AppendReceipt {
                    message_id: "message-0".to_owned(),
                    deduplicated,
                }))
            }
        };
        for _ in 0..2 {
            let response = queue_input_with_receipts(&peer, &who, "req-1", &body, &persist).await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
        }
        assert_eq!(rows.lock().unwrap().len(), 1);
        assert_eq!(owner.queued_inputs.drain("req-1").len(), 1, "one delivery");
    }

    /// One captured log line as `(field, value)` pairs.
    type LogLine = Vec<(String, String)>;

    /// Collects the fields of every `run cancel` line written on this thread.
    #[derive(Clone, Default)]
    struct CancelLines(Arc<std::sync::Mutex<Vec<LogLine>>>);

    struct Fields<'a>(&'a mut Vec<(String, String)>);

    impl tracing::field::Visit for Fields<'_> {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.push((field.name().to_owned(), value.to_owned()));
        }

        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }
    }

    impl tracing::Subscriber for CancelLines {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.is_event() && *metadata.level() == tracing::Level::INFO
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = Vec::new();
            event.record(&mut Fields(&mut fields));
            if fields
                .iter()
                .any(|(name, value)| name == "message" && value == "run cancel")
            {
                self.0.lock().expect("log capture lock").push(fields);
            }
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The browser drops this route's errors, so the log line is the only
    /// record of what a Stop actually did. One line per call, whichever way it
    /// ends, and it carries ids and codes, never a credential.
    ///
    /// Serialised with the other tests that call `cancel_run`: while this
    /// thread-local subscriber is the only one registered, a callsite first hit
    /// on another thread caches the interest of that thread's no-op default,
    /// and the line would be lost here.
    #[tokio::test]
    #[serial_test::serial(run_cancel_log)]
    async fn cancel_run_logs_one_outcome_line_per_call() {
        let (state, _publisher) = test_state().await;
        let owner = claims("org-owner", "user-owner");
        let mut zdr_owner = owner.clone();
        zdr_owner.zdr = true;
        let lines = CancelLines::default();
        let _guard = tracing::subscriber::set_default(lines.clone());

        for (caller, run_id) in [
            (owner.clone(), "run-owned"),
            (owner.clone(), "run-finished"),
            (owner, "run-vanished"),
            (claims("org-owner", "user-other"), "run-owned"),
            (zdr_owner, "run-owned"),
        ] {
            let _ = cancel_run(
                State(state.clone()),
                Extension(caller),
                VerifiedSessionBearer::for_test("test-session-bearer"),
                Path(run_id.to_owned()),
            )
            .await;
        }

        let lines = lines.0.lock().expect("log capture lock").clone();
        // `[run_id, outcome, receipt_id, code, via]`, empty where the line has none.
        let outcomes: Vec<[String; 5]> = lines
            .iter()
            .map(|line| {
                ["run_id", "outcome", "receipt_id", "code", "via"].map(|name| {
                    line.iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default()
                })
            })
            .collect();
        assert_eq!(
            outcomes,
            [
                ["run-owned", "cancelled", "receipt-cancel-1", "", "none"],
                ["run-finished", "already_terminal", "receipt-existing", "", "none"],
                ["run-vanished", "error", "", "NotFound", ""],
                ["run-owned", "owner_rejected", "", "", ""],
                ["run-owned", "zdr_rejected", "", "", ""],
            ]
        );
        assert!(
            lines
                .iter()
                .flatten()
                .all(|(_, value)| !value.contains("test-session-bearer")),
            "a cancel log line must never carry the credential: {lines:?}"
        );
    }
}

#[cfg(test)]
mod session_thread_error_tests {
    use super::*;

    #[test]
    fn not_found_maps_to_404_thread_not_found() {
        let (status, Json(body)) = session_thread_error(
            "session-core list_conversation failed",
            &tonic::Status::not_found("thread 01KX3CTBFND3888N31344E5YV4 not found"),
        );

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "thread_not_found");
        assert_eq!(
            body["error"]["message"],
            "thread 01KX3CTBFND3888N31344E5YV4 not found"
        );
    }

    #[test]
    fn permission_denied_maps_to_403_forbidden() {
        let (status, Json(body)) = session_thread_error(
            "session-core list_conversation failed",
            &tonic::Status::permission_denied("tenant access denied"),
        );

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
        assert_eq!(body["error"]["message"], "tenant access denied");
    }

    #[test]
    fn transport_and_unknown_errors_stay_502() {
        // Every non-NotFound/PermissionDenied/InvalidArgument code — including
        // transport failures and server-side faults — must keep the 502
        // default so a genuine upstream problem is never misreported as a
        // client error.
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::Internal,
            tonic::Code::DeadlineExceeded,
            tonic::Code::Unknown,
            tonic::Code::Unauthenticated,
        ] {
            let (status, Json(body)) = session_thread_error(
                "session-core list_threads failed",
                &tonic::Status::new(code, "boom"),
            );

            assert_eq!(status, StatusCode::BAD_GATEWAY, "code {code:?} -> 502");
            assert_eq!(
                body["error"], "session-core list_threads failed: boom",
                "code {code:?} keeps the contextual string body",
            );
        }

        let (status, Json(body)) = session_thread_error(
            "session-core list_threads failed",
            &tonic::Status::invalid_argument("thread id is required"),
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_thread_request");
        assert_eq!(body["error"]["message"], "thread id is required");
    }
}

#[cfg(test)]
mod session_memory_error_tests {
    use super::*;

    #[test]
    fn not_found_maps_to_404_memory_not_found() {
        let (status, Json(body)) = session_memory_error(
            "session-core delete_memory failed",
            &tonic::Status::not_found("memory not found"),
        );

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "memory_not_found");
        assert_eq!(body["error"]["message"], "memory not found");
    }

    #[test]
    fn permission_denied_maps_to_403_forbidden() {
        let (status, Json(body)) = session_memory_error(
            "session-core list_memory failed",
            &tonic::Status::permission_denied("user access denied"),
        );

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
        assert_eq!(body["error"]["message"], "user access denied");
    }

    #[test]
    fn transport_and_unknown_errors_stay_502() {
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::Internal,
            tonic::Code::DeadlineExceeded,
            tonic::Code::Unknown,
            tonic::Code::InvalidArgument,
            tonic::Code::Unauthenticated,
        ] {
            let (status, Json(body)) = session_memory_error(
                "session-core list_memory failed",
                &tonic::Status::new(code, "boom"),
            );

            assert_eq!(status, StatusCode::BAD_GATEWAY, "code {code:?} -> 502");
            assert_eq!(
                body["error"], "session-core list_memory failed: boom",
                "code {code:?} keeps the contextual string body",
            );
        }
    }
}

#[cfg(test)]
mod durable_trace_projection_tests {
    use super::*;

    #[test]
    fn replay_projection_keeps_envelope_and_omits_any_payload() {
        let event = Event {
            event_id: "evt-1".to_owned(),
            // EVENT_TYPE_RUN_STARTED in the shared proto. Using the numeric
            // value keeps this fixture resilient to generated Rust variant
            // naming while still proving the stable wire label.
            event_type: 90,
            schema_version: 1,
            ts: Some(prost_types::Timestamp {
                seconds: 1_725_000_000,
                nanos: 0,
            }),
            producer: "session-core".to_owned(),
            correlation_id: "corr-1".to_owned(),
            causation_id: "cause-1".to_owned(),
            resource_ref: "run/run-1".to_owned(),
            payload: Some(prost_types::Any {
                type_url: "type.googleapis.com/private.Payload".to_owned(),
                value: br#"{"secret":"must-not-cross"}"#.to_vec(),
            }),
            ..Default::default()
        };

        let value = serde_json::to_value(durable_thread_event(event))
            .expect("durable trace envelope serializes");
        assert_eq!(value["event_id"], "evt-1");
        assert_eq!(value["event_type"], "EVENT_TYPE_RUN_STARTED");
        assert_eq!(value["producer"], "session-core");
        assert_eq!(value["resource_ref"], "run/run-1");
        assert!(value.get("payload").is_none());
        assert!(value.get("org_id").is_none());
        assert!(value.get("user_id").is_none());
    }

    #[test]
    fn replay_projection_prefers_typed_thread_and_message_discriminators() {
        for (type_url, expected) in [
            (
                "type.googleapis.com/model_plane.v1.ThreadCreated",
                "THREAD_CREATED",
            ),
            (
                "type.googleapis.com/model_plane.v1.MessageAppended",
                "MESSAGE_APPENDED",
            ),
            (
                "type.googleapis.com/model_plane.v1.StepCompleted",
                "STEP_COMPLETED",
            ),
            (
                "type.googleapis.com/model_plane.v1.RunCancelled",
                "RUN_CANCELLED",
            ),
        ] {
            let event = Event {
                // 120 is PLAN_CREATED in the shared enum. Legacy thread and
                // message rows used this value accidentally, so the typed
                // payload discriminator must win during replay.
                event_type: 120,
                payload: Some(prost_types::Any {
                    type_url: type_url.to_owned(),
                    value: br#"{"private":"bytes"}"#.to_vec(),
                }),
                ..Default::default()
            };

            assert_eq!(durable_thread_event_type(&event), expected);
        }
    }
}

#[cfg(test)]
// The inference-core stand-in answers with `tonic::Status`, as the trait does.
#[allow(clippy::result_large_err)]
mod speech_route_tests {
    //! The two audio routes, `/v1/ai/dictate` and `/v1/ai/speech`: zero
    //! retention reaches inference-core on every speech call, and their body
    //! limit is raised without touching any other route.
    use super::*;
    use crate::auth::VerifiedInferenceBearer;
    use mp_contracts::model_plane::v1 as pb;
    use std::{
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tower::ServiceExt as _;

    type InferStream =
        Pin<Box<dyn futures::Stream<Item = Result<pb::InferChunk, tonic::Status>> + Send>>;
    type VideoStream = Pin<
        Box<
            dyn futures::Stream<
                    Item = Result<pb::StreamVideoGenerationContentResponse, tonic::Status>,
                > + Send,
        >,
    >;

    /// Inference-core stand-in. It records every speech and clean-up request
    /// and, like the real service while no speech deployment is attested,
    /// refuses speech that asks for zero retention. `attested` stands for a
    /// future attested deployment that serves ZDR speech.
    #[derive(Clone, Default)]
    pub(super) struct SpeechInference {
        attested: bool,
        /// When set, `infer` answers with this instead of the clean-up stand-in (the ai_chat tests).
        reply: Arc<Mutex<Option<pb::InferResponse>>>,
        transcribe: Arc<Mutex<Vec<pb::TranscribeSpeechRequest>>>,
        synthesize: Arc<Mutex<Vec<pb::SynthesizeSpeechRequest>>>,
        infer: Arc<Mutex<Vec<pb::InferRequest>>>,
    }

    impl SpeechInference {
        fn refuse_unattested(&self, zdr: bool, operation: &str) -> Result<(), tonic::Status> {
            if zdr && !self.attested {
                return Err(tonic::Status::failed_precondition(format!(
                    "ZDR {operation} is unavailable because no provider deployment is independently attested"
                )));
            }
            Ok(())
        }

        fn transcriptions(&self) -> Vec<pb::TranscribeSpeechRequest> {
            self.transcribe
                .lock()
                .expect("transcription capture")
                .clone()
        }

        fn syntheses(&self) -> Vec<pb::SynthesizeSpeechRequest> {
            self.synthesize.lock().expect("synthesis capture").clone()
        }

        pub(super) fn answering(&self, reply: pb::InferResponse) {
            *self.reply.lock().expect("reply setting") = Some(reply);
        }

        pub(super) fn cleanups(&self) -> Vec<pb::InferRequest> {
            self.infer.lock().expect("clean-up capture").clone()
        }
    }

    fn unused<T>() -> Result<tonic::Response<T>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "not used by the speech route tests",
        ))
    }

    #[tonic::async_trait]
    impl pb::inference_core_server::InferenceCore for SpeechInference {
        type InferStreamStream = InferStream;
        type StreamVideoGenerationContentStream = VideoStream;

        async fn infer(
            &self,
            request: tonic::Request<pb::InferRequest>,
        ) -> Result<tonic::Response<pb::InferResponse>, tonic::Status> {
            self.infer
                .lock()
                .expect("clean-up capture")
                .push(request.into_inner());
            if let Some(reply) = self.reply.lock().expect("reply setting").clone() {
                return Ok(tonic::Response::new(reply));
            }
            Ok(tonic::Response::new(pb::InferResponse {
                content: "Hei der.".to_owned(),
                model_used: "cleanup-model".to_owned(),
                ..Default::default()
            }))
        }

        async fn infer_stream(
            &self,
            _request: tonic::Request<pb::InferRequest>,
        ) -> Result<tonic::Response<Self::InferStreamStream>, tonic::Status> {
            unused()
        }

        async fn create_embedding(
            &self,
            _request: tonic::Request<pb::CreateEmbeddingRequest>,
        ) -> Result<tonic::Response<pb::CreateEmbeddingResponse>, tonic::Status> {
            unused()
        }

        async fn get_model_access(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::GetModelAccessRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetModelAccessResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_model_access not used"))
        }

        async fn list_models(
            &self,
            _request: tonic::Request<pb::ListModelsRequest>,
        ) -> Result<tonic::Response<pb::ListModelsResponse>, tonic::Status> {
            unused()
        }

        async fn synthesize_speech(
            &self,
            request: tonic::Request<pb::SynthesizeSpeechRequest>,
        ) -> Result<tonic::Response<pb::SynthesizeSpeechResponse>, tonic::Status> {
            let request = request.into_inner();
            let zdr = request.zdr;
            self.synthesize
                .lock()
                .expect("synthesis capture")
                .push(request);
            self.refuse_unattested(zdr, "speech synthesis")?;
            Ok(tonic::Response::new(pb::SynthesizeSpeechResponse {
                audio: b"mp3-bytes".to_vec(),
                format: "mp3".to_owned(),
                ..Default::default()
            }))
        }

        async fn transcribe_speech(
            &self,
            request: tonic::Request<pb::TranscribeSpeechRequest>,
        ) -> Result<tonic::Response<pb::TranscribeSpeechResponse>, tonic::Status> {
            let request = request.into_inner();
            let zdr = request.zdr;
            self.transcribe
                .lock()
                .expect("transcription capture")
                .push(request);
            self.refuse_unattested(zdr, "speech transcription")?;
            Ok(tonic::Response::new(pb::TranscribeSpeechResponse {
                text: "eh hei der".to_owned(),
                detected_language: "nb".to_owned(),
                model_used: "stt-model".to_owned(),
                provider_used: "azure-openai".to_owned(),
                ..Default::default()
            }))
        }

        async fn list_speech_voices(
            &self,
            _request: tonic::Request<pb::ListSpeechVoicesRequest>,
        ) -> Result<tonic::Response<pb::ListSpeechVoicesResponse>, tonic::Status> {
            unused()
        }

        async fn translate_text(
            &self,
            _request: tonic::Request<pb::TranslateTextRequest>,
        ) -> Result<tonic::Response<pb::TranslateTextResponse>, tonic::Status> {
            unused()
        }

        async fn batch_translate_text(
            &self,
            _request: tonic::Request<pb::BatchTranslateTextRequest>,
        ) -> Result<tonic::Response<pb::BatchTranslateTextResponse>, tonic::Status> {
            unused()
        }

        async fn detect_text_language(
            &self,
            _request: tonic::Request<pb::DetectTextLanguageRequest>,
        ) -> Result<tonic::Response<pb::DetectTextLanguageResponse>, tonic::Status> {
            unused()
        }

        async fn list_translation_languages(
            &self,
            _request: tonic::Request<pb::ListTranslationLanguagesRequest>,
        ) -> Result<tonic::Response<pb::ListTranslationLanguagesResponse>, tonic::Status> {
            unused()
        }

        async fn generate_image(
            &self,
            _request: tonic::Request<pb::GenerateImageRequest>,
        ) -> Result<tonic::Response<pb::GenerateImageResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_image(
            &self,
            _request: tonic::Request<pb::AnalyzeImageRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeImageResponse>, tonic::Status> {
            unused()
        }

        async fn extract_image_text(
            &self,
            _request: tonic::Request<pb::ExtractImageTextRequest>,
        ) -> Result<tonic::Response<pb::ExtractImageTextResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_document(
            &self,
            _request: tonic::Request<pb::AnalyzeDocumentRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeDocumentResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_language(
            &self,
            _request: tonic::Request<pb::AnalyzeLanguageRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeLanguageResponse>, tonic::Status> {
            unused()
        }

        async fn create_realtime_session(
            &self,
            _request: tonic::Request<pb::CreateRealtimeSessionRequest>,
        ) -> Result<tonic::Response<pb::CreateRealtimeSessionResponse>, tonic::Status> {
            unused()
        }

        async fn create_video_generation_job(
            &self,
            _request: tonic::Request<pb::CreateVideoGenerationJobRequest>,
        ) -> Result<tonic::Response<pb::CreateVideoGenerationJobResponse>, tonic::Status> {
            unused()
        }

        async fn get_video_generation_job(
            &self,
            _request: tonic::Request<pb::GetVideoGenerationJobRequest>,
        ) -> Result<tonic::Response<pb::GetVideoGenerationJobResponse>, tonic::Status> {
            unused()
        }

        async fn stream_video_generation_content(
            &self,
            _request: tonic::Request<pb::StreamVideoGenerationContentRequest>,
        ) -> Result<tonic::Response<Self::StreamVideoGenerationContentStream>, tonic::Status>
        {
            unused()
        }
    }

    /// An `AppState` whose inference client talks to `inference`.
    pub(super) async fn state_with(inference: SpeechInference) -> AppState {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference stand-in");
        let address = listener.local_addr().expect("inference stand-in address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(pb::inference_core_server::InferenceCoreServer::new(
                    inference,
                ))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("inference stand-in endpoint")
            .connect()
            .await
            .expect("connect inference stand-in");
        let mut state = AppState::new();
        state.inference_client = pb::inference_core_client::InferenceCoreClient::new(channel);
        state
    }

    fn claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    pub(super) fn bearer() -> VerifiedInferenceBearer {
        VerifiedInferenceBearer::for_test("inference-test-bearer")
    }

    /// The body the SPA's dictation client sends, parsed the way the route
    /// parses it.
    fn dictation(zdr: bool) -> AiDictateRequest {
        serde_json::from_value(json!({
            "audio_base64": STANDARD.encode(b"webm-audio"),
            "format": "webm",
            "language": "nb-NO",
            "context": "chat message",
            "zdr": zdr,
        }))
        .expect("a dictation body parses")
    }

    fn speech(body: Value) -> AiSpeechRequest {
        serde_json::from_value(body).expect("a speech body parses")
    }

    /// Temporary chat sets `zdr` in the body; an organisation that requires
    /// zero retention sets it in the token. Either way the audio travels with
    /// the flag, inference-core refuses it, and no clean-up pass runs.
    #[tokio::test]
    async fn zdr_dictation_sends_the_flag_with_the_audio_and_is_refused() {
        for (token_zdr, body_zdr) in [(false, true), (true, false)] {
            let case = format!("token_zdr={token_zdr} body_zdr={body_zdr}");
            let inference = SpeechInference::default();
            let state = state_with(inference.clone()).await;

            let error = ai_dictate(
                State(state),
                Extension(claims(token_zdr)),
                bearer(),
                Json(dictation(body_zdr)),
            )
            .await
            .expect_err("ZDR dictation must be refused while speech is unattested");
            assert_eq!(error.0, StatusCode::PRECONDITION_FAILED, "{case}");

            let sent = inference.transcriptions();
            assert_eq!(sent.len(), 1, "{case}");
            assert!(sent[0].zdr, "{case}: the audio must carry the ZDR flag");
            assert!(
                inference.cleanups().is_empty(),
                "{case}: a refused transcript must not reach the clean-up pass"
            );
        }
    }

    /// Once a speech deployment is attested, both legs of one dictation carry
    /// the same flag.
    #[tokio::test]
    async fn attested_zdr_dictation_carries_the_flag_on_both_legs() {
        let inference = SpeechInference {
            attested: true,
            ..SpeechInference::default()
        };
        let state = state_with(inference.clone()).await;

        let Json(body) = ai_dictate(
            State(state),
            Extension(claims(false)),
            bearer(),
            Json(dictation(true)),
        )
        .await
        .expect("an attested stand-in serves ZDR dictation");
        assert_eq!(body["cleaned"], true);

        let sent = inference.transcriptions();
        let cleanups = inference.cleanups();
        assert_eq!((sent.len(), cleanups.len()), (1, 1));
        assert!(sent[0].zdr, "speech-to-text leg");
        assert!(cleanups[0].zdr, "clean-up leg");
    }

    #[tokio::test]
    async fn ordinary_dictation_is_transcribed_and_cleaned_without_zdr() {
        let inference = SpeechInference::default();
        let state = state_with(inference.clone()).await;

        let Json(body) = ai_dictate(
            State(state),
            Extension(claims(false)),
            bearer(),
            Json(dictation(false)),
        )
        .await
        .expect("ordinary dictation succeeds");
        assert_eq!(body["text"], "Hei der.");
        assert_eq!(body["raw_text"], "eh hei der");
        assert_eq!(body["cleaned"], true);

        let sent = inference.transcriptions();
        assert_eq!(sent.len(), 1);
        assert!(!sent[0].zdr);
        assert_eq!(sent[0].audio, b"webm-audio");
        assert_eq!(sent[0].provider_hint, "azure-openai");
        let cleanups = inference.cleanups();
        assert_eq!(cleanups.len(), 1);
        assert!(!cleanups[0].zdr);
    }

    /// `/v1/ai/speech` read no `zdr` at all before, so Temporary chat's flag
    /// was dropped for both read-aloud and transcription.
    #[tokio::test]
    async fn zdr_speech_route_sends_the_flag_and_is_refused() {
        for (token_zdr, body_zdr) in [(false, true), (true, false)] {
            let case = format!("token_zdr={token_zdr} body_zdr={body_zdr}");
            let inference = SpeechInference::default();
            let state = state_with(inference.clone()).await;

            let tts = ai_speech(
                State(state.clone()),
                Extension(claims(token_zdr)),
                bearer(),
                Json(speech(json!({
                    "operation": "tts",
                    "text": "Hei der.",
                    "voice": "nb-NO-PernilleNeural",
                    "format": "mp3",
                    "provider": "azure-speech",
                    "zdr": body_zdr,
                }))),
            )
            .await
            .expect_err("ZDR synthesis must be refused while speech is unattested");
            assert_eq!(tts.0, StatusCode::PRECONDITION_FAILED, "{case}");

            let stt = ai_speech(
                State(state),
                Extension(claims(token_zdr)),
                bearer(),
                Json(speech(json!({
                    "operation": "stt",
                    "audio_base64": STANDARD.encode(b"mp3-audio"),
                    "zdr": body_zdr,
                }))),
            )
            .await
            .expect_err("ZDR transcription must be refused while speech is unattested");
            assert_eq!(stt.0, StatusCode::PRECONDITION_FAILED, "{case}");

            let syntheses = inference.syntheses();
            let transcriptions = inference.transcriptions();
            assert_eq!((syntheses.len(), transcriptions.len()), (1, 1), "{case}");
            assert!(syntheses[0].zdr, "{case}: text-to-speech");
            assert!(transcriptions[0].zdr, "{case}: speech-to-text");
        }
    }

    #[tokio::test]
    async fn ordinary_speech_route_calls_are_served_without_zdr() {
        let inference = SpeechInference::default();
        let state = state_with(inference.clone()).await;

        let Json(tts) = ai_speech(
            State(state.clone()),
            Extension(claims(false)),
            bearer(),
            Json(speech(
                json!({ "text": "Hei der.", "provider": "azure-speech" }),
            )),
        )
        .await
        .expect("ordinary synthesis succeeds");
        assert_eq!(tts["audio_base64"], STANDARD.encode(b"mp3-bytes"));

        let Json(stt) = ai_speech(
            State(state),
            Extension(claims(false)),
            bearer(),
            Json(speech(
                json!({ "audio_base64": STANDARD.encode(b"mp3-audio") }),
            )),
        )
        .await
        .expect("ordinary transcription succeeds");
        assert_eq!(stt["text"], "eh hei der");

        assert!(!inference.syntheses()[0].zdr);
        assert!(!inference.transcriptions()[0].zdr);
    }

    /// `ai_routes()` with the identity the auth middleware would attach, so a
    /// request reaches the handler's body extractor.
    fn ai_router(state: AppState) -> Router {
        ai_routes()
            .with_state(state)
            .layer(Extension(claims(false)))
            .layer(Extension(bearer()))
    }

    /// A JSON body of at least `size` bytes whose valid base64 `field` fills it.
    fn base64_body(size: usize, field: &str) -> Vec<u8> {
        let prefix = format!("{{\"format\":\"webm\",\"{field}\":\"");
        let suffix = "\"}";
        let quads = (size - prefix.len() - suffix.len()).div_ceil(4);
        let mut body = prefix.into_bytes();
        body.resize(body.len() + quads * 4, b'A');
        body.extend_from_slice(suffix.as_bytes());
        body
    }

    async fn post_json(router: Router, uri: &str, body: Vec<u8>) -> StatusCode {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request builds");
        router
            .oneshot(request)
            .await
            .expect("the router answers")
            .status()
    }

    const MIB: usize = 1024 * 1024;

    /// Base64 in JSON held recordings to about 1.5 MiB under axum's 2 MiB
    /// default. The audio routes now take 8 MiB bodies.
    #[tokio::test]
    async fn audio_routes_accept_a_3_mib_body_and_refuse_9_mib_with_413() {
        let inference = SpeechInference::default();
        let state = state_with(inference.clone()).await;

        for uri in ["/v1/ai/dictate", "/v1/ai/speech"] {
            let accepted = post_json(
                ai_router(state.clone()),
                uri,
                base64_body(3 * MIB, "audio_base64"),
            )
            .await;
            assert_eq!(accepted, StatusCode::OK, "{uri}: 3 MiB reaches the handler");

            let refused = post_json(
                ai_router(state.clone()),
                uri,
                base64_body(9 * MIB, "audio_base64"),
            )
            .await;
            assert_eq!(refused, StatusCode::PAYLOAD_TOO_LARGE, "{uri}: 9 MiB");
        }

        let sent = inference.transcriptions();
        assert_eq!(sent.len(), 2, "only the two 3 MiB bodies were transcribed");
        assert!(
            sent.iter().all(|request| request.audio.len() > 2 * MIB),
            "each take carries more audio than a whole 2 MiB body could"
        );
    }

    /// The raised limit is scoped to the audio routes.
    #[tokio::test]
    async fn other_ai_routes_keep_the_2_mib_default() {
        let state = state_with(SpeechInference::default()).await;

        let status = post_json(
            ai_router(state),
            "/v1/ai/images/ocr",
            base64_body(3 * MIB, "content_base64"),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    }
}

/// WS04 S4 review defect 1: the retry of the approval's durable write.
#[cfg(test)]
mod set_run_mode_request_tests {
    use super::set_run_mode_request_for;
    use mp_contracts::autonomy::AutonomyEscalation;
    use mp_contracts::model_plane::v1::AutonomyRung;

    fn escalation() -> AutonomyEscalation {
        AutonomyEscalation::request(
            AutonomyRung::ReadOnly,
            AutonomyRung::WorkspaceWrite,
            "the plan writes its report into the workspace",
        )
        .expect("a justified escalation")
    }

    /// WS04 S4 second review, defect 1: a waived credential's approval carries the mode change
    /// alone. Session Core refuses a waived caller that states a grant, and would refuse the
    /// whole call, mode update included.
    #[test]
    fn a_waived_approval_states_the_mode_and_nothing_that_feeds_a_durable_grant() {
        let request = set_run_mode_request_for("run-1", "org-1", &escalation(), "plan-7", true);
        assert_eq!(request.run_id, "run-1");
        assert_eq!(request.org_id, "org-1");
        assert_eq!(request.mode, "execute");
        assert_eq!(request.granted_rung, AutonomyRung::Unspecified as i32);
        assert_eq!(request.justification, "");
        assert_eq!(request.approved_plan_id, "");
    }

    #[test]
    fn an_ordinary_approval_states_its_grant_and_plan() {
        let request = set_run_mode_request_for("run-1", "org-1", &escalation(), "plan-7", false);
        assert_eq!(request.mode, "execute");
        assert_eq!(request.granted_rung, AutonomyRung::WorkspaceWrite as i32);
        assert_eq!(request.justification, "the plan writes its report into the workspace");
        assert_eq!(request.approved_plan_id, "plan-7");
        let no_plan = set_run_mode_request_for("run-1", "org-1", &escalation(), "", false);
        assert_eq!(no_plan.approved_plan_id, "");
    }
}

#[cfg(test)]
mod set_run_mode_retry_tests {
    use super::{retry_set_run_mode, set_run_mode_retryable, SET_RUN_MODE_RETRY_BACKOFF};
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[test]
    fn the_backoff_is_about_100_300_and_900_ms() {
        assert_eq!(
            SET_RUN_MODE_RETRY_BACKOFF,
            [
                Duration::from_millis(100),
                Duration::from_millis(300),
                Duration::from_millis(900)
            ]
        );
    }

    #[test]
    fn only_a_transient_failure_is_retried() {
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::DeadlineExceeded,
            tonic::Code::ResourceExhausted,
            tonic::Code::Aborted,
            tonic::Code::Internal,
            tonic::Code::Unknown,
        ] {
            assert!(set_run_mode_retryable(code), "{code:?}");
        }
        for code in [
            tonic::Code::PermissionDenied,
            tonic::Code::Unauthenticated,
            tonic::Code::InvalidArgument,
            tonic::Code::NotFound,
            tonic::Code::FailedPrecondition,
            tonic::Code::Unimplemented,
        ] {
            assert!(!set_run_mode_retryable(code), "{code:?}");
        }
    }

    /// Two transient failures then a success: three attempts, and the waits are 100 then 300 ms.
    #[tokio::test(start_paused = true)]
    async fn a_write_that_fails_twice_then_succeeds_is_retried_with_the_backoff() {
        let attempts = AtomicUsize::new(0);
        let started = tokio::time::Instant::now();
        let result = retry_set_run_mode(&SET_RUN_MODE_RETRY_BACKOFF, || async {
            if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(tonic::Status::unavailable("restarting"))
            } else {
                Ok("recorded")
            }
        })
        .await;
        assert_eq!(result.expect("the third attempt succeeds"), "recorded");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(started.elapsed(), Duration::from_millis(400));
    }

    /// A write that never succeeds is attempted four times (the first and three retries), waits
    /// 1.3 s in all, and returns the last failure.
    #[tokio::test(start_paused = true)]
    async fn a_write_that_always_fails_is_bounded() {
        let attempts = AtomicUsize::new(0);
        let started = tokio::time::Instant::now();
        let result: Result<(), tonic::Status> =
            retry_set_run_mode(&SET_RUN_MODE_RETRY_BACKOFF, || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(tonic::Status::deadline_exceeded("slow"))
            })
            .await;
        assert_eq!(result.expect_err("it gives up").code(), tonic::Code::DeadlineExceeded);
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        assert_eq!(started.elapsed(), Duration::from_millis(1_300));
    }

    /// A refusal is attempted once and returned at once.
    #[tokio::test(start_paused = true)]
    async fn a_refusal_is_not_retried() {
        let attempts = AtomicUsize::new(0);
        let started = tokio::time::Instant::now();
        let result: Result<(), tonic::Status> =
            retry_set_run_mode(&SET_RUN_MODE_RETRY_BACKOFF, || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(tonic::Status::permission_denied("not yours"))
            })
            .await;
        assert_eq!(result.expect_err("refused").code(), tonic::Code::PermissionDenied);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}

#[cfg(test)]
mod tool_policy_proxy_tests {
    use super::*;
    use crate::auth::VerifiedCapabilityBearer;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    fn claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    /// A stand-in capability-core that records `METHOD path?query` and the body.
    async fn recorder() -> (
        String,
        Arc<Mutex<Vec<(String, Value)>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let seen: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback(axum::routing::any({
            let seen = Arc::clone(&seen);
            move |method: axum::http::Method, uri: axum::http::Uri, body: String| {
                let seen = Arc::clone(&seen);
                async move {
                    let body = serde_json::from_str(&body).unwrap_or(Value::Null);
                    seen.lock()
                        .expect("recorder lock")
                        .push((format!("{method} {uri}"), body));
                    Json(json!([]))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind recorder");
        let address = listener.local_addr().expect("recorder address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve recorder");
        });
        (format!("http://{address}"), seen, task)
    }

    #[test]
    fn registry_identifiers_cannot_redirect_a_forwarded_request() {
        for ok in [
            "cap.mcp_tool.0123abcd",
            "cap.command.shell",
            "srv",
            "a",
            "echo_read",
            "tool:v1-x",
        ] {
            assert!(is_registry_identifier(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a/../b",
            "a%2Fb",
            "a b",
            "a?b=1",
            "a#b",
            "a\\b",
            "_x",
            "-x",
            ".hidden",
            "é",
            &"x".repeat(201),
        ] {
            assert!(!is_registry_identifier(bad), "{bad:?}");
        }
        assert!(is_registry_identifier(&"x".repeat(200)));
    }

    #[tokio::test]
    async fn the_routes_forward_to_the_registry_paths_with_the_callers_own_bearer() {
        let (base_url, seen, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let claims = claims(false);

        let _ = list_tool_policies_proxy(
            State(state.clone()),
            Extension(claims.clone()),
            bearer.clone(),
            Query(
                [("capability_id".to_owned(), "cap.mcp_tool.x".to_owned())]
                    .into_iter()
                    .collect(),
            ),
        )
        .await
        .expect("list");
        let body = json!({"scope": "org", "decision": "allow", "pinned_digest": "d"});
        let _ = set_tool_policy_proxy(
            State(state.clone()),
            Extension(claims.clone()),
            bearer.clone(),
            Path("cap.mcp_tool.x".to_owned()),
            Json(body.clone()),
        )
        .await
        .expect("set");
        let _ = revoke_tool_policy_proxy(
            State(state.clone()),
            Extension(claims.clone()),
            bearer.clone(),
            Path("cap.mcp_tool.x".to_owned()),
            Query(
                [("scope".to_owned(), "org".to_owned())]
                    .into_iter()
                    .collect(),
            ),
        )
        .await
        .expect("revoke");
        let _ = list_mcp_server_tools_proxy(
            State(state.clone()),
            Extension(claims.clone()),
            bearer.clone(),
            Path("srv".to_owned()),
        )
        .await
        .expect("tools");
        let approval = json!({"approved_digest": "d"});
        let _ = approve_mcp_tool_proxy(
            State(state),
            Extension(claims),
            bearer,
            Path(("srv".to_owned(), "echo_read".to_owned())),
            Json(approval.clone()),
        )
        .await
        .expect("approve");

        let seen = seen.lock().expect("lock").clone();
        let requests: Vec<&str> = seen.iter().map(|(line, _)| line.as_str()).collect();
        assert_eq!(
            requests,
            [
                "GET /api/v1/tool-policies?capability_id=cap.mcp_tool.x",
                "PUT /api/v1/tool-policies/cap.mcp_tool.x",
                "DELETE /api/v1/tool-policies/cap.mcp_tool.x?scope=org",
                "GET /api/v1/mcp/srv/tools",
                "POST /api/v1/mcp/srv/tools/echo_read/approve",
            ]
        );
        // The body is forwarded as sent: the scope, the decision and the digest
        // are capability-core's to check, and nothing is added to it.
        assert_eq!(seen[1].1, body);
        assert_eq!(seen[4].1, approval);
        task.abort();
    }

    #[tokio::test]
    async fn a_zdr_caller_is_refused_every_write_before_anything_is_forwarded() {
        let (base_url, seen, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let zdr = claims(true);

        let set = set_tool_policy_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Path("cap.mcp_tool.x".to_owned()),
            Json(json!({"decision": "never"})),
        )
        .await
        .expect_err("a ZDR set must be refused");
        assert_eq!(set.0, StatusCode::PRECONDITION_FAILED);
        let revoke = revoke_tool_policy_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Path("cap.mcp_tool.x".to_owned()),
            Query(Default::default()),
        )
        .await
        .expect_err("a ZDR revoke must be refused");
        assert_eq!(revoke.0, StatusCode::PRECONDITION_FAILED);
        let approve = approve_mcp_tool_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Path(("srv".to_owned(), "echo_read".to_owned())),
            Json(json!({"approved_digest": "d"})),
        )
        .await
        .expect_err("a ZDR approval must be refused");
        assert_eq!(approve.0, StatusCode::PRECONDITION_FAILED);
        assert!(
            seen.lock().expect("lock").is_empty(),
            "a ZDR write was forwarded"
        );

        // Reading is not a durable mutation and stays available under ZDR.
        let _ = list_tool_policies_proxy(
            State(state),
            Extension(zdr),
            bearer,
            Query(Default::default()),
        )
        .await
        .expect("a ZDR caller may read the policies in force");
        assert_eq!(seen.lock().expect("lock").len(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn an_identifier_that_is_not_a_registry_id_is_refused_before_forwarding() {
        let (base_url, seen, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");

        let refused = set_tool_policy_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Path("../internal/mcp/oauth-token".to_owned()),
            Json(json!({"decision": "never"})),
        )
        .await
        .expect_err("a path-walking id must be refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        let refused = approve_mcp_tool_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Path(("srv".to_owned(), "a/../../x".to_owned())),
            Json(json!({"approved_digest": "d"})),
        )
        .await
        .expect_err("a path-walking tool name must be refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        let refused = list_tool_policies_proxy(
            State(state),
            Extension(claims(false)),
            bearer,
            Query(
                [("capability_id".to_owned(), "a&org_id=org-b".to_owned())]
                    .into_iter()
                    .collect(),
            ),
        )
        .await
        .expect_err("an injected query must be refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(seen.lock().expect("lock").is_empty());
        task.abort();
    }
}

#[cfg(test)]
mod org_agent_policy_proxy_tests {
    use super::*;
    use crate::auth::VerifiedCapabilityBearer;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    fn claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    /// What a stand-in capability-core received, in order: `METHOD path?query`, the `If-Match` header and the body.
    type Received = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

    async fn recorder() -> (String, Received, tokio::task::JoinHandle<()>) {
        let received: Received = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback(axum::routing::any({
            let received = Arc::clone(&received);
            move |method: axum::http::Method,
                  uri: axum::http::Uri,
                  headers: axum::http::HeaderMap,
                  body: String| {
                let received = Arc::clone(&received);
                async move {
                    let if_match = headers
                        .get("if-match")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    let body = serde_json::from_str(&body).unwrap_or(Value::Null);
                    received.lock().expect("recorder lock").push((
                        format!("{method} {uri}"),
                        if_match,
                        body,
                    ));
                    Json(json!({"ok": true}))
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind recorder");
        let address = listener.local_addr().expect("recorder address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve recorder");
        });
        (format!("http://{address}"), received, task)
    }

    #[tokio::test]
    async fn a_policy_write_names_its_revision_in_if_match_and_forwards_the_rest() {
        let (base_url, received, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let settings = json!({"mode_cap": "read_only", "lockdown": false});

        let _ = set_org_agent_policy_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Json(json!({"settings": settings.clone(), "revision": 4})),
        )
        .await
        .expect("a write at a revision");
        let _ = set_org_agent_policy_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Json(json!({"settings": settings.clone()})),
        )
        .await
        .expect("a write with no revision is forwarded, for capability-core to refuse");

        let received = received.lock().expect("lock").clone();
        assert_eq!(received[0].0, "PUT /api/v1/org-agent-policy");
        assert_eq!(received[0].1.as_deref(), Some("4"));
        // The revision leaves the body: capability-core refuses unknown fields.
        assert_eq!(received[0].2, json!({"settings": settings}));
        assert_eq!(received[1].1, None);
        task.abort();
    }

    #[tokio::test]
    async fn a_revision_that_is_not_a_non_negative_integer_is_refused_before_forwarding() {
        let (base_url, received, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        for revision in [json!("4"), json!(-1), json!(1.5), json!(null)] {
            let refused = set_org_agent_policy_proxy(
                State(state.clone()),
                Extension(claims(false)),
                bearer.clone(),
                Json(json!({"settings": {}, "revision": revision.clone()})),
            )
            .await
            .expect_err("a malformed revision must be refused");
            assert_eq!(refused.0, StatusCode::BAD_REQUEST, "{revision}");
        }
        assert!(
            received.lock().expect("lock").is_empty(),
            "a malformed revision was forwarded"
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_zdr_caller_is_refused_every_org_policy_and_hook_write_before_anything_is_forwarded()
    {
        let (base_url, received, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let zdr = claims(true);
        let rule = json!({
            "event": "pre_tool",
            "tool_matcher": "shell",
            "decision": "deny",
            "reason": "no shell"
        });

        let set = set_org_agent_policy_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Json(json!({"settings": {}, "revision": 0})),
        )
        .await
        .expect_err("a ZDR policy write must be refused");
        assert_eq!(set.0, StatusCode::PRECONDITION_FAILED);
        let create = create_org_hook_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Json(rule.clone()),
        )
        .await
        .expect_err("a ZDR hook create must be refused");
        assert_eq!(create.0, StatusCode::PRECONDITION_FAILED);
        let update = update_org_hook_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Path("ohr_1".to_owned()),
            Json(rule.clone()),
        )
        .await
        .expect_err("a ZDR hook update must be refused");
        assert_eq!(update.0, StatusCode::PRECONDITION_FAILED);
        let delete = delete_org_hook_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
            Path("ohr_1".to_owned()),
        )
        .await
        .expect_err("a ZDR hook retirement must be refused");
        assert_eq!(delete.0, StatusCode::PRECONDITION_FAILED);
        assert!(
            received.lock().expect("lock").is_empty(),
            "a ZDR write was forwarded"
        );

        // Reading the policy and the rules is not a durable mutation and stays open under ZDR.
        let _ = get_org_agent_policy_proxy(
            State(state.clone()),
            Extension(zdr.clone()),
            bearer.clone(),
        )
        .await
        .expect("a ZDR caller may read the policy in force");
        let _ = list_org_hooks_proxy(State(state), Extension(zdr), bearer)
            .await
            .expect("a ZDR caller may read the hook rules");
        assert_eq!(received.lock().expect("lock").len(), 2);
        task.abort();
    }

    #[tokio::test]
    async fn the_gateway_names_the_capability_core_paths_and_pins_the_effective_read() {
        let (base_url, received, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let rule = json!({
            "event": "pre_tool",
            "tool_matcher": "*",
            "decision": "ask",
            "reason": "check first",
            "priority": 5
        });

        let _ = get_org_agent_policy_effective_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
        )
        .await
        .expect("effective");
        let _ = list_org_hooks_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
        )
        .await
        .expect("list");
        let _ = create_org_hook_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Json(rule.clone()),
        )
        .await
        .expect("create");
        let _ = update_org_hook_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Path("ohr_1".to_owned()),
            Json(rule.clone()),
        )
        .await
        .expect("update");
        let _ = delete_org_hook_proxy(
            State(state),
            Extension(claims(false)),
            bearer,
            Path("ohr_1".to_owned()),
        )
        .await
        .expect("delete");

        let received = received.lock().expect("lock").clone();
        let lines: Vec<&str> = received.iter().map(|(line, _, _)| line.as_str()).collect();
        assert_eq!(
            lines,
            [
                "GET /api/v1/org-agent-policy/effective?user=me",
                "GET /api/v1/org-hooks",
                "POST /api/v1/org-hooks",
                "PUT /api/v1/org-hooks/ohr_1",
                "DELETE /api/v1/org-hooks/ohr_1",
            ]
        );
        // A rule is forwarded as sent: the gateway adds no org id and drops nothing from it.
        assert_eq!(received[2].2, rule);
        assert_eq!(received[3].2, rule);
        task.abort();
    }

    #[tokio::test]
    async fn a_hook_id_that_is_not_an_identifier_is_refused_before_forwarding() {
        let (base_url, received, task) = recorder().await;
        let mut state = AppState::new();
        state.capability_core_base_url = base_url;
        let bearer = VerifiedCapabilityBearer::for_test("capability-test-bearer");
        let rule = json!({"event": "pre_tool", "decision": "deny", "reason": "no"});

        let refused = update_org_hook_proxy(
            State(state.clone()),
            Extension(claims(false)),
            bearer.clone(),
            Path("../org-agent-policy".to_owned()),
            Json(rule),
        )
        .await
        .expect_err("a path-walking hook id must be refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        let refused = delete_org_hook_proxy(
            State(state),
            Extension(claims(false)),
            bearer,
            Path("ohr_1/../../x".to_owned()),
        )
        .await
        .expect_err("a hook id with a slash must be refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(received.lock().expect("lock").is_empty());
        task.abort();
    }
}

#[cfg(test)]
mod memory_controls_route_tests {
    use super::*;
    use mp_contracts::model_plane::v1::{MemoryControls, MemoryState};

    #[test]
    fn only_the_three_literal_words_parse_as_a_state() {
        assert_eq!(parse_memory_state("on"), Some(MemoryState::On));
        assert_eq!(parse_memory_state("paused"), Some(MemoryState::Paused));
        assert_eq!(parse_memory_state("off"), Some(MemoryState::Off));
        for refused in ["", "ON", "pause", "true", "unspecified"] {
            assert_eq!(parse_memory_state(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn controls_json_carries_the_decision_and_no_content() {
        let controls = MemoryControls {
            state: MemoryState::Off as i32,
            org_memory_policy: "allow".to_owned(),
            use_allowed: false,
            reason: "user_off".to_owned(),
            hidden_rows: 4,
            ..Default::default()
        };
        let body = memory_controls_json(&controls);
        assert_eq!(body["state"], "off");
        assert_eq!(body["reason"], "user_off");
        assert_eq!(body["hidden_rows"], 4);
        assert_eq!(body["use_allowed"], false);
        assert!(body["updated_at"].is_null());
    }

    #[test]
    fn a_gate_refusal_is_a_typed_conflict_not_an_outage() {
        let refused = session_memory_error(
            "session-core index_memory failed",
            &tonic::Status::failed_precondition("memory_not_enabled:user_paused"),
        );
        assert_eq!(refused.0, StatusCode::CONFLICT);
        assert_eq!(refused.1 .0["error"]["code"], "memory_not_enabled");
        assert_eq!(refused.1 .0["error"]["details"]["reason"], "user_paused");
    }
}

#[cfg(test)]
mod user_profile_route_tests {
    //! EP-8 S4: the three profile routes send session-core the verified claims' org and user and
    //! the verified session bearer, whatever the body says, and map its refusals to typed bodies.
    use super::*;
    use mp_contracts::model_plane::v1 as pb;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request as TonicRequest, Response as TonicResponse, Status};

    /// What session-core received: the RPC, its org and user, and the forwarded authorization.
    type Seen = Arc<Mutex<Vec<(&'static str, String, String, String)>>>;

    #[derive(Clone, Default)]
    struct ProfileMemory {
        seen: Seen,
    }

    impl ProfileMemory {
        fn record<T>(&self, rpc: &'static str, request: &TonicRequest<T>, org: &str, user: &str) {
            let authorization = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            self.seen
                .lock()
                .expect("capture")
                .push((rpc, org.to_owned(), user.to_owned(), authorization));
        }
    }

    fn unused<T>() -> Result<TonicResponse<T>, Status> {
        Err(Status::unimplemented("not used by the profile route tests"))
    }

    fn profile_answer() -> pb::UserProfileResponse {
        pb::UserProfileResponse {
            profile: Some(pb::UserProfile {
                content: "- Prefers tables".to_owned(),
                source_memory_ids: vec!["m-1".to_owned()],
                origin: "built".to_owned(),
                revision: 3,
                updated_at: None,
                sources_deleted_since_edit: 0,
            }),
            skip_reason: String::new(),
        }
    }

    #[tonic::async_trait]
    impl pb::memory_service_server::MemoryService for ProfileMemory {
        async fn search_memory(
            &self,
            _: TonicRequest<pb::SearchMemoryRequest>,
        ) -> Result<TonicResponse<pb::SearchMemoryResponse>, Status> {
            unused()
        }

        async fn index_memory(
            &self,
            _: TonicRequest<pb::IndexMemoryRequest>,
        ) -> Result<TonicResponse<pb::IndexMemoryResponse>, Status> {
            unused()
        }

        async fn list_memory(
            &self,
            _: TonicRequest<pb::ListMemoryRequest>,
        ) -> Result<TonicResponse<pb::ListMemoryResponse>, Status> {
            unused()
        }

        async fn delete_memory(
            &self,
            _: TonicRequest<pb::DeleteMemoryRequest>,
        ) -> Result<TonicResponse<pb::DeleteMemoryResponse>, Status> {
            unused()
        }

        async fn health(
            &self,
            _: TonicRequest<pb::MemoryHealthRequest>,
        ) -> Result<TonicResponse<pb::MemoryHealthResponse>, Status> {
            unused()
        }

        async fn get_memory_controls(
            &self,
            _: TonicRequest<pb::GetMemoryControlsRequest>,
        ) -> Result<TonicResponse<pb::GetMemoryControlsResponse>, Status> {
            unused()
        }

        async fn set_memory_controls(
            &self,
            _: TonicRequest<pb::SetMemoryControlsRequest>,
        ) -> Result<TonicResponse<pb::SetMemoryControlsResponse>, Status> {
            unused()
        }

        async fn reset_memory(
            &self,
            _: TonicRequest<pb::ResetMemoryRequest>,
        ) -> Result<TonicResponse<pb::ResetMemoryResponse>, Status> {
            unused()
        }

        async fn get_user_profile(
            &self,
            request: TonicRequest<pb::GetUserProfileRequest>,
        ) -> Result<TonicResponse<pb::UserProfileResponse>, Status> {
            let (org, user) = (request.get_ref().org_id.clone(), request.get_ref().user_id.clone());
            self.record("get", &request, &org, &user);
            Ok(TonicResponse::new(profile_answer()))
        }

        async fn set_user_profile(
            &self,
            request: TonicRequest<pb::SetUserProfileRequest>,
        ) -> Result<TonicResponse<pb::UserProfileResponse>, Status> {
            let (org, user) = (request.get_ref().org_id.clone(), request.get_ref().user_id.clone());
            self.record("set", &request, &org, &user);
            // A ZDR refusal arrives as the catalogue status with its trailers.
            if request.get_ref().content.contains("zdr") {
                return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("user_profile")
                    .grpc_status(tonic::Code::FailedPrecondition));
            }
            if request.get_ref().expected_revision != 3 {
                return Err(Status::failed_precondition("profile_revision_stale"));
            }
            Ok(TonicResponse::new(profile_answer()))
        }

        async fn rebuild_user_profile(
            &self,
            request: TonicRequest<pb::RebuildUserProfileRequest>,
        ) -> Result<TonicResponse<pb::UserProfileResponse>, Status> {
            let (org, user) = (request.get_ref().org_id.clone(), request.get_ref().user_id.clone());
            self.record(if request.get_ref().force { "rebuild_force" } else { "rebuild" }, &request, &org, &user);
            Ok(TonicResponse::new(profile_answer()))
        }
    }

    async fn state_with(memory: ProfileMemory) -> AppState {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind memory stand-in");
        let address = listener.local_addr().expect("memory stand-in address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(pb::memory_service_server::MemoryServiceServer::new(memory))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("memory stand-in endpoint")
            .connect()
            .await
            .expect("connect memory stand-in");
        let mut state = AppState::new();
        state.memory_client = pb::memory_service_client::MemoryServiceClient::new(channel);
        state
    }

    fn claims(org_id: &str, user_id: &str) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: user_id.to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    #[tokio::test]
    async fn profile_routes_take_identity_from_claims() {
        let memory = ProfileMemory::default();
        let state = state_with(memory.clone()).await;
        let bearer = || VerifiedModelBearer::for_test("session-core-token");

        let read = get_user_profile(State(state.clone()), Extension(claims("org-a", "user-a")), bearer())
            .await
            .expect("read");
        assert_eq!(read.0["profile"]["content"], "- Prefers tables");
        assert_eq!(read.0["profile"]["source_memory_ids"], json!(["m-1"]));
        assert_eq!(read.0["profile"]["revision"], 3);
        assert_eq!(read.0["skip_reason"], "");

        // A body naming another user or org cannot reach session-core: the body has no such field.
        let body: SetUserProfileBody = serde_json::from_value(json!({
            "content": "- Prefers prose",
            "expected_revision": 3,
            "user_id": "user-b",
            "org_id": "org-b",
        }))
        .expect("extra fields are ignored");
        set_user_profile(State(state.clone()), Extension(claims("org-a", "user-a")), bearer(), Json(body))
            .await
            .expect("edit");
        rebuild_user_profile(
            State(state.clone()),
            Extension(claims("org-a", "user-a")),
            bearer(),
            Json(RebuildUserProfileBody { force: true }),
        )
        .await
        .expect("rebuild");

        let seen = memory.seen.lock().expect("capture").clone();
        assert_eq!(
            seen.iter().map(|(rpc, _, _, _)| *rpc).collect::<Vec<_>>(),
            ["get", "set", "rebuild_force"]
        );
        for (rpc, org, user, authorization) in &seen {
            assert_eq!((org.as_str(), user.as_str()), ("org-a", "user-a"), "{rpc}");
            assert_eq!(authorization, "Bearer session-core-token", "{rpc}");
        }
    }

    #[tokio::test]
    async fn profile_refusals_keep_their_typed_codes() {
        let state = state_with(ProfileMemory::default()).await;
        let bearer = || VerifiedModelBearer::for_test("session-core-token");
        let edit = |content: &str, expected_revision: u64| SetUserProfileBody {
            content: content.to_owned(),
            expected_revision,
        };

        let stale = set_user_profile(
            State(state.clone()),
            Extension(claims("org-a", "user-a")),
            bearer(),
            Json(edit("- Late", 1)),
        )
        .await
        .expect_err("a stale edit is refused");
        assert_eq!(stale.0, StatusCode::CONFLICT);
        assert_eq!(stale.1 .0["error"]["code"], "profile_revision_stale");

        let zdr = set_user_profile(
            State(state),
            Extension(claims("org-a", "user-a")),
            bearer(),
            Json(edit("- zdr", 3)),
        )
        .await
        .expect_err("a ZDR write is refused");
        assert_eq!(zdr.0, StatusCode::CONFLICT);
        assert_eq!(zdr.1 .0["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(zdr.1 .0["error"]["feature"], "user_profile");
    }
}

#[cfg(test)]
mod model_access_route_tests {
    //! `POST /v1/models/access`: the question reaches inference-core with the verified
    //! caller's organisation and the draft as the BFF sent it, the verdicts carry their
    //! attestation, and an inference-core refusal keeps a typed status.
    use super::*;
    use crate::auth::VerifiedInferenceBearer;
    use mp_contracts::model_entitlement::Refusal;
    use mp_contracts::model_plane::v1 as pb;
    use std::{
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;

    type InferStream =
        Pin<Box<dyn futures::Stream<Item = Result<pb::InferChunk, tonic::Status>> + Send>>;
    type VideoStream = Pin<
        Box<
            dyn futures::Stream<
                    Item = Result<pb::StreamVideoGenerationContentResponse, tonic::Status>,
                > + Send,
        >,
    >;

    /// Inference-core stand-in for the access question. It records every question
    /// and answers with `refusal` when one is set. Otherwise it answers with one
    /// attested chat model that the draft refuses.
    #[derive(Clone, Default)]
    struct AccessInference {
        asked: Arc<Mutex<Vec<pb::GetModelAccessRequest>>>,
        refusal: Arc<Mutex<Option<tonic::Status>>>,
    }

    impl AccessInference {
        fn questions(&self) -> Vec<pb::GetModelAccessRequest> {
            self.asked.lock().expect("question capture").clone()
        }

        fn refuse_with(&self, status: tonic::Status) {
            *self.refusal.lock().expect("refusal setting") = Some(status);
        }
    }

    fn unused<T>() -> Result<tonic::Response<T>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "not used by the model access route tests",
        ))
    }

    #[tonic::async_trait]
    impl pb::inference_core_server::InferenceCore for AccessInference {
        type InferStreamStream = InferStream;
        type StreamVideoGenerationContentStream = VideoStream;

        async fn infer(
            &self,
            _request: tonic::Request<pb::InferRequest>,
        ) -> Result<tonic::Response<pb::InferResponse>, tonic::Status> {
            unused()
        }

        async fn infer_stream(
            &self,
            _request: tonic::Request<pb::InferRequest>,
        ) -> Result<tonic::Response<Self::InferStreamStream>, tonic::Status> {
            unused()
        }

        async fn create_embedding(
            &self,
            _request: tonic::Request<pb::CreateEmbeddingRequest>,
        ) -> Result<tonic::Response<pb::CreateEmbeddingResponse>, tonic::Status> {
            unused()
        }

        async fn get_model_access(
            &self,
            request: tonic::Request<pb::GetModelAccessRequest>,
        ) -> Result<tonic::Response<pb::GetModelAccessResponse>, tonic::Status> {
            self.asked
                .lock()
                .expect("question capture")
                .push(request.into_inner());
            if let Some(status) = self.refusal.lock().expect("refusal setting").clone() {
                return Err(status);
            }
            Ok(tonic::Response::new(pb::GetModelAccessResponse {
                state: "role".to_owned(),
                enforced: true,
                revision: 7,
                role: "member".to_owned(),
                restricted: true,
                model_allowed: false,
                reason: "not_in_allow_list".to_owned(),
                min_privacy_tier: pb::PrivacyTier::Sovereign as i32,
                can_manage: true,
                catalogue: vec![pb::ModelAccessEntry {
                    id: "gpt-attested".to_owned(),
                    provider: "azure".to_owned(),
                    entitled: false,
                    reason: "not_in_allow_list".to_owned(),
                    zdr_attested: true,
                }],
                ..Default::default()
            }))
        }

        async fn list_models(
            &self,
            _request: tonic::Request<pb::ListModelsRequest>,
        ) -> Result<tonic::Response<pb::ListModelsResponse>, tonic::Status> {
            unused()
        }

        async fn synthesize_speech(
            &self,
            _request: tonic::Request<pb::SynthesizeSpeechRequest>,
        ) -> Result<tonic::Response<pb::SynthesizeSpeechResponse>, tonic::Status> {
            unused()
        }

        async fn transcribe_speech(
            &self,
            _request: tonic::Request<pb::TranscribeSpeechRequest>,
        ) -> Result<tonic::Response<pb::TranscribeSpeechResponse>, tonic::Status> {
            unused()
        }

        async fn list_speech_voices(
            &self,
            _request: tonic::Request<pb::ListSpeechVoicesRequest>,
        ) -> Result<tonic::Response<pb::ListSpeechVoicesResponse>, tonic::Status> {
            unused()
        }

        async fn translate_text(
            &self,
            _request: tonic::Request<pb::TranslateTextRequest>,
        ) -> Result<tonic::Response<pb::TranslateTextResponse>, tonic::Status> {
            unused()
        }

        async fn batch_translate_text(
            &self,
            _request: tonic::Request<pb::BatchTranslateTextRequest>,
        ) -> Result<tonic::Response<pb::BatchTranslateTextResponse>, tonic::Status> {
            unused()
        }

        async fn detect_text_language(
            &self,
            _request: tonic::Request<pb::DetectTextLanguageRequest>,
        ) -> Result<tonic::Response<pb::DetectTextLanguageResponse>, tonic::Status> {
            unused()
        }

        async fn list_translation_languages(
            &self,
            _request: tonic::Request<pb::ListTranslationLanguagesRequest>,
        ) -> Result<tonic::Response<pb::ListTranslationLanguagesResponse>, tonic::Status> {
            unused()
        }

        async fn generate_image(
            &self,
            _request: tonic::Request<pb::GenerateImageRequest>,
        ) -> Result<tonic::Response<pb::GenerateImageResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_image(
            &self,
            _request: tonic::Request<pb::AnalyzeImageRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeImageResponse>, tonic::Status> {
            unused()
        }

        async fn extract_image_text(
            &self,
            _request: tonic::Request<pb::ExtractImageTextRequest>,
        ) -> Result<tonic::Response<pb::ExtractImageTextResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_document(
            &self,
            _request: tonic::Request<pb::AnalyzeDocumentRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeDocumentResponse>, tonic::Status> {
            unused()
        }

        async fn analyze_language(
            &self,
            _request: tonic::Request<pb::AnalyzeLanguageRequest>,
        ) -> Result<tonic::Response<pb::AnalyzeLanguageResponse>, tonic::Status> {
            unused()
        }

        async fn create_realtime_session(
            &self,
            _request: tonic::Request<pb::CreateRealtimeSessionRequest>,
        ) -> Result<tonic::Response<pb::CreateRealtimeSessionResponse>, tonic::Status> {
            unused()
        }

        async fn create_video_generation_job(
            &self,
            _request: tonic::Request<pb::CreateVideoGenerationJobRequest>,
        ) -> Result<tonic::Response<pb::CreateVideoGenerationJobResponse>, tonic::Status> {
            unused()
        }

        async fn get_video_generation_job(
            &self,
            _request: tonic::Request<pb::GetVideoGenerationJobRequest>,
        ) -> Result<tonic::Response<pb::GetVideoGenerationJobResponse>, tonic::Status> {
            unused()
        }

        async fn stream_video_generation_content(
            &self,
            _request: tonic::Request<pb::StreamVideoGenerationContentRequest>,
        ) -> Result<tonic::Response<Self::StreamVideoGenerationContentStream>, tonic::Status>
        {
            unused()
        }
    }

    /// An `AppState` whose inference client talks to `inference`.
    async fn state_with(inference: AccessInference) -> AppState {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference stand-in");
        let address = listener.local_addr().expect("inference stand-in address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(pb::inference_core_server::InferenceCoreServer::new(
                    inference,
                ))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("inference stand-in endpoint")
            .connect()
            .await
            .expect("connect inference stand-in");
        let mut state = AppState::new();
        state.inference_client = pb::inference_core_client::InferenceCoreClient::new(channel);
        state
    }

    fn claims(org_id: &str) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: org_id.to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn bearer() -> VerifiedInferenceBearer {
        VerifiedInferenceBearer::for_test("inference-test-bearer")
    }

    /// The body the Frontend BFF sends: a draft that allows one open model, with
    /// the catalogue asked for.
    fn bff_question() -> Value {
        json!({
            "draft": {
                "allow_mode": "listed",
                "allow": ["gpt-open"],
                "min_privacy_tier": "sovereign",
                "defaults": {"chat": {"model": "gpt-open", "effort": "low"}},
            },
            "include_catalogue": true,
        })
    }

    #[tokio::test]
    async fn the_question_reaches_inference_core_with_the_callers_org_and_the_draft_as_sent() {
        let access = AccessInference::default();
        let state = state_with(access.clone()).await;

        let Json(answer) = post_model_access(
            State(state),
            Extension(claims("org-a")),
            bearer(),
            Json(bff_question()),
        )
        .await
        .expect("the answer");

        let asked = access.questions();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].org_id, "org-a");
        assert!(asked[0].include_catalogue);
        let draft = asked[0].draft.as_ref().expect("the draft");
        assert_eq!(draft.allow, ["gpt-open"]);
        assert_eq!(draft.min_privacy_tier, pb::PrivacyTier::Sovereign as i32);
        assert_eq!(draft.defaults["chat"].model, "gpt-open");

        assert_eq!(answer["model_allowed"], false);
        assert_eq!(answer["min_privacy_tier"], "sovereign");
        assert_eq!(answer["catalogue"][0]["id"], "gpt-attested");
        assert_eq!(answer["catalogue"][0]["entitled"], false);
        assert_eq!(answer["catalogue"][0]["zdr_attested"], true);
    }

    #[tokio::test]
    async fn a_body_that_names_an_organisation_never_reaches_inference_core() {
        let access = AccessInference::default();
        let state = state_with(access.clone()).await;

        let (status, Json(body)) = post_model_access(
            State(state),
            Extension(claims("org-a")),
            bearer(),
            Json(json!({"org_id": "org-b", "include_catalogue": true})),
        )
        .await
        .expect_err("a body that names an organisation is refused");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_model_access_request");
        assert!(access.questions().is_empty());
    }

    #[tokio::test]
    async fn inference_core_refusals_keep_their_typed_status_and_echo_no_upstream_detail() {
        let access = AccessInference::default();
        let state = state_with(access.clone()).await;

        // The policy cannot be read: the catalogue's 503, typed.
        access.refuse_with(Refusal::unavailable().grpc_status());
        let (status, Json(body)) = post_model_access(
            State(state.clone()),
            Extension(claims("org-a")),
            bearer(),
            Json(bff_question()),
        )
        .await
        .expect_err("an unreadable policy is refused");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "model_entitlement_unavailable");

        // A caller who may not manage the policy asked for the draft: a typed 403.
        access.refuse_with(tonic::Status::permission_denied(
            "the model policy can be read in full only by those who manage it",
        ));
        let (status, Json(body)) = post_model_access(
            State(state.clone()),
            Extension(claims("org-a")),
            bearer(),
            Json(bff_question()),
        )
        .await
        .expect_err("a member may not ask for a draft");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "model_access_forbidden");
        assert!(!body.to_string().contains("only by those who manage"));

        // Anything else is a typed 502 that names no upstream detail.
        access.refuse_with(tonic::Status::unavailable(
            "inference-core restarting at node-3",
        ));
        let (status, Json(body)) = post_model_access(
            State(state),
            Extension(claims("org-a")),
            bearer(),
            Json(bff_question()),
        )
        .await
        .expect_err("an unanswered question is refused");
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "model_access_unavailable");
        assert!(!body.to_string().contains("node-3"));
    }
}

#[cfg(test)]
mod thread_messages_page_tests {
    use super::*;

    #[test]
    fn the_page_ceiling_is_a_whole_number_from_one_to_two_hundred() {
        assert_eq!(thread_messages_page_ceiling(None), 200);
        assert_eq!(thread_messages_page_ceiling(Some("50")), 50);
        assert_eq!(thread_messages_page_ceiling(Some(" 1 ")), 1);
        assert_eq!(thread_messages_page_ceiling(Some("200")), 200);
        assert_eq!(thread_messages_page_ceiling(Some("0")), 200);
        assert_eq!(thread_messages_page_ceiling(Some("201")), 200);
        assert_eq!(thread_messages_page_ceiling(Some("fifty")), 200);
        assert_eq!(thread_messages_page_ceiling(Some("")), 200);
    }

    #[test]
    fn a_cursor_is_a_bounded_message_id_and_nothing_else() {
        assert!(is_thread_message_cursor("01JABCDEFGHJKMNPQRSTVWXYZ0"));
        assert!(is_thread_message_cursor("msg_01-X"));
        assert!(!is_thread_message_cursor(""));
        assert!(!is_thread_message_cursor(&"a".repeat(129)));
        assert!(!is_thread_message_cursor("abc def"));
        assert!(!is_thread_message_cursor("a&limit=1"));
        assert!(!is_thread_message_cursor("../etc"));
    }

    #[test]
    fn an_unpaged_body_has_no_has_more_key() {
        let body = serde_json::to_value(ListThreadMessagesResponse {
            thread_id: "thread-1".to_owned(),
            messages: Vec::new(),
            has_more: None,
            omitted_before: None,
            versions_enabled: None,
        })
        .expect("the response serialises");
        assert_eq!(
            body,
            serde_json::json!({"thread_id": "thread-1", "messages": []})
        );
    }

    #[test]
    fn a_paged_body_always_says_whether_older_turns_exist() {
        let body = serde_json::to_value(ListThreadMessagesResponse {
            thread_id: "thread-1".to_owned(),
            messages: Vec::new(),
            has_more: Some(false),
            omitted_before: None,
            versions_enabled: None,
        })
        .expect("the response serialises");
        assert_eq!(body["has_more"], serde_json::json!(false));
    }

    #[test]
    fn a_transcript_without_versions_keeps_its_shape() {
        // The version fields appear only with a versioned thread or the switch on, so an
        // unversioned transcript is the same bytes it was before the message tree.
        let body = serde_json::to_value(ListThreadMessagesResponse {
            thread_id: "thread-1".to_owned(),
            messages: vec![ThreadMessage {
                message_id: "m1".to_owned(),
                role: "user".to_owned(),
                content: "hi".to_owned(),
                agent_name: String::new(),
                author_subject_id: String::new(),
                post_kind: String::new(),
                created_at: None,
                metadata: None,
                version: None,
                entry: None,
                mentions: Vec::new(),
                revision: None,
            }],
            has_more: None,
            omitted_before: None,
            versions_enabled: None,
        })
        .expect("the response serialises");
        assert_eq!(
            body,
            serde_json::json!({
                "thread_id": "thread-1",
                "messages": [{"message_id": "m1", "role": "user", "content": "hi"}],
            })
        );
    }

    #[test]
    fn a_versioned_turn_says_where_it_sits_among_its_versions() {
        let body = serde_json::to_value(ThreadMessage {
            message_id: "m2".to_owned(),
            role: "assistant".to_owned(),
            content: "answer".to_owned(),
            entry: None,
            mentions: Vec::new(),
            revision: None,
            agent_name: String::new(),
            author_subject_id: String::new(),
            post_kind: String::new(),
            created_at: None,
            metadata: None,
            version: Some(ThreadMessageVersion {
                siblings: Vec::new(),
                parent_message_id: "m1".to_owned(),
                sibling_ordinal: 2,
                sibling_count: 3,
                on_active_path: false,
            }),
        })
        .expect("the turn serialises");
        assert_eq!(
            body["version"],
            serde_json::json!({
                "parent_message_id": "m1",
                "sibling_ordinal": 2,
                "sibling_count": 3,
                "on_active_path": false,
            })
        );
    }

    #[test]
    fn a_branch_refusal_maps_to_the_code_the_client_acts_on() {
        let stale = branch_thread_error(&tonic::Status::aborted("active_leaf_stale"));
        assert_eq!(stale.0, StatusCode::CONFLICT);
        assert_eq!(stale.1 .0["error"]["code"], "active_leaf_stale");

        let disabled =
            branch_thread_error(&tonic::Status::unavailable("message_versions_disabled"));
        assert_eq!(disabled.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(disabled.1 .0["error"]["code"], "history_versions_disabled");

        let busy = branch_thread_error(&tonic::Status::failed_precondition(
            "active_run_blocks_switch",
        ));
        assert_eq!(busy.0, StatusCode::CONFLICT);
        assert_eq!(busy.1 .0["error"]["code"], "history_branch_refused");
    }

    #[test]
    fn the_gateway_reports_versions_only_when_its_switch_is_on() {
        let body = serde_json::to_value(ListThreadMessagesResponse {
            thread_id: "thread-1".to_owned(),
            messages: Vec::new(),
            has_more: None,
            omitted_before: None,
            versions_enabled: Some(true),
        })
        .expect("the response serialises");
        assert_eq!(body["versions_enabled"], serde_json::json!(true));
    }

    #[test]
    fn a_versioned_turn_lists_the_turns_sharing_its_parent_in_order() {
        let turn = |id: &str, parent: &str, ordinal: u32| mp_contracts::model_plane::v1::SessionMessage {
            message_id: id.to_owned(),
            parent_message_id: parent.to_owned(),
            sibling_ordinal: ordinal,
            ..Default::default()
        };
        let by_parent = sibling_ids_by_parent(&[
            turn("a2", "u1", 2),
            turn("a1", "u1", 1),
            turn("a3", "u1", 3),
            turn("u2", "a1", 1),
        ]);
        assert_eq!(by_parent["u1"], vec!["a1", "a2", "a3"]);
        assert_eq!(by_parent["a1"], vec!["u2"]);
    }
}

#[cfg(test)]
mod run_replay_projection_tests {
    use super::*;

    fn event(
        event_type: i32,
        resource_ref: &str,
        correlation_id: &str,
        type_url: &str,
        payload: &str,
    ) -> Event {
        Event {
            event_id: "evt-1".to_owned(),
            event_type,
            resource_ref: resource_ref.to_owned(),
            correlation_id: correlation_id.to_owned(),
            payload: Some(prost_types::Any {
                type_url: type_url.to_owned(),
                value: payload.as_bytes().to_vec(),
            }),
            ..Default::default()
        }
    }

    const RUN_TERMINAL: &str = "type.googleapis.com/model_plane.v1.RunTerminal";

    #[test]
    fn a_managed_run_failed_is_projected_exactly() {
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::RunFailed as i32,
                "run:run-1",
                "run-1",
                RUN_TERMINAL,
                r#"{"failure_code":"browser_permission_denied"}"#,
            ),
        )
        .expect("the run's own RUN_FAILED is replayed");
        assert_eq!(projected.event_type, "EVENT_TYPE_RUN_FAILED");
        assert_eq!(projected.event_id, "evt-1");
        assert_eq!(
            projected.payload,
            json!({
                "run_id": "run-1",
                "terminal": true,
                "status": "failed",
                "failure_code": "browser_permission_denied",
            })
        );
    }

    #[test]
    fn a_legacy_run_failed_never_carries_its_error_text() {
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::RunFailed as i32,
                "run:run-1",
                "",
                RUN_TERMINAL,
                r#"{"error":"permission denied by policy at https://secret.example/path"}"#,
            ),
        )
        .expect("bound by resource_ref");
        assert_eq!(
            projected.payload,
            json!({ "run_id": "run-1", "terminal": true, "status": "failed" })
        );
    }

    #[test]
    fn a_run_completed_is_projected_without_a_failure_code() {
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::RunCompleted as i32,
                "",
                "run-1",
                RUN_TERMINAL,
                "{}",
            ),
        )
        .expect("bound by correlation_id");
        assert_eq!(projected.event_type, "EVENT_TYPE_RUN_COMPLETED");
        assert_eq!(
            projected.payload,
            json!({ "run_id": "run-1", "terminal": true, "status": "completed" })
        );
    }

    #[test]
    fn a_run_cancelled_is_found_by_its_type_url_and_leaks_nothing() {
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::Unspecified as i32,
                "run:run-1",
                "run-1",
                RUN_CANCELLED_TYPE_URL,
                r#"{"run_id":"run-1","reason":"user said: stop https://x.example","cancelled_by":"user-7"}"#,
            ),
        )
        .expect("RUN_CANCELLED is replayed");
        assert_eq!(projected.event_type, "EVENT_TYPE_RUN_CANCELLED");
        assert_eq!(
            projected.payload,
            json!({ "run_id": "run-1", "terminal": true, "status": "cancelled" })
        );
    }

    #[test]
    fn another_runs_terminal_event_is_excluded() {
        for (kind, type_url) in [
            (EventType::RunFailed as i32, RUN_TERMINAL),
            (EventType::RunCompleted as i32, RUN_TERMINAL),
            (EventType::Unspecified as i32, RUN_CANCELLED_TYPE_URL),
        ] {
            // The payload naming run-1 does not bind it: only the envelope does.
            let other = event(
                kind,
                "run:run-2",
                "run-2",
                type_url,
                r#"{"run_id":"run-1"}"#,
            );
            assert!(project_run_replay_event("run-1", other).is_none(), "{kind}");
        }
        // A prefix of the requested id is not the run either.
        let prefix = event(
            EventType::RunFailed as i32,
            "run:run-10",
            "run-10",
            RUN_TERMINAL,
            "{}",
        );
        assert!(project_run_replay_event("run-1", prefix).is_none());
    }

    #[test]
    fn an_attach_shared_jar_event_is_replayed_for_its_run() {
        let payload = r#"{"run_id":"run-1","plan_id":"plan-1","attach_id":"att-1","session_run_id":"user-run-9","tab_id":"tab-3"}"#;
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::BrowserAttachSharedJar as i32,
                "run:run-1",
                "run-1",
                "type.googleapis.com/model_plane.v1.BrowserAttachSharedJar",
                payload,
            ),
        )
        .expect("a reconnecting client replays the attach audit event");
        assert_eq!(projected.event_type, "EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR");
        assert_eq!(projected.payload["attach_id"], "att-1");
        // Another run's attach event is not replayed.
        let other = event(
            EventType::BrowserAttachSharedJar as i32,
            "run:run-2",
            "run-2",
            "type.googleapis.com/model_plane.v1.BrowserAttachSharedJar",
            r#"{"run_id":"run-2"}"#,
        );
        assert!(project_run_replay_event("run-1", other).is_none());
    }

    #[test]
    fn a_non_slug_failure_code_is_dropped() {
        for code in [
            "Browser_failed",
            "browser failed",
            "1browser",
            "browser-failed",
            "",
            "browser_failed: https://secret.example",
        ] {
            let payload = json!({ "failure_code": code }).to_string();
            let projected = project_run_replay_event(
                "run-1",
                event(
                    EventType::RunFailed as i32,
                    "run:run-1",
                    "run-1",
                    RUN_TERMINAL,
                    &payload,
                ),
            )
            .expect("the terminal marker is still replayed");
            assert!(projected.payload.get("failure_code").is_none(), "{code:?}");
            assert_eq!(projected.payload["status"], "failed");
        }
        let long = "a".repeat(65);
        assert!(!is_failure_code_slug(&long));
        assert!(is_failure_code_slug(&"a".repeat(64)));
        assert!(is_failure_code_slug("browser_timed_out"));
    }

    #[test]
    fn browser_events_are_unchanged() {
        let payload = r#"{"run_id":"run-1","action":"navigate","decision":"granted"}"#;
        let projected = project_run_replay_event(
            "run-1",
            event(
                EventType::BrowserActionDispatched as i32,
                "",
                "",
                "type.googleapis.com/model_plane.v1.BrowserActionDispatched",
                payload,
            ),
        )
        .expect("a browser event naming the run is replayed");
        assert_eq!(projected.event_type, "EVENT_TYPE_BROWSER_ACTION_DISPATCHED");
        assert_eq!(
            projected.payload,
            serde_json::from_str::<Value>(payload).unwrap()
        );

        let other_run = event(
            EventType::BrowserActionDispatched as i32,
            "run:run-1",
            "run-1",
            "",
            r#"{"run_id":"run-2"}"#,
        );
        assert!(project_run_replay_event("run-1", other_run).is_none());

        let unrelated = event(
            EventType::RunStarted as i32,
            "run:run-1",
            "run-1",
            "",
            r#"{"run_id":"run-1"}"#,
        );
        assert!(project_run_replay_event("run-1", unrelated).is_none());
    }
}

#[cfg(test)]
mod fork_route_tests {
    use super::*;

    #[test]
    fn fork_body_reads_camel_case_defaults_the_title_and_refuses_unknown_fields() {
        let body: ForkThreadBody =
            serde_json::from_value(json!({"fromMessageId": "turn-4"})).unwrap();
        assert_eq!(body.from_message_id, "turn-4");
        assert!(body.title.is_empty());
        assert!(serde_json::from_value::<ForkThreadBody>(
            json!({"fromMessageId": "turn-4", "copyEverything": true})
        )
        .is_err());
    }

    #[test]
    fn fork_refusals_answer_typed_codes() {
        let (status, Json(body)) = fork_thread_error(&tonic::Status::unavailable(
            "thread_fork_disabled",
        ));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "history_fork_disabled");

        let (status, Json(body)) = fork_thread_error(&tonic::Status::failed_precondition(
            "fork_source_not_forkable",
        ));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "history_fork_refused");
        assert_eq!(body["error"]["message"], "fork_source_not_forkable");
    }
}

#[cfg(test)]
mod space_post_wire_tests {
    use super::*;

    fn message(post_kind: &str) -> ThreadMessage {
        ThreadMessage {
            message_id: "m-1".to_owned(),
            role: "user".to_owned(),
            content: "hello".to_owned(),
            agent_name: String::new(),
            author_subject_id: "user-2".to_owned(),
            post_kind: post_kind.to_owned(),
            created_at: None,
            metadata: None,
            version: None,
            entry: None,
            mentions: Vec::new(),
            revision: None,
        }
    }

    fn summary(reply_count: u32, last_author: &str) -> ThreadSummaryResponse {
        ThreadSummaryResponse {
            attention_state: None, attention_since: None, attention_unseen: None, forked_from_thread_id: String::new(),
            thread_id: "t-1".to_owned(),
            session_key: "k".to_owned(),
            title: "title".to_owned(),
            preview: "preview".to_owned(),
            created_at: "2026-10-27T00:00:00Z".to_owned(),
            updated_at: "2026-10-27T00:00:00Z".to_owned(),
            pinned: false,
            space_id: "space-1".to_owned(),
            owner_subject_id: "user-1".to_owned(),
            latest_run_id: String::new(),
            latest_run_status: String::new(),
            latest_run_updated_at: None,
            origin: "space".to_owned(),
            persona_enabled: true,
            archived_at: String::new(),
            starred: false,
            unread: false,
            reply_count,
            last_author_subject_id: last_author.to_owned(),
            last_message_seq: 0,
            recent_author_subject_ids: Vec::new(),
            last_role: String::new(),
            turn_count: 0,
        }
    }

    #[test]
    fn thread_message_json_carries_created_at_as_rfc3339_and_omits_it_when_unset() {
        // 2026-10-31T09:30:15.250Z
        let stored = prost_types::Timestamp {
            seconds: 1_793_439_015,
            nanos: 250_000_000,
        };
        let mut stamped = message("post");
        stamped.created_at = message_created_at(Some(&stored));
        let body = serde_json::to_value(stamped).expect("serialises");
        let created_at = body["created_at"].as_str().expect("created_at is a string");
        let parsed = chrono::DateTime::parse_from_rfc3339(created_at).expect("RFC3339");
        assert_eq!(parsed.timestamp(), 1_793_439_015);
        assert_eq!(parsed.timestamp_subsec_millis(), 250);
        // A row session-core sent no time for keeps the shape it always had.
        assert_eq!(message_created_at(None), None);
        let unset = serde_json::to_value(message("")).expect("serialises");
        assert!(unset.get("created_at").is_none(), "{unset}");
    }

    #[test]
    fn thread_message_json_omits_post_kind_for_turns_and_carries_it_for_posts() {
        // The handler maps a turn (or a row from before post kinds) to an empty kind, which is
        // omitted, so a turn keeps the shape it always had.
        let turn = serde_json::to_value(message("")).expect("serialises");
        assert!(turn.get("post_kind").is_none(), "{turn}");
        assert_eq!(turn["author_subject_id"], "user-2");
        let post = serde_json::to_value(message("post")).expect("serialises");
        assert_eq!(post["post_kind"], "post");
        assert_eq!(post["author_subject_id"], "user-2");
        assert_eq!(post["message_id"], "m-1");
    }

    #[test]
    fn thread_summary_omits_reply_count_when_zero_and_last_author_when_empty() {
        let quiet = serde_json::to_value(summary(0, "")).expect("serialises");
        assert!(quiet.get("reply_count").is_none(), "{quiet}");
        assert!(quiet.get("last_author_subject_id").is_none(), "{quiet}");
        let busy = serde_json::to_value(summary(3, "user-2")).expect("serialises");
        assert_eq!(busy["reply_count"], 3);
        assert_eq!(busy["last_author_subject_id"], "user-2");
        // One of the two alone is carried independently of the other.
        let only_author = serde_json::to_value(summary(0, "user-9")).expect("serialises");
        assert!(only_author.get("reply_count").is_none());
        assert_eq!(only_author["last_author_subject_id"], "user-9");
        let only_count = serde_json::to_value(summary(1, "")).expect("serialises");
        assert_eq!(only_count["reply_count"], 1);
        assert!(only_count.get("last_author_subject_id").is_none());
    }
}

#[cfg(test)]
mod memory_withheld_json_tests {
    use super::*;

    fn summary(withheld_reason: &str, origin_space_ref: &str) -> MemorySummaryResponse {
        MemorySummaryResponse {
            memory_id: "mem-1".to_owned(),
            topic: "USER".to_owned(),
            content: "likes tea".to_owned(),
            updated_at: "2026-10-27T12:00:00Z".to_owned(),
            provenance: "unknown",
            withheld_reason: withheld_reason.to_owned(),
            origin_space_ref: origin_space_ref.to_owned(),
        }
    }

    #[test]
    fn memories_json_omits_withheld_fields_for_an_ordinary_row() {
        let ordinary = serde_json::to_value(summary("", "")).expect("serialises");
        assert!(ordinary.get("withheld_reason").is_none(), "{ordinary}");
        assert!(ordinary.get("origin_space_ref").is_none(), "{ordinary}");
        // The shape an older SPA reads is unchanged.
        for key in ["memory_id", "topic", "content", "updated_at", "provenance"] {
            assert!(ordinary.get(key).is_some(), "{key} must stay: {ordinary}");
        }
    }

    #[test]
    fn memories_json_carries_withheld_reason_and_origin_space_when_set() {
        let withheld = serde_json::to_value(summary("room_origin_unverified", "space-room-1")).expect("serialises");
        assert_eq!(withheld["withheld_reason"], "room_origin_unverified");
        assert_eq!(withheld["origin_space_ref"], "space-room-1");
    }

    #[test]
    fn memories_json_carries_each_withheld_field_independently() {
        let only_reason = serde_json::to_value(summary("room_origin_unverified", "")).expect("serialises");
        assert_eq!(only_reason["withheld_reason"], "room_origin_unverified");
        assert!(only_reason.get("origin_space_ref").is_none());
        let only_space = serde_json::to_value(summary("", "space-room-1")).expect("serialises");
        assert!(only_space.get("withheld_reason").is_none());
        assert_eq!(only_space["origin_space_ref"], "space-room-1");
    }

    #[test]
    fn the_listing_envelope_still_serialises_with_withheld_rows_inside() {
        let body = serde_json::to_value(ListMemoriesResponse {
            memories: vec![summary("room_origin_unverified", "space-room-1"), summary("", "")],
            degraded: false,
            degradation_reason: String::new(),
        })
        .expect("serialises");
        let rows = body["memories"].as_array().expect("array");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["withheld_reason"], "room_origin_unverified");
        assert!(rows[1].get("withheld_reason").is_none());
    }
}

#[cfg(test)]
mod ai_chat_lane_tests {
    //! WS16.S1: `POST /v1/ai/chat` as a machine route. The handler runs against a real inference
    //! stand-in (the speech tests' gRPC server, which records every `InferRequest`), so what these
    //! tests assert is what inference-core would have received and what the bus would have carried.
    use super::speech_route_tests::{bearer, state_with, SpeechInference};
    use super::*;
    use mp_contracts::model_plane::v1 as pb;

    fn user(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn service(sub: &str, zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: sub.to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: String::new(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: vec!["models:invoke".to_owned()],
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("service".to_owned()),
            service_id: Some(sub.to_owned()),
            reason: Some("extract structured data from page text".to_owned()),
        }
    }

    fn quarry(zdr: bool) -> Claims {
        service("service:quarry-edge", zdr)
    }

    fn request(extra: Value) -> AiChatRequest {
        let mut body = json!({
            "messages": [{ "role": "user", "content": "extract the title" }],
            "model": "test-model",
        });
        for (key, value) in extra.as_object().expect("extra fields are an object") {
            body[key] = value.clone();
        }
        serde_json::from_value(body).expect("an ai_chat body parses")
    }

    fn reply() -> pb::InferResponse {
        pb::InferResponse {
            content: "{\"title\":\"Hello\"}".to_owned(),
            model_used: "test-model".to_owned(),
            provider_used: "azure".to_owned(),
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        }
    }

    async fn call(
        inference: &SpeechInference,
        claims: Claims,
        extra: Value,
        metering: AiChatMetering,
    ) -> (AppState, Result<Json<Value>, HttpJsonError>) {
        let mut state = state_with(inference.clone()).await;
        state.pricing = crate::pricing::PricingCache::with_rate_for_test("test-model", 1.0, 2.0);
        let outcome = ai_chat_with(state.clone(), claims, bearer(), request(extra), metering).await;
        (state, outcome)
    }

    #[tokio::test]
    async fn ai_chat_defaults_are_unchanged_without_new_fields() {
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (_, outcome) = call(&inference, user(false), json!({}), AiChatMetering::Off).await;
        let Json(body) = outcome.expect("a plain chat call succeeds");

        let sent = inference.cleanups();
        assert_eq!(sent.len(), 1);
        assert!((sent[0].temperature - 0.7).abs() < f32::EPSILON, "{}", sent[0].temperature);
        assert_eq!(sent[0].max_tokens, 4096);
        // The keys every caller already read are all still there.
        assert_eq!(body["content"], "{\"title\":\"Hello\"}");
        assert_eq!(body["model_used"], "test-model");
        assert_eq!(body["usage"]["input_tokens"], 1000);
        assert_eq!(body["usage"]["output_tokens"], 500);
        assert!(body["id"].as_str().is_some_and(|id| !id.is_empty()));
    }

    #[tokio::test]
    async fn ai_chat_accepts_the_bounds_and_passes_them_through() {
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (_, outcome) = call(
            &inference,
            user(false),
            json!({ "temperature": 0.0, "max_tokens": 8192 }),
            AiChatMetering::Off,
        )
        .await;
        let _ = outcome.expect("the inclusive bounds are accepted");
        let sent = inference.cleanups();
        assert!(sent[0].temperature.abs() < f32::EPSILON);
        assert_eq!(sent[0].max_tokens, 8192);
    }

    #[tokio::test]
    async fn ai_chat_rejects_temperature_above_one_and_max_tokens_above_8192() {
        for extra in [
            json!({ "temperature": 1.01 }),
            json!({ "temperature": -0.1 }),
            json!({ "max_tokens": 8193 }),
            json!({ "max_tokens": 0 }),
            json!({ "max_tokens": -5 }),
        ] {
            let inference = SpeechInference::default();
            let (_, outcome) = call(&inference, user(false), extra.clone(), AiChatMetering::Off).await;
            let (status, _) = outcome.expect_err("an out-of-range field is refused");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{extra}");
            assert!(inference.cleanups().is_empty(), "{extra}: no model call");
        }
    }

    #[tokio::test]
    async fn ai_chat_returns_cost_and_provider() {
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (_, outcome) = call(&inference, quarry(false), json!({}), AiChatMetering::Off).await;
        let Json(body) = outcome.expect("a service call succeeds");
        assert_eq!(body["provider_used"], "azure");
        // 1000 input tokens at 1.0 per million plus 500 output tokens at 2.0 per million.
        let cost = body["usage"]["cost_usd"].as_f64().expect("a metered call is priced");
        assert!((cost - 0.002).abs() < 1e-9, "{cost}");
        assert!(body["usage"]["latency_ms"].is_u64(), "{body}");

        // Under ZDR the cost is null, never a number.
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (_, outcome) = call(&inference, quarry(true), json!({}), AiChatMetering::Service).await;
        let Json(body) = outcome.expect("a ZDR service call succeeds");
        assert!(body["usage"]["cost_usd"].is_null(), "{body}");
        assert_eq!(body["provider_used"], "azure");
        assert!(inference.cleanups()[0].zdr, "the ZDR flag reaches inference-core");
    }

    #[tokio::test]
    async fn a_service_call_publishes_one_usage_envelope_and_a_zdr_call_none() {
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (state, outcome) = call(
            &inference,
            quarry(false),
            json!({ "purpose": "quarry.extract" }),
            AiChatMetering::Service,
        )
        .await;
        let Json(body) = outcome.expect("the call succeeds");
        let published = state.publisher.drain();
        assert_eq!(published.len(), 1, "{published:?}");
        let (subject, envelope) = &published[0];
        assert_eq!(subject, &subjects::usage_subject("org-a"));
        assert_eq!(envelope.event_type, "USAGE_ENVELOPE");
        assert_eq!(envelope.producer, "model-gateway");
        assert_eq!(envelope.org_id, "org-a");
        assert_eq!(envelope.user_id, "service:quarry-edge");
        let id = body["id"].as_str().expect("the response id");
        assert_eq!(envelope.idempotency_key, format!("{id}-USAGE_ENVELOPE"));
        assert!(!envelope.zdr);
        assert_eq!(envelope.payload["route"], "ai_chat");
        assert_eq!(envelope.payload["purpose"], "quarry.extract");
        assert_eq!(envelope.payload["input_tokens"], 1000);
        assert_eq!(envelope.payload["output_tokens"], 500);
        assert_eq!(envelope.payload["provider_used"], "azure");
        assert_eq!(envelope.payload["user_id"], "service:quarry-edge");

        // A ZDR call publishes nothing at all.
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (state, outcome) = call(
            &inference,
            quarry(true),
            json!({ "purpose": "quarry.extract" }),
            AiChatMetering::Service,
        )
        .await;
        let _ = outcome.expect("the ZDR call succeeds");
        assert!(state.publisher.drain().is_empty(), "a ZDR call is never ledgered");
    }

    #[tokio::test]
    async fn a_user_call_publishes_nothing_in_service_mode() {
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (state, outcome) = call(&inference, user(false), json!({}), AiChatMetering::Service).await;
        let _ = outcome.expect("the user call succeeds");
        assert!(state.publisher.drain().is_empty());

        // `all` is the only mode that meters a user, and it attributes the user.
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (state, outcome) = call(&inference, user(false), json!({}), AiChatMetering::All).await;
        let _ = outcome.expect("the user call succeeds");
        let published = state.publisher.drain();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].1.user_id, "user-a");
        assert!(published[0].1.payload["purpose"].is_null());
    }

    /// New behaviour is off until switched on: no switch, an unknown value and `off` all meter nothing.
    #[tokio::test]
    async fn metering_defaults_to_off_and_only_the_literal_modes_enable_it() {
        assert_eq!(AiChatMetering::parse(None), AiChatMetering::Off);
        assert_eq!(AiChatMetering::parse(Some("")), AiChatMetering::Off);
        assert_eq!(AiChatMetering::parse(Some("1")), AiChatMetering::Off);
        assert_eq!(AiChatMetering::parse(Some("off")), AiChatMetering::Off);
        assert_eq!(AiChatMetering::parse(Some("service")), AiChatMetering::Service);
        assert_eq!(AiChatMetering::parse(Some(" ALL ")), AiChatMetering::All);

        let inference = SpeechInference::default();
        inference.answering(reply());
        let (state, outcome) = call(&inference, quarry(false), json!({}), AiChatMetering::default()).await;
        let _ = outcome.expect("the call succeeds");
        assert!(state.publisher.drain().is_empty(), "a service call is unmetered by default");
    }

    const CONTRACT_DIR: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/contracts/ai-chat-v1"
    );

    fn contract(name: &str) -> String {
        std::fs::read_to_string(format!("{CONTRACT_DIR}/{name}"))
            .unwrap_or_else(|error| panic!("cannot read ai-chat-v1/{name}: {error}"))
    }

    /// The `ai-chat-v1` fixtures are what a caller's fake gateway replays (Quarry vendors them), so
    /// they must be what this handler speaks: the request parses, with its schema as a STRING; the
    /// ok response has exactly the keys the handler writes; the 412 is the catalogue's own body; and
    /// the pin file matches the bytes.
    #[tokio::test]
    async fn the_ai_chat_v1_fixtures_are_what_the_handler_speaks() {
        let request_text = contract("request.ok.json");
        let request: AiChatRequest = serde_json::from_str(&request_text).expect("request.ok.json parses");
        let schema = request.structured_output_schema.as_deref().expect("the schema is a string");
        assert!(serde_json::from_str::<Value>(schema).is_ok_and(|v| v.is_object()));
        assert!(
            ai_chat_params(&request, &quarry(false)).is_ok(),
            "the fixture request is admitted for service:quarry-edge"
        );

        // The ok response and the handler's own body have the same keys.
        let fixture: Value = serde_json::from_str(&contract("response.ok.json")).expect("response.ok.json");
        assert_eq!(fixture["status"], 200);
        let inference = SpeechInference::default();
        inference.answering(reply());
        let (_, outcome) = call(&inference, quarry(false), json!({}), AiChatMetering::Off).await;
        let Json(body) = outcome.expect("the call succeeds");
        let keys = |value: &Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().expect("an object").keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&body), keys(&fixture["body"]));
        assert_eq!(keys(&body["usage"]), keys(&fixture["body"]["usage"]));

        let denied: Value = serde_json::from_str(&contract("response.route-denied.json")).expect("route-denied");
        assert_eq!(denied["status"], 403);
        let unavailable: Value =
            serde_json::from_str(&contract("response.zdr-unavailable.json")).expect("zdr-unavailable");
        assert_eq!(unavailable["status"], 412);
        assert_eq!(
            unavailable["body"],
            mp_contracts::zdr_refusal::Refusal::UNAVAILABLE.json_body()
        );

        // SHA256SUMS pins every other file in the directory, byte for byte.
        use sha2::{Digest, Sha256};
        let sums = contract("SHA256SUMS");
        let mut pinned = 0;
        for line in sums.lines() {
            let (digest, name) = line.split_once(" *").expect("`<digest> *<file>` lines");
            let bytes = std::fs::read(format!("{CONTRACT_DIR}/{name}")).expect("a pinned file");
            assert_eq!(digest, format!("{:x}", Sha256::digest(&bytes)), "{name}");
            pinned += 1;
        }
        assert_eq!(pinned, 4);
    }

    #[tokio::test]
    async fn purpose_is_for_service_quarry_edge_only() {
        for (claims, purpose, expected) in [
            (user(false), "quarry.extract", StatusCode::FORBIDDEN),
            (service("service:someone-else", false), "quarry.extract", StatusCode::FORBIDDEN),
            (quarry(false), "something.else", StatusCode::BAD_REQUEST),
        ] {
            let inference = SpeechInference::default();
            let (_, outcome) = call(
                &inference,
                claims.clone(),
                json!({ "purpose": purpose }),
                AiChatMetering::Service,
            )
            .await;
            let (status, _) = outcome.expect_err("the purpose is refused");
            assert_eq!(status, expected, "{} / {purpose}", claims.sub);
            assert!(inference.cleanups().is_empty(), "no model call for a refused purpose");
        }
    }
}

/// INNH-10 MP-3: an `api_key` principal reaches the two key-grant routes and nothing else. The
/// route list is read from the source, so a route added anywhere later is covered without a
/// change here.
#[cfg(test)]
mod api_key_route_fence_tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest};
    use tower::ServiceExt;

    /// Routes outside `authed_routes`: the unauthenticated probes of `build_router_with_readiness`.
    const PUBLIC: &[&str] = &["/healthz", "/readyz", "/version", "/metrics"];

    /// Every production `.route("<path>", ..)` in the crate, as `(file, path)`.
    fn production_route_paths() -> Vec<(String, String)> {
        let pattern = regex::Regex::new(r#"\.route\(\s*"([^"]+)""#).expect("a valid pattern");
        let mut paths = Vec::new();
        for (file, text) in crate::zdr_posture::tests::read_sources() {
            let code: String = crate::zdr_posture::tests::strip_test_items(&text)
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for capture in pattern.captures_iter(&code) {
                paths.push((file.clone(), capture[1].to_owned()));
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    fn concrete(path: &str) -> String {
        path.split('/')
            .map(|segment| {
                if segment.starts_with(':') || segment.starts_with('*') {
                    "x1"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn key_claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "apikey:key-1",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": false,
            "principal_type": "api_key",
            "key_id": "key-1",
            "acting_user_id": "user-a",
            "scopes": ["browser:agent"],
        }))
        .expect("an api key token decodes")
    }

    #[tokio::test]
    async fn api_key_is_refused_on_every_other_route() {
        let paths = production_route_paths();
        let fenced: Vec<&(String, String)> = paths
            .iter()
            .filter(|(_, path)| !PUBLIC.contains(&path.as_str()))
            .collect();
        assert!(
            fenced.len() > 100,
            "the source scan found only {} routes; it is not reading the route table",
            fenced.len()
        );
        for (_, key_route) in auth::API_KEY_ROUTES {
            assert!(
                paths.iter().any(|(_, path)| path == key_route),
                "{key_route} is not registered"
            );
        }
        let mut state = AppState::new();
        // On, so the two key routes reach their handler: it answers 401 for the missing key bearer.
        state.browser_key_grants = Some(true);
        let router = authed_routes()
            .layer(middleware::from_fn(auth::authorize_principal_route))
            .layer(Extension(key_claims()))
            .with_state(state);

        let mut failures = Vec::new();
        for (file, path) in fenced {
            let uri = concrete(path);
            let is_key_path = auth::API_KEY_ROUTES.iter().any(|(_, key)| key == path);
            let mut refused = false;
            for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
                let status = router
                    .clone()
                    .oneshot(
                        HttpRequest::builder()
                            .method(method)
                            .uri(&uri)
                            .header("content-type", "application/json")
                            .body(Body::from("{}"))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status();
                if auth::API_KEY_ROUTES
                    .iter()
                    .any(|(key_method, key_path)| *key_method == method && key_path == path)
                {
                    if status != StatusCode::UNAUTHORIZED {
                        failures.push(format!("{method} {path}: the key route answered {status}"));
                    }
                    continue;
                }
                match status {
                    StatusCode::FORBIDDEN => refused = true,
                    StatusCode::METHOD_NOT_ALLOWED => {}
                    other => failures.push(format!("{method} {path} ({file}): {other}")),
                }
            }
            if !refused && !is_key_path {
                failures.push(format!(
                    "{path} ({file}): no method was refused; is it outside authed_routes?"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "an api key principal reached a route it may not:\n{}",
            failures.join("\n")
        );
    }
}

/// WS10.S6: the run trace route against a stand-in Session Core EventLog.
#[cfg(test)]
mod run_trace_route_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use axum::{body::Body, http::Request as HttpRequest};
    use mp_contracts::model_plane::v1::{
        event_log_client::EventLogClient,
        event_log_server::{EventLog, EventLogServer},
        AgentRef, AppendEventRequest, AppendEventResponse, Cursor, QueryEventsRequest,
        QueryEventsResponse, RunOrigin, SubscribeEventsRequest,
    };
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct FakeEventLog {
        seen: Arc<Mutex<Vec<(QueryEventsRequest, Option<String>)>>>,
        failure: Option<tonic::Code>,
        /// When set, the answer to every query (C6 builds its page here).
        page: Option<QueryEventsResponse>,
    }

    #[tonic::async_trait]
    impl EventLog for FakeEventLog {
        async fn append_event(
            &self,
            _: tonic::Request<AppendEventRequest>,
        ) -> Result<tonic::Response<AppendEventResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn query_events(
            &self,
            request: tonic::Request<QueryEventsRequest>,
        ) -> Result<tonic::Response<QueryEventsResponse>, tonic::Status> {
            let authorization = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            self.seen
                .lock()
                .unwrap()
                .push((request.into_inner(), authorization));
            if let Some(code) = self.failure {
                return Err(tonic::Status::new(code, "session-core says no"));
            }
            if let Some(page) = &self.page {
                return Ok(tonic::Response::new(page.clone()));
            }
            Ok(tonic::Response::new(QueryEventsResponse {
                events: vec![Event {
                    event_id: "01JC7K3M9Q4R8T2V5W6X7Y8Z9A".to_owned(),
                    event_type: EventType::BrowserStepReceiptRecorded as i32,
                    schema_version: 1,
                    ts: Some(prost_types::Timestamp {
                        seconds: 1_793_525_415,
                        nanos: 250_000_000,
                    }),
                    producer: "quarry-edge".to_owned(),
                    org_id: "org-a".to_owned(),
                    user_id: "user-a".to_owned(),
                    payload: Some(prost_types::Any {
                        type_url: RUN_TRACE_PAYLOAD_TYPE_URL.to_owned(),
                        value: br#"{"action":"click","step":7}"#.to_vec(),
                    }),
                    producer_seq: 7,
                    agent: Some(AgentRef {
                        id: "general-v1".to_owned(),
                        role: "subagent".to_owned(),
                        parent_id: "primary-v1".to_owned(),
                    }),
                    origin: RunOrigin::Agent as i32,
                    run_id: "run_1".to_owned(),
                    request_id: "req_1".to_owned(),
                    ..Default::default()
                }],
                next_cursor: Some(Cursor {
                    value: "42".to_owned(),
                }),
                has_more: true,
            }))
        }

        type SubscribeEventsStream = futures::stream::Empty<Result<Event, tonic::Status>>;

        async fn subscribe_events(
            &self,
            _: tonic::Request<SubscribeEventsRequest>,
        ) -> Result<tonic::Response<Self::SubscribeEventsStream>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }
    }

    fn claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a", "iss": "auth-core", "exp": 4_102_444_800_i64, "org_id": "org-a",
            "user_id": "user-a", "principal_type": "user", "zdr": false
        }))
        .expect("claims")
    }

    async fn app(fake: FakeEventLog, with_session_bearer: bool) -> Router {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(EventLogServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.event_log_client = EventLogClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        let mut router = Router::new()
            .route("/v1/runs/:run_id/trace", get(get_run_trace))
            .with_state(state)
            .layer(Extension(claims()));
        if with_session_bearer {
            router = router.layer(Extension(crate::auth::VerifiedSessionBearer::for_test(
                "session-core-bearer",
            )));
        }
        router
    }

    async fn get_json(router: Router, uri: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn run_trace_route_requires_the_verified_bearer() {
        let fake = FakeEventLog::default();
        let (status, _) = get_json(app(fake.clone(), false).await, "/v1/runs/run_1/trace").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(fake.seen.lock().unwrap().is_empty(), "nothing reaches Session Core");

        let (status, body) = get_json(
            app(fake.clone(), true).await,
            "/v1/runs/run_1/trace?cursor=41&limit=9999&types=BROWSER_STEP_RECEIPT_RECORDED,%20RUN_COMPLETED",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let seen = fake.seen.lock().unwrap();
        let (request, authorization) = &seen[0];
        assert_eq!(authorization.as_deref(), Some("Bearer session-core-bearer"));
        let filter = request.filter.as_ref().expect("a filter");
        assert_eq!(filter.org_id, "org-a", "the org is the token's");
        assert_eq!(filter.run_id, "run_1");
        assert_eq!(
            filter.event_types,
            [
                EventType::BrowserStepReceiptRecorded as i32,
                EventType::RunCompleted as i32
            ]
        );
        assert_eq!(request.cursor.as_ref().map(|c| c.value.as_str()), Some("41"));
        assert_eq!(request.limit, 500, "the limit is clamped to 1..500");
        assert!(!request.reverse);
        assert_eq!(body["meta"], json!({ "next_cursor": "42", "has_more": true }));
        let event = &body["data"]["events"][0];
        assert_eq!(event["event_type"], "BROWSER_STEP_RECEIPT_RECORDED");
        assert_eq!(event["origin"], "AGENT");
        assert_eq!(event["ts"], "2026-11-01T09:30:15.250Z");
        assert_eq!(event["payload"], json!({ "action": "click", "step": 7 }));
        assert_eq!(event["agent"]["parent_id"], "primary-v1");
        assert_eq!(event["producer_seq"], 7);
        assert_eq!(event["zdr"], false);
    }

    /// WS10.S9 C6: the read-API shape of `GET /v1/runs/:run_id/trace`, pinned as an authored
    /// fixture that the Frontend gateway's type test also reads. The body below is built by the
    /// real handler from a Session Core page; it must equal the fixture, field for field.
    #[tokio::test]
    async fn run_trace_read_response_matches_the_c6_fixture() {
        use sha2::{Digest, Sha256};
        const FIXTURE: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../api/contracts/run-trace-read-v1.golden.json"
        );
        let text = std::fs::read_to_string(FIXTURE).expect("the C6 fixture");
        assert!(!text.contains('\r'), "the fixture is hashed byte for byte: LF only");
        let pin = std::fs::read_to_string(format!("{FIXTURE}.sha256")).expect("the C6 pin");
        assert_eq!(
            pin.trim(),
            format!("{:x} *run-trace-read-v1.golden.json", Sha256::digest(text.as_bytes())),
            "the fixture changed without its pin (and the sync-script digest)"
        );
        let expected: Value = serde_json::from_str(&text).expect("the fixture parses");

        let first = Event {
            event_id: "01JC7K3M9Q4R8T2V5W6X7Y8Z9A".to_owned(),
            event_type: EventType::BrowserStepReceiptRecorded as i32,
            schema_version: 1,
            ts: Some(prost_types::Timestamp {
                seconds: 1_793_525_415,
                nanos: 250_000_000,
            }),
            producer: "quarry-edge".to_owned(),
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            payload: Some(prost_types::Any {
                type_url: RUN_TRACE_PAYLOAD_TYPE_URL.to_owned(),
                value: br#"{"action":"click","step":7}"#.to_vec(),
            }),
            producer_seq: 7,
            agent: Some(AgentRef {
                id: "general-v1".to_owned(),
                role: "subagent".to_owned(),
                parent_id: "primary-v1".to_owned(),
            }),
            origin: RunOrigin::Agent as i32,
            run_id: "run_1".to_owned(),
            request_id: "req_1".to_owned(),
            ..Default::default()
        };
        let second = Event {
            event_id: "01JC7K3M9Q4R8T2V5W6X7Y8Z9B".to_owned(),
            event_type: EventType::BrowserArtifactRecorded as i32,
            schema_version: 1,
            ts: Some(prost_types::Timestamp {
                seconds: 1_793_525_416,
                nanos: 0,
            }),
            producer: "quarry-edge".to_owned(),
            correlation_id: "corr_1".to_owned(),
            causation_id: "01JC7K3M9Q4R8T2V5W6X7Y8Z9A".to_owned(),
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            resource_ref: "artifact_1".to_owned(),
            // Another payload type is reported by its type_url, never as bytes.
            payload: Some(prost_types::Any {
                type_url: "type.googleapis.com/other".to_owned(),
                value: b"raw bytes".to_vec(),
            }),
            zdr: true,
            producer_seq: 8,
            run_id: "run_1".to_owned(),
            request_id: "req_1".to_owned(),
            ..Default::default()
        };
        let fake = FakeEventLog {
            page: Some(QueryEventsResponse {
                events: vec![first, second],
                next_cursor: Some(Cursor {
                    value: "42".to_owned(),
                }),
                has_more: true,
            }),
            ..Default::default()
        };
        let (status, body) = get_json(app(fake, true).await, "/v1/runs/run_1/trace").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, expected, "the handler's body drifted from the C6 fixture");

        // The last page: no events, no cursor. The same envelope with the cursor null.
        let empty = FakeEventLog {
            page: Some(QueryEventsResponse::default()),
            ..Default::default()
        };
        let (status, body) = get_json(app(empty, true).await, "/v1/runs/run_1/trace").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({ "data": { "events": [] }, "meta": { "next_cursor": null, "has_more": false } })
        );
    }

    #[tokio::test]
    async fn run_trace_route_maps_unimplemented_and_not_found() {
        for (code, want, error_code) in [
            (tonic::Code::NotFound, StatusCode::NOT_FOUND, "run_not_found"),
            (tonic::Code::PermissionDenied, StatusCode::NOT_FOUND, "run_not_found"),
            (tonic::Code::Unimplemented, StatusCode::NOT_IMPLEMENTED, "trace_unavailable"),
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST, "invalid_trace_query"),
        ] {
            let fake = FakeEventLog {
                failure: Some(code),
                ..Default::default()
            };
            let (status, body) = get_json(app(fake, true).await, "/v1/runs/run_1/trace").await;
            assert_eq!(status, want, "{code:?}");
            assert_eq!(body["error"]["code"], error_code, "{code:?}");
            assert!(
                !body.to_string().contains("session-core says no"),
                "the upstream message is not echoed: {body}"
            );
        }
    }

    #[tokio::test]
    async fn run_trace_route_refuses_bad_input_before_session_core() {
        for uri in [
            "/v1/runs/run.1/trace",
            "/v1/runs/run_1/trace?types=NOT_A_TYPE",
            "/v1/runs/run_1/trace?types=UNSPECIFIED",
            "/v1/runs/run_1/trace?types=EVENT_TYPE_RUN_COMPLETED",
        ] {
            let fake = FakeEventLog::default();
            let (status, _) = get_json(app(fake.clone(), true).await, uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert!(fake.seen.lock().unwrap().is_empty(), "{uri}");
        }
        let long_cursor = format!("/v1/runs/run_1/trace?cursor={}", "1".repeat(513));
        let fake = FakeEventLog::default();
        let (status, _) = get_json(app(fake.clone(), true).await, &long_cursor).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // A missing limit reads the default page, and an unknown payload type is never raw bytes.
        let (status, _) = get_json(app(fake.clone(), true).await, "/v1/runs/run_1/trace").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(fake.seen.lock().unwrap()[0].0.limit, 100);
        let opaque = Event {
            payload: Some(prost_types::Any {
                type_url: "type.googleapis.com/other".to_owned(),
                value: b"secret bytes".to_vec(),
            }),
            ..Default::default()
        };
        let json = trace_event_json(&opaque);
        assert_eq!(json["payload"], json!({ "type_url": "type.googleapis.com/other" }));
        assert_eq!(json["origin"], Value::Null);
    }
}

/// EP-14 S5: the run timing route against a stand-in Session Core RunLedgerService.
#[cfg(test)]
mod run_timing_route_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use axum::{body::Body, http::Request as HttpRequest};
    use mp_contracts::model_plane::v1::{
        run_ledger_service_client::RunLedgerServiceClient,
        run_ledger_service_server::{RunLedgerService, RunLedgerServiceServer},
        GetRequestSnapshotRequest, GetRequestSnapshotResponse, GetRunTimingRequest,
        GetRunTimingResponse, PromptSection, RecordRequestSnapshotRequest,
        RecordRequestSnapshotResponse, RecordRunTimingRequest, RecordRunTimingResponse,
        RunActionTiming, RunTimingRecorded,
    };
    use tower::ServiceExt;

    /// Session Core as the owner check sees it: `run_1` is user-a's, every other run is absent.
    #[derive(Clone, Default)]
    struct FakeRunLedger {
        seen: Arc<Mutex<Vec<(GetRunTimingRequest, Option<String>)>>>,
        inspected: Arc<Mutex<Vec<GetRequestSnapshotRequest>>>,
        failure: Option<tonic::Code>,
    }

    #[tonic::async_trait]
    impl RunLedgerService for FakeRunLedger {
        async fn record_request_snapshot(
            &self,
            _: tonic::Request<RecordRequestSnapshotRequest>,
        ) -> Result<tonic::Response<RecordRequestSnapshotResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn record_run_timing(
            &self,
            _: tonic::Request<RecordRunTimingRequest>,
        ) -> Result<tonic::Response<RecordRunTimingResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn get_run_timing(
            &self,
            request: tonic::Request<GetRunTimingRequest>,
        ) -> Result<tonic::Response<GetRunTimingResponse>, tonic::Status> {
            let authorization = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let request = request.into_inner();
            self.seen
                .lock()
                .unwrap()
                .push((request.clone(), authorization));
            if let Some(code) = self.failure {
                return Err(tonic::Status::new(code, "session-core says no"));
            }
            if request.run_id != "run_1" {
                return Err(tonic::Status::not_found("run not found"));
            }
            Ok(tonic::Response::new(GetRunTimingResponse {
                timings: vec![RunTimingRecorded {
                    run_id: "run_1".to_owned(),
                    request_id: "req_1".to_owned(),
                    schema: "turn-timing-v1".to_owned(),
                    report_json: r#"{"server_ms":1200,"phases":[]}"#.to_owned(),
                    phases_dropped: 0,
                    recorded_at: Some(prost_types::Timestamp {
                        seconds: 1_793_525_415,
                        nanos: 250_000_000,
                    }),
                }],
                actions: vec![RunActionTiming {
                    action_id: "act_1".to_owned(),
                    tool: "web_search".to_owned(),
                    status: "completed".to_owned(),
                    reserved_at: Some(prost_types::Timestamp {
                        seconds: 1_793_525_415,
                        nanos: 0,
                    }),
                    finalized_at: None,
                    effect_class: String::new(),
                }],
                truncated: false,
            }))
        }

        /// Call 1 of `run_1` is user-a's; Session Core has already dropped the platform section's text.
        async fn get_request_snapshot(
            &self,
            request: tonic::Request<GetRequestSnapshotRequest>,
        ) -> Result<tonic::Response<GetRequestSnapshotResponse>, tonic::Status> {
            let request = request.into_inner();
            self.inspected.lock().unwrap().push(request.clone());
            if let Some(code) = self.failure {
                return Err(tonic::Status::new(code, "session-core says no"));
            }
            if request.run_id != "run_1" || request.call_seq != 1 {
                return Err(tonic::Status::not_found("request snapshot not found"));
            }
            Ok(tonic::Response::new(GetRequestSnapshotResponse {
                snapshot_json: r#"{"schema":"request-snapshot-v1","model":"gpt-5.1"}"#.to_owned(),
                system_sections: vec![
                    PromptSection {
                        label: "stable".to_owned(),
                        source: "platform".to_owned(),
                        digest: "a".repeat(64),
                        chars: 40,
                        text: String::new(),
                    },
                    PromptSection {
                        label: "Memory".to_owned(),
                        source: "memory".to_owned(),
                        digest: "b".repeat(64),
                        chars: 30,
                        text: "The user prefers metric units.".to_owned(),
                    },
                ],
                tools_json: r#"[{"name":"web_search"}]"#.to_owned(),
                dropped: 2,
                tools_truncated: false,
                system_truncated: false,
                call_purpose: "answer".to_owned(),
            }))
        }
    }

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a", "iss": "auth-core", "exp": 4_102_444_800_i64, "org_id": "org-a",
            "user_id": "user-a", "principal_type": "user", "zdr": zdr
        }))
        .expect("claims")
    }

    async fn app(fake: FakeRunLedger, zdr: bool) -> Router {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(RunLedgerServiceServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.run_ledger_client = RunLedgerServiceClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        Router::new()
            .route("/v1/runs/:run_id/timing", get(get_run_timing))
            // The inspector with its switch on, and with it off; the env read stays out of parallel tests.
            .route(
                "/v1/runs/:run_id/requests/:call_seq",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: VerifiedModelBearer,
                     path: Path<(String, String)>| {
                        run_request_inspection(state, claims, bearer, path, true)
                    },
                ),
            )
            .route(
                "/off/v1/runs/:run_id/requests/:call_seq",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: VerifiedModelBearer,
                     path: Path<(String, String)>| {
                        run_request_inspection(state, claims, bearer, path, false)
                    },
                ),
            )
            .with_state(state)
            .layer(Extension(claims(zdr)))
            .layer(Extension(crate::auth::VerifiedSessionBearer::for_test(
                "session-core-bearer",
            )))
    }

    async fn get_json(router: Router, uri: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn get_run_timing_owner_only() {
        let fake = FakeRunLedger::default();
        let (status, body) = get_json(app(fake.clone(), false).await, "/v1/runs/run_1/timing").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let data = &body["data"];
        assert_eq!(data["timings"][0]["schema"], "turn-timing-v1");
        assert_eq!(data["timings"][0]["report"]["server_ms"], 1200);
        assert!(data["timings"][0]["recorded_at"].as_str().is_some_and(|at| at.ends_with(".250Z")));
        assert_eq!(data["actions"][0]["tool"], "web_search");
        assert_eq!(data["actions"][0]["finalized_at"], Value::Null);
        assert_eq!(data["actions"][0]["effect_class"], Value::Null);
        assert_eq!(data["truncated"], false);
        {
            let seen = fake.seen.lock().unwrap();
            assert_eq!(seen[0].1.as_deref(), Some("Bearer session-core-bearer"), "the owner's bearer");
        }

        // Another user's run reads exactly like a run that does not exist.
        let (status, body) = get_json(app(fake.clone(), false).await, "/v1/runs/run_2/timing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "run_not_found");
        let denied = FakeRunLedger {
            failure: Some(tonic::Code::PermissionDenied),
            ..FakeRunLedger::default()
        };
        let (status, body) = get_json(app(denied, false).await, "/v1/runs/run_1/timing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "run_not_found");

        // Session Core with the read switched off.
        let off = FakeRunLedger {
            failure: Some(tonic::Code::Unimplemented),
            ..FakeRunLedger::default()
        };
        let (status, body) = get_json(app(off, false).await, "/v1/runs/run_1/timing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "run_timing_unavailable");

        let (status, _) = get_json(app(fake.clone(), false).await, "/v1/runs/run.1/timing").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_run_timing_refuses_zdr_bearer() {
        let fake = FakeRunLedger::default();
        let (status, body) = get_json(app(fake.clone(), true).await, "/v1/runs/run_1/timing").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["feature"], "run_inspection");
        assert!(fake.seen.lock().unwrap().is_empty(), "nothing reaches Session Core");
    }

    #[tokio::test]
    async fn inspector_route_serves_the_owner_the_filtered_call() {
        let fake = FakeRunLedger::default();
        let (status, body) = get_json(app(fake.clone(), false).await, "/v1/runs/run_1/requests/1").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let data = &body["data"];
        assert_eq!(data["call_seq"], 1);
        assert_eq!(data["call_purpose"], "answer");
        assert_eq!(data["snapshot"]["model"], "gpt-5.1");
        assert_eq!(data["system_sections"][0]["source"], "platform");
        assert_eq!(data["system_sections"][0]["text"], Value::Null, "no platform text");
        assert_eq!(data["system_sections"][1]["text"], "The user prefers metric units.");
        assert_eq!(data["tools"][0]["name"], "web_search");
        assert_eq!(data["dropped"], 2);

        // Another user's run, an org admin and an unknown call all read as one 404.
        let (status, body) = get_json(app(fake.clone(), false).await, "/v1/runs/run_2/requests/1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "run_request_not_found");
        let off = FakeRunLedger {
            failure: Some(tonic::Code::Unimplemented),
            ..FakeRunLedger::default()
        };
        let (status, body) = get_json(app(off, false).await, "/v1/runs/run_1/requests/1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "run_request_not_found");

        for bad in ["/v1/runs/run.1/requests/1", "/v1/runs/run_1/requests/0", "/v1/runs/run_1/requests/x"] {
            let (status, _) = get_json(app(fake.clone(), false).await, bad).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        }
    }

    #[tokio::test]
    async fn inspector_route_off_is_a_404() {
        let fake = FakeRunLedger::default();
        let (status, body) = get_json(app(fake.clone(), false).await, "/off/v1/runs/run_1/requests/1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert!(fake.inspected.lock().unwrap().is_empty(), "nothing reaches Session Core");
    }

    #[tokio::test]
    async fn inspector_refuses_zdr_bearer() {
        let fake = FakeRunLedger::default();
        let (status, body) = get_json(app(fake.clone(), true).await, "/v1/runs/run_1/requests/1").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["feature"], "run_inspection");
        assert!(fake.inspected.lock().unwrap().is_empty(), "nothing reaches Session Core");
    }
}

#[cfg(test)]
mod approval_preview_route_tests {
    //! EP-10-S1: the full-draft preview route. Off and zero-retention answers are decided before
    //! anything reaches Session Core, so these tests need no Session Core at all.
    use super::*;

    use axum::{body::Body, http::Request as HttpRequest};
    use tower::ServiceExt;

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a", "iss": "auth-core", "exp": 4_102_444_800_i64, "org_id": "org-a",
            "user_id": "user-a", "principal_type": "user", "zdr": zdr
        }))
        .expect("claims")
    }

    /// The route with its switch on and with it off; the env read stays out of parallel tests.
    /// The orchestration client is the lazy default, which nothing here may reach.
    fn app(zdr: bool) -> Router {
        Router::new()
            .route(
                "/v1/orchestration/approvals/:approval_id/preview",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: VerifiedModelBearer,
                     path: Path<String>| approval_preview(state, claims, bearer, path, true),
                ),
            )
            .route(
                "/off/v1/orchestration/approvals/:approval_id/preview",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: VerifiedModelBearer,
                     path: Path<String>| approval_preview(state, claims, bearer, path, false),
                ),
            )
            .with_state(AppState::new())
            .layer(Extension(claims(zdr)))
            .layer(Extension(crate::auth::VerifiedSessionBearer::for_test(
                "session-core-bearer",
            )))
    }

    async fn get_json(router: Router, uri: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn preview_route_off_by_default() {
        for unset in [None, Some(""), Some("1"), Some("yes"), Some("on")] {
            assert!(
                !mp_contracts::loop_fold::switch_from(unset),
                "{unset:?} must not enable the preview"
            );
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
        let (status, body) =
            get_json(app(false), "/off/v1/orchestration/approvals/appr_1/preview").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "preview_unavailable");
        assert!(body["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn preview_route_refuses_zdr_token() {
        let (status, body) =
            get_json(app(true), "/v1/orchestration/approvals/appr_1/preview").await;
        let (expected_status, Json(expected)) =
            crate::zdr_posture::continuation_gate(&claims(true)).expect_err("a ZDR bearer is refused");
        assert_eq!(status, expected_status);
        assert_eq!(body, expected, "the typed refusal of a decision, before any lookup");
        assert!(body["error"]["code"].is_string());
    }

    #[test]
    fn preview_route_maps_not_found() {
        for status in [
            tonic::Status::not_found("approval not found"),
            tonic::Status::permission_denied("user-bound approval credential required"),
            tonic::Status::unimplemented("older session-core"),
        ] {
            let (code, Json(body)) = approval_preview_status_to_http(&status);
            assert_eq!(code, StatusCode::NOT_FOUND, "{status:?}");
            assert_eq!(body["error"]["code"], "approval_not_found");
        }
        let (code, Json(body)) =
            approval_preview_status_to_http(&tonic::Status::invalid_argument("approval_id is required"));
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_approval_preview");

        // A waived caller's typed refusal from Session Core keeps its catalogue body.
        let refused = mp_contracts::zdr_refusal::Refusal::WAIVER_ROUTE_MISMATCH
            .grpc_status(tonic::Code::PermissionDenied);
        let (code, Json(body)) = approval_preview_status_to_http(&refused);
        assert_eq!(code, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            mp_contracts::zdr_refusal::Refusal::WAIVER_ROUTE_MISMATCH.json_body(),
            "not flattened into a 404"
        );
    }

    #[test]
    fn preview_value_is_the_shaped_draft_as_plain_text() {
        let preview = mp_contracts::model_plane::v1::ApprovalPreview {
            kind: "mail_send".to_owned(),
            provider: "google".to_owned(),
            operation: "gmail.send".to_owned(),
            connection_id: "conn-1".to_owned(),
            to: vec!["a@example.com".to_owned(), "b@example.com".to_owned()],
            subject: "S".to_owned(),
            body_text: "<b>not markup</b>".to_owned(),
            has_html: true,
            is_reply: false,
            raw_only: false,
            editable_fields: vec!["to".to_owned(), "subject".to_owned(), "body_text".to_owned()],
            action_fingerprint: "f".repeat(64),
            revision: 0,
        };
        let value = approval_preview_value(&preview);
        assert_eq!(value["kind"], "mail_send");
        assert_eq!(value["to"], json!(["a@example.com", "b@example.com"]));
        assert_eq!(value["body_text"], "<b>not markup</b>");
        assert_eq!(value["format"], "plain_text");
        assert_eq!(value["editable_fields"], json!(["to", "subject", "body_text"]));
        assert_eq!(value["revision"], 0);
    }

    #[test]
    fn preview_can_edit_follows_the_edit_switch() {
        let mut preview = mp_contracts::model_plane::v1::ApprovalPreview {
            kind: "mail_send".to_owned(),
            editable_fields: vec!["to".to_owned(), "subject".to_owned(), "body_text".to_owned()],
            ..Default::default()
        };
        assert!(!preview_can_edit(&preview, false), "the switch off never offers an edit");
        assert!(preview_can_edit(&preview, true));
        preview.raw_only = true;
        assert!(!preview_can_edit(&preview, true), "a raw-only draft is not editable");
        preview.raw_only = false;
        preview.editable_fields.clear();
        assert!(!preview_can_edit(&preview, true), "no editable fields, no edit mode");
        preview.editable_fields = vec!["to".to_owned()];
        preview.kind = "calendar_create".to_owned();
        assert!(!preview_can_edit(&preview, true), "only a mail send is editable");
    }
}

#[cfg(test)]
mod deck_templates_route_tests {
    //! Wave 3 EP-5 S7: the deck template list route. The switch, the zero-retention refusal and the
    //! missing credential are all decided before capability-core is asked, so these tests need none;
    //! which bundles are listed is covered by `deck_templates::tests`.
    use super::*;

    use axum::{body::Body, http::Request as HttpRequest};
    use tower::ServiceExt;

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a", "iss": "auth-core", "exp": 4_102_444_800_i64, "org_id": "org-a",
            "user_id": "user-a", "principal_type": "user", "zdr": zdr
        }))
        .expect("claims")
    }

    /// The route with its switch on and with it off; the env read stays out of parallel tests. The
    /// capability client is the lazy default, which nothing here may reach.
    fn app(zdr: bool, with_bearer: bool) -> Router {
        let router = Router::new()
            .route(
                "/v1/deck-templates",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: Option<Extension<VerifiedCapabilityBearer>>| {
                        deck_templates_listing(state, claims, bearer, true)
                    },
                ),
            )
            .route(
                "/off/v1/deck-templates",
                get(
                    |state: State<AppState>,
                     claims: Extension<Claims>,
                     bearer: Option<Extension<VerifiedCapabilityBearer>>| {
                        deck_templates_listing(state, claims, bearer, false)
                    },
                ),
            )
            .with_state(AppState::new())
            .layer(Extension(claims(zdr)));
        if with_bearer {
            router.layer(Extension(VerifiedCapabilityBearer::for_test("capability-bearer")))
        } else {
            router
        }
    }

    async fn get_json(router: Router, uri: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn deck_templates_route_404_when_switch_off() {
        // Off reads as no route at all, for a ZDR caller and a caller with no credential too.
        for (zdr, with_bearer) in [(false, true), (true, true), (false, false)] {
            let (status, body) = get_json(app(zdr, with_bearer), "/off/v1/deck-templates").await;
            assert_eq!(status, StatusCode::NOT_FOUND, "zdr={zdr} bearer={with_bearer}");
            assert_eq!(body["error"]["code"], "not_found");
        }
    }

    #[tokio::test]
    async fn deck_templates_route_zdr_token_is_refused() {
        let (status, body) = get_json(app(true, true), "/v1/deck-templates").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["feature"], "skills");
    }

    #[tokio::test]
    async fn deck_templates_route_needs_a_capability_credential() {
        let (status, body) = get_json(app(false, false), "/v1/deck-templates").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "capability_credential_required");
    }

    #[test]
    fn deck_templates_invoke_field_parses_as_before_and_takes_the_spa_spelling() {
        let base = json!({ "content": "Lag en presentasjon", "zdr": false });
        let plain: InvokeRequest = serde_json::from_value(base.clone()).unwrap();
        assert!(plain.deck_template.is_none());
        let mut picked = base.clone();
        picked["deckTemplate"] = json!("skill-brand");
        let picked: InvokeRequest = serde_json::from_value(picked).unwrap();
        assert_eq!(
            crate::deck_templates::requested(picked.deck_template.as_ref()),
            Some("skill-brand")
        );
        // A value of the wrong type still parses, as the field did not exist before: it is never a pick.
        let mut odd = base;
        odd["deck_template"] = json!({ "id": "skill-brand" });
        let odd: InvokeRequest = serde_json::from_value(odd).unwrap();
        assert_eq!(crate::deck_templates::requested(odd.deck_template.as_ref()), None);
    }
}
