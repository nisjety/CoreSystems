//! Vector retrieval for hybrid search and find-similar.
//!
//! Quarry is the evidence engine and does not own a vector store. Semantic
//! retrieval is delegated to the Data Plane's retrieval engine, which owns the
//! embeddings and the Qdrant index. `DataPlaneVectorIndex` calls its
//! `POST /v1/retrieve` (contract `retrieve-v1`, vendored under
//! `tests/fixtures/dp-contracts/retrieve-v1/`) and maps the answer to hits that
//! carry a real page address. `hybrid::HybridSearchProvider` fuses those hits
//! with Quarry's lexical recall by rank.
//!
//! Posture is decided here, before anything is sent. A zero-retention run and a
//! sovereign deployment skip the leg with zero requests, and so does an org that
//! has spent its hourly budget. A skip is an outcome, not an error.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;

use crate::data_plane_http::{DataPlaneCallError, DataPlaneHttp};
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

/// The Data Plane's retrieve route, appended to its base URL.
const RETRIEVE_PATH: &str = "/v1/retrieve";

/// The corpus Quarry reads: `documents.source = 'quarry'` on the Data Plane. It is
/// sent as a hint; the Data Plane forces it for the quarry-edge principal.
const QUARRY_SOURCE: &str = "quarry";

/// The longest a vector call may take, token exchange included. Hybrid search
/// bounds the leg with the same figure, so lexical results are never held back.
pub const VECTOR_LEG_TIMEOUT: Duration = Duration::from_millis(1500);

/// Vector calls one org may spend per hour when the operator sets no budget.
pub const DEFAULT_PER_ORG_HOURLY_BUDGET: u32 = 600;

/// The length of one budget window.
const BUDGET_WINDOW: Duration = Duration::from_secs(3600);

/// Above this many tracked orgs, expired budget windows are dropped.
const BUDGET_PRUNE_AT: usize = 4096;

/// A semantic-retrieval hit, normalized for RRF fusion with lexical results. The
/// Data Plane's returned order is the only rank Quarry uses.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    /// The page address: the Data Plane's `sources[].url` for a quarry document.
    pub url: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub document_id: Option<String>,
}

/// Why the vector leg did not run. Each reason has a stable marker for logs and
/// metrics. None is an error: the search continues lexical-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The run is zero-retention, so the query must not reach an embedding call.
    Zdr,
    /// The deployment is sovereign (`zero_saas_search`): no external embedding call.
    Sovereign,
    /// The org has spent its hourly budget of vector calls.
    Budget,
    /// There is no tenant to scope the retrieval to, so nothing is sent.
    NoOrg,
}

impl SkipReason {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Zdr => "skipped_zdr",
            Self::Sovereign => "skipped_sovereign",
            Self::Budget => "skipped_budget",
            Self::NoOrg => "skipped_no_org",
        }
    }
}

/// What one vector-leg call produced.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorOutcome {
    Hits(Vec<VectorHit>),
    Skipped(SkipReason),
}

/// One vector-leg call: the tenant, the query text, the ZDR posture of the run
/// and the number of hits wanted.
#[derive(Debug, Clone, Copy)]
pub struct VectorQuery<'a> {
    pub org_id: &'a str,
    pub query: &'a str,
    pub zdr: ZdrMode,
    pub limit: usize,
}

/// Semantic retrieval backend. Org-scoped: implementations MUST restrict hits to
/// `query.org_id`.
#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn retrieve(&self, query: &VectorQuery<'_>) -> QuarryResult<VectorOutcome>;
}

/// The Data Plane's `zdr_mode` values (`retrieve-v1`, `zdr-modes.golden.json`).
/// Quarry's own `off` has no counterpart and never reaches the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataPlaneZdrMode {
    Disabled,
    Reject,
    Ephemeral,
}

impl DataPlaneZdrMode {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Reject => "reject",
            Self::Ephemeral => "ephemeral",
        }
    }
}

impl From<ZdrMode> for DataPlaneZdrMode {
    fn from(zdr: ZdrMode) -> Self {
        match zdr {
            ZdrMode::Off => Self::Disabled,
            // The mode the Data Plane gives any verified zero-retention claim.
            ZdrMode::On => Self::Ephemeral,
        }
    }
}

/// The `retrieve-v1` request Quarry sends. It matches the vendored request golden
/// field for field: no `top_k`, no literal `off`, and `sovereign_required` is
/// always sent explicitly.
#[derive(Debug, Serialize, PartialEq)]
struct RetrieveRequestBody<'a> {
    org_id: &'a str,
    query: &'a str,
    top_n: usize,
    zdr_mode: DataPlaneZdrMode,
    sovereign_required: bool,
    filters: RetrieveFilters,
    mode_mix: RetrieveModeMix,
}

#[derive(Debug, Serialize, PartialEq)]
struct RetrieveFilters {
    sources: Vec<&'static str>,
}

#[derive(Debug, Serialize, PartialEq)]
struct RetrieveModeMix {
    rerank: bool,
}

impl<'a> RetrieveRequestBody<'a> {
    fn for_query(query: &VectorQuery<'a>, sovereign: bool) -> Self {
        Self {
            org_id: query.org_id,
            query: query.query,
            top_n: query.limit,
            zdr_mode: DataPlaneZdrMode::from(query.zdr),
            sovereign_required: sovereign,
            filters: RetrieveFilters {
                sources: vec![QUARRY_SOURCE],
            },
            mode_mix: RetrieveModeMix { rerank: false },
        }
    }
}

/// The `retrieve-v1` response, read strictly. The fields Quarry uses are required:
/// a missing one is a contract break, not an empty answer. Additive fields are
/// ignored. Candidates carry no page address and no score that Quarry ranks by.
/// The address comes from `sources`, and rank is position.
#[derive(Debug, Deserialize)]
struct RetrieveResponseBody {
    candidates: Vec<WireCandidate>,
    sources: Vec<WireSource>,
}

#[derive(Debug, Deserialize)]
struct WireCandidate {
    document_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct WireSource {
    document_id: String,
    title: String,
    source: String,
    /// Present only for a quarry document whose stored address is a page URL.
    #[serde(default)]
    url: Option<String>,
}

/// Map a `retrieve-v1` answer to hits, in returned order. A candidate is kept only
/// when its document is a quarry document with a page address. Otherwise it is
/// dropped, never guessed at. A page with several chunks counts once, at its best
/// (first) chunk, so RRF does not reward the number of chunks.
fn map_response(body: RetrieveResponseBody, limit: usize) -> Vec<VectorHit> {
    let mut sources: HashMap<&str, &WireSource> = HashMap::new();
    for source in &body.sources {
        sources.entry(source.document_id.as_str()).or_insert(source);
    }
    let mut seen_urls: HashSet<&str> = HashSet::new();
    let mut hits = Vec::new();
    for candidate in &body.candidates {
        if hits.len() >= limit {
            break;
        }
        let Some(source) = sources.get(candidate.document_id.as_str()) else {
            continue;
        };
        if source.source != QUARRY_SOURCE {
            continue;
        }
        let Some(url) = source.url.as_deref().filter(|url| is_page_url(url)) else {
            continue;
        };
        if !seen_urls.insert(url) {
            continue;
        }
        hits.push(VectorHit {
            url: url.to_owned(),
            title: non_empty(&source.title),
            snippet: non_empty(&candidate.text),
            document_id: Some(candidate.document_id.clone()),
        });
    }
    hits
}

/// A page address Quarry will show: http or https, with no whitespace or control
/// characters in it.
fn is_page_url(url: &str) -> bool {
    let scheme_ok = url
        .get(..8)
        .is_some_and(|head| head.eq_ignore_ascii_case("https://"))
        || url
            .get(..7)
            .is_some_and(|head| head.eq_ignore_ascii_case("http://"));
    scheme_ok && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Vector calls per org per hour, in fixed windows. Each call is one paid query
/// embedding, and the usage ledger does not meter it yet (DEF-QB-VECTOR-METERING),
/// so this is the cap. It lives in each process, so every replica keeps its own
/// count.
#[derive(Debug)]
pub struct VectorBudget {
    per_org_per_hour: u32,
    windows: Mutex<HashMap<String, BudgetWindow>>,
}

#[derive(Debug, Clone, Copy)]
struct BudgetWindow {
    started: Instant,
    spent: u32,
}

impl VectorBudget {
    pub fn new(per_org_per_hour: u32) -> Self {
        Self {
            per_org_per_hour,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Spend one call for `org_id` if its window has room.
    pub fn try_spend(&self, org_id: &str) -> bool {
        self.try_spend_at(org_id, Instant::now())
    }

    fn try_spend_at(&self, org_id: &str, now: Instant) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if windows.len() >= BUDGET_PRUNE_AT {
            windows.retain(|_, window| now.duration_since(window.started) < BUDGET_WINDOW);
        }
        let window = windows.entry(org_id.to_owned()).or_insert(BudgetWindow {
            started: now,
            spent: 0,
        });
        if now.duration_since(window.started) >= BUDGET_WINDOW {
            *window = BudgetWindow {
                started: now,
                spent: 0,
            };
        }
        if window.spent < self.per_org_per_hour {
            window.spent += 1;
            true
        } else {
            false
        }
    }
}

/// Calls the Data Plane's `retrieve-v1` route over HTTP and JSON.
pub struct DataPlaneVectorIndex {
    /// The retrieve route, its client and its credential (`data_plane_http`).
    http: DataPlaneHttp,
    /// The deployment's sovereignty posture (`zero_saas_search`). When set, no call
    /// is made.
    sovereign: bool,
    budget: VectorBudget,
}

impl DataPlaneVectorIndex {
    /// `base_url` is the Data Plane root (e.g. `http://dpv2-retrieval-engine:8080`).
    /// The retrieve path is appended.
    pub fn new(base_url: impl AsRef<str>) -> QuarryResult<Self> {
        let base = base_url.as_ref().trim_end_matches('/');
        Ok(Self {
            http: DataPlaneHttp::new(format!("{base}{RETRIEVE_PATH}"), VECTOR_LEG_TIMEOUT)?,
            sovereign: false,
            budget: VectorBudget::new(DEFAULT_PER_ORG_HOURLY_BUDGET),
        })
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.http = self.http.with_api_key(key);
        self
    }

    pub fn with_token_provider(mut self, provider: SharedServiceTokenProvider) -> Self {
        self.http = self.http.with_token_provider(provider);
        self
    }

    /// The deployment is sovereign (`zero_saas_search`): every call is skipped.
    pub fn with_sovereign_posture(mut self, sovereign: bool) -> Self {
        self.sovereign = sovereign;
        self
    }

    /// Vector calls one org may spend per hour.
    pub fn with_hourly_budget(mut self, per_org_per_hour: u32) -> Self {
        self.budget = VectorBudget::new(per_org_per_hour);
        self
    }

    /// The most one call may take, the token exchange included. Production keeps
    /// [`VECTOR_LEG_TIMEOUT`].
    pub fn with_timeout(mut self, timeout: Duration) -> QuarryResult<Self> {
        self.http = self.http.with_timeout(timeout)?;
        Ok(self)
    }

    /// One exchange with the Data Plane: the token, the request and the reading of
    /// the answer. Failures are logged here at the level their class deserves. An
    /// auth or contract break is an error, and an outage is a warning.
    async fn send(
        &self,
        query: &VectorQuery<'_>,
        body: &RetrieveRequestBody<'_>,
    ) -> QuarryResult<Vec<VectorHit>> {
        let token_request = ServiceTokenRequest::from_manifest("vector-retrieve")?;
        let response = match self
            .http
            .post_json(&token_request, query.org_id, body)
            .await
        {
            Ok(response) => response,
            // Auth Core's refusal or outage is already logged by its class.
            Err(DataPlaneCallError::Token(error)) => return Err(error),
            Err(DataPlaneCallError::Timeout(error)) => {
                // zdr-log-ok: retrieve() skips a ZDR query before send(); the reqwest text names only the operator-configured Data Plane URL (the query travels in the POST body)
                tracing::warn!(
                    reason = "dp_timeout",
                    error = %error,
                    "vector retrieval: the Data Plane did not answer in time"
                );
                return Err(QuarryError::new(
                    ErrorCode::Timeout,
                    // The address and the transport detail stay in the log above, not in
                    // the error a caller (find-similar's HTTP body) returns.
                    "retrieve request timed out",
                ));
            }
            Err(DataPlaneCallError::Unreachable(error)) => {
                // zdr-log-ok: retrieve() skips a ZDR query before send(); the reqwest text names only the operator-configured Data Plane URL (the query travels in the POST body)
                tracing::warn!(
                    reason = "dp_unreachable",
                    error = %error,
                    "vector retrieval: the Data Plane could not be reached"
                );
                return Err(QuarryError::new(
                    ErrorCode::UpstreamBlocked,
                    "retrieve request failed",
                ));
            }
        };
        let status = response.status();
        if status.is_success() {
            return match response.json::<RetrieveResponseBody>().await {
                Ok(parsed) => Ok(map_response(parsed, query.limit)),
                Err(error) if error.is_decode() => {
                    // zdr-log-ok: retrieve() skips a ZDR query before send(); a decode error names a field or a position and the operator-configured Data Plane URL
                    tracing::error!(
                        reason = "dp_contract_decode",
                        error = %error,
                        "vector retrieval: the answer does not match retrieve-v1; this call yields no vector hits"
                    );
                    Err(QuarryError::new(
                        ErrorCode::UpstreamBlocked,
                        "retrieve answer does not match retrieve-v1",
                    ))
                }
                Err(error) => {
                    // zdr-log-ok: retrieve() skips a ZDR query before send(); the reqwest text names only the operator-configured Data Plane URL
                    tracing::warn!(
                        reason = "dp_unreachable",
                        error = %error,
                        "vector retrieval: the answer was cut off"
                    );
                    Err(QuarryError::new(
                        ErrorCode::UpstreamBlocked,
                        "retrieve answer was cut off",
                    ))
                }
            };
        }
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                tracing::error!(
                    reason = "dp_auth_refused",
                    status = status.as_u16(),
                    "vector retrieval: the Data Plane refused the retrieval credential; check the service principal and its scope"
                );
                Err(QuarryError::new(
                    ErrorCode::UpstreamBlocked,
                    format!("retrieve refused the credential ({status})"),
                ))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                tracing::warn!(
                    reason = "dp_rate_limited",
                    "vector retrieval: the Data Plane rate-limited the call"
                );
                Err(QuarryError::new(
                    ErrorCode::RateLimited,
                    "retrieve was rate limited",
                ))
            }
            status if status.is_server_error() => {
                tracing::warn!(
                    reason = "dp_unavailable",
                    status = status.as_u16(),
                    "vector retrieval: the Data Plane is unavailable"
                );
                Err(QuarryError::new(
                    ErrorCode::UpstreamBlocked,
                    format!("retrieve returned status {status}"),
                ))
            }
            status => {
                tracing::error!(
                    reason = "dp_contract_rejected",
                    status = status.as_u16(),
                    "vector retrieval: the Data Plane rejected the request; it no longer matches retrieve-v1"
                );
                Err(QuarryError::new(
                    ErrorCode::UpstreamBlocked,
                    format!("retrieve rejected the request ({status})"),
                ))
            }
        }
    }
}

#[async_trait]
impl VectorIndex for DataPlaneVectorIndex {
    async fn retrieve(&self, query: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
        // Posture first. A request that may not leave is never sent, and it spends
        // no budget.
        if query.zdr.is_active() {
            return Ok(VectorOutcome::Skipped(SkipReason::Zdr));
        }
        if self.sovereign {
            return Ok(VectorOutcome::Skipped(SkipReason::Sovereign));
        }
        if query.org_id.trim().is_empty() {
            return Ok(VectorOutcome::Skipped(SkipReason::NoOrg));
        }
        if !self.budget.try_spend(query.org_id) {
            tracing::warn!(
                "vector retrieval: this org has spent its hourly budget; the leg is skipped until the window resets"
            );
            return Ok(VectorOutcome::Skipped(SkipReason::Budget));
        }
        let body = RetrieveRequestBody::for_query(query, self.sovereign);
        match tokio::time::timeout(self.http.timeout(), self.send(query, &body)).await {
            Ok(hits) => hits.map(VectorOutcome::Hits),
            Err(_elapsed) => {
                tracing::warn!(
                    reason = "dp_timeout",
                    deadline = ?self.http.timeout(),
                    "vector retrieval: no answer inside its deadline"
                );
                Err(QuarryError::new(
                    ErrorCode::Timeout,
                    "vector retrieval exceeded its deadline",
                ))
            }
        }
    }
}

/// No-op backend for tests and for deployments without a Data Plane.
pub struct NoopVectorIndex;

#[async_trait]
impl VectorIndex for NoopVectorIndex {
    async fn retrieve(&self, _query: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
        Ok(VectorOutcome::Hits(Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_GOLDEN: &str =
        include_str!("../tests/fixtures/dp-contracts/retrieve-v1/request.golden.json");
    const RESPONSE_GOLDEN: &str =
        include_str!("../tests/fixtures/dp-contracts/retrieve-v1/response.golden.json");

    fn golden_query() -> VectorQuery<'static> {
        VectorQuery {
            org_id: "org-golden",
            query: "how often is the conveyor belt cleaned",
            zdr: ZdrMode::Off,
            limit: 10,
        }
    }

    #[test]
    fn the_request_is_the_vendored_request_golden_field_for_field() {
        let body = RetrieveRequestBody::for_query(&golden_query(), false);
        let sent = serde_json::to_value(&body).unwrap();
        let golden: serde_json::Value = serde_json::from_str(REQUEST_GOLDEN).unwrap();
        assert_eq!(sent, golden);
    }

    #[test]
    fn zero_retention_maps_to_ephemeral_and_the_literal_off_is_never_produced() {
        assert_eq!(DataPlaneZdrMode::from(ZdrMode::Off).as_wire(), "disabled");
        assert_eq!(DataPlaneZdrMode::from(ZdrMode::On).as_wire(), "ephemeral");
    }

    #[test]
    fn the_response_golden_maps_to_the_one_quarry_page() {
        let body: RetrieveResponseBody = serde_json::from_str(RESPONSE_GOLDEN).unwrap();
        assert_eq!(
            map_response(body, 10),
            vec![VectorHit {
                url: "https://docs.example.test/pricing?plan=pro".into(),
                title: Some("Pricing".into()),
                snippet: Some("The plan costs a fixed amount per cleaning cycle.".into()),
                document_id: Some("doc-quarry-pricing".into()),
            }]
        );
    }

    #[test]
    fn a_candidate_whose_document_has_no_page_address_is_dropped() {
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"t"}],
                "sources":[{"document_id":"d1","title":"T","source":"quarry"}]}"#,
        )
        .unwrap();
        assert!(map_response(body, 10).is_empty());
    }

    #[test]
    fn invented_aliases_are_not_read_as_page_addresses() {
        // The tolerant mirror this replaces read `source_url`, `uri` and `score`.
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"t","score":0.9,"source_url":"https://x.test/a"}],
                "sources":[{"document_id":"d1","title":"T","source":"quarry","uri":"https://x.test/a"}]}"#,
        )
        .unwrap();
        assert!(map_response(body, 10).is_empty());
    }

    #[test]
    fn a_source_other_than_quarry_is_dropped_even_with_an_address() {
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"t"}],
                "sources":[{"document_id":"d1","title":"T","source":"sharepoint","url":"https://x.test/a"}]}"#,
        )
        .unwrap();
        assert!(map_response(body, 10).is_empty());
    }

    #[test]
    fn a_non_http_address_is_dropped() {
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"t"}],
                "sources":[{"document_id":"d1","title":"T","source":"quarry","url":"javascript:alert(1)"}]}"#,
        )
        .unwrap();
        assert!(map_response(body, 10).is_empty());
    }

    #[test]
    fn a_page_with_several_chunks_counts_once_at_its_best_chunk() {
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"best"},{"document_id":"d1","text":"second"}],
                "sources":[{"document_id":"d1","title":"T","source":"quarry","url":"https://x.test/p"}]}"#,
        )
        .unwrap();
        let hits = map_response(body, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet.as_deref(), Some("best"));
    }

    #[test]
    fn the_limit_caps_the_hits() {
        let body: RetrieveResponseBody = serde_json::from_str(
            r#"{"candidates":[{"document_id":"d1","text":"a"},{"document_id":"d2","text":"b"}],
                "sources":[{"document_id":"d1","title":"A","source":"quarry","url":"https://x.test/1"},
                           {"document_id":"d2","title":"B","source":"quarry","url":"https://x.test/2"}]}"#,
        )
        .unwrap();
        assert_eq!(map_response(body, 1).len(), 1);
    }

    #[test]
    fn a_contract_break_is_a_decode_error_and_not_an_empty_answer() {
        let broken = r#"{"candidates":[{"text":"t"}],"sources":[]}"#;
        assert!(serde_json::from_str::<RetrieveResponseBody>(broken).is_err());
    }

    #[test]
    fn the_hourly_budget_is_per_org_and_resets_with_the_window() {
        let budget = VectorBudget::new(2);
        let t0 = Instant::now();
        assert!(budget.try_spend_at("org_a", t0));
        assert!(budget.try_spend_at("org_a", t0));
        assert!(!budget.try_spend_at("org_a", t0));
        assert!(
            budget.try_spend_at("org_b", t0),
            "another org has its own window"
        );
        assert!(budget.try_spend_at("org_a", t0 + BUDGET_WINDOW));
    }

    #[test]
    fn a_zero_budget_spends_nothing() {
        let budget = VectorBudget::new(0);
        assert!(!budget.try_spend_at("org_a", Instant::now()));
    }

    #[test]
    fn the_skip_markers_are_stable() {
        assert_eq!(SkipReason::Zdr.marker(), "skipped_zdr");
        assert_eq!(SkipReason::Sovereign.marker(), "skipped_sovereign");
        assert_eq!(SkipReason::Budget.marker(), "skipped_budget");
        assert_eq!(SkipReason::NoOrg.marker(), "skipped_no_org");
    }
}
