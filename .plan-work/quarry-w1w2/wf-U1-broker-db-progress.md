# U1-broker-db progress (updated while running)
Design: dedicated db+role browser_broker on ingestion-postgres; network browser-broker-db (OWNED by Ingestion compose, external in Model Plane); BROWSER_BROKER_PG_PASSWORD in Model Plane deploy/.env.
UNCOMMITTED, all mine (edited, none committed yet):
- apps/Ingestion Plane/docker-compose.yml (postgres joins browser-broker-db; network defined), init-databases.sql (comment only), scripts/bootstrap_broker_db.sh (new)
- apps/Model Plane/deploy/docker-compose.yml (DATABASE_URL, network), deploy/.env.example, scripts/release-artifact.sh (RUNTIME_SECRET_KEYS)
- .github/workflows/browser-broker-ci.yml (integration + bootstrap-db jobs, path triggers)
- scripts/tests/bootstrap-broker-db.test.sh (new), scripts/tests/browser-grant-contract.test.mjs (+1 test; 16/16 pass)
Results so far: compose config renders (model base+override, model prod overlay, ingestion base+prod) with dummy values; missing password -> clear :? error. Image browser-broker-u1:test built (rc 0).
Running (under docker lock): scratchpad/U1-broker-db/bstest.log (BS_DONE) then e2e.log (E2E_FINISHED). Then: host run of CI integration cmd (GOWORK=off go test -tags integration ./internal/grant/...) under lock; then deploy doc wf-U1-broker-db-deploy.md; commit in 3 steps via qcommit.sh.

## Update
- COMMITTED 1c5f000a feat(compose): ... (Model Plane compose + .env.example + release-artifact.sh + Ingestion compose + init-databases.sql comment + contract test)
- Uncommitted, mine: apps/Ingestion Plane/scripts/bootstrap_broker_db.sh, scripts/tests/bootstrap-broker-db.test.sh (both new; stage with git add --chmod=+x first, core.fileMode=false), .github/workflows/browser-broker-ci.yml
- Background chain (scratchpad/U1-broker-db/chain.sh) queued behind another agent's cargo job on the docker lock: bstest2.log (BS_DONE), e2e2.log (E2E_FINISHED), label.log (LABEL_FINISHED, tests whether compose adopts a hand-made labelled network), integ.log (INTEG_FINISHED, CI integration cmd on host). First bstest run had 1 FAIL (--check with no password; fixed in script).
- Deploy doc draft written: wf-U1-broker-db-deploy.md (rollback section needs tidying; Evidence section to fill)
- First e2e run showed: entrypoint migration works (browser_grants created), empty DATABASE_URL exits 1 with documented message; the broker needs a reachable JWKS at boot (stub JWKS container added to e2e.sh).

## Update 2 (17:58)
- COMMITTED: 1c5f000a (compose etc.), e25d9ec0 (bootstrap script + test), e7e9b9e1 (chmod +x of both).
- bootstrap test: 19/19 pass, run on the final test file (bstest3.log).
- UNCOMMITTED, mine: .github/workflows/browser-broker-ci.yml (msg in scratchpad/U1-broker-db/msg3.txt). Commit BLOCKED: Docker Desktop engine answers 500 (the pre-commit gitleaks hook runs through docker; fail-closed). Retry: bash scratchpad/tools/qcommit.sh scratchpad/U1-broker-db/msg3.txt .github/workflows/browser-broker-ci.yml
- Docker engine 500 since ~17:53: e2e4 (broker image vs throwaway pg with a stub JWKS) never ran; integ2 (CI integration cmd) never ran. I killed my e2e.sh processes and removed my stale docker lock. Throwaway containers u1-*/bbdb-* may remain if the kill came mid-run: `docker ps -a | grep -E "u1-|bbdb-|u1lbl"` then docker rm -f / docker network rm.
- e2e legs OBSERVED in the first runs (e2e.log/e2e2.log): bootstrap creates role+db, TCP login verified; image entrypoint migration created browser_grants in browser_broker; empty DATABASE_URL -> exit 1 with the documented message. NOT observed: /readyz 200, kill-pg -> readyz fails, ephemeral flag boot (broker needs a reachable JWKS at boot; first stub (busybox httpd) did not exist in postgres:16-alpine, e2e.sh now uses python:3.12-alpine http.server).

## Round 4 (review fixes) - in progress
Done in the tree (UNCOMMITTED unless noted; scratch scripts in scratchpad/U1-broker-db/r4-*.js, mutate.mjs, mint-harness.sh):
- bootstrap_broker_db.sh: mint without argv (builtins + cat), lockdown REVOKE CONNECT on other DBs (--keep-public-connect), honest isolation header.
- bootstrap-broker-db.test.sh: 33 checks (PATH shims argv log, mint-present no-op, lockdown, superuser KNOWN LIMITATION). NOT run on real postgres: Docker 500/timeout all round.
- init-databases.sql + migration 0001 comment reworded (comment only).
- scripts/bootstrap_runtime_environment.sh: ensure_secret BROWSER_BROKER_PG_PASSWORD. Root bootstrap test: BROWSER_BROKER_PG_PASSWORD no longer in the Model unprovisioned list (4 pre-existing remain; mode-600/stat failures are host-specific).
- browser-grant-contract.test.mjs: EPHEMERAL_FLAG regex fixed (\s+), membership parser (list/map/flow, all compose overlays), parser self-test; mutate.mjs: control PASS, 7 mutations all FAIL as wanted. node --test: 19/19.
- browser-broker-ci.yml: path filters add compose globs, .env.example, release-artifact.sh, bootstrap_runtime_environment.sh; comment states the superuser limit.
- deploy notes updated.
COMMIT: staged (not committed) group A paths; qcommit blocked: gitleaks hook runs through docker and docker is down (hook prints "found a secret" for docker exit 1: misleading). Message: scratchpad/U1-broker-db/msg-r4a.txt. Waiting for docker (dockerwait.sh). Group B (browser-grant-contract.test.mjs [file also carries ANOTHER agent's uncommitted WS6.E budget hunks: commit only HEAD+my edit via a temp index], bootstrap_runtime_environment.sh, browser-broker-ci.yml) not committed yet.

## Round 4b (verifier findings) - Docker engine still 500; NOTHING COMMITTED
Done in the tree: header/usage/SQL comment/echo of bootstrap_broker_db.sh and init-databases.sql say the lockdown covers only databases that exist when the script runs (re-run after creating one); deploy note updated; msg-r4a.txt now opens with a CORRECTION paragraph naming the false claims in 1c5f000a/e25d9ec0 (password in argv, "no other role can open it", dead ephemeral-flag regex). bash -n OK; mint-harness 8/8 OK. HEAD blob + r4-l1-edit applied to the contract test: no budget hunks, U1's own tests pass (the 2 proto tests that fail in that variant read the working-tree proto of another unit).
Commit attempts: qcommit (group A) blocked by the pre-commit hook: docker engine 500 -> hook reports docker exit 1 as "found a secret" (misleading). Docker still down after a ~17 min wait.
RUN WHEN DOCKER IS UP, in this order:
  1. bash C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/tools/qcommit.sh C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/U1-broker-db/msg-r4a.txt "apps/Ingestion Plane/scripts/bootstrap_broker_db.sh" "apps/Ingestion Plane/init-databases.sql" "apps/Model Plane/go/services/browser-broker/migrations/0001_browser_grants.up.sql" scripts/tests/bootstrap-broker-db.test.sh
  2. bash C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/U1-broker-db/commit-b.sh   (temp index: HEAD contract test + U1 edit only, bootstrap_runtime_environment.sh, browser-broker-ci.yml)
  3. bash scripts/tests/bootstrap-broker-db.test.sh   (33 checks, needs Docker; never run on real postgres this round)
