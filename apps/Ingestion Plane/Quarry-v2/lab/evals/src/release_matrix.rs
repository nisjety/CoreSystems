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

// scaffolding: dev-tool report fields parsed from JSON but not all read yet.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
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

    let overall_pass = gates.iter().all(|g| g.pass);
    let generated_at = Utc::now().to_rfc3339();
    let score = build_score(
        &release_commit(),
        &generated_at,
        &gates,
        scoreboard.as_ref(),
        bakeoff.as_ref(),
        provider.as_ref(),
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
/// an input report are `None` when that report is missing.
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
}

fn build_score(
    commit: &str,
    generated_at: &str,
    gates: &[Gate],
    fixtures: Option<&Scoreboard>,
    bakeoff: Option<&Bakeoff>,
    provider: Option<&ProviderMatrix>,
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
        let score = build_score("abc123", "2026-10-23T00:00:00Z", &gates, Some(&sb), None, None);
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
        let score = build_score("def456", "t", &gates, Some(&sb), None, None);
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
        let score = build_score("ghi789", "t", &gates, None, Some(&bo), Some(&pm));
        assert_eq!(score.bakeoff_net_wins, Some(2));
        assert_eq!(score.provider_captured, Some(2));
        assert_eq!(score.fixtures_total, None);
        assert!(score.overall_pass);
    }
}
