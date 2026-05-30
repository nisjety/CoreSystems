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
    match read_optional::<Scoreboard>(&scoreboard_path) {
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
    match read_optional::<Bakeoff>(&bakeoff_path) {
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
    match read_optional::<ProviderMatrix>(&matrix_path) {
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
    let matrix = ReleaseMatrix {
        generated_at: Utc::now().to_rfc3339(),
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
