# U1 deploy steps: browser-broker on Quarry's Postgres (for you to approve and run)

Nothing below has been run against the live stack. Everything was proven on throwaway containers (see "Evidence").
Run from the MAIN checkout `C:\dev\CoresSystem` (the live stacks bind-mount from it) once the U1 commits are in it,
in Git Bash. Secrets: nothing here prints a password; do not paste one into a terminal either.

## What changed (so you know what you are approving)

- Dedicated database `browser_broker` and role `browser_broker` on `ingestion-postgres` (not `quarry_edge`, not `quarry_v2`).
  The database is owned by the role and CONNECT is revoked from PUBLIC, so no ORDINARY role can open it. The script also revokes
  PUBLIC CONNECT on every other non-template database that exists when it runs (Quarry's; a database created LATER keeps the
  default PUBLIC CONNECT until the script is re-run), so the `browser_broker` role cannot log in to
  them (`--keep-public-connect` skips that). Every Quarry service uses `ingestion_user`, which is the container superuser;
  other roles on this server: none that Quarry uses (connector-runtime has its own Postgres instance).
- This is a deliberate SHARED Postgres SERVER across two planes (Model Plane data on Ingestion Plane's server). The separation is
  by database and role ONLY, it is not a security boundary against Quarry: a superuser ignores the CONNECT revoke, so Quarry's
  `ingestion_user` (and anything else running as a superuser) can still read AND WRITE `browser_grants` directly, including
  forging a grant row or changing the `browser_broker` password. The broker's derived-grant rules (domain narrowing, TTL, org
  binding) therefore protect against callers of the broker, not against a compromised Quarry service with database access.
  The repo rule is no direct database crossing between planes: this layout does not cross it in code (Quarry never queries
  `browser_grants`), but the server can. To close it for real: give the broker its own Postgres instance (same URL shape, change
  the host and drop the bootstrap/network steps), or run Quarry as a non-superuser role and add a pg_hba.conf line such as
  `host browser_broker all <ingestion-net subnet> reject`. `scripts/tests/bootstrap-broker-db.test.sh` pins this limit
  ("KNOWN LIMITATION" check) so it cannot be forgotten.
  Side effects of sharing: Quarry's Postgres backups and the 256 MB container limit now cover this database; the role is
  capped at 40 connections of the server's 100.
- New docker network `browser-broker-db`, owned by the Ingestion Plane compose, containing only `ingestion-postgres` and
  `model-plane-browser-broker-1`. The broker is NOT put on `ingestion-net`, so it cannot reach Dragonfly, Temporal or
  Quarry's other services.
- New secret `BROWSER_BROKER_PG_PASSWORD` in `apps/Model Plane/deploy/.env` (URL-safe, 16+ characters). It was added to
  `deploy/.env.example` (empty) and to `RUNTIME_SECRET_KEYS` in `scripts/release-artifact.sh` (a signed release otherwise
  refuses an undeclared credential variable).
- `apps/Ingestion Plane/scripts/bootstrap_broker_db.sh`: idempotent creator of the role and database (init-databases.sql
  only runs on a fresh volume and cannot read the password). `--check` is read-only.

## IMPORTANT ordering trap

`BROWSER_BROKER_PG_PASSWORD` uses `:?` in the compose file, like `MODEL_POSTGRES_PASSWORD`. Until the key is in
`apps/Model Plane/deploy/.env`, EVERY `scripts/compose.sh` command for ANY Model Plane service fails at interpolation
("BROWSER_BROKER_PG_PASSWORD is required"). So do step 2 before you run compose.sh for anything.
Also: the rebuilt broker image exits 1 at boot without `DATABASE_URL`, so never rebuild it before steps 1 to 3.

## Steps

0. Remember what runs now (rollback anchor) and look, read-only:
   ```
   docker inspect -f '{{.Image}} {{.State.Status}}' model-plane-browser-broker-1
   bash "apps/Ingestion Plane/scripts/bootstrap_broker_db.sh" --check
   ```
   Expect: role/database `missing`, network `browser-broker-db` not attached (or the network is missing).

1. Make the network exist and attach `ingestion-postgres` to it. Pick ONE:

   A. (recommended if a short Quarry DB blip is acceptable) let the Ingestion compose do it. It recreates only
      `ingestion-postgres` (same volume, healthcheck 10 s); Quarry services reconnect:
      ```
      docker compose -p ingestion-plane \
        --env-file C:/dev/CoresSystem/.env \
        --env-file "apps/Control Plane/.env.generated-secrets" --env-file "apps/Application Plane/.env.generated-secrets" \
        --env-file "apps/Ingestion Plane/Quarry-v2/.env" --env-file "apps/Ingestion Plane/autocomplete-core/.env" \
        --env-file "apps/Ingestion Plane/finspo-core/.env" --env-file "apps/Ingestion Plane/imports-core/.env" \
        --env-file "apps/Ingestion Plane/integration-corev2/.env" --env-file "apps/Ingestion Plane/shipping-core/.env" \
        --env-file "apps/Ingestion Plane/.env.generated-secrets" \
        -f "apps/Ingestion Plane/docker-compose.yml" up -d --no-deps postgres
      ```
      (the env-file chain is the one in the ingestion-plane-deploy-recipe memory note; keep its order.)

   B. (no restart of ingestion-postgres) create the network by hand with the labels compose expects; step 2 then attaches
      the running container with a hot `docker network connect` (Quarry is not interrupted):
      ```
      docker network create --label com.docker.compose.network=browser-broker-db \
        --label com.docker.compose.project=ingestion-plane browser-broker-db
      ```
      (`ingestion-plane` is the compose project label the running ingestion-postgres carries today; confirm with
      `docker inspect -f '{{index .Config.Labels "com.docker.compose.project"}}' ingestion-postgres`.)
      Proven on a throwaway compose project (Docker Compose 5.4.0): with these labels a later `compose up` adopts the network
      silently; without the labels it still works but warns "a network with name ... exists but was not created by compose".
      `compose down` of the Ingestion stack removes a compose-owned network (refused while the broker is attached).

2. Add the secret, then create role and database (idempotent; run it as often as you like):
   ```
   bash "apps/Ingestion Plane/scripts/bootstrap_broker_db.sh" --mint-if-missing
   ```
   - `--mint-if-missing` generates a random hex password into `apps/Model Plane/deploy/.env` ONLY if the key is absent or empty
     (it never overwrites a value; the value is never shown). Without the flag, put the key in `.env` yourself first.
   - It attaches the container to `browser-broker-db` if it is not, creates role/database if missing, always applies the
     password from `.env` (so rotation = edit `.env`, rerun, recreate the broker), revokes PUBLIC CONNECT on the other
     databases (it changes their ACL: `\l` then shows `=T/ingestion_user` instead of `=Tc/...`; nothing else changes because only
     the superuser connects to them), and proves a TCP login as `browser_broker`.
   - The password never reaches a command line: the mint step rewrites the file with shell builtins, and the test records the argv
     of every external command through PATH shims.
   - Expected output: `role browser_broker: t`, `database browser_broker: t`, `verified login: browser_broker@browser_broker`
     (a second run prints `f` twice).

3. Check the compose renders (no containers touched):
   ```
   bash "apps/Model Plane/scripts/compose.sh" config --quiet && echo CONFIG_OK
   ```

4. Rebuild and start the broker ONLY (recreates `model-plane-browser-broker-1`; every in-memory grant of the old process is
   lost once, which is what a deploy of a stateless broker always did; ZDR grants are lost on every restart by design, X-2):
   ```
   bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build browser-broker
   ```

5. Verify:
   ```
   docker ps --filter name=model-plane-browser-broker-1 --format '{{.Status}}'          # (healthy)
   docker logs model-plane-browser-broker-1 2>&1 | grep -c "durable Postgres grant store"   # 1
   docker exec model-plane-browser-broker-1 curl -s -o /dev/null -w '%{http_code}\n' localhost:8087/readyz   # 200
   docker exec ingestion-postgres psql -U ingestion_user -d browser_broker -c '\dt'      # browser_grants
   docker network inspect browser-broker-db -f '{{range .Containers}}{{.Name}} {{end}}'  # exactly the two
   docker exec ingestion-postgres psql -U ingestion_user -d ingestion_plane_db -c '\l' | grep browser_broker
   ```
   Then the Quarry side: `quarry-edge` `/ready` still 200 and a browser `start_run` with a grant still validates
   (the broker address `model-plane-browser-broker-1:9095` on `inter-plane-bus` did not change).

6. Restart-survival check (plan WS1.C "Deployment evidence"), hand-driven with real tokens, no secrets in the log. Use
   counts and ids only; never print `allowed_domains`:
   ```
   Q() { docker exec ingestion-postgres psql -U ingestion_user -d browser_broker -tA -c "$1"; }
   Q "SELECT count(*) FROM browser_grants"                       # n0
   # (a) open a NON-ZDR session through the gateway: POST /v1/browser/grants with the user's bearer
   Q "SELECT id, kind, expires_at FROM browser_grants ORDER BY created_at DESC LIMIT 1"   # the row exists, id has no z_
   docker restart model-plane-browser-broker-1                  # wait for (healthy)
   # /step on the same session -> 200 (grant survived), the row is still there
   # (b) repeat with zdr:true: the count did NOT increase on creation
   Q "SELECT count(*) FROM browser_grants"                       # n0+1 after (a), still n0+1 after (b)
   docker restart model-plane-browser-broker-1
   # /step on the ZDR session -> 403 inactive (memory-only grant lost, by design)
   ```
   Then rerun L2 legs A, B, C against this Postgres-backed broker (plan item WS1.C).

## Rollback

- Back to the previous broker (keeps the database untouched): the image of step 0 has no database support and ignores
  `DATABASE_URL`. Either revert the compose commit and run step 4 again (rebuilds the old source), or run a dev-only
  broker with `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true` and no `DATABASE_URL` (every grant is then lost on restart;
  never in production).
- Remove the data: `docker exec ingestion-postgres psql -U ingestion_user -d ingestion_plane_db -c 'DROP DATABASE browser_broker' -c 'DROP ROLE browser_broker'`
  (stop the broker first; the DROP fails while it is connected). Nothing else uses them.
- `docker network disconnect browser-broker-db ingestion-postgres` is safe once the broker is stopped.

## Production overlay and releases

- `docker-compose.production.yml` (Model Plane) needs no change: the broker keeps its base `DATABASE_URL`.
- A production runtime env file must contain `BROWSER_BROKER_PG_PASSWORD` (the release builder now accepts it as an audited
  secret). The `browser-broker-db` network must exist on the production host before the broker starts.
- The Ingestion Plane production overlay needs no change (`postgres` inherits the network from the base file).
- `scripts/bootstrap_runtime_environment.sh` (root) now mints `BROWSER_BROKER_PG_PASSWORD` into `apps/Model Plane/deploy/.env`
  with the other Model Plane secrets (`ensure_secret`: only when the key is missing or a placeholder, never overwriting; hex,
  so URL-safe, 64 characters). Round 4 added it because `scripts/tests/bootstrap_runtime_environment.test.sh` ("Model Compose
  required variables are bootstrapped") listed it as unprovisioned. That test also fails on four variables that were already
  unprovisioned before U1 (AUTH_CORE_ISSUER, COST_BILLING_CORE_SERVICE_TOKEN, MODEL_NATS_PROVISIONER_PASSWORD,
  SESSION_CORE_GDPR_NATS_PASSWORD) and on host-specific mode-600 checks; those are not U1's. Running the root bootstrap
  against the live ledgers is yours to decide (see the bootstrap-secrets-ledger note); `--mint-if-missing` on the broker
  script does the same job for this one key alone.

## Evidence (all on throwaway containers; the live ingestion-postgres and model-plane-browser-broker-1 were not touched)

Seen (logs in `scratchpad/U1-broker-db/`):
- Image `browser-broker-u1:test` built from the tree at `1c5f000a`'s parent plus these edits (`build.log`, rc 0). The Dockerfile and
  `docker-entrypoint.sh` needed no change.
- `scripts/tests/bootstrap-broker-db.test.sh`: 19 of 19 checks pass (`bstest3.log`, run on the final test file). It covers the
  two-run idempotence, rotation, no password in the output, role not superuser, database owned by the role, other roles refused.
- Broker image against a throwaway postgres on a private network (`e2e.log`, `e2e2.log`): the bootstrap script created the role and
  database and the TCP login worked; the image's entrypoint then applied the migration (`browser_grants` exists in database
  `browser_broker`, seen twice); with `DATABASE_URL=` empty and no ephemeral flag the container exits 1 and logs
  "refusing to start browser-broker without a durable database" (documented message).
- `docker compose config` renders for Model Plane (base+override, base+production) and Ingestion Plane (base, base+production)
  with dummy values; without `BROWSER_BROKER_PG_PASSWORD` it stops with "BROWSER_BROKER_PG_PASSWORD is required".
- Compose and a hand-made network (`label.log`): labelled network adopted silently, unlabelled adopted with a warning.

NOT seen (state this when you approve): the broker image reaching `/readyz` 200, `/readyz` failing after the throwaway postgres is
killed (and recovering), and the ephemeral-flag boot. The broker verifies JWKS at boot, so those legs need a reachable JWKS; my
first stub (busybox httpd in postgres:16-alpine) does not exist, and the corrected run (`e2e.sh`, python http.server) never executed
because Docker Desktop began answering 500 on the engine API. Unit tests for `/readyz` (503 while the store does not answer,
200 otherwise) and the startup gate pass on the host (`GOWORK=off go test ./cmd/... ./internal/...`), and the tagged integration
suite last passed at `5951f200` (77 PASS lines, WS1.C ledger); neither was re-run on this tree's image. Re-run
`bash "C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/side-session-notes/local_6a0cd694-160f-4e6d-9b45-da7a2400e1ec/wf-U1-broker-db-scripts/e2e.sh" (it needs the image browser-broker-u1:test; rebuild with: docker build -t browser-broker-u1:test -f "apps/Model Plane/go/services/browser-broker/Dockerfile" "apps/Model Plane/go")` (under the docker lock) when Docker is healthy.

## Round 4 status (review fixes) - read this before approving

Fixed in the working tree (see wf-U1-broker-db-progress.md "Round 4"): password kept out of argv in the mint step, PUBLIC CONNECT
revoked on Quarry's databases, honest isolation wording (separation by database and role only; Quarry's superuser can still
read and write `browser_grants`), L1 compose guards (dead regex, network membership incl. overlays), root bootstrap mints the key,
CI path filters.

NOT YET COMMITTED and NOT RUN against Postgres: Docker Desktop was down for the whole round (engine API 500/timeouts, >30 min), and
the pre-commit gitleaks hook runs through docker (it prints "found a secret" when `docker run` exits 1, which is the daemon being
down, not a finding). When Docker answers:
1. `bash scripts/tests/bootstrap-broker-db.test.sh` (needs Docker; expects 33 checks to pass; the DB-level ones, lockdown REVOKE,
   --keep-public-connect, superuser limitation and present-password no-op, have never executed).
2. Commit group A: `bash <scratchpad>/tools/qcommit.sh <scratchpad>/U1-broker-db/msg-r4a.txt "apps/Ingestion Plane/scripts/bootstrap_broker_db.sh" scripts/tests/bootstrap-broker-db.test.sh "apps/Ingestion Plane/init-databases.sql" "apps/Model Plane/go/services/browser-broker/migrations/0001_browser_grants.up.sql"`
3. Commit group B: `bash <scratchpad>/U1-broker-db/commit-b.sh` (temp index: contract test = HEAD + U1's edit only, so another unit's uncommitted budget hunks stay out; plus bootstrap_runtime_environment.sh and browser-broker-ci.yml).
The step list above (bootstrap, compose, verify) is unchanged except that step 2 now also revokes PUBLIC CONNECT on the other databases.

## DEPLOYED 2026-10-20 (live dev stack) - what was actually done

- Source: built from the WORKTREE (branch quarryv2-next, HEAD e7c321d5), not the main checkout (main has no U1 commits). Compose was run by hand from the worktree files with the main checkout's env ledgers (`apps/Control Plane/.env.generated-secrets`, `apps/Model Plane/deploy/.env`), project `model-plane`, `up -d --no-deps --build browser-broker`. Only `model-plane-browser-broker-1` was recreated.
- Step 1 option B: network `browser-broker-db` created by hand with the compose labels (project ingestion-plane); ingestion-postgres was NOT restarted, attached hot by the bootstrap script.
- Step 2: `BROWSER_BROKER_PG_PASSWORD` minted into `C:/dev/CoresSystem/apps/Model Plane/deploy/.env` (one line added, 380 -> 381 lines, nothing else changed). Role and database browser_broker created; login verified; second run reported f/f (idempotent).
- Verified: broker healthy, log "durable Postgres grant store", /readyz 200, `browser_grants` table in database browser_broker, network members exactly ingestion-postgres + the broker, quarry-edge /ready 200, one `docker restart` of the broker came back healthy with the table intact (0 grants before and after).
- NOT run: the grant-level restart-survival check (open a session through the gateway with a user bearer, restart, /step still 200; ZDR session -> 403) and L2 legs A-C: they need a real user token / service token that I do not have. Unit tests and the tagged Postgres integration suite cover the store semantics.
- TRAPS: (1) the main checkout's Ingestion compose does not define `browser-broker-db`: a `compose up` of ingestion-postgres from the main checkout recreates it WITHOUT the network and the broker loses its database (readyz 503) until `docker network connect browser-broker-db ingestion-postgres`. Merging the branch (or copying the compose hunk) fixes that. (2) The main checkout's Model Plane compose still builds the OLD broker: a `compose up --build browser-broker` from main would replace this one with a broker that ignores DATABASE_URL.
- Rollback: old image kept as `model-plane-browser-broker:rollback-20261020` (sha256:3c0a03e2f146...). To go back: stop broker, `docker tag model-plane-browser-broker:rollback-20261020 model-plane-browser-broker:latest`, `compose up -d --no-deps --no-build browser-broker` from main (ignores DATABASE_URL). Data removal as in the Rollback section above.
