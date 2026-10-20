#!/usr/bin/env bash
# browser-broker end-to-end run (see e2e/README.md).
#
# Builds the broker image from the CURRENT tree, starts a throwaway Postgres and
# a stub identity provider on a private docker network, starts the broker
# against them, runs the Go test client in this directory (build tag e2e) and
# removes everything it created, even on failure.
#
# Everything it creates is named e2e-*-<suffix>. It never touches another
# container, network or volume, reads no .env ledger and uses no real
# credential: the signing key and the database password are generated here.
#
#   E2E_LOCK_CMD   optional command prefix that serialises docker use between
#                  agents, for example:  "bash <tools>/with-lock.sh docker 2700 --"
#   E2E_SKIP_BUILD=1   reuse the image browser-broker-e2e:test as it is
#   E2E_KEEP=1         leave the containers running (for debugging; clean up by hand)
set -uo pipefail

if [ -n "${E2E_LOCK_CMD:-}" ] && [ -z "${E2E_LOCKED:-}" ]; then
  # shellcheck disable=SC2086
  exec $E2E_LOCK_CMD env E2E_LOCKED=1 bash "$0" "$@"
fi
export MSYS_NO_PATHCONV=1

HERE="$(cd "$(dirname "$0")" && pwd)"
BB="$(cd "$HERE/.." && pwd)"          # services/browser-broker
GODIR="$(cd "$BB/../.." && pwd)"      # apps/Model Plane/go (the Docker build context)
SFX="$$-$RANDOM"
NET="e2e-net-$SFX"
PG="e2e-pg-$SFX"
VOL="e2e-pgdata-$SFX"
IDP="e2e-idp-$SFX"
BR="e2e-broker-$SFX"
IDP_IMG="e2e-idp-img:$SFX"
BR_IMG="browser-broker-e2e:test"
ISSUER="http://e2e-idp.invalid"
PGUSER=e2e
PGDB=browser_broker
PGPW="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
W="$(mktemp -d "${TMPDIR:-/tmp}/bbe2e.XXXXXX")"
WIN() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }
LOG="$W/test.log"

cleanup() {
  local rc=$?
  if [ -n "${E2E_KEEP:-}" ]; then echo "E2E_KEEP set: leaving $BR $PG $IDP $NET $VOL in place"; return; fi
  docker rm -f -v "$BR" "$PG" "$IDP" >/dev/null 2>&1
  docker volume rm -f "$VOL" >/dev/null 2>&1
  docker network rm "$NET" >/dev/null 2>&1
  docker rmi "$IDP_IMG" >/dev/null 2>&1
  rm -rf "$W"
  local c n v
  c="$(docker ps -a --format '{{.Names}}' 2>/dev/null | grep -c -- "-$SFX$")"
  n="$(docker network ls --format '{{.Name}}' 2>/dev/null | grep -c -- "-$SFX$")"
  v="$(docker volume ls --format '{{.Name}}' 2>/dev/null | grep -c -- "-$SFX$")"
  echo "E2E_CLEANUP leftovers containers=${c:-?} networks=${n:-?} volumes=${v:-?}"
  return $rc
}
trap cleanup EXIT
trap 'exit 130' INT TERM

fail() { echo "E2E_RUN_ERROR: $*" >&2; exit 1; }
mask() { sed "s|$PGPW|***|g"; }

command -v go >/dev/null || fail "go is not on PATH"
docker info >/dev/null 2>&1 || fail "DOCKER IS NOT ANSWERING"

# ---- 1. broker image from the current tree ----------------------------------
if [ -z "${E2E_SKIP_BUILD:-}" ]; then
  rev="$(git -C "$(WIN "$GODIR")" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  if [ -n "$(git -C "$(WIN "$GODIR")" status --porcelain -- services/browser-broker 2>/dev/null)" ]; then rev="$rev-dirty"; fi
  echo "== docker build $BR_IMG (source revision $rev)"
  (cd "$GODIR" && docker build -q -t "$BR_IMG" --build-arg "SOURCE_REVISION=$rev" \
      -f services/browser-broker/Dockerfile .) >"$W/build.log" 2>&1 || { tail -30 "$W/build.log"; fail "broker image build failed"; }
fi
docker image inspect "$BR_IMG" >/dev/null 2>&1 || fail "image $BR_IMG does not exist"

# ---- 2. run-time key, JWKS, stub IdP image ----------------------------------
mkdir -p "$W/keys" "$W/idpctx"
arch="$(docker info --format '{{.Architecture}}' 2>/dev/null)"
case "$arch" in aarch64|arm64) goarch=arm64 ;; *) goarch=amd64 ;; esac
(cd "$BB" && GOWORK=off go run -tags e2e ./e2e/cmd/idp keygen "$(WIN "$W/keys")") || fail "key generation failed"
cp "$W/keys/jwks.json" "$W/idpctx/jwks.json"
(cd "$BB" && GOWORK=off GOOS=linux GOARCH="$goarch" CGO_ENABLED=0 go build -tags e2e -trimpath -ldflags="-s -w" \
    -o "$(WIN "$W/idpctx/idp")" ./e2e/cmd/idp) || fail "stub IdP build failed"
cp "$HERE/Dockerfile.idp" "$W/idpctx/Dockerfile"
docker build -q -t "$IDP_IMG" "$(WIN "$W/idpctx")" >/dev/null 2>"$W/idpbuild.log" || { cat "$W/idpbuild.log"; fail "stub IdP image build failed"; }

# ---- 3. network, stub IdP, throwaway Postgres ---------------------------------
echo "== private network $NET, stub IdP $IDP, Postgres $PG"
docker network create "$NET" >/dev/null || fail "network create"
docker run -d --name "$IDP" --network "$NET" "$IDP_IMG" >/dev/null || fail "start the stub IdP"
for _ in $(seq 1 30); do docker logs "$IDP" 2>&1 | grep -q "idp listening" && break; sleep 1; done
docker logs "$IDP" 2>&1 | grep -q "idp listening" || fail "the stub IdP did not start"

docker volume create "$VOL" >/dev/null || fail "volume create"
docker run -d --name "$PG" --network "$NET" -v "$VOL:/var/lib/postgresql/data" \
  -e POSTGRES_USER="$PGUSER" -e POSTGRES_PASSWORD="$PGPW" -e POSTGRES_DB="$PGDB" postgres:16-alpine >/dev/null || fail "start postgres"
pg_up=""
for _ in $(seq 1 90); do
  # -h <own ip>: the init phase's temporary server listens on the socket only
  if docker exec "$PG" sh -c "pg_isready -q -h \"\$(hostname -i)\" -U $PGUSER -d $PGDB" >/dev/null 2>&1; then pg_up=1; break; fi
  sleep 1
done
[ -n "$pg_up" ] || fail "postgres did not become ready"

# ---- 4. the broker -------------------------------------------------------------
echo "== broker $BR from $BR_IMG"
docker run -d --name "$BR" --network "$NET" -p 127.0.0.1::9095 -p 127.0.0.1::8087 \
  -e "DATABASE_URL=postgresql://$PGUSER:$PGPW@$PG:5432/$PGDB?sslmode=disable" \
  -e "AUTH_CORE_JWKS_URL=http://$IDP:3000/jwks.json" -e "AUTH_CORE_ISSUER=$ISSUER" \
  "$BR_IMG" >/dev/null || fail "start the broker"
ready=""
for _ in $(seq 1 60); do
  code="$(docker exec "$BR" curl -s -o /dev/null -w '%{http_code}' --max-time 3 http://localhost:8087/readyz 2>/dev/null || true)"
  if [ "$code" = 200 ]; then ready=1; break; fi
  [ "$(docker inspect -f '{{.State.Running}}' "$BR" 2>/dev/null)" = true ] || break
  sleep 1
done
if [ -z "$ready" ]; then docker logs --tail 40 "$BR" 2>&1 | mask; fail "the broker did not become ready"; fi

# ---- 5. the Go test client ------------------------------------------------------
echo "== go test (build tag e2e)"
(cd "$BB" && \
  E2E_KEY_FILE="$(WIN "$W/keys/private.pem")" E2E_ISSUER="$ISSUER" \
  E2E_BROKER="$BR" E2E_PG="$PG" E2E_PG_USER="$PGUSER" E2E_PG_DB="$PGDB" \
  GOWORK=off go test -tags e2e -count=1 -v -timeout 25m ./e2e/ 2>&1 | tee "$LOG")
rc=${PIPESTATUS[0]}

# ---- 6. what the broker logged ---------------------------------------------------
docker logs "$BR" >"$W/broker.log" 2>&1
echo "E2E_BROKER_LOG lines=$(wc -l <"$W/broker.log" | tr -d ' ') \
host_names=$(grep -c -E 'example\.(com|org|net)' "$W/broker.log") \
bearer_tokens=$(grep -c -E 'eyJ[A-Za-z0-9_-]{10,}' "$W/broker.log") \
db_password=$(grep -c -F -- "$PGPW" "$W/broker.log")"
echo "== summary"
grep -E '^E2E_SCENARIO_OK|^(--- |=== RUN)|^(ok|FAIL|PASS)\b|\[[a-z_]+\] FAIL' "$LOG" | grep -v '=== RUN' || true
echo "E2E_RESULT rc=$rc"
exit "$rc"
