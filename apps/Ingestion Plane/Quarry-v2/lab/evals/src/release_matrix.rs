//! quarry-release-matrix — release-candidate coverage matrix.
//!
//! Single-binary entrypoint that runs the canonical release gate:
//!
//! 1. `quarry-eval` — fixture suite (10/10 expected)
//! 2. `quarry-bakeoff` — diff vs. baseline (when present)
//! 3. `quarry-provider-matrix` — fingerprint capture (when creds set)
//!
//! Aggregates the three reports into a single `release_matrix.json` and
//! a human-readable `release_matrix.md` checklist. CI uses this as the
//! release-cut artifact: if any gate fails, the binary exits non-zero so
//! the pipeline blocks promotion.
//!
//! Inputs come from sibling artifacts (`scoreboard.json`, `bakeoff.json`,
//! `provider_matrix.report.json`) — the binary doesn't re-run anything;
//! it reports the state. Run those binaries first in your CI pipeline.
//!
//! Also writes `release_score.json`: one record per run with the commit, gate
//! counts, fixture pass rate, bakeoff net wins and captured providers. The commit
//! comes from `QUARRY_RELEASE_COMMIT` or `GITHUB_SHA`, else `unknown`. The eval CI
//! job (`.github/workflows/quarry-v2-eval.yml`) runs quarry-eval, quarry-bakeoff and
//! this binary in that order.
//!
//! Cost (E3.9.6): the score file also has a `cost` section, priced from the WS6.C usage
//! meters in `<RELEASE_MATRIX_DIR>/usage_meters.json` (`{"metrics": {"<metric>": <quantity>}}`)
//! with unit prices from `usage_prices.json` (`usd_micros_per_unit`). Both files are optional.
//! `quarry-eval` writes the meter file at the end of each fixture run (see `main.rs`).
//! With `QUARRY_EVAL_SPEND_CAP_USD` set (USD), a `spend_cap` gate fails when the estimate is
//! over the cap, and it fails closed when spend cannot be measured or priced. Unset, the cap is
//! not checked and the gates are unchanged.

// scaffolding: dev-tool report fields parsed from JSON but not all read yet.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use quarry_runtime::usage::metrics;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Default)]
struct Scoreboard {
    #[serde(default)]
    summary: ScoreboardSummary,
}

#[derive(Debug, Deserialize, Default)]
struct ScoreboardSummary {
    #[serde(default)]
    total: usize,
    #[serde(default)]
    passed: usize,
    #[serde(default)]
    failed: usize,
    #[serde(default)]
    pass_rate: f64,
}

#[derive(Debug, Deserialize, Default)]
struct Bakeoff {
    #[serde(default)]
    summary: BakeoffSummary,
    #[serde(default)]
    challenger_label: String,
    #[serde(default)]
    baseline_label: String,
}

#[derive(Debug, Deserialize, Default)]
struct BakeoffSummary {
    #[serde(default)]
    fixtures: usize,
    #[serde(default)]
    challenger_pass: usize,
    #[serde(default)]
    baseline_pass: usize,
    #[serde(default)]
    challenger_wins: usize,
    #[serde(default)]
    baseline_wins: usize,
    #[serde(default)]
    ties_pass: usize,
    #[serde(default)]
    solo: usize,
}

#[derive(Debug, Deserialize, Default)]
struct ProviderMatrix {
    #[serde(default)]
    summary: ProviderMatrixSummary,
}

#[derive(Debug, Deserialize, Default)]
struct ProviderMatrixSummary {
    #[serde(default)]
    provider_count: usize,
    #[serde(default)]
    captured_count: usize,
    #[serde(default)]
    skipped_count: usize,
    #[serde(default)]
    distinct_ja3: usize,
    #[serde(default)]
    distinct_ja4: usize,
}

#[derive(Debug, Serialize)]
struct ReleaseMatrix {
    generated_at: String,
    gates: Vec<Gate>,
    overall_pass: bool,
}

#[derive(Debug, Serialize)]
struct Gate {
    name: String,
    pass: bool,
    detail: String,
}

fn read_optional<T: for<'de> Deserialize<'de> + Default>(path: &PathBuf) -> Option<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn main() {
    let base_dir = env::var("RELEASE_MATRIX_DIR").unwrap_or_else(|_| "lab/evals".into());
    let scoreboard_path = PathBuf::from(format!("{base_dir}/scoreboard.json"));
    let bakeoff_path = PathBuf::from(format!("{base_dir}/bakeoff.json"));
    let matrix_path = PathBuf::from(format!("{base_dir}/provider_matrix.report.json"));
    let usage_path = format!("{base_dir}/usage_meters.json");
    let prices_path = format!("{base_dir}/usage_prices.json");

    let mut gates: Vec<Gate> = Vec::new();

    // Gate 1: scoreboard fixtures
    let scoreboard = read_optional::<Scoreboard>(&scoreboard_path);
    match scoreboard.as_ref() {
        Some(sb) => {
            let pass = sb.summary.failed == 0 && sb.summary.total > 0;
            gates.push(Gate {
                name: "fixture_scoreboard".into(),
                pass,
                detail: format!(
                    "{}/{} passed ({:.0}%)",
                    sb.summary.passed,
                    sb.summary.total,
                    sb.summary.pass_rate * 100.0
                ),
            });
        }
        None => gates.push(Gate {
            name: "fixture_scoreboard".into(),
            pass: false,
            detail: format!(
                "missing scoreboard at {} — run quarry-eval first",
                scoreboard_path.display()
            ),
        }),
    }

    // Gate 2: bakeoff (informational — solo runs are fine)
    let bakeoff = read_optional::<Bakeoff>(&bakeoff_path);
    match bakeoff.as_ref() {
        Some(b) => {
            // We never block release on baseline winning — bakeoffs inform.
            // We DO block on challenger losses with no compensating wins.
            let net = b.summary.challenger_wins as i64 - b.summary.baseline_wins as i64;
            let pass = net >= 0;
            gates.push(Gate {
                name: "bakeoff_diff".into(),
                pass,
                detail: format!(
                    "{} vs {}: {} wins / {} losses / {} ties / {} solo (net {:+})",
                    b.challenger_label,
                    b.baseline_label,
                    b.summary.challenger_wins,
                    b.summary.baseline_wins,
                    b.summary.ties_pass,
                    b.summary.solo,
                    net
                ),
            });
        }
        None => gates.push(Gate {
            name: "bakeoff_diff".into(),
            pass: true, // optional gate
            detail: format!("no bakeoff at {} (informational)", bakeoff_path.display()),
        }),
    }

    // Gate 3: provider matrix
    let provider = read_optional::<ProviderMatrix>(&matrix_path);
    match provider.as_ref() {
        Some(pm) => {
            // When providers configured, we expect at least one capture.
            // If providers=0, the gate passes (offline mode).
            let pass = pm.summary.provider_count == 0 || pm.summary.captured_count > 0;
            gates.push(Gate {
                name: "provider_matrix".into(),
                pass,
                detail: format!(
                    "{} providers, {} captured, {} skipped, {} distinct JA3, {} distinct JA4",
                    pm.summary.provider_count,
                    pm.summary.captured_count,
                    pm.summary.skipped_count,
                    pm.summary.distinct_ja3,
                    pm.summary.distinct_ja4
                ),
            });
        }
        None => gates.push(Gate {
            name: "provider_matrix".into(),
            pass: true, // optional
            detail: format!(
                "no provider matrix at {} (informational)",
                matrix_path.display()
            ),
        }),
    }

    // Gate 4 (E3.9.6): spend, priced from the WS6.C meters. The gate is added only when a spend
    // cap is set or the cap is invalid. The cost section is written on every run.
    let (cost, cost_gate) = build_cost(
        &usage_path,
        read_usage(&usage_path),
        read_prices(&prices_path),
        parse_spend_cap(env::var_os(SPEND_CAP_ENV).map(|v| v.to_string_lossy().into_owned())),
    );
    gates.extend(cost_gate);

    let overall_pass = gates.iter().all(|g| g.pass);
    let generated_at = Utc::now().to_rfc3339();
    let score = build_score(
        &release_commit(),
        &generated_at,
        &gates,
        scoreboard.as_ref(),
        bakeoff.as_ref(),
        provider.as_ref(),
        cost,
    );
    let score_path = format!("{base_dir}/release_score.json");
    fs::write(&score_path, serde_json::to_string_pretty(&score).unwrap())
        .expect("write release_score.json");
    let matrix = ReleaseMatrix {
        generated_at,
        gates,
        overall_pass,
    };

    let json_out = serde_json::to_string_pretty(&matrix).unwrap();
    let json_path = format!("{base_dir}/release_matrix.json");
    fs::write(&json_path, &json_out).expect("write release_matrix.json");

    let md_path = format!("{base_dir}/release_matrix.md");
    fs::write(&md_path, render_markdown(&matrix)).expect("write release_matrix.md");

    println!("{}", json_out);

    if !overall_pass {
        std::process::exit(1);
    }
}

/// Commit the score belongs to. The eval CI job exports `QUARRY_RELEASE_COMMIT`
/// (or GitHub's `GITHUB_SHA`); a local run has neither and records `unknown`.
fn release_commit() -> String {
    env::var("QUARRY_RELEASE_COMMIT")
        .or_else(|_| env::var("GITHUB_SHA"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Per-release score file (`release_score.json`): one flat record per run,
/// keyed by commit, so the SCOREBOARD rows can cite it. Fields that come from
/// an input report are `None` when that report is missing. `cost` is always written; its
/// `status` says whether the spend was measured.
#[derive(Debug, Serialize, PartialEq)]
struct ReleaseScore {
    commit: String,
    generated_at: String,
    overall_pass: bool,
    gates_total: usize,
    gates_failed: usize,
    fixtures_total: Option<usize>,
    fixtures_failed: Option<usize>,
    fixture_pass_rate: Option<f64>,
    bakeoff_net_wins: Option<i64>,
    provider_captured: Option<usize>,
    cost: CostScore,
}

fn build_score(
    commit: &str,
    generated_at: &str,
    gates: &[Gate],
    fixtures: Option<&Scoreboard>,
    bakeoff: Option<&Bakeoff>,
    provider: Option<&ProviderMatrix>,
    cost: CostScore,
) -> ReleaseScore {
    let gates_failed = gates.iter().filter(|g| !g.pass).count();
    ReleaseScore {
        commit: commit.to_string(),
        generated_at: generated_at.to_string(),
        overall_pass: gates_failed == 0,
        gates_total: gates.len(),
        gates_failed,
        fixtures_total: fixtures.map(|f| f.summary.total),
        fixtures_failed: fixtures.map(|f| f.summary.failed),
        fixture_pass_rate: fixtures.map(|f| f.summary.pass_rate),
        bakeoff_net_wins: bakeoff
            .map(|b| b.summary.challenger_wins as i64 - b.summary.baseline_wins as i64),
        provider_captured: provider.map(|p| p.summary.captured_count),
        cost,
    }
}

fn render_markdown(m: &ReleaseMatrix) -> String {
    let mut s = String::new();
    s.push_str("# Quarry v2 Release Coverage Matrix\n\n");
    s.push_str(&format!("Generated: {}\n\n", m.generated_at));
    s.push_str(&format!(
        "Overall: {}\n\n",
        if m.overall_pass { "PASS" } else { "FAIL" }
    ));
    s.push_str("| Gate | Status | Detail |\n|---|---|---|\n");
    for g in &m.gates {
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            g.name,
            if g.pass { "PASS" } else { "FAIL" },
            g.detail
        ));
    }
    s
}

/// Environment variable for this run's spend cap, in USD. Unset or blank means no cap: the cost
/// section is still written, but no `spend_cap` gate is added.
const SPEND_CAP_ENV: &str = "QUARRY_EVAL_SPEND_CAP_USD";

/// WS6.C usage meters for the eval window, read from `<base>/usage_meters.json`:
/// `{"metrics": {"<metric name>": <quantity>}}`. `deny_unknown_fields` stops a misspelt key from
/// reading as an empty snapshot, which would let a cap pass on no data.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageSnapshot {
    metrics: BTreeMap<String, f64>,
}

/// Unit prices from `<base>/usage_prices.json`: micro-USD (millionths of a dollar) per unit, keyed
/// by metric name. An entry overrides the built-in estimate for that metric.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PriceTable {
    #[serde(default)]
    usd_micros_per_unit: BTreeMap<String, f64>,
}

/// Built-in estimate for the two browser-second meters: 0.00003 USD per browser second, which is
/// 30 micro-USD (X-12, DECISIONS-ROUND-2026-10-27). It is an estimate, not a price, and the price
/// table overrides it. Every other meter has no default price, so any quantity above zero for it
/// is unpriced.
const DEFAULT_USD_MICROS_PER_UNIT: [(&str, f64); 2] = [
    (metrics::BROWSER_SECOND_AGENT, 30.0),
    (metrics::BROWSER_SECOND_INTERACTIVE, 30.0),
];

/// Cost section of `release_score.json`. `status` is `measured` only when every metered quantity
/// has a price. `not_measured`: there is no usage snapshot, or it holds no metric. `unreadable`: the snapshot or the price
/// table failed to read. `unpriced`: a metered quantity has no price, so the estimate is partial.
#[derive(Debug, Serialize, PartialEq)]
struct CostScore {
    status: &'static str,
    usage_path: String,
    input_error: Option<String>,
    /// The sum over the priced metrics. Complete only when `status` is `measured`.
    estimated_usd: Option<f64>,
    spend_cap_usd: Option<f64>,
    unpriced_metrics: Vec<String>,
    metrics: Vec<MetricCost>,
}

#[derive(Debug, Serialize, PartialEq)]
struct MetricCost {
    metric: String,
    quantity: f64,
    /// `price_table`, `default_estimate`, or null when the metric has no price.
    price_basis: Option<&'static str>,
    usd_micros_per_unit: Option<f64>,
    usd: Option<f64>,
}

impl CostScore {
    fn not_measured(usage_path: &str) -> Self {
        Self {
            status: "not_measured",
            usage_path: usage_path.to_string(),
            input_error: None,
            estimated_usd: None,
            spend_cap_usd: None,
            unpriced_metrics: Vec::new(),
            metrics: Vec::new(),
        }
    }
}

/// Reads an optional JSON input. A missing file is `Ok(None)`. A file that is present but cannot be
/// read, or does not match the expected shape, is an error, never an empty value.
fn read_json_strict<T: for<'de> Deserialize<'de>>(path: &str) -> Result<Option<T>, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<T>(&text)
            .map(Some)
            .map_err(|e| format!("{path} does not match the expected shape: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path} cannot be read: {e}")),
    }
}

/// Every value must be finite and zero or more. The error names the key, never the value.
fn check_non_negative<'a>(
    path: &str,
    values: impl Iterator<Item = (&'a str, f64)>,
) -> Result<(), String> {
    for (name, value) in values {
        if !value.is_finite() || value < 0.0 {
            return Err(format!(
                "{path}: {name} must be a finite amount of zero or more"
            ));
        }
    }
    Ok(())
}

fn read_usage(path: &str) -> Result<Option<UsageSnapshot>, String> {
    let snapshot = read_json_strict::<UsageSnapshot>(path)?;
    if let Some(s) = &snapshot {
        check_non_negative(path, s.metrics.iter().map(|(k, v)| (k.as_str(), *v)))?;
    }
    Ok(snapshot)
}

fn read_prices(path: &str) -> Result<Option<PriceTable>, String> {
    let table = read_json_strict::<PriceTable>(path)?;
    if let Some(t) = &table {
        check_non_negative(
            path,
            t.usd_micros_per_unit.iter().map(|(k, v)| (k.as_str(), *v)),
        )?;
    }
    Ok(table)
}

/// Parses `QUARRY_EVAL_SPEND_CAP_USD`. Unset or blank is no cap. Anything that is not a finite
/// amount of zero or more is an error, and the spend gate fails closed on it.
fn parse_spend_cap(raw: Option<String>) -> Result<Option<f64>, String> {
    let raw = match raw {
        Some(raw) if !raw.trim().is_empty() => raw,
        _ => return Ok(None),
    };
    match raw.trim().parse::<f64>() {
        Ok(cap) if cap.is_finite() && cap >= 0.0 => Ok(Some(cap)),
        _ => Err(format!(
            "{SPEND_CAP_ENV} must be a USD amount of zero or more, for example 1 or 0.50"
        )),
    }
}

fn default_prices() -> BTreeMap<String, (f64, &'static str)> {
    DEFAULT_USD_MICROS_PER_UNIT
        .iter()
        .map(|(metric, micros)| (metric.to_string(), (*micros, "default_estimate")))
        .collect()
}

/// Prices each metered quantity. A quantity above zero with no price is unpriced. A zero quantity
/// needs no price.
fn price_metrics(
    snapshot: &UsageSnapshot,
    prices: &BTreeMap<String, (f64, &'static str)>,
) -> (Vec<MetricCost>, Vec<String>) {
    let mut metric_costs = Vec::new();
    let mut unpriced = Vec::new();
    for (metric, quantity) in &snapshot.metrics {
        let quantity = *quantity;
        let (micros, basis) = match prices.get(metric) {
            Some((micros, basis)) => (Some(*micros), Some(*basis)),
            None => (None, None),
        };
        if micros.is_none() && quantity > 0.0 {
            unpriced.push(metric.clone());
        }
        metric_costs.push(MetricCost {
            metric: metric.clone(),
            quantity,
            price_basis: basis,
            usd_micros_per_unit: micros,
            usd: micros.map(|m| quantity * m / 1_000_000.0),
        });
    }
    (metric_costs, unpriced)
}

/// Builds the cost section and, when the spend cap is set or invalid, the `spend_cap` gate. Pure,
/// so the gate logic is testable without files.
fn build_cost(
    usage_path: &str,
    usage: Result<Option<UsageSnapshot>, String>,
    prices: Result<Option<PriceTable>, String>,
    cap: Result<Option<f64>, String>,
) -> (CostScore, Option<Gate>) {
    let mut score = CostScore::not_measured(usage_path);
    score.spend_cap_usd = cap.as_ref().ok().copied().flatten();

    let mut price_map = default_prices();
    let mut errors: Vec<String> = Vec::new();
    match prices {
        Ok(Some(table)) => {
            for (metric, micros) in table.usd_micros_per_unit {
                price_map.insert(metric, (micros, "price_table"));
            }
        }
        Ok(None) => {}
        Err(e) => errors.push(e),
    }
    let snapshot = match usage {
        Ok(snapshot) => snapshot,
        Err(e) => {
            errors.push(e);
            None
        }
    };

    // An empty snapshot means no meter was recorded in the eval process (quarry-eval writes
    // `{"metrics":{}}` when nothing ran through a metered path), not a measured zero. It stays
    // `not_measured`, so a spend cap keeps failing closed until a real meter source exists
    // (D-E3967-8).
    if !errors.is_empty() {
        score.status = "unreadable";
        score.input_error = Some(errors.join("; "));
    } else if let Some(snapshot) = snapshot.filter(|s| !s.metrics.is_empty()) {
        let (metric_costs, unpriced) = price_metrics(&snapshot, &price_map);
        score.estimated_usd = Some(metric_costs.iter().filter_map(|m| m.usd).sum::<f64>());
        score.status = if unpriced.is_empty() {
            "measured"
        } else {
            "unpriced"
        };
        score.unpriced_metrics = unpriced;
        score.metrics = metric_costs;
    }

    let gate = match cap {
        Err(e) => Some(Gate {
            name: "spend_cap".into(),
            pass: false,
            detail: e,
        }),
        Ok(None) => None,
        Ok(Some(cap_usd)) => Some(spend_gate(&score, cap_usd)),
    };
    (score, gate)
}

/// The `spend_cap` gate. It passes only when the spend is measured, fully priced and under the
/// cap.
fn spend_gate(score: &CostScore, cap_usd: f64) -> Gate {
    let (pass, detail) = match (score.status, score.estimated_usd) {
        ("measured", Some(estimate)) if estimate <= cap_usd => (
            true,
            format!("{estimate:.6} USD estimated, within the {cap_usd:.6} USD cap"),
        ),
        ("measured", Some(estimate)) => (
            false,
            format!("{estimate:.6} USD estimated exceeds the {cap_usd:.6} USD cap"),
        ),
        ("unpriced", _) => (
            false,
            format!(
                "no price for {}; spend cannot be checked against the cap",
                score.unpriced_metrics.join(", ")
            ),
        ),
        ("unreadable", _) => (
            false,
            format!(
                "usage or price input unreadable ({}); spend cannot be checked against the cap",
                score.input_error.as_deref().unwrap_or("unknown")
            ),
        ),
        _ => (
            false,
            format!(
                "no usage snapshot at {}; spend cannot be checked against the cap",
                score.usage_path
            ),
        ),
    };
    Gate {
        name: "spend_cap".into(),
        pass,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(name: &str, pass: bool) -> Gate {
        Gate {
            name: name.into(),
            pass,
            detail: String::new(),
        }
    }

    fn scoreboard(total: usize, failed: usize) -> Scoreboard {
        let passed = total - failed;
        let pass_rate = passed as f64 / total as f64;
        serde_json::from_value(serde_json::json!({
            "summary": {
                "total": total,
                "passed": passed,
                "failed": failed,
                "pass_rate": pass_rate
            }
        }))
        .expect("scoreboard fixture")
    }

    fn bakeoff(challenger_wins: usize, baseline_wins: usize) -> Bakeoff {
        serde_json::from_value(serde_json::json!({
            "challenger_label": "quarry",
            "baseline_label": "baseline",
            "summary": {
                "challenger_wins": challenger_wins,
                "baseline_wins": baseline_wins
            }
        }))
        .expect("bakeoff fixture")
    }

    fn provider(captured: usize) -> ProviderMatrix {
        serde_json::from_value(serde_json::json!({
            "summary": { "provider_count": captured, "captured_count": captured }
        }))
        .expect("provider fixture")
    }

    #[test]
    fn score_records_commit_and_fixture_rate() {
        let gates = [gate("fixture_scoreboard", true)];
        let sb = scoreboard(10, 0);
        let score = build_score(
            "abc123",
            "2026-10-23T00:00:00Z",
            &gates,
            Some(&sb),
            None,
            None,
            unmeasured(),
        );
        assert_eq!(score.commit, "abc123");
        assert_eq!(score.generated_at, "2026-10-23T00:00:00Z");
        assert!(score.overall_pass);
        assert_eq!(score.gates_total, 1);
        assert_eq!(score.gates_failed, 0);
        assert_eq!(score.fixtures_total, Some(10));
        assert_eq!(score.fixtures_failed, Some(0));
        assert_eq!(score.fixture_pass_rate, Some(1.0));
        assert_eq!(score.bakeoff_net_wins, None);
        assert_eq!(score.provider_captured, None);
    }

    #[test]
    fn score_fails_when_any_gate_fails() {
        let gates = [gate("fixture_scoreboard", false), gate("bakeoff_diff", true)];
        let sb = scoreboard(10, 2);
        let score = build_score("def456", "t", &gates, Some(&sb), None, None, unmeasured());
        assert!(!score.overall_pass);
        assert_eq!(score.gates_failed, 1);
        assert_eq!(score.fixtures_failed, Some(2));
        assert_eq!(score.fixture_pass_rate, Some(0.8));
    }

    #[test]
    fn score_carries_bakeoff_net_and_provider_capture() {
        let gates = [gate("bakeoff_diff", true), gate("provider_matrix", true)];
        let bo = bakeoff(3, 1);
        let pm = provider(2);
        let score = build_score(
            "ghi789",
            "t",
            &gates,
            None,
            Some(&bo),
            Some(&pm),
            unmeasured(),
        );
        assert_eq!(score.bakeoff_net_wins, Some(2));
        assert_eq!(score.provider_captured, Some(2));
        assert_eq!(score.fixtures_total, None);
        assert!(score.overall_pass);
    }

    const USAGE_PATH: &str = "lab/evals/usage_meters.json";

    fn unmeasured() -> CostScore {
        CostScore::not_measured(USAGE_PATH)
    }

    fn snapshot_of(entries: &[(&str, f64)]) -> UsageSnapshot {
        UsageSnapshot {
            metrics: entries
                .iter()
                .map(|(metric, quantity)| (metric.to_string(), *quantity))
                .collect(),
        }
    }

    fn measured(entries: &[(&str, f64)]) -> Result<Option<UsageSnapshot>, String> {
        Ok(Some(snapshot_of(entries)))
    }

    fn table_of(entries: &[(&str, f64)]) -> PriceTable {
        PriceTable {
            usd_micros_per_unit: entries
                .iter()
                .map(|(metric, micros)| (metric.to_string(), *micros))
                .collect(),
        }
    }

    #[test]
    fn spend_cap_accepts_plain_usd_amounts_and_blank_means_no_cap() {
        assert_eq!(parse_spend_cap(None), Ok(None));
        assert_eq!(parse_spend_cap(Some("  ".into())), Ok(None));
        assert_eq!(parse_spend_cap(Some("1".into())), Ok(Some(1.0)));
        assert_eq!(parse_spend_cap(Some(" 0.50 ".into())), Ok(Some(0.5)));
    }

    #[test]
    fn spend_cap_refuses_negative_non_finite_and_non_numeric_values() {
        for raw in ["-1", "nan", "inf", "five", "1 USD"] {
            assert!(
                parse_spend_cap(Some(raw.into())).is_err(),
                "{raw} must be refused"
            );
        }
    }

    #[test]
    fn usage_snapshot_refuses_unknown_keys_and_negative_quantities() {
        assert!(serde_json::from_str::<UsageSnapshot>(r#"{"metric": {}}"#).is_err());
        assert!(serde_json::from_str::<UsageSnapshot>(
            r#"{"metrics": {"quarry.agent.run": 1}, "window": "x"}"#
        )
        .is_err());
        assert!(check_non_negative("p", [("quarry.agent.run", -1.0)].into_iter()).is_err());
        assert!(check_non_negative("p", [("quarry.agent.run", 0.0)].into_iter()).is_ok());
    }

    #[test]
    fn spend_without_a_snapshot_is_not_measured_and_a_cap_fails_closed() {
        let (score, gate) = build_cost(USAGE_PATH, Ok(None), Ok(None), Ok(None));
        assert_eq!(score.status, "not_measured");
        assert_eq!(score.estimated_usd, None);
        assert!(gate.is_none(), "no cap, so no gate");

        let (score, gate) = build_cost(USAGE_PATH, Ok(None), Ok(None), Ok(Some(5.0)));
        assert_eq!(score.spend_cap_usd, Some(5.0));
        let gate = gate.expect("a cap adds a gate");
        assert_eq!(gate.name, "spend_cap");
        assert!(!gate.pass);
        assert!(gate.detail.contains("no usage snapshot"), "{}", gate.detail);
    }

    #[test]
    fn measured_spend_within_the_cap_passes() {
        // 1000 browser seconds at the built-in 30 micro-USD estimate is 0.03 USD.
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::BROWSER_SECOND_AGENT, 1000.0)]),
            Ok(None),
            Ok(Some(1.0)),
        );
        assert_eq!(score.status, "measured");
        assert!((score.estimated_usd.unwrap_or_default() - 0.03).abs() < 1e-9);
        let gate = gate.expect("a cap adds a gate");
        assert!(gate.pass, "{}", gate.detail);
    }

    #[test]
    fn measured_spend_over_the_cap_fails() {
        // 100000 browser seconds is 3 USD, over the 1 USD cap.
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::BROWSER_SECOND_AGENT, 100_000.0)]),
            Ok(None),
            Ok(Some(1.0)),
        );
        assert_eq!(score.status, "measured");
        let gate = gate.expect("a cap adds a gate");
        assert!(!gate.pass);
        assert!(gate.detail.contains("exceeds"), "{}", gate.detail);
    }

    #[test]
    fn spend_without_a_cap_is_observed_and_adds_no_gate() {
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::BROWSER_SECOND_AGENT, 100_000.0)]),
            Ok(None),
            Ok(None),
        );
        assert_eq!(score.status, "measured");
        assert!(gate.is_none());
    }

    #[test]
    fn an_unpriced_quantity_is_observed_without_a_cap_and_fails_a_cap() {
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::AGENT_RUN, 2.0)]),
            Ok(None),
            Ok(None),
        );
        assert_eq!(score.status, "unpriced");
        assert_eq!(score.unpriced_metrics, vec![metrics::AGENT_RUN.to_string()]);
        assert!(gate.is_none());

        let (_, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::AGENT_RUN, 2.0)]),
            Ok(None),
            Ok(Some(5.0)),
        );
        let gate = gate.expect("a cap adds a gate");
        assert!(!gate.pass);
        assert!(gate.detail.contains("no price"), "{}", gate.detail);
    }

    #[test]
    fn a_zero_quantity_needs_no_price() {
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::AGENT_RUN, 0.0)]),
            Ok(None),
            Ok(Some(5.0)),
        );
        assert_eq!(score.status, "measured");
        assert!(gate.expect("a cap adds a gate").pass);
    }

    #[test]
    fn the_price_table_overrides_the_built_in_estimate() {
        // Browser time is free in this table. Two runs at 2500 micro-USD each are 0.005 USD.
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[
                (metrics::BROWSER_SECOND_AGENT, 1000.0),
                (metrics::AGENT_RUN, 2.0),
            ]),
            Ok(Some(table_of(&[
                (metrics::BROWSER_SECOND_AGENT, 0.0),
                (metrics::AGENT_RUN, 2500.0),
            ]))),
            Ok(Some(1.0)),
        );
        assert_eq!(score.status, "measured");
        assert!((score.estimated_usd.unwrap_or_default() - 0.005).abs() < 1e-9);
        let basis = |name: &str| {
            score
                .metrics
                .iter()
                .find(|m| m.metric == name)
                .and_then(|m| m.price_basis)
        };
        assert_eq!(basis(metrics::BROWSER_SECOND_AGENT), Some("price_table"));
        assert_eq!(basis(metrics::AGENT_RUN), Some("price_table"));
        assert!(gate.expect("a cap adds a gate").pass);
    }

    #[test]
    fn an_unreadable_price_table_fails_a_cap_closed() {
        let (score, gate) = build_cost(
            USAGE_PATH,
            measured(&[(metrics::AGENT_RUN, 1.0)]),
            Err("price table is not JSON".to_string()),
            Ok(Some(5.0)),
        );
        assert_eq!(score.status, "unreadable");
        assert_eq!(score.estimated_usd, None);
        let gate = gate.expect("a cap adds a gate");
        assert!(!gate.pass);
        assert!(gate.detail.contains("unreadable"), "{}", gate.detail);
    }

    #[test]
    fn an_invalid_cap_is_a_failing_gate() {
        let (score, gate) = build_cost(
            USAGE_PATH,
            Ok(None),
            Ok(None),
            Err("cap is not a number".to_string()),
        );
        assert_eq!(score.spend_cap_usd, None);
        let gate = gate.expect("an invalid cap adds a failing gate");
        assert!(!gate.pass);
        assert_eq!(gate.detail, "cap is not a number");
    }

    #[test]
    fn the_score_file_always_carries_the_cost_section() {
        let gates = [gate("fixture_scoreboard", true)];
        let score = build_score("abc123", "t", &gates, None, None, None, unmeasured());
        let value = serde_json::to_value(&score).expect("score serializes");
        assert_eq!(value["cost"]["status"].as_str(), Some("not_measured"));
        assert_eq!(value["cost"]["usage_path"].as_str(), Some(USAGE_PATH));
    }
}
