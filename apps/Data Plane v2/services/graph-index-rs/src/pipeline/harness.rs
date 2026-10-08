//! The rig the pipeline's tests run on.
//!
//! Two implementations of [`Ports`] sit behind one test-only [`Control`] trait, so every
//! scenario in `tests.rs` is written once and runs twice:
//!
//! * [`Memory`] keeps the chunks' state in a map, with a clock the test moves. It always runs.
//!   Its rules for "due", "claim" and "mark" restate the SQL in `state_store`, and the second
//!   implementation exists to prove that they say the same thing.
//! * [`Database`] is the production [`Live`] over the real schema (`init.sql` plus every
//!   migration, as the migrated test database is built) with the real state SQL, the real graph
//!   writes and the real organization-scoped transactions: `SET LOCAL ROLE dataplane_app`, so row-level security is
//!   real. Its tests are `#[ignore]`d behind `MIGRATED_TEST_DATABASE_URL` and FAIL (never skip)
//!   under the Postgres contract runner (`tests/scripts/run-postgres-contract-tests.sh`).
//!
//! Both talk to the real [`GraphExtractor`]: the real bearer client against a stand-in Auth Core
//! that replays what the real mint produced, the real metering into an in-memory ledger, and the
//! real typed errors, with only the gRPC transport answered from a script (see `testing`).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use usage_ledger_rs::{LedgerSink, MemoryLedger};

use super::*;
use crate::budget::{BudgetMode, GlobalBudget, PriceTable, Switch};
use crate::state::{DEFAULT_MAX_ATTEMPTS, LEGACY_PROMPT_VERSION};
use crate::testing::{config_for, GoldenAuthCore, ManualClock, Scripted};

/// The part of the world both rigs share: Auth Core's stand-in, the provider's script, the usage
/// ledger and an extractor built on them.
pub(crate) struct Parts {
    pub auth: Arc<GoldenAuthCore>,
    pub inference: Arc<Scripted>,
    pub ledger: Arc<MemoryLedger>,
    pub extractor: Arc<GraphExtractor>,
    pub settings: Settings,
}

impl Parts {
    pub(crate) async fn new() -> Self {
        Self::build(
            Arc::new(GoldenAuthCore::start().await),
            Scripted::new(),
            Arc::new(MemoryLedger::new()),
        )
    }

    fn build(
        auth: Arc<GoldenAuthCore>,
        inference: Arc<Scripted>,
        ledger: Arc<MemoryLedger>,
    ) -> Self {
        let config = config_for(&auth);
        let extractor = GraphExtractor::with_canned_transport(
            &config,
            ManualClock::at_golden_mint(),
            inference.clone(),
        )
        .expect("an extractor over the stand-ins")
        .with_ledger(Some(ledger.clone() as Arc<dyn LedgerSink>));
        let settings = Settings {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            lease: Duration::from_secs(180),
            breaker: BREAKER,
            mark_batch: MARK_BATCH,
            // Alert-only, no figure: the owner's default. One dollar per million prompt tokens
            // and nothing for completions, so a test sets a month's spend in whole tokens.
            budget: GlobalBudget {
                mode: BudgetMode::AlertOnly,
                monthly_usd: None,
                price: Some(PriceTable {
                    input_usd_per_mtok: 1.0,
                    output_usd_per_mtok: 0.0,
                }),
            },
        };
        Self {
            auth,
            inference,
            ledger,
            extractor: Arc::new(extractor),
            settings,
        }
    }

    /// A new process: a fresh extractor (a fresh bearer cache) over the same Auth Core, the same
    /// provider and the same ledger.
    pub(crate) fn restarted(&self) -> Self {
        let mut next = Self::build(
            self.auth.clone(),
            self.inference.clone(),
            self.ledger.clone(),
        );
        next.settings = self.settings.clone();
        next
    }
}

/// What a chunk's row says, as the tests read it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub status: String,
    pub attempts: i32,
    pub error_class: Option<String>,
    pub prompt_version: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub entities: Option<i32>,
    pub relationships: Option<i32>,
    pub claims: Option<i32>,
    /// Whole seconds until the chunk is due again; `None` when nothing is scheduled.
    pub retry_in_secs: Option<i64>,
    pub leased: bool,
}

/// A chunk's row as the database returns it: status, attempts, error class, prompt version,
/// model, provider, the three counts, seconds until due and whether it is leased.
type DatabaseRow = (
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<f64>,
    bool,
);

/// What a test may do to the world, and see of it, besides running the pipeline.
pub(crate) trait Control: Ports + Sized {
    fn parts(&self) -> &Parts;
    /// A new process over the same data.
    async fn restart(&self) -> Self;
    /// An organization id no other test uses.
    fn org(&self, tag: &str) -> String {
        format!(
            "org-gst-{tag}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        )
    }
    /// A document with `chunks`, returning the chunks' ids.
    async fn add_document(
        &self,
        org: &str,
        name: &str,
        visibility: &str,
        classification: &str,
        chunks: &[&str],
    ) -> Vec<String>;
    /// Make a chunk's backoff over and release any lease on it.
    async fn make_due(&self, org: &str, knowledge_id: &str);
    async fn row(&self, org: &str, knowledge_id: &str) -> Option<Row>;
    async fn set_policy(
        &self,
        org: &str,
        graph_extraction: &str,
        budget_mode: &str,
        figure: Option<f64>,
    );
    /// The organization has spent this many dollars on background LLM calls this month.
    async fn set_spend_usd(&self, org: &str, usd: f64);
    async fn set_prompt_version(&self, org: &str, knowledge_id: &str, version: &str);
    async fn set_visibility(&self, org: &str, document_id: &str, visibility: &str);
    /// A chunk the way the backfill records one that was extracted before this table existed.
    async fn set_legacy_ready(&self, org: &str, knowledge_id: &str);
    fn letters(&self) -> Vec<DeadLetter>;
}

// ── in memory ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct MemRow {
    status: Status,
    attempts: i32,
    error_class: Option<ErrorClass>,
    next_at: Option<i64>,
    lease_until: Option<i64>,
    prompt_version: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    counts: Option<(i32, i32, i32)>,
}

struct MemDoc {
    visibility: String,
    classification: String,
    chunks: Vec<(String, String)>,
}

#[derive(Default)]
struct MemState {
    /// Seconds. Moves only when a test moves it.
    now: i64,
    rows: HashMap<(String, String), MemRow>,
    docs: BTreeMap<(String, String), MemDoc>,
    policies: HashMap<String, OrgPolicy>,
    spend: HashMap<String, f64>,
    /// Communities without a summary, by organization.
    communities: HashMap<String, Vec<(String, Vec<String>)>>,
    summaries: Vec<(String, String)>,
    /// The documents stop being visible to the organization after the claim and before the
    /// write: the race `persist` reports as `visible: false`.
    hide_while_extracting: bool,
}

/// The state's rules, restated: this is `due_predicate` in Rust.
fn memory_is_due(row: &MemRow, now: i64) -> bool {
    if row.lease_until.is_some_and(|until| until > now) {
        return false;
    }
    match row.status {
        Status::Pending
        | Status::FailedTransient
        | Status::SkippedZdr
        | Status::SkippedPolicy
        | Status::SkippedVisibility
        | Status::SkippedBudget => row.next_at.is_none_or(|at| at <= now),
        Status::Empty => row.prompt_version.as_deref() != Some(PROMPT_VERSION),
        Status::Ready | Status::FailedPermanent => false,
    }
}

pub(crate) struct Memory {
    state: Arc<Mutex<MemState>>,
    parts: Parts,
    letters: Arc<Mutex<Vec<DeadLetter>>>,
}

impl Memory {
    pub(crate) async fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
            parts: Parts::new().await,
            letters: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, MemState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn advance(&self, seconds: i64) {
        self.lock().now += seconds;
    }

    pub(crate) fn hide_documents_while_extracting(&self) {
        self.lock().hide_while_extracting = true;
    }

    pub(crate) fn add_community(&self, org: &str, community_id: &str, labels: &[&str]) {
        self.lock()
            .communities
            .entry(org.to_owned())
            .or_default()
            .push((
                community_id.to_owned(),
                labels.iter().map(|label| (*label).to_owned()).collect(),
            ));
    }

    pub(crate) fn summaries(&self) -> Vec<String> {
        self.lock()
            .summaries
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Mark the chunks of the organization's documents that `select` picks, as the SQL does:
    /// missing rows are inserted, and a row is rewritten only while it is still waiting,
    /// unleased and saying something else.
    fn mark(
        &self,
        org_id: &str,
        select: impl Fn(&str, &MemDoc) -> bool,
        settlement: &Settlement,
    ) -> u64 {
        let mut state = self.lock();
        let now = state.now;
        let targets: Vec<String> = state
            .docs
            .iter()
            .filter(|((org, id), doc)| org == org_id && select(id, doc))
            .flat_map(|(_, doc)| doc.chunks.iter().map(|(kid, _)| kid.clone()))
            .collect();
        let mut changed = 0;
        for knowledge_id in targets {
            let next_at = settlement.retry_in.map(|wait| now + wait.as_secs() as i64);
            match state
                .rows
                .get_mut(&(org_id.to_owned(), knowledge_id.clone()))
            {
                None => {
                    state.rows.insert(
                        (org_id.to_owned(), knowledge_id),
                        MemRow {
                            status: settlement.status,
                            attempts: 0,
                            error_class: settlement.error_class,
                            next_at,
                            lease_until: None,
                            prompt_version: None,
                            model: None,
                            provider: None,
                            counts: None,
                        },
                    );
                    changed += 1;
                }
                Some(row) => {
                    let waiting = crate::state::WORK_STATUSES.contains(&row.status);
                    let unleased = row.lease_until.is_none_or(|until| until <= now);
                    let differs = row.status != settlement.status
                        || row.error_class != settlement.error_class;
                    if waiting && unleased && differs {
                        row.status = settlement.status;
                        row.error_class = settlement.error_class;
                        row.next_at = next_at;
                        changed += 1;
                    }
                }
            }
        }
        changed
    }
}

impl Ports for Memory {
    fn settings(&self) -> &Settings {
        &self.parts.settings
    }

    async fn claim_chunk(&self, org_id: &str, knowledge_id: &str) -> anyhow::Result<Option<Claim>> {
        let mut state = self.lock();
        let now = state.now;
        let visible = state.docs.iter().any(|((org, _), doc)| {
            org == org_id
                && doc.visibility == "org"
                && egress_allowed(Some(&doc.classification))
                && doc.chunks.iter().any(|(kid, _)| kid == knowledge_id)
        });
        if !visible {
            return Ok(None);
        }
        let lease_until = now + self.parts.settings.lease.as_secs() as i64;
        let key = (org_id.to_owned(), knowledge_id.to_owned());
        match state.rows.get_mut(&key) {
            None => {
                state.rows.insert(
                    key,
                    MemRow {
                        status: Status::Pending,
                        attempts: 1,
                        error_class: None,
                        next_at: None,
                        lease_until: Some(lease_until),
                        prompt_version: None,
                        model: None,
                        provider: None,
                        counts: None,
                    },
                );
                Ok(Some(Claim { attempts: 1 }))
            }
            Some(row) if memory_is_due(row, now) => {
                row.status = Status::Pending;
                row.attempts += 1;
                row.lease_until = Some(lease_until);
                Ok(Some(Claim {
                    attempts: row.attempts,
                }))
            }
            Some(_) => Ok(None),
        }
    }

    async fn settle_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<bool> {
        let mut state = self.lock();
        let now = state.now;
        let Some(row) = state
            .rows
            .get_mut(&(org_id.to_owned(), knowledge_id.to_owned()))
        else {
            return Ok(false);
        };
        row.status = settlement.status;
        row.error_class = settlement.error_class;
        row.next_at = settlement.retry_in.map(|wait| now + wait.as_secs() as i64);
        row.lease_until = None;
        if settlement.refund_attempt {
            row.attempts = (row.attempts - 1).max(0);
        }
        if let Some(stamp) = &settlement.stamp {
            row.prompt_version = Some(stamp.prompt_version.clone());
            row.model = Some(stamp.model.clone());
            row.provider = Some(stamp.provider.clone());
            row.counts = Some((stamp.entities, stamp.relationships, stamp.claims));
        }
        Ok(true)
    }

    async fn mark_document(
        &self,
        org_id: &str,
        document_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<u64> {
        Ok(self.mark(org_id, |id, _| id == document_id, settlement))
    }

    async fn mark_org(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        Ok(self.mark(
            org_id,
            |_, doc| doc.visibility == "org" && egress_allowed(Some(&doc.classification)),
            settlement,
        ))
    }

    async fn mark_restricted(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        Ok(self.mark(
            org_id,
            |_, doc| doc.visibility == "org" && !egress_allowed(Some(&doc.classification)),
            settlement,
        ))
    }

    async fn document_standing(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Option<Standing>> {
        Ok(self
            .lock()
            .docs
            .get(&(org_id.to_owned(), document_id.to_owned()))
            .map(|doc| Standing {
                visible: doc.visibility == "org",
                classification: doc.classification.clone(),
            }))
    }

    async fn load_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        Ok(self
            .lock()
            .docs
            .get(&(org_id.to_owned(), document_id.to_owned()))
            .filter(|doc| doc.visibility == "org")
            .map(|doc| doc.chunks.clone())
            .unwrap_or_default())
    }

    async fn due_documents(&self, org_id: &str, limit: i64) -> anyhow::Result<Vec<String>> {
        let state = self.lock();
        Ok(state
            .docs
            .iter()
            .filter(|((org, _), doc)| {
                org == org_id
                    && doc.visibility == "org"
                    && egress_allowed(Some(&doc.classification))
                    && doc.chunks.iter().any(|(kid, _)| {
                        state
                            .rows
                            .get(&(org_id.to_owned(), kid.clone()))
                            .is_none_or(|row| memory_is_due(row, state.now))
                    })
            })
            .map(|((_, id), _)| id.clone())
            .take(usize::try_from(limit).unwrap_or(0))
            .collect())
    }

    async fn org_policy(&self, org_id: &str) -> anyhow::Result<OrgPolicy> {
        Ok(self
            .lock()
            .policies
            .get(org_id)
            .copied()
            .unwrap_or(OrgPolicy::DEFAULT))
    }

    async fn month_spend_usd(&self, org_id: &str) -> anyhow::Result<Option<f64>> {
        if self.parts.settings.budget.price.is_none() {
            return Ok(None);
        }
        Ok(Some(self.lock().spend.get(org_id).copied().unwrap_or(0.0)))
    }

    async fn posture(&self, org_id: &str, reason: Reason) -> Result<Posture, InferenceError> {
        self.parts.extractor.posture(org_id, reason).await
    }

    async fn extract(&self, org_id: &str, text: &str) -> anyhow::Result<Extracted> {
        self.parts.extractor.extract(text, org_id).await
    }

    async fn persist(
        &self,
        org_id: &str,
        knowledge_id: &str,
        extraction: &ExtractionResult,
    ) -> anyhow::Result<Persisted> {
        let state = self.lock();
        let visible = !state.hide_while_extracting
            && state.docs.iter().any(|((org, _), doc)| {
                org == org_id
                    && doc.visibility == "org"
                    && doc.chunks.iter().any(|(kid, _)| kid == knowledge_id)
            });
        if !visible {
            return Ok(Persisted::default());
        }
        Ok(Persisted {
            visible: true,
            entities: extraction.entities.len(),
            relationships: extraction.relationships.len(),
            claims: extraction.claims.len(),
            claim_ids: extraction
                .claims
                .iter()
                .map(|claim| claim.claim_text.clone())
                .collect(),
        })
    }

    async fn dead_letter(&self, letter: &DeadLetter) {
        self.letters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(letter.clone());
    }

    async fn communities_needing_summary(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, Vec<String>)>> {
        Ok(self
            .lock()
            .communities
            .get(org_id)
            .map(|pending| {
                pending
                    .iter()
                    .take(usize::try_from(limit).unwrap_or(0))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn summarize(&self, org_id: &str, labels: &[String]) -> anyhow::Result<String> {
        self.parts
            .extractor
            .summarize_community(labels, org_id)
            .await
    }

    async fn store_summary(
        &self,
        org_id: &str,
        community_id: &str,
        summary: &str,
    ) -> anyhow::Result<bool> {
        let mut state = self.lock();
        let pending = state.communities.entry(org_id.to_owned()).or_default();
        let Some(at) = pending.iter().position(|(id, _)| id == community_id) else {
            return Ok(false);
        };
        pending.remove(at);
        state
            .summaries
            .push((community_id.to_owned(), summary.to_owned()));
        Ok(true)
    }
}

impl Control for Memory {
    fn parts(&self) -> &Parts {
        &self.parts
    }

    async fn restart(&self) -> Self {
        Self {
            state: self.state.clone(),
            parts: self.parts.restarted(),
            letters: self.letters.clone(),
        }
    }

    async fn add_document(
        &self,
        org: &str,
        name: &str,
        visibility: &str,
        classification: &str,
        chunks: &[&str],
    ) -> Vec<String> {
        let document_id = format!("{org}-{name}");
        let chunks: Vec<(String, String)> = chunks
            .iter()
            .enumerate()
            .map(|(index, text)| (format!("{document_id}-k{index}"), (*text).to_owned()))
            .collect();
        let ids = chunks.iter().map(|(kid, _)| kid.clone()).collect();
        self.lock().docs.insert(
            (org.to_owned(), document_id),
            MemDoc {
                visibility: visibility.to_owned(),
                classification: classification.to_owned(),
                chunks,
            },
        );
        ids
    }

    async fn make_due(&self, org: &str, knowledge_id: &str) {
        if let Some(row) = self
            .lock()
            .rows
            .get_mut(&(org.to_owned(), knowledge_id.to_owned()))
        {
            row.next_at = None;
            row.lease_until = None;
        }
    }

    async fn row(&self, org: &str, knowledge_id: &str) -> Option<Row> {
        let state = self.lock();
        state
            .rows
            .get(&(org.to_owned(), knowledge_id.to_owned()))
            .map(|row| Row {
                status: row.status.as_str().to_owned(),
                attempts: row.attempts,
                error_class: row.error_class.map(|class| class.as_str().to_owned()),
                prompt_version: row.prompt_version.clone(),
                model: row.model.clone(),
                provider: row.provider.clone(),
                entities: row.counts.map(|counts| counts.0),
                relationships: row.counts.map(|counts| counts.1),
                claims: row.counts.map(|counts| counts.2),
                retry_in_secs: row.next_at.map(|at| at - state.now),
                leased: row.lease_until.is_some_and(|until| until > state.now),
            })
    }

    async fn set_policy(
        &self,
        org: &str,
        graph_extraction: &str,
        budget_mode: &str,
        figure: Option<f64>,
    ) {
        self.lock().policies.insert(
            org.to_owned(),
            OrgPolicy {
                graph_extraction: Switch::parse(graph_extraction).expect("a switch"),
                budget_mode: match budget_mode {
                    "inherit" => None,
                    other => Some(BudgetMode::parse(other).expect("a budget mode")),
                },
                monthly_budget_usd: figure,
            },
        );
    }

    async fn set_spend_usd(&self, org: &str, usd: f64) {
        self.lock().spend.insert(org.to_owned(), usd);
    }

    async fn set_prompt_version(&self, org: &str, knowledge_id: &str, version: &str) {
        if let Some(row) = self
            .lock()
            .rows
            .get_mut(&(org.to_owned(), knowledge_id.to_owned()))
        {
            row.prompt_version = Some(version.to_owned());
        }
    }

    async fn set_visibility(&self, org: &str, document_id: &str, visibility: &str) {
        if let Some(doc) = self
            .lock()
            .docs
            .get_mut(&(org.to_owned(), document_id.to_owned()))
        {
            doc.visibility = visibility.to_owned();
        }
    }

    async fn set_legacy_ready(&self, org: &str, knowledge_id: &str) {
        self.lock().rows.insert(
            (org.to_owned(), knowledge_id.to_owned()),
            MemRow {
                status: Status::Ready,
                attempts: 0,
                error_class: None,
                next_at: None,
                lease_until: None,
                prompt_version: Some(LEGACY_PROMPT_VERSION.to_owned()),
                model: None,
                provider: None,
                counts: Some((1, 0, 0)),
            },
        );
    }

    fn letters(&self) -> Vec<DeadLetter> {
        self.letters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

// ── on the migrated schema ───────────────────────────────────────────────────

/// The production ports over a real, migrated database: nothing in the data path is a stand-in.
pub(crate) struct Database {
    live: Live,
    pool: PgPool,
    parts: Parts,
    letters: Arc<Mutex<Vec<DeadLetter>>>,
}

impl Database {
    /// FAILS, never skips, when the database is not there: the runner sets
    /// `MIGRATED_TEST_DATABASE_URL`, and a contract that cannot run proves nothing.
    pub(crate) async fn new() -> Self {
        let url = std::env::var("MIGRATED_TEST_DATABASE_URL")
            .expect("MIGRATED_TEST_DATABASE_URL must point to a disposable PostgreSQL with the migrated Data Plane schema");
        assert!(
            (url.contains("localhost") || url.contains("127.0.0.1"))
                && url
                    .split('?')
                    .next()
                    .is_some_and(|base| base.ends_with("_test")),
            "refusing a non-local database, or one whose name does not end in _test"
        );
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("connect to the migrated test database");
        Self::over(pool, Parts::new().await, Arc::new(Mutex::new(Vec::new())))
    }

    fn over(pool: PgPool, parts: Parts, letters: Arc<Mutex<Vec<DeadLetter>>>) -> Self {
        let live = Live {
            store: Arc::new(GraphStore::new(pool.clone())),
            extractor: parts.extractor.clone(),
            neo4j: None,
            nats: None,
            settings: parts.settings.clone(),
        };
        Self {
            live,
            pool,
            parts,
            letters,
        }
    }

    async fn exec(
        &self,
        sql: &str,
        bind: impl FnOnce(
            sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        )
            -> sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) {
        bind(sqlx::query(sql))
            .execute(&self.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}\nSQL: {sql}"));
    }

    /// The store's own method, for a test that needs the real SQL itself.
    pub(crate) fn store(&self) -> &GraphStore {
        &self.live.store
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl Ports for Database {
    fn settings(&self) -> &Settings {
        self.live.settings()
    }
    async fn claim_chunk(&self, org_id: &str, knowledge_id: &str) -> anyhow::Result<Option<Claim>> {
        self.live.claim_chunk(org_id, knowledge_id).await
    }
    async fn settle_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<bool> {
        self.live
            .settle_chunk(org_id, knowledge_id, settlement)
            .await
    }
    async fn mark_document(
        &self,
        org_id: &str,
        document_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<u64> {
        self.live
            .mark_document(org_id, document_id, settlement)
            .await
    }
    async fn mark_org(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        self.live.mark_org(org_id, settlement).await
    }
    async fn mark_restricted(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        self.live.mark_restricted(org_id, settlement).await
    }
    async fn document_standing(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Option<Standing>> {
        self.live.document_standing(org_id, document_id).await
    }
    async fn load_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        self.live.load_chunks(org_id, document_id).await
    }
    async fn due_documents(&self, org_id: &str, limit: i64) -> anyhow::Result<Vec<String>> {
        self.live.due_documents(org_id, limit).await
    }
    async fn org_policy(&self, org_id: &str) -> anyhow::Result<OrgPolicy> {
        self.live.org_policy(org_id).await
    }
    async fn month_spend_usd(&self, org_id: &str) -> anyhow::Result<Option<f64>> {
        self.live.month_spend_usd(org_id).await
    }
    async fn posture(&self, org_id: &str, reason: Reason) -> Result<Posture, InferenceError> {
        self.live.posture(org_id, reason).await
    }
    async fn extract(&self, org_id: &str, text: &str) -> anyhow::Result<Extracted> {
        self.live.extract(org_id, text).await
    }
    async fn persist(
        &self,
        org_id: &str,
        knowledge_id: &str,
        extraction: &ExtractionResult,
    ) -> anyhow::Result<Persisted> {
        self.live.persist(org_id, knowledge_id, extraction).await
    }
    async fn dead_letter(&self, letter: &DeadLetter) {
        self.letters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(letter.clone());
    }
    async fn communities_needing_summary(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, Vec<String>)>> {
        self.live.communities_needing_summary(org_id, limit).await
    }
    async fn summarize(&self, org_id: &str, labels: &[String]) -> anyhow::Result<String> {
        self.live.summarize(org_id, labels).await
    }
    async fn store_summary(
        &self,
        org_id: &str,
        community_id: &str,
        summary: &str,
    ) -> anyhow::Result<bool> {
        self.live.store_summary(org_id, community_id, summary).await
    }
}

impl Control for Database {
    fn parts(&self) -> &Parts {
        &self.parts
    }

    async fn restart(&self) -> Self {
        Self::over(
            self.pool.clone(),
            self.parts.restarted(),
            self.letters.clone(),
        )
    }

    async fn add_document(
        &self,
        org: &str,
        name: &str,
        visibility: &str,
        classification: &str,
        chunks: &[&str],
    ) -> Vec<String> {
        let document_id = format!("{org}-{name}");
        self.exec(
            "INSERT INTO documents (document_id, org_id, source, type, title, content, zdr_classification, visibility) \
             VALUES ($1, $2, 'contract-test', 'note', 'title', 'content', $3, $4)",
            |query| {
                query
                    .bind(document_id.clone())
                    .bind(org.to_owned())
                    .bind(classification.to_owned())
                    .bind(visibility.to_owned())
            },
        )
        .await;
        let mut ids = Vec::new();
        for (index, text) in chunks.iter().enumerate() {
            let knowledge_id = format!("{document_id}-k{index}");
            self.exec(
                "INSERT INTO knowledge_units (knowledge_id, document_id, org_id, chunk_index, text) \
                 VALUES ($1, $2, $3, $4, $5)",
                |query| {
                    query
                        .bind(knowledge_id.clone())
                        .bind(document_id.clone())
                        .bind(org.to_owned())
                        .bind(i32::try_from(index).expect("a small index"))
                        .bind((*text).to_owned())
                },
            )
            .await;
            ids.push(knowledge_id);
        }
        ids
    }

    async fn make_due(&self, org: &str, knowledge_id: &str) {
        self.exec(
            "UPDATE graph_extraction_state SET next_attempt_at = NULL, lease_until = NULL \
              WHERE org_id = $1 AND knowledge_id = $2",
            |query| query.bind(org.to_owned()).bind(knowledge_id.to_owned()),
        )
        .await;
    }

    async fn row(&self, org: &str, knowledge_id: &str) -> Option<Row> {
        let row: Option<DatabaseRow> = sqlx::query_as(
            "SELECT status, attempts, error_class, prompt_version, model, provider, \
                    entity_count, relationship_count, claim_count, \
                    EXTRACT(EPOCH FROM (next_attempt_at - now()))::float8, \
                    COALESCE(lease_until > now(), false) \
               FROM graph_extraction_state WHERE org_id = $1 AND knowledge_id = $2",
        )
        .bind(org)
        .bind(knowledge_id)
        .fetch_optional(&self.pool)
        .await
        .expect("read a chunk's state");
        row.map(
            |(
                status,
                attempts,
                error_class,
                prompt_version,
                model,
                provider,
                entities,
                relationships,
                claims,
                retry,
                leased,
            )| Row {
                status,
                attempts,
                error_class,
                prompt_version,
                model,
                provider,
                entities,
                relationships,
                claims,
                // The database's clock is the authority and a statement takes a moment: whole
                // seconds, rounded.
                retry_in_secs: retry.map(|seconds| seconds.round() as i64),
                leased,
            },
        )
    }

    async fn set_policy(
        &self,
        org: &str,
        graph_extraction: &str,
        budget_mode: &str,
        figure: Option<f64>,
    ) {
        self.exec(
            "INSERT INTO org_processing_policy (org_id, graph_extraction, budget_mode, monthly_budget_usd, reason, updated_by) \
             VALUES ($1, $2, $3, $4::float8, 'contract test', 'contract-test') \
             ON CONFLICT (org_id) DO UPDATE SET graph_extraction = EXCLUDED.graph_extraction, \
                budget_mode = EXCLUDED.budget_mode, monthly_budget_usd = EXCLUDED.monthly_budget_usd",
            |query| {
                query
                    .bind(org.to_owned())
                    .bind(graph_extraction.to_owned())
                    .bind(budget_mode.to_owned())
                    .bind(figure)
            },
        )
        .await;
    }

    async fn set_spend_usd(&self, org: &str, usd: f64) {
        // The harness prices a million prompt tokens at one dollar and completions at nothing.
        let tokens = (usd * 1_000_000.0).round() as i64;
        self.exec(
            "INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, \
                 schema_version, feature, provider_used, input_tokens, output_tokens, cache_read_tokens, \
                 cost_basis, estimated, zdr, request_id) \
             VALUES ('llm', 'gpt-4o-mini', $1, 1, 0, $2, 2, 'graph-extraction', 'azure-openai', $3, 0, 0, \
                 'metered', false, false, $2)",
            |query| {
                query
                    .bind(org.to_owned())
                    .bind(format!("spend-{}", uuid::Uuid::new_v4()))
                    .bind(tokens)
            },
        )
        .await;
    }

    async fn set_prompt_version(&self, org: &str, knowledge_id: &str, version: &str) {
        self.exec(
            "UPDATE graph_extraction_state SET prompt_version = $3 WHERE org_id = $1 AND knowledge_id = $2",
            |query| {
                query
                    .bind(org.to_owned())
                    .bind(knowledge_id.to_owned())
                    .bind(version.to_owned())
            },
        )
        .await;
    }

    async fn set_visibility(&self, org: &str, document_id: &str, visibility: &str) {
        self.exec(
            "UPDATE documents SET visibility = $3 WHERE org_id = $1 AND document_id = $2",
            |query| {
                query
                    .bind(org.to_owned())
                    .bind(document_id.to_owned())
                    .bind(visibility.to_owned())
            },
        )
        .await;
    }

    async fn set_legacy_ready(&self, org: &str, knowledge_id: &str) {
        self.exec(
            "INSERT INTO graph_extraction_state \
                 (org_id, knowledge_id, status, attempts, prompt_version, extracted_at, entity_count, relationship_count, claim_count) \
             VALUES ($1, $2, 'ready', 0, 'legacy', now(), 1, 0, 0) \
             ON CONFLICT (org_id, knowledge_id) DO NOTHING",
            |query| query.bind(org.to_owned()).bind(knowledge_id.to_owned()),
        )
        .await;
    }

    fn letters(&self) -> Vec<DeadLetter> {
        self.letters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
