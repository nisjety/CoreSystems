# WS04 replica-safe run control: code-map sweep (F-t3-016)

Read-only sweep, 2026-10-14. Worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` = HEAD afb83928, clean (verdict basis). Live `C:/dev/CoresSystem` = same HEAD plus uncommitted work (status from `.plan-work/live-tree-status.txt` and `git --no-optional-locks`). `live:` marks a line that differs in the live tree. Audit input: `03-model-plane-chat-and-agents-audit.md` in `scratch-2026-10-12-69a4ee`.

## 0. Summary

- F-t3-016: Recommended (was Priority), effort M (was L), wave W1, confidence Medium. Remaining slice: shared cancel, queued input, plan-mode and turn state across replicas, durable send receipts, editable queue. Card line 1252 says drop the editable queue and split send button unless the owner overrides the documented ruling (ledger 1639).
- All four path citations in the card MATCH the worktree. Those files are clean in live, so there is no drift on the card's own citations.
- Drift is in modified live files: cited symbols moved +2 to +142 lines (section 2b). Symbol presence confirmed; hunk content not reviewed.
- ABSENT: request_id-keyed NATS cancel/queue subject, editable queue, split send button. The queue strip is read-only (ChatMessages.tsx:2103-2128).
- Live-only and WS04-relevant: `model-gateway/src/zdr_posture.rs` (untracked) lists `invoke_queue_input` as a ZDR-guarded durable mutation (live:829).
- Not determined: whether ZDR turns skip the stream buffer (card says "memory-only"; compose:237-238 says the ZDR path does not buffer; the gate is not located at sse.rs:2168).

## 1. Audit context

| Where (audit doc) | Line(s) | Content |
|---|---|---|
| Card F-t3-016 | 1245-1255 | Effort M; Model Plane + Frontend Plane; W1; Confidence Medium. "Verevon today" 1252; dependencies 1254 |
| 9.1 wave table, W1 row | 1540 | recommended (was priority), M (was L), deps "Redis in every deploy, NATS mp.v1 subjects" |
| 9.1 dependents | 1535 (F-t3-002), 1561 (F-t3-011), 1562 (F-t3-012), 1565 (F-t3-015), 1621 (F-t3-064) | listed as dependency; F-t3-064 card repeats it at 1194 |
| 9.4 item 10 | 1705 | treat remaining M slice (shared cancel and queue state) as W1 work |
| 10.3 evidence limits | 1747 | Dragonfly/Redis presence in every deployment is an operator fact to check live |
| Appendix B index | 1886 | Recommended (was Priority); partial; M; W1 |
| Appendix C | 2049-2054 | reference check partial; no reference solves multi-replica run control |
| Appendix D | 2117 (tier), 2125 (effort) | Priority to Recommended; L to M |
| Reference-side rows citing F-t3-016 | 161, 295, 1388 | steering/queueing row; idempotent send receipts row; T3 Code note |

## 2. Citation verification

### 2a. Card citations and claims

| card id | cited path:lines | claim | worktree verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| F-t3-016 | apps/Model Plane/rust/services/model-gateway/src/cancel_registry.rs:1-40 | per-process cancel flags; promote-to-NATS note | MATCH: note 7-9; CancelRegistry 18; register 39; register_run 47 | clean | same |
| F-t3-016 | apps/Model Plane/rust/services/model-gateway/src/queued_input.rs:1-80 | per-process queued input; promote note | MATCH: note 25-32; pause text 47; bounds 62, 66; EnqueueOutcome 68-96 | clean | same |
| F-t3-016 | apps/Model Plane/rust/services/model-gateway/src/idempotency_registry.rs:1-40 | per-process duplicate-send cache | MATCH: single-replica note 14-16; scoped key 18-30, fn 52 | clean | same |
| F-t3-016 | apps/Model Plane/deploy/docker-compose.yml:226-241 | Redis stream buffer in shipped compose block | MATCH: comment 226-240; REDIS_URL 241 | clean | same |
| F-t3-016 | no path ("120 s drain") | drain window exists | MATCH: drain.rs:46 (DEFAULT_DRAIN_SECS = 120); compose:247 default 120 | clean | apps/Model Plane/rust/services/model-gateway/src/drain.rs:46 |
| F-t3-016 | no path ("unique (org_id, idempotency_key) on events") | DB-level dedupe | MATCH: apps/Model Plane/rust/services/session-core/migrations/0002_events_and_ordinals.sql:35-38; runtime ON CONFLICT session-core/src/grpc.rs:3231, 3303 (worktree) | migrations clean; grpc.rs M | grpc.rs live:3373, 3445 |
| F-t3-016 | no path ("only an enqueue route") | enqueue route exists | MATCH: route http_routes.rs:142; handler 7308 | M | route unchanged; handler live:7416 |
| F-t3-016 | no path ("read-only queue strip") | UI strip | MATCH: ChatMessages.tsx:2103-2128; state labels only, no controls | M | live:2105 |
| F-t3-016 | no path ("chat-turn map") | per-replica turn record | MATCH: chat_turn_registry.rs:22-23 (in-process, per-replica), 45 (TURN_TTL) | M (+36, below line 45) | same for 22-23, 45 |
| F-t3-016 scope | apps/Model Plane/rust/services/model-gateway/src/coordinator.rs:61-101 | PlanModeStore, in-memory | MATCH: PlanModeEntry 61; PlanModeStore 68-69; impl 101 | clean | same |
| F-t3-016 scope | coordinator.rs:401 | TeamWorkerStore (team-worker state) | MATCH: struct 401 | clean | same |
| F-t3-016 | apps/Model Plane/docs/decisions/ledger.md:1639-1661 | "documented ruling": classify mid-run input at round boundary | MATCH: 2026-08-24 entry | clean | same |
| F-t3-016 | no path ("no model-gateway replicas configured") | deploy shape | MATCH: no `replicas` key under apps/Model Plane/deploy | clean | n/a |
| F-t3-016 | no path ("Redis/Dragonfly in every deploy") | dependency | PARTIAL: Dragonfly service at deploy/docker-compose.yml:113-127; docker-compose.release.yml:14; docker-compose.production.yml:35; other deploy trees not checked | clean | n/a |
| F-t3-016 | no path ("cache-mode at 256 MB") | eviction risk | MATCH: docker-compose.yml:117-118 (--cache_mode=true, --maxmemory=256mb) | clean | same |
| F-t3-016 | no path ("10 min" chat frame TTL, compose:237-238) | buffer TTL | MATCH: stream_buffer.rs:33-34 (STREAM_TTL 600 s) | clean | same |
| F-t3-016 | no path ("ZDR turns stay memory-only") | ZDR scope of buffer | NOT DETERMINED: compose:237-238 says ZDR path does not buffer; buffering call sse.rs:2168 has no ZDR gate in 2160-2171 | sse.rs M | n/a |
| F-t3-016 | no path ("request_id-keyed NATS subject") | target subject | ABSENT: mp-events/src/subjects.rs has PREFIX 62, run events 72, session commands 78; no cancel, queue or invocation subject | clean | n/a |
| F-t3-016 | no path ("editable queue, split send button") | queue UX | ABSENT: no edit or split-send control; QueuedInputStrip is read-only (ChatMessages.tsx:2103-2128) | ChatMessages.tsx M | n/a |

### 2b. Symbol drift in live modified files

| symbol | worktree | live | delta | file status |
|---|---|---|---|---|
| model-gateway routes `/v1/invoke/:request_id/cancel`, `/queue` (http_routes.rs:140, 142) | 140, 142 | 140, 142 | 0 | M |
| http_routes.rs `cancel_run` (durable cancel) fn | 1602 | 1606 | +4 | M |
| http_routes.rs durable cancel calls `cancels.cancel_run_for` | 1655 | 1660 | +5 | M |
| http_routes.rs `invoke_cancel` fn (cancel_for call 6882-6883) | 6873 | 6979 | +106 | M |
| http_routes.rs `invoke_queue_input` fn (enqueue_for call 7351) | 7308 | 7416 | +108 | M |
| http_routes.rs tests: durable cancel publish; owner live-stream stop; outcome log | 10599; 10635; 10725 | 10740; 10776; 10866 | +141 | M |
| sse.rs cancel registration `register_run` and `queued_inputs.register` | 1978, 1983 | 2088, 2093 | +110 | M |
| sse.rs `invoke_stream_sse` / `invoke_resume_sse` fns | 835 / 7196 | not re-located | n/a | M |
| tool_loop.rs round-boundary drain `queued_inputs.drain` | 6528 | 6635 | +107 | M |
| tool_loop.rs "Emitted at DELIVERY" comment before QueuedInput event | 6547 | 6654 | +107 | M |
| sse_events.rs `ChatEvent::QueuedInput` variant | 318 | 360 | +42 | M |
| sse_events.rs `"queued_input"` wire name arm | 417 | 479 | +62 | M |
| state.rs AppState fields cancels, queued_inputs, idempotency, plan_mode, team_workers, stream_buffers | 239, 243, 247, 256, 259, 304 | same | 0 | M |
| session-core/src/grpc.rs events INSERT ON CONFLICT (org_id, idempotency_key) | 3231, 3303 | 3373, 3445 | +142 | M |
| sessions.proto AppendMessageRequest / its idempotency_key = 16 | 276 / 303 | 287 / 314 | +11 | M |
| sessions.proto durable-cancel comment | 185 | 185 | 0 | M |
| orchestration_http_test.rs foreign-cancel assertion | 1537 | 1537 | 0 | M |
| chat-client.ts cancelInvocation / QueuedInputResult / queueInvocationInput | 1807 / 1924 / 1947 | 1860 / 1977 / 2000 | +53 | M |
| ChatPage.tsx submitting / allowMidRunSubmit / QueuedInputStrip render | 243 / 244 / 881 | 252 / 253 / 892 | +9 / +9 / +11 | M |
| ChatMessages.tsx QueuedInputStrip | 2103 | 2105 | +2 | M |
| chat-types.ts QueuedInput type / ChatState.queuedInputs | 480 / 502 | 483 / 505 | +3 | M |
| DashboardComposer.tsx allowMidRunSubmit prop / submit guards | 883 / 1405, 2380 | 887 / 1409, 2384 | +4 | M |
| sse-event-taxonomy.md `queued_input` row | 29 | 31 | +2 | M |
| use-chat-controller.ts queueInvocationInput import / queuedInputs init | 79 / 247 | 79 / 247 | 0 | M |
| zdr_posture.rs DURABLE_MUTATIONS entry `invoke_queue_input` | none | 829 (test 825-826) | new | untracked, live-only |

## 3. As-built flow (worktree lines unless marked live)

- Stop: use-chat-controller.ts:2501, 2952 call `cancelInvocation` (chat-client.ts:1807) -> BFF `POST /api/v1/chat/invocations/{id}/cancel` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat.rs:36-37) -> json_handlers.rs:153-166 -> model-gateway `POST /v1/invoke/:id/cancel` (http_routes.rs:140) -> `invoke_cancel` (6873) -> `cancels.cancel_for` (cancel_registry.rs:92). Stream loops poll `is_cancelled` (sse.rs:6485, 6641; tool_loop.rs:6687, 6987, 7060) and end with a terminal `stopped` (sse-event-taxonomy.md:33).
- Durable cancel: `POST /v1/orchestration/runs/:run_id/cancel` (http_routes.rs:316) -> `cancel_run` (1602) -> session-core `RunService.CancelRun` (run_service_grpc.rs:758; `RUN_CANCELLED` inserted in the same transaction at 854; the event id is the receipt, per http_routes.rs:1587-1588) -> `cancels.cancel_run_for` (1655; live:1660) stops the owner's live stream for that run. BFF: orchestration.rs:112, 557.
- Queue: composer during a stream (ChatPage.tsx:243-253 `submitting` plus `allowMidRunSubmit`; DashboardComposer.tsx:1405, 2380 bypass the streaming lock when the prop is set) -> use-chat-controller.ts:2260 `deliverMidRun` (defined 2202) -> `queueInvocationInput` (use-chat-controller.ts:2221; chat-client.ts:1947) -> BFF `POST .../queue` (chat.rs:42; json_handlers.rs:176-220; the BFF mints a fresh `thread:append` decision from Control, json_handlers.rs:172-175) -> model-gateway `POST /v1/invoke/:id/queue` (http_routes.rs:142 -> `invoke_queue_input` 7308). Thread comes from the stream registration (sse.rs:1983), not the client; thread mismatch returns 409 (http_routes.rs:7336-7344); per the handler doc, the message is persisted to the thread before it is queued (7289-7303; the write call itself not traced) -> `enqueue_for` (queued_input.rs:163) -> round-boundary `drain` (tool_loop.rs:6528) -> `delivery_messages` plus `ChatEvent::QueuedInput` (sse_events.rs:318) -> SSE `queued_input` (sse-event-taxonomy.md:29; chat-client.ts:1693) -> `QueuedInputStrip` (ChatMessages.tsx:2103).
- Idempotency: `/v1/invoke` claims the client key (sse.rs:1180 `state.idempotency.claim`) -> idempotency_registry.rs:161 (scoped key 52; commit 243). Durable event dedupe is in session-core (migrations/0002:35-38; grpc.rs:3231, 3303).
- Resume buffer: stream_buffer.rs picks Redis when REDIS_URL is set, else in-process (9-14). Route `GET /v1/invoke/resume/:request_id` (http_routes.rs:138 -> sse.rs:7196; buffer_key 7217).
- Plan mode: `PlanModeStore` (coordinator.rs:68-101; in-memory; default TTL 3600 s at 48; grants keyed by thread, 71-85). Reached via gRPC EnterPlanMode/ExitPlanMode/IsPlanMode (gateway.proto:71-78) and HTTP `/v1/runs/:run_id/plan-approval` (http_routes.rs:214). Durable run mode is session-core `SetRunMode` (sessions.proto:141).
- Team workers: `TeamWorkerStore` (coordinator.rs:401; in-memory; handle_team_* 508, 535, 569; gateway.proto:83-91).
- Turn state: `ChatTurnRegistry` is an in-process, TTL-bounded map, per-replica (chat_turn_registry.rs:22-23; TURN_TTL 45 = 24 h). Live also has `global()` (live:175) and `PreviousTurnRegistry` (live:260).
- Drain: drain.rs (env `MODEL_GATEWAY_DRAIN_SECS` at 42; default 120 at 46; waits on `cancels.active()` at 134, 148); main.rs:43 wires cancels; compose stop_grace_period 150s (deploy/docker-compose.yml:200).

## 4. Affected-file map

Paths relative to `apps/Model Plane/rust/services/` unless noted. "live" in lines column marks a drifted line.

| path | role | lines (worktree; live if drifted) | live status | existing tests |
|---|---|---|---|---|
| model-gateway/src/cancel_registry.rs | per-request cancel flags, owner-scoped; durable-run fan-out | 16-139 (cancel_run_for 76, cancel_for 92, finish 112, is_live_for 126, is_cancelled 133) | clean | unit 149-211: register_then_cancel_sets_the_flag; cancel_unknown_request_returns_false; cancel_rejects_a_different_tenant_or_user; liveness_is_scoped_to_its_owner_and_ends_with_the_stream; a_durable_run_cancel_reaches_only_its_owners_stream; finish_stops_tracking_so_later_cancel_is_a_noop |
| model-gateway/src/queued_input.rs | mid-run queue; bounds 3 messages (66) and 2000 chars (62); pause text 47; enqueue 163; drain 214; delivery 244 | 47-244 | clean | 12 tests 277-463 (queued input keeps stream thread and scopes; foreign run reported; bounds; drain; tenant-bound thread lookup; finish idempotent; delivery order; pause text) |
| model-gateway/src/idempotency_registry.rs | /v1/invoke duplicate guard; key scoped to org and user | 52-243 (claim 161, commit 243) | clean | 11 tests 310-473 (first claim caches; in-flight duplicate; released on drop; cross-org key; separator forging; bounds) |
| model-gateway/src/chat_turn_registry.rs | per-turn record for ratings and skill attribution; PreviousTurnRegistry | 22-45 (live also 61, 79, 175, 220, 260) | M (+36) | 6 tests 419-475 |
| model-gateway/src/coordinator.rs | PlanModeStore 68-101, grants 85-99, enter 109, is_plan_mode 135, record_grant 166, take_approved_plan 197, granted_rung 208; handle_enter/exit/is_plan_mode 235, 298, 378; TeamWorkerStore 401; handle_team_* 508, 535, 569 | clean | plan-mode tests 612-659 (incl. fixed run event subject 659); team tests 712-769; grant tests 800-899 |
| model-gateway/src/state.rs | AppState fields | cancels 239, queued_inputs 243, idempotency 247, plan_mode 256, team_workers 259, stream_buffers 304 | M (+5/-1) | none |
| model-gateway/src/http_routes.rs | route table, plan-approval, cancel and queue handlers | routes 137-142; plan-approval 214; orchestration cancel 316; tasks cancel 347; cancel_run 1602 (live 1606); invoke_cancel 6873 (live 6979); invoke_queue_input 7308 (live 7416); 409 thread mismatch 7336-7344 | M (+190/-47) | tests 10599, 10635, 10725 (live 10740, 10776, 10866): durable_http_run_owner_can_publish_cancel_and_feedback_events; a_durable_cancel_stops_the_owners_live_stream_for_the_run; cancel_run_logs_one_outcome_line_per_call |
| model-gateway/src/sse.rs | stream start, cancel and queue registration, resume | invoke_stream_sse 835; buffer_key 1160; idempotency claim 1180; cancel and queue registration 1973-1983 (live 2088-2093); finish 63-64; .buffering 2168; cancel polls 6485, 6641; producer_live 7230-7233; invoke_resume_sse 7196 | M (+814/-69) | not enumerated (e2e in tests/e2e_invoke_chain_test.rs) |
| model-gateway/src/sse_events.rs | ChatEvent::QueuedInput and wire name | ChatEvent::QueuedInput 318 (live 360); wire name 417 (live 479); enqueue doc 312 | M (+202/-1) | not enumerated |
| model-gateway/src/tool_loop.rs | round-boundary drain and delivery | round-boundary drain 6528 (live 6635); delivery and event 6547 (live 6654); cancel checks 6687, 6987, 7060 | M (+370/-55) | not enumerated |
| model-gateway/src/stream_buffer.rs | resumable delta buffer; Redis or in-process by REDIS_URL | (9-14); STREAM_TTL 33-34; prefix 36 | clean | not enumerated |
| model-gateway/src/drain.rs | drain window, wait and deadline | drain window env 42; default 120 at 46; drain_window_from_env 70; wait_until_idle 103; drain 128 | clean | 202-389 (window parsing 202-226; wait and deadline 236-302; drain with stuck stream 341; trigger 355-389) |
| model-gateway/src/main.rs | drain wiring | AppState cancels clone at 43 (drain wiring) | M (+1) | none |
| model-gateway/src/lib.rs | module declarations for the registries | not captured | M (+3) | none |
| model-gateway/src/grpc.rs | plan-mode and team gRPC handlers | granted_rung comment 1365; plan-mode assertion 4251 | M (+31/-16) | in-file test near 4251 |
| model-gateway/src/session_flow.rs | idempotency_key plumbing | 144 | M (+5) | none |
| model-gateway/src/zdr_posture.rs | ZDR durable-mutation inventory naming invoke_queue_input | 825-837 (invoke_queue_input at live 829) | untracked, live-only | live test every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs |
| model-gateway/src/zdr_precheck.rs | ZDR precheck (adjacent) | not captured | M (+58/-226) | not checked |
| model-gateway/tests/e2e_invoke_chain_test.rs | invoke-chain e2e | grep finds no cancel or queue HTTP case; mock comment at 1061 mentions idempotency key | M (+271/-17) | this file |
| model-gateway/tests/orchestration_http_test.rs | run cancel owner and foreign tests; mock CancelRun counts | assertion 1537 | M (+102) | worktree bodies 1517-1537, 1580; live run_owner_state 888 |
| session-core/migrations/0002_events_and_ordinals.sql | unique index idx_events_idempotency_key on (org_id, idempotency_key) WHERE key <> '' | 35-38 | clean | session-core/tests/replay_deterministic.rs:318 dedup_drops_events_with_duplicate_idempotency_key (models the constraint; not DB) |
| session-core/migrations/0003_orchestration_tables.sql | approvals idempotency index | 16, 130-161 | clean | - |
| session-core/migrations/0029_scheduled_step_receipts.sql | UNIQUE (org_id, idempotency_key) on scheduled step receipts | 16, 26 | clean | - |
| session-core/src/grpc.rs | events INSERT with ON CONFLICT (org_id, idempotency_key) | 3231, 3303 (live 3373, 3445) | M (+817/-44) | in-file tests from ~7259 on (idempotency fixtures 8160, 8191) |
| session-core/src/run_service_grpc.rs | RunService.CancelRun; RUN_CANCELLED receipt | RunService.CancelRun 758; RUN_CANCELLED type URL 152; same-transaction insert 854 | clean | not captured |
| session-core/src/retention_receipt.rs | retention receipts (adjacent; not WS04 send receipts) | not captured | untracked, live-only | - |

Proto (apps/Model Plane/proto/model_plane/v1/):

| file | what and lines (worktree; live if drifted) | live status |
|---|---|---|
| sessions.proto | AppendMessage rpc 21; SetRunMode 141; durable-cancel comment 185; AppendMessageRequest 276 (live 287), idempotency_key 303 (live 314); AppendMessageResponse 313 | M (+42) |
| gateway.proto | EnterPlanMode 71; ExitPlanMode 75; IsPlanMode 78; TeamCreate 83; TeamDelete 87; TeamList 91 | clean |
| runs.proto | CancelRun rpc 54-55; CancelRunRequest 168; CancelRunResponse 176 with receipt_id 180-182 | clean |
| execution.proto | CancelRun rpc 20-21; resume and cancel comment 24-26 | clean |
| events.proto | EVENT_TYPE_RUN_CANCELLED = 93 at 124-126 | clean |
| buf.yaml, buf.gen.yaml, buf.gen.go-only.yaml | codegen config (proto dir) | clean |

Docs and gateway, frontend (apps/Frontend Plane/verevonv3 unless noted):

| path | role and lines (worktree; live if drifted) | live status | tests |
|---|---|---|---|
| apps/Model Plane/docs/sse-event-taxonomy.md | queued_input row 29 (live 31) to ChatEvent::QueuedInput; stopped 33 | M (+4) | - |
| apps/Model Plane/docs/decisions/ledger.md | ruling 1639-1661 (2026-08-24); scorecard 2371, item 5 at 2395-2398 ("client-side dead", now superseded by allowMidRunSubmit wiring); PlanModeStore grant notes 2611, 2664 | clean | - |
| apps/Model Plane/docs/capability-ownership-matrix.md | rows 19 (orchestration shell), 26 (plan mode, in-memory), 70 (TeamWorkerStore consolidate); section 4.1 at 85 | clean | - |
| apps/Model Plane/MODEL_PLANE_STATUS.md | grep finds no WS04-specific status line | clean | - |
| apps/gateway/src/domains/chat.rs | routes cancel 36-37, queue 42-43 | clean | - |
| apps/gateway/src/domains/chat/json_handlers.rs | cancel_invocation 153-166; queue_invocation_input 176-220; normalize_queue_response 228 | clean | tests 512-603 (ended_queue_race_is_a_successful_browser_protocol_response 551-552) |
| apps/gateway/src/domains/actions/handlers.rs | action dispatch chat.cancel_invocation 327; chat.queue_invocation_input 330 | clean | - |
| apps/gateway/src/domains/actions/dispatchers.rs | dispatch_chat_cancel_invocation 4578; dispatch_chat_queue_invocation_input 4600 | clean | - |
| apps/gateway/src/domains/orchestration.rs | run cancel route 112; proxy 557 | clean | line 613 test path |
| src/shared/actions/action-registry.ts | chat.cancel_invocation 2838; chat.queue_invocation_input 2849; schema chatQueueInvocationInput 1034; 2856 | clean | - |
| src/shared/api/chat-client.ts | ChatQueuedInputEvent 244; onQueuedInput 658; queued_input case 1693-1704; cancelInvocation 1807-1811 (live 1860); QueuedInputResult 1924-1937 (live 1977); queueInvocationInput 1947-1979 (live 2000) | M (+53) | - |
| src/shared/api/chat-client.test.ts | describe 'mid-run queued input' 297; it 308; queueInvocationInput cases 335-463 | M (+22) | this file |
| src/shared/api/sse.ts | not inspected | M (+2/-2) | - |
| src/shared/chat/verevon-ui-events.ts | input.queued mapping 34, 102, 142, 359 | clean | - |
| src/shared/api/ag-ui-client.ts | input.queued 186; 483-484 | clean | - |
| src/features/chat/components/ChatMessages.tsx | QueuedInputStrip 2103-2128, read-only (live 2105) | M (+185/-44) | ChatPanels.test.tsx 666-722 (clean) |
| src/features/chat/components/ChatPage.tsx | submitting 243; allowMidRunSubmit 244 (comment 245-248); strip render 881 (live 892) | M (+12/-1) | ChatPage.voice-first-turn.test.tsx (clean) |
| src/features/chat/components/use-chat-controller.ts | import 79; queuedInputs init 247; deliverMidRun 2202; queueInvocationInput call 2221; deliverMidRun call 2260; cancelInvocation calls 2501, 2952 | M (+4/-0) | follow-up 123, 146-174; resume-replay 144; retrieval-scope 26-42 (all clean) |
| src/features/chat/components/chat-types.ts | QueuedInput 480-490 (live 483); ChatState.queuedInputs 502 (live 505) | M (+5/-2) | - |
| src/features/chat/components/chat-normalizers.ts | ACTIVE_RUN_STATUSES 112 includes 'queued'; diff not inspected | M (+39/-0) | reconcile test (clean) |
| src/features/dashboard/home/DashboardComposer.tsx | allowMidRunSubmit prop 883 (live 887); guards 1405 (live 1409), 2380 (live 2384) | M (+7/-0) | DashboardComposer.mid-run.test.tsx (clean) |
| src/features/chat/components/mid-run-input.test.ts | source-text checks: guard routes to deliverMidRun 61-62; definition 75, 117; strip wiring 171-186 | clean | - |
| src/features/chat/components/ChatMessages.failure-actions.test.tsx | adjacent | M (+34/-0) | - |

## 5. Related contracts

- SQL migrations (apps/Model Plane/rust/services/session-core/migrations/): committed 0001_init.sql through 0043_audit_outbox_requeue.sql (43 files). Live adds untracked 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql and revert/0044_run_retention_receipt.down.sql, revert/0045_zdr_waived_runs_org_index.down.sql. Next number is 0044 on the committed tree but 0046 once the live pair lands, so a WS04 migration must be numbered after the live pair.
- Compose/env (apps/Model Plane/deploy/docker-compose.yml, model-gateway): REDIS_URL 241 (Dragonfly; the same URL capability-core uses, per 231-232); MODEL_GATEWAY_DRAIN_SECS 247 (default 120); stop_grace_period 150s at 200; NATS_URL 212, NATS_USER 213; MP_COMPAT_MODE 217. Dragonfly service 113-127 (cache_mode 117, maxmemory 256mb 118, requirepass 121). Other REDIS_URL consumers at 1389, 1588. Separate agent-memory Redis at 1672 (noeviction, 512 mb). Dragonfly also in docker-compose.release.yml:14 and docker-compose.production.yml:35.
- Redis keys: stream buffer prefix `mp:gw:stream:` (stream_buffer.rs:36), TTL 600 s (33).
- SSE events: ChatEvent::QueuedInput -> `queued_input` (sse_events.rs:417 worktree); ChatEvent::Stopped -> `stopped` (sse-event-taxonomy.md:33). Frontend: chat-client.ts:1693 handles `queued_input`; `input.queued` in verevon-ui-events.ts:359 and ag-ui-client.ts:186.
- Queue response contract: 202 `{queued, pending, persisted}` (chat-client.test.ts:437 mock; handler status code not traced); `{queued:false, resend_as_new_turn:true}` maps to `run_ended` (chat-client.ts:1969-1971); 409 `{queued:false, error}` on thread mismatch (http_routes.rs:7336-7344); refused reasons too_many, too_long, other (chat-client.ts:1932-1936).
- model-gateway HTTP routes (http_routes.rs): POST /v1/invoke/stream 137; GET /v1/invoke/resume/:request_id 138; POST /v1/invoke/:request_id/cancel 140; POST /v1/invoke/:request_id/queue 142; POST /v1/runs/:run_id/plan-approval 214; POST /v1/orchestration/runs/:run_id/cancel 316; POST /v1/tasks/:id/cancel 347.
- BFF routes: POST /api/v1/chat/invocations/{request_id}/cancel (chat.rs:36); POST /api/v1/chat/invocations/{request_id}/queue (chat.rs:42); POST /api/v1/orchestration/runs/{run_id}/cancel (orchestration.rs:112).
- Action contracts: `chat.cancel_invocation` (action-registry.ts:2838; dispatch handlers.rs:327 -> dispatchers.rs:4578); `chat.queue_invocation_input` (action-registry.ts:2849, schema 1034; dispatch handlers.rs:330 -> dispatchers.rs:4600).
- NATS: subject namespace `mp.v1` (mp-events/src/subjects.rs:62); `mp.v1.run.{run_id}.event` (72); `mp.v1.session.{session_key}.command` (78). No cancel, queue or invocation subject exists (ABSENT).
- Proto contracts: section 4 proto table (sessions, gateway, runs, execution, events).

## 6. Test and build commands

Rust, workspace `apps/Model Plane/rust/Cargo.toml` (members 3-19). Crate names: model-gateway (services/model-gateway/Cargo.toml:2), session-core (services/session-core/Cargo.toml:2), mp-events (crates/mp-events/Cargo.toml:2), mp-contracts (crates/mp-contracts/Cargo.toml:2), mp-ids.
- `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets` (Model Plane CLAUDE.md Testing rule)
- `cargo test -q -p session-core --all-targets`
- `cargo test -q -p mp-events -p mp-contracts --all-targets` when subjects or protos change
- CI: model-plane-rust-ci.yml:57 working directory apps/Model Plane/rust; :99 `cargo fmt --all -- --check`; :119 `cargo test --workspace --no-fail-fast`. CI file is M in live; quoted from worktree.
- Windows: user memory index says two cargo targets fail pre-existing on Windows and to use `--lib` (detail file not opened).
- Proto: `buf generate` in apps/Model Plane/proto (buf.gen.yaml). `buf lint` has pre-existing debt (Model Plane CLAUDE.md).
- Compose: `cd "apps/Model Plane" && ./scripts/compose.sh up -d` (scripts/compose.sh exists).
- Go: `go test ./...` and `go vet ./...` per service (Model Plane CLAUDE.md). No Go file on the cancel or queue path; relevant only if TeamWorkerStore consolidates into orchestrator-core.

Frontend SPA, `apps/Frontend Plane/verevonv3` (package.json is M in live; scripts quoted from worktree package.json):
- `pnpm lint` (eslint .); `pnpm typecheck` (tsc -b); `pnpm test` (node --test on gateway scripts, then `vitest run`); `pnpm test:e2e` (playwright). Focused: `pnpm vitest run src/shared/api/chat-client.test.ts src/features/chat/components/ChatPanels.test.tsx src/features/chat/components/mid-run-input.test.ts src/features/chat/components/use-chat-controller.follow-up.test.ts`.
- CI job "SPA - lint + typecheck + test + build" (.github/workflows/verevonv3-ci.yml:144-148; M in live).

BFF gateway, crate `verevon-gateway-rs` (apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml:2): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md). CI: verevonv3-ci.yml:113 working directory; :133 `cargo fmt --check`; :138 `cargo clippy --all-targets -- -D warnings`; :141 `cargo test`.

The user memory index notes pre-existing typecheck, lint and vitest failures in verevonv3. Not verified here.

## 7. Owners

- Model Plane owns sessions, runs, execution loops, inference and cost (root CLAUDE.md, Architecture Rules). Hot-path cancel, queue, idempotency and turn state live in model-gateway (Rust, master-ownership-matrix.md:125). Durable run and receipt state lives in session-core (Rust, :126).
- Control Plane owns sessions, audit, quotas and entitlements (root CLAUDE.md). The queue route's authority depends on the `thread:append` decision Control mints per message (json_handlers.rs:172-175).
- Frontend Plane owns the Verevon v3 UI and same-origin gateway/BFF normalization (root CLAUDE.md). The queue dock, chat-client and BFF routes sit here. master-ownership-matrix.md:20 says "App Shell presents"; App Shell is read here as Frontend Plane.
- Durable orchestration, policy and grants are Go (master-ownership-matrix.md:45). Capability matrix row 70 gives subagent lifecycle to orchestrator-core and calls the gateway TeamWorkerStore a cache to consolidate.
- Dragonfly is shared with capability-core (deploy compose:231-232), a Go service in Model Plane.
- One-owner rule: Model Plane CLAUDE.md "one canonical owner per capability" and capability-ownership-matrix.md row 26 (plan mode and approvals, in-memory gateway, EXIST) and section 4.1 (line 85: shadows, "promote to postgres later").
- Ledger first: Model Plane CLAUDE.md says check docs/decisions/ledger.md before merging or adding a registry. The 2026-08-24 queued-input ruling is at ledger.md:1639.
- Smallest abstraction: Model Plane CLAUDE.md says build the seam when the second real backend lands. The stream buffer already has two backends (Redis, in-process), but only for stream frames, not cancel or queue state.

## 8. Uncommitted-only (live tree)

Stat is `git --no-optional-locks diff --numstat` in C:/dev/CoresSystem. Where only stat was read, the one-line summary says so.

| file | live status | +/- | one-line summary |
|---|---|---|---|
| model-gateway/src/http_routes.rs | M | +190/-47 | cancel and queue handlers shifted +106 and +108; route lines unchanged (stat only otherwise) |
| model-gateway/src/sse.rs | M | +814/-69 | stream registration shifted +110; buffering and cancel polls present (stat only) |
| model-gateway/src/sse_events.rs | M | +202/-1 | QueuedInput variant shifted +42 (stat only) |
| model-gateway/src/tool_loop.rs | M | +370/-55 | round-boundary drain shifted +107 (stat only) |
| model-gateway/src/state.rs | M | +5/-1 | cited AppState fields unchanged |
| model-gateway/src/chat_turn_registry.rs | M | +36/-0 | additions below TURN_TTL; PreviousTurnRegistry and global() present (live) |
| model-gateway/src/grpc.rs | M | +31/-16 | plan-mode and team gRPC edits (stat only) |
| model-gateway/src/lib.rs | M | +3/-0 | module declarations (stat only) |
| model-gateway/src/main.rs | M | +1/-0 | one line added (stat only) |
| model-gateway/src/session_flow.rs | M | +5/-0 | idempotency_key plumbing (stat only) |
| model-gateway/src/zdr_precheck.rs | M | +58/-226 | adjacent ZDR precheck rewrite (stat only) |
| model-gateway/tests/e2e_invoke_chain_test.rs | M | +271/-17 | e2e changes (stat only) |
| model-gateway/tests/orchestration_http_test.rs | M | +102/-0 | run cancel tests added or extended (stat only) |
| session-core/src/grpc.rs | M | +817/-44 | ON CONFLICT lines shifted +142 (stat only) |
| proto/model_plane/v1/sessions.proto | M | +42/-0 | AppendMessageRequest shifted +11; durable-cancel comment unchanged |
| docs/sse-event-taxonomy.md | M | +4/-0 | queued_input row shifted +2 |
| docs/ZDR.md | M | +63/-1 | adjacent ZDR doc (stat only) |
| docs/core-research/model-gateway.md | M | +9/-0 | research notes (not inspected) |
| api/contracts/README.md | M | +1/-0 | adjacent (not inspected) |
| rust/crates/mp-contracts/src/lib.rs | M | +2/-0 | generated contract re-export (stat only) |
| session-core/migrations/0044_run_retention_receipt.sql | untracked | n/a | adjacent migration pair; numbering collision risk |
| session-core/migrations/0045_zdr_waived_runs_org_index.sql | untracked | n/a | adjacent migration pair |
| session-core/migrations/revert/0044_*.down.sql, 0045_*.down.sql | untracked | n/a | revert scripts for the pair |
| session-core/src/retention_receipt.rs | untracked | n/a | adjacent receipts code (not WS04 send receipts) |
| model-gateway/src/zdr_posture.rs | untracked | n/a | names invoke_queue_input as a guarded durable mutation (live:829) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | M | +185/-44 | QueuedInputStrip moved +2 (stat only) |
| .../components/ChatPage.tsx | M | +12/-1 | allowMidRunSubmit and strip wiring shifted +9 and +11 |
| .../components/chat-normalizers.ts | M | +39/-0 | not inspected |
| .../components/chat-types.ts | M | +5/-2 | QueuedInput type shifted +3 |
| .../components/use-chat-controller.ts | M | +4/-0 | additions after line 247 (stat only) |
| .../components/ChatMessages.failure-actions.test.tsx | M | +34/-0 | adjacent test (stat only) |
| .../src/shared/api/chat-client.ts | M | +53/-0 | cancelInvocation and queueInvocationInput shifted +53 |
| .../src/shared/api/chat-client.test.ts | M | +22/-0 | queue tests extended (stat only) |
| .../src/shared/api/sse.ts | M | +2/-2 | not inspected |
| .../src/features/dashboard/home/DashboardComposer.tsx | M | +7/-0 | allowMidRunSubmit guards shifted +4 |
| .../verevonv3/package.json | M | +2/-0 | test scripts source (stat only) |
| .github/workflows/model-plane-rust-ci.yml | M | +50/-0 | CI commands source (stat only) |
| .github/workflows/verevonv3-ci.yml | M | +55/-0 | CI commands source (stat only) |
| .../src/shared/api/contracts/zdr-refusals.json, .json.sha256 | untracked | n/a | adjacent ZDR refusal contract |

Committed and clean (WS04 core, no live change): cancel_registry.rs, queued_input.rs, idempotency_registry.rs, coordinator.rs, drain.rs, stream_buffer.rs, deploy/docker-compose.yml, migrations 0001-0043, gateway.proto, runs.proto, execution.proto, events.proto, ledger.md, capability-ownership-matrix.md, gateway chat and action files, action-registry.ts, ChatPanels.test.tsx, mid-run-input.test.ts, and the use-chat-controller follow-up, resume-replay and retrieval-scope tests.

## 9. Not determined

- ZDR turns and the stream buffer: card says memory-only; compose says the ZDR path does not buffer. The gate was not located at sse.rs:2168 (or in the 2160-2171 window). Check RichEventSink::buffering in sse_events.rs.
- Whether Dragonfly/Redis is present in every real deployment (audit 10.3, line 1747). The repo shows Dragonfly in three deploy files only. Other deploy targets were not inspected.
- Behaviour of live modified files beyond symbol presence. Live hunks were not read; stat-only rows are marked.
- Queued-input client path: the 2026-08-25 ledger finding (ledger.md:2395-2398) says the composer swallows submits while streaming. The worktree now passes `allowMidRunSubmit` (ChatPage.tsx:244; DashboardComposer.tsx:1405, 2380), so the finding looks superseded in code. No browser run was done; mid-run-input.test.ts is source-text based.
- Whether the queue handler writes the thread before enqueue (doc at http_routes.rs:7289-7303). The write call was not traced.
- Redis-backed StreamBufferStore: no test enumerated.
- Untracked src/shared/api/contracts/ (zdr-refusals.json) and any queue or cancel types in SPA generated contracts: not inspected.
- MODEL_PLANE_STATUS.md and docs/core-research/model-gateway.md (live M): no WS04 status line found by grep; not read in full.
- Test execution: none run (read-only sweep).
