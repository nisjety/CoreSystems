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

/// Everything before the first `#[cfg(test)]` that opens an inline `mod` block:
/// test-only routes are not the surface. A `#[cfg(test)] mod name;` declaration
/// (a test-only file module) does not end production code; cutting there would
/// hide every route registered after it.
fn production_part(source: &str) -> String {
    let text = source.replace("\r\n", "\n");
    let mut offset = 0usize;
    let lines: Vec<&str> = text.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            let following = lines[index + 1..]
                .iter()
                .find(|next| !next.trim_start().starts_with("#["));
            if following.is_some_and(|next| {
                let next = next.trim_start();
                let next = next
                    .strip_prefix("pub(crate) ")
                    .or_else(|| next.strip_prefix("pub "))
                    .unwrap_or(next);
                next.starts_with("mod ") && !next.trim_end().ends_with(';')
            }) {
                return text[..offset].to_owned();
            }
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
fn route_scan_keeps_going_past_a_cfg_test_file_module_declaration() {
    let source = "#[cfg(test)]\nmod test_support;\nfn r() { Router::new().route(\"/v1/before\", get(h)); }\n#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n fn t() { Router::new().route(\"/only-in-tests\", get(h)); }\n}\n";
    assert_eq!(routes_in(source), vec!["/v1/before"]);
    let after = "#[cfg(test)]\nmod test_support;\nfn r() { Router::new().route(\"/v1/after\", get(h)); }\n";
    assert_eq!(routes_in(after), vec!["/v1/after"]);
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

/// The body of `fn name(...) { ... }` in `source`: from the opening brace to its
/// match, skipping braces inside string literals.
fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let at = source.find(&format!("fn {name}("))?;
    let open = at + source[at..].find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, c) in source[open..].char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[open..open + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn fn_body_finds_a_function_and_skips_braces_in_strings() {
    let source = "fn a() { let s = \"}\"; { nested(); } }\nfn b() { other(); }\n";
    assert_eq!(fn_body(source, "a"), Some("{ let s = \"}\"; { nested(); } }"));
    assert_eq!(fn_body(source, "b"), Some("{ other(); }"));
    assert_eq!(fn_body(source, "missing"), None);
}

/// The log column of the matrix is never blank: a row either names the test that
/// captures the process log for a ZDR request on that feature (and that test is a
/// real capture against a ZDR identity), or it says `log-unverified` in the
/// published table, so a reader of `ZDR_MATRIX.md` can tell which rows have been
/// looked at. The type allows only those two values; this pins what each means.
#[test]
fn every_row_has_a_log_canary_or_is_marked_log_unverified() {
    let markdown = zdr_matrix::to_markdown();
    let json = zdr_matrix::to_json();
    let json_rows = json["rows"].as_array().expect("rows");
    let capture = std::fs::read_to_string(crate_root().join("tests/zdr_log_capture.rs"))
        .expect("tests/zdr_log_capture.rs")
        .replace("\r\n", "\n");
    let mut canaries = 0;
    for row in ROWS {
        let line = markdown
            .lines()
            .find(|line| line.starts_with(&format!("| `{}` |", row.id)))
            .unwrap_or_else(|| panic!("{}: no line in the markdown table", row.id));
        let published = json_rows
            .iter()
            .find(|candidate| candidate["id"] == row.id)
            .unwrap_or_else(|| panic!("{}: not in the JSON", row.id));
        match row.log {
            Log::Canary { test } => {
                canaries += 1;
                let (file, name) = test
                    .split_once("::")
                    .unwrap_or_else(|| panic!("{}: canary `{test}` is not file::name", row.id));
                assert_eq!(
                    file, "zdr_log_capture",
                    "{}: a log canary lives in tests/zdr_log_capture.rs, the binary that captures the process log",
                    row.id
                );
                let body = fn_body(&capture, name)
                    .unwrap_or_else(|| panic!("{}: canary `{name}` is not in zdr_log_capture.rs", row.id));
                assert!(
                    body.contains("user_in_zdr_org"),
                    "{}: canary `{name}` never sends a request as a ZDR-org identity",
                    row.id
                );
                assert!(
                    body.contains("logged()"),
                    "{}: canary `{name}` never reads the captured log",
                    row.id
                );
                assert!(line.contains(&format!("canary `{test}`")), "{}: {line}", row.id);
                assert_eq!(published["log"], "canary", "{}", row.id);
                assert_eq!(published["log_canary"], test, "{}", row.id);
            }
            Log::Unverified => {
                assert!(line.contains("log-unverified"), "{}: {line}", row.id);
                assert!(!line.contains("canary `"), "{}: {line}", row.id);
                assert_eq!(published["log"], "log_unverified", "{}", row.id);
                assert!(published["log_canary"].is_null(), "{}", row.id);
            }
        }
    }
    // The scrape, streamed scrape, search and agent-run rows have canaries; an
    // empty or all-unverified table would make this test vacuous.
    assert!(canaries >= 4, "only {canaries} rows name a log canary");
    assert!(canaries < ROWS.len(), "no row is log-unverified: update this test's premise");
}

/// The Model Plane's `zdr-refusals` catalogue (`model-plane.zdr-refusals`, v1):
/// the closed set of refusal codes and feature keys a ZDR refusal may carry.
///
/// A COPY. The catalogue file lives at `apps/Model Plane/api/contracts/zdr-refusals.json`
/// and, when this was written, was uncommitted work in a sibling checkout, so it
/// cannot be vendored yet (WS4.I, gated on X-16, replaces these two lists with the
/// vendored file and a sha pin). Until then the lists below are the plan's "10 code
/// strings and 30 feature keys", copied from that file as it stood on 2026-10-18.
const CATALOGUE_CODES: &[&str] = &[
    "zdr_unavailable",
    "zdr_feature_not_supported",
    "zdr_connection_not_supported",
    "zdr_connection_not_acknowledged",
    "zdr_explicit_request_not_supported_by_connection",
    "zdr_decision_unavailable",
    "zdr_waiver_route_mismatch",
    "zdr_posture_mismatch",
    "zdr_durable_mutation_forbidden",
    "zdr_inference_failed",
];

const CATALOGUE_FEATURES: &[&str] = &[
    "attachments",
    "image_generation",
    "deep_research",
    "agentic",
    "plan_mode",
    "web_search",
    "client_tools",
    "structured_output",
    "skills",
    "pinned_messages",
    "spend_cap",
    "deployed_agent",
    "tool_execution",
    "agent_run",
    "run_control",
    "scheduled_step",
    "approval_continuation",
    "send_message",
    "lsp_query",
    "mcp_execution",
    "command_execution",
    "speech_synthesis",
    "speech_transcription",
    "browser_run",
    "browser_run_control",
    "translation",
    "document_analysis",
    "language_analysis",
    "realtime_voice",
    "video_generation",
];

/// What the matrix names today that the catalogue does not hold, and why.
///
/// `Treatment::Refused { code, feature }` records Quarry's own envelope: the
/// `ErrorCode` wire name the refusal answers with, and a Quarry-local feature
/// key. Quarry refusals do not yet carry `details.zdr_refusal` with a catalogue
/// code (WS4.H adds it behind ADR-4), and the catalogue has no key for these
/// features until the Model Plane initiative takes the PR that adds
/// `durable_crawl`, `browser_profile`, `session_resume` and `page_screenshot`
/// (WS4.I, X-16). Until then Quarry uses its own names, and they are listed here
/// so a new one is a visible decision, not drift.
const PENDING_CODES: &[&str] = &["BAD_REQUEST", "FORBIDDEN"];
const PENDING_FEATURES: &[&str] = &[
    "durable_job",
    "persistent_profile",
    "resume",
    "scrape_record_read",
];

fn refusals_named_by_the_matrix() -> (BTreeSet<&'static str>, BTreeSet<&'static str>) {
    use quarry_core::zdr_matrix::Treatment;
    let mut codes = BTreeSet::new();
    let mut features = BTreeSet::new();
    for row in ROWS {
        if let Treatment::Refused { code, feature } = row.treatment {
            codes.insert(code);
            features.insert(feature);
        }
    }
    (codes, features)
}

#[test]
fn every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list() {
    assert_eq!(CATALOGUE_CODES.len(), 10, "the plan's closed list has 10 codes");
    assert_eq!(CATALOGUE_FEATURES.len(), 30, "the plan's closed list has 30 feature keys");
    let (codes, features) = refusals_named_by_the_matrix();
    assert!(!codes.is_empty() && !features.is_empty(), "no refusal row names a code: the premise is gone");

    let outside_codes: Vec<&&str> = codes
        .iter()
        .filter(|code| !CATALOGUE_CODES.contains(code) && !PENDING_CODES.contains(code))
        .collect();
    let outside_features: Vec<&&str> = features
        .iter()
        .filter(|feature| !CATALOGUE_FEATURES.contains(feature) && !PENDING_FEATURES.contains(feature))
        .collect();
    assert!(
        outside_codes.is_empty() && outside_features.is_empty(),
        "the matrix names a code or feature that is neither in the closed catalogue list nor a \
         declared pending name (add it to the Model Plane catalogue, or to PENDING_* with a reason):\n  codes {outside_codes:?}\n  features {outside_features:?}"
    );

    // The pending lists are exact: a name that is no longer used, or that the
    // catalogue has since gained, must leave them.
    let stale_codes: Vec<&&str> = PENDING_CODES.iter().filter(|code| !codes.contains(*code)).collect();
    let stale_features: Vec<&&str> = PENDING_FEATURES.iter().filter(|feature| !features.contains(*feature)).collect();
    assert!(
        stale_codes.is_empty() && stale_features.is_empty(),
        "PENDING_* names the matrix does not use any more: {stale_codes:?} {stale_features:?}"
    );
    let adopted: Vec<&&str> = PENDING_CODES
        .iter()
        .filter(|code| CATALOGUE_CODES.contains(code))
        .chain(PENDING_FEATURES.iter().filter(|feature| CATALOGUE_FEATURES.contains(feature)))
        .collect();
    assert!(adopted.is_empty(), "the catalogue now holds {adopted:?}: drop them from PENDING_*");
}

/// The pending codes are Quarry's own `ErrorCode` wire names, not invented
/// strings, so a refusal that says `FORBIDDEN` really answers with that code.
#[test]
fn the_pending_codes_are_quarry_error_wire_names() {
    use quarry_core::error::ErrorCode;
    let wire: BTreeSet<&str> = [ErrorCode::Forbidden, ErrorCode::BadRequest]
        .iter()
        .map(|code| code.wire_name())
        .collect();
    for code in PENDING_CODES {
        assert!(wire.contains(code), "{code} is not an ErrorCode wire name");
    }
}
