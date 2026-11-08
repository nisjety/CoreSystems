# D2 quarry-run-control: Quarry run admission and control gate (Evaluate deny, takeover authority, reaper and caps)

Author: Sonnet design agent D2, wave W1. Date: 2026-10-15. Tree read: LIVE `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Audit source: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Nothing in the product tree was edited.

Path shorthand (all provenance-tagged with `tools/provenance.sh`): `Q/` = `apps/Ingestion Plane/Quarry-v2/`; `Q/edge/` = `Q/crates/quarry-edge/src/`; `Q/rt/` = `Q/crates/quarry-runtime/src/`; `Q/core/` = `Q/crates/quarry-core/src/`; `FE/` = `apps/Frontend Plane/verevonv3/`; `GW/` = `FE/apps/gateway/src/domains/`; `MP/` = `apps/Model Plane/`.
Tags: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` file this plan would create. Evidence tags: `[READ]` opened and confirmed in this session, `[INF]` reasoned from code, `[UNVERIFIED]` needs a run.

## 1. Items resolved and wave

| Item | Scope in this design | Wave | Verified effort | This design's estimate |
|---|---|---|---|---|
| F-t2-002 | Default-deny `AgentAction::Evaluate` on `/step`, `/interact` and the WebSocket action path; drop `evaluate` and `pdf` from the unwired ModelPlanePlanner prompt | W1 | S | S, 1.5 engineer-days (slice S1) |
| F-t2-013 phase 1 | Quarry derives the initiator from the verified principal, refuses agent actions during HumanTakeover with `USER_TOOK_OVER`, stamps a monotonic control epoch, records initiator in receipts and tab events | W1 (phases 2 and 3 are W2 and out of scope) | S as stated by the audit | **M, 5 to 6 days** (slice S3). The audit's "S" omits the WebSocket and tab paths, the data-model change, the error-code mirrors and the lock-free control state. See section 3, D4 |
| F-t2-006 | Sweeper (idle plus absolute TTL), org-scoped and global concurrent-run caps with typed 429, fleet console fix | W1 | M | M, 6 to 7 days (slice S4) plus S, 0.5 day (slice S5, fleet console hide) |
| Module split | Go/no-go on a behaviour-preserving split of `agent_routes.rs` | n/a | n/a | **GO**, M, 2.5 days (slice S2) |

Total: about 16 to 18 engineer-days in one stream (3.5 weeks). Two streams can run S3 and S4 in parallel after a 0.5-day shared skeleton (section 5).

Honest summary of what changed against the audit cards (details in section 2):
- The F-t2-013 safety gap is real but **narrower than stated today**. A human cannot take over a service-owned (Model Plane) run, because `caller_owns_live_run` returns 404 to any caller whose actor id differs from the run's actor (Q/edge/agent_routes.rs:474-486, 1752-1758). The scenario "a Model Plane run keeps acting after the user takes over" is unreachable until phase 2 delegation exists. Phase 1 therefore (a) makes Quarry authoritative for agent-labelled actions that arrive under a user token (the gateway's `actor: "agent"` path), (b) covers the WebSocket and tab paths that no one checks, and (c) lands the data model phase 2 needs. It is still the right first slice; it must not be sold as closing a live exploit.
- The gateway's WebSocket takeover enforcement the audit cites does not run in production (`[DRIFT]`, section 2).
- The card's lease-`ttl_s` sweeper design would kill every manual browser session after 120 seconds, because the gateway sets `max_runtime_s: 120` on UI-created sessions. The sweeper must key on run class (`[DRIFT]`, section 2 and D7).
- `close_run` leaks the Chromium session whenever any other handler still holds the run's `Arc` (a new finding, section 2).

## 2. Verified current state

### 2.1 Measurement of the hotspot file

`wc -l "Q/edge/agent_routes.rs"` = **3737** lines `[READ]`, 19.6 percent of the 19,054 lines in `quarry-edge/src`. The whole implementation sits inside one `mod enabled` (lines 23 to 3730) behind `#[cfg(feature = "browser-agent")]`; a no-op `agent_router` closes the file (3735). It holds 29 async handlers and helpers, about 40 synchronous helper functions, 21 direct accesses to `state.agent_runs` (`grep -c agent_runs`), and a 265-line inline test module in the middle (1958-2223). Section 3, D11 lists the seams.

### 2.2 F-t2-002 (Evaluate)

| Claim (card or sweep) | Verdict | What the code does |
|---|---|---|
| `sensitive_approval_for_action` falls through for Evaluate, no approval branch (agent_routes.rs:280) | OK `[READ]` | `_ => Ok(None)` at 280. The function begins at 144. It also covers `FrameUploadRef` (191) and `FrameDownloadRef` (239): the card's three-name list is incomplete, minor `[DRIFT]` |
| Both entry points run the same gate | OK, extends the card `[READ]` | REST `step` gate 2276-2292 and WebSocket `execute_ws_action` 3106-3117. `/interact` is an alias of `step` (route at 3666). A fix at one site leaves the other open |
| `ChromiumoxideDriver::evaluate` runs in the main world | OK `[READ]` | `Q/br/chromiumoxide.rs:3003-3017`, `page.evaluate(script)` at 3009 |
| `/step` discards the return value | OK `[READ]` | `Q/rt/observation.rs:542-544`: `self.browser.evaluate(session, script).await?;` The arm is **the only production executor of `AgentAction::Evaluate`**. A mutating script is its only possible use, which settles D1 |
| Model Plane never offers evaluate; gateway denies it | OK `[READ]` | gateway `sanitize_action` denial at `GW/browser.rs:3751-3755`; execution-core planner schemas never list it (S2 verified `agent.rs:3479-3512`, `llm_planner.rs:53-65`) |
| `ModelPlanePlanner` prompt lists `pdf` and `evaluate` | OK `[READ]` | `Q/rt/mp_client.rs:382-383` inside `build_prompt` (345). No production caller of `ModelPlanePlanner::new` (S2) |
| Broker allowlist has no `evaluate` | OK `[READ]` | `MP/go/services/browser-broker/internal/grant/grant.go:32-36`. There is **no script-hash scope** in a broker sensitive grant: scopes are frame, dialog and artifact ids only (grant.go:44-70, `authorizes_sensitive_action` in `Q/rt/grant_validator.rs:50-92`). Binding a script hash needs a new broker scope field, a proto change and Model Plane work |

Evaluate caller audit (required by the brief). Constructors and consumers of `AgentAction::Evaluate` and `Action::Evaluate` across Quarry, Model Plane, gateway and SPA (`grep` of `Evaluate {` / `Action::Evaluate` / `"type":"evaluate"`, excluding `target`, `node_modules`, `.claude`):

| Site | Kind | Production reachable? | Disposition |
|---|---|---|---|
| `Q/rt/observation.rs:542` | executor arm | yes, via `/step`, `/interact`, WS | refuse unconditionally (S1) |
| `Q/rt/observation.rs:337` | mapping `AgentAction` to `Action::Evaluate` | only inside the executor path | leave; arm above refuses first |
| `Q/edge/agent_routes.rs:694, 751` | label maps | yes (labels only) | leave |
| `Q/rt/agent_loop.rs:319` | cost label | no production caller of `AgentLoop` (S2) | leave |
| `Q/rt/action_runtime.rs:380-392` | `ActionRuntime` Evaluate arm calls `driver.evaluate` and keeps the value | **no production caller** (S2) | refuse too, so F-t2-023 cannot inherit an open arm (S1) |
| `Q/rt/mp_client.rs:382-383` | planner prompt text | no production caller | delete `pdf` and `evaluate` lines (S1) |
| `MP/rust/services/execution-core/src/quarry_agent.rs:161` | Model-side wire enum variant, no emitter | wire-only | remove variant in a Model Plane follow-up owned by sibling (b) |
| `Q/core/contracts.rs:821-823`, `tests/contracts.rs:367` | contract type and serde test | n/a | keep (the wire type stays; the server refuses it) |
| `Q/docs/openapi.yaml` `evaluate` discriminator (2787) and `EvaluateAction` (3042) `[M]` | public contract | documented | mark deprecated, state 403 (S1) |
| generated SDKs `sdks/typescript`, `sdks/python` | clients | documented only | regenerate with `sdks/generate.sh` |
| compiled procedures (`browser_procedure.rs`) | `compile_procedure` and `analyze_impact` | no execution | contains no `evaluate` (grep) |
| gateway `sanitize_action` test `GW/browser.rs:5001` | negative test | n/a | keep |
| `Q/br/chromiumoxide.rs:3473-3540`, `kernel.rs:968` | driver-level tests call `driver.evaluate` directly | tests only | unaffected |

Result: **no legitimate production caller of `AgentAction::Evaluate` exists**. The audit's risk ("an extraction flow may rely on mutating evaluate") is not borne out in the tree. External callers using the published SDK are possible `[UNVERIFIED]`; S1 includes a deployment-log check.

### 2.3 F-t2-013 phase 1 (control authority)

| Claim | Verdict | Evidence |
|---|---|---|
| `BrowserControlMode` persisted on the run record, set by `set_browser_session_control` with hard-coded Human initiator | OK `[READ]` | `Q/rt/step_receipts.rs:84-90` (enum), `:39-71` (`AgentRunCheckpoint.control_mode`), `Q/edge/agent_routes.rs:1733-1781` (`entry.control_mode = body.mode` at 1760, `initiated_by: Human` at 1766) |
| `/step` never reads `control_mode` | OK `[READ]` | `step` 2228-2409 has no read. Also absent in `execute_ws_action` (3086-3193), `new_tab` (3240-3345), `select_tab` (3348-3398), `close_tab` (3401-3451) |
| Receipts carry only the run owner; no initiator, no epoch | OK `[READ]` | `StepReceipt` 183-210 has `actor_id` (the run owner, not the caller) and no initiator field. `BrowserTimelineInitiator::Agent` is never constructed (only `Human` at agent_routes.rs:1766) |
| Takeover is enforced only in the gateway from a caller-supplied field | OK for REST, **[CLAIM-FALSE] for WebSocket** | REST: `GW/browser.rs:663-760` reads `ActionBody.actor`, which **defaults to Human** (`GW/browser.rs:246-259`), so a caller that omits `actor` is never blocked. WebSocket: `prepare_client_ws_message` and `prepare_client_control_ws_message` are `#[cfg(test)]` only (`GW/browser.rs:2036-2037`, `:2108`); the production loop `browser_ws_proxy_loop` (1912-1999) is a byte relay. The audit's cite of `browser.rs:2065-2075` as live enforcement is wrong: **in production no component enforces takeover on the WebSocket path, and WebSocket human actions never flip `control_mode`** |
| A Model Plane run keeps acting after a user takeover | **[CLAIM-PARTIAL]** | A user token cannot switch control on a service-owned run: `caller_owns_live_run` (agent_routes.rs:484-486, used at 1752) requires `actor_id == claims.actor_id()`; execution-core's token is an org-scoped service principal (`Q/edge/auth.rs:148-174` normalises `actor_id` to `service:<id>`; execution-core `quarry_agent.rs:554-559`). Conversely an AI run has its own hidden lease (`MP/.../browser_agent.rs:673-690`). So today the only caller pair on one run is "user token with human or agent-labelled actions" |
| Phase 2 is blocked by ownership | OK `[READ]` | same lines. Out of scope here |
| ZDR sessions skip control timeline writes | OK `[READ]` | `record_browser_timeline_event` returns early at 493-495 |
| `Claims::is_service`, `actor_id` exist | OK `[READ]` | `Q/edge/auth.rs:101-103, 144-146` |
| `ErrorCode` is a wire contract mirrored elsewhere | OK, with a precise list (section 4, S3) | Rust enum `Q/core/error.rs:9-70`; Go mirror `Q/pkg/quarrycontracts/envelope.go:36-77` is **already stale** (11 of 17 codes) |

Additional findings from reading the code (none are in the audit):
- **`status_err` always stamps `ErrorCode::BadRequest` into the body** whatever the HTTP status (`agent_routes.rs:1106-1114`). A 404 "agent run not found", a 403 domain denial, a 409 and the step-budget 429 (2294-2295) all carry `"code":"BAD_REQUEST"`. Clients that branch on `code` (model-gateway `grpc.rs:2700`, `tools.rs:1184`) see the wrong class. The new typed errors below use `driver_err` with real codes; fixing `status_err` itself is an optional slice (S6).
- **Receipt `error_code` is `format!("{:?}", code).to_uppercase()`** (`step_receipts.rs:529`), so `TargetRepairRequired` is stored as `TARGETREPAIRREQUIRED`, not `TARGET_REPAIR_REQUIRED`. New code must not copy this; S3 adds `ErrorCode::wire_name()`.
- **`enforce_step_budget` counts human steps**: `ctx.step` is shared (`observation.rs:1015`, S2), and the gateway creates UI sessions with `max_steps: 12` (`GW/browser.rs:485`). The 13th human action on a manual session returns 429. S3 exposes the initiator to the budget function without changing behaviour; F-t2-008 slice 1 flips it.
- `execution-core` maps every Quarry step failure to `PlanStatus::Failed` (`MP/.../browser_agent.rs:742-745` after `AgentClientError::Status{status,body}`, `quarry_agent.rs:485, 680-705`); it never parses the typed code. New typed errors need a Model Plane mapping (section 7).
- Step handler holds the run's `TokioMutex` for the whole action (`step` 2248 to 2408), so viewers (`browser_session`, frames, tabs) stall during a step. Concurrent control changes therefore queue behind the in-flight step today.

### 2.4 F-t2-006 (reaper and caps)

| Claim | Verdict | Evidence |
|---|---|---|
| Nothing reads `ttl_s` | OK `[READ]` | written at `agent_routes.rs:1446` (`max_runtime_s.unwrap_or(120)`) and `:1502`; no reader (S9 grep, `BrowserLease.ttl_s` at `Q/core/lease.rs:13`) |
| Runs leave the registry only through `close_run` | OK `[READ]` | `.remove(&run_id)` at 3622-3626. `RunEntry` (79-102) has no `started_at`, `last_activity` or viewer field |
| No cap on agent runs | OK `[READ]` | `start_run` 1273-1663 has no admission check; `acquire` at 1511-1515; insert 1635-1639. The agent `ChromiumoxideDriver` is one shared browser with no pool (`main.rs:1284-1288`); the `RuntimeLeasePool` sizes 4 and 2 (`main.rs:323, 337` `[M]`) gate scrape rendering only |
| `FleetRunConsole.tsx` fetches `/v1/fleets` directly, no gateway route, Pause PATCHes an unregistered route | OK `[READ]` (cite drift minor: `fetchFleet` is 27-28) | `FleetRunConsole.tsx:27-35` fetch `/v1/fleets/${id}`; PATCH at 146-150; gateway has no `fleets` domain (`ls GW/`); edge routes are POST create, GET, POST members, GET budget only (`Q/edge/fleet_routes.rs:216-222`). Additionally: the registry is process-local (`FleetState = Arc<RwLock<FleetRegistry>>`, fleet_routes.rs:60-64), **no caller in the repo creates fleets**, the SPA route has no nav link (`App.tsx:216` is the only reference), and the SPA expects `{fleet}` or `{data}` while `get_budget` returns a bare struct. The console cannot show real data even with a proxy |
| `quarry-client.ts:127` is evidence | **[CLAIM-FALSE]** | line 127 is `listQuarryRequestQueues`; no fleet call in the file (S2) |
| **New:** a lease-`ttl_s` sweeper would reap manual sessions | **[DRIFT]** blocker | The gateway sends `max_runtime_s: 120` for every UI-created session (`GW/browser.rs:485-487`), and human actions use the same `/step` path. A reaper that honours `ttl_s` would close each manual session two minutes after creation. For service-created runs `max_runtime_s` is optional (`MP/.../browser_agent.rs:659-666`, 0 maps to `None`), so the 120 s default (`agent_routes.rs:1446`) is a lease placeholder, not an absolute deadline |
| **New:** `close_run` can leak the Chromium session | **[DRIFT]** real bug `[READ]` | `close_run` removes the entry, then `Arc::try_unwrap(entry_arc)`; on failure it logs "session will drop on last ref" and does not call `release` (3628-3649). Any concurrent handler that already cloned the `Arc` (a `browser_ws_loop` holds one for its lifetime, 2968-2977; a polling `browser_session` waiting on the mutex holds one) makes `try_unwrap` fail. `BrowserSession` has no `Drop` (`grep "impl Drop"` in quarry-browser finds only `ChromiumoxideDriver`, chromiumoxide.rs:3099), so the pages and browser context stay open. The sweeper must not reuse this body as is |
| ZDR runs are never checkpointed | OK `[READ]` | `agent_routes.rs:1615` guards the checkpoint write; resume refused at 1350-1356 |
| `RunEntry.control_mode` can be the takeover signal for idleness | OK | 92, 1760 |

### 2.5 Uncommitted work in the area (conflict check)

| File | Tag | In-flight diff | Conflict with this work |
|---|---|---|---|
| `Q/edge/agent_routes.rs` | `[C]` | none | none. Clean base for all four slices |
| `Q/rt/step_receipts.rs`, `postgres_step_receipts.rs`, `Q/core/error.rs`, `Q/core/event.rs`, `observation.rs`, `action_runtime.rs`, `mp_client.rs`, `fallback_driver.rs`, `nats_event_bus.rs` | `[C]` | none | none |
| `Q/edge/main.rs` | `[M]` (+188/-105 hunks at live lines 47, 244, 984-1201, 1232, 1395, 1463) | mod list line 47 (`mod model_plane_gate;`), Model Plane gate at 244-253, Data Plane wiring | My touch points are `mod` lines (26-47 region), the agent driver block 1284-1290 and the serve block 1500-1517. Hunks 1395 and 1463 are near the `AppState` literal (1507 region). **Adjacent but not overlapping**; rebase, do not merge blind. Declare the new `mod run_limits;` at a spot away from line 47 |
| `Q/edge/config.rs` | `[M]` (+187 lines across the struct and a new 78-line block at 424-501) | Model Plane switches, rerank | **Overlaps if new keys are added to `EdgeConfig`**. Design avoids it: new limits live in `Q/edge/run_limits.rs` `[NEW]` read from env (D13) |
| `Q/edge/state.rs`, `test_support.rs`, `tests/support/mod.rs` | `[M]` | one-line additions | Designed to need **no new `AppState` field**: `new_runs()` stays zero-argument and returns the new registry type (D9) |
| `Q/rt/lib.rs` | `[M]` (6 lines) | re-exports | additive re-exports only |
| `Q/edge/model_plane_gate.rs` | `[U]` | whole file | S2 asked whether it gates agent paths: **it does not** `[READ]` (Model Plane switches for search, answer, audio only) |
| `Q/docs/openapi.yaml` | `[M]` (+78 lines at 238-250, 4012-4150) | search and schema additions | Agent paths are at 263-345, 2389, 2517 and the evaluate entries at 2787 and 3042: no overlap |
| `FE/src/shared/i18n/errors.ts` | `[M]` (+12) | new strings | additive; same file, different keys |
| `FE/src/app/App.tsx` | `[M]` (+11) | routes | fleet route at 216 not touched by the hunks (S2) |
| `FE/src/features/dashboard/home/KnowledgeComposer.tsx` | `[M]` (10 lines) | not near the `actor: 'agent'` call at 1006 | none |
| `GW/browser.rs`, `GW/main.rs` | `[C]` | none | none |
| `apps/Ingestion Plane/docker-compose.yml` | `[M]` (+50) | switch defaults | only if compose defaults are added for the new env vars; they are optional |
| MP `grpc.rs` (model-gateway) | `[M]` | other | not touched by this design except the optional code-map line |

## 3. Design decisions

Decisions are numbered D1 to D13. "ADR-GATED" marks a decision that needs a user or ADR answer before the *named follow-on work*; none of them blocks the W1 slices, because each slice ships a conservative default (section 8).

### D1. Evaluate: reject outright, not allowlist plus script-hash approval

- Options. (A) Add `evaluate` to the broker's `allowedSensitiveActions` and to Quarry's sensitive-approval path, binding the script hash into the one-shot approval. (B) Refuse `AgentAction::Evaluate` unconditionally at the edge and in the executor until a consumer exists (F-t2-023).
- Recommendation: **B**, with the typed denial below and a defence-in-depth refusal in the executor.
- Rationale. (1) `/step` discards the script's return value (`observation.rs:542-544`), so Evaluate can only be used for its side effects, which is exactly the class the approval model is meant to gate. (2) Option A needs a script-hash scope that the broker does not have (grant.go:44-70 carries only frame, dialog and artifact scopes), a proto change, Model Plane work, and a live BrowserBroker issuance path; sensitive approvals are unreachable in production until F-t2-001 lands (S1 global note, `agent_routes.rs:154-157` fails without a parent run grant). (3) The caller audit in section 2.2 finds no production caller. (4) Codex makes evaluate structurally read-only instead of gating mutation behind consent (card); the isolated-world read-only evaluator belongs to F-t2-023 where a return value is consumed.
- Wire contract: keep `EvaluateAction` in `AgentAction` (no breaking SDK change) and answer **403 `FORBIDDEN`** with `details.reason = "raw_script_denied"`. The future read-only evaluator must be a **new action variant**, never a relaxed `Evaluate`.
- ADR-GATED: topic 6 (script execution model: governed single actions versus capped action lists and a sandboxed script runtime). Decision B is compatible with either outcome and ships without waiting. Only a "yes, mutating script" ruling would revive Option A as a separate item after F-t2-001.

### D2. Initiator: principal-derived, with a tighten-only caller hint

- Rule: `initiator = Agent` if `claims.is_service()`; otherwise `Agent` only when the request carries `initiated_by: "agent"` (REST body, WebSocket message `actor`/`initiated_by`); otherwise `Human`. A service principal can never claim `Human`. A user token can claim `Agent`, which only makes the request **stricter** (it is blocked during takeover). `System` is reserved for the sweeper and server-side transitions.
- Why a hint is needed: the only caller that labels agent actions today, the gateway's `KnowledgeComposer` apply path (`FE/src/features/dashboard/home/KnowledgeComposer.tsx:1006`), reaches Quarry through `run_action` with the user's bearer (`GW/browser.rs:663-760`), so a pure principal rule would classify it as human and weaken the existing behaviour. The gateway forwards `initiated_by: "agent"` when `actor == Agent` (one line, S3).
- Hazard to hand to sibling (b): if F-t2-001's "delegated bearer forwarding" makes Model Plane call Quarry with the *user's* token on agent steps, a pure principal rule would classify those steps as human and skip the gate. Model Plane must therefore send `initiated_by: "agent"` on every step, tab call and WebSocket action regardless of the token type (section 7). Quarry cannot infer it.
- Not an identity claim: the hint is caller-asserted and tighten-only, so it needs no Control Plane work and cannot widen authority. Identity (`principal_type`) is still the Auth Core claim Quarry consumes (auth.rs:99-103).
- Control transitions (`POST .../browser-session/control`) require `Human`. An `Agent` initiator gets 403 `FORBIDDEN` (`details.reason = "control_transition_requires_human"`): an agent can neither take control nor give it back. Today execution-core never calls this route (grep of Model Plane), so nothing regresses.
- ADR-GATED: topic 3 (shared human plus agent sessions) for phase 2. The delegation that lets a user token act on a service-owned run, or a service token attach to a user-owned run, changes how `initiator` is derived. Phase 1 stands without it.

### D3. Control state: lock-free `RunLive` beside the entry, monotonic epoch

- Problem: `control_mode` lives inside `RunEntry`, behind the same `TokioMutex` the step handler holds for the whole action. A takeover request therefore waits for the in-flight step, and the sweeper cannot read activity or viewers without taking the lock.
- Decision: introduce `RunLive` (`Arc`, atomics) stored next to the entry in a `RunHandle { entry: Arc<TokioMutex<RunEntry>>, live: Arc<RunLive> }`. `RunLive` carries: `control` (one `AtomicU64` packing `epoch << 1 | mode_bit`, so mode and epoch are read together), `taken_over_at_ms`, `last_activity_ms`, `viewers`, `in_flight`, plus immutable `org_id`, `class`, `started_at`, `ttl_s`.
- Epoch rules: starts at 0 (or the checkpointed value on resume); increments **only on an actual mode change**; never decrements; stamped on control timeline events, step receipts (the epoch **at admission**), tab events and the 409 detail. A step already past admission may complete after a takeover. The receipt's admission epoch is lower than the later control event's epoch; the audit trail therefore shows "admitted before takeover". Cooperative cancellation of an in-flight CDP call is not attempted (non-goal, noted as risk R2).
- Admission semantics: the check runs **after** the entry lock is acquired and **before** execution, reading `control` once. FIFO mutex fairness plus the atomic flip give: a step queued behind a takeover is refused; a step that already holds the lock completes.
- Optional compare-and-set: `POST .../control` accepts `expected_epoch`; a stale value returns 409 `CONFLICT` (prevents a stale Give back from undoing a newer takeover). Optional, S.
- ZDR: the state is memory only; checkpoint persistence of the epoch and every timeline write stay skipped for ZDR runs (existing guard 493-495).

### D4. F-t2-013 phase 1 is M, not S

The audit sizes phase 1 as S. Counting what is required: lock-free state plus admission pipeline shared by REST, WebSocket and three tab routes (1.5 days); data model (`StepReceipt`, timeline kinds, checkpoint JSONB, SQL migration 0010, Postgres store, in-memory store, five struct literals) (1 day); typed error and its mirrors (0.5 day); gateway hint plus error mapping plus docs and OpenAPI (0.5 day); tests including a Claims-injecting integration harness (1.5 days). Recommendation: re-estimate to **M (5 to 6 days)**. The audit's S holds only for "reject agent principals at `/step` and nothing else", which would leave the WebSocket and tabs open and ship no audit data.

### D5. New typed error: `USER_TOOK_OVER` (409, not retryable)

- Rust: `ErrorCode::UserTookOver` serialises as `USER_TOOK_OVER` (`#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`, error.rs:8). `http_status` 409 (alongside `ActionUnknown | CheckpointLost | TargetRepairRequired`, error.rs:54). `retryable()` false (retrying cannot help until control is given back).
- Envelope: `{ data: null, meta: { request_id }, error: { code, message, details, retryable } }` (`Q/core/envelope.rs:5-52`). `details = { "control_mode": "human_takeover", "control_epoch": N, "taken_over_at": "<RFC 3339>" }`.
- No other new `ErrorCode`. The caps reuse `RATE_LIMITED` (429, retryable, already mapped to `resource_exhausted` by model-gateway `grpc.rs:2703` and `tools.rs:1187`) with `details.reason` in `org_run_cap | org_interactive_run_cap | global_run_cap`. The Evaluate denial reuses `FORBIDDEN`. Reusing existing codes means the Model Plane mapping already exists for two of three conditions.
- The gateway maps `USER_TOOK_OVER` to its existing `browser_human_takeover_active` (409) so the SPA string already exists (`FE/src/shared/i18n/errors.ts:130`).

### D6. Rejected agent steps leave one Skipped receipt per epoch

When an agent step is refused for takeover, append a `StepOutcome::Skipped { reason: "user_took_over" }` receipt (the `ReceiptBuilder::skip` helper exists, step_receipts.rs:538) the first time per control epoch only (`RunEntry.last_rejected_epoch`), non-ZDR only. This lets an auditor see "the agent tried at epoch 3" without letting a looping agent write unbounded rows. Other admission refusals (grant, budget, domain) leave no receipt today; this stays consistent by limiting the exception to the takeover case where the audit value is high.

### D7. Reaper keys on run class, not on `ttl_s`; viewers block idle expiry only

- Classes (derived at `start_run`, stored in `RunLive`): `Agent` when the starting principal is a service (`claims.is_service()`), `Interactive` otherwise (UI-created sessions).
- `Agent`: expire on idle (no step, no control change) after `AGENT_RUN_IDLE_TTL_S` (300; Firecrawl's `activityTtl` default is 300), and on an **absolute** deadline of `explicit max_runtime_s + grace` when the caller supplied `max_runtime_s`, else the ceiling `AGENT_RUN_MAX_TTL_S` (1800). The 120 s default is **not** an absolute deadline. A watcher does not extend the absolute deadline (a budget must bind even if someone watches).
- `Interactive`: **ignore `ttl_s` and the gateway's 120 s**. Expire on idle `INTERACTIVE_RUN_IDLE_TTL_S` (900), on `TAKEOVER_IDLE_TTL_S` (1800) while in `human_takeover` with no viewer, and on an absolute backstop `INTERACTIVE_RUN_MAX_TTL_S` (14400).
- Never reap while a step is in flight, or while `viewers > 0` (open WebSocket, frame stream or event stream), as far as the idle rule goes. The absolute backstops still apply, so a half-open viewer connection cannot hold a session forever. This is the only deliberate departure from the card's "never reap a run a user is watching or has taken over": a backstop is needed or a vanished client leaks a Chromium context permanently. The user may raise the backstops but should not be offered "infinite".
- Expiry preserves resumability: release the Chromium session, append a lifecycle `Expired` timeline event (non-ZDR), **keep the checkpoint `Active`** (the table's CHECK allows only `active|closed`, 0006 migration), emit `agent.expired`. A later `start_run` with `resume_run_id` works exactly as after an edge restart. ZDR runs simply disappear.
- Event name: the card says `run.expired`; the existing agent-run family is `agent.started|completed|failed` (agent_routes.rs:965-978) and `run.*` belongs to crawl runs. Recommend **`agent.expired`** (`EventType::AgentExpired`, serde `agent_expired`, NATS token `agent_expired`). The user can rename before merge; it is a three-line change in four places (section 4, S4).
- ADR-GATED: topic 3, open question 10.2 item 2 ("what idle timeout replaces the 120 s run TTL for a human browsing session?"). The defaults above are conservative placeholders to ratify. The gateway's 12-step and 120-second literals (`GW/browser.rs:485-487`) are not touched by this design; they belong to F-t2-008 and F-t2-011.

### D8. Fix `close_run`'s leak by sharing one `finish_run` body

Extract `finish_run(state, handle, reason: CloseReason)` where `CloseReason` is `Explicit | Expired(reason)`; it locks the entry, sets `entry.closed = true`, removes the registry entry, releases the session **always** (`state.agent_driver.release(entry.session.clone())`, never via `Arc::try_unwrap`), then emits events. Steps and tab calls that were queued behind it check `entry.closed` right after acquiring the lock and return 404 `NOT_FOUND` (`details.reason = "run_not_live"`, plus `resumable` when a non-ZDR checkpoint exists). `close_run` becomes a thin wrapper. This is required by the sweeper (viewers hold `Arc` clones) and independently fixes the leak.

### D9. Run registry with slot accounting; caps counted at `start_run`; no new `AppState` field

- `AgentRuns` stays the exported alias name but becomes `Arc<RunRegistry>`; `new_runs()` keeps its zero-argument signature (returns generous defaults for tests) and `new_runs_with(limits)` is used by `main.rs`. The five callers (`main.rs:1290, routes.rs:1348, state.rs:150, test_support.rs:145, tests/support/mod.rs:90`) are unchanged except `main.rs`.
- `RunRegistry { runs: StdMutex<HashMap<String, RunHandle>>, counters: StdMutex<Counters>, limits: RunLimits }`. `try_reserve(org_id, class) -> Result<RunSlot, CapacityDenied>` increments counters **before** the (slow, awaited) `agent_driver.acquire`, so concurrent `start_run` calls cannot overshoot a cap; `RunSlot` rolls back on drop unless committed with the handle; `remove(run_id)` decrements.
- Both UI-created and service-created runs go through `start_run`, so both are counted with no extra plumbing (the card's "counting UI-created sessions too").
- Caps (all env, `0` = unlimited): global `MAX_CONCURRENT_RUNS` 32; per org `MAX_CONCURRENT_RUNS_PER_ORG` 8; per org interactive `MAX_INTERACTIVE_RUNS_PER_ORG` 6. The last guarantees at least 2 of 8 org slots stay available to agent runs so open browser tabs cannot starve a chat run (a likely failure if one counter served both). Numbers are placeholders: the slice's evidence records Chromium RSS at 8, 16 and 32 contexts and the defaults are set from that measurement. No memory limit is declared for `quarry-edge` in either compose file (grep), which makes an uncapped default dangerous.
- 429 shape in section 4, S4. `Retry-After` header plus `details.retry_after_seconds` (the gateway drops response headers in `proxy_bearer_json`, so the body carries the value).
- Ownership: these are **infrastructure safety limits** in the plane that owns the browser lifecycle. Entitlement- or plan-aware limits (Control Plane quotas, F-t2-022 and F-t2-025) are not designed here.
- ADR-GATED: topic 1 (web policy of record) only for the later "plan-aware per-org cap". Env-driven caps do not depend on it.

### D10. Fleet console: hide it now; make it real only after a fleet producer exists

- Options. (1) Proxy `/v1/fleets` through the gateway plus a cancel route. (2) Hide the route until fleets are real.
- Recommendation: **hide (S5)**; do (1) only if the product wants fleets, as a later M slice. Reasons from section 2.4: no fleet creator exists anywhere in the repo, the registry is process-local (lost on restart), the response shape does not match what the SPA reads, "Pause fleet" would need a cascade policy that does not exist in the edge, `014_fleets.sql` is unreferenced, and the route has no navigation entry. Building the proxy would add an authenticated surface (org scoping, ZDR, cancel semantics) for a feature that cannot be populated.
- Hiding means: remove the `/agents/fleets/:fleetId` route from `App.tsx:216` behind a `VITE_FLEET_CONSOLE` flag defaulting off (or delete the route), keep the component and its test for a later revival, and leave the edge routes (they are harmless and org-scoped).
- If the user chooses (1): gateway domain `GW/fleets.rs` `[NEW]` (merge line in `GW/main.rs` `[C]`, after the `domains::` list near 120-137) using `proxy_bearer_json` like `browser.rs`, plus edge `POST /v1/fleets/:id/cancel` cascading `close` over members; effort M, 4 days, plus a Model Plane producer that actually creates fleets (unowned today).

### D11. Module split: GO, with a precondition

Go/no-go: **GO**, as slice S2, merged before F-t2-008, 011, 014, 021, 032, 036 start editing the file. Measurements and seams:

| Proposed module (under `agent_routes/`) | Moves (current lines) | Approx lines | Touched by |
|---|---|---|---|
| `mod.rs` | feature gate, re-exports, `new_runs`, no-op `agent_router` (3732-3737), `ApiErr`/`status_err`/`driver_err` (1104-1122) | 150 | all |
| `types.rs` | `RunEntry` 77-107, request and response bodies 345-431, 535-577, 817-942, 1029-1102, query-number helpers 981-1027 | 520 | 006, 013, 014 |
| `admission.rs` | `validate_navigation` 109-133, `validate_run_grant` 310-338, `domain_allowed`/`enforce_step_budget` 442-451, ZDR guard 527-533, driver and egress guards 1124-1270 | 330 | 002, 008, 011, 013, 032, 036 |
| `sensitive_approval.rs` | `SensitiveApproval`, `sensitive_approval_for_action`, `validate_sensitive_approval` 135-308 | 175 | 024, 032, 036, 039 |
| `budget.rs` | `browser_action_name` 663-699, receipt cost fns 701-762 | 100 | 008, 014, 021 |
| `control.rs` | `checkpoint_for_entry` 453-472, `caller_owns_*` 474-486, `record_browser_timeline_event` 488-509, `set_browser_session_control` 1729-1781 | 120 | 013, 006 |
| `projection.rs` | session projections 550-661, `browser_session` 1665-1727, timeline mapping 764-815 and handler 1783-1895 | 330 | 013, 014 |
| `lifecycle.rs` | `start_run` 1272-1663, `close_run` 3561-3655 | 480 | 006, 008, 011 |
| `step.rs` | `step` 2225-2409, `execute_ws_action` 3086-3193 | 300 | 002, 008, 011, 013, 021, 036 |
| `receipts.rs` | `list_receipts` 2411-2459, procedure routes 2461-2539 | 130 | 014, 051 |
| `live.rs` | frame, live view, event stream, WebSocket 2541-3084, event helpers 944-979, frame helpers 1897-1956 | 620 | 014, 054 |
| `tabs.rs` | tab routes 3202-3451, devtools and egress receipts 3453-3559 | 360 | 013, 054 |
| `router.rs` | `agent_router` 3657-3729 | 75 | all |
| `tests.rs` | inline tests 1958-2223 (split further per module in later slices) | 265 | all |

Conflict-reduction rationale. The nine items that touch the file (F-t2-002, 006, 008, 011, 013, 014, 021, 032, 036) cluster on three function bodies: `step` (2228-2409; six items), `start_run` (1273-1663; three items) and `validate_run_grant` (314-338; three items). A file split alone moves these bodies into `step.rs`, `lifecycle.rs` and `admission.rs`, so same-function conflicts remain. The real saving comes in two parts: (1) hunks to unrelated code (timeline, tabs, live frames, projections, procedures: about 60 percent of the file) stop colliding, and (2) **S3 turns `step`'s inline gate chain into a single `admit_action` pipeline** (one function per gate, one ordered call list), so each later workstream adds a gate function in its own module and one line to the list rather than editing a 180-line body. The REST and WebSocket paths currently duplicate that chain (2257-2309 versus 3094-3127, which already differ in order and error mapping); the pipeline removes the duplication that made the audit find the WebSocket gap in F-t2-002.

The split itself is behaviour-preserving: move code, adjust visibility (`fn` to `pub(super)`), add `mod` lines, change nothing else. It does not unify REST and WebSocket (the order and error differences would be a behaviour change). Cost drivers: about 45 private functions need `pub(super)`; the inline tests rely on `use super::*` and on private access, so they move to `tests.rs` with `use super::*` of `mod.rs`, which re-exports the moved items with `pub(super) use`; `#[allow(clippy::items_after_test_module)]` (1961) disappears. `main.rs` and `lib.rs` both declare `mod agent_routes;` (main.rs:26, lib.rs:1), so a directory module must compile as both a binary-crate and a library-crate module (it does, as nothing is crate-root-specific). Verification: `cargo test -p quarry-edge` before and after with identical test names, plus a diff of `cargo doc`'s public item list for `quarry_edge::agent_routes`.

Preconditions: land S1 first (so the Evaluate fix is not caught mid-move; it is a 60-line change), announce a two-working-day freeze on `agent_routes.rs`, and let no other PR touch the file until S2 merges. If a freeze cannot be enforced, the no-go fallback is to skip S2 and accept that S3 and S4 carry the conflict cost themselves; the design still works, with more rebase work in W2.

### D12. Keep `AgentAction::Evaluate` out of scrape-time actions

F-t2-023 will wire `ActionRuntime` into `/v1/scrape` `actions[]`. S1 makes `ActionRuntime` refuse `Action::Evaluate` so the public scrape contract cannot inherit the arm. Flagged for the F-t2-023 designer.

### D13. Config location: `run_limits.rs` read from env, fail loud

New limits are parsed by `RunLimits::from_env()` in `Q/edge/run_limits.rs` `[NEW]`, with names under the `QUARRY_EDGE__` prefix so a later fold into `EdgeConfig` is a mechanical move. Reasons: `config.rs` `[M]` carries a 187-line uncommitted diff (conflict), and `EdgeConfig` silently falls back to defaults on a deserialize error (`config.rs` loader, S9 section 2.3), which is wrong for safety limits. `from_env` returns an error on an unparsable value and `main.rs` aborts boot, matching the production refusals already in `main.rs:128-215`. Follow-up (after the uncommitted config work commits): move the struct into `EdgeConfig`.

## 4. Slices (PR-sized, in build order)

Build order: **S1** (Evaluate deny) then **S2** (module split) then **S3** (control authority) then **S4** (registry, caps, sweeper), with **S5** (fleet console hide) independent and **S6** optional. Reasoning: S1 is a 60-line independent security fix that should not wait for a refactor; S2 must merge before the W2 workstreams open the file; S3 creates the admission pipeline and `RunLive` that S4 builds on.

Commands used throughout (from CLAUDE.md and the S9 sweep): from `apps/Ingestion Plane/Quarry-v2`: `cargo test -p quarry-core`, `cargo test -p quarry-runtime`, `cargo test -p quarry-edge`, and the CI-equivalent `cargo check --workspace && cargo test --workspace` (`.github/workflows/quarry-v2-ci.yml:201-205` `[C]`). The Windows host cannot build these crates (boring-sys2 needs cmake, clang, perl, go, protoc): run them in the `rust:1` container with cache volumes and `bash -c` (not `bash -lc`) per the memory note `quarry-rust-tests-in-docker`; a combined `cargo test -p quarry-core -p quarry-runtime -p quarry-edge` takes more than 10 minutes. Go: `cd "Q/pkg/quarrycontracts" && go test ./...`. Gateway: `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`. SPA: `cd "FE" && pnpm lint && pnpm typecheck && pnpm test` (restore `pnpm-lock.yaml` afterwards if run inside a worktree, memory `verevonv3-worktree-verification`). Stack: `cd "apps/Ingestion Plane" && make up` for the edge, with the env-file chain from memory `ingestion-plane-deploy-recipe`; the Makefile passes only two env files, so interpolations may be unset `[UNVERIFIED]`.

---

### S1. F-t2-002: hard-deny `AgentAction::Evaluate` (effort S, 1.5 engineer-days)

**Goal.** Quarry refuses raw page script on every entry point and in every executor, with a typed denial and no new broker or proto surface (D1).

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `Q/edge/agent_routes.rs` | new `fn deny_raw_script(action: &AgentAction) -> QuarryResult<()>` (near 1180) | returns `QuarryError::new(ErrorCode::Forbidden, "raw page script execution (evaluate) is not permitted; use typed browser actions").with_details(json!({"reason":"raw_script_denied","denied_action":"evaluate"}))` for `AgentAction::Evaluate`; `Ok(())` otherwise. Logs `tracing::warn!(target = "quarry_edge::agent", org_id, run_id, action = "evaluate", "raw script action denied")` and **never** the script text (scripts may contain secrets) | [C] |
| `Q/edge/agent_routes.rs` | `step` (after the capability check at 2269-2270) and `execute_ws_action` (after 3102) | call `deny_raw_script(&body.action)` / `(&action)` **before** `sensitive_approval_for_action`, the one-shot insert and the budget check, so a denied action consumes nothing | [C] |
| `Q/rt/observation.rs` | `ObservationRunner::execute`, arm `AgentAction::Evaluate` (542-544) | replace the driver call by `return Err(QuarryError::new(ErrorCode::Forbidden, ...).with_details(same details))` | [C] |
| `Q/rt/action_runtime.rs` | `Action::Evaluate` arm (380-392) and the 12-action test (about 485) | refuse with the same error; adjust the test to 11 actions plus a new refusal test (D12: keeps F-t2-023 from inheriting an open arm) | [C] |
| `Q/rt/mp_client.rs` | `ModelPlanePlanner::build_prompt` (345-395) | delete the `pdf` (382) and `evaluate` (383) example lines; update any prompt-content assertion in the two tests at about 341 and 365 `[UNVERIFIED which lines assert]` | [C] |
| `Q/docs/openapi.yaml` | `AgentAction` discriminator (2787), `EvaluateAction` (3042), `/step` and `/interact` 403 text (301-345) | mark `EvaluateAction` `deprecated: true` with "always answered 403 FORBIDDEN, reason raw_script_denied"; extend the 403 description | [M] (no overlap with the in-flight hunks 238-250, 4012-4150) |
| `Q/sdks/typescript/src/models/*`, `Q/sdks/python/quarry_client/models/*` | generated | regenerate with `Q/sdks/generate.sh`; commit only if the diff is limited to the deprecation text | [C] |
| `MP/rust/services/execution-core/src/quarry_agent.rs` | `AgentAction::Evaluate { script }` (161) | **follow-up in a separate Model Plane PR owned by sibling (b)**: remove the variant (no emitter) | [C] |

**API / contract changes.** No new route, no new `ErrorCode`, no proto, no migration. The 403 body is `{ "data": null, "meta": {"request_id": "..."}, "error": { "code": "FORBIDDEN", "message": "...", "details": {"reason": "raw_script_denied", "denied_action": "evaluate"}, "retryable": false } }`. WebSocket: the existing error frame `{"type":"error","message":"Forbidden: ..."}` (`browser_ws_loop` 3053-3058).

**Feature flag and rollout.** None: the action has no production caller (section 2.2). Rollout is a normal edge image deploy. Before merging, search the edge's last seven days of logs and the Model Plane's `browser.act` traffic for `"type":"evaluate"` bodies `[UNVERIFIED: needs log access]`; any hit means an external SDK user, who gets a clear 403. Rollback is a revert.

**Tests**
- Unit (inline module in `agent_routes.rs`, pre-split): `deny_raw_script_refuses_evaluate_only`; `deny_raw_script_error_is_typed_and_never_echoes_the_script`.
- Unit (`Q/rt/observation.rs` tests, using the existing counting double at about 2829): `evaluate_action_is_refused_without_calling_the_driver` (driver `evaluate` call count stays 0).
- Unit (`Q/rt/action_runtime.rs`): `evaluate_action_is_refused` and the adjusted 11-action test.
- Integration `Q/crates/quarry-edge/tests/agent_evaluate_denied.rs` `[NEW]`: build `agent_router().with_state(state).layer(Extension(claims))` (the router is `pub`, so no JWKS is involved), insert a `RunEntry` literal into `state.agent_runs` (all fields are `pub`; the literal pattern is at `agent_routes.rs:2064`), POST `/v1/agent/runs/{id}/step` and `/interact` with `{"action":{"type":"evaluate","script":"document.title='x'"}}` and expect 403, `error.code == "FORBIDDEN"`, `details.reason == "raw_script_denied"`, `ctx.step` unchanged, **zero receipts** in `InMemoryStepReceiptStore`, and zero `evaluate` calls on a recording driver; repeat over the WebSocket handler `execute_ws_action` through a test seam (`pub(crate)`).
- Regression: gateway `sanitize_action_denies_raw_script_evaluation` (`GW/browser.rs:5001`) must stay green.
- Commands: `cargo test -p quarry-runtime observation action_runtime mp_client`; `cargo test -p quarry-edge agent_evaluate_denied agent_routes`; `cargo test -p quarry-core contracts`; CI `cargo test --workspace`.

**Definition of done.** All four entry points (REST step, `/interact`, WebSocket, executor) refuse; the denial consumes no budget and writes no receipt; no log line contains script text; OpenAPI and SDK text updated; the Model Plane variant removal is filed with sibling (b).

**Deployment evidence required.** (1) Rebuild the quarry-edge image (`docker compose ... up -d --build quarry-edge` with the deploy-recipe env chain) and show the new image id. (2) Against the running edge, using a user-principal Quarry token obtained the way the gateway does (`get_audience_token(..., "quarry")`, record how in the PR `[UNVERIFIED mint command]`): `POST /v1/agent/runs` (real Chromium), `POST .../step {"action":{"type":"navigate","url":"https://example.com"}}`, then `POST .../step {"action":{"type":"evaluate","script":"document.title='pwned'"}}`. Observable assertions: HTTP 403; `error.details.reason == "raw_script_denied"`; a following `{"type":"get_content"}` step returns the page title `Example Domain`; the receipts list (`GET .../receipts`) contains exactly the navigate and get_content receipts; `docker logs quarry-edge` shows the warn line with no script text. (3) Repeat over `frames/ws` with an action frame and show the `error` frame.

**Risks and mitigations.** External SDK callers (log check above, clear error text). A future legitimate need for read-only evaluate (F-t2-023 adds a new action; do not relax this one). Prompt-test brittleness (list affected asserts in the PR).

---

### S2. Module split of `agent_routes.rs` (effort M, 2.5 engineer-days, go/no-go: GO, D11)

**Goal.** Move code into `agent_routes/` modules with no logic change so the W2 and W3 workstreams stop colliding.

**File-level change list.** `Q/edge/agent_routes.rs` `[C]` becomes `Q/edge/agent_routes/mod.rs` `[NEW path]` plus `types.rs`, `admission.rs`, `sensitive_approval.rs`, `budget.rs`, `control.rs`, `projection.rs`, `lifecycle.rs`, `step.rs`, `receipts.rs`, `live.rs`, `tabs.rs`, `router.rs`, `tests.rs` (all `[NEW]`), with the line ranges of the D11 table. `mod.rs` keeps the `#[cfg(feature = "browser-agent")]` gate, `pub use` of every item the rest of the crate uses today (`AgentRuns`, `new_runs`, `agent_router`, `RunEntry`, the pub request and response types), and the no-op `agent_router`. `Q/edge/lib.rs` `[M]` and `Q/edge/main.rs` `[M]` declare `mod agent_routes;` unchanged (a directory module resolves from the same `mod` line, so **neither file changes**). No other file changes.

**API / data-model / migration changes.** None. This is a pure move; public paths `quarry_edge::agent_routes::*` stay identical.

**Feature flag and rollout.** None. Rollout is a normal merge; rollback is a revert. A two-working-day freeze on `agent_routes.rs` is announced before the branch is cut.

**Tests.** No new tests; the gate is "same tests, same names, same results".
- `cargo test -p quarry-edge -- --list` captured before and after must be identical in test names (diff of the sorted list), and `cargo test -p quarry-edge` green.
- `cargo doc -p quarry-edge --no-deps` public-item list for `agent_routes` identical before and after.
- `cargo clippy -p quarry-edge --all-targets -- -D warnings` (the `make lint` command, not CI-gated per S9) clean for the touched module, since `items_after_test_module` is no longer needed.
- Commands as in the preamble.

**Definition of done.** Largest resulting file under about 650 lines (`live.rs`); `git diff --stat -M` shows renames and moves only; reviewers can verify with `git diff --color-moved=dimmed-zebra`; the freeze is lifted by merging.

**Deployment evidence.** Because behaviour is unchanged, evidence is the existing integration surface: rebuild the edge image, start the stack, run one end-to-end browser flow through the Verevon UI (create session, navigate, screenshot frame, close) and show the same receipts and timeline as before the split; `make test-endpoints` green (`apps/Ingestion Plane/Makefile`).

**Risks and mitigations.** Visibility errors (`pub(super)` sweep: compiler finds them all). A hidden coupling through the shared `use` block at the top of `mod enabled` (23-74): each module gets its own `use` list; reviewers diff `use` lines. Merge races: the freeze. **Do not** unify REST and WebSocket admission here (order and error differences make it a behaviour change; that happens in S3 with tests).

---

### S3. F-t2-013 phase 1: control authority, initiator and epoch inside Quarry (effort M, 5 to 6 engineer-days)

**Goal.** Quarry decides who may drive a run (D2, D3): derives the initiator from the verified principal (plus tighten-only hint), refuses agent actions during human takeover with `USER_TOOK_OVER`, stamps a monotonic epoch, and records initiator and epoch on receipts, control events and tab events. One shared admission pipeline serves REST, WebSocket and the tab routes.

**File-level change list** (module names are post-S2; if S2 is skipped the same symbols live in `agent_routes.rs` at the ranges in section 2)

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `Q/edge/agent_routes/run_live.rs` | `RunLive`, `RunHandle`, `StepInitiator`, `InitiatorHint`, `derive_initiator(claims, hint)`, `ControlSnapshot`, `RunClass` | new; `RunLive` holds `AtomicU64 control` (`epoch << 1 | mode_bit`), `taken_over_at_ms`, immutable `org_id`, `actor_id`, `class`, `started_at`; `set_mode(mode) -> (changed, epoch)` uses `fetch_update`, increments only on change; `restore(epoch, mode)` for resume | [NEW] |
| `Q/edge/agent_routes/types.rs` | `RunEntry`; `AgentRuns` | add `closed: bool`, `last_rejected_epoch: Option<u64>`; `AgentRuns` becomes `Arc<StdMutex<HashMap<String, RunHandle>>>` in S3 (S4 swaps it for the registry); `control_mode` stays as the mirror used by projections and checkpoints | [NEW] (moved from [C]) |
| `Q/edge/agent_routes/admission.rs` | new `admit_action(state, handle, entry, AdmitInput) -> QuarryResult<Admitted>` | the **single ordered gate chain**: (1) `closed` then 404 `run_not_live`; (2) control gate (agent initiator and `HumanTakeover` then `USER_TOOK_OVER`); (3) grant revalidation; (4) driver capabilities; (5) ephemeral tier; (6) `deny_raw_script` (S1); (7) sensitive approval and one-shot insert; (8) step budget (`enforce_step_budget(entry, initiator)`, behaviour unchanged for now); (9) navigation domain; (10) navigation security. Pinned by test `admission_order_is_pinned` | [NEW] |
| `Q/edge/agent_routes/step.rs` | `step`, `execute_ws_action` | derive initiator from `claims` and `StepBody.initiated_by`; call `admit_action`; stamp the receipt with `initiated_by` and the **admission epoch**; on a takeover refusal append one Skipped receipt per epoch (D6); wrap execution in an `InFlightGuard` | [NEW] (moved from [C]) |
| `Q/edge/agent_routes/live.rs` | `live_frame_ws`, `browser_ws_loop` | pass the claims-derived default initiator into the loop; per message use `actor`/`initiated_by` (hint can only tighten) | [NEW] (moved) |
| `Q/edge/agent_routes/tabs.rs` | `new_tab`, `select_tab`, `close_tab` | run `admit_action` (surface `Tab*`); record `initiated_by` and `control_epoch` in the `Tab` timeline event | [NEW] (moved) |
| `Q/edge/agent_routes/control.rs` | `set_browser_session_control`, `record_browser_timeline_event`, `checkpoint_for_entry` | initiator must be `Human` (else 403, reason `control_transition_requires_human`); flip `RunLive` **first** (enforcement is immediate, D3), then lock the entry for the timeline and checkpoint write and the projection response; optional `expected_epoch` compare-and-set; checkpoint now stores `control_epoch`; `Control` event carries the epoch and the derived initiator instead of hard-coded `Human` | [NEW] (moved) |
| `Q/edge/agent_routes/lifecycle.rs` | `start_run` | build `RunLive` (class from `claims.is_service()`, epoch and mode restored from the checkpoint on resume), insert a `RunHandle` | [NEW] (moved) |
| `Q/edge/agent_routes/projection.rs` | `BrowserSessionProjectionData`, `BrowserTimelineItemDetail` | additive fields `controlEpoch` (projection), `initiatedBy` and `controlEpoch` on `Action`, `Tab`, `Control`; `Lifecycle` gets `initiatedBy` | [NEW] (moved) |
| `Q/rt/step_receipts.rs` | `StepReceipt` (183-210), `ReceiptBuilder` (453-556), `AgentRunCheckpoint` (39-71), `BrowserTimelineEventKind` (138-157) | add `initiated_by: Option<BrowserTimelineInitiator>` and `control_epoch: Option<u64>` to `StepReceipt` (`#[serde(default, skip_serializing_if = "Option::is_none")]`); `ReceiptBuilder::initiated_by(...)`, `control_epoch(...)`; `AgentRunCheckpoint.control_epoch: u64` (`#[serde(default)]`); `Control { mode, initiated_by, #[serde(default)] control_epoch }`, `Tab { operation, tab_id, active_tab_id, #[serde(default)] initiated_by: Option<..>, #[serde(default)] control_epoch: Option<u64> }`; struct-literal fixes at 501, 519, 540, 742 and `browser_procedure.rs:231` | [C] |
| `Q/rt/postgres_step_receipts.rs` | `append` (INSERT at 143), `list_for_org`/`list_for_actor`/`get` (SELECT lists at 181, 208, 225), `decode` (34-117) | bind and read the two new columns | [C] |
| `Q/crates/quarry-runtime/migrations/0010_step_receipt_initiator_epoch.sql` | table `quarry_step_receipts` | `ALTER TABLE ... ADD COLUMN IF NOT EXISTS initiated_by TEXT CHECK (initiated_by IN ('human','agent','system'))`; `ADD COLUMN IF NOT EXISTS control_epoch BIGINT CHECK (control_epoch >= 0)`. Nullable, no backfill (legacy rows stay unknown). Next free number is 0010 (0001 to 0009 exist) | [NEW] |
| `Q/core/error.rs` | `ErrorCode` (9-70) | add `UserTookOver`; `http_status` 409; add `wire_name(self) -> &'static str` returning the SCREAMING_SNAKE name (use it for new receipts; the old `format!("{:?}").to_uppercase()` at `step_receipts.rs:529` is left as is to avoid rewriting stored values, listed in risks) | [C] |
| `Q/core/tests/contracts.rs` | error tests (40-60) | add `ErrorCode::UserTookOver.http_status() == 409`, `!retryable()`, serde name `USER_TOOK_OVER` | [C] |
| `Q/rt/fallback_driver.rs` | `error_code_short` (587) | add `UserTookOver => "user_took_over"` (exhaustive match: compile error otherwise) | [C] |
| `Q/pkg/quarrycontracts/envelope.go` | `ErrorCode` constants, `HTTPStatus` (36-77) | add `CodeUserTookOver = "USER_TOOK_OVER"` (409) and the six codes already missing (`ACTION_UNKNOWN`, `CHALLENGE_DETECTED`, `RUNTIME_NOT_READY`, `CHECKPOINT_LOST`, `TARGET_REPAIR_REQUIRED` with their statuses) so the mirror is whole; `go test ./...` | [C] |
| `Q/docs/openapi.yaml` | `AgentStepRequest` (2517), `/step` 409 text (301-345), `/interact` | add optional `initiated_by` (`human` or `agent`, "only `agent` has effect for user tokens"); document 409 `USER_TOOK_OVER` | [M] |
| `Q/docs/CONTRACTS.md` | error section | one paragraph for the new code | [C] |
| `GW/browser.rs` | `run_action` (663-760) | add `"initiated_by": "agent"` to the step body when `actor == Agent` (the existing pre-check stays as early UX rejection); `forward_quarry_failure` (3873): translate Quarry `USER_TOOK_OVER` to the existing gateway code `browser_human_takeover_active` (409) | [C] |
| `FE/src/shared/api/browser-client.ts` | timeline item types | optional `controlEpoch`, `initiatedBy` (additive) | [C] |
| `MP/rust/services/model-gateway/src/grpc.rs` (2700), `tools.rs` (1184) | `QuarryError::Typed` code map | add `"USER_TOOK_OVER" => Status::failed_precondition(message)`; **owned by sibling (b)**, not required for the slice | [M] / [C] |
| `Q/edge/main.rs` | (none) | no flag plumbing needed: the enforcement flag below is read by `run_limits::takeover_enforced()` on first use | [M] untouched |

**API / data-model changes (exact names)**
- `POST /v1/agent/runs/{run_id}/step` and `/interact` request: `{ "action": {...}, "instruction": null, "extraction_profile": null, "initiated_by": "agent" }` (new optional field, snake_case, lowercase enum `human | agent`).
- WebSocket client message: `{ "type": "action", "action": {...}, "instruction": "...", "actor": "agent" }`; `actor` accepts alias `initiated_by`.
- `POST /v1/agent/runs/{run_id}/tabs` body `NewTabBody`: optional `initiated_by` (camelCase `initiatedBy` is accepted because the struct is `rename_all = "camelCase"`).
- `POST /v1/agent/runs/{run_id}/browser-session/control` request: `{ "mode": "human_takeover" | "agent_control", "expected_epoch": 3 }` (the second field optional, `u64`). Response: the existing `BrowserSessionProjectionData` plus `controlEpoch` (u64).
- 409 body: `{ "data": null, "meta": {"request_id": "..."}, "error": { "code": "USER_TOOK_OVER", "message": "a human has taken over this browser session; agent actions are refused until control is given back", "details": {"control_mode": "human_takeover", "control_epoch": 3, "taken_over_at": "2026-10-15T09:12:44Z"}, "retryable": false } }`.
- Receipt JSON (`GET .../receipts`): additive `"initiated_by": "human" | "agent" | "system"`, `"control_epoch": 3`.
- Timeline items (`GET .../browser-session/timeline`): additive `"initiatedBy"`, `"controlEpoch"` (camelCase, the struct is `rename_all = "camelCase"`).
- Checkpoint JSONB `state`: additive `control_epoch`. Timeline event JSONB: additive fields inside the tagged `event`. **No SQL beyond migration 0010.**
- **Intentional wire change inside the pipeline unification**: REST budget exhaustion and out-of-domain navigation currently return a body `code` of `BAD_REQUEST` (via `status_err`, 2294-2305). Through `QuarryError` they become `RATE_LIMITED` (429, same HTTP status) and `SECURITY_BLOCKED` (403, same HTTP status), matching what the WebSocket path already returns (3118-3127). HTTP statuses do not change; clients that branched on the `BAD_REQUEST` code string for those two cases must be checked (gateway forwards bodies as is; `errors.ts` keys are gateway codes). Listed in risks.

**Where `ErrorCode` is mirrored (complete list).** (1) `Q/core/error.rs` enum, `retryable`, `http_status`; (2) `Q/core/tests/contracts.rs:40-60, 76` golden asserts; (3) `Q/rt/fallback_driver.rs:587` exhaustive string map; (4) Go `Q/pkg/quarrycontracts/envelope.go:36-77` (already stale); (5) `Q/docs/CONTRACTS.md` and `Q/docs/openapi.yaml` (the OpenAPI `ErrorBody.code` is a free string, so no enum to edit, but the 409 text and the SDK docs mention codes); (6) generated SDK docs (`Q/sdks/*/docs/AgentAction.md` mention `TARGET_REPAIR_REQUIRED` in prose); (7) Model Plane `model-gateway/src/grpc.rs:2700` and `tools.rs:1184` string maps (default arm is `Status::internal`, so an unmapped code degrades to internal); (8) Model Plane `go/pkg/quarry/client.go` and `execution-core/src/quarry_agent.rs:485, 680-705` pass `code` and status through as strings, so they need behaviour (pause on 409 `USER_TOOK_OVER`) rather than an enum entry; (9) the gateway `forward_quarry_failure` (pass-through) and `FE/src/shared/i18n/errors.ts:130` (the translated gateway code). Nothing in the SPA or gateway Rust matches Quarry's code strings today (grep of `FE/src` and `apps/gateway/src` for `SECURITY_BLOCKED`, `RATE_LIMITED`, `TARGET_REPAIR_REQUIRED`: no hits).

**Feature flag.** `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` (default `1`). `0` turns the control gate into log-only (`would_reject` warn line, counter) for emergency rollback; epoch and initiator stamping stay on. Parsed in `Q/edge/run_limits.rs` `[NEW]` (introduced in S3 with only this field, extended in S4). Rollout: deploy with the default; rollback by env flip without an image change.

**Tests**
- Unit (`run_live.rs`): `derive_initiator_service_principal_is_agent_even_with_human_hint`; `derive_initiator_user_is_human_unless_hint_tightens`; `derive_initiator_dev_bypass_stub_user_is_human`; `control_epoch_increments_only_on_change`; `control_epoch_is_monotonic_under_concurrent_flips` (8 threads, 1000 flips, final epoch equals the number of observed changes); `restore_continues_the_checkpointed_epoch`.
- Unit (`admission.rs`): `agent_step_is_refused_during_takeover_with_user_took_over`; `human_step_is_allowed_during_takeover`; `agent_hint_on_user_token_is_refused_during_takeover`; `admission_order_is_pinned` (the 10-gate order, including `closed` before everything and the control gate before grant revalidation so a refused agent never calls the broker); `takeover_refusal_consumes_no_budget_and_no_approval_grant`.
- Unit (`control.rs`): `agent_cannot_change_control_mode`; `stale_expected_epoch_returns_conflict`; `control_event_records_derived_initiator_and_epoch`.
- Unit (`step_receipts.rs`): `receipt_roundtrips_initiator_and_epoch`; `legacy_receipt_without_new_fields_deserialises`; `legacy_checkpoint_without_control_epoch_defaults_to_zero`; contracts test for `USER_TOOK_OVER`.
- Integration `Q/crates/quarry-edge/tests/agent_run_control.rs` `[NEW]` (Claims injected with `Extension`, recording driver extended from the observation test double): service-principal claims and user claims on one run; takeover then agent step gives 409 with `details.control_epoch`; give back then agent step succeeds and its receipt has `initiated_by == "agent"` and `control_epoch == 2`; WebSocket action with `actor: "agent"` is refused; `new_tab` by an agent is refused during takeover; the Skipped receipt appears exactly once per epoch; a ZDR run enforces the gate but writes no receipt, no timeline event, no checkpoint (assert the in-memory store is empty).
- Postgres (`#[ignore]`, `TEST_DATABASE_URL`, pattern from `Q/rt/postgres_queue.rs:435`): `postgres_receipt_roundtrips_initiator_and_epoch` and migration 0010 idempotency `[UNVERIFIED pattern]`.
- Go: `cd "Q/pkg/quarrycontracts" && go test ./...` with a new `TestErrorCodeStatusMirror`.
- Gateway: `run_action_forwards_agent_initiator_to_quarry` and `quarry_user_took_over_maps_to_browser_human_takeover_active` in `browser.rs` tests (the existing websocket tests at 5571, 5631, 5686 stay).
- Commands: `cargo test -p quarry-core contracts`; `cargo test -p quarry-runtime step_receipts fallback_driver`; `cargo test -p quarry-edge agent_run_control agent_routes`; CI `cargo test --workspace`; `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" browser`.

**Definition of done.** Every action surface (REST step, `/interact`, WebSocket, tab open, tab select, tab close) goes through `admit_action`; a user-token request labelled agent and a service-principal request are both refused with `USER_TOOK_OVER` during takeover and succeed after give back; receipts, control events and tab events carry initiator and epoch (non-ZDR); migration 0010 applies on boot and old binaries keep working against the new columns; every mirror in the list above is updated or has a filed owner; OpenAPI documents the field and the 409.

**Deployment evidence required.** (1) Rebuild edge and gateway images; at edge boot show migration 0010 applied (`psql ... "\d quarry_step_receipts"` lists `initiated_by` and `control_epoch`). (2) Human flow in the real UI: open a Verevon browser session, click Take over, then drive the "model suggestion apply" path (`KnowledgeComposer`, `actor: "agent"`). Observable: the SPA shows the existing takeover message; independently, a direct `curl` to the edge (`127.0.0.1:8082`) with the same user token and `{"initiated_by":"agent"}` returns **409 `USER_TOOK_OVER`** (proves Quarry, not the gateway, enforces), then `GET .../browser-session/timeline` shows the `control` item with `controlEpoch: 1`, `initiatedBy: "human"`, and a `skipped` action item once. (3) Give back; the same agent-labelled call succeeds; `GET .../receipts` shows that receipt with `"initiated_by":"agent","control_epoch":2`. (4) Real AI run: start a chat browser run (service principal, execution-core); its receipts show `"initiated_by":"agent","control_epoch":0`, and the user's own session receipts show `"human"`. (5) **Honest limit**: a stack-level proof that a *service principal* is refused during takeover is not possible before phase 2, because the user cannot flip control on a service-owned run; that case is proven by the Claims-injecting integration test, and the evidence file says so.

**Risks and mitigations**
- R1. The pipeline unification changes two error-body codes on REST (section 4, API changes). Mitigation: HTTP statuses are unchanged; document in the PR and openapi; grep `BAD_REQUEST` consumers (none found in `FE/src` or gateway Rust).
- R2. An in-flight action can complete after takeover (no CDP cancellation). Mitigation: documented semantics (D3); receipts show the admission epoch; bounded by driver action timeouts (F-t2-026 adds the deadlines).
- R3. `RunHandle` changes the registry value type; S2's `lookup_run` helper (introduced in S2 only if it stays mechanical, otherwise in S3's first commit) confines the 21 call sites.
- R4. Old stored `error_code` strings (`TARGETREPAIRREQUIRED`) differ from wire names. Mitigation: not changed here; `wire_name()` is used only for new receipts; filed as S6.
- R5. The model-gateway and execution-core treat 409 as failure (section 2.3). Mitigation: unreachable for service-owned runs until phase 2; sibling (b) owns the pause mapping (section 7).

---

### S4. F-t2-006 (a): run registry, concurrent-run caps, sweeper (effort M, 6 to 7 engineer-days)

**Goal.** Bound how many browser runs one process holds (global, per org, interactive per org), reap abandoned runs by class (D7), fix the `close_run` leak (D8), and make expiry resumable and observable.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `Q/edge/run_limits.rs` | `RunLimits`, `RunLimits::from_env() -> anyhow::Result<RunLimits>`, `takeover_enforced()` | env parser (list below); unparsable value aborts boot with the variable name; `Display` impl for one boot log line | [NEW] |
| `Q/edge/lib.rs`, `Q/edge/main.rs` | `mod run_limits;` | declare in both crate roots; place away from the in-flight `mod model_plane_gate;` hunk at main.rs:47 | [M], [M] |
| `Q/edge/agent_routes/run_registry.rs` | `RunRegistry`, `RunHandle`, `RunSlot`, `CapacityDenied { reason, limit, active }`, `ViewerGuard`, `InFlightGuard`, `RegistryStats` | new. `try_reserve(org, class)` before `acquire`; `commit(run_id, handle, slot)`; `snapshot() -> Vec<RunHandle>`; `remove(run_id)`; counters for `denied_total`, `expired_total{reason}`, `active{class}` (for F-t2-029 to export; no metrics endpoint is added here) | [NEW] |
| `Q/edge/agent_routes/run_live.rs` | `RunLive` | extend S3's struct with `last_activity_ms`, `viewers`, `in_flight`, `ttl_s`, `explicit_runtime: bool`; `touch()`, `idle_for(now)`, `age(now)`, `verdict(now, &RunLimits) -> Verdict { Keep, Expire(ExpireReason) }` as a **pure function of `now`** | [NEW] (from S3) |
| `Q/edge/agent_routes/run_sweeper.rs` | `spawn_run_sweeper(state, limits) -> JoinHandle<()>`, `sweep_once(state, now)` | every `RUN_SWEEP_INTERVAL_S`: snapshot the registry (std mutex held only for the clone), skip handles with `in_flight > 0`, compute `verdict`; for `Expire` call `finish_run(..., Expired(reason))`. Dry-run mode logs `would_expire` and does nothing | [NEW] |
| `Q/edge/agent_routes/lifecycle.rs` | `start_run`; `close_run`; new `finish_run(state, handle, reason)` | `try_reserve` right after the cheap validations and **before** `agent_driver.acquire` (1511-1515), `RunSlot` rolled back on every early return (the six `release`-and-return branches at 1529-1632 already exist); typed 429 on denial; class from `claims.is_service()`; store `ttl_s` and `explicit_runtime = constraints.max_runtime_s.is_some()`. `finish_run` (D8): lock entry, set `closed`, `registry.remove`, `agent_driver.release(entry.session.clone())` always, then events and the lifecycle timeline row; for `Explicit` also `close_run_checkpoint`; for `Expired` **leave the checkpoint Active** and emit `agent.expired` | [NEW] (moved) |
| `Q/edge/agent_routes/live.rs` | `live_frame_ws`, `live_frame_stream`, `agent_events_stream` | create a `ViewerGuard` (increments `viewers`, decrements on drop) after the ownership check; move it into the WebSocket task and into the `async_stream::stream!` block so it drops with the connection | [NEW] (moved) |
| `Q/edge/agent_routes/step.rs`, `tabs.rs`, `control.rs`, `projection.rs`, `live.rs` | handlers | `live.touch()` on step admission and completion, WebSocket actions, tab ops, control change, and every authenticated REST read (`browser_session`, `live_frame`, `live_view`, `list_tabs`, `devtools_events`, `egress_receipts`, timeline, receipts) | [NEW] (moved) |
| `Q/edge/agent_routes/mod.rs` | `new_runs()`, `new_runs_with(limits)` | `AgentRuns = Arc<RunRegistry>`; `new_runs()` zero-argument with unlimited caps and the sweeper not started (tests keep working untouched) | [NEW] (moved) |
| `Q/edge/main.rs` | agent block (1284-1290), serve block (1500-1517) | `let limits = run_limits::RunLimits::from_env()?` and `agent_routes::new_runs_with(limits.clone())` at 1290; after `app_state` is built, `let sweeper = agent_routes::spawn_run_sweeper(app_state.clone(), limits)`; `sweeper.abort()` after `axum::serve` returns | [M] (adjacent to in-flight hunks at 1395 and 1463; rebase) |
| `Q/core/event.rs` | `EventType` (8-95) | add `AgentExpired` | [C] |
| `Q/rt/nats_event_bus.rs` | `event_type_token` (394) | `EventType::AgentExpired => "agent_expired"` (exhaustive match) | [C] |
| `Q/edge/agent_routes/live.rs` | `is_user_facing_event` (949), `event_name` (965) | include `AgentExpired` and map to `"agent.expired"` | [NEW] (moved) |
| `Q/rt/step_receipts.rs` | `BrowserTimelineLifecycle` (175-180) | add `Expired` (JSONB only, no SQL) | [C] |
| `Q/docs/CONTRACTS.md` | event list (lines 77, 422) | add `agent.expired` | [C] |
| `Q/docs/openapi.yaml` | `/v1/agent/runs` (263-300) | add `429` (RATE_LIMITED with `details.reason`, `Retry-After` header) | [M] |
| `GW/browser.rs` | `forward_quarry_failure` (3873) | when Quarry returns 429 with `error.code == "RATE_LIMITED"` and `details.reason` in the three cap reasons, answer 429 with gateway code `browser_capacity_reached` and `retryAfterSeconds` (copied from `details`); other statuses unchanged | [C] |
| `FE/src/shared/i18n/errors.ts` | error map near 130 | add `browser_capacity_reached` (nb and en) | [M] |
| `FE/src/features/dashboard/home/BrowserChrome.tsx` or the session store | `pagehide` handler (optional) | `fetch(deleteUrl, {method:'DELETE', keepalive:true, credentials:'include'})` for the open session so a closed tab frees the slot immediately; `fetch` with `keepalive` supports DELETE, so no gateway route is added (the sweeper remains the backstop). Optional, S | [C] |
| `apps/Ingestion Plane/docker-compose.yml`, `Q/deploy/compose/docker-compose.yml` | edge environment | optional commented defaults for the new variables (code defaults apply without them) | [M], [C] |

**Config (all under the `QUARRY_EDGE__` prefix; `0` disables a cap or timeout where noted; invalid values abort boot)**

| Variable | Default | Meaning |
|---|---|---|
| `QUARRY_EDGE__RUN_SWEEPER` | `1` | master switch (janitor-style kill switch) |
| `QUARRY_EDGE__RUN_SWEEPER_DRY_RUN` | `0` | `1` logs `would_expire` and takes no action; use for the first deploy |
| `QUARRY_EDGE__RUN_SWEEP_INTERVAL_S` | `15` (min 1) | tick period |
| `QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S` | `300` | agent class idle expiry (no step, no control change) |
| `QUARRY_EDGE__AGENT_RUN_MAX_TTL_S` | `1800` (cap 3600) | agent absolute ceiling when the caller gave no `max_runtime_s`; also clamps an explicit one |
| `QUARRY_EDGE__AGENT_RUN_TTL_GRACE_S` | `30` | added to an explicit `max_runtime_s` so Quarry never fires before the caller's own clock |
| `QUARRY_EDGE__INTERACTIVE_RUN_IDLE_TTL_S` | `900` | interactive idle expiry (ignored while a viewer is attached or a step is in flight) |
| `QUARRY_EDGE__INTERACTIVE_RUN_MAX_TTL_S` | `14400` | interactive absolute backstop (applies even with viewers) |
| `QUARRY_EDGE__TAKEOVER_IDLE_TTL_S` | `1800` | idle expiry for `human_takeover` with no viewer |
| `QUARRY_EDGE__MAX_CONCURRENT_RUNS` | `32` | global live runs (`0` = unlimited) |
| `QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_ORG` | `8` | per org, both classes |
| `QUARRY_EDGE__MAX_INTERACTIVE_RUNS_PER_ORG` | `6` | per org, interactive class only (keeps at least 2 org slots for agent runs) |
| `QUARRY_EDGE__RUN_CAP_RETRY_AFTER_S` | `10` | `Retry-After` value on a cap denial |
| `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` | `1` | from S3 |

The numeric defaults are placeholders: the PR must record Chromium RSS at 8, 16 and 32 concurrent contexts on the target host and adjust the global cap before the default ships. Per-org overrides and plan-aware values are not part of this slice (F-t2-025 and F-t2-022 supply them).

**API / data-model changes**
- `POST /v1/agent/runs` over the cap: HTTP **429** with header `Retry-After: 10` and body `{ "data": null, "meta": {"request_id": "..."}, "error": { "code": "RATE_LIMITED", "message": "concurrent browser run limit reached for this organization", "details": {"reason": "org_run_cap", "scope": "org", "limit": 8, "active": 8, "retry_after_seconds": 10}, "retryable": true } }`. `reason` is one of `org_run_cap`, `org_interactive_run_cap`, `global_run_cap` (`scope` is `org` or `global`). No new `ErrorCode`.
- Step or tab call on a run that is no longer live: 404 `NOT_FOUND` with `details: {"reason": "run_not_live", "resumable": true}` when a non-ZDR checkpoint exists for the caller (one extra checkpoint read on the miss path only), else the existing "agent run not found".
- Event `agent.expired`, SSE name `agent.expired`, payload `{ "reason": "idle" | "ttl" | "takeover_idle" | "max_age", "idle_s": 912, "age_s": 1320 }` (no URL, no title, no org-visible content; ZDR runs go through `emit_for_zdr`, so only live subscribers see it).
- Timeline: lifecycle item `{ "kind": "lifecycle", "state": "expired", "initiatedBy": "system", "controlEpoch": 2 }`.
- `GET .../browser-session` for an expired non-ZDR run keeps working from the checkpoint projection (`live: false`); `status` stays `active` because the checkpoint CHECK allows only `active|closed` (0006); `live` is the signal.
- **No migration.** The sweeper reuses table `quarry_agent_run_checkpoints` and event JSONB.

**Rollout and rollback.** Deploy 1: `QUARRY_EDGE__RUN_SWEEPER_DRY_RUN=1` for at least 24 hours on the shared stack, review `would_expire` lines for interactive runs that a human was still using (false positives), then flip. Caps are live from deploy 1 at the placeholder numbers, with `0` as an immediate escape. Rollback: `RUN_SWEEPER=0` and `MAX_CONCURRENT_RUNS*=0` without an image change; image rollback is safe because nothing is persisted that an older binary cannot read.

**Tests**
- Unit (`run_registry.rs`): `reserve_respects_global_org_and_interactive_caps`; `slot_rolls_back_on_drop_until_committed`; `remove_decrements_counters`; `concurrent_reservations_never_exceed_cap` (64 tasks against cap 8, assert at most 8 succeed); `zero_means_unlimited`; `interactive_cap_leaves_agent_headroom`.
- Unit (`run_live.rs`, pure `verdict`): `agent_ttl_uses_explicit_runtime_plus_grace`; `agent_default_120_is_not_absolute`; `agent_ttl_ignores_viewers`; `interactive_ignores_gateway_120s_ttl`; `interactive_idle_blocked_by_viewer`; `interactive_backstop_applies_with_viewer`; `takeover_idle_longer_than_interactive_idle`; `touch_resets_idle`; `in_flight_is_never_expired`.
- Unit (`run_limits.rs`): `from_env_rejects_unparsable_and_names_the_variable`; `defaults_match_the_documented_table`.
- Integration `Q/crates/quarry-edge/tests/agent_run_sweeper.rs` `[NEW]` (recording driver counts `acquire` and `release`; `sweep_once(now)` called directly with an injected `now`, no sleeps): idle agent run released exactly once and `agent.expired` emitted; **a run with an open viewer survives idle but not the backstop**; a takeover run with no viewer expires at `takeover_idle`; a step in flight is skipped; ZDR run expiry writes no checkpoint, no timeline event; non-ZDR expiry leaves the checkpoint `Active` and a later `start_run` with `resume_run_id` succeeds; **`close_run` while another `Arc` is held still releases the session** (regression for D8); cap denial returns 429, `Retry-After`, the three `reason` values, counts UI-created (user claims) and agent (service claims) runs together; a failed `acquire` frees the reserved slot.
- Gateway: `forward_quarry_failure_maps_cap_denial_to_browser_capacity_reached`.
- SPA (optional pagehide): vitest `closes the live session with keepalive on pagehide`.
- Commands: `cargo test -p quarry-edge run_registry run_live run_limits agent_run_sweeper`; `cargo test -p quarry-core event`; `cargo test -p quarry-runtime nats_event_bus step_receipts`; CI `cargo test --workspace`.

**Definition of done.** Registry-level counters enforce all three caps with no overshoot under concurrency; the sweeper releases idle and over-age runs by class, never one with a step in flight or (for idle) a viewer; `close_run` and the sweeper share one release path that no longer depends on `Arc::try_unwrap`; expiry is resumable for non-ZDR runs; event, timeline and docs updated; defaults recorded with measured RSS; dry-run stage completed.

**Deployment evidence required.** Rebuild the edge and gateway images (`make up`; env chain per the deploy recipe). Test-only values supplied by the last env file in the chain (not by editing committed compose): `RUN_SWEEP_INTERVAL_S=5`, `INTERACTIVE_RUN_IDLE_TTL_S=20`, `AGENT_RUN_IDLE_TTL_S=20`, `MAX_CONCURRENT_RUNS_PER_ORG=3`, `MAX_INTERACTIVE_RUNS_PER_ORG=2`. Boot log shows one `run limits: global=32 org=3 interactive=2 sweeper=on dry_run=off ...` line. Then, on the running stack:
1. **Caps.** Create two browser sessions in the Verevon UI (gateway `POST /api/v1/browser/sessions`), then a third: the third returns 429 with gateway code `browser_capacity_reached` and `retryAfterSeconds`; start an AI browser run from chat (service principal): it succeeds (org total 3); a fourth run of either class returns 429. Direct to the edge, the 429 carries the `Retry-After` header and `details.reason`.
2. **Idle expiry.** Leave session 1 untouched for 30 seconds: edge log shows `run expired reason=idle class=interactive` (run id only, no URL); `GET .../browser-session` returns `live:false`; a new session now succeeds at the cap (capacity freed is the observable proof of release); `docker stats quarry-edge` memory drops after the release (supporting evidence). `psql` shows the run's `quarry_agent_run_checkpoints.status` still `active`.
3. **Viewer holds.** On session 2, `curl -N` the `frames/stream` SSE with the user token and wait 60 seconds: still `live:true`. Kill curl: the run expires within `idle + interval`. A pre-opened `events/stream` shows `agent.expired`.
4. **Resume.** `POST /v1/agent/runs` with `resume_run_id` of the expired run (with a valid grant or dev bypass) succeeds and continues the step count.
5. **Leak regression.** Hold a `frames/ws` connection open, `DELETE` the session, then create sessions up to the cap: the freed slot is available and the Chromium context is gone (before this slice the slot logic did not exist and the context leaked; the evidence file records the pre-fix reproduction from the integration test).
6. Record RSS at 8, 16 and 32 contexts (script in the PR) and set the shipped global default from it.

**Risks and mitigations**
- R1. A false-positive reap of a human session. Mitigation: interactive class ignores the 120 s literal; dry-run stage; viewers and takeover protection; generous 900 s idle.
- R2. Cap starvation of AI runs by open tabs. Mitigation: the interactive sub-cap leaves headroom.
- R3. A half-open WebSocket holds a run forever. Mitigation: the absolute backstop applies with viewers.
- R4. A 429 breaks execution-core (it fails the whole plan on any start error, `browser_agent.rs:690-700`). Mitigation: sibling (b) handles `RATE_LIMITED` on `start_run` with a bounded retry using `Retry-After` (section 7); until then a capped org sees a failed run with a clear message.
- R5. The `Arc`-clone assumption in `finish_run`: a handler that holds the `Arc` and runs after release gets a driver error. Mitigation: the `closed` flag is checked immediately after the lock in every handler and in `admit_action` (S3), returning `run_not_live`.
- R6. `main.rs` and `config.rs` rebase against uncommitted work. Mitigation: no `config.rs` edit (D13); `main.rs` edits are two small blocks.

---

### S5. F-t2-006 (b): hide the fleet console (effort S, 0.5 engineer-day)

**Goal.** Stop shipping a route that cannot work (D10).

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `FE/src/app/App.tsx` | route `/agents/fleets/:fleetId` (216) and the lazy import (17) | wrap in `import.meta.env.VITE_FLEET_CONSOLE === '1'`, default off (or delete both lines) | [M] (the in-flight hunks at 10, 39, 178, 242 do not touch 17 or 216, per S2) |
| `FE/src/features/agents/components/FleetRunConsole.tsx`, `.test.tsx` | component | kept untouched for revival; the test renders the component directly, so it keeps passing | [C] |
| `Q/edge/fleet_routes.rs` | routes | none | [C] |

Tests: `pnpm vitest run src/features/agents/components/FleetRunConsole.test.tsx` unchanged; add `App.fleet-route-is-hidden.test.tsx` asserting `/agents/fleets/x` falls through to the not-found route (if an `App` route test harness exists `[UNVERIFIED]`; otherwise a one-line assertion on the route table export). Commands: `cd "FE" && pnpm lint && pnpm typecheck && pnpm test`.
DoD: navigating to the URL no longer renders the console; the lazy chunk is not emitted in `pnpm build` output.
Evidence: rebuild and restart the SPA (container over a bind mount does not see host edits; restart per memory `verevonv3-vite-needs-restart`, and clear the cached `global.css`), open `/agents/fleets/abc` and show the not-found page; show the build output without the `FleetRunConsole` chunk.
Risk: none material. If the user prefers option 1 of D10, this slice is replaced by the M proxy-plus-cancel slice and a Model Plane fleet producer (open question 3, section 9).

### S6 (optional). Wire-code accuracy for agent routes (effort S, 1 engineer-day)

Make `status_err` (`agent_routes.rs:1106-1114`) derive its `ErrorCode` from the HTTP status (404 `NOT_FOUND`, 403 `FORBIDDEN`, 409 `CONFLICT`, 429 `RATE_LIMITED`, default `BAD_REQUEST`) and add `ErrorCode::wire_name()` to receipts' `error_code` for **new** rows only. This changes response bodies for about 30 call sites and the persisted `error_code` text, so it is a separate PR with its own gateway and SPA check; it is not required by S1 to S5. Tests: `status_err_maps_http_status_to_error_code`, and a golden for the receipt text. Not scheduled unless the Model Plane mapping work (section 7) needs it.

## 5. Dependencies and ordering

### 5.1 Ordering

| Order | Slice | Depends on | Kind |
|---|---|---|---|
| 1 | S1 Evaluate deny | nothing | none |
| 2 | S2 module split | S1 merged (so the fix is not caught mid-move); a freeze on `agent_routes.rs` | hard, process |
| 3 | S3 control authority | S2 (soft: the same code can be written in the monolith at more merge cost) | soft |
| 4 | S4 registry, caps, sweeper | S3's `RunLive` and `RunHandle` skeleton (soft: S4 can define them itself if S3 slips; then S3 rebases onto S4's struct) | soft |
| any | S5 fleet hide | nothing | none |
| any | S6 `status_err` accuracy | nothing; better after S3 | soft |

Parallelism: after a 0.5-day shared skeleton PR (`RunLive`, `RunHandle`, `lookup_run`) one engineer takes S3 (`admission.rs`, `step.rs`, `control.rs`, `tabs.rs`, migration 0010) and another takes S4 (`run_registry.rs`, `run_sweeper.rs`, `lifecycle.rs`, `live.rs` guards, `run_limits.rs`). They meet in `RunLive` and `types.rs::RunEntry`; agree on the field list up front.

### 5.2 Cross-workstream dependencies

| Other item | Relation | Detail |
|---|---|---|
| F-t2-001 (production broker grants, delegated bearer) | **Not required** for S1 to S5; **hard for** the alternative Evaluate design (Option A), for any sensitive-approval path, and for phase 2 | Sensitive approvals fail without a parent run grant (`agent_routes.rs:154-157`). Hazard: if F-t2-001 makes Model Plane forward a *user* bearer on agent steps, principal derivation would call those steps human; Model Plane must then send `initiated_by: "agent"` (section 7) |
| F-t2-008 (budgets) | consumes S3 and S4 | `StepInitiator` lets it scope step and runtime budgets to agent runs (the manual-session 12-step cap today counts human actions); `RunLive.started_at` and the `RunLimits` vocabulary are reusable; it also needs the 429 admission path S4 adds |
| F-t2-025 (per-org capacity) | shares the admission seam | The crawl path uses `RuntimeLeasePool` (`Q/rt/lease_pool.rs`, per-host semaphores, pool sizes `main.rs:323, 337`); the agent path uses S4's registry. One `RunLimits` naming scheme should serve both; plan-aware values arrive through `ResolvedPolicy` (F-t2-022) later |
| F-t2-029 (metrics, drain) | consumes S4 | `RegistryStats` counters for export; labels must stay ZDR-safe (reason and class only). Lease drain on shutdown should call `finish_run` for every handle |
| F-t2-011 (free human browsing) | overlaps D7 | Interactive-class idle and backstop numbers are its "what replaces the 120 s TTL" answer (open question 10.2.2). The gateway literals `max_steps: 12`, `max_runtime_s: 120` (`GW/browser.rs:485-487`) are its to change |
| F-t2-014 (run trace), F-t2-021 (redaction) | additive on S3 | Timeline and receipt fields are additive; redaction edits the same receipt construction in `step.rs` |
| F-t2-023 (scrape actions, PDF) | inherits S1 | Must introduce a new read-only evaluate action, never relax `Evaluate`; `ActionRuntime` is refused by S1 |
| F-t2-026 / F-t2-047 (deadlines, sub-codes) | share `Q/core/error.rs` | Coordinate PR order; each new `ErrorCode` needs the mirror checklist in S3 |
| F-t2-013 phase 2 (attach) | builds on S3 | Needs ADR topic 3, F-t2-001 and a delegation claim; S3 ships the data model and enforcement it needs |
| F-t2-054 / F-t2-035 (run and turn tabs, durable sessions) | share the session state machine | S4's run classes and `expired` lifecycle are the starting vocabulary |
| F-t2-007 (viewport relaunch, process pool) | informs S4 caps | Measure RSS per context after part A so the shipped defaults are real |

### 5.3 Merge-conflict hotspots

| File | Lines (pre-split) | Shared with |
|---|---|---|
| `Q/edge/agent_routes.rs` `[C]` | `RunEntry` 77-107 | 006, 013, 014, 036 |
|  | `validate_run_grant` 314-338 | 001, 011, 032, 036 |
|  | `sensitive_approval_for_action` 144-282 | 024, 032, 036, 039 |
|  | `start_run` 1273-1663 (policy 1397-1412, ttl 1446, lease and acquire 1487-1515, insert 1635-1639) | 001, 008, 011, 020, 058 |
|  | `step` 2228-2409 (grant 2257-2267, sensitive 2276-2292, budget 2294, domain 2297-2309, receipts 2340-2398) | 002, 008, 011, 013, 021, 036 |
|  | `execute_ws_action` 3086-3193; tabs 3203-3451 | 013, 054 |
|  | timeline and projection 764-815, 1665-1895; events 944-979 | 014, 038 |
|  | `close_run` 3562-3655 | 006, 029 |
| `Q/rt/step_receipts.rs` `[C]` | `StepReceipt` 183-210; `ReceiptBuilder` 453-556; checkpoint 39-71; timeline kinds 138-180 | 014, 021, 036 |
| `Q/core/error.rs` `[C]`, `Q/core/event.rs` `[C]` | enums | 026, 029, 047 |
| `Q/edge/main.rs` `[M]` | 26-47 (mods), 1284-1290, 1500-1517 | 001, 025, 029; in-flight hunks at 47, 1395, 1463 |
| `Q/edge/config.rs` `[M]`, `state.rs` `[M]` | avoided on purpose | 001, 022, 025 |
| `Q/docs/openapi.yaml` `[M]` | 263-345, 2389, 2517, 2787, 3042 | 005, 017, 064 |
| `GW/browser.rs` `[C]` | `run_action` 663-760; `forward_quarry_failure` 3873-3885 | 001, 008, 011, 026 |
| `FE/src/shared/i18n/errors.ts` `[M]` | one key block near 130 | many (additive) |

---

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

| Rule | Statement | Status |
|---|---|---|
| Control Plane owns identity, orgs, billing, quotas, entitlements | S3 *consumes* Auth Core's `principal_type` and `service_id` claims (`Q/edge/auth.rs:99-103, 148-174`); it defines no identity. S4's caps are infrastructure safety limits in the plane that owns the browser lifecycle, set by env; they are not entitlements and do not read Control Plane quotas. Plan-aware limits are left to F-t2-022 and F-t2-025 | OK. Risk only if S4's numbers are later presented as plan limits |
| Data Plane owns documents, chunks, traces | no Data Plane contact | OK |
| Ingestion persists through Data Plane contracts only | receipts, checkpoints and timeline events stay in Quarry's own Postgres through `quarry-runtime` migrations (new `0010_step_receipt_initiator_epoch.sql`); no other plane's tables are read or written | OK |
| Provider actions go through integration-corev2 | not touched | OK |
| Model Plane owns reasoning, sessions, browser grants, cost | S1 deliberately avoids a broker or proto change (no script-hash scope); the grant flow, `AcquireGrant` and `ValidateGrant` are untouched. Cost is untouched (F-t2-008) | OK |
| Application Plane owns projections and notifications | the new `agent.expired` event and the additive timeline fields are inputs a later projection may consume; they carry no URLs or content | OK |
| Frontend Plane owns UI and same-origin BFF normalisation | the gateway change is one body field (`initiated_by`, tighten-only, not a header, so it cannot forge a scoping header) and two error translations; no secret, token or upstream detail is exposed | OK |
| No direct database crossing | none introduced | OK |
| Model proposes, Quarry executes or rejects | S1 and S3 are Quarry rejecting; the admission pipeline decides, not the planner. The caller-asserted `initiated_by` hint can only make a request stricter. Approval authority never moves to the model: the one-shot sensitive-approval path is unchanged and still requires a broker grant | OK |
| ZDR propagates through every content-persisting boundary | see below | OK, with the checks listed |
| Channel Plane is docs-only | not touched | OK |

ZDR, per slice:
- S1: denial logs the action name only, never the script (scripts may carry secrets); no receipt is written for the denied action (the same as every other admission refusal).
- S3: enforcement is independent of persistence, so a ZDR run is gated identically; epoch and mode live in memory; the epoch is **not** checkpointed, control events, receipts and the Skipped receipt are all skipped under `entry.zdr.is_active()` (existing guards at 493-495, 2360, 2374, 2391); tab events likewise. Test `zdr_run_enforces_gate_but_persists_nothing` asserts the in-memory store is empty.
- S4: ZDR runs are reaped by the same rules but write no checkpoint, no lifecycle timeline row; `agent.expired` goes through `emit_for_zdr` (live subscribers only, `agent_routes.rs:1643`); the payload has no URL; `RegistryStats` labels are class and reason only; the sweeper never reads `last_observation`; viewer counts are not persisted.
- Risk to watch: the `run_not_live` response's `resumable` flag reads a checkpoint only for non-ZDR runs and only for the owning caller; for ZDR it must always answer `false` without a read (test `zdr_miss_path_does_not_read_checkpoints`).

---

## 7. Cross-session interfaces

### (b) Model Plane, chat and agents (owner of the items below: sibling (b); Quarry-side contract owner: this workstream)

| Interface | What Quarry now does | What sibling (b) must do | Where |
|---|---|---|---|
| `initiated_by` on every agent step | Derives `Agent` from a service principal; accepts a tighten-only `initiated_by: "agent"` body field | Always send `initiated_by: "agent"` on `step`, tab calls and WebSocket actions, **especially if F-t2-001's delegated bearer makes the call carry a user token** (otherwise those steps are classed human and skip the takeover gate). Add the field to `StepRequest` | `MP/rust/services/execution-core/src/quarry_agent.rs` (`StepRequest` near 612-640) |
| `USER_TOOK_OVER` (409) | Refuses agent actions during takeover | Map to a paused state (treat like `WaitingApproval`, `browser_agent.rs` `PlanStepResult::WaitingApproval`), not to `PlanStatus::Failed` (today any step error fails the plan, `browser_agent.rs:742-745`). Parse the body `error.code`; `AgentClientError::Status` currently keeps the raw body | `execution-core/src/browser_agent.rs`, `quarry_agent.rs:485, 680-705`; `model-gateway/src/grpc.rs:2700`, `tools.rs:1184` (add `failed_precondition`) |
| `RATE_LIMITED` on `POST /v1/agent/runs` (cap) | 429 with `Retry-After` and `details.retry_after_seconds` | Bounded retry honouring `Retry-After`, then a user-visible "browser capacity reached"; today a start error fails the plan (`browser_agent.rs:690-700`) | `browser_agent.rs`, `quarry_agent.rs` `start_run` (574-600) |
| 404 with `details.reason = "run_not_live"` and `resumable: true` | Expired runs keep an `Active` checkpoint | Resume with `resume_run_id` and a fresh grant instead of failing | `quarry_agent.rs` `StartRunRequest` (needs a `resume_run_id` field) |
| Evaluate | Always 403 `FORBIDDEN` / `raw_script_denied` | Remove the `Evaluate` variant from the wire enum (`quarry_agent.rs:161`); never add evaluate to `browser.act` | same file |
| Broker allowlist and proto | unchanged by design (Option B) | none. Do **not** add `evaluate` to `allowedSensitiveActions` (`grant.go:32-36`) | `browser-broker` |

### (a) Verevon Space

| Interface | Proposed owner | Notes |
|---|---|---|
| Agent PC or browser inside a Space | Space session reuses `/v1/agent/runs`; Quarry owns run class, caps and the sweeper | Space-hosted agent runs count against the same org caps and class rules as chat runs. If a Space needs its own cap, add a third key later; do not invent a parallel counter |
| Space members as viewers | Blocked until phase 2 delegation | `caller_owns_live_run` only admits the run owner (`agent_routes.rs:474-486`); Space members cannot attach, so `ViewerGuard` counts only owners today. The Space design must not assume member viewing exists |
| Presence and control UI | Quarry projection fields `controlEpoch`, `initiatedBy` (additive) | Space presence should render "agent working / user took over" from these, not from a gateway-local cache |
| ZDR-safe telemetry | `agent.expired` and `RegistryStats` | Class and reason only; no URL, title or page text. Any Space telemetry built on them inherits the rule |
| What the user must reconcile | | (1) who counts as the `Human` initiator when a Space member acts on an owner's session, (2) whether Space runs are `Interactive` or `Agent` class for TTL purposes, (3) the idle numbers in D7 against Space expectations of long-lived shared sessions |

Shared pieces and proposed owner: `RunLive`, the admission pipeline and the 429 contract belong to Quarry (this workstream); the mapping of Quarry codes into Model Plane run states belongs to sibling (b); presence rendering belongs to sibling (a) and the Frontend Plane.

---

## 8. What can proceed without an ADR or decision, and what is blocked

Proceeds now (each ships a conservative default; none needs a ruling):
- S1 (reject Evaluate), S2 (split, process decision only), S3 (control authority; phase 1 only), S4 (caps and sweeper with env defaults and a dry-run stage), S5 (hide the fleet console).
- The Go mirror sync in S3 and the optional S6.

Blocked or gated:

| Work | Blocked by | Why |
|---|---|---|
| Evaluate behind a broker approval (Option A) | ADR topic 6 (script model) and F-t2-001 | needs a script-hash scope the broker lacks and a live grant issuer |
| Read-only isolated-world evaluator | F-t2-023, ADR topic 6 | belongs where a return value is consumed |
| Phase 2 attach, Reveal, Take over and Give back in tabs | ADR topic 3, F-t2-001, a delegation design | ownership gate (`caller_owns_live_run`) |
| Final idle and backstop numbers; human-session policy | ADR topic 3 (open question 10.2.2) | defaults ship, ratification pending |
| Plan-aware or per-org-override caps | ADR topic 1 (policy of record), F-t2-022, F-t2-025 | no claim channel yet; env caps cover the safety need |
| Making the fleet console real | product decision plus a fleet producer in Model Plane | nothing creates fleets today |
| Flipping the budgets to agent-only | F-t2-008 | consumes S3's initiator |

---

## 9. Open questions for the user (real blockers only)

None of these blocks S1 to S5 from starting; items 1 and 2 should be answered before S4 merges, item 3 before S5 is finalised.

1. **Idle and backstop numbers (ADR topic 3).** Are 300 s (agent idle), 900 s (interactive idle), 1800 s (takeover idle, no viewer) and a 4-hour interactive backstop acceptable? The backstop deliberately overrides "never reap a run a user watches": without it a vanished client holds a Chromium context forever. Do you want a different ceiling, or no ceiling and an alert instead?
2. **Event name.** `agent.expired` (this design, consistent with `agent.completed` and `agent.failed`) or `run.expired` (the audit card, which collides with the crawl `run.*` family)?
3. **Fleet console.** Hide it now (recommended), or build the gateway proxy plus cancel route? The second needs someone to own the code that creates fleets; nothing does today.
4. **Effort and wire change.** Accept the re-estimate of F-t2-013 phase 1 from S to M, and the S3 side effect that two REST error bodies change from code `BAD_REQUEST` to `RATE_LIMITED` and `SECURITY_BLOCKED` (HTTP statuses unchanged)?
5. **Freeze for the split.** Can the team hold a two-working-day freeze on `agent_routes.rs` so S2 merges cleanly, and who arbitrates merge order against the F-t2-001 and F-t2-008 owners?

---


## 10. INTERFACE SUMMARY

**Files touched.** Ingestion Plane (Quarry-v2): `quarry-edge` `agent_routes.rs` (split into `agent_routes/*`, new `run_live.rs`, `run_registry.rs`, `run_sweeper.rs`, `admission.rs`), `run_limits.rs`, `main.rs` and `lib.rs` (mod and spawn lines); `quarry-core` `error.rs`, `event.rs`, `contracts` tests; `quarry-runtime` `observation.rs`, `action_runtime.rs`, `mp_client.rs`, `step_receipts.rs`, `postgres_step_receipts.rs`, `fallback_driver.rs`, `nats_event_bus.rs`; Go `pkg/quarrycontracts/envelope.go`; docs `openapi.yaml`, `CONTRACTS.md`. Frontend Plane: gateway `browser.rs` (initiator field, two error translations), SPA `errors.ts`, `App.tsx` (fleet route hidden), optional pagehide close. Model Plane (sibling b, follow-ups): `quarry_agent.rs`, `browser_agent.rs`, model-gateway `grpc.rs` and `tools.rs`.

**New contracts.** Error `USER_TOOK_OVER` (409, non-retryable); `RATE_LIMITED` with `details.reason` (`org_run_cap`, `org_interactive_run_cap`, `global_run_cap`) plus `Retry-After`; 403 `FORBIDDEN` `raw_script_denied`; optional `initiated_by` on step, tab and WebSocket actions; `controlEpoch`, `initiatedBy` on projection, timeline and receipts; event `agent.expired`; migration `quarry-runtime/migrations/0010_step_receipt_initiator_epoch.sql`; flags `QUARRY_EDGE__TAKEOVER_ENFORCEMENT`, `RUN_SWEEPER`, `RUN_SWEEPER_DRY_RUN`, eleven timing and cap variables (section 4, S4). No proto, no broker, no Control Plane or Data Plane change.

**Dependencies.** None hard for S1 to S5. F-t2-001 gates the Option A and phase 2 alternatives. F-t2-008, F-t2-025, F-t2-029 consume `StepInitiator`, the registry and its counters. F-t2-026 and F-t2-047 share `error.rs`.

**Effort.** S1 S 1.5 d; S2 M 2.5 d (go); S3 M 5 to 6 d (audit said S); S4 M 6 to 7 d; S5 S 0.5 d; S6 optional S 1 d. Total about 16 to 18 engineer-days.

**ADR gates.** Topic 6 (script model; only for reviving evaluate); topic 3 (human plus agent sessions; idle numbers, phase 2); topic 1 (plan-aware caps). None blocks W1.

**Parallelisable with.** S1 and S5 with anything; S3 and S4 after a 0.5-day shared skeleton; F-t2-026, F-t2-007, F-t2-004 phase 1 (different crates).

**Conflicts with.** Any PR editing `agent_routes.rs` until S2 merges (freeze); F-t2-008, 011, 014, 021, 032, 036 in `step` and `start_run`; F-t2-001 in `main.rs` and `validate_run_grant`; uncommitted `main.rs`, `config.rs`, `state.rs` diffs (rebase, do not merge blind).

**Key findings to carry forward.** Gateway WebSocket takeover enforcement is test-only; users cannot take over service-owned runs until phase 2; a lease-`ttl_s` sweeper would kill manual sessions after 120 s; `close_run` leaks sessions when any handler holds the run; `status_err` mislabels error codes.

<!-- COMPLETE -->
