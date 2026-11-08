# paths-C7: path and line check, plan lines 3236-3714

Reviewer C7 (Haiku path checker). Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md` (read only). Code truth: live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Tags from `.plan-work/tools/provenance.sh`. Line checks by Read, Grep, and read-only `git diff HEAD --numstat` and hunk headers. No product file was changed.

## Counts

Paths (rows = path mentions with a line; 207 rows, 191 unique strings):
- Plan-cited product path rows checked: 201. Six rows were my own probes or extraction fragments (a glob fragment, two bare migration stems, a bare `orchestration.rs` probe, a probe under `Quarry-v2/scripts/tests`, a bare `0009` probe) and were re-checked by hand, not counted as plan errors.
- Present: 141 (C 95, M 33, U 10, and 3 present but not tracked: two sdks `models` dirs tagged [C], and `apps/Ingestion Plane/.env.generated-secrets`, untagged, git-ignored).
- Missing: 60 (44 tagged [NEW] as expected; 1 `apps/auth/test/...`, which the plan says is missing and it is; 6 tagged [C]/[M]/[U] with a wrong path; 9 untagged, of which 8 are wrong paths or should be [NEW] and 1 is a relative path that resolves elsewhere).
- Tag mismatches (exists, different status): 2 rows (sdks `models`, tagged [C], not tracked at afb83928; git-ignored at `Quarry-v2/.gitignore:50` and `:32`).
- [NEW] collisions (new file already exists): 0.

Line citations and line-level count claims (111 items):
- Checked: 104 (OK 94, DRIFT 5, WRONG 5).
- Unverified: 7 (pipeline.rs 421-476, 436-448, 506-508, 595-705, 506-518, 601-683; the "16 AppState literals in quarry-edge" count).

## Failures

| Plan line | Path[:line] | Plan tag/claim | Actual | Proposed correction |
|---|---|---|---|---|
| 3254, 3295 | `apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/store/pg/store.go` | [C] | MISSING. `pg/` holds `pg.go` (type `postgresDB`, line 65), `resources.go` (23 SELECT or Job matches), `resources_v2.go`, `gdpr_purge.go`, `webhooks_blocklists.go`. [WRONG-CITATION] | Keep `resources.go` [C] (it holds the SELECT lists). Replace `store.go` with `pg.go` [C] if the store type is meant. |
| 3282 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/tests/support/mod.rs` | [M] | MISSING. The file is `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/support/mod.rs`, tagged [M]. [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/support/mod.rs` [M] |
| 3542 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/tests/rerank_behavior.rs` | [U] | MISSING. The file is `crates/quarry-edge/tests/rerank_behavior.rs`, tagged [U]; 3604 already cites that path. [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs` [U] |
| 3543 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/tests/ssrf.rs` (and "three `#[ignore]` tests") | [C]; three | MISSING. The file is `crates/quarry-browser/tests/ssrf.rs`, tagged [C], with five `#[ignore]` attributes (lines 38, 73, 101, 129, 157). [WRONG-CITATION] [WRONG-COUNT] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/tests/ssrf.rs` [C]; "five" |
| 3285 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/tests/contracts.rs:40-60` | untagged | MISSING. The file is `crates/quarry-core/tests/contracts.rs` [C]. Lines 40-60 match the claim (ErrorCode http_status and retryable asserts). [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/tests/contracts.rs:40-60` |
| 3629 | `dp_retrieve_contract.rs` [U], `dp_rerank_contract.rs` [U] (listed beside `quarry-runtime/src/token_request_contract.rs`) | [U] | MISSING under `quarry-runtime/src/`. The files are `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_retrieve_contract.rs` [U] and `.../tests/dp_rerank_contract.rs` [U]. [WRONG-CITATION] | Use the `tests/` paths. |
| 3283 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/migrations/` | untagged | MISSING. The directory is `crates/quarry-runtime/migrations/` [C] (0001 to 0009; latest `0009_browser_timeline_events.sql`, as 3309 says). [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/migrations/` [C] |
| 3295 | `apps/Ingestion Plane/Quarry-v2/services/quarry-control/migrations/` | untagged | MISSING. The directory is `services/quarry-control/internal/store/pg/migrations/` [C] (001 to 014; latest `014_fleets.sql`, as 3310 says). [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/store/pg/migrations/` [C] |
| 3635 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/src/*.rs` | untagged | MISSING (doubled `src`). The intended scan target exists as `crates/quarry-edge/src/` [C]. [WRONG-CITATION] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/*.rs` |
| 3695 | `apps/Ingestion Plane/Quarry-v2/sdks/typescript/src/models` and `sdks/python/quarry_client/models` | [C] | Present but not tracked. `Quarry-v2/.gitignore` line 50 ignores `sdks/typescript/src/` and line 32 ignores `sdks/python/quarry_client/`. `git ls-files` at afb83928 returns nothing for either. [TAG-MISMATCH] | Drop [C]. Put the deprecation text in the tracked `apps/Ingestion Plane/Quarry-v2/sdks/typescript/docs/EvaluateAction.md` [C] (it exists at afb83928). The regenerated models are local output only. |
| 3251 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/zdr_matrix.rs` and `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/zdr_log.rs` | untagged | MISSING for both. Both are new files. [TAG-OMISSION] | Tag both [NEW] (`zdr_matrix.rs` is [NEW] at 3634; `zdr_log.rs` has no other tag). |
| 3481 | `scripts/tests/browser-grant-prod-contract.sh` | untagged | MISSING. It is [NEW] at 3624. [TAG-OMISSION] | Tag [NEW] at 3481. |
| 3517 | `apps/Frontend Plane/verevonv3/src/features/browser/` | untagged | MISSING. It is [NEW] at 3289. [TAG-OMISSION] | Tag [NEW] at 3517. |
| 3624 | `scripts/run-control-plane.sh` | untagged, not written in full | MISSING at the repo root. `apps/Control Plane/scripts/run-control-plane.sh` [C] exists; 3577 shows the Control Plane directory is the base. [NOT-IN-FULL] | `apps/Control Plane/scripts/run-control-plane.sh` [C] |
| 3288 | `apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs:312` (AppState literal) | line 312 | DRIFT (+1). The `AppState {` literal is at line 313; line 312 is `fn test_state`. | `:313` |
| 3273, 3288 | "5 AppState literals in the gateway" | count | WRONG-COUNT. Six literal sites: `gateway/src/main.rs:313`, `config.rs:146`, `onboarding/crawl_preview/stream_e2e.rs:110`, `domains/browser.rs:4915`, `domains/chat/shared.rs:745`, `domains/orchestration.rs:632`. | "six", and add `onboarding/crawl_preview/stream_e2e.rs:110` to the list |
| 3287 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` start_ai_run 1212-1382 | range | DRIFT (label, 51 lines). `pub(crate) async fn start_ai_run` begins at 1263; line 1212 starts `build_ai_run_request`. The end at 1382 is correct. | `start_ai_run 1263-1382` |
| 3608, 3697 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:5001` (test `sanitize_action_denies_raw_script_evaluation`) | line 5001 | DRIFT (-1). The `fn` is at line 5000. | `:5000` |
| 3686 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs` crawl_handoff 590-667 | range | DRIFT (partial). The function runs 590-725; `HandoffAck` starts at 728. | 590-725, or state that 590-667 is the edited sub-range |
| 3624 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` boot bails 132-215 | range | DRIFT (partial). Bails also at 219 (`QUARRY_EDGE__DATABASE_URL` unset) and 239-241 (`QUARRY_EDGE__QUARRY_SERVICE_API_KEY` required). | 132-241 |
| 3635 | "85 literal registrations" under `quarry-edge/src` | count | WRONG-COUNT. A line grep for `\.route\(\s*"` gives 56 matching lines in `quarry-edge/src` (93 with multiline). Neither is 85, and the scan path is wrong (row 9). | Recount with the test's own regex after the path fix. |
| 3677 | `apps/Frontend Plane/verevonv3/apps/gateway/src/observability.rs` (`record_result_cache`) | [C] function | CLAIM-FALSE. No `record_result_cache` or `result_cache` symbol exists under `gateway/src`. `observability.rs` has `record_authenticated_request` at line 53. | Mark as a new function [NEW], or name the existing function. |
| 3634 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/tests/zdr_feature_matrix.rs` | [NEW] | Absent, as tagged. But `src/tests/` does not exist; the crate's integration tests live in `crates/quarry-edge/tests/`, and 3641 follows that layout. [CONVENTION, low] | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/zdr_feature_matrix.rs` [NEW], or state that `src/tests/` is intended. |

## Notes (checked and correct, or resolved, not errors)

- Verified: `main.rs:183-201` prod grant gate; `main.rs:133` exact-case bypass set versus `auth.rs:258-261` `eq_ignore_ascii_case` (the mismatch described at 3701 is real); `main.rs:369-373` lowercases at 370 and the `match` at 373 does not; `main.rs:659-672` validator wiring; `main.rs:694-708` durable gate.
- Verified: `agent_routes.rs` 314 (`validate_run_grant`), 1381 (`grant_id is required`), 1399-1411 (client domain list overwritten), 527 (`zdr_forbids_persistent_profile`), 2269-2270, 3102, 3106. `start_run` 1372-1413 is a region inside `start_run` (fn at 1273): content right, label loose.
- Verified: `auth.rs` 504-545 is `require_service_route_scope`; crawl and batch are absent from its allow-list.
- Verified: `grant.go` 32-36 (`allowedSensitiveActions`) and 104-125 (scoped sensitive approval).
- Verified: `workflows.go` 456, 474, 555, 574 (MaxPages and MaxDepth checks); CrawlJobWF 396, crawlJobDurableFrontier 518, `dispatcher.go` 432. `workflow.GetVersion` ids are not present yet (planned, not an error).
- Verified: `zdr_claim_floor.rs` 842-870 (path `crates/quarry-edge/tests/zdr_claim_floor.rs`, [M]); `zdr-contract-sync.test.sh` GOLDENS at 48 to the function end at 419; `quarry-v2-ci.yml` 201-205 and 207-211; `org-service.yml` 74-93; `docker-compose.yml` 109-110; `master-ownership-matrix.md` 166; `apps/Control Plane/auth-core/package.json` lines 21 and 23 (both reference the missing `apps/auth/test` path, as 3562 says).
- Verified: `browser.rs` 370 (router), 451 (`create_session`), 1023 (`close_session`), 705-713 (agent takeover 409 inside the cited 663-759); `search.rs` 51, 589, 155 (target log field in 154-158); `knowledge/quarry.rs` 62, 213 (`CacheAdmission`), 306.
- Verified: `chromiumoxide.rs` 194, 371, 1873, 1914, 1944, 2392, 2438, 2828-3019 (inputs); 3551 lines. `BrowserChrome.tsx` 2172 lines. `pipeline.rs` 298 (`run_inner`) and 821-829 (`page extracted` log). `routes.rs` 81, 303-305, 312, 398-530, 488-509, 563, 590-725, 728.
- Verified: `observation.rs` 542-544; `action_runtime.rs` 380-392; `mp_client.rs` 382-383 (exact lines); `step_receipts.rs` 183 and 457; `fallback_driver.rs` 587 (`RuntimeNotReady` wire name); `model-gateway grpc.rs:2700` and `tools.rs:1184` (typed-code mapping); `browser_run.rs` 337 (grant resolve, the derive hunk); `browser_agent.rs` 749-752 (`Err(e)` arm, `PlanStatus::Failed`); `quarry_agent.rs:161` (`Evaluate` variant).
- Verified: openapi hunks 238, 243-250, 4012-4031, 4052-4057, 4102-4143; `CrawlRequest` 4361; `EvaluateAction` 3042; `AgentAction` evaluate ref 2787; agent run and step paths 263-345.
- Verified churn (insertions plus deletions, not file length): `config.rs` 167+20 (plan "+187"); `main.rs` 132+56 (plan "+188"); openapi 77+1 (plan "+78"); `quarry.rs` 534+12 and `tool_loop.rs` 370+55 (plan "546" and "425"; file lengths are 2515 and 13032). `state.rs` hunks 57-93 correct.
- Verified latest migrations: session-core latest `0045` (0044 and 0045 are [U]); quarry-runtime latest `0009`; quarry-control latest `014_fleets.sql`.
- Resolved bare names (not errors): `orchestration.rs:632` is `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orchestration.rs` [C]; `fallback_driver.rs:587` is `crates/quarry-runtime/src/fallback_driver.rs` [C]; PR 2 and PR 3 `docs/` and `sdks/` paths resolve under `apps/Ingestion Plane/Quarry-v2/` (`docs/openapi.yaml` [M], `docs/CONTRACTS.md` [C], `sdks/generate.sh` [C]).
- Verified: `browser-workspace-*.spec.ts` (5 specs) and `browser*.pb.go` (4 files, all [C]); the 3 [NEW] collision probes (none exist).
- Unverified: the six pipeline.rs ranges listed above (only 298 and 821-829 checked); the 16 `AppState {` literals in quarry-edge (a grep finds 22 `AppState {` lines, including the struct definition and fn signatures, so a literal-only count is needed); `model-plane-rust-ci.yml` end line 169.
- User memory note: the plan's `apps/Model Plane/go/go.work` (3537) exists and is committed [C]; the user's Model Plane memory says "no go.work". The memory may be stale.
- `apps/Ingestion Plane/.env.generated-secrets` (3575) exists and is git-ignored; checked for presence only, not read.

<!-- COMPLETE -->
