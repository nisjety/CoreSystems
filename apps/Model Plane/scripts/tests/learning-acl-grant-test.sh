#!/usr/bin/env bash
set -euo pipefail

# Live-ACL proof for the G7 learning loop.
#
# scripts/tests/learning-nats-trigger-test.sh already proves subject routing and
# the review->persist path, but it runs against an UNAUTHENTICATED server. That
# cannot reproduce the way this loop actually stayed dead: NATS reports a
# publish-permission denial only to the publisher's async error handler and drops
# the message, so `Publish()` returns nil, both services log success, and no
# skill is ever learned. The grant is the invariant, so it is tested against the
# real deploy/nats.conf with the real principals.
#
# Passwords here are throwaway values for a disposable container; the config
# reads them from the environment exactly as it does in Compose.

CONTAINER="model-plane-acl-grant-${RANDOM}-$$"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONF="${ROOT_DIR}/deploy/nats.conf"

test -f "$CONF" || { echo "missing $CONF" >&2; exit 1; }

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

SESSION_PW="acl-test-session-core"
CAPABILITY_PW="acl-test-capability-core"

# Every principal in the config needs a value or nats-server refuses to start;
# only the two under test get distinct passwords.
docker run -d --name "$CONTAINER" -P \
  --tmpfs /data \
  -v "${CONF}:/etc/nats/nats.conf:ro" \
  -e "MODEL_SESSION_CORE_NATS_PASSWORD=${SESSION_PW}" \
  -e "MODEL_CAPABILITY_CORE_NATS_PASSWORD=${CAPABILITY_PW}" \
  -e "MODEL_GATEWAY_NATS_PASSWORD=unused" \
  -e "MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD=unused" \
  -e "MODEL_TOOL_COMPLETION_NATS_PASSWORD=unused" \
  -e "MODEL_COST_CORE_NATS_PASSWORD=unused" \
  -e "APPLICATION_CONVEX_MODEL_NATS_PASSWORD=unused" \
  -e "APPLICATION_INSIGHT_MODEL_NATS_PASSWORD=unused" \
  -e "AUDIT_MODEL_NATS_PASSWORD=unused" \
  -e "MODEL_NATS_PROVISIONER_PASSWORD=unused" \
  nats:2-alpine -c /etc/nats/nats.conf >/dev/null

PORT="$(docker port "$CONTAINER" 4222/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
test -n "$PORT" || { echo "no mapped 4222 port" >&2; docker logs "$CONTAINER" >&2; exit 1; }

# The config binds monitoring to container-local 127.0.0.1, so readiness is read
# from the server's own log line rather than /varz.
for _ in $(seq 1 30); do
  if docker logs "$CONTAINER" 2>&1 | grep -q "Server is ready"; then
    break
  fi
  sleep 1
done
docker logs "$CONTAINER" 2>&1 | grep -q "Server is ready" || {
  echo "nats-server did not become ready with the production ACL:" >&2
  docker logs "$CONTAINER" >&2
  exit 1
}

cd "$ROOT_DIR/go/services/capability-core"
NATS_ACL_SESSION_CORE_URL="nats://session-core-runtime:${SESSION_PW}@127.0.0.1:${PORT}" \
NATS_ACL_CAPABILITY_CORE_URL="nats://capability-core-runtime:${CAPABILITY_PW}@127.0.0.1:${PORT}" \
  go test ./internal/sessionreview -run '^TestRunEventACLGrant_ProductionConfig$' -count=1 -v 2>&1 | tail -20

echo "learning ACL grant: ok"
