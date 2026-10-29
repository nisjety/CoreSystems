# Legacy Aquatiq Artifact Boundary

The active infrastructure contract is now **CoreSystem Infra Plane**. Its only
supported local runtime is [`docker-compose.yml`](docker-compose.yml), started
with [`start-local.sh`](start-local.sh).

The older shared-data, external deployment, webhook, Cloudflare, and
integration-management material is retained only as historical migration
reference. It is not composed, built, or called by the active local stack.

In particular, do not use the historical material to:

- provision a shared database, cache, broker, search index, object store, or
  workflow engine for other CoreSystem planes;
- mount Docker's socket into a CoreSystem runtime service;
- expose an external route or initiate an automated deployment;
- move product authorization, tenant scope, or provider credentials into the
  Infra Plane.

Before removing this historical material permanently, first confirm that no
operator runbook or external system still depends on it. The CoreSystem runtime
does not depend on it.
