# Studio local Ingestion subset

The canonical entrypoint is this plane's `docker-compose.yml`. The local overlay
`docker-compose.studio-local.yml` selects Quarry-v2 and Integration Core plus
their required infrastructure. It does not start Model Plane, Data Plane,
Imports, Shipping, autocomplete, Finspo, or connector workers.

From this plane's directory:

```sh
INTEGRATION_LOCAL_PAGESPEED=1 INTEGRATION_LOCAL_CLARITY=1 \
INTEGRATION_LOCAL_CLARITY_TRIODELAB=1 \
  sh scripts/run-studio-ingestion.sh config --quiet

INTEGRATION_LOCAL_PAGESPEED=1 INTEGRATION_LOCAL_CLARITY=1 \
INTEGRATION_LOCAL_CLARITY_TRIODELAB=1 \
  sh scripts/run-studio-ingestion.sh up -d --build
```

The launcher reuses the protected Integration provider projection and existing
per-core/shared interpolation stores; it does not generate or rotate credentials.
Do not print unrestricted `config`, container environments, or secret files in
shared logs. The optional Meta overlays remain explicit (`INTEGRATION_LOCAL_META=1`).
Do not enable the `full-ingestion`, legacy, or support profiles for this subset.

## One-time transition from the earlier mini-stacks

1. Verify the current Integration database mount is exactly
   `studio-integration-local_integration-postgres-data`. Back up the database
   through the running database owner's approved backup path before switching.
2. Preserve the original vault encryption key, Google client secret, Clarity
   tokens/bindings, PageSpeed key and callback `http://localhost:3026`. A new empty
   database or a changed encryption key will make existing grants unavailable.
3. Stop `studio-integration-local-integration-api-1` first, then
   `studio-integration-local-integration-postgres-1`, then
   `studio-quarry-local-quarry-edge-1`. Never run two Postgres containers against
   the same volume, and do not leave duplicate `integration-api`/`quarry-edge`
   network aliases active.
4. Validate the canonical configuration with `config --quiet`, then bring it up
   with the command above. The external Integration volume is reused unchanged.
   Canonical Quarry infra uses its own Ingestion-owned database, not Integration's.
5. Verify health/readiness, authenticated Search Console/GA4/Clarity reports, and
   explicit bounded Quarry discovery. Missing Model/Data Plane features must
   remain unavailable, not return fabricated success.
6. Retain stopped former containers and all volumes until verification is complete.
   Never use `down -v`, volume prune, or broad system prune as part of migration.

Rollback: stop the new canonical Integration API/database and Quarry edge before
restarting the former containers. No volume rename, credential replacement or
database copy is needed. Keep canonical Quarry infra stopped if rolling back the
whole subset. Existing grant storage belongs only to Integration Core.

Quarry's capture result is not a durable Data Plane acknowledgement. In this
subset the Data/Model URLs are explicitly empty; durable ingest/AI requests must
fail closed until their owner planes are intentionally enabled. Rebuild the edge
after source changes; an older local image does not include new guards.

Contract check:

```sh
node --test scripts/studio-ingestion.test.mjs
```
