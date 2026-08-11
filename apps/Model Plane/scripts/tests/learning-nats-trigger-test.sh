#!/usr/bin/env bash
set -euo pipefail

# Live-bus proof for the learning-review trigger. Boundary clients remain fakes
# inside the Go test; the NATS subscription, subject routing, delivery, and
# cancellation are exercised against a real server.

CONTAINER="model-plane-learning-nats-${RANDOM}-$$"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$CONTAINER" -P nats:2-alpine -m 8222 >/dev/null
PORT="$(docker port "$CONTAINER" 4222/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
MONITOR_PORT="$(docker port "$CONTAINER" 8222/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
test -n "$PORT" -a -n "$MONITOR_PORT"
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${MONITOR_PORT}/varz" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

(cd "$ROOT_DIR/go/services/capability-core" && NATS_URL="nats://127.0.0.1:${PORT}" \
  go test ./internal/sessionreview -run '^TestRunConsumerAgainstLiveNATS$' -count=1)

echo "learning NATS trigger: ok"
