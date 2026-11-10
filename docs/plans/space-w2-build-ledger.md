# Space Wave 2 build ledger

Scope: the 98 rows of `.plan-work/space-build/queue-w2.json` (WS04 to WS16).

Status key:
- **built**: a `feat(...)` commit on this branch names the slice in its subject (checked over the last 1500 commits).
- **compile: confirmed** for every Rust crate and Go module that Wave 2 touched: `cargo check` on frontend gateway, session-core, model-gateway and execution-core, and `go build` on org-core. Frontend `pnpm typecheck` is clean except 21 errors in `quarry-client.ts` from the missing Quarry client in this worktree (environmental).
- The first check of this ledger listed 52 rows as not built. That came from searching only `space-w2`. The history of this branch names all 98, so those 52 are built here, and the next pass is verification, not new code.

Totals: 98 rows; the table gives each row its feature commit, or none.

| slice | lane | size | feature commit | deps |
| --- | --- | --- | --- | --- |
| WS04-S8 | L4 | M/L | `6e8700b8b` | WS04-S4, P-UC1, WS04-S5 (c), WS04-S6 (c) |
| WS04-S9 | L7 | M | `fff0b85d8` | P-GW1, WS04-S3 (c) |
| WS04-S10 | L4 | S/M | `8eeccc726` | WS04-S4, P-UC1 |
| WS04-S11 | L4 | L | `8f1ada435` | WS04-S10, P-UC1, WS04-S5 (c), WS04-S8 (c) |
| WS04-S12 | L6 | S | `2b57eb174` | WS04-S6, P-CX1 |
| WS05-S4 | L5 | M | `e10202329` | WS05-S1 |
| WS05-S5 | L4 | M | `ee93245f2` | WS03-S1, P-UC1 |
| WS05-S6 | L5 | M | `3fcde4fe9` | M0-BASE |
| WS05-S7 | L5 | L | `43b0a3951` | WS05-S3, WS05-S6 |
| WS05-S8 | L5 | L | `1229bf844` | WS05-S2, WS05-S6, WS05-S7, WS04-S10 (c) |
| WS05-S9 | L5 | S | `f3db9f144` | WS05-S8, WS04-S10 (c) |
| WS06-S6 | L3 | M | `6a43748b8` | WS06-S5 (c) |
| WS06-S8 | L3 | M | `6ac954ff1` | WS06-S7 |
| WS06-S9 | L3 | M | `588a60d35` | WS06-S5 (c) |
| WS06-S10 | L3 | M | `23af43690` | WS06-S7, WS06-S8, WS06-S9 |
| WS08-S1 | L4 | M | `9e5da2c82` | P-UC1 |
| WS08-S2 | L1 | L | `4b0c0638b` | WS01-S1, WS09-S1, WS08-S1 (c) |
| WS08-S3 | L1 | L | `aa6ae39ed` | WS08-S2, WS01-S4 |
| WS08-S4 | L7 | L | `71c66aa00` | P-GW1, WS08-S1 (c), WS08-S3 (c), WS01-S5 (c) |
| WS08-S5 | L6 | M | `3b82693af` | P-CX1 |
| WS08-S6 | L8 | L | `6b371a166` | WS01-S6, P-SPA2, P-SPA1, WS08-S4 (c) |
| WS08-S8 | L8 | S | `91c9d7f16` | WS08-S6, P-SPA1, WS08-S5 (c) |
| WS09-S2 | L8 | S | `efe6144f6` | P-SPA2, P-SPA1 |
| WS09-S3 | L2 | M | `7f6f5cdab` | M0-BASE |
| WS09-S4 | L2 | L | `60db2b789` | WS09-S3 |
| WS09-S5 | L8 | M | `261d57710` | P-SPA1, WS09-S4 (c) |
| WS09-S6 | L8 | S | `388f7773e` | WS09-S5, P-SPA1 |
| WS09-S7 | L4 | M | `1b7580092` | P-UC1 |
| WS09-S8 | L2 | M | `46d385127` | WS09-S4, WS09-S7 (c) |
| WS10-S1 | L2 | M | `5334f633f` | M0-BASE |
| WS10-S2 | L4 | M | `e06833dc1` | P-UC1 |
| WS10-S3 | L3 | L | `d2be41cbe` | WS10-S1, WS10-S2 (c) |
| WS10-S4 | L7 | M | `e417f86fc` | P-GW1, WS10-S1 (c), WS10-S2 (c), WS10-S3 (c) |
| WS10-S5a | L4 | M | `cdc3e0b97` | WS10-S2, P-UC1 |
| WS10-S5b | L8 | M | `1049df2d5` | P-SPA1, WS10-S5a (c) |
| WS10-S6 | L2 | L | `ee6a809ca` | WS10-S1, WS10-S3 (c), WS01-S3 (c) |
| WS10-S7 | L2 | S | `f4a2459c9` | WS10-S6, WS01-S3 (c), WS08-S2 (c) |
| WS10-S8 | L3 | L | `419578392` | WS10-S1, WS10-S3, WS10-S6 (c) |
| WS10-S9 | L3 | M | `a8e027718` | WS10-S1, WS10-S3 |
| WS11-S1 | L4 | S | `2bdc7ab6c` | P-UC1 |
| WS11-S2 | L2 | L | `8e4ea3517` | WS15-S5, WS11-S1 (c) |
| WS11-S3 | L6 | L | `53ff1471c` | P-CX1, WS11-S1 (c), WS11-S2 (c) |
| WS11-S4 | L9 | M | `907ee8498` | P-SPA1, WS11-S3 (c) |
| WS11-S5 | L2 | M | `b3d53ca1b` | WS02-S1 |
| WS11-S6 | L6 | L | `c64622ff4` | WS11-S3, P-CX1, WS11-S5 (c) |
| WS11-S7 | L9 | L | `afd46aa77` | P-SPA1, WS11-S2 (c), WS11-S6 (c) |
| WS11-S8 | L7 | S | `db6d2204a` | P-GW1, WS11-S6 (c) |
| WS11-S9 | L6 | M | `74eada311` | WS11-S6, P-CX1 |
| WS11-S10 | L6 | M | `04333da06` | WS11-S3, WS11-S6, P-CX1 |
| WS11-S11 | L6 | M | `594ad15a6` | WS11-S6, P-CX1, WS11-S7 (c) |
| WS12-S1 | L4 | M | `d9bc1e1f0` | P-UC1 |
| WS12-S2 | L6 | M | `9b358eb92` | P-CX1, WS12-S1 (c) |
| WS12-S3 | L7 | L | `b8fbe2ae9` | P-GW1, WS12-S1 (c), WS12-S2 (c), WS01-S5 (c), WS01-S6 (c), WS01-S7b (c) |
| WS12-S4 | L1 | L | `ee2d29fa0` | WS01-S1 |
| WS12-S5 | L4 | M | `90051a719` | P-UC1, WS12-S4 (c) |
| WS12-S6 | L1 | M | `6b6c7bca8` | WS12-S4, WS12-S5 (c) |
| WS12-S7 | L8 | L | `707900c17` | WS01-S7a, P-SPA2, P-SPA1, WS12-S5 (c), WS12-S6 (c), WS01-S7b (c) |
| WS12-S8 | L6 | M | `de627f79d` | P-CX1, WS12-S7 (c), WS12-S4 (c), WS01-S5 (c) |
| WS13-S1 | L1 | L | `fdb3f1d30` | P-SC1 |
| WS13-S2 | L1 | S/M | `733e88f29` | WS13-S1 |
| WS13-S3 | L7 | M | `a1b6eecd0` | P-GW1, WS13-S2 (c) |
| WS13-S4a | L8 | S | `2575e3cda` | WS01-S7a, P-SPA1 |
| WS13-S4b | L8 | S | `da9f3ef78` | WS13-S4a, P-SPA1, WS13-S3 (c) |
| WS13-S5 | L8 | M | `df6c9f625` | WS13-S4a, P-SPA1, WS13-S3 (c) |
| WS13-S6 | L9 | M | `0027817c1` | P-SPA1 |
| WS13-S7 | L8 | S | `c129e5b24` | WS13-S6, P-SPA2, P-SPA1 |
| WS13-S8 | L6 | M | `f7f952452` | P-CX1 |
| WS14-S1 | L4 | M | `b0b3173fa` | P-UC1 |
| WS14-S2 | L4 | L | `34b1f1859` | P-UC1 |
| WS14-S3 | L10 | L | `6ce345f95` | WS14-S1 (c), WS14-S2 (c) |
| WS14-S4 | L10 | M | `2bb0a261a` | WS14-S2 (c) |
| WS14-S5 | L10 | M | `96a1a7020` | WS14-S2 (c) |
| WS14-S6 | L10 | M | `10ef79360` | WS14-S1 (c) |
| WS14-S7 | L7 | M | `cee325c9a` | P-GW1, WS14-S1 (c), WS14-S5 (c), WS14-S6 (c) |
| WS14-S8 | L8 | M | `4d663fd2c` | P-SPA2, P-SPA1 |
| WS14-S9 | L8 | L | `487393d9e` | P-SPA1, WS14-S7 (c) |
| WS15-S1 | L8 | S | `848da02ec` | P-SPA1 |
| WS15-S2 | L8 | S | `db1b2b181` | P-SPA2, P-SPA1 |
| WS15-S3 | L8 | M | `ed2259335` | P-SPA2, P-SPA1 |
| WS15-S4 | L8 | S | `214f4d30a` | WS07-S5, P-SPA1 |
| WS15-S5 | L1 | M | `1080e1be6` | P-SC1 |
| WS15-S6 | L1 | L | `038f6f8fa` | WS15-S5, WS01-S1 (c) |
| WS15-S7 | L7 | M | `43d267ba9` | P-GW1, WS15-S6 (c) |
| WS15-S8 | L8 | L | `e2845e7d1` | WS15-S1, P-SPA1, WS15-S7 (c) |
| WS15-S9 | L1 | M | `2c87529c3` | WS01-S1, WS15-S6 |
| WS15-S11 | L7 | M | `cd78d5684` | P-GW1, WS15-S8 (c), WS15-S9 (c), WS04-S4 (c) |
| WS15-S12 | L6 | M | `06b4a7c8d` | P-CX1, WS15-S11 (c) |
| WS16-S1 | L9 | M | `a7cd2fd18` | M0-BASE |
| WS16-S2 | L4 | S | `9b9a420b9` | P-UC1 |
| WS16-S3 | L9 | M | `37df8cea5` | WS16-S1, P-SPA1 |
| WS16-S4 | L9 | M | `92fcd8eb5` | WS16-S1, P-SPA1 |
| WS16-S5 | L9 | S | `0aecbe69d` | WS16-S3, P-SPA1 |
| WS16-S6 | L7 | S | `e28c4cc5f` | P-GW1 |
| WS16-S7 | L6 | M | `5bbf82022` | P-CX1 |
| WS16-S8 | L7 | M | `2c2d7b871` | P-GW1, WS16-S7 (c), WS01-S5 (c) |
| WS16-S9 | L9 | L | `d75a558cf` | P-SPA1, WS16-S8 (c) |
| WS16-S10 | L9 | M | `39f351603` | WS16-S9, P-SPA1, WS11-S3 (c), WS11-S4 (c) |
| WS16-S11 | L6 | S | `4cdc3a8cd` | WS16-S7, P-CX1 |

## Open items

- Fixed in the commit after batch 1: the SSO settings test handler type and the action registry test id type.
- `quarry-client.ts`: 21 errors that come from the missing Quarry client package in this worktree. Environmental, unchanged.
- Tests: not run in this pass, per the owner's instruction. Batch-1 test additions are unverified until the suites run.


## Compile status (after the live-view waiver change)

Compile checks that pass on the current content (tests are not run in this pass):

- `cargo check` on the frontend gateway, session-core (library and tests), model-gateway (library and tests),
  execution-core and retrieval-engine-rs.
- `go vet` on user-core, audit-core, notification-core, documents-api-go, the regenerated `go/gen`, `go/pkg/zdrrefusal`
  and sandbox-manager (which needed its `zdrrefusal` require added). `go build` on org-core.
- `python -m compileall` on imports-core (its code is Python, so `cargo check` does not apply).
- `tsc --noEmit` on convex-core and `tsc` on auth-core.
- Frontend `pnpm typecheck`: 0 errors, after the Quarry SDK's built types were restored in this worktree's `node_modules`.
  The first runs showed 21 errors in `quarry-client.ts`. Those came from a stale pnpm store copy of `@quarry/client`
  that predated the SDK build. The fix was a clean `pnpm install` of this worktree's frontend project; the lockfile is
  unchanged.

## Merge fixes: kept and replaced (for the post-merge test round)

This list says where to look when the suites run. Kept means my fix is on the branch; replaced means the space-w2 side
won the merge and my version is superseded.

Kept:
- `model-gateway` `run_live` posture on the run's route, the room-run zdr fix, and the room-metadata allowlist test.
- The approval decide ZDR guard: the frontend gateway's decide body refuses a zero-retention caller first, and the
  model-gateway decide route calls `require_non_zdr_durable_mutation`, listed in `DURABLE_MUTATIONS`.
- The approval decide test (`zdr_guard_tests` in the frontend `approvals.rs`), appended to the merged file.
- `space-tab-registry.test.ts`: my comment is kept.
- `CoreSidebarSpacesPanel.test.tsx`: my `searchSpaceMessages` stub in the mock is kept.
- `SpaceApprovalPanel.test.tsx`: the space-w2 `scope` argument with my approval copy (`Godkjent. Avgjørelsen ...`).
- The gateway-error copy (`errors.ts`) and the gateway-error-codes and error-catch-sites guards.
- The agent-action-admission pin and the sync script's line, both at the digest of the merged contract (90d33707...).
- The surface golden (the nine model-gateway rows, the 46 bff rows, the decide guard rows and decision types).

Replaced (the space-w2 side won):
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/mod.rs`: the room-member tests. The space-w2 side asserts
  the member writer is off by default (`feature_disabled`). My WS04-S12 rewrites (viewer refusal relayed from Control,
  unconfirmed roster as a 200 with a warning, and the owner-adds test on the Control writer) are superseded in this file.
  Re-check: `domains::spaces::tests::*room*`, `*membership*`, `*roster*`, and the two member-writer tests.
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/approvals.rs`: the space-w2 file. My decide guard is
  re-applied in its decide body; my earlier approval tests are superseded, except the new `zdr_guard_tests` module.
- `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts`: the space-w2 counts (236 ids, fragment
  order). My 240-id expectation and my Space-block assertion are superseded. Re-check the counts against the registry.
- `apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-approvals.test.ts`: the space-w2 mocks
  (`orchestration-client` and `space-approvals-client`). My mock arrangement is superseded. Re-check the approval-decide
  expectations.
- `apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomTimeline.test.tsx`: the space-w2 comment only.

Compile fixes made in this pass, not test runs:
- `session-core` `grpc.rs`: the test module imports `thread_revise_payload_digest`.
- `model-gateway` integration tests (`orchestration_http_test.rs`, `e2e_invoke_chain_test.rs`): the mocks implement the
  RPCs the proto declares (`reroute_approval`, `claim_once_approval`, `hold_quarantined_output`,
  `claim_quarantine_release`, `get_quarantine_preview`, `resolve_approval_authority`, `list_space_timeline`), and the
  literals take `..Default::default()` for the new fields.
- `SsoPolicyPanel.test.tsx` and `action-registry.test.ts`: typecheck fixes.

## Phase 1: post-merge verification (2026-11-10, branch `space-finish`)

Off main 031e7965d. Logs: `.plan-work/space-finish/logs/`. Status key from the top of this file applies; the
"tested" and "live-proven" rungs are new here. No slice has been live-proven yet (Docker engine down on this host).

### Defects fixed (commit, subject)

| id | commit | what | evidence |
|---|---|---|---|
| W2-P1-01 | ba13bfd17 | `fix(chat,gateway)`: restore the 10 chat files that merge 6a1ae082e reverted and the browser.rs test brace dropped by merge 292ded7ad (cherry-pick of owner commit afcee0802). Gateway crate did not compile on main. | `p1-gw-cargo.log` (compile fail) → `p1-gw-cargo2.log` (compiles) |
| W2-P1-02 | 522e75699 | `test(audit-core)`: stale ACL fixture missing `MODEL_QUARRY_TRACE_NATS_PASSWORD` | `p1-go.log` FAIL → `p1-audit-fix.log` ok |
| F6 | cd2845300 | `fix(auth-core)`: restore `init_better_auth.sql` (blob b4e1d53c0, CRLF hash equals the live ledger row); entrypoint refuses to start when a named bootstrap file is missing | `f6-jest.log` 30/30 |
| W2-P1-03 | 64720be75 | `test(gateway)`: relay-list guard names `space_run_live` as its second caller (WS09-S4) | `p1-gw-cargo2.log` FAILED → `p1-gw-continue.log` 7/7 |

### Suites: result and classification

| suite | result | class of each failure |
|---|---|---|
| user-core `go test ./...` | all packages ok | none |
| org-core `go test ./...` | all packages ok | none |
| audit-core `go test ./...` | all ok after W2-P1-02 | W2-P1-02 (stale test) |
| convex-core `tsc --noEmit` | 0 errors | none |
| auth-core `tsc --noEmit` | 0 errors | none |
| frontend `pnpm typecheck` | 0 errors after W2-P1-01 and the SDK dist copy | 30 real (merge loss, fixed); 21 env (`@quarry/client` dist missing in the pnpm store, fixed in the store, not in source) |
| gateway `cargo test --bin` | 1936 passed, 2 failed | 2 `domains::orgs::retention` tests: upstream mock returns 503 `organization_membership_authority_unavailable` where 403/201 is expected. org-core retention (802b59ffb), not Space. Pre-existing, out of scope; not fixed here. |
| gateway `cargo test --test *` | `continue_error_vocabulary` 7/7 after W2-P1-03; other integration binaries ok in `p1-gw-cargo2.log` | W2-P1-03 (stale guard) |
| gateway `--bin` stack overflow | default 2 MB test-thread stack overflows in `ingestions_dispatch_tests`; passes with RUST_MIN_STACK=16 MiB | environment (Windows test thread stack) |
| session-core / model-gateway / execution-core `cargo test --no-fail-fast` | completed: 112 test binaries, 11 failing targets, 23 failing tests (log `p1-mp-cargo2.log`) | see the Model Plane list below |
| SPA `pnpm test` (full) | 76 failed, 500 passed, 24 files failed | see the SPA table below |

Model Plane failures seen in the first run (`p1-mp-cargo.log`, killed mid-suite, so partial):

- `execution-core` `executor::tests::*` (9 tests): spawn `/bin/sh` and are `#[cfg(unix)]`. Environment: this host is Windows.
- `execution-core` `browser_planner_hermetic` (`challenge_detected_ends_blocked`, `forbidden_off_domain_is_terminal`, `reply_fixtures_read_as_expected`, `prompt_goldens_match`) and `browser_challenge_rule_contract` (`the_rule_pins_go_red_when_either_side_drifts`): the browser procedure prompt in `src/browser_agent/procedure.rs` was rewritten in 6b538f324 (2026-11-05, "update Verevon and browser agent workflows"); the goldens and rule-pin fixtures (last set in 391417341 and 58b2a9285) were not regenerated. Stale fixtures, not Space. Needs an owner decision on whether to regenerate.
- `execution-core` `metric_label_guard` (`execution_browser_runs_total` label `planner`, `execution_routing_first_tool_total` label `class`, `mp_exec_screening_taint_total` label `trust`): labels added in 76607197f (WS11.S5) and later without updating the reviewed list. Stale list.
- `model-gateway` `agent_mode_resolution_test` (`agent_mode_resolves_before_admission_in_the_stream_path`) and `loop_guard_inline_test` (`the_repeat_check_and_the_guard_share_one_query_normaliser`): from ccf9fd4b4 and fb9edf4ff (WS25-S6/S9, 2026-11-01); not Space.
- `model-gateway` `offline_downstreams_contract` (`every_grpc_client_of_the_offline_state_is_unavailable_at_once`, `the_offline_state_replaces_every_grpc_client_field_of_app_state`): the test expects 22 gRPC client fields on `AppState` and finds 35. The extra fields include Space clients (`space_events_client`, `space_room_read_client`, `space_history_client`, `space_attention_client`). **Space-caused**, from the Wave 2 client additions; depth row D1-527 named this gap. Fix: update the count, helper and `one_call_per_client!` together. Not fixed in this commit.
- `model-gateway` `orchestration_http_test` (`decide_approval_grant_succeeds_without_a_continuation_dispatcher`): expects `continuation_delivery: "pending"`, gets `"unknown"`. WS10 approval path, so **possibly Space-caused**; not diagnosed.
- `model-gateway` `tool_retry_contract` (`both_loops_exclude_the_same_permanent_failures`): the two tool loops disagree on the non-retryable list (the left loop lacks `resource limit exceeded`, `refusing unlimited execution`, `requires a sandbox lease`, `sandbox egress is not available`). Not Space; not diagnosed.
- `execution-core` `runtime_loop::agent::tests::a_fan_out_item_still_waiting_for_a_permit_is_not_started_after_the_deadline`: not diagnosed; not Space.

### Space-touched frontend tests

Space test files in the Wave 2 "merge fixes" list (`space-tab-registry`, `CoreSidebarSpacesPanel`, `SpaceApprovalPanel`,
`space-approvals`, `action-registry`, `SpaceRoomTimeline`) and all of `features/spaces`, `shared/api/spaces*`, `shared/actions`
pass in the Space-only run: 99 files, 1473 passed, 2 skipped (`p1-fe-spaces2.log`, exit 0). None of them appeared in the
full-suite failure list (`p1-fe-vitest-r2.log`, 76 failed across 24 files, none under `features/spaces`).

### Evidence location

The raw logs (`.plan-work/space-finish/logs/*.log`, about 4 MB) are untracked and stay in the worktree. The repo
ignores `*.log` and the worktree is local. Every result above names its log file, so a reviewer on this machine can
open it. The run commands are in the matching `*.ps1` beside each log. Nothing in the logs was secret-scanned beyond
a pattern check for API keys, private keys and DB URLs, which found no matches.

### Not run in Phase 1

- Live proof of any slice (Docker engine down on this host; see `phase1-notes.md`).
- DB-tier Postgres tests (`TEST_DATABASE_URL`) for user-core, audit-core, session-core: needs a throwaway Postgres, which needs Docker.
- Playwright (`tests/e2e`).
- Gateway clippy (`-D warnings`), which the CI gate runs; not run in this pass.

