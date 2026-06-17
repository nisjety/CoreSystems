# Data Plane v2 — production deployment runbook

> Operator-facing reference for shipping DPv2 to Kubernetes (or a managed
> equivalent). Companion to `docs/operations.md` (local stack) and
> `docs/gap-data.md` (closure log).

## Topology

DPv2 is a single Postgres + Qdrant + Dragonfly + NATS dependency stack with
six service binaries on top. Production sizing assumes ~10 RPS sustained
peak retrieval per region; scale numbers below scale linearly past that.

| Service | Replicas | CPU / mem | Notes |
|---|---|---|---|
| `retrieval-engine` (Rust) | 3 | 1 CPU / 1 GiB | gRPC + HTTP; horizontal scaling friendly |
| `index-engine` (Rust) | 2 | 1 CPU / 1 GiB | NATS pull consumer; safe to scale by N |
| `embedding-engine` (Rust) | 2–4 | 1 CPU / 2 GiB | NATS pull consumer; scale on Model Plane embedding RPS |
| `graph-index` (Rust) | 2 | 0.5 CPU / 512 MiB | admin/observability only |
| `documents-api` (Go) | 2 | 0.5 CPU / 512 MiB | HTTP CRUD |
| `wiki-store` (Go) | 2 | 0.5 CPU / 512 MiB | HTTP CRUD + NATS pub on publish |
| `data-orchestrator` (Go) | 1 | 0.5 CPU / 512 MiB | cron + cost-ledger consumer |
| `data-quality` (Go) | 1 | 0.5 CPU / 512 MiB | eval + gates |

Dependencies:

- **Postgres 16** — at least 100 GB SSD, 4 vCPU, 8 GiB RAM. Connection
  pool sized via `PG_MAX_CONNECTIONS` (default 20 per replica).
- **Qdrant 1.17** — single-node OK up to ~5 M points; cluster past that.
  Persistent volume on SSD.
- **Dragonfly 1.37+** — 2 GiB Redis-compatible cache tier. Run in cache mode
  so memory pressure evicts cache entries instead of failing retrieval writes.
- **NATS JetStream 2.x** — 3-node cluster; file-backed streams.
- **Prometheus + Grafana** — scrape `/metrics` on each service.

## Required secrets

Use a secret manager (Vault, AWS Secrets Manager, Azure Key Vault). Never
commit values.

| Env var | Purpose | Rotation |
|---|---|---|
| `DATABASE_URL` | Postgres DSN with TLS | yearly |
| `QDRANT_URL` / `QDRANT_API_KEY` | Qdrant gRPC + API key | quarterly |
| `DRAGONFLY_URL` / `CACHE_URL` / `REDIS_URL` | Redis-compatible Dragonfly URI with TLS. `REDIS_URL` remains accepted for compatibility. | quarterly |
| `DPV2_NATS_URL` | NATS cluster URL | quarterly |
| `MODEL_PLANE_AI_CORE_GRPC_URL` | Legacy env name for current Model Plane `inference-core` gRPC endpoint | n/a |
| `MODEL_PLANE_EMBEDDING_PROVIDER` | Provider hint passed to `InferenceCore.CreateEmbedding` (`azure_openai` by default) | n/a |
| `COHERE_API_KEY` | Reranker provider | monthly |
| `INTERNAL_API_KEY` | Cross-plane shared secret | quarterly + on incident |
| `JWT_PUBLIC_KEY_PEM` | RS256 verifier (fallback) | when auth-core rotates |
| `JWT_JWKS_URL` | JWKS endpoint (§16.5.5) | n/a — dynamic |

## Network policy

- **Ingress**: only `retrieval-engine` (8004 HTTP, 50052 gRPC) and
  `documents-api` / `wiki-store` (8010 / 8011) face the cluster ingress.
- **Egress**: outbound HTTPS to `api.cohere.ai` and the JWKS endpoint.
  Embedding traffic stays east-west through current Model Plane `inference-core`; Azure
  OpenAI credentials belong to Model Plane, not DPv2. Block everything else.
- **East-west**: services join the `dpv2` namespace plus the shared
  cross-plane network. Allow `retrieval-engine` and `embedding-engine` to
  reach `inference-core:9092`; mTLS via Istio or Linkerd if available.

## Autoscaling

- HPA on CPU 70% for `retrieval-engine` (3 → 12 replicas).
- HPA on NATS lag (custom metric) for `embedding-engine` (2 → 8 replicas).
- Vertical PodAutoscaler in recommend-only mode for the Go services.

## Rollout strategy

1. **Migration first**: `make migrate-up` against the production
   Postgres. Migrations are forward-only — never roll back without a
   point-in-time recovery plan.
2. **Canary** `retrieval-engine` (1 replica) for 30 minutes. Watch
   `dpv2_retrieve_latency_seconds_bucket{quantile="0.99"}` and
   `dpv2_postgres_pool_saturation`.
3. **Full rollout** if canary metrics stay green: rolling update with
   `maxSurge=1, maxUnavailable=0`.
4. **Smoke** `make smoke-test` against the production ingress.

## Observability checks

- `dpv2_postgres_pool_saturation` ≥ 0.8 → page (§16.2.7).
- `dpv2_retrieve_latency_seconds{quantile="0.95"}` > 1.5s → warn.
- `dpv2_dlq_messages_total` rate > 1/min → page (§16.2.4 — replay via
  `cargo run --bin dlq-replay`).
- `dpv2_rate_limit_429_total` rate > 10/min/org → notify org owner.

## Incident playbook (high level)

| Symptom | First action | Reference |
|---|---|---|
| Retrieval p95 > 1s | Check `dense_ms` + `rerank_ms` in `retrieval_runs`; throttle reranker if needed | §16.3.5 |
| DLQ growing | `dlq-replay --dlq <subject> --target <subject> --dry-run` first | §16.2.4 |
| Stale cache complaints | Bump `org_versions` for the affected org | §16.2.2 |
| JWT verify failures | Confirm JWKS reachable; fall back to `JWT_PUBLIC_KEY_PEM` | §16.5.5 |
| Embedding cost spike | Check `cost_events` for the user — rate-limit if rogue agent | §15-F + §16.5.1 |

## Disaster recovery

- **Postgres**: PITR with WAL archival to object storage; RPO 5 min,
  RTO 30 min.
- **Qdrant**: snapshot every 6 hours; rebuild from `knowledge_units` if
  the snapshot is stale (reindex script in `data-orchestrator-go`).
- **Dragonfly cache**: ephemeral; service degrades gracefully.
- **NATS**: stream replicated 3x; lost messages re-emitted from
  `documents_outbox` (§16.2.6).
