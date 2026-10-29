#!/usr/bin/env bash
# Runs the database tests against a throwaway Postgres container.
#
# The container gets a unique name and a random host port on 127.0.0.1, and a
# trap removes it when the script exits, however it exits. The password is random
# per run, handed to Docker by name, and never printed. The database tests run
# with ANBUD_REQUIRE_DB_TESTS=1, so an unreachable database fails the run rather
# than skipping its tests. Before them it runs scripts/roles.sql twice under the
# container's own psql, the way a DBA runs it, so a script that psql rejects fails here,
# and once more against the server's own database, which the script must refuse.
#
# Usage: scripts/test-db.sh [package ...]
#   With no arguments it runs the database packages listed below.
set -euo pipefail

readonly IMAGE="postgres:16.14-alpine3.24"
readonly ADMIN_DB="postgres"
readonly DEFAULT_PACKAGES="./internal/db/ ./internal/rawstore/ ./internal/docstore/ ./cmd/doffin-sync/ ./cmd/doffin-probe/ ./cmd/api/ ./cmd/anbud-retention/"

# Run from the module root, whatever the caller's directory.
cd "$(dirname "$0")/.."

if ! command -v docker >/dev/null 2>&1; then
	echo "test-db: docker is required to run the database tests" >&2
	exit 1
fi

# A random suffix keeps two runs on one host from sharing a container name.
suffix="$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
name="anbud-test-db-${suffix}"
# The hex password goes to docker by name (-e NAME), so its value stays out of
# the argument list.
POSTGRES_PASSWORD="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
export POSTGRES_PASSWORD

cleanup() {
	docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 1' INT TERM

echo "test-db: starting ${IMAGE} as ${name}"
docker run --detach --name "$name" \
	-e POSTGRES_PASSWORD \
	-p 127.0.0.1::5432 \
	"$IMAGE" >/dev/null

# The official image runs a temporary server during first-time setup, so wait for
# TCP readiness rather than the first socket answer.
ready=""
for _ in $(seq 1 60); do
	if docker exec "$name" pg_isready -h 127.0.0.1 -p 5432 -U postgres -q >/dev/null 2>&1; then
		ready=1
		break
	fi
	sleep 1
done
if [ -z "$ready" ]; then
	echo "test-db: the database did not become ready in time" >&2
	exit 1
fi

host_port="$(docker port "$name" 5432/tcp | awk -F: 'NR==1 {print $NF}')"
if [ -z "$host_port" ]; then
	echo "test-db: could not find the published port" >&2
	exit 1
fi

# scripts/roles.sql is what a DBA runs, with psql. The Go role tests expand its one psql
# variable themselves and run the result through pgx, and psql is not needed on the
# host, so this runs the file under the real psql in the container, twice, the way a
# DBA would, on a scratch database. The scratch database and the roles are removed
# again before the Go tests start, so they begin on a cluster without the roles. The
# container's psql reaches the server over its Unix socket, which the image trusts.
smoke_db="anbud_test_psql_${suffix}"
psql_in() { docker exec -i "$name" psql -X -q -v ON_ERROR_STOP=1 -U postgres "$@"; }
psql_in -d "$ADMIN_DB" -c "CREATE DATABASE ${smoke_db} TEMPLATE template0" >/dev/null
for run in 1 2; do
	if ! psql_in -d "$smoke_db" -v dbname="$smoke_db" -f - <scripts/roles.sql >/dev/null; then
		echo "test-db: scripts/roles.sql failed under psql (run ${run} of 2)" >&2
		exit 1
	fi
done
if ! psql_in -d "$ADMIN_DB" -c "DROP DATABASE ${smoke_db} WITH (FORCE)" \
	-c "DROP ROLE anbud_app, anbud_dba, anbud_migrator" >/dev/null; then
	echo "test-db: could not remove the scratch database and the roles after the psql run" >&2
	exit 1
fi
echo "test-db: scripts/roles.sql ran twice under psql"

# The script must refuse a database whose name does not start with anbud, before it changes
# anything: the roles have fixed names and belong to the whole cluster. The server's own database
# is such a database. A script that does not refuse would prepare it and leave the roles
# behind, which the count below finds; the container goes with the run either way.
if psql_in -d "$ADMIN_DB" -v dbname="$ADMIN_DB" -f - <scripts/roles.sql >/dev/null 2>&1; then
	echo "test-db: scripts/roles.sql prepared the database ${ADMIN_DB}, whose name does not start with anbud" >&2
	exit 1
fi
roles_left="$(psql_in -d "$ADMIN_DB" -tA -c "SELECT count(*) FROM pg_roles WHERE rolname IN ('anbud_migrator', 'anbud_app', 'anbud_dba')")"
if [ "$roles_left" != "0" ]; then
	echo "test-db: scripts/roles.sql, refused for the database ${ADMIN_DB}, still made ${roles_left} roles" >&2
	exit 1
fi
echo "test-db: scripts/roles.sql refused a database that is not named anbud"

# sslmode=disable is safe here: the host is 127.0.0.1, which the DSN check treats as local.
test_dsn="postgres://postgres:${POSTGRES_PASSWORD}@127.0.0.1:${host_port}/${ADMIN_DB}?sslmode=disable"

if [ "$#" -gt 0 ]; then
	packages="$*"
else
	packages="$DEFAULT_PACKAGES"
fi

# shellcheck disable=SC2086
# -p 1 runs one package at a time, because the migration tests share one server. -skip is anchored:
# a plain Live would also skip every test whose name merely contains the word.
ANBUD_TEST_DSN="$test_dsn" \
ANBUD_REQUIRE_DB_TESTS=1 \
GOTOOLCHAIN="${GOTOOLCHAIN:-local}" \
	go test -race -count=1 -p 1 -skip '^TestLive' $packages
