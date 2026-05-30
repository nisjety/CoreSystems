# finspo-core — HTTP API

Base URL (standard stack): `http://localhost:3130`

## Authentication

- **Public:** `GET /health`, `GET /ready` — no auth.
- **All `/api/v1/*`:** require two headers:
  - API key — `X-API-Key: <FINSPO_API_KEY>` (or `x-internal-api-key: <key>`).
  - Org scope — `X-Org-ID: <organization id>`. Every query is scoped to this org.
  - `X-User-ID: <user>` — optional; recorded as the actor on proposals/audits.
    Falls back to `org:<org id>` when absent.

Responses use a consistent envelope: `{"success": bool, "data": ..., "error": ...}`.

---

## Health

| Method | Path | Notes |
|--------|------|-------|
| GET | `/health` | Liveness. `200 {"status":"ok","service":"finspo-core"}` |
| GET | `/ready` | Readiness. Pings DB + NATS. `200` ready / `503` degraded. Unconfigured deps report `skipped`. |

## SharePoint discovery — `/api/v1/sharepoint`

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/sites` | List sites visible to the org's Graph connection |
| GET | `/sites/:siteID/items?path=/` | Browse a drive path (ad-hoc; not the sync path) |

## Sources (connector state) — `/api/v1`

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/sources` | Register/upsert a drive to sync. Body: `{site_id, drive_id, tenant_id?, site_web_url?, drive_name?, drive_type?, enabled?}` |
| GET | `/sources` | List sources for the org |
| GET | `/sources/:id` | Fetch one source |
| GET | `/sources/:id/status` | Source + its `delta_cursors` row (last sync state) |
| POST | `/sources/:id/sync` | Trigger a delta sync now (synchronous; returns `SyncResult`) |

## Analytics — `/api/v1/analytics`

All read-only, org-scoped. Limits are clamped server-side.

| Method | Path | Query params | Returns |
|--------|------|--------------|---------|
| GET | `/analytics/largest` | `limit` (≤500, def 50), `min_size` (bytes) | Largest live files |
| GET | `/analytics/inactive` | `older_than` (Go duration, def 180d-equiv), `limit` | Files untouched since cutoff, with `days_since_touch` |
| GET | `/analytics/by-site` | — | Per-drive file count + total bytes |
| GET | `/analytics/duplicates` | `min_count` (≥2), `min_size`, `max_groups` (≤200) | Groups of files sharing a content hash, members inline |

## Recommendations — `/api/v1`

| Method | Path | Query params | Purpose |
|--------|------|--------------|---------|
| GET | `/recommendations` | `min_size`, `max_groups`, `older_than`, `inactive_limit` | Pre-filled proposal **drafts** (keep-newest duplicate deletes + a stale-file archive). Not persisted — POST a draft to `/proposals` to act on it. |

## Proposals (governance workflow) — `/api/v1`

| Method | Path | Purpose | Notable statuses |
|--------|------|---------|------------------|
| POST | `/proposals` | Create. Body: `{kind:"delete"\|"archive", reason, item_pks:[uuid], notes?}` | `201` |
| GET | `/proposals?status=&limit=` | List for the org | |
| GET | `/proposals/:id` | Fetch one | `404` if wrong org |
| POST | `/proposals/:id/approve` | Human gate → `approved`. Body: `{notes?}` | `409` if not `pending` |
| POST | `/proposals/:id/reject` | → `rejected` | `409` if not `pending` |
| POST | `/proposals/:id/execute` | Run the destructive action | see below |

### `POST /proposals/:id/execute` responses
| Status | Meaning |
|--------|---------|
| `200` | Executed; body is the per-item `ExecResult` |
| `404` | Proposal not found / not your org |
| `409` | Proposal not `approved` |
| `400` | `archive` proposal but `FINSPO_ARCHIVE_FOLDER_ID` unset |
| `503` | Execution disabled (`FINSPO_ALLOW_EXECUTION=false`) |

Execution requires the kill-switch on, the proposal approved, **and** the Graph
connection to carry write scopes (`Files.ReadWrite.All`). See
[REQUIREMENTS.md](REQUIREMENTS.md#execution-safety-model).

---

## Example: full governance loop

```bash
KEY=...   # FINSPO_API_KEY
ORG=org_123
H=(-H "X-API-Key: $KEY" -H "X-Org-ID: $ORG" -H "X-User-ID: alice")

# Find duplicate-cleanup candidates
curl -s "${H[@]}" "http://localhost:3130/api/v1/recommendations?min_size=1048576" | jq

# Create a delete proposal from a draft's item_pks
curl -s "${H[@]}" -H "Content-Type: application/json" \
  -d '{"kind":"delete","reason":"older duplicate","item_pks":["<uuid>"]}' \
  http://localhost:3130/api/v1/proposals | jq

# Approve, then (if FINSPO_ALLOW_EXECUTION=true) execute
curl -s "${H[@]}" -X POST http://localhost:3130/api/v1/proposals/<id>/approve | jq
curl -s "${H[@]}" -X POST http://localhost:3130/api/v1/proposals/<id>/execute | jq
```
