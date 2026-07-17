# Control Plane — Current Status

Last verified: 2026-07-16 (continuation of the 2026-07-10 production-readiness audit)

## Decision

**The remaining secure-MVP engineering gates are implemented and pass source, static, embedded-broker, disposable-Postgres, and isolated-current-image checks; production release acceptance is still withheld pending an operator-owned integration credential rollout.** Org, Billing, Audit, Session, and User privileged paths use audience/scope-bound principals in current source. Better Auth's invitation-status gap has a durable, tombstone-aware repair ledger; historical owner repair is report-and-stop with an explicit reviewed mapping; membership invite/role/remove evidence is append-only and revision ordered; critical identity/organization/billing producers use durable outbox/PubAck paths; and runtime NATS principals cannot administer topology. The critical Session impersonation and User caller-asserted role/profile paths remain closed.

The Velion v3 continuation repaired the previously missing OAuth/OIDC/SAML callback proxy, canonical invitation acceptance, real Auth-owned organization list/switch, Auth-owned membership invite/remove/role changes, Nexi checkout selection/status handling, and Audit Core row normalization. Auth Core now generates invitation links from the public frontend origin and issues mandatory issuer-selected ZDR claims for interactive users. Caller fields cannot downgrade retention; a non-ZDR service claim requires an exact deployment-owned service/audience policy and a durable issuance-audit PubAck.

Independent code/security review then closed five additional boundary defects: organization completion state no longer leaks across workspace switches; invitation acceptance is Auth-owned, executes through Better Auth's trusted-origin/rate-limited HTTP router, normalizes enumeration-prone failures, idempotently recovers a committed lost response only from invitation/user/email/membership-bound evidence, explicitly selects the accepted organization, and bypasses Better Auth's session cookie cache; gateway/release-nginx access logs are path-only and normalize invitation IDs while reset tokens are immediately removed from browser history; gateway transport, translation, SSE, and WebSocket errors no longer expose internal URLs or upstream bodies; and billing return URLs are derived from the configured Velion origin. The invitation wrapper overwrites every Better Auth IP-precedence header with a verified-actor-derived 120-bit HMAC address, uses an atomic Dragonfly increment/expiry operation, and is the only path that can mint the invitation-bound 30-second HMAC marker required by the canonical Better Auth mutation. Dragonfly read/write failure propagates as an operational failure rather than an unauthenticated cache miss. Better Auth 1.6.23 still performs the status change before its member transaction, so migration 017 now records observed acceptance intent and durably reconciles both partial shapes with cancel/delete tombstones. Production public origins must be canonical HTTPS origins, executable checkout URLs are provider-pinned, unsupported Hyperswitch is fail-closed, and Nexi payment IDs/provider errors are bounded and sanitized.

Auth's full lint command now enforces an exact reviewed legacy-debt ratchet while checking every changed TypeScript file without suppressions. The Velion gateway has an executable per-module Rust coverage gate above 80% for all five selected security modules. The only remaining acceptance work is operational: inject and rotate pairwise-distinct credentials in an integration deployment, roll provisioner -> consumers -> producers, prove health/PubAck/lag/outbox convergence and the authenticated/unauthenticated matrix from reviewed images, then revoke the legacy bridge token. A bounded check of the pre-existing shared local stack on 2026-07-15 found it degraded/restarting because its `.env` supplied only 8/68 required credential/file inputs; no secrets were generated or changed.

The enterprise-readiness phase is intentionally deferred until the operator-owned production acceptance step is green.

## Service-local development environment — 2026-07-16

The Control Plane no longer has a root `.env` or `.env.example`. Each of the six
cores owns its own ignored `.env` and tracked `.env.example`; the base Compose
file loads those files (plus Docker-hostname overrides where required). Use
`./scripts/run-control-plane.sh ...` for local Compose commands. The runner
merges the six service-local files and creates a temporary `0600` interpolation
file containing only disposable development defaults for values that are not
provided locally; it deletes that file on exit and refuses the production
overlay. Production still requires the external secret-manager/file-backed
inputs in `docker-compose.production.yml` and must not use generated defaults.

### Local Docker rebuild evidence — 2026-07-16

The supported local run was rebuilt without resetting or pruning any database
or Docker volume:

```bash
./scripts/run-control-plane.sh up -d --build
./scripts/run-control-plane.sh up -d
```

All eight local Control images built successfully. The first start created the
missing external `inter-plane-bus` bridge, then the existing Postgres role was
aligned with the rotated service-local development credential (data and schema
were preserved). Final container state is Auth, User, Org, Billing, and
Session **healthy**. Audit Core is Docker-**healthy** via its local `/healthz`
probe and serving HTTP, while `/readyz` returns **503** because this
Control-only project has no `model-nats` or
`application-nats` endpoints for its configured extra-plane consumers. This is
an honest cross-plane dependency gap, not a readiness bypass.

Observed local probes: Auth JWKS `200` (436 bytes), Org health `200`, Billing
health `200`, Session health `200`, missing/forged Session requests `401`,
Audit `/healthz` `200`, and Audit `/readyz` `503` (1,142-byte diagnostic).
User Core has no host-published
HTTP port in this Compose file and was verified healthy through Docker health.
No tenant lifecycle operation, volume reset, database deletion, or production
overlay was performed.

## Final secure-MVP engineering acceptance — 2026-07-15

The Control Plane secure-MVP source and isolated acceptance matrix is green. This is not a production deployment certificate: real secret generation, secret-manager injection, coordinated rotation, deployment, and post-deploy verification require an operator with that authority.

- The first current-image run failed before `/api/convex-auth/jwks` became ready. Bounded, redacted logs safely identified a production startup dependency on legacy internal-key configuration. Auth now requires file-backed scoped gRPC/internal registries, validates a matching RSA private/public pair with a minimum 2048-bit modulus, and exposes a structural one-key `RS256` JWKS readiness check. Missing, unreadable, mismatched, EC, and weak keys fail closed. The rebuilt production-mode image reached JWKS readiness.
- After swallowed initialization errors were removed, the fresh production image exposed a second readiness blocker: the User proto was resolved from `/app/dist/src/proto` even though the runtime image contains `/app/proto`. A failing regression fixed the path to `/app/proto`; the rebuilt image reached JWKS readiness.
- A real Nest gRPC transport matrix passes 9/9 cases: missing, wrong, ambiguous, retired, and wrong-principal credentials are unauthenticated; insufficient scope is permission denied; bounded old/new overlap is accepted. Auth internal HTTP/NATS and reciprocal Auth↔User clients use exact credential ID, principal, audience, token, scope/method tuples. Production User verifies delegated proofs with the same mounted Auth public-key secret and a required non-localhost issuer.
- Auth→User gRPC now uses CA-pinned TLS in production; User Core requires a certificate/key pair, TLS 1.3 minimum, and bounded certificate validation. The transport helper is 100% covered, the real Auth/User TLS integration passes, and a plaintext channel is rejected.
- The final fresh-image Control runner passed 4/4 phases after two fixture races were reproduced test-first: asynchronous outbox insertion and Billing's correct `organization.created` auto-provisioning. The fixture now waits for durable state, permits bounded retry with stable event IDs, and seeds Billing idempotently. It proved invitation acceptance/repair, Auth→Org convergence, membership retry/role/remove/audit ordering, Billing-down deletion checkpointing, restart/resume, tombstones, and delayed-resurrection rejection. Cleanup left zero matching containers, images, networks, volumes, or temporary credentials.
- The five Go services pass full `go test ./...` and `go vet ./...`. Auth passes Nest build, 39 active suites/372 active tests (6 suites/35 tests intentionally skipped), a 135-file lint ratchet, and 6/6 lint-contract tests. Eighty-four changed TypeScript files are unsuppressed; the reviewed legacy baseline is 212 violations in seven untouched files. Production dependency audit reports no known vulnerabilities.
- The non-printing release preflight validates 58 pairwise-distinct credentials and 10 bounded credential/key/TLS files; three valid profiles and 20 fail-closed cases pass. The production Compose render has 22 services, zero host-published ports, zero host-network services, and no development `env_file` on the six Control services.
- Each of the six Control cores now has an independent `.env` and tracked `.env.example`. Compose layers `./<core>/.env` before the Docker-specific override for Auth, User, Org, Billing, and Session, and loads Audit's own `.env`; the production override resets all six `env_file` entries and requires secret-manager inputs instead. Local `.env` files are ignored and the three newly created files are `0600` templates with no production values.
- Velion/Data real-authority proof passes 31 gRPC methods across four auth shapes (124 checks), 28/28 HTTP checks, real Auth/User integration, and 2/2 browser E2E cases. Gateway Rust evidence remains 288 tests with all selected security modules above 80%.

The only remaining MVP gate is execution by the deployment authority: create and inject the real secret versions, run the preflight without printing values, deploy registries/servers before clients and consumers before producers, prove health/auth denial/PubAck/lag/outbox convergence from the reviewed image digests, then revoke old credentials. Rollback keeps the old scoped credentials valid during overlap or pauses producers while durable outboxes are retained; it never restores a shared fallback token. File-backed Compose secrets are handed off from a root-only entrypoint to app-owned `0600` files before the services drop to `appuser`.

## Shared local deployment state — 2026-07-15 (not release evidence)

The pre-existing shared project was inspected without changing its volumes, tenant rows, or secret files. `docker compose ls --all` reported `control-plane restarting(4), running(14)`. The restart loops were `controlplane-nats` (missing `AUTH_NATS_PASSWORD`), `user-service` (NATS DNS/auth failure because `controlplane-nats` was down), and `audit-core-service` (JetStream inspect timeout). The ignored `.env` contained only 8 of 68 required credential/file inputs. No real secret values were generated, read, printed, rotated, or written. This shared project must not be called production-ready until the external secret/deployment authority provisions it.

## 2026-07-15 GDPR fanout durability closure

The previously single-org, core-NATS GDPR fanout is now a durable cross-plane
protocol in current source. Migration 016 snapshots every active User Core
organization in the same transaction that creates the erasure operation and
before Auth or local cleanup can run. Each organization receives one bounded,
deterministic child ID. The parent cannot set `fanout_published_at` or
`completed_at` until every child has a valid `AQENCIA_CONTROLPLANE` JetStream
PubAck persisted; retry and terminal state remain per child. Audit and fanout
terminal counts plus oldest lag are exposed in User health, and a verified-admin
operator route requeues at most 100 selected terminal rows without deleting
their payload, error, org snapshot, or requeue history. Published and terminal
GDPR audit evidence is retained rather than purged.

Documents API now binds the deployment-provisioned
`documents-api-gdpr-erasure-v1` durable using a dedicated user/password
principal. It validates the strict child envelope, ACKs only after ownership
transfer and receipt publication, NAKs transient failures, and writes poison or
exhausted deliveries to `velion.gdpr.erasure.dlq.documents-api` with a valid
PubAck before ACKing the source. Consumer pending, ACK-pending, redelivery,
terminal, success, error, and lag state is exposed at
`/internal/gdpr/health`. The principal cannot publish erasure requests or
administer JetStream. `velion.gdpr.*` is explicitly excluded from the legacy
token bridge.

Evidence: isolated tmpfs PostgreSQL tests passed for pre-mutation multi-org
snapshot, partial-PubAck incompletion, retry/resume, terminal evidence, health,
and bounded requeue. User changed GDPR files measured 80.8% statement coverage
(282/349). Documents GDPR tests, including embedded JetStream durable binding,
ACK and DLQ ordering, pass with 84.2% package coverage. Audit provisioner
coverage is 83.2%; its GDPR consumer config is 100%, and the legacy forwarder is
81.0%. Full serialized Go tests, `go vet`, and changed-package race tests pass
for User Core, Documents API, and Audit Core. Fresh scoped-broker ACL tests prove
the positive delivery/ACK/DLQ path and denial of request forgery and topology
administration. Control and Data Compose renders pass with injected placeholder
validation values, and the non-printing credential preflight now covers 58
pairwise-distinct credentials plus 10 bounded credential/registry/key/TLS files.

This is source/isolated evidence, not a live deployment claim. The new
`DOCUMENTS_GDPR_NATS_PASSWORD` must be coordinated between the Control broker
and Data workload; provisioner -> Documents consumer -> User producer rollout,
live health/lag/DLQ proof, and secret rotation remain part of the single
operator-owned integration gate.

## Current release blockers

1. Run the non-printing credential preflight with integration secret-manager values, deploy the scoped broker and one-shot provisioner, then roll consumers and producers with runtime token fallback disabled.
2. Re-run Docker health, direct/gateway denial, authenticated lifecycle, broker ACL, lag/redelivery, and outbox/dead-letter checks from images built from the reviewed worktree. Revoke the legacy bridge token only after non-Control consumer owners confirm zero dependency.

## Deployment state

| Component | Live result | Evidence |
|---|---|---|
| Docker runtime/infrastructure | **Local stack running; cross-plane Audit readiness pending** | On 2026-07-16 the service-local runner rebuilt all eight images and started the stack without data reset. Auth/User/Org/Billing/Session are healthy; Audit `/readyz` is 503 until external Model/Application NATS endpoints are present. This is not release evidence. |
| auth-core | **Source + isolated DB/container verified; shared dev image restarting** | Migrations 017-027 cover invitation repair, reviewed owner preflight, revisioned membership/audit intent, identity-event and GDPR outboxes, and Better Auth adapter compatibility without rewriting bootstrap history. Full build, 39 active suites/372 tests, and the 135-file lint ratchet pass. Auth→User TLS and the root-to-appuser secret handoff are contract-tested. |
| org-core | **Healthy** | Current live counts: 1 Auth organization/member and 1 Org projection/member; projection-version counts are both 1. |
| user-core | **Source + isolated GDPR durability verified; existing dev image older** | Gateway→User self-service uses audience-bound signed delegation. Multi-org erasure recipients are durably snapshotted before cleanup and require per-child PubAck; terminal evidence/lag and bounded recovery are implemented. No raw browser identity header reaches User Core. |
| billing-core | **Source + isolated DB/container verified; existing dev image older** | Scoped callers are pairwise distinct; org deletion uses port 3014; monotonic plan revisions and permanent tombstones reject duplicate, reordered, and delayed resurrection events. No live checkout was started. |
| session-core | **Healthy** | Missing and forged direct bootstrap requests return 401; gateway requests do the same. |
| audit-core | **Docker-healthy; local cross-plane readiness pending** | `/healthz` is healthy from the Control DB/NATS dependencies and does not gate on optional planes. Current source/isolated image still proves Control/Model/Application buses ready, scoped events persisted, producer ACL denial, and unauthenticated HTTP 401. In the 2026-07-16 Control-only run, `model-nats` and `application-nats` were absent, so `/readyz` correctly returned 503 while the supervisor retried attachment. |
| Velion v3 gateway/SPA | **Healthy; both rebuilt from current worktree** | Gateway `/health` and SPA `/` return 200. Callback, org, membership, billing, session, logging, and audit contract tests pass. The local SPA is the dev target; release nginx syntax is separately verified. |
| Cross-plane policy | **Implemented and isolated-tested; coordinated rollout pending** | Org/Billing/Audit HTTP principals and Control/Model/Application NATS users are audience/scope/subject bounded. Runtime services have no stream-admin rights and release producers disable token fallback; only the temporary compatibility bridge retains the legacy token. Critical identity/plan publications use stable-ID outbox/PubAck paths. |

## MVP acceptance matrix

| Criterion | Status | Verified state |
|---|---|---|
| A. Session fail-closed | **Pass** | Live Session Core: missing bearer, malformed bearer, and forged JWT plus `X-User-Id`/`X-Org-Id`/`X-User-Role` all return 401 and do not echo the fixture identity. Gateway bootstrap gives the same result. Unit regressions cover missing, malformed, expiry, issuer, audience, signature, and header impersonation cases. |
| B. Scoped internal auth | **Source/isolation pass; rollout pending** | Verified-context user/admin/profile claims remain enforced. Org/Billing/Audit plus Auth internal/gRPC and User gRPC registries require exact audience/scope/method-bound principals and reject placeholder, legacy, ambiguous, retired, wrong-principal, and pairwise-reused values. Production User shares Auth's public-key secret and requires an explicit issuer. |
| C. Auth-to-Org convergence | **Isolated container pass; integration rollout pending** | A fresh Auth/Org/Billing image stack proved invitation repair -> migration 015 -> Auth outbox -> Org convergence, retry/reordering, deletion checkpoint/resume, and tombstone protection. The existing dev data remains 1=1 and was not mutated. |
| D. RLS GUC contract | **Pass** | Org migrations 010-013 are live. Policy inventory is `app.current_org_id=0`, `app.current_org=14`; functional and rollback migration tests pass. |
| E. Canonical membership lifecycle | **Source + isolated Postgres/current-image pass** | Auth remains canonical for list/invite/accept/remove/role change. Migrations 017/019 durably repair partial acceptance and transactionally record revision-ordered invitation/member audit intent. The fresh-image runner proves duplicate invitation, same-role replay, removal replay, exact audit cardinality, and Auth-to-Org convergence. No real tenant membership was mutated. |
| F. Deletion and billing tombstones | **Isolated pass** | Auth uses billing port 3014, checkpoints Billing and Org independently, and resumes after Billing restart. Real Postgres and current-image container tests prove monotonic plan revisions, durable retry/DLQ, permanent cancellation tombstones, and delayed-resurrection rejection. No real tenant was deleted. |
| G. Audit SQL and aggregation | **Current-image isolated pass; rollout pending** | Corrected dashboard SQL remains covered. Three isolated scoped NATS buses were ready; Model/Application events produced 2 audit + 2 usage rows, ACL-negative publish was denied, and readiness/lag state was visible. Existing dev still runs an older image/config. |
| H. Tests/build/static/live matrix | **Source/isolation pass; integration deployment pending** | The five Go Control services pass full test/vet and retain earlier race evidence. Auth build, 39 active suites/372 tests, full lint ratchet, and lint contract pass. Gateway format/test/check/strict-Clippy passes with 288 tests and five security files above 80%. SPA lint/typecheck/test/build passes (68 files/358 tests, one existing warning). Seven-phase DB lifecycle and four-phase fresh-image container lifecycle pass. |
| I. Evidence docs | **Pass for this continuation** | Status, roadmap, plane audit, and all six service research docs record 2026-07-15 source-tested/isolated-live/existing-dev distinctions. No legacy docs were deleted. |

## Exact live evidence

### 2026-07-15 isolated current-image evidence

The destructive cases ran only in uniquely named, disposable containers/databases with no host ports or persistent volumes. Cleanup traps removed the fixtures. The long-running development stack and its tenant rows were not changed.

```text
scripts/run-isolated-control-lifecycle-e2e.sh
  7/7 phases passed: invitation repair; deletion checkpoint/resume; Org ordering/outbox/tombstone;
  scoped Org HTTP; Billing revision/tombstone; durable retry/DLQ; scoped Billing HTTP.

scripts/run-isolated-control-container-e2e.sh
  4/4 phases passed: real invitation + Auth outbox -> Org convergence; membership invite/role/remove
  retry and exact audit ordering; Billing-down partial deletion; Billing restart/retry plus
  delayed-resurrection rejection. Auth migrations through 027 applied in every phase.
  Production-mode Auth reached a structurally valid one-key RSA/RS256 JWKS first.

isolated Audit current image
  readiness: control/model/application ready
  persisted: audit=2 usage=2
  audit principal producer publish: denied
  unauthenticated Audit HTTP: 401

release compose render
  22 services; 0 host-published ports; 0 host-network services
  broker copies receive syntax-quoted generated passwords; clients receive the exact raw value
```

Coverage measured on changed critical paths: invitation repair 89.47% statements/81.35% branches/100% functions/90% lines; Auth identity outbox 100% statements/lines/functions and 92.3% branches; Audit subscriber package 96.1%; Org `UpdatePlanWithOutbox` 81.8% and `FlushPlanChangeOutbox` 84.2%; Billing revision/tombstone/deactivation functions 81.6%/85.7%/90.0%; gateway selected security files 81.55%-98.82%. These figures do not imply 80% whole-repository coverage.

### 2026-07-14 existing-development-stack evidence

2026-07-14 continuation. These requests are non-mutating; no real invitation, membership, billing, role, or deletion transition was exercised.

```bash
# Final boundary rebuild from the current worktree:
SOURCE_REVISION=6b4a42c967890c85271497af31dd5ee4484617fa \
BUILD_DATE=2026-07-14T20:32:02Z \
  ./scripts/run-control-plane.sh up -d --build auth-core
cd "../Frontend Plane/velionv3"
SOURCE_REVISION=6b4a42c967890c85271497af31dd5ee4484617fa \
BUILD_DATE=2026-07-14T20:32:02Z \
  # Gateway is outside the Control Plane runner; use its own Compose project.
  docker compose up -d --build gateway

docker compose ps auth-core user-core org-core billing-core session-core audit-core
docker compose ps gateway frontend
# all eight application containers healthy

curl http://127.0.0.1:3185/health
curl http://127.0.0.1:5173/
# 200, 200

curl http://127.0.0.1:3185/api/v1/session/bootstrap
curl -H 'Authorization: Bearer garbage' \
  -H 'X-User-Id: forged-user' -H 'X-Org-Id: forged-org' \
  -H 'X-User-Role: admin' \
  http://127.0.0.1:3185/api/v1/session/bootstrap
# 401, 401

curl -H 'Authorization: Bearer garbage' \
  -H 'X-User-Id: forged-user' -H 'X-Org-Id: forged-org' \
  -H 'X-User-Role: admin' \
  http://127.0.0.1:3017/api/v1/sessions/current
# 401

curl -X POST \
  http://127.0.0.1:3185/api/v1/orgs/invitations/fake-invitation/accept
curl -X POST -H 'Origin: http://localhost:5173' \
  http://127.0.0.1:3011/api/v1/organization/invitations/fake-invitation/accept
# 401, 401

curl -X POST -H 'Content-Type: application/json' \
  -H 'Origin: http://127.0.0.1:5173' \
  --data '{"invitationId":"fake"}' \
  http://127.0.0.1:3011/api/auth/organization/accept-invitation
# 400; the canonical mutation cannot bypass the signed wrapper marker

curl 'http://127.0.0.1:5173/api/auth/callback/google?code=garbage&state=garbage'
# 302 -> http://localhost:5173/api/auth/error?error=state_mismatch

# Fake callback/reset/invitation markers and a fake email query were sent, then
# container logs were searched without printing their contents:
# sensitive_marker_seen=no

curl http://127.0.0.1:8187/readyz
# 200; database_connected=true; primary_nats_connected=true;
# extra_nats_connected=[]; delivery_mode=jetstream_durable

# Read-only database counts:
# Auth organizations=1, Auth memberships=1
# Org organizations=1, Org memberships=1
# Org organization projection versions=1, membership projection versions=1
```

The Docker storage incident below is retained as historical evidence. It was not reproduced on 2026-07-14 and is no longer the active blocker.

The first block was captured before the Docker storage incident and remains evidence for the deployed fixes at that time, not the current stack health. No tenant membership, invitation, billing, or deletion mutation was performed.

```bash
docker ps --filter label=com.docker.compose.project=control-plane \
  --format '{{.Names}}|{{.Status}}'
# auth-service, org-core-service, user-service, billing-core-service,
# session-core-service, audit-core-service: all healthy

curl -H 'X-User-Id: <existing-user>' \
  http://127.0.0.1:3017/api/v1/sessions/current
# 401; identity_leak=no

curl -H 'Authorization: Bearer garbage' \
  -H 'X-User-Id: <existing-user>' -H 'X-Org-Id: forged-org' \
  -H 'X-User-Role: admin' \
  http://127.0.0.1:3017/api/v1/sessions/current
# 401; identity_leak=no

curl -H 'Authorization: Bearer garbage' \
  -H 'X-User-Id: <existing-user>' -H 'X-User-Role: admin' \
  http://127.0.0.1:3185/api/v1/session/bootstrap
# 401; identity_leak=no

curl -H 'X-Internal-Api-Key: <legacy-key>' \
  -H 'X-User-Id: <existing-user>' -H 'X-User-Role: admin' \
  http://127.0.0.1:3012/api/v1/users/me
# 401; identity_leak=no

curl http://127.0.0.1:8187/readyz
# 200; database_connected=true; primary_nats_connected=true;
# extra_nats_connected=[true]; delivery_mode=jetstream_durable

curl -H 'X-Internal-Api-Key: <redacted>' \
  'http://127.0.0.1:8187/v1/usage/summary?org_id=<fixture-org>'
# 200; rows=0 in the selected safe fixture; SQL executed on the live schema

curl -X PATCH -H 'X-Internal-Api-Key: <redacted>' \
  -H 'content-type: application/json' --data '{"role":"admin"}' \
  http://127.0.0.1:18080/orgs/<fixture>/members/<fixture-user>/role
# 404; competing Org membership/role mutation route is not mounted

# Valid gateway service token but no signed self-delegation envelope:
curl -H 'X-Service-Token: <gateway-token>' \
  -H 'X-User-Id: <fixture-user>' http://user-core:3012/api/v1/users/me
# 403
```

Historical 2026-07-11 incident evidence:

```text
gateway Docker build:
failed to open cached Rust rlib: input/output error (os error 5)
failed to write /var/lib/docker/buildkit/containerd-overlayfs/metadata_v2.db

docker inspect controlplane-postgres:
exited|open /var/lib/docker/containers/<redacted>/hosts: input/output error|139

docker logs controlplane-postgres / audit-core-service:
open ...-json.log: input/output error

docker ps after final recreate:
user-service=unhealthy; session-core-service=unhealthy;
audit-core-service=starting; velion-gateway-rs=unhealthy
```

Projection reconciliation evidence after the Auth redeploy:

```text
Before BIGINT fix: projection total=5, published=0, pending=5, HTTP-400 errors=3
After BIGINT fix:  projection total=5, published=3, pending=2, errors=0
Membership:        total=3, synced=3, pending=0
Cross-database IDs: Auth=5, Org=3, missing=2, extra=0
Pending cause:     owner_user_id NULL=2; canonical ownerless organizations=2
```

## Verification and measured coverage

| Target | Tests/static result | Measured statement coverage |
|---|---|---|
| auth-core | 39 active suites/372 tests pass (6 suites/35 environment-gated in the non-Docker run); Nest build, full 135-file lint ratchet, and 6/6 lint-contract tests pass. The exact 212-violation legacy baseline covers seven untouched files; 84 changed files use an empty suppression file. | Invitation repair: 89.47% statements/81.35% branches/100% functions/90% lines. Identity outbox: 100% statements/lines/functions, 92.3% branches. New scoped-auth/key helpers: 88.23% statements/83.2% branches/91.3% lines. Auth→User TLS transport helper: 100% statements/branches/functions/lines. |
| session-core | `go test ./...` and `go vet ./...` pass | Whole service 20.2%; `internal/internalkey` 100%; HTTP package 38.0% |
| user-core | `go test ./...` and `go vet ./...` pass | Whole service 3.8%; `internal/authztaxonomy` 93.3%; HTTP package 12.3% |
| org-core | `go test ./...`, `go vet ./...`, and the earlier race run pass | `UpdatePlanWithOutbox` 81.8%; `FlushPlanChangeOutbox` 82.1%. Older projection functions remain below 80% and are not represented as covered. |
| billing-core | `go test ./...`, `go vet ./...`, and `go test -race ./...` pass | `ApplyOrganizationPlanRevision` 81.6%; `TombstoneOrganization` 85.7%; `DeactivateOrganization` 90.0%. |
| audit-core | `go test ./...`, `go vet ./...`, `go test -race ./...`, embedded scoped-ACL JetStream tests, and isolated three-bus Docker proof pass | Subscriber package 96.1%; `Start` 91.7%; audit/usage handlers 96.7%; consumer health 84.6%. `cmd/server` overall is 62.2% because main/retention are not covered, while changed extra-bus paths exceed 80%. |
| Velion v3 SPA | ESLint (0 errors, 1 pre-existing warning), typecheck, production build, 68 files/358 tests pass | Existing focused changed-module measurements remain recorded below; no whole-SPA percentage is claimed. |
| Velion gateway | format, debug/release workspace checks, strict Clippy (`-D warnings`), all 288 Rust tests, and 4/4 coverage-gate contract tests pass | audience tokens 98.82%; config 90.63%; membership boundary 89.62%; middleware 87.32%; upstream 81.55%. |

`git diff --check` passes. Generated coverage output was removed after recording the summary; no coverage artifacts were added to the repository.

## Release blockers

1. Deploy and rotate the scoped HTTP/NATS credentials in a coordinated integration stack; the current evidence is source plus isolated-current-image, not the older long-running dev containers or production.
2. From those reviewed images and rotated credentials, rerun health/auth-denial, lifecycle, broker ACL, lag/redelivery, outbox/dead-letter, and bridge-drain checks before revoking the legacy token.

## Safety and rollback

- No existing tenant database row was manually changed during this pass. All lifecycle/owner/audit mutations in the final continuation used disposable fixture databases; the older deployed-worker observation above is retained only as historical evidence.
- No destructive billing, invitation, role, or deletion action was run against live tenants.
- No commit, push, reset, checkout, clean, or doc deletion was performed.
- Roll back application images by redeploying the prior image digest; schema migrations are additive and were separately rehearsed in isolated transactional schemas. Do not down-migrate the live database as a routine rollback.

## Sources of truth

- `docs/core-research/plane-audit-2026-07-10.md`
- `docs/core-research/{auth-core,org-core,user-core,billing-core,session-core,audit-core}.md`
- `CONTROL_PLANE_ROADMAP.md`
- `CONTROL_PLANE_OWNERSHIP.md`
- `apps/STALE_DOC_DELETION_REGISTER.md` (report only; no deletion performed)
