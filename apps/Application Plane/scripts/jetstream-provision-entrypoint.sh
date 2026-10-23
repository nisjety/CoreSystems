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
  "VEREVON_APPLICATION verevon.application.>" \
  "VEREVON_INGESTION verevon.ingestion.>" \
  "VEREVON_MODEL verevon.model.>"; do
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

echo "[jetstream-provisioner] audit-core stream (plane application)"
# audit-core reads this plane's audit and usage events from one stream with two
# durables. Values must equal audit-core's provisioner (audit-core/internal/
# provisioner, plane "application"): both converge this stream and a difference
# would make each undo the other. scripts/tests/audit-stream-contract-test.sh
# pins them. Any failure here exits non-zero (set -e) so the stack stays down
# rather than publishing audit events into a missing stream.
AUDIT_STREAM=VEREVON_CONTROL_OBSERVABILITY
# The pre-audit-core VEREVON_AUDIT stream captured verevon.audit.>, which NATS
# refuses to overlap with the stream below. Consumer inventory: nothing binds it
# (no durable in this script, none in nats.conf) and the only audit publishers on
# this broker (conversation-core, leads-core) publish under
# verevon.audit.v2.application.*. Narrowing keeps its retained messages (72 h).
if $N stream info VEREVON_AUDIT 2>/dev/null | grep -q 'verevon\.audit\.>'; then
  $N stream edit VEREVON_AUDIT --subjects='verevon.audit.retired.>' --force >/dev/null
  echo "  ~ VEREVON_AUDIT (narrowed to verevon.audit.retired.>)"
fi
if $N stream info "$AUDIT_STREAM" >/dev/null 2>&1; then
  echo "  = $AUDIT_STREAM (exists)"
else
  $N stream add "$AUDIT_STREAM" \
    --subjects='verevon.audit.v1.application.>,verevon.usage.v1.application.>,verevon.audit.v2.application.>,verevon.usage.v2.application.>,verevon.dlq.audit-core.>' \
    --storage=file --retention=limits --discard=old --max-msgs=2000000 --max-bytes=-1 \
    --max-age=720h --dupe-window=2m --replicas=1 --defaults >/dev/null
  echo "  + $AUDIT_STREAM"
fi

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
# audit-core binds these with nats.Bind and cannot create them; ack-wait and
# max-deliver are explicit because its provisioner refuses a durable that differs.
add_audit_consumer() {
  if $N consumer info "$AUDIT_STREAM" "audit-core-application-v3-$1" >/dev/null 2>&1; then
    echo "  = $AUDIT_STREAM/audit-core-application-v3-$1 (exists)"
  else
    $N consumer add "$AUDIT_STREAM" "audit-core-application-v3-$1" \
      --filter="verevon.$1.v2.application.>" --target="_VEREVON.AUDIT.DELIVER.application.$1-v2" \
      --deliver-group="audit-core-application-v3-$1" --ack=explicit --wait=30s --max-deliver=5 \
      --deliver=all --replay=instant --defaults >/dev/null
    echo "  + $AUDIT_STREAM/audit-core-application-v3-$1"
  fi
}
add_audit_consumer audit
add_audit_consumer usage
add_consumer VEREVON_INGESTION  conversation-core-webhook-received       verevon.ingestion.integration.webhook_received      _VEREVON.APPLICATION.DELIVER.conversation.webhook-received
add_consumer VEREVON_MODEL      conversation-core-model-action-proposed  verevon.model.action.proposed                        _VEREVON.APPLICATION.DELIVER.conversation.model-action-proposed
add_consumer VEREVON_APPLICATION conversation-core-ai-action-executor    verevon.application.conversation.ai_action.reviewed  _VEREVON.APPLICATION.DELIVER.conversation.ai-action-reviewed
add_consumer VEREVON_APPLICATION insight-core-metric-subscriber          "verevon.application.>"                              _VEREVON.APPLICATION.DELIVER.insight.metrics
add_consumer VEREVON_INGESTION  insight-core-ingestion-subscriber        "verevon.ingestion.>"                                _VEREVON.APPLICATION.DELIVER.insight.ingestion

echo "[jetstream-provisioner] complete"
