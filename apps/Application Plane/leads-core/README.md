# leads-core

Application Plane lead service (Go/Gin, port 3164).

Two separated domains:

1. **Company lead-builder (W1)** — filtered Enhetsregisteret (Brreg) company
   search, org-scoped saved lists (`lead_lists` / `lead_list_companies`), and
   metered CSV export with a per-export audit event
   (`velion.audit.v1.application.lead_export`).
2. **Provider lead sync** — LinkedIn Lead Gen form responses pulled through
   integration-corev2's actions gateway into the `provider_leads` table, with a
   per-sync-run audit event (`velion.audit.v1.application.provider_lead_sync`).

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

Per org with a `linkedin` connection carrying the `social.leads.read`
capability, the syncer calls integration-corev2:

- `GET /api/v1/connections?organizationId=...&providerKey=linkedin`
  (internal `X-Internal-API-Key` header, same pattern as social-core)
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
| `INTERNAL_API_KEY` | — (required) | gates the HTTP surface; also sent to integration-corev2 |
| `NATS_URL` / `NATS_TOKEN` | empty (audit disabled) | best-effort audit publishing |
| `INTEGRATION_CORE_URL` | `http://integration-api:3026` | actions gateway; empty disables provider lead sync |
| `PROVIDER_LEAD_SYNC_ENABLED` | `true` | interval worker toggle |
| `PROVIDER_LEAD_SYNC_INTERVAL` | `1h` | interval worker cadence |

## Tests

```bash
go build ./... && go vet ./... && go test ./...
```

`internal/providerleads/sync_test.go` runs the syncer against a mocked actions
gateway (fixtures shaped from the LinkedIn executor's real pass-through
handling), covering persistence, idempotency, capability/owner honest-skips,
and the no-connection path.
