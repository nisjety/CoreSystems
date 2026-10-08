#!/usr/bin/env bash
# =============================================================================
# graph-index-no-azure-contract-test.sh  (design task CR-T12)
#
# graph-index reaches a model through inference-core and nowhere else. This contract
# fails when a way to reach one directly comes back.
#
# WHY IT EXISTS
#   graph-index used to carry a second extraction backend, selected by ONE variable
#   (GRAPH_EXTRACTION_PROVIDER=azure_openai), that called Azure OpenAI directly with
#   a key held in the container. That path sent no Auth Core service token, no
#   retention-posture claim, no usage record and no typed error: flipping one variable
#   bypassed the token, the zero-retention guarantee, the metering and the errors.
#   The backend is deleted from the code (graph-index-rs/src/extractor.rs, and a unit
#   test there refuses a path that mentions an HTTP client or an Azure key) and the
#   key and the endpoint are gone from the container's environment. This script keeps
#   the deployment side honest.
#
#   It is a sibling of compose-security-contract-test.sh, which belongs to other tasks
#   and is not edited here.
#
# WHAT IT CHECKS (every check names what it protects)
#   1. The graph-index service of the Data Plane compose sets none of
#      GRAPH_EXTRACTION_PROVIDER, AZURE_OPENAI_API_KEY, AZURE_OPENAI_ENDPOINT or
#      AZURE_OPENAI_EXTRACTION_DEPLOYMENT, and mounts no Azure credential file.
#   2. It does set the inference-core hop: the gRPC endpoint, the Auth Core token
#      URL, issuer, service id, credential and retention posture (and the credential
#      has no inline default).
#   3. No other service of the compose, and no isolated end-to-end overlay, selects
#      the removed backend.
#   4. graph-index's usage-event signing key is its own: the private key is mounted into
#      graph-index and into no other service, and the switch is off by default.
#   5. The poison cap, the background budget and community summaries have their
#      documented defaults (cap 6, alert-only, no figure, no prices, summaries off).
#
# Run from anywhere:  bash tests/scripts/graph-index-no-azure-contract-test.sh
# Exit status: 0 the contract holds, 1 it does not.
# =============================================================================
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
data_root="$root_dir/apps/Data Plane v2"
compose="${GRAPH_NO_AZURE_COMPOSE:-$data_root/docker-compose.yml}"

failures=0
fail() { printf 'FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }

[ -f "$compose" ] || { printf 'FAIL: %s is missing\n' "$compose" >&2; exit 1; }

# The text of the compose without CR (a Windows checkout) and without comment lines: a
# sentence that explains why a variable is gone must not trip the check that it is.
active() { tr -d '\r' <"$1" | sed -E '/^[[:space:]]*#/d'; }

# The `graph-index:` service block: from its key to the next service key (two-space indent).
service_block() {
  active "$compose" | awk -v name="$1" '
    $0 ~ "^  " name ":[[:space:]]*$" { inside = 1; print; next }
    inside && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { exit }
    inside { print }'
}

graph="$(service_block graph-index)"
[ -n "$graph" ] || { echo "FAIL: no graph-index service in $compose" >&2; exit 1; }

# 1. No way to a provider that is not inference-core.
for forbidden in GRAPH_EXTRACTION_PROVIDER AZURE_OPENAI_API_KEY AZURE_OPENAI_ENDPOINT AZURE_OPENAI_EXTRACTION_DEPLOYMENT AZURE_OPENAI; do
  if printf '%s\n' "$graph" | grep -Eq "(^|[^A-Z_])${forbidden}([^A-Z_]|\$)"; then
    fail "graph-index sets $forbidden: the direct-Azure backend is deleted, and a key in the container is a way around the service token, the retention posture and the metering"
  fi
done
if printf '%s\n' "$graph" | grep -Eiq 'azure[^[:space:]]*(key|secret|credential)|openai\.azure\.com|\.cognitiveservices\.'; then
  fail "graph-index carries an Azure credential or endpoint"
fi
if printf '%s\n' "$graph" | grep -Eq '^[[:space:]]*-[[:space:]].*(azure|openai)[^[:space:]]*:/'; then
  fail "graph-index mounts an Azure credential file"
fi

# 2. The inference-core hop is configured.
for required in \
  'MODEL_PLANE_AI_CORE_GRPC_URL:' \
  'MODEL_PLANE_INFERENCE_TOKEN_URL:' \
  'MODEL_PLANE_INFERENCE_TOKEN_ISSUER:' \
  'MODEL_PLANE_INFERENCE_SERVICE_ID:.*graph-index' \
  'MODEL_PLANE_INFERENCE_SERVICE_API_KEY:' \
  'MODEL_PLANE_INFERENCE_RETENTION_POSTURE: \$\{MODEL_PLANE_INFERENCE_RETENTION_POSTURE:-(persistent|zdr|tenant)\}'; do
  printf '%s\n' "$graph" | grep -Eq "$required" || fail "graph-index does not set $required"
done
# The credential is deployment-owned: no inline default value.
if printf '%s\n' "$graph" | grep -Eq '^[[:space:]]*MODEL_PLANE_INFERENCE_SERVICE_API_KEY:[[:space:]]*[^$[:space:]]'; then
  fail "graph-index's inference credential has an inline value"
fi
if printf '%s\n' "$graph" | grep -Eq 'MODEL_PLANE_INFERENCE_SERVICE_API_KEY:.*:-[^}]'; then
  fail "graph-index's inference credential has a default"
fi

# 3. Nobody else selects the removed backend, in the compose or in an isolated overlay.
selectors="$(active "$compose" | grep -c 'GRAPH_EXTRACTION_PROVIDER' || true)"
[ "$selectors" -eq 0 ] || fail "the compose still names GRAPH_EXTRACTION_PROVIDER ($selectors lines)"
while IFS= read -r overlay; do
  [ -n "$overlay" ] || continue
  if active "$overlay" | grep -Eq 'GRAPH_EXTRACTION_PROVIDER:[[:space:]]*"?azure'; then
    fail "$overlay selects the removed direct-Azure graph backend"
  fi
done < <(find "$data_root/tests" "$data_root" -maxdepth 4 \( -name '*compose*.yml' -o -name '*compose*.yaml' \) -not -path '*/node_modules/*' -not -path '*/target/*' 2>/dev/null | sort -u)

# 4. graph-index's usage-event signing key is its alone.
private_mounts="$(active "$compose" | grep -c 'graph-events\.pem:ro' || true)"
[ "$private_mounts" -eq 1 ] || fail "the graph signing key is mounted on $private_mounts lines of the compose, want exactly one (the graph-index mount)"
private_paths="$(active "$compose" | grep -c '/run/event-keys/graph-events\.pem' || true)"
[ "$private_paths" -eq 2 ] || fail "the container path of the graph signing key is named on $private_paths lines of the compose, want two (the mount and graph-index's GRAPH_EVENT_PRIVATE_KEY_PATH)"
printf '%s\n' "$graph" | grep -Eq 'graph-events\.pem:ro' || fail "graph-index does not mount its own usage-event signing key read-only"
printf '%s\n' "$graph" | grep -Eq 'GRAPH_EVENT_PRIVATE_KEY_PATH: /run/event-keys/graph-events\.pem' || fail "graph-index does not name its signing key path"
printf '%s\n' "$graph" | grep -Eq 'USAGE_EMIT_ENABLED: \$\{USAGE_EMIT_ENABLED:-0\}' || fail "the usage ledger is not off by default in graph-index"
other_private="$(active "$compose" | grep -E 'GRAPH_EVENT_PRIVATE_KEY_PATH' | grep -vc 'graph-events\.pem' || true)"
[ "$other_private" -eq 0 ] || fail "another service names the graph signing key"

# 5. The documented defaults of what CR-T12 added.
for expected in \
  'GRAPH_EXTRACTION_MAX_ATTEMPTS: \$\{GRAPH_EXTRACTION_MAX_ATTEMPTS:-6\}' \
  'BACKGROUND_BUDGET_MODE: \$\{BACKGROUND_BUDGET_MODE:-alert_only\}' \
  'BACKGROUND_BUDGET_MONTHLY_USD: \$\{BACKGROUND_BUDGET_MONTHLY_USD:-\}' \
  'BACKGROUND_PRICE_INPUT_USD_PER_MTOK: \$\{BACKGROUND_PRICE_INPUT_USD_PER_MTOK:-\}' \
  'BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK: \$\{BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK:-\}' \
  'COMMUNITY_SUMMARY_ENABLED: \$\{COMMUNITY_SUMMARY_ENABLED:-0\}'; do
  printf '%s\n' "$graph" | grep -Eq "$expected" || fail "graph-index does not carry the documented default $expected"
done

if [ "$failures" -ne 0 ]; then
  printf 'graph-index no-Azure contract: %d check(s) failed\n' "$failures" >&2
  exit 1
fi
echo "PASS: graph-index reaches a model through inference-core only"
