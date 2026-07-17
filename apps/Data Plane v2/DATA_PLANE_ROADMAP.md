# Data Plane v2 — Secure-MVP Roadmap

Updated: 2026-07-16. Read `DATA_PLANE_STATUS.md` first.

Only the secure MVP is active. The enterprise phase remains explicitly unopened until criteria A–L are all proven on rebuilt, deployed images.

Current checkpoint: the latest disposable source build is green for the real
Auth/User/Control Velion journey, strict HTTP/gRPC matrices, supported signed
broker flows, and the six-store ZDR final-state comparison. Any older note below
that says Docker was unavailable or those final isolated reruns were pending is
retained as chronology and superseded by this checkpoint. Remaining work is
production provisioning/rotation/deployment, strict mutation telemetry for four
stores, database-backed security coverage, and safe post-deploy verification.

2026-07-16 rebuild delta: the production Compose GDPR broker URL is explicit
and fail-closed (standalone leaves the subscriber paused), and embedding-engine's Model Plane path requires a dedicated
Auth-Core-issued `aud=inference-core` / `inference:invoke` bearer rather than a
shared API key. The missing embedding workspace dependency and test-only runtime
defects were fixed. A fresh isolated rebuild passed all images, **31/124 gRPC**,
**28/28 HTTP**, and the six-store restrictive-ZDR final-state matrix. The real
Model Plane inference hop and production broker/secret rollout remain deployment
evidence, not source blockers.

2026-07-16 startup decoupling delta: Data Plane's default network is now
private and local, `docker-compose.standalone.yml` provides an explicit
non-production boot posture, and `docker-compose.cross-plane.yml` attaches to
the pre-provisioned shared network only when Control/User/Model are available.
Graph JWT verification uses a mounted public key instead of a synchronous JWKS
fetch, so the process can bind while Auth Core is offline; protected requests
still fail closed. The `.env` cannot be completed with deployment-owned GDPR
broker credentials, event keys, or Model/Auth service-principal keys from this
plane; those remain Control/Auth/Model secret-manager rollout blockers.

The rebuilt standalone stack was started locally on 2026-07-16 and all 15
long-running Data services reported healthy. Graph and embedding bind without
an inference principal in this explicit posture but reject inference work until
the registered credential is present. Documents `/readyz` is healthy with its
Control-owned GDPR consumer paused; connected startup remains a separate
post-provisioning step.

## Completed source remediations

1. **Anonymous/header-only containment**
   - Graph, quality, orchestrator, documents, retrieval, wiki, and Quickwit sensitive routes require verified JWTs or explicitly scoped service principals.
   - Tenant and user scope come from verified claims. Conflicting headers/body/path values are rejected.
   - Quickwit admin mutation is fail-closed; preview is authenticated and tenant-scoped.

2. **Documents privacy and ZDR**
   - Signature, issuer, audience, time, identity, and JWKS validation default strict and startup validation fails closed.
   - Service reads retain service identity/visibility unless an explicit org-wide scope exists.
   - Idempotency cannot cross owner boundaries.
   - Single and bulk durable ingest share the restrictive ZDR guard. Documents
     now requires a signed boolean `zdr` claim and applies
     `verified_claim.zdr OR request_policy.zdr` before persistence. Source-object
     upsert also rejects restrictive or missing verified posture before its
     source row and outbox event can be written.

3. **Retrieval authorization and visibility**
   - User JWTs are verified for HTTP/gRPC and tenant/user are claim-pinned.
   - Retrieval→Control membership uses a short-lived org-bound `aud=control-policy` service bearer; the decision endpoint independently verifies it.
   - Auxiliary handlers share canonical owner/org/grant-only-shared semantics.
   - Explicit-grant reads forward the original verified user bearer; User Core
     independently verifies its signed user/tenant claims and consumes the v2
     nonce. Retrieval leaves grants uncached so revocation is next-request safe.
   - Trace actor attribution, chunk text schema, and wiki visibility/runtime drift are fixed in source/migrations.
   - Embedding and semantic cache writes are bypassed under restrictive ZDR.

4. **Cross-plane ZDR propagation**
   - Frontend sends separate Model/Data audience tokens and forces restrictive JSON posture.
   - Model verifies both tokens and requires subject/tenant equality.
   - Documents independently verifies the Data token's signed ZDR posture and
     rejects durable single/bulk writes whenever either signed or body posture
     is restrictive; source-object upsert is denied under restrictive posture.
   - Unary/stream Model paths suppress session, event, idempotency, response-cache, semantic-cache, and publisher effects under ZDR. Direct document/bulk/retrieval/wiki proxies enforce the same monotonic signed posture, and restrictive retrieval skips retaining rerank/embedding egress.
   - Quarry single/bulk/CAS/event paths use one durable-persistence guard.

5. **Secure-default containment**
   - Unsigned mutation consumers in documents, graph, Quickwit, orchestrator, embedding, and index are disabled unless two explicit insecure-development gates are set.
   - Unverified Model Gateway, Execution Core, and Inference Core production
     listener constructors are removed/test-confined; legacy Compose host ports
     are removed.

6. **Compose and provenance**
   - Data application services have no host publications. Default Postgres and
     Qdrant maintenance ports are loopback-only; optional observability is also
     loopback-only. The isolated acceptance profile resets the maintenance
     publications entirely.
   - Credentials are required, internal stores stay on the private Data network, and insecure gates default off.
   - Data verifiers mount only Control's public key file, never the signing-key directory.
   - Control diagnostics default off; optional self-owned dependencies have no host publications/default credentials.
   - Model and Ingestion production overrides reset all base-published ports,
     disable dev bypasses, and require credentials; merged configs validate.
   - Dockerfiles/Compose carry OCI revision/build-label contracts and require
     `SOURCE_REVISION` and `BUILD_DATE`. Earlier image inspection predates later
     source changes; the isolated checkpoint images carry both labels, but the
     final Documents signed-ZDR/source-object patches still require a rebuild. The unchanged
     shared deployment requires a controlled rebuild/inspection.
   - Data services can boot independently with `make standalone-up`; the
     connected `make cross-plane-up` target is the only path that attaches the
     pre-provisioned shared network. Standalone mode pauses cross-plane event
     consumers and keeps authorization fail-closed; it is not a production
     bypass.

7. **Post-review containment**
   - Static Control service credentials cannot derive delegated self identity from `X-User-Id` or call tenant-selected authz/grant routes.
   - Retrieval gRPC Create/Delete/Bulk are permanently disabled and fail before storage; the historical single-variable escape hatch is gone.
   - Model standard chat/embedding routes propagate monotonic `verified_claim.zdr OR request.zdr`.
   - Auth Core user/service Model issuance requires and signs restrictive
     `zdr:true`; no caller-selected downgrade exists.
   - Model Gateway verifies canonical service identity/reason and restricts exact
     `models:invoke` tokens to POST chat/embeddings; no user impersonation or
     delegated/Data route access is permitted.

8. **Signed asynchronous and delegation foundations**
   - The shared Rust signed-envelope verifier binds producer, audience, scopes,
     tenant, subject, payload digest, issued/expiry times, event identity/replay,
     key ID, and ZDR; its measured coverage is 92.66% regions / 94.91% lines /
     100% functions.
   - Documents commits lifecycle intent through its transactional outbox and
     publishes a signed envelope only with JetStream acknowledgement. Wiki page/
     version writes now atomically enqueue signed event intent; index deletion
     progression uses a durable signed outbox. Outbox retries use stable
     JetStream message IDs in addition to signed event identities. Supported embedding/index/graph
     consumers verify producer-scoped envelopes. Broker/database runtime proof is
     still pending where listed below.
   - Control v2 read delegation binds caller, user, tenant, method/URI,
     operation, resource, reason, restrictive ZDR, nonce, body digest, and short
     lifetime. Retrieval/documents and Control share a cross-language signature
     vector. A separately verified matching user bearer is mandatory and replayed
     nonces fail. Grant writes remain denied pending resource-owner authorization.

9. **Durable service state and admin jobs**
   - Quality evals and orchestrator jobs have additive tenant-scoped PostgreSQL
     models, organization-local idempotency, constrained transitions, and readback.
     Quality recovers pending/expired-running work across replicas. Orchestrator
     production mutations return 503 before persistence until a signed resumable
     worker exists; the insecure publisher is never used as fallback.
     Their earlier disposable-PostgreSQL lifecycle checkpoints passed; expanded
     database-backed coverage reruns remain required.
   - Quickwit admin requests have durable tenant-default job identity,
     idempotency, two-person approval, leases/heartbeats/checkpoints, append-only
     audit, preflight, and rate/concurrency bounds. Its disposable PostgreSQL
     lifecycle passed. Destructive clear remains 501 until Quickwit completion can
     be proven safely.

10. **Velion/GraphRAG identity wiring**
   - Velion derives Data tenant/user from verified session membership, mints an
     exact Data bearer for documents/retrieval/wiki/graph/source requests, and
     fails closed without a shared-key fallback.
   - Retrieval HTTP requires a strict verified bearer. Retrieval and graph mint
     short-lived org-bound `aud=inference-core` / `inference:invoke` service
     bearers and forward no shared API key to Model gRPC.
   - Control registry scopes are audience-specific, so retrieval's Control
     decision authority cannot be reused for inference.
   - Signed graph events require claim/payload tenant equality, and production
     cannot enable unsigned legacy graph mutation.

11. **Isolated HTTP/ZDR checkpoint plus final signed-posture repair**
   - A disposable Compose project built the Data service images with revision
     and creation labels, applied migrations, and passed the seven-family,
     four-shape HTTP matrix **28/28**.
   - After a successful authenticated durable-write control and a stable
     baseline, the same run matched the exact restrictive single/bulk ZDR denial
     contracts and left the document/chunk/trace/graph/source/outbox PostgreSQL
     snapshot unchanged through a ten-second delayed-write window. It used only
     random synthetic tenants/credentials and removed its containers, network,
     volumes, and generated keys.
   - The successful non-ZDR control exposed that the checkpoint bearer carried
     signed `zdr:true` while Documents ignored it. RED/GREEN source tests now
     require/preserve the boolean claim and reject signed OR body restriction.
     Final review then found the same signed-posture bypass on source-object
     upsert; its endpoint regression failed before the fix and now proves denial
     before source/outbox persistence. Documents full race/vet/build pass, but
     Docker became unavailable before these final patches could be rebuilt into
     the isolated image.
   - Startup re-verification also added bounded migrator connection retry and
     made retrieval's optional `filters` request member default safely to empty;
     focused/full regression gates pass.

## Remaining MVP work, dependency order

### P0 — Convert isolated authority/event proof into deployable authority

1. Preserve the green real Auth/User/Control browser E2E, 31-method/124-shape
   gRPC matrix, 28-shape HTTP matrix, and signed broker delivery matrix as release
   gates. The gRPC harness must remain loopback-only and exact-outcome checked.
2. Provision production subject-specific NATS publish/subscribe ACLs and the
   scoped `documents-api-gdpr` durable consumer through Control-managed secrets.
   The disposable real-consumer ACK/redelivery/health/ACL proof is green; verify
   the same contract after rollout without publishing customer identifiers or
   payloads.
3. Keep unverified Execution/Model gRPC listeners and every unsupported unsigned
   consumer disabled. Re-enable only after they implement the same verified
   principal, tenant, scope, ZDR, and audit contract.
4. Keep grant mutation/listing denied until resource-owner authorization exists;
   the real-user signed read-delegation journey is complete and must remain green.

Rollback: disable the affected listener/consumer or revoke its scoped broker
credential. Never restore shared identity headers, static user impersonation, or
the legacy unsigned-event gate.

### P0 — Complete destructive-operation safety

1. Keep the implemented durable Quickwit job/approval/lease/checkpoint/audit path
   fail-closed. Isolated authenticated preview passed; destructive execution
   remains intentionally untested and disabled.
2. Add trustworthy Quickwit task-completion verification and rebuilt-runtime
   crash/retry/concurrency proof. Retain HTTP 501 for destructive clear until then.
3. Preserve explicit break-glass scope, approval, and preview for global intent;
   never invoke it on a shared stack.

Rollback: mutation remains disabled; previews stay side-effect free.

### P0 — Cryptographically bind retention posture

1. Replace the current conservative always-restrictive Model claim only after an
   authoritative server-side organization retention policy exists. Never accept
   caller-selected downgrade.
   Until that policy can issue a signed non-restrictive Data token, Documents
   correctly rejects durable ingest rather than silently discarding the claim.
2. Keep effective Model ZDR as signed policy OR caller opt-in; extend the
   implemented async signed posture across remaining Ingestion/legacy consumers.
3. Carry the posture through HTTP, gRPC, async envelopes, callbacks, and tool execution.
4. The six-store final-state comparison is green and NATS/Dragonfly have
   monotonic no-write evidence. Add per-operation mutation telemetry/audit for
   PostgreSQL, Qdrant, Quickwit, and MinIO so an insert-then-delete cannot escape
   the proof. Extend the same strict instrumentation to Model session/cache state.

Rollback: reject restrictive requests rather than risk persistence.

### P1 — Complete runtime proof and controlled deployment

1. Rotate the locally surfaced Model shared credential and retrieval
   Control-policy service credential before any deployment.
2. Preserve the completed disposable build/runtime harnesses. They now cover
   real Auth/User/Control plus Velion, HTTP/gRPC authorization, signed broker
   delivery, and the six-store ZDR comparison with random projects and cleanup.
3. Add the strict mutation telemetry described above and retain the exact
   cross-tenant/body/header/service-principal matrices.
4. Run documents visibility/idempotency, signed broker ACL/redelivery, outbox,
   and non-destructive Quickwit crash/retry/concurrency matrices on disposable
   fixtures. Never invoke global or destructive work on the shared stack.
5. After credential rotation, broker provisioning, coverage, and telemetry gates
   pass, plan a controlled shared rebuild/deployment. Verify OCI revision/build
   labels and repeat only safe, synthetic-tenant reachability/effectiveness checks.

Rollback: stop/remove only the isolated project and its volumes. Never restart, rebuild, or clean the shared stack as part of this proof.

### P1 — Coverage, dependency, and quality gates

1. Keep the now-installed Rust coverage/advisory tools in the verification path.
   Current event-envelope coverage is 92.66% regions / 94.91% lines / 100%
   functions; Quickwit `auth.rs` is 97.06% lines and `api.rs` 86.73%, while
   `jobs.rs` remains below the gate at 52.53% until database coverage is expanded.
2. Keep RustSec at zero unignored Data/Model vulnerabilities. Continue proving
   the unfixed RSA advisory is dev/test-only and recording Data's unmaintained
   `rustls-pemfile` warning rather than silently suppressing either.
3. Keep Go auth coverage at or above the measured values in status and rerun `govulncheck` after dependency changes.
   The migrator source/toolchain remediation (pgx v5.7.4→v5.9.2 and Go build
   pin 1.26.5), Control User Core remediation (Go 1.26.5, pgx 5.9.2, gRPC
   1.79.3, quic-go 0.59.1, x/net 0.53.0), and Shipping Go/Docker 1.26.5 pin are
   green for their full test/vet/build/scans. The Data migrator image has now
   built and run in the disposable project; keep other rebuilds isolated before
   any deployment decision.
4. Keep Control pnpm/npm production audits at zero known advisories and rerun the 41 policy/token tests after every auth dependency change.
5. Raise database-backed coverage for quality eval (current source-only profile
   26.7%; recovery 69.2%), orchestrator jobs (31.9%), Quickwit jobs (52.53%
   lines), and wiki
   `internal/events` (28.7% full race profile) with database-backed fixtures. Preserve
   the already green auth packages and focused pure outbox/PubAck tests.
6. Keep Auth Core's full no-fix lint/build/frozen-install gate and 99-test suite
   green. Current focused security coverage is 90.00% statements / 90.04%
   branches / 94.73% functions / 89.77% lines; full and production pnpm audits
   report zero advisories.
7. Keep Quarry's remediated dependency graph green. Its fixable RustSec paths
   were upgraded; runtime/edge tests, strict clippy, and the production audit
   pass. RSA must remain absent from all normal/build graphs; record the remaining
   unmaintained dependency warnings without treating them as fixed.

## Documentation and release gate

The isolated HTTP, gRPC, real-authority browser, broker, and multi-store ZDR
matrices are complete and recorded with sanitized evidence. The final review's
matrix-safety findings are fixed and regression-tested. Remaining documentation
must continue to distinguish this isolated proof from scoped-broker provisioning,
shared deployment, strict mutation telemetry, and post-deploy effectiveness.

Production-ready may be declared only when every A–L row is `Proven` rather than `Partial`, `Contained`, or `Pending`.

## Enterprise-next — not started

Do not schedule or mix these into the MVP unless a specific item is necessary to close an MVP security defect:

- workload identity/mTLS and automated rotation;
- formal policy engine/ABAC and delegated administration;
- HA/DR, multi-region residency, backup/restore drills;
- zero-downtime migrations and online rebuild orchestration;
- formal threat model/compliance evidence/retention attestations;
- SLOs, alerting, capacity/load/chaos programs;
- customer-managed keys, legal holds, advanced classification, enterprise audit export.
