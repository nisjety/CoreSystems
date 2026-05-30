//! HTTP routes for model-gateway on :8080.
//!
//! Public routes: /healthz, /readyz, /metrics
//! Auth-gated routes: invoke endpoints, orchestration read endpoints, run-event SSE

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use metrics_exporter_prometheus::PrometheusHandle;
use mp_contracts::model_plane::v1::{
    AnalyzeDocumentRequest, AnalyzeImageRequest, AnalyzeLanguageRequest, Approval, ApprovalKind,
    ApprovalState, BatchTranslateTextRequest, CreateEmbeddingRequest, CreateRealtimeSessionRequest,
    CreateVideoGenerationJobRequest, DecideApprovalRequest, DetectTextLanguageRequest,
    ExtractImageTextRequest, GenerateImageRequest, GetApprovalRequest, GetPlanRequest,
    GetSubagentLineageRequest, GetTodoRequest, GetVideoGenerationJobRequest, ListApprovalsRequest,
    ListModelsRequest, ListPlansRequest, ListSpeechVoicesRequest, ListTodosRequest,
    ListTranslationLanguagesRequest, Plan, PlanState, PlanStep, PlanStepState,
    StreamVideoGenerationContentRequest, SubagentLineage, SubagentRole, SynthesizeSpeechRequest,
    Todo, TodoPriority, TodoState, TranscribeSpeechRequest, TransitionPlanRequest,
    TransitionTodoRequest, TranslateTextRequest, TranslationInput,
};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::{
    auth::{self, Claims},
    gateway_metrics, normalize, rate_limit, session_flow, sse,
    state::AppState,
};

/// Start the HTTP server on :8080.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(state: AppState, prom_handle: Option<PrometheusHandle>) -> anyhow::Result<()> {
    let app = build_router(state, prom_handle);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("HTTP listening on :8080");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the axum router without binding a socket. Useful for tests.
pub fn build_router(state: AppState, prom_handle: Option<PrometheusHandle>) -> Router {
    let rate_limiter = state.rate_limiter.clone();

    // Public routes — no auth required
    let public = if let Some(handle) = prom_handle {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route(
                "/metrics",
                get(gateway_metrics::metrics_handler).with_state(handle),
            )
    } else {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route("/metrics", get(metrics_placeholder))
    };

    // Auth-gated routes with rate limiting
    let authed = Router::new()
        .route("/v1/invoke", post(invoke))
        .route("/v1/invoke/stream", post(sse::invoke_stream_sse))
        .route("/v1/invoke/resume/:request_id", get(sse::invoke_resume_sse))
        // Orchestration — read
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
            "/v1/orchestration/approvals/:approval_id",
            get(get_approval),
        )
        .route(
            "/v1/orchestration/threads/:thread_id/lineage",
            get(get_subagent_lineage),
        )
        // Orchestration — mutations
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
        // Operator feedback → skill-promotion signal (HARNESS_PHASE1 §6).
        .route("/v1/feedback", post(ingest_feedback))
        // Run event SSE
        .route("/v1/runs/:run_id/events", get(sse::run_events_sse))
        // AI modality routes  /v1/ai/*
        .route("/v1/ai/chat", post(ai_chat))
        .route("/v1/ai/embeddings", post(ai_embeddings))
        .route("/v1/ai/models", get(ai_models))
        .route("/v1/ai/images", post(ai_images))
        .route("/v1/ai/images/analyze", post(ai_images_analyze))
        .route("/v1/ai/images/ocr", post(ai_images_ocr))
        .route("/v1/ai/speech", post(ai_speech))
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
        // Capabilities  /v1/capabilities/*
        .route("/v1/capabilities", get(list_capabilities_proxy))
        .route("/v1/capabilities/:id", get(get_capability_proxy))
        // Tasks  /v1/tasks/*
        .route("/v1/tasks", get(list_tasks_proxy).post(create_task_proxy))
        .route("/v1/tasks/:id", get(get_task_proxy).patch(patch_task_proxy))
        .route("/v1/tasks/:id/cancel", post(cancel_task_proxy))
        // Cron  /v1/cron/*
        .route("/v1/cron", get(list_cron_proxy).post(create_cron_proxy))
        .route(
            "/v1/cron/:id",
            get(get_cron_proxy)
                .patch(patch_cron_proxy)
                .delete(delete_cron_proxy),
        )
        // Memory  /v1/memory/*
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
        // Skills  /v1/skills/*
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
        // --- Data Plane v2 ---
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
        .layer(middleware::from_fn(rate_limit::rate_limit_middleware))
        .layer(middleware::from_fn(auth::require_auth))
        .layer(axum::Extension(rate_limiter));

    Router::new()
        .merge(public)
        .merge(authed)
        .layer(middleware::from_fn(gateway_metrics::metrics_middleware))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

async fn metrics_placeholder() -> impl IntoResponse {
    (StatusCode::OK, "# HELP model_gateway_requests_total\n")
}

type HttpJsonError = (StatusCode, Json<Value>);

#[derive(Debug, Default, Deserialize)]
struct TodosQuery {
    run_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ApprovalsQuery {
    step_id: Option<String>,
}

async fn list_plans(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_plans(ListPlansRequest { run_id })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    Ok(Json(json!({
        "plans": response.plans.iter().map(plan_value).collect::<Vec<_>>(),
    })))
}

async fn get_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_plan(GetPlanRequest { plan_id })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let plan = response.plan.ok_or_else(|| not_found("plan not found"))?;
    Ok(Json(json!({ "plan": plan_value(&plan) })))
}

async fn list_todos(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<TodosQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_todos(ListTodosRequest {
            thread_id,
            run_id: query.run_id.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    Ok(Json(json!({
        "todos": response.todos.iter().map(todo_value).collect::<Vec<_>>(),
    })))
}

async fn get_todo(
    State(state): State<AppState>,
    Path(todo_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_todo(GetTodoRequest { todo_id })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let todo = response.todo.ok_or_else(|| not_found("todo not found"))?;
    Ok(Json(json!({ "todo": todo_value(&todo) })))
}

async fn list_approvals(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ApprovalsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .list_approvals(ListApprovalsRequest {
            run_id,
            step_id: query.step_id.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    Ok(Json(json!({
        "approvals": response.approvals.iter().map(approval_value).collect::<Vec<_>>(),
    })))
}

async fn get_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_approval(GetApprovalRequest { approval_id })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let approval = response
        .approval
        .ok_or_else(|| not_found("approval not found"))?;
    Ok(Json(json!({ "approval": approval_value(&approval) })))
}

async fn get_subagent_lineage(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let response = state
        .orchestration_client
        .clone()
        .get_subagent_lineage(GetSubagentLineageRequest { thread_id })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let lineage = response
        .lineage
        .ok_or_else(|| not_found("subagent lineage not found"))?;
    Ok(Json(json!({ "lineage": lineage_value(&lineage) })))
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
    Path(plan_id): Path<String>,
    Json(body): Json<TransitionPlanBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let target = parse_plan_state(&body.target_state)?;
    let resp = state
        .orchestration_client
        .clone()
        .transition_plan(TransitionPlanRequest {
            plan_id,
            target_state: target as i32,
            actor: claims.user_id.clone(),
            reason: body.reason,
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();
    let plan = resp.plan.ok_or_else(|| not_found("plan not found"))?;
    Ok(Json(json!({ "plan": plan_value(&plan) })))
}

async fn reject_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(plan_id): Path<String>,
    Json(body): Json<TransitionPlanBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .orchestration_client
        .clone()
        .transition_plan(TransitionPlanRequest {
            plan_id,
            target_state: PlanState::Rejected as i32,
            actor: claims.user_id.clone(),
            reason: body.reason,
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Path(todo_id): Path<String>,
    Json(body): Json<TransitionTodoBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let target = parse_todo_state(&body.status)?;
    let resp = state
        .orchestration_client
        .clone()
        .transition_todo(TransitionTodoRequest {
            todo_id,
            target_state: target as i32,
            actor: claims.user_id.clone(),
            reason: body.reason,
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Path(approval_id): Path<String>,
    Json(body): Json<DecideApprovalBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let target_state = match body.decision.as_str() {
        "approve" => ApprovalState::Granted,
        "reject" => ApprovalState::Denied,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown decision: {other}") })),
            ))
        }
    };
    let resp = state
        .orchestration_client
        .clone()
        .decide_approval(DecideApprovalRequest {
            approval_id,
            decision: target_state as i32,
            decided_by: claims.user_id.clone(),
            decision_reason: body.reason,
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();
    let approval = resp
        .approval
        .ok_or_else(|| not_found("approval not found"))?;
    Ok(Json(json!({ "approval": approval_value(&approval) })))
}

/// Cancel a run — recorded as an event; no gRPC method exists yet.
async fn cancel_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
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
        zdr: false,
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

/// Resume a cancelled/paused run — recorded as an event.
async fn resume_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
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
        zdr: false,
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
        json!({ "run_id": run_id, "status": "resume_requested" }),
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
    stream: bool,
    #[serde(default)]
    structured_output_schema: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiEmbeddingRequest {
    input: String,
    model: String,
    provider: Option<String>,
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

async fn ai_chat(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiChatRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
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
        .infer(InferRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id.clone(),
            model: req.model,
            provider_hint: String::new(),
            messages,
            temperature: 0.7,
            max_tokens: 4096,
            structured_output_schema: req.structured_output_schema.unwrap_or_default(),
            zdr: false,
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();
    Ok(Json(json!({
        "id": request_id,
        "content": resp.content,
        "model_used": resp.model_used,
        "usage": { "input_tokens": resp.input_tokens, "output_tokens": resp.output_tokens },
    })))
}

/// Embedding generation via inference-core. Data Plane owns vector storage and
/// retrieval; this route only exposes the provider-facing primitive.
async fn ai_embeddings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiEmbeddingRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_embedding(CreateEmbeddingRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            text: req.input,
            model: req.model,
            provider_hint: req.provider.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    Ok(Json(json!({
        "id": request_id,
        "object": "embedding",
        "model_used": resp.model_used,
        "provider_used": resp.provider_used,
        "embedding": resp.vector,
    })))
}

/// Active inference model catalogue.
async fn ai_models(
    State(state): State<AppState>,
    Query(query): Query<AiModelsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(ListModelsRequest {
            modality: query.modality.unwrap_or_default(),
            provider: query.provider.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
        "generate" | "image" | "text_to_image" => generate_image(state, claims, req).await,
        "analyze" | "vision" => analyze_image(state, claims, req).await,
        "ocr" | "extract_text" => extract_image_text(state, claims, req).await,
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
    Json(req): Json<AiImagesRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_image(state, claims, req).await
}

async fn ai_images_ocr(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiImagesRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    extract_image_text(state, claims, req).await
}

async fn generate_image(
    state: AppState,
    claims: Claims,
    req: AiImagesRequest,
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
        .generate_image(GenerateImageRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            prompt,
            model: req.model.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
            size: req.size.unwrap_or_default(),
            quality: req.quality.unwrap_or_default(),
            n: req.n.unwrap_or(1),
        })
        .await
        .map_err(grpc_status_to_http)?
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
        .analyze_image(AnalyzeImageRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            image_url,
            image_data,
            mime_type: req.mime_type.unwrap_or_default(),
            prompt,
            model: req.model.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
            max_tokens: req.max_tokens.unwrap_or(1024),
        })
        .await
        .map_err(grpc_status_to_http)?
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
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let (image_url, image_data) =
        image_input(req.url, req.image_url, req.content_base64, req.image_base64)?;

    let resp = state
        .inference_client
        .clone()
        .extract_image_text(ExtractImageTextRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            image_url,
            image_data,
            mime_type: req.mime_type.unwrap_or_default(),
            model: req.model.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
async fn ai_speech(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
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
            .transcribe_speech(TranscribeSpeechRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
                audio,
                format: req.format.unwrap_or_else(|| "mp3".to_owned()),
                model: req.model.unwrap_or_default(),
                provider_hint: req.provider.unwrap_or_default(),
                language: req.language.unwrap_or_default(),
            })
            .await
            .map_err(grpc_status_to_http)?
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
        .synthesize_speech(SynthesizeSpeechRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            text,
            voice: req.voice.unwrap_or_default(),
            format: req.format.unwrap_or_else(|| "mp3".to_owned()),
            model: req.model.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
            language: req.language.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Query(query): Query<AiSpeechVoicesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_speech_voices(ListSpeechVoicesRequest {
            provider: query.provider.unwrap_or_default(),
            language: query.language.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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

/// Translation via inference-core translation providers.
async fn ai_translate(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
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
        )
        .await;
    }

    if matches!(operation.as_str(), "languages" | "list_languages") {
        return list_translation_languages(state, req.provider.unwrap_or_default()).await;
    }

    let request_id = new_ulid();
    if matches!(operation.as_str(), "batch" | "batch_translate") {
        let Some(items) = req.items else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "items are required for batch translation" })),
            ));
        };
        let resp = state
            .inference_client
            .clone()
            .batch_translate_text(BatchTranslateTextRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id,
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
            })
            .await
            .map_err(grpc_status_to_http)?
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

        return Ok(Json(json!({
            "id": request_id,
            "object": "translation.batch",
            "translations": translations,
            "model_used": resp.model_used,
            "provider_used": resp.provider_used,
        })));
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
        .translate_text(TranslateTextRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            text: req.text.unwrap_or_default(),
            source_language: req.source_language.unwrap_or_default(),
            target_language: req.target_language.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
            model: req.model.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Json(req): Json<AiTranslateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    detect_text_language(
        state,
        claims.org_id,
        req.text.unwrap_or_default(),
        req.provider.unwrap_or_default(),
        req.model.unwrap_or_default(),
    )
    .await
}

/// Supported translation languages.
async fn ai_translate_languages(
    State(state): State<AppState>,
    Query(query): Query<AiTranslateLanguagesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    list_translation_languages(state, query.provider.unwrap_or_default()).await
}

async fn detect_text_language(
    state: AppState,
    org_id: String,
    text: String,
    provider_hint: String,
    model: String,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .detect_text_language(DetectTextLanguageRequest {
            request_id: request_id.clone(),
            org_id,
            text,
            provider_hint,
            model,
        })
        .await
        .map_err(grpc_status_to_http)?
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
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_translation_languages(ListTranslationLanguagesRequest { provider })
        .await
        .map_err(grpc_status_to_http)?
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
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-document").await
}

async fn ai_documents_layout(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-layout").await
}

async fn ai_documents_forms(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-document").await
}

async fn ai_documents_receipts(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-receipt").await
}

async fn ai_documents_invoices(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiDocumentIntelRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_document_with_model(state, claims, req, "prebuilt-invoice").await
}

async fn analyze_document_with_model(
    state: AppState,
    claims: Claims,
    req: AiDocumentIntelRequest,
    default_model: &str,
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
        .analyze_document(AnalyzeDocumentRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            document_url,
            document_data,
            content_type: req.content_type.unwrap_or_default(),
            model: req.model.unwrap_or_else(|| default_model.to_owned()),
            provider_hint: req.provider.unwrap_or_default(),
            pages: req.pages.unwrap_or_default(),
            locale: req.locale.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let operation = req
        .operation
        .clone()
        .unwrap_or_else(|| "sentiment".to_owned());
    analyze_language_with_operation(state, claims, req, &operation).await
}

async fn ai_language_sentiment(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "sentiment").await
}

async fn ai_language_entities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "entities").await
}

async fn ai_language_key_phrases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "key_phrases").await
}

async fn ai_language_pii(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "pii").await
}

async fn ai_language_detect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "detect").await
}

async fn ai_language_summary_text(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AiLanguageRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    analyze_language_with_operation(state, claims, req, "summary").await
}

async fn analyze_language_with_operation(
    state: AppState,
    claims: Claims,
    req: AiLanguageRequest,
    operation: &str,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let texts = language_texts(req.text, req.texts)?;
    let resp = state
        .inference_client
        .clone()
        .analyze_language(AnalyzeLanguageRequest {
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
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let results: Vec<Value> = resp
        .results
        .into_iter()
        .map(language_result_value)
        .collect();

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

fn language_result_value(result: mp_contracts::model_plane::v1::LanguageAnalysisResult) -> Value {
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

/// Ingest a document via Data Plane v2 DocumentService.
async fn ai_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::dataplane::documents_v2::CreateDocumentRequest;
    let resp = state
        .document_client
        .clone()
        .create_document(CreateDocumentRequest {
            org_id: claims.org_id,
            source: req["url"].as_str().unwrap_or("").to_owned(),
            r#type: req["type"].as_str().unwrap_or("document").to_owned(),
            title: req["title"].as_str().unwrap_or("").to_owned(),
            content: req["content"].as_str().unwrap_or("").to_owned(),
            metadata: None,
            zdr_classification: req["zdr_classification"].as_str().unwrap_or("").to_owned(),
            ingest_policy: None,
        })
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
    Json(req): Json<AiRealtimeSessionRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_realtime_session(CreateRealtimeSessionRequest {
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
        })
        .await
        .map_err(grpc_status_to_http)?
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

async fn ai_realtime_models(State(state): State<AppState>) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(ListModelsRequest {
            modality: "realtime".to_owned(),
            provider: String::new(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Json(req): Json<AiVideoGenerateRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .create_video_generation_job(CreateVideoGenerationJobRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            prompt: req.prompt,
            width: req.width.unwrap_or(1280),
            height: req.height.unwrap_or(720),
            duration_seconds: req.duration_seconds.unwrap_or(5),
            n_variants: req.n_variants.unwrap_or(1),
            model: req.model.unwrap_or_default(),
            provider_hint: req.provider.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Path(job_id): Path<String>,
    Query(query): Query<AiVideoJobQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let request_id = new_ulid();
    let resp = state
        .inference_client
        .clone()
        .get_video_generation_job(GetVideoGenerationJobRequest {
            request_id: request_id.clone(),
            org_id: claims.org_id,
            job_id,
            provider_hint: query.provider.unwrap_or_default(),
            model: query.model.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
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
    Path(generation_id): Path<String>,
    Query(query): Query<AiVideoJobQuery>,
) -> Result<Response, HttpJsonError> {
    let request_id = new_ulid();
    let stream = state
        .inference_client
        .clone()
        .stream_video_generation_content(StreamVideoGenerationContentRequest {
            request_id,
            org_id: claims.org_id,
            generation_id,
            provider_hint: query.provider.unwrap_or_default(),
            model: query.model.unwrap_or_default(),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();

    let body_stream = futures::stream::unfold(stream, |mut stream| async move {
        match stream.message().await {
            Ok(Some(chunk)) if chunk.done => None,
            Ok(Some(chunk)) => Some((Ok::<Bytes, std::io::Error>(Bytes::from(chunk.data)), stream)),
            Ok(None) => None,
            Err(status) => Some((
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    status.to_string(),
                )),
                stream,
            )),
        }
    });

    let mut response = Body::from_stream(body_stream).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    Ok(response)
}

async fn ai_video_models(State(state): State<AppState>) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .inference_client
        .clone()
        .list_models(ListModelsRequest {
            modality: "video".to_owned(),
            provider: String::new(),
        })
        .await
        .map_err(grpc_status_to_http)?
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

async fn list_capabilities_proxy(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::ListCapabilitiesRequest;
    let resp = state
        .capability_client
        .clone()
        .list_capabilities(ListCapabilitiesRequest {
            kind_filter: q.get("kind").cloned().unwrap_or_default(),
            query: q.get("q").cloned().unwrap_or_default(),
            after_id: q.get("after_id").cloned().unwrap_or_default(),
            limit: q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();
    Ok(Json(json!({
        "capabilities": resp.capabilities.iter().map(|c| json!({
            "id": c.capability_id,
            "name": c.name,
            "kind": c.kind,
            "version": c.version,
            "description": c.description,
            "risk_level": c.risk_level,
            "lazy_load": c.lazy_load,
            "scope": c.scope,
        })).collect::<Vec<_>>(),
        "has_more": resp.has_more,
    })))
}

async fn get_capability_proxy(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    use mp_contracts::model_plane::v1::GetCapabilityRequest;
    let c = state
        .capability_client
        .clone()
        .get_capability(GetCapabilityRequest {
            capability_id: id,
            version_constraint: String::new(),
        })
        .await
        .map_err(grpc_status_to_http)?
        .into_inner();
    Ok(Json(json!({
        "id": c.capability_id,
        "name": c.name,
        "kind": c.kind,
        "version": c.version,
        "description": c.description,
        "risk_level": c.risk_level,
        "lazy_load": c.lazy_load,
        "scope": c.scope,
    })))
}

// ============================================================================
// Tasks / cron / memory / skills  — proxy to capability-core HTTP API
// The capability-core service exposes these on :8085 and we forward from the
// public gateway so clients have a single origin.
// ============================================================================

/// Proxy GET/POST to capability-core's /api/v1/{path}.
async fn proxy_to_capability_core(
    state: &AppState,
    path: &str,
    method: &str,
    body: Option<&Value>,
) -> Result<Json<Value>, HttpJsonError> {
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

async fn list_tasks_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let path = format!(
        "tasks?org_id={}&status={}",
        c.org_id,
        q.get("status").cloned().unwrap_or_default()
    );
    proxy_to_capability_core(&s, &path, "GET", None).await
}
async fn create_task_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, "tasks", "POST", Some(&b)).await
}
async fn get_task_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("tasks/{id}"), "GET", None).await
}
async fn patch_task_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("tasks/{id}"), "PATCH", Some(&b)).await
}
async fn cancel_task_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("tasks/{id}/cancel"), "POST", None).await
}

async fn list_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("cron?org_id={}", c.org_id), "GET", None).await
}
async fn create_cron_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, "cron", "POST", Some(&b)).await
}
async fn get_cron_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("cron/{id}"), "GET", None).await
}
async fn patch_cron_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("cron/{id}"), "PATCH", Some(&b)).await
}
async fn delete_cron_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("cron/{id}"), "DELETE", None).await
}

async fn list_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let scope = q.get("scope").cloned().unwrap_or_default();
    proxy_to_capability_core(
        &s,
        &format!("memory?org_id={}&scope={scope}", c.org_id),
        "GET",
        None,
    )
    .await
}
async fn create_memory_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, "memory", "POST", Some(&b)).await
}
async fn get_memory_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("memory/{id}"), "GET", None).await
}
async fn patch_memory_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("memory/{id}"), "PATCH", Some(&b)).await
}
async fn delete_memory_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("memory/{id}"), "DELETE", None).await
}

async fn list_skills_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("skills?org_id={}", c.org_id), "GET", None).await
}
async fn create_skill_proxy(
    State(s): State<AppState>,
    Extension(c): Extension<Claims>,
    Json(mut b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    b["org_id"] = json!(c.org_id);
    proxy_to_capability_core(&s, "skills", "POST", Some(&b)).await
}
async fn get_skill_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("skills/{id}"), "GET", None).await
}
async fn patch_skill_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("skills/{id}"), "PATCH", Some(&b)).await
}
async fn delete_skill_proxy(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    proxy_to_capability_core(&s, &format!("skills/{id}"), "DELETE", None).await
}

fn grpc_status_to_http(error: tonic::Status) -> HttpJsonError {
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

/// Operator feedback published as `mp.v1.feedback.rated`, consumed by
/// orchestrator-core's FeedbackPromotionWorkflow (HARNESS_PHASE1 §6).
#[derive(Debug, Deserialize)]
struct FeedbackBody {
    run_id: String,
    /// Skill (or agent acting as a skill) the rating applies to.
    skill_id: String,
    #[serde(default = "default_from_scope")]
    from_scope: String,
    #[serde(default = "default_to_scope")]
    to_scope: String,
    /// "good" | "acceptable" | "poor".
    rating: String,
}

fn default_from_scope() -> String {
    "agent".to_owned()
}
fn default_to_scope() -> String {
    "workspace".to_owned()
}

/// Publish an operator rating as a feedback envelope. Best-effort: the durable
/// rating already lives in the rating store; this is the promotion signal.
async fn ingest_feedback(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<FeedbackBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "FEEDBACK_RATED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: body.run_id.clone(),
        causation_id: String::new(),
        idempotency_key: format!("feedback_{}", body.run_id),
        org_id: claims.org_id.clone(),
        user_id: claims.user_id.clone(),
        resource_ref: format!("run/{}", body.run_id),
        payload: json!({
            "run_id": body.run_id,
            "skill_id": body.skill_id,
            "from_scope": body.from_scope,
            "to_scope": body.to_scope,
            "rating": body.rating,
        }),
        zdr: false,
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
    Ok(Json(json!({ "accepted": true })))
}

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub content: String,
    pub model: Option<String>,
    pub session_key: Option<String>,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub structured_output_schema: Option<String>,
    #[serde(default)]
    pub zdr: bool,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Harness profile ("chat" | "deployed_agent"). Drives the approval
    /// posture (HARNESS_PHASE1 §1). Absent → "chat" (auto, non-gating).
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InvokeResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
}

#[allow(clippy::too_many_lines)]
async fn invoke(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<InvokeRequest>,
) -> Result<Json<InvokeResponse>, (StatusCode, Json<serde_json::Value>)> {
    let start = std::time::Instant::now();

    // Normalize and validate the request
    let normalized = normalize::normalize(&req)?;

    // Pre-flight budget check against cost-core
    crate::budget::check_budget(&state.http_client, &claims.org_id, &normalized).await?;

    let request_id = new_ulid();
    let session_run = session_flow::prepare_run(
        &state,
        normalized.thread_id.as_deref(),
        normalized.session_key.as_deref(),
        &claims.org_id,
        &claims.user_id,
        &normalized.content,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("session-core prepare_run failed: {error}")})),
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
        zdr: normalized.zdr,
    };

    state
        .publisher
        .publish(&subjects::ingress_subject("accepted"), &envelope)
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to publish INGRESS_ACCEPTED");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal server error"})),
            )
        })?;

    // Call inference-core
    let infer_resp = {
        use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
        state
            .inference_client
            .clone()
            .infer(InferRequest {
                request_id: request_id.clone(),
                org_id: claims.org_id.clone(),
                model: normalized.model.clone(),
                provider_hint: String::new(),
                messages: vec![ChatMessage {
                    role: "user".to_owned(),
                    content: normalized.content.clone(),
                    name: String::new(),
                }],
                temperature: 0.7,
                max_tokens: 4096,
                structured_output_schema: normalized
                    .structured_output_schema
                    .clone()
                    .unwrap_or_default(),
                zdr: normalized.zdr,
            })
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
            })?
            .into_inner()
    };

    session_flow::append_assistant_message(&state, &session_run.thread_id, &infer_resp.content)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("session-core append assistant failed: {error}")})),
            )
        })?;

    info!(run_id = %session_run.run_id, thread_id = %session_run.thread_id, request_id = %request_id, "http invoke completed");

    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

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
        zdr: normalized.zdr,
    };

    if let Err(e) = state
        .publisher
        .publish(&subjects::usage_subject(&claims.org_id), &usage_envelope)
        .await
    {
        warn!(error = %e, "failed to publish USAGE_ENVELOPE");
    }

    Ok(Json(InvokeResponse {
        request_id,
        content: infer_resp.content,
        model_used: infer_resp.model_used,
    }))
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
