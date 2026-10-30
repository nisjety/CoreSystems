# xero_service_v2

Raw-to-GCS ingestion for Xero. The `xero` CLI fetches Xero Accounting
API pages and writes the **verbatim** response bytes to a GCS bucket (one object
per page), then exits. All dedup / merge / modeling happens downstream
(Cloud Function → BigQuery → Dataform). Multi-tenant, with Custom Connection
or PKCE auth, driven by an external scheduler.

The CLI is a one-shot job. A separate `xero-bff` HTTP service handles PKCE
onboarding. PKCE connection state is persisted in a local file for development
or a dedicated GCS bucket for multi-instance deployment.

The separate `xero-mcp` service exposes authenticated AI tools, private result
resources, analytical prompts, and permission-gated raw extraction over stdio
or Streamable HTTP. It reuses the connector's token providers and sync jobs;
model execution stays with the calling agent platform.

## At a glance

| Concern             | Choice                                                       |
|---------------------|--------------------------------------------------------------|
| Language            | Rust workspace                                                |
| Entry point         | `xero` CLI — one-shot job invoked by cron / scheduler        |
| Destination         | GCS bucket (raw verbatim pages + `.meta.json` sidecars)      |
| State               | Raw objects plus optional PKCE connection store               |
| Auth                | Custom Connection (`client_credentials`) or PKCE              |
| Incremental         | Rolling `now − N days` window, computed per run              |
| Rate limit / retry  | Adaptive header-driven throttle + exp backoff with jitter    |
| Backfill            | `--backfill FROM:TO` expanded into business-date chunks      |
| Cadence             | Host scheduler (see [deploy/crontab.example](deploy/crontab.example)) |

## Docs

- **New architecture:** [docs/NEW_ARCHITECTURE_RAW_GCS.md](docs/NEW_ARCHITECTURE_RAW_GCS.md) — design, GCS layout, metadata, cadence rationale
- **Operations:** [docs/OPERATIONS.md](docs/OPERATIONS.md) — env vars, cadence table, deploy, backfill, live validation
- **Extended APIs:** [docs/XERO_EXTENDED_RUNBOOK.md](docs/XERO_EXTENDED_RUNBOOK.md) — Assets, Projects, Bank Feeds, Identity, Finance
- **Endpoint parity:** [docs/XERO_ENDPOINT_PARITY.md](docs/XERO_ENDPOINT_PARITY.md) — pinned OpenAPI counts and remaining gaps
- **AI/MCP service:** [docs/XERO_MCP.md](docs/XERO_MCP.md) — 150 read operations, eight compact tools, transports, security, extraction, and tenant-scoped warehouse counts
- **CoreSystem mapping:** [docs/XERO_MODEL_PLANE_MAPPING.md](docs/XERO_MODEL_PLANE_MAPPING.md) — Model Plane contract and remaining integration gates
- **Cron cadence:** [deploy/crontab.example](deploy/crontab.example) — the layered schedule, ready to install

## CLI surface

```text
xero sync [flags]      Fetch raw Xero pages and land them in GCS (or local disk).
xero sync-extended     Opt-in read-only Assets, Projects, Bank Feeds, Identity and Finance.
xero healthcheck       Mint a token per tenant; --check-bucket also probes GCS.
```

The sync *mode* is derived from the flags (precedence top→bottom):

| Flag(s)                                | Mode             |
|----------------------------------------|------------------|
| `--backfill FROM:TO`                   | backfill (business-date chunks) |
| `--reports` (needs `--as-of`)          | report snapshots |
| `--where <expr>` / `--no-window`       | open-sweep (status filter, no window) |
| `--business-from` / `--business-to`    | rolling-full (explicit business window) |
| `--full-with-window`                   | rolling-full (configured tight window) |
| `--full`                               | master (no filter, master entities) |
| _(default)_                            | incremental (modified ≥ now − window) |

Other flags: `--window-days N`, `--chunk-months N`, `--as-of YYYY-MM-DD`,
`--entity <csv>`, `--tenant <csv>`, `--max-concurrent N`, `--dry-run`,
`--local-dir <path>`, `--expand-details`, `--max-detail-records N`.

## GCS object layout

```text
object:   {GCS_PREFIX}/{tenant}/2.0/{endpoint}/{YYYY-MM-DD}/{ts}_{run_id_short}_p{NNN}.json
report:   {GCS_PREFIX}/{tenant}/2.0/{endpoint}/{YYYY-MM-DD}/{ts}_{run_id_short}__{period_key}.json
sidecar:  <object-key>.meta.json
manifest: _manifests/xero/{YYYY-MM-DD}/{run_id}.json      (OUTSIDE GCS_PREFIX)
```

`GCS_PREFIX` defaults to `raw/xero`. The object body is the exact bytes Xero
returned for that page (envelope intact). Custom object metadata (lowercase
string keys) records `x-vendor`, `x-tenant-id`, `x-org-name`, `x-endpoint`,
`x-api-version`, `x-sync-type`, `x-page`, `x-record-count`, `x-http-status`,
`x-run-id`, `x-synced-at`, plus window/report keys as applicable. See
[docs/NEW_ARCHITECTURE_RAW_GCS.md](docs/NEW_ARCHITECTURE_RAW_GCS.md) for the full
metadata table.

## Quick start (local dry-run, no GCS needed)

A dry-run mirrors the production object layout onto local disk — no bucket, no
GCS credentials required (only Xero custom-connection creds).

```bash
cp .env.example .env        # fill in XERO_ORG_1_* (and friends)

# Build + run a dry-run of the report snapshots into ./out
cargo run -p xero-cli -- sync --reports --as-of 2026-06-17 \
    --dry-run --local-dir ./out

# Inspect the raw object layout
find ./out -type f
```

Or via docker compose (writes to `./out`):

```bash
docker compose run --rm xero sync --reports --as-of 2026-06-17 \
    --dry-run --local-dir /out
```

## Auth in one paragraph

For Custom Connections, each Xero organisation has its own
`client_credentials` app; configure numbered blocks
`XERO_ORG_N_{CLIENT_ID,CLIENT_SECRET,TENANT_ID,NAME}` (or the single-org
`XERO_CC_*` form). `client_credentials` issues no refresh token, so there is
nothing to persist. For PKCE, `xero-bff` onboards tenants and the CLI reads a
shared refresh-token store. Production PKCE requires the GCS backend and the
deployment checks in [docs/OPERATIONS.md](docs/OPERATIONS.md); it has not yet
been verified against a live bucket and tenant.

## What gets synced

`EntityType::all()` covers the Accounting-API entities (Invoices, Payments,
Contacts, Journals, …). The default entity set depends on the sync mode:

- **incremental / backfill** — full accounting set (reports excluded)
- **master** (`--full`) — `accounts`, `contacts`, `items`, `tax_rates`,
  `tracking_categories`, `currencies`, `organisations`, `users`, `branding_themes`
- **open-sweep** (`--where … --no-window`) — `invoices`, `bills`
- **rolling-full** — transactional entities (all minus master)
- **reports** (`--reports`) — the report endpoints (per-contact aged reports are
  excluded by default; aging is derived downstream)

See `core/crates/xero-common/src/types.rs` for the authoritative list.

## Layout

```text
xero_service_v2/
├── core/crates/
│   ├── xero-common/    # Shared types, errors, config (GCS + sync config)
│   ├── xero-auth/      # Custom Connection and PKCE token providers
│   ├── xero-store/     # PKCE connection store (file or GCS)
│   ├── xero-bff/       # PKCE onboarding HTTP service
│   ├── xero-client/    # REST client, retry, rate-limit, raw-page capture
│   ├── xero-gcs/       # RawSink trait, GcsRawSink, LocalDirSink, manifest
│   ├── xero-sync/      # Accounting and extended raw sync jobs
│   ├── xero-mcp/       # AI tools, resources, prompts, and extraction service
│   └── xero-cli/       # `xero` binary: sync, sync-extended, healthcheck
├── deploy/             # crontab.example (layered cadence)
├── docs/               # New architecture, operations, transformation plan
├── Dockerfile          # Multi-stage one-shot job image (Rust → debian-slim)
├── Dockerfile.mcp      # Non-root MCP service image
├── docker-compose.yml  # Local one-shot / dry-run convenience
├── .env.example        # Env var template (no secrets)
└── README.md           # this file
```
