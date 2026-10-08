.PHONY: eval-retrieval eval-retrieval-lexical eval-gate eval-check eval-golden eval-evidence eval-variant eval-corpus-verify eval-harness-test up standalone-up cross-plane-env cross-plane-up down build logs clean status dev-retrieve proto ci smoke smoke-health auth-matrix test-security-scripts test-rs test-integration build-rs build-go check-rs fmt-rs clippy-rs vet-go load-grpc load-http migrate-build migrate-status migrate-up backup restore tenant-static-check tenant-check docker-build docker-up docker-up-obs docker-down docker-restart docker-test docker-logs docker-shell verevon-net go-tidy

SOURCE_REVISION ?= $(shell git rev-parse --verify HEAD 2>/dev/null || echo unverified)
BUILD_DATE ?= $(shell date -u +%Y-%m-%dT%H:%M:%SZ)
COMPOSE_BUILD_ENV = SOURCE_REVISION="$(SOURCE_REVISION)" BUILD_DATE="$(BUILD_DATE)"
INTER_PLANE_BUS_NETWORK ?= inter-plane-bus
export INTER_PLANE_BUS_NETWORK

# Keep host-side compiler caches off the internal disk and source volume.
CORE_SYSTEM_CACHE_ROOT ?= /Volumes/Applikasjon/Triodelab/CoreSystem-build-cache
CARGO_TARGET_DIR ?= $(CORE_SYSTEM_CACHE_ROOT)/cargo/data-plane-v2
GOCACHE ?= $(CORE_SYSTEM_CACHE_ROOT)/go/build
GOMODCACHE ?= $(CORE_SYSTEM_CACHE_ROOT)/go/mod
export CARGO_TARGET_DIR GOCACHE GOMODCACHE

# ── Environment ────────────────────────────────────────────────────────────────

cp-env:
	@if [ -f .env ]; then :; else cp .env.example .env; echo "Created .env from .env.example"; fi

# ── Docker Compose ─────────────────────────────────────────────────────────────

# inter-plane-bus is the cross-plane shared network. The historical target name
# remains for command compatibility and is used only by the explicit connected
# target; the default Compose file creates a private local network itself.
verevon-net:
	@docker network inspect "$(INTER_PLANE_BUS_NETWORK)" >/dev/null 2>&1 || \
		(echo "Creating $(INTER_PLANE_BUS_NETWORK) (local stub)..." && docker network create "$(INTER_PLANE_BUS_NETWORK)")

up: cp-env
	$(COMPOSE_BUILD_ENV) docker compose up -d
	@echo ""
	@echo "  Data Plane services are private to dpv2-net/dpv2-cross-plane."
	@echo "  Use 'docker compose exec <service> curl http://127.0.0.1:<port>/readyz' for health checks."
	@echo ""

standalone-up: cp-env
	$(COMPOSE_BUILD_ENV) docker compose -f docker-compose.yml -f docker-compose.standalone.yml up -d
	@echo "  Standalone posture: cross-plane calls remain fail-closed; Control GDPR/event consumers are paused."

cross-plane-env: cp-env
	@set -eu; \
	env_dump="$$(mktemp)"; \
	trap 'rm -f "$$env_dump"' EXIT; \
	$(COMPOSE_BUILD_ENV) docker compose config --environment >"$$env_dump"; \
	for name in NATS_SHARED_URL DOCUMENTS_GDPR_NATS_PASSWORD MODEL_PLANE_INFERENCE_SERVICE_API_KEY MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY USER_CORE_DOCUMENTS_TOKEN USER_CORE_RETRIEVAL_TOKEN CONTROL_POLICY_SERVICE_API_KEY DATA_ORCHESTRATOR_COST_SERVICE_API_KEY AUTH_CORE_PUBLIC_KEY_PATH DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH DOCUMENTS_EVENT_VERIFYING_PUBLIC_KEY_PATH INDEX_EVENT_SIGNING_PRIVATE_KEY_PATH INDEX_EVENT_VERIFYING_PUBLIC_KEY_PATH EMBEDDING_EVENT_SIGNING_PRIVATE_KEY_PATH EMBEDDING_EVENT_VERIFYING_PUBLIC_KEY_PATH WIKI_EVENT_SIGNING_PRIVATE_KEY_PATH WIKI_EVENT_VERIFYING_PUBLIC_KEY_PATH RETRIEVAL_EVENT_SIGNING_PRIVATE_KEY_PATH RETRIEVAL_EVENT_VERIFYING_PUBLIC_KEY_PATH GRAPH_EVENT_SIGNING_PRIVATE_KEY_PATH GRAPH_EVENT_VERIFYING_PUBLIC_KEY_PATH; do \
		if ! awk -F= -v name="$$name" '$$1 == name && length(substr($$0, index($$0, "=") + 1)) > 0 { found=1 } END { exit !found }' "$$env_dump"; then \
			echo "$$name must be provisioned before cross-plane-up" >&2; exit 1; \
		fi; \
	done; \
	for name in AUTH_CORE_PUBLIC_KEY_PATH DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH DOCUMENTS_EVENT_VERIFYING_PUBLIC_KEY_PATH INDEX_EVENT_SIGNING_PRIVATE_KEY_PATH INDEX_EVENT_VERIFYING_PUBLIC_KEY_PATH EMBEDDING_EVENT_SIGNING_PRIVATE_KEY_PATH EMBEDDING_EVENT_VERIFYING_PUBLIC_KEY_PATH WIKI_EVENT_SIGNING_PRIVATE_KEY_PATH WIKI_EVENT_VERIFYING_PUBLIC_KEY_PATH RETRIEVAL_EVENT_SIGNING_PRIVATE_KEY_PATH RETRIEVAL_EVENT_VERIFYING_PUBLIC_KEY_PATH GRAPH_EVENT_SIGNING_PRIVATE_KEY_PATH GRAPH_EVENT_VERIFYING_PUBLIC_KEY_PATH; do \
		path="$$(awk -F= -v name="$$name" '$$1 == name { print substr($$0, index($$0, "=") + 1); exit }' "$$env_dump")"; \
		case "$$path" in .secrets/event-keys/*|*/.secrets/event-keys/*) echo "$$name still points at local sandbox key material" >&2; exit 1 ;; esac; \
		case "$$path" in /*) ;; *) path="$(CURDIR)/$$path" ;; esac; \
		test -r "$$path" || { echo "$$name points to an unreadable file" >&2; exit 1; }; \
	done; \
	for name in MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE MODEL_PLANE_INFERENCE_RETENTION_POSTURE; do \
		value="$$(awk -F= -v name="$$name" '$$1 == name { print substr($$0, index($$0, "=") + 1); exit }' "$$env_dump")"; \
		case "$$value" in ""|persistent|zdr|tenant) ;; *) echo "$$name must be persistent|zdr|tenant (it must match the principal's retentionByAudience for inference-core, or be tenant), got '$$value'" >&2; exit 1 ;; esac; \
	done

cross-plane-up: cp-env cross-plane-env
	@docker network inspect "$(INTER_PLANE_BUS_NETWORK)" >/dev/null 2>&1 || \
		(echo "$(INTER_PLANE_BUS_NETWORK) must be provisioned by Control Plane before cross-plane-up" >&2; exit 1)
	$(COMPOSE_BUILD_ENV) docker compose -f docker-compose.yml -f docker-compose.cross-plane.yml up -d
	@echo "  Connected posture: using the pre-provisioned $(INTER_PLANE_BUS_NETWORK) network."

down:
	docker compose down

build:
	$(COMPOSE_BUILD_ENV) docker compose build --no-cache

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
docker-build:
	$(COMPOSE_BUILD_ENV) docker compose build

# Build with no cache — use after dep changes (Cargo.toml, go.mod) or to force a fresh build.
docker-build-clean:
	$(COMPOSE_BUILD_ENV) docker compose build --no-cache

# Start all services (without observability stack). Same as `make up`.
docker-up: up

# Start with Prometheus + Grafana (profile "obs"). Adds ~512 MiB memory.
#   Prometheus → http://localhost:9090
#   Grafana    → http://localhost:3001  (admin / GRAFANA_ADMIN_PASSWORD)
docker-up-obs: cp-env
	@grep -Eq '^GRAFANA_ADMIN_PASSWORD=.+$$' .env || \
		(echo "GRAFANA_ADMIN_PASSWORD is required for the obs profile" >&2; exit 1)
	$(COMPOSE_BUILD_ENV) docker compose --profile obs up -d
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
		if ./scripts/smoke-test.sh --health-only >/dev/null 2>&1; then \
			echo "All services ready in $$((i*5))s"; ready=1; break; \
		fi; \
		ready=0; \
		sleep 5; \
	done; \
	[ $$ready = 1 ] || (echo "Services did not become ready" >&2; exit 1)
	@echo ""
	./scripts/smoke-test.sh --with-auth-matrix

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

smoke-health:
	./scripts/smoke-test.sh --health-only

auth-matrix:
	./scripts/auth-regression-matrix.sh

smoke:
	./scripts/smoke-test.sh --with-auth-matrix

# ── Retrieval quality eval ─────────────────────────────────────────────────────
# Runs the judged golden set (87 hand-authored queries, scripts/eval-*) against
# the two eval-fixture orgs and scores recall@10 / nDCG@10 / MRR with the
# corrected goldenMetrics. Run BEFORE and AFTER any change to fusion weights,
# rerank config, chunking, or embedding models — the 2026-08-26 sweeps are the
# baseline evidence (see docker-compose.yml comments at W_BM25 / RERANK_TOP_K).
# Requires: the stack up with the cross-plane overlay, the two organizations
# provisioned (docs/contextual-retrieval-runbook.md, section 5.12), and the FROZEN
# corpus ingested in org-corpus-baseline / org-corpus-contextual
# (scripts/eval-ingest-corpus.py: a paid act that needs the owner's approval, OD8).
# EVAL_SCRATCH receives the golden sets (built for the organizations as they are,
# by the eval-golden target below: the id-keyed files are no longer committed
# state) and must contain seed-cred.txt or have DATA_PLANE_CORPUS_SEEDER_API_KEY
# exported (the corpus-seeder service credential — a secret, deliberately never in
# the repo). Defaults to scripts/ so a colocated setup needs no arguments.
eval-retrieval: eval-golden
	./scripts/eval-attribution-study.sh "$${EVAL_SCRATCH:-./scripts}"
	python3 ./scripts/eval-score-attribution.py "$${EVAL_SCRATCH:-./scripts}"

# The golden sets (natural language, Norwegian morphology, mixed language), keyed by
# document title and resolved to the ids the two organizations hold today through the
# corpus key the ingest stored (CR-T14). Run it again after every re-seed.
eval-golden:
	python3 ./scripts/eval-build-golden-set.py --out "$${EVAL_SCRATCH:-./scripts}"

# The MINED LEXICAL class — exact identifiers, config keys, file paths — as its
# own gated cell. Separate from the target above on purpose: the 87-query set is
# all natural-language questions, the class where the dense arm dominates, and
# averaging the two classes together hides a lexical-only regression inside the
# blended metric's noise floor. `w_bm25` is worth +0.37 nDCG on this class and
# costs nDCG on the other; one number cannot gate both.
#
# Judgments are correct by construction (tokens occurring in exactly one corpus
# document), so this needs no hand-judging — but the ids are specific to this
# deployment's ingest, which is why the mined sets are regenerated rather than
# committed. Run the miner first:
#   python3 scripts/eval-mine-lexical-queries.py "$${EVAL_SCRATCH:-./scripts}"
eval-retrieval-lexical:
	./scripts/eval-lexical-cell.sh "$${EVAL_SCRATCH:-./scripts}"

# CI gate: fails (exit 1) when a metric regresses past the measured noise floor,
# or when the eval itself did not really run (too few queries scored, or queries
# returning nothing — the shape every silent-stage failure in this service has
# taken). Tolerances and their derivation live in scripts/eval-gate.py; the
# committed baseline is scripts/eval-baseline.json, so a regression shows up as
# a reviewable diff. Re-baseline deliberately:
#   python3 scripts/eval-gate.py <results-dir> --update
#
# Do NOT pipe this target's commands into head/tail in CI — a pipeline exits
# with the pager's status and would turn a failing gate into a green build.
eval-gate:
	python3 ./scripts/eval-gate.py "$${EVAL_SCRATCH:-./scripts}"

# Full loop: measure both query classes, attribute, then gate. This is the one to
# run before/after any change to fusion weights, rerank config, chunking or
# models. Both classes are included because they disagree — a change that helps
# prose retrieval can gut identifier lookup, and only running one class would
# call that a win.
eval-check: eval-retrieval eval-retrieval-lexical eval-evidence eval-gate

# The evidence the gate judges the numbers by: the coverage of the contextual
# organization (99 percent verified, and a baseline that is really raw) and the
# keyword-index parity of both. Fetched from data-quality; a report that cannot be
# read is removed, never left stale. Make the gate demand it:
#   EVAL_REQUIRE_COVERAGE=1 EVAL_REQUIRE_PARITY=1 make eval-gate
eval-evidence:
	python3 ./scripts/eval-service.py coverage "$${EVAL_SCRATCH:-./scripts}"
	python3 ./scripts/eval-service.py gates "$${EVAL_SCRATCH:-./scripts}"

# One variant of the contextual organization (a prompt version, a model, the rerank
# input) for the promotion gate. Putting the organization in that state is an
# operator's act (the runbook, section 5.4); this measures it and DECLARES it:
#   make eval-variant VARIANT=ctx-v2 DECLARE="--prompt ctx-v2"
#   python3 scripts/eval-gate.py "$${EVAL_SCRATCH:-./scripts}" --promote ctx-v2 --against ctx-v1
# 371 retrieval requests, 261 of them reranked (paid): not a loop.
eval-variant:
	./scripts/eval-variant-cell.sh "$${EVAL_SCRATCH:-./scripts}" "$(VARIANT)" $(DECLARE)

# The harness's own tests (no stack, no cost): the frozen corpus matches its manifest,
# the Python tests of the gate (both ways), the promotion gate, the ingest, the golden
# builder and the service client, the contract goldens on both sides, and the shell
# scripts against a stand-in for docker. CI runs the same (the eval-harness job).
eval-corpus-verify:
	python3 ./scripts/eval-ingest-corpus.py verify

eval-harness-test: eval-corpus-verify
	python3 -m unittest discover -s scripts/tests -p "test_eval_*.py"
	bash ./scripts/check-eval-contract.sh
	bash ./scripts/check-eval-contract.sh --self-test
	bash ./tests/scripts/eval-harness-contract-test.sh

test-security-scripts:
	./tests/scripts/auth-regression-matrix-test.sh
	./tests/scripts/smoke-test-test.sh
	./tests/scripts/load-auth-contract-test.sh
	bash ./tests/scripts/image-provenance-contract-test.sh
	bash ./tests/scripts/compose-security-contract-test.sh
	bash ./tests/scripts/graph-index-no-azure-contract-test.sh
	bash ./tests/scripts/background-budget-contract-test.sh

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

migrate-status:
	$(COMPOSE_BUILD_ENV) docker compose run --rm migrate status

migrate-up:
	$(COMPOSE_BUILD_ENV) docker compose run --rm migrate up

# ── Backup & restore ──────────────────────────────────────────────────────────

backup:
	./scripts/backup.sh

restore:
	@echo "Usage: ./scripts/restore.sh <backup-dir>"

# ── CI (local) ────────────────────────────────────────────────────────────────

tenant-static-check:
	./scripts/check-tenant-isolation.sh

tenant-check: tenant-static-check auth-matrix

ci: fmt-rs clippy-rs check-rs build-go vet-go tenant-static-check test-security-scripts
	@echo "CI checks passed"

# ── Dev helpers ────────────────────────────────────────────────────────────────

dev-retrieve:
	@echo "Ad hoc retrieval body output is disabled in the production-readiness harness."
	@echo "Run 'make auth-matrix' with a disposable organization and short-lived bearer."
	@exit 2

dev-trace:
	@echo "Ad hoc trace body output is disabled in the production-readiness harness."
	@echo "Use an isolated fixture through a claim-pinned integration test."
	@exit 2

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


# -- Docker base images -------------------------------------------------------

# Every plane pins its Docker bases by digest (FROM ...@sha256:...). Docker
# re-pulls a missing digest on demand, so nothing breaks without this -- but the
# pull lands mid-build and stalls it, and one `docker image prune -a` strips
# every pin at once. Warm them first instead.
#
# Deliberately NON-FATAL: offline, or on a machine without Docker, the build
# should still proceed and let Docker pull (or fail) on its own terms. Warming
# is an optimisation; `make check-bases` at the root is the gate.
#
# Written as a ONE-LINE recipe on purpose. A backslash continuation needs the
# backslash immediately before the newline, and these files still check out
# CRLF on Windows, which puts a CR in between and silently breaks the join.
#
# The rules below are prerequisite-only: they ADD warm-bases to targets defined
# earlier in this file without redefining their recipes.
REPO_ROOT ?= $(shell git rev-parse --show-toplevel 2>/dev/null || echo ../..)

.PHONY: warm-bases
warm-bases:
	@$(MAKE) --no-print-directory -C "$(REPO_ROOT)" warm-bases 2>/dev/null || echo "  warm-bases skipped; Docker will pull pinned bases on demand"

build: warm-bases
docker-build: warm-bases
docker-build-clean: warm-bases
