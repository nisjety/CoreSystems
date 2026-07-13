# Control Plane — Current Status

Last verified: 2026-07-11 (continuation of the 2026-07-10 production-readiness audit)

## Decision

**Secure MVP acceptance is not yet proven, and the live Docker stack is currently degraded.** The critical Session impersonation and User caller-asserted role/profile paths are closed in source/tests. Audit durable consumption was live before the final pass, and stream-sequence inbox idempotency now passes against disposable Postgres. However, Docker Desktop's containerd/BuildKit storage began returning `input/output error`; Postgres/container metadata became inconsistent, the rebuilt User/Session/Audit containers are not healthy, and the gateway signing image could not be built. A Docker engine recovery and matched-image redeploy are now the first operational gate.

The enterprise-readiness phase is intentionally deferred until the MVP acceptance matrix is fully green.

## Current operational blocker

After the earlier all-green live matrix, the final image build failed while reading a cached Rust `.rlib`, followed by failures reading BuildKit metadata, image blobs, container `/hosts`, and JSON logs. `docker inspect controlplane-postgres` reported `exited|open .../hosts: input/output error|139` while `docker ps -a` simultaneously showed it as healthy, proving daemon metadata is not trustworthy. No Docker reset/restart was attempted because that would disrupt every plane and requires explicit operator approval.

## Deployment state

| Component | Live result | Evidence |
|---|---|---|
| Docker runtime/infrastructure | **Degraded/untrusted** | Containerd/BuildKit and container metadata return storage I/O errors; Postgres/NATS state is contradictory across Docker APIs. |
| auth-core | Last verified healthy before incident | Reconciliation deadline/CAS image was deployed and verified; current daemon health cannot be treated as release evidence. |
| org-core | Last verified healthy before incident | Migrations 001-013 and route removal were live-verified before the incident. |
| user-core | **Recreated, unhealthy after incident** | Final source closes every raw profile-header handler path, but matched live verification is blocked by the Docker/Postgres failure. |
| billing-core | Last verified healthy before incident | Migrations 0001-0005 were applied, including `billing_organization_tombstones`; current daemon health is not release evidence. |
| session-core | **Recreated, unhealthy after incident** | Final source requires signed Gateway delegation inbound; matched gateway image is not deployed. |
| audit-core | **Recreated, startup unverified** | Durable consumer image previously ran; final `002_jetstream_inbox` migration is proven only on disposable Postgres because live Postgres became inaccessible. |
| Velion v3 gateway | **Old image; Docker unhealthy** | Process `/health` still returns 200, but the final Session signing source image failed to build because of Docker storage I/O. |
| Data boundaries | Not re-verified after Docker incident | They were healthy with distinct User credentials before the engine failure; no current claim is made. |

## MVP acceptance matrix

| Criterion | Status | Verified state |
|---|---|---|
| A. Session fail-closed | **Pass** | Live Session Core: missing bearer, malformed bearer, and forged JWT plus `X-User-Id`/`X-Org-Id`/`X-User-Role` all return 401 and do not echo the fixture identity. Gateway bootstrap gives the same result. Unit regressions cover missing, malformed, expiry, issuer, audience, signature, and header impersonation cases. |
| B. Scoped internal auth | **Implemented in changed paths; deployment mismatch/blockers remain** | User/admin/profile claims are verified-context-only. Gateway→User, Session→User, and Gateway→Session have matching Go/Rust HMAC vectors and reject unsigned delegation in tests. The final Gateway→Session pair is not deployed because the gateway build failed. Org, Billing, and Audit still retain a fleet-shared inbound key. |
| C. Auth-to-Org convergence | **Blocked by historical data** | The `BIGINT` JSON serialization defect was reproduced, tested, fixed, deployed, and automatically retried. Three canonical organizations and all three membership rows reconciled. Live result is Auth 5 vs Org 3: the remaining two Auth organizations have no owner membership and the outbox correctly refuses to invent one. No automatic owner assignment was performed. |
| D. RLS GUC contract | **Pass** | Org migrations 010-013 are live. Policy inventory is `app.current_org_id=0`, `app.current_org=14`; functional and rollback migration tests pass. |
| E. Canonical membership lifecycle | **Implemented; live mutation E2E pending** | Gateway invite, accept, remove, and role change target Better Auth's canonical organization API. Auth emits revisioned membership projections. Org's six competing membership/role mutation routes are unmounted and return 404 live; the Auth reconcile endpoint remains mounted. Real tenant invitation/role mutation was deliberately not exercised. |
| F. Deletion and billing tombstones | **Implemented; live destructive E2E pending** | Auth uses billing port 3014, tracks Billing and Org completion independently, and retries idempotently. Billing migration 0005 and Org projection tombstones/revision guards are live. No real tenant was deleted. |
| G. Audit SQL and aggregation | **Source/integration pass; final live migration blocked** | Durable consumers ACK after idempotent `(source_bus, stream_sequence)` persistence; duplicates ACK without reinsertion. DLQ publication must succeed before TERM. Disposable Postgres proves migration 002 and one-row redelivery semantics. Producer-side transactional outboxes, pending-count metrics, replay drill, and live migration verification remain. |
| H. Tests/build/static/live matrix | **Partial; Docker gate failed** | All six service suites pass; all five Go services pass `go vet`; Auth build/78 tests and gateway format/strict Clippy/209 tests pass. Docker health is no longer green due the engine storage incident. Auth lint and broad coverage debt remain. |
| I. Evidence docs | **Pass for this audit state** | Status, roadmap, plane audit, and all six service research docs distinguish implemented, live, degraded, and unverified claims. No legacy docs were deleted. |

## Exact live evidence

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

Current incident evidence:

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
| auth-core | 12 suites, 78 tests pass; build pass; ESLint baseline remains 591 errors/43 warnings | Changed reconciliation/revision/service-principal set 89.58%; `reconciliation-http.ts` and `outbox-revision.ts` 100%; `plane-service-principal.ts` 85.71% |
| session-core | `go test ./...` and `go vet ./...` pass | Whole service 20.2%; `internal/internalkey` 100%; HTTP package 38.0% |
| user-core | `go test ./...` and `go vet ./...` pass | Whole service 3.8%; `internal/authztaxonomy` 93.3%; HTTP package 12.3% |
| org-core | `go test ./...` and `go vet ./...` pass | Whole service 5.6%; config 90.7%; gRPC 86.7% |
| billing-core | `go test ./...` and `go vet ./...` pass | Whole service 12.6%; config 84.0%; gRPC 81.0% |
| audit-core | `go test ./...`, `go vet ./...`, embedded JetStream race test, and disposable-Postgres inbox test pass | Durable subscriber 72.0% after inbox/DLQ branches; whole-service percentage remains below target |
| Velion gateway | `cargo fmt --check`, strict `cargo clippy`, and all 209 tests pass | Rust coverage tooling was not configured; no percentage is claimed. |

`git diff --check` passes. Coverage files were written under `/tmp`; no generated coverage artifacts were added to the repository.

## Release blockers

1. Operator-approved Docker Desktop recovery: preserve volumes, restart/repair the engine, prove Postgres/NATS consistency, rebuild/deploy the final gateway image, and re-run the full health/auth matrix. Do not factory-reset or delete volumes.
2. Produce a reviewed owner-repair mapping for the two historical ownerless Auth organizations; never infer an owner automatically.
3. Replace shared inbound keys on Org, Billing, and Audit with audience/scope-bound service principals; close the Model token compatibility test.
4. Add producer-side transactional outboxes/JetStream publish acknowledgements for security-critical Audit events, pending-count metrics, and a replay drill.
5. Run isolated lifecycle/reordering/deletion E2E, raise remaining critical-path coverage, and establish a reviewed Auth lint gate.

## Safety and rollback

- No database rows were manually changed during this pass. Reconciliation changes came only from the deployed worker retrying its existing durable outbox.
- No destructive billing, invitation, role, or deletion action was run against live tenants.
- No commit, push, reset, checkout, clean, or doc deletion was performed.
- Roll back application images by redeploying the prior image digest; schema migrations are additive and were separately rehearsed in isolated transactional schemas. Do not down-migrate the live database as a routine rollback.

## Sources of truth

- `docs/core-research/plane-audit-2026-07-10.md`
- `docs/core-research/{auth-core,org-core,user-core,billing-core,session-core,audit-core}.md`
- `CONTROL_PLANE_ROADMAP.md`
- `CONTROL_PLANE_OWNERSHIP.md`
- `apps/STALE_DOC_DELETION_REGISTER.md` (report only; no deletion performed)
