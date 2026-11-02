# integ/main-20261028 build and test verification (2026-10-28)

Tree: branch `integ/main-20261028` (quarryv2-next plus the live-tree snapshot `0b3dee61f`, applied with `cherry-pick -n`).
Nothing was committed, reset, checked out or stashed. Fixed files are staged with `git add`.

## Step 1: conflicts

- `git diff --name-only --diff-filter=U`: empty.
- `git grep -I '^<<<<<<< \|^>>>>>>> \|^=======$'` over the whole tree, `.md` included: no match.

## Step 2: lanes

| Lane | Result | Baseline (quarryv2-next) |
|---|---|---|
| a. Quarry Rust, six crates (`cargo test --no-fail-fast`, container `quarry-rust-env:2`, volume `quarry-target-integ`) | first run: 106 test binaries, 3956 passed, 1 failed, 52 ignored (RC=101). After the one fix below, `zdr_gap_rows` re-run: 9 passed 0 failed; all other binaries unchanged, so 3957 passed, 0 failed, 52 ignored. | 3801 / 0 |
| a. `cargo check --workspace --all-targets` | exit 0. Warnings only (unused imports in `tests/profile_guard.rs`, `tests/browser_grants_prod.rs`, `src/fleet_routes.rs`, `src/agent_routes/tab_ledger.rs`; unused `MutexGuard` in `src/agent_routes/tests.rs`). | n/a |
| b. Root node | first run 151 tests, 150 pass, 1 fail; after fix 1: 151 pass, 0 fail | 143 / 0 |
| b. Quarry node | 53 tests, 53 pass, 0 fail, 0 todo | 41 pass, 12 todo, 0 fail (12 todo now pass) |
| c. Go quarry-control (`GOWORK=off`) | all packages ok, 0 failed | 0 failed |
| c. Go quarry-orchestrator (`GOWORK=off`) | all packages ok, 0 failed | 0 failed |
| c. Go `apps/Model Plane/go/services/browser-broker` | cmd, internal/authz, internal/grant, internal/server ok; 0 failed | 0 failed |
| d. verevonv3 gateway (host cargo 1.97.1, `--all-targets`) | 830+1+13+3+1 = 848 passed, 0 failed, 1 ignored | 848 / 0 |
| e. SPA `tsc -b` | 33 `error TS` (21 `src/shared/api/quarry-client.ts`, 12 `src/features/support/components/RemoteSupportPage.tsx`; unresolved `@quarry/client`, `@verevon/remote-core`) | 33 |
| e. SPA eslint `--quiet` | 0 errors | 0 |
| e. SPA vitest (`--maxWorkers=2 --pool=forks`) | 380 files: 377 passed, 3 failed; 4450 tests: 4449 passed, 1 failed | same 3 files fail on quarryv2-next |
| f. Data Plane `retrieval-engine-rs` `cargo test` (host) | 22 binaries: 848 passed, 0 failed, 15 ignored (includes `rerank_behavior.rs`) | none recorded |

## Fixes made (merge breakage)

1. `apps/Model Plane/docs/decisions/ledger.md`, service-principal entry (2026-10-06). Root docs test `ledger: the service-principal entry says whether the rerank route is registered, as the code has it` failed: the live tree registered `POST /v1/rerank` (`retrieval-engine-rs/src/rerank_api`, merged in `api/mod.rs`) while the quarryv2-next ledger text still said "not registered in the engine's router yet"; the test forces that reconciliation. Two sentences changed (rerank bullet now says registered and built/unit-tested but not proven live; live-proof step 5 "With the rerank handler registered"). CRLF kept. Docs only.
2. `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/zdr_gap_rows.rs`, test `gap_rows_currently_violate_search_suggest`. The live side added `model_plane_gate.rs` (QB-T17: each Model Plane feature has a switch, default OFF; `search_suggest` off answers an empty 200 with no Model Plane call). quarryv2-next's gap test builds its harness without the switch, so `corrected_query` was `null` instead of "rust async". The harness `AppState` now sets `model_plane: ModelPlaneFeatures { search_suggest: true, ..Default::default() }`, the pattern the live side's `zdr_claim_floor.rs` uses for `/v1/audio`. Assertions untouched; the test still pins the gap that remains once the switch is on. Judgement call: if you would rather record than change a test, revert this file and treat it as "gap closed by default by QB-T17" (the matrix row may then need updating). 6 lines added, CRLF kept.

Both files are staged.

## Failures left

SPA vitest, 3 files, all pre-existing (same three fail identically in the quarryv2-next worktree, run read-only):

1. `src/shared/api/quarry-client.test.ts`: suite does not load, cannot resolve `@quarry/client` (file-linked SDK not built).
2. `src/features/support/components/SupportPage.test.tsx`: suite does not load, cannot resolve `@verevon/remote-core` (workspace package not built).
3. `src/features/dashboard/home/BrowserBlockedNotice.test.tsx` > `"Ta over" hands the browser ...`: handler called with `("human_takeover", null)`, test expects `("human_takeover")`.

Not run: Playwright/e2e, docker DB integration tests, live stack.

## Notes

- First Rust launch with `nohup &` lost its docker client; the container was killed and the lane relaunched. Only the second run is used.
- An `alpine` image was pulled once to read the Rust log from the docker volume.
- Raw logs: scratchpad `integ/`; Rust log is `integ.log` in docker volume `quarry-target-integ`.
