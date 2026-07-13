# Model Plane — Current Status

Last verified: 2026-07-13 (Europe/Oslo). Baseline: branch `main`, commit
`c3b0057e5e0f538e216bf228754f10849b90ce84`, with a large pre-existing dirty
worktree. Runtime and worktree are different deployment states.

## Verdict

**The Model Plane is not production-ready as an MVP and is not ready for a
general rebuild.** The highest-impact failure is already live: the rebuilt
`model-gateway` and `inference-core` containers no longer serve their required
gRPC APIs. Health endpoints remain green, so container health does not prove
chat, inference, embeddings, tools, or MCP are operational.

Do not rebuild or deploy `model-gateway`, `inference-core`, or
`execution-core` from this worktree until the compatibility gates in
`docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md` pass and an
immutable rollback artifact exists.

The source listeners and ordinary HTTP/SSE caller graph are additive again.
Auth Core and Frontend source mint and forward separate exact-audience Model,
inference, execution, session, capability, cost, and Data Plane credentials.
Approval/browser flows now require the exact execution + session credentials,
execution resume must return an affirmative acknowledgement, and Frontend
browser records are bound to the validated user and organization. Session to
Letta also uses an exact, bounded service credential and exposes degradation.

That is still not a deployable release. Auth Core currently marks every
interactive/delegated Model credential `zdr=true`, while durable session and
Letta writes correctly reject ZDR and no provider deployment is independently
confirmed for ZDR (`AZURE_OPENAI_ZDR_CONFIRMED` defaults false). The source
therefore fails closed before ordinary durable chat/provider work. Capability
availability is now fail-closed in Capability Core policy but is not yet an
unavoidable Execution Core authority. Approval persistence and execution
resume still lack a transactional outbox/reconciler, legacy session
data has no trustworthy ZDR provenance, and migrations/integration tests have
not run against a release Postgres. Keep these gates and resolve the policy and
operational blockers before cutover.

## State classification

| Classification | Meaning |
|---|---|
| **Verified live** | Reproduced against the running 2026-07-13 stack or its live database/metrics. |
| **Verified source** | Confirmed from source and tests, but not deployed. |
| **Blocked/unverified** | A required acceptance property has not been proved. |

## Verified live

- `model-gateway` HTTP health on `:8080` is green, but its required gRPC
  listener on `:9090` refuses connections.
- `inference-core` HTTP health on `:18082` is green, but its required gRPC
  listener on `:9092` refuses connections. Default chat/inference and the
  retrieval query-embedding hop therefore fail.
- `execution-core` still has an authenticated gRPC service internally, but
  caller authentication/migration is incomplete; container port publication
  is not proof of end-to-end tool execution.
- The running images report unverified revision/build provenance. No immutable
  pre-removal rollback artifact has been identified.
- `cost-core` exposes live cost records without authentication. The audit read
  43 entries spanning 4 organizations and 5 users, proving cross-tenant
  disclosure rather than a theoretical route-only issue.
- `session-core` compaction is currently succeeding: 479 successful runs, no
  observed failures, and 24 checkpoints since process start with no duplicate
  `(run_id, ordinal)` pairs. The earlier “100% failing” claim is withdrawn.
  Retry/backoff, poison input, concurrency, idempotency under retries, and ZDR
  behavior still need dedicated tests.
- `session-core` gRPC is unauthenticated. An empty-organization pending-approval
  request lists approvals across tenants, and an approval can be decided using
  a caller-supplied actor. This is a critical HITL bypass adjacent to the real
  guarded agentic path.
- `capability-core` HTTP and gRPC surfaces accept unauthenticated access, and
  tenant identity is accepted from request fields.
- `letta-bridge` health is green while semantic search fails on its downstream
  embedding request. It is not an operational memory-search service merely
  because liveness passes.
- The live `visma mcp` record is malformed (`stdio` transport with an HTTPS URL
  and an empty allowlist). No MCP bridge container is deployed. Velion has no
  live Visma runtime integration; the operator-side Codex/Claude connector is a
  separate system and must not be reported as one.
- The running stack is the base/development composition rather than a proven
  production override. NATS and cache credentials are exposed through process
  arguments/healthcheck configuration, the Model Plane NATS connection uses a
  shared token without TLS, and an audit-time storage check found a near-full
  host/Docker volume. No credentials were printed or rotated during this audit.

## Verified source, not deployed

- A new shared Go verifier in `go/pkg/authctx` enforces RS256, issuer, audience,
  token lifetime, unambiguous user/service identity, canonical tenant context,
  and HTTP/gRPC negative cases. Its measured package coverage is **90.0%**.
- `cost-core` source now protects product APIs, pins organization/user scope to
  verified identity, restricts service operations with `cost:read` and
  `cost:write`, scopes run lookup by organization, fails closed when budget
  state is unavailable, requires durable Postgres unless an explicit ephemeral
  development flag is set, and validates finite/bounded usage, attribution,
  NATS subject/envelope/payload identity, and tenant-scoped idempotency. The
  security-critical server and ledger packages measured **86.4%** and
  **91.0%** coverage. The whole Go module measured **58.0%** because main
  lifecycle/telemetry and Postgres integration require external infrastructure.
- `model-gateway` source now forwards the verified bearer and user identity to
  `cost-core`, and budget authority transport/auth/parse failures return 503
  rather than allowing spend.
- Model-gateway now uses separately verified session, inference, execution,
  capability, cost, and Data Plane credentials. Its HTTP JWKS loader rejects
  redirects and non-success responses, bounds connection/total time and body
  size, and is warmed before listeners start. The final full package suite
  passed **407** tests, including **21** authenticated invoke-chain, **3** secure
  gRPC compatibility, and **5** signed-token orchestration HTTP tests.
- Model-gateway MCP source now rejects stdio registrations, malformed
  transport/URL hybrids, raw credentials, empty/prefix allowlists, caller-
  supplied server IDs, private/loopback/link-local/internal HTTP destinations,
  redirects, and oversized responses. DNS is re-resolved for discovery and
  invocation. Discovery failure no longer fabricates callable open-schema
  tools. Targeted tests pass. This is containment, not a deployed Visma
  integration.
- Legacy MCP records without explicit ownership now fail closed, and the direct
  inline chat loop neither advertises nor dispatches `mcp__*` tools. MCP remains
  available only for a future governed agentic bridge with durable approval.
- Session approval storage now rejects blank actor/org identity, scopes list,
  get, decision, and idempotency to the verified user within the organization,
  and uses an atomic `requested`-state compare-and-set. Exact authenticated
  decision retries are idempotent; only the durable CAS winner emits approval
  and resume events, so exact/concurrent replay does not emit duplicates. The
  CAS and process-local broadcast are not atomic, so a crash can still lose
  delivery and requires an outbox/reconciler. The Session Core suite passes
  **122** tests with zero failures and five database-gated cases ignored.
  Global empty-organization boot rehydration is removed. Migration `0011` is
  source-only and has not been exercised against release Postgres.
- Model-gateway now prepares approval requests without cache or event side
  effects, lets session-core validate run ownership and persist first, then
  commits the acknowledged record to a bounded user-owned cache. Same-org
  users cannot list or decide each other's cached approvals, including when
  the durable read is unavailable. This closes cache poisoning/disclosure in
  source. An already-granted retry whose execution delivery is unknown now
  returns explicit gRPC `Unavailable` / HTTP 503 instead of false success and
  does not request another resume. Execution Core atomically resumes only
  `AwaitingApproval`/`Paused` runs; completed, cancelled, running, and unknown
  runs are not reactivated. Decision/resume crash recovery still needs an
  outbox/reconciler.
- `capability-core` source now authenticates product HTTP/unary gRPC APIs,
  derives tenant/actor from verified claims, scopes repositories, makes global
  promotion service/scope-only, and has an additive availability contract with
  server-time freshness, version CAS, safe public reasons, and health audit.
  `EvaluatePolicy` now requires a canonical nonempty scope, uses a
  tenant-bound durable capability/grant lookup, and denies disabled, stale,
  unhealthy, quarantined, deprecated, and unknown-risk entries. Unknown legacy
  risk values and ambiguous legacy grants are quarantined by source migration
  `0007`; malformed revoked scopes are normalized before constraints and both a
  fast regression test and an integration-gated PostgreSQL fixture cover the
  failure mode. The migration has not run on release Postgres. For the MVP,
  policy accepts only `global` and verified-tenant `org`; caller-supplied
  `agent`, `run`, `thread`, `workspace`, and `user` scopes fail before lookup
  until trusted server-derived identifiers exist. Execution Core does not yet
  enforce this policy as an unavoidable dispatch gate. Availability
  attestation updates/audits atomically, but general capability, rollout, and
  scope mutations still ignore a separate audit-append failure; transaction/
  outbox fault-injection remains a release gate.
- Capability Core's HTTP memory API now pins private rows and every mutation to
  the verified actor; same-org cross-user and legacy ownerless rows fail closed.
  Resource-scoped writes are intentionally unavailable until Session Core can
  authorize the referenced resource. Changed memory functions measured
  **81.8–100%**; Capability Core packages measured policy **95.4%**, models
  **93.6%**, server **85.2%**, registry **47.8%**, and
  **55.4%** overall.
- The capability MCP registry boundary now uses bounded strict JSON, disables
  stdio, requires public HTTPS plus exact nonempty allowlists, rejects raw
  credentials and malformed Visma-style hybrids, accepts only managed secret
  references, quarantines invalid legacy rows, and never returns raw config.
  Focused boundary coverage measured **84.49%**. Execution must still re-resolve
  and pin DNS at connect time.
- `letta-bridge` source now distinguishes liveness from structured semantic
  readiness/degraded states, reports the same state through standard gRPC
  health, authenticates memory RPCs, and tenant-pins access. Changed server
  coverage measured **84.5%**. The provider-side semantic failure remains.
- `inference-core` source restores the full additive gRPC contract with eager
  RS256/JWKS verification, signed tenant/ZDR derivation, public standard health,
  and no HTTP removal. Authentication code measured **86.77%** line coverage;
  inherited RPC-method coverage remains **11.50%**, a documented gap.
- Inference provider fallback now fails closed for ZDR unless the exact
  deployment has an explicitly verified capability. Azure confirmation defaults
  false and EU geography alone is not treated as retention evidence.
  Routing-policy HTTP GET/PUT now requires exact `inference:policy:admin` in
  source; the live endpoint remains open until a gated deployment.
- Model-gateway unary/SSE source computes issuer-monotonic ZDR and applies it to
  persistence and inference. SSE now performs the same fail-closed cost budget
  gate before durable work.
- Auth Core and Frontend v3 source now recognize and issue the exact bounded
  audience/scope set for Model, Data, inference, execution, cost, session, and
  capability services. Required issuance failure becomes an explicit 503; it
  no longer silently drops the credential. Three focused Auth Core token/role
  suites passed **40** tests and its production build passed; three focused Frontend
  gateway tests passed. Frontend browser ownership tests pass **53/53**, the
  full gateway passes **235/235**, and new ownership helpers measured **100%**
  line coverage. The ownership store remains process-local and all paths still
  require live proof.
- Session memory search and upsert now bind user-scoped memory to the verified
  thread owner, and the new uniqueness key includes owner so two users in one
  organization cannot read or overwrite each other's memory. Source tests
  pass; no migration/live database proof exists.
- Session compaction now has deterministic IDs, bounded retry/backoff, ZDR and
  poison exclusion, semantic-conflict isolation, and focused tests. Its query
  still aggregates before the batch limit and manual/automatic ordinal writers
  lack shared serialization; the database-gated concurrency/ZDR tests remain
  ignored without a test Postgres.

- During read-only process inspection, an audit command surfaced unrelated
  third-party connector credentials that were already present in process
  arguments. Their values are intentionally omitted and were not reused. The
  affected connector credentials require operator rotation after moving them
  out of argv; this audit did not rotate credentials.

None of these source fixes changes the running containers until a safe staged
deployment is performed. The live IDOR and MCP exposure therefore remain.

## Preserved behavior and corrected claims

- The Model Plane agent/tool loop is real code, not a mock. The agentic `ask`
  path pauses before approval-required tools and persists the approval. Preserve
  that behavior.
- HITL is not globally safe in the running deployment: unauthenticated session
  approval RPCs remain live. Session CAS/tenant containment and direct inline
  MCP denial are fixed in source. Exact approval execution/session delegation,
  affirmative resume acknowledgement, user-scoped durable decisions, and
  retry-idempotency are source-tested. Gateway cache ownership, bounds, and
  persist-before-publish ordering are also source-tested. A durable
  outbox/reconciler and live
  regression evidence are still required to prevent a granted decision from
  remaining paused after process/network failure.
- The previous claim that ordinary Velion v3 chat accidentally omits all tool
  wiring is withdrawn. Current Frontend v3 supports explicit selected tools and
  Plan/Agent Run Console agentic behavior; a plain no-tool turn is intentional.
  The unresolved issue is divergent capability semantics across static UI
  catalogs, ordinary chat, Browse, Plan, gateway discovery, capability-core,
  and execution-core.
- No real Velion Visma server is configured. The correct product status is
  `not_configured`, with an onboarding contract—not “connected” or “working.”

## MVP blockers

1. Safely deploy the source-restored backward-compatible, authenticated gRPC
   contracts and prove chat/inference/tool behavior with dependency-aware
   readiness, immutable rollback, and live positive/negative probes.
2. Deploy and live-verify `cost-core` authentication and cross-tenant denial.
3. Complete residual caller authentication and safely deploy/live-prove the
   source-authenticated session, capability, inference-policy, MCP, browser,
   sandbox, and bridge boundaries; add cryptographic NATS producer identity.
4. Add user-scoped durable approval read-through plus a decision/resume outbox
   or reconciler so cache eviction/crash cannot strand a decision; make browser
   ownership durable/shared for multi-instance operation, and deploy/live-verify
   the source-fixed caller graph without regressing guarded HITL.
5. Restore Letta semantic search or expose a structured, observable degraded
   state through readiness and user-facing contracts.
6. Make the new capability contract an unavoidable model-offer and execution
   authority; keep agent/run/thread/workspace/user scopes rejected until trusted
   concrete identifiers and ownership checks exist, reconcile List/Get/model/
   global semantics, and provision a
   scoped health reporter. Then prove machine-readable
   `available`, `disabled`, `unhealthy`, `approval_required`, and `unavailable`
   states in every chat mode.
7. Resolve the issuer policy that currently makes every credential ZDR while
   durable product flows require non-ZDR, then prove ZDR cannot be downgraded
   and does not persist through
   sessions, compaction, memory, traces, tools, caches, or external bridges.
8. Complete positive and negative live probes, contract/E2E tests, measured
   coverage, deployment gates, and rollback rehearsal.

## Release evidence

The detailed evidence matrix, finding register, acceptance-criteria status, and
test commands are in `docs/core-research/plane-audit-2026-07-13.md`. The safe
rebuild decision is in
`docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md`. Enterprise-only
work is separated in `MODEL_PLANE_ROADMAP.md`; this status does not claim
enterprise readiness.
