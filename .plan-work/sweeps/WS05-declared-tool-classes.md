# WS05-declared-tool-classes: code map for F-t3-003

Read-only sweep. No tests or builds run. Only this file was written.

**Trees.** WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). LIVE = `C:/dev/CoresSystem` (same HEAD plus uncommitted work; status from `git --no-optional-locks status`). The audit read LIVE. Unmarked line numbers are WT. "LIVE n" marks live-tree lines.

**Audit sources.** Card `### F-t3-003` at `03-model-plane-chat-and-agents-audit.md:369`, body 370-383 ("Verevon today" line 380, "How to build it" line 382). Wave row §9.1 line 1536 (W1). Priority row Appendix B line 1821. Appendix C block lines 1938-1945. Appendix D: no line cites F-t3-003 (grep by id).

## 0. What the code says (corrections to the card)

1. The card's tool_loop.rs and runtime_loop/mod.rs line numbers match LIVE, not WT. In WT they sit 13-24 lines lower. The logic is the same in both trees.
2. MCP calls stop before the permission gate. `trusted_capability_id` returns None for `mcp__*` (capability_policy.rs:838). `GrpcCapabilityPolicy` turns that into `permission_denied` (capability_policy.rs:361-362). `execute_step_inner` returns a failed step before `permission::evaluate_call` (runtime_loop/mod.rs:488-539, then 557). The model reads "capability policy unavailable: tool has no governed capability binding" (runtime_loop/mod.rs:535 labels every non-Unavailable error that way).
3. `risk_level` is read only by capability-core, for registered rows (engine.go:112-122: RiskHigh returns Ask). execution-core never reads it. Dynamic MCP names have no row, so that path never applies to them.
4. MCP annotations stop inside model-gateway. The wire carries only name, description and parameters_json (inference.proto:178-185). No digest pinning exists.
5. ZDR for MCP is committed at the gateway. `ProxyMcpTool` refuses when the caller identity is ZDR (model-gateway/src/grpc.rs:1777-1778, `effective_zdr(false)`). The run's zdr flag never reaches execution-core's MCP path (`execute_mcp`, runtime_loop/mod.rs:1069-1074 has no zdr parameter). LIVE re-types that refusal under the feature key `mcp_execution` (§8).
6. The never-auto set is more than `tickets.create`. `requires_consent_to_disclose` (permission/mod.rs:160-162, `read_subagent_result`) is a second always-gated predicate (evaluate_call, 53-58).
7. Hook rules already express per-tool allow, ask or deny on `mcp__*` globs (hook/mod.rs:193-201 test). The hook registry is in memory in the gateway (runtime_registries.rs:2002-2005), and malformed hook context fails open (hook/mod.rs:117-120).
8. No per-tool allow/ask/never storage exists. The per-server `tool_allowlist` is exact-name. It lives in gateway memory (runtime_registries.rs:89) and in catalog `config_json` (runtime_registries.rs:1094-1098).
9. The ownership docs disagree on MCP registration (§7). The capability-ownership matrix says the stdio transport is wired (line 142). Code quarantines non-HTTP transports (runtime_registries.rs:1785-1795).
10. Proto comment capabilities.proto:78 lists "safe, read_only, mutation, dangerous". The Go model and migrations use low, medium, high (capability.go:24-29; migration 0007:27-29).

## 1. Card digest

- Tier Priority | Verevon today Partial | Effort L | Plane Model Plane | Wave W1 | Confidence High (card line 372).
- Build (card 374): declared side-effect class on every capability-core row (built-in, provider action, discovered MCP tool), derived at discovery from annotations and pinned by digest; `trusted_capability_id` resolves `mcp__server__tool` through the registry; autonomy rung reads the class; non-delegable never-auto set; per-tool allow/ask/never at user and org scope in capability-core. Do mode first; read-only MCP in Ask is later.
- Dependencies (§9.1 row 1536): continuation descriptor key provisioned; integration-corev2 catalogue metadata.
- Named dependents (Appendix B, 1557-1658): F-t3-006, -010, -018, -025, -030, -032, -045, -056, -057, -059, -070, -097.
- Appendix C (1938-1945): partial, high. Corrections: Manus drives only replay dedupe and MCP annotations, not approvals; Hermes has a per-SERVER trust tier, not a per-tool store; Perplexity's allow/ask labels appear only in image alt text; Claude's never-allow set covers 8 tools on each built-in connector (GitHub, Microsoft 365), not MCP in general.

## 2. Citation verification

Key: MATCH; DRIFT (same symbol, other lines); PARTIAL (some claims absent); ABSENT. Live status: clean / M (modified) / ?? (untracked).

| # | Card cite | Claim | WT verdict | Live | Corrected WT | Corrected LIVE |
|---|---|---|---|---|---|---|
| 1 | execution-core/src/permission/mod.rs:200-293 | name-substring risk; every mcp__ risky | DRIFT (minor) | clean | is_risky_call 195-200; is_risky_tool 229-285; mcp__ arm 280-282 | same |
| 2 | go/services/capability-core/internal/models/capability.go:1-60 | three-level risk_level | MATCH | clean | consts 24-29; IsSupportedRiskLevel 35-42 | same |
| 3 | go/services/capability-core/internal/policy/engine.go:105-155 | engine maps risk to decision | MATCH (+/-8) | clean | EvaluateCapability 97-154; risk switch 112-153 | same |
| 4 | execution-core/src/capability_policy.rs:736-840 | trusted_capability_id static map, None for MCP | DRIFT (+5) | clean | fn 741-841; MCP None arm 836-838 | same |
| 5 | execution-core/src/runtime_loop/mod.rs:495-565 | execute_step_inner refuses every call | DRIFT (-7 to -13) | M | capability gate 488-539; hook 544-550; permission gate 557-561 | capability gate 501-552; hook 557-563; permission gate 570 |
| 6 | model-gateway/src/tool_loop.rs:625-642 | inline loop denies MCP ("Ask refuses") | DRIFT (-24) | M | doc 601-613; fn 615-620; refusal 2489-2494; MCP fallback arm 3679-3682 | MATCH: doc 625-637; fn 639-644; refusal 2515-2518; MCP arm 3686-3689 |
| 7 | tool_loop.rs:625-638 | continuation contract text | DRIFT (-24) | M | text 608-613 (contract line 610) | MATCH: text 632-637 (line 634) |
| 8 | model-gateway/src/mcp_jsonrpc.rs:55-125 | side_effect() has no production consumer | MATCH | clean | struct 64-79 (docs 51-63); enum 85-97; side_effect 108-124; callers are tests only (458-533) | same |
| 9 | model-gateway/src/state.rs:317 | hooks held in memory | MATCH | M (6 changed lines; line 317 unchanged) | `pub hooks` 317; HookRegistry is a DashMap, runtime_registries.rs:2002-2005 | `pub hooks` 317 |
| 10 | permission/mod.rs:72-100 | never-auto set is only tickets.create | PARTIAL | clean | requires_durable_owner_approval 77-79 (ticket only); second predicate requires_consent_to_disclose 160-162 (read_subagent_result, every posture) | same |
| 11 | permission/mod.rs:81-92 | warning that two risk tables diverge | MATCH | clean | doc 81-98 (warning 92-93) | same |
| 12 | permission/mod.rs:100-200 | reuse risk_level, rung_required_for, check_autonomy_rung | PARTIAL | clean | rung_required_for 100-108; check_autonomy_rung 124-143; risk_level ABSENT from this file (only a comment in health_attest.rs:321) | same |
| 13 | capability_policy.rs:836-838 | dynamic MCP needs a registry binding | MATCH | clean | 836-838 exact; `_ => return None` at 838 | same |
| 14 | proto/model_plane/v1/execution.proto:100-107 | agentic runs carry zdr | MATCH | clean | zdr = 9 at 107 (inside RunAgentRequest 76-211) | same |
| 15 | (no cite) operation_is_write | fail-safe: unknown operation counts as write | MATCH | clean | integration_tools.rs:747-761 | same |

## 3. Claim checks

| Claim | Verdict | Evidence |
|---|---|---|
| Every mcp__ name is risky under the permission gate | CONFIRMED | permission/mod.rs:280-282 |
| trusted_capability_id("mcp__x") is None | CONFIRMED | capability_policy.rs:838; test asserts it at 965 |
| MCP refused before the permission gate | CONFIRMED | capability gate 488-539 precedes permission gate 557; text at 530-535 |
| Missing binding reads as "policy unavailable" to the model | CONFIRMED (wording issue) | runtime_loop/mod.rs:535 |
| Invariant test covers dynamic MCP tools | NOT the case | every_offered_tool_has_a_capability_binding (agent.rs:3805) checks built-ins; MCP defs appended later at agent.rs:622-652 |
| Ask-mode MCP reachable in execution-core | NOT reachable today | capability gate denies first; `is_risky_tool` AwaitApproval (permission/mod.rs:65-67) never reached |
| Inline (chat) loop denies all mcp__ in every posture | CONFIRMED | inline_tool_allowed 615-620; refusal 2489-2494; fallback 3679-3682; tests tool_loop.rs 8929, 8966, 9430 |
| side_effect() has a production consumer | NOT the case | only test call sites (mcp_jsonrpc.rs:458-533); grep over apps/Model Plane/rust |
| Annotations reach execution-core | NOT the case | ToolDefinition (inference.proto:178-185) reused by ListMcpToolsResponse (gateway.proto:1134-1137) |
| MCP discovery is pinned by digest | ABSENT | no digest, sha256 or fingerprint in mcp_jsonrpc.rs, runtime_registries.rs, execution-core mcp_gateway.rs (grep) |
| ZDR propagates to MCP execution | PARTIAL | Gateway gate committed (grpc.rs:1777-1778, `effective_zdr(false)` on caller identity); run zdr flag not passed (`execute_mcp`, runtime_loop/mod.rs:1069-1074 has no zdr). LIVE re-keys the refusal to `mcp_execution` (grpc.rs:1787) |
| Never-auto set is tickets.create only | PARTIAL | permission/mod.rs:53-58 (both predicates), 77-79, 160-162 |
| risk_level drives a decision | CONFIRMED for registered rows only | engine.go:112-122; runtime_loop/mod.rs:505-509 (Ask returns awaiting_approval); no MCP rows (registry.go seeds only cap.mcp.filesystem, :46) |
| Hooks are in memory | CONFIRMED | runtime_registries.rs:2002-2005 (DashMap) |
| Hook rules can ask on mcp__* | CONFIRMED | hook/mod.rs:193-201 test; fail-open on malformed context 117-120 |
| Per-tool allow/ask/never stored at user or org scope | ABSENT | no tool-policy column or table in capability-core migrations; mcp_servers (0003:188-213) has none |
| Stdio MCP transport is wired (ownership doc §G2) | CONTRADICTED by code | capability-ownership-matrix.md:142 vs runtime_registries.rs:1785-1795 (only "http" is dispatched) |
| MCP tools are offered to Do runs | CONFIRMED | agent.rs:622-652 merged_tool_defs appends ListMcpTools output (McpGatewayClient at 652) |

## 4. Affected-file map

Paths are under `apps/Model Plane/rust/services/` unless shown in full. "Clean" means no uncommitted change in LIVE.

| Path | Role | Lines (WT) | Live | Tests (WT lines) |
|---|---|---|---|---|
| execution-core/src/permission/mod.rs | evaluate_call; is_risky_call and is_risky_tool; rung functions; never-auto predicates | 31-285; rung 100-143; never-auto 77-79, 160-162 | clean | external_mcp_tools_are_gated_under_ask (383); reserved_owner_action_never_auto_executes (343); reading_a_subagent_result_needs_approval_on_every_posture (503); the_required_rung_follows_the_existing_risk_classification (588); the_same_tool_needs_different_rungs_on_different_calls (609); a_run_with_no_stated_rung_is_unconstrained_by_the_ladder (664) |
| execution-core/src/capability_policy.rs | CapabilityPolicy trait; Grpc binding; trusted_capability_id; response binding check | trait 54-72; binding 361-362; fn 741-841; check 518 | clean | bindings_are_exact_and_unknown_or_dynamic_tools_fail_closed (940); absent_capability_decision_key_is_allowed_only_for_dev_mode (896) |
| execution-core/src/runtime_loop/mod.rs | execute_step_inner (capability, hook, permission gates); MCP arm; execute_mcp | 455-~800 (end not read); MCP arm 747-748; execute_mcp 1069-1093; callers 305, 364, 428 | M (+18) | none with mcp__ names (grep) |
| execution-core/src/runtime_loop/agent.rs | merged_tool_defs (MCP offer); leaf-role refusal; rung check; plan-mode refusal; data_category; binding invariant | merged 622-652; leaf 1639-1661; rung 1685-1700; plan mode from 1802; data_category 320; invariant 3805 | M (207 changed) | every_offered_tool_has_a_capability_binding (3805); a_delegated_subagent_may_never_run_a_risky_tool_even_under_auto_mode (10698); plan_mode_refuses_a_risky_tool_at_the_top_level_even_under_auto_mode (10864) |
| execution-core/src/runtime_loop/retry.rs | is_risky_call call site (purpose not read) | 150 | clean | not read |
| execution-core/src/mcp_gateway.rs | McpGatewayClient: ListMcpTools and ProxyMcpTool over gRPC; parse_mcp_tool_name; MODEL_GATEWAY_ADDR | 24-100; parse 108; env 29-37 | clean | parse tests at 116-137 (names not read) |
| execution-core/src/provenance.rs | TrustClass::classify: mcp__ prefix (duplicate of gateway rule, "keep in sync by hand") | 49-67 | clean | 333 (mcp__srv__create_order) |
| execution-core/src/integration_tools.rs | operation_is_write (unknown counts as write); CATALOG mirror | fn 747-761; CATALOG 71 | clean | operation_is_write_classifies_reads_and_writes (965); ..._accepts_provider_prefixed_aliases_case_insensitively (979); ..._returns_none_for_unknown_and_empty (1051) |
| execution-core/src/hook/mod.rs | PreToolUse glob rules (allow, ask, deny); fail-open on malformed | evaluate 113-132; fail-open 117-120 | clean | rule tests 175-245 (mcp__* ask 193-201) |
| execution-core/src/grpc.rs | reject_direct_owner_action (LIVE 721-722); ExecuteStep handler | WT tests 1679-1683 | M (98 changed) | 1679-1683 (WT) |
| execution-core/src/approval_delivery_worker.rs; ticket_tools.rs | tickets.create owner adapter; TOOL_NAME and CAPABILITY_ID | worker 120-122 | clean | not read |
| execution-core/tests/autonomy_ladder_contract.rs | pins rung_required_for to is_risky_call; pins check_autonomy_rung shape | file-level | clean | the_required_rung_is_decided_from_the_call_not_the_tool_name (68); the_rung_gate_runs_in_the_loop_and_never_in_the_tool_catalogue (85); an_unstated_rung_is_unconstrained_in_the_loop_and_narrowest_in_the_contract (108); both_the_gateway_and_session_core_validate_the_grant (135) |
| execution-core: comment-only references to trusted_capability_id | plan.rs:17; process_tools.rs:12; subagent/mod.rs:130; health_attest.rs:16; subagent/research_fan_out.rs:34 | as listed | clean | none |
| model-gateway/src/tool_loop.rs | inline_tool_allowed; dispatch refusal; MCP fallback arm; continuation doc | 601-620; refusal 2489-2494; fallback 3679-3682 | M (425 changed; +24 at line 345 shifts MCP refs to 625-644, 2515-2518, 3686-3689) | direct_mcp_tool_is_denied_before_registry_dispatch (8929); mcp_catalog_is_denied_in_inline_chat (8966); mcp_call_* (8807-8890); inline_loop_rejects_side_effects_that_require_governed_agentic_approval (9106); inline_loop_denies_mcp_until_governed_agentic_dispatch_exists (9430) |
| model-gateway/src/mcp_jsonrpc.rs | tools/list annotation parse; McpToolAnnotations; McpSideEffect; side_effect(); JSON-RPC framing | structs 64-97; side_effect 108-124; parse 246 | clean | parse_extracts_every_annotation_hint_the_server_supplies (463); side_effect_applies_the_specs_defaults_without_inventing_claims (480); malformed_annotations_yield_no_claims_rather_than_a_guess (521) |
| model-gateway/src/runtime_registries.rs | McpRegistry (DashMap); handle_proxy_mcp_tool (owner check, enabled, exact allowlist, http only); mcp_capability_payload; filter_allowlist; discovery cache; staging; allowlist validation; HookRegistry | McpRegistry 137-146; handle_proxy 1741-1795; payload 1085-1110; filter 497; discover cache 512; staging 624-689; allowlist 946-968; HookRegistry 2002-2005 | clean | exposes_only_successfully_discovered_exact_allowlist_tools (1617); filter_allowlist_matches_exact_names_only (1597); stage_mcp_tool_defs_collapses_to_two_synthetic_tools_over_threshold (2366); mcp_register_assigns_id (2495) |
| model-gateway/src/state.rs | AppState fields mcp, plugins, hooks | 308, 314, 317 | M (6 changed, boot-time env check) | none |
| model-gateway/src/grpc.rs | ProxyMcpTool (ZDR gate 1778, service-only check 1779); ListMcpTools | WT fn 1773 (handler call 1798); WT fn 1812 | M (47 changed); LIVE ProxyMcpTool 1782, gate 1787 | not read |
| model-gateway/src/capability_consumer.rs | NATS reconcile consumer; capability.mcp_server.removed clears gateway cache | reconcile_action 60-84; consumer 90-130 | clean | reconcile tests 151-222 |
| model-gateway/src/moderation.rs | TrustClass enum and classify (source of the duplicated rule) | enum 206; classify 228 | clean | classify tests 917-945 (mcp__srv__create_order 925; mcp__github__create_issue 929) |
| model-gateway/src/sse.rs | awaiting_approval event; browser_action_approval_required; outcome map | WT 8878, 9536, 9864; LIVE 9238, 9897, 10226 | M (883 changed) | classify_agentic_run_outcome("awaiting_approval") (WT 11870) |
| model-gateway/src/http_routes.rs | MCP server HTTP handlers; tool_allowlist resolution | tool_allowlist hits WT 2679-2849, 3147-3441 | M (237 changed) | not read |
| model-gateway/src/ownership.rs | per-user usable() check for KIND_MCP | called at runtime_registries.rs:1752-1759 | clean | not read |
| capability-core/internal/models/capability.go | RiskLevel constants; IsSupportedRiskLevel; seeded high-risk ids | 24-29; 35-42; 68+ | clean | models/capability_test.go; availability_test.go |
| capability-core/internal/policy/engine.go | EvaluateCapability; denyOnScope; grant checks; risk switch | 97-154; switch 112-153 | clean | engine_test.go TestEngine_EvaluateCapability_ArgumentValidation (23), TestEngine_ScopeEnforcement (167); engine_security_test.go TestEngineDeniesUnknownRiskLevel (105); engine_sandbox_test.go TestEngineAllowsHermeticSandboxWhileArbitraryShellStillAsks (55); engine_grant_test.go TestEngine_DurableGrant_* (64-189) |
| capability-core/internal/server/server.go | EvaluatePolicy gRPC calls s.policy.EvaluateCapability | 489 | clean | server_test.go TestEvaluatePolicy (800), TestEvaluatePolicyUsesDurableTenantAndFailsClosedRuntimeState (386); authz_test.go TestAuthorizeGRPCPinsEvaluatePolicyAndProtectsPromotion (117) |
| capability-core/internal/registry/registry.go | staticSeedSource rows: cap.mcp.filesystem (46), cap.memory.index RiskLow (30), cap.command.shell RiskHigh (50), cap.command.sandbox RiskLow (51-52); NewFromSource(staticSeedSource{}) | 25-50; 132 | clean | registry_test.go; migration_security_test.go |
| capability-core/internal/registry/capabilities_source.go; capabilities_store.go | DB-backed capability source and store | Load 38 | clean | scope_store_integration_test.go; models_integration_test.go |
| capability-core/internal/api (mcp_dns_revalidator.go, plugins.go) | MCP DNS revalidation; plugin and MCP surfaces | file-level | clean | mcp_dns_revalidator_test.go; mcp_auth_test.go; mcp_oauth_internal_test.go |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mcp.rs | BFF routes for MCP servers; no per-tool policy field | routes 46-54 | clean | not read |
| apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts | MCP delete and share input schemas; register and connect deliberately unregistered; chatApprovePlanInput grantedRung | 785-795; 1021-1026 | clean | action-registry.test.ts; agent-tools.test.ts; action-client.test.ts |
| docs/actions-surface-operations.md (repo root) | frozen operation and capability contract (source of is_write) | located only | clean | n/a |
| apps/Ingestion Plane/integration-corev2 | catalogue metadata dependency (§9.1 row) | not compared | M (several files) | not read |

## 5. Related contracts

Paths are relative to apps/Model Plane/ unless stated.

| Contract | Location | Relevance | Live |
|---|---|---|---|
| execution.proto RunAgentRequest | proto/model_plane/v1/execution.proto:76-211 (zdr=9 at 107; tools=10 at 117; min_privacy_tier=11 at 125; plan_mode comment 126-132 cites is_risky_call at 129; autonomy_rung=12 at 154) | run-level zdr and rung inputs | clean |
| execution.proto ExecuteStepRequest | :268 (permission_mode=5 at 282); ExecuteStepResponse :321 | direct step path | clean |
| capabilities.proto CapabilityDetail | :62 (risk_level=6 at 79; stale comment 78) | risk vocabulary drift | clean |
| capabilities.proto EvaluatePolicy | :123 (request), :146 (response) | capability gate contract | clean |
| gateway.proto MCP RPCs | rpc ProxyMcpTool :146; rpc ListMcpTools :151; ListMcpToolsRequest :1123; ListMcpToolsResponse :1134 (tools = ToolDefinition); ProxyMcpToolRequest :1141 (user_id=6, comment 1148-1151); ProxyMcpToolResponse :1155 | MCP wire; no annotation or class field | clean |
| inference.proto ToolDefinition | :178-185 (name, description, parameters_json) | annotations cannot travel | clean |
| openapi.yaml risk_level | apps/Model Plane/api/openapi.yaml:1553 | HTTP contract | clean |
| TS SDK Capability | apps/Model Plane/api/sdk/typescript/src/models/Capability.ts:130, 159 | riskLevel mapping (generated) | clean |
| Contract goldens | apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256) | feature key mcp_execution at line 153 | ?? |
| SSE events | sse.rs awaiting_approval (WT 9536-9540; LIVE 9897-9901); browser_action_approval_required (WT 9864; LIVE 10226) | approval surface | M |
| Gateway BFF routes | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mcp.rs:46-54 | /api/v1/mcp/servers, /{id}, /{id}/share, /oauth/start, /oauth/callback, /connect | clean |
| Frontend action contracts | src/shared/actions/action-registry.ts:785-795, 1021-1026 | add contract before UI operation (root CLAUDE.md) | clean |
| SQL capability-core | migrations/0003_capabilities_registry.up.sql (capabilities.risk_level :23; capability_scopes :94; mcp_servers :188-213; mcp_oauth_tokens :220; registry_audit_log :321); 0007 (risk CHECK :27-29). Latest: 0015_background_process_capability.{up,down}.sql | no tool-policy table; latest 0015 | clean |
| SQL session-core | migrations WT latest 0043_audit_outbox_requeue.sql; approval continuation 0018_approval_continuation_receipts.sql, 0022_encrypted_approval_continuation_descriptors.sql | continuation descriptors | ?? adds 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql |
| Env SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY | session-core/src/continuation_crypto.rs:14; deploy/docker-compose.yml:755 (optional); deploy/docker-compose.production.yml:70 (required); scripts/release-artifact.sh:80 | 9.1 dependency | clean |
| Env MODEL_GATEWAY_ADDR / MODEL_GATEWAY_GRPC_URL | execution-core/src/mcp_gateway.rs:20, 29-37 | MCP transport address | clean |
| Env NATS_URL | model-gateway/src/capability_consumer.rs:101 | reconcile consumer | clean |

## 6. Test and build commands

- Rust, CI gate: `cd "apps/Model Plane/rust"` then `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:57, 99, 112, 119`).
- Rust, per crate (CLAUDE.md, Model Plane): `cargo test -q -p execution-core --all-targets`; `cargo test -q -p model-gateway --all-targets`; `cargo test -q -p mp-contracts --all-targets`. Contract test: `cargo test -p execution-core --test autonomy_ladder_contract`. Package names from `Cargo.toml` line 2 in each crate.
- Windows (user memory, model-plane-build-and-deploy): two cargo targets fail on Windows (pre-existing); use `--lib`. Not re-checked here.
- Go: `cd "apps/Model Plane/go/services/capability-core"` then `go test ./...` and `go vet ./...` (CLAUDE.md, Testing). Module `github.com/triodelab/model-plane/services/capability-core` (go.mod:1). No `.github/workflows` file references capability-core (grep). Go -race needs cgo, unavailable (user memory).
- Proto: `buf generate` to regenerate; CI runs `buf lint apps/Model Plane/proto` and `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (`.github/workflows/buf.yml:21-23`). buf lint has pre-existing debt (CLAUDE.md).
- Contract goldens (CI): `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test` (`model-plane-rust-ci.yml:95-96`). Needed if api/contracts changes.
- Frontend, only if action contracts change: `cd "apps/Frontend Plane/verevonv3"` then `pnpm lint`, `pnpm typecheck`, `pnpm test`; gateway `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (root CLAUDE.md, Build And Run).
- Live stack: `cd "apps/Model Plane" && ./scripts/compose.sh up -d` (CLAUDE.md; user memory compose.sh recipe).

## 7. Owners

- Model Plane owns capabilities, execution loops and sandboxes; Control Plane owns identity, orgs, audit and entitlements (root `CLAUDE.md`, Architecture Rules).
- Tool and MCP registries: capability-core is the system of record; gateway registries are read-through caches; "No second registry implementation" (`apps/Model Plane/docs/capability-ownership-matrix.md:59, 104-106`).
- Policy and RBAC: capability-core `policy/engine.go` decides; execution-core `permission/` enforces (`capability-ownership-matrix.md:60`).
- MCP registry: capability-core-go owns it; Rust runtime adapter (`apps/master-ownership-matrix.md:85`). execution-core owns tool loop and hooks (`master:128`); capability-core owns tools, MCP, safety (`master:130`).
- Two dispatch loops stay separate; their refusal boundary is the authority boundary (`apps/Model Plane/CLAUDE.md`, Code Style). One canonical owner per capability; check the ledger first (same file).
- Ledger rulings: "service token for MCP" rejected (`apps/Model Plane/docs/decisions/ledger.md:1548-1553`); MCP output shape detection (1571-1576).
- ZDR must propagate through any content-persisting boundary (root `CLAUDE.md`, Architecture Rules).
- Gateway domains must not leak upstream secrets or forged scoping headers (root `CLAUDE.md`, Code Style).
- Conflict: `execution-core/src/mcp_gateway.rs:3-4` says the gateway owns MCP registration, discovery and proxy (citing matrix §G2). `capability-ownership-matrix.md:106` and `master:85` say capability-core. Authoritative owner: NOT DETERMINED (§9).
- Plane per area: Model Plane (execution-core, model-gateway, capability-core, mp-contracts, session-core approvals); Frontend Plane (action-registry.ts, gateway mcp.rs); Ingestion Plane (integration-corev2 catalogue, the §9.1 dependency).

## 8. Uncommitted-only (LIVE)

Source: `live-tree-status.txt` and `git --no-optional-locks diff --stat` in C:/dev/CoresSystem.

| File (LIVE) | Status | One-line summary |
|---|---|---|
| execution-core/src/runtime_loop/agent.rs | M (207 changed) | Hunks in run_agent (571, 599), run_agent_with_tools (749, 769), run_rounds (1937), continuation_descriptor (4566), and many added test lines (7185-13410); contents not read. Leaf, rung and plan-mode gate bodies (LIVE 1645, 1692, 1806) sit outside every hunk. |
| execution-core/src/runtime_loop/mod.rs | M +18 | Adds `waived: bool` to execute_step, execute_step_with_browser_grant, execute_step_with_subagent, execute_step_inner (sig 484) and execute_knowledge_search. Gates shift only (capability 501-552, MCP arm 761). |
| execution-core/src/grpc.rs | M (98 changed) | Hunks in create_durable_approval (660), enforce_persistence_free_execution (667-670), validate_direct_permission_mode (749), enforce_agent_route (764-766), authorize_scheduled_step_service (834); contents not read. |
| model-gateway/src/tool_loop.rs | M (425 changed) | +24 lines at 345 move MCP refs to 625-644; web-search gating and dispatch_tool_with_cost changes. inline_tool_allowed body unchanged. |
| model-gateway/src/grpc.rs | M (47 changed) | Gate helper refactored: WT 279-289 (`effective_zdr(false)`, failed_precondition) becomes LIVE 279-290 (`zdr_for(Route::plain(false))`, typed `zdr_refusal::Refusal`). MCP feature key renamed to "mcp_execution" (LIVE 1787; WT 1778 says "MCP execution"). Same gate already covers send_message (WT 1266), lsp_query (1426), command execution (1996), speech (2325, 2361). |
| model-gateway/src/state.rs | M (6 changed) | Boot-time check for VEREVON_RELEVANCE_CROSS_ENCODER_SCORES (716-723). Fields unchanged; hooks still 317. |
| model-gateway/src/sse.rs | M (883 changed) | Not read in detail. awaiting_approval event still present (LIVE 9897). |
| model-gateway/src/sse_events.rs; http_routes.rs | M (203 changed); M (237 changed) | Not read. http_routes.rs holds WT MCP server handlers. |
| execution-core/src/auth.rs | M (631 changed) | Not read. |
| proto/model_plane/v1/sessions.proto | M +42 | Not read. execution.proto and capabilities.proto are clean. |
| mp-contracts/src/lib.rs | M +2 | Module exports; not read. |
| mp-contracts/src/zdr_refusal.rs | ?? | Feature inventory with `mcp_execution` (LIVE 292-295). |
| api/contracts/zdr-refusals.json (+ .sha256) | ?? | Golden with `mcp_execution` (LIVE 153). |
| model-gateway/src/zdr_posture.rs; zdr_features.rs | ?? | No mcp references (grep). zdr_precheck.rs is M; not checked for MCP. |
| session-core/migrations/0044_run_retention_receipt.sql; 0045_zdr_waived_runs_org_index.sql | ?? | Latest session-core migrations in LIVE; not MCP-related. |
| .github/workflows/model-plane-rust-ci.yml | M | Adds job `session-core-receipts` (Postgres, zdr_receipt tests; LIVE 121-170). Existing cargo steps unchanged. |
| apps/Model Plane/docs/sse-event-taxonomy.md | M | Not read. |
| Clean (no uncommitted change) | clean | permission/mod.rs; capability_policy.rs; mcp_gateway.rs; mcp_jsonrpc.rs; runtime_registries.rs; hook/mod.rs; provenance.rs; moderation.rs; integration_tools.rs; retry.rs; autonomy.rs; execution.proto; capabilities.proto; inference.proto; gateway.proto; openapi.yaml; capability-core/ (all); capability-ownership-matrix.md; master-ownership-matrix.md; ledger.md; action-registry.ts; gateway domains/mcp.rs |

## 9. Not determined

1. Whether the MCP ZDR refusal should key on the caller identity (as coded, WT grpc.rs:1778) or on the run's zdr flag plus the server's attestation boundary (card wording, "How to build it"). The run flag never reaches `execute_mcp`.
2. How `effective_zdr` (WT) and `zdr_for` (LIVE) derive a caller identity's ZDR state. Not traced; zdr_posture.rs was checked by grep only.
3. Whether the integration-corev2 catalogue matches the mirrored CATALOG in integration_tools.rs (71+). `docs/actions-surface-operations.md` was located, not read; integration-corev2 was not compared.
4. Whether SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY is provisioned in any deployed environment. Only compose wiring was seen.
5. Whether capability-core holds MCP tool-level rows, or validates tool_allowlist on its write path. Only the gateway payload builder was traced (runtime_registries.rs:1085-1110).
6. Which MCP-registration owner is authoritative (§7).
7. Whether the stdio quarantine (runtime_registries.rs:1785-1795) is intended. capability-ownership-matrix.md:142 says it is wired.
8. Content of the LIVE changes to sse.rs, sse_events.rs, http_routes.rs and execution-core auth.rs. Not read.
9. Whether mp-contracts/src/autonomy.rs has its own unit tests. Not checked.
10. Test and build status of any file. Nothing was run, so pass or fail is undetermined.
