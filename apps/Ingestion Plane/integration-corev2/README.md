# integration-corev2

First-party Velion integration broker. This service replaces the existing
Ingestion Plane `integration-core` v1 and removes Nango as the owner of OAuth
sessions, token storage, capability consent, provider connections, and internal
token brokering.

Nango was used as a reference for the primitives Velion needs:

- Auth: provider OAuth, token refresh, multi-tenant connections.
- Proxy/token broker: internal services get short-lived provider tokens without
  storing credentials themselves.
- Functions/actions: later worker layer for provider-specific sync and actions.

The implementation is first-party and Velion-owned. It does not depend on
Nango packages or Nango runtime services.

## Current scope

- Go API service on `:3026`.
- Control Plane integration matching v1:
  - Bearer user requests are verified through auth-core
    `POST /internal/sessions/verify`.
  - User-facing connect/session routes are plan-gated through org-core
    `GET /orgs/{id}`.
  - Usage is recorded to billing-core
    `POST /api/v1/billing/orgs/{id}/usage`.
  - Integration audit is stored locally and forwarded best-effort to audit-core
    `POST /v1/audit` when configured.
  - Internal service calls use the shared `x-internal-api-key`.
- Provider catalog for Microsoft 365, Slack, Google Workspace, Notion, GitHub,
  Shopify, Stripe, Okta, and SCIM.
- Direct OAuth connect-session generation for Microsoft, Slack, Google, Notion,
  GitHub, Shopify, and Stripe. Provider credentials are required before a
  provider can be connected. Okta is an admin/API-token integration and SCIM is
  inbound provisioning, so they are not exposed through user OAuth sessions.
- Reconnect sessions reuse the active org/provider connection when one exists,
  so scope and capability upgrades keep the same connection ID and audit
  history.
- Provider readiness on `GET /api/v1/providers`, including `configured`,
  `status`, and `missingConfig`, so Velion UI only shows connectable providers
  when required credentials exist.
- Capability bundles for onboarding, knowledge sync, inbox, and full workspace.
- Connection capabilities and source consent APIs.
- Encrypted access/refresh token storage.
- Safe metadata discovery endpoint for connected providers.
- Named provider action execution for the old integration-service capability
  surface without exposing a raw arbitrary provider proxy.
- Okta admin/API-token actions for org metadata, directory reads, and approved
  lifecycle writes.
- Sync job queue records, sync event SSE snapshots, webhook intake, and token
  lease audit records.
- Optional Rust webhook hot-path worker for deterministic provider event
  normalization, replay key generation, and stable webhook IDs before Go stores
  the event.
- Configurable rate limiting on connect sessions, token leases, provider
  actions, sync queueing, webhook intake, and SCIM provisioning.
- Request ID propagation through `X-Request-ID`; IDs are echoed in response
  headers, response metadata, request logs, and forwarded audit events.
- Internal Prometheus-style `/metrics` endpoint for request counters and
  accumulated route duration by method, route template, and status.
- Best-effort provider revocation on disconnect, followed by local tombstone and
  audit.
- Optional NATS lifecycle event publishing for unified/profile/knowledge
  projections.
- `cmd/finspo-worker` for Microsoft handoff jobs. It claims `finspo-core`
  Microsoft jobs, ensures a Finspo source, starts Finspo source sync, and
  completes the integration checkpoint with a safe source reference. The job
  checkpoint or metadata must include SharePoint `site_id` and `drive_id`
  (`siteId`/`driveId` aliases are accepted).
- Compatibility endpoints used by the existing stack:
  - `GET /api/v1/providers` (public catalog, matching v1)
  - `POST /api/v1/providers/{provider}/connect-session`
  - `POST /api/v1/providers/{provider}/connect`
  - `GET /api/v1/connections`
  - `GET /api/v1/connections/{id}/status`
  - `GET /api/v1/connections/{id}/capabilities`
  - `PATCH /api/v1/connections/{id}/capabilities`
  - `GET /api/v1/connections/{id}/consents`
  - `POST /api/v1/connections/{id}/consents`
  - `GET /api/v1/connections/{id}/discovery`
  - `POST /api/v1/connections/{id}/actions`
  - `POST /api/v1/actions/execute`
  - `POST /api/v1/connections/{id}/sync`
  - `POST /api/v1/sync-jobs`
  - `GET /api/v1/sync-jobs`
  - `GET /api/v1/sync-jobs/{id}`
  - `POST /api/v1/sync-jobs/{id}/cancel`
  - `POST /api/v1/sync-jobs/{id}/retry`
  - `GET /api/v1/sync-jobs/{id}/events`
  - `POST /internal/sync-jobs/claim` (internal workers only)
  - `PATCH /internal/sync-jobs/{id}/progress` (internal workers only)
  - `POST /api/v1/webhooks/{provider}`
  - `GET /api/v1/scim/tokens`
  - `POST /api/v1/scim/tokens`
  - `DELETE /api/v1/scim/tokens/{id}`
  - `ALL /api/v1/scim/v2/*`
  - `GET /api/v1/projections/integration-profile`
  - `GET /metrics` (internal auth only)
  - `DELETE /api/v1/connections/{id}`
  - `POST /internal/connectors/token`
  - selected `/integrations/...` compatibility routes from the old NestJS
    integration-service, backed by the named action executor.
- OAuth callback that posts a browser message back to Velion UI.

## Microsoft capability bundles

| Bundle | Capabilities | Important scopes |
| --- | --- | --- |
| `onboarding` | profile, SharePoint/OneDrive read, Teams metadata | `User.Read`, `Files.Read.All`, `Sites.Read.All`, `Team.ReadBasic.All`, `Channel.ReadBasic.All` |
| `knowledge` | same as onboarding | same as onboarding |
| `inbox` | profile, mail read, mail send | `User.Read`, `Mail.Read`, `Mail.Send` |
| `full` | knowledge + inbox + calendar | adds `Mail.Read`, `Mail.Send`, `Calendars.Read` |

Write/destructive SharePoint actions are deliberately separate through
`sharepoint.write` (`Files.ReadWrite.All`, `Sites.ReadWrite.All`) and should be
enabled only behind human approval and admin consent flows.

## Connect-session provider context

Most providers only need org/user/workspace context. Shopify also needs a shop
domain because its OAuth URLs are shop-scoped:

```json
{
  "organizationId": "org_123",
  "workspaceId": "org_123",
  "userId": "user_123",
  "providerContext": {
    "shop": "example.myshopify.com"
  },
  "bundles": ["onboarding"]
}
```

The short form `{ "shop": "example" }` is accepted and normalized to
`example.myshopify.com`.

## Running locally

```bash
cp .env.example .env
openssl rand -base64 32
go test ./...
go run ./cmd/api
```

Optional Rust webhook hot path:

```bash
cd services/webhook-normalizer-rs
cargo test
PORT=3036 cargo run
```

Then set `INTEGRATION_WEBHOOK_HOTPATH_URL=http://localhost:3036` for the Go
service. If the Rust worker is not configured or is temporarily unavailable,
Go falls back to the same deterministic normalization rules locally.

For a throwaway local run without Postgres:

```bash
INTEGRATION_ALLOW_IN_MEMORY_STORE=true \
DATABASE_URL= \
INTERNAL_API_KEY=dev \
INTEGRATION_CREDENTIALS_ENCRYPTION_KEY="$(openssl rand -base64 32)" \
AZURE_CLIENT_ID=placeholder \
AZURE_CLIENT_SECRET=placeholder \
go run ./cmd/api
```

## Security model

- Provider tokens never go to the browser.
- User-facing routes accept `Authorization: Bearer ...` and verify the session
  with auth-core before deriving org/user/workspace context.
- Trusted service routes accept `x-internal-api-key` and use the shared
  Control Plane internal secret.
- Bearer callers are always scoped to their auth-core organization; query/body
  attempts to act on another organization are ignored or rejected.
- Connect-session creation is gated by org-core plan, matching v1. Internal
  service calls bypass the plan gate for onboarding and orchestration flows.
- Tokens are encrypted with AES-GCM using
  `INTEGRATION_CREDENTIALS_ENCRYPTION_KEY`.
- Internal token access requires `X-Internal-API-Key`.
- Finspo and other planes request short-lived access tokens through
  `/internal/connectors/token` with either `{ "connectionId": "conn_..." }` or
  `{ "organizationId": "org_...", "connectorType": "microsoft-graph" }`.
  Callers must include a stable `consumer` such as `finspo-core`; token leases
  are audited against that consumer identity before any provider token leaves
  this service. `INTEGRATION_TOKEN_LEASE_CONSUMERS` is the allowlist for these
  internal consumers.
- Expired provider tokens are refreshed through in-process singleflight and,
  with the Postgres repository, a transaction-scoped advisory lock per
  connection. A replica re-reads the connection under the lock before calling
  the provider so it can reuse a token another replica already refreshed.
- Capability bundles make scope upgrades explicit and auditable.
- Discovery returns only `safe_metadata_only` snapshots: workspace identity,
  availability flags, counts, and bounded public/allowed samples. Lightweight
  onboarding discovery does not expose provider tokens, email/message content,
  private Drive folder names, private file names, issue bodies, or document
  contents.
- Webhook signature verification and provider secrets stay in Go. Stripe,
  Slack, GitHub, and Shopify signatures are verified when the matching secret is
  configured. The Rust hot path receives only provider key, bounded headers, and
  body bytes, then returns `schemaVersion`, event type, organization hint,
  signature hash, body hash, replay key, stable webhook ID, and parsed payload.
- Action execution is named and provider-specific. There is no unbounded
  external API proxy endpoint. Write actions such as mail send, Slack post, and
  Okta lifecycle actions require human approval metadata (`approvalId` or
  `approvalRef`) before token lookup or provider calls.
- SCIM supports durable per-org bearer tokens through `/api/v1/scim/tokens`.
  Tokens are stored only as SHA-256 hashes and short prefixes; the raw bearer is
  returned once on create. Legacy global `SCIM_BEARER_TOKEN` and env-scoped
  `SCIM_ORG_BEARER_TOKENS` remain as bootstrap fallbacks. SCIM calls do not
  create OAuth sessions.

## Action execution

Callers execute named provider operations against an exact Velion connection:

```json
{
  "connectionId": "conn_123",
  "operation": "slack.channels.list",
  "params": { "types": "public_channel" }
}
```

Supported operation families:

- Microsoft: `profile`, `calendar.events`, `mail.messages`, `drive.files`,
  `mail.send`
- Slack: `channels.list`, `users.list`, `user`, `messages.list`,
  `message.send`
- Google: `profile`, `gmail.messages`, `gmail.send`, `calendar.events`,
  `drive.files`
- GitHub: `user`, `orgs`, `teams`, `repos`, `repo`
- Notion: `user`, `databases`, `pages`
- Shopify: `shop`, `products`, `orders`
- Stripe: `account`, `customers`, `subscriptions`, `invoices`
- Okta: `org`, `users`, `groups`, `user.suspend`, `user.activate`

## NestJS compatibility routes

The old ID-Knuten integration-service exposed provider-shaped routes. Go keeps
the useful route names for migration, but every route still requires internal
auth and resolves a Velion connection before calling a whitelisted action.

Pass either:

- `connectionId` as a query parameter or `X-Connection-ID` header.
- `organizationId` as a query parameter or `X-Org-ID` header. The route's
  connector type is then used to resolve the active connection.

Supported compatibility routes:

- Microsoft: `GET /integrations/ms-graph/me`,
  `GET /integrations/ms-graph/calendar/events`,
  `GET /integrations/ms-graph/mail/messages`,
  `POST /integrations/ms-graph/mail/send`.
- Microsoft docs alias:
  `GET /integrations/microsoft/graph/user/profile`,
  `GET /integrations/microsoft/graph/user/calendar`,
  `GET /integrations/microsoft/graph/user/mail`,
  `POST /integrations/microsoft/graph/mail/send`.
- Google: `GET /integrations/google/profile`,
  `GET /integrations/google/gmail/messages`,
  `POST /integrations/google/gmail/send`,
  `GET /integrations/google/calendar/events`,
  `GET /integrations/google/drive/files`.
- Slack: `GET /integrations/slack/channels`,
  `GET /integrations/slack/users`, `GET /integrations/slack/user/{userId}`,
  `GET /integrations/slack/messages/{channelId}`,
  `POST /integrations/slack/message`, `POST /integrations/slack/messages`.
- GitHub: `GET /integrations/github/user`,
  `GET /integrations/github/organizations`,
  `GET /integrations/github/organizations/{org}/teams`,
  `GET /integrations/github/repositories`, `GET /integrations/github/repos`,
  `GET /integrations/github/repos/{owner}/{repo}`.
- Notion: `GET /integrations/notion/user`,
  `GET /integrations/notion/databases`, `GET /integrations/notion/pages`,
  `GET /integrations/notion/pages/{databaseId}`.
- Shopify: `GET /integrations/shopify/shop`,
  `GET /integrations/shopify/products`, `GET /integrations/shopify/orders`.
- Stripe: `GET /integrations/stripe/account`,
  `GET /integrations/stripe/customers`,
  `GET /integrations/stripe/subscriptions`,
  `GET /integrations/stripe/invoices`.
- Okta: named actions are available through `/api/v1/connections/{id}/actions`
  or `/api/v1/actions/execute`: `okta.org`, `okta.users`, `okta.groups`,
  `okta.user.suspend`, and `okta.user.activate`.

Raw proxy routes return `410 Gone`. That is deliberate: provider actions must
be explicit, auditable, and capability-bound.

## Sync handoff

`POST /api/v1/sync-jobs` and `POST /api/v1/connections/{id}/sync` create a
durable sync intent, write a `sync.queued` event, then advance the job to the
first safe orchestration boundary:

- Microsoft 365 jobs move to `waiting_provider` and emit
  `sync.waiting_provider`; SharePoint and OneDrive inventory are delegated to
  `finspo-core`, which obtains provider access through the internal token
  broker and hands source evidence to Data Plane v2.
- Other providers move to `handoff_data_plane` and emit
  `sync.handoff_data_plane`; deeper content ingestion is explicit and owned by
  Data Plane v2 workers, not onboarding discovery.
- `finspo-core` and `data-plane-v2` claim handoff work through
  `POST /internal/sync-jobs/claim`. Claims are internal-only, require an
  allowlisted `consumer`, atomically move a job to `running`, and record
  `sync.claimed`.
- Workers advance checkpoint/status through
  `PATCH /internal/sync-jobs/{id}/progress`. Checkpoints merge safe source
  references idempotently, so repeated worker callbacks do not duplicate a
  Data Plane or Finspo source reference.
- Worker implementations can use the Go handoff clients in `internal/handoff`
  for the contract edges:
  - `FinspoClient.EnsureSource` calls Finspo `POST /api/v1/sources` with
    `FINSPO_API_KEY_HEADER` (default `X-API-Key`), `X-Org-ID`, and optional
    `X-User-ID`.
  - `FinspoClient.SyncSource` calls Finspo
    `POST /api/v1/sources/{id}/sync`.
  - `DataPlaneDocumentsClient.CreateDocument` calls Data Plane
    `POST /internal/v1/documents` with `DATA_PLANE_INTERNAL_API_KEY_HEADER`
    (default `X-Internal-Api-Key`) and `X-Org-Id`.
  - `IntegrationClient.ClaimSyncJob` and `UpdateSyncProgress` call the
    internal integration-corev2 worker endpoints using `INTERNAL_API_KEY`.
  These clients do not store source content in integration-corev2 and return
  redacted service errors so API keys are not leaked into worker logs.
- `cmd/finspo-worker` is the first concrete worker loop over these contracts.
  It idles when `POST /internal/sync-jobs/claim` returns no available work,
  fails a claimed job with a visible sync event if required SharePoint source
  identifiers are missing, and treats a successful Finspo source-sync request as
  completion of the integration-corev2 handoff.
- Pending or handed-off jobs can be moved to `cancelled` through
  `POST /api/v1/sync-jobs/{id}/cancel`; this is a reversible UX control for
  stopping onboarding/settings work without deleting historical job evidence.

Browser/UI callers should consume the Velion v2 BFF sync routes and SSE stream.
They must not call the internal token broker or infer proof evidence from
optimistic UI nodes.

## Events

When `NATS_ENABLED=true`, the service publishes:

- `integration.connected`
- `integration.disconnected`
- `velion.ingestion.integration.sync_started`
- `velion.ingestion.integration.sync_handoff`
- `velion.ingestion.integration.connection_updated`
- `velion.ingestion.integration.consent_changed`
- `velion.ingestion.integration.webhook_received`

Set `NATS_SUBJECT_PREFIX=velion.events` to publish subjects such as
`velion.events.integration.connected`.

## Provider migration blueprint

See [docs/PROVIDER_BLUEPRINT.md](docs/PROVIDER_BLUEPRINT.md) for how the Go
integration core uses Nango and the existing NestJS services as blueprints while
keeping OAuth, tokens, refresh, revocation, audit, and token leasing first-party.

## Unified-service replacement

The old unified-service profile and real-time features are split by ownership:

- `integration-corev2` owns integration identity, connection state, capability
  consent, sync events, webhook records, token leases, and integration audit.
- Velion v2 BFF/UI owns real-time delivery, profile views, settings UX, source
  inspectors, and onboarding/dashboard presentation.
- Data Plane v2 owns source records, graph evidence, indexing, and retrieval
  state.
- Control Plane remains the source of truth for users, sessions, organizations,
  roles, and plans.

`GET /api/v1/projections/integration-profile` is a lightweight integration read
model for Velion UI/BFF composition. It is not a replacement user profile store
and does not own matching or AI interpretation.
