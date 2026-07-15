#!/bin/sh
set -eu

stream="AQENCIA_CONTROLPLANE"
consumer="documents-api-gdpr-erasure-v1"
delivery="_VELION.CONTROL.SHARED.DELIVER.data.documents-api.gdpr-erasure"
subject="velion.gdpr.erasure.requested"

nats stream add "$stream" \
  --subjects="velion.gdpr.erasure.requested,velion.gdpr.ownership.transferred,velion.gdpr.erasure.dlq.documents-api" \
  --retention=limits \
  --storage=file \
  --replicas=1 \
  --max-age=10m \
  --max-msgs=128 \
  --max-bytes=2097152 \
  --max-msg-size=262144 \
  --dupe-window=2m \
  --deny-delete \
  --deny-purge \
  --defaults >/dev/null

nats consumer add "$stream" "$consumer" \
  --target="$delivery" \
  --deliver-group="$consumer" \
  --ack=explicit \
  --deliver=all \
  --filter="$subject" \
  --max-deliver=20 \
  --max-pending=8 \
  --wait=1s \
  --defaults >/dev/null

echo "PASS: isolated Control-style GDPR stream and durable consumer provisioned"
