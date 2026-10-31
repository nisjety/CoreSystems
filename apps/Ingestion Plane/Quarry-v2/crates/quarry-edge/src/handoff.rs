//! Forward durable-path requests (crawl, batch, scheduled) to control plane.
//!
//! Edge posts a job record to `quarry-control-go` so the operator can list/query
//! it. Orchestrator eventually picks up matching Temporal workflow via the
//! control → orchestrator channel (schedules reconcile loop + future dispatch).

use std::time::Duration;

use serde::Deserialize;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::resources::RunOrigin;

use crate::routes::HandoffAck;

/// Posts `POST {control}/v1/jobs?org_id=<org>` with `{kind, policy?}` plus an
/// opaque `params` blob carrying kind-specific payload (url, urls,
/// max_pages…).
///
/// `org_id` rides as a query param — mirroring `resource_routes.rs`'s
/// `forward_mutation` convention for `/v1/sources` — so control's
/// `createJob` handler can read the edge-verified org the same way
/// `createSourceHandler` does, and reject/stamp it without trusting
/// anything from the JSON body. `params.org_id` (embedded in `payload` by
/// the caller) is untouched: the orchestrator dispatcher still reads org
/// from there to build the Temporal workflow input.
pub async fn forward_to_orchestrator(
    control_base_url: &str,
    _request_id: &str,
    org_id: &str,
    payload: serde_json::Value,
    signer: Option<&crate::internal_auth::InternalSigner>,
) -> Result<HandoffAck, QuarryError> {
    if control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control base url empty",
        ));
    }
    if org_id.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "handoff missing org_id",
        ));
    }

    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if kind.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "handoff payload missing 'kind'",
        ));
    }

    let body = serde_json::json!({ "kind": kind, "params": payload });
    post_job(control_base_url, org_id, body, None, signer).await
}

/// Posts a durable job with its origin (WS15.L1): the body is `{kind, params,
/// origin}`, and the caller's `Idempotency-Key` rides along when one was sent.
/// `forward_to_orchestrator` stays for callers that carry no origin.
pub async fn forward_job_signed(
    control_base_url: &str,
    org_id: &str,
    kind: &str,
    params: serde_json::Value,
    origin: &RunOrigin,
    idempotency_key: Option<&str>,
    signer: Option<&crate::internal_auth::InternalSigner>,
) -> Result<HandoffAck, QuarryError> {
    let body = serde_json::json!({ "kind": kind, "params": params, "origin": origin });
    post_job(control_base_url, org_id, body, idempotency_key, signer).await
}

/// WS6.H: the plan-dependent admission caps stamped into a durable job's params
/// (`params.admission = {cap, backlog_cap}`), which control reads when job
/// admission is on (`QUARRY_JOB_ADMISSION`; off, control ignores the stamp).
///
/// Only an entitlement answer carries plan values: the floor and the fail-open
/// answer stamp nothing, so control keeps its own defaults for those. A plan
/// value of `0` is not stamped either: control reads a cap below 1 as "none
/// given" and would apply its default, so a stamped `0` would not mean what it
/// says. Edge-built params only: a caller's body never reaches this stamp.
pub fn admission_stamp(limits: &crate::limits::OrgLimits) -> Option<serde_json::Value> {
    if limits.origin != crate::limits::LimitsOrigin::Entitlement {
        return None;
    }
    let positive = |value: Option<u32>| value.filter(|v| *v >= 1);
    let cap = positive(limits.caps.crawl_jobs);
    let backlog_cap = positive(limits.caps.job_backlog);
    if cap.is_none() && backlog_cap.is_none() {
        return None;
    }
    let mut stamp = serde_json::Map::new();
    if let Some(cap) = cap {
        stamp.insert("cap".to_string(), serde_json::json!(cap));
    }
    if let Some(backlog_cap) = backlog_cap {
        stamp.insert("backlog_cap".to_string(), serde_json::json!(backlog_cap));
    }
    Some(serde_json::Value::Object(stamp))
}

/// `params` with the organization's admission stamp set (see
/// [`admission_stamp`]). Any `admission` key already in `params` is replaced or,
/// when the plan has no value to stamp, removed, so the stamp is always the edge's.
pub fn with_admission(
    mut params: serde_json::Value,
    limits: &crate::limits::OrgLimits,
) -> serde_json::Value {
    if let Some(map) = params.as_object_mut() {
        match admission_stamp(limits) {
            Some(stamp) => {
                map.insert("admission".to_string(), stamp);
            }
            None => {
                map.remove("admission");
            }
        }
    }
    params
}

/// Service principals that run Model Plane work for a user. A job one of them
/// submits has an agent origin; any other service principal is an API client.
const MODEL_PLANE_PRINCIPALS: &[&str] = &["service:model-gateway", "service:execution-core"];

/// The origin of a job, from the verified claims and never from the body: a user
/// token is `user`; a Model Plane service is `agent`, carrying the run id it sent;
/// any other service is `api`.
pub fn run_origin_from_request(
    claims: &crate::auth::Claims,
    headers: &axum::http::HeaderMap,
) -> RunOrigin {
    let run_ref = if claims.is_service() {
        headers
            .get("x-verevon-run-id")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty() && v.len() <= 128)
            .map(str::to_string)
    } else {
        None
    };
    if claims.is_service() {
        let actor = claims.actor_id().to_string();
        if MODEL_PLANE_PRINCIPALS.contains(&actor.as_str()) {
            return RunOrigin {
                origin_type: "agent".to_string(),
                actor: Some(actor),
                origin_ref: run_ref,
            };
        }
        return RunOrigin {
            origin_type: "api".to_string(),
            actor: Some(actor),
            origin_ref: None,
        };
    }
    let actor = claims.user_id.trim();
    RunOrigin {
        origin_type: "user".to_string(),
        actor: (!actor.is_empty()).then(|| actor.to_string()),
        origin_ref: None,
    }
}

/// The caller's `Idempotency-Key` header, when it is a usable key (1 to 128
/// characters). Control answers a repeated key with the job it already created.
pub fn idempotency_key_from_request(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .map(str::to_string)
}

/// Posts a job body to control's `POST /v1/jobs?org_id=` and reads the created
/// job back. The body is signed over the bytes and the query that are sent when
/// a signer is configured. `idempotency_key`, when present, is sent as the
/// `Idempotency-Key` header.
pub(crate) async fn post_job(
    control_base_url: &str,
    org_id: &str,
    body: serde_json::Value,
    idempotency_key: Option<&str>,
    signer: Option<&crate::internal_auth::InternalSigner>,
) -> Result<HandoffAck, QuarryError> {
    if control_base_url.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control base url empty",
        ));
    }
    if org_id.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "handoff missing org_id",
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {}", e.without_url())))?;

    // Control verifies an HMAC over (method, path?query, body hash, ts, nonce)
    // once QUARRY_INTERNAL_HMAC_REQUIRED=1 (the production overlay sets it), so
    // the bytes signed are the bytes sent and the query is the one on the wire.
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode handoff: {e}")))?;
    let mut url = reqwest::Url::parse(&format!(
        "{}/v1/jobs",
        control_base_url.trim_end_matches('/')
    ))
    .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("control url: {e}")))?;
    url.query_pairs_mut().append_pair("org_id", org_id);
    let mut request = client
        .post(url.clone())
        .header("content-type", "application/json")
        .body(body_bytes.clone());
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    if let Some(signer) = signer {
        let path_q = match url.query() {
            Some(query) => format!("{}?{}", url.path(), query),
            None => url.path().to_string(),
        };
        let signed = crate::internal_auth::apply_to_request(signer, "POST", &path_q, &body_bytes);
        request = request
            .header(crate::internal_auth::HEADER_SIG, signed.signature)
            .header(crate::internal_auth::HEADER_TS, signed.timestamp)
            .header(crate::internal_auth::HEADER_NONCE, signed.nonce);
    }
    let resp = request
        .send()
        .await
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::UpstreamBlocked,
                format!("control unreachable: {}", e.without_url()),
            )
        })?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // The organization's own queue is full (job admission): that is the
        // caller's to retry later, not an upstream fault, so it stays a typed
        // RATE_LIMITED with the numbers control gave instead of a 502.
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if let Some(error) = backlog_full(&text) {
                return Err(error);
            }
        }
        return Err(QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("control {}: {}", status.as_u16(), truncate(&text, 500)),
        ));
    }

    // Control answers 200 (not 201) when the `Idempotency-Key` names a job it already holds:
    // this call created nothing. Only a call that sent a key can be a replay.
    let replayed = idempotency_key.is_some() && status == reqwest::StatusCode::OK;

    let parsed: ControlJobEnvelope = resp.json().await.map_err(|e| {
        QuarryError::new(ErrorCode::Internal, format!("decode control response: {}", e.without_url()))
    })?;

    let Some(job) = parsed.data else {
        return Err(QuarryError::new(
            ErrorCode::Internal,
            "control returned no job",
        ));
    };

    Ok(HandoffAck {
        job_id: job.id,
        run_id: job.run_id,
        accepted_at: chrono::Utc::now(),
        limits: None,
        effective: None,
        rejected: Vec::new(),
        replayed,
    })
}

/// The id a crawl or batch usage event is keyed by (decision 3.11): Control's job id, and
/// only for a job this handoff created. A refused or failed handoff created no job, so it
/// has no id and is not billed; a replay (Control answered 200 for a key it already holds)
/// created nothing either, so a client retry never bills a second time. The id is Control's
/// and unique across organizations; the caller-suppliable request id plays no part.
pub(crate) fn billable_job_id(ack: &Result<HandoffAck, QuarryError>) -> Option<&str> {
    match ack {
        Ok(ack) if !ack.replayed => Some(ack.job_id.as_str()),
        _ => None,
    }
}

/// The `org_backlog_full` refusal from control's error envelope
/// The `org_backlog_full` refusal from control's error envelope
/// (`{"error": {"code": "RATE_LIMITED", "details": {"reason": ..}}}`), as a
/// typed `RATE_LIMITED` carrying `reason`, `limit`, `queued` and
/// `retry_after_seconds`. `None` for any other 429 body.
fn backlog_full(body: &str) -> Option<QuarryError> {
    let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
    let details = envelope.get("error")?.get("details")?;
    if details.get("reason")?.as_str()? != "org_backlog_full" {
        return None;
    }
    let pick = |key: &str| details.get(key).cloned().unwrap_or(serde_json::Value::Null);
    Some(
        QuarryError::new(
            ErrorCode::RateLimited,
            "this organization has too many jobs waiting; try again shortly",
        )
        .with_details(serde_json::json!({
            "reason": "org_backlog_full",
            "scope": "org",
            "limit": pick("limit"),
            "queued": pick("queued"),
            "retry_after_seconds": pick("retry_after_seconds"),
        })),
    )
}

#[derive(Debug, Deserialize)]
struct ControlJobEnvelope {
    data: Option<ControlJob>,
}

#[derive(Debug, Deserialize)]
struct ControlJob {
    id: String,
    /// Populated by the orchestrator once the job is running; `None` at
    /// initial handoff. Threaded into `HandoffAck.run_id` for consumers
    /// that prefer the run-keyed event stream.
    #[serde(default)]
    run_id: Option<String>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_429(reason: &str) -> String {
        serde_json::json!({
            "data": null,
            "meta": { "request_id": "req_1" },
            "error": {
                "code": "RATE_LIMITED",
                "message": "this organization has too many jobs waiting",
                "details": { "reason": reason, "limit": 200, "queued": 200, "retry_after_seconds": 30 },
                "retryable": false
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn the_handoff_is_signed_over_the_bytes_and_query_it_sends() {
        use crate::internal_auth::{InternalSigner, HEADER_NONCE, HEADER_SIG, HEADER_TS};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/jobs"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": { "id": "job_1" } })),
            )
            .mount(&server)
            .await;
        let signer = InternalSigner::new("0123456789abcdef0123456789abcdef".to_string()).unwrap();
        let ack = forward_to_orchestrator(
            &server.uri(),
            "req_1",
            "org_1",
            serde_json::json!({ "kind": "crawl", "url": "https://example.com/" }),
            Some(&signer),
        )
        .await
        .expect("accepted");
        assert_eq!(ack.job_id, "job_1");

        let received = server.received_requests().await.unwrap();
        let request = &received[0];
        let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap().to_string();
        let timestamp: i64 = header(HEADER_TS).parse().unwrap();
        let nonce = header(HEADER_NONCE);
        let expected = InternalSigner::header_value(&signer.sign_raw(
            "POST",
            "/v1/jobs?org_id=org_1",
            &InternalSigner::body_digest(&request.body),
            timestamp,
            &nonce,
        ));
        assert_eq!(header(HEADER_SIG), expected, "signature must cover the sent body and query");
    }

    #[tokio::test]
    async fn without_a_signer_the_handoff_carries_no_signature_headers() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": { "id": "job_2" } })),
            )
            .mount(&server)
            .await;
        forward_to_orchestrator(
            &server.uri(),
            "req_1",
            "org_1",
            serde_json::json!({ "kind": "batch" }),
            None,
        )
        .await
        .expect("accepted");
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.get("x-quarry-sig").is_none());
    }

    #[tokio::test]
    async fn a_200_for_a_sent_key_is_a_replay_and_has_no_billable_job() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Control: 201 for a created job, 200 for one it already held (decision 3.11).
        for (status, key, replayed) in [
            (201_u16, Some("k1"), false),
            (200, Some("k1"), true),
            // No key was sent, so nothing can be a replay, whatever the status.
            (200, None, false),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(serde_json::json!({ "data": { "id": "job_7" } })),
                )
                .mount(&server)
                .await;
            let ack = forward_job_signed(
                &server.uri(),
                "org_1",
                "crawl",
                serde_json::json!({ "kind": "crawl" }),
                &RunOrigin { origin_type: "user".to_string(), actor: None, origin_ref: None },
                key,
                None,
            )
            .await;
            assert_eq!(ack.as_ref().unwrap().replayed, replayed, "status {status} key {key:?}");
            assert_eq!(
                billable_job_id(&ack),
                if replayed { None } else { Some("job_7") },
                "status {status} key {key:?}"
            );
        }

        // A refused handoff has no job to bill.
        let refused: Result<HandoffAck, QuarryError> =
            Err(QuarryError::new(ErrorCode::UpstreamBlocked, "control 500"));
        assert_eq!(billable_job_id(&refused), None);
    }

    fn entitlement(caps: crate::limits::PlanCaps) -> crate::limits::OrgLimits {
        crate::limits::OrgLimits {
            origin: crate::limits::LimitsOrigin::Entitlement,
            caps,
            ..crate::limits::OrgLimits::floor()
        }
    }

    #[test]
    fn two_plans_stamp_two_different_admission_caps() {
        use crate::limits::PlanCaps;
        let small = entitlement(PlanCaps { crawl_jobs: Some(2), job_backlog: Some(20), ..PlanCaps::default() });
        let large = entitlement(PlanCaps { crawl_jobs: Some(10), job_backlog: Some(500), ..PlanCaps::default() });
        let params = serde_json::json!({ "kind": "crawl", "url": "https://example.com/" });
        let a = with_admission(params.clone(), &small);
        let b = with_admission(params, &large);
        assert_eq!(a["admission"], serde_json::json!({ "cap": 2, "backlog_cap": 20 }));
        assert_eq!(b["admission"], serde_json::json!({ "cap": 10, "backlog_cap": 500 }));
        assert_eq!(a["kind"], "crawl", "the rest of the params is untouched");
    }

    #[test]
    fn the_floor_the_open_answer_and_a_plan_with_no_value_stamp_nothing() {
        use crate::limits::{OrgLimits, PlanCaps};
        let params = serde_json::json!({ "kind": "batch" });
        assert!(with_admission(params.clone(), &OrgLimits::floor()).get("admission").is_none());
        assert!(with_admission(params.clone(), &OrgLimits::open()).get("admission").is_none());
        assert!(with_admission(params.clone(), &entitlement(PlanCaps::default())).get("admission").is_none());
        // A plan value of 0 is not a usable cap for control, so it is not stamped.
        let zero = entitlement(PlanCaps { crawl_jobs: Some(0), job_backlog: Some(0), ..PlanCaps::default() });
        assert!(with_admission(params, &zero).get("admission").is_none());
    }

    #[test]
    fn one_plan_value_stamps_only_that_key_and_a_stale_admission_key_is_replaced() {
        use crate::limits::PlanCaps;
        let only_cap = entitlement(PlanCaps { crawl_jobs: Some(4), ..PlanCaps::default() });
        let stamped = with_admission(serde_json::json!({ "admission": { "cap": 999, "backlog_cap": 999 } }), &only_cap);
        assert_eq!(stamped["admission"], serde_json::json!({ "cap": 4 }));
        let none = with_admission(serde_json::json!({ "admission": { "cap": 999 } }), &crate::limits::OrgLimits::floor());
        assert!(none.get("admission").is_none(), "a caller-shaped stamp never survives");
    }

    #[test]
    fn a_full_backlog_is_a_typed_rate_limit_with_the_numbers() {
        let error = backlog_full(&control_429("org_backlog_full")).expect("typed");
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert!(error.retryable);
        let details = error.details.expect("details");
        assert_eq!(details["reason"], "org_backlog_full");
        assert_eq!(details["scope"], "org");
        assert_eq!(details["limit"], 200);
        assert_eq!(details["queued"], 200);
        assert_eq!(details["retry_after_seconds"], 30);
    }

    #[test]
    fn any_other_429_or_a_body_that_is_not_the_envelope_stays_an_upstream_error() {
        assert!(backlog_full(&control_429("something_else")).is_none());
        assert!(backlog_full("rate limit exceeded").is_none());
        assert!(backlog_full("{}").is_none());
        assert!(backlog_full(r#"{"error":{"details":{}}}"#).is_none());
        assert!(backlog_full(r#"{"error":{"details":{"reason":7}}}"#).is_none());
    }

    #[test]
    fn the_refusal_never_echoes_the_control_message_or_extra_fields() {
        let body = serde_json::json!({
            "error": { "message": "SECRET-INTERNAL-TEXT", "details": {
                "reason": "org_backlog_full", "limit": 1, "queued": 1, "retry_after_seconds": 1,
                "internal_dsn": "postgres://leak"
            } }
        })
        .to_string();
        let rendered = serde_json::to_string(&backlog_full(&body).unwrap()).unwrap();
        assert!(!rendered.contains("SECRET-INTERNAL-TEXT"));
        assert!(!rendered.contains("postgres://leak"));
    }
}
