#!/bin/sh
set -eu

create_stream() {
  stream=$1
  subject=$2
  consumer=$3
  nats stream add "$stream" \
    --subjects="$subject" \
    --retention=work \
    --storage=file \
    --replicas=1 \
    --max-age=5m \
    --max-msgs=64 \
    --max-bytes=1048576 \
    --max-msg-size=262144 \
    --dupe-window=2m \
    --deny-delete \
    --deny-purge \
    --defaults >/dev/null
  nats consumer add "$stream" "$consumer" \
    --pull \
    --ack=explicit \
    --deliver=all \
    --filter="$subject" \
    --max-deliver=3 \
    --max-pending=8 \
    --wait=1s \
    --defaults >/dev/null
}

wait_for_messages() {
  stream=$1
  expected=$2
  attempts=0
  while [ "$attempts" -lt 20 ]; do
    actual=$(nats stream info "$stream" -j | jq -r '.state.messages')
    [ "$actual" = "$expected" ] && return 0
    attempts=$((attempts + 1))
    sleep 0.1
  done
  echo "FAIL: $stream message count did not become $expected" >&2
  return 1
}

exercise_stream() {
  stream=$1
  subject=$2
  consumer=$3
  shape=$4
  id="isolated-$shape-message"
  body=$(printf '{"shape":"%s","fixture":"synthetic"}' "$shape")

  # -J requires a JetStream PubAck. Reusing the stable message id must return
  # an acknowledgement without storing a duplicate.
  nats pub -J -H "Nats-Msg-Id:$id" "$subject" "$body" >/dev/null
  nats pub -J -H "Nats-Msg-Id:$id" "$subject" "$body" >/dev/null
  wait_for_messages "$stream" 1

  first=$(nats consumer next "$stream" "$consumer" --raw --nak --wait=2s)
  [ "$first" = "$body" ] || { echo "FAIL: $stream first delivery changed" >&2; return 1; }
  second=$(nats consumer next "$stream" "$consumer" --raw --ack --wait=2s)
  [ "$second" = "$body" ] || { echo "FAIL: $stream redelivery changed" >&2; return 1; }
  wait_for_messages "$stream" 0

  info=$(nats consumer info "$stream" "$consumer" -j)
  delivered=$(printf '%s' "$info" | jq -r '.delivered.consumer_seq')
  acked=$(printf '%s' "$info" | jq -r '.ack_floor.consumer_seq')
  [ "$delivered" -ge 2 ] || { echo "FAIL: $stream did not redeliver after NAK" >&2; return 1; }
  [ "$acked" -ge 2 ] || { echo "FAIL: $stream did not record the explicit ACK" >&2; return 1; }
}

create_stream DATAPLANE_DOCUMENTS dataplane.documents.created broker-documents
create_stream DATAPLANE_SOURCE_OBJECTS dataplane.source_objects.changed broker-sources
create_stream DATAPLANE_COST dataplane.cost.ledger broker-cost

exercise_stream DATAPLANE_DOCUMENTS dataplane.documents.created broker-documents document
exercise_stream DATAPLANE_SOURCE_OBJECTS dataplane.source_objects.changed broker-sources source
exercise_stream DATAPLANE_COST dataplane.cost.ledger broker-cost cost

echo "PASS: bounded document/source/cost PubAck, dedupe, NAK/redelivery, and ACK matrix"
