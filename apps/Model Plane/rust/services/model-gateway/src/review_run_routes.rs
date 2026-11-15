//! Review as a run (F-t3-086): the HTTP surface of [`crate::review_run`].
//!
//! `POST /v1/ai/review` takes one text (`target`, `label`, `material`, `brief`, `locale`, `zdr`)
//! and answers with one card under `data.run`. The route answers 404 `review_run_disabled` while
//! `MODEL_GATEWAY_REVIEW_RUN_ENABLED` is not the literal `true`. An input the gateway cannot review
//! is a typed 400, or 413 for material over its limit, and makes no model call. A zero-retention
//! turn is honoured: the flag reaches inference-core and the cost is null. A waived token is refused
//! as a route mismatch on the platform chain, since a review names no connection there. A request that names a
//! `thread_id` whose turns were written under a connection is reviewed on that connection, with the `model` the request
//! names for it (W4-BYO, DEC-V-R8); such a review is never served on the platform chain. A customer gateway returns no
//! structured reply, and a review needs one, so a review on one is refused typed before its call. A named thread that is
//! a Space thread, or holds a zero-retention or waived run, is refused 409 before the call, as a side chat refuses it
//! (V3-mg-2-rel2). Nothing is stored;
//! the card goes back to the caller only. A failed call on a subscription connection ends on the subscription
//! catalogue, as a side chat's does (V3-mg-2-rel1).

use axum::{extract::State, http::StatusCode, routing::post, Extension, Json, Router};
use mp_contracts::user_content::UserContent;
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

use crate::auth::{Claims, VerifiedInferenceBearer, VerifiedSessionBearer};
use crate::http_routes::{authenticated_inference_request, HttpJsonError};
use crate::review_run::{
    self, card_json, parse_review_reply, review_request, InputError, ReviewInput, RunUsage,
};
use crate::side_chat_routes::side_chat_inference_error;
use crate::state::AppState;
use crate::thread_connection::{
    answer_basis, feature_connection_with, refuse_without_structured_output, side_route, SideRoute,
    ThreadConnection,
};
use mp_contracts::model_plane::v1::InferRequest;

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
    /// The thread the text belongs to (W4-BYO, DEC-V-R8). Read only while `MODEL_GATEWAY_BYO_GATEWAY` is on: on a thread
    /// whose turns were written under a connection the review runs on that connection, or is refused typed.
    thread_id: String,
    /// The model the thread's connection serves. Read only on such a thread.
    model: String,
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
    session_bearer: Option<VerifiedSessionBearer>,
    inference_bearer: VerifiedInferenceBearer,
    Json(req): Json<ReviewRunRequest>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate(review_run::enabled())?;
    review_run_with(
        crate::normalize::byo_gateway_enabled(),
        state,
        claims,
        session_bearer,
        inference_bearer,
        req,
    )
    .await
}

/// A review asks for a structured reply. A customer gateway connection returns none, so a review on one is refused
/// typed before its call (W4-BYO); the platform and a subscription route return one.
fn refuse_unstructured_route(route: &SideRoute) -> Result<(), HttpJsonError> {
    match route {
        SideRoute::Platform => Ok(()),
        SideRoute::Connection { connection, .. } => {
            refuse_without_structured_output(connection, "a review")
        }
    }
}

/// The review's one call: the request the review builds, put on the route it is served on. On a thread's connection the
/// call names that connection and the model the request gives; on the platform chain the call is as it was built.
fn review_call(
    input: &ReviewInput,
    run_id: &str,
    org_id: &str,
    zdr: bool,
    route: &SideRoute,
) -> InferRequest {
    let mut request = review_request(input, run_id, org_id, zdr);
    route.apply_to(&mut request);
    request
}

/// Whether the call is served on a subscription connection, which ends a failed call on the subscription catalogue.
fn on_subscription(route: &SideRoute) -> bool {
    matches!(
        route,
        SideRoute::Connection {
            connection: ThreadConnection::Subscription(_),
            ..
        }
    )
}

/// The answer to a failed `infer` (V3-mg-2-rel1). A call on the thread's subscription connection ends on the subscription
/// catalogue, as a side chat's does: its typed code, or the generic route failure, with the catalogue's words, `retryable`
/// and timing, and never the upstream message. A typed refusal keeps its own body, and a platform or customer-gateway call
/// keeps `grpc_status_to_http`, as before.
fn review_inference_error(error: &tonic::Status, route: &SideRoute) -> HttpJsonError {
    side_chat_inference_error(error, on_subscription(route))
}

/// The review once the switch is on: check the input, decide the posture, resolve the route the thread's connection
/// gives, make the one read-only call, and read its reply. `byo_enabled` is the `MODEL_GATEWAY_BYO_GATEWAY` switch,
/// passed in so a test decides it without the environment variable.
pub(crate) async fn review_run_with(
    byo_enabled: bool,
    state: AppState,
    claims: Claims,
    session_bearer: Option<VerifiedSessionBearer>,
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
    let connection = feature_connection_with(
        byo_enabled,
        &state,
        &claims,
        session_bearer.as_ref(),
        &req.thread_id,
        req.zdr,
        "a review",
    )
    .await?;
    let route = side_route(
        connection,
        &req.model,
        crate::normalize::load_allowed_models().as_deref(),
    )?;
    refuse_unstructured_route(&route)?;
    let run_id = new_ulid();
    let request = review_call(&input, &run_id, &claims.org_id, zdr, &route);
    let started = Instant::now();
    let resp = state
        .inference_client
        .clone()
        .infer(authenticated_inference_request(request, &inference_bearer))
        .await
        .map_err(|e| review_inference_error(&e, &route))?
        .into_inner();
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let lines: Vec<&str> = input.material.expose_for_model().lines().collect();
    let parse = parse_review_reply(&resp.content, &lines);
    let cost_basis = answer_basis(&route, &resp.provider_used);
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
    use crate::thread_connection::{
        thread_connection, ThreadConnection, CUSTOMER_GATEWAY_PROVIDER,
    };
    use mp_contracts::agentic_subscription::PROVIDER_ID;
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
        review_run_with(false, state, claims, None, bearer(), request(extra)).await
    }

    /// The review input the tests send, checked the way the route checks it.
    fn review_input() -> ReviewInput {
        review_run::validate(
            "artifact_revision",
            "Tilbud",
            &UserContent::from(String::new()),
            &UserContent::from("Prisen er 900 kr.".to_owned()),
            "nb",
        )
        .expect("a review input is valid")
    }

    /// A thread whose user turn was written under the customer-gateway connection `connection`.
    fn gateway_thread(connection: &str) -> Vec<pb::SessionMessage> {
        vec![pb::SessionMessage {
            role: "user".to_owned(),
            content: "Prisen er 900 kr.".to_owned(),
            model_connection_id: connection.to_owned(),
            ..Default::default()
        }]
    }

    /// A thread whose user turn was written under the subscription connection `connection`.
    fn subscription_thread(connection: &str) -> Vec<pb::SessionMessage> {
        vec![pb::SessionMessage {
            role: "user".to_owned(),
            content: "Prisen er 900 kr.".to_owned(),
            subscription_connection_id: connection.to_owned(),
            ..Default::default()
        }]
    }

    fn review_on(route: &SideRoute) -> pb::InferRequest {
        review_call(&review_input(), "run-1", "org-a", false, route)
    }

    #[test]
    fn a_platform_review_is_the_request_the_review_builds() {
        let built = review_request(&review_input(), "run-1", "org-a", false);
        assert_eq!(review_on(&SideRoute::Platform), built);
    }

    #[test]
    fn a_byo_thread_review_is_one_call_on_its_connection_with_the_model_named() {
        let connection = thread_connection(true, &gateway_thread("conn-a")).expect("not refused");
        let route = side_route(connection, " gpt-4o-mini ", None).expect("a named model is served");
        let request = review_on(&route);
        assert_eq!(
            request.request_id, "run-1-review",
            "still one call of the run"
        );
        assert_eq!(request.model, "gpt-4o-mini");
        assert_eq!(request.model_connection_id, "conn-a");
        assert_eq!(request.provider_hint, CUSTOMER_GATEWAY_PROVIDER);
        assert!(request.subscription_connection_id.is_empty());
        assert!(
            request.role_fallback_model.is_empty(),
            "a connection call has no platform fallback"
        );
        assert!(request.tools.is_empty(), "no tools are offered");
        assert!(!request.zdr);
        assert_eq!(request.purpose, pb::InferPurpose::Validation as i32);
    }

    #[test]
    fn a_byo_thread_review_is_refused_typed_before_the_call_as_it_needs_a_structured_reply() {
        let connection = thread_connection(true, &gateway_thread("conn-a")).expect("not refused");
        let route = side_route(connection, "gpt-4o-mini", None).expect("a named model is served");
        let error = refuse_unstructured_route(&route)
            .expect_err("a customer gateway returns no structured reply");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        let Json(body) = &error.1;
        assert_eq!(body["error"]["code"], "feature_not_supported");
    }

    #[test]
    fn a_subscription_or_platform_review_keeps_its_structured_reply() {
        let connection =
            thread_connection(true, &subscription_thread("conn_sub")).expect("not refused");
        let route = side_route(connection, "gpt-4o-mini", None).expect("a named model is served");
        assert!(refuse_unstructured_route(&route).is_ok());
        assert!(refuse_unstructured_route(&SideRoute::Platform).is_ok());
    }

    #[test]
    fn a_subscription_thread_review_is_one_call_on_its_subscription_connection() {
        let connection =
            thread_connection(true, &subscription_thread("conn_sub")).expect("not refused");
        let route = side_route(connection, "gpt-4o-mini", None).expect("a named model is served");
        let request = review_on(&route);
        assert_eq!(request.model, "gpt-4o-mini");
        assert_eq!(request.subscription_connection_id, "conn_sub");
        assert_eq!(request.provider_hint, PROVIDER_ID);
        assert!(request.model_connection_id.is_empty());
    }

    #[test]
    fn a_connection_review_without_a_model_is_refused_before_the_call() {
        let connection = thread_connection(true, &gateway_thread("conn-a")).expect("not refused");
        let error = side_route(connection, "", None).expect_err("no model, no call");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        let Json(body) = &error.1;
        assert_eq!(body["error"]["code"], "side_chat_model_required");
    }

    #[test]
    fn a_review_on_a_connection_is_billed_on_the_connection() {
        let route = SideRoute::Connection {
            connection: ThreadConnection::CustomerGateway("conn-a".to_owned()),
            model: "gpt-4o-mini".to_owned(),
        };
        assert_eq!(
            answer_basis(&route, "customer-gateway.conn-a"),
            crate::pricing::CostBasis::CustomerBilled
        );
    }

    #[test]
    fn a_zero_retention_review_on_a_connection_thread_is_refused_before_the_call() {
        let zdr = Claims {
            org_zdr: Some(true),
            ..claims(true)
        };
        let refused = crate::thread_connection::connection_posture(
            &zdr,
            &ThreadConnection::Subscription("conn_sub".to_owned()),
            false,
        )
        .expect_err("a ZDR org cannot review on a subscription");
        let Json(body) = &refused.1;
        assert_eq!(
            body["error"]["code"],
            mp_contracts::zdr_refusal::code::CONNECTION_NOT_ACKNOWLEDGED
        );
    }

    #[test]
    fn with_the_switch_off_a_byo_thread_is_reviewed_on_the_platform_chain() {
        let connection = thread_connection(false, &gateway_thread("conn-a")).expect("not refused");
        let route = side_route(connection, "gpt-4o-mini", None).expect("a platform route");
        assert_eq!(route, SideRoute::Platform);
        assert_eq!(
            review_on(&route),
            review_request(&review_input(), "run-1", "org-a", false)
        );
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

    #[tokio::test]
    async fn with_the_switch_off_a_review_naming_a_thread_is_the_platform_call() {
        let inference = SpeechInference::default();
        inference.answering(reply(price_finding()));
        let mut state = state_with(inference.clone()).await;
        state.pricing = crate::pricing::PricingCache::with_rate_for_test("test-model", 1.0, 2.0);
        let Json(body) = review_run_with(
            false,
            state,
            claims(false),
            None,
            bearer(),
            request(json!({ "thread_id": "thread-1", "model": "x" })),
        )
        .await
        .expect("a switch-off review is served without a session credential");
        let run_id = body["data"]["run"]["runId"].as_str().expect("a run id");
        let sent = inference.cleanups();
        assert_eq!(sent.len(), 1, "exactly one model call");
        let built = request(json!({}));
        let input = review_run::validate(
            &built.target,
            &built.label,
            &built.brief,
            &built.material,
            &built.locale,
        )
        .expect("the review input is valid");
        assert_eq!(sent[0], review_request(&input, run_id, "org-a", false));
    }

    #[tokio::test]
    async fn with_the_switch_on_a_review_on_a_thread_without_a_session_credential_is_refused() {
        let inference = SpeechInference::default();
        inference.answering(reply(price_finding()));
        let (status, Json(body)) = review_run_with(
            true,
            state_with(inference.clone()).await,
            claims(false),
            None,
            bearer(),
            request(json!({ "thread_id": "thread-1", "model": "x" })),
        )
        .await
        .expect_err("the thread's connection cannot be read without the session credential");
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "delegated_credential_required");
        assert!(inference.cleanups().is_empty(), "no model call");
    }

    /// A failed call as inference-core declares it on a subscription connection: its reason on the trailer, and an
    /// upstream message that must never reach the caller.
    fn subscription_status(kind: &str) -> tonic::Status {
        let mut status = tonic::Status::resource_exhausted("upstream said: quota for acct 42");
        status.metadata_mut().insert(
            crate::compaction::PROVIDER_ERROR_KIND_TRAILER,
            kind.parse().expect("ascii"),
        );
        status
    }

    fn subscription_route() -> SideRoute {
        let connection =
            thread_connection(true, &subscription_thread("conn_sub")).expect("not refused");
        side_route(connection, "gpt-4o-mini", None).expect("a named model is served")
    }

    fn gateway_route() -> SideRoute {
        let connection = thread_connection(true, &gateway_thread("conn-a")).expect("not refused");
        side_route(connection, "gpt-4o-mini", None).expect("a named model is served")
    }

    #[test]
    fn a_failed_review_on_a_subscription_thread_ends_on_the_subscription_catalogue() {
        let status = subscription_status(crate::subscription_failure::SUBSCRIPTION_USAGE_LIMIT);
        let (http_status, Json(body)) = review_inference_error(&status, &subscription_route());
        assert_eq!(http_status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(
            body["code"].as_str(),
            Some(crate::subscription_failure::SUBSCRIPTION_USAGE_LIMIT),
            "{body}"
        );
        assert_eq!(body["retryable"].as_bool(), Some(false), "{body}");
        assert!(!body.to_string().contains("acct 42"), "{body}");
    }

    #[test]
    fn a_failure_with_no_typed_reason_on_a_subscription_thread_is_the_generic_route_failure() {
        let status = tonic::Status::unavailable("upstream said: token expired for acct 42");
        let (http_status, Json(body)) = review_inference_error(&status, &subscription_route());
        assert_eq!(http_status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(
            body["code"].as_str(),
            Some(crate::subscription_failure::SUBSCRIPTION_ROUTE_UNAVAILABLE),
            "{body}"
        );
        assert_eq!(body["retryable"].as_bool(), Some(true), "{body}");
        assert!(!body.to_string().contains("acct 42"), "{body}");
    }

    #[test]
    fn a_failed_review_on_the_platform_or_a_customer_gateway_keeps_grpc_status_to_http() {
        let status = subscription_status(crate::subscription_failure::SUBSCRIPTION_REAUTH_REQUIRED);
        let (flat_status, Json(flat)) = crate::http_routes::grpc_status_to_http(&status);
        for route in [SideRoute::Platform, gateway_route()] {
            let (http_status, Json(body)) = review_inference_error(&status, &route);
            assert_eq!((http_status, &body), (flat_status, &flat), "{body}");
        }
    }

    #[test]
    fn a_typed_refusal_on_a_subscription_thread_keeps_its_own_body() {
        let zdr = mp_contracts::zdr_refusal::Refusal::CONNECTION_NOT_ACKNOWLEDGED
            .grpc_status(tonic::Code::FailedPrecondition);
        let (flat_status, Json(flat)) = crate::http_routes::grpc_status_to_http(&zdr);
        let (http_status, Json(body)) = review_inference_error(&zdr, &subscription_route());
        assert_eq!((http_status, &body), (flat_status, &flat), "{body}");
        assert_eq!(
            body["error"]["code"],
            mp_contracts::zdr_refusal::code::CONNECTION_NOT_ACKNOWLEDGED,
            "{body}"
        );
    }
}
