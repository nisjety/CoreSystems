## EP-13-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment, directly after the line `      MODEL_GATEWAY_AUX_MODEL: ${MODEL_GATEWAY_AUX_MODEL:-}` (about line 226), add the line `      MODEL_GATEWAY_CATALOGUE_PRICES: ${MODEL_GATEWAY_CATALOGUE_PRICES:-false}`.

## EP-14-S1
shared-edit ChatPage.tsx (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx): (1) after line 82 add `import { foldPlan, settledFoldEnabled } from '../lib/settled-fold'`; (2) before `const messageRows = createMemo(` (line 452) add `const settledFolds = createMemo(() => (settledFoldEnabled ? foldPlan(state.turns, { activeTurnId: (isStreaming() || liveRunId()) ? state.turns.at(-1)?.id : undefined }) : undefined))`; (3) in the messageRows row object, after `docked: dockedNodesFor(dockedTrays(), turn.id),` (line 462), add `settledFold: settledFolds()?.get(turn.id),`; (4) in renderMessageRow's `<MessageBlock`, after `docked={row().docked}` (line 1101), add `settledFold={row().settledFold}`. Until these four lines are in, VITE_CHAT_SETTLED_FOLD changes nothing.

