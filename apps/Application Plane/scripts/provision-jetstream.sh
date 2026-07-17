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
# Cross-plane note: Ingestion's integration-corev2 publishes velion.ingestion.*
# into VELION_INGESTION here (per-user "ingestion-integration-publisher"), so the
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
  "VELION_APPLICATION:velion.application.>" \
  "VELION_INGESTION:velion.ingestion.>" \
  "VELION_MODEL:velion.model.>" \
  "VELION_AUDIT:velion.audit.>"; do
  name=${spec%%:*}; subj=${spec##*:}
  if nb "stream info $name" | grep -q "Information for Stream"; then
    echo "  = $name (exists)"
  else
    nb "stream add $name --subjects='$subj' --storage=file --retention=limits \
      --discard=old --max-msgs=-1 --max-bytes=-1 --max-age=72h --dupe-window=2m \
      --replicas=1 --defaults" >/dev/null && echo "  + $name ($subj)"
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
VELION_INGESTION|conversation-core-webhook-received|velion.ingestion.integration.webhook_received|_VELION.APPLICATION.DELIVER.conversation.webhook-received
VELION_MODEL|conversation-core-model-action-proposed|velion.model.action.proposed|_VELION.APPLICATION.DELIVER.conversation.model-action-proposed
VELION_APPLICATION|conversation-core-ai-action-executor|velion.application.conversation.ai_action.reviewed|_VELION.APPLICATION.DELIVER.conversation.ai-action-reviewed
VELION_APPLICATION|insight-core-metric-subscriber|velion.application.>|_VELION.APPLICATION.DELIVER.insight.metrics
CONSUMERS

echo "done."
