# cost-core Research Dive

Generated: 2026-07-11 (supersedes 2026-06-09 pass)

Scope: `apps/Model Plane/go/services/cost-core`

## 2026-07-13 secure-MVP correction

The 2026-07-11 observations below remain useful as **historical live evidence**, but their source-code conclusions are no longer current.

- **Live deployment (unchanged, not rebuilt):** unauthenticated tenant reads remain reachable in the running container. A 2026-07-13 safe probe returned 43 ledger rows spanning 4 organizations and 5 users without a bearer. This is a confirmed cross-tenant disclosure and the live service is not production-ready.
- **Source-only remediation (not deployed):** all tenant APIs now require a validated RS256 identity with issuer and `aud=cost-core`; tenant and user filters come from the verified identity, not caller headers or query parameters. Global pricing and liveness/readiness remain public. User principals may read their own usage and personal budget state; service writes require explicit cost scopes. Internal errors are no longer returned verbatim.
- The budget handler and model-gateway caller now **fail closed** when the ledger or cost-core is unavailable. The gateway forwards a separately minted cost-core audience token rather than reusing its Model Plane bearer.
- Production startup now fails if durable Postgres is unavailable. Ephemeral in-memory fallback requires the explicit `COST_CORE_ALLOW_EPHEMERAL=true` development opt-in.
- NATS ingestion now checks that subject organization, envelope organization, and payload organization agree; it also requires event type, consistent user/request identity, and an idempotency key. Per-producer workload identity and encrypted NATS transport are still open platform gates.
- Verification: `go test ./...`, focused race tests, and `go vet ./...` are green. Focused security/business-critical coverage measured 86.4% for the HTTP server and 91.0% for the ledger; the whole module measured 58.0%. Postgres integration coverage was not run because `COST_CORE_TEST_DATABASE_URL` is unset. These are source results, not live deployment proof.

**Deployment state:** do not rebuild this service independently. Authenticated callers and credentials must be provisioned first, then the compatibility/deployment gates in `grpc-safe-rebuild-decision-2026-07-13.md` must pass. Until a staged deployment and negative live probes succeed, finding 1 below remains **OPEN in production** and **FIXED IN SOURCE ONLY**.

Evidence grades used below: **[live-curl]** = observed against the running
service via host HTTP; **[source-only]** = read from source/config on disk;
**[inspect]** = `docker inspect` config/state (no exec — the containerd content
store is corrupted this pass, so `docker exec/build/logs` are unavailable).

## Snapshot

`cost-core` is the Model Plane token/cost ledger. It is **real and non-mocked**,
and materially more built out than the 2026-06-09 doc claimed. It:

- appends per-run / per-org / per-user cost-bearing events to a **durable
  Postgres** store (`cost_entries`), with in-memory fallback for dev/tests
- **prices token counts authoritatively** from a migration-seeded catalogue
  (`model_pricing`) when an event arrives without a `cost_usd`
- exposes an HTTP API for record / usage / run / aggregate / entries / pricing /
  budget-check on `:8089`
- subscribes to `mp.v1.usage.*` USAGE_ENVELOPE events over NATS to record cost
  as runs execute (with a `NATS_FEED_PATH` file fallback for local testing)
- gRPC on `:9098` is **health-only by design** (the Model Plane proto defines no
  CostService; all cost RPCs are HTTP; the model-gateway budget guard is an HTTP
  client). This is not a stub.

The old doc's headline claims — "ledger is in-memory", "gRPC is health-only
[partial]", "usage subscriber still uses placeholder file-input mode until a real
NATS client is wired" — are **stale**. Durable Postgres, real pricing, and a real
NATS subscriber are all present in source. [source-only]

Non-generated Go files: 12 (7 source packages + tests + 2 migrations + Dockerfile
+ entrypoint).

## Live State

- HTTP health `GET /healthz` → **200 "ok"**. [live-curl]
- `GET /api/v1/pricing` → **200**, 14 rates (default + 13 model keys), matches the
  migration `0002` seed exactly. Serves from the in-memory resolver. [live-curl]
- `docker inspect model-plane-cost-core-1` → `running`, `health=unhealthy`,
  started 2026-07-09. The `unhealthy` label is a **false negative**: the
  compose healthcheck runs `curl` *inside* the container and that exec path is
  broken this pass; the service answers host HTTP fine. [inspect]+[live-curl]
- **The durable ledger is currently unreachable in this deployment.**
  `GET /api/v1/cost/run?run_id=...` → **500** with body
  `postgres ledger: run usage: failed to connect to user=postgres
  database=session_core ... 172.21.0.12:5432 (model-plane-postgres-1): connect:
  no route to host`. Record / usage / entries **time out**. So cost is **not
  being persisted right now** and the cost dashboard reads would fail/stale.
  This is (at least partly) the broken-Docker environment — the Model Plane
  Postgres container is not routable — not proof of a source defect, but the
  operational effect is real. [live-curl]

## Runtime Shape

- `cmd/main.go` — HTTP `:8089` (health + API), gRPC `:9098` (health-only), NATS
  USAGE_ENVELOPE subscriber (`mp.v1.usage.*`), ledger + pricing selection.
  `DATABASE_URL` set → durable Postgres; unset/connect-fail → in-memory fallback.
- `internal/ledger/*` — `Ledger` interface + in-memory `Store` (rollups + raw
  entries + idempotency set).
- `internal/postgres/*` — durable pgx-backed `Store`; SQL aggregates at query
  time; `ON CONFLICT (idempotency_key) WHERE idempotency_key <> '' DO NOTHING`
  dedupe. `store_integration_test.go` is gated behind `//go:build integration`
  (`COST_CORE_TEST_DATABASE_URL`).
- `internal/pricing/*` — `Resolver`: exact key → longest-prefix key → mandatory
  `default` row. Seed mirrors migration `0002`. `GET /api/v1/pricing` serves it;
  model-gateway mirrors the same match logic for the SSE display cost.
- `internal/server/*` — HTTP handlers, error mapping, query-filter parsing.
- `internal/telemetry/metrics.go` — OTel counters (requests, tokens recorded,
  budget checks, budget exceeded).

Config (from `deploy/docker-compose.yml`, service `cost-core`): [source-only]
- `DATABASE_URL: postgresql://postgres:postgres@model-plane-postgres-1:5432/session_core`
  — shares the Model Plane Postgres, `session_core` DB. The `session_core`
  database name is **intentional** (shared DB), matching the live error, not a
  misconfiguration.
- `NATS_URL: nats://model-plane-nats-1:4222`
- Joins `inter-plane-bus` with aliases `model-plane-cost-core-1` / `cost-core`
  so the Velion gateway cost dashboard can reach the HTTP API.
- Entrypoint applies `0001` + `0002` migrations via `psql` on boot when
  `DATABASE_URL` is set (no `schema_migrations` ledger; idempotent SQL).

## API And Relationship Map

Producers → cost-core:
- **model-gateway** (Rust) publishes USAGE_ENVELOPE to `mp.v1.usage.{org}`; the
  subscriber records them. It also polls `GET /api/v1/pricing` and calls
  `POST /api/v1/budget/check` (`rust/services/model-gateway/src/{pricing,budget}.rs`).
- **inference-core** (Rust) intent layer calls `POST /api/v1/budget/check`
  (`provider/intent.rs`, `provider/fallback.rs`, `config.rs cost_core_url`).
- The Velion gateway cost/usage dashboard reads the query endpoints over the
  inter-plane-bus.

cost-core → Postgres (`cost_entries`, `model_pricing`) and → NATS (subscribe).

## Findings (ranked)

### 1. No inbound authentication — unauthenticated cross-tenant read/write. HIGH
`RegisterRoutes` wires handlers straight onto the mux with **no auth middleware,
no token/bearer/secret check anywhere** (grep for auth/token/bearer/secret in
source returns zero enforcement). [source-only] Live: `GET /api/v1/pricing`
returns 200 and record/budget/run are reachable **with no `Authorization`
header at all**. [live-curl] Any caller that can reach `:8089` (published on the
host **and** aliased on the inter-plane-bus) can:
- read **any** org's cost/usage via `GET /api/v1/usage?org_id=X&user_id=Y`,
  `/api/v1/cost/aggregate`, `/api/v1/cost/entries` — no tenant scoping, a
  straight cross-tenant IDOR;
- write arbitrary entries via `POST /api/v1/cost/record` (poison a competitor's
  budget posture, or under-report your own).

This confirms the prior "cost-core VALIDATES NOTHING inbound" note. Note the
callers do not even send a key — `model-gateway/src/budget.rs` issues a bare
`.post(url).json(body)` with no auth — so this is unauthenticated end-to-end, not
a "shared key that cost-core forgets to check". [source-only]

### 2. Budget check fails OPEN on any ledger error. MEDIUM
`handleBudgetCheck` (server.go ~293-313) captures `usageErr` but only uses the
usage when `usageErr == nil`; on error it evaluates the caps against
`currentCost=0` / `currentTokens=0` and returns **`allowed:true`**. Confirmed
live: with the DB unreachable, `POST /api/v1/budget/check {org, max_cost_usd:
0.001}` returned `200 {"allowed":true,"current_cost_usd":0,"current_tokens":0}`.
[live-curl] So during any DB outage/blip, **no org is ever over budget**. The
model-gateway caller *also* fails open by design ("cost-core unreachable; allowing
request (fail-open)", budget.rs). Net effect: budget enforcement is advisory and
evaporates whenever the ledger is unhealthy. Acceptable only if that is the
explicit product stance; otherwise the handler should surface a distinct
"budget unknown" signal instead of a clean allow.

### 3. Internal DB error (DSN topology) leaked to unauthenticated clients. MEDIUM
The default arm of `mapHTTPStatus` passes `err.Error()` verbatim into the 500
body. Live, an unauthenticated `GET /api/v1/cost/run` returned the full connect
error including **DB username, database name, container host and IP+port**
(`user=postgres database=session_core ... 172.21.0.12:5432 model-plane-postgres-1`).
[live-curl] Combined with finding 1 this hands internal network topology to any
caller. Handlers should log the detail and return a generic message.

### 4. Durable ledger unreachable in the live deployment. HIGH (operational)
See Live State. Cost is not persisting right now; the dashboard's stored reads
500/time out. Likely environment (Postgres container not routable), not a source
bug — but worth an explicit callout because pricing still answers 200, which can
mask the outage.

## Positives / Real (not stubs)

- Pricing/accounting is genuine: durable append-only ledger, SQL rollups,
  idempotent inserts, authoritative pricing when `cost_usd` is absent
  (`RecordUsage` prices from the catalogue so the dollar ledger and budget
  posture are never silently $0). [source-only]
- The legacy `__org__` sentinel bug (org budget always read $0) is **already
  fixed** — empty `user_id` now aggregates org-wide (server.go comment + code).
- Host toolchain works: `go build ./...` clean, `go vet ./...` clean, `go test
  ./...` green (ledger 7, pricing 5, server 7 test funcs; postgres integration
  test gated behind the `integration` tag). [source-only]
- No genuine stubs/TODO/FIXME/mock/fake in non-test source; the only
  "placeholder" hit is a code comment about parameter-index formatting. [source-only]
- No uncommitted WIP: `git status --porcelain` / `git diff --stat` for the
  service dir are **empty** — everything is committed. [source-only]

## Out Of Scope For cost-core (Phase-4 headline questions)

cost-core is a ledger; it has **no** MCP wiring, **no** tool dispatch, and **no**
HITL/approval logic. The user's "test the Visma MCP", the model-gateway →
execution-core tool loop, shipping tools, and HITL enforcement live in
`bridges/mcp-bridge`, `execution-core`, `session-core`, and `bridge-core` — not
here. cost-core only prices and ledgers whatever runs those services produce.

## Bottom Line

cost-core is real, durable, and correctly priced — no longer the "partial shell"
the old doc described. The substantive gaps are **security, not completeness**:
zero inbound auth (cross-tenant read/write + DSN leak on error) and a budget
check that fails open on ledger errors. The durable path is additionally
**down in this deployment** because the Model Plane Postgres is unroutable, so
cost is not currently being persisted even though `/healthz` and `/pricing`
answer 200.
