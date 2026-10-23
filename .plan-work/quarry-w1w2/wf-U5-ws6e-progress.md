# U5-ws6e progress (updated 17:45)

Design note: wf-U5-ws6e-design.md (done). Scratch: scratchpad/U5/ (edit scripts edit-*.js, q1.sh, mp1.log, pg-integration.log).

## Done in the tree (all edits applied; compile/test results pending the docker queue)
- Step 1 proto 40-44 + Go regen + vendored Quarry proto + scripts/tests/browser-grant-contract.test.mjs: STAGED, commit message at U5/msg1.txt,
  commit running in background (qcommit waits on the pre-commit hook / docker lock). Verified: node --test browser-grant-contract 18 passed.
- Step 2 broker: internal/grant/budget.go (new), grant.go, postgres.go, server/server.go, server/errors.go, migrations/0003 up+down (new),
  budget_test.go (grant, server), postgres_integration_test.go edited. Verified: GOWORK=off go vet ./... clean; go test -count=1 ./... ok (4 pkgs);
  go vet -tags integration ok. Integration run queued: U5/pg-integration.log (marker PG_DONE).
- Step 3/4 Quarry: grant_validator.rs (GrantBudget + budget field), grpc/browser_broker.rs (validation_from_response), step_receipts.rs
  (BudgetCheckpoint.claim; FILE ALSO HAS U3's uncommitted hunks: stage only mine), budget.rs (ClaimMode, resolve_caps_with_claim, tighter_claim,
  RunBudget::start_with_claim/snapshot, limit_source), agent_routes/lifecycle.rs (also U3 hunk: tab_origins), control.rs, main.rs (boot check),
  tests: budget.rs unit tests, tests/grant_budget_claim.rs (new), agent_routes/tests.rs (one-line `budget: None`, file has U3 hunks).
- Step 5 model-gateway: browser_run.rs (BudgetClaim, BudgetMint, mint_budget, derive request fields, tool_input bounded_run_params, tests).
- Docs script written, NOT applied: U5/edit-docs.js (RUNBOOKS append, STALE register append, plan ledger row; fill commit hashes first).
  openapi.yaml `clamped.by` enum needs `- grant` (file has U3 hunks; stage mine only).

## Queued docker jobs (do not start more): U5/q1.log (QTEST_DONE) = runtime+edge tests; U5/mp1.log (MP_DONE) = model-gateway tests;
   U5/pg-integration.log (PG_DONE) = broker Postgres integration.
## Next: read logs; fix failures; cargo check --workspace --all-targets (one more qtest); commit in steps with qcommit / hunk staging;
   final notes wf-U5-ws6e.md.
Traps found: disk hit 100% at 17:30 (docker vhdx 315 GB); cargo test takes ONE positional filter, use `-- a b`.

## 19:20 final: docker still down (checked 17:53, 19:09, 19:20). No commit possible (hook). Everything uncommitted in the tree; see wf-U5-ws6e.md for state, commit recipe and what was/was not run.
