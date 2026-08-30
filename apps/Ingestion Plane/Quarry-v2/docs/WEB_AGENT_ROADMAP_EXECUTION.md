# Quarry-v2 web-agent execution ledger

This document is the implementation ledger for the web-capability roadmap. It
keeps the Tavily lineage (discovery → governed acquisition → cited synthesis).
Quarry owns acquisition, browser execution, security, and proof; Model Plane
owns planning and synthesis; Data Plane owns promoted knowledge.

## Implemented in this pass

- `POST /v1/search` remains the canonical Tavily-style SmartSearchRouter API.
- The Python and TypeScript SDKs are regenerated from the OpenAPI contract.
  `sdks/generate.sh` now uses a pinned OpenAPI Generator Docker image rather
  than an unpinned npm wrapper plus ambient Java, so a normal Docker-equipped
  developer/CI machine can reproduce the generated clients. SDK smoke tests
  exercise governed browser/procedure payloads.
- Agent action outcomes are explicit: `verified`, `failed`, or `unknown`.
  Browser API completion without a deterministic business postcondition is
  intentionally `unknown`.
- Agent runs enforce `max_steps`, exact/subdomain `allowed_domains`, and both
  pre-action and post-action navigation checks. `/interact` is a one-action
  ergonomic alias that retains the same grant and receipt semantics.
- Browser observations expose selector alternatives (`id`, `data-testid`,
  `name`, `aria-label`, and `role`) plus URL/title/DOM deltas.
- Successful and failed agent steps append immutable receipts and can be read
  with `GET /v1/agent/runs/{run_id}/receipts`. With the `postgres-queue`
  feature and a successful migration, receipts use the tenant-bound
  `quarry_step_receipts` table; development still uses the in-memory store.
  Closed runs are looked up from the durable store using the verified JWT
  tenant rather than requiring a live Chromium map entry.
- Non-ZDR runs persist an append-only continuation descriptor in
  `quarry_agent_run_checkpoints` after start and after every receipt. A caller
  can request `resume_run_id` on `POST /v1/agent/runs`; the persisted profile,
  constraints, URL, and step are authoritative and the BrowserBroker grant is
  revalidated before resuming. ZDR runs deliberately cannot be resumed from
  durable state.
- Verified receipt streams can be compiled into a deterministic procedure
  candidate with `POST /v1/agent/runs/{run_id}/procedure`; proposed action
  sequences can be checked without executing effects through
  `POST /v1/agent/procedures/replay-check`. Exact action equality is required;
  mismatches return an explicit repair decision.
- Read-only target repair has a serializable candidate score trace and
  `TargetRepairReceipt` decision type. Effectful actions remain approval-bound
  and cannot be silently substituted from similarity alone.
- Procedure candidates have deterministic quality and change-impact checks at
  `/v1/agent/procedures/quality-check` and
  `/v1/agent/procedures/impact-check`; a changed navigation source is marked
  for quarantine/review rather than replayed automatically.
- BrowserBroker grants are accepted on run start and revalidated before every
  action through the configured validator. Production forces the validator
  URL and rejects missing grants. `max_cost_usd` is enforced via the flat
  action-cost table (`crates/quarry-runtime/src/action_cost.rs`) and the
  `AgentLoop` budget tracker; receipts carry `cost_micro_usd` and the fleet
  budget tracker aggregates across member runs.
- Observations include conservative challenge signals (captcha/login/consent/
  access-denied/rate-limit), ordered escalation evidence, and a proof bundle
  linking the action outcome, content fingerprint, and captured artifacts.
- `/ready` is a dependency/readiness probe, not a process-health alias. A
  production process refuses to start with in-memory artifacts, missing
  Postgres, or an ephemeral profile/history backend.
- The Control Plane request-queue view now reads the Rust-owned Postgres
  frontier through an optional read interface and keeps tenant scoping.
- The Go/Temporal crawl worker now uses the Rust frontier bridge when its edge
  queue credentials are configured: seeds/links are idempotently enqueued,
  work is `SKIP LOCKED` popped and acknowledged, and depth is carried in the
  durable payload. The old in-memory BFS remains the test/dev fallback when
  the bridge is not configured.
- The standalone compose stack enables the durable Postgres profile/history
  path and a filesystem artifact volume by default.
- Fleet orchestration: `crates/quarry-runtime/src/fleet.rs` (`FleetTask`,
  `FleetBudgetTracker`, `FleetRegistry`, NATS `quarry.fleet.<id>.>` subjects)
  and `services/quarry-orchestrator/internal/fleet` (Go durable envelope +
  budget tracker) provide the batch-level budget and fan-out/fan-in shape.
  The edge exposes `POST /v1/fleets`, `GET /v1/fleets/:id`,
  `POST /v1/fleets/:id/members`, `GET /v1/fleets/:id/budget` (all org-scoped)
  and the agent events SSE reuses `?fleet_id=` for a single fleet-level
  live view.

## Deliberate gates still required before claiming full production closure

1. Wire the durable receipt/checkpoint store and grant validator into the
   production deployment, and add crash/restart integration tests for
   `resume_run_id`. The descriptor restores the last known URL/profile, but a
   browser provider still needs a verified session/profile rehydration test
   before this is called crash-safe for every provider.
2. Complete DNS-rebinding pinning in the Chromium transport (the current
   navigation/CDP guards re-check every request but Chromium still performs
   its own resolution), then add redirect, iframe/XHR, and subresource SSRF
   tests for every remote browser provider before enabling those providers for
   agents.
3. Expand extraction-profile fixtures to network JSON and visual evidence
  sources; the runtime now executes bounded JSON-LD/DOM profiles, while
  challenge escalation remains a policy/Model decision.
4. Expand the procedure recording/replay/change-impact checks into persistent
   rollout-state and fixture-backed quality gates. Add the Spider-rs adapter
   only after its licence, isolation, and security review. Spider remains a
   benchmark/source candidate, not a new authority or search index.
5. OpenAPI schemas and generated Python/TypeScript clients now cover
   native `/v1/search`, agent start/step/interact, receipts, deltas, procedure
   replay/quality/impact, and run close. Publish versioned SDK packages and
   run the complete edge/runtime/control verification matrix with coverage
   evidence.

These gates are intentionally visible: an endpoint count or a green process
probe is not evidence that durable, tenant-safe agent browsing is complete.
