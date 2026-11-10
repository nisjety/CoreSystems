//! quarry-eval — benchmark runner for extraction quality.
//!
//! Reads HTML fixtures + expectations, runs the transform pipeline,
//! scores results, and writes a JSON scoreboard.
//!
//! A fixture run also writes `usage_meters.json` (E3.9.6): the WS6.C usage totals that
//! `quarry-release-matrix` prices. See `write_usage_snapshot`.
//!
//! Subcommands:
//! * (none)        — legacy fixture-based scoring
//! * `scoreboard`  — cycle 31: run every `BenchmarkSuite` from
//!   `quarry_core::benchmark::builtin_suites()`
//!   against each baseline producer, emit a
//!   ScorecardEntry-shaped JSON file.

mod bench_runner;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use chrono::Utc;
use metrics::{
    Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use quarry_runtime::usage::metrics as usage_names;
use quarry_transform::{
    fingerprint::content_fingerprint, links, metadata, readability::html_to_readable_markdown,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Deserialize)]
struct FixtureManifest {
    fixtures: Vec<FixtureEntry>,
}

#[derive(Debug, Deserialize)]
struct FixtureEntry {
    id: String,
    file: String,
    category: String,
    expected: Expectations,
}

#[derive(Debug, Deserialize)]
struct Expectations {
    title: Option<String>,
    lang: Option<String>,
    must_contain: Vec<String>,
    must_not_contain: Vec<String>,
    min_links: usize,
    #[serde(default)]
    link_must_contain: Vec<String>,
    fingerprint_stable: bool,
}

#[derive(Debug, Serialize)]
struct Scoreboard {
    generated_at: String,
    version: String,
    results: Vec<FixtureResult>,
    summary: Summary,
}

#[derive(Debug, Serialize)]
struct FixtureResult {
    id: String,
    category: String,
    pass: bool,
    duration_ms: u64,
    checks: Vec<Check>,
    fingerprint: String,
    markdown_bytes: usize,
    link_count: usize,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    pass: bool,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct Summary {
    total: usize,
    passed: usize,
    failed: usize,
    pass_rate: f64,
    total_duration_ms: u64,
}

fn find_fixtures_dir() -> PathBuf {
    let candidates = [
        PathBuf::from("lab/evals/fixtures"),
        PathBuf::from("fixtures"),
        PathBuf::from("../fixtures"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    panic!("fixtures directory not found; run from repo root or lab/evals/");
}

fn run_fixture(entry: &FixtureEntry, fixtures_dir: &Path) -> FixtureResult {
    let html_path = fixtures_dir.join(&entry.file);
    let html = fs::read_to_string(&html_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", html_path.display()));

    let base_url = Url::parse("https://example.com/").unwrap();
    let start = Instant::now();

    let md = html_to_readable_markdown(&html);
    let link_list = links::extract(&html, &base_url);
    let meta = metadata::extract(&html, Some("text/html".into()));
    let fp = content_fingerprint(html.as_bytes());

    let fp2 = content_fingerprint(html.as_bytes());

    let duration_ms = start.elapsed().as_millis() as u64;

    let mut checks = Vec::new();

    if let Some(expected_title) = &entry.expected.title {
        let actual = meta.title.as_deref().unwrap_or("");
        let pass = actual == expected_title.as_str();
        checks.push(Check {
            name: "title".into(),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!("expected '{}', got '{}'", expected_title, actual))
            },
        });
    }

    if let Some(expected_lang) = &entry.expected.lang {
        let actual = meta.lang.as_deref().unwrap_or("");
        let pass = actual == expected_lang.as_str();
        checks.push(Check {
            name: "lang".into(),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!("expected '{}', got '{}'", expected_lang, actual))
            },
        });
    }

    for phrase in &entry.expected.must_contain {
        let pass = md.contains(phrase.as_str());
        checks.push(Check {
            name: format!("must_contain: {}", truncate(phrase, 40)),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!("'{}' not found in markdown output", phrase))
            },
        });
    }

    for phrase in &entry.expected.must_not_contain {
        let pass = !md.contains(phrase.as_str());
        checks.push(Check {
            name: format!("must_not_contain: {}", truncate(phrase, 40)),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!(
                    "'{}' found in markdown output (boilerplate leak)",
                    phrase
                ))
            },
        });
    }

    let link_pass = link_list.len() >= entry.expected.min_links;
    checks.push(Check {
        name: format!("min_links >= {}", entry.expected.min_links),
        pass: link_pass,
        detail: if link_pass {
            None
        } else {
            Some(format!(
                "found {} links, expected >= {}",
                link_list.len(),
                entry.expected.min_links
            ))
        },
    });

    let link_hrefs: Vec<&str> = link_list.iter().map(|l| l.href.as_str()).collect();
    for expected_link in &entry.expected.link_must_contain {
        let pass = link_hrefs
            .iter()
            .any(|h| h.contains(expected_link.as_str()));
        checks.push(Check {
            name: format!("link_must_contain: {}", truncate(expected_link, 40)),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!("'{}' not found in extracted links", expected_link))
            },
        });
    }

    if entry.expected.fingerprint_stable {
        let pass = fp.0 == fp2.0;
        checks.push(Check {
            name: "fingerprint_stable".into(),
            pass,
            detail: if pass {
                None
            } else {
                Some(format!("fingerprints diverged: {} vs {}", fp.0, fp2.0))
            },
        });
    }

    let all_pass = checks.iter().all(|c| c.pass);

    FixtureResult {
        id: entry.id.clone(),
        category: entry.category.clone(),
        pass: all_pass,
        duration_ms,
        checks,
        fingerprint: fp.0,
        markdown_bytes: md.len(),
        link_count: link_list.len(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Environment variable for the spend cap. `release_matrix.rs` reads the same name. It is repeated
/// here because a failed snapshot write exits non-zero only when a cap is set.
const SPEND_CAP_ENV: &str = "QUARRY_EVAL_SPEND_CAP_USD";

/// The usage snapshot, written next to `scoreboard.json`. `release_matrix.rs` reads this name.
const USAGE_SNAPSHOT_FILE: &str = "usage_meters.json";

/// Every canonical meter name in `quarry_runtime::usage::metrics`. The recorder sums only these, so
/// an unrelated counter cannot reach the snapshot. A meter that is recorded but has no price fails
/// the spend cap closed, so no meter is dropped from the snapshot.
const USAGE_METER_NAMES: [&str; 12] = [
    usage_names::SCRAPE_PAGE,
    usage_names::CRAWL_SEED,
    usage_names::SEARCH_QUERY,
    usage_names::ANSWER_SYNTH,
    usage_names::BATCH_URL,
    usage_names::BROWSER_SECOND_AGENT,
    usage_names::BROWSER_SECOND_INTERACTIVE,
    usage_names::AGENT_RUN,
    usage_names::AGENT_STEP,
    usage_names::CRAWL_PAGE,
    usage_names::EXTRACT_URL,
    usage_names::MAP_CALL,
];

/// Meter totals keyed by meter name.
type MeterTotals = BTreeMap<&'static str, f64>;

/// Usage totals for this run. The global recorder and `main` share one.
#[derive(Default)]
struct UsageTotals {
    totals: Mutex<MeterTotals>,
    /// Samples that could not be summed. One of them refuses the snapshot, so a sample that cannot
    /// be summed never yields a total that looks complete.
    refused: AtomicU64,
}

impl UsageTotals {
    fn lock(&self) -> MutexGuard<'_, MeterTotals> {
        self.totals.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add(&self, meter: &'static str, amount: f64) {
        *self.lock().entry(meter).or_insert(0.0) += amount;
    }

    fn refuse(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }

    /// The totals to write. Refused when a sample was refused, or a total is not a finite amount of
    /// zero or more.
    fn snapshot(&self) -> Result<MeterTotals, String> {
        let refused = self.refused.load(Ordering::Relaxed);
        if refused > 0 {
            return Err(format!("{refused} usage sample(s) could not be summed"));
        }
        let totals = self.lock().clone();
        for (meter, value) in &totals {
            if !value.is_finite() || *value < 0.0 {
                return Err(format!(
                    "{meter} total is not a finite amount of zero or more"
                ));
            }
        }
        Ok(totals)
    }
}

/// The handle the recorder gives a meter name. A counter increment and a histogram record add to
/// the total. A gauge, or an absolute counter, has no sum, so using one refuses the snapshot.
struct MeterSink {
    meter: &'static str,
    totals: Arc<UsageTotals>,
}

impl CounterFn for MeterSink {
    fn increment(&self, value: u64) {
        self.totals.add(self.meter, value as f64);
    }

    fn absolute(&self, _value: u64) {
        self.totals.refuse();
    }
}

impl HistogramFn for MeterSink {
    fn record(&self, value: f64) {
        if value.is_finite() && value >= 0.0 {
            self.totals.add(self.meter, value);
        } else {
            self.totals.refuse();
        }
    }
}

impl GaugeFn for MeterSink {
    fn increment(&self, _value: f64) {
        self.totals.refuse();
    }

    fn decrement(&self, _value: f64) {
        self.totals.refuse();
    }

    fn set(&self, _value: f64) {
        self.totals.refuse();
    }
}

/// The global recorder of the `metrics` facade. A metric outside [`USAGE_METER_NAMES`] gets a no-op
/// handle, so the run's other counters are unaffected.
struct UsageRecorder {
    totals: Arc<UsageTotals>,
}

impl UsageRecorder {
    fn sink_for(&self, key: &Key) -> Option<MeterSink> {
        let meter = USAGE_METER_NAMES
            .iter()
            .copied()
            .find(|name| *name == key.name())?;
        Some(MeterSink {
            meter,
            totals: Arc::clone(&self.totals),
        })
    }
}

impl Recorder for UsageRecorder {
    fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
        match self.sink_for(key) {
            Some(sink) => Counter::from_arc(Arc::new(sink)),
            None => Counter::noop(),
        }
    }

    fn register_gauge(&self, key: &Key, _metadata: &Metadata<'_>) -> Gauge {
        match self.sink_for(key) {
            Some(sink) => Gauge::from_arc(Arc::new(sink)),
            None => Gauge::noop(),
        }
    }

    fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
        match self.sink_for(key) {
            Some(sink) => Histogram::from_arc(Arc::new(sink)),
            None => Histogram::noop(),
        }
    }
}

/// Installs the usage recorder as the process's global `metrics` recorder. It returns the shared
/// totals, or the reason it could not be installed, so no snapshot is written that cannot be
/// vouched for.
fn install_usage_recorder() -> Result<Arc<UsageTotals>, String> {
    let totals = Arc::new(UsageTotals::default());
    metrics::set_global_recorder(UsageRecorder {
        totals: Arc::clone(&totals),
    })
    .map_err(|e| format!("usage recorder not installed: {e}"))?;
    Ok(totals)
}

/// The snapshot body. A run that recorded no meter writes an empty map, which the release matrix
/// reads as a measured zero. An absent file is never a measured zero.
#[derive(Serialize)]
struct UsageSnapshotFile<'a> {
    metrics: &'a MeterTotals,
}

/// Writes `usage_meters.json` into `dir` as LF text. It goes to a temporary file first and is then
/// renamed, so a reader never sees a partial file.
fn write_usage_file(dir: &Path, meters: &MeterTotals) -> Result<PathBuf, String> {
    let mut text = serde_json::to_string_pretty(&UsageSnapshotFile { metrics: meters })
        .map_err(|e| format!("encode {USAGE_SNAPSHOT_FILE}: {e}"))?;
    text.push('\n');
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(USAGE_SNAPSHOT_FILE);
    let tmp = dir.join(format!("{USAGE_SNAPSHOT_FILE}.tmp"));
    if let Err(e) = fs::write(&tmp, text) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("write {}: {e}", tmp.display()));
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename to {}: {e}", path.display()));
    }
    Ok(path)
}

/// Writes this run's usage snapshot and reports where it went. It returns `false` only when the
/// write failed and a spend cap is set, the one case that must exit non-zero. Without a cap a
/// failed write is a warning, and the release matrix then reports the spend as not measured.
fn write_usage_snapshot(dir: &Path, usage: &Result<Arc<UsageTotals>, String>) -> bool {
    let written = match usage {
        Ok(totals) => totals
            .snapshot()
            .and_then(|meters| write_usage_file(dir, &meters)),
        Err(e) => Err(e.clone()),
    };
    match written {
        Ok(path) => {
            println!("usage: wrote {}", path.display());
            true
        }
        Err(e) if spend_cap_is_set() => {
            eprintln!("quarry-eval: usage snapshot not written: {e}");
            false
        }
        Err(e) => {
            eprintln!("quarry-eval: warning: usage snapshot not written: {e}");
            true
        }
    }
}

/// True when a spend cap is set: the variable is present and not blank.
fn spend_cap_is_set() -> bool {
    std::env::var(SPEND_CAP_ENV).is_ok_and(|raw| !raw.trim().is_empty())
}

/// Removes the snapshot an earlier run left in `dir`. A missing file is not an error. Clearing it
/// first means a run that panics, or fails part way, leaves no snapshot for the release matrix.
fn clear_usage_snapshot(dir: &Path) {
    let _ = fs::remove_file(dir.join(USAGE_SNAPSHOT_FILE));
}

fn main() {
    // Cycle 31 — `scoreboard` subcommand runs every BenchmarkSuite
    // and writes a `ScorecardEntry[]` JSON file. Falls through to
    // the legacy fixture runner when no subcommand is given.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "scoreboard") {
        let entries = bench_runner::run_all_suites(/* dry_run = */ true);
        let json = serde_json::to_string_pretty(&entries)
            .unwrap_or_else(|e| panic!("serialize scorecard: {e}"));
        let out = find_fixtures_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join("scorecard.json");
        fs::write(&out, &json).unwrap_or_else(|e| panic!("failed to write {}: {e}", out.display()));
        println!(
            "scoreboard: wrote {} entries to {}",
            entries.len(),
            out.display()
        );
        return;
    }

    let fixtures_dir = find_fixtures_dir();
    // The usage snapshot (E3.9.6). An earlier run's snapshot is removed first, and the recorder is
    // installed before the first case, so every meter this run records is counted.
    let usage_dir = fixtures_dir
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    clear_usage_snapshot(&usage_dir);
    let usage = install_usage_recorder();
    let manifest_path = fixtures_dir.join("expectations.json");
    let manifest_str = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", manifest_path.display()));
    let manifest: FixtureManifest = serde_json::from_str(&manifest_str)
        .unwrap_or_else(|e| panic!("failed to parse expectations.json: {e}"));

    println!("quarry-eval: running {} fixtures", manifest.fixtures.len());
    println!("{}", "=".repeat(60));

    let start = Instant::now();
    let mut results = Vec::new();

    for entry in &manifest.fixtures {
        let result = run_fixture(entry, &fixtures_dir);
        let status = if result.pass { "PASS" } else { "FAIL" };
        let failed_checks: Vec<&Check> = result.checks.iter().filter(|c| !c.pass).collect();

        println!(
            "[{}] {} ({}) — {}ms, {} bytes, {} links",
            status,
            result.id,
            result.category,
            result.duration_ms,
            result.markdown_bytes,
            result.link_count
        );

        if !failed_checks.is_empty() {
            for c in &failed_checks {
                println!("  FAIL: {} — {}", c.name, c.detail.as_deref().unwrap_or(""));
            }
        }

        results.push(result);
    }

    let total_duration_ms = start.elapsed().as_millis() as u64;
    let passed = results.iter().filter(|r| r.pass).count();
    let failed = results.len() - passed;
    let pass_rate = if results.is_empty() {
        0.0
    } else {
        passed as f64 / results.len() as f64
    };

    let summary = Summary {
        total: results.len(),
        passed,
        failed,
        pass_rate,
        total_duration_ms,
    };

    println!("{}", "=".repeat(60));
    println!(
        "SUMMARY: {}/{} passed ({:.0}%) in {}ms",
        passed,
        results.len(),
        pass_rate * 100.0,
        total_duration_ms
    );

    let scoreboard = Scoreboard {
        generated_at: Utc::now().to_rfc3339(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        results,
        summary,
    };

    let out_dir = fixtures_dir.parent().unwrap_or(Path::new("."));
    let scoreboard_path = out_dir.join("scoreboard.json");
    let json = serde_json::to_string_pretty(&scoreboard).unwrap();
    fs::write(&scoreboard_path, &json)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", scoreboard_path.display()));
    println!("Scoreboard written to {}", scoreboard_path.display());

    let usage_ok = write_usage_snapshot(&usage_dir, &usage);
    if failed > 0 || !usage_ok {
        std::process::exit(1);
    }
}
