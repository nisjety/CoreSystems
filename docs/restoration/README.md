# Local work restoration

Restored on branch `codex/restore-local-work`, based on CoreSystems/dev
`461b155a4505750911b3742fa9920248bbcaa34f`.

The source is the preserved `CoreSystem-backup-20261029-before-dev` checkout,
whose Git HEAD is `da6fa0d45d8d646a3acace01fcdedcec883b96dd`. Existing files,
including uncommitted edits and untracked nested repository contents, are
restored with their supporting baseline files. Local deletions remain absent.
Nested repositories are imported as source files, not empty Git links; their
original history remains in the backup. `restored-paths.json` lists copied files.

## Layout and reconciliation

- `frontend/` remains the dev Next.js app.
- `Application Plane/` remains the dev stack and event contract.
- `apps/` restores the original multi-plane layout, Solid Verevon v3 app,
  Rust gateway, separate marketing app, Engagement and connector services.
  Keeping original paths preserves cross-plane scripts and relative contracts.
- The only root image collision is preserved as `backup-root-image.png` here;
  dev's `image.png` is unchanged.
- Ignored dependencies, build caches, runtime data and private environment files
  are retained in the backup rather than imported. Reinstall dependencies and
  configure environments per package. Git-ignored restored files remain on disk.

The two Application Plane overlaps use explicit contract selection:

1. `convex-core/nats-subscriber.js` retains the dev class, exports, event subjects,
   webhook routing and default startup. `NATS_SUBSCRIBER_MODE=control-projection`
   selects the restored subscriber, preserved separately as
   `nats-subscriber.control-projection.cjs`. Its scoped credentials and matching
   projection endpoints are required. Invalid mode names fail before connection.
   The Dockerfile includes both implementations.
2. The base `docker-compose.yml` remains byte-for-byte dev's version. The opt-in
   `docker-compose.control-projection.yml` changes the schema mount and deployer/
   subscriber build contexts together to the restored Convex package. This avoids
   mixing revisioned projection events with dev's older webhook implementation.

To select the restored projection contract, from `Application Plane/`:

```sh
docker compose -f docker-compose.yml -f docker-compose.control-projection.yml config --quiet
# After configuring credentials and ensuring dependent services are available:
docker compose -f docker-compose.yml -f docker-compose.control-projection.yml up -d --build
```

Configure `APPLICATION_CONVEX_CONTROL_NATS_USER`,
`APPLICATION_CONVEX_CONTROL_NATS_PASSWORD`, `CONVEX_CONTROL_PROJECTION_KEY`, and
optionally `CONTROL_PROJECTION_NATS_URL`. The restored full multi-plane stack
also remains available through `apps/Application Plane/docker-compose.yml`.
The two stack layouts share container names and ports: run one layout at a time.

## Validation

- Restored Convex package: 60 tests pass; `tsc --noEmit` passes.
- Subscriber entry points pass Node syntax checks.
- Base plus projection Compose override passes `docker compose config --quiet`.
- Restoration verification compares every copied file with the backup and checks
  that dev's files are unchanged except the documented subscriber/Dockerfile edits.

This is a source recovery, not a deployed or fully integrated six-plane release.
The full Go/Rust/frontend and infrastructure suites have not all been run; each
package needs its dependencies, configuration and runtime services for those
checks. Nothing has been pushed or deployed.
