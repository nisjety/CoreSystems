## WS10-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/core/lib/shell-data.ts: add '/settings/model-access' to the VerevonRoute union (beside '/settings/mcp', line ~53) and `if (path.startsWith('/settings/model-access')) return '/settings/model-access'` in routeFromPath (beside line ~254); then remove the `MODEL_ACCESS_HREF` cast constant in sidebar-navigation.ts and use the literal href.

## WS10-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/core/components/CoreSidebar.tsx line ~107: `applyDemoModeNavGate(sidebarSections, { modelAccessEnabled: <BFF enabled flag from getModelEntitlement(orgId)> })`, with the flag read only for admins (`shouldShowWorkspaceAdminNavigation(session)`).

## WS10-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/CoreSidebarSettingsPanels.tsx: add `{ id: 'model-access', label: 'Modelltilgang', icon: BrainCircuit, href: '/settings/model-access' }` beside the mcp entry (line ~54), shown only under the same BFF flag.

## WS10-S6
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css: optionally add a `.verevon-ai-model-picker__notice` rule (font-size 11.5px, opacity 0.8, margin 4px 0 0) and drop POLICY_NOTICE_STYLE from AiModelPicker.tsx.

## WS07-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core service env (next to DREAMING_LLM_EXTRACTION, about line 878) add: '      # WS07-S1 kill switch. Default true: the memory gate enforces each user paused/off state. false restores pre-S1 behaviour (every user treated as on) and logs ERROR once per Dreaming cycle; SetMemoryControls is then refused.' then '      MEMORY_CONTROLS_ENABLED: ${MEMORY_CONTROLS_ENABLED:-true}'

## WS08-S3.code
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs: add `image_parts: Vec::new(),` to the pb::InferRequest literal at ~2720 (it ends with `prompt_tiers: prompt_tier_plan(req.run_context.as_ref(), &messages),` at ~2774; add the new line after it).

## WS08-S3.code
shared-edit apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs: add `image_parts: Vec::new(),` after `prompt_tiers: None,` in the InferRequest literal at ~7067 (literal begins at ~7038).

