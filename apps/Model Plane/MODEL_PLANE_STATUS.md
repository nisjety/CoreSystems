# Model Plane — Current Status

Last source-state reconciliation: 2026-07-16 (Europe/Oslo). Source baseline:
branch `main`, commit `ae3ee041e13d482b20e5883e79044e714bf3d216`, plus a large
pre-existing dirty worktree. Broad source verification completed at 2026-07-16
15:10 CEST; live Docker reconciliation completed at 20:05 CEST; the final
release-tooling contract completed at 22:24 CEST. The running stack is evidence
for this dirty source aggregate only, not a production-release authorization.

Read the current evidence in
`docs/core-research/plane-audit-2026-07-16.md` and the binding release decision
in `docs/core-research/grpc-safe-rebuild-decision-2026-07-16.md`. The dated
2026-07-11 and 2026-07-13 reports are retained as historical evidence, not
current runtime claims.

## Verdict

**The Model Plane is running as a local integration stack, but is not yet a
production-ready MVP and is not enterprise-ready. Do not promote the current
dirty-worktree deployment as the canonical production release.**

At 2026-07-16 20:05 CEST, all 21 Model Plane Compose containers were running;
all 19 containers with health checks were healthy, and OTEL Collector plus
Temporal UI were running without health checks. Eleven application images were
built from the dirty working tree and carry `working-tree` provenance. They are
not signed, attested, or retained as an immutable release artifact. Gateway,
inference, and execution expose authenticated gRPC compatibility listeners on
loopback `:9090`, `:9092`, and `:9093`; the production overlay removes all host
ports. This proves a functioning local integration deployment, not an accepted
production candidate or rollback target.

The initial 17:52 CEST preflight failure is retained as historical evidence.
The later user-authorized rollout supplied runtime-only scoped NATS credentials,
repaired Control Auth service-principal issuance, attached Data retrieval,
graph, and wiki to `inter-plane-bus`, and started the stack without deleting
volumes or rebuilding Control/Data services. Those Model NATS credentials and
the Control Auth principal registry are not yet durable managed release
configuration. Quarry remains absent. Never replace scoped credentials with a
shared token or copy operator connector credentials into Verevon.

Source hardening has advanced materially, but it is not deployment evidence:

- model-gateway and inference-core have additive gRPC listeners and `/readyz`
  stays unready until the listener binds;
- release-artifact format v3 snapshots release inputs and image archives under a
  signed root manifest. Its generated configuration policy partitions every
  Compose input as signed public, external secret, or artifact-owned; unknown
  credential/image-shaped keys fail closed. Auth issuer/JWKS and any asserted
  Azure ZDR route are independently signed and evidence-bound. External inputs
  are securely snapshotted with bounded size and ownership/mode checks, and
  Docker receives only a temporary 0600 secret snapshot. The artifact runner
  accepts only fixed config validation or fixed no-build deployment, so an
  unsigned trailing Compose overlay cannot replace signed images or commands.
  No candidate or rollback artifact exists yet;
- cost HTTP auth, tenant/user scope, and a least-privilege cost NATS consumer
  are implemented and tested in source;
- the ambient gateway `SendMessage` RPC is intentionally quarantined with
  `FAILED_PRECONDITION` before any NATS publish; its broad NATS grants were
  removed, and legitimate telemetry uses the bound per-run event subject;
- approval grant persistence now creates one content-free delivery outbox row,
  with owner-bound durable cache read-through and claim/lease/retry/terminal
  delivery primitives. There is still no restartable continuation descriptor,
  dispatcher, or successful execution receipt;
- Session Core now has an additive managed-run lifecycle contract, migration,
  metadata-only terminalization obligation, lease/recovery worker, and
  service-scoped terminal/heartbeat calls. Gateway and Execution use that
  contract rather than a terminal `CompleteStep` shortcut; this is source and
  focused-test evidence only, not a migrated release database or live path;
- gateway start keys are now keyed-MAC-derived opaque values rather than raw
  caller idempotency keys, and its completed-response cache has fixed key,
  entry, value, and retention bounds; and
- browser execution now preserves terminal meaning in source: explicit success
  alone completes a run; permission denial, approval/runtime timeout, and
  resource exhaustion fail it; cancelled/aborted work uses Session Core
  cancellation; and unsupported browser approval is rejected before dispatch;
- capability policy is an unavoidable direct and agentic dispatch gate; deny,
  ask, and policy outage stop before tool code. The new global-only health
  attestation path is scoped separately from tenant reporters, but no reporter
  is configured or attested, so seeded tools remain unavailable;
- Letta `/v1/tools/search` is an optional non-authoritative ranking input for
  locally authorized tool definitions only; it is disabled for ZDR callers and
  is not semantic-memory search;
- the agent can reformulate and backtrack across typed hybrid retrieval results,
  suppresses exact repeated searches, and reports dependency degradation
  distinctly from an empty corpus; and
- signed retention posture is preserved through execution dispatch and cannot
  be downgraded by a request. ZDR tool output is redacted from durable step
  records, and every unattested inference modality fails before provider I/O.

## Evidence states

| State | Current meaning |
|---|---|
| **Source-verified** | Source and repeatable local tests pass; this alone is not deployment evidence. |
| **Live-verified** | Local Docker health, authenticated listeners, selected authorization, dependency reachability, ZDR denial, and degraded-state probes reproduced on 2026-07-16; not immutable production evidence. |
| **Blocked** | Required implementation, operator input, or live proof is absent. |
| **Enterprise-next** | Deferred until every secure-MVP criterion passes. |

## Final source verification — 2026-07-16 15:10 CEST

The current dirty source aggregate passed these repeatable, non-deployment
checks:

- `cargo test -q -p model-gateway -p inference-core -p execution-core -p
  session-core --all-targets`: Execution Core 225 library plus 17 integration
  tests, Inference Core 127 library plus 15 integration tests, model-gateway
  409 library plus 41 integration tests, and Session Core 119 library plus 17
  integration tests passed. Six Session Core database-gated tests and one live
  Quarry browser test remain intentionally ignored.
- Rust formatting and the targeted `-D warnings` Clippy gate passed with its
  documented legacy lint allowances. This is not a claim that raw strict
  Clippy has no repository debt.
- `go test ./...` and `go vet ./...` passed in capability-core, cost-core,
  orchestrator-core, and browser-broker. The current focused Control Plane Auth
  Core registry/controller/deployment suites pass 93/93; the changed principal
  loader measures 91.96% statements and 91.58% lines.
- Frontend v3 `next typegen`, typecheck, and production `pnpm build` passed.
- Model Plane shell/release contract tests, `buf generate`, and scoped `git
  diff --check` passed. `buf lint` still fails on pre-existing package/input
  debt, and there is no usable non-empty breaking baseline.

This production-MVP program does not require 100% coverage. The stated
risk-based target is 80% measured coverage for changed security/business-
critical modules, with any gap explicitly accepted by the release authority.
The 2026-07-16 all-target `cargo llvm-cov` run reports 59.52% line coverage
across the four Rust services. Changed lifecycle/security paths remain below 80%:
target: execution browser agent 77.90%, execution runtime loop 65.43%,
execution tool bridge 70.67%, gateway browser run 62.49%, gateway session flow
64.53%, gateway SSE 56.43%, and Session Core approval delivery 35.32%.
Capability Core's new global-health functions previously measured 100%
(`GetGlobal`) and 90% (`AttestAvailabilityGlobal`). These gaps must be closed
or explicitly accepted by the release authority before an artifact build.

- The approval outbox has claim/lease, retry/backoff, poison/terminal, and
  acknowledgement-state primitives; it deliberately has no success transition
  that pretends execution restarted.
- NATS configuration uses distinct named principals for its actual Model Plane
  producers/consumers and removes the generic `model-runtime` principal. It
  does **not** configure TLS, mTLS, or workload identity. The only former
  broad gateway publisher is quarantined. Scoped principals are connected live,
  but hostile-principal subject tests and transport identity remain absent.
- The global health-attestation endpoint accepts only its dedicated global
  health scope and operates on the global capability row. No configured,
  attested reporter or live proof exists.
- Signed ZDR is preserved at execution ingress and delegation, while inference
  blocks each unattested modality before provider dispatch. This is fail-closed
  source behavior, not a verified provider route.
- Artifact v3 refuses a mutable/dirty release source and release mode requires
  signed verification, compatibility gates, and an external runtime-env file.
  No candidate or rollback artifact was created.
- `buf lint` legacy violations and the missing usable `buf breaking` baseline
  remain release gates.

These results classify mocks/test servers honestly: Capability Letta tests use
a local HTTP test server; no external Letta, provider, Postgres release
database, NATS deployment, or end-to-end Model Plane is represented as real
production evidence.

## Incremental source verification — 2026-07-16 after the 15:10 baseline

- `cargo fmt --check -p session-core`, `cargo check -p session-core`, and
  `cargo test -p session-core --bin session-core` passed (128 passed, 7 ignored).
  The focused managed-terminalization unit set passed (6 tests), and an isolated
  disposable PostgreSQL 16 regression proved that a legacy terminal
  `CompleteStep` cannot settle a managed run; the temporary database was removed.
- `cargo fmt --check -p model-gateway`, `cargo check -p model-gateway`, and
  `cargo test -p model-gateway --lib` passed (420 passed). Focused idempotency,
  opaque-start-key, fail-closed startup, managed-lifecycle gRPC, and unary/
  streaming E2E tests also passed. Three legacy unused-helper warnings remain;
  no new gateway warning blocks these checks.
- `cargo test -p execution-core --lib` passed (228 passed); `cargo test -p
  inference-core --all-targets` passed (127 library plus 15 integration tests).
  `cargo fmt --check -p inference-core` also passed.
- The default Session Core context path now distinguishes verified empty memory,
  returned entries, and bounded Letta degradation; focused RPC/timeout tests,
  the Session Core suite above, and `go test ./...` for letta-bridge pass.
  Compose health now checks bridge liveness at `/healthz`; semantic readiness
  remains separately visible at `/readyz`.
- The focused Control Plane Auth Core retention/token suite passed 72 tests and
  `pnpm build` passed. Its new interactive policy is deployment-owned exact-org
  configuration: absent configuration means all-ZDR, malformed configuration
  fails closed, and neither a request nor a header can select a persistent
  posture.
- The subsequent Control registry hardening passes 93/93 focused loader,
  controller, and deployment-contract tests plus Auth build, production Compose
  parsing, scoped lint, shell syntax, and diff hygiene. The changed registry
  module measures 91.58% lines. This is source evidence; the healthy running Auth
  container still uses the runtime-only environment registry.
- The final `release-contract`, `release-runtime-partition`, and
  `release-input-security` suites plus Bash syntax passed at 22:24 CEST. They
  validate artifact/rollback guardrails without building or starting a real
  container.

These incremental source/tooling results are supplemented by the live local
evidence below. They still do not establish a usable provider route, immutable
release database/artifact, approval continuation, or rollback rehearsal. The
five-minute SSE/gRPC heartbeat ticker and full browser background-dispatch path
still require clock-driven release evidence.

## Live local reconciliation — 2026-07-16 20:05 CEST

- All 21 Model containers run; all 19 health-checked containers are healthy and
  restart counts are zero; this state was reconfirmed at 22:25 CEST. The three
  protocol-critical gRPC listeners are
  loopback-reachable and reject unauthenticated requests.
- Control Auth Core is healthy and mints the canonical issuer
  `http://localhost:3011/api/convex-auth`. No-auth, malformed credential, and
  wrong-tenant service-token exchanges deny; the exact principal succeeds.
  A valid inference-core token read Session RoutingPolicy successfully.
- A valid Data retrieval-engine identity reached Inference Core. Its signed ZDR
  claim overrode request `zdr=false`; both configured providers were skipped,
  zero provider attempts occurred, and the call failed closed with
  `FailedPrecondition`. No paid provider call was made.
- Data retrieval, graph, and wiki are reachable from Model over the shared bus.
  Gateway, Cost, Capability, Session, and Letta negative authorization probes
  reject no auth, malformed bearer, and forged scope/tenant headers.
- Capability Core accepted a valid execution identity but denied
  `cap.retrieval.query` as `health_not_attested`; cross-organization reuse was
  denied. All 27 enabled catalog rows currently derive unavailable, and no
  global health attestation exists.
- Letta liveness is 200; semantic readiness is 503 with
  `DEGRADED_SEMANTIC_UNVERIFIED`. The configured agent-memory tier would require
  provider-backed embeddings and has no independently verified ZDR route, so it
  was not forced ready with an unsafe or paid probe.
- Session migrations through `0015` and approval/terminalization outbox tables
  are present. Four pre-existing requested approvals have zero delivery rows;
  no approval was granted and no destructive effect was invoked.
- The final release suites pass for artifact v3. They reject unsigned Compose
  arguments, caller-relative runtime files, unsafe file types/modes/sizes,
  concurrent artifact mutation, public/secret/artifact partition drift,
  placeholder Auth/ZDR/rollback evidence, self/same-revision/same-image
  rollback, and runtime secret-schema/keyset drift. A candidate binds a
  separately signed rollback plus opaque secret-version reference and proves
  only `rollback-config-renderable`; live credential, database, health, and
  cutover proof remain later gates. The dirty tree, missing managed signer, and
  missing accepted rollback artifact correctly prevent creation of a
  production artifact.

## Critical blocked behavior

### Live local protocol; no immutable rollback

The gateway/inference/execution listeners are running and authenticated in the
local integration stack. Artifact v3 is present, but production mode
requires an isolated clean revision, managed signing/trust anchors,
non-placeholder compatibility evidence, signed public policy, a private
secret-only runtime file, and candidate-bound rollback runtime evidence. It
deliberately refuses the current dirty tree. A separate known-good rollback
artifact must use a different source revision and image payload; config
renderability is not a live rollback drill.

### Workload credentials are authenticated but tenant delegation is incomplete

Control Auth's runtime registry now issues canonical-issuer, least-privilege
service tokens and the live negative/exact-principal matrix passes. Source work
adds a file-backed secret input so production does not need to place the
credential-bearing registry directly in the container environment. The running
Auth container still uses runtime-only environment configuration and has not
been rebuilt from that source.

Several cross-tenant workers currently require `allowAnyOrg`. Under that
contract, possession of the workload credential is enough to request a token
for a caller-selected organization; there is no separately signed delegated
user/organization grant. This is not an observed bypass of a valid user token,
but it expands the blast radius of a stolen workload key to every tenant. The
production file loader now rejects every `allowAnyOrg` principal outside exact
development/test; production can use only explicit per-principal organization
allowlists until a separately verified tenant-delegation protocol exists. The
running development container still permits the legacy dynamic shape and was
not rebuilt. Caller-supplied organization headers/body fields alone never
establish production scope; dynamic background/interactive paths remain
unavailable until migrated.

### Approval delivery is durable but cannot continue execution

Session Core persists an approval-delivery intent, supports a lease-based
claim, bounded retry/backoff, poison/terminal handling, and acknowledgement
state, and gateway reads the durable record after cache eviction. However, the
agent loop has already exited at the approval gate. There is no durable
restartable continuation descriptor, authenticated continuation dispatcher, or
success receipt that proves execution accepted the continuation. Execution
therefore returns `Unavailable` after validating a grant rather than flipping
process-local state and falsely claiming resume. HITL remains enforced, but
successful post-approval continuation is a P0 blocker.

This is deliberately not “fixed” by re-running the agent: the current pause
path discards the exact tool arguments, so re-planning after a grant could
change or duplicate an external effect. A general continuation needs either an
encrypted, durable exact-effect descriptor or a downstream-owned immutable
intent/receipt contract. Both are retention-bearing. ZDR callers must be
rejected before descriptor creation, and no verified persistent-posture policy,
envelope-encryption/KMS lifecycle, or downstream idempotency/receipt contract
exists today. Approval-required external-effect capabilities must remain
unavailable; grant-to-quarantine is the correct secure-MVP behavior.

### Managed-run terminalization is source-implemented, not yet release-proven

The additive Session Core lifecycle now supplies `StartManagedRun`,
`RecordTerminalOutcome`, and `HeartbeatManagedRun`; migration `0015` stores a
request-to-run binding and metadata-only obligation/receipt. Its leased worker
reconciles open obligations with bounded backoff. The terminal outcome path is
service-authenticated, derives the run owner from durable state, and records
only allowlisted terminal metadata. Gateway and Execution send a narrowly
scoped Session Core service token; direct, gRPC, SSE, and browser flows bind
start/heartbeat/finalization to the managed run. A legacy terminal
`CompleteStep` is now rejected for managed runs, preventing that bypass.

The implementation is deliberately additive: existing `StartRun` behavior
remains for deployed callers. Migrations through `0015` and the terminalization
tables exist in the local database, and real Auth Core service-token issuance
works. A missing final receipt remains `finalization_pending`, never a synthetic
successful run. It still needs an immutable release-artifact migration/recovery
run, old/new caller compatibility, crash/response-loss/cancel-race probes, and a
rollback rehearsal before it can remove the P0 deployment gate. It is also
separate from the still-missing approval-continuation dispatcher.

### Capabilities fail closed until attested

Capability Core migration `0008` binds known execution dispatch names, while
every seeded capability starts unavailable with reason `health_not_attested`.
Execution policy is enforced before all dispatch. A live valid execution-core
identity was denied with `health_not_attested`, cross-organization reuse was
denied, and all 27 enabled rows derive unavailable. A dedicated global-only
health-attestation path prevents a tenant reporter from changing the global
catalog row, but no reporter is configured or attested. A deployment must
provision that reporter and prove `allow`, `ask`, `deny`, stale, unhealthy, and
outage behavior end to end. Dynamic MCP dispatch has no authoritative binding
and is denied.

### All-ZDR versus provider availability

Interactive user credentials default to all-ZDR. Auth Core now has a strict
deployment-owned, exact-organization policy input for a persistent posture:
absent policy remains all-ZDR, malformed policy fails closed, and no request,
header, frontend, or service credential can choose the posture. Signed ZDR
still cannot be downgraded at execution ingress or on delegated credentials,
and inference rejects every unattested modality before provider I/O. Control
Auth now mints canonical-issuer ZDR service tokens and a valid Session
RoutingPolicy read succeeds. A live Data-to-Inference request proved signed ZDR
cannot be downgraded: every configured provider was skipped with zero attempts
and the RPC returned `FailedPrecondition` before provider I/O.

No real organization policy has been provisioned and no provider deployment has
independently verified ZDR eligibility. External inference therefore remains
unavailable under the currently evidenced posture. A release must either
provision and evidence a canonical exact-org retention decision or verify a
ZDR-capable provider, then prove every content boundary end to end. Do not set
a provider-confirmation flag or weaken issuer-monotonic enforcement without
that evidence.

### Letta and retrieval scope

Letta tool search ranks tool definitions; it does not restore Session Core
semantic memory. The default context path now preserves Letta's detailed
outcome: `empty`, `results`, or a bounded `DEGRADED_LETTA_*` code. It emits
content-free outcome/reason metrics and a structured warning rather than
silently collapsing an RPC failure or timeout into an empty memory result.
Compose checks process liveness at `/healthz`; `/readyz` remains the separate
semantic-capability signal.

Live `/healthz` returns 200 while `/readyz` returns 503 with
`DEGRADED_SEMANTIC_UNVERIFIED`. This is an explicit, observable degraded state,
not a silent empty result. The context response remains backward compatible;
authenticated semantic success and Frontend user-facing degraded-state behavior
remain unverified. Agentic RAG
currently uses Data Plane's server-managed hybrid route only. SQL/tabular,
graph, vector-only, and MCP retrieval are not configured. They require typed,
parameterized/allowlisted owner-plane contracts, validated identity, strict
row/time/cost limits, provenance, and ZDR tests; raw model-authored SQL/Cypher
must never be introduced as an MVP shortcut.

## MVP release blockers

1. Isolate an exact reviewed revision; produce and verify signed artifact-v3
   candidate and separate rollback artifacts with managed signing/verification
   keys, non-secret compatibility gates, and an external allowlisted runtime
   environment; rehearse restore without rebuilding.
2. Persist the runtime-only scoped NATS credentials and Control Auth principal
   registry through managed secret files/references; complete exact caller/
   audience/scope compatibility without printing or reusing operator secrets.
   Provision explicitly bounded tenant assignments accepted by the new
   production loader, or later add verified tenant delegation; prove a stolen
   or wrong-tenant workload credential cannot select another organization.
3. Before enabling any approval-required external-effect capability, provision
   verified non-ZDR retention, envelope-encryption/KMS lifecycle, scoped worker
   identity, and tool-specific downstream idempotency/receipt evidence; then
   implement and test an encrypted exact-effect descriptor, dispatcher, and
   receipt. Reuse the existing lease/retry primitives and prove both crash
   windows. Do not re-plan from a granted generic approval.
4. Migrate and prove the managed-run terminalization implementation against a
   release-shaped database and real scoped service tokens: process crash,
   response loss, cancel race, no-side-effect replay, heartbeat expiry, and
   backward-compatible caller behavior without persisting ZDR content.
5. Deploy the dedicated global health reporter and prove capability state at
   catalog, policy, offer, and execution dispatch.
6. Provision and evidence either the exact-org retention policy or a verified
   ZDR-capable provider; prove no content persistence/provider leakage end to
   end without a ZDR downgrade.
7. Run Session Core's ignored database tests and every migration against
   disposable production-shaped Postgres; verify compaction, concurrency,
   approval outbox, and ZDR behavior.
8. Deploy and verify semantic memory or the new explicit operational degraded
   state through authenticated search, `/readyz`, metrics, and Frontend UX.
   Keep Letta tool ranking classified separately.
9. Complete MCP execution DNS/auth/HITL proof and expose Visma as
   `not_configured`; no real Verevon Visma server exists today.
10. Resolve or stage the legacy Buf quality debt and establish a real protocol
   breaking baseline.
11. Start only immutable candidates in safe dependency order and collect live
   curl/gRPC/browser/NATS evidence for no auth, malformed bearer, wrong
   audience/tenant/user/scope, forged headers, internal credential misuse,
   valid access, HITL, cost, ZDR, retrieval degradation, and rollback.
12. A Data Plane internal credential appeared in operator output during live
   validation. Coordinate its secret-manager rotation across Data and Model
   before production, without printing it. Also rotate/revoke previously
   observed third-party connector credentials after moving them to an approved
   mechanism.

## Correct product claims

- The agent/tool loop and server-side HITL gate are real, not mocked.
- A granted approval can be persisted and placed in a lease/retry/terminal
  delivery state, but is **not yet delivered to a restartable execution
  continuation** and has no success receipt.
- A browser run is reported complete only after an explicit successful result;
  denied, timed-out, exhausted, cancelled, and aborted work cannot be presented
  as success. Its source-level managed terminalization receipt/reconciliation
  tables are migrated locally, but crash/recovery and immutable-release proof
  remain absent.
- Plain chat, explicitly selected tools, Browse, Plan, and Agent Run Console are
  distinct modes; Frontend owns intentional tool UX while Model owns the
  capability contract.
- Letta tool search is optional tool-definition ranking. Semantic memory is a
  different path; live readiness explicitly reports
  `DEGRADED_SEMANTIC_UNVERIFIED`, while authenticated semantic success and
  Frontend degraded behavior are not verified.
- Agentic hybrid retrieval supports bounded reformulation/backtracking in
  source. Structured/tabular, graph, and MCP retrieval are not configured.
- No real Verevon Visma MCP server is configured. The status is
  `not_configured`, never the operator's separate Codex/Claude connector.
- No provider or test double is reported as production. Ordinary external
  inference remains unavailable under the current all-ZDR/no-verified-provider
  posture; all unattested modalities fail closed before provider I/O.

## Enterprise readiness

No enterprise claim is made. Workload identity/mTLS, ABAC, HA/regional
failover, DR drills, SLO/error budgets, enterprise audit export/retention,
policy-as-code, key rotation, supply-chain provenance/signing, capacity/cost
controls, residency, and compliance evidence remain in Enterprise-next after
the MVP gates.
