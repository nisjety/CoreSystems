use std::collections::HashSet;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::{GraphCounts, GraphPreviewQuery, GraphPreviewResponse, PreviewEdge, PreviewNode},
    envelope::{error, ok, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

pub(crate) async fn graph_preview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<GraphPreviewQuery>,
) -> (StatusCode, Json<Value>) {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.is_empty() || query.org_id.trim() != org_id {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "The requested organization is not authorized for this session.",
            )),
        );
    }

    let url = format!(
        "{}/v1/graphs/{}?limit_nodes=2000&limit_edges=8000",
        state.graph_index_url,
        urlencoding::encode(&org_id)
    );

    let response = state
        .client
        .get(url)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-org-id", &org_id)
        .send()
        .await;

    let Ok(response) = response else {
        return empty_graph_response();
    };

    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let data = unwrap_data(&body);
    let nodes_in = data
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let edges_in = data
        .get("edges")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let nodes = nodes_in
        .iter()
        .filter_map(|entity| {
            let id = entity.get("entity_id").and_then(Value::as_str)?.to_owned();
            Some(PreviewNode {
                label: entity
                    .get("entity_text")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                group: entity
                    .get("entity_type")
                    .and_then(Value::as_str)
                    .unwrap_or("entity")
                    .to_lowercase(),
                id,
            })
        })
        .collect::<Vec<_>>();
    let node_ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let edges = edges_in
        .iter()
        .filter_map(|edge| {
            let a = edge.get("entity_a_id").and_then(Value::as_str)?.to_owned();
            let b = edge.get("entity_b_id").and_then(Value::as_str)?.to_owned();
            if !node_ids.contains(&a) || !node_ids.contains(&b) {
                return None;
            }
            Some(PreviewEdge { a, b })
        })
        .collect::<Vec<_>>();
    let groups = nodes
        .iter()
        .filter(|node| node.group != "org")
        .map(|node| node.group.clone())
        .collect::<HashSet<_>>();

    (
        StatusCode::OK,
        Json(ok(GraphPreviewResponse {
            counts: GraphCounts {
                nodes: data
                    .get("node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(nodes.len() as u64) as usize,
                edges: data
                    .get("edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(edges.len() as u64) as usize,
                groups: groups.len(),
            },
            nodes,
            edges,
        })),
    )
}

fn empty_graph_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(ok(GraphPreviewResponse {
            nodes: vec![],
            edges: vec![],
            counts: GraphCounts {
                nodes: 0,
                edges: 0,
                groups: 0,
            },
        })),
    )
}
