# W2-U44 progress (WS12.S2, observation-level masking, stamp, events, errors, mode boot)

Status: BUILT to scope and compile-checked. NOT run: no test suite, no stack, no Flow A/B/C, no staging shadow. Nothing is deployed. The mode defaults to `enforce`; there is no flag that is "dark": masking is on by default, which is the plan's compose default.

## Committed

- `b2873b3df` feat(quarry): observation-level secret masking, redaction stamp, event and error surfaces, mode boot (WS12.S2)
  - quarry-core: `contracts.rs` (`BrowserObservation.redaction`, `ProofBundle.redaction_rule_version`), `error.rs` (`QuarryError::redacted()`), `tests/contracts.rs` (one literal, hunk-only).
  - quarry-runtime: `observation_redaction.rs` NEW (Redactor, `action_for_event`, `event_url`, `redact_json_artifact`, 21 tests incl. the runner through a fixture driver), `observation.rs` (execute wiring, `typed_field_kind`, `put_json_artifact_if_allowed`), `action_runtime.rs` (`emit_action_event` error text), `lib.rs` (+1 mod), literals in `agent_loop.rs`, `browser_procedure.rs`, `mp_client.rs`, `planner.rs`.
  - quarry-edge: `redaction_boot.rs` NEW (+5 tests), `lib.rs` and `main.rs` (mod lines and the one `init_from_env` call, hunk-only), `agent_routes/types.rs` (`driver_err` returns `err.redacted()`), `tests/redaction_wire.rs` NEW (4 tests).
  - `apps/Ingestion Plane/docker-compose.yml`: `QUARRY_EDGE__REDACTION_MODE: ${QUARRY_REDACTION_MODE:-enforce}`.
- `d707acf60` docs(quarry): WS12.S2 openapi RedactionStamp, SDK doc, secret-redaction and runbook sections (openapi, SDK TS docs by hand, SECRET_REDACTION.md, RUNBOOKS.md; hunk-only).

## Not committed

Nothing of ours. Other units' uncommitted hunks remain in the tree (main.rs, edge lib.rs, openapi.yaml, RUNBOOKS.md, tests/contracts.rs).

## Verified (what was actually run)

- `cargo check --tests -p quarry-core -p quarry-runtime -p quarry-edge` (qtest): quarry-core, quarry-runtime lib test, quarry-edge lib test (includes the `redaction_boot` unit tests) and the integration targets compiled. The quarry-edge BIN test target failed with 4 `scrape_actions` errors (another unit's `crate::scrape_actions` is used in routes.rs and scrape_contract.rs but `mod scrape_actions;` is not yet in main.rs): not ours. Re-run once that unit lands.
- `cargo check -p quarry-edge --lib --test redaction_wire --test agent_evaluate_denied` (qtest): rc 0.
- `rustfmt --edition 2021` on the three new files: clean. TRAP: rustfmt follows `mod`/`#[path]` children. Running it on `tests/redaction_wire.rs` reformatted `tests/support/agent_harness.rs` and `fixture_site.rs` (other units' files); I reverted exactly that formatting with `git apply -R` of the captured diff (numstat for both is back to 0). Never run rustfmt on a test file that declares `mod support`/`#[path]`.
- `git apply --check --cached` on both patches, gitleaks in the pre-commit hook: no leaks, both commits.

## Not run (build-first rule)

All of: `cargo test -p quarry-runtime observation_redaction`, `cargo test -p quarry-edge --lib redaction_boot`, `cargo test -p quarry-edge --test redaction_wire`, `cargo test -p quarry-core --test contracts`, `cargo test --workspace`, the openapi and docs contract node tests, Flow A/B/C, staging shadow.

## Notes for the next batch / the test phase

- The plan names the tests `execute_masks_*` under `observation.rs`; they live in `observation_redaction.rs` (its own `FixtureDriver`), so the filter `cargo test -p quarry-runtime observation_redaction` runs them. The char-boundary fix named in the plan already exists (`text_snippet_of`, test `text_snippet_cuts_on_a_character_boundary`); no duplicate was written.
- `redaction_wire.rs::a_stale_type_ref_error_body_has_no_secret` accepts 400 or 409; tighten after the first run.
- WS12.S3 (U49 gate) reads `typed_field_kind`, `observation_redaction::action_for_event` (the "local helper until S3") and `Redactor`; S3 should replace the helper with the shared `redacted_for_record(SensitiveOnly, typed_kind)` and reuse `typed_kind` for the receipt.
- The edge main.rs `init_from_env` call runs before any `redact_*` call; the first read of the mode pins the default (`enforce`), so a future call placed earlier than it would pin it.
- SDK docs for `BrowserObservation.redaction` and `RedactionStamp` were written by hand; regenerate with `sdks/generate.sh` (needs a container) at the wave end and diff.
- Depth items: `depth/W2-U44.md`.
