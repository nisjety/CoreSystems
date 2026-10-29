.PHONY: help up down migrate build test fmt lint discover validate-config endpoint-matrix shadow-diff flag-show merge shadow normalize

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "%-20s %s\n", $$1, $$2}'

up: ## Start Postgres + Redis
	docker compose up -d postgres redis

down: ## Stop infra
	docker compose down

migrate: ## Apply SQL migrations to v2 Postgres
	@for f in migrations/*.sql; do \
		echo "Applying $$f"; \
		docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -f /docker-entrypoint-initdb.d/$$(basename $$f); \
	done

build: ## Build Rust core
	cd core && cargo build --workspace

test: ## Run all tests
	cd core && cargo test --workspace
	uv run pytest

fmt: ## Format
	cd core && cargo fmt --all
	cd tooling && uv run ruff format .

lint: ## Lint
	cd core && cargo clippy --workspace --all-targets -- -D warnings
	cd tooling && uv run ruff check .

discover: ## Run Phase 0 discovery
	cd tooling && uv run vsv2-discover introspect
	cd tooling && uv run vsv2-discover coverage

validate-config: ## Validate endpoint config JSON against schema
	cd tooling && uv run vsv2-validate-config

endpoint-matrix: ## Regenerate docs/endpoint_matrix.md from the configs and live-run evidence
	uv run python scripts/endpoint_matrix.py

## Phase 4 — shadow mode helpers
# Usage: make shadow-diff TABLE=visma_customers PK=number
shadow-diff: ## Diff <TABLE>_dm_shadow vs live bronze. Requires TABLE, PK, PROJECT, DATASET_LIVE, DATASET_SHADOW.
	uv run vsv2-shadow-diff \
		$(TABLE) \
		--project       $(PROJECT) \
		--dataset-live  $(DATASET_LIVE) \
		--dataset-shadow $(DATASET_SHADOW) \
		--pk            $(PK)

# Usage: make flag-show TENANT=33f319b9-4d57-11ee-960c-025417856183
flag-show: ## List all endpoint source override flags for TENANT.
	cargo run --bin vsv2 -- flag-show --tenant $(TENANT)

## Phase 10 — BigQuery merge / shadow / normalize helpers
# Usage: make merge TABLE=visma_customers PROJECT=... DATASET=visma_v2 PK=number [NORMALIZER=customers]
merge: ## MERGE staged rows into live BQ table. Requires TABLE, PROJECT, DATASET, PK. Optional NORMALIZER.
	cd core && cargo run --bin vsv2-bq-merge -- \
		--table      $(TABLE) \
		--project    $(PROJECT) \
		--dataset    $(DATASET) \
		--pk         $(PK) \
		$(if $(NORMALIZER),--normalizer $(NORMALIZER),)

# Usage: make shadow TABLE=visma_customers PROJECT=... DATASET=visma_v2_shadow [NORMALIZER=customers]
shadow: ## Write rows to <TABLE>_dm_shadow for diffing. Requires TABLE, PROJECT, DATASET. Optional NORMALIZER.
	cd core && cargo run --bin vsv2-bq-shadow -- \
		--table      $(TABLE) \
		--project    $(PROJECT) \
		--dataset    $(DATASET) \
		$(if $(NORMALIZER),--normalizer $(NORMALIZER),)

# Usage: make normalize INPUT=raw.json OUTPUT=norm.json NORMALIZER=customers
normalize: ## Run a normalizer over INPUT JSON and emit OUTPUT JSON. Requires INPUT, OUTPUT, NORMALIZER.
	cd core && cargo run --bin vsv2-normalize -- \
		--input      $(INPUT) \
		--output     $(OUTPUT) \
		--normalizer $(NORMALIZER)
