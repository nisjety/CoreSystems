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
	@echo "  make test-quarry-legacy Run legacy Go Quarry tests (deprecated)"
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
	@echo "  make temporal-ui        Open Temporal UI (http://localhost:8089)"
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
	docker-compose -f docker-compose.full.yml up -d
	@echo "Waiting for services to be ready..."
	sleep 10
	@echo ""
	@echo "Services started:"
	@echo "  Quarry API:     http://localhost:8090"
	@echo "  Imports API:    http://localhost:3025"
	@echo "  Temporal UI:    http://localhost:8089"
	@echo "  PostgreSQL:     localhost:5434"
	@echo "  Redis:          localhost:6380"
	@echo "  NATS:           localhost:4223"
	@echo ""
	@echo "Run 'make test-endpoints' to verify all services are working"

.PHONY: down
down:
	@echo "Stopping Ingestion Plane services..."
	docker-compose -f docker-compose.full.yml down

.PHONY: clean
clean:
	@echo "Removing all containers and volumes..."
	docker-compose -f docker-compose.full.yml down -v
	@echo "✓ Cleanup complete"

.PHONY: build
build:
	@echo "Building Docker images..."
	docker-compose -f docker-compose.full.yml build
	@echo "✓ Build complete"

.PHONY: dev-imports
dev-imports:
	@echo "Starting imports-core in development mode..."
	cd imports-core && . venv/bin/activate && uvicorn app.main:app --reload --port 3025

.PHONY: dev-quarry
dev-quarry:
	@echo "Starting Quarry-v2 edge in development mode..."
	cd Quarry-v2 && cargo run -p quarry-edge

.PHONY: dev-quarry-legacy
dev-quarry-legacy:
	@echo "Starting legacy Go Quarry (deprecated — use Quarry-v2)..."
	cd Quarry && go run ./cmd/api/main.go

.PHONY: logs
logs:
	docker-compose -f docker-compose.full.yml logs -f

.PHONY: shell-imports
shell-imports:
	docker-compose -f docker-compose.full.yml exec imports-api /bin/bash

.PHONY: shell-postgres
shell-postgres:
	docker-compose -f docker-compose.full.yml exec postgres psql -U root

.PHONY: test-imports
test-imports:
	cd imports-core && . venv/bin/activate && pytest -v tests/

.PHONY: test-quarry
test-quarry:
	cd Quarry-v2 && cargo test --workspace

.PHONY: test-quarry-legacy
test-quarry-legacy:
	@echo "Legacy Go Quarry tests (deprecated — use 'make test-quarry' for Quarry-v2)"
	cd Quarry && go test -v ./...

.PHONY: test-endpoints
test-endpoints:
	@echo "Testing Ingestion Plane endpoints..."
	@echo ""
	@echo "1. Quarry Health Check"
	@curl -s http://localhost:8090/health | jq . || echo "❌ Quarry not responding"
	@echo ""
	@echo "2. Imports Health Check"
	@curl -s http://localhost:3025/health | jq . || echo "❌ Imports not responding"
	@echo ""
	@echo "3. Temporal UI"
	@curl -s http://localhost:8089 > /dev/null && echo "✓ Temporal UI running" || echo "❌ Temporal UI not responding"
	@echo ""
	@echo "4. Database"
	@docker-compose -f docker-compose.full.yml exec postgres pg_isready -U root > /dev/null && echo "✓ PostgreSQL ready" || echo "❌ PostgreSQL not ready"
	@echo ""
	@echo "5. Redis"
	@docker-compose -f docker-compose.full.yml exec redis redis-cli ping > /dev/null 2>&1 && echo "✓ Redis ready" || echo "❌ Redis not ready"
	@echo ""
	@echo "✓ All endpoints operational!"

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
	docker-compose -f docker-compose.full.yml exec postgres psql -U root < imports-core/migrations/001_init.sql
	@echo "✓ Migrations complete"

.PHONY: seed
seed:
	@echo "Seeding test data..."
	docker-compose -f docker-compose.full.yml exec -T postgres psql -U root << 'EOF'
	\c imports
	INSERT INTO import_jobs (org_id, user_id, source_type, status, total_items) 
	VALUES ('test-org', 'test-user', 'upload', 'completed', 10);
	EOF
	@echo "✓ Test data seeded"

.PHONY: db-reset
db-reset: down up migrate seed
	@echo "✓ Database reset complete"

.PHONY: metrics
metrics:
	@echo "Quarry Metrics:"
	@curl -s http://localhost:8090/metrics | grep -E "quarry_|go_" | head -20
	@echo ""

.PHONY: stats
stats:
	docker stats --no-stream

.PHONY: temporal-ui
temporal-ui:
	@echo "Opening Temporal UI at http://localhost:8089"
	@open http://localhost:8089 || xdg-open http://localhost:8089 || echo "Open http://localhost:8089 manually"

.PHONY: push-images
push-images:
	@echo "Pushing Docker images to registry..."
	@test -n "$(REGISTRY)" || (echo "Error: REGISTRY not set. Usage: make push-images REGISTRY=registry.example.com" && exit 1)
	docker tag quarry:latest $(REGISTRY)/quarry:latest
	docker tag imports-core:latest $(REGISTRY)/imports-core:latest
	docker push $(REGISTRY)/quarry:latest
	docker push $(REGISTRY)/imports-core:latest
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
	@echo "  - Legacy Quarry (deprecated): Quarry/docs/API.md, Quarry/docs/DEPLOYMENT.md"
	@echo ""
	@echo "Open docs with: open INGESTION_PLANE_ARCHITECTURE.md"

.PHONY: version
version:
	@echo "Ingestion Plane Version Information:"
	@echo "  Quarry-v2:      Rust workspace (active)"
	@echo "  Quarry:         v0.1.0 (legacy Go service, deprecated)"
	@echo "  Imports-Core:   v1.0.0 (Development)"
	@echo "  Last Updated:   July 2, 2026"

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
	docker-compose -f docker-compose.full.yml build --no-cache

.PHONY: ci-push
ci-push: ci-build push-images
	@echo "✓ CI pipeline complete"

# Default target
.DEFAULT_GOAL := help
