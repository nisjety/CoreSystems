//! Pins the gateway's vendored copy of the Space event stream contract (WS02-S4).
//!
//! The validating relay in `src/domains/spaces/stream.rs` carries closed tables (event kinds and
//! their ref keys, closing codes, error codes, run states, thread-update fields). Its router-level
//! behaviour is tested next to it (`#[cfg(test)] mod tests` in `stream.rs`, because the gateway is
//! a binary crate and an integration test cannot reach the router); the tests there compare those
//! tables with `tests/fixtures/contracts/space-events.json`.
//!
//! This file closes the loop: the vendored fixture must be the same document as its two owners,
//! so a contract change in the Model Plane cannot leave the gateway tests green against a stale
//! copy. Same hard-failing style as `sse_relay_no_allowlist.rs`: if a path moves, fix the path;
//! never skip.

use std::fs;
use std::path::{Path, PathBuf};

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
        .replace("\r\n", "\n")
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_vendored_contract_equals_the_model_plane_owner() {
    let vendored = read(&manifest().join("tests/fixtures/contracts/space-events.json"));
    let owner = read(&manifest().join("../../../../Model Plane/api/contracts/space-events.json"));
    assert_eq!(
        vendored, owner,
        "tests/fixtures/contracts/space-events.json drifted from apps/Model Plane/api/contracts/space-events.json; re-vendor it"
    );
}

#[test]
fn the_vendored_contract_equals_the_spa_copy() {
    let vendored = read(&manifest().join("tests/fixtures/contracts/space-events.json"));
    let spa = read(&manifest().join("../../src/shared/api/contracts/space-events.json"));
    assert_eq!(
        vendored, spa,
        "tests/fixtures/contracts/space-events.json drifted from src/shared/api/contracts/space-events.json"
    );
}

#[test]
fn the_relay_registers_exactly_one_stream_route_and_never_reads_the_environment() {
    let source = read(&manifest().join("src/domains/spaces/stream.rs"));
    let production = source
        .split("#[cfg(test)]\nmod tests")
        .next()
        .expect("production section");
    assert_eq!(
        production.matches(".route(").count(),
        1,
        "stream.rs mounts a single route"
    );
    assert!(
        production.contains("\"/api/v1/spaces/{space_ref}/events\""),
        "the BFF stream path"
    );
    assert!(
        !production.contains("std::env") && !production.contains("env::var"),
        "knobs live on StreamRuntime and AppState, never in a handler"
    );
}
