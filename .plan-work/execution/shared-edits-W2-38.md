## WS15-S5
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\eslint-suppressions.json: prune the no-restricted-syntax entry for src/features/chat/lib/stream-activity.ts (recorded count 3, now 0). Run the prune command the comment in eslint.ui-gates.config.js gives (--prune-suppressions over src), then confirm pnpm lint exits 0. Until then eslint exits 2 on this slice's files.

## WS15-S5
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\src\styles\global.css: add .verevon-chat-lifecycle-notices {display:flex; flex-direction:column; align-items:flex-start} and give .verevon-chat-lifecycle-notice the same declarations as .verevon-chat-source-scope-notice (around line 27290). Also add .verevon-chat-lifecycle-notice to the svg flex-shrink group near line 27281. Without this the notes render legible but unstyled.

## WS15-S5
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\docs\UI_STATE_CHECKLIST.md: add a 'Retrying and trimmed' state row. Automated: ChatMessages.lifecycle.test.tsx (sentences in both locales; a plain turn is silent), use-chat-controller.lifecycle.test.ts (collected, replay dropped, cap 20, session-only), stream-activity.test.ts (retrying line), tests/e2e/chat-lifecycle-cues.spec.ts (browser). Tick it only on browser evidence.

## WS22-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/ag-ui-client.ts: in toAgUiEvent (switch on VerevonUiEvent, around line 195) add `case 'lifecycle.sandbox':` beside `case 'lifecycle.context':`, returning null. Without it the exhaustive switch no longer type-checks (pnpm typecheck fails). I am recording this as needed and have not edited the file.

