# WS1.C hand-over (unit R3-ws1c): Postgres grant store of browser-broker

Branch quarryv2-next, local commits only:
- 26f1c679 feat(browser-broker): Postgres grant store and migration 0001_browser_grants
- 7c5a564b feat(browser-broker): route ZDR grants to memory and the rest to Postgres; DATABASE_URL is required
- 669fde75 test(browser-broker): Postgres grant store integration suite, including the WS1.B Derive contract

Design: wf-R3-ws1c-design.md. Compose lines to apply: wf-R3-ws1c-compose.md.

## What exists
- migrations/0001_browser_grants.{up,down}.sql (idempotent, no zdr column, z_ CHECK, 4 named indexes)
- internal/grant/postgres.go PgStore; router.go Routed{persistent, ephemeral}; MemoryRepository.PurgeExpired
- cmd/main.go: DATABASE_URL required unless BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true; /readyz pings DB;
  purge every 10 min (expired > 24 h)
- Dockerfile + docker-entrypoint.sh (migrations applied on each start, sandbox-manager pattern)
- browser_broker_grant_store_errors_total{op}; ErrStoreUnavailable -> gRPC Unavailable

## Verified (commands actually run)
- host: `GOWORK=off go vet ./...`, `go vet -tags integration ./...`, `go test -count=1 ./...` all ok
- `GOWORK=off go test -count=1 -tags integration -v ./internal/grant/` with testcontainers under
  with-lock.sh docker: 70 PASS, 0 FAIL (twice)
- mutations: dropping the per-parent advisory lock / the revoke root FOR UPDATE both make tests fail

## Traps
- A broker image rebuilt from 7c5a564b on WITHOUT compose DATABASE_URL exits 1 at boot. Apply
  wf-R3-ws1c-compose.md first.
- `go mod tidy` alone resolves testcontainers to v0.44.0 and bumps otel; pin with
  `GOWORK=off go get ...@v0.42.0` first.
- RevokeScoped must stay two statements (FOR UPDATE root, then WITH RECURSIVE update): a single statement
  takes its snapshot before waiting on an in-flight Derive and misses the child (proven by mutation).
- A ZDR-raised approval on a non-ZDR parent is now refused (ErrZdrPostureChanged) behind Routed; the
  memory-only broker still allows it. Intentional: no column can record it and the cascade would break.
- Approvals lock the chain top-down (root session FOR SHARE, then parent) to avoid a lock cycle with revoke.
- testcontainers works on this Docker Desktop (Ryuk ok). TEST_DATABASE_URL is the fallback hook.

## Left
- .github/workflows/browser-broker-ci.yml job `integration`
- compose edits (outside unit), then the deployment-evidence run (rebuild broker, restart, psql row,
  ZDR /step 403 after restart, L2 legs A-C)
- dedicated database + least-privilege role (hardening follow-up per plan risks)

## Round 3 review fixes (2026-10-19)
- 04695ae8: RevokeScoped serialized per root session by pg_advisory_xact_lock (ns 0x62620003) + inTx retries 40P01/40001 (3 attempts); NUL/invalid UTF-8 handled (truncateReason strips, ErrInvalidGrantText -> InvalidArgument, AcquireGrant session_key bounded/printable); cmd/main.go extracted startupGate/newPool/readyzHandler/purgeOnce.
- 9867a8be: tests for every finding; mutants verified with go test -overlay (scratchpad/R3-ws1c/mut).
- Trap: the retry alone makes the deadlock invisible in tests (only 40P01 log lines show it); the advisory lock is what removes it. Heredocs containing backslash escapes into JS templates turned \x00 into real NUL bytes: write such scripts with the Write tool.
