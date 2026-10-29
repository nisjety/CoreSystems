# Knowledge lint

## Product role

Knowledge lint continuously audits the knowledge plane for structural defects:
**orphan pages**, **stale claims**, **missing cross-links**, and **missing
pages**. Lint findings are themselves derived artifacts with provenance — they
do not mutate the knowledge store, they surface work for synthesizers and
operators.

## Transport

- **Checks**:
  - **Orphan pages** — wiki pages with no inbound back-links and no active
    source events.
  - **Stale claims** — graph edges or wiki claims whose source events have
    aged beyond a policy-defined threshold without corroboration.
  - **Missing cross-links** — entities or topics referenced in text without
    corresponding graph edges or wiki back-links.
  - **Missing pages** — entities referenced in graph memory with no wiki page.
- **Execution**: scheduled by `capability-core`; checks run inside
  `inference-core` against the current graph and wiki state.
- **Query**: `GET /v1/knowledge/lint/*` returns findings with severity,
  affected artifact IDs, and provenance. HTTP is GET-only; non-GET returns
  405 with `Allow: GET`.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Check execution (orphan / stale / missing-link / missing-page) | `inference-core` |
| Finding materialization and storage | `inference-core` |
| Catalog, policy, retention, scope, scheduling metadata | `capability-core` |
| Scheduling and cadence of lint runs | `capability-core` |

## `capability-core` responsibilities

- **Catalog** — register lint checks as capabilities with explicit versions.
- **Policy** — enforce scope on lint queries; define staleness thresholds and
  severity gates per org.
- **Metadata** — durable records of lint runs, findings, and resolutions.
- **Scheduling hints** — cadence and triggers for lint runs (post-materialization,
  periodic, on-demand), delegated to Rust workers.

## Reference inputs

- `LLM Wiki` — page-graph coherence patterns.
- `logseq` — back-link and orphan detection.

References only; no code is copied.

## Out of scope

- Automatic mutation of the knowledge store (lint surfaces findings, does not
  rewrite claims).
- Free-form user-authored lint rules (checks are registered capabilities).
- Check implementation details (owned by `inference-core`).
