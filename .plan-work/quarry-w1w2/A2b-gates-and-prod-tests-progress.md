# A2b-gates-and-prod-tests progress

Nothing is committed yet (see "Uncommitted files" for what is mine).

## Done (written, with the check I ran)
1. quarry-runtime `build.rs`: `build_server(true)` so tests can stand up a fake BrowserBroker.
2. `crates/quarry-runtime/src/grpc/browser_broker.rs`: G1 + G6 (one `validate_inner`): `with_service_tokens(SharedServiceTokenProvider)`, per-call `token_for_org`, ONE refresh on `Unauthenticated` (invalidate + force_refresh), second one is Forbidden; org-blind `validate` is Forbidden when tokens are set; `NotFound` and `FailedPrecondition` -> `active:false`; `PermissionDenied` Forbidden no retry; `Unavailable/Internal/DeadlineExceeded` 3 attempts (50 ms, 200 ms) then DriverFailed; mint failure returned as is, broker never called; `revoke` refused for a token validator. 11 new tests with a scripted tonic server + wiremock Auth Core. qtest `cargo test -p quarry-runtime --features grpc --lib grpc::browser_broker`: 16 passed (t1.log).
3. `crates/quarry-edge/src/grant_boot.rs` (new, `pub mod` in lib.rs and `mod` in main.rs): pure `decide()`: production needs gRPC (HTTP-only refused), gRPC needs `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` (dev bypass is the only exception, never in production), existing generic gate and grpc-feature gate folded in with their old messages. main.rs: gate call replaces the two old bails; validator built with `with_service_tokens(cross_plane_tokens)`; manifest row checked at boot (`browser_grant_token_request()?`) and the connect is eager.
4. `agent_routes/lifecycle.rs` doc comment corrected (no longer "static bearer").
5. `test_support.rs`: `test_state_with_grants` + one test (append-only; the file also carries lane B's `insert_live_run` hunk, which is NOT mine).
6. `crates/quarry-edge/tests/browser_grants_prod.rs` (new) + dev-deps `tonic`, `tokio-stream` in `quarry-edge/Cargo.toml`.
7. `apps/Ingestion Plane/docker-compose.grant-contract.yml` (new; env only), base compose gets `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: ${...:-}`.
8. `scripts/tests/grant-contract-overlay.test.mjs` (new): renders base / +production / +contract with `docker compose config --format json` (dummy values for every `:?` variable) and diffs hardening keys. `node --test`: 7 pass; a real planted overlay (read_only false, cap_add, NO_SANDBOX=1) was detected (4 violations); mutation tests on the pure diff cover 18 mutations.
9. `scripts/tests/browser-grant-contract.test.mjs`: +4 L1 tests (registry row, manifest row + byte-identical vendored copy + pins, call sites use `validate_for_org`, overlay pins). `node --test`: 23 pass.
10. `scripts/tests/browser-grant-prod-contract.sh` (new, +x): preflight (read-only), `--boot --yes-rebuild-stack`, tunnel container `grant-contract-tunnel`, three Playwright phases around the log scan, counts only. `--self-test`: 18 ok. NOT run against a stack.
11. `apps/Frontend Plane/verevonv3/tests/e2e/browser-grants-prod-contract.spec.ts` (new; NOT run) + `playwright.config.ts` (new regex, `local` project matches it, `e2e` ignores it). `tsc --ignoreConfig --strict` clean; `playwright test --list` shows 11 tests in `local` and none in `e2e`.
12. `.github/workflows/browser-grant-contract.yml` (new).

## Pending
- qtest run of the edge tests (t2.log), then `cargo test -p quarry-edge --no-fail-fast`, `-p quarry-runtime`, `cargo check --workspace --all-targets`, log lint (inside quarry-edge tests), zdr-contract-sync.
- RUNBOOKS section (append-only), hunk-only commits.

## Uncommitted files that are mine
crates/quarry-runtime/build.rs, src/grpc/browser_broker.rs; crates/quarry-edge/{Cargo.toml, src/grant_boot.rs, src/lib.rs, src/main.rs, src/agent_routes/lifecycle.rs, src/test_support.rs (hunks mine: test_state_with_grants + its test only), tests/browser_grants_prod.rs}; apps/Ingestion Plane/docker-compose.yml + docker-compose.grant-contract.yml; scripts/tests/{grant-contract-overlay.test.mjs, browser-grant-prod-contract.sh, browser-grant-contract.test.mjs}; the spec and playwright.config.ts; the workflow.

## Commits (local, quarryv2-next)
- f8153418 feat(quarry-runtime): validator G1 + G6 (build.rs, grpc/browser_broker.rs)
- 720ae31f feat(quarry-edge): boot gates, grant_boot.rs, prod tests, test_state_with_grants, lifecycle comment, RUNBOOKS section. TRAP: the RUNBOOKS.md hunk also carried C2's "ZDR run tombstone (WS4.J)" section (another unit appended to the same file between my append and my commit; I swept it in).
- 543730ba test(grants): overlay, overlay test, L1 asserts, wrapper, spec, playwright config, workflow, base compose key

## Test results
- qtest runtime `--features grpc --lib grpc::browser_broker`: 16 passed
- qtest edge `--features grpc --test browser_grants_prod`: 8 passed; `--lib -- grant_boot test_support`: 9 passed
- node --test scripts/tests/*.test.mjs: 124 pass; zdr-contract-sync: 65 passed; wrapper --self-test 18 ok
- IN FLIGHT (queued behind another agent): scratchpad/A2b-gates-and-prod-tests/t3.log = edge --no-fail-fast (default features), runtime --no-fail-fast, cargo check --workspace --all-targets
- NOT run: mutation runs of the new tests (planned: FailedPrecondition mapping, org-blind validate), the L2 wrapper/spec on a stack.

## Final state (23:02)
- All three commits done (above). Mutation run (t4.log): M1 FailedPrecondition not inactive -> runtime 2 tests fail, edge prod_step_with_revoked fails; M2 org-blind validate falls through -> 1 runtime test fails; M3 HTTP-only accepted in production -> grant_boot unit test and binary prod_boot_refuses_http_only_validator fail; files restored (RESTORED_OK, git diff empty).
- NOT run: full `cargo test -p quarry-edge --no-fail-fast` (default features), `-p quarry-runtime --no-fail-fast`, `cargo check --workspace --all-targets`, the log lint as part of the full edge suite. The qtest wrapper printed "DOCKER IS NOT ANSWERING" at 23:00 (t3.log) after a long queue; stopped, not retried. Command to run when Docker answers: scratchpad/A2b-gates-and-prod-tests/run3.sh via qtest.
- NOT run: the L2 wrapper and spec on a stack.

## Round 4 (review findings) - 2026-10-22
Findings fixed (new commits, nothing amended):
- 3fb780d8 refactor(quarry-edge): `grant_boot::build_grpc_validator(url, tokens, dev_bearer, dev_bypass)` holds the production validator construction; main.rs calls it with `cross_plane_tokens.clone()`; `tests/browser_grants_prod.rs::production_edge()` builds its validator with it; new L1 test pins the wiring (main passes the provider, builds nothing by hand, the function attaches the tokens). `browser-grant-contract.test.mjs` 24/24.
- (second commit, see git log) wrapper: /proc-based no-sandbox probe (pattern `[-]-no-sandbox`, prints scanned+count, unrunnable = failure, also run after the happy legs), EXECUTION_CORE_CONTAINER in the preflight, scan_hops fails on a missing container; spec: legs B and C use `untilWs1b` instead of `test.fail()` (only a known HTTP refusal passes, observed annotation before asserting, transport error recorded and rethrown, unexpected pass fails, CONTRACT_BC_GREEN=1 runs plain). `--self-test` 26 ok; `tsc6 --strict` on the spec clean; `playwright --list` 12 tests in `local`.
- Count error (finding 2): 543730ba says 18 planted mutations; the test defines 19. Corrected in the second commit's message and here; no mutation was missing.

NOT run in round 4 (Docker did not answer; a native cargo check fails in boring-sys2 because the host has no libclang; the disk was also at 100% for a while):
- the Rust change in 3fb780d8 has been reviewed by hand only. Command to run when Docker answers: `qtest.sh 'cargo test -p quarry-edge --features grpc --test browser_grants_prod 2>&1 | grep -E "^test |test result|FAILED|^error|panicked" | head -60'`, then `cargo test -p quarry-edge --lib grant_boot`, `cargo check --workspace --all-targets`, then the mutation: delete the `(Some(tokens), _)` arm in build_grpc_validator and expect prod_* failures.
- the full quarry-edge / quarry-runtime suites (see run3.sh), the log lint, the L2 wrapper and spec on a stack.
- Trap: the pre-commit hook (gitleaks via docker) hangs when Docker is wedged and holds the index lock; commit 3fb780d8 did land, but the qcommit wrapper stayed hung afterwards and was killed by hand.
- Round 4 commits: 3fb780d8 (validator function + L1 test), 6e7ab12b (wrapper probe, hop scan, spec legs B/C). Both landed, but the pre-commit gitleaks run (docker) hung each time; I killed the hung hook processes by hand and removed my stale git lock dir, so the local secret scan did NOT complete for either commit (CI scan is the backstop). The staged content is test code with dummy values only.

## Round 4b (verifier findings) - 2026-10-22
- Edited (NOT committed): scripts/tests/browser-grant-contract.test.mjs (two L1 regexes now whitespace/.clone() tolerant, still pin cross_plane_tokens and the with_service_tokens arm; 24/24 pass) and the spec (stray double blank line after untilWs1b removed). Both files are STAGED (index) with the message at scratchpad/A2b-gates-and-prod-tests/msg5.txt.
- Commit blocked: Docker is wedged (qtest printed DOCKER IS NOT ANSWERING; the pre-commit gitleaks docker run hung, I killed it, the hook then refused: "gitleaks exited 143 without completing the scan - commit blocked"). I did not bypass the hook. When Docker answers: bash qcommit.sh msg5.txt <the two paths>.
- Still not run (Docker): browser_grants_prod, grant_boot lib tests, full edge/runtime suites, cargo check --workspace, log lint, L2 wrapper/spec on a stack.

## Round 5 - 2026-10-23 (Docker answered; everything left from rounds 4 and 4b run)
- Commit fffe3627 (round 4b: L1 regexes tolerant of layout, spec blank line). The pre-commit gitleaks run completed this time ("no leaks found").
- qtest, quarry-rust-env: `-p quarry-edge --features grpc --test browser_grants_prod` 8 passed (first run of 3fb780d8's build_grpc_validator); `--lib -- grant_boot test_support` 9 passed; `-p quarry-runtime --features grpc --lib grpc::browser_broker` 16 passed.
- Mutation M4 (round-4 pending item): the `(Some(tokens), _)` arm of build_grpc_validator replaced by a bare validator -> 4 of 8 prod tests fail (prod_start_run_without_grant_is_403, prod_start_run_with_active_grant_overrides_client_domains, prod_step_with_revoked_grant_is_403_not_5xx, prod_validator_mints_org_scoped_browser_broker_token); restored, git diff of grant_boot.rs empty.
- Full run (t5.log, default features, --no-fail-fast): `-p quarry-edge` all targets and `-p quarry-runtime` all targets: 40 test-result lines, 1854 passed, 0 failed (includes log_field_discipline 22 passed, run_sweeper_integration 25); `cargo check --workspace --all-targets` Finished, no error.
- Host: node --test grant-contract-overlay + browser-grant-contract 31 pass; browser-grant-prod-contract.sh --self-test passed; zdr-contract-sync 65 passed.
- Still NOT run: the L2 wrapper and the Playwright spec on a stack (exit gate), compose render of the overlay inside the node test is covered (renders base, production, contract with dummy values).
- Nothing of mine is uncommitted.

## Round 6 (review findings) - 2026-10-23
- Commit 1bd6d0e4: legs B/C pin the known refusal (session step, 403, "browser grant_id is required", auth messages and 401 never known; sign-in precondition), classifier in tests/e2e/browser-grants-prod-contract-refusal.ts + scripts/tests/browser-grant-refusal.test.mjs (10 tests); A4/A5b/A5c/A6 assert messages; wrapper --self-test covers scan_hops fatal branch (30 ok, mutant fails 4); L1 test for cross_plane_tokens single binding (mutant "found 2"); RUNBOOKS append with the 11-test count correction.
- Verified: node --test scripts/tests/*.test.mjs 136 pass; --self-test passed; playwright --list 11 spec tests (Total 18 in 7 files); tsc6 strict and eslint clean on spec and helper. No Rust changed. L2 on a stack NOT run.

## Round 7 (verifier findings) - 2026-10-23
- Commit 81849e6c (scripts/tests/browser-grant-contract.test.mjs only): the cross_plane_tokens single-binding L1 test counts let patterns (tuple/typed/mut), Some/Ok/Err patterns, closure params, for patterns, fn params, and rejects destructuring assignment; the env-var title is now backed by checks of config.rs (prefix QUARRY_EDGE + separator "__", field declared with no serde rename/alias) and both compose files (QUARRY_EDGE__QUARRY_SERVICE_API_KEY from the required QUARRY_SERVICE_API_KEY).
- Verified: browser-grant-contract.test.mjs 25/25; node --test scripts/tests/*.test.mjs 136 pass; 11 mutations on a scratch copy (scratchpad/A2b-gates-and-prod-tests/mutate6.js) all caught, baseline passes.
- Third finding (spec "observed" annotations not in wrapper RESULT): not a defect, exit code carries any non-known leg failure. No change.
- No Rust changed. Nothing of mine uncommitted.
