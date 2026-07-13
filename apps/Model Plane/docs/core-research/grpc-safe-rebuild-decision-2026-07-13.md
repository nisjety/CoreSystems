# Decision Record: Do Not Rebuild Yet / Safe gRPC Rebuild

- Date: 2026-07-13
- Status: **Accepted as a release block; execution not authorized**
- Scope: Model Plane `model-gateway`, `inference-core`, `execution-core`, their
  clients, and the session/cost/capability dependencies required by those paths
- Owners: Model Plane runtime and release engineering; cross-plane caller owners
  participate in compatibility gates

## Decision

Do not rebuild, recreate, or deploy the three gRPC-critical services from the
current dirty worktree. Recover with a side-by-side, backward-compatible,
authenticated protocol path and immutable rollback artifacts. Do not perform a
flag-day transport removal.

This decision remains binding until every pre-cutover gate below is evidenced.

## Context

The initial secure-MVP worktree removed or test-gated required gRPC server
surfaces without migrating all callers. Source now restores additive,
authenticated `:9090` and `:9092` services and retains authenticated `:9093`;
combined all-target compilation and full unit/contract suites pass. Auth Core
and the ordinary HTTP/SSE invoke path now use separately verified target-
audience credentials. The running
`model-gateway` and `inference-core` containers still have healthy HTTP
endpoints but no listeners on `:9090` and `:9092`. Default chat/inference and
Data Plane query embedding therefore remain down live. Source restoration is
not a cutover authorization. Exact approval/browser/Letta caller credentials
are source-tested, but Auth Core currently marks every delegated token ZDR,
which durable session/Letta and unverified providers correctly reject.
Capability availability is not yet execution authority, approval decision and
resume lack an outbox, browser ownership is process-local, migrations `0011`
and `0007` have no release-Postgres proof, and sandbox/bridge/background caller
evidence is incomplete. The live positive/negative matrix and rollback
rehearsal do not exist. Readiness does not reveal the outage.

The source tree and running containers are not a single coherent release:

- branch `main`, commit `c3b0057e5e0f538e216bf228754f10849b90ce84`,
  has a large uncommitted diff;
- current images expose unverified revision/build provenance;
- no identified immutable pre-removal image is available as a rollback target;
- the host is near disk exhaustion, making unplanned builds operationally risky;
- security fixes and protocol removal are interleaved, so deploying all changes
  together would make cause and rollback ambiguous.

## Required contracts and callers

The inventory must be generated from the actual proto descriptors and source at
release time. The minimum known dependency set is:

| Server | Current contract expectation | Known caller classes that must be migrated/tested |
|---|---|---|
| inference-core `:9092` | Inference, models/providers, embeddings, routing/policy RPCs | model-gateway (many unary/SSE paths), execution-core, Data Plane embedding/retrieval/graph, capability-core |
| execution-core `:9093` | Agent runs, step execution/resume/cancel, tool execution | model-gateway, Go orchestrator, browser/session resume paths and internal workers |
| model-gateway `:9090` | Gateway/model/capability/MCP-facing RPCs | Frontend gateway/BFF, capability consumers, MCP list/proxy callers, any generated SDK/control clients |
| session-core `:9091` | Runs, events, approvals, compaction/session state | model-gateway, execution-core, Go orchestrator, browser approval lifecycle |
| cost-core HTTP | Pricing, budget, usage/cost | model-gateway, Frontend cost BFF, inference/cost publishers and operators |

Known generated-contract drift, including ZDR fields and run-owner RPCs, must be
resolved before treating compilation as compatibility proof.

## Rejected options

### Rebuild the current tree in place

Rejected. It has already produced green containers with missing protocol
listeners, has no rollback artifact, and combines security and transport changes.

### Remove gRPC and migrate callers later

Rejected. There is no served replacement transport for all RPCs, and the outage
would span Model, Data, and Frontend paths.

### Temporarily expose unauthenticated compatibility listeners

Rejected. It would restore availability by violating the MVP identity boundary.
Compatibility must preserve the RPC shape while enforcing verified audience,
scope, tenant, and user/service identity.

### Treat HTTP liveness as the release gate

Rejected. The current incident proves liveness can be green while required
listeners and downstreams are absent.

## Safe build and deployment sequence

### Gate 0 — Operator prerequisites

No build begins until all are true:

1. An operator confirms sufficient disk headroom for candidate plus rollback
   images, logs, and database migration safety. No pruning/deletion is implicit.
2. Current container IDs, image IDs/digests, creation times, configuration hashes,
   port/listener inventory, database schema versions, and relevant NATS stream/
   consumer state are recorded without printing secrets.
3. A known source revision for each candidate is isolated from unrelated dirty
   work. User changes are preserved; no reset/checkout/clean/revert is used.
4. Candidate and rollback images are built once, labeled with source revision
   and build provenance, pinned by digest, retained locally/registry-side, and
   scanned. Rollback must not require a new build.
5. Backups/checkpoints required for any schema migration exist and a restore
   command has been rehearsed against disposable data.

Failure of any item keeps the decision at **do not rebuild**.

### Gate 1 — Contract and identity freeze

1. Export descriptors for currently expected client RPCs and candidate servers.
2. Add contract tests for every RPC and required field; include old-client →
   candidate-server and candidate-client → old/compat-server matrices where an
   old server still exists.
3. Freeze audiences and scopes. At minimum, define separate caller identities
   for gateway, execution, Data Plane embedding/retrieval, capability,
   orchestrator, and browser/session workers.
4. Verify missing bearer, malformed bearer, wrong algorithm, issuer, audience,
   expiry, ambiguous identity, wrong tenant, forged identity headers, and wrong
   scope fail closed. Valid user and workload identities must retain their
   distinct semantics.
5. Establish credential rotation/overlap behavior before a caller depends on a
   newly protected server.
6. Resolve the retention-policy contract before building: a ZDR credential may
   not enter durable session/Letta work, while a non-ZDR credential must be an
   explicit governed issuer decision. Do not weaken downstream ZDR gates merely
   to restore availability.

### Gate 2 — Build and test authenticated inference side by side

1. Restore the existing inference gRPC contract additively on a shadow internal
   address such as `inference-core-candidate:19092`; do not replace `:9092` yet.
2. Require the frozen inference audience/scopes. Add bounded connect, RPC, and
   provider timeouts, cancellation, idempotency where applicable, and
   ZDR-capability filtering.
3. Point test instances—not production callers—of model-gateway,
   execution-core, Data Plane retrieval/embedding/graph, and capability-core at
   the shadow listener.
4. Pass provider-sandbox/no-paid-load inference, embedding, fallback, failure,
   cost attribution, ZDR, and auth-negative tests.
5. Make readiness fail when the gRPC listener is absent or its required internal
   dependencies are unavailable.

### Gate 3 — Validate execution without regressing HITL

1. Run an execution candidate on a shadow endpoint such as `:19093` with the
   authenticated contract.
2. Update gateway, orchestrator, browser-resume, and session-related callers to
   send the intended user or workload identity. Internal service identity must
   not gain human approval-decision authority.
3. Pass read-only tool, approval-required pause, approval grant/deny, resume,
   cancel, retry/idempotency, direct-MCP denial, wrong-tenant, wrong-user, and
   forged-header tests.
4. Prove no approval-required tool executes before a durable, tenant-scoped,
   compare-and-set decision from verified identity.
5. Prove a decision accepted before a process/network failure is eventually
   resumed or terminally reconciled by an idempotent durable outbox. Client
   retry alone is insufficient recovery evidence.
6. Prove an identical approval retry cannot resume a completed, cancelled, or
   otherwise terminal run and cannot emit a second resume side effect.
7. Until a durable delivery receipt/outbox exists, require an already-granted
   retry with unknown execution delivery to return explicit non-success; never
   treat idempotent decision persistence alone as proof that execution resumed.

### Gate 4 — Validate gateway and downstream policy side by side

1. Run a gateway candidate on shadow HTTP/gRPC endpoints, configured only with
   shadow/passed dependencies.
2. Restore every required gateway gRPC RPC on the candidate and protect it with
   the frozen identity contract.
3. Exercise the Frontend v3 BFF, model catalog, default chat, explicitly selected
   tool, Browse, Plan, Agent Run Console, shipping quote, MCP unavailable, cost
   budget, session approval, and ZDR paths.
4. Deploy/authenticate cost/session/capability dependencies only when both old
   and candidate callers have compatible credentials. Do not create a second
   outage by protecting a server before updating its callers.
5. Verify capability states/reasons and SSE terminal/error behavior. Policy,
   model offers, and execution must all deny disabled, stale, unhealthy, and
   unavailable capabilities from one versioned health attestation. Zero usage
   or a draft/running plan must not be reported as successful completion.
6. Apply session migration `0011` and Capability migration `0007` to disposable
   production-shaped Postgres first. Prove malformed legacy scopes are safely
   quarantined before constraints, and prove same-org users cannot list/decide
   another user's approval or read/overwrite another user's memory, including
   idempotency and concurrency.

### Gate 5 — Cutover order

Use explicit service discovery/config switches; do not overwrite working
containers in place.

1. Deploy protected session/cost/capability dependencies with dual-compatible
   authenticated callers, one boundary at a time.
2. Cut Data Plane embedding/retrieval test traffic to candidate inference; hold
   and observe. Then cut Model execution/gateway dependency traffic.
3. Cut execution traffic to the candidate after HITL and resume gates pass.
4. Cut Frontend gateway test traffic to the gateway candidate; then a bounded
   production slice if available.
5. Move canonical `:9092`, `:9093`, and `:9090` service discovery only after the
   shadow endpoints meet the observation window. Do not remove compatibility
   code in the same release.
6. Keep prior/candidate artifacts and configurations available through the full
   rollback window. Protocol removal is a later change after caller telemetry
   proves zero old use.

The exact orchestration mechanism may be Compose profiles, distinct service
names, or another operator-approved canary mechanism. The invariant is parallel
validation and reversible address switching, not the example port numbers.

## Pre-cutover health gates

Every gate must record command, expected/actual result, timestamp, artifact
digest, real/sandbox classification, and limitations.

| Gate | Pass condition |
|---|---|
| Listener | Required HTTP/gRPC ports accept connections; removed/no-op listeners make readiness fail. |
| Authentication | Full no/malformed/wrong-audience/wrong-scope/wrong-tenant/forged-header/valid matrix passes. |
| Contract | All expected RPCs and required fields pass generated descriptor and old/new client tests. |
| Chat/inference | Seeded authenticated unary and SSE chat complete with real or explicitly classified sandbox inference; terminal errors are honest. |
| Embedding/retrieval | Data Plane query embedding succeeds through the authenticated route; keyword fallback is classified, not confused with semantic success. |
| Tools/HITL | Read-only tool succeeds; approval-required tool pauses; bypass attempts fail; grant/deny/resume are tenant+user scoped; crash recovery reconciles resume. |
| Cost | Usage is attributed once; duplicate/retry is idempotent; wrong tenant denied; budget authority outage fails closed. |
| MCP | Both registry and execution deny malformed/RCE/SSRF/auth/raw-secret/allowlist/size/timeout cases; DNS is re-resolved/pinned at connect; unavailable Visma reports `not_configured`. |
| Capability | Version/freshness/health state is authoritative at policy, offer, and dispatch; health reporters have bounded scoped identity; security mutations and audit commit atomically. |
| ZDR | Issuer policy is explicit; required ZDR cannot be downgraded; unknown legacy retention is quarantined; persistence/provider/bridge probes show no retention. |
| Observability | Readiness, error rates, latency, traces, audit, token/cost usage, and degraded dependencies agree with actual outcomes. |
| Quality | Format, lint, typecheck, unit, integration, contract, E2E, security, migration tests pass; changed critical packages have measured coverage. |

## Rollback triggers

Rollback the most recent boundary immediately if any occurs:

- required listener absent or readiness incorrectly green;
- authentication bypass, cross-tenant access, forged identity acceptance, or
  service credential gaining user authority;
- approval-required tool executes before verified durable approval;
- ZDR downgrade or persisted ZDR content;
- chat, SSE terminal state, query embedding, or tool success rate falls below the
  predeclared canary threshold;
- sustained error/timeout/latency regression beyond the declared observation
  threshold;
- missing/duplicated cost usage, fail-open budget, or unexplained paid-provider
  load;
- audit events omit principal, tenant, action, decision, or correlation ID;
- migration error or data integrity discrepancy;
- dependency degraded state is hidden by readiness or returned as false success.

Exact numeric error/latency/cost thresholds must be written into the release
ticket before cutover; absence of thresholds is itself a failed gate.

## Rollback procedure

1. Stop traffic shifting; preserve logs, traces, metrics, audit IDs, and database
   evidence without printing secrets.
2. Switch only the most recently changed discovery/config address back to the
   prior pinned digest/config. Do not rebuild during rollback.
3. If a backward-compatible migration was applied, leave it in place. If a
   destructive migration somehow passed review, use the pre-rehearsed restore
   procedure and declare the longer outage explicitly.
4. Confirm old listener, auth matrix, chat/inference, embedding, HITL, cost, and
   readiness behavior before resuming traffic.
5. Keep the failed candidate isolated for diagnosis. Do not delete its evidence
   or “fix forward” under live traffic without a new gate review.

Because no verified rollback artifact exists at the time of this decision, this
procedure is **not yet executable**. That is why deployment remains blocked.

## Consequences

Positive:

- security hardening can ship without deleting required behavior;
- compatibility and identity are tested together;
- failures become visible in readiness before traffic cutover;
- rollback is a configuration/address switch to a retained artifact.

Costs:

- temporary parallel services and artifact storage are required;
- callers across Model, Data, and Frontend planes must coordinate;
- gRPC compatibility code remains until telemetry proves migration complete;
- recovery takes longer than an in-place rebuild, but avoids another invisible
  protocol outage.

## Decision exit criteria

This decision may be superseded only by a later dated record that includes:

- immutable current/candidate/rollback digests and source revisions;
- complete caller/RPC/identity inventory;
- all pre-cutover gates with actual evidence;
- exact numeric rollback thresholds and observation windows;
- migration/restore rehearsal results;
- operator and affected plane-owner approval.

Until then: **do not rebuild yet**.
