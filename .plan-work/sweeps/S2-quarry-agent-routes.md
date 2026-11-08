# S2 quarry-agent-routes

Sweep: Haiku S2 (code presence map). Date: 2026-10-14. Tree read: LIVE `C:/dev/CoresSystem` (commit afb83928 plus uncommitted entries). The worktree copy was not read.
Items: F-t2-002, F-t2-006, F-t2-008, F-t2-025, F-t2-026, F-t2-029. All six have cards and verified-items JSON. Read-only: no product file was edited.

Path shorthand (all tagged): `Q/` = `apps/Ingestion Plane/Quarry-v2/`; `Q/edge/` = `Q/crates/quarry-edge/src/`; `Q/rt/` = `Q/crates/quarry-runtime/src/`; `Q/br/` = `Q/crates/quarry-browser/src/`; `Q/core/` = `Q/crates/quarry-core/src/`; `Q/ctl/` = `Q/services/quarry-control/`; `Q/orch/` = `Q/services/quarry-orchestrator/`; `FE/` = `apps/Frontend Plane/verevonv3/`; `GW/` = `FE/apps/gateway/src/`; `MP/` = `apps/Model Plane/`; `CP/` = `apps/Control Plane/`; `AP/` = `apps/Application Plane/`; `IC/` = `apps/Ingestion Plane/integration-corev2/`.
Tags (from tools/provenance.sh, about 110 paths): [C] committed and unchanged; [M] committed, modified live (uncommitted hunks checked against the cited lines); [U] untracked; [NEW] file the plan would create; [registry] file in the local cargo registry, outside the repo.
Verdicts: OK; DRIFTED (corrected line given); FALSE; STALE (doc claim contradicted by code); NOT-RE-READ (cited, not opened, cut by the 25-citation cap).

## Cross-item overlaps (merge-conflict hotspots)
1. `Q/edge/agent_routes.rs` [C, 3737 lines]: touched by all six items (002 gate, 006 reaper and caps, 008 budgets, 025 admission, 026 deadlines on the step path, 029 metrics labels). Serialize these edits.
2. `Q/edge/main.rs` [M, 1569 lines]: 006 (pools 323/337, agent driver 1284-1288, agent_runs 1290), 025 (pool permits), 029 (second listener near 1513-1517, shutdown_signal 84-111, lease drain). Uncommitted hunks (+132/-56) start at 47, 244, 984-1201, 1232, 1395, 1463; none touch the cited lines.
3. `Q/br/chromiumoxide.rs` [C, 3551 lines]: 002 (evaluate 3003), 006 (acquire 1914, ensure_browser 194), 026 (deadlines 76-77, 226-262, 683-693, 1988-2023), 029 (viewport relaunch 194-270).
4. `Q/rt/lease_pool.rs` [C, 294] and `Q/rt/browser_driver.rs` [C]: 006, 025 (per-org permits, empty org_id on the internal lease), 026 (acquire deadline), 029 (metrics_snapshot 53, no production caller).
5. `Q/rt/observation.rs` [C, 3214] and `Q/rt/agent_loop.rs` [C, 895]: 002 (Evaluate arm 542-544; label 319), 008 (ctx.step 1015; over_budget 154-163).
6. `Q/edge/routes.rs` [M, 1564]: 006 (fleet_router merge 285), 008 (CrawlRequest 533-679). Its only uncommitted line (1329) is unrelated.
7. `GW/domains/browser.rs` [C, 6317 lines]: 002 (sanitize_action 3751; session grant 1235), 008 (StartAiRunBody 312-330; create_session caps 485-487; run_action 663), 026 (12/120 defaults), 006 (manual sessions).
8. `Q/ctl/internal/store/pg/resources_v2.go` and `resources_v2_mem.go` [C]: 008 (CreditUsage) and 025 (Concurrency). `Q/orch/internal/workflows/workflows.go` and `jobs/dispatcher.go` [C]: 008 (max_pages, 30-minute timeout) and 025 (job admission and queue naming).
9. `MP/rust/services/execution-core/src/quarry_agent.rs` [C]: 002 (Evaluate variant 161) and 006 (close_run client 655). `runtime_loop/agent.rs` [M]: 002 only. `browser_agent.rs` [C]: 008 only.
10. Cross-plane manifests: `CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json` [M] and `Q/crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M] (plus `.sha256` [M]): 025 needs an org-core grant row in both copies.
11. Docs and contract: `Q/docs/openapi.yaml` [M] (002 evaluate schema; 006 has no fleet path), `Q/docs/PLAN.md` [C], `Q/docs/GAP.md` [C], `Q/docs/gap-quarry.md` [C] (029 stale claims). `.github/workflows/quarry-docs-contract.yml` [U] reads `Q/docs/**`.

---

## F-t2-002 Gate Evaluate and raw script actions behind a grant capability and one-shot approval

Card: Priority, Partial, S, W1. JSON: verdict "reword"; the isolated-world evaluator (effort M) moves to F-t2-023.

### 1. Citation check
22 citations checked (card plus verification.evidence). No skips.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | Q/core/contracts.rs:821-823 | C | OK | `AgentAction::Evaluate { script }` |
| 2 | Q/edge/agent_routes.rs:144-170 | C | OK | `sensitive_approval_for_action` begins 144; RespondDialog arm 149; no parent run grant gives Forbidden at 154-157 |
| 3 | Q/edge/agent_routes.rs:276-310 | C | OK | DownloadRef and FrameDownloadRef arm 233-279; `_ => Ok(None)` at 280; `validate_sensitive_approval` 284-313 |
| 4 | Q/edge/agent_routes.rs:280 (card) | C | OK | fall-through confirmed: Evaluate returns Ok(None), so no approval is required |
| 5 | Q/edge/agent_routes.rs:2230-2290 | C | OK | `pub async fn step` at 2228; gate 2276-2290; one-shot `used_sensitive_approval_grants.insert` 2282-2284 |
| 6 | Q/edge/agent_routes.rs:2294 | C | OK | `enforce_step_budget(&entry)` call; the same call is also at 3118 (WS path) |
| 7 | Q/edge/agent_routes.rs:3106-3117 (not in card) | C | OK, extra | `execute_ws_action` (3086) runs the same gate. The gap applies to both entry points, not only /step |
| 8 | Q/rt/observation.rs:540-544 (card 542-543) | C | OK | Evaluate arm calls `self.browser.evaluate(session, script).await?` and drops the return value |
| 9 | Q/rt/action_runtime.rs:380-392 | C | OK | `Action::Evaluate` arm returns serialized value; ActionRuntime has no production caller (section 3) |
| 10 | Q/rt/grant_validator.rs:25-109 (card 98-109) | C | OK | `GrantValidation` at 25; `authorizes_exact_sensitive_action` 98-109 (exactly one allowed action, bound to parent grant) |
| 11 | Q/rt/mp_client.rs:345-395 (card 383) | C | OK | `build_prompt` 345; prompt example lists `pdf` (382) and `evaluate` (383) |
| 12 | Q/br/chromiumoxide.rs:3003-3017 | C | OK | `evaluate` 3003; `page.evaluate(script)` at 3009 runs in the main world |
| 13 | MP/go/services/browser-broker/internal/grant/grant.go:30-36 | C | OK | `allowedSensitiveActions` 32-36 = respond_dialog, upload_ref, download_ref; no evaluate |
| 14 | MP/rust/services/execution-core/src/runtime_loop/agent.rs:3479-3512 | M | OK | `browser_act_parameters_json()` at 3479; no evaluate in the schema; uncommitted hunks (1937, 4566, 7185+) do not overlap 3479-3512 |
| 15 | MP/rust/services/execution-core/src/llm_planner.rs:53-65 | C | OK | `ACTION_SCHEMA` enum = navigate, click, type, scroll, wait, extract, done |
| 16 | MP/rust/services/execution-core/src/quarry_agent.rs:130-200 | C | OK, caveat | Model-side action enum still has `Pdf` (160) and `Evaluate { script }` (161) |
| 17 | GW/domains/browser.rs:3751-3755 | C | OK | `"evaluate" => Err(("browser_action_denied", ...))` |
| 18 | GW/domains/browser.rs:1235 | C | OK | `"grant_id": format!("session:{session_id}")`: UI runs carry no broker grant |
| 19 | Q/edge/agent_routes.rs:1153-1176 | C | OK | `require_ephemeral_evidence_action`: allow-list has no Evaluate; other actions return Forbidden |
| 20 | Q/edge/agent_routes.rs:694 (and 751) | C | OK | action label "evaluate" in two label maps |
| 21 | Q/rt/observation.rs:243-260 (verevonToday) | C | OK | Evaluate listed in outcome classification at 259 |
| 22 | Q/docs/openapi.yaml:2787 and 3042 (not in card) | M | OK, extra | `evaluate` discriminator entry (2787) and `EvaluateAction` schema (3042): the public contract documents evaluate |

### 2. Affected files
- `Q/edge/agent_routes.rs` [C, 3737 lines]: `sensitive_approval_for_action` 144-283 (callers 2276 and 3106); `validate_sensitive_approval` 284; `validate_run_grant` 314; `RunEntry.used_sensitive_approval_grants` 101; `step` 2228; `execute_ws_action` 3086; `require_ephemeral_evidence_action` 1153; labels 694 and 751.
- `Q/core/contracts.rs` [C]: `AgentAction::Evaluate` 821-823. Callers: `Q/rt/observation.rs` 337 and 542, `Q/edge/agent_routes.rs` 694 and 751, `Q/rt/agent_loop.rs` 319, serde test `Q/crates/quarry-core/tests/contracts.rs:367` [C].
- `Q/rt/grant_validator.rs` [C, 371]: reuse `authorizes_exact_sensitive_action` (98); no change needed.
- `Q/rt/observation.rs` [C, 3214]: mapping 337 (to `Action::Evaluate`) and return-value drop 542-544. The gate must run before 542.
- `Q/rt/agent_loop.rs` [C, 895]: label "evaluate" at 319.
- `Q/rt/action_runtime.rs` [C, 541]: `Action::Evaluate` 380-392, test use 485.
- `Q/rt/mp_client.rs` [C, 715]: planner prompt lines 382-383 (card says remove `pdf` and `evaluate` here).
- `Q/br/chromiumoxide.rs` [C, 3551]: `evaluate` 3003-3017. `Q/br/actions.rs` [C]: `Action::Evaluate` enum at 39.
- `Q/rt/lib.rs` [M]: re-exports at 99 (ActionRuntime), 100 (AgentLoop), 145 (ModelPlanePlanner). No change.
- `MP/go/services/browser-broker/internal/grant/grant.go` [C, 362]: `allowedSensitiveActions` 32-36 (add `evaluate` only if the sensitive path is chosen); `CreateScopedSensitiveApproval` 104; `createWithSensitiveScopes` 124-144.
- `MP/go/services/browser-broker/internal/server/server.go` [C, 179]: `RevokeScoped` call at 119. No change.
- `MP/rust/services/execution-core/src/quarry_agent.rs` [C]: Model-side `Evaluate` variant 161 (remove or document as refused).
- `MP/rust/services/execution-core/src/runtime_loop/agent.rs` [M] and `llm_planner.rs` [C]: already exclude evaluate. No change; the card's "hide from planner" sub-task is already done.
- `GW/domains/browser.rs` [C, 6317]: `sanitize_action` 3702, denial 3751-3755. No change.
- Contract: `Q/docs/openapi.yaml` [M]: `evaluate` discriminator 2787, `EvaluateAction` 3042. Also `.github/workflows/quarry-docs-contract.yml` [U].
- Protos: `MP/proto/model_plane/v1/browser.proto` [C] and `browser_agent.proto` [C] contain no sensitive-action names (grep). The card's "proto alignment" has no current target [UNVERIFIED].
- Migrations: none.

### 3. Reusable but unwired
- `ActionRuntime` (`Q/rt/action_runtime.rs:62`, Evaluate 380-392): no production caller. Grep with `--exclude-dir=.claude` finds only `action_runtime.rs` (tests at 446-520) and the re-export at `Q/rt/lib.rs:99`.
- `ModelPlanePlanner` (`Q/rt/mp_client.rs:117`, `impl Planner` at 225): no production `ModelPlanePlanner::new` call. Uses are tests (341, 365) and the re-export at `lib.rs:145`.
- Model-side `AgentAction::Evaluate` (`quarry_agent.rs:161`): a type with no emitter in the planner schema.
- Wired and reusable: `sensitive_approval_for_action`, `validate_sensitive_approval`, `used_sensitive_approval_grants` (called at 2276-2290 and 3106-3117); `authorizes_exact_sensitive_action` (`grant_validator.rs:98`).

### 4. Tests
- `Q/edge/agent_routes.rs` inline `mod tests` (1963-): `agent_navigation_blocks_private_targets_at_edge_boundary` (1967), `non_navigation_actions_do_not_trigger_network_validation` (1981), `browser_session_projection_requires_exact_tenant_and_actor` (2029), `step_budget_is_fail_closed_once_limit_is_reached` (2063). No test references sensitive approval or Evaluate gating (grep).
- `Q/crates/quarry-core/tests/contracts.rs:367` Evaluate serde round trip [C]; `Q/crates/quarry-core/tests/properties.rs` [C] mentions sensitive or Evaluate.
- `Q/rt/grant_validator.rs`: 9 inline tests [C] (names not listed).
- `GW/domains/browser.rs:5000` `sanitize_action_denies_raw_script_evaluation` [C].
- `MP/go/services/browser-broker/internal/grant/grant_test.go`: `TestStoreScopesCopiesAndRevokesGrants`, `TestStoreReportsExpiredAndUnknownGrants` [C].
- `MP/rust/services/execution-core/tests/browser_agent_e2e.rs` [C] (live stack).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge agent_routes`; `cargo test -p quarry-runtime grant_validator`; `cargo test -p quarry-core contracts`. Broker: `cd "apps/Model Plane/go/services/browser-broker" && go test ./internal/grant/...`. Gateway: `cd "FE/apps/gateway" && cargo test --all-targets`.
- CI: `.github/workflows/quarry-v2-ci.yml` cargo-check job, `cargo test --workspace` (line 205) [C]; `model-plane-rust-ci.yml` line 119, `cargo test --workspace --no-fail-fast` [M]; `verevonv3-ci.yml` gateway-rust job, lines 108-123 [M]. No workflow references browser-broker (grep), so the broker's Go tests are not in CI.

### 5. Feature flags and env
- `require_browser_grants`: `Q/edge/config.rs:132` (bool, default false at 385) [M]; `Q/edge/state.rs:38` [M]. `Q/edge/main.rs:183-184` [M] sets it from config OR environment `prod`/`production`. The step handler fails closed without a grant when set (agent_routes.rs 2261-2265). Env name [INF: `QUARRY_EDGE__REQUIRE_BROWSER_GRANTS`, the `QUARRY_EDGE__` prefix used in `compose/docker-compose.yml:70`].
- `QUARRY_LITE_EXPERIMENTAL` (`agent_routes.rs:1144`): gates the ephemeral evidence tier (1283).
- Cargo feature `browser-agent` (`Q/crates/quarry-edge/Cargo.toml` 68-74, default on): gates the router at `agent_routes.rs:3659` and the no-op router at 3735.

### 6. Surprises
- The card lists "respond_dialog, upload_ref and download_ref". The gate also covers the Frame variants (FrameUploadRef at 191, FrameDownloadRef at 239). DRIFT, minor.
- The WS entry point shares the gate and `enforce_step_budget` (3106, 3118). Any fix must cover both.
- The public OpenAPI documents `evaluate` (openapi.yaml 2787, 3042). Removing it changes the contract, the docs gate, and any generated SDK.
- UI runs carry `session:{id}` (browser.rs:1235), so the parent-grant requirement (Forbidden at 154-157) is the only enforcement point for them. Consistent with the card's "fail closed until F-t2-001".
- In-flight, not read: `Q/edge/model_plane_gate.rs` [U] is an untracked file in the edge crate and may gate the same paths.
- Nested worktree: `Q/.claude/worktrees/vigilant-banach-6933aa/` contains copies of quarry-runtime sources. Greps without `--exclude-dir=.claude` report false production callers.

---

## F-t2-006 Reap abandoned agent runs (idle and absolute TTL), cap concurrent runs, and make the fleet console real

Card: Priority, Partial, M, W1. JSON: verdict "reword"; the multi-instance registry idea is replaced by checkpoint-for-resume with run-id affinity. The fleet proxy is a separate S.

### 1. Citation check
26 citations checked (card plus verification.evidence). No skips.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | Q/edge/agent_routes.rs:1446 | C | OK | `let ttl_s = constraints.max_runtime_s.unwrap_or(120);`; stored only |
| 2 | Q/edge/agent_routes.rs:1502 | C | OK | `ttl_s,` in the lease request inside `start_run` (1273) |
| 3 | Q/edge/agent_routes.rs:1513 | C | OK | `.acquire(&lease)`; no admission check before it |
| 4 | Q/edge/agent_routes.rs:446-451 | C | OK | `enforce_step_budget` checks `max_steps` only (callers 2294, 3118) |
| 5 | Q/edge/agent_routes.rs:453-472 (card 453-471) | C | OK | `checkpoint_for_entry` |
| 6 | Q/edge/agent_routes.rs:1425-1446 | C | OK | resume path; ZDR resolution 1440-1446; checkpoint constraints authoritative (1366-1368) |
| 7 | Q/edge/agent_routes.rs:107 | C | OK | `pub type AgentRuns = Arc<StdMutex<HashMap<...>>>`: in-process registry |
| 8 | Q/edge/agent_routes.rs:1630-1641 (card 1639) | C | OK | `.insert(run_id, ...)` at 1636-1639; grep for limit, cap, busy, concurrent in 1273-1668 finds nothing relevant |
| 9 | Q/edge/agent_routes.rs:3562-3640 (card 3623-3626) | C | OK | `close_run` at 3562; `.remove(&run_id)` 3623-3626; `release` 3634 |
| 10 | Q/edge/agent_routes.rs:1733-1763 (write 1760) | C | OK | `set_browser_session_control`; `entry.control_mode = body.mode` at 1760 |
| 11 | Q/edge/agent_routes.rs:79-102 (not in card) | C | OK | `RunEntry`: `control_mode` 92; no last-activity, idle or started_at field |
| 12 | Q/edge/main.rs:323 | M | OK | `RuntimeLeasePool::new(4)` (Browserbase) |
| 13 | Q/edge/main.rs:337 | M | OK | `RuntimeLeasePool::new(2)` (local Chromium) |
| 14 | Q/edge/main.rs:1284-1290 | M | OK | ChromiumoxideDriver built with no pool (1284-1288); `agent_runs = new_runs()` at 1290 |
| 15 | Q/rt/lease_pool.rs:1-140 | C | OK | `RuntimeLeasePool` 34; `new` 43; `metrics_snapshot` 53; `try_acquire` 96 (RateLimited 103); `acquire_timeout` 124 |
| 16 | Q/rt/browser_driver.rs:128-142 | C | OK | `do_fetch` pool guard 137-142: the pool gates only scrape-render fetches |
| 17 | Q/br/chromiumoxide.rs:1914-1942 | C | OK | `acquire` 1914 calls `ensure_browser(lease.viewport)` at 1925; no semaphore |
| 18 | MP/go/services/browser-broker/internal/server/server.go:22 | C | OK | `defaultGrantTTL = 15 * time.Minute` |
| 19 | MP/go/services/browser-broker/internal/grant/grant.go:189 (not in card) | C | OK | `RevokeScoped`, called at `server.go:119`: a revocation call already exists |
| 20 | Q/edge/fleet_routes.rs:216-222 (JSON 200-222) | C | OK | `fleet_router` 216-223: POST /v1/fleets, GET /v1/fleets/:fleet_id, POST members, GET budget. No PATCH, no cancel |
| 21 | Q/edge/routes.rs:285 (not in card) | M | OK | `.merge(crate::fleet_routes::fleet_router())` |
| 22 | FE/src/features/agents/components/FleetRunConsole.tsx:26 | C | DRIFTED | `fetchFleet` begins at 27; the fetch is at 28; line 26 is blank |
| 23 | FE/src/features/agents/components/FleetRunConsole.tsx:131 (card 120-130, 155-163) | C | OK | placeholder text `live view: /v1/agent/runs/...` at 131; tiles 118-133 are placeholders; PATCH at 147 with `{status:'cancelled'}`; button "Pause fleet" at 155 (155-163 not re-read) |
| 24 | FE/src/shared/api/quarry-client.ts:127 | C | FALSE as evidence | line 127 is `listQuarryRequestQueues`; the file has no fleet call (grep) |
| 25 | FE/src/app/App.tsx:216 | M | OK | route `/agents/fleets/:fleetId`; uncommitted hunks (10, 39, 178, 242) do not touch it |
| 26 | MP/rust/services/execution-core/src/quarry_agent.rs:655 | C | OK | Model-side `close_run(run_id, org_id)` sends DELETE `/v1/agent/runs/{run_id}` |

### 2. Affected files
- `Q/edge/agent_routes.rs` [C, 3737]: `RunEntry` 79-102 (add `last_activity` and `started_at`); `AgentRuns` 107; `new_runs` 341; `enforce_step_budget` 446; `checkpoint_for_entry` 453; `start_run` 1273 (ttl 1446 and 1502; acquire 1513; insert 1636-1639); `browser_session` 1669; `set_browser_session_control` 1733; `step` 2228 (refresh last_activity); `execute_ws_action` 3086; `close_run` 3562 (sweeper must reuse its release, receipt and checkpoint body, so extract a helper); `agent_router` 3659 (only `DELETE /v1/agent/runs/:run_id`) and the no-op variant 3735.
- `Q/edge/main.rs` [M, 1569]: pools 323 and 337; agent driver 1284-1288; agent_runs 1290. The sweeper task spawns beside these and before serve (1513-1517). Uncommitted hunks do not overlap these lines.
- `Q/rt/lease_pool.rs` [C, 294]: per-org permit accounting (shared with 025); `metrics_snapshot` 53.
- `Q/br/chromiumoxide.rs` [C, 3551]: `ensure_browser` 194-270; the context-close path is not traced [UNVERIFIED].
- `Q/core/lease.rs` [C]: `BrowserLease.ttl_s: u32` at 13.
- `Q/rt/step_receipts.rs` [C]: `AgentRunCheckpoint` 39-50; `save_run_checkpoint` called at `agent_routes.rs` 507 and 1617; `load_run_checkpoint` at 1324.
- `Q/crates/quarry-runtime/migrations/0006_agent_run_checkpoints.sql` [C]: table `quarry_agent_run_checkpoints` (line 5). Use it for resume; no new table is needed for resume.
- `Q/edge/fleet_routes.rs` [C, 252] and `Q/rt/fleet.rs` [C, 343]: fleet registry; `get_budget` 172. Cancel or PATCH route would be added here.
- `Q/edge/routes.rs` [M, 1564]: `fleet_router` merge at 285.
- `GW/` has no fleet domain (grep finds no `fleets`). A proxy needs [NEW] `GW/domains/fleets.rs` (MISSING today) and a merge line in `GW/main.rs` [C] (domain merge list about 130-137).
- `FE/src/features/agents/components/FleetRunConsole.tsx` [C]: fetch 27-28; PATCH 147; placeholders 118-133.
- `FE/src/app/App.tsx` [M]: route 216.
- Janitor pattern: `Q/ctl/internal/janitor/janitor.go` [C, 115]: `Run` and `OptionsFromEnv`; started at `Q/ctl/cmd/control/main.go:200` [C].
- Sweeper pattern (other plane, copy and do not import): `MP/go/services/sandbox-manager/internal/process/sweeper.go` [C] (`NewSweeper`, `WithTiming`, `Start`); `cmd/main.go:149` [C]; `store.go:916` `SweepStale`.
- OpenAPI: `Q/docs/openapi.yaml` [M] has no `/v1/fleets` path (grep). Fleet routes are undocumented.
- Migrations: none needed for a memory-only reaper. Durable state uses the existing 0006 table.

### 3. Reusable but unwired
- `RuntimeLeasePool::metrics_snapshot` (`lease_pool.rs:53`): callers are tests only (273, 280, 288).
- No last-activity or idle concept exists in `RunEntry`. Nothing to reuse; add the fields.
- `FleetRegistry` and `fleet_router` are wired into the edge (state, routes 285), but no cancel or PATCH handler exists.
- `AgentRunCheckpoint` persistence is wired (save 507 and 1617; load 1324).
- `control_mode` is wired (write 1760; reads 470, 629, 655) and can be the takeover signal.
- Model Plane `RevokeScoped` is wired (`server.go:119`). Grant revocation is one RPC away, so the card's "optional" reading holds.
- `Q/ctl/internal/janitor` is wired at `cmd/control/main.go:200`, but its package has no test file.

### 4. Tests
- `Q/edge/agent_routes.rs:2063` `step_budget_is_fail_closed_once_limit_is_reached` [C]: the only budget test.
- `Q/edge/fleet_routes.rs:231` `fleet_budget_snapshot_aggregates_receipt_cost` [C].
- `Q/rt/fleet.rs` tests [C]: `fleet_registry_create_and_get`, `fleet_registry_rejects_duplicate_fleet_id`, `fleet_budget_tracker_aggregates_across_members`, `fleet_budget_tracker_back_pressure_on_over_budget`, `fleet_task_add_member_is_idempotent`.
- `Q/rt/lease_pool.rs` tests [C]: `try_acquire_respects_capacity` (240), `permit_released_on_guard_drop` (249), `acquire_timeout_returns_error_when_pool_full` (260).
- No reaper or TTL test exists. `Q/ctl/internal/janitor/` has no `_test.go` [C].
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge step_budget`; `cargo test -p quarry-runtime fleet`; `cargo test -p quarry-runtime lease_pool`. Frontend: `cd "FE" && pnpm test` (no FleetRunConsole test found in scope [UNVERIFIED]).
- CI: `quarry-v2-ci.yml` cargo-check, `cargo test --workspace` (line 205) [C]; `verevonv3-ci.yml` [M].

### 5. Feature flags and env
- No env for TTL or caps exists. `max_runtime_s` defaults to 120 (`agent_routes.rs:1446`). The gateway sends `max_steps` 12 and `max_runtime_s` 120 for manual sessions (`GW/domains/browser.rs:485-487`).
- Janitor kill switch: `QUARRY_RETENTION_DAYS` (default 0 = off) and `QUARRY_RETENTION_DRY_RUN` (default true) at `janitor.go:43-47` [C]. Reuse this pattern for a reaper switch.
- New keys (idle TTL, caps) belong in `Q/edge/config.rs` [M] under the `QUARRY_EDGE__` prefix (`compose/docker-compose.yml:70` [C]).
- Cargo feature `browser-agent` (default on, `Q/crates/quarry-edge/Cargo.toml` 68-74).

### 6. Surprises
- Two card citations are wrong: `FleetRunConsole.tsx:26` (blank line) and `quarry-client.ts:127` (request-queue list, not fleets). See the table.
- The "Pause fleet" button sends `status: 'cancelled'` with a PATCH. The edge has no PATCH route (fleet_routes 216-223). The button is a no-op against the edge today.
- `/v1/fleets` is absent from the gateway and from `openapi.yaml`. The SPA fetches a same-origin path with no proxy, so the console cannot load real data [INF from the absent proxy; not browser-tested].
- The fleet budget route already aggregates receipt cost (test at `fleet_routes.rs:231`). The card does not mention this spend roll-up.
- `close_run` is an axum handler that takes `State` and `Extension`. A reaper must call a shared helper, not the handler.
- `Q/edge/main.rs` is [M]; a sweeper spawn added there must be rebased onto the uncommitted diff.

---

## F-t2-008 Enforce cost and runtime budgets in the Quarry executor, meter browser time, and clamp crawls to the remaining entitlement

Card: Priority, Partial, L (JSON revised to L; slice 1 is S), W2. JSON verdict "reword": the gap is confirmed, but the runtime and cost caps would break manual sessions and must be scoped to agent runs.

### 1. Citation check
26 citations checked (card plus verification.evidence). No skips.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | Q/edge/agent_routes.rs:446-451 | C | OK | `enforce_step_budget` checks only `max_steps` |
| 2 | Q/edge/agent_routes.rs:2294 | C | OK | HTTP step call; WS call at 3118 |
| 3 | Q/edge/agent_routes.rs:433-440 | C | OK | `default_constraints`: max_steps 0, max_runtime_s None, max_cost_usd None (unbounded) |
| 4 | Q/edge/agent_routes.rs:1446 | C | OK | `max_runtime_s.unwrap_or(120)` becomes lease ttl_s |
| 5 | Q/edge/agent_routes.rs:1370-1372 | C | OK, stale comment | comment says max_cost_usd enforcement is "in AgentLoop"; AgentLoop has no HTTP caller |
| 6 | Q/edge/agent_routes.rs:1436-1456 | C | OK | resume constraints come from the checkpoint (1366-1368); ZDR checks 1436-1446 |
| 7 | Q/edge/agent_routes.rs:2222-2310 | C | OK | `step` 2228-2294: grant check, sensitive gate, budget check |
| 8 | Q/edge/agent_routes.rs:2346-2362 | C | OK | `cost_usd = observation_receipt_cost_usd(&obs)`, fallback `estimated_action_cost_usd` (2350); not accumulated; `RunEntry` has no spend field |
| 9 | Q/edge/agent_routes.rs:700-765 | C | OK | `observation_receipt_cost_usd` 709; `telemetry_receipt_cost_usd` 713-718; `estimated_action_cost_usd` 725 calls `action_cost::estimate_action_cost` at 761 |
| 10 | Q/rt/agent_loop.rs:154-163 | C | OK | `over_budget` |
| 11 | Q/rt/agent_loop.rs:140-215 | C | OK | `record_cost` 141; `spent_usd` 150; `execute` 186; max_runtime 194; max_steps 224; over_budget 248; flat charge 295 |
| 12 | Q/rt/usage.rs:1-110 | C | OK | `UsageEvent` 50; metric constants 89-93 (SCRAPE_PAGE, CRAWL_SEED, SEARCH_QUERY, ANSWER_SYNTH, BATCH_URL: no browser metric); `UsageMeter` 99; `NatsUsageMeter` 119-150 |
| 13 | Q/rt/action_cost.rs:1-40 | C | OK | flat table `estimate_action_cost` 21; `estimate_planner_cost` 39 |
| 14 | Q/rt/observation.rs:1000-1018 | C | OK | `ctx.step += 1` at 1015 (shared by human and agent steps) |
| 15 | Q/edge/routes.rs:553-680 | M | OK | `CrawlRequest` struct begins 533; `zdr` 676-678; fail-closed comment 555-560 |
| 16 | MP/rust/services/execution-core/src/browser_agent.rs:244-345 | C | OK | `PlanConfig` fields 249-255; `check_limits` 320-330 |
| 17 | MP/rust/services/execution-core/src/browser_agent.rs:640-690 | C | OK | `max_cost_usd: plan.config.max_cost_usd` forwarded at 666 and never compared |
| 18 | MP/rust/services/execution-core/src/browser_agent.rs:255 | C | OK | `pub max_cost_usd: Option<f64>` |
| 19 | MP/go/services/cost-core/internal/ledger/ledger.go:15-45 | C | OK | `ErrBudgetExceededCost` 28; `ValidateBudget` 165; enforcement at 554 (model spend only) |
| 20 | GW/domains/browser.rs:455-492 | C | OK | `create_session` 451; caps `max_steps: 12`, `max_runtime_s: 120` at 485-487 |
| 21 | GW/domains/browser.rs:300-302 | C | DRIFTED (wrong struct) | 298-303 is `SuggestActionBody`. The body with max_steps, max_runtime_s and max_cost_usd is `StartAiRunBody` at 312-330 (fields 317, 319, 329); forwarded at 1239-1243 |
| 22 | GW/domains/browser.rs:663-732 | C | OK | `run_action` at 663: human browser actions use the same /step path |
| 23 | GW/domains/knowledge/quarry.rs:386-405 | C | OK | `max_pages.min(5_000)` at 404, applied only when `max_pages` is sent (396-401) |
| 24 | Q/orch/internal/workflows/workflows.go:75-110 and 385-460 | C | OK | `CrawlJobInput` 75-85 (MaxPages 85); `CrawlJobWF` 385; unlimited when 0 (456, 555) |
| 25 | Q/orch/internal/jobs/dispatcher.go:236-262 | C | OK | `WorkflowExecutionTimeout: 30 * time.Minute` at 254; `MaxPages` from params 452 |
| 26 | Q/ctl/internal/store/resources_v2_mem.go:115-126 | C | OK | `memTeamUsage.CreditUsage`: pages times creditsPerPage; comment "No ceiling is modeled" |

### 2. Affected files
- `Q/edge/agent_routes.rs` [C, 3737]: `RunEntry` 79-102 (add `spent_micro_usd` and `started_at`; none today); `default_constraints` 433; `enforce_step_budget` 446 (becomes `enforce_budget`; callers 2294 and 3118); constraint resolution 1366-1372 and ttl 1446 (min of request, grant claim, org default); cost helpers 709-765; receipt cost 2346-2362.
- `Q/rt/step_receipts.rs` [C]: `StepReceipt.cost_micro_usd` 209; `ReceiptBuilder::complete` cost conversion 497-513; test 753.
- `Q/rt/action_cost.rs` [C]: flat table (21).
- `Q/rt/usage.rs` [C, 298]: add a `BROWSER_SECOND` constant near 88-94; `NatsUsageMeter` is wired at `Q/edge/main.rs:451-458` [M, not touched].
- `Q/rt/agent_loop.rs` [C, 895]: reference only; no production caller.
- `Q/rt/fleet.rs` [C, 343]: `FleetBudgetTracker` 99-120, `over_budget` 150 (no caller outside fleet.rs).
- `Q/edge/routes.rs` [M, 1564]: `CrawlRequest` 533-679. Add `max_cost_usd` and rate fields, return clamped values.
- `Q/edge/config.rs` [M] and `Q/edge/state.rs` [M]: org default-cap keys would land here.
- `MP/rust/services/execution-core/src/browser_agent.rs` [C]: `PlanConfig` 249-255, `check_limits` 320-330, forwarding 659-666.
- `GW/domains/browser.rs` [C, 6317]: `StartAiRunBody` 312-330; `create_session` 451-492; `run_action` 663-732.
- `GW/domains/knowledge/quarry.rs` [C, 1158]: clamp at 396-404.
- `Q/orch/internal/workflows/workflows.go` [C, 640]: `CrawlJobInput` 75-85; limits 456, 555; `durableFrontierQueueName` 612.
- `Q/orch/internal/jobs/dispatcher.go` [C, 551]: timeout 254; params 452.
- `Q/ctl/internal/store/resources_v2_mem.go` [C, 426] and `pg/resources_v2.go` [C, 584]: `CreditUsage` read model (add a ceiling read here).
- `CP/billing-core/internal/billing/defaults.go` [C]: `defaultQuotaLimitsByPlan` 61-92, read at 161-163.
- `GW/domains/orgs/quotas.rs` [C]: `ENFORCED_QUOTA_KEYS` 27 (two keys). A new key needs a reader and an allowlist change.
- Migrations: none for slice 1. A persisted ledger would go to `Q/ctl/internal/store/pg/migrations/015_*.sql` [NEW; highest existing is 014_fleets.sql, C].

### 3. Reusable but unwired
- `AgentLoop::over_budget`, `record_cost`, `spent_usd` (`agent_loop.rs` 141, 150, 154-163): no production caller. Grep outside agent_loop.rs finds only `Q/rt/lib.rs:100` (re-export) and doc comments (`fleet.rs:94`, `agent_memory.rs:6`, `test_site.rs:12`, `step_receipts.rs:3`, `agent_routes.rs:722`).
- `FleetBudgetTracker` (`fleet.rs:99-120`): no caller outside fleet.rs.
- `estimate_planner_cost` (`action_cost.rs:39`): caller not traced [UNVERIFIED].
- Wired and reusable: receipt cost (709-765); `NatsUsageMeter` (main.rs 451-458); `ErrBudgetExceededCost` (cost-core `ledger.go:554`); quarry-control CreditUsage (read-only).

### 4. Tests
- `Q/rt/agent_loop.rs` inline tests (10) [C]: `record_cost_accumulates_and_triggers_termination`, `record_cost_ignores_negative_and_nan`, `mock_planner_drives_agent_loop_e2e`.
- `Q/rt/action_cost.rs:54` `flat_table_matches_known_actions`; `:74` `planner_call_returns_estimated_cost_when_model_plane_omits` [C].
- `Q/rt/fleet.rs`: `fleet_budget_tracker_aggregates_across_members`, `fleet_budget_tracker_back_pressure_on_over_budget` [C].
- `Q/edge/fleet_routes.rs:231` `fleet_budget_snapshot_aggregates_receipt_cost` [C].
- `Q/edge/agent_routes.rs:1991` `provider_meter_cost_is_copied_exactly_and_unknown_remains_unknown_in_telemetry`; `:2063` `step_budget_is_fail_closed_once_limit_is_reached` [C].
- `MP/rust/services/execution-core/src/browser_agent.rs:1141` `max_steps_exhausts_the_plan_without_reporting_completion`; `:1160` (max_runtime_s) [C].
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime agent_loop action_cost fleet`; `cargo test -p quarry-edge fleet_budget`; `cd "apps/Model Plane/rust" && cargo test -p execution-core max_`; `cd "apps/Model Plane/go/services/cost-core" && go test ./internal/ledger/...`.
- CI: `quarry-v2-ci.yml` cargo-check (205) [C]; `model-plane-rust-ci.yml` (119) [M]. No workflow references cost-core (grep), so its Go tests are not in CI.

### 5. Feature flags and env
- Manual sessions hard-code `max_steps: 12` and `max_runtime_s: 120` (`GW/domains/browser.rs:485-487`) [C].
- `max_cost_usd` is optional, default None (`agent_routes.rs:438`).
- No budget env var exists. Org default caps would come from config [M] or claims (F-t2-022).
- Crawl: `max_pages` absent means unlimited (`workflows.go:456`).

### 6. Surprises
- Stale comment: `agent_routes.rs:1370-1371` says cost is enforced in AgentLoop. No HTTP path calls AgentLoop, so cost is not enforced on the live path.
- The gateway forwards a client-supplied `max_cost_usd` (`browser.rs:1243`). Quarry does not enforce it, so a caller can set a cap that is silently ignored.
- The card's proposed runtime cap would cap human browsing. Confirmed: manual sessions carry 12 and 120 (485-487), human actions use `run_action` (663), and `ctx.step` is shared (observation.rs 1015).
- The JSON citation `browser.rs:300-302` points at the wrong struct (see the table).
- Execution-core already enforces `max_runtime_s` on its own clock (`browser_agent.rs:324-326`). Only its `max_cost_usd` check is missing.
- A fleet spend roll-up already exists (`fleet_routes.rs:231`): sums receipts, which the card does not mention.

---

## F-t2-025 Per-org capacity: concurrency caps with backlog and fair share, per-class rate limits and plan-aware quotas

Card: Partial, L, W2, Ingestion Plane plus Frontend gateway plus Control Plane. JSON: the card's premise is partly wrong. The gateway already rate-limits every route, the backlog does not belong in the per-crawl Postgres queue, and cross-plane work is larger than M.

### 1. Citation check
32 rows (card plus JSON evidence). Rows 3, 6 and 8 were not opened; row 17 was read only for 130-143. Rows 3, 6, 8 and 17 are marked NOT-RE-READ.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | GW/rate_limit.rs:50-58 | C | OK | `DEFAULT_RPM: f64 = 600.0` at 58; doc (51-56) says the limiter runs before require_session and keys by IP |
| 2 | GW/rate_limit.rs:26-30, 74-84 (not in card) | C | OK, extra | limiter is Redis-backed (Lua with `redis.call('TIME')` at 82); local fallback when the cache is absent |
| 3 | GW/rate_limit.rs:269-290, 350-376 | C | NOT-RE-READ | key selection org > user > ip (test `prefers_org_then_user_over_ip`) |
| 4 | GW/main.rs:147-148 | C | OK | `.layer(rate_limit_middleware)` at 147; `Extension(rate_limiter)` at 148: global layer |
| 5 | GW/domains/mcp.rs:54-60 | C | OK | per-org inner limiter: `route_layer(rate_limit_middleware)` 59 and `require_session` 60 |
| 6 | GW/domains/agents_runs.rs:49-51; eval.rs:40; memory.rs:59 | C | NOT-RE-READ | same pattern per JSON |
| 7 | GW/config.rs:255-259 | C | OK | `rate_limiter` from cache (255); `mail_task_rate_limiter` from cache with rpm (256-259) |
| 8 | GW/config.rs:279-285 | C | NOT-RE-READ | `mail_task_rate_limit_rpm()` |
| 9 | GW/domains/orgs/quotas.rs:15-27 | C | OK | doc 15-20; `ENFORCED_QUOTA_KEYS` at 27 = two keys (`max_cost_per_run_usd_micros`, `max_tokens_per_run`); check at 82 |
| 10 | GW/domains/browser.rs:448 | C | OK | `route_layer(require_session)` on browser routes |
| 11 | GW/domains/search.rs:46 | C | OK | `route_layer(require_session)` on search routes |
| 12 | Q/edge/api_error.rs:46-60 | C | OK | `rate_limited(message, retry_after_seconds)` 46-52: RATE_LIMITED envelope; callers `extract_routes.rs:336`, `map_routes.rs:179` |
| 13 | Q/edge/auth.rs:62-91 | C | OK | `Claims` 62-70 (sub, iss, exp, org_id, user_id); grep for plan, entitlement or quota finds only doc comments |
| 14 | Q/rt/lease_pool.rs:29-53, 96-150 | C | OK | `RuntimeLeasePool` 34; `new` 43; `metrics_snapshot` 53; `try_acquire` 96-108; `acquire_timeout` 124-140. Semaphore only, no org key |
| 15 | Q/edge/main.rs:323, 337 | M | OK | pool sizes 4 and 2, hard-coded, no env |
| 16 | Q/edge/agent_routes.rs:107, 1636-1639 | C | OK | `AgentRuns` map; insert with no capacity check |
| 17 | Q/rt/browser_driver.rs:137-142 (card 138-142, 160-163) | C | OK for 137-142; 160-163 NOT-RE-READ | pool acquire keyed by host; internal lease org_id empty (JSON) |
| 18 | Q/rt/postgres_queue.rs:271-290 | C | OK | `pop` 271: `WHERE org_id AND queue_id AND status = 'queued'` (286); `ORDER BY priority DESC, enqueued_at ASC` (287) |
| 19 | Q/rt/postgres_queue.rs:74 | C | OK | `PostgresRequestQueue` struct |
| 20 | Q/orch/internal/workflows/workflows.go:518-531, 612 | C | OK | `queueName := durableFrontierQueueName(in.RunID)` at 525; function at 612 |
| 21 | Q/orch/internal/jobs/dispatcher.go:346-350 | C | OK | poll `ControlBaseURL + "/v1/jobs/?limit=50"` at 348 |
| 22 | Q/ctl/internal/store/pg/resources_v2.go:102-125 | C | OK | `teamUsageStore.Concurrency` 102-133: per-host counts, read-only, no ceiling |
| 23 | Q/edge/resource_routes.rs:889-905 | C | OK | `team_concurrency` at 889 forwards to `/v1/team/concurrency` |
| 24 | Q/rt/autoscale.rs:1-25 | C | OK | `global_autoscale()` 18-21 uses `AutoscaledPool::from_parallelism()` (CPU-sized) |
| 25 | Q/rt/host_scheduler.rs:1-40 | C | OK | per-host AIMD scheduler (doc 1-12) |
| 26 | MP/rust/services/model-gateway/src/org_quota.rs:1-60, 106-112 | C | OK | constants 24-26; `limits_from_rows` 79-93 treats 0 or less as unset (comment 71-79) |
| 27 | MP/rust/services/model-gateway/src/rate_limit.rs:1-40 | C | OK | per-org token bucket; `org_rpm` overrides (24-32) |
| 28 | MP/go/services/browser-broker/internal/server/ratelimit.go:1-60 | C | OK | fixed window keyed by peer (`rateLimitBucket` 18, `rateLimiter` 24): per peer, not per org |
| 29 | CP/billing-core/internal/billing/defaults.go:61-92 | C | OK | `defaultQuotaLimitsByPlan` (api_calls, users, storage_mb); used at 161-163 |
| 30 | CP/billing-core/internal/billing/types.go:36-37 | C | OK | `QuotaLimits map[string]float64` at 37 |
| 31 | CP/org-core/internal/org/types.go:34-42 | C | OK | `Quota` {OrgID, Key, Value, Limit, ResetPeriod, LastResetAt, UpdatedAt} |
| 32 | CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json:29-73 | M | OK | quarry-edge rows present; uncommitted hunks at 35 and 37 change only the `data:rerank` row; no org-core or billing-core row |

Not re-read because of the cap: rows 3, 6, 8, 17 (160-163), and the `api_error.rs` 94 test. The card's `quarry-client.ts:127` reference is a request-queue list (see F-t2-006).

### 2. Affected files
- `GW/rate_limit.rs` [C, 577]: `DEFAULT_RPM` 58; `from_cache`; the middleware. New class-keyed limiters go here, following the `mcp.rs` pattern.
- `GW/main.rs` [C]: global layer 147-148. New classes use `route_layer` in domain routers (`browser.rs` 448; `search.rs` 46).
- `GW/config.rs` [C]: limiter construction 255-259 and mail-task limiter 279-285.
- `GW/domains/mcp.rs` [C], `agents_runs.rs` [C], `eval.rs` [C], `memory.rs` [C]: per-org limiter pattern (not re-read beyond mcp).
- `GW/domains/orgs/quotas.rs` [C]: `ENFORCED_QUOTA_KEYS` 27 and check 82. A new key requires its reader and this allowlist.
- `GW/domains/browser.rs` [C, 6317]: classing for session routes (create 451, run_action 663).
- `Q/rt/lease_pool.rs` [C, 294]: per-org permits and `acquire_timeout` 124.
- `Q/rt/browser_driver.rs` [C]: adapter acquire must receive `org_id` (empty today).
- `Q/edge/main.rs` [M]: pool construction 323 and 337.
- `Q/edge/agent_routes.rs` [C, 3737]: start_run cap (insert 1636-1639).
- `Q/edge/api_error.rs` [C, 109]: RATE_LIMITED envelope (reuse).
- `Q/edge/auth.rs` [C]: Claims has no plan field. Adding plan data by claims needs an ADR and an auth-core change.
- `Q/rt/postgres_queue.rs` [C, 672]: per-crawl frontier. Not the place for a cross-job backlog (card agrees).
- `Q/orch/internal/workflows/workflows.go` [C, 640] and `jobs/dispatcher.go` [C, 551]: job admission seam (poll 348).
- `Q/ctl/internal/store/pg/resources_v2.go` [C, 584] and `resources_v2_mem.go` [C, 426]: Concurrency view; new keys.
- `Q/edge/resource_routes.rs` [C]: team concurrency view.
- `Q/rt/autoscale.rs` [C, 165], `host_scheduler.rs` [C]: crawl-path admission (reference).
- `MP/rust/services/model-gateway/src/org_quota.rs` [C]: pattern (service-credential read, fail-open).
- `CP/org-core/internal/org/types.go` [C] and `CP/billing-core/internal/billing/defaults.go` [C]: plan defaults and quota keys.
- Two cross-plane manifests, both [M]: `CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json` and `Q/crates/quarry-runtime/contracts/cross-plane-token-requests.json` (plus `.sha256`, [M]). An org-core read grant needs a row in both and a regenerated `.sha256`.
- Migrations: org quota rows need none. A backlog table would go to `Q/ctl/internal/store/pg/migrations/015_*.sql` [NEW; highest is 014_fleets.sql, C].

### 3. Reusable but unwired
- `ApiError::rate_limited`: wired (two callers). Reuse for the typed 429.
- `defaultQuotaLimitsByPlan`: wired inside billing-core (161-163), but holds no browser or crawl keys.
- `team_concurrency` view: wired (resource_routes 889; control 102).
- `host_scheduler.rs` AIMD and `autoscale.rs`: wired on the crawl path. Callers of `global_autoscale` in the agent path were not traced [UNVERIFIED].
- The model-gateway `org_rpm` overrides and the browser-broker `ratelimit.go`: wiring not traced [UNVERIFIED].

### 4. Tests
- Gateway `rate_limit.rs` [C]: `allows_requests_within_limit`, `rejects_when_exhausted_with_retry_after`, `isolates_keys`, `refills_over_time`, `try_acquire_falls_back_to_local_without_conn`, `distributed_rate_limit_call_has_a_short_deadline`, `distributed_key_is_namespaced_and_scannable`, `from_env_rejects_non_positive_and_keeps_default`, `prefers_org_then_user_over_ip`, `falls_back_to_client_ip_pre_auth`, `falls_back_to_real_ip_then_anonymous`.
- `orgs/quotas.rs` test at about 167 asserts the allowlist [C].
- `Q/rt/lease_pool.rs` tests (240-290) [C]; `Q/rt/host_scheduler.rs` 12 inline tests [C] (names not listed).
- Commands: `cd "FE/apps/gateway" && cargo test rate_limit`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime lease_pool host_scheduler`; `cd "apps/Model Plane/rust" && cargo test -p model-gateway rate_limit`.
- CI: `verevonv3-ci.yml` gateway-rust [M]; `quarry-v2-ci.yml` cargo-check [C]; `model-plane-rust-ci.yml` [M]. The Go services in `Q/ctl` are only govulncheck'd in CI (see section 6).

### 5. Feature flags and env
- `GATEWAY_RATE_LIMIT_RPM`: parsed at `GW/rate_limit.rs:128-135`; default 600 [C].
- Mail-task RPM: `mail_task_rate_limit_rpm()` at `GW/config.rs:279-285` [NOT-RE-READ; env name not verified].
- Quarry pool sizes are hard-coded (`main.rs` 323, 337). No env.
- Gateway limiter depends on the Redis cache (rate_limit.rs 26-30).

### 6. Surprises
- The card's premise is partly wrong. The gateway limiter already covers every route, keyed by IP, and is Redis-backed (not per-process). The card's "no rate limiting on these routes" is false.
- `ENFORCED_QUOTA_KEYS` holds only two keys (`orgs/quotas.rs:27`). Its comment (15-20) says a key ships only with its reader.
- Two cross-plane manifests are both uncommitted. Changing one without the other diverges them.
- `team_concurrency` already exists, so queue position is partly built.
- The model-gateway `org_rpm` is a precedent for org-keyed limits with overrides (`rate_limit.rs` 24-32).
- The Go services in `Q/ctl` and `Q/orch` have govulncheck only in `quarry-v2-ci.yml`. No `go test` step for them appears in that file (grep).

---

## F-t2-026 Per-operation browser deadlines, bounded tab attach and retry-once on a destroyed execution context

Card: Partial, S, W1, Ingestion Plane (quarry-browser). JSON: verdict "reword": keep it inside quarry-browser; drop the Model Plane planner rule.

### 1. Citation check
20 citations checked (card plus verification.evidence). One range (row 12, beyond 130-143) was not opened.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | Q/br/chromiumoxide.rs:76-77 | C | OK | `DOWNLOAD_BEGIN_TIMEOUT` 10 s; `DOWNLOAD_COMPLETE_TIMEOUT` 30 s |
| 2 | Q/br/chromiumoxide.rs:226-262 | C | OK | `BrowserConfig::builder()` at 226; no `request_timeout(` or `launch_timeout(` call in the file (grep) |
| 3 | Q/br/chromiumoxide.rs:683-693 (card 685) | C | OK | 685 is `timeout(Duration::from_secs(5), capture_snapshot(&page))` in the persist-profile branch: profile capture, not an AX snapshot. The JSON note is right; the card is right |
| 4 | Q/br/chromiumoxide.rs:1733-1740 | C | OK | caller-bounded `timeout(timeout, events.next())` at 1734 |
| 5 | Q/br/chromiumoxide.rs:1245-1278 | C | OK | `TargetRepairRequired` at 1247, 1254, 1269, 1276 |
| 6 | Q/br/chromiumoxide.rs:1988-2023 | C | OK | goto (2002) and content (2020) map failures to `DriverFailed` strings |
| 7 | Q/br/chromiumoxide.rs:2100-2140 and 2116 | C | OK | exact-node resolution; comment 2115-2117 says a stale node fails; TargetRepair 2126, 2133 |
| 8 | Q/br/chromiumoxide.rs:2286-2296 (not in card) | C | OK, extra | `DOWNLOAD_BEGIN_TIMEOUT` with `ErrorCode::Timeout` at 2296: the existing typed-timeout pattern |
| 9 | Q/core/error.rs:10-70 | C | OK | `Timeout` 17; `retryable()` 38-43 (Timeout, RateLimited, UpstreamBlocked, Internal); HTTP 504 for Timeout (54), 502 for DriverFailed (55) |
| 10 | Q/core/error.rs:38-43 | C | OK | DriverFailed is not retryable |
| 11 | Q/rt/lease_pool.rs:118-140 | C | OK | `acquire_timeout` 124-140 returns an error on timeout |
| 12 | Q/rt/browser_driver.rs:135-250 | C | NOT-RE-READ beyond 130-143 | cap |
| 13 | Q/edge/agent_routes.rs:446-451 | C | OK | `enforce_step_budget`: max_steps only |
| 14 | GW/domains/browser.rs:483-490 (485-487) | C | OK | 12 and 120 defaults inside `create_session` (451) |
| 15 | MP/rust/services/execution-core/src/runtime_loop/retry.rs:1-80 | C | OK | `MAX_TOOL_ATTEMPTS = 3` (37); `is_transient_tool_failure` 130; `tool_call_is_retryable` 149 (risky gate first) |
| 16 | MP/rust/services/execution-core/src/permission/mod.rs:255-275 | C | OK | `browser.act` in the risky list (about 271): no silent retry |
| 17 | [registry] chromiumoxide-0.9.1 src/handler/mod.rs:34 | registry | OK | `REQUEST_TIMEOUT: u64 = 30_000` at 34 |
| 18 | [registry] chromiumoxide-0.9.1 src/handler/mod.rs:660-690 | registry | OK | `HandlerConfig` default: `request_timeout: Duration::from_millis(REQUEST_TIMEOUT)` at about 681 |
| 19 | [registry] chromiumoxide-0.9.1 src/browser/config.rs:15-155 | registry | OK | `launch_timeout` default at 150; `request_timeout` default at 155; setters at 213-218 |
| 20 | Q/br/chromiumoxide.rs:3253-3516 (CHROMIUMOXIDE_TEST gates, not in card) | C | OK, extra | live-browser tests are gated by env `CHROMIUMOXIDE_TEST` |

### 2. Affected files
- `Q/br/chromiumoxide.rs` [C, 3551]: constants 76-77; `BrowserConfig` builder 226-262 (add `request_timeout` and `launch_timeout`); goto 509 and 2002; `evaluate` 3003; content 2020; `capture_snapshot` 1771; download 2286-2298.
- `Q/br/deadlines.rs` [NEW]: `cdp_call(op, fut)` wrapper and a constants table.
- `Q/core/error.rs` [C, 109+]: reuse `ErrorCode::Timeout`; no change.
- `Q/rt/browser_driver.rs` [C]: adapter wraps driver calls (135-250, not re-read).
- `Q/edge/agent_routes.rs` [C, 3737]: per-run overrides, only if passed through `AgentConstraints.max_runtime_s` (1446) and clamped to a Quarry maximum.
- `Q/rt/lease_pool.rs` [C]: acquire deadline reuse (124).
- `GW/domains/browser.rs` [C]: 485-487 defaults; no change.
- `MP/.../execution-core/src/runtime_loop/retry.rs` [C] and `permission/mod.rs` [C]: no change (card's planner rule is dropped).
- Migrations: none. Compose: none.

### 3. Reusable but unwired
- `BrowserConfig::request_timeout` and `launch_timeout` setters ([registry] `config.rs:213-218`): not called by Quarry.
- A retry helper for CDP calls: none exists in quarry-browser (grep for "retry" in chromiumoxide.rs finds nothing). The only retry helper is Model-side `tool_call_is_retryable` (risk-gated).
- `BrowserTelemetry.startup_latency_ms` (JSON reuse for calibration): not re-verified [UNVERIFIED].

### 4. Tests
- Live-browser tests in `chromiumoxide.rs` gated by `CHROMIUMOXIDE_TEST` (lines 3253, 3332, 3360, 3406, 3459, 3516) [C].
- `MP/rust/services/execution-core/src/runtime_loop/retry.rs:226` `recognizes_real_transient_transport_and_throttle_failures` [C].
- No deadline or context-destroyed test found in chromiumoxide.rs (grep of the test module not run) [UNVERIFIED].
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo check -p quarry-browser --features chromiumoxide && cargo test -p quarry-browser --features chromiumoxide`.
- CI: `quarry-v2-ci.yml` cargo-check job, lines 208-211 [C].

### 5. Feature flags and env
- `CHROMIUMOXIDE_TEST`: test-only gate.
- `QUARRY_BROWSER_SETTLE_MS` (`Q/rt/browser_driver.rs:47`, `settle_budget`) [C]: existing bounded settle budget.
- Cargo feature `chromiumoxide` (quarry-browser, optional) and edge feature `browser-agent` (default on).
- Per-run override: `AgentConstraints.max_runtime_s` (`agent_routes.rs:1446`).

### 6. Surprises
- The card's `chromiumoxide.rs:685` description is correct for profile capture. The JSON's "evidence line is wrong" note is itself correct.
- No retry helper exists in quarry-browser, so "reuse" for retry-once means writing new code.
- Any call without an explicit setting uses the registry default of 30 s (`REQUEST_TIMEOUT`).
- The browser relaunches on any viewport change (`ensure_browser` 194-197 and 269). That path bypasses per-call deadlines while it runs [INF].
- The card's Model Plane rule is already excluded by design (`permission/mod.rs` gates browser.act), so dropping it is safe.

---

## F-t2-029 Quarry /metrics recorder, per-run statistics and graceful drain, then a span vocabulary, memory-aware admission and alert rules

Card: Stub, L, W2, Ingestion Plane plus Control Plane observability. JSON: verdict "reword": the recorder and /metrics are missing; graceful SIGTERM already exists; the alerting phase is new infrastructure.

### 1. Citation check
22 checked (card plus verification.evidence). No skips.

| # | Citation | Tag | Verdict | Live finding |
|---|---|---|---|---|
| 1 | Q/edge/telemetry.rs:1-16 (card 6-14) | C | OK | module doc: `metrics` and `metrics-exporter-prometheus` are inert; no recorder; no /metrics served. File is 64 lines |
| 2 | Q/edge/telemetry.rs:29 | C | OK | OTLP exporter only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set |
| 3 | Q/edge/search_routes.rs:226-233 (card 230) | M | OK | comment: tracing used deliberately because nothing installs a recorder. Uncommitted diff (+530/-81) has no hunk covering 226-233 |
| 4 | Cargo.toml:113-116 (card 115) | C | OK | `metrics = "0.24"`, `metrics-exporter-prometheus = "0.16"` |
| 5 | Q/crates/quarry-edge/Cargo.toml:56-59 (card 58) | C | OK | both declared as workspace deps |
| 6 | Q/rt/lease_pool.rs:53 (24, 62) | C | OK | `metrics_snapshot` 53; `LeaseMetricsSnapshot` 24; callers are tests only (273, 280, 288) |
| 7 | Q/edge/main.rs:84-111 (card 84-100) | M | OK | `shutdown_signal` 84; SIGTERM handler 101; log lines 110-111. Uncommitted hunks do not touch 84-111 |
| 8 | Q/edge/main.rs:1513-1517 (card 1513-1535) | M | OK | `format!("0.0.0.0:{}", cfg.port)` (1513); `TcpListener::bind` (1515); `with_graceful_shutdown(shutdown_signal())` (1517). One public listener; no metrics listener |
| 9 | Q/edge/main.rs:323, 337 (JSON 331-340) | M | OK | pool sizes, as in F-t2-006 |
| 10 | Q/rt/autoscale.rs:1-35 | C | OK | `from_parallelism` (18-21): CPU-sized; no memory or cgroup signal (grep) |
| 11 | Q/br/chromiumoxide.rs:194-270 | C | OK | `ensure_browser` 194; reuse only when `active_viewport == viewport` (197); otherwise relaunch and set `active_viewport` (269) |
| 12 | Q/orch/cmd/orchestrator/main.go:119-131 | C | OK | root context cancelled on SIGINT or SIGTERM (comment 120-125) |
| 13 | CP/observability/prometheus.yml:1-47 (jobs 14-43) | C | OK | jobs: prometheus, org-core, billing-core, user-core, session-core, audit-core, verevon-gateway. No quarry job; no `rule_files` |
| 14 | AP/observability/prometheus.yml:1-16 | C | OK | "Add each service here only when its metrics endpoint ships" |
| 15 | IC/internal/api/server.go:178-189 | M | OK | `app.Get("/metrics", internalAuth, ...)` at 178; uncommitted hunks at 59, 217, 1368, 2987, 3008 do not touch it |
| 16 | Q/pkg/quarryotel/quarryotel.go:8-15, 36-50 | C | OK | W3C TraceContext set at 44; composite propagator at 83. Go side only |
| 17 | Q/edge and Q/rt (grep `traceparent`) | C | OK | no traceparent extraction in any Quarry Rust crate (grep finds no file) |
| 18 | Q/docs/PLAN.md:359 | C | STALE | "Prometheus metrics (already wired in edge via metrics-exporter-prometheus)" |
| 19 | Q/docs/GAP.md:257 | C | STALE | row "Metrics endpoint | ✅ /metrics" |
| 20 | Q/docs/gap-quarry.md:175 | C | STALE | QRY-15 row marks "traceparent propagated edge -> runtime -> control" as done; the Rust edge does not extract it |
| 21 | Q/core/contracts.rs:100-123 | C | OK | `BrowserTelemetry` (100); `verified_action_cost_micro_usd` (123) |
| 22 | Q/ctl/internal/notify/sink.go:1-50 | C | OK | `Sink` (20); `HTTPSink` (24). Wired at `Q/ctl/cmd/control/main.go:58` |

### 2. Affected files
- `Q/edge/telemetry.rs` [C, 64]: install the recorder here or in main; update the doc comment (1-16).
- `Q/edge/main.rs` [M, 1569]: second listener for /metrics near 1513-1517 (uncommitted diff does not touch it); shutdown drain at 84-111 and 1517; lease drain near 323 and 337.
- `Q/edge/search_routes.rs` [M, 2708]: stale comment at 226-233. No metric emission sites.
- `Q/edge/routes.rs` [M]: no change for an internal listener.
- `Q/crates/quarry-edge/Cargo.toml` [C] (57-58) and `Cargo.toml` [C] (114-115): deps exist.
- `Q/rt/lease_pool.rs` [C, 294]: `metrics_snapshot` (53) as the source of pool gauges; RESOURCE_BUSY refusal.
- `Q/rt/autoscale.rs` [C, 165]: add a memory or cgroup signal [NEW logic].
- `Q/br/chromiumoxide.rs` [C]: `ensure_browser` 194-270 (multi-Chromium pool).
- `Q/rt/step_receipts.rs` [C]: source for RunStatistics. No `RunStatistics` type exists anywhere (grep) [NEW].
- Migrations: `RunStatistics` in the Go run record would go to `Q/ctl/internal/store/pg/migrations/015_*.sql` [NEW; highest 014_fleets.sql, C]. A Rust-side table would go to `Q/crates/quarry-runtime/migrations/0010_*.sql` [NEW; highest 0009_browser_timeline_events.sql, C].
- `Q/ctl/internal/notify/sink.go` [C] and `Q/ctl/cmd/control/main.go` [C] (line 58): alert delivery through notification-core.
- `Q/orch/cmd/orchestrator/main.go` [C]: SIGTERM reference.
- `Q/pkg/quarryotel/quarryotel.go` [C]: span vocabulary (phase 3).
- `Q/deploy/compose/docker-compose.yml` [C]: quarry-edge service at 64; `QUARRY_EDGE__PORT: "8082"` at 70. Add an internal-only metrics port; do not publish it.
- `CP/observability/prometheus.yml` [C]: add a quarry job and rule files. No Alertmanager or rule file exists in any observability directory (grep).
- `IC/internal/api/server.go` [M]: pattern reference (internalAuth on /metrics).
- Docs: `Q/docs/PLAN.md` [C], `Q/docs/GAP.md` [C], `Q/docs/gap-quarry.md` [C]: fix in the same change. `.github/workflows/quarry-docs-contract.yml` [U] reads `Q/docs/**`.

### 3. Reusable but unwired
- `metrics` and `metrics-exporter-prometheus` (declared, no recorder).
- `RuntimeLeasePool::metrics_snapshot` (`lease_pool.rs:53`): tests only.
- `notify::HTTPSink`: wired (`Q/ctl/cmd/control/main.go:58`).
- `quarryotel` OTLP setup: wired in orchestrator and control; not in the Rust edge.
- `BrowserTelemetry` (`contracts.rs:100`): wired; input to a persisted RunStatistics.

### 4. Tests
- `Q/rt/lease_pool.rs:273-288` snapshot test [C] (test-only use of `metrics_snapshot`).
- `telemetry.rs`, `search_routes.rs`: no metrics tests found [UNVERIFIED: no test-module grep].
- `IC` metrics tests and `quarryotel` Go tests: not read.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime lease_pool`; `cargo test -p quarry-edge`; Go: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...` (not run in CI).
- CI: `quarry-v2-ci.yml` cargo-check (205) [C]; govulncheck matrix (245-263) [C]; `quarry-docs-contract.yml` [U].

### 5. Feature flags and env
- `OTEL_EXPORTER_OTLP_ENDPOINT` (`telemetry.rs:29`): unset means JSON fmt only.
- `QUARRY_EDGE__PORT` (compose 70; bind 1513): a metrics port env would be [NEW] (for example `QUARRY_EDGE__METRICS_PORT`).
- `NOTIFICATION_CORE_INTERNAL_KEY` (`Q/ctl/cmd/control/main.go:58`) [C].
- `APP_ENV` and `ENVIRONMENT` (`main.rs` 128-129, 157-158) [M]: drive prod behaviour.
- `SOURCE_REVISION` and `BUILD_DATE` (`main.rs` 339-340) [M]: usable as a build_info gauge.

### 6. Surprises
- SIGTERM drain already exists (`main.rs` 84-111, 1517). The real gap is draining browser leases and agent runs, as the JSON says.
- Three docs claim metrics or traceparent are done (PLAN.md:359, GAP.md:257, gap-quarry.md:175). The code says no.
- `search_routes.rs` has +530/-81 uncommitted. Its stale metrics comment sits in that same file.
- The single public listener means a /metrics listener is a second bind plus a compose change.
- Phase 2 (alerting) is new infrastructure: no Alertmanager and no rule files exist, and `AP/observability/prometheus.yml` says not to add targets that do not ship.
- `quarry-docs-contract.yml` is untracked [U], so the docs gate is not in git yet.

<!-- COMPLETE -->
