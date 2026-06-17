.PHONY: up down build logs clean status dev-retrieve proto ci smoke test-rs test-integration build-rs build-go check-rs fmt-rs clippy-rs vet-go load-grpc load-http migrate-build migrate-status migrate-up backup restore tenant-check docker-build docker-up docker-up-obs docker-down docker-restart docker-test docker-logs docker-shell velion-net go-tidy

# ── Environment ────────────────────────────────────────────────────────────────

cp-env:
	@[ -f .env ] || cp .env.example .env && echo "Created .env from .env.example"

# ── Docker Compose ─────────────────────────────────────────────────────────────

# velion-net is the cross-plane shared network. For local-only testing without
# Control Plane, we still need it to exist or compose will refuse to start.
velion-net:
	@docker network inspect velion-net >/dev/null 2>&1 || \
		(echo "Creating velion-net (local stub)..." && docker network create velion-net)

up: cp-env velion-net
	docker compose up -d
	@echo ""
	@echo "  Retrieval Engine   → http://localhost:8014/health  (gRPC: localhost:50062)"
	@echo "  Documents API      → http://localhost:8010/health"
	@echo "  Wiki Store         → http://localhost:8011/health"
	@echo "  Data Orchestrator  → http://localhost:8012/health"
	@echo "  Data Quality       → http://localhost:8013/health"
	@echo "  Qdrant Dashboard   → http://localhost:6345/dashboard"
	@echo "  PostgreSQL         → localhost:5442"
	@echo "  Dragonfly cache    → localhost:6389"
	@echo "  NATS               → localhost:4232"
	@echo ""

down:
	docker compose down

build:
	docker compose build --no-cache

restart:
	docker compose restart

logs:
	docker compose logs -f --tail=50

logs-retrieval:
	docker compose logs -f retrieval-engine

status:
	docker compose ps

# ── Docker (granular targets for the v2.2 docker workflow) ────────────────────

# Build all 8 service images. Reuses cargo-chef layer cache; first run is slow,
# rebuilds on source changes are fast.
docker-build: velion-net
	docker compose build

# Build with no cache — use after dep changes (Cargo.toml, go.mod) or to force a fresh build.
docker-build-clean: velion-net
	docker compose build --no-cache

# Start all services (without observability stack). Same as `make up`.
docker-up: up

# Start with Prometheus + Grafana (profile "obs"). Adds ~512 MiB memory.
#   Prometheus → http://localhost:9090
#   Grafana    → http://localhost:3001  (admin / GRAFANA_ADMIN_PASSWORD)
docker-up-obs: cp-env velion-net
	docker compose --profile obs up -d
	@echo ""
	@echo "  Prometheus         → http://localhost:9090"
	@echo "  Grafana            → http://localhost:3001  (admin / \$$GRAFANA_ADMIN_PASSWORD)"
	@echo ""

docker-down: down

docker-restart: down up

# Build → start → wait for ready → smoke test.
# Returns non-zero if any check fails. Use this in CI / pre-deploy verification.
docker-test: docker-build docker-up
	@echo ""
	@echo "Waiting for services to become healthy (max 90s)..."
	@for i in $$(seq 1 18); do \
		ready=1; \
		for url in http://localhost:8014/readyz http://localhost:8010/readyz \
		           http://localhost:8011/readyz http://localhost:8012/readyz \
		           http://localhost:8013/readyz; do \
			curl -sf $$url >/dev/null 2>&1 || ready=0; \
		done; \
		if [ $$ready = 1 ]; then echo "All services ready in $$((i*5))s"; break; fi; \
		sleep 5; \
	done
	@echo ""
	./scripts/smoke-test.sh --http-only

docker-logs:
	docker compose logs -f --tail=100

# Open a shell in a running service container. Usage: make docker-shell SVC=retrieval-engine
docker-shell:
	@if [ -z "$(SVC)" ]; then \
		echo "Usage: make docker-shell SVC=<service-name>"; \
		echo "Services: retrieval-engine index-engine embedding-engine graph-index documents-api wiki-store data-orchestrator data-quality"; \
		exit 1; \
	fi
	docker compose exec $(SVC) sh

# ── Rust builds (local) ───────────────────────────────────────────────────────

build-rs:
	cargo build --release

check-rs:
	cargo check --workspace

test-rs:
	cargo test --workspace

fmt-rs:
	cargo fmt --all

clippy-rs:
	cargo clippy --workspace -- -D warnings

# ── Go builds ─────────────────────────────────────────────────────────────────

build-go:
	@for svc in documents-api-go wiki-store-go data-orchestrator-go data-quality-go; do \
		echo "=== $$svc ===" && (cd services/$$svc && go build ./...) && echo "OK"; \
	done

vet-go:
	@for svc in documents-api-go wiki-store-go data-orchestrator-go data-quality-go; do \
		echo "=== $$svc ===" && (cd services/$$svc && go vet ./...) && echo "OK"; \
	done

# ── Testing ───────────────────────────────────────────────────────────────────

test-integration:
	@echo "Requires TEST_DATABASE_URL env var"
	cargo test -p retrieval-engine-rs --test grpc_integration -- --nocapture

smoke:
	./scripts/smoke-test.sh

load-grpc:
	./tests/load/grpc-retrieve.sh

load-http:
	k6 run tests/load/http-retrieve.js

# ── Proto generation ──────────────────────────────────────────────────────────

proto:
	./scripts/gen-clients.sh

# ── DB migrations ─────────────────────────────────────────────────────────────

migrate-build:
	cd tools/migrator && go build -o ../../bin/migrator ./...

migrate-status: migrate-build
	@DATABASE_URL=$${DATABASE_URL:-postgres://dataplane:dataplane@localhost:5442/dataplane} \
		./bin/migrator status

migrate-up: migrate-build
	@DATABASE_URL=$${DATABASE_URL:-postgres://dataplane:dataplane@localhost:5442/dataplane} \
		./bin/migrator up

# ── Backup & restore ──────────────────────────────────────────────────────────

backup:
	./scripts/backup.sh

restore:
	@echo "Usage: ./scripts/restore.sh <backup-dir>"

# ── CI (local) ────────────────────────────────────────────────────────────────

tenant-check:
	./scripts/check-tenant-isolation.sh

ci: fmt-rs clippy-rs check-rs build-go vet-go tenant-check
	@echo "CI checks passed"

# ── Dev helpers ────────────────────────────────────────────────────────────────

dev-retrieve:
	curl -s -X POST http://localhost:8014/v1/retrieve \
	  -H "Content-Type: application/json" \
	  -d '{"org_id":"org_demo","query":"How many vacation days do employees get?","filters":{}}' \
	  | python3 -m json.tool

dev-trace:
	@echo "Usage: make dev-trace TRACE_ID=<id>"
	curl -s http://localhost:8014/v1/retrieval/$(TRACE_ID) | python3 -m json.tool

# ── Go module hygiene ──────────────────────────────────────────────────────────
#
# Run after touching any go.mod or after adding a new import. Walks every
# Go service module so a single command keeps all four in sync.
go-tidy:
	@for svc in services/documents-api-go services/wiki-store-go \
		services/data-orchestrator-go services/data-quality-go; do \
		echo "tidy $$svc"; \
		(cd $$svc && go mod tidy) || exit 1; \
	done
	@echo "All Go modules tidied."

# ── Cleanup ────────────────────────────────────────────────────────────────────

clean:
	docker compose down -v --remove-orphans
	@echo "Volumes and containers removed."
