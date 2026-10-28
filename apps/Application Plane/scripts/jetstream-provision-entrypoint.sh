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
# nats prints durations in nanoseconds (336 h is 1209600000000000, 30 s is 30000000000).
# A value matches only as a whole number: the next character must not be a digit (or
# the line must end), so 10000000 does not match 1000000.
json_has() { printf '%s\n' "$1" | grep -Eq "\"$2\": ?$3([^0-9]|$)"; }

echo "[jetstream-provisioner] streams"
# Runtime streams take audit-core's values (its ProvisionApplicationRuntime, via
# ensureRuntimeStream: 14 d, 1,000,000 messages), which that provisioner converges on
# every run. The two must agree or each undoes the other, so an existing stream with
# other values is converged in place with stream edit. Nothing is deleted.
# name subjects
for spec in \
  "VEREVON_APPLICATION verevon.application.>" \
  "VEREVON_INGESTION verevon.ingestion.>" \
  "VEREVON_MODEL verevon.model.>"; do
  name=$(echo "$spec" | cut -d' ' -f1); subj=$(echo "$spec" | cut -d' ' -f2)
  if $N stream info "$name" >/dev/null 2>&1; then
    info=$($N stream info "$name" --json 2>/dev/null) || info=""
    if json_has "$info" max_age 1209600000000000 && json_has "$info" max_msgs 1000000; then
      echo "  = $name (exists)"
    else
      $N stream edit "$name" --max-age=336h --max-msgs=1000000 --force >/dev/null
      echo "  ~ $name (max-age 336h, max-msgs 1000000)"
    fi
  else
    $N stream add "$name" --subjects="$subj" --storage=file --retention=limits \
      --discard=old --max-msgs=1000000 --max-bytes=-1 --max-age=336h --dupe-window=2m \
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
# For a durable no other owner converges (see add_shared_consumer below).
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
# Shared durables: audit-core's Control provisioner (ProvisionApplicationRuntime,
# applicationPushConsumer) converges these with ack-wait 30 s and max-deliver 5, and
# refuses an existing durable whose max-deliver or ack-wait differs. So they are
# created with those values here too, and an existing one is converged in place with
# consumer edit. A durable is never deleted and re-added: that loses its delivery
# position. This script does not create notification-core-conversation-followed-message
# (the Control provisioner does, and the host script creates it with the same values).
add_shared_consumer() {
  if $N consumer info "$1" "$2" >/dev/null 2>&1; then
    info=$($N consumer info "$1" "$2" --json 2>/dev/null) || info=""
    if json_has "$info" max_deliver 5 && json_has "$info" ack_wait 30000000000; then
      echo "  = $1/$2 (exists)"
    else
      $N consumer edit "$1" "$2" --max-deliver=5 --wait=30s --force >/dev/null
      echo "  ~ $1/$2 (max-deliver 5, ack-wait 30s)"
    fi
  else
    $N consumer add "$1" "$2" --filter="$3" --target="$4" --deliver-group="$2" \
      --ack=explicit --wait=30s --max-deliver=5 --deliver=all --replay=instant \
      --defaults >/dev/null
    echo "  + $1/$2"
  fi
}
add_audit_consumer audit
add_audit_consumer usage
add_shared_consumer VEREVON_INGESTION   conversation-core-webhook-received       verevon.ingestion.integration.webhook_received      _VEREVON.APPLICATION.DELIVER.conversation.webhook-received
add_shared_consumer VEREVON_MODEL       conversation-core-model-action-proposed  verevon.model.action.proposed                        _VEREVON.APPLICATION.DELIVER.conversation.model-action-proposed
add_shared_consumer VEREVON_APPLICATION conversation-core-ai-action-executor     verevon.application.conversation.ai_action.reviewed  _VEREVON.APPLICATION.DELIVER.conversation.ai-action-reviewed
add_shared_consumer VEREVON_APPLICATION insight-core-metric-subscriber           "verevon.application.>"                              _VEREVON.APPLICATION.DELIVER.insight.metrics
# No Control owner: insight-core-ingestion-subscriber keeps the defaults.
add_consumer VEREVON_INGESTION  insight-core-ingestion-subscriber        "verevon.ingestion.>"                                _VEREVON.APPLICATION.DELIVER.insight.ingestion

echo "[jetstream-provisioner] complete"
