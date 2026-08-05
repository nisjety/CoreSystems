# billing-core Research Dive

Generated: 2026-06-07
Updated: 2026-07-15 (scoped authority and revision/tombstone lifecycle verification)

Scope: `apps/Control Plane/billing-core` plus its Lago dependency (lago-api:3016, lago-front:3015, lago-db:5434, lago-dragonfly:6381, lago-worker, lago-clock, lago-pdf)

## 2026-07-15 final secure-MVP addendum (current)

The final fresh-image 4/4 lifecycle passed with Billing on the correct 3014 endpoint. `organization.created` correctly auto-provisioned the isolated account before fixture setup, and the test now treats that asynchronous winner idempotently rather than expecting an empty table. Billing-down deletion persisted the Org checkpoint, Billing restart resumed the Auth outbox, cancellation/tombstone state remained permanent, and a delayed revision 99 resurrection was rejected. Stable IDs and revision guards preserve one logical effect under retry/reordering.

Full `go test ./...` and `go vet ./...` pass. Existing changed-path coverage remains 81.6% for revision application, 85.7% for tombstoning, 90.0% for deactivation, and 80.8%-100% for migration-0007 usage/outbox functions. No live checkout, provider call, plan change, cancellation, or tenant deletion was executed. Production secret rotation/deployment is still operator-owned; the external-provider orphan-compensation limitation remains outside the local secure-MVP state guarantee and is not represented as fixed.

## 2026-07-15 scoped/lifecycle detail (superseded by final addendum above)

Billing HTTP requires distinct Gateway self-service and Auth organization-deactivation principals bound to the `billing-core` audience/scopes; Org's plan publisher credential is also distinct from all legacy values. Migration 0006 adds monotonic plan revisions and permanent organization tombstones. Application is serialized with advisory locks and applies only newer revisions; duplicate/reordered plan events and delayed events after cancellation cannot reactivate local billing state. The plan consumer is a named explicit-ACK JetStream consumer with bounded retry and DLQ.

Disposable Postgres/embedded NATS and a fresh-image lifecycle stack prove revision 2 before 1, duplicates, same-revision conflict rejection, publish retry with a stable message ID, crash-before-ACK, max-delivery DLQ, Billing-down deletion checkpointing, restart/resume, and tombstone rejection. Billing now requires a successful shared-bus PubAck before delivery state advances and fails closed when the publisher is absent or denied. Runtime broker principals have no topology-admin rights. `go test ./...`, `go vet ./...`, and the earlier race pass remain green. Changed coverage is 81.6% for `ApplyOrganizationPlanRevision`, 85.7% for `TombstoneOrganization`, and 90.0% for `DeactivateOrganization`. No live plan, checkout, subscription, cancellation, or tenant was mutated; external-provider orphan compensation remains a separate known limitation.

Migration 0007 adds caller-stable usage identity and a durable Lago delivery outbox. New HTTP and NATS usage ingress requires a bounded event ID, an explicit RFC3339 occurrence time, a positive finite bounded quantity, and bounded dimensions/metadata. The repository serializes by event ID and commits the de-duplication binding, aggregate usage row, and Lago job in one transaction: an exact retry returns 202, conflicting reuse returns 409, and a crash cannot lose or double-count the Lago delivery. Processing leases are reclaimable and Lago receives the stable event ID as its transaction ID. Generic account persistence is now compare-and-swap protected by plan revision plus update time, including same-revision stale writers. Full Go test/vet, focused race tests, and disposable-Postgres crash/concurrency/legacy-row tests pass; changed critical functions measure 80.8%-100%. Migration 0007 and the required `event_id`/`occurred_at` caller cutover have not been applied to an integration deployment, so this is source and isolated-database evidence only.

## 2026-07-14 Verevon checkout addendum (historical deployment evidence)

The Billing Core and Lago stack remain healthy. Verevon's shared checkout resolver now prefers a complete Nexi embedded checkout (`checkout_id`, checkout key, and approved client URL), accepts only provider-pinned executable/redirect origins, and treats Nexi `charged` and `reserved` statuses as successful activation states. The gateway ignores caller return destinations and derives settings/onboarding success/cancel URLs from its validated public Verevon origin. Hyperswitch is fail-closed for the secure MVP because its runtime origins are not in the reviewed production CSP.

Nexi confirmation now validates a bounded opaque payment ID, applies path escaping before credentialed provider access, and never returns raw provider bodies through Billing HTTP. Regression tests failed before the fixes and now pass; checkout-resolver line coverage is 95%, Nexi statement coverage is 87.8%, the full Verevon suite was then 68 files/357 tests, and Billing's full Go suite/vet passed. Auth, Billing, gateway, and SPA images were rebuilt and healthy. No live checkout, plan change, cancellation, tenant deletion, or provider call was made. At that verification Billing HTTP still used the legacy shared key and lifecycle E2E was pending; the 2026-07-15 addendum supersedes both source/isolation gaps.

## 2026-07-11 production-readiness addendum (historical)

The port defect is fixed: Auth Core calls Billing Core on 3014. Organization deletion checkpoints Billing and Org reconciliation independently, so either side resumes after an outage. Migration 0005 (`billing_organization_tombstones`) is applied, and delayed plan events cannot resurrect a deleted local account: the repository serializes lifecycle writes with a per-org advisory transaction lock and rechecks the tombstone inside the upsert transaction. The service and Lago stack are healthy.

One narrower provider-side gap remains: a tombstone can commit while an already-started remote `EnsureCustomer` call is in flight. The subsequent local upsert fails closed, but the provider may retain an orphan customer because the adapter contract has no compensation operation. This does not reactivate local billing state; provider cleanup/compensation must be designed and tested before calling the external lifecycle fully atomic.

No live tenant deletion, subscription change, or destructive billing action was executed. Correctness is proven by unit/migration tests and additive live schema inspection; isolated outage/retry/deletion E2E remains an MVP gate. `go test ./...` and `go vet ./...` pass. Measured whole-service coverage is 12.6% (config 84.0%, gRPC 81.0%); lifecycle repository coverage remains below target.

## 2026-07-10 Update Summary

Re-audited against a live stack (all Control Plane + Lago containers healthy/up ~13h) plus the uncommitted working-tree diff. Highlights:

- **Live-verified**: `/health`, `GetAccount` (auto-provisions default free-tier account), `POST usage` (records + feeds the retry-to-Lago path), `GET entitlements/:feature`, `GET quotas/:metric` — all internal-key gated (401 without `X-Internal-Api-Key`, confirmed against the real key in `.env.docker`).
- **Nexi Checkout status corrected**: prior memory/notes said Nexi was "awaiting vendor creds." That is now stale — `apps/Control Plane/billing-core/.env.docker` has real-looking DIBS/Nexi **test-mode** credentials populated (`NEXI_BASE_URL=https://test.api.dibspayment.eu`, `NEXI_SECRET_KEY`, `NEXI_CHECKOUT_KEY`, `NEXI_WEBHOOK_SECRET` all non-empty) and `PAYMENT_PROVIDER=nexi` is pinned in both `.env.example` and `.env.docker`. Stripe keys are also populated (test-mode `pk_test_.../sk_test_...`) but Stripe remains the code-path fallback only, selected when `PAYMENT_PROVIDER` is unset/unknown. `.env.docker` is gitignored (`**/*.env.docker`) and untracked, so none of this is a committed-secret exposure.
- **Lago confirmed real and live**, and it is a **usage-metering integration, not the payment provider** — this distinction was implicit but not stated plainly in the 2026-06-07 pass. `internal/adapters/lago/adapter.go` posts usage events to `LAGO_BASE_URL` (`http://lago-api:3000` internally) using `LAGO_API_KEY`; `lago-api` responded `{"version":"v1.42.0", ..., "message":"Success"}` on `/health`. Full Lago stack (api/front/db/dragonfly/worker/clock/pdf) is up and healthy. Payment/checkout is Nexi (or Stripe fallback); Lago never touches money movement in this service, only usage aggregation for invoicing/entitlement math.
- **Uncommitted WIP reviewed** (`internal/billing/service.go` +3, `internal/http/handlers.go` +16, `internal/http/server.go` +1): this is a real, working piece of the P0/P1 org-membership/deletion gap fix, but it ships with **one concrete, high-confidence bug** in a sibling file (`docker-compose.yml`) that will break it on next deploy. See Findings below.
- **grep sweep**: no TODO/FIXME "fake" clusters in the core's own business logic. Two genuine, labeled `not implemented` stubs remain in secondary/fallback payment adapters (Hyperswitch invoice charging, Stripe checkout-status retrieval) — neither is on the active Nexi path.
- **gRPC**: still health/reflection only (`internal/grpc/server.go`, 62 lines, no business RPCs registered) — 2026-06-07 finding still accurate.
- **internal-key helper duplication**: still duplicated verbatim across billing-core, org-core, user-core, session-core (`internal/internalkey/assert.go` in all four) — 2026-06-07 finding still accurate.

## Findings (2026-07-10)

### 1. HIGH — `BILLING_CORE_URL` wrong port in uncommitted `docker-compose.yml`, will silently break org-deletion billing reconciliation AND block org-core's own deletion reconciliation

This is not in billing-core's own files, but it directly breaks a billing-core integration point that ships in the same WIP, so it belongs here.

- The WIP diff adds to auth-core-service's environment block:
  ```
  ORG_CORE_URL: http://org-core:8080
  BILLING_CORE_URL: http://billing-core:3017
  INTERNAL_API_KEY: ${INTERNAL_API_KEY:?INTERNAL_API_KEY is required}
  ```
- `billing-core-service`'s actual container port mapping is `"3014:3014"` (HTTP), `"50013:50013"` (gRPC), `"6062:6062"` (metrics) — there is no `3017` anywhere on billing-core. Port `3017` belongs to **session-core-service** (`"3017:3017"`), a few blocks down in the same compose file. This reads as a copy/paste slip.
- `apps/Control Plane/auth-core/src/auth/organization-events.plugin.ts` (also part of this WIP) added `deactivateOrganizationBilling()`, which does:
  ```ts
  const url = (process.env.BILLING_CORE_URL || '').replace(/\/$/, '');
  ...
  const response = await fetch(`${url}/api/v1/billing/orgs/${orgId}/deactivate`, { method: 'POST', ... });
  if (!response.ok) throw new Error(`Billing Core reconciliation returned ${response.status}`);
  ```
  With the wrong port this call will fail every time (nothing listens on `billing-core:3017` inside the container network — it should be `:3014`).
- The failure mode compounds: in `flushOrganizationDeletionOutbox()`, `deactivateOrganizationBilling(...)` is `await`ed **before** `postOrgCore('/internal/orgs/:orgId/reconcile-delete', {})`, inside the same `try` block. A thrown error from the billing call means the org-core reconciliation call **never executes either** — the whole outbox row just gets `attempts += 1`, `last_error` recorded, and is retried on the next pass.
- This flush runs every minute via `@Cron(CronExpression.EVERY_MINUTE)` in `apps/Control Plane/auth-core/src/services/orphan-organization-cleanup.service.ts` (`removeStaleOwnerlessOrganizations` → calls `flushOrganizationDeletionOutbox()`), so once this WIP is deployed, every organization deletion will retry-loop indefinitely without ever reconciling on either the billing or org-core side, until the port is fixed.
- **Currently not yet live**: verified the running `billing-core-service` container (started `2026-07-09T21:01:32Z` / `23:01:32 CEST`) predates the source edits to `internal/http/server.go` / `handlers.go` (mtimes `2026-07-10 02:04–02:18 CEST`), and live-curled `POST /api/v1/billing/orgs/{id}/deactivate` against it returns `404 page not found` — confirming the container has not been rebuilt with this WIP yet. So this bug is **dormant today** but will fire as soon as billing-core and auth-core are rebuilt/redeployed with the current working tree.
- **Fix**: change `BILLING_CORE_URL: http://billing-core:3017` to `http://billing-core:3014` in `apps/Control Plane/docker-compose.yml` before this WIP is committed/deployed. Also worth considering: swap the call order (org-core reconcile-delete before billing deactivate, or run them independently/in parallel with individual catch blocks) so a billing-core outage doesn't block org-core's own deletion reconciliation.

### 2. VERIFIED GOOD — `CanUseFeature` canceled-subscription gate (the +3 lines in `service.go`) is a real, correct fix

```go
if account.SubscriptionState == SubscriptionStateCanceled {
    return false, account, nil
}
```
- Confirmed by reading `GetAccount`: it hydrates/returns whatever `Entitlements`/`FeatureFlags`/`Products` maps are cached or persisted, with **no** state-based zeroing. Before this change, a canceled org whose entitlements map still had `feature.x: true` (e.g., cached from when it was on a paid plan) would keep passing `CanUseFeature` even after cancellation — a real gap, not a redundant/defensive no-op.
- Scope note: the gate only checks `SubscriptionStateCanceled`. `SubscriptionStatePastDue` is a distinct enum value (`internal/billing/types.go`) and is **not** gated here — an org with a failed payment still passes `CanUseFeature` today. This may be an intentional grace-period design, but it means the "close the entitlement gap" fix is partial: canceled orgs are blocked, past-due orgs are not.
- New route wiring: `POST /api/v1/billing/orgs/:orgId/deactivate` → `deactivateOrganization` handler → `Service.DeactivateOrganization(ctx, orgID, reason)`, which sets `SubscriptionState = SubscriptionStateCanceled` and records `metadata.deactivated_reason`, then persists. This reuses logic that already existed and was already called from the pre-existing `organization.deleted` NATS subscriber (`internal/nats/subscriber.go:135-148`) — the new HTTP route is an additional synchronous entry point for the same effect, called by auth-core's outbox-based deletion reconciliation (see Finding 1) rather than a duplicate/competing implementation.
- Live-tested end-to-end against a scratch org (`test-org-audit-billing`): `GetAccount` auto-provisioned a default free-tier account, `POST usage` recorded successfully, `GET quotas/api_calls` reflected the recorded usage (`used: 1`), `GET entitlements/feature.chat` returned `allowed: true`. The `POST .../deactivate` call itself 404'd only because the running container predates this WIP (see Finding 1) — the code path was read and reasoned through directly, not exercised against a live binary.

### 3. LOW — two explicit `not implemented` stubs remain in fallback payment adapters

- `internal/adapters/hyperswitch/adapter.go:85`: `return fmt.Errorf("hyperswitch invoice charging is not implemented for invoice %s", invoice.InvoiceID)`
- `internal/adapters/stripe/adapter.go:200`: `return billing.CheckoutStatus{}, fmt.Errorf("stripe checkout status retrieval is not implemented")`
- Neither is on the active path (`PAYMENT_PROVIDER=nexi`). They only matter if `PAYMENT_PROVIDER` is switched to `hyperswitch` or `stripe`, or falls back to `stripe` due to an unrecognized `PAYMENT_PROVIDER` value (`buildPaymentAdapter` in `cmd/server/main.go` logs `billing-core unknown PAYMENT_PROVIDER=%q; falling back to stripe` and returns a Stripe adapter with checkout-status retrieval unimplemented). Worth a config-validation guard so a typo'd `PAYMENT_PROVIDER` doesn't silently degrade into a fallback with a known-missing method, but this is a pre-existing, low-severity gap, not new WIP.

## Snapshot

`billing-core` is the Control Plane billing, quota, and invoice facade. It is a Go service with HTTP as the real business API, health/reflection-only gRPC, local NATS ingestion for usage and organization lifecycle, shared NATS publishing for cross-plane billing events, provider adapters for Nexi (active), Stripe (fallback), Hyperswitch (fallback), and Lago (usage metering, separate from payment), optional Redis/Dragonfly caching, a retry processor, and a trial-expiry sweep.

Current evidence highlights:

- HTTP is the true business surface, internal-key gated, live-verified end to end
- gRPC exists only for health/reflection
- local and shared NATS are both used
- retry and trial background loops are active
- Lago is a real, live, separate usage-metering dependency (not payment) — full stack up and healthy
- Nexi Checkout is the live active payment provider with test-mode credentials configured (not "awaiting creds" as previously noted)
- uncommitted WIP adds a real entitlement-gap fix (canceled orgs) and a new deactivate endpoint, but the endpoint's only known caller is currently broken by a wrong port in `docker-compose.yml`

Non-generated/non-vendored file count: about `37` (unchanged from 2026-06-07 pass).

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - optional pprof
  - `buildPaymentAdapter`: selects Nexi / Hyperswitch / Stripe by `PAYMENT_PROVIDER` (or auto-detects by which credentials are present), defaulting to Stripe on empty/unknown values
  - Lago adapter construction (usage metering, independent of the payment adapter)
  - billing service creation (`billing.NewService(repo, paymentAdapter, lagoAdapter, redisClient)`)
  - shared NATS publisher hookup
  - retry processor background loop
  - trial-expiry sweep background loop
  - local NATS subscriber startup
  - HTTP and gRPC startup
- `internal/billing/service.go`
  - main billing domain logic, including the new canceled-subscription entitlement gate and `DeactivateOrganization`
- `internal/http/server.go`
  - HTTP API surface, internal-key auth middleware (`X-Internal-Api-Key` header, checked against `INTERNAL_API_KEY` / `INTERNAL_SERVICE_SECRET`), with the Nexi webhook path exempted (it authenticates via its own shared secret in `Authorization`)
- `internal/nats/subscriber.go`
  - local event ingestion: `usage.>` and `organization.deleted` (the latter already called `DeactivateOrganization` before this WIP)
- `internal/nats/shared_publisher.go`
  - shared cross-plane billing publication

## API And Relationship Map

Primary HTTP surface (all under `internal-key` auth except `/health` and the Nexi webhook):

- `GET  /health`
- `GET  /api/v1/billing/orgs/:orgId/account`
- `PUT  /api/v1/billing/orgs/:orgId/account`
- `POST /api/v1/billing/orgs/:orgId/usage`
- `GET  /api/v1/billing/orgs/:orgId/entitlements/:feature`
- `GET  /api/v1/billing/orgs/:orgId/quotas/:metric`
- `POST /api/v1/billing/orgs/:orgId/invoices`
- `POST /api/v1/billing/orgs/:orgId/checkout-session`
- `POST /api/v1/billing/orgs/:orgId/checkout-session/confirm`
- `POST /api/v1/billing/orgs/:orgId/deactivate` — **new in uncommitted WIP**; see Finding 1 for its caller and the port bug that currently makes it unreachable from auth-core once deployed
- `POST /api/v1/billing/webhooks/nexi` — internal-key-exempt, authenticated by Nexi's own shared secret

gRPC surface:

- health and reflection only (unchanged)

Current relationships:

- `billing-core` -> Nexi adapter (active payment provider, test-mode credentials configured)
- `billing-core` -> Stripe adapter (fallback; checkout-status retrieval not implemented)
- `billing-core` -> Hyperswitch adapter (fallback; invoice charging not implemented)
- `billing-core` -> Lago adapter (usage metering only; `lago-api:3016` live, full stack healthy)
- `billing-core` -> local NATS subjects
  - `usage.>`
  - `organization.deleted` (deactivates billing account)
- `billing-core` -> shared NATS consumers
  - `aqencia.controlplane.billing.*`
  - plain notification subjects via `PublishPlain`
- `auth-core` -> `billing-core` via `BILLING_CORE_URL` HTTP call to the new `/deactivate` route (new WIP integration point; currently misconfigured, see Finding 1)

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is still duplicated verbatim across billing-core, org-core, user-core, and session-core (re-verified 2026-07-10).

Cross-core semantic overlap:

- `billing-core` and `org-core` both publish plan-change style events, but from different ownership angles. That is not automatically wrong, but it is a documentation and event-contract drift hotspot.
- `billing-core`'s `DeactivateOrganization` is now reachable from three places: (1) the pre-existing `organization.deleted` NATS subscriber, (2) the new HTTP `/deactivate` route called by auth-core's deletion outbox, and (3) nothing else observed. Two independent triggers for the same effect is deliberate defense-in-depth (NATS best-effort + synchronous outbox retry), not obviously redundant, but worth documenting explicitly so a future reader doesn't assume it's dead code.

Non-relationship / partial relationship:

- gRPC port exists, but business methods are not registered (unchanged).

## Stubs, Placeholders, And Missing Connections

Observed placeholder logic:

- startup internal-key validation treats placeholder values as fatal in production (unchanged, legitimate safety check, not a code smell)
- provider adapters can run in non-charging/non-sync local mode when keys are absent (unchanged)
- `hyperswitch` adapter: invoice charging genuinely not implemented (labeled, explicit error, not silently mocked)
- `stripe` adapter: checkout-status retrieval genuinely not implemented (labeled, explicit error, not silently mocked) — matters if `PAYMENT_PROVIDER` falls back to Stripe

Missing or partial surfaces:

- no business gRPC API despite exposed port (unchanged)
- local and shared event paths increase contract coordination cost (unchanged)
- new `/deactivate` HTTP route's only known caller is currently misconfigured to the wrong port (Finding 1) — not a billing-core code defect, but an integration break introduced by a sibling file in the same WIP

No large active TODO cluster was found in the main core source (re-confirmed by fresh grep for TODO/FIXME/mock/stub/fake/placeholder/"not implemented" across all non-test `.go` files).

## API Design And Performance Notes

API design:

- billing HTTP routes are coherent and resource-shaped
- API is internal-key gated and clearly scoped by org (live-verified: unauthenticated requests get `401 {"error":"unauthorized"}`)
- gRPC presence without business services may mislead other teams (unchanged)

Performance and operational notes:

- retry processor with exponential backoff is a good resilience feature
- dead-letter emission on exhausted retries is explicit
- trial sweep is periodic and bounded
- optional Redis/Dragonfly caching is operationally sensible; `GetAccount` cache-aside pattern confirmed by reading the code

## Current Doc Cleanup Read

Keep for now:

- `README.md` — current enough to remain, though it should eventually mention health-only gRPC if we want docs to be exact (unchanged read from 2026-06-07)

No delete-ready core-local text docs were identified beyond ordinary metadata noise.

## Bottom Line

`billing-core` is still one of the cleaner Control Plane cores, and its own uncommitted change (the canceled-subscription entitlement gate) is a real, correctly-reasoned fix for a genuine gap. The main concerns as of 2026-07-10:

- **A wrong port (`3017` instead of `3014`) in the uncommitted `docker-compose.yml` will break the new billing-deactivation-on-org-deletion path, and will also silently prevent org-core's own deletion reconciliation from running, the moment this WIP is deployed.** Fix before commit/deploy.
- health-only gRPC despite an exposed gRPC port (unchanged)
- duplicated internal-key helper across four cores (unchanged)
- cross-core plan-change/event semantics that deserve clearer contract ownership (unchanged)
- past-due (vs. canceled) organizations are not yet gated by the new entitlement check — may be intentional, but should be a documented decision rather than an implicit gap
- Nexi is confirmed live with test-mode credentials (memory note claiming "awaiting vendor creds" is stale as of this pass); Lago is confirmed live and healthy as a usage-metering dependency, distinct from the payment provider


## 2026-07-17 optimization-program reconciliation

`CanUseFeature` now gates `past_due`: a past-due subscription keeps access for a 60-day grace window (`PastDueGracePeriod`) then is suspended (access denied). Grace clock tracked in account metadata (`past_due_since`), stamped/cleared on durable save; missing marker fails open. Commit `21212d76`. DB pool bounds (org-core + billing-core) are now env-tunable (`DB_MAX_CONNS`/`DB_MIN_CONNS`, same safe 5/2 default).
