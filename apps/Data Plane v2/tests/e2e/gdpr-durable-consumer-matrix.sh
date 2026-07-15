#!/usr/bin/env bash
set -euo pipefail
set +x

: "${BROKER_COMPOSE_PROJECT:?required}"
: "${BROKER_COMPOSE_FILE:?required}"
: "${DOCUMENTS_GDPR_NATS_PASSWORD:?required}"
: "${GDPR_TEST_ORG_ID:?required}"
: "${GDPR_TEST_OWNER_OK:?required}"
: "${GDPR_TEST_OWNER_RETRY:?required}"

case "$BROKER_COMPOSE_PROJECT" in
  dpv2-broker-e2e-*) ;;
  *) echo "refusing non-isolated GDPR broker project" >&2; exit 2 ;;
esac
root=$(cd "$(dirname "$0")/../.." && pwd)
expected_compose="$root/tests/e2e/isolated/broker-compose.yml"
provided_compose=$(cd "$(dirname "$BROKER_COMPOSE_FILE")" && pwd)/$(basename "$BROKER_COMPOSE_FILE")
[ "$provided_compose" = "$expected_compose" ] || {
  echo "refusing unexpected GDPR broker compose file" >&2
  exit 2
}

compose=(docker compose --project-name "$BROKER_COMPOSE_PROJECT" -f "$BROKER_COMPOSE_FILE" --profile gdpr)
stream="AQENCIA_CONTROLPLANE"
consumer="documents-api-gdpr-erasure-v1"
subject="velion.gdpr.erasure.requested"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

control_nats() {
  "${compose[@]}" run --rm --no-deps control-probe nats "$@"
}

scoped_nats() {
  "${compose[@]}" run --rm --no-deps \
    -e NATS_USER=documents-api-gdpr \
    -e NATS_PASSWORD="$DOCUMENTS_GDPR_NATS_PASSWORD" \
    control-probe nats --inbox-prefix=_INBOX.DOCUMENTS_GDPR "$@"
}

consumer_info() {
  control_nats consumer info "$stream" "$consumer" -j 2>/dev/null
}

health_snapshot() {
  "${compose[@]}" exec -T documents-gdpr \
    curl -fsS http://127.0.0.1:8010/internal/gdpr/health 2>/dev/null
}

document_owner() {
  local document_id=$1
  "${compose[@]}" exec -T gdpr-postgres \
    psql -U dataplane -d dataplane -Atc \
      "SELECT owner_id FROM documents WHERE document_id = '$document_id' AND org_id = '$GDPR_TEST_ORG_ID'" 2>/dev/null
}

insert_document() {
  local document_id=$1 owner_id=$2
  "${compose[@]}" exec -T gdpr-postgres \
    psql -U dataplane -d dataplane -v ON_ERROR_STOP=1 -c \
      "INSERT INTO documents (document_id, org_id, source, type, title, content, status, owner_id, visibility) VALUES ('$document_id', '$GDPR_TEST_ORG_ID', 'isolated-gdpr', 'text', 'Synthetic fixture', 'Synthetic fixture', 'ready', '$owner_id', 'private')" \
    >/dev/null 2>&1
}

publish_erasure() {
  local event_id=$1 owner_id=$2
  local body
  body=$(printf '{"event_id":"%s","operation_id":"%s","subject_type":"user","subject_id":"%s","org_id":"%s","requested_by":"isolated-control","mode":"erase"}' \
    "$event_id" "$event_id" "$owner_id" "$GDPR_TEST_ORG_ID")
  control_nats pub -J -H "Nats-Msg-Id:$event_id" "$subject" "$body" >/dev/null 2>&1
}

wait_for_owner() {
  local document_id=$1 expected=$2
  local attempt actual
  for attempt in $(seq 1 80); do
    actual=$(document_owner "$document_id" || true)
    [ "$actual" = "$expected" ] && return 0
    sleep 0.25
  done
  return 1
}

wait_for_healthy_acknowledged() {
  local minimum=$1
  local attempt snapshot status acknowledged ack_pending
  for attempt in $(seq 1 80); do
    snapshot=$(health_snapshot || true)
    status=$(printf '%s' "$snapshot" | jq -r '.data.status // empty' 2>/dev/null || true)
    acknowledged=$(printf '%s' "$snapshot" | jq -r '.data.acknowledged // 0' 2>/dev/null || true)
    ack_pending=$(printf '%s' "$snapshot" | jq -r '.data.ack_pending // -1' 2>/dev/null || true)
    if [ "$status" = "healthy" ] && [ "$acknowledged" -ge "$minimum" ] && [ "$ack_pending" -eq 0 ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

initial_health=$(health_snapshot)
[ "$(printf '%s' "$initial_health" | jq -r '.data.status')" = "healthy" ] || fail "real GDPR consumer was not healthy after binding"
[ "$(printf '%s' "$initial_health" | jq -r '.data.acknowledged')" -eq 0 ] || fail "isolated consumer did not start clean"

# The documents principal may inspect only its own pre-provisioned durable. It
# cannot publish Control input or administer streams/consumers.
scoped_nats consumer info "$stream" "$consumer" -j >/dev/null 2>&1 || fail "scoped consumer cannot inspect its own durable"
if scoped_nats pub -J "$subject" '{"fixture":"must-not-publish"}' >/dev/null 2>&1; then
  fail "documents principal published a Control-owned GDPR request"
fi
if scoped_nats stream add UNAUTHORIZED --subjects=unauthorized.fixture --defaults >/dev/null 2>&1; then
  fail "documents principal gained stream administration"
fi
if scoped_nats pub '$JS.ACK.AQENCIA_CONTROLPLANE.other-consumer.1.1.1.1.1' '' >/dev/null 2>&1; then
  fail "documents principal ACKed a different durable"
fi

document_ok="doc-ok-${GDPR_TEST_OWNER_OK#owner-}"
event_ok="gdpr-ok-${GDPR_TEST_OWNER_OK#owner-}"
insert_document "$document_ok" "$GDPR_TEST_OWNER_OK"
publish_erasure "$event_ok" "$GDPR_TEST_OWNER_OK"
wait_for_owner "$document_ok" "org-system-account" || fail "valid durable event did not transfer synthetic ownership"
wait_for_healthy_acknowledged 1 || fail "valid durable event did not converge to healthy ACK state"

after_valid=$(consumer_info)
baseline_ack=$(printf '%s' "$after_valid" | jq -r '.ack_floor.consumer_seq')
[ "$(printf '%s' "$after_valid" | jq -r '.num_ack_pending')" -eq 0 ] || fail "valid durable event remained ACK-pending"

document_retry="doc-retry-${GDPR_TEST_OWNER_RETRY#owner-}"
event_retry="gdpr-retry-${GDPR_TEST_OWNER_RETRY#owner-}"
insert_document "$document_retry" "$GDPR_TEST_OWNER_RETRY"

# A disposable database outage forces the real repository operation to fail.
# The consumer must NAK/redeliver and keep the ownership row unchanged until
# Postgres is healthy again; teardown removes only this random Compose project.
"${compose[@]}" stop -t 2 gdpr-postgres >/dev/null
publish_erasure "$event_retry" "$GDPR_TEST_OWNER_RETRY"

redelivery_observed=0
for attempt in $(seq 1 80); do
  info=$(consumer_info || true)
  ack_pending=$(printf '%s' "$info" | jq -r '.num_ack_pending // 0' 2>/dev/null || true)
  redelivered=$(printf '%s' "$info" | jq -r '.num_redelivered // 0' 2>/dev/null || true)
  ack_floor=$(printf '%s' "$info" | jq -r '.ack_floor.consumer_seq // 0' 2>/dev/null || true)
  snapshot=$(health_snapshot || true)
  status=$(printf '%s' "$snapshot" | jq -r '.data.status // empty' 2>/dev/null || true)
  acknowledged=$(printf '%s' "$snapshot" | jq -r '.data.acknowledged // 0' 2>/dev/null || true)
  if [ "$ack_pending" -ge 1 ] && [ "$redelivered" -ge 1 ] && [ "$ack_floor" -eq "$baseline_ack" ] && \
     [ "$status" = "degraded" ] && [ "$acknowledged" -eq 1 ]; then
    redelivery_observed=1
    break
  fi
  sleep 0.5
done
[ "$redelivery_observed" -eq 1 ] || fail "dependency failure was ACKed or did not redeliver fail-closed"

"${compose[@]}" start gdpr-postgres >/dev/null
for attempt in $(seq 1 60); do
  if "${compose[@]}" exec -T gdpr-postgres pg_isready -U dataplane >/dev/null 2>&1; then
    break
  fi
  [ "$attempt" -lt 60 ] || fail "isolated Postgres did not recover"
  sleep 0.5
done

wait_for_owner "$document_retry" "org-system-account" || fail "redelivered event did not converge after dependency recovery"
wait_for_healthy_acknowledged 2 || fail "consumer health did not recover after redelivered ACK"

final_info=$(consumer_info)
final_ack=$(printf '%s' "$final_info" | jq -r '.ack_floor.consumer_seq')
[ "$final_ack" -gt "$baseline_ack" ] || fail "redelivered event did not advance durable ACK floor"
[ "$(printf '%s' "$final_info" | jq -r '.num_ack_pending')" -eq 0 ] || fail "redelivered event remained ACK-pending"
[ "$(printf '%s' "$final_info" | jq -r '.num_pending')" -eq 0 ] || fail "durable consumer retained pending GDPR input"

echo "PASS: real documents GDPR durable bind, scoped authority, health, ACK, fail-closed redelivery, and recovery"
