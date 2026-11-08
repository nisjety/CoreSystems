## WS25-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs: register_hook (~2380) and list_hooks (~2389). In register_hook, bind the identity from authorize_rpc(&request, RpcAccess::Write) instead of discarding it. When runtime_registries::hooks_durable() is true, build HookWriteCaller { zdr: identity.zdr_for(&crate::zdr_posture::Route::plain(false))?, admin: <org admin, mirroring ownership::is_admin_claim incl. the :admin suffix> }, then return runtime_registries::proxy_register_hook(&self.state.hooks, &self.state.http_client, &self.state.capability_core_base_url, <token>, caller, request.into_inner()). For list_hooks call proxy_list_hooks with the same arguments minus caller. <token> is forwarded_authorization(&request) with the Bearer scheme stripped (trim_start_matches("Bearer ").trim(), as set_permission does), because the hydrate path applies bearer_auth itself. Keep handle_register_hook and handle_list_hooks for the off path.

## WS25-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: the Do-run call at ~11244 (run_agent_req.org_policy = do_run_org_policy(&state.hooks, org_id, hooks_do_runs_mode())). In durable mode, await runtime_registries::ensure_org_hooks(&state.hooks, &state.http_client, &state.capability_core_base_url, org_id, <caller token without the Bearer scheme>) before it, and refuse the run on Err (typed org_policy_unavailable, the WS25-S9 code, until S9 lands). The enclosing function may need to become async. sse.rs has WS17-S5 edits in progress from another agent.

## WS25-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/tools.rs: the code_interpreter hook_context at ~232, and apps/Model Plane/rust/services/model-gateway/src/browser_run.rs: hook_context_json(&state.hooks, &org_id) at ~1485. Same ensure-then-read in durable mode; refuse the tool call on Err.

## WS17-S5
shared-edit crates/mp-content-safety/src/trust.rs: in TOOL_TRUST add `("recall_conversations", TrustClass::PriorConversation),`; in the tests add `assert_eq!(TrustClass::classify("recall_conversations"), TrustClass::PriorConversation);`. Then run `cargo test -p mp-content-safety` and check execution-core/tests/compaction_parity_contract.rs for TrustClass cases.

## WS17-S5
shared-edit services/model-gateway/src/prompt_context.rs line 415: `const CONDITIONAL_TOOLS: [&str; 4] = ["reattach_context", "web_search", "skill_file", "recall_conversations"];` and the doc comment above it. Then rerun the model-gateway lib suite (prompt_context tests pin this order).

## WS21-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx: (1) add imports BrowserSitesSection from '@/features/settings/components/BrowserSitesSection' and OrgBrowserPolicySection from '@/features/settings/components/OrgBrowserPolicySection'; (2) in the Record map near line 243 (next to 'model-access': []) add 'browser-sites': [] and 'org-browser-policy': []; (3) near the model-access Match (~line 506) add <Match when={props.section === 'browser-sites'}><BrowserSitesSection /></Match> and <Match when={props.section === 'org-browser-policy'}><OrgBrowserPolicySection /></Match>. Effect: clears TS2739 and the failing negative-render test.

