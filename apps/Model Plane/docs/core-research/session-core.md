# session-core Research Dive (Model Plane)

Generated: 2026-07-11; corrected and updated 2026-07-13 (supersedes the
2026-06-09 pass while retaining dated evidence below)

Scope: `apps/Model Plane/rust/services/session-core`
Distinct from Control Plane `session-core` (`:3017`/`:50017`, container `session-core-service`). This is the **Model Plane** durable-state authority: gRPC `:9091`, HTTP health/metrics `:18081` (container port `8081`), container `model-plane-session-core-1`.

## 2026-07-13 correction and secure-MVP source state

The headline 2026-07-11 claim that compaction was failing 100% is **withdrawn**. Current read-only live metrics show 479 successful compactions, 0 errors, and 24 persisted checkpoints, with no duplicate run ordinal observed. The prior counters were either misattributed or from a different runtime interval. This corrects the live finding; it does not yet prove poison-input handling, retry/backoff behavior, concurrent idempotency, or ZDR-safe compaction.

The running gRPC service remains unauthenticated. Safe live probes also demonstrated a serious adjacent approval flaw: approvals with an empty organization could be listed, and `DecideApproval` trusted caller-supplied actor/tenant fields. That is distinct from the real execution-core `ask` gate and must not be used to describe HITL as mocked.

Source-only remediation now:

- rejects blank organization and actor identity before approval persistence;
- scopes list/get/pending/decision/idempotency SQL by organization **and
  verified user** for user credentials;
- performs an atomic compare-and-set decision (`requested` only); exact
  authenticated retries are idempotent while wrong-tenant/user and conflicting
  decisions fail closed;
- prevents global empty-organization approval rehydration in model-gateway and rehydrates only a known organization;
- includes focused success, wrong-tenant/user, exact-retry/conflict, and blank-
  identity regression tests.

The complete current source now requires exact `aud=session-core` RS256 identity
on every business gRPC service, pins tenant/user/actor fields, rejects
multi-audience and lateral Data Plane credentials, and leaves only standard
gRPC health public. Frontend and model-gateway source mint/verify/forward a
separate `x-session-authorization` credential; execution-core requires it for
durable run/session/approval work. The current source suite passes **122
tests** with zero failures and five database-backed cases ignored.
Exact/concurrent approval-decision replays now emit neither a second decision
event nor a second resume notification; only the request that wins the durable
compare-and-set emits them. This is at-most-once emission during live
processing, not crash-safe exactly-once delivery: the CAS and process-local
broadcast still require a transactional outbox/reconciler. Ordinary
model-gateway session calls are source-tested, but background orchestrator,
approval-resume, browser, live negative probes, JWKS rotation, deliberate
staged deployment remain open; the live listener
is still unauthenticated and MUST NOT be rebuilt piecemeal.

Compaction source now covers deterministic IDs, bounded retry/backoff,
poison/ZDR exclusion, conflict isolation, statement timeout, and metrics. It is
still not release-complete: candidates aggregate full history before the batch
limit, manual/automatic ordinal allocation is not jointly serialized, and the
Postgres concurrency/ZDR cases are among the ignored tests.

Memory source now returns the thread owner from authorization, filters user
memory by that owner, verifies it again before index, and makes NULL-session
uniqueness `(org_id, scope, owner, key)` in migration `0011`. This closes a
same-org read/overwrite path in source. The migration has not run against a
release database and previously overwritten legacy values cannot be recovered.

The session-to-Letta caller gap is now fixed **in source only**. When
`LETTA_MEMORY_ADDR`/`LETTA_MEMORY_URL` is configured, startup requires
`AUTH_CORE_URL`, `SESSION_CORE_SERVICE_ID`, and
`SESSION_CORE_SERVICE_API_KEY`. Session-core mints short-lived, organization-
bound tokens only through Auth Core's `/api/letta-bridge/internal-token`
contract, accepts only the exact `letta-bridge` response audience, and uses
separate `memory:read` and `memory:write` scopes for semantic search and
dreaming/index calls. The bearer is sensitive gRPC metadata; the token cache is
bounded and refresh-skewed, redirects and oversized responses are rejected,
and the token request contains no prompt, query, or memory content. Missing or
refused credentials remain an explicit logged degraded state and never create
synthetic semantic results. Focused Rust tests cover missing configuration,
exact tenant/audience/scope issuance, wrong-audience rejection, cache reuse,
and bearer redaction. This has not been deployed or live-probed; Auth Core's
service-principal registry must independently authorize the intended orgs and
only `memory:read`/`memory:write`. Letta rejects missing retention policy and
ZDR before durable search/index. Provider semantic search, legacy Dreaming/
replay retention provenance, and end-to-end ZDR remain release blockers.

Evidence grades used below: **[live-curl]** = observed from the running service over the host; **[inspect]** = `docker ps`/`docker inspect` config/state; **[source-only]** = read from disk, not executed. Docker `exec`/`build`/`logs` are unavailable this pass (containerd content store corruption); the session-core Postgres publishes only `5432/tcp` with no host mapping (the dev override drops the base file's `55434:5432`), so **direct DB inspection was not possible** — DB-level facts are graded accordingly.

## Snapshot

`session-core` is a **real, durable, non-mocked** state authority, confirmed live. It owns thread/message timeline, run metadata + lifecycle, append-only event log (with replay), checkpoints, plans/plan-steps/todos, **approvals (HITL)**, subagent lineage, the runtime RoutingPolicy singleton, finetune-job state, and a "dreaming"/memory-consolidation loop that syncs to Letta. Storage is Postgres 16; gRPC is the primary surface with five tonic services registered.

Headline verdicts for the Phase-4 questions this service touches:

- **Run durability is real and Postgres-backed** [live-curl + source-only]. `start_run`, `append_message`, `complete_step`, `get_context_assembly`, `list_conversation`, `get_run`, `list_plans` all show live `status="ok"` gRPC traffic with millisecond latencies and near-zero errors on the hot path.
- **HITL approvals are genuinely persisted and signalled** [source-only]. `create_approval` writes a durable `approvals` row and emits `RunPausedForApproval`; `decide_approval` performs a cross-org ownership check, persists the decision, and emits `RunResumedAfterApproval` **only on grant**. session-core is the durable HITL state; actual loop-blocking enforcement lives in execution-core (out of this service's boundary).
- **No genuine stubs** in the service boundary. The two grep hits for "placeholder" are honest comments (one explicitly forbids inserting placeholder knowledge into context assembly).

The 2026-07-11 sampling initially appeared to show a material background-loop
defect. The 2026-07-13 recheck contradicts that interpretation: compaction
reports 479 successes, zero errors, and 24 persisted checkpoints. Treat the
older counter snapshot below as retained historical evidence from an
unresolved or misattributed runtime interval, not as current health.

The source inventory has expanded since the 2026-07-11 snapshot, including
authentication, compaction tests, NATS connection handling, run service, and
migration `0011`; the older file/LOC count is intentionally not reused.

## Live health & runtime evidence [live-curl] / [inspect]

- `GET http://localhost:18081/healthz` → **200** (the `/health` path 404s; health path is `/healthz`). [live-curl]
- `GET http://localhost:18081/metrics` returns real Prometheus counters (see below). [live-curl]
- `docker ps`: `model-plane-session-core-1` Up 2 days, published `0.0.0.0:9091->9091` and `0.0.0.0:18081->8081`, shown **"(unhealthy)"** — this is the broken exec-based healthcheck (docker exec I/O error), **not** a down service; host curl confirms it serves. [inspect]
- `DATABASE_URL=postgresql://postgres:postgres@postgres:5432/session_core`; DB image `postgres:16-alpine`; `COMPACTION_THRESHOLD=80000`. [inspect: `deploy/docker-compose.yml`]
- Build: `cargo check -p session-core --tests` → **exit 0** in 1m13s on host (cargo 1.94.1); compiles clean under workspace lints. [live-curl/host]

### Live gRPC traffic (healthy hot path)

`mp_session_grpc_requests_total` shows real, near-clean session/run activity: `create_thread` 6 ok, `append_message` 28 ok / 1 error, `start_run` 16 ok, `complete_step` 3 ok, `list_conversation` 34 ok, `get_context_assembly` 14 ok, `list_threads` 12 ok. `run_service` `get_run` 3 ok. `orchestration` `list_plans` 4 ok, `record_orchestration_event` 2 ok. Latencies are sub-second. This is genuine durable session/run usage, no mocks. [live-curl]

### Superseded 2026-07-11 counter interpretation

The following totals were recorded on 2026-07-11, but their runtime provenance
could not be reconciled with the 2026-07-13 counters. They must not be used as
current-state claims:

| Subsystem | ok | error | error rate | note |
|---|---|---|---|---|
| **compaction loop** | **0** | **3265** | **100%** | has *never* succeeded; no synthesized checkpoints being written |
| routing_policy `get_policy` | 1059 | 931 | ~47% | polled by inference-core; singleton row IS seeded (migration 0008) |
| finetune `list_active_jobs` | 2284 | 982 | ~30% | polled by gateway finetune poller |
| dreaming loop | 458 | 196 | ~30% | memory-consolidation loop |
| orchestration NATS bridge | (ok counted) | 11100 `recv_error` | — | JetStream pull recv errors; retry loop |

At the time, the error counts—not the latency quantiles—drove the interpretation.
The later contradictory counter set means that interpretation is historical
and unresolved, not a current incident claim. [live-curl]

## Runtime shape [source-only]

Entrypoint `src/main.rs`: connect Postgres (`PgPoolOptions::max_connections(20)`, no custom acquire/statement timeout), run embedded migrations (panics on failure — service is up, so all 0001–0010 applied), then spawn: gRPC server, HTTP health, NATS consumer, orchestration NATS bridge, compaction loop, dreaming loop. Background loops are supervised with a 1s restart.

gRPC services registered in `grpc::serve` (`src/grpc.rs:2051`): `SessionCore`, `Orchestration`, `FinetuneJobs`, `Memory`, `RoutingPolicy`, `RunService`. **There is no auth/authorization interceptor** — grep for interceptor/Authorization/bearer in `grpc.rs` returns nothing; the service fully trusts callers (gateway / execution-core / orchestrator-core) for tenant scoping.

Schema (migrations): `threads`, `messages`, `runs` (+`ended_at`, `residency`), append-only `events` (proto-envelope columns, `step_ordinal` via trigger, idempotency unique index), `checkpoints` (`ordinal` via trigger), `plans`/`plan_steps`/`todos` (ordinals via triggers), `approvals` (idempotency partial-unique index), `subagent_edges`, `routing_policy` (singleton `id=1`, seeded), `finetune_jobs`, dream/memory tables. All keys TEXT ULID-prefixed. Genuinely well-modelled, single-writer-per-run design.

## Durability & HITL verification [source-only]

- **start_run** (`grpc.rs:346`): one tx inserts the `runs` row (stamping EU `residency`, default `swedencentral`, override `MODEL_PLANE_RESIDENCY`) + a `RUN_STARTED` event, then best-effort creates a durable `plan_{run_id}`.
- **complete_step** (`grpc.rs:500`): tx appends `STEP_COMPLETED` (via `INSERT ... SELECT FROM runs WHERE id=$2` — guards run existence), on terminal status appends `RUN_COMPLETED`/`RUN_FAILED` with `ON CONFLICT DO NOTHING` idempotency, flips run status, mirrors the step into the plan, and best-effort publishes a `velion.audit.v1.model.tool_action` audit event when the step output carries execution-core's `[data_category=… zdr=…]` prefix.
- **create_approval** (`orchestration_grpc.rs:1070`): durable `request_approval` write with caller-honored id + idempotency key; emits `ApprovalStateChanged(REQUESTED)` + `RunPausedForApproval`. A documented prior fix (lines 1091–1096) keeps `plan_id` NULL and stores `step_id` in metadata — binding `step_id`→`plan_id` used to violate the FK and *silently fail to persist the approval* (gate fired, nothing recorded); that is fixed.
- **decide_approval** (`orchestration_grpc.rs:1197`): resolves the row and verifies `org_id` ownership before mutating (Phase-6 cross-org IDOR fix — mismatch is treated as not-found, never a distinguishable "forbidden"); persists the decision; emits `RunResumedAfterApproval` only when granted. Denials/timeouts leave the run paused with no resume signal.

Conclusion: the durable HITL record and the pause/resume signalling are **real, not decorative**, within session-core's boundary.

## Findings (bugs / gaps / risks)

1. **[WITHDRAWN — formerly HIGH/live] The 100%-failing compaction claim does
   not reproduce.** The 2026-07-13 counters show 479 successes, zero errors,
   and 24 persisted checkpoints. The older `0 ok / 3265 error` snapshot is
   retained above for provenance only. Remaining release work is scale,
   poison/retry, concurrent ordinal allocation, ZDR, and isolated-Postgres
   verification—not repair of a currently reproduced 100% outage.

2. **[MED — historical live signal, current state unverified] Elevated
   `get_policy`, `list_active_jobs`, and dreaming error counters were sampled on
   2026-07-11.** Because the compaction counters from the same interpretation
   were later contradicted, these rates require a new timestamped scrape/log
   correlation before being treated as current incidents.

3. **[CRITICAL — live open, source fixed] The deployed run/approval read model
   has no effective inbound identity enforcement.** Current source adds exact-
   audience RS256 authentication, tenant/user pinning, and scoped storage
   predicates, but the live `:9091` listener predates that work. It remains a
   release blocker until caller compatibility, migration `0011`, negative
   identity probes, and staged deployment are proven.

4. **[LOW/MED — source] ZDR flag is not persisted on the durable event log.** [source-only] The `events` table (migration 0002) has no `zdr` column, and `replay_event_row_to_proto` (`grpc.rs:701`) hardcodes `zdr: false` when reconstructing proto `Event`s. session-core *does* durably persist event content (payloads including `goal`/`output`), but the ZDR classification does not survive persistence/replay here — it is only carried inline in the `tool_action` audit prefix. Any consumer relying on a replayed event's `zdr` field always sees false. Relevant to the plane rule "ZDR must propagate through content-persisting boundaries."

5. **[LOW — live] Orchestration NATS bridge `recv_error` = 11100.** [live-curl] The ephemeral JetStream pull consumer (`orchestration_nats.rs`) repeatedly gets receive errors, logging + sleeping 500ms + retrying. This degrades **live** SSE orchestration-event fan-out (`stream_run_events`), not durability — gRPC subscribers re-read durable state from Postgres for replay. Likely idle-pull timeouts on a consumer lacking explicit idle-heartbeat/expiry config; noisy but non-fatal.

## Stubs / mocks / placeholders [source-only]

Refined grep (excluding the "todo" domain noun) found **2 hits, both honest comments, zero real stubs**:
- `grpc.rs:2276` — comment: context assembly "must not insert a placeholder 'knowledge'" (an anti-stub guard).
- `finetune_grpc.rs:332` — comment describing operator-tracked rows that aren't polled.

Every gRPC handler executes real parameterized SQL. Test doubles remain limited
to test modules; five database-dependent security/compaction cases are ignored
without an explicitly isolated `DATABASE_URL`. This **corroborates**
`apps/STALE_DOC_DELETION_REGISTER.md`'s assessment that `docs/STUBS.md`/
`gap-model.md`/`ARCHITECTURE.md` overstate stub status for Model Plane—session-
core's orchestration durability is real and live, while the secure source delta
is not deployed.

## Tests [source-only]

`cargo test -p session-core --no-fail-fast` passes **122 tests**, with zero
failures and five database-dependent cases ignored. The suite now covers
approval CAS/idempotency and event replay suppression, authentication, memory
ownership, compaction failure containment, and existing deterministic runtime
logic. Measured line coverage remains **81.37%** for auth, **82.87%** for memory
gRPC, and **35.30%** overall. The ignored migration/Postgres concurrency cases
and low compaction/dreaming/store coverage remain release gaps.

## Uncommitted WIP [source-only]

The service directory has a large uncommitted secure-MVP source delta. It is
part of the wider user-owned worktree and must not be reset or deployed
piecemeal. The 2026-07-13 review added the approval replay-event gate and its
test without discarding unrelated changes.

## Relationship map

- `model-gateway` → session-core: session/run/orchestration/approval durability; RoutingPolicy read/write; finetune-job state.
- `execution-core` → session-core: `complete_step`, event append (also via NATS `mp.v1.orchestration.>`), approval decisions on in-loop timeout/cancel.
- `orchestrator-core` → session-core: orchestration gRPC proxy paths.
- `inference-core` → session-core: polls `GetPolicy` (RoutingPolicy).
- session-core → Postgres (source of truth), NATS/JetStream (event bridge), Letta/agent-memory (dreaming sync, `LETTA_TIMEOUT` 900ms, best-effort).

## Doc-drift correction

The 2026-06-09 doc's core claim—"real durable authority, not a placeholder"—
**holds and is reaffirmed**. The 2026-07-11 compaction interpretation is
withdrawn after the 2026-07-13 counter recheck. Future passes must correlate
timestamped `:18081/metrics`, container identity, logs, and database evidence
rather than infer an outage from a single counter snapshot.

## Bottom line

Model Plane session-core is a genuinely real, Postgres-backed durable authority
with working sessions/runs and a real HITL record. Current source closes the
known inbound-auth, tenant/user, exact-retry, duplicate-event, and private-
memory paths, but those fixes and migration `0011` are not deployed. The MVP
remains blocked on staged live verification, the five database-gated tests,
transactional approval event/resume delivery, compaction scale/concurrency,
and legacy ZDR provenance. The 100%-failing compaction claim is not current.
