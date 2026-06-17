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

use axum::response::sse::{Event, Sse};
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

/// `POST /v1/answer/stream` — SSE variant of `/v1/answer`. Runs the SAME
/// pipeline (hybrid search → full-page fetch → grounded context) and streams
/// synthesis token-by-token, preserving full-page grounding AND answer caching.
/// Frames: `event: citations` (array), `event: delta` ({delta}), repeated,
/// then `event: done` ({answer, cached}); `event: error` ({message}) on failure.
/// A warm org-scoped answer-cache hit replays instantly with no LLM call.
pub async fn answer_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<AnswerHttpRequest>,
) -> axum::response::Response {
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
    let Some(pipeline) = state.answer_pipeline.clone() else {
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

    let query = req.query.clone();
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();
    let zdr = quarry_core::zdr::ZdrMode::from(req.zdr.unwrap_or(false));
    let pipeline_req = AnswerRequest {
        query: query.clone(),
        top_k: req.top_k,
        country: req.country.clone(),
        language: req.language.clone(),
        zdr: req.zdr,
        // Tenant isolation: server-asserted from the verified JWT.
        org_id: Some(org_id.clone()),
    };

    let scache = state.redis.clone().map(crate::cache::SearchCache::new);
    let cache_key = crate::cache::SearchCache::key(&org_id, &query, "answer:stream:v1");
    let usage = state.usage.clone();

    let sse = Sse::new(async_stream::stream! {
        // Warm answer-cache hit → replay instantly, skip search + synthesis.
        if let Some(c) = &scache {
            if let Some(cached) = c.get::<String>(&cache_key).await {
                yield Ok::<Event, std::convert::Infallible>(Event::default().event("citations").data("[]"));
                if !cached.is_empty() {
                    yield Ok(Event::default()
                        .event("delta")
                        .data(serde_json::json!({ "delta": cached }).to_string()));
                }
                yield Ok(Event::default()
                    .event("done")
                    .data(serde_json::json!({ "answer": cached, "cached": true }).to_string()));
                return;
            }
        }

        let prepared = match pipeline.prepare(&pipeline_req).await {
            Ok(p) => p,
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({ "message": e.message }).to_string()));
                return;
            }
        };

        let citations_json =
            serde_json::to_string(&prepared.citations).unwrap_or_else(|_| "[]".into());
        yield Ok(Event::default().event("citations").data(citations_json));

        if prepared.sources_used == 0 {
            yield Ok(Event::default()
                .event("done")
                .data(serde_json::json!({ "answer": "", "cached": false }).to_string()));
            return;
        }

        // The model-gateway's /v1/invoke/stream is currently a stub (returns an
        // immediate empty `done`: model_used="default", 0 tokens), so synthesize
        // the full answer via the WORKING non-streaming /v1/invoke (`query`) and
        // reveal it to the client in word-chunked delta frames. `query_stream` /
        // `invoke_stream` stay wired and switch on to TRUE token streaming once
        // the gateway implements it (swap `query` → `query_stream` here).
        let full: String;
        match pipeline.formats().query(&prepared.combined, &query, zdr).await {
            Ok(qr) => {
                full = qr.answer;
                // Emit ~4-word delta frames (whitespace preserved) for a smooth
                // streaming reveal; the client concatenates them back verbatim.
                let mut frame = String::new();
                let mut words_in_frame = 0usize;
                for token in full.split_inclusive(char::is_whitespace) {
                    frame.push_str(token);
                    words_in_frame += 1;
                    if words_in_frame >= 4 {
                        yield Ok(Event::default().event("delta").data(
                            serde_json::json!({ "delta": std::mem::take(&mut frame) }).to_string(),
                        ));
                        words_in_frame = 0;
                    }
                }
                if !frame.is_empty() {
                    yield Ok(Event::default()
                        .event("delta")
                        .data(serde_json::json!({ "delta": frame }).to_string()));
                }
            }
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({ "message": e.message }).to_string()));
                return;
            }
        }

        // Best-effort cache write-back + usage meter (only when we got an answer).
        if !full.is_empty() {
            if let Some(c) = &scache {
                let _ = c.put_with_ttl(&cache_key, &full, 3600).await;
            }
            usage
                .meter(quarry_runtime::UsageEvent::new(
                    format!(
                        "answer-stream-{}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    ),
                    org_id.clone(),
                    quarry_runtime::usage_metrics::ANSWER_SYNTH,
                    1.0,
                    serde_json::json!({
                        "user_id": user_id,
                        "sources_used": prepared.sources_used,
                        "streamed": true,
                    }),
                ))
                .await;
        }

        yield Ok(Event::default()
            .event("done")
            .data(serde_json::json!({ "answer": full, "cached": false }).to_string()));
    });

    sse.into_response()
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
