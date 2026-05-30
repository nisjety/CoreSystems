# Quarry Docker Stack

This stack runs Quarry and core infrastructure on one Docker network for internal service communication.

## Services

- `quarry-api` (Fiber API)
- `quarry-worker` (Temporal worker)
- `temporal` + `temporal-ui`
- `nats` (JetStream enabled)
- `redis` (cache + pub/sub)
- `postgres`
- `qdrant`

## Start

```bash
docker compose up -d --build
```

## Endpoints

- Quarry API: `http://localhost:8090`
- Temporal UI: `http://localhost:8088`
- NATS monitor: `http://localhost:8222`
- Redis: `localhost:6379`
- Postgres: `localhost:5432`
- Qdrant: `http://localhost:6333`

## Notes

- Internal service names are available directly via Docker DNS (`redis`, `nats`, `postgres`, `qdrant`, `temporal`).
- `AI_CORE_GRPC_ADDR` defaults to `host.docker.internal:50851` in compose, so Quarry can call an externally running ai-core.
- For full in-stack ai-core later, add an `ai-core` service and set `AI_CORE_GRPC_ADDR=ai-core:50851`.
