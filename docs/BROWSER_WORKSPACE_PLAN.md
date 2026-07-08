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
| **2 — Durable browser-agent run** | Connect the Model Plane browser-agent loop to the Velion run UI; stream events from Model Plane/Quarry into Velion; record action, observation, suggestion, rationale, and artifacts per step. | **done** (2026-07-08) — the deferred SPA wiring below landed via `de09584e`: `KnowledgeComposer.tsx`'s AI-loop Play button now calls `beginBrowserAiRun` (durable server-stream path: `startBrowserAiRun` + `streamRunEvents`), replacing the old client-side `performBrowserAutoRun` loop; `BrowserChrome.tsx`/`KnowledgeScrapePreview.tsx` thread the new `browserLoopRationale` prop into the AI bubble. `performBrowserSuggestedAction` (single-shot suggestion) untouched, as specified. Verified: `pnpm typecheck` / `pnpm lint` / `pnpm test` (55 files, 271 tests) all green with this change applied. Backend transport re-verified live via direct gateway calls as `local@velion.dev` (session create against `https://example.com`, single-frame + 3-frame bounded SSE stream, clean close). Interactive Playwright click-through of the exact Play button was attempted but inconclusive: the shared dev session had concurrent live traffic (browser tabs/timeline entries changing independent of the test's own actions — most likely the `browser-live.spec.ts` E2E suite or another concurrent session against the same long-lived dev stack), which made deterministic UI-only verification unreliable; this is an environment/concurrency artifact of the shared dev stack, not a defect in the change. Backend original landed via `548f7e25` (event schema: `reason`/`screenshot_ref`/`dom_snapshot_ref` fields, `BrowserRunPaused`/`BrowserRunResumed` events, `profile_id` passthrough, `StateStore::Paused` + `PauseRun` RPC, in-loop pause/cancel gate) + `73df7991` (model-gateway `POST /v1/browser/runs` + `.../control`, fire-and-forget `ExecuteStep(tool_name="browser_agent")`) + `ae024a94` (Velion gateway `POST /api/v1/browser/sessions/:id/ai-runs` + `.../runs/:id/control` proxies, `run_events_stream` last-event-id fix) + `d8f1cdd0` (fix: explicit-null `allowed_domains` 422) + `087703c3` (fix: `start_url` so a freshly leased Quarry session navigates before its first action instead of failing "no current page") + `f0753507` (fix: `QuarryAgentClient` always sends a bearer so `QUARRY_EDGE_AUTH_DEV_BYPASS` has a header to accept) + `4463fc90` (SPA client layer: `run-console-client.ts` event extensions, new `browser-run-client.ts`, additive server-event-driven mode in `browser-loop.ts`). Also required two local deployment-config fixes (not code, not committed — gitignored `.env` / compose): `QUARRY_BROWSER_AGENT_ENABLED=1` on execution-core (`apps/Model Plane/deploy/docker-compose.yml`, committed) and `QUARRY_EDGE_AUTH_DEV_BYPASS=1` on quarry-edge (`apps/Ingestion Plane/.env`, gitignored, matches `.env.example`'s own recommendation — no real signed JWT has ever been issued for `QUARRY_EDGE_TOKEN` anywhere in this stack). Live-verified end to end (real chromiumoxide session against `https://example.com`, signed in as `local@velion.dev`): `POST .../ai-runs` → real durable `run_id` → `GET /api/v1/runs/:run_id/events` SSE streamed real `browser_action_dispatched` (`goto` with `reason:"navigate to the run's starting page"`, then repeated `observe`) / `browser_observation_received` (`status:"success"`, real `screenshot_ref` artifact ids) / derived `step_update` events; pause-before-first-action → real `browser_run_paused` event with zero actions dispatched → resume → real `browser_run_resumed` event → 90+ subsequent actions streamed → stop → `CancelRun` accepted. `PauseRun`/`ResumeRun`/`CancelRun` also confirmed directly via `grpcurl` against execution-core's gRPC port. All sessions closed and DB left as `local@velion.dev` + `org_1782152609927` only after testing. The `KnowledgeComposer.tsx` fast-follow wiring landed 2026-07-08 (see above, `de09584e`), closing out this row. |
| **3 — Profile/cookie management** | UI for Quarry profiles; persistent vs ephemeral selection; enforce ZDR no-persistence. | **done, with one explicitly-flagged exception** (2026-07-08 continuation, closing out the row above). Delivered across 4 sub-stages plus this closing live-verification pass, all on `main`: **(a) backends** — real `name: Option<String>` + `scope: ephemeral\|user_private\|org_shared\|run_scoped` metadata added to the `ProfileStore` trait and all 4 implementations (`InMemoryProfileStore` in `quarry-browser/src/session.rs`, plus `s3_profile_store.rs`/`cached_profile_store.rs`/`postgres_profile_store.rs` in `quarry-runtime`), backed by a real versioned migration `quarry-runtime/migrations/0005_profile_metadata.sql` (`ALTER TABLE quarry_profiles ADD COLUMN name`, `ADD COLUMN scope ... DEFAULT 'user_private' CHECK (scope IN (...))` — confirmed applied against the live `ingestion-postgres`/`quarry_edge` database, schema verified via `\d quarry_profiles`) — commits `e0dba28e` + `4282a778` (413+ tests incl. live Postgres+Redis integration passes, `cargo clippy -D warnings` clean); **(b) quarry-edge HTTP surface** — `POST /v1/profiles/create`, `PATCH /v1/profiles/:id`, and `list`/`restore_probe` extended to carry name+scope, plus a fix for a real update-merge bug where a bare rename could silently persist `scope: "ephemeral"` onto a real stored profile — commit `6006e5e0` (125/125 tests); **(c) gateway ZDR-guard fix** — closed a real bypass where `{zdr: true, profileId: "<real>", scope: "ephemeral"}` could pass the Phase-3-baseline guard (`9307af55`) by claiming an explicit `scope` that contradicted the real `profileId`/`persistentProfile` facts; `effective_profile_scope` now takes the more restrictive of "what the raw facts imply" vs "what the caller claims" — commit `21d740cf` (190/190 gateway tests, +12 new); **(d) SPA UI** — full create/name/scope-pick/list/inspect/rename/delete profile-manager popover wired into the existing unified `BrowserChrome`, plus a ZDR checkbox in the pre-session composer that force-resets/disables the profile picker (client-side UX mirror only — the server-side guard in (c) is the actual enforcement) — commits `f8cbc083` + `c138757f` (`pnpm typecheck`/`lint`/`test`: 0 errors, 55 files/271 tests green). **Final live end-to-end verification** (this pass, `local@velion.dev`, dev-bypass off, via `python3 urllib`+cookiejar against the rebuilt gateway + quarry-edge containers): created a named `user_private` profile (200, correct id/name/scope) → creating with `scope: "ephemeral"` correctly rejected (400 `invalid_profile_scope`) → **the exact ZDR-bypass payload from (c)** (`{zdr:true, profileId:<real>, scope:"ephemeral"}`) → **400 `zdr_persistent_profile_forbidden`, confirmed live** → plain `{zdr:true, persistentProfile:true}` → 400 (original `9307af55` guard, still intact) → `{zdr:true}` alone (no profile) → 200, real chromiumoxide session against `https://example.com` → opened a session with the named profile, navigated to `https://httpbin.org/cookies/set/velion_test/livecheck123` (real cookie set by the real remote server, confirmed via the returned `dom_summary.text_snippet` echoing `{"cookies":{"velion_test":"livecheck123"}}`) → closed the session → `restore-probe` against the profile **before** close showed `cookies_count: 0` (correct — not yet snapshotted) and **after** close showed `cookies_count: 1, restorable: true` (genuine server-side proof the cookie was captured into the profile store, not an assumption) → renamed the profile (name-only PATCH) and confirmed `scope` was untouched → deleted the profile and confirmed it was gone from a subsequent list. **The one thing this pass could not close start-to-finish:** attempting the full loop — a *second* session reusing the *same* `profile_id` to prove the cookie is auto-resent — hit a genuine, previously-undiscovered production bug: `quarry-browser/src/chromiumoxide.rs::open_tab_page` hydrated cookies onto the freshly-opened page immediately after `new_page("about:blank")`, before the first real navigation; chromiumoxide's `Page::set_cookies` unconditionally rejects a non-http(s) current page URL (confirmed against chromiumoxide 0.9's own source: `validate_cookie_url` runs against `page.url()` before any per-cookie field is even inspected), so **every profile carrying real cookies has been unusable for a second session since this code path was introduced** — a regression nobody had actually exercised end-to-end until this stage's own live test forced it. Root-caused, fixed (hydrate cookies/storage *after* the initial `goto`, then reload so the already-completed navigation replays with the restored state — mirrors the pre-existing storage-only reload path), and covered by a new real-Chrome integration test (`reopening_a_session_with_a_saved_profile_restores_cookies_without_erroring`, gated on `CHROMIUMOXIDE_TEST=1`) that pre-seeds a profile's cookies and asserts a fresh session's `goto` now succeeds — verified on this host against a real, locally-installed Chrome (not mocked): quarry-browser's full suite passes 61/61 serially (one test flakes only under full-suite *parallel* execution due to a chromium `SingletonLock` collision on a shared temp profile dir — a pre-existing test-isolation issue unrelated to this change, confirmed by re-running serially), `cargo fmt --check` and `cargo clippy --all-targets -D warnings` both clean. Commit `8ac80c92`. **Honest gap, stated plainly:** this specific fix has *not* been re-verified against the actual redeployed `quarry-edge` container — a Docker rebuild was attempted and made genuine, monitored, non-stuck progress (confirmed via `/proc/loadavg` and per-process CPU-time sampling inside the Docker Desktop VM, ruling out a hang), but the shared sandbox's VM-level resource contention (independently confirmed: crates.io/apt network fetches complete in ~1s in an isolated container, so the slowness is VM CPU/IO contention, not network) made a full from-scratch release rebuild impractical to complete within this session. The next step is purely mechanical, no more code changes needed: rebuild `quarry-edge` (`docker compose build quarry-edge` in `apps/Ingestion Plane`), redeploy, and re-run the second-session cookie-reuse check above. A related, smaller, deliberately-*not*-fixed-in-this-pass inconsistency was also found and is worth a fast-follow: `InMemoryProfileStore`/`S3ProfileStore`/`CachedProfileStore` still default a metadata-less-but-snapshot-saved profile's reported `scope` to `"ephemeral"` (the Rust-level `ProfileScope::default()`, exercised by the *actually-deployed* backend — quarry-edge runs `InMemoryProfileStore` today, `QUARRY_EDGE__PROFILE_STORE_KIND` is unset in compose) whereas the Postgres backend's column default was corrected to `"user_private"` during sub-stage (a); the (c)-stage `update_profile` merge-gap fix already prevents this from being durably *written* back via a rename, but a profile that only ever goes through the legacy implicit-attach path (never through the new explicit `create_profile`) will still *list* as `scope: "ephemeral"` — cosmetic today (SPA's UI-created profiles are unaffected, since they always call `create_profile` explicitly) but worth aligning in Phase 5/6. DB hygiene confirmed after every step across all sub-stages: control-plane DBs hold exactly `local@velion.dev` + `org_1782152609927`; `quarry_profiles` in `ingestion-postgres` has 0 rows. |
| **4 — Live/near-live frames** | Periodic screenshot refresh or streaming frame transport; bounded + policy-aware; no raw remote desktop until needed. | **done, independently verified** (2026-07-08) — landed via `4724b6eb` (Quarry: `quarry-browser`/`quarry-edge` transient live-frame capture, human-takeover input, tabs + DevTools capture), `1069d937` (quarry-evals live-frame vs screenshot-artifact benchmark), `79b27006` (Velion gateway browser live-frame proxy: HTTP/SSE/WS, takeover control gate, tabs + DevTools + replay), `48ac55aa` (SPA: unified chrome WS/SSE frames, human takeover, tabs, DevTools, replay, omnibox), `2249693b` (Playwright e2e: frames, tabs, WS takeover, DevTools, replay). A prior agent session landed these 5 commits and was cut off before a verification transcript survived; this pass independently re-verified them rather than trusting the "already committed" status: (1) diff-read all 5 commits (`git diff d35f23fd..2249693b`, 35 files, +8167/-495); (2) grepped the full Quarry-side diff for `opencv|vlm|vision::|reasoning|llm|gpt|claude|anthropic|openai` — zero hits, confirming no reasoning/VLM/OpenCV moved into Quarry; (3) read `agent_routes.rs`'s new `live_frame`/`live_frame_stream`/`live_frame_ws` handlers — frames are explicitly documented and coded as never artifact-written ("This never writes an artifact... for live preview/human takeover only"), bounded (`quality` clamped 1-100, `max_width`/`max_height` clamped to viewport, `every_nth_frame` 1-10, `timeout_ms` 100-5000ms, stream `interval_ms` clamped 75-2000ms, `max_frames` capped at 7200), and human-takeover WS actions (`BrowserWsClientMessage::Action`) route through the same artifact-backed `ObservationRunner` as manual/AI actions — not a raw/uncontrolled input channel; (4) read the gateway `browser.rs` diff — pure HTTP/SSE/WS proxy plus a `BrowserControlMode` gate (`websocket_agent_action_is_rejected_during_human_takeover` test proves agent actions are rejected while a human has taken over), replay events are bounded/sampled and payload-free (`websocket_frame_replay_is_sampled_bounded_and_payload_free`, `websocket_devtools_events_are_deduped_sorted_and_bounded` — both passing tests), no reasoning added; (5) ran quality gates per-crate rather than full-workspace: `cargo fmt --check` (quarry-browser/quarry-edge/quarry-core/quarry-runtime, exit 0), `cargo clippy -D warnings` (quarry-browser+quarry-edge with `--features browser-agent`, exit 0; gateway, exit 0), `cargo test` (quarry-browser+quarry-edge: all pass; gateway: 174 passed/0 failed; quarry-evals lab crate: builds clean), SPA `pnpm typecheck`/`pnpm lint`/`pnpm test` (55 files, 271 tests, all green); (6) confirmed the running containers actually reflect this code, not stale BuildKit cache, by comparing image `Created` timestamps to commit timestamps — `ingestion-plane-quarry-edge` built 09:55:28, `model-plane-execution-core` built 09:54:35, `frontend-plane-velionv3-gateway` built 09:51:47, all strictly after the last Phase-4 commit (`2249693b`, 09:48:54); (7) live-verified the actual transport end-to-end as `local@velion.dev` against the real deployed gateway: `POST /api/v1/browser/sessions` (real chromiumoxide session against `https://example.com`) → `GET .../frame` returned a real single JPEG frame (raw `JFIF` bytes) → `GET .../frames/stream?intervalMs=300&maxFrames=3` streamed exactly 3 real sequential `event: frame` SSE frames then a `done` event at the `max_frames` bound → `DELETE .../sessions/:id` closed cleanly. No hard-constraint violation found; no revert needed. One unrelated stray file (`apps/Model Plane/python/eval-lab-py/src/eval_lab/types.py`, orphaned WIP from the separate `docs/EVAL_HARNESS_MVP.md` residual-item-#5 program, unrelated to quarry-evals) was found dirty in the same working tree and reverted (`git checkout --`) rather than left dangling. Honest remainder for Phase 5/6: explicit UI/policy surfacing of frame-rate/bandwidth bounds and domain/action-based takeover gating beyond the existing human-takeover mode switch, and a dedicated ZDR-ephemeral visual indicator on the live-frame UI itself (server-side ZDR marking already flows through every frame/devtools/tab payload as a `zdr` boolean — confirmed above — but no SPA-visible badge was found tying it specifically to the live-frame channel as opposed to the general ZDR chip). |
| **5 — Approvals & policy** | HITL gates for risky browser actions; denials + required approvals surfaced in the timeline. | pending |
| **6 — Testing & runtime** | Playwright E2E coverage; verify compose wiring across Velion gateway, Model Gateway, Quarry edge, OpenCV sidecar, inference-core, profile storage; ZDR behavior tests. | pending |

## Phase 2 implementation spec — durable browser-agent run (2026-07-07)

> Written from live recon (file/line citations below). Supersedes nothing above; this is
> the concrete build plan for the "pending" Phase 2 row.

### The architectural call

**Reuse the existing durable-run event backbone (session-core `StartRun` → `RecordOrchestrationEvent`
→ `StreamRunEvents` → model-gateway `GET /v1/runs/:run_id/events` → Velion gateway
`GET /api/v1/runs/:run_id/events` → SPA `streamRunEvents`), triggered by a new, narrow
"start a browser run" path that calls `ExecutionCore.ExecuteStep(tool_name="browser_agent")`
directly. Do NOT route the browser run through the generic multi-tool chat loop
(`agentic_run_stream` → `RunAgent` → `run_agent_with_tools`).**

Why the backbone, not a new stream: the transport is already real and already carries
browser-specific events end-to-end — `execution-core`'s `browser_agent::run_browser_agent_loop`
(`apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:336`) already calls a
`BrowserEventSink` per dispatched action/received observation
(`browser_agent.rs:127-132`); the concrete sink `OrchestrationEventSink`
(`execution-core/src/browser_events.rs:28-101`) already publishes
`BrowserActionDispatched`/`BrowserObservationReceived` via session-core's
`RecordOrchestrationEvent` RPC (`orchestration.proto:67-74,716-726`); model-gateway's
`run_events_sse` (`model-gateway/src/sse.rs:1677-1754`) already turns those into SSE frames on
`GET /v1/runs/:run_id/events`; the Velion gateway's `run_events_stream`
(`apps/Frontend Plane/velionv3/apps/gateway/src/domains/chat/streams.rs:67-91`) already proxies
that route at `GET /api/v1/runs/:run_id/events`; and the SPA's `streamRunEvents`
(`src/shared/api/run-console-client.ts:222`) already parses `browser_action_dispatched` /
`browser_observation_received` and is already wired up and rendering them in
`AgentRunConsole.tsx` (lines 378-397). None of that needs to be invented — only triggered and
enriched.

Why not the generic chat tool-loop: `browser_agent` is absent from
`runtime_loop::agent::offered_tool_defs()` (`execution-core/src/runtime_loop/agent.rs:419-487`),
and the one call site inside that loop that dispatches tools
(`runtime_loop::execute_step`, called from `agent.rs:292-304`) hardcodes
`browser_event_sink = None`. Routing Phase 2 through `agentic_run_stream` would require (a)
offering `browser_agent` as a tool, (b) restricting the offered tool set to just
`browser_agent` for a browser run (otherwise a browser session could call `web_search`/`shell`/
MCP tools mid-run), and (c) threading a live sink through `agent.rs`'s call site — three
non-trivial, unrelated-to-browser changes to a shared loop used by every chat run. Meanwhile
`run_browser_agent_loop` is *already* a complete bounded multi-step loop by itself (it calls
Quarry's `/v1/agent/runs/{id}/step` repeatedly inside one call, `browser_agent.rs:384-442`), and
`ExecutionCore.ExecuteStep`'s existing gRPC handler already wires a live
`OrchestrationEventSink` unconditionally
(`execution-core/src/grpc.rs:69-88`: `let browser_sink = OrchestrationEventSink::new(...); ...
Some(&browser_sink)`). So a single `ExecuteStep(tool_name="browser_agent")` call is, on its own,
"a durable multi-step run with live events" — no chat-loop nesting required.

The one real gap in the reused backbone: `run_browser_agent_loop` calls
`client.start_run(&plan.config.org_id, &constraints, zdr, None)` unconditionally
(`browser_agent.rs:367-368`) — it always opens a **fresh** Quarry browser lease, it never
attaches to the Quarry run the user already has open in the tab. Phase 2 accepts this rather
than building lease-reattachment plumbing: an AI run gets its own Quarry session, and cookie/
profile continuity with the user's tab is preserved through the **profile_id** (Quarry profiles
already persist cookies across runs; `create_session` already passes `profile_id` through to
Quarry, `browser.rs:150-176`) rather than through raw lease/run_id reuse. This requires one
small addition (below): thread `profile_id` from the gateway's `BrowserRunMetadata` through to
`PlanConfig`/`BrowserAgentInput`/`start_run`'s existing 4th parameter, which is currently wired
to a hardcoded `None`.

### Endpoints

**model-gateway (new):**

- `POST /v1/browser/runs` — start a durable browser-agent run. New handler
  `browser_run_start` in a new `model-gateway/src/browser_run.rs`, registered next to the
  existing `/v1/browser/suggest-action` route (`http_routes.rs:1072`).
  Request:
  ```json
  {
    "org_id": "org_123", "user_id": "user_456",
    "thread_id": null,
    "goal": "Find the cheapest flight to Oslo on the airline's own site",
    "grant_id": "session:qrun_abc123",
    "plan_id": null,
    "profile_id": "prof_789",
    "allowed_domains": ["norwegian.com"],
    "max_steps": 20, "max_runtime_s": 120,
    "stop_criteria": "", "require_approval": false, "max_cost_usd": 0.50,
    "zdr": false
  }
  ```
  Behavior (mirrors `session_flow::prepare_run` + `spawn_run_dispatch`,
  `model-gateway/src/session_flow.rs:72-126` and `sse.rs:1858-1896`):
  1. `SessionCoreClient::create_thread` (or reuse `thread_id`), then
     `StartRun(StartRunRequest{thread_id, agent_id:"browser-agent", goal, mode:"execute",
     org_id, user_id})` → durable `run_id`.
  2. Generate `plan_id = mp_ids::new_ulid()` if not supplied.
  3. `tokio::spawn` (fire-and-forget, exactly like `spawn_run_dispatch`,
     `sse.rs:1879-1896`) a call to `execution_client.execute_step(ExecuteStepRequest{
     tool_name: "browser_agent",
     tool_input: <JSON BrowserAgentInput below>,
     permission_mode: "auto",
     hook_context: "", org_id, user_id, run_id, step_id: mp_ids::new_ulid() })`.
     `permission_mode: "auto"` is a deliberate choice — see "Permission mode" below.
  4. Returns `200 {"run_id": "...", "thread_id": "...", "plan_id": "..."}` immediately.
     This route is **not** itself SSE; the caller observes progress via the existing
     `/v1/runs/:run_id/events`.

- `POST /v1/browser/runs/:run_id/control` — pause / resume / stop. New handler
  `browser_run_control`, body `{"action": "pause" | "resume" | "stop"}`. Maps to:
  - `stop` → `execution_client.cancel_run(CancelRunRequest{run_id, reason:"user_stop"})`
    (RPC already exists, `grpc.rs:218-226`, unchanged).
  - `resume` → `execution_client.resume_run(ResumeRunRequest{run_id})` (already exists,
    `grpc.rs:196-216`, unchanged).
  - `pause` → `execution_client.pause_run(PauseRunRequest{run_id})` — **new** RPC (below).
  Returns `200 {"status": "cancelled" | "running" | "paused"}`.

**model-gateway (reused unchanged):** `GET /v1/runs/:run_id/events` (`sse.rs:1677`).

**Velion gateway (new, in `apps/Frontend Plane/velionv3/apps/gateway/src/domains/browser.rs`):**

- `POST /api/v1/browser/sessions/:session_id/ai-runs` — new handler `start_ai_run`, added
  to `browser::router()` (`browser.rs:106-135`) under the same `require_session` layer.
  Reads `state.browser_run_store` for the session's `BrowserRunMetadata` (`profile_id`,
  `zdr` — `browser.rs:48-63`), mints a model-plane bearer via
  `shared::model_token(&state, &user, &headers)` (audience `"model-plane"`,
  `domains/chat/shared.rs:29-45`), resolves `org_id` via
  `authorized_org_id(&state, &user)` (`upstream.rs:115`), then calls
  `shared::proxy_model_json(&state, Method::POST, "{model_gateway_url}/v1/browser/runs",
  Some(body), token, &user)` with:
  ```json
  {
    "org_id": "<authorized_org_id>", "user_id": "<user.user_id>",
    "goal": "<request body 'goal'>",
    "grant_id": "session:<session_id>",
    "profile_id": "<metadata.profile_id or null>",
    "allowed_domains": "<request body, optional>",
    "max_steps": "<request body, optional>",
    "max_runtime_s": "<request body, optional>",
    "stop_criteria": "<request body, optional>",
    "require_approval": "<request body, optional, default false>",
    "max_cost_usd": "<request body, optional>",
    "zdr": "<metadata.zdr — server-derived, never trusts a client-supplied flag>"
  }
  ```
  `grant_id: "session:{session_id}"` is an **interim, synthetic** grant id, not a real
  Model-Plane browser-grant record — there is no grant-issuance flow to reuse today
  (`BrowserAgentInput.grant_id` is a bare required `String`, `tool_bridge/mod.rs:141`,
  with no registry backing it anywhere in the recon). Real grant issuance belongs to Phase 5
  ("Approvals & policy"); Phase 2 just needs a stable, session-scoped string that flows
  through unchanged. Returns `{run_id, thread_id, plan_id}` verbatim.

- `POST /api/v1/browser/runs/:run_id/control` — new handler `control_ai_run`, proxies via
  `proxy_model_json` to `POST {model_gateway_url}/v1/browser/runs/{run_id}/control`
  with the same body forwarded verbatim.

**Velion gateway (reused, one required fix):** `run_events_stream`
(`domains/chat/streams.rs:67-91`) is reused **unchanged in shape** for browser runs — the SPA
calls the exact same `GET /api/v1/runs/:run_id/events` with the browser run's `run_id`. The
one fix Phase 2 must land here: `run_events_stream` currently hardcodes `last_event_id: None`
when calling `proxy_sse_stream` (`streams.rs:86`), unlike `resume_stream` which correctly reads
`headers.get("last-event-id")` (`streams.rs:45-48`). Apply the same header-forwarding to
`run_events_stream` so a reconnecting browser-run client actually resumes via
`after_event_id` instead of silently re-observing from the live tail. This is a pre-existing
gap on a route Phase 2 newly depends on, not new scope.

### Per-step event schema

Reused unchanged (already implemented, already flowing to `AgentRunConsole.tsx`):

```protobuf
// orchestration.proto:686-697 (unchanged tags 1-5)
message BrowserActionDispatched {
  string run_id = 1;
  string plan_id = 2;
  string action_id = 3;
  string action_type = 4;   // goto|click|type|extract|observe|scroll|wait
  string url = 5;
  string reason = 6;        // NEW — model's rationale for choosing this action
}

// orchestration.proto:700-713 (unchanged tags 1-6)
message BrowserObservationReceived {
  string run_id = 1;
  string plan_id = 2;
  string action_id = 3;
  string status = 4;         // success|failed|timeout|blocked
  string page_url = 5;
  string page_title = 6;
  string screenshot_ref = 7;      // NEW — artifact reference id, never inlined bytes
  string dom_snapshot_ref = 8;    // NEW — artifact reference id, never inlined bytes
}
```

Both new fields are **honest, not invented**: `reason` closes a gap recon found explicitly —
the planner's `ACTION_SCHEMA` already asks the model for a `"reason"` field
(`llm_planner.rs:33-43`) but `NextAction` never captures it
(`llm_planner.rs:167-176`, confirmed by the test `extra_unknown_fields_are_ignored`,
`llm_planner.rs:267-275`, which explicitly documents the field being dropped). Fix: add
`#[serde(default)] reason: String` to `NextAction`, carry it into a new `pub reason: String`
field on `BrowserAction` (`browser_agent.rs:88-97`), and have
`browser_events.rs::action_dispatched` copy `action.reason.clone()` into the new proto field.
`screenshot_ref`/`dom_snapshot_ref` are **already present** on `BrowserObservation`
(`browser_agent.rs:99-111`, fields `screenshot_ref: String, dom_snapshot_ref: String`) — they
are simply never copied into the proto today (`browser_events.rs:85-100` only copies
`status`/`page_url`/`page_title`). Fix is a two-line addition to that existing translation, no
new capture logic anywhere. `extracted_text` (also on `BrowserObservation`) is **deliberately
excluded** from the wire event — including scraped page content on the durable
orchestration-event record would defeat ZDR intent for a ZDR run; the event stream stays
structural metadata only (status/url/title/refs), matching what recon already found is
selectively surfaced today.

New additive event kinds for run-level pause/resume (distinct from
`RunPausedForApproval`/`RunResumedAfterApproval`, tags 15-16, which are HITL-approval-specific
and out of scope here):

```protobuf
// orchestration.proto: new tags 19-20 ("B5" — user-initiated browser-run control)
BrowserRunPaused browser_run_paused = 19;
BrowserRunResumed browser_run_resumed = 20;

message BrowserRunPaused {
  string run_id = 1;
  string plan_id = 2;
}
message BrowserRunResumed {
  string run_id = 1;
  string plan_id = 2;
}
```

model-gateway's `orchestration_event_to_sse` and `orchestration_event_to_step_update`
(`sse.rs` ~2126-2148, `sse.rs:2148`) get new match arms for these two variants (SSE event
names `browser_run_paused` / `browser_run_resumed`), and the existing arms for
`BrowserActionDispatched`/`BrowserObservationReceived` get their `detail` strings extended to
include `reason` / artifact refs so the unified `step_update` timeline entries carry them too.

### Pause / resume / stop → server-side control

Today `run_browser_agent_loop` has **no** external control hook: `PlanStore`
(`browser_agent.rs:152-207`) is constructed but never populated by the loop, and
`StateStore`'s existing `cancel()`/`resume_run` RPCs (`grpc.rs:196-226`, `state.rs:76-82`) are
never polled from inside the loop — `RunStatus::Cancelled` can be *set* today but nothing
*reads* it while a browser run is in flight. Phase 2 closes this using the state store that
already exists rather than inventing a second one:

1. `state.rs`: add `RunStatus::Paused` (additive enum variant next to the existing
   `Running | AwaitingApproval | Completed | Failed | Cancelled`, `state.rs:8-14`).
2. `grpc.rs`: add `pause_run` RPC, mirroring `resume_run`/`cancel_run` exactly
   (`grpc.rs:196-226`) — sets `RunStatus::Paused` via a new `StateStore::pause(run_id)`
   method (mirrors `StateStore::cancel`, `state.rs:76-82`).
3. `execute_step`'s existing call site (`grpc.rs:72-89`) passes `Some(&self.state)` as a new
   trailing argument.
4. `runtime_loop::execute_step` (`runtime_loop/mod.rs:118-129`) gains an 11th parameter
   `state: Option<&crate::state::StateStore>`, forwarded into the `browser_agent` branch
   (`mod.rs:155-156`) → `tool_bridge::execute_browser_agent` gains a `state` parameter →
   `run_browser_agent_loop` (`browser_agent.rs:336-341`) gains a 5th parameter
   `state: Option<&StateStore>`.
5. `decide_next_action` (`browser_agent.rs:459+`) checks `state.get_or_create(&plan.config.run_id).status`
   at the top of each iteration, before consulting the planner:
   - `Cancelled` → `plan.status = PlanStatus::Aborted` (variant already exists,
     `browser_agent.rs:24`, currently dead — nothing sets it today) and return a new
     `PlanStepResult::Aborted(String)` variant, causing the outer `loop` to `break` with
     "cancelled by user" and fall through to the existing `close_run` cleanup
     (`browser_agent.rs:445-447`, unchanged — the lease is always released).
   - `Paused` → call `sink.run_paused(&plan.config)` once (new `BrowserEventSink` method,
     mirrors `action_dispatched`/`observation_received`), then poll
     (`tokio::time::sleep(Duration::from_millis(500))`, bounded loop) re-checking
     `StateStore` until it flips to `Running` (call `sink.run_resumed(&plan.config)`, resume
     gating normally) or `Cancelled` (fall into the `Aborted` path above).
6. `browser_events.rs`: implement `run_paused`/`run_resumed` on `OrchestrationEventSink`,
   publishing the two new proto variants — same best-effort/no-op-if-`run_id`-empty shape as
   the existing two methods (`browser_events.rs:66-100`).

**Scope boundary:** this is *user-initiated* pause/resume/stop only (new `RunStatus::Paused`/
`Cancelled` path above). `PlanConfig.require_approval` → `PlanStatus::WaitingApproval`
(`browser_agent.rs:295-298`) is a *different*, pre-existing in-loop gate that today just ends
the loop with no distinguishing sink event — richer HITL-approval UX for that path is Phase 5's
job ("Approvals & policy"), not Phase 2's. Don't conflate the two "paused" concepts.

### Permission mode

The Velion-triggered `ExecuteStepRequest.permission_mode` is `"auto"`, not `"ask"`. Under
`"ask"`, `permission::evaluate_call` (`permission/mod.rs:41-55`) unconditionally routes
`browser_agent` (it's in `is_risky_tool`'s list, `permission/mod.rs:143`) to
`AwaitApproval` — `execute_step` would return `"awaiting_approval"` and create a durable
`Approval` (`grpc.rs:96-120`) **before `run_browser_agent_loop` ever starts**, which would mean
every AI-loop click pauses immediately with no loop to pause/resume/stop at all. Since starting
the AI loop is itself an explicit, supervised user action (the user is looking at the live tab)
and the loop's own `PlanConfig` fields (`allowed_domains`, `max_steps`, `max_runtime_s`,
`stop_criteria`, `max_cost_usd`, `require_approval`) are the actual risk-containment
mechanism — already implemented and tested (domain-allowlist subdomain-attack tests,
`browser_agent.rs:655-708`) — Phase 2 treats the outer `ExecuteStep`-level HITL gate as
inapplicable here, matching how `"auto"` already works for ordinary chat tool use. A richer,
purpose-built approval surface for specific risky in-loop actions (checkout, login, posting)
is Phase 5's job, per the plan's own phase table (capability #8).

### ZDR propagation

- Client can never set `zdr` for an AI run — the Velion gateway's `start_ai_run` reads
  `zdr` from the server-held `BrowserRunMetadata.zdr` (`browser.rs:48-63`, already
  server-derived at session-create time, `browser.rs:161`, unchanged), never from the request
  body. This mirrors how `create_session` already refuses to trust a client-supplied flag today.
- `zdr` flows: Velion gateway → model-gateway `POST /v1/browser/runs` body → `ExecuteStepRequest.tool_input`
  (`BrowserAgentInput.zdr`, `tool_bridge/mod.rs:110`, unchanged) → `PlanConfig.zdr`
  (`browser_agent.rs:149`, unchanged) → `QuarryAgentClient::start_run`'s `zdr: bool` param and
  every `.step(...)` call (`quarry_agent.rs:317,353`, unchanged). Quarry is, as today, the only
  enforcement point that actually withholds persisted visual/DOM/cookie state — Phase 2 does
  not touch that contract.
- The **new** persistence surface Phase 2 introduces is session-core's durable run/thread
  record and the orchestration-event replay buffer. This is metadata only — action types,
  URLs, status strings, rationale text, and artifact **references** — never inlined
  screenshot/DOM bytes and never `extracted_text` (excluded from the wire schema, see above).
  When `zdr=true`, `screenshot_ref`/`dom_snapshot_ref` are whatever (possibly empty or
  short-TTL) reference Quarry already hands back for a ZDR observation today — Phase 2 forwards
  it as an opaque string, it does not change what Quarry decides to persist or expire.

### Cookie/profile continuity fix required for the reused loop

`run_browser_agent_loop` calls `client.start_run(&plan.config.org_id, &constraints, zdr, None)`
(`browser_agent.rs:367-368`) — the 4th argument (`profile_id: Option<String>`) is hardcoded
`None`, so an AI run today can never reuse the cookie state of a persistent profile. Fix:
add `profile_id: Option<String>` to `PlanConfig` (`browser_agent.rs:134-150`) and to
`BrowserAgentInput` (`tool_bridge/mod.rs:139-153`), thread it through
`execute_browser_agent` into `PlanConfig`, and change the `start_run` call to
`client.start_run(&plan.config.org_id, &constraints, zdr, plan.config.profile_id.clone())`.
The Velion gateway's `start_ai_run` populates this from `BrowserRunMetadata.profile_id`
(above), so an AI run launched from a tab that already has a persistent profile attached
inherits its cookies; a run launched from an ephemeral tab gets `None`, exactly as manual
`create_session` already behaves.

### Build sequencing (and why)

1. **execution-core first** (`browser_agent.rs`, `browser_events.rs`, `llm_planner.rs`,
   `tool_bridge/mod.rs`, `grpc.rs`, `state.rs`, plus the two proto edits): `profile_id`
   threading, `reason`/`screenshot_ref`/`dom_snapshot_ref` surfacing, the
   `StateStore`-aware pause/cancel gate, the new `pause_run` RPC. This is the layer recon
   proved is real and independently testable (existing unit tests cover the domain-allowlist
   and planner-mapping logic); extending it is lower-risk than building anything on top of an
   unverified base, and every other layer is a thin proxy over it.
2. **model-gateway second** (`browser_run.rs` new module, `sse.rs` mapping additions):
   `POST /v1/browser/runs`, `POST /v1/browser/runs/:run_id/control`. Thin — mostly wiring
   calls into layer 1 plus reusing the already-working `run_events_sse` unchanged — and can be
   smoke-tested standalone with `grpcurl`/`curl` against a running execution-core before
   touching Velion.
3. **Velion gateway third** (`browser.rs` new routes, `chat/streams.rs` last-event-id fix):
   mechanical, mirrors `proxy_model_json`/`proxy_sse_stream` patterns already used
   throughout this file and `orchestration.rs`. Lowest-risk layer; can be curl-verified
   against a live model-gateway before any SPA change lands.
4. **SPA last** (`run-console-client.ts` type/handler extensions, new
   `browser-run-client.ts`, `browser-loop.ts` controller rework, `KnowledgeComposer.tsx`
   wiring change): every dependency is already live and independently verifiable by the time
   this layer starts, so UI issues are isolated to the UI. This also matches how Phase 1 was
   actually executed (gateway facade → view-model → workspace UI, per this doc's Phase 1 row).

### SPA changes (files and functions)

- `src/shared/api/run-console-client.ts`:
  - Extend `BrowserActionDispatchedEvent` with `reason?: string`.
  - Extend `BrowserObservationReceivedEvent` with `screenshotRef?: string`, `domSnapshotRef?: string`.
  - Add `BrowserRunPausedEvent = { runId?: string; planId?: string; at?: string }` and
    `BrowserRunResumedEvent` (same shape); add `onBrowserRunPaused?`/`onBrowserRunResumed?` to
    `RunEventHandlers`, dispatched on SSE event names `browser_run_paused`/`browser_run_resumed`
    inside `streamRunEvents` (`run-console-client.ts:222`, same dispatch pattern already used
    for the other named events).

- New `src/shared/api/browser-run-client.ts` (mirrors the shape of `run-console-client.ts`,
  kept separate from `browser-client.ts` the same way `run-console-client.ts` is already kept
  separate from `chat-client.ts`):
  ```ts
  export async function startBrowserAiRun(
    orgId: string,
    sessionId: string,
    params: {
      goal: string;
      allowedDomains?: string[];
      maxSteps?: number;
      maxRuntimeSeconds?: number;
      stopCriteria?: string;
      requireApproval?: boolean;
      maxCostUsd?: number;
    },
    signal?: AbortSignal,
  ): Promise<{ runId: string; threadId: string; planId: string }>;

  export async function controlBrowserAiRun(
    runId: string,
    action: 'pause' | 'resume' | 'stop',
    signal?: AbortSignal,
  ): Promise<void>;
  ```
  `startBrowserAiRun` POSTs `/api/v1/browser/sessions/{sessionId}/ai-runs`;
  `controlBrowserAiRun` POSTs `/api/v1/browser/runs/{runId}/control`.

- `src/features/dashboard/home/browser-loop.ts`: keep `BrowserLoopStatus`/`BrowserLoopState`
  (BrowserChrome's rendering contract is unchanged) but make `createBrowserLoopController`'s
  internals **server-event-driven** instead of locally-flagged:
  - Remove the local `pauseRequested`/`stopRequested`/`resumeWaiters` promise machinery and
    the `gate()` checkpoint (`browser-loop.ts:12-48` region) — there is no client-side loop
    left to gate.
  - `requestPause()`/`requestResume()`/`requestStop()` now call
    `controlBrowserAiRun(runId, 'pause' | 'resume' | 'stop')` (network) instead of mutating
    local booleans; they do not themselves flip `state().status` — that happens when the
    corresponding SSE event arrives.
  - Add reducer-style handlers the caller feeds from the SSE stream:
    `onActionDispatched()` → `markActing()`dispatched-not-yet-observed sets status
    `'acting'`; `onObservationReceived()` → `markStepDone()`; `onRunPaused()` → sets status
    `'paused'`; `onRunResumed()` → sets status `'acting'`/`'suggesting'` (whichever the loop
    was in before pause); `onDone()`/`onError()` → `finish('done' | 'stopped', error?)`.

- `src/features/dashboard/home/KnowledgeComposer.tsx`:
  - Delete `performBrowserAutoRun` (the client-side `for` loop, ~lines 648-691) — this is the
    exact function Phase 2 replaces.
  - Add `async function beginBrowserAiRun(goal: string): Promise<void>` that: calls
    `startBrowserAiRun(orgId, sessionId, { goal, ...loopOptions })`, stores `runId`/`planId` in
    a new signal, then calls the **existing** `streamRunEvents(runId, { onBrowserAction,
    onBrowserObservation, onBrowserRunPaused, onBrowserRunResumed, onDone, onError },
    controller.signal)` from `run-console-client.ts` (no new SSE client needed) — each handler
    both feeds the `browser-loop.ts` reducer above and calls the existing
    `recordBrowserRationale`/`withStepRationale` (`browser-session.ts`, unchanged) using the
    event's `reason`/`screenshotRef`/`domSnapshotRef`, and `setPreview(attachBrowserSession(...))`
    to keep the timeline/evidence drawer populated from streamed events instead of synchronous
    REST responses.
  - Rewire the AI-loop Play button from `onBrowserAutoRun={performBrowserAutoRun}` to
    `onBrowserAutoRun={beginBrowserAiRun}`.
  - Leave `performBrowserSuggestedAction` (the single-shot suggest+act button,
    ~lines 621-646) **untouched** — Phase 2 targets the multi-step loop specifically; the
    plan's capability #3 lists "action suggestions" and "bounded autonomous multi-step runs"
    as two separate things, and the one-shot suggestion still legitimately uses the unary
    `POST /v1/browser/suggest-action` (`http_routes.rs:1072`, unchanged).

- `src/shared/api/browser-client.ts`: no changes. Its session/action/one-shot-suggestion/
  profile calls remain the manual-browsing and single-suggestion surface, untouched by this
  phase.

## Success criteria

- A user can open a URL inside Velion, interact manually, let AI run bounded browser steps, replay every step, inspect evidence artifacts, and persist/reuse cookies only when explicitly using a persistent profile.
- Quarry never reasons over vision. Model Plane never executes browser actions directly. ZDR sessions leave no persisted visual/cookie/profile artifacts.
