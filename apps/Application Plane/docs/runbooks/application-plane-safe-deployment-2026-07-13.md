# Application Plane safe deployment and rollback — 2026-07-13

This runbook is operator-gated. It does not authorize a rollout. As of 2026-07-13 19:13 CEST, root/Data had 32 GiB free and the workspace volume had 63 GiB after an earlier transient 3.6 GiB alarm cleared without action from this audit; Docker retained 35.67 GB of images and 4.17 GB of build cache. Capacity is no longer the immediate blocker, but this is not a blanket build authorization. Do not deploy the changed plane until canonical gateway authority and the conversation→integration bearer/attestation chain are provisioned for a coordinated cutover; plane-wide ZDR, stale delivery reconciliation, cross-plane Control event authority, notification membership-projection availability/outbox/callback recovery, immutable rollback artifacts, and coverage gaps remain open.

## Non-negotiable preflight

Run from `apps/Application Plane`. Stop if any result is unexpected.

```bash
df -h /
docker system df
docker compose config --services
docker compose ps
git status --short
docker inspect --format '{{.Name}} {{.Image}} {{.Config.Image}}' \
  conversation-core-go conversation-ingest-rs social-core insight-core \
  leads-core information-core notification-core convex-gateway convex-subscriber
```

Required before build:

- operator-defined free-space margin sufficient for all target images plus the current rollback images;
- current immutable image IDs recorded outside the repository;
- Postgres and Convex backups completed and restore-tested by the operator;
- Compose secrets resolved from the approved secret store, never shell history or committed env files;
- dedicated membership, gateway, ingest, Auth Core service-principal, integration approval-authority, notification, NATS, and reconciliation credentials validated as non-placeholder and pairwise distinct where the source requires it;
- `CONVERSATION_INTEGRATION_SERVICE_API_KEY` matches exactly one `conversation-core` entry in Auth Core `PLANE_SERVICE_PRINCIPALS_JSON` with `aud=ingestion`, scopes `integration:read`/`integration:write`, `allowAnyOrg=true`, and an empty static `orgIds` list; Auth Core still mints each token for one requested organization. Set `CONVERSATION_INTEGRATION_SERVICE_ID=conversation-core`, and ensure `AUTH_CORE_URL` resolves to the trusted Auth Core;
- `INTEGRATION_INTERNAL_API_KEY` remains a separate credential used only for internal webhook-event reads; it is never a fallback for tenant connection/action calls;
- `CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY` is a secret-store reference containing a standard-base64 64-byte Ed25519 private key; `CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID` is a non-placeholder rotation ID;
- Integration `INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON` contains the matching standard-base64 32-byte public key under issuer `conversation-core` and the exact key ID. Keep the private key out of Integration, environment files, logs, shell history, and the repository;
- source tests green and build metadata contains the intended revision;
- approved maintenance window, owner, abort criteria, and rollback owner recorded.

Never restart/prune the daemon, delete volumes, recreate the whole stack, or use `docker compose down` as part of a service rollout.

## Generic targeted replacement

Set one Compose service/container pair at a time. This preserves the old image by ID even when the Compose tag moves.

```bash
export SERVICE=information-core
export CONTAINER=information-core
export IMAGE_REF="$(docker inspect --format '{{.Config.Image}}' "$CONTAINER")"
export OLD_IMAGE_ID="$(docker inspect --format '{{.Image}}' "$CONTAINER")"
docker image inspect "$OLD_IMAGE_ID" >/dev/null

docker compose build "$SERVICE"
export NEW_IMAGE_ID="$(docker image inspect --format '{{.Id}}' "$IMAGE_REF")"
test "$NEW_IMAGE_ID" != "$OLD_IMAGE_ID"

docker compose up -d --no-deps "$SERVICE"
docker compose ps "$SERVICE"
```

Run the service-specific gates below. On any failure, rollback immediately:

```bash
docker image tag "$OLD_IMAGE_ID" "$IMAGE_REF"
docker compose up -d --no-deps "$SERVICE"
test "$(docker inspect --format '{{.Image}}' "$CONTAINER")" = "$OLD_IMAGE_ID"
docker compose ps "$SERVICE"
```

Do not remove either image until the observation window and rollback window close.

## Service order and gates

| Order | Workload | Rollout gate | Post-replacement proof | Current disposition |
|---|---|---|---|---|
| 1 | Control auth-core | duplicate query reviewed; migration 016 preflight clean or duplicates explicitly remediated; membership/service-principal credentials and durable audit available | exact grant/deny/duplicate/authority-outage tests; unique index present; no secret reuse | **do not deploy yet**: migration unexecuted and new credentials unprovisioned |
| 2 | Control user-core | Auth Core exact authority live; dedicated membership credential | canonical deny marks the exact local projection removed; authority outage is 503 and does not mutate; persistence failure is visible | source race/vet verification passes; real PostgreSQL execution and deployment remain pending |
| 3 | integration-corev2 | Auth Core `ingestion` principal and trusted attestation public-key ring provisioned; receipt migrations 0007/0008/0009 ready; 0009 mismatch audit reviewed; migration advisory-lock behavior proven on isolated Postgres | legacy/opaque/missing/wrong-tenant/wrong-payload proof denied before OAuth; concurrent API/worker startup applies each version once; pending/executing/replay/conflict on real Postgres; strong authorization constraint validated | **coordinated cutover only**: deploying first rejects every old conversation write |
| 4 | conversation-core-go | matching private key and key ID provisioned; outbound migrations 004/005 ready under lock; authorized sandbox | provider failure non-2xx/no false row; durable manual/AI binding; same-key pre-provider retry; ambiguity unknown; safe valid submission | **coordinated cutover only**: deploying first omits the legacy `approvalId` old Integration requires; reconciliation/ZDR/sandbox remain open |
| 5 | conversation-ingest-rs | core healthy; provider signature/replay contract complete | no/malformed/replay/wrong-tenant negative tests and dedupe | **do not deploy yet** |
| 6 | Velion v3 gateway/UI | Control exact authority live; gateway/notification tokens; authenticated test tenant | removed member denied across conversation/notification/social/leads/onboarding; org-A cannot read org-B metadata/entitlements; authority outage 503; provenance labels; Inbox key reuse/honest copy | 242 source tests and upstream-not-called IDOR regressions pass; **do not deploy yet** until upstream chain and live negatives are ready |
| 7 | notification-core | service-specific gateway/support tokens provisioned; Control-authoritative membership writer/backfill proven; delivery mode explicitly `disabled` | `/health`; `/ready` is 503 while disabled; legacy-key/replay/wrong-tenant requests rejected; no submitted claim | **do not deploy yet**: authority writer/backfill and delivery/feed outbox/callback open |
| 8 | information-core | Velion v3 and Model formatter release artifacts ready together | `/health`; authenticated traffic response has observation provenance and no bare invented metrics | source ready; coordinated rollout blocked by callers |
| 9 | Model execution-core | Model Plane runbook/health gates | targeted formatter tests plus healthy inference/retrieval | blocked by Model outage |
| 10 | convex-backend schema/functions via convex-gateway | backup; dedicated reconciliation key; executing backend tests; issuer-verifiable revisioned Control events | function presence, removed-member read denial, import projection, duplicate/out-of-order tests | **do not deploy yet**: event authority open |
| 11 | convex-subscriber | functions already live; durable consumer/DLQ monitored | removal event revokes membership/session; redelivery and DLQ metrics | same blocker as Convex gateway |
| 12 | social-core | authenticated test tenant/account; no real publication | metrics/catalog UI path; worker HITL bypass negatives; simulated provider failure | source regression green; live proof pending |
| 13 | leads-core | reproducible multi-arch artifact and rollback digest | `/health`, `/ready`, image architecture, authenticated company-only Brreg 200 | current arm64 runtime already passes; no corrective rebuild needed |
| 14 | insight-core | scheduler configuration and notification disabled-mode semantics agreed | overview/connectors, subscriber health, trigger-to-notification evidence | trigger path unproven |

Base infrastructure (`application-postgres`, Dragonfly, NATS, Prometheus, Grafana, Convex storage) is not replaced by this runbook. Use separate owner-approved backup/restore procedures.

## Coordinated conversation/Integration cutover

The new contract is intentionally not rolling-compatible: old Integration requires an opaque `approvalId`, while new Integration requires the signed `writeAttestation`; new Conversation no longer sends the opaque fallback. Do not replace either workload independently while provider writes are enabled.

In an approved maintenance window, first block new Inbox/AI provider-write traffic and verify the queue has no in-flight provider action. Record both old image IDs. Stop only `conversation-core-go` so the public path fails visibly instead of mixing contracts. Before rollout, use an isolated copy of the production Postgres major version to start the migration-capable Integration API and email worker concurrently; both must become ready and each migration version must appear once. Changed source serializes migration-table bootstrap and every version through one transaction-scoped advisory lock, but only pgxmock ordering is proven in this audit. Then replace `integration-api` from the Ingestion Compose project (which applies migrations 0007/0008/0009 at startup). Migration 0009 takes a receipt-table write lock, reports only an aggregate mismatch count, and aborts before replacing the constraint if any older attested receipt violates the stronger human/AI identifier relationships. Stop for owner-led reconciliation; never rewrite completed or unknown effects blindly. Only after 0009 validates should the operator replace `conversation-core-go` from the Application Compose project (which applies 004/005) and run all forged-proof/tenant/readiness gates before reopening writes. Do not send a production message as the gate.

Rollback is also paired: block writes, stop conversation, restore both old image IDs, then start old Integration followed by old Conversation. Preserve all five ledger migrations/tables and receipt rows; never down-migrate or delete them during rollback. Exact image tagging/replacement uses the generic commands above from each owning Compose directory.

## Safe HTTP gates

These checks do not send provider messages or publish social content:

```bash
curl --fail --silent --show-error http://127.0.0.1:3160/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3161/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3162/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3163/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3164/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3190/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:3210/version >/dev/null
```

For notification disabled mode, HTTP 503 from readiness is the expected safe state:

```bash
test "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:3140/ready)" = 503
```

Authenticated/cross-tenant tests must use approved seeded tenants and identities. Record only status, request correlation ID, tenant shape, and redacted outcome—never tokens or payload content.

## Support-worker disposition

Current source profile-gates support automation and defaults notification mode to disabled, but an old worker is still running. An operator should first confirm it has no active intended workload, then stop only that service through the Ingestion Compose project. Do not remove its container or queues. Keep it disabled until every workflow carries authoritative Control organization/user mapping and an observable trigger-to-notification test passes. Disabled activities currently complete as skipped without a durable skip metric, so absence of failures is not proof of delivery.

## Evidence record

For every replacement record: timestamp, owner, source revision, old/new image ID, Compose config digest, migrations, backup reference, test tenant, expected/actual health and readiness, auth/tenant negative results, provider classification, observation window, and rollback result. Update `docs/core-research/plane-audit-2026-07-13.md`; a healthy process alone is not deployment proof.
