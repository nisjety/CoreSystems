# Control Plane — Secure MVP Roadmap

Updated: 2026-07-11. Read `CONTROL_PLANE_STATUS.md` first.

The production-readiness program remains in the MVP phase. Enterprise readiness must not be planned as if it were the next active phase until every MVP acceptance gate below is proven.

## Completed in risk order

1. **Session authentication fail-closed**
   - Removed header-fallback identity.
   - Added explicit audience/scope-bound service credentials.
   - Added regressions for missing, malformed, expired, wrong issuer/audience/signature, and caller-supplied identity headers.
   - Live Session Core and gateway attacks return 401 with no data.

2. **User Core privilege hardening**
   - Removed caller-asserted `X-User-Role` authority.
   - Added scoped caller credentials and verified-claim authorization.
   - Bound gateway/Session self-service delegation to an HMAC-signed caller, audience, timestamp, method, URI, body digest, subject, org, and verified profile claims.
   - Removed caller-controlled email/name/avatar from the bearer profile path.
   - Normalized email lookup at the canonical repository boundary.
   - Live legacy-key plus asserted admin returns 401.

3. **Auth/Org durable projection foundation**
   - Added revisioned transactional organization and membership outboxes.
   - Fixed update re-enqueue, RLS GUC mismatch, monotonic CAS application, deletion tombstones, and reconciliation retries.
   - Fixed Postgres `BIGINT` string serialization to Org Core's numeric JSON contract using a tested fail-closed normalizer.
   - Added a five-second reconciliation deadline and gated acknowledgements/follow-on notifications on exact-revision compare-and-swap success.
   - Live worker recovered three organizations and all three membership rows automatically.

4. **Canonical membership routes**
   - Velion gateway invite/accept/remove/role-change routes now target Auth Core's Better Auth organization contract.
   - Org Core is a projection/domain authority, not a competing canonical membership writer.
   - Removed six legacy Org membership/role mutation registrations; live requests now return 404 while Auth's internal reconcile route remains available.

5. **Deletion and billing ordering**
   - Corrected Billing Core port to 3014.
   - Decoupled Billing and Org completion markers so either side effect can resume independently.
   - Added Billing tombstones/revision protection against delayed resurrection.

6. **Audit correctness and connectivity**
   - Fixed usage-summary SQL and added database coverage.
   - Recreated the live service on the inter-plane network.
   - Added readiness and Prometheus connectivity/event-age visibility.
   - Moved both buses to a file-backed JetStream stream with named durable audit/usage consumers, explicit ACK, bounded NAK, and durable DLQ+TERM handling.
   - Added migration-ledgered `(source_bus, source_stream_sequence)` inbox uniqueness so ACK-loss redelivery is a no-op, and made TERM conditional on confirmed durable DLQ publication.
   - Proved success, transient retry, malformed-event DLQ, five-delivery exhaustion, and stream update against embedded JetStream; proved inbox dedupe against disposable Postgres 16.

7. **Deployment safety**
   - Added migration ledgers/checksums/advisory locks and fixed Auth's constant-folded checksum failure.
   - Removed Auth image's recursive runtime `chown` bottleneck.
   - Rehearsed additive migration rollback paths in isolated transactional schemas.
   - All six Control containers and the gateway are healthy.

## Remaining dependency-ordered MVP work

### 0. Recover and re-verify the Docker runtime

**Why first:** Docker Desktop's content store and BuildKit metadata are returning filesystem I/O errors. The current container health view is contradictory and the final gateway image did not build, so no later live acceptance result is trustworthy.

- With operator approval, restart/repair Docker Desktop while preserving every named volume.
- Do not factory-reset, prune volumes, or recreate databases.
- Verify Postgres/NATS volume consistency and migration ledgers before starting application services.
- Rebuild/deploy matched User, Session, Audit, and gateway images; prove `002_jetstream_inbox` live.
- Re-run health, forged/missing-token, unsigned-delegation, signed-delegation, Audit readiness, and projection-count checks.

**Rollback/safety:** snapshot/export Docker Desktop data before any repair beyond a normal restart. If volume checks fail, stop and restore from backup; never initialize an empty replacement database over the existing data path.

### 1. Repair historical ownerless canonical organizations

**Why first:** Auth-to-Org convergence cannot reach 5=5 while two Auth organizations lack any canonical owner membership. The outbox is correctly fail-closed.

- Build a read-only report that accepts an explicit `organization_id -> owner_user_id` mapping and validates that each organization is currently ownerless, each proposed user exists, and no conflicting owner exists.
- Obtain human review of the mapping from authoritative creation/audit evidence. Do not infer ownership solely from a current/expired session or an invitation.
- Apply the smallest transactional repair in an isolated rehearsal first, then in live only with explicit approval.
- Let the existing outbox retry naturally; prove Auth IDs equal Org IDs, no extras exist, projection/membership pending counts are zero, and repeat retries are no-ops.

**Rollback/safety:** one transaction per organization; lock the canonical organization and membership rows; abort on any changed precondition; record an audit event. Never auto-select an owner. A mistaken owner grant is a security incident, not routine data cleanup.

### 2. Finish scoped service authentication

**Why second:** acceptance B is only partial while Org, Billing, and Audit accept a fleet-shared key.

- Define separate audiences/scopes for Org projection, Billing lifecycle, and Audit ingest/query.
- Configure per-caller principals; reject generic shared keys on privileged routes.
- Confirm Model/Application/Ingestion/Data contracts include the exact subject, audience, org, user, and scopes each endpoint requires.
- Close the Model gateway compatibility test where a service principal may not carry a user subject.

**Rollback/safety:** support an explicitly time-bounded dual-read window only in non-production or during a controlled rollout; emit metrics for legacy-key use; remove the fallback before MVP acceptance. Never log credentials.

### 3. Finish end-to-end Audit durability and replay operations

**Why third:** consumer redelivery is now idempotent in source/integration tests, but security-critical producers still use Core NATS fire-and-forget and can lose an event before it reaches the stream. Operators also lack pending-count and replay proof.

- Add transactional producer outboxes and JetStream publish acknowledgements for Auth/User/Org security-critical events.
- Export consumer pending/redelivery/DLQ counts and oldest-pending age.
- Document and test the DLQ inspection/replay procedure.
- Verify disconnect/reconnect without event loss using isolated fixtures.

**Rollback/safety:** retain the current stream and query API, use additive uniqueness/backfill migrations, and reject conflicting duplicate IDs. Never replay a DLQ into production without a dry-run count and bounded subject filter.

### 4. Prove lifecycle E2E with isolated fixtures

- Invite -> verified-email accept -> role change -> remove through gateway/Auth, including duplicate requests and out-of-order projection delivery.
- Organization delete with Billing unavailable, Org unavailable, and retry after each recovery.
- Delayed plan/member events after tombstone must not resurrect state.
- RLS requests with missing/wrong GUC must fail closed.
- Confirm audit events and reconciliation metrics for every transition.

**Rollback/safety:** create dedicated test tenants or transaction-scoped fixtures. Never mutate the existing live tenant set, subscriptions, invitations, or owner roles.

### 5. Close verification debt

- Add focused tests until changed security-critical modules meet at least 80% measured coverage.
- Raise Session HTTP, User HTTP/service auth, Org projection/RLS, Billing lifecycle, Auth organization outbox, and Audit store/subscriber coverage first.
- Establish a reviewed Auth lint baseline and make changed files lint-clean. The current full run reports 591 errors/43 warnings; do not hide this with a blanket disable.
- Add a Rust coverage command for the gateway or document the chosen coverage tool in CI.
- Re-run builds, tests, vet, lint, format, Clippy, migration tests, `git diff --check`, and the live authenticated/unauthenticated matrix.

**Rollback/safety:** tests and static checks are non-mutating. Avoid `--fix` across the dirty worktree; apply scoped edits only.

### 6. MVP acceptance review

MVP is accepted only when:

- Auth and Org converge exactly with zero unresolved canonical rows.
- All privileged service routes use scoped credentials.
- Audit ingestion is durable and observable.
- Isolated lifecycle/reordering/deletion E2E passes.
- All six services and gateway pass tests/build/static gates.
- Changed critical modules meet the coverage threshold.
- Docker health and auth matrices pass from the images built from the reviewed worktree.
- Docs contain no unverified production claim.

## Enterprise phase

**Deferred.** Once the MVP review above is green, create a separate enterprise-readiness plan covering workload identity/key rotation, HA/DR, multi-region, SLOs/alerting, formal threat modeling/compliance evidence, zero-downtime migrations, chaos/recovery drills, enterprise SSO/SCIM, fine-grained policy administration, and capacity/cost validation. Do not count any of those as completed MVP work unless required to close an active security gate.
