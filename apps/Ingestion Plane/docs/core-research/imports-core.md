# imports-core Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/imports-core`

## Snapshot

`imports-core` is the file and source import intake API. It accepts upload and source-import jobs, tracks progress, publishes events, and participates in Control Plane-driven org-state behavior.

Current evidence highlights:

- Python FastAPI service
- async job intake plus orchestration dispatch
- shared NATS publisher and Control Plane subscriber are both live at startup
- historical legacy-table migration logic still exists in the database layer

Non-generated, non-vendored file count from the current tree: about `22`.

## Runtime Shape

Key runtime entrypoints:

- `app/main.py`
  - FastAPI app, DB migrations, shared HTTP client, event publisher, shared NATS publisher, Control Plane subscriber, import job routes
- `app/service.py`
  - import-service core behavior
- `app/orchestration.py`
  - background dispatch
- `app/control_plane_subscriber.py`
  - Control Plane event subscription
- `app/auth_middleware.py`
  - internal auth boundary

Primary surfaces:

- `/health`
- `/api/v1/import/jobs/upload`
- `/api/v1/import/jobs/source`
- additional job status and progress endpoints behind the app module

## API And Relationship Map

Current relationships:

- Frontend/Application Plane -> `imports-core`
  - file-upload and source-import intake
- `imports-core` -> Postgres
  - job and item state
- `imports-core` -> shared NATS and local event publication
  - cross-plane signaling
- `imports-core` -> Control Plane subscriber
  - org/user/provider-linked and pause-state behavior

## Duplicates, Redundancies, And Inactive Surfaces

Historical migration residue:

- `app/db.py` still archives legacy imports tables by renaming them with `_legacy` suffixes

That is not dead code by itself, but it is a sign of schema-transition history still living in the runtime.

## Stubs, Placeholders, And Missing Connections

Observed transitional behavior:

- `app/control_plane_subscriber.py` still documents a legacy `velion.controlplane.user.provider_linked` setup hook path
- `app/service.py` still consults an in-memory paused-orgs set populated by the Control Plane subscriber

This pass did not find explicit `.unused` or `.backup` residue in the active service tree.

## API Design And Performance Notes

API design:

- intake ownership is clear
- separating upload and source-import job creation is appropriate

Performance and operational notes:

- shared HTTP client initialization is good and avoids per-request connection churn
- subscriber-driven in-memory org pause state is lightweight, but it means behavior can diverge if subscription health degrades

## Current Doc Cleanup Read

Keep:

- `imports-core/README.md`

Review or archive, not delete:

- `IMPORTS_COMPLETION_SUMMARY.md`
  - milestone framing, not current runtime truth

## Bottom Line

`imports-core` is real and active. The interesting debt is migration and subscriber-era residue, not whether the import intake path exists.
