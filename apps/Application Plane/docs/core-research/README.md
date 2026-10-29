# Application Plane Core Research

Generated: 2026-06-09
Current-state refresh: 2026-07-13 CEST

The latest audit is `plane-audit-2026-07-13.md`. It supersedes the current-state conclusions in `plane-audit-2026-07-11.md`; the older audit remains historical evidence and is not deleted. Read the plane-level `APPLICATION_PLANE_STATUS.md` and `APPLICATION_PLANE_ROADMAP.md` with it.

## Current decision

The Application Plane is not yet a production-ready secure MVP and is not enterprise-ready. Source fixes are tested but not deployed. The principal blockers are deploying/live-testing canonical membership across every protected gateway domain, provisioning/deploying the implemented conversation→integration bearer and Ed25519 approval proof, incomplete plane-wide ZDR, stale outbound/receipt reconciliation and provider callbacks, unsigned/lossy cross-plane membership fan-out after the Control→Org revisioned outbox, notification authority-projection/outbox/callback availability, and remaining coverage gaps.

No stack rebuild was attempted on July 13. Root capacity briefly fell to 3.6 GiB, then recovered without action from this audit; the latest 19:13 CEST check showed 32 GiB free on root/Data and 63 GiB on the workspace volume. Capacity is no longer the immediate blocker, but each targeted build needs a fresh margin calculation, and immutable rollback artifacts, migrations/functions, credentials, approval/ZDR controls, and live gates remain incomplete. No prune, daemon restart, database mutation, production reconciliation, customer message, social publication, or paid provider action occurred.

## Facts corrected on 2026-07-13

- `application-postgres` is healthy in the current runtime; the July 11 corruption result is stale.
- `leads-core` is native arm64 and the exact running container completed DNS/TLS and reached real Brreg with HTTP 200; the Rosetta/x86 conclusion is stale.
- Conversation send-first/502 and fail-closed ingest behavior exists in source; unavailable external-delivery paths no longer store false sent rows, AI review is compare-and-set, and real HITL remains.
- Social HITL is worker-enforced and metrics/catalog gateway routes exist.
- Notification's canonical intake is `/api/v1/notification-requests`; the caller was wrong.
- Convex's `change-me` fallback is gone. Member removal, import completion, authz, tombstones, durable consumption, and removal-only reconciliation are now implemented in source, but the live bundle is old.
- Information providers are real. The traffic defect was synthetic local values layered on real metadata; source/UI/Model now preserve provenance, but those builds are not live.
- Auth Core now exposes an exact membership decision, rejects ambiguous duplicate rows, and ships a duplicate-preflight/unique-index migration. User Core and Inbox use that authority in changed source; the database migration and workloads are not live.
- Conversation and integration now have content-free intent/receipt ledgers and the Inbox reuses one idempotency key across ambiguous retries. Integration's forward-only migration 0009 audits databases that recorded the weaker 0008 relationship constraint before validating the stronger human/AI binding. This prevents blind duplicate transmission in source, but does not supply delivery callbacks, stale-state reconciliation, or authoritative ZDR.
- `affine-core` is not in the current default Compose graph. Zammad remains opt-in and is not running.

## Scope and files

- `conversation-core-go.md`
- `conversation-ingest-rs.md`
- `social-core.md`
- `convex-core.md`
- `notification-core.md`
- `information-core.md`
- `insight-core.md`
- `leads-core.md`
- `verevon-gateway-rs.md`
- `zammad-foundation.md`
- `plane-audit-2026-07-13.md` — current finding register, evidence matrix, acceptance state, and blockers
- `plane-audit-2026-07-11.md` — historical audit; retained because it records prior/refuted findings

Cross-plane callers reviewed in the current audit include Verevon v3, the Ingestion support-worker, Control identity/org event publishers, and the Model traffic formatter.

## Current runtime shape

The default Compose graph contains 16 running services: Postgres, Dragonfly, local NATS, Prometheus, Grafana, four Convex workloads, conversation core/ingest, social, insight, leads, information, and notification. The support-worker is an adjacent Ingestion workload; an old container is still running although current source profile-gates it as opt-in.

Application Plane owns collaborative/realtime projections, conversations, notifications, social workflows, leads, information, and insight services. Control remains authoritative for identity/org/billing; Data for knowledge/retrieval; Ingestion for evidence capture; Model for reasoning/execution; Frontend for UI and the same-origin gateway. No Application projection becomes an authority.

## Highest-signal unresolved risks

1. Canonical gateway membership now covers protected cross-domain routes in source, signed conversation delegation is source-remediated, and conversation's per-org Auth Core bearer plus effect-bound Ed25519 approval proof are implemented and contract-tested. Credential/key provisioning, migrations/deployment, and live cross-tenant/provider negatives must still complete before tenant/HITL isolation is credible.
2. ZDR and retention are not proven over every content-persisting boundary.
3. Conversation/integration ledgers exist in source, but stale `sending`/`executing`/`unknown` rows have no reconciliation worker, provider acceptance has no delivered-state callback, and pre-provider failures can strand keys.
4. Auth Core has a transactional revisioned outbox to Org Core; the later cross-plane membership fan-out still lacks signed issuer, immutable source event ID/revision, acknowledged delivery, and narrow ACLs.
5. Notification now has an organization key, typed user recipient, signed delegation, membership gates, and a ZDR support path in source. It lacks a trustworthy Control membership writer/backfill, transactional feed/delivery outbox, callback reconciliation, and HA replay state; support workflows still lack authoritative mappings.
6. Running Convex, notification, information, Model, and callers are stale relative to source, with no trustworthy revision endpoint.
7. Model health and authorized provider sandbox identities block complete Inbox/notification E2E.
8. Changed ledger functions meet 80%, but several broader conversation/notification packages and Inbox UI coverage remain below or unmeasured against the required gate.

## Operator docs

- `../runbooks/application-plane-safe-deployment-2026-07-13.md`
- `../runbooks/convex-membership-reconciliation-2026-07-13.md`
- `../runbooks/leads-core-native-build-deploy-2026-07-13.md`

Do not promote a source-only statement to a deployed/live claim without an immutable image/function revision and the corresponding safe runtime evidence.
