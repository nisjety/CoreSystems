# Data Plane v2 Plane Audit — Secure-MVP Re-verification

Baseline live audit: 2026-07-10

Remediation re-verification: 2026-07-11

Velion/GraphRAG re-verification: 2026-07-15

Compose/rebuild re-verification: 2026-07-16

Scope: Data Plane v2 services/infrastructure plus the Control, Frontend, Model, and Ingestion contracts that authorize, invoke, or persist Data Plane work.

## 2026-07-16 Compose and embedding-auth addendum

The production Data Compose posture requires an explicit shared GDPR broker
URL; the standalone overlay can leave it empty without silently falling back
to an unprovisioned Control hostname. The
embedding-engine Model Plane provider now requires a dedicated Auth Core service
principal, validates the bounded token response, and forwards only an
org-bound `aud=inference-core` / `inference:invoke` bearer. Its missing workspace
dependency and test-only runtime defects were corrected first.

The fresh disposable rebuild (`COMPOSE_ANSI=never bash
tests/e2e/run-isolated-mvp.sh`) built every image and reached healthy state. It
passed **31 methods / 124 gRPC shapes**, **28/28 HTTP assertions**, and the
six-store restrictive-ZDR final-state comparison. Static Compose, provenance,
broker/GDPR, gRPC, browser, real-authority, and multi-store contracts also
passed. The random project and all resources it created were removed.

This is not a shared deployment claim: the isolated provider uses deterministic
embeddings, so a provisioned sandbox call through the real Model Plane
`inference-core` remains required. The local `.env` carries only the known
Control broker URL/user and sandbox event-key paths; connected startup continues
to fail closed until scoped GDPR broker credentials, deployment event keys, and
the dedicated embedding service credential are provisioned. No shared/customer
resource was changed.

## 2026-07-16 independent-startup and secret-ownership addendum

Data Plane's default Compose network is now private and local, so its services
can bind against their own Postgres/Qdrant/Dragonfly/NATS/Quickwit dependencies
without a running Control, User, or Model Plane. `make standalone-up` uses an
explicit non-production overlay that pauses only cross-plane event consumers
and the Control-owned GDPR durable subscriber. Authenticated traffic remains
strict and fails closed while external authorities are unavailable. The graph
index uses a mounted Auth Core public key instead of fetching JWKS at startup.
`make cross-plane-up` applies the explicit external-network overlay after the
deployment-owned shared network is provisioned.

The Data Plane `.env` records only the non-secret Control broker URL/user and
local sandbox event-key paths; it was not populated with guessed deployment
credentials. The GDPR broker URL/password must match Control's scoped NATS ACL and durable;
embedding/graph/retrieval service credentials must be registered in Auth Core
with their audience and scopes; and event key pairs must come from deployment
secret management. The connected preflight rejects local `.secrets/event-keys/`
paths. Existing local event keys are sandbox material only. This
boundary is a release blocker for shared deployment, not a reason to weaken
standalone authorization or ZDR behavior.

Evidence: Go tests for all four services, full Rust workspace tests, Compose
security contract, both Compose overlay config checks, formatting, and diff
checks passed on 2026-07-16. No production data, shared network, broker, or
secret was mutated.

The rebuilt standalone Compose project was also started locally. All 15
long-running Data services reached healthy state; graph-index and
embedding-engine stayed healthy while their external inference principals were
absent because they now defer that credential check to the first inference
request. Documents `/readyz` was healthy with the GDPR subscriber explicitly
paused. The project uses the private `dpv2-cross-plane` network and was not
attached to `inter-plane-bus`.

## 2026-07-15 final isolated acceptance addendum

This addendum supersedes earlier same-day statements that Docker was unavailable
or that the final signed-ZDR, gRPC, browser, broker, or multi-store reruns were
pending. It does **not** supersede the conclusion that the shared deployment is
unchanged and not production-ready.

- A unique disposable build passed **31 methods / 124 gRPC authorization
  assertions**. No credential and header-only identity are `Unauthenticated`;
  authenticated cross-tenant requests are exact service-specific tenant
  mismatches. Same-tenant reads require `OK` or a named authenticated `NotFound`.
  Deprecated document mutations require exact permanent containment; wiki
  mutations require exact approval- or signed-ZDR guards.
  The script refuses every non-`127.0.0.1:<port>` target, with a negative test.
- The same build passed **28/28 HTTP assertions** across graph, quality,
  orchestrator, documents, retrieval, wiki, and Quickwit admin preview:
  401/401/200/403 for no credential, forged header, valid bearer, and valid
  bearer plus spoofed organization.
- A supported-contract fixture created two real users and organizations through
  Auth Core/User Core/Control. Velion Playwright passed **2/2**: authorized
  Knowledge, GraphRAG, and navbar retrieval are usable, while spoofed tenant and
  search calls contain none of the second fixture's serialized fields.
- The restrictive signed-ZDR run left stabilized final state identical across
  PostgreSQL, Qdrant, Dragonfly, Quickwit, MinIO, and NATS. NATS sequence and
  Dragonfly command counters are monotonic no-write evidence. Counts/hashes for
  the other four stores are final-state evidence and cannot alone exclude a
  transient write followed by delete; per-operation mutation telemetry remains
  required before claiming strict zero writes.
- Supported producer-scoped event delivery/redelivery passes the isolated broker
  matrix. The real Documents binary also passes scoped GDPR durable bind/health,
  ACL denial, ownership-transfer-before-ACK, database-outage redelivery/degraded
  health, and recovery ACK. Production provisioning remains pending and is
  recorded separately from this disposable proof.
- Retrieval's Control token budget is longer than Auth Core's durable audit
  PubAck budget. Only connect/timeout/503 failures retry once; 401/403, malformed
  contracts, and explicit denial never retry. Dependency exhaustion is bounded,
  uncached, and exposed as gRPC `Unavailable`; verified non-membership remains
  `PermissionDenied`.
- Gateway tests pass **288/288**. The SPA Node security gate passes **4/4** and
  Vitest passes **68 files / 364 tests**, with typecheck and production build.
  Retrieval security-module line coverage is 93.76% for policy and 88.14% for
  the gRPC interceptor; its final all-target suite passes **212 tests with 10
  explicit infrastructure ignores**.

Safety: response bodies, credentials, generated identifiers, fixture text, and
graph content were suppressed. No shared/customer content was enumerated and no
rebuild, cleanup, purge, bulk delete, global admin, or destructive endpoint was
invoked. Each final harness removed only its own random project resources.

## 2026-07-15 Velion/GraphRAG addendum

The Velion gateway no longer uses a shared internal key as interactive Data
authority. Every documents, retrieval, wiki, source, graph, navbar-search,
ingestion-source, onboarding graph-preview, and Operating Map call mints an
`aud=data-plane` bearer from the verified session, pins tenant from canonical
membership, and fails closed with a sanitized 503 when delegation is
unavailable. The SPA/gateway now normalize the actual documents/wiki/retrieval
envelopes, use bounded document pagination with honest truncation metadata, and
call the implemented search/chunk contracts.

Retrieval HTTP requires strict RS256 bearer verification with configured
issuer/audience and all identity/time claims. Retrieval and graph no longer send
a shared key to Inference Core: they mint short-lived, audited, tenant-bound
service tokens for exact `aud=inference-core` / `inference:invoke` and forward
only Bearer metadata. Auth Core's optional `scopesByAudience` registry map is
fully validated and prevents cross-audience scope reuse while preserving legacy
single-audience entries. Inference Core independently verifies JWKS,
issuer/audience, tenant, exact scope, and monotonic ZDR.

Graph event consumption now requires exact verified-claim/payload tenant
equality; verified tenant drives extraction/persistence. Production cannot
enable unsigned legacy graph events. Direct-Azure graph extraction rejects ZDR
before egress, and Graph API storage errors return sanitized 500 rather than a
successful error payload.

Sanitized source evidence dated 2026-07-15:

| Area | Result |
|---|---|
| Graph | 28 passed, 1 explicit disposable-PostgreSQL ignore; focused extractor 9/9; strict clippy passed |
| Retrieval | 193 non-ignored passed, 10 explicit infrastructure ignores; strict all-target clippy passed |
| Auth Core registry | 12/12 focused tests and build passed; 90.19% statements / 93.84% branches / 100% functions / 89.79% lines |
| Velion Gateway | 272/272 tests passed |
| Velion SPA | 68 files / 360 tests, typecheck, and production build passed; lint has zero errors and one pre-existing warning |
| Documents signed ZDR | RED/GREEN required-claim, monotonic single/bulk, and source-object pre-persistence tests; full race/vet/build passed; auth 95.5%, both changed ZDR decision helpers 100% (broad handler package 14.9%) |

The passing checkpoint used the durable command
`./tests/e2e/run-isolated-mvp.sh`, a fresh random project, and a rebuild of every
Data image at that checkpoint. Caller-selected project suffixes and build bypass are
not supported; pre-existing resource collisions are refused before project
cleanup is armed, generated keys have an immediate cleanup trap, and the
isolated override publishes no database/vector host ports. The matrix exposed
two real product defects before passing: the retrieval request DTO incorrectly
required an otherwise optional `filters` object, and the one-shot migrator did
not tolerate the startup race between PostgreSQL health and the first TCP
connection. Regression tests were written first; retrieval now defaults omitted
filters to empty, and the migrator uses bounded retry. Retrieval fmt/full tests/
strict clippy and migrator Go test/vet pass. That checkpoint then exposed that
Documents ignored the bearer's signed `zdr:true` posture while honoring only the
body. The latest source requires/preserves a boolean claim and enforces
`verified_claim.zdr OR request_policy.zdr`; Docker became unavailable before the
fixed Documents image/matrix could rebuild. Final security review also found
that source-object upsert persisted its row and outbox while hardcoding
`zdr:false`; an endpoint regression failed before the fix and now proves denial
before persistence under restrictive or missing verified posture. That final
guard is likewise source-tested but not rebuilt.

The shared running images observed during this pass were healthy but carried
older source revisions. No customer content was requested, printed, copied, or
mutated. A disposable isolated Compose project built the pre-final-fix source with
base-revision label `eeebd0bc98c66434936460020958891066eb05fd` and passed the
safe runtime matrix described below; the dirty worktree was included, so the
label is not a reproducible release identity. It did not replace or validate the
shared deployment.

## Executive conclusion

The baseline audit proved critical anonymous/header-only tenant access, unsafe shared-key and unsigned-token behavior, an unauthenticated global Quickwit rebuild surface, and durable writes under restrictive ZDR. Those are treated as verified release blockers, not hypotheses.

The current dirty worktree closes the direct HTTP/gRPC identity defects in source
and tests, adds producer-scoped signed envelopes and acknowledged transactional
outboxes to supported async paths, implements signed Control read delegation, and
adds durable quality/orchestrator/Quickwit state. This is a substantial security
improvement, but **not production readiness**: unsupported consumers and
unverified Model/Execution gRPC listeners remain contained, enabled Data gRPC
families and broker paths lack the complete matrix, and the shared deployment
still predates this source. The disposable checkpoint stack passed all **28/28**
HTTP tenant/auth checks plus its bounded relational body-ZDR snapshot; that is
isolated effectiveness evidence for the checkpoint, not runtime proof of the
later signed-claim/source-object guards or shared deployment acceptance.

No customer identifiers, corpus sizes, content, credentials, or response bodies are retained in this document. The baseline evidence came from a read-only cross-tenant matrix; remediation used synthetic keys, local mock servers, disposable organizations/fixtures, and disposable PostgreSQL only.

## Baseline findings and current disposition

| Baseline blocker | Disposition in current source | Runtime/deployment status |
|---|---|---|
| Graph read accepted no credential and caller-selected tenant | Strict RS256 user/scoped-service verification, HTTP/gRPC claim pinning, and org-visible provenance filtering; route matrices pass | Isolated HTTP 401/401/200/403 passed; shared deployment and gRPC matrix pending |
| Quality accepted forged `X-Org-ID` | Strict JWKS middleware, claim pinning, and `data:quality:admin` scope; matrices pass | Isolated HTTP 401/401/200/403 passed; shared deployment pending |
| Orchestrator accepted forged `X-Org-ID` | Strict JWKS middleware, claim pinning, and operation scopes; matrices pass | Isolated HTTP 401/401/200/403 passed; shared deployment pending |
| Documents shared-key/no-viewer escalation and unsigned JWT default | Shared inbound key removed; strict signature/issuer/audience/time/startup checks; required boolean ZDR posture; canonical service identity and visibility; unsigned tokens fail | Checkpoint HTTP 401/401/200/403 passed; final signed-ZDR/source-object image, shared deployment, and expanded visibility matrix pending |
| Retrieval denied legitimate users while adjacent services were open | Strict user JWT path plus versioned Control decision contract using an org-bound `aud=control-policy` service bearer. Explicit grants additionally require the original matching verified user bearer at User Core | Isolated cryptographic authority fixture passed 200/403; real Auth/User Core user journey pending |
| Quickwit `/admin/rebuild` unauthenticated/global destructive | Dedicated admin JWT/scopes; durable tenant-default jobs, idempotency, two-person approval, leases/checkpoints, immutable request fields, append-only audit, preflight, and rate/concurrency bounds; destructive clear remains 501 | Isolated authenticated preview 401/401/200/403 passed; destructive execution was not invoked |
| Bulk ingest persisted under restrictive ZDR | Documents single/bulk share monotonic signed-or-body durable-persistence rejection; source-object upsert fails before row/outbox persistence under restrictive posture; Quarry single/bulk/CAS/events share one guard | Checkpoint body-ZDR denials plus an unchanged ten-second relational snapshot passed. Final signed-claim/source-object guards are source-tested only; rebuilt and multi-store proof pending |
| Retrieval auxiliary endpoints trusted body tenant/user | Claim-pinned org/user and canonical visibility across pack/sources/freshness/chunks/compare/timeline/contradictions/wiki/graph/trace | Tests pass; runtime pending |
| Grant-only shared content leaked org-wide | Shared now requires explicit grant; org-visible remains the only org-wide class | Synthetic Postgres visibility tests pass |
| Trace GET lacked tenant/actor scope | Trace reads/writes bind verified org and actor; additive actor migration added | Migration tested, not deployed |
| Chunk query referenced obsolete column | Query uses `knowledge_units.text` and tenant-bound joins | Tests pass |
| Wiki code/schema drift caused 500s | Forward-compatible wiki migration reconciles page/source/maintenance contracts | Disposable PostgreSQL integration passes |
| Query embedding, semantic response caches, and retaining rerank egress wrote/sent content under ZDR | Exact ZDR enum, cache read/write bypass, retaining embedding guard, and text-rerank suppression under every restrictive posture | Unit tests pass |
| Model grounding/knowledge and standard/direct Data proxy routes omitted or downgraded ZDR | Frontend mints separate audience tokens; Model independently verifies identity equality and forwards Data bearer. Chat/embeddings and document/bulk/retrieval/wiki proxies use monotonic verified-claim OR request ZDR; durable wiki mutations are rejected | Model Gateway all-target tests/clippy pass; rebuilt runtime pending |
| API-key/gRPC paths trusted caller org/user | Data external surfaces use verified claims; Model/Inference/Execution production listener constructors were removed/test-confined and legacy Compose ports removed | Verified replacements remain pending |
| Control membership route/config drift | One versioned decision endpoint and canonical membership lookup; strict caller bearer with dedicated scope/audience/org/reason | Control/retrieval mock suites pass |
| Images lacked reliable revision | OCI labels and mandatory Compose build args cover Data, changed Model, and Control auth/user images | Isolated checkpoint images carry revision/build labels; final Documents source is not rebuilt, and shared images remain older and unchanged |

## Final security-review findings and disposition

The post-remediation read-only review found additional HIGH/CRITICAL boundary
defects; none are treated as optional hardening:

- Data Compose mounted Control's complete signing-key directory into retrieval
  and Quickwit. Both now mount only `convex-auth.pub`; a static contract rejects
  directory mounts.
- static Control service credentials could treat arbitrary `X-User-Id` as self
  and could submit tenant/subject/grant actors to the authz facade. Legacy
  delegated-user access remains denied. The replacement v2 read delegation now
  binds caller, user, tenant, method/URI, operation, resource type/ID, bounded
  reason, restrictive ZDR, nonce, request digest, and short lifetime. It also
  requires the original RS256 `aud=data-plane` user bearer; User Core independently
  verifies issuer/audience/time/user/tenant, consumes the nonce, and rejects
  missing, mismatched, unsigned, or replayed proof. Control and its Rust/Go clients
  share a fixed signature vector. Grant mutation/listing
  remains denied pending resource-owner authorization.
- retrieval's grant cache listened on Data's local broker while Control published
  invalidations on the shared broker. Secure MVP grant reads are now uncached, so
  revocation is effective on the next request without an event-topology dependency.
- index and Quickwit delayed-event paths trusted stale event posture. Index now
  rechecks canonical deletion/ZDR state, and every Quickwit knowledge ingest
  source excludes deleted, restricted, and unknown classifications.
- provider error handling could retain upstream response bodies. Retrieval
  embedding and rerank errors now record status only.
- deprecated retrieval gRPC Create/Delete/Bulk could be re-enabled by one
  environment variable and bypass canonical ZDR/outbox behavior. These methods
  now permanently return `FailedPrecondition` before storage.
- Model `/v1/ai/chat` and `/v1/ai/embeddings` hard-coded downstream `zdr=false`,
  and Auth Core did not issue the claim Model expected. Auth Core now requires
  and signs `zdr:true` for user/service Model tokens; both routes propagate
  `verified_claim.zdr OR request.zdr` with real Axum/JWKS/tonic capture tests.
- Auth Core's canonical Model service tokens initially lacked the user field
  required by Gateway and were unusable. Gateway now distinguishes verified user
  and service principals; services require matching `sub`/`service_id`, bounded
  reason, tenant/audience/time validity, and exact `models:invoke`, and can reach
  only POST chat/embeddings. User/delegated/retrieval/session routes return 403.
- Model service-token issuance carried a signed reason but had no proven audit
  emission. It now publishes a tenant-bound Control audit event with the bounded
  service, audience, scopes, reason, and restrictive ZDR posture; issuance
  returns 503 when the local audit publisher is unavailable. Audit-core durable
  persistence remains runtime-unproven.
- Control pprof and optional self-owned dependencies published unauthenticated
  host ports/default credentials. Diagnostics default off; those host
  publications/default credentials were removed.

## Asynchronous and internal-boundary containment

The shared `event-envelope-rs` contract verifies RS256 producer identity,
audience, scopes, tenant, exact subject, payload digest, issued/expiry times,
event/replay identity, key ID, and ZDR. Documents publishes signed lifecycle
events from an acknowledged transactional outbox. Wiki page/version writes
atomically enqueue signed wiki events for a leased PubAck-based outbox. Index
deletion progression has a signed durable outbox, and supported embedding/index/
graph consumers verify producer-scoped envelopes. Source tests cover wrong
producer/tenant/subject/digest, unsigned, replay, expired, and ZDR cases.
Documents, Wiki, and Index outbox retries also use stable JetStream message IDs;
canonical consumers remain idempotent across broker redelivery.

Unsupported legacy consumers remain disabled by default, including documents
GDPR, graph cleanup, Quickwit indexing, and orchestrator cost ingestion. Their
development-only escape paths require both `ALLOW_UNVERIFIED_LEGACY_EVENTS=1`
and `ALLOW_INSECURE_DEV_DEFAULTS=1`.

Model Gateway gRPC, Execution Core gRPC, and Inference Core gRPC production
binaries no longer construct or bind the unverified listeners; listener modules
are test-only and the legacy Compose host ports were removed.

Containment is not acceptance. NATS subject ACLs, rebuilt broker/database event
flows, verified inference scopes, verified run ownership, and gRPC route matrices
are required before the remaining paths can be accepted.

## ZDR posture

Implemented/tested safeguards:

- documents durable single/bulk ingest reject `zdr_mode=on` or `ephemeral_only=true` before repository/event work;
- Quarry suppresses durable Data handoff, object-store CAS, and durable event publication under restrictive posture;
- retrieval query-embedding and semantic caches bypass reads/writes in ephemeral mode;
- retrieval traces do not persist in ephemeral mode;
- Model unary/stream paths bypass sessions, events, idempotency, publisher backends, and response/semantic caches; standard chat/embedding routes propagate monotonic ZDR;
- Frontend converts authenticated restrictive header posture into the Model JSON body and sends a separate Data bearer.
- supported async envelopes cryptographically bind restrictive ZDR to producer,
  tenant, subject, event identity, and payload digest; unsigned or downgraded
  envelopes fail verification.

Still required: extend the signed posture to every remaining Ingestion/legacy
consumer, establish an authoritative server-side organization policy before any
future non-ZDR Model token, and run a storage-spy/runtime proof of zero writes
across Postgres, Qdrant, Redis/Dragonfly, Quickwit/MinIO, graph, trace, event
payloads, and Model state.

## Destructive/admin posture

Quickwit authorization proves tenant default and dedicated scopes; global intent
requires separate scope, approval, and break-glass metadata. Durable PostgreSQL
jobs provide scoped idempotency, two-person approval, bounded claims/leases,
heartbeats/checkpoints, immutable request fields, append-only audit, rate/
concurrency protection, and read-only preflight. The isolated lifecycle passed.
Destructive clear remains unavailable with HTTP 501.

A production Quickwit clear still requires trustworthy task-completion checks and
rebuilt-runtime crash/retry proof. No destructive route was invoked during this
program.

## Verification ledger

Entries are local source/test evidence dated 2026-07-10–11 unless explicitly
updated by the 2026-07-15 addendum.

| Area | Command/result |
|---|---|
| Data Rust | Full workspace test: **299 passed, 15 explicitly ignored**; strict workspace all-target all-feature clippy passes. Every infrastructure-dependent test is explicit: graph PostgreSQL 1, index PostgreSQL 2, Quickwit PostgreSQL 2, retrieval PostgreSQL/Dragonfly 7, and retrieval Docker E2E 3 |
| Graph | 22 tests pass with one explicit disposable-PostgreSQL ignore, including HTTP/gRPC auth matrix and provenance visibility |
| Signed event contract | `event-envelope-rs` wrong-principal/tenant/subject/digest/replay/expiry/ZDR suite passes; measured 92.66% regions / 94.91% lines / 100% functions |
| Quickwit | 43 tests pass with two explicit PostgreSQL ignores; the separately run earlier disposable-PostgreSQL lifecycle passed 1/1 before Docker failed. Coverage: `auth.rs` 97.06% lines / `api.rs` 86.73% / `jobs.rs` 52.53% |
| Embedding/index | 26 and 26 tests pass; index has two explicit disposable-PostgreSQL ignores; strict workspace clippy passes |
| Documents Go | Full race suite/vet/build pass; `authctx` 95.5% statements, both changed ZDR helpers 100%, and signed `eventauth` 89.8%; transactional outbox signing/PubAck/stable-message-ID regressions pass |
| Quality Go | Full race suite/vet/build and govulncheck pass; auth 82.8%; source-only eval profile 26.7%, recovery 69.2%. Pending/expired-running recovery is tested in memory; expanded PostgreSQL recovery coverage remains pending |
| Orchestrator Go | Full race suite/vet/build and govulncheck pass; auth 91.1%; source-only jobs profile 31.9%. Production mutation acceptance fails 503 before persistence while no signed resumable worker exists |
| Wiki Go | Full race suite/vet/build pass; schema integration checkpoint passes. Signed `eventauth` 88.1%, config 84.2%, focused pure outbox drain/PubAck 100%, full `internal/events` race profile 28.7%; PostgreSQL outbox execution remains pending |
| Retrieval visibility | 76 library tests pass, including non-serializable/non-debuggable original-user proof, strict ZDR enum/no-retaining-egress, status-only provider errors, original-user-proof grant delegation, uncached revocation safety, auxiliary/search/trace visibility, and deprecated-mutation containment |
| Migrations | Wiki integration passes; trace actor migration applies twice and exposes expected column/index in disposable PostgreSQL |
| Frontend | Gateway 206/206 tests pass |
| Model Rust | Full workspace: **893 passed, 4 explicitly ignored**; strict workspace all-target clippy passes, covering Gateway/Execution/Inference containment and ZDR changes |
| Ingestion | Quarry runtime plus edge tests and strict clippy pass; fixable RustSec dependencies were upgraded. The production audit passes with RSA absent from normal/build graphs and only recorded unmaintained dependency warnings |
| Control policy | 41 focused authorization tests pass; after the production dependency refresh, 30 focused policy/token tests and the full build pass; coverage is 88.46% statements / 84% branches / 95% functions / 88.2% lines; pnpm and npm production audits report no known vulnerabilities |
| Control service auth | Arbitrary self/authz static-service regressions went RED at 200 and GREEN at 403; missing/mismatched/unsigned/replayed original-user proof also fails 403. User-core race/build/vet/govulncheck pass; existing service guards remain 100%, new proof/replay functions measure 81.8–100%, broad HTTP package 19.8% |
| Auth Core signed ZDR/audit | User/service Model tokens require/sign `zdr:true`; downgrade tests pass; service issuance fails closed when tenant-bound audit publication fails; audit-core persistence remains runtime-unproven. Full Jest **99/99**, full no-fix lint, build, and frozen install pass. Focused security coverage: 90.00% statements / 90.04% branches / 94.73% functions / 89.77% lines |
| Dependency scans | Documents/wiki/quality/orchestrator/user-core `govulncheck`: zero reachable. Migrator's initial 4 reachable findings were removed by Go 1.26.5/pgx 5.9.2; test/vet/build/scan pass. User Core's initial 13 reachable findings were removed by Go 1.26.5, pgx 5.9.2, gRPC 1.79.3, quic-go 0.59.1, x/net 0.53.0, and compatible locks; full race/vet/build/scan pass with zero reachable. Shipping's initial 7 reachable standard-library findings were removed by Go/Docker builder 1.26.5; full race/vet/build/scan pass with zero reachable, with one required-module advisory reported not called. Auth Core full/production pnpm audits: zero advisories. RustSec: zero unignored Data/Model vulnerabilities; dev-only RSA and Data's unmaintained `rustls-pemfile` warning are recorded. Quarry's production audit passes after dependency remediation, with RSA absent from normal/build graphs and only recorded unmaintained dependency warnings |
| Safe scripts | Matrix/smoke/load contract tests, Bash/Node syntax, static tenant scanner, and `git diff --check` pass |
| Compose | Data/self-owned, Model, and Control configs plus Model/Ingestion production overrides validate with test-only credentials/provenance. Production overrides reset 19 Model and 14 Ingestion host-port publications, disable dev bypasses, and require credentials |
| Image provenance | Isolated Data checkpoint images were built with base-revision label `eeebd0bc98c66434936460020958891066eb05fd` and created label `2026-07-15T00:45:36Z`. The build included dirty-worktree content, so the label is not a reproducible release identity. Short image IDs: documents `7d668137516c`, graph `fc138c8a6b23`, embedding `401d28370432`, index `b642e7485f5a`, Quickwit adapter `a5f547a120db`, quality `f7d4ded34db4`, orchestrator `f00398f1e980`, wiki `e7c75b756544`, retrieval `5a9bd5f96a14`. The Documents image predates the final signed-ZDR/source-object guards. No shared deployment claim is made |
| Runtime matrix | **Pass for checkpoint HTTP/body-ZDR scope**: 28/28 across seven route families; exact shapes were 401 no credential, 401 forged tenant header, 200 authorized tenant bearer, and 403 bearer plus spoofed tenant. An authenticated durable write first succeeded at 201; after snapshot quiescence, single/bulk matched their exact body-ZDR denials and the relational content snapshot stayed unchanged for ten seconds. The signed bearer on that control was incorrectly ignored, which the latest source fixes; no runtime-effectiveness claim is made for that repair or the final source-object guard. gRPC, broker, real Auth/User Core, full visibility, and multi-store ZDR matrices remain pending |

`cargo llvm-cov` and `cargo-audit` are installed and produced the evidence above.
Coverage below 80% remains an acceptance gap for database-backed Quickwit jobs,
quality evals, orchestrator jobs, and the full wiki events package; functional
checks that require Docker/PostgreSQL are not inferred from source-only coverage.

The earlier Docker content-store failure cleared without an authorized restart
or shared-project mutation. The disposable checkpoint build/runtime proof
completed and its cleanup trap removed all project containers, volumes, and the
network. Docker later became unavailable before the final Documents signed-ZDR
and source-object guards could rebuild. Pending latest-source, broker/outbox/
database-coverage, and cross-plane matrices are not inferred from the narrower
passing checkpoint.

## Dependency remediation note

Control auth-core's initial production scans reported critical/high advisories. Direct dependencies and compatible transitive overrides were upgraded; both supported production audits now report zero known vulnerabilities. The supported runtime/Docker package manager is Corepack pnpm. The npm lockfile was also synchronized so npm's production audit agrees.

## Security incident note

Audit commands accidentally surfaced existing local Model and retrieval
Control-policy deployment credentials in tool output. The values are not
repeated or stored in evidence. They must be rotated through normal secret
management before deployment; this program did not mutate secrets.

## Release blockers

1. Supported signed producer-scoped broker flows pass in isolation, but
   production subject ACLs/credentials and the scoped Documents GDPR durable
   consumer still require coordinated provisioning and post-deploy proof.
2. Inference Core verifies tenant-bound JWKS credentials and exact invoke scope.
   Execution/Model gRPC listeners without the equivalent contract must remain
   disabled; re-enabling them would reopen an MVP blocker.
3. Signed Control plus original-user-proof read delegation and the real-user
   Velion journey pass in isolation. Grant mutation/listing remains denied until
   resource-owner authorization is defined.
4. Quickwit durable safe orchestration exists and its isolated PostgreSQL
   lifecycle passed, but destructive clear remains 501 pending trustworthy
   Quickwit completion and rebuilt-runtime crash/retry proof.
5. Six-store final-state ZDR evidence passes, but PostgreSQL/Qdrant/Quickwit/
   MinIO still need per-operation mutation telemetry to exclude transient
   write/delete cycles. An authoritative organization policy is required before
   any future signed non-restrictive token is issued.
6. Current shared images still predate this source. Credential rotation,
   controlled rebuild/deployment, OCI provenance verification, and safe
   synthetic-tenant post-deploy matrices remain required.
7. Database-backed coverage remains below 80% for Quickwit jobs (52.53% lines),
   quality eval (26.7% source-only; recovery 69.2%), orchestrator jobs (31.9%), and the full wiki
   events package (28.7%). Their expanded/outbox PostgreSQL coverage reruns are
   still required.
8. Orchestrator production mutations remain intentionally unavailable (503)
   until a signed resumable worker/callback identity contract replaces the
   disabled unsigned publisher.
9. The surfaced local deployment credentials require rotation.

## Safety record

- No production customer content was printed, copied, exported, reindexed, deleted, or further enumerated during remediation.
- No global rebuild, orphan cleanup, bulk delete, index reset, purge, or destructive shared-stack operation was invoked.
- Disposable PostgreSQL and synthetic/mock identities were used and cleaned up.
- No commit, push, reset, checkout, clean, or historical-document deletion was performed.

See `DATA_PLANE_STATUS.md`, `DATA_PLANE_ROADMAP.md`, and per-service documents in this directory for current source evidence. Do not start the enterprise-readiness phase while any blocker above remains.
