//! Cross-plane token-request manifest contract (QB-T02).
//!
//! `contracts/cross-plane-token-requests.json` is the one list of the Auth Core
//! service tokens quarry-edge asks for. These tests make it binding, and none of
//! them reads the manifest through the loader they are checking: the file is
//! parsed here as plain JSON, so a loader that normalised a row wrongly could not
//! agree with itself and pass.
//!
//! What is pinned, and why each is a separate test:
//!
//! * the SHAPE of the file (strict fields, canonical scopes, bounded reasons), so
//!   a row Auth Core would refuse for form alone cannot be committed;
//! * the SCOPE RULES that were defects once: retrieval never holds the system
//!   write scope, rerank is a request of its own and never a Model Plane one;
//! * the PIN, so editing the manifest without refreshing the digest the Auth Core
//!   side (and the repo-level drift check) compare against fails here;
//! * the SOURCE SCAN: no production file builds a token request by hand, every
//!   `from_manifest("..")` names a row from a file that row lists, and every row
//!   that is not pending has its call site;
//! * the WIRE: each production call path, run for real against the real
//!   `AuthCoreServiceTokenProvider`, sends Auth Core exactly the manifest row
//!   (path, service id, `orgId`, `scopes`, `reason`, and nothing else). The
//!   quarry-edge audio route is covered the same way, in a unit test of
//!   `audio_routes.rs`.
//!
//! These are unit tests of the crate (this module is `#[cfg(test)]`) and not an
//! integration test target on purpose: every integration test binary links the
//! whole dependency graph again, and quarry-v2-ci.yml records linker crashes (Bus
//! error, disk full) from exactly the binaries that do.
//!
//! Nothing here is skipped when a file is missing: a scan that cannot find the
//! tree it scans, or a manifest that cannot be read, fails.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use crate::service_tokens::AuthCoreServiceTokenProvider;
use crate::{DataPlaneVectorIndex, IngestClient, VectorIndex};

const MANIFEST: &str = include_str!("../contracts/cross-plane-token-requests.json");
const PIN: &str = include_str!("../contracts/cross-plane-token-requests.json.sha256");
const MANIFEST_FILE_NAME: &str = "cross-plane-token-requests.json";

/// The one production file allowed to build a token request.
const LOADER: &str = "crates/quarry-runtime/src/service_tokens.rs";
/// This file: it names the patterns it forbids, so the scan must not read itself.
const THIS_FILE: &str = "crates/quarry-runtime/src/token_request_contract.rs";

// -- the manifest, read as plain JSON ---------------------------------------

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("the manifest is valid JSON")
}

fn rows() -> Vec<Value> {
    manifest()["rows"]
        .as_array()
        .cloned()
        .expect("`rows` is an array")
}

fn row(name: &str) -> Value {
    rows()
        .into_iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("the manifest has no row named {name}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected an array, found {value}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("expected a string, found {item}"))
                .to_owned()
        })
        .collect()
}

/// `crates/quarry-runtime` -> the workspace root (the Docker build context).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// -- shape --------------------------------------------------------------------

#[test]
fn the_manifest_has_exactly_the_documented_shape() {
    let manifest = manifest();
    let top: BTreeSet<&str> = manifest
        .as_object()
        .expect("the manifest is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        top,
        BTreeSet::from(["description", "rows", "schema_version", "service_id"]),
        "unknown or missing top-level field"
    );
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["service_id"], "quarry-edge");

    let required = [
        "name",
        "audience",
        "mint_path",
        "scopes",
        "reason",
        "granted",
        "used_by",
        "feature_flags",
        "notes",
    ];
    let optional = ["pending"];
    let mut names = BTreeSet::new();
    assert!(!rows().is_empty(), "a manifest with no rows binds nothing");
    for row in rows() {
        let object = row.as_object().expect("a row is an object");
        let name = row["name"].as_str().expect("a row has a name").to_owned();
        for key in required {
            assert!(object.contains_key(key), "row {name} lacks `{key}`");
        }
        for key in object.keys() {
            assert!(
                required.contains(&key.as_str()) || optional.contains(&key.as_str()),
                "row {name} has the unknown field `{key}`"
            );
        }

        // name: kebab-case, unique
        assert!(
            name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "row name {name:?} is not kebab-case"
        );
        assert!(names.insert(name.clone()), "row name {name} is duplicated");

        // audience and the Auth Core path that mints for it
        let audience = row["audience"].as_str().expect("audience is a string");
        let expected_path = match audience {
            "data-plane" => "/api/data-plane/internal-token",
            "model-gateway" => "/api/model-plane/internal-token",
            other => panic!(
                "row {name}: audience {other:?} is not a registry audience quarry-edge mints for"
            ),
        };
        assert_eq!(
            row["mint_path"], expected_path,
            "row {name}: mint_path does not match its audience"
        );

        // scopes: canonical, so the request is byte-identical to what Auth Core's
        // own normalisation (trim, unique) would produce
        let scopes = strings(&row["scopes"]);
        assert!(!scopes.is_empty(), "row {name} requests no scope");
        let mut canonical = scopes.clone();
        canonical.sort();
        canonical.dedup();
        assert_eq!(
            scopes, canonical,
            "row {name}: scopes must be sorted and unique"
        );
        for scope in &scopes {
            assert_eq!(
                scope.trim(),
                scope,
                "row {name}: scope {scope:?} has whitespace"
            );
            assert!(
                scope.contains(':')
                    && !scope.starts_with(':')
                    && !scope.ends_with(':')
                    && scope.chars().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '_'
                        || c == ':'),
                "row {name}: {scope:?} is not a resource:action scope"
            );
        }

        // reason: Auth Core trims it and requires 3..=500 characters. ASCII keeps
        // bytes (Rust) and UTF-16 units (Auth Core) the same number.
        let reason = row["reason"].as_str().expect("reason is a string");
        assert_eq!(
            reason.trim(),
            reason,
            "row {name}: reason has outer whitespace"
        );
        assert!(reason.is_ascii(), "row {name}: reason must be ASCII");
        assert!(
            (3..=500).contains(&reason.len()),
            "row {name}: reason must be 3..=500 characters"
        );

        assert!(
            row["granted"].is_boolean(),
            "row {name}: granted is a boolean"
        );
        assert!(
            row["notes"].as_str().is_some_and(|n| !n.trim().is_empty()),
            "row {name}: notes say what the request is for"
        );

        // used_by / pending: a row with no call site yet says so and who adds it
        let used_by = strings(&row["used_by"]);
        let unique: BTreeSet<&String> = used_by.iter().collect();
        assert_eq!(
            unique.len(),
            used_by.len(),
            "row {name}: used_by repeats a file"
        );
        for file in &used_by {
            assert!(
                file.starts_with("crates/") && file.ends_with(".rs") && !file.contains('\\'),
                "row {name}: used_by {file:?} is not a workspace-relative Rust file"
            );
            assert!(
                workspace_root().join(file).is_file(),
                "row {name}: used_by {file} does not exist"
            );
        }
        match object.get("pending") {
            Some(pending) => {
                assert!(
                    used_by.is_empty(),
                    "row {name} is pending but lists call sites: drop `pending`"
                );
                assert!(
                    pending.as_str().is_some_and(|p| !p.trim().is_empty()),
                    "row {name}: `pending` must say who adds the call site"
                );
            }
            None => assert!(
                !used_by.is_empty(),
                "row {name} has no call site and is not marked pending"
            ),
        }

        // feature flags: real edge config fields, and a request Auth Core refuses
        // must be reachable only behind named switches
        let flags = strings(&row["feature_flags"]);
        for flag in &flags {
            let field = flag.strip_prefix("QUARRY_EDGE__").unwrap_or_else(|| {
                panic!("row {name}: flag {flag} lacks the QUARRY_EDGE__ prefix")
            });
            assert!(
                !field.is_empty()
                    && field
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "row {name}: flag {flag} is not an environment variable name"
            );
        }
        if row["granted"] == false {
            assert!(
                !flags.is_empty(),
                "row {name} is refused by the registry, so it must name the switches that reach it"
            );
        }
    }
}

#[test]
fn every_feature_flag_is_a_real_edge_config_field() {
    let config = fs::read_to_string(workspace_root().join("crates/quarry-edge/src/config.rs"))
        .expect("quarry-edge's config.rs is in the build context");
    for row in rows() {
        for flag in strings(&row["feature_flags"]) {
            let field = flag
                .trim_start_matches("QUARRY_EDGE__")
                .to_ascii_lowercase();
            assert!(
                config.contains(&format!("pub {field}:")),
                "row {}: {flag} maps to the config field `{field}`, which crates/quarry-edge/src/config.rs does not declare",
                row["name"]
            );
        }
    }
}

// -- the scope rules that were defects ----------------------------------------

#[test]
fn retrieval_never_holds_the_system_write_scope() {
    let retrieve = strings(&row("vector-retrieve")["scopes"]);
    assert_eq!(retrieve, vec!["data:read"]);
    for row in rows() {
        let scopes = strings(&row["scopes"]);
        if scopes.contains(&"data:read".to_owned()) {
            assert!(
                !scopes.contains(&"org:data:read_all".to_owned()),
                "row {} mixes the read scope with org:data:read_all",
                row["name"]
            );
        }
        if scopes.contains(&"org:data:read_all".to_owned()) {
            assert_eq!(
                row["name"], "ingest",
                "org:data:read_all marks a system ingest and belongs to the ingest row alone"
            );
        }
    }
}

#[test]
fn ingest_and_rerank_are_requests_of_their_own() {
    let ingest = strings(&row("ingest")["scopes"]);
    assert_eq!(ingest, vec!["documents:write", "org:data:read_all"]);
    let rerank = strings(&row("web-rerank")["scopes"]);
    assert_eq!(rerank, vec!["data:rerank"]);
    for row in rows() {
        let scopes = strings(&row["scopes"]);
        if scopes.contains(&"data:rerank".to_owned()) {
            assert_eq!(
                (row["name"].as_str(), scopes.len()),
                (Some("web-rerank"), 1),
                "data:rerank is held by the web-rerank row alone, with no other scope"
            );
        }
    }
}

#[test]
fn rerank_never_uses_the_model_plane() {
    assert_eq!(row("web-rerank")["audience"], "data-plane");
    for row in rows() {
        let scopes = strings(&row["scopes"]);
        let model_plane = row["audience"] == "model-gateway";
        assert_eq!(
            model_plane,
            scopes.contains(&"models:invoke".to_owned()),
            "row {}: models:invoke and the model-gateway audience go together",
            row["name"]
        );
        if model_plane {
            assert_eq!(scopes, vec!["models:invoke"], "row {}", row["name"]);
            for file in strings(&row["used_by"]) {
                assert!(
                    !file.contains("rerank") && !file.contains("data_plane"),
                    "row {} is a Model Plane request but is used by {file}: rerank is a Data Plane request",
                    row["name"]
                );
            }
        }
    }
}

// -- the pin ------------------------------------------------------------------

#[test]
fn the_pin_names_exactly_these_bytes() {
    assert!(
        !MANIFEST.contains('\r'),
        "the manifest is an LF file: it is hashed byte for byte and vendored into other trees"
    );
    let digest = format!("{:x}", Sha256::digest(MANIFEST.as_bytes()));
    let mut words = PIN.split_whitespace();
    assert_eq!(
        words.next(),
        Some(digest.as_str()),
        "{MANIFEST_FILE_NAME} changed without its pin: refresh {MANIFEST_FILE_NAME}.sha256, the digest \
         registered in scripts/tests/zdr-contract-sync.test.sh, and every vendored copy listed there"
    );
    assert_eq!(
        words.next().map(|name| name.trim_start_matches('*')),
        Some(MANIFEST_FILE_NAME),
        "the pin file names the file it pins, in sha256sum format"
    );
}

// -- the source scan -------------------------------------------------------------

struct Source {
    /// Workspace-relative, `/`-separated.
    path: String,
    /// The file with every `//` comment line blanked (line numbers are kept).
    code: String,
}

/// Every Rust file under `crates/*/src` and `lab/**/src`: the production tree.
fn production_sources() -> Vec<Source> {
    let root = workspace_root();
    let mut found = Vec::new();
    let crates = root.join("crates");
    let mut crate_dirs: Vec<PathBuf> = fs::read_dir(&crates)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", crates.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.is_dir())
        .collect();
    crate_dirs.sort();
    for crate_dir in crate_dirs {
        let name = crate_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        walk(
            &crate_dir.join("src"),
            &format!("crates/{name}/src"),
            &mut found,
        );
    }
    walk(&root.join("lab"), "lab", &mut found);
    found
}

fn walk(dir: &Path, relative: &str, found: &mut Vec<Source>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries
        .map(|entry| entry.expect("directory entry"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            if name != "target" {
                walk(&path, &format!("{relative}/{name}"), found);
            }
        } else if name.ends_with(".rs") {
            let text = String::from_utf8_lossy(&fs::read(&path).expect("readable source file"))
                .into_owned();
            let code = text
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with("//") {
                        ""
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            found.push(Source {
                path: format!("{relative}/{name}"),
                code,
            });
        }
    }
}

/// `(line, Some(name))` for `from_manifest("name")`, `(line, None)` for a call
/// whose argument is not a plain string literal.
fn call_sites(source: &Source) -> Vec<(usize, Option<String>)> {
    const CALL: &str = "from_manifest(";
    source
        .code
        .match_indices(CALL)
        .map(|(at, _)| {
            let line = source.code[..at].matches('\n').count() + 1;
            let name = source.code[at + CALL.len()..]
                .trim_start()
                .strip_prefix('"')
                .and_then(|after| after.split_once('"'))
                .and_then(|(name, tail)| {
                    let tail = tail.trim_start();
                    let tail = tail.strip_prefix(',').unwrap_or(tail).trim_start();
                    tail.strip_prefix(')').map(|_| name.to_owned())
                });
            (line, name)
        })
        .collect()
}

#[test]
fn the_scan_looks_at_the_tree_it_claims_to_scan() {
    let sources = production_sources();
    let paths: BTreeSet<&str> = sources.iter().map(|source| source.path.as_str()).collect();
    for expected in [
        LOADER,
        "crates/quarry-runtime/src/ingest_client.rs",
        "crates/quarry-runtime/src/vector_index.rs",
        "crates/quarry-runtime/src/mp_client.rs",
        "crates/quarry-edge/src/audio_routes.rs",
        "crates/quarry-edge/src/main.rs",
    ] {
        assert!(
            paths.contains(expected),
            "the source scan did not see {expected}: it is not scanning the workspace"
        );
    }
}

#[test]
fn no_token_request_is_built_outside_the_manifest_loader() {
    let forbidden = [
        "ServiceTokenRequest::data_plane(",
        "ServiceTokenRequest::model_plane(",
        "ServiceTokenRequest::new(",
        "ServiceTokenRequest {",
        "ServiceTokenAudience::",
    ];
    let mut offenders = Vec::new();
    for source in production_sources() {
        if source.path == LOADER || source.path == THIS_FILE {
            continue;
        }
        for (number, line) in source.code.lines().enumerate() {
            for pattern in forbidden {
                if line.contains(pattern) {
                    offenders.push(format!("{}:{}: {}", source.path, number + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a token request is built by hand instead of from the manifest \
         (ServiceTokenRequest::from_manifest(\"<row>\")):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_call_site_names_a_row_from_a_file_that_row_lists_and_every_row_is_called() {
    let rows = rows();
    let mut called: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut problems = Vec::new();
    for source in production_sources() {
        if source.path == LOADER || source.path == THIS_FILE {
            continue;
        }
        for (line, name) in call_sites(&source) {
            match name {
                Some(name) => {
                    called.entry(name).or_default().insert(source.path.clone());
                }
                None => problems.push(format!(
                    "{}:{line}: from_manifest must be called with a string literal so this contract can see the row",
                    source.path
                )),
            }
        }
    }

    for (name, files) in &called {
        match rows.iter().find(|row| row["name"] == name.as_str()) {
            None => problems.push(format!(
                "from_manifest(\"{name}\") in {files:?} names no row of the manifest"
            )),
            Some(row) => {
                let listed: BTreeSet<String> = strings(&row["used_by"]).into_iter().collect();
                for file in files {
                    if !listed.contains(file) {
                        problems.push(format!(
                            "row {name} is requested from {file}, which its used_by does not list"
                        ));
                    }
                }
            }
        }
    }

    for row in &rows {
        let name = row["name"].as_str().unwrap();
        let files = called.get(name);
        if row.get("pending").is_some() {
            if let Some(files) = files {
                problems.push(format!(
                    "row {name} is marked pending but is already requested from {files:?}: fill used_by and drop `pending`"
                ));
            }
            continue;
        }
        for file in strings(&row["used_by"]) {
            if !files.is_some_and(|files| files.contains(&file)) {
                problems.push(format!(
                    "row {name} lists {file} in used_by, but that file never calls from_manifest(\"{name}\")"
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the manifest and the source tree disagree:\n{}",
        problems.join("\n")
    );
}

// -- the wire ----------------------------------------------------------------------

const MINTED: &str = "header.payload.signature";

struct Mint {
    path: String,
    service_id: Option<String>,
    body: Value,
}

/// An Auth Core that accepts every mint, and keeps what it was asked.
async fn auth_core() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": MINTED,
            "expiresInSeconds": 300
        })))
        .mount(&server)
        .await;
    server
}

async fn mints(auth_core: &MockServer) -> Vec<Mint> {
    auth_core
        .received_requests()
        .await
        .expect("request recording is on")
        .into_iter()
        .map(|request| Mint {
            path: request.url.path().to_owned(),
            service_id: request
                .headers
                .get("x-service-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            body: serde_json::from_slice(&request.body).expect("a mint body is JSON"),
        })
        .collect()
}

/// The REAL provider, aimed at the recording Auth Core.
fn provider(auth_core: &MockServer) -> Arc<AuthCoreServiceTokenProvider> {
    Arc::new(
        AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough")
            .expect("the provider accepts a long enough credential"),
    )
}

/// Exactly one mint reached Auth Core, and it is the manifest row: the path of
/// its audience, the service id, and a body of `orgId`, `scopes`, `reason`. No
/// field beyond those three: Auth Core refuses a request that names a retention.
fn assert_one_mint_is_row(mints: &[Mint], name: &str, org: &str) {
    let row = row(name);
    assert_eq!(mints.len(), 1, "one call path mints exactly one token");
    assert_eq!(
        mints[0].path,
        row["mint_path"].as_str().unwrap(),
        "row {name}"
    );
    assert_eq!(
        mints[0].service_id.as_deref(),
        Some("quarry-edge"),
        "row {name}"
    );
    assert_eq!(
        mints[0].body,
        json!({ "orgId": org, "scopes": row["scopes"], "reason": row["reason"] }),
        "row {name}: the mint request differs from the manifest row"
    );
}

async fn assert_bearer_reached(server: &MockServer, route: &str) {
    let reached = server
        .received_requests()
        .await
        .expect("request recording is on");
    let request = reached
        .iter()
        .find(|request| request.url.path() == route)
        .unwrap_or_else(|| panic!("{route} was never called"));
    assert_eq!(
        request
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Bearer {MINTED}"),
        "the token the call path minted is the one it sent"
    );
}

#[tokio::test]
async fn ingest_asks_auth_core_for_exactly_the_ingest_row() {
    let auth = auth_core().await;
    let data_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/documents"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "document_id": "doc_wire",
            "org_id": "org_wire",
            "source": "quarry",
            "type": "web_page",
            "title": "Wire",
            "content": "# Wire",
            "status": "pending",
            "metadata": {"url": "https://example.com/wire"},
            "zdr_classification": "internal",
            "created_at": "2026-05-08T00:00:00Z",
            "updated_at": "2026-05-08T00:00:00Z",
        })))
        .mount(&data_plane)
        .await;

    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    let request: quarry_core::contracts::DataPlaneIngestRequest = serde_json::from_value(json!({
        "run_id": run_id,
        "org_id": "org_wire",
        "source_url": "https://example.com/wire",
        "title": "Wire",
        "markdown": "# Wire",
        "metadata": {},
        "fingerprint": "blake3:wire",
        "zdr": quarry_core::zdr::ZdrMode::Off,
    }))
    .expect("a minimal ingest request");
    let client = IngestClient::with_token_provider(data_plane.uri(), provider(&auth))
        .expect("an ingest client");
    client
        .ingest(&request)
        .await
        .expect("the mock Data Plane accepts the document");

    assert_one_mint_is_row(&mints(&auth).await, "ingest", "org_wire");
    assert_bearer_reached(&data_plane, "/v1/documents").await;
}

#[tokio::test]
async fn the_vector_leg_asks_auth_core_for_exactly_the_retrieval_row() {
    let auth = auth_core().await;
    let data_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/retrieve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [],
            "sources": []
        })))
        .mount(&data_plane)
        .await;

    let index = DataPlaneVectorIndex::new(data_plane.uri()).with_token_provider(provider(&auth));
    index
        .retrieve("org_wire", "wire query", 3)
        .await
        .expect("the mock Data Plane answers");

    assert_one_mint_is_row(&mints(&auth).await, "vector-retrieve", "org_wire");
    assert_bearer_reached(&data_plane, "/v1/retrieve").await;
}

fn model_request() -> ModelPlaneInvokeRequest {
    ModelPlaneInvokeRequest {
        content: "ping".into(),
        model: None,
        session_key: None,
        thread_id: None,
    }
}

#[tokio::test]
async fn a_model_invoke_asks_auth_core_for_exactly_the_invoke_row() {
    let auth = auth_core().await;
    let model_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "request_id": "req_wire",
            "content": "pong",
            "model_used": "wire-model"
        })))
        .mount(&model_plane)
        .await;

    let client = ModelPlaneClient::new(model_plane.uri())
        .expect("a model plane client")
        .with_token_provider(provider(&auth));
    client
        .invoke_for_org("org_wire", &model_request())
        .await
        .expect("the mock Model Plane answers");

    assert_one_mint_is_row(&mints(&auth).await, "model-invoke", "org_wire");
    assert_bearer_reached(&model_plane, "/v1/invoke").await;
}

#[tokio::test]
async fn a_model_stream_asks_auth_core_for_exactly_the_stream_row() {
    let auth = auth_core().await;
    let model_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("event: delta\ndata: {\"delta\":\"pong\"}\n\n"),
        )
        .mount(&model_plane)
        .await;

    let client = ModelPlaneClient::new(model_plane.uri())
        .expect("a model plane client")
        .with_token_provider(provider(&auth));
    let stream = client
        .invoke_stream_for_org("org_wire", &model_request())
        .await
        .expect("the mock Model Plane opens the stream");
    drop(stream);

    assert_one_mint_is_row(&mints(&auth).await, "model-stream", "org_wire");
    assert_bearer_reached(&model_plane, "/v1/invoke/stream").await;
}
