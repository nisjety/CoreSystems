#!/usr/bin/env bash
# Idempotent bootstrap of the Model Plane browser-broker's database on the
# Ingestion Plane Postgres server (ingestion-postgres).
#
# Creates, only when missing:
#   - role     browser_broker  (LOGIN, no superuser/createdb/createrole, 40 connections)
#   - database browser_broker  (owned by that role; CONNECT revoked from PUBLIC, so no
#                               ordinary role can open it)
# and always (re)applies the role's password from BROWSER_BROKER_PG_PASSWORD, so a
# rotation in the env file is picked up by running this again. The broker applies its
# own table migrations at start (docker-entrypoint.sh); this script creates no tables.
# Unless --keep-public-connect is given it also revokes CONNECT from PUBLIC on every
# other non-template database THAT EXISTS WHEN THE SCRIPT RUNS, so the browser_broker
# role can log in to none of the Quarry databases that exist at that moment (PostgreSQL
# grants CONNECT to PUBLIC by default; every Quarry service connects as the superuser
# ingestion_user, which that revoke does not touch). The revoke is a one-time ACL change,
# not a standing rule: a database created AFTER the script ran keeps the PostgreSQL
# default (PUBLIC may connect) until this script is run again, so re-run it after adding
# a database to this server.
#
# Why a script: init-databases.sql only runs on a fresh data volume and cannot read
# the password. This is a deliberate SHARED Postgres SERVER across two planes. The
# separation is by database and role only (no shared schema, no shared credential for
# the broker). It is NOT a security boundary against Quarry: every Quarry service
# connects as ingestion_user, the container's POSTGRES_USER superuser, and a superuser
# ignores the CONNECT revoke, so Quarry (and any other superuser) can still read and
# write browser_grants directly. A dedicated Postgres instance for the broker, or
# Quarry services on a non-superuser role plus a pg_hba.conf reject for browser_broker,
# is the real isolation; an owner may want that later.
#
# Password source, first hit wins (the value is NEVER printed, logged or put on the
# command line of any command; scripts/tests/bootstrap-broker-db.test.sh checks that
# with PATH shims that record every external command's argv):
#   1. $BROWSER_BROKER_PG_PASSWORD
#   2. BROWSER_BROKER_PG_PASSWORD= in $MODEL_PLANE_ENV_FILE
#      (default: <repo>/apps/Model Plane/deploy/.env)
# It must be URL-safe ([A-Za-z0-9._~-], at least 16 characters) because it is embedded in
# the broker's DATABASE_URL. --mint-if-missing generates one into the env file when the
# key is absent or empty; it never overwrites an existing value.
#
# Usage: bootstrap_broker_db.sh [--check] [--mint-if-missing] [--no-network]
#                               [--container NAME] [--admin-user U] [--admin-db D]
#                               [--network NAME] [--keep-public-connect]
#   --check        read-only: report what exists, change nothing
#   --no-network   do not attach the container to the broker-db network
#   --keep-public-connect  do not revoke CONNECT from PUBLIC on the other databases
#                          (without it only the databases that exist now are covered)
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
container=${INGESTION_POSTGRES_CONTAINER:-ingestion-postgres}
admin_user=${BROKER_DB_ADMIN_USER:-ingestion_user}
admin_db=${BROKER_DB_ADMIN_DB:-ingestion_plane_db}
network=${BROKER_DB_NETWORK:-browser-broker-db}
env_file=${MODEL_PLANE_ENV_FILE:-$root/apps/Model Plane/deploy/.env}
key=BROWSER_BROKER_PG_PASSWORD
role=browser_broker
database=browser_broker
check_only=0 mint=0 attach=1 lockdown=1

die() { printf 'bootstrap_broker_db: %s\n' "$*" >&2; exit "${2:-1}"; }

while [[ $# -gt 0 ]]; do
  case $1 in
    --check) check_only=1 ;;
    --mint-if-missing) mint=1 ;;
    --no-network) attach=0 ;;
    --keep-public-connect) lockdown=0 ;;
    --container) container=${2:?--container needs a value}; shift ;;
    --admin-user) admin_user=${2:?--admin-user needs a value}; shift ;;
    --admin-db) admin_db=${2:?--admin-db needs a value}; shift ;;
    --network) network=${2:?--network needs a value}; shift ;;
    -h|--help) sed -n '2,/^set -euo/{/^set -euo/!s/^# \{0,1\}//p}' "$0"; exit 0 ;;
    *) die "unknown argument: $1" 2 ;;
  esac
  shift
done

read_key() { # value of KEY in the env file, CR and surrounding quotes stripped; empty if absent
  [[ -f "$env_file" ]] || return 0
  local line
  line=$(grep -E "^${key}=" "$env_file" 2>/dev/null | tail -n 1 || true)
  line=${line#*=}; line=${line%$'\r'}
  line=${line#\"}; line=${line%\"}; line=${line#\'}; line=${line%\'}
  printf '%s' "$line"
}

pw=${BROWSER_BROKER_PG_PASSWORD:-}
[[ -n "$pw" ]] || pw=$(read_key)

if [[ -z "$pw" ]]; then
  if [[ $mint -eq 1 && $check_only -eq 0 ]]; then
    [[ -f "$env_file" ]] || die "cannot mint: env file not found: $env_file (set MODEL_PLANE_ENV_FILE)" 3
    if command -v openssl >/dev/null 2>&1; then
      pw=$(openssl rand -hex 24)
    else
      pw=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
    fi
    if grep -qE "^${key}=" "$env_file"; then
      # Present but empty (anything non-empty was read above and never reaches here).
      # Rewrite the file with shell builtins only (read, printf): the value must not
      # appear in the argv of any external command such as sed, where `ps` could show it.
      tmp=$(mktemp "${env_file}.XXXXXX")
      trap 'rm -f "$tmp"' EXIT
      while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" == "${key}="* && -z "${line#"${key}="}" ]]; then
          printf '%s=%s\n' "$key" "$pw"
        elif [[ "$line" == "${key}="$'\r' ]]; then
          printf '%s=%s\r\n' "$key" "$pw"
        else
          printf '%s\n' "$line"
        fi
      done <"$env_file" >"$tmp"
      # write through the existing file so its owner, mode and inode survive
      cat "$tmp" >"$env_file"
      rm -f "$tmp"; trap - EXIT
    else
      [[ -z "$(tail -c1 "$env_file")" ]] || printf '\n' >>"$env_file"
      printf '%s=%s\n' "$key" "$pw" >>"$env_file"
    fi
    [[ "$(read_key)" == "$pw" ]] || die "minted a password but could not write it to $env_file" 3
    printf 'minted a new %s into %s (value not shown)\n' "$key" "$env_file"
  elif [[ $check_only -eq 0 ]]; then
    die "$key is not set (environment or $env_file). Add it, or pass --mint-if-missing to generate one." 3
  else
    printf '%s is not set yet (a real run needs it)\n' "$key"
  fi
fi
if [[ -n "$pw" || $check_only -eq 0 ]]; then
  [[ "$pw" =~ ^[A-Za-z0-9._~-]{16,}$ ]] ||
    die "$key must be URL-safe ([A-Za-z0-9._~-]) and at least 16 characters (it sits inside DATABASE_URL)" 3
fi

command -v docker >/dev/null 2>&1 || die "docker not found" 4
state=$(docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null || true)
[[ "$state" == "true" ]] || die "container $container is not running" 4

# Run psql in the server container as the admin role, SQL on stdin. Anything psql prints is
# masked before it is shown, in case an error message quotes a statement.
psql_admin() {
  local out rc=0
  out=$(docker exec -i "$container" psql -X -q -v ON_ERROR_STOP=1 -U "$admin_user" -d "$admin_db" "$@" 2>&1) || rc=$?
  printf '%s\n' "${out//"$pw"/***}"
  return $rc
}
psql_scalar() { docker exec -i "$container" psql -X -tA -U "$admin_user" -d "$admin_db" -c "$1" 2>/dev/null | tr -d '[:space:]' || true; }

role_exists=$(psql_scalar "SELECT 1 FROM pg_roles WHERE rolname = '$role'")
db_exists=$(psql_scalar "SELECT 1 FROM pg_database WHERE datname = '$database'")
net_ok=$(docker inspect -f '{{if index .NetworkSettings.Networks "'"$network"'"}}1{{end}}' "$container" 2>/dev/null || true)
printf 'role %s: %s\ndatabase %s: %s\nnetwork %s: %s\n' \
  "$role" "$([[ -n $role_exists ]] && echo present || echo missing)" \
  "$database" "$([[ -n $db_exists ]] && echo present || echo missing)" \
  "$network" "$([[ -n $net_ok ]] && echo "$container attached" || echo "$container NOT attached")"
if [[ $check_only -eq 1 ]]; then exit 0; fi

if [[ $attach -eq 1 && -z "$net_ok" ]]; then
  docker network inspect "$network" >/dev/null 2>&1 ||
    die "network $network does not exist. It is created by the Ingestion Plane compose (service postgres); bring that service up with the updated compose file first." 5
  docker network connect "$network" "$container"
  printf 'attached %s to %s\n' "$container" "$network"
fi

psql_admin <<SQL
\\set pw '$pw'
SELECT NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '$role') AS role_missing \\gset
\\if :role_missing
  CREATE ROLE $role LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION CONNECTION LIMIT 40;
\\endif
SELECT format('ALTER ROLE $role WITH PASSWORD %L', :'pw') \\gexec
SELECT NOT EXISTS (SELECT FROM pg_database WHERE datname = '$database') AS db_missing \\gset
\\if :db_missing
  CREATE DATABASE $database OWNER $role;
\\endif
ALTER DATABASE $database OWNER TO $role;
REVOKE ALL ON DATABASE $database FROM PUBLIC;
\\if $lockdown
  -- the broker role must not log in to Quarry's databases (CONNECT is granted to PUBLIC by default);
  -- covers only the databases that exist now, a later CREATE DATABASE needs a re-run
  SELECT format('REVOKE CONNECT ON DATABASE %I FROM PUBLIC', datname) FROM pg_database
    WHERE NOT datistemplate AND datallowconn AND datname <> '$database' \\gexec
\\endif
\\echo role $role: :role_missing (t = created now, f = already existed; password applied either way)
\\echo database $database: :db_missing (t = created now, f = already existed)
\echo other databases: CONNECT $([[ $lockdown -eq 1 ]] && echo 'revoked from PUBLIC on the ones that exist now (re-run after creating a database)' || echo left as it was)
SQL

# Prove the password works over TCP (the loopback line of the stock pg_hba is trust, so use the
# container's own address). The password reaches psql through the environment, not argv.
login_rc=0
login_out=$(PGPASSWORD="$pw" docker exec -i -e PGPASSWORD "$container" sh -c \
  'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U '"$role"' -d '"$database"' -c "SELECT current_user || chr(64) || current_database()"' \
  2>&1) || login_rc=$?
printf 'verified login: %s\n' "${login_out//"$pw"/***}"
[[ $login_rc -eq 0 ]] || die "the $role login check failed" 6
printf 'done. DATABASE_URL for the broker: postgresql://%s:<%s>@%s:5432/%s\n' "$role" "$key" "$container" "$database"
