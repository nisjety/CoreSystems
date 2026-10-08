//! The per-organization processing policy, as the embedding engine reads it (CR-T11).
//!
//! `org_processing_policy` holds an operator's overrides for background content processing,
//! one row per organization, written through the audited admin API of `data-quality-go`
//! (`PUT /v1/admin/orgs/{org}/processing-policy`, operator-only, reason required for an
//! off). This module turns a row into what the context pipeline does:
//!
//! * **No row means on.** A row exists only because an operator set something, so the
//!   absence of one is the system default, which is on (owner ruling of 2026-10-03:
//!   contextual retrieval is not switched off; the per-organization off exists for the
//!   evaluation baseline organizations and for incidents).
//! * **`inherit` and `on` generate.** `on` does not override the global kill switch
//!   (`CONTEXTUAL_RETRIEVAL_ENABLED=false`): this module is only consulted when generation
//!   is on at all, so an organization set to `on` is never switched on against an incident
//!   hold.
//! * **`off` makes zero provider calls.** The pipeline records `skipped_policy` on the
//!   organization's units and embeds them from what they already have. Never silent: the
//!   status is the marker, and the coverage view counts it.
//! * **A pin** (`context_prompt_version`) is the organization's prompt version for the
//!   contexts it generates (the canary of a prompt rollout). An unknown value is an error
//!   and never the default: a typo must not quietly keep the default, so it is `Unknown`.
//! * **The budget** (`budget_mode`, `monthly_budget_usd`, CR-T9b) rides on the same row and
//!   the same read, so the switch, the pin and the budget are one consistent view of it:
//!   `inherit` / NULL leave the deployment's setting in force (see [`crate::budget`]). The
//!   budget is read only for an organization that generates, since an organization that is off
//!   or unknown generates nothing and has no spend to hold. An unrecognized `budget_mode` or a
//!   figure that is not a number from 0 up is `Unknown`, exactly as a typo in the switch is: a
//!   gate that cannot be read is never read as open.
//!
//! ## Reading it
//!
//! A batch mixes organizations, so the reader serves a set of organizations with one query
//! (`WHERE org_id = ANY($1)`, an explicit predicate: the engine connects as the superuser,
//! for which row-level security is inert) and keeps each answer for 30 seconds. A policy
//! change therefore reaches the engine within 30 seconds, which the admin API says.
//!
//! ## When the policy cannot be read
//!
//! * **An answer we already have is kept** (stale-if-error), without a limit. An
//!   organization whose policy says off stays off when the table becomes unreachable: the
//!   fallback to on never applies to an organization that is known to be off.
//! * **The table cannot be consulted at all** in this database (it does not exist, or the
//!   role may not read it: the migration is not applied, or was rolled back) and nothing
//!   is known about the organization: the deployment default applies, which is on. There
//!   can be no policy to honour in a database that has no policy table.
//! * **Any other failure** (a timeout, a reset connection) with nothing known about the
//!   organization is [`OrgSetting::Unknown`]: nothing is generated and nothing is
//!   recorded; the units stay as they are and are picked up on the next pass. This is
//!   deliberately NOT a fallback to on: it is the one case in which falling open could
//!   send content to a provider for an organization an operator has switched off, and
//!   waiting costs a few seconds of raw embedding and no money.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::time::Instant;

use crate::budget::OrgBudget;
use crate::context::PromptVersion;

/// How long an organization's policy is kept before it is read again.
pub(crate) const POLICY_CACHE_TTL: Duration = Duration::from_secs(30);

/// After a failed read, how long before the same organization is read again. Without it a
/// database that is struggling would be asked once per batch per organization.
const RETRY_AFTER_FAILED_READ: Duration = Duration::from_secs(5);

/// What the pipeline does for one organization.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OrgSetting {
    /// Generate contexts. `pin` is the organization's prompt version, when it has one, and
    /// `budget` what its row says about its background LLM budget (`OrgBudget::INHERIT` when it
    /// says nothing, which is the case of an organization with no row).
    Generate {
        pin: Option<PromptVersion>,
        budget: OrgBudget,
    },
    /// An operator switched contextual retrieval off for the organization: zero provider
    /// calls, `skipped_policy` on its units.
    Off,
    /// The policy cannot be determined (see the module documentation). Nothing is
    /// generated and nothing is recorded.
    Unknown,
}

/// One row of `org_processing_policy`, as far as the engine reads it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PolicyRow {
    pub org_id: String,
    pub contextual_retrieval: String,
    pub context_prompt_version: Option<String>,
    /// `inherit`, `alert_only`, `throttle` or `stop`.
    pub budget_mode: String,
    /// The organization's own monthly figure in dollars, when it has one.
    pub monthly_budget_usd: Option<f64>,
}

/// Why a read failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// The table cannot be consulted at all in this database: it does not exist, its schema
    /// does not, or the role may not read it.
    TableUnreachable(String),
    /// Anything else.
    Transient(String),
}

pub(crate) type SourceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ReadError>> + Send + 'a>>;

/// Where policy rows come from.
pub(crate) trait PolicySource: Send + Sync {
    /// The rows of the organizations named; an organization without a row is simply not in
    /// the answer.
    fn read<'a>(&'a self, org_ids: &'a [String]) -> SourceFuture<'a, Vec<PolicyRow>>;
}

/// `org_processing_policy` in Postgres.
pub(crate) struct PgPolicySource {
    pool: PgPool,
}

impl PgPolicySource {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// SQLSTATEs that say the table cannot be consulted at all: `undefined_table`,
/// `insufficient_privilege`, `invalid_schema_name`.
fn is_structural(code: Option<&str>) -> bool {
    matches!(code, Some("42P01" | "42501" | "3F000"))
}

fn classify(error: &sqlx::Error) -> ReadError {
    match error {
        sqlx::Error::Database(database) if is_structural(database.code().as_deref()) => {
            ReadError::TableUnreachable(database.message().to_owned())
        }
        other => ReadError::Transient(other.to_string()),
    }
}

impl PolicySource for PgPolicySource {
    fn read<'a>(&'a self, org_ids: &'a [String]) -> SourceFuture<'a, Vec<PolicyRow>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT org_id, contextual_retrieval, context_prompt_version, budget_mode, \
                        monthly_budget_usd::float8 AS monthly_budget_usd \
                   FROM org_processing_policy WHERE org_id = ANY($1)",
            )
            .bind(org_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| classify(&error))?;
            Ok(rows
                .iter()
                .map(|row| PolicyRow {
                    org_id: row.get("org_id"),
                    contextual_retrieval: row.get("contextual_retrieval"),
                    context_prompt_version: row.get("context_prompt_version"),
                    budget_mode: row.get("budget_mode"),
                    monthly_budget_usd: row.get("monthly_budget_usd"),
                })
                .collect())
        })
    }
}

/// What an organization with no row has: on, with its prompt and budget inherited.
const DEFAULT_SETTING: OrgSetting = OrgSetting::Generate {
    pin: None,
    budget: OrgBudget::INHERIT,
};

/// What a row means. `None` is no row, which is on. An unrecognized switch value, pin, budget
/// mode or budget figure is an error: the table's CHECK constraints and the admin API both
/// refuse them, so one that arrives was written around both, and it is not guessed at. (An
/// organization that is off is off whatever its budget columns say, so they are not read for it.)
fn setting_of(row: Option<&PolicyRow>) -> Result<OrgSetting, String> {
    let Some(row) = row else {
        return Ok(DEFAULT_SETTING);
    };
    match row.contextual_retrieval.as_str() {
        "off" => Ok(OrgSetting::Off),
        "inherit" | "on" => {
            let pin = match row.context_prompt_version.as_deref() {
                None => None,
                Some(version) => Some(PromptVersion::parse(version)?),
            };
            let budget = OrgBudget::from_row(&row.budget_mode, row.monthly_budget_usd)?;
            Ok(OrgSetting::Generate { pin, budget })
        }
        other => Err(format!(
            "unknown contextual_retrieval value `{other}` (expected inherit, on or off)"
        )),
    }
}

struct Entry {
    setting: OrgSetting,
    fetched_at: Instant,
    /// After a failed refresh: serve the entry as it is until this moment.
    retry_not_before: Option<Instant>,
}

/// The policy of the organizations a batch holds, cached.
pub(crate) struct PolicyReader {
    source: Arc<dyn PolicySource>,
    ttl: Duration,
    cache: Mutex<HashMap<String, Entry>>,
}

impl PolicyReader {
    /// A reader with the 30-second cache.
    pub(crate) fn new(source: Arc<dyn PolicySource>) -> Self {
        Self::with_ttl(source, POLICY_CACHE_TTL)
    }

    /// A reader with its own cache lifetime. Zero reads the policy on every call.
    pub(crate) fn with_ttl(source: Arc<dyn PolicySource>, ttl: Duration) -> Self {
        Self {
            source,
            ttl,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// What to do for each of the organizations. Infallible: an organization that cannot be
    /// determined is [`OrgSetting::Unknown`], never a guess.
    ///
    /// Call it only when contextual retrieval is enabled globally. The caller is the
    /// context pipeline, which does not generate at all when the global switch is off.
    pub(crate) async fn settings(&self, org_ids: &[&str]) -> HashMap<String, OrgSetting> {
        let wanted: BTreeSet<&str> = org_ids.iter().copied().collect();
        let mut result: HashMap<String, OrgSetting> = HashMap::with_capacity(wanted.len());
        let mut to_read: Vec<String> = Vec::new();
        {
            let now = Instant::now();
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            for org in &wanted {
                match cache.get(*org) {
                    Some(entry)
                        if now.saturating_duration_since(entry.fetched_at) < self.ttl
                            || entry.retry_not_before.is_some_and(|until| now < until) =>
                    {
                        result.insert((*org).to_owned(), entry.setting);
                    }
                    _ => to_read.push((*org).to_owned()),
                }
            }
        }
        if to_read.is_empty() {
            return result;
        }

        match self.source.read(&to_read).await {
            Ok(rows) => self.record_rows(&to_read, &rows, &mut result),
            Err(error) => self.record_failure(&to_read, &error, &mut result),
        }
        result
    }

    fn record_rows(
        &self,
        read: &[String],
        rows: &[PolicyRow],
        result: &mut HashMap<String, OrgSetting>,
    ) {
        let by_org: HashMap<&str, &PolicyRow> =
            rows.iter().map(|row| (row.org_id.as_str(), row)).collect();
        let now = Instant::now();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        for org in read {
            let setting = match setting_of(by_org.get(org.as_str()).copied()) {
                Ok(setting) => setting,
                Err(reason) => {
                    tracing::error!(
                        org_id = %org,
                        %reason,
                        "org_processing_policy holds a value the engine does not know; \
                         nothing is generated for this organization until it is corrected"
                    );
                    OrgSetting::Unknown
                }
            };
            cache.insert(
                org.clone(),
                Entry {
                    setting,
                    fetched_at: now,
                    retry_not_before: None,
                },
            );
            result.insert(org.clone(), setting);
        }
    }

    fn record_failure(
        &self,
        read: &[String],
        error: &ReadError,
        result: &mut HashMap<String, OrgSetting>,
    ) {
        let now = Instant::now();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut kept, mut defaulted, mut unknown) = (0usize, 0usize, 0usize);
        for org in read {
            if let Some(entry) = cache.get_mut(org) {
                // An answer we have is kept: an organization that is off stays off.
                entry.retry_not_before = Some(now + RETRY_AFTER_FAILED_READ);
                result.insert(org.clone(), entry.setting);
                kept += 1;
                continue;
            }
            match error {
                ReadError::TableUnreachable(_) => {
                    // No policy table, so no policy to honour: the deployment default.
                    let setting = DEFAULT_SETTING;
                    cache.insert(
                        org.clone(),
                        Entry {
                            setting,
                            fetched_at: now,
                            retry_not_before: None,
                        },
                    );
                    result.insert(org.clone(), setting);
                    defaulted += 1;
                }
                ReadError::Transient(_) => {
                    // Not cached: the next batch asks again after the cool-down, which a
                    // failed read of nothing cannot record, so ask again soon.
                    result.insert(org.clone(), OrgSetting::Unknown);
                    unknown += 1;
                }
            }
        }
        let (kind, detail) = match error {
            ReadError::TableUnreachable(detail) => ("table_unreachable", detail),
            ReadError::Transient(detail) => ("transient", detail),
        };
        tracing::error!(
            kind,
            detail = %detail,
            organizations = read.len(),
            kept_known = kept,
            defaulted_on = defaulted,
            unknown,
            "org_processing_policy could not be read: known policies are kept, an organization \
             that is not known is either on by default (the table is unreachable) or waits \
             (any other failure); an organization known to be off is never turned on by a failed read"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::budget::BudgetMode;

    fn row(org: &str, switch: &str, pin: Option<&str>) -> PolicyRow {
        PolicyRow {
            org_id: org.to_owned(),
            contextual_retrieval: switch.to_owned(),
            context_prompt_version: pin.map(str::to_owned),
            budget_mode: "inherit".to_owned(),
            monthly_budget_usd: None,
        }
    }

    /// A row that says something about the budget and nothing else.
    fn budget_row(org: &str, mode: &str, figure: Option<f64>) -> PolicyRow {
        PolicyRow {
            budget_mode: mode.to_owned(),
            monthly_budget_usd: figure,
            ..row(org, "inherit", None)
        }
    }

    /// A source whose answers a test scripts, and which records what it was asked.
    #[derive(Default)]
    struct Script {
        rows: Mutex<Vec<PolicyRow>>,
        failure: Mutex<Option<fn() -> ReadError>>,
        asked: Mutex<Vec<Vec<String>>>,
        reads: AtomicUsize,
    }

    impl Script {
        fn with_rows(rows: Vec<PolicyRow>) -> Arc<Self> {
            let script = Arc::new(Self::default());
            *script.rows.lock().unwrap() = rows;
            script
        }

        fn set_rows(&self, rows: Vec<PolicyRow>) {
            *self.rows.lock().unwrap() = rows;
        }

        fn fail_with(&self, failure: Option<fn() -> ReadError>) {
            *self.failure.lock().unwrap() = failure;
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        fn asked(&self) -> Vec<Vec<String>> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl PolicySource for Script {
        fn read<'a>(&'a self, org_ids: &'a [String]) -> SourceFuture<'a, Vec<PolicyRow>> {
            Box::pin(async move {
                self.reads.fetch_add(1, Ordering::SeqCst);
                self.asked.lock().unwrap().push(org_ids.to_vec());
                if let Some(failure) = *self.failure.lock().unwrap() {
                    return Err(failure());
                }
                Ok(self
                    .rows
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|row| org_ids.contains(&row.org_id))
                    .cloned()
                    .collect())
            })
        }
    }

    fn unreachable_table() -> ReadError {
        ReadError::TableUnreachable("relation \"org_processing_policy\" does not exist".into())
    }

    fn transient() -> ReadError {
        ReadError::Transient("connection reset".into())
    }

    fn reader(script: &Arc<Script>) -> PolicyReader {
        PolicyReader::new(script.clone())
    }

    const ON: OrgSetting = OrgSetting::Generate {
        pin: None,
        budget: OrgBudget::INHERIT,
    };

    async fn one(reader: &PolicyReader, org: &str) -> OrgSetting {
        reader.settings(&[org]).await[org]
    }

    // ── what a row means ────────────────────────────────────────────────────────────

    /// The default: no row, no policy, on.
    #[tokio::test(start_paused = true)]
    async fn an_absent_row_means_on() {
        let script = Script::with_rows(vec![]);
        assert_eq!(one(&reader(&script), "org-a").await, ON);
    }

    #[tokio::test(start_paused = true)]
    async fn inherit_and_on_generate_and_off_does_not() {
        let script = Script::with_rows(vec![
            row("org-inherit", "inherit", None),
            row("org-on", "on", None),
            row("org-off", "off", None),
        ]);
        let settings = reader(&script)
            .settings(&["org-inherit", "org-on", "org-off", "org-none"])
            .await;
        assert_eq!(settings["org-inherit"], ON);
        assert_eq!(settings["org-on"], ON);
        assert_eq!(settings["org-off"], OrgSetting::Off);
        assert_eq!(settings["org-none"], ON);
    }

    #[tokio::test(start_paused = true)]
    async fn a_pin_is_the_organizations_prompt_version() {
        let script = Script::with_rows(vec![
            row("org-v2", "inherit", Some("ctx-v2")),
            row("org-v1", "on", Some(" ctx-v1 ")),
        ]);
        let settings = reader(&script).settings(&["org-v2", "org-v1"]).await;
        assert_eq!(
            settings["org-v2"],
            OrgSetting::Generate {
                pin: Some(PromptVersion::V2),
                budget: OrgBudget::INHERIT
            }
        );
        assert_eq!(
            settings["org-v1"],
            OrgSetting::Generate {
                pin: Some(PromptVersion::V1),
                budget: OrgBudget::INHERIT
            }
        );
    }

    // ── the budget (CR-T9b) ─────────────────────────────────────────────────────────

    /// An organization with no row inherits the deployment's budget, and so does one whose row
    /// says `inherit` and has no figure: the budget is something an operator sets, never
    /// something an organization has by being there.
    #[tokio::test(start_paused = true)]
    async fn an_organization_with_no_budget_of_its_own_inherits_the_deployments() {
        let script = Script::with_rows(vec![
            row("org-row", "inherit", None),
            budget_row("org-inherit", "inherit", None),
        ]);
        let settings = reader(&script)
            .settings(&["org-none", "org-row", "org-inherit"])
            .await;
        for org in ["org-none", "org-row", "org-inherit"] {
            assert_eq!(settings[org], ON, "{org}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_organizations_own_mode_and_figure_ride_on_the_same_read_as_its_switch() {
        let script = Script::with_rows(vec![
            budget_row("org-stop", "stop", Some(12.5)),
            budget_row("org-alert", "alert_only", None),
            budget_row("org-figure", "inherit", Some(0.0)),
            PolicyRow {
                budget_mode: "throttle".to_owned(),
                monthly_budget_usd: Some(40.0),
                ..row("org-pinned", "on", Some("ctx-v2"))
            },
        ]);
        let settings = reader(&script)
            .settings(&["org-stop", "org-alert", "org-figure", "org-pinned"])
            .await;
        assert_eq!(
            settings["org-stop"],
            OrgSetting::Generate {
                pin: None,
                budget: OrgBudget {
                    mode: Some(BudgetMode::Stop),
                    monthly_usd: Some(12.5)
                }
            }
        );
        assert_eq!(
            settings["org-alert"],
            OrgSetting::Generate {
                pin: None,
                budget: OrgBudget {
                    mode: Some(BudgetMode::AlertOnly),
                    monthly_usd: None
                }
            }
        );
        assert_eq!(
            settings["org-figure"],
            OrgSetting::Generate {
                pin: None,
                budget: OrgBudget {
                    mode: None,
                    monthly_usd: Some(0.0)
                }
            }
        );
        assert_eq!(
            settings["org-pinned"],
            OrgSetting::Generate {
                pin: Some(PromptVersion::V2),
                budget: OrgBudget {
                    mode: Some(BudgetMode::Throttle),
                    monthly_usd: Some(40.0)
                }
            }
        );
        assert_eq!(
            script.reads(),
            1,
            "one query serves the switch, the pin and the budget"
        );
    }

    /// A budget that cannot be read is not an open gate: an unrecognized mode or a figure that
    /// is not a number from 0 up (a hand-written NaN, say) is `Unknown`, and nothing is
    /// generated for the organization until it is corrected. An organization that is off is off
    /// whatever its budget columns say.
    #[tokio::test(start_paused = true)]
    async fn an_unreadable_budget_is_unknown_never_an_open_gate() {
        let script = Script::with_rows(vec![
            budget_row("org-typo", "sometimes", None),
            budget_row("org-upper", "STOP", None),
            budget_row("org-nan", "inherit", Some(f64::NAN)),
            budget_row("org-negative", "stop", Some(-1.0)),
            PolicyRow {
                budget_mode: "sometimes".to_owned(),
                ..row("org-off", "off", None)
            },
        ]);
        let settings = reader(&script)
            .settings(&[
                "org-typo",
                "org-upper",
                "org-nan",
                "org-negative",
                "org-off",
            ])
            .await;
        for org in ["org-typo", "org-upper", "org-nan", "org-negative"] {
            assert_eq!(settings[org], OrgSetting::Unknown, "{org}");
        }
        assert_eq!(settings["org-off"], OrgSetting::Off);
    }

    /// With the table unreachable and nothing known, the deployment default applies, budget
    /// included: there is no policy to honour in a database that has none.
    #[tokio::test(start_paused = true)]
    async fn with_no_policy_table_the_budget_is_the_deployments_too() {
        let script = Script::with_rows(vec![]);
        script.fail_with(Some(unreachable_table));
        assert_eq!(one(&reader(&script), "org-a").await, ON);
    }

    /// An organization whose own budget was read keeps it when a refresh fails: a stop does not
    /// become an alert because the database blinked.
    #[tokio::test(start_paused = true)]
    async fn a_budget_that_was_read_is_kept_when_a_refresh_fails() {
        let script = Script::with_rows(vec![budget_row("org-stop", "stop", Some(10.0))]);
        let reader = reader(&script);
        let stop = OrgSetting::Generate {
            pin: None,
            budget: OrgBudget {
                mode: Some(BudgetMode::Stop),
                monthly_usd: Some(10.0),
            },
        };
        assert_eq!(one(&reader, "org-stop").await, stop);
        script.fail_with(Some(transient));
        tokio::time::advance(Duration::from_secs(31)).await;
        assert_eq!(one(&reader, "org-stop").await, stop);
    }

    /// A typo is not the default: an unknown pin or switch is `Unknown` (nothing generated),
    /// and an off stays off whatever the pin says.
    #[tokio::test(start_paused = true)]
    async fn an_unrecognized_value_is_unknown_never_a_quiet_default() {
        let script = Script::with_rows(vec![
            row("org-typo-pin", "inherit", Some("ctx-v9")),
            row("org-typo-switch", "disabled", None),
            row("org-off-bad-pin", "off", Some("ctx-v9")),
        ]);
        let settings = reader(&script)
            .settings(&["org-typo-pin", "org-typo-switch", "org-off-bad-pin"])
            .await;
        assert_eq!(settings["org-typo-pin"], OrgSetting::Unknown);
        assert_eq!(settings["org-typo-switch"], OrgSetting::Unknown);
        assert_eq!(settings["org-off-bad-pin"], OrgSetting::Off);
    }

    /// The admin API accepts exactly the versions the engine knows (`policy.PromptVersions`
    /// in data-quality-go, pinned there as well).
    #[test]
    fn the_prompt_versions_are_the_ones_the_admin_api_accepts() {
        let known: Vec<&str> = PromptVersion::ALL.iter().map(|v| v.as_str()).collect();
        assert_eq!(known, ["ctx-v1", "ctx-v2"]);
    }

    // ── the cache ───────────────────────────────────────────────────────────────────

    #[tokio::test(start_paused = true)]
    async fn a_policy_is_kept_for_thirty_seconds_and_read_again_after() {
        let script = Script::with_rows(vec![row("org-a", "off", None)]);
        let reader = reader(&script);
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 1);

        // Within the lifetime the answer is the cached one, even though the row changed.
        script.set_rows(vec![]);
        tokio::time::advance(Duration::from_secs(29)).await;
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 1, "no read inside the cache lifetime");

        // After it, the change is seen: this is the bound the admin API promises.
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(script.reads(), 2);
        // And an organization with no row is cached as well (no query per batch).
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(script.reads(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_zero_lifetime_reads_every_time() {
        let script = Script::with_rows(vec![row("org-a", "off", None)]);
        let reader = PolicyReader::with_ttl(script.clone(), Duration::ZERO);
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        script.set_rows(vec![]);
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(script.reads(), 2);
    }

    /// A batch of several organizations is one query, and a later batch asks only for the
    /// organizations whose answer has expired.
    #[tokio::test(start_paused = true)]
    async fn the_organizations_of_a_batch_are_read_with_one_query() {
        let script = Script::with_rows(vec![row("org-b", "off", None)]);
        let reader = reader(&script);
        let settings = reader.settings(&["org-a", "org-b", "org-a", "org-c"]).await;
        assert_eq!(settings.len(), 3);
        assert_eq!(script.reads(), 1);
        assert_eq!(
            script.asked()[0],
            ["org-a", "org-b", "org-c"],
            "each organization once, in a stable order"
        );

        tokio::time::advance(Duration::from_secs(20)).await;
        reader.settings(&["org-d"]).await;
        assert_eq!(
            script.asked()[1],
            ["org-d"],
            "only the organization not yet known"
        );
        tokio::time::advance(Duration::from_secs(11)).await;
        reader.settings(&["org-a", "org-d"]).await;
        assert_eq!(
            script.asked()[2],
            ["org-a"],
            "org-d is still fresh, org-a expired"
        );
    }

    // ── when the table cannot be read ───────────────────────────────────────────────

    /// The invariant the manifest names: an organization whose policy says off is never turned on
    /// by a failed read, however the read fails and however long it fails.
    #[tokio::test(start_paused = true)]
    async fn an_organization_that_is_off_stays_off_when_the_table_becomes_unreachable() {
        for failure in [unreachable_table as fn() -> ReadError, transient] {
            let script = Script::with_rows(vec![row("org-off", "off", None)]);
            let reader = reader(&script);
            assert_eq!(one(&reader, "org-off").await, OrgSetting::Off);

            script.fail_with(Some(failure));
            // Many lifetimes of failure, each past the retry cool-down.
            for _ in 0..40 {
                tokio::time::advance(Duration::from_secs(31)).await;
                assert_eq!(
                    one(&reader, "org-off").await,
                    OrgSetting::Off,
                    "a failed read turned an organization on"
                );
            }

            // And when the table comes back, the answer is read again.
            script.fail_with(None);
            script.set_rows(vec![]);
            tokio::time::advance(Duration::from_secs(31)).await;
            assert_eq!(one(&reader, "org-off").await, ON);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_refresh_is_not_retried_for_five_seconds() {
        let script = Script::with_rows(vec![row("org-a", "off", None)]);
        let reader = reader(&script);
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 1);

        script.fail_with(Some(transient));
        tokio::time::advance(Duration::from_secs(31)).await;
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 2, "the refresh was attempted");
        // A database that is struggling is not asked once per batch.
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 2);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(one(&reader, "org-a").await, OrgSetting::Off);
        assert_eq!(script.reads(), 3, "and again after the cool-down");
    }

    /// A database that has no policy table has no policy to honour: the deployment default
    /// (on), asked for once per lifetime rather than once per batch.
    #[tokio::test(start_paused = true)]
    async fn an_unreachable_table_with_nothing_known_is_the_deployment_default() {
        let script = Script::with_rows(vec![]);
        script.fail_with(Some(unreachable_table));
        let reader = reader(&script);
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(script.reads(), 1, "the fallback is cached for the lifetime");
        tokio::time::advance(Duration::from_secs(31)).await;
        assert_eq!(one(&reader, "org-a").await, ON);
        assert_eq!(script.reads(), 2);
    }

    /// Any other failure with nothing known: NOT on. It is the one case in which falling open
    /// could send content for an organization an operator switched off.
    #[tokio::test(start_paused = true)]
    async fn a_transient_failure_with_nothing_known_is_unknown_and_never_on() {
        let script = Script::with_rows(vec![row("org-off", "off", None)]);
        script.fail_with(Some(transient));
        let reader = reader(&script);
        let settings = reader.settings(&["org-off", "org-other"]).await;
        assert_eq!(settings["org-off"], OrgSetting::Unknown);
        assert_eq!(settings["org-other"], OrgSetting::Unknown);

        // Not cached as an answer: the next call asks again, and the table's answer wins.
        script.fail_with(None);
        let settings = reader.settings(&["org-off", "org-other"]).await;
        assert_eq!(settings["org-off"], OrgSetting::Off);
        assert_eq!(settings["org-other"], ON);
    }

    /// A failure affects only the organizations it had to read: the ones with a fresh answer
    /// are served from the cache and keep their answer.
    #[tokio::test(start_paused = true)]
    async fn a_failed_read_for_new_organizations_leaves_the_known_ones_alone() {
        let script = Script::with_rows(vec![row("org-off", "off", None)]);
        let reader = reader(&script);
        assert_eq!(one(&reader, "org-off").await, OrgSetting::Off);

        script.fail_with(Some(transient));
        let settings = reader.settings(&["org-off", "org-new"]).await;
        assert_eq!(settings["org-off"], OrgSetting::Off);
        assert_eq!(settings["org-new"], OrgSetting::Unknown);
        assert_eq!(script.asked().last().unwrap(), &["org-new"]);
    }

    #[test]
    fn only_a_structural_sqlstate_makes_the_table_unreachable() {
        for code in ["42P01", "42501", "3F000"] {
            assert!(is_structural(Some(code)), "{code}");
        }
        for code in ["57014", "08006", "40001", "40P01", "53300", "23505"] {
            assert!(!is_structural(Some(code)), "{code}");
        }
        assert!(!is_structural(None));
    }
}
