# Data Plane v1 → v2 Migration Runbook

## Overview

Dual-run migration: v1 and v2 run simultaneously with traffic split. v2 ports are offset from v1 so both coexist on the same host.

| Component | v1 Port | v2 Port |
|-----------|---------|---------|
| Retrieval HTTP | 8004 | 8014 |
| Retrieval gRPC | — | 50052 |
| Documents API | 8000 | 8010 |
| PostgreSQL | 5432 | 5442 |
| Qdrant | 6333 | 6345 |
| Redis | 6379 | 6389 |
| NATS | — | 4232 |

## Pre-Migration Checklist

- [ ] All v2 services pass CI (`cargo check --workspace`, `go build ./...`)
- [ ] Integration tests pass against staging DB (`TEST_DATABASE_URL=... cargo test`)
- [ ] Smoke test passes (`./scripts/smoke-test.sh`)
- [ ] Load test baseline established (`k6 run tests/load/http-retrieve.js`)
- [ ] `INTERNAL_API_KEY` rotated and set in v2 environment
- [ ] `AZURE_OPENAI_API_KEY`, `COHERE_API_KEY` configured in v2
- [ ] `OTEL_EXPORTER_OTLP_ENDPOINT` configured for observability
- [ ] Grafana dashboards imported for v2 metrics
- [ ] Rollback plan reviewed with oncall

## Phase 1: Data Migration (read-only)

**Goal**: v2 Postgres has all v1 data. v2 is not serving traffic yet.

1. Stand up v2 infrastructure:
   ```bash
   cd "apps/Data Plane v2"
   make up
   ```

2. Verify infra health:
   ```bash
   ./scripts/smoke-test.sh --http-only
   ```

3. Migrate Postgres data from v1 → v2:
   ```bash
   # Export from v1
   pg_dump -h localhost -p 5432 -U dataplane \
     --data-only --table=documents --table=knowledge_units \
     dataplane > /tmp/v1-data.sql

   # Import to v2
   psql -h localhost -p 5442 -U dataplane dataplane < /tmp/v1-data.sql
   ```

4. Rebuild Qdrant vectors (v2 uses different collection schema):
   ```bash
   # Trigger full reindex via orchestrator
   curl -X POST http://localhost:8012/v1/jobs \
     -H "Content-Type: application/json" \
     -d '{"type":"reindex","org_id":"*","params":{"full":true}}'
   ```

5. Verify document counts match:
   ```sql
   -- v1
   SELECT COUNT(*) FROM documents WHERE deleted_at IS NULL;
   -- v2
   SELECT COUNT(*) FROM documents WHERE deleted_at IS NULL;
   ```

## Phase 2: Shadow Traffic (validation)

**Goal**: v2 receives copy of production traffic. Responses compared but not served to users.

1. Configure Model Plane to dual-call both v1 and v2:
   ```
   RETRIEVAL_URL_PRIMARY=http://retrieval-v1:8004
   RETRIEVAL_URL_SHADOW=http://retrieval-v2:8014
   ```

2. Run shadow comparison for 24-48h. Monitor:
   - Response parity (same top-5 candidates?)
   - Latency delta (v2 should be faster on p95)
   - Error rate (v2 errors → investigate before proceeding)

3. Run eval comparison:
   ```bash
   curl -X POST http://localhost:8013/v1/evals/compare \
     -H "Content-Type: application/json" \
     -d '{
       "org_id": "org_production",
       "strategy_a": "v1-python",
       "strategy_b": "v2-rust"
     }'
   ```

4. **Gate**: proceed only if v2 recall@10 >= v1 recall@10 and error rate < 1%.

## Phase 3: Traffic Cutover

**Goal**: Model Plane switches to v2 as primary.

1. Update Model Plane config:
   ```
   # gRPC (preferred for new integrations)
   DATAPLANE_GRPC_ADDR=retrieval-v2:50052

   # HTTP (for existing integrations)
   RETRIEVAL_URL=http://retrieval-v2:8014
   DOCUMENTS_URL=http://documents-v2:8010
   ```

2. Cutover in stages:
   - 10% traffic → v2 (monitor 1h)
   - 50% traffic → v2 (monitor 2h)
   - 100% traffic → v2 (monitor 24h)

3. Monitor during cutover:
   - `readyz` on all v2 services
   - Retrieval latency p95 < 2s
   - Error rate < 0.5%
   - Qdrant query latency
   - Redis cache hit rate

## Phase 4: v1 Decommission

**Goal**: v1 shut down cleanly.

1. Confirm no traffic to v1 for 48h (check v1 access logs)
2. Take final v1 Postgres backup
3. Stop v1 services
4. Remove v1 infrastructure after 7-day cool-down
5. Archive v1 codebase

## Rollback Plan

At any phase, rollback by reverting Model Plane config to v1 endpoints:

```
RETRIEVAL_URL=http://retrieval-v1:8004
```

v1 stays running and data-current through Phase 3. After Phase 4, rollback requires re-deploying v1 from archive and restoring Postgres backup.

**Rollback triggers** (automatic if monitoring in place):
- v2 error rate > 5% sustained for 5 min
- v2 p95 latency > 5s sustained for 5 min
- v2 readyz returns not_ready on any service for > 2 min

## Environment Variables Reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | yes | — | v2 Postgres connection string |
| `QDRANT_URL` | yes | — | v2 Qdrant HTTP endpoint |
| `REDIS_URL` | no | redis://localhost:6379 | Redis cache (degrades gracefully) |
| `NATS_URL` | yes | — | NATS JetStream for async events |
| `AZURE_OPENAI_API_KEY` | yes | — | Embedding provider |
| `AZURE_OPENAI_ENDPOINT` | yes | — | Embedding endpoint |
| `COHERE_API_KEY` | no | — | Reranker (optional) |
| `INTERNAL_API_KEY` | no | — | Cross-plane auth |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | — | OpenTelemetry collector |
| `GRPC_TIMEOUT_SECS` | no | 30 | gRPC request timeout |
| `GRPC_MAX_CONCURRENT` | no | 256 | gRPC concurrency limit |
