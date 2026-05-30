//! /v1/search — SERP-backed discovery (QRY-12).
//!
//! Wraps the runtime's `SearchProvider` trait. The edge route normalizes
//! request shape, enforces ZDR (search queries are control-plane signals
//! and never persisted, so ZDR=on is fine), and returns ranked URLs ready
//! for crawl seeding via `/v1/crawl` or one-shot scrape.
//!
//! When no provider is configured, returns 501 Unsupported with a hint
//! instead of 404 — clients can detect "no SERP backend wired" cleanly.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_runtime::serp::{SearchOptions, SearchResult};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default = "default_safe_search")]
    pub safe_search: bool,
    /// Optional client-supplied request ID for tracing.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub request_id: Option<String>,
}

fn default_safe_search() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub provider: String,
    pub results: Vec<SearchResult>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(provider) = &state.search else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "no SERP provider configured".into(),
                code: "UNSUPPORTED".into(),
                hint: Some(
                    "set BRAVE_SEARCH_KEY, SERPER_KEY, or SEARXNG_URL in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    let opts = SearchOptions {
        limit: req.limit.unwrap_or(10).min(50),
        country: req.country.clone(),
        language: req.language.clone(),
        safe_search: req.safe_search,
        // Tenant isolation: thread the verified JWT org_id into the
        // search call so providers that hold private corpora
        // (TantivyLocalIndex) restrict results to this org. Public-web
        // providers (Brave/Serper/SearXNG/Stract) ignore the field.
        org_id: Some(claims.org_id.clone()),
    };

    match provider.search(&req.query, &opts).await {
        Ok(results) => {
            let count = results.len();
            let provider_name = provider.name().to_string();
            // Cycle 19 / cluster #19: emit SearchIssued for autocomplete-core
            // consumption. Fire-and-forget — never block the search
            // response on event emission.
            let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
            let run_id_str = run_id.to_string();
            let idem = format!("search:{}", run_id);
            state
                .event_sink
                .emit(
                    run_id,
                    quarry_core::event::EventType::SearchIssued,
                    serde_json::json!({
                        "query": req.query,
                        "provider": provider_name,
                        "result_count": count,
                        "limit": opts.limit,
                        "org_id": claims.org_id,
                        "user_id": claims.user_id,
                    }),
                    idem,
                )
                .await;

            // P3 / cluster #billing — emit a per-query usage record so
            // billing-core can meter the call. One unit per query
            // regardless of how many results came back; downstream
            // pricing (Lago billable metric `quarry.search.query`) can
            // include `result_count` from metadata if it wants
            // granular per-result accounting.
            state
                .usage
                .meter(quarry_runtime::UsageEvent::new(
                    run_id_str,
                    claims.org_id.clone(),
                    quarry_runtime::usage_metrics::SEARCH_QUERY,
                    1.0,
                    serde_json::json!({
                        "user_id": claims.user_id,
                        "result_count": count,
                        "provider": provider_name,
                        "query_chars": req.query.chars().count(),
                    }),
                ))
                .await;
            (
                StatusCode::OK,
                Json(SearchResponse {
                    query: req.query,
                    provider: provider_name,
                    results,
                    count,
                }),
            )
                .into_response()
        }
        Err(e) => {
            let status = match e.code.http_status() {
                400 => StatusCode::BAD_REQUEST,
                401 => StatusCode::UNAUTHORIZED,
                403 => StatusCode::FORBIDDEN,
                429 => StatusCode::TOO_MANY_REQUESTS,
                502 => StatusCode::BAD_GATEWAY,
                504 => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
    use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
    use std::sync::Arc;

    struct FakeProvider {
        name: String,
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl SearchProvider for FakeProvider {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.results.clone())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    struct ErrProvider;

    #[async_trait]
    impl SearchProvider for ErrProvider {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Err(QuarryError::new(ErrorCode::RateLimited, "throttled"))
        }
        fn name(&self) -> &str {
            "err"
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_two_results() {
        let p = FakeProvider {
            name: "fake".into(),
            results: vec![
                SearchResult {
                    url: "https://x.com/a".into(),
                    title: Some("A".into()),
                    snippet: None,
                    rank: 1,
                    provider: "fake".into(),
                },
                SearchResult {
                    url: "https://x.com/b".into(),
                    title: None,
                    snippet: None,
                    rank: 2,
                    provider: "fake".into(),
                },
            ],
        };
        let arc: Arc<dyn SearchProvider> = Arc::new(p);
        let res = arc
            .search("anything", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(res.len(), 2);
    }

    #[tokio::test]
    async fn rate_limited_provider_yields_typed_error() {
        let p: Arc<dyn SearchProvider> = Arc::new(ErrProvider);
        let err = p
            .search("anything", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }
}
