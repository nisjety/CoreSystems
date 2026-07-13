# Data Plane v2 — Secure-MVP Roadmap

Updated: 2026-07-11. Read `DATA_PLANE_STATUS.md` first.

Only the secure MVP is active. The enterprise phase remains explicitly unopened until criteria A–L are all proven on rebuilt, deployed images.

## Completed source remediations

1. **Anonymous/header-only containment**
   - Graph, quality, orchestrator, documents, retrieval, wiki, and Quickwit sensitive routes require verified JWTs or explicitly scoped service principals.
   - Tenant and user scope come from verified claims. Conflicting headers/body/path values are rejected.
   - Quickwit admin mutation is fail-closed; preview is authenticated and tenant-scoped.

2. **Documents privacy and ZDR**
   - Signature, issuer, audience, time, identity, and JWKS validation default strict and startup validation fails closed.
   - Service reads retain service identity/visibility unless an explicit org-wide scope exists.
   - Idempotency cannot cross owner boundaries.
   - Single and bulk durable ingest share the restrictive ZDR guard.

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
   - Unary/stream Model paths suppress session, event, idempotency, response-cache, semantic-cache, and publisher effects under ZDR. Direct document/bulk/retrieval/wiki proxies enforce the same monotonic signed posture, and restrictive retrieval skips retaining rerank/embedding egress.
   - Quarry single/bulk/CAS/event paths use one durable-persistence guard.

5. **Secure-default containment**
   - Unsigned mutation consumers in documents, graph, Quickwit, orchestrator, embedding, and index are disabled unless two explicit insecure-development gates are set.
   - Unverified Model Gateway, Execution Core, and Inference Core production
     listener constructors are removed/test-confined; legacy Compose host ports
     are removed.

6. **Compose and provenance**
   - Data services and infrastructure have no host-published ports in the default profile.
   - Credentials are required, internal stores stay on the private Data network, and insecure gates default off.
   - Data verifiers mount only Control's public key file, never the signing-key directory.
   - Control diagnostics default off; optional self-owned dependencies have no host publications/default credentials.
   - Model and Ingestion production overrides reset all base-published ports,
     disable dev bypasses, and require credentials; merged configs validate.
   - Dockerfiles/Compose carry OCI revision/build-label contracts and require
     `SOURCE_REVISION` and `BUILD_DATE`. Earlier image inspection predates later
     source changes; current images still require a clean rebuild/inspection.

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
     coverage reruns are blocked by Docker's storage failure.
   - Quickwit admin requests have durable tenant-default job identity,
     idempotency, two-person approval, leases/heartbeats/checkpoints, append-only
     audit, preflight, and rate/concurrency bounds. Its disposable PostgreSQL
     lifecycle passed. Destructive clear remains 501 until Quickwit completion can
     be proven safely.

## Remaining MVP work, dependency order

### P0 — Restore functionality with verified machine/event identity

1. Apply subject-specific NATS publish/subscribe ACLs and prove the implemented
   signed envelope on rebuilt images with wrong-producer, wrong-tenant, unsigned,
   replay, expired, digest, subject, and ZDR cases.
2. Run the documents/wiki/index transactional-outbox tests that require
   disposable PostgreSQL/JetStream, then prove signed PubAck/redelivery through
   embedding/index/graph. Keep unsupported legacy Quickwit/orchestrator/GDPR
   consumers disabled until they use the same contract.
3. Introduce a dedicated inference audience and per-caller scopes; verify RS256/JWKS in Inference Core and pin org/user from claims. Update retrieval/graph/embedding callers to Bearer only.
4. Add verified run ownership/scopes to Execution Core and every pause/resume/cancel/execute RPC, then re-enable its listener.
5. Apply the same verified principal contract to Model Gateway gRPC before re-enabling it.
6. Exercise the implemented signed Control plus original-user proof delegation through a real
   authorized-user isolated E2E. Define resource-owner authorization before any
   grant mutation/listing is restored.

Containment rollback: if any verified replacement regresses, keep the corresponding listener/consumer disabled. Never fall back to the legacy single gate or shared identity headers.

### P0 — Complete destructive-operation safety

1. Keep the implemented durable Quickwit job/approval/lease/checkpoint/audit path
   fail-closed while images and isolated runtime verification are blocked.
2. Add trustworthy Quickwit task-completion verification and rebuilt-runtime
   crash/retry/concurrency proof. Retain HTTP 501 for destructive clear until then.
3. Preserve explicit break-glass scope, approval, and preview for global intent;
   never invoke it on a shared stack.

Rollback: mutation remains disabled; previews stay side-effect free.

### P0 — Cryptographically bind retention posture

1. Replace the current conservative always-restrictive Model claim only after an
   authoritative server-side organization retention policy exists. Never accept
   caller-selected downgrade.
2. Keep effective Model ZDR as signed policy OR caller opt-in; extend the
   implemented async signed posture across remaining Ingestion/legacy consumers.
3. Carry the posture through HTTP, gRPC, async envelopes, callbacks, and tool execution.
4. Run a spy-backed and storage-backed zero-write test across Postgres, Qdrant, Redis/Dragonfly, Quickwit/MinIO, graph, trace, event payloads, and Model session/cache systems.

Rollback: reject restrictive requests rather than risk persistence.

### P1 — Build and isolated runtime proof

1. Rotate the locally exposed shared internal credential before any deployment.
2. Repair or restart Docker only with explicit operational authorization. Current
   `docker system df` fails with a containerd blob `input/output error`; 99
   containers/86 running were observed, so an engine restart is not a plane-local
   action and could disrupt shared workloads.
3. Rebuild all affected images from the current source with explicit revision/
   build date and verify OCI labels. Earlier image evidence predates later source
   changes and is not current build proof.
4. Start a separate Compose project with separate volumes, networks/aliases, test credentials, and an isolated Control organization. Do not attach it ambiguously to the shared Data service aliases.
5. Apply migrations twice, seed only disposable fixtures through supported APIs, and run:
   - read-only health smoke;
   - 7-family × 4-shape HTTP auth matrix;
   - equivalent gRPC matrices for every enabled service;
   - documents visibility/idempotency matrix;
   - restrictive ZDR zero-persistence matrix;
   - Quickwit preview/admin denial matrix.
6. Clean up only the disposable project/fixtures created by this program.

Rollback: stop/remove only the isolated project and its volumes. Never restart, rebuild, or clean the shared stack as part of this proof.

### P1 — Coverage, dependency, and quality gates

1. Keep the now-installed Rust coverage/advisory tools in the verification path.
   Current event-envelope coverage is 92.66% regions / 94.91% lines / 100%
   functions; Quickwit `auth.rs` is 97.06% lines and `api.rs` 86.73%, while
   `jobs.rs` remains below the gate at 52.53% until database coverage can run.
2. Keep RustSec at zero unignored Data/Model vulnerabilities. Continue proving
   the unfixed RSA advisory is dev/test-only and recording Data's unmaintained
   `rustls-pemfile` warning rather than silently suppressing either.
3. Keep Go auth coverage at or above the measured values in status and rerun `govulncheck` after dependency changes.
   The migrator source/toolchain remediation (pgx v5.7.4→v5.9.2 and Go build
   pin 1.26.5), Control User Core remediation (Go 1.26.5, pgx 5.9.2, gRPC
   1.79.3, quic-go 0.59.1, x/net 0.53.0), and Shipping Go/Docker 1.26.5 pin are
   green for their full test/vet/build/scans. Rebuild their images when Docker is
   restored.
4. Keep Control pnpm/npm production audits at zero known advisories and rerun the 41 policy/token tests after every auth dependency change.
5. Raise database-backed coverage for quality eval (current source-only profile
   26.7%; recovery 69.2%), orchestrator jobs (31.9%), Quickwit jobs (52.53%
   lines), and wiki
   `internal/events` (28.7% full race profile) after Docker is available. Preserve
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

After the isolated matrix passes, update status/audit/per-service docs with exact image IDs, source revision, sanitized commands, counts of tests (not customer records), timestamps, and expected/actual status codes. Then perform a final security review.

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
