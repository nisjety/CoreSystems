# WS13-approvals-v2: code map for F-t3-006 (Approvals v2)

Read-only static sweep. No product file, config or doc was edited. No build or test was run. Verdicts come from reads and greps only.

- **WT** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). Short paths below are relative to WT unless marked LIVE. Unmarked line numbers are WT.
- **LIVE** = `C:/dev/CoresSystem` (same HEAD plus uncommitted work). Status from `.plan-work/live-tree-status.txt`; stats from `git --no-optional-locks diff --stat`. "live N" = line in the live tree.
- **Audit** = `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Card `### F-t3-006` at 421-434 (Verevon today at 432, How to build at 434). Wave W2 row at 1557 (deps F-t3-003, F-t3-002, F-t3-066). Appendix B row at 1824. Appendix C at 1963-1971. Appendix D: no line cites F-t3-006.
- **Siblings**: `.plan-work/sweeps/WS03-durable-loop.md` = F-t3-002 ("this run" scope). `.plan-work/sweeps/WS05-declared-tool-classes.md` = F-t3-003 (classes, never-auto set). Space card F-t1-010 starts at `C:/dev/verevon-audit-2026-10-14/01-verevon-space-audit.md:484` (read to 492).

## 0. What the code does today (static)

1. **Decision surface is approve/reject plus free-text reason. No scope exists.** `DecideApprovalRequest` (orchestration.proto:738-757) = approval_id, decision, decided_by, decision_reason, org_id. Model-gateway `DecideApprovalBody {decision, reason}` (http_routes.rs:1486-1491) has no `deny_unknown_fields`, so extra keys are dropped silently. No token matching approved_for_session, for_session, always_allow, approve_always or once_this_thread in Model Plane rust/go or verevonv3 src.
2. **Approval.context exists but holds only {reason, step_id}.** Proto `context` = Struct (orchestration.proto:366). DB column is `approvals.metadata JSONB` (0003:147). Session-core writes `reason` and `step_id` only (orchestration_grpc.rs:2500-2507). `CreateApprovalRequest` has no context or cause field (orchestration.proto:629-676).
3. **HTTP projection drops reason and context.** `approval_value` (http_routes.rs:6042-6053) emits id, run_id, step_id, kind, state, requested_of, decided_by, decision_reason. No reason/detail key. Frontend `normalizeApproval` (orchestration-client.ts:61-74) falls back detail <- summary <- reason <- tool, all absent. Static result: the chat card gets no reason sentence.
4. **Chat card headline is the raw kind.** ChatMessages.tsx:1899-1903 renders `approval.kind`. `approval_value` uses `enum_name` -> prost `as_str_name` (http_routes.rs:6221-6226, EnumName impl at 6256), so the value is the proto name, e.g. APPROVAL_KIND_DESTRUCTIVE (static inference, not run).
5. **Kind is coarse.** Browser gate always sends DESTRUCTIVE (browser_events.rs:274). Agent pause uses `approval_kind_for`: PERMISSION when `requires_consent_to_disclose` (read_subagent_result, permission/mod.rs:160-162), else DESTRUCTIVE (agent.rs:4446-4452).
6. **Browser RiskCategory is typed but not on the approval.** Enum at browser_risk.rs:24-58. It reaches the SSE event (`BrowserActionApprovalRequired.risk_category`, browser_events.rs:262) and the approval `reason` string ("browser action requires approval (<cat>): ...", browser_events.rs:278-282). No typed approval field.
7. **Never-auto seed is one tool.** `requires_durable_owner_approval(t)` = `t.trim() == "tickets.create"` (permission/mod.rs:77-79; ticket_tools.rs:31). Other tool risk is a substring match on the tool name: `is_risky_tool` RISKY list at permission/mod.rs:229-242, match at :284. execute_provider_action is classified by operation (`provider_action_is_risky`, :206-217).
8. **Four other approval-requirement definitions exist.** (a) execution-core permission (item 7). (b) capability-core `riskRequiresApproval` on RiskLevel high/mutation/dangerous (go/services/capability-core/internal/models/availability.go:277-284; used at :73-81 and :137). (c) execution-core `ModelActionViewTool.requires_approval` must be true for tickets.create (capability_policy.rs:216-222, 322-340). (d) browser `classify_action_risk` (browser_risk.rs:253-294). Not traced: whether (b)'s flag reaches the execution gate (model-gateway http_routes.rs:5055 reads `capability.requires_approval`).
9. **Single-use = compare-and-set on pending state.** orchestration_store.rs:755 `decide_approval`; test name at :1511. Exact replay is idempotent and conflicts are refused (orchestration_grpc.rs test at :3748).
10. **Only scope precedent: thread-keyed autonomy grant, in memory.** `PlanModeStore` (model-gateway coordinator.rs:69-100, `GrantEntry` at :89) keyed by thread, written by `handle_exit_plan_mode` (:298). Ledger 1759-1782 (rung granted by plan approval) and 2599-2616 (state: "durable thread-scoped carrier belongs in session-core and remains open").
11. **Denial memory: none found.** Denial aborts the run for browser (browser_agent.rs:908-930, `PlanStatus::Aborted`). Space `settleApproval` denial cancels the run (space-approvals.ts:79-99 doc comment).
12. **Step-up on approvals: none.** Decide needs `authorize_operation("approval:decide")` only (LIVE orchestration_grpc.rs:2652; WT not located by grep). Precedent is client-side: PrivacyDataSection.tsx:23-24 (typed email plus password re-auth), :120-124 (`signIn` re-auth), :131 (`eraseMyAccount`). Gateway always sends `confirm: true` (dispatchers.rs:3668-3688).
13. **LIVE-ONLY dependencies on the decision path.** Live model-gateway decide_approval calls `crate::zdr_posture::continuation_gate` (LIVE http_routes.rs:1503-1504; zdr_posture.rs is `??`). Live session-core decide_approval calls `retention_receipt::refuse_zdr_continuation` and `stored_approval_retention` (LIVE orchestration_grpc.rs:2651, 2659-2671; retention_receipt.rs is `??`). Live agent continuation refusal uses `mp_contracts::zdr_refusal` (LIVE agent.rs:4565-4568; zdr_refusal.rs is `??`). None of these exist on a clean HEAD checkout.

## 1. Card citation check (F-t3-006)

Columns: # | cited in card | claim | WT verdict | live status | corrected path:lines (live where different)

| # | cited | claim | WT verdict | live | corrected |
|---|---|---|---|---|---|
| 1 | apps/Model Plane/proto/model_plane/v1/orchestration.proto:340-381 | durable rows; decided_by; decision_reason | MATCH: decided_by 360, decision_reason 363, context 366 | CLEAN | same |
| 2 | same file:738-757 | decide body approve/reject + reason | MATCH: DecideApprovalRequest 738-757. "approve/reject" is the gateway body; proto uses ApprovalState | CLEAN | same |
| 3 | gateway/src/domains/orchestration.rs:455-478 (resolved: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orchestration.rs) | decide body approve/reject + optional reason | DRIFT (small): doc 457-463, decide_approval 464-479 | CLEAN | :457-479 |
| 4 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1885-1945 | chat card renders only Approve and Reject | MATCH: ApprovalRequests 1887-1930, buttons 1910-1923. Also raw kind headline 1899-1903 and detail 1906-1908 | M (+229); body unchanged, shifted +2 | WT 1887-1930; LIVE 1889-1932 |
| 5 | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4440-4500 | approval_reason_for returns a generic sentence | DRIFT + CLAIM OUTDATED: approval_kind_for 4446-4452; approval_reason_for 4459-4467 has a consent-disclosure branch, else "tool 'X' requires approval"; pause_for_approval 4469-4504 | M (+207); these fns unchanged, shifted +7 | WT 4446-4504; LIVE 4453-4511 |
| 6 | execution-core/src/browser_agent/browser_risk.rs:25-60 (resolved: apps/Model Plane/rust/services/execution-core/src/browser_agent/browser_risk.rs) | typed browser RiskCategory | MATCH: enum 24-45, as_str 47-58 | CLEAN | :24-58 |
| 7 | BrowserChrome.tsx:168-170 (resolved: apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx) | localised reason per category | MATCH: browserApprovalRiskLabel 168-170; label map 159-166 is Bokmal-only; fallback at 169 | CLEAN | :159-170 |
| 8 | gateway/src/domains/actions/dispatchers.rs:3665-3690 (resolved: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs) | account erasure: typed confirm + re-auth | PARTIAL: dispatch 3675-3688, comment 3668-3674 says the client gate is the control. Typed-email and password re-auth are in PrivacyDataSection.tsx:23-24, 99-131 | CLEAN | dispatchers.rs:3668-3688; PrivacyDataSection.tsx:23-24, 99-131; privacy-client.ts:48-54 |
| 9 | "Approval.context" (no path) | reuse context field | MATCH: proto 366; DB metadata 0003:147; approval_from_row (orchestration_grpc.rs:1139-1156) maps context from metadata | proto CLEAN; orchestration_grpc.rs M (shift +1 at LIVE 1140) | n/a |
| 10 | "ApprovalKind" (no path) | reuse kind enum | MATCH: orchestration.proto:194-207 (UNSPECIFIED, PLAN, TOOL_CALL, PERMISSION, DESTRUCTIVE, COST) | CLEAN | same |
| 11 | "requires_durable_owner_approval" (no path) | never-auto seed | MATCH, but one tool: permission/mod.rs:77-79 ("tickets.create") | CLEAN | same |
| 12 | "autonomy-rung grant that plan approval writes" (no path) | grant precedent | MATCH: plan_approval http_routes.rs:7041 (LIVE 7147); PlanModeStore coordinator.rs:69-100, writer :298 | http_routes M; coordinator CLEAN | same |
| 13 | "browser RiskCategory pattern for tool, hook and capability decisions" (What to build) | extend typed categories to tools | NOT PRESENT: tool risk is name-substring (permission/mod.rs:229-284) | CLEAN | n/a |
| 14 | "Typed causes exist only for browser actions" (Verevon today) | claim | OUTDATED: consent gate has typed kind PERMISSION (agent.rs:4446-4452; permission/mod.rs:160-162) | n/a | n/a |
| 15 | "every decision is single-use" (Why it matters) | claim | MATCH: CAS at orchestration_store.rs:755; test :1511 | CLEAN | same |

Corrections to make in the card (static reads):
- Row 5: the reason is no longer generic-only. Kind and reason both branch on the consent predicate (agent.rs:4446-4467). Test `a_disclosure_is_not_reported_as_a_destructive_operation` pins this (permission/mod.rs:557).
- Row 14: a typed kind exists for the consent gate. No typed cause code exists for any approval.
- Row 8: the re-auth is client-side (PrivacyDataSection.tsx:120-124), not in dispatchers.rs.
- Add: the chat card cannot show Approval.context or reason today because approval_value omits both (http_routes.rs:6042-6053). "Reuse Approval.context" needs an HTTP projection change as well as session-core work.
- Add: "no proto change first" holds only for new context keys. A cause written by the creators needs a channel. CreateApprovalRequest has no context field (orchestration.proto:629-676), so creators (agent.rs:4483-4504, browser_events.rs:271-296) would carry cause inside `reason` (string) or need a new field (proto change). Not decided here.

## 2. Gap check against "What to build" (F-t3-006)

| Item | Present at HEAD? | Evidence |
|---|---|---|
| Typed cause code in Approval.context | No | metadata = {reason, step_id} only (orchestration_grpc.rs:2500-2507) |
| Plain localised reason on chat card | No | kind raw (ChatMessages.tsx:1899-1903); reason not projected (http_routes.rs:6042-6053). Browser label map Bokmal-only (BrowserChrome.tsx:159-166) |
| Decision scope once / this thread / always-as-reviewed-rule | No | DecideApprovalRequest has no scope (orchestration.proto:738-757). Thread-keyed grant is in memory only (coordinator.rs:69-100) |
| Denial memory within a run | No | no symbol found: grep for denial_memory, deny_memory, denied_memory, cause_code, CauseCode, rule_id, matched_rule over Model Plane rust/go |
| {actor, verdict, cause, rule_id} in approval event | Partial | ApprovalStateChanged {approval_id, run_id, approval_kind, to, decided_by} (orchestration.proto:1068-1074), emitted by broadcast_approval_decision_events (orchestration_grpc.rs:1753-1778). Actor and verdict yes; cause, rule_id, decision_reason no |
| ... in run log | Not determined | run_event.rs:26-33 names ApprovalStateChanged and RunPausedForApproval; no cause field seen (not fully read) |
| ... in audit | No approval-specific write found | audit_publisher.rs has no "approval" token; "audit" hits in orchestration_grpc.rs are a doc comment (:451) and a test assert message (:3562) |
| Step-up for never-auto and high-risk | No | decide checks ZDR, approval:decide scope and org/user pinning only (LIVE orchestration_grpc.rs:2651-2658) |
| "This run" scope | No; card defers | needs in-run continuation (F-t3-002 / WS03) |
| "Always" as a reviewed rule in a policy registry | No rule store | F-t3-032 not inspected; tool gate uses name substring |

## 3. Affected-file map

Columns: path | role (WT lines) | live status | tests (where cheap)

**Proto / contracts**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/proto/model_plane/v1/orchestration.proto | ApprovalKind 194-207; ApprovalState 210-221; Approval 340-381; CreateApprovalRequest 629-676; ListApprovalsRequest 685; GetApprovalRequest 719; DecideApprovalRequest 738-757; ApprovalStateChanged 1068-1074; RunPausedForApproval 1102; RunResumedAfterApproval 1107; BrowserActionApprovalRequired 1176 | CLEAN | rust/crates/mp-orchestration/tests/proto_wire_parity.rs; go/gen/model_plane/v1/proto_wire_parity_orchestration_test.go |
| apps/Model Plane/proto/model_plane/v1/events.proto | EVENT_TYPE_APPROVAL_REQUESTED=121 (143-144), APPROVAL_DECIDED=123 (147-148) | CLEAN | none found |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | 42 lines added (RPC at 196, message at 1219); no approval token in diff | M | n/a |

**Session-core (system of record)**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs | kind/state maps 701-742; validate_approval_decision_context 752; approval_from_row 1139-1156; broadcast_approval_decision_events 1753-1778; list_approvals 2268; list_pending_approvals 2406; get_approval 2430; create_approval 2461 (metadata 2500-2507); decide_approval 2596 | M (+249). LIVE: approval_from_row 1140 (unchanged body); create_approval 2509; decide_approval 2644 with retention gate 2651, 2659-2671 | 3248 approval_identity_is_pinned_and_cross_tenant_requests_are_rejected; 3690 approval_state_round_trip; 3706 approval_mutations_require_tenant_and_actor; 3748 exact_approval_decision_retry_is_idempotent_but_conflicts_are_not; 3778 approval_decision_replay_does_not_emit_duplicate_events; 3834 approval_reads_require_tenant; 4361 an_approval_without_a_continuation_reports_no_execution; 4610 approval_counts_and_median_decision_latency_are_correct; 5129-5865 plan_gate_* |
| apps/Model Plane/rust/services/session-core/src/orchestration_store.rs | request_approval 644; decide_approval 755 (CAS); list_approvals_by_run 854; ApprovalRow 980; get_approval 1066; list_pending_approvals 1262 | M (+8, provenance only) | 1511 approval_decision_query_is_tenant_scoped_and_compare_and_set; 1522 approval_delivery_insert_rejects_expired_grants; 1528 pending_approval_query_has_no_empty_org_wildcard; 1535 approval_idempotency_is_user_scoped; 1541/1564 delivery outbox tests |
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | called by LIVE decide_approval (refuse_zdr_continuation, stored_approval_retention, require_stored_posture); content not read | ?? (live-only) | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0003_orchestration_tables.sql | approvals table 135-151 (metadata JSONB, decided_by, decision_reason, idempotency_key, expires_at) | CLEAN | n/a |
| .../migrations/0011_identity_scoping.sql | unique index on (org_id, user_id, idempotency_key), lines 4-6 | CLEAN | n/a |
| .../migrations/0014_approval_delivery_outbox.sql | approval_delivery_outbox | CLEAN | n/a |
| .../migrations/0018_approval_continuation_receipts.sql | approval_continuation_receipts | CLEAN | n/a |
| .../migrations/0019_approval_delivery_settlement.sql | delivery settlement constraints | CLEAN | n/a |
| .../migrations/0022_encrypted_approval_continuation_descriptors.sql | encrypted continuation descriptors | CLEAN | n/a |

**Model-gateway (public API, ephemeral caches)**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | routes 213-214 (plan-approval), 283 (list), 295 (get), 313 (decide); list_approvals 963-986; get_approval 1081; DecideApprovalBody 1486-1491; decide_approval 1493; should_resume_granted_approval 767; require_non_zdr_durable_mutation 823; approval_value 6042-6053; enum_name 6221; plan_approval 7041; `mod approval_resume_auth_tests` ~9571 | M (+237). LIVE: decide 1496 (ZDR gate at 1504 uses zdr_posture); plan_approval 7147 | tests/orchestration_http_test.rs:1015 decide_approval_grant_succeeds_without_a_continuation_dispatcher; :1070 decide_approval_reject_has_no_continuation_delivery_field (file M +102) |
| apps/Model Plane/rust/services/model-gateway/src/approvals.rs | ephemeral ApprovalStore (capability matrix:42 says not SoR); to_create_approval_request 52; idempotency_key 78; gateway_approval_from_proto 90; to_decide_approval_request 125; quarantine_granted_approval_continuation 265; handle_deny_approval 823; handle_list_pending_approvals 941; NATS "agents.approval.requested" 755 | CLEAN | unit tests 1160-1630: create_assigns_id_and_pending_status 1160; resolve_approved_sets_decided_fields 1173; granted_retry_after_unknown_resume_delivery_fails_closed 1271; cross_org_resolve_returns_not_found 1377; to_create_request_maps_kind_and_aligns_id 1441; to_decide_request_maps_status_to_state 1460; granted_approval_continuation_is_quarantined_without_a_descriptor 1485; create_request_carries_idempotency_key 1630 |
| apps/Model Plane/rust/services/model-gateway/src/coordinator.rs | PlanModeStore 69 (grants keyed by thread), GrantEntry 89 (rung, expires_at, approved_plan_id), handle_exit_plan_mode 298 | CLEAN | not inspected |
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs | continuation_gate, called by LIVE decide_approval; content not read | ?? (live-only) | n/a |
| apps/Model Plane/rust/services/model-gateway/src/grpc.rs | comment "emit APPROVAL_REQUESTED" at 1448 | M (+47) | not inspected |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs, sse_events.rs | orchestration/SSE relay; symbol grep matched, approval lines not read | M (+883, +203) | not inspected |
| apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs | approval HTTP tests (see above) | M (+102) | see above |

**Execution-core (gate, pause, continuation)**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | approval_kind_for 4446-4452; approval_reason_for 4459-4467; pause_for_approval 4469-4504 (CreateApprovalRequest 4483-4504; TTL 3600 at 4496; idempotency at 4502); continuation_descriptor 4551 (ZDR refusal "approval_continuation" live 4565-4568); owner ask override 4567 | M (+207). Approval fns shifted +7 (LIVE 4453, 4466, 4476); continuation LIVE 4558 | owner_action_descriptor_cannot_preserve_auto_permission 5066; a_run_stopped_by_a_childs_violation_does_not_mint_an_approval_for_a_gated_sibling 8067; gated_tool_creates_approval_and_returns_early 9088; gated_tool_rejects_an_undurable_approval_pause 9167; provider_write_auto_records_and_forwards_real_approval 9233 |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs | execute_step / execute_step_with_browser_grant / execute_step_with_subagent / execute_step_inner call sites | M (+18), no approval token in diff | n/a |
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | create_durable_approval 633 (LIVE 644; body unchanged, 3 comment lines added LIVE 660-662); reject_direct_owner_action 707 (LIVE 722); auth_tests module | M (+98) | auth_tests (not all read) |
| apps/Model Plane/rust/services/execution-core/src/permission/mod.rs | evaluate 31; evaluate_call 43; rung_required_for 100; is_risky_call 195-200; provider_action_is_risky 206-217; is_risky_tool 229-284; requires_durable_owner_approval 77-79; requires_consent_to_disclose 160-162 | CLEAN | 300 restricted_context_writes_are_exactly_the_memory_write; 318 ask_gates_risky_allows_benign; 343 reserved_owner_action_never_auto_executes; 383 external_mcp_tools_are_gated_under_ask; 525 the_consent_gate_is_not_a_risk_gate_in_disguise; 557 a_disclosure_is_not_reported_as_a_destructive_operation; 609 the_same_tool_needs_different_rungs_on_different_calls |
| apps/Model Plane/rust/services/execution-core/src/ticket_tools.rs | TOOL_NAME "tickets.create" (31) | CLEAN | n/a |
| apps/Model Plane/rust/services/execution-core/src/capability_policy.rs | ModelActionViewTool.requires_approval 216-222; tickets.create must have requires_approval true (322-340) | CLEAN | n/a |
| apps/Model Plane/rust/services/execution-core/src/browser_agent.rs | BrowserEventSink::require_approval 235; gate_persistent_cookie_use 856 (once per run); gate_risky_action 908-948; request_approval 950 | CLEAN | 1528 gate_persistent_cookie_use_skips_when_zdr; 1551 ..._asks_once_and_proceeds_on_grant; 1571 ..._aborts_the_run_on_denial; 1630 gate_risky_action_records_deterministic_checkout_not_a_planner_login_label |
| apps/Model Plane/rust/services/execution-core/src/browser_agent/browser_risk.rs | RiskCategory 24-58; RiskyActionDetail 97; classify_action_risk 253-294; risk_reason 466; approval_timeout_seconds 507-516 (env QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S; default 1800; 1 under cfg(test)) | CLEAN | tests module 683-1097 |
| apps/Model Plane/rust/services/execution-core/src/browser_events.rs | require_approval 238; BrowserActionApprovalRequired publish 251-267; CreateApprovalRequest 271-296 (kind DESTRUCTIVE 274; requested_of 275; user_id empty 277; reason 278-282; idempotency "{run}:{action}:{cat}" 289-294; no continuation 295) | CLEAN | 758 require_approval_exits_promptly_when_run_is_cancelled_mid_poll; 801 require_approval_times_out_normally_without_a_state_store; 828 require_approval_ignores_cancellation_of_a_different_run |
| apps/Model Plane/rust/services/execution-core/src/approval_delivery_worker.rs | delivery consumer of approval outbox | CLEAN | not inspected |

**Shared crates**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/rust/crates/mp-contracts/src/autonomy.rs | autonomy ladder: rung_from_keyword, label, permits (per ledger 1759) | CLEAN | not inspected |
| apps/Model Plane/rust/crates/mp-contracts/src/lib.rs | `pub mod autonomy` at line 2 | M (+2) | n/a |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | Refusal::feature_not_supported used by LIVE agent.rs | ?? (live-only) | n/a |
| apps/Model Plane/rust/crates/mp-orchestration/src/approval.rs | domain Approval 73 (context: serde_json::Value); ApprovalKind 25; ApprovalState 41; grant 165; deny 179; mark_timeout 194 | CLEAN | not inspected |
| apps/Model Plane/rust/crates/mp-orchestration/src/run_event.rs | run-event kinds incl. ApprovalStateChanged (27), RunPausedForApproval (33) | CLEAN | not inspected |

**Go services**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/go/services/orchestrator-core/internal/orchestration/handlers.go | pass-through approvals to session-core client: ListApprovals 82-86, GetApproval 89-93, DecideApproval 96-100, CreateApproval 109-113, ListPendingApprovals 116-120 | CLEAN | handlers_test.go (symbol grep only) |
| apps/Model Plane/go/services/capability-core/internal/models/availability.go | Availability {State, ReasonCode, Reason, RequiresApproval} 36-46 (typed reason-code precedent); requiresApproval 73-81, 137; riskRequiresApproval 277-284 | CLEAN | models/availability_test.go |
| apps/Model Plane/go/services/capability-core/internal/api/availability.go | wire fields reason_code/requires_approval (128, 151, 278, 304, 343) | CLEAN | availability_test.go; availability_integration_test.go |

**Frontend Plane (verevonv3)**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Frontend Plane/verevonv3/src/shared/api/orchestration-client.ts | Approval type 13-24 (kind "provider string, rendered as-is"); canonicalApprovalStatus; normalizeApproval 61-74; listApprovals 77-85; decideApproval 87-100 (POST .../approvals/:id/decide, body {decision, reason}) | CLEAN | orchestration-client.test.ts: describe decideApproval 95; test 106-109 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | ApprovalRequests 1887-1930 (kind headline 1899-1903; detail 1906-1908; buttons 1910-1923); rendered for approvals node at 280 | M (+229). ApprovalRequests body unchanged, LIVE 1889-1932 | no test found that renders ApprovalRequests (static grep); ChatMessages.failure-actions.test.tsx is M (+34), approval assertions not read |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts | pendingApprovals 380 | M (+7); LIVE 383 | n/a |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | listApprovals 712 and 1993; PENDING filter 1998; removal from pendingApprovals 2156; decideApproval call 2159 | M (+4), changes all after line 3282; approval code unchanged | use-chat-controller.resume-replay.test.ts (approval assertions not read) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx | run-proof approval kind label 1369 (raw kind) | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx | decideApproval 405; kind render 1302 (LIVE 1309) and 1799 (LIVE 1806) | M (+11) | AgentRunConsole.test.tsx (M +40; decideApproval mock at 64) |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx | BROWSER_APPROVAL_RISK_LABELS 159-166 (Bokmal only); browserApprovalRiskLabel 168-170; browser approval card 312; props 403 | CLEAN | no BrowserChrome.test.tsx; tests that mention it: features/chat/components/ChatLiveRunPanel.test.tsx, features/chat/lib/chat-run-watch.test.ts, features/dashboard/home/browser-session.test.ts (assertions not read) |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx | browser-workspace HITL: listApprovals/decideApproval imports 48; decide call 1145; comment 1115 | M (+10), changes at 542, 1172, 1755 only | not determined |
| apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-approvals.ts | describeApproval (kind sentence, lines 55-68); settleApproval (approve resumes, deny cancels; doc 79-99) | CLEAN | space-approvals.test.ts; SpacePage.test.tsx (approve call expectation 630-631); SpaceApprovalPanel.test.tsx |
| apps/Frontend Plane/verevonv3/src/shared/api/run-console-client.ts | comment on the decide route (130) | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/src/shared/chat-nodes/derive.test.ts | approvals and plan-approval node tests 136-168 | CLEAN | this is the test |
| apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts | no approval action contract (grep empty); privacy erase entry 798-805 | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/src/features/settings/components/PrivacyDataSection.tsx | step-up re-auth precedent: 23-24 (doc), 99-131 (password; signIn re-auth 120-124; eraseMyAccount 131) | CLEAN | TwoFactorEnrollmentSection.test.tsx (symbol grep only) |
| apps/Frontend Plane/verevonv3/src/shared/api/privacy-client.ts | eraseMyAccount 48-54 (sends confirm: true) | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orchestration.rs | routes 71, 83, 92; decide_approval 464-479 forwards body unchanged to model-gateway (475) | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs | dispatch_privacy_erase_my_account 3675-3688 (comment 3668-3674) | CLEAN | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs | line 12: no new approval routes for browser (generic decide route) | CLEAN | n/a |

**Docs, ownership, config**

| path | role (WT lines) | live | tests |
|---|---|---|---|
| apps/Model Plane/docs/decisions/ledger.md | 1759-1782 autonomy ladder (rung granted by plan approval, per-call enforcement); 2388-2390 earlier fault (per-call gate never refused); 2599-2616 C7 fix, durable thread carrier "remains open" | CLEAN | n/a |
| apps/Model Plane/docs/capability-ownership-matrix.md | 26 plan mode/approvals in-memory; 42 gateway stores are caches; 55 approvals -> session-core; 83-91 section 4.1 three-way shadow; 146 G8 implemented; 157 verify-durable-layer | CLEAN | n/a |
| apps/Model Plane/docs/CONTRACTS.md | EventType rows 50-52 (APPROVAL_REQUESTED 121 execution-core; APPROVAL_DECIDED 123 session-core) | CLEAN | n/a |
| apps/Model Plane/docs/gap-analysis.md | line 283 says DecideApproval "Unimplemented" (stale; session-core implements it, orchestration_grpc.rs decide) | CLEAN | n/a |
| apps/Model Plane/docs/sse-event-taxonomy.md | no approval rows; LIVE adds 4 non-approval rows | M (+4) | n/a |
| apps/Model Plane/api/openapi.yaml | DecideApprovalBody 1529-1540; decide operation near 425 | CLEAN | n/a |
| apps/Model Plane/scripts/verify-durable-layer.sh | throwaway Postgres with all migrations; asserts G8 approval id alignment (capability matrix:157) | CLEAN | this is the test (not run) |
| apps/master-ownership-matrix.md | 129 orchestrator-core listed with "approvals"; 168 rule 12 (external effects need single-use receipt) | CLEAN | n/a |
| apps/Model Plane/rust/Cargo.toml | workspace members incl. crates/mp-contracts, crates/mp-orchestration, services/model-gateway, services/session-core, services/execution-core | CLEAN | n/a |
| .github/workflows/model-plane-rust-ci.yml | working-directory apps/Model Plane/rust (57); `cargo test --workspace --no-fail-fast` (119) | M (+50) | n/a |
| .github/workflows/verevonv3-ci.yml | `cargo test` (141) | M (+55) | n/a |
| apps/Frontend Plane/verevonv3/package.json | scripts: build 9, lint 11, test 13, typecheck 17 | M (+2) | n/a |

## 4. Related contracts

- **Proto**: orchestration.proto (messages and enums listed in section 3). events.proto:143-148 (EVENT_TYPE_APPROVAL_REQUESTED=121, APPROVAL_DECIDED=123). sessions.proto has no approval RPC or message at HEAD (two comments only: lines 9 and 1157). The line 9 comment introduces AutonomyRung, the shared ladder a plan approval grants. LIVE adds RPC 196 and message 1219 without approval tokens.
- **SQL**: session-core/migrations has 43 committed files. Latest committed: `0043_audit_outbox_requeue.sql`. LIVE-only (`??`): `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/`. A migration created at HEAD would take 0044, which LIVE already uses for an untracked file; LIVE's next free number is 0046. Approval tables: 0003, 0011, 0014, 0018, 0019, 0022 (section 3).
- **Config/env**: `QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S` (browser_risk.rs:511; default 1800; fails closed to TimedOut). Agent-loop TTL is hard-coded 3600 s (agent.rs:4496). No approval env key found for agent path.
- **Event types**: proto OrchestrationEvent `ApprovalStateChanged`, `RunPausedForApproval`, `RunResumedAfterApproval`, `BrowserActionApprovalRequired` (orchestration.proto:1068-1176). EventType enum 121/123 (events.proto:143-148). sse-event-taxonomy.md has no approval rows. NATS ad-hoc subject `agents.approval.requested` (approvals.rs:755) is a dead end per capability matrix:88. Older phase docs (HARNESS_PHASE1.md:20, PHASE_1_COMPLETION.md:36-50) mention these events and are not authoritative.
- **Gateway routes** (verevonv3 gateway orchestration.rs): GET `/api/v1/orchestration/runs/{run_id}/approvals` (71); GET `/api/v1/orchestration/approvals/{approval_id}` (83); POST `/api/v1/orchestration/approvals/{approval_id}/decide` (92; body `{decision, reason}`, forwarded unchanged).
- **Model-gateway routes**: POST `/v1/runs/:run_id/plan-approval` (213-214); GET `/v1/orchestration/runs/:run_id/approvals` (283); GET `/v1/orchestration/approvals/:approval_id` (295); POST `.../decide` (313).
- **Action contracts** (`src/shared/actions`): none for approvals. Approval decide is a direct client call (orchestration-client.ts:87-100), not an action contract. Erase is an action (action-registry.ts:798-805).
- **OpenAPI / generated**: api/openapi.yaml DecideApprovalBody (1529-1540); api/sdk/typescript/src/models/DecideApprovalBody.ts; api/sdk/python/model_plane_sdk/models/decide_approval_body.py; go/gen/model_plane/v1/orchestration.pb.go; python/mp-events-py/src/mp_events/_gen/model_plane/v1/orchestration_pb2.py.
- **Frontend type**: orchestration-client.ts `Approval` (13-24), `ApprovalDecision` (10).

## 5. Test and build commands (not run)

- **Rust, Model Plane** (`apps/Model Plane/rust/Cargo.toml`; package names from each Cargo.toml): `cd "apps/Model Plane/rust" && cargo test -q -p session-core --all-targets`; same for `-p model-gateway`, `-p execution-core`, `-p mp-orchestration`, `-p mp-contracts`. Name filter: `cargo test -p execution-core approval`. Full: `cargo test --workspace --no-fail-fast` (CI: .github/workflows/model-plane-rust-ci.yml:119, working dir line 57). User memory (model-plane-build-and-deploy): two cargo targets fail pre-existing on Windows; use `--lib` for those.
- **Go** (go.mod modules `github.com/triodelab/model-plane/services/orchestrator-core` and `.../capability-core`): `cd "apps/Model Plane/go/services/orchestrator-core" && go test ./... && go vet ./...`; same in capability-core. Do not use `-race` (user memory: cgo unavailable).
- **Proto / generated**: if proto or openapi changes, `buf generate` from apps/Model Plane (Model Plane CLAUDE.md; `buf lint` has pre-existing debt). Regenerate go/gen, python _gen, api/sdk.
- **Postgres-backed**: `apps/Model Plane/scripts/verify-durable-layer.sh` (needs Docker and Postgres; covers approval id alignment).
- **Frontend** (`apps/Frontend Plane/verevonv3/package.json`): `pnpm test` (node --test gateway scripts, then `vitest run`); targeted: `pnpm vitest run src/shared/api/orchestration-client.test.ts src/features/spaces/lib/space-approvals.test.ts src/shared/chat-nodes/derive.test.ts`; `pnpm typecheck` (tsc -b); `pnpm lint` (eslint .); `pnpm build`. User memory (verevonv3-worktree-verification): main checkout dev server owns :5173; typecheck, lint and vitest have pre-existing failures, so compare failing sets against HEAD.
- **Gateway**: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` run from the verevonv3 root (root CLAUDE.md); crate name `verevon-gateway-rs` (apps/gateway/Cargo.toml). CI: .github/workflows/verevonv3-ci.yml:141 `cargo test`.

## 6. Owners

- **Durable approval record** (table, CAS decide, list, idempotency): session-core, `orchestration_store.rs` and `orchestration_grpc.rs`. Rule: capability-ownership-matrix.md:55 ("Plans / steps / todos / approvals -> session-core") and section 4.1 (83-91). Model Plane CLAUDE.md: "One canonical owner per capability; check docs/capability-ownership-matrix.md".
- **Conflict**: master-ownership-matrix.md:129 lists `orchestrator-core` (Go) with "approvals" in the Model Plane target table. Go handlers.go:82-120 are pass-through to session-core. Capability matrix is the tie-breaker under Model Plane CLAUDE.md. Flag for the WS13 owner decision.
- **Gate decisions** (never-auto, risky, consent, rung): execution-core (`permission/mod.rs`, `runtime_loop/agent.rs`), per master-ownership-matrix.md "Model Plane target" row for execution-core (tool loop, hooks, browser-agent protocol).
- **Ephemeral caches and public projection**: model-gateway (`approvals.rs`, `coordinator.rs` are caches per capability matrix:42; public API per master-ownership row for model-gateway).
- **Declared capability approval flag** (`requires_approval`, riskRequiresApproval): capability-core (Go) per master-ownership row ("safety"). Consumers not traced.
- **Chat card, Space card, gateway routes, client normalization**: Frontend Plane (root CLAUDE.md: "Frontend Plane owns Verevon v3 UI plus same-origin gateway/BFF normalization").
- **Browser actions**: Model Plane owns browser grants; Quarry-v2 (Ingestion Plane) executes or rejects proposed actions (root CLAUDE.md). The card's "Quarry rejection cause is passed through" depends on this boundary.
- **External-effect approvals** (provider writes): integration-corev2 owns provider execution and the single-use receipt; "every other issuer, including Model Plane, fails closed until it implements an equivalent durable contract" (master-ownership-matrix.md:168, rule 12). Step-up for external effects must not be minted by Model Plane alone.
- **Audit**: Control Plane owns audit (root CLAUDE.md). Approval decision audit lands there; Model Plane writes events only.
- **Identity and step-up re-auth**: Control Plane owns identity (root CLAUDE.md). PrivacyDataSection.tsx re-auth calls `signIn`.
- **Localisation gate**: F-t3-066 (UI i18n gates, W2 dependency; audit :1209). BrowserChrome label map is Bokmal-only (BrowserChrome.tsx:159-166).

## 7. Uncommitted-only (LIVE) items relevant to F-t3-006

Status key: M = modified in live, ?? = untracked (exists only in live). Stats from `git --no-optional-locks diff --stat` in C:/dev/CoresSystem.

| path | status | what changed (1 line) | stat |
|---|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | M | decide_approval swaps `require_non_zdr_durable_mutation` for `crate::zdr_posture::continuation_gate` (LIVE 1503-1504); rewrites require_non_zdr_durable_mutation (LIVE 823-834); plan_approval shifted +106 | 237 |
| apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs | M | decide_approval adds retention_receipt refuse/posture gate (LIVE 2651, 2659-2671); approval_from_row body unchanged (+1 shift); new tests | 249 |
| apps/Model Plane/rust/services/session-core/src/orchestration_store.rs | M | provenance-only (RunProvenanceRow 1334; get_run_provenance_for_org 1374-1379); no approval fn changed | 8 |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | M | continuation_descriptor ZDR refusal now `mp_contracts::zdr_refusal` tag "approval_continuation" (LIVE 4565-4568); approval fns unchanged (+7 shift); new tests | 207 |
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | M | 3 comment lines after create_durable_approval (LIVE 660-662); other ZDR and agent-route changes | 98 |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs | M | execute_step call sites and parameters; no approval token | 18 |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | M | ZDR and retrieval changes (approval tokens not read) | 883 |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | M | ZDR and retrieval changes | 203 |
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | M | tool loop changes; not approval-specific by token scan | 425 |
| apps/Model Plane/rust/services/model-gateway/src/grpc.rs | M | not approval-specific by token scan | 47 |
| apps/Model Plane/rust/services/model-gateway/src/browser_run.rs | M | not approval-specific by token scan | 29 |
| apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs | M | added tests (approval tests not read) | 102 |
| apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | M | added tests (approval content not read) | 288 |
| apps/Model Plane/rust/services/session-core/src/learning_events.rs | M | not approval | 77 |
| apps/Model Plane/rust/crates/mp-contracts/src/lib.rs | M | adds 2 lines (content not read; presumably declares zdr_refusal) | 2 |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M | RPC at 196, message at 1219; no approval token | 42 |
| apps/Model Plane/docs/sse-event-taxonomy.md | M | 4 non-approval rows | 4 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | M | ApprovalRequests body unchanged, shifted +2; other edits in EmptyChatState and ErrorNotice | 229 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts | M | pendingApprovals shifted +3; LOW_CONFIDENCE edits | 7 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | M | edits after line 3282 only | 4 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.failure-actions.test.tsx | M | approval assertions not read | 34 |
| apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx | M | kind render shifted +7 | 11 |
| apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.test.tsx | M | added tests | 40 |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx | M | edits at 542, 1172, 1755; decide path unchanged | 10 |
| apps/Frontend Plane/verevonv3/package.json | M | 2 lines | 2 |
| .github/workflows/model-plane-rust-ci.yml | M | CI changes (commands not diffed) | 50 |
| .github/workflows/verevonv3-ci.yml | M | CI changes (commands not diffed) | 55 |
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | ?? | live-only; called by LIVE decide_approval | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql | ?? | live-only migration | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql | ?? | live-only migration | n/a |
| apps/Model Plane/rust/services/session-core/migrations/revert/ | ?? | live-only directory | n/a |
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs | ?? | live-only; called by LIVE decide_approval | n/a |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | ?? | live-only; called by LIVE agent.rs | n/a |

Not relevant but untracked in LIVE: `apps/Model Plane/api/contracts/zdr-refusals.json` and `apps/Frontend Plane/verevonv3/src/shared/api/contracts/` (zdr-refusals.json only).

## 8. Reconciliation with the Space sibling (F-t1-010)

Space card F-t1-010 (01-verevon-space-audit.md:484-492 read; card continues): "scoped decisions (once, this conversation, always for Space plus agent plus action hash) with an explicit addressee", "per-tool allow, ask or block policy with an org floor and a Space ceiling", "untrusted tool output as an approval kind with a preview and single-use release", "turn_origin on decisions". Its Verevon-today claim (01-...:66) says approvals resume only for three action kinds and there are no once/session/always scopes. Code confirms both points.

| topic | F-t3-006 (this card) | F-t1-010 (Space) | overlap / conflict | suggested owner |
|---|---|---|---|---|
| Scope field on DecideApprovalRequest | once / this thread / always-as-reviewed-rule (session scope until confirmed) | once / this conversation / always (Space + agent + action hash) | Same proto field. Card says "no proto change first"; F-t1-010 implies one. Pick one vocabulary before either lands | WS13 with Space session |
| Rule key | rule_id (F-t3-032 policy registry) | agent plus action hash | Two rule identities | WS13 decides, Space adopts |
| "This run" / "this conversation" grant | deferred until in-run continuation (F-t3-002) | scope said to need durable resume | Both depend on resume | WS03 (F-t3-002) |
| Resume honesty | not addressed | card must say which approvals resume | Only 3 kinds resume (Space audit :66) | Space session |
| Denial | denial memory within run (new) | denial cancels run (Space settleApproval; browser aborts run) | Memory would change stop semantics | WS13 |
| Step-up | never-auto and high-risk (new) | not addressed | None | WS13 |
| Addressee | not addressed | requester, named approver, or any member | None | Space session |
| Approval kinds | cause codes in context (no new kind) | new kind for untrusted output (proto ApprovalKind change) | Both edit ApprovalKind (orchestration.proto:194-207) | WS13 and Space session, one proto change |
| Shared code | decide: proto 738-757; session-core decide (LIVE 2644); model-gateway decide (LIVE 1496); orchestration-client.ts 87-100; ChatMessages.tsx 1887 | Space: space-approvals.ts; SpaceApprovalPanel | Same decide path and client | coordinate |

## 9. NOT DETERMINED

- Runtime behaviour: nothing built or run. The raw-kind headline (`APPROVAL_KIND_*`) and missing reason sentence are static inferences from approval_value and normalizeApproval.
- LIVE-only modules: retention_receipt.rs, zdr_posture.rs, zdr_refusal.rs were called from LIVE code but their contents were not read. Whether the live tree compiles was not checked.
- Whether approval rows are written for ZDR runs (agent path) was not traced. `continuation_descriptor` refuses ZDR; the row write path was not followed.
- Audit: whether approval decisions reach Control Plane audit. No approval token in audit_publisher.rs; the outbox path was not traced.
- Whether capability-core `requires_approval` / `riskRequiresApproval` reaches the execution gate (model-gateway http_routes.rs:5055 and execution-core capability_policy.rs:322-340 not fully traced).
- Run-log content (run_event.rs variants beyond the names listed) and mp-orchestration proto_shim.rs not read.
- Tests: none run. Test names listed are from grep. Approval assertions in ChatMessages.failure-actions.test.tsx, use-chat-controller.resume-replay.test.ts, e2e_invoke_chain_test.rs, and orchestration_http_test.rs (live) were not read.
- Whether any test renders the chat ApprovalRequests card or the BrowserChrome approval labels: none found by static grep, not exhaustive.
- Reference-side items in audit Appendix C (Claude 'always' to session coercion; Codex step-up scope) are outside the code map and were not checked.
- Go orchestrator-core Temporal approval workflow (if any) beyond the pass-through handlers: not inspected.
- SSE relay of approval events in model-gateway sse.rs and sse_events.rs: symbol grep only.
- Whether WS13's branch differs from the Space session's branch: not inspected.
- Whether `ChatPanels.tsx` or `ChatPage.tsx` render any approval other than ApprovalRequests and PlanApprovalControl: only ChatPanels.tsx:1369 was seen.
