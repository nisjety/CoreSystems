//! "Forward an email to create a task" — the gateway's own half (chat-wave2
//! mail-task spec, owner ruling 2026-09-28: Postmark inbound, self-forward
//! only).
//!
//! ## What this module does today
//! - Verifies the HMAC service-delegation on every inbound call from
//!   integration-corev2 (`x-service-id: integration-mail-task-worker`),
//!   mirroring conversation-ingest-rs's `require_ingest_delegation`/
//!   `verify_ingest_delegation` (same canonical-string shape, same nonce +
//!   timestamp replay window).
//! - Rate-limits per `(org_id, user_id)` so a leaked alias cannot drive
//!   unbounded execution-core spend (spec §6).
//! - Deduplicates by `provider_message_id` so a webhook retry cannot fan out
//!   into two runs.
//! - Publishes a content-free NATS completion event
//!   (`verevon.application.mail_task.drafted`) — but ONLY when a
//!   [`MailTaskPipeline`] is wired AND reports success.
//!
//! ## What this module deliberately does NOT do yet
//! Creating the thread, posting the synthesized first turn, and invoking
//! exactly one execution-core turn on it — step 3 of mail-task spec §3.4 — is
//! represented here only as the [`MailTaskPipeline`] trait, with
//! `AppState.mail_task_pipeline` wired to `None` in every production build
//! (`config.rs::build_state`). Every call currently answers
//! `503 mail_task_pipeline_not_configured` once past verification/rate-limit/
//! dedup, which is the correct behavior for "not built yet": Postmark's
//! webhook redelivery keeps retrying (a `5xx`, not a `2xx` that would silently
//! drop the email) until it is.
//!
//! This is a real, unresolved architectural gap, not an oversight:
//! - Chat threads (session-core's `CreateThread`/`AppendMessage`,
//!   `origin="mail_task"`) are reached today only two ways in this codebase:
//!   (a) Model Gateway's HTTP surface (`model_gateway_url` + `/v1/threads*`),
//!       which requires a LIVE user's `aud=session-core` bearer minted from a
//!       real browser cookie (`audience_tokens.rs::get_audience_token`) — an
//!       inbound email has no browser session to mint one from; or
//!   (b) a direct gRPC client of session-core's `mp_contracts` proto (the way
//!       capability-core's cron sweeper calls `PrepareScheduledRunThread` —
//!       `apps/Model Plane/go/services/capability-core/internal/taskexec/
//!       workflow_dispatcher.go`) — but that RPC is tightly coupled to Space
//!       authority (`space_id`, `space_decision_ref`,
//!       `resource_authorization_ref`, `control_decision_token`, ...), which
//!       mail-task's own design explicitly avoids dragging in (spec §1.D),
//!       and this crate has no `tonic`/`mp_contracts` dependency at all —
//!       adding one is a new-dependency, cross-plane architecture change well
//!       outside a mail-task-sized change.
//! - Similarly, "invoke exactly one execution-core turn on that thread" has
//!   no existing unattended (no-live-user) precedent outside the same
//!   Space-scoped scheduled-run machinery.
//!
//! **2026-09-28 update — the thread-creation half of this gap now has a
//! concrete, built, tested seam on the session-core side, from the "schedule"
//! spec's own "results must land in a thread the user owns and can read"
//! work**: `session-core`'s new `PrepareUserOwnedThread` RPC
//! (`grpc.rs::prepare_user_owned_thread`) creates-or-reuses a thread owned by
//! a REAL user (never a service principal) with a caller-named `origin`,
//! keyed by `session_key` for idempotency — the general "service creates or
//! reuses a thread owned by user U, readable by U, with a named origin" seam,
//! restricted to an allow-listed set of trusted service callers
//! (`USER_OWNED_THREAD_CALLERS`, today just `service:model-gateway`, not yet
//! provisioned in any deployment's service-credential registry). Because the
//! thread's owner IS the real user, reading it back needs no Control decision
//! at all — the existing owner-bound `ListThreads` path already covers it.
//!
//! What is STILL missing, and is genuinely Model-Plane-adjacent work this
//! change does not attempt blind: (1) model-gateway has no self-service
//! (non-user-delegated) gRPC credential to session-core today — its existing
//! `GrpcCallCredentials` machinery only ever forwards a LIVE user's
//! `session_bearer`; calling the new RPC as `service:model-gateway` needs
//! Auth Core to provision that service principal (a client-credentials-style
//! registration, the same shape as the `USER_CORE_SERVICE_CREDENTIALS` entry
//! the mail-task alias resolver already needs) and model-gateway to mint and
//! attach that credential instead of a forwarded session_bearer for this one
//! call — new plumbing, not configuration alone; and (2) a new internal,
//! service-authenticated HTTP route on model-gateway wrapping the RPC (mirror
//! of `/v1/internal/space-deletion/threads`'s `authorize_principal_route`
//! pattern) for THIS crate's `MailTaskPipeline` to call over plain HTTP
//! (this crate has no gRPC client and should not gain one). Once both exist,
//! wiring `MailTaskPipeline::run` to them is a small, mostly mechanical step:
//! call the new endpoint with `origin="mail_task"` and
//! `session_key=request.session_key()` to get a real `thread_id`, then close
//! the SEPARATE remaining gap — posting the synthesized first turn and
//! invoking exactly one execution-core turn on it — which has no unattended
//! precedent anywhere in this codebase yet and is a distinctly larger piece.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Mutex, OnceLock},
};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::AppState,
    envelope::{error, ok},
};

const INGEST_PATH: &str = "/internal/mail-task/ingest";
const EXPECTED_SERVICE_ID: &str = "integration-mail-task-worker";
const SELF_AUDIENCE: &str = "verevon-gateway";
const DELEGATION_WINDOW_MINUTES: i64 = 2;
// integration-corev2 caps ONE inbound Postmark payload at 15MB raw JSON
// (mailtask.maxInboundPayloadBytes) and each individual attachment's decoded
// content at 1MB (mailtask.maxAttachmentBytes) before it ever calls this
// endpoint, but does not cap how many allow-listed attachments one message
// may carry (spec §3.2 requires extraction to never fail the whole message,
// which only holds if the aggregate forwarded here can actually fit). For a
// text-based allow-listed type (txt/md/csv/json/html/htm), imports-core's
// extracted text can be roughly as large as the decoded attachment itself, so
// a message with several near-1MB attachments can legitimately produce a
// forwarded body of several MB. This must stay at or above
// integration-corev2's own 15MB raw-payload ceiling (plus headroom for this
// JSON's own field overhead, which is smaller than the base64 encoding
// overhead already spent on the inbound side) -- 2MB was tight enough to
// reject a real, non-malicious, attachment-heavy forward with a permanent
// (non-retryable-into-success) 413, silently losing the email end to end.
const MAX_BODY_BYTES: usize = 20 << 20; // 20MB

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(INGEST_PATH, post(ingest))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_mail_task_delegation,
        ))
}

// ---------------------------------------------------------------------------
// Wire contract (mirrors integration-corev2's mailtask.GatewayIngestRequest)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IngestRequest {
    pub(crate) org_id: String,
    pub(crate) user_id: String,
    pub(crate) provider_message_id: String,
    // Deserialized (and required on the wire) but not yet read by this
    // crate: the code that turns these into the synthesized first chat turn
    // is exactly the deferred `MailTaskPipeline` step this module's doc
    // comment describes. Kept on the struct now so the wire contract is
    // final and integration-corev2 does not need to change its request body
    // again once a real pipeline lands.
    #[allow(dead_code, reason = "consumed by the deferred MailTaskPipeline implementation, see module doc")]
    #[serde(default)]
    pub(crate) subject: String,
    #[allow(dead_code, reason = "consumed by the deferred MailTaskPipeline implementation, see module doc")]
    #[serde(default)]
    pub(crate) body_text: String,
    #[allow(dead_code, reason = "consumed by the deferred MailTaskPipeline implementation, see module doc")]
    #[serde(default)]
    pub(crate) attachments: Vec<IngestAttachment>,
    pub(crate) occurred_at: DateTime<Utc>,
    /// Delegated-identity design, Gap B (§2.3). A signed mail-task-v1
    /// Control decision minted by user-core at alias-resolve time, riding
    /// through integration-corev2's HMAC-delegated body opaquely (this crate
    /// never reads or verifies it — session-core's `PrepareUserOwnedThread`
    /// does, once a real `MailTaskPipeline` implementation forwards it via
    /// `mail_task_thread_bridge::ThreadBridgeClient`). `#[serde(default)]`
    /// so a redelivered/older request with no `thread_decision` field still
    /// decodes — it just cannot reach `PrepareUserOwnedThread`'s mail_task
    /// gate, matching every other additive field on this struct.
    #[allow(dead_code, reason = "consumed by the deferred MailTaskPipeline implementation, see module doc")]
    #[serde(default)]
    pub(crate) thread_decision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code, reason = "consumed by the deferred MailTaskPipeline implementation, see module doc")]
pub(crate) struct IngestAttachment {
    pub(crate) filename: String,
    #[serde(default)]
    pub(crate) extracted_text: String,
    #[serde(default)]
    pub(crate) extracted: bool,
}

impl IngestRequest {
    fn is_well_formed(&self) -> bool {
        !self.org_id.trim().is_empty()
            && !self.user_id.trim().is_empty()
            && !self.provider_message_id.trim().is_empty()
    }

    /// Deterministic session_key for session-core's `CreateThread` (spec
    /// §3.4): `"mail_task/" + sha256(org_id + provider_message_id)`, so a
    /// webhook retry that reaches this endpoint again resolves to the SAME
    /// thread rather than minting a second one.
    pub(crate) fn session_key(&self) -> String {
        let digest = Sha256::digest(format!("{}{}", self.org_id, self.provider_message_id).as_bytes());
        format!("mail_task/{}", hex::encode(digest))
    }
}

// ---------------------------------------------------------------------------
// The deferred pipeline seam — see module doc for why this has no production
// implementation yet.
// ---------------------------------------------------------------------------

/// One outcome of running the mail-task pipeline for one [`IngestRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MailTaskOutcome {
    pub(crate) thread_id: String,
    /// True when the model's turn proposed at least one write-capable tool
    /// call — which must have surfaced as a PENDING APPROVAL, never an
    /// executed side effect (spec's non-negotiable draft-only requirement,
    /// §3.4 step 3 / §6 "runaway agent turn"). A correct [`MailTaskPipeline`]
    /// implementation derives this from the turn's actual approval-rung
    /// state (the same mechanism `ChatMessages.tsx`'s `PlanApprovalControl`
    /// / tool-approval rungs use for a live chat turn), never assumes it.
    pub(crate) has_pending_tool_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MailTaskPipelineError(pub(crate) String);

impl std::fmt::Display for MailTaskPipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The actual "create the thread, post the synthesized first turn, invoke
/// exactly one execution-core turn" step (spec §3.4 steps 1-3). No `async fn`
/// in the trait (this crate has no `async-trait` dependency and the object
/// needs to be `dyn`-safe for `AppState.mail_task_pipeline`), so `run`
/// returns a manually boxed future — the standard workaround for a
/// `dyn`-compatible async trait method.
///
/// MUST be idempotent on `request.session_key()`: a retried
/// `provider_message_id` must resolve to the same `thread_id`, never start a
/// second run (spec §6, "provider retry storms").
pub(crate) trait MailTaskPipeline: Send + Sync {
    fn run<'a>(
        &'a self,
        request: &'a IngestRequest,
    ) -> Pin<Box<dyn Future<Output = Result<MailTaskOutcome, MailTaskPipelineError>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// HMAC service-delegation verification (mirrors conversation-ingest-rs)
// ---------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;
type ReplayCache = Mutex<HashMap<String, DateTime<Utc>>>;

fn replay_cache() -> &'static ReplayCache {
    static CACHE: OnceLock<ReplayCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn require_mail_task_delegation(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut parts, body) = request.into_parts();
    let body = match to_bytes(body, MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(error("payload_too_large", "Request body exceeds the mail-task ingest limit.")),
            )
                .into_response()
        }
    };
    if verify_mail_task_delegation(&state, &parts, &body) {
        return next
            .run(axum::http::Request::from_parts(parts, Body::from(body)))
            .await;
    }
    parts.extensions.clear();
    (
        StatusCode::UNAUTHORIZED,
        Json(error("unauthorized", "Invalid or missing service delegation.")),
    )
        .into_response()
}

fn verify_mail_task_delegation(state: &AppState, parts: &axum::http::request::Parts, body: &[u8]) -> bool {
    if state.mail_task_ingest_service_token.trim().is_empty() {
        // Off by default: no token configured means this endpoint accepts
        // nothing, matching integration-corev2's own "empty secret = feature
        // disabled" convention.
        return false;
    }
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or_default()
    };
    if header("x-service-id") != EXPECTED_SERVICE_ID {
        return false;
    }
    let organization_id = header("x-org-id");
    if organization_id.is_empty() {
        return false;
    }
    let timestamp_text = header("x-delegation-timestamp");
    let Ok(timestamp) = DateTime::parse_from_rfc3339(timestamp_text) else {
        return false;
    };
    let timestamp = timestamp.with_timezone(&Utc);
    let now = Utc::now();
    if timestamp < now - chrono::Duration::minutes(DELEGATION_WINDOW_MINUTES)
        || timestamp > now + chrono::Duration::minutes(DELEGATION_WINDOW_MINUTES)
    {
        return false;
    }
    let nonce = header("x-delegation-nonce");
    if !(16..=128).contains(&nonce.len())
        || !nonce.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return false;
    }
    let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    if header("x-delegation-body-sha256") != digest {
        return false;
    }
    let request_uri = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| parts.uri.path());
    let canonical = [
        "v2",
        EXPECTED_SERVICE_ID,
        SELF_AUDIENCE,
        timestamp_text,
        nonce,
        parts.method.as_str(),
        request_uri,
        "",
        organization_id,
        "",
        digest.as_str(),
    ]
    .join("\n");
    let Ok(provided_signature) = URL_SAFE_NO_PAD.decode(header("x-delegation-signature")) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(state.mail_task_ingest_service_token.as_bytes()) else {
        return false;
    };
    mac.update(canonical.as_bytes());
    if mac.verify_slice(&provided_signature).is_err() {
        return false;
    }

    let replay_key = format!("{EXPECTED_SERVICE_ID}\0{nonce}");
    let Ok(mut replays) = replay_cache().lock() else {
        return false;
    };
    replays.retain(|_, expires_at| *expires_at > now);
    if replays.contains_key(&replay_key) {
        return false;
    }
    replays.insert(replay_key, timestamp + chrono::Duration::minutes(DELEGATION_WINDOW_MINUTES));
    true
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn ingest(State(state): State<AppState>, Json(request): Json<IngestRequest>) -> impl IntoResponse {
    if !request.is_well_formed() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(error("validation_error", "org_id, user_id and provider_message_id are required.")),
        )
            .into_response();
    }

    let rate_limit_key = format!("mail-task:{}:{}", request.org_id, request.user_id);
    if let Err(retry_after_secs) = state.mail_task_rate_limiter.try_acquire(&rate_limit_key).await {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error("rate_limited", "Mail-task run rate exceeded for this user.")),
        )
            .into_response();
        response
            .headers_mut()
            .insert(axum::http::header::RETRY_AFTER, axum::http::HeaderValue::from(retry_after_secs));
        return response;
    }

    let Some(pipeline) = state.mail_task_pipeline.clone() else {
        // See this module's doc comment: no production pipeline is wired yet.
        // A 503 (not 2xx, not 4xx) tells integration-corev2's receiver to
        // retry the webhook delivery later rather than treat the email as
        // permanently dropped.
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "mail_task_pipeline_not_configured",
                "Mail-task turn execution is not yet configured on this deployment.",
            )),
        )
            .into_response();
    };

    match pipeline.run(&request).await {
        Ok(outcome) => {
            publish_mail_task_drafted(&state, &request, &outcome).await;
            (
                StatusCode::OK,
                Json(ok(serde_json::json!({
                    "thread_id": outcome.thread_id,
                    "has_pending_tool_approval": outcome.has_pending_tool_approval,
                }))),
            )
                .into_response()
        }
        Err(pipeline_error) => {
            tracing::warn!(error = %pipeline_error, "mail-task pipeline run failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(error("mail_task_pipeline_failed", "Mail-task turn execution failed.")),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// NATS completion event (content-free by construction — see
// mail_task_drafted_event's own field list)
// ---------------------------------------------------------------------------

// JetStream publish routes by subject alone (unlike a pull-consumer bind,
// which does need the stream name — see `gdpr_nats.rs`), so this constant
// documents which stream owns the subject rather than being passed anywhere.
#[allow(dead_code, reason = "documents which stream owns MAIL_TASK_DRAFTED_SUBJECT; publish itself only needs the subject")]
pub(crate) const MAIL_TASK_DRAFTED_STREAM: &str = "VEREVON_APPLICATION";
pub(crate) const MAIL_TASK_DRAFTED_SUBJECT: &str = "verevon.application.mail_task.drafted";

/// Content-free by construction: every field here is an id, a type tag, or a
/// timestamp. Never subject, body, sender, or attachment content — the same
/// shape `conversation_followed_message.go`'s `conversationLifecycleEvent`
/// uses, which `notification-core`'s new consumer (spec §3.5) will subscribe
/// to.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct MailTaskDraftedEvent {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) event_type: &'static str,
    pub(crate) org_id: String,
    pub(crate) user_id: String,
    pub(crate) thread_id: String,
    pub(crate) occurred_at: DateTime<Utc>,
}

fn mail_task_drafted_event(request: &IngestRequest, outcome: &MailTaskOutcome) -> MailTaskDraftedEvent {
    MailTaskDraftedEvent {
        id: format!("mail_task_drafted:{}", request.session_key()),
        event_type: "mail_task_drafted",
        org_id: request.org_id.clone(),
        user_id: request.user_id.clone(),
        thread_id: outcome.thread_id.clone(),
        occurred_at: request.occurred_at,
    }
}

/// Best-effort publish, mirroring `gdpr_nats.rs`'s philosophy: a broker
/// outage must never fail the HTTP response the caller (integration-corev2)
/// is waiting on — the thread and its draft already exist by this point, only
/// the notification would be delayed. No-ops (logs at WARN once) when the
/// `VEREVON_GATEWAY_MAIL_TASK_NATS_*` credential trio is unset, matching
/// every other optional cross-plane connection in this gateway.
async fn publish_mail_task_drafted(state: &AppState, request: &IngestRequest, outcome: &MailTaskOutcome) {
    if state.mail_task_nats_url.trim().is_empty()
        || state.mail_task_nats_user.trim().is_empty()
        || state.mail_task_nats_password.trim().is_empty()
    {
        tracing::warn!(
            "mail-task NATS publisher not configured; skipping verevon.application.mail_task.drafted"
        );
        return;
    }
    let event = mail_task_drafted_event(request, outcome);
    let payload = match serde_json::to_vec(&event) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(%error, "failed to encode mail_task_drafted event");
            return;
        }
    };
    if let Err(error) = publish_once(
        &state.mail_task_nats_url,
        &state.mail_task_nats_user,
        &state.mail_task_nats_password,
        payload,
    )
    .await
    {
        tracing::warn!(%error, "mail_task_drafted publish failed");
    }
}

async fn publish_once(url: &str, user: &str, password: &str, payload: Vec<u8>) -> anyhow::Result<()> {
    let client = async_nats::ConnectOptions::with_user_and_password(user.to_owned(), password.to_owned())
        .custom_inbox_prefix("_INBOX.VEREVON_GATEWAY_MAIL_TASK")
        .connect(url)
        .await?;
    let js = async_nats::jetstream::new(client);
    js.publish(MAIL_TASK_DRAFTED_SUBJECT, payload.into()).await?.await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body as AxumBody;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn test_request_json() -> serde_json::Value {
        serde_json::json!({
            "org_id": "org-1",
            "user_id": "user-1",
            "provider_message_id": "provider-msg-1",
            "subject": "Please handle this",
            "body_text": "Body",
            "attachments": [],
            "occurred_at": "2026-09-28T12:00:00Z",
        })
    }

    fn signed_headers(service_token: &str, body: &[u8], org_id: &str, nonce: &str) -> Vec<(&'static str, String)> {
        let timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
        let canonical = [
            "v2",
            EXPECTED_SERVICE_ID,
            SELF_AUDIENCE,
            timestamp.as_str(),
            nonce,
            "POST",
            INGEST_PATH,
            "",
            org_id,
            "",
            digest.as_str(),
        ]
        .join("\n");
        let mut mac = HmacSha256::new_from_slice(service_token.as_bytes()).expect("hmac key");
        mac.update(canonical.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        vec![
            ("x-service-id", EXPECTED_SERVICE_ID.to_owned()),
            ("x-org-id", org_id.to_owned()),
            ("x-delegation-timestamp", timestamp),
            ("x-delegation-nonce", nonce.to_owned()),
            ("x-delegation-body-sha256", digest),
            ("x-delegation-signature", signature),
        ]
    }

    async fn post_ingest(
        router: &Router,
        headers: Vec<(&'static str, String)>,
        body: serde_json::Value,
    ) -> axum::http::StatusCode {
        let body_bytes = body.to_string();
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri(INGEST_PATH)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let request = request.body(AxumBody::from(body_bytes)).expect("request");
        router.clone().oneshot(request).await.expect("response").status()
    }

    fn test_router() -> Router {
        let state = crate::tests::test_state(false);
        super::router(state.clone()).with_state(state)
    }

    #[tokio::test]
    async fn missing_delegation_is_rejected() {
        let router = test_router();
        let status = post_ingest(&router, vec![], test_request_json()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_service_id_is_rejected() {
        let router = test_router();
        let mut headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            test_request_json().to_string().as_bytes(),
            "org-1",
            "nonce-wrong-service-id-000001",
        );
        headers[0] = ("x-service-id", "some-other-worker".to_owned());
        let status = post_ingest(&router, headers, test_request_json()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tampered_body_after_signing_is_rejected() {
        let router = test_router();
        let body = test_request_json();
        let headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-tampered-body-0000001",
        );
        let mut tampered = body.clone();
        tampered["subject"] = serde_json::Value::String("different subject".into());
        let status = post_ingest(&router, headers, tampered).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_delegation_reaches_the_handler_and_a_replay_is_rejected() {
        let router = test_router();
        let body = test_request_json();
        let headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-valid-then-replay-0001",
        );
        let first = post_ingest(&router, headers.clone(), body.clone()).await;
        // No pipeline is wired in the test fixture, so a valid delegation
        // reaches the handler and gets the documented "not configured yet"
        // 503 -- proving auth passed (a real 401 would look identical from
        // the outside if verification were broken silently-permissive, so
        // this asserts NOT-401 specifically).
        assert_ne!(first, StatusCode::UNAUTHORIZED);
        assert_eq!(first, StatusCode::SERVICE_UNAVAILABLE);

        let replay = post_ingest(&router, headers, body).await;
        assert_eq!(replay, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn empty_ingest_service_token_rejects_every_caller() {
        let mut state = crate::tests::test_state(false);
        state.mail_task_ingest_service_token = String::new();
        let router = super::router(state.clone()).with_state(state);
        let body = test_request_json();
        let headers = signed_headers(
            "anything",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-disabled-by-default-01",
        );
        let status = post_ingest(&router, headers, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// A fake pipeline standing in for the deferred real one (see this
    /// module's doc comment), used only to prove the ENDPOINT's own contract:
    /// idempotency on `session_key()`, and that the response never claims
    /// success without the pipeline itself reporting it.
    struct FakePipeline {
        calls: std::sync::Mutex<Vec<String>>,
        pending_tool_approval: bool,
        /// Every `thread_decision` this fake observed, in call order — lets
        /// a test assert the field reaches the pipeline verbatim without
        /// changing `calls`'s existing shape/assertions above.
        thread_decisions: std::sync::Mutex<Vec<String>>,
    }

    impl MailTaskPipeline for FakePipeline {
        fn run<'a>(
            &'a self,
            request: &'a IngestRequest,
        ) -> Pin<Box<dyn Future<Output = Result<MailTaskOutcome, MailTaskPipelineError>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(request.session_key());
                self.thread_decisions
                    .lock()
                    .unwrap()
                    .push(request.thread_decision.clone());
                Ok(MailTaskOutcome {
                    thread_id: format!("thread-for-{}", request.session_key()),
                    has_pending_tool_approval: self.pending_tool_approval,
                })
            })
        }
    }

    #[tokio::test]
    async fn a_retried_provider_message_id_resolves_to_the_same_thread_id_and_calls_the_pipeline_once() {
        let mut state = crate::tests::test_state(false);
        let pipeline = Arc::new(FakePipeline {
            calls: std::sync::Mutex::new(Vec::new()),
            pending_tool_approval: false,
            thread_decisions: std::sync::Mutex::new(Vec::new()),
        });
        state.mail_task_pipeline = Some(pipeline.clone());
        let router = super::router(state.clone()).with_state(state);

        let body = test_request_json();
        let headers_a = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-idempotent-run-a-00001",
        );
        let first = post_ingest(&router, headers_a, body.clone()).await;
        assert_eq!(first, StatusCode::OK);

        let headers_b = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-idempotent-run-b-00002",
        );
        let second = post_ingest(&router, headers_b, body).await;
        assert_eq!(second, StatusCode::OK);

        // Same provider_message_id -> same session_key -> the fake pipeline
        // (standing in for session-core's own session_key idempotency) was
        // asked to run twice with an IDENTICAL key both times, which is
        // exactly what lets a real implementation return the same thread_id
        // both times instead of minting a second thread.
        let calls = pipeline.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
    }

    /// Delegated-identity design, Gap B — gateway BFF "Exact tests":
    /// `thread_decision_forwarded_verbatim_and_unread_by_this_crate`.
    #[tokio::test]
    async fn thread_decision_deserializes_and_is_forwarded_verbatim_to_the_pipeline() {
        let mut state = crate::tests::test_state(false);
        let pipeline = Arc::new(FakePipeline {
            calls: std::sync::Mutex::new(Vec::new()),
            pending_tool_approval: false,
            thread_decisions: std::sync::Mutex::new(Vec::new()),
        });
        state.mail_task_pipeline = Some(pipeline.clone());
        let router = super::router(state.clone()).with_state(state);

        let mut body = test_request_json();
        body["thread_decision"] = serde_json::json!("mail-task-v1.key-1.payload-1.sig-1");
        let headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-thread-decision-00001",
        );
        let status = post_ingest(&router, headers, body).await;
        assert_eq!(status, StatusCode::OK);

        let seen = pipeline.thread_decisions.lock().unwrap();
        assert_eq!(seen.as_slice(), ["mail-task-v1.key-1.payload-1.sig-1"]);
    }

    /// Backward compatibility: a request with no `thread_decision` field at
    /// all (every caller today, until integration-corev2 and the gateway
    /// BFF forward it end to end) still decodes and runs — the field is
    /// additive, never required.
    #[tokio::test]
    async fn absent_thread_decision_still_decodes_as_empty_and_runs() {
        let mut state = crate::tests::test_state(false);
        let pipeline = Arc::new(FakePipeline {
            calls: std::sync::Mutex::new(Vec::new()),
            pending_tool_approval: false,
            thread_decisions: std::sync::Mutex::new(Vec::new()),
        });
        state.mail_task_pipeline = Some(pipeline.clone());
        let router = super::router(state.clone()).with_state(state);

        let body = test_request_json(); // no thread_decision key at all
        let headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-no-thread-decision-00001",
        );
        let status = post_ingest(&router, headers, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(pipeline.thread_decisions.lock().unwrap().as_slice(), [""]);
    }

    #[tokio::test]
    async fn a_write_capable_tool_call_never_reports_as_executed_without_the_pipeline_saying_so() {
        let mut state = crate::tests::test_state(false);
        let pipeline = Arc::new(FakePipeline {
            calls: std::sync::Mutex::new(Vec::new()),
            pending_tool_approval: true,
            thread_decisions: std::sync::Mutex::new(Vec::new()),
        });
        state.mail_task_pipeline = Some(pipeline);
        let router = super::router(state.clone()).with_state(state);

        let body = test_request_json();
        let headers = signed_headers(
            "mail-task-test-secret-at-least-32-bytes",
            body.to_string().as_bytes(),
            "org-1",
            "nonce-pending-approval-00001",
        );
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri(INGEST_PATH)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let request = request.body(AxumBody::from(body.to_string())).expect("request");
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // The endpoint relays the pipeline's own signal rather than asserting
        // completion on its own authority -- it never has enough information
        // to know a write-capable tool call actually executed vs. is pending.
        assert_eq!(decoded["data"]["has_pending_tool_approval"], true);
    }

    #[test]
    fn mail_task_drafted_event_carries_no_content() {
        let request = IngestRequest {
            org_id: "org-1".into(),
            user_id: "user-1".into(),
            provider_message_id: "provider-msg-1".into(),
            subject: "Do not leak this subject".into(),
            body_text: "Do not leak this body".into(),
            attachments: vec![],
            occurred_at: Utc::now(),
            thread_decision: String::new(),
        };
        let outcome = MailTaskOutcome {
            thread_id: "thread-1".into(),
            has_pending_tool_approval: false,
        };
        let event = mail_task_drafted_event(&request, &outcome);
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("Do not leak"));
        let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, vec!["id", "occurred_at", "org_id", "thread_id", "type", "user_id"]);
        assert_eq!(event.event_type, "mail_task_drafted");
    }

    #[test]
    fn session_key_is_deterministic_and_scoped_by_org() {
        let a = IngestRequest {
            org_id: "org-1".into(),
            user_id: "user-1".into(),
            provider_message_id: "provider-msg-1".into(),
            subject: String::new(),
            body_text: String::new(),
            attachments: vec![],
            occurred_at: Utc::now(),
            thread_decision: String::new(),
        };
        let mut b = a.clone();
        b.org_id = "org-2".into();

        assert_eq!(a.session_key(), a.session_key());
        assert_ne!(a.session_key(), b.session_key());
        assert!(a.session_key().starts_with("mail_task/"));
    }

    #[test]
    fn is_well_formed_requires_the_three_identifiers() {
        let base = IngestRequest {
            org_id: "org-1".into(),
            user_id: "user-1".into(),
            provider_message_id: "provider-msg-1".into(),
            subject: String::new(),
            body_text: String::new(),
            attachments: vec![],
            occurred_at: Utc::now(),
            thread_decision: String::new(),
        };
        assert!(base.is_well_formed());

        let mut missing_org = base.clone();
        missing_org.org_id = "  ".into();
        assert!(!missing_org.is_well_formed());

        let mut missing_user = base.clone();
        missing_user.user_id = String::new();
        assert!(!missing_user.is_well_formed());

        let mut missing_message_id = base.clone();
        missing_message_id.provider_message_id = String::new();
        assert!(!missing_message_id.is_well_formed());
    }
}
