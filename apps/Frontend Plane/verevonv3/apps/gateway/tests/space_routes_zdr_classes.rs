//! Every `/api/v1/spaces` route of the gateway has a classified row in the Space surface
//! registry (WS06-S4).
//!
//! The registry is `apps/Model Plane/api/contracts/space-surface.v1.json`, vendored here as
//! `tests/fixtures/space-surface.v1.json` with its pin (scripts/tests/zdr-contract-sync.test.sh
//! keeps the copy byte-identical to the golden). A route registered without a row is a surface
//! nobody classified for Zero Data Retention, so it fails here and names the id to add.
//!
//! The scan derives the routes from the source (every `.route("/api/v1/spaces...", verb(..))`
//! of `src/domains/spaces/*.rs`, which includes the per-feature modules merged by
//! `module_routes`), never from a count, so a new route fails the test without anyone editing
//! it. The gateway is a binary crate: the scan reads the files, like the other contract tests
//! here, and fails hard when a path moves.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE: &str = "tests/fixtures/space-surface.v1.json";

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
        .replace("\r\n", "\n")
}

fn surface() -> Value {
    serde_json::from_str(&read(&manifest().join(FIXTURE))).expect("space-surface.v1.json is JSON")
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Every `(METHOD, path)` the `source` registers with `.route("<path>", verb(handler)...)`,
/// for paths under `prefix`. Test items (everything from the first `#[cfg(test)]`) are not
/// scanned. A route whose path is not a string literal is reported as `("?", "<non-literal>")`.
fn routes_in(source: &str, prefix: &str) -> Vec<(String, String)> {
    let code = source.split("#[cfg(test)]").next().unwrap_or(source);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = code[from..].find(".route(") {
        let open = from + offset + ".route(".len();
        from = open;
        let mut depth = 1_usize;
        let mut end = code.len();
        for (index, character) in code[open..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + index;
                        break;
                    }
                }
                _ => {}
            }
        }
        let arguments = &code[open..end];
        let Some(rest) = arguments.trim_start().strip_prefix('"') else {
            found.push(("?".to_owned(), "<non-literal>".to_owned()));
            continue;
        };
        let Some(close) = rest.find('"') else { continue };
        let path = &rest[..close];
        if !path.starts_with(prefix) {
            continue;
        }
        let tail = rest[close + 1..].as_bytes();
        for verb in ["get", "post", "put", "patch", "delete"] {
            let word = verb.as_bytes();
            let mut at = 0;
            while at + word.len() <= tail.len() {
                if &tail[at..at + word.len()] == word
                    && (at == 0 || !is_identifier_byte(tail[at - 1]))
                    && !tail.get(at + word.len()).is_some_and(|byte| is_identifier_byte(*byte))
                {
                    let after = tail[at + word.len()..]
                        .iter()
                        .find(|byte| !byte.is_ascii_whitespace());
                    if after == Some(&b'(') {
                        found.push((verb.to_ascii_uppercase(), path.to_owned()));
                    }
                }
                at += 1;
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn space_sources() -> Vec<(String, String)> {
    let directory = manifest().join("src/domains/spaces");
    let mut files = Vec::new();
    for entry in fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", directory.display()))
    {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
            files.push((name, read(&path)));
        }
    }
    files.sort();
    files
}

/// The `bff:<METHOD>:<path>` ids of `routes` with no row, or a row whose `zdr_class` is not
/// in the registry vocabulary, or a `DurableWrite` row that does not refuse.
fn unclassified(routes: &[(String, String)], surface: &Value) -> Vec<String> {
    let classes: Vec<&str> = surface["zdr_classes"]
        .as_object()
        .expect("zdr_classes")
        .keys()
        .map(String::as_str)
        .collect();
    let rows = surface["rows"].as_array().expect("rows");
    let mut problems = Vec::new();
    for (method, path) in routes {
        let id = format!("bff:{method}:{path}");
        match rows.iter().find(|row| row["id"] == id.as_str()) {
            None => problems.push(format!("{id} has no row")),
            Some(row) => {
                let class = row["zdr_class"].as_str().unwrap_or_default();
                if !classes.contains(&class) {
                    problems.push(format!("{id} has no valid zdr_class ({class:?})"));
                }
                if row["route_class"] == "DurableWrite" && class != "refuse" {
                    problems.push(format!("{id} is a DurableWrite route that does not refuse"));
                }
            }
        }
    }
    problems
}

#[test]
fn every_space_route_has_a_classified_row() {
    let surface = surface();
    let mut routes = Vec::new();
    for (file, source) in space_sources() {
        for (method, path) in routes_in(&source, "/api/v1/spaces") {
            assert_ne!(method, "?", "{file} registers a computed Space route");
            routes.push((method, path));
        }
    }
    assert!(
        routes.len() > 25,
        "the scan found only {} Space routes: it is not reading the router",
        routes.len()
    );
    let problems = unclassified(&routes, &surface);
    assert!(
        problems.is_empty(),
        "Space routes without a classified row in space-surface.v1.json (add the row to \
         api/contracts, re-vendor and re-pin):\n{problems:#?}"
    );
}

#[test]
fn a_synthetic_unclassified_space_route_fails_the_lint() {
    let surface = surface();
    let source = r#"
        pub(crate) fn router() -> Router {
            Router::new()
                .route("/api/v1/spaces/{space_ref}/synthetic", post(create).delete(remove))
                .route("/api/v1/spaces/{space_ref}/synthetic-read", get(read_it))
                .route("/api/v1/directory/people", get(people))
        }
    "#;
    let routes = routes_in(source, "/api/v1/spaces");
    assert_eq!(
        routes,
        [
            ("DELETE".to_owned(), "/api/v1/spaces/{space_ref}/synthetic".to_owned()),
            ("GET".to_owned(), "/api/v1/spaces/{space_ref}/synthetic-read".to_owned()),
            ("POST".to_owned(), "/api/v1/spaces/{space_ref}/synthetic".to_owned()),
        ],
        "every verb of a Space route is scanned and other prefixes are not"
    );
    let problems = unclassified(&routes, &surface);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems
        .iter()
        .all(|problem| problem.contains("/api/v1/spaces/{space_ref}/synthetic") && problem.ends_with("has no row")));
    // A computed path cannot hide, and a handler named like a verb is not a verb.
    assert_eq!(
        routes_in("Router::new().route(PATH, post(h))", "/api/v1/spaces"),
        [("?".to_owned(), "<non-literal>".to_owned())]
    );
    assert!(routes_in(
        r#"Router::new().route("/api/v1/spaces/x", get(post_things).layer(delete_layer))"#,
        "/api/v1/spaces"
    )
    .iter()
    .all(|(method, _)| method == "GET"), "a handler named like a verb is not a verb");
}

#[test]
fn a_durable_write_row_that_does_not_refuse_fails_the_lint() {
    let mut surface = surface();
    let id = "bff:PATCH:/api/v1/spaces/{space_ref}/instructions";
    let rows = surface["rows"].as_array_mut().expect("rows");
    let row = rows
        .iter_mut()
        .find(|row| row["id"] == id)
        .expect("a known DurableWrite row");
    assert_eq!(row["route_class"], "DurableWrite");
    assert_eq!(row["zdr_class"], "refuse");
    row["zdr_class"] = Value::from("n/a");
    let problems = unclassified(
        &[("PATCH".to_owned(), "/api/v1/spaces/{space_ref}/instructions".to_owned())],
        &surface,
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("DurableWrite route that does not refuse"));
}

#[test]
fn every_durable_write_row_of_the_bff_refuses() {
    let surface = surface();
    let mut durable = 0;
    for row in surface["rows"].as_array().expect("rows") {
        if row["id"].as_str().is_some_and(|id| id.starts_with("bff:")) && row["route_class"] == "DurableWrite" {
            durable += 1;
            assert_eq!(row["zdr_class"], "refuse", "{}", row["id"]);
        }
    }
    assert!(durable >= 2, "the registry lists {durable} DurableWrite BFF routes");
}

#[test]
fn the_vendored_registry_matches_its_pin_and_the_model_plane_golden() {
    let vendored = fs::read(manifest().join(FIXTURE)).expect("the vendored registry");
    let pin = read(&manifest().join("tests/fixtures/space-surface.v1.json.sha256"));
    let digest: String = Sha256::digest(&vendored)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        Some(digest.as_str()),
        pin.split_whitespace().next(),
        "re-vendor the golden and its pin (scripts/tests/zdr-contract-sync.test.sh)"
    );
    let owner = read(&manifest().join("../../../../Model Plane/api/contracts/space-surface.v1.json"));
    assert_eq!(
        read(&manifest().join(FIXTURE)),
        owner,
        "the vendored registry drifted from apps/Model Plane/api/contracts/space-surface.v1.json"
    );
}
