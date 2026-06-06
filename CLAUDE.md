# Project Instructions

## Scope
CoreSystem is a multi-plane monorepo. For cross-plane orientation, start with `apps/CODEBASE_INFORMATION_SYSTEM.md`, then `apps/master-ownership-matrix.md`, then plane-specific docs.

Focused planes:
- `apps/Application Plane`
- `apps/Data Plane v2`
- `apps/Channel Plane`
- `apps/Control Plane`
- `apps/Model Plane`
- `apps/Ingestion Plane`
- `apps/Frontend Plane/velionv2`

## Tech Stack
- Rust: latency-sensitive runtime, retrieval, browser, protocol, and hot-path services.
- Go: durable workflow, CRUD, registry, policy, orchestration, billing, and control services.
- TypeScript/TSX: Velion v2 frontend/BFF, NestJS auth-core, Convex functions, support workers.
- Python: imports, labs, evals, provider glue, and generated SDK tests.
- Infra: Docker Compose stacks, Postgres, Redis, NATS/JetStream, Qdrant, MinIO, Temporal, Quickwit, Convex.

## Architecture Rules
- Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements.
- Data Plane v2 owns documents, chunks, embeddings, retrieval, graph, wiki, and source traces.
- Ingestion Plane captures evidence and persists durable knowledge through Data Plane contracts only.
- Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants, and cost.
- Application Plane owns collaborative/realtime workspace projections and notifications.
- Frontend Plane owns Velion v2 UI plus BFF route normalization.
- Channel Plane is future/docs-only today; do not build against it as if runtime exists.
- No direct database crossing between planes.
- No independent embeddings/reranking outside isolated labs.
- Model may propose browser actions; Quarry-v2 executes or rejects them.
- Zero Data Retention must propagate through any content-persisting boundary.

## Code Style
- Prefer existing plane patterns over new abstractions.
- Keep authority boundaries explicit in names, clients, and tests.
- Go services usually enter through `cmd/*/main.go` and keep HTTP/gRPC under `internal/http` and `internal/grpc`.
- Rust services usually enter through `src/main.rs` and keep cross-plane contracts in crate/service-specific modules.
- Velion v2 uses feature-sliced folders under `src/features/*-v2`.
- Velion v2 uses Server Components by default and Client Components at interaction leaves.
- Velion v2 BFF routes live under `src/app/api/**/route.ts` and must not leak upstream secrets or raw OAuth tokens.
- API responses use typed envelopes: `{ data }`, cursor `meta`/`links`, or `{ error: { code, message, details } }`.

## Testing
- Go tests: `*_test.go`, usually `go test ./...`.
- Rust tests: `cargo test --workspace` or per-crate `tests/*.rs`.
- TypeScript tests: `*.test.ts` / `*.test.tsx` with Vitest or Jest, depending on package.
- Playwright tests: `tests/e2e/*.spec.ts`.
- Python tests: `test_*.py`.
- Coverage commands detected: `apps/Control Plane/auth-core` has `test:cov`; `apps/Control Plane/user-core` has `make test-coverage`.

## Build And Run
Frontend:
```bash
cd "apps/Frontend Plane/velionv2"
pnpm dev
pnpm lint
pnpm typecheck
pnpm test
pnpm test:e2e
pnpm build
```

Data Plane v2:
```bash
cd "apps/Data Plane v2"
make up
make check-rs
make test-rs
make build-go
make test-integration
```

Ingestion Plane:
```bash
cd "apps/Ingestion Plane"
make up
make test-endpoints
```

Model Plane:
```bash
cd "apps/Model Plane"
./scripts/compose.sh up -d
cd rust && cargo test --workspace
```

Control/Application stacks:
```bash
cd "apps/Control Plane" && docker compose up -d --build
cd "apps/Application Plane" && docker compose up -d --build
```

## Git Conventions
- Recent commits use Conventional Commit style: `feat:`, `fix(scope):`, `docs:`.
- The worktree may contain large pre-existing local changes. Never reset, checkout, clean, or revert user changes unless explicitly requested.
