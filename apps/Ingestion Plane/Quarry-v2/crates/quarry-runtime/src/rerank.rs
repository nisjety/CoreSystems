//! Semantic rerank of search results (Quarry option B).
//!
//! After the router or hybrid provider returns a merged result set (good *recall*), the Data
//! Plane's `POST /v1/rerank` orders the top-N by a cross-encoder score for the query. The Data
//! Plane owns the capability and its paid provider. Quarry sends the candidates it already holds
//! and reads the order back. No general model is prompted, and no Model Plane token is minted for
//! this call.
//!
//! The wrapper is [`RerankingSearchProvider`], a [`crate::serp::SearchProvider`], so it slots into
//! `AppState.search` transparently and its output is cached by the edge. The production reranker
//! is [`DataPlaneSearchReranker`].
//!
//! Degrade-safe: a Data Plane failure, refusal or unreadable answer returns the input order with no
//! scores, and says so in the [`RerankReport`] (`degraded`, with a reason). A search that was never
//! asked says `skipped`, and why. A chain with no reranker says `rerank-not-configured`. Shadow mode
//! asks, measures and keeps the input order (`shadow`). The long tail beyond `top_n` is never
//! reordered: only the head the user is most likely to read.

use std::cmp::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use quarry_core::error::QuarryResult;
use quarry_core::zdr::ZdrMode;

use crate::data_plane_http::{DataPlaneCallError, DataPlaneHttp};
use crate::serp::{SearchOptions, SearchProvider, SearchResult};
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};
use crate::vector_index::DataPlaneZdrMode;

/// The profile the Data Plane serves. It resolves the provider model server-side.
pub const RERANK_PROFILE: &str = "web-v1";
/// The route, appended to the Data Plane's base URL.
const RERANK_PATH: &str = "/v1/rerank";
/// The most candidates one call may carry: the Data Plane's `documents` limit.
pub const MAX_RERANK_DOCUMENTS: usize = 100;
/// The most query characters one call may carry: the Data Plane's `query` limit.
pub const MAX_RERANK_QUERY_CHARS: usize = 1_024;
/// The Data Plane's `deadline_ms` range.
const DEADLINE_MIN_MS: u128 = 100;
const DEADLINE_MAX_MS: u128 = 8_000;
/// The budget of one call when the caller sets none. It is sent as `deadline_ms`.
pub const DEFAULT_RERANK_DEADLINE: Duration = Duration::from_millis(2_000);
/// How many leading results are reordered when the configuration sets no figure.
pub const DEFAULT_RERANK_TOP_N: usize = 10;
/// Time past the budget, so the Data Plane's own timeout answer can arrive before Quarry gives up.
const TRANSPORT_ALLOWANCE: Duration = Duration::from_millis(500);
/// The client's own bound, above the largest budget the Data Plane accepts. The per-call deadline
/// is the real limit.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
/// Consecutive failed calls before the reranker stops calling the Data Plane for a while.
pub const BREAKER_FAILURE_THRESHOLD: u32 = 5;
/// How long the breaker stays open before it admits one probe call.
pub const BREAKER_OPEN_FOR: Duration = Duration::from_secs(30);
/// Organisations that hold no tenant data, so a rerank for one is never attempted. `onboarding` is
/// the pre-org preview sentinel of Auth Core (`ONBOARDING_ORG_SENTINEL`). `global` is the
/// pseudo-organisation of its health probe, which no directory holds.
const NON_TENANT_ORGS: &[&str] = &["onboarding", "global"];

/// What the rerank step did for one search. `applied` means the scores ordered the head of the
/// results. `skipped` means the Data Plane was never asked, and `reason` says why. `degraded` means
/// it was asked and failed, so the results are in input order. `shadow` means it was asked, and its
/// order was measured but not applied. The edge cache stores only the outcomes that are final for
/// the epoch in its key, which are `applied` and `rerank-not-configured`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RerankStatus {
    Applied,
    Skipped,
    Degraded,
    /// The Data Plane was asked and its order was measured, but the results keep their input
    /// order and carry no score (shadow mode, `QUARRY_EDGE__RERANK_SHADOW`).
    Shadow,
    /// No rerank step is wired into the provider chain. This is the default, so a provider that
    /// does not implement `search_with_report` reports it.
    #[default]
    RerankNotConfigured,
}

impl RerankStatus {
    /// Whether an outcome with this status may be written to the edge cache. `skipped`, `degraded`
    /// and `shadow` are transient states of the reranker. Caching them would pin unreranked results
    /// for the whole TTL after the reranker recovered.
    pub fn is_cacheable(&self) -> bool {
        matches!(self, Self::Applied | Self::RerankNotConfigured)
    }

    /// The label the log line carries: the same words the wire carries.
    pub fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Skipped => "skipped",
            Self::Degraded => "degraded",
            Self::Shadow => "shadow",
            Self::RerankNotConfigured => "rerank-not-configured",
        }
    }
}

/// The rerank report carried with every search. It is reported to the caller and kept in the edge
/// cache, so a cache hit reports the status of the search it replays.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RerankReport {
    pub status: RerankStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The rerank profile that ran, or that would have run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

impl RerankReport {
    pub fn applied(profile: Option<String>) -> Self {
        Self {
            status: RerankStatus::Applied,
            reason: None,
            profile,
        }
    }

    pub fn skipped(reason: &str, profile: Option<String>) -> Self {
        Self {
            status: RerankStatus::Skipped,
            reason: Some(reason.to_string()),
            profile,
        }
    }

    pub fn degraded(reason: &str, profile: Option<String>) -> Self {
        Self {
            status: RerankStatus::Degraded,
            reason: Some(reason.to_string()),
            profile,
        }
    }

    /// The report of a shadow call: the Data Plane's answer, recorded but not applied. A call that
    /// would have reordered the results says `would_apply`. Any other answer keeps its own reason.
    pub fn shadow_of(report: &RerankReport) -> Self {
        let reason = match report.status {
            RerankStatus::Applied => Some("would_apply".to_string()),
            _ => report.reason.clone(),
        };
        Self {
            status: RerankStatus::Shadow,
            reason,
            profile: report.profile.clone(),
        }
    }

    /// True for the default report. The response skips the field when this holds, so a chain with
    /// no rerank step keeps the wire shape it had before reports existed.
    pub fn is_not_configured(&self) -> bool {
        self.status == RerankStatus::RerankNotConfigured
    }
}

/// A reranker's result: the reordered results and the report that explains them.
#[derive(Debug, Clone)]
pub struct RerankOutcome {
    pub results: Vec<SearchResult>,
    pub report: RerankReport,
}

/// The rerank configuration a search runs under. It is part of the edge cache key, so a change to
/// any field makes the entries written under the old one unreachable instead of serving them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RerankEpoch {
    pub profile: Option<String>,
    pub enabled: bool,
    pub shadow: bool,
    pub top_n: usize,
}

impl RerankEpoch {
    /// Canonical text for the cache key. The profile is Debug-formatted so that a profile name
    /// cannot be mistaken for a field separator.
    pub fn signature(&self) -> String {
        format!(
            "rp={:?};re={};rs={};rn={}",
            self.profile, self.enabled, self.shadow, self.top_n
        )
    }
}

/// What one rerank call may do, decided by the search before the reranker is asked.
#[derive(Debug, Clone, Copy)]
pub struct RerankContext<'a> {
    /// The verified tenant of the search. It is never empty for a call that reaches the Data Plane.
    pub org_id: &'a str,
    /// The run's effective zero-retention posture. A ZDR run is never sent to the Data Plane.
    pub zdr: bool,
    /// The deployment is sovereign (`zero_saas_search`): no SaaS provider is called.
    pub sovereign: bool,
    /// The call's id: the Data Plane's idempotency and log correlation key.
    pub request_id: &'a str,
    /// The call's budget: sent as `deadline_ms`, and the most Quarry waits.
    pub deadline: Duration,
    /// How many leading results to reorder. The tail is left in place.
    pub top_n: usize,
}

/// Reorders the head of a result set for a query. Implementations MUST be degrade-safe: return the
/// input order with a `degraded` or `skipped` report on any failure, rather than erroring.
#[async_trait]
pub trait SearchReranker: Send + Sync {
    async fn rerank(
        &self,
        query: &str,
        results: Vec<SearchResult>,
        ctx: &RerankContext<'_>,
    ) -> RerankOutcome;

    /// The profile this reranker runs, for the cache epoch. `None` when it has no profile of its own.
    fn profile(&self) -> Option<String> {
        None
    }
}

/// Consecutive-failure breaker for the rerank route. Open, it answers `skipped circuit_open` with no
/// call. After [`BREAKER_OPEN_FOR`] it admits one probe. It is per process: every replica keeps its
/// own, as the Data Plane's own limits do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakerState {
    Closed { failures: u32 },
    Open { since: Instant },
    HalfOpen { probing: bool },
}

struct RerankBreaker {
    state: Mutex<BreakerState>,
}

impl Default for RerankBreaker {
    fn default() -> Self {
        Self {
            state: Mutex::new(BreakerState::Closed { failures: 0 }),
        }
    }
}

impl RerankBreaker {
    /// Whether a call may go out now. Once the open period has passed, one probe is admitted.
    fn admit(&self, now: Instant) -> bool {
        let mut state = lock(&self.state);
        match *state {
            BreakerState::Closed { .. } => true,
            BreakerState::Open { since } if now.duration_since(since) >= BREAKER_OPEN_FOR => {
                *state = BreakerState::HalfOpen { probing: true };
                true
            }
            BreakerState::Open { .. } => false,
            BreakerState::HalfOpen { probing: false } => {
                *state = BreakerState::HalfOpen { probing: true };
                true
            }
            BreakerState::HalfOpen { probing: true } => false,
        }
    }

    fn succeeded(&self) {
        *lock(&self.state) = BreakerState::Closed { failures: 0 };
    }

    fn failed(&self, now: Instant) {
        let mut state = lock(&self.state);
        *state = match *state {
            BreakerState::Closed { failures } if failures + 1 >= BREAKER_FAILURE_THRESHOLD => {
                BreakerState::Open { since: now }
            }
            BreakerState::Closed { failures } => BreakerState::Closed {
                failures: failures + 1,
            },
            BreakerState::HalfOpen { .. } | BreakerState::Open { .. } => {
                BreakerState::Open { since: now }
            }
        };
    }

    /// The call said nothing about the provider, such as our own rate gate refusing it. The probe
    /// is released without counting either way.
    fn neutral(&self) {
        let mut state = lock(&self.state);
        if let BreakerState::HalfOpen { probing: true } = *state {
            *state = BreakerState::HalfOpen { probing: false };
        }
    }
}

/// Releases a half-open probe when the call that took it is dropped before it settles. Every settled
/// path leaves the breaker out of the probing state, so this only ever releases a cancelled probe.
struct ProbeGuard<'a>(&'a RerankBreaker);

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        self.0.neutral();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The production reranker: one call per search to the Data Plane's `POST /v1/rerank` (profile
/// `web-v1`), under the `web-rerank` manifest row. It holds the breaker and the log throttle for
/// that route.
pub struct DataPlaneSearchReranker {
    http: DataPlaneHttp,
    token_request: ServiceTokenRequest,
    breaker: RerankBreaker,
}

impl DataPlaneSearchReranker {
    /// `base_url` is the Data Plane root. The rerank route is appended.
    pub fn new(
        base_url: impl AsRef<str>,
        tokens: SharedServiceTokenProvider,
    ) -> QuarryResult<Self> {
        let base = base_url.as_ref().trim_end_matches('/');
        Ok(Self {
            http: DataPlaneHttp::new(format!("{base}{RERANK_PATH}"), CLIENT_TIMEOUT)?
                .with_token_provider(tokens),
            // The only grant this call makes: `data:rerank` on the data-plane audience.
            token_request: ServiceTokenRequest::from_manifest("web-rerank")?,
            breaker: RerankBreaker::default(),
        })
    }

    /// A refusal of the call by the Data Plane. It is logged at ERROR, at most once per window: the
    /// window belongs to the route (see `DataPlaneHttp`), so token refusals share it.
    fn refused(&self, status: u16, reason: &'static str) {
        if self.http.admit_error_log() {
            tracing::error!(
                status,
                reason,
                "semantic rerank: the Data Plane refused the call, so results keep their lexical order. \
                 401 and 403 mean the quarry-edge principal lacks data:rerank or its audience; 404 and 422 \
                 mean the rerank-v1 contract has drifted"
            );
        }
    }
}

#[async_trait]
impl SearchReranker for DataPlaneSearchReranker {
    async fn rerank(
        &self,
        query: &str,
        results: Vec<SearchResult>,
        ctx: &RerankContext<'_>,
    ) -> RerankOutcome {
        let profile = self.profile();
        // Posture first. A request that may not leave is never sent, and no token is minted for it.
        if ctx.zdr {
            return skipped_outcome(results, "zdr", profile);
        }
        if ctx.sovereign {
            return skipped_outcome(results, "sovereign", profile);
        }
        if !is_tenant_org(ctx.org_id) {
            return skipped_outcome(results, "input_not_tenant", profile);
        }
        let n = ctx.top_n.min(results.len()).min(MAX_RERANK_DOCUMENTS);
        if n == 0 {
            return skipped_outcome(results, "input_too_small", profile);
        }
        let query = clamp_query(query);
        if query.is_empty() {
            return skipped_outcome(results, "empty_query", profile);
        }
        if !self.breaker.admit(Instant::now()) {
            return skipped_outcome(results, "circuit_open", profile);
        }
        // A call dropped before it settles (its caller went away) must not hold the half-open probe
        // for ever. Every settled path leaves the breaker out of the probing state.
        let _probe = ProbeGuard(&self.breaker);

        let body = WireRequest::new(ctx, &query, n, &results[..n]);
        let call = self.http.post_json(&self.token_request, ctx.org_id, &body);
        let answer = match tokio::time::timeout(ctx.deadline + TRANSPORT_ALLOWANCE, call).await {
            Ok(answer) => answer,
            Err(_elapsed) => {
                self.breaker.failed(Instant::now());
                tracing::warn!(
                    reason = "timeout",
                    deadline_ms = ctx.deadline.as_millis() as u64,
                    "semantic rerank: the Data Plane did not answer inside its deadline; lexical order stands"
                );
                return degraded_outcome(results, "timeout", profile);
            }
        };
        match answer {
            Ok(response) => {
                self.read(response, ctx.request_id, n, results, profile)
                    .await
            }
            Err(DataPlaneCallError::Token(_)) => {
                self.breaker.failed(Instant::now());
                degraded_outcome(results, "token_unavailable", profile)
            }
            Err(DataPlaneCallError::Timeout(error)) => {
                self.breaker.failed(Instant::now());
                // zdr-log-ok: a ZDR search returns skipped above and never reaches this call; the reqwest text names only the operator-configured Data Plane URL (the query travels in the POST body)
                tracing::warn!(reason = "timeout", error = %error, "semantic rerank: the Data Plane timed out");
                degraded_outcome(results, "timeout", profile)
            }
            Err(DataPlaneCallError::Unreachable(error)) => {
                self.breaker.failed(Instant::now());
                // zdr-log-ok: a ZDR search returns skipped above and never reaches this call; the reqwest text names only the operator-configured Data Plane URL (the query travels in the POST body)
                tracing::warn!(reason = "dp_unreachable", error = %error, "semantic rerank: the Data Plane could not be reached");
                degraded_outcome(results, "unreachable", profile)
            }
        }
    }

    fn profile(&self) -> Option<String> {
        Some(RERANK_PROFILE.to_owned())
    }
}

impl DataPlaneSearchReranker {
    /// Classifies one answer. Every outcome keeps the input order unless the answer scores the
    /// head, and every failure is counted by the breaker.
    async fn read(
        &self,
        response: reqwest::Response,
        request_id: &str,
        n: usize,
        results: Vec<SearchResult>,
        profile: Option<String>,
    ) -> RerankOutcome {
        let status = response.status().as_u16();
        match status {
            200 => {}
            429 => {
                // Our own rate gate in the Data Plane. The provider was not called, so it is not a
                // failure of the route.
                self.breaker.neutral();
                tracing::warn!(
                    reason = "dp_rate_limited",
                    "semantic rerank: the Data Plane rate-limited the call"
                );
                return degraded_outcome(results, "rate_limited", profile);
            }
            401 | 403 | 404 | 422 => {
                self.breaker.failed(Instant::now());
                self.refused(status, "dp_refused");
                return degraded_outcome(results, "refused", profile);
            }
            400 | 413 => {
                self.breaker.failed(Instant::now());
                self.refused(status, "dp_contract");
                return degraded_outcome(results, "contract", profile);
            }
            500..=599 => {
                self.breaker.failed(Instant::now());
                tracing::warn!(
                    reason = "dp_unavailable",
                    status,
                    "semantic rerank: the Data Plane is unavailable"
                );
                return degraded_outcome(results, "provider_error", profile);
            }
            _ => {
                self.breaker.failed(Instant::now());
                self.refused(status, "dp_contract");
                return degraded_outcome(results, "contract", profile);
            }
        }

        let answer: WireAnswer = match response.json().await {
            Ok(answer) => answer,
            Err(error) if error.is_decode() => {
                self.breaker.failed(Instant::now());
                if self.http.admit_error_log() {
                    // zdr-log-ok: a ZDR search never reaches the Data Plane call (posture first in rerank()); a serde decode error names a field or a position, not the query
                    tracing::error!(
                        reason = "dp_contract",
                        error = %error,
                        "semantic rerank: the answer does not match rerank-v1; this call keeps the lexical order"
                    );
                }
                return degraded_outcome(results, "contract", profile);
            }
            Err(error) => {
                self.breaker.failed(Instant::now());
                // zdr-log-ok: a ZDR search never reaches the Data Plane call (posture first in rerank()); the reqwest text names only the operator-configured Data Plane URL
                tracing::warn!(reason = "dp_unreachable", error = %error, "semantic rerank: the answer was cut off");
                return degraded_outcome(results, "unreachable", profile);
            }
        };
        if answer.request_id != request_id || answer.profile != RERANK_PROFILE {
            // An answer to another call, or another profile, must never reorder these results.
            self.breaker.failed(Instant::now());
            if self.http.admit_error_log() {
                tracing::error!(
                    reason = "dp_contract",
                    "semantic rerank: the answer names another request or profile; this call keeps the lexical order"
                );
            }
            return degraded_outcome(results, "contract", profile);
        }
        match answer.status.as_str() {
            "ok" => {
                let scored = valid_scores(&answer.results, n);
                if scored.is_empty() {
                    self.breaker.failed(Instant::now());
                    return degraded_outcome(results, "no_scores", profile);
                }
                self.breaker.succeeded();
                RerankOutcome {
                    results: apply_scores(results, &scored, n),
                    report: RerankReport::applied(profile),
                }
            }
            "skipped" => {
                // The Data Plane answered and declined on posture or configuration. That is not a
                // failure of the route.
                self.breaker.succeeded();
                skipped_outcome(results, dp_skip_reason(answer.reason.as_deref()), profile)
            }
            "degraded" => {
                self.breaker.failed(Instant::now());
                degraded_outcome(
                    results,
                    dp_degrade_reason(answer.reason.as_deref()),
                    profile,
                )
            }
            _ => {
                self.breaker.failed(Instant::now());
                if self.http.admit_error_log() {
                    tracing::error!(
                        reason = "dp_contract",
                        "semantic rerank: the answer has an unknown status"
                    );
                }
                degraded_outcome(results, "contract", profile)
            }
        }
    }
}

/// The Data Plane's skip reasons that Quarry passes on. Anything else is `unknown`, so the report
/// carries a fixed vocabulary.
fn dp_skip_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("disabled") => "disabled",
        Some("misconfigured") => "misconfigured",
        Some("processor_unapproved") => "processor_unapproved",
        Some("metering_unavailable") => "metering_unavailable",
        Some("zdr") => "zdr",
        Some("sovereign") => "sovereign",
        Some("circuit_open") => "circuit_open",
        _ => "unknown",
    }
}

/// The Data Plane's degrade reasons, reduced to the three Quarry reports.
fn dp_degrade_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("timeout") => "timeout",
        Some("provider_rate_limited") => "provider_rate_limited",
        _ => "provider_error",
    }
}

fn skipped_outcome(
    results: Vec<SearchResult>,
    reason: &str,
    profile: Option<String>,
) -> RerankOutcome {
    RerankOutcome {
        results,
        report: RerankReport::skipped(reason, profile),
    }
}

fn degraded_outcome(
    results: Vec<SearchResult>,
    reason: &str,
    profile: Option<String>,
) -> RerankOutcome {
    RerankOutcome {
        results,
        report: RerankReport::degraded(reason, profile),
    }
}

/// Whether an org is a tenant. An empty id and the non-tenant orgs are not, and a rerank for them is
/// never attempted.
pub fn is_tenant_org(org_id: &str) -> bool {
    let org = org_id.trim();
    !org.is_empty() && !NON_TENANT_ORGS.contains(&org)
}

/// A fresh id for one rerank call: `qrr-` and a ULID, 30 characters the Data Plane accepts.
pub fn new_request_id() -> String {
    format!("qrr-{}", ulid::Ulid::new())
}

/// The query as the Data Plane accepts it: trimmed, and cut to its character limit.
fn clamp_query(query: &str) -> String {
    query.trim().chars().take(MAX_RERANK_QUERY_CHARS).collect()
}

/// The text the cross-encoder reads for one candidate: its title and snippet, or its address when it
/// has neither. Never empty, because the Data Plane refuses an empty candidate.
fn candidate_text(result: &SearchResult, index: usize) -> String {
    let title = result.title.as_deref().map(str::trim).unwrap_or("");
    let snippet = result.snippet.as_deref().map(str::trim).unwrap_or("");
    let text = match (title.is_empty(), snippet.is_empty()) {
        (false, false) => format!("{title}\n{snippet}"),
        (false, true) => title.to_owned(),
        (true, false) => snippet.to_owned(),
        (true, true) => result.url.trim().to_owned(),
    };
    if text.trim().is_empty() {
        format!("result {index}")
    } else {
        text
    }
}

/// The `rerank-v1` request. Field names are the Data Plane's; it refuses unknown fields.
#[derive(Serialize)]
struct WireRequest<'a> {
    request_id: &'a str,
    query: &'a str,
    profile: &'static str,
    zdr_mode: DataPlaneZdrMode,
    sovereign_required: bool,
    top_n: usize,
    deadline_ms: u64,
    documents: Vec<WireDocument>,
}

#[derive(Serialize)]
struct WireDocument {
    /// The candidate's position in the head. It is the key the answer maps back by.
    id: String,
    text: String,
}

impl<'a> WireRequest<'a> {
    fn new(ctx: &RerankContext<'a>, query: &'a str, n: usize, head: &[SearchResult]) -> Self {
        Self {
            request_id: ctx.request_id,
            query,
            profile: RERANK_PROFILE,
            // A ZDR run returned before this point, so the run's mode is the non-retention one.
            zdr_mode: DataPlaneZdrMode::from(ZdrMode::Off),
            sovereign_required: ctx.sovereign,
            top_n: n,
            deadline_ms: ctx
                .deadline
                .as_millis()
                .clamp(DEADLINE_MIN_MS, DEADLINE_MAX_MS) as u64,
            documents: head
                .iter()
                .enumerate()
                .map(|(index, result)| WireDocument {
                    id: index.to_string(),
                    text: candidate_text(result, index),
                })
                .collect(),
        }
    }
}

/// The `rerank-v1` answer, read leniently for additive fields and strictly for the fields Quarry
/// uses. A missing one is a contract break, not an empty answer.
#[derive(Deserialize)]
struct WireAnswer {
    request_id: String,
    status: String,
    #[serde(default)]
    reason: Option<String>,
    profile: String,
    results: Vec<WireEntry>,
}

#[derive(Deserialize)]
struct WireEntry {
    index: usize,
    scored: bool,
    #[serde(default)]
    score: Option<f32>,
}

/// The candidate scores an answer may use: each index inside the `n` candidates, counted once (the
/// first valid entry wins), with a finite score clamped to `[0, 1]`. Unscored entries, out-of-range
/// indexes, repeats and non-finite scores are ignored.
fn valid_scores(entries: &[WireEntry], n: usize) -> Vec<(usize, f32)> {
    let mut seen = vec![false; n];
    let mut scored = Vec::with_capacity(entries.len().min(n));
    for entry in entries {
        if !entry.scored || entry.index >= n || seen[entry.index] {
            continue;
        }
        let Some(score) = entry.score.filter(|score| score.is_finite()) else {
            continue;
        };
        seen[entry.index] = true;
        scored.push((entry.index, score.clamp(0.0, 1.0)));
    }
    scored
}

/// Pure reorder: the head `[..n]` of `results` sorted by descending score, with equal scores keeping
/// the lower input index. Head results without a score trail in their original order, and the tail
/// `[n..]` is untouched. Ranks are renumbered 1-based. Scores are attached. Highlights are not
/// produced here (they are deferred, QB-T06).
fn apply_scores(
    mut results: Vec<SearchResult>,
    scored: &[(usize, f32)],
    n: usize,
) -> Vec<SearchResult> {
    let n = n.min(results.len());
    let tail = results.split_off(n);
    let mut slots: Vec<Option<SearchResult>> = results.into_iter().map(Some).collect();

    let mut order: Vec<(usize, f32)> = scored
        .iter()
        .copied()
        .filter(|(index, _)| *index < slots.len())
        .collect();
    order.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    let mut out: Vec<SearchResult> = Vec::with_capacity(slots.len() + tail.len());
    for (index, score) in order {
        if let Some(mut result) = slots[index].take() {
            result.score = Some(score);
            out.push(result);
        }
    }
    out.extend(slots.into_iter().flatten());
    out.extend(tail);
    for (position, result) in out.iter_mut().enumerate() {
        result.rank = position as u32 + 1;
    }
    out
}

/// Wraps an inner [`SearchProvider`] and reranks its top-N results. Transparent to callers and to the
/// edge cache: reranked results carry their score through serialization.
pub struct RerankingSearchProvider {
    inner: Arc<dyn SearchProvider>,
    reranker: Arc<dyn SearchReranker>,
    top_n: usize,
    deadline: Duration,
    sovereign: bool,
    shadow: bool,
}

impl RerankingSearchProvider {
    /// `top_n` is how many leading results to rerank. 0 disables reranking (a pass-through), which
    /// keeps the wrapper safe to construct unconditionally.
    pub fn new(
        inner: Arc<dyn SearchProvider>,
        reranker: Arc<dyn SearchReranker>,
        top_n: usize,
    ) -> Self {
        Self {
            inner,
            reranker,
            top_n,
            deadline: DEFAULT_RERANK_DEADLINE,
            sovereign: false,
            shadow: false,
        }
    }

    /// The budget of one call, sent to the Data Plane as its deadline.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The deployment is sovereign (`zero_saas_search`): no call leaves for the reranker.
    pub fn with_sovereign_posture(mut self, sovereign: bool) -> Self {
        self.sovereign = sovereign;
        self
    }

    /// Shadow mode: the reranker is asked and its answer measured, but the results keep their
    /// input order and carry no score.
    pub fn with_shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }
}

#[async_trait]
impl SearchProvider for RerankingSearchProvider {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        Ok(self.search_with_report(query, opts).await?.0)
    }

    async fn search_with_report(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> QuarryResult<(Vec<SearchResult>, RerankReport)> {
        let results = self.inner.search(query, opts).await?;
        let profile = self.reranker.profile();
        if self.top_n == 0 {
            return Ok((results, RerankReport::skipped("disabled", profile)));
        }
        // A zero-retention search keeps its query and result snippets out of the reranker: no
        // provider is attested for ZDR content, and the Data Plane is never asked for it.
        if opts.zdr {
            return Ok((results, RerankReport::skipped("zdr", profile)));
        }
        if self.sovereign {
            return Ok((results, RerankReport::skipped("sovereign", profile)));
        }
        let org = opts.org_id.as_deref().unwrap_or("");
        if !is_tenant_org(org) {
            return Ok((results, RerankReport::skipped("input_not_tenant", profile)));
        }
        if results.len() < 2 {
            return Ok((results, RerankReport::skipped("input_too_small", profile)));
        }

        let request_id = new_request_id();
        let ctx = RerankContext {
            org_id: org,
            zdr: false,
            sovereign: false,
            request_id: &request_id,
            deadline: self.deadline,
            top_n: self.top_n.min(MAX_RERANK_DOCUMENTS),
        };
        if self.shadow {
            let original = results.clone();
            let outcome = self.reranker.rerank(query, results, &ctx).await;
            return Ok((original, RerankReport::shadow_of(&outcome.report)));
        }
        let outcome = self.reranker.rerank(query, results, &ctx).await;
        Ok((outcome.results, outcome.report))
    }

    fn rerank_epoch(&self) -> RerankEpoch {
        RerankEpoch {
            profile: self.reranker.profile(),
            enabled: self.top_n > 0,
            shadow: self.shadow,
            top_n: self.top_n,
        }
    }

    fn name(&self) -> &str {
        "reranked"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(url: &str, rank: u32) -> SearchResult {
        SearchResult {
            url: url.into(),
            rank,
            provider: "test".into(),
            ..Default::default()
        }
    }

    fn entry(index: usize, score: Option<f32>) -> WireEntry {
        WireEntry {
            index,
            scored: score.is_some(),
            score,
        }
    }

    /// Inner provider that returns a fixed list, for the wrapper tests.
    struct FixedInner(Vec<SearchResult>);

    #[async_trait]
    impl SearchProvider for FixedInner {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.0.clone())
        }
        fn name(&self) -> &str {
            "fixed"
        }
    }

    /// Reranker that hands back a canned outcome and a canned profile.
    struct CannedReranker {
        outcome: RerankOutcome,
        profile: Option<String>,
    }

    #[async_trait]
    impl SearchReranker for CannedReranker {
        async fn rerank(
            &self,
            _q: &str,
            _r: Vec<SearchResult>,
            _ctx: &RerankContext<'_>,
        ) -> RerankOutcome {
            self.outcome.clone()
        }
        fn profile(&self) -> Option<String> {
            self.profile.clone()
        }
    }

    /// Reranker that must never run: the wrapper should not ask it.
    struct MustNotRun;

    #[async_trait]
    impl SearchReranker for MustNotRun {
        async fn rerank(
            &self,
            _q: &str,
            _r: Vec<SearchResult>,
            _ctx: &RerankContext<'_>,
        ) -> RerankOutcome {
            panic!("the reranker must not be called for this search");
        }
    }

    fn ctx<'a>(org: &'a str, request_id: &'a str) -> RerankContext<'a> {
        RerankContext {
            org_id: org,
            zdr: false,
            sovereign: false,
            request_id,
            deadline: DEFAULT_RERANK_DEADLINE,
            top_n: 10,
        }
    }

    /// Search options of a tenant. A search with no org is not a tenant, so the wrapper skips it with
    /// `input_not_tenant` before any size or shadow rule is read; the tests that exercise those rules
    /// must therefore search as a tenant.
    fn tenant_search() -> SearchOptions {
        SearchOptions {
            org_id: Some("org-acme".to_owned()),
            ..SearchOptions::default()
        }
    }

    #[test]
    fn apply_scores_orders_the_head_by_score_and_keeps_the_tail() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3), sr("d", 4)];
        let out = apply_scores(results, &[(1, 0.9), (0, 0.2)], 2);
        assert_eq!(out[0].url, "b");
        assert_eq!(out[0].score, Some(0.9));
        assert_eq!(out[1].url, "a");
        assert_eq!(out[1].score, Some(0.2));
        assert_eq!(out[2].url, "c");
        assert_eq!(out[3].url, "d");
        assert!(out[2].score.is_none(), "the tail carries no score");
        let ranks: Vec<u32> = out.iter().map(|r| r.rank).collect();
        assert_eq!(ranks, vec![1, 2, 3, 4]);
    }

    #[test]
    fn apply_scores_trails_unscored_head_results_in_input_order() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3)];
        let out = apply_scores(results, &[(2, 0.8)], 3);
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["c", "a", "b"]);
    }

    #[test]
    fn equal_scores_keep_the_lower_input_index_first() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3)];
        let out = apply_scores(results, &[(2, 0.5), (1, 0.5)], 3);
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["b", "c", "a"]);
    }

    #[test]
    fn valid_scores_ignores_out_of_range_repeated_unscored_and_non_finite_entries() {
        let entries = vec![
            entry(9, Some(1.0)),
            entry(1, Some(0.7)),
            entry(1, Some(0.1)),
            entry(0, Some(f32::NAN)),
            entry(2, None),
            WireEntry {
                index: 0,
                scored: false,
                score: Some(0.9),
            },
        ];
        assert_eq!(valid_scores(&entries, 3), vec![(1, 0.7)]);
    }

    #[test]
    fn valid_scores_clamps_into_the_unit_interval() {
        let entries = vec![entry(0, Some(1.4)), entry(1, Some(-0.2))];
        assert_eq!(valid_scores(&entries, 2), vec![(0, 1.0), (1, 0.0)]);
    }

    #[test]
    fn candidate_text_is_never_empty_and_falls_back_to_the_address() {
        let titled = SearchResult {
            url: "https://x.test/a".into(),
            title: Some(" Pricing ".into()),
            snippet: Some("per cleaning cycle".into()),
            ..Default::default()
        };
        assert_eq!(candidate_text(&titled, 0), "Pricing\nper cleaning cycle");
        let bare = SearchResult {
            url: " https://x.test/b ".into(),
            ..Default::default()
        };
        assert_eq!(candidate_text(&bare, 1), "https://x.test/b");
        let nothing = SearchResult::default();
        assert_eq!(candidate_text(&nothing, 2), "result 2");
    }

    #[test]
    fn the_query_is_trimmed_and_capped_at_the_data_plane_limit() {
        assert_eq!(clamp_query("  solar panels  "), "solar panels");
        let long: String = "é".repeat(MAX_RERANK_QUERY_CHARS + 5);
        assert_eq!(clamp_query(&long).chars().count(), MAX_RERANK_QUERY_CHARS);
        assert_eq!(clamp_query("   "), "");
    }

    #[test]
    fn a_request_id_is_one_the_data_plane_accepts() {
        let id = new_request_id();
        assert!(id.starts_with("qrr-"));
        assert!((8..=64).contains(&id.chars().count()), "{id}");
        assert!(id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-')));
        assert_ne!(new_request_id(), id, "each call gets its own id");
    }

    #[test]
    fn only_tenant_orgs_may_be_reranked() {
        assert!(is_tenant_org("org-acme"));
        assert!(!is_tenant_org("onboarding"));
        assert!(!is_tenant_org("global"));
        assert!(!is_tenant_org("  "));
        assert!(!is_tenant_org(""));
    }

    #[test]
    fn the_request_carries_exactly_the_data_plane_fields() {
        let head = vec![sr("https://x.test/a", 1), sr("https://x.test/b", 2)];
        let request_id = "qrr-0123456789";
        let body = WireRequest::new(&ctx("org-acme", request_id), "solar", 2, &head);
        let value = serde_json::to_value(&body).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "deadline_ms",
                "documents",
                "profile",
                "query",
                "request_id",
                "sovereign_required",
                "top_n",
                "zdr_mode",
            ]
        );
        assert_eq!(value["zdr_mode"], "disabled");
        assert_eq!(value["sovereign_required"], false);
        assert_eq!(value["profile"], "web-v1");
        assert_eq!(value["deadline_ms"], 2000);
        assert_eq!(value["documents"][1]["id"], "1");
    }

    #[test]
    fn a_skip_or_degrade_reason_from_the_data_plane_is_mapped_or_marked_unknown() {
        assert_eq!(
            dp_skip_reason(Some("processor_unapproved")),
            "processor_unapproved"
        );
        assert_eq!(dp_skip_reason(Some("made_up")), "unknown");
        assert_eq!(dp_skip_reason(None), "unknown");
        assert_eq!(dp_degrade_reason(Some("timeout")), "timeout");
        assert_eq!(
            dp_degrade_reason(Some("provider_rate_limited")),
            "provider_rate_limited"
        );
        assert_eq!(dp_degrade_reason(Some("anything else")), "provider_error");
    }

    #[test]
    fn the_breaker_opens_after_five_failures_and_admits_one_probe_after_its_window() {
        let breaker = RerankBreaker::default();
        let t0 = Instant::now();
        for _ in 0..(BREAKER_FAILURE_THRESHOLD - 1) {
            breaker.failed(t0);
            assert!(breaker.admit(t0), "below the threshold calls still go out");
        }
        breaker.failed(t0);
        assert!(!breaker.admit(t0), "the fifth failure opens the breaker");
        assert!(!breaker.admit(t0 + BREAKER_OPEN_FOR - Duration::from_millis(1)));
        let probe_at = t0 + BREAKER_OPEN_FOR;
        assert!(breaker.admit(probe_at), "one probe after the open period");
        assert!(
            !breaker.admit(probe_at),
            "no second call while the probe is out"
        );
        breaker.succeeded();
        assert!(
            breaker.admit(probe_at),
            "a successful probe closes the breaker"
        );
    }

    #[test]
    fn a_failed_probe_reopens_the_breaker_and_a_neutral_one_releases_it() {
        let breaker = RerankBreaker::default();
        let t0 = Instant::now();
        for _ in 0..BREAKER_FAILURE_THRESHOLD {
            breaker.failed(t0);
        }
        let probe_at = t0 + BREAKER_OPEN_FOR;
        assert!(breaker.admit(probe_at));
        breaker.neutral();
        assert!(
            breaker.admit(probe_at),
            "a neutral probe is released, not counted"
        );
        breaker.failed(probe_at);
        assert!(!breaker.admit(probe_at + Duration::from_secs(1)));
    }

    #[test]
    fn a_cancelled_probe_is_released_for_the_next_call() {
        let breaker = RerankBreaker::default();
        let t0 = Instant::now();
        for _ in 0..BREAKER_FAILURE_THRESHOLD {
            breaker.failed(t0);
        }
        let probe_at = t0 + BREAKER_OPEN_FOR;
        assert!(breaker.admit(probe_at), "the probe is admitted");
        {
            // The call that took the probe is dropped before it settles.
            let _probe = ProbeGuard(&breaker);
        }
        assert!(
            breaker.admit(probe_at),
            "a cancelled probe must not hold the breaker for ever"
        );
    }

    #[test]
    fn a_token_refusal_takes_the_routes_error_line_for_the_window() {
        use quarry_core::error::{ErrorCode, QuarryError};

        // The route's ERROR window is shared by its token refusals and its own refusals.
        let http =
            DataPlaneHttp::new("http://127.0.0.1:9/v1/rerank", CLIENT_TIMEOUT).expect("a client");
        let request = ServiceTokenRequest::from_manifest("web-rerank").expect("the row exists");
        for _ in 0..3 {
            let returned =
                http.log_token_failure(QuarryError::new(ErrorCode::Forbidden, "refused"), &request);
            assert!(
                matches!(returned.code, ErrorCode::Forbidden),
                "the error is returned unchanged"
            );
        }
        assert!(
            !http.admit_error_log(),
            "the token refusals used this window's ERROR line"
        );
    }

    #[tokio::test]
    async fn disabled_wrapper_reports_skipped_and_never_calls_the_reranker() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(MustNotRun),
            0,
        );
        let (out, report) = p
            .search_with_report("q", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(out[0].url, "a");
        assert_eq!(report.status, RerankStatus::Skipped);
        assert_eq!(report.reason.as_deref(), Some("disabled"));
    }

    #[tokio::test]
    async fn a_zdr_search_is_never_sent_to_the_reranker() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(MustNotRun),
            10,
        );
        let opts = SearchOptions {
            zdr: true,
            ..SearchOptions::default()
        };
        let (out, report) = p.search_with_report("q", &opts).await.unwrap();
        assert_eq!(out[0].url, "a");
        assert_eq!(report.status, RerankStatus::Skipped);
        assert_eq!(report.reason.as_deref(), Some("zdr"));
    }

    #[tokio::test]
    async fn a_sovereign_deployment_never_calls_the_reranker() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(MustNotRun),
            10,
        )
        .with_sovereign_posture(true);
        let (_, report) = p
            .search_with_report("q", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(report.reason.as_deref(), Some("sovereign"));
    }

    #[tokio::test]
    async fn a_non_tenant_org_is_never_sent_to_the_reranker() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(MustNotRun),
            10,
        );
        for org in ["onboarding", ""] {
            let opts = SearchOptions {
                org_id: Some(org.to_owned()),
                ..SearchOptions::default()
            };
            let (out, report) = p.search_with_report("q", &opts).await.unwrap();
            assert_eq!(out[0].url, "a");
            assert_eq!(
                report.reason.as_deref(),
                Some("input_not_tenant"),
                "{org:?}"
            );
        }
    }

    #[tokio::test]
    async fn single_result_reports_input_too_small_without_calling_the_reranker() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1)])),
            Arc::new(MustNotRun),
            10,
        );
        let (out, report) = p.search_with_report("q", &tenant_search()).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(report.status, RerankStatus::Skipped);
        assert_eq!(report.reason.as_deref(), Some("input_too_small"));
    }

    #[tokio::test]
    async fn wrapper_hands_back_the_rerankers_order_and_report() {
        let applied = RerankOutcome {
            results: vec![sr("b", 1), sr("a", 2)],
            report: RerankReport::applied(Some("web-v1".into())),
        };
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(CannedReranker {
                outcome: applied,
                profile: Some("web-v1".into()),
            }),
            10,
        );
        let (out, report) = p.search_with_report("q", &tenant_search()).await.unwrap();
        assert_eq!(out[0].url, "b");
        assert_eq!(report, RerankReport::applied(Some("web-v1".into())));

        // A failed rerank keeps the input order and says that it degraded.
        let degraded = RerankOutcome {
            results: vec![sr("a", 1), sr("b", 2)],
            report: RerankReport::degraded("provider_error", Some("web-v1".into())),
        };
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(CannedReranker {
                outcome: degraded,
                profile: Some("web-v1".into()),
            }),
            10,
        );
        let (out, report) = p.search_with_report("q", &tenant_search()).await.unwrap();
        assert_eq!(out[0].url, "a");
        assert_eq!(report.status, RerankStatus::Degraded);
        assert_eq!(report.reason.as_deref(), Some("provider_error"));
    }

    #[tokio::test]
    async fn shadow_mode_keeps_the_input_order_and_carries_no_score() {
        let applied = RerankOutcome {
            results: vec![
                SearchResult {
                    score: Some(0.9),
                    ..sr("b", 1)
                },
                sr("a", 2),
            ],
            report: RerankReport::applied(Some("web-v1".into())),
        };
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(CannedReranker {
                outcome: applied,
                profile: Some("web-v1".into()),
            }),
            10,
        )
        .with_shadow(true);
        let (out, report) = p.search_with_report("q", &tenant_search()).await.unwrap();
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "b"]);
        assert!(out
            .iter()
            .all(|r| r.score.is_none() && r.highlights.is_empty()));
        assert_eq!(report.status, RerankStatus::Shadow);
        assert_eq!(report.reason.as_deref(), Some("would_apply"));
        assert!(p.rerank_epoch().shadow);
    }

    #[tokio::test]
    async fn plain_search_returns_the_same_results_as_search_with_report() {
        let p = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![sr("a", 1), sr("b", 2)])),
            Arc::new(CannedReranker {
                outcome: RerankOutcome {
                    results: vec![sr("b", 1), sr("a", 2)],
                    report: RerankReport::applied(None),
                },
                profile: None,
            }),
            10,
        );
        let plain = p.search("q", &tenant_search()).await.unwrap();
        let (with_report, report) = p.search_with_report("q", &tenant_search()).await.unwrap();
        let urls = |v: &[SearchResult]| v.iter().map(|r| r.url.clone()).collect::<Vec<_>>();
        assert_eq!(urls(plain.as_slice()), urls(with_report.as_slice()));
        assert_eq!(
            urls(plain.as_slice()),
            vec!["b".to_owned(), "a".to_owned()],
            "the reranked order is what the plain search returns"
        );
        assert_eq!(report.status, RerankStatus::Applied);
    }

    #[test]
    fn wrapper_reports_the_epoch_it_searches_under() {
        let canned = |profile: Option<&str>| CannedReranker {
            outcome: RerankOutcome {
                results: vec![],
                report: RerankReport::default(),
            },
            profile: profile.map(str::to_string),
        };
        let on = RerankingSearchProvider::new(
            Arc::new(FixedInner(vec![])),
            Arc::new(canned(Some("web-v1"))),
            10,
        );
        assert_eq!(
            on.rerank_epoch(),
            RerankEpoch {
                profile: Some("web-v1".into()),
                enabled: true,
                shadow: false,
                top_n: 10,
            }
        );
        let off =
            RerankingSearchProvider::new(Arc::new(FixedInner(vec![])), Arc::new(canned(None)), 0);
        assert_eq!(off.rerank_epoch(), RerankEpoch::default());
    }

    #[test]
    fn only_applied_and_not_configured_are_cacheable() {
        assert!(RerankStatus::Applied.is_cacheable());
        assert!(RerankStatus::RerankNotConfigured.is_cacheable());
        assert!(!RerankStatus::Skipped.is_cacheable());
        assert!(!RerankStatus::Degraded.is_cacheable());
        assert!(!RerankStatus::Shadow.is_cacheable());
    }

    #[test]
    fn rerank_report_wire_shape_is_stable() {
        assert_eq!(
            serde_json::to_value(RerankReport::default()).unwrap(),
            serde_json::json!({ "status": "rerank-not-configured" })
        );
        assert_eq!(
            serde_json::to_value(RerankReport::degraded(
                "provider_error",
                Some("web-v1".into())
            ))
            .unwrap(),
            serde_json::json!({
                "status": "degraded",
                "reason": "provider_error",
                "profile": "web-v1"
            })
        );
        assert_eq!(
            serde_json::to_value(RerankReport::shadow_of(&RerankReport::applied(None))).unwrap(),
            serde_json::json!({ "status": "shadow", "reason": "would_apply" })
        );
    }
}
