# wf-D2-ws3i progress (builder unit, round 4)

Q = apps/Ingestion Plane/Quarry-v2. Design: D2-ws3i-design.md. Scratch: scratchpad/D2-ws3i/ (edit scripts e*.js, msgN.txt).

## Committed
- 61906c93 feat(quarry-runtime): escalation.rs + modes in fetch/egress_broker/fallback_driver/driver_plan + openapi deprecated line; default legacy
- 64346fd1 feat(quarry-control): stealth -> careful with deprecated alias
- 83e920f3 docs: ARCHITECTURE, CONTRACTS, GAP, OSS_PARITY_BACKLOG, PARITY_IMPLEMENTATION_PLAN, POLICY, PROGRESS, ROADMAP, RUNBOOKS (section "Block escalation"), SOVEREIGN

## Next
- [ ] preview: run quarry-runtime + quarry-edge tests with QUARRY_BLOCK_ESCALATION=honest to find what the flip breaks
- [ ] LAST commit: BUILD_DEFAULT -> Honest, test build_default_matches_the_documented_default, cache_policy.rs case "prior_block_signals 2" made mode-aware, RUNBOOKS "Where the default stands" + PROGRESS entry + CONTRACTS/RUNBOOKS wording; message says NO measurement possible
- [ ] final answer + notes file wf-D2-ws3i.md

## Verification so far
- cargo test -p quarry-runtime: lib 761 ok + integration ok (RT_RC=0), before the rustfmt-only cleanups
- edge --test log_field_discipline 22, cache_policy 9, zdr_behavior 4 ok
- go test ./... in quarry-control all ok; node --test scripts/tests 23 ok; sdks/generate.sh -> git status sdks clean
- BLOCKED by other units' uncommitted edits: cargo test -p quarry-edge --lib and cargo check --workspace --all-targets (schedule_routes.rs ControlScheduleCreate missing `zdr`), quarry-core zdr_matrix golden drift. Retry up to 5x.

## FINAL (2026-10-21)
All four commits done: 61906c93, 64346fd1, 83e920f3, 89097598 (flip, last). Final run with the flipped default: quarry-runtime 764 lib + integration ok, same lib under QUARRY_BLOCK_ESCALATION=legacy 764 ok, quarry-edge all targets ok, quarry-core ok, cargo check --workspace --all-targets ok. Notes: wf-D2-ws3i.md (incl. errata on RUNBOOKS landing in efebf3e0).

## Round 4 review fixes (in progress, uncommitted)
Files edited (mine): crates/quarry-runtime/src/{escalation.rs,fallback_driver.rs}; services/quarry-control/internal/resources/{presets.go,presets_test.go}; docs/RUNBOOKS.md (2 paragraphs + 1 table row); apps/Model Plane/rust/services/model-gateway/src/quarry.rs (DriverSignals::browser -> profile_required, 4 wire pins).
- F1 prior_block_signals callers: model-gateway moved to profile_required; verevonv3 gateway TLS retry documented as known regression (RUNBOOKS).
- F2 child-process tests for current() in escalation.rs.
- F3 fallback_driver: challenge_forbids_escalation gate + 3 tests.
- F4 careful description now says Block is advisory; test added; go test ./... ok.
DONE: all committed (654806c2, 8cf01450, 8566f2c1).

## Round 5 review fixes (in progress, uncommitted)
Files mine: crates/quarry-runtime/src/fallback_driver.rs (thinner-branch gate + 6 tests: 3-driver honest block, honest strong challenge from Tls, legacy counterparts, weak/non-signal shells, connect-failure pin), crates/quarry-edge/tests/cache_policy.rs (honest_prior_block_signals_do_not_bypass_the_page_cache), docs/RUNBOOKS.md (3 sentences in the Block escalation table/known limit; file also carries another unit's WS4.H hunks: stage with filtered git apply --cached).
Test run: qtest r1.log in scratchpad/D2-ws3i (cargo test -p quarry-runtime --lib fallback_driver). Waiting on the docker lock.
Findings 1 and 2 (commit-message counts): no code change; recorded in wf-D2-ws3i.md errata.
DONE: round 5 committed 39b0dec3

## Round "Prøv igjen" (2026-10-23)
Slice found fully built; no code change, no commit. Go resources tests ok (native). qtest could not run: image quarry-rust-env:1 not present locally (docker: pull access denied). Rust not re-run this round; last Rust run was at HEAD 39b0dec3 (775 lib ok).

## Round 6 review fixes (2026-10-23): DONE, committed cfd8b57b
F1 shell kept (fallback_driver.rs refuses_rotation + gate), F2 errata (wf-D2-ws3i.md + commit message), F3 model-gateway scrape_readable block-status gate + test. Tests: quarry-runtime 776 lib + integration ok; model-gateway quarry:: 57 ok; log_field_discipline 22 ok. Docker image quarry-rust-env:1 rebuilt from scratchpad/tools/qenv. RUNBOOKS: only my 2 hunks committed; WS4.H hunks of another unit remain uncommitted.

## Round 7 review fixes (2026-10-23): DONE, committed 181a2b3c
RUNBOOKS block-status row reworded (no shell: block returned; shell held: shell returned) + documents that the 200 hides the block from autoscale/scheduler back-off (warn log only; no status-carrying signal built). fallback_driver.rs comment re-wrapped. qtest fallback_driver lib: 44 ok. Finding 4 (model-gateway tests in container) = environment note, run natively.
