# anbud-core: requirements

What anbud-core needs to run, and where each value lives.

## 1. Environment variables

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DOFFIN_API_KEY` | for Doffin ingestion | none | Subscription key for the **Public API** (section 2). Absent: the service still boots and reports `doffin: not_configured`. Present but malformed: startup fails, and the value is never echoed. |
| `DOFFIN_API_KEY_SECONDARY` | no | none | The subscription's second key, kept for rotation. Only `cmd/doffin-probe` reads it; the service never loads it, so a stray value cannot leak it or stop the service starting. The probe fails if it is set without the primary. |
| `DOFFIN_BASE_URL` | no | `https://api.doffin.no/public/v2` | Must be `https` (plain `http` only for loopback, used by tests). No credentials, query or fragment. Both keys are sent to this host, so any host other than `api.doffin.no` is refused unless it is named in `DOFFIN_BASE_URL_ALLOW_HOST`. |
| `DOFFIN_BASE_URL_ALLOW_HOST` | no | none | The one non-default host the keys may be sent to. Must equal the host of `DOFFIN_BASE_URL` exactly. |
| `DOFFIN_SEARCH_RPM` | no | `24` | Requests per minute for `search`. Doffin documents 30 per key; larger values are rejected at startup. |
| `DOFFIN_DOWNLOAD_RPM` | no | `8` | Requests per minute for `download`. Doffin documents 10 per key; larger values are rejected at startup. |
| `DOFFIN_HTTP_TIMEOUT` | no | `30s` | Per-request timeout. |
| `TED_API_KEY` | no | none | **Not needed and not read.** TED serves published notices anonymously; a key is only for endpoints that touch unpublished ones. Nothing sends it. |
| `ANBUD_ORG_CORE_URL` | for organisation lookups | none | org-core's origin, such as `http://org-core:8080`. No path, query or credentials. Set together with the token. |
| `ANBUD_ORG_CORE_TOKEN` | for organisation lookups | none | The service token anbud-core presents to org-core as principal `anbud-core`. 32 to 512 characters, not starting with `test`, `placeholder`, `change-me` or `replace-with` (org-core refuses those). One without the other fails startup; neither leaves lookups "not configured". See section 10. |
| `ANBUD_TENDER_DOCS` | no | `false` | `true` turns on the upload of tender documents (section 12): a company's own documents, kept for a time. Off, the routes are not there. `true`, `false`, `1` or `0`; anything else fails startup. |
| `ANBUD_TENDER_DOCS_RETENTION` | no | `2160h` (90 days) | How long an upload is kept before it expires and is purged: a Go duration such as `720h`, at least `1s` (the database keeps the retention as an interval to the microsecond, so less is nothing and every upload would be refused) and at most `17520h` (two years, which the table enforces). There is no `d` unit. Checked whether or not the feature is on. |
| `ANBUD_TENDER_DOCS_MAX_MB` | no | `100` | The largest archive an upload may be, 1 to 512. The server's body limit for the upload route follows it. Memory is the cost: see section 12. |
| `ANBUD_API_KEY` | yes | none | The key callers must present (`X-API-Key` or `X-Internal-Api-Key`). Compose sets it from `AUTH_CORE_INTERNAL_API_KEY`. |
| `ANBUD_API_KEY_HEADER` | no | `X-API-Key` | Header name for the key above. |
| `ANBUD_DSN` | yes | none | Postgres DSN for database `anbud_core`. Compose builds it from `INGESTION_PG_PASSWORD`. `cmd/doffin-probe` also reads it, optionally, to take the budget lock (section 6). |
| `PORT` | no | `3138` | |
| `ENVIRONMENT` | no | `dev` | `dev` and `local` enable debug logging. |
| `SERVICE_NAME` | no | `anbud-core` | |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | none | Recorded only; exporter wiring is not implemented. |

`ANBUD_API_KEY` and `ANBUD_DSN` are required **by the binary**. Under compose you do not set them in `.env`; they are derived from the plane's shared secrets.

## 2. Getting the Doffin key

1. Open the Doffin API portal: <https://dof-notices-prod-api.developer.azure-api.net>.
2. Sign up / sign in, then create (or open) your subscription to the **Public API**.
3. Copy the **Primary** key into `DOFFIN_API_KEY` and, optionally, the **Secondary** key into `DOFFIN_API_KEY_SECONDARY` in `anbud-core/.env`. Both keys work and share one budget. To rotate without downtime, regenerate one key in the portal while the other is in use, then swap the two lines.

Doffin runs two separate APIs. Only the first is for anbud-core:

| API | Purpose | Use here |
|---|---|---|
| **Public API** | Search and download published notices | **Yes** |
| **Notices API** (`eform-api`) | *Publishes* notices to TED and Doffin; submissions are legally binding and cannot be deleted; needs manual activation by DFØ | **Never.** Do not put its key in this service. |

Production and test are separate portals with separate registrations. This service targets production.

## 3. Where values live

- `anbud-core/.env`: owned by this core, gitignored, mode 0600, created from `.env.example`. Holds the Doffin key and any tuning.
- Shared plane secrets (`AUTH_CORE_INTERNAL_API_KEY`, `INGESTION_PG_PASSWORD`) live in the plane's gitignored `.env.generated-secrets`, managed by `scripts/run-ingestion-plane.sh`.
- Compose loads `anbud-core/.env` through `env_file:`. Do **not** re-list `DOFFIN_API_KEY` under `environment:`: an `${DOFFIN_API_KEY:-}` there resolves to empty when compose runs without the runner's `--env-file` arguments, and `environment:` beats `env_file:`, so it would silently blank the key.
- Keep names namespaced (`ANBUD_*`, `DOFFIN_*`, `TED_*`). The runner merges every core's env file for compose interpolation, so a generic name such as `PORT` would collide.

## 4. Bring-up

Run from `apps/Ingestion Plane`, always through the runner: it passes every core's env file and makes sure the `inter-plane-bus` network exists. A plain `docker compose up <service>` can leave a service off the cross-plane bus.

```bash
scripts/run-ingestion-plane.sh up -d --build anbud-api
```

**Database.** `init-databases.sql` creates `anbud_core`, but Postgres runs init scripts only on a *fresh data volume*. On an existing volume create it by hand once:

```bash
docker exec -i ingestion-postgres psql -U ingestion_user -d ingestion_plane_db -c "CREATE DATABASE anbud_core OWNER ingestion_user"
```

**After editing `.env`.** `env_file` is read when the container is created, so recreate it:

```bash
scripts/run-ingestion-plane.sh up -d --force-recreate --no-deps anbud-api
```

**Without Docker.** Set `ANBUD_API_KEY` and `ANBUD_DSN` in your shell, then `go run ./cmd/api`.

## 5. Health checks

```bash
curl -s http://localhost:3138/health
```

returns `{"status":"ok","service":"anbud-core"}`.

```bash
curl -s http://localhost:3138/ready
```

returns the database state and, per source, `configured` or `not_configured`. A source that is not configured does **not** make the service unready; the database being down does (HTTP 503).

`GET /api/v1/sources` (needs the API key) lists each source with its effective request budgets. It never returns a secret.

## 6. Verify the Doffin key

```bash
set -a; source .env; set +a; go run ./cmd/doffin-probe -save-dir testdata/captured
```

It first checks each configured key with one search ("accepted" or "REJECTED"), and carries on with the first that works (it tells you if the primary was rejected but the secondary works). Only a rejection marks a key as bad: a rate limit or an outage stops the probe without blaming the key. The default run then makes about 15 search calls and 1 download. For a corpus sample instead (a few minutes; 12 recent competitions, one notice of every other type and the oldest, with each notice's flavour, root and tender platforms summarised):

```bash
set -a; source .env; set +a; go run ./cmd/doffin-probe -discover=false -samples 12 -save-dir testdata/captured
```

All calls are paced under the per-key budgets. The overall time limit is 5 minutes plus 15 seconds per requested notice unless `-timeout` says otherwise. A sample that is rate limited or runs out of time stops early and still prints its summary, with the failed downloads counted. The probe reports the real response shapes. [SOURCES.md](SOURCES.md) records what they settled and what is still open.

**Budget lock.** The probe spends the same per-key budget as `cmd/doffin-sync`. When `ANBUD_DSN` is set in its environment, it takes the sync's budget lock before its first call. (`source .env` exports it only if `.env` holds it, and section 1 says it is not set there under compose, so export the sync's DSN yourself.) It holds the lock for its whole run and releases it on every exit except a signal kill, where the closing of its database session frees it. It applies no migration and writes nothing to the database. The DSN must pass the sync's TLS rule (section 9.5), or the probe exits 1 before it connects. If another sync holds the lock, the probe makes no Doffin call, says so on stderr and exits 7, the code the sync uses. It has no lock-loss heartbeat, which the sync has, so it does not notice if its session ends mid-run. When `ANBUD_DSN` is unset it takes no lock, and its header says `budget lock: not taken (ANBUD_DSN is not set); do not run this while a sync is running`. In that case do not run it while a sync is active, or it can overspend the key's budget.

Every raw body is saved *before* it is decoded. If Doffin's response differs from the published spec, the probe says so ("the key works, but the response does not match the published spec"), names the field that did not fit, keeps the raw file, and carries on instead of stopping. Saved files are private (mode 0600, directory 0700) and `testdata/captured/` is gitignored. Everything the API sends is stripped of control and invisible formatting characters before it is printed.

## 7. Tests

```bash
go vet ./... && go test -race -cover ./...
```

`TestApplyMigrationsAgainstARealDatabase` runs only when `ANBUD_TEST_DSN` points at a throwaway database. The sync's database tests, and the make targets that run them, are in [section 9.6](#96-tests).

## 8. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `fatal: config: ANBUD_API_KEY is required; ANBUD_DSN is required` | Binary started outside compose with no env | Use the runner, or export both variables |
| `fatal: doffin source: ... API key must be 16-256 printable ASCII characters without spaces` | Key pasted with a space, newline or quotes | Re-paste it as a single token |
| `/ready` returns 503, `db: down` | Database `anbud_core` does not exist | Create it (section 4) |
| `/ready` shows `doffin: not_configured` | Key empty, or the container predates your edit | Fill `.env`, then recreate (section 4) |
| Probe: HTTP 401 | Wrong key, key from the Notices API, or a test-portal key against production | Use the Public API subscription's key from the **production** portal |
| Probe or client: 429 / `rate limited` | The per-key budget is shared by everyone using that key | Wait for `Retry-After`; do not run two probes at once, or a probe during a sync. With `ANBUD_DSN` set, the probe refuses to start while another process holds the budget lock (section 6) |
| Probe says the response "does not match the published spec" | Expected: the spec contradicts itself in places | Nothing is broken. The raw body is saved; check it holds nothing personal, then share it so the types can be corrected |
| `fatal: config: DOFFIN_SEARCH_RPM must be a whole number from 1 to 30` | The value is not a number, or is above Doffin's documented limit | Fix the variable. The message never repeats the value you typed |

## 9. Raw-store sync

`cmd/doffin-sync` copies Doffin search responses and notice bodies into the database named by `ANBUD_DSN`, one closed UTC day at a time. Design and status: [ARCHITECTURE.md, Raw store and day-window sync](ARCHITECTURE.md#raw-store-and-day-window-sync). It runs on the host. The design gives it no compose service, and the Dockerfile builds `cmd/api` only.

**Status: built. The database tests passed against PostgreSQL 16.14 on 2026-10-10, and the live smoke test (V1 to V4) ran against Doffin the same day with the owner's go-ahead.** The census of the full history (V5) has not run. Before a live run, read [Verification status](ARCHITECTURE.md#verification-status).

### 9.1 Usage

```
GOTOOLCHAIN=local go run ./cmd/doffin-sync -from YYYY-MM-DD [-to YYYY-MM-DD] [-recheck N] [-max-downloads N] [-page-size N] [-census] [-source doffin|ted] [-countries SWE,DNK,...]
```

`-source ted` (2026-10-28) syncs TED instead of Doffin, with the same window, flags, exit codes and budget lock. TED is read anonymously, so `DOFFIN_API_KEY` is not required for it. `-countries` (TED only) names the buyer countries, ISO 3166-1 alpha-3, default `NOR,SWE,DNK,FIN,ISL` (a Norwegian notice is on Doffin too, and the readers show it once). A day that is done is not searched again for a new country: add `-recheck N` once over the held days. Both calls of TED are paced at 20 a minute. `TED_SEARCH_URL` and `TED_NOTICE_URL` point a test at a loopback TED.

| Flag | Default | Meaning |
|---|---|---|
| `-from` | required | First UTC day. Not before 2017-01-02. |
| `-to` | UTC yesterday | Last UTC day. Must be a closed day: not later than UTC yesterday. |
| `-recheck` | `0` | Also search again the last N days of the window, even when they are done. Only ids not yet held are downloaded. |
| `-max-downloads` | `0` (unlimited) | Stop after N downloads. The day that hits the limit ends `partial` with `download_limit`. |
| `-page-size` | `0` (the default of 100) | Search hits per page, 1 to 100. A smaller page makes a short day span several pages, which is how the multi-page path is tried live on a day of fewer than 100 hits (D-B10). It costs more searches. The size is part of the stored page's query key. |
| `-census` | off | Search and count only: download nothing. Done days are skipped. |

A bad flag, a bad date or a window outside these rules exits 1. The message names the rule and does not echo the value.

### 9.2 Exit codes

| Code | Meaning |
|---|---|
| 0 | Every day in the window is done, including a day whose refused notices are all terminal. A census exits 0 when every day is searched or capped. |
| 1 | Usage or configuration error. Nothing was written and nothing was called. Also a start refused after the migrations because the database role of `ANBUD_DSN` is too privileged for a non-loopback host (section 9.5): nothing was called, but that start's migrations have been applied. |
| 2 | Gaps remain: a day is partial, or `-max-downloads` was reached. A census exits 2 when a day is partial or failed. |
| 3 | Doffin answered 429, and the retry instant is stored. Also a start refused because the stored instant is in the future: no call is made, and the run id is 0. |
| 4 | Doffin answered 401 or 403. The key was rejected. |
| 5 | Cancelled by SIGINT or SIGTERM, or the budget lock was lost. A second signal exits at once with 5. |
| 6 | A search, store or ledger failure, five counted errors in a row, the database could not be opened or migrated, or the role of `ANBUD_DSN` could not be read (the start fails closed). |
| 7 | Another sync holds the budget lock. Nothing was called. |

### 9.3 Output

- stdout: one JSON object per touched day (`day`, `state`, `error_code`, `total`, `accessible`, `held`, `stored`, `not_found`, `rejected`, `unresolved`, `searches`), then one summary object (`run_id`, `reason`, `counters`, and `retry_after_until` when a 429 set it). Counts only. `rejected` counts the day's notices that were refused for their content three times: terminal, never downloaded again, and counted toward a done day. It is a per-day count; the summary's `counters` have no such field. `searches` is the number of search calls the day took, so a census shows how many sweeps a busy day needed (searches divided by pages); like `rejected`, it is reported on the line and not stored in the ledger.
- stderr: one line for a fatal error. It names a variable or a rule, never a value. Database errors say only "check ANBUD_DSN".
- Neither stream carries a notice id, a body, a key or the DSN.

### 9.4 Environment

| Variable | Required | Default | Notes |
|---|---|---|---|
| `ANBUD_DSN` | yes | none | Postgres DSN. The CLI applies the migrations to whichever database the DSN names, and it checks no database name. The name guard exists only in the test helper. |
| `ANBUD_MIGRATE_DSN` | no | none | The schema owner's DSN (role `anbud_migrator`, [ROLES.md](ROLES.md)). When set, the migrations run through it and `ANBUD_DSN` is the application role's (`anbud_app`). When unset, one DSN does both, as before. It follows the same TLS rule as `ANBUD_DSN` (section 9.5), and no message ever echoes it. |
| `DOFFIN_API_KEY` | yes | none | The primary key only. The sync never reads the secondary key. |
| `DOFFIN_SEARCH_RPM` | no | `24` | Search requests per minute, from 1 to 30. |
| `DOFFIN_DOWNLOAD_RPM` | no | `8` | Download requests per minute, from 1 to 10. |
| `DOFFIN_HTTP_TIMEOUT` | no | `30s` | Per request. A positive duration. |
| `DOFFIN_BASE_URL` | no | `https://api.doffin.no/public/v2` | The same rules as section 1. Any host other than `api.doffin.no` needs `DOFFIN_BASE_URL_ALLOW_HOST`. |
| `DOFFIN_BASE_URL_ALLOW_HOST` | no | none | The one non-default host the key may be sent to. Must match the host of `DOFFIN_BASE_URL`. |
| `SERVICE_NAME` | no | `doffin-sync` | Loaded, but the CLI does not use it. |

The sync does not read `ANBUD_API_KEY`, `ANBUD_API_KEY_HEADER`, `PORT`, `ENVIRONMENT`, `OTEL_EXPORTER_OTLP_ENDPOINT` or `DOFFIN_API_KEY_SECONDARY`.

### 9.5 Database connection rules

- A loopback host (`localhost`, `127.0.0.1`, `::1`) or a Unix socket directory may use any `sslmode`.
- Any other host must verify the server certificate, and the DSN must name exactly one host, with no fallback. The check reads what pgx verifies, not the literal `sslmode` string: `sslmode=verify-ca` or `verify-full` pass; `sslmode=require` passes only with an `sslrootcert` CA file (pgx verifies the chain as verify-ca); `sslrootcert=system` passes as verify-full. Plain `require`, `prefer`, `allow` and `disable` are refused. A DSN that names several hosts is refused if any of them is not local. Otherwise the CLI exits 1 and names the rule. It never echoes the DSN.
- `require` encrypts the connection but does not check the server certificate, unless `sslrootcert` names a CA file; then it behaves as `verify-ca`. `sslrootcert=system` makes pgx use `verify-full`. `verify-full` also checks the host name. Use `verify-full` for a remote database. (Checked against the pgx v5.9.2 source.)
- The rule lives in `internal/db` (`db.CheckTLS`; `db.CheckTLSFor` names the variable, so `ANBUD_MIGRATE_DSN` gets the same rule and its own message). `cmd/doffin-probe` applies the same rule when `ANBUD_DSN` is set (section 6).
- **Database role.** After the migrations the CLI reads the role of `ANBUD_DSN` (`db.CheckRuntimeRole`) and judges it by what it can use or become: a superuser, a role with CREATEROLE, CREATEDB or BYPASSRLS (itself or through a role it is a member of), a member of a predefined role that reaches files, programs or other sessions or of the owner or DBA roles of the role model, or an owner (directly or through a role it belongs to) of a relation, a function, a schema on its search path or the database, can drop or disable the append-only triggers or erase a body. On a non-loopback host such a role ends the start with exit 1; on a loopback host the CLI prints one warning line to stderr and goes on, so a development database with one superuser DSN still works. A role that cannot be read ends the start with exit 6. The role of `ANBUD_MIGRATE_DSN` is checked first, before it migrates anything, on the attributes and predefined roles only (it owns everything by design): refused with exit 1 off loopback, one warning on loopback, exit 6 if unreadable. The probe applies the runtime check where it opens a pool. The role model that satisfies it is in [ROLES.md](ROLES.md).

### 9.6 Tests

| Command | What it does |
|---|---|
| `make test` | Pure tests for the whole module, with `ANBUD_TEST_DSN`, `DOFFIN_API_KEY` and `ANBUD_REQUIRE_DB_TESTS` unset, so the database tests skip. Runs with `-race` and `-skip '^TestLive'`: the pattern is anchored, because a plain `Live` also skipped every test whose name merely contains the word (three did, until 2026-10-10). |
| `make test-db` | Starts a throwaway `postgres:16.14-alpine3.24` container on 127.0.0.1 at a random port, with a random password. Runs `scripts/roles.sql` twice under the container's own `psql`, then `./internal/db/ ./internal/rawstore/ ./internal/docstore/ ./cmd/doffin-sync/ ./cmd/doffin-probe/ ./cmd/api/` one package at a time with `ANBUD_REQUIRE_DB_TESTS=1`, and removes the container on exit. Override the packages with `make test-db PKGS="./internal/db/"`. Needs Docker. |

- `ANBUD_TEST_DSN` is a throwaway server, given as a `postgres://` URL. Each database test creates its own `anbud_test_<random>` database, migrates it, and drops it at the end. The guard refuses any database that is not `postgres` and does not start with `anbud_test`, so `anbud_core` is refused.
- `ANBUD_REQUIRE_DB_TESTS=1` turns a missing `ANBUD_TEST_DSN` into a failure instead of a skip. Only the value `1` does this.
- Database tests never delete rows, because the evidence triggers refuse it. Cleanup is `DROP DATABASE`.
- Do not run `go test ./...` with `ANBUD_TEST_DSN` set to a real database. Use `make test-db`, which lists its packages.
- The database suite passed on 2026-10-10 through `make test-db` against PostgreSQL 16.14, most recently with the sweep rule and the role model. No other server version has been exercised. See [Verification status](ARCHITECTURE.md#verification-status).

### 9.7 What the sync does not do

- No scheduler of its own and no HTTP endpoint. The design has no compose service for it, and the Dockerfile builds `cmd/api` only. On this Mac a launchd job runs it daily (section 9.8).
- No parsing, normalisation, validation or year-CSV backfill.

### 9.8 Daily run (2026-10-28)

`scripts/install-daily-sync.sh` installs a launchd job that runs `scripts/daily-sync.sh` at 06:15 and at login: the last 14 closed UTC days with `-recheck 4`, Doffin and then TED, into the local Studio stack's `anbud_core` on loopback (127.0.0.1:9540). launchd may not read the external volume the repository is on, so the installer builds `doffin-sync` and writes `sync.env` (mode 600: `DOFFIN_API_KEY` and the two DSNs) into `~/Library/Application Support/no.triodelab.anbud-daily-sync`; reinstall after changing the sync or rotating a credential. Log: `~/Library/Logs/anbud-daily-sync.log`. `scripts/install-daily-sync.sh uninstall` removes the job, its binary and its credentials. Run from the repository, `scripts/daily-sync.sh` builds and reads the credentials itself.
- No change detection for an id it already holds. A changed body under a known id is not seen, because seeing it would need a download.
- No retention expiry (open owner decision O1). Erasure of a notice body is a recorded DBA procedure, not a feature of this CLI: `anbud_redact_raw_document()` and its runbook ([PERSONAL-DATA.md](PERSONAL-DATA.md)), built on 2026-10-10 as the mechanism only. It does not reach search pages, backups or logs, and the legal decision behind it is not made. No tenant column, by design (P2).
- No further attempt at a notice that was refused for its content three times. It is terminal, and undoing that takes a DBA procedure or a new migration ([ARCHITECTURE.md, Known gaps](ARCHITECTURE.md#known-gaps)).
- No code reads a body back. The store reads only ids and attempt outcomes, to skip work.
- No completion of a saturated day. It is flagged `capped`.
- No check against a probe that runs without `ANBUD_DSN`: it takes no budget lock, so nothing stops it from spending the key's budget during a sync (section 6).
- No telemetry: the CLI does not start `telemetry.Init`, and writes its own JSON to stdout.

### 9.9 Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `doffin-sync: config: ANBUD_DSN is required` (exit 1) | The variable is not set | Export it, as in section 9.4 |
| `ANBUD_DSN names a host that is not loopback, so it must verify the server certificate: sslmode=verify-ca or verify-full, with exactly one host` (exit 1) | A remote host whose server certificate is not verified, or a remote DSN with several hosts | Use `sslmode=verify-full` with one host (section 9.5) |
| `ANBUD_DSN names a host that is not loopback and a database role that is too privileged: ...` (exit 1) | The role is a superuser, holds CREATEROLE, CREATEDB or BYPASSRLS (itself or through a role it belongs to), belongs to a predefined role that reaches files, programs or other sessions or to the owner or DBA roles, or owns relations, functions, the schema or the database, and the host is not loopback. The migrations of that start have run. | Run `scripts/roles.sql` and give the sync the `anbud_app` DSN, with `ANBUD_MIGRATE_DSN` as `anbud_migrator` ([ROLES.md](ROLES.md)) |
| `ANBUD_MIGRATE_DSN names a host that is not loopback and a database role that is too privileged: ...` (exit 1) | The owner role is a superuser, holds CREATEROLE, CREATEDB or BYPASSRLS, or belongs to a predefined role that reaches files, programs or other sessions, and the host is not loopback. Nothing was migrated. | Use `anbud_migrator` as `roles.sql` makes it: it needs to own the schema and nothing more |
| `doffin-sync: warning: the database role in ANBUD_MIGRATE_DSN is more privileged than the migrations need ...` (a warning, the run goes on) | The same owner role on a loopback host | Accepted for development |
| `could not check the privileges of the database role in ANBUD_MIGRATE_DSN` (exit 6) | The owner role's facts could not be read | Check the connection and that the role can read the catalogs |
| `doffin-sync: warning: the database role in ANBUD_DSN is a superuser ...` (a warning, the run goes on) | The same role on a loopback host | Accepted for development. Use the role model before any shared database. |
| `could not check the privileges of the database role in ANBUD_DSN` (exit 6) | The role facts could not be read | Check the connection and that the role can read `pg_roles` and `pg_tables` |
| Exit 7, "another sync holds the budget lock" (the sync), or "another sync holds the Doffin budget lock; the probe made no Doffin calls" (the probe) | A live sync session holds the lock. A dead one releases it when its session closes. | Wait for the other run to finish, then rerun |
| Exit 3 with `retry_after_until` | A 429 closed the gate | Wait until the instant passes. The start is refused before then, with no call. |
| Exit 4 | The key was rejected | Use the Public API key from the production portal (section 2) |
| Exit 2 with `download_limit` | `-max-downloads` was reached | Run again to continue. Held ids are skipped. |
| Exit 2 with days in `item_rejected` | A notice of the day is unresolved. Usually its download was refused for its content (too large, or an empty body) fewer than three times, so it cools for 24 hours after each refusal. Otherwise a download failed with an HTTP or transport error, or an id was malformed. | Expected for a refused notice. Rerun 24 hours after its latest refusal: it is downloaded again, and the third refusal is terminal, so the day becomes done with the notice counted in `rejected`. An HTTP or transport error is tried again on every run. |
| A done day with `rejected` above 0 in its line | Those notices were refused for their content three times. They are terminal: never downloaded again, and their bodies are not held. | Expected. Undoing it takes a DBA procedure (ARCHITECTURE, Known gaps). |
| Exit 6, "database or ledger error" | A database or ledger write failed | Check the failed run in `sync_runs`. The next start recovers rows left running. |
| Exit 5 after "the budget lock was lost" | The lock's session ended during the run | Rerun the sync |

## 10. Organisation lookups (org-core)

anbud-core looks organisations up through the Control Plane's org-core, never by calling Brønnøysund itself (`internal/orgcore`). It calls `GET /api/v1/brreg/{orgnr}` with `X-Service-Id: anbud-core` and `X-Service-Token`, and the route needs the scope `org:brreg:read`. That scope is not `:self`, so no signed user delegation is sent, and no user identity is either.

What the operator does, once (Control Plane; not done by this core):

1. Generate a token of 32 or more random characters.
2. Add a credential to org-core's `ORG_CORE_SERVICE_CREDENTIALS`: `{"principal":"anbud-core","audience":"org-core","token":"<token>","scopes":["org:brreg:read"]}`. org-core reads the registry at start-up, so restart it.
3. Put the same token in `ANBUD_ORG_CORE_TOKEN`, and org-core's origin in `ANBUD_ORG_CORE_URL`. The compose service `anbud-api` must pass both; it does not yet.

Responses, as the client maps them: 404 not in Enhetsregisteret; 401 credential rejected; 403 scope `org:brreg:read` not granted; 502 Enhetsregisteret unreachable behind org-core; 503 org-core's service authentication is not set up. The client never follows a redirect (the token would go with it), never echoes a response body into an error, refuses a record for another number than it asked for, and checks the number's modulus-11 check digit before any call.

Only what a profile needs is kept: number, name, organisation form, first NACE code, municipality of the business address, employee count and the bankruptcy and winding-up flags. E-mail, telephone and street addresses are not decoded: for a sole trader they are a person's.

Not verified against a running org-core: the client is tested against a local test server only, and the stack is not running on this machine.

## 11. Demo API

Four routes show the pipeline end to end: a company's profile matched against the stored tenders, the audit sheet of one tender (with its buyer's past awards), a register lookup to pre-fill a profile, and the re-tender radar. They need a database that `cmd/doffin-sync` has filled, and the plane's API key (`X-API-Key`, the value of `ANBUD_API_KEY`: set it in your shell, never paste it into a file or a chat). The design, the response fields and the known gaps are in [ARCHITECTURE.md, Demo API](ARCHITECTURE.md#demo-api).

```bash
# Match a profile. Everything is in the body: nothing about a company belongs in a URL.
curl -s -X POST "http://localhost:3138/api/v1/demo/matches" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "Content-Type: application/json" \
  -d '{"profile":{"cpv":["72000000"],"nuts":["NO08"],"keywords":["fagsystem"],"value_range":{"currency":"NOK","min":200000,"max":5000000}},"strictness":"balanced"}'

# The audit sheet of one tender, for the same profile.
curl -s -X POST "http://localhost:3138/api/v1/demo/matches/2026-104837" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "Content-Type: application/json" \
  -d '{"profile":{"cpv":["72000000"]}}'

# What the register holds about an organisation (needs ANBUD_ORG_CORE_*, section 10).
curl -s -X POST "http://localhost:3138/api/v1/demo/company" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "Content-Type: application/json" \
  -d '{"org_number":"974760673"}'

# Contracts likely to be tendered again within the next 12 months, for a profile's codes.
curl -s -X POST "http://localhost:3138/api/v1/demo/retenders" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "Content-Type: application/json" \
  -d '{"profile":{"cpv":["72000000"],"nuts":["NO08"]},"horizon_months":12,"strictness":"balanced"}'
```

The audit sheet's `incumbency` and the radar name a winner only when the register confirms it is an organisation that is not a sole trader, so without `ANBUD_ORG_CORE_*` (section 10) every winner shows as `unverified`, with no name. Contract ends that are not stated are estimates: `end_basis` says from what. The radar's `strictness` is `strict`, `balanced` (the default) or `wide`, as for the matches, and its `widening` says how many more contracts each wider step would match, before the cap of 50 rows.

| Status | Code | Meaning |
|---|---|---|
| 400 | `validation_failed` | The body is not valid. `error.details` lists `{ field, reason }`; a rejected value is never echoed. |
| 401 | `unauthorized` | No key, or the wrong one. |
| 404 | `not_found` | The notice is not held (or every body of it has been erased), or the organisation is not in the register. |
| 413 | `payload_too_large` | The body is over 64 KiB. |
| 422 | `not_an_opportunity`, `unreadable_notice`, `unsupported_notice` | The stored notice is a result notice, is not a readable notice, or has a shape no field map covers (`details` names the shape). |
| 429 | `rate_limited` | Two scans are already running. Not queued; see `Retry-After`. |
| 502 / 503 | `upstream_unavailable` / `not_configured` | The organisation lookup failed, or is not configured. In `/matches` this never fails the request: the answer says the register was not checked. |
| 504 | `timeout` | The scan took over 20 seconds. Lower `scan`. |

Nothing in a request is stored or logged: the access log records method, path, status and duration only. With an empty database the funnel says `scanned: 0`. Not verified against a running stack: the tests use a throwaway Postgres and a local stand-in for org-core.

## 12. Tender documents (uploads)

A company uploads the tender documents it already holds for a notice (the specification, the contract, the annexes) as a ZIP archive, and the gate reads them as far as keyword rules go. Opt-in: set `ANBUD_TENDER_DOCS=true`. The design, what is kept, how the archive is read and the known gaps are in [ARCHITECTURE.md, Tender documents](ARCHITECTURE.md#tender-documents); what it means for personal data is in [PERSONAL-DATA.md section 9](PERSONAL-DATA.md#9-uploaded-tender-documents).

The routes are tenant routes: every request needs the plane's key and `X-Org-ID`. The notice must be one the service holds. The body of an upload is the ZIP itself, not a form.

```bash
# Upload. The archive is the request body. X-Archive-Name is optional and percent-encoded.
curl -s -X POST "http://localhost:3138/api/v1/notices/2026-104837/documents" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "X-Org-ID: $ORG_ID" -H "Content-Type: application/zip" \
  -H "X-Archive-Name: Konkurransegrunnlag.zip" \
  --data-binary @Konkurransegrunnlag.zip

# What the organisation has uploaded for the notice: the manifest and the findings of each upload.
curl -s "http://localhost:3138/api/v1/notices/2026-104837/documents" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "X-Org-ID: $ORG_ID"

# Delete one upload.
curl -s -X DELETE "http://localhost:3138/api/v1/notices/2026-104837/documents/7" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "X-Org-ID: $ORG_ID"

# The matching is judged by the organisation's documents when the request names the organisation.
curl -s -X POST "http://localhost:3138/api/v1/demo/matches" \
  -H "X-API-Key: $ANBUD_API_KEY" -H "X-Org-ID: $ORG_ID" -H "Content-Type: application/json" \
  -d '{"profile":{"cpv":["72000000"]}}'
```

An upload answers 201 with `data`: `id`, `archive_name`, `archive_sha256`, `archive_bytes`, `created_at`, `expires_at`, `read` and `unread` (files), `documents` (one entry per file: `path`, `kind`, `size`, `sha256`, `status`, `reason`, `pages`, `chars`) and `findings` (`document`, `capability`, `class`, `page` or `paragraph`, `snippet`), and `findings_dropped`. The same archive again is 200 with the same upload. A file's `status` is `read`, `empty` (no text: a scan), `unsupported` (a kind that is not read), `failed`, `encrypted`, `too_large`, `skipped` (with a `reason`) or `duplicate`.

| Status | Code | Meaning |
|---|---|---|
| 400 | `organization_required`, `invalid_organization`, `bad_request`, `validation_failed` | No `X-Org-ID`, or one sent more than once or in a trailer (the tenant is named once, in the head of the request); an empty body or a notice id that is not one; a request the HTTP parser refuses (the answer says only that it could not be read, and never quotes it: the quote would hold the gateway's key); an `X-Archive-Name` that is not percent-encoded UTF-8 text. |
| 401 | `unauthorized` | No key, or the wrong one. |
| 404 | `not_found` | The notice is not held, or the upload is not there (for this organisation and notice). |
| 409 | `zdr_persistence_forbidden`, `limit_reached` | The request carries `X-ZDR: true` (`true` or `1`): nothing is stored for an organisation in zero-data-retention mode. Or the organisation already holds 10 uploads for the notice or 200 in all (`details.scope`): counted when the upload is stored, under a lock of the organisation, so uploads that arrive at once cannot pass the limit, and an upload that loses the race for the last place is read and then refused with the same 409. |
| 413 | `payload_too_large` | The archive, or what it expands to, is over a limit. |
| 415 | `unsupported_media_type` | The body is not sent as `application/zip` (or `application/octet-stream`), or it is compressed (any route). |
| 422 | `invalid_archive` | `details.reason`: `not_a_zip`, `too_many_entries` (over 2000 in one directory, or over 8000 across the directories of the archive and the archives inside it), `too_many_files` (over 500 files in the manifest, skipped ones included) or `expands_too_far`. |
| 429 | `rate_limited` | Two archives are already being read. Not queued; see `Retry-After`. |
| 504 | `timeout` | Reading took over three minutes. Nothing was stored. Upload fewer documents at a time. |

**Memory.** Reading an upload holds the archive, one expanded file, the extracted text (at most 250 000 units and 3 million characters) and the working copies of the one paragraph being cleaned, for each of two concurrent uploads; on the worst shapes tried (a 20 MB run in a DOCX of characters that double in size when normalised is the worst), a reader needed up to about 200 MiB beyond the file itself, counting the garbage not yet collected, which is what a container limit sees. Each PDF is read by a child process with a soft limit of 768 MiB, which ends itself at 1 GiB. Receiving is the larger cost: the server buffers a whole request body before a handler runs, and allocates it from the declared `Content-Length` (rounded up to a power of two) when the headers are accepted, so the peak is the archive limit for **every upload request in flight**, not for two. Nothing in this service bounds how many arrive at once; limit that at the gateway, and size the container for the archive limit you set (the default is 100 MiB). The demo is a third cost, and is bounded since 2026-10-13. A scan reads the live uploads of the organisation for the notices it looks at (`docstore.EvidenceFor`), at most 200 uploads, each with up to 1 MB of findings and a manifest of as much, and reads them one at a time: a file and a finding are counted by a hash of each and not held, so what is said of the documents is whole and the number of findings is exact. Of the findings only the best are held: cut the way an upload is (at most 50 of a capability and 400 for a notice, requirements first and of equal weight the earliest), and then to what is left of a budget of 8 MiB for the call (`docstore.MaxEvidenceBytes`), in the order the notices come, with the first requirement of every capability of a notice always held, so that the gate can always tell that a requirement is there and where. What is left out is counted (`Evidence.FindingsDropped`): `documents.findings` of a result and `findings_total` of the audit are all of the findings, the audit lists at most 100 of those held, and the gate says "at least" of the passages and documents it counts when something was left out. A scan reads at most 1000 notices, which is two calls to the store, each with a budget of its own. Measured on the part that holds them: 40 000 findings in letters of four bytes, which are 69 MiB when they are held whole, are held in 9 MiB. The rows themselves are still read and decoded, up to a megabyte of findings and as much of manifest each, one at a time, so the time is what it was and the memory is not: on a database of 200 uploads built to be as large as the store takes (71 MiB of manifests and 133 MiB of findings, with 350 findings of 2.7 KB in each) a scan peaked at 34 MiB of heap above what the process held before, against 255 MiB before the change, and took about nine seconds either way under the race detector, which is near the demo's ten seconds for the documents (the demo then reports them unavailable).

**Not verified.** Against a running stack, a real tender platform package, or any archive near the limits. The tests use a throwaway Postgres, synthetic archives and one Chrome-made PDF with invented text.
