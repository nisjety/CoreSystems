# WS1.C compose lines (not applied: compose files are outside unit R3-ws1c)

IMPORTANT: from the WS1.C commits on, a rebuilt browser-broker image EXITS 1 at boot unless
`DATABASE_URL` is set or `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true`. Apply the base-file
lines below before rebuilding `model-plane-browser-broker`.

## apps/Model Plane/deploy/docker-compose.yml, service `browser-broker`

Under `environment:` (after `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER`):

```yaml
      # Durable grant store (WS1.C). Shared session_core database for now
      # (X-15); a dedicated database and least-privilege role is a hardening
      # follow-up. ZDR grants are never written here (X-2): they stay in
      # memory, which is why the broker stays one replica.
      DATABASE_URL: postgresql://postgres:${MODEL_POSTGRES_PASSWORD:?MODEL_POSTGRES_PASSWORD is required}@postgres:5432/session_core
```

New `depends_on:` block (the service has none today):

```yaml
    depends_on:
      postgres:
        condition: service_healthy
```

Optional: point the healthcheck at readiness only if a DB outage should mark the container unhealthy
(`/readyz` now pings Postgres and returns 503 when it does not answer); `/healthz` stays liveness-only.
The plan does not ask for this; leave `/healthz` unless decided otherwise.

## apps/Model Plane/deploy/docker-compose.override.yml (dev only), service `browser-broker`

Nothing is required once the base file has DATABASE_URL. For a database-less dev experiment only:

```yaml
      BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT: "true"
```

(Then also remove DATABASE_URL for that run: with DATABASE_URL set, the Postgres store is used.)

## docker-compose.production.yml

No change (broker is already `*hardened-service`). Production must never set the ephemeral flag.
