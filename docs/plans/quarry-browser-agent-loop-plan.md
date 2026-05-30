# Plan (P7) — Wire Model Plane Agentic Browser Loop → Quarry

**Status:** 🔧 IN PROGRESS — Phase A shipped + tested (7/7). Driver decided: **chromiumoxide (self-host)**, feature-gated. Phase B fully blueprinted + ready to implement.
**Date:** 2026-05-30
**Scope:** Model Plane v1 (`execution-core`) · Quarry-v2 (`quarry-edge`, `quarry-runtime`, `quarry-browser`, `quarry-core`)
**Parent:** Phase 7 of `quarry-dataplane-integration-fix-plan.md` (deferred there).

---

## Implementation log — 2026-05-30

### ✅ Phase A — DONE (Model Plane wire client)
- New `execution-core/src/quarry_agent.rs`: mirrored wire DTOs (`AgentAction` internally tagged `{"type":..}`, `AgentConstraints`, `BrowserObservation` + `DomSummary`/`InteractiveElement`/`ConsoleLine`/`NetworkEntry`, `ZdrMode`), `QuarryAgentClient` (HTTP `start_run`/`step`/`close_run`; bearer + `X-Quarry-Org`; env-gated `from_env` via `QUARRY_BROWSER_AGENT_ENABLED` + `QUARRY_EDGE_URL` + `QUARRY_EDGE_TOKEN`), internal⇄wire mapping (`action_to_wire`, `observation_from_wire`, policy-denial→`Blocked`).
- Deps: `reqwest` (workspace) + dev `wiremock`; registered in `lib.rs`.
- **Tests 7/7 green** (`cargo test -p execution-core --lib quarry_agent`).
- Auth note: Quarry derives org from the verified JWT `Claims.org_id` (not the `X-Quarry-Org` header) — the bearer token must be a valid JWT.

### Phase B — Quarry agent endpoint (BLUEPRINTED; gated on driver decision)
Files: `quarry-edge/src/{state.rs, main.rs, routes.rs}`.
- AppState += `agent_driver: Arc<dyn quarry_browser::BrowserDriver>` + `agent_runs: Arc<DashMap<String, RunEntry>>` (`RunEntry { session: BrowserSession, ctx: ObservationContext, lease: BrowserLease }`).
- Handlers (protected router, inherit `require_auth`/Claims):
  - `POST /v1/agent/runs` → mint run_id + `BrowserLease`, `agent_driver.acquire(&lease)`, insert RunEntry, emit `AgentStarted`, return `{run_id, lease_id}`.
  - `POST /v1/agent/runs/{id}/step` → get RunEntry (404 + org check), build `AgentActionRequest`, `ObservationRunner{browser,artifacts,events}.execute(&req,&session,&mut ctx)`, return `BrowserObservation`.
  - `DELETE /v1/agent/runs/{id}` → remove + `agent_driver.release(session)`, emit release event.
- Use **`ObservationRunner::execute`** (public, per-action), NOT `AgentLoop` (batch-only).
- **Blockers:** (1) no BrowserDriver/session-map in AppState → add both + wire in `main.rs` AND the `routes.rs` `test_state` literal; (2) `PersistentSessionRegistry` stores metadata not live sessions → use the DashMap; (3) ✅ **driver decided: chromiumoxide (self-host)**.

**Exact signatures (verified, for implementation):**
- `BrowserDriver` trait `quarry-browser/src/lib.rs:34-105` (`acquire`/`release`/`goto`/`content`/`screenshot`/`pdf` required; `click`/`type_text`/`scroll`/… default to `Err(unsupported_action)`). `BrowserSession { lease, inner }` `:107`.
- `ChromiumoxideDriver::new()` — no args (`chromiumoxide.rs:47`), gate `#[cfg(feature="chromiumoxide")]` (`lib.rs:21`); Cargo feature `chromiumoxide = ["dep:chromiumoxide","dep:futures"]`.
- `BrowserLease` `quarry-core/src/lease.rs:7` { lease_id: LeaseKind, profile_id: ProfileKind, session_affinity_key, proxy_affinity: ProxyAffinity{pool,sticky_key}, ttl_s: u32, capabilities: Vec<Capability{Js,Screenshots,Pdf,Actions,Downloads}>, artifact_bucket, org_id }. Mint ids via `kinds::LeaseKind::new()` (`ids.rs`).
- `ObservationRunner { browser: Arc<dyn BrowserDriver>, artifacts: Option<Arc<dyn ArtifactStore>>, events: Option<EventSink> }` (struct-literal, all pub); `ObservationContext { step: u32, current_url, page_hash }`; `execute(&self, &AgentActionRequest, &BrowserSession, &mut ObservationContext) -> QuarryResult<BrowserObservation>` (`observation.rs:70`, increments step + emits ActionStarted/ObservationReady).
- `EventSink::emit(run_id: RunKind, EventType, Value, idempotency_key: String)` (`events.rs:66`); `EventType::{AgentStarted,AgentCompleted,...}`. `EventSink: Clone`.
- Edge helpers: `Envelope::ok(request_id, data)` / `Envelope::<()>::err(&request_id, err)` (`envelope.rs`); `RequestKind::new().to_string()`; `Claims { org_id, user_id, .. }` via `Extension<crate::auth::Claims>` (`auth.rs:43`).
- Anchors to patch: `AppState` `state.rs:15-75`; `AppState{..}` literal `main.rs:~650`; `router()` `routes.rs:77-217` (add routes before `require_auth` layer); `test_state` literal `routes.rs:867-892`; module list `main.rs:25-46` (add `mod agent_routes;`).

**Driver-wiring strategy (Chrome-free CI):** add `quarry-edge` feature `browser-agent-chromium = ["quarry-browser/chromiumoxide"]`. `main.rs`: `#[cfg(feature="browser-agent-chromium")]` → `Arc::new(ChromiumoxideDriver::new())`; `#[cfg(not)]` → a `NoopBrowserDriver` (acquire → `unsupported`). Handler unit tests inject a mock `BrowserDriver` returning canned observations, so default `cargo test` needs no Chrome.

### Phases C–F (depend on B)
- **C:** `run_browser_agent_loop` → async; replace stub act-arm (`browser_agent.rs:303-318`) with `start_run` → loop `step` → `observation_from_wire` → `plan_next_action` → `close_run`; make `tool_bridge::execute_browser_agent` await it.
- **D:** `plan_next_action` calls an LLM (execution-core has no LLM path) — add gRPC to inference-core/model-gateway; structured-output → `AgentAction`.
- **E:** `is_domain_allowed` at dispatch + server-side enforce; `max_cost_usd`; ZDR; `WaitingApproval` human gate; SSRF.
- **F:** integration (mock driver) + 1 real E2E behind the chosen driver.

### ⚠ Pre-existing breakage (NOT P7)
`execution-core/tests/runtime_loop_test.rs` fails to compile — calls `runtime_loop::execute_step` synchronously but it's now `async`. Pre-existing, unrelated to P7. Blocks full `cargo test -p execution-core`; `--lib` is green. Flagged as a separate task.

---

## Goal

Make the autonomous browser loop (observe → LLM-decide → act → observe) real: a Model Plane agent can drive a live browser through Quarry — click/type/navigate/extract across steps — under domain/step/runtime/cost limits, with observations fed back each step. Today both sides have the machinery but **no connection** and **no brain**.

---

## Current state (verified)

### Model Plane — `rust/services/execution-core/src/browser_agent.rs`
- ✅ Plan state machine, terminal states, types: `ActionType` (`:44`: Goto/Click/Type/Extract/Observe/Scroll/Wait), `BrowserAction` (`:77`), `BrowserObservation` (`:88`), `ObservationStatus` (`:69`). 18 tests.
- ✅ Loop shape: `run_browser_agent_loop` (`:293`); limits `check_limits` (`:136`, max_steps + max_runtime_s).
- ❌ **Act-arm is a stub** (`:303-318`): forces `Completed`, returns "awaiting Quarry wiring", never dispatches, never feeds an observation back.
- ❌ **No brain**: `plan_next_action` (`:227`) is deterministic — always emits `Observe` (branches `:269`, `:281`). Zero LLM/inference-core imports in execution-core.
- ❌ **No Quarry client** (no trait, no `Option<Arc<dyn>>`), **no feature flag**.
- ⚠ `is_domain_allowed` (`:149`) defined but never called in the loop; no `max_cost_usd`.
- Caller: `execute_browser_agent` (`tool_bridge/mod.rs:90`) ← tool dispatch `"browser_agent"` (`:31`).

### Quarry-v2
- ✅ **Shared agent contracts** — `quarry-core/src/contracts.rs`: `AgentActionRequest` (`:75`), `AgentAction` enum (`:87`: Navigate/Click/Type/Press/Scroll/Select/Wait/WaitFor/Screenshot/Pdf/Evaluate/Back/GetContent), `AgentConstraints` (`:104`: max_steps, allowed_domains, max_runtime_s, max_cost_usd), `BrowserObservation` (`:18`: run_id, step, url, dom_summary, console/network summaries, policy_denials).
- ✅ **AgentLoop implemented** — `quarry-runtime/src/agent_loop.rs`: enforces domain allowlist + step/runtime/cost budget, emits 7 NATS lifecycle events (`AgentStarted`…`AgentFailed`). `ObservationRunner` (`observation.rs:32,70`) maps `AgentAction`→driver→`BrowserObservation`. **Only callers are tests** (`mock_planner_drives_agent_loop_e2e`).
- ✅ **Real CDP driver** — `quarry-browser/src/chromiumoxide.rs` (click `:192`, type_text `:205`, scroll `:227`, …) behind `--features chromiumoxide` (`Cargo.toml:22`, off by default); remote `browserless`/`browserbase`/`kernel` drivers also present. `BrowserDriver` trait default methods return `Unsupported` (`lib.rs:33`).
- ✅ **Leased sessions / profiles** — `persistent_session.rs` (`PersistentSession:28`, registry `:81`), lease pool, profile CRUD `/v1/profiles*`.
- ❌ **No agent endpoint** in `quarry-edge/routes.rs` — exposes `/v1/scrape|crawl|batch|search|map|extract|answer|profiles`, but nothing to start/step an agent run.
- `deep_research.rs` is NOT this loop (its fetch/extract still `Unsupported`, Phase Q5) — out of scope.

---

## Architecture decision

**Model Plane is the planner; Quarry is the per-step executor.** Forced by the code: Quarry's `AgentLoop` executes *pre-decided* `AgentActionRequest`s (a mock planner drives it in tests); the reasoning/LLM lives in Model Plane (inference-core / model-gateway). Matches `deep_research`'s stated split ("Model Plane owns the loop, Quarry executes per-step capture").

**Flow:** `start run` (Quarry acquires lease+session) → loop[ Model Plane LLM picks `AgentAction` from last `BrowserObservation` → `step` (Quarry `ObservationRunner` executes vs leased session, enforces budget/domain, emits events, returns `BrowserObservation`) ] → `close run` (release lease).

**Contract source of truth = `quarry-core` shared types.** Model Plane adopts/maps to `AgentAction`/`AgentActionRequest`/`BrowserObservation`/`AgentConstraints` instead of its local ad-hoc `ActionType`/`BrowserObservation`.

---

## Phases

### Phase A — Contract alignment (Model Plane)
- Replace (or adapter-map) local `ActionType`/`BrowserObservation` with `quarry-core` shared `AgentAction`/`BrowserObservation`/`AgentConstraints`. Add `max_cost_usd` to config.
- **Exit:** Model Plane compiles against shared contracts; existing 18 tests green or updated.

### Phase B — Quarry agent endpoint (`quarry-edge`)
- Add stateful surface (HTTP, reuse protected router):
  - `POST /v1/agent/runs` → acquire lease+`PersistentSession`, init per-run budget/domain state, return `{run_id, lease_id}`.
  - `POST /v1/agent/runs/{run_id}/step` (body `AgentActionRequest`) → enforce domain+budget, run `ObservationRunner` against the session, emit lifecycle events, return `BrowserObservation`.
  - `DELETE /v1/agent/runs/{run_id}` → release lease/session.
- Wrap per-run budget/domain state (port `AgentLoop` enforcement to the stateful per-step path). Wire a concrete driver (feature-gate decision below). Session reaper for abandoned runs.
- **Exit:** endpoint drives a real browser behind a chosen driver; integration test with mock driver.

### Phase C — Model Plane Quarry agent client
- Add `QuarryAgentClient` trait + `Option<Arc<dyn>>` injected into the loop, gated by env (reuse `QUARRY_EDGE_URL` + new `QUARRY_BROWSER_AGENT_ENABLED`). Mirror the Fetch client auth (`X-Internal-Api-Key`).
- Replace stub act-arm (`:303-318`) with: dispatch `step` → await `BrowserObservation` → push as `last_observation`. Handle session open/close around the loop.
- **Exit:** loop round-trips real observations against a Quarry mock server (wiremock).

### Phase D — The brain (LLM decider)
- `plan_next_action` (`:227`) calls inference-core/model-gateway: prompt = goal + last observation (dom_summary, interactive_elements) → returns next `AgentAction` (or Done). Structured-output schema for the action.
- **Exit:** given a goal, the LLM picks varied real actions (not always Observe); deterministic test with a stubbed model.

### Phase E — Safety & limits
- Call `is_domain_allowed` at dispatch (Model Plane) **and** enforce in Quarry (defense in depth). Wire `max_cost_usd` accounting (LLM + browser cost). ZDR passthrough. Honor `WaitingApproval` state for human-in-the-loop on sensitive actions. SSRF guard (Quarry `tests/ssrf.rs`).
- **Exit:** budget/domain/SSRF violations terminate cleanly with typed reasons.

### Phase F — Verify
- Unit (both sides) + integration (mock driver) + one real E2E behind `--features chromiumoxide` (or remote driver) hitting a fixture site.
- **Exit:** green; documented run instructions.

---

## Risks

| Sev | Risk | Mitigation |
|-----|------|------------|
| HIGH | Agentic browsing = SSRF / data-exfil / runaway loops | Domain allowlist (both sides) + budget + ZDR + approval gate; reuse Quarry SSRF guard |
| HIGH | LLM cost/latency per step (network + model round trip each step) | `max_cost_usd` + `max_steps` + `max_runtime_s`; consider SSE streaming of observations |
| MED | Two `BrowserObservation` shapes diverge → mapping bugs | Phase A adopts shared `quarry-core` contracts as single source |
| MED | Browser driver off by default (`chromiumoxide` feature) → endpoint inert | Driver-choice decision below; CI builds the chosen feature |
| MED | Stateful run/session leaks if `close` missed | Server-side lease reaper + idle timeout (Quarry has session reaping) |

---

## Open decisions (confirm)

1. **Browser driver in prod:** self-hosted `chromiumoxide` (CDP, ops burden) vs `browserless`/`browserbase` (managed, cost). Recommend: chromiumoxide for dev/CI, pluggable remote for prod.
2. **Step transport:** HTTP per-step (reuse `QUARRY_EDGE_URL`) — recommended — vs gRPC streaming vs NATS. Observations/events optionally streamed via SSE.
3. **Contract migration:** fully replace Model Plane local types with `quarry-core` shared types (cleaner) vs thin adapter layer (smaller diff). Recommend full replace in Phase A.

---

## Sequencing & complexity

- Order: A → B → C → D → E → F. A/B parallelizable (different repos) once contracts frozen.
- Complexity: **LARGE** (multi-service, new stateful endpoint, LLM integration, browser infra). Much bigger than the P1–P6 write-path fix.
- Smallest shippable slice: A + B(step only, mock driver) + C + D with a trivial goal → proves the round trip; real driver + safety hardening (E) follow.
