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

fn pin_request_org(
    auth: Option<&axum::extract::Extension<crate::authz::AuthContext>>,
    org_id: &mut String,
) -> Result<(), AppError> {
    crate::authz::pin_org_from_ctx(auth.map(|extension| &extension.0), org_id)
        .map_err(|_| AppError::forbidden("authenticated tenant does not match requested tenant"))
}

/// Resolve the viewer identity + their explicit document grants for the by-id
/// retrieval endpoints (chunks/sources/freshness). The viewer comes only from the
/// authenticated `AuthContext` — never a request body — and grants come from the
/// visibility client (fail-open to empty). Returns `(None, [])` for unauthenticated
/// callers, which makes the ownership predicate a no-op (legacy org-scoped path).
async fn resolve_viewer_grants(
    pipeline: &AppState,
    auth: Option<&axum::extract::Extension<crate::authz::AuthContext>>,
    org_id: &str,
) -> (Option<String>, Vec<String>) {
    // Org-admin super-visibility (`org:data:read_all`): viewer=None makes the
    // ownership predicate a no-op (org-scoped) — audited. Only a verified JWT
    // carries scopes, so api-key/agent callers never get the bypass here.
    if let Some(ext) = auth {
        if ext.scopes.iter().any(|s| s == "org:data:read_all") {
            tracing::warn!(
                org_id = %org_id,
                actor = ext.user_id.as_deref().unwrap_or("unknown"),
                reason = "admin_bypass:read_all",
                "org-admin super-visibility on by-id retrieval (org-scoped, audited)"
            );
            return (None, Vec::new());
        }
    }
    let viewer = auth.and_then(|ext| ext.user_id.clone());
    let granted = match &viewer {
        Some(uid) => {
            pipeline
                .visibility
                .visible_documents(
                    org_id,
                    uid,
                    auth.and_then(|ext| ext.verified_bearer.as_deref()),
                )
                .await
        }
        None => Vec::new(),
    };
    (viewer, granted)
}

/// Wave 3 §15 — full auth + AuthContext + policy + audit-log middleware.
///
/// Flow:
///  1. Identify the caller from a verified JWT (`sub` + `org_id` claims).
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
    let header_org_id = req
        .headers()
        .get("x-org-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // A fleet-shared API key is not an identity and can no longer authorize a
    // tenant. User and service callers both use audience-scoped signed tokens.
    let auth_ctx: AuthContext = if let Some(token) = bearer_token(req.headers()) {
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
                    verified_bearer: Some(token.clone()),
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
#[allow(clippy::too_many_arguments)] // pre-existing arity; clippy 1.94 -D warnings
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
        verified_bearer: None,
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
        // Semantic response cache — Model Plane gateway SemanticCache seam.
        .route("/v1/cache/semantic/search", post(semantic_cache_search))
        .route("/v1/cache/semantic/store", post(semantic_cache_store))
        .route("/v1/cache/semantic/prune", post(semantic_cache_prune))
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
    auth: axum::extract::Extension<crate::authz::AuthContext>,
    Path(trace_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let actor = if auth.scopes.iter().any(|scope| scope == "org:data:read_all") {
        None
    } else {
        auth.user_id.as_deref()
    };
    let detail = trace::get_trace(&pipeline.pool, &trace_id, &auth.org_id, actor).await?;
    match detail {
        Some(d) => Ok(Json(serde_json::to_value(d).unwrap()).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "trace not found"})),
        )
            .into_response()),
    }
}

/// Semantic response cache — the Model Plane gateway calls these behind its
/// `SemanticCache` seam (the gateway cannot host a vector store itself). Both
/// are best-effort from the caller's side: a miss/error just reruns inference.
#[derive(serde::Deserialize)]
struct SemanticCacheSearchRequest {
    org_id: String,
    model: String,
    prompt: String,
    /// Optional ZDR session mode (e.g. "ephemeral"). When ephemeral, the prompt
    /// is ZDR content and its embedding must not egress to a retaining provider;
    /// the embed layer's egress guard enforces it. Defaults to non-ZDR.
    #[serde(default)]
    zdr_mode: Option<ZdrMode>,
    /// Authorization scope that partitions the cache (PR-A). The Model Plane
    /// gateway MUST pass the caller's per-user visible-document-set hash, or the
    /// literal `"org-shared"` when grounding used only org-public docs. Absent
    /// under a require-scope deployment ⇒ the cache no-ops (fail-closed), so a
    /// response grounded on one user's private docs is never served to another.
    #[serde(default)]
    scope_key: Option<String>,
}

#[derive(serde::Deserialize)]
struct SemanticCacheStoreRequest {
    org_id: String,
    model: String,
    prompt: String,
    response: String,
    #[serde(default)]
    zdr_mode: Option<ZdrMode>,
    /// See `SemanticCacheSearchRequest::scope_key`. For `store`, the gateway
    /// should pass the hash of the visible-document set the response was grounded
    /// on (or `"org-shared"`); it must match the scope used at search time.
    #[serde(default)]
    scope_key: Option<String>,
}

fn is_restrictive_zdr(zdr_mode: Option<ZdrMode>) -> bool {
    zdr_mode.unwrap_or_default().restricts_egress()
}

async fn semantic_cache_search(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<SemanticCacheSearchRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    let hit = crate::cache::semantic::search(
        &pipeline.qdrant,
        &pipeline.embedder,
        &pipeline.config,
        &req.org_id,
        &req.model,
        &req.prompt,
        is_restrictive_zdr(req.zdr_mode),
        req.scope_key.as_deref(),
    )
    .await?;
    Ok(match hit {
        Some(h) => Json(serde_json::json!({
            "hit": true,
            "response": h.response,
            "score": h.score,
        })),
        None => Json(serde_json::json!({ "hit": false })),
    })
}

async fn semantic_cache_store(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<SemanticCacheStoreRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    crate::cache::semantic::store(
        &pipeline.qdrant,
        &pipeline.embedder,
        &pipeline.config,
        &req.org_id,
        &req.model,
        &req.prompt,
        &req.response,
        is_restrictive_zdr(req.zdr_mode),
        req.scope_key.as_deref(),
    )
    .await?;
    Ok(Json(serde_json::json!({ "stored": true })))
}

#[derive(serde::Deserialize, Default)]
struct SemanticCachePruneRequest {
    /// Delete entries older than this many seconds. Defaults to the configured
    /// `SEMANTIC_CACHE_TTL_SECS`. A cron can POST this on a schedule.
    #[serde(default)]
    older_than_secs: Option<i64>,
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default = "default_true")]
    dry_run: bool,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    idempotency_key: Option<String>,
}

async fn semantic_cache_prune(
    State(pipeline): State<AppState>,
    auth: axum::extract::Extension<crate::authz::AuthContext>,
    body: Option<Json<SemanticCachePruneRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let admin = authorize_admin_mutation(
        &auth,
        req.org_id.as_deref(),
        "data:admin:cache-prune",
        req.dry_run,
        &req.reason,
        req.idempotency_key.as_deref(),
    )?;
    let older_than = req
        .older_than_secs
        .unwrap_or(pipeline.config.semantic_cache_ttl_secs as i64)
        .max(0);
    let mut tx = if admin.dry_run {
        None
    } else {
        let mut tx = pipeline.pool.begin().await?;
        let lock_key = format!("semantic_cache_prune:{}", admin.org_id);
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(&lock_key)
                .fetch_one(&mut *tx)
                .await?;
        if !locked {
            return Err(AppError::conflict(
                "another cache prune is already running for this tenant",
            ));
        }
        let key = admin
            .idempotency_key
            .as_deref()
            .expect("mutation validation requires idempotency key");
        let previous: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT payload FROM admin_audit_log
             WHERE org_id = $1 AND actor = $2 AND action = 'semantic_cache_prune'
               AND outcome = 'ok' AND payload->>'idempotency_key' = $3
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&admin.org_id)
        .bind(&admin.actor)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(payload) = previous {
            tx.rollback().await?;
            return Ok(Json(serde_json::json!({
                "org_id": admin.org_id,
                "idempotent_replay": true,
                "result": payload,
            })));
        }
        Some(tx)
    };

    let pruned = crate::cache::semantic::prune(
        &pipeline.qdrant,
        &pipeline.config,
        &admin.org_id,
        older_than,
        admin.dry_run,
    )
    .await?;
    let payload = serde_json::json!({
        "reason": admin.reason,
        "idempotency_key": admin.idempotency_key,
        "dry_run": admin.dry_run,
        "older_than_secs": older_than,
        "matched": pruned,
        "pruned": if admin.dry_run { 0 } else { pruned },
    });
    if let Some(mut tx) = tx.take() {
        sqlx::query(
            "INSERT INTO admin_audit_log
             (org_id, actor, action, target_kind, target_id, request_id, payload, outcome)
             VALUES ($1, $2, 'semantic_cache_prune', 'org', $1, $3, $4, 'ok')",
        )
        .bind(&admin.org_id)
        .bind(&admin.actor)
        .bind(&admin.request_id)
        .bind(&payload)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    } else {
        crate::audit::record_admin(
            &pipeline.pool,
            crate::audit::AdminEvent {
                org_id: Some(&admin.org_id),
                actor: &admin.actor,
                action: "semantic_cache_prune",
                target_kind: Some("org"),
                target_id: Some(&admin.org_id),
                request_id: Some(&admin.request_id),
                payload: Some(payload),
                outcome: "ok",
                error: None,
            },
        )
        .await;
    }
    Ok(Json(serde_json::json!({
        "org_id": admin.org_id,
        "matched": pruned,
        "pruned": if admin.dry_run { 0 } else { pruned },
        "dry_run": admin.dry_run,
        "older_than_secs": older_than,
    })))
}

fn default_true() -> bool {
    true
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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<GraphRetrieveRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    // Per-user ownership gate (mirrors the dense/sparse post-filter): graph nodes
    // are filtered to those derived from documents the viewer can see. No viewer
    // → org-scoped (legacy). Closes the graph-grounding leak (4-path test).
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let entities = graph::graph_expansion_search(
        &pipeline.pool,
        &req.query,
        &req.org_id,
        req.max_entities,
        viewer.as_deref(),
        &granted,
    )
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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<WikiRetrieveRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let allowed_workspaces = auth
        .as_ref()
        .map(|extension| {
            if viewer.is_none() {
                Vec::new()
            } else {
                extension.acl.workspaces.clone()
            }
        })
        .unwrap_or_default();
    let pages = wiki::wiki_search(
        &pipeline.pool,
        &req.query,
        &req.org_id,
        req.limit,
        viewer.as_deref(),
        &allowed_workspaces,
        &granted,
    )
    .await?;
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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<ContradictionsRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let results = contradictions::search_contradictions(
        &pipeline.pool,
        &req.org_id,
        req.query.as_deref(),
        req.limit,
        viewer.as_deref(),
        &granted,
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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<TimelineRequest>,
) -> Result<impl IntoResponse, AppError> {
    // GAP-1: pin org from the verified principal — never trust the body org_id.
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    if let Some(tid) = &req.trace_id {
        let actor = auth.as_ref().and_then(|extension| {
            if extension
                .scopes
                .iter()
                .any(|scope| scope == "org:data:read_all")
            {
                None
            } else {
                extension.user_id.as_deref()
            }
        });
        let detail = timeline::replay_trace(&pipeline.pool, tid, &req.org_id, actor).await?;
        return Ok(Json(serde_json::json!({"replay": detail})));
    }

    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let entries = timeline::temporal_search(
        &pipeline.pool,
        &req.org_id,
        viewer.as_deref(),
        &granted,
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
    #[serde(default)]
    zdr_mode: Option<ZdrMode>,
}

fn default_pack_budget() -> usize {
    4000
}
fn default_pack_format() -> String {
    "json".to_string()
}

fn pack_retrieval_request(
    mut req: PackRequest,
    auth: &crate::authz::AuthContext,
) -> Result<RetrievalRequest, AppError> {
    crate::authz::pin_org_from_ctx(Some(auth), &mut req.org_id)
        .map_err(|_| AppError::forbidden("authenticated tenant does not match requested tenant"))?;

    let mut retrieval = RetrievalRequest {
        query: req.query,
        org_id: req.org_id,
        top_k: None,
        top_n: req.top_n,
        filters: RetrievalFiltersInput::default(),
        context_budget_tokens: Some(req.context_budget_tokens),
        context_format: Some(req.context_format),
        zdr_mode: req.zdr_mode,
        user_id: None,
        verified_bearer: None,
        query_expansion: None,
        reranker_model: None,
        mode_mix: None,
        agent_id: None,
        admin_read_all: false,
    };
    auth.apply_to_request(&mut retrieval);
    Ok(retrieval)
}

async fn retrieve_pack(
    State(pipeline): State<AppState>,
    axum::extract::Extension(auth): axum::extract::Extension<crate::authz::AuthContext>,
    Json(req): Json<PackRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Claim tenant/user, ACL, admin visibility, and ZDR posture all flow through
    // the same RetrievalRequest consumed by the canonical pipeline.
    let retrieval_req = pack_retrieval_request(req, &auth)?;
    let resp = pipeline.retrieve(retrieval_req).await?;
    Ok(Json(serde_json::json!({
        "context_pack": resp.context_pack,
        "trace_id": resp.trace_id,
        "candidates_used": resp.candidates.len(),
    })))
}

// D7: Source lookup
const SOURCES_SQL: &str = "SELECT document_id, title, source, type, status, zdr_classification
     FROM documents
     WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL
       AND ($3::text IS NULL OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))";

const FRESHNESS_SQL: &str = "SELECT document_id, title, updated_at::TEXT, status
     FROM documents
     WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL
       AND ($3::text IS NULL OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))";

#[derive(serde::Deserialize)]
struct SourcesRequest {
    org_id: String,
    document_ids: Vec<String>,
}

async fn retrieve_sources(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<SourcesRequest>,
) -> Result<impl IntoResponse, AppError> {
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    if req.document_ids.is_empty() {
        return Ok(Json(serde_json::json!({"sources": []})));
    }
    // Per-user ownership: only return metadata for docs the viewer may see.
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(SOURCES_SQL)
        .bind(&req.document_ids)
        .bind(&req.org_id)
        .bind(&viewer)
        .bind(&granted)
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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<FreshnessRequest>,
) -> Result<impl IntoResponse, AppError> {
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    let rows = sqlx::query_as::<_, (String, String, String, String)>(FRESHNESS_SQL)
        .bind(&req.document_ids)
        .bind(&req.org_id)
        .bind(&viewer)
        .bind(&granted)
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
    auth: axum::extract::Extension<crate::authz::AuthContext>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<i32>, Option<i32>, Option<String>, String)>(
        "SELECT version_id, org_id, description, document_count, chunk_count, embedding_model, created_at::TEXT
         FROM index_versions WHERE org_id = $1 ORDER BY created_at DESC LIMIT 50"
    )
    .bind(&auth.org_id)
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
const CHUNKS_BY_IDS_SQL: &str =
    "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
     FROM knowledge_units ku
     JOIN documents d ON d.document_id = ku.document_id AND d.org_id = $2
     WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL
       AND ($3::text IS NULL OR d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))
     ORDER BY ku.chunk_index";

const CHUNKS_BY_DOCUMENT_SQL: &str =
    "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
     FROM knowledge_units ku
     JOIN documents d ON d.document_id = ku.document_id AND d.org_id = $2
     WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
       AND ($5::text IS NULL OR d.owner_id = $5 OR d.visibility = 'org' OR d.document_id = ANY($6))
     ORDER BY ku.chunk_index LIMIT $3 OFFSET $4";

fn chunk_by_ids_sql() -> &'static str {
    CHUNKS_BY_IDS_SQL
}

fn chunk_by_document_sql() -> &'static str {
    CHUNKS_BY_DOCUMENT_SQL
}

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
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<ChunksRequest>,
) -> Result<impl IntoResponse, AppError> {
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    // Per-user ownership: gate at the SQL level so chunk TEXT for a document the
    // viewer cannot see is never even fetched (this endpoint returns raw content).
    let (viewer, granted) = resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
    if let Some(kids) = &req.knowledge_ids {
        if !kids.is_empty() {
            let rows = sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                chunk_by_ids_sql(),
            )
            .bind(kids)
            .bind(&req.org_id)
            .bind(&viewer)
            .bind(&granted)
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
            chunk_by_document_sql(),
        )
        .bind(did)
        .bind(&req.org_id)
        .bind(req.limit)
        .bind(req.offset)
        .bind(&viewer)
        .bind(&granted)
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
type EntityCompareRow = (String, String, String, f64, String, serde_json::Value);
type RelationshipCompareRow = (String, String, String, String, f64);
type DocumentCompareRow = (String, String, String, String, String, String, String);

const ENTITY_COMPARE_SQL: &str =
    "SELECT ge.entity_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0),
            COALESCE(ge.provenance, ''),
            CASE WHEN $3::text IS NULL THEN COALESCE(ge.source_refs, '[]')
                 ELSE visible_refs.source_refs END
     FROM graph_entities ge
     LEFT JOIN LATERAL (
         SELECT COALESCE(jsonb_agg(sr.knowledge_id), '[]'::jsonb) AS source_refs
         FROM jsonb_array_elements_text(COALESCE(ge.source_refs, '[]')) AS sr(knowledge_id)
         JOIN knowledge_units ku ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = ge.org_id
         JOIN documents d ON d.document_id = ku.document_id AND d.org_id = ge.org_id
         WHERE d.deleted_at IS NULL
           AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))
     ) visible_refs ON TRUE
     WHERE ge.entity_id = ANY($1) AND ge.org_id = $2
       AND ($3::text IS NULL OR (
           jsonb_array_length(COALESCE(ge.source_refs, '[]')) > 0
           AND jsonb_array_length(COALESCE(ge.source_refs, '[]')) =
               jsonb_array_length(visible_refs.source_refs)
       ))";

const RELATIONSHIP_COMPARE_SQL: &str =
    "SELECT gr.rel_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type,
            COALESCE(gr.confidence, 0)
     FROM graph_relationships gr
     WHERE gr.org_id = $1
       AND gr.entity_a_id = ANY($2) AND gr.entity_b_id = ANY($2)
       AND ($3::text IS NULL OR (
           jsonb_array_length(COALESCE(gr.source_refs, '[]')) > 0
           AND NOT EXISTS (
               SELECT 1
               FROM jsonb_array_elements_text(COALESCE(gr.source_refs, '[]')) AS sr(knowledge_id)
               LEFT JOIN knowledge_units ku
                 ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = gr.org_id
               LEFT JOIN documents d
                 ON d.document_id = ku.document_id AND d.org_id = gr.org_id
               WHERE ku.knowledge_id IS NULL OR d.document_id IS NULL
                  OR d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4),
                      FALSE
                  )
           )
       ))";

const DOCUMENT_COMPARE_SQL: &str =
    "SELECT document_id, title, source, type, status, updated_at::TEXT, zdr_classification
     FROM documents
     WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL
       AND ($3::text IS NULL OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))";

fn entity_compare_sql() -> &'static str {
    ENTITY_COMPARE_SQL
}

fn relationship_compare_sql() -> &'static str {
    RELATIONSHIP_COMPARE_SQL
}

fn compared_document_ids(rows: &[DocumentCompareRow]) -> Vec<String> {
    rows.iter().map(|row| row.0.clone()).collect()
}

#[cfg(test)]
fn auxiliary_document_visibility_queries() -> [&'static str; 5] {
    [
        SOURCES_SQL,
        FRESHNESS_SQL,
        CHUNKS_BY_IDS_SQL,
        CHUNKS_BY_DOCUMENT_SQL,
        DOCUMENT_COMPARE_SQL,
    ]
}

#[derive(serde::Deserialize)]
struct CompareRequest {
    org_id: String,
    compare_type: String,
    ids: Vec<String>,
}

async fn retrieve_compare(
    State(pipeline): State<AppState>,
    auth: Option<axum::extract::Extension<crate::authz::AuthContext>>,
    Json(mut req): Json<CompareRequest>,
) -> Result<impl IntoResponse, AppError> {
    pin_request_org(auth.as_ref(), &mut req.org_id)?;
    match req.compare_type.as_str() {
        "entities" => {
            let (viewer, granted) =
                resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
            let rows = sqlx::query_as::<_, EntityCompareRow>(entity_compare_sql())
                .bind(&req.ids)
                .bind(&req.org_id)
                .bind(&viewer)
                .bind(&granted)
                .fetch_all(&pipeline.pool)
                .await?;

            // Relationships are constrained to both the visible entity set and
            // a source document the caller can see. Requested-but-hidden entity
            // IDs therefore cannot be used as an existence oracle.
            let visible_entity_ids: Vec<String> = rows
                .iter()
                .map(|(entity_id, ..)| entity_id.clone())
                .collect();

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
            let shared_rels = if visible_entity_ids.len() >= 2 {
                sqlx::query_as::<_, RelationshipCompareRow>(relationship_compare_sql())
                .bind(&req.org_id)
                .bind(&visible_entity_ids)
                .bind(&viewer)
                .bind(&granted)
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
            // Per-user ownership: only compare docs the viewer may see.
            let (viewer, granted) =
                resolve_viewer_grants(&pipeline, auth.as_ref(), &req.org_id).await;
            let rows = sqlx::query_as::<_, DocumentCompareRow>(DOCUMENT_COMPARE_SQL)
                .bind(&req.ids)
                .bind(&req.org_id)
                .bind(&viewer)
                .bind(&granted)
                .fetch_all(&pipeline.pool)
                .await?;

            // Count chunks only for rows already admitted by the canonical
            // document visibility predicate. Binding the caller-supplied IDs
            // directly would disclose the existence/size of hidden documents.
            let visible_document_ids = compared_document_ids(&rows);

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
            .bind(&visible_document_ids)
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
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default = "default_true")]
    dry_run: bool,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    idempotency_key: Option<String>,
}

struct AuthorizedAdminMutation {
    org_id: String,
    actor: String,
    request_id: String,
    dry_run: bool,
    reason: String,
    idempotency_key: Option<String>,
}

fn authorize_admin_mutation(
    ctx: &crate::authz::AuthContext,
    requested_org_id: Option<&str>,
    required_scope: &str,
    dry_run: bool,
    reason: &str,
    idempotency_key: Option<&str>,
) -> Result<AuthorizedAdminMutation, AppError> {
    if !ctx.scopes.iter().any(|scope| scope == required_scope) {
        return Err(AppError::forbidden("dedicated admin scope required"));
    }
    if requested_org_id
        .map(str::trim)
        .filter(|org| !org.is_empty())
        .is_some_and(|org| org != ctx.org_id)
    {
        return Err(AppError::forbidden(
            "admin operation tenant does not match verified claims",
        ));
    }
    let reason = reason.trim();
    if reason.len() < 8 || reason.len() > 500 {
        return Err(AppError::bad_request(
            "admin reason must be between 8 and 500 characters",
        ));
    }
    let idempotency_key = idempotency_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_owned);
    if !dry_run
        && idempotency_key
            .as_deref()
            .is_none_or(|key| key.len() < 8 || key.len() > 200)
    {
        return Err(AppError::bad_request(
            "idempotency_key of 8-200 characters is required for mutation",
        ));
    }
    let actor = ctx
        .user_id
        .clone()
        .ok_or_else(|| AppError::forbidden("verified admin user identity required"))?;
    Ok(AuthorizedAdminMutation {
        org_id: ctx.org_id.clone(),
        actor,
        request_id: ctx.request_id.clone(),
        dry_run,
        reason: reason.to_owned(),
        idempotency_key,
    })
}

async fn cleanup_orphans(
    State(pipeline): State<AppState>,
    auth: axum::extract::Extension<crate::authz::AuthContext>,
    Json(req): Json<CleanupOrphansRequest>,
) -> Result<impl IntoResponse, AppError> {
    let admin = authorize_admin_mutation(
        &auth,
        req.org_id.as_deref(),
        "data:admin:cleanup",
        req.dry_run,
        &req.reason,
        req.idempotency_key.as_deref(),
    )?;

    if admin.dry_run {
        let orphan_docs: Vec<(String,)> = sqlx::query_as(
            "SELECT document_id FROM documents WHERE org_id = $1 AND deleted_at IS NOT NULL",
        )
        .bind(&admin.org_id)
        .fetch_all(&pipeline.pool)
        .await?;
        let orphan_doc_ids: Vec<String> = orphan_docs.into_iter().map(|(id,)| id).collect();
        let orphan_chunk_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_units WHERE document_id = ANY($1) AND org_id = $2",
        )
        .bind(&orphan_doc_ids)
        .bind(&admin.org_id)
        .fetch_one(&pipeline.pool)
        .await?;
        crate::audit::record_admin(
            &pipeline.pool,
            crate::audit::AdminEvent {
                org_id: Some(&admin.org_id),
                actor: &admin.actor,
                action: "cleanup_orphans",
                target_kind: Some("org"),
                target_id: Some(&admin.org_id),
                request_id: Some(&admin.request_id),
                payload: Some(serde_json::json!({
                    "reason": admin.reason,
                    "dry_run": true,
                    "orphan_doc_count": orphan_doc_ids.len(),
                    "orphan_chunk_count": orphan_chunk_count,
                })),
                outcome: "ok",
                error: None,
            },
        )
        .await;
        return Ok(Json(serde_json::json!({
            "org_id": admin.org_id,
            "orphan_doc_count": orphan_doc_ids.len(),
            "orphan_chunk_count": orphan_chunk_count,
            "deleted_chunk_count": 0,
            "dry_run": true,
        }))
        .into_response());
    }

    let idempotency_key = admin
        .idempotency_key
        .as_deref()
        .expect("mutation validation requires idempotency key");
    let mut tx = pipeline.pool.begin().await?;
    let lock_key = format!("cleanup_orphans:{}", admin.org_id);
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&lock_key)
            .fetch_one(&mut *tx)
            .await?;
    if !locked {
        return Err(AppError::conflict(
            "another cleanup is already running for this tenant",
        ));
    }

    let previous: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM admin_audit_log
         WHERE org_id = $1 AND actor = $2 AND action = 'cleanup_orphans'
           AND outcome = 'ok' AND payload->>'idempotency_key' = $3
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&admin.org_id)
    .bind(&admin.actor)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(payload) = previous {
        tx.rollback().await?;
        return Ok(Json(serde_json::json!({
            "org_id": admin.org_id,
            "idempotent_replay": true,
            "result": payload,
        }))
        .into_response());
    }

    let orphan_docs: Vec<(String,)> = sqlx::query_as(
        "SELECT document_id FROM documents WHERE org_id = $1 AND deleted_at IS NOT NULL",
    )
    .bind(&admin.org_id)
    .fetch_all(&mut *tx)
    .await?;
    let orphan_doc_ids: Vec<String> = orphan_docs.into_iter().map(|(id,)| id).collect();
    let orphan_chunk_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_units WHERE document_id = ANY($1) AND org_id = $2",
    )
    .bind(&orphan_doc_ids)
    .bind(&admin.org_id)
    .fetch_one(&mut *tx)
    .await?;

    for doc_id in &orphan_doc_ids {
        let filter = qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("document_id", doc_id.clone()),
            qdrant_client::qdrant::Condition::matches("org_id", admin.org_id.clone()),
        ]);
        pipeline
            .qdrant
            .delete_points(
                qdrant_client::qdrant::DeletePointsBuilder::new(&pipeline.config.qdrant_collection)
                    .points(filter)
                    .wait(true),
            )
            .await?;
    }

    let deleted_chunk_count =
        sqlx::query("DELETE FROM knowledge_units WHERE document_id = ANY($1) AND org_id = $2")
            .bind(&orphan_doc_ids)
            .bind(&admin.org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    let payload = serde_json::json!({
        "reason": admin.reason,
        "idempotency_key": idempotency_key,
        "dry_run": false,
        "orphan_doc_count": orphan_doc_ids.len(),
        "orphan_chunk_count": orphan_chunk_count,
        "deleted_chunk_count": deleted_chunk_count,
    });
    sqlx::query(
        "INSERT INTO admin_audit_log
         (org_id, actor, action, target_kind, target_id, request_id, payload, outcome)
         VALUES ($1, $2, 'cleanup_orphans', 'org', $1, $3, $4, 'ok')",
    )
    .bind(&admin.org_id)
    .bind(&admin.actor)
    .bind(&admin.request_id)
    .bind(&payload)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({
        "org_id": admin.org_id,
        "orphan_doc_count": orphan_doc_ids.len(),
        "orphan_chunk_count": orphan_chunk_count,
        "deleted_chunk_count": deleted_chunk_count,
        "dry_run": false,
        "idempotent_replay": false,
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

    let cache_ok = match &pipeline.cache {
        Some(cache) => cache.health_check().await,
        None => true,
    };

    let all_ok = pg_ok && qdrant_ok && cache_ok;
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
            "checks": { "postgres": pg_ok, "qdrant": qdrant_ok, "cache": cache_ok }
        })),
    )
}

#[derive(Debug)]
struct AppError {
    error: anyhow::Error,
    status: StatusCode,
}

impl AppError {
    fn forbidden(message: &'static str) -> Self {
        Self {
            error: anyhow::anyhow!(message),
            status: StatusCode::FORBIDDEN,
        }
    }

    fn bad_request(message: &'static str) -> Self {
        Self {
            error: anyhow::anyhow!(message),
            status: StatusCode::BAD_REQUEST,
        }
    }

    fn conflict(message: &'static str) -> Self {
        Self {
            error: anyhow::anyhow!(message),
            status: StatusCode::CONFLICT,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        if self.status.is_server_error() {
            tracing::error!(err = %self.error, "request error");
        } else {
            tracing::warn!(err = %self.error, status = %self.status, "request rejected");
        }
        (
            self.status,
            Json(serde_json::json!({"error": self.error.to_string()})),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            error: err.into(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(test)]
mod admin_security_tests {
    use super::*;
    use crate::authz::{AuthContext, AuthMethod, EffectiveAcl};

    fn ctx(scopes: &[&str]) -> AuthContext {
        AuthContext {
            user_id: Some("admin-user".into()),
            org_id: "org-a".into(),
            auth_method: AuthMethod::Jwt,
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            acl: EffectiveAcl::allow_all(),
            request_id: "request-1".into(),
            verified_bearer: None,
        }
    }

    #[test]
    fn cleanup_requires_dedicated_scope_and_claim_tenant() {
        assert!(authorize_admin_mutation(
            &ctx(&[]),
            Some("org-a"),
            "data:admin:cleanup",
            true,
            "preview orphan cleanup",
            None,
        )
        .is_err());

        assert!(authorize_admin_mutation(
            &ctx(&["data:admin:cleanup"]),
            Some("org-b"),
            "data:admin:cleanup",
            true,
            "preview orphan cleanup",
            None,
        )
        .is_err());
    }

    #[test]
    fn destructive_admin_requires_reason_and_idempotency_but_preview_does_not() {
        let ctx = ctx(&["data:admin:cleanup"]);
        assert!(authorize_admin_mutation(
            &ctx,
            None,
            "data:admin:cleanup",
            false,
            "remove confirmed orphan rows",
            None,
        )
        .is_err());
        assert!(authorize_admin_mutation(
            &ctx,
            None,
            "data:admin:cleanup",
            false,
            "",
            Some("cleanup-1"),
        )
        .is_err());

        let preview = authorize_admin_mutation(
            &ctx,
            None,
            "data:admin:cleanup",
            true,
            "preview orphan cleanup",
            None,
        )
        .expect("safe preview");
        assert_eq!(preview.org_id, "org-a");
        assert!(preview.dry_run);
    }
}

#[cfg(test)]
mod auxiliary_security_tests {
    use super::*;
    use crate::authz::{AuthContext, AuthMethod, EffectiveAcl};

    fn ctx(org_id: &str, user_id: &str, scopes: &[&str]) -> AuthContext {
        AuthContext {
            user_id: Some(user_id.to_owned()),
            org_id: org_id.to_owned(),
            auth_method: AuthMethod::Jwt,
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            acl: EffectiveAcl::allow_all(),
            request_id: "aux-security-test".into(),
            verified_bearer: None,
        }
    }

    fn pack_request(value: serde_json::Value) -> PackRequest {
        serde_json::from_value(value).expect("valid pack request")
    }

    #[test]
    fn pack_rejects_conflicting_tenant_before_pipeline_access() {
        let request = pack_request(serde_json::json!({
            "org_id": "org-b",
            "query": "safe synthetic query"
        }));

        let error = pack_retrieval_request(request, &ctx("org-a", "user-a", &[]))
            .err()
            .expect("spoofed body tenant must be rejected");

        assert_eq!(error.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn pack_propagates_claim_identity_and_ephemeral_zdr() {
        let request = pack_request(serde_json::json!({
            "org_id": "org-a",
            "query": "safe synthetic query",
            "zdr_mode": "ephemeral"
        }));

        let retrieval =
            pack_retrieval_request(request, &ctx("org-a", "user-a", &["org:data:read_all"]))
                .expect("matching tenant");

        assert_eq!(retrieval.org_id, "org-a");
        assert_eq!(retrieval.user_id.as_deref(), Some("user-a"));
        assert_eq!(retrieval.zdr_mode, Some(ZdrMode::Ephemeral));
        assert!(retrieval.admin_read_all);
    }

    #[test]
    fn invalid_or_case_variant_pack_zdr_modes_fail_deserialization() {
        for mode in ["Ephemeral", "unknown", ""] {
            let result = serde_json::from_value::<PackRequest>(serde_json::json!({
                "org_id": "org-a",
                "query": "safe synthetic query",
                "zdr_mode": mode,
            }));
            assert!(result.is_err(), "mode {mode:?} must fail closed");
        }
    }

    #[test]
    fn entity_comparison_query_uses_canonical_document_visibility() {
        let sql = entity_compare_sql();

        assert!(sql.contains("jsonb_array_elements_text"));
        assert!(sql.contains("JOIN knowledge_units"));
        assert!(sql.contains("JOIN documents"));
        assert!(
            sql.contains("jsonb_agg"),
            "visible entities must not return source refs from hidden documents"
        );
        assert!(
            sql.contains("jsonb_array_length(COALESCE(ge.source_refs, '[]')) ="),
            "derived entity content must be hidden unless every source is visible"
        );
        assert!(sql.contains("owner_id = $3"));
        assert!(sql.contains("visibility = 'org'"));
        assert!(sql.contains("document_id = ANY($4)"));
        assert!(!sql.contains("visibility IN ('org', 'shared')"));

        let relationship_sql = relationship_compare_sql();
        assert!(relationship_sql.contains("jsonb_array_elements_text"));
        assert!(relationship_sql.contains("visibility = 'org'"));
        assert!(relationship_sql.contains("document_id = ANY($4)"));
        assert!(relationship_sql.contains("NOT EXISTS"));
        assert!(!relationship_sql.contains("visibility IN ('org', 'shared')"));
    }

    #[test]
    fn every_document_auxiliary_query_uses_grant_only_shared_visibility() {
        for sql in auxiliary_document_visibility_queries() {
            assert!(sql.contains("deleted_at IS NULL"), "{sql}");
            assert!(sql.contains("owner_id ="), "{sql}");
            assert!(sql.contains("visibility = 'org'"), "{sql}");
            assert!(sql.contains("document_id = ANY("), "{sql}");
            assert!(!sql.contains("visibility IN ('org', 'shared')"), "{sql}");
        }

        assert!(chunk_by_ids_sql().contains("ku.text"));
        assert!(chunk_by_document_sql().contains("ku.text"));
        assert!(chunk_by_ids_sql().contains("d.org_id = $2"));
        assert!(chunk_by_document_sql().contains("d.org_id = $2"));
    }

    #[test]
    fn document_chunk_counts_are_limited_to_visible_compare_rows() {
        let rows: Vec<DocumentCompareRow> = vec![
            (
                "visible-a".into(),
                "A".into(),
                "source".into(),
                "type".into(),
                "ready".into(),
                "2026-07-10 00:00:00+00".into(),
                "standard".into(),
            ),
            (
                "visible-b".into(),
                "B".into(),
                "source".into(),
                "type".into(),
                "ready".into(),
                "2026-07-10 00:00:00+00".into(),
                "standard".into(),
            ),
        ];

        assert_eq!(
            compared_document_ids(&rows),
            vec!["visible-a".to_string(), "visible-b".to_string()]
        );
    }
}
