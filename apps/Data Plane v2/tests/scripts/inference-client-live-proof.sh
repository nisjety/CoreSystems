#!/usr/bin/env bash
# Live proof for the Data Plane inference client (CR-T5). FREE: it mints bearers and
# reads logs, it never calls a model. Run it after the embedding-engine and graph-index
# images carrying services/inference-client-rs are deployed through the launcher stage
# copies (never a hand-built `docker compose`).
#
# It prints statuses, the posture, and three claims (zdr, ttl, reason). It never prints
# a token or a credential: the credential is read by curl inside the container from the
# container's own environment, and only the decoded claims leave it.
#
#   ORG_ID=<live organization id> bash tests/scripts/inference-client-live-proof.sh
#   ORG_ID=<zdr test organization id> EXPECT_POSTURE=tenant EXPECT_ZDR=true bash ...
#
# Variables
#   ORG_ID               required. A live organization (the registry binds these principals to
#                        the organization directory, so an unknown id is a 403).
#   EXPECT_POSTURE       persistent | zdr | tenant. What the services were started with
#                        (default persistent until the enablement step, then tenant).
#   EXPECT_ZDR           true | false. What the organization's token must carry (default
#                        false). With the persistent posture this is always false; with
#                        tenant it is the organization's own ZDR setting.
#   EMBEDDING_ENGINE_CONTAINER, GRAPH_INDEX_CONTAINER   container names.
#
# Pass criteria (each is checked and the script exits 1 on the first miss)
#   1. The startup line `inference token client ready` is in each service's log and says
#      the posture the service was configured with. (Needs RUST_LOG to include
#      inference_client_rs=info, which the compose default does. A RUST_LOG in the
#      git-ignored .env replaces it: add the target there.)
#   2. A mint for ORG_ID succeeds for every reason the two services use (HTTP 201),
#      the token's audience-bound claims carry the reason that was asked for, `zdr` is
#      EXPECT_ZDR, and the lifetime is 60 s under tenant and 300 s otherwise.
#   3. The cache, from the logs of a real run (CR-T7/T8/T12 live proofs make the calls;
#      this script makes none): `inference bearer minted` appears ONCE per
#      (organization, reason) within the 45 s reuse window however many chunks were sent.
#      Printed here as a count for the last 10 minutes; compare it with the number of
#      provider calls of the run.
set -euo pipefail

: "${ORG_ID:?set ORG_ID to a live organization id}"
ENGINE="${EMBEDDING_ENGINE_CONTAINER:-data-plane-v2-embedding-engine-1}"
GRAPH="${GRAPH_INDEX_CONTAINER:-data-plane-v2-graph-index-1}"
EXPECT_POSTURE="${EXPECT_POSTURE:-persistent}"
EXPECT_ZDR="${EXPECT_ZDR:-false}"
case "$EXPECT_POSTURE" in
  persistent) EXPECT_TTL=300 ;;
  zdr) EXPECT_TTL=300 ;;
  tenant) EXPECT_TTL=60 ;;
  *) echo "EXPECT_POSTURE must be persistent, zdr or tenant" >&2; exit 2 ;;
esac

fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

# 1. the posture each service started with
for container in "$ENGINE" "$GRAPH"; do
  line="$(docker logs "$container" 2>&1 | grep 'inference token client ready' | tail -1 || true)"
  [ -n "$line" ] || fail "$container logged no 'inference token client ready' line (is RUST_LOG missing inference_client_rs=info?)"
  printf '%s: %s\n' "$container" "$line"
  case "$line" in
    *"posture=\"$EXPECT_POSTURE\""*|*"posture=$EXPECT_POSTURE"*) ;;
    *) fail "$container started with a posture other than $EXPECT_POSTURE" ;;
  esac
  case "$line" in
    *"configured=true"*) ;;
    *) fail "$container has no service credential (configured=false)" ;;
  esac
done

# 2. a mint per (service, reason), through the container's own credential
probe() {
  local container="$1" reason="$2" out status zdr ttl got_reason
  out="$(docker exec -e ORG_ID="$ORG_ID" -e REASON="$reason" "$container" sh -c '
    set -eu
    body="$(mktemp)"
    code="$(curl -sS -o "$body" -w "%{http_code}" -X POST "$MODEL_PLANE_INFERENCE_TOKEN_URL" \
      -H "x-service-id: $MODEL_PLANE_INFERENCE_SERVICE_ID" \
      -H "x-service-api-key: $MODEL_PLANE_INFERENCE_SERVICE_API_KEY" \
      -H "content-type: application/json" \
      -d "{\"orgId\":\"$ORG_ID\",\"scopes\":[\"inference:invoke\"],\"reason\":\"$REASON\"}")"
    echo "status=$code"
    case "$code" in
      200|201)
        token="$(sed -n "s/.*\"token\":\"\([^\"]*\)\".*/\1/p" "$body")"
        payload="$(printf %s "$token" | cut -d. -f2 | tr "_-" "/+")"
        pad=$(( (4 - ${#payload} % 4) % 4 )); i=0
        while [ "$i" -lt "$pad" ]; do payload="$payload="; i=$((i + 1)); done
        claims="$(printf %s "$payload" | base64 -d 2>/dev/null)"
        echo "zdr=$(printf %s "$claims" | grep -o "\"zdr\":[a-z]*" | cut -d: -f2)"
        iat="$(printf %s "$claims" | grep -o "\"iat\":[0-9]*" | cut -d: -f2)"
        exp="$(printf %s "$claims" | grep -o "\"exp\":[0-9]*" | cut -d: -f2)"
        echo "ttl=$((exp - iat))"
        echo "reason=$(printf %s "$claims" | grep -o "\"reason\":\"[^\"]*\"" | cut -d\" -f4)"
        ;;
      *)
        echo "error=$(sed -n "s/.*\"error\":\"\([^\"]*\)\".*/\1/p" "$body")"
        ;;
    esac
    rm -f "$body"
  ')"
  status="$(printf '%s\n' "$out" | sed -n 's/^status=//p')"
  zdr="$(printf '%s\n' "$out" | sed -n 's/^zdr=//p')"
  ttl="$(printf '%s\n' "$out" | sed -n 's/^ttl=//p')"
  got_reason="$(printf '%s\n' "$out" | sed -n 's/^reason=//p')"
  printf '%s  reason=%s -> status=%s zdr=%s ttl_s=%s reason_claim=%s %s\n' \
    "$container" "$reason" "$status" "${zdr:--}" "${ttl:--}" "${got_reason:--}" \
    "$(printf '%s\n' "$out" | sed -n 's/^error=/error=/p')"
  [ "$status" = 201 ] || [ "$status" = 200 ] || fail "$container could not mint for $reason (status $status)"
  [ "$zdr" = "$EXPECT_ZDR" ] || fail "$container: zdr claim is $zdr, expected $EXPECT_ZDR"
  [ "$ttl" = "$EXPECT_TTL" ] || fail "$container: token lifetime is $ttl s, expected $EXPECT_TTL"
  [ "$got_reason" = "$reason" ] || fail "$container: reason claim is '$got_reason', expected '$reason'"
}

for reason in document-embedding contextual-retrieval video-caption; do probe "$ENGINE" "$reason"; done
for reason in graph-extraction graph-community-summary; do probe "$GRAPH" "$reason"; done

# 3. the cache, as far as a free run can show it
for container in "$ENGINE" "$GRAPH"; do
  minted="$(docker logs --since 10m "$container" 2>&1 | grep -c 'inference bearer minted' || true)"
  printf '%s: %s "inference bearer minted" line(s) in the last 10 minutes (compare with the provider calls of the run)\n' "$container" "$minted"
done

echo "PASS: inference client live proof (posture $EXPECT_POSTURE, zdr $EXPECT_ZDR, ttl ${EXPECT_TTL}s)"
