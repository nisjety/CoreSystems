# Learning Plane

CoreSystem's product-independent learning domain. First consumer: Teachme, a school pilot for ages 13–18 on Windows. Later consumers: Verevon employee onboarding and Agency learning experiences.

**Status: L01 source implemented; live school operation is not yet enabled.** Learning Core can run in its original closed foundation mode or, when fully configured, connect to Control Auth and Learning-owned PostgreSQL for school sessions, classes, class members, reviewed course revisions, assignments, learner attempts and formative event summaries. Teachers can inspect attempt summaries in the dashboard; completion is explicitly learner-reported and is not verified mastery. This is not a production or school-pilot readiness claim: audit/outbox and notification integration, school provisioning/entitlement policy, student-facing attempt UX, real-session acceptance and negative-isolation checks remain open. No child account or student data has been accessed.

- [Structured implementation plan](docs/IMPLEMENTATION_PLAN.md)
- [Architecture decision](docs/ADR-001-learning-plane.md)
- [Sovereignty and device access](docs/SOVEREIGNTY_AND_DEVICE_ACCESS.md)
- [HTTP contract](contracts/openapi.yaml)
- [Frontend and Windows workspace](../Frontend%20Plane/learn-core/README.md)

## Foundation mode

```powershell
cd 'C:\dev\CoresSystem\apps\learning-plane\learning-core'
go run ./cmd/learning-core
```

The default listener is `127.0.0.1:3196`. `LEARNING_LISTEN_ADDR` may select another loopback IP/port; non-loopback binds are rejected.

- `GET /healthz`: process liveness.
- `GET /readyz`: HTTP 503 while authenticated school/course operations are not configured.
- `GET /api/v1/learning/status`: content-free capability status.
- Other `/api/v1/learning/` routes: closed with HTTP 503.

## Enable local school/course development

Requires a running Control Auth Core on `127.0.0.1:3011`, an existing Control account and active organization membership, Docker, and dedicated local secrets. Do not use real student accounts or data in this development setup.

Generate distinct credentials in PowerShell. Keep them in a secret manager or local ignored environment files so they can be reused in separate terminals. Never commit them. Docker Compose reads its project `.env` file for interpolation; `go run` does not load `.env`, so export the values into the Learning Core process environment in that terminal:

```powershell
$env:LEARNING_DB_OWNER_PASSWORD = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$env:LEARNING_DB_APP_PASSWORD = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$env:LEARNING_CORE_MEMBERSHIP_SERVICE_TOKEN = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
```

Set the same `LEARNING_CORE_MEMBERSHIP_SERVICE_TOKEN` for Control Auth and Learning Core. It must be distinct from every other Control service credential. Auth Core's development trusted-origin list includes the Learn dashboard at `http://127.0.0.1:5201` and `http://localhost:5201`; if `BETTER_AUTH_TRUSTED_ORIGINS` is explicitly set in your local environment, include the one you use. Production uses its own required trusted-origin setting.

Start the isolated Learning database (its host port is bound to loopback):

```powershell
cd 'C:\dev\CoresSystem\apps\learning-plane'
docker compose up -d learning-postgres
```

The compose service creates a separate `learning_app` login with no superuser, database-creation or role-creation authority. Learning Core uses this app login for tenant-scoped queries and the `learning_owner` login only to apply embedded migrations. The database persists in a named volume; changing compose environment passwords does not rotate existing database credentials.

With the three generated secrets available in the shell, set the URLs and start Learning Core:

```powershell
$env:AUTH_SERVICE_URL = 'http://127.0.0.1:3011'
$env:LEARNING_DATABASE_URL = "postgres://learning_app:$env:LEARNING_DB_APP_PASSWORD@127.0.0.1:5546/learning?sslmode=disable"
$env:LEARNING_MIGRATION_DATABASE_URL = "postgres://learning_owner:$env:LEARNING_DB_OWNER_PASSWORD@127.0.0.1:5546/learning?sslmode=disable"
cd 'C:\dev\CoresSystem\apps\learning-plane\learning-core'
go run ./cmd/learning-core
```

All four Learning settings (`AUTH_SERVICE_URL`, `LEARNING_CORE_MEMBERSHIP_SERVICE_TOKEN`, `LEARNING_DATABASE_URL`, `LEARNING_MIGRATION_DATABASE_URL`) are required together. Partial configuration stops startup. Auth Core must be restarted with the dedicated membership token before Learning can authenticate school members. The Vite dashboard proxies Control Auth at `127.0.0.1:3011` and Learning Core at `127.0.0.1:3196`.

The dashboard signs into an existing Control account, creates a class (school owner/admin only), verifies members through the canonical exact-membership authority, authors/publishes immutable course revisions, assigns a published revision and lists class assignments. Teachers can add students; only a school administrator can add a teacher. Students only see published revisions assigned to their class. Teachers can inspect each assignment's active learners' attempts. The attempt API stores minimal ordered events against the exact assigned revision, deduplicates retries, and only accepts learner-reported completion; it does not run Windows guidance or verify a learner's outcome.

School/course readiness does not imply complete product readiness. Whisper, translation/TTS, tutoring, device enrollment, screen capture, remote teacher control, Control audit outbox and Application notifications are still disconnected. Attempt events are currently Learning-owned only and are not yet mirrored to Control Audit. Keep this local workflow synthetic until those gates and school approvals are complete.

## Maintainer checks

```powershell
cd 'C:\dev\CoresSystem\apps\learning-plane\learning-core'
go test ./...
go vet ./...
```

Frontend workspace checks and current acceptance evidence are documented in the [Learn Core README](../Frontend%20Plane/learn-core/README.md) and [implementation status](docs/STATUS.md).
