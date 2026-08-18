//! HTTP routes for model-gateway on :8080.
//!
//! Public routes: /healthz, /readyz, /version, /metrics
//! Auth-gated routes: invoke endpoints, orchestration read endpoints, run-event SSE

use std::{env, fmt::Write as _};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use metrics_exporter_prometheus::PrometheusHandle;
use mp_contracts::model_plane::v1::{
    AnalyzeDocumentRequest, AnalyzeImageRequest, AnalyzeLanguageRequest, Approval, ApprovalKind,
    ApprovalProof, ApprovalState, BatchTranslateTextRequest, ContinuationExecution,
    CreateEmbeddingRequest, CreateRealtimeSessionRequest, CreateVideoGenerationJobRequest,
    DecideApprovalRequest, DetectTextLanguageRequest, ExtractImageTextRequest,
    GenerateImageRequest, GetApprovalRequest, GetPlanRequest, GetRunProofBundleRequest,
    GetRunRequest, GetSubagentLineageRequest, GetTodoRequest, GetVerificationMetricsRequest,
    GetVideoGenerationJobRequest, ListApprovalsRequest, ListMcpServersRequest, ListModelsRequest,
    ListPlansRequest, ListRunsRequest, ListSpeechVoicesRequest, ListSystemRunsRequest,
    ListTodosRequest, ListTranslationLanguagesRequest, McpServer, Plan, PlanState, PlanStep,
    PlanStepState, RegisterMcpServerRequest, ResumeRunRequest, ResumeRunResponse, RunDetail,
    RunProofBundle, StreamVideoGenerationContentRequest, SubagentLineage, SubagentRole,
    SynthesizeSpeechRequest, Todo, TodoPriority, TodoState, TranscribeSpeechRequest,
    TransitionPlanRequest, TransitionTodoRequest, TranslateTextRequest, TranslationInput,
    VerificationMetrics, VerificationStatus,
};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::{
    approvals,
    auth::{
        self, Claims, VerifiedCapabilityBearer, VerifiedCostBearer,
        VerifiedDataPlaneBearer as VerifiedBearer, VerifiedExecutionBearer,
        VerifiedInferenceBearer, VerifiedSessionBearer as VerifiedModelBearer,
    },
    gateway_metrics, normalize, rate_limit,
    readiness::GrpcReadiness,
    session_flow, sse,
    state::AppState,
};

/// Start the HTTP server on :8080.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(
    state: AppState,
    prom_handle: Option<PrometheusHandle>,
    grpc_readiness: GrpcReadiness,
) -> anyhow::Result<()> {
    let app = build_router_with_readiness(state, prom_handle, grpc_readiness);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("HTTP listening on :8080");
    axum::serve(listener, app).await?;
    Ok(())
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
    let authed = Router::new()
        .route("/v1/invoke", post(invoke))
        .route("/v1/invoke/stream", post(sse::invoke_stream_sse))
        .route("/v1/invoke/resume/:request_id", get(sse::invoke_resume_sse))
        // chat-parity §4: cooperative stop/cancel of an in-flight stream.
        .route("/v1/invoke/:request_id/cancel", post(invoke_cancel))
        // chat-parity §1: reload a thread's conversation (cross-device resume).
        .route("/v1/threads", get(list_threads).delete(delete_threads))
        // Archiving remains available as a non-destructive administrative
        // operation; DELETE is reserved for the durable erase contract below.
        .route("/v1/threads/archive", post(archive_threads))
        .route(
            "/v1/threads/:thread_id/presentation",
            post(update_thread_presentation),
        )
        .route("/v1/threads/:thread_id/archive", post(archive_thread))
        .route("/v1/threads/:thread_id", delete(delete_thread))
        // Control-authorized internal deletion adapter. This is deliberately
        // outside the browser/BFF API and is allowed only for the exact
        // deletion coordinator service by auth::authorize_principal_route.
        .route(
            "/v1/internal/space-deletion/threads",
            post(delete_space_threads_for_deletion_coordinator),
        )
        .route("/v1/threads/:thread_id/messages", get(list_thread_messages))
        // Memory management ("what do you remember about me") — user-scoped,
        // backed by session-core's MemoryService.ListMemory/DeleteMemory.
        // Named `/v1/memories` (plural) to avoid colliding with the unrelated
        // `/v1/memory` capability-core proxy registered in proxy_routes()
        // below.
        .route("/v1/memories", get(list_memories))
        .route("/v1/memories/:memory_id", delete(delete_memory_entry))
        // chat-parity §2: list models + per-model feature families for the picker.
        .route("/v1/models", get(list_models))
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
        // Run event SSE
        .route("/v1/runs/:run_id/events", get(sse::run_events_sse))
        // Browser reasoning: Model Plane proposes one safe browser action from
        // Quarry evidence; Quarry remains the only executor/capture layer.
        .route("/v1/browser/suggest-action", post(browser_suggest_action))
        // Durable browser-agent run (Phase 2) — start/pause/resume/stop a
        // bounded multi-step browser-agent loop; progress streams over the
        // existing `/v1/runs/:run_id/events` route above.
        .merge(crate::browser_run::router())
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
        .route("/v1/orchestration/runs/:run_id/cancel", post(cancel_run))
        .route("/v1/orchestration/runs/:run_id/resume", post(resume_run))
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
        // Tasks
        .route("/v1/tasks", get(list_tasks_proxy).post(create_task_proxy))
        .route("/v1/tasks/:id", get(get_task_proxy).patch(patch_task_proxy))
        .route("/v1/tasks/:id/cancel", post(cancel_task_proxy))
        // Cron
        .route("/v1/cron", get(list_cron_proxy).post(create_cron_proxy))
        .route(
            "/v1/cron/:id",
            get(get_cron_proxy)
                .patch(patch_cron_proxy)
                .delete(delete_cron_proxy),
        )
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
        .route("/v1/ai/speech", post(ai_speech))
        .route("/v1/ai/dictate", post(ai_dictate))
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

async fn readyz(Extension(grpc_readiness): Extension<GrpcReadiness>) -> Response {
    if grpc_readiness.is_bound() {
        (StatusCode::OK, "ok").into_response()
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

type HttpJsonError = (StatusCode, Json<Value>);

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

fn authenticated_session_request<T>(
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

fn require_non_zdr_durable_mutation(claims: &Claims) -> Result<(), HttpJsonError> {
    if !claims.zdr {
        return Ok(());
    }
    Err((
        StatusCode::PRECONDITION_FAILED,
        Json(json!({
            "error": {
                "code": "zdr_durable_mutation_forbidden",
                "message": "Zero Data Retention credentials cannot create, change, or delete durable state"
            }
        })),
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
            ListPlansRequest { run_id },
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
/// newest-first, for the runs-history rail. `thread_id` is required; `status`
/// filters by run status, `after` is a ULID cursor, `limit` bounds the page.
/// The verified claims gate access; session-core owns the run metadata.
async fn list_runs(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let thread_id = query.thread_id.unwrap_or_default();
    if thread_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "thread_id is required" })),
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
    let resp = state
        .orchestration_client
        .clone()
        .transition_plan(authenticated_session_request(
            TransitionPlanRequest {
                plan_id,
                target_state: PlanState::Rejected as i32,
                actor: claims.user_id.clone(),
                reason: body.reason,
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
}

async fn decide_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: VerifiedModelBearer,
    Path(approval_id): Path<String>,
    Json(body): Json<DecideApprovalBody>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
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
    let prior_approval = if matches!(target_state, ApprovalState::Granted) {
        state
            .orchestration_client
            .clone()
            .get_approval(authenticated_session_request(
                GetApprovalRequest {
                    approval_id: approval_id.clone(),
                    org_id: claims.org_id.clone(),
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
    let resp = state
        .orchestration_client
        .clone()
        .decide_approval(authenticated_session_request(
            DecideApprovalRequest {
                approval_id,
                decision: target_state as i32,
                decided_by: claims.user_id.clone(),
                decision_reason: body.reason,
                // Cross-org IDOR fix (Phase 6): scope to the caller's verified
                // JWT org, never a client-suppliable value.
                org_id: claims.org_id.clone(),
            },
            &session_bearer,
        )?)
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let approval = resp
        .approval
        .ok_or_else(|| not_found("approval not found"))?;

    // A grant is durable authority, but not a restartable continuation: this
    // path never invokes a generic `ResumeRun`, since the outbox is
    // identifier-only and there is no descriptor-backed, service-only
    // dispatcher yet to prove the paused work actually restarted.
    //
    // `quarantine_granted_approval_continuation` reports exactly that
    // boundary and — for a granted approval — is DESIGNED to always return
    // `unavailable`. That is a statement about continuation delivery, not a
    // failure of the decision itself: `DecideApproval` above already durably
    // recorded the grant (session-core's CAS write) before this ever runs.
    // So its result becomes an informational field on the success response
    // rather than clobbering it into a 502.
    let mut continuation_delivery: Option<&'static str> = None;
    if matches!(target_state, ApprovalState::Granted)
        && should_resume_granted_approval(prior_approval.as_ref(), &approval)?
    {
        continuation_delivery = Some(
            match approvals::quarantine_granted_approval_continuation(
                &approvals::gateway_approval_from_proto(&approval),
            ) {
                Ok(()) => "not_applicable",
                Err(_) => "pending",
            },
        );
    }

    let mut body = json!({ "approval": approval_value(&approval) });
    if let Some(status) = continuation_delivery {
        body["continuation_delivery"] = json!(status);
    }
    Ok(Json(body))
}

/// Cancel a run — recorded as an event; no gRPC method exists yet.
async fn cancel_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    session_bearer: VerifiedModelBearer,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    require_non_zdr_durable_mutation(&claims)?;
    require_durable_run_owner(&state, &claims, &run_id, &session_bearer).await?;
    let envelope = mp_events::envelope::Envelope {
        event_id: new_ulid(),
        event_type: "RUN_CANCEL_REQUESTED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: run_id.clone(),
        causation_id: String::new(),
        idempotency_key: format!("cancel_{run_id}"),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("run/{run_id}"),
        payload: serde_json::json!({ "run_id": run_id }),
        zdr: claims.zdr,
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
        json!({ "run_id": run_id, "status": "cancel_requested" }),
    ))
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
    require_non_zdr_durable_mutation(&claims)?;
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
        zdr: claims.zdr,
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
    #[allow(dead_code)] // accepted on the wire but not yet acted upon
    stream: bool,
    #[serde(default)]
    structured_output_schema: Option<String>,
    #[serde(default)]
    zdr: bool,
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

async fn browser_suggest_action(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<BrowserSuggestActionRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

    let request_id = new_ulid();
    let visual_summary =
        browser_visual_summary(&state, &claims, &req, &request_id, &inference_bearer).await?;
    let evidence = browser_suggestion_prompt(&req, visual_summary.as_deref());

    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: req.model.clone().unwrap_or_default(),
                provider_hint: req.provider.clone().unwrap_or_default(),
                messages: vec![
                    ChatMessage {
                        role: "system".to_owned(),
                        content: BROWSER_SUGGEST_SYSTEM_PROMPT.to_owned(),
                        name: String::new(),
                    },
                    ChatMessage {
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
    let observation = compact_json_for_prompt(&req.observation, BROWSER_OBSERVATION_PROMPT_CHARS);
    let visual_observation =
        compact_json_for_prompt(&req.visual_observation, BROWSER_VISUAL_PROMPT_CHARS);
    let visual_summary = visual_summary.map_or_else(
        || "not provided".to_owned(),
        |summary| truncate_for_prompt(summary, BROWSER_VISUAL_PROMPT_CHARS),
    );
    let visual_artifact = req
        .visual_observation_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("not provided");

    format!(
        "Goal:\n{goal}\n\nCurrent URL: {}\nCurrent title: {}\n\nQuarry observation JSON:\n{observation}\n\nOpenCV visual observation artifact id: {visual_artifact}\nOpenCV visual observation JSON:\n{visual_observation}\n\nVLM screenshot summary:\n{visual_summary}\n\nReturn the single next browser action JSON only.",
        truncate_for_prompt(&req.url, 1_000),
        truncate_for_prompt(&req.title, 500),
    )
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

async fn ai_chat(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<AiChatRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
    let zdr = claims.effective_zdr(req.zdr);
    let messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: m["role"].as_str().unwrap_or("user").to_owned(),
            content: m["content"].as_str().unwrap_or("").to_owned(),
            name: String::new(),
        })
        .collect();
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(
            InferRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: req.model,
                provider_hint: String::new(),
                messages,
                temperature: 0.7,
                max_tokens: 4096,
                structured_output_schema: req.structured_output_schema.unwrap_or_default(),
                zdr,
                ..Default::default()
            },
            &inference_bearer,
        ))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    Ok(Json(json!({
        "id": request_id,
        "content": resp.content,
        "model_used": resp.model_used,
        "usage": { "input_tokens": resp.input_tokens, "output_tokens": resp.output_tokens },
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

/// Recommend an onboarding plan from accumulated onboarding signals. Wraps
/// inference-core with a versioned prompt + JSON-schema structured output and
/// runs with ZDR (the context contains org signals). The frontend renders an
/// instant local recommendation first and swaps in this authoritative result;
/// it also falls back to its local engine if this endpoint is unavailable, so
/// this handler favors always returning a valid plan id.
async fn recommend_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<RecommendPlanRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};

    let locale = if req.locale == "en" { "en" } else { "nb" };
    let context_json = serde_json::to_string(&req.context).unwrap_or_else(|_| "{}".to_owned());
    let messages = vec![
        ChatMessage {
            role: "system".to_owned(),
            content: RECOMMEND_PLAN_SYSTEM_PROMPT.to_owned(),
            name: String::new(),
        },
        ChatMessage {
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
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: String::new(),
                provider_hint: String::new(),
                messages,
                temperature: 0.55,
                max_tokens: 1100,
                structured_output_schema: RECOMMEND_PLAN_SCHEMA.to_owned(),
                zdr: true,
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
    let zdr = claims.effective_zdr(req.zdr);
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
    // Reject before ownership checks, Capability Core forwarding, or cache
    // mutation; an issuer-ZDR principal may not change tool availability.
    require_non_zdr_durable_mutation(&claims)?;
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

    let http = reqwest::Client::new();
    let resource_metadata = crate::mcp_oauth::discover_protected_resource(&http, &body.url)
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
    let http = reqwest::Client::new();
    let Some(authorization_server) = resource_metadata.authorization_servers.first() else {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "MCP server did not name an authorization server" })),
        ));
    };
    let auth_server_metadata = crate::mcp_oauth::discover_authorization_server(
        &http,
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
    let registration =
        crate::mcp_oauth::register_client(&http, registration_endpoint, &redirect_uri)
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

    let http = reqwest::Client::new();
    match crate::mcp_oauth::discover_protected_resource(&http, &url).await {
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

    let http = reqwest::Client::new();
    let tokens = match crate::mcp_oauth::exchange_code(
        &http,
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
    let zdr = claims.effective_zdr(req.zdr);
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
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: cleanup_model,
                provider_hint: String::new(),
                messages: vec![
                    ChatMessage {
                        role: "system".to_owned(),
                        content: system_prompt,
                        name: String::new(),
                    },
                    ChatMessage {
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
    if claims.effective_zdr(req["zdr"].as_bool().unwrap_or(false))
        || crate::dataplane::document_zdr_requested(
            false,
            req["zdr_classification"].as_str().unwrap_or(""),
        )
    {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(json!({ "error": "durable document ingest is unavailable under ZDR" })),
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
        "DELETE" => client.delete(&url),
        _ => {
            return Err((
                StatusCode::METHOD_NOT_ALLOWED,
                Json(json!({"error":"method not allowed"})),
            ))
        }
    }
    .bearer_auth(bearer.as_str());
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

async fn list_tasks_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let path = format!(
        "tasks?org_id={}&status={}",
        c.org_id,
        q.get("status").cloned().unwrap_or_default()
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

async fn list_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("cron?org_id={}", c.org_id),
        "GET",
        None,
    )
    .await
}
async fn create_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
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

async fn list_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let scope = q.get("scope").cloned().unwrap_or_default();
    proxy_to_capability_core(
        &s,
        &c,
        &bearer,
        &format!("memory?org_id={}&scope={scope}", c.org_id),
        "GET",
        None,
    )
    .await
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
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (status, Json(json!({ "error": error.message() })))
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
fn session_thread_error(context: &str, error: &tonic::Status) -> HttpJsonError {
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
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("{context}: {}", error.message()),
            })),
        ),
    }
}

/// Serialize a `RunDetail` for the runs-history UI. `Snake_case` wire keys match
/// the rest of the orchestration surface; the SPA client normalizes to
/// camelCase. Timestamps are emitted as RFC3339 strings (null when unset) and
/// the arbitrary `metadata` Struct is flattened to plain JSON.
fn run_detail_value(run: &RunDetail) -> Value {
    json!({
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
    })
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

fn approval_value(approval: &Approval) -> Value {
    json!({
        "id": approval.id,
        "run_id": approval.run_id,
        "step_id": empty_to_null(&approval.step_id),
        "kind": enum_name(ApprovalKind::try_from(approval.kind).ok().as_ref()),
        "state": enum_name(ApprovalState::try_from(approval.state).ok().as_ref()),
        "requested_of": approval.requested_of,
        "decided_by": empty_to_null(&approval.decided_by),
        "decision_reason": empty_to_null(&approval.decision_reason),
    })
}

/// Serialize a Verevon Proof Bundle. Every stage that did not happen stays
/// `null` rather than becoming an empty object — a reader must never mistake
/// "no execution recorded" for "executed with blank fields".
fn proof_bundle_value(bundle: &RunProofBundle) -> Value {
    json!({
        "bundle_version": bundle.bundle_version,
        "run_id": bundle.run_id,
        "org_id": bundle.org_id,
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
        "edges": lineage.edges.iter().map(|edge| {
            json!({
                "parent_run_id": edge.parent_run_id,
                "child_run_id": edge.child_run_id,
                "role": enum_name(SubagentRole::try_from(edge.role).ok().as_ref()),
            })
        }).collect::<Vec<_>>(),
    })
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
    let target = resolve_feedback_target(crate::chat_turn_registry::global(), &claims, &body)?;
    require_durable_run_owner(&state, &claims, &target.run_id, &session_bearer).await?;
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
        zdr: claims.effective_zdr(false),
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
    #[serde(default)]
    pub structured_output_schema: Option<String>,
    #[serde(default)]
    pub zdr: bool,
    /// Explicit public-web search intent from chat clients. This duplicates the
    /// `web_search` tool definition as a durable request flag so a UI Search
    /// toggle cannot be lost by tool normalization or client/BFF drift.
    #[serde(default)]
    pub browse_web: bool,
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
    /// The client resubmitted an EDITED version of the previous question.
    ///
    /// Also client-declared, and for the same reason: the edit happened in the
    /// composer and never reached the server as a distinct action.
    #[serde(default, alias = "editResubmit")]
    pub edited_resubmit: bool,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Harness profile ("chat" | "`deployed_agent`"). Drives the approval
    /// posture (`HARNESS_PHASE1` §1). Absent → "chat" (auto, non-gating).
    #[serde(default)]
    pub profile: Option<String>,
    /// Response-style / verbosity profile (token-efficiency layer):
    /// `concise` | `detailed` | `minimal` | `normal`. `normal`/unset/unknown
    /// injects no directive. See `crate::verbosity`.
    #[serde(default)]
    pub verbosity: Option<String>,
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
/// event and closes. 404 when no active stream matches the id.
async fn invoke_cancel(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(request_id): Path<String>,
) -> Response {
    // The stream registry binds request ids to the authenticated tenant/user at
    // registration time. A mismatched owner gets the same 404 as an inactive
    // stream, avoiding unauthorized cancellation and an existence oracle.
    if state
        .cancels
        .cancel_for(&request_id, &claims.org_id, &claims.user_id)
    {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "request_id": request_id, "cancelled": true })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "request_id": request_id,
                "cancelled": false,
                "error": "no active stream",
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct ModelDescriptor {
    id: String,
    provider: String,
    modality: String,
    streaming: bool,
    features: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ListModelsHttpResponse {
    models: Vec<ModelDescriptor>,
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

    let models = resp
        .models
        .into_iter()
        .map(|m| ModelDescriptor {
            id: m.id,
            provider: m.provider,
            modality: m.modality,
            streaming: m.streaming,
            features: m.features,
        })
        .collect();

    Ok(Json(ListModelsHttpResponse { models }))
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
    if claims.effective_zdr(req.zdr) {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({"error": "durable document ingest is unavailable under ZDR"})),
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
    role: String,
    content: String,
    /// The persona this turn answered as, when it had one. Identity history
    /// from session-core's record — never an authority claim.
    #[serde(skip_serializing_if = "String::is_empty")]
    agent_name: String,
}

#[derive(Debug, Serialize)]
struct ListThreadMessagesResponse {
    thread_id: String,
    messages: Vec<ThreadMessage>,
}

#[derive(Debug, Deserialize)]
struct ListThreadsQuery {
    limit: Option<u32>,
    space_id: Option<String>,
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
    latest_run_id: String,
    latest_run_status: String,
    latest_run_updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListThreadsResponse {
    threads: Vec<ThreadSummaryResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateThreadPresentationBody {
    title: Option<String>,
    preview: Option<String>,
    pinned: Option<bool>,
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

async fn list_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Query(query): Query<ListThreadsQuery>,
) -> Result<Json<ListThreadsResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::ListThreadsRequest;

    let response = state
        .session_client
        .clone()
        .list_threads(authenticated_session_request(
            ListThreadsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: query.limit.unwrap_or(80),
                space_id: query.space_id.unwrap_or_default(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_threads failed", &e))?
        .into_inner();

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
            latest_run_id: thread.latest_run_id,
            latest_run_status: thread.latest_run_status,
            latest_run_updated_at: thread
                .latest_run_updated_at
                .map(|value| timestamp_to_rfc3339(Some(value))),
        })
        .collect();

    Ok(Json(ListThreadsResponse { threads }))
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

fn timestamp_to_rfc3339(value: Option<prost_types::Timestamp>) -> String {
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
async fn list_thread_messages(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedModelBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<ListThreadMessagesResponse>, (StatusCode, Json<serde_json::Value>)> {
    use mp_contracts::model_plane::v1::ListConversationRequest;

    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "thread_id is required"})),
        ));
    }

    let response = state
        .session_client
        .clone()
        .list_conversation(authenticated_session_request(
            ListConversationRequest {
                org_id: claims.org_id.clone(),
                thread_id: trimmed.to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_conversation failed", &e))?
        .into_inner();

    let messages = response
        .messages
        .into_iter()
        .map(|m| ThreadMessage {
            role: m.role,
            content: m.content,
            agent_name: m.agent_name,
        })
        .collect();

    Ok(Json(ListThreadMessagesResponse {
        thread_id: trimmed.to_owned(),
        messages,
    }))
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
        })
        .collect();

    Ok(Json(ListMemoriesResponse {
        memories,
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

#[allow(clippy::too_many_lines)]
async fn invoke(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    model_bearer: VerifiedModelBearer,
    inference_bearer: VerifiedInferenceBearer,
    capability_bearer: Option<Extension<VerifiedCapabilityBearer>>,
    cost_bearer: Option<Extension<VerifiedCostBearer>>,
    Json(req): Json<InvokeRequest>,
) -> Result<Json<InvokeResponse>, (StatusCode, Json<serde_json::Value>)> {
    let start = std::time::Instant::now();

    // Normalize and validate the request
    let normalized = normalize::normalize(&req)?;
    let effective_zdr = claims.effective_zdr(normalized.zdr);
    let persistence = effective_invoke_persistence_plan(&claims, normalized.zdr);
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
                    request_id: request_id.clone(),
                    org_id: claims.org_id.clone(),
                    model: normalized.model.clone(),
                    provider_hint: String::new(),
                    messages: vec![mp_contracts::model_plane::v1::ChatMessage {
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
                    zdr: true,
                    ..Default::default()
                },
                &inference_bearer,
            ))
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": error.to_string()})),
                )
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
        Some(key) => match state.idempotency.claim(key) {
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
                ));
            }
            crate::idempotency_registry::Claim::Rejected(
                crate::idempotency_registry::ClaimRejection::InvalidKey,
            ) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "idempotency_key is invalid or too large",
                    })),
                ));
            }
            crate::idempotency_registry::Claim::Rejected(
                crate::idempotency_registry::ClaimRejection::CapacityExceeded,
            ) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "idempotency protection is temporarily unavailable",
                    })),
                ));
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
    let session_run = session_flow::prepare_managed_run_authenticated(
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
    )
    .await
    .map_err(|error| {
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
        ));
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
        ));
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
                    request_id: request_id.clone(),
                    org_id: claims.org_id.clone(),
                    model: normalized.model.clone(),
                    provider_hint: String::new(),
                    messages: vec![ChatMessage {
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
                    ..Default::default()
                },
                &inference_bearer,
            ))
            .await;
        match response {
            Ok(response) => response.into_inner(),
            Err(error) => {
                if let Err(terminal_error) =
                    session_flow::terminalize_direct_inference_run_authenticated(
                        &state,
                        &session_run,
                        session_flow::DirectInferenceTerminal::Failed("inference_unavailable"),
                        &model_bearer,
                    )
                    .await
                {
                    warn!(%terminal_error, run_id = %session_run.run_id, "failed to terminalize failed direct inference run");
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({"error": "session-core terminalization failed"})),
                    ));
                }
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": error.to_string()})),
                ));
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
            ));
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
    let usage = invoke_usage(
        &infer_resp.content,
        infer_resp.input_tokens,
        infer_resp.output_tokens,
        state
            .pricing
            .cost_usd(
                &infer_resp.model_used,
                i64::from(infer_resp.input_tokens.max(0)),
                i64::from(infer_resp.output_tokens.max(0)),
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
        }),
        zdr: effective_zdr,
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

fn effective_invoke_persistence_plan(claims: &Claims, request_zdr: bool) -> InvokePersistencePlan {
    invoke_persistence_plan(claims.effective_zdr(request_zdr))
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
            sub: "user-a".to_owned(),
            iss: "issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: true,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        };

        assert_eq!(
            effective_invoke_persistence_plan(&claims, false),
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
            sub: "user-a".to_owned(),
            iss: "issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: true,
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
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
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

        let before_bind = readyz(Extension(readiness.clone())).await.into_response();
        assert_eq!(before_bind.status(), StatusCode::SERVICE_UNAVAILABLE);

        readiness.mark_bound();
        let after_bind = readyz(Extension(readiness)).await.into_response();
        assert_eq!(after_bind.status(), StatusCode::OK);
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
            _: TonicRequest<CancelRunRequest>,
        ) -> Result<TonicResponse<CancelRunResponse>, Status> {
            Err(Status::unimplemented("cancel_run not needed in test"))
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
            Ok(TonicResponse::new(ResolveRunOwnerResponse {
                authorized: request.run_id == "run-owned"
                    && request.org_id == "org-owner"
                    && request.user_id == "user-owner",
            }))
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

    fn claims(org_id: &str, user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr: false,
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
