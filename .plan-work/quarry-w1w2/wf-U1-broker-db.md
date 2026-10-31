# U1-broker-db hand-over

Deploy steps for you to approve and run: `wf-U1-broker-db-deploy.md` (same folder). Throwaway proof scripts: `wf-U1-broker-db-scripts/`.

## Done (branch quarryv2-next)
- 1c5f000a feat(compose): `DATABASE_URL` for browser-broker on a dedicated database and role `browser_broker` on ingestion-postgres,
  new Postgres-only network `browser-broker-db` (owned by the Ingestion compose, external in Model Plane), `BROWSER_BROKER_PG_PASSWORD`
  in `.env.example` and in the release secret allowlist, one L1 contract test.
- e25d9ec0 + e7e9b9e1 `apps/Ingestion Plane/scripts/bootstrap_broker_db.sh` (idempotent, never prints the password, `--check`,
  `--mint-if-missing`) and `scripts/tests/bootstrap-broker-db.test.sh` (19 checks pass).
- CI jobs `integration` and `bootstrap-db` in `.github/workflows/browser-broker-ci.yml`: see the progress file for whether the commit landed.

## Shared Postgres server (say it to the owner)
Model Plane data now lives on Ingestion Plane's Postgres SERVER. The repo rule is no direct database crossing between planes: a separate
database and role keep the data apart and only the broker touches `browser_grants`, but it is a deliberate shared server (backups, the
256 MB container limit, the 100 connection cap, a Postgres restart takes the broker's store down too). A dedicated instance later only
changes the host in `DATABASE_URL`.

## Traps found
- `BROWSER_BROKER_PG_PASSWORD` uses `:?`: every Model Plane compose command fails until the key is in `deploy/.env`.
- Nothing in the repo mints it: the root `scripts/bootstrap_runtime_environment.sh` was not touched (ledger rules).
- The stock postgres `pg_hba` trusts 127.0.0.1, so a password check must use the container's own address, not loopback.
- gitleaks (pre-commit, generic-api-key) flagged `PGPASSWORD=<literal>` in a test: generate throwaway passwords at runtime.
- `core.fileMode=false` here: `git add` of a new script commits 100644; use `git add --chmod=+x` first (done in e7e9b9e1).
- The broker fetches JWKS at boot (`authctx.NewVerifier`), so running its image standalone needs a reachable JWKS with a `kty: "RSA"`,
  `alg: RS256`, `kid` key (the repo's JWKS shape, not the standard `kty`); `postgres:16-alpine` has no httpd, use `python:3.12-alpine`.
- Compose adopts a hand-made network when its two compose labels are set; unlabelled it still works with a warning.
- The docker lock queue was 45 minutes deep behind other agents' cargo jobs; with-lock's wait of 2700 s ran out once.
- Docker Desktop began answering 500 on the engine API around 17:53; see not-done items.

## Not done
See the structured report: readyz/kill-postgres legs of the image proof, the CI integration command re-run, nothing deployed.

## Round 4 addendum
Reviewer findings handled (all real): dead `s+` regex in the L1 ephemeral-flag guard (fixed + self-test + mutation run: 7 of 7 mutations fail the
test, control passes); password on sed's argv in the mint step (fixed: builtins only; offline harness 8/8 incl. a mutation; test now records argv via
PATH shims); isolation overclaim (superuser ingestion_user can read/write browser_grants; wording corrected everywhere except the already-made
commit messages, which cannot be amended: the corrections are in this round's commit messages and the deploy notes); broker role could connect to
Quarry's databases (REVOKE PUBLIC CONNECT added, --keep-public-connect opts out); network membership only partly pinned (parser over base + overlays);
root bootstrap test rejected the new required variable (bootstrap_runtime_environment.sh now mints it; 4 other pre-existing failures remain, not U1's);
CI path filters (added); --mint-if-missing no-overwrite untested (test added).
Trap: the pre-commit hook says "gitleaks found a secret" when docker run exits 1 because the daemon is down; the commit is blocked either way, never bypassed.
Trap: MSYS grep strips CR on Git Bash; count CRs in bash instead.
Real isolation still open: dedicated Postgres instance, or non-superuser Quarry role + pg_hba reject.

## Round 4b status
Review fixes are in the working tree but NOT committed: the pre-commit gitleaks hook runs through Docker and Docker Desktop's engine answers 500 (hook then prints a misleading "found a secret"). Commit commands are in wf-U1-broker-db-progress.md (Round 4b). The extended bootstrap test (33 checks) has never run against a real Postgres.
