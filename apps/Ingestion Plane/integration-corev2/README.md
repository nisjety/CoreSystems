# integration-corev2

Pulse's connection picker can read protected installation metadata through
`POST /api/v1/insights/site-sources/binding` and
`POST /api/v1/insights/web-vitals/binding`. These accept the same strict
site/website-ID/revision/provider or environment context as their reporting
counterparts, under the existing verified organization and read scopes. They
return only `data.configured` in the standard envelope. They reload protected
bindings without fetching reports, launching tracking or exposing keys, origins,
tokens, paths or credential fingerprints.

First-party Verevon integration broker. This service replaces the existing
Ingestion Plane `integration-core` v1 and removes Nango as the owner of OAuth
sessions, token storage, capability consent, provider connections, and internal
token brokering.

Nango was used as a reference for the primitives Verevon needs:

- Auth: provider OAuth, token refresh, multi-tenant connections.
- Proxy/token broker: internal services get short-lived provider tokens without
  storing credentials themselves.
- Functions/actions: later worker layer for provider-specific sync and actions.

The implementation is first-party and Verevon-owned. It does not depend on
Nango packages or Nango runtime services.

## Current scope

### Algolia user-authorized analytics and search-term shortlists

Live compatibility status (23 October 2026): OAuth consent, MCP initialization
and authenticated tool listing work. The provider currently advertises only
`algolia_discover_tools`, `algolia_describe_tool` and
`algolia_call_read_tool`; its documented leaf operations are not direct MCP
tools. The bounded discover/describe/read-call adapter is implemented and its
discover/describe/read transport has been checked against the live account.
Hosted application discovery uses the fixed read operation
`algolia_application_list` and the verified `applications[].applicationId`
envelope, not an enum inferred from list-indices. Pulse automatically received
Aquatiq's `JL96OC7GC2` application and its real `development`/`everything`
indices. A real read-only service probe for Aquatiq's `everything` index then
returned connected aggregate, top-search and no-result-term components for
16–22 October 2026, with twenty provider rows in each bounded term list. No
query values were logged. This proves Integration's provider receipt, not that
a downstream site mapping or background job has been saved successfully.
Provider-authorized reporting does not require our own DNS
ownership gate; this does not remove Algolia's permissions or prove that an app
belongs to a domain. Do not broaden generic token leasing, infer permissions
from prose/defaults or automatically map Aquatiq resources to Triodelab.

`ALGOLIA_MCP_OAUTH_CLIENT_FILE` points to an absolute private mode-0600 regular
JSON file: exactly `{version:1,clientId,redirectUri,resource}`. Resource is fixed
to `https://mcp.algolia.com/mcp`; redirect equals `PUBLIC_BASE_URL` plus
`/api/v1/providers/algolia/callback`. This is an Integration-owned public client,
not the Algolia CLI client. OAuth uses PKCE S256, scope `public`, resource
indicators, no secret, and existing encrypted grant/session/refresh storage.
The user must enable Productivity MCP and approve account access. Every consent
creates an independent grant instead of overwriting an unidentified account.

User/org-verified discovery (grant owner or org administrator):

- `GET /api/v1/providers/algolia/connections/:id/applications`
- `GET /api/v1/providers/algolia/connections/:id/indices?applicationId=...`

Verified org bearers with `integration:read`, without internal-key bypass:

- `POST /api/v1/insights/algolia/access`: exact `{connectionId}`, returns local
  `{connectionId,status,generation}`; status `active|reconnect_required`.
- `POST /api/v1/insights/algolia/resources`: exact
  `{connectionId,applicationId,indexName}`, returns those fields plus
  `status:"allowed"|"unavailable"` after real discovery.
- `POST /api/v1/insights/algolia/report`: exact
  `{websiteId,site,websiteRevision,connectionId,applicationId,indexName}`,
  returns the minimal site-source Report with Algolia region `provider`.

Responses are exactly `{success:true,data:...}`. Unknown/duplicate fields,
unsafe schemas, wrong organizations and unmapped resources fail closed. Calls
target only the fixed MCP endpoint: no redirects/cookies, 2 MiB responses,
concurrency two, a 20-second/fourteen-RPC metadata budget and a bounded
30-second/eighteen-RPC report budget. Both direct
logical tools and the hosted `algolia_discover_tools` → `algolia_describe_tool` →
`algolia_call_read_tool` transport are implemented. Wrapper calls require actual
outer schemas; only fixed metadata queries are sent, never user prompts or the
optional `originalQuery`/`userIntent` fields. Described operations must exactly
match the six fixed account/index/analytics operations and have independently confirmed `read`
effects. Unsupported schema composition/references fail closed. Description
caches exist only within the current grant-checked request. Hosted application
discovery validates the no-argument read and projects at most 128 unique IDs;
direct-leaf compatibility can still use a validated enum. An explicit user ID
may attempt a schema-validated fixed index list, but only the successful provider
response proves resource access. Any present enum still restricts the request.
Schema defaults or prose descriptions are never permission evidence.

User-authorized reports may include at most twenty actual search terms per
component; raw hit records, user identifiers, tool descriptions, schemas and
tokens never leave Integration. Search terms never appear in logs. Safe
troubleshooting projects bounded counts/type/presence flags
and narrowly filtered protocol identifiers, never metadata values. Wrapper
transport and hosted application/index enumeration have been exercised with real
account consent; the aggregate and term envelopes have also passed the real
read-only service probe described above. Unsupported shapes stay unavailable;
fixtures are not live data.

New Algolia receipts include independent optional `topSearches` and
`noResultTerms` components; old stored reports may omit them. Each is
`{status:"connected"|"unavailable",limit:20,rows?:[{query,count,averageResults?}]}`.
A connected receipt always has an explicit rows array, including `[]` only when
the provider observed an empty array. Unavailable components omit rows. Query
text is preserved, including empty searches, within 512 Unicode code points /
2048 UTF-8 bytes without C0/C1 controls. Counts and average results are bounded
to JavaScript-safe nonnegative values; averages must be finite. The known
Analytics source envelope is `searches:[{search,count,nbHits?}]`; other source
metadata such as `withFilterCount` is discarded. Duplicate/oversized lists,
ambiguous envelopes and malformed metrics fail closed without partial rows.

The fixed `algolia_analytics_top_searches` and
`algolia_analytics_searches_no_results` operations must be independently
discovered/described with read effects and verified schemas. Application,
index and the same seven completed UTC days are fixed by the authenticated
request. A typed limit is set to twenty where exposed; source lists exceeding
twenty rows are rejected, never silently truncated. `direction:"desc"` and
`orderBy:"searchCount"` are requested only when the actual schema exposes and
permits them. Missing analytics permission or one failed component does not
erase independently measured components. A report is connected when any
component is connected, but revocation, organization mismatch or a UTC rollover
discards the whole receipt. Private snapshots remain the downstream owning
core's organization-scoped, grant-checked responsibility.

Generic token leases explicitly deny this resource-bound grant. Local disconnect
and provider-observed HTTP 401 hide access; stale rejections cannot poison a
concurrent replacement. Nonsecret grant generations remain stable across normal
refresh. Provider-side revocation is not claimed until its endpoint contract is
verified; users can revoke in Algolia's dashboard. Existing restricted analytics
key bindings remain a separate fallback. This flow creates no applications,
indices, keys, subscriptions or tracking tags.

Sources: [Productivity MCP documentation](https://www.algolia.com/doc/guides/model-context-protocol/productivity-mcp),
[advertised OAuth metadata](https://dashboard.algolia.com/.well-known/oauth-authorization-server).

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
  `status`, and `missingConfig`, so Verevon UI only shows connectable providers
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
  - `POST /api/v1/connections/{id}/readiness` (fixed, read-only Google Search
    Console or Meta Ads connection test; no body or query parameters)
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
- OAuth callback that posts a browser message back to Verevon UI.

## OAuth browser-return rollout

Before any browser client sends `returnUrl` to a connect-session endpoint,
configure these deployment-owned values and deploy them together:

- `INTEGRATION_PUBLIC_BASE_URL` is the stable public callback origin. Register
  its exact callback paths, such as
  `https://integrations.example/oauth/callback/google` and
  `https://integrations.example/oauth/callback/meta`, in the provider consoles.
- `INTEGRATION_ALLOWED_RETURN_ORIGINS` is a comma-separated list of exact UI
  origins permitted to receive the post-OAuth return, for example
  `https://app.example`. Wildcards, paths, queries, fragments, and credentials
  are rejected.
- Production URLs must use HTTPS. Local HTTP is accepted only for loopback
  hosts (`localhost`, `127.0.0.1`, or `::1`) when `ENVIRONMENT` explicitly
  names `local`, `dev`, `development`, or `test`. An omitted environment fails
  closed as production.

Clients that do not need a browser redirect may omit `returnUrl` and poll
`GET /api/v1/connect-sessions/{id}/status`. During a rollout, update existing
clients to an allowed origin before removing an older origin. If an in-flight
session contains an origin that has since been removed, the connection still
completes safely and is visible through the status endpoint, but the callback
window remains open and does not redirect or send `postMessage` to that origin.

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
MICROSOFT_CLIENT_AUTH_MODE=public \
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
  Okta lifecycle actions require both an org-bound `integration:write` service
  bearer and a signed conversation-core provider-write attestation before token
  lookup or provider calls. Caller-provided `approvalId`/`approvalRef` markers
  are not authorization.
- SCIM supports durable per-org bearer tokens through `/api/v1/scim/tokens`.
  Tokens are stored only as SHA-256 hashes and short prefixes; the raw bearer is
  returned once on create. Legacy global `SCIM_BEARER_TOKEN` and env-scoped
  `SCIM_ORG_BEARER_TOKENS` remain as bootstrap fallbacks. SCIM calls do not
  create OAuth sessions.

## Action execution

Callers execute named provider operations against an exact Verevon connection:

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
  `drive.files`, `gsc.sites.list`, `gsc.search_analytics.query`
- GitHub: `user`, `orgs`, `teams`, `repos`, `repo`
- Notion: `user`, `databases`, `pages`
- Shopify: `shop`, `products`, `orders`
- Stripe: `account`, `customers`, `subscriptions`, `invoices`
- Okta: `org`, `users`, `groups`, `user.suspend`, `user.activate`

### Marketing connection readiness

`POST /api/v1/connections/{id}/readiness` is an explicit, protected test after
an OAuth connection exists. It has no request body or query parameters, so it
cannot be used as a provider proxy. The service selects one fixed read-only
action from the stored provider connection:

- Google Search Console: `GET /webmasters/v3/sites` through `gsc.sites.list`.
- Meta Ads: `GET /me/adaccounts` through `ads.adaccounts`, bounded to one
  account.

The response exposes only normalized state, aggregate count, required next
step, and timestamp. It never returns OAuth tokens, provider account IDs,
property URLs, names, scopes, or the upstream response. Each completed check
is recorded as `connection.readiness.checked` with its fixed operation and
normalized state.

### Provider-write attestation v1

Effectful operations use a top-level `writeAttestation` compact JWS. The JWS
header is exactly `alg=EdDSA`,
`typ=verevon.provider-write-attestation+jwt`, and a configured nonempty `kid`.
The service trusts only standard-base64, 32-byte Ed25519 public keys from
`INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON`; startup rejects missing,
placeholder, duplicate, malformed, symmetric, or non-conversation issuers.

Required claims are `v=1`, `iss=conversation-core`,
`aud=integration-corev2`, `presenter_service`, `authorization_kind`,
`authorization_id`, `action_id`, `org_id`, `connection_id`, `provider_key`,
`operation`, `actor_id`, `payload_sha256`, `idempotency_key`, `jti`, `iat`,
`nbf`, and `exp`. `authorization_kind` is `human_intent` or
`human_approved_ai_action`; `approval_id` must be absent for a human intent and
its `action_id` must equal `authorization_id`. For approved AI actions,
`approval_id` must equal `action_id`. `iat` must equal `nbf`, and `exp-iat`
must be between one and sixty seconds. Every claim is checked against the
verified bearer and exact request before OAuth token resolution.

`payload_sha256` is lowercase SHA-256 over Go `encoding/json` output for this
field order and spelling (maps are key-sorted by `encoding/json`):

```json
{
  "org_id": "org_123",
  "connection_id": "conn_123",
  "provider_key": "slack",
  "operation": "message.send",
  "params": {},
  "body": { "channel": "C123", "text": "Approved reply" }
}
```

The idempotency key and attestation identifiers are separate signed bindings.
Migration `0008_action_receipt_attestations.sql` stores hashes and identifiers,
never params, message bodies, or provider response payloads. Relationship
constraints are strengthened by the forward-only
`0009_action_receipt_authorization_relationships.sql` migration. It blocks
receipt writes, audits pre-existing attested rows using only an aggregate
mismatch count, and fails before replacing the old constraint if a human
intent has `action_id != authorization_id` or an approved AI action has
`approval_id != action_id`. The failed migration is rolled back and must not be
marked applied; operators must audit the affected receipts rather than
automatically rewriting completed or unknown provider effects.

Receipts are
`pending` until token resolution succeeds, atomically become `executing`
immediately before the provider call, then become `completed` or `unknown`.
An exact `pending` request can retry after a pre-provider token failure;
the API returns `503 action_pre_provider_retryable` so the caller can mint a
fresh short-lived attestation. A fresh `jti` and a rotated trusted `kid` are
allowed for the same pending authorization/effect while the receipt retains
the first-seen signer metadata. `executing` and `unknown` never retransmit
blindly. The same `(issuer, org_id, authorization_id)` cannot move to another
authorization kind, action, actor, payload, connection, provider operation, or
idempotency key. A cross-language WhatsApp `PrepareSend` fixture with the exact
send request, canonical JSON, digest, deterministic seed/random/time inputs,
public test key, claims, and regenerated full JWS is in
`testdata/provider_write_attestation_v1.json`.

## NestJS compatibility routes

The old ID-Knuten integration-service exposed provider-shaped routes. Go keeps
read-only route names for migration, but every route still requires internal
auth and resolves a Verevon connection before calling a whitelisted action.
Legacy write routes fail closed because an internal key cannot satisfy the
service-bearer plus signed-attestation contract; callers must migrate to
`POST /api/v1/connections/{id}/actions` or `POST /api/v1/actions/execute`.

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
    `POST /v1/documents` with a short-lived `aud=data-plane`,
    `documents:write` bearer minted from Auth Core for the verified handoff
    organization. The durable `INTEGRATION_SERVICE_API_KEY` is sent only to
    Auth Core; identity/shared-key headers are never forwarded to Data Plane.
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

Browser/UI callers should consume the Verevon v2 BFF sync routes and SSE stream.
They must not call the internal token broker or infer proof evidence from
optimistic UI nodes.

## Events

When `NATS_ENABLED=true`, the service publishes:

- `integration.connected`
- `integration.disconnected`
- `verevon.ingestion.integration.sync_started`
- `verevon.ingestion.integration.sync_handoff`
- `verevon.ingestion.integration.connection_updated`
- `verevon.ingestion.integration.consent_changed`
- `verevon.ingestion.integration.webhook_received`

Set `NATS_SUBJECT_PREFIX=verevon.events` to publish subjects such as
`verevon.events.integration.connected`.

## Provider migration blueprint

See [docs/PROVIDER_BLUEPRINT.md](docs/PROVIDER_BLUEPRINT.md) for how the Go
integration core uses Nango and the existing NestJS services as blueprints while
keeping OAuth, tokens, refresh, revocation, audit, and token leasing first-party.

## Unified-service replacement

The old unified-service profile and real-time features are split by ownership:

- `integration-corev2` owns integration identity, connection state, capability
  consent, sync events, webhook records, token leases, and integration audit.
- Verevon v2 BFF/UI owns real-time delivery, profile views, settings UX, source
  inspectors, and onboarding/dashboard presentation.
- Data Plane v2 owns source records, graph evidence, indexing, and retrieval
  state.
- Control Plane remains the source of truth for users, sessions, organizations,
  roles, and plans.

`GET /api/v1/projections/integration-profile` is a lightweight integration read
model for Verevon UI/BFF composition. It is not a replacement user profile store
and does not own matching or AI interpretation.

## Read-only website measurement

`ga4.reporting_host.resolve` is an `analytics.read` connection action. It accepts
only `site`, `propertyId`, and `streamId` with an empty body. It verifies the web
stream's default URI against the exact www/non-www pair, then requests at most
two hostname rows with a fixed stream filter over the last 28 completed UTC
days. It returns only the chosen `reportingSite`; counts are not exposed. GA4
report access is authorized by the connected Analytics resource, independently
of Search Console ownership. No provider edits or additional OAuth scopes are
used. Ambiguous streams remain a resource-selection state in Marketing Core.

Google connections granting `analytics.read` can execute `ga4.report.run` through
the existing organization-authorized connection action endpoint. Its only
parameters are numeric `propertyId`, numeric `streamId`, and canonical HTTPS
website `site`; its body contains `startDate` and `endDate` (at most 366 days,
ending before today UTC). Integration first verifies the existing web stream's
default URI. It then requests fixed Analytics Data API summary and daily reports
with exact hostname and stream filters. The action returns `{ summary, daily }`
provider report objects; consumers must normalize their fixed metric headers,
property timezone, and data-quality metadata. Unique users come from the
dimensionless summary, never a sum of daily users. The six fixed metrics are
`totalUsers`, `sessions`, `engagedSessions`, `screenPageViews`, `keyEvents`, and
`userEngagementDuration`. No properties, tags, events, or settings are created.

`GET /api/v1/insights/clarity?site=https%3A%2F%2Fexample.com%2F&numOfDays=3`
requires a verified organization principal (`integration:read` for services).
It never accepts a token, project identifier, dimensions, or provider URL from
the caller. Configure `CLARITY_BINDINGS_FILE` as an absolute deployment-owned
manifest path. Both manifest and token must be regular mode-`0600` files; token
filenames are basenames resolved beside the manifest:

```json
{
  "version": 1,
  "bindings": [{
    "organizationId": "approved-organization-id",
    "site": "https://example.com/",
    "projectId": "approvedprojectid",
    "tokenFile": "clarity_export_token_example",
    "confirmedAt": "2026-10-21T00:00:00Z"
  }]
}
```

`confirmedAt` records the operator's confirmation of the organization/project/
website mapping; API success alone cannot verify Clarity project identity or
website ownership. Tokens are generated by a Clarity project admin through an
explicitly approved provider workflow, then mounted through the deployment's
protected secret path. Do not put tokens in `.env`, source, browser storage, or
HTTP request bodies. Google/Microsoft sign-in is not a substitute for this token.

Clarity reports are explicitly `scope: "clarity_project"`: the mapping selects
the correct project credential but does not filter its metrics to one URL.
The API exposes only normalized Traffic metrics (`sessions`, `botSessions`,
`users`, `pagesPerSession`); absent metrics remain null. Its rolling window is
1, 2, or 3 days in UTC, not the selected Pulse quarter. Successful reports are
cached for six hours; failures have a one-hour cooldown. The local quota guard
allows at most eight provider requests per project per UTC day **per process**;
it resets on restart and does not claim to enforce a distributed quota. Provider
429 remains authoritative. Cached observation timestamps do not change on reads.
No replay URLs, visitor identifiers, page URLs, or raw provider errors are exposed.

### First-party Web Vitals exports

`POST /api/v1/insights/web-vitals` is a read-only export operation requiring a
verified organization principal (`integration:read` for services). Its only body
fields are `websiteId`, canonical HTTPS `site`, positive `websiteRevision`, and
`environment` (`production` or `development`); the body is bounded to 2 KiB.
Query parameters, unknown or duplicate keys, internal-key tenant bypasses, and
caller-selected upstream URLs or credentials are rejected.

Set `WEB_VITALS_BINDINGS_FILE` to an absolute regular mode-`0600` deployment
manifest. Its version-1 `installations` list is bounded to 32 records:

```json
{
  "version": 1,
  "installations": [{
    "organizationId": "approved-organization-id",
    "websiteId": "web_12345678901234567890123456789012",
    "websiteRevision": 1,
    "site": "https://example.com/",
    "environment": "production",
    "reportingOrigin": "https://www.example.com",
    "tokenFile": "/run/secrets/website_vitals_export_token",
    "paths": ["/", "/public-insights"]
  }]
}
```

The dedicated export token is read from its own absolute regular mode-`0600`
file, never from browser input. A production reporting origin must exactly match
the registered site host or its `www` alias. Integration issues only a GET to
that origin's `/api/web-vitals/export`, with that installation's bearer. No
cookies, organization headers, redirects, or global egress allowlist exceptions
are forwarded. Bindings and token fingerprints are rechecked after each read.
Missing bindings return `not_configured`; configured-source failures return
generic HTTP 503, not a fabricated empty report. Two concurrent exports and a
15-second deadline bound upstream work.

Exports are bounded to 2 MiB / 5,000 latest metric samples in the last seven
days. Measurements are client-reported `LCP`, `INP`, or `CLS`, not CrUX or
verified visitor identities. An ephemeral page-load UUID may be shared by its
three metric series; `(sampleId, metric)` must be unique, with consistent
approved path and coarse viewport. Unknown export fields are discarded. No IP,
query string, referrer, DOM entry, persistent visitor ID, or raw error is
returned. Marketing Core owns report aggregation and website authorization;
this adapter does not install tracking, provision site storage, or publish it.

The sole local exception requires Integration's explicit
`ENVIRONMENT=development` and an installation with
`environment: "development"`, `reportingOrigin: "http://localhost:3000"`.
Optionally set its protected `developmentBackendOrigin` to exactly
`http://host.docker.internal:3000` for Docker-to-host access. This uses an
isolated exact-host egress allowlist, cannot be selected by an API caller, and
is refused in production. Production and local measurements remain distinct.

### Cookiebot and Algolia read-only site sources

`POST /api/v1/insights/site-sources` accepts only
`{websiteId,site,websiteRevision,provider}` with `provider` equal to `cookiebot`
or `algolia`. A verified organization bearer with `integration:read` is
mandatory; internal-key tenant overrides are refused. Its bounded response is
exactly `{success:true,data:Report}`, without provider JSON or credentials.

Set `MARKETING_PROVIDERS_BINDINGS_FILE` to an absolute private regular
mode-`0600` file. Empty installations are a valid unconfigured deployment:

```json
{
  "version": 1,
  "installations": [{
    "organizationId": "approved-organization-id",
    "websiteId": "web_12345678901234567890123456789012",
    "websiteRevision": 1,
    "site": "https://example.com/",
    "cookiebot": {
      "domainGroupId": "12345678-1234-4234-8234-123456789012",
      "domain": "www.example.com",
      "culture": "en",
      "apiKeyFile": "/run/secrets/cookiebot_data_api_key"
    },
    "algolia": {
      "applicationId": "ABCDEFGHIJ",
      "indexName": "existing_index",
      "region": "de",
      "analyticsKeyFile": "/run/secrets/algolia_analytics_api_key"
    }
  }]
}
```

Omit a provider until its installation mapping and key are available. Bound key
files must be absolute regular mode-`0600` files, not symlinks or empty
placeholders. At most 32 website bindings are accepted. Bindings and key
fingerprints are rechecked before and after each upstream request; changing
ownership, revision, mapping or key invalidates an in-flight receipt.

Reports cover the last seven completed UTC dates, not the selected Pulse
quarter. Cookiebot returns consent-event totals and inventory category counts;
it never forwards cookie values, IPs or script contents. Consent days omitted by
the provider are not filled with zeros. Inventory includes the provider's scan
timestamp and is not proof of current compliance. See Cookiebot's documented
[consent statistics](https://support.cookiebot.com/hc/en-us/articles/4405045044882-Extracting-consent-data-via-API)
and [inventory API](https://support.cookiebot.com/hc/en-us/articles/360006346473-Extracting-cookie-information-via-API).

Algolia reads only aggregate searches/no-result rates for the explicitly mapped
existing index and `us`/`de` analytics region. The operator must provide an
analytics-ACL key restricted to that index, not an Admin key; this adapter does
not create keys or inspect/change their ACLs. An observed zero-search window
has zero counts and an explicit null rate. Search-term reads and search
installation are out of scope for this manual REST fallback; bounded authorized
terms use the separate OAuth/MCP report above. The provider's [regional Analytics API guidance](https://support.algolia.com/hc/en-us/articles/15066949059857-Why-is-the-analytics-API-returning-null-response)
explains region selection.

Fixed HTTPS hosts, disabled redirects/cookie jars, 8-second request timeouts,
20-second collection deadlines, two concurrent collections and 2 MiB upstream
body limits constrain work. Transport errors and provider messages are never
logged or returned: Cookiebot places its API key in the documented URL path.
Cookiebot remains a manual API-key provider. Algolia's catalog additionally
exposes OAuth readiness when the protected Integration-owned client registration
is configured. Neither readiness nor a browser session substitutes for the
organization-scoped grant and actual resource-access checks.
