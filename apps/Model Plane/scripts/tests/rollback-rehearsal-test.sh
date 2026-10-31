#!/usr/bin/env bash
set -euo pipefail

# Guard and flow test for scripts/rollback-rehearsal.sh with a stub docker and stub artifact
# runners. It proves the refusals, the migration plan (additive versus contract), that the
# ignore-missing switch reaches only the rollback run that needs it, the scripted revert for a
# contract migration, and the evidence file. It never talks to a real daemon.

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/rollback-rehearsal.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/rollback-rehearsal-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# shellcheck source=/dev/null
source "$SRC"

# Defined after the source: the script has its own fail().
failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }

mkdir -p "$TMP/bin"
CALLS="$TMP/calls.log"
STDIN_LOG="$TMP/stdin.log"
cat >"$TMP/bin/docker" <<'STUB'
#!/usr/bin/env bash
echo "docker $*" >>"$CALLS"
case "$*" in
  "ps -q"*) printf '%s' "${STUB_PS:-}" ;;
  "exec -i "*) { echo "-- psql $*"; cat; } >>"$STDIN_LOG" ;;
  *"pg_isready"*) ;;
  "logs mp-rollback-rehearsal-sc-strict-"*) echo "${STUB_STRICT_LOG:-migration 3 was previously applied but is missing in the resolved migrations}" ;;
  "logs "*) echo "migrations applied" ;;
  "inspect "*) echo "running" ;;
  *) ;;
esac
STUB
chmod +x "$TMP/bin/docker"
export CALLS STDIN_LOG PATH="$TMP/bin:$PATH"

# make_artifact <dir> <revision> <migration-file>...  (a migration given as name=contract adds the marker)
make_artifact() {
  local dir="$1" revision="$2" spec name
  shift 2
  mkdir -p "$dir/migrations/session-core/revert" "$dir/scripts"
  printf 'SOURCE_REVISION=%s\nSESSION_CORE_RELEASE_IMAGE=sha256:%064d\nPOSTGRES_RELEASE_IMAGE=sha256:%064d\n' "$revision" 1 2 >"$dir/images.lock.env"
  printf 'path\tkind\tsha256\tbytes\n' >"$dir/artifact-manifest.tsv"
  for spec in "$@"; do
    name="${spec%%=*}"
    printf 'CREATE TABLE t_%s (id int);\n' "${name%%_*}" >"$dir/migrations/session-core/$name"
    if [[ "$spec" == *=contract ]]; then
      printf -- '-- contract: drops x, safe because nothing reads it\nDROP TABLE IF EXISTS x;\n' >>"$dir/migrations/session-core/$name"
      printf 'CREATE TABLE x (id int);\n' >"$dir/migrations/session-core/revert/${name%.sql}.down.sql"
    fi
  done
  cat >"$dir/scripts/release-artifact.sh" <<'STUB'
#!/usr/bin/env bash
echo "artifact $(basename "$(dirname "$(dirname "$0")")") $*" >>"$CALLS"
[[ "${STUB_FAIL_STEP:-}" != "$1" ]] || exit 1
STUB
}

# --- helpers ---------------------------------------------------------------------------------
make_artifact "$TMP/h-cand" aaaaaaa 0001_a.sql 0002_b.sql 0003_c.sql=contract 0004_d.sql
make_artifact "$TMP/h-roll" bbbbbbb 0001_a.sql 0002_b.sql
plan="$(rollback_plan "$TMP/h-cand/migrations/session-core" "$TMP/h-roll/migrations/session-core")" ||
  fail "a contract migration with a revert file must plan cleanly"
[[ "$(printf '%s\n' "$plan" | cut -f1,2 | tr '\t' ' ')" == $'contract 3\nadditive 4' ]] ||
  fail "the plan must list exactly the migrations the rollback lacks: $plan"
printf '%s\n' "$plan" | grep -q "revert/0003_c.down.sql" || fail "a contract plan line must name its revert file"

rm "$TMP/h-cand/migrations/session-core/revert/0003_c.down.sql"
plan="$(rollback_plan "$TMP/h-cand/migrations/session-core" "$TMP/h-roll/migrations/session-core")" &&
  fail "a contract migration without a revert file must fail the plan"
printf '%s\n' "$plan" | grep -q "^contract-without-revert" || fail "the missing revert must be named"

[[ "$(sqlx_description 0049_cron_fire_run_link.sql)" == "cron fire run link" ]] || fail "sqlx description"
printf 'abc' >"$TMP/sum.sql"
[[ "$(sqlx_checksum_hex "$TMP/sum.sql")" == "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7" ]] ||
  fail "sqlx checksum is SHA-384 of the file"

# --- CLI guards ------------------------------------------------------------------------------
make_artifact "$TMP/cand" aaaaaaa 0001_a.sql 0002_b.sql 0003_c.sql
make_artifact "$TMP/roll" bbbbbbb 0001_a.sql 0002_b.sql
run() { : >"$CALLS"; : >"$STDIN_LOG"; bash "$SRC" "$@" >"$TMP/out" 2>&1; }
ev="$TMP/evidence.txt"

run && fail "no arguments must not succeed"
run --dry-run "$TMP/cand" && fail "one directory must not succeed"
run --dry-run --evidence-file "$ev" "$TMP/cand" "$TMP/cand" && fail "the same artifact twice must be refused"
run --dry-run --evidence-file "$ev" "$TMP/cand" "$TMP/nope" && fail "a missing directory must be refused"
MODEL_PLANE_PRODUCTION=1 run --dry-run --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "production mode must be refused"
grep -q "refusing MODEL_PLANE_PRODUCTION=1" "$TMP/out" || fail "the production refusal must say why"
run --dry-run --date 2026-1-1 --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "a malformed date must be refused"
run --dry-run --full-copy --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "--full-copy without a source container must be refused"
DOCKER_HOST=tcp://elsewhere:2375 run --dry-run --full-copy --source-container src --evidence-file "$ev" "$TMP/cand" "$TMP/roll" &&
  fail "--full-copy against a remote daemon must be refused"
grep -q "remote daemon" "$TMP/out" || fail "the full-copy refusal must say why"
run --dry-run --source-container 'a;b' --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "a shell metacharacter in a container name must be refused"
: >"$ev"
run --dry-run --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "an existing evidence file must not be overwritten"
rm "$ev"

cp -r "$TMP/cand" "$TMP/cand-switch"
printf '%s=true\n' SESSION_CORE_MIGRATION_IGNORE_MISSING >"$TMP/cand-switch/runtime-public-policy.env"
run --dry-run --evidence-file "$ev" "$TMP/cand-switch" "$TMP/roll" && fail "an artifact that carries the ignore-missing switch must be refused"

make_artifact "$TMP/cand-bad" aaaaaaa 0001_a.sql 0003_c.sql=contract
rm "$TMP/cand-bad/migrations/session-core/revert/0003_c.down.sql"
run --dry-run --evidence-file "$ev" "$TMP/cand-bad" "$TMP/roll" && fail "a contract migration without a revert file must be refused"

run --dry-run --evidence-file "$ev" "$TMP/cand" "$TMP/roll" || { cat "$TMP/out" >&2; fail "a dry run must succeed"; }
[[ ! -s "$CALLS" ]] || fail "a dry run must not call docker"
[[ ! -e "$ev" ]] || fail "a dry run must not write evidence"
grep -q "the rollback does not know: 1" "$TMP/out" || fail "the dry run must report the migration count"

# Real-run preconditions: inputs by path, and no colliding compose stack.
: >"$TMP/key.pem"
printf 'FOO=bar\n' >"$TMP/rollback-runtime.env"
export MODEL_PLANE_ARTIFACT_VERIFY_KEY="$TMP/key.pem" MODEL_PLANE_RUNTIME_ENV_FILE="$TMP/runtime.env" \
  MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="$TMP/rollback-runtime.env"
( unset MODEL_PLANE_RUNTIME_ENV_FILE; run --evidence-file "$ev" "$TMP/cand" "$TMP/roll" ) && fail "a missing runtime env path must be refused"
STUB_PS="abc123" run --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "a running model-plane stack must be refused"
grep -q "collide" "$TMP/out" || fail "the collision refusal must say why"
grep -q "^artifact " "$CALLS" && fail "nothing may be verified before the collision guard"

# --- flow: one additive migration the rollback lacks -----------------------------------------
rm -f "$ev"
run --no-build --evidence-file "$ev" "$TMP/cand" "$TMP/roll" || { cat "$TMP/out" >&2; fail "the additive flow must pass"; }
grep -q "result PASSED" "$ev" || fail "the evidence file must record the result"
grep -q "revision aaaaaaa" "$ev" && grep -q "revision bbbbbbb" "$ev" || fail "the evidence must name both revisions"
grep -q "postgresql://" "$ev" && fail "the evidence must not carry a connection string"
grep -q "tmpfs" "$CALLS" || fail "the disposable Postgres must keep its data on tmpfs"
grep -q -- "-p \|--publish" "$CALLS" && fail "the disposable Postgres must publish no port"
strict_line="$(grep '^docker run .*sc-strict-' "$CALLS")"
ignore_line="$(grep '^docker run .*sc-ignore-' "$CALLS")"
[[ -n "$strict_line" && -n "$ignore_line" ]] || fail "both a strict and an ignore-missing rollback run are expected"
[[ "$strict_line" != *SESSION_CORE_MIGRATION_IGNORE_MISSING* ]] || fail "the strict run must not carry the switch"
[[ "$ignore_line" == *"-e SESSION_CORE_MIGRATION_IGNORE_MISSING=true"* ]] || fail "the ignore run must carry the literal true"
[[ "$(grep -c 'SESSION_CORE_MIGRATION_IGNORE_MISSING' "$CALLS")" == "1" ]] || fail "the switch must reach exactly one container"
grep -q "INSERT INTO _sqlx_migrations.*(3, 'c'" "$STDIN_LOG" || fail "applied candidate migrations must be recorded for sqlx"
grep -q "^artifact roll compose .* config" "$CALLS" || grep -q "^artifact roll compose" "$CALLS" || fail "compose config must run on the rollback artifact"
grep -q "^artifact cand validate-deployability" "$CALLS" || fail "validate-deployability must run on the candidate"
grep -q "^artifact roll deploy" "$CALLS" && fail "the stack must not start without --start-stack"

# --- flow: contract migration, scripted revert, no ignore-missing -----------------------------
make_artifact "$TMP/cand2" aaaaaaa 0001_a.sql 0002_b.sql 0003_c.sql=contract
rm -f "$ev"
run --no-build --evidence-file "$ev" "$TMP/cand2" "$TMP/roll" || { cat "$TMP/out" >&2; fail "the contract flow must pass"; }
grep -q "DELETE FROM _sqlx_migrations WHERE version = 3" "$CALLS" "$STDIN_LOG" || fail "the version row must be deleted with the revert"
grep -q "CREATE TABLE x" "$STDIN_LOG" || fail "the revert file must be applied"
grep -q "scripted revert of contract migration 3" "$ev" || fail "the scripted step must be in the evidence"
grep -q "SESSION_CORE_MIGRATION_IGNORE_MISSING" "$CALLS" && fail "a fully reverted pair needs no ignore-missing"

# --- flow: equal migration sets, strict boots -------------------------------------------------
make_artifact "$TMP/cand3" aaaaaaa 0001_a.sql 0002_b.sql
rm -f "$ev"
STUB_STRICT_LOG="migrations applied" run --no-build --evidence-file "$ev" "$TMP/cand3" "$TMP/roll" ||
  { cat "$TMP/out" >&2; fail "equal migration sets must pass strict"; }
grep -q "no scripted step" "$ev" || fail "equal sets need no scripted step"
grep -q "SESSION_CORE_MIGRATION_IGNORE_MISSING" "$CALLS" && fail "equal sets must not use the switch"

# --- flow: a strict boot that should have been refused ----------------------------------------
rm -f "$ev"
STUB_STRICT_LOG="migrations applied" run --no-build --evidence-file "$ev" "$TMP/cand" "$TMP/roll" &&
  fail "a strict boot over unknown migrations means the drift detector is off: must fail"
grep -q "result FAILED" "$ev" || fail "a failed rehearsal must still write its evidence"

# --- flow: a failing artifact step ------------------------------------------------------------
rm -f "$ev"
STUB_FAIL_STEP=verify run --no-build --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "a failed verify must fail the rehearsal"
grep -q "sc-strict" "$CALLS" && fail "nothing may boot after a failed verify"
grep -q "docker network rm" "$CALLS" || true

# --- flow: --start-stack needs --no-build -----------------------------------------------------
rm -f "$ev"
run --start-stack --evidence-file "$ev" "$TMP/cand" "$TMP/roll" && fail "--start-stack without --no-build must be refused"
rm -f "$ev"
run --no-build --start-stack --evidence-file "$ev" "$TMP/cand" "$TMP/roll" || { cat "$TMP/out" >&2; fail "--no-build --start-stack must pass"; }
grep -q "^artifact roll deploy" "$CALLS" || fail "--start-stack must deploy the rollback artifact"

[[ "$failed" == "0" ]] || exit 1
echo "rollback-rehearsal test: ok"
