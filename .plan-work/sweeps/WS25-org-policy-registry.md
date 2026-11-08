# WS25 Org agent policy registry: code-map sweep

Items: **F-t3-032** (Org agent policy registry; card: Partial, XL, W2; Control + Model + Frontend) and **F-t3-063** (Loop guard, durable org hooks, tool-output handles; card: Partial, L, W2, Model Plane, Medium confidence).
Audit cards: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` lines 801 (F-t3-032) and 1173 (F-t3-063). Wave rows: 1570 and 1573. Appendix B: 1849 and 1880. Appendix D: 2131 and 2144 (source line 2131 is truncated at "M (hub and frozen...").  Appendix C: no entry for either item (both are Recommended, not Priority).

Trees: **wt** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean apart from untracked `.plan-work/`). **live** = `C:/dev/CoresSystem` (same HEAD + 324 entries per `live-tree-status.txt`). Notation `wt / live`. Offset = live minus wt. The audit read the live tree: `runtime_loop/agent.rs` is 13,521 lines live and 13,328 in wt; the card line numbers match live.

Method: read-only. Symbol greps over `apps/Model Plane`, `apps/Control Plane`, `apps/Frontend Plane/verevonv3`. "ABSENT" means no symbol match in those trees (not proof of absence in untracked or generated code). No builds or tests were run.

---

## 0. Headline findings

1. **Hook rules already reach execution-core on two gateway paths.** `model-gateway/src/tools.rs:211` (code_interpreter ExecuteStep) and `model-gateway/src/browser_run.rs:417-427 wt` call `hook_context_json`. The Do/RunAgent loop passes `""` (`agent.rs` wt 1919-1923 / live 1923-1927). The card is right about the Do loop but omits the gateway wiring.
2. **Card F-t3-063 places the signatures "in mp-contracts". That is wrong.** `duplicate_call_signature` is a private fn in `model-gateway/src/tool_loop.rs` (wt 2304 / live 2328). `retrieval_signature` is a private fn in `execution-core/src/runtime_loop/agent.rs` (wt 3449 / live 3456). No crate under `apps/Model Plane/rust/crates` defines either. The two copies exist only because the loops are separate (see section 5).
3. **Card F-t3-032 omits an existing org-policy RPC surface.** `gateway.proto:161-166` defines RegisterHook, ListHooks, CheckPermission, SetPermission, GetPolicy, SetPolicy. Implemented in `model-gateway/src/grpc.rs` (wt 2043-2202 / live 2052-2211). `denied_tools` becomes a capability-core org-scope revoke (`/api/v1/capabilities/scopes/revoke`). Spend and token caps become Control Plane `org_quotas`. `allowed_models` is refused with Unimplemented (`unowned_policy_fields`, grpc.rs:154-161). `rate_limit_rpm` is process-local (grpc.rs wt 2269-2273). No BFF route or UI consumer. `OrgPolicy` has no mode or lock field.
4. **Fail-open defaults conflict with the card's fail-closed requirement.** `PermissionMode::from_wire` maps unknown to Auto (`permission/mod.rs:14-19`). `AgentProfile::from_wire` maps unknown to Chat, which is the Auto posture (`profile.rs:38-43`). `hook::evaluate` returns Allow on malformed context (`hook/mod.rs:117-121`, test `malformed_context_fails_open` at 239). An unrecognised hook decision maps to Allow (`hook/mod.rs:103-107`).
5. **Ownership drift for durable hooks.** `capability-ownership-matrix.md:61` says capability-core owns `hook_configs`. The only DDL is session-core migration 0004 (lines 165-185). The only code touching the table is GDPR erasure (`session-core/src/gdpr.rs:376`). No Go code references it.
6. **ABSENT in the trees:** named agent modes (AgentMode), setting locks, lockdown profile, frozen tool snapshots, "Customize hub", LoopGuard / `loop_detected`, a hook REST route, hook admin UI, handle parking for fetch_url / knowledge_search / web_search. **Partial precedents:** `AgentProfile` (Chat | DeployedAgent) with `ApprovalPosture` and the `stricter()` clamp (`profile.rs`); MCP per-server `tool_allowlist` (`capability-core/internal/api/registry_apis.go:441,472`); org-scope denied tools.
7. **Live-only changes do not touch the WS25 logic.** Keyword check of added lines (hook, duplicate, handle, autonomy, rung, posture, granted, PermissionMode and similar) in `agent.rs`, `runtime_loop/mod.rs`, `tool_loop.rs`, `state.rs`, `session-core/src/grpc.rs`, `sessions.proto` and `mp-contracts/src/lib.rs` returns no hook, loop, handle or rung logic. The live edits are ZDR-posture, retention and delegation or credential work. They do change the seams a hook_context wiring would use: `runtime_loop/mod.rs` live +18 adds `waived` parameters through the execute_step helpers, and `agent.rs` live +207 adds AuthenticatedUser threading.

---

## 1. Citation verification

Verdicts: MATCH, DRIFT (same symbol, other lines), ABSENT, WRONG, N/A. "M" = modified in live, "clean" = not in live-tree-status.txt.

| # | Card | Cited path:lines | Claim | wt verdict | live status | Corrected path:lines (wt / live) |
|---|---|---|---|---|---|---|
| 1 | 032 | `execution-core/src/permission/mod.rs:6-58` | PermissionMode Auto/Ask/Deny; unknown string maps to Auto | MATCH (window partial) | clean | enum 5-10; from_wire 13-20 (fallback 18); evaluate 30-37; evaluate_call 43-70 (same both) |
| 2 | 032 | `execution-core/src/runtime_loop/agent.rs:1900-1930` | Do loop passes empty hook_context | MATCH (call 1919; `""` at 1923) | M | wt 1919-1923 / live 1923-1927 |
| 3 | 032 | `session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:160-185` (path resolved by Glob) | hook_configs table; only erasure touches it | MATCH | clean | 161-185 (table 165-178; indexes 180-185); erasure `session-core/src/gdpr.rs:376` |
| 4 | 032 | (no path) "HookRegistry is in-memory" | in-memory DashMap | MATCH | runtime_registries.rs clean; state.rs M (+6, fields unchanged) | `model-gateway/src/runtime_registries.rs:2002-2011` (DashMap field at 2004); `state.rs:317` (field), `:460` (init) |
| 5 | 032 | (no path) "RunAgentRequest has no hook field" | no hook field on RunAgentRequest | MATCH | clean | RunAgentRequest at `proto/model_plane/v1/execution.proto:76`; only hook field is ExecuteStepRequest.hook_context = 6 at `:285` |
| 6 | 032 | (no path) "admins cannot set modes or locks" | no admin mode or lock write path | MATCH for modes and locks; card OMITS OrgPolicy | gateway grpc.rs M | see headline 3: grpc.rs wt 2061 / 2110 / 2167 / 2202 (live 2070 / 2119 / 2176 / 2211) |
| 7 | 032 | (no path) admin sections for MCP, plugins, skills, org instructions exist | exist | MATCH | clean | `apps/Frontend Plane/verevonv3/src/features/settings/components/{McpServersSection,PluginsSection,SkillsSection,OrgInstructionsSection}.tsx` |
| 8 | 032 | (no path) "model-gateway mode clamp" | clamp of client posture | MATCH | clean | `model-gateway/src/profile.rs` stricter 78; floor_from_scopes 95-108; resolve_posture 119-121 |
| 9 | 032 | (no path) "AutonomyRung grant exists, no org cap" | rung grant per call | MATCH | clean | `permission/mod.rs:124-141` (check_autonomy_rung); `mp-contracts/src/autonomy.rs:45-151`; `proto/model_plane/v1/runs.proto:22-36`; `execution.proto:154` (autonomy_rung = 12) |
| 10 | 032 | (no path) "one Customize hub" | hub exists? | ABSENT | clean | `features/agents/components/ChatbotStudioCards.tsx:73-79` is a placeholder ("tool menu/customize have no backend yet") |
| 11 | 032 | (no path) setting locks, lockdown, extension allow-lists, frozen tool snapshots | none | ABSENT (no symbols in CP, MP, FE). Partial: MCP tool_allowlist | clean | `capability-core/internal/api/registry_apis.go` mcpServerRow 432 (ToolAllowlist 441); mcpServerRegistration 453; mcpServerConfig 471 (ToolAllowlist 472); cap 371 |
| 12 | 032 | "plan B4" (Do-run hook_context) | plan item | NOT DETERMINED | n/a | no "B4" in `apps/Model Plane/**/*.md` |
| 13 | 063 | `model-gateway/src/tool_loop.rs:2320-2345` | exact-duplicate read-only dedupe | MATCH (live); DRIFT (wt) | M (+24 at this site) | wt doc 2296-2303, fn 2304-2322 / live doc 2320-2327, fn 2328-2346 |
| 14 | 063 | `execution-core/src/runtime_loop/agent.rs:3456-3473` | retrieval_signature | DRIFT (wt -7) | M (+7) | wt fn 3449-3466 / live fn 3456-3473; call site wt 1829 / live 1833 |
| 15 | 063 | `agent.rs:1923-1927` | `""` hook_context in Do loop | DRIFT (wt -4) | M | wt 1919-1923 / live 1923-1927 |
| 16 | 063 | (no path) "RunAgentRequest has no hook field" | as #5 | MATCH | clean | execution.proto:76; hook_context only at :285 |
| 17 | 063 | (no path) "hook_configs table exists but nothing reads it" | table without reader | MATCH | clean | `gdpr.rs:156, 197, 376 (DELETE), 621` (test); DDL 0004:165 |
| 18 | 063 | (no path) "duplicate_call_signature and retrieval_signature in mp-contracts" | in mp-contracts | WRONG | n/a | not in any crate. Private fns: `tool_loop.rs:2304` (wt) and `agent.rs:3449` (wt) |
| 19 | 063 | (no path) "retry.rs risky-call classifier" | retry gated on risky call | MATCH | clean | `execution-core/src/runtime_loop/retry.rs:149-150` (tool_call_is_retryable calls permission::is_risky_call); model-gateway copy `tool_retry.rs:16` (comment) |
| 20 | 063 | (no path) "handles park oversized built-in reads in the inline loop" | parking exists | MATCH | tool_loop M; tool_result_handles.rs clean | `verevon_read_outcome` wt 2164 / live 2188 calls `handle_or_inline_output` (wt 3798 / live 3805); store `tool_result_handles.rs:588`; field `state.rs:297` |
| 21 | 063 | (no path) "not fetch_url, knowledge_search or provider results" | not parked | MATCH (provider results not traced) | M | parking callers only (live): knowledge_graph_search 3480, knowledge_wiki_search 3508, knowledge_contradictions 3535, insights_overview 3573, inbox_search 3593, inbox_get_conversation 3608, social_list_accounts 3622, social_list_posts 3630, social_list_campaigns 3645, knowledge_list_documents 3666. Not parked: fetch_url arm live 2987 / wt 2996; knowledge_search arm live 3338 / wt 3347; web_search arm live 2874 / wt 2848 |
| 22 | 063 | (no path) "execution-core only truncates at 8,000 characters" | 8,000 cap | MATCH | agent.rs M, const line same | `agent.rs:279` `MAX_TOOL_CONTEXT_CHARS: usize = 8000` (both); `tool_context_cap` wt 4321 / live 4328 (call wt 4349 / live 4356); fan-out exemption `subagent/research_fan_out.rs:421` CONTEXT_CHARS = 24_000 (clean) |
| 23 | 063 | (no path) "no handles under ZDR" | ZDR bypass | MATCH | tool_loop M | zdr short-circuit in `handle_or_inline_output`: wt 3806 / live 3813 |
| 24 | 063 | (no path) "hook webhooks are egress and need allow-listing" | webhooks | N/A (none exist) | clean | `gateway.proto:1277-1279` callback_url DEPRECATED ("nothing has ever POSTed") |
| 25 | 063 | (no path) LoopGuard / typed `loop_detected` | new | ABSENT | n/a | no symbol in `apps/Model Plane/rust` |

Dedupe reference behaviour (verified): model-gateway sets `is_duplicate` per turn via `attempted_calls.insert(signature)`, skipping truncated calls (`tool_loop.rs` live 7192-7200 / wt 7085-7092). execution-core rejects a repeat with `CallSlot::Rejected` and the message "duplicate knowledge_search suppressed..." (`agent.rs` live 1833-1846 / wt 1829-1841). Neither loop stops the run. Both return error results to the model.

---

## 2. Affected-file map

Columns: path | role | approx lines wt / live | live status | tests (cheap names).

### 2a Model Plane, execution-core (Rust)

| Path | Role | Lines wt / live | Live | Tests |
|---|---|---|---|---|
| `src/permission/mod.rs` | PermissionMode, evaluate_call, is_risky_call, check_autonomy_rung | 5-70; 124-141; 195 | clean | in-file: deny_mode_always_denies 292, ask_gates_risky_allows_benign 318, external_mcp_tools_are_gated_under_ask 383, browser_agent_is_gated_under_ask 435, the_required_rung_follows_the_existing_risk_classification 588, the_same_tool_needs_different_rungs_on_different_calls 609, a_run_with_no_stated_rung_is_unconstrained_by_the_ladder 664 |
| `src/hook/mod.rs` | HookEvent, HookRule, HookContext, evaluate (fail-open), decision_of | 23-135 | clean | in-file: empty_context_allows 153, legacy_block_execution_denies_pre_tool_only 161, deny_rule_matches_exact_tool 176, ask_rule_with_prefix_matcher_and_camelcase_event 192, wildcard_and_first_match_wins 207, post_tool_use_can_reject_result 225, malformed_context_fails_open 239 |
| `src/runtime_loop/mod.rs` | execute_step 278; execute_step_with_browser_grant 336/339; execute_step_with_subagent 400/408 (hook_context param 404/412); execute_step_inner 455/467; hook::evaluate Pre 544/557, Post 783/797 | see role | M (+18: `waived` args) | `execution-core/tests/runtime_loop_test.rs:114` hook_can_block_step (clean) |
| `src/runtime_loop/agent.rs` | Do loop; `""` hook call 1919/1923; check_autonomy_rung call 1688/1692; retrieval dedupe call 1829/1833; retrieval_signature fn 3449/3456; tool_context_cap fn 4321/4328 (call 4349/4356); MAX_TOOL_CONTEXT_CHARS 279 | see role | M (+207 region) | 5141/5149 canonical_retrieval_signature_detects_exact_duplicate_queries; 10659 (live) a_granted_rung_refuses_what_it_does_not_cover_and_permits_what_it_does; tool_context asserts 12332-12333 (wt) / 12391-12394 (live) |
| `src/runtime_loop/retry.rs` | tool_call_is_retryable (149-150) | 149-150 | clean | in-file |
| `src/runtime_loop/session_core_rules_double.rs` | test double for session-core credential rules (delegation tests) | n/a | M (+537) | test-only |
| `src/grpc.rs` | ExecuteStep handler forwards `req.hook_context` | 925 / 943 | M (+98) | auth_tests module |
| `src/subagent/research_fan_out.rs` | CONTEXT_CHARS = 24_000 | 421 | clean | n/a |
| `tests/cross_service_loop_contract.rs` | asserts execution-core MAX_TOOL_CONTEXT_CHARS == model-gateway MAX_TOOL_OUTPUT_CHARS | 11-12, 150-155 | clean | the test itself |
| `tests/autonomy_ladder_contract.rs` | source-grep contract: gate placement, is_risky_call reuse | 76-125 | clean | the test itself |

### 2b Model Plane, model-gateway (Rust)

| Path | Role | Lines wt / live | Live | Tests |
|---|---|---|---|---|
| `src/tool_loop.rs` | dispatch_tool 2406/2430; dispatch_tool_with_cost 2467/2492; duplicate_call_signature 2304/2328; verevon_read_outcome 2164/2188; handle_or_inline_output 3798/3805; result_query arm 3691/3698; dedupe call site 7089/7196; MAX_TOOL_OUTPUT_CHARS = 8_000 (186) | see role | M (+425; retrieval_notice, ZDR) | duplicate_signature_catches_exact_repeat_fetch_url wt 9606 / live 9851; duplicate_signature_web_search_is_case_and_whitespace_insensitive 9627 / 9872; duplicate_signature_does_not_apply_to_stateful_or_side_effecting_tools 9640 / 9885; an_oversized_read_result_is_parked_and_then_readable_by_handle wt 7893 / live 8007; a_zdr_turn_never_parks_a_result_in_the_handle_store 8009; an_oversized_result_becomes_a_queryable_handle_instead_of_a_truncated_blob 8038; an_unparseable_oversized_result_stays_inline... 8070; result_query_refuses_an_unknown_handle... 8122; result_query_reads_a_slice_of_a_parked_result 8156; result_query_aggregate_returns_no_rows_at_all 8211; a_colleague_in_the_same_org_cannot_read_another_users_handle 8260 (all wt) |
| `src/tool_result_handles.rs` | ToolResultStore 588; insert 602; resolve 641; attach_artifact 657; prune 690; HANDLE_TTL = 900 s (53); apply_query 362; unwrap_mcp_content 167 | see role | clean (1037 lines) | 21 `#[test]` from line 721 |
| `src/runtime_registries.rs` | HookRegistry (DashMap) 2002-2011; hook_context_json 2013-2060; handle_register_hook 2068; handle_list_hooks 2099 | see role | clean (2628 lines) | hook_rules_reach_execution_core_in_the_shape_its_engine_parses 2448; an_empty_tool_scope_becomes_the_any_tool_matcher 2463; disabled_hooks_and_unevaluated_events_are_omitted_entirely 2472; one_orgs_hook_rules_never_travel_with_another_orgs_dispatch 2487 |
| `src/grpc.rs` | register_hook 2043/2052; list_hooks 2052/2061; check_permission 2061/2070; set_permission 2110/2119; get_policy 2167/2176; set_policy 2202/2211; unowned_policy_fields 154; forwarded_authorization 261; put_org_quota 608; capability_json 645 | see role | M (+47; ZDR gates) | policy_fields_with_no_owning_service_are_named_not_silently_banked (wt 2760). No hook or permission RPC tests found |
| `src/state.rs` | hooks field 317; HookRegistry::new 460; tool_results 297 (init 452) | 317, 460, 297, 452 | M (+6) | n/a |
| `src/tools.rs` | handle_code_interpreter (163) builds ExecuteStepRequest with `hook_context` (211) | 163, 211 | clean | n/a |
| `src/browser_run.rs` | browser_run_start (297); hook_context built 417/418; forwarded 427/428 | see role | M (+29) | n/a |
| `src/profile.rs` | AgentProfile 20; ApprovalPosture 27; from_wire 38; as_permission_mode 68; stricter 78; floor_from_scopes 95; resolve_posture 119 | see role | clean | deployed_agent_is_ask_posture 136; stricter_lets_ask_dominate_auto 161; client_profile_can_ratchet_stricter_but_never_weaker 170; autonomous_scope_pins_floor_to_ask 183 |
| `src/http_routes.rs` | no hook or policy route (0 matches); ZDR gates | n/a | M (+237) | n/a |
| `src/coordinator.rs` | PlanModeStore.record_grant 166; escalation request 318, 348 | see role | clean | n/a |
| `src/tool_retry.rs` | model-gateway copy of the retry gate (comment at 16) | 16 | clean | n/a |
| `src/sse_events.rs` | ChatEvent payloads; live adds RetentionClass | n/a | M (+203) | n/a |
| `src/lib.rs` | module list; live adds zdr_posture, zdr_features, relevance_calibration | n/a | M (+3) | n/a |
| `src/zdr_posture.rs` | ZDR route posture used by live grpc.rs and browser_run.rs | n/a | ?? (live only; absent in wt) | n/a |
| `tests/e2e_invoke_chain_test.rs` | a_repeated_read_after_the_scoped_revision_prefetch_is_never_dispatched_again | wt 8462 / live 8479 | M | the test itself |

### 2c Model Plane, session-core (Rust, SQL)

| Path | Role | Lines wt / live | Live | Tests |
|---|---|---|---|---|
| `migrations/0004_tasks_cron_hooks_skills_memory.sql` | hook_configs DDL (165-178); indexes hook_configs_org_idx (180-181), hook_configs_pattern_idx (183-185) | 161-185 | clean | n/a |
| `src/gdpr.rs` | hook_configs erasure count 156, 197; DELETE 376; test 621 | see role | clean | erasure tests |
| `src/grpc.rs` | set_run_mode 4984/5155 (grant to AutonomyEscalation); autonomy_rung metadata 5039/5210 | see role | M (+861) | set_run_mode_and_upsert_skill_against_real_pg wt 9338 / live 9585 |
| `src/retention_receipt.rs` | ZDR retention receipt | n/a | ?? (live only) | n/a |

### 2d mp-contracts (Rust shared crate)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| `src/autonomy.rs` | rank 45; permits 59; EscalationRefusal 69; label 118; rung_from_keyword 131; AutonomyEscalation 151 (request 164) | see role | clean | the_ladder_is_strictly_ordered 228; an_unset_rung_is_the_narrowest_never_a_grant 237; a_rung_permits_itself_and_everything_narrower 253; a_widening_request_with_a_real_reason_is_accepted 273; a_request_that_widens_nothing_is_refused 295 |
| `src/lib.rs` | `pub mod autonomy;` (2); live adds retention, zdr_refusal | 2 | M (+2) | n/a |

### 2e Protos (`apps/Model Plane/proto/model_plane/v1/`)

| Path | Contents | Lines (wt) | Live |
|---|---|---|---|
| `gateway.proto` | RegisterHook 161; ListHooks 162; CheckPermission 163; SetPermission 164; GetPolicy 165; SetPolicy 166; Hook message 1268-1288; RegisterHookRequest 1290; ListHooksResponse 1308; permissions comment 1313; CheckPermissionRequest 1315; SetPermissionRequest 1328; policy comment 1341; OrgPolicy 1343 | as listed | clean |
| `execution.proto` | ExecuteStep rpc 15; RunAgent rpc 34; RunAgentRequest 76 (plan_mode 143, autonomy_rung 154); ExecuteStepRequest 268 (permission_mode 282, hook_context 285) | as listed | clean |
| `runs.proto` | enum AutonomyRung 22-36 (UNSPECIFIED 0, READ_ONLY 1, WORKSPACE_WRITE 2, DANGER_FULL_ACCESS 3) | 22-36 | clean |
| `events.proto` | EVENT_TYPE_PRE_TOOL_USE 77 (=20); POST_TOOL_USE 79 (=21); POST_TOOL_USE_FAILURE 81 (=22) | 77-81 | clean |
| `sessions.proto` | live adds CountWaivedRuns rpc and messages (ZDR) | n/a | M (+42) |

### 2f Go, capability-core and Control Plane org-core

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| `apps/Model Plane/go/services/capability-core/internal/policy/engine.go` | Engine 70; EvaluateCapability 97; denyOnGrant 199 (tool permission owner per matrix) | see role | clean | engine_grant_test.go; engine_sandbox_test.go; `internal/authz/authz_test.go` |
| `.../capability-core/internal/api/capabilities.go` | `/api/v1/capabilities/scopes/revoke` route 59; revokeScope 345 | 59, 345 | clean | n/a |
| `.../capability-core/internal/api/registry_apis.go` | mcpServerRow 432 (ToolAllowlist 441); mcpServerRegistration 453; mcpServerConfig 471 (ToolAllowlist 472); maxMCPToolAllowlistEntries 371 (= 64) | as listed | clean | n/a |
| `.../capability-core/internal/registry/registry.go` | capability catalogue (cap.command.shell at 50) | 50 | clean | n/a |
| `apps/Control Plane/org-core/internal/org/types.go` | Entitlement 28; Entitlements 259; defaultEntitlements 266; InteractiveZdrEntitlementKey 276 | as listed | clean | n/a |
| `apps/Control Plane/org-core/internal/org/repository.go` | org_quotas read and write | 879, 919, 923 | clean | n/a |

### 2g Frontend, verevonv3 (`apps/Frontend Plane/verevonv3`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| `src/shared/actions/action-registry.ts` | settings.* 2376-2420; mcp.delete_server 2431; mcp.share_server 2442; org.set_quota 2739; org.update_instructions 2750; org.update_zdr 2761; chat.approve_plan 2827 (input chatApprovePlanInput 1024-1028: grantedRung enum); orchestration.decide_approval 2948 | as listed | clean | action-registry.test.ts |
| `src/features/settings/components/` | McpServersSection, PluginsSection, SkillsSection, OrgInstructionsSection, PersonaSection (28 files in dir) | n/a | clean | SettingsPages.test.tsx; PersonaSection.test.tsx |
| `src/features/agents/components/ChatbotStudioCards.tsx` | "Customize" placeholder | 73-79 | clean | n/a |
| `src/features/agents/lib/verevon-agent-blueprints.tsx` | "Customize widget" label | 270 | clean | n/a |
| `src/features/dashboard/home/DashboardComposer.tsx` | Ask/Do ("Arbeidsmodus") plan-mode segment; no rung vocabulary in this file | ~3026-3056 (both trees) | M (+7) | n/a |
| `src/features/chat/components/chat-types.ts` | AutonomyRung import 5; grantedRung 395 (wt) | 5, 395 | M (+7) | ChatPanels.test.tsx (clean) |
| `src/features/chat/components/use-chat-controller.ts` | plan-gate consumer | n/a | M (+4) | use-chat-controller.plan-gate.test.ts (clean) |
| `src/shared/api/chat-client.ts` | chat API client | n/a | M (+53) | chat-client.test.ts (M) |
| `apps/gateway/src/domains/router_policy.rs` | only BFF policy route: GET/PUT `/api/v1/router-policy` | 30 | clean | n/a |
| `apps/gateway/src/domains/settings.rs`, `settings/`, `orgs.rs`, `orgs/`, `mcp.rs` | candidate BFF homes for admin routes (not read) | n/a | clean | n/a |
| `apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_pb.ts`, `gateway_connect.ts`, `events_pb.ts` | generated TS for hook, permission and policy RPCs (legacy app dir `verevon`, not v3) | gateway_pb.ts 3632-3728, 4954-4967; events_pb.ts 172-186 | clean | n/a |

### 2h Docs and rules

| Path | Content used | Live |
|---|---|---|
| `apps/Model Plane/docs/decisions/ledger.md` | 1759-1782 autonomy ladder decision (per-call; UNSPECIFIED semantics; reuse of risk classifiers). 2607-2615 C7: grant thread-keyed in PlanModeStore, in-memory, durable carrier "remains open". 2664 "C7 ... was a no-op" | clean |
| `apps/Model Plane/docs/capability-ownership-matrix.md` | 42 (gateway registries = ephemeral caches); 52 (identity/org/billing = Control Plane); 59 (registries = capability-core); 60 (policy/permissions = capability-core, execution-core permission/ = enforcement); 61 (hook config = capability-core hook_configs); 62 (hook firing = execution-core hook/mod.rs); 105-106 (capability-core system of record) | clean |
| `apps/master-ownership-matrix.md` | 125 model-gateway (public invoke, ZDR and cross-plane guards); 128 execution-core (tool loop, hooks, subagents); 130 capability-core (tools, skills, plugins, MCP, safety); 165 rule 9 (identity and membership = Control Plane) | clean |
| `apps/Model Plane/CLAUDE.md` | Code Style: two tool-dispatch loops must stay separate; one canonical owner per capability; smallest abstraction | clean |
| `CLAUDE.md` (repo root) | 23 Control Plane owns entitlements; 27 Model Plane owns execution loops; 35 ZDR propagation; 43 action contracts in src/shared/actions | clean |
| `apps/Model Plane/docs/GATEWAY_DEDUPLICATION_PLAN.md` | 130 hook_context wiring; hook_configs mention | clean |

---

## 3. Related contracts

- **Protos:** see 2e. Hook wire = `ExecuteStepRequest.hook_context` JSON `{"rules":[{event,tools,decision,reason}]}` (gateway writes `pre_tool_use` / `post_tool_use`; decisions deny|block, ask|approve|confirm, else allow; `hook/mod.rs:42-47,97-108`). `block_execution` legacy flag at `hook/mod.rs:70`.
- **SQL, actual latest file names (directory listings):**
  - `apps/Model Plane/rust/services/session-core/migrations/`: committed latest `0043_audit_outbox_requeue.sql` (43 files in wt). Live untracked (`??`): `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/`.
  - `apps/Model Plane/go/services/capability-core/migrations/`: 29 files; latest `0015_background_process_capability.up.sql` and `.down.sql`.
  - `apps/Control Plane/org-core/migrations/`: latest `020_interactive_retention_outbox_org_id_check.up.sql` and `.down.sql`.
  - model-gateway: no SQL (hook and permission state is in memory).
  - hook_configs DDL: session-core 0004 lines 165-185 (columns: id, org_id, tool_name_pattern, hook_type pre_tool|post_tool, action approve|block|modify|log, reason, modify_input, modify_output, priority, enabled, timestamps).
- **Config and env:** no hook, loop-guard or handle env flag found in `apps/Model Plane/deploy` (grep).
- **SSE event types:** events.proto hook lifecycle enum (77-81); no Rust, Go or Python emitter outside generated code (grep). Live adds `retrieval_notice` (`tool_loop.rs` RETRIEVAL_NOTICE_KEY, retrieval_notice_events; uncommitted). Taxonomy gate: `.github/workflows/sse-taxonomy.yml:45` runs `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`. Doc: `apps/Model Plane/docs/sse-event-taxonomy.md` (live M).
- **Gateway routes:** hooks, permissions and policy are gRPC only. `http_routes.rs` has no hook or policy route. The verevonv3 BFF's only policy route is `/api/v1/router-policy` (`router_policy.rs:30`).
- **Action contracts** (`apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts`): settings.* 2376-2420; mcp.delete_server 2431; mcp.share_server 2442; org.set_quota 2739; org.update_instructions 2750; org.update_zdr 2761; org.update_support_ai_mode 2772; chat.approve_plan 2827 (grantedRung read_only | workspace_write | danger_full_access). No action for hooks, modes, permissions or org policy. Root CLAUDE.md line 43 requires the action contract before any meaningful UI operation.

---

## 4. Test and build commands

Cited from repo files. Not run.

- **Rust, Model Plane workspace** (`apps/Model Plane/rust/Cargo.toml` `[workspace] members` include `crates/mp-contracts`, `services/model-gateway`, `services/session-core`, `services/inference-core`, `services/execution-core`). Package names from each `Cargo.toml`: `model-gateway`, `execution-core`, `session-core`, `mp-contracts`.
  - CI, `.github/workflows/model-plane-rust-ci.yml` (working-directory `apps/Model Plane/rust`): `cargo fmt --all -- --check` (line 99), `cargo check --workspace` (105), `cargo check --workspace --all-targets` (112), `cargo test --workspace --no-fail-fast` (119). Step "Contract goldens match their pins" (lines 94-96) runs `bash scripts/tests/zdr-contract-sync.test.sh` and `bash scripts/tests/zdr-contract-sync.test.sh --self-test` from the repo root.
  - Per package (`apps/Model Plane/CLAUDE.md` Testing): `cargo test -q -p model-gateway --all-targets`; same for `-p execution-core`, `-p session-core`, `-p mp-contracts`.
  - Name filters for this sweep: `cargo test -p model-gateway duplicate_signature`; `cargo test -p model-gateway tool_result_handles`; `cargo test -p model-gateway policy_fields`; `cargo test -p execution-core hook::`; `cargo test -p execution-core autonomy`; `cargo test -p execution-core retrieval_signature`; `cargo test -p mp-contracts autonomy`.
  - User memory note (MEMORY.md, Model Plane build entry): two cargo targets fail pre-existing on Windows; use `--lib`.
- **Proto:** `buf generate` (regenerate `apps/Model Plane/go/gen`, python `_gen`, and the Frontend gen dirs; config not read). Per `apps/Model Plane/CLAUDE.md`, `buf lint` has pre-existing debt.
- **SSE gate:** `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`.
- **Go, capability-core** (`apps/Model Plane/go/services/capability-core`, module `github.com/triodelab/model-plane/services/capability-core`, go 1.25.1): `go test ./...` and `go vet ./...` (per `apps/Model Plane/CLAUDE.md`). No go.work; user memory says Go `-race` needs cgo (unavailable here) and `GOWORK=off` for new deps.
- **Go, Control Plane org-core** (`apps/Control Plane/org-core`, go.mod present): `go test ./...`. No org-core-specific command found in root or Model Plane CLAUDE.md; not verified.
- **Frontend, verevonv3** (`apps/Frontend Plane/verevonv3/package.json`): `pnpm test` (node --test on `apps/gateway/scripts/*.test.mjs` then `vitest run`), `pnpm lint` (`eslint .`), `pnpm typecheck` (`tsc -b`), `pnpm build` (typecheck + vite build). Gateway Rust (root CLAUDE.md Build And Run): `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`. User memory: pre-existing typecheck, lint and vitest failures in verevonv3; `:5173` is the main checkout, so use a second dev server.

---

## 5. Owners

- **Hook config registry:** capability-core (`capability-ownership-matrix.md:61`). Firing and enforcement: execution-core `hook/` (`:62`; master matrix 128). Gateway `hooks` = cache (`:61`; `:42` ephemeral caches must hydrate and publish). Actual table location (session-core 0004) disagrees with the matrix. See NOT DETERMINED 1.
- **Permissions, RBAC, tool ACL:** capability-core `policy/engine.go` (`:60`). execution-core `permission/` = enforcement. Gateway ACL cache must hydrate from capability-core (`:60`). Current gateway `set_permission` writes through to capability-core (grpc.rs wt 2110-2115 comment).
- **Tool, skill, MCP, plugin, command registries:** capability-core (`:59`; flagged CONSOLIDATE). Rulings `:105-106`: capability-core is the system of record and "no second registry implementation".
- **Identity, org, billing, quotas, entitlements:** Control Plane (root `CLAUDE.md:23`; `capability-ownership-matrix.md:52`; master rule 9 at `:165`). Spend and token caps go to Control `org_quotas` (grpc.rs set_policy).
- **Execution loops, hooks, subagents:** Model Plane. execution-core owns the Do loop and hook firing (master `:128`). model-gateway owns the chat loop and its dispatch_tool (master `:125`; `apps/Model Plane/CLAUDE.md` Code Style).
- **Two loops rule:** model-gateway `dispatch_tool` (read-only router, refuses side effects) and execution-core `execute_step_inner` (capability, hook and permission pipeline). They "must stay separate". Consequence for F-t3-063: a LoopGuard means one implementation per loop, or a shared pure helper; not a merged loop.
- **Autonomy ladder:** Model Plane. Shared implementation in `mp-contracts::autonomy` (ledger 1759-1782).
- **Zero Data Retention:** must propagate through every content-persisting boundary (root `CLAUDE.md:35`). Applies to handles and to any run or hook record.
- **Frontend admin and hub:** Frontend Plane owns UI and same-origin gateway or BFF normalisation (root `CLAUDE.md` Architecture Rules; no line captured).
- **Owner conflict (card):** F-t3-032 puts the registry and signing in Control Plane; the matrices put policy and hook registries in capability-core. NOT DETERMINED 2.

---

## 6. Uncommitted-only (live tree)

Relevant files whose live state differs from wt. Summaries from `git --no-optional-locks diff --stat` in `C:/dev/CoresSystem` plus keyword checks on added lines.

| Path | Live status | One-line change summary |
|---|---|---|
| `apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs` | M (+207 region) | AuthenticatedUser threading and `ctx.route.is_waived()` ZDR refusal; 0 hook, loop, handle or autonomy hits in added lines |
| `apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs` | M (+18) | `waived: bool` parameters added through the execute_step helpers |
| `apps/Model Plane/rust/services/execution-core/src/grpc.rs` | M (+98) | ExecutionService auth and ZDR helpers (enforce_persistence_free_execution, enforce_agent_route); hook_context forwarding not changed |
| `apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs` | M (+537) | Test double enforcing session-core credential rules for managed runs and delegation writes |
| `apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs` | M (+425) | retrieval_notice SSE events; ZDR and dispatch changes in dispatch_tool_with_cost and run_tool_rounds; no added line touches duplicate or handle code |
| `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` | M (+237) | ZDR gates (zdr_or_refuse, continuation_gate) on routes; no hook or policy route |
| `apps/Model Plane/rust/services/model-gateway/src/grpc.rs` | M (+47) | zdr_for route gating in RPC handlers; policy RPC bodies not changed in added lines |
| `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` | M (+29) | zdr_or_refuse on browser run start; hook_context lines unchanged |
| `apps/Model Plane/rust/services/model-gateway/src/state.rs` | M (+6) | cross-encoder env wiring; hooks field unchanged |
| `apps/Model Plane/rust/services/model-gateway/src/lib.rs` | M (+3) | module declarations for zdr_posture, zdr_features, relevance_calibration |
| `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` | M (+203) | RetentionClass enum and related ZDR event shaping |
| `apps/Model Plane/rust/services/model-gateway/src/retrieval_tools.rs` | M (+4) | zdr_mode field on retrieval payload |
| `apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` | M | test edits (not read) |
| `apps/Model Plane/rust/services/session-core/src/grpc.rs` | M (+861) | persona, posture and ZDR gates in run start and memory indexing; set_run_mode grant path not changed |
| `apps/Model Plane/rust/crates/mp-contracts/src/lib.rs` | M (+2) | module declarations for retention and zdr_refusal |
| `apps/Model Plane/proto/model_plane/v1/sessions.proto` | M (+42) | CountWaivedRuns rpc and messages (ZDR) |
| `.github/workflows/model-plane-rust-ci.yml` | M (+50) | added CI steps, including a session-core `zdr_receipt` test run |
| `apps/Model Plane/docs/sse-event-taxonomy.md` | M | not diffed |
| `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx` | M (+7) | composer changes (not keyword-checked) |
| `apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts` | M (+7) | chat types (not keyword-checked) |
| `apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts` | M (+4) | chat controller (not keyword-checked) |
| `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts` | M (+53) | chat client (not keyword-checked) |
| `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx` | M | chat messages (approval-button i18n is in F-t3-066 scope) |

Live-only (`??`) files relevant to these items:

| Path | Relevance |
|---|---|
| `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs` | ZDR posture used by live grpc.rs and browser_run.rs; absent in wt |
| `apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs`, `relevance_calibration.rs`, `calibration/`, tests `zdr_*` | ZDR and calibration work |
| `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs` | retention receipt |
| `apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/` | latest live-only SQL (see section 3) |
| `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs`, `zdr_refusal.rs`, `tests/zdr_retention_matrix.rs` | ZDR contract |
| `apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs` | ZDR refusal inventory |
| `apps/Model Plane/api/contracts/zdr-refusals.json` (+ `.sha256`) | ZDR contract golden |

Deleted (`D`) in live: none in the WS25 surface. Deleted items are Frontend verevon-web product-demo files.

---

## 7. NOT DETERMINED

1. **Owner and table location of durable hooks and org policy.** capability-core (`capability-ownership-matrix.md:61`) vs Control Plane (root `CLAUDE.md:23`, entitlements) vs session-core (table DDL in 0004). Not decided by any doc read.
2. **"Plan B4".** Referenced by the card and by Appendix D. No `B4` in `apps/Model Plane/**/*.md`. Location not found.
3. **OrgPolicy write path after about line 2262 of set_policy.** Read only as far as the `denied_tools` and quota branches. `rate_limit_rpm` is process-local (grpc.rs wt 2269-2273). Whether `denied_tools` revocations have a read-back path is not traced.
4. **Hook event emitters.** No Rust, Go or Python emitter for EVENT_TYPE_PRE/POST_TOOL_USE outside generated code. The repo-wide grep was truncated at 40 hits, so the Frontend legacy and other apps were not fully enumerated.
5. **`AgentModeRequest`.** Named only in a comment at `tool_loop.rs` (about line 3068 wt). No proto message found. Meaning not determined.
6. **Frozen tool snapshot hashing.** `capability-core/internal/api/model_action_view.go` and `internal/cron/control_authorizer.go` matched a schema-hash pattern. Not read; reuse not determined.
7. **Provider-action results parking.** Card says provider results are not parked. Not traced through `execute_provider_action` (`execution-core/src/runtime_loop/mod.rs` wt 1634 / live 1652).
8. **Settings sub-page routing.** That the four admin sections are reachable from the settings router was not traced.
9. **Compatibility with live edits.** Live +207 in agent.rs, +18 in runtime_loop/mod.rs (waived args) and +537 in session_core_rules_double.rs may conflict with a hook_context seam. Determined from diff sampling only; not merged or tested.
10. **Build and test status.** Not run. No claim is made that any listed test passes.
11. **Audit Section 8 citation** `DashboardComposer.tsx:3030-3052` (Ask/Do switch with rung vocabulary). The Ask/Do segment is at about 3026-3056 in both trees. The rung vocabulary is not in that file (it is in chat-types.ts and action-registry.ts).
12. **`apps/Frontend Plane/verevon/` (legacy, non-v3).** Only its generated TS was checked. Whether it is in scope is not determined.
13. **Appendix D F-t3-032 text** is truncated in the source at "M (hub and frozen...". The remainder is not recoverable from the audit file.
14. **Live continuation changes.** `agent.rs` live touches `run_agent_with_tools` and `continuation_descriptor`. Relevance to F-t3-037 (out of scope here) not determined.
