//! Review as a run (F-t3-086): the HTTP surface of [`crate::review_run`].
//!
//! `POST /v1/ai/review` takes one text (`target`, `label`, `material`, `brief`, `locale`, `zdr`)
//! and answers with one card under `data.run`. The route answers 404 `review_run_disabled` while
//! `MODEL_GATEWAY_REVIEW_RUN_ENABLED` is not the literal `true`. An input the gateway cannot review
//! is a typed 400, or 413 for material over its limit, and makes no model call. A zero-retention
//! turn is honoured: the flag reaches inference-core and the cost is null. A waived token is refused
//! as a route mismatch, since a review names no connection. Nothing is stored; the card goes back to
//! the caller only.

use axum::{extract::State, http::StatusCode, routing::post, Extension, Json, Router};
use mp_contracts::user_content::UserContent;
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

use crate::auth::{Claims, VerifiedInferenceBearer};
use crate::http_routes::{authenticated_inference_request, grpc_status_to_http, HttpJsonError};
use crate::pricing::CostBasis;
use crate::review_run::{
    self, card_json, parse_review_reply, review_request, InputError, RunUsage,
};
use crate::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/ai/review", post(post_review_run))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ReviewRunRequest {
    target: String,
    label: String,
    material: UserContent,
    brief: UserContent,
    locale: String,
    zdr: bool,
}

fn typed(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

/// The switch first, so a deployment without the feature answers the same for every caller.
pub(crate) fn switch_gate(enabled: bool) -> Result<(), HttpJsonError> {
    if enabled {
        Ok(())
    } else {
        Err(typed(
            StatusCode::NOT_FOUND,
            "review_run_disabled",
            "review runs are not enabled on this deployment",
        ))
    }
}

fn input_refusal(error: InputError) -> HttpJsonError {
    let status = match error {
        InputError::MaterialTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::BAD_REQUEST,
    };
    typed(status, error.code(), error.message())
}

async fn post_review_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<ReviewRunRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate(review_run::enabled())?;
    review_run_with(state, claims, inference_bearer, req).await
}

/// The review once the switch is on: check the input, decide the posture, make the one read-only
/// call, and read its reply.
pub(crate) async fn review_run_with(
    state: AppState,
    claims: Claims,
    inference_bearer: VerifiedInferenceBearer,
    req: ReviewRunRequest,
) -> Result<Json<Value>, HttpJsonError> {
    let input = review_run::validate(
        &req.target,
        &req.label,
        &req.brief,
        &req.material,
        &req.locale,
    )
    .map_err(input_refusal)?;
    let zdr =
        crate::zdr_posture::zdr_or_refuse(&claims, &crate::zdr_posture::Route::plain(req.zdr))?;
    let run_id = new_ulid();
    let request = review_request(&input, &run_id, &claims.org_id, zdr);
    let started = Instant::now();
    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(request, &inference_bearer))
        .await
        .map_err(|e| grpc_status_to_http(&e))?
        .into_inner();
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let lines: Vec<&str> = input.material.expose_for_model().lines().collect();
    let parse = parse_review_reply(&resp.content, &lines);
    let cost_basis = CostBasis::of_route("", "").served_by(&resp.provider_used);
    let cost_usd = if zdr {
        None
    } else {
        state
            .pricing
            .cost_usd_for(
                cost_basis,
                &resp.model_used,
                i64::from(resp.input_tokens.max(0)),
                i64::from(resp.output_tokens.max(0)),
                i64::from(resp.cache_read_input_tokens.max(0)),
                i64::from(resp.cache_creation_input_tokens.max(0)),
            )
            .await
    };
    let usage = RunUsage {
        input_tokens: resp.input_tokens,
        output_tokens: resp.output_tokens,
        cost_usd,
        latency_ms,
    };
    Ok(Json(json!({
        "data": { "run": card_json(&run_id, &input, &parse, &resp.model_used, &usage) }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ZdrWaiverClaim;
    use crate::http_routes::speech_route_tests::{bearer, state_with, SpeechInference};
    use mp_contracts::model_plane::v1 as pb;

    fn claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn waived() -> Claims {
        Claims {
            org_zdr: Some(true),
            zdr_waiver: Some(ZdrWaiverClaim {
                version: 1,
                provider: "azure-openai".to_owned(),
                connection: "conn-1".to_owned(),
            }),
            ..claims(false)
        }
    }

    fn request(extra: Value) -> ReviewRunRequest {
        let mut body = json!({
            "target": "artifact_revision",
            "label": "Tilbud",
            "material": "Prisen er 900 kr.\nLeveringstiden er 14 dager.",
            "brief": "Sjekk tallene",
        });
        for (key, value) in extra.as_object().expect("extra fields are an object") {
            body[key] = value.clone();
        }
        serde_json::from_value(body).expect("a review body parses")
    }

    fn reply(findings: Value) -> pb::InferResponse {
        pb::InferResponse {
            content: json!({ "findings": findings }).to_string(),
            model_used: "test-model".to_owned(),
            provider_used: "azure".to_owned(),
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        }
    }

    fn price_finding() -> Value {
        json!([{
            "severity": "P1",
            "title": "Pris uten mva",
            "body": "Prisen oppgis uten merverdiavgift.",
            "suggestion": "Oppgi prisen med mva.",
            "lineStart": 1,
            "lineEnd": 1,
            "quote": "Prisen er 900 kr.",
        }])
    }

    async fn call(
        inference: &SpeechInference,
        claims: Claims,
        extra: Value,
    ) -> Result<Json<Value>, HttpJsonError> {
        let mut state = state_with(inference.clone()).await;
        state.pricing = crate::pricing::PricingCache::with_rate_for_test("test-model", 1.0, 2.0);
        review_run_with(state, claims, bearer(), request(extra)).await
    }

    #[test]
    fn the_switch_answers_404_with_a_typed_body_and_passes_when_on() {
        assert!(switch_gate(true).is_ok());
        let (status, Json(body)) = switch_gate(false).expect_err("off is refused");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "review_run_disabled");
        assert!(body["error"]["details"].is_object());
    }

    #[tokio::test]
    async fn a_review_is_one_read_only_call_and_returns_the_card() {
        let inference = SpeechInference::default();
        inference.answering(reply(price_finding()));
        let Json(body) = call(&inference, claims(false), json!({}))
            .await
            .expect("a review succeeds");
        let sent = inference.cleanups();
        assert_eq!(sent.len(), 1, "exactly one model call");
        let run = &body["data"]["run"];
        let run_id = run["runId"].as_str().expect("a run id");
        assert!(!run_id.is_empty());
        assert_eq!(sent[0].request_id, format!("{run_id}-review"));
        assert_eq!(sent[0].purpose, pb::InferPurpose::Validation as i32);
        assert!(sent[0].tools.is_empty(), "no tools are offered");
        assert!(sent[0].temperature.abs() < f32::EPSILON);
        assert!(!sent[0].zdr);
        assert!(sent[0].messages[1]
            .content
            .contains("L1: Prisen er 900 kr."));
        assert_eq!(run["status"], "completed");
        assert_eq!(run["verdict"], "findings");
        assert_eq!(run["findings"][0]["severity"], "P1");
        assert_eq!(run["findings"][0]["lineStart"], 1);
        assert_eq!(run["sourceChecks"][2]["check"], "quoted_text");
        assert_eq!(run["sourceChecks"][2]["outcome"], "passed");
        let cost = run["usage"]["costUsd"].as_f64().expect("a priced call");
        assert!((cost - 0.002).abs() < 1e-9, "{cost}");
    }

    #[tokio::test]
    async fn a_zdr_review_carries_the_flag_and_is_not_priced() {
        let inference = SpeechInference::default();
        inference.answering(reply(price_finding()));
        let Json(body) = call(&inference, claims(true), json!({}))
            .await
            .expect("a ZDR review succeeds");
        assert!(
            inference.cleanups()[0].zdr,
            "the ZDR flag reaches inference-core"
        );
        assert!(body["data"]["run"]["usage"]["costUsd"].is_null());
        assert_eq!(body["data"]["run"]["verdict"], "findings");
    }

    #[tokio::test]
    async fn a_refused_input_makes_no_model_call() {
        let cases = [
            (
                json!({ "target": "file" }),
                StatusCode::BAD_REQUEST,
                "review_target_unknown",
            ),
            (
                json!({ "material": "   " }),
                StatusCode::BAD_REQUEST,
                "review_material_missing",
            ),
            (
                json!({ "material": "a".repeat(review_run::MAX_MATERIAL_BYTES + 1) }),
                StatusCode::PAYLOAD_TOO_LARGE,
                "review_material_too_large",
            ),
            (
                json!({ "label": "l".repeat(review_run::MAX_LABEL_CHARS + 1) }),
                StatusCode::BAD_REQUEST,
                "review_label_too_long",
            ),
            (
                json!({ "brief": "b".repeat(review_run::MAX_BRIEF_CHARS + 1) }),
                StatusCode::BAD_REQUEST,
                "review_brief_too_long",
            ),
        ];
        for (extra, status, code) in cases {
            let inference = SpeechInference::default();
            inference.answering(reply(price_finding()));
            let (got, Json(body)) = call(&inference, claims(false), extra.clone())
                .await
                .expect_err("the input is refused");
            assert_eq!(got, status, "{extra}");
            assert_eq!(body["error"]["code"], code, "{extra}");
            assert!(inference.cleanups().is_empty(), "{extra}: no model call");
        }
    }

    #[tokio::test]
    async fn an_unreadable_reply_is_a_card_that_is_not_clean() {
        let inference = SpeechInference::default();
        inference.answering(pb::InferResponse {
            content: "Sorry, I cannot review this.".to_owned(),
            model_used: "test-model".to_owned(),
            ..Default::default()
        });
        let Json(body) = call(&inference, claims(false), json!({}))
            .await
            .expect("the call itself succeeds");
        let run = &body["data"]["run"];
        assert_eq!(run["status"], "unreadable");
        assert_eq!(run["verdict"], "unreadable");
        assert!(run["findings"].as_array().expect("findings").is_empty());
        assert_eq!(run["sourceChecks"][0]["outcome"], "failed");
    }

    #[tokio::test]
    async fn a_waived_token_is_refused_before_the_model() {
        let inference = SpeechInference::default();
        inference.answering(reply(price_finding()));
        let (status, _) = call(&inference, waived(), json!({}))
            .await
            .expect_err("a waiver names a connection, and a review names none");
        assert!(status.is_client_error(), "{status}");
        assert!(inference.cleanups().is_empty(), "no model call");
    }
}
