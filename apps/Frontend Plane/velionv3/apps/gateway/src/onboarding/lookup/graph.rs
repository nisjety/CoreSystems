use std::collections::HashSet;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::{GraphCounts, GraphPreviewQuery, GraphPreviewResponse, PreviewEdge, PreviewNode},
    envelope::{ok, unwrap_data},
};

pub(crate) async fn graph_preview(
    State(state): State<AppState>,
    Query(query): Query<GraphPreviewQuery>,
) -> (StatusCode, Json<Value>) {
    if query.org_id.trim().is_empty() {
        return empty_graph_response();
    }

    let url = format!(
        "{}/v1/graphs/{}?limit_nodes=2000&limit_edges=8000",
        state.graph_index_url,
        urlencoding::encode(query.org_id.trim())
    );

    let response = state
        .client
        .get(url)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-org-id", query.org_id.trim())
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
