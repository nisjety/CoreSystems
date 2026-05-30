use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::store::GraphStore;

type AppState = Arc<GraphStore>;

pub fn router(store: Arc<GraphStore>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
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
        .with_state(store)
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
    Json(req): Json<ExportRequest>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use crate::model::{Entity, Relationship};
    use axum::http::{header, StatusCode};

    let (nodes, edges, _n_total, _e_total): (Vec<Entity>, Vec<Relationship>, i64, i64) = store
        .snapshot_org_graph(&req.org_id, req.limit_nodes, req.limit_edges)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "snapshot_org_graph failed for export");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let (body, ext) = match req.format.as_str() {
        "graphml" => (render_graphml(&nodes, &edges), "graphml"),
        "markdown" | "md" => (render_markdown(&req.org_id, &nodes, &edges), "md"),
        _ => (
            serde_json::json!({
                "org_id": req.org_id,
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
        req.org_id,
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
    Path(org_id): Path<String>,
    Query(q): Query<SnapshotQuery>,
) -> Json<serde_json::Value> {
    match store
        .snapshot_org_graph(&org_id, q.limit_nodes, q.limit_edges)
        .await
    {
        Ok((nodes, edges, n_total, e_total)) => {
            let truncated = (n_total as i32) > q.limit_nodes || (e_total as i32) > q.limit_edges;
            Json(serde_json::json!({
                "org_id": org_id,
                "nodes": nodes,
                "edges": edges,
                "node_count": n_total,
                "edge_count": e_total,
                "truncated": truncated,
            }))
        }
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
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
    Path(entity_id): Path<String>,
    Query(q): Query<OrgQuery>,
) -> Json<serde_json::Value> {
    match store.get_entity(&q.org_id, &entity_id).await {
        Ok(Some(e)) => Json(serde_json::json!({"entity": e})),
        Ok(None) => Json(serde_json::json!({"error": "not found"})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn list_entities(
    State(store): State<AppState>,
    Query(q): Query<ListEntitiesQuery>,
) -> Json<serde_json::Value> {
    match store
        .list_entities_by_type(&q.org_id, &q.entity_type, q.limit, q.offset)
        .await
    {
        Ok((entities, total)) => Json(serde_json::json!({"entities": entities, "total": total})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn get_relationships(
    State(store): State<AppState>,
    Path(entity_id): Path<String>,
    Query(q): Query<RelQuery>,
) -> Json<serde_json::Value> {
    match store
        .get_relationships(&q.org_id, &entity_id, q.relation_type.as_deref())
        .await
    {
        Ok(rels) => Json(serde_json::json!({"relationships": rels})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn get_claims(
    State(store): State<AppState>,
    Query(q): Query<ClaimsQuery>,
) -> Json<serde_json::Value> {
    match store
        .get_claims(&q.org_id, q.entity_id.as_deref(), q.status.as_deref())
        .await
    {
        Ok(claims) => Json(serde_json::json!({"claims": claims})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn get_contradictions(
    State(store): State<AppState>,
    Query(q): Query<ContradictionsQuery>,
) -> Json<serde_json::Value> {
    match store.get_contradictions(&q.org_id, q.limit, q.offset).await {
        Ok((claims, total)) => Json(serde_json::json!({"contradictions": claims, "total": total})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn expand_graph(
    State(store): State<AppState>,
    Json(req): Json<ExpandRequest>,
) -> Json<serde_json::Value> {
    match store
        .get_graph_expansion(&req.org_id, &req.entity_ids, req.max_hops, req.max_entities)
        .await
    {
        Ok((entities, rels)) => Json(serde_json::json!({
            "entities": entities,
            "relationships": rels,
            "hops_traversed": req.max_hops,
            "new_entities_found": entities.len(),
        })),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}
