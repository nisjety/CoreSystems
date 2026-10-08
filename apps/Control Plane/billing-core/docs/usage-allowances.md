# Usage allowances, renewal and Cost Core delivery

Implementation status: 2026-10-05. This documents the implemented unit-accounting layer and its limits, not full Polar feature parity.

For the shared Billing, Cost and Insight Core status, implementation hold, decisions and phased acceptance plan, see [the current handoff](../../../../docs/BILLING_COST_INSIGHTS_STATUS_AND_ROADMAP.md). This guide remains the detailed reference for the implemented allowance layer.

## Why this exists

Provider costs, subscription quotas, product allowances and customer invoices are different quantities. A request can include thousands of input tokens; a subscription request can consume tokens without creating a per-token API charge. A dashboard reporting reset must not erase usage evidence or financial obligations.

Cost Core remains the authority for recorded provider invocations. Billing Core owns organization allowances and recurring credit policy. Insights presents their authorized projections. Neither service reads another plane's database.

## Data flow and ownership

1. Model Plane `cost-core/internal/postgres/store.go` commits a classified cost entry and a `cost_billing_outbox` row together. Unknown billing basis is retained in Cost Core but not silently turned into billable usage. Migration `0010_billing_usage_outbox` does not backfill historical entries.
2. `internal/postgres/billing_delivery.go` delivers due rows every 15 seconds to Billing Core's existing usage API. Its batch is limited to 50 source entries. Failed deliveries retry with bounded exponential delay. HTTP redirects are rejected to avoid forwarding service credentials.
3. An invocation becomes one request counter and, when positive, separate input and output token counters. Stable IDs are `cost:<entry UUID>:requests`, `:input`, `:output`. All counters must be acknowledged before the outbox row is marked delivered. Partial delivery can retry safely because the receiver compares event identity and payload.
4. Control Plane `billing-core/internal/billing/repository.go` reserves the event ID, records usage and projects consumption into the configured allowance in one database transaction. A shared organization lifecycle lock prevents competing consumption/configuration and rejects tombstoned organizations.
5. `allowance.go` replays event time into exact integer unit balances. `allowance_repository.go` persists snapshots atomically, on reads and through a minute scheduler. Restarting the scheduler does not grant duplicate units: renewal is a deterministic projection of the immutable policy and source entries.
6. The authenticated gateway reads `/api/v1/billing/allowances` for the active organization. Insights renders `BillingAllowancesSection` only for an authorized organization billing viewer and organization scope. Its resource identity changes with the authenticated tenant, preventing stale tenant responses from being displayed. It refreshes with the existing Insights timer.

### Canonical counters

| Billing basis | Request meter | Input-token meter | Output-token meter |
|---|---|---|---|
| Metered API | `ai.metered.requests` | `ai.metered.input_tokens` | `ai.metered.output_tokens` |
| Subscription | `ai.subscription.requests` | `ai.subscription.input_tokens` | `ai.subscription.output_tokens` |

These six raw counters are internal usage measurements. They do **not** enqueue Lago invoice events. Commercial meter mapping, price versions and financial settlement must be implemented/configured separately before they create charges. Existing unrelated usage meters retain their existing provider delivery behavior.

The delivery payload contains organization, metric, quantity, source, event identity and occurrence time. It contains no prompt, response or raw OAuth token. Per-user reporting remains in Cost Core; the organization allowance is not a user wallet. The connected ChatGPT account's provider allowance remains a separate account-level snapshot.

## Renewal and credit rules

- Each organization/meter has an immutable initial policy, an explicit UTC anchor, a unit (`tokens`, `requests` or `units`) and an interval of 1–12 calendar months. The meter clock is independent of invoice frequency.
- Calendar boundaries retain the original day: January 31, February 28, March 31. A policy can end at an explicit instant; the final displayed period is clipped to that instant.
- `included_units` grants the recurring quantity. `rollover_cap` bounds eligible unused recurring units carried to the next cycle. `rollover_cycles=0` disables carry-over, `-1` removes time expiry, and positive values specify additional cycles of validity. The cap still applies when time expiry is disabled.
- At each renewal, expired units and units above the carry cap are removed before the new recurring grant. Additional explicit grants survive renewal until their own expiry; they are not subject to the recurring carry cap.
- Consumption uses the earliest-expiring available credit first. Credits expire at the exact expiry instant. Grant events at the same instant as usage are applied first.
- Usage beyond available credits is recorded as overage **units**, not money owed. Reading quota status is not a transactional hard-limit reservation.
- Late usage is assigned to its occurrence period and recalculates subsequent balances and carry-over. History may be corrected by late arrival; source entries remain intact. This projection never silently modifies an invoice.
- Non-active/non-trialing account transitions open a pause window. Renewal starts inside a pause do not receive recurring credits. Reactivation resumes grants at a following renewal; it does not back-grant missed unpaid allowances. Existing unexpired credits remain visible when active again.
- An exact grant/configuration retry is idempotent; conflicting reuse is rejected. Extra grants require a reason and cannot use the reserved `usage:` identity prefix.

### Scheduled changes

Migration `0010_allowance_revisions` stores append-only revisions. An authorized operator can schedule new included units and carry-over rules at a future original renewal boundary. Revisions have a unique version, boundary and reason and must be appended chronologically. Changing the original calendar anchor or unit through a revision is unsupported.

Historical periods keep their original policy version. Existing credit lots keep their original expiry; the new rollover cap applies at the change boundary. This makes a reduced cap an explicit scheduled forfeiture rule rather than a rewrite of prior grants. Insights shows the next scheduled included quantity and effective date. Mid-cycle proration is not implied by this endpoint.

## API and authorization

All following routes are internal Billing Core service routes:

| Method/path | Purpose | Scope |
|---|---|---|
| `GET /api/v1/billing/orgs/:orgId/allowances` | Current balance, period history, next expiry/change | Existing delegated `billing:account:read:self` or trusted `:any` |
| `PUT /api/v1/billing/orgs/:orgId/allowances` | Configure initial future policy | `billing:allowance:write:any` |
| `POST /api/v1/billing/orgs/:orgId/allowances/:metric/grants` | Audited additional unit grant | `billing:allowance:write:any` |
| `POST /api/v1/billing/orgs/:orgId/allowances/:metric/revisions` | Schedule renewal-boundary change | `billing:allowance:write:any` |

The gateway has read access only; there is no browser route for minting credits or assigning its own allowance. The new write scope is not granted to the gateway or Cost Core. A trusted plan-provisioning principal must be configured before commercial policy activation. Do not add the write scope to a general browser-facing service.

Example policy shape (illustrative, not an activated commercial offer):

```json
{
  "metric": "ai.metered.input_tokens",
  "name": "API input tokens",
  "unit": "tokens",
  "version": "plan-v1",
  "starts_at": "2027-01-01T00:00:00Z",
  "interval_months": 1,
  "included_units": 100000,
  "rollover_cap": 100000,
  "rollover_cycles": 1
}
```

Initial activation must be future-dated. Already-received events occurring after activation are imported using the same stable consumption identity as later retries. Legacy unknown usage before activation is not reclassified or erased. Changing the dashboard's reporting baseline does not change this policy.

## Running and operating

Model Plane Cost Core requires `COST_BILLING_CORE_URL=http://billing-core:3014` and `COST_BILLING_CORE_SERVICE_TOKEN`. Control's canonical launcher generates the shared credential and its service registry grants Cost Core only `billing:usage:write:any`. Use the plane launchers and their generated environment files rather than inventing independent credentials.

Billing Core runs migrations at startup. Cost Core runs its own migrations. The shared Docker service network provides HTTP transport; there is no cross-plane database connection. Frontend changes are unconditional source code and are included by `pnpm build`; they are not a development-only alternate UI.

On 2026-10-05 the local Cost Core, Billing Core, gateway and frontend images were rebuilt and recreated. The existing local frontend Compose target remains its Vite development target; a successful production bundle build is not a hosted production deployment.

Support checks should inspect outbox retry age/status, service credential scopes, migrations, scheduler errors and configured policies. An empty policy list is a valid unconfigured plan, not a fake zero balance. Do not hide delivery failures by marking jobs delivered or clearing history. Customer UI receives concise availability states; raw database/provider errors stay out of the UI.

## Verification and boundaries

Automated coverage includes calendar anchors, capped/disabled/no-expiry carry-over, expiry instants, additional grants, overage units, late delivery, exact retries, concurrent duplicate events, tenant isolation, deleted-organization rejection, paused renewal, scheduled revisions and unauthorized gateway writes. Real Postgres tests run in a separate disposable container. Cost delivery integration tests simulate an upstream failure after a partial acknowledgement, then verify stable identities on retry. No synthetic requests or credits were added to Aquatiq's live usage ledger for these tests.

The browser check confirms the unconfigured allowance state loads, the organization section disappears in personal scope, and the existing minute refresh remains enabled. This does not constitute a paid end-to-end invoice or provider settlement test.

Current scale limits: replay accepts at most 100,000 entries per meter and 1,200 renewal periods. Larger histories fail closed and need an incremental settlement/archive design before this layer is used for high-volume commercial billing. Individual grants/policy amounts are bounded integer units; the frontend rejects totals beyond JavaScript's safe integer range rather than displaying rounded accounting values.

Still outside this implementation: monetary wallets, grant reversal/refund lineage, priced overage settlement, mid-cycle proration, self-service policy editing, and full checkout/tax/payment/dunning reconciliation. Existing provider adapters remain, but were not expanded or certified for those workflows here. Commercial included quantities, price/currency, carry-over policy and unpaid-account treatment must be selected before activating customer billing. No new charge, reporting reset or GitHub push was performed.
