//! Session-core's mirror of the organization's memory and learning-review policy
//! (build plan WS07 S4).
//!
//! The policy lives in org-core (`metadata.agentLearning`) and is read here, not
//! owned: a poller copies it into `org_learning_policy_mirror`, and
//! `memory_control::resolve` folds the copy into the one memory decision. Nothing
//! else in session-core reads org-core for this.
//!
//! # Fail-closed, unlike `org_quota`
//!
//! A spend ceiling that cannot be read can safely be skipped. A memory policy that
//! cannot be read must not read as "allowed": with `ORG_POLICY_FAIL_CLOSED=true` an
//! org whose mirror is missing or older than `ORG_POLICY_MAX_STALE` is answered
//! `Unavailable`, which refuses every memory read, write and learning
//! announcement for it until a poll succeeds. The default is off so a dev stack
//! without org-core keeps working; production turns it on.
//!
//! # Dormant until configured
//!
//! With no service token and no fail-closed switch the gate is not governed by this
//! module at all and answers exactly as it did before it existed. The poller idles
//! (one debug line) when its credentials are unset.
//!
//! # The watermark
//!
//! `memory_enabled_since` is set when a poll sees the org go from disabled to
//! allowed. Dreaming and the write-time check never read a message at or before it,
//! so the period the org had memory off is not mined after it is switched back on.
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::{debug, info, warn};

use crate::memory_control::OrgMemoryPolicy;

pub(crate) const FAIL_CLOSED_ENV: &str = "ORG_POLICY_FAIL_CLOSED";
pub(crate) const POLL_SECS_ENV: &str = "ORG_POLICY_POLL_SECS";
pub(crate) const MAX_STALE_ENV: &str = "ORG_POLICY_MAX_STALE";
pub(crate) const LEARNING_DEFAULT_ENV: &str = "ORG_LEARNING_DEFAULT";
pub(crate) const ORG_CORE_URL_ENV: &str = "ORG_CORE_URL";
pub(crate) const SERVICE_TOKEN_ENV: &str = "SESSION_ORG_CORE_SERVICE_TOKEN";
/// The fast lane (WS07-S4): ordered, concurrent polls and a tick for orgs with no mirror row. Only `true` enables it.
pub(crate) const FAST_LANE_ENV: &str = "ORG_POLICY_FAST_LANE";
/// How often the fast lane looks for orgs with no mirror row, in seconds (1 to 60; default 5).
pub(crate) const NEW_ORG_POLL_SECS_ENV: &str = "ORG_POLICY_NEW_ORG_POLL_SECS";
/// The startup guard: refuse to boot fail-closed without org-core credentials. Only `true` enables it.
pub(crate) const STARTUP_GUARD_ENV: &str = "ORG_POLICY_STARTUP_GUARD";

/// The principal name org-core's credential registry knows this service by.
const SERVICE_ID: &str = "session-core";
const DEFAULT_POLL_SECS: u64 = 60;
/// Fifteen minutes: several missed polls, short enough that a disabled org is not
/// served from a copy that old.
const DEFAULT_MAX_STALE_SECS: i64 = 900;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 4096;
/// How many org refreshes one poll keeps in flight, so org-core sees a bounded number of requests.
pub(crate) const ORG_POLICY_POLL_CONCURRENCY: usize = 8;
/// The most orgs the fast lane refreshes on one tick.
const NEW_ORG_BATCH: i64 = 50;
const DEFAULT_NEW_ORG_POLL_SECS: u64 = 5;
const MAX_NEW_ORG_POLL_SECS: u64 = 60;
/// The longest the tick leaves an org alone after its refreshes keep failing.
const MAX_NEW_ORG_BACKOFF: Duration = Duration::from_secs(60);
/// How many failing orgs the tick remembers at once. Past this, the one due soonest is forgotten.
const NEW_ORG_TRACKED_MAX: usize = 1024;

/// What the gate needs from the environment. Resolved once; pure parsing is in
/// [`GateSettings::from_lookup`] so the vocabulary is testable without mutating
/// process env (`unsafe_code` is forbidden workspace-wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GateSettings {
    /// Only the literal `true` enables it.
    pub(crate) fail_closed: bool,
    pub(crate) max_stale_secs: i64,
    /// Learning review for an org the mirror has no row for, while credentials
    /// are configured. Only the literal `proposals` enables it.
    pub(crate) learning_default: bool,
    /// Whether this module governs the decision at all.
    pub(crate) governed: bool,
}

impl GateSettings {
    pub(crate) fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let fail_closed = get(FAIL_CLOSED_ENV).as_deref() == Some("true");
        let credentials = Poller::from_lookup(&get).is_some();
        Self {
            fail_closed,
            max_stale_secs: positive(get(MAX_STALE_ENV).as_deref())
                .and_then(|secs| i64::try_from(secs).ok())
                .unwrap_or(DEFAULT_MAX_STALE_SECS),
            learning_default: get(LEARNING_DEFAULT_ENV).as_deref() == Some("proposals"),
            governed: fail_closed || credentials,
        }
    }
}

fn positive(value: Option<&str>) -> Option<u64> {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
}

pub(crate) fn settings() -> &'static GateSettings {
    static SETTINGS: OnceLock<GateSettings> = OnceLock::new();
    SETTINGS.get_or_init(|| GateSettings::from_lookup(|name| std::env::var(name).ok()))
}

/// One stored copy of an org's policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MirrorRow {
    pub(crate) memory_allowed: bool,
    pub(crate) learning_mode: String,
    pub(crate) fetched_at: DateTime<Utc>,
}

/// What the gate folds in for one org.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OrgView {
    pub(crate) memory: OrgMemoryPolicy,
    pub(crate) learning_review: bool,
}

/// The decision table of the mirror. Pure.
pub(crate) fn view_from_row(
    row: Option<&MirrorRow>,
    now: DateTime<Utc>,
    settings: &GateSettings,
) -> OrgView {
    const UNAVAILABLE: OrgView = OrgView {
        memory: OrgMemoryPolicy::Unavailable,
        learning_review: false,
    };
    if !settings.governed {
        return OrgView {
            memory: OrgMemoryPolicy::Allow,
            learning_review: true,
        };
    }
    let Some(row) = row else {
        // Credentials are configured but this org was never fetched (new, or the
        // poller has not reached it yet).
        return if settings.fail_closed {
            UNAVAILABLE
        } else {
            OrgView {
                memory: OrgMemoryPolicy::Allow,
                learning_review: settings.learning_default,
            }
        };
    };
    if settings.fail_closed && (now - row.fetched_at).num_seconds() > settings.max_stale_secs {
        return UNAVAILABLE;
    }
    OrgView {
        memory: if row.memory_allowed {
            OrgMemoryPolicy::Allow
        } else {
            OrgMemoryPolicy::Disabled
        },
        learning_review: row.memory_allowed && row.learning_mode == "proposals",
    }
}

/// The org's learning-review policy in one word, for the memory controls an owner
/// reads. Empty when this module does not govern the gate, so there is nothing to
/// report; `unavailable` when the fail-closed rule refuses the org; otherwise
/// `proposals` or `disabled`, as the gate itself decides them.
pub(crate) fn learning_policy_label(
    row: Option<&MirrorRow>,
    now: DateTime<Utc>,
    settings: &GateSettings,
) -> &'static str {
    if !settings.governed {
        return "";
    }
    let view = view_from_row(row, now, settings);
    if view.memory == OrgMemoryPolicy::Unavailable {
        return "unavailable";
    }
    if view.learning_review {
        "proposals"
    } else {
        "disabled"
    }
}

/// The mirror row of one org, `None` when it was never fetched.
pub(crate) async fn mirror_row<'e, E>(executor: E, org_id: &str) -> Result<Option<MirrorRow>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let row: Option<(bool, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT memory_allowed, learning_mode, fetched_at FROM org_learning_policy_mirror WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|(memory_allowed, learning_mode, fetched_at)| MirrorRow {
        memory_allowed,
        learning_mode,
        fetched_at,
    }))
}

/// The three values a background query binds to apply the org policy to a batch of
/// messages: whether the filter is active, whether a missing or stale mirror
/// blocks, and how old counts as stale. `active` also folds in the memory gate's
/// own kill switch, which restores the behaviour before any of this existed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingGate {
    pub(crate) active: bool,
    pub(crate) fail_closed: bool,
    pub(crate) max_stale_secs: i64,
}

pub(crate) fn pending_gate() -> PendingGate {
    let settings = settings();
    PendingGate {
        active: settings.governed && crate::memory_control::controls_enabled(),
        fail_closed: settings.fail_closed,
        max_stale_secs: settings.max_stale_secs,
    }
}

/// SQL conjunct, to be combined with AND into a query over `messages m JOIN threads t`, that
/// leaves out what the org policy does not let Dreaming read: an org that disabled
/// memory, a message from the period before it was switched back on, and (when
/// failing closed) an org whose mirror is missing or stale. `$gate`, `$fail_closed`
/// and `$stale` are the parameter numbers of the three [`PendingGate`] values.
pub(crate) fn pending_filter(gate: usize, fail_closed: usize, stale: usize) -> String {
    format!(
        "AND NOT (${gate} AND ( \
           (${fail_closed} AND NOT EXISTS (SELECT 1 FROM org_learning_policy_mirror p WHERE p.org_id = t.org_id)) \
           OR EXISTS (SELECT 1 FROM org_learning_policy_mirror p WHERE p.org_id = t.org_id AND ( \
                NOT p.memory_allowed \
                OR m.created_at <= p.memory_enabled_since \
                OR (${fail_closed} AND p.fetched_at < now() - (${stale} * interval '1 second')) \
           )) \
         ))"
    )
}

/// What org-core answered for one org.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FetchedPolicy {
    pub(crate) memory_allowed: bool,
    pub(crate) learning_mode: &'static str,
    pub(crate) revision: i64,
}

impl FetchedPolicy {
    /// An org org-core does not know has no policy: the defaults.
    fn defaults() -> Self {
        Self {
            memory_allowed: true,
            learning_mode: "disabled",
            revision: 0,
        }
    }
}

#[derive(Deserialize)]
struct Wire {
    memory: String,
    #[serde(rename = "learningReview")]
    learning_review: String,
    #[serde(default)]
    revision: i64,
}

/// Strict reading of org-core's answer: memory is allowed only when it says exactly
/// `allow`, learning review is on only when it says exactly `proposals`. Anything
/// else is the restrictive value, so a spelling this build does not know can never
/// widen what is allowed.
fn parse_policy(body: &[u8]) -> Result<FetchedPolicy> {
    let wire: Wire = serde_json::from_slice(body).context("agent-policy body is not a policy")?;
    Ok(FetchedPolicy {
        memory_allowed: wire.memory == "allow",
        learning_mode: if wire.learning_review == "proposals" {
            "proposals"
        } else {
            "disabled"
        },
        revision: wire.revision.max(0),
    })
}

/// Reads org-core on behalf of session-core. Holds a service credential, so it has
/// no `Debug`. `Clone` shares the HTTP client; a fast-lane task owns its own copy.
#[derive(Clone)]
pub(crate) struct Poller {
    base_url: String,
    token: String,
    client: reqwest::Client,
    interval: Duration,
    /// Whether the fast lane is on, read once at boot (`ORG_POLICY_FAST_LANE`).
    fast_lane: bool,
    /// How often the fast lane looks for orgs with no mirror row.
    new_org_interval: Duration,
}

impl Poller {
    /// `None` when the credentials are unset: the module is then dormant.
    pub(crate) fn from_lookup(get: &impl Fn(&str) -> Option<String>) -> Option<Self> {
        let base_url = get(ORG_CORE_URL_ENV)?.trim().to_owned();
        let token = get(SERVICE_TOKEN_ENV)?.trim().to_owned();
        if base_url.is_empty() || token.is_empty() {
            return None;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok()?;
        Some(Self {
            base_url,
            token,
            client,
            interval: Duration::from_secs(
                positive(get(POLL_SECS_ENV).as_deref()).unwrap_or(DEFAULT_POLL_SECS),
            ),
            fast_lane: mp_contracts::loop_fold::switch_from(get(FAST_LANE_ENV).as_deref()),
            new_org_interval: Duration::from_secs(new_org_poll_secs(
                get(NEW_ORG_POLL_SECS_ENV).as_deref(),
            )),
        })
    }

    fn policy_url(&self, org_id: &str) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&self.base_url).context("ORG_CORE_URL is not a URL")?;
        url.path_segments_mut()
            .map_err(|()| anyhow!("ORG_CORE_URL cannot carry a path"))?
            .pop_if_empty()
            .extend(["internal", "orgs", org_id, "agent-policy"]);
        Ok(url)
    }

    /// One org's policy. A 404 is a definitive "org-core does not know it" and reads
    /// as the defaults; any other failure is an error and leaves the mirror as it
    /// was, so it ages toward stale instead of being replaced by a guess.
    async fn fetch(&self, org_id: &str) -> Result<FetchedPolicy> {
        let response = self
            .client
            .get(self.policy_url(org_id)?)
            .header("x-service-id", SERVICE_ID)
            .header("x-service-token", &self.token)
            .send()
            .await
            .context("agent-policy request failed")?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(FetchedPolicy::defaults());
        }
        if !status.is_success() {
            return Err(anyhow!("agent-policy answered {status}"));
        }
        let body = response.bytes().await.context("agent-policy body unreadable")?;
        if body.len() > MAX_BODY_BYTES {
            return Err(anyhow!("agent-policy body too large"));
        }
        parse_policy(&body)
    }

    /// Fetch and store one org. The watermark moves only on a transition from
    /// disabled to allowed.
    pub(crate) async fn refresh_org(&self, pool: &PgPool, org_id: &str) -> Result<()> {
        let fetched = self.fetch(org_id).await?;
        record_policy(pool, org_id, &fetched).await?;
        Ok(())
    }

    /// One pass over every org this service knows. Returns how many refreshed. With the fast lane off
    /// this is the sequential pass it always was. With it on, the orgs with no mirror row come first,
    /// then the stalest, and at most [`ORG_POLICY_POLL_CONCURRENCY`] refreshes are in flight at once.
    pub(crate) async fn poll_once(&self, pool: &PgPool) -> Result<usize> {
        let refreshed = if self.fast_lane {
            let orgs: Vec<String> = sqlx::query_scalar(ORDERED_ORGS_SQL).fetch_all(pool).await?;
            self.refresh_bounded(pool, orgs, record_refresh).await
        } else {
            let orgs: Vec<String> = sqlx::query_scalar(
                "SELECT org_id FROM org_learning_policy_mirror UNION SELECT DISTINCT org_id FROM threads",
            )
            .fetch_all(pool)
            .await?;
            let mut refreshed = 0usize;
            for org_id in &orgs {
                if record_refresh(org_id, self.refresh_org(pool, org_id).await) {
                    refreshed += 1;
                }
            }
            refreshed
        };
        let oldest: Option<f64> = sqlx::query_scalar(
            "SELECT EXTRACT(EPOCH FROM now() - MIN(fetched_at))::float8 FROM org_learning_policy_mirror",
        )
        .fetch_one(pool)
        .await?;
        metrics::gauge!("mp_session_org_policy_oldest_mirror_age_seconds").set(oldest.unwrap_or(0.0));
        Ok(refreshed)
    }

    /// The fast lane's tick: refreshes at most [`NEW_ORG_BATCH`] orgs that have a thread but no mirror
    /// row, so a brand-new org is answered within one tick rather than one full poll. An org whose
    /// refresh failed is left out until its backoff in `backoff` has run out, so a failing org is not
    /// asked for on every tick. Returns how many refreshed.
    pub(crate) async fn poll_new_orgs(
        &self,
        pool: &PgPool,
        backoff: &mut NewOrgBackoff,
    ) -> Result<usize> {
        let now = Instant::now();
        let tick = self.new_org_interval;
        let waiting = backoff.waiting(now);
        let orgs: Vec<String> = sqlx::query_scalar(NEW_ORGS_SQL)
            .bind(NEW_ORG_BATCH)
            .bind(waiting)
            .fetch_all(pool)
            .await?;
        let refreshed = self
            .refresh_bounded(pool, orgs, |org_id: &str, outcome: Result<()>| match outcome {
                Ok(()) => {
                    count_refresh(true);
                    backoff.settle(org_id, true, now, tick);
                    true
                }
                Err(error) => {
                    count_refresh(false);
                    if backoff.settle(org_id, false, now, tick) {
                        warn!(%org_id, %error, "org agent policy new-org refresh failed; it waits out a backoff");
                    } else {
                        debug!(%org_id, %error, "org agent policy new-org refresh failed again");
                    }
                    false
                }
            })
            .await;
        Ok(refreshed)
    }

    /// Refreshes `orgs` with at most [`ORG_POLICY_POLL_CONCURRENCY`] in flight. `settle` is told each
    /// outcome and says whether it counts as refreshed. Returns how many refreshed.
    async fn refresh_bounded<S>(&self, pool: &PgPool, orgs: Vec<String>, settle: S) -> usize
    where
        S: FnMut(&str, Result<()>) -> bool,
    {
        let poller = self.clone();
        let pool = pool.clone();
        refresh_concurrently(
            orgs,
            ORG_POLICY_POLL_CONCURRENCY,
            move |org_id| {
                let poller = poller.clone();
                let pool = pool.clone();
                async move {
                    let outcome = poller.refresh_org(&pool, &org_id).await;
                    (org_id, outcome)
                }
            },
            settle,
        )
        .await
    }
}

/// Every org this service knows, the ones with no mirror row first and then the stalest. The fast lane's order.
const ORDERED_ORGS_SQL: &str = "SELECT orgs.org_id FROM ( \
     SELECT org_id FROM org_learning_policy_mirror \
     UNION \
     SELECT DISTINCT org_id FROM threads \
 ) AS orgs \
 LEFT JOIN org_learning_policy_mirror mirror ON mirror.org_id = orgs.org_id \
 ORDER BY mirror.fetched_at ASC NULLS FIRST, orgs.org_id";

/// One batch of orgs that have a thread and no mirror row yet, lowest org id first, for the fast lane's
/// tick. The orgs bound as `$2` (still inside their backoff) are left out.
///
/// It reads the orgs, not the threads. Several of the threads indexes lead with `org_id`, so each distinct
/// org is one index probe (a loose index scan): a steady-state tick costs one probe per org and never reads
/// every thread. The old form, an anti-join with GROUP BY over `threads`, read every thread on every tick.
const NEW_ORGS_SQL: &str = "WITH RECURSIVE thread_orgs(org_id) AS ( \
     (SELECT t.org_id FROM threads t ORDER BY t.org_id LIMIT 1) \
     UNION ALL \
     (SELECT (SELECT n.org_id FROM threads n WHERE n.org_id > o.org_id ORDER BY n.org_id LIMIT 1) \
      FROM thread_orgs o WHERE o.org_id IS NOT NULL) \
 ) \
 SELECT o.org_id FROM thread_orgs o \
 WHERE o.org_id IS NOT NULL \
   AND NOT (o.org_id = ANY($2::text[])) \
   AND NOT EXISTS (SELECT 1 FROM org_learning_policy_mirror m WHERE m.org_id = o.org_id) \
 ORDER BY o.org_id \
 LIMIT $1";

/// The tick's memory of orgs whose refresh failed. It lives in the tick's task only: a restart forgets
/// it, and the first tick after a restart asks for every missing org again.
#[derive(Debug, Default)]
pub(crate) struct NewOrgBackoff {
    failing: HashMap<String, Failing>,
}

/// One org's failures in a row, and the instant it may be asked for again.
#[derive(Debug, Clone, Copy)]
struct Failing {
    failures: u32,
    retry_at: Instant,
}

impl NewOrgBackoff {
    /// The orgs still inside their backoff at `now`. The tick leaves them out of its batch.
    fn waiting(&self, now: Instant) -> Vec<String> {
        self.failing
            .iter()
            .filter(|(_, failing)| failing.retry_at > now)
            .map(|(org_id, _)| org_id.clone())
            .collect()
    }

    /// Records one refresh of `org_id` at `now`, on a tick of `tick`. A refresh that worked forgets the
    /// org. A failure waits longer than the one before (see [`backoff_for`]), and returns true when it is
    /// worth a warning: the first failure, then each power of two, so a long outage is logged less often.
    fn settle(&mut self, org_id: &str, refreshed: bool, now: Instant, tick: Duration) -> bool {
        if refreshed {
            self.failing.remove(org_id);
            return false;
        }
        if !self.failing.contains_key(org_id) && self.failing.len() >= NEW_ORG_TRACKED_MAX {
            let due_soonest = self
                .failing
                .iter()
                .min_by_key(|(_, failing)| failing.retry_at)
                .map(|(due, _)| due.clone());
            if let Some(due) = due_soonest {
                self.failing.remove(&due);
            }
        }
        let failing = self.failing.entry(org_id.to_owned()).or_insert(Failing {
            failures: 0,
            retry_at: now,
        });
        failing.failures = failing.failures.saturating_add(1);
        failing.retry_at = now + backoff_for(failing.failures, tick);
        failing.failures.is_power_of_two()
    }
}

/// How long an org waits after its `failures`-th failure in a row: the tick interval, doubled for each
/// failure after the first, and never more than [`MAX_NEW_ORG_BACKOFF`].
fn backoff_for(failures: u32, tick: Duration) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    tick.saturating_mul(1 << doublings).min(MAX_NEW_ORG_BACKOFF)
}

/// Counts one org's refresh outcome on the poll counter. Every poll path counts through here.
fn count_refresh(refreshed: bool) {
    if refreshed {
        metrics::counter!("mp_session_org_policy_polls_total", "outcome" => "ok").increment(1);
    } else {
        metrics::counter!("mp_session_org_policy_polls_total", "outcome" => "error").increment(1);
    }
}

/// Counts one org's refresh outcome the way every poll counts it, and logs a failure. True when it refreshed.
fn record_refresh(org_id: &str, outcome: Result<()>) -> bool {
    match outcome {
        Ok(()) => {
            count_refresh(true);
            true
        }
        Err(error) => {
            count_refresh(false);
            warn!(%org_id, %error, "org agent policy refresh failed; the mirror keeps ageing");
            false
        }
    }
}

/// Runs `refresh` for each org, with at most `limit` tasks in flight. A task holds its semaphore permit
/// until its refresh is over, so the bound holds however many orgs there are. `settle` is told each
/// outcome and says whether it counts as refreshed. Returns how many refreshed.
async fn refresh_concurrently<F, Fut, S>(
    orgs: Vec<String>,
    limit: usize,
    refresh: F,
    mut settle: S,
) -> usize
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = (String, Result<()>)> + Send + 'static,
    S: FnMut(&str, Result<()>) -> bool,
{
    let gate = Arc::new(Semaphore::new(limit));
    let mut tasks = JoinSet::new();
    let mut refreshed = 0usize;
    for org_id in orgs {
        let Ok(permit) = Arc::clone(&gate).acquire_owned().await else {
            break;
        };
        let work = refresh(org_id);
        tasks.spawn(async move {
            let outcome = work.await;
            drop(permit);
            outcome
        });
        while let Some(joined) = tasks.try_join_next() {
            refreshed += count_joined(joined, &mut settle);
        }
    }
    while let Some(joined) = tasks.join_next().await {
        refreshed += count_joined(joined, &mut settle);
    }
    refreshed
}

/// Settles one finished refresh task. A task that panicked has no org to name, so it counts as an error.
fn count_joined(
    joined: std::result::Result<(String, Result<()>), tokio::task::JoinError>,
    settle: &mut impl FnMut(&str, Result<()>) -> bool,
) -> usize {
    match joined {
        Ok((org_id, outcome)) => usize::from(settle(&org_id, outcome)),
        Err(error) => {
            metrics::counter!("mp_session_org_policy_polls_total", "outcome" => "error")
                .increment(1);
            warn!(%error, "org agent policy refresh task ended early");
            0
        }
    }
}

/// The fast lane's tick in seconds (WS07-S4): 5 by default, and any whole number is kept within 1 to 60.
/// A value that is not a whole number reads as the default.
fn new_org_poll_secs(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(DEFAULT_NEW_ORG_POLL_SECS, |secs| {
            secs.clamp(1, MAX_NEW_ORG_POLL_SECS)
        })
}

/// The startup guard (WS07-S4). With `ORG_POLICY_STARTUP_GUARD=true`, a service set to fail closed that
/// has no org-core credentials refuses to boot, rather than refusing every org's memory while it looks
/// healthy. The error names the variables and never their values.
pub(crate) fn startup_check(lookup: impl Fn(&str) -> Option<String>) -> Result<()> {
    startup_check_with(&lookup, settings().fail_closed)
}

/// [`startup_check`] with the fail-closed reading passed in, so the three conditions are testable.
fn startup_check_with(lookup: &impl Fn(&str) -> Option<String>, fail_closed: bool) -> Result<()> {
    let guard_on = mp_contracts::loop_fold::switch_from(lookup(STARTUP_GUARD_ENV).as_deref());
    if guard_on && fail_closed && Poller::from_lookup(lookup).is_none() {
        return Err(anyhow!(
            "{FAIL_CLOSED_ENV}=true refuses every org's memory without org-core credentials: set {ORG_CORE_URL_ENV} and {SERVICE_TOKEN_ENV}, or set {STARTUP_GUARD_ENV} to something other than true"
        ));
    }
    Ok(())
}

async fn record_policy(pool: &PgPool, org_id: &str, fetched: &FetchedPolicy) -> Result<(), sqlx::Error> {
    // clock_timestamp so the watermark is the instant the transition was stored.
    sqlx::query(
        "INSERT INTO org_learning_policy_mirror \
             (org_id, memory_allowed, learning_mode, memory_enabled_since, revision, fetched_at) \
         VALUES ($1, $2, $3, NULL, $4, clock_timestamp()) \
         ON CONFLICT (org_id) DO UPDATE SET \
             memory_enabled_since = CASE \
                 WHEN NOT org_learning_policy_mirror.memory_allowed AND EXCLUDED.memory_allowed \
                 THEN clock_timestamp() ELSE org_learning_policy_mirror.memory_enabled_since END, \
             memory_allowed = EXCLUDED.memory_allowed, \
             learning_mode = EXCLUDED.learning_mode, \
             revision = EXCLUDED.revision, \
             fetched_at = EXCLUDED.fetched_at",
    )
    .bind(org_id)
    .bind(fetched.memory_allowed)
    .bind(fetched.learning_mode)
    .bind(fetched.revision)
    .execute(pool)
    .await?;
    Ok(())
}

/// Background task beside Dreaming. Idles when the credentials are unset.
pub(crate) async fn run(pool: PgPool) -> Result<()> {
    let lookup = |name: &str| std::env::var(name).ok();
    let Some(poller) = Poller::from_lookup(&lookup) else {
        if settings().fail_closed {
            tracing::error!(
                "{FAIL_CLOSED_ENV} is true but {SERVICE_TOKEN_ENV} is unset: every org's memory is refused"
            );
        } else {
            debug!("org policy credentials unset; the memory gate is not governed by org policy");
        }
        std::future::pending::<()>().await;
        return Ok(());
    };
    info!(interval_secs = poller.interval.as_secs(), "org agent policy poller started");
    if poller.fast_lane {
        // Its own task, so a long full poll never holds up a brand-new org.
        info!(
            interval_secs = poller.new_org_interval.as_secs(),
            "org agent policy fast lane started"
        );
        let fast = poller.clone();
        let fast_pool = pool.clone();
        tokio::spawn(async move {
            // The tick's own memory: the orgs still in a backoff, and how many ticks in a row have failed.
            let mut backoff = NewOrgBackoff::default();
            let mut tick_failures = 0u32;
            loop {
                match fast.poll_new_orgs(&fast_pool, &mut backoff).await {
                    Ok(_) => tick_failures = 0,
                    Err(error) => {
                        tick_failures = tick_failures.saturating_add(1);
                        if tick_failures.is_power_of_two() {
                            warn!(%error, tick_failures, "org agent policy new-org poll failed");
                        } else {
                            debug!(%error, tick_failures, "org agent policy new-org poll failed again");
                        }
                    }
                }
                tokio::time::sleep(fast.new_org_interval).await;
            }
        });
    }
    loop {
        if let Err(error) = poller.poll_once(&pool).await {
            warn!(%error, "org agent policy poll failed");
        }
        tokio::time::sleep(poller.interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_control::{decide, MemoryReason, MemoryState};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::{
        matchers::{header, method, path, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn row(memory_allowed: bool, learning: &str, age_secs: i64) -> MirrorRow {
        MirrorRow {
            memory_allowed,
            learning_mode: learning.to_owned(),
            fetched_at: Utc::now() - chrono::Duration::seconds(age_secs),
        }
    }

    fn governed(fail_closed: bool) -> GateSettings {
        GateSettings {
            fail_closed,
            max_stale_secs: 900,
            learning_default: false,
            governed: true,
        }
    }

    #[test]
    fn unset_credentials_idle_quietly() {
        // Nothing configured: no poller, no governance, the gate answers as before.
        assert!(Poller::from_lookup(&env(&[])).is_none());
        assert!(Poller::from_lookup(&env(&[(ORG_CORE_URL_ENV, "http://org-core:8080")])).is_none());
        assert!(Poller::from_lookup(&env(&[(SERVICE_TOKEN_ENV, "token-value")])).is_none());
        assert!(Poller::from_lookup(&env(&[
            (ORG_CORE_URL_ENV, "http://org-core:8080"),
            (SERVICE_TOKEN_ENV, "   "),
        ]))
        .is_none());
        let settings = GateSettings::from_lookup(env(&[]));
        assert!(!settings.governed && !settings.fail_closed);
        let view = view_from_row(None, Utc::now(), &settings);
        assert_eq!(view.memory, OrgMemoryPolicy::Allow);
        assert!(view.learning_review, "ungoverned must leave learning as it was");
        // And an ungoverned gate ignores even a stored row.
        let stored = row(false, "disabled", 5);
        assert_eq!(
            view_from_row(Some(&stored), Utc::now(), &settings).memory,
            OrgMemoryPolicy::Allow
        );
    }

    #[test]
    fn switches_use_the_strict_vocabulary() {
        for value in ["TRUE", "1", "yes", "on", " true", ""] {
            assert!(
                !GateSettings::from_lookup(env(&[(FAIL_CLOSED_ENV, value)])).fail_closed,
                "{value:?} must not enable fail-closed"
            );
        }
        assert!(GateSettings::from_lookup(env(&[(FAIL_CLOSED_ENV, "true")])).fail_closed);
        // Fail-closed governs even without credentials.
        assert!(GateSettings::from_lookup(env(&[(FAIL_CLOSED_ENV, "true")])).governed);

        let configured = [
            (ORG_CORE_URL_ENV, "http://org-core:8080"),
            (SERVICE_TOKEN_ENV, "token-value"),
        ];
        let settings = GateSettings::from_lookup(env(&configured));
        assert!(settings.governed && !settings.learning_default);
        assert_eq!(settings.max_stale_secs, DEFAULT_MAX_STALE_SECS);

        let mut tuned = configured.to_vec();
        tuned.extend([
            (LEARNING_DEFAULT_ENV, "proposals"),
            (MAX_STALE_ENV, "120"),
        ]);
        let settings = GateSettings::from_lookup(env(&tuned));
        assert!(settings.learning_default);
        assert_eq!(settings.max_stale_secs, 120);
        for junk in ["Proposals", "enabled", "auto"] {
            let mut other = configured.to_vec();
            other.push((LEARNING_DEFAULT_ENV, junk));
            assert!(!GateSettings::from_lookup(env(&other)).learning_default, "{junk}");
        }
        for junk in ["0", "-5", "soon", ""] {
            let mut other = configured.to_vec();
            other.push((MAX_STALE_ENV, junk));
            assert_eq!(
                GateSettings::from_lookup(env(&other)).max_stale_secs,
                DEFAULT_MAX_STALE_SECS,
                "{junk:?}"
            );
        }
    }

    #[test]
    fn stale_mirror_denies_generation_when_fail_closed() {
        let now = Utc::now();
        let stale = row(true, "proposals", 1_000);
        let view = view_from_row(Some(&stale), now, &governed(true));
        assert_eq!(view.memory, OrgMemoryPolicy::Unavailable);
        assert!(!view.learning_review);
        let decision = decide(false, view.memory, MemoryState::On);
        assert!(!decision.generation_allowed);
        assert!(!decision.learning_allowed);
        assert_eq!(decision.reason, MemoryReason::PolicyUnavailable);

        // A missing row fails closed the same way.
        let missing = view_from_row(None, now, &governed(true));
        assert_eq!(missing.memory, OrgMemoryPolicy::Unavailable);

        // Fresh enough is trusted.
        let fresh = row(true, "disabled", 899);
        assert_eq!(
            view_from_row(Some(&fresh), now, &governed(true)).memory,
            OrgMemoryPolicy::Allow
        );
        // Without the switch a stale copy is still the best answer there is.
        let view = view_from_row(Some(&stale), now, &governed(false));
        assert_eq!(view.memory, OrgMemoryPolicy::Allow);
        assert!(view.learning_review);
    }

    #[test]
    fn a_disabled_org_denies_everything_and_learning_needs_the_opt_in() {
        let now = Utc::now();
        let disabled = view_from_row(Some(&row(false, "proposals", 1)), now, &governed(true));
        assert_eq!(disabled.memory, OrgMemoryPolicy::Disabled);
        assert!(!disabled.learning_review, "disabled memory leaves nothing to learn from");
        let decision = decide(false, disabled.memory, MemoryState::On);
        assert_eq!(decision.reason, MemoryReason::OrgDisabled);
        assert!(!decision.generation_allowed);

        let default_org = view_from_row(Some(&row(true, "disabled", 1)), now, &governed(true));
        assert_eq!(default_org.memory, OrgMemoryPolicy::Allow);
        assert!(!default_org.learning_review);

        let opted_in = view_from_row(Some(&row(true, "proposals", 1)), now, &governed(true));
        assert!(opted_in.learning_review);

        // Configured but never fetched, not failing closed: the configured default.
        let mut settings = governed(false);
        assert!(!view_from_row(None, now, &settings).learning_review);
        settings.learning_default = true;
        assert!(view_from_row(None, now, &settings).learning_review);
    }

    #[test]
    fn org_core_answers_are_read_strictly() {
        let policy = parse_policy(br#"{"memory":"allow","learningReview":"proposals","revision":7}"#).unwrap();
        assert_eq!(
            policy,
            FetchedPolicy { memory_allowed: true, learning_mode: "proposals", revision: 7 }
        );
        let unknown = parse_policy(br#"{"memory":"Allow","learningReview":"auto","revision":-3}"#).unwrap();
        assert_eq!(
            unknown,
            FetchedPolicy { memory_allowed: false, learning_mode: "disabled", revision: 0 }
        );
        assert!(parse_policy(br#"{"learningReview":"proposals"}"#).is_err());
        assert!(parse_policy(b"<html>").is_err());
    }

    #[test]
    fn the_pending_filter_names_each_parameter_and_the_watermark() {
        let sql = pending_filter(3, 4, 5);
        assert!(sql.contains("$3 AND"));
        assert!(sql.contains("$4 AND NOT EXISTS"));
        assert!(sql.contains("$4 AND p.fetched_at < now() - ($5 * interval '1 second')"));
        assert!(sql.contains("m.created_at <= p.memory_enabled_since"));
        assert!(sql.contains("NOT p.memory_allowed"));
    }

    #[test]
    fn the_migration_is_additive_and_has_a_revert() {
        let migration = include_str!("../migrations/0057_org_learning_policy_mirror.sql");
        assert!(migration.contains("CREATE TABLE IF NOT EXISTS org_learning_policy_mirror"));
        assert!(migration.contains("CHECK (learning_mode IN ('disabled', 'proposals'))"));
        assert!(!migration.to_uppercase().contains("DROP "));
        let revert = include_str!("../migrations/revert/0057_org_learning_policy_mirror.down.sql");
        assert!(revert.contains("DROP TABLE IF EXISTS org_learning_policy_mirror"));
    }

    #[test]
    fn the_learning_policy_label_says_what_the_org_allows() {
        let now = Utc::now();
        let ungoverned = GateSettings::from_lookup(env(&[]));
        assert_eq!(
            learning_policy_label(Some(&row(true, "proposals", 1)), now, &ungoverned),
            "",
            "an ungoverned gate reports nothing"
        );
        assert_eq!(
            learning_policy_label(None, now, &governed(true)),
            "unavailable",
            "a missing mirror fails closed"
        );
        assert_eq!(
            learning_policy_label(Some(&row(true, "proposals", 1_000)), now, &governed(true)),
            "unavailable",
            "a stale mirror fails closed"
        );
        assert_eq!(
            learning_policy_label(Some(&row(false, "proposals", 1)), now, &governed(true)),
            "disabled"
        );
        assert_eq!(
            learning_policy_label(Some(&row(true, "disabled", 1)), now, &governed(true)),
            "disabled"
        );
        assert_eq!(
            learning_policy_label(Some(&row(true, "proposals", 1)), now, &governed(true)),
            "proposals"
        );
        assert_eq!(
            learning_policy_label(None, now, &governed(false)),
            "disabled",
            "configured but never fetched, without the fail-closed rule, is the default"
        );
    }

    fn poller(server: &MockServer) -> Poller {
        Poller::from_lookup(&env(&[
            (ORG_CORE_URL_ENV, &server.uri()),
            (SERVICE_TOKEN_ENV, "unit-test-token"),
        ]))
        .expect("configured")
    }

    #[tokio::test]
    async fn fetch_sends_the_service_headers_to_the_one_route() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org-1/agent-policy"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-token", "unit-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"memory":"disabled","learningReview":"proposals","revision":2}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;
        let fetched = poller(&server).fetch("org-1").await.unwrap();
        assert!(!fetched.memory_allowed);
        assert_eq!(fetched.learning_mode, "proposals");
        assert_eq!(fetched.revision, 2);
    }

    #[tokio::test]
    async fn an_unknown_org_is_the_defaults_and_a_failure_is_never_a_policy() {
        let server = MockServer::start().await;
        Mock::given(path("/internal/orgs/org-gone/agent-policy"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(path("/internal/orgs/org-down/agent-policy"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(path("/internal/orgs/org-denied/agent-policy"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let poller = poller(&server);
        assert_eq!(poller.fetch("org-gone").await.unwrap(), FetchedPolicy::defaults());
        assert!(poller.fetch("org-down").await.is_err());
        assert!(poller.fetch("org-denied").await.is_err());
    }

    #[test]
    fn an_org_id_cannot_escape_its_path_segment() {
        let poller = Poller::from_lookup(&env(&[
            (ORG_CORE_URL_ENV, "http://org-core:8080/"),
            (SERVICE_TOKEN_ENV, "unit-test-token"),
        ]))
        .unwrap();
        let url = poller.policy_url("../org-2?x=1").unwrap();
        assert_eq!(url.path(), "/internal/orgs/..%2Forg-2%3Fx=1/agent-policy");
        assert_eq!(url.query(), None);
    }

    #[test]
    fn the_fast_lane_and_the_startup_guard_are_on_only_for_the_literal_true() {
        let configured = [
            (ORG_CORE_URL_ENV, "http://org-core:8080"),
            (SERVICE_TOKEN_ENV, "token-value"),
        ];
        assert!(
            !Poller::from_lookup(&env(&configured))
                .expect("configured")
                .fast_lane
        );
        for value in ["TRUE", "1", "yes", "on", " true", ""] {
            let mut pairs = configured.to_vec();
            pairs.push((FAST_LANE_ENV, value));
            assert!(
                !Poller::from_lookup(&env(&pairs))
                    .expect("configured")
                    .fast_lane,
                "{value:?} must not turn the fast lane on"
            );
            let guard_only = [(STARTUP_GUARD_ENV, value)];
            assert!(
                startup_check_with(&env(&guard_only), true).is_ok(),
                "{value:?} must not arm the startup guard"
            );
        }
        let mut on = configured.to_vec();
        on.push((FAST_LANE_ENV, "true"));
        assert!(
            Poller::from_lookup(&env(&on))
                .expect("configured")
                .fast_lane
        );
    }

    #[test]
    fn the_new_org_tick_is_five_seconds_kept_within_one_to_sixty() {
        assert_eq!(DEFAULT_NEW_ORG_POLL_SECS, 5);
        assert_eq!(new_org_poll_secs(None), 5);
        assert_eq!(new_org_poll_secs(Some(" 30 ")), 30);
        assert_eq!(new_org_poll_secs(Some("1")), 1);
        assert_eq!(new_org_poll_secs(Some("60")), 60);
        assert_eq!(new_org_poll_secs(Some("0")), 1);
        assert_eq!(new_org_poll_secs(Some("600")), 60);
        for junk in ["soon", "-3", "2.5", ""] {
            assert_eq!(new_org_poll_secs(Some(junk)), 5, "{junk:?}");
        }
        let configured = [
            (ORG_CORE_URL_ENV, "http://org-core:8080"),
            (SERVICE_TOKEN_ENV, "token-value"),
            (NEW_ORG_POLL_SECS_ENV, "15"),
        ];
        let poller = Poller::from_lookup(&env(&configured)).expect("configured");
        assert_eq!(
            poller.new_org_interval,
            Duration::from_secs(15),
            "read at boot"
        );
    }

    #[test]
    fn the_fast_lane_queries_put_a_missing_mirror_first_and_batch_the_new_orgs() {
        assert!(ORDERED_ORGS_SQL.contains("LEFT JOIN org_learning_policy_mirror mirror"));
        assert!(ORDERED_ORGS_SQL.contains("ORDER BY mirror.fetched_at ASC NULLS FIRST"));
        assert!(NEW_ORGS_SQL.contains("NOT EXISTS (SELECT 1 FROM org_learning_policy_mirror m"));
        assert!(NEW_ORGS_SQL.contains("LIMIT $1"));
        assert_eq!(NEW_ORG_BATCH, 50);
    }

    #[test]
    fn the_new_org_tick_reads_orgs_not_threads() {
        // Steady state reads no thread: no aggregate over an anti-join, and each org is one index probe
        // (the `ORDER BY ... LIMIT 1` steps). The orgs still in a backoff arrive as $2 and are left out.
        assert!(
            !NEW_ORGS_SQL.contains("GROUP BY"),
            "no aggregate over every thread"
        );
        assert!(NEW_ORGS_SQL.contains("WITH RECURSIVE"));
        assert!(
            NEW_ORGS_SQL.contains("ORDER BY t.org_id LIMIT 1"),
            "one index probe per org"
        );
        assert!(NEW_ORGS_SQL.contains("n.org_id > o.org_id"));
        assert!(
            NEW_ORGS_SQL.contains("ANY($2::text[])"),
            "backed-off orgs are bound, not refetched"
        );
    }

    #[test]
    fn a_failing_new_org_waits_longer_each_time_up_to_the_cap() {
        let tick = Duration::from_secs(5);
        let waits: Vec<u64> = (1..=7)
            .map(|failures| backoff_for(failures, tick).as_secs())
            .collect();
        assert_eq!(waits, [5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(
            backoff_for(0, tick),
            tick,
            "a count of zero reads as the first failure"
        );
        assert_eq!(backoff_for(u32::MAX, tick), MAX_NEW_ORG_BACKOFF);
    }

    #[test]
    fn a_failing_org_is_held_back_and_logged_less_often_until_it_refreshes() {
        let tick = Duration::from_secs(5);
        let start = Instant::now();
        let mut backoff = NewOrgBackoff::default();
        assert!(
            backoff.settle("org-down", false, start, tick),
            "the first failure is logged"
        );
        assert_eq!(backoff.waiting(start), ["org-down"]);
        assert!(
            backoff.waiting(start + tick).is_empty(),
            "the wait is one tick"
        );
        // Failures two to four are logged at two and four only.
        let mut logged = vec![true];
        for _ in 0..3 {
            let due = backoff.failing["org-down"].retry_at;
            logged.push(backoff.settle("org-down", false, due, tick));
        }
        assert_eq!(logged, [true, true, false, true]);
        // A refresh that works forgets the org, and its next failure starts a new episode.
        let now = backoff.failing["org-down"].retry_at;
        assert!(!backoff.settle("org-down", true, now, tick));
        assert!(backoff.failing.is_empty());
        assert!(
            backoff.settle("org-down", false, now, tick),
            "a new episode is logged again"
        );
    }

    #[test]
    fn the_tick_remembers_a_bounded_number_of_failing_orgs() {
        let tick = Duration::from_secs(5);
        let now = Instant::now();
        let mut backoff = NewOrgBackoff::default();
        for n in 0..NEW_ORG_TRACKED_MAX + 10 {
            backoff.settle(&format!("org-{n:05}"), false, now, tick);
        }
        assert_eq!(backoff.failing.len(), NEW_ORG_TRACKED_MAX);
    }

    #[test]
    fn the_startup_guard_refuses_fail_closed_without_credentials_and_names_no_value() {
        let guard = [(STARTUP_GUARD_ENV, "true")];
        let refused = startup_check_with(&env(&guard), true)
            .expect_err("guard on, fail-closed and no credentials");
        let message = format!("{refused}");
        assert!(message.contains(ORG_CORE_URL_ENV), "{message}");
        assert!(message.contains(SERVICE_TOKEN_ENV), "{message}");
        assert!(!message.contains("token-value-secret"), "{message}");
        assert!(
            startup_check_with(&env(&guard), false).is_ok(),
            "not failing closed boots as it always did"
        );
        assert!(
            startup_check_with(&env(&[]), true).is_ok(),
            "with the guard off the boot is unchanged"
        );
        let mut only_url = guard.to_vec();
        only_url.push((ORG_CORE_URL_ENV, "http://org-core:8080"));
        assert!(
            startup_check_with(&env(&only_url), true).is_err(),
            "a URL without a token is not credentials"
        );
        let mut complete = guard.to_vec();
        complete.extend([
            (ORG_CORE_URL_ENV, "http://org-core:8080"),
            (SERVICE_TOKEN_ENV, "token-value-secret"),
        ]);
        assert!(startup_check_with(&env(&complete), true).is_ok());
    }

    /// The bound: however many orgs a poll has, no more than `ORG_POLICY_POLL_CONCURRENCY` fetches are in
    /// flight at once, and the fetches do overlap. Measured on the fetch against a mock org-core.
    #[tokio::test]
    async fn no_more_than_eight_org_fetches_are_in_flight() {
        let server = MockServer::start().await;
        Mock::given(path_regex(r"^/internal/orgs/[^/]+/agent-policy$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        r#"{"memory":"allow","learningReview":"disabled","revision":1}"#,
                    )
                    .set_delay(Duration::from_millis(30)),
            )
            .mount(&server)
            .await;
        let poller = poller(&server);
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let orgs: Vec<String> = (0..24).map(|n| format!("org-{n:02}")).collect();
        let refreshed = refresh_concurrently(
            orgs,
            ORG_POLICY_POLL_CONCURRENCY,
            {
                let (poller, in_flight, peak) = (poller.clone(), in_flight.clone(), peak.clone());
                move |org_id: String| {
                    let (poller, in_flight, peak) =
                        (poller.clone(), in_flight.clone(), peak.clone());
                    async move {
                        let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        let outcome = poller.fetch(&org_id).await.map(|_| ());
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        (org_id, outcome)
                    }
                }
            },
            |_, outcome| outcome.is_ok(),
        )
        .await;
        assert_eq!(refreshed, 24, "every fetch was answered");
        let peak = peak.load(Ordering::SeqCst);
        assert!(
            peak <= ORG_POLICY_POLL_CONCURRENCY,
            "at most eight in flight, saw {peak}"
        );
        assert!(peak > 1, "the fetches run side by side, saw {peak}");
    }

    // ---- Postgres ----

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn policy_poll_marks_disabled_and_sets_watermark() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let org = format!("org-policy-{}", mp_ids::new_ulid());

        let server = MockServer::start().await;
        let poller = poller(&server);
        let answer = |memory: &str, revision: i64| {
            ResponseTemplate::new(200).set_body_string(format!(
                r#"{{"memory":"{memory}","learningReview":"disabled","revision":{revision}}}"#
            ))
        };
        let route = format!("/internal/orgs/{org}/agent-policy");

        // First sight of a disabled org: stored as disabled, no watermark yet.
        Mock::given(path(route.clone()))
            .respond_with(answer("disabled", 1))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        poller.refresh_org(&pool, &org).await.unwrap();
        let (allowed, since, revision): (bool, Option<DateTime<Utc>>, i64) = sqlx::query_as(
            "SELECT memory_allowed, memory_enabled_since, revision FROM org_learning_policy_mirror WHERE org_id = $1",
        )
        .bind(&org)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!allowed);
        assert!(since.is_none());
        assert_eq!(revision, 1);

        // Re-enabled: the watermark is set to the moment the poll stored it.
        let before = Utc::now() - chrono::Duration::seconds(1);
        server.reset().await;
        Mock::given(path(route.clone()))
            .respond_with(answer("allow", 2))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        poller.refresh_org(&pool, &org).await.unwrap();
        let (allowed, since): (bool, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT memory_allowed, memory_enabled_since FROM org_learning_policy_mirror WHERE org_id = $1",
        )
        .bind(&org)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(allowed);
        let since = since.expect("the transition into allowed sets the watermark");
        assert!(since >= before);

        // A further poll of an allowed org leaves the watermark alone.
        poller.refresh_org(&pool, &org).await.unwrap();
        let (again,): (Option<DateTime<Utc>>,) = sqlx::query_as(
            "SELECT memory_enabled_since FROM org_learning_policy_mirror WHERE org_id = $1",
        )
        .bind(&org)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(again, Some(since));

        // A failed poll keeps the row (it only ages).
        server.reset().await;
        Mock::given(path(route))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(poller.refresh_org(&pool, &org).await.is_err());
        let (allowed,): (bool,) =
            sqlx::query_as("SELECT memory_allowed FROM org_learning_policy_mirror WHERE org_id = $1")
                .bind(&org)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(allowed);
    }

    /// The SQL conjunct Dreaming and the write-time check share, run over two
    /// synthetic messages (one before and one after the watermark) of one org.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn pending_filter_leaves_out_what_org_policy_forbids() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let sql = format!(
            "SELECT m.id FROM (VALUES ('before', now() - interval '1 hour'), ('after', now() + interval '1 hour')) AS m(id, created_at)              CROSS JOIN (SELECT $4::text AS org_id) t WHERE true {} ORDER BY m.id",
            pending_filter(1, 2, 3)
        );
        let ids = |org: String, active: bool, fail_closed: bool| {
            let pool = pool.clone();
            let sql = sql.clone();
            async move {
                sqlx::query_scalar::<_, String>(&sql)
                    .bind(active)
                    .bind(fail_closed)
                    .bind(900i64)
                    .bind(org)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
            }
        };
        let insert = |org: String, allowed: bool, since: &'static str, age: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(&format!(
                    "INSERT INTO org_learning_policy_mirror (org_id, memory_allowed, learning_mode, memory_enabled_since, revision, fetched_at)                      VALUES ($1, $2, 'disabled', {since}, 1, now() - interval '{age}')"
                ))
                .bind(org)
                .bind(allowed)
                .execute(&pool)
                .await
                .unwrap();
            }
        };

        let none = format!("org-filter-none-{}", mp_ids::new_ulid());
        assert_eq!(ids(none.clone(), true, false).await, ["after", "before"], "no row, not failing closed");
        assert!(ids(none, true, true).await.is_empty(), "no row, failing closed");

        let open = format!("org-filter-open-{}", mp_ids::new_ulid());
        insert(open.clone(), true, "NULL", "1 minute").await;
        assert_eq!(ids(open.clone(), true, true).await, ["after", "before"], "fresh and never disabled");
        assert_eq!(ids(open, false, true).await, ["after", "before"], "an inactive filter reads nothing");

        let watermarked = format!("org-filter-wm-{}", mp_ids::new_ulid());
        insert(watermarked.clone(), true, "now()", "1 minute").await;
        assert_eq!(ids(watermarked, true, false).await, ["after"], "the disabled period is never mined");

        let disabled = format!("org-filter-off-{}", mp_ids::new_ulid());
        insert(disabled.clone(), false, "NULL", "1 minute").await;
        assert!(ids(disabled, true, false).await.is_empty());

        let stale = format!("org-filter-stale-{}", mp_ids::new_ulid());
        insert(stale.clone(), true, "NULL", "1 hour").await;
        assert!(ids(stale.clone(), true, true).await.is_empty(), "stale and failing closed");
        assert_eq!(ids(stale, true, false).await, ["after", "before"], "stale but not failing closed");
    }

    /// The fast lane (`ORG_POLICY_FAST_LANE`): an org with a thread and no mirror row comes first in the
    /// poll's order, ahead of an org whose mirror is stale, and one tick of the new-org poll refreshes it.
    /// Needs a disposable Postgres.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn the_fast_lane_refreshes_a_new_org_within_one_tick() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = mp_ids::new_ulid();
        let stale_org = format!("org-fast-stale-{suffix}");
        let new_org = format!("org-fast-new-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'fast-lane-user')",
        )
        .bind(format!("thr-stale-{suffix}"))
        .bind(&stale_org)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO org_learning_policy_mirror \
                 (org_id, memory_allowed, learning_mode, memory_enabled_since, revision, fetched_at) \
             VALUES ($1, true, 'disabled', NULL, 1, now() - interval '1 hour')",
        )
        .bind(&stale_org)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'fast-lane-user')",
        )
        .bind(format!("thr-new-{suffix}"))
        .bind(&new_org)
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(path(format!("/internal/orgs/{new_org}/agent-policy")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"memory":"allow","learningReview":"proposals","revision":3}"#,
                ),
            )
            .mount(&server)
            .await;
        let poller = poller(&server);

        let ordered: Vec<String> = sqlx::query_scalar(ORDERED_ORGS_SQL)
            .fetch_all(&pool)
            .await
            .unwrap();
        let position = |org: &str| ordered.iter().position(|candidate| candidate == org);
        assert!(
            position(new_org.as_str()) < position(stale_org.as_str()),
            "no mirror row goes before a stale one"
        );

        let mut backoff = NewOrgBackoff::default();
        assert!(
            poller.poll_new_orgs(&pool, &mut backoff).await.unwrap() >= 1,
            "the tick refreshed the new org"
        );
        let (allowed, learning): (bool, String) = sqlx::query_as(
            "SELECT memory_allowed, learning_mode FROM org_learning_policy_mirror WHERE org_id = $1",
        )
        .bind(&new_org)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(allowed);
        assert_eq!(learning, "proposals");
    }

    /// The tick's batch lists each missing org once, leaves out an org that has a mirror row, and leaves out
    /// an org bound as backed off. Needs a disposable Postgres.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn the_new_org_query_lists_each_missing_org_once_and_skips_the_backed_off() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = mp_ids::new_ulid();
        let missing = format!("org-list-missing-{suffix}");
        let mirrored = format!("org-list-mirrored-{suffix}");
        let backed_off = format!("org-list-backed-off-{suffix}");
        for (id, org) in [
            (format!("thr-list-a-{suffix}"), &missing),
            (format!("thr-list-b-{suffix}"), &missing),
            (format!("thr-list-c-{suffix}"), &mirrored),
            (format!("thr-list-d-{suffix}"), &backed_off),
        ] {
            sqlx::query(
                "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'fast-lane-user')",
            )
            .bind(id)
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO org_learning_policy_mirror (org_id, memory_allowed, learning_mode, revision) \
             VALUES ($1, true, 'disabled', 1)",
        )
        .bind(&mirrored)
        .execute(&pool)
        .await
        .unwrap();

        let listed: Vec<String> = sqlx::query_scalar(NEW_ORGS_SQL)
            .bind(100_000i64)
            .bind(vec![backed_off.clone()])
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            listed.iter().filter(|org| **org == missing).count(),
            1,
            "listed once"
        );
        assert!(
            !listed.contains(&mirrored),
            "an org with a mirror row is not missing"
        );
        assert!(
            !listed.contains(&backed_off),
            "an org in its backoff is left out"
        );
    }

    /// A failing org is asked for once and then waits out its backoff, so a second tick straight after does not
    /// ask org-core again. Needs a disposable Postgres.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn the_fast_lane_tick_waits_out_a_failing_org() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // Sorts before the other test orgs, so the tick's first batch reaches it whatever else the database holds.
        let failing = format!("0-fast-fail-{}", mp_ids::new_ulid());
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'fast-lane-user')",
        )
        .bind(format!("thr-fail-{failing}"))
        .bind(&failing)
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(path(format!("/internal/orgs/{failing}/agent-policy")))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        let poller = poller(&server);
        let mut backoff = NewOrgBackoff::default();
        poller.poll_new_orgs(&pool, &mut backoff).await.unwrap();
        poller.poll_new_orgs(&pool, &mut backoff).await.unwrap();
        server.verify().await;
        let (rows,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM org_learning_policy_mirror WHERE org_id = $1")
                .bind(&failing)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows, 0, "a failing org gets no mirror row");
    }
}
