# Velion Browser Workspace — Program Plan

> **Goal:** a Codex-like in-app browser inside Velion v3 (reference: https://developers.openai.com/codex/app/browser), built on the existing Quarry-v2 + Model Plane + Velion gateway architecture. A user or agent can visit pages, preserve cookies through scoped browser profiles, see live/near-live page state, execute browser actions, inspect deterministic evidence, replay steps, and let Model Plane suggest or run bounded browser actions.
>
> **Division of authority (non-negotiable):** Quarry-v2 **executes and captures evidence**. Model Plane **reasons** (VLM / action planning). Velion v3 **displays and controls**.

## Hard constraints

- Do **not** move VLM reasoning into Quarry.
- Do **not** use OpenCV for anti-bot bypass.
- Do **not** replace Model Plane vision providers.
- Do **not** persist ZDR visual artifacts, screenshots, cookies, or profile state.
- Keep Go control/orchestrator services free of OpenCV.
- Quarry executes and captures; Model Plane reasons; Velion displays and controls.
- All outputs deterministic and artifact-backed where possible.

## Key locations

| Layer | Files |
|---|---|
| Quarry-v2 evidence/executor | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/{observation.rs,page_renderer.rs,page_image.rs,vision.rs}`, `crates/quarry-core/src/{contracts.rs,artifact.rs}`, `services/quarry-vision-sidecar`, `deploy/compose/docker-compose.yml` |
| Velion gateway browser facade | `apps/Frontend Plane/velionv3/apps/gateway/src/domains/browser.rs` (proxies sessions/actions/artifacts to Quarry; suggestions to Model Plane) |
| Velion v3 browser UI | `src/features/dashboard/home/{KnowledgeComposer,KnowledgeScrapePreview}.tsx`, `src/features/dashboard/home/browser-session.ts(+.test.ts)`, `src/shared/api/browser-client.ts`, `src/styles/global.css` |
| Model Plane reasoning | `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` (`/v1/browser/suggest-action`), `services/execution-core/src/{browser_agent.rs,llm_planner.rs}` |

## Verified current state (recon 2026-07-07)

- Quarry runs Chromium browser sessions and returns observations; captures screenshots, DOM summaries, console/network summaries, visual artifacts, OpenCV visual observations, visual diffs, thumbnails, tiles, OCR-preconditioned images, palette/logo candidates.
- Gateway `browser.rs` exposes: `POST /api/v1/browser/sessions`, per-session action/artifact/close routes, `GET /api/v1/browser/profiles` (+ probe/delete). Suggestion proxy to Model Plane wired.
- Model-gateway `/v1/browser/suggest-action` is live (committed via Stream 1 "cost-aware browser planner"): planner prompt + JSON-schema single-action output; Quarry captures evidence, Model Plane only decides the next action.
- SPA `browser-client.ts` has the full typed surface (sessions, actions, suggestions, profiles incl. `probeBrowserProfile`/`deleteBrowserProfile`, `BrowserProfileScope = ephemeral|user_private|org_shared|run_scoped`, `BrowserTimelineEntry`, `BrowserVisualEvidence`, `BrowserFrame`, render modes `chromium|dom_snapshot|readability_fallback`).
- SPA UI (KnowledgeScrapePreview + browser-session view-model): manual browser actions, one AI-suggested step, a capped AI loop, timeline/replay display.
- Cookie/profile persistence exists through Quarry browser profiles — needs better UI/management.
- **Working-tree status:** the current browser surface is uncommitted WIP (+353/−22 across the 7 UI/gateway files) on top of `d91f74e5`+ — Phase 1 verifies and commits this as its baseline.

## Target capabilities (definition of done for the program)

1. Live or near-live browser frame updates.
2. Manual controls: navigate, back, forward, reload, click, type, press, scroll, wait, select, screenshot.
3. Model Plane action suggestions and bounded autonomous multi-step runs.
4. Pause, resume, stop, and user interrupt for running browser loops.
5. Browser step timeline with before/after screenshots, visual diffs, DOM deltas, console logs, network logs, policy denials, and model rationale.
6. First-class artifact viewer: screenshots, annotated screenshots, `visual_change.json`, `visual_observation.json`, `tiles.json`, thumbnails, OCR-preconditioned images, palettes, logo candidates.
7. Cookie/profile UX: create, name, reuse, inspect, delete, isolate, persist per user/org/run.
8. Permission/approval gates for risky actions: login, checkout, posting forms, destructive actions, downloads, uploads, cross-domain navigation, persistent cookie use.
9. ZDR mode that disables persistence and clearly marks ephemeral evidence.
10. File upload/download handling where policy permits.
11. Dialog/popup handling.
12. Viewport/device controls.
13. Session replay and debugging export.
14. E2E tests: session creation, cookie persistence, action execution, AI loop, visual evidence, replay timeline, ZDR no-persistence.

## Phases

| Phase | Scope | Status |
|---|---|---|
| **1 — Strengthen existing dashboard browser** | Expand `BrowserSessionSurface` into a richer browser workspace; timeline detail viewer + artifact preview panels; pause/stop state for the AI loop; clear profile/cookie status. *(Baseline: verify + commit the existing browser WIP first.)* | **done** — WIP foundation verified (gateway fmt/clippy/tests, SPA tsc/lint/249 vitest, live smoke 10/10) and landed via `0300f009c` (global.css/plan.rs checkpoint) + `333a6494` (gateway facade: timeline, visual-obs to Model Plane, per-step evidence, zdr) + `e03494e2` (view-model: timeline detail, artifact descriptors, rationale, pausable loop controller) + `39f56820` (workspace UI: status/profile strips, loop controls, timeline detail + typed artifact viewer); gate fixes `394d6c44`. Live-verified in-app (Playwright): real AI loop step with pause/resume/stop, before/after + DOM delta, verbatim model rationale. Addendum: unified single-chrome refinement via `3d328284` — one browser-tab strip + one toolbar (nested panel/status bands killed), profile popover from the padlock, collapsible DevTools + evidence drawer, dismissible AI bubble, ZDR chip stays always-visible; live-verified. |
| **2 — Durable browser-agent run** | Connect the Model Plane browser-agent loop to the Velion run UI; stream events from Model Plane/Quarry into Velion; record action, observation, suggestion, rationale, and artifacts per step. | pending |
| **3 — Profile/cookie management** | UI for Quarry profiles; persistent vs ephemeral selection; enforce ZDR no-persistence. | pending |
| **4 — Live/near-live frames** | Periodic screenshot refresh or streaming frame transport; bounded + policy-aware; no raw remote desktop until needed. | pending |
| **5 — Approvals & policy** | HITL gates for risky browser actions; denials + required approvals surfaced in the timeline. | pending |
| **6 — Testing & runtime** | Playwright E2E coverage; verify compose wiring across Velion gateway, Model Gateway, Quarry edge, OpenCV sidecar, inference-core, profile storage; ZDR behavior tests. | pending |

## Success criteria

- A user can open a URL inside Velion, interact manually, let AI run bounded browser steps, replay every step, inspect evidence artifacts, and persist/reuse cookies only when explicitly using a persistent profile.
- Quarry never reasons over vision. Model Plane never executes browser actions directly. ZDR sessions leave no persisted visual/cookie/profile artifacts.
