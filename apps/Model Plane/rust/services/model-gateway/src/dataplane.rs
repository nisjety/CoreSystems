//! Data Plane v2 integration handlers.
//!
//! Proxies Model Plane HTTP requests to Data Plane gRPC services:
//! - Document ingest via DocumentService
//! - Retrieval via RetrievalService
//! - Knowledge unit lookup via KnowledgeService
//! - Graph expansion via GraphService
//! - Wiki pages via WikiService

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use mp_contracts::dataplane::{
    documents_v2::{self as doc_pb, Document},
    graph_v1::{self as graph_pb, GraphClaim, GraphEntity, GraphRelationship},
    knowledge_v2::{self as know_pb, KnowledgeUnit},
    retrieval_v2::{self as ret_pb, Candidate, ContextFact, ContextPack, RetrievalTrace},
    wiki_v1::{self as wiki_pb, WikiPage, WikiPageVersion, WikiProposal, WikiSourceLog},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::state::AppState;

type HttpJsonError = (StatusCode, Json<Value>);

fn grpc_err(e: tonic::Status) -> HttpJsonError {
    let code = match e.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::Unavailable => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(json!({ "error": e.message() })))
}

fn ts_to_str(ts: &Option<prost_types::Timestamp>) -> Value {
    ts.as_ref().map_or(Value::Null, |t| {
        match chrono::DateTime::from_timestamp(t.seconds, t.nanos as u32) {
            Some(dt) => Value::String(dt.to_rfc3339()),
            None => Value::Number(t.seconds.into()),
        }
    })
}

#[allow(dead_code)]
fn prost_value_to_json(v: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::NumberValue(n)) => json!(*n),
        Some(Kind::StringValue(s)) => Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => Value::Bool(*b),
        Some(Kind::StructValue(st)) => struct_to_json(&Some(st.clone())),
        Some(Kind::ListValue(l)) => {
            Value::Array(l.values.iter().map(prost_value_to_json).collect())
        }
        None => Value::Null,
    }
}

#[allow(dead_code)]
fn struct_to_json(s: &Option<prost_types::Struct>) -> Value {
    s.as_ref().map_or(Value::Null, |st| {
        let map: serde_json::Map<String, Value> = st
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
            .collect();
        Value::Object(map)
    })
}

// ============================================================================
// JSON conversion helpers
// ============================================================================

pub fn document_value(d: &Document) -> Value {
    json!({
        "document_id": d.document_id,
        "org_id": d.org_id,
        "source": d.source,
        "type": d.r#type,
        "title": d.title,
        "content": d.content,
        "status": d.status,
        "zdr_classification": d.zdr_classification,
        "created_at": ts_to_str(&d.created_at),
        "updated_at": ts_to_str(&d.updated_at),
    })
}

fn candidate_value(c: &Candidate) -> Value {
    json!({
        "knowledge_id": c.knowledge_id,
        "document_id": c.document_id,
        "text": c.text,
        "dense_score": c.dense_score,
        "sparse_score": c.sparse_score,
        "rerank_score": c.rerank_score,
        "final_score": c.final_score,
        "chunk_index": c.chunk_index,
        "chunk_version": c.chunk_version,
    })
}

fn source_value(s: &ret_pb::Source) -> Value {
    json!({
        "document_id": s.document_id,
        "title": s.title,
        "source": s.source,
        "type": s.r#type,
        "created_at": ts_to_str(&s.created_at),
    })
}

fn context_fact_value(f: &ContextFact) -> Value {
    json!({
        "knowledge_id": f.knowledge_id,
        "document_id": f.document_id,
        "text": f.text,
        "score": f.score,
        "source_title": f.source_title,
        "source_type": f.source_type,
        "estimated_tokens": f.estimated_tokens,
    })
}

fn context_pack_value(p: &ContextPack) -> Value {
    json!({
        "facts": p.facts.iter().map(context_fact_value).collect::<Vec<_>>(),
        "total_tokens": p.total_tokens,
        "budget_tokens": p.budget_tokens,
        "format": p.format,
    })
}

fn trace_value(t: &RetrievalTrace) -> Value {
    json!({
        "trace_id": t.trace_id,
        "org_id": t.org_id,
        "query": t.query,
        "query_embedding_model": t.query_embedding_model,
        "index_version": t.index_version,
        "reranker_name": t.reranker_name,
        "zdr_mode": t.zdr_mode,
        "dense_retrieval_time_ms": t.dense_retrieval_time_ms,
        "sparse_retrieval_time_ms": t.sparse_retrieval_time_ms,
        "rerank_time_ms": t.rerank_time_ms,
        "total_time_ms": t.total_time_ms,
        "candidate_count_dense": t.candidate_count_dense,
        "candidate_count_sparse": t.candidate_count_sparse,
        "candidate_count_after_fusion": t.candidate_count_after_fusion,
        "candidate_count_after_rerank": t.candidate_count_after_rerank,
    })
}

fn knowledge_unit_value(u: &KnowledgeUnit) -> Value {
    json!({
        "knowledge_id": u.knowledge_id,
        "document_id": u.document_id,
        "org_id": u.org_id,
        "chunk_index": u.chunk_index,
        "text": u.text,
        "embedding_status": u.embedding_status,
        "content_hash": u.content_hash,
        "chunk_version": u.chunk_version,
        "created_at": ts_to_str(&u.created_at),
        "updated_at": ts_to_str(&u.updated_at),
    })
}

fn entity_value(e: &GraphEntity) -> Value {
    json!({
        "entity_id": e.entity_id,
        "org_id": e.org_id,
        "type": e.r#type,
        "text": e.text,
        "confidence": e.confidence,
        "provenance": e.provenance,
        "source_refs": e.source_refs,
        "created_at": ts_to_str(&e.created_at),
    })
}

fn relationship_value(r: &GraphRelationship) -> Value {
    json!({
        "rel_id": r.rel_id,
        "org_id": r.org_id,
        "entity_a_id": r.entity_a_id,
        "entity_b_id": r.entity_b_id,
        "relation_type": r.relation_type,
        "confidence": r.confidence,
        "provenance": r.provenance,
        "source_refs": r.source_refs,
        "created_at": ts_to_str(&r.created_at),
    })
}

fn claim_value(c: &GraphClaim) -> Value {
    json!({
        "claim_id": c.claim_id,
        "org_id": c.org_id,
        "text": c.text,
        "entity_ids": c.entity_ids,
        "confidence": c.confidence,
        "provenance": c.provenance,
        "source_refs": c.source_refs,
        "contradicted_by_claim_ids": c.contradicted_by_claim_ids,
        "status": c.status,
        "created_at": ts_to_str(&c.created_at),
    })
}

fn wiki_page_value(p: &WikiPage) -> Value {
    json!({
        "page_id": p.page_id,
        "org_id": p.org_id,
        "workspace_id": p.workspace_id,
        "title": p.title,
        "path": p.path,
        "current_version_id": p.current_version_id,
        "status": p.status,
        "backlinks": p.backlinks,
        "created_at": ts_to_str(&p.created_at),
        "updated_at": ts_to_str(&p.updated_at),
    })
}

fn wiki_version_value(v: &WikiPageVersion) -> Value {
    json!({
        "version_id": v.version_id,
        "page_id": v.page_id,
        "content": v.content,
        "source_refs": v.source_refs,
        "edit_reason": v.edit_reason,
        "status": v.status,
        "created_at": ts_to_str(&v.created_at),
        "published_at": ts_to_str(&v.published_at),
    })
}

fn wiki_proposal_value(p: &WikiProposal) -> Value {
    json!({
        "proposal_id": p.proposal_id,
        "page_id": p.page_id,
        "org_id": p.org_id,
        "proposed_content": p.proposed_content,
        "edit_reason": p.edit_reason,
        "proposed_by_agent": p.proposed_by_agent,
        "source_refs": p.source_refs,
        "status": p.status,
        "created_at": ts_to_str(&p.created_at),
    })
}

fn wiki_source_log_value(l: &WikiSourceLog) -> Value {
    json!({
        "log_id": l.log_id,
        "page_id": l.page_id,
        "original_chunks": l.original_chunks,
        "processing_model": l.processing_model,
        "synthesis_prompt_hash": l.synthesis_prompt_hash,
        "created_at": ts_to_str(&l.created_at),
    })
}

// ============================================================================
// Document endpoints — /v1/documents/*
// ============================================================================

#[derive(Deserialize)]
pub struct ListDocumentsQuery {
    #[serde(rename = "type", default)]
    doc_type: String,
    #[serde(default = "default_limit")]
    limit: i32,
    #[serde(default)]
    offset: i32,
}

fn default_limit() -> i32 {
    50
}

pub async fn list_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ListDocumentsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .document_client
        .clone()
        .list_documents(doc_pb::ListDocumentsRequest {
            org_id: claims.org_id,
            limit: q.limit,
            offset: q.offset,
            r#type: q.doc_type,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "documents": resp.documents.iter().map(document_value).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

pub async fn get_document(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(document_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .document_client
        .clone()
        .get_document(doc_pb::GetDocumentRequest {
            document_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let doc = resp.document.map(|d| document_value(&d));
    Ok(Json(json!({ "document": doc })))
}

#[derive(Deserialize)]
pub struct CreateDocumentBody {
    pub source: String,
    #[serde(rename = "type", default)]
    pub doc_type: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub zdr_classification: String,
}

pub async fn create_document(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateDocumentBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .document_client
        .clone()
        .create_document(doc_pb::CreateDocumentRequest {
            org_id: claims.org_id,
            source: body.source,
            r#type: body.doc_type,
            title: body.title,
            content: body.content,
            metadata: None,
            zdr_classification: body.zdr_classification,
            ingest_policy: None,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let doc = resp.document.map(|d| document_value(&d));
    Ok(Json(json!({ "document": doc })))
}

pub async fn delete_document(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(document_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .document_client
        .clone()
        .delete_document(doc_pb::DeleteDocumentRequest {
            document_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({ "success": resp.success })))
}

pub async fn bulk_ingest(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    let documents: Vec<Value> = body["documents"].as_array().cloned().unwrap_or_default();
    let create_reqs: Vec<doc_pb::CreateDocumentRequest> = documents
        .into_iter()
        .map(|d| doc_pb::CreateDocumentRequest {
            org_id: claims.org_id.clone(),
            source: d["source"].as_str().unwrap_or("").to_owned(),
            r#type: d["type"].as_str().unwrap_or("").to_owned(),
            title: d["title"].as_str().unwrap_or("").to_owned(),
            content: d["content"].as_str().unwrap_or("").to_owned(),
            metadata: None,
            zdr_classification: d["zdr_classification"].as_str().unwrap_or("").to_owned(),
            ingest_policy: None,
        })
        .collect();
    let resp = state
        .document_client
        .clone()
        .bulk_ingest(doc_pb::BulkIngestRequest {
            org_id: claims.org_id,
            documents: create_reqs,
            ingest_policy: None,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "accepted": resp.accepted,
        "rejected": resp.rejected,
        "document_ids": resp.document_ids,
    })))
}

pub async fn get_document_index_status(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(document_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .document_client
        .clone()
        .get_document_index_status(doc_pb::GetDocumentIndexStatusRequest {
            document_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let status = resp.status.map(|s| {
        json!({
            "document_id": s.document_id,
            "chunk_count": s.chunk_count,
            "chunk_status": s.chunk_status,
            "embed_status": s.embed_status,
            "embeddings_synced": s.embeddings_synced,
            "vector_status": s.vector_status,
            "last_indexed_at": ts_to_str(&s.last_indexed_at),
        })
    });
    Ok(Json(json!({ "status": status })))
}

// ============================================================================
// Retrieval endpoints — /v1/retrieval/*
// ============================================================================

#[derive(Deserialize)]
pub struct RetrieveBody {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: i32,
    #[serde(default)]
    pub filters: Option<RetrieveFilters>,
    #[serde(default)]
    pub reranker_model: Option<String>,
    #[serde(default)]
    pub context_budget_tokens: Option<i32>,
    #[serde(default)]
    pub context_format: Option<String>,
    #[serde(default)]
    pub zdr_mode: Option<String>,
    /// Phase 4 durable-retrieval readiness await (best-effort, opt-in). When set
    /// (> 0) **and** `filters.document_ids` is non-empty, the gateway waits up to
    /// this many milliseconds (capped at [`MAX_READY_WAIT_MS`]) for each named
    /// document's `dataplane.documents.indexed` signal before retrieving — the
    /// "retrieve durable on next turn" path, without polling. Omitted/`None`
    /// preserves the prior immediate read; a timeout simply proceeds (the caller
    /// still holds inline scrape context and `pending_documents` reports truth).
    #[serde(default)]
    pub wait_for_ready_ms: Option<u64>,
}

fn default_top_k() -> i32 {
    10
}

#[derive(Deserialize, Default)]
pub struct RetrieveFilters {
    #[serde(default)]
    pub document_types: Vec<String>,
    #[serde(default)]
    pub document_ids: Vec<String>,
    #[serde(default)]
    pub workspace_ids: Vec<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

pub async fn retrieve(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<RetrieveBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let filters = body.filters.unwrap_or_default();

    // Phase 4: optionally give Data Plane v2's async index/embed pipeline a
    // bounded window to land the `dataplane.documents.indexed` signal for the
    // named documents before we read. Best-effort and additive — `None` keeps
    // the prior behavior, and a timeout falls through to the read below.
    if let Some(ms) = body.wait_for_ready_ms {
        if ms > 0 && !filters.document_ids.is_empty() {
            await_documents_ready(&state, &claims.org_id, &filters.document_ids, ms).await;
        }
    }

    let pending_documents = if filters.document_ids.is_empty() {
        Vec::new()
    } else {
        check_pending_documents(&state, &claims.org_id, &filters.document_ids).await
    };

    let resp = state
        .retrieval_client
        .clone()
        .retrieve(ret_pb::RetrieveRequest {
            org_id: claims.org_id.clone(),
            query: body.query,
            filters: Some(ret_pb::Filters {
                document_types: filters.document_types,
                departments: vec![],
                languages: vec![],
                document_ids: filters.document_ids,
                region: String::new(),
                sources: filters.sources,
                workspace_ids: filters.workspace_ids,
                collection_ids: filters.collection_ids,
            }),
            top_k: body.top_k,
            user_id: Some(claims.user_id),
            role: None,
            query_expansion: None,
            reranker_model: body.reranker_model,
            top_k_before_rerank: None,
            zdr_mode: body.zdr_mode,
            context_budget_tokens: body.context_budget_tokens,
            context_format: body.context_format,
            agent_id: None,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "candidates": resp.candidates.iter().map(candidate_value).collect::<Vec<_>>(),
        "sources": resp.sources.iter().map(source_value).collect::<Vec<_>>(),
        "query": resp.query,
        "trace_id": resp.trace_id,
        "index_version": resp.index_version,
        "zdr_mode": resp.zdr_mode,
        "low_confidence": resp.low_confidence,
        "context_pack": resp.context_pack.as_ref().map(context_pack_value),
        "pending_documents": pending_documents,
    })))
}

/// Upper bound on how long a single `retrieve` will block on readiness signals,
/// regardless of the caller-supplied `wait_for_ready_ms`. Keeps a stray large
/// value from pinning a connection; on the order of the Quarry scrape timeout.
const MAX_READY_WAIT_MS: u64 = 30_000;

/// Best-effort: await the `dataplane.documents.indexed` signal for each
/// requested document concurrently, bounded by `min(ms, MAX_READY_WAIT_MS)`.
///
/// Never fails retrieval and returns nothing — a document that never signals
/// within the window just falls through to the normal `check_pending_documents`
/// report (and the agent's inline scrape context). The registry is populated by
/// [`crate::doc_indexed_consumer`]; when NATS is absent it stays empty and every
/// wait simply times out, degrading to the prior immediate-read behavior.
async fn await_documents_ready(state: &AppState, org_id: &str, document_ids: &[String], ms: u64) {
    let within = std::time::Duration::from_millis(ms.min(MAX_READY_WAIT_MS));
    let waits = document_ids
        .iter()
        .map(|doc_id| state.doc_ready.await_ready(org_id, doc_id, within));
    let _ = futures::future::join_all(waits).await;
}

/// Pre-flight check: query DocumentService.GetDocumentIndexStatus for each requested
/// document_id and return the ones whose chunk_status, embed_status, or vector_status
/// is not "ready". The model can use this to wait, fall back, or warn the user.
///
/// Calls are dispatched concurrently via `futures::future::join_all` so the
/// total latency for N documents is `~max(per_call)` rather than `sum(per_call)`.
/// Per-document RPC failures are silently omitted from the pending list rather
/// than failing the whole retrieval — the worst case is "we missed a pending
/// flag for one doc", not "the user's retrieval blew up".
async fn check_pending_documents(
    state: &AppState,
    org_id: &str,
    document_ids: &[String],
) -> Vec<Value> {
    let futures = document_ids.iter().map(|doc_id| {
        let mut client = state.document_client.clone();
        let req = doc_pb::GetDocumentIndexStatusRequest {
            document_id: doc_id.clone(),
            org_id: org_id.to_owned(),
        };
        async move { client.get_document_index_status(req).await }
    });
    let results = futures::future::join_all(futures).await;

    let mut pending = Vec::new();
    for resp in results {
        let Ok(resp) = resp else { continue };
        let Some(status) = resp.into_inner().status else {
            continue;
        };
        let chunk_ok = status.chunk_status == "ready";
        let embed_ok = status.embed_status == "ready";
        let vector_ok = status.vector_status == "ready";
        if !(chunk_ok && embed_ok && vector_ok) {
            pending.push(json!({
                "document_id": status.document_id,
                "chunk_status": status.chunk_status,
                "embed_status": status.embed_status,
                "vector_status": status.vector_status,
                "error_message": status.error_message,
            }));
        }
    }
    pending
}

pub async fn get_retrieval_trace(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(trace_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .retrieval_client
        .clone()
        .get_trace(ret_pb::GetTraceRequest {
            trace_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let trace = resp.trace.map(|t| trace_value(&t));
    Ok(Json(json!({ "trace": trace })))
}

pub async fn get_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    let document_ids: Vec<String> = body["document_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let resp = state
        .retrieval_client
        .clone()
        .get_sources(ret_pb::GetSourcesRequest {
            org_id: claims.org_id,
            document_ids,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "sources": resp.sources.iter().map(source_value).collect::<Vec<_>>(),
    })))
}

pub async fn get_chunks(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpJsonError> {
    let knowledge_ids: Vec<String> = body["knowledge_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let document_id = body["document_id"].as_str().map(str::to_owned);
    let resp = state
        .retrieval_client
        .clone()
        .get_chunks(ret_pb::GetChunksRequest {
            org_id: claims.org_id,
            knowledge_ids,
            document_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "chunks": resp.chunks.iter().map(candidate_value).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct PackContextBody {
    pub knowledge_ids: Vec<String>,
    #[serde(default = "default_budget")]
    pub budget_tokens: i32,
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_budget() -> i32 {
    4096
}
fn default_format() -> String {
    "json".to_owned()
}

pub async fn pack_context(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PackContextBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .retrieval_client
        .clone()
        .pack_context(ret_pb::PackContextRequest {
            org_id: claims.org_id,
            knowledge_ids: body.knowledge_ids,
            budget_tokens: body.budget_tokens,
            format: body.format,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let pack = resp.pack.map(|p| context_pack_value(&p));
    Ok(Json(json!({ "pack": pack })))
}

// ============================================================================
// Knowledge endpoints — /v1/knowledge/*
// ============================================================================

pub async fn get_knowledge_units(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(document_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .knowledge_client
        .clone()
        .get_knowledge_units(know_pb::GetKnowledgeUnitsRequest {
            document_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "units": resp.units.iter().map(knowledge_unit_value).collect::<Vec<_>>(),
    })))
}

pub async fn check_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(document_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .knowledge_client
        .clone()
        .check_permissions(know_pb::CheckPermissionsRequest {
            org_id: claims.org_id.clone(),
            document_id,
            user_id: claims.user_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(
        json!({ "allowed": resp.allowed, "reason": resp.reason }),
    ))
}

// ============================================================================
// Graph endpoints — /v1/graph/*
// ============================================================================

pub async fn get_entity(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(entity_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .get_entity(graph_pb::GetEntityRequest {
            entity_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let entity = resp.entity.map(|e| entity_value(&e));
    Ok(Json(json!({ "entity": entity })))
}

#[derive(Deserialize)]
pub struct ListEntitiesQuery {
    #[serde(rename = "type", default)]
    pub entity_type: String,
    #[serde(default = "default_limit")]
    pub limit: i32,
    #[serde(default)]
    pub offset: i32,
}

pub async fn list_entities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ListEntitiesQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .list_entities_by_type(graph_pb::ListEntitiesByTypeRequest {
            org_id: claims.org_id,
            entity_type: q.entity_type,
            limit: q.limit,
            offset: q.offset,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "entities": resp.entities.iter().map(entity_value).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

pub async fn get_relationships(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(entity_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .get_relationships(graph_pb::GetRelationshipsRequest {
            org_id: claims.org_id,
            entity_id,
            relation_type: q.get("type").cloned().unwrap_or_default(),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "relationships": resp.relationships.iter().map(relationship_value).collect::<Vec<_>>(),
    })))
}

pub async fn get_claims(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(entity_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .get_claims(graph_pb::GetClaimsRequest {
            org_id: claims.org_id,
            entity_id,
            status: q.get("status").cloned().unwrap_or_default(),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "claims": resp.claims.iter().map(claim_value).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct ExpandGraphBody {
    pub entity_ids: Vec<String>,
    #[serde(default = "default_max_hops")]
    pub max_hops: i32,
    #[serde(default = "default_max_entities")]
    pub max_entities: i32,
}

fn default_max_hops() -> i32 {
    2
}
fn default_max_entities() -> i32 {
    50
}

pub async fn expand_graph(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ExpandGraphBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .expand_graph(graph_pb::GraphExpansionRequest {
            org_id: claims.org_id,
            entity_ids: body.entity_ids,
            max_hops: body.max_hops,
            max_entities: body.max_entities,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    let graph = resp.graph.map(|g| {
        json!({
            "entities": g.entities.iter().map(entity_value).collect::<Vec<_>>(),
            "relationships": g.relationships.iter().map(relationship_value).collect::<Vec<_>>(),
            "claims": g.claims.iter().map(claim_value).collect::<Vec<_>>(),
            "communities": g.communities.iter().map(|c| json!({
                "community_id": c.community_id,
                "org_id": c.org_id,
                "entity_ids": c.entity_ids,
                "summary": c.summary,
                "level": c.level,
            })).collect::<Vec<_>>(),
        })
    });
    Ok(Json(json!({
        "graph": graph,
        "hops_traversed": resp.hops_traversed,
        "new_entities_found": resp.new_entities_found,
    })))
}

pub async fn get_contradictions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .graph_client
        .clone()
        .get_contradictions(graph_pb::GetContradictionsRequest {
            org_id: claims.org_id,
            entity_id: q.get("entity_id").cloned(),
            limit: q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
            offset: q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "contradictions": resp.contradictions.iter().map(claim_value).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

// ============================================================================
// Wiki endpoints — /v1/wiki/*
// ============================================================================

pub async fn get_wiki_page(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(page_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .get_page(wiki_pb::GetPageRequest {
            page_id,
            org_id: claims.org_id,
            version_id: q.get("version_id").cloned(),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "page": resp.page.map(|p| wiki_page_value(&p)),
        "version": resp.version.map(|v| wiki_version_value(&v)),
    })))
}

pub async fn get_wiki_page_by_path(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let path = q.get("path").cloned().unwrap_or_default();
    let resp = state
        .wiki_client
        .clone()
        .get_page_by_path(wiki_pb::GetPageByPathRequest {
            org_id: claims.org_id,
            path,
            version_id: q.get("version_id").cloned(),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "page": resp.page.map(|p| wiki_page_value(&p)),
        "version": resp.version.map(|v| wiki_version_value(&v)),
    })))
}

pub async fn list_wiki_page_versions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(page_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .list_page_versions(wiki_pb::ListPageVersionsRequest {
            page_id,
            org_id: claims.org_id,
            limit: q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
            offset: q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "versions": resp.versions.iter().map(wiki_version_value).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

pub async fn get_wiki_page_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(page_id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .get_page_sources(wiki_pb::GetPageSourcesRequest {
            page_id,
            org_id: claims.org_id,
            version_id: q.get("version_id").cloned(),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "source_log": resp.source_log.map(|l| wiki_source_log_value(&l)),
    })))
}

pub async fn list_wiki_maintenance_issues(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .list_maintenance_issues(wiki_pb::ListMaintenanceIssuesRequest {
            org_id: claims.org_id,
            page_id: q.get("page_id").cloned(),
            status: q.get("status").cloned().unwrap_or_default(),
            limit: q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
            offset: q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "issues": resp.issues.iter().map(|i| json!({
            "log_id": i.log_id,
            "page_id": i.page_id,
            "issue_type": i.issue_type,
            "status": i.status,
            "created_at": ts_to_str(&i.created_at),
            "resolved_at": ts_to_str(&i.resolved_at),
        })).collect::<Vec<_>>(),
        "total": resp.total,
    })))
}

pub async fn get_wiki_backlinks(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(page_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .get_backlinks(wiki_pb::GetBacklinksRequest {
            page_id,
            org_id: claims.org_id,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "pages": resp.pages.iter().map(wiki_page_value).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateWikiPageBody {
    pub workspace_id: String,
    pub title: String,
    pub path: String,
    pub initial_content: String,
}

pub async fn create_wiki_page(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateWikiPageBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .create_page(wiki_pb::CreatePageRequest {
            org_id: claims.org_id,
            workspace_id: body.workspace_id,
            title: body.title,
            path: body.path,
            initial_content: body.initial_content,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "page": resp.page.map(|p| wiki_page_value(&p)),
        "version": resp.version.map(|v| wiki_version_value(&v)),
    })))
}

#[derive(Deserialize)]
pub struct UpdateWikiPageBody {
    pub new_content: String,
    pub edit_reason: String,
}

pub async fn update_wiki_page(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(page_id): Path<String>,
    Json(body): Json<UpdateWikiPageBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .update_page_version(wiki_pb::UpdatePageVersionRequest {
            page_id,
            org_id: claims.org_id,
            new_content: body.new_content,
            edit_reason: body.edit_reason,
            proposed_by_user: Some(claims.user_id),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "new_version": resp.new_version.map(|v| wiki_version_value(&v)),
    })))
}

#[derive(Deserialize)]
pub struct SubmitProposalBody {
    pub page_id: String,
    pub proposed_content: String,
    pub edit_reason: String,
    #[serde(default)]
    pub proposed_by_agent: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
}

pub async fn submit_wiki_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<SubmitProposalBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .submit_proposal(wiki_pb::SubmitProposalRequest {
            page_id: body.page_id,
            org_id: claims.org_id,
            proposed_content: body.proposed_content,
            edit_reason: body.edit_reason,
            proposed_by_agent: body.proposed_by_agent,
            source_refs: body.source_refs,
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "proposal": resp.proposal.map(|p| wiki_proposal_value(&p)),
    })))
}

#[derive(Deserialize)]
pub struct ReviewProposalBody {
    pub decision: String,
}

pub async fn review_wiki_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(proposal_id): Path<String>,
    Json(body): Json<ReviewProposalBody>,
) -> Result<Json<Value>, HttpJsonError> {
    let resp = state
        .wiki_client
        .clone()
        .review_proposal(wiki_pb::ReviewProposalRequest {
            proposal_id,
            org_id: claims.org_id,
            decision: body.decision,
            reviewed_by: Some(claims.user_id),
        })
        .await
        .map_err(grpc_err)?
        .into_inner();
    Ok(Json(json!({
        "proposal": resp.proposal.map(|p| wiki_proposal_value(&p)),
        "new_version": resp.new_version.map(|v| wiki_version_value(&v)),
    })))
}
