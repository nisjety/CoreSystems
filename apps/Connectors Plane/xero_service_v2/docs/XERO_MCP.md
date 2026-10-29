# Xero MCP server

Status: implemented in `core/crates/xero-mcp`. Both transports and live,
read-only Accounting, Assets, and Projects probes have been exercised locally.
This is not a CoreSystem registration or a production cloud-extraction sign-off.

## Surface and ownership

The native Rust server pins the official `rmcp = 3.5.0` crate. It exposes stdio
for a trusted local MCP host and Streamable HTTP at `http://127.0.0.1:5003/mcp`
by default, with `GET /healthz` for process health. This is a Xero read and raw
extraction service, not an LLM host or an agent runtime. It uses this repository's
PKCE connection store or Custom Connection/client-credentials token provider;
there is no external Xero SDK or AI SDK dependency in the request path. Its
optional warehouse tools read tenant-scoped counts and approved aggregates from
BigQuery using Application Default Credentials.

The pinned OpenAPI catalog inventories **186 GET operations**: 126 Accounting,
4 Assets, 4 Bank Feeds, 8 Finance, 1 Identity, 7 Projects, and 36 Payroll NZ.
The MCP exposes **184** of them. The two Payment Services configuration routes
are deliberately out of warehouse scope and are absent from MCP discovery,
tools, direct query, and extraction. Each exposed operation has a
`xero_{family}_{operationId}` tool name, with the Xero `operationId` retained
verbatim. The additional fourteen compact tools are:

| Tool | Purpose |
| --- | --- |
| `xero_list_connections` | List only connected Xero tenants allowed to this MCP client. |
| `xero_list_operations` | Discover catalogued GETs, parameters, scopes, and sensitivity before calling them. |
| `xero_list_write_operations` | Discover the fixed, approval-gated mutation operations compatible with Xero's official MCP. |
| `xero_warehouse_overview` | Return only BigQuery row counts for the caller's authorized tenants; no records, fields, or destination overrides. |
| `xero_warehouse_metric` | Return a tenant-scoped approved aggregate; currently product volume and net sales by month. It never accepts SQL. |
| `xero_business_read` | Read a common Accounting collection by a business name such as `invoices`, `contacts`, `items`, or `payments`. It maps to a pinned catalog operation and still validates documented query parameters. |
| `xero_financial_report` | Read aged receivables/payables, balance sheet, profit and loss, or trial balance through a fixed report name. |
| `xero_payroll_nz_read` | Read common Payroll NZ collections when the selected tenant has the relevant regional scopes and product access. |
| `xero_build_ui_link` | Build a validated Xero web-app URL for an authorized record. When a URL needs the organisation short code, the server resolves it from the selected tenant; it never reads the target record or grants record access. |
| `xero_query` | Run one catalogued, tenant-scoped GET with explicit path/query/header parameters. |
| `xero_read_result` | Page or project a recent JSON result by handle and JSON pointer. |
| `xero_extract` | Start a bounded, asynchronous raw extraction after permission and destination checks. |
| `xero_get_job` | Inspect an authorized extraction job and its content-free outcome. |
| `xero_cancel_job` | Request cancellation of an authorized job; already written raw objects may remain. |

The pinned catalog is `core/crates/xero-mcp/catalog.json`; use
`cargo run -p xero-mcp -- --print-manifest` to inspect its source commit,
operation names, and documented scopes. Catalog availability does **not**
imply a tenant has the corresponding Xero OAuth scope, product entitlement, or
data. Bank Feeds has a write-capable `bankfeeds` scope despite this server's
GET-only API; Finance needs partner access. PKCE tenants must reauthorize for
new scopes; Custom Connection scopes are granted in Xero's Developer Portal.
See [endpoint parity](XERO_ENDPOINT_PARITY.md) and the
[extended API runbook](XERO_EXTENDED_RUNBOOK.md).

Compatibility with the current official Xero MCP is held by contract tests:
all 26 upstream read operations and all 25 upstream source-changing operations
are available here. This server presents all reads through Xero OpenAPI
operation IDs and an explicit tenant UUID. It additionally provides a small
set of curated aliases for common Accounting, report, and Payroll NZ reads.
The aliases select only fixed catalog operations and use the same tenant
checks, input validation, response paging, and source-coverage metadata as
`xero_query`; they are not a separate API client or a bypass around the
catalog.

### Reports and large results

The catalog exposes the documented Xero report operations through their normal
`operation_id` values: aged payables/receivables by contact, balance sheet,
bank summary, budget summary, executive summary, profit and loss, trial
balance, 1099, report-list, and `getReportFromId`. In compact mode call them
through `xero_query`; in full mode they are individual tools. `getReportFromId`
is usable only when Xero supplies a usable report ID. The current report-list
response did not provide one for the UI-only reports requested in this tenant.
The API does not provide a generic route for every report that the Xero UI can
render, so an unlisted custom report cannot be made available by an MCP
configuration change alone.

### Curated reads and Xero links

Use the curated read tools for the common analytical paths, while retaining
`xero_query` for the rest of the pinned OpenAPI catalog. The aliases accept
only documented query parameters and retain the same single-response coverage
metadata as the underlying operation:

```json
{
  "tenant_id": "authorized-xero-tenant-uuid",
  "resource": "invoices",
  "query": {"page": 1, "pageSize": 100},
  "limit": 100
}
```

When a read or an approved write returns a Xero resource UUID, clients can ask
for a web-app link without exposing a Xero token or organisation short code:

```json
{
  "tenant_id": "authorized-xero-tenant-uuid",
  "resource": "invoice",
  "resource_id": "xero-invoice-uuid"
}
```

`xero_build_ui_link` validates the tenant and UUIDs, then reads only the
tenant's organisation short code when the Xero URL requires it. It does not
fetch the target invoice, contact, quote, or payment, and the returned URL is
not proof that the caller has access to that record in the Xero UI. Bank
transaction links additionally require `bank_account_id`.

For interactive responses larger than a chat can safely display, a client with
result retention enabled receives a private result handle and uses
`xero_read_result` with pagination or a JSON pointer. For full historical
report or transaction delivery, use `xero_extract` to GCS rather than putting
raw rows in the chat transcript.

To backfill the business inputs required for product-level sales, call
`xero_extract` once for each authorized organization with this bounded
Accounting plan. `expand_details` writes the invoice and credit-note line
payloads needed by the staging parser:

```json
{
  "tenant_id": "authorized-xero-tenant-uuid",
  "destination": "gcs",
  "idempotency_key": "xero-business-backfill-2026-tenant-suffix",
  "plan": {
    "kind": "accounting",
    "entities": [
      "invoices",
      "credit_notes",
      "items",
      "accounts",
      "tracking_categories",
      "contacts"
    ],
    "mode": "backfill",
    "from": "2026-01-01",
    "to": "2026-10-18",
    "expand_details": true,
    "max_detail_records": 10000
  }
}
```

Run reports separately with `mode: "reports"` and report entities such as
`report_profit_and_loss`; report snapshots are point-in-time artifacts, not
incremental transaction collections.

Nine analytical MCP prompts are defined: `financial_overview`,
`receivables_payables`, `reconciliation`, `data_quality`,
`warehouse_extraction`, `project_profitability`, `sales_performance`,
`cash_position`, and `payroll_nz_review`. Prompts provide guidance to an MCP
client; they neither call Xero nor grant authority. Their provider content
must be treated as untrusted data, not instructions.

## Configure and run

Use a private `.env.mcp` outside source control or inject these values from a
secret manager. The file must include existing Xero PKCE or Custom Connection
configuration; see [PKCE onboarding](PKCE_ONBOARDING.md) and
[operations](OPERATIONS.md). Do not put tokens in a command line, MCP tool
arguments, or a CoreSystem registration request.

| Variable | Requirement and behavior |
| --- | --- |
| `XERO_MCP_TENANTS` | Required for local stdio or a static fallback token. OAuth-only HTTP obtains its tenant UUID allowlist from each sign-in. |
| `XERO_MCP_TOKEN` | HTTP-only fallback bearer credential, 32-1024 characters; required when neither `XERO_MCP_CLIENTS_JSON` nor the opt-in OAuth bridge is configured. It uses the local allowlist as one `model-plane` client. |
| `XERO_MCP_CLIENTS_JSON` | Optional JSON array of `{ "token", "client_id", "tenant_ids", "allow_extract", "allow_sensitive", "allow_warehouse", "zero_data_retention", "write_actions" }`; each token is 32-1024 characters and identities/tokens must be unique. `write_actions` defaults to empty. Each client receives its own tenant allowlist and flags. |
| `XERO_MCP_ENABLE_WRITES` | Default `false`. When true, adds `xero_prepare_write` and `xero_confirm_write` to this server. It grants no client access by itself. |
| `XERO_MCP_WRITE_ACTIONS` | Optional comma-separated allowlist for trusted local stdio only. The static HTTP fallback token is always read-only. For an independent HTTP service client, set `write_actions` in that client's `XERO_MCP_CLIENTS_JSON` object instead. |
| `XERO_MCP_BIND` | HTTP socket address; defaults to `127.0.0.1:5003`. Exposing a non-loopback listener requires a reviewed TLS/auth network boundary. |
| `XERO_MCP_TLS_PROXY` | Default `false`. Non-loopback binds fail startup unless this is `true` and deployment enforces TLS termination plus restricted network access to the HTTP listener. It does not implement TLS itself. |
| `XERO_MCP_STORAGE_ROOT` | Job receipts and local raw runs; defaults to `./out/mcp`. Use a restricted, persistent volume for extraction. |
| `XERO_MCP_TOOL_MODE` | `full` (default) advertises 198 read/operational tools; `compact` advertises fourteen operational tools while retaining access to all 184 in-scope GETs through discovery and `xero_query`. Enabling writes adds two tools in either mode. Use compact for the future Model Plane mapping. |
| `XERO_MCP_ALLOW_EXTRACT` | Local stdio/fallback-client permission; `true` or `false`, default `false`. |
| `XERO_MCP_ALLOW_SENSITIVE` | Local stdio/fallback-client permission for sensitive operations/resources; default `false`. |
| `XERO_MCP_ALLOW_WAREHOUSE` | Local stdio/fallback-client permission for `xero_warehouse_overview`; default `false`. |
| `XERO_MCP_ZERO_DATA_RETENTION` | Local stdio/fallback-client policy; default `false`. When `true`, do not persist result handles or extraction jobs/raw data. |
| `XERO_MCP_WAREHOUSE_PROJECT`, `XERO_MCP_WAREHOUSE_DATASET`, `XERO_MCP_WAREHOUSE_LOCATION` | Optional BigQuery target; all three must be configured together. They are service configuration, never tool arguments. |
| `XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED` | Warehouse-query ceiling; default `5368709120` (5 GiB). |
| `XERO_MCP_ALLOWED_HOSTS`, `XERO_MCP_ALLOWED_ORIGINS` | Optional comma-separated HTTP host/origin restrictions; review when publishing behind a proxy. |

The default host allowlist is `localhost,127.0.0.1,::1`; browser Origin
headers are rejected by default. Configure exact permitted origins only when
needed. Authenticated HTTP traffic is limited to 120 requests/minute per
client, MCP request bodies to 64 KiB, and interactive reads to eight concurrent
calls with a 60-second timeout. Health exposes process status only; it is not
an assertion of Xero connectivity or entitlement.

HTTP middleware must authenticate `Authorization: Bearer ...` and bind the
request to the matching configured `client_id`, tenant UUIDs, sensitive-data,
warehouse, extraction, and retention policy. Tool-supplied tenant IDs only select from
that allowlist. Stdio has a fixed local scope and inherits trust from the
process/host; do not offer it to untrusted local users. The optional Xero-backed
OAuth bridge below issues its own MCP-only tokens; Auth Core is not used.

PKCE connection-name/scope data is read from the shared connection store on
each request; Custom Connection metadata remains configured at startup.
Xero access/refresh tokens come from the server-side token provider and are
never returned to an MCP client.

## Opt-in Xero sign-in for MCP

`core/crates/xero-mcp` is this repository's native Rust server, not a clone of
the upstream Xero MCP repository. Its OAuth bridge reuses this repository's
Xero authorization-code + PKCE flow. It verifies Xero's signed OIDC ID token
and nonce to identify the user, records the connected Xero organizations in the
server-side connection store, and issues separate opaque MCP authorization,
access, and rotating refresh tokens. Claude never receives a Xero token. MCP
tokens are bound to the Xero organizations returned by that sign-in, allow
read-only non-sensitive tools, and expire after 24 hours at the latest. Users
see an explicit approval page before
Xero sign-in. The MCP token store is a private SQLite file in
`XERO_MCP_STORAGE_ROOT/oauth.sqlite` and must run on one persistent worker.

The bridge is disabled unless all of these settings are configured:

| Variable | Purpose |
| --- | --- |
| `XERO_MCP_OAUTH_ISSUER` | Public HTTPS origin of MCP, for example `https://tools.aquatiq.com` (local HTTP loopback is allowed for tests). |
| `XERO_MCP_OAUTH_CLIENT_ID` | Stable public client ID returned by the restricted Claude DCR endpoint; it also works as a manual fallback. |
| `XERO_MCP_OAUTH_XERO_REDIRECT_URI` | Exact `{issuer}/oauth/xero/callback`; register it in the Xero developer app. |
| `XERO_MCP_OAUTH_SCOPES` | Xero PKCE consent scopes, including `openid` and `offline_access` plus the required read scopes. |
| `XERO_AUTH_MODE`, `XERO_PKCE_CLIENT_ID`, `XERO_PKCE_CLIENT_SECRET` | Select the existing PKCE Xero app. The client secret is optional only if the registered Xero app is a public client. |
| `XERO_MCP_OAUTH_ALLOW_EXTRACT` | Default `false`. When `true`, allow the signed-in caller to start bounded tenant-scoped raw extracts. Requires retained results. |
| `XERO_MCP_OAUTH_ALLOW_WAREHOUSE` | Default `false`. When `true`, allow only fixed tenant-scoped warehouse counts and approved aggregates. It does not grant SQL access. |
| `XERO_MCP_OAUTH_WRITE_ACTIONS` | Empty by default. A comma-separated allowlist of exact, approval-gated Xero write actions for signed-in MCP users. It requires `XERO_MCP_ENABLE_WRITES=true`, retained results, and the corresponding Xero PKCE write scopes. |
| `XERO_MCP_OAUTH_ZERO_DATA_RETENTION` | Default `true`. Set `false` only on a private persistent worker to enable private ten-minute result handles and extraction/job receipts. |

`/mcp` then returns a `401` challenge pointing to
`/.well-known/oauth-protected-resource/mcp`. Authorization-server metadata is
served at `/.well-known/oauth-authorization-server`; the authorization, Xero
callback, approval, token, and registration endpoints are under `/oauth/`.
The RFC 7591 registration endpoint accepts only a public client with the
exact hosted Claude callback, S256 authorization code flow, and `xero.read`
scope. Repeated registrations return the same public client ID without a
client secret. This is restricted Claude DCR, not open registration for
arbitrary redirects. CIMD is not implemented. Hosted Claude uses
`https://claude.ai/api/mcp/auth_callback`; Claude Code's variable loopback
callback is not yet supported. The token endpoint accepts form-encoded code
exchange and refresh with S256 PKCE and rotates refresh tokens. Reuse of an old
refresh token revokes its active token family. MCP access tokens retain the
exact `/mcp` resource audience. Each grant is bound to the connected-at
version of every Xero tenant used at sign-in; the HTTP guard rejects a grant
after that tenant's source connection is replaced. The Xero access-token cache
also checks that version before reusing a token. These checks do not provide
atomic per-user source isolation across a concurrent reconnect and API call.

For a separate BFF or sync worker, use the shared GCS Xero connection store,
not independent file-backed stores. The MCP OAuth SQLite database is
single-worker only. On one MCP worker, login and reads share the same Xero
connection-store instance, so newly connected organizations appear without a
restart. The OAuth bridge is deployed as a single-worker pilot at
`https://tools.aquatiq.com/mcp`, but a real Claude/Xero sign-in and token
refresh have not yet been verified. Its private MCP connection file is separate
from the onboarding BFF and sync worker. The Xero source store remains keyed by
tenant, not by Xero user.

A second user connecting the same tenant replaces the first user's source
connection and invalidates the first user's MCP grant; concurrent independent
grants for one tenant are not supported.

Per-user source credentials, shared refresh coordination, explicit grant
revocation, and a shared MCP token store are still required before broad
multi-user distribution. Do not advertise it as a validated Claude connector
until the end-to-end checks pass.

## Opt-in Xero writes

The Rust MCP now has capability parity with the 25 source-changing operations
in Xero's current official MCP: Accounting contacts, invoices, payments, bank
transactions, credit notes, items, manual journals, quotes, tracking
categories/options, and Payroll NZ timesheets. The original four typed actions
remain available. The other official operations use the `official_mcp` write
form, which carries an allowlisted operation name, fixed host and method,
declared UUID path parameters, and a bounded JSON body.

This is deliberately not an arbitrary Xero write proxy. A caller cannot choose
an HTTP method, host, or path. Every action has an explicit tenant UUID, exact
per-client action grant, stored idempotency key, and independent operator
approval. `xero_list_write_operations` is the machine-readable contract for
the official-compatible actions. Invoice creation through the legacy typed
action remains hard-coded to `DRAFT` for clients that need that stricter
contract.

Writes are disabled by default. Enable `XERO_MCP_ENABLE_WRITES=true` only on a
reviewed worker with a private, persistent `XERO_MCP_STORAGE_ROOT`. Set
`write_actions` on a specifically provisioned `XERO_MCP_CLIENTS_JSON` service
client or `XERO_MCP_WRITE_ACTIONS` for trusted local stdio. The static fallback
bearer token never receives write access. A Xero-backed per-user OAuth grant
receives only the exact actions in `XERO_MCP_OAUTH_WRITE_ACTIONS`; provisioning
write actions requires result retention and adds the matching Xero scopes.
Zero-data-retention clients are denied because the approval/audit database
persists the exact request body. A read-capable token cannot request an
arbitrary Xero method, path, scope, or request body through `xero_query`.

1. Call `xero_prepare_write` with either a typed action or the `official_mcp`
   compatibility form. It saves the exact body and
   a server-generated Xero `Idempotency-Key` in private `writes.sqlite` and
   returns an approval ID and preview. It does **not** call Xero.
2. A server operator independently inspects the stored request, including
   client, tenant, action, and body, from the same private storage root:

   ```bash
   xero-mcp --inspect-write <approval-uuid>
   xero-mcp --approve-write <approval-uuid> --confirm-client <client-id> --confirm-tenant <tenant-uuid> --confirm-action <action> --confirm-body-sha256 <digest-from-inspection>
   ```

3. The original MCP client calls `xero_confirm_write` with the approval ID.
   The store atomically claims the approval before one Xero request is sent.
   Repeated confirmation cannot submit it again. Unapproved or expired
   requests (15 minutes), different clients/tenants, revoked action grants,
   and mismatched operator approvals or body digests fail closed. Inspection
   prints the exact stored body locally; treat that output as sensitive data.

The database records preparation, approval, claim, and completion times plus
`committed`, `rejected`, `not_sent`, or `outcome_uncertain` status; it
must be restricted to the service/operator identities and covered by a
retention policy. A process crash or network timeout after submission leaves
an uncertain outcome: inspect Xero and the private receipt before preparing a
new write. The server never automatically retries writes. Xero documents a
six-minute [idempotency-key retention window](https://developer.xero.com/documentation/guides/idempotent-requests/idempotency/),
so this is not an indefinite duplicate-prevention guarantee. Approvals are
single-worker SQLite state, not a distributed approval service. There has been
no live write test or deployment of this opt-in path.
Do not enable writes on the current VPS pilot. The owner confirmed rotation of
the previously exposed Xero source credential; this was not independently
verified by the MCP deployment checks.

## Warehouse overview and approved aggregates

`xero_warehouse_overview` accepts an empty arguments object and returns only
the configured project, dataset, location, table count, total row count, and
per-table row counts. It sends the MCP client's immutable tenant allowlist as
a parameterized BigQuery query; it cannot receive a tenant, project, dataset,
table, SQL, or field override from the caller. No record content or per-record
metadata is returned.

`xero_warehouse_metric` currently supports `product_volume_by_month`. It reads
only the four finance facts and two tracking dimensions named in the server
query, applies negative signs to credit-note lines, uses the caller's
authorized tenant IDs as BigQuery parameters, and returns at most 1,000
aggregates over a 366-day range. It accepts no SQL, table, field, project, or
destination override. Credit-note lines without an Xero item code are returned
with `is_product_attributed=false`; they are not incorrectly assigned to a
product. Completing product-level credit attribution requires the staging
parser to retain Xero's credit-note item reference when the source provides it.

The MCP process must have Application Default Credentials through a workload
identity, attached service account, or `GOOGLE_APPLICATION_CREDENTIALS`. Grant
that identity BigQuery job creation and table-level `dataViewer` only for the
named Xero staging and Gold tables, plus write-only object creation in the raw
Xero GCS bucket. The tool disables query cache and enforces
`XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED`; it fails closed when
the destination is not configured, the client lacks `allow_warehouse`, or
BigQuery cannot complete the count query. It is a volume overview, not a
freshness or data-quality audit.

From the Xero repository root, after provisioning the environment file:

```bash
cargo run -p xero-mcp -- --env-file .env.mcp --transport http
# MCP: http://127.0.0.1:5003/mcp
# Process health: http://127.0.0.1:5003/healthz
```

For a local MCP host, a stdio configuration is:

```json
{
  "mcpServers": {
    "xero": {
      "command": "cargo",
      "args": ["run", "-p", "xero-mcp", "--", "--env-file", ".env.mcp", "--transport", "stdio"],
      "cwd": "/absolute/path/to/xero_service"
    }
  }
}
```

The host must start this in the repository root (or point `command` to a
built binary and use an absolute env-file path). Restrict the env file and
token store to the service identity. `--transport stdio` is the default, but
it is explicit above for clarity. Stdio sends protocol frames to stdout and
diagnostics to stderr.

Build the non-root service image with:

```bash
docker build -f Dockerfile.mcp -t xero-mcp:local .
```

Inject a private environment file at runtime, use the selected credential
provider and a restricted persistent output volume, and attach port 5003 only
to the approved network. Do not bake environment files, token stores, or
service credentials into the image. The HTTP credential is independent of
Xero's OAuth access/refresh tokens. Do not mount the current file-backed PKCE
connection store into a second process: its refresh-token coordination is
process-local. A multi-process PKCE rollout requires a shared store and
re-onboarding plan.

The container binds `0.0.0.0:5003`; it therefore requires the explicit
`XERO_MCP_TLS_PROXY=true` runtime setting and a restricted TLS-proxy network.
Never expose its plain HTTP listener directly on a public host port.

## VPS pilot and distribution gate

`deploy/mcp/compose.vps.yml` runs one read-only MCP worker on the VPS's
existing `internal` Docker network. It publishes no host port and mounts only
its own private configuration and output volume. The pilot uses one Custom
Connection tenant, not the onboarding container's file-backed PKCE store. The
worker configuration keeps compact mode, zero-data-retention, and extraction,
sensitive, and warehouse access disabled. This permits discovery and
authorized read-only Xero queries but does not load the warehouse.

Build from a source-only context (no `.env` files), copy the compose file to
the deployment directory as `compose.yml`, and provision the runtime file with
`deploy/mcp/provision_custom_env.py`. Use the first explicitly selected
`XERO_ORG_N` credential block from the existing private service environment;
do not copy all organizations into one worker. Keep `secrets/server.env`
owner-readable only by the container UID, and `secrets/client.token` root-only.
Never print either file or put a token in a URL, shell argument, or application
log. Start with `docker compose up -d`, then check container state and that
Docker publishes no port.

The provisioner requires a source file owned by root or the invoking user with
mode 0600 and an explicit DNS hostname matching the eventual TLS route. It
refuses to replace existing outputs. For the `tools.aquatiq.com` route, run as
root in the private deployment directory:

```bash
python3 provision_custom_env.py \
  --source-env /private/path/to/xero/.env \
  --output-env secrets/server.env \
  --token-file secrets/client.token \
  --uid 10001 --gid 10001 \
  --allowed-host tools.aquatiq.com
```

If a different hostname is approved, set `--allowed-host` to that hostname
and update the Traefik rule together. Rotate credentials with new private
files and a controlled worker restart; do not overwrite or print the old
secrets.

Before enabling a new public route, verify `/healthz`, unauthenticated rejection,
bad Host/Origin rejection, one authorized tenant read, cross-tenant rejection,
and denial of extraction and sensitive operations. The Traefik route is
`deploy/traefik/xero-mcp.yml`; review its hostname, DNS/TLS, and client policy
before any future installation or change. Public client
credentials must be delivered out of band, scoped per client and tenant, and
rotatable. `XERO_MCP_CLIENTS_JSON` supports separate static pilot identities,
but it does not implement CoreSystem user-to-organization binding or credential
brokering. Do not register it directly with Model Plane until those controls
and end-to-end tests are complete.

On 2026-10-07, the VPS pilot container started on `internal` with no published
host port. The private runtime file is owned by UID/GID 10001 with mode 0600;
the separate client token is root-owned with mode 0600. An internal probe
confirmed HTTP 200 health and one Accounting `getOrganisations` source GET,
148 advertised operations and eight compact tools at that time, 401 for absent/invalid
credentials, 403 for bad Host/Origin, and MCP denials for another tenant,
extraction, and sensitive access. No Xero records were printed or persisted by
the probe. The source credential file was found world-readable on the VPS and
tightened to mode 0600 with root ownership. At the owner's explicit request,
the public `/mcp` route was installed on 2026-10-07 using those unrotated
credentials for the limited pilot. Rotate the Custom Connection credentials
urgently; this exposure is not approval for broad distribution. Proxy
`/healthz` is only liveness: add a separate scheduled authorized source-read
monitor and alerting before broader rollout.

### Claude remote connector

The previous static-bearer pilot was replaced on 2026-10-08. The public
`/mcp` endpoint now returns HTTP 401 with `resource_metadata` and `xero.read`
in `WWW-Authenticate`; its OAuth protected-resource and authorization-server
metadata return HTTP 200, and `/oauth/authorize` returns the approval page.
These probes establish reachability, not a completed Claude connection.
Claude's calls originate from Anthropic's cloud, so a browser-only probe does
not exclude a Cloudflare block affecting Claude. If sign-in fails, inspect
Cloudflare security events before changing the MCP server. Follow
[Claude's custom connector setup](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)
and [authentication requirements](https://claude.com/docs/connectors/building/authentication).

### OAuth pilot deployment

`deploy/mcp/provision_oauth_env.py` creates a private OAuth-only runtime file
from the existing owner-only BFF PKCE app configuration. It copies no static
MCP bearer or Custom Connection credential. The VPS runtime file is
`services/xero-mcp/secrets/oauth.env` (mode 0600, UID/GID 10001); the running
container uses `deploy/mcp/compose.oauth.vps.yml` and reads this OAuth-only
file. The worker uses its own persistent file connection store, so organizations connected via
this Claude flow do not automatically appear in the separate onboarding BFF or
scheduled sync. Do not copy that file store into another worker; shared sync
requires the GCS connection-store migration and live refresh-coordination test.
The Xero consent list is read-only Accounting scope coverage, not every
scope needed by all currently advertised GET operations.

The owner reported Xero callback registration and confirmed Custom Connection
secret rotation. The OAuth router and rate limiter from
`deploy/traefik/xero-mcp.yml` are installed on the VPS. The active image is
`aquatiq/xero-mcp:warehouse-analytics-2026-10-18`. It mounts a dedicated
`xero-mcp-runtime` service-account credential, with BigQuery job creation,
table-level read access only to Xero staging and approved Gold tables, and
write-only object creation in `aquatiq-datavarehus-xero-raw`. OAuth policy
enables retained result handles, extraction, and approved warehouse analytics;
Xero writes remain disabled. Public probes confirmed 201 for hosted Claude
registration, 400 for an unrelated callback, and 401 for unauthenticated MCP
calls. The earlier OAuth compose definition is preserved at
`services/xero-mcp/compose.oauth.pre-dcr-2026-10-08.yml`; the original static
pilot definition remains at `services/xero-mcp/compose.yml`. The previous router
is preserved as `services/xero-mcp/xero-mcp-traefik.pre-oauth-2026-10-08.yml`
for rollback. Claude's hosted custom
connector can select **Register automatically**, with no client ID or secret.
The manual fallback remains **Use your own OAuth client** with public client
ID `claude-xero` and no secret. Do not select Claude's published identity,
which requires CIMD. Complete one real sign-in,
one allowed organization read, a cross-tenant denial, and token refresh before
considering the pilot operational. Do not enable writes or broad distribution.

The approval page now permits form redirects to `login.xero.com`,
`authorize.xero.com`, and `claude.ai` in its CSP. Chrome follows Xero's
`login.xero.com` to `authorize.xero.com` redirect after the form POST and
enforces `form-action` on that hop. After adding the exact origin, a fresh
Chrome probe reached Xero's consent page using PKCE callback
`https://tools.aquatiq.com/oauth/xero/callback` on client
`29694FFD89F1437B8BB478DA1B2F2F48`; an earlier probe had returned
`Invalid redirect_uri`. The isolated probe was not approved, and a real Claude
round trip remains unverified. The prior image's compose file is preserved at
`services/xero-mcp/compose.oauth.pre-redirect-csp-2026-10-08.yml` for rollback.
Do not route this callback through the separate BFF callback.

## Read and result contract

Every pinned GET and `xero_query` validates the selected tenant and catalogued
parameters before fetching. `limit` is 1-100 rows (default 50); Xero's own
pagination is still explicit. A query returns `data` plus `meta` provenance in
one tool response: tenant, family/operation, fetch time, HTTP status, pinned
spec commit, source page/offset where applicable, byte count/hash, and
coverage flags. `coverage=single_response` and `complete_collection=false`
mean one response is **not** proof of a full collection or deletion. Failed,
partial, or unentitled routes are unknown, not zero.

Interactive responses are bounded below a 64 KiB model-output budget (the
current JSON projection cap is 60 KiB). When data cannot fit, `data` is null
or truncated and `meta.result_id` identifies an in-memory result that
`xero_read_result` can slice. Handles expire after 10 minutes, are limited to
32 entries and 5 MiB each, and are scoped to the original client and tenant.
They are not durable warehouse records. Zero-data-retention disables handles.
The 32-entry cache is shared across clients: eviction can remove a handle before
its TTL, and retention until expiry is not guaranteed. Reads authorize and
snapshot the original response under the cache lock; JSON parsing/projection
and binary encoding run after releasing it. Top-level `available_paths` metadata
is capped at 64 paths and 2 KiB, with `available_paths_truncated` identifying
omissions. This bounds discovery metadata without changing cached raw bytes.
Binary responses use private `xero://results/{result_id}` resources rather
than inline model data; retrieval requires sensitive-data permission. The
Identity `/connections` response is filtered to the **selected** tenant before
it enters a result or resource, for every accepted success status. Malformed
connection bodies fail closed. Never expose the app-wide connection list.

## Endpoint efficiency and quota recovery

The live backend shares one credential-free HTTP client across interactive
reads. Tenant headers and bearer tokens are assigned per request, not stored as
pool defaults; Identity requests omit the tenant header. A keep-alive integration
test verifies reuse across tenants without reusing their credentials. This
follows [reqwest's client reuse guidance](https://docs.rs/reqwest/latest/reqwest/struct.Client.html).

Interactive response bodies are capped at 5 MiB before buffering a declared
oversized body and during streamed chunk consumption. Exceeding this limit
returns non-retryable `result_too_large`; narrow the provider query or request
an authorized extraction. The native catalogued raw GET extraction ceiling
remains 50 MiB per response, separate from interactive policy.

The shared native GET fetcher accepts successful 2xx responses, preserves their
status, and validates content type; MCP result capture validates JSON shape.
This fixes Bank Feeds `getFeedConnections`, whose pinned
[OpenAPI response is HTTP 202](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f/xero_bankfeeds.yaml),
not 200. Its status and pagination are fixture-tested; no live Bank Feeds
entitlement is claimed.

All native request paths share per-tenant 429 cooldowns in this process,
including after the caller's last retry. Daily quota is reserved before
requests and learned from response headers, not a hard-coded subscription
allowance. Exhaustion no longer requires a process restart: an observed
`Retry-After` permits one later probe; without it, a conservative 24-hour
fallback permits one probe. Without a newer `Retry-After` signal, failed or
still-exhausted probes are limited to one per minute until observed headers
restore allowance. See
[Xero's limits](https://developer.xero.com/faq/limits).

`X-AppMinLimit-Remaining` now gates an app-wide process budget. An optional
Redis adapter coordinates tenant/app reservations, observed header budgets,
daily recovery probes, 429 cooldowns and bounded request leases across workers.
Configure both `XERO_REDIS_URL` and `XERO_REDIS_APP_KEY` consistently on every
worker sharing the app. Verified TLS is required for non-loopback TCP; startup
or runtime backend failures fail closed. Without these variables the service
remains process-local. This does not coordinate v1 or unconfigured services.
See [shared quota operations](OPERATIONS.md#shared-xero-quotas) for deployment
and the 120-second lease limitation. The connected local read-only pilot does
not enable Redis or authorize warehouse extraction.

Bulk Accounting and extended collections now upload each page before pulling
the next, using the current token per request and a 50 MiB raw-page ceiling.
Late failures retain committed counts in error manifests; capped or failed
extended lists do not publish complete membership snapshots. Detail expansion
uses bounded private disk staging to retain the original collection until all
details succeed. See [bulk extraction operations](OPERATIONS.md#bounded-bulk-extraction)
for the 512 MiB staging budget, cancellation/crash cleanup, and reconciliation
requirements. Compatibility collector APIs still return buffered page vectors.

## Extraction and raw-data safety

`xero_extract` requires an explicit tenant, `destination` (`local` or `gcs`),
idempotency key, and plan. Supported plans are Accounting entity modes,
Extended family sync, one pinned Accounting operation, and explicit-parent
Accounting attachment metadata. It reuses `xero-sync`, the existing local/GCS
raw sinks, sidecars, and run manifests; it does not make a query preview a
warehouse load. Extraction starts asynchronously, with at most two active
jobs. The same client/tenant/idempotency key returns the prior job for the
same plan and conflicts on a different plan. Job receipts persist under
`XERO_MCP_STORAGE_ROOT/jobs` without response payloads or secrets. Cancellation
is best effort: inspect manifests and reconcile partial raw objects before a
replacement run. ZDR or missing extraction permission denies the job before
anything is persisted.

Run one extraction worker per storage root. Local receipts do not provide a
distributed scheduler or cross-instance idempotency lock. At most 128 jobs
are cached in memory per worker; completed entries are evicted without losing
their indexed, persisted receipts. Sensitive receipts recheck the current
client permission. A running receipt from a previous process is reported as
interrupted, never silently resumed. Queued work has not executed: it can be
cancelled or recovered only by an explicit same-key, same-plan retry after
storage is repaired. Cold cancellation and same-key recovery are serialized
within the worker; a confirmed queued cancellation cannot revive that job.
Running-state receipts are synced before extraction
begins; files and directory entries are synced on Unix. Damaged/unreadable
idempotency receipts fail closed. Receipt-write failures produce
`receipt_unavailable` and require manifest reconciliation, not a success claim.

For GCS, keep `GCS_PREFIX=raw/xero`, provision a private raw bucket and loader
before sending data, and use a separate restricted
`XERO_SENSITIVE_GCS_BUCKET` for Bank Feeds/Finance. Binary Accounting output
requires a separate private `XERO_DOCUMENT_GCS_BUCKET`. The PKCE connection
store bucket must also be separate. Apply bucket-level IAM, encryption,
retention, audit logging, and the appropriate restricted loader/dataset as in
[operations](OPERATIONS.md) and the
[extended runbook](XERO_EXTENDED_RUNBOOK.md). Local output contains real data;
use a restricted volume and retention policy. A completed job means the raw
run finished, not that ETL, staging, or Silver models are current.

The design adapts the discovery/tool and prompt ideas from Xero's
[MCP server](https://github.com/XeroAPI/xero-mcp-server),
[agent toolkit](https://github.com/XeroAPI/xero-agent-toolkit),
[command-line project](https://github.com/XeroAPI/xero-command-line), and
[prompt library](https://github.com/XeroAPI/xero-prompt-library). The
transport, authorization, bounded results, and extraction implementation are
native to this repository. No hosted LLM runs in this service.

## Validation

On 2026-10-03, the local PKCE-backed HTTP smoke probe successfully negotiated
MCP `2024-11-05`, discovered the compact seven-tool surface, listed only its
single authorized connection, and made 11 successful source GETs (10 distinct
operations) across Accounting, Assets, and Projects, all HTTP 200:

- Accounting: `getInvoices` pages 1 and 2, `getAccounts`, `getOrganisations`,
  `getContacts`, `getCurrencies`, `getItems`, `getTaxRates`, and
  `getTrackingCategories`.
- Assets: `getAssets`, filtered to `DRAFT`.
- Projects: `getProjects`; its empty requested page is not proof of an empty
  complete collection.

The probe also sliced the cached invoice response, rendered all six prompts,
validated discovery counts for all six API families, and denied cross-tenant,
invalid-parameter, sensitive, and extraction requests. The restarted worker
passed the same probe: a cached invoice slice took 2 ms, the initial source
read took 1294 ms, and subsequent source reads took 222-577 ms. These are
observational timings, not a throughput benchmark. Only counts, timings, and
coverage flags were printed, not financial
records. Extraction and sensitive access were disabled; no Xero writes or
cloud loads were run.

Extraction responses expose at most 100 sanitized entity outcomes and state
whether that list was truncated. The private raw manifest remains the full
run record. `complete` describes the requested raw run, not whole-collection
coverage or completed warehouse modeling.

Repeat the read-only probe with a private service-credential file:

```bash
cargo run -p xero-mcp --example live_smoke -- \
  --url http://127.0.0.1:5003/mcp \
  --tenant <authorized-tenant-uuid> --token-file <private-service-token-file>
```

Add `--extended` to run eight additional bounded source GETs, for 11 total.
Unavailable optional routes are reported with a sanitized error code, not an
empty successful dataset. The probe never starts an extraction, even when
the credential permits one.

Automated tests cover pinned schemas/parameters for every GET, stdio and HTTP
handshakes, pagination, tenant/client isolation, host/origin rejection, rate
limits, prompts, result expiry/projection/binary policies, malformed receipts,
idempotency/cancellation, CLI startup, and failure manifests from the native
extraction pipeline. These are not evidence of every endpoint returning data.

The final local workspace check passed 264 tests, including 45 MCP tests,
and strict Clippy. Source-file line coverage reported by `cargo llvm-cov` was:

| Changed component | Covered lines | Coverage |
| --- | --- | --- |
| MCP source | 2487/2696 | 92.25% |
| Native rate limiter | 343/422 | 81.28% |
| Native catalogued GET fetcher | 407/461 | 88.29% |

These source-file reports include compiled inline unit-test code; they are
not whole-workspace coverage or proof of every live Xero route. The
dependency advisory audit reported no vulnerabilities or warnings after
compatible lockfile updates. The non-root `Dockerfile.mcp` image built and
ran its credential-free capability manifest successfully. Reproduce the
source checks with:

```bash
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt -p xero-mcp -p xero-client -p xero-sync -- --check
cargo audit
cargo llvm-cov -p xero-mcp -p xero-client -p xero-sync --all-targets --json \
  --output-path out/mcp-streaming-quota-coverage.json -- --test-threads=1
```

The coverage report is local build output, not financial data. Docker's
build context excludes environment files, token stores, local raw runs,
job receipts, and the local source index.

The changed crates pass the scoped format check. The workspace-wide format
check still reports pre-existing differences in other crates; this audit does
not reformat those unrelated files.

Remaining production validation:

- Exercise the actual CoreSystem HTTP client, credential broker, and governed
  agent loop; a compatible local handshake does not bypass its execution gates.
- Live-test each entitled family with one non-production tenant, including
  401/403, 429/retry, pagination, binary access, tenant isolation, cancellation,
  and raw/loader count reconciliation. No claim is made that all 186 catalogued endpoints
  have been called successfully.
- Reconcile streaming raw/loader counts in a private warehouse test and enable
  the shared Redis coordinator on every worker before multi-instance rollout.
  Local fixture coverage is not a production Redis or warehouse certification.
- Add per-client cache fairness if guaranteed handle availability is required.
- See [Model Plane mapping](XERO_MODEL_PLANE_MAPPING.md) for the separate
  credential adapter and registration gates; direct CoreSystem registration
  is not available merely because the local HTTP endpoint exists.
