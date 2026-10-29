#!/usr/bin/env bash
# Run Phase 0 discovery end-to-end against the configured tenant.
set -euo pipefail

cd "$(dirname "$0")/.."

# Step 1 — verify OAuth + GraphQL reachability
echo "→ Step 1/4 — OAuth + DataMart smoke (Rust)"
( cd core && cargo run --quiet --bin vsv2 -- dm-ping ) || {
    echo "✗ dm-ping failed — check VISMA_CLIENT_ID / VISMA_OAUTH_SCOPE_DATAMART"
    exit 1
}

# Step 2 — list companies → operator picks Aquatiq AS, sets env
echo "→ Step 2/4 — companies"
( cd tooling && uv run vsv2-discover companies )

# Gate: VSV2_AQUATIQ_DATAMART_COMPANY_ID must be set after step 2.
: "${VSV2_AQUATIQ_DATAMART_COMPANY_ID:?Set VSV2_AQUATIQ_DATAMART_COMPANY_ID in .env then re-run}"

# Step 3 — full schema introspection (Python, writes SDL + introspection.json)
echo "→ Step 3/4 — introspect"
( cd tooling && uv run vsv2-discover introspect \
    --out core/queries/_generated/datamart.sdl )

# Step 4 — coverage matrix + rate-limit probe
echo "→ Step 4/4 — coverage + probe"
( cd tooling && uv run vsv2-discover coverage \
    --bronze-dir ../../Dataform/definitions/bronze/visma \
    --out docs/phase0_coverage.md )
( cd tooling && uv run vsv2-discover probe \
    --pages 10 \
    --out docs/phase0_probe.md )

echo "✓ Phase 0 complete"
echo "  - core/queries/_generated/datamart.sdl"
echo "  - docs/phase0_coverage.md"
echo "  - docs/phase0_probe.md"
