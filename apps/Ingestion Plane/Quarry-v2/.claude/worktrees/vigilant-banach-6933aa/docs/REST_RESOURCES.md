# REST Resource Breadth — Part 1

Cycle 22 / cluster #4 part 1.

## Resource matrix

| Resource          | Path                          | Owner            | Source       | Status |
| ----------------- | ----------------------------- | ---------------- | ------------ | ------ |
| Artifacts         | `GET /v1/artifacts`           | quarry-edge      | `ArtifactStore.list` | ✅ shipped |
| Sources           | `GET /v1/sources`             | quarry-control   | forward      | ✅ edge route + forward stub; Go side pending |
| Snapshots         | `GET /v1/snapshots`           | quarry-control   | forward      | ✅ edge route + forward stub; Go side pending |
| Crawl jobs        | `GET /v1/crawl/jobs`          | quarry-control   | forward      | ✅ edge route + forward stub |
| Search jobs       | `GET /v1/search/jobs`         | quarry-control   | forward      | ✅ edge route + forward stub |
| Extract jobs      | `GET /v1/extract/jobs`        | quarry-control   | forward      | ✅ edge route + forward stub |
| Research jobs     | `GET /v1/research/jobs`       | quarry-control   | forward      | ✅ edge route + forward stub |
| Agent jobs        | `GET /v1/agent/jobs`          | quarry-control   | forward      | ✅ edge route + forward stub |
| Batch jobs        | `GET /v1/batch/jobs`          | quarry-control   | forward      | ✅ edge route + forward stub |
| Request queues    | `GET /v1/request-queues`      | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Benchmarks        | `GET /v1/benchmarks`          | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Team credit usage | `GET /v1/team/credit-usage`   | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Team token usage  | `GET /v1/team/token-usage`    | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Team concurrency  | `GET /v1/team/concurrency`    | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Team queue-status | `GET /v1/team/queue-status`   | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Team activity     | `GET /v1/team/activity`       | quarry-control   | forward      | ✅ cycle 23 — edge route + forward stub |
| Schedules (list)  | `GET /v1/schedules`           | quarry-control   | forward      | ✅ cycle 23 |
| Schedules (lifecycle) | `POST /v1/schedules + :id/{pause,unpause,trigger,backfill}`, `DELETE :id` | quarry-control | forward | ✅ cycle 23 — see SCHEDULES.md |

## Pagination contract

Every list endpoint accepts the same query-string shape:

```
GET /v1/<resource>?
    status=<string>&
    created_before=<RFC3339>&
    created_after=<RFC3339>&
    limit=<1..100, default 25>&
    cursor=<opaque>&
    sort=<newest|oldest|asc|desc>
```

Server-side `org_id` always overrides any client-supplied `org_id` —
the verified JWT claim is the only tenant gate.

## Response envelope

```jsonc
{
  "request_id": "req_01H...",
  "status": "ok",
  "data": {
    "items":           [ /* resource summaries */ ],
    "next_cursor":     "<opaque>" | absent,
    "total_estimated": 1234       | absent
  }
}
```

- `next_cursor` is present iff there are more pages. Treat it as a
  black box and pass it back unchanged in the next request.
- `total_estimated` is OPTIONAL — backends omit it when an exact count
  is too expensive. Clients show "more available" rather than a hard
  number in that case.

## Cursor shape (server-side detail)

Cursors are base64-url-no-pad JSON of `{ created_at, id }`. They're
deliberately opaque so we can grow the cursor shape later without
breaking older clients. A malformed cursor is treated as "start from
the top" rather than 4xx.

## Tenant isolation

Every backend implementation MUST scope by the verified `org_id`. The
edge enforces this for `/v1/artifacts` via the `ArtifactStore::list`
trait method (takes `org_id` as a required parameter). Forward routes
pass `?org_id=<verified>` to the control plane, which has its own
JWT middleware that re-asserts the claim before reading.

## Resource shapes

Defined in `crates/quarry-core/src/resources.rs`:

- `Source { source_id, org_id, name, url, kind, status, created_at,
  updated_at, config }`
- `Snapshot { snapshot_id, org_id, source_id?, url, fingerprint,
  prev_fingerprint?, change_status, captured_at, artifact_id? }`
- `ArtifactSummary { artifact_id, org_id, kind, bytes, sha256?,
  created_at, source_url? }`
- `JobSummary { job_id, kind: JobResourceKind, org_id, status,
  created_at, started_at?, completed_at?, stats }`

`JobResourceKind = Crawl | Search | Extract | Research | Agent | Batch`.

## What's still pending (cluster #4 part 2 — cycle 23)

- `/v1/request-queues`, `/v1/benchmarks` resource lists
- `/v1/team/*` family (credit-usage, token-usage, concurrency,
  queue-status, activity)
- Go control-plane handlers (the forward target for everything except
  `/v1/artifacts`)
- HMAC service-to-service auth on the edge→control forward path
  (cluster #14)

## Tests

- 11 pagination contract tests (`crates/quarry-core/src/pagination.rs`)
- 3 resource shape tests (`crates/quarry-core/src/resources.rs`)
- 4 `ArtifactStore::list` tests (org isolation, cursor pagination,
  default impl returns empty)
- 3 `resource_routes::ListQuery` parsing tests (RFC3339, sort
  validation, bad-input rejection)
