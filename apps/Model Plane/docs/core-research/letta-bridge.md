# letta-bridge Research Dive

Generated: 2026-07-11 (supersedes 2026-06-09)
Auditor pass: Phase 4 (Model Plane), evidence-graded.

Scope: `apps/Model Plane/go/services/letta-bridge`

## 2026-07-16 delta

Model Plane letta-bridge is running in Docker, reports healthy at `/healthz`,
and had zero restarts in the 2026-07-16 read-only inspection. Semantic readiness
correctly fails closed: `/readyz` returns HTTP 503 with
`DEGRADED_SEMANTIC_UNVERIFIED`. Missing/malformed gRPC credentials are denied.
There is no successful semantic-provider query or authenticated live memory
round-trip, so this is an explicit degraded deployment, not working semantic
recall. Capability Core's optional Letta
`POST /v1/tools/search` integration is separate: it ranks tool definitions and
does not call this memory bridge, search passages, or restore semantic memory.
Source gives default Session Core context assembly a bounded observable degraded
outcome: verified empty, entries, or `DEGRADED_LETTA_*` for RPC/timeout failure,
with content-free metrics and warnings. Compose uses `/healthz` for bridge
liveness while `/readyz` remains semantic capability. The backend degraded
state is deployed and observable, but user-visible Frontend degraded-state
evidence is still absent.

## 2026-07-13 secure-MVP correction

The 2026-07-11 runtime evidence below remains valid for the still-running image: agent-memory semantic search returns an error, while the old `/readyz` incorrectly reports ready. No container was rebuilt during the 2026-07-13 pass.

Source-only remediation now makes the state explicit and fail-safe:

- `/healthz` is liveness only. `/readyz` returns structured backend state and HTTP 503 until a semantic search has succeeded.
- Backend states distinguish `OK`, `DEGRADED_SEMANTIC_UNVERIFIED`, `DEGRADED_SEMANTIC_UNAVAILABLE`, and `DEGRADED_LEXICAL_FALLBACK`. Lexical in-memory/Postgres modes are never advertised as semantic readiness.
- The standard gRPC health service reports the same readiness state.
- All memory RPCs require validated RS256 identity with issuer and `aud=letta-bridge`; tenant access is derived from the token. User/service read and write permissions are separated.
- The incremental memory filter now consistently treats `updated_after` as strictly-after, eliminating the equality-boundary flake described later in this document.
- Missing retention policy and `zdr=true` are rejected before durable search or
  index; liveness remains callable without weakening memory authorization.
- Verification: full tests/vet are green; the changed server package measured
  above 80% coverage.

**Deployment state:** FIXED IN SOURCE ONLY. session-core now has a dedicated
source-level Auth Core caller path: it mints short-lived, organization-bound
tokens through `/api/letta-bridge/internal-token`, requires the exact
`aud=letta-bridge` response contract, and separates `memory:read` from
`memory:write`. Missing caller configuration fails session-core startup closed
when the adapter is enabled; token refusal or bridge failure remains a logged
degraded result rather than fabricated recall. The bridge authorization test
also proves a scoped `service:session-core` principal can search/index only its
signed organization. Rust focused tests and the complete Letta Go suite/vet
pass in source. The current dirty-tree services are deployed locally, but a
positive authenticated Session-to-Letta round-trip is not live-proven. Auth Core's
service-principal registry, a real provider-side semantic repair, legacy
retention provenance, end-to-end ZDR
gating, and a live authenticated index/search probe are still required. The
accurate production status remains **semantic recall unavailable**, never
“healthy” and never a fabricated success.

> Evidence grades used below: **[live-curl]** = observed against a running port on
> this host; **[source-only]** = read from source/config on disk; **[inspect]** =
> from `docker ps` / `docker inspect` (state/config, not exec).
>
> Environment caveat: the Docker containerd content store is corrupted on this
> host — `docker exec`, `docker build`, and `docker logs` fail fleet-wide, so every
> container reports `(unhealthy)` because its **exec-based** healthcheck cannot run.
> This is NOT a signal that the service is down. Health was re-established by
> host-side HTTP probes instead.

## Snapshot

`letta-bridge` is the Model Plane's **agent long-term-memory bridge**: a small Go
gRPC service (`MemoryService`: `IndexMemory`, `SearchMemory`, `Health`) that fronts
a pluggable memory store. It is real, not a mock, and it is wired into the live
chat memory loop via **session-core** (not directly from model-gateway).

What changed since the 2026-06-09 doc:

- The old doc described **two** backends (external + in-memory) and claimed the
  "default runtime is still in-memory." That is now **wrong** on two counts:
  1. There are **three** backend tiers (see below), and
  2. The composed deployment selects the **Redis agent-memory-server (vector)**
     tier by default — in-memory is only the no-dependency fallback.
- letta-bridge is **not** an "unwired/optional" stub. session-core calls it on both
  the write path (memory consolidation / "dreaming") and the read path (context
  hydration).

Non-generated Go source: 13 files, ~1,645 LOC incl. tests.

## Runtime Shape (verified)

- HTTP health `:8088` — `/healthz` → `200 "ok"` **[live-curl]**, `/readyz` → `200 "ok"` **[live-curl]**
- gRPC `:9096` — `MemoryService` (Go `google.golang.org/grpc`) **[source-only]** (`cmd/main.go`)
- Sidecar `agent-memory-server` (image `redislabs/agent-memory-server:latest`) —
  container port `8000`, host-mapped **`:8100`**. Live: `/v1/health` → `200`
  `{"now":...}` **[live-curl]**, `/docs` → `200` (Swagger UI) **[live-curl]**.
- Sidecar `agent-memory-redis` (image `redis/redis-stack-server:7.4.0-v3`,
  RediSearch vector index) — internal `6379` **[inspect]**.
- Container state: `model-plane-letta-bridge-1`, `model-plane-agent-memory-server`,
  `model-plane-agent-memory-redis` all `running`, `Up 2 days`, `health=unhealthy`
  (false signal — exec healthcheck, see caveat) **[inspect]**.

### Backend selection (`cmd/main.go` `buildMemoryServer`, priority order)

1. `AGENT_MEMORY_URL` set → **Redis Agent Memory** (semantic vector recall) via
   `internal/agentmemory` REST client. **This is the tier selected in the composed
   deployment** — compose sets `AGENT_MEMORY_URL=http://agent-memory-server:8000`. **[source-only]**
2. `DATABASE_URL` set (and no `AGENT_MEMORY_URL`) → **Postgres durable store**
   (`internal/pgstore`, table `letta_memory_blocks`, ILIKE substring + recency).
   Compose also sets `DATABASE_URL=…/session_core`, but tier 1 wins. **[source-only]**
3. Neither set → **in-memory substring store** (`internal/memstore`), so the
   service always boots with no external dependency. **[source-only]**

All three implement the same `server.Store` interface; the gRPC layer is
store-agnostic. This is clean, real code.

## How letta-bridge is actually wired into chat (re-verified)

model-gateway does **not** call letta-bridge directly. Its `MEMORY_SERVICE_URL`
and the legacy `LETTA_BRIDGE_ADDR` both point at **session-core:9091**
(`deploy/docker-compose.yml`). session-core is the memory authority; it forwards to
letta-bridge through its `LettaMemoryAdapter` (`LETTA_MEMORY_ADDR` →
`http://model-plane-letta-bridge-1:9096`). **[source-only]**

The chain, both directions (`rust/services/session-core/src/{grpc.rs,dreaming.rs,letta_adapter.rs}`):

- **Write:** `append_message` → `extract_memory_candidates` (dreaming) →
  `sync_candidates_to_letta` → `LettaMemoryAdapter.index` → letta-bridge
  `IndexMemory` → agentmemory `POST /v1/long-term-memory/`. Best-effort, **900 ms
  timeout**, errors logged and swallowed (session-core's Postgres is the durable
  source of truth). **[source-only]**
- **Read:** `append_letta_memory_rows` (grpc.rs:777) → `LettaMemoryAdapter.search`
  (limit 8) → letta-bridge `SearchMemory` → agentmemory
  `POST /v1/long-term-memory/search`. Also best-effort; on error returns empty. **[source-only]**

Consequence: letta-bridge failures **degrade** recall silently but never break chat.
The compose comment that letta-bridge is "currently shadowed" is **stale/misleading** —
session-core does route to it.

## FINDING — semantic recall is broken at the agent-memory-server layer (HIGH)

Live probes against the exact endpoints letta-bridge's `agentmemory` client uses **[live-curl]**:

| Endpoint | Result |
|---|---|
| `POST /v1/long-term-memory/` (index, mirrors `Client.Put`) | **`200 {"status":"ok"}`** |
| `POST /v1/long-term-memory/search` (mirrors `Client.Search`) | **`500 Internal Server Error`** — reproduced on 3 body shapes: minimal `{text,limit}`, namespace-filtered, and full filter set |

The **write** path returns 200 (records are queued via `--task-backend=asyncio`),
but the **search** path 500s on every well-formed query. A 500 (not a 422) points
to a server-side failure during **query-time embedding**, not a bad request. Most
likely root cause: `agent-memory-server` is configured with
`EMBEDDING_MODEL=text-embedding-3-small` / `GENERATION_MODEL=gpt-4o-mini` consuming
`OPENAI_API_KEY`, while the rest of Model Plane runs on **Azure OpenAI**
(`AZURE_OPENAI_*`). `deploy/.env` has `OPENAI_API_KEY` present but it is not
verifiable here and is likely stale/placeholder relative to the live Azure path. **[live-curl]/[source-only]**

Impact:
- letta-bridge's `SearchMemory` receives a non-2xx → returns an error → session-core
  logs `"Letta memory search failed"` and returns empty. **Semantic long-term recall
  in chat is effectively non-functional right now**, but fails safe.
- The async index 200 is a **false-OK**: letta-bridge (and session-core) believe the
  write succeeded, but background embedding almost certainly also fails, so the
  vector index never populates. letta-bridge cannot detect this — the API returns
  before embedding runs. (MEDIUM)

Remediation is a **deployment/creds fix on the sidecar**, not a letta-bridge code
change: point `agent-memory-server` at the Azure deployment
(`AGENT_MEMORY_EMBEDDING_MODEL=azure/<embedding-deployment>`,
`AGENT_MEMORY_GENERATION_MODEL=azure/<chat-deployment>`, with `AZURE_API_*` mapped —
the compose already documents this path in comments) or supply a valid
`OPENAI_API_KEY`. After fixing, re-probe `POST /v1/long-term-memory/search`.

## Stubs / mocks / placeholders

`grep -niE "TODO|FIXME|mock|stub|fake|placeholder|not.?implemented|unimplemented|hack|XXX"`
over the Go source returns **no genuine stubs**:

- The word "stub" appears only in **doc comments** that (accurately) call the
  in-memory `memstore` a substring stub relative to the semantic tier. The store is
  fully functional; it is just the no-dependency fallback, not the runtime backend. **[source-only]**
- `UnimplementedMemoryServiceServer` (server.go:44) is the standard gRPC
  forward-compatibility embed, not an unimplemented method. All three RPCs are
  implemented. **[source-only]**
- No `TODO`/`FIXME`/`mock`/`fake`/`placeholder`/`not-implemented` anywhere. **[source-only]**

Relative to `apps/STALE_DOC_DELETION_REGISTER.md` (which flags Model-plane
STUBS.md / gap-model.md as likely overstating stub status): confirmed — letta-bridge
is **not** a stub and those docs should be revised accordingly for this service.

## Build / test on host

- Toolchain: `go1.26.2 darwin/arm64` (go.mod declares `go 1.25.1`). **[source-only]**
- `go build ./...` → **exit 0** (clean). **[source-only]**
- `go vet ./...` → **exit 0** (clean). **[source-only]**
- `go test -short ./...`: `agentmemory`, `pgstore`, `server` **PASS**; `memstore`
  had **one flaky failure**: `TestTimeRangeFiltering/cutoff_excludes_old_record`
  ("got 2 hits, want 1"). Re-run in isolation **5/5 PASS**. **[source-only]**

  Root cause (LOW, test-only): the test captures `cutoff := time.Now()` immediately
  after the first `Put` with **no sleep between them** (memstore_test.go:194–199),
  then relies on the old record being strictly before `cutoff`. `memstore.Search`
  filters with `r.UpdatedAt.Before(updatedAfter)` (strict), so when the clock does
  not advance between the `Put`'s internal `time.Now()` and `cutoff`, the old record
  is retained → 2 hits. The **production filter is correct**; the fix is a 2 ms
  sleep before capturing `cutoff` (mirroring the sleep already present before the
  second insert at line 202). Affects only the fallback tier, which is not the live
  backend.
  - `pgstore_integration_test.go` requires a live Postgres; not exercised in `-short`.

## Uncommitted WIP

`git status --porcelain` and `git diff --stat` scoped to the service dir are
**empty** — no uncommitted changes. The running container was started 2026-07-09
(binary ~2 days old) and matches committed source, so the usual "Docker serves a
stale binary" caveat does not apply for this service in this pass. **[source-only]/[inspect]**

## Answers to the Phase-4 questions (letta-bridge scope)

- **Real integration or stub?** Real. Three genuine store implementations; the
  composed deployment runs the semantic Redis-vector tier via `agent-memory-server`.
- **What memory store?** Redis Stack (`redis/redis-stack-server:7.4.0-v3`,
  RediSearch vector index) fronted by `agent-memory-server` REST; Postgres
  (`letta_memory_blocks`) is the durable middle tier; in-memory is the fallback.
- **Is it live?** Yes — `:8088` health and the `:8100` sidecar API both respond
  live. Write path works; **semantic search is currently 500-ing (see HIGH finding).**
- **MCP / "test the Visma MCP":** **Out of scope for letta-bridge.** This service
  is memory only — no MCP client, no Visma wiring, no external-tool surface (grep
  confirms zero MCP/Visma references). The MCP question belongs to
  `bridges/mcp-bridge` + execution-core, not here.

## Bottom line

letta-bridge is a clean, real, correctly-wired memory bridge — no stubs, builds and
vets clean, and it is genuinely on session-core's live memory read+write path. The
one operational gap is **not in letta-bridge's code**: the `agent-memory-server`
sidecar's semantic **search** returns 500 (query-embedding failure, almost certainly
OpenAI-vs-Azure creds), so long-term semantic recall is non-functional today while
failing safe. Fix the sidecar embedding config and re-probe. Secondary items: a
false-OK on async index (letta-bridge cannot see downstream embedding failure) and a
timing-flaky memstore test.
