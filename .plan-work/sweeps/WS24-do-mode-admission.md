# WS24-do-mode-admission: code map for F-t3-025 and F-t3-030

Sweep date 2026-10-15. Read-only. Trees:
- Worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`, HEAD afb83928. `git status`: only untracked `.plan-work/`.
- Live tree `C:/dev/CoresSystem`, per `.plan-work/live-tree-status.txt` (324 entries). Line numbers below are worktree unless marked "live".
- Audit cards in `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`: F-t3-025 at :716-727, F-t3-030 at :776-787.
- Wave rows: :1567 (F-t3-025, W2, M, deps F-t3-003, F-t3-006, "AIF M1 trace") and :1569 (F-t3-030, W2, M, deps F-t3-003, OD2). Quick wins: :1673 and :1674. Appendix B: :1842 and :1847.
- Appendix C and D: no entries for either id (both are "Recommended", not "Priority"). Grep confirmed.

## 0. Findings

- F-t3-025 citations: 3 of 3 MATCH.
- F-t3-030 citations: 4 rows. Behaviour matches in all four. Two need range corrections (store.go, permission/mod.rs). Two are region matches inside agent.rs, which is modified in live (+7 lines in the catalogue region).
- Counts verified: 168 action ids in `action-registry.ts` (grep count of `id:` lines). 8 model-executable ids at `model-eligibility.ts:126-135`. One owner-receipt contract (`tickets.create`) in `catalog-manifest.ts:36-41`.
- The Control decision (`run_action_decision.go`) is generic in shape. It carries `action_schema_hash` (:76, :114, :142). `runActionTarget` (:340-347) admits only `tickets.create`. The only model-callable owner tool is `ticket_tools.rs` (TOOL_NAME gated by `requires_durable_owner_approval`).
- Execution-core has four classifiers, not one:
  - `is_risky_tool` (name substring, :229)
  - `is_risky_call` (argument-aware, :195)
  - `is_restricted_context_write` (only `save_memory`, :181-183)
  - `requires_durable_owner_approval` (only the ticket tool, :77)
- `rung_required_for` (:100-108) returns ReadOnly for any name that is neither risky nor `save_memory`. Under `ask`, `evaluate_call` (:43-70) gates `is_risky_call` plus the owner-approval and consent sets (:53-58). So an edit or write name missing from `RISKY` (:230-279) is Allowed and classed ReadOnly. The card's fix (add ids to `is_risky_tool`) classes them DangerFullAccess instead. Choosing WorkspaceWrite (via `is_restricted_context_write`) versus DangerFullAccess (via RISKY) is a design fork the card does not mention.
- Durable workspace merge is not wired. `SnapshotSandbox` writes run overlay rows (`store.go` UpsertOverlay :104-123). `PromoteWorkspace` has no production caller in execution-core: `sandbox_manager_client.rs:314-328`, with only a test call at :826.
- Name collision: `execution-core/src/workspace_promote.rs` is the Data Plane v2 document-promotion client (S3.3 step 5). It is not the sandbox-manager merge.
- Stale doc comment: `workspace_hydrate.rs:6-14` says hydrate and diff are "not wired into a real caller". They are wired by `sandbox_lease.rs:230` (hydrate) and `:503` (diff_and_upload), and by the code_interpreter arm at `runtime_loop/mod.rs:604-619`.
- Literal gate: `grpc.rs:864-873` demands the delegated sandbox bearer only when `tool_name == "code_interpreter"` and a Space id is set. A new Space file tool that needs a sandbox lease needs the same handling, or AcquireLease fails.
- Redaction hash domain: `diff_and_upload` hashes the redacted bytes (`workspace_hydrate.rs:116-117`) and compares against the baseline (:119-122). The CAS `put` returns sha256 of the same bytes (`workspace_cas.rs:141-143`), so the upload domain matches. Whether an unchanged file re-scrubs to its baseline hash depends on `scrub_string` being idempotent. `scrub.rs` was not read, so this is NOT DETERMINED.
- Live tree has a large uncommitted Z-18 (own-connection ZDR waiver) change set in the same execution-core files. `runtime_loop/mod.rs` threads `waived: bool` through `execute_step_inner` (live :484). Also touched: agent.rs, grpc.rs, agent_route.rs, auth.rs, knowledge_tools.rs. F-t3-030 says ZDR runs write nothing durable, so this needs coordination.
- Coordination with WS05. Its design (`.plan-work/designs/WS05-declared-tool-classes.md:238, :250-251`) says new tools need a capability row, a class and an attestor. It says permission/mod.rs and capability_policy.rs should gain class lookup "not new name lists". That conflicts with the card's name-list approach. WS05 :238 also names "WS30" as the workspace file tools owner, which differs from this task's WS24 scope.

## 1. Citation verdicts

| card | cited path:lines | claim (short) | worktree verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| F-t3-025 | apps/Frontend Plane/verevonv3/src/shared/actions/types.ts:15-24 | ActionDescriptor carries risk, requiresApproval, reversible | MATCH | clean | :15-25 (risk :20, requiresApproval :21, reversible :22) |
| F-t3-025 | apps/Frontend Plane/verevonv3/src/shared/actions/model-eligibility.ts:126-140 | isModelExecutableAction admits 8 of 168 | MATCH (8 ids; 168 registry ids) | clean | :126-135 (set of 8), :137-139 (function) |
| F-t3-025 | apps/Control Plane/user-core/internal/spaces/run_action_decision.go:340-347 | runActionTarget accepts only tickets.create | MATCH | clean | :340-347 unchanged |
| F-t3-030 | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4090 | no read/grep/glob/edit/write tool in either catalogue | MATCH by region: :4090 is the closing brace of the process_read definition inside process_tool_defs (fn :4079-4107) | M (live +7: live :4097) | catalogues: process_tool_defs :4079-4107; offered_tool_defs starts :4109 (end not read) |
| F-t3-030 | agent.rs:4190-4222 | (same claim) web, browser and code_interpreter definitions | MATCH by region (web.read :4192, browser.act :4202, code_interpreter :4216, save_memory :4228; no file tool in span) | M (live +7: :4197-4229) | :4190-4222 (worktree) |
| F-t3-030 | apps/Model Plane/go/services/sandbox-manager/internal/workspace/store.go:100-175 | Promote merges overlay with per-path base_hash CAS | MATCH (behaviour); range truncated | clean | UpsertOverlay :104-123; Promote :158-217 (CAS SQL :195-208; conflicts :212-216) |
| F-t3-030 | apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:229-275 | is_risky_tool substring match misses fs.edit | MATCH; range truncated | clean | fn :229; RISKY list :230-279; match loop :283-284; no "edit" keyword |

Not cited with a path in the cards (resolved anyway): `trusted_capability_id` at `capability_policy.rs:741` (match at :766, code_interpreter to cap.command.sandbox at :773). `diff_and_upload` at `workspace_hydrate.rs:102`. `isModelExecutableAction` callers at `agent-tools.ts:120,128`, `catalog-manifest.ts:54`, `context-pack.ts:72`.

## 2. Affected-file map

Line counts are `wc -l` of the worktree file. "n/c" = not counted. Live status: clean, M (modified), ?? (live-only), D (deleted).

### A. Typed action registry and admission (Frontend Plane)

| path | role | lines | live | existing tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts | 168 action definitions (grep count of `id:`) | 3151 | clean | action-registry.test.ts (:39 comment: keep allowlist explicit) |
| apps/Frontend Plane/verevonv3/src/shared/actions/types.ts | ActionDescriptor type, ActionActor, ActionExecution | 50 | clean | none direct |
| apps/Frontend Plane/verevonv3/src/shared/actions/model-eligibility.ts | MODEL_EXECUTABLE_ACTION_IDS (:126-135); isModelExecutableAction (:137-139); evidence comments (:3-125) | 139 | clean | catalog-manifest.test.ts:23-50; agent-tools.test.ts:93 |
| apps/Frontend Plane/verevonv3/src/shared/actions/catalog-manifest.ts | ACTION_CATALOG_SCHEMA_VERSION = 2 (:6); executionContract per id (:32-46, only tickets.create is owner_operation_receipt); allowedActorTypes (:54); sort and duplicate check (:74-84) | 109 | clean | catalog-manifest.test.ts:10-71 (determinism, eight-id list :35-44, tickets.create contract :52-60) |
| apps/Frontend Plane/verevonv3/src/shared/actions/agent-tools.ts | createVerevonActionToolSpecs (:112-123), createVerevonActionToolDefinitions (:125-137), createSelectedAgentToolSpecs (:139) | 181 | clean | agent-tools.test.ts (:88 non-allowlisted never falls through; :93) |
| apps/Frontend Plane/verevonv3/src/shared/context-packs/context-pack.ts | buildModelContextPack filters by isModelExecutableAction (:72) | 76 | clean | context-pack.test.ts |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | sends request `tools` built by createSelectedAgentToolSpecs (:943); agentic sent only in plan mode (:1048 comment) | n/c | M (+53: retrieval notice, ZDR feature field) | chat-client.test.ts |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions.rs | routes /api/v1/actions/catalog (:15), /execute (:18), /tickets/create/{idempotency_key} (:20) | 24 | clean | none |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs | execute_action dispatch (:385, to dispatch_browser_run_action) | n/c | clean | none |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs | tickets.create forged-call denial and receipt echo; required_delegation "control_target_action_decision" (:6061) | 6402 | clean | ticket_receipt_is_refused_when_the_owner_returned_no_operation (:6242); ticket_receipt_is_refused_when_the_operation_is_not_completed (:6250); ticket_receipt_is_refused_without_a_durable_audit_event (:6263); ticket_receipt_is_refused_when_owner_identifiers_are_blank (:6275); ticket_receipt_echoes_only_owner_issued_identifiers (:6287) |
| apps/Frontend Plane/verevonv3/apps/gateway/src/upstream.rs:2371 | active_membership_uses_control_role_and_requires_exact_scope (named in model-eligibility comments) | n/c | clean | itself |

### B. Control Plane (user-core) and Application Plane (conversation-core-go)

| path | role | lines | live | existing tests |
|---|---|---|---|---|
| apps/Control Plane/user-core/internal/spaces/run_action_decision.go | run-action-v1: sign (:280), envelope verify (:293), RunActionDecision with action_schema_hash (:76, :114, :142), runActionTarget (:340-347) | 360 | clean | run_action_decision_test.go: TestIssueRunActionDecisionIsTargetBoundWithoutWideningThreadAuthority (:38); TestIssueRunActionDecisionFailsClosedOnStaleOrIneligibleAuthority (:66); TestValidateCurrentRunActionDecisionRejectsSupersededControlFacts (:108); TestVerifyRunActionDecisionRejectsTamperingAndExpiry (:148) |
| apps/Control Plane/user-core/internal/spaces/model_action_view.go | Control-signed model action view (control view token), requested by execution-core | 204 | clean | not located |
| apps/Control Plane/user-core/internal/spaces/owner_effect_reservation.go (+ _test.go, _live_test.go) | reserve before owner effect ("reserve -> commit" in model-eligibility comment) | n/c | clean | owner_effect_reservation_test.go; owner_effect_reservation_live_test.go |
| apps/Control Plane/user-core/internal/http/server.go | POST /api/v1/internal/spaces/run-action-decision (:377); run-action-authority-check (:378) | 724 | clean | service_auth_test.go:164-165; authz_facade_test.go:240, :290 |
| apps/Control Plane/user-core/internal/http/service_auth.go | path-to-scope mapping for the two routes (:128, :130) | n/c | clean | service_auth_test.go |
| apps/Control Plane/user-core/migrations/024_space_agent_action_effect_policy.up.sql | ALTER space_effect_policies ADD agent_action_entitled (:4-5) | n/c | clean | none |
| apps/Control Plane/user-core/migrations/027_space_sandbox_capability_effect_policy.up.sql | ADD sandbox_capability_entitled (:5-6); a Space gets no sandbox lease by existing (:1) | n/c | clean | none |
| apps/Application Plane/conversation-core/conversation-core-go/internal/http/run_action_decision.go | owner-side verify of run-action-v1 (version const :16; envelope check :123) | n/c | clean | run_action_decision_encoding_test.go |
| apps/Application Plane/conversation-core/conversation-core-go/internal/http/run_action_authority_client.go | calls Control run-action-authority-check (:16) | n/c | clean | run_action_authority_client_test.go:18 |
| apps/Application Plane/conversation-core/conversation-core-go/internal/http/agent_ticket_operation*.go | durable ticket operation; Control reservation committed before owner effect | n/c | clean | TestAgentTicketOperationCommitsControlReservationBeforeOwnerEffect (agent_ticket_operation_test.go:163); ...NeverWritesWhenControlCommitIsDenied (:217); ...RequiresExactControlDecisionAndUsesItsAuthority (:355); ...RejectsReboundPayloadBeforeOwnerEffect (:449) |
| apps/Application Plane/conversation-core/conversation-core-go/internal/http/action_contract.go | RequiredDelegation "control_target_action_decision" (:97) | n/c | clean | action_contract_test.go:58 |
| apps/Application Plane/conversation-core/conversation-core-go/internal/database/migrations/028_ticket_operations.sql, 030_conversation_agent_action_grants.sql, 031_ticket_operation_reservations.sql | owner tables for ticket operations, agent-action grants, reservations | n/c | clean | none |

### C. Capability gate and execution-core (Model Plane)

| path | role | lines | live | existing tests |
|---|---|---|---|---|
| apps/Model Plane/go/services/capability-core/internal/api/model_action_view.go | run-view route (:17); model-action-view-v1 (:18); resolveModelActionView (:107); ControlModelActionViewVerifier (:93), Verify (:180) | 233 | clean | model_action_view_test.go: TestModelActionViewFailsClosedWithoutControlVerifier (:24); TestModelActionViewIsRunBoundAndUnavailableUntilOwnerHealthIsAttested (:34). cmd/model_action_view_wiring_test.go: TestModelActionViewVerifierFromEnvFailsClosedWhenControlPublicKeyIsMissing |
| apps/Model Plane/go/services/capability-core/internal/authz/authz.go:87 | path-specific authz for the run-view route | n/c | clean | capability-core authz tests not located |
| apps/Model Plane/go/services/capability-core/migrations/0010_sandbox_code_execution_capability.up.sql | seeds cap.command.sandbox (INSERT :58; id :82; dispatch binding :92) | 130 | clean | none |
| apps/Model Plane/go/services/capability-core/migrations/0015_background_process_capability.up.sql | seeds cap.process.background (LOW, per capability_policy.rs:774-781) | 162 | clean | none |
| apps/Model Plane/rust/services/execution-core/src/capability_policy.rs | trusted_capability_id (:741; match :766; code_interpreter to cap.command.sandbox :773); evaluation fails closed without a binding (:361); model action view request (:276-292; scope capability:model-action:view :284; path :291) | 1051 | clean | bindings_are_exact_and_unknown_or_dynamic_tools_fail_closed (:940); decision_evidence_verifies_and_binds_the_dispatch_tuple (:907); decision_evidence_rejects_tampering_and_expiry (:921); absent_capability_decision_key_is_allowed_only_for_dev_mode (:896) |
| apps/Model Plane/rust/services/execution-core/src/ticket_tools.rs | only model-callable owner action; CONTROL_DECISION_PATH (:34); request_model_action_view (:391); request_control_decision (:437); request_ticket_operation (:478); reconcile (:571); execute_if_configured (:635) | 1205 | clean | model_action_view_requires_its_own_control_scope_token (:864); payload_digest_matches_the_owner_contract_shape (:880); approved_continuation_rejects_schema_or_retry_key_drift_before_network (:890); two_hop_execution_requests_control_then_owner_and_returns_only_receipt (:1010); ambiguous_owner_response_reconciles_without_replaying_the_effect (:1071); ambiguous_owner_response_with_no_receipt_is_safe_to_retry (:1129); a_control_denial_never_calls_the_owner (:1173) |
| apps/Model Plane/rust/services/execution-core/src/permission/mod.rs | evaluate (:31); evaluate_call (:43-70); requires_durable_owner_approval (:77); rung_required_for (:100-108); check_autonomy_rung (:124); requires_consent_to_disclose (:160); is_restricted_context_write (:181-183); is_risky_call (:195-199); is_risky_tool (:229; RISKY :230-279) | 701 | clean | tests :291-682 include restricted_context_writes_are_exactly_the_memory_write (:299; checks save_memory true, recall_memory and knowledge_search false; does not test the whole set); external_mcp_tools_are_gated_under_ask (:383); social_publish_is_gated_and_account_listing_is_not (:399); code_interpreter_runs_ungated_while_shell_stays_gated (:415); provider_action_gating_is_operation_aware (:450); workspace_write_permits_a_durable_write_and_still_refuses_an_outbound_action (:643); read_only_refuses_everything_plan_mode_refuses (:682) |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | purpose-lock catalogue: process_tool_defs (:4079-4107), offered_tool_defs (:4109-, end not read); merged_tool_defs (:622); un-offered tool refusal (:1611); depth>0 risky or restricted refusal (:1641-1642); rung gate (:1688); restricted-write check again (:1802-1803); consent (:4447, :4460); owner-approval permission mode (:4567) | 13328 | M (+207; Z-18 route threading) | every_offered_tool_has_a_parameter_schema_that_is_a_json_object (:3545); the_real_offered_toolset_keeps_the_guidance_it_had (:3758); every_offered_tool_has_a_capability_binding (:3805); every_offered_tool_capability_has_an_attestor (:3838); only_the_server_resolved_view_may_offer_ticket_create (:5214); plan_mode_refuses_a_risky_tool_at_the_top_level_even_under_auto_mode (:10864); offered_tool_defs_has_no_duplicate_tool_names (:11577) |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs | execute_step (:278); execute_step_inner (:455); permission gate evaluate_call (:557); dispatch arms: code_interpreter (:597-623), process tools (:624), web search (:629); execute_code_interpreter (:891); execute_process_tool (:921) | 2838 | M (+18: `waived` param, live :484) | not enumerated |
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | execute_step RPC (:828); sandbox_bearer literal gate (:864-873); reject_direct_owner_action (:700-712, uses requires_durable_owner_approval at :707) | 2067 | M (+98; live literal at :882) | not enumerated |
| apps/Model Plane/rust/services/execution-core/src/agent_route.rs | validate_run_route (:113-131; OD2 refusal :120-124; provider check :126-130) | 482 | M (+285; live fn at :122) | not enumerated |
| apps/Model Plane/rust/services/execution-core/tests/autonomy_ladder_contract.rs | source-text contracts: rung from call args (:68); gate in loop not catalogue (:85); unstated rung unconstrained (:108); grant validated both sides (:135) | 155 | clean | the four tests above |
| apps/Model Plane/rust/services/execution-core/src/process_tools.rs | background-process family (five names, one capability); pattern for non-risky names | 549 | clean | the_family_is_exactly_the_five_dispatchable_names (:482); no_family_name_trips_the_risky_substring_gate (:499) |
| apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs | per-call hermetic code tool, LOW capability; deliberately not risky (module doc :20-22); run (:483) | 1347 | clean | the_result_json_shape_is_stable (:763); empty_code_is_rejected (:792); unsafe_files_in_names_are_rejected_never_sanitized (:824); duplicate_and_reserved_files_in_names_are_rejected (:850); workspace_names_are_unique_and_path_safe (:922) |

### D. Workspace overlay and sandbox (Model Plane)

| path | role | lines | live | existing tests |
|---|---|---|---|---|
| apps/Model Plane/rust/services/execution-core/src/workspace_hydrate.rs | hydrate (:68-88); diff_and_upload (:102-132; redact before hash :116-117; base_hash compare :119-122; CAS put :123); redact (:146-151); sha256_digest (:153-155); safe_relative_path (:160-174); stale header :6-14 | 382 | clean | hydrate_writes_files_and_returns_baseline (:212); hydrate_rejects_unsafe_paths_without_touching_cas (:238); diff_and_upload_skips_unchanged_files_without_uploading (:261); diff_and_upload_uploads_changed_and_new_files (:281); diff_and_upload_redacts_before_uploading (:317); diff_and_upload_preserves_binary_content_unchanged (:341) |
| apps/Model Plane/rust/services/execution-core/src/workspace_cas.rs | CasClient.put returns sha256:<hex> of the uploaded bytes (:140-143); get (:172); digest validation (:281-292) | 422 | clean | hex_digest_matches_known_sha256 (:344); validate_digest_accepts_only_well_formed_sha256 (:352) |
| apps/Model Plane/rust/services/execution-core/src/sandbox_lease.rs | ensure_sandbox_lease (:106); ensure_hydrated_workspace (:183; GetWorkspaceManifest :210; hydrate :230); workspace_dir (:245); upload_and_discard_workspace (:495; diff_and_upload :503; SnapshotSandbox :517, :529) | 888 | clean | a_hydrated_workspace_is_reused_by_a_second_call_without_touching_the_network (:700); hydrating_a_new_workspace_without_cas_configured_fails_closed (:733); releasing_a_lease_with_no_cas_configured_still_releases_and_cleans_up (:762) |
| apps/Model Plane/rust/services/execution-core/src/sandbox_manager_client.rs | defs: snapshot_sandbox (:247), get_workspace_manifest (:278), promote_workspace (:314-328, no production caller) | 852 | clean | test calls at :794, :802, :818, :826 |
| apps/Model Plane/rust/services/execution-core/src/workspace_promote.rs | Data Plane v2 document promotion (S3.3 step 5), not the PromoteWorkspace merge; ZDR refusal now Refusal::DURABLE_MUTATION_FORBIDDEN (live) | 499 | M (+7) | not enumerated |
| apps/Model Plane/rust/services/execution-core/src/scrub.rs | secret scrub; scrub_string used by redact | 376 | clean | not read |
| apps/Model Plane/rust/services/execution-core/src/state.rs | comment: the run's eventual diff_and_upload (:91) | n/c | clean | none |
| apps/Model Plane/go/services/sandbox-manager/internal/workspace/store.go | UpsertOverlay (:104-123); Promote (:158-217; per-path CAS :195-215; conflicts :212-216) | 217 | clean | store_test.go: TestGetManifestRejectsMissingIdentifiers (:93); TestGetManifestReturnsTheLayeredView (:112); TestUpsertOverlayRejectsMissingIdentifiers (:147); TestUpsertOverlayRejectsAFileMissingPathOrHash (:163); TestUpsertOverlayWritesOnePerFileWithBaseHash (:176); TestUpsertOverlaySurfacesAWriteFailure (:206); TestPromoteRejectsMissingIdentifiers (:222); TestPromoteUpsertsOnePerOverlayPathAndReportsOnlyTheConflictingOne (:251); TestPromoteReturnsNoConflictsWhenOverlayIsEmpty (:294); store_integration_test.go (setupWorkspaceDB :22) |
| apps/Model Plane/go/services/sandbox-manager/internal/server/server.go | SnapshotSandbox (:204); GetWorkspaceManifest (:282); PromoteWorkspace (:316) | 351 | clean | server_test.go: TestGetWorkspaceManifestAndSnapshotSandboxLayerTheOverlayOverTheSpace (:501); TestPromoteWorkspaceReportsConflictWithoutOverwriting (:613); TestPromoteWorkspaceMergesNonConflictingPathsIndependently (:662); TestPromoteWorkspaceStillWorksAfterReleaseLease (:724); TestPromoteWorkspaceRejectsANonSpaceLease (:790); TestSnapshotSandboxSkipsTheOverlayWriteForANonSpaceLease (:805) |
| apps/Model Plane/go/services/sandbox-manager/internal/authz/authz_test.go:74 | TestActivateLeaseAndGetWorkspaceManifestAreKnownMethods | n/c | clean | itself |
| apps/Model Plane/go/services/sandbox-manager/migrations/0001_workspace_manifest.up.sql | workspace_files table (:9-22); identity unique index (:24-25); space and run partial indexes (:27-31) | 31 | clean | none |

### E. Model-gateway and SSE

| path | role | lines | live | existing tests |
|---|---|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | plain-chat read-only router: dispatch_tool (:2406), dispatch_tool_with_cost (:2467) | 12717 | M (+425; retrieval_notice events) | context_recovery_contract.rs:67 reads the dispatch arm |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | ChatEvent (:105); ToolCall (:119); ToolResult (:125); family "tools" (:355); wire tool_call and tool_result (:403-404) | 1899 | M (+203; RetentionClass) | in-file tests near :1085-1497; .github/workflows/sse-taxonomy.yml (clean) |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | run stream (ZDR and waiver turn logic, live) | large | M (+883) | not read |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs | test double; live adds user_bearer_with_waiver | 1158 | M (+537) | not read |

## 3. Related contracts

- Proto, `apps/Model Plane/proto/model_plane/v1/`:
  - sandboxes.proto: rpc SnapshotSandbox (:20), GetWorkspaceManifest (:33), PromoteWorkspace (:44; compare-and-swap comment :37). SnapshotRequest (:179) with changed_files (:193). SnapshotResponse (:196). GetWorkspaceManifestRequest (:217). WorkspaceManifestEntry (:225). WorkspaceChangedFile (:239; base_hash :254). PromoteWorkspaceRequest (:257). PromoteWorkspaceResponse (:265).
  - execution.proto: RunAgentRequest (:76); AGENTIC_SUBSCRIPTION comment (:190).
  - inference.proto: ToolDefinition (:178); ToolCall (:188).
  - sessions.proto: live M (+42): SessionRetentionService and CountWaivedRunsRequest.
- Generated copies to regenerate with any proto change (status not determined, not read): `apps/Model Plane/go/gen/model_plane/v1/sandboxes.pb.go`; `apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/sandboxes_pb2.py`; `apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/sandboxes_pb.ts` and `sandboxes_connect.ts`.
- SQL migrations, latest file names (directory listings):

| directory | latest | notes |
|---|---|---|
| apps/Model Plane/go/services/sandbox-manager/migrations | 0004_audience_revision_ceiling.up.sql (+ .down.sql) | workspace_files in 0001_workspace_manifest.up.sql |
| apps/Model Plane/go/services/capability-core/migrations | 0015_background_process_capability.up.sql | precedents: 0010_sandbox_code_execution_capability, 0014_memory_tool_capabilities |
| apps/Model Plane/rust/services/session-core/migrations | worktree: 0043_audit_outbox_requeue.sql | live adds ??: 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, revert/ |
| apps/Control Plane/user-core/migrations | 029_mail_task_addresses.up.sql | entitlement precedents 024, 025_owner_effect_reservations, 027, 028 |
| apps/Application Plane/conversation-core/conversation-core-go/internal/database/migrations | 031_ticket_operation_reservations.sql | 028_ticket_operations, 030_conversation_agent_action_grants |

- Config and env:
  - AGENTIC_SUBSCRIPTION_ENABLED (default false): apps/Model Plane/deploy/docker-compose.yml:378, :847, :1038. Constant ENABLED_ENV at rust/crates/mp-contracts/src/agentic_subscription.rs:41.
  - AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS: docker-compose.yml:379, :1039.
  - OD2 enforcement: agent_route.rs:120-124 (`agentic_subscription_disabled`). Gate table at `.plan-work/common/flags-and-config.md:115-133`. OD2 row at `.plan-work/common/rules-ledger-adrs.md:248`.
- HTTP and gRPC routes:
  - Gateway: /api/v1/actions/catalog, /api/v1/actions/execute, /api/v1/actions/tickets/create/{idempotency_key} (domains/actions.rs:15-20); /api/v1/spaces/{space_ref}/actions (domains/spaces.rs:666).
  - Control internal: /api/v1/internal/spaces/run-action-decision (user-core server.go:377); /api/v1/internal/spaces/run-action-authority-check (:378).
  - Capability-core internal: /api/v1/internal/model-actions/run-view (model_action_view.go:17; authz.go:87).
  - Sandbox-manager: gRPC only (sandboxes.proto).
- SSE event types: ChatEvent tool_call and tool_result (sse_events.rs:403-404; family "tools" :355). Live-only additions: retrieval_notice (tool_loop.rs live; chat-client.ts live hunk :246, ChatRetrievalNoticeEvent) and RetentionClass (sse_events.rs live).
- Frontend action contracts: src/shared/actions/action-registry.ts (168 ids), types.ts, model-eligibility.ts, catalog-manifest.ts (schema v2), agent-tools.ts. Reaches the model through chat-client.ts:943 (request `tools`).
- ZDR refusal contract (live-only, ??): apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256); apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json (+ .sha256). Committed siblings: connection-zdr-capabilities.json, zdr-posture-matrix.json, infer-statuses.json.
- Decision ledger: apps/Model Plane/docs/decisions/ledger.md:3609 (`process_write` rejected: is_risky_tool substring would route it to ask on the name alone) and :3986-3987 (process_stdin naming rationale). Also :3964 (trusted_capability_id maps the five process tools).

## 4. Test and build commands

- Rust execution-core: `cd "apps/Model Plane/rust" && cargo test -q -p execution-core --all-targets`.
  - Crate name from services/execution-core/Cargo.toml:2. Workspace members in rust/Cargo.toml:3-17 (includes crates/mp-contracts and services/execution-core).
  - CI: model-plane-rust-ci.yml:57 (working-directory), :105 `cargo check --workspace`, :112 `cargo check --workspace --all-targets`, :119 `cargo test --workspace --no-fail-fast`. CI file is live M.
  - Memory note: on Windows two cargo targets fail pre-existing, use `--lib`.
- Go sandbox-manager: `cd "apps/Model Plane/go/services/sandbox-manager" && go test ./... && go vet ./...`. Module github.com/triodelab/model-plane/services/sandbox-manager (go.mod:1, go 1.25.1). No CI workflow found for this service.
- Go capability-core: `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`. Module .../services/capability-core (go.mod:1). No CI workflow found. Memory note: Go -race needs cgo, unavailable here; no go.work, build per service.
- Control user-core: `cd "apps/Control Plane/user-core" && go test -v ./...` (CI user-service.yml:31, :67). Module (go.mod:1). Coverage target `make test-coverage` per root CLAUDE.md.
- Application conversation-core: `cd "apps/Application Plane/conversation-core/conversation-core-go" && go test -v ./...` (CI conversation-core-service.yml:27, :41).
- Gateway BFF (Rust): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md). Crate verevon-gateway-rs (Cargo.toml:2). CI verevonv3-ci.yml:138 (`cargo clippy --all-targets -- -D warnings`) and :141 (`cargo test`).
- Frontend (verevonv3, pnpm@11.3.0): `cd "apps/Frontend Plane/verevonv3" && pnpm test` (package.json:13: node --test gateway scripts, then vitest run); `pnpm typecheck` (:17, tsc -b); `pnpm lint` (:11, eslint .); `pnpm build` (:9, typecheck then vite build; CI verevonv3-ci.yml:177, :211). Focused run: `pnpm vitest run src/shared/actions src/shared/context-packs`. package.json is live M (+2 demo scripts).
- Proto and contracts: `buf generate` (Model Plane CLAUDE.md); `buf lint` has pre-existing debt. `.github/workflows/buf.yml` exists (breaking check; contents not read).
- Ratchets: catalog-manifest.test.ts:23-50 (eight-id list) and autonomy_ladder_contract.rs (source-text contracts).

## 5. Owners

- Action registry, catalog, agent tool specs (src/shared/actions): Frontend Plane. Root CLAUDE.md: "Verevon v3 action contracts live in src/shared/actions; add or update the action contract before wiring a meaningful UI operation."
- BFF gateway domains/actions: Frontend Plane. Root CLAUDE.md: gateway domains must not leak upstream secrets, raw OAuth tokens, or forged scoping headers.
- Run-action decision issuance and current-authority checks: Control Plane user-core. Root CLAUDE.md: Control owns identity, users, orgs, billing, sessions, audit, quotas and entitlements. Entitlement columns at user-core migrations 024 and 027.
- Owner receipts and ticket operations: Application Plane conversation-core (model-eligibility comments; conversation-core-go internal/http). master-ownership-matrix.md:166 (rule 10): Application services may cache or mirror membership but must revoke projections.
- Model action view and capability binding: Model Plane capability-core. capability-ownership-matrix.md:59 (tool and command registries to capability-core); :60 (policy and permissions to capability-core policy/engine.go; execution-core permission/ is enforcement only). master-ownership-matrix.md:130.
- Permission gate, tool loop, purpose-lock, hydrate and diff, code_interpreter, process tools: execution-core. master-ownership-matrix.md:128. capability-ownership-matrix.md:62 (hook firing), :64 (secret scrub), :68 (process lifecycle, process_host.rs).
- Durable workspace rows and overlay merge (workspace_files, UpsertOverlay, Promote): sandbox-manager. Inferred from capability-ownership-matrix.md:65 (sandbox lease lifecycle) and :67 (background process rows). No explicit row for workspace_files exists: NOT DETERMINED.
- Approvals: session-core (capability-ownership-matrix.md:55) versus orchestrator-core (master-ownership-matrix.md:129). Conflict already flagged in `.plan-work/sweeps/WS13-approvals-v2.md:204`; not resolved here.
- Do-mode gate (OD2): enforcement in execution-core agent_route.rs; decision state open per `.plan-work/common/rules-ledger-adrs.md:248`.
- ZDR posture: Control decides; model-gateway, inference-core and execution-core enforce. master-ownership-matrix.md:90 and :177 (rule 17, waiver row). HEAD text says "designed, not built".

## 6. Uncommitted-only (live tree)

Live file-level stat from `git --no-optional-locks diff --stat` (changed lines) with a one-line summary.

Modified (M), relevant to WS24:
- apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (207): Z-18 waiver route threading (`ctx.route.is_waived()`), new test helpers; catalogue functions unchanged, shifted +7.
- apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs (18): `waived: bool` added to execute_step, execute_step_with_browser_grant, execute_step_with_subagent, execute_step_inner (live :484) and execute_knowledge_search.
- apps/Model Plane/rust/services/execution-core/src/grpc.rs (98): ZDR refusal mapping, per-user retention posture, enforce_agent_route changes.
- apps/Model Plane/rust/services/execution-core/src/auth.rs (631): parses connection-bound zdr_waiver claim (parse_zdr_waiver), retention imports.
- apps/Model Plane/rust/services/execution-core/src/agent_route.rs (285): validate_run_route resolves waived route via mp_contracts::retention::resolve_turn.
- apps/Model Plane/rust/services/execution-core/src/knowledge_tools.rs (131): waived retrieval filtered (zdr_mode reject).
- apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs (537): test double gains user_bearer_with_waiver.
- apps/Model Plane/rust/services/execution-core/src/workspace_promote.rs (7): DP v2 promotion ZDR refusal now Refusal::DURABLE_MUTATION_FORBIDDEN.
- apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs (425): retrieval_notice events; ZDR and waived content-safety.
- apps/Model Plane/rust/services/model-gateway/src/sse_events.rs (203): RetentionClass enum; retrieval notice family.
- apps/Model Plane/rust/services/model-gateway/src/sse.rs (883): large ZDR and waiver turn logic (not itemised).
- apps/Model Plane/proto/model_plane/v1/sessions.proto (42): SessionRetentionService, CountWaivedRunsRequest.
- apps/Model Plane/rust/crates/mp-contracts/src/lib.rs (2): adds pub mod retention and pub mod zdr_refusal.
- apps/Model Plane/docs/sse-event-taxonomy.md (4) and apps/Model Plane/docs/ZDR.md (M, not stat'd).
- apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts (53): retrieval notice event type; feature field on ZDR errors.
- apps/Frontend Plane/verevonv3/src/shared/actions/home-updates.ts (2): not read.
- apps/Frontend Plane/verevonv3/package.json (2): adds demo:build and demo:probe scripts.
- .github/workflows/model-plane-rust-ci.yml (50): new job "Session Core retention receipt (Postgres)".
- .github/workflows/verevonv3-ci.yml (55): new job "SPA - interactive demo bundle" (incl. Quarry SDK generation).

Live-only (??), relevant:
- apps/Frontend Plane/verevonv3/src/shared/actions/chat-work-period.ts and chat-work-period.test.ts (action-folder file, not in worktree).
- apps/Frontend Plane/verevonv3/src/shared/api/contracts/ (zdr-refusals.json, .sha256).
- apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256).
- apps/Model Plane/rust/crates/mp-contracts/src/retention.rs, zdr_refusal.rs; tests/zdr_retention_matrix.rs; tests/fixtures/.
- apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs.
- apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, revert/; session-core/src/retention_receipt.rs.

Clean (no live delta): all sandbox-manager, capability-core, user-core, conversation-core, gateway domains/actions, shared/actions except the two items above, permission/mod.rs, capability_policy.rs, ticket_tools.rs, workspace_hydrate.rs, workspace_cas.rs, sandbox_lease.rs, sandbox_manager_client.rs, code_interpreter.rs, process_tools.rs, scrub.rs, store.go, server.go, the migrations listed above, and the proto files except sessions.proto. Deleted (D): none relevant.

## 7. Coordination with other workstreams (shared files)

- WS05 design (`.plan-work/designs/WS05-declared-tool-classes.md`): :238 new tools not offered until they have a capability_tools row, a class and an attestor (contract for "WS24 and WS30 (workspace file tools)"). :250-251: agent.rs, runtime_loop/mod.rs, permission/mod.rs and capability_policy.rs are shared with WS24. "Class lookup is the seam; add predicates as floors or rows, not new name lists." Tool_class.rs is NEW in that design and absent from both trees. :20: approval_delivery_worker.rs:16-27 resumes only three tools (per design text; not verified in code).
- WS03 design: :176 effect.rs stub uses permission::is_risky_call (permission/mod.rs:195); :272 agent.rs, grpc.rs, mod.rs shared with WS24.
- WS02 design: :67 the Do loop does not offer reattach_context; :393 and :400 assign it to WS24 or WS25 with WS05; :411 and :414 execution.proto and agent.rs shared with WS24.
- WS08 design: :18 names "typed file tools (WS24)"; :272 read-only mounts and reserved name "attachments"; :296 agent.rs and mod.rs shared with WS24.
- WS10 design: :270 action-registry.ts, handlers.rs and dispatchers.rs are add-only shared with WS13, WS14, WS24.
- WS11 design: :315 execution.proto and agent.rs shared with WS24.
- WS13 sweep: :204 approvals owner conflict (session-core versus orchestrator-core).
- Live Z-18 work (section 6) touches the same execution-core files; rebase order matters.

## 8. NOT DETERMINED

- AIF M1, M2 and M3 milestones (card: "plan milestones M2 and M3"; row :1567 "AIF M1 trace"). No definition found in the audit doc or in verevonv3 (grep "AIF M[0-9]" empty).
- Whether model-gateway dispatch_tool (tool_loop.rs:2406) refuses side effects by construction. CLAUDE.md asserts it; only the signature was read.
- Whether ExecuteStep (grpc.rs:828) is on the Do-run path or only a direct RPC.
- Whether any Do run other than code_interpreter calls ensure_hydrated_workspace (only the code_interpreter arm at runtime_loop/mod.rs:604-619 was read).
- scrub_string idempotency (scrub.rs not read). Hence the redaction hash-domain consequence is unverified.
- Capability-core and sandbox-manager CI coverage (no workflow found by grep).
- Contents of: model_action_view.go (user-core), model_action_decision.go, owner_effect_reservation.go, capability-core authz.go tests, knowledge_tools.rs, auth.rs, sse.rs, session_core_rules_double.rs, home-updates.ts, buf.yml, and the generated copies in section 3.
- Whether the Frontend "apps/Frontend Plane/verevon" tree (a second frontend, with generated Model Plane clients) is live or used.
- Rung and permission semantics under Auto (chat) posture for new tools: only the Ask path was traced.
