//! quarry-eval — benchmark runner for extraction quality.
//!
//! Reads HTML fixtures + expectations, runs the transform pipeline,
//! scores results, and writes a JSON scoreboard.
//!
//! Subcommands:
//! * (none)        — legacy fixture-based scoring
//! * `scoreboard`  — cycle 31: run every `BenchmarkSuite` from
//!   `quarry_core::benchmark::builtin_suites()`
//!   against each baseline producer, emit a
//!   ScorecardEntry-shaped JSON file.

mod bench_runner;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
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

    if failed > 0 {
        std::process::exit(1);
    }
}
