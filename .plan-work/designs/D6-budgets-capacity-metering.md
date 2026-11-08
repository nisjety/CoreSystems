# D6 budgets-capacity-metering: enforce run budgets and org capacity in the Quarry executor, meter web usage, and give Quarry a metrics, statistics and drain base

Author: Sonnet design agent D6, wave W2. Date: 2026-10-15. Tree read: LIVE `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Audit source: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Nothing in the product tree was edited.

Path shorthand (tags from `tools/provenance.sh`): `IP/` = `apps/Ingestion Plane/`; `Q/` = `IP/Quarry-v2/`; `QE/` = `Q/crates/quarry-edge/src/`; `QR/` = `Q/crates/quarry-runtime/src/`; `QC/` = `Q/crates/quarry-core/src/`; `QO/` = `Q/services/quarry-orchestrator/`; `QCT/` = `Q/services/quarry-control/`; `CP/` = `apps/Control Plane/`; `BC/` = `CP/billing-core/`; `MP/` = `apps/Model Plane/`; `GW/` = `apps/Frontend Plane/verevonv3/apps/gateway/src/`; `SPA/` = `apps/Frontend Plane/verevonv3/src/`.
Tags: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` file this plan would create (verified MISSING). Evidence tags: `[READ]` opened and confirmed in this session, `[INF]` reasoned from code, `[UNVERIFIED]` needs a run or a read I did not do.

## 1. Items resolved and wave

| Item | Scope in this design | Wave | Verified effort | This design's estimate |
|---|---|---|---|---|
| F-t2-008 | Budget accumulator and `enforce_budget` on the step path for agent-class runs (slice 1), crawl `max_pages` 0-is-unlimited fix (immediate), clamp of crawl, batch and agent limits with the clamped value returned, grant-carried budget claim, Control Plane remaining-entitlement read | W2 | L | **L, about 20 engineer-days across Quarry, Control Plane and Model Plane** (slices S0, S1, S3, S4, including 3.5 days of sibling Model Plane work), of which the audit's "slice 1" is M (5 days), not S |
| F-t2-025 | Per-org capacity: class-aware gateway rate limits, per-org permits and plan-aware caps in Quarry, job backlog with fair share at quarry-control | W2 | L | **L, about 11 engineer-days** (S5, S6, S7); shares admission code with D2 (F-t2-006) |
| F-t2-046 | Web-usage metering: browser-second, agent run and step, crawl page, extract and map metrics with outcome and surface attributes, ZDR-clean metadata, billing-core plan keys and read | W3 in the audit, **pulled to W2 here** because the entitlement clamp in F-t2-008 has nothing to subtract from until usage events exist | L | **L, about 8 engineer-days** (S2 plus the Control Plane part of S3) |
| F-t2-029 | Quarry `/metrics` recorder on an internal listener, graceful drain of leases and usage, per-run statistics; design-level only for memory-aware admission, span vocabulary and alert rules | W2 | L | **S8, S9, S10 about 9 engineer-days scheduled; about 15 more days design-level and unscheduled (L1 to L3)** |

Total scheduled: about 47 engineer-days; two streams are possible (Section 5). Six honest corrections to the audit cards, details in Section 2:

1. The card's slice 1 enforcement "as written" would still be wrong after D2: `ctx.step` only increments on success (`QR/observation.rs:1015`), so a model looping on failing actions never reaches `max_steps`. Attempts must be counted.
2. With the flat action-cost table (click 0.0001 USD, navigate 0.001, screenshot 0.005, `QR/action_cost.rs:22-29`) a `max_cost_usd` of 1.00 cannot bind before hundreds of steps. A cost cap without a browser-time component is decorative. The design adds an estimated browser-second term, labelled as an estimate.
3. `verified_action_cost_micro_usd` has **no producer anywhere** (only reads at `QE/agent_routes.rs:715` and the field at `QC/contracts.rs:123`). "Provider-billed first, flat table fallback" is today "flat table always".
4. Crawl pages are **not metered at all**. `routes.rs:642-659` meters one `quarry.crawl.seed` per submission and its comment says the orchestrator meters pages through `run_page`; `internal_run_page` (`QE/routes.rs:898-1020`) has no `.meter(` and the orchestrator has no usage code (`grep` empty). Without a page event the "remaining entitlement" for a crawl never decreases.
5. Cost-core cannot supply "remaining budget" (card 008): it accounts model spend by `(org, user)` with a caller-supplied cap (`MP/go/services/cost-core/internal/ledger/ledger.go:536-561`). The remaining *entitlement* lives in billing-core (`GET /api/v1/billing/orgs/:orgId/quotas/:metric` already exists with scope `billing:quota:read:any`). The grant claim should be minted by model-gateway, which already reads both.
6. The `max_pages` bug has a twin: failed pages never count toward `max_pages` and `max_depth == 0` is unlimited (`QO/workflows.go:464-469, 474, 571, 574`). One PR fixes all three.

## 2. Verified current state

### 2.1 F-t2-008 (budgets)

| Claim (card, audit or sweep) | Verdict | What the code does |
|---|---|---|
| `enforce_step_budget` checks `max_steps` only, called at `agent_routes.rs:2294` | OK `[READ]`, extends | defined 446-451 (`if max_steps > 0 && entry.ctx.step >= max_steps`). A **second** call site exists in the WebSocket path at 3118 (S2 sweep noted it, the card does not) |
| `default_constraints` leaves cost and runtime unbounded | OK `[READ]` | 433-440: `max_steps 0`, `max_runtime_s None`, `max_cost_usd None`; used as the serde default of `StartRunBody.constraints` (349) |
| `max_runtime_s` only becomes the lease `ttl_s` (1446) | OK `[READ]` | `constraints.max_runtime_s.unwrap_or(120)` at 1446; D2 already established that nothing reads `ttl_s` and the default 120 is a placeholder |
| Per-step `cost_usd` is computed for receipts and never accumulated (2346-2362) | OK, with three precision findings | REST path 2344-2362 falls back to the flat table when the driver omits a cost. **[DRIFT]** the WebSocket path (3154-3165) records `observation_receipt_cost_usd(&observation)` only, with **no flat fallback**, so WebSocket receipts carry cost 0 whenever the driver is silent. **[DRIFT]** failed-step receipts (`ReceiptBuilder::fail`, `QR/step_receipts.rs:534`) and skip receipts (554) record `cost_micro_usd: 0`, so the fleet budget route that re-sums receipt cost (`QE/fleet_routes.rs`, test at 231) also ignores failures. **[DRIFT]** `verified_action_cost_micro_usd` has no producer (grep over `Q/` finds only `agent_routes.rs:715`, `contracts.rs:123` and a `None` assertion at `browser_egress_proxy.rs:790`) |
| Library `AgentLoop` has cost and runtime logic and no HTTP caller | OK `[READ]` | `agent_loop.rs:154-163` (`over_budget`), 186-260 (runtime, steps, cost checks). Note its step check is `ctx.step >= max_steps` with no `> 0` guard, so `max_steps 0` stops at once, the opposite of the edge's rule; the two must not be mixed |
| Flat table is a usable cost signal | **[DRIFT]** | `action_cost.rs:22-29`: click-class 0.0001, navigate 0.001 + 1e-7 per byte, screenshot and pdf 0.005. Two hundred screenshots cost 1.00 USD. No time term exists |
| Failed actions consume step budget | **[CLAIM-FALSE] implicit in the card** | `ctx.step += 1` runs only at the end of a successful `execute` (`QR/observation.rs:1015`). A failing action leaves `ctx.step` unchanged, so `enforce_step_budget` never trips on a loop of failures |
| The gateway caps manual sessions at 12 steps and 120 s, human actions share `/step` | OK `[READ]` | `GW/domains/browser.rs:483-488` (`max_steps 12`, `max_runtime_s 120`). Today the 13th human action returns 429 (D2 2.3); this design flips that |
| AI-run caps are client-optional | OK `[READ]` | `GW/browser.rs:1239-1243` forwards `max_steps`, `max_runtime_s`, `max_cost_usd` from the request body (all `Option`), model-gateway forwards them (`MP/rust/services/model-gateway/src/browser_run.rs:402-407`), execution-core copies them into `AgentConstraints` (`MP/.../execution-core/src/browser_agent.rs:658-667`). Omit them and Quarry sees `max_steps 0`, no runtime, no cost |
| execution-core never checks cost | OK `[READ]` | `check_limits` (`browser_agent.rs:320-331`) tests steps and runtime only; its `BrowserResourceLimit` enum has `MaxSteps` and `Runtime` (59-62). It always calls `close_run` at loop end (773) |
| `max_pages` absent or 0 means unlimited (`workflows.go:456`) | OK `[READ]` | `QO/internal/workflows/workflows.go:456` (in-workflow loop) and **555** (durable-frontier loop): `in.MaxPages > 0 && pagesVisited >= in.MaxPages`. Dispatcher maps `uintFrom(params["max_pages"])` at `QO/internal/jobs/dispatcher.go:452`; the edge forwards the raw optional value (`QE/routes.rs:633`). The gateway rejects 0 and clamps to 5000 only when the field is sent (`GW/domains/knowledge/quarry.rs:396-405`); direct callers of `/v1/crawl` are unbounded |
| Same defect class (new) | **[DRIFT]** | `pagesFailed` never counts toward `max_pages` (464-469 and 571); `MaxDepth == 0` is unlimited (474, 574). Rust `FrontierConfig` defaults (max_depth 5, max_pages 1000, `crawl_frontier.rs:72-73` per S5) are unused by the workflow. The only wall limit is a fixed 30-minute Temporal execution timeout (`QO/jobs/dispatcher.go:254`), which kills a run with no typed outcome |
| The workflow can use per-crawl concurrency | **[CLAIM-FALSE] premise of the clamp wording** | both loops visit pages strictly sequentially (`runPage` in a `for`, 456-470 and 559-591). Only job-level concurrency exists (Section 2.2) |
| cost-core "carries the remaining budget" | **[CLAIM-FALSE]** as a mechanism | `Ledger.CheckBudget(org, user, maxCost, maxTokens)` compares the **cumulative** org+user total against a cap the *caller* passes (`ledger.go:536-561`); model-gateway passes the org-core per-run ceiling into it (`MP/rust/services/model-gateway/src/budget.rs:48-80`), so the key `max_cost_per_run_usd_micros` is applied to cumulative usage (naming and semantics differ, noted, not fixed here). Cost-core's billing outbox delivers only `metered` and `subscription` bases (`MP/go/services/cost-core/internal/postgres/billing_delivery.go:46`). It has no browser or run-budget concept |
| Org default caps exist in Control Plane | OK, partial | org-core `org_quotas` key `max_cost_per_run_usd_micros` is read by model-gateway with a service credential and **fails open** (`MP/.../model-gateway/src/org_quota.rs:24, 97-150`); the gateway allowlists the key (`GW/domains/orgs/quotas.rs:27`) with the rule "a key is added only together with its reader". Only model-gateway reads it |
| A Control Plane remaining-entitlement read is missing | **[DRIFT]** (it exists) | `BC/internal/http/server.go:68-69`: `GET /api/v1/billing/orgs/:orgId/entitlements/:feature` and `.../quotas/:metric`; scope map `service_auth.go:30-44` and `253-254` (`billing:quota:read:self|any`); `GetQuotaStatus` returns `{limit, used, remaining, is_exceeded}` (`BC/internal/billing/service.go:845-919`). Gaps: (a) an unknown metric returns `limit 0` (876), indistinguishable from "zero allowance"; (b) a plan-default quota counts **all-time** usage (`repository.go:525-539`, no period filter), only an allowance policy is periodic (854-874), and no plan template seeds allowance policies `[UNVERIFIED: allowance_revisions.go and PUT /allowances are per-org manual configuration]`; (c) Quarry has no credential: billing-core's static `BILLING_CORE_SERVICE_CREDENTIALS` (`CP/docker-compose.yml:621`) holds the gateway and auth-core principals, and quarry-edge's token manifest has no billing-core row (`CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json`, no billing audience) |

### 2.2 F-t2-025 (capacity)

| Claim | Verdict | Evidence |
|---|---|---|
| The gateway has one IP-keyed 600 rpm class; per-org inner limiters exist on mcp, agents_runs, eval, memory | OK, with a precision **[DRIFT]** | `GW/rate_limit.rs:58` (600), global layer `main.rs:147-148`. The inner "per-org limiter" in `domains/mcp.rs` (route_layer, 54-60) and `agents_runs.rs:49-51` reuses the **same** `rate_limit_middleware` and the **same** `RateLimiter` extension (`rate_limit.rs:350-376`), so it is one shared 600 rpm bucket keyed `org:<id>` across all those routes, not a class. The only real separate class is `mail_task_rate_limiter` (`GW/domains/mail_task.rs:392-393`, key `mail-task:<org>:<user>`) |
| Distributed buckets are namespaced per key only | **[DRIFT]** matters for design | `distributed_key(key)` is `verevon:gw:ratelimit:{key}` (`rate_limit.rs:260-262`) with no class or rpm segment. A second limiter instance that reused the same key string would share the Redis bucket under a different capacity argument. Class limiters must put the class into the key |
| Quarry capacity is a process-global lease pool | OK `[READ]` | `RuntimeLeasePool::new(4)` Browserbase and `new(2)` local Chromium (`QE/main.rs:323, 337`), semaphore plus host-affinity lease map (`QR/lease_pool.rs:34-60, 96-157`); `metrics_snapshot` (53) has no production caller. Agent runs use a separate un-pooled `ChromiumoxideDriver` (`main.rs:1284-1290`) and the `agent_runs` map has no capacity check (D2 covers it) |
| Backlog belongs at the quarry-control jobs table, not `postgres_queue.rs` | OK, with a new hard constraint **[DRIFT]** | the dispatcher lists `GET /v1/jobs/?limit=50` (`QO/jobs/dispatcher.go:342`) which is `jobsStore.List`: `ORDER BY created_at DESC LIMIT 50` across **all orgs** (`QCT/internal/store/pg/resources.go:241-250`). A job deliberately held in `accepted` falls out of that window as newer jobs arrive and is starved forever. Any backlog design must replace this poll |
| Job admission seam | OK | `QCT/internal/resources/resources.go:396-458` (`createJob`: stamps `accepted`, no cap or backlog check; org from the edge-verified query param); status flips `accepted -> running` by the dispatcher PUT, terminal states by events (`resources.go:300-330`); per-org running count exists as a read model (`QCT/internal/store/pg/resources_v2.go:102-135`, `status = 'running'`). Migrations: last is `014_fleets.sql` (D4 claims 015) |
| Model Plane has a per-org pattern to copy | OK | `MP/.../model-gateway/src/org_quota.rs` (service credential read of org-core, fail open); `MP/.../browser-broker/internal/server/ratelimit.go` is per peer, not per org (card) |

### 2.3 F-t2-046 (metering)

| Claim | Verdict | Evidence |
|---|---|---|
| Five idempotent metrics over NATS | OK `[READ]` | `QR/usage.rs:88-94`; event id `quarry:<request_id>:<metric>` (74); publish is a `tokio::spawn` fire-and-forget (144-179), so an exit loses in-flight publishes (matters for drain) |
| Extract and map are billed as `quarry.search.query`; extract meters every attempted target | OK `[READ]` | `QE/extract_routes.rs:344-360` (`SEARCH_QUERY`, quantity `targets.len()` after the loop, even when items failed; the rate-limited early return at 333-342 skips it); `QE/map_routes.rs:293-310` (`SEARCH_QUERY`, 1.0) |
| The scrape event puts the final URL in metadata even under ZDR | OK `[READ]` | `QE/routes.rs:488-509`: `url`, `fingerprint`, `user_id`, `status`, `zdr`, privacy fields, unconditionally. D4 slice S-4 removes `url` and `fingerprint` for ZDR; this design reuses its allowlist and rebases |
| agent_routes emits no UsageEvent | OK | no `.meter(` in `agent_routes.rs` (grep) |
| **New:** crawl seed is metered even when the handoff fails | **[DRIFT]** | `routes.rs:626-660`: `forward_to_orchestrator` result is held in `ack`, the meter runs, then `match ack`. Same for batch (688-718) |
| **New:** crawl pages are unmetered | **[CLAIM-FALSE] (code comment)** | see Section 1, item 4 |
| billing-core accepts only positive quantities and stable ids | OK `[READ]` | `BC/internal/billing/usage.go:11-60`: id pattern `^[A-Za-z0-9][A-Za-z0-9._:-]*$`, at most 128 bytes, `quantity > 0` (a zero-second event is rejected), metadata at most 16 KiB, depth 8, 512 nodes. New event ids must satisfy this (run ids and segment numbers do) |
| Plan keys exist for browser | OK (absent) | `BC/internal/billing/defaults.go:61-92` (`api_calls`, `users`, `storage_mb`) and org-core `types.go` default quotas (same three); no Quarry key. Lago billable-metric codes exist nowhere in the repo (`BILLING.md:56-64` is the only contract) |
| Surface tag from the verified principal | feasible `[READ]` | `QE/auth.rs:62-91, 99-103, 148-174` (`principal_type`, `service_id`, `is_service`) |

### 2.4 F-t2-029 (metrics, statistics, drain)

| Claim | Verdict | Evidence |
|---|---|---|
| The recorder is inert and no `/metrics` exists | OK `[READ]` | `QE/telemetry.rs:6-14`; `metrics = "0.24"` and `metrics-exporter-prometheus = "0.16"` declared (`Q/Cargo.toml:114-115`, `QE` crate `Cargo.toml:57-58`); no `metrics::counter!` call site anywhere in `Q/crates` (grep), so nothing is falsely instrumented |
| Graceful SIGTERM exists at the HTTP layer | OK, with a **[DRIFT]** that changes the design | `main.rs:84-117` plus `axum::serve(...).with_graceful_shutdown(...)` at 1513-1518. axum waits for open connections; the edge serves long-lived SSE and WebSocket streams (frames, events), so the graceful phase does not finish while a viewer is connected and the container is killed by Docker's default 10 s (`grep stop_grace_period` in the Ingestion compose is empty). Leases and `agent_runs` are never drained; usage publishes are fire-and-forget |
| The orchestrator cancels a root context on SIGTERM | OK | `QO/cmd/orchestrator/main.go:119-131` |
| No Quarry Prometheus job and no rules | OK `[READ]` | `CP/observability/prometheus.yml:1-47` (jobs: prometheus, org-core, billing-core, user-core, session-core, audit-core, verevon-gateway; no `rule_files`); no Alertmanager in any observability directory. Reachability is **not** a problem: the Control Plane Prometheus joins `inter-plane-bus` (`CP/docker-compose.yml:1081`) and quarry-edge is on `ingestion-net` and `inter-plane-bus` (`IP/docker-compose.yml:229-231`) |
| Go services expose no metrics | OK | no `promhttp` in `Q/services` or `Q/pkg`; only `pkg/quarryotel` (`Init`, W3C propagator) |
| Stale docs claim metrics are wired | OK `[READ]` | `Q/docs/PLAN.md:359` ("already wired in edge"), `Q/docs/GAP.md:257` (`/metrics` shown as done); `gap-quarry.md:175` is the OTEL row `[UNVERIFIED: not the metrics row; recheck at edit time]` |
| Docs claim budgets are enforced | OK `[READ]` | `Q/docs/WEB_AGENT_ROADMAP_EXECUTION.md:49-53` says `max_cost_usd` is enforced via the flat table and `AgentLoop`; the HTTP path enforces neither (2.1). Correct in S1 |

### 2.5 Uncommitted work in the area (conflict check)

| File | Tag | In-flight diff (hunk positions, `git diff -U0`) | Conflict with this work |
|---|---|---|---|
| `QE/agent_routes.rs` | `[C]` | none | none; but D2's split and S3/S4 land first (Section 5) |
| `QE/routes.rs` | `[M]` | 1 line in the test module at 1329 | none. Crawl, batch, scrape meter blocks (488-509, 626-725) are untouched |
| `QE/extract_routes.rs` | `[M]` | hunks 205-208, 228, 512-563 (+57/-3, tests) | meter block 344-360 is untouched; rebase the test-module addition |
| `QE/main.rs`, `config.rs` | `[M]` | D2 recorded hunks at main.rs 47, 244, 984-1201, 1232, 1395, 1463; config.rs +187 | **config.rs avoided** (limits in new `QE/limits.rs` read from env, D2's D13 rule); main.rs touched at the mod list, `new_runs_with`, the listener block 1513-1518 and the pool block 323/337: adjacent to hunks, rebase do not merge blind |
| `QE/state.rs`, `test_support.rs`, `tests/support/mod.rs` | `[M]` | state.rs hunks 57-93 (struct fields, +13/-8); test harnesses one-line additions | S3 adds one `AppState` field (`limits`) near `usage` (line 101) and one literal line in each harness; adjacent to hunk 88-93, rebase |
| `QE/lib.rs`, `QR/lib.rs` | `[M]` | lib.rs line 18 (`pub mod`), runtime lib.rs lines 42 and 160 (re-exports) | additive `pub mod` and re-export lines |
| `Q/docs/openapi.yaml` | `[M]` | +78 lines at 238-250 and 4012-4150 | this design edits crawl at about 4361-4374 and agent paths 263-345: no overlap |
| `IP/docker-compose.yml` | `[M]` | hunks 121-160 (switch defaults) | S9 adds `stop_grace_period` and S8 a metrics port and token on the quarry-edge service block; adjacent to hunk 121-160, rebase |
| `SPA/shared/i18n/errors.ts` | `[M]` | +12 at 11-16 and 190-196 | additive keys only |
| `QO/*`, `QCT/*`, `BC/*`, cost-core, broker, gateway Rust files | `[C]` | none | clean base. Gateway `rate_limit.rs`, `main.rs`, `browser.rs`, `quarry.rs` are `[C]` |
| `CP/auth-core/.../cross-plane-token-requests.json` and Quarry's canonical copy | `[M]` | 28 fixtures uncommitted | **not touched**: Quarry reads billing-core through billing-core's own static credential registry (D7 below), avoiding the manifest and its vendored copies |

## 3. Design decisions

Decisions are numbered DEC-1 to DEC-14 (D2 uses D1 to D13; this keeps them apart). "ADR-GATED" marks a decision that needs a user or ADR answer before the **named follow-on work**; none blocks the first slices, because each ships a conservative default (Section 8).

### Assumed interfaces from D2 (F-t2-006, F-t2-013 phase 1, split of `agent_routes.rs`)

D2 is a sibling design; this design relies on exactly these, and each is a hard interface request to D2's author (module names are D2's post-split names; if S2 of D2 is skipped, the same symbols live in `agent_routes.rs` at the ranges in Section 2):

| # | Assumed from D2 | Used by this design for |
|---|---|---|
| A1 | `RunLive` (lock-free, atomics) stored beside the entry in `RunHandle { entry, live }`, holding `started_at`, `class: RunClass { Agent, Interactive }`, `org_id`, `taken_over_at_ms`, `last_activity_ms`, `viewers`, `in_flight` | accumulators and caps live here so the sweeper, projections and the step handler read them without taking the entry mutex |
| A2 | `admit_action(state, handle, entry, AdmitInput) -> QuarryResult<Admitted>`: one ordered gate chain shared by REST step, WebSocket and tab routes, pinned by test `admission_order_is_pinned` | `enforce_budget` replaces D2's gate 8. **Request:** move the budget gate *before* the sensitive-approval gate. Today the one-shot approval is consumed (`used_sensitive_approval_grants.insert`, `agent_routes.rs:2282-2291`) before the budget check (2294), so a budget-refused action burns its single-use approval. **Conflict to resolve:** D7 pins `gate_order_is_policy_before_approval_before_budget` (D7 sections 3 and 4), which keeps that flaw. Proposed canonical order, side-effect-free gates first: owner, grant, capability proof, raw-script deny (D2 S1), policy (D7), takeover and initiator (D2 S3), **budget (this design)**, sensitive approval (the only gate that consumes something), navigation. Question 6 |
| A3 | `RunRegistry` with `try_reserve(org_id, class)` before `agent_driver.acquire`, `RunSlot`, `RegistryStats`, `ViewerGuard`, `InFlightGuard`; `RunLimits::from_env()` in `QE/run_limits.rs`; typed 429 with `details.reason` in `org_run_cap`, `org_interactive_run_cap`, `global_run_cap` plus `Retry-After` | plan-aware per-org caps (S6) and the metrics gauges (S8). **Request:** `try_reserve(org_id, class, org_override: Option<OrgRunCaps>)` so a resolved plan value can replace the env per-org number (never above the global cap) |
| A4 | `finish_run(state, handle, CloseReason)` as the single release path (explicit close, sweeper expiry) | one place to emit usage and the budget summary. **Request:** add `CloseReason::Drain` (S9) |
| A5 | `StepInitiator { Human, Agent, System }` and `derive_initiator(claims, hint)` | receipts keep recording it; budgets do **not** key on it (DEC-1) |
| A6 | `RunClass` derived at `start_run` from `claims.is_service()` | **Request:** extend the rule to "service principal, or grant kind `run` (D1), or body hint `initiated_by: "agent"` (tighten-only)". If F-t2-001's delegated bearer ever makes execution-core call `start_run` with a user token, a pure principal rule would class an AI run as `Interactive` and skip every budget |
| A7 | Error-code mirror checklist (D2 S3: `error.rs`, contract test, `fallback_driver.rs:587`, Go `envelope.go`, CONTRACTS, openapi, model-gateway maps) | I add one code, `BUDGET_EXHAUSTED`, and walk the same list |
| A8 | `EventType::AgentExpired`, `agent.expired`; `emit_for_zdr` for live-only events | I add `AgentBudgetExhausted` / `agent.budget_exhausted` through the same four places |
| A9 | Migration `0010_step_receipt_initiator_epoch.sql` takes the next Rust migration number; no new `AppState` field; limits read from env in a new file (not `config.rs`) | my receipt column is `0011`; I need **one** `AppState` field (`limits`), flagged in Section 5 |

### DEC-1. The budget subject is the run class, not the per-step initiator

- Options. (A) Key on the per-step initiator (D2 S3 hands `initiator` to the budget function). (B) Key on the run class fixed at `start_run`. (C) Stop the gateway passing `max_steps 12` and `max_runtime_s 120` for manual sessions.
- **Recommendation: B, and leave C to F-t2-011.**
- Rationale. A budget is the cap of the authority a *run* was granted, set by the principal that started it. The gateway's `KnowledgeComposer` apply path labels a few *steps* of a user's own manual session as agent actions (D2 D2); applying the manual session's 12-step and 120 s constraints to those steps would resurface the exact bug this slice removes. Under B: `Agent` runs are budgeted; `Interactive` runs are **never** capped by steps, runtime or cost (they have D2's idle TTL, takeover TTL and absolute backstop instead). The gateway literals become inert for Interactive runs; deleting them touches `GW/domains/browser.rs:483-488`, which is F-t2-011's file, so this design does not.
- Metering is separate from capping: Interactive seconds are still metered (DEC-10).
- ADR-GATED: topic 3 (shared human and agent sessions), for exactly one follow-on: whether org entitlement exhaustion may ever stop a human session. Default shipped here: **never**.

### DEC-2. Typed error `BUDGET_EXHAUSTED`, HTTP 402, non-retryable, latched

- Options. (A) New `ErrorCode::BudgetExhausted` with HTTP 402. (B) Reuse `RATE_LIMITED` (429) with `details.reason`. (C) Reuse `FORBIDDEN`.
- **Recommendation: A.** 429 means "retry later", which `model-gateway` maps to `resource_exhausted` and execution-core treats as backpressure (D2 section 7); a spent budget never recovers by waiting. 402 matches the existing billing-gate convention (`GW/domains/leads.rs:105`, billing-core `entitlement` 402 at `BC/internal/http/handlers.go:150-155`) and is an HTTP status the gateway forwards untouched (`forward_quarry_failure`, `GW/domains/browser.rs:3873-3885`). Low-stakes; the user may prefer 429 and the change is one line in `error.rs`.
- Wire: `ErrorCode::BudgetExhausted` serialises `BUDGET_EXHAUSTED`; `http_status` 402; `retryable()` false. `details`: `{ "dimension": "steps" | "attempts" | "runtime" | "cost" | "org_allowance", "limit": 12.0, "spent": 12.0, "unit": "steps" | "seconds" | "usd", "estimated": true, "limit_source": "request" | "grant" | "org_default" | "ceiling" | "entitlement", "resumable": true }`. No URL, title or content.
- **Latch.** The first refusal sets `RunLive.exhausted = dimension` (CAS, once). From then on every action-bearing request on that run (step, WebSocket action, tab open) is refused with the same error without re-evaluating, so a planner cannot probe past a cap by sending a cheaper action. Reads, projections, receipts, `DELETE` and `finish_run` keep working. One `agent.budget_exhausted` event is emitted per run segment.
- **Graceful stop.** Admission-time refusal is the "finish the step and persist partial results" the card (from Apify, a Verevon proposal) asks for: the in-flight action completes, receipts and the checkpoint persist, the run stays `Active` and resumable.
- ADR-GATED: none. The mirror list is D2's (A7).

### DEC-3. What a run's cost is: flat action estimate plus an estimated browser-second term, failures included, always labelled estimated

- Options. (A) Keep the flat action table alone. (B) Add a browser-time term. (C) Pull a price table from billing-core now.
- **Recommendation: B now, C later (design-level L4).**
- Rationale. Section 2.1: the flat table alone cannot bind a realistic cap. The time term is `active_seconds * QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD` (placeholder `0.00003` USD per second, about 0.11 USD per hour; **must be set from the billing-core price table before `on`**). Failed attempts charge the flat estimate for their action (the browser did the work; the receipt records it, fixing the zero-cost failure receipts). `provider_billed` stays in the precedence code (`verified_action_cost_micro_usd` first) but has no producer, so every figure is `estimated: true` and `cost_source: flat_table` on receipts and in `meta.budget`.
- Plane rule kept: these are **budget-estimator rates**, not prices. Prices stay in billing-core and Lago (`F-t2-046` constraint). The estimator may under- or over-state the invoice; a budget is a safety stop, not an invoice, and the docs say so.
- Model spend is **not** in Quarry's number (DEC-9).
- Units: micro-USD in `u64` atomics (matches the receipt column `cost_micro_usd`, `QR/step_receipts.rs:513`).

### DEC-4. Dimensions, clocks, overshoot bounds and resume

- Steps: completed steps (`ctx.step`, unchanged meaning) against `max_steps`, **plus** an attempts allowance: admitted attempts (success or failure) against `max_steps * QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR` (default 2; an absent `max_steps` takes the agent default first). This closes the failure-loop hole (Section 2.1).
- Runtime: **active** clock = `now - started_at - paused_total`, where paused time is time spent in `HumanTakeover` (D2's `taken_over_at_ms`). Approval wait is *not* excluded, because Quarry cannot see it (execution-core pauses on `WaitingApproval` while the Quarry run idles). That is a documented limitation; F-t2-033 (approval UX) decides whether a `POST .../budget/pause` is worth adding. D2's sweeper keeps its own absolute deadline (`explicit max_runtime_s + grace`), which is wall time; this design extends D2's pure `verdict(now)` with the same active-clock helper rather than a second timer.
- Cost: **projected** check before execution, `spent + estimate(action) > cap` refuses; charge after execution with the same estimate. Overshoot is bounded by one action's estimate error and, for time, by one action's duration (driver deadlines are F-t2-026).
- Evaluation point: `admit_action` gate, before sensitive approval (A2), so a refusal consumes neither budget nor approval. It runs in the REST step, `/interact` alias, WebSocket and tab paths because they share D2's chain.
- Resume: constraints from the checkpoint stay authoritative (existing rule, `agent_routes.rs:1362-1368`), and **so do spent, attempts and active time**: `AgentRunCheckpoint` gains `spent_micro_usd`, `attempts`, `active_ms`, `budget_exhausted` (all `#[serde(default)]`). Without this a caller could refill any budget by expiring the run and resuming it. To get more budget the caller starts a new run, which resolves a fresh budget.
- ZDR: accumulators are memory-only; the checkpoint write already skips ZDR (`agent_routes.rs:2391, 3180`), so a ZDR run has no durable budget state and cannot be resumed anyway (1350-1356).

### DEC-5. Resolution, defaults, ceilings, and "never silent"

- Rule (Agent class): `effective = min over the present values of { request, grant claim, org default, entitlement remaining, Quarry ceiling }`. A request value of `0`, `None` or negative counts as **absent**, and an absent request takes the **agent default**, not "unlimited". Present values only tighten (the claim, the org default and the entitlement can lower the request, never raise it).
- Quarry defaults and ceilings (env, placeholders to be ratified, Question 3): `AGENT_DEFAULT_MAX_STEPS 60`, `AGENT_DEFAULT_MAX_RUNTIME_S 900`, `AGENT_DEFAULT_MAX_COST_USD 1.00`; ceilings `AGENT_CEILING_MAX_STEPS 300`, `AGENT_CEILING_MAX_RUNTIME_S 3600`, `AGENT_CEILING_MAX_COST_USD 20.00`. Ceilings equal D2's `AGENT_RUN_MAX_TTL_S` order of magnitude (1800 to 3600) so the sweeper and the budget agree.
- **Returned, never silent.** `POST /v1/agent/runs` response `StartRunData` gains `constraints` (the effective values) and `clamped: [ { field, requested, effective, by } ]` with `by` in `default | ceiling | grant | org_default | entitlement`. `meta.budget` on every step response restates the live numbers. Model Plane and the SPA must show `clamped` (sibling b, Section 7). Crawl and batch clamps follow the same shape (DEC-11).
- Interactive class: no caps are resolved (DEC-1); the request's constraints are stored in the checkpoint as before.

### DEC-6. Fail policy for the entitlement read: fail-to-floor (ADR-GATED, topic 1)

- Options. (A) Fail open (what Firecrawl's limiter and `MP/.../org_quota.rs:106-111` do: Control Plane down means no ceiling). (B) Fail closed (refuse every browser start and crawl when the read fails). (C) **Fail to floor**: when the live read fails, use the last good value for up to 15 minutes, then the Quarry static floor (defaults and ceilings above, per-org caps from env).
- **Recommendation: C.** A is rejected because it removes the only protection exactly during an outage and the audit's risk 6 is unbounded cost; B is rejected because it turns an unrelated Control Plane outage into a browsing and crawling outage for every tenant (the same reason `org_quota.rs` gives for failing open). C never grants more than the static floor allows and never blocks outright.
- Asymmetry, by design: for **budgets and page limits** an entitlement can only tighten the floor. For **capacity** (per-org concurrency) a higher plan may raise the per-org number above the env default but never above the global safety cap (A3).
- Limits of C, stated: an org whose allowance is exhausted keeps working at the floor while billing-core is unreachable. For orgs that must hard-stop (prepaid), the later per-feature switch is "closed" for that key; the `LimitsResolver` interface carries a `fail_mode` per key so that is a data change.
- ADR-GATED: topic 1 (web policy of record), the "fail-open ADR" the audit asks for F-t2-025. Shipping default is C with `QUARRY_EDGE__LIMITS_FAIL_MODE=floor`; `open` and `closed` are accepted values.

### DEC-7. Control Plane read API: reuse billing-core `quotas/:metric`, one new credential, one additive field

- Options. (A) New read endpoint in org-core. (B) Reuse billing-core `GET /api/v1/billing/orgs/:orgId/quotas/:metric` and `.../entitlements/:feature`. (C) Put limits in tokens (claims) via Auth Core.
- **Recommendation: B for the pull path (S3); C is D7's (F-t2-022) push path and plugs into the same resolver.**
- Why B. The endpoints and scopes exist (Section 2.1); the gateway already gates features this way (`GW/domains/leads.rs:80-113`). Quarry gets one new principal `quarry-edge` in billing-core's static registry (`CP/docker-compose.yml:621`) with `billing:quota:read:any` and `billing:entitlement:read:any`. This avoids the Auth Core manifest, which is uncommitted-modified and vendored into seven trees (S9 hotspot list), and avoids any cross-plane database read. `[UNVERIFIED]` billing-core's startup check requires exactly one gateway and one auth-core principal but tolerates others (`service_auth.go:171-222`: it only counts those two); confirm with a boot test in S3.
- Two billing-core gaps fixed in S3: (1) `QuotaStatus` gains `configured bool` (additive) so "no ceiling configured" is distinguishable from "zero allowance" (today both return `limit 0`, `service.go:876`); (2) capacity numbers are plan-default keys `quarry.cap.*` in `defaultQuotaLimitsByPlan` (they are limits, usage is irrelevant), while **periodic** usage allowances (`quarry.crawl.page`, `quarry.browser.second.agent`) must be allowance policies, because plan-default quotas count all-time usage (`repository.go:525-539`). Plan templates for allowance policies are a Control Plane decision (Question 4); until one exists the clamp-to-remaining activates only for orgs that have a policy and every other org gets the Quarry floor.
- Cache: per `(org, metric)` 30 s (billing-core itself caches usage 30 s, `service.go:878-892`), stale up to 15 min on error (DEC-6).

### DEC-8. The grant-carried budget claim is minted by model-gateway; cost-core is not written in v1

- Options. (A) The broker computes remaining budget (needs billing and cost credentials it does not have). (B) cost-core computes it (cannot, Section 2.1). (C) model-gateway computes it at the run-grant derivation and the broker stores it verbatim. (D) execution-core computes it before `start_run`.
- **Recommendation: C.** model-gateway already holds both inputs (`org_quota::fetch_org_limits` for the org-core per-run ceiling, `budget.rs` for cost-core), is where D1 puts `DeriveRunGrant` (`browser_run.rs`, 337 and 348), and the broker stays a pure grant store.
- Proto: fields `40-44` (D1's convention reserves 40-49 for F-t2-008): `int64 budget_micro_usd = 40; int32 budget_max_steps = 41; int32 budget_max_runtime_s = 42; string budget_source = 43; int64 budget_minted_at_unix = 44;` on `DeriveRunGrantRequest`, `DeriveRunGrantResponse`, `AcquireGrantRequest`, `AcquireGrantResponse` and `ValidateGrantResponse`. Zero or absent means "no claim". Quarry reads them from `GrantValidation` (new `budget: Option<GrantBudget>`), snapshots at `start_run`, and never re-widens them when the grant is revalidated before each action.
- **One ledger key per run, without a cost-core write.** Spend classes are disjoint: model spend in cost-core keyed by `run_id` (`GET /api/v1/cost/run`), execution spend in Quarry keyed by `run_id` and segment. The run view (F-t2-014) joins them by `run_id`; billing of execution spend is Quarry's NATS event only (id `quarry:<run_id>:<segment>:<metric>`), billing of model spend is cost-core's outbox only. No path writes the same spend twice, which is the risk the card names. An optional later entry (design-level L5): execution-core records one cost-core row per run segment, `idempotency_key = quarry-run:<run_id>:<segment>`, with a new cost basis `execution` that `billingUsagePayloads` already excludes (`billing_delivery.go:46`, it returns nil for any basis other than `metered` and `subscription`); it needs `NormalizeCostBasis` and a CHECK-free column (`0005_cost_basis` adds a plain text column) change only.
- ADR-GATED: topic 1 (grant field versus org-core claim). If the policy ADR picks capability-core as owner, only the *minter* moves; the Quarry reader `GrantBudget` and the proto fields do not.

### DEC-9. What `max_cost_usd` caps on each plane

Quarry's `max_cost_usd` caps **execution spend** (browser actions, browser time, paid providers when added). Model Plane caps **planner spend** with cost-core and enforces the user-facing **combined** cap in execution-core, using Quarry's `meta.budget.spent_usd` plus `cost-core GET /api/v1/cost/run?run_id=`. Documented in openapi so a caller does not read one number as the other. Today execution-core forwards the user's number to Quarry and checks nothing itself (`browser_agent.rs:320-331, 666`); that is sibling (b)'s change (Section 7).

### DEC-10. Metering: names, attributes, event identity, and when events are emitted

- Metric names (free-form strings; each needs a Lago billable-metric code **before** it is enabled, `BILLING.md:56-64`): `quarry.browser.second.agent`, `quarry.browser.second.interactive`, `quarry.agent.run`, `quarry.agent.step`, `quarry.crawl.page`, `quarry.extract.url`, `quarry.map.call`. Existing five unchanged. Two browser-second metrics, not one with a property, because billing-core quotas and allowances key on the metric string (`allowance_repository.go:189-210`), so plan keys can treat human and agent time differently.
- Emission: browser-second, run and step events are emitted **once per run segment in `finish_run`** (explicit close, sweeper expiry, drain), quantity = whole seconds `ceil(lease_seconds)`, steps = completed steps; a zero quantity is skipped (billing-core rejects `quantity <= 0`). A segment is one acquire to one release; a resumed run is a new segment (checkpoint `segment` counter), so event ids `quarry:<run_id>:s<segment>:<metric>` are unique and idempotent. Crash loses the in-memory segment (non-ZDR: a boot-time settle of orphaned `Active` checkpoints, design-level S9b; ZDR: accepted under-billing, documented, because nothing durable may exist).
- Crawl pages: `quarry.crawl.page` is emitted in `internal_run_page` on success, **only** for pages whose activity passes `kind = "crawl"` (new field on the orchestrator's `RunPageInput`), id `quarry:<run_id>:<sha256(url)[:16]>:quarry.crawl.page`. Batch pages are already billed upfront as `quarry.batch.url` (qty = URL count), so emitting a page event for them would double bill. Seed and batch events are emitted **only after a successful handoff** (today they fire on failure too).
- Extract bills successes only (`quarry.extract.url`, items with a fetched or extracted result, failures free as in Firecrawl); map is `quarry.map.call`. Neither is billed as a search query any more.
- Attributes (closed vocabularies, in `metadata`): `outcome` in `ok | error | refused | cancelled | timeout | budget_exhausted | expired`; `driver`; `class` in `agent | interactive`; `surface`; `zdr` bool; `estimated` bool; `paid_provider` bool; `llm_format` bool. `surface` is derived server-side from the verified principal: a user token yields `user`; a service token yields a value from a static `service_id -> surface` table, and a request header `x-verevon-surface` is honoured **only** on service tokens and **only** if its value is in the closed enum `{ chat, browser_ui, space, api, schedule, mcp, internal }` (`[UNVERIFIED]` the service id list: take it from `CP/config/plane-service-principals.json` at implementation). A user token can never set it.
- ZDR: under ZDR the metadata is an **allowlist** (`zdr, outcome, class, surface, driver, estimated, counts, user_id`), mirroring D4 slice S-4 (which removes `url` and `fingerprint` from the scrape event). No URL, host, title, fingerprint, query or content in any event, ever.
- Flag `QUARRY_EDGE__USAGE_BROWSER_METRICS = off | shadow | on` (default `off`); `shadow` logs `usage.shadow metric= quantity= class=` and publishes nothing, so volumes can be checked before Lago codes exist.
- Interactive (human) seconds are **metered**, not capped; whether they are priced is billing-core's table (Question 2, ADR-GATED topic 3 for the product posture).

### DEC-11. Crawl and batch limits: immediate fix, then clamp-to-remaining with the value returned

- S0 (immediate, independent of every entitlement): defaults and hard ceilings applied at three places, **authoritative at the edge**, defence in depth in the orchestrator. Numbers (env, placeholders, same names on both sides so one compose anchor sets them): `QUARRY_CRAWL_DEFAULT_MAX_PAGES 1000` (equals the existing unused Rust `FrontierConfig` default, so no onboarding surprise), `QUARRY_CRAWL_HARD_MAX_PAGES 5000` (equals the gateway clamp), `QUARRY_CRAWL_DEFAULT_MAX_DEPTH 5`, `QUARRY_CRAWL_HARD_MAX_DEPTH 10`.
- Workflow safety: the loop change must go behind `workflow.GetVersion` because Temporal replays history against new code; the workflow itself cannot read env (non-determinism), so it uses a package constant fallback and the dispatcher passes the resolved value in `CrawlJobInput`.
- Return, never silent: `HandoffAck` gains `limits.max_pages { requested, effective, clamped_by[] }` and `limits.max_depth {...}`; the job `params` store the **effective** value and `max_pages_requested`; the gateway surfaces `limits` next to `upstream` in its normalised crawl response (`GW/domains/knowledge/quarry.rs:355-373`).
- S3 adds the entitlement term: `effective = min(request-or-default, hard, remaining(quarry.crawl.page))`; `remaining == 0` returns 402 `BUDGET_EXHAUSTED` with `dimension: org_allowance` **before** a job is created. Batch: accept the first `N` URLs that fit `min(cap 200, remaining(quarry.batch.url))`, return `urls_accepted`, `urls_dropped` and `clamped_by` (counts only, no URLs); never drop silently, never reject a partly-fitting batch.
- The card's "request rate" and "max concurrency" clamp: the workflow is sequential (Section 2.1), so there is no per-job concurrency to clamp; per-host rate stays with `HostScheduler` (AIMD). What is clamped is job-level concurrency (DEC-12).

### DEC-12. Capacity architecture: three layers, numbers owned by Control Plane, counters honest about their scope

1. **Gateway (abuse protection and classes).** Class-aware, org-keyed limiters built from the existing limiter with the class in the Redis key. Classes and starting rpm (env, per org): `interactive` (browser actions, frames, polls) 1200; `browser_start` (session create, AI run start) 30; `job_submit` (crawl, batch, schedule create) 20; `search_scrape` 120; default unchanged 600. Human browsing is never throttled by agent or job classes because the classes are separate buckets (the audit's risk). No new `AppState` field: classes derive from `state.rate_limiter.with_class(...)` inside each domain `router()`.
2. **Quarry per-org permits.** Agent runs: D2's registry (A3) with the plan-aware override. Scrape/crawl rendering: a per-org permit wrapper in front of `RuntimeLeasePool` (the pool is host-keyed with a global semaphore; `org_id` is empty on the internal lease, `browser_driver.rs:138-142, 160-163` per card, so the adapter acquire must carry the org first). Per-org permit cap default `max(1, pool_size / 2)`. Over-cap returns `RATE_LIMITED` with `details.reason = "org_render_cap"`, `Retry-After`.
3. **Job admission, backlog, fair share (quarry-control and the dispatcher).** Not in `postgres_queue.rs` (per-crawl queues). The job table already has the backlog: `accepted`. What it lacks is an admission rule and a fair poll (Section 2.2). Design: a server-side **claim** endpoint in quarry-control (`POST /v1/internal/jobs/claim`) that returns the next batch of `accepted` jobs ordered by per-org rank (round robin across orgs) and filtered to orgs below their running cap; the dispatcher calls it instead of the newest-50 list. Create-time backlog cap per org returns 429 `RATE_LIMITED` `org_backlog_full` naming the limit; a janitor fails jobs older than `JOB_BACKLOG_TTL` (default 24 h) with `reason: backlog_expired`.
- Numbers come from billing-core `quarry.cap.*` keys through the DEC-6 resolver; org overrides are billing-core allowance or account configuration (Control Plane owns the numbers, Quarry enforces). The static floors are env.
- **Honest scope limit.** All Quarry counters are in-process. The compose stack runs one quarry-edge container (`IP/docker-compose.yml:216-231`), so per-process equals per-fleet today. With more than one edge replica the per-org caps become per-replica; the job layer (3) is shared because it lives in Postgres. Moving edge counters to a shared store is an explicit non-goal until a second replica exists.
- ZDR: backlog rows hold URLs, but `/v1/crawl` and `/v1/batch` are refused under ZDR (`QE/routes.rs:555-588`), so no ZDR row can exist; the guard stays and a test pins it. Agent-run and permit counters hold no content.

### DEC-13. Observability order and label policy

- Order (each is independently shippable): (1) recorder plus internal `/metrics` (S8); (2) drain of leases, streams and usage publishes (S9); (3) per-run statistics (S10); then design-level: (4) memory-aware admission together with D3's multi-Chromium pool; (5) span vocabulary after F-t2-004 phase 2 (ZDR sampler); (6) alert rules and the Alertmanager decision.
- `/metrics` is on a **separate listener** (`QUARRY_EDGE__METRICS_ADDR`, default `0.0.0.0:9092`, **not published to the host**), with a bearer token (`QUARRY_EDGE__METRICS_TOKEN`) so a stray route cannot expose it, following the shape of integration-corev2's `/metrics` behind internal auth (`IP/integration-corev2/internal/api/server.go:178-189` per card). Prometheus scrapes `quarry-edge:9092` over `inter-plane-bus` with `authorization.credentials_file`.
- Label policy (hard rule, pinned by a test): labels come from closed sets of at most 12 values; **never** a URL, host, org id, user id, run id or message. `route` is the matched path template.
- Per-run statistics: for crawl jobs a `statistics` JSON on the job record (counts, status-code classes, error classes, retry histogram), set from the terminal event; for agent runs a content-free `usage` object in the `DELETE` response and `meta.budget`. Under ZDR counts only, no signatures.
- ADR-GATED (new, not in the audit's ten): where Prometheus rules and an Alertmanager live; the audit notes none exists. Recommendation: infrastructure rules in the Control Plane Prometheus (`rule_files`), user-facing alerts delivered through quarry-control's existing notify sink to notification-core (`QCT/internal/notify/sink.go`).

### DEC-14. Sequencing against the F-t2-022 claim channel (D7)

What ships **before** D7 and needs nothing from it: S0, S1 (resolution is `min(request, org default = static env, ceiling)`, the grant claim and entitlement terms are `None`), S2, S5, S8, S9, S10. S3 (billing-core pull plus clamp) is the *alternative* delivery of org numbers; it ships without D7 and D7 later adds a second source behind the same `LimitsResolver` (precedence: grant claim, then billing pull, then floor). S4 (grant claim) needs D1's `DeriveRunGrant` and proto reservation, not D7; if D7's ADR (topic 1) chooses capability-core, only the minter moves. Nothing here blocks D7, and D7 blocks nothing here. D7's v1 claim carries no numeric limits (Section 5.2), so the resolver's claim slot is empty until D7 or a later change adds a `limits` layer.

## 4. Slices (PR-sized, in build order)

Build order: **S0** (crawl cap fix, immediate and independent) first; **S1** (budget enforcement) as soon as D2's S3 skeleton exists; **S2** (metering) next, because S3's clamp needs usage events to subtract; **S3** (limits resolver and Control Plane read) then **S4** (grant claim); **S5** (gateway classes), **S8** (metrics) run in parallel with all of them; **S6, S7** (capacity) after S3; **S9, S10** after D2's S4 and S2. Design-level, unscheduled work is listed after S10.

Conventions for every slice: Rust Quarry tests run in the `rust:1` container recipe (crates do not build on the Windows host; `bash -c`, cache volumes, more than 10 minutes for the full set); Go commands per module; Model Plane Rust on Windows uses `--lib`; Model Plane Go has no CI (S9 H8), so each Model Plane slice that touches Go adds its path to CI in the same PR (`.github/workflows/browser-broker-ci.yml` is D1's; reuse it). A deployment claim needs rebuilt local images, a running stack and the flow below.

---

### S0. Crawl `max_pages` 0 is unlimited, failed pages unbounded, depth unbounded (effort S, 1.5 engineer-days)

**Goal.** Ship the immediate fix independent of entitlements: every crawl has a finite page, failure and depth bound, and the response says what bound applied.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QO/internal/jobs/limits.go` | `CrawlLimits`, `LoadCrawlLimits()`, `Resolve(reqPages, reqDepth) Resolved` | new; env `QUARRY_CRAWL_DEFAULT_MAX_PAGES` (1000), `QUARRY_CRAWL_HARD_MAX_PAGES` (5000), `QUARRY_CRAWL_DEFAULT_MAX_DEPTH` (5), `QUARRY_CRAWL_HARD_MAX_DEPTH` (10); `0` for a default disables the default (emergency rollback), never the hard ceiling; `Resolved{Pages, Depth, PagesBy, DepthBy}` with `By` in `default | hard | none` | [NEW] |
| `QO/internal/jobs/dispatcher.go` | `crawlInputFromJob` (432-454), `Config`, `New` | resolve `max_pages` and `max_depth` through `CrawlLimits` (covers legacy rows and direct control callers); pass resolved values in `CrawlJobInput` | [C] |
| `QO/cmd/orchestrator/main.go` | dispatcher config wiring | `LoadCrawlLimits()` into `jobs.Config` | [C] |
| `QO/internal/workflows/workflows.go` | `CrawlJobWF` (396-512) and `crawlJobDurableFrontier` (518-610): lines 456, 474, 555, 574 and the failure branches 464-469 and 571 | behind `workflow.GetVersion(ctx, "crawl-bounded-v1", workflow.DefaultVersion, 1)`: `MaxPages == 0` falls back to package const `workflowFallbackMaxPages = 1000` and `MaxDepth == 0` to `workflowFallbackMaxDepth = 5` (a workflow must not read env); stop when `pagesFailed >= effectivePages` and emit the terminal event with `reason: "max_failures"` | [C] |
| `QE/limits.rs` | `CrawlLimits`, `CrawlLimits::from_env()`, `resolve(req_pages, req_depth) -> Resolved` | new; same env names as the orchestrator (one compose anchor sets both); `0` request counts as absent; invalid env aborts boot (D2's D13 rule). S3 extends this file with the resolver | [NEW] |
| `QE/lib.rs`, `QE/main.rs` | `mod limits;` | declare in both crate roots, away from D2's `run_limits` line | [M], [M] |
| `QE/routes.rs` | `CrawlRequest` (533-553), `crawl_handoff` (590-667), `HandoffAck` (727-738) | resolve, write the **effective** value and `max_pages_requested` into the forwarded `params`; `HandoffAck.limits: Option<CrawlLimitsAck>` (`#[serde(skip_serializing_if = "Option::is_none")]`) | [M] |
| `QE/handoff.rs` | `forward_to_orchestrator`, the `HandoffAck {..}` literal at 94-98 | the caller attaches `limits` after the ack | [C] |
| `GW/domains/knowledge/quarry.rs` | `start_crawl` (306-376): normalised JSON at 355-373 | add top-level `limits` copied from the upstream ack beside `upstream`; keep the existing 5000 clamp and the "0 is invalid" gateway rule | [C] |
| `Q/docs/openapi.yaml`, `Q/docs/CONTRACTS.md` | `CrawlRequest` (about 4361-4374), crawl section | document defaults, ceilings, `limits`, and that 0 means default | [M], [C] |
| `QO/internal/workflows/workflows_test.go`, `QO/internal/jobs/limits_test.go` | tests | see below | [C], [NEW] |

**API / data model.** `POST /v1/crawl` response `data` additive: `"limits": { "max_pages": { "requested": null, "effective": 1000, "clamped_by": ["default"] }, "max_depth": { "requested": 3, "effective": 3, "clamped_by": [] } }` (`clamped_by` values `default | hard`). Job `params`: `max_pages` becomes the effective value, `max_pages_requested` added. No SQL.

**Flags and rollout.** No new feature flag; rollback is the env: `QUARRY_CRAWL_DEFAULT_MAX_PAGES=0` restores "absent means the hard ceiling" (still finite). In-flight workflows are protected by `GetVersion`. Rollout: deploy orchestrator and edge together (the edge alone is safe, the orchestrator alone is safe).

**Tests.**
- Go workflow (Temporal test env, style of `TestCrawlJobWF_RespectsMaxPages`, `workflows_test.go:200-232`): `TestCrawlJobWF_ZeroMaxPagesFallsBackToCap` (stub `RunPage` returns 5 fresh links per page, fallback var overridden to 7, assert exactly 7 visits), `TestCrawlJobWF_FailuresCountTowardCap`, `TestCrawlJobWF_ZeroMaxDepthFallsBackToDepthCap`, `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour` (uses `workflow.DefaultVersion` through the test env's `OnGetVersion` hook; `[UNVERIFIED]` hook availability in the pinned Temporal SDK, else a `WorkflowReplayer` run on a recorded history).
- Go jobs: `TestResolve_ZeroUsesDefault`, `TestResolve_ClampsToHardAndReports`, `TestResolve_DefaultZeroDisablesDefaultNotHard`.
- Rust unit (`limits.rs`): `zero_is_absent_and_takes_default`, `above_hard_is_clamped_and_reported`, `depth_resolves_like_pages`, `from_env_rejects_unparsable_naming_the_variable`.
- Rust integration `QE/../tests/crawl_limits.rs` [NEW]: harness plus an axum stub on `127.0.0.1:0` standing in for control (`AppState.control_base_url`), asserts the forwarded `params.max_pages`, the ack `limits`, and that a ZDR caller is still refused (`routes.rs:555-588`).
- Gateway: `start_crawl_exposes_upstream_limits` beside the existing quarry.rs tests.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./...`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge crawl_limits limits`; `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" quarry`.

**Definition of done.** No code path starts a crawl without a finite page, failure and depth bound; the ack and the stored job carry requested and effective values; old workflow histories replay; docs corrected.

**Deployment evidence required.** Rebuild the edge and orchestrator images (`make up` with the deploy recipe's env chain). Test-only env in the last env file: `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7`. Flow: (1) through the Verevon gateway `POST /api/v1/knowledge/crawl` with only `url` of a real public site that has more than 7 internal links; response `limits.max_pages.effective == 7` and `clamped_by == ["default"]`. (2) Same direct to the edge (`127.0.0.1:8082`) with `"max_pages": 0`: identical. (3) Watch `/api/v1/knowledge/jobs/{id}/events`: the terminal `run.completed` payload shows `pages_visited == 7`. (4) `GET /v1/jobs/{id}` in control shows `params.max_pages == 7` and `params.max_pages_requested` absent or 0. (5) Before the change, repeat (1) and record that the crawl runs until the 30-minute timeout or exhaustion (pre-fix evidence, bounded by a manual cancel).

**Effort.** S, 1.5 days (the audit says "S"; the version gate, the returned value and tests are the extra day). **Risks.** Workflow non-determinism if `GetVersion` is wrong (mitigated by the replay test); a default of 1000 changes behaviour for callers that relied on unbounded crawls (mitigated: 1000 equals the existing unused frontier default, and the response now says so); the 30-minute execution timeout still ends large crawls without a typed outcome (out of scope, noted for F-t2-017).

---

### S1. Budget accumulator and `enforce_budget` for agent-class runs (F-t2-008 slice 1; effort M, 5 engineer-days Quarry plus 1.5 in Model Plane by sibling b)

**Goal.** Quarry decides, for runs started by a service principal (or labelled agent), when the run has spent its steps, attempts, active runtime or estimated cost, answers with a typed 402, persists partial results, and reports the effective limits and the live numbers. Human manual sessions are never capped by these dimensions.

**File-level change list** (module names are D2's post-split names; D2's S2 and S3 must have landed or the same symbols live in the monolith at the Section 2 ranges)

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QC/error.rs` | `ErrorCode` (9-31), `retryable`, `http_status` (36-61) | add `BudgetExhausted` (`BUDGET_EXHAUSTED`, 402, not retryable); plus D2's `wire_name` | [C] |
| `QC/event.rs`, `QR/nats_event_bus.rs` | `EventType`, `event_type_token` (exhaustive) | add `AgentBudgetExhausted` / `agent_budget_exhausted` | [C], [C] |
| `QC/contracts.rs` | new `BudgetMeta`, `EffectiveConstraints`, `Clamp` | serde types for `meta.budget`, `StartRunData.constraints`, `clamped[]` | [C] |
| `QC/envelope.rs` | `EnvelopeMeta` (15-19) | additive `budget: Option<BudgetMeta>` (`skip_serializing_if`), `Envelope::ok_with_budget(request_id, data, meta)` | [C] |
| `QE/agent_routes/budget.rs` (D2's budget module) | `BudgetCaps`, `CapSources`, `LimitSource`, `BudgetLedger`, `BudgetSnapshot`, `Estimator`, `ResolveInputs`, `resolve_caps(&ResolveInputs) -> Resolved` (pure), `enforce_budget(&BudgetLedger, &BudgetCaps, &Estimator, &AgentAction, Instant) -> Result<(), QuarryError>`, `charge(&BudgetLedger, &Estimator, &AgentAction, Charge) -> u64`, `receipt_cost_usd(obs, action)` (replaces both copies of 709-762 and the WS-only variant), `budget_meta(...)` | new logic; `BudgetLedger` is atomics: `spent_micro`, `attempts`, `completed`, `paused_ms`, `paused_since_ms`, `exhausted: AtomicU8`; cost now = `spent_micro + active_s * browser_second_micro`; `resolve_caps` implements DEC-5 with `ResolveInputs { class, request, grant: Option<GrantBudget>, org_default: Option<..>, entitlement: Option<..>, limits: &BudgetLimits }` | [NEW] (moved from [C] 663-762) |
| `QE/agent_routes/run_live.rs` (D2) | `RunLive` | add `budget: BudgetLedger`, `caps: BudgetCaps`, `segment: u32`; `restore(&BudgetSnapshot)` | [NEW] |
| `QE/agent_routes/admission.rs` (D2) | `admit_action` gate list | replace gate 8 (`enforce_step_budget`) with `budget::enforce_budget` **and move it before the sensitive-approval gate** (A2); skip for `RunClass::Interactive`; on first refusal latch, emit `agent.budget_exhausted`, write checkpoint `budget_exhausted`; `QE/run_limits.rs` supplies `BudgetLimits`. Update test `admission_order_is_pinned` | [NEW] |
| `QE/agent_routes/step.rs` (D2) | `step`, `execute_ws_action` | after execution call `budget::charge` for **both** success and failure; failure receipts carry the flat cost (`ReceiptBuilder::fail(...).cost_usd(..)`); success path uses `receipt_cost_usd` in REST and WebSocket (fixes the WS zero-cost drift); REST returns `Envelope::ok_with_budget`; WebSocket action results add a `budget` key | [NEW] (moved from [C] 2228-2409, 3086-3193) |
| `QE/agent_routes/lifecycle.rs` (D2) | `start_run`, `StartRunBody` (hint field), `StartRunData` | class rule per A6; `resolve_caps` after constraints are resolved (1365-1368), store in `RunLive`; `StartRunData.constraints` and `clamped`; resume: `BudgetLedger::restore` from the checkpoint; `DELETE` response gains `usage` (`{ spent_usd, steps, attempts, active_s, class, exhausted }`) | [NEW] (moved from [C] 1273-1663, 3562-3655) |
| `QE/run_limits.rs` (D2) | `BudgetLimits`, `BudgetLimits::from_env()` | env table below; unparsable value aborts boot | [NEW] (from D2) |
| `QR/step_receipts.rs` | `AgentRunCheckpoint` (39-71), `StepReceipt` (183-210), `ReceiptBuilder::fail/skip` (534, 554) | checkpoint adds `spent_micro_usd`, `attempts`, `active_ms`, `segment`, `budget_exhausted: Option<String>` (all `#[serde(default)]`); receipt adds `cost_source: Option<CostSource>`; `fail()` accepts a cost | [C] |
| `QR/postgres_step_receipts.rs` | `append` (INSERT at 143), SELECT lists (181, 207, 224), `decode` (34-117) | bind and read `cost_source` | [C] |
| `Q/crates/quarry-runtime/migrations/0011_step_receipt_cost_source.sql` | table `quarry_step_receipts` | `ALTER TABLE quarry_step_receipts ADD COLUMN IF NOT EXISTS cost_source TEXT CHECK (cost_source IN ('flat_table','estimated','provider_billed'));` nullable, no backfill (next free after D2's 0010; renumber at merge) | [NEW] |
| `QR/action_cost.rs` | `estimate_action_cost`, new `action_cost_name(&AgentAction)` and `browser_time_micro(active_ms, rate)` | one name mapping used by the edge and by `AgentLoop` (dedupes the 34-arm matches at `agent_routes.rs:725-760` and `agent_loop.rs:299+`); flat table unchanged | [C] |
| `QR/fallback_driver.rs` | `error_code_short` (587) | `BudgetExhausted => "budget_exhausted"` (exhaustive match) | [C] |
| `QC/tests/contracts.rs` | error tests | `BudgetExhausted` is 402, not retryable, serde name `BUDGET_EXHAUSTED` | [C] |
| `Q/pkg/quarrycontracts/envelope.go` | `ErrorCode`, `HTTPStatus` | `CodeBudgetExhausted` 402 (D2's mirror sync adds the other missing codes) | [C] |
| `Q/docs/openapi.yaml`, `CONTRACTS.md`, `WEB_AGENT_ROADMAP_EXECUTION.md` | step 402, `StartRunData`, `meta.budget`; roadmap lines 49-53 corrected to what is enforced | [M], [C], [C] |
| `MP/rust/services/model-gateway/src/grpc.rs` (2700), `tools.rs` (1184) | `QuarryError::Typed` map | `"BUDGET_EXHAUSTED" => Status::failed_precondition` (owned by sibling b, not required for this slice) | [M], [C] |
| `MP/rust/services/execution-core/src/quarry_agent.rs`, `browser_agent.rs` | `AgentClientError`, `BrowserResourceLimit` (59-62), `check_limits` (320-331) | **sibling (b), Section 7**: parse the 402 body into a typed error, add `BrowserResourceLimit::Cost` and `Attempts`, treat it as a resource-limit terminal state and **not** `PlanStatus::Failed` (today any step error fails the plan, `browser_agent.rs:749-752`); enforce the combined cap from `meta.budget` plus cost-core run usage (DEC-9) | [C], [C] |

**Config (all `QUARRY_EDGE__`, parsed fail-loud in `run_limits.rs`; `0` disables a dimension where noted)**

| Variable | Default | Meaning |
|---|---|---|
| `BUDGET_ENFORCEMENT` | `shadow` first deploy, then `on` | `off`, `shadow` (compute, log `would_exhaust` and count, refuse nothing), `on` |
| `AGENT_DEFAULT_MAX_STEPS` / `_MAX_RUNTIME_S` / `_MAX_COST_USD` | 60 / 900 / 1.00 | applied when the request is absent or 0 |
| `AGENT_CEILING_MAX_STEPS` / `_MAX_RUNTIME_S` / `_MAX_COST_USD` | 300 / 3600 / 20.00 | request values above are clamped and reported |
| `BUDGET_ATTEMPT_FACTOR` | 2 | attempts allowance = `max_steps * factor` (`0` disables the attempts cap) |
| `BUDGET_BROWSER_SECOND_USD` | 0.00003 | estimator rate; **set from the billing-core price table before `on`** |

**API / data-model changes (exact names).**
- `POST /v1/agent/runs` body: optional `initiated_by: "agent"` (tighten-only class hint). Response `data` additive: `"constraints": { "class": "agent", "max_steps": 60, "max_runtime_s": 900, "max_cost_usd": 1.0, "estimated": true }` and `"clamped": [ { "field": "max_steps", "requested": null, "effective": 60, "by": "default" } ]`.
- `POST /v1/agent/runs/{id}/step` and `/interact`: success `meta.budget { class, steps, attempts, max_steps, max_attempts, elapsed_s, max_runtime_s, spent_usd, max_cost_usd, estimated, cost_source, exhausted }`; refusal HTTP 402 body `{ data: null, meta: { request_id }, error: { code: "BUDGET_EXHAUSTED", message, details: { dimension, limit, spent, unit, estimated, limit_source, resumable }, retryable: false } }`.
- `DELETE /v1/agent/runs/{id}` response gains `usage` (above). Event `agent.budget_exhausted` payload `{ dimension, limit, spent, unit, estimated }`, once per segment, via `emit_for_zdr` (live subscribers only under ZDR).
- Receipts additive `cost_source`; checkpoint JSONB additive fields. One SQL file, `0011`.

**Rollout and rollback.** Deploy 1 with `BUDGET_ENFORCEMENT=shadow` for at least 24 hours on the shared stack: review `would_exhaust` lines by class (a human session must never appear), compare `spent_usd` against receipts. Then `on`. Rollback: `BUDGET_ENFORCEMENT=off` (no image change). Image rollback is safe: all persisted additions are `#[serde(default)]` or nullable.

**Tests.**
- Unit `budget.rs`: `interactive_class_is_never_budgeted`, `manual_session_12_steps_120s_constraints_do_not_cap`, `agent_cap_counts_completed_steps`, `failed_attempts_are_bounded_by_the_attempt_allowance`, `runtime_clock_excludes_takeover_pause`, `cost_projection_refuses_before_overshoot`, `cost_includes_browser_time_term`, `exhaustion_latches_and_repeats_the_same_error`, `resolve_caps_takes_min_of_request_grant_org_ceiling`, `zero_or_none_request_takes_the_default_not_unlimited`, `clamped_values_are_reported_with_their_source`, `resume_restores_spent_attempts_and_active_time`, `zdr_run_accumulates_in_memory_and_writes_no_checkpoint`, `receipt_cost_is_identical_on_rest_and_websocket_paths`.
- Unit `admission.rs`: `budget_gate_precedes_sensitive_approval`, `budget_refusal_burns_no_one_shot_approval`, `admission_order_is_pinned` (updated).
- Unit `step_receipts.rs`: `legacy_checkpoint_without_budget_fields_deserialises`, `failure_receipt_carries_flat_cost`, `receipt_roundtrips_cost_source`; `quarry-core` contracts test for the 402 code; Go `TestErrorCodeStatusMirror` extended.
- Integration `QE/../tests/agent_run_budget.rs` [NEW] (D2's Claims-injecting harness plus a recording driver with scripted costs and durations): service-principal run with `max_steps 3` refuses the 4th action with 402 `dimension: steps`; a user-token run created with `constraints {12, 120}` performs 20 actions all 200; `max_cost_usd 0.0003` refuses at the projected overshoot; a failing driver script hits `attempts` before `steps`; WebSocket action refused identically; the event fires once; ZDR run enforces and persists nothing (in-memory store empty); resumed run keeps spent; `meta.budget` present on success.
- Postgres `#[ignore]` (`TEST_DATABASE_URL`, pattern `postgres_queue.rs:435`): migration `0011` idempotent and `cost_source` roundtrip.
- Commands: `cargo test -p quarry-core contracts error`; `cargo test -p quarry-runtime step_receipts action_cost`; `cargo test -p quarry-edge budget admission agent_run_budget`; Go `cd "apps/Ingestion Plane/Quarry-v2/pkg/quarrycontracts" && go test ./...`; CI `cargo test --workspace` (`quarry-v2-ci.yml`).

**Definition of done.** Agent-class runs are bounded in steps, attempts, active runtime and estimated cost with a typed 402 that names the dimension; manual sessions are demonstrably uncapped; effective limits and live numbers are returned; failure and WebSocket cost drift fixed; resume cannot refill; documentation matches the code.

**Deployment evidence required.** Rebuild edge image. Test-only env in the last env file: `BUDGET_ENFORCEMENT=on`, `AGENT_DEFAULT_MAX_STEPS=5`. Flows: (1) **Human not capped.** In the Verevon UI open a browser session, then through the gateway `run_action` route send 20 scripted actions with the user's token: all return 200 and the session stays live (before this slice the 13th returns 429; record that baseline). Edge log shows no `would_exhaust` for class `interactive`. (2) **Agent capped.** Start an AI browser run from chat without a step limit against a real public site; after the fifth completed action the edge answers 402 `BUDGET_EXHAUSTED` `dimension: steps`; `GET /v1/agent/runs/{id}/receipts` returns exactly 5 completed receipts, each with `cost_micro_usd > 0` and `cost_source: flat_table`; the run's SSE shows one `agent.budget_exhausted`; `psql` on `quarry_agent_run_checkpoints` shows `state->>'budget_exhausted' = 'steps'` and status still `active`. (3) **Clamp is visible.** Start via the edge with a service token and `max_cost_usd: 50`; the response `clamped` lists `max_cost_usd` effective 20.0 `by: ceiling`. (4) **Resume does not refill.** Let run (2) idle-expire (D2), `POST /v1/agent/runs` with `resume_run_id`: the first step is refused at once. (5) The honest limit: until sibling (b) maps the 402, execution-core reports the run as failed with a step error; the evidence file records that and links the Model Plane follow-up.

**Effort.** M, 5 days Quarry (the audit says S; D2's own re-estimate of the adjacent F-t2-013 phase 1 from S to M shows the same pattern: typed error, mirrors, checkpoint, tests) plus 1.5 days in Model Plane (sibling b). **Risks and mitigations.** (R1) A false cap on a human session: class-keyed, shadow stage, test `manual_session_12_steps_120s_constraints_do_not_cap`. (R2) The estimator stops runs too early or too late: labelled `estimated`, shadow stage compares to receipts, rate is env. (R3) execution-core fails the plan on 402 until sibling (b) lands: documented, and the agent default is generous (60 steps). (R4) Approval wait counts as runtime: documented limit, F-t2-033 decides. (R5) D2 not landed: the same code can be written in the monolith at higher merge cost; A2's gate-order request is a one-line difference.

---

### S2. Usage metering for browser time, agent runs, crawl pages, extract and map; ZDR-clean metadata (F-t2-046 phase A; effort M, 6 engineer-days)

**Goal.** Every billable web action Quarry performs becomes a content-free, idempotent usage event with outcome and surface attributes, behind a flag until Lago has the codes; fix the places that bill wrongly (failed handoffs, failed extract items, search-labelled extract and map) and the one that does not bill (crawl pages).

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QR/usage.rs` | `metrics` (88-94), `UsageEvent::new` (64-82), `UsageMeter` (99-101), `NatsUsageMeter::meter` (144-179) | add the seven constants of DEC-10; `UsageEvent::keyed(parts: &[&str], ...)` building `quarry:<a>:<b>:<metric>` and rejecting ids over 128 bytes or outside `^[A-Za-z0-9][A-Za-z0-9._:-]*$`; `UsageMeter::flush(&self, deadline: Duration)` (default no-op); `NatsUsageMeter` tracks in-flight publishes (counter plus `Notify`) so `flush` can wait | [C] |
| `QR/lib.rs` | re-exports | additive | [M] |
| `QE/usage_attrs.rs` | `Surface` (closed enum), `surface_for(&Claims, Option<&str>)`, `UsageAttrs`, `metadata(attrs, zdr) -> Value` (ZDR allowlist), `ShadowMeter` wrapper for `shadow` mode | new | [NEW] |
| `QE/lib.rs`, `QE/main.rs` | `mod usage_attrs;`, meter wrapping at 451-458 | wrap the chosen meter in `ShadowMeter` when `USAGE_BROWSER_METRICS=shadow`; `off` drops only the **new** metrics | [M], [M] |
| `QE/agent_routes/lifecycle.rs` (D2) | `finish_run` | after release: `usage::emit_run_segment(state, handle, reason)`: `browser.second.{agent|interactive}` (whole seconds, skip 0), `agent.run` and `agent.step` for the agent class; `outcome` from `CloseReason` and the budget latch; increments the checkpoint `segment` on the next resume | [NEW] |
| `QE/routes.rs` | scrape meter (488-509), `crawl_handoff` meter (642-659), `batch_handoff` meter (702-717), `internal_run_page` (898-1020) | scrape metadata via `usage_attrs::metadata` (D4 slice S-4 removes `url` and `fingerprint` first; take whichever lands first and rebase); crawl and batch events only on `Ok(ack)`; `internal_run_page` reads `kind` and, for `crawl` and non-ZDR success, emits `quarry.crawl.page` with id `quarry:<run_id>:<sha256(url)[:16]>:quarry.crawl.page` | [M] |
| `QE/extract_routes.rs` | meter block (344-360) | `quarry.extract.url`, quantity = items fetched or extracted, skip when 0, metadata `{op, structured, outcome counts}` | [M] |
| `QE/map_routes.rs` | meter block (293-310) | `quarry.map.call`; drop `host` from the metadata under ZDR | [C] |
| `QO/internal/activities/activities.go` | `RunPageInput`, `RunPage` body (213) | new `Kind string`; body carries `kind` | [C] |
| `QO/internal/workflows/workflows.go` | `runPage` (174+) and its callers in `CrawlJobWF`, `crawlJobDurableFrontier`, `BatchJobWF` | pass `"crawl"` or `"batch"` | [C] |
| `BC/internal/billing/usage_test.go` | tests | `TestUsageEventIDPatternAcceptsQuarryRunEvents` pins the id shapes and the `quantity > 0` rule from the consumer side | [C] |
| `Q/docs/BILLING.md`, `CONTRACTS.md` | metric table, failure semantics, ZDR allowlist | document the seven metrics and the corrected emission rules | [C] |

**API / data-model.** No HTTP contract change. Wire: new `usage.<metric>` subjects (covered by billing-core's `usage.>` subscription, `BC/internal/nats/subscriber.go`). Event ids `quarry:<run_id>:s<segment>:<metric>` and `quarry:<run_id>:<h16>:quarry.crawl.page`. Metadata keys are the closed set of DEC-10. No SQL. **External prerequisite:** a Lago billable-metric code per new metric (none exist in the repo; Question 4).

**Flags and rollout.** `QUARRY_EDGE__USAGE_BROWSER_METRICS` `off` (default), `shadow`, `on`. The *corrected* behaviour of existing metrics (no seed or batch event on failed handoff, extract and map no longer billed as search) ships with `on`; to avoid a surprise invoice change the corrections are gated by the same flag. Rollout: `shadow` on the shared stack for a week, compare shadow volumes to receipts and to `billing_usage_events`, enable when Lago codes exist. Rollback: `off`.

**Tests.**
- Unit `usage_attrs.rs`: `surface_is_derived_from_the_principal_for_user_tokens`, `header_is_ignored_unless_service_token_and_closed_enum`, `zdr_metadata_is_an_allowlist_without_url_host_or_fingerprint`, `event_id_fits_the_billing_pattern_and_length` (property test with a copy of billing-core's regex), `resume_segments_produce_distinct_ids`, `zero_quantity_is_not_emitted`.
- Unit `usage.rs`: `flush_waits_for_inflight_publishes_then_returns`, existing five tests unchanged.
- Integration `QE/../tests/usage_metering.rs` [NEW] with the `RecordingMeter` pattern of `zdr_claim_floor.rs:225-231, 379-388`: agent run close emits one `browser.second.agent` and one `agent.run`; interactive close emits `browser.second.interactive`; a ZDR run's events contain none of `url, host, fingerprint, title`; failed extract items are not metered; a failed crawl handoff emits no seed event; `run_page` with `kind=crawl` emits one page event and a retried call reuses the id; `kind=batch` emits none.
- Go: `QO/internal/activities/activities_test.go` [NEW] (`httptest` server asserts `kind` in the body); workflows tests pass the kind.
- Commands: `cargo test -p quarry-runtime usage`; `cargo test -p quarry-edge usage_attrs usage_metering`; `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./...`; `cd "apps/Control Plane/billing-core" && go test ./internal/billing/ ./internal/nats/`.

**Definition of done.** Browser seconds, agent runs, steps, crawl pages, extract URLs and map calls are emitted exactly once with stable ids and closed attributes; nothing content-bearing is emitted under ZDR; corrected rules are in force when the flag is `on`; docs and the billing-core pin test agree.

**Deployment evidence required.** Rebuild edge and orchestrator; `USAGE_BROWSER_METRICS=on` for a test org whose Lago codes are stubbed (billing-core persists regardless of Lago). Flows, asserting on `billing_usage_events` (billing-core Postgres, `psql`): (1) an AI browser run of about 20 s: one row `quarry.browser.second.agent`, `quantity` between 19 and 22, `metadata->>'class' = 'agent'`; one `quarry.agent.run`; one `quarry.agent.step` with the receipt count. (2) A manual browser session open for 10 s then closed: one `...interactive` row. (3) A 5-page crawl (with `QUARRY_CRAWL_DEFAULT_MAX_PAGES=5`): one `crawl.seed`, five `crawl.page`; re-run of the same page activity does not add rows. (4) A ZDR-org agent run: its rows have `metadata ? 'url'` false and `metadata->>'zdr' = 'true'`. (5) Failure case: stop the control container, submit a crawl: the edge returns the error and **no** seed row appears. (6) `GET /api/v1/billing/orgs/{org}/quotas/quarry.crawl.page` shows `used` 5.

**Effort.** M, 6 days (the audit says L for the item; this is its phase A). **Risks.** A new billing line without Lago codes (flag-gated); batch double billing (avoided by `kind`); crash loses an in-memory segment (S9b); user-visible billing correction for failed extract and handoff (flag-gated and announced).

---

### S3. Limits resolver, Control Plane quota read, clamp to remaining entitlement with the value returned (F-t2-008 slice 2a, F-t2-025 numbers, F-t2-046 plan keys; effort M, 5 engineer-days)

**Goal.** One `LimitsResolver` answers every limit question (budget ceilings, crawl and batch remaining, per-org capacity) from, in order, the grant claim (S4/D7, absent until then), the billing-core read, and the static floor, failing to the floor; crawl, batch and agent runs are clamped and the clamped values returned.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `BC/internal/billing/types.go` | `QuotaStatus` (56-64) | additive `Configured bool json:"configured"` | [C] |
| `BC/internal/billing/service.go` | `GetQuotaStatus` (845-919) | set `Configured` true when an allowance policy matches or `account.QuotaLimits` has the key | [C] |
| `BC/internal/billing/defaults.go` | `defaultQuotaLimitsByPlan` (61-92) | add `quarry.cap.agent_runs`, `quarry.cap.interactive_runs`, `quarry.cap.render_permits`, `quarry.cap.crawl_jobs`, `quarry.cap.job_backlog`, `quarry.cap.rate_multiplier` for every plan (placeholders: free 1/2/1/1/10/1, hobby 2/4/1/2/50/2, standard 4/6/2/3/200/4, pro 8/8/2/5/1000/10, enterprise 16/16/4/10/5000/50; trial equals free); numbers are Control Plane's to set | [C] |
| `CP/docker-compose.yml` | billing-core `BILLING_CORE_SERVICE_CREDENTIALS` (621) | add principal `quarry-edge`, audience `billing-core`, scopes `billing:quota:read:any`, `billing:entitlement:read:any`, token from a new secret generated through the existing secrets bootstrap (not hand-pasted) | [C] |
| `BC/internal/http/service_auth_test.go` | tests | `TestQuarryPrincipalCanReadQuotasButNotWriteUsage`; boot test that a registry with a third principal passes `ValidateRequiredServiceCredentialRegistry` | [C] |
| `QE/limits.rs` | `LimitsResolver` (trait), `OrgLimits`, `StaticFloor`, `BillingCoreLimits` (reqwest, 1500 ms timeout, per `(org, metric)` cache 30 s, stale-on-error 15 min), `FailMode` | new (S0 created the file) | [NEW] |
| `QE/state.rs`, `test_support.rs`, `tests/support/mod.rs` | `AppState.limits: Arc<dyn LimitsResolver>` | one field next to `usage` (101) and one literal line in each harness | [M], [M], [M] |
| `QE/main.rs` | resolver construction | `BillingCoreLimits` when `BILLING_CORE_URL` and token are set, else `StaticFloor`; boot log line `limits: source=billing_core fail_mode=floor` | [M] |
| `QE/routes.rs` | `crawl_handoff`, `batch_handoff` | clamp per DEC-11 including `remaining == 0 -> 402 BUDGET_EXHAUSTED(org_allowance)` before the job is created; `limits` in the ack extends S0's shape with `"remaining": 25, "clamped_by": ["entitlement"]`; batch returns `urls_accepted`, `urls_dropped`, `clamped_by` | [M] |
| `QE/agent_routes/lifecycle.rs`, `budget.rs` (D2, S1) | `ResolveInputs.entitlement` | the remaining `quarry.browser.second.agent` seconds become a runtime ceiling (`limit_source: entitlement`); `org_allowance` exhaustion refuses `start_run` with 402 | [NEW] |
| `QE/agent_routes/run_registry.rs` (D2) | `try_reserve(org, class, org_override)` | A3 request: pass `OrgLimits` per-org values (S6 uses them) | [NEW] |
| `IP/docker-compose.yml` | quarry-edge env | `QUARRY_EDGE__BILLING_CORE_URL`, `QUARRY_EDGE__BILLING_CORE_SERVICE_TOKEN`, `QUARRY_EDGE__LIMITS_FAIL_MODE=floor`, `QUARRY_EDGE__LIMITS_CACHE_TTL_S=30`, `QUARRY_EDGE__LIMITS_STALE_MAX_S=900` | [M] |
| `Q/docs/openapi.yaml`, `CONTRACTS.md`, `docs/BILLING.md` | `limits` shapes, 402 `org_allowance`, the resolver order | [M], [C], [C] |

**API / data-model.** Edge responses: crawl ack `limits.max_pages { requested, effective, remaining, clamped_by[] }`; batch ack `limits.urls { requested, accepted, dropped, remaining, clamped_by[] }`; `start_run` `clamped[].by = "entitlement"`; 402 `BUDGET_EXHAUSTED` with `details.dimension = "org_allowance"` and `metric`. billing-core: `quota.configured` (additive, existing consumers ignore it). No SQL (plan defaults are Go maps; allowance policies are per-org API data).

**Flags and rollout.** `QUARRY_EDGE__LIMITS_SOURCE = static | billing_core` (default `static`); `LIMITS_FAIL_MODE = floor | open | closed`. Rollout: ship `static`; add the credential in the Control Plane compose; flip an org-scoped canary (`LIMITS_BILLING_ORGS=<comma list>` for the first week) to `billing_core`; then default on. Rollback: `static`.

**Tests.**
- billing-core: `TestQuotaStatusReportsConfiguredForPlanKeyAndAllowancePolicy`, `TestQuotaStatusUnconfiguredIsLimitZeroAndNotConfigured`, `TestPlanDefaultsContainQuarryCapKeysForEveryPlan`, the service-auth tests above. Command `cd "apps/Control Plane/billing-core" && go test ./internal/billing/ ./internal/http/` (CI uses `go test -v -p 1 ./...` with Postgres).
- Quarry unit `limits.rs`: `last_good_is_served_within_the_stale_window`, `floor_after_the_stale_window`, `unconfigured_quota_means_floor_not_zero`, `entitlement_never_raises_budget_caps`, `entitlement_may_raise_capacity_up_to_the_global_cap`, `billing_error_never_blocks_start_run_in_floor_mode`, `closed_mode_refuses_when_unreadable`, `open_mode_uses_no_ceiling`, `cache_hit_does_not_call_billing_core`.
- Quarry integration `tests/limits_clamp.rs` [NEW] with an axum stub standing in for billing-core: crawl clamps to remaining 25 and reports it; remaining 0 returns 402 and creates no job (stub control records zero POSTs); batch of 40 with remaining 25 accepts 25, reports 15 dropped; stub down falls to the floor with a warning.
- Commands: `cargo test -p quarry-edge limits limits_clamp`.

**Definition of done.** A crawl or batch can never exceed the org's remaining configured allowance; every clamp is returned; a Control Plane outage degrades to the floor; Quarry holds exactly one new credential and no database access; billing-core distinguishes unconfigured from zero.

**Deployment evidence required.** Rebuild billing-core and edge; add the principal; boot-check that billing-core starts (`docker logs`). Flow: (1) configure an allowance policy of 25 `quarry.crawl.page` for a test org through billing-core's `PUT /api/v1/billing/orgs/{org}/allowances` `[UNVERIFIED request shape: read allowances.go before the run]`; (2) crawl with `max_pages` omitted: ack `effective: 25`, `clamped_by: ["entitlement"]` (not 1000); (3) after S2 events consume the 25 pages, `GET .../quotas/quarry.crawl.page` shows `remaining: 0` and the next crawl submit returns 402 with no job row (`psql` count unchanged); (4) `docker stop billing-core`, wait past the cache TTL: a crawl is accepted at the floor and the edge logs `limits: billing_core unreachable; serving floor` (and last-good within 15 minutes); (5) an org with no policy: ack shows `clamped_by: ["default"]`, never `entitlement`.

**Effort.** M, 5 days (Control Plane 1.5, Quarry 3.5). **Risks.** Period semantics (plan-default quotas are all-time, so periodic remaining depends on allowance policies, Question 4); cache staleness lets a burst slightly exceed the allowance (bounded by the 30 s TTL and noted); a new static credential in the Control Plane compose (secrets-bootstrap rules apply); `AppState` field rebase against the uncommitted `state.rs` hunks.

---

### S4. Budget claim in the BrowserBroker grant (F-t2-008 slice 2b; effort M, 5 engineer-days across Model Plane and Quarry, plus 2 in execution-core by sibling b)

**Goal.** The Model Plane mints a tighten-only budget claim into the run grant; Quarry reads it at `start_run` and enforces it as the `grant` term of DEC-5; execution-core treats exhaustion as a resource-limit outcome.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `MP/proto/model_plane/v1/browser.proto` and generated `MP/go/gen/model_plane/v1/browser*.pb.go` | `AcquireGrantRequest`, `AcquireGrantResponse`, `ValidateGrantResponse`, and D1's `DeriveRunGrantRequest`/`Response` | fields `40-44` per DEC-8 (`budget_micro_usd`, `budget_max_steps`, `budget_max_runtime_s`, `budget_source`, `budget_minted_at_unix`), additive, no `reserved`; regenerate Go with `buf generate` through the `bufbuild/buf` image (buf is not on the host) | [C], [C] |
| `Q/crates/quarry-runtime/proto/browser.proto` | vendored copy | same fields on `ValidateGrantResponse` | [C] |
| `MP/go/services/browser-broker/internal/grant/grant.go` | `Grant`, `Derive` | store and return the budget verbatim; derived run grants may only tighten (`ErrDeriveWidensBudget`) relative to the parent | [C] |
| `MP/go/services/browser-broker/internal/server/server.go`, `internal/server/errors.go` | RPC handlers, error map | map the new fields; validation: non-negative, `budget_source` in a closed set (`request`, `org_default`, `entitlement`) | [C] |
| `MP/go/services/browser-broker/migrations/0003_browser_grants_budget.up.sql` and `.down.sql` (D1 owns `0001`, D7 `0002_browser_grants_web_policy`; renumber at merge) | columns | `ALTER TABLE ... ADD COLUMN IF NOT EXISTS budget_micro_usd BIGINT, budget_max_steps INT, budget_max_runtime_s INT, budget_source TEXT` | [NEW] |
| `MP/rust/services/model-gateway/src/browser_run.rs` | `browser_run_start` (297-415), the `DeriveRunGrant` call D1 adds near 337-348 | `mint_budget(&state, &req, &org_id) -> BudgetClaim`: `min(req.max_cost_usd, org-core per-run ceiling)` using the existing `org_quota::fetch_org_limits` (fail open there is acceptable: Quarry's floor still applies); `max_steps` and `max_runtime_s` from the request; sent on the derive call; the same effective values go into `tool_input` (402-407) so execution-core and Quarry agree | [M] |
| `Q/crates/quarry-runtime/src/grant_validator.rs` | `GrantValidation` (28-50), HTTP and gRPC validators | `budget: Option<GrantBudget>` (`#[serde(default)]`) | [C] |
| `QE/agent_routes/lifecycle.rs`, `budget.rs` (D2, S1) | `ResolveInputs.grant` | snapshot the claim at `start_run`; `limit_source: grant`; never re-read on per-action revalidation | [NEW] |
| `MP/rust/services/execution-core/src/quarry_agent.rs`, `browser_agent.rs` | see S1 (sibling b) | parse the typed 402; `BrowserResourceLimit::Cost`; show `clamped` | [C], [C] |
| `.github/workflows/browser-broker-ci.yml` | D1's workflow | already runs `go test ./...` for the broker; add the proto paths if D1 has not | [NEW] (D1) |

**API / proto.** As above. Compatibility: all fields additive; an old broker returns zeros (no claim); an old Quarry ignores them.

**Flags and rollout.** `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM = off | on` (default off), `QUARRY_EDGE__GRANT_BUDGET_CLAIM = ignore | enforce` (default `ignore`, `enforce` only after the broker deploys). Rollback by either flag.

**Tests.**
- Broker Go: `TestDeriveRunGrantBudgetNeverWidensParent`, `TestValidateGrantReturnsBudgetVerbatim`, `TestBudgetFieldsRoundTripMemoryAndPostgres`. `cd "apps/Model Plane/go" && go test ./services/browser-broker/...`.
- model-gateway: `mint_budget_takes_the_min_of_request_and_org_ceiling`, `claim_is_absent_when_neither_is_set`, `org_core_failure_does_not_block_the_run`. `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib`.
- Quarry: `grant_claim_tightens_but_never_loosens_the_request`, `claim_is_not_rewidened_when_the_grant_is_revalidated`, `absent_claim_changes_nothing`.
- execution-core (sibling b): `budget_exhausted_maps_to_resource_limit_not_failed`.
- Proto guard: D1 fixes `buf.yml` and its 0-byte baseline; this slice's PR must pass that gate (S9 H5).

**Definition of done.** A chat browser run's effective limits show `by: grant` when the claim is tighter, the claim cannot be widened by Quarry's revalidation or by a derived grant, and exhaustion ends the run as a resource-limit outcome with partial results.

**Deployment evidence required.** Requires D1's production grants (without them Quarry rejects runs, `main.rs:182-200`). Rebuild broker, model-gateway, execution-core and edge. Flow: start a chat browser run with `maxCostUsd 0.0005` and no step limit; edge `start_run` response `clamped` lists `max_cost_usd` effective 0.0005 `by: grant`; the run stops at the projected cost with 402; the run's terminal state in the Model Plane is a resource-limit outcome, not `failed`, with the receipts kept. Org ceiling case: set the org-core quota `max_cost_per_run_usd_micros` to 400 through the gateway quotas route and repeat with a larger request: effective 0.0004.

**Effort.** M, 5 days (Model Plane Go and Rust 3, Quarry 2) plus 2 in execution-core. **Risks.** Hard dependency on D1; the broker proto convention (40-49) must be recorded in the Model Plane decision ledger; org-core fails open in `fetch_org_limits`, so the org ceiling can be missing for one run (the floor still applies).

---

### S5. Gateway class-aware, org-keyed rate limits (F-t2-025 part a; effort S, 1.5 engineer-days)

**Goal.** Separate buckets per endpoint class, keyed by org, so human browsing is never throttled by agent or job traffic and one org cannot exhaust another's allowance. Independent of every other slice.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `GW/rate_limit.rs` | `RateLimiter` (121), new `with_class(&self, class: &'static str, rpm: f64) -> Self`, `distributed_key` (260), `rpm_from_env` | the class limiter shares the Dragonfly connection and gets its own in-process buckets; the **Redis key gains the class** (`verevon:gw:ratelimit:{class}:{key}`) for non-default classes while the default class keeps today's key, so live buckets are untouched (Section 2.2 drift: today two limiters with one key string would share a bucket under different capacities); per-class rpm from `GATEWAY_RATE_LIMIT_RPM_<CLASS>` with a default | [C] |
| `GW/domains/browser.rs` | `router` (369-449) | split routes into two groups: `browser_start` (`POST /api/v1/browser/sessions` at 371 and the AI-run start route) and `interactive` (everything else). Each group: `.route_layer(from_fn(rate_limit_middleware))` first (innermost), then `.route_layer(Extension(state.rate_limiter.with_class(..)))`, then the existing `require_session` (outermost), the same ordering rule the mcp and agents_runs routers document | [C] |
| `GW/domains/knowledge.rs` | crawl and batch routes (179-183) | class `job_submit` | [C] |
| `GW/domains/search.rs` | search and scrape routes (46 per card) | class `search_scrape` | [C] |
| `GW/main.rs`, `GW/config.rs` | none | no new `AppState` field: classes derive from `state.rate_limiter` inside each `router()`, which avoids the five `AppState` literals that already carry `mail_task_rate_limiter` (`browser.rs:4974`, `chat/shared.rs:804`, `orchestration.rs:691`, `main.rs:374`, `crawl_preview/stream_e2e.rs:174`) | [C] |

**API.** HTTP 429 with `Retry-After` (existing body), now per class. Class table and defaults (per org, per minute; env overridable): `interactive` 1200, `browser_start` 30, `job_submit` 20, `search_scrape` 120, global default unchanged at 600. Plan multiplier (billing-core `quarry.cap.rate_multiplier`, read with the gateway's existing `billing:quota:read:self` delegation and cached 60 s) is design-level L7; this slice is env only. No Quarry change, no data change.

**Flags and rollout.** `GATEWAY_RATE_CLASSES=on|off` (default `on`; `off` removes the class layers). Rollback by env.

**Tests.** `class_limiters_have_independent_buckets_for_the_same_org`, `distributed_key_includes_the_class_and_the_default_key_is_unchanged` (extends the existing `distributed_key_is_namespaced_and_scannable`, `rate_limit.rs:478`), `interactive_class_is_not_exhausted_by_job_submit`, `class_key_is_the_org_not_the_ip_after_require_session`, route test in `browser.rs`: `session_create_is_limited_per_org_while_polling_continues`. Commands: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" rate_limit browser`; CI also runs `clippy --all-targets -D warnings` and `fmt` (`verevonv3-ci.yml:133-141`).

**Definition of done.** Each class has its own bucket and key; limits are per org once authenticated; interactive browsing is unaffected by exhausting `browser_start` or `job_submit`.

**Deployment evidence required.** Rebuild the gateway image (per the recipe, image swap and frontend restart). `GATEWAY_RATE_LIMIT_RPM_BROWSER_START=3` in the last env file. Flow as org A: create four browser sessions within a minute (gateway `POST /api/v1/browser/sessions`): the fourth returns 429 with `Retry-After`; in the same minute `run_action` and frame polling on the first session keep returning 200; as org B the fourth create succeeds. Dragonfly: `KEYS "*ratelimit:browser_start:*"` lists class keys for org A and B separately; `KEYS "*ratelimit:org:*"` still shows the unchanged default keys.

**Effort.** S, 1.5 days. **Risks.** Mis-set limits block onboarding (generous defaults, env knobs); the inner-limiter layering order is easy to invert (a test pins that the key source is `org`); per-process fallback buckets are per replica (the existing fail-open behaviour).

---

### S6. Per-org capacity in Quarry: plan-aware agent run caps and per-org render permits (F-t2-025 part b; effort M, 4.5 engineer-days)

**Goal.** One tenant can neither hold the whole browser lease pool nor all agent run slots; caps come from the resolver (S3) with the D2 env numbers as the floor.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QR/lease_pool.rs` | `RuntimeLeasePool` (34-158), `LeaseGuard` | additive `acquire_timeout_for_org(org_id, affinity_key, deadline)` and `try_acquire_for_org`: per-org counters (`DashMap<String, usize>` under the same permit), per-org cap, over-cap returns `RATE_LIMITED` with `details { reason: "org_render_cap", scope: "org", limit, active, retry_after_seconds }`; guard release decrements the org counter | [C] |
| `QR/browser_driver.rs` | adapter acquire (138-142, 160-163 per card) | thread the request's org id into the acquire (today the internal lease's `org_id` is empty). `PageRunner` already carries `org_id: Option<String>` (`QE/routes.rs:996`); the exact signature change is to be read at implementation `[UNVERIFIED]` | [C] |
| `QE/main.rs` | pool construction (323, 337) | per-org cap from `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` (default `max(1, pool / 2)`, `0` disables) | [M] |
| `QE/agent_routes/lifecycle.rs`, `run_registry.rs` (D2) | `start_run`, `try_reserve` | `let caps = state.limits.org(&org_id).await;` then `try_reserve(org, class, caps.run_override())`; when the binding limit came from the override the denial `details.reason = "org_plan_cap"`; never above the global cap | [NEW] |
| `GW/domains/browser.rs` | `forward_quarry_failure` (3873-3885) | extend D2's mapping of the cap reasons to the gateway code `browser_capacity_reached` with `org_plan_cap` and `org_render_cap` (same `retryAfterSeconds` copy) | [C] |
| `SPA/shared/i18n/errors.ts` | key `browser_capacity_reached` (D2) | no new key | [M] |

**API.** Existing `RATE_LIMITED` 429 envelope (`QE/api_error.rs:46-60`, `Retry-After` and `retry_after_seconds`) with the two new `details.reason` values. No SQL.

**Flags and rollout.** `QUARRY_EDGE__POOL_ORG_MAX_PERMITS`, plus S3's `LIMITS_SOURCE`. Rollback: `0` and `static`.

**Tests.** Unit `lease_pool.rs`: `org_cap_limits_one_org_without_blocking_another`, `org_permit_is_released_on_guard_drop`, `org_cap_zero_means_unlimited`, `wait_deadline_reports_the_org_reason` (beside the existing `try_acquire_respects_capacity` at 240 and `acquire_timeout_returns_error_when_pool_full` at 260). Integration `QE/../tests/org_capacity.rs` [NEW]: org A at cap 1 gets 429 `org_plan_cap` on the second concurrent run, org B unaffected; a stub billing-core raising A's cap to 3 admits three but a fourth is refused by the global cap. Gateway: `forward_quarry_failure_maps_org_caps_to_browser_capacity_reached`. Commands: `cargo test -p quarry-runtime lease_pool`; `cargo test -p quarry-edge org_capacity`; gateway `cargo test ... browser`.

**Definition of done.** Per-org permits and plan-aware run caps are enforced with typed, retryable 429s; the lease pool's internal org id is no longer empty; a second tenant keeps working while the first is at its cap.

**Deployment evidence required.** Rebuild edge and gateway. `QUARRY_EDGE__POOL_ORG_MAX_PERMITS=1` with the pool of 2. (1) Two concurrent JS-rendered scrapes from org A: the second returns 429 `org_render_cap`; a scrape from org B succeeds at the same moment. (2) With the billing-core plan key `quarry.cap.agent_runs = 1` for org A (S3 read), two chat browser runs: the second `start_run` returns 429 `org_plan_cap` with `Retry-After`; org B starts a run. (3) After S8: `quarry_lease_pool_in_flight` and `quarry_agent_run_denied_total{reason="org_plan_cap"}` move as expected.

**Effort.** M, 4.5 days. **Risks.** Threading `org_id` through the driver adapter touches the shared fetch path (F-t2-005/023 hotspot, mitigated by a one-parameter additive change); the pool is host-affinity keyed so per-org fairness is coarse (a per-org minimum plus burst is design-level); counters are per process (DEC-12).

---

### S7. Job admission, bounded backlog and fair share at quarry-control (F-t2-025 part c; effort M, 5 engineer-days)

**Goal.** Cap concurrent durable jobs per org with a bounded, expiring backlog and round-robin fair share, replacing the newest-50 cross-org poll that cannot hold a backlog without starving it.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QCT/internal/store/pg/migrations/016_jobs_admission.sql` | table `jobs` | `ALTER TABLE jobs ADD COLUMN IF NOT EXISTS claimed_until TIMESTAMPTZ; CREATE INDEX IF NOT EXISTS idx_jobs_status_org_created ON jobs (status, org_id, created_at);` (next free after D4's `015_zdr_run_tombstones.sql`; renumber at merge) | [NEW] |
| `QCT/internal/store/store.go` | `Jobs()` interface | add `ClaimAccepted(limit int, now time.Time, lease time.Duration) ([]Job, error)`, `CountAccepted(org string) int`, `FailExpired(before time.Time) (int, error)`; in-memory implementation for tests | [C] |
| `QCT/internal/store/pg/resources.go` | `jobsStore` (241+) | implement with one statement: running-or-leased count per org, `row_number() OVER (PARTITION BY org_id ORDER BY created_at, id)`, admit rows with `rn <= GREATEST(cap - in_use, 0)` where `cap` is the job's own stamped `params.admission.cap` (below), order `rn, created_at`, set `claimed_until = now() + lease` in the same transaction (`UPDATE ... RETURNING`), so a claim counts against the cap immediately and a crashed dispatcher's lease expires | [C] |
| `QCT/internal/resources/resources.go` | `createJob` (396-458), new `claimJobs` handler, route table | backlog check before `Create`: `CountAccepted(org) >= params.admission.backlog_cap` returns 429 `RATE_LIMITED` with `details { reason: "org_backlog_full", limit, queued, retry_after_seconds }`; `POST /v1/internal/jobs/claim` (HMAC-authenticated like `updateJob`) returns the fair batch | [C] |
| `QCT/cmd/control/main.go` | janitor | goroutine every 60 s: `FailExpired(now - QUARRY_JOB_BACKLOG_TTL)` marks stale `accepted` jobs `failed` with `params.failure_reason = "backlog_expired"` and writes the terminal event so event streams close | [C] |
| `QE/handoff.rs` | `forward_to_orchestrator` (28-100), non-2xx mapping (78-86) | stamp `params.admission = { cap, backlog_cap }` from `state.limits` (S3 resolver; floors `QUARRY_JOB_CAP_DEFAULT` 3 and `QUARRY_JOB_BACKLOG_DEFAULT` 200); map a control 429 with `org_backlog_full` to `ErrorCode::RateLimited` carrying the details instead of today's generic `UpstreamBlocked` 502 | [C] |
| `QO/internal/jobs/dispatcher.go` | `listJobs` (342-380), `tick` (148-203) | call `POST /v1/internal/jobs/claim {limit: CLAIM_BATCH}` instead of the newest-50 list; fall back to the old list when the endpoint answers 404 (rolling deploy); everything after is unchanged (`dispatch`, `markRunning`, `markFailed`) | [C] |
| `QCT/internal/store/pg/resources_v2.go` | `Concurrency` (102-135) | count `running` plus leased `accepted` so the team view matches admission | [C] |
| `Q/docs/REST_RESOURCES.md`, `QUEUE.md`, `openapi.yaml` | job states and the 429 | document backlog, fair share, expiry | [C], [C], [M] |

**API / data model.** Internal `POST /v1/internal/jobs/claim` request `{ "limit": 50 }`, response `[ job ]`. Public: `POST /v1/crawl` and `/v1/batch` may return 429 `RATE_LIMITED` `details.reason = "org_backlog_full"`; job reads keep `status: accepted` for a backlogged job (no new status value, so the SPA needs no change). New env: `QUARRY_JOB_CAP_DEFAULT` (3), `QUARRY_JOB_BACKLOG_DEFAULT` (200), `QUARRY_JOB_BACKLOG_TTL` (24h), `QUARRY_JOB_CLAIM_LEASE` (60s), `QUARRY_JOB_CLAIM_BATCH` (50). SQL: migration `016` only.

**Flags and rollout.** `QUARRY_JOB_ADMISSION=off|on` (default `off`): off keeps the old poll and no cap. Rollout: deploy control first (claim endpoint inert), then orchestrator and edge together, enable `on`. Rollback: `off` (the old poll resumes; jobs held `accepted` are picked up by it).

**Tests.** Control Go: `TestClaimIsRoundRobinAcrossOrgs`, `TestClaimRespectsPerOrgCapIncludingLeasedJobs`, `TestOldestAcceptedIsNotStarvedByNewerJobsOfAnotherOrg`, `TestClaimLeaseExpiresAndTheJobIsReclaimable`, `TestBacklogFullReturns429WithDetails`, `TestBacklogExpiryFailsOldJobsAndClosesTheirEventStream`, pg variants behind `TEST_DATABASE_URL` (pattern `store/pg/pg_test.go`). Dispatcher: `TestDispatcherUsesClaimAndFallsBackTo404` (first test file in `internal/jobs`). Edge: `handoff_maps_control_429_to_rate_limited_with_details`, `zdr_crawl_is_still_refused_so_no_backlog_row_exists` (pins `routes.rs:555-588`). Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`; `cd ".../quarry-orchestrator" && go test ./...`; `cargo test -p quarry-edge handoff`.

**Definition of done.** An org never has more than its cap of running or leased jobs; its excess waits in `accepted`, in submission order, up to a bound and a TTL, and is rejected with a typed 429 beyond that; a second org is not delayed by the first's backlog; the dispatcher no longer reads a cross-org newest-50 window.

**Deployment evidence required.** Rebuild control, orchestrator and edge. `QUARRY_JOB_ADMISSION=on`, `QUARRY_JOB_CAP_DEFAULT=1`, `QUARRY_JOB_BACKLOG_DEFAULT=2`, `QUARRY_JOB_BACKLOG_TTL=60s`. Flow: (1) org A submits three small crawls back to back; `GET /v1/jobs` shows one `running`, two `accepted`; org B submits one: it is `running` within one dispatcher tick, not behind A. (2) When A's first completes, A's second starts (status running, run id stamped). (3) A fourth submission from A with two already queued returns 429 `org_backlog_full` and no job row. (4) Leave a queued job untouched for 60 s: it becomes `failed` with `backlog_expired` and its event stream closes. (5) Starvation regression: queue 60 jobs for A, then one for B: B starts first-tick. (6) `psql`: `claimed_until` is set on claimed rows.

**Effort.** M, 5 days. **Risks.** Rolling deploy (fallback to the old list); the lease column and reclaim logic are the subtle part (covered by tests); caps are stamped by the edge at submit time, so a plan change takes effect for new jobs only (documented; control trusts the stamp because its API is HMAC-restricted to the edge and orchestrator `[INF]`).

---

### S8. Quarry Prometheus recorder, internal `/metrics`, stale docs corrected (F-t2-029 phase 1; effort S, 2 engineer-days)

**Goal.** Quarry has about twenty low-cardinality series on a listener that is not the public one, scraped by the existing Control Plane Prometheus.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QE/metrics.rs` | `install() -> PrometheusHandle` (`PrometheusBuilder::new().install_recorder()`), `describe_all`, `route_class(MatchedPath)`, `http_layer`, a 5 s sampler task | new; closed-set label helpers; the sampler turns `RunRegistry::stats()` (D2 A3) and `RuntimeLeasePool::metrics_snapshot()` (lease_pool.rs:53, finally a caller) into gauges | [NEW] |
| `QE/metrics_server.rs` | `serve(addr, token, handle)` | separate axum listener on `QUARRY_EDGE__METRICS_ADDR` (default `0.0.0.0:9092`, not published to the host), bearer token `QUARRY_EDGE__METRICS_TOKEN` compared in constant time; routes `/metrics`, `/healthz`; production refuses to start without a token | [NEW] |
| `QE/main.rs` | startup, router assembly, pool and listener blocks | install the recorder before the first request, spawn the metrics server; the HTTP layer is added to the app router next to the existing `TraceLayer` (D4's S-5 edits those same lines `routes.rs:286-303`: rebase, keep the layers separate) | [M] |
| `QE/telemetry.rs` | header comment (6-14) | rewrite: the recorder is installed in `metrics.rs`, label policy stated | [C] |
| `QE/agent_routes/*`, `usage_attrs.rs`, `routes.rs` | counter increments | `metrics::counter!` at the points below (bounded labels only) | [NEW]/[M] |
| `Q/docs/PLAN.md` (359), `GAP.md` (257), `gap-quarry.md` (metrics row `[UNVERIFIED: 175 is the OTEL row]`) | stale claims | state "recorder and internal `/metrics` (S8); alerts and spans not yet" | [C] |
| `CP/observability/prometheus.yml` | scrape config (after 47) | job `quarry-edge`, target `quarry-edge:9092`, `authorization: { credentials_file: /etc/prometheus/secrets/quarry_metrics_token }`, labels `{ service: quarry-edge, plane: ingestion }` | [C] |
| `CP/docker-compose.yml` | Prometheus service (1067-1081) | mount the token file read-only | [C] |
| `IP/docker-compose.yml` | quarry-edge service | `QUARRY_EDGE__METRICS_ADDR`, `QUARRY_EDGE__METRICS_TOKEN`, `expose: ["9092"]` (never `ports`) | [M] |

**Series (about 20; every label from a closed set of at most 12 values, never URL, host, org, user, run id or message).**

| Series | Type | Labels |
|---|---|---|
| `quarry_http_requests_total`, `quarry_http_request_duration_seconds` | counter, histogram | `route` (matched template), `status_class` |
| `quarry_agent_runs_active` | gauge | `class` |
| `quarry_agent_run_starts_total` | counter | `class`, `outcome` |
| `quarry_agent_run_denied_total` | counter | `reason` (the closed D2 and S6 list) |
| `quarry_agent_run_expired_total` | counter | `reason` |
| `quarry_agent_steps_total`, `quarry_agent_step_duration_seconds` | counter, histogram | `action_class` (navigate, input, read, other), `outcome` |
| `quarry_agent_budget_exhausted_total` | counter | `dimension` |
| `quarry_agent_run_duration_seconds` | histogram | `class` |
| `quarry_lease_pool_in_flight`, `_capacity`, `_acquired_total`, `_refused_total`, `_wait_seconds` | gauge, gauge, counter, counter, histogram | `pool`, `reason` |
| `quarry_scrape_outcomes_total` | counter | `driver`, `outcome` |
| `quarry_cache_requests_total` | counter | `cache`, `outcome` |
| `quarry_usage_events_total` | counter | `metric`, `result` (published, failed, shadow, dropped) |
| `quarry_edge_draining`, `quarry_build_info` | gauge | `version` |

Webhook delivery and job queue gauges belong to quarry-control (Go, no `/metrics` today); they are the Go follow-up inside L3.

**Flags and rollout.** `QUARRY_EDGE__METRICS_ENABLED=1` (default on). Rollback: `0` (the listener is not started). No data change.

**Tests.** `metrics_endpoint_requires_the_bearer_token`, `public_listener_does_not_serve_metrics` (the 8082 router answers 404 for `/metrics`), `route_label_is_the_matched_template_not_the_raw_path`, `metrics_labels_are_closed_sets` (exercise scrape, agent run, search with a probe URL and org id and assert neither appears in the rendered body), `production_refuses_to_start_without_a_metrics_token`, `sampler_exports_lease_pool_gauges`. Commands: `cargo test -p quarry-edge metrics`; CI `cargo test --workspace`.

**Definition of done.** `up{job="quarry-edge"} == 1` in the Control Plane Prometheus; series above present; no URL, host, org or user value anywhere in the exposition; the public port serves no metrics; docs corrected.

**Deployment evidence required.** Rebuild the edge image, restart the Control Plane Prometheus with the new config. Flow: from a container on `inter-plane-bus` `curl -H "Authorization: Bearer $T" http://quarry-edge:9092/metrics` returns the series; without the header 401; `curl http://127.0.0.1:8082/metrics` 404; the Prometheus targets page shows the job up; perform a scrape and an agent run and watch `quarry_agent_runs_active`, `quarry_http_requests_total` and `quarry_lease_pool_in_flight` move; `curl ... | grep -c "<probe-host>"` is 0.

**Effort.** S, 2 days. **Risks.** Label cardinality (pinned by test); exposing the listener (separate port, token, not published); the recorder's macros are global, so tests that install it use a separate test binary.

---

### S9. Graceful drain of runs, streams and usage (F-t2-029 phase 1b; effort M, 3 engineer-days; S9b orphan settle +2 days design-level)

**Goal.** On SIGTERM the edge stops admitting, ends its streams so the HTTP graceful phase can finish, releases every browser session (resumable for non-ZDR), and flushes usage publishes before it exits, inside a stated grace period.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QE/main.rs` | `shutdown_signal` (84-117), serve (1513-1535) | the shutdown future first calls `registry.begin_drain()` and sets `quarry_edge_draining`; after `serve` returns: `registry.drain(DRAIN_DEADLINE)`, then `state.usage.flush(5s)`, then the existing index flush | [M] |
| `QE/agent_routes/run_registry.rs`, `lifecycle.rs` (D2) | `RunRegistry::begin_drain`, `drain(deadline)`, `CloseReason::Drain` | new admission state; `drain` runs `finish_run(Drain)` over a snapshot with bounded parallelism (8) and a per-run 5 s release timeout; non-ZDR checkpoints stay `Active`, event `agent.expired` with `reason: "shutdown"`, usage segment emitted (S2) | [NEW] |
| `QE/agent_routes/live.rs` (D2) | `agent_events_stream`, `live_frame_stream`, `browser_ws_loop` | take a `watch::Receiver<bool>` shutdown token and end when it flips (otherwise `with_graceful_shutdown` never completes while a viewer is connected, Section 2.4) | [NEW] |
| `QE/routes.rs` | `start_run` guard, `crawl_handoff`, `batch_handoff`, `/ready` | while draining: 503 `RUNTIME_NOT_READY` (existing code, retryable) with `Retry-After`; `/ready` reports `draining` (D3 extends the same JSON with `browser`; coordinate) | [M] |
| `IP/docker-compose.yml` | quarry-edge service | `stop_grace_period: 45s` (default 10 s today) | [M] |
| `QO/cmd/orchestrator/main.go` | worker shutdown (119-131 and the `w.Run` call) | verify the worker stops with a bounded `WorkerStopTimeout` after the root context cancels `[UNVERIFIED: not read]`; set `stop_grace_period: 60s` in compose | [C] |
| `QR/usage.rs` | `NatsUsageMeter::flush` | from S2 | [C] |

**Config.** `QUARRY_EDGE__DRAIN_DEADLINE_S` (30; must be below the compose grace period).

**S9b (design-level, +2 days).** Boot-time settlement of orphans for non-ZDR runs: list `Active` checkpoints with no live handle and older than the idle TTL, emit the missing segment event once (idempotent id), mark the checkpoint `settled_segment`. Needs a store method `list_active_checkpoints_older_than` in `StepReceiptStore` and a checkpoint marker. ZDR crashes remain unbilled by design.

**Tests.** Unit and integration (`QE/../tests/drain.rs` [NEW], driving `registry.drain` and the shutdown token directly, no signals): `drain_refuses_new_runs_with_503_and_retry_after`, `drain_closes_streams_so_graceful_shutdown_completes`, `drain_releases_every_session_exactly_once`, `drain_keeps_non_zdr_checkpoints_active_and_writes_none_for_zdr`, `drain_emits_the_usage_segment_before_exit`, `usage_flush_waits_for_inflight_publishes`, `release_timeout_does_not_block_the_drain`. Commands: `cargo test -p quarry-edge drain`; `cargo test -p quarry-runtime usage`.

**Definition of done.** `docker stop` ends the edge with exit code 0 inside the grace period while a stream and a live session exist; no session leaks; usage for the segment is delivered; a non-ZDR run is resumable after restart.

**Deployment evidence required.** Rebuild edge. Baseline first: with an SSE `events/stream` and one manual session open, `time docker stop quarry-edge` takes about 10 s and `docker inspect` shows exit code 137 (SIGKILL). After the change: the same sequence exits 0 in under the drain deadline; `docker logs` shows `drain: released N sessions`; the open SSE received `agent.expired` with `reason: shutdown` before it closed; after restart a `POST /v1/agent/runs` with `resume_run_id` of a non-ZDR run succeeds; billing-core's `billing_usage_events` has the segment's `quarry.browser.second.*` row (S2 on); a ZDR run left no checkpoint.

**Effort.** M, 3 days (the audit's "S" assumed the HTTP shutdown already worked; it does not with live streams). **Risks.** A hung Chromium release (per-run timeout); a drain longer than the platform's kill window (grace period and deadline are separate settings); coordination with D3 on `/ready`.

---

### S10. Per-run statistics for crawl jobs (F-t2-029 phase 1c; effort M, 4 engineer-days)

**Goal.** Every durable crawl or batch ends with a content-light statistics object stored on its job record.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QO/internal/workflows/workflows.go` | `CrawlJobWF`, `crawlJobDurableFrontier`, `BatchJobWF`: terminal events (508-511, 607-609) | accumulate in workflow state (deterministic): `pages_visited`, `pages_failed`, `status_classes {2xx,3xx,4xx,5xx}` from `RunPageResult.Status` (activities.go:190-208), `drivers {name: n}` from `Driver`, `error_classes {category: n}` from `errs.Category`, `duration_ms` from `workflow.Now`; attach as `payload.statistics`; closed vocabularies, no URL, no title, no error text | [C] |
| `QCT/internal/store/pg/migrations/017_job_statistics.sql` | table `jobs` | `ALTER TABLE jobs ADD COLUMN IF NOT EXISTS statistics JSONB;` | [NEW] |
| `QCT/internal/store/store.go`, `pg/resources.go` | `Job.Statistics map[string]any`, the seven `SELECT` column lists (94, 125, 161, 197, 245, 303, 361) | refactor the lists to one `jobColumns` constant in the same PR, then scan the new column | [C], [C] |
| `QCT/internal/resources/resources.go` | `markJobTerminalIfNeeded` (300-318) | copy `ev.Payload["statistics"]` onto the job when the terminal event carries it | [C] |
| `QE/agent_routes/lifecycle.rs` (D2), `budget.rs` | `DELETE` response `usage` | already added in S1; add `quarry_agent_run_duration_seconds` | [NEW] |

**API.** `GET /v1/jobs/{id}` additive `statistics`. Quality counts keep counts only; no signatures and nothing page-derived (crawl is refused under ZDR, so no ZDR job has statistics at all). SQL: `017`.

**Tests.** `TestCrawlJobWF_TerminalEventCarriesStatistics`, `TestStatisticsContainNoUrlOrErrorText` (asserts keys and value types), control `TestTerminalEventPersistsStatistics`, pg roundtrip. Commands: `go test ./...` in the orchestrator and control modules.

**Definition of done.** A completed 5-page crawl's job record has `statistics.pages_visited == 5` and non-empty `status_classes`; failures show their classes; no URL appears in the object.

**Deployment evidence required.** Rebuild control and orchestrator (migration `017` visible in `\d jobs`). Crawl a 5-page site with one broken link: `GET /v1/jobs/{id}` shows the counts and `error_classes` for the broken page; `SELECT statistics FROM jobs WHERE id = ...` matches.

**Effort.** M, 4 days. **Risks.** Seven SELECT sites (hence the constant); Temporal replay (state is deterministic, no new calls).

---

### Design-level and unscheduled (not estimated for a sprint, each needs the stated precondition)

| # | Work | Precondition | Rough effort |
|---|---|---|---|
| L1 | Memory-aware admission: sample cgroup `memory.current / memory.max` and Chromium RSS, refuse leases above 0.8 with `RATE_LIMITED` `details.reason = "host_memory"` (no new `RESOURCE_BUSY` code: D3 already extends `RateLimited.details` for the driver slots), 25 consecutive refusals turn `/ready` to 503. Execution-core already treats `RATE_LIMITED` as backpressure after D2 | D3's multi-Chromium pool and viewport fix (F-t2-007); ADR topic 10 | M to L, 6 to 8 days |
| L2 | Span vocabulary: `browser.<action>` and `scrape.pipeline` spans with `driver`, `outcome`, `snapshot_bytes`, `url.origin` **only for non-ZDR runs**; W3C `traceparent` extraction in the Rust edge (the Go side already sets the propagator, `pkg/quarryotel`), propagation gateway to execution-core to Quarry, approval wait excluded from agent latency | F-t2-004 phase 2 `ZdrSampler` and span contract (designed in D10, which builds on D4's `zdr` span field and `SENSITIVE_FIELDS`), F-t2-014 correlation ids (D10) | M, 4 days |
| L3 | Alert rules and Alertmanager: rule file under `CP/observability/rules/` (`QuarryEdgeDown`, `QuarryLeasePoolSaturated` above 0.9 for 5 minutes, `QuarryBudgetExhaustedSpike`, `QuarryDrainSlow`, `QuarryUsagePublishFailures`, `QuarryJobBacklogGrowing`), a Go `/metrics` for quarry-control and the orchestrator, user-facing run-failure alerts through the existing notify sink (`QCT/internal/notify/sink.go`) to notification-core | new ADR: where Prometheus rules and an Alertmanager live (none exists) | M, 4 days |
| L4 | billing-core price-estimate endpoint feeding the Quarry `Estimator` and Model Plane approval cards; per-run usage in the run view (reads `meta.budget` and `usage`) | F-t2-014 run trace, F-t2-033 approval UX, Lago price table | M, 4 days |
| L5 | cost-core `execution` cost basis and one reconciliation entry per run segment (`idempotency_key = quarry-run:<run_id>:<segment>`), excluded from the billing outbox by construction | org-level cumulative budgets that must include execution spend (a product ask, not today's) | S to M, 3 days |
| L6 | Edge backstop token bucket per (org, class) returning `RATE_LIMITED` with `retry_after_seconds`; per-org permit minimum plus burst | a second edge replica, or evidence the gateway layer is bypassed | M, 3 days |
| L7 | Gateway plan multiplier: `quarry.cap.rate_multiplier` read through the gateway's existing billing-core delegation, cached 60 s, applied to the S5 class limits | S3 plan keys live | S, 1 day |

## 5. Dependencies and ordering

### 5.1 Ordering inside this workstream

| Slice | Needs | Kind | Notes |
|---|---|---|---|
| S0 crawl cap fix | nothing | none | ship first, alone |
| S1 budget enforcement | D2 S3 (`RunLive`, `admit_action`, `RunClass`) | soft (the same code fits the monolith at higher merge cost) | D2's gate-order request A2 (budget before sensitive approval) is a design agreement, not code |
| S2 metering | D2 S4 (`finish_run`) | hard for the run events; the crawl, extract and map parts are independent | Lago codes are external and hard only for the flag `on`; D4 S-4 (scrape metadata) soft, whichever lands first |
| S3 limits resolver | S2 (events to subtract) | soft for code, hard for the evidence flow | Control Plane credential and a billing-core release are hard for `billing_core` mode; `static` mode needs neither |
| S4 grant claim | D1 slice 1b (`DeriveRunGrant`, field reservation 40-49, Postgres store) and S1 (the reader) | hard | S3 soft |
| S5 gateway classes | nothing | none | parallel with everything |
| S6 per-org capacity | D2 S4 (registry) for the agent part; S3 for plan values | hard for the agent part, soft otherwise | render-permit part is independent |
| S7 job admission | S3 for plan-aware caps | soft (env floors work alone) | control, orchestrator and edge deploy together |
| S8 metrics | D2 S4 `RegistryStats` for run gauges | soft | conflicts with D4 S-5 on the router layers |
| S9 drain | D2 S4 (`finish_run`, registry), S2 (`flush`) | hard | coordinate `/ready` with D3 |
| S10 statistics | nothing | none | shares the `jobs` table with S7: migrations 016 and 017 land in order |

Streams (two engineers): **A** = S0, S1, S2, S3, S4 (budgets and metering, Quarry then Model Plane); **B** = S5, S8, S7, S6, S9, S10 (capacity and operations). They meet in `QE/limits.rs` (S3 introduces `OrgLimits` that S6 and S7 consume), `RunRegistry::try_reserve` (S3 and S6, one signature agreed up front), and the `jobs` migrations.

### 5.2 Cross-workstream dependencies

| Other item | Relation | Detail |
|---|---|---|
| **D2** (F-t2-006, F-t2-013 phase 1, split) | consumes | Section 3 lists nine assumed interfaces A1 to A9; two are *requests* (A2 gate order, A6 class rule, plus A3's override parameter and A4's `Drain`). If D2 slips, S1 and S6 wait or are written in the monolith |
| **D1** (F-t2-001 grants) | S4 hard | field numbers 40-49 are reserved for F-t2-008 (D1 D11); `DeriveRunGrant` and the Postgres store are D1's; production grants must exist for the S4 evidence |
| **D3** (F-t2-007 browser pool, F-t2-026 deadlines) | informs S6, S9, L1 | `RateLimited.details.reason` extends D3's driver reasons (`browser_session_cap`, `browser_page_cap`); `/ready` JSON is shared (S9); measured RSS per context (D2 S4, D3) sets real cap defaults; L1 needs D3's multi-Chromium pool |
| **D4** (F-t2-003/004 ZDR boundary) | S2, S8 | S-4 removes `url` and `fingerprint` from the scrape meter metadata (S2 reuses its allowlist); S-5 adds `ZdrMarkLayer` and edits the same router lines S8 edits; migration `015_zdr_run_tombstones.sql` takes control's next number (this design uses 016, 017); D4 points spans to a "D10": reconcile with L2 |
| **D5** (F-t2-005/009 scrape contract) and F-t2-016/017/018/040 (crawl scope, job lifecycle, schedules, batch) | conflict | `CrawlRequest`, `crawl_handoff`, `crawlInputFromJob`, the two crawl loops and `runPage` are their files too (Section 5.3). F-t2-018 (schedules that execute) must not switch on before S0/S3 so recurring crawls are bounded |
| **D7** (F-t2-022, `ResolvedPolicy`, ADR 1) | orthogonal, shares two files | D7's `web_policy` claim and `ResolvedPolicy` carry capabilities, ceilings (`robots.max_mode`, `proxy.max_tier`) and domain rules, **no numeric budgets, page limits or capacity** in v1, and org-core folds entitlements into its claim before minting. This design's numbers (steps, runtime, cost, pages, caps) therefore arrive by the billing-core pull (S3) and the grant claim (S4); if D7 later adds a numeric `limits` layer it plugs in as the first source of `LimitsResolver` (precedence: D7 claim, then grant claim, then billing pull, then floor). Shared: `browser.proto` (D7 field 20, D1 10-19, this 40-44), broker migrations (D7 `0002`, this `0003`), the `step` gate order (Section 3, A2, Question 6), `grant_validator.rs` and `QE/auth.rs` readers; nothing here waits for D7 |
| **D10** (F-t2-014 envelope, F-t2-004 phase 2) | complements | D10 owns the `ZdrSampler`, the late-ZDR registry and the span safety contract; it explicitly leaves the metrics recorder, per-run statistics, drain, admission and alerts to this design. L2 here is the span vocabulary on top of D10's contract |
| F-t2-010 (planner on by default) | **blocked by S1 and S4** | audit dependency note 4: the planner multiplies spend, so budgets must exist first |
| F-t2-042 (Quarry jobs as gated agent tools), F-t2-062 (research jobs), F-t2-065 | consume | the same `clamped`, `limits` and 402 contracts |
| F-t2-014 (run trace), F-t2-033 (approval UX), F-t2-038 | consume | `meta.budget`, `usage`, `agent.budget_exhausted` are the budget data for the run view and presence |
| F-t2-021 (redaction), F-t2-026 (deadlines), F-t2-047 (sub-codes) | share files | `error.rs` (one new code each), receipts; S1 passes the D2 mirror checklist |
| F-t2-011 (free human browsing) | consumes DEC-1 | owns deletion of the gateway literals `max_steps 12`, `max_runtime_s 120` (`GW/domains/browser.rs:483-488`) |

### 5.3 Merge-conflict hotspots (files and ranges shared with other workstreams)

| File | Ranges (live tree) | Shared with |
|---|---|---|
| `QE/agent_routes.rs` `[C]` (before D2's split) | `RunEntry` 79-102; `default_constraints` 433-440; `enforce_step_budget` 446-451; cost helpers 701-762; `start_run` 1273-1663 (constraints 1365-1372, ttl 1446); `step` 2228-2409 (budget 2294, receipts 2344-2398); `execute_ws_action` 3086-3193 (budget 3118, receipts 3154-3178); `close_run` 3562-3655 | D2 (all), F-t2-001, 011, 014, 021, 032, 036 |
| `QE/routes.rs` `[M]` | scrape meter 488-509; `CrawlRequest` 533-553; `crawl_handoff` 590-667; `batch_handoff` 681-725; `HandoffAck` 727-738; `internal_run_page` 898-1020; router layers 286-303 | D4 (S-4, S-5), D5 (E1: ScrapeOptions in crawl and batch), D3 (`/ready`), F-t2-016/017/040 |
| `QE/main.rs` `[M]` | mods 26-47; pools 323, 337; meter 451-458; agent driver 1284-1290; serve and shutdown 1513-1535 | D2, D3, D4, F-t2-001; D2-recorded in-flight hunks at 47, 244, 984-1201, 1232, 1395, 1463 |
| `QE/state.rs`, `test_support.rs`, `tests/support/mod.rs` `[M]` | struct 29-150 (one field near `usage` 101); literals 128 and 73 | every item adding state; state.rs hunks 57-93 in flight |
| `QE/extract_routes.rs` `[M]` | meter block 344-360 | D5 (token and parse changes), F-t2-019; in-flight hunks 205-228, 512-563 |
| `QR/usage.rs` `[C]` | metrics 88-94, `UsageEvent` 49-83, `NatsUsageMeter` 119-180 | D4 (metadata), F-t2-014 |
| `QR/step_receipts.rs` `[C]`, `postgres_step_receipts.rs` `[C]` | checkpoint 39-71, `StepReceipt` 183-210, builder 453-556, SQL 143-225 | D2 S3, F-t2-014, 021, 036; migrations `0010` (D2) then `0011` (this) |
| `QC/error.rs`, `event.rs` `[C]` | enums | D2, F-t2-026, 029, 047 |
| `QO/internal/workflows/workflows.go` `[C]` | `CrawlJobInput` 75-87, `runPage` 174+, crawl loops 396-610 | F-t2-016 (scope), F-t2-017, F-t2-040 (batch), F-t2-018 |
| `QO/internal/jobs/dispatcher.go` `[C]` | `listJobs` 342-380, `crawlInputFromJob` 432-454 | F-t2-016/017/018 (job params, scope fields) |
| `QCT/internal/store/pg/resources.go` `[C]`, `store.go` `[C]` | `jobsStore` 241-460, the seven SELECT lists | F-t2-017 (job lifecycle), F-t2-018 |
| `MP/proto/model_plane/v1/browser.proto` `[C]` | messages 25-114 (fields 40-44 here, 10-19 D1, 20-39 F-t2-032) | D1, F-t2-032, F-t2-013 |
| `MP/rust/services/model-gateway/src/browser_run.rs` `[M]` | 297-415 | D1 (grant derive), sibling (b) |
| `MP/rust/services/execution-core/src/browser_agent.rs`, `quarry_agent.rs` `[C]` | 59-71, 320-331, 658-770; 481-705 | D2 follow-ups, sibling (b), F-t2-010 |
| `GW/domains/browser.rs` `[C]` | `router` 369-449; `forward_quarry_failure` 3873-3885 | D1 (`create_session` 451-565), D2 S4, F-t2-011, 013, 026 |
| `GW/rate_limit.rs`, `GW/main.rs` `[C]` | limiter 121-260, layers 143-148 | D4 S-5 (tracing init, span) |
| `IP/docker-compose.yml` `[M]` | quarry-edge service block (216-240) and env | D2, D3, D4, F-t2-001; in-flight hunks 121-160 |
| `CP/docker-compose.yml` `[C]`, `CP/observability/prometheus.yml` `[C]` | billing-core credentials 621; Prometheus 1067-1081; scrape jobs | Control Plane owner, F-t2-022 |
| `BC/internal/billing/service.go`, `defaults.go`, `types.go` `[C]` | 845-919, 61-92, 56-64 | Control Plane owner, F-t2-022 |
| `Q/docs/openapi.yaml` `[M]` | agent paths 263-345, crawl 4361-4374 | D2, D5, F-t2-017, 064; in-flight +78 lines at 238-250 and 4012-4150 |

---

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

| Rule | Statement | Status |
|---|---|---|
| Control Plane owns identity, orgs, billing, quotas, entitlements | All numbers that depend on plan or org are Control Plane data: capacity keys `quarry.cap.*` and allowance policies in billing-core, the per-run ceiling in org-core (read by model-gateway only). Quarry **enforces and never stores or computes entitlements**. The one new read is an authenticated API call (billing-core `quotas/:metric`) with a dedicated read-only principal; there is no database access | OK. Watch: the Quarry estimator rates (`BUDGET_BROWSER_SECOND_USD`, the flat table) are budget heuristics, not prices; if they ever feed an invoice the rule breaks. Prices stay in billing-core and Lago |
| Data Plane owns documents, chunks, traces | no contact | OK |
| Ingestion persists durable knowledge through Data Plane contracts only | receipts, checkpoints, jobs and statistics stay in Quarry's own stores (`quarry-runtime` migration `0011`, quarry-control migrations `016`, `017`); crawl page ingestion is unchanged | OK |
| Provider actions go through integration-corev2 | untouched | OK |
| Model Plane owns reasoning, sessions, browser grants, cost | the budget claim is **minted by the Model Plane** (model-gateway into the broker grant) and Quarry only reads it; cost-core is not written in v1 and its ledger semantics are untouched (L5 is optional); the combined run cap is enforced in execution-core | OK. Risk: the broker proto convention 40-49 must be recorded in the Model Plane decision ledger |
| Application Plane owns projections and notifications | `agent.budget_exhausted`, `meta.budget` and `usage` are inputs for later projections; user-facing alerts go through the existing notify sink (L3) | OK |
| Frontend Plane owns UI and same-origin BFF normalisation | gateway changes are class limiters (S5), a `limits` passthrough (S0) and error-code mapping (S6); no secret, token or upstream detail leaks, and no scoping header is forged | OK |
| No direct database crossing | none introduced; Quarry reads billing-core over HTTP, model-gateway reads org-core over HTTP (existing) | OK |
| Model proposes, Quarry executes or rejects | budgets, caps and clamps are Quarry **rejecting**; the Model Plane's grant claim and request values can only **tighten** (`min`), never widen; the `initiated_by` and class hints are tighten-only; approval authority is untouched (budget gate runs before the one-shot approval is consumed) | OK |
| ZDR propagates through every content-persisting boundary | per slice below | OK with the listed checks |
| Channel Plane is docs-only | not touched | OK |

ZDR per slice:
- **S0.** Crawl and batch are refused under ZDR (`QE/routes.rs:555-588`); a test pins that the new handoff code keeps the refusal.
- **S1.** Accumulators are memory-only; the checkpoint write is skipped for ZDR (existing guards), so a ZDR run keeps no durable spend; the 402 body and `meta.budget` carry numbers and closed words only; `agent.budget_exhausted` goes through `emit_for_zdr` (live subscribers only); no receipt exists for a ZDR run.
- **S2.** Event metadata under ZDR is an allowlist with no URL, host, title, fingerprint, query or content; the `shadow` log carries metric, quantity and class only; a ZDR process crash loses its in-memory segment (under-billing accepted and documented; no durable marker may exist).
- **S3.** The billing-core read carries an org id and a metric name; the 402 `org_allowance` details carry numbers.
- **S4.** The claim is three integers and a closed source word.
- **S5, S6.** No content; the 429 details are numbers and closed reasons.
- **S7.** Backlog rows contain URLs, but only non-ZDR crawl and batch jobs can exist; the test above pins it. Stamped admission values are numbers.
- **S8.** Label policy is a closed-set rule pinned by a test; no URL, host, org, user or run id is ever a label.
- **S9.** Drain writes no checkpoint for ZDR runs; its usage events follow the S2 allowlist.
- **S10.** Statistics are counts and closed classes; non-ZDR only by construction.

Rules at risk: none broken by design. Two to watch: (1) the estimator versus price boundary above; (2) `LIMITS_FAIL_MODE=open` is accepted by configuration, and an operator choosing it removes the only protection during a Control Plane outage (DEC-6), so production overlays should pin `floor`.

---

## 7. Cross-session interfaces

### (b) Model Plane and chat or agents (owner of the Model Plane side: sibling (b); contract owner: this workstream)

| Interface | What Quarry does | What sibling (b) must do | Where |
|---|---|---|---|
| `BUDGET_EXHAUSTED` (402) | refuses further actions, keeps the run resumable, emits `agent.budget_exhausted` | treat it as a resource-limit terminal outcome with partial results, **not** `PlanStatus::Failed` (today any step error fails the plan); show `details.dimension` and `limit_source` | `execution-core/src/quarry_agent.rs` (`AgentClientError`, 481-505; step status handling 680-705), `browser_agent.rs` (59-71, 749-752); `model-gateway/src/grpc.rs:2700`, `tools.rs:1184` map to `failed_precondition` |
| `StartRunData.constraints` and `clamped[]` | returns the effective limits and each clamp with its source | surface them in the run view and in the chat message ("limited to 60 steps by default"); never present the requested value as the applied one | `quarry_agent.rs` `StartRunResponse` (574-600), run events |
| `meta.budget` on step responses; `usage` on `DELETE` | live and final execution spend, steps, attempts, active seconds | enforce the **combined** user cap (planner spend from `cost-core GET /api/v1/cost/run?run_id=` plus Quarry execution spend), add `BrowserResourceLimit::Cost`; record `usage` in the run summary | `browser_agent.rs` `check_limits` (320-331), `PlanConfig.max_cost_usd` (254-255) |
| Grant budget claim (S4) | reads fields 40-44 from `ValidateGrant`, snapshots at `start_run` | model-gateway mints the claim from the request and `org_quota::fetch_org_limits`; the user must confirm model-gateway as minter (alternative: capability-core after ADR 1) | `model-gateway/src/browser_run.rs` (337-407) |
| `initiated_by: "agent"` on `start_run` and every step | classes the run `Agent` | always send it, including when a delegated user bearer carries the call (A6); add it to `StartRunRequest` and `StepRequest` | `quarry_agent.rs` (574-640) |
| `RATE_LIMITED` with `org_plan_cap` or `org_render_cap` on start | typed 429 with `Retry-After` | bounded retry honouring `Retry-After`, then a user-visible "browser capacity reached" (D2 section 7 already asks for the retry) | `browser_agent.rs` (673-695) |
| `RUNTIME_NOT_READY` 503 during drain | refuses new runs for the drain window | retry with backoff | same |
| Planner-on (F-t2-010) | budgets exist | do not enable the planner by default until S1 is `on` and S4 is deployed | `llm_planner.rs` |

### (a) Verevon Space

| Interface | Proposed owner | Notes |
|---|---|---|
| Space-hosted agent browser runs | Quarry run class and budgets (this workstream) | a run started by a service principal is class `Agent` and is budgeted; Space needs no parallel counter. If a Space wants its own cap, add a billing-core `quarry.cap.*` key later, do not invent another counter |
| Surface attribute | Frontend gateway and Quarry | `space` is in the closed enum; the Space domain must send `x-verevon-surface: space` **only** on service-token calls to Quarry; a user token cannot set it |
| Agent PC (sandbox) metering | **not covered here** | Quarry's browser seconds are metered; a Space agent PC runs in the sandbox lease path (memory notes on S3.2/S3.3 sandbox-manager) which has no equivalent event. The user must decide whether sandbox-manager emits a `sandbox.second` usage event through the same billing-core path |
| Presence and cost display | Space and Frontend | render "agent working", `meta.budget` and `agent.budget_exhausted` from Quarry, not from a gateway-local cache |
| ZDR-safe telemetry | this workstream provides the rule | metrics labels are closed sets; events and `meta.budget` are content-free; Space telemetry built on them inherits the rule |
| What the user must reconcile | | (1) whether Space-hosted runs are class `Agent` (budgeted) or `Interactive` (never capped) for TTL and budget purposes, (2) whether a Space member's browsing time bills the org, (3) per-Space limits (none exist here) |

Shared pieces and proposed owners: `LimitsResolver`, `meta.budget`, `clamped`, usage events and the 402 contract belong to Quarry (this workstream); grant budget fields belong to the broker owner (D1); plan and capacity numbers belong to Control Plane (billing-core); the mapping into Model Plane run states belongs to sibling (b); the run view and presence belong to F-t2-014/038 and sibling (a).

---

## 8. What can proceed WITHOUT an ADR or decision, and what is blocked

Proceeds now (each ships a conservative default; none needs a ruling):
- **S0** (crawl cap fix), **S5** (gateway classes), **S8** (metrics), **S10** (statistics): independent.
- **S1** in `shadow` then `on`: resolution is `min(request, org default = env, ceiling)`; the placeholders are ratified before `on` (Question 1).
- **S2** in `shadow`.
- **S6** with env caps and render permits; **S7** with env floors; **S9** drain (needs D2's registry).
- **S3** in `static` mode (resolver, clamp code, floors) without any Control Plane change.

Blocked or gated:

| Work | Blocked by | Why |
|---|---|---|
| S2 `on` (billing the new metrics) | Lago billable-metric codes (external, Question 4) | a new metric needs its code first (`BILLING.md:56-64`) |
| S3 `billing_core` mode and entitlement-driven clamps | the `quarry-edge` credential in the Control Plane compose; a decision on periodic allowance templates per plan (Question 4); ADR 1 for the default fail mode (floor shipped) | plan-default quotas count all-time usage; only allowance policies are periodic |
| S4 grant claim | D1 slice 1b (`DeriveRunGrant`, proto reservation); ADR 1 for the minter location | a claim cannot be minted without production grants |
| Human browser seconds priced or limited | ADR 3 and a product decision | metered now, never capped |
| Orphan settlement for ZDR runs | ADR 4 | nothing durable may exist for ZDR |
| L1 memory-aware admission | D3's multi-Chromium pool, ADR 10 | one shared Chromium cannot be admission-controlled per context |
| L2 spans | F-t2-004 phase 2 (`ZdrSampler`) | spans must not be exported for ZDR runs before the sampler exists (audit note 8) |
| L3 alerts | an ADR on where Prometheus rules and an Alertmanager live | none exists |
| F-t2-010 planner on | S1 `on` and S4 | audit dependency note 4 |

---

## 9. Open questions for the user (real blockers only)

None blocks S0, S5, S8, S9 or S10 from starting. Questions 1 and 2 must be answered before S1 and S3 go beyond `shadow` and `static`; Question 6 before S1's PR merges (it pins a test that D7 pins differently).

1. **Default and ceiling numbers.** Are 60 steps, 900 s and 1.00 USD (agent defaults), 300 steps, 3600 s and 20.00 USD (ceilings), 1000 pages and depth 5 (crawl defaults), 5000 pages and depth 10 (crawl ceilings), 3 concurrent jobs and a backlog of 200 acceptable as the shipped floors? And the estimator rate of 0.00003 USD per browser second, which must come from the billing-core price table before `on`.
2. **Fail mode (ADR 1).** Confirm fail-to-floor (last good for 15 minutes, then the static floor) as the default for the entitlement read, instead of fail-open or fail-closed.
3. **Human browsing.** Confirm that human (interactive) browser seconds are metered but never capped by a run budget, and that whether they are priced is billing-core's table, not Quarry's (ADR 3).
4. **Billing codes and allowances.** Who creates the seven Lago billable-metric codes (names in DEC-10), and does Control Plane want periodic (monthly) allowance policies seeded per plan for `quarry.crawl.page` and `quarry.browser.second.agent`? Without that, the clamp-to-remaining activates only for orgs that already have a policy.
5. **Credential owner.** Who adds the `quarry-edge` principal and its secret to billing-core's service credentials in the Control Plane compose and owns its rotation?
6. **Gate order in `step`.** Confirm that the budget gate runs before the sensitive-approval gate (so a budget refusal never burns a single-use approval), which changes D7's pinned test from `policy_before_approval_before_budget` to `policy_before_budget_before_approval`, or accept the flaw and keep D7's order.

---

## 10. INTERFACE SUMMARY

**Files touched.** Ingestion Plane (Quarry-v2): `quarry-edge` (`agent_routes/{budget,run_live,admission,step,lifecycle,live}.rs` via D2's split, `routes.rs`, `handoff.rs`, `extract_routes.rs`, `map_routes.rs`, `main.rs`, `state.rs`, new `limits.rs`, `usage_attrs.rs`, `metrics.rs`, `metrics_server.rs`), `quarry-runtime` (`usage.rs`, `lease_pool.rs`, `browser_driver.rs`, `step_receipts.rs`, `postgres_step_receipts.rs`, `action_cost.rs`, `grant_validator.rs`), `quarry-core` (`error.rs`, `event.rs`, `envelope.rs`, `contracts.rs`), orchestrator (`workflows.go`, `dispatcher.go`, `activities.go`, new `jobs/limits.go`), quarry-control (`resources.go`, `store`, `pg`), compose, docs. Control Plane: billing-core (`defaults.go`, `service.go`, `types.go`), compose credential, Prometheus job. Model Plane: `browser.proto` fields 40-44, broker, model-gateway `browser_run.rs`; execution-core mapping (sibling b). Frontend: gateway `rate_limit.rs`, `browser.rs`, `knowledge/quarry.rs`.

**New contracts.** Error `BUDGET_EXHAUSTED` (402) with `details.dimension`; `StartRunData.constraints` and `clamped[]`; `meta.budget`; `DELETE` `usage`; crawl and batch `limits`; 429 reasons `org_plan_cap`, `org_render_cap`, `org_backlog_full`; event `agent.budget_exhausted`; seven usage metrics; billing-core `quota.configured` and `quarry.cap.*` keys; internal `POST /v1/internal/jobs/claim`; migrations `0011_step_receipt_cost_source.sql`, control `016_jobs_admission.sql`, `017_job_statistics.sql`; flags `BUDGET_ENFORCEMENT`, `USAGE_BROWSER_METRICS`, `LIMITS_SOURCE`, `LIMITS_FAIL_MODE`, `JOB_ADMISSION`, `GATEWAY_RATE_CLASSES`.

**Dependencies.** Hard: D2 (S1, S2, S6, S9), D1 (S4), Lago codes (S2 `on`). Soft: D4, D3, D7.

**Effort.** S0 1.5 d; S1 5 d (+1.5 Model Plane); S2 6 d; S3 5 d; S4 5 d (+2); S5 1.5 d; S6 4.5 d; S7 5 d; S8 2 d; S9 3 d; S10 4 d. About 47 engineer-days, two streams, plus about 26 unscheduled design-level days (L1 to L7).

**ADR gates.** Topic 1 (fail mode, minter, policy owner), topic 3 (human seconds), topic 4 (ZDR orphan settlement), topic 10 (L1), plus a new Alertmanager decision.

**Parallelisable with.** S0, S5, S8, S10 with anything; streams A and B.

**Conflicts with.** D2 until its split merges (`agent_routes.rs`); D4 (`routes.rs` meter and router); F-t2-016/017/018/040 (`workflows.go`, `dispatcher.go`, `crawl_handoff`); D1 (`browser.proto`, `browser_run.rs`); D7 (`step` gate order, proto field allocation, broker migrations; Question 6); uncommitted `main.rs`, `state.rs`, `IP/docker-compose.yml`, `extract_routes.rs`.

**Key findings to carry forward.** Failed actions never consume step budget; the flat cost table cannot bind a realistic cap; `verified_action_cost_micro_usd` has no producer; crawl pages are unmetered; the dispatcher polls the newest 50 jobs across all orgs; graceful shutdown never completes with a live stream; billing-core already exposes the quota read.

<!-- COMPLETE -->
