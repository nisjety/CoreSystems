use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::pipeline::types::ScoredCandidate;

const MAX_RETRIES: u32 = 2;
/// First backoff step; doubles per attempt (1s, then 2s).
///
/// Was 300ms, which is why the reranker was effectively dead under load: the
/// Azure Foundry S0 tier answers 429 with `Retry-After: 1` and a per-minute
/// window, so retries at +300ms/+600ms all landed inside the same window and
/// exhausted. Because rerank failure deliberately degrades to fused order, the
/// only visible symptom was every candidate carrying `rerank_score: 0.0` —
/// measured live at a 23% failure rate on light probe traffic, and worse in
/// bursts. Worst-case added latency is 1s+2s=3s on a rate-limited query, which
/// is the price of the cross-encoder actually running; a chronically limited
/// deployment still degrades non-fatally exactly as before.
const INITIAL_BACKOFF_MS: u64 = 1_000;
/// Upper bound on a provider-supplied `Retry-After`, so a hostile or confused
/// upstream cannot pin a retrieval request for a minute.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

/// Provider-directed backoff: prefer the 429's own `Retry-After` (seconds
/// form) over the local schedule, since the provider knows its window and the
/// local guess is what made retries useless before. Capped, and only ever
/// LENGTHENS the local backoff — a `Retry-After: 0` must not turn the retry
/// into a same-window hammer.
fn backoff_for(attempt: u32, retry_after: Option<Duration>) -> Duration {
    let local = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt.saturating_sub(1)));
    match retry_after {
        Some(hinted) => hinted.min(MAX_RETRY_AFTER).max(local),
        None => local,
    }
}

fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[derive(Clone)]
pub struct RerankClient {
    http: Client,
    api_key: String,
    model: String,
    /// Full rerank URL. Defaults to public Cohere; set to an Azure AI Foundry
    /// serverless Cohere rerank endpoint (`https://<deployment>.<region>.models.ai.azure.com/v2/rerank`)
    /// to use the in-EU deployment instead of the public API.
    endpoint: String,
    /// Auth header style: `true` → `Authorization: Bearer <key>` (public Cohere);
    /// `false` → `api-key: <key>` (Azure Foundry).
    use_bearer: bool,
    /// The replica's provider gate (QB-T05b). `None` only for a client built without one.
    gate: Option<Arc<ProviderGate>>,
    /// Whether the pipeline's rerank sends each chunk with its context in force (CR-T15, the
    /// `RERANK_INPUT_INCLUDE_CONTEXT` switch). Off unless the switch is on. The `POST /v1/rerank` route
    /// never sets it: its callers send their own documents, which carry no context.
    include_chunk_context: bool,
}

#[cfg(test)]
mod security_tests {
    use super::sanitized_rerank_status_error;

    #[test]
    fn provider_error_does_not_include_response_body() {
        let error = sanitized_rerank_status_error(reqwest::StatusCode::BAD_GATEWAY).to_string();
        assert_eq!(error, "rerank API returned HTTP status 502");
        assert!(!error.contains("document"));
    }
}

#[cfg(test)]
mod backoff_tests {
    use super::{backoff_for, MAX_RETRY_AFTER};
    use std::time::Duration;

    // Regression: the local schedule must clear the provider's advertised
    // window. Azure Foundry S0 answers 429 with `Retry-After: 1`; the old
    // 300ms/600ms schedule retried inside the same window and exhausted, which
    // — because rerank failure is non-fatal by design — silently disabled the
    // cross-encoder (23% of probe traffic degraded to fused order).
    #[test]
    fn local_schedule_clears_a_one_second_rate_window() {
        assert!(backoff_for(1, None) >= Duration::from_secs(1));
        assert_eq!(backoff_for(2, None), Duration::from_secs(2));
    }

    #[test]
    fn provider_retry_after_lengthens_but_never_shortens() {
        // The provider knows its window better than the local guess does.
        assert_eq!(
            backoff_for(1, Some(Duration::from_secs(3))),
            Duration::from_secs(3)
        );
        // A `Retry-After: 0` must not turn the retry into a same-window hammer.
        assert_eq!(backoff_for(1, Some(Duration::ZERO)), Duration::from_secs(1));
    }

    #[test]
    fn provider_retry_after_is_capped() {
        // A hostile or confused upstream cannot pin a retrieval request.
        assert_eq!(
            backoff_for(1, Some(Duration::from_secs(3600))),
            MAX_RETRY_AFTER
        );
    }
}

#[derive(Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: usize,
}

/// The most context, in characters, a rerank document carries ahead of its chunk (CR-T15). It is the
/// bound the generator enforces on a context it stores (`MAX_CHARS` of the embedding engine's
/// validator), so a context the generator wrote is never cut here.
pub const RERANK_CONTEXT_MAX_CHARS: usize = 600;

/// The text one candidate contributes to a rerank request (CR-T15).
///
/// With `include_context` off, or for a candidate with no context in force, this is the raw chunk
/// text, exactly as it always was. With it on, the context comes first, then a blank line, then the
/// chunk: the string the embedder built the vector from (`compose_contextualized`). So the reranker
/// judges the text retrieval matched on. The context is trimmed as the embedder trims it and bounded by
/// [`RERANK_CONTEXT_MAX_CHARS`]; the chunk itself is never cut.
#[must_use]
pub fn rerank_document(candidate: &ScoredCandidate, include_context: bool) -> String {
    let context = candidate
        .chunk_context()
        .map(str::trim)
        .filter(|context| !context.is_empty());
    match (include_context, context) {
        (true, Some(context)) => {
            let (kept, _cut) =
                crate::rerank_api::limits::truncate_chars(context, RERANK_CONTEXT_MAX_CHARS);
            format!("{kept}\n\n{}", candidate.text)
        }
        _ => candidate.text.clone(),
    }
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
    #[serde(default)]
    meta: Option<RerankMeta>,
}

#[derive(Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f32,
}

#[derive(Deserialize)]
struct RerankMeta {
    #[serde(default)]
    billed_units: Option<BilledUnits>,
}

#[derive(Deserialize)]
struct BilledUnits {
    #[serde(default)]
    search_units: Option<u64>,
}

/// Why one provider call for `POST /v1/rerank` produced no scores. The variants carry no
/// provider body and no text, so they are safe to log and to map to a response reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankError {
    /// The request budget ran out, or the provider did not answer inside it.
    Timeout,
    /// The provider answered 429 and its `Retry-After` does not fit the remaining budget.
    RateLimited,
    /// A non-success status other than 429.
    Provider(u16),
    /// The connection failed before an answer.
    Transport,
    /// An answer arrived but it was not the expected shape.
    Malformed,
    /// The breaker is open, or its probe is already out. No provider call was made.
    CircuitOpen,
}

/// What one successful provider call returned, before our own validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderScores {
    /// `(input index, relevance score)` as the provider reported them. Not yet validated:
    /// the caller maps them with `rerank_api::map_provider_scores`.
    pub scores: Vec<(usize, f32)>,
    /// The provider's own billed search units, when it reports them.
    pub search_units: Option<u64>,
}

/// Headroom kept after a provider `Retry-After` before a retry is attempted.
const RETRY_HEADROOM: Duration = Duration::from_millis(50);
/// A provider attempt needs at least this much budget left; below it no call is made.
const MIN_ATTEMPT_BUDGET: Duration = Duration::from_millis(50);

/// Permits kept for tenant retrieval (QB-T05b). Web rerank may hold at most `max_inflight` minus
/// this many, so a web burst can never take the last permits tenant retrieval needs.
pub const TENANT_RESERVED_PERMITS: usize = 2;

/// The gate's clock. It is tokio's, so the tests advance it under a paused runtime; in production
/// it is the monotonic clock.
type Clock = tokio::time::Instant;

/// The provider limits of this replica (QB-T05b). Every value is per replica: the deployment runs
/// one retrieval-engine replica, and each extra replica is another full allowance of the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderLimits {
    /// Provider calls this replica may have in flight at once. `0` leaves tenant retrieval
    /// unbounded and gives web rerank no capacity.
    pub max_inflight: usize,
    /// Provider requests this replica may send per second. `0` gives web rerank no capacity.
    pub requests_per_second: u32,
    /// How long a tenant call waits for a permit once the pool is saturated. Past it the call
    /// fails, and the pipeline keeps the fused order as it does for any reranker failure.
    pub tenant_wait: Duration,
    pub breaker: BreakerPolicy,
}

impl Default for ProviderLimits {
    fn default() -> Self {
        Self {
            max_inflight: 0,
            requests_per_second: 0,
            tenant_wait: Duration::from_millis(1_000),
            breaker: BreakerPolicy::default(),
        }
    }
}

impl ProviderLimits {
    /// Whether web rerank has any capacity: a share of the pool above the tenant reserve, and a
    /// request budget. Without both, `POST /v1/rerank` is misconfigured while it is switched on.
    #[must_use]
    pub fn web_capacity(&self) -> bool {
        self.max_inflight > TENANT_RESERVED_PERMITS && self.requests_per_second > 0
    }
}

/// When the provider breaker opens, and for how long (QB-T05b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerPolicy {
    /// Failed provider calls inside `window` that open the breaker.
    pub failures: u32,
    pub window: Duration,
    /// How long the breaker stays open before one probe call is let through.
    pub open_for: Duration,
}

impl Default for BreakerPolicy {
    fn default() -> Self {
        Self {
            failures: 5,
            window: Duration::from_secs(30),
            open_for: Duration::from_secs(30),
        }
    }
}

/// Why the gate did not let a provider call start. Carries no provider data and no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No free permit: web's share of the pool, or the pool itself, is in use.
    Saturated,
    /// The per-second request budget is spent.
    Throttled,
    /// The breaker is open, or its one half-open probe is already out.
    CircuitOpen,
    /// A tenant call waited `tenant_wait` for a permit and none came.
    TenantWaitExpired,
}

impl Refusal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Saturated => "saturated",
            Self::Throttled => "throttled",
            Self::CircuitOpen => "circuit_open",
            Self::TenantWaitExpired => "tenant_wait_expired",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    Web,
    Tenant,
}

impl Lane {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Tenant => "tenant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Circuit {
    Closed,
    Open {
        until: Clock,
    },
    /// `probe_out`: the one probe call is in flight.
    HalfOpen {
        probe_out: bool,
    },
}

impl Circuit {
    /// The gauge value: 0 closed, 1 half-open, 2 open.
    const fn code(self) -> f64 {
        match self {
            Self::Closed => 0.0,
            Self::HalfOpen { .. } => 1.0,
            Self::Open { .. } => 2.0,
        }
    }
}

/// The breaker over every provider answer on this replica. While closed, failures are counted in
/// a sliding window. Open, it admits one probe once its period is over, and the probe decides.
struct Breaker {
    policy: BreakerPolicy,
    circuit: Circuit,
    /// Instants of failed calls inside the window, while closed.
    failures: VecDeque<Clock>,
}

impl Breaker {
    fn new(policy: BreakerPolicy) -> Self {
        Self {
            policy,
            circuit: Circuit::Closed,
            failures: VecDeque::new(),
        }
    }

    /// Whether a web call may start now. `Ok(true)` makes that call the half-open probe.
    fn admit(&mut self, now: Clock) -> Result<bool, Refusal> {
        match self.circuit {
            Circuit::Closed => return Ok(false),
            Circuit::Open { until } if now < until => return Err(Refusal::CircuitOpen),
            Circuit::Open { .. } => {}
            Circuit::HalfOpen { probe_out: true } => return Err(Refusal::CircuitOpen),
            Circuit::HalfOpen { probe_out: false } => {}
        }
        self.circuit = Circuit::HalfOpen { probe_out: true };
        Ok(true)
    }

    /// Whether a web call would be refused now. Claims nothing.
    fn blocks(&self, now: Clock) -> bool {
        match self.circuit {
            Circuit::Closed => false,
            Circuit::Open { until } => now < until,
            Circuit::HalfOpen { probe_out } => probe_out,
        }
    }

    fn release_probe(&mut self) {
        if let Circuit::HalfOpen { probe_out: true } = self.circuit {
            self.circuit = Circuit::HalfOpen { probe_out: false };
        }
    }

    /// Records one settled call. Returns `true` when it opened the breaker.
    fn settle(&mut self, now: Clock, probe: bool, ok: bool) -> bool {
        if probe {
            if ok {
                self.circuit = Circuit::Closed;
                self.failures.clear();
                return false;
            }
            return self.open(now);
        }
        // Only failures while closed count. Answers that come back while open or half-open
        // belong to calls admitted earlier, and the probe alone decides a half-open breaker.
        if ok || self.circuit != Circuit::Closed {
            return false;
        }
        while let Some(&oldest) = self.failures.front() {
            if now.saturating_duration_since(oldest) < self.policy.window {
                break;
            }
            self.failures.pop_front();
        }
        self.failures.push_back(now);
        if self.failures.len() >= self.policy.failures as usize {
            return self.open(now);
        }
        false
    }

    fn open(&mut self, now: Clock) -> bool {
        self.circuit = Circuit::Open {
            until: now + self.policy.open_for,
        };
        self.failures.clear();
        true
    }
}

/// The per-second request budget of web rerank (QB-T05b): a token bucket whose burst is one
/// second of requests.
struct TokenBucket {
    per_second: f64,
    burst: f64,
    level: f64,
    last: Clock,
}

impl TokenBucket {
    fn new(requests_per_second: u32, now: Clock) -> Self {
        let burst = f64::from(requests_per_second);
        Self {
            per_second: burst,
            burst,
            level: burst,
            last: now,
        }
    }

    fn refill(&mut self, now: Clock) {
        if now > self.last {
            let elapsed = (now - self.last).as_secs_f64();
            self.level = (self.level + elapsed * self.per_second).min(self.burst);
            self.last = now;
        }
    }

    /// Takes one token for a web call, or refuses without taking anything.
    fn take(&mut self, now: Clock) -> bool {
        self.refill(now);
        if self.level >= 1.0 {
            self.level -= 1.0;
            true
        } else {
            false
        }
    }

    /// Records a tenant call. Never refused: tenant retrieval keeps its behaviour, and web yields
    /// while the budget is spent. The debt is floored at one burst, so web recovers in bounded time.
    fn spend(&mut self, now: Clock) {
        self.refill(now);
        self.level = (self.level - 1.0).max(-self.burst);
    }
}

/// The provider gate of this replica (QB-T05b): one permit pool, one request budget and one
/// breaker, shared by tenant retrieval and `POST /v1/rerank`.
///
/// Tenant retrieval is the protected lane. It may use the whole pool and waits at most
/// `tenant_wait` for a permit. The budget never refuses it. Its answers feed the breaker. Web
/// rerank may hold at most `max_inflight - TENANT_RESERVED_PERMITS` permits, needs a token from
/// the budget, and is refused while the breaker is open. Its refusals are immediate and make no
/// provider call. With the default limits tenant retrieval is unchanged and web has no capacity.
pub struct ProviderGate {
    limits: ProviderLimits,
    /// Every permit. `None` when `max_inflight` is 0, which leaves tenant retrieval unbounded.
    total: Option<Arc<Semaphore>>,
    /// Web's share, `max_inflight - TENANT_RESERVED_PERMITS`. `None` when there is none.
    web: Option<Arc<Semaphore>>,
    /// `None` when `requests_per_second` is 0.
    bucket: Mutex<Option<TokenBucket>>,
    breaker: Mutex<Breaker>,
}

impl ProviderGate {
    #[must_use]
    pub fn new(limits: ProviderLimits) -> Arc<Self> {
        let now = Clock::now();
        let total =
            (limits.max_inflight > 0).then(|| Arc::new(Semaphore::new(limits.max_inflight)));
        let web = (limits.max_inflight > TENANT_RESERVED_PERMITS).then(|| {
            Arc::new(Semaphore::new(
                limits.max_inflight - TENANT_RESERVED_PERMITS,
            ))
        });
        let bucket = (limits.requests_per_second > 0)
            .then(|| TokenBucket::new(limits.requests_per_second, now));
        Arc::new(Self {
            limits,
            total,
            web,
            bucket: Mutex::new(bucket),
            breaker: Mutex::new(Breaker::new(limits.breaker)),
        })
    }

    /// Admits a tenant call: waits at most `tenant_wait` for a permit, then records one call
    /// against the budget without being refused by it.
    pub async fn admit_tenant(self: &Arc<Self>) -> Result<CallSlot, Refusal> {
        let mut slot = CallSlot::new(self.clone(), Lane::Tenant, false);
        if let Some(total) = &self.total {
            match tokio::time::timeout(self.limits.tenant_wait, total.clone().acquire_owned()).await
            {
                Ok(Ok(permit)) => slot._total = Some(permit),
                _ => return Err(refuse(Lane::Tenant, Refusal::TenantWaitExpired)),
            }
        }
        if let Some(bucket) = lock(&self.bucket).as_mut() {
            bucket.spend(Clock::now());
        }
        Ok(slot)
    }

    /// Admits a web call: the breaker first, then web's share and the pool, then one token of the
    /// budget. It never waits. A refusal is immediate and makes no provider call.
    pub fn admit_web(self: &Arc<Self>) -> Result<CallSlot, Refusal> {
        let now = Clock::now();
        let probe = {
            let mut breaker = lock(&self.breaker);
            let admitted = breaker.admit(now);
            publish_circuit(breaker.circuit);
            admitted.map_err(|refusal| refuse(Lane::Web, refusal))?
        };
        // From here the slot owns the probe, so every early return below releases it.
        let mut slot = CallSlot::new(self.clone(), Lane::Web, probe);
        let (Some(web), Some(total)) = (&self.web, &self.total) else {
            return Err(refuse(Lane::Web, Refusal::Saturated));
        };
        slot._web = Some(
            web.clone()
                .try_acquire_owned()
                .map_err(|_| refuse(Lane::Web, Refusal::Saturated))?,
        );
        slot._total = Some(
            total
                .clone()
                .try_acquire_owned()
                .map_err(|_| refuse(Lane::Web, Refusal::Saturated))?,
        );
        let taken = lock(&self.bucket)
            .as_mut()
            .is_some_and(|bucket| bucket.take(now));
        if taken {
            Ok(slot)
        } else {
            Err(refuse(Lane::Web, Refusal::Throttled))
        }
    }

    /// Charges one tenant retry against the request budget (QB-T05b). Tenant calls are never refused
    /// by the budget, so a retry is not refused either: it spends, as the call admission did.
    fn spend_tenant_retry(&self) {
        if let Some(bucket) = lock(&self.bucket).as_mut() {
            bucket.spend(Clock::now());
        }
    }

    /// Takes one token for a web retry (QB-T05b). Without one the retry is not sent, and the caller
    /// reports the rate limit as it reports a provider 429.
    fn take_web_retry(&self) -> bool {
        lock(&self.bucket)
            .as_mut()
            .is_some_and(|bucket| bucket.take(Clock::now()))
    }
}

impl crate::rerank_api::CircuitGate for ProviderGate {
    /// Whether web rerank would be refused by the breaker right now. Claims nothing.
    fn is_open(&self) -> bool {
        lock(&self.breaker).blocks(Clock::now())
    }
}

/// One admitted provider call. It holds its permits and, when it is the half-open probe, the probe
/// itself. Settle it with the provider's answer. A slot dropped unsettled, because the caller went
/// away, returns its permits and releases a probe without counting the call either way.
pub struct CallSlot {
    gate: Arc<ProviderGate>,
    lane: Lane,
    probe: bool,
    settled: bool,
    _total: Option<OwnedSemaphorePermit>,
    _web: Option<OwnedSemaphorePermit>,
}

impl CallSlot {
    fn new(gate: Arc<ProviderGate>, lane: Lane, probe: bool) -> Self {
        crate::metrics::record_provider_call_started(lane.as_str());
        Self {
            gate,
            lane,
            probe,
            settled: false,
            _total: None,
            _web: None,
        }
    }

    /// Records the provider's answer. `ok` is true for a usable answer. Every failure after the
    /// call's own retries (timeout, transport, status, malformed body) is `false`.
    pub fn settle(mut self, ok: bool) {
        let opened = {
            let mut breaker = lock(&self.gate.breaker);
            let opened = breaker.settle(Clock::now(), self.probe, ok);
            publish_circuit(breaker.circuit);
            opened
        };
        if opened {
            crate::metrics::record_breaker_opened();
        }
        self.settled = true;
    }
}

impl Drop for CallSlot {
    fn drop(&mut self) {
        if self.probe && !self.settled {
            let mut breaker = lock(&self.gate.breaker);
            breaker.release_probe();
            publish_circuit(breaker.circuit);
        }
        crate::metrics::record_provider_call_finished(self.lane.as_str());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish_circuit(circuit: Circuit) {
    crate::metrics::record_breaker_state(circuit.code());
}

fn refuse(lane: Lane, refusal: Refusal) -> Refusal {
    crate::metrics::record_provider_refused(lane.as_str(), refusal.as_str());
    refusal
}

/// How a web refusal reads to the route. An open breaker skips the call. A spent share or budget
/// is a rate limit, which degrades the call as the provider's own 429 does.
fn web_refusal(refusal: Refusal) -> RerankError {
    match refusal {
        Refusal::CircuitOpen => RerankError::CircuitOpen,
        Refusal::Saturated | Refusal::Throttled | Refusal::TenantWaitExpired => {
            RerankError::RateLimited
        }
    }
}

impl RerankClient {
    #[allow(dead_code)]
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_endpoint(api_key, model, "https://api.cohere.ai/v1/rerank", true)
    }

    /// Build with an explicit endpoint + auth style. An empty `endpoint` falls
    /// back to public Cohere.
    ///
    /// `use_bearer` is honored exactly as given -- there is deliberately NO
    /// endpoint sniffing here. An earlier version of this doc claimed an
    /// `*.azure.com` endpoint "auto-selects the api-key header", which the code
    /// never did; that false promise is why nobody noticed `RERANK_ENDPOINT` was
    /// unset. Both `api-key` and `Authorization: Bearer` are in fact accepted by
    /// Azure AI Foundry's Cohere rerank route (verified against the live
    /// deployment), so the auth style was never the failure -- sending an Azure
    /// key to the *public Cohere* default URL was, and that returns 401.
    pub fn with_endpoint(api_key: &str, model: &str, endpoint: &str, use_bearer: bool) -> Self {
        let endpoint = if endpoint.trim().is_empty() {
            "https://api.cohere.ai/v1/rerank".to_string()
        } else {
            endpoint.trim().to_string()
        };
        Self {
            http: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint,
            use_bearer,
            gate: None,
            include_chunk_context: false,
        }
    }

    /// POST builder with the configured URL + auth header.
    fn rerank_post(&self) -> reqwest::RequestBuilder {
        let rb = self.http.post(&self.endpoint);
        if self.use_bearer {
            rb.header("Authorization", format!("Bearer {}", self.api_key))
        } else {
            rb.header("api-key", &self.api_key)
        }
    }

    /// §16.3.5 — when the candidate list is bigger than `PARALLEL_SPLIT_AT`
    /// we split it into chunks and call Cohere in parallel, then merge by
    /// best score per index. Cuts total RTT roughly in half on a 100-item
    /// rerank without changing the contract.
    const PARALLEL_SPLIT_AT: usize = 50;
    const PARALLEL_CHUNK: usize = 32;

    #[tracing::instrument(
        name = "rerank.invoke",
        skip(self, query, candidates),
        fields(
            otel.kind = "client",
            model = self.model.as_str(),
            candidate_count = candidates.len(),
            top_n = top_n,
        ),
    )]
    pub async fn rerank(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }

        // Parallel split path: keep behavior identical for small lists,
        // shard + parallel-call only when it actually saves wall time.
        if candidates.len() > Self::PARALLEL_SPLIT_AT {
            return self.rerank_parallel(query, candidates, top_n).await;
        }

        let documents: Vec<String> = candidates
            .iter()
            .map(|c| rerank_document(c, self.include_chunk_context))
            .collect();

        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents,
            top_n,
        };

        let rerank_resp = self.send_with_retry(&body, Duration::ZERO).await?;

        // §14.2 NaN sanitization: if the rerank API returns NaN/Inf, downstream
        // ordering breaks (any comparison with NaN is `false`, so sort becomes
        // undefined and `final_score` poisons the response). Coerce non-finite
        // scores to 0.0 so the candidate sinks but doesn't corrupt the sort.
        let reranked: Vec<ScoredCandidate> = rerank_resp
            .results
            .into_iter()
            .filter_map(|r| {
                candidates.get(r.index).map(|c| {
                    let mut reranked = c.clone();
                    let score = if r.relevance_score.is_finite() {
                        r.relevance_score
                    } else {
                        tracing::warn!(
                            index = r.index,
                            raw = ?r.relevance_score,
                            "rerank API returned non-finite score; coerced to 0.0"
                        );
                        0.0
                    };
                    reranked.rerank_score = score;
                    reranked.final_score = score;
                    reranked
                })
            })
            .collect();

        Ok(reranked)
    }

    /// The provider client of `POST /v1/rerank`, over the caller's transport settings. Redirects
    /// are never followed: a 307 or 308 would re-send the documents and the `api-key` header to
    /// whatever host it names, and this route is where Quarry-supplied text leaves for a paid
    /// processor. The shared pipeline client keeps its own construction, unchanged.
    pub fn for_route(
        builder: reqwest::ClientBuilder,
        api_key: &str,
        model: &str,
        endpoint: &str,
        use_bearer: bool,
    ) -> reqwest::Result<Self> {
        Ok(Self {
            http: builder
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint: endpoint.trim().to_string(),
            use_bearer,
            gate: None,
            include_chunk_context: false,
        })
    }

    /// Puts this client under the replica's provider gate (QB-T05b). Production wires one gate
    /// into the tenant reranker and into the route. A client without a gate is ungated, which is
    /// what the constructors produce; tests that build a client directly rely on that.
    #[must_use]
    pub fn with_gate(mut self, gate: Arc<ProviderGate>) -> Self {
        self.gate = Some(gate);
        self
    }

    /// Sends each chunk with its context in force ahead of it (CR-T15). The pipeline's client sets this
    /// from `RERANK_INPUT_INCLUDE_CONTEXT`; with it off every document is the raw chunk text.
    #[must_use]
    pub fn with_chunk_context_input(mut self, on: bool) -> Self {
        self.include_chunk_context = on;
        self
    }

    /// Whether this client sends chunk context to the provider.
    #[must_use]
    pub fn includes_chunk_context(&self) -> bool {
        self.include_chunk_context
    }

    /// `POST /v1/rerank`: one request carrying every candidate, no sharding, under a hard
    /// budget. The budget is a per-request timeout, so the socket is cut at the deadline and
    /// an abandoned call holds no connection. A 429 is retried once, and only when its
    /// `Retry-After` fits the budget left and a token of the request budget is free; otherwise it
    /// is reported at once, without sleeping.
    #[tracing::instrument(
        name = "rerank.texts",
        skip(self, query, texts),
        fields(
            otel.kind = "client",
            model = self.model.as_str(),
            documents = texts.len(),
            top_n = top_n,
        ),
    )]
    pub async fn rerank_texts(
        &self,
        query: &str,
        texts: &[String],
        top_n: usize,
        budget: Duration,
    ) -> Result<ProviderScores, RerankError> {
        let slot = match &self.gate {
            Some(gate) => Some(gate.admit_web().map_err(web_refusal)?),
            None => None,
        };
        let outcome = self.rerank_texts_within(query, texts, top_n, budget).await;
        if let Some(slot) = slot {
            slot.settle(outcome.is_ok());
        }
        outcome
    }

    /// The attempts of one `POST /v1/rerank` call under its budget, after admission.
    async fn rerank_texts_within(
        &self,
        query: &str,
        texts: &[String],
        top_n: usize,
        budget: Duration,
    ) -> Result<ProviderScores, RerankError> {
        let started = Instant::now();
        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: texts.to_vec(),
            top_n,
        };
        let mut retried = false;
        loop {
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining < MIN_ATTEMPT_BUDGET {
                return Err(RerankError::Timeout);
            }
            let sent = self.rerank_post().timeout(remaining).json(&body).send();
            let response = match tokio::time::timeout(remaining, sent).await {
                Err(_) => return Err(RerankError::Timeout),
                Ok(Err(error)) if error.is_timeout() => return Err(RerankError::Timeout),
                Ok(Err(_)) => return Err(RerankError::Transport),
                Ok(Ok(response)) => response,
            };
            let status = response.status();
            if status.as_u16() == 429 {
                let window = parse_retry_after(&response);
                let left = budget.saturating_sub(started.elapsed());
                match window {
                    Some(wait) if !retried && wait + RETRY_HEADROOM <= left => {
                        // The retry is a provider request of its own: it needs a token of the request
                        // budget before it is sent (QB-T05b). Without one it is not sent at all.
                        if self
                            .gate
                            .as_ref()
                            .is_some_and(|gate| !gate.take_web_retry())
                        {
                            return Err(RerankError::RateLimited);
                        }
                        retried = true;
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    _ => return Err(RerankError::RateLimited),
                }
            }
            if !status.is_success() {
                return Err(RerankError::Provider(status.as_u16()));
            }
            let parsed = response.json::<RerankResponse>().await.map_err(|error| {
                if error.is_timeout() {
                    RerankError::Timeout
                } else {
                    RerankError::Malformed
                }
            })?;
            let search_units = parsed
                .meta
                .and_then(|meta| meta.billed_units)
                .and_then(|units| units.search_units);
            return Ok(ProviderScores {
                scores: parsed
                    .results
                    .iter()
                    .map(|result| (result.index, result.relevance_score))
                    .collect(),
                search_units,
            });
        }
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// The one place a rerank HTTP call is made, with 429/5xx retry.
    ///
    /// Extracted because it used to exist only inline in the small-list path,
    /// while `rerank_single` — the sharded path, which is the COMMON one, since
    /// six fused arms routinely exceed `PARALLEL_SPLIT_AT` candidates — sent
    /// bare requests. One 429 on any shard failed the whole rerank, and because
    /// rerank failure deliberately degrades to fused order, the cross-encoder
    /// was silently absent from most production queries (measured: 91 of 103
    /// retrievals degraded during one eval window, all 429).
    ///
    /// `stagger` delays the FIRST attempt: `rerank_parallel` fires shards
    /// concurrently, and against a per-second rate window simultaneous shards
    /// guarantee that all but one 429 on arrival and then retry in lockstep.
    /// Offsetting each shard's start spreads them across the window instead.
    async fn send_with_retry(
        &self,
        body: &RerankRequest,
        stagger: Duration,
    ) -> anyhow::Result<RerankResponse> {
        if !stagger.is_zero() {
            tokio::time::sleep(stagger).await;
        }
        let slot = match &self.gate {
            Some(gate) => Some(gate.admit_tenant().await.map_err(|refusal| {
                anyhow::anyhow!("rerank provider not admitted: {}", refusal.as_str())
            })?),
            None => None,
        };
        let outcome = self.send_attempts(body).await;
        if let Some(slot) = slot {
            slot.settle(outcome.is_ok());
        }
        outcome
    }

    /// The attempts of one tenant call, after the admission `send_with_retry` took for it.
    async fn send_attempts(&self, body: &RerankRequest) -> anyhow::Result<RerankResponse> {
        let mut last_err = None;
        let mut retry_after: Option<Duration> = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = backoff_for(attempt, retry_after.take());
                tracing::warn!(attempt, ?backoff, "rerank retry");
                tokio::time::sleep(backoff).await;
                // The retry is a provider request of its own: it spends the request budget (QB-T05b).
                if let Some(gate) = &self.gate {
                    gate.spend_tenant_retry();
                }
            }

            let resp = match self.rerank_post().json(body).send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(anyhow::anyhow!(e).context("rerank API call failed"));
                    continue;
                }
            };

            if resp.status().is_server_error() || resp.status().as_u16() == 429 {
                let status = resp.status();
                retry_after = parse_retry_after(&resp);
                last_err = Some(sanitized_rerank_status_error(status));
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                return Err(sanitized_rerank_status_error(status));
            }

            return resp
                .json::<RerankResponse>()
                .await
                .context("parse rerank response");
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("rerank retries exhausted")))
    }

    /// Shard the candidate list, rerank each shard concurrently, merge by
    /// taking the top-N globally. A Vec of futures + `join_all`, so failure of
    /// one shard fails the whole call — retries live INSIDE each shard's
    /// `send_with_retry`; nothing above this retries (the postprocess stage
    /// deliberately degrades to fused order instead).
    async fn rerank_parallel(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let chunks: Vec<&[ScoredCandidate]> = candidates.chunks(Self::PARALLEL_CHUNK).collect();
        let per_chunk_top = top_n.max(8);

        let mut tasks = Vec::with_capacity(chunks.len());
        for (i, chunk) in chunks.into_iter().enumerate() {
            // Each chunk is reranked against the same query, asking for the
            // chunk's own top-N. We re-merge globally below.
            //
            // Staggered starts: simultaneous shards against a per-second rate
            // window guarantee all but one 429 on arrival, then retry in
            // lockstep and 429 again — the split's whole latency benefit spent
            // on synchronized failure. The offset spreads shard arrivals across
            // the window; on an unthrottled provider it costs at most
            // (shards-1) x 300ms, bounded by the small shard count.
            tasks.push(self.rerank_single(
                query,
                chunk,
                per_chunk_top,
                Duration::from_millis(300) * i as u32,
            ));
        }

        let results = futures::future::join_all(tasks).await;
        let mut merged: Vec<ScoredCandidate> = Vec::new();
        for r in results {
            merged.extend(r?);
        }

        // Sort by final_score desc; NaN-safe because rerank_single() already
        // coerced non-finite scores to 0.0 (§14.2).
        merged.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        merged.truncate(top_n);
        Ok(merged)
    }

    /// Single-call rerank — extracted so `rerank_parallel` can reuse it.
    /// Identical body to the small-list path in `rerank()`, just factored out.
    async fn rerank_single(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
        stagger: Duration,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let documents: Vec<String> = candidates
            .iter()
            .map(|c| rerank_document(c, self.include_chunk_context))
            .collect();
        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents,
            top_n,
        };

        let rerank_resp = self.send_with_retry(&body, stagger).await?;

        Ok(rerank_resp
            .results
            .into_iter()
            .filter_map(|r| {
                candidates.get(r.index).map(|c| {
                    let mut out = c.clone();
                    let score = if r.relevance_score.is_finite() {
                        r.relevance_score
                    } else {
                        0.0
                    };
                    out.rerank_score = score;
                    out.final_score = score;
                    out
                })
            })
            .collect())
    }
}

fn sanitized_rerank_status_error(status: reqwest::StatusCode) -> anyhow::Error {
    anyhow::anyhow!("rerank API returned HTTP status {}", status.as_u16())
}

#[cfg(test)]
mod chunk_context_tests {
    use super::{rerank_document, RerankRequest, RERANK_CONTEXT_MAX_CHARS};
    use crate::pipeline::types::ScoredCandidate;

    fn candidate(text: &str, context: Option<&str>) -> ScoredCandidate {
        let mut candidate = ScoredCandidate {
            knowledge_id: "k1".to_string(),
            document_id: "d1".to_string(),
            text: text.to_string(),
            dense_score: 0.0,
            sparse_score: 0.0,
            rerank_score: 0.0,
            final_score: 0.0,
            chunk_index: 0,
            metadata: std::collections::HashMap::new(),
        };
        candidate.attach_chunk_context(context.map(str::to_owned));
        candidate
    }

    // Flag off: the request is the one the code always sent, the raw chunk texts in order,
    // serialized by the same struct. A context on the candidate is not consulted at all.
    #[test]
    fn flag_off_request_body_is_byte_identical_to_the_raw_text_body() {
        let candidates = [
            candidate("Margin: 31%.", Some("Fra ACMEs Q2-rapport.")),
            candidate("Lønn og pensjon.", None),
        ];
        let documents: Vec<String> = candidates
            .iter()
            .map(|candidate| rerank_document(candidate, false))
            .collect();
        let body = serde_json::to_string(&RerankRequest {
            model: "m".to_string(),
            query: "q".to_string(),
            documents,
            top_n: 2,
        })
        .expect("the request serializes");
        assert_eq!(
            body,
            r#"{"model":"m","query":"q","documents":["Margin: 31%.","Lønn og pensjon."],"top_n":2}"#
        );
    }

    // Flag on: the document is compose_contextualized(context, text), the string the vector was
    // built from. The context is trimmed, as the embedder trims it; the chunk is never touched.
    #[test]
    fn flag_on_document_is_the_trimmed_context_a_blank_line_and_the_chunk() {
        assert_eq!(
            rerank_document(
                &candidate("Margin: 31%.", Some("  Fra ACMEs Q2-rapport.  ")),
                true
            ),
            "Fra ACMEs Q2-rapport.\n\nMargin: 31%."
        );
        assert_eq!(
            rerank_document(
                &candidate("  kort tekst  ", Some("Dokumentet om marginer.")),
                true
            ),
            "Dokumentet om marginer.\n\n  kort tekst  ",
            "the chunk text is never trimmed"
        );
    }

    #[test]
    fn a_candidate_without_context_reranks_on_its_raw_text_either_way() {
        assert_eq!(
            rerank_document(&candidate("Rå tekst.", None), true),
            "Rå tekst."
        );
        assert_eq!(
            rerank_document(&candidate("Rå tekst.", Some(" \n ")), true),
            "Rå tekst.",
            "a blank context is no context"
        );
        assert_eq!(
            rerank_document(&candidate("Rå tekst.", Some("Kontekst.")), false),
            "Rå tekst.",
            "with the flag off the context is ignored"
        );
    }

    // The context is bounded by the 600 characters the generator accepts, so a context the
    // generator wrote is never cut here.
    #[test]
    fn a_context_at_the_cap_is_kept_whole() {
        assert_eq!(RERANK_CONTEXT_MAX_CHARS, 600, "the generator's MAX_CHARS");
        let context = "å".repeat(RERANK_CONTEXT_MAX_CHARS);
        assert_eq!(
            rerank_document(&candidate("T.", Some(&context)), true),
            format!("{context}\n\nT.")
        );
    }

    // A longer context is cut at a character boundary. Norwegian letters are two bytes, so a byte
    // cut would split a code point and panic or corrupt the text. The chunk is never cut.
    #[test]
    fn an_over_long_norwegian_context_is_cut_at_a_character_boundary() {
        let context = "æøå Ærlig Østlandet ".repeat(80);
        assert!(context.chars().count() > RERANK_CONTEXT_MAX_CHARS);
        let document = rerank_document(&candidate("Chunk.", Some(&context)), true);
        let (head, tail) = document
            .split_once("\n\n")
            .expect("context, a blank line, then the chunk");
        assert_eq!(head.chars().count(), RERANK_CONTEXT_MAX_CHARS);
        assert!(
            context.starts_with(head),
            "the kept context is a prefix of the stored one"
        );
        assert_eq!(tail, "Chunk.", "the chunk is never cut");
    }
}
