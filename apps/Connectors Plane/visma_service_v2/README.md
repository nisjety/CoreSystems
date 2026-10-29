# visma_service_v2

Sync platform for Visma.net that replaces the v1 Python service (`../`). It lands what Visma returns as raw
JSON in BigQuery staging, and the Dataform project in `dataform/` builds the warehouse Power BI reads.

```
Visma ERP REST · DataMart GraphQL · Expense API
        │   vsv2: Rust orchestrator + Python writers, state in Postgres, limits in Redis
        ▼
aquatiq-datavarehus.dw_staging_visma          one JSON table per endpoint (+ _sync_heartbeat)
        │   Dataform repository `Visma` (dataform/)
        ▼
visma_bronze → visma_silver → visma_gold_* → visma_datamart   (the Power BI layer)
```

## Status

**Not live.** Everything that can be proven on one machine has been: the data is checked against Visma
itself, the pipeline ran in its container against real Visma and BigQuery, and what is left needs a
decision, an account or time. The verdict, the scorecard and the order to go live in are in
[docs/PRODUCTION_READINESS.md](docs/PRODUCTION_READINESS.md). Read, in this order:
[PRODUCTION_READINESS](docs/PRODUCTION_READINESS.md) → [DEPLOYMENT](docs/DEPLOYMENT.md) (runbook,
rehearsal, monitoring) → [v2_roadmap](docs/v2_roadmap.md) (why, and the decisions) →
[gates](docs/gates/README.md) (how correctness is measured).

## Decisions (locked 2026-04-29)

| Decision | Choice |
|---|---|
| Language split | **Rust core + Python tooling** |
| Phase strategy | Full skeleton, then iterate per phase |
| State store | **Separate Postgres database** (`visma_v2`) |
| Datamart auth | **Same Visma.net OAuth scope as ERP** (reuse token manager, branch on audience if needed) |

More in [docs/DECISIONS.md](docs/DECISIONS.md).

## Layout

```
visma_service_v2/
├── core/        # Rust workspace: orchestrator (scheduler, open-set re-read), ERP/DataMart/Expense clients,
│                #   rate limiter, state store and migration runner, `vsv2` CLI (sync, schedule, monitor),
│                #   HTTP service, guarded writes (vsv2-interactive), MCP server (vsv2-mcp) with a per-person sign-in (vsv2-signin)
├── tooling/     # Python: BigQuery staging/merge/quarantine writers (vsv2-bq-*), normalizers, discovery
├── config/      # 135 endpoint configs, tenant files, DataMart GraphQL queries
├── migrations/  # Postgres state schema (applied by the binary at start)
├── dataform/    # The warehouse: bronze, silver, gold, datamart, assertions, Excel inputs, tools
├── scripts/     # The gates (a, b, c, e, s), the silver completeness audit, probes and comparisons
└── docs/        # Readiness, runbook, roadmap, gate results, audits
```

## Invariants

1. Tier 1 endpoints must not use DataMart.
2. Soft-delete only: no hard deletes against BigQuery. A complete scan sets `deleted_at` on what Visma
   stopped returning.
3. Staged rows always carry `tenant_id`, `_source` and `_synced_at` (RFC-3339 UTC).
4. DataMart endpoints require either a `datamart` block or `pilot_override`.
5. Tenant scoping via `tenant_id` for all state and all writes.
6. Live sync writes raw staged payloads, quarantines invalid rows, and advances checkpoints only after a
   validated write succeeded.

`make validate-config` enforces 1 and 4 (and the JSON schema) on every change.

## Quick start (local)

```bash
cp .env.example .env              # add the Visma credentials; .env is git-ignored, never commit it
make up                           # Postgres and Redis (docker compose)
make validate-config              # 135 endpoint configs against the schema
cargo run -p vsv2-cli -- sync --tenant <tenant-id> --endpoint customers     # one endpoint, one tenant
cargo run -p vsv2-cli -- schedule --once --tenant <tenant-id>               # one scheduler round
cargo run -p vsv2-cli -- monitor                                            # is the pipeline healthy?
docker compose build vsv2         # the image the scheduler runs in
```

The state schema is applied by `vsv2 schedule` and `vsv2-http` when they start. Visma and BigQuery
settings, the request budget and the production override are in [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md);
the same page has the dress rehearsal (§ 4c) that runs one tenant against a scratch dataset.

## Before a change is merged

```bash
make validate-config
cd core && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace                       # needs `make up`
uv run ruff check . && uv run ruff format --check . && uv run pytest
(cd dataform && dataform compile)                 # 218 tables and views, 40 assertions
```

`.github/workflows/visma-service-v2.yml` runs the same on every pull request that touches this folder.

## Demo-safe sync

Use the demo helper script for bounded one-week runs on explicit endpoints only:

```bash
# Preview only (no execution)
bash scripts/demo_week_sync.sh --endpoint cash_transactions

# Execute a bounded demo run
bash scripts/demo_week_sync.sh --endpoint cash_transactions --run --confirm-demo

# Preview with a custom one-week window
bash scripts/demo_week_sync.sh --endpoint customer_invoices --date-from 2026-04-20T00:00:00Z --date-to 2026-04-26T23:59:59Z
```

Safety notes:

- The demo script requires explicit `--endpoint` arguments and rejects `all`.
- The demo script only executes with both `--run` and `--confirm-demo`.
- `scripts/full_sweep_replay.sh` now requires `ALLOW_FULL_SWEEP=YES` unless you pass `--dry-run`.
