//! The ZDR feature matrix (`quarry_core::zdr_matrix`) and the router agree.
//!
//! The matrix says what ZDR does to each part of the surface. This file keeps it
//! honest in both directions: every `.route(...)` the edge registers has a row,
//! every row names routes that exist, and every test a row cites as its
//! evidence (or its log canary) exists. A new endpoint cannot ship without a
//! classification, and a rename cannot leave a row pointing at nothing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use quarry_core::zdr_matrix::{self, Log, ROWS};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Everything before a `#[cfg(test)] mod`: test-only routes are not the surface.
fn production_part(source: &str) -> String {
    let text = source.replace("\r\n", "\n");
    let mut offset = 0usize;
    let lines: Vec<&str> = text.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod "))
        {
            return text[..offset].to_owned();
        }
        offset += line.len() + 1;
    }
    text
}

/// The `.route("...")` literals in a source, multi-line aware.
fn routes_in(source: &str) -> Vec<String> {
    let text = production_part(source);
    let mut found = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(".route(") {
        rest = &rest[at + ".route(".len()..];
        let trimmed = rest.trim_start();
        if let Some(after_quote) = trimmed.strip_prefix('"') {
            if let Some(end) = after_quote.find('"') {
                found.push(after_quote[..end].to_owned());
            }
        }
    }
    found
}

fn route_source_files() -> Vec<PathBuf> {
    let src = crate_root().join("src");
    let mut files: Vec<PathBuf> = ["routes.rs", "fleet_routes.rs", "queue_routes.rs"]
        .iter()
        .map(|name| src.join(name))
        .collect();
    // agent_routes.rs was split into agent_routes/*.rs (WS2.B): follow either shape.
    let split = src.join("agent_routes");
    if split.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&split).unwrap().flatten().map(|e| e.path()).collect();
        entries.sort();
        files.extend(entries.into_iter().filter(|p| p.extension().is_some_and(|e| e == "rs")));
    } else {
        files.push(src.join("agent_routes.rs"));
    }
    files.into_iter().filter(|path| path.exists()).collect()
}

fn registered_routes() -> (BTreeSet<String>, usize) {
    let mut set = BTreeSet::new();
    let mut registrations = 0;
    for file in route_source_files() {
        for route in routes_in(&std::fs::read_to_string(&file).unwrap()) {
            registrations += 1;
            set.insert(zdr_matrix::normalize_route(&route));
        }
    }
    (set, registrations)
}

#[test]
fn route_scan_ignores_cfg_test_modules() {
    let source = "fn r() { Router::new().route(\"/v1/real\", get(h)).route(\n \"/v1/multi/:x\", post(h)) }\n#[cfg(test)]\nmod tests {\n fn t() { Router::new().route(\"/only-in-tests\", get(h)); }\n}\n";
    assert_eq!(routes_in(source), vec!["/v1/real", "/v1/multi/:x"]);
}

#[test]
fn route_scan_follows_the_agent_routes_split() {
    let files = route_source_files();
    assert!(files.iter().any(|p| p.ends_with("routes.rs")));
    assert!(
        files.iter().any(|p| p.to_string_lossy().replace('\\', "/").contains("agent_routes")),
        "no agent routes file found"
    );
    let (set, _) = registered_routes();
    assert!(set.contains("/v1/agent/runs"));
}

#[test]
fn the_scan_finds_a_plausible_number_of_routes() {
    let (set, registrations) = registered_routes();
    assert!(registrations >= 80, "only {registrations} registrations found: the scan is empty or broken");
    assert!(set.len() >= 60, "only {} distinct paths", set.len());
}

#[test]
fn every_registered_route_has_a_matrix_row() {
    let (registered, _) = registered_routes();
    let classified: BTreeSet<String> = zdr_matrix::classified_routes().into_iter().collect();
    let missing: Vec<&String> = registered.difference(&classified).collect();
    assert!(
        missing.is_empty(),
        "routes with no row in quarry_core::zdr_matrix::ROWS (add one, and regenerate with QUARRY_UPDATE_GOLDEN=1):\n  {}",
        missing.iter().map(|route| route.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn every_row_matches_a_registered_route() {
    let (registered, _) = registered_routes();
    let classified: BTreeSet<String> = zdr_matrix::classified_routes().into_iter().collect();
    let phantom: Vec<&String> = classified.difference(&registered).collect();
    assert!(
        phantom.is_empty(),
        "matrix rows name routes that are not registered:\n  {}",
        phantom.iter().map(|route| route.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

/// Source files under `crates/` that could hold a test: `tests/*.rs` and `src/**/*.rs`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `file::name` or `module::tests::name`: a test fn called `name` exists in a
/// Rust file whose path contains the first segment.
fn test_exists(reference: &str, files: &[(String, String)]) -> bool {
    let mut parts = reference.split("::");
    let (Some(head), Some(name)) = (parts.next(), reference.rsplit("::").next()) else {
        return false;
    };
    let needle = format!("fn {name}(");
    files
        .iter()
        .any(|(path, text)| path.contains(head) && text.contains(&needle))
}

#[test]
fn every_test_a_row_cites_exists() {
    let mut paths = Vec::new();
    rust_files(&crate_root().join(".."), &mut paths);
    let files: Vec<(String, String)> = paths
        .iter()
        .filter_map(|path| {
            Some((path.to_string_lossy().replace('\\', "/"), std::fs::read_to_string(path).ok()?))
        })
        .collect();
    let mut missing = Vec::new();
    for row in ROWS {
        for reference in row.enforced_by {
            if !test_exists(reference, &files) {
                missing.push(format!("{}: enforced_by {reference}", row.id));
            }
        }
        if let Log::Canary { test } = row.log {
            if !test_exists(test, &files) {
                missing.push(format!("{}: log canary {test}", row.id));
            }
        }
    }
    assert!(missing.is_empty(), "tests the matrix cites that do not exist:\n  {}", missing.join("\n  "));
}

#[test]
fn every_enforced_honouring_row_cites_a_test_or_says_why_it_cannot() {
    use quarry_core::zdr_matrix::{Status, Treatment};
    // An enforced row that claims ZDR is honoured, inline-only or refused needs a
    // test that would fail if that stopped being true; a row with none must be
    // one whose basis names the code (the "basis" field is mandatory), and is
    // listed here so growing this list is a visible decision.
    let without_a_test: Vec<&str> = ROWS
        .iter()
        .filter(|row| {
            row.status == Status::Enforced
                && matches!(row.treatment, Treatment::Honored | Treatment::InlineOnly | Treatment::Refused { .. })
                && row.enforced_by.is_empty()
        })
        .map(|row| row.id)
        .collect();
    assert_eq!(
        without_a_test,
        vec!["search_similar", "agent_resume", "internal_run_page"],
        "a new honoured or refused row without a test: add the test, or add it here on purpose"
    );
}
