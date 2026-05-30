use std::sync::Arc;

use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::pipeline::orchestrator::RetrievalPipeline;
use crate::pipeline::types::*;
use crate::search::{contradictions, graph, timeline, wiki};
use crate::trace;

pub type AppState = Arc<RetrievalPipeline>;

/// Wave 3 §15 — full auth + AuthContext + policy + audit-log middleware.
///
/// Flow:
///  1. Identify the caller: API key (no user) OR JWT (decode `sub` + `org_id`
///     claims).
///  2. Build `AuthContext` with `request_id` and `org_id`.
///  3. JWT path only — verify `X-Org-ID` header matches `claims.org_id`
///     (rejects org-claim spoofing).
///  4. JWT path with `CONTROL_PLANE_ENFORCEMENT≠off` — call PolicyClient;
///     deny if not a member or no ACL.
///  5. Inject AuthContext into request extensions; downstream handlers + the
///     pipeline read it via `Extension<AuthContext>`.
///  6. After response, append one row to `access_audit_log`.
async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    use crate::authz::{AuthContext, AuthMethod};

    let started = std::time::Instant::now();
    let endpoint_label = format!("{} {}", req.method(), req.uri().path());

    // Stable request id for log correlation. Re-use X-Request-Id if the
    // caller set one; otherwise mint a fresh one.
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // ── Step 1: identify caller ────────────────────────────────────────
    let api_key_ok = match &state.config.internal_api_key {
        Some(expected) if !expected.is_empty() => {
            let provided = req
                .headers()
                .get("x-api-key")
                .or_else(|| req.headers().get("x-internal-api-key"))
                .or_else(|| req.headers().get("x-internal-key"))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            provided == expected
        }
        _ => true,
    };

    let header_org_id = req
        .headers()
        .get("x-org-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // The AuthContext we'll build varies by which path admits the request.
    let auth_ctx: AuthContext = if api_key_ok {
        // API-key path: no user identity, org_id taken from header (trusted
        // until §15-A2 ships a "header must match a server-side mapping").
        let org_id = header_org_id.clone().unwrap_or_default();
        AuthContext::org_scoped(org_id, AuthMethod::ApiKey, request_id.clone())
    } else if let Some(token) = bearer_token(req.headers()) {
        match verify_jwt(&token).await {
            Ok(claims) => {
                let claims_org = claims.org_id.clone().unwrap_or_default();
                // §15-A core check: header MUST match claims for JWT path.
                // Empty header is interpreted as "use claims".
                let header_org = header_org_id.clone().unwrap_or_else(|| claims_org.clone());
                if header_org != claims_org && !claims_org.is_empty() {
                    crate::metrics::record_authz_denial(&claims_org, "header_claims_mismatch");
                    audit_unauthorized(
                        &state,
                        &request_id,
                        Some(&claims.sub),
                        &claims_org,
                        &endpoint_label,
                        started.elapsed().as_millis() as u32,
                        AuthMethod::Jwt,
                        "denied:header_claims_mismatch",
                    )
                    .await;
                    return Err(StatusCode::FORBIDDEN);
                }

                let mut ctx = AuthContext {
                    user_id: Some(claims.sub.clone()),
                    org_id: claims_org.clone(),
                    auth_method: AuthMethod::Jwt,
                    scopes: claims.scopes.clone(),
                    acl: crate::authz::EffectiveAcl::allow_all(),
                    request_id: request_id.clone(),
                };

                // §15-B + §15-C: PolicyClient lookup.
                let decision = state.policy.resolve(&claims.sub, &claims_org).await;
                if !decision.is_member {
                    crate::metrics::record_authz_denial(&claims_org, &decision.cause);
                    audit_unauthorized(
                        &state,
                        &request_id,
                        Some(&claims.sub),
                        &claims_org,
                        &endpoint_label,
                        started.elapsed().as_millis() as u32,
                        AuthMethod::Jwt,
                        &decision.cause,
                    )
                    .await;
                    return Err(StatusCode::FORBIDDEN);
                }
                ctx.acl = decision.acl;
                ctx
            }
            Err(_) => return Err(StatusCode::UNAUTHORIZED),
        }
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    // ── Step 2: org_id must be present at this point ───────────────────
    if auth_ctx.org_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // ── Step 3: inject into request extensions for downstream consumers ─
    let endpoint_for_audit = endpoint_label.clone();
    let auth_for_audit = auth_ctx.clone();
    req.extensions_mut().insert(auth_ctx);

    let response = next.run(req).await;
    let status = response.status();
    let latency_ms = started.elapsed().as_millis() as u32;

    // ── Step 4: audit-log append (best-effort) ─────────────────────────
    let cause = if status.is_success() {
        "ok"
    } else if status.as_u16() == 403 {
        "denied:downstream_forbidden"
    } else if status.is_client_error() {
        "error:client"
    } else {
        "error:server"
    };
    crate::audit::record_access(
        &state.pool,
        crate::audit::AccessEvent {
            ctx: &auth_for_audit,
            endpoint: &endpoint_for_audit,
            http_status: status.as_u16(),
            latency_ms,
            document_ids: vec![], // populated by handlers in v2.4+
            cause,
        },
    )
    .await;

    Ok(response)
}

/// Helper: write a denial row to the audit log for requests we reject in
/// the middleware itself (before `next.run`).
async fn audit_unauthorized(
    state: &AppState,
    request_id: &str,
    user_id: Option<&str>,
    org_id: &str,
    endpoint: &str,
    latency_ms: u32,
    method: crate::authz::AuthMethod,
    cause: &str,
) {
    let ctx = crate::authz::AuthContext {
        user_id: user_id.map(String::from),
        org_id: org_id.to_string(),
        auth_method: method,
        scopes: vec![],
        acl: crate::authz::EffectiveAcl::default(),
        request_id: request_id.to_string(),
    };
    crate::audit::record_access(
        &state.pool,
        crate::audit::AccessEvent {
            ctx: &ctx,
            endpoint,
            http_status: 403,
            latency_ms,
            document_ids: vec![],
            cause,
        },
    )
    .await;
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    value.strip_prefix("Bearer ").map(|s| s.to_string())
}

/// Verify a JWT against `JWT_PUBLIC_KEY_PEM` (RS256). Returns the decoded
/// claims so the caller can build `AuthContext` from them. JWKS fetching is
/// the wave-3.1 follow-up — most internal deployments pin a single key.
async fn verify_jwt(token: &str) -> Result<crate::authz::Claims, StatusCode> {
    // §16.5.5 — if a JWKS cache is initialized and the token's `kid`
    // resolves there, use that key. Otherwise fall through to the static
    // `JWT_PUBLIC_KEY_PEM` path (pre-3.4 behavior).
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.validate_exp = true;
    if let Ok(iss) = std::env::var("JWT_REQUIRED_ISSUER") {
        validation.set_issuer(&[iss]);
    }
    if let Ok(aud) = std::env::var("JWT_REQUIRED_AUDIENCE") {
        validation.set_audience(&[aud]);
    }

    if let Some(jwks) = crate::authz::JwksCache::global() {
        if let Ok(header) = jsonwebtoken::decode_header(token) {
            if let Some(kid) = header.kid {
                if let Some(key) = jwks.key_for_kid(&kid).await {
                    return jsonwebtoken::decode::<crate::authz::Claims>(token, &key, &validation)
                        .map(|d| d.claims)
                        .map_err(|e| {
                            tracing::debug!(error = %e, kid = %kid, "jwks jwt verify failed");
                            StatusCode::UNAUTHORIZED
                        });
                }
            }
        }
    }

    let pem = std::env::var("JWT_PUBLIC_KEY_PEM").ok();
    let pem = pem
        .filter(|p| !p.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let key = jsonwebtoken::DecodingKey::from_rsa_pem(pem.as_bytes())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let data =
        jsonwebtoken::decode::<crate::authz::Claims>(token, &key, &validation).map_err(|e| {
            tracing::debug!(error = %e, "jwt verification failed");
            StatusCode::UNAUTHORIZED
        })?;
    Ok(data.claims)
}

pub fn router_with_metrics(
    state: AppState,
    metrics_handle: Option<Arc<crate::metrics::PrometheusHandle>>,
) -> Router {
    router_inner(state, metrics_handle)
}

#[allow(dead_code)] // kept as the no-metrics-handle variant for tests + downstream embedders
pub fn router(state: AppState) -> Router {
    router_inner(state, None)
}

fn router_inner(
    state: AppState,
    metrics_handle: Option<Arc<crate::metrics::PrometheusHandle>>,
) -> Router {
    let authed = Router::new()
        // Core retrieval
        .route("/v1/retrieve", post(retrieve))
        .route("/v1/retrieve/hybrid", post(retrieve))
        .route("/v1/retrieval/{trace_id}", get(get_trace))
        // Agent-facing tools (D5/D7)
        .route("/v1/retrieve/graph", post(retrieve_graph))
        .route("/v1/retrieve/wiki", post(retrieve_wiki))
        .route("/v1/retrieve/contradictions", post(retrieve_contradictions))
        .route("/v1/retrieve/timeline", post(retrieve_timeline))
        .route("/v1/retrieve/pack", post(retrieve_pack))
        .route("/v1/retrieve/sources", post(retrieve_sources))
        .route("/v1/retrieve/freshness", post(retrieve_freshness))
        .route("/v1/retrieve/chunks", post(retrieve_chunks))
        .route("/v1/retrieve/compare", post(retrieve_compare))
        .route("/v1/index/versions", get(list_index_versions))
        // Admin cleanup (DATA-22 / v2.2)
        .route("/v1/admin/cleanup/orphans", post(cleanup_orphans))
        // App Shell read-only knowledge API (DATA-17)
        .route("/v1/knowledge/search", post(retrieve))
        .route("/v1/knowledge/graph", post(retrieve_graph))
        .route("/v1/knowledge/wiki", post(retrieve_wiki))
        .route("/v1/knowledge/sources", post(retrieve_sources))
        .route("/v1/knowledge/freshness", post(retrieve_freshness))
        // §16.5.1 — per-org rate limit applied AFTER auth_middleware runs,
        // so the limiter key resolves against the authenticated org_id.
        // `route_layer` order: bottom layer runs innermost, so we put rate
        // limit BELOW auth_middleware in the chain.
        .route_layer(middleware::from_fn_with_state(
            crate::rate_limit::PerOrgLimiter::from_env(),
            crate::rate_limit::per_org_rate_limit,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());

    let mut app = Router::new()
        .merge(authed)
        .route("/health", get(health))
        .route("/readyz", get(readyz));

    if let Some(handle) = metrics_handle {
        app = app.route(
            "/metrics",
            get(move || {
                let h = handle.clone();
                async move { h.render() }
            }),
        );
    }

    // Wave-3.1 §16.2.5 — bound request body to 10 MiB. Anything larger
    // (e.g. an oversized BulkIngest body) is rejected with 413 before
    // hitting handlers. Per-endpoint stricter limits live in validators.
    app.layer(tower_http::limit::RequestBodyLimitLayer::new(
        10 * 1024 * 1024,
    ))
    .layer(CorsLayer::permissive())
    .layer(TraceLayer::new_for_http())
    .with_state(state)
}

async fn retrieve(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<RetrievalRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Wave-3.1 §15-C completion: when an AuthContext was injected by the
    // auth_middleware, apply its ACL to the request filters BEFORE the
    // pipeline runs. Caller-supplied workspaces / collections / acl_tags
    // are intersected with what org-core says the user can see.
    if let Some(axum::extract::Extension(ctx)) = auth.as_ref() {
        ctx.apply_to_request(&mut req);
    }
    let resp = pipeline.retrieve(req).await?;
    Ok(Json(resp))
}

async fn get_trace(
    State(pipeline): State<AppState>,
    Path(trace_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let detail = trace::get_trace(&pipeline.pool, &trace_id, "").await?;
    match detail {
        Some(d) => Ok(Json(serde_json::to_value(d).unwrap()).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "trace not found"})),
        )
            .into_response()),
    }
}

// D5: Graph expansion retrieval
#[derive(serde::Deserialize)]
struct GraphRetrieveRequest {
    org_id: String,
    query: String,
    #[serde(default = "default_graph_entities")]
    max_entities: i32,
    #[serde(default)]
    include_communities: bool,
}

fn default_graph_entities() -> i32 {
    10
}

async fn retrieve_graph(
    State(pipeline): State<AppState>,
    Json(req): Json<GraphRetrieveRequest>,
) -> Result<impl IntoResponse, AppError> {
    let entities =
        graph::graph_expansion_search(&pipeline.pool, &req.query, &req.org_id, req.max_entities)
            .await?;

    let communities = if req.include_communities {
        let eids: Vec<String> = entities.iter().map(|e| e.entity_id.clone()).collect();
        graph::community_summary_search(&pipeline.pool, &eids, &req.org_id).await?
    } else {
        vec![]
    };

    Ok(Json(serde_json::json!({
        "entities": entities,
        "communities": communities,
        "entity_count": entities.len(),
    })))
}

// D5: Wiki retrieval
#[derive(serde::Deserialize)]
struct WikiRetrieveRequest {
    org_id: String,
    query: String,
    #[serde(default = "default_wiki_limit")]
    limit: i32,
}

fn default_wiki_limit() -> i32 {
    10
}

async fn retrieve_wiki(
    State(pipeline): State<AppState>,
    Json(req): Json<WikiRetrieveRequest>,
) -> Result<impl IntoResponse, AppError> {
    let pages = wiki::wiki_search(&pipeline.pool, &req.query, &req.org_id, req.limit).await?;
    Ok(Json(serde_json::json!({
        "pages": pages,
        "count": pages.len(),
    })))
}

// D7: Contradictions retrieval
#[derive(serde::Deserialize)]
struct ContradictionsRequest {
    org_id: String,
    query: Option<String>,
    #[serde(default = "default_contra_limit")]
    limit: i32,
}

fn default_contra_limit() -> i32 {
    20
}

async fn retrieve_contradictions(
    State(pipeline): State<AppState>,
    Json(req): Json<ContradictionsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let results = contradictions::search_contradictions(
        &pipeline.pool,
        &req.org_id,
        req.query.as_deref(),
        req.limit,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "contradictions": results,
        "count": results.len(),
    })))
}

// D7: Timeline/temporal retrieval
#[derive(serde::Deserialize)]
struct TimelineRequest {
    org_id: String,
    before: Option<String>,
    after: Option<String>,
    #[serde(default = "default_timeline_limit")]
    limit: i32,
    trace_id: Option<String>,
}

fn default_timeline_limit() -> i32 {
    50
}

async fn retrieve_timeline(
    State(pipeline): State<AppState>,
    Json(req): Json<TimelineRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(tid) = &req.trace_id {
        let detail = timeline::replay_trace(&pipeline.pool, tid).await?;
        return Ok(Json(serde_json::json!({"replay": detail})));
    }

    let entries = timeline::temporal_search(
        &pipeline.pool,
        &req.org_id,
        req.before.as_deref(),
        req.after.as_deref(),
        req.limit,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "timeline": entries,
        "count": entries.len(),
    })))
}

// D7: Context packing as standalone tool
#[derive(serde::Deserialize)]
struct PackRequest {
    org_id: String,
    query: String,
    #[serde(default = "default_pack_budget")]
    context_budget_tokens: usize,
    #[serde(default = "default_pack_format")]
    context_format: String,
    top_n: Option<usize>,
}

fn default_pack_budget() -> usize {
    4000
}
fn default_pack_format() -> String {
    "json".to_string()
}

async fn retrieve_pack(
    State(pipeline): State<AppState>,
    Json(req): Json<PackRequest>,
) -> Result<impl IntoResponse, AppError> {
    let retrieval_req = RetrievalRequest {
        query: req.query,
        org_id: req.org_id,
        top_k: None,
        top_n: req.top_n,
        filters: RetrievalFiltersInput::default(),
        context_budget_tokens: Some(req.context_budget_tokens),
        context_format: Some(req.context_format),
        zdr_mode: None,
        user_id: None,
        query_expansion: None,
        reranker_model: None,
        mode_mix: None,
        agent_id: None,
    };
    let resp = pipeline.retrieve(retrieval_req).await?;
    Ok(Json(serde_json::json!({
        "context_pack": resp.context_pack,
        "trace_id": resp.trace_id,
        "candidates_used": resp.candidates.len(),
    })))
}

// D7: Source lookup
#[derive(serde::Deserialize)]
struct SourcesRequest {
    org_id: String,
    document_ids: Vec<String>,
}

async fn retrieve_sources(
    State(pipeline): State<AppState>,
    Json(req): Json<SourcesRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.document_ids.is_empty() {
        return Ok(Json(serde_json::json!({"sources": []})));
    }
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT document_id, title, source, type, status, zdr_classification
         FROM documents
         WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL",
    )
    .bind(&req.document_ids)
    .bind(&req.org_id)
    .fetch_all(&pipeline.pool)
    .await?;

    let sources: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(did, title, source, dtype, status, zdr)| {
            serde_json::json!({
                "document_id": did,
                "title": title,
                "source": source,
                "type": dtype,
                "status": status,
                "zdr_classification": zdr,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"sources": sources})))
}

// D7: Freshness scoring
#[derive(serde::Deserialize)]
struct FreshnessRequest {
    org_id: String,
    document_ids: Vec<String>,
}

async fn retrieve_freshness(
    State(pipeline): State<AppState>,
    Json(req): Json<FreshnessRequest>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT document_id, title, updated_at::TEXT, status
         FROM documents
         WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL",
    )
    .bind(&req.document_ids)
    .bind(&req.org_id)
    .fetch_all(&pipeline.pool)
    .await?;

    let now = chrono::Utc::now();
    let freshness: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(did, title, updated, status)| {
            let age_days = chrono::DateTime::parse_from_str(&updated, "%Y-%m-%d %H:%M:%S%.f%z")
                .map(|dt| (now - dt.with_timezone(&chrono::Utc)).num_days())
                .unwrap_or(9999);
            let score = if age_days <= 7 {
                1.0
            } else if age_days <= 30 {
                0.8
            } else if age_days <= 90 {
                0.5
            } else {
                0.2
            };
            serde_json::json!({
                "document_id": did,
                "title": title,
                "updated_at": updated,
                "status": status,
                "age_days": age_days,
                "freshness_score": score,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"freshness": freshness})))
}

// D7: Index versions
async fn list_index_versions(
    State(pipeline): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<i32>, Option<i32>, Option<String>, String)>(
        "SELECT version_id, org_id, description, document_count, chunk_count, embedding_model, created_at::TEXT
         FROM index_versions ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&pipeline.pool)
    .await?;

    let versions: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(vid, oid, desc, docs, chunks, model, created)| {
            serde_json::json!({
                "version_id": vid,
                "org_id": oid,
                "description": desc,
                "document_count": docs,
                "chunk_count": chunks,
                "embedding_model": model,
                "created_at": created,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"versions": versions})))
}

// retrieve.chunks — exact knowledge-unit lookup by ID or document
#[derive(serde::Deserialize)]
struct ChunksRequest {
    org_id: String,
    knowledge_ids: Option<Vec<String>>,
    document_id: Option<String>,
    #[serde(default = "default_chunks_limit")]
    limit: i32,
    #[serde(default)]
    offset: i32,
}

fn default_chunks_limit() -> i32 {
    100
}

async fn retrieve_chunks(
    State(pipeline): State<AppState>,
    Json(req): Json<ChunksRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(kids) = &req.knowledge_ids {
        if !kids.is_empty() {
            let rows = sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                "SELECT ku.knowledge_id, ku.document_id, ku.chunk_text, ku.chunk_index, ku.content_hash, ku.embedding_status
                 FROM knowledge_units ku
                 JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL
                 ORDER BY ku.chunk_index"
            )
            .bind(kids)
            .bind(&req.org_id)
            .fetch_all(&pipeline.pool)
            .await?;

            let chunks: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(kid, did, text, idx, hash, status)| {
                    serde_json::json!({
                        "knowledge_id": kid, "document_id": did, "text": text,
                        "chunk_index": idx, "content_hash": hash, "embedding_status": status,
                    })
                })
                .collect();
            return Ok(Json(
                serde_json::json!({"chunks": chunks, "count": chunks.len()}),
            ));
        }
    }

    if let Some(did) = &req.document_id {
        let rows = sqlx::query_as::<_, (String, String, String, i32, String, String)>(
            "SELECT ku.knowledge_id, ku.document_id, ku.chunk_text, ku.chunk_index, ku.content_hash, ku.embedding_status
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
             ORDER BY ku.chunk_index LIMIT $3 OFFSET $4"
        )
        .bind(did)
        .bind(&req.org_id)
        .bind(req.limit)
        .bind(req.offset)
        .fetch_all(&pipeline.pool)
        .await?;

        let chunks: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(kid, did, text, idx, hash, status)| {
                serde_json::json!({
                    "knowledge_id": kid, "document_id": did, "text": text,
                    "chunk_index": idx, "content_hash": hash, "embedding_status": status,
                })
            })
            .collect();
        return Ok(Json(
            serde_json::json!({"chunks": chunks, "count": chunks.len()}),
        ));
    }

    Ok(Json(
        serde_json::json!({"error": "provide knowledge_ids or document_id"}),
    ))
}

// retrieve.compare — compare entities, sources, or document versions
#[derive(serde::Deserialize)]
struct CompareRequest {
    org_id: String,
    compare_type: String,
    ids: Vec<String>,
}

async fn retrieve_compare(
    State(pipeline): State<AppState>,
    Json(req): Json<CompareRequest>,
) -> Result<impl IntoResponse, AppError> {
    match req.compare_type.as_str() {
        "entities" => {
            let rows = sqlx::query_as::<_, (String, String, String, f64, String, serde_json::Value)>(
                "SELECT entity_id, entity_type, entity_text, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
                 FROM graph_entities WHERE entity_id = ANY($1) AND org_id = $2"
            )
            .bind(&req.ids)
            .bind(&req.org_id)
            .fetch_all(&pipeline.pool)
            .await?;

            let entities: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(eid, etype, etext, conf, prov, refs)| {
                    serde_json::json!({
                        "entity_id": eid, "entity_type": etype, "entity_text": etext,
                        "confidence": conf, "provenance": prov, "source_refs": refs,
                    })
                })
                .collect();

            // Cross-compare: find shared relationships
            let shared_rels = if req.ids.len() >= 2 {
                sqlx::query_as::<_, (String, String, String, String, f64)>(
                    "SELECT rel_id, entity_a_id, entity_b_id, relation_type, COALESCE(confidence, 0)
                     FROM graph_relationships
                     WHERE org_id = $1 AND entity_a_id = ANY($2) AND entity_b_id = ANY($2)"
                )
                .bind(&req.org_id)
                .bind(&req.ids)
                .fetch_all(&pipeline.pool)
                .await?
                .into_iter()
                .map(|(rid, a, b, rt, conf)| serde_json::json!({"rel_id": rid, "entity_a_id": a, "entity_b_id": b, "relation_type": rt, "confidence": conf}))
                .collect::<Vec<_>>()
            } else {
                vec![]
            };

            Ok(Json(serde_json::json!({
                "compare_type": "entities",
                "entities": entities,
                "shared_relationships": shared_rels,
            })))
        }
        "documents" => {
            let rows = sqlx::query_as::<_, (String, String, String, String, String, String, String)>(
                "SELECT document_id, title, source, type, status, updated_at::TEXT, zdr_classification
                 FROM documents WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL"
            )
            .bind(&req.ids)
            .bind(&req.org_id)
            .fetch_all(&pipeline.pool)
            .await?;

            let docs: Vec<serde_json::Value> = rows.into_iter().map(|(did, title, source, dtype, status, updated, zdr)| {
                serde_json::json!({
                    "document_id": did, "title": title, "source": source,
                    "type": dtype, "status": status, "updated_at": updated, "zdr_classification": zdr,
                })
            }).collect();

            // Chunk counts per doc
            let chunk_counts = sqlx::query_as::<_, (String, i64)>(
                "SELECT document_id, COUNT(*) FROM knowledge_units
                 WHERE document_id = ANY($1) AND org_id = $2 GROUP BY document_id",
            )
            .bind(&req.ids)
            .bind(&req.org_id)
            .fetch_all(&pipeline.pool)
            .await?;
            let counts: std::collections::HashMap<String, i64> = chunk_counts.into_iter().collect();

            Ok(Json(serde_json::json!({
                "compare_type": "documents",
                "documents": docs,
                "chunk_counts": counts,
            })))
        }
        _ => Ok(Json(
            serde_json::json!({"error": "compare_type must be 'entities' or 'documents'"}),
        )),
    }
}

// Admin: cleanup orphan vectors in Qdrant (vectors whose document is soft-deleted).
// Body: { "org_id": "...", "dry_run": bool }
// Response: { "orphan_doc_count": N, "orphan_chunk_count": N, "deleted_chunk_count": N, "dry_run": bool }
#[derive(serde::Deserialize)]
struct CleanupOrphansRequest {
    org_id: String,
    #[serde(default)]
    dry_run: bool,
}

async fn cleanup_orphans(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(req): Json<CleanupOrphansRequest>,
) -> Result<impl IntoResponse, AppError> {
    // §16.5.3 admin audit — resolve actor for the trail. If middleware
    // attached an AuthContext we use its subject (user_id) or auth_method;
    // otherwise we record "internal" so the row is still attributable.
    let actor = auth
        .as_ref()
        .map(|axum::extract::Extension(c)| {
            c.user_id
                .clone()
                .unwrap_or_else(|| format!("internal:{}", c.auth_method.as_str()))
        })
        .unwrap_or_else(|| "internal:unknown".to_string());
    let request_id = auth
        .as_ref()
        .map(|axum::extract::Extension(c)| c.request_id.clone());

    if req.org_id.is_empty() {
        crate::audit::record_admin(
            &pipeline.pool,
            crate::audit::AdminEvent {
                org_id: None,
                actor: &actor,
                action: "cleanup_orphans",
                target_kind: None,
                target_id: None,
                request_id: request_id.as_deref(),
                payload: Some(serde_json::json!({"dry_run": req.dry_run})),
                outcome: "error",
                error: Some("org_id required"),
            },
        )
        .await;
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "org_id required"})),
        )
            .into_response());
    }

    // 1. Find soft-deleted documents in this org
    let orphan_docs: Vec<(String,)> = sqlx::query_as(
        "SELECT document_id FROM documents
         WHERE org_id = $1 AND deleted_at IS NOT NULL",
    )
    .bind(&req.org_id)
    .fetch_all(&pipeline.pool)
    .await?;
    let orphan_doc_ids: Vec<String> = orphan_docs.into_iter().map(|(id,)| id).collect();

    if orphan_doc_ids.is_empty() {
        return Ok(Json(serde_json::json!({
            "org_id": req.org_id,
            "orphan_doc_count": 0,
            "orphan_chunk_count": 0,
            "deleted_chunk_count": 0,
            "dry_run": req.dry_run,
        }))
        .into_response());
    }

    // 2. Find all knowledge_units belonging to those documents
    let chunks: Vec<(String,)> = sqlx::query_as(
        "SELECT knowledge_id FROM knowledge_units
         WHERE document_id = ANY($1) AND org_id = $2",
    )
    .bind(&orphan_doc_ids)
    .bind(&req.org_id)
    .fetch_all(&pipeline.pool)
    .await?;
    let orphan_chunk_count = chunks.len();

    if req.dry_run {
        return Ok(Json(serde_json::json!({
            "org_id": req.org_id,
            "orphan_doc_count": orphan_doc_ids.len(),
            "orphan_chunk_count": orphan_chunk_count,
            "deleted_chunk_count": 0,
            "dry_run": true,
        }))
        .into_response());
    }

    // 3. Delete vectors from Qdrant — one filter per document_id to avoid huge OR conditions
    for doc_id in &orphan_doc_ids {
        let filter = qdrant_client::qdrant::Filter {
            must: vec![
                qdrant_client::qdrant::Condition::from(qdrant_client::qdrant::FieldCondition {
                    key: "document_id".into(),
                    r#match: Some(qdrant_client::qdrant::Match {
                        match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                            doc_id.clone(),
                        )),
                    }),
                    ..Default::default()
                }),
                qdrant_client::qdrant::Condition::from(qdrant_client::qdrant::FieldCondition {
                    key: "org_id".into(),
                    r#match: Some(qdrant_client::qdrant::Match {
                        match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                            req.org_id.clone(),
                        )),
                    }),
                    ..Default::default()
                }),
            ],
            ..Default::default()
        };

        let _ = pipeline
            .qdrant
            .delete_points(
                qdrant_client::qdrant::DeletePointsBuilder::new(&pipeline.config.qdrant_collection)
                    .points(filter)
                    .wait(true),
            )
            .await;
    }

    // 4. Delete knowledge_units rows in Postgres
    let result =
        sqlx::query("DELETE FROM knowledge_units WHERE document_id = ANY($1) AND org_id = $2")
            .bind(&orphan_doc_ids)
            .bind(&req.org_id)
            .execute(&pipeline.pool)
            .await?;
    let deleted_chunk_count = result.rows_affected() as usize;

    tracing::info!(
        org_id = %req.org_id,
        docs = orphan_doc_ids.len(),
        chunks = deleted_chunk_count,
        "orphan cleanup complete"
    );

    crate::audit::record_admin(
        &pipeline.pool,
        crate::audit::AdminEvent {
            org_id: Some(&req.org_id),
            actor: &actor,
            action: "cleanup_orphans",
            target_kind: Some("org"),
            target_id: Some(&req.org_id),
            request_id: request_id.as_deref(),
            payload: Some(serde_json::json!({
                "orphan_doc_count": orphan_doc_ids.len(),
                "orphan_chunk_count": orphan_chunk_count,
                "deleted_chunk_count": deleted_chunk_count,
                "dry_run": req.dry_run,
            })),
            outcome: "ok",
            error: None,
        },
    )
    .await;

    Ok(Json(serde_json::json!({
        "org_id": req.org_id,
        "orphan_doc_count": orphan_doc_ids.len(),
        "orphan_chunk_count": orphan_chunk_count,
        "deleted_chunk_count": deleted_chunk_count,
        "dry_run": false,
    }))
    .into_response())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "service": "retrieval-engine-rs"}))
}

async fn readyz(State(pipeline): State<AppState>) -> impl IntoResponse {
    let pg_ok = sqlx::query("SELECT 1")
        .fetch_one(&pipeline.pool)
        .await
        .is_ok();

    let qdrant_ok = pipeline.qdrant.health_check().await.is_ok();

    let redis_ok = match &pipeline.cache {
        Some(cache) => cache.health_check().await,
        None => true,
    };

    let all_ok = pg_ok && qdrant_ok && redis_ok;
    let status = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(serde_json::json!({
            "status": if all_ok { "ready" } else { "not_ready" },
            "service": "retrieval-engine-rs",
            "sparse_backend": pipeline.sparse_backend.name(),
            "checks": { "postgres": pg_ok, "qdrant": qdrant_ok, "redis": redis_ok }
        })),
    )
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!(err = %self.0, "request error");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": self.0.to_string()})),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
