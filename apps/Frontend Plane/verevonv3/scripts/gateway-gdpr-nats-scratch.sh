#!/usr/bin/env bash
# Throwaway NATS JetStream (and Redis) for the WS4.C erasure evidence.
#
# The gateway's GDPR consumer (apps/gateway/src/gdpr_nats.rs) binds a PROVISIONED durable
# pull consumer on stream CONTROLPLANE and refuses a different filter subject, so
# a bare NATS server is not enough. This script starts a scratch nats:2-alpine -js, creates
# the stream and the durable the way audit-core's provisioner declares them
# (verevonGatewayOrgErasureConsumerConfig: durable verevon-gateway-gdpr-erasure-v1, filter
# verevon.gdpr.erasure.requested, pull, deliver all, explicit ack, 30 s ack wait, max
# deliver 20), starts a scratch Redis, and runs the gated Rust test that publishes org
# erasure events and runs the production consumer loop.
#
# STATE PLAINLY IN ANY RECORD OF A RUN: the broker is a scratch NATS on this host, not the
# shared control-shared-nats of the Control Plane, and it has no auth, so the ACLs of
# control-shared-nats.conf are not exercised.
#
#   gateway-gdpr-nats-scratch.sh up     start nats + redis, create the stream and the durable
#   gateway-gdpr-nats-scratch.sh test   run the gated test against them (fails on a skip)
#   gateway-gdpr-nats-scratch.sh down   remove both containers
#   gateway-gdpr-nats-scratch.sh run    up, test, down (down also on failure)
#
# Environment (all optional):
#   NATS_PORT, REDIS_PORT   loopback ports to publish (default 14222, 16390)
#   NATS_IMAGE              default nats:2-alpine
#   REDIS_IMAGE             default redis:7-alpine
#   NATSBOX_IMAGE           image that carries the nats CLI (default natsio/nats-box:latest)
#   DOCKER_PREFIX           a wrapper put before every docker command, e.g. the shared lock:
#                           DOCKER_PREFIX="bash <scratchpad>/tools/with-lock.sh docker 2700 --"
#   PROVISION_DURABLE_WITH  natsbox (default) or migrate: converge the durable with
#                           audit-core's own cmd/nats-consumer-migrate (needs go), which is
#                           the exact wanted shape instead of a copy of it
#
# It touches only the containers it names (scratch-gdpr-nats, scratch-gdpr-redis) and never
# reads or writes any .env file.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
GATEWAY_DIR="$HERE/../apps/gateway"
AUDIT_CORE_DIR="$HERE/../../../Control Plane/audit-core"

NATS_PORT=${NATS_PORT:-14222}
REDIS_PORT=${REDIS_PORT:-16390}
NATS_IMAGE=${NATS_IMAGE:-nats:2-alpine}
REDIS_IMAGE=${REDIS_IMAGE:-redis:7-alpine}
NATSBOX_IMAGE=${NATSBOX_IMAGE:-natsio/nats-box:latest}
PROVISION_DURABLE_WITH=${PROVISION_DURABLE_WITH:-natsbox}
NATS_NAME=scratch-gdpr-nats
REDIS_NAME=scratch-gdpr-redis

STREAM=CONTROLPLANE
SUBJECT=verevon.gdpr.erasure.requested
DURABLE=verevon-gateway-gdpr-erasure-v1

dk() {
  # DOCKER_PREFIX is intentionally unquoted: it is a command prefix.
  # shellcheck disable=SC2086
  ${DOCKER_PREFIX:-} docker "$@"
}

wait_tcp() { # host port seconds
  local deadline=$((SECONDS + $3))
  until (exec 3<>"/dev/tcp/$1/$2") 2>/dev/null; do
    [ "$SECONDS" -ge "$deadline" ] && { echo "timed out waiting for $1:$2" >&2; return 1; }
    sleep 1
  done
}

up() {
  for name in "$NATS_NAME" "$REDIS_NAME"; do
    if dk ps -a --format '{{.Names}}' | grep -qx "$name"; then
      echo "refusing: container $name already exists (run 'down' first)" >&2
      exit 2
    fi
  done
  dk run -d --name "$NATS_NAME" -p "127.0.0.1:${NATS_PORT}:4222" "$NATS_IMAGE" -js >/dev/null
  dk run -d --name "$REDIS_NAME" -p "127.0.0.1:${REDIS_PORT}:6379" "$REDIS_IMAGE" >/dev/null
  wait_tcp 127.0.0.1 "$NATS_PORT" 30
  wait_tcp 127.0.0.1 "$REDIS_PORT" 30

  # The stream org-core and user-core publish into. Only the erasure subject is needed
  # here; the real stream carries more subjects (provisioner.go ProvisionControlSharedRuntime).
  dk run --rm --network "container:$NATS_NAME" "$NATSBOX_IMAGE" nats --server nats://127.0.0.1:4222 \
    stream add "$STREAM" --subjects "$SUBJECT" --storage memory --retention limits --discard old \
    --max-age 24h --max-msgs 100000 --replicas 1 --defaults >/dev/null
  echo "stream $STREAM created (subject $SUBJECT, memory storage)"

  case "$PROVISION_DURABLE_WITH" in
    natsbox)
      dk run --rm --network "container:$NATS_NAME" "$NATSBOX_IMAGE" nats --server nats://127.0.0.1:4222 \
        consumer add "$STREAM" "$DURABLE" --pull --filter "$SUBJECT" --deliver all --ack explicit \
        --wait 30s --max-deliver 20 --replay instant --defaults >/dev/null
      ;;
    migrate)
      # The dummy credentials are accepted by a server with no auth configured.
      (cd "$AUDIT_CORE_DIR" && GOWORK=off NATS_URL="nats://127.0.0.1:${NATS_PORT}" NATS_USER=scratch \
        NATS_PASSWORD=scratch go run ./cmd/nats-consumer-migrate --stream="$STREAM" --durable="$DURABLE")
      ;;
    *) echo "PROVISION_DURABLE_WITH must be natsbox or migrate" >&2; exit 2 ;;
  esac

  # Check what the gateway checks: the durable exists, is pull, and filters the subject.
  info=$(dk run --rm --network "container:$NATS_NAME" "$NATSBOX_IMAGE" nats --server nats://127.0.0.1:4222 \
    consumer info "$STREAM" "$DURABLE" --json)
  echo "$info" | grep -q "\"filter_subject\": \"$SUBJECT\"" || { echo "durable filter is not $SUBJECT" >&2; exit 1; }
  echo "$info" | grep -q '"deliver_subject"' && { echo "durable is a push consumer, the gateway needs a pull consumer" >&2; exit 1; }
  echo "durable $DURABLE provisioned (pull, filter $SUBJECT)"
  echo "nats://127.0.0.1:${NATS_PORT}  redis://127.0.0.1:${REDIS_PORT}"
}

run_test() {
  local log
  log=$(mktemp)
  # --nocapture so the "skipping" line is visible: a skipped test also reports "ok".
  if ! (cd "$GATEWAY_DIR" &&
    GATEWAY_TEST_NATS_URL="nats://127.0.0.1:${NATS_PORT}" GATEWAY_TEST_REDIS_URL="redis://127.0.0.1:${REDIS_PORT}" \
      cargo test a_published_org_erasure_purges_result_cache_keys_through_a_real_jetstream -- --nocapture) 2>&1 | tee "$log"; then
    echo "TEST FAILED" >&2
    rm -f "$log"
    return 1
  fi
  if grep -q "skipping:" "$log"; then
    echo "TEST DID NOT RUN: the gate variables were not seen (a skip is not a pass)" >&2
    rm -f "$log"
    return 1
  fi
  grep -q "1 passed" "$log" || { echo "TEST did not report 1 passed" >&2; rm -f "$log"; return 1; }
  rm -f "$log"
  echo "TEST PASSED: org A's result-cache keys were purged through a real JetStream; org B's, org A-more's and the identity key survived (scratch broker, not control-shared-nats)"
}

down() {
  dk rm -f "$NATS_NAME" "$REDIS_NAME" >/dev/null 2>&1 || true
  echo "removed $NATS_NAME and $REDIS_NAME"
}

case "${1:-}" in
  up) up ;;
  test) run_test ;;
  down) down ;;
  run)
    trap down EXIT
    up
    run_test
    ;;
  *) sed -n '2,32p' "${BASH_SOURCE[0]}"; exit 2 ;;
esac
