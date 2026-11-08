//! quarry-web-rerank-gate (QB-T14): the judged web-result set, the runner, the calibration fit and
//! the regression gate for the Data Plane's `POST /v1/rerank` (Quarry option B).
//!
//! The lab measures, on frozen data and with the production request, whether the paid rerank beats
//! the engine order. It is lab-only: labels never reach a request, and no output here feeds runtime
//! ranking. The labelling itself is an operator step (OPS-LABEL); until it is complete the gate
//! reports `pending` and never `pass`.
//!
//! Arms, per query, over the same frozen top-20 SearXNG candidates:
//!   A  engine order: the production order (the SearXNG fusion ranking)
//!   B  lexical overlap between the query and the candidate's title and snippet
//!   C  Data Plane rerank of the title and snippet: the production candidate text
//!   D  C plus the candidate's host in the text (diagnostic: recorded and reported, never gated)
//! A rerank call that is not `ok` falls back to engine order, as the production client does, and
//! every fallback is counted in the report.
//!
//! Metrics: nDCG@10 with gain 2^grade - 1 and a log2 discount, the ideal order taken from the query's
//! own judged pool. MRR and P@3 count a candidate as relevant at grade >= 2. noise@5 is the share of
//! the top five at grade 0. The comparison is paired over queries: a seeded bootstrap (10 000
//! resamples, 95 percent percentile interval) and the minimum detectable effect at 80 percent power.
//!
//! Acceptance, read from `web-rerank-baseline.json` (config-driven): nDCG@10 gain of C over A of at
//! least 0.04 with a 95 percent lower bound above zero; no category losing more than 0.03; added p95
//! at most 900 ms; a degraded rate of at most 2 percent over the 200-call soak; and a powered
//! comparison. Regression: the arm A and arm C nDCG@10 against the committed baseline, each within its
//! tolerance. The arm C tolerance is twice the sample SD of arm C across repeated live runs; arm A is
//! a deterministic function of the frozen SERP and labels, so its tolerance is zero.
//!
//! Exit codes: 0 pass, 1 fail, 2 pending or invalid input, 3 the Data Plane refused a live call or
//! broke its contract (the recording keeps every call made before the refusal).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const QUERIES_SCHEMA: &str = "quarry-web-rerank-queries-v1";
const BASELINE_SCHEMA: &str = "quarry-web-rerank-baseline-v1";
const REPORT_SCHEMA: &str = "quarry-web-rerank-report-v1";
const QUERIES_FILE: &str = "queries.v1.json";
const RESULTS_FILE: &str = "results.v1.jsonl";
const JUDGMENTS_FILE: &str = "judgments.v1.jsonl";
const RECORDING_FILE: &str = "provider-scores.v1.jsonl";
const BASELINE_FILE: &str = "web-rerank-baseline.json";
const FIXTURE_DIR: &str = "fixtures/web-rerank";
const ADJUDICATED: &str = "adjudicated";
const QUERY_COUNT: usize = 80;
const CANDIDATES: usize = 20;
const MAX_QUERY_CHARS: usize = 1024;
const RELEVANT_GRADE: u8 = 2;
const MAX_GRADE: u8 = 3;
const DCG_DEPTH: usize = 10;
const KAPPA_MINIMUM: f64 = 0.6;
const MIN_BASELINE_RUNS: usize = 3;
const WEIGHT_STEPS: usize = 20;
/// Production orders the head by the provider score alone (`apply_scores` in quarry-runtime's rerank.rs),
/// which is weight 1.0 in the blend `w * provider + (1 - w) * lexical`. Production has no 0.6 provider
/// weight; the 0.6 in the SearXNG settings is the engine weight of yandex.
const PRODUCTION_PROVIDER_WEIGHT: f64 = 1.0;
const RRF_K: f32 = 60.0;
const DEFAULT_SEARXNG_IMAGE: &str = "searxng/searxng:2026.7.12-c19d86faa";
const DP_PROFILE: &str = "web-v1";
const DP_DEADLINE_MS: u64 = 2000;
/// The production rerank head: QUARRY_EDGE__SEMANTIC_RERANK_TOP_N defaults to 10 in compose.
const DEFAULT_WINDOW: usize = 10;
const CALL_TIMEOUT: Duration = Duration::from_secs(12);
const FREEZE_PACING: Duration = Duration::from_millis(250);
const Z_975: f64 = 1.959_964;
const Z_80: f64 = 0.841_621;
const EPS: f64 = 1e-9;
const BASELINE_DESCRIPTION: &str = "Measured baseline of the web-rerank gate: arm means over repeated complete live runs. Tolerances: arm A is a deterministic function of the frozen SERP and labels, so its tolerance is zero; arm C is twice the sample SD of its nDCG@10 across the runs.";
const NOISE_METHOD: &str = "2 x sample SD of arm C nDCG@10 across repeated complete live runs";

/// Query categories with their total in the judged set and their smoke quota (the 10-query subset).
const CATEGORIES: [(&str, usize, usize); 6] = [
    ("navigational", 10, 1),
    ("factual", 20, 2),
    ("fresh_news", 15, 2),
    ("research_comparative", 15, 2),
    ("local", 10, 1),
    ("technical", 10, 2),
];
/// Language codes with their totals: 50 Norwegian and 30 English queries.
const LANGUAGES: [(&str, usize); 2] = [("no", 50), ("en", 30)];

const RUNBOOK: &str = r#"quarry-web-rerank-gate: judged web-result set, rerank runner, calibration and regression gate (QB-T14)

Build (the host cannot compile the Quarry crates: BoringSSL and libclang):
  cargo build -p quarry-evals --bin quarry-web-rerank-gate      (inside the Quarry-v2 build image)

Commands:
  check                          validate the fixtures and the baseline (exit 1 when invalid)
  freeze --searxng URL           freeze the top-20 SearXNG results per query. Refused once labels exist.
                                 --start N --count M freezes a batch (keep each run under ten minutes),
                                 --searxng-image TAG records the image (default: the compose tag).
  live --dp-url URL              paid calls to POST /v1/rerank. Arm C per query (80 calls); --arms C,D adds
                                 arm D (80 more, diagnostic); --soak N adds N sequential calls cycling the
                                 queries (200 for the soak). --smoke limits arms C and D to the 10 smoke
                                 queries. --window N is the rerank head (default 10: the production
                                 QUARRY_EDGE__SEMANTIC_RERANK_TOP_N); the tail keeps engine order. The window
                                 is recorded per call and pinned by the baseline. --run-id ID (1 to 16 letters
                                 or digits), --out FILE.
                                 Needs QUARRY_RERANK_BEARER: a web-rerank token (audience data-plane, scope
                                 data:rerank; row web-rerank in crates/quarry-runtime/contracts/
                                 cross-plane-token-requests.json). The token is never printed.
                                 http is accepted only on a loopback address.
  gate [--recording FILE]        evaluate one recording against the labels, the acceptance and the baseline.
                                 --smoke runs the 10-query check (offline, CI). --report-out FILE.
  write-baseline --recordings A,B,C
                                 pin the baseline from three or more repeated complete live runs.

Procedure. The live passes need no labels and can run first (they record provider scores only); write-baseline
and gate need complete labels (operator step OPS-LABEL):
  1. quarry-web-rerank-gate check
  2. run one live pass with --soak 200 and --run-id r1 --out provider-scores-r1.jsonl, then two more
     arm-C passes with --run-id r2 and r3 and their own --out files (no soak needed); keep --window identical in all three
  3. quarry-web-rerank-gate write-baseline --recordings provider-scores-r1.jsonl,provider-scores-r2.jsonl,provider-scores-r3.jsonl
  4. quarry-web-rerank-gate gate --recording provider-scores-r1.jsonl --report-out report-r1.json
  5. quarry-web-rerank-gate gate --smoke --recording provider-scores-r1.jsonl   (CI smoke, offline)
Pass: step 4 exits 0 with verdict pass; exit 2 means pending (a named input is missing), exit 1 a failed check.
"#;

// ----- command line -----

struct Args {
    command: String,
    flags: BTreeMap<String, String>,
    switches: BTreeSet<String>,
}

impl Args {
    fn parse(raw: &[String]) -> Result<Self, String> {
        let command = raw.first().cloned().unwrap_or_else(|| "help".to_owned());
        let mut flags = BTreeMap::new();
        let mut switches = BTreeSet::new();
        let mut position = 1;
        while position < raw.len() {
            let token = raw[position].as_str();
            let Some(name) = token.strip_prefix("--") else {
                return Err(format!("unexpected argument `{token}`"));
            };
            match raw.get(position + 1) {
                Some(next) if !next.starts_with("--") => {
                    flags.insert(name.to_owned(), next.clone());
                    position += 2;
                }
                _ => {
                    switches.insert(name.to_owned());
                    position += 1;
                }
            }
        }
        Ok(Self {
            command,
            flags,
            switches,
        })
    }

    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    fn number<T: FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.flag(name) {
            Some(value) => value
                .parse::<T>()
                .map_err(|_| format!("--{name} must be a whole number")),
            None => Ok(default),
        }
    }
}

pub fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&raw) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("quarry-web-rerank-gate: {message}");
            ExitCode::from(2)
        }
    }
}

fn dispatch(raw: &[String]) -> Result<u8, String> {
    let args = Args::parse(raw)?;
    match args.command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{RUNBOOK}");
            Ok(0)
        }
        "check" => check(&args),
        "freeze" => runtime()?.block_on(freeze(&args)),
        "live" => runtime()?.block_on(live(&args)),
        "gate" => gate_command(&args),
        "write-baseline" => write_baseline_command(&args),
        other => Err(format!("unknown command `{other}`; run `help`")),
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

fn eval_root() -> PathBuf {
    std::env::var_os("WEB_RERANK_EVAL_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn fixture_dir(args: &Args) -> PathBuf {
    args.flag("fixtures")
        .map(PathBuf::from)
        .unwrap_or_else(|| eval_root().join(FIXTURE_DIR))
}

fn baseline_path(args: &Args) -> PathBuf {
    args.flag("baseline")
        .map(PathBuf::from)
        .unwrap_or_else(|| eval_root().join(BASELINE_FILE))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

// ----- fixtures -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acceptance {
    pub min_ndcg_gain: f64,
    pub min_ci_lower: f64,
    pub category_max_drop: f64,
    pub added_p95_ms_max: f64,
    pub degraded_rate_max: f64,
    pub mde_max: f64,
    pub bootstrap_resamples: usize,
    pub bootstrap_seed: u64,
    pub soak_calls: usize,
}

impl Default for Acceptance {
    fn default() -> Self {
        Self {
            min_ndcg_gain: 0.04,
            min_ci_lower: 0.0,
            category_max_drop: 0.03,
            added_p95_ms_max: 900.0,
            degraded_rate_max: 0.02,
            mde_max: 0.04,
            bootstrap_resamples: 10_000,
            bootstrap_seed: 20_261_013,
            soak_calls: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueriesFile {
    pub schema: String,
    pub label_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<FreezeHeader>,
    pub queries: Vec<QueryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeHeader {
    pub searxng_image: String,
    pub frozen_at_unix: u64,
    pub count_param: u32,
    pub safesearch: u8,
    pub candidates_per_query: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEntry {
    pub id: String,
    pub lang: String,
    pub category: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<FrozenQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenQuery {
    pub raw_results: usize,
    pub unresponsive_engines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerpRow {
    pub query_id: String,
    pub rank: usize,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub engines: Vec<String>,
    #[serde(default)]
    pub positions: Vec<u32>,
    #[serde(default)]
    pub fusion_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentRow {
    pub query_id: String,
    pub url: String,
    pub judge: String,
    pub grade: u8,
}

#[derive(Debug, Clone)]
struct Fixtures {
    queries: QueriesFile,
    results: Vec<SerpRow>,
    judgments: Vec<JudgmentRow>,
    serp_digest: String,
    labels_digest: String,
}

impl Fixtures {
    fn load(dir: &Path) -> Result<Self, String> {
        let queries_text = read_text(&dir.join(QUERIES_FILE))?;
        let queries: QueriesFile = serde_json::from_str(&queries_text)
            .map_err(|error| format!("{QUERIES_FILE}: {error}"))?;
        let results = parse_jsonl::<SerpRow>(&read_text(&dir.join(RESULTS_FILE))?, RESULTS_FILE)?;
        let judgments =
            parse_jsonl::<JudgmentRow>(&read_text(&dir.join(JUDGMENTS_FILE))?, JUDGMENTS_FILE)?;
        let serp_digest = serp_digest(&queries.queries, &results);
        let labels_digest = labels_digest(&queries.label_state, &judgments);
        Ok(Self {
            queries,
            results,
            judgments,
            serp_digest,
            labels_digest,
        })
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn parse_jsonl<T: DeserializeOwned>(text: &str, name: &str) -> Result<Vec<T>, String> {
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row = serde_json::from_str::<T>(line)
            .map_err(|error| format!("{name} line {}: {error}", number + 1))?;
        rows.push(row);
    }
    Ok(rows)
}

fn write_jsonl<T: Serialize>(path: &Path, rows: &[T]) -> Result<(), String> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&serde_json::to_string(row).map_err(|error| error.to_string())?);
        text.push('\n');
    }
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn digest_of(canonical: &str) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(canonical.as_bytes()))
}

/// Digest of the request inputs (query ids and texts) and the frozen candidates. A recording is
/// bound to it, so a re-frozen SERP can never be scored with a recording made on the old one.
fn serp_digest(queries: &[QueryEntry], results: &[SerpRow]) -> String {
    let mut canonical = String::new();
    for query in queries {
        canonical.push_str(&format!("q\u{1}{}\u{1}{}\u{2}", query.id, query.text));
    }
    let mut rows: Vec<&SerpRow> = results.iter().collect();
    rows.sort_by(|a, b| a.query_id.cmp(&b.query_id).then(a.rank.cmp(&b.rank)));
    for row in rows {
        canonical.push_str(&format!(
            "r\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{2}",
            row.query_id, row.rank, row.url, row.title, row.snippet
        ));
    }
    digest_of(&canonical)
}

/// Digest of the label state and the judgments. The baseline pins it, because the metrics depend on it.
fn labels_digest(label_state: &str, judgments: &[JudgmentRow]) -> String {
    let mut rows: Vec<&JudgmentRow> = judgments.iter().collect();
    rows.sort_by(|a, b| (&a.query_id, &a.url, &a.judge).cmp(&(&b.query_id, &b.url, &b.judge)));
    let mut canonical = format!("state\u{1}{label_state}\u{2}");
    for row in rows {
        canonical.push_str(&format!(
            "j\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{2}",
            row.query_id, row.url, row.judge, row.grade
        ));
    }
    digest_of(&canonical)
}

fn valid_query_id(id: &str) -> bool {
    let length = id.chars().count();
    (3..=32).contains(&length)
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Structural rules that hold in every state, plus the label rules once labels are complete. A
/// pending label state must hold no judgment at all, so partial labels can never be scored.
fn validate_fixtures(fx: &Fixtures) -> Vec<String> {
    let mut problems = Vec::new();
    let file = &fx.queries;
    if file.schema != QUERIES_SCHEMA {
        problems.push(format!("{QUERIES_FILE}: schema must be {QUERIES_SCHEMA}"));
    }
    if file.label_state != "pending" && file.label_state != "complete" {
        problems.push(format!(
            "{QUERIES_FILE}: label_state must be pending or complete"
        ));
    }
    if file.queries.len() != QUERY_COUNT {
        problems.push(format!(
            "expected {QUERY_COUNT} queries, found {}",
            file.queries.len()
        ));
    }
    let mut ids = HashSet::new();
    for query in &file.queries {
        if !ids.insert(query.id.as_str()) {
            problems.push(format!("query id `{}` is repeated", query.id));
        }
        if !valid_query_id(&query.id) {
            problems.push(format!(
                "query id `{}` must be 3 to 32 lowercase letters, digits or hyphens",
                query.id
            ));
        }
        if !CATEGORIES
            .iter()
            .any(|(name, _, _)| *name == query.category)
        {
            problems.push(format!(
                "query `{}` has unknown category `{}`",
                query.id, query.category
            ));
        }
        if !LANGUAGES.iter().any(|(code, _)| *code == query.lang) {
            problems.push(format!(
                "query `{}` has unknown language `{}`",
                query.id, query.lang
            ));
        }
        let text = query.text.as_str();
        if text.is_empty() || text.trim() != text || text.chars().count() > MAX_QUERY_CHARS {
            problems.push(format!(
                "query `{}` must be trimmed, non-empty and at most {MAX_QUERY_CHARS} characters",
                query.id
            ));
        }
    }
    for (name, total, _) in CATEGORIES {
        let found = file.queries.iter().filter(|q| q.category == name).count();
        if found != total {
            problems.push(format!(
                "category `{name}` has {found} queries, need {total}"
            ));
        }
    }
    for (code, total) in LANGUAGES {
        let found = file.queries.iter().filter(|q| q.lang == code).count();
        if found != total {
            problems.push(format!(
                "language `{code}` has {found} queries, need {total}"
            ));
        }
    }
    let known: HashSet<&str> = file.queries.iter().map(|q| q.id.as_str()).collect();
    let mut by_query: BTreeMap<&str, Vec<&SerpRow>> = BTreeMap::new();
    for row in &fx.results {
        if !known.contains(row.query_id.as_str()) {
            problems.push(format!("result row for unknown query `{}`", row.query_id));
            continue;
        }
        by_query.entry(row.query_id.as_str()).or_default().push(row);
    }
    for query in &file.queries {
        let rows = by_query.get(query.id.as_str()).cloned().unwrap_or_default();
        if rows.len() != CANDIDATES {
            problems.push(format!(
                "query `{}` has {} candidates, need {CANDIDATES}",
                query.id,
                rows.len()
            ));
            continue;
        }
        let mut ranks: Vec<usize> = rows.iter().map(|row| row.rank).collect();
        ranks.sort_unstable();
        if ranks != (1..=CANDIDATES).collect::<Vec<usize>>() {
            problems.push(format!(
                "query `{}` ranks must be 1 to {CANDIDATES}",
                query.id
            ));
        }
        let mut urls = HashSet::new();
        for row in &rows {
            if !urls.insert(row.url.as_str()) {
                problems.push(format!("query `{}` repeats url `{}`", query.id, row.url));
            }
            if !(row.url.starts_with("https://") || row.url.starts_with("http://")) {
                problems.push(format!(
                    "query `{}` has a url that is not http(s)",
                    query.id
                ));
            }
        }
    }
    if file.label_state == "pending" && !fx.judgments.is_empty() {
        problems.push(format!(
            "{JUDGMENTS_FILE}: label_state is pending but judgments exist"
        ));
    }
    if file.label_state == "complete" {
        problems.extend(validate_labels(fx));
    }
    problems
}

/// The label protocol of a complete set: exactly two independent judges, each candidate judged by
/// both, disagreements adjudicated, and Cohen's kappa of at least 0.6 between the two judges.
fn validate_labels(fx: &Fixtures) -> Vec<String> {
    let mut problems = Vec::new();
    let candidates: HashSet<(&str, &str)> = fx
        .results
        .iter()
        .map(|row| (row.query_id.as_str(), row.url.as_str()))
        .collect();
    let mut judges: BTreeSet<&str> = BTreeSet::new();
    let mut independent: HashMap<(&str, &str), BTreeMap<&str, u8>> = HashMap::new();
    for row in &fx.judgments {
        if row.grade > MAX_GRADE {
            problems.push(format!(
                "judgment grade {} is outside 0 to {MAX_GRADE}",
                row.grade
            ));
        }
        if row.judge.trim().is_empty() {
            problems.push("a judgment has no judge name".to_owned());
        }
        if !candidates.contains(&(row.query_id.as_str(), row.url.as_str())) {
            problems.push(format!(
                "judged url `{}` is not in the results of `{}`",
                row.url, row.query_id
            ));
            continue;
        }
        if row.judge == ADJUDICATED {
            continue;
        }
        judges.insert(row.judge.as_str());
        let slot = independent
            .entry((row.query_id.as_str(), row.url.as_str()))
            .or_default();
        if slot.insert(row.judge.as_str(), row.grade).is_some() {
            problems.push(format!(
                "judge `{}` judged `{}` of `{}` twice",
                row.judge, row.url, row.query_id
            ));
        }
    }
    if judges.len() != 2 {
        problems.push(format!(
            "complete labels need exactly two independent judges, found {}",
            judges.len()
        ));
    }
    for row in &fx.results {
        let count = independent
            .get(&(row.query_id.as_str(), row.url.as_str()))
            .map_or(0, |map| map.len());
        if count != 2 {
            problems.push(format!(
                "candidate `{}` of `{}` has {count} independent judgments, need 2",
                row.url, row.query_id
            ));
        }
    }
    if let Err(message) = final_grades(&fx.judgments) {
        problems.push(message);
    }
    if judges.len() == 2 {
        let names: Vec<&str> = judges.iter().copied().collect();
        let mut pairs = Vec::new();
        for row in &fx.results {
            if let Some(map) = independent.get(&(row.query_id.as_str(), row.url.as_str())) {
                if let (Some(first), Some(second)) = (map.get(&names[0]), map.get(&names[1])) {
                    pairs.push((*first, *second));
                }
            }
        }
        match cohen_kappa(&pairs) {
            Some(kappa) if kappa >= KAPPA_MINIMUM => {}
            Some(kappa) => problems.push(format!(
                "judge agreement kappa {kappa:.3} is below {KAPPA_MINIMUM}"
            )),
            None => {
                problems.push("there are no judged pairs to measure judge agreement".to_owned())
            }
        }
    }
    problems
}

/// The grade of each candidate: the adjudicated grade when one exists, else the common grade of the
/// independent judges. Disagreement without an adjudication is an error, never a silent average.
fn final_grades(judgments: &[JudgmentRow]) -> Result<HashMap<(String, String), u8>, String> {
    let mut independent: BTreeMap<(String, String), Vec<u8>> = BTreeMap::new();
    let mut adjudicated: HashMap<(String, String), u8> = HashMap::new();
    for row in judgments {
        let key = (row.query_id.clone(), row.url.clone());
        if row.judge == ADJUDICATED {
            if adjudicated.insert(key, row.grade).is_some() {
                return Err(format!(
                    "candidate `{}` of `{}` is adjudicated twice",
                    row.url, row.query_id
                ));
            }
        } else {
            independent.entry(key).or_default().push(row.grade);
        }
    }
    let mut grades = HashMap::new();
    for (key, values) in independent {
        let agreed = values.iter().all(|value| *value == values[0]);
        let grade = match adjudicated.get(&key) {
            Some(grade) => *grade,
            None if agreed => values[0],
            None => {
                return Err(format!(
                    "candidate `{}` of `{}` has disagreeing judges and no adjudication",
                    key.1, key.0
                ));
            }
        };
        grades.insert(key, grade);
    }
    Ok(grades)
}

/// Unweighted Cohen's kappa over the grades 0 to 3. `None` when there are no pairs or a grade is out
/// of range; a degenerate table (both judges constant and equal) is perfect agreement.
fn cohen_kappa(pairs: &[(u8, u8)]) -> Option<f64> {
    if pairs.is_empty() || pairs.iter().any(|(a, b)| *a > MAX_GRADE || *b > MAX_GRADE) {
        return None;
    }
    let n = pairs.len() as f64;
    let agree = pairs.iter().filter(|(a, b)| a == b).count() as f64 / n;
    let mut first = [0f64; 4];
    let mut second = [0f64; 4];
    for (a, b) in pairs {
        first[usize::from(*a)] += 1.0;
        second[usize::from(*b)] += 1.0;
    }
    let expected: f64 = (0..4).map(|k| (first[k] / n) * (second[k] / n)).sum();
    if (1.0 - expected).abs() < 1e-12 {
        return Some(if (1.0 - agree).abs() < 1e-12 {
            1.0
        } else {
            0.0
        });
    }
    Some((agree - expected) / (1.0 - expected))
}

// ----- metrics -----

/// Gain of a grade: 2^grade - 1, so grades 0 to 3 gain 0, 1, 3 and 7.
fn gain(grade: u8) -> f64 {
    f64::from((1u32 << grade.min(MAX_GRADE)) - 1)
}

fn dcg(grades: &[u8], depth: usize) -> f64 {
    grades
        .iter()
        .take(depth)
        .enumerate()
        .map(|(position, grade)| gain(*grade) / ((position as f64) + 2.0).log2())
        .sum()
}

/// nDCG at `depth` for a ranking, against the ideal order of the query's whole judged pool. A pool
/// with no gain (no relevant candidate) scores zero.
fn ndcg_at(ranked: &[u8], pool: &[u8], depth: usize) -> f64 {
    let mut ideal = pool.to_vec();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let ideal_dcg = dcg(&ideal, depth);
    if ideal_dcg <= 0.0 {
        return 0.0;
    }
    dcg(ranked, depth) / ideal_dcg
}

fn reciprocal_rank(ranked: &[u8]) -> f64 {
    ranked
        .iter()
        .position(|grade| *grade >= RELEVANT_GRADE)
        .map_or(0.0, |position| 1.0 / (position as f64 + 1.0))
}

fn precision_at(ranked: &[u8], depth: usize) -> f64 {
    if depth == 0 {
        return 0.0;
    }
    let hits = ranked
        .iter()
        .take(depth)
        .filter(|grade| **grade >= RELEVANT_GRADE)
        .count();
    hits as f64 / depth as f64
}

fn noise_at(ranked: &[u8], depth: usize) -> f64 {
    if depth == 0 {
        return 0.0;
    }
    let noise = ranked
        .iter()
        .take(depth)
        .filter(|grade| **grade == 0)
        .count();
    noise as f64 / depth as f64
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmSummary {
    pub ndcg10: f64,
    pub mrr: f64,
    pub p_at_3: f64,
    pub noise_at_5: f64,
}

fn score_order(order: &[usize], grades: &[u8]) -> ArmSummary {
    let ranked: Vec<u8> = order.iter().map(|&index| grades[index]).collect();
    ArmSummary {
        ndcg10: ndcg_at(&ranked, grades, DCG_DEPTH),
        mrr: reciprocal_rank(&ranked),
        p_at_3: precision_at(&ranked, 3),
        noise_at_5: noise_at(&ranked, 5),
    }
}

// ----- statistics -----

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn sample_sd(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let center = mean(values);
    let variance = values
        .iter()
        .map(|value| (value - center).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    variance.sqrt()
}

/// SplitMix64: a small, fully specified generator, so every resample is reproducible from its seed.
#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Mean of the paired deltas with a percentile bootstrap interval over queries (95 percent).
fn paired_bootstrap(deltas: &[f64], resamples: usize, seed: u64) -> (f64, f64, f64) {
    let center = mean(deltas);
    if deltas.is_empty() || resamples == 0 {
        return (center, center, center);
    }
    let mut rng = SplitMix64::new(seed);
    let mut means = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut total = 0.0;
        for _ in 0..deltas.len() {
            total += deltas[rng.below(deltas.len())];
        }
        means.push(total / deltas.len() as f64);
    }
    means.sort_by(|a, b| a.total_cmp(b));
    let last = (resamples - 1) as f64;
    let low = means[(last * 0.025).round() as usize];
    let high = means[(last * 0.975).round() as usize];
    (center, low, high)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Power {
    pub sd_delta: f64,
    pub mde_80: f64,
}

/// Minimum detectable mean difference at 80 percent power, two-sided 5 percent, from the paired SD.
fn power(deltas: &[f64]) -> Power {
    let sd = sample_sd(deltas);
    let mde = if deltas.is_empty() {
        f64::INFINITY
    } else {
        (Z_975 + Z_80) * sd / (deltas.len() as f64).sqrt()
    };
    Power {
        sd_delta: sd,
        mde_80: mde,
    }
}

/// The 95th percentile by the nearest-rank method.
fn p95_nearest_rank(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = (0.95 * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

// ----- candidate text and lexical arm -----

fn tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_owned)
        .collect()
}

/// Share of the query's distinct tokens that also occur in the document.
fn lexical_score(query: &str, document: &str) -> f64 {
    let query_tokens = tokens(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let document_tokens = tokens(document);
    let shared = query_tokens
        .iter()
        .filter(|token| document_tokens.contains(*token))
        .count();
    shared as f64 / query_tokens.len() as f64
}

/// The lower-case host without userinfo, port or a leading `www.`.
fn host_of(url: &str) -> String {
    let trimmed = url.trim();
    let rest = trimmed.split_once("://").map_or(trimmed, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = host_port
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_owned()
}

/// The text the cross-encoder reads for one candidate. This is the production candidate text: the
/// title and snippet, or whichever of them is present, or the address. Arm D adds the host.
fn candidate_text(row: &SerpRow, with_host: bool) -> String {
    let title = row.title.trim();
    let snippet = row.snippet.trim();
    let mut text = match (title.is_empty(), snippet.is_empty()) {
        (false, false) => format!("{title}\n{snippet}"),
        (false, true) => title.to_owned(),
        (true, false) => snippet.to_owned(),
        (true, true) => row.url.trim().to_owned(),
    };
    if with_host {
        let host = host_of(&row.url);
        if !host.is_empty() {
            text.push('\n');
            text.push_str(&host);
        }
    }
    text
}

// ----- rankings -----

/// Descending by score; equal scores keep the lower candidate index (the engine order).
fn rank_by_score(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
    order
}

/// Scored candidates first in descending score, then the unscored ones in engine order.
fn rank_scored(scores: &[Option<f64>]) -> Vec<usize> {
    let mut scored: Vec<usize> = (0..scores.len())
        .filter(|index| scores[*index].is_some())
        .collect();
    scored.sort_by(|a, b| {
        scores[*b]
            .unwrap_or(0.0)
            .total_cmp(&scores[*a].unwrap_or(0.0))
            .then(a.cmp(b))
    });
    let unscored = (0..scores.len()).filter(|index| scores[*index].is_none());
    scored.into_iter().chain(unscored).collect()
}

/// The provider order of one call, and whether the query fell back to engine order.
fn provider_order(call: &RecordedCall, candidates: usize) -> (Vec<usize>, bool) {
    if call.status != "ok" {
        return ((0..candidates).collect(), true);
    }
    let mut scores: Vec<Option<f64>> = vec![None; candidates];
    for entry in &call.results {
        if entry.index < candidates {
            scores[entry.index] = entry.score;
        }
    }
    (rank_scored(&scores), false)
}

/// The smoke subset: the first queries of each category in file order, by the category quotas.
fn smoke_queries(all: &[QueryEntry]) -> Vec<&QueryEntry> {
    let mut taken: HashMap<&str, usize> = HashMap::new();
    let mut picked = Vec::new();
    for query in all {
        let quota = CATEGORIES
            .iter()
            .find(|(name, _, _)| *name == query.category)
            .map_or(0, |(_, _, quota)| *quota);
        let used = taken.entry(query.category.as_str()).or_insert(0);
        if *used < quota {
            *used += 1;
            picked.push(query);
        }
    }
    picked
}

// ----- recordings -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedEntry {
    pub index: usize,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedCall {
    pub arm: String,
    pub query_id: String,
    pub request_id: String,
    pub serp_digest: String,
    /// How many leading candidates were sent (the production head: QUARRY_EDGE__SEMANTIC_RERANK_TOP_N,
    /// default 10). The tail keeps engine order, as in production.
    pub window: usize,
    pub http_status: u16,
    pub status: String,
    pub reason: Option<String>,
    pub latency_ms: f64,
    pub results: Vec<RecordedEntry>,
}

fn read_recording(path: &Path) -> Result<Vec<RecordedCall>, String> {
    parse_jsonl::<RecordedCall>(&read_text(path)?, &path.display().to_string())
}

struct CallIndex<'a> {
    arms: HashMap<(&'a str, &'a str), &'a RecordedCall>,
    soak: Vec<&'a RecordedCall>,
    window: Option<usize>,
}

/// Indexes one recording. Every row must carry the SERP digest of the fixtures, every arm call must be
/// unique per query, and an unknown arm is refused.
fn index_calls<'a>(calls: &'a [RecordedCall], serp: &str) -> Result<CallIndex<'a>, String> {
    let mut index = CallIndex {
        arms: HashMap::new(),
        soak: Vec::new(),
        window: None,
    };
    for call in calls {
        if call.serp_digest != serp {
            return Err(format!(
                "the recording of `{}` was made on a different SERP: run live again",
                call.request_id
            ));
        }
        match index.window {
            None => index.window = Some(call.window),
            Some(window) if window != call.window => {
                return Err(format!(
                    "the recording mixes rerank windows ({window} and {}): one window per recording",
                    call.window
                ));
            }
            Some(_) => {}
        }
        match call.arm.as_str() {
            "soak" => index.soak.push(call),
            "C" | "D" => {
                if index
                    .arms
                    .insert((call.arm.as_str(), call.query_id.as_str()), call)
                    .is_some()
                {
                    return Err(format!(
                        "duplicate arm {} call for query `{}`",
                        call.arm, call.query_id
                    ));
                }
            }
            other => return Err(format!("unknown arm `{other}` in the recording")),
        }
    }
    Ok(index)
}

// ----- evaluation -----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEval {
    pub id: String,
    pub category: String,
    pub lang: String,
    pub arms: BTreeMap<String, ArmSummary>,
    pub c_fallback: bool,
}

/// One ok call of the provider arm, for the weight fit. The blend reorders only the head, as production
/// does: the tail (candidates past `window`) keeps engine order beneath it.
#[derive(Debug, Clone)]
struct WeightCase {
    window: usize,
    provider: Vec<f64>,
    lexical: Vec<f64>,
    grades: Vec<u8>,
}

impl WeightCase {
    fn ndcg_at_weight(&self, weight: f64) -> f64 {
        let head = self.window.min(self.grades.len());
        let blended: Vec<f64> = (0..head)
            .map(|index| weight * self.provider[index] + (1.0 - weight) * self.lexical[index])
            .collect();
        let mut order = rank_by_score(&blended);
        order.extend(head..self.grades.len());
        let ranked: Vec<u8> = order.iter().map(|&index| self.grades[index]).collect();
        ndcg_at(&ranked, &self.grades, DCG_DEPTH)
    }
}

struct Bundle {
    evals: Vec<QueryEval>,
    latencies: Vec<f64>,
    pairs: Vec<(f64, f64)>,
    cases: Vec<WeightCase>,
}

enum Stop {
    Pending(String),
    Invalid(String),
}

fn stop_message(stop: Stop) -> String {
    match stop {
        Stop::Pending(message) => format!("pending: {message}"),
        Stop::Invalid(message) => message,
    }
}

fn evaluate(
    fx: &Fixtures,
    grades: &HashMap<(String, String), u8>,
    index: &CallIndex,
    selected: &[&QueryEntry],
) -> Result<Bundle, Stop> {
    let mut rows_by_query: HashMap<&str, Vec<&SerpRow>> = HashMap::new();
    for row in &fx.results {
        rows_by_query
            .entry(row.query_id.as_str())
            .or_default()
            .push(row);
    }
    let with_d = selected
        .iter()
        .filter(|query| index.arms.contains_key(&("D", query.id.as_str())))
        .count();
    if with_d != 0 && with_d != selected.len() {
        return Err(Stop::Invalid(
            "arm D is recorded for some queries but not all".to_owned(),
        ));
    }
    let mut bundle = Bundle {
        evals: Vec::with_capacity(selected.len()),
        latencies: Vec::with_capacity(selected.len()),
        pairs: Vec::new(),
        cases: Vec::new(),
    };
    for query in selected {
        let mut rows = rows_by_query
            .get(query.id.as_str())
            .cloned()
            .unwrap_or_default();
        rows.sort_by_key(|row| row.rank);
        if rows.len() != CANDIDATES {
            return Err(Stop::Invalid(format!(
                "query `{}` has {} candidates, need {CANDIDATES}",
                query.id,
                rows.len()
            )));
        }
        let mut judged = Vec::with_capacity(CANDIDATES);
        for row in &rows {
            match grades.get(&(query.id.clone(), row.url.clone())) {
                Some(grade) => judged.push(*grade),
                None => {
                    return Err(Stop::Invalid(format!(
                        "candidate `{}` of `{}` is ungraded",
                        row.url, query.id
                    )))
                }
            }
        }
        let c_call = match index.arms.get(&("C", query.id.as_str())) {
            Some(call) => *call,
            None => {
                return Err(Stop::Pending(format!(
                    "no arm C call is recorded for `{}`",
                    query.id
                )))
            }
        };
        let engine: Vec<usize> = (0..CANDIDATES).collect();
        let lexical_scores: Vec<f64> = rows
            .iter()
            .map(|row| lexical_score(&query.text, &candidate_text(row, false)))
            .collect();
        let lexical = rank_by_score(&lexical_scores);
        let (c_order, c_fallback) = provider_order(c_call, CANDIDATES);
        let mut arms = BTreeMap::new();
        arms.insert("A".to_owned(), score_order(&engine, &judged));
        arms.insert("B".to_owned(), score_order(&lexical, &judged));
        arms.insert("C".to_owned(), score_order(&c_order, &judged));
        if with_d > 0 {
            let d_call = match index.arms.get(&("D", query.id.as_str())) {
                Some(call) => *call,
                None => {
                    return Err(Stop::Invalid(format!(
                        "arm D is missing for `{}`",
                        query.id
                    )))
                }
            };
            let (d_order, _) = provider_order(d_call, CANDIDATES);
            arms.insert("D".to_owned(), score_order(&d_order, &judged));
        }
        bundle.latencies.push(c_call.latency_ms);
        if !c_fallback {
            let mut provider = vec![0.0; CANDIDATES];
            for entry in &c_call.results {
                if let Some(score) = entry.score {
                    if entry.index < CANDIDATES {
                        provider[entry.index] = score;
                        let relevant = if judged[entry.index] >= RELEVANT_GRADE {
                            1.0
                        } else {
                            0.0
                        };
                        bundle.pairs.push((score, relevant));
                    }
                }
            }
            bundle.cases.push(WeightCase {
                window: index.window.unwrap_or(CANDIDATES).min(CANDIDATES),
                provider,
                lexical: lexical_scores.clone(),
                grades: judged.clone(),
            });
        }
        bundle.evals.push(QueryEval {
            id: query.id.clone(),
            category: query.category.clone(),
            lang: query.lang.clone(),
            arms,
            c_fallback,
        });
    }
    Ok(bundle)
}

// ----- calibration -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Knot {
    pub score: f64,
    pub relevance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightPoint {
    pub weight: f64,
    pub mean_ndcg10: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightFit {
    pub production_weight: f64,
    pub best_weight: f64,
    pub best_mean_ndcg10: f64,
    pub curve: Vec<WeightPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub fit_pairs: usize,
    pub knots: Vec<Knot>,
    pub provider_weight: WeightFit,
}

#[derive(Debug, Clone)]
struct Block {
    sum_x: f64,
    sum_y: f64,
    count: f64,
}

impl Block {
    fn mean_y(&self) -> f64 {
        self.sum_y / self.count
    }

    fn merge(self, other: Block) -> Block {
        Block {
            sum_x: self.sum_x + other.sum_x,
            sum_y: self.sum_y + other.sum_y,
            count: self.count + other.count,
        }
    }
}

/// Isotonic regression of the relevance indicator on the provider score (pool adjacent violators).
/// The knots are block averages, so the fitted relevance never decreases as the score rises.
fn fit_knots(pairs: &[(f64, f64)]) -> Vec<Knot> {
    let mut sorted = pairs.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut initial: Vec<Block> = Vec::new();
    let mut last_x: Option<f64> = None;
    for (x, y) in sorted {
        let merge = last_x == Some(x) && !initial.is_empty();
        if merge {
            if let Some(block) = initial.last_mut() {
                block.sum_x += x;
                block.sum_y += y;
                block.count += 1.0;
            }
        } else {
            initial.push(Block {
                sum_x: x,
                sum_y: y,
                count: 1.0,
            });
        }
        last_x = Some(x);
    }
    let mut stack: Vec<Block> = Vec::new();
    for block in initial {
        let mut current = block;
        while let Some(top) = stack.last() {
            if top.mean_y() > current.mean_y() {
                let previous = stack.pop().expect("the top block exists");
                current = previous.merge(current);
            } else {
                break;
            }
        }
        stack.push(current);
    }
    stack
        .iter()
        .map(|block| Knot {
            score: block.sum_x / block.count,
            relevance: block.mean_y(),
        })
        .collect()
}

/// Piecewise-linear lookup of a knot table, clamped at both ends. `None` for an empty table. The
/// consumer of the committed table is QB-T12; the lab itself only checks the lookup.
#[allow(dead_code)]
fn calibrate(knots: &[Knot], score: f64) -> Option<f64> {
    let first = knots.first()?;
    let last = knots.last()?;
    if score <= first.score {
        return Some(first.relevance);
    }
    if score >= last.score {
        return Some(last.relevance);
    }
    for pair in knots.windows(2) {
        let (lower, upper) = (&pair[0], &pair[1]);
        if score >= lower.score && score <= upper.score {
            let span = upper.score - lower.score;
            if span <= 0.0 {
                return Some(upper.relevance);
            }
            let t = (score - lower.score) / span;
            return Some(lower.relevance + t * (upper.relevance - lower.relevance));
        }
    }
    Some(last.relevance)
}

/// The provider weight that maximises the mean nDCG@10 of the blend `w * provider + (1 - w) * lexical`.
/// Ties go to the smallest weight (the least reliance on the provider).
fn fit_weight(cases: &[WeightCase]) -> WeightFit {
    let mut curve = Vec::with_capacity(WEIGHT_STEPS + 1);
    let mut best: Option<(f64, f64)> = None;
    for step in 0..=WEIGHT_STEPS {
        let weight = step as f64 / WEIGHT_STEPS as f64;
        let values: Vec<f64> = cases
            .iter()
            .map(|case| case.ndcg_at_weight(weight))
            .collect();
        let average = mean(&values);
        curve.push(WeightPoint {
            weight,
            mean_ndcg10: average,
        });
        let better = match best {
            None => true,
            Some((_, top)) => average > top + EPS,
        };
        if better {
            best = Some((weight, average));
        }
    }
    let (best_weight, best_mean) = best.unwrap_or((PRODUCTION_PROVIDER_WEIGHT, 0.0));
    WeightFit {
        production_weight: PRODUCTION_PROVIDER_WEIGHT,
        best_weight,
        best_mean_ndcg10: best_mean,
        curve,
    }
}

// ----- gate -----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub value: Option<f64>,
    pub threshold: f64,
    pub rule: String,
    pub pass: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    pub state: String,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRow {
    pub queries: usize,
    pub ndcg10_a: f64,
    pub ndcg10_c: f64,
    pub delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paired {
    pub queries: usize,
    pub mean_delta: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
    pub resamples: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latency {
    pub calls: usize,
    pub p95_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Soak {
    pub calls: usize,
    pub not_ok: usize,
    pub not_ok_rate: f64,
    pub by_outcome: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReport {
    pub schema: String,
    pub mode: String,
    pub verdict: String,
    pub reasons: Vec<String>,
    pub label_state: String,
    pub queries: usize,
    pub serp_digest: String,
    pub labels_digest: String,
    pub provider_requests: usize,
    pub window: Option<usize>,
    pub arms: BTreeMap<String, ArmSummary>,
    pub per_category: BTreeMap<String, CategoryRow>,
    pub paired: Option<Paired>,
    pub power: Option<Power>,
    pub latency: Option<Latency>,
    pub soak: Option<Soak>,
    pub c_fallback_queries: usize,
    pub checks: Vec<Check>,
    pub regression: Regression,
    pub calibration: Option<Calibration>,
    pub freeze: Option<FreezeProvenance>,
}

/// The provenance of the frozen SERP, stated in every report: the image and time of the freeze, which
/// engines contributed rows, and on how many queries each engine did not answer at freeze time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreezeProvenance {
    pub searxng_image: String,
    pub frozen_at_unix: u64,
    pub engines_seen: BTreeMap<String, usize>,
    pub unresponsive_queries: BTreeMap<String, usize>,
}

fn freeze_provenance(fx: &Fixtures) -> Option<FreezeProvenance> {
    let header = fx.queries.freeze.as_ref()?;
    let mut engines_seen: BTreeMap<String, usize> = BTreeMap::new();
    for row in &fx.results {
        for engine in &row.engines {
            *engines_seen.entry(engine.clone()).or_insert(0) += 1;
        }
    }
    let mut unresponsive_queries: BTreeMap<String, usize> = BTreeMap::new();
    for query in &fx.queries.queries {
        if let Some(frozen) = &query.frozen {
            for engine in &frozen.unresponsive_engines {
                *unresponsive_queries.entry(engine.clone()).or_insert(0) += 1;
            }
        }
    }
    Some(FreezeProvenance {
        searxng_image: header.searxng_image.clone(),
        frozen_at_unix: header.frozen_at_unix,
        engines_seen,
        unresponsive_queries,
    })
}

fn gate_skeleton(mode: &str, fx: &Fixtures) -> GateReport {
    GateReport {
        schema: REPORT_SCHEMA.to_owned(),
        mode: mode.to_owned(),
        verdict: String::new(),
        reasons: Vec::new(),
        label_state: fx.queries.label_state.clone(),
        queries: 0,
        serp_digest: fx.serp_digest.clone(),
        labels_digest: fx.labels_digest.clone(),
        provider_requests: 0,
        window: None,
        arms: BTreeMap::new(),
        per_category: BTreeMap::new(),
        paired: None,
        power: None,
        latency: None,
        soak: None,
        c_fallback_queries: 0,
        checks: Vec::new(),
        regression: Regression {
            state: "not_evaluated".to_owned(),
            checks: Vec::new(),
        },
        calibration: None,
        freeze: freeze_provenance(fx),
    }
}

fn finish(mut report: GateReport, verdict: &str, reasons: Vec<String>) -> GateReport {
    report.verdict = verdict.to_owned();
    report.reasons = reasons;
    report
}

fn mean_arms(evals: &[QueryEval]) -> BTreeMap<String, ArmSummary> {
    let mut out = BTreeMap::new();
    let Some(first) = evals.first() else {
        return out;
    };
    for arm in first.arms.keys() {
        let rows: Vec<&ArmSummary> = evals.iter().filter_map(|eval| eval.arms.get(arm)).collect();
        if rows.len() != evals.len() {
            continue;
        }
        let n = rows.len() as f64;
        out.insert(
            arm.clone(),
            ArmSummary {
                ndcg10: rows.iter().map(|row| row.ndcg10).sum::<f64>() / n,
                mrr: rows.iter().map(|row| row.mrr).sum::<f64>() / n,
                p_at_3: rows.iter().map(|row| row.p_at_3).sum::<f64>() / n,
                noise_at_5: rows.iter().map(|row| row.noise_at_5).sum::<f64>() / n,
            },
        );
    }
    out
}

fn category_rows(evals: &[QueryEval]) -> BTreeMap<String, CategoryRow> {
    let mut out = BTreeMap::new();
    let names: BTreeSet<&str> = evals.iter().map(|eval| eval.category.as_str()).collect();
    for name in names {
        let rows: Vec<&QueryEval> = evals.iter().filter(|eval| eval.category == name).collect();
        let a: Vec<f64> = rows
            .iter()
            .filter_map(|eval| eval.arms.get("A").map(|arm| arm.ndcg10))
            .collect();
        let c: Vec<f64> = rows
            .iter()
            .filter_map(|eval| eval.arms.get("C").map(|arm| arm.ndcg10))
            .collect();
        let (ndcg_a, ndcg_c) = (mean(&a), mean(&c));
        out.insert(
            name.to_owned(),
            CategoryRow {
                queries: rows.len(),
                ndcg10_a: ndcg_a,
                ndcg10_c: ndcg_c,
                delta: ndcg_c - ndcg_a,
            },
        );
    }
    out
}

fn paired_deltas(evals: &[QueryEval]) -> Vec<f64> {
    evals
        .iter()
        .filter_map(|eval| Some(eval.arms.get("C")?.ndcg10 - eval.arms.get("A")?.ndcg10))
        .collect()
}

fn soak_summary(calls: &[&RecordedCall]) -> Option<Soak> {
    if calls.is_empty() {
        return None;
    }
    let mut by_outcome: BTreeMap<String, usize> = BTreeMap::new();
    let mut not_ok = 0usize;
    for call in calls {
        if call.status != "ok" {
            not_ok += 1;
        }
        let key = match &call.reason {
            Some(reason) => format!("{}:{}", call.status, reason),
            None => call.status.clone(),
        };
        *by_outcome.entry(key).or_insert(0) += 1;
    }
    Some(Soak {
        calls: calls.len(),
        not_ok,
        not_ok_rate: not_ok as f64 / calls.len() as f64,
        by_outcome,
    })
}

fn acceptance_checks(report: &GateReport, acceptance: &Acceptance) -> Vec<Check> {
    let mut checks = Vec::new();
    let delta = report.paired.as_ref().map(|paired| paired.mean_delta);
    checks.push(Check {
        name: "ndcg10_gain_c_over_a".to_owned(),
        value: delta,
        threshold: acceptance.min_ndcg_gain,
        rule: ">=".to_owned(),
        pass: delta.map(|value| value >= acceptance.min_ndcg_gain),
    });
    let low = report.paired.as_ref().map(|paired| paired.ci95_low);
    checks.push(Check {
        name: "ci95_lower_bound".to_owned(),
        value: low,
        threshold: acceptance.min_ci_lower,
        rule: ">".to_owned(),
        pass: low.map(|value| value > acceptance.min_ci_lower),
    });
    let worst = report
        .per_category
        .values()
        .map(|row| row.delta)
        .reduce(f64::min);
    checks.push(Check {
        name: "worst_category_delta".to_owned(),
        value: worst,
        threshold: -acceptance.category_max_drop,
        rule: ">=".to_owned(),
        pass: worst.map(|value| value >= -acceptance.category_max_drop),
    });
    let p95 = report.latency.as_ref().map(|latency| latency.p95_ms);
    checks.push(Check {
        name: "added_p95_ms".to_owned(),
        value: p95,
        threshold: acceptance.added_p95_ms_max,
        rule: "<=".to_owned(),
        pass: p95.map(|value| value <= acceptance.added_p95_ms_max),
    });
    let soak_ready = report
        .soak
        .as_ref()
        .is_some_and(|soak| soak.calls >= acceptance.soak_calls);
    let rate = report.soak.as_ref().map(|soak| soak.not_ok_rate);
    checks.push(Check {
        name: "soak_not_ok_rate".to_owned(),
        value: rate,
        threshold: acceptance.degraded_rate_max,
        rule: "<=".to_owned(),
        pass: if soak_ready {
            rate.map(|value| value <= acceptance.degraded_rate_max)
        } else {
            None
        },
    });
    let mde = report.power.as_ref().map(|power| power.mde_80);
    checks.push(Check {
        name: "minimum_detectable_effect".to_owned(),
        value: mde,
        threshold: acceptance.mde_max,
        rule: "<=".to_owned(),
        pass: mde.map(|value| value <= acceptance.mde_max),
    });
    checks
}

fn baseline_is_stale(base: &BaselineFile, fx: &Fixtures) -> bool {
    base.state == "measured"
        && (base.serp_digest.as_deref() != Some(fx.serp_digest.as_str())
            || base.labels_digest.as_deref() != Some(fx.labels_digest.as_str()))
}

fn regression_check(
    report: &GateReport,
    baseline: Option<&BaselineFile>,
    fx: &Fixtures,
) -> Regression {
    let unbaselined = || Regression {
        state: "unbaselined".to_owned(),
        checks: Vec::new(),
    };
    let Some(base) = baseline else {
        return unbaselined();
    };
    if base.state != "measured" {
        return unbaselined();
    }
    if baseline_is_stale(base, fx) {
        return Regression {
            state: "fixtures_changed".to_owned(),
            checks: Vec::new(),
        };
    }
    if base.window != report.window {
        return Regression {
            state: "window_changed".to_owned(),
            checks: Vec::new(),
        };
    }
    let (Some(metrics), Some(tolerances)) = (&base.metrics, &base.tolerances) else {
        return Regression {
            state: "invalid".to_owned(),
            checks: Vec::new(),
        };
    };
    let mut checks = Vec::new();
    for (arm, tolerance) in [
        ("A", tolerances.ndcg10_arm_a),
        ("C", tolerances.ndcg10_arm_c),
    ] {
        let current = report.arms.get(arm).map(|summary| summary.ndcg10);
        let reference = metrics.get(arm).map(|summary| summary.ndcg10);
        let pass = match (current, reference) {
            (Some(value), Some(base_value)) => Some(value >= base_value - tolerance - EPS),
            _ => None,
        };
        checks.push(Check {
            name: format!("ndcg10_arm_{}_vs_baseline", arm.to_lowercase()),
            value: current,
            threshold: reference.map_or(0.0, |base_value| base_value - tolerance),
            rule: ">=".to_owned(),
            pass,
        });
    }
    let state = if checks.iter().any(|check| check.pass == Some(false)) {
        "fail"
    } else if checks.iter().all(|check| check.pass == Some(true)) {
        "pass"
    } else {
        "incomplete"
    };
    Regression {
        state: state.to_owned(),
        checks,
    }
}

fn decide(report: &GateReport) -> (String, Vec<String>) {
    let mut fails = Vec::new();
    let mut pending = Vec::new();
    for check in &report.checks {
        match check.pass {
            Some(false) => fails.push(format!(
                "check {} failed: value {:?} must be {} {}",
                check.name, check.value, check.rule, check.threshold
            )),
            None => pending.push(format!("check {} has no value yet", check.name)),
            Some(true) => {}
        }
    }
    match report.regression.state.as_str() {
        "fail" => fails.push(
            "regression: an arm nDCG@10 fell below the committed baseline minus its tolerance"
                .to_owned(),
        ),
        "fixtures_changed" => fails.push(
            "regression: the baseline was measured on other fixtures (write-baseline again)"
                .to_owned(),
        ),
        "window_changed" => fails.push(
            "regression: the rerank window differs from the baseline's window (a different production head)"
                .to_owned(),
        ),
        "invalid" => fails.push("regression: the baseline is missing its metrics".to_owned()),
        "unbaselined" => pending.push(
            "no measured baseline yet: write-baseline needs three repeated live runs".to_owned(),
        ),
        "incomplete" => pending.push("the regression check is incomplete".to_owned()),
        _ => {}
    }
    if !fails.is_empty() {
        ("fail".to_owned(), fails)
    } else if !pending.is_empty() {
        ("pending".to_owned(), pending)
    } else {
        ("pass".to_owned(), Vec::new())
    }
}

/// Evaluates one recording. `smoke` runs the 10-query subset and passes when every one of its
/// rerank calls was `ok`; a full run applies the acceptance, the regression and the calibration.
fn run_gate(
    fx: &Fixtures,
    recording: Option<&[RecordedCall]>,
    baseline: Option<&BaselineFile>,
    smoke: bool,
) -> GateReport {
    let mode = if smoke { "smoke" } else { "full" };
    let report = gate_skeleton(mode, fx);
    let mut problems = validate_fixtures(fx);
    if let Some(base) = baseline {
        problems.extend(baseline_problems(base));
    }
    if !problems.is_empty() {
        return finish(report, "invalid", problems);
    }
    if fx.queries.label_state != "complete" {
        return finish(
            report,
            "pending",
            vec![format!(
                "labels are pending: {JUDGMENTS_FILE} holds no judgments (operator step OPS-LABEL)"
            )],
        );
    }
    let Some(calls) = recording else {
        return finish(
            report,
            "pending",
            vec!["no provider recording: run the live subcommand first".to_owned()],
        );
    };
    let index = match index_calls(calls, &fx.serp_digest) {
        Ok(index) => index,
        Err(message) => return finish(report, "invalid", vec![message]),
    };
    let grades = match final_grades(&fx.judgments) {
        Ok(grades) => grades,
        Err(message) => return finish(report, "invalid", vec![message]),
    };
    let all: Vec<&QueryEntry> = fx.queries.queries.iter().collect();
    let selected: Vec<&QueryEntry> = if smoke {
        smoke_queries(&fx.queries.queries)
    } else {
        all
    };
    let mut report = report;
    report.provider_requests = calls.len();
    report.window = index.window;
    let bundle = match evaluate(fx, &grades, &index, &selected) {
        Ok(bundle) => bundle,
        Err(Stop::Pending(message)) => return finish(report, "pending", vec![message]),
        Err(Stop::Invalid(message)) => return finish(report, "invalid", vec![message]),
    };
    report.queries = bundle.evals.len();
    report.c_fallback_queries = bundle.evals.iter().filter(|eval| eval.c_fallback).count();
    report.arms = mean_arms(&bundle.evals);
    report.per_category = category_rows(&bundle.evals);
    if smoke {
        let fallbacks = report.c_fallback_queries;
        return if fallbacks == 0 {
            finish(report, "pass", Vec::new())
        } else {
            finish(
                report,
                "fail",
                vec![format!(
                    "{fallbacks} smoke queries fell back to engine order"
                )],
            )
        };
    }
    let acceptance = baseline
        .map(|base| base.acceptance.clone())
        .unwrap_or_default();
    let deltas = paired_deltas(&bundle.evals);
    let (center, low, high) = paired_bootstrap(
        &deltas,
        acceptance.bootstrap_resamples,
        acceptance.bootstrap_seed,
    );
    report.paired = Some(Paired {
        queries: deltas.len(),
        mean_delta: center,
        ci95_low: low,
        ci95_high: high,
        resamples: acceptance.bootstrap_resamples,
        seed: acceptance.bootstrap_seed,
    });
    report.power = Some(power(&deltas));
    let mut latencies = bundle.latencies.clone();
    latencies.extend(index.soak.iter().map(|call| call.latency_ms));
    report.latency = p95_nearest_rank(&latencies).map(|p95| Latency {
        calls: latencies.len(),
        p95_ms: p95,
    });
    report.soak = soak_summary(&index.soak);
    report.calibration = Some(Calibration {
        fit_pairs: bundle.pairs.len(),
        knots: fit_knots(&bundle.pairs),
        provider_weight: fit_weight(&bundle.cases),
    });
    report.checks = acceptance_checks(&report, &acceptance);
    report.regression = regression_check(&report, baseline, fx);
    let (verdict, reasons) = decide(&report);
    finish(report, &verdict, reasons)
}

// ----- baseline -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tolerances {
    pub ndcg10_arm_a: f64,
    pub ndcg10_arm_c: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Noise {
    pub method: String,
    pub runs: Vec<f64>,
    pub sd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineFile {
    pub schema: String,
    pub state: String,
    pub description: String,
    pub serp_digest: Option<String>,
    pub labels_digest: Option<String>,
    pub window: Option<usize>,
    pub acceptance: Acceptance,
    pub tolerances: Option<Tolerances>,
    pub metrics: Option<BTreeMap<String, ArmSummary>>,
    pub noise: Option<Noise>,
    pub calibration: Option<Calibration>,
    pub provenance: Option<Value>,
}

/// Structural rules for a baseline file: a pending one holds no measured value, a measured one holds
/// its digests, its metrics, its tolerances, its calibration and at least three repeated runs.
fn baseline_problems(base: &BaselineFile) -> Vec<String> {
    let mut problems = Vec::new();
    if base.schema != BASELINE_SCHEMA {
        problems.push(format!("baseline schema must be {BASELINE_SCHEMA}"));
    }
    match base.state.as_str() {
        "pending" => {
            let measured = base.serp_digest.is_some()
                || base.labels_digest.is_some()
                || base.metrics.is_some()
                || base.tolerances.is_some()
                || base.noise.is_some()
                || base.calibration.is_some();
            if measured {
                problems.push("a pending baseline must hold no measured values".to_owned());
            }
        }
        "measured" => {
            if base.window.is_none() {
                problems.push("a measured baseline needs its rerank window".to_owned());
            }
            if base.serp_digest.is_none() || base.labels_digest.is_none() {
                problems.push("a measured baseline needs its serp and labels digests".to_owned());
            }
            if base.metrics.is_none()
                || base.tolerances.is_none()
                || base.noise.is_none()
                || base.calibration.is_none()
            {
                problems.push(
                    "a measured baseline needs metrics, tolerances, noise and calibration"
                        .to_owned(),
                );
            }
            if let Some(noise) = &base.noise {
                if noise.runs.len() < MIN_BASELINE_RUNS {
                    problems.push(format!(
                        "a measured baseline needs at least {MIN_BASELINE_RUNS} repeated runs"
                    ));
                }
            }
        }
        other => problems.push(format!(
            "baseline state `{other}` is neither pending nor measured"
        )),
    }
    problems
}

fn load_baseline(path: &Path) -> Result<Option<BaselineFile>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = read_text(path)?;
    serde_json::from_str::<BaselineFile>(&text)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

struct Measured {
    metrics: BTreeMap<String, ArmSummary>,
    window: usize,
    c_runs: Vec<f64>,
    sd: f64,
    tolerances: Tolerances,
    calibration: Calibration,
}

/// Measures a baseline from repeated complete runs. The arm means are taken over the runs; the arm C
/// tolerance is twice the sample SD of its nDCG@10 across the runs, so identical runs are refused.
fn measure_runs(fx: &Fixtures, runs: &[Vec<RecordedCall>]) -> Result<Measured, String> {
    if runs.len() < MIN_BASELINE_RUNS {
        return Err(format!(
            "a baseline needs at least {MIN_BASELINE_RUNS} repeated live runs, got {}",
            runs.len()
        ));
    }
    let grades = final_grades(&fx.judgments)?;
    let selected: Vec<&QueryEntry> = fx.queries.queries.iter().collect();
    let mut arm_runs: Vec<BTreeMap<String, ArmSummary>> = Vec::with_capacity(runs.len());
    let mut c_runs = Vec::with_capacity(runs.len());
    let mut pairs = Vec::new();
    let mut cases = Vec::new();
    let mut window: Option<usize> = None;
    for calls in runs {
        let index = index_calls(calls, &fx.serp_digest)?;
        let run_window = index.window.ok_or("a run has no calls")?;
        match window {
            None => window = Some(run_window),
            Some(first) if first != run_window => {
                return Err("the repeated runs use different rerank windows".to_owned());
            }
            Some(_) => {}
        }
        let bundle = evaluate(fx, &grades, &index, &selected).map_err(stop_message)?;
        let arms = mean_arms(&bundle.evals);
        let c_value = arms
            .get("C")
            .map(|summary| summary.ndcg10)
            .ok_or("a run has no arm C summary")?;
        c_runs.push(c_value);
        arm_runs.push(arms);
        pairs.extend(bundle.pairs);
        cases.extend(bundle.cases);
    }
    let mut metrics = BTreeMap::new();
    for arm in ["A", "B", "C", "D"] {
        let values: Vec<&ArmSummary> = arm_runs.iter().filter_map(|run| run.get(arm)).collect();
        if values.len() != arm_runs.len() {
            continue;
        }
        let n = values.len() as f64;
        metrics.insert(
            arm.to_owned(),
            ArmSummary {
                ndcg10: values.iter().map(|value| value.ndcg10).sum::<f64>() / n,
                mrr: values.iter().map(|value| value.mrr).sum::<f64>() / n,
                p_at_3: values.iter().map(|value| value.p_at_3).sum::<f64>() / n,
                noise_at_5: values.iter().map(|value| value.noise_at_5).sum::<f64>() / n,
            },
        );
    }
    let sd = sample_sd(&c_runs);
    if sd <= EPS {
        return Err(
            "the repeated runs agree exactly (SD 0): they are not independent live runs".to_owned(),
        );
    }
    Ok(Measured {
        metrics,
        window: window.ok_or("the runs carry no rerank window")?,
        c_runs,
        sd,
        tolerances: Tolerances {
            ndcg10_arm_a: 0.0,
            ndcg10_arm_c: 2.0 * sd,
        },
        calibration: Calibration {
            fit_pairs: pairs.len(),
            knots: fit_knots(&pairs),
            provider_weight: fit_weight(&cases),
        },
    })
}

fn baseline_from(
    fx: &Fixtures,
    measured: Measured,
    acceptance: Acceptance,
    provenance: Value,
) -> BaselineFile {
    BaselineFile {
        schema: BASELINE_SCHEMA.to_owned(),
        state: "measured".to_owned(),
        description: BASELINE_DESCRIPTION.to_owned(),
        serp_digest: Some(fx.serp_digest.clone()),
        labels_digest: Some(fx.labels_digest.clone()),
        window: Some(measured.window),
        acceptance,
        tolerances: Some(measured.tolerances),
        metrics: Some(measured.metrics),
        noise: Some(Noise {
            method: NOISE_METHOD.to_owned(),
            runs: measured.c_runs,
            sd: measured.sd,
        }),
        calibration: Some(measured.calibration),
        provenance: Some(provenance),
    }
}

// ----- rerank wire contract -----

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRerank {
    pub status: String,
    pub reason: Option<String>,
    pub results: Vec<RecordedEntry>,
}

/// The `rerank-v1` request exactly as the production client sends it: the zdr mode is `disabled` for
/// public web text, the candidate ids are their input positions, and no field beyond the contract.
fn rerank_body(request_id: &str, query: &str, documents: &[String]) -> Value {
    json!({
        "request_id": request_id,
        "query": query,
        "profile": DP_PROFILE,
        "zdr_mode": "disabled",
        "sovereign_required": false,
        "top_n": documents.len(),
        "deadline_ms": DP_DEADLINE_MS,
        "documents": documents
            .iter()
            .enumerate()
            .map(|(index, text)| json!({ "id": index.to_string(), "text": text }))
            .collect::<Vec<Value>>(),
    })
}

/// Reads the answer under the contract: a known status; an `ok` answer carries a finite score for each
/// listed candidate; any other status carries no score; candidate indexes lie inside the request and
/// are not repeated. Anything else is a contract break, never an empty answer.
fn parse_rerank_response(value: &Value, documents: usize) -> Result<ParsedRerank, String> {
    let object = value
        .as_object()
        .ok_or("contract break: the answer is not a JSON object")?;
    let profile = object.get("profile").and_then(Value::as_str).unwrap_or("");
    if profile != DP_PROFILE {
        return Err("contract break: unexpected profile".to_owned());
    }
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or("contract break: no status")?;
    if !matches!(status, "ok" | "skipped" | "degraded") {
        return Err("contract break: unknown status".to_owned());
    }
    let reason = match object.get("reason") {
        None | Some(Value::Null) => None,
        Some(Value::String(reason)) => Some(reason.clone()),
        Some(_) => return Err("contract break: reason is not a string".to_owned()),
    };
    if status == "ok" && reason.is_some() {
        return Err("contract break: an ok answer carries a reason".to_owned());
    }
    let entries = object
        .get("results")
        .and_then(Value::as_array)
        .ok_or("contract break: no results array")?;
    let mut seen = HashSet::new();
    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        let index = entry
            .get("index")
            .and_then(Value::as_u64)
            .ok_or("contract break: a result has no index")? as usize;
        if index >= documents || !seen.insert(index) {
            return Err("contract break: a result index is out of range or repeated".to_owned());
        }
        let scored = entry
            .get("scored")
            .and_then(Value::as_bool)
            .ok_or("contract break: a result has no scored flag")?;
        let score = entry.get("score").and_then(Value::as_f64);
        match (status == "ok", scored, score) {
            (true, true, Some(value)) if value.is_finite() => results.push(RecordedEntry {
                index,
                score: Some(value),
            }),
            (false, false, None) => results.push(RecordedEntry { index, score: None }),
            _ => return Err("contract break: scores do not match the status".to_owned()),
        }
    }
    if status == "ok" && results.is_empty() {
        return Err("contract break: an ok answer has no results".to_owned());
    }
    Ok(ParsedRerank {
        status: status.to_owned(),
        reason,
        results,
    })
}

fn check_endpoint(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        return Ok(());
    }
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"];
    let local = loopback.iter().any(|prefix| {
        url == *prefix
            || url.starts_with(format!("{prefix}:").as_str())
            || url.starts_with(format!("{prefix}/").as_str())
    });
    if local {
        Ok(())
    } else {
        Err("the Data Plane URL must be https, or http on a loopback address".to_owned())
    }
}

fn valid_run_id(run: &str) -> bool {
    (1..=16).contains(&run.len()) && run.chars().all(|c| c.is_ascii_alphanumeric())
}

fn refusal_message(status: u16, value: &Value) -> String {
    let code = value
        .get("error")
        .and_then(Value::as_str)
        .filter(|code| code.len() <= 64 && code.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .unwrap_or("no_error_code");
    format!("the Data Plane refused the call: HTTP {status} ({code})")
}

/// One `POST /v1/rerank`, one attempt. Production never retries: `DataPlaneSearchReranker` answers a 429
/// from the Data Plane's own limiter as a `degraded` `rate_limited` result with the input order
/// (quarry-runtime rerank.rs, `read`), and the HTTP client in data_plane_http.rs has no retry. The lab
/// mirrors that, so the degraded rate it measures is the rate production would see. Every status is
/// returned to the caller unchanged.
async fn post_rerank(
    client: &reqwest::Client,
    url: &str,
    bearer: &str,
    body: &Value,
) -> Result<(u16, Value, f64), String> {
    let started = Instant::now();
    let response = client
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .send()
        .await
        .map_err(|error| format!("transport error: {}", error.without_url()))?;
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("read error: {}", error.without_url()))?;
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let value = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    Ok((status, value, latency_ms))
}

/// The recorded form of a 429: a degraded call with reason `rate_limited` and no scores. It falls back
/// to engine order and counts as not ok in the soak.
fn rate_limited_call(
    arm: &str,
    query_id: &str,
    request_id: &str,
    serp: &str,
    window: usize,
    latency_ms: f64,
) -> RecordedCall {
    RecordedCall {
        arm: arm.to_owned(),
        query_id: query_id.to_owned(),
        request_id: request_id.to_owned(),
        serp_digest: serp.to_owned(),
        window,
        http_status: 429,
        status: "degraded".to_owned(),
        reason: Some("rate_limited".to_owned()),
        latency_ms,
        results: Vec::new(),
    }
}

async fn send_call(
    client: &reqwest::Client,
    url: &str,
    bearer: &str,
    request_id: &str,
    query: &QueryEntry,
    rows: &[&SerpRow],
    arm: &str,
    serp: &str,
    window: usize,
) -> Result<RecordedCall, String> {
    let documents: Vec<String> = rows
        .iter()
        .take(window)
        .map(|row| candidate_text(row, arm == "D"))
        .collect();
    let body = rerank_body(request_id, &query.text, &documents);
    let (http_status, value, latency_ms) = post_rerank(client, url, bearer, &body).await?;
    if http_status == 429 {
        return Ok(rate_limited_call(
            arm,
            &query.id,
            request_id,
            serp,
            documents.len(),
            latency_ms,
        ));
    }
    if http_status != 200 {
        return Err(refusal_message(http_status, &value));
    }
    let parsed = parse_rerank_response(&value, documents.len())?;
    Ok(RecordedCall {
        arm: arm.to_owned(),
        query_id: query.id.clone(),
        request_id: request_id.to_owned(),
        serp_digest: serp.to_owned(),
        window: documents.len(),
        http_status,
        status: parsed.status,
        reason: parsed.reason,
        latency_ms,
        results: parsed.results,
    })
}

// ----- commands -----

fn check(args: &Args) -> Result<u8, String> {
    let fx = Fixtures::load(&fixture_dir(args))?;
    let mut problems = validate_fixtures(&fx);
    let baseline = load_baseline(&baseline_path(args))?;
    let mut baseline_state = "absent".to_owned();
    if let Some(base) = &baseline {
        baseline_state = base.state.clone();
        problems.extend(baseline_problems(base));
        if baseline_is_stale(base, &fx) {
            problems.push(
                "the baseline was measured on other fixtures: write-baseline again".to_owned(),
            );
        }
    }
    let summary = json!({
        "label_state": fx.queries.label_state,
        "queries": fx.queries.queries.len(),
        "results": fx.results.len(),
        "judgments": fx.judgments.len(),
        "serp_digest": fx.serp_digest,
        "labels_digest": fx.labels_digest,
        "baseline_state": baseline_state,
        "problems": problems,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
    );
    Ok(if problems.is_empty() { 0 } else { 1 })
}

fn gate_command(args: &Args) -> Result<u8, String> {
    let dir = fixture_dir(args);
    let fx = Fixtures::load(&dir)?;
    let recording_path = args
        .flag("recording")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join(RECORDING_FILE));
    let calls = if recording_path.exists() {
        Some(read_recording(&recording_path)?)
    } else {
        None
    };
    let baseline = load_baseline(&baseline_path(args))?;
    let report = run_gate(
        &fx,
        calls.as_deref(),
        baseline.as_ref(),
        args.switch("smoke"),
    );
    let text = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    if let Some(path) = args.flag("report-out") {
        fs::write(path, format!("{text}\n")).map_err(|error| format!("{path}: {error}"))?;
    }
    println!("{text}");
    Ok(match report.verdict.as_str() {
        "pass" => 0,
        "fail" => 1,
        _ => 2,
    })
}

fn write_baseline_command(args: &Args) -> Result<u8, String> {
    let target = baseline_path(args);
    let fx = Fixtures::load(&fixture_dir(args))?;
    let problems = validate_fixtures(&fx);
    if !problems.is_empty() {
        return Err(format!("the fixtures are invalid: {}", problems.join("; ")));
    }
    if fx.queries.label_state != "complete" {
        return Err("write-baseline needs complete labels (operator step OPS-LABEL)".to_owned());
    }
    let list = args
        .flag("recordings")
        .ok_or("--recordings A,B,C is required: three or more repeated live runs")?;
    let paths: Vec<PathBuf> = list
        .split(',')
        .map(|item| PathBuf::from(item.trim()))
        .collect();
    let mut runs = Vec::with_capacity(paths.len());
    for path in &paths {
        runs.push(read_recording(path)?);
    }
    let measured = measure_runs(&fx, &runs)?;
    let sd = measured.sd;
    let acceptance = load_baseline(&target)?
        .map(|existing| existing.acceptance)
        .unwrap_or_default();
    let provenance = json!({
        "recordings": paths.iter().map(|path| path.display().to_string()).collect::<Vec<String>>(),
        "runs": paths.len(),
    });
    let baseline = baseline_from(&fx, measured, acceptance, provenance);
    let problems = baseline_problems(&baseline);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    write_json(&target, &baseline)?;
    let summary = json!({
        "wrote": target.display().to_string(),
        "runs": paths.len(),
        "state": "measured",
        "tolerance_arm_c": 2.0 * sd,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
    );
    Ok(0)
}

async fn freeze(args: &Args) -> Result<u8, String> {
    let dir = fixture_dir(args);
    let mut fx = Fixtures::load(&dir)?;
    if fx.queries.label_state != "pending" || !fx.judgments.is_empty() {
        return Err(
            "freeze is refused once labels exist: the frozen SERP must not change under judged rows"
                .to_owned(),
        );
    }
    let base = args
        .flag("searxng")
        .ok_or("--searxng is required, for example http://127.0.0.1:18888")?;
    let start: usize = args.number("start", 0)?;
    let count: usize = args.number("count", QUERY_COUNT)?;
    let image = args
        .flag("searxng-image")
        .unwrap_or(DEFAULT_SEARXNG_IMAGE)
        .to_owned();
    let endpoint = format!("{}/search", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| error.to_string())?;
    let batch: Vec<QueryEntry> = fx
        .queries
        .queries
        .iter()
        .skip(start)
        .take(count)
        .cloned()
        .collect();
    let mut fresh: BTreeMap<String, (Vec<SerpRow>, FrozenQuery)> = BTreeMap::new();
    for query in &batch {
        let language = if query.lang == "no" { "nb-NO" } else { "en" };
        let response = client
            .get(&endpoint)
            .query(&[
                ("q", query.text.as_str()),
                ("format", "json"),
                ("count", "20"),
                ("language", language),
                ("safesearch", "1"),
            ])
            .send()
            .await
            .map_err(|error| format!("query `{}`: {}", query.id, error.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "query `{}`: SearXNG answered HTTP {}",
                query.id,
                status.as_u16()
            ));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("query `{}`: {}", query.id, error.without_url()))?;
        let frozen = serp_rows_from_searxng(&query.id, &body)?;
        fresh.insert(query.id.clone(), frozen);
        tokio::time::sleep(FREEZE_PACING).await;
    }
    fx.results.retain(|row| !fresh.contains_key(&row.query_id));
    for query in fx.queries.queries.iter_mut() {
        if let Some((_, frozen)) = fresh.get(&query.id) {
            query.frozen = Some(frozen.clone());
        }
    }
    for (rows, _) in fresh.values() {
        fx.results.extend(rows.iter().cloned());
    }
    let order: HashMap<&str, usize> = fx
        .queries
        .queries
        .iter()
        .enumerate()
        .map(|(position, query)| (query.id.as_str(), position))
        .collect();
    fx.results.sort_by_key(|row| {
        (
            order
                .get(row.query_id.as_str())
                .copied()
                .unwrap_or(usize::MAX),
            row.rank,
        )
    });
    fx.queries.freeze = Some(FreezeHeader {
        searxng_image: image,
        frozen_at_unix: unix_now(),
        count_param: 20,
        safesearch: 1,
        candidates_per_query: CANDIDATES,
    });
    write_jsonl(&dir.join(RESULTS_FILE), &fx.results)?;
    write_json(&dir.join(QUERIES_FILE), &fx.queries)?;
    let summary = json!({
        "frozen_queries": fresh.len(),
        "results_rows": fx.results.len(),
        "start": start,
        "count": count,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
    );
    Ok(0)
}

/// The top candidates of one SearXNG answer, in the production order: fused by the reciprocal rank of
/// each engine position (descending, stable), deduplicated by the raw url, then cut to the candidate
/// count. Fewer unique results than that is an error, never a shorter list.
fn serp_rows_from_searxng(
    query_id: &str,
    body: &Value,
) -> Result<(Vec<SerpRow>, FrozenQuery), String> {
    let results = body
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("query `{query_id}`: the answer has no results array"))?;
    let unresponsive_engines: Vec<String> = body
        .get("unresponsive_engines")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get(0)
                        .and_then(Value::as_str)
                        .or_else(|| item.as_str())
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut candidates: Vec<(f32, SerpRow)> = Vec::new();
    for item in results {
        let url = item
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        if url.is_empty() {
            continue;
        }
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let snippet = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let engines: Vec<String> = item
            .get("engines")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let positions: Vec<u32> = item
            .get("positions")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_u64)
                    .map(|position| position as u32)
                    .collect()
            })
            .unwrap_or_default();
        let fusion: f32 = positions
            .iter()
            .map(|position| 1.0f32 / (RRF_K + *position as f32))
            .sum();
        let fusion_score = if positions.is_empty() {
            None
        } else {
            Some(f64::from(fusion))
        };
        candidates.push((
            fusion,
            SerpRow {
                query_id: query_id.to_owned(),
                rank: 0,
                url,
                title,
                snippet,
                engines,
                positions,
                fusion_score,
            },
        ));
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut seen: HashSet<String> = HashSet::new();
    let mut rows: Vec<SerpRow> = Vec::new();
    for (_, mut row) in candidates {
        if !seen.insert(row.url.clone()) {
            continue;
        }
        row.rank = rows.len() + 1;
        rows.push(row);
        if rows.len() == CANDIDATES {
            break;
        }
    }
    if rows.len() < CANDIDATES {
        return Err(format!(
            "query `{query_id}`: {} unique results, need {CANDIDATES}",
            rows.len()
        ));
    }
    Ok((
        rows,
        FrozenQuery {
            raw_results: results.len(),
            unresponsive_engines,
        },
    ))
}

async fn live(args: &Args) -> Result<u8, String> {
    let dir = fixture_dir(args);
    let fx = Fixtures::load(&dir)?;
    let problems = validate_fixtures(&fx);
    if !problems.is_empty() {
        return Err(format!("the fixtures are invalid: {}", problems.join("; ")));
    }
    let url = match args.flag("dp-url") {
        Some(value) => value.to_owned(),
        None => std::env::var("QUARRY_RERANK_URL")
            .map_err(|_| "--dp-url or QUARRY_RERANK_URL is required".to_owned())?,
    };
    check_endpoint(&url)?;
    let bearer = std::env::var("QUARRY_RERANK_BEARER")
        .map_err(|_| "QUARRY_RERANK_BEARER must hold a web-rerank token".to_owned())?;
    if bearer.trim().is_empty() {
        return Err("QUARRY_RERANK_BEARER is empty".to_owned());
    }
    let run_id = args
        .flag("run-id")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("t{}", unix_now() % 1_000_000_000));
    if !valid_run_id(&run_id) {
        return Err("--run-id must be 1 to 16 letters or digits".to_owned());
    }
    let arms: Vec<String> = args
        .flag("arms")
        .unwrap_or("C")
        .split(',')
        .map(|arm| arm.trim().to_owned())
        .collect();
    if arms.is_empty() || arms.iter().any(|arm| arm != "C" && arm != "D") {
        return Err("--arms takes C, D or C,D".to_owned());
    }
    let soak: usize = args.number("soak", 0)?;
    let window: usize = args.number("window", DEFAULT_WINDOW)?;
    if !(1..=CANDIDATES).contains(&window) {
        return Err(format!("--window must be 1 to {CANDIDATES}"));
    }
    let out = args
        .flag("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join(RECORDING_FILE));
    let selected: Vec<&QueryEntry> = if args.switch("smoke") {
        smoke_queries(&fx.queries.queries)
    } else {
        fx.queries.queries.iter().collect()
    };
    let mut rows_by_query: HashMap<&str, Vec<&SerpRow>> = HashMap::new();
    for row in &fx.results {
        rows_by_query
            .entry(row.query_id.as_str())
            .or_default()
            .push(row);
    }
    for rows in rows_by_query.values_mut() {
        rows.sort_by_key(|row| row.rank);
    }
    let mut plan: Vec<(String, String, &QueryEntry)> = Vec::new();
    for arm in &arms {
        for query in &selected {
            plan.push((
                arm.clone(),
                format!("qwrg-{run_id}-{arm}-{}", query.id),
                *query,
            ));
        }
    }
    for index in 0..soak {
        let query = &fx.queries.queries[index % fx.queries.queries.len()];
        plan.push((
            "soak".to_owned(),
            format!("qwrg-{run_id}-S-{index:04}"),
            query,
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(CALL_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let file = File::create(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let mut writer = BufWriter::new(file);
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut sent = 0usize;
    let mut refusal: Option<String> = None;
    for (arm, request_id, query) in &plan {
        let rows = rows_by_query
            .get(query.id.as_str())
            .cloned()
            .unwrap_or_default();
        match send_call(
            &client,
            &url,
            &bearer,
            request_id,
            query,
            &rows,
            arm,
            &fx.serp_digest,
            window,
        )
        .await
        {
            Ok(call) => {
                sent += 1;
                let key = match &call.reason {
                    Some(reason) => format!("{}:{}", call.status, reason),
                    None => call.status.clone(),
                };
                *outcomes.entry(key).or_insert(0) += 1;
                let line = serde_json::to_string(&call).map_err(|error| error.to_string())?;
                writer
                    .write_all(line.as_bytes())
                    .and_then(|_| writer.write_all(b"\n"))
                    .and_then(|_| writer.flush())
                    .map_err(|error| format!("{}: {error}", out.display()))?;
            }
            Err(message) => {
                refusal = Some(format!("call {request_id}: {message}"));
                break;
            }
        }
    }
    writer
        .flush()
        .map_err(|error| format!("{}: {error}", out.display()))?;
    let summary = json!({
        "run_id": run_id,
        "recording": out.display().to_string(),
        "arms": arms,
        "queries": selected.len(),
        "soak_calls": soak,
        "window": window,
        "planned_calls": plan.len(),
        "requests_sent": sent,
        "outcomes": outcomes,
        "serp_digest": fx.serp_digest,
        "stopped_early": refusal.is_some(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
    );
    if let Some(message) = refusal {
        eprintln!("quarry-web-rerank-gate: {message}");
        return Ok(3);
    }
    Ok(0)
}

// ----- tests -----

#[cfg(test)]
mod tests {
    use super::*;

    const OK_GOLDEN: &str =
        include_str!("../fixtures/web-rerank/contracts/rerank-v1/ok.golden.json");
    const SKIPPED_GOLDEN: &str =
        include_str!("../fixtures/web-rerank/contracts/rerank-v1/skipped-zdr.golden.json");
    const DEGRADED_GOLDEN: &str =
        include_str!("../fixtures/web-rerank/contracts/rerank-v1/degraded-timeout.golden.json");

    /// English queries per category, in CATEGORIES order (navigational, factual, fresh_news,
    /// research_comparative, local, technical): 3 + 8 + 6 + 7 + 2 + 4 = 30.
    const EN_PER_CATEGORY: [usize; 6] = [3, 8, 6, 7, 2, 4];

    fn real_dir() -> PathBuf {
        eval_root().join(FIXTURE_DIR)
    }

    fn real_fixtures() -> Fixtures {
        Fixtures::load(&real_dir()).expect("the committed fixtures load")
    }

    fn synthetic_grade(query: usize, rank: usize) -> u8 {
        ((query * 7 + rank * 3) % 4) as u8
    }

    /// A complete synthetic fixture set with the real shape: 80 queries by the quotas, 20 candidates
    /// each, and (when `complete`) two judges with one disputed candidate in five, adjudicated.
    fn synthetic_fixtures(complete: bool) -> Fixtures {
        let mut queries = Vec::new();
        let mut counter = 0usize;
        for (category_index, (name, total, _)) in CATEGORIES.iter().enumerate() {
            for position in 0..*total {
                let lang = if position < EN_PER_CATEGORY[category_index] {
                    "en"
                } else {
                    "no"
                };
                queries.push(QueryEntry {
                    id: format!("s{counter:02}"),
                    lang: lang.to_owned(),
                    category: (*name).to_owned(),
                    text: format!("synthetic {name} query {counter}"),
                    frozen: None,
                });
                counter += 1;
            }
        }
        let mut results = Vec::new();
        for (query_index, query) in queries.iter().enumerate() {
            for rank in 1..=CANDIDATES {
                results.push(SerpRow {
                    query_id: query.id.clone(),
                    rank,
                    url: format!("https://site{query_index}.example/r{rank}"),
                    title: format!("{} result {rank}", query.text),
                    snippet: format!("snippet {rank}"),
                    engines: vec!["bing".to_owned()],
                    positions: vec![rank as u32],
                    fusion_score: None,
                });
            }
        }
        let mut judgments = Vec::new();
        if complete {
            for (query_index, query) in queries.iter().enumerate() {
                for rank in 1..=CANDIDATES {
                    let url = format!("https://site{query_index}.example/r{rank}");
                    let grade = synthetic_grade(query_index, rank);
                    let disputed = (query_index + rank) % 5 == 0;
                    let second = if disputed { (grade + 1) % 4 } else { grade };
                    judgments.push(JudgmentRow {
                        query_id: query.id.clone(),
                        url: url.clone(),
                        judge: "llm-prejudge".to_owned(),
                        grade,
                    });
                    judgments.push(JudgmentRow {
                        query_id: query.id.clone(),
                        url: url.clone(),
                        judge: "human".to_owned(),
                        grade: second,
                    });
                    if disputed {
                        judgments.push(JudgmentRow {
                            query_id: query.id.clone(),
                            url,
                            judge: ADJUDICATED.to_owned(),
                            grade,
                        });
                    }
                }
            }
        }
        let label_state = if complete { "complete" } else { "pending" };
        let serp = serp_digest(&queries, &results);
        let labels = labels_digest(label_state, &judgments);
        Fixtures {
            queries: QueriesFile {
                schema: QUERIES_SCHEMA.to_owned(),
                label_state: label_state.to_owned(),
                freeze: None,
                queries,
            },
            results,
            judgments,
            serp_digest: serp,
            labels_digest: labels,
        }
    }

    /// Synthetic live calls: arm C for every query (score per query and rank), and a 200-call soak with
    /// the first `degraded_soak` calls degraded.
    fn synthetic_run(
        fx: &Fixtures,
        scores: &dyn Fn(usize, usize) -> f64,
        degraded_soak: usize,
    ) -> Vec<RecordedCall> {
        let mut calls = Vec::new();
        for (query_index, query) in fx.queries.queries.iter().enumerate() {
            calls.push(RecordedCall {
                arm: "C".to_owned(),
                query_id: query.id.clone(),
                request_id: format!("qwrg-syn-C-{}", query.id),
                serp_digest: fx.serp_digest.clone(),
                window: CANDIDATES,
                http_status: 200,
                status: "ok".to_owned(),
                reason: None,
                latency_ms: 120.0,
                results: (0..CANDIDATES)
                    .map(|index| RecordedEntry {
                        index,
                        score: Some(scores(query_index, index + 1)),
                    })
                    .collect(),
            });
        }
        for index in 0..200 {
            let query_index = index % QUERY_COUNT;
            let query = &fx.queries.queries[query_index];
            let degraded = index < degraded_soak;
            calls.push(RecordedCall {
                arm: "soak".to_owned(),
                query_id: query.id.clone(),
                request_id: format!("qwrg-syn-S-{index:04}"),
                serp_digest: fx.serp_digest.clone(),
                window: CANDIDATES,
                http_status: 200,
                status: (if degraded { "degraded" } else { "ok" }).to_owned(),
                reason: if degraded {
                    Some("timeout".to_owned())
                } else {
                    None
                },
                latency_ms: 150.0,
                results: if degraded {
                    Vec::new()
                } else {
                    (0..CANDIDATES)
                        .map(|position| RecordedEntry {
                            index: position,
                            score: Some(scores(query_index, position + 1)),
                        })
                        .collect()
                },
            });
        }
        calls
    }

    /// Scores that follow the grades exactly, with a tiny tie-break on the engine rank.
    fn good_scores(query: usize, rank: usize) -> f64 {
        f64::from(synthetic_grade(query, rank)) / 3.0 - rank as f64 * 1e-6
    }

    /// Scores that follow the grades with a per-run jitter large enough to reorder candidates.
    fn noisy_scores(jitter: f64) -> impl Fn(usize, usize) -> f64 {
        move |query, rank| {
            let hash = ((query * 37 + rank * 17) % 100) as f64 / 100.0;
            f64::from(synthetic_grade(query, rank)) / 3.0 + jitter * hash
        }
    }

    /// Scores that invert the grades: a provider that makes the ranking worse than the engine order.
    fn reversed_scores(query: usize, rank: usize) -> f64 {
        f64::from(3 - synthetic_grade(query, rank)) / 3.0
    }

    fn three_runs(fx: &Fixtures) -> Vec<Vec<RecordedCall>> {
        vec![
            synthetic_run(fx, &good_scores, 0),
            synthetic_run(fx, &noisy_scores(0.5), 0),
            synthetic_run(fx, &noisy_scores(0.9), 0),
        ]
    }

    fn measured_baseline(fx: &Fixtures, acceptance: Acceptance) -> BaselineFile {
        let measured = measure_runs(fx, &three_runs(fx)).expect("three independent runs measure");
        baseline_from(fx, measured, acceptance, json!({ "synthetic": true }))
    }

    // ----- metrics: hand-computed references -----

    #[test]
    fn ndcg_matches_the_hand_computed_reference() {
        let value = ndcg_at(&[3, 0, 2, 1], &[3, 2, 1, 0], DCG_DEPTH);
        assert!((value - 0.950_801_333_894).abs() < 1e-9, "got {value}");
    }

    #[test]
    fn ndcg_is_one_for_the_ideal_order_and_zero_without_relevance() {
        assert!((ndcg_at(&[3, 2, 1, 0], &[3, 2, 1, 0], DCG_DEPTH) - 1.0).abs() < 1e-12);
        assert_eq!(ndcg_at(&[0, 0, 0], &[0, 0, 0], DCG_DEPTH), 0.0);
    }

    #[test]
    fn gain_is_two_to_the_grade_minus_one() {
        let gains: Vec<f64> = (0..=3).map(gain).collect();
        assert_eq!(gains, vec![0.0, 1.0, 3.0, 7.0]);
    }

    #[test]
    fn reciprocal_rank_precision_and_noise_count_grade_two_as_relevant() {
        // Relevant (grade >= 2) first at position 3: reciprocal rank 1/3.
        assert!((reciprocal_rank(&[0, 1, 2, 3, 0]) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(reciprocal_rank(&[0, 1, 1]), 0.0);
        // Top three is [0, 1, 2]: one relevant of three.
        assert!((precision_at(&[0, 1, 2, 3, 0], 3) - 1.0 / 3.0).abs() < 1e-12);
        // Grade-zero candidates in the top five: positions 1 and 5.
        assert!((noise_at(&[0, 1, 2, 3, 0], 5) - 0.4).abs() < 1e-12);
    }

    #[test]
    fn p95_is_the_nearest_rank_value() {
        let values: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(p95_nearest_rank(&values), Some(19.0));
        assert_eq!(p95_nearest_rank(&[7.0]), Some(7.0));
        assert_eq!(p95_nearest_rank(&[]), None);
    }

    #[test]
    fn cohen_kappa_matches_the_reference_value() {
        let pairs = [
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (1, 1),
            (1, 1),
            (2, 2),
            (3, 3),
            (0, 1),
            (2, 3),
        ];
        let kappa = cohen_kappa(&pairs).expect("pairs exist");
        assert!((kappa - 0.714_285_714_286).abs() < 1e-9, "got {kappa}");
        assert_eq!(cohen_kappa(&[(2, 2), (3, 3)]), Some(1.0));
        assert_eq!(cohen_kappa(&[]), None);
        assert_eq!(
            cohen_kappa(&[(4, 0)]),
            None,
            "an out-of-range grade has no kappa"
        );
    }

    // ----- statistics -----

    #[test]
    fn splitmix_first_output_from_seed_zero_is_the_reference() {
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn paired_bootstrap_is_deterministic_for_a_seed_and_brackets_the_mean() {
        let deltas: Vec<f64> = (0..80)
            .map(|i| if i % 3 == 0 { -0.05 } else { 0.1 })
            .collect();
        let first = paired_bootstrap(&deltas, 2_000, 20_261_013);
        let second = paired_bootstrap(&deltas, 2_000, 20_261_013);
        assert_eq!(first, second, "a seed fixes every resample");
        let (center, low, high) = first;
        assert!(low <= center && center <= high, "{low} {center} {high}");
    }

    #[test]
    fn paired_bootstrap_collapses_for_constant_deltas_and_is_positive_for_a_clear_gain() {
        let constant = vec![0.05; 80];
        let (center, low, high) = paired_bootstrap(&constant, 500, 1);
        for value in [center, low, high] {
            assert!(
                (value - 0.05).abs() < 1e-12,
                "constant deltas must collapse: {value}"
            );
        }
        let consistent: Vec<f64> = (0..80)
            .map(|i| 0.05 + if i % 2 == 0 { 0.01 } else { -0.01 })
            .collect();
        let (_, low, _) = paired_bootstrap(&consistent, 10_000, 20_261_013);
        assert!(low > 0.0, "the lower bound should be above zero, got {low}");
    }

    #[test]
    fn default_acceptance_is_the_design_gate() {
        let acceptance = Acceptance::default();
        assert_eq!(acceptance.min_ndcg_gain, 0.04);
        assert_eq!(acceptance.min_ci_lower, 0.0);
        assert_eq!(acceptance.category_max_drop, 0.03);
        assert_eq!(acceptance.added_p95_ms_max, 900.0);
        assert_eq!(acceptance.degraded_rate_max, 0.02);
        assert_eq!(acceptance.mde_max, 0.04);
        assert_eq!(acceptance.bootstrap_resamples, 10_000);
        assert_eq!(acceptance.bootstrap_seed, 20_261_013);
        assert_eq!(acceptance.soak_calls, 200);
    }

    // ----- calibration -----

    #[test]
    fn isotonic_knots_match_the_hand_fit_and_are_monotone() {
        let pairs = [
            (0.1, 0.0),
            (0.2, 1.0),
            (0.3, 0.0),
            (0.4, 1.0),
            (0.5, 1.0),
            (0.6, 0.0),
        ];
        let knots = fit_knots(&pairs);
        let expected = [(0.1, 0.0), (0.25, 0.5), (0.5, 2.0 / 3.0)];
        assert_eq!(knots.len(), expected.len(), "{knots:?}");
        for (knot, (score, relevance)) in knots.iter().zip(expected) {
            assert!((knot.score - score).abs() < 1e-12, "{knot:?}");
            assert!((knot.relevance - relevance).abs() < 1e-12, "{knot:?}");
        }
        assert!(knots
            .windows(2)
            .all(|pair| pair[0].relevance <= pair[1].relevance));
    }

    #[test]
    fn calibration_interpolates_linearly_and_clamps_outside_the_knots() {
        let knots = vec![
            Knot {
                score: 0.1,
                relevance: 0.0,
            },
            Knot {
                score: 0.25,
                relevance: 0.5,
            },
            Knot {
                score: 0.5,
                relevance: 2.0 / 3.0,
            },
        ];
        assert_eq!(calibrate(&knots, 0.05), Some(0.0));
        assert_eq!(calibrate(&knots, 0.6), Some(2.0 / 3.0));
        assert!((calibrate(&knots, 0.175).expect("in range") - 0.25).abs() < 1e-12);
        assert_eq!(calibrate(&[], 0.5), None);
    }

    #[test]
    fn weight_refit_keeps_the_lexical_signal_when_only_lexical_is_right() {
        let grades: Vec<u8> = (0..CANDIDATES).map(|i| (i % 4) as u8).collect();
        let lexical: Vec<f64> = grades.iter().map(|g| f64::from(*g) / 3.0).collect();
        let provider: Vec<f64> = grades.iter().map(|g| f64::from(3 - *g) / 3.0).collect();
        let fit = fit_weight(&[WeightCase {
            window: CANDIDATES,
            provider,
            lexical,
            grades,
        }]);
        assert_eq!(fit.best_weight, 0.0);
        assert!((fit.best_mean_ndcg10 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn weight_refit_moves_to_the_provider_when_only_the_provider_is_right() {
        let grades: Vec<u8> = (0..CANDIDATES).map(|i| (i % 4) as u8).collect();
        let provider: Vec<f64> = grades.iter().map(|g| f64::from(*g) / 3.0).collect();
        let lexical = vec![0.0; CANDIDATES];
        let fit = fit_weight(&[WeightCase {
            window: CANDIDATES,
            provider,
            lexical,
            grades,
        }]);
        assert_eq!(fit.curve.len(), WEIGHT_STEPS + 1);
        assert!(fit.curve[0].mean_ndcg10 < 1.0, "lexical alone cannot rank");
        assert!(
            (fit.best_weight - 0.05).abs() < 1e-12,
            "the smallest weight that attains the maximum"
        );
        assert_eq!(fit.production_weight, PRODUCTION_PROVIDER_WEIGHT);
    }

    // ----- lexical arm and candidate text -----

    #[test]
    fn host_of_strips_scheme_userinfo_port_and_www() {
        assert_eq!(host_of("https://www.Example.com:443/a?b#c"), "example.com");
        assert_eq!(host_of("http://user:pw@host.no/x"), "host.no");
        assert_eq!(host_of("mattilsynet.no/veiledning"), "mattilsynet.no");
    }

    #[test]
    fn lexical_score_counts_shared_query_tokens_case_insensitively() {
        assert_eq!(
            lexical_score("Hygiene krav", "hygiene regler og KRAV i industrien"),
            1.0
        );
        assert!((lexical_score("PLC CIP", "CIP-anlegg i bryggerier") - 0.5).abs() < 1e-12);
        assert_eq!(
            lexical_score("a", "a"),
            0.0,
            "single characters are not tokens"
        );
    }

    #[test]
    fn candidate_text_matches_the_production_client_and_the_host_variant() {
        let mut row = SerpRow {
            query_id: "q".to_owned(),
            rank: 1,
            url: "https://www.example.no/pricing".to_owned(),
            title: " Pricing ".to_owned(),
            snippet: "per cleaning cycle".to_owned(),
            engines: Vec::new(),
            positions: Vec::new(),
            fusion_score: None,
        };
        assert_eq!(candidate_text(&row, false), "Pricing\nper cleaning cycle");
        assert_eq!(
            candidate_text(&row, true),
            "Pricing\nper cleaning cycle\nexample.no"
        );
        row.snippet = String::new();
        assert_eq!(candidate_text(&row, false), "Pricing");
        row.title = String::new();
        row.snippet = "only the snippet".to_owned();
        assert_eq!(candidate_text(&row, false), "only the snippet");
        row.snippet = String::new();
        assert_eq!(
            candidate_text(&row, false),
            "https://www.example.no/pricing"
        );
    }

    // ----- rerank wire contract -----

    #[test]
    fn rerank_goldens_parse_to_the_documented_shapes() {
        let ok: Value = serde_json::from_str(OK_GOLDEN).expect("golden parses");
        let parsed = parse_rerank_response(&ok, 3).expect("ok golden is valid");
        assert_eq!(parsed.status, "ok");
        assert_eq!(parsed.reason, None);
        let scores: Vec<Option<f64>> = parsed.results.iter().map(|entry| entry.score).collect();
        assert_eq!(scores, vec![Some(1.0), Some(0.99), Some(0.98)]);

        let skipped: Value = serde_json::from_str(SKIPPED_GOLDEN).expect("golden parses");
        let parsed = parse_rerank_response(&skipped, 3).expect("skipped golden is valid");
        assert_eq!(parsed.status, "skipped");
        assert_eq!(parsed.reason.as_deref(), Some("zdr"));
        assert!(parsed.results.iter().all(|entry| entry.score.is_none()));

        let degraded: Value = serde_json::from_str(DEGRADED_GOLDEN).expect("golden parses");
        let parsed = parse_rerank_response(&degraded, 3).expect("degraded golden is valid");
        assert_eq!(parsed.status, "degraded");
        assert_eq!(parsed.reason.as_deref(), Some("timeout"));
    }

    #[test]
    fn rerank_parser_rejects_contract_breaks() {
        let ok_without_score = json!({
            "profile": "web-v1", "status": "ok", "reason": null,
            "results": [{"index": 0, "scored": false}]
        });
        assert!(parse_rerank_response(&ok_without_score, 1).is_err());
        let out_of_range = json!({
            "profile": "web-v1", "status": "ok", "reason": null,
            "results": [{"index": 2, "scored": true, "score": 0.5}]
        });
        assert!(parse_rerank_response(&out_of_range, 2).is_err());
        let repeated = json!({
            "profile": "web-v1", "status": "ok", "reason": null,
            "results": [{"index": 0, "scored": true, "score": 0.5}, {"index": 0, "scored": true, "score": 0.4}]
        });
        assert!(parse_rerank_response(&repeated, 2).is_err());
        let unknown_status =
            json!({"profile": "web-v1", "status": "weird", "reason": null, "results": []});
        assert!(parse_rerank_response(&unknown_status, 1).is_err());
        let wrong_profile =
            json!({"profile": "web-v2", "status": "skipped", "reason": "zdr", "results": []});
        assert!(parse_rerank_response(&wrong_profile, 1).is_err());
        let scored_skip = json!({
            "profile": "web-v1", "status": "skipped", "reason": "zdr",
            "results": [{"index": 0, "scored": true, "score": 0.5}]
        });
        assert!(parse_rerank_response(&scored_skip, 1).is_err());
    }

    #[test]
    fn rerank_body_has_exactly_the_data_plane_fields() {
        let body = rerank_body(
            "qwrg-r1-C-nav-01",
            "query",
            &["a".to_owned(), "b".to_owned()],
        );
        let mut keys: Vec<&str> = body
            .as_object()
            .expect("object")
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
                "zdr_mode"
            ]
        );
        assert_eq!(body["zdr_mode"], "disabled");
        assert_eq!(body["documents"][1]["id"], "1");
        assert_eq!(body["top_n"], 2);
    }

    #[test]
    fn check_endpoint_requires_https_or_a_loopback_address() {
        assert!(check_endpoint("https://dp.example.internal/v1/rerank").is_ok());
        assert!(check_endpoint("http://127.0.0.1:18080/v1/rerank").is_ok());
        assert!(check_endpoint("http://dp.example.internal/v1/rerank").is_err());
    }

    // ----- freeze -----

    #[test]
    fn freeze_orders_by_fusion_and_keeps_the_first_url() {
        let mut results = Vec::new();
        results.push(json!({"url": "https://a.example/one", "title": "one", "content": "c1", "engines": ["bing"], "positions": [2]}));
        results.push(json!({"url": "https://b.example/two", "title": "two", "content": "c2", "engines": ["google", "bing"], "positions": [1, 2]}));
        results.push(json!({"url": "https://c.example/three", "title": "three", "content": "c3", "engines": ["bing"], "positions": [1]}));
        results.push(json!({"url": "https://b.example/two", "title": "two again", "content": "x", "engines": ["brave"], "positions": [9]}));
        for index in 0..20 {
            results.push(json!({"url": format!("https://d.example/{index}"), "title": format!("t{index}"), "content": "", "engines": ["bing"], "positions": [10 + index]}));
        }
        let body =
            json!({"results": results, "unresponsive_engines": [["brave", "too many requests"]]});
        let (rows, frozen) =
            serp_rows_from_searxng("nav-01", &body).expect("twenty unique results");
        assert_eq!(rows.len(), CANDIDATES);
        assert_eq!(
            rows[0].url, "https://b.example/two",
            "two engines agreeing outranks one"
        );
        assert_eq!(rows[1].url, "https://c.example/three");
        assert_eq!(rows[2].url, "https://a.example/one");
        assert_eq!(
            rows.iter().map(|row| row.rank).collect::<Vec<_>>(),
            (1..=CANDIDATES).collect::<Vec<_>>()
        );
        let unique: HashSet<&str> = rows.iter().map(|row| row.url.as_str()).collect();
        assert_eq!(unique.len(), CANDIDATES, "a repeated url is dropped");
        assert_eq!(frozen.raw_results, 24);
        assert_eq!(frozen.unresponsive_engines, vec!["brave".to_owned()]);
    }

    #[test]
    fn freeze_refuses_a_query_with_fewer_than_twenty_unique_results() {
        let results: Vec<Value> = (0..19)
            .map(|index| json!({"url": format!("https://e.example/{index}"), "title": "t", "content": "c", "engines": [], "positions": []}))
            .collect();
        let body = json!({"results": results});
        assert!(serp_rows_from_searxng("nav-02", &body).is_err());
    }

    // ----- fixtures, labels and the gate -----

    #[test]
    fn a_complete_synthetic_label_set_passes_the_label_rules() {
        let fx = synthetic_fixtures(true);
        assert_eq!(validate_fixtures(&fx), Vec::<String>::new());
    }

    #[test]
    fn label_rules_reject_each_defect() {
        let mut stray = synthetic_fixtures(true);
        stray.judgments.push(JudgmentRow {
            query_id: "s00".to_owned(),
            url: "https://nowhere.example/x".to_owned(),
            judge: "human".to_owned(),
            grade: 1,
        });
        let problems = validate_fixtures(&stray);
        assert!(
            problems.iter().any(|p| p.contains("is not in the results")),
            "{problems:?}"
        );

        let mut single = synthetic_fixtures(true);
        let index = single
            .judgments
            .iter()
            .position(|row| row.judge == "human")
            .expect("a human judgment exists");
        single.judgments.remove(index);
        let problems = validate_fixtures(&single);
        assert!(
            problems.iter().any(|p| p.contains("need 2")),
            "{problems:?}"
        );

        let mut undecided = synthetic_fixtures(true);
        let index = undecided
            .judgments
            .iter()
            .position(|row| row.judge == ADJUDICATED)
            .expect("an adjudication exists");
        undecided.judgments.remove(index);
        let problems = validate_fixtures(&undecided);
        assert!(
            problems.iter().any(|p| p.contains("no adjudication")),
            "{problems:?}"
        );

        let mut disagreeing = synthetic_fixtures(true);
        for row in disagreeing.judgments.iter_mut() {
            if row.judge == "human" {
                row.grade = (row.grade + 2) % 4;
            }
        }
        disagreeing.judgments.retain(|row| row.judge != ADJUDICATED);
        let problems = validate_fixtures(&disagreeing);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("kappa") || p.contains("no adjudication")),
            "{problems:?}"
        );

        let mut pending = synthetic_fixtures(false);
        pending.queries.label_state = "pending".to_owned();
        pending.judgments.push(JudgmentRow {
            query_id: "s00".to_owned(),
            url: "https://site0.example/r1".to_owned(),
            judge: "human".to_owned(),
            grade: 1,
        });
        let problems = validate_fixtures(&pending);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("pending but judgments exist")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_gate_is_pending_without_labels_and_without_a_recording() {
        let fx = synthetic_fixtures(false);
        let report = run_gate(&fx, None, None, false);
        assert_eq!(report.verdict, "pending");
        assert!(report.reasons[0].contains("labels are pending"));

        let labelled = synthetic_fixtures(true);
        let report = run_gate(&labelled, None, None, false);
        assert_eq!(report.verdict, "pending");
        assert!(report.reasons[0].contains("no provider recording"));
    }

    #[test]
    fn the_gate_refuses_a_recording_made_on_another_serp() {
        let fx = synthetic_fixtures(true);
        let mut calls = synthetic_run(&fx, &good_scores, 0);
        calls[0].serp_digest = "fnv1a64:0000000000000000".to_owned();
        let report = run_gate(&fx, Some(&calls), None, false);
        assert_eq!(report.verdict, "invalid");
        assert!(report.reasons[0].contains("different SERP"));
    }

    #[test]
    fn a_head_window_keeps_the_tail_in_engine_order() {
        let call = RecordedCall {
            arm: "C".to_owned(),
            query_id: "q".to_owned(),
            request_id: "r".to_owned(),
            serp_digest: "d".to_owned(),
            window: 10,
            http_status: 200,
            status: "ok".to_owned(),
            reason: None,
            latency_ms: 1.0,
            results: (0..10)
                .map(|index| RecordedEntry {
                    index,
                    score: Some(index as f64),
                })
                .collect(),
        };
        let (order, fallback) = provider_order(&call, CANDIDATES);
        assert!(!fallback);
        let expected: Vec<usize> = (0..10).rev().chain(10..CANDIDATES).collect();
        assert_eq!(
            order, expected,
            "the head is reranked, the tail keeps engine order"
        );
    }

    #[test]
    fn a_recording_that_mixes_windows_is_refused() {
        let fx = synthetic_fixtures(true);
        let mut calls = synthetic_run(&fx, &good_scores, 0);
        calls[1].window = 10;
        let report = run_gate(&fx, Some(&calls), None, false);
        assert_eq!(report.verdict, "invalid");
        assert!(report.reasons[0].contains("mixes rerank windows"));
    }

    #[test]
    fn a_window_change_fails_the_regression_check() {
        let fx = synthetic_fixtures(true);
        let mut baseline = measured_baseline(&fx, Acceptance::default());
        baseline.window = Some(10);
        let run = synthetic_run(&fx, &good_scores, 0);
        let report = run_gate(&fx, Some(&run), Some(&baseline), false);
        assert_eq!(report.regression.state, "window_changed");
        assert_eq!(report.verdict, "fail");
    }

    #[test]
    fn the_gate_passes_a_clear_gain_against_its_baseline() {
        let fx = synthetic_fixtures(true);
        let baseline = measured_baseline(&fx, Acceptance::default());
        let run = synthetic_run(&fx, &good_scores, 0);
        let report = run_gate(&fx, Some(&run), Some(&baseline), false);
        assert_eq!(
            report.verdict, "pass",
            "{:?} {:?}",
            report.reasons, report.checks
        );
        assert_eq!(report.regression.state, "pass");
        assert!(report.calibration.is_some());
    }

    #[test]
    fn the_gate_fails_on_a_degraded_soak() {
        let fx = synthetic_fixtures(true);
        let baseline = measured_baseline(&fx, Acceptance::default());
        let run = synthetic_run(&fx, &good_scores, 30);
        let report = run_gate(&fx, Some(&run), Some(&baseline), false);
        assert_eq!(report.verdict, "fail");
        let soak = report
            .checks
            .iter()
            .find(|check| check.name == "soak_not_ok_rate")
            .expect("the soak check exists");
        assert_eq!(soak.pass, Some(false));
    }

    #[test]
    fn the_gate_fails_on_a_synthetic_regression() {
        let fx = synthetic_fixtures(true);
        let baseline = measured_baseline(&fx, Acceptance::default());
        let run = synthetic_run(&fx, &reversed_scores, 0);
        let report = run_gate(&fx, Some(&run), Some(&baseline), false);
        assert_eq!(report.verdict, "fail");
        assert_eq!(report.regression.state, "fail");
    }

    #[test]
    fn the_gate_never_passes_without_a_measured_baseline() {
        let fx = synthetic_fixtures(true);
        let run = synthetic_run(&fx, &good_scores, 0);
        let report = run_gate(&fx, Some(&run), None, false);
        assert_eq!(report.regression.state, "unbaselined");
        assert_ne!(report.verdict, "pass");
    }

    #[test]
    fn a_baseline_measured_on_other_fixtures_is_stale_and_fails_the_regression() {
        let fx = synthetic_fixtures(true);
        let mut baseline = measured_baseline(&fx, Acceptance::default());
        baseline.labels_digest = Some("fnv1a64:ffffffffffffffff".to_owned());
        let run = synthetic_run(&fx, &good_scores, 0);
        let report = run_gate(&fx, Some(&run), Some(&baseline), false);
        assert_eq!(report.regression.state, "fixtures_changed");
        assert_eq!(report.verdict, "fail");
    }

    #[test]
    fn the_smoke_run_passes_only_when_every_smoke_rerank_call_was_ok() {
        let fx = synthetic_fixtures(true);
        let run = synthetic_run(&fx, &good_scores, 0);
        let report = run_gate(&fx, Some(&run), None, true);
        assert_eq!(report.verdict, "pass");
        assert_eq!(report.queries, 10);
        let mut degraded = run.clone();
        if let Some(call) = degraded.iter_mut().find(|call| call.arm == "C") {
            call.status = "degraded".to_owned();
            call.reason = Some("timeout".to_owned());
            call.results.clear();
        }
        let report = run_gate(&fx, Some(&degraded), None, true);
        assert_eq!(report.verdict, "fail");
    }

    #[test]
    fn a_baseline_needs_three_independent_runs() {
        let fx = synthetic_fixtures(true);
        let runs = vec![
            synthetic_run(&fx, &good_scores, 0),
            synthetic_run(&fx, &noisy_scores(0.5), 0),
        ];
        assert!(measure_runs(&fx, &runs).is_err());
        let identical = vec![
            synthetic_run(&fx, &good_scores, 0),
            synthetic_run(&fx, &good_scores, 0),
            synthetic_run(&fx, &good_scores, 0),
        ];
        assert!(
            measure_runs(&fx, &identical).is_err(),
            "identical runs have no noise"
        );
    }

    #[test]
    fn the_arm_c_tolerance_is_twice_the_sample_sd_of_the_runs() {
        let fx = synthetic_fixtures(true);
        let measured = measure_runs(&fx, &three_runs(&fx)).expect("three runs measure");
        let expected = 2.0 * sample_sd(&measured.c_runs);
        assert!((measured.tolerances.ndcg10_arm_c - expected).abs() < 1e-12);
        assert_eq!(measured.tolerances.ndcg10_arm_a, 0.0);
        assert!(expected > 0.0);
    }

    #[test]
    fn a_measured_baseline_is_consistent_with_its_fixtures() {
        let fx = synthetic_fixtures(true);
        let baseline = measured_baseline(&fx, Acceptance::default());
        assert_eq!(baseline_problems(&baseline), Vec::<String>::new());
        assert!(!baseline_is_stale(&baseline, &fx));
    }

    #[test]
    fn paired_bootstrap_interval_spans_zero_for_noisy_zero_mean_deltas() {
        let deltas: Vec<f64> = (0..80)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        let (center, low, high) = paired_bootstrap(&deltas, 2_000, 20_261_013);
        assert!(center.abs() < 1e-12, "{center}");
        assert!(
            low < 0.0 && high > 0.0,
            "a zero-mean difference must not look significant: {low} {high}"
        );
    }

    #[test]
    fn power_is_the_minimum_detectable_effect_at_eighty_percent_power() {
        // Deltas 0 and 2 have sample SD sqrt(2), so the minimum detectable effect is Z(0.975) + Z(0.80).
        let value = power(&[0.0, 2.0]);
        assert!((value.sd_delta - 2f64.sqrt()).abs() < 1e-12, "{value:?}");
        assert!((value.mde_80 - (Z_975 + Z_80)).abs() < 1e-12, "{value:?}");
        assert!(power(&[]).mde_80.is_infinite());
    }

    #[test]
    fn the_regression_tolerance_separates_noise_from_a_real_drop() {
        let fx = synthetic_fixtures(true);
        let baseline = measured_baseline(&fx, Acceptance::default());
        let metrics = baseline.metrics.as_ref().expect("the baseline has metrics");
        let tolerance = baseline
            .tolerances
            .as_ref()
            .expect("the baseline has tolerances")
            .ndcg10_arm_c;
        assert!(tolerance > 0.0);
        let arm_c = metrics["C"].ndcg10;
        let summary = |ndcg10: f64| ArmSummary {
            ndcg10,
            mrr: 0.0,
            p_at_3: 0.0,
            noise_at_5: 0.0,
        };
        let mut report = gate_skeleton("full", &fx);
        report.window = baseline.window;
        report.arms.insert("A".to_owned(), summary(metrics["A"].ndcg10));
        report.arms.insert("C".to_owned(), summary(arm_c - 0.5 * tolerance));
        assert_eq!(regression_check(&report, Some(&baseline), &fx).state, "pass");
        report.arms.insert("C".to_owned(), summary(arm_c - 1.5 * tolerance));
        assert_eq!(regression_check(&report, Some(&baseline), &fx).state, "fail");
    }

    #[test]
    fn a_rate_limited_answer_is_a_degraded_call_never_a_retry() {
        let call = rate_limited_call("soak", "nav-01", "qwrg-r1-S-0001", "fnv1a64:0", 10, 3.0);
        assert_eq!(call.status, "degraded");
        assert_eq!(call.reason.as_deref(), Some("rate_limited"));
        assert_eq!(call.http_status, 429);
        assert!(call.results.is_empty());
        let (order, fallback) = provider_order(&call, CANDIDATES);
        assert!(fallback, "a rate-limited call keeps the engine order");
        assert_eq!(order, (0..CANDIDATES).collect::<Vec<usize>>());
        let soak = soak_summary(&[&call]).expect("one call");
        assert_eq!(soak.not_ok, 1);
        assert_eq!(soak.not_ok_rate, 1.0);
    }

    #[test]
    fn weight_refit_reorders_only_the_head_as_production_does() {
        // Ten head candidates with grade 0 and a tail of grade 3 that lexical overlap ranks first.
        // Production never moves the tail above the head, so no weight can lift it into the top ten.
        let grades: Vec<u8> = (0..CANDIDATES).map(|i| if i < 10 { 0 } else { 3 }).collect();
        let provider = vec![0.0; CANDIDATES];
        let lexical: Vec<f64> = (0..CANDIDATES).map(|i| if i < 10 { 0.0 } else { 1.0 }).collect();
        let fit = fit_weight(&[WeightCase {
            window: 10,
            provider,
            lexical,
            grades,
        }]);
        assert!(fit.best_mean_ndcg10 < 0.5, "{fit:?}");
    }

    #[test]
    fn the_report_states_the_engines_of_the_frozen_serp() {
        let fx = real_fixtures();
        let report = run_gate(&fx, None, None, false);
        let freeze = report
            .freeze
            .expect("the committed SERP carries its freeze header");
        assert!(!freeze.searxng_image.is_empty());
        assert!(!freeze.engines_seen.is_empty());
    }

    // ----- the committed fixtures and baseline -----

    #[test]
    fn real_fixtures_are_structurally_valid() {
        let fx = real_fixtures();
        assert_eq!(fx.queries.queries.len(), QUERY_COUNT);
        assert_eq!(validate_fixtures(&fx), Vec::<String>::new());
        if fx.queries.label_state == "pending" {
            assert!(
                fx.judgments.is_empty(),
                "pending labels must hold no judgment"
            );
        }
    }

    #[test]
    fn real_smoke_selection_covers_every_category_and_both_languages() {
        let fx = real_fixtures();
        let smoke = smoke_queries(&fx.queries.queries);
        assert_eq!(smoke.len(), 10);
        let categories: BTreeSet<&str> =
            smoke.iter().map(|query| query.category.as_str()).collect();
        assert_eq!(categories.len(), CATEGORIES.len());
        let languages: BTreeSet<&str> = smoke.iter().map(|query| query.lang.as_str()).collect();
        assert_eq!(languages, BTreeSet::from(["en", "no"]));
    }

    #[test]
    fn real_baseline_file_is_consistent_with_the_fixtures() {
        let fx = real_fixtures();
        let base = load_baseline(&eval_root().join(BASELINE_FILE))
            .expect("the baseline file parses")
            .expect("the baseline file is committed");
        assert_eq!(baseline_problems(&base), Vec::<String>::new());
        assert!(
            !baseline_is_stale(&base, &fx),
            "the baseline must match the committed fixtures"
        );
        if base.state == "pending" {
            assert_eq!(
                base.acceptance.min_ndcg_gain,
                Acceptance::default().min_ndcg_gain
            );
        }
    }

    #[test]
    fn real_gate_never_passes_without_a_recording() {
        let fx = real_fixtures();
        let report = run_gate(&fx, None, None, false);
        assert_ne!(report.verdict, "pass");
        if fx.queries.label_state == "pending" {
            assert_eq!(report.verdict, "pending");
        }
    }
}
