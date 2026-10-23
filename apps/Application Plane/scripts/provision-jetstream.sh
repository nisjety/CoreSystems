#!/usr/bin/env bash
set -euo pipefail

# Provision the Application-Plane JetStream topology on app-nats.
#
# App-Plane services (conversation-core, insight-core, …) use BindProvisioned =
# nats.Bind(stream,durable) — they bind to PRE-EXISTING streams + durable push
# consumers and never create them (their per-user NATS creds can't). This script
# creates that topology with the observability-provisioner-application principal.
# Idempotent: re-running is safe (skips anything already present). Run after a
# fresh app-nats volume (the JetStream state lives in the app-nats-data volume;
# a container recreate keeps it, a volume wipe loses it).
#
#   apps/Application Plane> ./scripts/provision-jetstream.sh
#
# Cross-plane note: Ingestion's integration-corev2 publishes verevon.ingestion.*
# into VEREVON_INGESTION here (per-user "ingestion-integration-publisher"), so the
# conversation-core webhook consumer receives third-party integration events.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
secrets="${APPLICATION_PLANE_SECRETS_FILE:-$root/.env.generated-secrets}"
pw=$(grep -hE '^APPLICATION_NATS_PROVISIONER_PASSWORD=' "$secrets" | head -1 | cut -d= -f2-)
[[ -n "$pw" ]] || { echo "APPLICATION_NATS_PROVISIONER_PASSWORD not found in $secrets" >&2; exit 1; }

net="${APPLICATION_NET:-app-net}"
srv="nats://observability-provisioner-application:${pw}@app-nats:4222"
# The provisioner principal may only reply on this inbox prefix (see nats.conf).
ip="_INBOX.PROVISIONER_APPLICATION"

nb() { docker run --rm --network "$net" natsio/nats-box:latest \
  sh -c "nats --server '$srv' --inbox-prefix='$ip' --timeout=20s $*" 2>&1; }

echo "== streams =="
# name:subjects
for spec in \
  "VEREVON_APPLICATION:verevon.application.>" \
  "VEREVON_INGESTION:verevon.ingestion.>" \
  "VEREVON_MODEL:verevon.model.>"; do
  name=${spec%%:*}; subj=${spec##*:}
  if nb "stream info $name" | grep -q "Information for Stream"; then
    echo "  = $name (exists)"
  else
    nb "stream add $name --subjects='$subj' --storage=file --retention=limits \
      --discard=old --max-msgs=-1 --max-bytes=-1 --max-age=72h --dupe-window=2m \
      --replicas=1 --defaults" >/dev/null && echo "  + $name ($subj)"
  fi
done

echo "== audit-core stream (plane application) =="
# Same topology as scripts/jetstream-provision-entrypoint.sh (see there for the
# why); scripts/tests/audit-stream-contract-test.sh pins both. A failure here
# stops the script: an audit stream that silently does not exist loses events.
audit_stream=VEREVON_CONTROL_OBSERVABILITY
if nb "stream info VEREVON_AUDIT" | grep -q 'verevon\.audit\.>'; then
  nb "stream edit VEREVON_AUDIT --subjects='verevon.audit.retired.>' --force" >/dev/null \
    || { echo "could not narrow VEREVON_AUDIT" >&2; exit 1; }
  echo "  ~ VEREVON_AUDIT (narrowed to verevon.audit.retired.>)"
fi
if nb "stream info $audit_stream" | grep -q "Information for Stream"; then
  echo "  = $audit_stream (exists)"
else
  nb "stream add $audit_stream \
    --subjects='verevon.audit.v1.application.>,verevon.usage.v1.application.>,verevon.audit.v2.application.>,verevon.usage.v2.application.>,verevon.dlq.audit-core.>' \
    --storage=file --retention=limits --discard=old --max-msgs=2000000 --max-bytes=-1 \
    --max-age=720h --dupe-window=2m --replicas=1 --defaults" >/dev/null \
    || { echo "could not create $audit_stream" >&2; exit 1; }
  echo "  + $audit_stream"
fi
for kind in audit usage; do
  durable="audit-core-application-v3-$kind"
  if nb "consumer info $audit_stream $durable" | grep -q "Information for Consumer"; then
    echo "  = $audit_stream/$durable (exists)"
  else
    nb "consumer add $audit_stream $durable --filter='verevon.$kind.v2.application.>' \
      --target='_VEREVON.AUDIT.DELIVER.application.$kind-v2' --deliver-group='$durable' \
      --ack=explicit --wait=30s --max-deliver=5 --deliver=all --replay=instant --defaults" >/dev/null \
      || { echo "could not create $audit_stream/$durable" >&2; exit 1; }
    echo "  + $audit_stream/$durable"
  fi
done

echo "== consumers (stream durable filter deliver) =="
# stream|durable|filter|deliver-subject   (deliver-group == durable)
while IFS='|' read -r stream durable filter deliver; do
  [[ -z "$stream" ]] && continue
  if nb "consumer info $stream $durable" | grep -q "Information for Consumer"; then
    echo "  = $stream/$durable (exists)"
  else
    nb "consumer add $stream $durable --filter='$filter' --target='$deliver' \
      --deliver-group='$durable' --ack=explicit --deliver=all --replay=instant \
      --defaults" >/dev/null && echo "  + $stream/$durable"
  fi
done <<'CONSUMERS'
VEREVON_INGESTION|conversation-core-webhook-received|verevon.ingestion.integration.webhook_received|_VEREVON.APPLICATION.DELIVER.conversation.webhook-received
VEREVON_MODEL|conversation-core-model-action-proposed|verevon.model.action.proposed|_VEREVON.APPLICATION.DELIVER.conversation.model-action-proposed
VEREVON_APPLICATION|conversation-core-ai-action-executor|verevon.application.conversation.ai_action.reviewed|_VEREVON.APPLICATION.DELIVER.conversation.ai-action-reviewed
VEREVON_APPLICATION|insight-core-metric-subscriber|verevon.application.>|_VEREVON.APPLICATION.DELIVER.insight.metrics
VEREVON_INGESTION|insight-core-ingestion-subscriber|verevon.ingestion.>|_VEREVON.APPLICATION.DELIVER.insight.ingestion
VEREVON_APPLICATION|notification-core-conversation-followed-message|verevon.application.conversation.message.received|_VEREVON.APPLICATION.DELIVER.notification.conversation-followed-message
CONSUMERS

echo "done."
