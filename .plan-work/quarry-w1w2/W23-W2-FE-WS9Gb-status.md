# W2-FE-WS9Gb status: WS9.G integration: ChatPage, live run panel and controller (lane a, branch w23/lane-a)

## Built
Checked first. The Gb gap was built; nothing in the lane already covered it. Ga's dock modules (`browser-dock-state.ts`, `BrowserDockControls.tsx`, `browser-dock.css`, and the `toggleBrowserDock`/`cycleBrowserDock` options in `use-chat-shortcuts.ts`) are NOT in the lane: W2-FE-WS9Ga was blocked in S5 and committed nothing. The Gb code is written against Ga's draft contract. Only export names were read from the `ws9ga-*` drafts in the vigorous-perlman scratchpad; no Ga file was copied into the lane.

- `ChatLiveRunPanel.tsx`: optional `dockControls?: JSX.Element` prop, placed in the header after the run id. It shows only while the panel is open and the Work tab does not own the rail.
- `use-chat-controller.ts`: `useBrowserDock()` owns the live panel layout. `runPanelCollapsed` is `position === 'hidden'`. `toggleRunPanel` hides an open dock, or restores the last visible layout of this visit (defaults to right). New return names `browserDock` and `setBrowserDockPosition`; `runPanelCollapsed` and `toggleRunPanel` keep their names. The controller no longer writes the old collapse key.
- `chat-run-watch.ts`: the old key constant and `readChatRunPanelCollapsed` stay, documented as the read-only migration source. `writeChatRunPanelCollapsed` stays exported with no production caller, because `chat-run-watch.test.ts` (not in scope) round-trips it.
- `ChatPage.tsx`: `dockLayout` memo (flag on, live run shown, position not hidden). The page root takes `verevon-chat-page--dock-right|bottom|maximised` and `--browser-dock-size` while docked. `liveRailDockControls` memo is passed as `dockControls`. `useChatShortcuts` receives `toggleBrowserDock` and `cycleBrowserDock`, each acting only with a live run open. `ChatWideResearchPanel` is untouched and still mounted in the rail. ZDR copy is untouched.
- Tests: `ChatLiveRunPanel.test.tsx` gets four new tests appended (picker in header; absent while collapsed; absent while Work owns the rail; one element shown once and again after a collapse and restore). Existing tests are untouched. `ChatPage.browser-dock.test.ts` (new) pins the class list, the mounts, the chords and the controller record against the source, as `workspace-rail-geometry.test.ts` does. `tests/e2e/browser-workspace-chat-dock.spec.ts` (new, not run) covers right by default, bottom kept across a reload, maximised, hidden, and restore from the rail.

## Commits (lane a, local, not pushed)
- 44aa5105c feat(chat): dock picker slot in the live run panel header (W2-FE-WS9Gb)
- fe8b5cd7a feat(chat): the live panel layout lives in the browser dock record (W2-FE-WS9Gb)
- 33f4ad9ab feat(chat): ChatPage mounts the browser dock classes, picker and chords (W2-FE-WS9Gb)
- e13bce2be test(e2e): browser dock beside a live run, written and not run (W2-FE-WS9Gb)
- this file and the lane a depth rows: docs(plan-work) commit that adds it (sha in the lane log)

## Code only, not compiled
No tsc, build, vitest or Playwright run (owner order 2026-11-03). The lane has no node_modules. Bracket balance was checked heuristically against HEAD (unchanged per file). The integration step must compile-check after W2-FE-WS9Ga lands:
`pnpm typecheck` (baseline 33), then `pnpm exec vitest run src/features/chat`, then `pnpm exec vitest run src/features/browser/dock`.

Edit and Write refused the lane files under the harness different-worktree rule. Per the unit instruction, the lane files were written with python (bytes), each block required to match exactly once, and CR/LF was checked (all lane files here are LF).

## Flag and default
`VITE_CHAT_BROWSER_DOCK`, read through Ga's `isChatBrowserDockEnabled()` (only `1` or `true` turn it on). Unset means off in every build, and no env file was touched. With the flag off there is no picker, no dock class, no `--browser-dock-size`, and no chord action. The collapse state itself now lives in the dock record in both flag states (decision 1).

## Assumed decisions
1. The layout record replaces the old collapse flag in every build. The flag gates only the positions, the picker, the classes and the chords. This follows the design's single record and keeps the controller's write unconditional. The old key is read once by Ga's migration and never written.
2. Hiding keeps the last visible layout for this visit only. A reload with a hidden record restores right. The design fixes the record at four fields, so the restore target is not stored.
3. The picker shows only while the panel is expanded and the Work tab does not own the rail.
4. Dock classes apply whenever a live run is shown and the layout is not hidden, whatever the canvas tab. The CSS (Ga) excludes canvas tabs.
5. The chords (`toggleBrowserDock`, `cycleBrowserDock`) do nothing without a live run, on top of Ga's flag check.
6. Two return names are added to the controller (`browserDock`, `setBrowserDockPosition`). The design called the hook "six lines"; it is larger, because the restore memory and the record write are both needed.
7. `writeChatRunPanelCollapsed` stays exported with no production caller, so the existing round-trip test keeps passing without editing a file outside scope.
8. The class-list test is a source pin, because the full page has no unit harness (see `ChatPage.voice-first-turn.test.tsx`). It follows the precedent of `work-content-identity.test.ts` and `workspace-rail-geometry.test.ts`.
9. The Playwright spec runs in the `local` project, which the `browser-workspace-*` pattern selects. Its deep-research control locator is a guess (bilingual label) and is marked UNVERIFIED in the spec.
10. Ga names used by Gb, taken from the drafts: `useBrowserDock`, `browserDockSizeCss`, `isChatBrowserDockEnabled`, `nextBrowserDockPosition`, `toggleBrowserDockPosition`, `BrowserDockControls` (props `position`, `onPositionChange`), types `BrowserDockPosition` and `BrowserDockVisiblePosition`, and `toggleBrowserDock?`/`cycleBrowserDock?` on `UseChatShortcutsOptions`. If Ga lands with other names, fix them at integration.

## MP asks
None. No Model Plane file or contract is touched, and the dock uses the existing run-event stream and Quarry records only.

## Not verified
- Nothing compiled or run. The new ChatLiveRunPanel tests, the class-list pins and the Playwright spec have not executed.
- Hard dependency on W2-FE-WS9Ga. Until Ga is on the integration branch, `ChatPage.tsx` and `use-chat-controller.ts` do not resolve their imports, and every test that imports the controller fails with them (about ten `use-chat-controller.*.test.ts` files). Do not integrate Gb before Ga, or land Ga's files first.
- Visual layout of the bottom and maximised dock (CSS is Ga's), the narrow-rail header, and the `Mod+Alt` chords on macOS Safari (Ga).
- The dock picker's keyboard order inside the header.

## Depth rows
Appended to `W23-depth-lane-a.md` as `W2-FE-WS9Gb` rows (D-WS9Gb-1 to D-WS9Gb-10).
