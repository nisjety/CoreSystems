#!/usr/bin/env bash
set -euo pipefail
umask 077

# Rollback rehearsal: prove that the previous signed artifact (the rollback) still boots over the
# database the next one (the candidate) has migrated, before anyone needs it.
#
#   scripts/rollback-rehearsal.sh [--dry-run] [--no-build] [--start-stack] [--date YYYY-MM-DD]
#                                 [--source-container NAME [--full-copy]] [--seed-sql FILE]
#                                 [--evidence-file PATH] [--wait-seconds N]
#                                 <candidate-artifact-dir> <rollback-artifact-dir>
#
# 1. Verifies both artifacts and loads their images.
# 2. Starts a disposable Postgres from the candidate artifact's postgres image (data on tmpfs, on a
#    private network, nothing published) and builds the candidate's schema in it: from the
#    artifact's own session-core migrations, or from a schema-only dump of --source-container
#    (--full-copy copies the rows too; refused unless the Docker daemon is on this host, because
#    the data must not leave it). --seed-sql adds synthetic rows.
# 3. Runs the rollback artifact's session-core against that database. Strict first: with candidate
#    migrations the rollback does not know it must refuse to boot. Then, for a policy-compliant
#    pair (docs/runbooks/migration-policy.md) it must boot with SESSION_CORE_MIGRATION_IGNORE_MISSING
#    set in this overlay and nowhere else. A contract migration needs its revert file and a
#    `DELETE FROM _sqlx_migrations` first; the script does that step and records it.
# 4. `compose <rollback> config`, optionally a `--no-build` start of the rollback stack, and
#    `validate-deployability` of the candidate.
# 5. Writes docs/compliance/evidence/rollback-rehearsal-<date>.txt: names, versions, counts and
#    pass/fail only. No credential, runtime value or row content is read into it.
#
# Inputs by path or environment, never printed: MODEL_PLANE_ARTIFACT_VERIFY_KEY,
# MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY (defaults to the first), MODEL_PLANE_RUNTIME_ENV_FILE
# (candidate runtime), MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE (rollback runtime).
#
# The compose project is fixed (`model-plane`), so a running dev stack collides with a rollback
# start: run this on a second host or stop that stack. --dry-run reads the two artifact trees and
# prints the plan without Docker and without verifying signatures.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="model-plane"
SWITCH="SESSION_CORE_MIGRATION_IGNORE_MISSING"

die() {
  echo "rollback-rehearsal: $*" >&2
  exit 1
}

usage() {
  sed -n '4,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

assignment_value() {
  sed -n "s/^$2=//p" "$1" | head -n 1
}

# "<version><TAB><file name>" for each migration directly under <dir>, ascending by version.
migration_versions() {
  local file name
  for file in "$1"/[0-9][0-9][0-9][0-9]_*.sql; do
    [[ -f "$file" ]] || continue
    name="$(basename "$file")"
    printf '%s\t%s\n' "$((10#${name%%_*}))" "$name"
  done | sort -n
}

is_contract_migration() {
  grep -Eq '^[[:space:]]*--[[:space:]]*contract:' "$1"
}

# Migrations the candidate has applied that the rollback does not embed, one line each:
#   additive<TAB><version><TAB><file>
#   contract<TAB><version><TAB><file><TAB><revert file>
# A contract migration without its revert file is reported as `contract-without-revert`, and the
# function returns 1 so the caller refuses before touching Docker.
rollback_plan() {
  local candidate="$1" rollback="$2" version name revert status=0
  while IFS=$'\t' read -r version name; do
    if migration_versions "$rollback" | cut -f1 | grep -qx "$version"; then continue; fi
    if is_contract_migration "$candidate/$name"; then
      revert="$candidate/revert/${name%.sql}.down.sql"
      if [[ -f "$revert" ]]; then
        printf 'contract\t%s\t%s\t%s\n' "$version" "$name" "$revert"
      else
        printf 'contract-without-revert\t%s\t%s\n' "$version" "$name"
        status=1
      fi
    else
      printf 'additive\t%s\t%s\n' "$version" "$name"
    fi
  done < <(migration_versions "$candidate")
  return "$status"
}

# sqlx stores the migration description as the file name without its number and extension, with
# spaces for underscores, and the SHA-384 of the file as its checksum.
sqlx_description() {
  local name="${1#*_}"
  name="${name%.sql}"
  printf '%s' "${name//_/ }"
}

sqlx_checksum_hex() {
  sha384sum "$1" | awk '{print $1}'
}

docker_daemon_is_local() {
  case "${DOCKER_HOST:-}" in
    "" | unix://* | npipe://*) return 0 ;;
    *) return 1 ;;
  esac
}

today_utc() { date -u +%Y-%m-%d; }

main() {
  local dry_run=0 no_build=0 start_stack=0 full_copy=0 date_arg="" source_container="" seed_sql=""
  local evidence_file="" wait_seconds=120 candidate="" rollback=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --dry-run) dry_run=1 ;;
      --no-build) no_build=1 ;;
      --start-stack) start_stack=1 ;;
      --full-copy) full_copy=1 ;;
      --date)
        [[ $# -ge 2 ]] || die "--date needs YYYY-MM-DD"
        date_arg="$2"
        shift
        ;;
      --source-container)
        [[ $# -ge 2 && "$2" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || die "--source-container needs a container name"
        source_container="$2"
        shift
        ;;
      --seed-sql)
        [[ $# -ge 2 ]] || die "--seed-sql needs a file"
        seed_sql="$2"
        shift
        ;;
      --evidence-file)
        [[ $# -ge 2 && -n "$2" ]] || die "--evidence-file needs a path"
        evidence_file="$2"
        shift
        ;;
      --wait-seconds)
        [[ $# -ge 2 && "$2" =~ ^[0-9]+$ && "$2" -ge 1 ]] || die "--wait-seconds needs a positive integer"
        wait_seconds="$2"
        shift
        ;;
      -h | --help) usage ;;
      -*) die "unknown option '$1'" ;;
      *)
        if [[ -z "$candidate" ]]; then
          candidate="$1"
        elif [[ -z "$rollback" ]]; then
          rollback="$1"
        else
          die "exactly two artifact directories are expected"
        fi
        ;;
    esac
    shift
  done
  [[ -n "$candidate" && -n "$rollback" ]] || usage

  # Guards come first and do not depend on --dry-run: the answer must not change with a flag.
  [[ "${MODEL_PLANE_PRODUCTION:-0}" != "1" ]] ||
    die "refusing MODEL_PLANE_PRODUCTION=1; a rehearsal never runs against the production stack"
  [[ -d "$candidate" && -d "$rollback" ]] || die "both artifact directories must exist"
  candidate="$(cd "$candidate" && pwd)"
  rollback="$(cd "$rollback" && pwd)"
  [[ "$candidate" != "$rollback" ]] || die "the candidate and the rollback must be different artifacts"
  local dir
  for dir in "$candidate" "$rollback"; do
    [[ -f "$dir/images.lock.env" && -d "$dir/migrations/session-core" ]] ||
      die "$dir is not a release artifact (images.lock.env or migrations/session-core is missing)"
    [[ "$(assignment_value "$dir/images.lock.env" SOURCE_REVISION)" =~ ^[0-9a-f]{7,40}$ ]] ||
      die "$dir has no SOURCE_REVISION in images.lock.env"
    # The switch belongs to this script's rollback run alone. If a signed artifact carries it, a
    # strict boot is no longer strict and the rehearsal would prove nothing.
    if grep -qs "^$SWITCH=" "$dir/runtime-public-policy.env" "$dir/images.lock.env"; then
      die "$dir sets $SWITCH in its own configuration; only the rollback overlay may"
    fi
  done
  [[ -z "$date_arg" || "$date_arg" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || die "--date must be YYYY-MM-DD"
  local date_utc="${date_arg:-$(today_utc)}"
  if [[ "$full_copy" == "1" ]]; then
    [[ -n "$source_container" ]] || die "--full-copy needs --source-container"
    docker_daemon_is_local ||
      die "--full-copy copies real rows; refused because DOCKER_HOST points at a remote daemon"
  fi
  # The rollback start is `deploy`, which is always `up --no-build`; asking for it by name keeps
  # anyone from expecting a build here.
  [[ "$start_stack" == "0" || "$no_build" == "1" ]] || die "--start-stack needs --no-build"
  [[ -z "$seed_sql" || -f "$seed_sql" ]] || die "--seed-sql file does not exist"
  [[ -n "$evidence_file" ]] || evidence_file="$ROOT_DIR/../../docs/compliance/evidence/rollback-rehearsal-$date_utc.txt"
  [[ ! -e "$evidence_file" ]] || die "evidence file already exists, pick another --date or --evidence-file: $evidence_file"

  local plan
  if ! plan="$(rollback_plan "$candidate/migrations/session-core" "$rollback/migrations/session-core")"; then
    echo "$plan" >&2
    die "a contract migration has no revert file; the rollback cannot be rehearsed"
  fi
  local extra_count
  extra_count="$(printf '%s' "$plan" | grep -c . || true)"

  if [[ "$dry_run" == "1" ]]; then
    echo "rollback-rehearsal: dry run (no Docker, signatures not verified)"
    echo "  candidate: $candidate (revision $(assignment_value "$candidate/images.lock.env" SOURCE_REVISION))"
    echo "  rollback:  $rollback (revision $(assignment_value "$rollback/images.lock.env" SOURCE_REVISION))"
    echo "  session-core migrations the rollback does not know: $extra_count"
    [[ -z "$plan" ]] || printf '%s\n' "$plan" | sed 's/^/    /'
    if [[ "$extra_count" == "0" ]]; then
      echo "  expected: the rollback boots strict, no scripted step"
    else
      echo "  expected: strict boot refused, then boots with $SWITCH=true in the rollback overlay"
    fi
    echo "  evidence would be written to $evidence_file"
    return 0
  fi

  local key
  for key in MODEL_PLANE_ARTIFACT_VERIFY_KEY MODEL_PLANE_RUNTIME_ENV_FILE MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE; do
    [[ -n "${!key:-}" ]] || die "$key must be set (a path; its content is never read here)"
  done
  [[ -f "$MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE" ]] || die "MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE is not a file"
  command -v docker >/dev/null 2>&1 || die "docker is not available"
  if [[ -n "$(docker ps -q --filter "label=com.docker.compose.project=$PROJECT" | head -n 1)" ]]; then
    die "a '$PROJECT' compose stack is running here; its names collide with the rollback stack. Use a second host or stop it"
  fi
  run_rehearsal "$candidate" "$rollback" "$plan" "$extra_count" "$evidence_file" "$date_utc" \
    "$start_stack" "$full_copy" "$source_container" "$seed_sql" "$wait_seconds"
}

EVIDENCE=()
RESULT="FAILED"
EVIDENCE_FILE=""
TMP_DIR=""
NET=""
CONTAINERS=()

note() { EVIDENCE+=("$*"); echo "rollback-rehearsal: $*"; }

write_evidence() {
  [[ -n "$EVIDENCE_FILE" ]] || return 0
  mkdir -p "$(dirname "$EVIDENCE_FILE")"
  {
    echo "# Rollback rehearsal: result $RESULT"
    echo "# Names, versions, counts and pass/fail only. No credential, runtime value or row content."
    printf '%s\n' "${EVIDENCE[@]}"
  } >"$EVIDENCE_FILE"
}

cleanup() {
  local name
  for name in ${CONTAINERS[@]+"${CONTAINERS[@]}"}; do docker rm -f "$name" >/dev/null 2>&1 || true; done
  [[ -z "$NET" ]] || docker network rm "$NET" >/dev/null 2>&1 || true
  [[ -z "$TMP_DIR" ]] || rm -rf "$TMP_DIR"
  write_evidence
}

fail() {
  note "FAIL: $*"
  echo "rollback-rehearsal: $*" >&2
  exit 1
}

step() {
  local label="$1"
  shift
  if "$@" >"$TMP_DIR/step.out" 2>&1; then
    note "step ok: $label"
  else
    tail -n 5 "$TMP_DIR/step.out" >&2 || true
    fail "step failed: $label"
  fi
}

run_rehearsal() {
  local candidate="$1" rollback="$2" plan="$3" extra_count="$4" date_utc="$6" line
  local start_stack="$7" full_copy="$8" source_container="$9" seed_sql="${10}" wait_seconds="${11}"
  EVIDENCE_FILE="$5"
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rollback-rehearsal.XXXXXX")"
  trap cleanup EXIT

  local cand_rev roll_rev
  cand_rev="$(assignment_value "$candidate/images.lock.env" SOURCE_REVISION)"
  roll_rev="$(assignment_value "$rollback/images.lock.env" SOURCE_REVISION)"
  note "date: $date_utc"
  note "candidate: revision $cand_rev, root manifest sha256 $(sha256sum "$candidate/artifact-manifest.tsv" 2>/dev/null | awk '{print $1}')"
  note "rollback: revision $roll_rev, root manifest sha256 $(sha256sum "$rollback/artifact-manifest.tsv" 2>/dev/null | awk '{print $1}')"
  note "session-core migrations unknown to the rollback: $extra_count"
  [[ -z "$plan" ]] || while IFS= read -r line; do note "  $line"; done <<<"$plan"

  local rollback_key="${MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY:-$MODEL_PLANE_ARTIFACT_VERIFY_KEY}"

  # 1. Verify, then load both artifacts' images.
  step "verify candidate" env MODEL_PLANE_RELEASE_MODE=1 \
    MODEL_PLANE_ARTIFACT_VERIFY_KEY="$MODEL_PLANE_ARTIFACT_VERIFY_KEY" \
    MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback" \
    bash "$candidate/scripts/release-artifact.sh" verify "$candidate"
  # The baseline is built in non-release mode, so it is verified as a plain signed artifact.
  step "verify rollback" env -u MODEL_PLANE_RELEASE_MODE \
    MODEL_PLANE_ARTIFACT_VERIFY_KEY="$rollback_key" \
    bash "$rollback/scripts/release-artifact.sh" verify "$rollback"
  step "restore candidate images" bash "$candidate/scripts/release-artifact.sh" restore "$candidate"
  step "restore rollback images" bash "$rollback/scripts/release-artifact.sh" restore "$rollback"

  # 2. Disposable Postgres. The password is generated here, written to a private file and never
  # echoed; the data directory is tmpfs and no port is published.
  local pg_image rollback_image suffix pg db_env pg_env
  pg_image="$(assignment_value "$candidate/images.lock.env" POSTGRES_RELEASE_IMAGE)"
  rollback_image="$(assignment_value "$rollback/images.lock.env" SESSION_CORE_RELEASE_IMAGE)"
  [[ -n "$pg_image" && -n "$rollback_image" ]] || fail "images.lock.env lacks POSTGRES_RELEASE_IMAGE or SESSION_CORE_RELEASE_IMAGE"
  suffix="$$"
  NET="mp-rollback-rehearsal-$suffix"
  pg="mp-rollback-rehearsal-pg-$suffix"
  local password
  password="$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  pg_env="$TMP_DIR/pg.env"
  db_env="$TMP_DIR/db.env"
  printf 'POSTGRES_USER=postgres\nPOSTGRES_PASSWORD=%s\nPOSTGRES_DB=session_core\n' "$password" >"$pg_env"
  printf 'DATABASE_URL=postgresql://postgres:%s@rehearsal-postgres:5432/session_core\n' "$password" >"$db_env"

  docker network create "$NET" >/dev/null || fail "could not create the private network"
  # Registered up front: boot_outcome runs in a command substitution, where an append would be lost.
  CONTAINERS+=("$pg" "mp-rollback-rehearsal-sc-strict-$suffix" "mp-rollback-rehearsal-sc-after-$suffix" "mp-rollback-rehearsal-sc-ignore-$suffix")
  docker run -d --name "$pg" --network "$NET" --network-alias rehearsal-postgres \
    --tmpfs /var/lib/postgresql/data --env-file "$pg_env" "$pg_image" >/dev/null ||
    fail "could not start the disposable Postgres"
  local waited=0
  # TCP answers only after the image's init server has been replaced by the final one.
  until docker exec "$pg" pg_isready -q -h 127.0.0.1 -U postgres >/dev/null 2>&1; do
    waited=$((waited + 1))
    [[ "$waited" -le "$wait_seconds" ]] || fail "the disposable Postgres did not become ready"
    sleep 1
  done
  note "step ok: disposable postgres from the candidate artifact image"

  pgx() { docker exec -i "$pg" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -X -q "$@"; }
  pgx_c() { pgx "$@" </dev/null; }

  local source_mode="candidate migrations only"
  if [[ -n "$source_container" ]]; then
    if [[ "$full_copy" == "1" ]]; then
      docker exec "$source_container" pg_dump -U postgres session_core | pgx >/dev/null ||
        fail "full copy from $source_container failed"
      source_mode="full copy of $source_container (same host, tmpfs)"
    else
      docker exec "$source_container" pg_dump -s -U postgres session_core | pgx >/dev/null ||
        fail "schema dump from $source_container failed"
      docker exec "$source_container" pg_dump -a -t _sqlx_migrations -U postgres session_core | pgx >/dev/null ||
        fail "migration table copy from $source_container failed"
      source_mode="schema-only dump of $source_container"
    fi
  fi
  note "database source: $source_mode"

  pgx_c -c "CREATE TABLE IF NOT EXISTS _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMPTZ NOT NULL DEFAULT now(), success BOOLEAN NOT NULL, checksum BYTEA NOT NULL, execution_time BIGINT NOT NULL)" ||
    fail "could not create _sqlx_migrations"
  local applied version name applied_count=0
  applied="$(pgx_c -At -c "SELECT version FROM _sqlx_migrations ORDER BY version" || true)"
  while IFS=$'\t' read -r version name; do
    if printf '%s\n' "$applied" | grep -qx "$version"; then continue; fi
    pgx -1 <"$candidate/migrations/session-core/$name" >/dev/null || fail "candidate migration $name did not apply"
    pgx_c -c "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES ($version, '$(sqlx_description "$name")', true, decode('$(sqlx_checksum_hex "$candidate/migrations/session-core/$name")', 'hex'), 0)" >/dev/null ||
      fail "could not record candidate migration $name"
    applied_count=$((applied_count + 1))
  done < <(migration_versions "$candidate/migrations/session-core")
  note "step ok: applied $applied_count candidate migration(s) not already in the database"
  if [[ -n "$seed_sql" ]]; then
    pgx <"$seed_sql" >/dev/null || fail "the synthetic seed did not apply"
    note "step ok: synthetic seed $(basename "$seed_sql") applied"
  fi

  # 3. The rollback session-core. Only the ignore run carries the switch.
  local public_env="$rollback/runtime-public-policy.env"
  [[ -f "$public_env" ]] || public_env=/dev/null
  boot_outcome() {
    local mode="$1" container="$2" args=() logs state elapsed=0
    [[ "$mode" != "ignore" ]] || args=(-e "$SWITCH=true")
    docker run -d --name "$container" --network "$NET" \
      --env-file "$public_env" --env-file "$MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE" --env-file "$db_env" \
      ${args[@]+"${args[@]}"} "$rollback_image" >/dev/null 2>&1 || { echo "did-not-start"; return 0; }
    while [[ "$elapsed" -lt "$wait_seconds" ]]; do
      logs="$(docker logs "$container" 2>&1 || true)"
      if grep -q "previously applied but is missing" <<<"$logs"; then echo "refused-unknown-migration"; return 0; fi
      if grep -q "migrations applied" <<<"$logs"; then echo "migrated"; return 0; fi
      state="$(docker inspect --format '{{.State.Status}}' "$container" 2>/dev/null || echo gone)"
      if [[ "$state" != "running" && "$state" != "created" ]]; then echo "exited-other"; return 0; fi
      elapsed=$((elapsed + 1))
      sleep 1
    done
    echo "timeout"
  }

  local strict
  strict="$(boot_outcome strict "mp-rollback-rehearsal-sc-strict-$suffix")"
  docker rm -f "mp-rollback-rehearsal-sc-strict-$suffix" >/dev/null 2>&1 || true
  if [[ "$extra_count" == "0" ]]; then
    [[ "$strict" == "migrated" ]] || fail "rollback session-core, strict, over an equal migration set: $strict (expected migrated)"
    note "step ok: rollback session-core boots strict, no scripted step"
  else
    [[ "$strict" == "refused-unknown-migration" ]] ||
      fail "rollback session-core, strict, over unknown migrations: $strict (expected refused-unknown-migration)"
    note "step ok: strict rollback session-core refuses the unknown migrations, as the drift detector must"
    # Contract migrations are reverted newest first; the version row goes with the revert so the
    # older image does not see a version it cannot embed.
    local revert_file contract_versions
    contract_versions="$(printf '%s\n' "$plan" | awk -F'\t' '$1 == "contract" {print $2 "\t" $4}' | sort -rn)"
    while IFS=$'\t' read -r version revert_file; do
      [[ -n "$version" ]] || continue
      pgx <"$revert_file" >/dev/null || fail "revert for migration $version did not apply"
      pgx_c -c "DELETE FROM _sqlx_migrations WHERE version = $version" >/dev/null ||
        fail "could not delete the version row for migration $version"
      note "step ok: scripted revert of contract migration $version"
    done <<<"$contract_versions"
    local remaining
    remaining="$(printf '%s\n' "$plan" | grep -c '^additive' || true)"
    if [[ "$remaining" == "0" ]]; then
      local after
      after="$(boot_outcome strict "mp-rollback-rehearsal-sc-after-$suffix")"
      docker rm -f "mp-rollback-rehearsal-sc-after-$suffix" >/dev/null 2>&1 || true
      [[ "$after" == "migrated" ]] || fail "rollback session-core after the scripted reverts: $after (expected migrated)"
      note "step ok: rollback session-core boots strict after the scripted reverts"
    else
      local ignore
      ignore="$(boot_outcome ignore "mp-rollback-rehearsal-sc-ignore-$suffix")"
      docker rm -f "mp-rollback-rehearsal-sc-ignore-$suffix" >/dev/null 2>&1 || true
      [[ "$ignore" == "migrated" ]] || fail "rollback session-core with $SWITCH=true: $ignore (expected migrated)"
      note "step ok: rollback session-core boots with $SWITCH=true over $remaining additive migration(s)"
    fi
  fi

  # 4. Compose and deployability.
  step "compose <rollback> config" env MODEL_PLANE_RUNTIME_ENV_FILE="$MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE" \
    MODEL_PLANE_ARTIFACT_VERIFY_KEY="$rollback_key" \
    bash "$rollback/scripts/release-artifact.sh" compose "$rollback" config
  if [[ "$start_stack" == "1" ]]; then
    step "start rollback stack without building" env MODEL_PLANE_RUNTIME_ENV_FILE="$MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE" \
      MODEL_PLANE_ARTIFACT_VERIFY_KEY="$rollback_key" \
      bash "$rollback/scripts/release-artifact.sh" deploy "$rollback"
    note "the rollback stack is left running; stop it before using this host for anything else"
  else
    note "rollback stack start: not requested"
  fi
  step "validate-deployability candidate" env MODEL_PLANE_RELEASE_MODE=1 \
    MODEL_PLANE_ARTIFACT_VERIFY_KEY="$MODEL_PLANE_ARTIFACT_VERIFY_KEY" \
    MODEL_PLANE_RUNTIME_ENV_FILE="$MODEL_PLANE_RUNTIME_ENV_FILE" \
    MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback" \
    bash "$candidate/scripts/release-artifact.sh" validate-deployability "$candidate"

  RESULT="PASSED"
  note "result: PASSED"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
