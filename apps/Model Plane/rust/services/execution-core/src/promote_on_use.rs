//! Phase 8 — promote-on-use. The promote PRIMITIVE already exists (quarry
//! `/v1/scrape|/v1/crawl|/v1/batch` with `ingest:true`); this is the TRIGGER:
//! count grounded `web_fetch` uses per (org, user, url) and, past a threshold,
//! promote the live page into the durable knowledge base so the agent stops
//! re-fetching content it keeps relying on.
//!
//! SAFE BY DEFAULT — two-key opt-in, dry-run-first (mirrors the Phase 7
//! retention janitor). With no config it is a complete no-op:
//!   - `PROMOTE_ON_USE_ENABLED=true`  → start counting + LOG promote candidates
//!     (dry-run; performs no ingest).
//!   - `PROMOTE_ON_USE_EXECUTE=true`  → additionally perform the real ingest.
//!   - `PROMOTE_ON_USE_THRESHOLD`     → grounded-use count to fire at (default 3).
//!
//! This is a read-path side effect (the plan's flagged-riskiest element), so it
//! defaults OFF and, when enabled, defaults to dry-run. Two REFINEMENTS are
//! required before arming EXECUTE in production:
//!   1. Ownership — the Edge `/v1/scrape` derives owner from the JWT
//!      `claims.user_id`; execution-core holds only a service token, so today an
//!      executed promote would attribute the doc to the service principal, not
//!      the agent's user. A per-user (data-plane/quarry) token must be minted so
//!      the promoted doc lands owner=user, visibility=private. (The request body
//!      now states `visibility: private` and an untrusted provenance marker, but
//!      the per-user token is still an arming prerequisite.)
//!   2. Signal — "grounded use" defaults to a successful `web_fetch`
//!      (`PROMOTE_ON_USE_SIGNAL=fetch`). `PROMOTE_ON_USE_SIGNAL=cited` counts a
//!      page only when its URL appears in the run's final answer: a fetch just
//!      records a pending candidate for the run, and the run's end
//!      ([`settle_run`]) counts the candidates the answer cites.
//!
//! The counter is process-local (in-memory) by default. With
//! `PROMOTE_ON_USE_DURABLE=true` it lives in session-core instead
//! (`PromoteOnUseService.IncrementPromoteUse`), per org, user and SHA-256 of the
//! normalised URL, so a restart does not reset it and the URL itself is never
//! stored. That path FAILS CLOSED: with no session credential or on any RPC
//! error the use is not counted and nothing is promoted.
//!
//! Content gate: a ZDR run, a retention-waived run and a Space-scoped run never
//! count and never promote (see [`PromoteContext`]). A promoted page is a
//! private, untrusted-provenance document.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{
    promote_on_use_service_client::PromoteOnUseServiceClient, IncrementPromoteUseRequest,
};
use sha2::{Digest, Sha256};
use tonic::transport::Channel;

/// How long one durable count may take before the use is treated as not counted.
const DURABLE_COUNT_TIMEOUT: Duration = Duration::from_secs(5);

/// The session-core handle the durable counter needs: the run's channel and its verified
/// session credential. Built only when the durable counter is on ([`durable_session`]).
#[derive(Clone)]
pub struct DurableSession {
    channel: Channel,
    bearer: String,
}

/// The session-core handle for a durable count, or `None` when promote-on-use or its durable
/// counter is off, or the run holds no session channel or credential. A `None` under the durable
/// switch means the use is not counted (fail closed).
#[must_use]
pub fn durable_session(channel: Option<&Channel>, bearer: Option<&str>) -> Option<DurableSession> {
    let cfg = config();
    if !cfg.enabled || !cfg.durable {
        return None;
    }
    Some(DurableSession {
        channel: channel?.clone(),
        bearer: bearer.filter(|b| !b.is_empty())?.to_owned(),
    })
}

/// What the caller knows about the run a grounded use came from. Any one of the
/// first three makes the use ineligible: the page content must not be persisted
/// on behalf of such a run, and not even counted towards a later promote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PromoteContext {
    /// The run is zero-data-retention.
    pub zdr: bool,
    /// The run's retention was waived by the organisation.
    pub waived: bool,
    /// The run is scoped to a Space, whose content stays in the Space.
    pub space_scoped: bool,
    /// The page's URL is cited in the run's final answer. Only read when the
    /// signal is `cited`; the `fetch` signal ignores it.
    pub cited: bool,
}

impl PromoteContext {
    /// True when the run may not contribute to promote-on-use at all.
    #[must_use]
    pub fn blocked(&self) -> bool {
        self.zdr || self.waived || self.space_scoped
    }
}

struct Config {
    enabled: bool,
    execute: bool,
    threshold: u32,
    /// `PROMOTE_ON_USE_DURABLE=true`: count in session-core, not in this process.
    durable: bool,
    /// `PROMOTE_ON_USE_SIGNAL=cited`: count only pages the final answer cites.
    /// Any other value (or unset) keeps the fetch signal.
    signal_cited: bool,
}

fn config() -> &'static Config {
    static CFG: OnceLock<Config> = OnceLock::new();
    CFG.get_or_init(|| Config {
        enabled: std::env::var("PROMOTE_ON_USE_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false),
        execute: std::env::var("PROMOTE_ON_USE_EXECUTE")
            .map(|v| v == "true")
            .unwrap_or(false),
        threshold: std::env::var("PROMOTE_ON_USE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(3),
        durable: std::env::var("PROMOTE_ON_USE_DURABLE")
            .map(|v| v == "true")
            .unwrap_or(false),
        signal_cited: std::env::var("PROMOTE_ON_USE_SIGNAL")
            .map(|v| v == "cited")
            .unwrap_or(false),
    })
}

/// The most runs with candidates the pending set holds at once. A run that never
/// reaches its end (a crashed process loses the set anyway) cannot grow it past
/// this; new runs are ignored while it is full.
const MAX_PENDING_RUNS: usize = 1024;
/// The most distinct candidate URLs one run records.
const MAX_PENDING_URLS_PER_RUN: usize = 64;
/// How long a run's candidates are kept without a new fetch before a later
/// insert drops them (a run parked for longer than this loses its candidates).
const PENDING_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// One run's fetched URLs, awaiting the run's final answer (`cited` signal).
struct PendingRun {
    org_id: String,
    user_id: String,
    urls: Vec<String>,
    touched: Instant,
}

/// Candidates by run id. Only ever filled for an eligible (non-ZDR, non-waived,
/// non-Space) run, and emptied by [`settle_run`] when the run ends.
fn pending() -> &'static Mutex<HashMap<String, PendingRun>> {
    static PENDING: OnceLock<Mutex<HashMap<String, PendingRun>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// URL comparison form: a trailing slash is not a different page.
fn same_page_key(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// A successful `web_fetch` of `url` during run `run_id`. With the default
/// `fetch` signal this counts the use right away; with `cited` it only records a
/// pending candidate, which [`settle_run`] counts if the final answer cites it.
/// No-op unless enabled and the run is eligible (see [`PromoteContext`]).
/// Fire-and-forget — never affects the calling tool's result.
pub async fn observe_fetch(
    run_id: &str,
    org_id: &str,
    user_id: &str,
    url: &str,
    ctx: PromoteContext,
    durable: Option<&DurableSession>,
) {
    let cfg = config();
    if !cfg.enabled || ctx.blocked() || org_id.is_empty() || user_id.is_empty() || url.is_empty() {
        return;
    }
    if !cfg.signal_cited {
        maybe_promote(org_id, user_id, url, ctx, durable).await;
        return;
    }
    if run_id.is_empty() {
        return;
    }
    let Ok(mut map) = pending().lock() else {
        return;
    };
    let now = Instant::now();
    if map.len() >= MAX_PENDING_RUNS {
        map.retain(|_, run| now.duration_since(run.touched) < PENDING_TTL);
    }
    if map.len() >= MAX_PENDING_RUNS && !map.contains_key(run_id) {
        return;
    }
    let run = map.entry(run_id.to_owned()).or_insert_with(|| PendingRun {
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        urls: Vec::new(),
        touched: now,
    });
    run.touched = now;
    if run.urls.len() < MAX_PENDING_URLS_PER_RUN && !run.urls.iter().any(|u| u == url) {
        run.urls.push(url.to_owned());
    }
}

/// The run `run_id` has ended. Drops its pending candidates; with the `cited`
/// signal and a successful run (`answer` is `Some`), counts each candidate whose
/// URL appears among the answer's http(s) sources. The counting (and any
/// promote) runs on a spawned task, so ending the run never waits on it.
/// Call from an async context. No-op when nothing was recorded for the run.
pub fn settle_run(run_id: &str, answer: Option<&str>, durable: Option<DurableSession>) {
    let Ok(mut map) = pending().lock() else {
        return;
    };
    let Some(run) = map.remove(run_id) else {
        return;
    };
    drop(map);
    let Some(answer) = answer else {
        return;
    };
    let sources = crate::subagent::research_fan_out::extract_sources(answer);
    let cited: Vec<String> = run
        .urls
        .into_iter()
        .filter(|url| {
            sources
                .iter()
                .any(|source| same_page_key(source) == same_page_key(url))
        })
        .collect();
    if cited.is_empty() {
        return;
    }
    let (org_id, user_id) = (run.org_id, run.user_id);
    tokio::spawn(async move {
        // Eligibility was decided when each candidate was recorded.
        let ctx = PromoteContext {
            cited: true,
            ..PromoteContext::default()
        };
        for url in cited {
            maybe_promote(&org_id, &user_id, &url, ctx, durable.as_ref()).await;
        }
    });
}

fn counts() -> &'static Mutex<HashMap<String, u32>> {
    static COUNTS: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a grounded use of `url` by (org, user). No-op unless enabled. When
/// the per-(org,user,url) count reaches the threshold, promotes exactly once
/// (dry-run logs unless `PROMOTE_ON_USE_EXECUTE=true`). Fire-and-forget — never
/// affects the calling tool's result. Returns without counting when the run is
/// ZDR, waived or Space-scoped, and — under the `cited` signal — when the page
/// was not cited in the run's answer.
pub async fn maybe_promote(
    org_id: &str,
    user_id: &str,
    url: &str,
    ctx: PromoteContext,
    durable: Option<&DurableSession>,
) {
    let cfg = config();
    if !cfg.enabled || org_id.is_empty() || user_id.is_empty() || url.is_empty() {
        return;
    }
    if ctx.blocked() || (cfg.signal_cited && !ctx.cited) {
        return;
    }
    let fire = if cfg.durable {
        // Fail closed: no credential or a failed RPC is "not counted", never "promote anyway".
        let Some(session) = durable else {
            tracing::warn!(%user_id, "promote-on-use: durable counter has no session credential; use not counted");
            return;
        };
        match count_durable(session, org_id, user_id, url, cfg.threshold).await {
            Ok(fire) => fire,
            Err(error) => {
                tracing::warn!(%user_id, error = %error, "promote-on-use: durable count failed; use not counted, nothing promoted");
                return;
            }
        }
    } else {
        // Separator can't appear in the components; keeps the composite key unambiguous.
        let key = format!("{org_id}\u{1}{user_id}\u{1}{url}");
        match counts().lock() {
            Ok(mut map) => {
                let n = map.entry(key).or_insert(0);
                *n += 1;
                *n == cfg.threshold // fire EXACTLY once, at the threshold
            }
            Err(_) => return,
        }
    };
    if !fire {
        return;
    }
    if !cfg.execute {
        tracing::info!(
            url,
            %user_id,
            threshold = cfg.threshold,
            "promote-on-use: candidate reached threshold (DRY-RUN — not promoted; set PROMOTE_ON_USE_EXECUTE=true to arm)"
        );
        return;
    }
    match promote(org_id, user_id, url).await {
        Ok(()) => tracing::info!(url, %user_id, "promote-on-use: promoted page to knowledge base"),
        Err(e) => tracing::warn!(url, error = %e, "promote-on-use: promote failed"),
    }
}

/// The key form of a URL for the durable counter: no surrounding space, no fragment, no trailing
/// slash, and a lower-cased scheme and host (the path and query stay as written).
fn normalize_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or(url);
    let url = same_page_key(url);
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, tail) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
            format!(
                "{}://{}{}",
                scheme.to_ascii_lowercase(),
                authority.to_ascii_lowercase(),
                tail
            )
        }
        None => url.to_owned(),
    }
}

/// SHA-256 of the normalised URL. Only this ever leaves the process for the durable counter.
fn url_hash(url: &str) -> Vec<u8> {
    Sha256::digest(normalize_url(url).as_bytes()).to_vec()
}

/// One use counted in session-core. `Ok(true)` means this use reached the threshold for the first
/// time, so the caller promotes; every error is the caller's cue to promote nothing.
async fn count_durable(
    session: &DurableSession,
    org_id: &str,
    user_id: &str,
    url: &str,
    threshold: u32,
) -> Result<bool, String> {
    let request = crate::runtime_loop::authenticated_session_request(
        IncrementPromoteUseRequest {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            url_hash: url_hash(url),
            threshold,
        },
        &session.bearer,
    )?;
    let response = tokio::time::timeout(
        DURABLE_COUNT_TIMEOUT,
        PromoteOnUseServiceClient::new(session.channel.clone()).increment_promote_use(request),
    )
    .await
    .map_err(|_| "session-core count timed out".to_owned())?
    .map_err(|status| status.message().to_owned())?;
    Ok(response.into_inner().fire)
}

/// Perform the promote via Quarry's ingest primitive with a tenant-bound
/// execution-core service principal. `x-user-id` is advisory attribution only;
/// Quarry derives authority and tenant from the signed bearer.
async fn promote(org_id: &str, user_id: &str, url: &str) -> Result<(), String> {
    let base = std::env::var("QUARRY_EDGE_URL")
        .or_else(|_| std::env::var("QUARRY_EDGE_ADDR"))
        .map_err(|_| "QUARRY_EDGE_URL/ADDR unset".to_string())?;
    let endpoint = format!("{}/v1/scrape", base.trim_end_matches('/'));
    let http = reqwest::Client::new();
    let auth = crate::quarry_auth::TokenSource::from_env()
        .map_err(|error| format!("quarry authentication failed: {error}"))?;
    // A promoted page is the live web, not something the person wrote: it lands
    // private, and marked untrusted so later retrieval treats it as data.
    let body = serde_json::json!({
        "url": url,
        "org_id": org_id,
        "ingest": true,
        "visibility": "private",
        "provenance": { "source": "promote_on_use", "trust": "untrusted" },
    });
    let mut retried_unauthorized = false;
    let resp = loop {
        let token = auth
            .token(org_id, &["scrape:write"])
            .await
            .map_err(|error| format!("quarry authentication failed: {error}"))?;
        let resp = http
            .post(&endpoint)
            .bearer_auth(&token)
            .header("x-quarry-org", org_id)
            .header("x-user-id", user_id)
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
            auth.invalidate_if_matches(org_id, &["scrape:write"], &token)
                .await
                .map_err(|error| format!("quarry authentication failed: {error}"))?;
            retried_unauthorized = true;
            continue;
        }
        break resp;
    };
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("status {}", resp.status()))
    }
}
