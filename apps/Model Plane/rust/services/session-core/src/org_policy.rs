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
use std::{sync::OnceLock, time::Duration};
use tracing::{debug, info, warn};

use crate::memory_control::OrgMemoryPolicy;

pub(crate) const FAIL_CLOSED_ENV: &str = "ORG_POLICY_FAIL_CLOSED";
pub(crate) const POLL_SECS_ENV: &str = "ORG_POLICY_POLL_SECS";
pub(crate) const MAX_STALE_ENV: &str = "ORG_POLICY_MAX_STALE";
pub(crate) const LEARNING_DEFAULT_ENV: &str = "ORG_LEARNING_DEFAULT";
pub(crate) const ORG_CORE_URL_ENV: &str = "ORG_CORE_URL";
pub(crate) const SERVICE_TOKEN_ENV: &str = "SESSION_ORG_CORE_SERVICE_TOKEN";

/// The principal name org-core's credential registry knows this service by.
const SERVICE_ID: &str = "session-core";
const DEFAULT_POLL_SECS: u64 = 60;
/// Fifteen minutes: several missed polls, short enough that a disabled org is not
/// served from a copy that old.
const DEFAULT_MAX_STALE_SECS: i64 = 900;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 4096;

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
/// no `Debug`.
pub(crate) struct Poller {
    base_url: String,
    token: String,
    client: reqwest::Client,
    interval: Duration,
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

    /// One pass over every org this service knows. Returns how many refreshed.
    pub(crate) async fn poll_once(&self, pool: &PgPool) -> Result<usize> {
        let orgs: Vec<String> = sqlx::query_scalar(
            "SELECT org_id FROM org_learning_policy_mirror UNION SELECT DISTINCT org_id FROM threads",
        )
        .fetch_all(pool)
        .await?;
        let mut refreshed = 0usize;
        for org_id in &orgs {
            match self.refresh_org(pool, org_id).await {
                Ok(()) => {
                    refreshed += 1;
                    metrics::counter!("mp_session_org_policy_polls_total", "outcome" => "ok")
                        .increment(1);
                }
                Err(error) => {
                    metrics::counter!("mp_session_org_policy_polls_total", "outcome" => "error")
                        .increment(1);
                    warn!(%org_id, %error, "org agent policy refresh failed; the mirror keeps ageing");
                }
            }
        }
        let oldest: Option<f64> = sqlx::query_scalar(
            "SELECT EXTRACT(EPOCH FROM now() - MIN(fetched_at))::float8 FROM org_learning_policy_mirror",
        )
        .fetch_one(pool)
        .await?;
        metrics::gauge!("mp_session_org_policy_oldest_mirror_age_seconds").set(oldest.unwrap_or(0.0));
        Ok(refreshed)
    }
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
    use wiremock::{
        matchers::{header, method, path},
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
}
