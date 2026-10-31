# wf-W2-U27 progress (WS9.B reload and navigation error card)

Status 2026-10-23: code written, NOT yet committed. Compile check in progress (qtest cargo check, log in the scratchpad W2-U27/check1.log).

Uncommitted files owned by U27 (none pre-existing with foreign hunks at the start, git status was clean for every path):
- apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs (Reload variant, unit test module)
- apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/actions.rs, lib.rs (Reload action, BrowserDriver::reload default)
- apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs (reload impl before forward, live test reload_ignores_cache)
- apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs, action_runtime.rs, challenge.rs, action_cost.rs
- apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/web_policy/action_class.rs
- apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/budget.rs (label table, test module)
- apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml (ReloadAction, additive)
- apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md (append-only WS9.B section)
- apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs (reload in sanitize_action, refusal for agent actor, capability strings, tests)
- apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts (BrowserAction reload)
- apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx, KnowledgeScrapePreview.tsx, BrowserChrome.tsx (wiring hunks)
- apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.reload.test.tsx (NEW)
- apps/Frontend Plane/verevonv3/src/features/browser/session/browser-navigation-error.ts, browser-navigation-error.test.ts (NEW)
- apps/Frontend Plane/verevonv3/src/features/browser/omnibox/BrowserNavigationError.tsx, BrowserNavigationError.css (NEW)
- .plan-work/quarry-w1w2/depth/W2-U27.md, W2-U27-progress.md, wf-W2-U27.md, wf-W2-U27-progress.md

Next: read check result, fix compile errors in own files, run tsc on the four SPA files if cheap, commit in steps with qcommit.sh.
