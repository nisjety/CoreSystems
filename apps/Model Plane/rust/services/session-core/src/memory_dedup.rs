//! `session-core memory-dedup`: a reviewed, reversible cleanup of the
//! near-duplicate `agent_memory` rows that predate write-time grounding.
//!
//! Grounding ([`crate::memory_grounding`]) only ever looks at NEW candidates,
//! so restatements already stored -- one standing preference minted under
//! three slot names by three threads -- stay current and are injected into
//! every prompt. This command retires them the same non-destructive way
//! grounding would have: `is_latest = false` on the redundant rows and a
//! `supersedes_id` chain from the surviving row. No row, key or content is
//! ever created, rewritten or deleted.
//!
//! ```text
//! session-core memory-dedup [plan] [--classifier on|off] [--out <file>] [--show-content]
//! session-core memory-dedup apply --plan <file> [--dry-run]
//! session-core memory-dedup rollback --plan-id <id> [--cluster <id>]
//! ```
//!
//! It runs inside the service's own container (`docker exec ... session-core
//! memory-dedup ...`), so it reuses the service's database pool, partition
//! rules and grounding classifier. `main` dispatches here before anything else
//! starts: no gRPC, HTTP, NATS, background worker or migration run. Set
//! `RUST_LOG=warn` for the run (`docker exec -e RUST_LOG=warn ...`) to keep the
//! service's own log level out of the report.
//!
//! # plan
//!
//! Reads, in one `REPEATABLE READ, READ ONLY` transaction, every current row
//! (`is_latest`, `accepted`, unexpired, not forgotten) and compares rows only
//! within their unique-index partition: `(org_id, session_id, scope)` for
//! thread-bound rows, `(org_id, scope, owner)` for the rest. A token-overlap
//! pre-filter keeps at most [`TOP_K`] neighbours per row. With the classifier
//! on (the default) each such pair is then classified, and a cluster is placed
//! in the `auto` tier only when EVERY row it would retire is a NOOP
//! restatement of the survivor. EXTEND/SUPERSEDE verdicts, and every cluster
//! of a `--classifier off` run, go to `review`: this command never writes
//! merged content, so retiring a row that says more than its survivor would
//! lose information.
//!
//! Survivor: a row the user stated (`tool:save_memory`) beats a row of unknown
//! provenance, which beats an inferred one; then the newest `updated_at`, then
//! the higher confidence, then the fuller content.
//!
//! The plan file is for a person to review and edit. `apply` changes only what
//! the file says, never re-clusters, and only clusters in the `auto` tier. A
//! reviewer approves a `review` cluster by setting its tier to `auto`, drops a
//! cluster by deleting it, and changes a survivor by swapping it with an entry
//! of `retire`. The file holds ids, keys, content hashes and scores, never
//! content (`--show-content` puts a short preview in the printed report only).
//!
//! # apply
//!
//! One transaction per cluster: lock the rows, skip the cluster as stale when
//! any row is missing, retired, forgotten, or its content hash, `updated_at`,
//! key, partition or lineage differs from the plan, then flip `is_latest` and
//! chain `supersedes_id` (survivor -> newest retired -> ... -> oldest) only
//! where it is NULL, verify that exactly one row is current and no cycle
//! exists, and write a `dream_runs` ledger row (trigger
//! `memory_dedup_backfill`, `thread_id` NULL, ids and booleans only).
//! `updated_at` is never touched, so the memory panel keeps its order. A
//! cluster whose ledger says it is applied and whose rows are still as apply
//! left them is skipped, so a re-run is safe.
//!
//! Semantic mirrors are left alone on purpose: every semantic read filters
//! out retired ids ([`crate::memory_control::filter_forgotten`]), and keeping
//! the mirrors is what lets a rollback restore recall exactly.
//!
//! # rollback
//!
//! Driven by the ledger, never by the plan file. Each recorded change is
//! reverted only while the row is still exactly as apply left it and has not
//! been forgotten; rows changed or deleted since are reported and left alone,
//! and deleted content is never recreated. Rollback writes its own ledger row.
//! Only two columns ever change, so the ledger also supports a plain SQL
//! rollback.

use crate::dreaming::MemoryProvenance;
use crate::memory_grounding::{ExistingMatch, GroundingAction, GroundingClassifier};
use anyhow::{bail, Context as _};
use chrono::{DateTime, Utc};
use mp_ids::new_ulid;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub(crate) const SUBCOMMAND: &str = "memory-dedup";
const PLAN_FORMAT: &str = "session-core/memory-dedup-plan/v1";
const LEDGER_TRIGGER: &str = "memory_dedup_backfill";
/// `migrations/0037_agent_memory_lineage.sql`: `is_latest` and `supersedes_id`.
const LINEAGE_MIGRATION: i64 = 37;
/// Neighbours kept per row by the lexical pre-filter.
const TOP_K: usize = 5;
/// A pair is a candidate when either score reaches its floor. These are the
/// thresholds the duplicate investigation validated by reading every cluster
/// they produced on the live data.
const JACCARD_FLOOR: f64 = 0.4;
const OVERLAP_FLOOR: f64 = 0.75;
const PREVIEW_CHARS: usize = 100;

const USAGE: &str = "\
usage: session-core memory-dedup [plan] [--classifier on|off] [--out <file>] [--show-content]
       session-core memory-dedup apply --plan <file> [--dry-run]
       session-core memory-dedup rollback --plan-id <id> [--cluster <id>]";

const PLAN_NOTE: &str = "apply changes only clusters whose tier is \"auto\". Delete a cluster to \
skip it, set a \"review\" cluster's tier to \"auto\" to approve it, and swap the survivor with an \
entry of \"retire\" to keep a different row. Rows are matched by id, content_sha256 and \
updated_at; a cluster whose rows changed since this plan is skipped as stale.";

/// Whether the process was started as `session-core memory-dedup ...`.
///
/// Compared without decoding the whole argument list, so a plain start --
/// the container's argument-less `session-core` -- takes exactly the path it
/// always did.
pub(crate) fn is_invocation(first_arg: Option<&OsStr>) -> bool {
    first_arg.and_then(OsStr::to_str) == Some(SUBCOMMAND)
}

/// Entry point for `session-core memory-dedup <args>`; `args` excludes the
/// subcommand itself.
pub(crate) async fn run(args: &[String]) -> anyhow::Result<()> {
    let command = parse_args(args).map_err(|problem| anyhow::anyhow!("{problem}\n{USAGE}"))?;
    if command == Command::Help {
        println!("{USAGE}");
        return Ok(());
    }
    let pool = crate::store::connect_postgres().await?;
    let result = match command {
        Command::Plan {
            classifier,
            out,
            show_content,
        } => run_plan(&pool, classifier, out, show_content).await,
        Command::Apply { plan, dry_run } => run_apply(&pool, &plan, dry_run).await,
        Command::Rollback { plan_id, cluster } => {
            run_rollback(&pool, &plan_id, cluster.as_deref()).await
        }
        Command::Help => Ok(()),
    };
    pool.close().await;
    result
}

// ---- command line ----

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Plan {
        classifier: bool,
        out: Option<PathBuf>,
        show_content: bool,
    },
    Apply {
        plan: PathBuf,
        dry_run: bool,
    },
    Rollback {
        plan_id: String,
        cluster: Option<String>,
    },
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let (mode, flags) = match args.first().map(String::as_str) {
        None => ("plan", args),
        Some(first) if first.starts_with('-') => ("plan", args),
        Some(first) => (first, &args[1..]),
    };
    if matches!(mode, "help" | "-h" | "--help")
        || flags.iter().any(|flag| flag == "--help" || flag == "-h")
    {
        return Ok(Command::Help);
    }
    let mut flags = flags.iter();
    match mode {
        "plan" => {
            let (mut classifier, mut out, mut show_content) = (true, None, false);
            while let Some(flag) = flags.next() {
                match flag.as_str() {
                    "--classifier" => {
                        classifier = match flags.next().map(String::as_str) {
                            Some("on") => true,
                            Some("off") => false,
                            _ => return Err("--classifier takes on or off".to_owned()),
                        };
                    }
                    "--out" => out = Some(PathBuf::from(required_value(flag, flags.next())?)),
                    "--show-content" => show_content = true,
                    other => return Err(format!("unknown plan option {other}")),
                }
            }
            Ok(Command::Plan {
                classifier,
                out,
                show_content,
            })
        }
        "apply" => {
            let (mut plan, mut dry_run) = (None, false);
            while let Some(flag) = flags.next() {
                match flag.as_str() {
                    "--plan" => plan = Some(PathBuf::from(required_value(flag, flags.next())?)),
                    "--dry-run" => dry_run = true,
                    other => return Err(format!("unknown apply option {other}")),
                }
            }
            let plan = plan.ok_or_else(|| "apply needs --plan <file>".to_owned())?;
            Ok(Command::Apply { plan, dry_run })
        }
        "rollback" => {
            let (mut plan_id, mut cluster) = (None, None);
            while let Some(flag) = flags.next() {
                match flag.as_str() {
                    "--plan-id" => plan_id = Some(required_value(flag, flags.next())?.to_owned()),
                    "--cluster" => cluster = Some(required_value(flag, flags.next())?.to_owned()),
                    other => return Err(format!("unknown rollback option {other}")),
                }
            }
            let plan_id = plan_id.ok_or_else(|| "rollback needs --plan-id <id>".to_owned())?;
            Ok(Command::Rollback { plan_id, cluster })
        }
        other => Err(format!("unknown memory-dedup mode {other}")),
    }
}

fn required_value<'a>(flag: &str, value: Option<&'a String>) -> Result<&'a str, String> {
    match value {
        Some(value) if !value.starts_with("--") && !value.trim().is_empty() => Ok(value.trim()),
        _ => Err(format!("{flag} needs a value")),
    }
}

// ---- rows and partitions ----

#[derive(Debug, Clone, PartialEq)]
struct MemoryRow {
    id: String,
    org_id: String,
    session_id: Option<String>,
    scope: String,
    owner: String,
    key: String,
    content: String,
    confidence: f64,
    source_links: Vec<String>,
    updated_at: DateTime<Utc>,
    supersedes_id: Option<String>,
}

impl MemoryRow {
    fn partition(&self) -> Partition {
        Partition::new(
            &self.org_id,
            self.session_id.as_deref(),
            &self.scope,
            &self.owner,
        )
    }

    fn provenance(&self) -> MemoryProvenance {
        MemoryProvenance::classify(&self.source_links)
    }
}

/// The same partition the unique indexes enforce:
/// `agent_memory_org_session_scope_key_uq (org_id, session_id, scope, key)`
/// for thread-bound rows, `agent_memory_org_scope_key_uq (org_id, scope,
/// owner, key)` for the rest. Rows in different partitions are never
/// compared: they are injected into different prompts, and a thread's copy of
/// a user-level fact is not a duplicate this command may retire.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
struct Partition {
    org_id: String,
    scope: String,
    /// Set for thread-bound rows, whose index ignores `owner`.
    session_id: Option<String>,
    /// Set for rows without a session.
    owner: Option<String>,
}

impl Partition {
    fn new(org_id: &str, session_id: Option<&str>, scope: &str, owner: &str) -> Self {
        match session_id {
            Some(session_id) => Self {
                org_id: org_id.to_owned(),
                scope: scope.to_owned(),
                session_id: Some(session_id.to_owned()),
                owner: None,
            },
            None => Self {
                org_id: org_id.to_owned(),
                scope: scope.to_owned(),
                session_id: None,
                owner: Some(owner.to_owned()),
            },
        }
    }
}

// ---- lexical pre-filter ----

/// Words that carry no identity in an extractor-written sentence ("User
/// prefers ...", "The user wants ..."). Kept identical to the investigation's
/// scorer so the plan reproduces the clusters it found.
const STOPWORDS: &[&str] = &[
    "user", "users", "a", "an", "the", "and", "or", "of", "to", "in", "for", "on", "with", "from",
    "as", "is", "are", "be", "by", "their", "this", "that", "not", "no", "any", "before", "only",
    "does", "do", "want", "wants", "prefer", "prefers",
];

fn tokens(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .map(|word| word.trim_matches('.'))
        .filter(|word| !word.is_empty() && !STOPWORDS.contains(word))
        .map(stem)
        .collect()
}

/// A crude suffix strip, enough for "sends"/"send" and "orders"/"order" to
/// meet. Short words are left alone so "is"/"as" style words never collapse.
fn stem(word: &str) -> String {
    if word.chars().count() > 5 {
        for suffix in ["ing", "es", "s", "ed"] {
            if let Some(stem) = word.strip_suffix(suffix) {
                return stem.to_owned();
            }
        }
    }
    word.to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LexicalScore {
    jaccard: f64,
    /// Shared tokens over the smaller set: catches a restatement that is a
    /// subset of a fuller one, which Jaccard alone scores low.
    overlap: f64,
}

impl LexicalScore {
    fn between(a: &BTreeSet<String>, b: &BTreeSet<String>) -> Self {
        let shared = a.intersection(b).count();
        Self {
            jaccard: ratio(shared, a.union(b).count()),
            overlap: ratio(shared, a.len().min(b.len())),
        }
    }

    fn is_candidate(self) -> bool {
        self.jaccard >= JACCARD_FLOOR || self.overlap >= OVERLAP_FLOOR
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// A lexically similar pair within one partition, by index into its rows.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CandidatePair {
    a: usize,
    b: usize,
    score: LexicalScore,
}

/// Every row's best [`TOP_K`] candidate neighbours, as unordered pairs
/// (`a < b`), each pair once, in a deterministic order.
fn candidate_pairs(rows: &[MemoryRow]) -> Vec<CandidatePair> {
    let token_sets: Vec<BTreeSet<String>> = rows.iter().map(|row| tokens(&row.content)).collect();
    let mut kept: BTreeMap<(usize, usize), LexicalScore> = BTreeMap::new();
    for row in 0..rows.len() {
        let mut neighbours: Vec<(usize, LexicalScore)> = (0..rows.len())
            .filter(|&other| other != row)
            .map(|other| {
                (
                    other,
                    LexicalScore::between(&token_sets[row], &token_sets[other]),
                )
            })
            .filter(|(_, score)| score.is_candidate())
            .collect();
        neighbours.sort_by(|(left, a), (right, b)| {
            b.overlap
                .total_cmp(&a.overlap)
                .then(b.jaccard.total_cmp(&a.jaccard))
                .then_with(|| rows[*left].id.cmp(&rows[*right].id))
        });
        neighbours.truncate(TOP_K);
        for (other, score) in neighbours {
            kept.insert((row.min(other), row.max(other)), score);
        }
    }
    kept.into_iter()
        .map(|((a, b), score)| CandidatePair { a, b, score })
        .collect()
}

/// Connected components of size two or more, members ascending.
fn components(len: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    fn find(parent: &mut [usize], node: usize) -> usize {
        let mut root = node;
        while parent[root] != root {
            root = parent[root];
        }
        let mut current = node;
        while parent[current] != root {
            let next = parent[current];
            parent[current] = root;
            current = next;
        }
        root
    }

    let mut parent: Vec<usize> = (0..len).collect();
    for &(a, b) in edges {
        let (root_a, root_b) = (find(&mut parent, a), find(&mut parent, b));
        if root_a != root_b {
            parent[root_a.max(root_b)] = root_a.min(root_b);
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for node in 0..len {
        let root = find(&mut parent, node);
        groups.entry(root).or_default().push(node);
    }
    groups
        .into_values()
        .filter(|group| group.len() > 1)
        .collect()
}

// ---- survivor ----

fn provenance_rank(provenance: MemoryProvenance) -> u8 {
    match provenance {
        MemoryProvenance::Stated => 2,
        MemoryProvenance::Unknown => 1,
        MemoryProvenance::Inferred => 0,
    }
}

fn provenance_label(provenance: MemoryProvenance) -> &'static str {
    match provenance {
        MemoryProvenance::Stated => "stated",
        MemoryProvenance::Unknown => "unknown",
        MemoryProvenance::Inferred => "inferred",
    }
}

/// `Greater` is the better survivor. The id is a last, arbitrary but stable
/// tiebreak so the same data always yields the same plan.
fn survivor_order(a: &MemoryRow, b: &MemoryRow) -> std::cmp::Ordering {
    provenance_rank(a.provenance())
        .cmp(&provenance_rank(b.provenance()))
        .then(a.updated_at.cmp(&b.updated_at))
        .then(a.confidence.total_cmp(&b.confidence))
        .then(a.content.chars().count().cmp(&b.content.chars().count()))
        .then_with(|| a.id.cmp(&b.id))
}

fn choose_survivor(rows: &[MemoryRow], members: &[usize]) -> Option<usize> {
    members
        .iter()
        .copied()
        .max_by(|&a, &b| survivor_order(&rows[a], &rows[b]))
}

/// The pair ordered as write-time grounding would have seen it: the newer row
/// is the NEW FACT, the older one the EXISTING MEMORY.
fn newer_first<'a>(a: &'a MemoryRow, b: &'a MemoryRow) -> (&'a MemoryRow, &'a MemoryRow) {
    if (a.updated_at, &a.id) >= (b.updated_at, &b.id) {
        (a, b)
    } else {
        (b, a)
    }
}

// ---- plan file ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum Verdict {
    Add,
    Noop,
    Extend,
    Supersede,
}

impl Verdict {
    fn of(action: &GroundingAction) -> Self {
        match action {
            GroundingAction::Add => Self::Add,
            GroundingAction::Noop => Self::Noop,
            GroundingAction::Extend { .. } => Self::Extend,
            GroundingAction::Supersede { .. } => Self::Supersede,
        }
    }

    /// ADD means "a different fact". Every other verdict relates the pair.
    fn relates(self) -> bool {
        self != Self::Add
    }

    fn label(self) -> &'static str {
        match self {
            Self::Add => "ADD",
            Self::Noop => "NOOP",
            Self::Extend => "EXTEND",
            Self::Supersede => "SUPERSEDE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Tier {
    Auto,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RowState {
    is_latest: bool,
    supersedes_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PlanRow {
    id: String,
    key: String,
    content_sha256: String,
    updated_at: DateTime<Utc>,
    provenance: String,
    confidence: f64,
    before: RowState,
}

impl PlanRow {
    fn of(row: &MemoryRow) -> Self {
        Self {
            id: row.id.clone(),
            key: row.key.clone(),
            content_sha256: content_sha256(&row.content),
            updated_at: row.updated_at,
            provenance: provenance_label(row.provenance()).to_owned(),
            confidence: row.confidence,
            before: RowState {
                is_latest: true,
                supersedes_id: row.supersedes_id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Check {
    /// A pre-filtered pair that joined these rows into one cluster.
    ClusterLink,
    /// A retired row compared against the survivor, the row as NEW FACT: the
    /// check the `auto` tier rests on.
    RestatesSurvivor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Evidence {
    check: Check,
    new_fact_id: String,
    existing_id: String,
    jaccard: f64,
    overlap: f64,
    /// `None` when the plan ran with `--classifier off`.
    verdict: Option<Verdict>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PlanCluster {
    cluster_id: String,
    tier: Tier,
    reason: String,
    partition: Partition,
    survivor: PlanRow,
    /// Newest first: the order `apply` chains `supersedes_id` in.
    retire: Vec<PlanRow>,
    evidence: Vec<Evidence>,
}

impl PlanCluster {
    fn rows(&self) -> impl Iterator<Item = &PlanRow> {
        std::iter::once(&self.survivor).chain(self.retire.iter())
    }

    fn row_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.rows().map(|row| row.id.clone()).collect();
        ids.sort();
        ids
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ClassifierInfo {
    enabled: bool,
    model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LexicalConfig {
    top_k: usize,
    jaccard_floor: f64,
    overlap_floor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Plan {
    format: String,
    plan_id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    note: String,
    classifier: ClassifierInfo,
    lexical: LexicalConfig,
    rows_considered: usize,
    clusters: Vec<PlanCluster>,
}

fn content_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// Build one cluster from a component. Pure: every verdict it reads was
/// already decided and is passed in.
fn assemble_cluster(
    cluster_id: String,
    partition: &Partition,
    rows: &[MemoryRow],
    members: &[usize],
    pairs: &[CandidatePair],
    verdicts: Option<&HashMap<(String, String), Verdict>>,
) -> Option<PlanCluster> {
    let survivor_index = choose_survivor(rows, members)?;
    let survivor = &rows[survivor_index];
    let mut retire: Vec<&MemoryRow> = members
        .iter()
        .filter(|&&member| member != survivor_index)
        .map(|&member| &rows[member])
        .collect();
    if retire.is_empty() {
        return None;
    }
    retire.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
    });

    let verdict_of = |new: &MemoryRow, existing: &MemoryRow| {
        verdicts.and_then(|map| map.get(&(new.id.clone(), existing.id.clone())).copied())
    };
    let member_set: BTreeSet<usize> = members.iter().copied().collect();
    let mut evidence: Vec<Evidence> = pairs
        .iter()
        .filter(|pair| member_set.contains(&pair.a) && member_set.contains(&pair.b))
        .map(|pair| {
            let (new, existing) = newer_first(&rows[pair.a], &rows[pair.b]);
            Evidence {
                check: Check::ClusterLink,
                new_fact_id: new.id.clone(),
                existing_id: existing.id.clone(),
                jaccard: round2(pair.score.jaccard),
                overlap: round2(pair.score.overlap),
                verdict: verdict_of(new, existing),
            }
        })
        .collect();

    let (tier, reason) = if verdicts.is_some() {
        let survivor_tokens = tokens(&survivor.content);
        let mut not_restating = 0usize;
        for row in &retire {
            let verdict = verdict_of(row, survivor);
            let score = LexicalScore::between(&tokens(&row.content), &survivor_tokens);
            evidence.push(Evidence {
                check: Check::RestatesSurvivor,
                new_fact_id: row.id.clone(),
                existing_id: survivor.id.clone(),
                jaccard: round2(score.jaccard),
                overlap: round2(score.overlap),
                verdict,
            });
            if verdict != Some(Verdict::Noop) {
                not_restating += 1;
            }
        }
        if not_restating == 0 {
            (
                Tier::Auto,
                "every retired row restates the survivor (classifier NOOP)".to_owned(),
            )
        } else {
            (
                Tier::Review,
                format!(
                    "{not_restating} row(s) add to, correct or differ from the survivor; \
                     retiring them would lose information"
                ),
            )
        }
    } else {
        (
            Tier::Review,
            "lexical similarity only (--classifier off); confirm each retired row says nothing \
             the survivor does not"
                .to_owned(),
        )
    };

    Some(PlanCluster {
        cluster_id,
        tier,
        reason,
        partition: partition.clone(),
        survivor: PlanRow::of(survivor),
        retire: retire.into_iter().map(PlanRow::of).collect(),
        evidence,
    })
}

// ---- plan ----

async fn ensure_lineage_migration(conn: &mut PgConnection) -> anyhow::Result<()> {
    let tracked: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(&mut *conn)
        .await?;
    let applied = tracked
        && sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = $1 AND success)",
        )
        .bind(LINEAGE_MIGRATION)
        .fetch_one(&mut *conn)
        .await?;
    if !applied {
        bail!(
            "migration 0037 (agent memory lineage) is not applied to this database; \
             memory-dedup needs its is_latest/supersedes_id columns and will not run"
        );
    }
    Ok(())
}

/// Every current row, read in one read-only snapshot.
async fn load_current_rows(pool: &PgPool) -> anyhow::Result<Vec<MemoryRow>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    ensure_lineage_migration(&mut tx).await?;
    #[allow(clippy::type_complexity)]
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            f64,
            Vec<String>,
            DateTime<Utc>,
            Option<String>,
        ),
    >(
        "SELECT m.id, m.org_id, m.session_id, m.scope, m.owner, m.key, m.content, m.confidence, \
                m.source_links, m.updated_at, m.supersedes_id \
         FROM agent_memory m \
         WHERE m.is_latest = true \
           AND m.review_state = 'accepted' \
           AND (m.expires_at IS NULL OR m.expires_at > now()) \
           AND NOT EXISTS ( \
             SELECT 1 FROM forgotten_memory_ids f \
             WHERE f.org_id = m.org_id AND f.memory_id = m.id) \
         ORDER BY m.org_id, m.id",
    )
    .fetch_all(&mut *tx)
    .await?;
    // Read-only: there is nothing to commit.
    tx.rollback().await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                org_id,
                session_id,
                scope,
                owner,
                key,
                content,
                confidence,
                source_links,
                updated_at,
                supersedes_id,
            )| {
                MemoryRow {
                    id,
                    org_id,
                    session_id,
                    scope,
                    owner,
                    key,
                    content,
                    confidence,
                    source_links,
                    updated_at,
                    supersedes_id,
                }
            },
        )
        .collect())
}

async fn classify_pair(
    classifier: &GroundingClassifier,
    partition: &Partition,
    new: &MemoryRow,
    existing: &MemoryRow,
) -> Verdict {
    let matches = [ExistingMatch {
        id: existing.id.clone(),
        content: existing.content.clone(),
    }];
    Verdict::of(
        &classifier
            .classify(&partition.org_id, &new.content, &partition.scope, &matches)
            .await,
    )
}

/// The whole plan, plus each considered row's content for the optional
/// report preview (never written to the plan file).
async fn build_plan(
    pool: &PgPool,
    classifier: Option<&GroundingClassifier>,
) -> anyhow::Result<(Plan, HashMap<String, String>)> {
    let rows = load_current_rows(pool).await?;
    let rows_considered = rows.len();
    let contents: HashMap<String, String> = rows
        .iter()
        .map(|row| (row.id.clone(), row.content.clone()))
        .collect();
    let mut partitions: BTreeMap<Partition, Vec<MemoryRow>> = BTreeMap::new();
    for row in rows {
        partitions.entry(row.partition()).or_default().push(row);
    }

    let mut clusters = Vec::new();
    for (partition, mut rows) in partitions {
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        let pairs = candidate_pairs(&rows);
        if pairs.is_empty() {
            continue;
        }
        // Classification runs after the read transaction closed: these are
        // network calls, and nothing here holds a snapshot across them.
        let mut verdicts: HashMap<(String, String), Verdict> = HashMap::new();
        let edges: Vec<(usize, usize)> = match classifier {
            None => pairs.iter().map(|pair| (pair.a, pair.b)).collect(),
            Some(classifier) => {
                let mut edges = Vec::new();
                for pair in &pairs {
                    let (new, existing) = newer_first(&rows[pair.a], &rows[pair.b]);
                    let verdict = classify_pair(classifier, &partition, new, existing).await;
                    verdicts.insert((new.id.clone(), existing.id.clone()), verdict);
                    if verdict.relates() {
                        edges.push((pair.a, pair.b));
                    }
                }
                edges
            }
        };
        for members in components(rows.len(), &edges) {
            if let (Some(classifier), Some(survivor)) =
                (classifier, choose_survivor(&rows, &members))
            {
                for &member in members.iter().filter(|&&member| member != survivor) {
                    let key = (rows[member].id.clone(), rows[survivor].id.clone());
                    if !verdicts.contains_key(&key) {
                        let verdict =
                            classify_pair(classifier, &partition, &rows[member], &rows[survivor])
                                .await;
                        verdicts.insert(key, verdict);
                    }
                }
            }
            let cluster_id = format!("c{:03}", clusters.len() + 1);
            if let Some(cluster) = assemble_cluster(
                cluster_id,
                &partition,
                &rows,
                &members,
                &pairs,
                classifier.map(|_| &verdicts),
            ) {
                clusters.push(cluster);
            }
        }
    }

    let plan = Plan {
        format: PLAN_FORMAT.to_owned(),
        plan_id: new_ulid(),
        created_at: Utc::now(),
        note: PLAN_NOTE.to_owned(),
        classifier: ClassifierInfo {
            enabled: classifier.is_some(),
            model: classifier.map(|classifier| classifier.model().to_owned()),
        },
        lexical: LexicalConfig {
            top_k: TOP_K,
            jaccard_floor: JACCARD_FLOOR,
            overlap_floor: OVERLAP_FLOOR,
        },
        rows_considered,
        clusters,
    };
    Ok((plan, contents))
}

async fn run_plan(
    pool: &PgPool,
    classifier_on: bool,
    out: Option<PathBuf>,
    show_content: bool,
) -> anyhow::Result<()> {
    let classifier = if classifier_on {
        Some(GroundingClassifier::from_connection_env().context(
            "--classifier on needs the inference-core connection grounding uses \
             (INFERENCE_CORE_ADDR or INFERENCE_CORE_URL, AUTH_CORE_URL, SESSION_CORE_SERVICE_ID, \
             SESSION_CORE_SERVICE_API_KEY); pass --classifier off for a lexical-only plan",
        )?)
    } else {
        None
    };
    let (plan, contents) = build_plan(pool, classifier.as_ref()).await?;
    let path = out.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("memory-dedup-plan-{}.json", plan.plan_id))
    });
    let mut json = serde_json::to_string_pretty(&plan)?;
    json.push('\n');
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    print!("{}", render_plan(&plan, show_content.then_some(&contents)));
    println!("\nplan written to {}", path.display());
    println!(
        "review it, then: session-core memory-dedup apply --plan {} [--dry-run]",
        path.display()
    );
    Ok(())
}

fn short(value: &str, chars: usize) -> String {
    value.chars().take(chars).collect()
}

fn describe_partition(partition: &Partition) -> String {
    let org = short(&partition.org_id, 8);
    match (&partition.session_id, &partition.owner) {
        (Some(session_id), _) => format!(
            "org {org} {} thread {}",
            partition.scope,
            short(session_id, 12)
        ),
        // Owners are user ids; a hash prefix is enough to tell them apart.
        (None, Some(owner)) => format!(
            "org {org} {} owner #{}",
            partition.scope,
            short(&content_sha256(owner), 6)
        ),
        (None, None) => format!("org {org} {}", partition.scope),
    }
}

fn render_plan(plan: &Plan, contents: Option<&HashMap<String, String>>) -> String {
    let auto: Vec<&PlanCluster> = plan
        .clusters
        .iter()
        .filter(|cluster| cluster.tier == Tier::Auto)
        .collect();
    let retire: usize = plan
        .clusters
        .iter()
        .map(|cluster| cluster.retire.len())
        .sum();
    let retire_auto: usize = auto.iter().map(|cluster| cluster.retire.len()).sum();
    let classifier = match (&plan.classifier.enabled, &plan.classifier.model) {
        (true, Some(model)) => format!("on, {model}"),
        (true, None) => "on".to_owned(),
        (false, _) => "off".to_owned(),
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "memory-dedup plan {} (classifier {classifier})",
        plan.plan_id
    );
    let _ = writeln!(
        out,
        "{} current row(s) considered; {} cluster(s), {} auto and {} review; \
         {retire} row(s) to retire, {retire_auto} of them in auto clusters",
        plan.rows_considered,
        plan.clusters.len(),
        auto.len(),
        plan.clusters.len() - auto.len(),
    );
    for cluster in &plan.clusters {
        let tier = match cluster.tier {
            Tier::Auto => "auto",
            Tier::Review => "review",
        };
        let _ = writeln!(
            out,
            "\n{} {tier:<6} {}: {}",
            cluster.cluster_id,
            describe_partition(&cluster.partition),
            cluster.reason
        );
        for (role, row) in std::iter::once(("keep", &cluster.survivor))
            .chain(cluster.retire.iter().map(|row| ("retire", row)))
        {
            let _ = writeln!(
                out,
                "  {role:<6} {}  {:<8} {}  {}",
                row.id,
                row.provenance,
                row.updated_at.format("%Y-%m-%d %H:%M"),
                row.key
            );
            if let Some(content) = contents.and_then(|contents| contents.get(&row.id)) {
                let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
                let _ = writeln!(out, "         \"{}\"", short(&collapsed, PREVIEW_CHARS));
            }
        }
        for evidence in &cluster.evidence {
            let check = match evidence.check {
                Check::ClusterLink => "link",
                Check::RestatesSurvivor => "vs survivor",
            };
            let _ = writeln!(
                out,
                "  {check:<11} {} ~ {}  jaccard {:.2} overlap {:.2}  {}",
                short(&evidence.new_fact_id, 12),
                short(&evidence.existing_id, 12),
                evidence.jaccard,
                evidence.overlap,
                evidence.verdict.map_or("-", Verdict::label)
            );
        }
    }
    out
}

// ---- apply ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RowChange {
    id: String,
    before: RowState,
    after: RowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LedgerAction {
    Apply,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SkippedRow {
    id: String,
    reason: String,
}

/// `dream_runs.metadata` of a ledger row. Ids, booleans and fixed labels
/// only: nothing here is content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LedgerEntry {
    plan_id: String,
    cluster_id: String,
    action: LedgerAction,
    changes: Vec<RowChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<SkippedRow>,
}

/// A planned row as it is in the database now, read under `FOR UPDATE`.
#[derive(Debug, Clone, PartialEq)]
struct LiveRow {
    partition: Partition,
    key: String,
    content_sha256: String,
    updated_at: DateTime<Utc>,
    state: RowState,
    accepted: bool,
    expired: bool,
    forgotten: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleReason {
    Missing,
    Retired,
    Forgotten,
    NotAccepted,
    Expired,
    PartitionChanged,
    KeyChanged,
    ContentChanged,
    UpdatedAtChanged,
    LineageChanged,
}

impl StaleReason {
    fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Retired => "retired",
            Self::Forgotten => "forgotten",
            Self::NotAccepted => "not accepted",
            Self::Expired => "expired",
            Self::PartitionChanged => "partition changed",
            Self::KeyChanged => "key changed",
            Self::ContentChanged => "content changed",
            Self::UpdatedAtChanged => "updated_at changed",
            Self::LineageChanged => "lineage changed",
        }
    }
}

/// Everything that makes a planned row no longer the row that was reviewed.
fn stale_reasons(
    planned: &PlanRow,
    partition: &Partition,
    live: Option<&LiveRow>,
) -> Vec<StaleReason> {
    let Some(live) = live else {
        return vec![StaleReason::Missing];
    };
    let mut reasons = Vec::new();
    if !live.state.is_latest {
        reasons.push(StaleReason::Retired);
    }
    if live.forgotten {
        reasons.push(StaleReason::Forgotten);
    }
    if !live.accepted {
        reasons.push(StaleReason::NotAccepted);
    }
    if live.expired {
        reasons.push(StaleReason::Expired);
    }
    if &live.partition != partition {
        reasons.push(StaleReason::PartitionChanged);
    }
    if live.key != planned.key {
        reasons.push(StaleReason::KeyChanged);
    }
    if live.content_sha256 != planned.content_sha256 {
        reasons.push(StaleReason::ContentChanged);
    }
    if live.updated_at != planned.updated_at {
        reasons.push(StaleReason::UpdatedAtChanged);
    }
    if live.state.supersedes_id != planned.before.supersedes_id {
        reasons.push(StaleReason::LineageChanged);
    }
    reasons
}

fn stale_rows(
    cluster: &PlanCluster,
    live: &HashMap<String, LiveRow>,
) -> Vec<(String, StaleReason)> {
    cluster
        .rows()
        .flat_map(|row| {
            stale_reasons(row, &cluster.partition, live.get(&row.id))
                .into_iter()
                .map(|reason| (row.id.clone(), reason))
        })
        .collect()
}

/// A plan cluster is only applied as the reviewer left it if it still makes
/// sense on its own terms.
fn validate_cluster(cluster: &PlanCluster) -> Result<(), String> {
    if cluster.retire.is_empty() {
        return Err("nothing to retire".to_owned());
    }
    if cluster.partition.session_id.is_some() == cluster.partition.owner.is_some() {
        return Err("the partition must name exactly one of session_id and owner".to_owned());
    }
    let mut seen = BTreeSet::new();
    for row in cluster.rows() {
        if !seen.insert(row.id.as_str()) {
            return Err(format!("row {} is listed twice", row.id));
        }
        if !row.before.is_latest {
            return Err(format!("row {} was not current when planned", row.id));
        }
    }
    Ok(())
}

fn check_plan(plan: &Plan) -> Result<(), String> {
    if plan.format != PLAN_FORMAT {
        return Err(format!(
            "unsupported plan format {:?} (expected {PLAN_FORMAT:?})",
            plan.format
        ));
    }
    if plan.plan_id.trim().is_empty() {
        return Err("the plan has no plan_id".to_owned());
    }
    let mut seen = BTreeSet::new();
    for cluster in &plan.clusters {
        if !seen.insert(cluster.cluster_id.as_str()) {
            return Err(format!("cluster {} appears twice", cluster.cluster_id));
        }
    }
    Ok(())
}

/// What apply changes: every retired row stops being current, and
/// `supersedes_id` is chained survivor -> newest retired -> ... -> oldest,
/// but only where it is NULL, so existing lineage is never overwritten.
fn planned_changes(cluster: &PlanCluster) -> Vec<RowChange> {
    let mut retire: Vec<&PlanRow> = cluster.retire.iter().collect();
    retire.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    let chain: Vec<&PlanRow> = std::iter::once(&cluster.survivor).chain(retire).collect();
    let mut changes = Vec::new();
    for (index, row) in chain.iter().enumerate() {
        let supersedes_id = match (&row.before.supersedes_id, chain.get(index + 1)) {
            (None, Some(next)) => Some(next.id.clone()),
            (existing, _) => existing.clone(),
        };
        let after = RowState {
            is_latest: index == 0,
            supersedes_id,
        };
        if after != row.before {
            changes.push(RowChange {
                id: row.id.clone(),
                before: row.before.clone(),
                after,
            });
        }
    }
    changes
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApplyOutcome {
    Applied { retired: usize, linked: usize },
    WouldApply { retired: usize, linked: usize },
    AlreadyApplied,
    NeedsReview,
    Invalid(String),
    Stale(Vec<(String, StaleReason)>),
    Refused(String),
}

impl ApplyOutcome {
    fn describe(&self) -> String {
        match self {
            Self::Applied { retired, linked } => {
                format!("applied: {retired} row(s) retired, {linked} lineage link(s) set")
            }
            Self::WouldApply { retired, linked } => format!(
                "dry run: would retire {retired} row(s) and set {linked} lineage link(s); rolled back"
            ),
            Self::AlreadyApplied => "already applied; skipped".to_owned(),
            Self::NeedsReview => "tier is review; skipped (set it to auto to approve)".to_owned(),
            Self::Invalid(problem) => format!("invalid, skipped: {problem}"),
            Self::Stale(rows) => {
                let detail: Vec<String> = rows
                    .iter()
                    .map(|(id, reason)| format!("{id} {}", reason.label()))
                    .collect();
                format!("stale, skipped: {}", detail.join("; "))
            }
            Self::Refused(problem) => format!("refused and rolled back: {problem}"),
        }
    }
}

async fn lock_rows(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[String],
) -> Result<HashMap<String, LiveRow>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            DateTime<Utc>,
            bool,
            Option<String>,
            bool,
            bool,
            bool,
        ),
    >(
        "SELECT m.id, m.org_id, m.session_id, m.scope, m.owner, m.key, m.content, m.updated_at, \
                m.is_latest, m.supersedes_id, \
                m.review_state = 'accepted', \
                (m.expires_at IS NOT NULL AND m.expires_at <= now()), \
                EXISTS (SELECT 1 FROM forgotten_memory_ids f \
                        WHERE f.org_id = m.org_id AND f.memory_id = m.id) \
         FROM agent_memory m \
         WHERE m.id = ANY($1) \
         ORDER BY m.id \
         FOR UPDATE OF m",
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                org_id,
                session_id,
                scope,
                owner,
                key,
                content,
                updated_at,
                is_latest,
                supersedes_id,
                accepted,
                expired,
                forgotten,
            )| {
                let row = LiveRow {
                    partition: Partition::new(&org_id, session_id.as_deref(), &scope, &owner),
                    key,
                    content_sha256: content_sha256(&content),
                    updated_at,
                    state: RowState {
                        is_latest,
                        supersedes_id,
                    },
                    accepted,
                    expired,
                    forgotten,
                };
                (id, row)
            },
        )
        .collect())
}

async fn latest_ledger_entry(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    plan_id: &str,
    cluster_id: &str,
) -> Result<Option<LedgerEntry>, sqlx::Error> {
    let entry: Option<Json<LedgerEntry>> = sqlx::query_scalar(
        "SELECT metadata FROM dream_runs \
         WHERE trigger = $1 AND org_id = $2 \
           AND metadata ->> 'plan_id' = $3 AND metadata ->> 'cluster_id' = $4 \
         ORDER BY started_at DESC, id DESC \
         LIMIT 1",
    )
    .bind(LEDGER_TRIGGER)
    .bind(org_id)
    .bind(plan_id)
    .bind(cluster_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(entry.map(|Json(entry)| entry))
}

/// The ledger row. `thread_id` stays NULL: the Dreaming loop's pending-message
/// query matches ledger rows on `thread_id`, so this can never be mistaken
/// for a consolidated message.
async fn record_ledger(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    entry: &LedgerEntry,
    found: usize,
    saved: usize,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dream_runs \
         (id, org_id, thread_id, trigger, status, memories_found, memories_saved, completed_at, metadata) \
         VALUES ($1, $2, NULL, $3, 'completed', $4, $5, now(), $6)",
    )
    .bind(new_ulid())
    .bind(org_id)
    .bind(LEDGER_TRIGGER)
    .bind(i32::try_from(found).unwrap_or(i32::MAX))
    .bind(i32::try_from(saved).unwrap_or(i32::MAX))
    .bind(Json(entry))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Exactly one current row -- the survivor -- and no `supersedes_id` cycle
/// reachable from any row of the cluster.
async fn verify_cluster(
    tx: &mut Transaction<'_, Postgres>,
    cluster: &PlanCluster,
    ids: &[String],
) -> Result<Result<(), String>, sqlx::Error> {
    let current: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM agent_memory WHERE id = ANY($1) AND is_latest ORDER BY id",
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    if current != [cluster.survivor.id.clone()] {
        return Ok(Err(format!(
            "expected exactly one current row (the survivor), found {}",
            current.len()
        )));
    }
    let cycle: bool = sqlx::query_scalar(
        "WITH RECURSIVE chain(id, supersedes_id) AS ( \
             SELECT id, supersedes_id FROM agent_memory WHERE id = ANY($1) \
           UNION ALL \
             SELECT m.id, m.supersedes_id FROM agent_memory m \
             JOIN chain c ON m.id = c.supersedes_id \
         ) CYCLE id SET is_cycle USING path \
         SELECT EXISTS (SELECT 1 FROM chain WHERE is_cycle)",
    )
    .bind(ids)
    .fetch_one(&mut **tx)
    .await?;
    if cycle {
        return Ok(Err(
            "the supersedes_id chain would contain a cycle".to_owned()
        ));
    }
    Ok(Ok(()))
}

async fn apply_cluster(
    pool: &PgPool,
    plan_id: &str,
    cluster: &PlanCluster,
    dry_run: bool,
) -> Result<ApplyOutcome, sqlx::Error> {
    if cluster.tier != Tier::Auto {
        return Ok(ApplyOutcome::NeedsReview);
    }
    if let Err(problem) = validate_cluster(cluster) {
        return Ok(ApplyOutcome::Invalid(problem));
    }
    let org_id = cluster.partition.org_id.as_str();
    let ids = cluster.row_ids();
    let mut tx = pool.begin().await?;
    let live = lock_rows(&mut tx, &ids).await?;

    if let Some(entry) = latest_ledger_entry(&mut tx, org_id, plan_id, &cluster.cluster_id).await? {
        let still_applied = entry.action == LedgerAction::Apply
            && entry.changes.iter().all(|change| {
                live.get(&change.id)
                    .is_some_and(|row| row.state == change.after)
            });
        if still_applied {
            tx.rollback().await?;
            return Ok(ApplyOutcome::AlreadyApplied);
        }
    }

    let stale = stale_rows(cluster, &live);
    if !stale.is_empty() {
        tx.rollback().await?;
        return Ok(ApplyOutcome::Stale(stale));
    }

    let changes = planned_changes(cluster);
    for change in &changes {
        // `updated_at` is deliberately absent: retiring a duplicate is not an
        // edit of it, and the memory panel orders by it.
        let updated = sqlx::query(
            "UPDATE agent_memory SET is_latest = $2, supersedes_id = $3 \
             WHERE id = $1 AND org_id = $4 \
               AND is_latest = $5 AND supersedes_id IS NOT DISTINCT FROM $6",
        )
        .bind(&change.id)
        .bind(change.after.is_latest)
        .bind(change.after.supersedes_id.as_deref())
        .bind(org_id)
        .bind(change.before.is_latest)
        .bind(change.before.supersedes_id.as_deref())
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(ApplyOutcome::Refused(format!(
                "row {} changed while locked",
                change.id
            )));
        }
    }
    if let Err(problem) = verify_cluster(&mut tx, cluster, &ids).await? {
        tx.rollback().await?;
        return Ok(ApplyOutcome::Refused(problem));
    }

    let retired = changes
        .iter()
        .filter(|change| change.before.is_latest && !change.after.is_latest)
        .count();
    let linked = changes
        .iter()
        .filter(|change| change.before.supersedes_id != change.after.supersedes_id)
        .count();
    let entry = LedgerEntry {
        plan_id: plan_id.to_owned(),
        cluster_id: cluster.cluster_id.clone(),
        action: LedgerAction::Apply,
        changes,
        skipped: Vec::new(),
    };
    record_ledger(&mut tx, org_id, &entry, ids.len(), retired).await?;
    if dry_run {
        tx.rollback().await?;
        return Ok(ApplyOutcome::WouldApply { retired, linked });
    }
    tx.commit().await?;
    Ok(ApplyOutcome::Applied { retired, linked })
}

async fn run_apply(pool: &PgPool, path: &Path, dry_run: bool) -> anyhow::Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let plan: Plan = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not a memory-dedup plan", path.display()))?;
    check_plan(&plan).map_err(anyhow::Error::msg)?;
    {
        let mut conn = pool.acquire().await?;
        ensure_lineage_migration(&mut conn).await?;
    }
    println!(
        "memory-dedup {} plan {} ({} cluster(s))",
        if dry_run { "apply --dry-run" } else { "apply" },
        plan.plan_id,
        plan.clusters.len()
    );
    let (mut applied, mut skipped) = (0usize, 0usize);
    for cluster in &plan.clusters {
        let outcome = apply_cluster(pool, &plan.plan_id, cluster, dry_run)
            .await
            .with_context(|| format!("applying cluster {}", cluster.cluster_id))?;
        if matches!(
            outcome,
            ApplyOutcome::Applied { .. } | ApplyOutcome::WouldApply { .. }
        ) {
            applied += 1;
        } else {
            skipped += 1;
        }
        println!("{} {}", cluster.cluster_id, outcome.describe());
    }
    println!(
        "{applied} cluster(s) {}, {skipped} skipped",
        if dry_run { "checked" } else { "applied" }
    );
    if !dry_run && applied > 0 {
        println!(
            "undo with: session-core memory-dedup rollback --plan-id {}",
            plan.plan_id
        );
    }
    Ok(())
}

// ---- rollback ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RollbackSkip {
    Missing,
    Changed,
    Forgotten,
}

impl RollbackSkip {
    fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Changed => "changed since apply",
            Self::Forgotten => "forgotten since apply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RollbackDecision {
    Revert,
    Skip(RollbackSkip),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RollbackRow {
    state: RowState,
    forgotten: bool,
}

/// Revert only a row that is still exactly as apply left it. A row deleted
/// since stays deleted, a row something else changed keeps that change, and
/// a row the user forgot is never made current again.
fn rollback_decision(change: &RowChange, live: Option<&RollbackRow>) -> RollbackDecision {
    match live {
        None => RollbackDecision::Skip(RollbackSkip::Missing),
        Some(row) if row.forgotten => RollbackDecision::Skip(RollbackSkip::Forgotten),
        Some(row) if row.state == change.after => RollbackDecision::Revert,
        Some(_) => RollbackDecision::Skip(RollbackSkip::Changed),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RollbackOutcome {
    RolledBack {
        reverted: usize,
        skipped: Vec<SkippedRow>,
    },
    AlreadyRolledBack,
}

async fn rollback_cluster(
    pool: &PgPool,
    org_id: &str,
    applied: &LedgerEntry,
) -> Result<RollbackOutcome, sqlx::Error> {
    let mut ids: Vec<String> = applied
        .changes
        .iter()
        .map(|change| change.id.clone())
        .collect();
    ids.sort();
    let mut tx = pool.begin().await?;
    let live: HashMap<String, RollbackRow> =
        sqlx::query_as::<_, (String, bool, Option<String>, bool)>(
            "SELECT m.id, m.is_latest, m.supersedes_id, \
                EXISTS (SELECT 1 FROM forgotten_memory_ids f \
                        WHERE f.org_id = m.org_id AND f.memory_id = m.id) \
         FROM agent_memory m \
         WHERE m.id = ANY($1) AND m.org_id = $2 \
         ORDER BY m.id \
         FOR UPDATE OF m",
        )
        .bind(&ids)
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(id, is_latest, supersedes_id, forgotten)| {
            (
                id,
                RollbackRow {
                    state: RowState {
                        is_latest,
                        supersedes_id,
                    },
                    forgotten,
                },
            )
        })
        .collect();

    let (mut reverted, mut skipped) = (Vec::new(), Vec::new());
    for change in &applied.changes {
        match rollback_decision(change, live.get(&change.id)) {
            RollbackDecision::Revert => {
                let updated = sqlx::query(
                    "UPDATE agent_memory SET is_latest = $2, supersedes_id = $3 \
                     WHERE id = $1 AND org_id = $4 \
                       AND is_latest = $5 AND supersedes_id IS NOT DISTINCT FROM $6 \
                       AND NOT EXISTS (SELECT 1 FROM forgotten_memory_ids f \
                                       WHERE f.org_id = agent_memory.org_id \
                                         AND f.memory_id = agent_memory.id)",
                )
                .bind(&change.id)
                .bind(change.before.is_latest)
                .bind(change.before.supersedes_id.as_deref())
                .bind(org_id)
                .bind(change.after.is_latest)
                .bind(change.after.supersedes_id.as_deref())
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() == 1 {
                    reverted.push(RowChange {
                        id: change.id.clone(),
                        before: change.after.clone(),
                        after: change.before.clone(),
                    });
                } else {
                    skipped.push(SkippedRow {
                        id: change.id.clone(),
                        reason: RollbackSkip::Changed.label().to_owned(),
                    });
                }
            }
            RollbackDecision::Skip(reason) => skipped.push(SkippedRow {
                id: change.id.clone(),
                reason: reason.label().to_owned(),
            }),
        }
    }

    let count = reverted.len();
    let entry = LedgerEntry {
        plan_id: applied.plan_id.clone(),
        cluster_id: applied.cluster_id.clone(),
        action: LedgerAction::Rollback,
        changes: reverted,
        skipped: skipped.clone(),
    };
    record_ledger(&mut tx, org_id, &entry, applied.changes.len(), count).await?;
    tx.commit().await?;
    Ok(RollbackOutcome::RolledBack {
        reverted: count,
        skipped,
    })
}

/// Roll back every cluster of `plan_id` (or just `cluster`) whose latest
/// ledger row is an apply.
async fn rollback_plan(
    pool: &PgPool,
    plan_id: &str,
    cluster: Option<&str>,
) -> Result<Vec<(String, RollbackOutcome)>, sqlx::Error> {
    let entries: Vec<(String, Json<LedgerEntry>)> = sqlx::query_as(
        "SELECT org_id, metadata FROM dream_runs \
         WHERE trigger = $1 AND metadata ->> 'plan_id' = $2 \
           AND ($3::text IS NULL OR metadata ->> 'cluster_id' = $3) \
         ORDER BY started_at ASC, id ASC",
    )
    .bind(LEDGER_TRIGGER)
    .bind(plan_id)
    .bind(cluster)
    .fetch_all(pool)
    .await?;
    let mut latest: BTreeMap<String, (String, LedgerEntry)> = BTreeMap::new();
    for (org_id, Json(entry)) in entries {
        latest.insert(entry.cluster_id.clone(), (org_id, entry));
    }
    let mut outcomes = Vec::with_capacity(latest.len());
    for (cluster_id, (org_id, entry)) in latest {
        let outcome = match entry.action {
            LedgerAction::Rollback => RollbackOutcome::AlreadyRolledBack,
            LedgerAction::Apply => rollback_cluster(pool, &org_id, &entry).await?,
        };
        outcomes.push((cluster_id, outcome));
    }
    Ok(outcomes)
}

async fn run_rollback(pool: &PgPool, plan_id: &str, cluster: Option<&str>) -> anyhow::Result<()> {
    {
        let mut conn = pool.acquire().await?;
        ensure_lineage_migration(&mut conn).await?;
    }
    let outcomes = rollback_plan(pool, plan_id, cluster).await?;
    if outcomes.is_empty() {
        bail!(
            "no memory-dedup ledger rows for plan {plan_id}{}",
            cluster
                .map(|cluster| format!(" cluster {cluster}"))
                .unwrap_or_default()
        );
    }
    for (cluster_id, outcome) in &outcomes {
        match outcome {
            RollbackOutcome::AlreadyRolledBack => {
                println!("{cluster_id} already rolled back; skipped")
            }
            RollbackOutcome::RolledBack { reverted, skipped } => {
                let detail: Vec<String> = skipped
                    .iter()
                    .map(|row| format!("{} {}", row.id, row.reason))
                    .collect();
                if detail.is_empty() {
                    println!("{cluster_id} rolled back: {reverted} row(s) reverted");
                } else {
                    println!(
                        "{cluster_id} rolled back: {reverted} row(s) reverted, left alone: {}",
                        detail.join("; ")
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 19, 11, minute, 0).unwrap()
    }

    fn row(id: &str, content: &str, minute: u32, links: &[&str]) -> MemoryRow {
        MemoryRow {
            id: id.to_owned(),
            org_id: "org-a".to_owned(),
            session_id: None,
            scope: "user".to_owned(),
            owner: "user-a".to_owned(),
            key: format!("user:llm:{id}"),
            content: content.to_owned(),
            confidence: 0.85,
            source_links: links.iter().map(|link| (*link).to_owned()).collect(),
            updated_at: at(minute),
            supersedes_id: None,
        }
    }

    // ---- dispatch and arguments ----

    /// The container starts `session-core` with no arguments. Only the exact
    /// subcommand may divert it; anything else is the service as before.
    #[test]
    fn only_the_exact_subcommand_diverts_startup() {
        assert!(!is_invocation(None));
        assert!(is_invocation(Some(OsStr::new("memory-dedup"))));
        for other in ["serve", "memory", "memory-dedup-x", "--memory-dedup", ""] {
            assert!(!is_invocation(Some(OsStr::new(other))), "{other:?}");
        }
    }

    #[test]
    fn plan_is_the_default_mode_and_takes_its_options() {
        assert_eq!(
            parse_args(&[]).unwrap(),
            Command::Plan {
                classifier: true,
                out: None,
                show_content: false
            }
        );
        assert_eq!(
            parse_args(&args(&[
                "--classifier",
                "off",
                "--out",
                "/tmp/p.json",
                "--show-content"
            ]))
            .unwrap(),
            Command::Plan {
                classifier: false,
                out: Some(PathBuf::from("/tmp/p.json")),
                show_content: true
            }
        );
        assert!(parse_args(&args(&["plan", "--classifier", "maybe"])).is_err());
        assert!(parse_args(&args(&["plan", "--out"])).is_err());
        assert!(parse_args(&args(&["plan", "--force"])).is_err());
    }

    #[test]
    fn apply_and_rollback_require_their_target() {
        assert_eq!(
            parse_args(&args(&["apply", "--plan", "p.json", "--dry-run"])).unwrap(),
            Command::Apply {
                plan: PathBuf::from("p.json"),
                dry_run: true
            }
        );
        assert!(parse_args(&args(&["apply"])).is_err());
        assert!(parse_args(&args(&["apply", "--plan", "--dry-run"])).is_err());
        assert_eq!(
            parse_args(&args(&[
                "rollback",
                "--plan-id",
                "01PLAN",
                "--cluster",
                "c002"
            ]))
            .unwrap(),
            Command::Rollback {
                plan_id: "01PLAN".to_owned(),
                cluster: Some("c002".to_owned())
            }
        );
        assert!(parse_args(&args(&["rollback", "--cluster", "c002"])).is_err());
        assert!(parse_args(&args(&["delete"])).is_err());
        assert_eq!(
            parse_args(&args(&["apply", "--help"])).unwrap(),
            Command::Help
        );
    }

    // ---- partitioning ----

    #[test]
    fn partitions_follow_the_unique_indexes() {
        let user_a = Partition::new("org-a", None, "user", "user-a");
        assert_eq!(user_a.owner.as_deref(), Some("user-a"));
        assert!(user_a.session_id.is_none());
        assert_ne!(user_a, Partition::new("org-a", None, "user", "user-b"));
        assert_ne!(user_a, Partition::new("org-b", None, "user", "user-a"));
        assert_ne!(user_a, Partition::new("org-a", None, "org", "user-a"));

        // Thread rows are keyed by session; the owner (always '' today) is
        // not part of their index and must not split them.
        let thread = Partition::new("org-a", Some("thread-1"), "thread", "");
        assert_eq!(
            thread,
            Partition::new("org-a", Some("thread-1"), "thread", "someone")
        );
        assert!(thread.owner.is_none());
        assert_ne!(
            thread,
            Partition::new("org-a", Some("thread-2"), "thread", "")
        );
        // A thread's copy of a user-level fact is never in the user's partition.
        assert_ne!(thread, Partition::new("org-a", None, "thread", ""));
    }

    // ---- lexical scoring ----

    #[test]
    fn restatements_score_as_candidates_and_unrelated_facts_do_not() {
        let a = tokens("User wants drafts only and never sends messages or changes orders.");
        let b = tokens("User prefers drafts only; never send messages or change orders.");
        let c = tokens("User works at Aquatiq in Bergen.");
        assert!(a.contains("draft") && a.contains("messag") && a.contains("order"));
        assert!(!a.contains("user") && !a.contains("wants"));
        let restated = LexicalScore::between(&a, &b);
        assert!(restated.is_candidate(), "{restated:?}");
        assert!(!LexicalScore::between(&a, &c).is_candidate());
        // A subset restatement is caught by overlap even when Jaccard is low.
        let subset = LexicalScore::between(
            &tokens("User uses Visma.net."),
            &tokens("User uses Visma.net daily for invoicing, orders and customer lookups."),
        );
        assert!(
            subset.jaccard < JACCARD_FLOOR && subset.overlap >= OVERLAP_FLOOR,
            "{subset:?}"
        );
        assert_eq!(
            LexicalScore::between(&BTreeSet::new(), &BTreeSet::new()).overlap,
            0.0
        );
    }

    #[test]
    fn the_prefilter_keeps_at_most_top_k_neighbours_per_row() {
        let rows: Vec<MemoryRow> = (0..9)
            .map(|index| {
                row(
                    &format!("r{index}"),
                    "User keeps drafts in Visma.net.",
                    index,
                    &[],
                )
            })
            .collect();
        let pairs = candidate_pairs(&rows);
        for index in 0..rows.len() {
            let degree = pairs
                .iter()
                .filter(|pair| pair.a == index || pair.b == index)
                .count();
            // Each row nominates TOP_K; a row can also be nominated by others.
            assert!(degree >= TOP_K.min(rows.len() - 1));
        }
        let nominated: BTreeSet<(usize, usize)> =
            pairs.iter().map(|pair| (pair.a, pair.b)).collect();
        assert_eq!(nominated.len(), pairs.len(), "each pair appears once");
        assert!(pairs.iter().all(|pair| pair.a < pair.b));
        assert!(pairs.len() <= rows.len() * TOP_K);
        assert!(candidate_pairs(&rows[..1]).is_empty());
    }

    #[test]
    fn components_join_linked_rows_and_drop_singletons() {
        assert_eq!(
            components(6, &[(0, 1), (1, 2), (4, 5)]),
            vec![vec![0, 1, 2], vec![4, 5]]
        );
        assert!(components(3, &[]).is_empty());
    }

    // ---- survivor ----

    #[test]
    fn a_stated_row_survives_over_newer_inferred_ones() {
        let rows = vec![
            row("stated", "User's name is Test.", 1, &["tool:save_memory"]),
            row("inferred", "User's name is Test.", 9, &["extractor:llm"]),
            row(
                "unknown",
                "User's name is Test.",
                5,
                &["thread:t", "message:m"],
            ),
        ];
        assert_eq!(choose_survivor(&rows, &[0, 1, 2]), Some(0));
        // Without the stated row, unknown provenance beats inferred.
        assert_eq!(choose_survivor(&rows, &[1, 2]), Some(2));
    }

    #[test]
    fn then_the_newest_then_confidence_then_the_fuller_content() {
        let mut rows = vec![
            row("old", "User uses Visma.net.", 1, &["extractor:llm"]),
            row("new", "User uses Visma.net.", 2, &["extractor:llm"]),
        ];
        assert_eq!(choose_survivor(&rows, &[0, 1]), Some(1));
        rows[0].updated_at = rows[1].updated_at;
        rows[0].confidence = 0.9;
        assert_eq!(choose_survivor(&rows, &[0, 1]), Some(0));
        rows[0].confidence = rows[1].confidence;
        rows[0].content = "User uses Visma.net for orders.".to_owned();
        assert_eq!(choose_survivor(&rows, &[0, 1]), Some(0));
        rows[0].content = rows[1].content.clone();
        // Fully tied: the id decides ("old" > "new"), the same way every run
        // and whatever order the members arrive in.
        assert_eq!(choose_survivor(&rows, &[0, 1]), Some(0));
        assert_eq!(choose_survivor(&rows, &[1, 0]), Some(0));
    }

    // ---- clusters and tiers ----

    fn three_restatements() -> (Vec<MemoryRow>, Vec<CandidatePair>) {
        let rows = vec![
            row(
                "a",
                "User wants drafts only and never sends messages or changes orders.",
                1,
                &["extractor:llm"],
            ),
            row(
                "b",
                "Drafts only: the user never sends messages and never changes orders.",
                2,
                &["extractor:llm"],
            ),
            row(
                "c",
                "User prefers drafts only; never send messages or change orders.",
                3,
                &["extractor:llm"],
            ),
        ];
        let pairs = candidate_pairs(&rows);
        (rows, pairs)
    }

    #[test]
    fn a_lexical_only_cluster_always_needs_review() {
        let (rows, pairs) = three_restatements();
        let partition = rows[0].partition();
        let cluster = assemble_cluster(
            "c001".to_owned(),
            &partition,
            &rows,
            &[0, 1, 2],
            &pairs,
            None,
        )
        .unwrap();
        assert_eq!(cluster.tier, Tier::Review);
        assert_eq!(cluster.survivor.id, "c", "newest inferred row survives");
        let retire: Vec<&str> = cluster.retire.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(retire, ["b", "a"], "newest first");
        assert!(cluster
            .evidence
            .iter()
            .all(|evidence| evidence.verdict.is_none()));
    }

    #[test]
    fn only_all_noop_restatements_of_the_survivor_are_automatic() {
        let (rows, pairs) = three_restatements();
        let partition = rows[0].partition();
        let mut verdicts: HashMap<(String, String), Verdict> = HashMap::new();
        for (new, existing) in [("b", "a"), ("c", "a"), ("c", "b"), ("a", "c"), ("b", "c")] {
            verdicts.insert((new.to_owned(), existing.to_owned()), Verdict::Noop);
        }
        let cluster = assemble_cluster(
            "c001".to_owned(),
            &partition,
            &rows,
            &[0, 1, 2],
            &pairs,
            Some(&verdicts),
        )
        .unwrap();
        assert_eq!(cluster.tier, Tier::Auto);
        assert_eq!(
            cluster
                .evidence
                .iter()
                .filter(|evidence| evidence.check == Check::RestatesSurvivor)
                .count(),
            2
        );

        // One retired row that says more than the survivor (EXTEND) sends the
        // whole cluster to review: nothing merged is ever written.
        verdicts.insert(("a".to_owned(), "c".to_owned()), Verdict::Extend);
        let cluster = assemble_cluster(
            "c001".to_owned(),
            &partition,
            &rows,
            &[0, 1, 2],
            &pairs,
            Some(&verdicts),
        )
        .unwrap();
        assert_eq!(cluster.tier, Tier::Review);

        // A missing verdict (a failed classification) is not a NOOP.
        verdicts.remove(&("a".to_owned(), "c".to_owned()));
        let cluster = assemble_cluster(
            "c001".to_owned(),
            &partition,
            &rows,
            &[0, 1, 2],
            &pairs,
            Some(&verdicts),
        )
        .unwrap();
        assert_eq!(cluster.tier, Tier::Review);
    }

    // ---- plan file ----

    fn sample_plan() -> Plan {
        let (rows, pairs) = three_restatements();
        let partition = rows[0].partition();
        let cluster = assemble_cluster(
            "c001".to_owned(),
            &partition,
            &rows,
            &[0, 1, 2],
            &pairs,
            None,
        )
        .unwrap();
        Plan {
            format: PLAN_FORMAT.to_owned(),
            plan_id: "01PLANID".to_owned(),
            created_at: at(30),
            note: PLAN_NOTE.to_owned(),
            classifier: ClassifierInfo {
                enabled: false,
                model: None,
            },
            lexical: LexicalConfig {
                top_k: TOP_K,
                jaccard_floor: JACCARD_FLOOR,
                overlap_floor: OVERLAP_FLOOR,
            },
            rows_considered: 3,
            clusters: vec![cluster],
        }
    }

    #[test]
    fn the_plan_round_trips_and_carries_hashes_not_content() {
        let plan = sample_plan();
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let parsed: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, plan);

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let cluster = &value["clusters"][0];
        for field in [
            "cluster_id",
            "tier",
            "reason",
            "partition",
            "survivor",
            "retire",
            "evidence",
        ] {
            assert!(!cluster[field].is_null(), "cluster.{field}");
        }
        assert_eq!(cluster["tier"], "review");
        assert_eq!(cluster["partition"]["owner"], "user-a");
        assert!(cluster["partition"]["session_id"].is_null());
        let retired = &cluster["retire"][0];
        for field in [
            "id",
            "key",
            "content_sha256",
            "updated_at",
            "provenance",
            "confidence",
        ] {
            assert!(!retired[field].is_null(), "retire.{field}");
        }
        assert_eq!(retired["before"]["is_latest"], true);
        assert!(retired["before"]["supersedes_id"].is_null());
        assert_eq!(retired["content_sha256"].as_str().unwrap().len(), 64);
        assert!(
            !json.contains("drafts"),
            "content must never be written to the plan"
        );
        assert_eq!(value["plan_id"], "01PLANID");
    }

    #[test]
    fn the_report_shows_content_only_when_asked() {
        let plan = sample_plan();
        let plain = render_plan(&plan, None);
        assert!(plain.contains("c001 review"));
        assert!(plain.contains("user:llm:c"));
        assert!(!plain.contains("drafts"));
        let contents: HashMap<String, String> =
            [("c".to_owned(), "User prefers drafts only.".to_owned())].into();
        assert!(render_plan(&plan, Some(&contents)).contains("\"User prefers drafts only.\""));
    }

    #[test]
    fn a_reviewed_plan_is_checked_before_anything_is_applied() {
        let mut plan = sample_plan();
        assert!(check_plan(&plan).is_ok());
        assert!(validate_cluster(&plan.clusters[0]).is_ok());

        let mut cluster = plan.clusters[0].clone();
        cluster.retire.push(cluster.survivor.clone());
        assert!(
            validate_cluster(&cluster).is_err(),
            "survivor also listed as retired"
        );
        let mut cluster = plan.clusters[0].clone();
        cluster.retire.clear();
        assert!(validate_cluster(&cluster).is_err());
        let mut cluster = plan.clusters[0].clone();
        cluster.partition.owner = None;
        assert!(validate_cluster(&cluster).is_err());

        plan.clusters.push(plan.clusters[0].clone());
        assert!(check_plan(&plan).is_err(), "duplicate cluster id");
        plan.clusters.pop();
        plan.format = "something-else".to_owned();
        assert!(check_plan(&plan).is_err());
    }

    // ---- apply ----

    #[test]
    fn apply_chains_newest_to_oldest_and_only_where_lineage_is_empty() {
        let plan = sample_plan();
        let changes = planned_changes(&plan.clusters[0]);
        let expected = vec![
            RowChange {
                id: "c".to_owned(),
                before: RowState {
                    is_latest: true,
                    supersedes_id: None,
                },
                after: RowState {
                    is_latest: true,
                    supersedes_id: Some("b".to_owned()),
                },
            },
            RowChange {
                id: "b".to_owned(),
                before: RowState {
                    is_latest: true,
                    supersedes_id: None,
                },
                after: RowState {
                    is_latest: false,
                    supersedes_id: Some("a".to_owned()),
                },
            },
            RowChange {
                id: "a".to_owned(),
                before: RowState {
                    is_latest: true,
                    supersedes_id: None,
                },
                after: RowState {
                    is_latest: false,
                    supersedes_id: None,
                },
            },
        ];
        assert_eq!(changes, expected);

        // Existing lineage is never overwritten: a survivor that already
        // carries something forward only has its retired rows flipped.
        let mut cluster = plan.clusters[0].clone();
        cluster.survivor.before.supersedes_id = Some("older".to_owned());
        let changes = planned_changes(&cluster);
        assert!(changes.iter().all(|change| change.id != "c"));
        assert_eq!(changes[0].after.supersedes_id.as_deref(), Some("a"));
    }

    fn live_of(planned: &PlanRow, partition: &Partition) -> LiveRow {
        LiveRow {
            partition: partition.clone(),
            key: planned.key.clone(),
            content_sha256: planned.content_sha256.clone(),
            updated_at: planned.updated_at,
            state: planned.before.clone(),
            accepted: true,
            expired: false,
            forgotten: false,
        }
    }

    #[test]
    fn any_drift_since_the_plan_makes_a_row_stale() {
        let plan = sample_plan();
        let cluster = &plan.clusters[0];
        let planned = &cluster.retire[0];
        let fresh = live_of(planned, &cluster.partition);
        assert!(stale_reasons(planned, &cluster.partition, Some(&fresh)).is_empty());
        assert_eq!(
            stale_reasons(planned, &cluster.partition, None),
            [StaleReason::Missing]
        );

        let cases: Vec<(Box<dyn Fn(&mut LiveRow)>, StaleReason)> = vec![
            (
                Box::new(|row| row.state.is_latest = false),
                StaleReason::Retired,
            ),
            (Box::new(|row| row.forgotten = true), StaleReason::Forgotten),
            (
                Box::new(|row| row.accepted = false),
                StaleReason::NotAccepted,
            ),
            (Box::new(|row| row.expired = true), StaleReason::Expired),
            (
                Box::new(|row| row.partition.owner = Some("user-b".to_owned())),
                StaleReason::PartitionChanged,
            ),
            (
                Box::new(|row| row.key = "user:llm:other".to_owned()),
                StaleReason::KeyChanged,
            ),
            (
                Box::new(|row| row.content_sha256 = content_sha256("changed")),
                StaleReason::ContentChanged,
            ),
            (
                Box::new(|row| row.updated_at = at(59)),
                StaleReason::UpdatedAtChanged,
            ),
            (
                Box::new(|row| row.state.supersedes_id = Some("x".to_owned())),
                StaleReason::LineageChanged,
            ),
        ];
        for (mutate, reason) in cases {
            let mut live = fresh.clone();
            mutate(&mut live);
            assert_eq!(
                stale_reasons(planned, &cluster.partition, Some(&live)),
                [reason]
            );
        }

        let mut live: HashMap<String, LiveRow> = cluster
            .rows()
            .map(|row| (row.id.clone(), live_of(row, &cluster.partition)))
            .collect();
        assert!(stale_rows(cluster, &live).is_empty());
        live.remove(&cluster.survivor.id);
        assert_eq!(
            stale_rows(cluster, &live),
            [(cluster.survivor.id.clone(), StaleReason::Missing)]
        );
    }

    // ---- rollback ----

    #[test]
    fn rollback_reverts_only_rows_still_as_apply_left_them() {
        let change = RowChange {
            id: "b".to_owned(),
            before: RowState {
                is_latest: true,
                supersedes_id: None,
            },
            after: RowState {
                is_latest: false,
                supersedes_id: Some("a".to_owned()),
            },
        };
        let row = |is_latest: bool, supersedes_id: Option<&str>, forgotten: bool| RollbackRow {
            state: RowState {
                is_latest,
                supersedes_id: supersedes_id.map(str::to_owned),
            },
            forgotten,
        };
        assert_eq!(
            rollback_decision(&change, Some(&row(false, Some("a"), false))),
            RollbackDecision::Revert
        );
        assert_eq!(
            rollback_decision(&change, None),
            RollbackDecision::Skip(RollbackSkip::Missing)
        );
        // `a` was deleted since: ON DELETE SET NULL cleared the link.
        assert_eq!(
            rollback_decision(&change, Some(&row(false, None, false))),
            RollbackDecision::Skip(RollbackSkip::Changed)
        );
        assert_eq!(
            rollback_decision(&change, Some(&row(true, Some("a"), false))),
            RollbackDecision::Skip(RollbackSkip::Changed)
        );
        assert_eq!(
            rollback_decision(&change, Some(&row(false, Some("a"), true))),
            RollbackDecision::Skip(RollbackSkip::Forgotten)
        );
    }

    #[test]
    fn ledger_entries_hold_ids_and_booleans_only() {
        let entry = LedgerEntry {
            plan_id: "01PLAN".to_owned(),
            cluster_id: "c001".to_owned(),
            action: LedgerAction::Apply,
            changes: planned_changes(&sample_plan().clusters[0]),
            skipped: Vec::new(),
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["action"], "apply");
        assert!(value.get("skipped").is_none());
        for change in value["changes"].as_array().unwrap() {
            let keys: BTreeSet<&str> = change
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, BTreeSet::from(["after", "before", "id"]));
            for state in [&change["before"], &change["after"]] {
                assert!(state["is_latest"].is_boolean());
            }
        }
        let parsed: LedgerEntry = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, entry);
    }

    // ---- against a disposable database ----

    /// The whole lifecycle on seeded rows: plan (lexical-only, so review),
    /// approval by the reviewer, dry run, apply, idempotent re-apply, stale
    /// detection, rollback and idempotent re-rollback -- with `updated_at`
    /// untouched throughout and nothing but ids and booleans in the ledger.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn plan_apply_and_rollback_round_trip() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let org = format!("dedup-{}", new_ulid());
        let thread = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'user-a')",
        )
        .bind(&thread)
        .bind(&org)
        .execute(&pool)
        .await
        .unwrap();
        let seed = |id: &'static str,
                    session: Option<String>,
                    scope: &'static str,
                    owner: &'static str,
                    content: &'static str,
                    minute: u32,
                    link: &'static str| {
            let pool = pool.clone();
            let org = org.clone();
            async move {
                let id = format!("{id}-{}", new_ulid());
                sqlx::query(
                    "INSERT INTO agent_memory \
                     (id, org_id, session_id, scope, key, content, owner, source_links, updated_at) \
                     VALUES ($1, $2, $3, $4, $1, $5, $6, ARRAY[$7], $8)",
                )
                .bind(&id)
                .bind(&org)
                .bind(session)
                .bind(scope)
                .bind(content)
                .bind(owner)
                .bind(link)
                .bind(at(minute))
                .execute(&pool)
                .await
                .unwrap();
                id
            }
        };
        let stated = seed(
            "stated",
            None,
            "user",
            "user-a",
            "User prefers drafts only; never send messages or change orders.",
            1,
            "tool:save_memory",
        )
        .await;
        let middle = seed(
            "middle",
            None,
            "user",
            "user-a",
            "Drafts only: the user never sends messages and never changes orders.",
            2,
            "extractor:llm",
        )
        .await;
        let newest = seed(
            "newest",
            None,
            "user",
            "user-a",
            "User wants drafts only and never sends messages or changes orders.",
            3,
            "extractor:llm",
        )
        .await;
        let unrelated = seed(
            "unrelated",
            None,
            "user",
            "user-a",
            "User works at Aquatiq in Bergen.",
            4,
            "extractor:llm",
        )
        .await;
        // Same wording, other partitions: never clustered with the above.
        seed(
            "other-owner",
            None,
            "user",
            "user-b",
            "User wants drafts only and never sends messages or changes orders.",
            5,
            "extractor:llm",
        )
        .await;
        seed(
            "thread-copy",
            Some(thread.clone()),
            "thread",
            "",
            "User wants drafts only and never sends messages or changes orders.",
            6,
            "extractor:llm",
        )
        .await;

        let (plan, _) = build_plan(&pool, None).await.unwrap();
        let mut clusters: Vec<PlanCluster> = plan
            .clusters
            .iter()
            .filter(|cluster| cluster.partition.org_id == org)
            .cloned()
            .collect();
        assert_eq!(clusters.len(), 1, "{clusters:#?}");
        let cluster = &mut clusters[0];
        assert_eq!(cluster.tier, Tier::Review);
        assert_eq!(cluster.survivor.id, stated, "the stated row survives");
        let retire: Vec<&str> = cluster.retire.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(retire, [newest.as_str(), middle.as_str()]);

        let state = |id: String| {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, (bool, Option<String>, DateTime<Utc>)>(
                    "SELECT is_latest, supersedes_id, updated_at FROM agent_memory WHERE id = $1",
                )
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };

        assert_eq!(
            apply_cluster(&pool, &plan.plan_id, cluster, false)
                .await
                .unwrap(),
            ApplyOutcome::NeedsReview
        );
        cluster.tier = Tier::Auto; // the reviewer approves it

        assert_eq!(
            apply_cluster(&pool, &plan.plan_id, cluster, true)
                .await
                .unwrap(),
            ApplyOutcome::WouldApply {
                retired: 2,
                linked: 2
            }
        );
        assert_eq!(
            state(newest.clone()).await,
            (true, None, at(3)),
            "a dry run changes nothing"
        );

        assert_eq!(
            apply_cluster(&pool, &plan.plan_id, cluster, false)
                .await
                .unwrap(),
            ApplyOutcome::Applied {
                retired: 2,
                linked: 2
            }
        );
        assert_eq!(
            state(stated.clone()).await,
            (true, Some(newest.clone()), at(1))
        );
        assert_eq!(
            state(newest.clone()).await,
            (false, Some(middle.clone()), at(3))
        );
        assert_eq!(state(middle.clone()).await, (false, None, at(2)));
        let listed: Vec<String> = crate::dreaming::list_user_memory(&pool, &org, "user-a", 50)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(listed, [unrelated.clone(), stated.clone()]);

        let (ledger_thread, ledger_text): (Option<String>, String) = sqlx::query_as(
            "SELECT thread_id, metadata::text FROM dream_runs \
             WHERE org_id = $1 AND trigger = 'memory_dedup_backfill' AND metadata ->> 'action' = 'apply'",
        )
        .bind(&org)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(ledger_thread.is_none());
        assert!(
            !ledger_text.contains("drafts"),
            "the ledger holds no content"
        );

        assert_eq!(
            apply_cluster(&pool, &plan.plan_id, cluster, false)
                .await
                .unwrap(),
            ApplyOutcome::AlreadyApplied
        );

        let outcomes = rollback_plan(&pool, &plan.plan_id, Some(&cluster.cluster_id))
            .await
            .unwrap();
        assert_eq!(
            outcomes,
            [(
                cluster.cluster_id.clone(),
                RollbackOutcome::RolledBack {
                    reverted: 3,
                    skipped: Vec::new()
                }
            )]
        );
        for (id, minute) in [(&stated, 1), (&newest, 3), (&middle, 2)] {
            assert_eq!(state(id.clone()).await, (true, None, at(minute)));
        }
        assert_eq!(
            rollback_plan(&pool, &plan.plan_id, Some(&cluster.cluster_id))
                .await
                .unwrap(),
            [(
                cluster.cluster_id.clone(),
                RollbackOutcome::AlreadyRolledBack
            )]
        );

        // Content that changed after review makes the cluster stale.
        sqlx::query("UPDATE agent_memory SET content = content || ' Always.' WHERE id = $1")
            .bind(&middle)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            apply_cluster(&pool, &plan.plan_id, cluster, false)
                .await
                .unwrap(),
            ApplyOutcome::Stale(vec![(middle.clone(), StaleReason::ContentChanged)])
        );
        assert!(
            state(newest).await.0,
            "a stale cluster is left exactly as it was"
        );
    }
}
