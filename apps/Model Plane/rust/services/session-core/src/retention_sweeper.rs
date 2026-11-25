//! Retention sweeper (Wave 3 EP-9 C2 dry run, C3 delete mode).
//!
//! Finds the personal threads that have been idle past their org's retention window
//! and records, per org and pass, how many it would delete. In `enforce` it deletes
//! them too (see "Delete mode"); that is switched on per environment only after the
//! owner reviewed a week of dry-run counts.
//!
//! # Where the policy comes from
//!
//! Control owns the window and the legal holds (org-core, EP-9 C1). Session-core asks
//! `POST /internal/orgs/:orgId/retention/check` with its own principal (scope
//! `org:retention-policy:read:any`, held by session-core alone) and never stores the
//! answer. The orgs asked are the ones that own personal threads here; an org
//! org-core does not know (404) or with no window set keeps everything.
//!
//! # Fail closed
//!
//! Any answer that is not a clean 200 for the right org at one policy revision means
//! "unknown", and an unknown policy yields no candidates: the pass writes its row
//! with an `error_code` and zero counts. A window outside 30..3650 days (D-C2) is
//! refused the same way rather than clamped, because a clamped window is a window
//! nobody set.
//!
//! # What counts as a candidate (D-C2)
//!
//! A thread with no Space id and no Space kind (Space threads are out of scope in
//! W3), whose last activity (its newest message, or the latest change of one of its
//! runs, whichever is later) is older than the window, and with no run still open.
//! Held threads, and every thread while the org is held, are counted as `held`.
//! Threads whose every run was zero-retention hold no content and are counted as
//! `skipped_zdr` without asking org-core about them.
//!
//! # The switch
//!
//! `SESSION_RETENTION_SWEEP` is `off`, `dry_run` or `enforce`; unset or empty is
//! `off`. A value that is blank, or is `off` in any case once its surrounding
//! whitespace is removed, is also `off`: off does nothing, so neither padding nor a
//! capital letter can start a pass nobody asked for. `enforce` is read exactly,
//! never through padding. Any other value is `dry_run`, never `enforce`, because
//! this switch is the one that will delete data: a typo, or a padded `enforce`,
//! must not widen it (a deliberate deviation from the strict "unknown means off"
//! rule, stated in the ledger entry).
//!
//! # Delete mode (C3)
//!
//! With `enforce` the pass selects and counts exactly as the dry run does, then
//! deletes the would-delete threads in batches of [`DELETE_BATCH`]:
//!
//! - Before each batch org-core is asked again about exactly those threads, so a
//!   hold placed after the page was read is honoured; a changed revision or an
//!   unreadable answer stops the pass.
//! - Each thread is deleted in its own transaction that locks the thread row,
//!   re-checks that it is still an idle personal thread with no open run (activity
//!   since the page was read keeps it), and erases it through the user-delete path
//!   (`grpc::delete_thread_rows`) plus the thread-bound rows that path leaves (see
//!   [`delete_residual_rows`]), so every [`THREAD_BOUND_TABLES`] table is covered.
//! - After the commit, never inside it, the semantic memory twins on letta-bridge
//!   are erased (`memory_erasure`) and the thread's sandbox workspace erasure is
//!   asked of a [`WorkspaceEraser`]. Both are best effort and counted, the way the
//!   user delete counts them: an outage there must not undo a delete Postgres made.
//! - Per org, at most [`DEFAULT_DAILY_CAP`] threads are deleted in any 24 hours,
//!   counted from the deletion ledger (below), so a misconfigured window cannot
//!   wipe an org in one pass. `SESSION_RETENTION_SWEEP_DAILY_CAP` can lower the
//!   cap, never raise it. Threads past the cap stay as `would_delete` for a later
//!   pass.
//!
//! A pass that stops after it deleted something still records its row with what it
//! did (the run table admits a policy error only with zero counts); the reason is in
//! the audit event as `stopped`.
//!
//! # The deletion ledger (F-f10-10)
//!
//! Each thread's delete transaction also writes one `retention_sweep_deletions` row
//! (run id, org id and time; no thread id) before it commits, under the run id the
//! pass allocated when it started. The run row and its audit event are still written
//! once, at the end of the org's pass, so a crash, a restart or a failed record
//! between the two used to leave committed deletes with no run row and outside the
//! daily cap. Now:
//!
//! - The daily cap counts the org's ledger rows of the last 24 hours, plus the
//!   `deleted` of the enforce runs recorded before the ledger existed (runs with no
//!   ledger row), so it never counts less than it did.
//! - Each pass first records, under the sweep lock, every run id with ledger rows
//!   and no run row whose last delete is older than [`INTERRUPTED_AFTER_MINUTES`], as
//!   an `enforce` row with `error_code = 'interrupted'` and the ledger count as
//!   `deleted` ([`reconcile_interrupted`]), so every delete has an audit event.
//! - Ledger rows older than [`LEDGER_KEEP_HOURS`] whose run row exists are pruned
//!   ([`prune_ledger`]); a row whose run is not recorded yet is never pruned.
//!
//! # What the delete does not reach
//!
//! - Sandbox workspaces: after the commit, the thread's workspace is erased through
//!   sandbox-manager's service erase (`EraseThreadWorkspaceForRetention`, F-f10-08) by
//!   `sandbox_workspace_eraser::GrpcWorkspaceEraser`, but only while
//!   `SESSION_RETENTION_WORKSPACE_ERASE` is the literal `true` and the sandbox-manager
//!   address and session-core's service credential are configured. Otherwise the eraser
//!   is [`UnavailableWorkspaceEraser`], which counts every deleted thread's workspace as
//!   unconfirmed. `DeleteThreadWorkspace` accepts a user principal only and is never
//!   called from here.
//! - Data Plane v2: no chat-derived content reaches it bound to a thread. Chat
//!   attachments travel inline with the turn and are not stored as documents;
//!   documents a person creates (`/v1/documents`) or promotes from a workspace
//!   (execution-core `workspace_promote`) are that person's knowledge with their own
//!   lifecycle, and session-core holds no document id per thread to delete by. No
//!   Data Plane call is made and none is claimed.
//!
//! # ZDR
//!
//! The run table holds ids, numbers and closed words only, and the audit event
//! carries the same counts. No content is read beyond row counts. Deletion only
//! removes content; threads whose every run was zero-retention are never deleted.
use crate::letta_adapter::LettaMemoryAdapter;
use crate::memory_erasure::SemanticErasure;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tracing::{error, info, warn};

pub(crate) const SWITCH_ENV: &str = "SESSION_RETENTION_SWEEP";
pub(crate) const INTERVAL_SECS_ENV: &str = "SESSION_RETENTION_SWEEP_INTERVAL_SECS";
pub(crate) const DAILY_CAP_ENV: &str = "SESSION_RETENTION_SWEEP_DAILY_CAP";

/// Threads deleted per hold re-check: small enough that a hold placed during a pass
/// is seen within one batch, large enough that org-core is asked rarely.
pub(crate) const DELETE_BATCH: usize = 100;
/// The most threads one org loses in any 24 hours (EP-9 C3).
pub(crate) const DEFAULT_DAILY_CAP: i64 = 10_000;

/// Audit-only reasons a pass that already deleted something stopped early. A pass
/// that deleted nothing records the closed `error_code` instead.
pub(crate) const STOPPED_DELETE_FAILED: &str = "delete_failed";

/// Why a deleted thread's workspace erasure is not confirmed: the eraser is not configured,
/// or sandbox-manager did not confirm the erase (refused, unreachable, or past its deadline).
/// Every such answer is this one closed word.
pub(crate) const WORKSPACE_ERASURE_UNAVAILABLE: &str = "workspace_erasure_unavailable";

/// Six hours: four passes a day is enough for a window measured in days.
const DEFAULT_INTERVAL_SECS: u64 = 6 * 60 * 60;
/// A floor so a misconfigured interval cannot turn the sweep into a hot loop
/// against org-core.
const MIN_INTERVAL_SECS: u64 = 300;

/// The window bounds of D-C2, the same CHECK org-core's table holds.
pub(crate) const MIN_WINDOW_DAYS: i64 = 30;
pub(crate) const MAX_WINDOW_DAYS: i64 = 3650;

/// org-core's `MaxCheckThreadIDs`: the most thread ids one check may name. The
/// candidates are paged at this size, so a page is also one check call.
const CHECK_PAGE: i64 = 500;

/// The principal org-core's credential registry knows this service by.
const SERVICE_ID: &str = "session-core";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// A check answer names at most 500 ids of at most 128 bytes each.
const MAX_BODY_BYTES: usize = 96 * 1024;

/// Session-level advisory lock key, so two replicas do not sweep at the same time.
/// The value is arbitrary; it only has to differ from every other lock key in the
/// database ("retention" in ASCII).
const SWEEP_LOCK_KEY: i64 = 0x7265_7465_6e74_696f;

/// Audit event name; also the last token of the wire subject.
pub(crate) const EVENT_RETENTION_SWEEP: &str = "retention_sweep";

/// The closed vocabulary of `retention_sweep_runs.error_code`.
pub(crate) const ERROR_POLICY_UNAVAILABLE: &str = "policy_unavailable";
pub(crate) const ERROR_POLICY_WINDOW_INVALID: &str = "policy_window_invalid";
pub(crate) const ERROR_POLICY_CHANGED: &str = "policy_changed";
/// A pass that deleted and then never recorded its row (crash, restart, failed
/// record), recorded later from its ledger rows. The one error with deletes.
pub(crate) const ERROR_INTERRUPTED: &str = "interrupted";

/// How long a run's last ledger row must be quiet before a pass records the run as
/// interrupted, so a pass still deleting on a replica that lost its lock is not
/// recorded under it.
pub(crate) const INTERRUPTED_AFTER_MINUTES: i32 = 15;
/// Ledger rows of a recorded run are kept this long: past the 24-hour cap window
/// with a day of margin.
pub(crate) const LEDGER_KEEP_HOURS: i32 = 48;

/// Every session-core table with a `thread_id` column (integration round B, C-B9).
/// The delete mode (EP-9 C3) must erase each of these with its thread, and the dry
/// run counts their rows per would-delete thread so the owner sees what a pass
/// would remove. A migration that adds a `thread_id` column without adding its
/// table here fails `thread_bound_tables_cover_every_migration`.
pub(crate) const THREAD_BOUND_TABLES: &[&str] = &[
    "messages",
    "runs",
    "memory_index",
    "plans",
    "todos",
    "dream_runs",
    "scheduled_step_receipts",
    "run_questions",
    "cron_fires",
    "approval_scope_grants",
    "skill_revisions",
    "space_event_log",
    "message_mentions",
    "message_revisions",
    "artifact_revisions",
    "thread_goals",
    "thread_share_snapshots",
    "research_runs",
];

/// What the switch asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Off,
    DryRun,
    Enforce,
}

/// The values a pass writes to `retention_sweep_runs.mode`: what the pass did, which
/// is `dry_run` whenever it was given no [`Erasure`].
const RECORDED_DRY_RUN: &str = "dry_run";
const RECORDED_ENFORCE: &str = "enforce";

/// Reads the switch (see the module docs). Unset is off, and so is a value that is blank or `off`
/// in any case once surrounding whitespace is removed: off does nothing, so neither padding nor a
/// capital letter can start a pass. `enforce` is read exactly, with no trimming, so a padded
/// `enforce` is `dry_run`. Anything else is `dry_run`, never `enforce`.
pub(crate) fn mode_from(raw: Option<&str>) -> Mode {
    match raw {
        None => Mode::Off,
        Some("enforce") => Mode::Enforce,
        Some(value) => {
            let word = value.trim();
            if word.is_empty() || word.eq_ignore_ascii_case("off") {
                Mode::Off
            } else {
                Mode::DryRun
            }
        }
    }
}

/// Whether a set value is one the startup report names (`switch_spellings`): anything `mode_from`
/// reads as dry run without being one of its words. `enforce` and `dry_run` read as themselves, and
/// blank or `off` in any case once trimmed reads as off. Every other value is named, so a padded or
/// cased `enforce` (which reads as dry run) shows in the logs instead of passing unnoticed.
pub(crate) fn is_unreadable_mode(raw: &str) -> bool {
    if matches!(raw, "enforce" | "dry_run") {
        return false;
    }
    let word = raw.trim();
    !(word.is_empty() || word.eq_ignore_ascii_case("off"))
}

pub(crate) fn mode() -> Mode {
    mode_from(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// The pass interval: the default when unset or unreadable, never below the floor.
pub(crate) fn interval_from(raw: Option<&str>) -> Duration {
    let secs = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
        .max(MIN_INTERVAL_SECS);
    Duration::from_secs(secs)
}

/// The per-org daily cap: the default when unset or unreadable, and never above it,
/// so the variable can only make the sweep more careful.
pub(crate) fn daily_cap_from(raw: Option<&str>) -> i64 {
    raw.and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|cap| *cap >= 0)
        .map_or(DEFAULT_DAILY_CAP, |cap| cap.min(DEFAULT_DAILY_CAP))
}

/// org-core's answer for one org at one policy revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolicyAnswer {
    /// `None` means keep: no window is set.
    pub(crate) window_days: Option<i64>,
    pub(crate) revision: i64,
    pub(crate) org_held: bool,
    pub(crate) held_thread_ids: HashSet<String>,
}

#[derive(Deserialize)]
struct CheckWire {
    organization_id: String,
    window_days: Option<i64>,
    revision: i64,
    org_held: bool,
    #[serde(default)]
    held_thread_ids: Vec<String>,
}

/// Strict reading of a check answer: it must name the org it was asked about, and a
/// negative revision is not a revision.
fn parse_check(org_id: &str, body: &[u8]) -> Result<PolicyAnswer> {
    let wire: CheckWire = serde_json::from_slice(body).context("retention check body is not a check answer")?;
    if wire.organization_id != org_id {
        return Err(anyhow!("retention check answered for another organization"));
    }
    if wire.revision < 0 {
        return Err(anyhow!("retention check revision is negative"));
    }
    Ok(PolicyAnswer {
        window_days: wire.window_days,
        revision: wire.revision,
        org_held: wire.org_held,
        held_thread_ids: wire.held_thread_ids.into_iter().collect(),
    })
}

/// Asks org-core the sweeper's question. Holds a service credential, so it has no
/// `Debug`.
pub(crate) struct CheckClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl CheckClient {
    /// `None` when the credentials are unset. Shares org-core's address and the
    /// session-core credential with the org policy mirror (`org_policy.rs`); the
    /// credential must also hold `org:retention-policy:read:any`.
    pub(crate) fn from_lookup(get: &impl Fn(&str) -> Option<String>) -> Option<Self> {
        let base_url = get(crate::org_policy::ORG_CORE_URL_ENV)?.trim().to_owned();
        let token = get(crate::org_policy::SERVICE_TOKEN_ENV)?.trim().to_owned();
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
        })
    }

    fn check_url(&self, org_id: &str) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&self.base_url).context("ORG_CORE_URL is not a URL")?;
        url.path_segments_mut()
            .map_err(|()| anyhow!("ORG_CORE_URL cannot carry a path"))?
            .pop_if_empty()
            .extend(["internal", "orgs", org_id, "retention", "check"]);
        Ok(url)
    }

    /// One check. `Ok(None)` is org-core's definitive 404: it does not know the org,
    /// so there is no policy and nothing is swept. Every other failure is an error,
    /// which the pass reads as "unknown".
    pub(crate) async fn check(&self, org_id: &str, thread_ids: &[String]) -> Result<Option<PolicyAnswer>> {
        let response = self
            .client
            .post(self.check_url(org_id)?)
            .header("x-service-id", SERVICE_ID)
            .header("x-service-token", &self.token)
            .json(&serde_json::json!({ "thread_ids": thread_ids }))
            .send()
            .await
            .context("retention check request failed")?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(anyhow!("retention check answered {status}"));
        }
        let body = response.bytes().await.context("retention check body unreadable")?;
        if body.len() > MAX_BODY_BYTES {
            return Err(anyhow!("retention check body too large"));
        }
        parse_check(org_id, &body).map(Some)
    }
}

/// One candidate thread of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) thread_id: String,
    /// Every run of the thread was zero-retention, so it holds no content.
    pub(crate) zdr_only: bool,
    /// The thread's owner, read with the page: the owner its workspace erase names (F-f10-09).
    pub(crate) user_id: String,
}

/// The counts of one pass over one org.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SweepCounts {
    pub(crate) candidates: i64,
    pub(crate) held: i64,
    pub(crate) skipped_zdr: i64,
    pub(crate) would_delete: i64,
    /// Rows of each [`THREAD_BOUND_TABLES`] table bound to the would-delete threads.
    pub(crate) rows_by_table: BTreeMap<&'static str, i64>,
    /// Threads deleted (delete mode only).
    pub(crate) deleted: i64,
    /// Would-delete threads that were active again, or gone, when their delete
    /// transaction re-checked them.
    pub(crate) skipped_active: i64,
    /// The daily cap left threads undeleted in this pass.
    pub(crate) cap_reached: bool,
    /// Why a pass that had deleted something stopped early.
    pub(crate) stopped: Option<&'static str>,
    /// Semantic memory twins whose erasure letta-bridge confirmed, and did not.
    pub(crate) semantic_confirmed: u64,
    pub(crate) semantic_unconfirmed: u64,
    /// Thread workspaces whose erasure sandbox-manager confirmed, and did not.
    pub(crate) workspace_confirmed: u64,
    pub(crate) workspace_unconfirmed: u64,
    /// The first reason each tier gave for an unconfirmed erasure, for the audit.
    pub(crate) semantic_degradation: Option<&'static str>,
    pub(crate) workspace_degradation: Option<&'static str>,
}

impl SweepCounts {
    fn add_semantic(&mut self, outcome: SemanticErasure) {
        self.semantic_confirmed += outcome.confirmed;
        self.semantic_unconfirmed += outcome.unconfirmed;
        self.semantic_degradation = self.semantic_degradation.or(outcome.degradation_reason);
    }

    fn add_workspace(&mut self, outcome: WorkspaceErasure) {
        match outcome {
            WorkspaceErasure::Erased => self.workspace_confirmed += 1,
            WorkspaceErasure::Unconfirmed(reason) => {
                self.workspace_unconfirmed += 1;
                self.workspace_degradation = self.workspace_degradation.or(Some(reason));
            }
        }
    }
}

/// What one thread workspace erasure came to. Same standard as the semantic tier:
/// only a clean answer from the store counts as erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceErasure {
    Erased,
    Unconfirmed(&'static str),
}

/// Erases one deleted thread's sandbox workspace. Called after the thread's delete
/// committed; it never fails the delete. `owner_id` is the thread's owner, read from the
/// thread row before the delete (F-f10-09).
pub(crate) trait WorkspaceEraser: Send + Sync {
    fn erase<'a>(
        &'a self,
        org_id: &'a str,
        thread_id: &'a str,
        owner_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = WorkspaceErasure> + Send + 'a>>;
}

/// The eraser when the production one is not configured (see the module docs): it reaches
/// nothing and says so, so a pass never claims a workspace it did not erase.
pub(crate) struct UnavailableWorkspaceEraser;

impl WorkspaceEraser for UnavailableWorkspaceEraser {
    fn erase<'a>(
        &'a self,
        _org_id: &'a str,
        _thread_id: &'a str,
        _owner_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = WorkspaceErasure> + Send + 'a>> {
        Box::pin(std::future::ready(WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)))
    }
}

/// What a delete pass needs beyond the dry run. A pass given none runs dry.
pub(crate) struct Erasure<'a> {
    pub(crate) letta: Option<&'a LettaMemoryAdapter>,
    pub(crate) workspaces: &'a dyn WorkspaceEraser,
    pub(crate) daily_cap: i64,
}

/// One `retention_sweep_runs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SweepRun {
    pub(crate) id: String,
    pub(crate) org_id: String,
    pub(crate) mode: &'static str,
    pub(crate) policy_revision: i64,
    pub(crate) window_days: Option<i64>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: DateTime<Utc>,
    pub(crate) counts: SweepCounts,
    pub(crate) error_code: Option<&'static str>,
}

/// Splits one page by the check answer: what is zero-retention, what is held, what
/// would be deleted. Pure. A held id the page did not name is ignored, so org-core
/// cannot add a thread to the counts.
pub(crate) fn split_page<'a>(
    page: &'a [Candidate],
    answer: &PolicyAnswer,
) -> (i64, i64, Vec<&'a Candidate>) {
    let mut skipped_zdr = 0;
    let mut held = 0;
    let mut would_delete = Vec::new();
    for candidate in page {
        if candidate.zdr_only {
            skipped_zdr += 1;
        } else if answer.org_held || answer.held_thread_ids.contains(&candidate.thread_id) {
            held += 1;
        } else {
            would_delete.push(candidate);
        }
    }
    (skipped_zdr, held, would_delete)
}

/// Splits one delete batch by the hold re-check: how many are held now, and the threads
/// still free to delete. Pure, and like [`split_page`] it only ever narrows.
pub(crate) fn split_batch<'a>(
    batch: &[&'a Candidate],
    answer: &PolicyAnswer,
) -> (i64, Vec<&'a Candidate>) {
    if answer.org_held {
        return (i64::try_from(batch.len()).unwrap_or(i64::MAX), Vec::new());
    }
    let free: Vec<&Candidate> = batch
        .iter()
        .copied()
        .filter(|candidate| !answer.held_thread_ids.contains(&candidate.thread_id))
        .collect();
    (i64::try_from(batch.len() - free.len()).unwrap_or(i64::MAX), free)
}

/// The same policy as `policy`: counts and deletes under two revisions describe no
/// single policy.
fn same_policy(answer: &PolicyAnswer, policy: &PolicyAnswer) -> bool {
    answer.revision == policy.revision && answer.window_days == policy.window_days
}

pub(crate) fn window_in_bounds(days: i64) -> bool {
    (MIN_WINDOW_DAYS..=MAX_WINDOW_DAYS).contains(&days)
}

/// One page of candidates after `cursor`, in id order, each with its owner. Last activity is the later of
/// the thread's own activity time and the latest change of any of its runs; a thread
/// with a run that has not ended is never a candidate.
async fn candidate_page(
    pool: &PgPool,
    org_id: &str,
    cutoff: DateTime<Utc>,
    cursor: &str,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let rows: Vec<(String, bool, String)> = sqlx::query_as(
        "SELECT t.id,
                (EXISTS (SELECT 1 FROM runs r WHERE r.thread_id = t.id)
                 AND NOT EXISTS (SELECT 1 FROM runs r
                                  WHERE r.thread_id = t.id AND r.retention_class <> 'zdr')) AS zdr_only,
                t.user_id
           FROM threads t
          WHERE t.org_id = $1
            AND t.space_id IS NULL
            AND t.space_kind IS NULL
            AND t.id > $3
            AND GREATEST(t.last_activity_at,
                         COALESCE((SELECT max(GREATEST(r.updated_at, COALESCE(r.ended_at, r.updated_at)))
                                     FROM runs r WHERE r.thread_id = t.id),
                                  t.last_activity_at)) < $2
            AND NOT EXISTS (SELECT 1 FROM runs r
                             WHERE r.thread_id = t.id
                               AND r.status NOT IN ('completed', 'failed', 'cancelled'))
          ORDER BY t.id
          LIMIT $4",
    )
    .bind(org_id)
    .bind(cutoff)
    .bind(cursor)
    .bind(CHECK_PAGE)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(thread_id, zdr_only, user_id)| Candidate {
            thread_id,
            zdr_only,
            user_id,
        })
        .collect())
}

/// Adds the rows each thread-bound table holds for `thread_ids` to `rows_by_table`.
async fn count_bound_rows(
    pool: &PgPool,
    thread_ids: &[&str],
    rows_by_table: &mut BTreeMap<&'static str, i64>,
) -> Result<(), sqlx::Error> {
    if thread_ids.is_empty() {
        return Ok(());
    }
    for table in THREAD_BOUND_TABLES {
        // `table` is one of the constants above, never input.
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE thread_id = ANY($1)"))
            .bind(thread_ids)
            .fetch_one(pool)
            .await?;
        *rows_by_table.entry(table).or_insert(0) += rows;
    }
    Ok(())
}

/// Threads this org lost to enforce passes in the last 24 hours: its ledger rows, which
/// every delete writes in its own transaction whether or not the pass was recorded,
/// plus the `deleted` of enforce runs recorded before the ledger existed (a run with
/// no ledger row). A run with ledger rows is counted by them alone, so nothing is
/// counted twice, and the sum is never below what the run rows alone said.
async fn deleted_in_last_day(pool: &PgPool, org_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*)
                   FROM retention_sweep_deletions
                  WHERE org_id = $1 AND deleted_at > now() - interval '24 hours')
              + (SELECT COALESCE(sum(r.deleted), 0)
                   FROM retention_sweep_runs r
                  WHERE r.org_id = $1 AND r.mode = 'enforce'
                    AND r.started_at > now() - interval '24 hours'
                    AND NOT EXISTS (SELECT 1 FROM retention_sweep_deletions d WHERE d.run_id = r.id))::bigint",
    )
    .bind(org_id)
    .fetch_one(pool)
    .await
}

/// The thread-bound rows `grpc::delete_thread_rows` leaves, because the user
/// delete relies on the database for them or never needed them. Run first: a
/// scheduled step receipt references its run without a cascade, so it must go
/// before the runs do. Message mentions and revisions, approval scope grants and
/// run questions cascade from their message, thread or run; `space_event_log` rows
/// of the thread are redacted by the threads delete trigger. A skill revision
/// belongs to its skill, which outlives the thread, but it was caused by a run of
/// this thread and its snapshots hold skill text, so it goes with the thread.
async fn delete_residual_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
) -> Result<(), sqlx::Error> {
    for query in [
        "DELETE FROM scheduled_step_receipts
          WHERE thread_id = $1 OR run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM cron_fires
          WHERE thread_id = $1 OR run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM skill_revisions WHERE thread_id = $1",
    ] {
        sqlx::query(query).bind(thread_id).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Deletes one would-delete thread and propagates the delete. `Ok(None)` when the
/// thread is no longer an idle personal thread of this org (activity since the page
/// was read, a run opened, or it is gone): nothing is deleted. The re-check holds the
/// thread row lock, which an append or a new run waits on, so neither can land
/// between the re-check and the delete. The delete writes its ledger row under
/// `run_id` in the same transaction, so it counts against the daily cap and leaves a
/// trace for [`reconcile_interrupted`] even if the pass never records its row. The
/// re-check also holds `owner_id`: the owner the workspace erase names is the owner of the
/// row this transaction deleted (F-f10-09).
async fn delete_thread(
    pool: &PgPool,
    erasure: &Erasure<'_>,
    run_id: &str,
    org_id: &str,
    thread_id: &str,
    owner_id: &str,
    cutoff: DateTime<Utc>,
) -> Result<Option<(SemanticErasure, WorkspaceErasure)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let still_idle: Option<String> = sqlx::query_scalar(
        "SELECT t.id
           FROM threads t
          WHERE t.id = $1
            AND t.org_id = $2
            AND t.space_id IS NULL
            AND t.space_kind IS NULL
            AND GREATEST(t.last_activity_at,
                         COALESCE((SELECT max(GREATEST(r.updated_at, COALESCE(r.ended_at, r.updated_at)))
                                     FROM runs r WHERE r.thread_id = t.id),
                                  t.last_activity_at)) < $3
            AND NOT EXISTS (SELECT 1 FROM runs r
                             WHERE r.thread_id = t.id
                               AND r.status NOT IN ('completed', 'failed', 'cancelled'))
            AND t.user_id = $4
            FOR UPDATE OF t",
    )
    .bind(thread_id)
    .bind(org_id)
    .bind(cutoff)
    .bind(owner_id)
    .fetch_optional(&mut *tx)
    .await?;
    if still_idle.is_none() {
        return Ok(None);
    }
    delete_residual_rows(&mut tx, thread_id).await?;
    let erased = crate::grpc::delete_thread_rows(&mut tx, thread_id)
        .await
        .map_err(|status| sqlx::Error::Protocol(format!("delete_thread_rows: {}", status.message())))?;
    let deleted = sqlx::query("DELETE FROM threads WHERE id = $1 AND org_id = $2")
        .bind(thread_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted != 1 {
        return Ok(None);
    }
    // Ids and the time only: the ledger never names the thread.
    sqlx::query("INSERT INTO retention_sweep_deletions (run_id, org_id) VALUES ($1, $2)")
        .bind(run_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // The sweeper holds no owner credential, so the Control grants of the erased snapshots stay; each
    // is named by id and code, after the commit.
    crate::thread_shares::log_snapshot_grants_not_removed(
        &erased.snapshot_ids,
        crate::thread_shares::GRANTS_NO_CREDENTIAL_CODE,
    );

    // After the commit, never inside it: a degraded tier must not hold or undo a
    // delete Postgres already made, and the erased memory ids exist nowhere else now.
    let semantic =
        crate::memory_erasure::erase_semantic_copies(erasure.letta, org_id, &erased.memories).await;
    let workspace = erasure.workspaces.erase(org_id, thread_id, owner_id).await;
    Ok(Some((semantic, workspace)))
}

/// How the delete half of one page ended.
enum BatchEnd {
    Done,
    /// Stop the pass for this reason (an `error_code` word, or
    /// [`STOPPED_DELETE_FAILED`]).
    Stop(&'static str, Option<sqlx::Error>),
}

/// Deletes one page's would-delete threads in batches, re-checking holds before
/// each batch. Adds every settled thread to `run.counts`; threads of a batch the
/// pass stopped before are not counted at all, so the counts still add up.
#[allow(clippy::too_many_arguments)]
async fn delete_page(
    pool: &PgPool,
    client: &CheckClient,
    erasure: &Erasure<'_>,
    policy: &PolicyAnswer,
    run: &mut SweepRun,
    cap_left: &mut i64,
    cutoff: DateTime<Utc>,
    would_delete: &[&Candidate],
) -> BatchEnd {
    let org_id = run.org_id.clone();
    let run_id = run.id.clone();
    for batch in would_delete.chunks(DELETE_BATCH) {
        let batch_len = i64::try_from(batch.len()).unwrap_or(i64::MAX);
        let (held, free) = if *cap_left <= 0 {
            // Nothing will be deleted, so there is nothing to re-check.
            run.counts.cap_reached = true;
            (0, batch.to_vec())
        } else {
            let asked: Vec<String> = batch
                .iter()
                .map(|candidate| candidate.thread_id.clone())
                .collect();
            let answer = match client.check(&org_id, &asked).await {
                Ok(Some(answer)) if same_policy(&answer, policy) => answer,
                Ok(Some(_)) => return BatchEnd::Stop(ERROR_POLICY_CHANGED, None),
                Ok(None) | Err(_) => return BatchEnd::Stop(ERROR_POLICY_UNAVAILABLE, None),
            };
            split_batch(batch, &answer)
        };
        let free_ids: Vec<&str> = free
            .iter()
            .map(|candidate| candidate.thread_id.as_str())
            .collect();
        if let Err(error) = count_bound_rows(pool, &free_ids, &mut run.counts.rows_by_table).await {
            return BatchEnd::Stop(STOPPED_DELETE_FAILED, Some(error));
        }
        // The batch is settled from here on: held, would delete, and of those some
        // deleted, so `held + skipped_zdr + would_delete = candidates` holds whatever
        // happens to the deletes below.
        run.counts.candidates += batch_len;
        run.counts.held += held;
        run.counts.would_delete += i64::try_from(free.len()).unwrap_or(i64::MAX);
        for candidate in free {
            if *cap_left <= 0 {
                run.counts.cap_reached = true;
                break;
            }
            let delete = delete_thread(
                pool,
                erasure,
                &run_id,
                &org_id,
                &candidate.thread_id,
                &candidate.user_id,
                cutoff,
            )
            .await;
            match delete {
                Ok(Some((semantic, workspace))) => {
                    run.counts.deleted += 1;
                    *cap_left -= 1;
                    run.counts.add_semantic(semantic);
                    run.counts.add_workspace(workspace);
                }
                Ok(None) => run.counts.skipped_active += 1,
                Err(error) => return BatchEnd::Stop(STOPPED_DELETE_FAILED, Some(error)),
            }
        }
    }
    BatchEnd::Done
}

/// One pass over one org: dry without an `erasure`, deleting with one. `Ok(None)`
/// when the org has no policy to apply (org-core does not know it, or no window is
/// set): nothing is recorded. A database error before anything was deleted is
/// returned; every org-core failure before then is recorded as the row's
/// `error_code` with zero counts, so an unknown policy never yields a candidate. Once
/// a thread is deleted the pass always returns its row, so the delete is recorded.
pub(crate) async fn sweep_org(
    pool: &PgPool,
    client: &CheckClient,
    org_id: &str,
    erasure: Option<&Erasure<'_>>,
) -> Result<Option<SweepRun>, sqlx::Error> {
    let started_at = Utc::now();
    let mut run = SweepRun {
        id: format!("rsw_{}", mp_ids::new_ulid()),
        org_id: org_id.to_owned(),
        mode: if erasure.is_some() { RECORDED_ENFORCE } else { RECORDED_DRY_RUN },
        policy_revision: 0,
        window_days: None,
        started_at,
        finished_at: started_at,
        counts: SweepCounts::default(),
        error_code: None,
    };

    // The empty question reads the window, the revision and the org hold.
    let policy = match client.check(org_id, &[]).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return Ok(None),
        Err(error) => {
            warn!(%org_id, %error, "retention check failed; no candidates for this org");
            return Ok(Some(fail(run, ERROR_POLICY_UNAVAILABLE)));
        }
    };
    let Some(window_days) = policy.window_days else {
        return Ok(None);
    };
    run.policy_revision = policy.revision;
    if !window_in_bounds(window_days) {
        warn!(%org_id, window_days, "retention window outside 30..3650 days; refused");
        return Ok(Some(fail(run, ERROR_POLICY_WINDOW_INVALID)));
    }
    run.window_days = Some(window_days);

    // One cutoff for the whole pass, from the database clock the activity times use.
    let window = i32::try_from(window_days).unwrap_or(i32::MAX);
    let cutoff: DateTime<Utc> = sqlx::query_scalar("SELECT now() - make_interval(days => $1)")
        .bind(window)
        .fetch_one(pool)
        .await?;
    let mut cap_left = match erasure {
        Some(erasure) => (erasure.daily_cap - deleted_in_last_day(pool, org_id).await?).max(0),
        None => 0,
    };

    let mut cursor = String::new();
    loop {
        let page = match candidate_page(pool, org_id, cutoff, &cursor).await {
            Ok(page) => page,
            Err(error) if run.counts.deleted == 0 => return Err(error),
            Err(error) => {
                warn!(%org_id, %error, "retention sweep stopped after deleting; recording what it did");
                return Ok(Some(stop(run, STOPPED_DELETE_FAILED)));
            }
        };
        let Some(last) = page.last() else {
            break;
        };
        cursor.clone_from(&last.thread_id);
        let asked: Vec<String> = page
            .iter()
            .filter(|candidate| !candidate.zdr_only)
            .map(|candidate| candidate.thread_id.clone())
            .collect();
        let answer = if asked.is_empty() {
            policy.clone()
        } else {
            match client.check(org_id, &asked).await {
                Ok(Some(answer)) => answer,
                // A 404 mid-pass means the org went away; nothing it held is known.
                Ok(None) | Err(_) => return Ok(fail_or_stop(run, ERROR_POLICY_UNAVAILABLE)),
            }
        };
        if !same_policy(&answer, &policy) {
            return Ok(fail_or_stop(run, ERROR_POLICY_CHANGED));
        }
        let (skipped_zdr, held, would_delete) = split_page(&page, &answer);
        let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
        let Some(erasure) = erasure else {
            run.counts.candidates += page_len;
            run.counts.skipped_zdr += skipped_zdr;
            run.counts.held += held;
            run.counts.would_delete += i64::try_from(would_delete.len()).unwrap_or(i64::MAX);
            let ids: Vec<&str> = would_delete
                .iter()
                .map(|candidate| candidate.thread_id.as_str())
                .collect();
            count_bound_rows(pool, &ids, &mut run.counts.rows_by_table).await?;
            if page_len < CHECK_PAGE {
                break;
            }
            continue;
        };
        // What the page check settled is counted now; the would-delete threads are
        // counted batch by batch as `delete_page` settles them.
        run.counts.candidates += skipped_zdr + held;
        run.counts.skipped_zdr += skipped_zdr;
        run.counts.held += held;
        let end = delete_page(pool, client, erasure, &policy, &mut run, &mut cap_left, cutoff, &would_delete).await;
        if let BatchEnd::Stop(reason, error) = end {
            if let Some(error) = error {
                if run.counts.deleted == 0 {
                    return Err(error);
                }
                warn!(%org_id, %error, "retention sweep stopped after deleting; recording what it did");
            }
            return Ok(fail_or_stop(run, reason));
        }
        if page_len < CHECK_PAGE {
            break;
        }
    }
    run.finished_at = Utc::now();
    Ok(Some(run))
}

/// Ends a pass that cannot go on. Before any delete the row reports nothing it could
/// not stand behind (zero counts and the closed `error_code`). After a delete the
/// counts are what happened and must be recorded, and the run table admits a policy
/// error only with zero counts, so the reason goes to the audit event as `stopped`.
fn fail_or_stop(run: SweepRun, reason: &'static str) -> Option<SweepRun> {
    if run.counts.deleted == 0 {
        Some(fail(run, reason))
    } else {
        Some(stop(run, reason))
    }
}

fn fail(mut run: SweepRun, code: &'static str) -> SweepRun {
    run.counts = SweepCounts::default();
    run.error_code = Some(code);
    run.finished_at = Utc::now();
    run
}

fn stop(mut run: SweepRun, reason: &'static str) -> SweepRun {
    run.counts.stopped = Some(reason);
    run.finished_at = Utc::now();
    run
}

/// The audit body of one pass: audit-core's flat shape, counts only.
pub(crate) fn build_sweep_audit_body(run: &SweepRun) -> serde_json::Value {
    serde_json::json!({
        "event_id": format!("retention_sweep:session-core:{}", run.id),
        "occurred_at": run.finished_at.to_rfc3339(),
        "org_id": run.org_id,
        "user_id": "system",
        "plane": "model",
        "producer": "session-core",
        "event": EVENT_RETENTION_SWEEP,
        "subject": run.id,
        "resource_id": run.org_id,
        "outcome": if run.error_code.is_some() { "failed" } else { "completed" },
        "details": {
            "mode": run.mode,
            "policy_revision": run.policy_revision,
            "window_days": run.window_days,
            "candidates": run.counts.candidates,
            "held": run.counts.held,
            "skipped_zdr": run.counts.skipped_zdr,
            "would_delete": run.counts.would_delete,
            "deleted": run.counts.deleted,
            "skipped_active": run.counts.skipped_active,
            "cap_reached": run.counts.cap_reached,
            "stopped": run.counts.stopped,
            "semantic_confirmed": run.counts.semantic_confirmed,
            "semantic_unconfirmed": run.counts.semantic_unconfirmed,
            "workspace_confirmed": run.counts.workspace_confirmed,
            "workspace_unconfirmed": run.counts.workspace_unconfirmed,
            "semantic_degradation": run.counts.semantic_degradation,
            "workspace_degradation": run.counts.workspace_degradation,
            "rows_by_table": run.counts.rows_by_table,
            "error_code": run.error_code,
        },
    })
}

/// Writes the run row and its audit event in one transaction, so the audit trail
/// never names a pass the table does not hold, or the reverse.
pub(crate) async fn record(pool: &PgPool, run: &SweepRun) -> Result<(), sqlx::Error> {
    let body = build_sweep_audit_body(run);
    let subject = crate::audit_publisher::audit_subject_for_event(EVENT_RETENTION_SWEEP)
        .ok_or_else(|| sqlx::Error::Protocol("retention sweep event is not a subject token".to_owned()))?;
    let rows_by_table =
        serde_json::to_value(&run.counts.rows_by_table).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO retention_sweep_runs
             (id, org_id, mode, policy_revision, window_days, started_at, finished_at,
              candidates, held, skipped_zdr, would_delete, deleted, rows_by_table, error_code)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&run.id)
    .bind(&run.org_id)
    .bind(run.mode)
    .bind(run.policy_revision)
    .bind(run.window_days.and_then(|days| i32::try_from(days).ok()))
    .bind(run.started_at)
    .bind(run.finished_at)
    .bind(run.counts.candidates)
    .bind(run.counts.held)
    .bind(run.counts.skipped_zdr)
    .bind(run.counts.would_delete)
    .bind(run.counts.deleted)
    .bind(&rows_by_table)
    .bind(run.error_code)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO session_audit_outbox (event_id, subject, payload) VALUES ($1, $2, $3)")
        .bind(body["event_id"].as_str().unwrap_or_default())
        .bind(&subject)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// The `interrupted` row of a run whose deletes the ledger holds but whose pass never
/// recorded it: enforce, no policy, zero counts except `deleted`, and the first and
/// last delete as its start and end. Pure.
pub(crate) fn interrupted_run(
    run_id: String,
    org_id: String,
    deleted: i64,
    first_delete: DateTime<Utc>,
    last_delete: DateTime<Utc>,
) -> SweepRun {
    SweepRun {
        id: run_id,
        org_id,
        mode: RECORDED_ENFORCE,
        policy_revision: 0,
        window_days: None,
        started_at: first_delete,
        finished_at: last_delete.max(first_delete),
        counts: SweepCounts {
            deleted,
            ..SweepCounts::default()
        },
        error_code: Some(ERROR_INTERRUPTED),
    }
}

/// Records every run whose ledger rows have no run row and whose last delete is older
/// than [`INTERRUPTED_AFTER_MINUTES`] as `interrupted` (see the module docs), each with
/// its audit event through [`record`]. Runs under the sweep lock. Returns how many
/// rows it wrote; a run whose record fails is logged and left for the next pass, and
/// its ledger rows keep counting against the cap meanwhile.
pub(crate) async fn reconcile_interrupted(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let orphans: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT d.run_id, d.org_id, count(*)::bigint, min(d.deleted_at), max(d.deleted_at)
           FROM retention_sweep_deletions d
          WHERE NOT EXISTS (SELECT 1 FROM retention_sweep_runs r WHERE r.id = d.run_id)
          GROUP BY d.run_id, d.org_id
         HAVING max(d.deleted_at) < now() - make_interval(mins => $1)
          ORDER BY min(d.deleted_at)",
    )
    .bind(INTERRUPTED_AFTER_MINUTES)
    .fetch_all(pool)
    .await?;
    let mut written = 0usize;
    for (run_id, org_id, deleted, first_delete, last_delete) in orphans {
        let run = interrupted_run(run_id, org_id, deleted, first_delete, last_delete);
        match record(pool, &run).await {
            Ok(()) => {
                written += 1;
                warn!(org_id = %run.org_id, run_id = %run.id, deleted,
                    "retention sweep recorded an interrupted pass from its deletion ledger");
                metrics::counter!("mp_session_retention_sweep_runs_total", "outcome" => "interrupted").increment(1);
            }
            Err(error) => {
                error!(org_id = %run.org_id, run_id = %run.id, deleted, %error,
                    "retention sweep could not record an interrupted pass; retried next pass");
            }
        }
    }
    Ok(written)
}

/// Deletes the ledger rows older than [`LEDGER_KEEP_HOURS`] whose run row exists. A
/// row whose run is not recorded is kept, whatever its age, until
/// [`reconcile_interrupted`] has recorded it. Returns how many rows went.
pub(crate) async fn prune_ledger(pool: &PgPool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM retention_sweep_deletions d
          WHERE d.deleted_at < now() - make_interval(hours => $1)
            AND EXISTS (SELECT 1 FROM retention_sweep_runs r WHERE r.id = d.run_id)",
    )
    .bind(LEDGER_KEEP_HOURS)
    .execute(pool)
    .await?
    .rows_affected())
}

/// The ledger upkeep that opens every pass: record the interrupted runs, then prune.
/// Neither failure stops the pass: the cap reads the ledger itself, so a pass that
/// could not reconcile still deletes no more than it may.
async fn upkeep_ledger(pool: &PgPool) -> usize {
    let written = match reconcile_interrupted(pool).await {
        Ok(written) => written,
        Err(error) => {
            warn!(%error, "retention sweep could not read its deletion ledger; interrupted passes wait for the next pass");
            0
        }
    };
    if let Err(error) = prune_ledger(pool).await {
        warn!(%error, "retention sweep could not prune its deletion ledger");
    }
    written
}

/// One pass over every org that owns personal threads here, deleting when given an
/// `erasure`. Returns how many rows it wrote, or `None` when another replica holds
/// the sweep. The advisory lock also keeps two replicas from reading the same daily
/// cap and deleting twice its worth. Under the lock, the pass first records the
/// interrupted runs its ledger holds and prunes it ([`upkeep_ledger`]); those rows
/// count among the rows written.
pub(crate) async fn sweep_once(
    pool: &PgPool,
    client: &CheckClient,
    erasure: Option<&Erasure<'_>>,
) -> Result<Option<usize>> {
    let mut lock = pool.acquire().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(SWEEP_LOCK_KEY)
        .fetch_one(&mut *lock)
        .await?;
    if !locked {
        return Ok(None);
    }
    let reconciled = upkeep_ledger(pool).await;
    let result = sweep_orgs(pool, client, erasure)
        .await
        .map(|written| written + reconciled);
    let unlocked = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SWEEP_LOCK_KEY)
        .execute(&mut *lock)
        .await;
    if unlocked.is_err() {
        // A session lock lives as long as its connection: close it rather than hand
        // a connection still holding the lock back to the pool.
        drop(lock.detach());
    }
    result.map(Some)
}

async fn sweep_orgs(pool: &PgPool, client: &CheckClient, erasure: Option<&Erasure<'_>>) -> Result<usize> {
    let orgs: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT org_id FROM threads WHERE space_id IS NULL ORDER BY org_id")
            .fetch_all(pool)
            .await?;
    let mut written = 0usize;
    for org_id in &orgs {
        let Some(run) = sweep_org(pool, client, org_id, erasure).await? else {
            continue;
        };
        if let Err(error) = record(pool, &run).await {
            // The deletes committed thread by thread, each with its ledger row; only
            // their record is missing. The ledger keeps them inside the daily cap, and
            // a later pass records the run as interrupted.
            if run.counts.deleted > 0 {
                error!(%org_id, run_id = %run.id, deleted = run.counts.deleted, %error,
                    "retention sweep deleted threads but could not record the pass; a later pass records it as interrupted");
            }
            return Err(error.into());
        }
        written += 1;
        let outcome = if run.error_code.is_some() { "error" } else { "ok" };
        metrics::counter!("mp_session_retention_sweep_runs_total", "outcome" => outcome).increment(1);
        metrics::counter!("mp_session_retention_sweep_would_delete_total")
            .increment(u64::try_from(run.counts.would_delete).unwrap_or(0));
        metrics::counter!("mp_session_retention_sweep_deleted_total")
            .increment(u64::try_from(run.counts.deleted).unwrap_or(0));
        let unconfirmed = run.counts.semantic_unconfirmed + run.counts.workspace_unconfirmed;
        if unconfirmed > 0 {
            metrics::counter!("mp_session_retention_sweep_propagation_unconfirmed_total").increment(unconfirmed);
        }
    }
    Ok(written)
}

/// The sweep loop: one pass now, then one per interval. It runs under
/// `supervise_background`; a failed pass is logged and the next one runs on time.
pub(crate) async fn run(pool: PgPool, mode: Mode) -> Result<()> {
    let lookup = |name: &str| std::env::var(name).ok();
    let Some(client) = CheckClient::from_lookup(&lookup) else {
        error!(
            "{SWITCH_ENV} is on but {} is unset: no org is swept",
            crate::org_policy::SERVICE_TOKEN_ENV
        );
        std::future::pending::<()>().await;
        return Ok(());
    };
    // A letta configuration that does not parse is refused rather than read as
    // "none": deleting the durable memory rows makes their semantic twins
    // unreachable, so the delete runs only where the tier is either reachable or
    // deliberately absent (and then counted unconfirmed, as the user delete does).
    let letta = match mode {
        Mode::Enforce => match LettaMemoryAdapter::from_env() {
            Ok(letta) => Some(letta),
            Err(error) => {
                error!(%error, "{SWITCH_ENV}=enforce but the letta configuration is invalid; passes run dry");
                None
            }
        },
        Mode::Off | Mode::DryRun => None,
    };
    // F-f10-09: a delete pass erases each workspace through sandbox-manager's service erase only
    // when SESSION_RETENTION_WORKSPACE_ERASE is the literal true and the eraser is configured;
    // otherwise the unavailable eraser, which claims nothing. A dry pass never erases.
    let sandbox_workspaces = match mode {
        Mode::Enforce => crate::sandbox_workspace_eraser::GrpcWorkspaceEraser::from_lookup(&lookup),
        Mode::Off | Mode::DryRun => None,
    };
    let unavailable_workspaces = UnavailableWorkspaceEraser;
    let workspaces: &dyn WorkspaceEraser = match sandbox_workspaces.as_ref() {
        Some(eraser) => eraser,
        None => &unavailable_workspaces,
    };
    let erasure = letta.as_ref().map(|letta| Erasure {
        letta: letta.as_ref(),
        workspaces,
        daily_cap: daily_cap_from(std::env::var(DAILY_CAP_ENV).ok().as_deref()),
    });
    let interval = interval_from(std::env::var(INTERVAL_SECS_ENV).ok().as_deref());
    info!(
        interval_secs = interval.as_secs(),
        delete = erasure.is_some(),
        daily_cap = erasure.as_ref().map(|erasure| erasure.daily_cap),
        workspace_erase = sandbox_workspaces.is_some(),
        "retention sweeper started"
    );
    loop {
        match sweep_once(&pool, &client, erasure.as_ref()).await {
            Ok(Some(written)) => info!(written, "retention sweep pass recorded"),
            Ok(None) => info!("retention sweep held by another replica; skipped"),
            Err(error) => warn!(%error, "retention sweep pass failed"),
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn client_for(base_url: &str) -> CheckClient {
        CheckClient::from_lookup(&env(&[
            (crate::org_policy::ORG_CORE_URL_ENV, base_url),
            (crate::org_policy::SERVICE_TOKEN_ENV, "test-token"),
        ]))
        .expect("client")
    }

    fn answer(org_held: bool, held: &[&str]) -> PolicyAnswer {
        PolicyAnswer {
            window_days: Some(30),
            revision: 3,
            org_held,
            held_thread_ids: held.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn candidate(id: &str, zdr_only: bool) -> Candidate {
        Candidate {
            thread_id: id.to_owned(),
            zdr_only,
            user_id: "u1".to_owned(),
        }
    }

    #[test]
    fn retention_sweep_switch_unset_blank_or_off_is_off() {
        // Padded or cased off is off: the operator asked for nothing, so nothing runs.
        for raw in [
            None,
            Some(""),
            Some("off"),
            Some(" off "),
            Some("  "),
            Some("\toff\n"),
            Some("Off"),
            Some("OFF"),
            Some(" oFf\r\n"),
        ] {
            assert_eq!(mode_from(raw), Mode::Off, "{raw:?}");
        }
    }

    #[test]
    fn retention_sweep_switch_enforce_is_exact_and_unknown_is_dry_run_never_enforce() {
        assert_eq!(mode_from(Some("dry_run")), Mode::DryRun);
        assert_eq!(mode_from(Some("enforce")), Mode::Enforce);
        // Enforce is never reached through padding: a padded word is another value, so dry_run.
        for raw in [" enforce ", "enforce\n", "\tenforce"] {
            assert_eq!(mode_from(Some(raw)), Mode::DryRun, "{raw:?}");
        }
        // A typo, another casing or a boolean must never become enforce.
        for raw in [
            "Enforce", "ENFORCE", "enforced", "true", "1", "on", "dry-run", "false", "offer",
        ] {
            assert_eq!(mode_from(Some(raw)), Mode::DryRun, "{raw}");
        }
    }

    /// The startup report names exactly the values the switch reads as dry run by accident: a padded
    /// or cased `enforce` is the case that matters, since it is the one that looks like a delete
    /// switch and does not turn one on. The words and the off values are never named.
    #[test]
    fn retention_sweep_startup_report_names_only_the_values_that_read_as_dry_run_by_accident() {
        for word in [
            "enforce",
            "dry_run",
            "off",
            "OFF",
            " off ",
            "",
            "   ",
            "\toff\r\n",
        ] {
            assert!(!is_unreadable_mode(word), "{word:?} is one of its words");
        }
        for junk in [
            " enforce ",
            "enforce\r\n",
            "Enforce",
            "ENFORCE",
            "enforced",
            "true",
            "maybe",
            "dry-run",
        ] {
            assert!(is_unreadable_mode(junk), "{junk:?} reads as dry run");
            assert_eq!(mode_from(Some(junk)), Mode::DryRun, "{junk:?}");
        }
    }

    #[test]
    fn retention_sweep_interval_defaults_to_six_hours_with_a_floor() {
        assert_eq!(interval_from(None), Duration::from_secs(21_600));
        assert_eq!(interval_from(Some("nonsense")), Duration::from_secs(21_600));
        assert_eq!(interval_from(Some("0")), Duration::from_secs(300));
        assert_eq!(interval_from(Some("60")), Duration::from_secs(300));
        assert_eq!(interval_from(Some("3600")), Duration::from_secs(3600));
    }

    #[test]
    fn retention_sweep_window_bounds_are_thirty_to_3650_days() {
        assert!(!window_in_bounds(0));
        assert!(!window_in_bounds(29));
        assert!(window_in_bounds(30));
        assert!(window_in_bounds(3650));
        assert!(!window_in_bounds(3651));
        assert!(!window_in_bounds(-30));
    }

    #[test]
    fn retention_sweep_check_answer_is_read_strictly() {
        let body = br#"{"organization_id":"org-a","window_days":90,"revision":4,"org_held":false,"held_thread_ids":["t1"]}"#;
        let parsed = parse_check("org-a", body).expect("answer");
        assert_eq!(parsed.window_days, Some(90));
        assert_eq!(parsed.revision, 4);
        assert!(parsed.held_thread_ids.contains("t1"));
        let keep = br#"{"organization_id":"org-a","window_days":null,"revision":0,"org_held":false,"held_thread_ids":[]}"#;
        assert_eq!(parse_check("org-a", keep).expect("keep").window_days, None);
        // An answer about another org, a negative revision or a wrong shape is unknown.
        assert!(parse_check("org-b", body).is_err());
        let negative = br#"{"organization_id":"org-a","window_days":90,"revision":-1,"org_held":false}"#;
        assert!(parse_check("org-a", negative).is_err());
        assert!(parse_check("org-a", br#"{"window_days":90}"#).is_err());
        assert!(parse_check("org-a", b"not json").is_err());
    }

    #[test]
    fn retention_sweep_page_split_keeps_held_and_zdr_out_of_would_delete() {
        let page = [candidate("t1", false), candidate("t2", false), candidate("t3", true)];
        let (zdr, held, would) = split_page(&page, &answer(false, &["t2", "not-asked"]));
        assert_eq!((zdr, held), (1, 1));
        let ids: Vec<&str> = would.iter().map(|candidate| candidate.thread_id.as_str()).collect();
        assert_eq!(ids, vec!["t1"]);
        // An org hold keeps every thread that holds content.
        let (zdr, held, would) = split_page(&page, &answer(true, &[]));
        assert_eq!((zdr, held), (1, 2));
        assert!(would.is_empty());
    }

    #[test]
    fn retention_sweep_batch_recheck_only_narrows() {
        let page = [
            candidate("t1", false),
            candidate("t2", false),
            candidate("t3", false),
        ];
        let batch: Vec<&Candidate> = page.iter().collect();
        let (held, free) = split_batch(&batch, &answer(false, &["t2", "not-asked"]));
        assert_eq!(held, 1);
        let ids: Vec<&str> = free.iter().map(|candidate| candidate.thread_id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t3"]);
        // A hold placed on the org since the page was read keeps the whole batch.
        let (held, free) = split_batch(&batch, &answer(true, &[]));
        assert_eq!(held, 3);
        assert!(free.is_empty());
    }

    #[test]
    fn retention_sweep_daily_cap_can_only_be_lowered() {
        assert_eq!(daily_cap_from(None), 10_000);
        assert_eq!(daily_cap_from(Some("nonsense")), 10_000);
        assert_eq!(daily_cap_from(Some("-5")), 10_000);
        assert_eq!(daily_cap_from(Some("250")), 250);
        assert_eq!(daily_cap_from(Some(" 0 ")), 0);
        assert_eq!(daily_cap_from(Some("1000000")), 10_000);
    }

    #[test]
    fn retention_sweep_interrupted_row_carries_only_the_ledger_count() {
        let first = Utc::now() - chrono::Duration::hours(2);
        let last = first + chrono::Duration::minutes(3);
        let run = interrupted_run("rsw_1".to_owned(), "org-a".to_owned(), 4, first, last);
        assert_eq!(run.mode, "enforce");
        assert_eq!(run.error_code, Some(ERROR_INTERRUPTED));
        assert_eq!((run.policy_revision, run.window_days), (0, None));
        assert_eq!((run.started_at, run.finished_at), (first, last));
        assert_eq!(
            run.counts,
            SweepCounts {
                deleted: 4,
                ..SweepCounts::default()
            },
            "every count but the ledger's deletes is zero, so held + skipped_zdr + would_delete = candidates"
        );
        let body = build_sweep_audit_body(&run);
        assert_eq!(body["event_id"], "retention_sweep:session-core:rsw_1");
        assert_eq!(body["outcome"], "failed");
        assert_eq!(body["details"]["error_code"], "interrupted");
        assert_eq!(body["details"]["deleted"], 4);
        assert_eq!(body["details"]["candidates"], 0);
        assert!(
            !body.to_string().contains("thread"),
            "the event names no thread: {body}"
        );
    }

    /// The function body that starts at `signature`, up to the next top-level item.
    fn body_of<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source.find(signature).expect("signature");
        let rest = &source[start..];
        let end = rest[1..].find("\n}\n").map_or(rest.len(), |at| at + 3);
        &rest[..end]
    }

    #[test]
    fn retention_sweep_ledger_row_is_written_inside_each_delete_transaction() {
        let source = include_str!("retention_sweeper.rs");
        let body = body_of(source, "async fn delete_thread(");
        let delete = body
            .find("DELETE FROM threads WHERE id = $1")
            .expect("thread delete");
        let ledger = body
            .find("INSERT INTO retention_sweep_deletions (run_id, org_id) VALUES ($1, $2)")
            .expect("ledger insert");
        let commit = body.find("tx.commit().await").expect("commit");
        assert!(
            delete < ledger && ledger < commit,
            "the ledger row must commit with the delete"
        );
        assert!(
            body[ledger..commit].contains(".execute(&mut *tx)"),
            "the ledger insert runs on the delete transaction"
        );
        assert!(
            !body[ledger..commit].contains("bind(thread_id)"),
            "the ledger never names the thread"
        );
    }

    #[test]
    fn retention_sweep_error_vocabulary_matches_the_migration() {
        let migration = include_str!("../migrations/0137_retention_sweep_deletions.sql");
        for code in [
            ERROR_POLICY_UNAVAILABLE,
            ERROR_POLICY_WINDOW_INVALID,
            ERROR_POLICY_CHANGED,
            ERROR_INTERRUPTED,
        ] {
            assert!(
                migration.contains(&format!("'{code}'")),
                "{code} missing from the widened CHECK"
            );
        }
        assert!(
            !migration.contains("thread_id"),
            "the ledger holds no thread id"
        );
        let revert = include_str!("../migrations/revert/0137_retention_sweep_deletions.down.sql");
        assert!(revert.contains("DROP TABLE IF EXISTS retention_sweep_deletions"));
        assert!(revert.contains("SET error_code = NULL WHERE error_code = 'interrupted'"));
    }

    #[tokio::test]
    async fn retention_sweep_production_workspace_eraser_claims_nothing() {
        let outcome = UnavailableWorkspaceEraser.erase("org-a", "t1", "u1").await;
        assert_eq!(outcome, WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE));
    }

    #[test]
    fn retention_sweep_enforce_audit_body_reports_deletes_and_propagation() {
        let now = Utc::now();
        let run = SweepRun {
            id: "rsw_2".to_owned(),
            org_id: "org-a".to_owned(),
            mode: RECORDED_ENFORCE,
            policy_revision: 2,
            window_days: Some(30),
            started_at: now,
            finished_at: now,
            counts: SweepCounts {
                candidates: 3,
                would_delete: 3,
                deleted: 2,
                skipped_active: 1,
                cap_reached: true,
                stopped: Some(STOPPED_DELETE_FAILED),
                semantic_confirmed: 4,
                semantic_unconfirmed: 1,
                workspace_unconfirmed: 2,
                workspace_degradation: Some(WORKSPACE_ERASURE_UNAVAILABLE),
                ..SweepCounts::default()
            },
            error_code: None,
        };
        let body = build_sweep_audit_body(&run);
        let details = &body["details"];
        assert_eq!(details["mode"], "enforce");
        assert_eq!(details["deleted"], 2);
        assert_eq!(details["skipped_active"], 1);
        assert_eq!(details["cap_reached"], true);
        assert_eq!(details["stopped"], STOPPED_DELETE_FAILED);
        assert_eq!(details["semantic_confirmed"], 4);
        assert_eq!(details["semantic_unconfirmed"], 1);
        assert_eq!(details["workspace_confirmed"], 0);
        assert_eq!(details["workspace_unconfirmed"], 2);
        assert_eq!(details["workspace_degradation"], WORKSPACE_ERASURE_UNAVAILABLE);
        assert!(details["semantic_degradation"].is_null());
        assert_eq!(body["outcome"], "completed");
    }

    #[test]
    fn retention_sweep_audit_body_carries_counts_only() {
        let now = Utc::now();
        let mut counts = SweepCounts {
            candidates: 5,
            held: 1,
            skipped_zdr: 1,
            would_delete: 3,
            ..SweepCounts::default()
        };
        counts.rows_by_table.insert("messages", 12);
        let run = SweepRun {
            id: "rsw_1".to_owned(),
            org_id: "org-a".to_owned(),
            mode: "dry_run",
            policy_revision: 2,
            window_days: Some(30),
            started_at: now,
            finished_at: now,
            counts,
            error_code: None,
        };
        let body = build_sweep_audit_body(&run);
        assert_eq!(body["event"], EVENT_RETENTION_SWEEP);
        assert_eq!(body["plane"], "model");
        assert_eq!(body["producer"], "session-core");
        assert_eq!(body["event_id"], "retention_sweep:session-core:rsw_1");
        assert_eq!(body["outcome"], "completed");
        assert_eq!(body["details"]["would_delete"], 3);
        assert_eq!(body["details"]["deleted"], 0);
        assert_eq!(body["details"]["rows_by_table"]["messages"], 12);
        assert!(body["details"]["error_code"].is_null());
        // The event name is a subject token, so the outbox accepts the row.
        assert!(crate::audit_publisher::audit_subject_for_event(EVENT_RETENTION_SWEEP).is_some());
    }

    /// Every table a migration gives a `thread_id` column, read from the migration
    /// files themselves: a `thread_id TEXT` line inside a CREATE TABLE, or an ALTER
    /// TABLE that adds the column.
    fn thread_id_tables_in_migrations() -> HashSet<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut tables = HashSet::new();
        for entry in std::fs::read_dir(&dir).expect("migrations dir") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("migration");
            let mut current: Option<String> = None;
            for line in text.lines() {
                let words: Vec<&str> = line.split_whitespace().collect();
                if let Some(at) = words.windows(2).position(|pair| *pair == ["CREATE", "TABLE"]) {
                    let rest = &words[at + 2..];
                    let name = if rest.starts_with(&["IF", "NOT", "EXISTS"]) { rest.get(3) } else { rest.first() };
                    current = name.map(|name| name.trim_end_matches('(').to_owned());
                }
                if let Some(at) = words.windows(2).position(|pair| *pair == ["ALTER", "TABLE"]) {
                    let added = line.contains("ADD COLUMN IF NOT EXISTS thread_id ") || line.contains("ADD COLUMN thread_id ");
                    if added {
                        if let Some(name) = words.get(at + 2) {
                            tables.insert((*name).to_owned());
                        }
                    }
                }
                let column = line.starts_with(char::is_whitespace)
                    && words.first() == Some(&"thread_id")
                    && words.get(1).is_some_and(|kind| kind.starts_with("TEXT"));
                if column {
                    if let Some(table) = &current {
                        tables.insert(table.clone());
                    }
                }
            }
        }
        tables
    }

    #[test]
    fn thread_bound_tables_cover_every_migration() {
        let registered: HashSet<String> = THREAD_BOUND_TABLES.iter().map(|table| (*table).to_owned()).collect();
        let found = thread_id_tables_in_migrations();
        let missing: Vec<&String> = found.difference(&registered).collect();
        assert!(
            missing.is_empty(),
            "tables with a thread_id column missing from THREAD_BOUND_TABLES (C3 would leave them behind): {missing:?}"
        );
    }

    #[tokio::test]
    async fn retention_sweep_check_sends_the_service_credential_and_reads_404_as_no_policy() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org-a/retention/check"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-token", "test-token"))
            .and(body_json(serde_json::json!({ "thread_ids": ["t1"] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organization_id": "org-a", "window_days": 30, "revision": 1,
                "org_held": false, "held_thread_ids": ["t1"],
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org-gone/retention/check"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/internal/orgs/org-down/retention/check"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let client = client_for(&server.uri());
        let held = client.check("org-a", &["t1".to_owned()]).await.expect("answer").expect("policy");
        assert!(held.held_thread_ids.contains("t1"));
        assert!(client.check("org-gone", &[]).await.expect("404").is_none());
        assert!(client.check("org-down", &[]).await.is_err());
    }

    #[test]
    fn retention_sweep_client_needs_both_credentials() {
        assert!(CheckClient::from_lookup(&env(&[])).is_none());
        assert!(CheckClient::from_lookup(&env(&[(crate::org_policy::ORG_CORE_URL_ENV, "http://org-core:8080")])).is_none());
        assert!(CheckClient::from_lookup(&env(&[
            (crate::org_policy::ORG_CORE_URL_ENV, "http://org-core:8080"),
            (crate::org_policy::SERVICE_TOKEN_ENV, "  "),
        ]))
        .is_none());
    }

    mod db {
        use super::*;
        use crate::loop_checkpoint::test_support::scratch_pool;

        struct Org {
            id: String,
        }

        impl Org {
            fn new() -> Self {
                Self {
                    id: format!("rsw-org-{}", mp_ids::new_ulid()),
                }
            }

            fn thread(&self, name: &str) -> String {
                format!("{}-{name}", self.id)
            }
        }

        /// A personal thread idle for `idle_hours`, with one ended run of `retention`
        /// whose last change is as old as the thread's activity.
        async fn seed_thread(pool: &PgPool, org: &Org, name: &str, idle_hours: i32, retention: &str) -> String {
            let thread_id = org.thread(name);
            sqlx::query(
                "INSERT INTO threads (id, session_key, org_id, user_id, last_activity_at)
                 VALUES ($1, $1, $2, 'u1', now() - make_interval(hours => $3))",
            )
            .bind(&thread_id)
            .bind(&org.id)
            .bind(idle_hours)
            .execute(pool)
            .await
            .expect("seed thread");
            let (waiver_ack, waiver_connection) = if retention == "zdr_waived" {
                (Some("ack-1"), Some("conn-1"))
            } else {
                (None, None)
            };
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class,
                                   waiver_ack_id, waiver_connection_id, created_at, updated_at, ended_at)
                 VALUES ($1, $2, 'g', $3, 'u1', 'completed', $4, $5, $6,
                         now() - make_interval(hours => $7), now() - make_interval(hours => $7),
                         now() - make_interval(hours => $7))",
            )
            .bind(format!("{thread_id}-run"))
            .bind(&thread_id)
            .bind(&org.id)
            .bind(retention)
            .bind(waiver_ack)
            .bind(waiver_connection)
            .bind(idle_hours)
            .execute(pool)
            .await
            .expect("seed run");
            thread_id
        }

        /// A Space thread idle for `idle_hours`. Origin `space` goes with a space id, and the full
        /// authority context and audience revision and hash go with the space (migrations 0023, 0024
        /// and 0034).
        async fn seed_space_thread(pool: &PgPool, org: &Org, name: &str, idle_hours: i32) -> String {
            let thread_id = org.thread(name);
            sqlx::query(
                "INSERT INTO threads (id, session_key, org_id, user_id, last_activity_at, origin, space_id,
                                      space_decision_ref, recipient_audience_ref, recipient_audience_revision,
                                      recipient_audience_hash, privacy_policy_ref, resource_authorization_ref,
                                      authority_revision)
                 VALUES ($1, $1, $2, 'u1', now() - make_interval(hours => $3), 'space', 'space-1',
                         'dec-1', 'aud-1', 1, 'sha256:aud-1', 'priv-1', 'res-1', 1)",
            )
            .bind(&thread_id)
            .bind(&org.id)
            .bind(idle_hours)
            .execute(pool)
            .await
            .expect("seed space thread");
            thread_id
        }

        /// org-core answering this org with a 30-day window at revision 7.
        async fn org_core(org: &Org, org_held: bool, held: &[&str]) -> MockServer {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(format!("/internal/orgs/{}/retention/check", org.id)))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "organization_id": org.id, "window_days": 30, "revision": 7,
                    "org_held": org_held, "held_thread_ids": held,
                })))
                .mount(&server)
                .await;
            server
        }

        async fn org_count(pool: &PgPool, table: &str, org: &Org) -> i64 {
            // `table` is a constant named by the tests below, never input.
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE org_id = $1"))
                .bind(&org.id)
                .fetch_one(pool)
                .await
                .expect("count")
        }

        const DAY: i32 = 24;

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_window_boundary() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            seed_thread(&pool, &org, "inside", 30 * DAY - 1, "persistent").await;
            let old = seed_thread(&pool, &org, "past", 30 * DAY + 1, "persistent").await;
            let server = org_core(&org, false, &[]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!(run.counts.candidates, 1, "only the thread past the window is a candidate");
            assert_eq!(run.counts.would_delete, 1);
            assert_eq!(run.counts.rows_by_table.get("runs"), Some(&1), "{old}'s run is counted");
            assert_eq!(run.window_days, Some(30));
            assert_eq!(run.policy_revision, 7);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_recent_run_keeps_an_old_thread() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let thread_id = seed_thread(&pool, &org, "busy", 60 * DAY, "persistent").await;
            // A run that ended yesterday is activity, though no message was written.
            sqlx::query("UPDATE runs SET updated_at = now() - interval '1 day', ended_at = now() - interval '1 day' WHERE thread_id = $1")
                .bind(&thread_id)
                .execute(&pool)
                .await
                .expect("touch run");
            let server = org_core(&org, false, &[]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!(run.counts.candidates, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_held_thread_kept() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let held = seed_thread(&pool, &org, "held", 40 * DAY, "persistent").await;
            seed_thread(&pool, &org, "free", 40 * DAY, "persistent").await;
            let server = org_core(&org, false, &[held.as_str()]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((run.counts.candidates, run.counts.held, run.counts.would_delete), (2, 1, 1));

            let org_held = Org::new();
            seed_thread(&pool, &org_held, "a", 40 * DAY, "persistent").await;
            seed_thread(&pool, &org_held, "b", 40 * DAY, "persistent").await;
            let server = org_core(&org_held, true, &[]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org_held.id, None)
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((run.counts.held, run.counts.would_delete), (2, 0), "an org hold keeps every thread");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_space_thread_kept_and_zdr_skipped() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            seed_space_thread(&pool, &org, "room", 90 * DAY).await;
            seed_thread(&pool, &org, "zdr", 90 * DAY, "zdr").await;
            seed_thread(&pool, &org, "waived", 90 * DAY, "zdr_waived").await;
            let server = org_core(&org, false, &[]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("a row");
            // The Space thread is never a candidate; the zero-retention one holds no
            // content; a waived run's content was kept, so its thread is swept.
            assert_eq!(run.counts.candidates, 2);
            assert_eq!(run.counts.skipped_zdr, 1);
            assert_eq!(run.counts.would_delete, 1);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_org_core_unreachable_means_no_candidates() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            seed_thread(&pool, &org, "old", 90 * DAY, "persistent").await;
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(503))
                .mount(&server)
                .await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("an error row");
            assert_eq!(run.error_code, Some(ERROR_POLICY_UNAVAILABLE));
            assert_eq!(run.counts, SweepCounts::default());

            // Nothing listens at all: the same answer.
            let closed = client_for("http://127.0.0.1:9");
            let run = sweep_org(&pool, &closed, &org.id, None)
                .await
                .expect("pass")
                .expect("an error row");
            assert_eq!(run.error_code, Some(ERROR_POLICY_UNAVAILABLE));
            assert_eq!(run.counts.candidates, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_no_window_and_unknown_org_write_nothing() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            seed_thread(&pool, &org, "old", 90 * DAY, "persistent").await;
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(format!("/internal/orgs/{}/retention/check", org.id)))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "organization_id": org.id, "window_days": null, "revision": 0,
                    "org_held": false, "held_thread_ids": [],
                })))
                .mount(&server)
                .await;
            let client = client_for(&server.uri());
            assert!(sweep_org(&pool, &client, &org.id, None).await.expect("pass").is_none());
            let unknown = Org::new();
            seed_thread(&pool, &unknown, "old", 90 * DAY, "persistent").await;
            // No mock for this org: wiremock answers 404, org-core's "unknown org".
            assert!(sweep_org(&pool, &client, &unknown.id, None).await.expect("pass").is_none());
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_counts_row_written_and_nothing_deleted() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let held = seed_thread(&pool, &org, "held", 40 * DAY, "persistent").await;
            seed_thread(&pool, &org, "free", 40 * DAY, "persistent").await;
            seed_thread(&pool, &org, "zdr", 40 * DAY, "zdr").await;
            let threads_before = org_count(&pool, "threads", &org).await;
            let runs_before = org_count(&pool, "runs", &org).await;
            let server = org_core(&org, false, &[held.as_str()]).await;
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, None)
                .await
                .expect("pass")
                .expect("a row");
            record(&pool, &run).await.expect("record");

            let row: (String, i64, Option<i32>, i64, i64, i64, i64, i64, Option<String>) = sqlx::query_as(
                "SELECT mode, policy_revision, window_days, candidates, held, skipped_zdr, would_delete,
                        deleted, error_code
                   FROM retention_sweep_runs WHERE id = $1",
            )
            .bind(&run.id)
            .fetch_one(&pool)
            .await
            .expect("row");
            assert_eq!(row, ("dry_run".to_owned(), 7, Some(30), 3, 1, 1, 1, 0, None));

            let payload: serde_json::Value = sqlx::query_scalar(
                "SELECT payload FROM session_audit_outbox WHERE event_id = $1",
            )
            .bind(format!("retention_sweep:session-core:{}", run.id))
            .fetch_one(&pool)
            .await
            .expect("audit row");
            assert_eq!(payload["org_id"], org.id.as_str());
            assert_eq!(payload["details"]["would_delete"], 1);
            assert_eq!(payload["details"]["held"], 1);

            // A pass given no erasure deletes nothing.
            assert_eq!(org_count(&pool, "threads", &org).await, threads_before);
            assert_eq!(org_count(&pool, "runs", &org).await, runs_before);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_thread_bound_tables_match_the_schema() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let in_schema: Vec<String> = sqlx::query_scalar(
                "SELECT c.table_name::text
                   FROM information_schema.columns c
                   JOIN information_schema.tables t
                     ON t.table_schema = c.table_schema AND t.table_name = c.table_name
                  WHERE c.table_schema = current_schema()
                    AND c.column_name = 'thread_id'
                    AND t.table_type = 'BASE TABLE'",
            )
            .fetch_all(&pool)
            .await
            .expect("schema");
            let in_schema: HashSet<String> = in_schema.into_iter().collect();
            let registered: HashSet<String> = THREAD_BOUND_TABLES.iter().map(|table| (*table).to_owned()).collect();
            let missing: Vec<&String> = in_schema.difference(&registered).collect();
            let stale: Vec<&String> = registered.difference(&in_schema).collect();
            assert!(missing.is_empty(), "thread_id tables missing from THREAD_BOUND_TABLES: {missing:?}");
            assert!(stale.is_empty(), "THREAD_BOUND_TABLES names tables without thread_id: {stale:?}");
        }

        /// A workspace eraser that records each call's thread, owner and whether, seen from
        /// another connection, the thread was already gone: true only after the commit.
        struct RecordingEraser {
            pool: PgPool,
            calls: std::sync::Mutex<Vec<(String, String, bool)>>,
        }

        impl RecordingEraser {
            fn new(pool: &PgPool) -> Self {
                Self {
                    pool: pool.clone(),
                    calls: std::sync::Mutex::new(Vec::new()),
                }
            }

            fn calls(&self) -> Vec<(String, String, bool)> {
                self.calls.lock().expect("calls").clone()
            }
        }

        impl WorkspaceEraser for RecordingEraser {
            fn erase<'a>(
                &'a self,
                _org_id: &'a str,
                thread_id: &'a str,
                owner_id: &'a str,
            ) -> Pin<Box<dyn Future<Output = WorkspaceErasure> + Send + 'a>> {
                Box::pin(async move {
                    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM threads WHERE id = $1")
                        .bind(thread_id)
                        .fetch_one(&self.pool)
                        .await
                        .expect("count thread");
                    self.calls
                        .lock()
                        .expect("calls")
                        .push((thread_id.to_owned(), owner_id.to_owned(), left == 0));
                    WorkspaceErasure::Erased
                })
            }
        }

        fn enforce(workspaces: &RecordingEraser, daily_cap: i64) -> Erasure<'_> {
            Erasure {
                letta: None,
                workspaces,
                daily_cap,
            }
        }

        async fn thread_exists(pool: &PgPool, thread_id: &str) -> bool {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_one(pool)
                .await
                .expect("count")
                == 1
        }

        /// Content in the thread-bound tables a personal thread commonly fills, one
        /// of them (`scheduled_step_receipts`) outside `grpc::delete_thread_rows`.
        async fn seed_content(pool: &PgPool, org: &Org, thread_id: &str) {
            let run_id = format!("{thread_id}-run");
            sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', 'old transcript')")
                .bind(format!("{thread_id}-m1"))
                .bind(thread_id)
                .execute(pool)
                .await
                .expect("seed message");
            sqlx::query(
                "INSERT INTO thread_goals (id, thread_id, org_id, user_id, objective, max_turns, status)
                 VALUES ($1, $2, $3, 'u1', 'old objective', 10, 'active')",
            )
            .bind(format!("goal_{thread_id}"))
            .bind(thread_id)
            .bind(&org.id)
            .execute(pool)
            .await
            .expect("seed goal");
            sqlx::query(
                "INSERT INTO scheduled_step_receipts
                     (run_id, step_id, org_id, thread_id, space_id, schedule_id, fire_key,
                      template_digest, step_index, policy_digest, idempotency_key, receipt_id, status)
                 VALUES ($1, 's1', $2, $3, '', 'sch-1', 'fk-1', 'td', 0, 'pd', $4, $5, 'completed')",
            )
            .bind(&run_id)
            .bind(&org.id)
            .bind(thread_id)
            .bind(format!("{thread_id}-idem"))
            .bind(format!("{thread_id}-receipt"))
            .execute(pool)
            .await
            .expect("seed receipt");
            sqlx::query(
                "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, is_latest, supersedes_id)
                 VALUES ($1, $2, $3, 'thread', 'thread:llm:project', 'seeded', '', true, NULL)",
            )
            .bind(format!("{thread_id}-mem"))
            .bind(&org.id)
            .bind(thread_id)
            .execute(pool)
            .await
            .expect("seed memory");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_enforce_deletes_every_thread_bound_row_and_propagates_after_commit() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let old = seed_thread(&pool, &org, "old", 40 * DAY, "persistent").await;
            seed_content(&pool, &org, &old).await;
            let recent = seed_thread(&pool, &org, "recent", DAY, "persistent").await;
            let server = org_core(&org, false, &[]).await;
            let eraser = RecordingEraser::new(&pool);
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, Some(&enforce(&eraser, DEFAULT_DAILY_CAP)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!(run.mode, "enforce");
            assert_eq!((run.counts.candidates, run.counts.would_delete, run.counts.deleted), (1, 1, 1));
            assert_eq!(run.counts.rows_by_table.get("messages"), Some(&1));

            assert!(!thread_exists(&pool, &old).await);
            assert!(thread_exists(&pool, &recent).await, "a thread inside the window is kept");
            for table in THREAD_BOUND_TABLES {
                // `table` is one of the registered constants, never input.
                let left: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE thread_id = $1"))
                    .bind(&old)
                    .fetch_one(&pool)
                    .await
                    .expect("count");
                assert_eq!(left, 0, "{table} kept rows of a deleted thread");
            }
            let memories: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE session_id = $1")
                .bind(&old)
                .fetch_one(&pool)
                .await
                .expect("count memory");
            assert_eq!(memories, 0);

            // Propagation ran after the commit: the eraser saw the thread gone, and the
            // erased memory reached the semantic step (unconfirmed with no letta here).
            assert_eq!(eraser.calls(), vec![(old.clone(), "u1".to_owned(), true)]);
            assert_eq!(run.counts.workspace_confirmed, 1);
            assert_eq!((run.counts.semantic_confirmed, run.counts.semantic_unconfirmed), (0, 1));
            assert_eq!(run.counts.semantic_degradation, Some(crate::memory_grpc::LETTA_NOT_CONFIGURED));

            record(&pool, &run).await.expect("record");
            let row: (String, i64, i64) =
                sqlx::query_as("SELECT mode, would_delete, deleted FROM retention_sweep_runs WHERE id = $1")
                    .bind(&run.id)
                    .fetch_one(&pool)
                    .await
                    .expect("row");
            assert_eq!(row, ("enforce".to_owned(), 1, 1));
            assert_eq!(
                ledger_rows(&pool, &run.id).await,
                1,
                "the delete wrote one ledger row"
            );
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_enforce_keeps_held_threads_including_a_hold_placed_after_the_page() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let held = seed_thread(&pool, &org, "held", 40 * DAY, "persistent").await;
            let late = seed_thread(&pool, &org, "late", 40 * DAY, "persistent").await;
            let free = seed_thread(&pool, &org, "free", 40 * DAY, "persistent").await;
            let server = MockServer::start().await;
            let check_path = format!("/internal/orgs/{}/retention/check", org.id);
            // The policy read and the page check see one hold; the batch re-check sees
            // a second one, placed in between.
            Mock::given(method("POST"))
                .and(path(check_path.clone()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "organization_id": org.id, "window_days": 30, "revision": 7,
                    "org_held": false, "held_thread_ids": [held],
                })))
                .up_to_n_times(2)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(check_path))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "organization_id": org.id, "window_days": 30, "revision": 7,
                    "org_held": false, "held_thread_ids": [held, late],
                })))
                .mount(&server)
                .await;
            let eraser = RecordingEraser::new(&pool);
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, Some(&enforce(&eraser, DEFAULT_DAILY_CAP)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((run.counts.candidates, run.counts.held, run.counts.would_delete), (3, 2, 1));
            assert_eq!(run.counts.deleted, 1);
            assert!(thread_exists(&pool, &held).await);
            assert!(thread_exists(&pool, &late).await, "a hold placed after the page read is honoured");
            assert!(!thread_exists(&pool, &free).await);
            assert_eq!(eraser.calls(), vec![(free, "u1".to_owned(), true)]);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_enforce_respects_the_daily_cap_across_passes() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            for name in ["a", "b", "c"] {
                seed_thread(&pool, &org, name, 40 * DAY, "persistent").await;
            }
            let server = org_core(&org, false, &[]).await;
            let client = client_for(&server.uri());
            let eraser = RecordingEraser::new(&pool);
            let first = sweep_org(&pool, &client, &org.id, Some(&enforce(&eraser, 2)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((first.counts.would_delete, first.counts.deleted), (3, 2));
            assert!(first.counts.cap_reached);
            record(&pool, &first).await.expect("record");

            // The deletes' ledger rows count against the same 24 hours, once (the run row
            // saying 2 is not added to them).
            let second = sweep_org(&pool, &client, &org.id, Some(&enforce(&eraser, 2)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((second.counts.would_delete, second.counts.deleted), (1, 0));
            assert!(second.counts.cap_reached);
            assert_eq!(org_count(&pool, "threads", &org).await, 1);
            assert_eq!(eraser.calls().len(), 2);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_enforce_recheck_unreachable_deletes_nothing() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let old = seed_thread(&pool, &org, "old", 40 * DAY, "persistent").await;
            let server = MockServer::start().await;
            let check_path = format!("/internal/orgs/{}/retention/check", org.id);
            Mock::given(method("POST"))
                .and(path(check_path.clone()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "organization_id": org.id, "window_days": 30, "revision": 7,
                    "org_held": false, "held_thread_ids": [],
                })))
                .up_to_n_times(2)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(check_path))
                .respond_with(ResponseTemplate::new(503))
                .mount(&server)
                .await;
            let eraser = RecordingEraser::new(&pool);
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, Some(&enforce(&eraser, DEFAULT_DAILY_CAP)))
                .await
                .expect("pass")
                .expect("an error row");
            assert_eq!(run.error_code, Some(ERROR_POLICY_UNAVAILABLE));
            assert_eq!(run.counts, SweepCounts::default());
            assert!(thread_exists(&pool, &old).await);
            assert!(eraser.calls().is_empty());
            // The error row is valid for the table's checks.
            record(&pool, &run).await.expect("record");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_enforce_skips_a_thread_active_again() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let thread_id = seed_thread(&pool, &org, "reopened", 40 * DAY, "persistent").await;
            let cutoff: DateTime<Utc> = sqlx::query_scalar("SELECT now() - interval '30 days'")
                .fetch_one(&pool)
                .await
                .expect("cutoff");
            // A run opened after the page was read keeps the thread.
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class)
                 VALUES ($1, $2, 'g', $3, 'u1', 'running', 'persistent')",
            )
            .bind(format!("{thread_id}-open"))
            .bind(&thread_id)
            .bind(&org.id)
            .execute(&pool)
            .await
            .expect("open run");
            let eraser = RecordingEraser::new(&pool);
            let run_id = format!("rsw_{}", mp_ids::new_ulid());
            let outcome = delete_thread(
                &pool,
                &enforce(&eraser, DEFAULT_DAILY_CAP),
                &run_id,
                &org.id,
                &thread_id,
                "u1",
                cutoff,
            )
            .await
            .expect("delete");
            assert!(outcome.is_none());
            assert!(thread_exists(&pool, &thread_id).await);
            assert!(eraser.calls().is_empty());
            assert_eq!(
                ledger_rows(&pool, &run_id).await,
                0,
                "a kept thread writes no ledger row"
            );
        }

        /// A refused workspace erase is counted and never undoes a delete: the pass deletes both
        /// threads, records each refusal as unconfirmed, and sends each erase the owner the row held
        /// (F-f10-09).
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_sandbox_refusal_is_counted_and_the_pass_completes() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            seed_thread(&pool, &org, "a", 40 * DAY, "persistent").await;
            seed_thread(&pool, &org, "b", 40 * DAY, "persistent").await;
            let server = org_core(&org, false, &[]).await;
            let auth = crate::sandbox_workspace_eraser::test_support::mint_erase_token(&org.id).await;
            let sandbox = crate::sandbox_workspace_eraser::test_support::FakeSandboxManager {
                refusal: Some(tonic::Code::FailedPrecondition),
                ..Default::default()
            };
            let seen = sandbox.seen.clone();
            let address = crate::sandbox_workspace_eraser::test_support::serve(sandbox).await;
            let eraser =
                crate::sandbox_workspace_eraser::test_support::eraser_for(&auth.uri(), address);
            let erasure = Erasure {
                letta: None,
                workspaces: &eraser,
                daily_cap: DEFAULT_DAILY_CAP,
            };
            let run = sweep_org(&pool, &client_for(&server.uri()), &org.id, Some(&erasure))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!(run.counts.deleted, 2, "a refused erase never undoes a delete");
            assert_eq!(run.counts.workspace_unconfirmed, 2);
            assert_eq!(run.counts.workspace_confirmed, 0);
            assert_eq!(run.counts.workspace_degradation, Some(WORKSPACE_ERASURE_UNAVAILABLE));
            assert_eq!(org_count(&pool, "threads", &org).await, 0);
            record(&pool, &run).await.expect("record");

            let seen = seen.lock().expect("seen").clone();
            assert_eq!(seen.len(), 2, "one erase per deleted thread");
            assert!(seen
                .iter()
                .all(|erase| erase.org_id == org.id && erase.owner_id == "u1"));
        }

        async fn ledger_rows(pool: &PgPool, run_id: &str) -> i64 {
            sqlx::query_scalar("SELECT count(*) FROM retention_sweep_deletions WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("count ledger")
        }

        async fn run_rows(pool: &PgPool, run_id: &str) -> (i64, i64) {
            let runs: i64 =
                sqlx::query_scalar("SELECT count(*) FROM retention_sweep_runs WHERE id = $1")
                    .bind(run_id)
                    .fetch_one(pool)
                    .await
                    .expect("count runs");
            let events: i64 =
                sqlx::query_scalar("SELECT count(*) FROM session_audit_outbox WHERE event_id = $1")
                    .bind(format!("retention_sweep:session-core:{run_id}"))
                    .fetch_one(pool)
                    .await
                    .expect("count events");
            (runs, events)
        }

        /// Moves a run's ledger rows `minutes` into the past, as if the pass had died then.
        async fn age_ledger(pool: &PgPool, run_id: &str, minutes: i32) {
            sqlx::query(
                "UPDATE retention_sweep_deletions
                    SET deleted_at = deleted_at - make_interval(mins => $2)
                  WHERE run_id = $1",
            )
            .bind(run_id)
            .bind(minutes)
            .execute(pool)
            .await
            .expect("age ledger");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_crashed_pass_is_capped_and_recorded_interrupted_once() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            for name in ["a", "b", "c"] {
                seed_thread(&pool, &org, name, 40 * DAY, "persistent").await;
            }
            let server = org_core(&org, false, &[]).await;
            let client = client_for(&server.uri());
            let eraser = RecordingEraser::new(&pool);

            // The pass deletes two threads and dies before record(): no run row, two ledger rows.
            let crashed = sweep_org(&pool, &client, &org.id, Some(&enforce(&eraser, 2)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!(crashed.counts.deleted, 2);
            assert_eq!(
                ledger_rows(&pool, &crashed.id).await,
                2,
                "each delete wrote its ledger row"
            );
            assert_eq!(run_rows(&pool, &crashed.id).await, (0, 0));

            // The unrecorded deletes still count against the same 24 hours.
            assert_eq!(deleted_in_last_day(&pool, &org.id).await.expect("cap"), 2);
            let next = sweep_org(&pool, &client, &org.id, Some(&enforce(&eraser, 2)))
                .await
                .expect("pass")
                .expect("a row");
            assert_eq!((next.counts.would_delete, next.counts.deleted), (1, 0));
            assert!(next.counts.cap_reached);
            assert_eq!(org_count(&pool, "threads", &org).await, 1);

            // A run whose last delete is recent may still be running elsewhere: not recorded yet.
            reconcile_interrupted(&pool).await.expect("reconcile");
            assert_eq!(run_rows(&pool, &crashed.id).await, (0, 0));

            // Quiet past the threshold: recorded once, as interrupted, with its audit event.
            age_ledger(&pool, &crashed.id, INTERRUPTED_AFTER_MINUTES + 5).await;
            reconcile_interrupted(&pool).await.expect("reconcile");
            assert_eq!(run_rows(&pool, &crashed.id).await, (1, 1));
            let row: (String, i64, i64, i64, i64, i64, Option<i32>, Option<String>) = sqlx::query_as(
                "SELECT mode, candidates, held, would_delete, deleted, policy_revision, window_days, error_code
                   FROM retention_sweep_runs WHERE id = $1",
            )
            .bind(&crashed.id)
            .fetch_one(&pool)
            .await
            .expect("row");
            assert_eq!(
                row,
                (
                    "enforce".to_owned(),
                    0,
                    0,
                    0,
                    2,
                    0,
                    None,
                    Some("interrupted".to_owned())
                )
            );
            let payload: serde_json::Value =
                sqlx::query_scalar("SELECT payload FROM session_audit_outbox WHERE event_id = $1")
                    .bind(format!("retention_sweep:session-core:{}", crashed.id))
                    .fetch_one(&pool)
                    .await
                    .expect("audit row");
            assert_eq!(payload["org_id"], org.id.as_str());
            assert_eq!(payload["outcome"], "failed");
            assert_eq!(payload["details"]["error_code"], "interrupted");
            assert_eq!(payload["details"]["deleted"], 2);

            // A second reconcile writes nothing more, and the cap still counts the two, once.
            reconcile_interrupted(&pool).await.expect("reconcile");
            assert_eq!(run_rows(&pool, &crashed.id).await, (1, 1));
            assert_eq!(deleted_in_last_day(&pool, &org.id).await.expect("cap"), 2);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_cap_counts_pre_ledger_runs_and_nothing_twice() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            // A run recorded before the ledger existed: deletes in its row, no ledger row.
            let legacy = format!("rsw_{}", mp_ids::new_ulid());
            sqlx::query(
                "INSERT INTO retention_sweep_runs
                     (id, org_id, mode, policy_revision, window_days, started_at, finished_at,
                      candidates, would_delete, deleted)
                 VALUES ($1, $2, 'enforce', 7, 30, now() - interval '1 hour', now() - interval '1 hour', 1, 1, 1)",
            )
            .bind(&legacy)
            .bind(&org.id)
            .execute(&pool)
            .await
            .expect("legacy run");
            for name in ["a", "b", "c"] {
                seed_thread(&pool, &org, name, 40 * DAY, "persistent").await;
            }
            let server = org_core(&org, false, &[]).await;
            let eraser = RecordingEraser::new(&pool);
            let run = sweep_org(
                &pool,
                &client_for(&server.uri()),
                &org.id,
                Some(&enforce(&eraser, 3)),
            )
            .await
            .expect("pass")
            .expect("a row");
            assert_eq!(
                run.counts.deleted, 2,
                "the pre-ledger delete still counts against the cap"
            );
            record(&pool, &run).await.expect("record");
            // Two ledger rows and a run row saying 2 count as 2, plus the legacy 1.
            assert_eq!(deleted_in_last_day(&pool, &org.id).await.expect("cap"), 3);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn retention_sweep_prune_drops_only_old_ledger_rows_of_recorded_runs() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let org = Org::new();
            let recorded = format!("rsw_{}", mp_ids::new_ulid());
            let unrecorded = format!("rsw_{}", mp_ids::new_ulid());
            let recent = format!("rsw_{}", mp_ids::new_ulid());
            let hours = LEDGER_KEEP_HOURS + 1;
            for (run_id, age) in [(&recorded, hours), (&unrecorded, hours), (&recent, 1)] {
                sqlx::query(
                    "INSERT INTO retention_sweep_deletions (run_id, org_id, deleted_at)
                     VALUES ($1, $2, now() - make_interval(hours => $3))",
                )
                .bind(run_id)
                .bind(&org.id)
                .bind(age)
                .execute(&pool)
                .await
                .expect("ledger row");
            }
            for run_id in [&recorded, &recent] {
                let at: DateTime<Utc> = sqlx::query_scalar(
                    "SELECT min(deleted_at) FROM retention_sweep_deletions WHERE run_id = $1",
                )
                .bind(run_id)
                .fetch_one(&pool)
                .await
                .expect("time");
                record(
                    &pool,
                    &interrupted_run(run_id.clone(), org.id.clone(), 1, at, at),
                )
                .await
                .expect("record");
            }
            prune_ledger(&pool).await.expect("prune");
            assert_eq!(
                ledger_rows(&pool, &recorded).await,
                0,
                "an old row of a recorded run goes"
            );
            assert_eq!(
                ledger_rows(&pool, &unrecorded).await,
                1,
                "a row whose run is not recorded stays"
            );
            assert_eq!(
                ledger_rows(&pool, &recent).await,
                1,
                "a row inside the keep window stays"
            );

            // The reconcile records the old orphan; only then may the prune take it.
            reconcile_interrupted(&pool).await.expect("reconcile");
            assert_eq!(run_rows(&pool, &unrecorded).await, (1, 1));
            prune_ledger(&pool).await.expect("prune");
            assert_eq!(ledger_rows(&pool, &unrecorded).await, 0);
        }
    }
}
