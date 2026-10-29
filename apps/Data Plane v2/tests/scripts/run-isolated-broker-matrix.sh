#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
COMPOSE_FILE="$ROOT/tests/e2e/isolated/broker-compose.yml"

for command in docker openssl cargo go jq; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

# Consumer-shape regression matrix. These focused tests exercise the same
# decoders used by the live subscribers: valid_signed, raw, forged,
# wrong_tenant, wrong_scope, replay, and restrictive zdr.
cargo test --manifest-path "$ROOT/Cargo.toml" -p event-envelope-rs --test envelope_security
cargo test --manifest-path "$ROOT/Cargo.toml" -p index-engine-rs --bin index-engine stream::signed_event_tests
cargo test --manifest-path "$ROOT/Cargo.toml" -p quickwit-adapter-rs --lib stream::tests
cargo test --manifest-path "$ROOT/Cargo.toml" -p retrieval-engine-rs --lib cache::invalidator::signed_event_tests
(
  cd "$ROOT/services/data-orchestrator-go"
  go test ./internal/cost -run 'TestSignedConsumer(RejectsUntrustedCostEvents|RejectsReplay|SuppressesPersistenceForZDR|AcceptsPinnedEmbeddingAndRetrievalProducers)$' -count=1
)

suffix=$(openssl rand -hex 6)
project="dpv2-broker-e2e-$suffix"
runtime_parent=${TMPDIR:-/tmp}
runtime_parent=${runtime_parent%/}
runtime_dir=''
export BROKER_EVENT_SIGNING_PRIVATE_KEY_PATH=''
compose=()
owned=0

cleanup() {
  rc=$?
  trap - EXIT
  if [ "$owned" -eq 1 ] && [[ "$project" == dpv2-broker-e2e-* ]] && [ "${#compose[@]}" -gt 0 ]; then
    "${compose[@]}" --profile verify --profile gdpr \
      down --volumes --remove-orphans >/dev/null 2>&1 || true
  fi
  if [[ -n "$runtime_dir" && "$runtime_dir" == "$runtime_parent"/dpv2-gdpr-broker.* ]]; then
    rm -f "$runtime_dir/documents-events.pem"
    rmdir "$runtime_dir" 2>/dev/null || true
  fi
  exit "$rc"
}

export BROKER_MATRIX_TOKEN=$(openssl rand -hex 32)
export CONTROL_NATS_ADMIN_PASSWORD=$(openssl rand -hex 32)
export DOCUMENTS_GDPR_NATS_PASSWORD=$(openssl rand -hex 32)
export BROKER_POSTGRES_PASSWORD=$(openssl rand -hex 32)
export BROKER_USER_CORE_TOKEN=$(openssl rand -hex 32)
export GDPR_TEST_ORG_ID="org-$(openssl rand -hex 8)"
export GDPR_TEST_OWNER_OK="owner-$(openssl rand -hex 8)"
export GDPR_TEST_OWNER_RETRY="owner-$(openssl rand -hex 8)"
runtime_dir=$(mktemp -d "$runtime_parent/dpv2-gdpr-broker.XXXXXX")
trap cleanup EXIT
case "$runtime_dir" in
  "$runtime_parent"/dpv2-gdpr-broker.*) ;;
  *) echo "refusing unexpected GDPR broker runtime directory" >&2; exit 2 ;;
esac
chmod 700 "$runtime_dir"
export BROKER_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/documents-events.pem"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$BROKER_EVENT_SIGNING_PRIVATE_KEY_PATH" >/dev/null 2>&1
chmod 600 "$BROKER_EVENT_SIGNING_PRIVATE_KEY_PATH"
compose=(docker compose --project-name "$project" -f "$COMPOSE_FILE")

case "$project" in
  dpv2-broker-e2e-*) ;;
  *) echo "refusing non-isolated project name" >&2; exit 2 ;;
esac

existing=$(docker ps -aq --filter "label=com.docker.compose.project=$project")
existing+=$(docker volume ls -q --filter "label=com.docker.compose.project=$project")
existing+=$(docker network ls -q --filter "label=com.docker.compose.project=$project")
[ -z "$existing" ] || { echo "refusing isolated broker project collision" >&2; exit 2; }

owned=1
"${compose[@]}" config --quiet
"${compose[@]}" up -d --wait --wait-timeout 60 nats
"${compose[@]}" --profile verify run --rm broker-probe

"${compose[@]}" --profile gdpr up -d --build --wait --wait-timeout 180 documents-gdpr
BROKER_COMPOSE_PROJECT="$project" \
BROKER_COMPOSE_FILE="$COMPOSE_FILE" \
  bash "$ROOT/tests/e2e/gdpr-durable-consumer-matrix.sh"

echo "PASS: signed broker security and disposable delivery matrices"
