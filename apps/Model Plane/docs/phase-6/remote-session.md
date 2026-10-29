# Remote Session

## Product role

A durable, resumable session that survives client disconnects, device handoff,
and backend restarts. Powers long-running agent tasks (hours to days) and
shared collaborative sessions.

## Transport

- **Session creation**: HTTPS POST to `model-gateway`.
- **Attach / resume**: WebSocket to `model-gateway`, which proxies to
  `execution-core`.
- **State durability**: session journal persisted by `execution-core` (Temporal
  workflow per Phase 0).

## Backend ownership

| Concern | Owner |
|---------|-------|
| Session lifecycle (create/resume/close) | `execution-core` |
| Journal durability | `execution-core` + Temporal |
| Ingress / fan-out to multiple attached clients | `model-gateway` |
| Policy gate on session start | `capability-core` |

## `capability-core` responsibilities

- **Policy**: max-session-duration, max-parallel-sessions-per-org, allowed tool
  classes.
- **Catalog**: which models may be pinned for a session’s duration.
- **Metadata**: session tags, retention class, export eligibility.
- **Scheduling hints**: affinity to a region for sticky sessions.

## Acceptance properties

1. Resume from arbitrary offset within the session journal.
2. Multiple attached viewers see a consistent event stream (no tearing).
3. Policy changes mid-session take effect at the next tool call, not
   retroactively.

## Reference inputs

- `openclaw` — remote session semantics and detach/attach flows.

## Out of scope

- Real-time co-editing CRDT (belongs to a future collaboration plane).
