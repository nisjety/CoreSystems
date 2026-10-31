# leads-core

Application Plane lead service (Go/Gin, port 3164).

Two separated domains:

1. **Company lead-builder (W1)** — filtered Enhetsregisteret (Brreg) company
   search, org-scoped saved lists (`lead_lists` / `lead_list_companies`), and
   metered CSV export with a per-export audit event
   (`verevon.audit.v1.application.lead_export`).
2. **Provider lead sync** — LinkedIn Lead Gen form responses pulled through
   integration-corev2's actions gateway into the `provider_leads` table, with a
   per-sync-run audit event (`verevon.audit.v1.application.provider_lead_sync`).

## PII posture

The company lead-builder is **company-only by design**: no person, role,
contact, or birth-number field exists anywhere in `lead_lists`,
`lead_list_companies`, the search responses, or the CSV export schema.

**`provider_leads` is the one deliberate, scoped exception.** Lead-form
answers are person data (names, emails, phone numbers — whatever the form
asked). The posture change is contained by these rules:

- Provider leads live **only** in `provider_leads` (migration
  `002_provider_leads.up.sql`). They are never joined into the company-search
  tables and never flow through the metered company CSV-export path.
- Sync-run audit events carry **counts only** — never form answers.
- GDPR erasure is org-scoped and internal-only:
  `DELETE /api/v1/provider-leads?organizationId=...` removes every provider
  lead an org holds.
- Any new surface that reads `provider_leads` must be reviewed against this
  section first.

## Provider lead sync

For its configured organization, if a `linkedin` connection carries the
`social.leads.read` capability, the syncer calls integration-corev2:

- `GET /api/v1/connections?organizationId=...&providerKey=linkedin`
  (verified organization-scoped `Authorization: Bearer` credential)
- `POST /api/v1/actions/execute` with operation `linkedin.lead.forms`
  (params: `owner` URN, `count`, `start`)
- `POST /api/v1/actions/execute` with operation `linkedin.lead.responses`
  (params: `leadForm` URN, `count`, `start`)

Rows dedupe on `(org_id, provider_key, provider_lead_id)` so re-syncs are
idempotent. Connections without the capability, or without a resolvable
lead-forms owner URN (`providerContext.ownerUrn` / `organizationUrn` /
numeric `providerAccountId`), are skipped honestly with a recorded reason.

Triggers:

- Interval worker (`PROVIDER_LEAD_SYNC_ENABLED`, default on;
  `PROVIDER_LEAD_SYNC_INTERVAL`, default `1h`).
- Manual: `POST /internal/sync/provider-leads` (internal-key gated), optional
  body `{"organization_id": "...", "owner": "urn:li:organization:..."}`.

## Configuration

| Env | Default | Purpose |
| --- | --- | --- |
| `PORT` | `3164` | HTTP port |
| `DATABASE_URL` | — (required) | shared application-postgres |
| `INTERNAL_API_KEY` | — (required) | gates the inbound HTTP surface; never sent to Integration Core |
| `NATS_URL` / `NATS_TOKEN` | empty (audit disabled) | best-effort audit publishing |
| `INTEGRATION_CORE_URL` | `http://integration-api:3026` | shared Integration Core actions gateway |
| `INTEGRATION_CORE_TENANT_ORGANIZATION_ID` | empty | the sole organization the gateway client may access |
| `INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE` | empty | approved read-only mounted file containing a verified Integration Core bearer token for that organization; loaded on startup |
| `PROVIDER_LEAD_SYNC_ENABLED` | `true` | interval worker toggle |
| `PROVIDER_LEAD_SYNC_INTERVAL` | `1h` | interval worker cadence |

Tenant bearer and organization ID must be configured together. Until the URL
and both tenant settings exist, the background syncer stays disabled and the
manual sync route returns `503 not_configured`; the Leads HTTP service can still run.
The periodic worker no longer enumerates connections across organizations.
After rotating the bearer file, restart the service to reload it. A short-lived
bearer requires an automated approved renewal/restart path before long-running
provider sync is enabled.

## Tests

```bash
go build ./... && go vet ./... && go test ./...
```

`internal/providerleads/sync_test.go` runs the syncer against a mocked actions
gateway (fixtures shaped from the LinkedIn executor's real pass-through
handling), covering persistence, idempotency, capability/owner honest-skips,
and the no-connection path.

## First-party website inquiries and outcomes

Leads Core now accepts first-party contact submissions at
`POST /api/v1/website-inquiries`. This server-to-server ingress uses a dedicated,
write-only installation credential. The protected `WEBSITE_INQUIRY_BINDINGS_FILE`
binds it to one organization, registered website ID/revision, canonical site and
`development` or `production`. The service API key cannot authenticate this
endpoint, browser Origin headers are rejected, and payloads cannot select an org.

A successful response is a durable provider-lead receipt. Atomic insertion dedupes
submission IDs; replaying different content returns 409. Fields remain exclusively
in `provider_leads` and are excluded from company lists/exports and Marketing.
The existing organization erasure operation deletes these inquiries too.

`GET /api/v1/website-outcomes` uses a separate read-only installation credential
and exact website/revision/environment/date context. It returns aggregate counts,
receipt timing, and unknown bookings, without names, emails, messages, IDs or tokens.
Inquiries and received leads describe the same committed receipt, not two separate
conversions. Period boundaries use Europe/Oslo; development never contributes to
production counts. Revoking/removing an installation requires restarting this local
service; changing URL/revision must be accompanied by an authorized binding update.

The canonical Studio launcher applies `docker-compose.website-inquiries.studio.yml`
when a protected `.local/secrets/website-inquiries/bindings.json` installation exists.
Its init service copies only the installation inputs into private owner-specific
volumes; Marketing receives only the read credential. Set
`STUDIO_WEBSITE_INQUIRIES_ENABLED=0` to explicitly disable this optional local slice.
No public ingress or production installation is enabled by this development setup.

Verification: `go test ./...`. The repository receipt/isolation/erasure test additionally
uses `WEBSITE_INQUIRIES_TEST_DATABASE_URL` against an already migrated Leads database.
It uses a unique test organization and removes only its own synthetic records.
