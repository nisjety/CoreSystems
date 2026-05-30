.PHONY: up up-ui down down-ui build logs proto clean restart dev-ingest dev-retrieve benchmark

# ── Environment ────────────────────────────────────────────────────────────────

cp-env:
	@[ -f .env ] || cp .env.example .env && echo "Created .env from .env.example"

# ── Docker Compose ─────────────────────────────────────────────────────────────

up: cp-env
	docker compose up -d
	@echo ""
	@echo "  Documents Service  → http://localhost:8001/docs"
	@echo "  Retrieval Service  → http://localhost:8004/docs"
	@echo "  Optional UI/Admin  → make up-ui"
	@echo ""

up-ui: cp-env
	docker compose -f docker-compose.yml -f docker-compose.ui.yml up -d
	@echo ""
	@echo "  Documents Service  → http://localhost:8001/docs"
	@echo "  Retrieval Service  → http://localhost:8004/docs"
	@echo "  Qdrant Dashboard   → http://localhost:6333/dashboard"
	@echo "  PostgreSQL         → localhost:5432"
	@echo "  Redis              → localhost:6379"
	@echo ""

down:
	docker compose down

down-ui:
	docker compose -f docker-compose.yml -f docker-compose.ui.yml down

build:
	docker compose build --no-cache

restart:
	docker compose restart

logs:
	docker compose logs -f --tail=50

logs-documents:
	docker compose logs -f documents-service

logs-knowledge:
	docker compose logs -f knowledge-index

logs-embedding:
	docker compose logs -f embedding-worker

logs-retrieval:
	docker compose logs -f retrieval-service

status:
	docker compose ps

# ── Proto generation ───────────────────────────────────────────────────────────
# Requires: pip install grpcio-tools

proto:
	@echo "Generating gRPC stubs..."
	@for svc in documents retrieval; do \
		mkdir -p services/$$svc/app/proto_gen; \
		touch services/$$svc/app/proto_gen/__init__.py; \
		python -m grpc_tools.protoc \
			-Iproto \
			-I$$(python -c "import grpc_tools; import os; print(os.path.dirname(grpc_tools.__file__))") \
			--python_out=services/$$svc/app/proto_gen \
			--grpc_python_out=services/$$svc/app/proto_gen \
			proto/documents.proto proto/knowledge.proto proto/retrieval.proto; \
	done
	@echo "Done."

# ── Dev helpers ────────────────────────────────────────────────────────────────

# Ingest a sample document (requires python3 + curl)
dev-ingest:
	curl -s -X POST http://localhost:8001/v1/documents \
	  -H "Content-Type: application/json" \
	  -d '{"org_id":"org_demo","source":"manual","type":"policy","title":"Vacation Policy","content":"Employees receive 25 vacation days per year. Days can be carried over to the following year up to a maximum of 5 days. Vacation requests must be submitted at least 2 weeks in advance. Public holidays are not counted as vacation days.","metadata":{"department":"HR","language":"en"}}' \
	  | python3 -m json.tool

# Run a retrieval query against the retrieval service
dev-retrieve:
	curl -s -X POST http://localhost:8004/v1/retrieve \
	  -H "Content-Type: application/json" \
	  -d '{"org_id":"org_demo","query":"How many vacation days do employees get?","filters":{}}' \
	  | python3 -m json.tool

benchmark:
	python3 data_plane_benchmark.py

# ── Cleanup ────────────────────────────────────────────────────────────────────

clean:
	docker compose -f docker-compose.yml -f docker-compose.ui.yml down -v --remove-orphans
	@echo "Volumes and containers removed."
