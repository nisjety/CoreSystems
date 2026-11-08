#!/usr/bin/env bash
# Recreate Model Plane services built from the worktree with the SAME environment chain the live stack uses.
# The worktree has only a stale copy of deploy/.env and none of the generated-secrets files, so a bare
# compose.sh run from here drops variables the running stack has (the decision signing key, rotated NATS
# passwords, service tokens). The live files are passed to compose as extra --env-file inputs, read-only,
# and later files win; each service's NATS password is carried over from the running NATS server's own
# environment. Nothing is printed, copied or written.
cd "C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b" || exit 1
LIVE="C:/dev/CoresSystem/apps"
ENVFILES=(
  "$LIVE/Control Plane/.env.generated-secrets"
  "$LIVE/Model Plane/deploy/.env.generated-secrets"
  "$LIVE/Model Plane/deploy/.env"
)
# EXTRA_ENV_FILE (optional, outside both checkouts) is passed LAST, so its values win:
# the owner's Anthropic test key and DEFAULT_MODEL live there (2026-10-27).
if [ -n "${EXTRA_ENV_FILE:-}" ]; then ENVFILES+=("$EXTRA_ENV_FILE"); fi
args=()
for f in "${ENVFILES[@]}"; do
  [ -r "$f" ] || { echo "missing live env file: $(basename "$f")"; exit 2; }
  args+=(--env-file "$f")
done
nats_pw() { # $1 = variable name
  local v
  v="$(docker inspect model-plane-nats-1 --format '{{range .Config.Env}}{{println .}}{{end}}' | sed -n "s/^$1=//p")"
  v="${v#\"}"; v="${v%\"}"
  printf '%s' "$v"
}
declare -A PWVAR=(
  [capability-core]=MODEL_CAPABILITY_CORE_NATS_PASSWORD
  [model-gateway]=MODEL_GATEWAY_NATS_PASSWORD
  [model-gateway-b]=MODEL_GATEWAY_NATS_PASSWORD
  [session-core]=MODEL_SESSION_CORE_NATS_PASSWORD
  [orchestrator-core]=MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD
  [cost-core]=MODEL_COST_CORE_NATS_PASSWORD
)
for s in "$@"; do
  echo "== recreate $s"
  var="${PWVAR[$s]:-}"
  if [ -n "$var" ]; then export "$var=$(nats_pw "$var")"; fi
  bash "apps/Model Plane/scripts/compose.sh" "${args[@]}" up -d --no-deps "$s" 2>&1 | grep -v -i -E "password|token|secret" | tail -4
  if [ -n "$var" ]; then unset "$var"; fi
done
