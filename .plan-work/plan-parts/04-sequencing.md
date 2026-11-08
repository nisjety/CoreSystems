## 4. Sequencing and parallelism

Abbreviations: MP `apps/Model Plane`; CP `apps/Control Plane`; FE `apps/Frontend Plane/verevonv3`; SC/MG/EC/IC = `MP/rust/services/{session-core,model-gateway,execution-core,inference-core}`; CC = `MP/go/services/capability-core`. DEC ids and "2.6 row n" = `plan-parts/02-decisions.md`. `[live-only]` = exists only in the `C:/dev/CoresSystem` working tree, not at HEAD afb83928. Integration branch = `fix/ui-premium-restore` (WS12 section 2.3). Audit = `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Numbers in 4.3 and 4.4 are proposals for the integrator to reserve (DEC-39), compiled from the designs.

### 4.1 Phase 0: land the live baseline (WS01 S0)

**Why first.** Every other design names WS01 S0 as a precondition for at least one slice (WS06 via WS01 S2). The HEAD taxonomy gate is red: `gen-sse-event-taxonomy.py --check` exits 1 at HEAD (rows `plan_proposed`, `source_scope_applied` missing) and 0 live.

**What the 324 entries are** (202 modified/deleted, 122 untracked): Model Plane 78; FE 106 (61 chat/core/dashboard/shared, 45 product-demo and scene); Ingestion/Quarry/integration-corev2 66; Data Plane v2 57; docs/compliance 6; CI/hooks 6; scripts 3; Control 2.

**Designs depend on (all [live-only]):**
- mp-contracts `retention.rs`, `zdr_refusal.rs`, `tests/zdr_retention_matrix.rs`; `api/contracts/zdr-refusals.json` (+`.sha256`); `scripts/tests/zdr-contract-sync.test.sh` (+95).
- SC: migrations `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/`; `retention_receipt.rs`; `grpc.rs` (+817 to +861 as WS04 and WS09 count it), `terminalization.rs`, `auth.rs`, `orchestration_grpc.rs`, `memory_grpc.rs`; `sessions.proto` (+42: `SessionRetentionService`, `CountWaivedRuns`).
- MG: `zdr_posture.rs`, `zdr_features.rs`, `zdr_precheck.rs`; `sse_events.rs` (+~200: `RetrievalNotice`, `ZdrError`); `sse.rs` (+883); `http_routes.rs` (+237); `tool_loop.rs`.
- EC (WS03, WS24): `agent.rs` (+207), `runtime_loop/mod.rs` (+18, `waived`), `grpc.rs` (+98), `agent_route.rs` (+285), `session_core_rules_double.rs` (+537). IC: `auth.rs`, `grpc.rs`.
- FE: `src/shared/api/zdr-refusals.ts`, `src/shared/api/contracts/`, `chat-client.ts` (+53), `ChatMessages.tsx` (+229), `use-chat-controller.ts`. CI: `model-plane-rust-ci.yml` (+50, job `session-core-receipts`).

**Not preconditions** (other sessions land them): Data Plane v2, Quarry/integration-corev2, FE product-demo/scene files and the `demo-product-build` job, the `DashboardComposer.tsx` hunk. MG `relevance*.rs`, `calibration/`, `quarry.rs`, `finetune_routes.rs`, `browser_run.rs` are not named by WS01 S0: classify per hunk before staging.

**How to land** (hunk-level `git add -p` in the owning session; never reset, clean or checkout the live tree: root CLAUDE.md). One commit per group, in compile order:

| Commit | Group | Gate before the commit |
|---|---|---|
| 0A | mp-contracts, `api/contracts`, `scripts/tests/zdr-contract-*` | `bash scripts/tests/zdr-contract-sync.test.sh` (+`--self-test`); `node --test scripts/tests/zdr-contract-schema.test.mjs`; `cargo test -p mp-contracts` |
| 0B | session-core + migrations 0044/0045 + `revert/` + `sessions.proto` | `cargo test -q -p session-core --all-targets` (Windows host: `--lib`); ignored DB test `cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1` (>= 8 pass) |
| 0C | inference-core | `cargo test -q -p inference-core --all-targets` |
| 0D | model-gateway | `cargo test -q -p model-gateway --all-targets` |
| 0E | execution-core [RECONCILE: WS01 S0 omits EC; WS03 and WS24 list these EC files as live-only; added here] | `cargo test -q -p execution-core --all-targets` (incl. `cross_service_loop_contract`, `zdr_refusal_inventory`) |
| 0F | FE ZDR files | `pnpm typecheck && pnpm vitest run src/shared/api/zdr-refusals.test.ts` |
| 0G | docs (`ZDR.md`, `sse-event-taxonomy.md`, `core-research/model-gateway.md`, `BUILD_PASS_*`), `docs/compliance/evidence/*` | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` at PR tip (0D and 0G ship in one PR) |
| 0H | `model-plane-rust-ci.yml` (`.gitleaks.toml` only with singular `[allowlist]`: CI pins gitleaks 8.24.3) | CI run |

At the PR tip also run `buf lint` / `buf breaking` on the +42 `sessions.proto` lines (outcome not determined; the baseline is 0 bytes until WS12 S2). Effort M (2-4 d).

**Rule.** Every stream branches from the tagged landed commit (`mp-baseline-0`, proposed name), never from the live tree. DoD: `git status --porcelain -- "apps/Model Plane"` empty; HEAD contains 0044, 0045, `revert/` and `SessionRetentionService`; taxonomy check exits 0. WS12 S1 may start in parallel; its buf baseline waits for 0B.

### 4.2 Workstream DAG

W0 = Phase 0; W1 = WS01-WS12; W2 = WS13-WS27. "Early" = slices free of decisions that can start in W1.

| WS | Depends on (workstream.slice) | Blocks | Wave |
|---|---|---|---|
| WS01 ops | none; S0 first. S2 needs S0, S1; S7 needs S0, S6; S12 needs S0, S2, S7, S8-S11 | everything (S0); WS06 chain, WS16.S3, WS20.S7 (S2); deploy harness for all (S2) | W0 + W1 |
| WS02 run context | WS01.S0; S6 needs WS07.S1 skip reason (stub first) and DEC-32; S3 enablement DEC-18 | WS03.S3 (tag, builder), WS08.S6, WS18.S1/S5, WS20.S5/S6, WS25 params struct, WS27.S1/S3 | W1 |
| WS03 durable loop | WS01.S0; WS02.S4 (field order); WS05.S1 (classes; stub first); WS04.S2 (cancel in resumed segments) | WS05.S3/S4 (`waived`), WS13.S6, WS14.S5, WS15.S7, WS16.S3, WS20.S6, WS21.S5, WS25.S2/S5, WS27.S9 | W1 |
| WS04 run control | WS01.S0 (S2+), WS01 NATS ACL (S1), WS03 loop-level cancel (S2), WS17 versions (S8) | WS14.S2 (S4 grant), WS17.S8 (S6 `client_message_id`), WS16 liveness, WS05.S2 hydration, WS25.S8 | W1 |
| WS05 tool classes | WS01.S0 (S4); WS03.S6 `waived` signature before S3/S4; WS02 (`merged_tool_defs`); WS04 (hydration, later) | WS13.S6/S7, WS20.S4, WS21.S3/S4, WS23.S3, WS24.S3/S6, WS25.S3/S9, WS26.S7, WS03.S5 | W1 |
| WS06 audit chain | WS01.S2 (replay first); Space F-t1-031 role gate on `GET /v1/audit` | WS13.S3 (soft), WS05 emitter, WS19, WS26.S7 | W1 |
| WS07 memory | WS01.S0; WS26 screening hook (S4/S5) | WS02.S6 (skip reason), WS17.S5, WS26.S4/S5, WS18 containment | W1 |
| WS08 attachments | WS01.S0; WS02.S6 (S6); WS11 tag 17; WS03 scrub | WS19.S8 (`ro_binds` seam), WS22 reserved names, WS10/WS08 `/v1/models` | W1 |
| WS09 transcript perf | WS01.S0 (S8 and FE hunks); WS17 (S8 shared `list_conversation`) | WS15.S5, WS14 tray, WS16.S2, WS17.S9 | W1 |
| WS10 entitlements | WS01.S0, S5, S6, S2; WS11 rung ids; WS12 | WS21.S3 (org-core wiring), WS25.S9, WS27.S11, WS11.S7 | W1 |
| WS11 effort | WS01.S0 (S5); WS10 (S7); WS02/WS03/WS05 (S8); WS12 (S6) | WS10 clamp, WS27.S7 (`RouteNote`) | W1 |
| WS12 FE CI gates | WS01.S0 (S3, S7, S8); DEC-39; WS09 (S12) | enforcement for all; WS13.S2 (S3, S4) | W0 (S1) + W1 |
| WS13 approvals v2 | WS01.S0; WS12.S3/S4; WS03.S2/S3 (S6 hard); WS05.S1/S5; WS06 | WS14 card, WS21.S5b cause, WS23, WS24 floor | W2 (S0-S4 early) |
| WS14 HITL | WS01.S0; WS04.S4 (S2); WS03.S3 (S5); WS02.S6; WS05 | WS16, WS15.S6 | W2 (S1-S3 early) |
| WS15 lifecycle events | WS01.S0; WS09.S2 (S5); WS03.S8 (S7); WS16 enum (S6); WS12.S4/S7 | WS22.S6 (`lifecycle_notice.rs`) | W2 (S1-S6 early) |
| WS16 attention | WS01.S0, S2; WS03.S8; WS13/WS14; WS12 | WS20.S7 (soft), WS15.S6 | W2 (S1-S3 early) |
| WS17 history | WS01.S0; WS09.S8 (shared fn); WS04; WS07.S1 (S5); WS26 (S5) | WS22.S5 (delete hook), WS18.S4, WS04.S8 | W2 (S1, S2, S6, S7 early) |
| WS18 Spaces as projects | WS01.S0; WS02.S4-S6 (S1, S5); WS03, WS02.S7 (S5); WS12 | Space session | W2 (S0-S3 early) |
| WS19 Office | WS01.S0/S2; WS08.S5 (S8); WS05; WS06; WS07 | none | W2 (S0-S3 early) |
| WS20 scheduled runs | WS01.S0/S2; WS02.S4/S7; WS03.S6; WS05.S1/S3/S4; WS16.S3/S4 (soft); WS13 (soft) | WS15 trigger chip (not scheduled) | W2 (S0-S3 early) |
| WS21 agent browser | WS01.S0; QRY Grant v2 note; WS03; WS05.S1/S3; WS10 org-core; WS13.S1; WS14 | WS22.S8 | W2 (S0-S2 early) |
| WS22 sandbox computer | WS01.S0; WS15.S1 (S6); WS17 delete hook (S5); WS08; WS21 (S8) | WS24 thread scope (later) | W2 (S0-S4 early) |
| WS23 connected apps | WS01.S0; WS05.S1; WS03 `waived`; WS02; WS13; WS26; WS08 | none | W2 (S0-S2 early) |
| WS24 Do admission | WS01.S0; WS03 `waived`; WS05.S1; WS13; WS12; WS08 | none | W2 (S0-S2 early) |
| WS25 org policy | WS01.S0; WS03 `waived`; WS05.S1-S5; WS04; WS02; WS10; WS26 (S4) | none | W2 (S0-S6 early) |
| WS26 content safety | WS01.S0; WS07.S1/S7 (S4/S5); WS05 (S7); WS06 | WS25.S4, WS17.S5, WS07 | W2 (S1, S3, S9 early) |
| WS27 cost meter | WS01.S0; WS02.S1/S3; WS03; WS11.S5; WS12.S7; WS09.S2; WS13/WS14 (S11); WS10 | none | W2 (S0-S4 early) |

**Cycle breakers.** WS03.S5 / WS05.S1: WS03 ships a stub class table in `effect.rs`, WS05 replaces it. WS02.S6 / WS07.S1: S6 stamps `SKIPPED_BY_POLICY(memory_off)` once WS07 returns a typed skip reason, else `UNAVAILABLE`. WS09.S8 / WS17.S1: first to start lands, the other rebases. **[RECONCILE]** order of the `waived: bool` signature versus the class seam in EC: WS05 and WS03 put WS03 first; WS13/WS23/WS25 put WS05 S1/S3 first. This plan: WS03.S6 (LoopState refactor plus `waived`) merges first as one early PR, then WS05.S3/S4; WS05.S1/S2 touch Go and gateway files only, so they go earlier.

**Critical paths** (engineer-days from the designs).
1. Spine: WS01.S0 (2-4) then WS02.S4-S7 (17-25 d, overlaps) then WS03 S1-S8 (about 63 d; two engineers, stage 1 in about 3 weeks, all in 8-9) then WS13.S6 (8-12), WS14.S5, WS16.S3-S6, WS20.S5-S8, WS21.S5.
2. Tool governance: WS05 S0-S1-S3-S4-S6 (about 25 d with S2/S5 on a second engineer) then WS13.S7, WS23.S3, WS24.S6, WS25.S9.
3. Ops: WS01 S0-S6-S7-S12-S13 (20-25 d to a signed candidate; S13 elapsed time set by the external ZDR contract).
4. Audit: WS01.S1-S2 (about 5 d) then WS06 S1-S6 (24-32 d). 5. Unattended: WS20 S4-S8 (28-40 d), S6 behind WS03.S6 and DEC-16.

### 4.3 Proto field reservation table

Rules: add-only; never renumber a wire field (`execution.proto:139-142`); none of these messages has a `reserved` range. Record this table as one append-only entry in `MP/docs/decisions/ledger.md` at T0 and put the owner tag in a doc comment at each claimed field. "Final" = integrator-assigned number (planned merge order, frozen when the PR enters its train). `buf breaking` is not a real gate until WS12.S2 (0-byte baseline), so this table is the only collision control until then.

**execution.proto**

| Message.field | Proposed | WS | Conflict | Final |
|---|---|---|---|---|
| RunAgentRequest.run_context (RunContext) | 18 | WS02 S4 (WS18 consumes) | WS03 also 18 | **18** |
| RunAgentRequest.resume (AgentResume) | 18 (WS08 assumes 19) | WS03 S3 | WS02 | **19** |
| RunAgentRequest.attachments (RunAttachment) | 20, fallback `RunContext.attachments = 5` | WS08 S6 | none after reservation | **20** |
| RunAgentRequest.reasoning_effort | "19 or later" | WS11 S8 | WS03 19 | **21** |
| RunAgentRequest.max_questions | next free | WS14 S4 | unspecified | **22** |
| RunAgentRequest.connector_scope (ConnectorScope) | 22 | WS23 S4 | WS25 22, WS14 | **23** |
| RunAgentRequest.org_policy (OrgPolicyContext) | 22 | WS25 S9 | WS23 22 | **24** |
| RunContext 1 authored_instructions, 2 history, 3 memory_block, 4 status | 1-4 | WS02 S4 | none | **1-4**; 5 reserved WS08 fallback; 6 reserved WS20 run notes |
| AgentResume 1 mode, 2 approval_id, 3 step_id, 4 tool_name, 5 action_fingerprint, 6 brief, 7 claim_token, 8 segment_no; question_id, answer_json | 1-8; WS14 next free | WS03 S3; WS14 S5 | none | **1-8; 9, 10** |
| RunAgentResponse.usage (RunUsage 1-7) | 9 | WS02 S3 | none | **9** |
| RunAgentResponse.failure_code | tag not stated | WS25 S2 | none | **10** |
| ExecuteStepRequest.attachments | 12 | WS08 S5 | WS22 12 | **12** |
| ExecuteStepRequest.thread_id | 12 | WS22 S2 | WS08 12 | **13** |
| ExecuteStepRequest.skill_mounts | 13 | WS19 S8 | WS22 | **14** |
| ExecuteScheduledStepResponse rounds_executed .. output_tokens | 7-11 | WS20 S6 | none | **7-11** |

**inference.proto**

| Message.field | Proposed | WS | Conflict | Final |
|---|---|---|---|---|
| InferRequest.reasoning_effort | 17 | WS11 S5 | none | **17** |
| InferRequest.prompt_tiers (PromptTierPlan) | 18 | WS02 S9 | none | **18** |
| InferRequest.cache_scope_key | 19 | WS02 S10 | none | **19** |
| InferRequest.image_parts (InferImagePart) | 20 | WS08 S3 | none | **20** |
| ModelInfo.effort_levels | 8 | WS11 S5 | none | **8** |
| ListModelsResponse.access (ModelAccessMeta) | 2 | WS10 S3 | none | **2** |
| InferResponse.route_note / InferChunk.route_note (RouteNote) | 15 / 16 | WS27 S7 (WS11 shares the message) | WS15 deferred `retry_notice` on InferChunk 16 | **15 / 16**; WS15 live retry withdrawn (2.6 row 1); if revived it takes 17 |
| LanguageAnalysisResult.injection_json | 14 | WS26 S6 | none | **14** |
| New RPC `GetModelAccess` + ModelPolicyDoc, SurfaceDefault, GetModelAccessRequest/Response, ModelAccessEntry; `enum ReasoningEffort` | new | WS10 S3; WS11 S5 | none | new messages, no field collision |

**sessions.proto** (new RPCs append at the end of `SessionCore` after live `CountWaivedRuns`, in merge order)

| Message.field | Proposed | WS | Conflict | Final |
|---|---|---|---|---|
| ListConversationRequest.limit, before_message_id; ListConversationResponse.has_more | 6, 7; 2 | WS09 S8 | none | **6, 7; 2** |
| ListConversationRequest.include_inactive; SessionMessage parent_message_id, sibling_ordinal, sibling_count, on_active_path | 8; 7-10 | WS17 S8 | none | **8; 7-10** |
| ListThreadsRequest cursor, thread_id, archived; ListThreadsResponse.next_cursor | 8, 9, 10; 2 | WS17 S1 | none | **8, 9, 10; 2** |
| ListThreadsRequest.starred_only | 11 | WS17 S7 | none | **11** |
| UpdateThreadPresentationRequest starred, unread, archived | 7, 8, 9 | WS17 S7, S2 | none | **7, 8, 9** |
| ThreadSummary.archived_at | 15 | WS17 S1 | WS16 15 | **15** |
| ThreadSummary attention_state, attention_since, attention_unseen | 15, 16, 17 | WS16 S1 | WS17 15-17 | **16, 17, 18** |
| ThreadSummary forked_from_thread_id; starred, unread, last_read_at | 16; 17, 18, 19 | WS17 S6, S7 | WS16 | **19; 20, 21, 22** |
| AppendMessageRequest.client_message_id; AppendMessageResponse message_id, deduplicated | 18; 2, 3 | WS04 S6 (WS17 S8 shares message_id 2) | none | **18; 2, 3** |
| SetRunModeRequest.approved_plan_id; new service `ThreadAutonomy.GetThreadGrant`; `BindGrantToRun` | 6 | WS04 S4; WS14 S2 | none | **6** |
| ReserveToolActionRequest args_digest, effect_class; FinalizeToolActionRequest.receipt_id | 7, 8; 4 | WS03 S5 | none | **7, 8; 4** |
| UpsertAgentSkillRequest scope .. proposal_reason; Response skip_reason, revision; ListAgentSkillsRequest.for_user_id; AgentSkill 13-18 | 10-15; 4, 5; 3; 13-18 | WS07 S7 (WS26 shares `skip_reason = 4` by name) | none | **as proposed** |
| StartScheduledRunRequest contract_json, contract_digest, contract_expires_at | 14, 15, 16 | WS20 S5 | none | **14-16** |
| New RPCs: RecordRunContext (WS02 S4), SaveLoopCheckpoint / ClaimLoopCheckpoint / CloseLoopCheckpoint (WS03 S4), RecordScheduledRunResult (WS20 S1), SearchMessages (WS17 S3), ForkThread (WS17 S6), SetActiveLeaf (WS17 S8), DecideSkillProposal / UndoLearningEntry / ListLearningLedger (WS07 S8); `attention.proto` AttentionService (WS16 S1, add to mp-contracts `build.rs` list `:37-58`) | new | | none | append order = train order |

**[RECONCILE]** WS16 `attention_unseen` (from `threads.attention_seen_at`) and WS17 `unread` (from `last_read_at`) are two seen markers; DEC-11 defines unread as `last_assistant_at > last_read_at`. Keep both fields; WS16 derives unseen from `last_read_at` and drops `attention_seen_at` from migration 0053.

**orchestration.proto and runs.proto**

| Message.field | Proposed | WS | Conflict | Final |
|---|---|---|---|---|
| CreateApprovalRequest.continuation_mode; .cause | 12; 12 (13 after WS03) | WS03 S2; WS13 S1 | yes | **12; 13** |
| Approval consumed_at, continuation_mode; cause; decision record | 14, 15; 14 (16 after WS03); 15 | WS03 S2; WS13 S1/S3 | yes | **14, 15; 16; 17** |
| DecideApprovalRequest decision_source, scope; ApprovalStateChanged cause_code .. step_up_verified | 6, 7; 6-10 | WS13 S3 | none | **6, 7; 6-10** |
| ApprovalKind next value | 6 | WS13/Space (DEC-08) | WS14 and WS16 text say WS14 supplies kind 6; DEC-05 says questions are a table, not a kind | **6** = the one coordinated Space/WS13 kind; WS14 takes none |
| OrchestrationEvent RunParked, RunResumed | 24, 25 | WS03 S8 | none | **24, 25** |
| OrchestrationEvent ToolRetryScheduled | 26 | WS15 S3 | WS14 26 | **26** |
| OrchestrationEvent QuestionAsked, QuestionAnswered | 26, 27 | WS14 S3 | WS15, WS27 27 | **27, 28** |
| OrchestrationEvent RunUsageProgress | 27 | WS27 S9 | WS14 | **29** |
| New RPCs: ConsumeApproval (WS03 S2); CreateRunQuestion, ListRunQuestions, AnswerRunQuestion (WS14 S3); UseApprovalScopeGrant, ListApprovalScopeGrants, RevokeApprovalScopeGrant (WS13 S6) | new | | none | n/a |
| RunDetail resumable, park_reason (runs.proto) | 17, 18 | WS03 S8 | none | **17, 18** |
| ScheduledStepContext contract_json, contract_digest, contract_expires_at, origin; ResolveScheduledStepAuthorityResponse contract_digest, contract_expires_at (runs.proto) | 6-9; 15, 16 | WS20 S5 | none | **6-9; 15, 16** |

**capabilities, gateway, memory, sandboxes, browser**

| Message.field | WS | Final |
|---|---|---|
| CapabilityDetail side_effect_class, approval_floor | WS05 S1 | **18, 19** |
| EvaluatePolicyRequest user_id, tool_name; EvaluatePolicyResponse side_effect_class, approval_floor, policy_source, tool_digest; new RPC ResolveToolBindings (+ToolBinding 1-8) | WS05 S3 | **6, 7; 7-10** |
| New skill-bundle RPCs ListSkillBundles, GetSkillBundleManifest, GetSkillBlob | WS19 S6 | new messages |
| gateway.proto ListMcpToolsRequest.unstaged; ListMcpToolsResponse.meta (McpToolMeta); ProxyMcpToolRequest.expected_digest | WS05 S2 | **4; 3; 7** |
| gateway.proto Hook priority, revision, created_by | WS25 S8 | **8, 9, 10** |
| gateway.proto ScreenContent RPC + messages (file end) | WS26 S7 | new messages |
| memory.proto IndexMemoryRequest.origin; SearchMemoryResponse.skip_reason, ListMemoryResponse.hidden_reason; Get/SetMemoryControls, ResetMemory | WS26 S4; WS07 S1/S2 | **7; 4, 4; new** |
| sandboxes.proto AcquireLeaseRequest.zdr; Get/Apply/DeleteThreadWorkspace; SandboxFailureCause, GetLeaseStatus | WS22 S1, S5, S6 | **8; new** |
| browser.proto AcquireGrantRequest space_id, read_only, ttl_seconds; AcquireGrantResponse read_only, denied_domains; ValidateGrantResponse read_only | WS21 S4 | **10-12; 10, 11; 10**. [RECONCILE] QRY session's Grant v2 note owns these tags (DEC-20, 2.6 row 15); take its numbers if they differ |

SSE ChatEvent variants (taxonomy gate, not proto): WS15 S1 `retry_scheduled`, `compaction`, `context_change` (`run_parked` at S7); WS14 S4 `user_question`; WS27 S3 `usage_progress`; additive keys on `usage` (WS02 S1) and `queued_input.ids` (WS04). Arrival order WS02, WS04, WS13/WS14, WS15, WS27.

### 4.4 Migration number plan

Provisional numbers in planned merge order, frozen when a PR enters its train. Never take session-core 0044/0045. Every session-core migration ships `revert/NNNN_*.down.sql` (WS01.S9 policy, the live 0044/0045 convention) and is additive. Go services re-run every `*.up.sql` at every boot with no ledger: write idempotent SQL. No third duplicate prefix (capability-core 0013, user-core 008 and 021 already are).

| Service (next free at HEAD / live) | Planned numbers (WS slice, purpose) |
|---|---|
| **session-core** (0044 / **0046**) | 0046 WS03.S2 `approval_consume_and_mode`; 0047 WS17.S1 `thread_activity_cursor`; 0048 WS03.S4 `run_loop_checkpoints`; 0049 WS03.S5 `tool_action_effect_ledger`; 0050 WS04.S4 `thread_autonomy_grant`; 0051 WS07.S1 `user_memory_state`; 0052 WS04.S6 `message_client_id`; 0053 WS16.S1 `attention_state`; 0054 WS14.S3 `run_questions`; 0055 WS17.S3 `message_search_index` (first line `-- no-transaction`, `CREATE INDEX CONCURRENTLY`); 0056 WS20.S1 `cron_fire_run_link`; 0057 WS07.S4 `org_agent_policy`; 0058 WS07.S7 `agent_skills_proposals`; 0059 WS17.S6 `thread_fork_lineage`; 0060 WS17.S7 `thread_star_unread`; 0061 WS17.S8 `message_versions`; 0062 WS13.S6 `approval_scope_grants` (the WS13.S7 rule link folds in if planned with S6, else next free); 0063 WS14.S5 `run_question_consume`; 0064 WS07.S8 `skill_revisions` (trigger); 0065 WS20.S4 `cron_schedule_contract`; 0066 WS25.S12 `drop_hook_configs` (only after one release with `HOOKS_DURABLE=on`); WS07.S6 `thread_memory_mode` (optional) takes the next free number if scheduled |
| **capability-core** (0016) | 0016 WS05.S1 `capability_tool_classes`; 0017 WS05.S5 `tool_policies`; 0018 WS24.S3 `workspace_read_capability`; 0019 WS24.S4 `workspace_write_capability`; 0020 WS25.S5 `result_query_capability`; 0021 WS19.S6 `skill_bundles`; 0022 WS24.S6 `agent_action_admissions`; 0023 WS25.S7 `org_agent_policy`; 0024 WS25.S8 `org_hook_rules`; 0025 WS21.S4 `browser_tool_capabilities`; 0026 WS15.S9 `mcp_server_incidents` (optional, WS23 recommended owner) |
| **org-core** (021) | 021 WS10.S2 `org_model_entitlements`; 022 WS06.S7 `siem_destinations`; 023 WS21.S3 `org_browser_sites` |
| **audit-core** (007; add each file to the `//go:embed` list and `migrations` slice in `internal/store/migrate.go:12-28, 36-43`) | 007 WS06.S2 `audit_chain`; 008 WS06.S3 `audit_chain_checkpoints`; 009 WS06.S4 `audit_chain_verification_state`; 010 WS06.S8 `audit_export_cursors` |
| **user-core** (030) | 030 WS07.S5 `retire_ai_memory_enabled` (up-only); 031 WS24.S7 `space_effect_policy_read_actions` (optional); 032 WS21.S8 `space_browser_entitlement` |
| **notification-core** (015) | 015 WS16.S4 `register_attention_notifications`; 016 WS16.S6 `web_push_subscriptions`; 017 WS20.S7 `register_scheduled_run_outcome` |
| **cost-core** (0011) | 0011 WS02.S2 `cache_miss_reason` |
| **sandbox-manager** (0005; not in the release snapshot) | 0005 WS22.S1 `thread_workspace`; 0006 WS22.S1 `thread_lease_unique`; 0007 WS22.S6 `lease_lifecycle` |
| orchestrator-core, auth-core, Convex | none. auth-core: WS01.S3 needs no migration and must not touch the missing `init_better_auth.sql` gap. Convex: WS10.S0 only comments `schema.ts:21-22` |

**[RECONCILE]** 2.6 row 2 lists a WS07 S8 trigger under capability-core; the WS07 design puts `skill_revisions` in session-core (`agent_skills` lives in the `session_core` database). This plan follows the design (0064). No migration: WS08, WS09, WS11, WS12, WS18, WS26, WS27, WS15 S1-S8, WS23 S3, WS13 cause (JSONB in `approvals.metadata`).

### 4.5 Hot-file conflict matrix (3+ workstreams)

Standing rules: new logic in new modules with thin call-ins; proto edits append-only; no reformatting of shared files; the second lander of a generated artifact regenerates it.

| File | Workstreams | Goes first, then |
|---|---|---|
| MG `sse.rs` | WS01 (S0, S6), WS02, WS03, WS04, WS11, WS14, WS15, WS18, WS23, WS25, WS26, WS27 | WS01.S0, WS01.S6, WS02 (new modules `run_context.rs`, `cache_telemetry.rs`), WS04 single-line call-ins, WS03 `Option<AgentResume>` params, WS11.S5 derivation sites, then the rest as call-ins into their own modules |
| MG `sse_events.rs` | WS01, WS02, WS04, WS12, WS13, WS14, WS15, WS18, WS27 | WS01.S0, WS12.S7 (tests only), WS02.S1 (Usage keys), WS04 (`queued_input.ids`), variants in arrival order (4.3); WS27 in a fenced block |
| MG `http_routes.rs` | WS03, WS04, WS08, WS09, WS10, WS11, WS13, WS14, WS15, WS16, WS17, WS25 | WS01.S0, WS03.S2 (`continuation_delivery`), WS13 (`cause`, `decision`, `scope`), then routes beside their siblings |
| MG `grpc.rs`, `state.rs`, `lib.rs`, `runtime_registries.rs` | WS04, WS05, WS10, WS25, WS26 | WS04.S1 (one `AppState` field), then WS05/WS25 on the committed ZDR helper |
| EC `agent.rs`, `runtime_loop/mod.rs` | WS02, WS03, WS05, WS08, WS11, WS13, WS14 (new `ask.rs`), WS15, WS21, WS22, WS23, WS24, WS25, WS26, WS27 | WS01.S0; WS02.S4 literal conversion (27 sites); WS03.S6 (LoopState refactor plus `waived`) as one early PR; WS02.S7; WS05.S3/S4; then the rest; WS03 owns checkpoint and resume sections, WS11.S8 last |
| EC `grpc.rs`, `permission/mod.rs`, `capability_policy.rs` | WS02, WS03, WS05, WS13, WS14, WS23, WS24, WS25 | WS05.S1/S3 class lookup is the seam; others add floors or rows, not name lists |
| SC `grpc.rs`, `orchestration_grpc.rs`, `terminalization.rs` | WS02, WS03, WS04, WS07, WS09, WS13, WS14, WS15, WS16, WS17, WS20 | WS01.S0; whoever first touches `append_message_in_tx` owns it (WS04.S6, WS17, Space F-t1-001); WS03.S8 owns park-vs-terminalize; WS16 uses a post-step `attention::enrich`, no SQL edit |
| `sessions.proto`, `orchestration.proto`, `execution.proto`, `inference.proto` | per 4.3 | WS01.S0 then the reservation table; WS02.S4 edits `execution.proto` first |
| IC `config.rs`, `grpc.rs`, `provider/*` | WS01, WS02, WS10, WS11, WS23 | WS01.S5, WS11.S1 (struct field), then WS10, WS02 |
| FE `ChatMessages.tsx` | WS03 (`continueRun`), WS09, WS12, WS13, WS14, WS15, WS16, WS17 | WS01.S0, WS12.S3 (six locale hunks), WS09.S2, WS03, WS13, WS14, WS15/WS16/WS17 |
| FE `use-chat-controller.ts`, `chat-client.ts`, `chat-normalizers.ts` | WS02, WS04, WS08, WS09, WS11, WS14-WS17, WS27 | WS01.S0, WS09 (S3/S5/S6/S9), then add-only hunks |
| FE `action-registry.ts`, BFF `handlers.rs`, `dispatchers.rs` | WS07, WS10, WS13, WS14, WS17, WS21, WS24, WS25 | additive blocks; registry entry and handler in one PR |
| CC `server.go`, `engine.go` | WS05, WS19, WS21, WS24, WS25 | WS05.S1, then 4.4 order |
| `MP/deploy/docker-compose.yml`, `.env.example`, `nats.conf`; `CP/docker-compose.yml`, `scripts/run-control-plane.sh`, `CP/config/plane-service-principals.json` | all | WS01.S1 (alias) and S8 (release overlay) first; one `${VAR:-default}` line per flag; credentials JSON is one line: merge by hand; scopes added alphabetically |
| `mp-contracts/src/lib.rs`, `build.rs` | WS02, WS05, WS10, WS13, WS16, WS19, WS20, WS26 | one-line `pub mod` additions after the live wave |
| Ledger, `ZDR.md`, `MODEL_PLANE_STATUS.md`, `docs/compliance/evidence/*` | all | append-only rows, one owner per row; `ZDR.md` Step 1 only by WS01.S5 |

### 4.6 Parallel lanes and merge trains

**Lanes** (one worktree per branch under `C:/dev/CoresSystem/.claude/worktrees/`; branches `feat/mp-wsNN-<slice-slug>`, `feat/cp-wsNN-...`, `feat/fe-wsNN-...`, `feat/ci-wsNN-...`; each starts at `mp-baseline-0` and rebases on the integration branch before its train).

| Lane | Workstreams and slices | Conflicts with | Serial constraint |
|---|---|---|---|
| A Ops and CI | WS01 S1-S3, S5, S8-S14; WS12 S1-S2, S5, S9-S11 | none in product code; compose passthrough lines only | WS01.S0 first; S2 after S1 |
| B Control and Application services (Go) | WS06; WS10.S0-S2 org-core; WS21.S3 org-core; WS07.S3-S5 org/user-core; WS16.S4/S6 and WS20.S7 notification-core; WS20.S4 user-core | `CP/docker-compose.yml`, principals file | audit-core and org-core numbers in 4.4 order; WS06 chain after WS01.S2 |
| C Spine (Rust loop) | WS02.S4-S7; WS03 all; WS05.S3/S4; WS13.S1; WS14.S4/S5 agent side; WS23.S3/S4; WS24.S3/S4; WS25.S1-S5; WS21.S5a/S5b; WS22.S2; WS26.S7 | EC `agent.rs`, `mod.rs`, `execution.proto` | **Serial queue, fixed pair of engineers**: order in 4.5 |
| D Session-core store | WS04; WS07.S1/S2/S4/S7/S8; WS14.S3; WS16.S1/S3; WS17; WS13.S6; WS20.S1/S4/S5 | SC `grpc.rs`, `sessions.proto`, `orchestration.proto` | parallel worktrees allowed (new modules, appended RPCs); migrations and tags by reservation; `append_message_in_tx` single owner |
| E Gateway chat path and inference | WS02.S1-S3, S9-S11; WS08.S2-S4; WS10.S3-S4; WS11; WS15; WS26.S2-S6; WS27; WS18.S1-S3; WS23.S5 | MG `sse.rs`, `sse_events.rs`, `http_routes.rs` | E1 inference-core (WS01.S5, WS11.S1-S4, WS10.S3, WS02.S9/S10, WS08.S3) runs free; E2 gateway follows the `sse.rs` queue of 4.5 |
| F Go capability and sandbox services | WS05.S1/S2/S5; WS19.S6/S7; WS24.S3/S6; WS25.S7/S8; WS22.S1/S5/S6; WS20.S5 cron side | CC `server.go`, migrations | capability-core numbers in 4.4 order |
| G Frontend | WS09; WS12.S3-S8; WS14.S1; WS16.S2; WS17.S2/S4/S9; WS13.S2; WS15.S5/S6; WS27.S4; WS18.S4; WS10.S6; WS25.S11; WS07.S3/S9; WS19.S1/S5; WS23.S6; WS21.S6; WS22.S6 | FE chat files | queue: WS12.S3, WS09.S2, WS03, WS13, WS14, WS15/16/17 |

Serial queues: C, E2 (`sse.rs`), G. Lanes A, B, E1 and F touch disjoint source trees and run beside C and D.

**Merge trains** (a train merges only after the previous train's gate is green on the integration branch; inside a train, order as listed; each PR carries its own slice DoD).

| Train | Contents | Gate |
|---|---|---|
| T0 Baseline | WS01.S0 commits 0A-0H; WS12.S1 | 4.1 DoD; taxonomy `--check` exits 0 |
| T1 Gates and contract-free wins | WS12.S2 (buf baseline from the committed state), S3, S6; WS01.S1, S3, S5, S6, S8; WS02.S1; WS04.S1 (dark); WS09.S1-S3, S5; WS11.S1-S4; WS10.S0-S1; WS26.S1, S3; WS27.S0-S1; WS14.S1; WS23.S0-S2; WS24.S0-S1; WS25.S0-S4; all other S0 decision-record slices (WS05, WS13, WS18-WS22) | `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, per-crate tests (`--lib` on Windows); taxonomy check; `zdr-contract-sync.test.sh`; buf lint ratchet and `buf breaking` green once WS12.S2 is in; Go `go test ./... && go vet ./...` per touched service (no CI reaches `MP/go` until WS01.S11) |
| T2 Spine contracts | WS02.S4 (+S5, then S3); WS03.S1, S2; WS05.S1 | `buf breaking` green (add-only); wire-parity goldens (Rust and Go); all 27 `RunAgentRequest` literal sites compile; migration lint (WS01.S9 once merged); ignored DB tests with `DATABASE_URL` |
| T3 Spine behaviour | WS03.S6 first, then WS02.S6/S7, WS03.S3, S4, S5, S7; WS05.S2-S4; WS04.S2-S7; WS07.S1-S2; WS17.S1, S2; WS16.S1 | `cross_service_loop_contract`, `autonomy_ladder_contract`, `compaction_parity_contract`, new `durable_loop_contract`; two-replica test `run_control_two_replica_test.rs`; ZDR posture matrix `zdr_posture_matrix.rs`; Stage 1 `/continue` proved on a running stack |
| T4 Stores and history | WS14.S3; WS17.S3, S6, S7, S8; WS09.S8, S9; WS07.S4, S7; WS16.S3; WS20.S1-S3; WS13.S1-S5; WS10.S2-S4; WS11.S5-S6; WS08.S2-S4; WS15.S1-S4; WS02.S2 (cost-core 0011) | session-core `--all-targets` plus ignored DB tests; erasure test rows; every migration has `revert/`; taxonomy regenerated by the second lander |
| T5 Approvals, HITL, events, reads | WS13.S6; WS14.S2, S4, S5; WS15.S5-S7; WS16.S2, S4; WS27.S2-S4, S6; WS07.S3, S5, S8; WS08.S5-S6; WS18.S1-S5; WS23.S3-S6; WS26.S2, S4-S6 | single-use consume tests; ZDR matrix rows for every new store (DEC-32); FE `pnpm typecheck && pnpm lint && pnpm test` against the recorded baseline |
| T6 Tools, policy, files | WS24.S2-S8; WS25.S5-S11; WS26.S7; WS19.S2-S9; WS22.S1-S5; WS06.S2-S8; WS05.S5-S6; WS10.S5-S6 | signed-gate tests (forged, unbound, replayed decision refused); org-core and audit-core Go tests with Postgres; sandbox integration tests; release-contract tests for new compose services |
| T7 Unattended, browser, computer | WS20.S4-S8; WS21.S2-S9; WS22.S6-S7; WS16.S5-S6 | contract-digest tests; Quarry-side grant verification (QRY session); lease and ZDR erasure tests |
| T8 Flips and proof | flag flips and live proofs: WS01.S7, S12-S14; WS02.S8; WS04.S9; WS10.S7; WS13.S8; WS14.S7; WS16.S7; WS21.S10; WS24.S9; WS25.S12; WS27 enablements | rebuilt images, running stack, one verified flow per flag (repo evidence rule); ZDR-TEST org proof (`zdr-live-proof.sh`, `zdr-live-turn.spec.ts`); rollback rehearsal evidence |

Gated, optional or unscheduled slices (WS04.S8, WS09.S4/S6/S7/S10, WS11.S7-S9, WS22.S7/S8, WS23.S9, WS20.S9, WS07.S6, WS02.S12, WS15.S8/S9) join a train only after their decision (`02-decisions.md` 2.1).

### 4.7 Team and time shape

Assumptions: net 4.0 engineer-days per engineer-week (review, Rust image rebuilds over 10 minutes, deploy evidence); 15 percent coordination overhead; effort = the designs' day ranges (low = committed slices, high = plus optional ones; "not scheduled" slices excluded); owner rulings, the external ZDR contract (WS01.S13) and the QRY and Space sessions are not counted. W1 about 400-520 engineer-days (WS01 45, WS02 34-50, WS03 63, WS04 30-41, WS05 31-44, WS06 38-52, WS07 36-47, WS08 34-49, WS09 15-23, WS10 31-44, WS11 17-24, WS12 27-35). W2 about 620-850 (WS13 37-54 through WS27 43-62; sum of the per-design totals).

Elapsed = max(spine, capacity). Spine: W1 about 10-11 weeks (WS01.S0 3 d, WS02.S4 3-5 d, WS03 8-9 weeks with two engineers); W2 spine ends near weeks 18-22 (WS20.S4-S8, 28-40 d, behind WS03.S6).

| Engineers | Lane mapping | W1 elapsed | W2 elapsed | Whole program (W2 overlaps W1) |
|---|---|---|---|---|
| 3 | pair on lane C; one engineer rotates A, D, E1, G (order WS01, WS12, WS04, WS09) | 38-50 weeks | 60-81 weeks | about 100-130 weeks; W1 only, then W2 by value |
| 5 | C pair; D one; A and B one; E and G one | 23-30 weeks | 36-49 weeks | about 60-80 weeks |
| 8 | C pair; D two; A, B, E, G, F one each | 14-19 weeks (capacity-bound; spine 10-11) | 22-30 weeks | about 37-49 weeks |

Headcount-independent milestones: CI and buf gate real in week 1; audit delivery proven (WS01.S1-S2) in weeks 1-2; run-context contract in week 2; Stage 1 `/continue` (WS03.S3) near weeks 5-6; signed candidate and fail-closed ZDR proof (WS01.S12) near weeks 5-7. Capacity figures treat every design as in scope; an owner scope cut shortens W2 more than extra engineers do.

## 7b. The first three pull requests

Branches open on day 1, rebase onto `mp-baseline-0`, and merge in order PR-1, PR-2, PR-3. Why these: PR-1 is the precondition of every design; PR-2 makes every later gate real (0-byte buf baseline, red taxonomy gate at HEAD, CI not firing on the integration branch); PR-3 is the highest-conflict mechanical change (tag 18, 27 struct-literal sites) and needs no decision, so merging it early shrinks every later rebase.

### PR-1: `chore(model-plane): land ZDR and session-core live baseline`
- **Branch:** `feat/mp-ws01-s0-land-live-baseline` (tag `mp-baseline-0` at merge). Slice WS01.S0, effort M (2-4 d).
- **Files:** the 4.1 groups 0A-0H, eight atomic commits in one PR, staged by hunk in the owning session. Excluded: Data Plane, Quarry, integration-corev2, FE product-demo and scene files, the `DashboardComposer.tsx` hunk.
- **Tests:** the per-commit gates of 4.1; at the PR tip `cargo test --workspace --no-fail-fast` (Windows host: per-crate `--lib`, two cargo targets fail pre-existing), taxonomy `--check`, `zdr-contract-sync.test.sh --self-test`.
- **Evidence:** commit list, gate output lines, the 4.1 DoD checks (`git status --porcelain`, migrations listing, taxonomy exit 0).
- **Risks:** hunk entanglement with other sessions in `sse.rs`, `grpc.rs`, `ChatMessages.tsx`.

### PR-2: `ci: run gates on the integration branch and repair the buf gate`
- **Branch:** `feat/ci-ws12-s1-s2-reachability-and-buf`. Slices WS12.S1 (S, 1-2 d) and S2 (M, 2-3 d); decisions D1(a), D7. Open after PR-1 lands 0A-0B (the baseline must be built from committed state, never from the dirty tree).
- **Files (S1):** `.github/workflows/verevonv3-ci.yml` (`:11-12` `pull_request` filter and `push:` block only; the live `demo-product-build` job belongs to the demo session: rebase over it, never edit inside jobs), `buf.yml:3-12`, `verevonv3-security.yml`, `quarry-v2-ci.yml` (filters only); NEW `FE/docs/CI_BASELINE_2026-10.md`; NEW `.github/workflows/repo-gates.yml` (trigger shell).
- **Files (S2):** `buf.yml:21,23` (quote the path with spaces); `MP/proto/baseline.binpb` (rebuild with `docker run --rm -v "$PWD:/w" -w /w bufbuild/buf build ...`; buf is not on the host); NEW `scripts/gates/buf-lint-ratchet.py`; NEW `MP/proto/buf-lint-baseline.json` (about 1,502 entries per ledger `:2479`, to be re-counted); `MP/go/pkg/cibuf/workflow_test.go:69-79`; `MP/proto/README.md`; NEW `scripts/tests/test_buf_lint_ratchet.py`.
- **Tests:** `cd "apps/Model Plane/go/pkg/cibuf" && go test ./...`; `python3 -m unittest scripts/tests/test_buf_lint_ratchet.py`; CI self-proof step (delete a field in a proto copy, assert `buf breaking` fails, as `sse-taxonomy.yml:49-62` does); throwaway branch plus `gh run list`; a deliberate `pnpm typecheck` error turns `spa-verify` red.
- **Evidence:** `gh run list` URLs for verevonv3-ci, model-plane-rust-ci, contract-goldens, buf and sse-taxonomy; one red and one green buf run; the baseline-file diff; the CI baseline table with an owner per pre-existing red; request to the repo owner to change the default branch (D1(c), an owner action).
- **Risks:** first runs are red for pre-existing reasons (12 vitest files, `@quarry/client` typecheck: project memory, not re-verified); each gate runs `continue-on-error: true` for 3 runs, then blocks.

### PR-3: `feat(model-plane): run-context contract on RunAgentRequest, no behaviour change`
- **Branch:** `feat/mp-ws02-s4-run-context-contract`. Slice WS02.S4, effort M (3-5 d); decision-free (inert until WS02.S6). Open after PR-1 (live `sessions.proto`, `grpc.rs`); its DoD "buf breaking green" needs PR-2.
- **Files:** `MP/proto/model_plane/v1/execution.proto` (`RunAgentRequest.run_context = 18`; RunContext 1-4, RunContextStatus, RunContextPart, enum RunContextState; 5 and 6 left for WS08 and WS20); `sessions.proto` (`rpc RecordRunContext` appended after live `CountWaivedRuns`; if importing `RunContextStatus` is circular, declare a session-named duplicate); NEW `MP/rust/crates/mp-contracts/src/run_context.rs` (`MAX_AUTHORED_INSTRUCTION_BYTES = 16384`, `MAX_HISTORY_MESSAGES = 24`, `MAX_HISTORY_BYTES = 65536`, `MAX_MEMORY_BYTES = 8192`, `validate(&RunContext, zdr)`, `status_json`) plus `pub mod` in `lib.rs`; SC `grpc.rs` handler beside `set_run_mode` (JSONB merge of `runs.metadata.run_context`, no migration); EC `grpc.rs:1237` call `validate` after `enforce_persistence_free_execution` (refusal = `invalid_argument` with a closed token); fix the 27 `RunAgentRequest` literal sites (EC `agent_route.rs:263,273`, `grpc.rs:1873,1959`, `agent.rs` 13 sites, MG `sse.rs` `build_run_agent_request` and tests, `tests/e2e_invoke_chain_test.rs`); regenerate Go bindings and force a Rust proto rebuild (`mp-contracts/build.rs:74-94` fingerprint caveat).
- **Tests:** `cargo test -p mp-contracts` incl. NEW `tests/run_context_posture.rs` (table over zdr/waived/persistent by instructions/history/memory); session-core `record_run_context_merges_without_dropping_other_keys` (+ ignored PG variant); EC `a_zdr_run_with_history_is_refused_at_the_boundary`; new `RunAgentRequest` wire-parity golden in `proto_wire_parity_execution.rs` and the Go parity test; `buf breaking` additive; `bash scripts/tests/zdr-contract-sync.test.sh`.
- **Evidence:** rebuild session-core, execution-core, model-gateway with `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service>`; run an existing Do flow unchanged (regression); call `RecordRunContext` and capture the `runs.metadata` JSON; record tag 18 in the 4.3 ledger entry.
- **Risks:** Rust proto rebuild fingerprint; `RunContextStatus` import cycle; concurrent `sessions.proto` edits.

**Day-1 parallel starters, not counted** (Lane A, no Model Plane Rust, no live-file overlap): WS01.S1 then S2 (CP `observability/rules/audit-delivery.rules.yml`, `model-session-core` scrape alias in `MP/deploy/docker-compose.yml`, `scripts/rebuild-service.sh`, `scripts/tests/audit-delivery-proof.sh`, replay evidence file; the session-core build comes from a clean detached worktree at `mp-baseline-0`) and WS01.S3 (auth-core membership audit outbox, `organization-events.plugin.ts:732-763, 828-829`; no deps; diff failing jest sets against HEAD, 15 pre-existing failures).
