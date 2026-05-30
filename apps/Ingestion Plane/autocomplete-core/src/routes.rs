use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::trace::TraceLayer;

use crate::{
    app::{AppState, SuggestionScope, SuggestionSet},
    normalization::{
        bounded_limit, bucket_for_org, normalize_host, normalize_query, stable_object,
    },
    store::IndexedObject,
    AppError, AppResult,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/v1/suggestions", get(suggestions))
        .route("/v1/internal/push", post(push))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<Envelope<HealthData>> {
    Json(Envelope::data(HealthData { status: "ok" }))
}

async fn ready(State(state): State<AppState>) -> Json<Envelope<ReadyData>> {
    Json(Envelope::data(ReadyData {
        status: "ready",
        sonic_enabled: state.sonic.enabled(),
        nats_enabled: state.settings.nats.enabled,
    }))
}

async fn suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SuggestionQuery>,
) -> AppResult<Json<Envelope<SuggestionSet>>> {
    authorize(&state, &headers)?;
    let org_id = org_id(&headers, query.org_id.as_deref())?;
    let scope = SuggestionScope::parse(query.scope.as_deref());
    let result = state
        .suggestions(&org_id, &query.q, scope, query.limit)
        .await?;
    Ok(Json(Envelope::data(result)))
}

async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PushRequest>,
) -> AppResult<(StatusCode, Json<Envelope<PushResponse>>)> {
    authorize(&state, &headers)?;

    let org_id = org_id(&headers, payload.org_id.as_deref())?;
    let bucket = bucket_for_org(&org_id);
    let source = payload.source.unwrap_or_else(|| "manual".to_string());
    let collection = payload.collection.unwrap_or_else(|| "queries".to_string());
    let text = match collection.as_str() {
        "hosts" => normalize_host(&payload.text),
        _ => normalize_query(&payload.text),
    }
    .ok_or_else(|| AppError::Validation("text must not be empty".to_string()))?;
    let object = payload
        .object
        .unwrap_or_else(|| stable_object(&source, &text.to_ascii_lowercase()));

    let item = IndexedObject {
        collection,
        bucket,
        object,
        text,
        source,
        target_url: payload.target_url,
        metadata: payload.metadata.unwrap_or_else(|| json!({})),
    };

    state.ingest.index_manual(item.clone()).await?;

    Ok((
        StatusCode::CREATED,
        Json(Envelope::data(PushResponse {
            collection: item.collection,
            object: item.object,
        })),
    ))
}

fn authorize(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    let Some(expected) = state.settings.internal_token.as_deref() else {
        return Ok(());
    };

    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if bearer == Some(expected) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}

fn org_id(headers: &HeaderMap, query_org_id: Option<&str>) -> AppResult<String> {
    let header_org = headers
        .get("x-org-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let org_id = header_org
        .or(query_org_id
            .map(str::trim)
            .filter(|value| !value.is_empty()))
        .ok_or_else(|| AppError::Validation("x-org-id header is required".to_string()))?;

    Ok(org_id.to_string())
}

#[derive(Debug, Deserialize)]
struct SuggestionQuery {
    q: String,
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct PushRequest {
    #[serde(default)]
    org_id: Option<String>,
    text: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target_url: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct Envelope<T> {
    data: T,
}

impl<T> Envelope<T> {
    fn data(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Serialize)]
struct HealthData {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ReadyData {
    status: &'static str,
    sonic_enabled: bool,
    nats_enabled: bool,
}

#[derive(Debug, Serialize)]
struct PushResponse {
    collection: String,
    object: String,
}

#[allow(dead_code)]
fn _limit_for_docs(limit: Option<usize>) -> usize {
    bounded_limit(limit, 8, 20)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::{
        config::{NatsSettings, Settings, SonicSettings},
        sonic::SonicClient,
        store::MetadataStore,
    };

    use super::*;

    #[tokio::test]
    async fn push_then_suggest_returns_hydrated_suggestion() {
        let store = MetadataStore::in_memory().unwrap();
        let settings = test_settings();
        let sonic = SonicClient::new(settings.sonic.clone());
        let app = router(AppState::for_tests(store, sonic, settings));

        let push_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/internal/push")
                    .header("x-org-id", "org_a")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"text":"Find me Grocery store","collection":"queries","source":"query"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(push_response.status(), StatusCode::CREATED);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/suggestions?org_id=org_a&q=Find%20me&scope=queries")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["data"]["suggestions"][0]["text"],
            "Find me Grocery store"
        );
    }

    fn test_settings() -> Settings {
        Settings {
            http_addr: "127.0.0.1:0".parse().unwrap(),
            metadata_db_path: ":memory:".into(),
            internal_token: None,
            sonic: SonicSettings {
                enabled: false,
                addr: "127.0.0.1:1491".to_string(),
                password: String::new(),
                timeout: std::time::Duration::from_secs(1),
            },
            nats: NatsSettings {
                enabled: false,
                url: "nats://127.0.0.1:4222".to_string(),
                stream_name: "QUARRY_EVENTS".to_string(),
                durable_name: "autocomplete-core-test".to_string(),
                subject_filter: "quarry.events.*".to_string(),
                auth_token: None,
            },
        }
    }
}
