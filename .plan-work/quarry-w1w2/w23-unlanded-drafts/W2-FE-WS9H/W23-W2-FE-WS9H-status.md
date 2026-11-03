# W2-FE-WS9H status: WS9.H interactive dock pane (dark), lane a (branch w23/lane-a)

## Status: NOT LANDED. Drafts only.

This session is rooted at `vigorous-perlman-e7ae5b`. The harness refused Write into the lane worktree (`innh-lane-a`) under the different-worktree rule. Entering the lane with EnterWorktree was not done: that tool is for explicit worktree instructions from the user or CLAUDE.md, and this unit's instruction came from a workflow script. Writing the lane files through Bash or python would have routed around the same refusal, so it was not done either. Nothing is in the lane: no commits, no edits, no status file, no depth rows. The lane checkout was read only, and `git status` in the lane is still clean.

The code is staged in the scratchpad folder `ws9h-drafts/`. It must be applied from a session rooted at `innh-lane-a`.

## Built (code only, not compiled)

| Draft file | Lands at (lane) | Notes |
|---|---|---|
| `chat-per-tab.ts` + `chat-per-tab.test.ts` | `src/features/browser/dock/` | New. In-memory `Map` per chat page, `shouldStartNewThread`, `planChatForTabSwitch`, default `keep` |
| `dock-session.ts` + `dock-session.test.ts` | `src/features/browser/dock/` | New. `useDockBrowserSession`: one controller per chat page, ZDR from the thread and the retention verdict, close then dispose on unmount |
| `OpenBrowserButton.tsx` | `src/features/browser/dock/` | New. `OpenBrowserButton` (chat header toggle) and `AskAboutPageButton` (focuses the composer, no page content) |
| `InteractiveBrowserPane.tsx` + `InteractiveBrowserPane.test.tsx` | `src/features/browser/dock/` | New. Mounts `BrowserChrome` compact, start state with omnibox, search results, navigation error dismissal, keep-alive, limit row |
| `interactive-browser-pane.css` | `src/features/browser/dock/` | New file, beyond the listed files. Convert to CRLF on apply, like the other dock stylesheets |
| `ws9h-chatpanels.patch` | `src/features/chat/components/ChatPanels.tsx` | `ChatHeader` gets an optional `browserSlot` prop and one render line |
| `ws9h-chatpage.patch` | `src/features/chat/components/ChatPage.tsx` | Dock body state (run or browser, page-local), chat-per-tab effect, ask and prefill, header memo, pane mount, one class element, run panel hidden while the browser body shows on the chat tab |

Both patches were generated with `diff -u` from the lane's current files and pass `git apply --check` against the lane checkout as it stands now. The ChatPage patch was regenerated after the temporary-chat guard was added, and it still passes.

Existing pins: `ChatPage.browser-dock.test.ts` (Gb) pins 18 exact strings in `ChatPage.tsx`. All 18 are still present in the edited copy, so that test should stay green. It was not edited.

## Flag and default

- `VITE_CHAT_BROWSER_DOCK`, read through `isChatBrowserDockEnabled()` from Ga. Only `1` or `true` turns it on.
- Flag off: no controller is created, no header button, no pane, no class element, and the chat-per-tab effect does nothing. The run watch behaves as it does today.
- The pane is not enabled for users before the Model Plane attach mode (WS8.G), per the queue note. The flag stays off in production builds.

## Assumed decisions (owner said stop asking; recommended defaults taken)

1. Chat per tab defaults to `keep` (X-18). `new` is reachable only through the dock record field `chatPerTab`. There is no UI for it yet.
2. Under `new`, switching browser tabs returns to that tab's remembered thread, or starts a new thread when it has none. The map is memory only and is dropped on reload.
3. A temporary (zero-retention) thread is never remembered for a tab, to keep the Temporary chat promise.
4. "Ask Verevon" from the omnibox prefills the composer with the typed text and focuses it. "Ask about this page" only focuses the composer. No page content is attached (F-t2-028 owns governed page context).
5. Zero retention: a session is ZDR when the thread on screen is temporary, or when the workspace retention verdict denies local copies. A ZDR session carries no profile. The page passes no profile at all.
6. The dock body (run or browser) is page-local. The layout stays in the dock record. Showing the browser needs no live run. Closing the browser with no live run writes `hidden` to the dock record, so the next run starts collapsed (depth row D-WS9H-5).
7. The pane mounts `BrowserChrome` compact and does not pass the agent loop callbacks (`onBrowserAutoRun`, `onBrowserSuggestAction`). The model controls stay disabled in the dock, and the run watch keeps the loop.
8. The header shows a persistent "This session is limited to {host}" row, which matches the pre-WS8 first-host limit. It is hidden while a cross-host refusal card is already showing.
9. The step-budget row is not built. The browser projection exposes no remaining-step field, and the unit scope makes the row conditional on one.
10. A failed `open` shows the controller's generic error line in the start state, not the BrowserNavigationError card. The controller's `open` does not classify failures, and changing it belongs to the WS9.F file, which is out of scope.
11. Dismissing a navigation failure is pane-local, by identity. The controller has no clear call.
12. Keep-alive is wired the same way as KnowledgeComposer. BrowserChrome runs the timer only for sessions that advertise `human_browsing`, which Quarry does not project yet, so no calls are made until then.
13. The Mod+Alt+B and Mod+Alt+L chords still act only with a live run. The pinned lines in ChatPage were left unchanged so the Gb pin stays green.
14. The chat header, and so the Browser button, appears only once the chat has messages. A fresh chat cannot open the browser yet (depth row D-WS9H-1).
15. FloatingComposer (the optional second step, which needs a 0.5-day spike) is not built.

## MP asks

None. The pane uses the existing human-session endpoints (`/api/v1/browser/sessions` and `/keepalive`). No Model Plane change is needed, so `MP-ASKS.md` was not appended. The keep-alive is inert until Quarry projects `human_browsing`. That is a dependency noted here, not an ask.

## Not verified

- Nothing compiled or run, per the owner's code-only order. No tsc (baseline 33), no vitest, no Playwright.
- The new tests have not run. The retention listener in `dock-session.test.ts` relies on `flush()` to commit under Solid 2. `InteractiveBrowserPane.test.tsx` stubs `BrowserChrome` and uses the real omnibox.
- Rail layout in bottom and maximised modes is untested. The pane uses `.verevon-chat-run-panel`, so the Ga dock rules apply to it.

## Not done (could not)

- Writing to the lane, so no commits. The lane branch `w23/lane-a` is unchanged.
- The status file and depth rows could not be written into the lane. Drafts are in `ws9h-drafts/`.
- Nothing pushed, no PR, no Docker touched.

## Apply steps (from a session rooted at `innh-lane-a`)

1. Copy the seven new `.ts` and `.tsx` files into `apps/Frontend Plane/verevonv3/src/features/browser/dock/`.
2. Copy `interactive-browser-pane.css` into the same folder and convert it to CRLF.
3. From the repo root, `git apply` `ws9h-chatpanels.patch` and `ws9h-chatpage.patch`.
4. Commit with explicit paths, with the trailer `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`:
   - `feat(browser): chat-per-tab map and dock session hook (W2-FE-WS9H)`
   - `feat(browser): interactive dock pane, open button and ask affordance (W2-FE-WS9H)`
   - `feat(chat): header slot and dock body switch for the interactive browser (W2-FE-WS9H)`
   - `docs(plan-work): W2-FE-WS9H status and lane a depth rows`
5. Compile check at integration: `pnpm typecheck` (baseline 33), then `pnpm exec vitest run src/features/chat` and `pnpm exec vitest run src/features/browser/dock`.
