#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/rebuild-service.sh with a stub docker and a stub compose.sh:
# refusals, the rollback tag before the rebuild, a no-deps rebuild of exactly one service,
# and a failing health wait. It never talks to a real daemon.

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/rebuild-service.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/rebuild-service-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/scripts" "$TMP/bin"
cp "$SRC" "$TMP/scripts/rebuild-service.sh"
CALLS="$TMP/calls.log"

cat >"$TMP/scripts/compose.sh" <<'STUB'
#!/usr/bin/env bash
echo "compose $*" >>"$CALLS"
STUB

cat >"$TMP/bin/docker" <<'STUB'
#!/usr/bin/env bash
echo "docker $*" >>"$CALLS"
args="$*"
case "$args" in
  "ps -q"*) echo "cid-1" ;;
  "inspect --format {{.Image}} "*) echo "sha256:aaaaaaaaaaaaaaaaaaaaaaaa" ;;
  "inspect --format {{.State.Status}}"*) echo "${STUB_STATE:-running|healthy|0}" ;;
  "image inspect --format {{.Id}} "*) exit 1 ;;
  "image inspect --format {{index .Config.Labels"*) echo "deadbeefcafe0000" ;;
  "image inspect --format {{.Created}}"*) echo "2026-10-23T00:00:00Z" ;;
  "tag "*) ;;
  *) echo "unexpected docker call: $args" >&2; exit 99 ;;
esac
STUB
chmod +x "$TMP/bin/docker" "$TMP/scripts/compose.sh" "$TMP/scripts/rebuild-service.sh"

export CALLS PATH="$TMP/bin:$PATH"
today="$(date -u +%Y%m%d)"
failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }
run() { : >"$CALLS"; bash "$TMP/scripts/rebuild-service.sh" "$@" >"$TMP/out" 2>&1; }

run && fail "no service name must not succeed"
run "Bad;name" && fail "a shell metacharacter in the service name must be refused"
run session-core model-gateway && fail "two services in one run must be refused"

MODEL_PLANE_PRODUCTION=1 run session-core && fail "production mode must be refused"
grep -q "refusing MODEL_PLANE_PRODUCTION=1" "$TMP/out" || fail "the production refusal must say why"
MODEL_PLANE_PRODUCTION=1 run --dry-run session-core && fail "production mode must be refused in a dry run too"

run --dry-run session-core || fail "dry run must succeed"
[[ ! -s "$CALLS" ]] || fail "a dry run must not call docker or compose"

run --env-file /abs/live.env session-core || { cat "$TMP/out" >&2; fail "rebuild with a healthy container must succeed"; }
grep -q "^docker tag sha256:aaaaaaaaaaaaaaaaaaaaaaaa model-plane-session-core:pre-$today-deadbeef$" "$CALLS" ||
  fail "the running image must be tagged model-plane-<service>:pre-<yyyymmdd>-<revision>"
grep -q "^compose --env-file /abs/live.env up -d --no-deps --build session-core$" "$CALLS" ||
  fail "compose must be called with up -d --no-deps --build for the one service"
tag_line="$(grep -n '^docker tag ' "$CALLS" | head -1 | cut -d: -f1)"
compose_line="$(grep -n '^compose ' "$CALLS" | head -1 | cut -d: -f1)"
[[ -n "$tag_line" && -n "$compose_line" && "$tag_line" -lt "$compose_line" ]] || fail "the rollback tag must exist before the rebuild"
grep -q "roll back with: docker tag model-plane-session-core:pre-$today-deadbeef" "$TMP/out" || fail "the rollback command must be printed"

STUB_STATE="exited||0" run --wait-seconds 4 session-core && fail "an exited container must fail the run"
grep -q "roll back with" "$TMP/out" || fail "a failed rebuild must print the rollback command"

[[ "$failed" == "0" ]] || exit 1
echo "rebuild-service contract test: ok"
