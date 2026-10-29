#!/usr/bin/env bash
# seed_tool_knowledge.sh — idempotently seed every built-in tool's skill +
# knowledge-base content (seed_tool_knowledge.sql) into session-core's
# agent_skills/agent_memory tables for one or more orgs.
#
# Safe to re-run any time (every statement is an upsert keyed by a stable id,
# never a delete) — re-run it whenever the tool catalogue changes
# (model-gateway::tool_loop::builtin_tool_defs /
# execution-core::runtime_loop::agent::offered_tool_defs) so the seeded
# content doesn't drift from the real tools.
#
# Talks directly to session-core's own Postgres via the running Compose
# stack's `postgres` service (through compose.sh, so it reuses the same
# env-file/override resolution as every other Model Plane command) — no new
# service credential or Auth Core registry grant needed, since this writes
# straight into session-core's database rather than calling its gRPC API.
#
# Usage:
#   SEED_TOOL_KNOWLEDGE_ORG_IDS=org_abc,org_def ./scripts/seed_tool_knowledge.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SQL_FILE="$ROOT_DIR/scripts/seed_tool_knowledge.sql"

die() {
  echo "seed_tool_knowledge error: $*" >&2
  exit 1
}

[[ -r "$SQL_FILE" ]] || die "missing $SQL_FILE"

raw_org_ids="${SEED_TOOL_KNOWLEDGE_ORG_IDS:-}"
[[ -n "$raw_org_ids" ]] || die "SEED_TOOL_KNOWLEDGE_ORG_IDS is required (comma-separated org ids) — this is a manual, explicit seed run, not an auto-discovering worker"

org_ids=()
IFS=',' read -ra parts <<<"$raw_org_ids"
for part in "${parts[@]}"; do
  trimmed="$(echo "$part" | xargs)"
  [[ -n "$trimmed" ]] && org_ids+=("$trimmed")
done
[[ ${#org_ids[@]} -gt 0 ]] || die "SEED_TOOL_KNOWLEDGE_ORG_IDS contained no non-blank org ids"

container="$("$ROOT_DIR/scripts/compose.sh" ps -q postgres 2>/dev/null || true)"
[[ -n "$container" ]] || die "Model Plane's postgres service is not running (scripts/compose.sh ps -q postgres returned nothing) — bring the stack up first"

echo "seed_tool_knowledge: seeding ${#org_ids[@]} org(s) into container ${container:0:12}"
for org_id in "${org_ids[@]}"; do
  echo "seed_tool_knowledge: org=$org_id"
  docker exec -i "$container" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -v orgid="$org_id" -q <"$SQL_FILE"
done
echo "seed_tool_knowledge: done (${#org_ids[@]} org(s): ${org_ids[*]})"
