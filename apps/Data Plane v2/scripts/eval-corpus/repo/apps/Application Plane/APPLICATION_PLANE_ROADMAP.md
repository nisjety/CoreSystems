# Application Plane — Secure-MVP Roadmap

As of 2026-07-13, the plane is not MVP-ready. This roadmap starts from the evidence in `docs/core-research/plane-audit-2026-07-13.md`. Source-complete does not mean deployed or live-verified.

## Gate 0 — Safe rollout prerequisites

1. Reconfirm root/Docker headroom immediately before each targeted build and retain every current image digest. The latest 19:13 CEST check showed 32 GiB free on root/Data and 63 GiB on the workspace volume after an earlier transient 3.6 GiB alarm; do not infer permission to build or prune.
2. Record current container image IDs, Compose config hash, database/Convex backup references, health responses, and rollback commands.
3. Add immutable image tags, OCI revision metadata, and a non-secret `/build-info` surface so source/runtime correlation is auditable.
4. Restore Model inference/retrieval health and provision authorized WhatsApp/Messenger and Novu sandbox identities.

## Gate 1 — Authority and tenant isolation

1. Deploy the exact Auth Core membership authority, run the duplicate preflight for migration 016, remediate any duplicate rows explicitly, then apply the unique `(organization_id,user_id)` index. Deploy User Core and the gateway only after their dedicated credentials are provisioned and proven distinct.
2. Deploy the completed shared gateway authorization result so notification, social, leads, knowledge/imports, privacy, billing, actions, and conversation all distinguish canonical membership denial (403) from authority degradation (503). No sensitive caller may authorize from `active_org_id` alone.
3. Deploy conversation-core's signed audience/tenant/user/role delegation and role gates. Provision and deploy the implemented per-org Auth Core `ingestion` service bearer and Ed25519 provider-write attestation key pair. Apply conversation migrations 004/005 and Integration migrations 0007/0008/0009; 0009 must audit an applied-0008 database before validating the stronger human/AI authorization relationships. Opaque `approvalId`, legacy-key, and unattested writes must remain forbidden.
4. Deploy notification's organization-scoped typed-recipient contract only after Control supplies a signed, revisioned membership writer and removal-first scoped backfill. Preserve quarantined legacy rows; never infer grants.
5. Add no-auth, malformed, wrong-audience, wrong-org, wrong-role, forged-header, replay, and valid-scoped integration tests for every sensitive HTTP/NATS/Convex boundary.
6. Preserve Auth Core's revisioned DB outbox to Org Core, then make the downstream cross-plane membership envelope durable and issuer-verifiable with immutable event ID, revision, previous/new state, and subject-level publish/consume ACLs.

## Gate 2 — Content lifecycle and ZDR

1. Define one immutable cross-plane content-handling envelope carrying ZDR, retention, purpose, tenant, and audit correlation.
2. Enforce it for conversation bodies/attachments/summaries/drafts, social payloads, notifications and provider handoff, jobs, caches, logs, analytics, DLQs, and downstream events. Local payload suppression is insufficient unless the provider retention contract is proven.
3. Add fail-closed tests for ZDR tenants, log/metric redaction tests, expiry/deletion proofs, and outage/retry behavior.

## Gate 3 — Conversation/Inbox delivery correctness

1. Deploy the existing conversation outbound-intent and integration provider-receipt migrations under a migration lock, then prove concurrent claim/replay against real Postgres. Preserve the tables and receipt history on rollback.
2. Add leases/attempt phases and a dry-run reconciliation worker/API for stale `sending`, `executing`, and `unknown` rows. A known pre-provider failure must remain safely retryable; ambiguous provider outcomes must never be blindly retransmitted.
3. Extend provider acceptance into callback-driven `submitted/sent/delivered/failed/unknown` states. Make UI/event wording distinguish submitted from delivered, and require durable delivery/execution evidence before final AI-action state.
4. Deploy and live-test the source-complete durable provider-write attestation described in Gate 1. Re-check proof at every endpoint, worker, retry, and queue publication path; `/messages` can never select store-only behavior. Model-originated provider writes remain disabled until Model can issue the identical authoritative contract.
5. Complete webhook signature/timestamp/replay, account-to-tenant routing, attachment validation/limits, ordering, dedupe, callback, and crash-recovery tests.
6. Run Inbox UI → gateway → worker → authorized provider sandbox E2E, including provider 5xx/timeout/duplicate callback cases. Never use a real customer.

## Gate 4 — Convex projection safety

1. Deploy schema/functions before the subscriber; verify member removal, session revocation, tombstone ordering, import completion, and public membership authorization against an executing backend.
2. Back up Convex, run a single-tenant reconciliation dry-run, review removal/demotion/missing/promotion reports, then apply only with explicit operator approval.
3. Prove duplicate/out-of-order/outage recovery from a Control-authoritative event stream. Do not rely on `_source` payload fields for authority.
4. Add an executing backend contract gate in CI in addition to the existing static called-function test.

## Gate 5 — Notification and support automation

1. Keep support-worker disabled until every workflow supplies an authoritative Control organization/user mapping. The source accepts only typed tenant-scoped users and ZDR; numeric Zammad owners, literal `broadcast`, and raw email-as-subscriber are rejected as identities.
2. Wire the source-complete notification/feed outbox and attempt state machine
   (`notification_delivery_attempts`) behind an explicit rollout flag. It has
   lease-fenced claims, `sent_unconfirmed`/`unknown` states, and a callback
   verifier contract; finish provider-specific callbacks/reconciliation,
   bounded retry/dead-letter policy, feed projection, and the Postgres-backed
   expiry/replay worker before enabling replicas. Never persist ZDR payloads in
   asynchronous attempts.
3. Make readiness depend on required database/cache/bus/provider state. Add channel-specific preference/consent semantics, recipient/schema limits, tenant rate limits, templates, audit, and opt-out tests.
4. Deploy in disabled mode first; verify no 2xx delivery claim. Enable Novu only with an authorized test subscriber and verify valid, duplicate, retry, suppression, failure, and recovery flows.

## Gate 6 — Information, social, leads, and full verification

1. Deploy information-core, Model formatter, and Verevon v3 provenance changes together. Retire or migrate Verevon v2 before treating it as active. Verify measured/estimated/synthetic/stale/unavailable rendering live.
2. Run authenticated tenant-scoped social metrics/catalog UI E2E. Regression-test worker HITL and provider failure without publishing to a real account.
3. Preserve the current native-arm64 Brreg proof; add reproducible `linux/amd64,linux/arm64` CI, immutable digest/SBOM/provenance, and image-internal TLS/readiness verification.
4. Run format, lint, typecheck, race/unit, integration, contract, and E2E suites; measure at least 80% for changed security/business-critical modules and report actual plane-wide gaps.
5. Execute the per-service health/readiness and rollback gates in `docs/runbooks/application-plane-safe-deployment-2026-07-13.md`. Update the evidence matrix with immutable image and test artifacts.

## Secure-MVP exit criteria

All 13 acceptance items in the 2026-07-13 audit must be pass—not partial or source-only—and no critical/high finding may remain without an explicit release-blocking disposition. Only then may the status say secure MVP.

## Enterprise-next — only after MVP

Workload mTLS, fine-grained ABAC, provider credential lifecycle, HA queues/workers, regional webhook failover, replayable event logs, DR/restore drills, eDiscovery/export, consent/data residency, SLO/error budgets, policy-as-code, signed SBOM/provenance/images, capacity/backpressure, advanced anti-abuse, and automated compliance evidence. This is a future roadmap, not an enterprise-readiness claim.
