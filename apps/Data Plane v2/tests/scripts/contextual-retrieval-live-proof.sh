#!/usr/bin/env bash
# Live proof of contextualizer v2 (CR-T7). PAID, and tiny: four gpt-4o-mini calls on
# one synthetic document of an invented company, about 0.002 USD. Run it ONCE, after
# the embedding-engine image carrying services/embedding-engine-rs/src/context is
# deployed through the launcher stage copies (never a hand-built `docker compose`), and
# only against a throwaway organization (the tenant posture refuses unknown ones).
#
# It runs the ignored test `provider::contextualize::live_proof` from this checkout
# against the LIVE stack, as the service would: the real token client, the real
# inference-core, the real Azure deployment. Everything it needs is copied from the
# running embedding-engine container's own environment into this process, and none of
# it is printed: no key, no token, no URL with a credential. The test prints statuses,
# counts, token totals and the (synthetic) contexts only.
#
#   CR_LIVE_ORG_ID=<throwaway org id> CR_LIVE_PREFLIGHT_ONLY=1 bash tests/scripts/contextual-retrieval-live-proof.sh
#       FREE. Mints a bearer, reads its zdr claim, and asks inference-core's ListModels
#       whether the Azure deployment that serves the model declares a residency that
#       meets the floor. Run this first.
#   CR_LIVE_ORG_ID=<throwaway org id> CR_LIVE_WRITE_CAPTURE=1 bash tests/scripts/contextual-retrieval-live-proof.sh
#       The proof, and the real Azure answers recorded to
#       services/embedding-engine-rs/tests/fixtures/contracts/infer-response-azure-live.json
#       (with its pin), which is then replayed through the validation tests.
#
# Variables
#   CR_LIVE_ORG_ID          required. A throwaway organization, not zero-retention.
#   CR_LIVE_PREFLIGHT_ONLY  set: stop after the free preflight.
#   CR_LIVE_WRITE_CAPTURE   set: write the capture (after the proof has passed).
#   CR_LIVE_TOKEN_URL       Auth Core's token endpoint as the HOST reaches it
#                           (default http://127.0.0.1:3011/api/inference-core/internal-token).
#   CR_LIVE_GRPC_URL        inference-core's gRPC endpoint as the host reaches it
#                           (default http://127.0.0.1:9092).
#   EMBEDDING_ENGINE_CONTAINER  default data-plane-v2-embedding-engine-1.
#
# Pass criteria (the test asserts each and ends with "PASS: contextualizer v2 live proof")
#   1. The bearer's zdr claim is false for the throwaway organization.
#   2. ListModels shows the configured model served by an Azure OpenAI deployment that
#      declares a residency at least the floor (eu): so the floor can be met. A floor
#      nobody meets is the way a missing AZURE_OPENAI_REGION shows up; found here for free.
#   3. Three chunks, three `Ready` outcomes, exactly three provider calls; provider_used
#      names Azure OpenAI, residency meets the floor, model_used starts with the model.
#   4. Every accepted answer's finish reason is `stop` (or `end_turn`); the 16-token
#      probe's is `length`. These are the REAL values, recorded for the validation tests.
#   5. One request reached Auth Core for all four calls (the bearer is cached).
#   6. cache_read_input_tokens is MEASURED and printed. It is NOT a pass criterion: Azure's
#      automatic prefix cache needs a 1,024-token prefix and is best effort, so zero is a
#      result. Record the number in the proof report.
#   7. Meta openings ("This chunk ...") are counted; they fail the proof only under ctx-v2.
#   8. With CR_LIVE_WRITE_CAPTURE: the recorded file passes the validation tests under
#      REQUIRE_CONTRACT=1 (this script runs them).
#
# Say in the report how many paid calls were made: four.
set -euo pipefail
# Nothing below may echo a value. Do not add `set -x`.
set +x

: "${CR_LIVE_ORG_ID:?set CR_LIVE_ORG_ID to a throwaway organization id}"
ENGINE="${EMBEDDING_ENGINE_CONTAINER:-data-plane-v2-embedding-engine-1}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

docker inspect "$ENGINE" >/dev/null 2>&1 || { echo "FAIL: container $ENGINE is not running" >&2; exit 1; }

# The variables the service itself runs with, copied by name. Only whether each was
# found is reported.
copy_from_container() {
  local name="$1" value
  if value="$(docker exec "$ENGINE" printenv "$name" 2>/dev/null)"; then
    export "$name=$value"
    printf 'env: %s found\n' "$name"
  else
    printf 'env: %s not set in the container (the service default applies)\n' "$name"
  fi
}
for name in \
  MODEL_PLANE_INFERENCE_TOKEN_ISSUER \
  MODEL_PLANE_INFERENCE_SERVICE_ID \
  MODEL_PLANE_INFERENCE_SERVICE_API_KEY \
  MODEL_PLANE_INFERENCE_RETENTION_POSTURE \
  CONTEXTUAL_RETRIEVAL_MODEL \
  CONTEXTUAL_RETRIEVAL_PROVIDER_HINT \
  CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY \
  CONTEXTUAL_RETRIEVAL_PROMPT_VERSION \
  CONTEXTUAL_RETRIEVAL_WINDOW_CHARS \
  CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS \
  CONTEXTUAL_RETRIEVAL_MAX_TOKENS \
  CONTEXTUAL_RETRIEVAL_TIMEOUT_MS \
  CONTEXTUAL_RETRIEVAL_CONCURRENCY \
  CONTEXTUAL_RETRIEVAL_RETRY_ATTEMPTS \
  CONTEXTUAL_RETRIEVAL_RETRY_BASE_MS \
  CONTEXT_INLINE_BUDGET_MS \
  CONTEXT_INLINE_DOCUMENT_BUDGET_MS; do
  copy_from_container "$name"
done

# The two addresses the container uses are names on the compose network; the host
# reaches the same services on their published loopback ports.
export MODEL_PLANE_INFERENCE_TOKEN_URL="${CR_LIVE_TOKEN_URL:-http://127.0.0.1:3011/api/inference-core/internal-token}"
export MODEL_PLANE_AI_CORE_GRPC_URL="${CR_LIVE_GRPC_URL:-http://127.0.0.1:9092}"
# Required by the service's configuration and never used here: nothing in this proof
# opens a database, a stream or a vector store.
export DATABASE_URL="postgres://unused.invalid/unused"
export NATS_URL="nats://unused.invalid:4222"
export QDRANT_URL="http://unused.invalid:6334"
export CONTEXTUAL_RETRIEVAL_ENABLED=true

cd "$ROOT"
echo "== running the live proof (org ${CR_LIVE_ORG_ID}) =="
cargo test -p embedding-engine-rs --bin embedding-engine contextualizer_v2_live_proof \
  -- --ignored --nocapture --test-threads=1

if [ -n "${CR_LIVE_WRITE_CAPTURE:-}" ] && [ -z "${CR_LIVE_PREFLIGHT_ONLY:-}" ]; then
  echo "== replaying the recorded answers through the validation tests =="
  REQUIRE_CONTRACT=1 cargo test -p embedding-engine-rs --bin embedding-engine \
    the_captured_real_azure_answers_are_accepted_or_cut_the_way_their_reason_says
  echo "Commit services/embedding-engine-rs/tests/fixtures/contracts/infer-response-azure-live.json and its .sha256."
fi
