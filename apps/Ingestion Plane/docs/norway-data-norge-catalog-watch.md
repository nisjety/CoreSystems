# Data.norge catalog watcher

**Status:** source-only Phase 3 handoff  
**Updated:** 2026-07-21

`Quarry-v2/services/quarry-control/internal/catalogwatch` is the first
Ingestion Plane slice for the Data.norge catalog roadmap item. It uses the
public Felles datakatalog SPARQL endpoint, submits a fixed `dcat:Dataset`
query, normalizes the result into a deterministic URI-sorted snapshot, and
computes added/removed/changed resources against a prior snapshot.

The package intentionally does not schedule jobs, write Data Plane storage, or
promote catalog changes automatically. That keeps the ownership boundary
explicit:

```text
Data.norge SPARQL
    -> quarry-control catalogwatch snapshot/diff
    -> human-reviewed Quarry schedule/resource
    -> Ingestion worker
    -> Data Plane contract
```

The watcher is bounded by a 30-second HTTP client timeout and an 8 MiB response
limit. Missing optional metadata is tolerated; rows without a dataset URI are
discarded. Duplicate rows for one URI are merged without mutating prior
snapshots.

Before scheduling this in production, add a Quarry-owned persisted snapshot
with an explicit retention policy, a review event for destructive changes, and
an ingestion adapter for only the approved catalog resources. Do not use the
Data.norge internal Search API as a fallback; the roadmap treats it as
unstable.
