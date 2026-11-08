# S7 frontend-browser-ui

Items: F-t2-011, F-t2-012, F-t2-013, F-t2-014, F-t2-028, F-t2-033, F-t2-038, F-t2-006 (sections in that order).
Tree: live `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted entries). Product paths were tagged with `tools/provenance.sh`; paths that were not run through it say so. Tags: [C] committed and unchanged, [M] committed but modified in live tree, [U] untracked, [NEW] would be created by the plan. "[tag not run]" marks a path cited without a provenance run.
Method: each cited `path:line` was opened in the live tree with a line-window check, and drifted ones were re-read. Absence claims are proven by grep over the live tree (`target/`, `node_modules/`, `.claude/` excluded). Citations to [M] files are live line numbers; the uncommitted hunk that shifts a number is named where it matters.
Not done: docs (`docs/*.md`) were not checked for stale claims beyond the code comments they cite.

## Focus-area provenance (requested)
- [C] `BrowserChrome.tsx` (2172 lines), `ChatLiveRunPanel.tsx` (713), `chat-run-watch.ts` (473), `KnowledgeScrapePreview.tsx` (496), `BrowserTimelineDetail.tsx` (size not measured), `FleetRunConsole.tsx` (166), `DashboardSearchResults.tsx` (693; contains the `WebPageViewer` symbol), `DashboardSearchPanel.tsx` (size not measured), `ChatWideResearchPanel.tsx`, `search-client.ts` (345), `browser-client.ts` (566), `orchestration-client.ts` (324), `quarry-client.ts`, `use-chat-shortcuts.ts` (200), `action-registry.ts` (3151), `web-history.ts` (246), `browser-session.ts` (825), `browser-loop.ts`.
- [M] `browser-omnibox.ts` (15 lines; the only uncommitted change is a 2-line `VITE_DEMO` guard at L12-13), `ChatPage.tsx` (1024), `ChatMessages.tsx` (4060), `DashboardComposer.tsx` (5925), `KnowledgeComposer.tsx` (1999), `SpaceRoomComposer.tsx` (800), `AgentRunConsole.tsx` (2563), `notifications-client.ts` (68), `App.tsx` (250).
- [U] `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatUtilityPage.tsx` (imported by the uncommitted `App.tsx` routes).
- Tests: [C] `browser-omnibox.test.ts`, `browser-session.test.ts`, `browser-loop.test.ts`, `ChatLiveRunPanel.test.tsx`, `chat-run-watch.test.ts`, `FleetRunConsole.test.tsx`, `use-chat-shortcuts.test.ts`, `DashboardComposer.mid-run.test.tsx`, `SpaceApprovalPanel.test.tsx`, `ChatWideResearchPanel.test.tsx`, `DashboardSearchPanel.web-history.test.tsx`; [M] `AgentRunConsole.test.tsx`. Playwright [C] `tests/e2e/browser-live.spec.ts`, `browser-workspace-hitl.spec.ts`, `browser-workspace-live-takeover.spec.ts`, `browser-workspace-zdr.spec.ts`, `chat-workspace.spec.ts`.
- Action registry: `src/shared/actions/action-registry.ts` [C] has `browser_run.start|control` (L2981, 2992), `browser_session.create|close` (3003, 3014), `browser_tab.create|select|close` (3025-3047), `browser_action.run|set_control_mode|suggest` (3058-3080), `browser_profile.*` (3091-3124). No `search.*` action ID was found, so the omnibox search path has no action contract.

## Cross-item overlaps (merge-conflict hotspots in this sweep)
1. `BrowserChrome.tsx` [C]: 011, 012, 013, 014, 028, 033, 038.
2. `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C]: 011, 012, 013, 014, 028, 038.
3. `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs` [C]: 006, 011, 013, 014, 028, 038.
4. `ChatLiveRunPanel.tsx` [C] and `chat-run-watch.ts` [C]: 012, 013, 014, 028, 038.
5. `KnowledgeScrapePreview.tsx` [C] and `browser-session.ts` [C]: 012, 028, 033, 038.
6. `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs` [C] and `browser_events.rs` [C]: 013, 033, 038.
7. `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` [M]: 013 (`permission_mode` doc L288-291) and 038 (L296). `execution-core/src/runtime_loop/agent.rs` [M] is 033 only (in-flight `approval_continuation`).
8. `apps/Application Plane/notification-core/internal/http/handlers.go` [C] and `apps/Model Plane/go/services/capability-core/internal/runwatch/consumer.go` [C]: 033, 038.
9. `AgentRunConsole.tsx` [M] (RunWatchToggle) and `notifications-client.ts` [M]: 033, 038.
10. `quarry-browser/src/chromiumoxide.rs` [C], `FleetRunConsole.tsx` [C], `quarry-runtime/src/step_receipts.rs` [C], `App.tsx` [M]: 006 with 028, 014, 013, 012 respectively.

---

## F-t2-011 Let users browse freely: a human-session policy plus an omnibox that routes free text to Verevon search

### 1. CITATION CHECK (20 checked; the rest of the JSON evidence list duplicates these)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:1143-1215` | [C] | OK | toolbar with forward at L1162 (`runAction({ type: 'forward' })`), takeover controls |
| 2 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:255-300` | [C] | OK | `enum BrowserActionActor` at L255 |
| 3 | `browser.rs:483-490` | [C] | OK | start body L483-488: `max_steps: 12`, `allowed_domains` = first host, `max_runtime_s: 120` |
| 4 | `browser.rs:690-790` | [C] | OK | L705: agent step rejected when `current_mode == HumanTakeover` |
| 5 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:446-451` | [C] | OK | `fn enforce_step_budget` L446 (max_steps only) |
| 6 | `agent_routes.rs:2258-2300` | [C] | OK | `validate_run_grant(..., &entry.constraints.allowed_domains)` at L2258 |
| 7 | `agent_routes.rs:314-339` | [C] | OK | `run_domains: &[String]` parameter at L317 |
| 8 | `agent_routes.rs:1397-1412` | [C] | OK | `BrowserEgressPolicy::canonical_broker_domains(` at L1398 |
| 9 | `agent_routes.rs:1446` | [C] | OK | `let ttl_s = constraints.max_runtime_s.unwrap_or(120);` |
| 10 | `agent_routes.rs:1735-1770` | [C] | OK | `entry.control_mode = body.mode` L1760; `initiated_by: ...Human` L1766 |
| 11 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs:55-77` (card gives a short path) | [C] | OK (body not line-read) | `canonical_broker_domains` L59; doc L56 says empty lists and wildcards are rejected |
| 12 | `quarry-browser/src/persistent_session.rs:55-110` | [C] | OK | L77 doc: registry "implements an idle reaper" |
| 13 | `apps/Model Plane/go/services/browser-broker/internal/server/server.go:22` | [C] | OK | `const defaultGrantTTL = 15 * time.Minute` |
| 14 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/browser-omnibox.ts:14-15` | [M] | OK (live numbers) | L14 returns `https://www.google.com/search?q=...`; committed line is L12 (uncommitted hunk `@@ -11,0 +12,2 @@`) |
| 15 | `BrowserChrome.tsx:846-852` | [C] | OK | `const navigateFromAddress` L846 |
| 16 | `BrowserChrome.tsx:1146` | [C] | OK | toolbar wrapper `knowledge-browser-chrome__toolbar` L1146 |
| 17 | `web-history.ts:1-60` | [C] | OK | exports `webHistoryStorageKey` (L8): a search-home card store, as the card says |
| 18 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs:36-70` | [C] | OK | router L37-45: `/api/v1/search/web` L39, `/search/suggest` L41, `/search/suggestions` L44 |
| 19 | `search.rs:100-140` | [C] | DRIFT | no suggest code in 100-140. Handlers: `search_suggest` L406, `search_suggestions` L522 |
| 20 | `browser-omnibox.ts:1-15` | [M] | OK | `browserOmniboxTarget` export at L12 |

Not line-checked: none beyond duplicates.

### 2. AFFECTED FILES
- `browser-omnibox.ts` [M] (15 lines): `browserOmniboxTarget` (L12) sends free text to google.com (L14). Imported by `BrowserChrome.tsx` [C]; tests `browser-omnibox.test.ts` [C]. Uncommitted change: only the `VITE_DEMO` guard (L13).
- `BrowserChrome.tsx` [C] (2172): `navigateFromAddress` L846; toolbar and back/forward/reload L1146-1215; takeover toggle L497, L624. Hard reload, error-page retry and the suggestion dropdown go here.
- `browser-client.ts` [C] (566): `createBrowserSession` L315, `runBrowserAction` L328, `listBrowserTabs` L379. `suggestBrowserAction` L457 is the AI browser-action suggestion, not omnibox suggestions.
- `search-client.ts` [C] (345): the only SPA consumer of `/search/suggest` and `/search/suggestions` (grep; dashboard search). The omnibox does not import it.
- `web-history.ts` [C] (246): card store; consumers are dashboard tests and `DashboardSearchPanel.tsx` (not omnibox history).
- gateway `browser.rs` [C] (6317): create-session body L483-488; control route and takeover checks L690-790; sessions route L371; `start_ai_run` L1263.
- gateway `search.rs` [C] (1078): routes L39-45; handlers `search_suggest` L406, `search_suggestions` L522.
- Quarry `agent_routes.rs` [C] (3737): `enforce_step_budget` L446; `domain_allowed` helper L442 (callers not traced); `validate_run_grant` call L2258; `canonical_broker_domains` call L1398; `ttl_s` L1446, L1502; `set_browser_session_control` L1733-1785.
- Quarry `quarry-browser/src/lib.rs` [C] (676): `BrowserEgressPolicy` L33-59, `canonical_broker_domains` L59.
- Quarry `persistent_session.rs` [C] (239): `PersistentSessionRegistry` idle reaper (see section 3).
- Model Plane `browser-broker/internal/server/server.go` [C] (179; `defaultGrantTTL` L22) and `internal/grant/grant.go` [C] (362; `AllowedDomains` L45; in-memory `grants` map L64, L69).
- Model Plane `execution-core/src/browser_agent/browser_risk.rs` [C] (1288; `RiskCategory` L25). The CrossDomainNavigation gate named by the card was not line-verified.
- Migrations: none for the frontend slice. If backend human-session state is persisted, the next free Quarry migration is `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/migrations/0010_*.sql` ([C] last file is `0009_browser_timeline_events.sql`).

### 3. REUSABLE BUT UNWIRED
- `PersistentSessionRegistry` idle reaper: `quarry-browser/src/persistent_session.rs` [C] L55-110. Proof: `grep -rln PersistentSessionRegistry crates` returns only `persistent_session.rs` and `quarry-browser/src/lib.rs` (re-export). No caller in quarry-edge or quarry-runtime. Built, not wired.
- Gateway suggest routes (`search.rs` [C] L41, L44): only SPA consumer is `search-client.ts`, so the suggest routes are wired to dashboard search only. Not wired to the omnibox.
- `BrowserEgressPolicy` SSRF blocks (`quarry-browser/src/lib.rs` [C] L33-59): wired (call at `agent_routes.rs:1398`); test `quarry-browser/tests/ssrf.rs` [C].
- Name collision: `browser_action.suggest` (`action-registry.ts` [C] L3080) is an AI action suggestion. Do not reuse it as the omnibox endpoint.

### 4. TESTS
- Vitest, run by CI through `pnpm verify` -> `pnpm test` (`vitest run`): `src/features/dashboard/home/browser-omnibox.test.ts` [C], `browser-session.test.ts` [C], `web-history.test.ts` [tag not run], `DashboardHome.web-history.test.tsx` [tag not run], `dashboard-search-session.test.ts` [tag not run].
- Playwright (run by no CI workflow, see section 6): `tests/e2e/browser-workspace-session.spec.ts` [tag not run], `browser-live.spec.ts` [C].
- Quarry: inline tests in `agent_routes.rs` `allowed_domains_accept_exact_and_subdomains_only` (L2051) and `step_budget_is_fail_closed_once_limit_is_reached` (L2063); `quarry-browser/tests/ssrf.rs` [C].
- Go: `browser-broker/internal/server/server_test.go` and `ratelimit_test.go` [tag not run]; `internal/grant/grant_test.go` [C].
- Commands:
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/dashboard/home/browser-omnibox.test.ts src/features/dashboard/home/browser-session.test.ts`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge allowed_domains_accept` and `cargo test -p quarry-edge step_budget_is_fail_closed`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-browser --test ssrf`
  - `cd "apps/Model Plane/go/services/browser-broker" && go test ./internal/server/... ./internal/grant/...`
- CI: `.github/workflows/verevonv3-ci.yml` [M] L193 (`pnpm verify`); `.github/workflows/quarry-v2-ci.yml` [C] L205 (`cargo test --workspace`) and L211 (`cargo test -p quarry-browser --features chromiumoxide`). No workflow references `browser-broker` (grep).

### 5. FEATURE FLAGS / ENV
- `import.meta.env.VITE_DEMO === '1'` returns null from `browserOmniboxTarget` (`browser-omnibox.ts` [M] L13, uncommitted).
- Session policy is hard-coded, not flagged: `max_steps` 12 and `max_runtime_s` 120 (`browser.rs` L485-487); Quarry lease TTL default 120 s (`agent_routes.rs` L1446); broker grant TTL 15 min (`server.go` L22); broker host list max 32 (`quarry-browser/src/lib.rs`, see L59-77).
- `ENVIRONMENT` / `APP_ENV` = `prod|production` forces `require_browser_grants` (`quarry-edge/src/main.rs` [M] L157-158, L183-184). In production every browser lease is governed, so a human-browse policy must still come from the broker.
- The gateway reads no env vars in `browser.rs` (grep). Its config lives on AppState.

### 6. SURPRISES
- The live omnibox still sends free text to google.com (`browser-omnibox.ts` L14). The uncommitted diff does not change that.
- The card's `browser-omnibox.ts:14-15` is a live number. In committed afb83928 the line is 12-13. Use the live number after the VITE_DEMO hunk lands.
- Suggest endpoints exist, but the omnibox has no consumer. The card's plan for suggestions is wiring work, not a backend gap.
- CLAUDE.md says Verevon v3 action contracts come before UI operations. Browser operations have action IDs (`browser_*`, `action-registry.ts` L2981-3124), but no `search.*` action ID exists, so the omnibox search path would need a new contract.
- Test runs: the browser-workspace Playwright specs are not executed by any CI workflow (`grep "playwright test|test:e2e|tests/e2e"` over `.github/workflows` finds nothing). Only the Chromium install and a demo probe run in CI.

---

## F-t2-012 Chat beside the browser: a dockable browser panel bound to the chat run

### 1. CITATION CHECK (17 checked; not line-checked: `KnowledgeScrapePreview.tsx:20`, `ChatLiveRunPanel.tsx:39`, the JSON-only `ChatLiveRunPanel.tsx:1-14` duplicate, which was checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx:957-984` | [M] | OK | `<ChatLiveRunPanel` at L973 (none of the 7 uncommitted hunks touch the mount) |
| 2 | `ChatLiveRunPanel.tsx:608` | [C] | OK | `{(model) => <BrowserChrome compact session={model()} />}` |
| 3 | `ChatLiveRunPanel.tsx:451` | [C] | OK | `await controlBrowserAiRun(orgId, currentRunId, action)` inside `browserControl` L441-456 |
| 4 | `ChatLiveRunPanel.tsx:1-14` | [C] | OK | header describes the chat split-view panel and its compact BrowserChrome mount |
| 5 | `apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-run-watch.ts:45` | [C] | OK | `CHAT_RUN_PANEL_COLLAPSED_KEY = 'verevon.chat.runPanel.collapsed.v1'` (the only persisted panel state) |
| 6 | `chat-run-watch.ts:402-452` | [C] | OK | `browserSessionFromChatRun` at L410; comment L402-404: live-frame transports left null |
| 7 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeScrapePreview.tsx:253` | [C] | OK | `<BrowserChrome` at L253 |
| 8 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:620-626` | [C] | OK | L624 `nextMode = humanTakeoverActive() ? 'agent_control' : 'human_takeover'` |
| 9 | `BrowserChrome.tsx:497` | [C] | OK | `humanTakeoverActive = () => session().controlMode === 'human_takeover'` |
| 10 | `BrowserChrome.tsx:402` | [C] | OK | `export function BrowserChrome(props` at L402 |
| 11 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:371-444` | [C] | OK | `.route("/api/v1/browser/sessions", post(create_session))` at L371 (route table) |
| 12 | `browser.rs:1212-1237` | [C] | OK | comment L1219 on `profile_id`; literal `grant_id` `session:{id}` at L1235 |
| 13 | `browser.rs:1258-1262` | [C] | OK (as doc) | doc comment states `grant_id` is interim and that real BrowserGrant issuance is unbuilt (the literal is at L1235) |
| 14 | `browser.rs:1311-1318` | [C] | OK | `let start_url = projection` at L1311 |
| 15 | `browser.rs:3310-3347` | [C] | DRIFT (minor) | `async fn owned_orchestration_run(` starts at L3347; the body runs past 3347 |
| 16 | `apps/Frontend Plane/verevonv3/src/app/App.tsx:172` | [M] | DRIFT (minor) | L172 is the `path: '/'` group; `/chat` -> `ChatPage` is at L177 |
| 17 | `browser.rs:1235-1245` (JSON) | [C] | OK | same literal at L1235 |

### 2. AFFECTED FILES
- `ChatLiveRunPanel.tsx` [C] (713): mount of compact `BrowserChrome` L608; `browserControl` L441-456; `onRunPaused`/`onRunResumed` L349-366. The dock wraps this component.
- `ChatPage.tsx` [M] (1024): `<ChatLiveRunPanel` L973; split layout around `workCanvasActive`; uncommitted hunks at L145, 231, 239, 789-801 (no RunPanel or browser tokens in added lines).
- `ChatWorkspaceCanvas.tsx` [C] (size not measured): dock host candidate named by the card. Not verified further.
- `ChatWideResearchPanel.tsx` [C]: mounted inside the `ChatLiveRunPanel` rail (its own doc at L6 and L11).
- `chat-run-watch.ts` [C] (473): collapse key L45; `browserSessionFromChatRun` L410; `commentAnchors: []` L424.
- `KnowledgeScrapePreview.tsx` [C] (496): interactive mount L253 (Crawl composer; the only interactive mount).
- `BrowserChrome.tsx` [C] (2172): `BrowserChrome` L402; takeover L497, L624; control chip L1228.
- `browser-client.ts` [C] (566): `createBrowserSession` L315, `runBrowserAction` L328, `setBrowserControlMode` L343, `getBrowserOwnerTimeline` L359, `listBrowserTabs` L379.
- gateway `browser.rs` [C] (6317): frame, control, timeline and artifact routes L371-444; `start_ai_run` L1263; `owned_orchestration_run` L3347.
- `App.tsx` [M] (250): `/chat` route L177 and chat utility routes L178-180 (uncommitted; L178-180 import the untracked `ChatUtilityPage.tsx` [U]).
- Migrations: none. Dock state is localStorage only today. A server-side dock store would be new work.
- [NEW] dock component (no existing file).

### 3. REUSABLE BUT UNWIRED
- Compact `BrowserChrome` mount in chat: wired (L608).
- Live-frame stream type `LiveFrameStreamPayload` in `BrowserChrome.tsx` L201: wired to the frame routes (consumption path not traced end to end).
- Real grant issuance: `AcquireGrant` has no production caller. `grep` over `apps/Model Plane/go/services/browser-broker`, `execution-core/src` and gateway `src` finds only authz, its tests and the latency test. The gateway comment at L1258-1262 agrees with this, so code and comment are consistent.

### 4. TESTS
- Vitest (CI via `pnpm verify`): `src/features/chat/components/ChatLiveRunPanel.test.tsx` [C], `src/features/chat/lib/chat-run-watch.test.ts` [C], `ChatWideResearchPanel.test.tsx` [C], `src/features/chat/lib/use-chat-shortcuts.test.ts` [C].
- Playwright (no CI): `tests/e2e/chat-workspace.spec.ts` [C], `tests/e2e/browser-live.spec.ts` [C].
- Command: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/components/ChatLiveRunPanel.test.tsx src/features/chat/lib/chat-run-watch.test.ts src/features/chat/components/ChatWideResearchPanel.test.tsx`
- CI: `verevonv3-ci.yml` [M] L193.

### 5. FEATURE FLAGS / ENV
- `verevon.chat.runPanel.collapsed.v1` (localStorage, `chat-run-watch.ts` L45) is the only persisted panel state. It is per-browser, not per-user.
- No dock flags. `grep import.meta.env` over ChatPage, ChatLiveRunPanel, BrowserChrome, FleetRunConsole and browser-omnibox finds only `VITE_DEMO` in browser-omnibox.
- ZDR: chat runs have no frames (comment at `chat-run-watch.ts` L402-404), so the panel shows the ZDR copy and dock state must not go server-side.

### 6. SURPRISES
- `ChatWideResearchPanel` is a second child of `ChatLiveRunPanel`, not mentioned by the card. A dock change must keep it.
- `App.tsx` [M] adds chat utility routes that import the untracked `ChatUtilityPage.tsx` [U] in the same route group as `/chat`. The dock work will touch `App.tsx` and `ChatPage.tsx` (both [M]).
- The pause/resume control exists (`ChatLiveRunPanel.tsx` L441-456), so the chat view is not purely read-only. Only the manual frame controls are disabled, because the chat session has no live URLs.
- The audit's `browser.rs:1258-1262` is a doc comment that states the interim-grant truth. The literal is at L1235.

---

## F-t2-013 Co-browsing: the agent works in the user's visible session in agent-flagged tabs, with takeover enforced inside Quarry

### 1. CITATION CHECK (19 checked; not line-checked: JSON-only `browser.rs:663-732`, `761-796`, `1190-1236` (overlapping)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:55-90` | [C] | OK | `pub control_mode: BrowserControlMode` L70 |
| 2 | `step_receipts.rs:67-70` | [C] | OK | L68 doc: a gateway restart cannot reset a human takeover |
| 3 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:1733-1785` | [C] | OK | `set_browser_session_control` L1733; `initiated_by: ...Human` L1766 |
| 4 | `agent_routes.rs:2227-2294` | [C] | OK | `pub async fn step(` L2228; no `control_mode` token in 2228-2310 |
| 5 | `agent_routes.rs:474-482` | [C] | OK | `fn caller_owns_browser_run` L474-482 (org equal and `actor_id()` equal) |
| 6 | `agent_routes.rs:493-495` | [C] | OK | `if entry.zdr.is_active() { return Ok(()) }` L493 in the timeline-write helper |
| 7 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:696-723` | [C] | OK | `.get("controlMode")` L702 (body field) |
| 8 | `browser.rs:2065-2075` | [C] | OK | `let actor = value` L2065 (caller-supplied actor) |
| 9 | `browser.rs:1222-1236` | [C] | OK | HITL and grant_id comment L1225; literal `session:{id}` L1235 |
| 10 | `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:674` | [C] | OK | `.start_run(` L674 |
| 11 | `browser_agent.rs:669-690` | [C] | OK | L670 comment: acquire a leased session, `profile_id` threads |
| 12 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs:60-139` | [C] | DRIFT (minor) | `is_service` L101 (inside); `actor_id` L144 (outside); use 60-146 |
| 13 | `apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:44-130` | [C] | OK | `SessionKey` L43, `ParentGrantID` L54-56, `CreateScopedSensitiveApproval` L102-104 |
| 14 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:1225-1245` | [C] | OK | chip class `knowledge-browser-control-chip--human` L1228 |
| 15 | `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatLiveRunPanel.tsx:598-612` | [C] | OK | `<BrowserChrome compact session={model()} />` L608 |
| 16 | `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs:556` | [C] | OK | `.token(org_id, AGENT_SCOPES)` L556 |
| 17 | `quarry_agent.rs:34` | [C] | OK | `const AGENT_SCOPES: &[&str] = &["browser:execute"];` |
| 18 | `BrowserChrome.tsx:497` and `:1230` (JSON) | [C] | OK | takeover predicate L497 |
| 19 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs` `Claims` on-behalf-of absence (JSON `auth.rs:62-91`) | [C] | [UNVERIFIED] | no grep run for `on_behalf|onBehalf|act_as` |

Confirmed claim: no `control_mode` read in the step handler. Reads are at L92, 470, 564, 629, 655, 1604-1607, 1760 (write), 2115 (init) in `agent_routes.rs`.

### 2. AFFECTED FILES
- `step_receipts.rs` [C] (818): `BrowserControlMode` persisted L55-90; `StepReceipt.actor_id` L192 (owner, not per-step initiator).
- `agent_routes.rs` [C] (3737): `set_browser_session_control` L1733-1785; `step()` L2228 (add the enforcement near the budget check at L2294); `caller_owns_browser_run` L474 with callers at L485, L1335, L1715, L1824; timeline helper L490-495 (ZDR skip).
- `quarry-edge/src/auth.rs` [C] (1242): `is_service` L101 (used at L508, L637); `actor_id` L144.
- gateway `browser.rs` [C] (6317): `controlMode` L702; actor L2065; `start_ai_run` grant literal L1235.
- `browser_agent.rs` [C] (1709): `start_run` L674; lease acquisition L669-690.
- `quarry_agent.rs` [C] (963): `AGENT_SCOPES` L34; token L556.
- `grant.go` [C] (362): `SessionKey` L43; `ParentGrantID` L54-56; `CreateScopedSensitiveApproval` L104 (caller `server.go` L85).
- SPA: `BrowserChrome.tsx` [C] (2172) chip L1228; `ChatLiveRunPanel.tsx` [C] (713) mount L608.
- Migrations: `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/migrations/0010_*.sql` (next free) if receipts gain an `initiator` column. Existing: `0008_step_receipt_actor_scope.sql` [C], `0009_browser_timeline_events.sql` [C].
- Proto: none (Quarry-internal). Gateway and SPA `browser-client.ts` [C] types change if the response changes.

### 3. REUSABLE BUT UNWIRED
- Control epoch: none. Proof: `grep -rn "control_epoch|controlEpoch"` over quarry, gateway, SPA and execution-core returns nothing ([NEW]).
- `attach_session_id`, `markTab`, `run.expired`: none (same grep).
- `CreateScopedSensitiveApproval`: wired (`server.go` L85); it is the reusable child-grant helper for attach.
- `AcquireGrant`: no production caller (see F-t2-012 section 3). The interim `session:{id}` grant is what production uses.
- `caller_owns_browser_run`: wired; blocks attach because the Model Plane run is a service principal (see constraints).

### 4. TESTS
- Quarry inline: test module in `agent_routes.rs` asserts `caller_owns_browser_run` near L2044-2047 (test fn name not traced).
- `quarry-edge/tests/zdr_behavior.rs` [C] (ZDR timeline skip).
- execution-core `tests/browser_agent_e2e.rs` [C].
- SPA: `browser-session.test.ts` [C]; `ChatLiveRunPanel.test.tsx` [C]; Playwright `browser-workspace-live-takeover.spec.ts` [C] and `browser-workspace-hitl.spec.ts` [C] (no CI).
- Commands:
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge zdr`
  - `cd "apps/Model Plane/rust" && cargo test -p execution-core --test browser_agent_e2e`
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec playwright test tests/e2e/browser-workspace-live-takeover.spec.ts` (local only)
- CI: `quarry-v2-ci.yml` [C] L205; `model-plane-rust-ci.yml` [M] L119 (`cargo test --workspace --no-fail-fast`).

### 5. FEATURE FLAGS / ENV
- `control_mode` default `AgentControl` at `agent_routes.rs` L2115; takeover body field `mode` L1760; gateway values `human_takeover` and `agent_control` (`browser.rs` L702; `BrowserChrome.tsx` L497, L624).
- ZDR: `entry.zdr.is_active()` skips control timeline writes (L493); `zdr` is trusted only for non-service claims (`auth.rs` L122).
- Browser runs use `permission_mode: "auto"` (`model-gateway/src/browser_run.rs` [M] doc L288-291), so HITL does not pause before loop gates.
- `QUARRY_EDGE__REQUIRE_DURABLE_RUNTIME` (`main.rs` L180) and prod environment force durable runtime; checkpoints of control_mode survive restart.

### 6. SURPRISES
- Confirmed by grep: the step handler never reads `control_mode`. Takeover enforcement exists only in the gateway (`browser.rs` L702, L2065, caller-supplied). The gap is real.
- `set_browser_session_control` hard-codes `initiated_by: Human` (`agent_routes.rs` L1766). Agent-initiated control changes cannot be recorded through this route, so an agent-side control change would need a new path [INF].
- `StepReceipt.actor_id` exists (L192) and migration `0008_step_receipt_actor_scope.sql` is already in place. The card's "receipts carry only the run owner" is correct, but the actor scope is partly there.
- `model-gateway/src/http_routes.rs` [M] (11681 lines, 51 hunks) changes queued-input comments near the queue route, not the Quarry takeover path.
- The card's reuse of `Claims::is_service` is correct; `actor_id` sits outside the cited range (L144).

---

## F-t2-014 Surface one canonical agent run trace in the UI: typed events, receipts, live view, correlation envelope

### 1. CITATION CHECK (22 checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Model Plane/proto/model_plane/v1/events.proto:8-60` | [C] | OK | `message Event {` L12 |
| 2 | `events.proto:112-130` | [C] | OK | `EVENT_TYPE_RUN_STARTED = 90;` L119 |
| 3 | `apps/Model Plane/proto/model_plane/v1/eventlog.proto:26-37` | [C] | OK | append/query/subscribe doc L27 |
| 4 | `apps/Model Plane/proto/model_plane/v1/orchestration.proto:990-1040` | [C] | OK | `message StreamRunEventsRequest {` L995 |
| 5 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:368-470` | [C] | OK | route table from L371; no egress-receipts, procedure, agent-events or live-view route (grep) |
| 6 | `browser.rs:625-648` | [C] | OK | `async fn get_owner_timeline(` L625 |
| 7 | `browser.rs:1819` | [C] | OK | devtools path `/v1/agent/runs/{}/devtools{}` |
| 8 | `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatLiveRunPanel.tsx:360-380` | [C] | OK | `onRunResumed: (event) => {` L360 |
| 9 | `ChatLiveRunPanel.tsx:371` | [C] | OK | `onBrowserActionApprovalRequired` L371 |
| 10 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserTimelineDetail.tsx:1-50` | [C] | OK | imports `artifactsForTimelineEntry` L6 |
| 11 | `apps/Frontend Plane/verevonv3/src/features/agents/components/FleetRunConsole.tsx:1-60` | [C] | OK | header L9 cites `GET /v1/fleets/:id`; `fetchFleet` L27-28 |
| 12 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:199` | [C] | OK | `type DevtoolsTab = 'console' | 'dom' | 'network' | 'vision'` L199 |
| 13 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:1055-1072` | [C] | OK | `pub struct DevtoolsQuery {` L1061 (after_sequence, limit) |
| 14 | `agent_routes.rs:930-960` | [C] | OK | `from_seq` doc L933; `pub from_seq: Option<u64>` L939 |
| 15 | `agent_routes.rs:2650-2706` | [C] | OK | reconnect doc L2655; `let _ = &query.from_seq;` L2705 (no-op replay) |
| 16 | `agent_routes.rs:3660-3730` | [C] | OK | router starts `.route("/v1/agent/runs", post(start_run))` L3661 |
| 17 | `agent_routes.rs:1739` | [C] | DRIFT | the initiator literal is at L1766 (function 1733-1785) |
| 18 | `apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/httpx/httpx.go:26-30` | [C] | OK | `X-Request-Id` read L26; `req_<uuid>` default L28; echoed L30; wired by `r.Use(httpx.RequestID, ...)` in `cmd/control/main.go` L144 |
| 19 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:100-125` | [C] | OK | L109 doc: timeline combines owner streams |
| 20 | `step_receipts.rs:190-206` | [C] | OK | `pub actor_id: Option<String>,` L192 |
| 21 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/ingest_client.rs:180` | [C] | OK | reads an upstream `x-request-id` response header (not an intake) |
| 22 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx:1053` | [M] | OK | inside the `onBrowserAction` handler L1052-1058 |

### 2. AFFECTED FILES
- Model Plane protos: `events.proto` [C] (185; `Event` L12-60; run types L112-130); `eventlog.proto` [C] (116); `orchestration.proto` [C] (1289; `StreamRunEventsRequest` L995).
- CI gates on proto and SSE changes: `.github/workflows/buf.yml` [C] (`buf lint apps/Model Plane/proto` and `buf breaking` against `proto/baseline.binpb`); `.github/workflows/sse-taxonomy.yml` [C] (gateway ChatEvent surface vs SPA stream client vs taxonomy doc).
- execution-core: `browser_events.rs` [C] (857; `action_dispatched` L163; publisher side).
- Quarry: `agent_routes.rs` [C] (3737; `DevtoolsQuery` L1061; `AgentEventsQuery` L930-939; `agent_events_stream` L2650-2706; `live_frame_stream` L2791; router L3660-3730; `RequestKind::new()` used 22 times in this file); `step_receipts.rs` [C] (818); `quarry-runtime/src/observation.rs` [C] (3214; `ActionStarted` payload L386-396, `emit_for_zdr` L388-395); `quarry-core/src/contracts.rs` [C] (973; action contracts L360-460).
- Gateway: `browser.rs` [C] (6317; router L368-470; owner timeline L625; devtools L1819); `chat/streams.rs` [C] (273; `Last-Event-ID` L192).
- SPA: `ChatLiveRunPanel.tsx` [C] (713); `chat-run-watch.ts` [C] (473); `BrowserTimelineDetail.tsx` [C] (size not measured); `FleetRunConsole.tsx` [C] (166); `BrowserChrome.tsx` [C] (2172); `browser-client.ts` [C] (566; `getBrowserDevtoolsEvents` L437); `KnowledgeComposer.tsx` [M] (1999; `onBrowserAction` L1052-1058); `quarry-client.ts` [C] (`listRunEvents` wrapper L120-125).
- Migrations: `quarry-runtime/migrations/0010_*.sql` (next free) if Quarry trace or egress receipts persist. No Model Plane migration path was traced ([UNVERIFIED]).
- Quarry-control request-id middleware: `services/quarry-control/internal/httpx/httpx.go` [C] (wired in `cmd/control/main.go` [tag not run]).

### 3. REUSABLE BUT UNWIRED
- Agent SSE `from_seq` replay: accepted but a no-op (`agent_routes.rs` L2705). Unwired.
- Quarry egress-receipts, procedure, agent-events and live-view routes: registered in the Quarry router (L3660-3730) but absent from the gateway (grep `egress|procedure|agent/events|live-view` over `apps/Frontend Plane/verevonv3/apps/gateway/src` is empty) and from the SPA (grep `egress-receipts|egressReceipts|/procedure` over `src` finds no route use).
- `DevtoolsQuery` has only `after_sequence` and `limit` (L1061-1072). No level or filter.
- Quarry edge request-id intake: none. `grep -rni x-request-id crates/quarry-edge/src` and gateway `src` returns nothing. The Go control plane already has `httpx.RequestID`, wired (L144 of `cmd/control/main.go`). That is the pattern to copy.
- `EventLog` query and subscribe: callers not traced ([UNVERIFIED]).

### 4. TESTS
- Quarry: `crates/quarry-edge/tests/sse_stream.rs` [C] (SSE stream test; agent events not confirmed).
- SPA: `ChatLiveRunPanel.test.tsx` [C]; `chat-run-watch.test.ts` [C]; `FleetRunConsole.test.tsx` [C].
- Commands:
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test sse_stream`
  - `buf lint "apps/Model Plane/proto"` (CI `buf.yml`)
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/lib/chat-run-watch.test.ts`
- CI: `buf.yml` [C]; `quarry-v2-ci.yml` [C] L205; `sse-taxonomy.yml` [C] if the trace reaches the chat stream; `verevonv3-ci.yml` [M] L193.

### 5. FEATURE FLAGS / ENV
- None for tracing. `ENVIRONMENT` / `APP_ENV` (`quarry-edge/src/main.rs` [M] L128-158) controls dev auth bypass and durable mode only.
- ZDR-aware emit exists: `events.emit_for_zdr(request.zdr, ...)` in `observation.rs` L388-395, usable for trace persistence gating.
- `from_seq` replay is off by behaviour (no-op), not by flag.

### 6. SURPRISES
- `RequestKind::new()` is called 22 times in `agent_routes.rs`, so every handler mints its own request id.
- The gateway has no request-id handling at all, while quarry-control does (wired). The two Go and Rust services disagree.
- `quarry-control/.../014_fleets.sql` says "Production wires this table", but no Go code outside the migration references fleets (grep).
- `FleetRunConsole.tsx` is a fleet console, not a single-run trace (the card agrees). The "live view" tile at L131 is text, not a stream.
- `KnowledgeComposer.tsx` [M] consumes browser events from the run stream in an uncommitted-modified file.
- The gateway proxies timeline, devtools, frames, control and artifacts only. Receipts, egress-receipts, procedure and agent-events routes are not proxied (grep confirmed).

---

## F-t2-028 Page context to chat: Quarry hit-test, @tab mentions, selection and region attachments, frame context menu

### 1. CITATION CHECK (20 checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:3873-3880` | [M] | OK | L3876: "`@` mentions are intentionally not handled in the global composer" (live line; committed 3873, shifted +3 by hunk `@@ -3430,0 +3435,3 @@`) |
| 2 | `apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx:20-50` | [M] | OK | mention rule doc L34-35 ("A mention never grants"); `MentionCandidate` L69; picker state L148-149 (L20 match was an import alias) |
| 3 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4399` | [C] | OK | `inject_mentioned_space_agent_persona(` L4399 |
| 4 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/browser-session.ts:127-137` | [C] | OK | L128 `capabilities ?? ['annotate', 'dom_select', 'knowledge_ingest']` |
| 5 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:811` | [C] | OK | `runAction({ type: 'click_point', x: point.x, y: point.y })` |
| 6 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeScrapePreview.tsx:190-270` | [C] | OK | `props.onToggle()` L190 (region selection) |
| 7 | `apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-run-watch.ts:1-40` | [C] | DRIFT | L1-40 is header; the adapter `browserSessionFromChatRun` is at L410 (range 402-452) |
| 8 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:365-395` | [C] | OK | `pub(crate) fn router(state: AppState)` L369 |
| 9 | `browser.rs:2578` | [C] | OK | capability list containing `annotate` |
| 10 | `browser.rs:2700` | [C] | OK | same list |
| 11 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/kernel.rs:651-660` | [C] | OK | `async fn click_point(` L651; `document.elementFromPoint(x, y)` L656 in injected click JS |
| 12 | `kernel.rs:729` | [C] | DRIFT (partial) | `document.elementFromPoint` at L729 is in injected **wheel** JS (`mouse_wheel`), so "only inside the click JS" is wrong |
| 13 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1165-1180` | [C] | OK | `node.get("backendDOMNodeId")` L1172 |
| 14 | `chromiumoxide.rs:2841-2870` | [C] | OK | `async fn click_point(` L2841 (raw mouse dispatch) |
| 15 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs:216-221` | [C] | FALSE for generation | `lib.rs` has no `generation` token; L216-221 unrelated. Generation lives in `quarry-core/src/contracts.rs` L339, L369-371, L652 |
| 16 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs:360-460` | [C] | OK | L370-371 `generation` (ref names its source generation) |
| 17 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:484-486` | [C] | OK | `fn caller_owns_live_run(entry, claims)` L484 |
| 18 | `apps/Model Plane/rust/services/execution-core/src/provenance.rs:30-70` | [C] | OK | doc L32 refers to `model-gateway::moderation::TrustClass` (second copy) |
| 19 | `apps/Model Plane/rust/services/model-gateway/src/moderation.rs:195-240` | [C] | OK | `pub enum TrustClass {` L206 |
| 20 | `apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts:384-428` | [C] | OK | `requestJson<BrowserTabsResponse>(\`/api/v1/browser/sessions/${sessionId}/tabs\`` L384 |

Quarry has no hit-test route and no bbox or rect in `contracts.rs` or `chromiumoxide.rs` (grep for `hit_test|hitTest|getNodeForLocation|bbox|box_model|rect` returns nothing relevant).

### 2. AFFECTED FILES
- `DashboardComposer.tsx` [M] (5925): global composer, no `@` picker (L3876). Attachments pipeline exists (see section 3).
- `SpaceRoomComposer.tsx` [M] (800): the mention pattern to copy (L34-35, L69-84).
- gateway `spaces.rs` [C] (9882): server-side mention resolver `inject_mentioned_space_agent_persona` (L4399).
- `browser-session.ts` [C] (825): capability defaults L128; `commentAnchors` type L54, default L129-135.
- `BrowserChrome.tsx` [C] (2172): frame click translation L811; select mode and context menu would live here.
- `KnowledgeScrapePreview.tsx` [C] (496): region selection model (`onToggle` L190); `commentAnchors` renderer L310.
- `chat-run-watch.ts` [C] (473): `browserSessionFromChatRun` L410.
- gateway `browser.rs` [C] (6317): browser router L369; capability lists L2578, L2700; tabs routes.
- `browser-client.ts` [C] (566): `listBrowserTabs` L379; tabs request L384.
- Quarry `kernel.rs` [C] (1216): `click_point` L651; injected JS `elementFromPoint` L656 and L729.
- Quarry `chromiumoxide.rs` [C] (3551): `backendDOMNodeId` L1172; `click_point` L2841.
- Quarry `quarry-core/src/contracts.rs` [C] (973): snapshot `generation` fields L339, L369-371, L652.
- Quarry `agent_routes.rs` [C] (3737): `caller_owns_live_run` L484.
- Model Plane `provenance.rs` [C] (459) and `moderation.rs` [C] (1265; `TrustClass` L206); `tool_loop.rs` [M] and `security_events.rs` [C] also reference `BrowserScraped` (grep).
- Migrations: none (attachments stay ephemeral under ZDR).
- [NEW] hit-test handler (Quarry), mention resolver (gateway), region attach (SPA).

### 3. REUSABLE BUT UNWIRED
- `commentAnchors` renderer: wired, but fed by a hard-coded stub (`browser-session.ts` L129-135) and rendered in two places (`BrowserChrome.tsx` L1844; `KnowledgeScrapePreview.tsx` L310).
- `dom_select` capability is advertised (`browser-session.ts` L128). No consumer found in this sweep ([UNVERIFIED]).
- Attachment pipeline: wired (`DashboardComposer.paste-attachment.test.tsx` [tag not run] exists). Reuse for page attachments; the card did not mention it.
- `spaces.rs` mention resolver: wired (L4399).
- `BrowserScraped` classification: name-based (card; `moderation.rs` L204-240 not fully read). Page attachments need a new classifier path.

### 4. TESTS
- SPA: `DashboardComposer.mid-run.test.tsx` [C]; `browser-session.test.ts` [C]; `SpaceRoomComposer.test.tsx` [tag not run]; `DashboardComposer.paste-attachment.test.tsx` [tag not run]. No test for `@tab`.
- Commands: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/spaces/components/SpaceRoomComposer.test.tsx src/features/dashboard/home/DashboardComposer.paste-attachment.test.tsx`
- CI: `verevonv3-ci.yml` [M] L193 (vitest); `quarry-v2-ci.yml` [C] L205; `model-plane-rust-ci.yml` [M] L119.

### 5. FEATURE FLAGS / ENV
- None found for these components. `VITE_DEMO` is in `browser-omnibox.ts` only.
- ZDR: attachments and crops stay ephemeral (card). The ZDR-gated timeline pattern at `agent_routes.rs` L493 is the reference.

### 6. SURPRISES
- The card's DashboardComposer citation is correct in the live tree but shifted by +3 lines from the uncommitted hunk; re-anchor after that hunk.
- Quarry's snapshot `generation` is in `quarry-core/src/contracts.rs`, not `quarry-browser/src/lib.rs:216-221` (wrong citation).
- `elementFromPoint` is used in two injected scripts (click and wheel), not one.
- `commentAnchors` is rendered by KnowledgeScrapePreview too (card names only BrowserChrome).
- `TrustClass` is a pure function of tool names (card; enum at `moderation.rs` L206 confirmed). Provenance for page attachments needs a new path.

---

## F-t2-033 Approval loop UX: keyboard-first approval card with redirect, persisted site rules and attention notifications

### 1. CITATION CHECK (20 checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:159-164` | [C] | OK | risk labels; `checkout: 'Utsjekk/betaling'` L160 |
| 2 | `BrowserChrome.tsx:322-395` | [C] | OK | `function BrowserApprovalCard(props` L322; no `onKeyDown` in the card (the only keydown handlers are a popover at L1057-1060) |
| 3 | `BrowserChrome.tsx:2039-2056` | [C] | OK | `<Show when={(props.browserApprovals ?? []).length > 0}>` L2039 |
| 4 | `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:220-240` | [C] | OK | doc L226: durable `Approval` record reused |
| 5 | `browser_agent.rs:870-945` | [C] | OK | `ApprovalOutcome::Denied(reason) =>` L883 (denial aborts the run) |
| 6 | `apps/Model Plane/rust/services/execution-core/src/browser_events.rs:129-151` | [C] | OK | `orchestration_event::Event::BrowserActionDecided(` L136 |
| 7 | `browser_events.rs:328-432` | [C] | OK | L335 comment: interrupt a run pending approval |
| 8 | `apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4453-4515` | [M] | OK (live) | `pb::ApprovalKind::Permission` L4455 (generic kind; no risk category) |
| 9 | `browser_agent/browser_risk.rs:1-60` | [C] | OK | `pub enum RiskCategory {` L25 |
| 10 | `apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:39-64` | [C] | OK | L45 `AllowedDomains` doc (broker-owned canonical policy); in-memory store L64/L69 |
| 11 | `apps/Model Plane/go/services/capability-core/internal/policy/engine.go:25-60` | [C] | OK | `DecisionAsk = "ask"` L25 |
| 12 | `apps/Application Plane/notification-core/internal/http/handlers.go:199-212` | [C] | OK | allowlist L201-212: capability-core only `modelplane.run_completed`, `modelplane.run_failed` |
| 13 | `apps/Application Plane/notification-core/migrations/013_register_run_watch_notifications.up.sql:1-15` | [C] | OK | `INSERT INTO notification_channel_configs` L4 |
| 14 | `apps/Model Plane/go/services/capability-core/internal/runwatch/consumer.go:1-50` | [C] | OK | package doc L1 (AUTO-2 "notify me when a run finishes") |
| 15 | `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1889-1935` | [M] | OK | `export function ApprovalRequests(props` L1889 |
| 16 | `apps/Frontend Plane/verevonv3/src/shared/api/orchestration-client.ts:13-25` | [C] | OK | `export type Approval = {` L13 |
| 17 | `orchestration-client.ts:69-72` | [C] | OK | L72 `raw.reason` fallback (mapping, not the decision) |
| 18 | `orchestration-client.ts:90-97` | [C] | OK | `decideApproval(... reason?: string` L87-90; `reason: reason ?? ''` L97 |
| 19 | `apps/Frontend Plane/verevonv3/src/features/chat/lib/use-chat-shortcuts.ts:1-30` | [C] | OK | header L9-31 (Escape handling); `Mod` L84; `Mod+Shift+O` L93 |
| 20 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/run_watchers.rs:1-60` | [C] | OK | L1 `/api/v1/runs/{run_id}/watchers` |

Not line-checked: JSON `browser_events.rs:238` (see 038) and capability-core `IsSupportedScope` body (verified separately: L47-55 returns true only for org and global).

### 2. AFFECTED FILES
- `BrowserChrome.tsx` [C] (2172): risk labels L159-170; `BrowserApprovalCard` L322-395; evidence list L2039-2056.
- `ChatMessages.tsx` [M] (4060): `ApprovalRequests` L1889-1935. The uncommitted diff (22 hunks, 229 lines) is in `EmptyChatState` and `ErrorNotice`; no approval token is added.
- `KnowledgeComposer.tsx` [M] (1999): imports `withBrowserApprovalRequested` (L83) and calls it (L1075).
- `browser-session.ts` [C] (825): `withBrowserApprovalRequested` L771.
- `orchestration-client.ts` [C] (324): `Approval` L13; `decideApproval` L87-97.
- `use-chat-shortcuts.ts` [C] (200): modifiers L84; bindings L93, L97.
- `notifications-client.ts` [M] (68): `Notification` and `NotificationPreference` interfaces edited (uncommitted, 18 lines).
- `AgentRunConsole.tsx` [M] (2563): `RunWatchToggle` L2478 (not inside an uncommitted hunk).
- `run_watchers.rs` [C] (150): opt-in watchers.
- execution-core `browser_agent.rs` [C] (1709): `require_approval` reuse L220-240; `gate_risky_action` L870-945; `classify_action_risk` call L914.
- execution-core `browser_events.rs` [C] (857): `BrowserActionDecided` L136; `require_approval` L238.
- execution-core `runtime_loop/agent.rs` [M] (13521): `ApprovalKind::Permission` L4455; uncommitted `"approval_continuation"` in `continuation_descriptor` (hunk `@@ -4559,3 +4566,4 @@`).
- execution-core `browser_risk.rs` [C] (1288): `RiskCategory` L25; `classify_action_risk` L251.
- browser-broker `grant.go` [C] (362): `AllowedDomains` L45; store L64.
- capability-core `engine.go` [C] (227): decisions L25; `IsSupportedScope` L47-55.
- capability-core `runwatch/consumer.go` [C] (301).
- notification-core `handlers.go` [C] (583): allowlist L201-212. `channels.go` [C] (size not measured).
- Migrations: next free notification-core migration is `015` (`014_register_mail_task_drafted.up.sql` [C] exists). New `apps/Application Plane/notification-core/migrations/015_register_approval_requested.up.sql` and `.down.sql` [NEW].

### 3. REUSABLE BUT UNWIRED
- `withBrowserApprovalRequested` (`browser-session.ts` L771): wired (`KnowledgeComposer.tsx` L1075).
- `ApprovalRequests` chat deck: wired (`ChatMessages.tsx` L1889).
- `decideApproval` reason parameter: wired and sent (`orchestration-client.ts` L97). The browser card does not expose it.
- `classify_action_risk`: wired (`browser_agent.rs` L914).
- `CreateScopedSensitiveApproval` (`grant.go` L104): wired (`server.go` L85).
- Persisted "always" rule store: none. Proof: `grep -rln "site_rule|siteRule|always_allow|AlwaysAllow|persist.*always"` over capability-core, browser-broker and execution-core/src is empty.
- `approval_requested` notification type: none in Application Plane. Proof: grep over `apps/Application Plane` finds only social-core `needs_approval` naming.
- Mod+Enter binding: none. Proof: grep `Mod\+Enter|Ctrl\+Enter|metaKey.*Enter` over chat and dashboard home is empty.

### 4. TESTS
- SPA: `use-chat-shortcuts.test.ts` [C]; `SpaceApprovalPanel.test.tsx` [C]; `AgentRunConsole.test.tsx` [M]; `ChatMessages.failure-actions.test.tsx` [M] (not approvals).
- Model Plane: inline tests in `browser_agent/browser_risk.rs` (`classify_action_risk` cases at L565-628) [C]; `execution-core/tests/browser_agent_e2e.rs` [C]; capability-core `internal/policy/engine_test.go` [C] and `internal/runwatch/consumer_test.go` [C]; browser-broker `internal/grant/grant_test.go` [C].
- notification-core: `go test -v ./...` via `notification-core-service.yml` L42.
- Commands:
  - `cd "apps/Model Plane/rust" && cargo test -p execution-core browser_risk`
  - `cd "apps/Model Plane/go/services/capability-core" && go test ./internal/policy/... ./internal/runwatch/...`
  - `cd "apps/Application Plane/notification-core" && go test ./...`
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/lib/use-chat-shortcuts.test.ts src/features/spaces/components/SpaceApprovalPanel.test.tsx`
- CI: `model-plane-rust-ci.yml` [M] L119; `notification-core-service.yml` [C] L42; `verevonv3-ci.yml` [M] L193. No workflow runs capability-core or browser-broker Go tests.

### 5. FEATURE FLAGS / ENV
- Browser runs use `permission_mode: "auto"` (`model-gateway/src/browser_run.rs` [M] doc L288-291), so the HITL gate runs per action only. Changing this is a behaviour change.
- The notification allowlist is code (`handlers.go` L201-212), not configuration.
- ZDR: notification bodies title-only (card). No ZDR gate for notification bodies was found in this sweep ([UNVERIFIED]).
- `use-chat-shortcuts.ts`: Escape already handled globally (header L12, L30-31), so Esc-to-deny needs collision review.

### 6. SURPRISES
- `withBrowserApprovalRequested` already exists (not in the card). It is a reducer the approval card can reuse.
- In-flight change: `runtime_loop/agent.rs` [M] adds `"approval_continuation"` in `continuation_descriptor`. This is the same area as the planner redirect work (slice 2). Coordinate before editing.
- `ChatMessages.tsx` [M] is heavily edited (229 lines), but its approval deck is untouched in the working tree.
- `notifications-client.ts` [M] changes the `Notification` shape in the working tree; the notification slice must rebase onto it.
- No `Mod+Enter` binding exists. The card's keyboard plan is new work, not a fix.
- Agent-loop approvals use `pause_for_approval` with a generic `ApprovalKind::Permission` (L4455) and no risk category; the browser loop uses `require_approval` with a `RiskCategory`. Two approval shapes exist.

---

## F-t2-038 Agent presence and run control UX: animated cursor and target highlight, real annotation markers, steer/queue follow-ups, needs-attention notifications

### 1. CITATION CHECK (21 checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/browser-session.ts:127-137` | [C] | OK | L128 default capabilities include `'annotate'` |
| 2 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:1822-1857` | [C] | OK | `<For each={session().commentAnchors}>` L1844 |
| 3 | `BrowserChrome.tsx:1844-1850` | [C] | OK | L1844 |
| 4 | `BrowserChrome.tsx:413` | [C] | OK | `browserLoopRationale?: BrowserStepRationale \| null` L413 |
| 5 | `apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-run-watch.ts:424` | [C] | OK | `commentAnchors: [],` L424 (chat-run view model has no anchors) |
| 6 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:1384-1400` | [C] | OK | route doc L1384: `POST /api/v1/browser/runs/:run_id/control` (pause, resume, stop) |
| 7 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs:180-195` | [C] | OK | `let last_event_id = headers` L192 |
| 8 | `apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx:2478-2490` | [M] | OK | `function RunWatchToggle(props: { runId: string })` L2478 |
| 9 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.mid-run.test.tsx:1-20` | [C] | OK | vitest imports L6 (test content not read) |
| 10 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:930-940` | [C] | OK | `from_seq` doc L933 |
| 11 | `agent_routes.rs:2655-2706` | [C] | OK | reconnect doc L2655; no-op L2705 |
| 12 | `agent_routes.rs:2791` | [C] | OK | `pub async fn live_frame_stream(` L2791 (live frames, not replay) |
| 13 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs:386-396` | [C] | OK | `EventType::ActionStarted`, payload `{ step, action }` L392 (no geometry) |
| 14 | `apps/Model Plane/rust/services/execution-core/src/browser_events.rs:163-200` | [C] | OK | `async fn action_dispatched(` L163 |
| 15 | `browser_events.rs:238-310` | [C] | OK | `async fn require_approval(` L238 (polls the durable approval) |
| 16 | `apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:972-1011` | [C] | OK | `async fn wait_out_pause_or_cancel(` L972 |
| 17 | `apps/Model Plane/rust/services/model-gateway/src/queued_input.rs:1-80` | [C] | OK | header L1-7 describes drop, queue and refuse semantics; caps L62-66 |
| 18 | `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:7400-7440` | [M] | OK | L7405: queued for the run's next tool-round boundary (chat turns) |
| 19 | `apps/Application Plane/notification-core/internal/http/handlers.go:197-216` | [C] | OK | allowlist L201-212 |
| 20 | `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs:296` | [M] | DRIFT (minor) | L296 is `#[allow(clippy::too_many_lines)]`; `browser_run_start` is at L297 |
| 21 | `apps/Application Plane/notification-core/internal/channels/channels.go:1-40` | [C] | OK | package doc L1 |

Absence confirmed: no `getNodeForLocation|hit_test` (none in Quarry or SPA); no `needs_approval` or `approval_requested` notification type; `queued_input` is not referenced by `browser_agent.rs` (L289 is a comment).

### 2. AFFECTED FILES
- `BrowserChrome.tsx` [C] (2172): overlay host; `commentAnchors` render L1844-1857; rationale L413.
- `KnowledgeScrapePreview.tsx` [C] (496): second `commentAnchors` renderer L310.
- `browser-session.ts` [C] (825): capability default L128; `commentAnchors` type L54, stub L129-135.
- `chat-run-watch.ts` [C] (473): `commentAnchors: []` L424.
- `ChatLiveRunPanel.tsx` [C] (713): mounts compact `BrowserChrome` L608.
- `AgentRunConsole.tsx` [M] (2563): `RunWatchToggle` L2478.
- `DashboardComposer.tsx` [M] (5925): mid-run composer (test [C]; logic location not traced).
- gateway `browser.rs` [C] (6317): capability lists L2578, L2700; control route L1384-1400.
- gateway `chat/streams.rs` [C] (273): `Last-Event-ID` L192.
- Quarry `agent_routes.rs` [C] (3737): `from_seq` L930-939, no-op L2705; `live_frame_stream` L2791.
- Quarry `observation.rs` [C] (3214): `ActionStarted` payload L386-396 (where a target rect would be added).
- Quarry `quarry-core/src/contracts.rs` [C] (973): action contracts L360-460 (no geometry).
- execution-core `browser_events.rs` [C] (857): `action_dispatched` L163; `require_approval` L238.
- execution-core `browser_agent.rs` [C] (1709): `wait_out_pause_or_cancel` L972-1011 (steer or queue consumption point).
- model-gateway `queued_input.rs` [C] (482): caps `MAX_QUEUED_MESSAGES` (3) and `MAX_QUEUED_CHARS` (2000), L62-66.
- model-gateway `http_routes.rs` [M] (11681): queue route doc L7405; `require_non_zdr_durable_mutation` L823 (called at L2785, L3103).
- model-gateway `browser_run.rs` [M] (1330): `browser_run_start` L297.
- notification-core `handlers.go` [C] (583): allowlist L201-212. Next migration `015_*` [NEW].
- capability-core `runwatch/consumer.go` [C] (301).

### 3. REUSABLE BUT UNWIRED
- Overlay anchors: built as a renderer, stubbed data (`browser-session.ts` L129-135). Wired but not functional.
- `queued_input.rs`: wired to chat-turn routes in `http_routes.rs`. Not used by the browser loop (grep: `browser_agent.rs` has no `queued_input` token).
- Quarry agent SSE replay: unwired (`agent_routes.rs` L2705 no-op).
- Model Plane run replay via `Last-Event-ID`: wired (`chat/streams.rs` L192). This is the replay that works, not Quarry.
- Run watch toggle and runwatch consumer: wired for run_completed and run_failed only. No approval type.
- `require_non_zdr_durable_mutation` (`http_routes.rs` L823): reusable guard for steer and queue under ZDR.

### 4. TESTS
- SPA: `DashboardComposer.mid-run.test.tsx` [C] (content not read); `browser-session.test.ts` [C] (annotate fixture L92); `AgentRunConsole.test.tsx` [M]; Playwright `browser-workspace-live-takeover.spec.ts` [C] (no CI).
- Quarry: `crates/quarry-edge/tests/sse_stream.rs` [C].
- Model-gateway queued-input tests: location not traced ([UNVERIFIED]).
- Commands:
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/dashboard/home/DashboardComposer.mid-run.test.tsx src/features/dashboard/home/browser-session.test.ts src/features/agents/components/AgentRunConsole.test.tsx`
  - `cd "apps/Model Plane/rust" && cargo test -p model-gateway queued`
- CI: `verevonv3-ci.yml` [M] L193; `model-plane-rust-ci.yml` [M] L119; `notification-core-service.yml` [C] L42.

### 5. FEATURE FLAGS / ENV
- Queue caps are constants: `MAX_QUEUED_MESSAGES = 3`, `MAX_QUEUED_CHARS = 2_000` (`queued_input.rs` L62-66).
- Capability lists are hard-coded in the gateway (L2578, L2700); `annotate` is advertised there and in `browser-session.ts`.
- Queued input is refused for ZDR and waived tokens (`http_routes.rs` [M] L823 guard; uncommitted comment at the queue route states this).
- `permission_mode: "auto"` in `browser_run.rs` [M] (doc L288-291).

### 6. SURPRISES
- `commentAnchors` renders in KnowledgeScrapePreview too (L310); the card names only BrowserChrome.
- `chat-run-watch.ts` L424 sets `commentAnchors: []`, so chat runs cannot show anchors even after the stub is fixed.
- The card says Quarry seq replay is reused for steer or queue. That is wrong: the agent SSE replay is a no-op (`agent_routes.rs` L2705). The working replay is the Model Plane `Last-Event-ID` path.
- No geometry: `grep -i "bbox|box_model|rect"` over `quarry-core/src/contracts.rs` and `quarry-browser/src/chromiumoxide.rs` finds no bbox, box model or rect. The card's "AX-ref bbox already available" is wrong; the CDP box-model call must be added.
- Uncommitted `http_routes.rs` [M] and `browser_run.rs` [M] touch the chat queue and browser run start, but not steer or queue logic.
- `DashboardComposer.mid-run.test.tsx` suggests a mid-run composer already exists for chat turns ([UNVERIFIED] contents).

---

## F-t2-006 Reap abandoned agent runs (idle and absolute TTL), cap concurrent runs, and make the fleet console real

### 1. CITATION CHECK (20 checked)
| # | Cited | Tag | Verdict | Live evidence |
|---|---|---|---|---|
| 1 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:446-471` | [C] | OK | `fn enforce_step_budget(entry: &RunEntry)` L446 (max_steps only) |
| 2 | `agent_routes.rs:1425-1445` | [C] | OK | L1435 comment: resumed run keeps persisted posture (`Claims::zdr_floor`) |
| 3 | `agent_routes.rs:1446` | [C] | OK | `let ttl_s = constraints.max_runtime_s.unwrap_or(120);` |
| 4 | `agent_routes.rs:1502` | [C] | OK | `ttl_s,` in the lease/entry constructor (stored) |
| 5 | `agent_routes.rs:1630-1641` | [C] | OK | `let _ = state.agent_driver.release(entry.session).await;` L1630 (release on close path) |
| 6 | `agent_routes.rs:3600-3640` | [C] | OK | `.receipts` L3611 (close path) |
| 7 | `agent_routes.rs:3623-3626` | [C] | OK | `.remove(&run_id);` L3626 (only registry removal) |
| 8 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:331-340` | [M] | OK (live) | `RuntimeLeasePool::new(2)` L337 |
| 9 | `main.rs:1280-1290` | [M] | OK (live) | comment L1280; `ChromiumoxideDriver::new()` L1285 under `#[cfg(feature = "browser-agent")]` L1283 |
| 10 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lease_pool.rs:1-140` | [C] | OK | `LeaseMetrics` L15 (pool file) |
| 11 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_driver.rs:128-142` | [C] | OK | L130 doc: scrape falls back to full render |
| 12 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1914-1942` | [C] | OK | `async fn acquire(&self, lease)` L1914; single shared browser mutex L1917 |
| 13 | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/fleet_routes.rs:200-222` | [C] | OK | `fleet_router()` L216; routes L219-222 are GET and POST only |
| 14 | `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs:655` | [C] | OK | `pub async fn close_run(&self, run_id, org_id)` L655 |
| 15 | `apps/Frontend Plane/verevonv3/src/features/agents/components/FleetRunConsole.tsx:26` | [C] | DRIFT (minor) | `fetchFleet` L27-28 (`/v1/fleets/${fleetId}`) |
| 16 | `FleetRunConsole.tsx:131` | [C] | OK | L131 text `live view: /v1/agent/runs/.../live-view` (a string, not a stream) |
| 17 | `FleetRunConsole.tsx:100-160` | [C] | OK | member tiles L118-126 (run id and a telemetry span); PATCH L146-150 with `status: 'cancelled'`; "Pause fleet" L155 |
| 18 | `apps/Frontend Plane/verevonv3/src/app/App.tsx:216` | [M] | OK (live) | `/agents/fleets/:fleetId` route L216 |
| 19 | `apps/Frontend Plane/verevonv3/src/shared/api/quarry-client.ts:127` | [C] | FALSE | L127 is `listQuarryRequestQueues`; no fleet call in this file |
| 20 | `apps/Model Plane/go/services/browser-broker/internal/server/server.go:22` | [C] | OK | `defaultGrantTTL` L22 (also in F-011) |

Absence confirmed: no `sweep|reap|janitor|idle_` in `quarry-edge/src`; no `.ttl_s` reader (writes at L1502, test-only L2074 and L2100 inside the inline test module); no `MAX_CONCURRENT|max_concurrent|RESOURCE_BUSY` in `quarry-edge/src`; no gateway `/v1/fleets` route.

### 2. AFFECTED FILES
- `agent_routes.rs` [C] (3737): `enforce_step_budget` L446-471 and call L2294; resume persistence L1425-1445; TTL L1446, L1502; release L1630; close path L3600-3640 (receipts L3611, registry removal L3626); caller ownership L474-485.
- `quarry-edge/src/main.rs` [M] (1569): `RuntimeLeasePool::new(4)` L323, `new(2)` L337; `durability_required` L179-182; agent driver L1283-1288. Uncommitted hunks inside `main()` (L244-1463) do not touch these lines.
- `quarry-runtime/src/lease_pool.rs` [C] (294); `quarry-runtime/src/browser_driver.rs` [C] (581; `BrowserDriverAdapter` L133-142; test `RuntimeLeasePool::new(4)` L395).
- `quarry-browser/src/chromiumoxide.rs` [C] (3551): `acquire` L1914.
- `quarry-browser/src/pool.rs` [C] (size not measured): `LeasePool<D>` tokio `Semaphore` admission (L32-43); callers `quarry-edge/src/main.rs` [M], `quarry-runtime/src/browser_driver.rs` [C], and `quarry-runtime` `egress_broker.rs`, `fetch.rs`, `lib.rs` [tag not run] (grep).
- `quarry-edge/src/fleet_routes.rs` [C] (252): routes L219-222.
- `quarry-runtime/src/fleet.rs` [C]: `FleetRegistry` used by `fleet_routes.rs` L26.
- `quarry-orchestrator/internal/fleet/fleet.go` [C] (196) and `fleet_test.go` [C]: no HTTP wiring found under `cmd/` or `internal/http` (grep).
- `quarry-control/internal/store/pg/migrations/014_fleets.sql` [C]: durable fleet table; no Go code references it (grep).
- execution-core `quarry_agent.rs` [C] (963): `close_run` L655.
- browser-broker `server.go` [C] (179): `RevokeGrant` L109-110 (defined); `authz.go` [C] L59 names the RevokeGrant method.
- SPA: `FleetRunConsole.tsx` [C] (166): `fetchFleet` L27-28; budget L35; PATCH L146-150; "Pause fleet" L155. `App.tsx` [M] (250): route L216. `quarry-client.ts` [C]: no fleet calls.
- Migrations: `quarry-runtime/migrations/0010_*.sql` (next free) for any durable run registry. Existing: `0006_agent_run_checkpoints.sql` [C], `0007_step_receipts.sql` [tag not run].
- Gateway: no fleets route (grep).

### 3. REUSABLE BUT UNWIRED
- `LeasePool<D>` (`quarry-browser/src/pool.rs`): wired to scrape and egress paths, not to the agent `ChromiumoxideDriver` (`main.rs` L1284-1288). The concurrency-cap primitive for runs.
- `RevokeGrant` (`browser-broker/internal/server/server.go` L109-110): no production caller found in Model Plane rust, gateway or quarry-edge (grep, excluding tests).
- Fleet orchestrator (`quarry-orchestrator/internal/fleet/fleet.go`) and fleet table (`014_fleets.sql`): unwired to HTTP and storage.
- `ttl_s`: stored (L1502), never read.
- `PersistentSessionRegistry` idle reaper (`quarry-browser/src/persistent_session.rs` L55-110): unwired (see F-t2-011).
- `enforce_step_budget` 429 (`agent_routes.rs` L2295 `TOO_MANY_REQUESTS`): wired; it is the typed-429 pattern to copy.

### 4. TESTS
- Quarry inline: `step_budget_is_fail_closed_once_limit_is_reached` (L2063) and `allowed_domains_accept_exact_and_subdomains_only` (L2051) in `agent_routes.rs`.
- Quarry: `quarry-runtime/tests/integration_runtime.rs` [tag not run] (not read); `quarry-runtime/tests/browser_egress_boundary.rs` [C].
- `quarry-browser/src/pool.rs` inline `MockDriver` tests (L148) [C].
- Go: `quarry-orchestrator/internal/fleet/fleet_test.go` [C].
- SPA: `FleetRunConsole.test.tsx` [C].
- Commands:
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge step_budget_is_fail_closed`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-browser pool`
  - `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/fleet/...`
  - `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/agents/components/FleetRunConsole.test.tsx`
- CI: `quarry-v2-ci.yml` [C] L205 (`cargo test --workspace`) and L329 (`go test -coverprofile ./...`, modules not traced); `verevonv3-ci.yml` [M] L193.

### 5. FEATURE FLAGS / ENV
- `QUARRY_EDGE__REQUIRE_DURABLE_RUNTIME` (`main.rs` L180-182) and `ENVIRONMENT` prod (L179) require a durable runtime.
- Cargo feature `browser-agent` gates the agent driver (`main.rs` L1283).
- Lease sizes are constants: `RuntimeLeasePool::new(4)` (L323) and `new(2)` (L337). No env var sets run caps; none exists.
- TTL default 120 s (`agent_routes.rs` L1446); test TTLs 60 s (L2074, L2100).
- Step cap `max_steps` 12 is hard-coded in the gateway (`browser.rs` L485).

### 6. SURPRISES
- JSON evidence `quarry-client.ts:127` is FALSE for fleets: L127 is `listQuarryRequestQueues`.
- FleetRunConsole's "Pause fleet" button (L155) sends `PATCH` with `status: 'cancelled'` (L146-150). The edge registers no PATCH (`fleet_routes.rs` L219-222). The button is both mislabelled and unrouted.
- A typed 429 already exists for the step budget (`agent_routes.rs` L2295). Concurrency caps can copy that pattern.
- The fleet tables and orchestrator exist but are not connected. The card's "fleet console is a stub" is correct, and the fix is larger than a proxy: the durable backend exists unwired.
- Greps for `beforeunload|sendBeacon` over `src/` find only `src/demo/product-demo/guard*.ts` [tag not run] (demo scaffolding). The browser UI has no unload close, as the card says.
- `main.rs` [M] has 18 uncommitted hunks in `main()`. Re-check the lease wiring after those land.
- A nested worktree exists at `apps/Ingestion Plane/Quarry-v2/.claude/worktrees/vigilant-banach-6933aa` (untracked). Greps over whole Quarry-v2 trees must exclude `.claude/`, or they double-count migrations and code.

<!-- COMPLETE -->
