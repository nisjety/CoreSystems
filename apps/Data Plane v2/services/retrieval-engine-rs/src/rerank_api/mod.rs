//! `POST /v1/rerank` (QB-T05a): rerank of caller-supplied candidates. Quarry option B.
//!
//! The Data Plane owns this capability. Quarry sends a query and up to 100 candidate texts
//! and gets them back ordered by a cross-encoder score, through the paid Azure Cohere
//! deployment the engine already uses. There is no Model Plane hop, no subscription route and
//! no sharding: one request, one provider call.
//!
//! Who may call: only a verified service principal named in `WEB_RERANK_ALLOWED_SERVICES`
//! that holds `data:rerank` (the route table in `authz::service_routes`). A user token is
//! refused before any lookup.
//!
//! What comes back: HTTP 200 for every outcome, `status` = `ok`, `skipped` or `degraded`.
//! A non-ok outcome answers in input order with `scored: false` and no scores, so the caller
//! always has a complete, identifiable list. Only validation (400), the body limit (413), the
//! admission refusals (401, 403) and our own rate limit (429) are errors.
//!
//! What is recorded: for each successful provider call, one usage-ledger event (schema_version 2,
//! `feature` = `web-rerank`, built by `usage-ledger-rs`), signed as the retrieval producer and
//! published to the durable `dataplane.cost.ledger` stream. Its count is the candidates scored; its
//! units are the provider's billed search units when it reports them. Otherwise the event is
//! estimated and carries no units (`CallUsage::rerank`). A skipped or degraded outcome is never
//! metered. The key is the ledger's `usage:web-rerank:{digest}`, where the digest covers the call's
//! deterministic key `rerank:{org}:{request_id}:{content digest}`, so a retry is one row. Also
//! recorded: one access-audit row per request carrying the outcome as its cause, and
//! `dpv2_web_rerank_*` metrics with bounded labels. Nothing persisted: no trace row, and no query
//! or document text in any log line, error body or span.
//!
//! Gates, all deployment switches with safe defaults: the route is off until
//! `WEB_RERANK_ENABLED` is set; the processor must be approved; the endpoint must be an https
//! Azure host (route-level, never a boot failure); a signed cost path must exist. The decision
//! order is in [`policy`].
//!
//! Rule for the caller: only `service:quarry-edge` may call this route, and it may send only
//! quarry-origin or public text. Vector hits are tenant chunks, so Quarry's vector leg is
//! confined to `documents.source = 'quarry'` before anything reaches a candidate list (the
//! corpus scope of QB-M1a). This route does not check the text it is given.
//!
//! Runbook (every switch: default, boot check, effect):
//!   * `WEB_RERANK_ENABLED` — `0|1|true|false`, default `false`. Off answers `skipped disabled`.
//!   * `WEB_RERANK_PROCESSOR_APPROVED` — `0|1|true|false`, default `false`. The processor gate.
//!   * `WEB_RERANK_ALLOWED_SERVICES` — comma list, default `service:quarry-edge`.
//!   * `WEB_RERANK_DAILY_UNIT_BUDGET` — integer, default `10000`, alert only; `0` disables it.
//!   * `USAGE_EMIT_ENABLED` — `0|1|true|false`, default `false`. The usage ledger. Off answers
//!     `skipped metering_unavailable` and never calls the provider, because a paid call that cannot be
//!     recorded is not made. On needs the `DATAPLANE_COST` stream to be creatable at boot: a failure logs
//!     one ERROR and the route stays `metering_unavailable` (the engine still starts). Turn it on only
//!     once the metering consumer is deployed and verified.
//!   * `DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS` / `..._BURST` — default `5` / `10` per org.
//!   * `RERANK_PROVIDER_MAX_INFLIGHT` — whole number `0..=1024`, default `0`. Provider calls in
//!     flight on this replica, shared by tenant retrieval and this route. The route may hold
//!     that minus `TENANT_RESERVED_PERMITS` (2); tenant retrieval may hold all of it. `0` leaves
//!     tenant retrieval unbounded and gives the route no capacity.
//!   * `RERANK_PROVIDER_RPS` — whole number `0..=10000`, default `0`. Provider requests per second
//!     on this replica. Tenant calls spend it and are never refused by it; the route is refused
//!     while it is spent. `0` gives the route no capacity.
//!   * `RERANK_PROVIDER_TENANT_WAIT_MS` — `0..=30000`, default `1000`. How long a tenant call waits
//!     for a permit once the pool is saturated; past it the call degrades to the fused order.
//!   * `RERANK_BREAKER_FAILURES` / `RERANK_BREAKER_WINDOW_SECS` / `RERANK_BREAKER_OPEN_SECS` —
//!     defaults `5` / `30` / `30`. Failed provider calls inside the window open the breaker, and
//!     it admits one probe after the open period. Open, it skips the route (`skipped circuit_open`)
//!     with no provider call and never refuses a tenant call.
//!   * Boot for the provider limits: a malformed value logs one ERROR and every provider limit
//!     reads as its default (tenant unbounded, route without capacity). With `WEB_RERANK_ENABLED`
//!     set, the route needs `RERANK_PROVIDER_MAX_INFLIGHT` above 2 and `RERANK_PROVIDER_RPS` above
//!     0, or it answers `skipped misconfigured` (`NoProviderCapacity`). Every limit is per replica:
//!     a second replica doubles what the provider sees.
//!   * Boot: a switch that is not `0|1|true|false` reads as off and logs one ERROR. An endpoint
//!     that fails the Azure rule, or a missing `COHERE_API_KEY`, logs one ERROR and the route
//!     answers `skipped misconfigured`. The engine still starts and tenant retrieval is untouched.
//!   * A signed `sovereign = true` claim floors `sovereign_required`, as it does for retrieval.

pub mod cache;
pub mod limits;
pub mod policy;
pub mod schema;

use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{Extension, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::limit::RequestBodyLimitLayer;
use usage_ledger_rs::{CallUsage, Feature, LedgerSink, UsageBuilder};

use crate::audit::AuditCause;
use crate::authz::{guard_middleware, AuthContext, AuthGuard, Principal};
use crate::rate_limit::PerOrgLimiter;
use crate::search::rerank::{RerankClient, RerankError};

pub use cache::RerankCache;
pub use policy::{PolicyInput, SkipReason, Verdict};
pub use schema::{RerankInput, RerankResponse, ResultEntry};

/// The one profile the route serves. The provider model is resolved server-side from it.
pub const PROFILE_WEB_V1: &str = "web-v1";
pub const RERANK_PATH: &str = "/v1/rerank";
/// The provider a web rerank call is billed by and the usage events name: the Azure AI Foundry
/// Cohere deployment, the same provider name the embedding path uses for Cohere on Azure.
pub const PROVIDER_USED: &str = "azure-cohere";
/// How long an answer waits for the usage ledger to acknowledge its call before the answer is sent
/// (QB-M). The wait is also capped by the caller's deadline. A publish still in flight when the wait
/// ends is not abandoned: it finishes in the background, and a failure is counted there. So a slow
/// broker never holds an answer past the caller's deadline, and never loses a paid call.
pub const METERING_WAIT: Duration = Duration::from_millis(250);

/// How long an answer waits for its usage event, given what is left of the caller's deadline: at
/// most [`METERING_WAIT`], and never past the deadline.
#[must_use]
pub fn publish_wait(remaining: Duration) -> Duration {
    METERING_WAIT.min(remaining)
}

const FORBIDDEN_CALLER: &str = "service_principal_route_not_allowed";

/// Why the endpoint rule refused the configured provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointProblem {
    Empty,
    Unparseable,
    NotHttps,
    Credentials,
    NotAzureHost,
    /// The endpoint is fine but no provider key is configured.
    NoApiKey,
    /// The endpoint and key are fine, but the provider gate has no web capacity: the permit pool
    /// must be above the tenant reserve and the request budget above zero (QB-T05b).
    NoProviderCapacity,
}

/// The endpoint rule: https, no embedded credentials, and an Azure host (`azure.com` or a
/// subdomain of it, label-aligned). The public Cohere default is refused here on purpose.
pub fn validate_endpoint(endpoint: &str) -> Result<(), EndpointProblem> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err(EndpointProblem::Empty);
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| EndpointProblem::Unparseable)?;
    if url.scheme() != "https" {
        return Err(EndpointProblem::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(EndpointProblem::Credentials);
    }
    let host = url
        .host_str()
        .ok_or(EndpointProblem::Unparseable)?
        .to_ascii_lowercase();
    if host == "azure.com" || host.ends_with(".azure.com") {
        Ok(())
    } else {
        Err(EndpointProblem::NotAzureHost)
    }
}

/// Whether the route can call a provider at all. Decided once at startup, per route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAvailability {
    Available,
    Misconfigured(EndpointProblem),
}

impl RouteAvailability {
    #[must_use]
    pub fn from_endpoint(endpoint: &str) -> Self {
        match validate_endpoint(endpoint) {
            Ok(()) => Self::Available,
            Err(problem) => Self::Misconfigured(problem),
        }
    }

    /// The endpoint rule and the presence of a provider key, together.
    #[must_use]
    pub fn for_route(endpoint: &str, has_api_key: bool) -> Self {
        if !has_api_key {
            return Self::Misconfigured(EndpointProblem::NoApiKey);
        }
        Self::from_endpoint(endpoint)
    }
}

/// The switches the handler reads, from `Config` at startup.
#[derive(Debug, Clone)]
pub struct RerankSettings {
    pub enabled: bool,
    pub processor_approved: bool,
    /// Service ids allowed to call the route, beyond the static route table.
    pub allowed_services: Vec<String>,
    /// Alert threshold for billed units per org per UTC day. Alert-only; `0` disables it.
    pub daily_unit_budget: u64,
}

/// Admission rate gate for the route. Production: [`PerOrgLimiter`], keyed by org.
#[async_trait]
pub trait RateGate: Send + Sync {
    async fn allow(&self, org_id: &str) -> bool;
}

#[async_trait]
impl RateGate for PerOrgLimiter {
    async fn allow(&self, org_id: &str) -> bool {
        self.check(org_id).await
    }
}

/// Counts billed units per org per UTC day. Returns the day's total after the add, or `None`
/// when the counter is unavailable (not counted, never a failed call).
#[async_trait]
pub trait UnitBudget: Send + Sync {
    async fn add_units(&self, org_id: &str, day: &str, units: u64) -> Option<u64>;
}

/// Whether the provider is attested for ZDR content. The real answer comes from the shared
/// egress-attestation module (Z-41). Until it is wired, nothing is attested.
pub trait EgressAttestation: Send + Sync {
    fn permits_rerank(&self) -> bool {
        false
    }
}

/// The attestation seam with its default answer. The engine itself wires the real
/// `egress_attestation::EgressAttestation`; this stays for the contract tests (as `ClosedCircuit`).
#[allow(dead_code)]
pub struct NoAttestation;

impl EgressAttestation for NoAttestation {}

/// Whether the provider circuit is open. The breaker itself is QB-T05b; until then, the
/// default is closed.
pub trait CircuitGate: Send + Sync {
    fn is_open(&self) -> bool {
        false
    }
}

/// The circuit seam with its default answer. The engine wires the provider gate instead, which
/// answers from the real breaker; this closed answer stays for the contract tests.
#[allow(dead_code)]
pub struct ClosedCircuit;

impl CircuitGate for ClosedCircuit {}

/// Records the successful web rerank calls in the usage ledger (QB-M). The ledger is
/// `usage-ledger-rs`: it builds the schema_version 2 event, derives its key, signs it with the
/// producer's key and publishes it to the durable stream. This type only says what the call was.
pub struct CostLedger {
    sink: Arc<dyn LedgerSink>,
}

impl CostLedger {
    #[must_use]
    pub fn new(sink: Arc<dyn LedgerSink>) -> Self {
        Self { sink }
    }

    /// Records one successful provider call and waits for the broker to acknowledge it.
    ///
    /// * `documents`: the candidates the provider scored. This is the event's count.
    /// * `search_units`: the provider's billed search units, when it reported them. When it did
    ///   not, the event is estimated by its candidates and carries no units.
    /// * `call_key`: the call's deterministic key, `rerank:{org}:{request_id}:{content digest}`.
    ///   The ledger's idempotency key is derived from it, so a retry of the same call is one row,
    ///   and a reused request id with different content is a second row.
    /// * `zdr`: the effective posture of the call, which the event records as a flag only.
    pub async fn record_rerank(
        &self,
        org_id: &str,
        model: &str,
        zdr: bool,
        documents: u64,
        search_units: Option<u64>,
        call_key: &str,
    ) -> Result<(), String> {
        let mut builder = UsageBuilder::new(Feature::WebRerank, org_id, zdr)
            .map_err(|error| error.to_string())?;
        builder.push(CallUsage::rerank(
            call_key,
            model,
            PROVIDER_USED,
            documents,
            search_units,
        ));
        for event in builder.build() {
            self.sink
                .publish(&event)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// Everything the route needs, injected so the crate's tests run the same router with
/// doubles only where the rule is not the thing under test.
pub struct RerankState {
    pub settings: RerankSettings,
    pub availability: RouteAvailability,
    /// Present only when `availability` is `Available`.
    pub provider: Option<RerankClient>,
    /// The provider model the profile resolves to.
    pub model: String,
    pub guard: AuthGuard,
    pub limiter: Arc<dyn RateGate>,
    /// `None` when metering cannot be signed and published: the route then never calls a
    /// paid provider (`metering_unavailable`).
    pub cost: Option<Arc<CostLedger>>,
    pub budget: Arc<dyn UnitBudget>,
    pub attestation: Arc<dyn EgressAttestation>,
    pub circuit: Arc<dyn CircuitGate>,
    pub cache: RerankCache,
}

/// The route, as a sub-router with its own body limit, admission and rate gate. Merged into
/// the engine's router by `api`; the tests mount it directly.
pub fn router<S>(state: RerankState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let guard = state.guard.clone();
    let gate = state.limiter.clone();
    Router::new()
        .route(RERANK_PATH, post(rerank_handler))
        .layer(RequestBodyLimitLayer::new(limits::MAX_BODY_BYTES))
        .route_layer(middleware::from_fn_with_state(gate, rate_gate))
        .route_layer(middleware::from_fn_with_state(guard, guard_middleware))
        .with_state(Arc::new(state))
}

/// Per-org admission rate. Runs after the guard, so the key is the verified org.
async fn rate_gate(State(gate): State<Arc<dyn RateGate>>, req: Request, next: Next) -> Response {
    let org = req
        .extensions()
        .get::<AuthContext>()
        .map(|context| context.org_id.clone())
        .filter(|org| !org.is_empty())
        .unwrap_or_else(|| "anonymous".to_owned());
    if gate.allow(&org).await {
        return next.run(req).await;
    }
    crate::metrics::record_rate_limit_denied(&org);
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "error": "rate_limited",
            "message": "per-org rate limit exceeded",
            "retry_after_seconds": 1,
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("Retry-After", HeaderValue::from_static("1"));
    response
}

/// Maps provider `(index, score)` pairs onto the request: an index outside the request or a
/// repeat is ignored, a non-finite score becomes `0.0`.
#[must_use]
pub fn map_provider_scores(document_count: usize, results: &[(usize, f32)]) -> Vec<(usize, f32)> {
    let mut seen = vec![false; document_count];
    let mut mapped = Vec::with_capacity(results.len().min(document_count));
    for &(index, score) in results {
        if index >= document_count || seen[index] {
            continue;
        }
        seen[index] = true;
        mapped.push((index, if score.is_finite() { score } else { 0.0 }));
    }
    mapped
}

/// Best score first; equal scores go to the lower input index. Then cut to `top_n`.
#[must_use]
pub fn rank(mut scores: Vec<(usize, f32)>, top_n: usize) -> Vec<(usize, f32)> {
    scores.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scores.truncate(top_n);
    scores
}

fn zdr_actions(effective_zdr: bool, attested: bool) -> Vec<&'static str> {
    if effective_zdr && !attested {
        schema::ZDR_ACTIONS.to_vec()
    } else {
        Vec::new()
    }
}

fn skip_cause(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::Disabled => "rerank:skipped_disabled",
        SkipReason::Misconfigured => "rerank:skipped_misconfigured",
        SkipReason::ProcessorUnapproved => "rerank:skipped_processor_unapproved",
        SkipReason::MeteringUnavailable => "rerank:skipped_metering_unavailable",
        SkipReason::Zdr => "rerank:skipped_zdr",
        SkipReason::Sovereign => "rerank:skipped_sovereign",
        SkipReason::CircuitOpen => "rerank:skipped_circuit_open",
    }
}

fn degraded_cause(reason: &str) -> &'static str {
    match reason {
        "timeout" => "rerank:degraded_timeout",
        "provider_rate_limited" => "rerank:degraded_provider_rate_limited",
        _ => "rerank:degraded_provider_error",
    }
}

/// A body with the outcome, the metrics for it, and the audit cause for its access row.
fn respond(
    body: RerankResponse,
    metric_reason: &'static str,
    cause: &'static str,
    started: Instant,
    documents: usize,
) -> Response {
    crate::metrics::record_web_rerank(body.status, metric_reason);
    crate::metrics::record_web_rerank_duration(started.elapsed().as_secs_f64());
    crate::metrics::record_web_rerank_documents(documents as u64);
    let mut response = (StatusCode::OK, Json(body)).into_response();
    response.extensions_mut().insert(AuditCause(cause));
    response
}

fn error_body(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn non_ok(
    input: &RerankInput,
    status: &'static str,
    reason: &'static str,
    zdr: Vec<&'static str>,
) -> RerankResponse {
    RerankResponse {
        request_id: input.request_id.clone(),
        status,
        reason: Some(reason),
        profile: PROFILE_WEB_V1,
        zdr_actions_applied: zdr,
        truncated_ids: input.truncated_ids.clone(),
        results: schema::identity_results(&input.documents),
    }
}

async fn rerank_handler(
    State(state): State<Arc<RerankState>>,
    Extension(auth): Extension<AuthContext>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let org = auth.org_id.clone();

    let service_id = match &auth.principal {
        Principal::Service { service_id } => service_id.clone(),
        Principal::User => {
            return route_refused(
                StatusCode::FORBIDDEN,
                "service_only_route",
                "denied:service_only_route",
            );
        }
    };
    if !state
        .settings
        .allowed_services
        .iter()
        .any(|allowed| allowed == &service_id)
    {
        return route_refused(
            StatusCode::FORBIDDEN,
            FORBIDDEN_CALLER,
            "denied:rerank_caller_not_allowed",
        );
    }

    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error_body(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let input = match schema::parse_request(&value) {
        Ok(input) => input,
        Err(code) => return error_body(StatusCode::BAD_REQUEST, code),
    };

    let effective_zdr = auth.zdr || input.zdr_mode.restricts_egress();
    let attested = state.attestation.permits_rerank();
    let zdr = zdr_actions(effective_zdr, attested);
    // The signed sovereignty claim floors the body's flag, as it does for retrieval: a body
    // cannot relax a `sovereign = true` token (`AuthContext::effective_sovereign_required`).
    let sovereign_required = auth
        .effective_sovereign_required(Some(input.sovereign_required))
        .unwrap_or(true);
    let decision = policy::decide(&PolicyInput {
        enabled: state.settings.enabled,
        endpoint_ok: state.availability == RouteAvailability::Available && state.provider.is_some(),
        processor_approved: state.settings.processor_approved,
        metering_available: state.cost.is_some(),
        token_zdr: auth.zdr,
        body_zdr_restricts: input.zdr_mode.restricts_egress(),
        attested_for_zdr: attested,
        sovereign_required,
        circuit_open: state.circuit.is_open(),
    });
    if let Verdict::Skip(reason) = decision {
        let response = non_ok(&input, "skipped", reason.as_str(), zdr);
        return respond(
            response,
            reason.as_str(),
            skip_cause(reason),
            started,
            input.documents.len(),
        );
    }
    let (Some(provider), Some(cost)) = (state.provider.as_ref(), state.cost.as_ref()) else {
        let response = non_ok(&input, "skipped", SkipReason::Misconfigured.as_str(), zdr);
        return respond(
            response,
            SkipReason::Misconfigured.as_str(),
            skip_cause(SkipReason::Misconfigured),
            started,
            input.documents.len(),
        );
    };

    let digest = schema::content_digest(&input);
    let call_key = limits::idempotency_key(&org, &input.request_id, &digest);
    if let Some(ranked) = state.cache.get(&call_key).await {
        let results = schema::scored_results(&input.documents, &ranked);
        let body = ok_body(&input, zdr, results);
        return respond(body, "replay", "rerank:ok", started, input.documents.len());
    }

    let texts: Vec<String> = input.documents.iter().map(|d| d.text.clone()).collect();
    let remaining = input.deadline.saturating_sub(started.elapsed());
    match provider
        .rerank_texts(&input.query, &texts, input.top_n, remaining)
        .await
    {
        Ok(provider_scores) => {
            let mapped = map_provider_scores(input.documents.len(), &provider_scores.scores);
            if mapped.is_empty() {
                return degraded(&input, zdr, "provider_error", started);
            }
            let ranked = rank(mapped, input.top_n);
            // Metered once, for the call the provider was paid for: the candidates it scored, and
            // the search units it billed when it reported them (the event is estimated otherwise).
            // The publish runs in its own task, so the answer waits for it at most METERING_WAIT and
            // never past the caller's deadline; a publish still in flight finishes in the background.
            let documents = input.documents.len() as u64;
            let search_units = provider_scores.search_units;
            let wait = publish_wait(input.deadline.saturating_sub(started.elapsed()));
            let recording = tokio::spawn({
                let metering = Arc::clone(cost);
                let (org, model, call_key) = (org.clone(), state.model.clone(), call_key.clone());
                async move {
                    if let Err(error) = metering
                        .record_rerank(
                            &org,
                            &model,
                            effective_zdr,
                            documents,
                            search_units,
                            &call_key,
                        )
                        .await
                    {
                        crate::metrics::record_web_rerank_cost_failure();
                        tracing::error!(org_id = %org, error = %error, "web rerank cost event was not published");
                    }
                }
            });
            if let Ok(Err(join_error)) = tokio::time::timeout(wait, recording).await {
                // The publish task panicked before it could record the call: counted, never silent.
                crate::metrics::record_web_rerank_cost_failure();
                tracing::error!(org_id = %org, error = %join_error, "web rerank cost publish task failed");
            }
            state.cache.put(call_key, ranked.clone()).await;
            if let Some(units) = search_units {
                crate::metrics::record_web_rerank_search_units(units);
            }
            // The alert counts the quantity the ledger records: the reported units, else the candidates.
            // It runs in its own task, off the answer's path: the unit budget is a shared counter with its
            // own timeout, and it must never hold the answer past the caller's deadline (QB-M).
            let counted = search_units.unwrap_or(documents);
            let counting = Arc::clone(&state);
            let counting_org = org.clone();
            tokio::spawn(async move { count_units(&counting, &counting_org, counted).await });
            let results = schema::scored_results(&input.documents, &ranked);
            respond(
                ok_body(&input, zdr, results),
                "none",
                "rerank:ok",
                started,
                input.documents.len(),
            )
        }
        // The breaker closed the door between the policy check and the call (a probe is out).
        // The provider was not called, so this is a skip, not a degraded provider answer.
        Err(RerankError::CircuitOpen) => {
            let reason = SkipReason::CircuitOpen;
            respond(
                non_ok(&input, "skipped", reason.as_str(), zdr),
                reason.as_str(),
                skip_cause(reason),
                started,
                input.documents.len(),
            )
        }
        Err(error) => {
            let reason = match error {
                RerankError::Timeout => "timeout",
                RerankError::RateLimited => "provider_rate_limited",
                RerankError::Provider(_)
                | RerankError::Transport
                | RerankError::Malformed
                | RerankError::CircuitOpen => "provider_error",
            };
            degraded(&input, zdr, reason, started)
        }
    }
}

fn ok_body(
    input: &RerankInput,
    zdr: Vec<&'static str>,
    results: Vec<ResultEntry>,
) -> RerankResponse {
    RerankResponse {
        request_id: input.request_id.clone(),
        status: "ok",
        reason: None,
        profile: PROFILE_WEB_V1,
        zdr_actions_applied: zdr,
        truncated_ids: input.truncated_ids.clone(),
        results,
    }
}

fn degraded(
    input: &RerankInput,
    zdr: Vec<&'static str>,
    reason: &'static str,
    started: Instant,
) -> Response {
    let body = non_ok(input, "degraded", reason, zdr);
    respond(
        body,
        reason,
        degraded_cause(reason),
        started,
        input.documents.len(),
    )
}

/// A refusal of the caller, not of the request: the status is 403 and the cause is recorded.
fn route_refused(status: StatusCode, code: &'static str, cause: &'static str) -> Response {
    let mut response = error_body(status, code);
    response.extensions_mut().insert(AuditCause(cause));
    response
}

async fn count_units(state: &RerankState, org: &str, units: u64) {
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let Some(total) = state.budget.add_units(org, &day, units).await else {
        return;
    };
    let budget = state.settings.daily_unit_budget;
    if budget > 0 && total > budget {
        crate::metrics::record_web_rerank_budget_exceeded(org);
        tracing::warn!(
            org_id = %org,
            total_units = total,
            budget,
            "web rerank daily unit budget exceeded (alert only, calls continue)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_rule_is_label_aligned_and_https_only() {
        assert_eq!(
            validate_endpoint("https://a.ai.azure.com/v2/rerank"),
            Ok(())
        );
        assert_eq!(validate_endpoint("https://AZURE.COM/x"), Ok(()));
        assert_eq!(
            validate_endpoint("https://x.azure.com.evil.example/"),
            Err(EndpointProblem::NotAzureHost)
        );
    }

    #[test]
    fn a_route_without_a_key_is_misconfigured_whatever_the_endpoint() {
        assert_eq!(
            RouteAvailability::for_route("https://a.azure.com/v2/rerank", false),
            RouteAvailability::Misconfigured(EndpointProblem::NoApiKey)
        );
        assert_eq!(
            RouteAvailability::for_route("https://a.azure.com/v2/rerank", true),
            RouteAvailability::Available
        );
    }

    #[test]
    fn provider_scores_are_mapped_by_index_once_and_finite() {
        assert_eq!(
            map_provider_scores(2, &[(1, 0.5), (1, 0.9), (0, f32::NEG_INFINITY)]),
            vec![(1, 0.5), (0, 0.0)]
        );
    }

    #[test]
    fn ranking_is_best_first_with_ties_to_the_lower_index_then_cut() {
        assert_eq!(
            rank(vec![(3, 0.1), (1, 0.9), (0, 0.9), (2, 0.5)], 3),
            vec![(0, 0.9), (1, 0.9), (2, 0.5)]
        );
    }

    /// The web route's event (QB-M): the candidates are its count, the reported units its billed
    /// quantity, its key the ledger's key over the call key, and it carries no tokens.
    #[tokio::test]
    async fn the_cost_ledger_records_the_web_rerank_feature_with_its_call_key_and_units() {
        let sink = Arc::new(usage_ledger_rs::MemoryLedger::new());
        let ledger = CostLedger::new(sink.clone());
        ledger
            .record_rerank(
                "org-1",
                "Cohere-rerank-v4.0-pro",
                false,
                10,
                Some(3),
                "rerank:org-1:rr-ledger-0001:abc",
            )
            .await
            .expect("recorded");
        let events = sink.events();
        assert_eq!(events.len(), 1, "one call, one event");
        let event = &events[0];
        assert_eq!(event.feature, "web-rerank");
        assert_eq!(event.event_type, "rerank");
        assert_eq!(event.provider_used, PROVIDER_USED);
        assert_eq!((event.count, event.units), (10, Some(3)));
        assert_eq!((event.cost_basis, event.estimated), ("metered", false));
        assert_eq!(
            (
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens
            ),
            (0, 0, 0)
        );
        assert_eq!(
            event.idempotency_key,
            format!("usage:web-rerank:{}", event.request_digest)
        );
        assert!(!event.zdr);
    }

    #[tokio::test]
    async fn a_refused_publish_is_reported_to_the_caller_and_records_nothing() {
        let sink = Arc::new(usage_ledger_rs::MemoryLedger::new());
        sink.refuse_next(1);
        let ledger = CostLedger::new(sink.clone());
        assert!(ledger
            .record_rerank(
                "org-1",
                "m",
                false,
                2,
                None,
                "rerank:org-1:rr-ledger-0002:abc"
            )
            .await
            .is_err());
        assert!(sink.events().is_empty());
    }

    /// The answer waits for its usage event at most METERING_WAIT, and never past the caller's deadline.
    #[test]
    fn the_publish_wait_is_capped_by_what_the_deadline_leaves() {
        assert_eq!(publish_wait(Duration::from_secs(2)), METERING_WAIT);
        assert_eq!(
            publish_wait(Duration::from_millis(100)),
            Duration::from_millis(100),
            "the deadline leaves less than the wait"
        );
        assert_eq!(publish_wait(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn the_audit_cause_names_the_skip_and_degrade_reasons() {
        assert_eq!(skip_cause(SkipReason::Zdr), "rerank:skipped_zdr");
        assert_eq!(degraded_cause("timeout"), "rerank:degraded_timeout");
        assert_eq!(
            degraded_cause("provider_error"),
            "rerank:degraded_provider_error"
        );
    }
}
