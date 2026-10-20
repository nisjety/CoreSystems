#!/usr/bin/env bash
# Proves apps/Ingestion Plane/scripts/bootstrap_broker_db.sh against a THROWAWAY
# postgres:16-alpine (never the live ingestion-postgres): creates the browser_broker
# role and database, is idempotent (second run changes nothing, third run after a
# password rotation re-applies the password), never prints the password and never puts it
# in the argv of any external command (PATH shims record every argv), refuses a password
# that is not URL-safe, never overwrites a present password, and keeps the browser_broker
# role out of the other databases and ordinary roles out of browser_broker.
# It also PINS a known limitation instead of hiding it: the container superuser
# (ingestion_user, which every Quarry service uses) can still connect to browser_broker,
# because a superuser ignores the CONNECT revoke. This is a shared server, not a security
# boundary against Quarry.
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
# CR bookkeeping in bash (grep strips CR on some hosts, e.g. Git Bash)
count_cr() { local n=0 l; while IFS= read -r l || [[ -n "$l" ]]; do [[ "$l" == *$'\r' ]] && n=$((n + 1)); done <"$1"; echo "$n"; }
has_line() { local l; while IFS= read -r l || [[ -n "$l" ]]; do [[ "$l" == "$2" ]] && return 0; done <"$1"; return 1; }
crA=$'A=1\r'; crB=$'B=2\r'
# connect over TCP as $2 with password $1 to database $3 (the stock pg_hba trusts loopback, so use the container's address)
tcp() { docker exec -i -e PGPASSWORD="$1" "$pg" sh -c 'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U '"$2"' -d '"$3"' -c "select 1"' >/dev/null 2>&1; }

# PATH shims: every external command the script runs is recorded (name and argv) in $ARGV_LOG before
# it is exec'd, so a secret on a command line (visible in `ps`) can be detected. Builtins are not recorded.
shim=$work/shim; mkdir -p "$shim"
for c in sed grep tail head od tr cat mktemp openssl docker hostname cut rm mv cp tee awk; do
  real=$(command -v "$c") || continue
  printf '#!/usr/bin/env bash\nprintf "%%s %%s\\n" "${0##*/}" "$*" >>"$ARGV_LOG"\nexec "%s" "$@"\n' "$real" >"$shim/$c"
  chmod +x "$shim/$c"
done

docker network create "$net" >/dev/null || { echo "cannot create network"; exit 1; }
docker run -d --name "$pg" -e POSTGRES_USER=ingestion_user -e POSTGRES_PASSWORD=adminpw0123456789 \
  -e POSTGRES_DB=ingestion_plane_db postgres:16-alpine >/dev/null || { echo "cannot start postgres"; exit 1; }
for _ in $(seq 1 60); do
  # the entrypoint restarts the server once after initdb; its temporary server has no TCP listener, so TCP readiness means the final one
  docker exec "$pg" pg_isready -q -h "$(docker exec "$pg" hostname -i | cut -d' ' -f1)" -U ingestion_user && break
  sleep 1
done
printf 'UNRELATED=1\nBROWSER_BROKER_PG_PASSWORD=\n' > "$envf"
# a stand-in for one of Quarry's databases, created before the bootstrap like the real ones are
sql "CREATE DATABASE quarry_probe" >/dev/null

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
argv=$work/argv3.log; : >"$argv"
PATH="$shim:$PATH" ARGV_LOG="$argv" run --mint-if-missing >"$work/o3" 2>&1; rc=$?
pw=$(grep -E '^BROWSER_BROKER_PG_PASSWORD=' "$envf" | cut -d= -f2-)
check "first run rc 0" '[[ $rc -eq 0 ]]'
check "a URL-safe password of 16+ characters was written to the env file" '[[ "$pw" =~ ^[A-Za-z0-9._~-]{16,}$ ]]'
check "other keys of the env file are untouched" 'grep -q "^UNRELATED=1$" "$envf"'
check "the run created role and database" 'grep -q "role browser_broker: t" "$work/o3" && grep -q "database browser_broker: t" "$work/o3"'
check "the password is not in the output" '! grep -qF -- "$pw" "$work/o3"'
check "the shims saw the run (docker and openssl were invoked through them)" 'grep -q "^docker " "$argv" && grep -q "^openssl " "$argv"'
check "the minted password is in the argv of no external command (mint into an empty key)" '! grep -qF -- "$pw" "$argv"'
check "login as browser_broker works over TCP" 'grep -q "verified login: browser_broker@browser_broker" "$work/o3"'
check "role is not superuser, createdb or createrole" '[[ "$(sql "SELECT rolsuper OR rolcreatedb OR rolcreaterole FROM pg_roles WHERE rolname = '"'"'browser_broker'"'"'")" == "f" ]]'
check "the database is owned by browser_broker" '[[ "$(sql "SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = '"'"'browser_broker'"'"'")" == "browser_broker" ]]'
check "the role was attached to the network" 'docker inspect -f "{{json .NetworkSettings.Networks}}" "$pg" | grep -q "$net"'

# 4b. mint into env files without the key / with CRLF line endings. The container does not exist, so the
#     run stops with rc 4 right after the mint; what matters is the file and the argv log.
envf2=$work/model2.env; printf 'OTHER=1' > "$envf2"   # no trailing newline, key absent
argv=$work/argv4b.log; : >"$argv"
PATH="$shim:$PATH" ARGV_LOG="$argv" MODEL_PLANE_ENV_FILE="$envf2" bash "$script" --container "no-such-$suffix" --network "$net" --mint-if-missing >"$work/o3b" 2>&1; rc=$?
pw2=$(grep -E '^BROWSER_BROKER_PG_PASSWORD=' "$envf2" | cut -d= -f2-)
check "mint appends the key to a file that lacks it (rc 4 = stopped at the missing container)" '[[ $rc -eq 4 && "$pw2" =~ ^[A-Za-z0-9._~-]{16,}$ ]] && grep -q "^OTHER=1$" "$envf2"'
check "the minted password is in the argv of no external command (mint appends)" '[[ -s "$argv" ]] && ! grep -qF -- "$pw2" "$argv" && ! grep -qF -- "$pw2" "$work/o3b"'
envf3=$work/model3.env; printf 'A=1\r\nBROWSER_BROKER_PG_PASSWORD=\r\nB=2\r\n' > "$envf3"
argv=$work/argv4c.log; : >"$argv"
PATH="$shim:$PATH" ARGV_LOG="$argv" MODEL_PLANE_ENV_FILE="$envf3" bash "$script" --container "no-such-$suffix" --network "$net" --mint-if-missing >"$work/o3c" 2>&1; rc=$?
pw3=$(grep -E '^BROWSER_BROKER_PG_PASSWORD=' "$envf3" | cut -d= -f2- | tr -d '\r')
check "mint into a CRLF file keeps the line endings and the neighbouring keys" '[[ $rc -eq 4 && "$pw3" =~ ^[A-Za-z0-9._~-]{16,}$ && "$(count_cr "$envf3")" == 3 ]] && has_line "$envf3" "$crA" && has_line "$envf3" "$crB"'
check "the minted password is in the argv of no external command (CRLF file)" '[[ -s "$argv" ]] && ! grep -qF -- "$pw3" "$argv" && ! grep -qF -- "$pw3" "$work/o3c"'

# 5. the migration-style DDL works as the broker role (it owns the database)
docker exec -i -e PGPASSWORD="$pw" "$pg" sh -c 'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U browser_broker -d browser_broker -c "CREATE TABLE IF NOT EXISTS t_probe (id int); DROP TABLE t_probe"' >/dev/null 2>&1; rc=$?
check "browser_broker can create and drop a table in public" '[[ $rc -eq 0 ]]'

# 6. second run: idempotent, nothing recreated, password unchanged
before=$(sha256sum "$envf" | cut -d' ' -f1)
run >"$work/o4" 2>&1; rc=$?
after=$(sha256sum "$envf" | cut -d' ' -f1)
check "second run rc 0, nothing created, env file unchanged" '[[ $rc -eq 0 && "$before" == "$after" ]] && grep -q "role browser_broker: f" "$work/o4" && grep -q "database browser_broker: f" "$work/o4"'
check "second run does not print the password" '! grep -qF -- "$pw" "$work/o4"'

# 6b. --mint-if-missing never overwrites a present value: the env file is byte-identical, rc 0, no output of the value
before=$(sha256sum "$envf" | cut -d' ' -f1)
argv=$work/argv6b.log; : >"$argv"
PATH="$shim:$PATH" ARGV_LOG="$argv" run --mint-if-missing >"$work/o4b" 2>&1; rc=$?
after=$(sha256sum "$envf" | cut -d' ' -f1)
check "--mint-if-missing with a present password: rc 0 and the env file is unchanged" '[[ $rc -eq 0 && "$before" == "$after" ]] && ! grep -q "minted" "$work/o4b"'
check "--mint-if-missing with a present password prints and passes it nowhere" '! grep -qF -- "$pw" "$work/o4b" && [[ -s "$argv" ]] && ! grep -qF -- "$pw" "$argv"'

# 7. rotation: a new value in the env file is applied by the next run
newpw=rotated0123456789abcdef
sed -i "s|^BROWSER_BROKER_PG_PASSWORD=.*|BROWSER_BROKER_PG_PASSWORD=$newpw|" "$envf"
run >"$work/o5" 2>&1; rc=$?
check "rotation: rc 0 and login works with the new password" '[[ $rc -eq 0 ]] && grep -q "verified login: browser_broker@browser_broker" "$work/o5"'
check "rotation: the old password no longer logs in" '! docker exec -i -e PGPASSWORD="$pw" "$pg" sh -c '"'"'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U browser_broker -d browser_broker -c "select 1"'"'"' >/dev/null 2>&1'

# 8. isolation. The claim is exactly this: ordinary roles cannot open browser_broker, the browser_broker role
#    cannot open Quarry's databases, and the container superuser (what Quarry uses) CAN open browser_broker.
other=$(date +%s%N | sha256sum | cut -c1-24)
sql "CREATE ROLE other_role LOGIN PASSWORD '$other'" >/dev/null
docker exec -i -e PGPASSWORD="$other" "$pg" sh -c 'psql -X -tA -h "$(hostname -i | cut -d" " -f1)" -U other_role -d browser_broker -c "select 1"' >/dev/null 2>&1; rc=$?
check "another ordinary role cannot connect to browser_broker (CONNECT revoked from PUBLIC)" '[[ $rc -ne 0 ]]'
check "browser_broker connects to its own database" 'tcp "$newpw" browser_broker browser_broker'
check "browser_broker cannot connect to a Quarry-style database (CONNECT revoked from PUBLIC there)" '! tcp "$newpw" browser_broker quarry_probe'
check "browser_broker cannot connect to the admin database either" '! tcp "$newpw" browser_broker ingestion_plane_db'
check "KNOWN LIMITATION: the superuser ingestion_user (every Quarry service) can still connect to browser_broker" 'tcp adminpw0123456789 ingestion_user browser_broker'
sql "CREATE DATABASE quarry_probe2" >/dev/null
run --keep-public-connect >"$work/o6" 2>&1; rc=$?
check "--keep-public-connect leaves a database that was created later as it was (rc 0)" '[[ $rc -eq 0 ]] && grep -q "CONNECT left as it was" "$work/o6" && tcp "$newpw" browser_broker quarry_probe2'
run >"$work/o7" 2>&1; rc=$?
check "a plain re-run closes it again (rc 0)" '[[ $rc -eq 0 ]] && ! tcp "$newpw" browser_broker quarry_probe2'

if [[ $fails -eq 0 ]]; then echo "ALL PASSED"; exit 0; fi
echo "$fails FAILED"; exit 1
