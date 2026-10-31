# W3-B2-06 progress (E3.6-9, parse cache module)

Status: module committed (9a5ba1886); compile check passed; slice doc, depth bullets and hand-over written.

## Built (uncommitted until the commits below)

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-transform/src/parse_cache.rs` (new): `ParseCacheKey` (org id plus SHA-256 of the source bytes; empty or blank org never a key), `ParseCache<V>` (bounded, FIFO eviction, `get` and `put` take `ZdrMode` and are no-ops under ZDR via `quarry_core::zdr::guard(_, WriteKind::Cache)`), and 10 unit tests (SHA-256 known vector, org refusal, org trim, same/other org isolation, hit and miss, ZDR put and get, eviction, re-put, zero capacity, clear).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-transform/src/lib.rs`: one `pub mod parse_cache;` line.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-transform/Cargo.toml`: `sha2 = { workspace = true }` (workspace pin 0.10 already used by quarry-runtime).
- `apps/Ingestion Plane/Quarry-v2/docs/PARSE_CACHE.md` (new): slice doc (rules, caller checklist, not done). Used instead of RUNBOOKS.md, which has another unit's uncommitted diff.
- `.plan-work/quarry-w1w2/depth/W3-B2-06.md`: 7 depth bullets.

## Verified

- `cargo check --tests -p quarry-transform` via qtest.sh (see result below).
- Not run: `cargo test` (build-first rule; tests are written, compile-checked only).

## Next

1. Commit the module, Cargo.toml (and Cargo.lock if cargo check changed it), lib.rs line, and the doc in separate coherent commits.
2. Hand-over note: .plan-work/quarry-w1w2/wf-W3-B2-06.md.

## Uncommitted files of this unit

None of the sibling units' files are touched. Cargo.lock may change by the sha2 dependency edge; check before committing.

## Result (update)

- Commit 9a5ba1886: parse_cache.rs, lib.rs mod line, Cargo.toml sha2 dep.
- `cargo check --tests -p quarry-transform` (quarry-rust-env:2): first run failed on a missing `Debug` derive for ParseCacheKey; fixed; second run `Finished dev profile`, rc=0, no warnings from the crate.
- The docs commit (this file, wf-W3-B2-06.md, depth/W3-B2-06.md, docs/PARSE_CACHE.md) follows.
- Not run: cargo test (build-first rule).
