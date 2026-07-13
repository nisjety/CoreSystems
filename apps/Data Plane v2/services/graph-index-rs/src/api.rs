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
    store::GraphStore,
};

type AppState = Arc<GraphStore>;

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

pub fn router(store: Arc<GraphStore>, verifier: Arc<JwtVerifier>) -> Router {
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
        // §16.1.5 — graph_exports endpoint. Formats: json (default),
        // graphml, markdown.
        .route("/v1/graph/exports", post(create_export))
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Ok(None) => Ok(Json(serde_json::json!({"error": "not found"}))),
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
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
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
    }
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
            assert_ne!(
                status,
                StatusCode::UNAUTHORIZED,
                "own-org route {}",
                case.uri
            );
            assert_ne!(status, StatusCode::FORBIDDEN, "own-org route {}", case.uri);
        }
    }
}
