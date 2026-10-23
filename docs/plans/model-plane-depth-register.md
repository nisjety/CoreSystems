# Model Plane build: depth register

**Building strategy (owner's order, 2026-10-23).** Build one wave at a time. A slice is built to the plan row's code and the unit tests the row names, then committed. Everything that is depth is written down here and is not chased while the wave is being built: live proofs, rebuilds, database harness runs, hostile reviews, edge cases, hardening, owner decisions, cleanup. When a wave is fully built the build stops. The depth findings are then prioritised with the owner, the wave is tested, and only then does the next wave start (or depth work is scheduled first).

Waves follow the plan (`docs/plans/model-plane-chat-build-plan-2026-10-14.md`, section 3): **Wave 1 = WS01 to WS12** ("make what exists trustworthy and provable"), **Wave 2 = WS13 to WS27** ("close the agent-loop and governance gaps"). Waves 3 to 5 are epics in plan section 3.3 and are not in the queue.

## How an item is logged

| Field | Meaning |
|---|---|
| ID | `D1-nnn` for Wave 1, `D2-nnn` for Wave 2 |
| Slice | the slice that raised it, or `cross` |
| Kind | `live-proof`, `test-gap`, `risk`, `follow-up`, `decision`, `cleanup` |
| P | **P0** a hole in authority, ZDR, money or data already visible in committed code; **P1** needed before the behaviour can be switched on or the wave called tested; **P2** quality or test gap; **P3** cleanup |
| Status | `open`, or `done <commit>` once closed in a depth phase |

A batch's `depth_findings` and the verifier's findings are appended here by the lead after the batch is committed.


## Standard depth passes at the end of a wave

1. Rebuild every service the wave changed (Model Plane images, BFF gateway, frontend on a second dev server) with `.plan-work/execution/tools/recreate-with-live-env.sh`; check boot logs.
2. Run the session-core DB harness, `scripts/verify-durable-layer.sh`, the capability-core integration tests and every `*-proof.sh` the wave added.
3. Hostile review (Opus) of the strict slices (authority, audit, ZDR, money, data-loss) built without one, then a fix round for what it finds (the table at the end lists them).
4. The live proofs below, then Playwright and manual checks for the SPA slices.
5. Triage the pre-existing failures (cross section) and decide which to fix.
6. Priorities with the owner; update the status of each item.

## Testing: light during the build, one test pass at the end of the wave (owner's order, 2026-10-23)

No big test run stops the build. The rule has three tiers:

| Tier | When | What | Blocks the build? |
|---|---|---|---|
| A | inside every slice | only the slice's own unit tests (the commands in its plan row), by the implementing agent | no |
| B | once, when the wave's last buildable slice is committed | the **wave test pass**: workspace compile (`cargo check --workspace --all-targets`, `go build ./...` per Go service, `tsc`), the unit suites (`cargo test --lib` for execution-core, model-gateway, inference-core, mp-contracts; `cargo test -p session-core --all-targets` without the ignored DB tests; `go test ./...` per Go service; `pnpm exec vitest run`, `pnpm lint`, the BFF `cargo test`), and the static proof runners (`durable-loop-proof.sh static`). Run in the background and time-boxed (stop a suite that runs past 20 minutes and log it) while the wave report is written. | no: a red test is logged here as a finding (P0 or P1 when it shows a broken slice, else P2) and the owner decides at the stop |
| C | after the owner has prioritised the depth | everything that needs docker: the session-core DB harness, `verify-durable-layer.sh`, capability-core integration tests, rebuilds and live proofs, Playwright, the hostile reviews | it is the depth work itself |

The pre-existing failures listed under Cross-wave (D0-003) are not a reason to stop: they are compared against by name, not fixed in the build.

## Wave 1 (WS01 to WS12)

Built so far (batches B01 to B07): WS01-S0, WS01-S1 (partial), WS01-S6; WS02-S1, WS02-S3, WS02-S4, WS02-S5, WS02-S6, WS02-S7; WS03-S1, WS03-S2, WS03-S3, WS03-S4, WS03-S5, WS03-S6, WS03-S7, WS03-S8 (partial), WS03-S9 (partial), WS3J-MP (partial); WS04-S1 (partial); WS05-S0, WS05-S1, WS05-S2 (partial), WS05-S3; WS09-S1 (partial), WS09-S2, WS09-S3; WS11-S1.code, WS11-S2.code, WS11-S3.code, WS11-S4.code, WS11-S5.code, WS11-S6; WS12-S1 (partial), WS12-S2, WS12-S3, WS12-S4, WS12-S5 (partial), WS12-S6. Items below come from those batches; each was a gap or a finding that was not closed when the slice was committed.

| ID | Slice | Kind | P | Finding | Status |
|---|---|---|---|---|---|
| D1-001 | WS01-S1 | live-proof | P1 | Audit alert delivery proof: rebuild the Control Plane Prometheus with the committed rules, trigger one synthetic audit-path failure, record the firing alert. Routing stays "visible, not paged". | open |
| D1-002 | WS09-S1 | live-proof | P2 | Three-run baseline on a second dev server still to record in `chat-perf-budget.json`. | open |
| D1-003 | WS12-S1 | live-proof | P1 | Recorded CI runs on the integration branch need a push (blocked on the owner). | open |
| D1-004 | WS04-S1 | live-proof | P1 | The five live NATS tests and the two-replica profile proof are not run (the `/readyz` run_control line is built in the wave). | open |
| D1-005 | WS05-S2 | live-proof | P1 | Fixture proof: connect `scripts/tests/fixtures/mcp-annotated-server.mjs`, see three rows, change a description and see `tool_definition_changed` after the 60 s catalogue TTL. Needs a minted gRPC token (see D1-008). | open |
| D1-006 | WS05-S2 | follow-up | P2 | `openapi.yaml` entries for `GET /api/v1/mcp/{id}/tools`, `POST .../approve` and `annotation_trusted` are missing. | open |
| D1-007 | WS05-S2, S4 | follow-up | P1 | ZDR goldens `mcp_tool_observation_under_zdr` and `mcp_tools_under_zdr_not_offered_not_executed` in `zdr-posture-matrix.json` (+ `.sha256`, vendored copies, pin in `zdr-contract-sync.test.sh`), then run that script. | open |
| D1-008 | WS05-S2 | risk | P2 | Only the gRPC `ListMcpTools` path posts observations; inline chat callers do not and execution-core's list call carries no bearer, so connect-time rows need a minted gRPC token. | open |
| D1-009 | WS05-S2 | risk | P2 | A first-sight tool written at the instant of a disable can stay enabled until the next observation; the approve response's `callable` comes from the pre-write read. | open |
| D1-010 | WS05-S2 | cleanup | P3 | Move `pub mod capability_observation` from `runtime_registries.rs` into `lib.rs`; `derive_org_scoped_service_token` is duplicated because the original is private. | open |
| D1-011 | WS05-S2 | decision | P1 | DEC-12 D2 (`annotation_trust` is an admin's per-server choice) and D5 (the HMAC observation-write route: a compromised gateway holding the root secret could mint a token for any org except `global`) need the owner and a security nod before the switch goes beyond the proof stack. | open |
| D1-012 | WS05-S3 | live-proof | P1 | Rebuild with `TOOL_CLASS_GATE=shadow`, soak, read the divergence counters; then enforce on the proof stack. Rollout order is execution-core first, then capability-core. | open |
| D1-013 | WS05-S3 | cleanup | P2 | The plan's WS05 section and `.plan-work/designs/WS05-declared-tool-classes.md` still say "roll capability-core first": correct them (the ledger entry has the right order). | open |
| D1-014 | WS05-S3 | test-gap | P3 | `execution_core_tool_binding_run_evictions_total` is logged and counted but not asserted. | open |
| D1-015 | WS05-S4 | live-proof | P1 | `MCP_DO_TOOLS=read` with `TOOL_CLASS_GATE=enforce` on the proof stack: a Do chat calls `echo_read`; `create_item` is absent; plan mode allows `echo_read` and refuses `create_item`; a ZDR run sees no MCP tool. Control's principal registry must grant execution-core a model-gateway token with `gateway:read` and `tools:execute`. | open |
| D1-016 | WS05-S4 | follow-up | P2 | `resume.rs` (`ApprovedRelease` pre-reserve) still classifies the effect by name; pass the run's gate. | open |
| D1-017 | WS05-S4 | risk | P2 | The forwarded capability bearer in `list_mcp_tools` is accepted only from a service identity because verifying it needs a `grpc_auth.rs` change (`verify_delegated_bearer`, capability-core audience): do that to drop the service-only rule. | open |
| D1-018 | WS04-S4 | live-proof | P1 | Migration 0050 has never run against Postgres. Run the session-core DB harness (three new ignored tests; harness count +3), `scripts/verify-durable-layer.sh`, then deploy session-core before the gateway flag. | open |
| D1-019 | WS04-S4 | live-proof | P1 | Two-replica proof (approve on :8080, Do run on :8081 with `MODEL_GATEWAY_DURABLE_PLAN_GRANT=true`). Needs the WS04-S1 two-replica profile. | open |
| D1-020 | WS04-S4 | follow-up | P2 | The raw gRPC `ExitPlanMode` keeps its best-effort `SetRunMode` with no retry; the matrix row `e_zdr_token_plan_grant_refused` was not added (the ZDR clause is covered by unit tests). | open |
| D1-021 | WS03-S9 | live-proof | P1 | `durable-loop-live-proof.sh` is built and has never run: container names, decide body and error codes will need adjusting on the first run; then write the evidence bundle into `MODEL_PLANE_STATUS.md`. | open |
| D1-022 | WS09-S3 | live-proof | P1 | Playwright before and after (transcript-key writes at most 1 per second), a kill-tab-mid-stream check, the Steps panel's Compose-response `createdAt` now stays at the first upsert, and "one write per terminal" is not pinned by a test. | open |
| D1-023 | WS12-S5 | test-gap | P2 | Remove each leak pin when its translation lands (`RECEIPT_HEADER_LEAKS`, `RECEIPT_EFFECT_LEAKS`, `SPARSE_RECEIPT_LEAKS`, four artifact pins); run the mutation proof on `PlanApprovalControl`'s grant label; run the full `pnpm test`. | open |
| D1-024 | WS11-S5 | live-proof | P1 | One-route live proof (quick then deep on a route the dev stack has: `effort_resolved` Low then High, distinct cache keys, `/v1/models` carries `effort_levels`). Rollout: inference-core adapter flags first, then `MODEL_GATEWAY_EFFORT_LADDER`. | open |

### Round-2 leftovers and integration notes (2026-10-23)

| ID | Slice | Kind | P | Finding | Status |
|---|---|---|---|---|---|
| D1-025 | WS04-S4, WS14-S2 | risk | **P0** | With `MODEL_GATEWAY_DURABLE_PLAN_GRANT` OFF (the default) the gRPC `ExitPlanMode` does not compare the caller's `session_id` with the run's own thread: the owner of any run can plant an in-memory grant (up to `danger_full_access`) on another thread of the org on that replica, and `RUN_PLAN_MODE_EXITED` names it. The path exists since WS14-S2a. Fix: call `read_run_detail` regardless of the flag, refuse a `session_id` that differs from `detail.thread_id`, pass `detail.thread_id` into the handler (the flag-on path already does). Found by the round-2 re-review; not fixed under the wave strategy. | open |
| D1-026 | WS04-S4 | risk | P1 | `with_local_fallback` consults memory only when Session Core holds no grant, so a newer, narrower in-memory grant (a waived approval, or a transient `SetRunMode` failure) loses to an older, broader durable grant for up to 24 h. Fix: stamp the memory entry and compare with a `granted_at` from `GetThreadGrant`, or take the narrower rung. | open |
| D1-027 | WS04-S4 | risk | P2 | `RUN_PLAN_MODE_EXITED` (rung, justification, plan id) is published before the `SetRunMode` result is known, so a refused write leaves an audit event naming a grant that exists nowhere; `ExitPlanModeResponse` has no `persisted` field. | open |
| D1-028 | WS04-S4 | risk | P2 | `set_run_mode_retryable` retries `Code::Unknown` but `set_run_mode_failure_is_transient` excludes it, so an approval that failed with `Unknown` is retried four times and then kept nowhere (stranded, fails closed). Use one predicate. | open |
| D1-029 | WS05-S4 | test-gap | P2 | The second-round fixes (built-in classes learned by ZDR and waived runs, merge-style hydrate, fail-closed on transport errors and 5xx) were not re-reviewed: the re-review was stopped to save time. | open |
| D1-030 | cross | live-proof | P1 | The Model Plane paths of `quarryv2-next` were imported (commit `5ca40750`); the Rust compile check of model-gateway and the browser-broker Go tests, the Postgres store tests and the end-to-end harness have not been run. A full merge of `quarryv2-next` (Quarry, Ingestion, Frontend parts included) is still ahead and conflicts today in about 8 non-Model-Plane files. | open |
| D1-031 | WS05-S4 | follow-up | P2 | `MCP_DO_TOOLS=all` stays blocked; the two-week shadow soak and the deletion of the name heuristics (WS05-S6) come later. | open |

## Wave 2 (WS13 to WS27)

Built so far: WS13-S0, WS13-S1, WS13-S2; WS14-S1, WS14-S2 (partial), WS14-S3; WS27-S0, WS27-S1. Wave 2 is not started as a wave: these came in with Wave 1 batches because they were Wave 1 prerequisites.

| ID | Slice | Kind | P | Finding | Status |
|---|---|---|---|---|---|
| D2-001 | WS13-S1 | live-proof | P1 | A live pause that stores `owner_effect_reserved` and `provider_write` in `approvals.metadata.cause`; `mp_session_approval_cause_total{code}`; a DB-gated round trip of `metadata.cause`; vendor `approval-causes.json` into execution-core fixtures and the sync script's GOLDENS (with S8). | open |
| D2-002 | WS13-S2 | live-proof | P1 | The approval card in nb and en, `GET approvals` showing the cause, `POST decide` with scope thread returning 422 `scope_not_enabled`, on a rebuilt BFF, frontend and model-gateway. | open |
| D2-003 | WS13-S2 | follow-up | P2 | The openapi schema and both SDKs were hand-edited; regenerate with `api/generate-sdk.sh` and diff (the Python SDK tests could not run: pydantic and pytest are not installed). `chat-client` no longer carries the raw approval kind on `approval_state_changed`. | open |
| D2-004 | WS02-S3 | live-proof | P1 | A live `AGENT_USAGE` ledger row on a rebuilt stack behind `MODEL_GATEWAY_AGENT_USAGE_ENVELOPE`; Control must confirm attribution (DEC-18 D5) before the flag turns on; a waived agentic turn cannot be driven end to end in the harness. | open |
| D2-005 | WS02-S3 | risk | P2 | The usage of a run that errors after spending rounds is not ledgered (decided gap); the per-round log is the reconciliation source. | open |
| D2-006 | WS27-S0 | decision | P1 | ADR items D1, D2, D4, D5, D6 are proposed under DEC-18 until the owner approves; the base commit named in the ledger entry (68424704) was never confirmed. | open |
| D2-007 | WS14-S1 | live-proof | P1 | Screenshots at 1280 and 375 px with an approval, a plan and a queued message pending, a keyboard-only pass, and the reload restore. | open |
| D2-008 | WS11-S6 | live-proof | P1 | The composer select on a model with and without `xhigh` and an invoke body with `effort: xhigh` (needs the ladder on and inference-core advertising xhigh). | open |
| D2-009 | WS27-S1 | risk | P3 | An unlisted model's cost is now `None` in the stream's `usage` frame; nothing consumes the meter yet (WS27-S2 and S3 do). | open |

## Cross-wave

| ID | Kind | P | Finding | Status |
|---|---|---|---|---|
| D0-001 | risk | P1 | A dev-stack NATS password was printed into a session transcript on 2026-10-22 (a grep matched `deploy/.env`). Rotate it. Env files are never grepped. | open |
| D0-002 | live-proof | P1 | The BFF gateway and the :5173 frontend were not rebuilt (they serve the main checkout); every SPA slice's live check waits for a second dev server. | open |
| D0-004 | cleanup | P3 | The main checkout (`C:/dev/CoresSystem`, branch `main`, 146 commits behind) has 78 uncommitted Model Plane files, all already on `model-plane` (checked file by file: 42 identical, the other 13 equal to the versions committed by WS01-S0). Nothing is unlanded there; resetting it is the owner's call and never done by an agent. The `vigorous-perlman` worktree holds an uncommitted `go.work.sum`. | open |
| D0-003 | cleanup | P2 | Pre-existing, not from the build: `pnpm typecheck` has 33 errors (`quarry-client.ts` cannot find `@quarry/client`; `RemoteSupportPage.tsx`), `zdr-contract-sync.test.sh` has 22 failures from other planes' goldens and CRLF `.sha256` copies, `cargo fmt --all --check` fails on about 120 files, and some model-gateway non-lib test targets fail on Windows. | open |

## Strict slices built without a hostile review

Slices that touch authority, audit, ZDR, money or data-loss and were built in wave mode (no per-slice hostile review, no fix round). Nothing is listed yet: B01 to B07 each had a review before this strategy began. Each wave-mode batch appends its strict slices here.

| Slice | Risk classes | Batch | Reviewed |
|---|---|---|---|
