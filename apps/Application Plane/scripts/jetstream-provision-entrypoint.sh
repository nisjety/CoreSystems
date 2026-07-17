#!/bin/sh
# Container-side JetStream provisioner (runs inside the natsio/nats-box service on
# app-net; see the `jetstream-provisioner` compose service). Creates the App-Plane
# streams + durable push consumers that services BindProvisioned to. Idempotent —
# safe to run on every `up`. The host-runnable equivalent is
# scripts/provision-jetstream.sh (uses `docker run`); this one talks to app-nats
# directly. Fails the container (non-zero) if provisioning cannot complete, so a
# clean bring-up surfaces the problem instead of silently degrading consumers.
set -eu

: "${APPLICATION_NATS_PROVISIONER_PASSWORD:?APPLICATION_NATS_PROVISIONER_PASSWORD is required}"
SRV="nats://observability-provisioner-application:${APPLICATION_NATS_PROVISIONER_PASSWORD}@app-nats:4222"
# The provisioner principal may only reply on this inbox prefix (see nats.conf);
# without it every request-reply (stream/consumer info/add) times out.
IP="_INBOX.PROVISIONER_APPLICATION"
N="nats --server ${SRV} --inbox-prefix=${IP} --timeout=20s"

echo "[jetstream-provisioner] streams"
# name subjects
for spec in \
  "VELION_APPLICATION velion.application.>" \
  "VELION_INGESTION velion.ingestion.>" \
  "VELION_MODEL velion.model.>" \
  "VELION_AUDIT velion.audit.>"; do
  name=$(echo "$spec" | cut -d' ' -f1); subj=$(echo "$spec" | cut -d' ' -f2)
  if $N stream info "$name" >/dev/null 2>&1; then
    echo "  = $name (exists)"
  else
    $N stream add "$name" --subjects="$subj" --storage=file --retention=limits \
      --discard=old --max-msgs=-1 --max-bytes=-1 --max-age=72h --dupe-window=2m \
      --replicas=1 --defaults >/dev/null
    echo "  + $name ($subj)"
  fi
done

echo "[jetstream-provisioner] consumers"
# stream durable filter deliver   (deliver-group == durable)
add_consumer() {
  if $N consumer info "$1" "$2" >/dev/null 2>&1; then
    echo "  = $1/$2 (exists)"
  else
    $N consumer add "$1" "$2" --filter="$3" --target="$4" --deliver-group="$2" \
      --ack=explicit --deliver=all --replay=instant --defaults >/dev/null
    echo "  + $1/$2"
  fi
}
add_consumer VELION_INGESTION  conversation-core-webhook-received       velion.ingestion.integration.webhook_received      _VELION.APPLICATION.DELIVER.conversation.webhook-received
add_consumer VELION_MODEL      conversation-core-model-action-proposed  velion.model.action.proposed                        _VELION.APPLICATION.DELIVER.conversation.model-action-proposed
add_consumer VELION_APPLICATION conversation-core-ai-action-executor    velion.application.conversation.ai_action.reviewed  _VELION.APPLICATION.DELIVER.conversation.ai-action-reviewed
add_consumer VELION_APPLICATION insight-core-metric-subscriber          "velion.application.>"                              _VELION.APPLICATION.DELIVER.insight.metrics

echo "[jetstream-provisioner] complete"
