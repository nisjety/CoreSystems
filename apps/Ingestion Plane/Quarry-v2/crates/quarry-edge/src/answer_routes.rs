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
//! - `403 Forbidden` with `reason: zdr_unavailable` under effective Zero Data Retention (the body
//!   flag or the token's floor). The Model Plane hop carries no ZDR posture (ADR-4), so a ZDR
//!   request is refused BEFORE the pipeline starts: no search, no page fetch, no model call
//! - `501 Unsupported` when no answer pipeline is configured on the edge. INNH-8a: the body
//!   carries `reason`, the FIRST closed gate (`synthesis_disabled`, `no_search_provider`,
//!   `no_model_plane_url`, `model_client_init_failed`), and a `hint` that names the exact switch
//! - `503 Service Unavailable` with `reason: model_plane_auth_refused` when the pipeline is wired
//!   but the Model Plane refuses the edge's service token (a deployment fact, never a 403 on the
//!   caller). Other model-hop failures keep their status and gain a `reason`

use axum::response::sse::{Event, Sse};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_runtime::answer::{AnswerRequest, AnswerResult};

use crate::answer_status::{self, AnswerReason};
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
    /// INNH-8a: a closed code from [`AnswerReason`], never provider text. Additive; absent on
    /// errors that have none (a 400 for an empty query).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// 501 for a pipeline that is not wired, naming the first closed gate.
fn not_configured(reason: AnswerReason) -> axum::response::Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            serde_json::to_value(ErrorBody {
                error: "answer pipeline not configured".into(),
                code: "UNSUPPORTED".into(),
                hint: Some(reason.hint().into()),
                reason: Some(reason.as_str()),
            })
            .unwrap(),
        ),
    )
        .into_response()
}

/// 403 for a ZDR request. Nothing has run: the Model Plane hop has no ZDR posture (ADR-4), so
/// the query and page text must not leave for it.
fn zdr_refused() -> axum::response::Response {
    let reason = AnswerReason::ZdrUnavailable;
    (
        StatusCode::FORBIDDEN,
        Json(
            serde_json::to_value(ErrorBody {
                error: "answer synthesis is not available under ZDR (zero data retention)".into(),
                code: quarry_core::error::ErrorCode::Forbidden.wire_name().into(),
                hint: Some(reason.hint().into()),
                reason: Some(reason.as_str()),
            })
            .unwrap(),
        ),
    )
        .into_response()
}

/// 403 for a request whose web policy cannot be decided (an unreadable token claim under
/// `enforce`). Nothing has run. The text is the policy engine's closed message, never a URL.
fn policy_refused(error: quarry_core::error::QuarryError) -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(
            serde_json::to_value(ErrorBody {
                error: error.message,
                code: quarry_core::error::ErrorCode::Forbidden.wire_name().into(),
                hint: None,
                reason: None,
            })
            .unwrap(),
        ),
    )
        .into_response()
}

/// D2-055: the URL gate of one answer request. The pipeline asks it for every search-result URL
/// before fetching it; a URL the request's web policy denies is skipped (typed
/// `policy_denied`), counted like any other enforced denial, and never fetched. The decision is
/// taken once, with the effective ZDR posture, so the ZDR floor applies the same as on the
/// scrape family. The policy fingerprint is untouched: this reads the decision, it adds no layer.
fn url_gate(
    state: &AppState,
    claims: &crate::auth::Claims,
    zdr: bool,
) -> Result<quarry_runtime::answer::UrlGate, quarry_core::error::QuarryError> {
    let decision = crate::web_policy_edge::decide(&state.policy, claims, zdr, None)?;
    Ok(std::sync::Arc::new(move |url: &str| {
        crate::web_policy_edge::gate_url(&decision, url).is_ok()
    }))
}

/// The response for a failed `AnswerPipeline::answer`. A refused service token is a 503 with a
/// fixed message (the model gateway's own text stays out); every other failure keeps the status
/// its error code always had and gains the closed `reason`.
fn failed(e: quarry_core::error::QuarryError) -> axum::response::Response {
    let reason = answer_status::classify_failure(&e);
    if reason == AnswerReason::ModelPlaneAuthRefused {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                serde_json::to_value(ErrorBody {
                    error: "the Model Plane refused quarry-edge's service token".into(),
                    code: "UNAVAILABLE".into(),
                    hint: Some(reason.hint().into()),
                    reason: Some(reason.as_str()),
                })
                .unwrap(),
            ),
        )
            .into_response();
    }
    (
        StatusCode::from_u16(e.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(
            serde_json::to_value(ErrorBody {
                error: e.message,
                code: e.code.wire_name().to_string(),
                hint: None,
                reason: Some(reason.as_str()),
            })
            .unwrap(),
        ),
    )
        .into_response()
}

/// `POST /v1/answer`. WS4.J: wraps `answer_run` so a ZDR run's metadata-only tombstone is
/// sent once, from the response status (off unless the flag is on; fail open). The
/// streamed variant (`answer_stream`) ends inside its stream, so it sends its tombstone
/// from the stream's end instead (D1-003, [`crate::zdr_tombstone::StreamEnd`]).
pub async fn answer(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<AnswerHttpRequest>,
) -> axum::response::Response {
    let pending = crate::zdr_tombstone::Pending::begin(
        state.tombstones.as_ref(),
        claims.effective_zdr(req.zdr.unwrap_or(false)),
        &claims.org_id,
        crate::zdr_tombstone::Kind::Answer,
    );
    let response = answer_run(State(state), Extension(claims), Json(req))
        .await
        .into_response();
    if let Some(pending) = pending {
        pending.finish_status(response.status().as_u16());
    }
    response
}

async fn answer_run(
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
                    reason: None,
                })
                .unwrap(),
            ),
        )
            .into_response();
    }

    // ADR-4: refused before the pipeline starts, so a ZDR request makes no search, no page fetch
    // and no model call (the gateway double sees 0 requests). Ahead of the gate so a ZDR caller
    // never learns the deployment's model path.
    if claims.effective_zdr(req.zdr.unwrap_or(false)) {
        return zdr_refused();
    }

    let pipeline = match answer_status::gate(&state) {
        Ok(pipeline) => pipeline,
        Err(reason) => return not_configured(reason),
    };

    let gate = match url_gate(&state, &claims, claims.effective_zdr(req.zdr.unwrap_or(false))) {
        Ok(gate) => gate,
        Err(error) => return policy_refused(error),
    };

    let pipeline_req = AnswerRequest {
        query: req.query,
        top_k: req.top_k,
        country: req.country,
        language: req.language,
        // Body flag raised (never lowered) by the token's ZDR floor: see
        // `Claims::zdr_floor`.
        zdr: Some(claims.effective_zdr(req.zdr.unwrap_or(false))),
        // Tenant isolation: server-asserted from verified JWT — clients
        // cannot bypass this even if they invent an `org_id` field.
        org_id: Some(claims.org_id.clone()),
    };

    match pipeline.answer_gated(pipeline_req, Some(&gate)).await {
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
        Err(e) => failed(e),
    }
}

/// `POST /v1/answer/stream` — SSE variant of `/v1/answer`. Runs the SAME
/// pipeline (hybrid search → full-page fetch → grounded context) and streams
/// synthesis token-by-token, preserving full-page grounding AND answer caching.
/// Frames: `event: citations` (array), `event: delta` ({delta}), repeated,
/// then `event: done` ({answer, cached}); `event: error` ({message}) on failure.
/// A warm org-scoped answer-cache hit replays instantly with no LLM call; a ZDR
/// request (body flag or the token's ZDR floor) neither reads nor writes it.
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
                    reason: None,
                })
                .unwrap(),
            ),
        )
            .into_response();
    }
    // D1-003: a streamed ZDR answer leaves its tombstone from the stream's end, whether the
    // stream finishes, fails or is dropped by a client that went away. Off unless the
    // tombstone flag is on (it stays off until counsel signs it off); a standard run is None.
    let mut tombstone = crate::zdr_tombstone::StreamEnd::new(crate::zdr_tombstone::Pending::begin(
        state.tombstones.as_ref(),
        claims.effective_zdr(req.zdr.unwrap_or(false)),
        &claims.org_id,
        crate::zdr_tombstone::Kind::Answer,
    ));
    // ADR-4: same refusal as `/v1/answer`, before any work. The tombstone still records the status.
    if claims.effective_zdr(req.zdr.unwrap_or(false)) {
        tombstone.status(StatusCode::FORBIDDEN.as_u16());
        return zdr_refused();
    }
    let pipeline = match answer_status::gate(&state) {
        Ok(pipeline) => pipeline.clone(),
        Err(reason) => {
            tombstone.status(StatusCode::NOT_IMPLEMENTED.as_u16());
            return not_configured(reason);
        }
    };

    let gate = match url_gate(&state, &claims, claims.effective_zdr(req.zdr.unwrap_or(false))) {
        Ok(gate) => gate,
        Err(error) => {
            tombstone.status(StatusCode::FORBIDDEN.as_u16());
            return policy_refused(error);
        }
    };

    let query = req.query.clone();
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();
    // Body flag raised (never lowered) by the token's ZDR floor: see
    // `Claims::zdr_floor`.
    let zdr = claims.effective_zdr_mode(req.zdr);
    let pipeline_req = AnswerRequest {
        query: query.clone(),
        top_k: req.top_k,
        country: req.country.clone(),
        language: req.language.clone(),
        zdr: Some(zdr.is_active()),
        // Tenant isolation: server-asserted from the verified JWT.
        org_id: Some(org_id.clone()),
    };

    // A ZDR answer neither reads nor writes the answer cache: the cached value
    // is the synthesized answer text itself, keyed only by org and query, so a
    // ZDR request must not be served from (or leave behind) a durable copy.
    let scache = if zdr.is_active() {
        None
    } else {
        state.redis.clone().map(crate::cache::SearchCache::new)
    };
    let cache_key = crate::cache::SearchCache::key(&org_id, &query, "answer:stream:v1");
    let usage = state.usage.clone();

    let sse = Sse::new(async_stream::stream! {
        // Held for the life of the stream: dropped (and sent) when the stream ends or is dropped.
        let mut tombstone = tombstone;
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

        let prepared = match pipeline.prepare_gated(&pipeline_req, Some(&gate)).await {
            Ok(p) => p,
            Err(e) => {
                tombstone.failed(&e);
                let reason = answer_status::classify(false, e.code).as_str();
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({ "message": e.message, "reason": reason }).to_string()));
                return;
            }
        };

        let citations_json =
            serde_json::to_string(&prepared.citations).unwrap_or_else(|_| "[]".into());
        yield Ok(Event::default().event("citations").data(citations_json));

        if prepared.sources_used == 0 {
            tombstone.ok();
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
        match pipeline
            .formats()
            .query_for_org(&org_id, &prepared.combined, &query, zdr)
            .await
        {
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
                tombstone.failed(&e);
                let reason = answer_status::classify(true, e.code).as_str();
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({ "message": e.message, "reason": reason }).to_string()));
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

        tombstone.ok();
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

    /// QB-T17: with `QUARRY_EDGE__ANSWER_SYNTHESIS` off there is no pipeline, so `/v1/answer` answers
    /// 501 and says which switch turns it on (the hint is what an operator reads at the 501).
    #[tokio::test]
    async fn answer_without_a_pipeline_names_the_switch() {
        use crate::test_support::{claims_for_org, test_state, StubDriver};

        let state = test_state(StubDriver::ok());
        let request: AnswerHttpRequest =
            serde_json::from_value(serde_json::json!({ "query": "rust async" })).unwrap();

        let response = answer(
            State(state),
            Extension(claims_for_org("org_answer")),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("QUARRY_EDGE__ANSWER_SYNTHESIS"), "{body}");
    }

    /// The `code` of an error body is the wire name CONTRACTS.md and ERROR_REASONS.md promise
    /// (`DRIVER_FAILED`, `RATE_LIMITED`), never the Rust identifier upper-cased (`DRIVERFAILED`).
    #[tokio::test]
    async fn a_failure_body_carries_the_wire_name_of_its_error_code() {
        use quarry_core::error::{ErrorCode, QuarryError};
        use quarry_runtime::answer::{MODEL_HOP, MODEL_HOP_KEY};

        for (code, status, wire, reason) in [
            (ErrorCode::DriverFailed, 502, "DRIVER_FAILED", "model_plane_error"),
            (ErrorCode::RateLimited, 429, "RATE_LIMITED", "model_plane_rate_limited"),
            (ErrorCode::Timeout, 504, "TIMEOUT", "model_plane_error"),
        ] {
            let error = QuarryError::new(code, "x")
                .with_details(serde_json::json!({ MODEL_HOP_KEY: MODEL_HOP }));
            let response = failed(error);
            assert_eq!(response.status().as_u16(), status, "{wire}");
            let body = axum::body::to_bytes(response.into_body(), 1 << 16)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["code"], wire);
            assert_eq!(body["reason"], reason);
            assert_eq!(body["code"], code.wire_name());
        }
    }

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
