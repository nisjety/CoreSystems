#!/bin/sh
# Provision only the streams produced by the current Studio Plane cores.
set -eu

: "${STUDIO_NATS_PROVISIONER_PASSWORD:?STUDIO_NATS_PROVISIONER_PASSWORD is required}"
export NATS_USER=studio-provisioner
export NATS_PASSWORD="$STUDIO_NATS_PROVISIONER_PASSWORD"

nats_admin() {
  nats --server nats://engagement-nats:4222 \
    --inbox-prefix _INBOX.STUDIO_PROVISIONER --timeout 20s "$@"
}

for spec in \
  "VEREVON_APPLICATION verevon.application.>" \
  "VEREVON_AUDIT verevon.audit.>"; do
  name=${spec%% *}
  subject=${spec#* }
  if nats_admin stream info "$name" >/dev/null 2>&1; then
    echo "[studio-jetstream] $name exists"
  else
    nats_admin stream add "$name" --subjects="$subject" --storage=file \
      --retention=limits --discard=old --max-msgs=-1 --max-bytes=-1 \
      --max-age=72h --dupe-window=2m --replicas=1 --defaults >/dev/null
    echo "[studio-jetstream] $name created"
  fi
done
