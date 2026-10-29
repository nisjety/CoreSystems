//! The built-in scheduler: which tier of endpoints runs for which tenant, and when.
//!
//! Three tiers over each tenant's endpoints:
//!
//! * `Hourly`: endpoints with `tier: 1`, incremental from their checkpoints;
//! * `Daily`: endpoints with `tier: 2` or `3` (no tier counts as 2), incremental;
//! * `Full`: every enabled endpoint, over its whole scope and ignoring checkpoints. Only a full
//!   scan can tell that Visma no longer has a record, so this is the pass that reconciles
//!   deletes. Tenants that have merged into another (`historical`) get this pass only.
//!
//! What is due is decided from `vsv2.scheduler_pass`, not from timers in memory. A restart, a
//! machine that slept or a second instance neither shifts nor repeats the schedule, and a pass
//! that ended with failures is retried for just the endpoints that failed.
//!
//! Syncs of one tenant never overlap: each endpoint takes the tenant's lock (and its Redis lease,
//! so two scheduler processes keep out of each other's way) for the length of that one sync, which
//! lets an hourly pass slip in between the endpoints of a long full pass.
//!
//! An endpoint with `open_set: true` also gets its open documents reconciled against Visma, at
//! most once a day per tenant, at the end of its hourly sync (see [`crate::open_set`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tracing::{info, warn};
use uuid::Uuid;
use vsv2_common::{Result, TenantConfig, TenantId, TenantStatus};
use vsv2_state::scheduler_pass::{self, LastPass, PassHistory, PassStatus};
use vsv2_state::StateStore;

use crate::executor::SyncExecutor;

/// How long a tenant lease lives without being renewed. The pass renews it every heartbeat, so a
/// process that dies frees its tenants after this long, not after the longest sync.
pub const LEASE_TTL_SECS: u64 = 5 * 60;

/// How often a running pass tells the state database it is alive.
pub const PASS_HEARTBEAT: Duration = Duration::from_secs(60);

/// A pass still `running` whose heartbeat is this old belongs to a process that is gone. Ten
/// missed beats: a stalled database connection or a short sleep does not abandon a live pass.
pub const STALE_PASS_AFTER: chrono::Duration = chrono::Duration::minutes(10);

/// Beats for one pass while it lives: the pass's heartbeat, and the tenant lease while this
/// process holds it. Stops when dropped, also when the pass ends in an error or the process dies
/// (then nothing beats, which is how the next poll knows).
struct Heartbeat(tokio::task::JoinHandle<()>);

impl Heartbeat {
    fn start(
        state: StateStore,
        pass_id: Uuid,
        tenant: TenantId,
        lease_owner: Option<String>,
    ) -> Self {
        Self(tokio::spawn(async move {
            let mut every = tokio::time::interval(PASS_HEARTBEAT);
            every.tick().await; // the first tick is immediate; start_pass just set the time
            loop {
                every.tick().await;
                if let Err(e) = scheduler_pass::heartbeat(&state.pg, pass_id).await {
                    warn!(err = %e, %pass_id, "scheduler: could not record the heartbeat");
                }
                if let Some(owner) = &lease_owner {
                    if let Err(e) = state.renew_lease(&tenant, owner, LEASE_TTL_SECS).await {
                        warn!(err = %e, %tenant, "scheduler: could not renew the tenant lease");
                    }
                }
            }
        }))
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Hourly,
    Daily,
    Full,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Full => "full",
        }
    }

    /// Whether the tier runs for a tenant of this status.
    pub fn covers(self, status: TenantStatus) -> bool {
        match (self, status) {
            (Self::Full, _) => true,
            (_, TenantStatus::Active) => true,
            (_, TenantStatus::Historical) => false,
        }
    }

    /// Whether a pass of this tier fetches the whole scope and reconciles deletes.
    pub fn is_full_scope(self) -> bool {
        self == Self::Full
    }

    /// How long after a failed pass the failed endpoints are tried again.
    pub fn retry_after(self) -> Duration {
        match self {
            Self::Hourly => Duration::from_secs(15 * 60),
            Self::Daily => Duration::from_secs(2 * 3600),
            Self::Full => Duration::from_secs(12 * 3600),
        }
    }
}

/// One tier: how often it runs and which endpoints it covers.
#[derive(Debug, Clone)]
pub struct TierSpec {
    pub tier: Tier,
    pub interval: Duration,
    pub endpoints: Vec<String>,
}

impl TierSpec {
    /// The three tiers of an executor's registry, with the given intervals.
    pub fn standard(
        exec: &SyncExecutor,
        hourly: Duration,
        daily: Duration,
        full: Duration,
    ) -> Vec<Self> {
        let mut every_endpoint = exec.endpoints_for_tier(0, u8::MAX);
        every_endpoint.sort();
        vec![
            Self {
                tier: Tier::Hourly,
                interval: hourly,
                endpoints: exec.endpoints_for_tier(1, 1),
            },
            Self {
                tier: Tier::Daily,
                interval: daily,
                endpoints: exec.endpoints_for_tier(2, 3),
            },
            Self {
                tier: Tier::Full,
                interval: full,
                endpoints: every_endpoint,
            },
        ]
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledTenant {
    pub id: TenantId,
    pub name: String,
    pub status: TenantStatus,
}

impl From<&TenantConfig> for ScheduledTenant {
    fn from(config: &TenantConfig) -> Self {
        Self {
            id: TenantId::new(config.tenant_id.clone()),
            name: config.tenant_name.clone(),
            status: config.status,
        }
    }
}

/// What the history says should happen next for one tier and tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Due {
    Nothing,
    /// A complete pass over the tier's endpoints.
    Full,
    /// Only the endpoints a failed pass left, repeating pass `of`.
    Retry {
        of: Uuid,
        names: Vec<String>,
    },
}

pub fn decide(history: &PassHistory, tier: Tier, interval: Duration, now: DateTime<Utc>) -> Due {
    let age = |at: DateTime<Utc>| (now - at).to_std().unwrap_or_default();

    if let Some(last) = &history.last {
        if last.status == PassStatus::Running && now - last.heartbeat_at < STALE_PASS_AFTER {
            return Due::Nothing; // someone is on it
        }
    }
    let complete_due = history
        .last_complete_started_at
        .is_none_or(|started| age(started) >= interval);
    if complete_due {
        return Due::Full;
    }
    match &history.last {
        Some(LastPass {
            pass_id,
            status: PassStatus::Failed,
            started_at,
            failed_names,
            ..
        }) if !failed_names.is_empty() && age(*started_at) >= tier.retry_after() => Due::Retry {
            of: *pass_id,
            names: failed_names.clone(),
        },
        _ => Due::Nothing,
    }
}

/// Whether an endpoint's sync is followed by its open-set reconciliation (which does nothing for
/// an endpoint without `open_set`): after a successful sync of the hourly tier, while the
/// scheduler is not shutting down. The weekly full pass rewrites every row anyway, and a sync that
/// failed says Visma or BigQuery is in no state to be asked more.
fn reconciles_open_set(tier: Tier, synced: bool, stopping: bool) -> bool {
    tier == Tier::Hourly && synced && !stopping
}

/// What one pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassReport {
    pub tier: Tier,
    pub tenant: TenantId,
    pub attempted: usize,
    pub failed: Vec<String>,
    pub records: u64,
}

pub struct Scheduler {
    exec: Arc<SyncExecutor>,
    tenants: Vec<ScheduledTenant>,
    locks: HashMap<TenantId, Arc<Mutex<()>>>,
    parallel: Arc<Semaphore>,
    /// Owner id for the Redis tenant lease; `None` when the process is the only scheduler.
    lease_owner: Option<String>,
    in_flight: Mutex<HashSet<(Tier, TenantId)>>,
    stop: Arc<AtomicBool>,
}

impl Scheduler {
    pub fn new(
        exec: Arc<SyncExecutor>,
        tenants: Vec<ScheduledTenant>,
        parallel_tenants: usize,
        lease_owner: Option<String>,
    ) -> Arc<Self> {
        let locks = tenants
            .iter()
            .map(|t| (t.id.clone(), Arc::new(Mutex::new(()))))
            .collect();
        Arc::new(Self {
            exec,
            tenants,
            locks,
            parallel: Arc::new(Semaphore::new(parallel_tenants.max(1))),
            lease_owner,
            in_flight: Mutex::new(HashSet::new()),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Ask running passes to end after their current endpoint.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Start a pass for every tier and tenant that is due, without waiting for it.
    pub async fn spawn_due(
        self: &Arc<Self>,
        specs: &[TierSpec],
        passes: &mut JoinSet<Result<PassReport>>,
    ) -> Result<usize> {
        let abandoned =
            scheduler_pass::abandon_stale(&self.exec.state.pg, STALE_PASS_AFTER).await?;
        if abandoned > 0 {
            warn!(
                abandoned,
                "scheduler: marked passes whose process is gone as failed, they run again"
            );
        }
        let now = Utc::now();
        let mut started = 0;
        for spec in specs {
            for tenant in &self.tenants {
                if !spec.tier.covers(tenant.status) || spec.endpoints.is_empty() {
                    continue;
                }
                let key = (spec.tier, tenant.id.clone());
                if self.in_flight.lock().await.contains(&key) {
                    continue;
                }
                let history =
                    scheduler_pass::history(&self.exec.state.pg, spec.tier.label(), &tenant.id)
                        .await?;
                let (names, retry_of) = match decide(&history, spec.tier, spec.interval, now) {
                    Due::Nothing => continue,
                    Due::Full => (spec.endpoints.clone(), None),
                    Due::Retry { of, names } => (names, Some(of)),
                };
                self.in_flight.lock().await.insert(key);
                let this = Arc::clone(self);
                let (tier, tenant) = (spec.tier, tenant.clone());
                passes.spawn(async move { this.run_pass(tier, tenant, names, retry_of).await });
                started += 1;
            }
        }
        Ok(started)
    }

    /// Run everything that is due and wait for it. For tests and `schedule --once`.
    pub async fn run_due_once(self: &Arc<Self>, specs: &[TierSpec]) -> Result<Vec<PassReport>> {
        let mut passes = JoinSet::new();
        self.spawn_due(specs, &mut passes).await?;
        let mut reports = Vec::new();
        while let Some(done) = passes.join_next().await {
            match done {
                Ok(report) => reports.push(report?),
                Err(e) => warn!(err = %e, "scheduler: a pass panicked"),
            }
        }
        Ok(reports)
    }

    /// Check what is due every `poll` until `shutdown` resolves, then let running passes finish
    /// their current endpoint.
    pub async fn run_forever(
        self: &Arc<Self>,
        specs: &[TierSpec],
        poll: Duration,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        let mut passes: JoinSet<Result<PassReport>> = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            match self.spawn_due(specs, &mut passes).await {
                Ok(0) => {}
                Ok(n) => info!(passes = n, "scheduler: started passes"),
                Err(e) => warn!(err = %e, "scheduler: could not work out what is due"),
            }
            while let Some(done) = passes.try_join_next() {
                match done {
                    Ok(Ok(report)) => info!(
                        tier = report.tier.label(),
                        tenant = %report.tenant,
                        attempted = report.attempted,
                        failed = report.failed.len(),
                        records = report.records,
                        "scheduler: pass finished"
                    ),
                    Ok(Err(e)) => warn!(err = %e, "scheduler: pass ended with an error"),
                    Err(e) => warn!(err = %e, "scheduler: a pass panicked"),
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = &mut shutdown => {
                    info!("scheduler: shutting down, passes end after their current endpoint");
                    self.stop();
                    while passes.join_next().await.is_some() {}
                    return Ok(());
                }
            }
        }
    }

    async fn run_pass(
        self: Arc<Self>,
        tier: Tier,
        tenant: ScheduledTenant,
        names: Vec<String>,
        retry_of: Option<Uuid>,
    ) -> Result<PassReport> {
        let outcome = self.run_pass_inner(tier, &tenant, &names, retry_of).await;
        self.in_flight
            .lock()
            .await
            .remove(&(tier, tenant.id.clone()));
        outcome
    }

    async fn run_pass_inner(
        &self,
        tier: Tier,
        tenant: &ScheduledTenant,
        names: &[String],
        retry_of: Option<Uuid>,
    ) -> Result<PassReport> {
        let _permit = self.parallel.acquire().await.ok();
        let pool = &self.exec.state.pg;
        let pass_id = Uuid::new_v4();
        scheduler_pass::start_pass(pool, pass_id, tier.label(), &tenant.id, retry_of).await?;
        let _alive = Heartbeat::start(
            self.exec.state.clone(),
            pass_id,
            tenant.id.clone(),
            self.lease_owner.clone(),
        );
        info!(
            tier = tier.label(),
            tenant = %tenant.id,
            name = %tenant.name,
            endpoints = names.len(),
            retry = retry_of.is_some(),
            "scheduler: pass start"
        );

        let (mut records, mut failed, mut error) = (0u64, Vec::new(), None);
        for (index, name) in names.iter().enumerate() {
            if self.stop.load(Ordering::SeqCst) {
                failed.extend_from_slice(&names[index..]);
                error = Some("stopped before the pass was done".to_string());
                break;
            }
            match self.sync_guarded(tier, tenant, name).await {
                Ok(Some(n)) => records += n,
                Ok(None) => {
                    failed.extend_from_slice(&names[index..]);
                    error = Some("another scheduler holds this tenant".to_string());
                    break;
                }
                Err(e) => {
                    warn!(tenant = %tenant.id, endpoint = %name, err = %e, "scheduler: endpoint failed");
                    failed.push(name.clone());
                }
            }
        }
        let status = if failed.is_empty() {
            PassStatus::Succeeded
        } else {
            PassStatus::Failed
        };
        scheduler_pass::finish_pass(
            pool,
            pass_id,
            status,
            names.len(),
            &failed,
            records,
            error.as_deref(),
        )
        .await?;
        Ok(PassReport {
            tier,
            tenant: tenant.id.clone(),
            attempted: names.len(),
            failed,
            records,
        })
    }

    /// One endpoint sync under the tenant's lock. `None` when another scheduler holds the tenant.
    async fn sync_guarded(
        &self,
        tier: Tier,
        tenant: &ScheduledTenant,
        endpoint: &str,
    ) -> Result<Option<u64>> {
        let lock = self.locks.get(&tenant.id).cloned().unwrap_or_default();
        let _held = lock.lock().await;
        if let Some(owner) = &self.lease_owner {
            let state = &self.exec.state;
            if !state
                .try_acquire_lease(&tenant.id, owner, LEASE_TTL_SECS)
                .await
                .unwrap_or(false)
            {
                return Ok(None);
            }
        }
        let result = if tier.is_full_scope() {
            self.exec.sync_one_full(&tenant.id, endpoint).await
        } else {
            self.exec.sync_one(&tenant.id, endpoint).await
        };
        // An endpoint with `open_set` also compares its open documents with Visma's once a day:
        // a payment closes an invoice without moving the timestamp the sync above follows. It runs
        // after a successful hourly sync only (the weekly full pass already rewrites everything),
        // under the same tenant lock, and cannot fail the pass: it reports to the log and to
        // `sync_run`, not to this result. It is inline, so the tenant's other endpoints wait for
        // it, at most `open_set::RUN_TIMEOUT` (4 minutes) per flagged endpoint.
        if reconciles_open_set(tier, result.is_ok(), self.stop.load(Ordering::SeqCst)) {
            self.exec
                .reconcile_open_set_if_due(&tenant.id, endpoint, &self.stop)
                .await;
        }
        if self.lease_owner.is_some() {
            let _ = self.exec.state.release_lease(&tenant.id).await;
        }
        result.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().expect("RFC3339 timestamp")
    }

    fn hour() -> Duration {
        Duration::from_secs(3600)
    }

    fn pass(status: PassStatus, started: &str, failed: &[&str]) -> LastPass {
        LastPass {
            pass_id: Uuid::nil(),
            status,
            started_at: at(started),
            heartbeat_at: at(started),
            failed_names: failed.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn a_tier_that_never_ran_is_due() {
        let history = PassHistory::default();

        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T10:00:00Z")),
            Due::Full
        );
    }

    #[test]
    fn a_tier_is_due_again_once_its_interval_has_passed_since_the_last_complete_pass() {
        let history = PassHistory {
            last: Some(pass(PassStatus::Succeeded, "2026-10-02T09:00:00Z", &[])),
            last_complete_started_at: Some(at("2026-10-02T09:00:00Z")),
        };

        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T09:59:00Z")),
            Due::Nothing
        );
        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T10:00:00Z")),
            Due::Full
        );
    }

    #[test]
    fn a_pass_that_is_still_beating_is_left_alone_however_long_it_runs() {
        let mut last = pass(PassStatus::Running, "2026-10-02T08:00:00Z", &[]);
        last.heartbeat_at = at("2026-10-02T11:59:30Z");
        let running = PassHistory {
            last: Some(last),
            last_complete_started_at: Some(at("2026-10-01T08:00:00Z")),
        };

        assert_eq!(
            decide(&running, Tier::Daily, hour(), at("2026-10-02T12:00:00Z")),
            Due::Nothing,
            "four hours in, but alive: a full pass can take that long"
        );
    }

    #[test]
    fn a_pass_whose_heartbeat_stopped_is_not_waited_for() {
        let running = PassHistory {
            last: Some(pass(PassStatus::Running, "2026-10-02T11:00:00Z", &[])),
            last_complete_started_at: Some(at("2026-10-01T08:00:00Z")),
        };

        assert_eq!(
            decide(&running, Tier::Daily, hour(), at("2026-10-02T11:11:00Z")),
            Due::Full,
            "silent for eleven minutes: its process is gone"
        );
        assert_eq!(
            decide(&running, Tier::Daily, hour(), at("2026-10-02T11:05:00Z")),
            Due::Nothing,
            "silent for five minutes: still within the allowance"
        );
    }

    #[test]
    fn a_failed_pass_is_retried_for_the_failed_endpoints_only_after_the_retry_delay() {
        let history = PassHistory {
            last: Some(pass(
                PassStatus::Failed,
                "2026-10-02T09:00:00Z",
                &["customers", "shipments"],
            )),
            last_complete_started_at: Some(at("2026-10-02T09:00:00Z")),
        };
        let interval = Duration::from_secs(24 * 3600);

        assert_eq!(
            decide(&history, Tier::Hourly, interval, at("2026-10-02T09:10:00Z")),
            Due::Nothing,
            "too soon"
        );
        assert_eq!(
            decide(&history, Tier::Hourly, interval, at("2026-10-02T09:15:00Z")),
            Due::Retry {
                of: Uuid::nil(),
                names: vec!["customers".into(), "shipments".into()]
            }
        );
    }

    #[test]
    fn a_retry_does_not_move_the_interval_of_the_complete_pass() {
        // The complete pass began at 06:00 and failed; its retry at 07:00 succeeded.
        let history = PassHistory {
            last: Some(pass(PassStatus::Succeeded, "2026-10-02T07:00:00Z", &[])),
            last_complete_started_at: Some(at("2026-10-02T06:00:00Z")),
        };

        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T07:05:00Z")),
            Due::Full,
            "the hour counts from 06:00, not from the retry"
        );
        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T06:30:00Z")),
            Due::Nothing
        );
    }

    #[test]
    fn a_failed_pass_with_no_names_cannot_be_retried_so_it_waits_for_the_interval() {
        let history = PassHistory {
            last: Some(pass(PassStatus::Failed, "2026-10-02T09:00:00Z", &[])),
            last_complete_started_at: Some(at("2026-10-02T09:00:00Z")),
        };

        assert_eq!(
            decide(&history, Tier::Hourly, hour(), at("2026-10-02T09:30:00Z")),
            Due::Nothing
        );
    }

    #[test]
    fn historical_tenants_get_the_full_pass_only() {
        assert!(Tier::Full.covers(TenantStatus::Historical));
        assert!(!Tier::Hourly.covers(TenantStatus::Historical));
        assert!(!Tier::Daily.covers(TenantStatus::Historical));
        assert!(Tier::Hourly.covers(TenantStatus::Active));
        assert!(Tier::Daily.covers(TenantStatus::Active));
        assert!(Tier::Full.covers(TenantStatus::Active));
    }

    #[test]
    fn open_set_follows_a_successful_hourly_sync_and_nothing_else() {
        assert!(reconciles_open_set(Tier::Hourly, true, false));
        assert!(
            !reconciles_open_set(Tier::Hourly, false, false),
            "the sync failed"
        );
        assert!(
            !reconciles_open_set(Tier::Hourly, true, true),
            "shutting down"
        );
        assert!(!reconciles_open_set(Tier::Daily, true, false));
        assert!(!reconciles_open_set(Tier::Full, true, false));
    }

    #[test]
    fn only_the_full_tier_fetches_the_whole_scope() {
        assert!(Tier::Full.is_full_scope());
        assert!(!Tier::Hourly.is_full_scope());
        assert!(!Tier::Daily.is_full_scope());
    }
}
