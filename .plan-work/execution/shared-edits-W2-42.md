## WS14-S4
shared-edit apps/Model Plane/go/services/capability-core/migrations/0029_ask_user_tool_class.up.sql and its .down.sql (new; number per DEC-39): insert ('global', 'cap.memory.index', 'ask_user', 'context_write', 'none', NULL, 'seed'), shaped like plan.propose's row in 0016_capability_tool_classes.up.sql (line 139). Required before TOOL_CLASS_GATE=enforce is enabled anywhere.

## WS14-S4
shared-edit apps/Model Plane/rust/services/execution-core/tests/tool_class_registry_parity.rs: add the new migration's file name to the later-migrations list in seeded_rows(). This turns the one red parity test green.

## WS14-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core and model-gateway service blocks add AGENT_ASK_USER_ENABLED: ${AGENT_ASK_USER_ENABLED:-false}. The code already defaults to off when unset.

## WS14-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx (ComposerTrays mount, around lines 919-943): question={controller.pendingQuestion() ?? undefined}, questionPending={controller.questionPending()}, questionError={controller.questionError()}, onAnswerQuestion={controller.answerPendingQuestion}, onSkipQuestion={controller.skipPendingQuestion}. Confirmed: the hook returns these as accessors (use-chat-controller.ts return block, lines 4774-4778). Until this lands the card is not visible.

## WS25-S11
shared-edit apps/Frontend Plane/verevonv3/src/app/App.tsx: add `const CustomizeHubPage = lazy(() => import('@/features/customize/CustomizeHub'))` with the other lazy page imports, and in the RequireAuth children add `{ path: '/agents/customize', component: CustomizeHubPage },` after the `/agents/quality` route.

## WS25-S11
shared-edit apps/Frontend Plane/verevonv3/src/features/core/lib/sidebar-navigation.ts: in the agents-core group, after the `agents-quality` item, add `{ id: 'agents-customize', label: 'Tilpass', href: '/agents/customize', icon: SlidersHorizontal, description: 'Agentmodus, hooks, MCP-servere, ferdigheter, instrukser og plugins.' }` and add SlidersHorizontal to the '@/shared/icons' import. Then run src/features/core/lib, which pins the nav shape.

## WS25-S11
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: import loadEffectiveAgentModeCap from '@/features/customize/AgentModesSection'; beside projectContext (about line 168) add `const [agentModeCap] = createResource(() => loadEffectiveAgentModeCap())`; on the DashboardComposer element (about line 363) add `agentModeCap={agentModeCap()}` beside `planMode={planMode()}`.

## WS25-S11
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: (a) in the request build (about line 3155) add `agentMode: options.agentMode,` after `connectorScope: options.connectorScope,`; (b) in the sendContent options object for the composer payload (about line 3521) add `agentMode: payload.agentMode,` after `connectorScope: payload.connectorScope,`. SendOptions already declares agentMode. WS14-S4 edits this file in the same batch, so merge the hunks.

