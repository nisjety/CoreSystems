# Q1-tab-initiator progress (2026-10-20) - DONE

Committed (all with the Sonnet trailer, pre-commit gitleaks hook ran, no bypass):
- dec93d1e feat(quarry-edge): tab initiator + AgentIdentity + both spellings + migration 0011 (15 files)
- 282d9025 docs(quarry): openapi.yaml tab routes (filtered, WITHOUT Q2's hunk), CONTRACTS.md, allowlist -3
- 52cfbf6d test(gateway): already in history when I got there (browser.rs clean vs HEAD); 4e333f75 earlier.

Verified on a clean "HEAD + my blobs" tree (scratchpad/Q1-tab-initiator/verify-tree) via qtest:
- cargo check --workspace --all-targets rc 0
- cargo test -p quarry-edge --no-fail-fast: first run 1 failing test (my own: the RecordingDriver double listed a closed tab forever; fixed the double in tests/support/agent_harness.rs, close_tab now removes it); second full run rc 0
- agent_run_control 28 pass (9 new), run_sweeper_integration 23, log_field_discipline 22 pass
- quarry-core 119+4+30+10 pass; quarry-runtime --lib 737 pass
- Postgres (pg-test-env.sh, --features postgres-queue): `--lib postgres_ -- --test-threads=1` 23 pass incl. both agent-identity round trips (sqlx::migrate! applies 0001..0011). In parallel 4 tests of OTHER stores (profile/baseline/event_history) fail on shared tables: pre-existing flake, pass serialised.
- node --test scripts/tests/openapi-*.test.mjs: 23 pass with filtered openapi + shrunk allowlist; yaml.v3 parse OK, 0 unresolved refs.
- gateway: cargo test --bin verevon-gateway-rs domains::browser: 105 pass (host).
- migration 0011: nullable JSONB, CHECK object; identity = ids/classes only.

Remaining uncommitted hunks in shared files (NOT mine, for Q2-ws6e / Q3-ws4h):
- quarry-edge/src/agent_routes/lifecycle.rs (19 lines: grant_budget snapshot, enforce_claim, RunBudget::start_with_claim) - Q2
- agent_routes/tests.rs (1 line `budget: None,` in a RunEntry/checkpoint literal) - Q2
- agent_routes/control.rs (budget.snapshot) - Q2
- quarry-runtime/src/step_receipts.rs (checkpoint `claim` field, 5 lines) - Q2
- docs/openapi.yaml (5 lines: budget "by" enum value `grant` + description) - Q2
- all of main.rs, lib.rs, config.rs, state.rs, budget.rs, routes.rs, test_support.rs, profile/schedule/change_routes.rs, zdr_refusal.rs etc: Q2/Q3
Trap: when Q2 commits lifecycle.rs/step_receipts.rs/tests.rs it can now just commit the working file diff (my hunks are in HEAD).
Trap: the shared Docker volume quarry-target now holds the verify-tree build; first build of the real tree recompiles workspace crates (deps cached).
Trap: pg tests in parallel flake (see above): run with --test-threads=1.
