# R3-ws1c progress (WS1.C Postgres grant store)

Design: wf-R3-ws1c-design.md (written first). Compose lines: wf-R3-ws1c-compose.md.

## Committed
- 26f1c679 feat(browser-broker): Postgres grant store and migration 0001_browser_grants (WS1.C)
- 7c5a564b feat(browser-broker): route ZDR grants to memory and the rest to Postgres; DATABASE_URL is required (WS1.C)

- 669fde75 test(browser-broker): Postgres grant store integration suite, including the WS1.B Derive contract (WS1.C)

## Uncommitted (mine)
- none

## Results
- host: GOWORK=off go vet ./... ; go vet -tags integration ./... ; go test -count=1 ./... -> all ok
- integration (testcontainers WORKS on this Docker Desktop, under with-lock.sh): 70 PASS lines, 0 FAIL, rc=0
  (16 contract subtests on Routed{PgStore,memory} + all 10 named TestPg_* + 3 race subtests + ClosedPool)
- mutation: no advisory lock in Derive -> TestPg_PerOwnerCapIsExactUnderConcurrency FAILS (good);
  no FOR UPDATE root lock in revoke -> race test failed by 10-min hang (test bug on failure path: select
  consumed the channel value) -> fixed (re-send), re-running

## Status
UNIT COMPLETE for owned paths. Hand-over: wf-R3-ws1c.md. Mutation 1 re-run after the fix: fails fast (0.9 s) with "run grant outlived its revoked parent"; restored suite 70 PASS rc=0.

## ROUND 3 (review fixes) - 2026-10-19
Uncommitted (mine, all under apps/Model Plane/go/services/browser-broker): internal/grant/{postgres.go,grant.go,postgres_integration_test.go,postgres_store_test.go(new)}, internal/server/{server.go,errors.go,grants_policy_test.go}, cmd/{main.go,main_test.go(new)}, go.mod (+otel/sdk/metric v1.43.0), go.sum.
Done in tree: NUL/invalid-UTF8 (truncateReason strips, validText -> ErrInvalidGrantText -> InvalidArgument, AcquireGrant session_key bounded+printable); revoke deadlock (advisory lock on revoke root + inTx retries 40P01/40001, storeError type); tests for all findings; main.go extracted startupGate/newPool/readyzHandler/purgeOnce.
Host results: go vet ./... and -tags integration ok; go test -count=1 ./... all ok.
Integration run queued behind docker lock: log scratchpad/R3-ws1c/int1.log (end marker INT_DONE). Then: commit in 2-3 steps with qcommit.sh.
ROUND 3 COMPLETE: committed 04695ae8 (fix: revoke advisory lock + retry, NUL text, cmd refactor) and 9867a8be (tests). Mutants M1-M6 all killed (M1b: retry alone masks the deadlock). Integration 77 PASS 0 FAIL.

## ROUND 3b (second review: 3 minor findings) - 2026-10-19
Uncommitted (mine): internal/grant/{postgres.go (deadlockRetries atomic counter in inTx), postgres_integration_test.go (revoke race asserts zero deadlock retries), postgres_store_test.go (meter provider installed in init, guard against replacement)}.
Finding 1 (lock before org check): no action, by design (result is NotFound, brief contention only).
Host: go vet (plain + integration tag) and go test -count=1 ./... ok. Integration + mutant M1b run: scratchpad/R3-ws1c/run4.log (end marker RUN4_DONE). Then commit with qcommit.sh.
