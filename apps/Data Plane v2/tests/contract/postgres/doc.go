// Package postgrescontract holds the Data Plane's Postgres contract tests: they
// apply the REAL rules, never a stand-in for them.
//
//   - The schema is built the way production builds it: infra/postgres/init.sql
//     (what docker-entrypoint-initdb runs once), then every migration in
//     infra/postgres/migrations applied in version order by the real
//     tools/migrator binary, built from source and driven through its own
//     command line (up, down, status).
//   - Row-level security is exercised through the real shared/go/orgscope helper
//     (set_config('app.current_org', ...) then SET LOCAL ROLE dataplane_app) as
//     the real NOLOGIN NOBYPASSRLS runtime role. The services connect as a
//     superuser, so RLS is inert on any other path; a test that does not drop to
//     dataplane_app proves nothing about isolation.
//
// # Files
//
//	migration_static_test.go       reads the migration files; needs no database
//	                               and always runs. Pins the vocabulary, the
//	                               idempotency rules, the rollback and the
//	                               decisions CR-T7/T8/T9a/T11 build on.
//	context_provenance_db_test.go  runs against a disposable Postgres 16:
//	                               up/down/up, the legacy backfill, every CHECK,
//	                               the coverage view, org isolation.
//	harness_test.go                scratch databases, the migrator, fixtures.
//	live_verify.sql                read-only checks to run on the live database
//	                               after CR-T6 is applied (see below).
//
// # Running the database tests
//
// The tests create and drop their own scratch databases, so the server only has
// to be a disposable Postgres 16 whose TEST_DATABASE_URL role is a superuser
// (CREATE DATABASE, CREATE ROLE, CREATE EXTENSION) and whose database name ends
// in _test. The host must be local. Without TEST_DATABASE_URL the database
// tests skip locally and FAIL when REQUIRE_CONTRACT or CI is set.
//
//	docker run -d --rm --name dp-contract-pg \
//	  -e POSTGRES_USER=dataplane -e POSTGRES_PASSWORD=dataplane -e POSTGRES_DB=dataplane_test \
//	  -p 127.0.0.1:55432:5432 postgres:16.14-alpine3.24
//	until docker exec dp-contract-pg pg_isready -U dataplane -d dataplane_test; do sleep 1; done
//
//	cd "apps/Data Plane v2/tests/contract/postgres"
//	TEST_DATABASE_URL='postgres://dataplane:dataplane@127.0.0.1:55432/dataplane_test?sslmode=disable' \
//	  REQUIRE_CONTRACT=1 go test -count=1 -v ./...
//
//	docker stop dp-contract-pg
//
// The password above belongs to a throwaway container that is removed on stop.
// Never point TEST_DATABASE_URL at a database that holds real data.
//
// # Verifying CR-T6 on the live database
//
// After the Data stage applies the migration, run live_verify.sql read-only and
// expect every row to say PASS:
//
//	docker exec -i data-plane-v2-postgres-1 psql -U dataplane -d dataplane -X -A \
//	  < "apps/Data Plane v2/tests/contract/postgres/live_verify.sql"
package postgrescontract
