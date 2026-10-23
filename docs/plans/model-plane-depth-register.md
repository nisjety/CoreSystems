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


### Wave-mode batch W2-01 (2026-10-23)

| ID | Slice | Kind | P | Finding | Status |
|---|---|---|---|---|---|
| D2-010 | WS26-S3 | risk | P0 | InlineCitationMarker in chat-media-markdown.tsx (around line 1237, href={entry.citation.url}) renders citation URLs as anchors with no scheme allowlist, and the citation URL is taken without a scheme check in chat-normalizers.ts (around line 1642); a javascript: citation URL would still become a clickable link, so confirm upstream normalisation or guard it with isSafeLinkHref (it is outside this slice's site list). | open |
| D2-011 | WS26-S3 | risk | P0 | The citation chip in C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/features/chat/components/chat-media-markdown.tsx (around line 1238, href={entry.citation.url}) renders citation URLs as anchors with no isSafeLinkHref guard, and the citation URL is taken from search payloads without a scheme check (src/shared/api/chat-client.ts around lines 1003 and 1413), so a javascript: citation URL stays clickable; the row's site list omits this sink, so it is not a required fix here, but it should get the same one-line guard in this slice or an immediate follow-up. | open |
| D2-012 | WS26-S3 | live-proof | P1 | Live proof not run: start a second dev server (pnpm dev --port 5174 --strictPort in the worktree), send a chat message with [click](javascript:alert(1)) and a citation with a data: URL, confirm no a[href^="javascript"] or a[href^="data"] in the DOM, and capture a screenshot. | open |
| D2-013 | WS26-S3 | live-proof | P1 | Decide the citation-chip sink (chat-media-markdown.tsx around line 1238): add the same isSafeLinkHref guard now, or record it as a named WS26 follow-up before the chip is treated as covered. | open |
| D2-014 | WS26-S3 | live-proof | P1 | Add the .verevon-chat-link-blocked rule to src/styles/global.css (outside this slice's ownership) if a visible blocked style is wanted. | open |
| D2-015 | WS26-S3 | test-gap | P2 | No unit test asserts that WebSourceCard, the reasoning-popover citation, the evidence-item link or the ChatLiveRunPanel step URLs emit the inert span for an unsafe URL; only the parseInline markdown path has DOM negatives in chat-markdown.test.ts. | open |
| D2-016 | WS26-S3 | live-proof | P2 | The plan's DoD evidence (second dev server on port 5174, a message with [click](javascript:alert(1)) and a data: citation, DOM inspection plus screenshot) was not run; it is a lead or depth step. | open |
| D2-017 | WS26-S3 | follow-up | P2 | needed an edit outside the slice's files: apps/Frontend Plane/verevonv3/src/styles/global.css (optional: a rule for .verevon-chat-link-blocked; not edited, outside ownership) | open |
| D2-018 | WS26-S3 | follow-up | P3 | 4 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |
| D2-019 | WS18-S0 | decision | P1 | The ADR addendum says 'Order unchanged' and 'makes no ADR-0003 change', but ADR-0003's own body (apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md around line 134) lists platform, org, Space, agent with no personal-instructions layer, while the addendum and composer (run_context.rs:146-176) add the personal layer between Space and agent; the owner or ADR-0003 sign-off must reconcile this before the S1 behaviour is switched on. | open |
| D2-020 | WS18-S0 | live-proof | P1 | The ledger entry is absent from the worktree: grep of apps/Model Plane/docs/decisions/ledger.md finds no WS18 block. The lead must insert the full WS18-S0 block (the round-1 lead_todo text, which is not in this report) and must apply the round-2 sentence change. The round-2 lead_todo presumes a block that is not in the file, so it cannot be applied as a one-line replace alone. The replacement order sentence must match the addendum: platform, org, Space, the user's personal instructions when the per-thread toggle is on, then a mentioned agent. | open |
| D2-021 | WS18-S0 | live-proof | P1 | The row's DoD says the three docs are merged and linked from the S4 PR. Merge and link are lead and PR actions that this verifier cannot confirm. | open |
| D2-022 | WS18-S0 | live-proof | P1 | The ADR-0003 sign-off on the order (see the P1 finding) is still open and is an owner action. | open |
| D2-023 | WS18-S0 | follow-up | P2 | The composer's persona layer (persona.md §6.3) is not in the ADR-0003 text as the ADR itself describes it; the owner should decide whether ADR-0003 gets a persona line in its own docs pass. This round deliberately did not change ADR-0003. | open |
| D2-024 | WS18-S0 | decision | P2 | DEC-28 sign-off of the ADR-0003 addendum is still not recorded anywhere (carried over from round 1). | open |
| D2-025 | WS18-S0 | cleanup | P2 | The addendum cites 'persona.md §6.3', but apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/chat-wave2-specs-2026-09-28/persona.md has no §6.3 (only '## 6. Genuine owner decisions'); the same dangling cite is copied from the composer comment in run_context.rs and should be corrected in both. | open |
| D2-026 | WS18-S0 | decision | P2 | The addendum header says 'Status: decided', but DEC-28 sign-off of the ADR-0003 addendum is not recorded anywhere; it rests on the default-taken decision and should carry an explicit sign-off line later. | open |
| D2-027 | WS18-S0 | follow-up | P3 | 4 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |
| D2-028 | WS19-S0 | live-proof | P1 | The LibreOffice-under-bwrap spike has still not been run. Cold start, RSS, the /skills read-only result and the D5 decision are all unmeasured. | open |
| D2-029 | WS19-S0 | risk | P1 | The LibreOffice registry values are unverified. MacroSecurityLevel 3 (highest) and Link 0 (Never) for Writer, Calc and Impress come from memory of LibreOffice's officecfg schema, not from the installed package. A wrong value would make the macro/link DoD line false while the info line still prints. The live run must check the effect, not just the file. | open |
| D2-030 | WS19-S0 | live-proof | P1 | The LibreOffice-under-bwrap spike has not been run live, so the cold start (25 s), peak RSS (1 GiB), /skills read-only and netns-only-lo results and the D5 decision are all unmeasured; the lead must run bash "apps/Model Plane/scripts/verify-office-render.sh" with docker. | open |
| D2-031 | WS19-S0 | risk | P1 | The macro (MacroSecurityLevel 3) and link-update (Link 0) registry settings written at apps/Model Plane/scripts/verify-office-render.sh:119-132 are only printed as info; nothing checks that LibreOffice honours them, and the Impress Content/Update path is unverified against the installed package, so the one-page result must show the effect (for example a linked-image or macro document) and not just the file. | open |
| D2-032 | WS19-S0 | live-proof | P1 | Live run of bash "apps/Model Plane/scripts/verify-office-render.sh" (docker, lead only): the four good fixtures convert to a %PDF, cold start <= 25 s, peak RSS <= 1 GiB, the sandbox network has only lo, and /skills and the root are read-only. | open |
| D2-033 | WS19-S0 | live-proof | P1 | One-page spike result recorded in apps/Model Plane/MODEL_PLANE_STATUS.md (lead-owned): image MB delta on the real image, cold and warm seconds, peak RSS, /skills mount result, and macro and link-update settings with evidence of effect. | open |
| D2-034 | WS19-S0 | live-proof | P1 | D5 entry in apps/Model Plane/docs/decisions/ledger.md (lead-owned): LibreOffice in the sandbox image or sidecar, decided from the live result. | open |
| D2-035 | WS19-S0 | live-proof | P1 | Confirm the LibreOffice registry values (MacroSecurityLevel 3, Link 0 for Writer, Calc, Impress) against the installed package and record any no-op setting as a finding. | open |
| D2-036 | WS19-S0 | follow-up | P3 | 13 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |
| D2-037 | WS20-S0 | decision | P1 | Owner must explicitly accept DEC-16 (b) before WS20 S4 starts; the default was taken, not ruled. | open |
| D2-038 | WS20-S0 | risk | P1 | The lead_todo ledger block says 'Each WS20 slice adds its own switch, default off', but plan S8 sets CRON_CONTRACT_GUARDS_ENABLED to default true and S3 defaults VITE_SCHEDULE_RESULTS on once S1 is on; the lead should narrow the sentence to the authority switches S4-S6 before pasting it, and S8's default-true guard needs a decision before S8 is built. | open |
| D2-039 | WS20-S0 | live-proof | P1 | Lead must apply the shared-edit to C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/decisions/ledger.md: insert the ADR-0006 block before the heading '## 2026-10-23 - Pending items dock above the composer in one tray (WS14-S1)' at line 184. The file is fully CRLF (5023 of 5023 lines), so the inserted lines must be CRLF. The S0 row's 'ledger entry' deliverable is absent from the worktree until this happens. | open |
| D2-040 | WS20-S0 | live-proof | P1 | Owner decision D1 (DEC-16 (b), amend not reverse) is answered only by the taken default; the S0 DoD says 'D1 answered', so an explicit owner yes is required before S4 starts. | open |
| D2-041 | WS20-S0 | live-proof | P1 | The ZDR clause restatement required by the S0 row ('never scheduled or unattended' stays true) exists only in the lead_todo ledger block; grep of the matrix finds no ZDR text, so this closes only when the lead applies the block. | open |
| D2-042 | WS20-S0 | live-proof | P1 | Commit the two owned files and the ledger block together so the plan and matrix references to the 2026-10-23 ADR-0006 entry are not dangling. | open |
| D2-043 | WS20-S0 | live-proof | P1 | Optional, not required by the row: a dated Build ledger entry for WS20-S0 is absent from the plan's Build ledger at the end of the file (docs-only, so no evidence row applies). | open |
| D2-044 | WS20-S0 | live-proof | P1 | Commit (lead): docs(model-plane): record the WS20 stored-consent contract as ADR-0006 and add the handoff rows. Two owned files plus the ledger block above. | open |
| D2-045 | WS20-S0 | follow-up | P3 | 10 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |
| D2-046 | WS21-S0 | risk | P1 | The broker counts raw entries against the 32-entry cap, so 33 copies of one host is refused although it dedupes to one entry; the golden pins this as current behaviour, and the owner or broker should decide whether to dedupe before the cap before S4 request validation depends on it. | open |
| D2-047 | WS21-S0 | live-proof | P1 | The decisions-ledger entry, the capability-ownership-matrix row and the master-ownership-matrix row are not applied. The lead applies the shared-edit lines from the implementer's report. Insert the WS21-S0 ledger entry directly above the 2026-10-21 'Do runs inherit the Ask run context' heading and below the WS10-S0 2026-10-23 Model entitlements entry, because scripts/tests/ownership-docs-contract.test.mjs asserts the topmost ledger heading is that entitlements entry. | open |
| D2-048 | WS21-S0 | live-proof | P1 | The Quarry doc CROSS_PLANE_INTEGRATION.md was edited in the worktree, but the plan row tags the item-5 fix as a live-only edit after land. The three edits (item 5, the BrowserBroker port row 9090 to 9095, and the health-check port) must be re-applied to the live file C:/dev/CoresSystem, not copied over it wholesale. | open |
| D2-049 | WS21-S0 | live-proof | P1 | node --test scripts/tests/ownership-docs-contract.test.mjs reports 46 of 50 passing. The 4 failures are WS10-S0 shared-edit items the lead still has to apply (rule 19, the inference-core row, the capability-matrix model-entitlement row, the ledger top entry). None of them is caused by WS21-S0. | open |
| D2-050 | WS21-S0 | live-proof | P1 | The Quarry doc CROSS_PLANE_INTEGRATION.md is tagged live-only in the plan: after the live-only work lands, re-apply the three edits (item 5, the BrowserBroker port row, and the grpcurl health-check port) to the live checkout C:/dev/CoresSystem, and do not overwrite the live file wholesale. | open |
| D2-051 | WS21-S0 | live-proof | P1 | Commit suggestion for the lead (after the shared edits): 'docs(model-plane): record the layered browser policy and pin the browser-domain golden (WS21-S0)'. No rebuild, no deploy, no docker needed for this slice. | open |
| D2-052 | WS21-S0 | test-gap | P2 | No committed test reads browser-domain-canonical.json; the 41-vector check was a scratchpad copy of the broker normaliser, so WS21-S4 must vendor the golden into browser-broker testdata and add a test that runs it against the real NormalizeAllowedDomains. | open |
| D2-053 | WS21-S0 | risk | P2 | The four other canonicalizers (Quarry, execution-core, model-gateway, gateway BFF) are not checked against this golden; each needs its own golden test in S4, S5a or S10 before the browser policy is switched on. | open |
| D2-054 | WS21-S0 | follow-up | P3 | 8 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |
| D2-055 | WS23-S0 | risk | P1 | The live model-execution registry row was not inspected; S8 cannot be claimed until the lead verifies its scopes and ingestion-audience grants against the live registry. | open |
| D2-056 | WS23-S0 | live-proof | P1 | The S0 row's live check of the model-execution registry grants was not done; the repo config (apps/Control Plane/config/plane-service-principals.json, model-execution) lists only integration:read and shipping:read on the ingestion audience, so S8 stays blocked until the lead confirms the live registry. | open |
| D2-057 | WS23-S0 | risk | P1 | During this verification a grep over apps/Model Plane/deploy/.env echoed one line containing the MODEL_EXECUTION_SERVICE_API_KEY value into this session's tool output; the value was not used or repeated in this report, but the lead should treat it as exposed in the transcript and rotate it if it is live. | open |
| D2-058 | WS23-S0 | live-proof | P1 | Live check that the deployed model-execution registry row grants the scopes and ingestion audience the S0 row asks for (needs the live stack, not verified here). | open |
| D2-059 | WS23-S0 | live-proof | P1 | Lead must write the four ledger entries and the capability-ownership-matrix line into apps/Model Plane/docs/decisions/ledger.md and apps/Model Plane/docs/capability-ownership-matrix.md, since this batch was not allowed to edit them. | open |
| D2-060 | WS23-S0 | test-gap | P2 | current_gating_table_is_pinned covers the catalogue as it stands; a mutation check that a new catalogue read flips the pin was not run (wave mode). | open |
| D2-061 | WS23-S0 | test-gap | P2 | current_gating_table_is_pinned only checks a fixed list of names, so adding a new CATALOG entry (or a read added with a new name) is not caught, and the doc comment's claim that a catalogue edit is always a deliberate change overstates what the test enforces; iterate CATALOG and compare the full (operation, is_write) set against a literal table. | open |
| D2-062 | WS23-S0 | cleanup | P2 | The two new assertion blocks in current_gating_table_is_pinned (around lines 1080 and 1109 of integration_tools.rs) are not rustfmt-clean, and the plan's section 6.4 gate is cargo fmt --all -- --check; reformat those two spots (the file already had about 10 pre-existing rustfmt diffs at HEAD, so only the new lines need fixing). | open |
| D2-063 | WS23-S0 | follow-up | P3 | 4 more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/W2-01.json | open |

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
| WS19-S0 | authority | W2-01 | no |
| WS20-S0 | authority, zdr | W2-01 | no |
