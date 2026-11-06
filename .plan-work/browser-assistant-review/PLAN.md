# Plan: Agent-First Browser in Verevon — Codex + browser-use parity to 100%

**Goal (owner):** Verevon ships a real browser its *agents* control, with a live real-time view — the T3/Codex/IDE-tool pattern. It is agent-first (the browser belongs to the agent; the user watches, steers, takes over), not user browsing.

**Parity scope.** "100% parity" = 100% of the *adoptable* set from the reference review (`REPORT.md` §5): Codex in-app browser (annotation, per-site permission levels, history search/clear, confirm-sensitive, split/floating) + browser-use (sensitive placeholders, replay with variables) + the adoptable ideas from open-computer-use / jev-ultrafast. Explicitly **not** parity (rejected by design, ADR-5/6/7): developer mode/full CDP, attaching to the user's real Chrome profile, automated uploads, OS profile import, stealth/CAPTCHA, desktop CUA, model-written JS, direct MCP actions, Bash.

**Ground rules (unchanged):** work in worktree `C:\dev\CoresSystem-assistant-slices`, branch `feat/browser-assistant-slices`, local commits only; every new behaviour behind a default-off flag; never `--no-verify`; never flip live flags/containers; verify by running; report what was verified by running vs. only read.

Reference architecture: Quarry = Chromium driver/executor (agent loop in execution-core). Ingestion/Model planes as in `AGENTS.md`. Exact code anchors below are the live state as of the 11 local commits.

---

## Current state (verified by running tests before commit)

Already shipped, all behind flags: slice 1 ZDR refusal, slice 2 durable navigation runs (`POST /v1/ai/assistant-navigation`, `model-gateway/src/assistant_navigation.rs`), slice 3 hub overview, slice 5 local bookmarks, slice 6 PDF content-type + zoom, slice 4 annotation (clipboard), slice 7 dock positions/tab menu/new-tab/keymap/split/floating, slice 8 interactive element filter, slice 9 redaction, slice 10 replay helper. Live view is an SSE frame stream (`AssistantLiveBrowser.tsx`); `BrowserChrome.tsx` already has a WS→SSE→polling ladder. Session model is per-task ephemeral (TTL 5 min, 1 host, ~20 steps). `assistant-browser-agent.ts` owns the task loop (session → site choice → run → events → approvals → control).

---

## Phase 0 — Design calls (decisions, not code)

Each call names owner, deliverable, and the phase it unblocks. **Nothing starts without its call recorded.**

| # | Call (from earlier list) | Owner | Deliverable | Unblocks |
|---|---|---|---|---|
| 0.1 | Slice 3: download/upload/recording catalogue | Control Plane (org-core) | Signed-off catalogue: levels, defaults, org-restrictability | Phase 6 permission levels migration, broker + Quarry gate |
| 0.2 | Slice 9: secret-custody ADR (ADR-2) | Model/Ingestion | ADR: where secrets live, `fill_secret` custody, redaction contract | Phase 6 `fill_secret` (redaction already shipped) |
| 0.3 | Slice 8: single-editor rule (R-27) + WS3.K tuning | Model Plane | R-27 decision + wait constants (combobox 200 ms / 2 frames) as tuning data | Phase 6 planner selective observation |
| 0.4 | Slice 2: cross-plane audit subject ownership | Model Plane (session-core) | Subject ownership for `verevon.audit.v2.model.*` navigation events | Phase 6 navigation audit event (runs already shipped) |
| 0.5 | Slice 4: governed annotation path | Model Plane (WS9/WS11) | Annotation = governed attachment contract (user-authored comment, ephemeral page region, ZDR text rule) | Phase 3 annotation→planner |
| 0.6 | Slice 5: server history retention | Data/Ingestion | Retention design: what is stored, ZDR purge, WS8.B metadata receipts as store | Phase 6 server history/bookmarks |
| 0.7 | Slice 10: replay parity semantics | Model Plane (WS10/WS8) | Zero-model-calls replayed prefix, per-step approval, fail-closed mismatch rule | Phase 6 step-picker UI on `buildReplay` (helper already shipped) |
| 0.8 | Agent session lifetime | Control + Model | Persistent-session policy: grant scope, TTL extension, per-task isolation inside a long-lived session | Phase 1 persistent workspace |

## Phase 1 — Persistent agent browser workspace (agent-first core)

- One long-lived Quarry session per (org, user) — "Verevon's browser"; tab strip always visible in the workspace; tasks attach to the live session instead of creating a new one (grant scoping already exists via `assistant_task` + broker; extend per 0.8).
- Keep all existing guards: 1-host ceiling per task, `assistant_interactive` disable-upload/download pinning, 5-min grant windows per task (renewed per task, not per session), revoke per task.
- Agent tabs stay badged `openedBy: agent`; user sees the same tab strip the agent sees.
- Flags: `AGENT_BROWSER_WORKSPACE` (gateway) + `VITE_AGENT_BROWSER_WORKSPACE` (SPA), default off.
- Verify: gateway tests (attach/reuse/revoke/isolation) + SPA tab-strip tests; both run before commit.

## Phase 2 — Real-time live view

- Port `AssistantLiveBrowser` to the WS-first ladder `BrowserChrome` already uses (`liveFrameWsUrl` → SSE → polling), with frame-rate telemetry and backoff; keep receipt-pinning (runId/sessionRunId/tabId/attachId) and cursor-telemetry validation (already in `AssistantLiveBrowser.tsx`).
- Keep the 2-min post-terminal snapshot; keep the 5 s disconnect grace only as fallback under WS.
- Flags: `VITE_AGENT_BROWSER_LIVE_WS`, default off. Verify: component tests (WS preferred, SSE fallback, stale-frame rejection) by running.

## Phase 3 — Steering (watch, pause, take over)

- Pause-and-let-me-click (WS2 inspector pattern: pause before execution) + one-click human take-over (existing `human_takeover` mode) in the live view.
- Annotation → planner governed attachment (contract from 0.5); annotations stay comments on coordinates, never raw DOM; ZDR: comment text user-authored, page region ephemeral.
- Flags: `VITE_AGENT_BROWSER_STEER`, default off. Verify: component + agent tests by running.

## Phase 4 — ZDR observe lane

- Read-only attach lane for ZDR orgs: observe + live view, no `act` (resolves the slice-1 dead-end without weakening ZDR; non-ZDR orgs keep `act`).
- Flags: `BROWSER_ZDR_OBSERVE_LANE`, default off. Verify: gateway contract test (ZDR org = observe only; non-ZDR unchanged) by running.

## Phase 5 — Multi-task browser pool

- Pool of sessions with queueing; user-visible: which task owns which tab, kill per task.
- Flags: `AGENT_BROWSER_POOL`, default off. Verify: pool isolation tests by running.

## Phase 6 — Parity depth (per design call)

- Permission levels (0.1): org-core migration (download/upload/record levels) → broker → Quarry gate → settings hub UI. Tests at each plane, run before commit.
- History (0.6): server visit log over WS8.B receipts, search + per-entry remove + Clear-by-range/type; ZDR orgs keep no entries. Test with purge included.
- Sensitive placeholders (0.2): `fill_secret` (Quarry carries the value; model sees only placeholder; receipt carries a boolean).
- Selective observation (0.3): element table + budgets in the planner view; wait constants as tuning; interactive-only filter already shipped.
- Replay UI (0.7): step-picker on top of `buildReplay` (already shipped); zero model calls per replayed step.
- Navigation audit (0.4): `verevon.audit.v2.model.*` event from session-core; gateway never publishes.

## Phase 7 — Stack evidence (STEP 2, SC1–SC7)

- Compose smoke for the worktree stack; live flag-off checks (all new flags default off, knobs have no effect when unset).
- Triage the 3 pre-existing `domains::browser` failures (409 vs expected — reproduced on clean HEAD; must be fixed or explicitly accepted before merge).
- Playwright pass on the live view + split/floating paths; full SPA suite + lint run (previously only targeted suites were run).
- Model Plane + Quarry lanes exercised end-to-end (run start → frames → approvals → replay) with evidence captured in the report.

## Phase 8 — Merge

- From a clean checkout only: `git merge feat/browser-assistant-slices` (the worktree is the same repo; no fetch needed). Local only — no push, no PR, per ground rules, unless the owner explicitly orders it.
- Progress note (`PROGRESS.md`) finalised with the evidence table and the verify-by-running column.

## Phase 9 — Runtime rollout

- Flip flags one at a time in compose/env **after** evidence, in this order: `MODEL_GATEWAY_ASSISTANT_NAVIGATION` → `VITE_ASSISTANT_DOCK_POSITION` → `VITE_BROWSER_TAB_MENU` → `VITE_BROWSER_BOOKMARKS` → feat flags → workspace/live/steer/pool flags. Each rollout step: flag change → smoke → observation window → next.
- Watch items: frame-stream cost (WS ladder), grant TTL vs. task length, ZDR lane usage, pool contention.

## Phase 10 — GA criteria (100% parity claim)

- Every row of the parity table at full depth (not partial): annotation end-to-end (planner), permission levels enforced at Quarry, server history with purge proven, `fill_secret` with custody receipts, selective observation live in planner, replay UI driving `buildReplay`, navigation audit event visible in session-core.
- SC1–SC7 evidence green; pre-existing failures closed; Playwright + full suites green.
- Owner sign-off on the parity table row-by-row.

## Risks (from report §8, still open)

Default-ask-as-consent posture unresolved · two-trace gap until 0.4 lands · observation redaction unwired until 0.3 · take-over enforcement flag to confirm in prod · actionability off by default (WS3.K).

---

*Written as a plan only; no code changed. Decisions in Phase 0 gate Phases 1–6. Nothing merges or pushes without Phase 7 evidence.*
