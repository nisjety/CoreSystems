#!/usr/bin/env bash
# Local dev setup for visma_service_v2.
set -euo pipefail

cd "$(dirname "$0")/.."

# 1. Bring up Postgres + Redis
docker compose up -d
echo "→ Waiting for Postgres..."
until docker compose exec -T postgres pg_isready -U visma_v2 >/dev/null 2>&1; do sleep 1; done

# 2. Apply SQL migrations (in order)
for f in migrations/*.sql; do
    echo "→ Applying $f"
    docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -f "/docker-entrypoint-initdb.d/$(basename "$f")"
done

# 3. Python tooling
if command -v uv >/dev/null 2>&1; then
    echo "→ Installing Python tooling (uv)"
    uv pip install -e ".[dev]"
else
    echo "→ uv not installed — skipping Python install"
    echo "  install: curl -LsSf https://astral.sh/uv/install.sh | sh"
fi

# 4. Rust build (cheap incremental)
if command -v cargo >/dev/null 2>&1; then
    echo "→ cargo check"
    (cd core && cargo check --workspace) || true
fi

echo "✓ bootstrap complete"
