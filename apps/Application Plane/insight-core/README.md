# insight-core

`insight-core` is the Application Plane backend for Verevon-wide workspace insights. It owns human-facing, org-scoped analytics projections across Social, Inbox, Agents, Campaigns, and future external analytics connectors.

## Plane Decision

Insights belong in the Application Plane because they are workspace projections for Verevon operators. Control Plane remains the authority for identity, organization membership, quotas, billing, and audit. `insight-core` must consume Control context through headers or service contracts and must not read Control Plane databases.

The first slice uses an in-memory projection repository and an internal metric-event ingest API. Durable storage can be added later inside the Application Plane database only. Cross-plane data must arrive through APIs, events, or token leases, never direct database access.

## API

All non-health routes require `x-internal-api-key`.

- `GET /health`
- `GET /ready`
- `GET /api/v1/insights/overview`
- `GET /api/v1/insights/connectors`
- `POST /internal/insight-events`

Org scope is required through `x-org-id` or `org_id`/`orgId` query parameters for read routes. Ingest payloads carry `org_id`.

## External Connectors

Google Analytics 4 and Google Search Console are represented as disabled connector slots. Their contracts follow the current official Google surfaces:

- GA4 Data API `properties.runReport`: dimensions, metrics, date ranges, filters, ordering, offset/limit.
- GA4 dimensions/metrics schema: provider-owned dimension and metric names.
- Search Console Search Analytics `query`: authorized date-range query grouped by requested dimensions.

Do not place OAuth client secrets or refresh tokens in this service. Future connector activation should use `integration-corev2` token leases with consumer `insight-core`.
