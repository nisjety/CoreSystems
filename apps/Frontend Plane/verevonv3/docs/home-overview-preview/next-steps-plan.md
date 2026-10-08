# Home dashboard: next steps

2026-09-29. Paths are relative to `apps/Frontend Plane/verevonv3`. The visuals
stayed as they were.

## A. Built (2026-09-29)

| # | What | Where |
| --- | --- | --- |
| A1 | Layout memory: chat dock minimised/expanded, updates feed and news category, Pinned filter. Per browser, keyed by org and user; nothing is read or written until both are known. | `src/features/dashboard/home/home-layout-state.ts` |
| A2 | Unsent draft survives reload and navigation. sessionStorage only, text only, debounced, cleared on send. Never stored when the next message is Temporary chat or zero-retention, or when the workspace denies local copies. | `home-draft.ts`, `src/shared/session/home-draft-storage.ts` |
| A3 | Tab in the URL: `?tab=web` / `?tab=extract`, no parameter is Chat. Back/Forward and links work. `?overview=` still takes precedence. | `src/shared/actions/home-tab.ts`, `DashboardHome.tsx` |
| A4 | Recent-work rows link to their own thread, so Ctrl-click, new tab and copy link open the right conversation. | `chatThreadHref` in `src/features/chat/lib/chat-thread-deep-link.ts` |
| A5 | Double-clicking the active Nett tab resets the search: query, saved snapshot, results and anything in flight. The chat draft and the 4-hour search cache are untouched. | `DashboardHomeChrome.tsx`, `dashboard-search-session.ts` |
| A6 | Per-tab cards. **Hent ut:** last crawls/extractions and scheduled crawls replace recent conversations and weather (`listIngestionRuns`, `listIngestionSchedules`). **Nett:** recent searches and opened pages replace recent conversations; re-run a search or open a page from the card. | `DashboardCrawlRuns.tsx`, `DashboardCrawlSchedules.tsx`, `DashboardWebHistory.tsx`, `web-history.ts` |

Decisions made while building:
- A prompt card ("Spør Verevon") opens the chat dock for that visit only; it does not change what is remembered.
- The Nett history is browser-local (no server-side search history exists). It holds 10 entries, only http/https URLs with credentials stripped, and has a "Tøm" button. The identity-scoped key keeps accounts apart; it is not cleared on sign-out.
- Search snapshots are now scoped per org and user and are cleared on sign-out, user change and a zero-retention verdict.
- Drafts, Nett history and search snapshots are purged when the workspace denies local copies. The verdict is learned from the first chat-thread listing; a probe on Home fetches it when no Home card does.
- The Chat tab is unchanged; the Updates and Tools cards are the same on every tab.

## B. Still planned (not started)
- **Keyboard:** "/" focuses the dock on Home (Ctrl+K stays search); Escape collapses the expanded dock; Chat/Nett/Hent ut become a real tablist with arrow keys. The double-click reset has no keyboard equivalent yet; an Escape-to-clear on the search box was built and removed because shortcuts are plan-only.
- **Live greeting and date:** one `now` signal, updated every minute and on `visibilitychange`.
- **Refresh when you come back:** after 5+ minutes away, refresh weather, the updates feed and recent work while keeping the old data on screen.
- **Don't overwrite drafts:** "Spør Verevon" asks before replacing typed text.
- **Also noticed:** pinned threads older than the top 4 are hidden unless the Pinned toggle is on; the Aquatiq site filter resets when its data reloads; Nett history has no age limit.
