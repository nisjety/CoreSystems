use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{config::AppState, domains::knowledge::shared, middleware::AuthenticatedUser};

fn pin_org(mut body: Value, org_id: Option<&str>) -> Value {
    if let (Some(object), Some(org_id)) = (body.as_object_mut(), org_id) {
        object.insert("org_id".to_owned(), Value::String(org_id.to_owned()));
    }
    body
}

fn normalize_search_request(body: Value, org_id: Option<&str>) -> Value {
    let mut input = body.as_object().cloned().unwrap_or_default();
    let top_k = input
        .get("top_k")
        .or_else(|| input.get("limit"))
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 100);
    let mut filters = input
        .remove("filters")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if !filters.contains_key("document_types") {
        if let Some(kinds) = input.remove("kinds").filter(Value::is_array) {
            filters.insert("document_types".to_owned(), kinds);
        }
    }
    input.insert("top_k".to_owned(), Value::from(top_k));
    input.insert("filters".to_owned(), Value::Object(filters));
    input.remove("limit");
    pin_org(Value::Object(input), org_id)
}

fn search_response(body: Value) -> Value {
    let sources = body
        .get("sources")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|source| {
                    let document_id = source.get("document_id")?.as_str()?.to_owned();
                    Some((document_id, source.clone()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();
    let results = body
        .get("candidates")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|candidate| {
                    let document_id = candidate
                        .get("document_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let source = sources.get(document_id);
                    json!({
                        "id": candidate.get("knowledge_id").and_then(Value::as_str).unwrap_or(document_id),
                        "title": source.and_then(|value| value.get("title")).and_then(Value::as_str).unwrap_or("Knowledge result"),
                        "excerpt": candidate.get("text").and_then(Value::as_str).unwrap_or_default(),
                        "score": candidate.get("final_score").and_then(Value::as_f64).unwrap_or(0.0),
                        "kind": source.and_then(|value| value.get("type")).and_then(Value::as_str).unwrap_or("document"),
                        "sourceId": document_id,
                        "path": format!("/knowledge?source={}", urlencoding::encode(document_id)),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut response = Map::new();
    response.insert("total".to_owned(), Value::from(results.len()));
    response.insert("results".to_owned(), Value::Array(results));
    for key in ["trace_id", "index_version", "low_confidence", "zdr_mode"] {
        if let Some(value) = body.get(key) {
            response.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(response)
}

pub(super) async fn search_knowledge(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/knowledge/search", state.retrieval_engine_url);
    let body = normalize_search_request(body, org_id.as_deref());
    let (status, Json(response)) = shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, Json(search_response(response)))
    } else {
        (status, Json(response))
    }
}

pub(super) async fn get_retrieval_trace(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(trace_id): Path<String>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/retrieval/{}",
        state.retrieval_engine_url,
        urlencoding::encode(&trace_id)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn resolve_sources(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/retrieve/sources", state.retrieval_engine_url);
    let body = pin_org(body, org_id.as_deref());
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn expand_chunks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/retrieve/chunks", state.retrieval_engine_url);
    let body = pin_org(body, org_id.as_deref());
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn graph_retrieve(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/retrieve/graph", state.retrieval_engine_url);
    let body = pin_org(body, org_id.as_deref());
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn wiki_retrieve(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/retrieve/wiki", state.retrieval_engine_url);
    let body = pin_org(body, org_id.as_deref());
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{normalize_search_request, search_response};
    use serde_json::json;

    #[test]
    fn spa_search_contract_maps_to_retrieval_request() {
        let request = normalize_search_request(
            json!({ "query": "budget", "limit": 6, "kinds": ["pdf"] }),
            Some("org-a"),
        );
        assert_eq!(request["org_id"], "org-a");
        assert_eq!(request["top_k"], 6);
        assert_eq!(request["filters"]["document_types"], json!(["pdf"]));
        assert!(request.get("limit").is_none());
        assert!(request.get("kinds").is_none());
    }

    #[test]
    fn retrieval_candidates_map_to_spa_search_results() {
        let response = search_response(json!({
            "candidates": [{
                "knowledge_id": "chunk-1",
                "document_id": "doc-1",
                "text": "Budget evidence",
                "final_score": 0.91
            }],
            "sources": [{
                "document_id": "doc-1",
                "title": "Budget plan",
                "source": "sharepoint",
                "type": "pdf"
            }],
            "trace_id": "trace-1"
        }));
        assert_eq!(response["total"], 1);
        assert_eq!(response["results"][0]["id"], "chunk-1");
        assert_eq!(response["results"][0]["title"], "Budget plan");
        assert_eq!(response["results"][0]["kind"], "pdf");
        assert_eq!(response["trace_id"], "trace-1");
    }
}
