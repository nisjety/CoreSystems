# Model Plane Production-Readiness Audit — 2026-07-13

> **Historical evidence.** The current audit is
> `plane-audit-2026-07-16.md`. The 2026-07-13 runtime described below no longer
> exists locally and must not be treated as current live state.

Scope: `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane`

Timezone: Europe/Oslo (CEST). Source baseline: branch `main`, commit
`c3b0057e5e0f538e216bf228754f10849b90ce84`, plus a large pre-existing dirty
worktree. The audit did not reset, clean, revert, commit, push, rebuild, restart,
prune, delete data, rotate credentials, or execute paid/destructive provider
actions.

This report is the 2026-07-13 delta and does not erase
`plane-audit-2026-07-11.md`. Where the reports disagree, the newer evidence in
this report wins. The older report remains useful historical evidence of the
previous runtime and the evolution of the dirty secure-MVP work.

## Executive verdict

**Not production-ready as an MVP. Not enterprise-ready. Do not rebuild the three
gRPC-critical services yet.**

The 2026-07-11 deploy warning is no longer hypothetical. Rebuilt running
containers for `model-gateway` and `inference-core` have green HTTP health while
their required gRPC listeners are absent. Default inference/chat and the Data
Plane query-embedding dependency fail. `execution-core` retains an authenticated
gRPC path, but its callers have not all migrated to the required identity
contract. There is no verified immutable pre-change rollback artifact.

The current worktree contains meaningful security fixes: authenticated
cost/capability/session/inference/Letta boundaries, fail-closed budgets,
tenant-scoped approval CAS, monotonic gateway ZDR, explicit memory degradation,
and MCP/HITL containment. Those fixes are **source-only**. The running
`cost-core` and session/capability boundaries remain unauthenticated, the live
MCP surface remains unsafe, and deploying the current tree before every caller
has a compatible audience token would compound the outage. The ordinary
HTTP/SSE invoke path and exact Auth Core audience issuance are now source-
tested. Exact approval/browser execution + session delegation, affirmative
resume acknowledgement, Frontend browser ownership, session-to-Letta caller
auth, user-scoped approval/memory storage, and capability MCP write validation
are now source-tested as well. Capability policy now rejects missing scope,
invalid risk, stale/unhealthy/quarantined rollout state, and cross-tenant
durable grants; Capability HTTP private memory is actor-scoped; gateway
approval creation is persist-before-publish with a bounded user-owned cache.
In addition, issuer-required ZDR has no
independently verified eligible provider and is also rejected by durable
session/Letta paths, so the source correctly fails closed rather than offering
a functional rebuilt product. Capability policy is still not an unavoidable
Execution Core dispatch authority; policy therefore rejects every agent/run/
thread/workspace/user scope until trusted server-derived subject identifiers
exist,
approval resume lacks an outbox, database migrations are unproved, and legacy
retention provenance is unknown.

## Corrections to prior claims

| Prior claim | 2026-07-13 determination | Evidence effect |
|---|---|---|
| Running containers predate the gRPC-removal WIP; chat works. | **Withdrawn.** `model-gateway` and `inference-core` were rebuilt; `:9090` and `:9092` refuse connections. | The former deploy landmine is now a P0 live incident. |
| Session compaction fails 100%. | **Withdrawn.** Live metrics show 479 successful compactions, no observed errors, and 24 recent checkpoints without duplicate `(run_id, ordinal)`. | Do not change the working loop based on stale evidence; add targeted failure-mode tests. |
| Ordinary Verevon v3 chat accidentally has no tool wiring. | **Withdrawn as stated.** Current Frontend v3 supports explicit selected tools and Plan/Agent Run Console agentic flows; an ordinary no-tool turn is intentional. | The remaining defect is inconsistent capability/catalog semantics and insufficient UX for intentional business-tool selection, not an unconditional default-agentic requirement. |
| Agentic HITL is decorative. | **False for the guarded `ask` path.** The runtime pauses and persists approval before risky tools. | Preserve the working gate, but close unauthenticated session approval and inline MCP bypasses. |
| Visma is a Verevon capability. | **False.** No deployable Verevon Visma MCP server is configured; the live record is malformed and the operator connector is external to Verevon runtime. | Report `not_configured`; do not fabricate an integration result. |

## Audit method and limitations

1. Read applicable project instructions, plane/cross-plane ownership and status
   documents, the 2026-07-11 audit, per-service research, source, compose, and
   current git diff.
2. Inspected running containers and listeners without mutation. Used safe health,
   metrics, unauthenticated negative probes, authenticated seeded probes where
   available, and read-only Postgres/NATS evidence.
3. Traced gRPC callers and tool/capability paths structurally, then confirmed
   exact sensitive code paths in source.
4. Added tests before security fixes where the active remediation scope allowed,
   then ran focused format/build/test/coverage gates.
5. Did not claim a deployed fix from a source test. “Green health” was not
   accepted as proof of a working protocol or downstream.

Limitations:

- No critical service was rebuilt or redeployed, because the worktree has no
  proven compatible rollback artifact and the host is nearly out of disk.
- No destructive or paid external call was made. Provider/tool evidence is
  historical live telemetry plus source classification unless explicitly noted.
- Full issuer/JWKS-authenticated live matrices cannot pass until the patched
  services are safely deployed.
- Coverage artifacts were removed after measurement; reported figures are the
  retained evidence.
- The per-service research pages still contain 2026-07-11 historical detail;
  this report and `MODEL_PLANE_STATUS.md` are authoritative for changed claims.

## Runtime and source state

| Component | Running state | Current source state | Release determination |
|---|---|---|---|
| model-gateway | HTTP `:8080` healthy; required gRPC `:9090` absent. | Additive authenticated contract restored; exact downstream credentials, startup-bounded JWKS, and HTTP/SSE invoke chain pass source tests. | Source compatibility restored; P0 live outage and cutover blockers remain. |
| inference-core | HTTP `:18082` healthy; required gRPC `:9092` absent. | Full additive 20-RPC gRPC service restored with eager JWT/JWKS auth, tenant/ZDR pinning, standard health, and migrated ordinary gateway callers. | Source compatibility restored; no ZDR-eligible provider and P0 live outage remain. |
| execution-core | Health green; authenticated gRPC remains internally reachable. | Exact `aud=execution-core` ingress is separated from data/session/inference credentials; approval/browser callers now supply separate execution + session credentials and require affirmative resume. | Do not rebuild or cut over independently; no live resume proof/outbox. |
| session-core | gRPC `:9091` live and compaction healthy; approval RPCs unauthenticated. | Business gRPC is authenticated and tenant/user-pinned; approval CAS, exact retry idempotency, user-owned memory, ZDR write denial, and exact Letta caller auth pass source tests. Migration `0011` is unapplied; database tests remain gated. | Critical live HITL boundary remains; source containment is not deployed. |
| cost-core | Health green; live ledger readable without auth. | Auth/tenant pinning, fail-closed budget, durable-startup gate, and NATS identity/idempotency checks added. | Source fixed; live vulnerability and caller cutover remain. |
| capability-core | HTTP `:8085` and gRPC `:9097` live without auth. | Product HTTP/unary gRPC authenticated; additive availability state/freshness/version-CAS/audit exists; strict MCP records use managed refs and exact allowlists. Execution does not yet consume availability. | Source containment is partial; authority/execution/live matrix remain. |
| letta-bridge | Health green; semantic search downstream times out/fails. | Structured degraded readiness/gRPC health, authenticated tenant-scoped RPCs, exact session caller token, and missing/ZDR retention denial added. | Source accurately degrades and fails closed; provider repair/live proof remain. |
| MCP registry/gateway | Live malformed Visma record; no bridge deployment; unsafe legacy execution surface. | Both gateway and capability registry disable stdio, reject malformed/secret-bearing records, require exact allowlists, and hide/quarantine invalid config. Gateway re-resolves at dispatch; registry-only DNS validation is not execution authority. | Containment is source-only; no Visma integration. |
| browser/sandbox/bridge | Containers live; sensitive boundary/auth/caller semantics incomplete or dormant. | Frontend browser metadata is user+org-owned across every HTTP/WS path; exact downstream tokens are required. Ownership is process-local; sandbox/bridge caller/live proof remains incomplete. | Quarantine until caller and deployment E2E passes. |
| NATS/observability | Shared token/no TLS; session NATS bridge degraded while readiness is green. | Cost ingestion validates subject/envelope/payload/producer; no cryptographic workload-identity/durable-consumer migration. | MVP producer/auth/readiness evidence plus enterprise-next transport work. |

## Exact path findings

### Default chat, Browse, Plan, Agent Run Console, and shipping

Frontend v3 uses the chat stream through the same-origin gateway to
model-gateway. Current source supports explicitly selected tools and Plan-mode
agentic behavior; a plain turn can intentionally remain tool-free. Browse offers
web tools; Plan/Agent Run Console can offer business tools such as shipping.
Execution-core owns the governed agent/tool loop and its `ask` posture persists
approval before risky execution.

All of these modes currently converge on the unavailable inference gRPC path, so
source wiring is not live success. Once inference is restored, the release test
must cover:

- plain intentional no-tool chat;
- explicit business-tool selection;
- Browse search/fetch and citations;
- Plan and Agent Run Console tool offers;
- `shipping_carriers` and `get_shipping_quotes` without booking;
- approval-required write tools paused before invocation;
- unavailable/unhealthy tools rendered with machine-readable reasons.

The frontend owns selection and presentation. The Model Plane owns a stable,
authoritative capability contract. Forcing every message into agent mode is not
an acceptable substitute for that contract.

### Capability authority and private memory

Capability Core source now requires authenticated service identity for policy
evaluation and a canonical nonempty scope. Policy resolves the capability for
the verified tenant, consumes fail-closed availability and rollout state, and
denies disabled, stale, unhealthy, quarantined, deprecated, or unknown-risk
entries. Durable scope grants carry `org_id`. Migration `0007` quarantines
invalid legacy risk rows, revokes ambiguous legacy grants, and normalizes
revoked malformed scope rows before adding constraints. A fast regression
passes and a PostgreSQL fixture compiles/skips without an isolated DSN; it has
not been executed against release-shaped Postgres.

This closes the reviewed omission/cross-tenant/unknown-risk paths, but not the
whole execution contract. For the MVP, only `global` and verified-tenant `org`
are accepted; `agent`, `run`, `thread`, `workspace`, `user`, empty, and unknown
scopes fail before registry/grant lookup. Execution Core does not yet make this
RPC an unavoidable pre-dispatch gate. Re-enabling resource/agent scopes requires
trusted identifiers and ownership checks, so those paths remain an MVP blocker
rather than being reported as authoritative.

The separate Capability HTTP memory API now requires signed organization and
actor identity for every route. Private user/session/resource rows require a
nonempty matching owner; ownerless/unknown legacy rows fail closed. Org/global
rows remain tenant-shared. Caller-selected resource-bound writes are rejected
until Session Core can validate the referenced resource. Same-org cross-user
read, resolve, update, and delete tests pass.

### MCP and Visma

The legacy source could register and spawn caller-selected stdio programs during
discovery/call while inheriting service/provider environment. Its HTTP transport
also allowed SSRF-relevant caller destinations, and raw role/header and
allowlist semantics were insufficient. The live registry includes a malformed
Visma-style record: stdio plus an HTTPS URL and an empty allowlist. No bridge
container/image is deployed and its ports are closed.

The source patch now:

- accepts HTTP transport only for the MVP and requires a public HTTPS base URL;
- rejects stdio, raw credentials, empty allowlists, invalid exact tool names,
  caller-supplied IDs, userinfo, query/fragment, loopback/private/link-local/
  metadata/internal targets, redirects, and oversized responses;
- re-resolves DNS at discovery and call time and rejects a host if any resolved
  address is forbidden;
- disables proxies, bounds connect/total time, and fails discovery closed;
- uses exact allowlist matches and no longer accepts `x-user-role: admin` as
  authority.

Focused tests pass, but this code is not deployed. The direct tools-only path no
longer advertises or dispatches `mcp__*`; a future MCP bridge must enter only
through the governed agentic approval loop. A real Visma outcome additionally
needs a deployable server, correct transport, OAuth/secret reference, exact
allowlist, health/discovery, audit, E2E invocation, and rollback. Until then the
status is `not_configured`.

### Cost and budget

The live no-auth API returns tenant-scoped ledger data. The read-only audit
observed 43 entries spanning 4 organizations and 5 users. This is an exploitable
cross-tenant disclosure and the same trust model permits unsafe operations.

The source patch introduces a shared verifier and protects cost product APIs.
Organization/user identity is pinned to signed claims; service operations need
`cost:read`/`cost:write`; run lookup is organization-scoped; missing auth
middleware fails closed; and budget database/auth/transport/parse errors return
503 rather than allowing spend. Model-gateway and the Frontend v3 BFF now mint
and forward a separately verified `aud=cost-core` bearer.

This rollout still needs a pricing compatibility decision and all non-gateway
callers updated before deployment. NATS ingestion must also prove subject,
envelope, and payload tenant equality.

### Session compaction and HITL

Compaction success is live evidence. Source tests now cover deterministic auto
checkpoint IDs, bounded retry/backoff, poison/ZDR exclusion, statement timeout,
semantic-conflict isolation, success/failure metrics, and concurrent-cycle
logic. Five Postgres-gated session tests remain ignored. The selection query
still aggregates full history before its 100-run limit, and manual plus
automatic `MAX(ordinal)+1` writers lack one shared lock; scale and writer-race
acceptance is therefore still open.

Separately, session-core's orchestration gRPC boundary is unauthenticated. An
empty organization lists approvals across tenants. `DecideApproval` accepts a
caller-supplied actor and the store update is keyed by approval ID without the
required organization and `requested` compare-and-set predicate. An attacker
who reaches the listener can grant another tenant's approval. This bypass does
not invalidate the real `ask` gate; it bypasses the authority that resolves the
gate.

Source now authenticates every business RPC, derives organization/user from
verified identity, and scopes approval list/get/decision/idempotency by user
inside the organization. The write CAS uses `(approval_id, org_id, user_id,
status=requested)`; exact authenticated retries are idempotent while conflicts
fail, and only the CAS winner emits approval/resume events. Exact/concurrent
replay emits no duplicates. The CAS and process-local event broadcast remain
non-atomic, so a crash can still lose delivery. Execution Core atomically
resumes only `AwaitingApproval`/`Paused`; completed, cancelled, running, and
unknown runs remain unchanged. All-org unauthenticated rehydration is removed.
Gateway approval/browser
calls carry separate execution and session credentials and require affirmative
execution acknowledgement. Decision persistence now precedes the gateway's
in-memory mutation. Approval creation now also reaches session-core ownership
validation and durable persistence before any bounded, user-owned cache insert
or lifecycle publish. Cache-only degradation filters the exact verified user,
and same-org foreign users cannot list or decide cached approvals. Persistence
and execution resume still need durable read-through plus an outbox/reconciler
for cache eviction and crash recovery. An already-granted retry with unknown
execution delivery now returns explicit non-success (`Unavailable`/503) and
requests no second resume rather than fabricating success. Migration `0011`
and concurrency behavior have not been exercised against release Postgres.

The same review found user memory isolation missing inside one organization.
Source now returns the verified thread owner from authorization, filters NULL-
session user memory by that owner, verifies the owner again on index, and
changes the unique key to `(org_id, scope, owner, key)`. The previously
overwritten legacy value cannot be reconstructed; rollout requires migration
and data classification before enabling durable memory.

### Letta semantic search

Liveness is green while `SearchMemory` fails on the agent-memory embedding
dependency. Agent Memory auth is disabled and the expected embedding key/config
is absent in the running stack. Source now authenticates and tenant-pins memory
RPCs and returns structured readiness/degraded state through HTTP and standard
gRPC health. Session Core mints a dedicated exact-audience/scope credential and
treats Auth Core or bridge failure as degraded; Letta rejects missing retention
policy and ZDR before persistence. The live provider remains broken. The
MVP must either restore a real compatible embedding route or deploy and surface
that explicit degraded state; empty/irrelevant “success” is not acceptable.

### ZDR

Initial source review found that request-body ZDR could downgrade issuer-enforced ZDR in
unary/SSE handling, provider fallback was not filtered by ZDR capability, and a
Data Plane graph call is hardcoded with `zdr=false`. Persistence boundaries
across sessions, compaction, memory, traces, tools, caches, and external bridges
are not covered by one end-to-end proof.

The invariant is monotonic: verified issuer policy can require ZDR; a request
may tighten but never loosen it. This is source-fixed for gateway unary/SSE and
inference unary/stream/embed. Inference rejects routes without an explicitly
verified deployment-level ZDR capability before provider I/O; Azure confirmation
defaults false and region is not treated as retention evidence. Session and
Letta durable writes now fail closed for issuer ZDR, exposing a release-blocking
policy contradiction because Auth Core marks every delegated token ZDR.
Dreaming Core still scans legacy messages without explicit retention provenance
and replay labels persisted events `zdr=false`; legacy data must be quarantined
or migrated to an explicit retention contract before claiming end-to-end ZDR.
Non-infer/embed modalities, tools, traces, and bridges remain unproved.

## Test and coverage evidence

| Area | Command | Result | Classification / limitation |
|---|---|---|---|
| Shared Go identity verifier | `cd go/pkg/authctx && go test -coverprofile=coverage.out ./... && go tool cover -func=coverage.out` | Pass; **90.0%** statements. | Verified source; not deployed; eager JWKS load lacks runtime rotation refresh. |
| Cost service | `cd go/services/cost-core && go test ./...` | Pass. | Verified source; live container still old/unauthenticated. |
| Cost accounting/auth packages | `go test ./...`; focused coverage; `go test -race ./internal/ledger ./internal/server ./internal/postgres ./cmd`; `go vet ./...` | Pass; server **86.4%**, ledger **91.0%**, module **58.0%**. | Critical packages meet the target. Postgres integration is defined but was not run because `COST_CORE_TEST_DATABASE_URL` is unset; main lifecycle/telemetry remains uncovered. |
| Capability authority/availability/memory | `go test ./...`; `go vet ./...`; `go test -race ./...`; coverage profile; migration regression + integration-gated fixture | Pass; policy **95.4%**, models **93.6%**, server **85.2%**, authz **91.7%**; private-memory functions **81.8–100%**; module **55.4%**. Only `global` and verified-tenant `org` policy scopes are accepted. | Registry remains **47.8%**; migration `0007` compiles/skips without an isolated PostgreSQL DSN and has no release-Postgres proof; policy is not yet unavoidable execution authority. |
| Capability MCP registry | full/race/vet plus focused coverage | Pass; boundary **84.49%** (267/316 statements). | No live Postgres; registry DNS validation must be repeated/pinned by execution. |
| Letta bridge + authctx | full Go tests and vet; focused server coverage | Pass; changed Letta server previously **87%**; shared authctx retention claim tests pass. | Source reports degraded semantic state and rejects missing/ZDR retention; live provider remains broken. |
| Inference core | full library plus cache/fallback integration targets | **124** library, **7** cache, and **8** fallback tests passed (**139** total); auth **86.77%** lines. | Existing gRPC implementation coverage is **11.50%**; live deploy and provider route unproved. |
| Session approval/memory/compaction containment | `cargo test -p session-core --no-fail-fast`; strict clippy; `cargo llvm-cov -p session-core --summary-only` | **122** passed, 0 failed, **5 DB-gated ignored**; auth **81.37%** lines, memory gRPC **82.87%**, overall module **35.30%**. | Exact/concurrent decision replay emits no duplicate events. CAS-to-broadcast remains non-atomic; low dreaming/store/compaction coverage and absent migration/Postgres proof are release gaps. |
| Execution core | Full package suite; strict clippy/fmt | **201** passed; **1** live-stack Quarry E2E ignored. New atomic resume function measured **93.75%** LLVM region coverage. | Exact ingress/downstream audience separation and terminal-run replay denial are source-tested; live deployment remains. |
| Model gateway | Full package suite, focused approval/capability tests, invoke-chain, gRPC compatibility, and signed orchestration HTTP tests | **407** passed with 0 failed/ignored; invoke-chain **21/21**, compatibility **3/3**, orchestration **5/5**. | Approval create is persist-before-publish with bounded user cache; unknown delivery returns explicit non-success, exact replay requests no second resume, and terminal resume fails closed. Outbox, durable cache read-through, live listener, provider ZDR, and rollback remain. |
| MCP secure registration | `cd rust && cargo test -p model-gateway --lib mcp_secure_registration_tests` | 7 passed. | Verified source; no live deployment/negative probe. |
| MCP exposure/allowlist | `cd rust && cargo test -p model-gateway --lib mcp_exposure_tests` | 5 passed. | Verified source; inline `mcp__*` advertisement/dispatch is removed, but full governed bridge E2E is pending. |
| Admin authority | `cd rust && cargo test -p model-gateway --lib ownership::tests::is_admin_claim_uses_only_signed_scopes -- --exact` | 1 passed. | Verified source. |
| Fail-closed budget client | `cd rust && cargo test -p model-gateway --lib budget::tests` | 2 passed. | Verified source; live deployment incomplete. |
| Auth Core issuance | three focused token/role Jest suites; `pnpm run build` | **40** tests passed; production build passed. | Exact bounded audience/scope issuance and least-privilege capability write roles are source-tested; no live cutover. |
| Frontend v3 gateway browser ownership | focused browser + full gateway/check/clippy/coverage | Browser **53/53**, full gateway **235/235**; ownership helpers **105/105 lines (100%)**. | Process-local ownership is not HA/durable; no live proxy contract. |
| Rust quality gates | `cargo fmt --all -- --check`; combined all-target check; strict clippy for the four critical Rust services | Pass with no warnings. | Does not replace live integration/deployment gates. |
| Other Go boundaries | full `go test ./...` in browser-broker, sandbox-manager, and bridge-core | Pass. | Source authentication/tenant containment only; caller/live E2E remains. |
| Compose parse | base and base+production `docker compose ... config --no-interpolate --quiet` | Pass. | Syntax only; no containers rebuilt or recreated. |
| Final scoped review | `git diff --check` plus filename-only secret-pattern review of the scoped diff | Pass; matches were environment lookups/placeholders/test values, with no new hardcoded production secret observed. | The pre-existing argv/healthcheck exposure in MP-019 remains operator-gated. |

Coverage artifacts were removed after measurement. Overall module/repository
coverage remains distinct from focused critical-package figures.

## Evidence matrix

Commands below are safe/reproducible forms. Authentication values and tenant
identifiers are represented by placeholders and were never written into this
report.

| Claim | Source/live command or evidence | Expected secure result | Actual result | Timestamp | Real/sandbox/mock | Limitations |
|---|---|---|---|---|---|---|
| Runtime and source differ | `git status --short`; `git rev-parse HEAD`; `docker ps --format '{{.Names}}|{{.Image}}|{{.Ports}}'` | Deployable source matches identified image revision. | Large dirty WIP; images expose unverified revision/build provenance. | 2026-07-13 audit window | Real source/live | No mutation or image extraction performed. |
| Gateway gRPC available | TCP probe to `127.0.0.1:9090`; container port inspection | Listener accepts and requires auth. | Connection refused; HTTP health remains 200. | 2026-07-13 audit window | Real live | Does not exercise every historical gateway RPC. |
| Inference gRPC available | TCP probes to `inference-core:9092` from gateway/execution/retrieval contexts; source `inference-core/src/main.rs` | Listener accepts authenticated RPC and returns bounded inference response. | Live connection refused; source restores authenticated additive listener and standard health. | 2026-07-13 audit window | Real live/source | No paid provider call; ordinary gateway caller is source-migrated, other callers/live route remain unproved. |
| Execution gRPC auth | Authenticated/unauthenticated internal gRPC probes to `execution-core:9093`; 201-test source suite | Missing/invalid denied; valid scoped caller accepted; terminal approval replay cannot resume. | Live listener/auth exists; source separates exact execution ingress from data/session/inference credentials and CAS-resumes only gated runs. | 2026-07-13 audit window | Real live/source | Full live run unavailable because inference is down; 1 Quarry E2E ignored. |
| Source invoke credential graph | Model-gateway `e2e_invoke_chain_test`; Auth Core audience tests; Frontend gateway tests | Exact audience token for each service; no target token reuse; required issuance fails closed. | 21/21 invoke-chain, 40 Auth Core token/role, and 3 Frontend focused tests pass. | 2026-07-13 | Source test | Not deployed; no independently verified ZDR-eligible provider. |
| Gateway JWKS bootstrap | Redirect/oversize regression test and full 407-test package suite | Redirect/non-2xx/oversize denied; key material warmed before listeners. | Pass; no raw ingress token retained for downstream use. | 2026-07-13 | Source test | Rotation/refresh lifecycle beyond the bounded cache still needs operational proof. |
| Health reflects dependency outage | `curl -fsS http://127.0.0.1:8080/healthz`; `curl -fsS http://127.0.0.1:18082/healthz`; TCP probes above | Readiness non-200 when required gRPC/downstream is absent. | Health/readiness stays 200 despite absent listeners. | 2026-07-13 audit window | Real live | Liveness itself may correctly remain green; readiness is the defect. |
| Cost requires auth | `curl -i 'http://127.0.0.1:8089/api/v1/cost/entries?...'` without `Authorization` | 401. | 200 with real ledger rows. | 2026-07-13 audit window | Real live | Sensitive values omitted. |
| Cost cross-tenant isolation | Read-only aggregate/entry queries plus Postgres counts grouped by org/user | Caller sees only signed tenant/user. | 43 entries across 4 orgs and 5 users available on unauthenticated API. | 2026-07-13 audit window | Real live | Seed/local data; exploit class is production-relevant. |
| Cost source fix | Full/focused Go test, race, vet, and coverage commands above | Negative identity/numeric/attribution matrix denied; valid scoped access and tenant-scoped idempotency work. | Pass; server 86.4%, ledger 91.0%; only model-gateway publishes usage in current source. | 2026-07-13 | Source test | Not deployed/live-probed; shared broker token is not cryptographic publisher identity; Postgres integration test not run. |
| Compaction operational | `curl -fsS http://127.0.0.1:18081/metrics`; read-only checkpoint SQL | Success increments, no persistent failure, unique checkpoints. | 479 successful runs, no errors observed; 24 checkpoints since start, no duplicate run/ordinal. | 2026-07-13 audit window | Real live | Retry/poison/concurrency/ZDR not deliberately induced. |
| Pending approvals tenant-scoped | Unauthenticated `ListPendingApprovals` gRPC with empty org | Unauthenticated denied; empty org invalid. | Returns pending approvals across tenants. | 2026-07-13 audit window | Real live | Approval contents omitted. |
| Approval decision authorized | Unauthenticated `DecideApproval` against safe seeded approval; current source/tests | Denied without verified actor/org/user; same-org foreign user denied; exact retry emits no duplicate events/resume and unknown execution delivery returns non-success. | Live old runtime accepts empty org/caller actor; source pins durable/cache user, validates and persists create before cache/publish, bounds cache, persists decision before memory, and requires affirmative execution resume. | 2026-07-13 audit window | Real live/source | CAS/event and decision/resume outboxes, migration, and live deploy remain open. |
| Agentic HITL gate is real | Source trace through execution runtime `ask` posture and durable approval; prior live Plan evidence | Risky tool pauses before dispatch. | Guarded path pauses/persists approval. | Revalidated 2026-07-13 | Real source; prior live | Live session approval boundary remains open; inline MCP bypass is source-fixed only. |
| Capability boundary authenticated | No-auth HTTP/gRPC probes on `:8085`/`:9097`; handler/repository review | 401/Unauthenticated; tenant from token. | Live requests accepted; source authenticates and adds org-scoped availability version-CAS/audit. | 2026-07-13 audit window | Real live/source | Availability is not yet authoritative in Execution Core. |
| Capability availability contract | Full/race/vet tests, focused coverage, migration regression/fixture | Fresh version-bound state/reason; missing/unsupported scope, invalid risk, stale/disabled/unhealthy/quarantined deny. | Source derives fail-closed state, atomically attests/audits, tenant-binds durable grants, and accepts only `global` or verified-tenant `org`; unsupported caller-supplied resource/agent scopes fail before lookup. | 2026-07-13 | Source test | No release-Postgres migration, health reporter, execution consumption, or live proof. |
| MCP malformed registration rejected | Live registry read; gateway + capability registry negative suites | Stdio+HTTPS/empty allowlist rejected at write. | Live old record exists; both source boundaries reject/quarantine it and hide raw config. | 2026-07-13 | Real live + source test | Source containment not deployed; no real Visma server. |
| MCP no arbitrary spawn | Legacy source trace; patched source tests | No caller-selected command/args executed. | Live/legacy path can spawn; patched source disables stdio. | 2026-07-13 | Real source/runtime configuration | No exploit command executed. |
| HTTP MCP SSRF defended | Patched registration/dispatch tests and source review | Private/loopback/rebinding/redirect destinations denied. | Source implements deny/re-resolution; live negative suite not run. | 2026-07-13 | Source test | DNS rebinding needs controlled integration infrastructure before deploy. |
| Visma operational | Registry/compose/container/port inspection; discovery state | Real server healthy, tools discovered, exact allowlist, authenticated call. | Malformed record, zero tools, no MCP bridge deployment. | 2026-07-13 audit window | Real live | Operator-side connector excluded as separate system. |
| Letta semantic search | Health plus safe `SearchMemory` gRPC probe; source auth/degraded tests | Real relevant results or explicit degraded error/status. | Live health green while backend fails; source uses exact caller auth and structured degraded state, rejecting missing/ZDR retention. | 2026-07-13 audit window | Real live/source | No production corpus relevance claim or live deployment. |
| Same-org memory isolation | Source query/migration tests | User A cannot read/overwrite User B memory. | Source filters by verified thread owner and uses owner in NULL-session uniqueness; migration `0011` is unapplied. | 2026-07-13 | Source test | No release Postgres/data migration proof; legacy overwritten value unrecoverable. |
| Capability HTTP private-memory isolation | Full/race/vet and actor/selector negative tests | Same-org user cannot list/get/resolve/update/delete another user's private memory; ownerless/resource-unknown fails closed. | Pass; changed memory functions 81.8–100%; resource-scoped writes return unavailable pending Session Core ownership authority. | 2026-07-13 | Source test | No live Postgres/deployment proof. |
| Browser BFF ownership | Browser 53 tests, full gateway 235 tests, llvm-cov | Every action/WS/run ID bound to exact user+org. | Pass in source; new ownership helpers 105/105 lines. | 2026-07-13 | Source test | Process-local store; no restart/HA/live proof. |
| Capability semantics consistent | Source comparison of frontend catalogs, gateway offers, capability DB, execution registry | One derivation with identical machine state/reasons. | Catalogs and modes diverge. | 2026-07-13 | Real source | Full browser UX E2E waits on inference recovery. |
| ZDR monotonic/end-to-end | Auth/request source review; focused gateway/inference tests | Issuer requirement cannot be downgraded; unverified providers receive no request; no durable/external retention. | Gateway/inference monotonic handling and provider eligibility are source-fixed. Every current interactive token requires ZDR and no deployment is confirmed eligible, so source invoke fails before provider I/O. Persistence/modality/tool/trace/bridge gaps remain. | 2026-07-13 | Real source/test | Correct fail-closed security posture currently blocks source chat availability; no compliant live end-to-end proof. |
| Process-argument secret exposure | Read-only process/config inspection | Secret values never appear in argv or diagnostic output. | A diagnostic command surfaced unrelated third-party connector credentials already present in process argv. Values are omitted and were not reused. | 2026-07-13 | Real local process state | Operator must move credentials out of argv, rotate the affected connector credentials, revoke old values, and verify clean process listings. |

## Finding register

Deployment states: **live-open** means the running stack remains vulnerable or
broken; **source-fixed** means focused tests pass but no safe deployment has
occurred; **open** means no complete patch/evidence exists.

| ID | Severity | Finding / exploitability | Owner | Required fix | Regression evidence | Deployment | Verification |
|---|---|---|---|---|---|---|---|
| MP-001 | P0 / Critical | Live gateway/inference gRPC listeners are absent; every live chat/inference request and query embedding fails. | Model runtime | Safely deploy source-restored additive authenticated contracts with honest readiness. | 407 gateway-package and 139 inference tests pass, including 21 invoke-chain and 3 gRPC compatibility tests; live old/new/query-embedding matrix remains. | source-fixed; live-open | Connection refusal reproduced; source contracts/caller graph pass. |
| MP-002 | P0 / Critical | No immutable rollback artifact or verified image provenance; green health masks missing listeners. | Operations + runtime | Pin/sign candidate and rollback digests; dependency readiness and staged rollback. | Rollback rehearsal and readiness-negative tests. | open | Image/config inspection. |
| MP-003 | Critical | Unauthenticated cost ledger exposes cross-tenant data and unsafe operations. Network access is sufficient. | cost-core | Verified identity, tenant/user pinning, service scopes, generic errors, bounded accounting and scoped idempotency. | No/malformed/wrong tenant/wrong user/wrong scope/valid plus invalid numeric/attribution matrix; server 86.4%, ledger 91.0%. | source-fixed; live-open | 43 rows/4 orgs/5 users observed live; Postgres integration not run. |
| MP-004 | Critical | Unauthenticated live session approval listing/decision can grant another tenant's pending action; source review also found same-org cross-user decisions. | session-core | Deploy verified actor/org/user list/get/CAS/idempotency plus migration after caller compatibility and add audit. | Cross-tenant/user, forged actor, exact retry/conflict, concurrent decision tests. | source-fixed; live-open | Live-safe probe; source suite passes with 5 DB-gated ignored. |
| MP-005 | Critical | MCP stdio registration permits caller-selected process/arguments with service environment, enabling RCE/secret theft. | model-gateway | Keep stdio disabled or fixed executable/argument registry with sandboxed identity. | Command/arg/scheme/raw-secret denial tests. | source-fixed; live-open | Legacy source trace; 7 secure-registration tests pass. |
| MP-006 | High | HTTP MCP accepts SSRF-relevant destinations/redirect/DNS behavior on live path. | model-gateway | Deploy HTTPS-only redirect/proxy denial, DNS re-resolution, forbidden CIDRs, and bounds only after controlled integration tests. | Private/link-local/metadata/redirect/rebinding/size/timeout suite. | source-fixed; live-open | Focused source tests; controlled rebinding/live suite pending. |
| MP-007 | High | Capability-core HTTP/gRPC and by-ID access lack validated tenant authority in the live stack. | capability-core | Shared verifier/interceptors, org-scoped repositories/policy, and admin-only mutation authority. | Full identity/tenant/by-ID/policy negative matrix. | source-fixed; live-open | No-auth live probes; authz 91.7%; policy 95.4%; role/token tests pass. |
| MP-008 | High | Live session/run APIs beyond approvals lack inbound auth; source caller/storage coverage is not fully live-proven. | session-core | Deploy all-business-RPC auth and claim pinning after every caller migrates; complete DB-backed by-ID/list/cancel/replay matrix. | Wrong-tenant IDs, list, cancel, replay tests. | auth source-fixed; live/DB matrix open | Source suite passes; live listener remains open and 5 DB tests are ignored. |
| MP-009 | High | ZDR was downgradeable and remains unproved across persistence/modalities/tools/traces/bridges. | gateway/inference/session/memory/data integrations | Monotonic effective ZDR, provider eligibility, no-persist branches, propagated contracts. | Unary/SSE/embed plus session/compaction/memory/cache/trace/tool/bridge E2E. | gateway + inference routing source-fixed; live/end-to-end open | 3 focused inference ZDR/cache tests pass. |
| MP-010 | High | Letta semantic search is unavailable while health is green; silent relevance degradation risk. | letta-bridge | Correct embedding auth/config or structured degraded readiness/API state. | Success, timeout, auth failure, degraded UX/metric tests. | degraded contract source-fixed; live-open | Safe SearchMemory probe; server 84.5%. |
| MP-011 | High | Direct inline MCP dispatch can bypass governed agentic HITL. | model-gateway + execution | Do not advertise/dispatch MCP in direct tools-only loop; route through durable approval. | MCP direct denial and approval/resume/bypass tests. | source-fixed; live-open | Inline advertisement/dispatch removed; no live deploy. |
| MP-012 | High | Tool/capability catalogs diverge across UI, chat modes, gateway, registry, and execution. | Model capability owner + Frontend | Make the additive status/reason/freshness/version contract authoritative in policy, offers, and execution. | Cross-mode contract and browser E2E matrix. | source contract partial; execution open | Derivation/attestation tests pass; Execution Core remains hardcoded. |
| MP-013 | High | Live inference router policy endpoints accept unauthenticated reads/writes. | inference-core | Exact-audience auth/admin scope, tenant/plane ownership, audit and CAS/versioning. | No-auth/forged/wrong-scope/valid admin tests. | auth/scope source-fixed; live/CAS/audit open | Exact-scope source test; no live deployment. |
| MP-014 | High | Shared NATS token/no TLS means a compromised peer can forge the allowlisted producer despite strict tenant/event validation. | Platform + event owners | Subject/envelope/payload and `producer=model-gateway` validation are source-fixed; add per-producer workload identity/ACL, TLS, durable scoped consumers. | Forged subject/payload/producer, replay, restart/dedup tests. | validation source-fixed; workload identity open | Source inventory found only model-gateway publishers, but the shared token is not cryptographic identity. |
| MP-015 | High | Bridge/browser/sandbox sensitive live surfaces and callers are incomplete or advertise behavior they do not provide. | Respective services | Deploy source auth/tenant scope only after caller credentials, real executor wiring, and honest state pass E2E. | Boundary auth + caller + degraded-state E2E. | browser source-fixed; other callers/live open | Browser 53/full gateway 235 tests; process-local owner store. |
| MP-016 | Medium / corrected claim | Compaction 100%-failure claim does not reproduce; source failure-mode containment is improved. | session-core | Preserve live behavior while bounding candidate aggregation and serializing ordinal allocation. | Retry/poison/ZDR/conflict tests pass; DB concurrency/uniqueness suite. | live healthy; source partial | 479 live successes; 5 DB-gated session tests ignored. |
| MP-017 | High | Provider fallback, retries/circuit breaking, and end-to-end usage reconciliation are not proven together; budget historically failed open. | inference/gateway/cost | Authenticated fail-closed budget, provider-aware retry/circuit policy, and ledger reconciliation. | Provider sandbox/fault injection plus duplicate/retry accounting E2E. | budget + ledger validation/idempotency source-fixed; provider E2E open | Source tests only; no surprise paid load. |
| MP-018 | Medium | Malformed Visma record implies a capability that does not exist. | Product + MCP owner | Reject/migrate invalid record and expose `not_configured` onboarding state. | Write-time validation, discovery health, UI state test. | source-fixed in validator; live record open | Registry/container inspection. |
| MP-019 | High | Secrets appear in service argv/healthchecks and shared credentials have broad blast radius. | Operations + service owners | Remove argv exposure, move to managed refs/files, rotate after migration, reduce scope. | Process/config leak scans and rotation/revocation drill. | open; operator-gated | Values not printed. |
| MP-020 | P0 / Critical | Auth Core requires ZDR for every interactive Model token, while no configured provider deployment is independently confirmed ZDR-eligible. The source therefore rejects all interactive provider routes. | Identity policy + provider owner | Preserve monotonic ZDR; independently verify/configure an eligible deployment or deliberately revise issuer policy through product/security governance. | Issuer-required ZDR plus provider eligibility/no-network and positive eligible-route tests. | source fails closed; release-blocking | No confirmation flag was enabled and no paid call was made. |
| MP-021 | Critical | Approval/browser paths previously lacked exact credentials; approval create/cache ordering allowed pre-validation poisoning, while decision + resume remain split. | gateway + execution + session + browser | Exact credentials/ack, persist-before-publish create, user-owned bounded cache, replay suppression, explicit non-success for unknown delivery, and terminal-run CAS are source-fixed; add transactional outbox/reconciler and live E2E. | Grant/deny/timeout/repeat/wrong-user/cache-degraded/terminal replay/browser resume/recovery E2E. | source partial; live-open | 407 gateway, 122 session, and 201 execution tests pass; no crash-recovery/live proof. |
| MP-022 | High | Same-org users could read and overwrite each other's NULL-session user memory. | session-core | Filter/search/index by verified thread owner and include owner in uniqueness. | Wrong-user read/write plus migration concurrency tests. | source-fixed; migration/live open | Source query/migration tests pass; no release Postgres. |
| MP-023 | High | Capability availability was advisory; policy scope omission, caller-supplied agent/resource identity, unknown risk, and tenant-ambiguous grants could allow an unsafe decision. Model offers/execution remain divergent. | capability-core + execution-core | Source policy now accepts only `global` and verified-tenant `org`, denies unsupported/invalid scope/risk/state, and tenant-binds grants; add trusted concrete IDs before re-enabling resource scopes and enforce the contract at offer/dispatch boundaries. | Missing/unknown/agent/resource scope, risk, cross-tenant grant, disabled/stale/unhealthy/approval-required cross-mode E2E. | policy source-fixed; execution open | Policy 95.4%; migration fixture is gated/unrun on PostgreSQL; hardcoded execution catalog remains. |
| MP-024 | High | Compaction aggregates full history before limiting candidates and races manual checkpoint ordinal allocation. | session-core | Pre-limit candidates and use one database serialization/allocation strategy for every writer. | Large-history timeout and manual-vs-auto Postgres concurrency tests. | open | Static query/migration review; live small-scale loop healthy. |
| MP-025 | High | Legacy messages/events lack explicit retention provenance; Dreaming scans them and replay labels them non-ZDR. | session-core + migration owner | Quarantine unknown legacy rows or migrate explicit signed retention state before dreaming/replay. | Legacy unknown/ZDR dreaming, replay, compaction, Letta tests. | open | Source review; no content exported. |
| MP-026 | High | Capability MCP CRUD accepted malformed/secret-bearing records and exposed raw config. | capability-core | Bounded strict schema, HTTPS/public DNS, exact allowlist, managed refs, redacted/quarantined reads. | 21 negative inputs, oversize, DNS/private and valid managed-ref cases. | source-fixed; live-open | Full/race/vet pass; focused 84.49%; execution re-resolution still required. |
| MP-027 | High | Capability HTTP memory allowed same-org users to read/resolve/mutate another user's private rows; ownerless legacy rows were ambiguous. | capability-core + session authority | Pin private rows/mutations to verified actor; fail closed for ownerless/unknown scopes; quarantine resource writes without ownership validation. | Same-org foreign user, direct ID, resolve selector, atomic patch/delete, ownerless/unsupported scope tests. | source-fixed; live-open | Changed functions 81.8–100%; no Postgres/deployment proof. |
| MP-028 | High | Gateway pending-approval cache was organization-only and mutated/published before session-core run ownership validation; same-org disclosure and cache/event poisoning were possible. | model-gateway + session-core | Persist/validate first; bind cache to user, bound/evict it, and filter degraded reads/decisions by verified user. | Same-org list/decision, durable-unavailable cache, pre-commit side-effect, bound/eviction tests. | source-fixed; live-open | Focused tests and full 407-test package suite pass; outbox/live proof remain. |
| MP-029 | Medium | Capability/grant mutations persist first and ignore a later audit append failure, allowing a successful security-sensitive change without its required audit event. | capability-core | Commit mutation and audit in one database transaction or durable outbox. | Fault-inject audit failure and prove no unaudited success is returned. | open in source/live | Availability attestation is atomic; general capability/rollout/scope handlers are not. |
| MP-030 | Medium | Gateway approval decisions still depend on a bounded process cache; eviction can make a durable pending approval undecidable, and crashes between the Session CAS/event broadcast or durable decision/execution resume are unreconciled. | model-gateway + session-core + execution-core | Add user-scoped durable read-through plus transactional resume/event outbox and reconciler; cache remains optimization only. | Pending-eviction decision, both crash windows, replay, terminal-run, and reconciliation tests. | partial source; live-open | User isolation/bounds/persist-first, duplicate-event suppression, and unknown-delivery non-success pass; durable read-through/outbox remain. |

## MVP acceptance criteria status

| # | Criterion | Status on 2026-07-13 | Evidence still required |
|---|---|---|---|
| 1 | Backward-compatible chat/inference/tool protocol; rebuild cannot delete gRPC silently. | **Source pass, live fail.** Additive listeners and ordinary invoke caller graph pass 407 gateway-package and 139 inference tests, including 21 invoke-chain and 3 gRPC compatibility tests; live listeners remain absent. | Dependency readiness, Data Plane/approval/browser matrix, immutable rollback, staged live E2E. |
| 2 | Every sensitive HTTP/gRPC/NATS entry authenticates and authorizes validated identity. | **Source partial, live fail.** Core HTTP/gRPC boundaries are substantially source-hardened; running session/capability/cost/router/MCP boundaries remain open and NATS lacks cryptographic producer identity. | Residual caller/boundary inventory and live negative/positive matrix. |
| 3 | cost-core denies unauth/cross-tenant and permits valid scoped reads. | **Source pass, live fail.** | Safe deploy and live 401/403/200 evidence plus caller compatibility. |
| 4 | Compaction success, retry/idempotency/metrics/ZDR proven. | **Source partial, live partial.** Live success and focused retry/poison/ZDR/conflict/metric tests pass. | Pre-limit/ordinal fixes plus ignored Postgres concurrency/idempotency and live-safe evidence. |
| 5 | Letta works or exposes intentional observable degraded state. | **Source pass, live fail.** Structured degraded/auth/retention behavior exists; search/live health remain wrong. | Safe deploy, readiness/API/UI live evidence or real semantic repair. |
| 6 | MCP rejects malformed/RCE/SSRF/auth cases; Visma honest. | **Source substantial, live fail.** Gateway and registry negative suites reject malformed/RCE/SSRF/raw-secret/allowlist cases. | Migration, execution DNS pinning, auth/HITL live suite, and `not_configured` state. |
| 7 | Capability semantics are non-divergent; Frontend intentional tool UX. | **Fail/source partial.** Versioned availability derivation and fail-closed tenant policy exist; untrusted resource/agent scopes are quarantined, but trusted concrete IDs, offers/execution, and catalogs remain divergent. | Authoritative consumption, complete scope subject contract, scoped health reporter, cross-plane UX/E2E. |
| 8 | HITL cannot be bypassed. | **Source partial, live fail.** Guarded path, user-scoped durable/cache authority, persist-before-publish create, exact retry/credentials/ack and inline MCP denial pass; live RPCs and crash recovery remain open. | Outbox/reconciler plus live CAS/direct-dispatch/approval/browser E2E. |
| 9 | ZDR honored end to end. | **Fail/partial.** Gateway/inference/session/Letta fail closed, but all-ZDR issuance makes the product unavailable and legacy dreaming/replay provenance is unsafe. | Policy decision, eligible provider, legacy migration/quarantine, and no-retention proof everywhere. |
| 10 | Full suites, coverage, live positive/negative probes, honest provider classification. | **Fail/partial.** Source suites and changed Capability critical functions exceed 80%, but session overall is 35.30%, Capability overall is 55.4% (registry 47.8%), and DB/provider/deployment/live matrices remain. | Complete release evidence; close or justify each changed critical coverage gap. |
| 11 | Safe deployment/rollback runbook. | **Documented decision, not executable yet.** | Immutable artifacts, exact image IDs/config/migrations, rehearsal and operator sign-off. |
| 12 | Docs match 2026-07-13 evidence. | **Pass for this audit snapshot.** Status, roadmap, dated audit, decision record, and affected service/cross-plane correction banners distinguish live from source state. | Update again after any deployment or provider-policy decision. |

## Safe deployment summary

The binding decision is
`grpc-safe-rebuild-decision-2026-07-13.md`. In brief:

1. no rebuild/recreate of the critical trio from the current tree;
2. inventory and pin current/candidate/rollback artifacts and migrations;
3. restore authenticated services side by side on shadow endpoints;
4. migrate and test every caller before cutover;
5. use dependency-aware readiness, contract, auth-negative, chat, embedding,
   tool/HITL, and cost/ZDR gates;
6. shift traffic one boundary at a time with observation holds;
7. rollback on listener/readiness/auth isolation/error/latency/cost/audit/ZDR
   triggers, using already-pulled immutable artifacts—not an emergency rebuild.

## Enterprise-next

Enterprise work is intentionally deferred until the MVP table is fully green.
The separate backlog in `MODEL_PLANE_ROADMAP.md` covers workload identity/mTLS,
fine-grained ABAC, HA/regional failover, DR/restore drills, SLO/error budgets,
enterprise audit retention/export, policy-as-code, key/secrets lifecycle,
SBOM/provenance/signing, capacity/cost controls, data residency, and compliance
evidence. This audit makes **no enterprise-readiness claim**.
