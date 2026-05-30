//! `/v1/answer` — Tavily-shape one-call answer endpoint.
//!
//! Cycle 19 / gap-quarry cluster #18. The edge route validates the request,
//! delegates to the runtime's [`AnswerPipeline`], and returns
//! `{answer, citations, sources_used, sources_skipped, model, latency_ms}`.
//!
//! Returns:
//! - `200` with answer JSON on success (including empty-answer when no
//!   sources matched or no fetches succeeded — clients see a typed shape
//!   rather than an error envelope)
//! - `400` for empty queries
//! - `501 Unsupported` when no answer pipeline is configured on the edge
//!   (i.e. no SearchProvider or no Model Plane URL)

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_runtime::answer::{AnswerRequest, AnswerResult};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AnswerHttpRequest {
    pub query: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub zdr: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub async fn answer(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<AnswerHttpRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::to_value(ErrorBody {
                    error: "query must not be empty".into(),
                    code: "BAD_REQUEST".into(),
                    hint: None,
                })
                .unwrap(),
            ),
        )
            .into_response();
    }

    let Some(pipeline) = state.answer_pipeline.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(
                serde_json::to_value(ErrorBody {
                    error: "answer pipeline not configured".into(),
                    code: "UNSUPPORTED".into(),
                    hint: Some(
                        "configure a SearchProvider AND a Model Plane URL to enable /v1/answer"
                            .into(),
                    ),
                })
                .unwrap(),
            ),
        )
            .into_response();
    };

    let pipeline_req = AnswerRequest {
        query: req.query,
        top_k: req.top_k,
        country: req.country,
        language: req.language,
        zdr: req.zdr,
        // Tenant isolation: server-asserted from verified JWT — clients
        // cannot bypass this even if they invent an `org_id` field.
        org_id: Some(claims.org_id.clone()),
    };

    match pipeline.answer(pipeline_req).await {
        Ok(result) => {
            // P3 / cluster #billing — meter the synth call. quantity=1
            // for the synthesis itself; per-citation cost (sources_used)
            // rides in metadata so Lago can choose a single-event or
            // dimensional pricing model.
            let usage_request_id = format!(
                "answer-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            );
            state
                .usage
                .meter(quarry_runtime::UsageEvent::new(
                    usage_request_id,
                    claims.org_id.clone(),
                    quarry_runtime::usage_metrics::ANSWER_SYNTH,
                    1.0,
                    serde_json::json!({
                        "user_id": claims.user_id,
                        "sources_used": result.sources_used,
                        "sources_skipped": result.sources_skipped,
                        "model": result.model,
                        "latency_ms": result.latency_ms,
                    }),
                ))
                .await;
            (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::from_u16(e.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(
                serde_json::to_value(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                })
                .unwrap(),
            ),
        )
            .into_response(),
    }
}

// Re-export the runtime AnswerResult so the OpenAPI / SDK generators
// can find it in the edge crate's module tree without an extra hop.
#[allow(unused_imports)] // deliberate re-export for OpenAPI/SDK codegen
pub use quarry_runtime::answer::AnswerResult as ApiAnswerResult;

#[allow(dead_code)]
fn _type_check_marker(_a: AnswerResult) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_request_decodes_minimal_shape() {
        let raw = serde_json::json!({"query": "rust async"});
        let r: AnswerHttpRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.query, "rust async");
        assert!(r.top_k.is_none());
    }

    #[test]
    fn http_request_decodes_full_shape() {
        let raw = serde_json::json!({
            "query": "rust async",
            "top_k": 3,
            "country": "us",
            "language": "en",
            "zdr": true
        });
        let r: AnswerHttpRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(r.top_k, Some(3));
        assert_eq!(r.zdr, Some(true));
    }
}
