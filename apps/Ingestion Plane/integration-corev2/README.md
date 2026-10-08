# integration-corev2

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

## Scope upgrades are additive and truthful (Microsoft, Google)

A Microsoft or Google connection belongs to one user and only ever widens when
that user connects another bundle:

- `connect-session` requests the union of the requested capabilities and the
  capabilities the user's one active connection already holds, and reconnecting
  keeps whatever the new authorization did not ask about. A user with several
  accounts connected gets the request as asked (the account is only known at the
  callback), and the reconnect still carries that account's grants over.
  `"replaceCapabilities": true` in the request body opts out: the connection
  ends up with exactly the requested capabilities.
- Microsoft's token response is the record of what was granted. When it carries a
  `scope`, only the capabilities whose scopes it lists are recorded, and a
  shortfall leaves the connection `needs_refresh`. Google requests
  `include_granted_scopes=true`, so a later bundle's token carries earlier grants.
  (Google's per-scope consent is not read back yet.)
- `PATCH /api/v1/connections/{id}/capabilities` refuses (400
  `capability_not_granted`, with `reconnectRequired: true`) a capability whose
  scopes are not in the connection's recorded scopes. Dropping a capability, and
  keeping one that is already recorded, are always accepted.
- A provider answer of 401, or a 403 about permissions, to a connector action
  comes back as 409 `reconnect_required` (with `providerStatus`, and the
  provider's short `providerCode` when it sent one) so a client can offer
  "reconnect to grant access". A 403 that is really throttling or quota
  exhaustion stays a plain `action_failed`. A refused provider write still
  leaves its receipt `unknown` (there is no failed state yet), so retrying it
  needs a fresh approval and idempotency key.

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
  `drive.file.content`, `sharepoint.sites`, `sharepoint.drives`, `mail.send`
- Slack: `channels.list`, `users.list`, `user`, `messages.list`,
  `message.send`
- Google: `profile`, `gmail.messages`, `gmail.send`, `calendar.events`,
  `drive.files`, `drive.file.content`
- GitHub: `user`, `orgs`, `teams`, `repos`, `repo`
- Notion: `user`, `databases`, `pages`
- Shopify: `shop`, `products`, `orders`
- Stripe: `account`, `customers`, `subscriptions`, `invoices`
- Okta: `org`, `users`, `groups`, `user.suspend`, `user.activate`

### Mail read params

`mail.messages` (Outlook) and `gmail.messages` (Gmail) take one neutral set of
optional `params`, validated before any provider call (a bad value is an error,
never silently dropped, except `maxResults`, which falls back to its default): `query` (free text in the provider's own search
syntax, at most 500 characters), `from` (an address or a display name),
`since` and `until` (`YYYY-MM-DD`, both days included), `folder` (`inbox`,
`sent`, `drafts`, `archive`, `deleted`), `unread`, `maxResults` (default 10,
at most 100; the agent read tool planned in F6 is to clamp to 25) and `pageToken`.

- Outlook: with `query` or `from` the list uses `$search` (KQL) and no
  `$filter` or `$orderby`; otherwise `$filter` plus `$orderby receivedDateTime
  desc`. `unread` on a search is applied to the returned page (`filteredOutRead`
  counts the dropped rows). The next page is `nextPageToken` beside `value`: the
  `@odata.nextLink`, which must be passed back unchanged. A token is refused
  unless it is an https mail-list link on the configured Graph host under
  `/v1.0/me/`. `mail.messages.get` also returns `toRecipients`, `ccRecipients`,
  `hasAttachments`, `conversationId`, `internetMessageId` and attachment
  metadata (`attachments`: id, name, contentType, size, isInline; never bytes).
- Gmail: `query`, `from`, `since` and `until` become `q`, `folder` and `unread`
  become `labelIds`, and `pageToken` is Gmail's own. The list stays ids-only
  (execution-core's `gmail.send` read-back lists 100 ids) unless `headers` is
  true: then each listed message gains `subject`, `from`, `to`, `cc`, `date`,
  `snippet` and `labelIds` from `messages.get?format=metadata`, at most 25 at a
  time five wide (the page is clamped to 25), and a message that fails becomes
  `{id, error}` without failing the list. `gmail.messages.get` also returns
  `to`, `cc`, `snippet` and `attachments` (filename, mimeType, size,
  attachmentId); attachment content is a separate, later operation.

### Calendar read params

`calendar.events` (Outlook and Google) takes `since`, `until`, `query`,
`maxResults` (default 10, at most 100) and `pageToken`; a bad value is an error
before any provider call. `since` and `until` are a date (`YYYY-MM-DD`: the whole
local day, `until` included), a local date-time (`YYYY-MM-DDTHH:MM`) or RFC 3339.
Days and local date-times are Europe/Oslo, and both providers answer in Oslo
local time. The window defaults to now through 14 days ahead; with a `query` and
no window it is 30 days back through 90 ahead; it is never longer than 366 days.
Given only `since`, the window runs a default span from it; given only `until`, it
ends there.

- Outlook: `GET /me/calendarView` with the window as UTC, `$orderby
  start/dateTime`, a trimmed `$select` and `Prefer: outlook.timezone="W. Europe
  Standard Time"`. Events do not support `$search`, so a `query` is applied here:
  every whitespace-separated term must appear (case-insensitive) in the subject,
  location, organizer, attendees or body preview of the fetched page. With a query
  `maxResults` defaults to 100 (the filter only sees the page it fetched), and the
  result adds `scanned` and `filteredOut`. The list stays under Graph's `value`.
- Google: `singleEvents=true`, `orderBy=startTime`, `timeZone=Europe/Oslo`, `q`
  and a `fields` mask that drops the bulky parts. The list stays under `items`.
- Both add `window` (start, end and zone actually used) and, with a query,
  `query`. The next page is `nextPageToken`: the provider's own next link or
  token wrapped together with the window and query of the first page (`cal1.` plus
  base64 JSON), passed back unchanged and alone. Wrapping stops the default window
  from moving between pages. An Outlook link is only admitted if it is an https
  `/v1.0/me/calendarView` link on the configured Graph host; when Graph reports
  more rows but the link cannot be handed on, `moreAvailable` is true without a
  token.
- The binary embeds the time zone database (`time/tzdata`); the runtime image has
  no zoneinfo.

### Files and SharePoint read params

`drive.files`, `drive.file.content` and, for Microsoft only, `sharepoint.sites`
and `sharepoint.drives` read the connected user's own files. They need the
capability the file listing already needs (`sharepoint.read` for Microsoft,
`drive.read` for Google), never ask for a write approval, and check their params
before any provider call (a text param of the wrong type, a dot segment as an id or a
text over 500 characters is an error; `maxResults` falls back to its default). Ids
that go into a URL path are percent-escaped.

- Microsoft `drive.files`: `query`, `folderId`, `driveId`, `maxResults` (default 10,
  at most 100) and `pageToken`. Without `driveId` the own OneDrive is read
  (`/me/drive`); with it the same requests go to `/drives/{driveId}`, which is how a
  SharePoint document library is read. A `query` searches the whole drive and wins
  over `folderId`. The search text is percent-escaped inside `search(q='...')`.
  `parentReference` is selected, so a hit says which folder it is in.
- Microsoft `drive.file.content`: `itemId` and an optional `driveId`. The bytes
  come back base64 (`content_base64`, `mime_type`, `size`); anything over
  1,000,000 bytes is refused outright, never cut. Graph's redirect to a
  pre-authenticated download is followed only to a documented Microsoft host
  (`sharepoint.com` included), and the bearer is not forwarded on that hop.
- `sharepoint.sites`: `query` (default `*`, every site the user can see) and
  `pageToken`; returns `id`, `displayName`, `webUrl`. `sharepoint.drives`: `siteId`
  (required) and `pageToken`; returns `id`, `name`, `driveType`, `webUrl`. The
  `siteId` of the first is the input of the second, and the library `id` of the
  second is the `driveId` of the first.
- Paging: a Graph listing's `nextPageToken` is its `@odata.nextLink`, passed back
  as `pageToken` unchanged and alone, and only by the operation that produced it.
  A link is admitted only if it is an https read-only listing (children, search,
  sites, a site's drives) on the configured Graph host under `/v1.0/`; anything else
  is refused before a request is made. A link Graph hands out in a shape that is
  not recognised gives `moreAvailable: true` and no token.
- Google `drive.files`: shared-drive items are allowed in (`supportsAllDrives`,
  `includeItemsFromAllDrives`), but the default listing is Drive's own `user` corpus
  (files the user created, opened or was given directly), so reading a whole shared
  drive needs its `driveId`, which narrows the listing to that drive (`corpora=drive`). `match` is `name` (the default, the
  picker's own file-name search) or `text` (`fullText`, inside the files). A
  listing narrowed by a text, a folder or a drive leaves out trashed files. Each
  file carries `parents` and, in a shared drive, `driveId`. `pageToken` and
  `nextPageToken` are Google's own.
- Google `drive.file.content`: `fileId`, `mimeType` and `exportAs`. A Docs Editors
  file has no bytes of its own and is exported: `.docx`, `.xlsx` or `.pptx` by
  default (what the picker attaches), or with `exportAs=text` as `text/plain`
  (Docs, Slides) or `text/csv` (Sheets, first sheet only). `exportAs` changes
  nothing for a file that is not a Docs Editors file. The result is the same shape
  for both; `export_extension` says which export was made. When `mimeType` is
  missing, one metadata request finds it first.
- Not yet shown against a live tenant or Google account (task F10): path-escaped
  site and drive ids, `$select` on the sites search, the exact shape of a drive
  `nextLink`, a slash inside a search text, and shared-drive listing with
  `corpora=drive`.

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

## ChatGPT subscription connections (Codex broker)

`openai-codex-subscription` is an opt-in, text-only provider for a user who
chooses to use their own eligible ChatGPT plan. It is intentionally not an
API-key importer and does not scrape a ChatGPT browser session.

**Sign-in.** Integration Core starts the Codex CLI device-code flow at
`POST /api/v1/model-subscriptions/openai-codex/connect`, returns the
verification URL and user code, and reports completion through
`GET /api/v1/model-subscriptions/openai-codex/connect/{connectionId}/{loginId}`.
The authenticated caller owns the resulting opaque `conn_*` connection. A
caller can revoke it with `DELETE
/api/v1/model-subscriptions/openai-codex/connections/{connectionId}`, which
runs Codex `account/logout` (revoke and delete). Login and logout still use the
Codex device-code flow; a native device-code login is a later phase.

**Two inference transports.**

- `direct` (opt-in) sends plain Responses API requests to
  `https://chatgpt.com/backend-api/codex/responses` over HTTP SSE with the
  connection's ChatGPT OAuth access token, the way OpenCode, Crush and pi do.
  It identifies itself honestly as `originator: verevon` with a `verevon/...`
  User-Agent, always sends `store:false` and `stream:true`, and never sends
  `max_output_tokens`, tools, or sampling parameters. A unary call is the same
  stream, collected into one answer. There is no process spawn per call.
- `app-server` (default) spawns the pinned Codex app-server per call. It stays
  the default until OpenAI confirms the direct transport for a hosted,
  multi-user product; its public statements so far cover individuals using
  their own account in a client. Every Codex process the broker starts
  (sign-in, sign-out, fallback) runs hardened: process-level `-c` flags turn
  off plugins, remote plugins, apps and connectors, skills, hooks, tool
  suggestion, shell and exec tools, web search, memories, analytics, feedback
  and history. Every MCP server the connection's `config.toml` declares is
  turned off by name (`-c mcp_servers.<name>.enabled=false`; an empty
  `mcp_servers={}` override would merge away to nothing). The broker decodes
  `config.toml` as TOML to find them, and refuses a fallback turn when the
  file does not decode or declares MCP servers it cannot turn off by name.
  A Codex process never inherits the broker's own secrets: `DATABASE_URL`
  and every variable whose name has a `KEY`, `TOKEN`, `SECRET`, `PASSWORD`
  or `CREDENTIAL` word are left out of its environment.
  `CODEX_SQLITE_HOME` and `-c sqlite_home` point at a per-process directory
  that is deleted afterwards, and startup removes any such directory a crash
  or redeploy left behind, so no `logs_2.sqlite` prompt log survives. The
  whole process group is killed on close, and shutdown ends pending sign-in
  processes. Compose runs the API with `init: true` so PID 1 reaps those
  processes. The API image links the vendored native Codex binary at
  `/usr/local/bin/codex-native`, resolved at build time the way the npm
  launcher (`bin/codex.js`) resolves it, and Compose points
  `CODEX_APP_SERVER_COMMAND` there, so a call starts no Node process; the
  broker then sets the launcher's `CODEX_MANAGED_PACKAGE_ROOT` and
  `CODEX_MANAGED_BY_NPM=1` itself. `thread/start` carries the model, service
  tier and reasoning effort (`model_reasoning_effort`) that `turn/start` uses,
  so Codex's startup-prewarm request can match the turn's first request; a
  turn with an output schema still differs from it in its text format, which
  the pinned protocol cannot set at `thread/start`. The broker logs
  `thread_effort_differs` when Codex reports another thread effort.
  `CODEX_APP_SERVER_THREAD_EFFORT=false` leaves the effort out of
  `thread/start` again (its earlier params, byte for byte; `turn/start` still
  sets it) and skips that check.

Switch to the direct transport without a rebuild by setting
`CODEX_SUBSCRIPTION_TRANSPORT=direct` and recreating the container.

**Token custody.** The Codex sign-in writes the connection's `auth.json` in its
`CODEX_HOME` below `CODEX_SUBSCRIPTION_HOME`. The broker is the only refresher
of that file: it re-reads it under a per-connection lock (plus a file lock
against a second broker process), refreshes 10 minutes before expiry and once
after a 401, and writes it back atomically in the Codex layout with mode 0600.
It also refreshes before spawning the fallback, so Codex never refreshes during
a call; when that refresh fails and Codex would refresh the token during the
call itself, no fallback process is started. A refresh that was sent always
completes and is stored, even when its caller gave up; if the file cannot be
written, the rotated token is kept in memory and written on the next call, and
no fallback process starts until it is, since Codex would read the consumed
refresh token from the file.
After a transient refresh failure the connection keeps its still-valid token
for 30 seconds instead of retrying on every call. Refresh tokens rotate, and
reusing one revokes the whole family, so run one broker replica per volume and
never run Codex by hand against a live connection home. Tokens never leave Integration Core: the database stores only
opaque connection metadata, capability consent, lifecycle status, and
metadata-only audit events; there is no credential-lease endpoint for this
provider; and Model Plane never receives a ChatGPT access token, refresh token,
or browser cookie. Logs carry only content-free fields (ids, status, timings,
token counts, sanitized error codes). Mount encrypted persistent storage at
`CODEX_SUBSCRIPTION_HOME`.

The Compose API image installs a pinned, reviewed release of the official
`codex` CLI (override `CODEX_VERSION` only after reviewing a release, and
re-run the offline hardening check). Configure a unique
`CODEX_SUBSCRIPTION_MODEL_PLANE_API_KEY`. Model Plane must use the same value as
`CODEX_SUBSCRIPTION_INTERNAL_API_KEY`, set
`CODEX_SUBSCRIPTION_INTEGRATION_CORE_URL`, allow-list models in
`CODEX_SUBSCRIPTION_MODELS`, and add `openai-codex-subscription` to
`INFERENCE_PROVIDER_ORDER`. The provider declares Global residency and no
verified ZDR commitment, so deployments that use Model Plane's strict residency
gate must explicitly allow that posture before registering it.

**Knobs.** The non-secret knobs are owned by the `integration-api` environment
block in `../docker-compose.yml`. An invalid value fails startup and is listed
in the provider's readiness.

| Env | Default | Meaning |
|---|---|---|
| `CODEX_SUBSCRIPTION_TRANSPORT` | `app-server` | `app-server` or `direct` |
| `CODEX_SUBSCRIPTION_DIRECT_BASE_URL` | `https://chatgpt.com/backend-api/codex` | https on host `chatgpt.com` only; no userinfo, query or fragment |
| `CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR` | `verevon` | `^[a-z][a-z0-9_-]{1,63}$`; Codex, OpenCode, Crush and pi names are rejected |
| `CODEX_SUBSCRIPTION_DIRECT_REQUEST_SHAPE` | `classic` | `classic` (top-level instructions) or `lite` (Codex's lite shape) |
| `CODEX_SUBSCRIPTION_DIRECT_PROMPT_MODE` | `roles` | `roles` (system, developer, user and assistant items) or `flattened` (one user turn, as the app-server sent it) |
| `CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES` | `2` | 0 to 3 extra attempts, only before any answer text |
| `CODEX_SUBSCRIPTION_REFRESH_SKEW` | `10m` | 1m to 24h; a startup warning below 5m plus the invocation timeout |
| `CODEX_SUBSCRIPTION_MAX_CONCURRENCY` | `16` | concurrent calls across all connections |
| `CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION` | `4` | 1 up to the global value |
| `CODEX_SUBSCRIPTION_QUEUE_TIMEOUT` | `30s` | wait for a slot, at most the invocation timeout; then 429 `rate_limited` |
| `CODEX_APP_SERVER_HARDENED` | `true` | `false` restores the pre-hardening Codex arguments |
| `CODEX_APP_SERVER_THREAD_EFFORT` | `true` | `thread/start` sends the turn's `model_reasoning_effort` so the startup prewarm matches the turn; `false` restores the earlier `thread/start` params |
| `CODEX_APP_SERVER_COMMAND` | `/usr/local/bin/codex-native` (Compose; `codex` in code) | the Codex executable; `codex` runs the npm Node launcher, and startup falls back to it (warning `app_server_command_missing`) when an absolute path does not exist in the image |
| `INTEGRATION_API_CPU_LIMIT` / `INTEGRATION_API_MEMORY_LIMIT` | `6.0` / `4G` | container limits, sized for per-call app-server spawns |
| `CODEX_SUBSCRIPTION_INVOCATION_TIMEOUT` | `90s` | whole call, including queue, refresh and retries (unchanged) |
| `CODEX_SUBSCRIPTION_LOGIN_TTL` | `10m` | device-code window (unchanged) |

**Errors.** Every failed call carries one content-free reason code, which
Model Plane relays to the chat UI. Nothing else of an upstream error is kept,
logged or returned: not Codex's `message` or `additionalDetails`, an upstream
body, a JSON-RPC `error.message`, or the plan type.

| Code | Unary status | Meaning |
|---|---|---|
| `subscription_usage_limit` | 429 | the plan's Codex usage limit is exhausted, or the plan has no Codex usage or no credits left (see below) |
| `rate_limited` | 429 | the broker's own concurrency queue timed out, Integration Core's request limiter refused the call, or ChatGPT throttled it briefly; it does not say which, so copy for it must not blame ChatGPT |
| `subscription_reauth_required` | 409 | the ChatGPT sign-in must be renewed; the connection is marked `needs_refresh` |
| `subscription_upstream_unavailable` | 503 | a transient ChatGPT or Codex failure (after their own retries), or the invocation deadline; a retry may still fail |
| `subscription_context_too_long` | 413 | the prompt exceeds the model's context window |
| `subscription_bad_request` | 422 | ChatGPT rejected the request (a 400/422 or a policy refusal) |
| `subscription_broker_failed` | 502 | anything else |

When known, the error object also carries `resetsAt` (RFC3339 UTC, whole
seconds), `retryAfterMs`, `windowMinutes` and `limitReachedType`, each omitted
when unknown; they are account metadata, never content. An integer
`Retry-After` is sent for `rate_limited`, for a usage limit whose reset is
known (there is no made-up default), and for an upstream outage only when the
upstream sent a hint. The stream route checks the connection and takes its
queue slot before it sends the 200 and `ready`; a failure after `ready` is one
`subscription.error` event with the same code and fields. A connection already
`needs_refresh` is refused up front with 409 `subscription_reauth_required`;
any other inactive state stays 409 `subscription_connection_inactive`.

On the app-server transport a usage limit takes its reset from Codex's
`account/rateLimits/updated` notification (the exhausted window, the later one
when both are), else from one `account/rateLimits/read` (a non-model usage
read, bounded to 3s) on the still-open process; a failed read leaves it
unknown. The direct transport reads the reset from `resets_at`,
`resets_in_seconds`, the exhausted window of the active `x-codex-*` rate-limit
header family, then `Retry-After`. On both, a reset more than 7 days ahead,
from any of these sources, is unknown rather than clamped, so no reset time
earlier than the real one is shown.

The pinned Codex app-server reports an exhausted limit, spent credits and a
plan without Codex all as `usageLimitExceeded`, so they cannot be told apart
there, and all are `subscription_usage_limit`. The direct transport does tell
them apart and logs a plan without Codex as `error_kind` `usage_not_included`,
but returns the same public code, so that every layer keeps one vocabulary.
Without a reset the message therefore names both causes; copy shown to the
user must too.

A 401 gets the broker's one forced token refresh on either transport: a
refresh the token endpoint refuses (or a sign-in for another account) is
`subscription_reauth_required`, and a successful one leaves a transient
`subscription_upstream_unavailable`, whether the retried request still got a
401 or the 401 came after answer text (direct transport, `error_kind`
`unauthorized_after_recovery`), so a passing 401 never disconnects the user.

Codex retries any unexpected HTTP status and then fails the turn as `other`,
which names no status. Its retry notifications do name one, so a turn failed
as `other` after retries is `subscription_upstream_unavailable` only when the
latest retry named no status (a dropped stream, a timeout), a 5xx, or 408, 425
or 429. A retried persistent client error, such as a 403 from a workspace
without Codex or a 404, is `subscription_broker_failed`, because a retry
would fail the same way. The latest retry stands in for the final attempt,
which Codex does not describe; the trade-off is that a final attempt failing
differently from the retry before it is labelled by that retry.

Each failed call logs one `codex_subscription_upstream` warning with `reason`,
`error_kind` and `error_code`, and for a usage limit `resets_at`,
`window_mins` and `limit_reached_type`. The app-server line also carries
`upstream_retries`, `retry_http_status` and `rate_limits_source`.

Model Plane's inference-core spells these codes and the `code`, `resetsAt` and
`retryAfterMs` fields as its own literals.
`internal/api/codex_subscription_contract_test.go` reads its adapter
(`Model Plane/rust/services/inference-core/src/provider/codex_subscription.rs`)
and fails when a code the broker sends, other than
`subscription_broker_failed`, or one of those field names, or the pre-flight
`subscription_connection_inactive`, is not what inference-core reads. A code
it does not read would reach the chat UI as the generic failure.

**Scope.** Callers select the connection by setting `provider_hint` to
`openai-codex-subscription` and `subscription_connection_id` on Model Plane's
`InferRequest`. Model Gateway exposes the same selector as `provider` and
`subscription_connection_id` (or `subscriptionConnectionId`) on its normal
invoke and streaming contracts. The broker checks the verified org and user
scope before every turn and audits only the request id and model, never
message content. Structured output (a JSON schema, sent as strict
`json_schema`) and tool proposals (JSON data validated by Model Gateway) are
supported; vision, embeddings, and ZDR requests are rejected rather than
silently using a less constrained route.

**Policy.** Each connection must be the end user's own ChatGPT account: no
sharing, pooling, or falling back to another connection's credential. ChatGPT
plan availability, model availability, rate limits, and terms remain governed by
the user's OpenAI subscription; this avoids API-token spend for an eligible run
but does not make usage unlimited or guaranteed free.

### ZDR capability record (Z-12)

Whether a model connection can give zero data retention (ZDR) is a recorded
fact about that connection, kept in its own tables (migration
`0013_model_connection_zdr.sql`) and not on the connection row:

- `integration_connection_zdr` holds one row per connection: `capability`
  (`supported`, `not_supported` or `cannot_verify`), the `basis` it rests on, the
  contract row it came from (`basis_ref`, `catalog_version`) and a `digest`. It is
  written only by `store.ApplyConnectionZDRAssessment`, from values computed by
  `internal/zdr`; no generic connection write (upsert, reconnect, credential
  refresh, the capability and provider-context patches) touches it, so a stale
  read-modify-write cannot write a stale digest back and no client field can set
  a capability.
- `integration_connection_zdr_acknowledgements` is the ledger of what a user
  was shown and accepted, bound to that digest. It has no foreign key to the
  connection (connections are soft-deleted, and the record must not depend on
  one), a unique index on the active row per connection and digest, and a
  trigger that lets a row change only to record its revocation. Nothing writes
  to it yet: the acknowledgement API ships after this change.

The rule table is the shared contract
`apps/Model Plane/api/contracts/connection-zdr-capabilities.json`, embedded from
the byte-identical, sha256-pinned copy in `internal/zdr/testdata/`. The service
re-hashes it against the pin and runs a strict loader at start and refuses to
start on a mismatch. Only the exact `(provider_key, auth_mode)` pair of a row is
assessed as that row (today only the ChatGPT subscription, `not_supported`, on
known provider terms); anything else is `cannot_verify` with no waiver route, and
a connection with no record at all is unassessed, which every reader must treat
as `cannot_verify`. The auth mode is the provider catalog's, never the
connection's own `provider_context`. The digest is computed by `internal/zdr`
alone; other planes compare it as an opaque string.

A connection is assessed when it is created (`/connect`), when it becomes
connected, and by `api.ReconcileConnectionZDR`, which runs once at every start and
assesses each live (not deleted) model connection. The reconcile is idempotent: a
second pass writes nothing and audits nothing. A changed digest (a contract
`catalog_version` bump) revokes the acknowledgements given against the old one and
writes `subscription_connection.zdr_assessed` and
`subscription_connection.zdr_acknowledgement_invalidated` audit rows in the same
transaction. There is no feature switch: the record is inert until the
acknowledgement and decision routes read it.

Check it live (read-only): after the image starts,
`SELECT capability, count(*) FROM integration_connection_zdr GROUP BY 1` shows one
row per live subscription connection, all `not_supported`, and the start-up log
line `connection ZDR assessment reconcile` gives the counts. To change a
provider's capability, edit the contract, bump `catalog_version`, add the digest
vector, re-pin and re-vendor every copy (see `apps/Model Plane/api/contracts/README.md`).

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
