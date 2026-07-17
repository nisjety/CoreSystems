use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::{bearer_from_http, AuthError, JwtVerifier, Principal},
    neo4j::{self, Neo4jClient},
    store::GraphStore,
};

type AppState = Arc<GraphStore>;

/// Request-scoped, config-derived bounds for multi-hop traversal. Injected as an
/// `Extension` so both the Neo4j and Postgres-fallback paths clamp identically.
#[derive(Clone, Copy)]
pub struct GraphLimits {
    pub max_hops: u8,
    pub max_entities: i64,
}

async fn require_verified_principal(
    verifier: Arc<JwtVerifier>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = bearer_from_http(req.headers()).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let principal = verifier.verify(token).map_err(|error| match error {
        AuthError::InsufficientScope => StatusCode::FORBIDDEN,
        _ => StatusCode::UNAUTHORIZED,
    })?;

    if let Some(header_org_id) = req.headers().get("x-org-id") {
        let header_org_id = header_org_id.to_str().map_err(|_| StatusCode::FORBIDDEN)?;
        if !principal.authorizes_org(header_org_id) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    req.extensions_mut().insert(principal);
    Ok(next.run(req).await)
}

fn require_org<'a>(
    principal: &'a Principal,
    requested_org_id: &str,
) -> Result<&'a str, StatusCode> {
    if principal.authorizes_org(requested_org_id) {
        Ok(&principal.org_id)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn store_failure(operation: &'static str, error: impl std::fmt::Display) -> StatusCode {
    tracing::error!(operation, error = %error, "graph store operation failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

pub fn router(
    store: Arc<GraphStore>,
    verifier: Arc<JwtVerifier>,
    neo4j: Option<Arc<Neo4jClient>>,
    limits: GraphLimits,
) -> Router {
    let public = Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz));

    let protected = Router::new()
        // D4+D5 spec §3.1 — aggregate graph snapshot for one org.
        .route("/v1/graphs/{org_id}", get(get_org_graph))
        .route("/v1/graph/entities/{entity_id}", get(get_entity))
        .route("/v1/graph/entities", get(list_entities))
        .route(
            "/v1/graph/relationships/{entity_id}",
            get(get_relationships),
        )
        .route("/v1/graph/claims", get(get_claims))
        .route("/v1/graph/contradictions", get(get_contradictions))
        .route("/v1/graph/expand", post(expand_graph))
        // GraphRAG multi-hop traversal (Neo4j read-model, Postgres fallback).
        .route("/v1/graph/traverse", post(traverse_graph))
        // On-demand re-detection of the org's derived communities.
        .route("/v1/graph/communities/rebuild", post(rebuild_communities))
        // §16.1.5 — graph_exports endpoint. Formats: json (default),
        // graphml, markdown.
        .route("/v1/graph/exports", post(create_export))
        .layer(Extension(neo4j))
        .layer(Extension(limits))
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let verifier = verifier.clone();
            async move { require_verified_principal(verifier, req, next).await }
        }));

    public.merge(protected).with_state(store)
}

#[derive(Deserialize)]
struct ExportRequest {
    org_id: String,
    #[serde(default = "default_export_format")]
    format: String,
    #[serde(default = "default_export_limit_nodes")]
    limit_nodes: i32,
    #[serde(default = "default_export_limit_edges")]
    limit_edges: i32,
}

fn default_export_format() -> String {
    "json".to_string()
}
fn default_export_limit_nodes() -> i32 {
    5000
}
fn default_export_limit_edges() -> i32 {
    20000
}

async fn create_export(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(req): Json<ExportRequest>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use crate::model::{Entity, Relationship};
    use axum::http::{header, StatusCode};

    let org_id = require_org(&principal, &req.org_id)?;

    let (nodes, edges, _n_total, _e_total): (Vec<Entity>, Vec<Relationship>, i64, i64) = store
        .snapshot_org_graph(org_id, req.limit_nodes, req.limit_edges)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "snapshot_org_graph failed for export");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let (body, ext) = match req.format.as_str() {
        "graphml" => (render_graphml(&nodes, &edges), "graphml"),
        "markdown" | "md" => (render_markdown(org_id, &nodes, &edges), "md"),
        _ => (
            serde_json::json!({
                "org_id": org_id,
                "entities": nodes,
                "relationships": edges,
            })
            .to_string(),
            "json",
        ),
    };

    let mut resp = axum::response::Response::new(axum::body::Body::from(body.clone()));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(match ext {
            "graphml" => "application/xml",
            "md" => "text/markdown",
            _ => "application/json",
        }),
    );
    if let Ok(disp) = axum::http::HeaderValue::from_str(&format!(
        "attachment; filename=graph-{}-{}.{}",
        org_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        ext
    )) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, disp);
    }
    Ok(resp)
}

fn render_graphml(nodes: &[crate::model::Entity], edges: &[crate::model::Relationship]) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\">\n\
           <graph edgedefault=\"directed\">\n",
    );
    for n in nodes {
        out.push_str(&format!(
            "    <node id=\"{}\"><data key=\"label\">{}</data></node>\n",
            xml_escape(&n.entity_id),
            xml_escape(&n.entity_text),
        ));
    }
    for e in edges {
        out.push_str(&format!(
            "    <edge source=\"{}\" target=\"{}\"><data key=\"type\">{}</data></edge>\n",
            xml_escape(&e.entity_a_id),
            xml_escape(&e.entity_b_id),
            xml_escape(&e.relation_type),
        ));
    }
    out.push_str("  </graph>\n</graphml>\n");
    out
}

fn render_markdown(
    org_id: &str,
    nodes: &[crate::model::Entity],
    edges: &[crate::model::Relationship],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Graph export — org `{}`\n\n", org_id));
    out.push_str(&format!(
        "- entities: {}\n- relationships: {}\n\n",
        nodes.len(),
        edges.len()
    ));
    out.push_str("## Entities\n\n");
    for n in nodes {
        out.push_str(&format!(
            "- `{}` — **{}** ({})\n",
            n.entity_id, n.entity_text, n.entity_type
        ));
    }
    out.push_str("\n## Relationships\n\n");
    for e in edges {
        out.push_str(&format!(
            "- `{}` --[{}]--> `{}`\n",
            e.entity_a_id, e.relation_type, e.entity_b_id
        ));
    }
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Deserialize)]
struct SnapshotQuery {
    #[serde(default = "default_snapshot_nodes")]
    limit_nodes: i32,
    #[serde(default = "default_snapshot_edges")]
    limit_edges: i32,
}

fn default_snapshot_nodes() -> i32 {
    5000
}
fn default_snapshot_edges() -> i32 {
    20000
}

/// `GET /v1/graphs/{org_id}` — D4+D5 spec §3.1 aggregator.
/// Returns `{nodes:[], edges:[], node_count, edge_count, org_id}`.
/// If the org's graph exceeds `limit_nodes` or `limit_edges`, the response
/// includes a `truncated: true` field so callers know to use the paginated
/// endpoints; we still return the partial slice rather than 413 because
/// that is what the spec acceptance row expects.
async fn get_org_graph(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(org_id): Path<String>,
    Query(q): Query<SnapshotQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &org_id)?;
    match store
        .snapshot_org_graph(org_id, q.limit_nodes, q.limit_edges)
        .await
    {
        Ok((nodes, edges, n_total, e_total)) => {
            let truncated = (n_total as i32) > q.limit_nodes || (e_total as i32) > q.limit_edges;
            Ok(Json(serde_json::json!({
                "org_id": org_id,
                "nodes": nodes,
                "edges": edges,
                "node_count": n_total,
                "edge_count": e_total,
                "truncated": truncated,
            })))
        }
        Err(e) => Err(store_failure("snapshot_org_graph", e)),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "graph-index-rs"}))
}

async fn readyz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ready", "service": "graph-index-rs"}))
}

#[derive(Deserialize)]
struct OrgQuery {
    org_id: String,
}

#[derive(Deserialize)]
struct ListEntitiesQuery {
    org_id: String,
    entity_type: String,
    #[serde(default = "default_limit")]
    limit: i32,
    #[serde(default)]
    offset: i32,
}

fn default_limit() -> i32 {
    50
}

#[derive(Deserialize)]
struct RelQuery {
    org_id: String,
    relation_type: Option<String>,
}

#[derive(Deserialize)]
struct ClaimsQuery {
    org_id: String,
    entity_id: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct ContradictionsQuery {
    org_id: String,
    #[serde(default = "default_limit")]
    limit: i32,
    #[serde(default)]
    offset: i32,
}

#[derive(Deserialize)]
struct ExpandRequest {
    org_id: String,
    entity_ids: Vec<String>,
    #[serde(default = "default_max_hops")]
    max_hops: i32,
    #[serde(default = "default_max_entities")]
    max_entities: i32,
}

fn default_max_hops() -> i32 {
    2
}

fn default_max_entities() -> i32 {
    50
}

async fn get_entity(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(entity_id): Path<String>,
    Query(q): Query<OrgQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &q.org_id)?;
    match store.get_entity(org_id, &entity_id).await {
        Ok(Some(e)) => Ok(Json(serde_json::json!({"entity": e}))),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(store_failure("get_entity", e)),
    }
}

async fn list_entities(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ListEntitiesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &q.org_id)?;
    match store
        .list_entities_by_type(org_id, &q.entity_type, q.limit, q.offset)
        .await
    {
        Ok((entities, total)) => Ok(Json(
            serde_json::json!({"entities": entities, "total": total}),
        )),
        Err(e) => Err(store_failure("list_entities_by_type", e)),
    }
}

async fn get_relationships(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(entity_id): Path<String>,
    Query(q): Query<RelQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &q.org_id)?;
    match store
        .get_relationships(org_id, &entity_id, q.relation_type.as_deref())
        .await
    {
        Ok(rels) => Ok(Json(serde_json::json!({"relationships": rels}))),
        Err(e) => Err(store_failure("get_relationships", e)),
    }
}

async fn get_claims(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ClaimsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &q.org_id)?;
    match store
        .get_claims(org_id, q.entity_id.as_deref(), q.status.as_deref())
        .await
    {
        Ok(claims) => Ok(Json(serde_json::json!({"claims": claims}))),
        Err(e) => Err(store_failure("get_claims", e)),
    }
}

async fn get_contradictions(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ContradictionsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &q.org_id)?;
    match store.get_contradictions(org_id, q.limit, q.offset).await {
        Ok((claims, total)) => Ok(Json(
            serde_json::json!({"contradictions": claims, "total": total}),
        )),
        Err(e) => Err(store_failure("get_contradictions", e)),
    }
}

async fn expand_graph(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(req): Json<ExpandRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &req.org_id)?;
    match store
        .get_graph_expansion(org_id, &req.entity_ids, req.max_hops, req.max_entities)
        .await
    {
        Ok((entities, rels)) => Ok(Json(serde_json::json!({
            "entities": entities,
            "relationships": rels,
            "hops_traversed": req.max_hops,
            "new_entities_found": entities.len(),
        }))),
        Err(e) => Err(store_failure("get_graph_expansion", e)),
    }
}

#[derive(Deserialize)]
struct TraverseRequest {
    org_id: String,
    #[serde(default)]
    seed_entity_ids: Vec<String>,
    #[serde(default)]
    max_hops: Option<u8>,
    #[serde(default)]
    max_entities: Option<u32>,
}

/// GraphRAG multi-hop traversal. Resolves connected entities from seeds via the
/// Neo4j read-model (native `*1..N` Cypher), then re-joins Postgres to enforce
/// provenance/org-visibility and attach chunk source_refs. Falls back to the
/// Postgres BFS (`get_graph_expansion`) when Neo4j is disabled/unreachable, so
/// the endpoint is always available; Neo4j only changes traversal speed.
async fn traverse_graph(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(neo4j): Extension<Option<Arc<Neo4jClient>>>,
    Extension(limits): Extension<GraphLimits>,
    Json(req): Json<TraverseRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Pin org from the verified principal; reject a body/bearer org mismatch.
    let org_id = require_org(&principal, &req.org_id)?;

    if req.seed_entity_ids.is_empty() {
        return Ok(Json(serde_json::json!({
            "entities": [],
            "relationships": [],
            "backend": "none",
            "hops": 0,
        })));
    }

    let max_hops = neo4j::clamp_hops(req.max_hops.unwrap_or(limits.max_hops), limits.max_hops);
    let max_entities = req
        .max_entities
        .map(i64::from)
        .unwrap_or(limits.max_entities)
        .clamp(1, limits.max_entities);

    // Neo4j path: native multi-hop, then Postgres provenance/visibility re-join.
    if let Some(client) = neo4j.as_ref() {
        match client
            .traverse(org_id, &req.seed_entity_ids, max_hops, max_entities)
            .await
        {
            Ok(reached) => {
                let ids: Vec<String> = reached.iter().map(|(id, _)| id.clone()).collect();
                let hop_by_id: std::collections::HashMap<&str, u8> =
                    reached.iter().map(|(id, h)| (id.as_str(), *h)).collect();
                match store.get_subgraph_visible(org_id, &ids).await {
                    Ok((entities, rels)) => {
                        return Ok(Json(serde_json::json!({
                            "entities": entities_with_hops(&entities, &hop_by_id),
                            "relationships": rels,
                            "backend": "neo4j",
                            "hops": max_hops,
                        })));
                    }
                    Err(e) => return Err(store_failure("get_subgraph_visible", e)),
                }
            }
            Err(e) => {
                // Non-fatal: fall through to the Postgres BFS so a Neo4j hiccup
                // never fails graph retrieval.
                tracing::warn!(err = %e, "neo4j traverse failed; falling back to postgres BFS");
            }
        }
    }

    // Postgres BFS fallback (also the path when Neo4j is disabled). Already
    // org-visible; hop distance is not tracked here, so callers treat it as 1.
    match store
        .get_graph_expansion(
            org_id,
            &req.seed_entity_ids,
            i32::from(max_hops),
            max_entities.clamp(0, i64::from(i32::MAX)) as i32,
        )
        .await
    {
        Ok((entities, rels)) => Ok(Json(serde_json::json!({
            "entities": entities_with_hops(&entities, &std::collections::HashMap::new()),
            "relationships": rels,
            "backend": "postgres",
            "hops": max_hops,
        }))),
        Err(e) => Err(store_failure("graph_traverse_fallback", e)),
    }
}

#[derive(Deserialize)]
struct RebuildCommunitiesRequest {
    org_id: String,
    #[serde(default = "default_community_min_size")]
    min_size: usize,
}

fn default_community_min_size() -> usize {
    3
}

/// Re-detects the org's derived communities (connected components over the
/// visibility-gated graph) and atomically replaces `graph_communities`. Also
/// runs automatically after each document's extraction; this endpoint covers
/// backfill and operator-triggered refresh.
async fn rebuild_communities(
    State(store): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(req): Json<RebuildCommunitiesRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let org_id = require_org(&principal, &req.org_id)?;
    let min_size = req.min_size.clamp(2, 100);
    match crate::community::detect_communities(&store, org_id, min_size).await {
        Ok(count) => Ok(Json(serde_json::json!({
            "communities": count,
            "min_size": min_size,
        }))),
        Err(e) => Err(store_failure("rebuild_communities", e)),
    }
}

/// Serializes entities with an attached `hops` distance. Missing hops (Postgres
/// fallback, which does not track per-entity distance) default to 1.
fn entities_with_hops(
    entities: &[crate::model::Entity],
    hop_by_id: &std::collections::HashMap<&str, u8>,
) -> Vec<serde_json::Value> {
    entities
        .iter()
        .map(|e| {
            let hops = hop_by_id.get(e.entity_id.as_str()).copied().unwrap_or(1);
            serde_json::json!({
                "entity_id": e.entity_id,
                "org_id": e.org_id,
                "entity_type": e.entity_type,
                "entity_text": e.entity_text,
                "confidence": e.confidence,
                "source_refs": e.source_refs,
                "hops": hops,
            })
        })
        .collect()
}

#[cfg(test)]
mod auth_tests {
    use std::{sync::OnceLock, time::Duration};

    use axum::{
        body::Body,
        http::{header::AUTHORIZATION, Method, Request},
    };
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
        RsaPrivateKey, RsaPublicKey,
    };
    use serde::Serialize;
    use tower::ServiceExt;

    use super::*;

    const ISSUER: &str = "https://control.test/api/convex-auth";
    const AUDIENCE: &str = "data-plane";

    struct TestAuth {
        verifier: Arc<JwtVerifier>,
        encoding: EncodingKey,
    }

    fn test_auth() -> &'static TestAuth {
        static AUTH: OnceLock<TestAuth> = OnceLock::new();
        AUTH.get_or_init(|| {
            let private = RsaPrivateKey::new(&mut thread_rng(), 2048).expect("generate test key");
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode private key");
            let public_pem = RsaPublicKey::from(&private)
                .to_public_key_pem(LineEnding::LF)
                .expect("encode public key");
            TestAuth {
                verifier: Arc::new(
                    JwtVerifier::from_pem(ISSUER, AUDIENCE, public_pem.as_bytes())
                        .expect("build verifier"),
                ),
                encoding: EncodingKey::from_rsa_pem(private_pem.as_bytes())
                    .expect("build encoding key"),
            }
        })
    }

    #[derive(Serialize)]
    struct UserClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        sub: &'a str,
        user_id: &'a str,
        org_id: &'a str,
        exp: usize,
        nbf: usize,
    }

    fn valid_token(org_id: &str) -> String {
        let now = chrono::Utc::now().timestamp() as usize;
        encode(
            &Header::new(Algorithm::RS256),
            &UserClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "user-1",
                user_id: "user-1",
                org_id,
                exp: now + 300,
                nbf: now.saturating_sub(5),
            },
            &test_auth().encoding,
        )
        .expect("sign token")
    }

    struct RouteCase {
        method: Method,
        uri: &'static str,
        body: Option<&'static str>,
    }

    fn route_cases(org_id: &str) -> Vec<RouteCase> {
        let graph_uri = if org_id == "org-a" {
            "/v1/graphs/org-a"
        } else {
            "/v1/graphs/org-b"
        };
        let entity_uri = if org_id == "org-a" {
            "/v1/graph/entities/entity-1?org_id=org-a"
        } else {
            "/v1/graph/entities/entity-1?org_id=org-b"
        };
        let entities_uri = if org_id == "org-a" {
            "/v1/graph/entities?org_id=org-a&entity_type=Person"
        } else {
            "/v1/graph/entities?org_id=org-b&entity_type=Person"
        };
        let relationships_uri = if org_id == "org-a" {
            "/v1/graph/relationships/entity-1?org_id=org-a"
        } else {
            "/v1/graph/relationships/entity-1?org_id=org-b"
        };
        let claims_uri = if org_id == "org-a" {
            "/v1/graph/claims?org_id=org-a"
        } else {
            "/v1/graph/claims?org_id=org-b"
        };
        let contradictions_uri = if org_id == "org-a" {
            "/v1/graph/contradictions?org_id=org-a"
        } else {
            "/v1/graph/contradictions?org_id=org-b"
        };
        let expand_body = if org_id == "org-a" {
            r#"{"org_id":"org-a","entity_ids":["entity-1"]}"#
        } else {
            r#"{"org_id":"org-b","entity_ids":["entity-1"]}"#
        };
        let traverse_body = if org_id == "org-a" {
            r#"{"org_id":"org-a","seed_entity_ids":["entity-1"]}"#
        } else {
            r#"{"org_id":"org-b","seed_entity_ids":["entity-1"]}"#
        };
        let communities_body = if org_id == "org-a" {
            r#"{"org_id":"org-a"}"#
        } else {
            r#"{"org_id":"org-b"}"#
        };
        let export_body = if org_id == "org-a" {
            r#"{"org_id":"org-a","format":"json"}"#
        } else {
            r#"{"org_id":"org-b","format":"json"}"#
        };

        vec![
            RouteCase {
                method: Method::GET,
                uri: graph_uri,
                body: None,
            },
            RouteCase {
                method: Method::GET,
                uri: entity_uri,
                body: None,
            },
            RouteCase {
                method: Method::GET,
                uri: entities_uri,
                body: None,
            },
            RouteCase {
                method: Method::GET,
                uri: relationships_uri,
                body: None,
            },
            RouteCase {
                method: Method::GET,
                uri: claims_uri,
                body: None,
            },
            RouteCase {
                method: Method::GET,
                uri: contradictions_uri,
                body: None,
            },
            RouteCase {
                method: Method::POST,
                uri: "/v1/graph/expand",
                body: Some(expand_body),
            },
            RouteCase {
                method: Method::POST,
                uri: "/v1/graph/traverse",
                body: Some(traverse_body),
            },
            RouteCase {
                method: Method::POST,
                uri: "/v1/graph/communities/rebuild",
                body: Some(communities_body),
            },
            RouteCase {
                method: Method::POST,
                uri: "/v1/graph/exports",
                body: Some(export_body),
            },
        ]
    }

    fn test_router() -> Router {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(1))
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .expect("lazy pool");
        router(
            Arc::new(GraphStore::new(pool)),
            test_auth().verifier.clone(),
            None,
            GraphLimits {
                max_hops: 3,
                max_entities: 100,
            },
        )
    }

    fn request(case: &RouteCase) -> axum::http::request::Builder {
        Request::builder()
            .method(case.method.clone())
            .uri(case.uri)
            .header("content-type", "application/json")
    }

    #[tokio::test]
    async fn route_matrix_rejects_missing_and_header_only_identity() {
        for case in route_cases("org-b") {
            let no_auth = request(&case)
                .body(case.body.map_or_else(Body::empty, Body::from))
                .expect("request");
            assert_eq!(
                test_router()
                    .oneshot(no_auth)
                    .await
                    .expect("response")
                    .status(),
                StatusCode::UNAUTHORIZED,
                "no-auth route {}",
                case.uri
            );

            let forged_header = request(&case)
                .header("x-org-id", "org-b")
                .body(case.body.map_or_else(Body::empty, Body::from))
                .expect("request");
            assert_eq!(
                test_router()
                    .oneshot(forged_header)
                    .await
                    .expect("response")
                    .status(),
                StatusCode::UNAUTHORIZED,
                "header-only route {}",
                case.uri
            );
        }
    }

    #[tokio::test]
    async fn route_matrix_denies_verified_principal_spoofing_another_tenant() {
        let token = valid_token("org-a");
        for case in route_cases("org-b") {
            let body_or_path_spoof = request(&case)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(case.body.map_or_else(Body::empty, Body::from))
                .expect("request");
            assert_eq!(
                test_router()
                    .oneshot(body_or_path_spoof)
                    .await
                    .expect("response")
                    .status(),
                StatusCode::FORBIDDEN,
                "body/path tenant pin route {}",
                case.uri
            );

            let header_spoof = request(&case)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header("x-org-id", "org-b")
                .body(case.body.map_or_else(Body::empty, Body::from))
                .expect("request");
            assert_eq!(
                test_router()
                    .oneshot(header_spoof)
                    .await
                    .expect("response")
                    .status(),
                StatusCode::FORBIDDEN,
                "header tenant pin route {}",
                case.uri
            );
        }
    }

    #[tokio::test]
    async fn route_matrix_accepts_verified_principal_for_claim_tenant() {
        let token = valid_token("org-a");
        for case in route_cases("org-a") {
            let request = request(&case)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header("x-org-id", "org-a")
                .body(case.body.map_or_else(Body::empty, Body::from))
                .expect("request");
            let status = test_router()
                .oneshot(request)
                .await
                .expect("response")
                .status();
            assert_eq!(
                status,
                StatusCode::INTERNAL_SERVER_ERROR,
                "an unavailable graph store must fail honestly for own-org route {}",
                case.uri
            );
        }
    }
}
