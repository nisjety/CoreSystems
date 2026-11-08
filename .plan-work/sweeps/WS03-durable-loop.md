# WS03-durable-loop: code-map sweep (audit item F-t3-002)

Read-only. No product file, config or doc was edited. No build or test was run; verdicts are static.

- Worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`: HEAD afb83928, clean. Untagged line numbers are worktree lines.
- Live `C:/dev/CoresSystem`: same HEAD plus uncommitted work. Status codes come from `.plan-work/live-tree-status.txt` (M modified, ?? untracked, D deleted). Lines tagged "live" were read in the live tree. Stats come from `git --no-optional-locks diff --stat` in the live root.
- Audit (read-only input): `scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md`. Card :353-367; 9.1 row :1535; 9.3 :1683-1692; 9.4 :1696; 10.1 :1716; 10.2 :1726-1728; Appendix B :1820; Appendix C :1929-1936.

## 0. Card digest and what the code does today

- Card: Priority, Partial, effort L, Model Plane (execution-core loop, session-core run state), W1, confidence High. Appendix C: partial; no reference shows crash-durable approval resume ([INF] synthesis).
- Stages: stage 1 = continue as a fresh run seeded by F-t3-001 context plus completed step summaries (plan B7, OD6). Stage 2 = true in-place resume from a scrubbed fold checkpoint. Typed closers, effect ledger, provider-exhaustion parking and ask_user are separable. ask_user is WS14; the pause state machine is shared.
- Dependencies: F-t3-016 (9.1 :1540, remaining M slice is W1), OD8 renewable delegated bearer, Control reauthorizer (9.1 :1535). Dependents (9.4 :1696): F-t3-011, 018, 026, 036, 037, 047, 064, 083.
- Verified today:
  - The Do loop keeps its messages in a local Vec per run (agent.rs:1168, 1184-1197). It writes no checkpoint. The only step-checkpoint writer is the ExecuteStep path (grpc.rs:1025-1038). Auto-compaction writes its own checkpoint namespace (compaction.rs:125).
  - Pause: pause_for_approval (agent.rs:4469) builds a plaintext descriptor (agent.rs:4551) and calls session-core create_approval (orchestration_grpc.rs:2461). session-core encrypts the descriptor (orchestration_store.rs:711). The run is set to AwaitingApproval.
  - Resume: the approval path always returns unavailable. grpc.rs:1101-1107 validates the grant, then calls resume_durable_approval_continuation (worktree 691-698; live 706-713), which returns Err. The user-pause path works via state.resume_paused (grpc.rs:1083). ResumeRunRequest.checkpoint_id (execution.proto:343-344) is never read.
  - The continuation worker resumes only execute_provider_action, book_shipment and tickets.create. Other tools fail closed (approval_delivery_worker.rs:9-12, 16-18, 31-33).
  - Step checkpoints are write-only: there is no restore reader. Only a row count is read (run_service_grpc.rs:378-381). Compaction matches checkpoints.state only against its own expected values (compaction.rs:114-116). events.proto:138 CHECKPOINT_RESTORED=111 has no Rust producer or consumer.
  - RunStatus has no parked state: Running, Paused, AwaitingApproval, Completed, Failed, Cancelled (state.rs:16-26).

## 1. Citation verification

Key: MATCH = same symbol and behaviour at the cited lines. DRIFT = present at other lines. renamed = same behaviour under another symbol.

| # | Cited (card) | Claim | Worktree | Live | Corrected path:lines (worktree / live) |
|---|---|---|---|---|---|
| 1 | execution-core/src/runtime_loop/agent.rs:4476-4560 | pause_for_approval persists approval and descriptor, then returns | DRIFT (symbol MATCH) | M | wt fn 4469-4545, descriptor fn 4551, ZDR refusal 4558-4562 / live fn 4476-4552, descriptor 4558, typed refusal 4565-4570 |
| 2 | agent.rs:1185-1200 | history is a local Vec per run_rounds call | MATCH (approx) | M | wt run_rounds 1168, vec 1184-1197 / live run_rounds 1172, vec 1188-1201 |
| 3 | approval_delivery_worker.rs:1-60 | only 3 action kinds re-executed; others fail closed | MATCH | clean | 1-62 (kinds 16-18; fail-closed 29-33; org discovery 41-47) |
| 4 | execution-core/src/grpc.rs:706-713 | resume_durable_approval_continuation always unavailable | DRIFT (live MATCH) | M | wt fn 691-698 (doc 686-690) / live 706-713 exact |
| 5 | model-gateway/src/approvals.rs:255-275 | gateway never turns a grant into ResumeRun | renamed (quarantine_granted_approval_continuation) | clean | 254-277 (doc 254-263; fn 264-277) |
| 6 | execution-core/src/state.rs:1-40 | run state is in-memory | MATCH | clean | 1-40 (RunStatus 15-26; resume_paused 147) |
| 7 | agent.rs:4376-4398 | durable step record content-redacted under ZDR | MATCH | M | wt 4375-4401 (ZDR branch 4379-4393) / live 4382-4408 |
| 8 | orchestrator-core/cmd/workflows/interactive_run.go:52-55 | approval signal; host drives ExecuteStep | MATCH | clean | 51-55 (SignalApproval 53); approval wait 130-149; per-turn ExecuteStepActivity step_loop.go:64-82 |
| 9 | "session-core checkpoints table" (no path) | durable checkpoint rows | resolved | clean | migrations/0001_init.sql:46-53; ordinal trigger 0002_events_and_ordinals.sql:74-97 |
| 10 | "SaveCheckpoint RPC with 4 MiB cap" (no path) | RPC and size cap | MATCH | proto clean; session-core/src/grpc.rs M, bodies unchanged | sessions.proto:62, 689-707; session-core grpc.rs:78 (cap), 103-120 (validator), 4701-4777 (handler) / live 80, 105-122, 4872 |
| 11 | "continuation_crypto.rs" | row-bound AEAD for descriptors | MATCH | clean | session-core/src/continuation_crypto.rs:14 (KEY_ENV), 56 (AAD: approval, org, user), 60 encrypt, 80 decrypt |
| 12 | "approval outbox and receipt protocol" | lease, receipts, outcome | MATCH | clean | session-core/src/approval_delivery.rs:501 claim, 551 ack, 650/673 descriptor load and decrypt, 686 started, 764 outcome; migrations 0014, 0018, 0019, 0020 |
| 13 | "terminalization recovery loop" | recovery loop | MATCH (named run_recovery_worker) | M | session-core/src/terminalization.rs:1741 recover_due_terminalizations, 1759 run_recovery_worker (live 1852); spawned session-core main.rs:193 |
| 14 | "stream_buffer replay" | resumable SSE delta replay | MATCH | clean | model-gateway/src/stream_buffer.rs:1-12; route http_routes.rs:138 (/v1/invoke/resume/:request_id) |
| 15 | "approval rehydration" | warm cache rebuilt from durable store on boot | renamed (rehydrate_pending_for_org) | clean | model-gateway/src/approvals.rs:12-13 (doc), 1048; read-through 919, 941-955 |
| 16 | "resume must re-authorise through Control (OD8)" | fresh delegated bearer at resume | open decision; no code path | Control auth-core clean | CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:452-455; convex-token.service.ts:331-349, 398-424 (sec 3.2) |
| 17 | "Control reauthorizer" (no path) | Control authority at resume | not implemented for resume | clean | analogues only: capability-core/cmd/main.go:338-345, 366-372 (watch sweeper refuses to start without one); session-core/src/grpc.rs:4538 (fresh Control decision per reauthorization, scheduled runs) |
| 18 | "plan B7 / OD6" | plan references | not code | n/a | CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:178 (B7, L); :442 (OD6) |
| 19 | interactive_run.go per-turn durability | per-turn checkpoint in Temporal history | MATCH | clean | step_loop.go:72 ExecuteActivity("ExecuteStepActivity"); activities.go:612-618 (loop not resume-safe), 654 |

## 2. Claims that do not match the code

- Stale line references: docs/core-research/agentic-engineering-audit-2026-07-17.md:146-152 cites session-core/src/grpc.rs:1644-1697 for SaveCheckpoint. It is now 4701-4777.
- Stale doc: docs/core-research/execution-core.md:48-52 says ResumeRunRequest has no approval ID. The proto has approval_id (execution.proto:349-352).
- ResumeRunRequest.checkpoint_id (execution.proto:343-344) is in the contract but unused. execution-core resume_run (grpc.rs:1068-1132) ignores it, and the gateway sends it empty (http_routes.rs:1723).
- Typed closers for dangling tool calls: no closer symbol exists. grep for dangling, closer, orphan and synthetic finds only prose. Nearest typed analogues: unknown_outcome dispositions (approval_delivery_worker.rs:568-632 resolve_unreceipted_write; ticket_tools.rs; session-core approval_delivery.rs:764 record_continuation_outcome).
- Effect ledger: partial primitives exist. Reserve and finalize tool action RPCs (sessions.proto:55, 59, 652-688) are backed by session_tool_audit_intents (migration 0013; status reserved, completed or failed). Implementations: session-core grpc.rs:3650 (reserve_tool_action_inner), 3732 (finalize_tool_action_inner), RPC impls 4667 and 4684. Production callers: model-gateway tool_loop.rs:3988-3999 and 4075-4087. Continuation receipts (0018) are append-only.
- Parking on provider exhaustion: no parked run state exists (state.rs:16-26). Not traced further.
- The ExecuteStep path already writes step output content into checkpoints for non-ZDR runs (grpc.rs:1007-1031). ZDR is refused upstream by enforce_persistence_free_execution (grpc.rs:881; defined 650-660).
- ZDR guards present: resume_run calls enforce_persistence_free_execution(caller, false, "run control") (grpc.rs:1077). The gateway resume route calls require_non_zdr_durable_mutation (http_routes.rs:1713). The continuation descriptor is refused for ZDR (agent.rs:4558-4562 worktree; live typed refusal 4565-4570). The SaveCheckpoint handler itself has no zdr check (grpc.rs:4701-4777); it relies on its callers.

## 3. Host question and OD8 prerequisite

### 3.1 Host: execution-core versus orchestrator-core Temporal

| Option | Evidence for | Evidence against |
|---|---|---|
| execution-core (RunAgent loop) | Fold, pause and descriptor already live here (agent.rs:1168-1197, 4469, 4551). Chat Do reaches it via model-gateway spawn_run_dispatch (sse.rs:8618) into run_agent (sse.rs:8705; execution.proto:34). Rust owns the request hot path (Model Plane CLAUDE.md). Capability matrix gives execution-core the step loop (capability-ownership-matrix.md:39). | Run state is in memory (state.rs:1-40). No durable fold exists. |
| orchestrator-core Temporal | One durable ExecuteStepActivity per turn (step_loop.go:64-82; activities.go:654). Approval signal wait (interactive_run.go:53, 130-149). Test: interactive_run_test.go:353. Matrices name orchestrator-core for recovery (capability-ownership-matrix.md:39, 184; master-ownership-matrix.md:129). | Drives ExecuteStep (one step), never the RunAgent fold. No RunAgent client in orchestrator-core (only comments at servicecred/minters.go:49). Service credentials cover only the scheduled-step lane (servicecred_test.go:347-350), so a Temporal host must carry a fresh user bearer per activity without persisting it (precedent: interactive_run.go:31-34). ExecuteStepLoopActivity is documented as not resume-safe (activities.go:612-618). |

Proposed resolution (owner ruling needed). docs/decisions/ledger.md has no decision on the chat-loop host. Its Temporal lines are :2943 (Temporal "is not the production path" for model-authored orchestration scripts) and orchestrator-core auth entries at :3119-3129 and :4247.
- Loop, pause, resume entry and continuation stay in execution-core. The durable fold lives in session-core checkpoints, written per round and before each effectful call, and refused for ZDR (ZDR guard pattern: grpc.rs:881, 1077; agent.rs:4558-4562).
- Recovery driver: the session-core worker pattern (terminalization.rs:1759). Only if an owner rules so, an orchestrator-core workflow that re-enters RunAgent with a freshly minted delegated bearer.
- Temporal stays the host for ExecuteStep-driven runs (step_loop.go; wide_research.go:74-108). The chat Do path does not touch Temporal today.
- The 2026-07-17 agentic audit points the same way: execution-core compute resume with a checkpoint consumer, and ResumeRun extended with {last_checkpoint_id, next_round} (agentic-engineering-audit-2026-07-17.md:178-185).
- Confidence: medium. Needs an ADR; the host choice is not on the 9.3 ADR list.

### 3.2 OD8: renewable delegated bearer

- Open decision (CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:452-455): allow a longer-lived delegated bearer class (up to 1500 s, hard cap 1800 s) beyond the 300 s default. Recommended only for wide-research turns after a security review. Blocks C9, B7, B11 and E run length. Plan :365: B7's manual continuation or OD8 decides Terra viability.
- Current lifetimes (apps/Control Plane/auth-core/src/auth/convex-token.service.ts): plane audiences default to 300 s. session-core is at :398-404 (PLANE_TOKEN_TTL_SESSION_CORE_SECONDS). execution-core is at :415-424 (PLANE_TOKEN_TTL_EXECUTION_CORE_SECONDS). The model-gateway audience defaults to 900 s (:346-350, MODEL_PLANE_AUTH_TOKEN_TTL_SECONDS). Exchanged tokens are about 90 s (:250-252 doc comment).
- Execution-core holds one session-core bearer per run and exchanges it per tool call (convex-token.service.ts:854-858). A resume request carries its own short bearer (resume_run takes it at grpc.rs:1072; the gateway resume route passes the verified model bearer at http_routes.rs:1710). The resumed loop therefore has at most about 300 s of authority, and nothing re-mints it mid-run. Plan :365 already puts a typical A run at about 270 s. No bearer is persisted with the paused run (interactive_run.go:31-34 states the same rule for the Temporal lane).
- Service-principal precedent that the card forbids for user-bound resume: approval_delivery_worker.rs:186-205 (AUTH_CORE_URL, EXECUTION_CORE_SERVICE_ID, EXECUTION_CORE_SERVICE_API_KEY) and :277-300 (POST /api/session-core/internal-token, scope approval:deliver, org-scoped). plane-service-principals.json lists approval:deliver at :153, inside the execution-core block that starts at :134; a second occurrence at :181 was not attributed.
- Fresh-authority precedents that exist (scheduled lane; no bearer persisted): interactive_run.go:31-34 (no Control bearer in Temporal history); session-core grpc.rs:4538 (Control mints a fresh decision per reauthorization); CONTROL_SCHEDULED_RUN_* constants at grpc.rs:86-91.
- Status: UNMET. No renewal or re-mint path for a paused user run exists in Model Plane. The Control reauthorizer is not implemented for resume; the capability-core watch sweeper refuses to start without one (cmd/main.go:366-372). Not determined: the code that binds the 300 s bearer to a gateway run (only env defaults were traced).

## 4. Affected-file map

Depth 2 from the cited symbols. Lines are worktree unless tagged live. Tests are named where cheap.

| Path | Role | Lines | Live | Tests (path: names) |
|---|---|---|---|---|
| execution-core/src/runtime_loop/agent.rs | RunAgent loop: run_agent 567, run_agent_with_tools 743, run_agent_with_route 813, run_rounds 1168 (fold 1184-1197), pause call 2089, approval kind and reason 4446/4459, pause_for_approval 4469, ZDR redaction 4375-4401, continuation_descriptor 4551, run_agent RPC entry 567 | M (+207) | agent.rs tests: continuation_descriptor_binds_the_exact_action_and_refuses_zdr 5017; owner_action_descriptor_cannot_preserve_auto_permission 5066; gated_tool_creates_approval_and_returns_early 9088; gated_tool_rejects_an_undurable_approval_pause 9167; a_replayed_delegation_resumes_from_its_recorded_answer_without_re_running 11184; a_tool_call_after_the_first_pause_is_still_recorded_not_silently_dropped 12359 |
| execution-core/src/runtime_loop/mod.rs | Step pipeline: execute_step 278, execute_step_with_browser_grant 336, execute_step_with_subagent 400, execute_step_inner 455 (the "deployed agent" loop per CLAUDE.md) | M (+18) | not enumerated |
| execution-core/src/grpc.rs | ExecuteStep handler 828-1066 (ZDR refusal 881; SaveCheckpoint call 1025-1038); resume_run 1068-1132; run_agent RPC 1203; create_durable_approval 633; enforce_persistence_free_execution 650-660; validate_granted_approval 664; resume_durable_approval_continuation 691-698 (live 706-713); reject_direct_owner_action 706-713 | M (+98) | approval_resume_requires_a_granted_durable_record_bound_to_run_and_tenant 1616; durable_grant_stays_gated_until_a_real_continuation_delivery_exists 1659; direct_hitl_rejects_an_undurable_approval_pause 1833 |
| execution-core/src/approval_delivery_worker.rs | Continuation dispatcher: env and token provider 176-205; mint 277-300; parse 454-463; unreceipted write 568; process_delivery 924; spawn 1280; run_forever 1299 | clean | unit tests 1466-1690+: parses_a_valid_provider_action_descriptor 1466; missing_descriptor_is_terminal_with_continuation_unavailable 1586; a_postcondition_completed_write_never_claims_success_on_its_own 1643 |
| execution-core/src/state.rs | In-memory run state: RunStatus 15-26; resume_paused 147; resume_approved 154 | clean | in-file |
| execution-core/src/permission/mod.rs | requires_durable_owner_approval 77 (never-auto seed used by pause and ExecuteStep refusal) | clean | in-file |
| execution-core/src/runtime_loop/session_core_rules_double.rs | Test double for session-core rules (waiver helpers added) | M (+537) | in-file |
| execution-core/src/scrub.rs | Secret scrub at checkpoint and step boundary (called before SaveCheckpoint, per scrub.rs:11) | clean | scrub_checkpoint_test.rs:68 serialized_checkpoint_bytes_contain_no_secrets |
| execution-core/src/main.rs | Spawns approval delivery worker (line 32) | clean | — |
| execution-core/tests/runtime_loop_test.rs | Loop tests | clean | :52 ask_mode_requires_approval_for_tools |
| session-core/src/grpc.rs | SaveCheckpoint handler 4701-4777; validate_user_checkpoint 103-120 (cap 78); reserve and finalize tool action 3650/3732 and RPC impls 4667/4684; Control scheduled-run constants 79-91; checkpoint delete on thread delete 484 | M (+861) | user_checkpoints_cannot_preclaim_compaction_or_exhaust_storage 8781 (live 9028); DB-gated #[ignore] tests 2711-2884 (need DATABASE_URL) |
| session-core/src/orchestration_grpc.rs | Approval RPCs: list_pending 2406, get_approval 2430, create_approval 2461 (RunPausedForApproval broadcast 2578), decide_approval 2596, claim_approval_deliveries 2679, get_approval_continuation 2723, record_approval_continuation_started 2763, record_approval_continuation_outcome 2807, acknowledge_approval_delivery 2931 | M (+249) | approval_identity_is_pinned_and_cross_tenant_requests_are_rejected 3248; continuation_descriptor_requires_exact_scope_and_rejects_credentials 3276; approval_delivery_is_service_only_tenant_scoped_and_zdr_fail_closed 3715; approval_decision_replay_does_not_emit_duplicate_events 3778 |
| session-core/src/orchestration_store.rs | decide_approval 755; get_approval 1066; list_pending_approvals 1262; descriptor encrypt on create 711 | M (+8) | approval_decision_query_is_tenant_scoped_and_compare_and_set 1511; approval_delivery_insert_rejects_expired_grants 1522; approval_delivery_outbox_is_one_per_approval_and_content_free 1541; approval_delivery_outbox_has_a_leased_claim_contract 1564 |
| session-core/src/approval_delivery.rs | Lease claim 501; reap 533; ack 551; descriptor load and decrypt 650/673; record_continuation_started 686; record_continuation_outcome 764 | clean | 835-1111 (13 tests): continuation_receipts_are_append_only_and_distinct_from_delivery_leases 882; continuation_outcome_requires_live_lease_and_authoritative_completion_receipt 924; expired_worker_lease_reclaims_without_duplicate_start_receipt 1111 |
| session-core/src/continuation_crypto.rs | AEAD descriptor cipher: KEY_ENV 14, AAD 56, encrypt 60, decrypt 80 | clean | encrypts_and_decrypts_with_row_bound_aad 114; rejects_missing_or_wrong_sized_keys 133 |
| session-core/src/terminalization.rs | Managed-run terminal outcomes and recovery worker: claim_due_recovery 1601; recover_claimed_terminalization 1682; recover_due_terminalizations 1741; run_recovery_worker 1759 (live 1852) | M (+123) | in-file |
| session-core/src/compaction.rs | Auto-checkpoints (reserved id prefix); MAX_CHECKPOINTS_PER_CYCLE 26; INSERT 125 | clean | compaction_tests.rs:95 compaction_only_checkpoints_durable_runs; :528 zdr_and_poison_runs_do_not_create_checkpoints (#[ignore] DB) |
| session-core/src/gdpr.rs | Erasure deletes checkpoints (236) | clean | test at 674 |
| session-core/src/run_service_grpc.rs | RunDetail.checkpoint_index = COUNT of checkpoints (378-381) | M (+21) | — |
| session-core/src/main.rs | Spawns run_recovery_worker (193) | M (+1) | — |
| model-gateway/src/approvals.rs | Ephemeral approval cache (ApprovalStore 288); grant quarantine 264-277; read-through 919, 941-955; rehydrate_pending_for_org 1048 | clean | granted_approval_continuation_is_quarantined_without_a_descriptor 1485; to_create_request_maps_kind_and_aligns_id 1441; to_decide_request_maps_status_to_state 1460 |
| model-gateway/src/http_routes.rs | Resume route 317 into resume_run 1706-1733 (ZDR refusal 1713; ResumeRun with empty approval and checkpoint ids 1720-1726); /v1/invoke/resume 138; quarantine call sites (live 1560, 1572); effective_invoke_persistence_plan changed (live) | M (+237) | tests/orchestration_http_test.rs:975 orchestration_run_events_route_relays_sse; :1015 decide_approval_grant_succeeds_without_a_continuation_dispatcher |
| model-gateway/src/grpc.rs | gRPC facade; quarantine call (live 1550) | M (+47) | test double 3565-3587 |
| model-gateway/src/sse.rs | spawn_run_dispatch 8618; run_agent call 8705; RunAgentRequest builder 8751-8800; agentic_run_stream 9167; awaiting_approval frame 9536; run_paused_for_approval projection 9857, 9941, 10133 | M (+883) | in-file (RunAgentRequest builders 644-800) |
| model-gateway/src/tool_loop.rs | Inline loop: reserve tool action 3988-3999; finalize 4075-4087 (effect-ledger consumers); inline_tool_allowed 614-618 excludes save_memory, browser_agent, mcp__* and mcp_call; browser_agent dispatch arm 3071 (disabled unless BROWSER_AGENT_URL is set) | M (+425) | tests/e2e_invoke_chain_test.rs (M, +288) |
| model-gateway/src/stream_buffer.rs | Resumable SSE delta buffer (Redis or memory backend, chosen by REDIS_URL) | clean | in-file |
| model-gateway/src/coordinator.rs, idempotency_registry.rs | Ephemeral PlanMode and TeamWorker stores (audit: cache); /v1/invoke idempotency guard | clean | in-file |
| model-gateway/src/cancel_registry.rs, queued_input.rs | F-t3-016 adjacency (cancel, queued input) | clean | — |
| model-gateway/src/chat_turn_registry.rs, state.rs, session_flow.rs, zdr_precheck.rs, sse_events.rs | Adjacent run-control, ZDR precheck, SSE shapes | M (+36, +6, +5, 284 changed, +203) | not inspected |
| mp-contracts/src/lib.rs | Declares modules retention (line 4) and zdr_refusal (line 7) | M (+2) | — |
| mp-contracts/src/zdr_refusal.rs | Typed ZDR refusal used by live continuation_descriptor (agent.rs:4566) | ?? (1006 lines) | — |
| orchestrator-core/cmd/workflows/interactive_run.go | InteractiveRunSupervision 73; cancel 93; SignalApproval 53; approval wait 130-149 | clean | interactive_run_test.go: TestInteractiveRun_ApprovalSignal_AllowsCompletion 353; TestScheduledRun_UnknownOutcomeStopsWithoutAnotherStep 179 |
| orchestrator-core/cmd/workflows/step_loop.go | executeStepLoop 64; per-turn ExecuteActivity 72; break on NeedsApproval 82 | clean | autoresearch_test.go (mocks ExecuteStepActivity, 49-52) |
| orchestrator-core/cmd/workflows/wide_research.go | Durable step loop under GetVersion 74-108 | clean | — |
| orchestrator-core/cmd/activities/activities.go | ExecuteStepLoopActivity 620 (not resume-safe, 612-618); executeOneStep 590; ExecuteStepActivity 654 | clean | activities_test.go:78, 99 |
| orchestrator-core/internal/servicecred/minters.go | DefaultScopes: execution-core scheduled-step lane only (comment at 49) | clean | servicecred_test.go:347-350 |
| orchestrator-core/internal/orchestration/types.go | run.paused_for_approval and related event constants 18, 108-112, 150-151, 175 | clean | — |
| Frontend gateway apps/gateway/src/domains/orchestration.rs | Approval routes 71-93; list_approvals 359; get_approval 427; decide_approval 464 | clean | — |
| Frontend gateway apps/gateway/src/domains/ag_ui.rs | run_paused_for_approval mapping 893 | clean | — |
| Frontend src/shared/api/chat-client.ts | run_paused_for_approval case 1369 | M (+53) | — |
| Frontend src/shared/api/run-console-client.ts | RunPausedForApprovalEvent 75; case 274 | clean | run-console-client.test.ts:41 |
| Frontend src/shared/chat/verevon-ui-events.ts | Event names 115, 213 | clean | — |
| Frontend src/features/agents/components/AgentRunConsole.tsx | Comment on run_paused_for_approval 689 | M (+11) | AgentRunConsole.test.tsx (M) |
| Frontend src/features/chat/components/ChatMessages.tsx | Approval card (audit F-t3-006 cites 1885-1945; not re-verified) | M (+229) | ChatMessages.failure-actions.test.tsx (M) |

Dependency-adjacent (F-t3-016 and OD8 owners): Control auth-core convex-token.service.ts (clean; :331-424, :854-858); plane-service-principals.json (clean; :134-181); capability-core/cmd/main.go (clean; :338-372).

## 5. Related contracts

Proto (worktree; live status in brackets):
- execution.proto (clean): rpc ExecuteStep 15, ResumeRun 18, PauseRun 27, RunAgent 34, ExecuteScheduledStep 40. ResumeRunRequest 339-353: checkpoint_id=2 (unread), org_id=3, approval_id=4.
- sessions.proto [M +42]: rpc ReserveToolAction 55, FinalizeToolAction 59, SaveCheckpoint 62. Messages: ReserveToolActionRequest 652, FinalizeToolActionRequest 674, SaveCheckpointRequest 691-700, SaveCheckpointResponse 702-707. Live adds SessionRetentionService.CountWaivedRuns (not checkpoint or approval).
- orchestration.proto (clean): approval RPCs 70-94 (ClaimApprovalDeliveries 70, GetApprovalContinuation 76, RecordApprovalContinuationStarted 82, RecordApprovalContinuationOutcome 87, AcknowledgeApprovalDelivery 93). Continuation messages 793-917. OrchestrationEvent oneof run_paused_for_approval=15 (1036), run_resumed_after_approval=16 (1037); messages 1102, 1107.
- events.proto (clean): EVENT_TYPE_CHECKPOINT_RESTORED=111 (138).
- ids.proto (clean): CheckpointId 43. verified_outcome.proto (clean): VerificationResult used by continuation outcome.
- docs/CONTRACTS.md (clean) event registry 47-58: CHECKPOINT_SAVED 110; CHECKPOINT_RESTORED 111 (no producer or consumer found); APPROVAL_REQUESTED 121; APPROVAL_DECIDED 123; RUN_PAUSED_FOR_APPROVAL 128 and RUN_RESUMED_AFTER_APPROVAL 129 (owner orchestrator-core). RunResumedAfterApproval is referenced in session-core approval_delivery.rs, session-core orchestration_grpc.rs, model-gateway sse.rs and orchestrator-core types.go (emission sites not traced).

Migrations (session-core, worktree latest 0043_audit_outbox_requeue.sql; live adds 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, and migrations/revert/ with 0044 and 0045 down scripts, all ??):
- 0001_init.sql:46-53 checkpoints (id, run_id, state, created_at)
- 0002_events_and_ordinals.sql:74-97 checkpoint ordinal trigger and unique index (run_id, ordinal)
- 0003_orchestration_tables.sql:135 approvals
- 0013_tool_audit_intents.sql session_tool_audit_intents (reserved, completed, failed)
- 0014_approval_delivery_outbox.sql; 0018_approval_continuation_receipts.sql (receipts :6, outcomes :35); 0019_approval_delivery_settlement.sql; 0020_verification_result.sql
- 0022_encrypted_approval_continuation_descriptors.sql:4

Config and env (names read in code; deploy wiring):
| Name | Where | Default or requirement |
|---|---|---|
| SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY | continuation_crypto.rs:14; deploy/.env.example:93; deploy/docker-compose.yml:755; deploy/docker-compose.production.yml:70 | compose default empty; production requires it |
| EXECUTION_CORE_SERVICE_API_KEY | approval_delivery_worker.rs:192; .env.example:159; docker-compose.yml:1125 | required |
| EXECUTION_CORE_SERVICE_ID | approval_delivery_worker.rs:188-191 | defaults to execution-core |
| AUTH_CORE_URL | approval_delivery_worker.rs:187 | required (deploy wiring not traced) |
| EXECUTION_CORE_APPROVAL_DELIVERY_ORG_IDS | .env.example:201; docker-compose.yml:1254 | empty = auto-discover (module doc 41-47) |
| EXECUTION_CORE_APPROVAL_DELIVERY_POLL_SECONDS | .env.example:202; docker-compose.yml:1255 | 15 |
| PLANE_TOKEN_TTL_SESSION_CORE_SECONDS, PLANE_TOKEN_TTL_EXECUTION_CORE_SECONDS | Control convex-token.service.ts:398-424 | 300 |
| MODEL_PLANE_AUTH_TOKEN_TTL_SECONDS | convex-token.service.ts:346-350 | 900 |
| CONVEX_AUTH_TOKEN_TTL_SECONDS | convex-token.service.ts:331-334 | 300 |

SSE and stream events:
- run_paused_for_approval: producer session-core orchestration_grpc.rs:2578 (create path); projection model-gateway sse.rs:9857, 9941, 10133; AG-UI adapter gateway ag_ui.rs:893; client run-console-client.ts:274, verevon-ui-events.ts:213, chat-client.ts:1369 [M].
- awaiting_approval native frame: model-gateway sse.rs:9536-9540 (status awaiting_approval).
- docs/sse-event-taxonomy.md (worktree) has no approval entries; live is M (+4), not inspected.
- Go constant run.paused_for_approval: orchestrator-core/internal/orchestration/types.go:18.
- Gateway emits RUN_RESUME_REQUESTED on resume (http_routes.rs:1737).
- Stream resume: GET /v1/invoke/resume/:request_id (http_routes.rs:138; stream_buffer.rs).

Gateway and HTTP routes:
- Frontend gateway (clean): GET /api/v1/orchestration/runs/{run_id}/approvals (orchestration.rs:71-72); GET /api/v1/orchestration/approvals/{approval_id} (83-84); POST /api/v1/orchestration/approvals/{approval_id}/decide (92-93).
- model-gateway: POST /v1/orchestration/runs/:run_id/resume (http_routes.rs:317 into resume_run 1706). It refuses ZDR (1713) and sends an empty approval_id, so it resumes only user-paused runs (comment 1715-1716).

Action contracts: apps/Frontend Plane/verevonv3/src/shared/actions has no approval or resume contract. Only requiresApproval flags appear (agent-tools.ts:26, 49, 59, 134; action-registry.ts).

## 6. Test and build commands

- Rust workspace (apps/Model Plane/rust; 15 members per CI header): `cargo fmt --all -- --check`; `cargo check --workspace --all-targets`; `cargo test --workspace --no-fail-fast`. Source: .github/workflows/model-plane-rust-ci.yml (working-directory :57; fmt :99; check :105; all-targets :112; test :119). Contract goldens step :92-96.
- Per package (names from Cargo.toml `name`): execution-core (services/execution-core/Cargo.toml:2), session-core (:2), model-gateway (:2), inference-core (:2), mp-contracts (crates/mp-contracts/Cargo.toml:2), mp-events. Example: `cargo test -p execution-core --all-targets` (apps/Model Plane/CLAUDE.md "Testing").
- Windows note (user memory, not repo-verified here): two cargo targets fail pre-existing on Windows; use --lib.
- DB-gated tests (#[ignore], need DATABASE_URL to a disposable Postgres): `cargo test -p session-core --bin session-core <filter> -- --ignored --test-threads=1` (compaction_tests.rs:346-353; grpc.rs:2711-2884). Live-only runner (??): apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/program-2026-10-03/run_session_db_tests.sh (uses `cargo test --locked -p session-core --bin session-core zdr_receipt -- --ignored`).
- Proto: `buf generate` (apps/Model Plane/CLAUDE.md). mp-contracts build.rs generates from ../../../proto (CI header). `buf lint` has pre-existing debt (CLAUDE.md).
- Go orchestrator-core (module github.com/triodelab/model-plane/services/orchestrator-core, go 1.25.1 at go.mod:1-3): `cd "apps/Model Plane/go/services/orchestrator-core" && go build ./... && go vet ./... && go test ./...` (apps/Model Plane/CLAUDE.md). Targeted: `go test ./cmd/workflows -run TestInteractiveRun`. No .github workflow runs go test for orchestrator-core (grep). Go -race needs cgo (user memory).
- Frontend gateway, only if gateway routes change: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from apps/Frontend Plane/verevonv3 (root CLAUDE.md, Frontend block).
- Control auth-core, if OD8 TTLs change: test:cov in apps/Control Plane/auth-core (root CLAUDE.md). Existing 300 s assertion: convex-token.service.spec.ts:343.
- Frontend, only if UI changes: `pnpm lint`, `pnpm typecheck`, `pnpm test` in apps/Frontend Plane/verevonv3 (root CLAUDE.md).

## 7. Owners

- Root CLAUDE.md, Architecture Rules: Control Plane owns identity, users, orgs, billing, sessions, audit, quotas and entitlements. Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants and cost. ZDR must propagate through any content-persisting boundary. No direct database crossing between planes.
- apps/master-ownership-matrix.md: :126 session-core owns "threads, runs, checkpoints, memory index, compaction, context assembly"; :129 orchestrator-core owns "Temporal workflows, autoresearch, wide research, tasks/cron, approvals, recovery"; :168 rule 12 (provider execution never invents approval authority); :177 rule 17 (ZDR waiver).
- apps/Model Plane/docs/capability-ownership-matrix.md: §2 :36-42 (durable state: session-core; COORDINATION recovery and retries: orchestrator-core and execution-core; :42 gateway approvals.rs is an ephemeral cache that must hydrate from its owner). §3 :55 approvals and :56 checkpoints owned by session-core, with execution-core triggering. §4.1 :83-91 orchestration and approvals: session-core durability, orchestrator-core lifecycle and recovery. §6 :184 durable multi-step coordination goes to Temporal.
- apps/Model Plane/CLAUDE.md: Rust owns the request hot path and Go owns durable work; the two tool-dispatch loops must stay separate (execution-core execute_step_inner versus model-gateway dispatch_tool); check docs/decisions/ledger.md before any new registry.
- Control-side bearer lifetime and OD8: Control Plane auth-core (convex-token.service.ts). The owner decision is OD8 in the CHAT_GAP plan (:452).

## 8. Uncommitted-only (live tree; relevant to F-t3-002)

Untracked (exists only in live):
- mp-contracts/src/zdr_refusal.rs (1006 lines). Required by live agent.rs:4566 (mp_contracts::zdr_refusal::Refusal). A commit of agent.rs without it will not build.
- mp-contracts/src/retention.rs (declared by live lib.rs:4).
- session-core/src/retention_receipt.rs; migrations/0044_run_retention_receipt.sql; migrations/0045_zdr_waived_runs_org_index.sql; migrations/revert/ (0044 and 0045 down scripts).
- execution-core/tests/zdr_refusal_inventory.rs (no approval_continuation string found).
- api/contracts/zdr-refusals.json and .sha256 (apps/Model Plane).
- Frontend: plans/system-audits/program-2026-10-03/run_session_db_tests.sh (DB test runner); src/shared/api/zdr-refusals.ts and zdr-refusals.test.ts (client typing of ZDR refusals).

Modified (M), one-line change summary:
- execution-core/src/runtime_loop/agent.rs (+207): continuation_descriptor ZDR refusal becomes typed (`approval_continuation`, live 4565-4570); run_rounds passes ctx.route.is_waived() (live 1937-1939); many test additions.
- execution-core/src/grpc.rs (+98): waived-subscription admission test and route_caller helper. The resume_durable body is unchanged (moved to live 706-713).
- execution-core/src/runtime_loop/mod.rs (+18): call-site arguments (bodies not traced).
- execution-core/src/runtime_loop/session_core_rules_double.rs (+537): waiver claim test doubles and waived-run tests.
- session-core/src/grpc.rs (+861): SessionRetentionGrpc count_waived_runs; ZDR test renames; start_run_inner made pub(crate). SaveCheckpoint and validate_user_checkpoint bodies unchanged (no hunks in their ranges; shifted +2 and +171).
- session-core/src/orchestration_grpc.rs (+249): proof-bundle retention reason and receipt tests. Approval RPC names unchanged.
- session-core/src/orchestration_store.rs (+8): not traced.
- session-core/src/terminalization.rs (+123): start_managed_run_with_retention_inner and existing_retention_matches added; run_recovery_worker kept.
- session-core/src/run_service_grpc.rs (+21), session-core/src/main.rs (+1), session-core/src/learning_events.rs (+77, adjacent): not traced.
- proto/model_plane/v1/sessions.proto (+42): adds SessionRetentionService.CountWaivedRuns. No checkpoint or approval field in the added lines.
- model-gateway/src/http_routes.rs (+237): effective_invoke_persistence_plan signature changed; quarantine call sites at live 1560 and 1572; resume route body not traced.
- model-gateway/src/grpc.rs (+47): quarantine call at live 1550.
- model-gateway/src/tool_loop.rs (+425), sse.rs (+883), sse_events.rs (+203): not traced beyond the worktree call sites listed in section 4.
- model-gateway/src/chat_turn_registry.rs (+36), state.rs (+6), session_flow.rs (+5), zdr_precheck.rs (284 changed): adjacent; not traced.
- model-gateway/tests/e2e_invoke_chain_test.rs (+288), tests/orchestration_http_test.rs (+102): not traced.
- mp-contracts/src/lib.rs (+2): declares retention and zdr_refusal modules.
- apps/Model Plane/docs/sse-event-taxonomy.md (+4), docs/ZDR.md (64 changed): not traced.
- Frontend: shared/api/chat-client.ts (+53, run_paused_for_approval handling), features/chat/components/ChatMessages.tsx (+229), features/agents/components/AgentRunConsole.tsx (+11), features/chat/components/use-chat-controller.ts (+4), shared/api/sse.ts (+4).

Clean (relevant, no live change): execution-core state.rs, permission/mod.rs, approval_delivery_worker.rs, scrub.rs, main.rs, tests/runtime_loop_test.rs, tests/scrub_checkpoint_test.rs. session-core approval_delivery.rs, continuation_crypto.rs, compaction.rs, gdpr.rs, migrations 0001-0043. model-gateway approvals.rs, stream_buffer.rs, coordinator.rs, idempotency_registry.rs, cancel_registry.rs, queued_input.rs. All orchestrator-core Go files. proto execution.proto, orchestration.proto, events.proto, ids.proto, verified_outcome.proto. docs/CONTRACTS.md, docs/decisions/ledger.md, both ownership matrices, CHAT_GAP plan. Control convex-token.service.ts, plane-service-principals.json, capability-core cmd/main.go. Gateway orchestration.rs and ag_ui.rs; run-console-client.ts; verevon-ui-events.ts.

## 9. NOT DETERMINED

- Code that binds the 300 s delegated bearer to a gateway run; only the env defaults (convex-token.service.ts) were traced. Whether Control has any renewal or re-mint endpoint usable at resume.
- Whether SaveCheckpoint has any production caller other than ExecuteStep (grpc.rs:1027). Only test doubles were found elsewhere.
- Whether every SaveCheckpoint caller is ZDR-gated. The handler has no zdr check (grpc.rs:4701-4777); the gates were found upstream only for ExecuteStep and resume_run.
- Whether the terminalization recovery worker can re-drive a Do run or only managed-run terminal state (run_recovery_worker, terminalization.rs:1759).
- "Parking on provider exhaustion": no symbol found. subscription_refusal.rs and subscription_failure.rs were not traced.
- Emission sites of RUN_RESUMED_AFTER_APPROVAL (CONTRACTS.md:58) were not traced.
- Bodies changed in live tool_loop.rs, sse.rs, http_routes.rs (resume route), orchestration_store.rs, runtime_loop/mod.rs, run_service_grpc.rs, sse-event-taxonomy.md and ZDR.md were not inspected beyond stats and name checks.
- ChatMessages.tsx approval-card line numbers (audit cites 1885-1945 for F-t3-006) were not re-verified.
- Whether OrchestrationEvent.RunPausedForApproval is defined at the orchestration.proto lines cited; the proto message lines were located (1036, 1102) but not read.
- Whether the Go orchestrator-core tests pass. No run was attempted and no CI gate for them was found.
- The "approval rehydration" and "effect ledger" primitives are present but not wired into a resume path. Whether tool_loop.rs reserve and finalize calls behave differently in the live tree was not verified (tool_loop.rs is M, +425).
- Nothing was executed. All verdicts are from reading code and grep.
