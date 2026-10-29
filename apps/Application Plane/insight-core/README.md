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

The connector endpoint is a truthful marketing registry. It currently lists:

- Google Search Console, GA4, and Google Ads — require an allowlisted
  `integration-corev2` governed-action adapter. GSC has read-only operation
  contracts for `gsc.sites.list` and a bounded `gsc.search_analytics.query`;
  GA4 and Google Ads remain registry-only until their fixed reporting adapters
  exist.
- Meta Ads — factual metric snapshots already flow from `social-core`; it is
  not a second OAuth or metric store.
- SEMrush — requires a future encrypted API-key broker in `integration-corev2`
  and is deliberately not shown as connectable.
- First-party SEO evidence — must arrive from authorized Quarry/Data Plane
  contracts with provenance. `insight-core` never stores raw crawls, sitemaps,
  or provider credentials.

Every marketing slot declares the credential-free `marketing_evidence.v1`
grounding shape for future Model Plane analysis. Evidence must include a source
trace and observation window, and Zero Data Retention material is excluded. The
registry does not implement an AI recommender or authorize campaign, budget,
content, sitemap, or publishing changes.

Google Search Console and Meta Ads now declare explicit integration steps in
the connector registry. Their `test_read_only_access` step points to the
protected Integration Core readiness endpoint, which performs a fixed request
only after a connection is authorized. The UI or gateway must supply the
existing connection ID; no client supplies a provider operation or account.

The GSC slot also publishes reviewed **advisory instruction packs** that map
the installed SEO and marketing skill IDs to short, credential-free guidance.
They are metadata for an AI consumer to select after readiness succeeds—not a
filesystem interpreter, an agent runtime, MCP configuration, or executable
third-party skill content. They cannot grant provider access or perform writes.

OAuth client secrets, refresh tokens, provider access tokens, and SEMrush API
keys must never be placed in this service, returned by its API, or sent to the
browser. Provider execution, credential storage, rate limiting, and audit stay
in `integration-corev2`; Studio and the frontend only compose normalized,
organization-scoped results.
