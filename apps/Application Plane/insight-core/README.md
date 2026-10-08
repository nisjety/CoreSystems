# insight-core

`insight-core` is the Application Plane backend for Verevon-wide workspace insights. It owns human-facing, org-scoped analytics projections across Social, Inbox, Agents, Campaigns, and future external analytics connectors.

## Current status — 2026-10-05

See the shared [Billing, Cost and Insight Core status and future plan](../../../docs/BILLING_COST_INSIGHTS_STATUS_AND_ROADMAP.md). Further implementation is on hold at the user's request. Operational event totals remain observed unless source reconciliation proves coverage for that metric, scope and window. The overall overview contract currently reports coverage as unknown because universal source delivery checkpoints are not implemented.

The frontend gateway can compose the overview with a fresh Conversation Core outcome snapshot. This verifies particular retained ticket/review counts; it does not prove completeness of message or agent-run event delivery. Billing allowances, provider cost evidence and subscription quotas retain their separate owning services.

## Plane Decision

Insights belong in the Application Plane because they are workspace projections for Verevon operators. Control Plane remains the authority for identity, organization membership, quotas, billing, and audit. `insight-core` must consume Control context through headers or service contracts and must not read Control Plane databases.

The service uses its PostgreSQL projection repository when `DATABASE_URL` is configured and an in-memory repository otherwise. The in-memory fallback loses recorded metrics on restart and is not evidence of durable production reporting. Cross-plane data arrives through APIs, events, or token leases, never direct database access.

## API

All non-health routes require `x-internal-api-key`.

- `GET /health`
- `GET /ready`
- `GET /api/v1/insights/overview`
- `POST /api/v1/insights/overview` — internal overview composition with a validated source outcome snapshot
- `GET /api/v1/insights/connectors`
- `POST /internal/insight-events`

Org scope is required through `x-org-id` or `org_id`/`orgId` query parameters for read routes. Ingest payloads carry `org_id`.

These are internal service contracts, not browser authorization. The public gateway resolves tenant/actor identity from the authenticated session and membership; it must not forward caller-forged scoping headers or expose the internal key. Source outcome snapshots are validated for organization, actor, reporting window, freshness and source before being presented as measured.

## External Connectors

Google Analytics 4 and Google Search Console are represented as disabled connector slots. Their contracts follow the current official Google surfaces:

- GA4 Data API `properties.runReport`: dimensions, metrics, date ranges, filters, ordering, offset/limit.
- GA4 dimensions/metrics schema: provider-owned dimension and metric names.
- Search Console Search Analytics `query`: authorized date-range query grouped by requested dimensions.

Do not place OAuth client secrets or refresh tokens in this service. Future connector activation should use `integration-corev2` token leases with consumer `insight-core`.
