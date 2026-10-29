//! quarry-bakeoff — multi-engine scoreboard comparison.
//!
//! Reads two scoreboard JSON files (Quarry vs. a baseline).
//! and produces:
//!
//! - `bakeoff.json` — per-fixture wins/losses/ties
//! - `bakeoff.md` — human-readable summary table
//!
//! Baselines use the same `Scoreboard` JSON shape `quarry-eval` produces.
//! For example:
//!
//! ```bash
//! quarry-v1-eval --fixtures lab/evals/fixtures > quarry-v1-scoreboard.json
//!
//! # Compare
//! cargo run --bin quarry-bakeoff -- \
//!     --challenger lab/evals/scoreboard.json \
//!     --baseline quarry-v1-scoreboard.json \
//!     --label quarry-v1
//! ```
//!
//! When the baseline file is missing, the bakeoff still writes a "challenger
//! solo" report so CI can run without an optional comparator.

// scaffolding: dev-tool report fields parsed from JSON but not all read yet.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ExternalScoreboard {
    #[allow(dead_code)]
    generated_at: Option<String>,
    #[allow(dead_code)]
    version: Option<String>,
    results: Vec<FixtureResult>,
    summary: Option<Summary>,
}

#[derive(Debug, Deserialize, Clone)]
struct FixtureResult {
    id: String,
    #[serde(default)]
    category: String,
    pass: bool,
    duration_ms: u64,
    #[serde(default)]
    markdown_bytes: usize,
    #[serde(default)]
    link_count: usize,
    #[serde(default)]
    fingerprint: String,
}

#[derive(Debug, Deserialize, Default)]
struct Summary {
    #[serde(default)]
    total: usize,
    #[serde(default)]
    passed: usize,
    #[serde(default)]
    failed: usize,
    #[serde(default)]
    pass_rate: f64,
}

#[derive(Debug, Serialize)]
struct BakeoffReport {
    challenger_label: String,
    baseline_label: String,
    fixtures: Vec<BakeoffFixture>,
    summary: BakeoffSummary,
}

#[derive(Debug, Serialize)]
struct BakeoffFixture {
    id: String,
    category: String,
    challenger_pass: bool,
    baseline_pass: Option<bool>,
    verdict: Verdict,
    challenger_duration_ms: u64,
    baseline_duration_ms: Option<u64>,
    challenger_bytes: usize,
    baseline_bytes: Option<usize>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    /// Both engines passed.
    BothPass,
    /// Both engines failed.
    BothFail,
    /// Challenger passed, baseline failed.
    ChallengerWin,
    /// Challenger failed, baseline passed.
    BaselineWin,
    /// Baseline missing — solo run.
    Solo,
}

#[derive(Debug, Serialize)]
struct BakeoffSummary {
    fixtures: usize,
    challenger_pass: usize,
    baseline_pass: usize,
    challenger_wins: usize,
    baseline_wins: usize,
    ties_pass: usize,
    ties_fail: usize,
    solo: usize,
    challenger_pass_rate: f64,
    baseline_pass_rate: f64,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let raw: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--challenger" => {
                args.challenger = raw.get(i + 1).cloned();
                i += 2;
            }
            "--baseline" => {
                args.baseline = raw.get(i + 1).cloned();
                i += 2;
            }
            "--label" => {
                args.baseline_label = raw.get(i + 1).cloned();
                i += 2;
            }
            "--challenger-label" => {
                args.challenger_label = raw.get(i + 1).cloned();
                i += 2;
            }
            "--out" => {
                args.out = raw.get(i + 1).cloned();
                i += 2;
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            _ => {
                eprintln!("unknown arg: {}", raw[i]);
                process::exit(2);
            }
        }
    }
    args
}

fn print_help() {
    eprintln!(
        "Usage: quarry-bakeoff --challenger <path> [--baseline <path>] [--label <name>] [--out <dir>]\n\
         \n\
         Compare two scoreboards and emit a bakeoff report.\n\
         \n\
         Options:\n\
           --challenger <path>      Path to Quarry scoreboard.json (required)\n\
           --baseline <path>        Path to baseline scoreboard.json\n\
           --label <name>           Human-readable label for the baseline (default: 'baseline')\n\
           --challenger-label <n>   Label for challenger (default: 'quarry')\n\
           --out <dir>              Output directory (default: alongside challenger)"
    );
}

#[derive(Debug, Default)]
struct Args {
    challenger: Option<String>,
    baseline: Option<String>,
    challenger_label: Option<String>,
    baseline_label: Option<String>,
    out: Option<String>,
}

fn load(path: &str) -> ExternalScoreboard {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        process::exit(1);
    });
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("failed to parse {path}: {e}");
        process::exit(1);
    })
}

fn main() {
    let args = parse_args();
    let challenger_path = args.challenger.unwrap_or_else(|| {
        eprintln!("--challenger is required");
        process::exit(2);
    });

    let challenger = load(&challenger_path);
    let challenger_label = args
        .challenger_label
        .unwrap_or_else(|| "quarry".to_string());
    let baseline_label = args
        .baseline_label
        .unwrap_or_else(|| "baseline".to_string());

    let baseline_map: BTreeMap<String, FixtureResult> = if let Some(path) = &args.baseline {
        let baseline = load(path);
        baseline
            .results
            .into_iter()
            .map(|f| (f.id.clone(), f))
            .collect()
    } else {
        BTreeMap::new()
    };

    let mut fixtures = Vec::new();
    let mut s_challenger_pass = 0usize;
    let mut s_baseline_pass = 0usize;
    let mut s_challenger_wins = 0usize;
    let mut s_baseline_wins = 0usize;
    let mut s_ties_pass = 0usize;
    let mut s_ties_fail = 0usize;
    let mut s_solo = 0usize;

    for c in &challenger.results {
        if c.pass {
            s_challenger_pass += 1;
        }
        let b = baseline_map.get(&c.id);
        let (verdict, baseline_pass, baseline_duration, baseline_bytes) = match b {
            Some(b) => {
                if b.pass {
                    s_baseline_pass += 1;
                }
                let v = match (c.pass, b.pass) {
                    (true, true) => {
                        s_ties_pass += 1;
                        Verdict::BothPass
                    }
                    (false, false) => {
                        s_ties_fail += 1;
                        Verdict::BothFail
                    }
                    (true, false) => {
                        s_challenger_wins += 1;
                        Verdict::ChallengerWin
                    }
                    (false, true) => {
                        s_baseline_wins += 1;
                        Verdict::BaselineWin
                    }
                };
                (v, Some(b.pass), Some(b.duration_ms), Some(b.markdown_bytes))
            }
            None => {
                s_solo += 1;
                (Verdict::Solo, None, None, None)
            }
        };
        fixtures.push(BakeoffFixture {
            id: c.id.clone(),
            category: c.category.clone(),
            challenger_pass: c.pass,
            baseline_pass,
            verdict,
            challenger_duration_ms: c.duration_ms,
            baseline_duration_ms: baseline_duration,
            challenger_bytes: c.markdown_bytes,
            baseline_bytes,
        });
    }

    let total = challenger.results.len();
    let summary = BakeoffSummary {
        fixtures: total,
        challenger_pass: s_challenger_pass,
        baseline_pass: s_baseline_pass,
        challenger_wins: s_challenger_wins,
        baseline_wins: s_baseline_wins,
        ties_pass: s_ties_pass,
        ties_fail: s_ties_fail,
        solo: s_solo,
        challenger_pass_rate: if total == 0 {
            0.0
        } else {
            s_challenger_pass as f64 / total as f64
        },
        baseline_pass_rate: if total == 0 || baseline_map.is_empty() {
            0.0
        } else {
            s_baseline_pass as f64 / total as f64
        },
    };

    let report = BakeoffReport {
        challenger_label: challenger_label.clone(),
        baseline_label: baseline_label.clone(),
        fixtures,
        summary,
    };

    let out_dir = args.out.map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(&challenger_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    });
    fs::create_dir_all(&out_dir).ok();

    let json_path = out_dir.join("bakeoff.json");
    let json = serde_json::to_string_pretty(&report).unwrap();
    fs::write(&json_path, &json).expect("write bakeoff.json");
    let md_path = out_dir.join("bakeoff.md");
    fs::write(&md_path, render_markdown(&report)).expect("write bakeoff.md");

    println!(
        "wrote {} ({} fixtures, {} wins / {} losses / {} ties_pass / {} ties_fail / {} solo)",
        json_path.display(),
        report.summary.fixtures,
        report.summary.challenger_wins,
        report.summary.baseline_wins,
        report.summary.ties_pass,
        report.summary.ties_fail,
        report.summary.solo
    );
}

fn render_markdown(r: &BakeoffReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# Bakeoff: {} vs {}\n\n",
        r.challenger_label, r.baseline_label
    ));
    s.push_str("## Summary\n\n");
    s.push_str(&format!(
        "- Fixtures: {}\n- {} pass: {} ({:.0}%)\n- {} pass: {} ({:.0}%)\n- {} wins: {}\n- {} wins: {}\n- Ties (both pass): {}\n- Ties (both fail): {}\n- Solo (no baseline): {}\n\n",
        r.summary.fixtures,
        r.challenger_label,
        r.summary.challenger_pass,
        r.summary.challenger_pass_rate * 100.0,
        r.baseline_label,
        r.summary.baseline_pass,
        r.summary.baseline_pass_rate * 100.0,
        r.challenger_label,
        r.summary.challenger_wins,
        r.baseline_label,
        r.summary.baseline_wins,
        r.summary.ties_pass,
        r.summary.ties_fail,
        r.summary.solo,
    ));
    s.push_str(&format!(
        "## Fixture Detail\n\n| Fixture | Category | {} | {} | Verdict | Δ ms |\n|---|---|---|---|---|---|\n",
        r.challenger_label, r.baseline_label
    ));
    for f in &r.fixtures {
        let chal = if f.challenger_pass { "PASS" } else { "FAIL" };
        let base = match f.baseline_pass {
            Some(true) => "PASS",
            Some(false) => "FAIL",
            None => "—",
        };
        let delta = match f.baseline_duration_ms {
            Some(b) => format!("{:+}", f.challenger_duration_ms as i64 - b as i64),
            None => "—".to_string(),
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {:?} | {} |\n",
            f.id, f.category, chal, base, f.verdict, delta
        ));
    }
    s
}
