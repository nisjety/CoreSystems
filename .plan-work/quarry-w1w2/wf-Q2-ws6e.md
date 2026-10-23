# Q2-ws6e hand-over (2026-10-20): WS6.E committed and proven

Commits (all with the Sonnet trailer, hook ran, no bypass):
- 8b350923 proto 40-44 + Go regen + vendored Quarry proto + L1 contract test (19 pass)
- 561b6d48 broker (Budget, migration 0003, tighten-only derive)
- 3dea6da5 Quarry (budget.rs, lifecycle, control, runtime validator/receipts, 7 route tests, openapi `grant`)
- df6460aa main.rs boot-check hunk only (Q3's WS4.H hunks in main.rs stay uncommitted)
- 6e8d2a60 model-gateway browser_run.rs
- 81948638 docs (RUNBOOKS WS6.E section only, STALE register, plan U5 row)

Proof: buf generate reproduces browser.pb.go byte for byte; buf breaking vs baseline.binpb clean; buf lint no new finding after adding per-field comments
(CI buf lint is report-only on this branch; scripts/gates/buf-lint-ratchet.py is not in this worktree). Broker go vet (+integration tag), go test ./... ok; tagged Postgres suite
under the lock ok (TestPg_BudgetContract, ...DurableNullableAndConstrained, MigrationIsRerunnable, DeriveContract, ZdrCreateWritesZeroRows). Quarry: cargo check --workspace --all-targets clean,
runtime --lib 739, grpc-feature subset 35, edge every binary ok, grant_budget_claim 7. model-gateway --lib 1741 pass/0 fail/4 ignored (49 browser_run).

Defects found and fixed: (1) grant_budget_claim tests used allowed_domains with a driver double that has no page, so each step 403'd after running: tests now use no domains (not weakened);
(2) budget::resolve_caps became dead code in the bin target: now #[cfg(test)]; (3) three new proto fields lacked comments (lint).

Traps: mp-test.sh image has no protobuf well-known-type protos: use scratchpad/Q2/mp-test2.sh (mounts the host winget include dir as PROTOC_INCLUDE). Quarry commits were built with Q3's
uncommitted WS4.H hunks in the tree, not alone. go test in apps/Model Plane/go/gen needs the go.work (no GOWORK=off there). RUNBOOKS diff is one hunk holding my section then Q3's:
use a built patch (scratchpad/Q2/rb.patch) rather than stage-hunks.

Not done: execution-core reading the claim / 402 mapping (sibling b); WS1.A2 credential; flipping flags, rebuilding images, live evidence; boot refusal of a bad claim mode not run in a started binary;
mutant checks not re-run in this pass.

## Review round 4 follow-up (2026-10-20)
Commits: 86827c60 broker (no-claim derive replay is a lookup; PG schema negatives on a claimed row), b841037d Quarry tests (boot refusal in a started binary; per-run pin), dba7fc7f model-gateway (probe on AlreadyExists, presented run grant clipped to the minted ceiling, 7 tests, AppState.browser_budget_claim), 6e9d90a1 RUNBOOKS (mine-only patch).
Findings: all 7 real. (6) is documented and pinned, not fixed: the claim caps one Quarry run; a per-grant total needs a use-once grant or a per-grant ledger. Left for the owner to decide.
Evidence: broker go test ok; tagged PG suite ok; mutant migration without inline CHECKs fails the schema test; quarry grant_budget_claim 8 + grant_budget_boot 2 ok; model-gateway --lib 1748 pass/0 fail/4 ignored; 4-way mutation run failed the 7 new tests.
Traps: BrowserRunStartRequest is not Clone (use a builder closure); mp-test2.sh takes the command as $1 and mounts the real tree, so a mutation can be applied and restored inside the container (check the sha256); with-lock + a trailing & inside run_in_background yields an empty log, run it without &; qwait greps after the whole command, so make the end marker the last line; clippy is not in the image.
Not done: no mutation check of the boot test (main.rs holds another unit's hunks); no live broker/org-core run; broker replay semantic change means the broker image must be rebuilt before the gateway's lookup helps in a live stack (old broker just answers 409 as before).

## Review round 5 (2026-10-20)
Commit 8410abea (model-gateway browser_run.rs only): a keyed retry that finds a run grant via the AlreadyExists lookup now runs under presented_run_budget(grant's claim, this attempt's minted claim), so a first attempt made with the flag off (claim none) or a since-lowered org ceiling no longer lets the retried run escape the ceiling. Two tests (one changed arm, one new); mutation (budget: returned) fails both; real lib 1749 pass/0 fail/4 ignored.
Finding 2 (cache holding mutant artifacts) is an operational note, not a code defect: the mutation was run inside the container on the real tree via a copy; the file was restored from the copy, touched, sha256 identical, and the full lib was rebuilt from the real file afterwards.
Not done: RUNBOOKS wording (its diff is mixed with another unit's hunks) still says "a retry finds its grant" without the clip sentence; broker image not rebuilt; no live run.
