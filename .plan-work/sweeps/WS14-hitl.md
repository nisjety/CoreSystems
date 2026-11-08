# WS14-hitl code-map sweep (audit item F-t3-011)

Read-only sweep, 2026-10-15. No product file, config, doc, build or test was touched or run. Verdicts are static.

- WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` at HEAD afb83928 (verdict basis; clean except `.plan-work/`).
- LIVE = `C:/dev/CoresSystem`, same HEAD plus uncommitted work. Status from `.plan-work/live-tree-status.txt` (snapshot 20:56), re-checked with `git --no-optional-locks diff|status` for each relevant path.
- Audit (read-only input): `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Card L501-515 (table L502-504). 9.1 W2 row L1561. Appendix B L1829. Appendix C L2008-2016. Appendix D: no F-t3-011 line. Verification record: `audit-evidence/t3/verify/t3-v03.verdicts.json` (F-t3-011 entry) and `audit-evidence/t3/doc/appx-C-corrections.md` L91-96.
- Paths are repo-relative from WT unless marked LIVE. "LIVE numbers" means the cited line matches the LIVE tree, not WT.
- Verdicts: MATCH (symbol or behaviour at the cited lines), DRIFT (found elsewhere; new range given), ABSENT (not in WT), LIVE-ONLY (untracked `??`), DIFFERS-IN-LIVE (`M`, behaviour differs from WT).

## 0. Summary

1. Card F-t3-011: Priority, Partial, effort L, Model Plane and Frontend Plane, W2, confidence High. Depends on F-t3-002, F-t3-001, F-t3-016, F-t3-015 (L1561). Appendix C corrects the reference side: Codex is not a model for approve-then-continue; DeepSeek is (L2008-2016).
2. The card's line numbers follow LIVE. ChatMessages.tsx:1889/2105/2217 are exact in LIVE and two lines lower in WT. The ChatPage and agent.rs ranges still enclose the cited code in both trees.
3. The one ABSENT citation, `zdr_posture.rs:247-260`, is LIVE-ONLY (untracked). `continuation_gate` (257-264) exists only in LIVE. In committed HEAD the plan approve, plan reject, todo-update and plan-approval handlers have no ZDR refusal call. Only decide, cancel and resume use `require_non_zdr_durable_mutation`.
4. No ask_user, user_question or request_user_input symbol exists in WT or LIVE (Rust and frontend src). PLAN.md:134 lists AskUserQuestionTool as an unported claude-code-fork gap.
5. Approval writes two grants (section 3). An in-memory, thread-keyed grant (PlanModeStore) is what the next run reads. A run-level grant (`runs.metadata.autonomy_rung`) is already persisted in session-core by SetRunMode. The card's "in-memory PlanModeStore" and "move the grant into session-core" are both accurate; the run-level half already exists.
6. Nothing starts on approval (card correct). The approved plan is consumed by the NEXT user message on the thread: run start calls `approved_plan_context` (sse.rs:9223 WT), which takes the grant, claims the plan APPROVED to EXECUTING once, and injects it.
7. Tool-approval continuation is quarantined in WT. A granted decision is recorded durably, then the gateway answers `continuation_delivery: "pending"` and never sends ResumeRun (http_routes.rs:1551-1581; approvals.rs:254-272). The card's "resume re-invokes run_agent" holds only on the execution-core side. An ask_user built on the approval store would inherit this boundary.
8. The sibling Space card F-t1-026 (request_input as an end-turn tool that starts a new run, typed form nodes) and F-t3-011 (ask_user as a new kind in the existing durable pause, user_question event and card) overlap and disagree on mechanism and vocabulary. See section 8.

## 1. Card citations (step 3)

Rows marked (extra) come from the verification record or from this sweep's own reads, not from the card text.

| card id | cited path:lines | claim | WT verdict | LIVE status | corrected path:lines (WT / LIVE) |
|---|---|---|---|---|---|
| F-t3-011 | apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts:2030-2135 | approve records grant, starts nothing | MATCH: approveTurnPlan 2040-2095; setPlanMode(false) 2077; warning 2078-2086; rejectTurnPlan 2103-2140 | M: +4 lines at 3283-3295, outside range | 2040-2095 / 2040-2095 |
| F-t3-011 | apps/Model Plane/rust/services/model-gateway/src/coordinator.rs:40-215 | in-memory PlanModeStore holds grant | MATCH: PlanModeStore 69; grants map 85; GrantEntry 88-99; record_grant 166-187; take_approved_plan 197-203; granted_rung 208-222 | clean | 58-222 / 58-222. Writer outside range: handle_exit_plan_mode 298-371, record_grant call 327 |
| F-t3-011 | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4441-4540 | durable approval pause, AwaitingApproval | DRIFT: approval_kind_for 4446; pause_for_approval 4469-4545; AwaitingApproval set 4521; doc 4436-4438 | M: +7 shift from hunks at 570-1932; pause body not in hunks | 4439-4545 / 4446-4552 (AwaitingApproval 4528) |
| F-t3-011 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1889 | ApprovalRequests card in transcript | DRIFT -2: ApprovalRequests 1887 | M: +185/-44; LIVE shift +2 from hunk at line 62 | 1887 / 1889 (exact) |
| F-t3-011 | ChatMessages.tsx:2105 | QueuedInputStrip in transcript | DRIFT -2: 2103 | M (as above) | 2103 / 2105 (exact) |
| F-t3-011 | ChatMessages.tsx:2217 | PlanApprovalControl in transcript | DRIFT -2: 2215 | M (as above) | 2215 / 2217 (exact) |
| F-t3-011 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx:880-960 | composer dock, queue strip | MATCH (range): QueuedInputStrip 881; dock 919-943; approveTurnPlan call 848 | M: +12/-1; LIVE shift +11 after lines 789-790 | strip 881, dock 919-943 / strip 892, dock 931-954; call 848 / 859 |
| F-t3-011 | apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs:247-260 | continuation_gate refuses ZDR | ABSENT: no zdr_posture.rs in WT model-gateway/src; no continuation_gate anywhere in WT Rust | LIVE-ONLY (`??`): continuation_gate 257-264; doc 247-256; refusal line 259 | none / 247-264 |
| F-t3-011 | card L514 "plan rows B2, B5, B7" -> apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md | plan rows B2, B5, B7 | MATCH: B2 line 148 (approve starts run), B5 149, B7 178 (continue after gate) | clean | 148, 149, 178 / same |
| F-t3-011 | card L512 "Reuse session-core SetRunMode, PlanApprovalControl, ApprovalRequests, QueuedInputStrip, composer dock" | reuse targets exist | MATCH: SetRunMode sessions.proto:141, 1136, 1162 and session-core grpc.rs:4984; components as above | sessions.proto M (+42, adds SessionRetentionService; SetRunMode untouched); session-core grpc.rs M, no live hunk in 4800-5100 | sessions.proto 141/1136/1162; session-core grpc.rs 4984 / same |
| F-t3-011 | card L512 "execution-core mints an approval" | who mints the approval | NUANCE: execution-core requests CreateApproval from session-core (agent.rs:4480-4518). Comment at 4497: session-core mints the durable id | M | 4480-4518 / 4487-4525 |
| F-t3-011 (extra) | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1911 | approvePlan client call | MATCH at LIVE 1911; WT 1858 (LIVE has +53 insertions above) | M: +53, no hunk inside approvePlan | 1858 / 1911. POST /api/v1/agents/runs/{runId}/plan-approval at WT 1866 |
| F-t3-011 (extra) | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts (rejectPlan) | rejectPlan client call | WT 1896 | M (+53) | 1896 / 1949 |
| F-t3-011 (extra) | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:137-142 | invoke, stream, resume, cancel, queue routes | MATCH: /v1/invoke/:request_id/queue at 142; cancel 140 | M: hunks start at 824 | 137-142 / 137-142 |
| F-t3-011 (extra) | http_routes.rs:1393-1451 | approve_plan and reject_plan handlers | DRIFT: approve_plan 1397-1422; reject_plan 1425-1450. No ZDR call in WT | DIFFERS-IN-LIVE: continuation_gate at 1402 (approve) and 1432 (reject) | 1397-1450 / 1393-1451 |
| F-t3-011 (extra) | apps/Model Plane/rust/services/model-gateway/src/sse.rs:8717-8760 | approved_plan_context doc and fn | DRIFT: 8357-8400 (fn 8384) | M: +360 shift at this point | 8357-8400 / 8717-8760 |
| F-t3-011 (extra) | sse.rs:8840-8870 | approved_plan_block_for: take, GetPlan, claim, inject | DRIFT: 8480-8510 (fn 8485; take_approved_plan 8491) | M (as above) | 8480-8510 / 8840-8870 (take 8851) |
| F-t3-011 (extra) | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:14-25 | module HITL doc: resume re-invokes run_agent | MATCH: lines 16-19 WT ("Resume re-invokes `run_agent`" at 19) | M, but no hunk above line 570, so same lines | 12-26 / 14-25 |
| F-t3-011 (extra) | agent.rs:930-940 | Paused arm returns early | MATCH: arm at 932 WT | M: arm at 939 LIVE | 928-942 / 935-947 (arm 939) |
| F-t3-011 (extra) | apps/Model Plane/docs/PLAN.md:134 | AskUserQuestionTool listed as gap | MATCH | clean | 134 / 134 |
| F-t3-011 (extra) | apps/Model Plane/docs/decisions/ledger.md:2591-2616, 2660-2672, 2706-2713 | C7: thread grant in memory; durable carrier "remains open" | MATCH (section "The 8 reachability defects fixed (2026-08-26)") | clean | same |
| F-t3-011 (extra) | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:214, 304, 307, 7041 | plan-approval route; approve and reject routes; plan_approval handler | MATCH: route 214 `/v1/runs/:run_id/plan-approval`; 304 `/v1/orchestration/plans/:plan_id/approve`; 307 `/reject`; handler 7041 | M; lines above 824 unchanged | plan_approval 7041 / 7147 |
| F-t3-011 (extra) | http_routes.rs:823-834, 1500, 1608, 1713 | ZDR helper callers in HEAD | MATCH: helper 823-834 (claims.zdr); callers decide_approval 1500, cancel_run 1608, resume_run 1713; none in approve, reject, todo, plan_approval | DIFFERS-IN-LIVE: helper now calls zdr_posture::zdr_or_refuse; continuation_gate added at approve 1402, reject 1432, todo 1468, decide 1504, cancel 1613, resume 1721, plan_approval 7156 | 823-834, 1500, 1608, 1713 / 823-834, 1504, 1613, 1721, 7156 |
| F-t3-011 (extra) | http_routes.rs:1551-1581 | granted tool approval: no ResumeRun, continuation_delivery "pending" | MATCH | M, but block not in hunks (hunks only at 1500 and 1608) | 1551-1581 / approx 1554-1584 |
| F-t3-011 (extra) | apps/Model Plane/rust/services/model-gateway/src/approvals.rs:254-272 (fn 265) | granted approval continuation quarantined | MATCH (sibling F-t1-026 cites the same lines). Callers: http_routes.rs:1568; grpc.rs:1541 | clean | 254-277 / same |
| F-t3-011 (extra) | apps/Model Plane/rust/services/model-gateway/src/grpc.rs:1541 | gRPC decide path also calls quarantine | MATCH | M (+31/-16) | 1541 / 1550 |
| F-t3-011 (extra) | http_routes.rs:7041-7305 (plan_approval) | owner check, thread lookup, grant write order | MATCH: owner check 7057; get_run 7097-7109; plan-of-run check 7117-7137; reject branch 7145-7176 (no grant); rung 7181; escalation 7188; TransitionPlan APPROVED 7203-7218; handle_exit_plan_mode 7225-7239; set_run_mode from 7248 (end not read) | DIFFERS-IN-LIVE: continuation_gate at 7156 | 7041-7305 / 7147+ (shift +106) |

## 2. Affected-file map (step 4)

Frontend (apps/Frontend Plane/verevonv3/src)

| path | role | approx lines (WT) | LIVE status | tests (path and names where cheap) |
|---|---|---|---|---|
| features/chat/components/use-chat-controller.ts | approveTurnPlan, rejectTurnPlan, plan-mode toggle, plan-approval errors | 2030-2140 | M (+4, comment and field near 3283-3295) | use-chat-controller.plan-gate.test.ts (clean): "approveTurnPlan forwards the proposed plan id..." (:159); "rejectTurnPlan calls rejectPlan..." (:197); "a refused approval grants nothing and leaves plan mode on for a retry" (:255). use-chat-controller.zdr-refusal.test.ts (`??`) |
| features/chat/components/ChatPage.tsx | plan-card onApprove wiring (848); QueuedInputStrip mount (881); composer dock (919-943); work-period props (789-801) | 848-960 | M (+12/-1) | mid-run-input.test.ts:174 (clean) asserts `<QueuedInputStrip entries={state.queuedInputs} />` in page source |
| features/chat/components/ChatMessages.tsx | ApprovalRequests 1887; QueuedInputStrip 2103; PlanApprovalControl 2215; used at 280 and 289 | 1887-2260 | M (+185/-44; edits sit in ErrorNotice 3414-3422 and EmptyChatState 3763-3898) | ChatMessages.plan-gate.test.tsx (clean; describe :76; tests :77, :95, :120, :140, :147). ChatPanels.test.tsx (clean; QueuedInputStrip describe :666 with tests :680-718; PlanApprovalControl describe :729 with tests :747-1078) |
| features/chat/components/chat-types.ts; chat-normalizers.ts | queued-input and plan types; normalizers | not read | M (+5/-2; +39) | chat-normalizers.zdr-refusal.test.ts (`??`) |
| features/chat/components/ChatMessages.failure-actions.test.tsx | failure-action tests | not read | M (+34) | itself |
| shared/api/chat-client.ts | approvePlan 1858 (LIVE 1911); rejectPlan 1896 (LIVE 1949); plan_proposed case 1641; onPlanProposed type 643; ChatQueuedInputEvent 244 | 243-1900 | M (+53; no hunk inside approvePlan) | chat-client.test.ts (M, names not read); chat-client.zdr-refusal.test.ts (`??`) |
| features/dashboard/home/DashboardComposer.tsx | mid-run composer and work-period picker props | not read | M (+7) | DashboardComposer.mid-run.test.tsx (clean; comment :11 names deliverMidRun, /v1/invoke/{id}/queue, QueuedInputStrip) |
| shared/chat-nodes/types.ts (:140); derive.ts (:187) | typed conversation nodes; PlanApprovalControl degrade note | 140, 187 | clean | derive.test.ts (clean; :171) |
| shared/actions/action-registry.ts | action contracts; only social.decide_approval (432-558); no plan-approval or ask contract | 432-558 | clean | action-registry.test.ts (clean; names not read) |

Gateway / BFF (apps/Frontend Plane/verevonv3/apps/gateway)

| path | role | approx lines (WT) | LIVE status | tests |
|---|---|---|---|---|
| src/domains/agents_runs.rs | BFF POST /api/v1/agents/runs/{run_id}/plan-approval (44) forwards to /v1/runs/{id}/plan-approval (231) | 44, 231 | clean | none found by name (NOT DETERMINED) |

Model Plane model-gateway (apps/Model Plane/rust/services/model-gateway/src)

| path | role | approx lines (WT) | LIVE status | tests |
|---|---|---|---|---|
| coordinator.rs | PlanModeStore (in-memory, thread-keyed grants); handle_enter_plan_mode 235; handle_exit_plan_mode 298-371 with record_grant 327 | 58-222, 235-371 | clean | mod tests 612-899: plan_mode_enter_and_check (612); plan_mode_exit_returns_was_active (622); plan_mode_expired_entries_are_swept (633); plan_mode_ttl_capped_at_max (651); plan_mode_lifecycle_events_use_the_fixed_run_event_subject (659); a_grant_is_readable_by_the_next_run_on_the_same_thread (800); the_plan_approved_with_a_grant_is_handed_over_exactly_once (821); a_grant_made_without_a_plan_hands_over_no_plan (848); a_newer_grant_replaces_the_pending_plan_of_an_older_one (860); an_empty_thread_or_unspecified_rung_records_nothing (877); exiting_plan_mode_records_the_grant_for_the_thread (899) |
| http_routes.rs | route table (140, 142, 214, 304, 307, 276); approve_plan 1397; reject_plan 1425; decide_approval 1493; cancel_run 1602; resume_run 1706; plan_approval 7041; invoke_queue_input 7308; grant quarantine 1551-1581 | as listed | M (+190/-47; ZDR gates, see section 1) | tests/orchestration_http_test.rs (below); e2e_invoke_chain_test.rs (below) |
| sse.rs | run start: plan_mode flag and approved_plan_context call 9215-9226; approved_plan_context 8384; approved_plan_block_for 8485; take_approved_plan 8491; granted_rung read 8663; mark_run_plan_mode 6154; latest_proposed_plan 8600 | as listed | M (+814/-69; ZDR stream helpers; approved-plan block shifted +360) | sse.rs tests: latest_proposed_plan_* (10355, 10370, 10380); only_an_approved_plan_of_this_conversation_is_handed_to_a_run (10402); an_approved_plan_is_claimed_then_handed_to_exactly_one_run (10542); a_run_that_loses_the_claim_is_handed_no_plan_and_nothing_is_retried (10567); a_plan_that_is_not_an_approved_plan_of_this_thread_is_never_claimed (10584); an_unreadable_plan_and_a_thread_with_no_pending_plan_hand_the_run_nothing (10600) |
| sse_events.rs | ChatEvent enum: PlanProposed 272; QueuedInput 318; event names "plan_proposed" 416 and "queued_input" 417; control-event rule 1409 | 242-420 | M (+202/-1; adds ZdrError) | sse_events.rs test module 1381-1828 (names not read) |
| approvals.rs | ApprovalStore (ephemeral cache per capability matrix row 42); granted-approval quarantine | 254-277 | clean | tests near 1494-1502 (quarantine assertions) |
| queued_input.rs | QueuedInputRegistry for mid-run input | not read | clean | e2e_invoke_chain_test.rs queue tests at WT 6701 and 6718 (LIVE shows 6718 only; file is M) |
| zdr_posture.rs | continuation_gate, zdr_or_refuse, decide_for, Route::plain | WT absent; LIVE 238-264 | LIVE-ONLY (`??`) | tests/zdr_posture_matrix.rs (`??`); tests/zdr_feature_table.rs (`??`) |
| zdr_precheck.rs | ZDR pre-refusal helpers; refusal bodies moved to mp-contracts | WT 1-200 | M (+58/-226) | in-file tests (not read) |
| state.rs | AppState.plan_mode (256; init 446) | 256, 446 | M (+5/-1; cross-encoder boot check, unrelated) | none |
| grpc.rs | gRPC decide path calls quarantine | 1541 | M (+31/-16) | not determined |
| tool_loop.rs | plain-chat tool dispatch (dispatch_tool); no ask_user builtin | not read | M (+370/-55) | not determined |

Model Plane execution-core (apps/Model Plane/rust/services/execution-core)

| path | role | approx lines (WT) | LIVE status | tests |
|---|---|---|---|---|
| src/runtime_loop/agent.rs | HITL pause: approval_kind_for 4446; pause_for_approval 4469-4545 (AwaitingApproval 4521); Paused arm 932; resume note 19; continuation descriptor 4558 (LIVE) | as listed | M (+200/-7; pause body unchanged) | continuation_descriptor_binds_the_exact_action_and_refuses_zdr (WT 5017 / LIVE 5025); RunStatus::AwaitingApproval asserts (LIVE 8184, 9195, 9261) |
| src/grpc.rs | resume_run 1068 (approval_id or resume_paused); run_agent 1203 -> runtime_loop::agent::run_agent 1249 | as listed | M (+76/-22) | not determined |
| tests/autonomy_ladder_contract.rs | source-text contract: handle_exit_plan_mode and set_run_mode bodies must contain AutonomyEscalation::request( | file | clean | itself |
| tests/zdr_refusal_inventory.rs | ZDR refusal inventory | file | `??` | itself |

Model Plane session-core (apps/Model Plane/rust/services/session-core)

| path | role | approx lines (WT) | LIVE status | tests |
|---|---|---|---|---|
| src/grpc.rs | set_run_mode 4984-5075: mode whitelist (execute, plan, reactive, research); OwnerIntent::Mutate; grant re-validated; JSONB merge of autonomy_rung and autonomy_justification into runs.metadata; no ZDR check inside the fn | 4984-5075 | M (+817/-44; no hunk in 4800-5100) | set_run_mode_and_upsert_skill_against_real_pg (9338) |
| src/orchestration_grpc.rs | CreateApproval, DecideApproval, TransitionPlan handlers (not read) | not read | M (+243/-6) | not determined |
| src/orchestration_store.rs | plans, steps, todos, approvals store (25 pub fns per capability matrix row 19) | not read | M (+7/-1) | not determined |
| migrations 0003, 0014, 0018, 0022 | plan and approval tables; approval delivery outbox; continuation receipts; encrypted continuation descriptors | file | clean | not applicable |

Model Plane crates and docs

| path | role | approx lines | LIVE status | tests |
|---|---|---|---|---|
| rust/crates/mp-contracts/src/autonomy.rs | AutonomyEscalation::request (rung validation) | file | clean | not read |
| rust/crates/mp-contracts/src/zdr_refusal.rs | typed ZDR refusal bodies; DURABLE_MUTATION_FORBIDDEN | file | `??` | mp-contracts/tests/zdr_retention_matrix.rs (`??`) |
| docs/sse-event-taxonomy.md | SSE event table | rows | M (+4 rows: error to ZdrError; plan_proposed; retrieval_notice; source_scope_applied) | none (no question or ask row in WT or LIVE) |
| docs/PLAN.md | gap list incl. AskUserQuestionTool | 134 | clean | none |
| docs/capability-ownership-matrix.md | owner rules (section 6) | 19, 26, 42, 55, 83-91 | clean | none |
| docs/ZDR.md | ZDR documentation | not read | M (+63/-1) | none |

Repo-level ownership docs

| path | role | approx lines | LIVE status | tests |
|---|---|---|---|---|
| apps/master-ownership-matrix.md | plane rules | 41, 45, 121-129 | clean | none |

Tests (cross-cutting; names where cheap)

- apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs (M): post_plan_approval helper 1671, used by plan-approval tests at 1699 and 1762 (WT). decide_approval_grant_succeeds_without_a_continuation_dispatcher (1015, WT). LIVE-only: a_zdr_credential_is_refused_before_any_plan_is_read_or_decided; a_waiver_is_not_refused_at_the_gateway_so_session_core_checks_its_receipt; helper user_tokens_with_retention.
- apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs (M): LIVE-only ZDR tests, including a_zdr_unary_turn_asking_for_plan_mode_is_refused_with_a_typed_body_and_no_model_call.
- Frontend tests named above; none of the plan-gate tests is in the live status list (clean).

## 3. Grant and continuation chain (WT unless marked)

1. Client: ChatPage plan card calls use-chat-controller `approveTurnPlan` (2040). It calls `approvePlan` (chat-client.ts:1858; LIVE 1911).
2. BFF: POST `/api/v1/agents/runs/{run_id}/plan-approval` (agents_runs.rs:44) forwards to model-gateway `/v1/runs/{run_id}/plan-approval` (agents_runs.rs:231; http_routes.rs:214).
3. `plan_approval` (http_routes.rs:7041): run-owner check (7057); decision is approve or reject; `get_run` resolves the thread (7097-7109); the plan must belong to that thread (7117-7137).
4. Reject branch (7145-7176): TransitionPlan REJECTED; grants nothing; no set_run_mode.
5. Approve branch: rung keyword and AutonomyEscalation (7181-7190); TransitionPlan APPROVED (7203-7218). Comment at 7093-7096: a failed thread lookup must leave the plan PROPOSED.
6. `handle_exit_plan_mode` (coordinator.rs:298-371; record_grant at 327): in-memory PlanModeStore grant keyed by THREAD, 24 h TTL (MAX_PLAN_TTL_SECS). The approved plan id is handed to the next run once (take_approved_plan, 197-203).
7. `set_run_mode` (plan_approval from 7248; session-core grpc.rs:4984-5075): mode "execute"; grant re-validated server-side; `autonomy_rung` and `autonomy_justification` merged into runs.metadata; returns `granted_rung`. The gateway reports `persisted` from this call.
8. Frontend: setPlanMode(false) at use-chat-controller 2077. If not persisted, 2078-2086 warns that the next message may lack the grant.
9. Next user message on the thread: run start (sse.rs:9215-9226) skips the lookup when the run is itself in plan mode. Otherwise approved_plan_context (8384) calls take_approved_plan (8491), GetPlan, approved_plan_is_for, then claims the plan APPROVED to EXECUTING through TransitionPlan (compare-and-swap in session-core), then formats the plan block (8528). granted_rung (8663) supplies the rung for the run.
10. Tool approvals (separate path): agent pause (agent.rs:4469-4545) creates the approval in session-core. decide_approval (http_routes.rs:1493) records the grant durably. The gateway quarantine (approvals.rs:265; http_routes.rs:1568) then answers continuation_delivery "pending" and sends no ResumeRun. Execution-core resume_run (grpc.rs:1068-1092) fetches an approval by id; the rest of that path was not traced.

## 4. Related contracts (step 5)

- Protobuf:
  - sessions.proto: rpc SetRunMode 141; SetRunModeRequest 1136 (mode, granted_rung, justification); SetRunModeResponse 1162. Clean in HEAD. LIVE adds SessionRetentionService at file end (+42), unrelated.
  - runs.proto: enum AutonomyRung 22 (clean).
  - orchestration.proto: TransitionPlan 24; CreateApproval 47; DecideApproval 65 (clean).
  - execution.proto: ResumeRun 18 (clean).
  - mp-contracts/src/autonomy.rs (clean): AutonomyEscalation::request.
  - mp-contracts/src/zdr_refusal.rs (`??`): Refusal::DURABLE_MUTATION_FORBIDDEN.
- SQL migrations (session-core, apps/Model Plane/rust/services/session-core/migrations):
  - WT latest: 0043_audit_outbox_requeue.sql (preceded by 0042_thread_persona_toggle.sql, 0041_mail_task_thread_origin.sql, 0040_direct_run_completion_key.sql).
  - LIVE adds 0044_run_retention_receipt.sql (`??`), 0045_zdr_waived_runs_org_index.sql (`??`), and revert/0044_run_retention_receipt.down.sql and revert/0045_zdr_waived_runs_org_index.down.sql (`??`). Neither is plan-related.
  - Plan and approval tables: 0003_orchestration_tables.sql, 0014_approval_delivery_outbox.sql, 0018_approval_continuation_receipts.sql, 0022_encrypted_approval_continuation_descriptors.sql.
  - No migration creates a thread-scoped grant table. Grep hits for grant, autonomy and plan_mode in 0016, 0028 and 0031 are unrelated (NATS publish grant, credential grant ref, skill grantees).
- SSE events (sse_events.rs): ChatEvent::PlanProposed (272, "plan_proposed" 416); ChatEvent::QueuedInput (318, "queued_input" 417). No question or ask event in WT or LIVE. LIVE adds ChatEvent::ZdrError. Frontend dispatch: chat-client.ts case 'plan_proposed' (1641, WT); ChatQueuedInputEvent type (244).
- Gateway routes (model-gateway): /v1/invoke/:request_id/cancel (140); /v1/invoke/:request_id/queue (142, invoke_queue_input 7308); /v1/orchestration/plans/:plan_id GET (276); /v1/orchestration/plans/:plan_id/approve (304); /reject (307); /v1/runs/:run_id/plan-approval (214). BFF: /api/v1/agents/runs/{run_id}/plan-approval (agents_runs.rs:44).
- Action contracts (apps/Frontend Plane/verevonv3/src/shared/actions): no plan-approval or ask contract. A narrow grep (plan_mode, plan_approval, plan_propose, plan-approval, approvePlan, PlanApproval, propose_plan, ask_user, user_question, question) on action-registry.ts returns nothing. The only approval contract is social.decide_approval (432-558). The root CLAUDE.md rule says to add the contract before wiring a meaningful UI operation.
- ZDR contract: apps/Model Plane/api/contracts/zdr-refusals.json (`??`; contract "model-plane.zdr-refusals" at line 74; keys plan_mode at 93, approval_continuation at 141) plus .sha256 (`??`). Frontend mirror: shared/api/zdr-refusals.ts (`??`).
- Config and env: no HITL-specific variable found. Not determined beyond grep.

## 5. Test and build commands (step 6)

Rust (workspace root apps/Model Plane/rust; members listed in rust/Cargo.toml lines 4 mp-contracts, 14 model-gateway, 15 session-core, 17 execution-core; package names from each Cargo.toml `name =`):
- Per crate: `cargo test -p model-gateway --all-targets`, `-p execution-core`, `-p session-core`, `-p mp-contracts`. Model Plane CLAUDE.md: `cargo test -q -p <service> --all-targets`; `cargo test --workspace`.
- CI: .github/workflows/model-plane-rust-ci.yml: working-directory apps/Model Plane/rust (L57); `cargo fmt --all -- --check` (L99); `cargo check --workspace` (L105); `cargo check --workspace --all-targets` (L112); `cargo test --workspace --no-fail-fast` (L119).
- Source-text contract: `cargo test -p execution-core --test autonomy_ladder_contract`.
- User memory (model-plane-build-and-deploy.md): two cargo targets fail pre-existing on Windows; use --lib. Not re-verified here.
- The LIVE-only tests (section 2) cannot run from WT until the uncommitted work is committed or applied.

Frontend (apps/Frontend Plane/verevonv3, package.json):
- `pnpm test` = node --test (four gate scripts) && vitest run (L13). `pnpm typecheck` = tsc -b (L17). `pnpm lint` = eslint . (L11). `pnpm build` = pnpm typecheck && vite build (L9).
- Targeted vitest on the plan-gate and queue tests (suggested, not run): use-chat-controller.plan-gate.test.ts, ChatMessages.plan-gate.test.tsx, ChatPanels.test.tsx, mid-run-input.test.ts, DashboardComposer.mid-run.test.tsx.
- CI: .github/workflows/verevonv3-ci.yml: gateway crate verevon-gateway-rs in apps/Frontend Plane/verevonv3/apps/gateway (working-directory L113) runs cargo fmt --check (L133), cargo clippy --all-targets -- -D warnings (L138), cargo test (L141).
- User memory (verevonv3-worktree-verification.md): :5173 is the main checkout; run a second dev server in worktrees; typecheck, lint and vitest have pre-existing failures. Not re-verified.

## 6. Owners (step 7)

- Model Plane capability matrix (apps/Model Plane/docs/capability-ownership-matrix.md):
  - Row 26: "Plan mode / approvals: EXIST (in-mem, gateway)".
  - Row 42: model-gateway coordinator.rs, approvals.rs and runtime_registries.rs are ephemeral caches, not systems of record. They must hydrate from the owner and publish mutations to NATS.
  - Row 55: plans, steps, todos, approvals and subagent lineage are owned by session-core (orchestration_store.rs), Postgres; marked CONSOLIDATE.
  - Section 4.1 (83-91): the fix is for the gateway to emit OrchestrationEvent on mp.v1.orchestration.{plan,approval}, with a session-core consumer that persists.
  - Implication: the thread-scoped grant belongs to session-core under rows 42, 55 and 4.1. The card's "move the grant into session-core" matches this ruling.
- Master matrix (apps/master-ownership-matrix.md):
  - Line 41: planning, reasoning, acting as an agent and choosing tools belong to Model Plane.
  - Line 45: durable workflow, grants, billing and operator control belong to Go.
  - Lines 121-129: Model Plane target. model-gateway (Rust) holds "ZDR/cross-plane guards". orchestrator-core (Go) holds "Temporal workflows ... approvals, recovery".
  - Conflict: master line 129 places approvals under orchestrator-core (Go), while capability row 55 places them under session-core (Rust). NOT DETERMINED which is canonical.
- Frontend Plane (root CLAUDE.md, Architecture Rules): owns the Verevon v3 UI and same-origin gateway/BFF normalization, which covers the chat cards and trays and agents_runs.rs.
- Model Plane CLAUDE.md (Code Style): the two tool-dispatch loops must stay separate. model-gateway dispatch_tool is the read-only plain-chat loop. execution-core execute_step_inner is the deployed-agent loop. Where an ask_user builtin sits must respect this. Not determined which loop.
- Control Plane (root CLAUDE.md): identity, users, orgs, billing, sessions, audit, quotas, entitlements. No HITL ownership.
- ZDR: root CLAUDE.md says ZDR must propagate through any content-persisting boundary. Live code cites "owner decision 17" (http_routes.rs live 1400-1401; zdr_posture.rs live 247-253). The ruling text was not located.

## 7. Uncommitted-only (step 8)

Files relevant to F-t3-011 that exist only in LIVE (`??`) or differ there (`M`). Stats are from `git --no-optional-locks diff --shortstat` in C:/dev/CoresSystem.

| path (repo-relative) | live status | one-line change |
|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs | ?? | New file: continuation_gate (257-264), zdr_or_refuse (238-245), decide_for. The only home of the card's ZDR gate. |
| apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs | ?? | New file; name suggests ZDR feature gating (not read). |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | ?? | New file: typed refusal bodies, including DURABLE_MUTATION_FORBIDDEN. |
| apps/Model Plane/rust/crates/mp-contracts/src/retention.rs | ?? | New file; name suggests retention types (not read). |
| apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256) | ?? | Refusal catalogue; keys plan_mode and approval_continuation. |
| apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs; zdr_feature_table.rs; zdr_refusal_inventory.rs; tests/fixtures/zdr-posture-matrix.json (+ .sha256) | ?? | ZDR test matrices and fixtures. |
| apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs | ?? | ZDR refusal inventory. |
| apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql; 0045_zdr_waived_runs_org_index.sql; migrations/revert/ | ?? | Retention receipt and waived-run index; not plan-related. |
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | ?? | Retention receipts (not plan-related). |
| apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (+ test); contracts/ | ?? | ZDR refusal typing; imported by chat-client.ts (isZdrCode). |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.zdr-refusal.test.ts; http.zdr-refusal.test.ts; src/shared/i18n/errors.zdr-refusal.test.ts | ?? | ZDR tests. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.zdr-refusal.test.ts; chat-normalizers.zdr-refusal.test.ts | ?? | ZDR tests. |
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | M (+190/-47) | ZDR helper moved to zdr_posture; continuation_gate added to approve, reject, todo, decide, cancel, resume and plan_approval. Grant quarantine block unchanged. |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | M (+814/-69) | ZDR refusal streams (zdr_error_stream, prepared_zdr_refusal_stream) and ChatEvent::ZdrError. Approved-plan block shifted +360; body not confirmed unchanged (hunks beyond the first 30 not listed). |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | M (+202/-1) | Adds ZdrError variant and mapping (not read in full). |
| apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs | M (+58/-226) | Refusal helpers moved to mp-contracts zdr_refusal and zdr_posture. |
| apps/Model Plane/rust/services/model-gateway/src/state.rs | M (+5/-1) | Unrelated: cross-encoder boot check. AppState.plan_mode unchanged (256, 446). |
| apps/Model Plane/rust/services/model-gateway/src/grpc.rs | M (+31/-16) | Decide path; quarantine call moved 1541 to 1550 (not read otherwise). |
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | M (+370/-55) | Web-search gate and dispatch changes (hunk headers only). |
| apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs | M (+102) | Adds ZDR plan-gate tests and user_tokens_with_retention helper. |
| apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | M (+271/-17) | Adds ZDR refusal tests, including plan-mode refusal. |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | M (+200/-7) | run_agent wiring (570-1932), continuation_descriptor (4566), new tests (7185+). pause_for_approval body unchanged. |
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | M (+76/-22) | Not read. |
| apps/Model Plane/rust/services/session-core/src/grpc.rs | M (+817/-44) | Import lines changed (ZDR refusal, authorize helpers); other hunks not read. No hunk in 4800-5100 (set_run_mode unchanged). |
| apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs | M (+243/-6) | Not read. |
| apps/Model Plane/rust/services/session-core/src/orchestration_store.rs | M (+7/-1) | Not read. |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M (+42) | Adds SessionRetentionService.CountWaivedRuns (Z-21a). SetRunMode untouched. |
| apps/Model Plane/docs/sse-event-taxonomy.md | M (+4) | Rows: error to ZdrError; plan_proposed; retrieval_notice; source_scope_applied. |
| apps/Model Plane/docs/ZDR.md | M (+63/-1) | Not read. |
| Unchanged in LIVE (clean): model-gateway src/coordinator.rs, src/approvals.rs, src/queued_input.rs; mp-contracts src/autonomy.rs; proto runs.proto, orchestration.proto, execution.proto | clean | none |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | M (+53) | ZDR isZdrCode import and typed refusal handling. approvePlan body (1858-1880) not in hunks. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | M (+4) | Doc comment on zero-retention refusal copy and a feature field (3283-3295). approveTurnPlan not in hunks. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx | M (+12/-1) | Work-period picker wiring (props at 789-801). QueuedInputStrip and dock unchanged apart from the +11 shift. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | M (+185/-44) | ErrorNotice and EmptyChatState edits. ApprovalRequests, QueuedInputStrip and PlanApprovalControl unchanged, shifted +2. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts | M (+5/-2) | Not read. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts | M (+39) | Not read (ZDR normalizers likely). |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.failure-actions.test.tsx | M (+34) | Not read. |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx | M (+7) | Work-period picker props (ChatWorkPeriodPicker). |

Deleted (`D`): none relevant to this item.

## 8. Reconciliation with sibling F-t1-026 (Space) and dependencies

- Dependencies: WS02 sweep (F-t3-001, F-t3-022) = `.plan-work/sweeps/WS02-run-context-cache.md`. WS03 (F-t3-002) = `WS03-durable-loop.md`. WS04 (F-t3-016) = `WS04-replica-safe-run-control.md`. F-t3-015 has no sweep in this pass.
- F-t1-026 (Space audit `01-verevon-space-audit.md` L764-776; rows L1529 and L1789): typed card vocabulary (form, choice, link nodes) in the shared/chat-nodes ConversationNode union. request_input is an END-TURN tool; the submit is posted as a typed user turn that starts a NEW run. Secret drop through a broker. Owner-only answers. Nothing stored for ZDR threads. Depends on F-t1-010 approval resume (Space audit L1650, L1676).
- Conflicts to resolve (this sweep does not decide them):
  - a. Mechanism. F-t3-011: ask_user is a new kind in the existing durable PAUSE, and the same run resumes. F-t1-026: request_input is an end-turn tool, and the answer starts a new run. These touch agent.rs pause and resume differently.
  - b. Vocabulary. F-t3-011: a `user_question` event and card with single or multi select, free text and skip. F-t1-026: typed form and choice nodes in chat-nodes plus request_input. One union and one event should carry both.
  - c. Answer storage. Reusing the approval table inherits the granted-approval quarantine (approvals.rs:265-273 keys on STATUS_APPROVED). A question answer must not be stored as an approval grant.
  - d. ZDR. F-t3-011 allows "an in-stream, non-durable ask or none" for ZDR threads. F-t1-026 stores nothing for ZDR threads. These are compatible. The F-t3-011 gate itself (zdr_posture.rs) is LIVE-ONLY.
  - e. Surfaces. F-t3-011 docks trays above the ChatPage composer. F-t1-026 renders cards in the Space room timeline (SpaceRoomTimeline.tsx). Both can share the card component.

## 9. NOT DETERMINED

- Whether committed HEAD lets ZDR credentials approve plans end to end. plan_approval and approve_plan have no gateway refusal. session-core set_run_mode has no ZDR check (4984-5075). Whether session_flow::require_durable_run_owner_with_token refuses ZDR was not read.
- Whether a granted tool approval resumes end to end through execution-core. resume_run (grpc.rs:1068) was traced only to the approval fetch (1068-1092). The Space audit reports three idempotent action kinds. Not built or run.
- LIVE hunks in orchestration_grpc.rs, orchestration_store.rs, execution-core grpc.rs, model-gateway grpc.rs, tool_loop.rs, sse_events.rs, chat-types.ts, chat-normalizers.ts, ChatMessages.failure-actions.test.tsx, DashboardComposer.tsx and ZDR.md: stats only, not read. The sse.rs approved-plan body is not confirmed unchanged beyond the first 30 hunk headers.
- Canonical owner of approval state: master matrix line 129 (orchestrator-core, Go) versus capability row 55 (session-core). Not resolved. Go services were not inspected.
- Owner decision 17 ruling text (cited in live code comments) was not located.
- Which tool loop would host an ask_user builtin (dispatch_tool or execute_step_inner) is not decided by any doc read.
- Whether the sse-event-taxonomy CI gate covers new events. The taxonomy file was checked; the CI gate was not traced.
- Whether ChatPage tray docking conflicts with F-t3-015 sidebar and PWA work. Not assessed.
- apps/Model Plane/MODEL_PLANE_STATUS.md was not read; root CLAUDE.md says to read it before trusting a "done" claim.
- Build and test status: nothing was run. LIVE-only tests cannot be executed from WT.

## 10. Method

- Card read at audit L501-515. All F-t3-011 mentions in the audit and evidence pack were grepped. Verification record entry read (t3-v03). Appendix C read (L2008-2016). No Appendix D line.
- Citations resolved by Grep and Read in WT and LIVE. LIVE shifts were measured from `git diff -U0` hunk headers and by grepping the same symbols in LIVE.
- Git used only as `--no-optional-locks` status, diff and log. No stash, commit, reset, checkout or merge.
