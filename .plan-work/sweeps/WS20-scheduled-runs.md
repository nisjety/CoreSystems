# WS20-scheduled-runs: code-map sweep (audit F-t3-018)

Sweep date 2026-10-15. Read-only. Worktree = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). Live = `C:/dev/CoresSystem` (same HEAD plus the entries in `.plan-work/live-tree-status.txt`, 324 lines). The audit doc was written against the LIVE tree.

Verdict key: MATCH = behaviour present at the cited lines. DRIFT = present at other lines (new range given). ABSENT = not found. Live status: `clean` = not listed in live-tree-status.txt; `M` / `??` / `D` as listed. Numbers in [brackets] are live line numbers where they differ from the worktree. "wt" = worktree.

## 0. Findings to read first

1. Card citations: 5 explicit path:line citations. 4 MATCH in the worktree. 1 DRIFT in the worktree (execution-core grpc.rs: worktree 487-520, live 498-531). The card's numbers match the LIVE file.
2. Card line 610 says plan rows D3-D11 are "planned and unbuilt". Not accurate for D9: plan line 9 lists D9 as built and deployed, and the "Varsle meg" control is in code (ChatLiveRunPanel.tsx, ChatMessages.tsx). D2 and D12 are also built. No code found for D3-D8, D10, D11.
3. Wave-2 spec `chat-wave2-specs-2026-09-28/schedule.md` is stale: (a) it says the scheduled thread's origin is left blank; the code resolves a blank origin server-side (session-core grpc.rs:2202, 2314-2340) to `space` when space_id is set, otherwise `chat`; (b) its line refs drifted (prepare_scheduled_run_thread was 4123-4206, now starts at 4499 wt); (c) test `TestDispatch_ScheduledRun_RejectsNonDefaultWorkflowType` does not exist. The guard it describes is at capability-core `internal/taskexec/workflow_dispatcher.go:228-232`.
4. No ADR. "ADR unattended authority" (9.1 row, audit line 1566) does not exist: no file, no "unattended authority" text in apps/**/*.md, no 2026-09-28 entry in `apps/Model Plane/docs/decisions/ledger.md`. The draft-only ruling is recorded only in `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:68`.
5. No stored approval contract. Control decision struct `controlDecision` has coarse `Permissions []string` (cron:create, cron:fire, schedule:run) and no per-tool capability list (`control_authorizer.go:147-170`, field at 166). Grep of schedule_fire_decision.go, schedule_create_decision.go and scheduled_run_decision.go for tool/approv: no tool or approval field.
6. No non-delegable tool set in Model Plane, gateway or frontend (grep non-delegable: no matches).
7. Origin is per thread only. `threads.origin` CHECK (migration 0034): chat, space, agent_run, support, system; 0041 adds mail_task. No run-level origin and no schedule or trigger origin.
8. Off-surface delivery precedent is partial. capability-core `internal/notifyclient/client.go:75,146` already POSTs `/api/v1/notification-requests` to notification-core (run-watch notices, `internal/runwatch/consumer.go:3,16-17`). Master ownership rule 11 (`apps/master-ownership-matrix.md:167`) names Ingestion workers and the Frontend gateway as callers, not Model Plane. Ownership conflict to decide.
9. The approval outbox is not a human notification channel. `execution-core/src/approval_delivery_worker.rs` delivers to Orchestration Core (`OrchestrationCoreServiceClient`, lines 689-730 region).
10. Live-only dependency: the uncommitted ZDR refusal in execution-core `authorize_scheduled_step_service` (live grpc.rs:833-838) calls `mp_contracts::zdr_refusal::Refusal::feature_not_supported` (defined at `mp-contracts/src/zdr_refusal.rs:392`). That file is UNTRACKED (`??`). Committing the grpc.rs change without it breaks the build.
11. `apps/Model Plane/go/go.work` is tracked (`git ls-files`). The user memory note "no go.work" is stale for this repo.
12. Migration numbering collision risk: worktree latest session-core migration is 0043. Live adds untracked 0044 and 0045 (plus revert/ down files). A new migration is 0044 in the worktree and 0046 in live.
13. Audit doc: Appendix C line 2066 is truncated in the source text ("Origin attribu..."). Appendix D has no F-t3-018 entry (lines 2111-2162).

## 1. Card, plan and audit facts

| Field | Value (source) |
|---|---|
| Tier / effort / plane / wave / confidence | Priority / L / Model Plane, Control Plane, Application Plane / W2 / High (`03-model-plane-...-audit.md:600`) |
| Verevon today | Partial (line 600) |
| Card body | lines 596-610 (what 602, why 604, references 606, today 608, build 610) |
| 9.1 wave row | line 1566: deps F-t3-001, F-t3-002, F-t3-003, "ADR unattended authority" |
| Appendix B row | line 1835 |
| Appendix C | lines 2063-2070, reference-side check only (partial, high). No Verevon paths. |
| Appendix D | no entry |
| Dependencies | F-t3-001 (run context, sibling WS02), F-t3-002 (durable loop, WS03), F-t3-003 (declared tool classes, WS05), OD8, ADR (card line 610) |
| Sibling sweeps | `.plan-work/sweeps/WS02-run-context-cache.md`, `WS03-durable-loop.md`, `WS05-declared-tool-classes.md` |
| Owner ruling 2026-09-28 | `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:68` ("scheduled runs are draft-only"); policy-test ask at :666 |
| OD8 | plan line 452: run-length credential, 300 s default, up to 1500 s, hard cap 1800 s |
| OD9 | plan line 457: scheduled-run merge decision (apply D1, step authority, read scope, notification surface) |
| Audit design options | line 1687: "a Control-committed template or action schema carrying approved capabilities, an expiry and a ZDR refusal, re-verified at fire and per step; or a service-principal shortcut" (shortcut rejected: contract belongs in Control) |

## 2. Citation verification (card F-t3-018, plus path-less references)

| Card id | Cited path:lines | Claim (short) | Worktree verdict | Live status | Corrected path:lines |
|---|---|---|---|---|---|
| F-t3-018 | apps/Model Plane/go/services/orchestrator-core/cmd/workflows/interactive_run.go:180-260 | ScheduledRunSupervision workflow | MATCH (doc 183-187, func 188-259) | clean | 183-259 |
| F-t3-018 | apps/Model Plane/go/services/capability-core/internal/cron/control_authorizer.go:150-240 | Control create and fire decisions bind permissions, expiry, ZDR | MATCH for create (struct 147-170; VerifyScheduleCreate 201-228 with ZDR+expiry+cron:create at 214); fire check is outside the window at validateFireDecision 330-343 (ZDR 336) | clean | 147-170, 201-228, 230-273, 330-343; AuthorizeScheduledRun 51 (schedule:run 101-102) |
| F-t3-018 | apps/Model Plane/rust/services/execution-core/src/grpc.rs:498-530 | one tool-less completion (tool_choice none, max_tokens 1024) | DRIFT: fn scheduled_inference_request 487-520 (max_tokens 516, tool_choice 517); failure map 522-529 | M (98 lines). MATCH in live: fn 498-531 (max_tokens 527, tool_choice 528); failure map 533-540 | wt 487-529; live 498-540 |
| F-t3-018 | apps/Model Plane/go/services/orchestrator-core/cmd/workflows/interactive_run.go:243 | approval need fails the run | MATCH (guard 242; handleFailure "scheduled runs cannot continue without a separately authorized approval" at 243) | clean | 242-243 |
| F-t3-018 | apps/Frontend Plane/verevonv3/src/features/chat/components/ScheduledRunsPanel.tsx:125-145 (card short name) | "See results" disabled | MATCH: row meta 124-132; "See results" button 135-142, unconditional `disabled` at 137, title "Coming soon" at 138 | clean | 135-142 |
| F-t3-018 | migration 0034 (card gives no path) | thread origin is the only origin | MATCH: apps/Model Plane/rust/services/session-core/migrations/0034_thread_origin.sql (column 14-15, backfill 22-29, CHECK 36-38, space consistency 44-47). 0041 extends CHECK (17-21) | clean | 14-38 |
| F-t3-018 | migration 0029 (card gives no path) | scheduled_step_receipts ledger | MATCH: session-core/migrations/0029_scheduled_step_receipts.sql (table 5-27; status CHECK line 18; UNIQUE(org_id, idempotency_key) line 26) | clean | 5-27 |
| F-t3-018 | "supervision cancel signal" (no path) | cancel signal reuse | MATCH: interactive_run.go:55 (SignalCancel), 207-213 (cancel channel and goroutine), 224-226, 245-247 | clean | same |
| F-t3-018 | "approval outbox" (no path) | reuse | EXISTS: session-core/migrations/0014_approval_delivery_outbox.sql:1 (table); consumer execution-core/src/approval_delivery_worker.rs (delivers to Orchestration Core). Session-side claim/reap: session-core/src/approval_delivery.rs:501, 533 | clean | n/a |
| F-t3-018 | "notification-core" (no path) | off-surface delivery | EXISTS: apps/Application Plane/notification-core/internal/http/server.go:64 (delegated /api/v1 group), :65 (POST /notification-requests). README.md:8, 28 | clean | 64-65 |
| F-t3-018 | "ScheduleDialog" (no path) | reuse | EXISTS: apps/Frontend Plane/verevonv3/src/features/chat/components/ScheduleDialog.tsx | M (+24 lines) | n/a |
| F-t3-018 | "cron sweeper" (no path) | fires due schedules every minute | EXISTS: capability-core/internal/cron/sweeper.go: NewSweeper 46, Start 55, RunOnce 95, taskConfigJSON 194, fireOne 226 | clean | n/a |
| F-t3-018 | "schedule-from-chat" (no path) | create schedule from chat | EXISTS: apps/Frontend Plane/verevonv3/src/shared/api/schedule-from-chat.ts:30 (createScheduleFromChat), :36 (task_template) | clean | n/a |
| F-t3-018 | "Plan rows D3 to D11 ... planned and unbuilt" (CHAT_GAP_CLOSURE_PLAN_2026-10-01.md) | D3-D11 unbuilt | PARTLY WRONG. Rows: :115-120 (D1, D3, D5, D6, D9, D12), :155-157 (D4, D7, D8), :183-185 (D10, D11, D13). Status :9 lists D2, D9, D12 built. Code: "Varsle meg" in ChatLiveRunPanel.tsx and ChatMessages.tsx | plan doc clean | D9 built; D3-D8, D10, D11 no code found |
| F-t3-018 | "D2 digest parity" (plan row :91) | prerequisite | MATCH: row :91; status :9 built; test orchestrator-core/internal/orchestration/workflowreg_template_digest_test.go:25, :47 | clean | n/a |
| F-t3-018 | "owner ruling 2026-09-28 (draft-only)" | draft-only ruling | MATCH: plan :68; not in apps/Model Plane/docs/decisions/ledger.md | clean | :68 |
| F-t3-018 | "ADR amending the draft-only ruling" / "ADR unattended authority" | ADR | ABSENT (no file, no text in apps/**/*.md, no ledger entry) | n/a | n/a |
| F-t3-018 | "OD8 renewable delegated bearer" | run-length credential | MATCH: plan :452 | clean | :452 |
| F-t3-018 | "Control-committed template" (card :610) | template committed at create | PARTIAL: gateway apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/schedule_template.rs (header 1-2, digest 80, reserved key 33). Control create binds payload digest (user-core schedule_create_decision.go:45, 76). No per-tool approval field (controlDecision 147-170) | clean | n/a |

Card "Verevon today" claims, behaviour check:
- Cron sweeper, ScheduledRunSupervision workflow and schedule-from-chat exist: MATCH.
- Control create and per-fire decisions carry permissions, expiry and ZDR block: MATCH (control_authorizer.go:214, 336; user-core schedule_create_decision.go:86, schedule_fire_decision.go:99, scheduled_run_decision.go:126, scheduled_step_decision.go:166).
- Each step is one tool-less completion: MATCH (live grpc.rs:515 system prompt "Do not call tools", :528 tool_choice "none"). Test scheduled_step_runtime_is_tool_free_and_bounded_to_unknown_on_transport_loss (grpc.rs:1804 wt).
- Approval need fails the run: MATCH. See results disabled: MATCH. Only threads carry origin: MATCH.
- Reuse list (receipts ledger, cancel signal, approval outbox, notification-core, ScheduleDialog, ScheduledRunsPanel): all present.

Card "How to build it" net-new items, status:
- Per-tool approved-capability contract with expiry in the Control template: ABSENT.
- Non-delegable set (from F-t3-003): ABSENT in Model Plane, gateway and frontend.
- Off-surface approval through Application Plane delivery: PARTIAL (run-watch notices only; no approval notice found in capability-core notifyclient; catalogue not checked).
- Run-level origin (chat, schedule, trigger): ABSENT (threads only).
- Working "See results": ABSENT (disabled, ScheduledRunsPanel.tsx:137; results view not built; SpaceWorkPanel.tsx is the proposed target per wave-2 spec).

## 3. Affected-file map (depth 2)

Columns: path | role | lines (wt; live in brackets where different) | live status | tests (path:line, names where cheap).

### 3a. Model Plane, orchestrator-core (Go) - `apps/Model Plane/go/services/orchestrator-core/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| cmd/workflows/interactive_run.go | ScheduledRunInput (35-49); SignalApproval/SignalCancel (53-55); scheduledStepLane const (61-62); InteractiveRunSupervision approval wait (131-134); ScheduledRunSupervision (183-259); approval fail (242-243); cancel (207-213, 224-226, 245-247); StartScheduledRunActivity call (216-220); executeScheduledStepLoop switch (229-237) | 183-259 | clean | interactive_run_test.go:141 TestScheduledRun_UsesPreparedThreadAndCompletes; :179 TestScheduledRun_UnknownOutcomeStopsWithoutAnotherStep; :218 TestScheduledRun_RetryKeepsTheSameStepIdempotencyTuple; SignalCancel sends :315, :478 |
| cmd/workflows/step_loop.go | executeScheduledStepLoop (93-136); ExecuteScheduledStepActivity call (116); unknown outcome non-retryable (120-126); stops on NeedsApproval (127-129) | 93-136 | clean | covered by interactive_run_test.go above |
| cmd/activities/activities.go | ExecuteScheduledStepActivity (78-85; def 83); StartScheduledRunActivity (521-566; Control reauthorize 540-547; StartScheduledRun RPC 548-560) | 521-566 | clean | integration_test.go:502 TestStartScheduledRunActivity_ExecutesOnlyWithPreparedExecutionIntent; :538 TestExecuteScheduledStepActivityForwardsExactIntentAndDecision; :581 ...StopsOnUnknownOutcome; :608 ...StopsOnFailedOutcome; :635 ...FailsClosedOnInvalidResponse; :662 TestStartScheduledRunActivity_FailsClosedWithoutSessionCoreOrExecutionAuthority; :676 ...RejectsSessionCoreReceiptMismatch; activities_test.go:160 TestExecuteScheduledStepActivityFailsClosedWithoutExecutionLane |
| cmd/activities/scheduled_step_authorizer.go | Control per-step authorizer; ScheduledStepExecutionIntent (28-46; type 31); verify (167); payload digest (199) | 28-200 | clean | scheduled_step_authorizer_test.go:45 TestControlScheduledStepAuthorizerBindsExactStep |
| cmd/activities/scheduled_run_authorizer.go | Control run-execution authorizer; ScheduledRunExecutionIntent (28-44; type 32); verify (166); digest (206) | 28-210 | clean | scheduled_run_authorizer_test.go:48 TestControlScheduledRunExecutionAuthorizerSendsExactWorkerIdentityAndBindsThread; :99 ...RejectsDifferentRunOrIdempotencyBindings |
| cmd/main.go | RegisterActivity StartScheduledRunActivity (152), ExecuteScheduledStepActivity (154); workflow registration ScheduledRunSupervision (381) | 152-154, 381 | clean | n/a |
| internal/orchestration/workflowreg.go | registry entry ScheduledRunSupervision (113-115, Policy PolicyScheduledRun at 115); PolicyScheduledRun def (29-33); buildScheduledRunInput (249); "requires" check (263) | 113-115, 249, 263 | clean | workflowreg_template_digest_test.go:25, :47; startworkflow_test.go:355 TestScheduledRunWorkflowAcceptsOnlyCapabilityCoreAndRetainsNonSecretBindings; :420; :452 TestScheduledRunInputRejectsDetachedTemplateAndForgedExecutionFields |
| internal/servicecred/ | scope minting; TestDefaultScopes_ExecutionCoreIsScheduledStepOnly | servicecred_test.go:349 | clean | — |

### 3b. Model Plane, capability-core (Go) - `apps/Model Plane/go/services/capability-core/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| internal/cron/control_authorizer.go | Control decision verifier; constants (21-26: model.cron.create, model.cron.fire, schema sha256:space-cron-create-v1 / space-cron-fire-v1); AuthorizeScheduledRun (51; schedule:run 101-102); controlDecision (147-170); VerifyScheduleCreate (201-228); AuthorizeFire (230-273; POST /api/v1/internal/spaces/schedule-fire-decision at 242); verify (275); validateFireDecision (330-343); hasPermission (345); payload digests (358, 394, 427) | 21-343 | clean | control_authorizer_test.go:94 TestControlFireAuthorizerAcceptsOnlyFreshSignedBoundDecision; :123 ...RejectsDecisionForAnotherFire; :146 TestControlDecisionVerifierAcceptsOnlyTemplateBoundScheduleCreate; :167 ...AcceptsOnlyMatchingPreparedRunAndPreparedThread; cmd/cron_authorizer_test.go:5, :11 |
| internal/cron/sweeper.go | fires due cron_schedules each minute; RunOnce (95); taskConfigJSON (194); fireOne (226) | 20-260 | clean | sweeper_test.go:110 TestSweeperFailsClosedWithoutFreshFireAuthorizer; :127 TestTaskConfigCarriesNonSecretFireIntentButNoDecisionToken; :145 TestMalformedTemplateCannotDropFreshFireIntent; :160 TestTemplateCannotOverrideSchedulerDerivedFireIntent |
| internal/cron/cron.go | cron parser and next-fire (Parse 32, Next 134, NextFrom 166) | - | clean | cron_test.go |
| internal/taskexec/workflow_dispatcher.go | DefaultWorkflowType (51); ScheduledRunWorkflowType (56); scheduled guard (228-232, error at 230, rewrite at 232); attachScheduledRunInput (233) | 51-233 | clean | workflow_dispatcher_test.go:99 TestWorkflowDispatchLoadFenceRefusesDeletedCronSchedule; :148 TestWorkflowDispatchReauthorizesOnlyBoundCronIntent; :184 TestScheduledRunRejectsDetachedTemplateBeforeControlReauthorization; :251 TestAttachScheduledRunInputCarriesOnlyNonSecretPreparationFacts; workflow_dispatcher_template_digest_test.go:42, :58; _integration_test.go:164 TestScheduleTemplateDigestSurvivesJSONB; executor_test.go:31 |
| internal/api/workplane_apis.go | cron CRUD handlers (not re-read in this sweep; F-t3-019 card cites 836-871) | - | clean | workplane_apis_template_digest_test.go:38 TestCreateScheduleHashesTheSharedVectorToTheBFFDigest; registry_mutation_test.go:227, :291; space_deletion_test.go:12 |
| internal/notifyclient/client.go | HMAC-signed POST to notification-core /api/v1/notification-requests (path const 75; Accept 146) | 75-150 | clean | not determined |
| internal/runwatch/consumer.go | run-watch consumer; notification-core calls per watcher (3, 16-17, 78) | - | clean | not determined |
| migrations/0012_run_watch_subscriptions.up.sql | run-watch subscriptions (line 8 references notification-requests) | - | clean | — |

### 3c. Model Plane, session-core (Rust) - `apps/Model Plane/rust/services/session-core/`

| Path | Role | Lines wt [live] | Live | Tests (wt lines) |
|---|---|---|---|---|
| src/grpc.rs | CONTROL_SCHEDULED_RUN_* consts 86-89 [88-91]; verify_scheduled_run_decision 766 [768]; verify_scheduled_run_execution_decision 994 [996]; validate_scheduled_step_request 1106 [1108]; claim_scheduled_step_inner 1142 [1144]; record_scheduled_step_receipt_inner 1280 [1282]; persist_scheduled_run_bindings 1378 [1380]; create_thread_inner_preverified 2143 [2145] (calls resolve_thread_origin at 2202); resolve_thread_origin 2314 [2316]; start_scheduled_run (gRPC) 4414 [4582]; prepare_scheduled_run_thread (gRPC) 4499 [4670] | scheduled bindings, receipts, origin | M (40 hunks; 6 added lines mention the scheduled lane) | scheduled_step_claim_shape_is_strict_and_digest_bound 7246; scheduled_step_claim_and_unknown_receipt_are_idempotent_against_real_pg 7276; scheduled_run_identifiers_are_nats_safe_and_never_path_like 7471; scheduled_run_preparation_accepts_a_fresh_retry_decision_only_for_the_same_fire 7485; scheduled_run_prepare_retry_with_fresh_decision_reuses_thread_against_real_pg 7587; scheduled_run_execution_decision_is_bound_to_the_exact_prepared_thread 7678; own_scheduled_thread_read_decision_issued_by_control_verifies 8538; origin mapping asserts 7831-7964 |
| src/auth.rs | SCHEDULED_STEP_AUTHORITY_SCOPE 257; SCHEDULED_STEP_SCOPE 262; SCHEDULED_STEP_SERVICE 263 (service:execution-core); scheduled-step checks 272-277, 312 | 257-312 | M (396 lines added) | not located |
| src/run_service_grpc.rs | authorize_operation(SCHEDULED_STEP_SCOPE) | 493 | M (21 lines) | not located |
| src/terminalization.rs | managed-run start and direct-run completion (relevant to plan row D5) | - | M (123 lines) | not located |
| src/approval_delivery.rs | approval outbox claim (501) and reap (533) | 501-533 | clean | - |
| migrations/*.sql | see section 5 | - | - | - |

### 3d. Model Plane, execution-core (Rust) - `apps/Model Plane/rust/services/execution-core/`

| Path | Role | Lines wt [live] | Live | Tests |
|---|---|---|---|---|
| src/grpc.rs | execute_scheduled_step_runtime 238 [249] (needs scheduled inference token, else failed_precondition); scheduled_inference_request 487 [498] (tool_choice none 517 [528]); scheduled_step_failure_status 522 [533]; validate_scheduled_step_bindings 762 [779]; authorize_scheduled_step_service [818] (live-only ZDR refusal 833-838); run_agent gRPC 1203 [1225]; execute_scheduled_step gRPC [1300]; enforce_persistence_free_execution [664] (live-only) | - | M (98 lines) | scheduled_step_contract_rejects_wildcards_and_requires_digests 1704; scheduled_step_service_gate_is_exact_and_non_zdr 1743; scheduled_step_runtime_is_tool_free_and_bounded_to_unknown_on_transport_loss 1804 |
| src/auth.rs | scheduled-step service identity | - | M (631 lines; 2 scheduled lines) | scheduled_step_auth_accepts_only_signed_service_identity 1301 |
| src/scheduled_step_decision.rs | Control per-step decision verification (not read) | - | clean | not determined |
| src/scheduled_inference_auth.rs | inference credential for scheduled steps (not read) | - | clean | not determined |
| src/runtime_loop/mod.rs | execute_step_inner (455): "deployed agent" tool loop (CLAUDE.md) | 455 | clean | - |
| src/runtime_loop/agent.rs | run_agent (567), run_agent_with_tools, run_rounds, continuation_descriptor (live hunks 749, 769, 1937, 4566) | 567 | M (207 lines; no scheduled lines) | agent.rs test module (live hunks at 7185-10702) |
| src/approval_delivery_worker.rs | approval outbox worker to Orchestration Core | - | clean | - |

### 3e. Model Plane, model-gateway (Rust) - `apps/Model Plane/rust/services/model-gateway/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| src/tool_loop.rs | dispatch_tool (2406): read-only router, the "plain chat" loop (CLAUDE.md says it must stay separate from execution-core) | 2406 | M (425 lines; no scheduled lines) | not located |
| src/sse_events.rs | SSE event mapping; grep schedul/cron/approval_required = 0 in live | - | M (203 lines) | not located |
| src/schedule_evidence.rs | matched "schedul" in grep; role not determined | - | clean | not determined |
| src/zdr_precheck.rs | ZDR precheck (adjacent) | - | M | not located |

### 3f. Control Plane, user-core (Go) - `apps/Control Plane/user-core/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| internal/spaces/schedule_create_decision.go | ScheduleCreateRequest (22); Validate (30); payload digest (45); IssueScheduleCreateDecision (76; ZDR 86; cron:create 95) | 22-95 | clean | schedule_create_decision_test.go:17 TestIssueScheduleCreateDecisionIsTemplateAndScheduleBound; :45; :51 (ZDR) |
| internal/spaces/schedule_fire_decision.go | ScheduleFireIntent (22); digest (51); IssueScheduleFireDecision (83; ZDR 99; cron:fire 114) | 22-114 | clean | schedule_fire_decision_test.go:17 TestIssueScheduleFireDecisionRefreshesCurrentAuthorityAndBindsOneFire; :49; :63 (ZDR) |
| internal/spaces/scheduled_run_decision.go | ScheduledRunIntent (23); SystemThreadKey (34); IssueScheduledRunDecision (89); IssueScheduledRunExecutionDecision (100); issueScheduledRunDecision (110; ZDR 126) | 23-126 | clean | scheduled_run_decision_test.go:21, :52, :65 |
| internal/spaces/scheduled_step_decision.go | action model.schedule.step (13); ScheduledStepIntent (20); ScheduledStepAuthority (76); digest (114); IssueScheduledStepDecision (151; ZDR 166) | 13-166 | clean | scheduled_step_decision_test.go:20, :52 |
| internal/http/server.go | spacesGroup (343); POST schedule-create-decision (363), schedule-fire-decision (370), scheduled-run-decision (372), scheduled-run-execution-decision (373), scheduled-step-decision (374) | 343-374 | clean | service_auth_test.go, space_membership_scope_test.go (names not read) |
| internal/clients/session_run_action_authority.go | matched schedul/cron grep; not read | - | clean | session_run_action_authority_test.go |

### 3g. Application Plane, notification-core (Go) - `apps/Application Plane/notification-core/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| internal/http/server.go | delegated /api/v1 group (64); POST /notification-requests (65) | 64-65 | clean | internal/http/server_test.go |
| README.md | intake contract | 8, 28 | clean | - |

### 3h. Frontend Plane, verevonv3 (TS/TSX) - `apps/Frontend Plane/verevonv3/`

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| src/features/chat/components/ScheduledRunsPanel.tsx | schedule rows (119-153); meta (124-132); "See results" disabled (135-142); pause/resume (143-150) | 119-153 | clean | ScheduledRunsPanel.test.tsx (clean) |
| src/features/chat/components/ScheduleDialog.tsx | create dialog; seedGoal; live adds Task textarea and goal gate | - | M (+24) | ScheduleDialog.test.tsx (M, +16) |
| src/features/chat/components/ChatLiveRunPanel.tsx | live run panel with "Varsle meg" (D9) | - | clean | ChatLiveRunPanel.test.tsx (clean) |
| src/features/chat/components/ChatMessages.tsx | run-notify and message actions | - | M (+229 / -) | ChatMessages.run-notify.test.tsx (clean); ChatMessages.failure-actions.test.tsx (M) |
| src/features/chat/components/ChatPage.tsx | composer submit wrapped with work period (live) | - | M (+13) | - |
| src/shared/api/cron-client.ts | BFF /api/v1/cron: list 59, create 83, get/patch 96, delete 104; task_template field 44 (unknown), 76 (CronTaskTemplate) | 44-104 | clean | cron-client.test.ts (clean) |
| src/shared/api/schedule-from-chat.ts | createScheduleFromChat 30; task_template 36; listMySchedules 58 | 30-58 | clean | schedule-from-chat.test.ts (clean) |
| src/features/settings/components/CronSchedulesSection.tsx | Settings schedule UI | - | clean | CronSchedulesSection.test.tsx (clean) |
| src/features/spaces/components/SpaceWorkPanel.tsx | proposed "See results" deep-link target (wave-2 spec) | - | clean | SpaceWorkPanel.test.tsx (clean) |
| src/shared/actions/action-registry.ts | schedule action schema (~872-913: pause, unpause, trigger, backfill, delete). Ingestion-schedule contract, not the Space cron path | 872-913 | clean | action-client.test.ts |

### 3i. Frontend Plane, gateway (Rust) - `apps/Frontend Plane/verevonv3/apps/gateway/` (crate `verevon-gateway-rs`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| src/domains/spaces.rs | inject_personal_schedule_create_context (426); schedule-create-decision call (539); checks action model.cron.create and sha256:space-cron-create-v1 (574-576); room schedules GET /v1/cron?space_ref= (2475) | 426-589, 2475 | clean | - |
| src/domains/spaces/schedule_template.rs | template digest (80), RESERVED_FIRE_INTENT_KEY (33), check_digest_stable (103); test vectors incl. run_route, run_notify, run_model (279) | 33-340 | clean | in-file: vector_digest_is_the_digest_go_computes (200); flat_templates_are_stable (274) |
| src/domains/chat/history.rs | read_durable_threads (335; origin=chat request 351); chat_history_sessions origin filter (189-192); blank origin excluded (184-185) | 181-404 | clean | in-file test near 896 |
| src/domains/run_watchers.rs | run-watch registration; no scheduled auto-watch (D7 not built) | - | clean | - |
| src/domains/agents_runs.rs, agent_actions.rs, ingestions/schedules.rs | matched cron/schedul grep; not read | - | clean | not determined |

## 4. Related contracts

Proto (`apps/Model Plane/proto/model_plane/v1/`):
- execution.proto: rpc ExecuteScheduledStep 40; comment 36-39 ("lane remains disabled until Session Core's current-run claim/receipt and Control's per-step decision are deployed"); ExecuteScheduledStepRequest 48-64 (includes control_decision_token 13); response 66-73. rpc RunAgent 34; RunAgentRequest 75-76; RunAgentResponse 211-212. Clean.
- sessions.proto: rpc StartScheduledRun 29; PrepareScheduledRunThread 34; ClaimScheduledStep 147; RecordScheduledStepReceipt 152; StartScheduledRunRequest 364-395 (13 fields, incl. template_digest 11, policy_digest 13); PrepareScheduledRunThreadRequest 398; PrepareScheduledRunThreadResponse 420; ClaimScheduledStepRequest 1171; Response 1185; RecordScheduledStepReceiptRequest 1191; Response 1203. Live diff adds rpc CountWaivedRuns (196-206) and 3 messages (1219-1249). M.
- runs.proto: rpc GetScheduledStepContext 49; ResolveScheduledStepAuthority 93; messages 101, 107, 238, 251. Clean.
- Generated: `apps/Model Plane/go/gen/model_plane/v1/sessions_grpc.pb.go`, `execution_grpc.pb.go`, `sessions.pb.go` (generated; not edited here).

Migrations (`apps/Model Plane/rust/services/session-core/migrations/`):
- Worktree latest: 0042_thread_persona_toggle.sql, 0043_audit_outbox_requeue.sql.
- Live adds untracked (`??`): 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, revert/0044_run_retention_receipt.down.sql, revert/0045_zdr_waived_runs_org_index.down.sql. Not read.
- Relevant existing: 0004_tasks_cron_hooks_skills_memory.sql (cron_schedules line 120, cron_fires 149); 0014_approval_delivery_outbox.sql (table line 1); 0018_approval_continuation_receipts.sql; 0019_approval_delivery_settlement.sql; 0028_space_scoped_cron.sql; 0029_scheduled_step_receipts.sql; 0034_thread_origin.sql; 0041_mail_task_thread_origin.sql (CHECK extension 17-21).
- capability-core migrations 0001-0015: no cron table by name.

Config and env (`apps/Model Plane/deploy/docker-compose.yml`, clean):
- EXECUTION_CORE_SCHEDULED_STEP_DECISION_KEY_ID and _PUBLIC_KEY_BASE64 (1135-1136).
- ORCHESTRATOR_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN (1361), ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN (1362).
- CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN (1436).
- Control side (DEV_AUTHORITY_HANDOFF_MATRIX.md:17-18): CONTROL_USER_CORE_URL, CONTROL_SPACE_DECISION_KEY_ID, CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64; scope session:runs:system-owner.
- Preflight and release: scripts/tests/dev-runtime-preflight.sh:105-118; scripts/release-artifact.sh:74-82; scripts/tests/scheduled-step-config-contract-test.sh (rg-based contract).

Scopes and permission strings:
- session:schedule-prepare (session-core grpc.rs:4504 wt); session:scheduled-step (auth.rs:262); session:scheduled-step-authority (auth.rs:257).
- Control permissions: cron:create (user-core schedule_create_decision.go:95), cron:fire (schedule_fire_decision.go:114), schedule:run (capability-core control_authorizer.go:102).

Control action IDs and schemas:

| Action | Schema | Where |
|---|---|---|
| model.cron.create | sha256:space-cron-create-v1 | capability-core control_authorizer.go:21, 23; gateway spaces.rs:574-576 |
| model.cron.fire | sha256:space-cron-fire-v1 | capability-core control_authorizer.go:24, 26 |
| model.schedule.run | sha256:space-scheduled-run-v1 | session-core grpc.rs:89-90 wt (live 89-90) |
| model.schedule.step | (Control) | user-core scheduled_step_decision.go:13 |

SSE events: no scheduled or approval event types. `docs/sse-event-taxonomy.md` (M, +4 rows unrelated) and model-gateway `sse_events.rs` (live count 0 for schedul/cron/approval_required). Plan-approval SSE only (PlanApprovalControl, sse_events.rs region 266, 372).

Gateway and internal routes:
- BFF: /api/v1/cron (GET 59, POST 83, GET/PATCH /{id} 96, DELETE /{id} 104) in cron-client.ts; to model-gateway /v1/cron.
- Control internal (prefix /api/v1/internal/spaces, capability-core control_authorizer.go:242): schedule-create-decision, schedule-fire-decision, scheduled-run-decision, scheduled-run-execution-decision, scheduled-step-decision (user-core server.go:363-374).
- notification-core: POST /api/v1/notification-requests (notification-core server.go:65); caller in capability-core notifyclient.

Frontend action contracts: `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts` ~872-913 holds schedule actions (pause_schedule, unpause_schedule, trigger_schedule, backfill_schedule, delete_schedule) for the ingestion schedule API (comment cites schedules.rs create_schedule). No Space-cron action contract found in src/shared/actions. Card F-t3-018 "action contract" requirement: NOT in place.

## 5. Test and build commands

- Scheduled-step proof (the four proofs in one script): `apps/Model Plane/scripts/tests/scheduled-step-proof.sh`. Runs from `apps/Model Plane/go`:
  - `go test -count=1 ./services/orchestrator-core/cmd/workflows -run 'TestScheduledRun_(UsesPreparedThreadAndCompletes|UnknownOutcomeStopsWithoutAnotherStep|RetryKeepsTheSameStepIdempotencyTuple)$'`
  - `go test -count=1 ./services/orchestrator-core/cmd/activities -run 'Test(ExecuteScheduledStepActivityForwardsExactIntentAndDecision|...|StartScheduledRunActivity_RejectsSessionCoreReceiptMismatch)$'`
  - `cargo test --manifest-path "apps/Model Plane/rust/Cargo.toml" -p execution-core scheduled --no-fail-fast`
  - `scripts/tests/scheduled-step-receipt-postgres-test.sh` (disposable Postgres).
- Config contract: `apps/Model Plane/scripts/tests/scheduled-step-config-contract-test.sh`.
- Rust (package names from Cargo.toml): session-core, execution-core, model-gateway, mp-contracts. Workspace: `apps/Model Plane/rust/Cargo.toml` line 1 `[workspace]`. Per `apps/Model Plane/CLAUDE.md` Testing: `cargo test -q -p <service> --all-targets`; `cargo test --workspace`.
- Go (module paths from go.mod): orchestrator-core = `github.com/triodelab/model-plane/services/orchestrator-core`; capability-core = `github.com/triodelab/model-plane/services/capability-core`. Run `go test ./...` and `go vet ./...` per service directory (CLAUDE.md). `apps/Model Plane/go/go.work` is tracked (see finding 11).
- Control Plane user-core (`apps/Control Plane/user-core/Makefile:44-46`): `make test` runs `go test -v ./...`; module `github.com/I-Dacosta/AquatiqCMS/apps/user-service-go`.
- Frontend (`apps/Frontend Plane/verevonv3/package.json`): `test` = node --test (gateway scripts) && `vitest run` (vitest ^4.1.10, line 53); `typecheck` = `tsc -b`; `lint` = `eslint .`. Targeted: ScheduleDialog.test.tsx, ScheduledRunsPanel.test.tsx, cron-client.test.ts, schedule-from-chat.test.ts, CronSchedulesSection.test.tsx, SpaceWorkPanel.test.tsx, ChatLiveRunPanel.test.tsx, ChatMessages.run-notify.test.tsx.
- Gateway crate: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md Build And Run).
- notification-core: `apps/Application Plane/notification-core/go.mod` present. Test command not verified (no Makefile read).
- Memory notes (unverified in this sweep): two cargo targets fail on Windows (use --lib); Go -race needs cgo; verevonv3 has pre-existing typecheck, lint and vitest failures.

## 6. Owners

| Area | Owner (plane / service) | Rule (cited) |
|---|---|---|
| Durable cron firing and workflow supervision | Model Plane / orchestrator-core (Temporal) | `apps/master-ownership-matrix.md:129` (orchestrator-core: tasks/cron, approvals, recovery); `apps/Model Plane/docs/capability-ownership-matrix.md:101` (durable cron = Temporal; durable task state = session-core; metadata/templates = capability-core; ingress = gateway) |
| Schedule records, template metadata, fire intake | Model Plane / capability-core | capability-ownership-matrix.md:101 (metadata/templates) |
| Session threads, receipts, origin | Model Plane / session-core | capability-ownership-matrix.md:55 (plans, approvals, subagent lineage in session-core) |
| Tool execution and scheduled step loop | Model Plane / execution-core | `apps/Model Plane/CLAUDE.md` Code Style bullet 2: execution-core execute_step_inner is the "deployed agent" loop; model-gateway dispatch_tool is read-only and refuses side effects; the two must stay separate |
| Schedule decisions and authority (create, fire, run, step) | Control Plane / user-core | root CLAUDE.md Architecture Rules (Control owns identity, users, orgs, billing, sessions, audit, quotas, entitlements); `master-ownership-matrix.md:165` rule 9 (identity and membership) |
| Approval authority for effectful calls | Conversation owns durable human intent or approved-AI action; Model Plane fails closed | `master-ownership-matrix.md:168` rule 12 ("Every other issuer, including Model Plane, fails closed until it implements an equivalent durable contract") |
| Notification intake and delivery | Application Plane / notification-core | `master-ownership-matrix.md:167` rule 11; line 12 (Application Plane owns the canonical notification request API and delivery ledgers). Conflict: capability-core also calls it (finding 8) |
| Projections only narrow access | Application Plane projections | `master-ownership-matrix.md:166` rule 10 |
| ZDR propagation | all planes | root CLAUDE.md ("Zero Data Retention must propagate through any content-persisting boundary"); `master-ownership-matrix.md:160` rule 4 |
| Chat UI and BFF normalization | Frontend Plane / verevonv3 and gateway | root CLAUDE.md (Frontend Plane owns Verevon v3 UI plus same-origin gateway/BFF normalization) |
| Scheduling in general | Go (durable workflow, scheduling) | `master-ownership-matrix.md:45` |

## 7. Uncommitted-only (live tree)

Scope: files relevant to F-t3-018 that are `M` or `??` in live, or that differ between live and worktree. Diff stats via `git --no-optional-locks diff --stat`.

| Path | Live status | Diff | One-line summary |
|---|---|---|---|
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | M | 98 lines | Adds ZDR refusal in authorize_scheduled_step_service (833-838, uses untracked zdr_refusal.rs), enforce_persistence_free_execution (660-674), and agent-route and permission-mode checks; shifts scheduled lines by +11 |
| apps/Model Plane/rust/services/execution-core/src/auth.rs | M | 631 lines | Large change; 2 scheduled-related added lines (not inspected) |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | M | 207 lines | Changes run_agent, run_agent_with_tools, run_rounds, continuation_descriptor; no scheduled lines |
| apps/Model Plane/rust/services/session-core/src/grpc.rs | M | 861 lines (40 hunks) | 6 added lines for the persistent scheduled lane (run-id guard); live line shifts +2 (consts) to +171 (prepare_scheduled_run_thread) for scheduled functions |
| apps/Model Plane/rust/services/session-core/src/auth.rs | M | 396 lines | Added auth paths; no scheduled lines |
| apps/Model Plane/rust/services/session-core/src/run_service_grpc.rs | M | 21 lines | Small run-service change; no scheduled lines |
| apps/Model Plane/rust/services/session-core/src/terminalization.rs | M | 123 lines | Changes managed-run start and direct-run completion; no scheduled lines |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M | +42 | Adds CountWaivedRuns RPC and 3 messages (ZDR waived-run count); not scheduled |
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | M | 425 lines | Read-only router loop changed; no scheduled lines |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | M | 203 lines | No scheduled or approval-required events |
| apps/Model Plane/docs/sse-event-taxonomy.md | M | +4 | Adds 4 rows (zdr error, plan_proposed, retrieval_notice, source_scope_applied) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ScheduleDialog.tsx | M | 24 lines | Adds required "Task" textarea when no seed goal; submit gated on goal; goalText = goal().trim() |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ScheduleDialog.test.tsx | M | +16 | Adds tests for the goal field (inferred from stat; not inspected) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx | M | 13 lines | Wraps composer submit with work-period payload; passes orgId and userId |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | M | 229 lines | Not scheduled (0 scheduled lines in diff) |
| chat-types.ts, use-chat-controller.ts, chat-normalizers.ts, chat-client.ts, notifications-client.ts (all under apps/Frontend Plane/verevonv3/src/) | M | 7, 4, 39, 53, 18 lines | 0 scheduled or cron lines in diff for chat-types, use-chat-controller, chat-client, notifications-client; chat-normalizers.ts not counted |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | ?? | untracked | Defines Refusal::feature_not_supported (392); used by live execution-core scheduled gate (finding 10) |
| apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql (+ revert/ down) | ?? | untracked | Not read |
| apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql (+ revert/ down) | ?? | untracked | Not read |
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | ?? | untracked | Not read |
| apps/Model Plane/rust/crates/mp-contracts/src/retention.rs | ?? | untracked | Not read |
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs, zdr_features.rs | ?? | untracked | Not read (adjacent ZDR gates) |
| apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256) | ?? | untracked | Not read |
| apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (+ test) | ?? | untracked | Not read |

Clean (identical in live and worktree): all Go under apps/Model Plane/go (orchestrator-core, capability-core), apps/Control Plane/user-core, apps/Application Plane/notification-core, the gateway crate, ScheduledRunsPanel.tsx, cron-client.ts, schedule-from-chat.ts, CronSchedulesSection.tsx, SpaceWorkPanel.tsx, ChatLiveRunPanel.tsx, action-registry.ts, execution.proto, runs.proto, deploy/docker-compose.yml, scripts/tests/scheduled-step-*.sh, DEV_AUTHORITY_HANDOFF_MATRIX.md, CHAT_GAP_CLOSURE_PLAN_2026-10-01.md, session-core migrations 0004-0043 (incl. 0029, 0034).

## 8. Drift and stale claims found

| Source | Claim | Actual |
|---|---|---|
| Audit card :610 | D3-D11 planned and unbuilt | D9 built (plan :9; ChatLiveRunPanel.tsx). D3-D8, D10, D11 no code |
| Audit card :608 | each step tool-less | still true (live grpc.rs:528) |
| Spec schedule.md | prepare_scheduled_run_thread 4123-4206 | wt starts 4499; live 4670 |
| Spec schedule.md | origin left at zero-value | resolved server-side (grpc.rs:2202, 2314-2340) |
| Spec schedule.md | history.rs read_durable_threads 311-352; chat_history_sessions 170-175 | wt 335-352 and 189-192 |
| Spec schedule.md | InteractiveRunSupervision approval wait 130-153 | wt approval channel 131-134 |
| Spec schedule.md | TestDispatch_ScheduledRun_RejectsNonDefaultWorkflowType | ABSENT; guard at workflow_dispatcher.go:228-232 |
| Spec schedule.md | task_template.policy set to a read-only policy string | not found in ScheduleDialog.tsx or schedule-from-chat.ts (no "policy" match); workflow policy digest handled in Go (orchestrator-core workflowreg_template_digest_test.go) |
| Memory note | no go.work | apps/Model Plane/go/go.work tracked |
| Appendix C :2066 | "Origin attribu..." | truncated in source |
| execution.proto :36-39 | lane disabled until deployed | gating is credential presence (live grpc.rs:249-254); deployment state not determined |

## 9. NOT DETERMINED

- Whether the ExecuteScheduledStep lane is enabled in any deployed environment. Gating is the scheduled inference credential (live grpc.rs:249-254). DEV_AUTHORITY_HANDOFF_MATRIX.md:17-18 says "running dev references absent"; row 18 says "live Temporal/provider proof open".
- Whether owner apply of D1 (scheduled-lane policy grants, OD9) has happened.
- Whether schedule_template.rs run_route, run_notify and run_model fields (test vectors, :279) are the D3 run profile or inert.
- Which migration number the scheduled-run approval storage will take (0044 or 0046).
- Whether untracked mp-contracts/src/zdr_refusal.rs is committed on any branch.
- Whether capability-core notifyclient can carry an approval notice type; whether notification-core has an approval catalogue entry.
- Role of model-gateway schedule_evidence.rs and gateway agents_runs.rs, agent_actions.rs, ingestions/schedules.rs (matched grep, not read).
- Exact approved-tool storage location: a design choice for Control (audit line 1687); no code yet.
- notification-core test command (no Makefile read).
- Whether the Windows cargo caveats in the memory note still apply (not re-verified).
- Line numbers for session-core test functions were taken from the worktree only; live lines may differ.
- Whether the card's Appendix C reference corrections (Claude expiry admin-managed; Hermes deny-default scope) need changes in other WS sweeps (out of scope).
