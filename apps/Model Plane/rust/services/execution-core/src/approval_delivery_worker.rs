//! Post-approval continuation dispatcher.
//!
//! HITL can already pause a risky tool call durably (`runtime_loop::agent::
//! pause_for_approval`) and Session Core already records a leaseable
//! `approval_delivery_outbox` row the moment a human grants it. What was
//! missing — the reason a granted approval could not yet resume anything —
//! was a worker on the other end of that lease. This module is that worker:
//! it claims due deliveries, fetches the exact immutable action descriptor
//! under its lease, re-executes the one now-attestable action kind
//! (`execute_provider_action`, see `attestation`/`integration_tools`), and
//! records an authoritative outcome.
//!
//! # Scope, deliberately narrow
//!
//! `is_risky_tool` gates a wide set of tools (shell, browser_agent, any MCP
//! tool, …), but only two are resumable here: `execute_provider_action` and
//! `book_shipment`. Both share the shape that makes cold resumption safe —
//! a single idempotent-key-gated HTTP call with no session state to
//! reconstruct, and a durable receipt on the other end (integration-corev2's
//! `ActionReceipt`, shipping-core's own booking record) that makes a retry
//! of an already-completed call a safe no-op rather than a duplicate side
//! effect. `browser_agent` (a live multi-step navigation session) and
//! arbitrary MCP tools (unknown, third-party idempotency semantics) do not
//! share that shape — re-attempting either from a cold descriptor is a
//! different, larger problem this worker does not attempt. A descriptor
//! naming any other tool fails closed as `invalid_continuation` rather than
//! attempting something unverified.
//!
//! # Which orgs this worker services
//!
//! `ClaimApprovalDeliveries` requires a service credential scoped to the
//! exact org being claimed (`VerifiedIdentity::authorize_org` is a strict
//! equality check — there is no "all orgs" bypass for this RPC, unlike the
//! internal-only empty-org-id path `ListPendingApprovals` offers). So
//! servicing every org means enumerating them first: `org_directory`
//! auto-discovers the full list from org-core's own `GET /internal/orgs`
//! (an org-core-native endpoint added alongside this worker — orgs are
//! Verevon's own data, not a third-party system, so there was never a reason
//! this had to stay a manual list). `EXECUTION_CORE_APPROVAL_DELIVERY_ORG_IDS`
//! remains as an explicit override for staged rollout (e.g. piloting on one
//! org before trusting the dispatcher broadly), not the normal path.
//!
//! # Protocol
//!
//! Per delivery: `GetApprovalContinuation` → parse/validate the descriptor →
//! **either** skip straight to `AcknowledgeApprovalDelivery` (Terminal, with
//! an allowlisted failure code) for anything that fails before execution
//! would even be attempted, **or** `RecordApprovalContinuationStarted` (never
//! execute if `already_started` — a prior worker owns that attempt) →
//! execute → `RecordApprovalContinuationOutcome` → `AcknowledgeApprovalDelivery`
//! (Settled after Completed, Terminal/Retry after Failed depending on
//! whether the failure is structurally unfixable or worth another pass).
//! `RecordApprovalContinuationOutcome`'s `receipt_id` only exists once
//! `RecordApprovalContinuationStarted` has run, which is why pre-execution
//! failures cannot go through that pair at all — `AcknowledgeApprovalDelivery`
//! carries its own independent `failure_code` for exactly this case.
//!
//! A pure gRPC/HTTP failure (session-core or integration-corev2 unreachable)
//! is never acknowledged — the lease simply expires and a later poll reclaims
//! it, the same fail-open-to-retry behavior the outbox was designed for.
//!
//! # Verified Outcome Foundation (verevon-roadmap.md §3b)
//!
//! Every recorded outcome also carries a [`pb::VerificationResult`] — this
//! worker is the first real producer of that shared cross-domain contract.
//!
//! There are now two judgment strengths, distinguished on the wire by
//! `method`:
//!
//! - `"structural"` — the default and the fallback. `Completed` with a
//!   non-empty `provider_receipt_id` is `VERIFIED_SUCCESS`, everything else
//!   is `VERIFIED_FAILURE`. This only establishes that the boundary handed
//!   back an authoritative id, **not** that the effect exists. Do not read a
//!   structural `VERIFIED_SUCCESS` as stronger proof than it is.
//! - `"postcondition"` — an independent re-read of the system of record
//!   (§3b P1 item 3, see [`crate::postcondition`]). Implemented today for
//!   `book_shipment`, which is re-read from shipping-core after booking.
//!
//! The precedence is enforced in `verification_result_with_postcondition`: a
//! confirmation upgrades the method, a refutation **overrides a structural
//! success into a failure** (the false-success case this exists to catch),
//! and anything inconclusive — including an unreachable provider — leaves
//! the structural judgment exactly as it was. A check that could not run is
//! never credited as one that ran and passed.

#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::model_plane::v1::{
    self as pb, orchestration_core_service_client::OrchestrationCoreServiceClient,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::integration_tools::IntegrationActionsClient;

const SESSION_CORE_AUDIENCE: &str = "session-core";
const APPROVAL_DELIVER_SCOPE: &str = "approval:deliver";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;

/// The two resumable tool names — see the module doc for why the rest of
/// `is_risky_tool`'s set (`browser_agent`, arbitrary MCP tools, …) isn't
/// here.
const PROVIDER_ACTION_TOOL_NAME: &str = "execute_provider_action";
const SHIPMENT_BOOKING_TOOL_NAME: &str = "book_shipment";

/// The exact allowlist `session-core`'s `validate_failure_code` enforces.
/// Any other string is rejected server-side, so these are reproduced here
/// verbatim rather than invented independently.
const FAILURE_CONTINUATION_UNAVAILABLE: &str = "continuation_unavailable";
const FAILURE_INVALID_CONTINUATION: &str = "invalid_continuation";
const FAILURE_TRANSIENT_DEPENDENCY: &str = "transient_dependency";

// ---------------------------------------------------------------------
// Token provider — mints session-core `approval:deliver` tokens, one per
// org, cached until near expiry. Mirrors session_terminal_auth's shape for
// an unrelated scope; kept as its own small type rather than folded into
// that one, matching this crate's existing convention of one focused
// token-minting client per trust boundary (see also
// integration_tools::mint_ingestion_token, shipping_tools).
// ---------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub(crate) enum ApprovalDeliveryTokenError {
    #[error("approval delivery worker is not configured: {0}")]
    Configuration(&'static str),
    #[error("approval delivery worker requires an organization")]
    MissingOrganization,
    #[error("Auth Core approval-delivery token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the approval-delivery credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid approval-delivery token")]
    InvalidResponse,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    #[serde(default)]
    audience: String,
}

pub(crate) struct ApprovalDeliveryTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Mutex<std::collections::HashMap<String, CachedToken>>,
    in_flight: DashMap<String, Arc<Mutex<()>>>,
}

impl ApprovalDeliveryTokenProvider {
    pub(crate) fn from_env() -> Result<Self, ApprovalDeliveryTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("EXECUTION_CORE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "execution-core".to_owned());
        let credential = required_env("EXECUTION_CORE_SERVICE_API_KEY")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
            cache: Mutex::new(std::collections::HashMap::new()),
            in_flight: DashMap::new(),
        })
    }

    #[cfg(test)]
    fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("test http client"),
            cache: Mutex::new(std::collections::HashMap::new()),
            in_flight: DashMap::new(),
        }
    }

    pub(crate) async fn token(&self, org_id: &str) -> Result<String, ApprovalDeliveryTokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(ApprovalDeliveryTokenError::MissingOrganization);
        }
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
            if let Some(cached) = cache.get(org_id) {
                return Ok(cached.token.clone());
            }
        }
        let mint_lock = self
            .in_flight
            .entry(org_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let mint_guard = mint_lock.lock().await;
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(org_id) {
                if cached.expires_at > Instant::now() + REFRESH_SKEW {
                    let token = cached.token.clone();
                    drop(cache);
                    drop(mint_guard);
                    self.forget_unused_lock(org_id, &mint_lock);
                    return Ok(token);
                }
            }
        }
        let minted = match self.mint(org_id).await {
            Ok(minted) => minted,
            Err(error) => {
                drop(mint_guard);
                self.forget_unused_lock(org_id, &mint_lock);
                return Err(error);
            }
        };
        let token = minted.token.clone();
        {
            let mut cache = self.cache.lock().await;
            cache.insert(
                org_id.to_owned(),
                CachedToken {
                    token: minted.token,
                    expires_at: Instant::now() + Duration::from_secs(minted.expires_in_seconds),
                },
            );
        }
        drop(mint_guard);
        self.forget_unused_lock(org_id, &mint_lock);
        Ok(token)
    }

    async fn mint(&self, org_id: &str) -> Result<TokenResponse, ApprovalDeliveryTokenError> {
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                ApprovalDeliveryTokenError::Configuration("EXECUTION_CORE_SERVICE_API_KEY")
            })?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{SESSION_CORE_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [APPROVAL_DELIVER_SCOPE],
                "reason": format!("{} approval delivery", self.service_id),
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ApprovalDeliveryTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| ApprovalDeliveryTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != SESSION_CORE_AUDIENCE
        {
            return Err(ApprovalDeliveryTokenError::InvalidResponse);
        }
        Ok(bundle)
    }

    fn forget_unused_lock(&self, org_id: &str, expected: &Arc<Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.in_flight
                .remove_if(org_id, |_, current| Arc::ptr_eq(current, expected));
        }
    }
}

fn required_env(name: &'static str) -> Result<String, ApprovalDeliveryTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ApprovalDeliveryTokenError::Configuration(name))
}

// ---------------------------------------------------------------------
// Pure descriptor parsing and outcome/acknowledgement decisions. No network
// calls below this point — everything here is a plain function of its
// inputs, which is what the unit tests exercise directly.
// ---------------------------------------------------------------------

/// Mirrors `runtime_loop::agent::continuation_descriptor`'s exact JSON shape.
#[derive(Debug, Deserialize, PartialEq)]
struct ContinuationDescriptor {
    version: u32,
    #[serde(default)]
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    step_id: String,
    action_kind: String,
    tool_name: String,
    input: Value,
}

/// `execute_provider_action`'s own tool-input shape (`runtime_loop::mod::
/// execute_provider_action`'s local `ActionInput`), re-parsed from the
/// descriptor's retained `input`.
#[derive(Debug, Deserialize, PartialEq)]
struct ProviderActionInput {
    connection_id: String,
    operation: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    body: Value,
}

/// One resumable action, already parsed into its tool's own input shape.
/// `book_shipment`'s `BookInput` isn't `PartialEq` (nested address/customs
/// fields aren't worth threading that derive through for test purposes
/// alone), so this enum itself only derives `Debug` — tests destructure a
/// variant and assert on its individual fields instead of comparing the
/// whole enum.
#[derive(Debug)]
enum ResumableAction {
    ProviderAction(ProviderActionInput),
    ShipmentBooking(Box<crate::shipping_tools::BookInput>),
}

/// What this pass decided to do with one claimed delivery, independent of
/// how that decision gets carried out over gRPC.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Disposition {
    /// Never reached RecordApprovalContinuationStarted: acknowledge directly
    /// with this allowlisted code, Terminal (nothing about retrying a
    /// descriptor that will never parse or a tool this worker cannot resume
    /// would help).
    RejectBeforeStart { failure_code: &'static str },
    /// A prior worker already recorded the start. Do not execute again, do
    /// not record any outcome — just acknowledge Retry.
    AlreadyStarted,
    /// Started, executed, got an authoritative provider receipt: record
    /// Completed, then acknowledge Settled with the same receipt id.
    Completed { provider_receipt_id: String },
    /// Started, but execution did not produce a usable outcome. Recorded as
    /// Failed with this allowlisted code; acknowledged Retry (the outbox's
    /// own bounded backoff governs how many more attempts happen — see
    /// `MAX_APPROVAL_DELIVERY_ATTEMPTS`/`reap_undeliverable_deliveries` in
    /// session-core, which this worker deliberately does not reimplement).
    FailedRetryable { failure_code: &'static str },
}

fn parse_descriptor(raw: &str) -> Result<ContinuationDescriptor, &'static str> {
    let descriptor: ContinuationDescriptor =
        serde_json::from_str(raw).map_err(|_| FAILURE_INVALID_CONTINUATION)?;
    if descriptor.version != 1 || descriptor.action_kind != "tool_call" {
        return Err(FAILURE_INVALID_CONTINUATION);
    }
    Ok(descriptor)
}

fn parse_resumable_action(
    descriptor: &ContinuationDescriptor,
) -> Result<ResumableAction, &'static str> {
    if descriptor.org_id.trim().is_empty() || descriptor.user_id.trim().is_empty() {
        return Err(FAILURE_INVALID_CONTINUATION);
    }
    match descriptor.tool_name.as_str() {
        PROVIDER_ACTION_TOOL_NAME => serde_json::from_value(descriptor.input.clone())
            .map(ResumableAction::ProviderAction)
            .map_err(|_| FAILURE_INVALID_CONTINUATION),
        SHIPMENT_BOOKING_TOOL_NAME => serde_json::from_value(descriptor.input.clone())
            .map(|input| ResumableAction::ShipmentBooking(Box::new(input)))
            .map_err(|_| FAILURE_INVALID_CONTINUATION),
        _ => Err(FAILURE_INVALID_CONTINUATION),
    }
}

/// What to do when `GetApprovalContinuation` reports no descriptor at all
/// (a stale/mismatched lease and a descriptor-free approval are
/// indistinguishable by design — see the proto comment on
/// `GetApprovalContinuationResponse.available` — so both map to the same
/// terminal disposition here).
fn disposition_for_missing_descriptor() -> Disposition {
    Disposition::RejectBeforeStart {
        failure_code: FAILURE_CONTINUATION_UNAVAILABLE,
    }
}

fn disposition_for_provider_action_result(
    result: Result<crate::integration_tools::ActionOutcome, String>,
) -> Disposition {
    match result {
        Ok(outcome) => match outcome.provider_receipt_id {
            Some(provider_receipt_id) => Disposition::Completed {
                provider_receipt_id,
            },
            // integration-corev2 accepted and durably completed the write,
            // but its response carried no id-like field this heuristic
            // could find. A retry would replay the SAME cached completed
            // receipt (integration-corev2's own idempotency key covers this
            // exact action) and hit the identical extraction gap
            // deterministically — so Terminal, not Retry, is the honest
            // choice; looping would never resolve it.
            None => Disposition::RejectBeforeStart {
                failure_code: FAILURE_INVALID_CONTINUATION,
            },
        },
        Err(_) => disposition_for_transient_failure(),
    }
}

/// Unlike a provider action's heuristic receipt extraction,
/// `book_shipment`'s `booking_id` is always present on `Ok` — shipping-core's
/// create step either returns one or the call already failed before
/// confirm — so there is no "succeeded but unreceipted" case to consider
/// here.
fn disposition_for_shipment_booking_result(
    result: Result<crate::shipping_tools::BookingOutcome, String>,
) -> Disposition {
    match result {
        Ok(outcome) => Disposition::Completed {
            provider_receipt_id: outcome.booking_id,
        },
        Err(_) => disposition_for_transient_failure(),
    }
}

/// Anything from a network blip to a permanent 4xx lands here, for either
/// resumable tool. Retrying is always safe — each tool's own idempotency key
/// (integration-corev2's request-hash receipt; shipping-core's booking
/// idempotency key) never re-executes a write it already completed — even
/// when retrying is ultimately futile; a persistently failing delivery
/// self-terminates via session-core's own `MAX_APPROVAL_DELIVERY_ATTEMPTS`
/// reaper rather than needing this worker to classify the failure itself.
fn disposition_for_transient_failure() -> Disposition {
    Disposition::FailedRetryable {
        failure_code: FAILURE_TRANSIENT_DEPENDENCY,
    }
}

/// Maps a decided disposition onto the one `AcknowledgeApprovalDelivery` call
/// that finalizes it (plus, when applicable, the failure code / receipt to
/// carry). Pulled out as its own function so the acknowledgement shape is
/// exhaustively covered and directly testable.
fn acknowledgement_for(disposition: &Disposition) -> (pb::ApprovalDeliveryAcknowledgement, &str) {
    match disposition {
        Disposition::RejectBeforeStart { failure_code } => {
            (pb::ApprovalDeliveryAcknowledgement::Terminal, failure_code)
        }
        Disposition::AlreadyStarted => (
            pb::ApprovalDeliveryAcknowledgement::Retry,
            // Retry requires some allowlisted code even though nothing
            // failed from this worker's own perspective; the closest honest
            // fit is "another attempt is in flight, check back."
            FAILURE_TRANSIENT_DEPENDENCY,
        ),
        Disposition::Completed { .. } => (pb::ApprovalDeliveryAcknowledgement::Settled, ""),
        Disposition::FailedRetryable { failure_code } => {
            (pb::ApprovalDeliveryAcknowledgement::Retry, failure_code)
        }
    }
}

// ---------------------------------------------------------------------
// Async glue: one gRPC round trip per protocol step, wired together by the
// pure decisions above.
// ---------------------------------------------------------------------

fn authenticated_request<T>(value: T, bearer: &str) -> Result<tonic::Request<T>, String> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .map_err(|_| "approval delivery credential is not forwardable".to_owned())?,
    );
    Ok(request)
}

async fn get_continuation_descriptor(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    delivery: &pb::ApprovalDelivery,
) -> Result<Option<String>, String> {
    let request = authenticated_request(
        pb::GetApprovalContinuationRequest {
            org_id: org_id.to_owned(),
            delivery_id: delivery.delivery_id.clone(),
            approval_id: delivery.approval_id.clone(),
            lease_token: delivery.lease_token.clone(),
        },
        bearer,
    )?;
    let response = OrchestrationCoreServiceClient::new(channel.clone())
        .get_approval_continuation(request)
        .await
        .map_err(|error| format!("get_approval_continuation failed: {error}"))?
        .into_inner();
    if !response.available {
        return Ok(None);
    }
    Ok(Some(response.continuation_descriptor_json))
}

/// Returns `(receipt_id, already_started)`.
async fn record_continuation_started(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    delivery: &pb::ApprovalDelivery,
) -> Result<(String, bool), String> {
    let request = authenticated_request(
        pb::RecordApprovalContinuationStartedRequest {
            org_id: org_id.to_owned(),
            delivery_id: delivery.delivery_id.clone(),
            approval_id: delivery.approval_id.clone(),
            lease_token: delivery.lease_token.clone(),
        },
        bearer,
    )?;
    let response = OrchestrationCoreServiceClient::new(channel.clone())
        .record_approval_continuation_started(request)
        .await
        .map_err(|error| format!("record_approval_continuation_started failed: {error}"))?
        .into_inner();
    Ok((response.receipt_id, response.already_started))
}

/// Build this outcome's [`pb::VerificationResult`] — see the module doc's
/// "Verified Outcome Foundation" section for what `method: "structural"`
/// does and does not prove. `effect_id` is the same `receipt_id` this
/// worker's own `RecordApprovalContinuationStarted` call minted, so a
/// verification result is always traceable to the exact execution attempt
/// it judges.
fn verification_result_for(effect_id: &str, disposition: &Disposition) -> pb::VerificationResult {
    verification_result_with_postcondition(effect_id, disposition, None)
}

/// Build this outcome's verification, optionally strengthened by a real
/// postcondition check (`crate::postcondition`, roadmap P1 item 3).
///
/// The precedence rules are the point of this function:
///
/// - `Confirmed` upgrades the judgment to `method: "postcondition"` — the
///   system of record was independently asked and agreed.
/// - `Refuted` **overrides a structural success** into `VERIFIED_FAILURE`.
///   The boundary said yes and the system of record said no; reporting that
///   as success is exactly the false-success failure mode this layer exists
///   to catch.
/// - `Inconclusive` and `None` change nothing. An unreachable provider, an
///   action with no verifier, or an ambiguous record all leave the existing
///   structural judgment exactly as it was — a check that could not run must
///   never be credited as one that ran and passed.
fn verification_result_with_postcondition(
    effect_id: &str,
    disposition: &Disposition,
    postcondition: Option<&crate::postcondition::PostconditionOutcome>,
) -> pb::VerificationResult {
    use crate::postcondition::PostconditionOutcome;

    let now = Some(prost_types::Timestamp::from(std::time::SystemTime::now()));
    let (status, reason) = match disposition {
        Disposition::Completed {
            provider_receipt_id,
        } => (
            pb::VerificationStatus::VerifiedSuccess,
            format!("provider returned authoritative receipt id {provider_receipt_id}"),
        ),
        Disposition::FailedRetryable { failure_code } => (
            pb::VerificationStatus::VerifiedFailure,
            (*failure_code).to_owned(),
        ),
        Disposition::RejectBeforeStart { .. } | Disposition::AlreadyStarted => {
            (pb::VerificationStatus::Unknown, String::new())
        }
    };

    match postcondition {
        Some(PostconditionOutcome::Confirmed { detail }) => pb::VerificationResult {
            effect_id: effect_id.to_owned(),
            status: pb::VerificationStatus::VerifiedSuccess as i32,
            method: "postcondition".to_owned(),
            reason: detail.clone(),
            verified_at: now,
        },
        Some(PostconditionOutcome::Refuted { detail }) => pb::VerificationResult {
            effect_id: effect_id.to_owned(),
            status: pb::VerificationStatus::VerifiedFailure as i32,
            method: "postcondition".to_owned(),
            reason: detail.clone(),
            verified_at: now,
        },
        Some(PostconditionOutcome::Inconclusive { .. }) | None => pb::VerificationResult {
            effect_id: effect_id.to_owned(),
            status: status as i32,
            method: "structural".to_owned(),
            reason,
            verified_at: now,
        },
    }
}

async fn record_continuation_outcome(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    delivery: &pb::ApprovalDelivery,
    receipt_id: &str,
    disposition: &Disposition,
    postcondition: Option<&crate::postcondition::PostconditionOutcome>,
) -> Result<(), String> {
    let (outcome, provider_receipt_id, failure_code) = match disposition {
        Disposition::Completed {
            provider_receipt_id,
        } => (
            pb::ApprovalContinuationOutcome::Completed,
            provider_receipt_id.as_str(),
            "",
        ),
        Disposition::FailedRetryable { failure_code } => {
            (pb::ApprovalContinuationOutcome::Failed, "", *failure_code)
        }
        // RejectBeforeStart / AlreadyStarted never call this — see process_delivery.
        Disposition::RejectBeforeStart { .. } | Disposition::AlreadyStarted => {
            return Err(
                "record_continuation_outcome called for a pre-start disposition".to_owned(),
            );
        }
    };
    let request = authenticated_request(
        pb::RecordApprovalContinuationOutcomeRequest {
            org_id: org_id.to_owned(),
            delivery_id: delivery.delivery_id.clone(),
            approval_id: delivery.approval_id.clone(),
            receipt_id: receipt_id.to_owned(),
            lease_token: delivery.lease_token.clone(),
            outcome: outcome as i32,
            provider_receipt_id: provider_receipt_id.to_owned(),
            failure_code: failure_code.to_owned(),
            verification: Some(verification_result_with_postcondition(
                receipt_id,
                disposition,
                postcondition,
            )),
        },
        bearer,
    )?;
    OrchestrationCoreServiceClient::new(channel.clone())
        .record_approval_continuation_outcome(request)
        .await
        .map_err(|error| format!("record_approval_continuation_outcome failed: {error}"))?;
    Ok(())
}

async fn acknowledge(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    delivery: &pb::ApprovalDelivery,
    disposition: &Disposition,
) -> Result<(), String> {
    let (acknowledgement, failure_code) = acknowledgement_for(disposition);
    let continuation_receipt_id = match disposition {
        Disposition::Completed {
            provider_receipt_id,
        } => provider_receipt_id.clone(),
        _ => String::new(),
    };
    let request = authenticated_request(
        pb::AcknowledgeApprovalDeliveryRequest {
            org_id: org_id.to_owned(),
            delivery_id: delivery.delivery_id.clone(),
            lease_token: delivery.lease_token.clone(),
            acknowledgement: acknowledgement as i32,
            failure_code: failure_code.to_owned(),
            continuation_receipt_id,
        },
        bearer,
    )?;
    OrchestrationCoreServiceClient::new(channel.clone())
        .acknowledge_approval_delivery(request)
        .await
        .map_err(|error| format!("acknowledge_approval_delivery failed: {error}"))?;
    Ok(())
}

/// Process exactly one claimed delivery end to end. Every early return below
/// a business decision has already been finalized (acknowledged); every
/// early return above one is a pure infra failure that intentionally leaves
/// the lease to expire so a later poll reclaims it.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn process_delivery(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    integration_client: &IntegrationActionsClient,
    shipping_client: Option<&crate::shipping_tools::ShippingToolsClient>,
    delivery: pb::ApprovalDelivery,
) {
    let descriptor_json =
        match get_continuation_descriptor(channel, bearer, org_id, &delivery).await {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                let disposition = disposition_for_missing_descriptor();
                finalize_before_start(channel, bearer, org_id, &delivery, &disposition).await;
                return;
            }
            Err(error) => {
                warn!(
                    delivery_id = %delivery.delivery_id,
                    %error,
                    "approval_delivery_worker: continuation lookup failed, leaving lease to expire"
                );
                return;
            }
        };

    let parsed = parse_descriptor(&descriptor_json).and_then(|descriptor| {
        parse_resumable_action(&descriptor).map(|action| (descriptor, action))
    });
    let (descriptor, action) = match parsed {
        Ok(pair) => pair,
        Err(failure_code) => {
            let disposition = Disposition::RejectBeforeStart { failure_code };
            finalize_before_start(channel, bearer, org_id, &delivery, &disposition).await;
            return;
        }
    };

    let (receipt_id, already_started) =
        match record_continuation_started(channel, bearer, org_id, &delivery).await {
            Ok(pair) => pair,
            Err(error) => {
                warn!(
                    delivery_id = %delivery.delivery_id,
                    %error,
                    "approval_delivery_worker: record-started failed, leaving lease to expire"
                );
                return;
            }
        };
    if already_started {
        if let Err(error) = acknowledge(
            channel,
            bearer,
            org_id,
            &delivery,
            &Disposition::AlreadyStarted,
        )
        .await
        {
            warn!(delivery_id = %delivery.delivery_id, %error, "approval_delivery_worker: acknowledge (already-started) failed");
        }
        return;
    }

    // Captured before the match consumes `action`, so the postcondition step
    // below can still tell which verifier (if any) applies.
    let is_shipment_booking = matches!(action, ResumableAction::ShipmentBooking(_));
    let disposition = match action {
        ResumableAction::ProviderAction(input) => {
            let execution_result = integration_client
                .execute_action(
                    &descriptor.org_id,
                    &input.connection_id,
                    &input.operation,
                    input.params,
                    input.body,
                    &descriptor.user_id,
                    Some(&delivery.approval_id),
                )
                .await;
            disposition_for_provider_action_result(execution_result)
        }
        ResumableAction::ShipmentBooking(input) => {
            let Some(shipping_client) = shipping_client else {
                let disposition = Disposition::RejectBeforeStart {
                    failure_code: FAILURE_CONTINUATION_UNAVAILABLE,
                };
                finalize_before_start(channel, bearer, org_id, &delivery, &disposition).await;
                return;
            };
            // Mirrors execute_book_shipment's own idempotency_key derivation
            // (runtime_loop/mod.rs) so a resumed booking is recognized as the
            // same request shipping-core already saw, not a fresh one.
            let idempotency_key = format!("{}:{}", delivery.run_id, descriptor.step_id);
            let execution_result = shipping_client
                .book_shipment(
                    &input,
                    &descriptor.org_id,
                    &delivery.approval_id,
                    &idempotency_key,
                )
                .await;
            disposition_for_shipment_booking_result(execution_result)
        }
    };

    // Postcondition verification (roadmap P1 item 3). Only a completed
    // shipment booking has a verifier today; everything else stays on the
    // structural judgment. A failure to reach shipping-core is inconclusive,
    // never a refutation — see `crate::postcondition`'s module doc.
    let postcondition = match (&disposition, is_shipment_booking, shipping_client) {
        (
            Disposition::Completed {
                provider_receipt_id,
            },
            true,
            Some(client),
        ) => {
            let outcome = crate::postcondition::verify_shipment_booking(
                client,
                provider_receipt_id,
                &descriptor.org_id,
            )
            .await;
            info!(
                delivery_id = %delivery.delivery_id,
                booking_id = %provider_receipt_id,
                verdict = ?outcome,
                "approval_delivery_worker: postcondition check complete"
            );
            Some(outcome)
        }
        _ => None,
    };

    if let Err(error) = record_continuation_outcome(
        channel,
        bearer,
        org_id,
        &delivery,
        &receipt_id,
        &disposition,
        postcondition.as_ref(),
    )
    .await
    {
        warn!(
            delivery_id = %delivery.delivery_id,
            %error,
            "approval_delivery_worker: record-outcome failed, leaving lease to expire"
        );
        return;
    }
    if let Err(error) = acknowledge(channel, bearer, org_id, &delivery, &disposition).await {
        warn!(delivery_id = %delivery.delivery_id, %error, "approval_delivery_worker: acknowledge failed");
        return;
    }
    match &disposition {
        Disposition::Completed { .. } => {
            info!(delivery_id = %delivery.delivery_id, approval_id = %delivery.approval_id, "approval_delivery_worker: continuation completed");
        }
        _ => {
            warn!(delivery_id = %delivery.delivery_id, approval_id = %delivery.approval_id, "approval_delivery_worker: continuation failed, will retry");
        }
    }
}

async fn finalize_before_start(
    channel: &Channel,
    bearer: &str,
    org_id: &str,
    delivery: &pb::ApprovalDelivery,
    disposition: &Disposition,
) {
    if let Err(error) = acknowledge(channel, bearer, org_id, delivery, disposition).await {
        warn!(delivery_id = %delivery.delivery_id, %error, "approval_delivery_worker: acknowledge (pre-start) failed");
    }
}

/// Pure parse of the override's comma-separated shape — split out from
/// `org_ids_override` so the parsing rules (trim, drop blanks) are testable
/// without going through real process env at all.
fn parse_org_ids_override(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

/// An explicit override, for staged rollout or a deliberately narrowed pilot
/// — comma-separated, blank/whitespace entries dropped. When set, this wins
/// over auto-discovery entirely (see `resolve_org_ids`); when unset (the
/// normal case), every organization is discovered from org-core instead.
fn org_ids_override() -> Vec<String> {
    parse_org_ids_override(
        &std::env::var("EXECUTION_CORE_APPROVAL_DELIVERY_ORG_IDS").unwrap_or_default(),
    )
}

/// The org ids this poll pass services. `override_ids` (the caller's sole
/// production call site resolves it once per pass via `org_ids_override`,
/// keeping the one `std::env` read at that single boundary) always wins when
/// non-empty; otherwise every organization is auto-discovered from org-core
/// (`ClaimApprovalDeliveries` requires a credential scoped to the exact org
/// being claimed, so servicing "every org" means enumerating them first —
/// see `org_directory`). Neither configured → an empty, honest no-op rather
/// than a guess.
async fn resolve_org_ids(
    override_ids: Vec<String>,
    directory: Option<&crate::org_directory::OrgDirectoryClient>,
) -> Result<Vec<String>, String> {
    if !override_ids.is_empty() {
        return Ok(override_ids);
    }
    match directory {
        Some(directory) => directory.list_all_org_ids().await,
        None => Ok(Vec::new()),
    }
}

const DEFAULT_POLL_SECONDS: u64 = 15;
const MAX_DELIVERIES_PER_CLAIM: u32 = 5;

fn poll_interval() -> Duration {
    let seconds = std::env::var("EXECUTION_CORE_APPROVAL_DELIVERY_POLL_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_POLL_SECONDS);
    Duration::from_secs(seconds)
}

/// Detached, infallible spawn — mirrors `health_attest::spawn_heartbeat`'s
/// shape. Safe to always call from `main`: a missing credential or empty org
/// list makes the loop idle quietly rather than fail, so there is no
/// startup-ordering dependency on an operator having configured this yet.
pub fn spawn() {
    let session_url = std::env::var("SESSION_CORE_URL")
        .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
        .unwrap_or_else(|_| "http://localhost:9091".to_owned());
    let channel = match tonic::transport::Endpoint::from_shared(session_url) {
        Ok(endpoint) => endpoint.connect_lazy(),
        Err(error) => {
            warn!(%error, "approval_delivery_worker: SESSION_CORE_URL/SESSION_CORE_ADDR is malformed, not starting");
            return;
        }
    };
    tokio::spawn(run_forever(channel));
}

/// Run the poll loop forever. A no-op (idles quietly) until the delivery
/// worker's own credentials/signer are configured and at least one org is
/// either overridden explicitly or discoverable from org-core — safe to
/// always spawn from `main`, matching this binary's other
/// unconditionally-started background tasks.
async fn run_forever(channel: Channel) {
    let interval = poll_interval();
    let tokens = match ApprovalDeliveryTokenProvider::from_env() {
        Ok(provider) => Some(provider),
        Err(error) => {
            warn!(%error, "approval_delivery_worker: credentials not configured, idling");
            None
        }
    };
    let directory = crate::org_directory::OrgDirectoryClient::from_env();
    loop {
        if let Some(tokens) = tokens.as_ref() {
            match resolve_org_ids(org_ids_override(), directory.as_ref()).await {
                Ok(org_ids) if org_ids.is_empty() => {
                    warn!(
                        "approval_delivery_worker: no organizations to service (no explicit \
                         override and org-core auto-discovery is unconfigured or empty), idling"
                    );
                }
                Ok(org_ids) => {
                    if let Some(integration_client) = IntegrationActionsClient::from_env() {
                        let shipping_client =
                            crate::shipping_tools::ShippingToolsClient::from_env();
                        if shipping_client.is_none() {
                            warn!(
                                "approval_delivery_worker: shipping-core client unavailable, book_shipment continuations will be rejected as continuation_unavailable"
                            );
                        }
                        for org_id in org_ids {
                            poll_one_org(
                                &channel,
                                tokens,
                                &integration_client,
                                shipping_client.as_ref(),
                                &org_id,
                            )
                            .await;
                        }
                    } else {
                        warn!(
                            "approval_delivery_worker: integration-corev2 client unavailable, idling"
                        );
                    }
                }
                Err(error) => {
                    warn!(%error, "approval_delivery_worker: could not resolve the organization list, idling");
                }
            }
        }
        tokio::time::sleep(interval).await;
    }
}

async fn poll_one_org(
    channel: &Channel,
    tokens: &ApprovalDeliveryTokenProvider,
    integration_client: &IntegrationActionsClient,
    shipping_client: Option<&crate::shipping_tools::ShippingToolsClient>,
    org_id: &str,
) {
    let bearer = match tokens.token(org_id).await {
        Ok(bearer) => bearer,
        Err(error) => {
            warn!(org_id = %org_id, %error, "approval_delivery_worker: could not mint a delivery credential");
            return;
        }
    };
    let request = match authenticated_request(
        pb::ClaimApprovalDeliveriesRequest {
            org_id: org_id.to_owned(),
            max_deliveries: MAX_DELIVERIES_PER_CLAIM,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err(error) => {
            warn!(org_id = %org_id, %error, "approval_delivery_worker: could not build claim request");
            return;
        }
    };
    let deliveries = match OrchestrationCoreServiceClient::new(channel.clone())
        .claim_approval_deliveries(request)
        .await
    {
        Ok(response) => response.into_inner().deliveries,
        Err(error) => {
            warn!(org_id = %org_id, %error, "approval_delivery_worker: claim failed");
            return;
        }
    };
    for delivery in deliveries {
        process_delivery(
            channel,
            &bearer,
            org_id,
            integration_client,
            shipping_client,
            delivery,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_DESCRIPTOR: &str = r#"{
        "version": 1,
        "run_id": "run-1",
        "org_id": "org-1",
        "user_id": "user-1",
        "step_id": "step-1",
        "action_kind": "tool_call",
        "tool_name": "execute_provider_action",
        "input": {"connection_id": "conn-1", "operation": "linkedin.posts.create", "params": {}, "body": {"text": "hi"}},
        "permission_mode": "ask",
        "action_fingerprint": "abc123"
    }"#;

    const VALID_SHIPMENT_DESCRIPTOR: &str = r#"{
        "version": 1,
        "run_id": "run-1",
        "org_id": "org-1",
        "user_id": "user-1",
        "step_id": "step-1",
        "action_kind": "tool_call",
        "tool_name": "book_shipment",
        "input": {
            "quote_ref": "quote-1", "carrier_code": "bring", "service_name": "Standard",
            "price_amount_cents": 1000, "price_currency": "NOK",
            "from": {"name": "A", "postal_code": "0001", "city": "Oslo", "country": "NO"},
            "to": {"name": "B", "postal_code": "7010", "city": "Trondheim", "country": "NO"},
            "weight_kg": 1.0, "length_cm": 10.0, "width_cm": 10.0, "height_cm": 10.0,
            "booked_by": "user-1"
        },
        "permission_mode": "ask",
        "action_fingerprint": "def456"
    }"#;

    #[test]
    fn parses_a_valid_provider_action_descriptor() {
        let descriptor = parse_descriptor(VALID_DESCRIPTOR).expect("should parse");
        let action = parse_resumable_action(&descriptor).expect("should parse input");
        assert_eq!(descriptor.org_id, "org-1");
        assert_eq!(descriptor.user_id, "user-1");
        assert_eq!(descriptor.step_id, "step-1");
        let ResumableAction::ProviderAction(input) = action else {
            panic!("expected ProviderAction, got {action:?}");
        };
        assert_eq!(input.connection_id, "conn-1");
        assert_eq!(input.operation, "linkedin.posts.create");
    }

    #[test]
    fn parses_a_valid_shipment_booking_descriptor() {
        let descriptor = parse_descriptor(VALID_SHIPMENT_DESCRIPTOR).expect("should parse");
        let action = parse_resumable_action(&descriptor).expect("should parse input");
        let ResumableAction::ShipmentBooking(input) = action else {
            panic!("expected ShipmentBooking, got {action:?}");
        };
        assert_eq!(input.carrier_code, "bring");
        assert_eq!(input.booked_by, "user-1");
    }

    #[test]
    fn rejects_malformed_json() {
        assert_eq!(
            parse_descriptor("not json"),
            Err(FAILURE_INVALID_CONTINUATION)
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        let raw = VALID_DESCRIPTOR.replace("\"version\": 1", "\"version\": 2");
        assert_eq!(parse_descriptor(&raw), Err(FAILURE_INVALID_CONTINUATION));
    }

    #[test]
    fn rejects_a_non_tool_call_action_kind() {
        let raw = VALID_DESCRIPTOR.replace("\"tool_call\"", "\"other\"");
        assert_eq!(parse_descriptor(&raw), Err(FAILURE_INVALID_CONTINUATION));
    }

    #[test]
    fn rejects_an_unsupported_tool_name() {
        let raw = VALID_DESCRIPTOR.replace("execute_provider_action", "browser_agent");
        let descriptor = parse_descriptor(&raw).expect("should still parse as JSON");
        assert!(matches!(
            parse_resumable_action(&descriptor),
            Err(FAILURE_INVALID_CONTINUATION)
        ));
    }

    #[test]
    fn rejects_a_descriptor_missing_org_or_user() {
        let raw = VALID_DESCRIPTOR.replace("\"org_id\": \"org-1\",", "\"org_id\": \"\",");
        let descriptor = parse_descriptor(&raw).expect("should still parse as JSON");
        assert!(matches!(
            parse_resumable_action(&descriptor),
            Err(FAILURE_INVALID_CONTINUATION)
        ));
    }

    #[test]
    fn rejects_input_that_does_not_match_the_provider_action_shape() {
        let raw = VALID_DESCRIPTOR.replace(
            r#""input": {"connection_id": "conn-1", "operation": "linkedin.posts.create", "params": {}, "body": {"text": "hi"}}"#,
            r#""input": {"not_a_connection_id": true}"#,
        );
        let descriptor = parse_descriptor(&raw).expect("should still parse as JSON");
        assert!(matches!(
            parse_resumable_action(&descriptor),
            Err(FAILURE_INVALID_CONTINUATION)
        ));
    }

    #[test]
    fn rejects_input_that_does_not_match_the_shipment_booking_shape() {
        let raw = VALID_SHIPMENT_DESCRIPTOR.replace(
            r#""input": {
            "quote_ref": "quote-1", "carrier_code": "bring", "service_name": "Standard",
            "price_amount_cents": 1000, "price_currency": "NOK",
            "from": {"name": "A", "postal_code": "0001", "city": "Oslo", "country": "NO"},
            "to": {"name": "B", "postal_code": "7010", "city": "Trondheim", "country": "NO"},
            "weight_kg": 1.0, "length_cm": 10.0, "width_cm": 10.0, "height_cm": 10.0,
            "booked_by": "user-1"
        }"#,
            r#""input": {"not_a_booking": true}"#,
        );
        let descriptor = parse_descriptor(&raw).expect("should still parse as JSON");
        assert!(matches!(
            parse_resumable_action(&descriptor),
            Err(FAILURE_INVALID_CONTINUATION)
        ));
    }

    #[test]
    fn missing_descriptor_is_terminal_with_continuation_unavailable() {
        assert_eq!(
            disposition_for_missing_descriptor(),
            Disposition::RejectBeforeStart {
                failure_code: FAILURE_CONTINUATION_UNAVAILABLE
            }
        );
    }

    #[test]
    fn a_successful_provider_action_with_a_receipt_is_completed() {
        let outcome = crate::integration_tools::ActionOutcome {
            rendered: "Executed op. Result:\n{}".to_owned(),
            provider_receipt_id: Some("msg-123".to_owned()),
        };
        assert_eq!(
            disposition_for_provider_action_result(Ok(outcome)),
            Disposition::Completed {
                provider_receipt_id: "msg-123".to_owned()
            }
        );
    }

    #[test]
    fn a_successful_provider_action_without_a_receipt_is_terminal_not_retried() {
        let outcome = crate::integration_tools::ActionOutcome {
            rendered: "Executed op. Result:\n{}".to_owned(),
            provider_receipt_id: None,
        };
        assert_eq!(
            disposition_for_provider_action_result(Ok(outcome)),
            Disposition::RejectBeforeStart {
                failure_code: FAILURE_INVALID_CONTINUATION
            }
        );
    }

    #[test]
    fn a_failed_provider_action_is_retryable() {
        assert_eq!(
            disposition_for_provider_action_result(Err("network error".to_owned())),
            Disposition::FailedRetryable {
                failure_code: FAILURE_TRANSIENT_DEPENDENCY
            }
        );
    }

    #[test]
    fn a_successful_shipment_booking_is_completed_with_the_booking_id() {
        let outcome = crate::shipping_tools::BookingOutcome {
            rendered: "Shipment BOOKED with Bring.".to_owned(),
            booking_id: "booking-123".to_owned(),
        };
        assert_eq!(
            disposition_for_shipment_booking_result(Ok(outcome)),
            Disposition::Completed {
                provider_receipt_id: "booking-123".to_owned()
            }
        );
    }

    #[test]
    fn a_failed_shipment_booking_is_retryable() {
        assert_eq!(
            disposition_for_shipment_booking_result(Err("shipping-core unreachable".to_owned())),
            Disposition::FailedRetryable {
                failure_code: FAILURE_TRANSIENT_DEPENDENCY
            }
        );
    }

    #[test]
    fn a_completed_disposition_verifies_as_success_and_carries_the_effect_id() {
        let verification = verification_result_for(
            "receipt-123",
            &Disposition::Completed {
                provider_receipt_id: "booking-456".to_owned(),
            },
        );
        assert_eq!(verification.effect_id, "receipt-123");
        assert_eq!(
            verification.status,
            pb::VerificationStatus::VerifiedSuccess as i32
        );
        assert_eq!(verification.method, "structural");
        assert!(verification.reason.contains("booking-456"));
        assert!(verification.verified_at.is_some());
    }

    #[test]
    fn a_failed_retryable_disposition_verifies_as_failure_with_the_failure_code_as_reason() {
        let verification = verification_result_for(
            "receipt-789",
            &Disposition::FailedRetryable {
                failure_code: FAILURE_TRANSIENT_DEPENDENCY,
            },
        );
        assert_eq!(verification.effect_id, "receipt-789");
        assert_eq!(
            verification.status,
            pb::VerificationStatus::VerifiedFailure as i32
        );
        assert_eq!(verification.method, "structural");
        assert_eq!(verification.reason, FAILURE_TRANSIENT_DEPENDENCY);
    }

    #[test]
    fn a_pre_start_disposition_verifies_as_unknown() {
        // Defensive only: verification_result_for is never actually called for
        // these dispositions (record_continuation_outcome is not invoked for
        // them either — see process_delivery), but the mapping must still be
        // total and must never claim a verified status for a decision that
        // was never executed.
        for disposition in [
            Disposition::RejectBeforeStart {
                failure_code: FAILURE_INVALID_CONTINUATION,
            },
            Disposition::AlreadyStarted,
        ] {
            let verification = verification_result_for("receipt-000", &disposition);
            assert_eq!(verification.status, pb::VerificationStatus::Unknown as i32);
        }
    }

    #[test]
    fn acknowledgement_mapping_is_exhaustive_and_matches_session_core_rules() {
        // Terminal/Retry must carry a non-empty allowlisted code; Settled
        // must carry a receipt and no code — session-core rejects any other
        // combination (see approval_delivery::acknowledge_delivery).
        let (ack, code) = acknowledgement_for(&Disposition::RejectBeforeStart {
            failure_code: FAILURE_CONTINUATION_UNAVAILABLE,
        });
        assert_eq!(ack, pb::ApprovalDeliveryAcknowledgement::Terminal);
        assert_eq!(code, FAILURE_CONTINUATION_UNAVAILABLE);

        let (ack, code) = acknowledgement_for(&Disposition::AlreadyStarted);
        assert_eq!(ack, pb::ApprovalDeliveryAcknowledgement::Retry);
        assert!(!code.is_empty());

        let completed = Disposition::Completed {
            provider_receipt_id: "msg-1".to_owned(),
        };
        let (ack, code) = acknowledgement_for(&completed);
        assert_eq!(ack, pb::ApprovalDeliveryAcknowledgement::Settled);
        assert!(code.is_empty());

        let failed = Disposition::FailedRetryable {
            failure_code: FAILURE_TRANSIENT_DEPENDENCY,
        };
        let (ack, code) = acknowledgement_for(&failed);
        assert_eq!(ack, pb::ApprovalDeliveryAcknowledgement::Retry);
        assert!(!code.is_empty());
    }

    #[test]
    fn org_ids_override_splits_trims_and_drops_blanks() {
        // Exercises the pure parser directly — no real process env involved,
        // so this can't race any other test's env var mutation.
        assert_eq!(
            parse_org_ids_override(" org-1, org-2 ,,org-3"),
            vec!["org-1".to_owned(), "org-2".to_owned(), "org-3".to_owned()]
        );
    }

    #[tokio::test]
    async fn explicit_override_wins_over_discovery_without_calling_org_core() {
        // Passing None here is the actual assertion: if the override did not
        // win, resolve_org_ids would try to call a directory client and this
        // test would need one — it deliberately doesn't provide one.
        let ids = resolve_org_ids(vec!["org-pilot".to_owned()], None)
            .await
            .unwrap();
        assert_eq!(ids, vec!["org-pilot".to_owned()]);
    }

    #[tokio::test]
    async fn no_override_and_no_directory_is_an_honest_empty_list() {
        assert_eq!(
            resolve_org_ids(Vec::new(), None).await.unwrap(),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn token_provider_mints_the_fixed_approval_deliver_scope() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", "secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["approval:deliver"],
                "reason": "execution-core approval delivery"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "delivery-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        let provider =
            ApprovalDeliveryTokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");
        assert_eq!(provider.token("org-a").await.unwrap(), "delivery-token");
        // Cached: a second call must not mint again (mock expects exactly 1).
        assert_eq!(provider.token("org-a").await.unwrap(), "delivery-token");
    }

    // --- Postcondition precedence (roadmap P1 item 3) ------------------------

    #[test]
    fn a_confirmed_postcondition_upgrades_the_method_to_postcondition() {
        let disposition = Disposition::Completed {
            provider_receipt_id: "booking-456".to_owned(),
        };
        let outcome = crate::postcondition::PostconditionOutcome::Confirmed {
            detail: "shipping-core independently reports booking booking-456 as booked".to_owned(),
        };
        let verification =
            verification_result_with_postcondition("receipt-1", &disposition, Some(&outcome));

        assert_eq!(verification.method, "postcondition");
        assert_eq!(
            verification.status,
            pb::VerificationStatus::VerifiedSuccess as i32
        );
        assert!(verification.reason.contains("independently reports"));
    }

    #[test]
    fn a_refuted_postcondition_overrides_a_structural_success_into_failure() {
        // The whole reason this layer exists: the boundary handed back a
        // receipt id, so the structural judgment alone would have said
        // VERIFIED_SUCCESS.
        let disposition = Disposition::Completed {
            provider_receipt_id: "booking-456".to_owned(),
        };
        let structural = verification_result_with_postcondition("receipt-1", &disposition, None);
        assert_eq!(
            structural.status,
            pb::VerificationStatus::VerifiedSuccess as i32,
            "precondition: without a postcondition check this reads as success"
        );

        let outcome = crate::postcondition::PostconditionOutcome::Refuted {
            detail: "shipping-core has no booking booking-456 for this organization".to_owned(),
        };
        let verification =
            verification_result_with_postcondition("receipt-1", &disposition, Some(&outcome));

        assert_eq!(
            verification.status,
            pb::VerificationStatus::VerifiedFailure as i32,
            "a refutation must override the boundary's own success claim"
        );
        assert_eq!(verification.method, "postcondition");
    }

    #[test]
    fn an_inconclusive_postcondition_leaves_the_structural_judgment_untouched() {
        let disposition = Disposition::Completed {
            provider_receipt_id: "booking-456".to_owned(),
        };
        let baseline = verification_result_with_postcondition("receipt-1", &disposition, None);

        let outcome = crate::postcondition::PostconditionOutcome::Inconclusive {
            detail: "could not read booking booking-456 back: connection refused".to_owned(),
        };
        let verification =
            verification_result_with_postcondition("receipt-1", &disposition, Some(&outcome));

        assert_eq!(
            verification.method, "structural",
            "a check that could not run must never be credited as one that ran"
        );
        assert_eq!(verification.status, baseline.status);
        assert_eq!(verification.reason, baseline.reason);
    }

    #[test]
    fn an_unreachable_provider_never_manufactures_a_refutation() {
        // Same as above but stated as the invariant it protects: an outage
        // must not look like the effect failing to happen.
        let disposition = Disposition::Completed {
            provider_receipt_id: "booking-456".to_owned(),
        };
        let outcome = crate::postcondition::PostconditionOutcome::Inconclusive {
            detail: "shipping-core unreachable".to_owned(),
        };
        let verification =
            verification_result_with_postcondition("receipt-1", &disposition, Some(&outcome));

        assert_ne!(
            verification.status,
            pb::VerificationStatus::VerifiedFailure as i32
        );
    }

    #[test]
    fn verification_result_for_still_produces_the_structural_judgment() {
        // The no-postcondition path must be byte-identical to the old
        // behavior, so adding this layer changed nothing for actions that
        // have no verifier.
        let disposition = Disposition::Completed {
            provider_receipt_id: "booking-456".to_owned(),
        };
        let legacy = verification_result_for("receipt-1", &disposition);
        let explicit = verification_result_with_postcondition("receipt-1", &disposition, None);

        assert_eq!(legacy.method, explicit.method);
        assert_eq!(legacy.status, explicit.status);
        assert_eq!(legacy.reason, explicit.reason);
        assert_eq!(legacy.effect_id, explicit.effect_id);
    }
}
