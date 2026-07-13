COMPOSE := docker compose --env-file ../../.env --env-file .env -f docker-compose.yml

.PHONY: help
help:
	@echo "Ingestion Plane - Make Commands"
	@echo "==============================="
	@echo ""
	@echo "Setup & Infrastructure:"
	@echo "  make setup              Install all dependencies"
	@echo "  make up                 Start all services (Docker Compose)"
	@echo "  make down               Stop all services"
	@echo "  make clean              Remove all containers and volumes"
	@echo "  make build              Build all Docker images"
	@echo ""
	@echo "Development:"
	@echo "  make dev-imports        Start imports-core in dev mode"
	@echo "  make dev-quarry         Start Quarry-v2 edge in dev mode"
	@echo "  make logs               Tail all service logs"
	@echo "  make shell-imports      Open shell in imports-api container"
	@echo "  make shell-postgres     Open PostgreSQL shell"
	@echo ""
	@echo "Testing:"
	@echo "  make test-imports       Run imports-core tests"
	@echo "  make test-quarry        Run Quarry-v2 workspace tests"
	@echo "  make test-endpoints     Test all endpoints"
	@echo "  make test-load          Run load test"
	@echo "  make smoke-integration  Run integration-api smoke tests (12 checks)"
	@echo ""
	@echo "Database:"
	@echo "  make migrate            Run database migrations"
	@echo "  make seed               Seed test data"
	@echo "  make db-reset           Reset database"
	@echo ""
	@echo "Monitoring:"
	@echo "  make metrics            Show Prometheus metrics"
	@echo "  make stats              Show Docker container stats"
	@echo "  make temporal-health    Check the Temporal cluster"
	@echo ""
	@echo "Deployment:"
	@echo "  make push-images        Push Docker images to registry"
	@echo "  make deploy-k8s         Deploy to Kubernetes"
	@echo ""

.PHONY: setup
setup:
	@echo "Setting up Ingestion Plane..."
	@echo "1. Creating virtual environment for imports-core..."
	cd imports-core && python -m venv venv
	@echo "2. Installing Python dependencies..."
	cd imports-core && . venv/bin/activate && pip install -r requirements.txt
	@echo "3. Fetching Quarry-v2 Rust dependencies..."
	cd Quarry-v2 && cargo fetch
	@echo "✓ Setup complete!"
	@echo ""
	@echo "Next steps:"
	@echo "  1. Copy .env.example to .env and configure"
	@echo "  2. Run 'make up' to start services"
	@echo "  3. Run 'make test-endpoints' to verify"

.PHONY: up
up:
	@echo "Starting Ingestion Plane services..."
	$(COMPOSE) up -d --build --wait --wait-timeout 300
	@echo ""
	@echo "Services started:"
	@echo "  Quarry Edge:    http://localhost:8082"
	@echo "  Quarry Control: http://localhost:8081"
	@echo "  Imports API:    http://localhost:3025"
	@echo "  Integration API:http://localhost:3026"
	@echo "  Finspo API:     http://localhost:3130"
	@echo "  Shipping Core:  http://localhost:3156"
	@echo "  PostgreSQL:     localhost:5432"
	@echo "  NATS:           localhost:4224"
	@echo "  Temporal:       localhost:7233"
	@echo ""
	@echo "Run 'make test-endpoints' to verify all services are working"

.PHONY: down
down:
	@echo "Stopping Ingestion Plane services..."
	$(COMPOSE) down

.PHONY: clean
clean:
	@echo "Removing all containers and volumes..."
	$(COMPOSE) down -v
	@echo "✓ Cleanup complete"

.PHONY: build
build:
	@echo "Building Docker images..."
	$(COMPOSE) build
	@echo "✓ Build complete"

.PHONY: dev-imports
dev-imports:
	@echo "Starting imports-core in development mode..."
	cd imports-core && . venv/bin/activate && uvicorn app.main:app --reload --port 3025

.PHONY: dev-quarry
dev-quarry:
	@echo "Starting Quarry-v2 edge in development mode..."
	cd Quarry-v2 && cargo run -p quarry-edge

.PHONY: logs
logs:
	$(COMPOSE) logs -f

.PHONY: shell-imports
shell-imports:
	$(COMPOSE) exec imports-api /bin/sh

.PHONY: shell-postgres
shell-postgres:
	$(COMPOSE) exec postgres sh -ec 'exec psql -U "$$POSTGRES_USER" -d "$$POSTGRES_DB"'

.PHONY: test-imports
test-imports:
	cd imports-core && . venv/bin/activate && pytest -v tests/

.PHONY: test-quarry
test-quarry:
	cd Quarry-v2 && cargo test --workspace

.PHONY: test-endpoints
test-endpoints:
	@set -eu; \
	compose() { $(COMPOSE) "$$@"; }; \
	check_http() { \
		name="$$1"; url="$$2"; \
		curl --fail --silent --show-error --max-time 10 --output /dev/null "$$url"; \
		printf '✓ %s\n' "$$name"; \
	}; \
	printf 'Testing Ingestion Plane readiness...\n'; \
	check_http 'Quarry Edge' 'http://127.0.0.1:8082/health'; \
	check_http 'Imports API' 'http://127.0.0.1:3025/ready'; \
	check_http 'Integration API' 'http://127.0.0.1:3026/health'; \
	check_http 'Finspo API' 'http://127.0.0.1:3130/health'; \
	check_http 'Shipping Core' 'http://127.0.0.1:3156/healthz'; \
	compose exec -T postgres pg_isready -U "$${INGESTION_PG_USER:-ingestion_user}" -d "$${INGESTION_PG_DB:-ingestion_plane_db}" >/dev/null; \
	printf '✓ PostgreSQL\n'; \
	compose exec -T dragonfly sh -ec 'redis-cli -a "$$INGESTION_DRAGONFLY_PASSWORD" ping >/dev/null 2>&1'; \
	printf '✓ Dragonfly\n'; \
	compose exec -T temporal temporal operator cluster health --address temporal:7233 >/dev/null; \
	printf '✓ Temporal\n'; \
	printf '✓ All Ingestion Plane endpoints are operational\n'

.PHONY: test-load
test-load:
	@echo "Running load test on Imports API..."
	@which ab > /dev/null || (echo "Apache Bench not found. Install with: brew install httpd" && exit 1)
	ab -n 1000 -c 10 http://localhost:3025/health
	@echo ""
	@echo "Load test complete. Check results above for latency and throughput."

.PHONY: migrate
migrate:
	@echo "Running database migrations..."
	$(COMPOSE) exec -T postgres sh -ec 'exec psql -v ON_ERROR_STOP=1 -U "$$POSTGRES_USER" -d imports' < imports-core/migrations/001_init.sql
	@echo "✓ Migrations complete"

.PHONY: seed
seed:
	@echo "Seeding test data..."
	@printf '%s\n' \
		"INSERT INTO import_jobs (id, org_id, user_id, source_type, status, total_items)" \
		"VALUES ('00000000-0000-0000-0000-000000000001', 'test-org', 'test-user', 'upload', 'completed', 10)" \
		"ON CONFLICT (id) DO NOTHING;" | \
		$(COMPOSE) exec -T postgres sh -ec 'exec psql -v ON_ERROR_STOP=1 -U "$$POSTGRES_USER" -d imports'
	@echo "✓ Test data seeded"

.PHONY: db-reset
db-reset: clean up migrate seed
	@echo "✓ Database reset complete"

.PHONY: metrics
metrics:
	@echo "Quarry Metrics:"
	@test -n "$(TOKEN)" || (echo "Error: TOKEN is required for the protected metrics endpoint" >&2; exit 1)
	@curl --fail --silent --show-error --max-time 10 -H "Authorization: Bearer $(TOKEN)" http://127.0.0.1:8082/metrics | head -20
	@echo ""

.PHONY: stats
stats:
	@ids="$$($(COMPOSE) ps -q)"; test -n "$$ids" || (echo "No Ingestion Plane containers are running" >&2; exit 1); docker stats --no-stream $$ids

.PHONY: temporal-health
temporal-health:
	@$(COMPOSE) exec -T temporal temporal operator cluster health --address temporal:7233

.PHONY: push-images
push-images:
	@echo "Pushing Docker images to registry..."
	@test -n "$(REGISTRY)" || (echo "Error: REGISTRY not set. Usage: make push-images REGISTRY=registry.example.com" && exit 1)
	docker tag ingestion-plane-quarry-edge:latest $(REGISTRY)/ingestion/quarry-edge:latest
	docker tag ingestion-plane-imports-api:latest $(REGISTRY)/ingestion/imports-api:latest
	docker push $(REGISTRY)/ingestion/quarry-edge:latest
	docker push $(REGISTRY)/ingestion/imports-api:latest
	@echo "✓ Images pushed to $(REGISTRY)"

.PHONY: deploy-k8s
deploy-k8s:
	@echo "Deploying to Kubernetes..."
	@test -f kubernetes/namespace.yaml || (echo "Error: kubernetes/ directory not found" && exit 1)
	kubectl apply -f kubernetes/
	@echo "✓ Deployment complete"
	@echo "Check status with: kubectl get all -n ingestion-plane"

.PHONY: docs
docs:
	@echo "Ingestion Plane Documentation:"
	@echo "  - Architecture:    INGESTION_PLANE_ARCHITECTURE.md"
	@echo "  - Quick Start:     QUICKSTART.md"
	@echo "  - Quarry-v2:       Quarry-v2/docs/ARCHITECTURE.md"
	@echo "  - Onboarding crawl: Quarry-v2/docs/ONBOARDING_LIVE_CRAWL.md"
	@echo "  - Imports README:  imports-core/README.md"
	@echo ""
	@echo "Open docs with: open INGESTION_PLANE_ARCHITECTURE.md"

.PHONY: version
version:
	@echo "Ingestion Plane Version Information:"
	@echo "  Quarry-v2:      Rust workspace (active)"
	@echo "  Imports-Core:   v1.0.0 (Development)"
	@echo "  Last Updated:   July 12, 2026"

# CI/CD targets
.PHONY: ci-test
ci-test: test-imports test-quarry test-endpoints
	@echo "✓ All CI tests passed"

.PHONY: smoke-integration
smoke-integration:
	@echo "Running integration-api smoke tests..."
	@bash smoke-test-integration-api.sh

.PHONY: ci-build
ci-build:
	$(COMPOSE) build --no-cache

.PHONY: ci-push
ci-push: ci-build push-images
	@echo "✓ CI pipeline complete"

# Default target
.DEFAULT_GOAL := help
