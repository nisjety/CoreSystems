# `details.reason` values

One closed list of the machine-readable reasons the edge puts in `error.details.reason` for a 429 or a 404. A client keys on the `reason`, never on the message. A design that adds a reason appends it here in the same change.

## 429 `RATE_LIMITED` on `POST /v1/agent/runs` (run caps)

Checked in this order, before the browser driver's own capacity (R-26). The body is
`details {reason, scope, limit, active, retry_after_seconds}` and the gateway turns it
into `browser_capacity_reached` with `Retry-After`. Retryable.

| reason | scope | meaning | knob (`QUARRY_EDGE__`) |
|---|---|---|---|
| `global_run_cap` | `global` | the edge has this many live runs | `MAX_CONCURRENT_RUNS` (32) |
| `org_run_cap` | `org` | this organization has this many live runs | `MAX_CONCURRENT_RUNS_PER_ORG` (8) |
| `org_interactive_run_cap` | `org` | this organization has this many of a person's own sessions open; the sub-cap leaves room for its agent runs | `MAX_INTERACTIVE_RUNS_PER_ORG` (6) |

`0` disables a cap.

## 429 `RATE_LIMITED` on `POST /v1/crawl` and `POST /v1/batch`

| reason | scope | meaning |
|---|---|---|
| `org_backlog_full` | `org` | the organization already has `limit` jobs waiting (job admission) |

## 429 `RATE_LIMITED` when a page is rendered

| reason | scope | meaning |
|---|---|---|
| `org_render_cap` | `org` | the organization holds its share of the browser pool |

## 404 `NOT_FOUND`

| reason | meaning |
|---|---|
| `run_not_live` | the run was closed or released (by its owner or by the sweeper) while the request was waiting for it; a non-ZDR run can be resumed with `resume_run_id` |
