# Data Plane v2 — Secure-MVP Status

Last verified: 2026-07-16

Verdict: **secure-MVP candidate in current source; not yet production-ready**.
The final disposable build passed the real Auth/User/Control browser journey,
the strict 31-method/124-shape gRPC matrix, the 28-shape HTTP matrix, and the
six-store restrictive-ZDR final-state comparison. Anonymous/header-only tenant
identity, spoofed-org access, unsigned JWTs/events, static shared-key user
impersonation, permissive Control failure, and unscoped admin mutation are closed
or fail-closed in current source. Verevon v3 uses session-minted audience-bound
bearers for Knowledge and GraphRAG. Retrieval retries only transient policy-token
failures within a fixed budget and distinguishes verified denial from Control
unavailability.

This is **isolated evidence**, not a deployment claim. Shared images were not
replaced; production credentials/ACLs and the scoped Documents GDPR durable
consumer have not been rolled out; no customer dataset was queried or mutated.
The ZDR runtime proof establishes unchanged final state in Postgres, Qdrant,
Quickwit, MinIO, NATS, and Dragonfly, with monotonic no-write evidence for NATS
and Dragonfly. It does not by itself exclude a transient insert-then-delete in
the other four stores, so strict per-store mutation telemetry remains a release
evidence gap. Enterprise readiness has not started.

## 2026-07-16 Compose/rebuild verification

The production GDPR contract requires an explicit `NATS_SHARED_URL` for the
enabled durable consumer; the Compose standalone overlay can leave it empty
without guessing a Control broker hostname. The embedding worker's Model Plane path requires
its dedicated Auth Core service-principal credential, mints an org-bound
`aud=inference-core` / `inference:invoke` bearer, and sends only
`Authorization: Bearer` metadata. The missing workspace `jsonwebtoken`
dependency and two test-only compile/runtime defects were fixed with focused
regressions.

Evidence dated 2026-07-16:

- `cargo test --workspace` passed; all four Go service modules' `go test ./...`
  passed; embedding-engine's focused suite passed **29 tests**; workspace
  fmt/check and strict targeted embedding clippy passed.
- `COMPOSE_ANSI=never bash tests/e2e/run-isolated-mvp.sh` rebuilt every Data
  image, reached a healthy disposable stack, passed **31 methods / 124 gRPC
  shapes**, **28/28 HTTP assertions**, and the six-store restrictive-ZDR
  final-state comparison. The random project, network, volumes, containers,
  images, and generated keys were removed by scoped cleanup.
- Compose security, provenance, isolated-harness, broker/GDPR, gRPC, browser,
  real-authority, and multi-store contracts all passed.

This remains source/isolated evidence. The connected posture intentionally
fails closed until operators provision deployment event keys, the Control
shared GDPR broker password, and the dedicated embedding service credential.
The local `.env` records the non-secret Control broker URL/user and sandbox
event-key paths only; `make cross-plane-env` rejects connected startup while
deployment-owned values are absent. The
isolated provider uses deterministic embeddings; a rebuilt sandbox hop to the
real Model Plane `inference-core` is still required before claiming deployed
cross-plane readiness. Shared images and production ACLs were not changed.

## 2026-07-16 independent-startup and credential-boundary verification

Data Plane now has three explicit startup postures:

- `make standalone-up` uses the private local cross-plane network and pauses
  only signed event consumers plus the Control-owned GDPR durable subscriber.
  Authenticated routes remain strict; missing Auth/User/Control/Model
  authorities fail closed, and grant-only documents remain hidden.
- `make cross-plane-up` applies `docker-compose.cross-plane.yml` and attaches
  running services to the pre-provisioned `inter-plane-bus` network. It never
  creates or guesses a shared production network.
- `make up` keeps the production defaults. With
  `GDPR_DURABLE_CONSUMER_REQUIRED=1` or `USER_CORE_GRANTS_REQUIRED=1`, missing
  Control-owned credentials terminate the affected service rather than
  broadening visibility or silently disabling erasure handling.

The graph index no longer fetches Auth Core JWKS during process startup; it
uses the deployment-configured, read-only public verification key mount and
still verifies issuer, audience,
signature, time, identity, and tenant on every protected request. Go JWKS,
Control policy, User Core, and Model Plane clients remain lazy runtime edges.

Evidence: `go test ./...` passed for all four Go services; `cargo test
--workspace`, `cargo fmt --all -- --check`, `bash
tests/scripts/compose-security-contract-test.sh`, `docker compose config --quiet`
for both overlays, and `git diff --check` all passed on 2026-07-16. No
deployment-owned broker password, Auth Core service-principal key, or
production event key was generated or copied. Those values must be injected by
Control/Auth/Model deployment owners; local synthetic values belong only in the
ignored isolated harness.

The preflight also rejects event-key paths under `.secrets/event-keys/`, so a
connected rollout cannot accidentally reuse local sandbox signing material.

The rebuilt local standalone stack was then started with the new images. All
15 long-running Data services reached healthy state, including graph-index and
embedding-engine with missing external inference credentials; their content
paths remain unavailable until a registered service principal is supplied.
Documents reached `/readyz` healthy with the GDPR subscriber paused by the
explicit standalone overlay. This is local runtime evidence, not a shared
deployment claim.

## Evidence vocabulary

- **Implemented**: present in the current dirty worktree.
- **Tested**: exercised locally by the command/result recorded below.
- **Built**: a local binary or image was produced from this worktree.
- **Deployed**: a running container was replaced with that build.
- **Reachable**: a caller reached the deployed surface.
- **Effective**: the deployed surface produced the expected allow/deny or ZDR behavior.

Unless stated otherwise, current claims are implemented/tested only. No production deployment mutation was performed.

## MVP acceptance status

| Criterion | Status | Current evidence / blocker |
|---|---|---|
| A. Verified identity on every sensitive boundary | **Source + isolated pass; deployment pending** | Real session, service-token, HTTP, gRPC, and supported signed-event boundaries fail closed. Unsupported unsigned listeners/consumers remain disabled. |
| B. Four-shape tenant matrix | **Isolated pass** | **28/28 HTTP** and **124/124 gRPC** assertions pass. gRPC reads require `OK` or a named authenticated `NotFound`; mutations require exact named guards; cross-tenant checks require service-specific tenant-mismatch reasons. The matrix itself refuses non-loopback targets. |
| C. Documents JWT/JWKS and service visibility | **Source + isolated pass; deployment pending** | Strict RS256 issuer/audience/JWKS startup, unsigned-token denial, scoped service identity, visibility, single/bulk/source-object monotonic ZDR, and final rebuilt runtime checks pass. Changed auth/ZDR modules exceed 80% coverage. |
| D. Retrieval + Control membership contract | **Isolated real-authority pass** | Two real users/organizations were created through supported Auth/User/Control contracts. Authorized retrieval/Knowledge/GraphRAG works; spoofed/cross-tenant access fails. One bounded transient policy retry is allowed; exhaustion fails closed and Control outage maps to `Unavailable`, not false non-membership. |
| E. Destructive/admin safety | **Contained; execution proof pending** | Quickwit preview/job lifecycle is authenticated, tenant-default, idempotent, approved, audited, rate/concurrency bounded, and durable. Actual clear remains 501 until trustworthy Quickwit completion exists; no destructive call was made. |
| F. One restrictive ZDR ingest guard | **Isolated final-state pass; strict mutation telemetry pending** | Signed restrictive posture covers single/bulk/source-object and downstream cache/event paths. Final state stayed identical across six stores; NATS sequence and Dragonfly command counters provide monotonic no-write evidence. Per-operation mutation telemetry is still needed to rule out transient write/delete in Postgres, Qdrant, Quickwit, and MinIO. |
| G. Cache bypass and signed ZDR propagation | **Source + isolated pass; production policy pending** | Auth, Frontend, Model, Ingestion, Documents, Retrieval, and signed events use monotonic restrictive posture; cache/trace/embedding/semantic/event persistence is bypassed or rejected. Production organization retention policy and deployed proof remain pending. |
| H. Auxiliary visibility/grants | **Tested, runtime pending** | Retrieval auxiliary visibility tests use the canonical contract. Control v2 read delegation binds caller, user, tenant, operation, resource type/ID, reason, nonce, request digest, and restrictive ZDR, and now also requires an independently verified matching user bearer. Grant reads are uncached so revocation does not depend on a misrouted event bus. Grant mutation/listing remains denied until resource-owner authorization is defined. |
| I. Schema/runtime reconciliation | **Tested, deployment pending** | Wiki migration integration passes; retrieval trace migration applies twice and exposes its actor column/index in disposable PostgreSQL. |
| J. Compose hardening | **Tested; deployment provisioning pending** | Base Compose uses a private local network; the explicit cross-plane overlay attaches only to a pre-provisioned shared network. The standalone overlay pauses cross-plane consumers without weakening auth/ZDR. App services remain private/loopback-bound and verifier containers receive public keys only. Production still requires Control-owned GDPR broker credentials, event keys, and registered Model/Auth service principals. |
| K. Builds/tests/live matrix | **Isolated pass; shared rollout pending** | Real-authority browser 2/2, Gateway 288/288, SPA 68 files/364 tests, Data HTTP/gRPC/ZDR matrices, Retrieval full/focused suites, typecheck/build, fmt/clippy, and Go build/test gates pass. Shared-image replacement, real Model Plane embedding-hop proof, and safe post-deploy verification remain pending. |
| L. Accurate evidence docs | **Pass for current checkpoint** | Current sections distinguish source, isolated runtime, shared deployment, and unresolved telemetry/coverage. Older checkpoint notes are retained as history and explicitly superseded. |

## Service state

| Service | Auth/tenant state in source | Secure-default runtime state | Remaining MVP blocker |
|---|---|---|---|
| documents-api-go | Strict JWT/JWKS, required signed boolean ZDR posture, canonical user/service claims, tenant/owner pinning, explicit service scopes | Final rebuilt HTTP/gRPC and six-store final-state ZDR matrices pass; release Compose requires the scoped durable GDPR consumer | Production scoped-broker provisioning/rollout, strict mutation telemetry, authoritative non-restrictive retention policy, and shared deployment |
| retrieval-engine-rs | Strict JWT, claim-pinned HTTP/gRPC/auxiliary routes, Control bearer decision/read-delegation, and tenant-bound Inference service bearer | Real-authority user reads and GraphRAG pass; transient Control token failures retry once, exhaustion fails closed, and dependency outage is `Unavailable` | Shared deployment and safe post-deploy real-user verification |
| graph-index-rs | Strict JWT/scoped service principal, tenant pinning, signed-event tenant equality, org-visible provenance, tenant-bound Inference service bearer | HTTP/gRPC auth and synthetic GraphRAG progression pass in the isolated real-authority stack | Shared deployment and safe post-deploy verification |
| data-quality-go | Strict JWT/JWKS and `data:quality:admin` route scope; durable tenant-scoped eval store | HTTP enabled; pending and expired-running evals are recovered by an atomic multi-replica-safe loop | Disposable-PostgreSQL recovery rerun and rebuilt-image matrix |
| data-orchestrator-go | Strict JWT/JWKS and operation scopes; durable tenant-scoped job lifecycle | Reads enabled; production mutations return 503 before persistence while no signed resumable worker exists; unsigned cost consumer disabled | Implement signed durable worker/callback identity before enabling mutations |
| quickwit-adapter-rs | Strict admin JWT; durable tenant-default jobs, approval, leases, checkpoints, audit, rate/concurrency bounds | Preview/job lifecycle enabled in source; destructive clear and unsigned consumers disabled | Rebuilt runtime matrix and trustworthy Quickwit task completion before clear |
| embedding-engine-rs | ZDR egress guards; signed document/wiki consumption; dedicated Auth Core inference bearer with exact audience/scope | 29 focused tests, strict clippy, corrected image build, signed broker delivery matrix, and isolated startup pass; legacy unsigned modes disabled | Real Model Plane `inference-core` hop in a provisioned sandbox, shared broker ACL rollout, and post-deploy progression proof |
| index-engine-rs | No external API; signed document consumption and transactional deletion outbox | Signed broker delivery matrix and isolated startup pass; legacy unsigned consumer disabled | Shared broker ACL rollout and database-backed outbox coverage |
| wiki-store-go | Strict HTTP/gRPC JWT and operation scopes; atomic signed acknowledged event outbox | HTTP/gRPC auth, migration, ZDR mutation guard, and signed broker delivery pass in isolation | Shared rollout and database-backed outbox coverage |
| retrieval-eval-py | No active implementation | Not deployed | Define or remove from MVP runtime scope |
| colqwen-reranker | Optional isolated GPU service | Not deployed by design | No Data MVP blocker while visual rerank remains off |

Cross-plane containment: Model Gateway gRPC, Execution Core gRPC, and Inference
Core gRPC production binaries do not expose the unverified listeners; their
listener constructors are test-only and Data Compose no longer publishes the
legacy ports. Static credentials cannot perform delegated-user/grant mutation.
Signed v2 Control read delegation is implemented/tested, but this remains source
evidence rather than functional runtime acceptance.

## Verification ledger

### 2026-07-15 final isolated acceptance delta

- Real Auth Core/User Core/Control fixture plus Verevon browser E2E: **2/2**
  Playwright journeys pass. One authorized user sees only its own Knowledge,
  GraphRAG nodes, and navbar retrieval results; spoofed organization headers and
  searches cannot disclose any serialized field from the second fixture.
- Data runtime: **31 methods / 124 gRPC auth assertions** and **28/28 HTTP auth
  assertions** pass against a unique loopback-only Compose project. The gRPC
  harness itself rejects non-loopback targets and exact-checks permanent legacy
  document-write containment, ZDR, approval-scope, and tenant-mismatch reasons.
- Restrictive ZDR: stabilized final state is identical before/after across
  PostgreSQL, Qdrant, Dragonfly, Quickwit, MinIO, and NATS. NATS sequence and
  Dragonfly counters are monotonic no-write checks; the other stores remain
  final-state checks, not proof against a transient write/delete cycle.
- Broker delivery: supported signed event flows pass the disposable broker
  delivery/redelivery matrix. The real Documents binary also binds a separately
  pre-provisioned scoped Control-style GDPR durable, reports healthy, ACKs only
  after ownership transfer, holds its ACK floor and redelivers while disposable
  PostgreSQL is unavailable, then recovers and ACKs. Its principal cannot publish
  Control input or administer the stream. No shared broker subject, customer
  data, or destructive admin route was used.
- Retrieval final full suite: **212 passed, 10 explicit infrastructure ignores**.
  Policy/interceptor security files measure 93.76% and 88.14% line coverage.
  The retry-exhaustion regression proves two immediate 503 attempts finish
  within one second, make no decision call, and return fail-closed Control
  unavailability. Final fmt and single-job, non-incremental strict all-target
  clippy pass; the preceding parallel attempt ended in a host SIGBUS without a
  lint diagnostic and is not counted as a code failure.
- Verevon Gateway: **288/288** tests. SPA: Node-native security gate **4/4** and
  Vitest **68 files / 364 tests**, plus typecheck and production build. Lint has
  zero errors and one existing Solid reactivity warning.
- Base Data Compose resolves with fresh explicit required inputs, and broker,
  gRPC, real-authority, browser, multi-store ZDR, and Compose security contracts
  pass. All projects generated unique names/credentials and removed only their
  own containers, volumes, networks, and temporary keys.

This section supersedes earlier 2026-07-15 checkpoint statements that Docker was
unavailable or that the final Documents image/multi-store/browser/gRPC reruns
were pending. Those statements remain below as chronological incident history.

### 2026-07-15 Verevon/GraphRAG delta

- Graph: `cargo test --no-default-features` **28 passed, 1 explicitly ignored**;
  focused extractor **9/9**; strict all-target no-default-feature clippy passed.
- Retrieval: `cargo test --all-targets` **193 non-ignored passed, 10 explicitly
  ignored**; strict all-target clippy passed. Focused HTTP JWT **5/5** and
  embedding/service-auth **19/19** passed.
- Control Auth Core per-audience service registry: **12/12** focused Jest tests,
  build, and Compose security contract passed. Changed-module coverage is
  **90.19% statements, 93.84% branches, 100% functions, 89.79% lines**.
- Verevon Gateway: `cargo test --all-targets` **272/272**. SPA: **68 files / 360
  tests**, typecheck, and production build passed. Lint returned zero errors and
  one pre-existing Solid reactivity warning.
- Data Compose config with non-secret placeholders passed. The isolated MVP
  script uses a random project, network, volumes, keys, organizations, and
  cleanup trap; no shared/customer content or destructive admin operation is
  used.
- The pre-final-fix isolated Data images were built with base-revision label
  `eeebd0bc98c66434936460020958891066eb05fd` and created label
  `2026-07-15T00:45:36Z`. The rebuilt stack passed **28/28** HTTP authorization
  checks. An authenticated durable write control succeeded, then body-restrictive
  single/bulk ingest returned exact ZDR denials and left the stabilized
  PostgreSQL content snapshot unchanged through a ten-second window. The
  disposable containers, network, volumes, and generated key directory were
  removed by the cleanup trap. Because the build intentionally included the
  dirty worktree, that Git label alone is not a reproducible release identifier;
  these are checkpoint verification artifacts, not release images.
- That stronger control exposed a missing claim-level guard: its bearer carried
  signed `zdr:true`, yet the checkpoint image accepted the 201. New RED/GREEN
  tests require a signed boolean claim, preserve it, enforce
  `verified_claim.zdr OR request_policy.zdr` for single/bulk persistence, and
  reject source-object upsert before its row/outbox write under restrictive
  posture. Documents full race/vet/build pass; auth coverage is 95.5% and both
  changed ZDR decision helpers are 100% (the broad handler package is 14.9%).
  Docker Desktop then became unavailable before the final image
  rebuild, so this latest repair is not claimed runtime-effective.
- The isolated startup path exposed and fixed two contract/reliability defects:
  retrieval now defaults an omitted `filters` object to empty filters, and the
  migrator retries transient PostgreSQL connection refusal within a bounded
  fail-closed window. Retrieval fmt/clippy/tests and migrator Go test/vet pass.

### 2026-07-10–11 retained evidence

- Data Rust final workspace fmt/check/test/strict all-target clippy: **299 passed,
  15 explicitly ignored**. Every database/cache/Docker-dependent test is now an
  explicit named ignore rather than a silent pass: graph 1, index 2, Quickwit 2,
  retrieval PostgreSQL/Dragonfly 7, and retrieval Docker E2E 3.
- `go test -race ./...` and `go vet ./...` in documents, quality, orchestrator, and wiki: passed.
- Measured auth coverage: documents 95.4%, quality 82.8%, orchestrator 91.1%, wiki 85.2%.
- Wiki schema migration integration against disposable PostgreSQL: passed.
- Retrieval trace actor migration applied twice against disposable PostgreSQL: passed; actor column and index verified.
- Model Rust full workspace: **893 passed, 4 explicitly ignored**; strict
  workspace all-target clippy passed. This includes the Model Gateway,
  Execution Core, and Inference Core secure-containment changes.
- Frontend Gateway: 206 tests passed.
- Quarry-v2 runtime plus edge: **652 passed, 0 ignored**; strict clippy passed.
- Control policy: 41 focused authorization tests passed. After the production
  dependency refresh, 30 focused policy/token tests and the full build passed;
  pnpm and npm production audits reported no known vulnerabilities.
  Controller/token coverage is 88.46% statements, 84% branches, 95% functions,
  and 88.2% lines.
- Auth Core signed Model ZDR/service audit: full Jest **99/99**, full no-fix
  ESLint, build, and frozen install pass. Focused security coverage is **90.00%
  statements, 90.04% branches, 94.73% functions, and 89.77% lines**. Both
  user and service issuance require and sign restrictive `zdr:true`;
  false/omitted downgrade tests pass. Service issuance publishes the bounded
  caller, tenant, scopes, and reason to the local Control audit subject and
  returns 503 when that publisher is unavailable. Audit-core persistence has
  not yet been proven by the isolated runtime matrix.
- Auth Core full and production pnpm audits report **zero advisories**.
- Control user-core: arbitrary static-service `X-User-Id` and tenant/grant
  requests went RED at HTTP 200 and GREEN at 403. V2 grant reads now require a
  matching RS256 `aud=data-plane` user bearer and consume each delegation nonce;
  missing/mismatched/unsigned/replayed proof returns 403. Full race/build/vet and
  `govulncheck` pass. Existing service-auth helpers remain 100%; new proof/replay
  functions measure 81.8–100% individually and the broad HTTP package is 19.8%.
- Signed event evidence: `event-envelope-rs` measures **92.66% regions, 94.91%
  lines, 100% functions**. Documents `eventauth` measures 89.8% statements and
  `authctx` 95.5%. Wiki `eventauth` measures 88.1%, config 84.2%, focused pure
  outbox drain/PubAck behavior 100%, and the full race profile for
  `internal/events` 28.7%; its PostgreSQL/JetStream paths have not been rerun.
- Quickwit coverage: `auth.rs` 97.06% lines, `api.rs` 86.73%, and `jobs.rs`
  52.53%. The crate passes **43 tests with two explicit PostgreSQL ignores**; the
  disposable PostgreSQL lifecycle separately passed in the earlier fixture.
- Quality auth coverage is 82.8%; the current non-database eval profile is 26.7%
  and its recovery function is 69.2%. Orchestrator auth coverage is 91.1%; the
  current non-database jobs profile is 31.9%. These honest profiles are below the
  80% target; their expanded database-backed coverage has not yet been rerun.
- Go `govulncheck ./...`: no reachable vulnerabilities in documents, wiki,
  quality, or orchestrator. Migrator initially exposed four reachable findings;
  pgx was upgraded from v5.7.4 to v5.9.2 and the Go build pin to 1.26.5, after
  which test/vet/build/govulncheck pass with `No vulnerabilities found`. Its
  current image also built for the disposable matrix; bounded startup-retry
  tests and the isolated migration run pass.
- Control User Core initially exposed 13 reachable findings. Its Go/build pin is
  now 1.26.5 with pgx 5.9.2, gRPC 1.79.3, quic-go 0.59.1, x/net 0.53.0, and
  compatible transitive locks; full race/vet/build and `govulncheck` now pass
  with zero reachable findings. The rebuilt image/runtime is unverified.
- Ingestion Shipping initially exposed seven reachable standard-library
  findings. Pinning its Go module and Docker builder to 1.26.5 leaves full
  race/vet/build and `govulncheck` green with zero reachable findings; the scan
  still reports one required-module advisory as not called. Its image/runtime is
  unverified. Quarry's fixable RustSec graph was upgraded; runtime+edge tests and
  strict clippy pass, and its production audit reports no unignored vulnerability
  (RSA is proven absent from every normal/build graph; only recorded unmaintained
  dependency warnings remain).
- Control auth-core production dependency audits: `corepack pnpm audit --prod --audit-level high` and `npm audit --omit=dev --audit-level=high` report zero known vulnerabilities after targeted upgrades/overrides.
- RustSec: no unignored Data or Model vulnerabilities. The audit script permits
  only the unfixed RSA advisory after proving `rsa` is dev/test-only; Data also
  retains the explicitly recorded unmaintained `rustls-pemfile` warning.
- Data plus optional self-owned, Model, Control, and the new Model/Ingestion
  production overrides validate with explicit test-only credentials. The
  production overrides reset 19 Model and 14 Ingestion host-port publications.
- Static tenant scanner, 28-case auth harness contract, safe smoke/load contracts,
  image provenance contract, isolated-MVP contract, and Compose security
  contract: passed. The live auth matrix was not invoked.

The earlier Docker content-store failure cleared without restarting or mutating
the shared engine. The 2026-07-15 isolated project built a passing checkpoint
and completed its safe HTTP/body-ZDR matrix. That checkpoint predates the final
signed-ZDR and source-object guards, which are source-tested only because Docker
became unavailable before the rebuild. This does **not** prove the unchanged
shared deployment, enabled gRPC families, broker redelivery/ACL behavior,
cross-plane Model/Execution runtime identity, or the pending database-backed
outbox and expanded durability suites.

## Immediate operational notes

- Do not enable any `ALLOW_UNVERIFIED_LEGACY_EVENTS` or unauthenticated gRPC gate outside disposable development.
- Do not restore static `users:*:self` or tenant-selected authz-facade scopes.
  Use only the implemented v2 signed, request-bound read delegation; keep grant
  mutation/listing denied until resource-owner authorization is defined.
- Rotate the local Model shared credential and retrieval Control-policy service
  credential accidentally surfaced by audit commands before any deployment.
  Their values are intentionally not recorded here.
- Do not invoke Quickwit global rebuild, orphan cleanup, bulk deletion, index reset, purge, or other destructive routes on a shared stack.
- Existing customer data was not read, copied, enumerated, or mutated during remediation.

See `DATA_PLANE_ROADMAP.md` for remaining MVP work and `docs/core-research/plane-audit-2026-07-10.md` for the sanitized audit trail.
