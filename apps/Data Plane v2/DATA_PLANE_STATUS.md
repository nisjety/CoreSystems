# Data Plane v2 — Secure-MVP Status

Last verified: 2026-07-11

Verdict: **not production-ready**. The previously verified anonymous,
header-only, static-service impersonation, private-key mount, and restrictive-ZDR
paths are closed or contained in source and regression tests. Producer-scoped
signed event envelopes, acknowledged transactional outboxes, signed Control
read delegation, durable quality/orchestrator state, and durable Quickwit admin
jobs now exist in source. Delegated grant reads additionally require the original
RS256 user proof and are intentionally uncached. Unsupported unsigned consumers, deprecated retrieval
writes, destructive Quickwit clearing, and unverified Model gRPC listeners remain
fail-closed. Images for the current source could not be rebuilt or deployed and
the isolated Docker/curl matrix could not run because Docker's containerd content
store is returning blob input/output errors. Enterprise readiness has not started.

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
| A. Verified identity on every sensitive boundary | **Partial** | Data HTTP/gRPC auth tests pass. Legacy static Control credentials cannot impersonate users. Supported async document/wiki/index/embedding/graph paths now use producer-scoped signed envelopes, and signed Control read delegation binds caller/user/tenant/operation/resource/ZDR. Model HTTP service principals require canonical identity/reason and exact `models:invoke`. Unsupported unsigned consumers and Model/Execution/Inference gRPC listeners remain disabled, so secure functionality is incomplete. |
| B. Four-shape tenant matrix | **Implemented, runtime pending** | Route-family unit tests pass; the sanitized 7-family × 4-shape HTTP script contract passes. It has not run against an isolated rebuilt stack. |
| C. Documents JWT/JWKS and service visibility | **Tested** | Strict RS256/JWKS startup/auth behavior, canonical service identity, visibility, unsigned-token denial, idempotency ownership, and ZDR guards pass. Auth package coverage is 95.4%. Deployment is pending. |
| D. Retrieval + Control membership contract | **Tested, runtime pending** | Control verifies a scoped `aud=control-policy` service bearer. Retrieval mints an org-bound token and validates the bounded profile. Grant resolution forwards the original verified `aud=data-plane` user bearer; User Core independently verifies RS256/issuer/audience/time/user/tenant, consumes the v2 nonce, and rejects mismatch/replay. No isolated real-user E2E yet. |
| E. Destructive/admin safety | **Partial** | Quickwit requires dedicated scopes and has durable tenant-default jobs, idempotency, two-person approval, leases/checkpoints, append-only audit, rate/concurrency bounds, and read-only preflight. The disposable PostgreSQL lifecycle passed. Actual clear remains 501 until trustworthy Quickwit completion proof exists; no destructive request was run. |
| F. One restrictive ZDR ingest guard | **Tested, E2E pending** | Single/bulk durable document ingest reject restrictive ZDR; Quarry suppresses durable handoff/CAS/events. Full multi-store zero-persistence proof is pending. |
| G. Cache bypass and signed ZDR propagation | **Partial** | Auth Core issues user/service Model tokens only with signed `zdr:true`; Model uses monotonic `verified_claim.zdr OR request.zdr` across direct document/bulk/retrieval/wiki proxy boundaries and suppresses publisher/session/cache writes. Retrieval accepts only exact ZDR enums and skips retaining text rerank/embedding egress under restrictive modes. Signed async envelopes bind tenant, producer, audience, scope, subject, payload digest, expiry, replay identity, and ZDR; outbox retries also use stable broker message IDs. Full broker/storage runtime proof remains absent. |
| H. Auxiliary visibility/grants | **Tested, runtime pending** | Retrieval auxiliary visibility tests use the canonical contract. Control v2 read delegation binds caller, user, tenant, operation, resource type/ID, reason, nonce, request digest, and restrictive ZDR, and now also requires an independently verified matching user bearer. Grant reads are uncached so revocation does not depend on a misrouted event bus. Grant mutation/listing remains denied until resource-owner authorization is defined. |
| I. Schema/runtime reconciliation | **Tested, deployment pending** | Wiki migration integration passes; retrieval trace migration applies twice and exposes its actor column/index in disposable PostgreSQL. |
| J. Compose hardening | **Tested** | Data Compose publishes no service/infra host ports and mounts only Control's public verification file. Control pprof defaults off. Optional self-owned dependencies are private. Model and Ingestion production overrides reset every base-published host port, disable dev bypasses, and require credentials; both merged configs validate with test-only values. Data/Model/Control builds require provenance. |
| K. Builds/tests/live matrix | **Partial** | Data Rust passed 299 tests with 15 explicit infrastructure ignores and strict clippy; Model Rust passed 893 with 4 ignores and the changed Gateway all-target suite/clippy; Quarry runtime/edge passed 652 with no ignores and strict clippy/audit; Auth Core passed 99/99 plus lint/build/audits/coverage. Current images/E2E remain blocked by Docker containerd blob I/O failure. |
| L. Accurate evidence docs | **Tested** | Central and per-service evidence distinguishes implemented/tested/built/deployed/reachable/effective state and is sanitized. Previous image evidence predates later source changes; no current build, deployment, or runtime effectiveness is claimed. |

## Service state

| Service | Auth/tenant state in source | Secure-default runtime state | Remaining MVP blocker |
|---|---|---|---|
| documents-api-go | Strict JWT/JWKS, canonical user/service claims, tenant/owner pinning, explicit service scopes | HTTP/gRPC enabled; transactional outbox signs producer-scoped lifecycle events and requires JetStream acknowledgement | Rebuilt-image event/runtime matrix and unsupported GDPR consumer replacement |
| retrieval-engine-rs | Strict JWT, claim-pinned HTTP/gRPC/auxiliary routes, Control bearer decision and v2 signed read-delegation contracts | User reads enabled; deprecated gRPC Create/Delete/Bulk permanently return `FailedPrecondition` | Isolated authorized-user/delegation E2E |
| graph-index-rs | Strict JWT/scoped service principal, tenant pinning, org-visible provenance filtering | Read APIs enabled; supported extraction events verify signed envelopes; legacy cleanup remains disabled | Runtime broker matrix and inference auth |
| data-quality-go | Strict JWT/JWKS and `data:quality:admin` route scope; durable tenant-scoped eval store | HTTP enabled; pending and expired-running evals are recovered by an atomic multi-replica-safe loop | Disposable-PostgreSQL recovery rerun and rebuilt-image matrix |
| data-orchestrator-go | Strict JWT/JWKS and operation scopes; durable tenant-scoped job lifecycle | Reads enabled; production mutations return 503 before persistence while no signed resumable worker exists; unsigned cost consumer disabled | Implement signed durable worker/callback identity before enabling mutations |
| quickwit-adapter-rs | Strict admin JWT; durable tenant-default jobs, approval, leases, checkpoints, audit, rate/concurrency bounds | Preview/job lifecycle enabled in source; destructive clear and unsigned consumers disabled | Rebuilt runtime matrix and trustworthy Quickwit task completion before clear |
| embedding-engine-rs | ZDR egress guards; signed document/wiki consumption and signed downstream progression | Legacy unsigned modes disabled | Runtime broker matrix and verified inference principal |
| index-engine-rs | No external API; signed document consumption and transactional deletion outbox | Legacy unsigned consumer disabled | Disposable-PostgreSQL outbox test and runtime broker matrix |
| wiki-store-go | Strict HTTP/gRPC JWT and operation scopes; atomic signed acknowledged event outbox | APIs/outbox enabled in source | Disposable-PostgreSQL outbox/migration proof and rebuilt-image matrix |
| retrieval-eval-py | No active implementation | Not deployed | Define or remove from MVP runtime scope |
| colqwen-reranker | Optional isolated GPU service | Not deployed by design | No Data MVP blocker while visual rerank remains off |

Cross-plane containment: Model Gateway gRPC, Execution Core gRPC, and Inference
Core gRPC production binaries do not expose the unverified listeners; their
listener constructors are test-only and Data Compose no longer publishes the
legacy ports. Static credentials cannot perform delegated-user/grant mutation.
Signed v2 Control read delegation is implemented/tested, but this remains source
evidence rather than functional runtime acceptance.

## Verification ledger — 2026-07-10–11

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
  `authctx` 95.4%. Wiki `eventauth` measures 88.1%, config 84.2%, focused pure
  outbox drain/PubAck behavior 100%, and the full race profile for
  `internal/events` 28.7% because the PostgreSQL/JetStream paths could not run.
- Quickwit coverage: `auth.rs` 97.06% lines, `api.rs` 86.73%, and `jobs.rs`
  52.53%. The crate passes **43 tests with two explicit PostgreSQL ignores**; the
  disposable PostgreSQL lifecycle separately passed before Docker storage failed.
- Quality auth coverage is 82.8%; the current non-database eval profile is 26.7%
  and its recovery function is 69.2%. Orchestrator auth coverage is 91.1%; the
  current non-database jobs profile is 31.9%. These honest profiles are below the
  80% target; the PostgreSQL paths cannot be reprofiled while Docker is broken.
- Go `govulncheck ./...`: no reachable vulnerabilities in documents, wiki,
  quality, or orchestrator. Migrator initially exposed four reachable findings;
  pgx was upgraded from v5.7.4 to v5.9.2 and the Go build pin to 1.26.5, after
  which test/vet/build/govulncheck pass with `No vulnerabilities found`. Its
  current image rebuild remains Docker-blocked.
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

Docker runtime evidence is blocked by a host engine failure, not a test failure:
sanitized `docker system df` returns `failed to retrieve image list: rpc error ...
blob ... open ...: input/output error`. At observation time Docker reported 99
containers, 86 running; restarting/repairing the shared engine would disrupt
out-of-scope workloads and was not authorized. Consequently current image builds,
the isolated Compose/curl matrix, and the pending disposable-PostgreSQL outbox and
expanded durability tests are unproven.

## Immediate operational notes

- Do not enable any `ALLOW_UNVERIFIED_LEGACY_EVENTS` or unauthenticated gRPC gate outside disposable development.
- Do not restore static `users:*:self` or tenant-selected authz-facade scopes.
  Use only the implemented v2 signed, request-bound read delegation; keep grant
  mutation/listing denied until resource-owner authorization is defined.
- Rotate the shared internal credential accidentally exposed from a local Model `.env` during this audit before any deployment. Its value is intentionally not recorded here.
- Do not invoke Quickwit global rebuild, orphan cleanup, bulk deletion, index reset, purge, or other destructive routes on a shared stack.
- Existing customer data was not read, copied, enumerated, or mutated during remediation.

See `DATA_PLANE_ROADMAP.md` for remaining MVP work and `docs/core-research/plane-audit-2026-07-10.md` for the sanitized audit trail.
