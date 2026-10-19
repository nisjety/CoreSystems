#!/usr/bin/env bash
# Proves apps/Ingestion Plane/scripts/bootstrap_broker_db.sh against a THROWAWAY
# postgres:16-alpine (never the live ingestion-postgres): creates the browser_broker
# role and database, is idempotent (second run changes nothing, third run after a
# password rotation re-applies the password), never prints the password, refuses a
# password that is not URL-safe, and leaves the database closed to other roles.
# Needs Docker. Run: bash scripts/tests/bootstrap-broker-db.test.sh
set -uo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
script="$root/apps/Ingestion Plane/scripts/bootstrap_broker_db.sh"
suffix=$$
pg=bbdb-test-pg-$suffix
net=bbdb-test-net-$suffix
work=$(mktemp -d "${TMPDIR:-/tmp}/bbdb-test.XXXXXX")
envf=$work/model.env
fails=0

cleanup() { docker rm -f "$pg" >/dev/null 2>&1; docker network rm "$net" >/dev/null 2>&1; rm -rf "$work"; }
trap cleanup EXIT
ok()   { printf 'ok   - %s\n' "$1"; }
fail() { printf 'FAIL - %s\n' "$1"; fails=$((fails + 1)); }
check() { if eval "$2"; then ok "$1"; else fail "$1"; fi; }
sql() { docker exec -i "$pg" psql -X -tA -U ingestion_user -d ingestion_plane_db -c "$1" | tr -d '[:space:]'; }
run() { MODEL_PLANE_ENV_FILE="$envf" bash "$script" --container "$pg" --network "$net" "$@"; }

docker network create "$net" >/dev/null || { echo "cannot create network"; exit 1; }
docker run -d --name "$pg" -e POSTGRES_USER=ingestion_user -e POSTGRES_PASSWORD=adminpw0123456789 \
  -e POSTGRES_DB=ingestion_plane_db postgres:16-alpine >/dev/null || { echo "cannot start postgres"; exit 1; }
for _ in $(seq 1 60); do
  # the entrypoint restarts the server once after initdb; its temporary server has no TCP listener, so TCP readiness means the final one
  docker exec "$pg" pg_isready -q -h "$(docker exec "$pg" hostname -i | cut -d' ' -f1)" -U ingestion_user && break
  sleep 1
done
printf 'UNRELATED=1\nBROWSER_BROKER_PG_PASSWORD=\n' > "$envf"

# 1. no password and no --mint-if-missing: refuses, exit 3
run >"$work/o0" 2>&1; rc=$?
check "refuses without a password (rc 3)" '[[ $rc -eq 3 ]]'

# 2. not URL-safe: refused, exit 3, nothing created
BROWSER_BROKER_PG_PASSWORD='bad password/with:chars' run >"$work/o1" 2>&1; rc=$?
check "refuses a non URL-safe password (rc 3)" '[[ $rc -eq 3 ]]'
check "nothing was created by a refused run" '[[ -z "$(sql "SELECT 1 FROM pg_roles WHERE rolname = '"'"'browser_broker'"'"'")" ]]'

# 3. the database does not exist yet, --check reports and changes nothing
run --check >"$work/o2" 2>&1; rc=$?
check "--check reports missing role and database, rc 0" '[[ $rc -eq 0 ]] && grep -q "role browser_broker: missing" "$work/o2" && grep -q "database browser_broker: missing" "$work/o2"'

# 4. first real run, mints the password into the (empty) key
run --mint-if-missing >"$work/o3" 2>&1; rc=$?
pw=$(grep -E '^BROWSER_BROKER_PG_PASSWORD=' "$envf" | cut -d= -f2-)
check "first run rc 0" '[[ $rc -eq 0 ]]'
check "a URL-safe password of 16+ characters was written to the env file" '[[ "$pw" =~ ^[A-Za-z0-9._~-]{16,}$ ]]'
check "other keys of the env file are untouched" 'grep -q "^UNRELATED=1$" "$envf"'
check "the run created role and database" 'grep -q "role browser_broker: t" "$work/o3" && grep -q "database browser_broker: t" "$work/o3"'
check "the password is not in the output" '! grep -qF -- "$pw" "$work/o3"'
check "login as browser_broker works over TCP" 'grep -q "verified login: browser_broker@browser_broker" "$work/o3"'
check "role is not superuser, createdb or createrole" '[[ "$(sql "SELECT rolsuper OR rolcreatedb OR rolcreaterole FROM pg_roles WHERE rolname = '"'"'browser_broker'"'"'")" == "f" ]]'
check "the database is owned by browser_broker" '[[ "$(sql "SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = '"'"'browser_broker'"'"'")" == "browser_broker" ]]'
check "the role was attached to the network" 'docker inspect -f "{{json .NetworkSettings.Networks}}" "$pg" | grep -q "$net"'

# 5. the migration-style DDL works as the broker role (it owns the database)
docker exec -i -e PGPASSWORD="$pw" "$pg" sh -c 'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U browser_broker -d browser_broker -c "CREATE TABLE IF NOT EXISTS t_probe (id int); DROP TABLE t_probe"' >/dev/null 2>&1; rc=$?
check "browser_broker can create and drop a table in public" '[[ $rc -eq 0 ]]'

# 6. second run: idempotent, nothing recreated, password unchanged
before=$(sha256sum "$envf" | cut -d' ' -f1)
run >"$work/o4" 2>&1; rc=$?
after=$(sha256sum "$envf" | cut -d' ' -f1)
check "second run rc 0, nothing created, env file unchanged" '[[ $rc -eq 0 && "$before" == "$after" ]] && grep -q "role browser_broker: f" "$work/o4" && grep -q "database browser_broker: f" "$work/o4"'
check "second run does not print the password" '! grep -qF -- "$pw" "$work/o4"'

# 7. rotation: a new value in the env file is applied by the next run
newpw=rotated0123456789abcdef
sed -i "s|^BROWSER_BROKER_PG_PASSWORD=.*|BROWSER_BROKER_PG_PASSWORD=$newpw|" "$envf"
run >"$work/o5" 2>&1; rc=$?
check "rotation: rc 0 and login works with the new password" '[[ $rc -eq 0 ]] && grep -q "verified login: browser_broker@browser_broker" "$work/o5"'
check "rotation: the old password no longer logs in" '! docker exec -i -e PGPASSWORD="$pw" "$pg" sh -c '"'"'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U browser_broker -d browser_broker -c "select 1"'"'"' >/dev/null 2>&1'

# 8. isolation: another role cannot connect to browser_broker
other=$(date +%s%N | sha256sum | cut -c1-24)
sql "CREATE ROLE other_role LOGIN PASSWORD '$other'" >/dev/null
docker exec -i -e PGPASSWORD="$other" "$pg" sh -c 'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U other_role -d browser_broker -c "select 1"' >/dev/null 2>&1; rc=$?
check "another role cannot connect to browser_broker (CONNECT revoked from PUBLIC)" '[[ $rc -ne 0 ]]'

if [[ $fails -eq 0 ]]; then echo "ALL PASSED"; exit 0; fi
echo "$fails FAILED"; exit 1
