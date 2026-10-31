# W2-U49 progress (WS12.S3, receipts, typed text, checkpoints, procedures)

Status 2026-10-24: BUILT to scope, compile-checked, NOT run (build-first rule). Local commits only, nothing pushed. Flag default: `QUARRY_EDGE__RECEIPT_TEXT_MODE=sensitive`. Masking itself is on by default (the S2 mode, `enforce`); there is no "dark" switch for it, the plan's compose default.

## Committed (quarryv2-next)

- `323f69753` feat(quarry): step receipts mask typed text, observations and checkpoints; receipts carry the rule version (WS12.S3)
  - quarry-core `redaction/action.rs`: `RecordPolicy`, `init_record_policy`/`record_policy`, `RecordBranch`, `redacted_for_record` (single function, R-23), `redacted_for_human_receipt` (the WS8.B function, renamed), `redact_observation_durable`, `selector_kind`, `AgentAction::redacted_for_record`; 11 unit tests (the 7 named by the plan plus fail-closed, human wins, off/shadow, idempotence, policy parse).
  - quarry-runtime `step_receipts.rs`: `StepReceipt.redaction_rule_version`, `redact_for_read(_with)`, builder `typed_field`/`record_policy`/`redaction_mode`, masking in `complete`/`fail`/`skip`, metadata paths use `finish_*`; 9 tests in `mod redaction_tests`. `postgres_step_receipts.rs` (bind, 3 SELECTs, decode, migration test 0010 to 0013, 2 new gated tests). `browser_procedure.rs` (`redacted_input`, `compare_replay`, 3 tests). `migrations/0013_step_receipt_redaction_version.sql`. `observation_redaction.rs` (`action_for_event` delegates).
  - quarry-edge `agent_routes/step.rs` (typed kind read before `execute`, REST and WS, 4 builder chains), `agent_routes/receipts.rs` (`redact_for_read` in `list_receipts` and `compile_run_procedure`), `agent_routes/control.rs` (checkpoint `current_url` through `redact_url`), `redaction_boot.rs` (`decide_record_policy`, `init_record_policy`, called from `init_from_env`; main.rs untouched; 2 tests).
- `308ae766d` test(edge): `tests/redaction_wire.rs` +4 tests (`receipts_endpoint_returns_masked_typed_text`, `checkpoint_current_url_has_no_query_token`, `ws_action_receipt_is_masked`, `the_receipt_text_policy_boot_values`).
- `3cbf91b93` docs(quarry-v2): OpenAPI `StepReceipt.redaction_rule_version`, `ProcedureStep.redacted_input` (hunk-only; the file carries 331 other lines of other units).
- `5202d1f5f` docs(quarry): RUNBOOKS section "Receipt masking" (blob commit against HEAD; working copy of RUNBOOKS.md still lacks it and other units' sections).

Left out of the commits (other units' uncommitted hunks): `step.rs` metrics hunks (`step_started`, `record_agent_step`, U48), `step_receipts.rs` test `policy_mode` reorder.

## Verified (what was actually run)

All through qtest (rust:1 container, CARGO_BUILD_JOBS=2), on the shared working tree with other units' edits:
- `cargo check --tests -p quarry-core -p quarry-runtime`: rc 0.
- `cargo check --tests -p quarry-runtime --features postgres-queue`: rc 0.
- `cargo check --tests -p quarry-edge --features browser-agent`: rc 0 (lib tests with `redaction_boot`, the bin tests and every integration target including `redaction_wire`). Two unused-import warnings in `redaction_wire.rs` were fixed after that run (removed the imports); the fix was not re-checked, it only deletes two `use` lines that the compiler reported unused.
- `git apply --check --cached` on the combined patch; gitleaks in the pre-commit hook on each commit: no leaks.

## NOT run

Every test: `cargo test -p quarry-core redaction::action`, `cargo test -p quarry-runtime receipts procedure observation_redaction`, `cargo test -p quarry-edge --lib redaction_boot`, `cargo test -p quarry-edge --features browser-agent --test redaction_wire`, the Postgres tests, `cargo test --workspace`, the log-field lint test (the new `tracing::info!` logs a static string, `policy.as_str()`), the OpenAPI and SDK checks, the deployment evidence.

## For the next batch / the test phase

- The three `redaction_wire` tests that drive the harness assume the harness `RecordingDriver` accepts a raw `type` action and a `navigate` (they ignore the status, like the existing S2 test) and that the benign `#q` step produces a receipt; if the first run shows a refusal before the receipt, loosen the lookup, not the masking assertions.
- `StepReceipt` has a new field: a literal must set `redaction_rule_version` (or spread). `ReceiptBuilder` now masks in `complete`/`fail`/`skip`; a caller that needs the raw text must use `.redaction_mode(Mode::Off)` (the tests do, to build a legacy row).
- `ReceiptBuilder::start` reads `redaction::mode()` and `record_policy()`, which pin the process defaults on first read; `redaction_boot::init_from_env` must stay before the first receipt is built (it is: main.rs calls it at boot).
- WS8.F (attach, U-later) keeps migration 0018; 0013 is now taken, 0014 is free for the optional WS12.S7.
- Depth items: `depth/W2-U49.md` (X-19 default needs the product owner).
