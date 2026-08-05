#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
runner="$root/tests/scripts/run-isolated-broker-matrix.sh"
compose="$root/tests/e2e/isolated/broker-compose.yml"
broker_config="$root/tests/e2e/isolated/control-shared-nats.conf"
setup="$root/tests/e2e/isolated/gdpr-control-broker-setup.sh"
matrix="$root/tests/e2e/gdpr-durable-consumer-matrix.sh"

for file in "$runner" "$compose" "$broker_config" "$setup" "$matrix"; do
  [ -f "$file" ] || { echo "FAIL: missing isolated GDPR broker artifact" >&2; exit 1; }
done
for executable in "$runner" "$setup" "$matrix"; do
  [ -x "$executable" ] || { echo "FAIL: isolated GDPR broker executable is not executable" >&2; exit 1; }
done

bash -n "$runner"
sh -n "$setup"
bash -n "$matrix"

for service in control-nats gdpr-data-nats control-init gdpr-postgres documents-gdpr control-probe; do
  rg -Fq "$service:" "$compose"
done

rg -Fq 'documents-api-gdpr' "$broker_config"
rg -Fq '_INBOX.DOCUMENTS_GDPR.>' "$broker_config"
rg -Fq '_VEREVON.CONTROL.SHARED.DELIVER.data.documents-api.gdpr-erasure' "$broker_config"
rg -Fq '$JS.ACK.AQENCIA_CONTROLPLANE.documents-api-gdpr-erasure-v1.>' "$broker_config"
! rg -Fq '$JS.ACK.AQENCIA_CONTROLPLANE.>' "$broker_config"
rg -Fq '$JS.API.CONSUMER.INFO.AQENCIA_CONTROLPLANE.documents-api-gdpr-erasure-v1' "$broker_config"
rg -Fq 'AQENCIA_CONTROLPLANE' "$setup"
rg -Fq 'documents-api-gdpr-erasure-v1' "$setup"
rg -Fq -- '--max-deliver=20' "$setup"
rg -Fq -- '--ack=explicit' "$setup"
rg -Fq -- '--deny-delete' "$setup"
rg -Fq -- '--deny-purge' "$setup"

rg -Fq '/internal/gdpr/health' "$matrix"
rg -Fq 'verevon.gdpr.erasure.requested' "$matrix"
rg -Fq 'docker compose' "$matrix"
rg -Fq 'stop -t' "$matrix"
rg -Fq 'num_redelivered' "$matrix"
rg -Fq 'num_ack_pending' "$matrix"
rg -Fq -- '--inbox-prefix=_INBOX.DOCUMENTS_GDPR' "$matrix"
rg -Fq 'org-system-account' "$matrix"
rg -Fq 'refusing non-isolated GDPR broker project' "$matrix"
rg -Fq 'refusing unexpected GDPR broker compose file' "$matrix"
rg -Fq '$JS.ACK.AQENCIA_CONTROLPLANE.other-consumer.' "$matrix"
rg -Fq 'documents principal ACKed a different durable' "$matrix"
rg -Fq 'GDPR_DURABLE_CONSUMER_REQUIRED: "1"' "$compose"
rg -Fq 'NATS_URL: nats://gdpr-data-nats:4222' "$compose"
rg -Fq 'start_period: 10s' "$compose"
rg -Fq "to_regclass('public.documents_outbox')" "$compose"
rg -Fq 'gdpr-durable-consumer-matrix.sh' "$runner"
rg -Fq 'rsa_keygen_bits:2048' "$runner"
rg -Fq 'trap cleanup EXIT' "$runner"
rg -Fq -- '--profile verify --profile gdpr' "$runner"

! rg -qi 'stream[[:space:]]+(purge|delete)|consumer[[:space:]]+delete|delete[[:space:]]+from[[:space:]]+documents' "$runner" "$setup" "$matrix"

echo "PASS: real documents GDPR durable broker contract"
