# imports-core Research Dive

Generated: 2026-07-11
Supersedes: 2026-06-07 pass (stale)
Scope: `apps/Ingestion Plane/imports-core` (Python / FastAPI, container `imports-api`, host port 3025)

> **2026-07-12 source update:** signed ingestion JWT identity now protects create/read/detail/SSE; quota fails closed; CMS/Odoo targets are public-network checked with redirects/size bounded; Postgres payloads and leases recover queued work after restart; M365 no longer writes an orphan empty-org job; Control subjects match `aqencia.*`; and Data Plane writes use the real `/v1/documents/` contract with scoped `documents:write` tokens and idempotency. The gateway now proxies import SSE as a stream. 46 tests pass; measured whole-app coverage is 48%, below the program target. Deployment/live DB verification remains blocked.

## Evidence grade key

Every finding below is tagged with how it was verified:

- `[live-curl]` — observed by host-side `curl` against `http://localhost:3025` right now (2026-07-11/12).
- `[source-only]` — read from the committed source on disk; NOT confirmed against the running image.
- `[logs]` — **UNAVAILABLE this pass.** `docker logs imports-api` fails with `input/output error` and `docker exec` fails with `openat etc/passwd: input/output error` because the host's containerd content store is corrupted. No container-internal evidence could be gathered. Image rebuild/redeploy is likewise blocked, so nothing below could be re-deployed to re-test.

## Snapshot

`imports-core` is the file- and source-import intake API for the Ingestion Plane. It accepts file-upload and external-source import jobs, parses/normalizes content, tracks job/item state in its own Postgres, streams progress over SSE, publishes cross-plane NATS events, and (by contract) forwards parsed documents to Data Plane v2 for durable persistence.

The **committed source is real and mostly well-built** — no production stubs in the core parse/connector/service code, real SDK-backed connectors, a genuine GitHub/Slack knowledge-sync worker, and real Notion body extraction. **But the running deployment is broken in two independent, high-severity ways** and carries two incomplete features:

1. **Live outage: every DB-touching endpoint returns HTTP 500** right now (upload, source, GET job all 500), while non-DB endpoints (`/health`, `/`, `/openapi.json`) return 200. The job API is effectively down. `[live-curl]`
2. **Document persistence is wired to a mock that does not exist.** `.env` sets `DOCUMENT_SERVICE_URL=http://mock-document-service:3030`; no such service/container exists anywhere in the stack, so imports never reach Data Plane v2 (`dpv2-documents-api:8010`). `[live-curl]`/`[source-only]`
3. **Deploy drift:** the committed knowledge-sync route (`051ae380`, `eb0ea7d9`, 2026-07-07) is NOT present in the running image. `[live-curl]`
4. **Two incomplete features:** Temporal orchestration is a no-op; the M365 provider-linked handler is a dead-end stub. `[source-only]`

Non-generated source file count: 20 Python files under `app/` + 3 test files under `tests/`.

## Runtime shape (committed source)

Entrypoint `app/main.py` (FastAPI + lifespan). Lifespan: `SELECT 1` DB probe → `run_sql_migrations()` → shared httpx client → local NATS `event_publisher` → shared cross-plane `SharedNatsPublisher` (verevon-nats) → `ControlPlaneSubscriber` (verevon-nats) wired to the M365 handler.

Module map:
- `app/main.py` — routes, auth wiring.
- `app/auth_middleware.py` — `require_internal_auth` (X-Internal-Api-Key + X-Org-Id).
- `app/service.py` — `ImportService`: quota check (+60s cache), job/item CRUD, bounded-parallel `run_job`, `_store_document` (the Data Plane hop).
- `app/parsers.py` — PDF/DOCX/CSV/JSON/HTML/TXT/MD, optional Apache Tika pre-pass.
- `app/connectors.py` + `app/notion_import.py` — external source connectors.
- `app/knowledge_sync.py` + `app/actions_gateway.py` — Phase 4 GitHub/Slack content-sync via integration-corev2 actions surface.
- `app/control_plane_subscriber.py` + `app/m365_provider_handler.py` — Control Plane event subscriber + M365 setup handler.
- `app/orchestration.py` — Temporal/async dispatch.
- `app/db.py`, `app/models.py`, `app/schemas.py`, `app/events.py`, `app/shared_nats.py`, `app/progress.py`.

## Live verification results `[live-curl]`

| Probe | Result | Reading |
|---|---|---|
| `GET /health` | 200 `{"status":"ok","service":"import-service"}` | app process alive |
| `GET /` | 200 (lists 4 endpoints) | no DB touch |
| `GET /openapi.json` | 200 — **6 paths only** | see deploy drift |
| `GET /api/v1/import/jobs/{uuid}` (no auth) | **500** | reaches handler, no auth gate, DB errors |
| `GET /api/v1/import/jobs/not-a-uuid` | 422 | FastAPI path validation |
| `POST /upload` (no key) | 401 `Missing X-Internal-Api-Key header` | write boundary enforced |
| `POST /source` (no key) | 401 | write boundary enforced |
| `POST /upload` (**valid** key + org, tiny .txt) | **500** | DB write path also broken |
| `POST /jobs/knowledge-sync` | 405 | route absent in running image → matches `{job_id}` GET only |

Live OpenAPI paths in the running image: `/`, `/health`, `/api/v1/import/jobs/upload`, `/api/v1/import/jobs/source`, `/api/v1/import/jobs/{job_id}`, `/api/v1/import/jobs/{job_id}/events`.

## Finding 1 — LIVE OUTAGE: every DB-touching endpoint 500s `[live-curl]`

Both the unauthenticated read (`GET /api/v1/import/jobs/{uuid}`) and the authenticated write (`POST /upload` with a valid internal key + org) return `500 Internal Server Error`. Meanwhile `/health`, `/`, and `/openapi.json` (none of which touch Postgres) all return 200. The fault is isolated to the database layer at runtime.

Per the committed source, a missing job should return **404** (`get_job_with_items` returns `(None, [])` → `main.py:279` raises 404). The 2026-07-10 baseline audit indeed observed 404 here. Getting 500 now is a **live regression**.

Leading hypothesis (timing-based, `[source-only]` timestamps + `[live-curl]` behavior; not confirmable without logs):
- `imports-api` container **started 2026-07-09 21:03** (`docker inspect`).
- `ingestion-postgres` shows **"Up 2 days"** (restarted ~2026-07-10) — i.e. Postgres restarted AFTER imports-api came up.
- imports-api's SQLAlchemy async engine (`pool_pre_ping=True`, asyncpg) appears to be holding connections to the since-restarted Postgres and is not recovering them; every runtime query errors. Startup migrations succeeded 2 days ago (the app is serving), so the schema exists — this is a connection-liveness failure, not a schema-absence failure.

Recommended fix: **restart the `imports-api` container** (does not require an image rebuild, so it is not blocked by the containerd corruption). If 500s persist after restart, the alternate cause is a schema/ORM drift and logs will be needed. Confirmation of the exact cause is blocked by `[logs]` being unavailable.

## Finding 2 — HEADLINE: document persistence wired to a non-existent mock `[live-curl]`/`[source-only]`

`imports-core/.env` line 7: `DOCUMENT_SERVICE_URL=http://mock-document-service:3030` with `DOCUMENT_SERVICE_IMPORT_PATH=/api/v1/documents/import`.

- `mock-document-service` is referenced **only** in that one `.env` line. It is defined in **no** compose file (`docker-compose.yml`, `.production.yml`, `.ui.yml`) and there is **no such running container**. `docker-compose.yml` does not override `DOCUMENT_SERVICE_URL` for `imports-api`, so the `.env` value stands.
- The real Data Plane v2 documents API is `dpv2-documents-api:8010` (running as `data-plane-v2-documents-api-1`, referenced 4× in `docker-compose.yml`; quarry-edge correctly ingests via `DATA_PLANE_INGEST_URL=http://dpv2-documents-api:8010`).

Consequence: `ImportService._store_document` (`service.py:326`) POSTs every parsed document to `http://mock-document-service:3030/api/v1/documents/import` → DNS resolution failure → each item fails → jobs land in `completed_with_errors` with **zero documents persisted to Data Plane v2**. This directly violates the CoreSystem rule "Ingestion Plane … persists durable knowledge through Data Plane contracts only," and means the entire import feature is a no-op end-to-end even when the DB layer (Finding 1) is healthy.

Note the host AND path are both wrong for DP v2: even `.env.example` points at `http://document-service:3021/api/v1/documents/import`, which is also not `dpv2-documents-api:8010`. The correct wiring needs both the host (`dpv2-documents-api:8010`) and the path aligned to DP v2's Go ingest contract (the same one quarry-edge uses via `DATA_PLANE_INGEST_URL`). This should be verified against the DP v2 documents-api route table before flipping.

## Finding 3 — Deploy drift: knowledge-sync route committed but not live `[live-curl]`/`[source-only]`

The committed source (`main.py:225`) exposes `POST /api/v1/import/jobs/knowledge-sync`, plus `app/knowledge_sync.py`, `app/actions_gateway.py`, `app/m365_provider_handler.py`, and `app/notion_import.py` — all committed on 2026-07-07 (`051ae380` Phase 3, `eb0ea7d9` Phase 4). `git status` for `imports-core` is **clean** (fully committed; the "large uncommitted work" caveat in the audit brief does not apply to imports-core).

But the running image's live OpenAPI exposes only the 6 legacy paths, and `POST /jobs/knowledge-sync` returns 405 (path only matches the `{job_id}` GET route). So the running image predates the Phase 3/4 code. The image was built 2026-07-07 21:12 — nominally after the 12:xx commits — so the most likely mechanism is the documented BuildKit stale-layer gotcha ("Docker rebuild SILENTLY serves stale binary; use `--no-cache`"). Cannot be corrected this pass because image rebuild is Docker-blocked. Net: the Phase 4 GitHub/Slack knowledge-sync feature is **not reachable in the running system**, only in source.

## Finding 4 — Auth boundary: read routes have NO authorization `[source-only]`/`[live-curl]`

- **Write routes are protected** (`[live-curl]` 401 without key): `POST /upload`, `POST /source`, `POST /knowledge-sync` all take `auth: AuthContext = Depends(require_internal_auth)`. `require_internal_auth` enforces a constant-time (`hmac.compare_digest`) internal-API-key check plus a required `X-Org-Id`. Good.
- **Read routes have no auth dependency at all** (`[source-only]`): `get_job` (`main.py:275`) and `stream_job_events` (`main.py:286`) declare no `Depends(...)`. This is stronger than the baseline phrasing "executes before authz" — there is literally no authorization on the read/SSE path. Any caller reaching the port can request any job by UUID and receive its full metadata + items (org_id, user_id, source names, error messages) and subscribe to its live progress. `[live-curl]` confirms the read path reaches the handler with no auth rejection (it 500s, not 401 — see Finding 1). This is an IDOR/BOLA-class gap: no `require_internal_auth`, no org-scoping of the lookup, no ownership check. The org boundary is currently only enforced on writes.

Recommended fix: add `require_internal_auth` to both read routes and constrain `get_job_with_items` / `get_job` by `auth.org_id` (filter `ImportJob.org_id == auth.org_id`, return 404 on mismatch to avoid an existence oracle).

## Finding 5 — Incomplete feature: Temporal orchestration is a no-op `[source-only]`

`app/orchestration.py` — with `TEMPORAL_ENABLED=true` (which the running `.env` sets) `dispatch` connects to Temporal but the workflow submission is commented out (`# Future: submit workflow to Temporal` / `# Example: await self._temporal_client.start_workflow(...)`). Execution ALWAYS falls through to `asyncio.create_task(runner(job_id))`. So:
- Temporal is decorative overhead (a connection is opened, never used).
- Jobs run as fire-and-forget in-process asyncio tasks (no reference retained), so an `imports-api` restart drops all in-flight jobs — there is no durable orchestration despite the README's "Async processing with Temporal orchestration" claim and `IMPORTS_COMPLETION_SUMMARY.md`'s "✅ Job orchestration (Temporal + async)".

## Finding 6 — Incomplete feature: M365 provider-linked handler is a dead-end stub `[source-only]`

`app/m365_provider_handler.py` (`M365ProviderLinkedHandler.handle`, wired to the Control Plane `user.provider_linked` event):
- Inserts an `ImportJob` with **`org_id=""`** and a comment "Will be retrieved from Control Plane in future". An empty org_id is a tenant-isolation/data-quality smell.
- Sets `status="pending"` for `source_type="m365"`, then only logs a numbered "next steps" plan ("In a real implementation, queue setup task…"). Nothing processes `source_type="m365"` (it is not in `import_from_source`, and `run_job` is never dispatched for it), so these rows are orphaned and never progress.
- `cleanup()` performs no DB work — it only logs; the intended "cancel jobs / remove OAuth tokens" is comment-only.

Net: the M365 auto-provisioning chain (`ControlPlaneSubscriber` → `m365_handler.handle`) terminates in a stub. The subscriber itself is real and correctly filters identity-only OAuth scopes (`{openid, profile, email}`) so it won't fire on plain sign-ins.

## Finding 7 — Connectors & parsers are REAL (no stubs) `[source-only]`

Grep for `todo|fixme|mock|stub|fake|placeholder|not implemented|hardcod|xxx|hack` across `app/` returned **zero** production hits (only test doubles under `tests/`). Concretely:
- `parsers.py`: real PDF (`pypdf`), DOCX (`python-docx`), CSV, JSON, HTML (`bs4`/`lxml`), TXT/MD; optional Tika pre-pass with local fallback. Solid.
- `connectors.py`: real SDK calls — HubSpot (`hubspot`), Salesforce (`simple_salesforce`), Odoo (`odoorpc`), generic CMS (`httpx`). These fail without real credentials but are not stubs.
- `notion_import.py`: fetches the REAL page body via `blocks.children.list` (one level of nested children), renders light-markdown — the Phase 3 fix over stringified metadata. Real.
- `knowledge_sync.py` + `actions_gateway.py`: a genuine worker that discovers connections and pulls GitHub README / Slack history through integration-corev2's actions surface, gating strictly on sensitive capabilities (`repo.contents.read`, `channels.history`) and never touching provider OAuth tokens. Emits audit events even on whole-provider failure. Real and careful. (Only live once the deploy drift in Finding 3 is resolved.)

## Data model, migrations, schema-compat `[source-only]`

- `models.py`: `import_jobs` + `import_job_items` (SQLAlchemy 2.0 typed mappings). ORM attribute `metadata_json` maps to DB column `"metadata"` (avoids the reserved-name clash). UUID PKs, JSONB metadata, cascade delete.
- `migrations/001_init.sql` + `002_performance_indexes.sql`: raw-SQL DDL run at startup under a Postgres advisory lock. Indexes on `(org_id,status)`, `(job_id,status)`, `status`, `created_at`.
- `db.py::_prepare_schema_compatibility`: legacy-table archival — if `import_jobs`/`import_job_items` exist but lack expected columns or have non-UUID id/job_id types, they are renamed to `*_legacy[_n]`. This is a real (destructive-rename) migration guard, not dead code; it is the "legacy suffix" residue the prior doc flagged.
- `requirements.txt` lists `alembic==1.16.4` but migrations are hand-rolled SQL — alembic is unused (dead dependency).

## Cross-plane events `[source-only]`

Two NATS connections (matches the platform's two-token design):
- `events.py` `event_publisher` → local `NATS_URL` (compose: `nats://nats:4222`) for intra-plane subjects (`import.started/completed`, `imports.knowledge_sync.run`).
- `shared_nats.py` `SharedNatsPublisher` → `VEREVON_NATS_URL` (verevon-nats), JetStream stream `VEREVON_INGESTION` on `verevon.ingestion.>` for cross-plane consumers, plus plain-NATS `verevon.notifications.import.completed`. All publishes are fire-and-forget (never break the caller). Minor doc drift: the docstring says "AQENCIA_INGESTION stream" but the code creates `VEREVON_INGESTION`.
- `progress.py` `ProgressHub` is in-memory per-process; SSE progress only works because jobs run in the same process (Finding 5). Won't survive horizontal scale-out.

## Visma question (out of scope for imports-core) `[source-only]`

The user's "test the Visma MCP" goal is **not** served by imports-core. `SourceImportRequest.source_type` is a fixed `Literal[notion, crm, erp, cms, pim, hubspot, salesforce, odoo]` — there is no Visma source, connector, or route here. The Visma path is a separate Visma Net MCP server (see the `visma-salgsordre-test` skill / `visma_net_mcp` connector), independent of the Ingestion Plane. Do not expect imports-core to reach Visma.

## Test coverage `[source-only]`

Tests exist for **only the three Phase 3/4 modules**: `test_actions_gateway.py` (5 tests, httpx `MockTransport`), `test_knowledge_sync.py` (9 tests, `FakeGateway`/`FakeAudit`), `test_notion_import.py` (6 tests, monkeypatched `FakeNotion`). ~20 test functions total. The **core pipeline has zero tests**: `parsers.py`, `connectors.py`, `service.py` (the actual import/quota/store logic), `main.py` (routes + auth), `db.py` migrations, `orchestration.py`, `control_plane_subscriber.py`, `m365_provider_handler.py`. There is no `pytest.ini`/`pyproject.toml`/`conftest.py`, and `pytest`/`pytest-asyncio` are **absent from `requirements.txt`**. Not run this pass (no Python env set up; running them would require installing dev deps). Coverage is far below the 80% target for the module as a whole — the tested surface is the newest ~15% of the code.

## Prioritized findings

1. **P0 (live outage):** DB-touching endpoints 500. Restart `imports-api` (Postgres restarted under it; suspected stale async pool). Finding 1. `[live-curl]`
2. **P0 (feature non-functional):** `DOCUMENT_SERVICE_URL=http://mock-document-service:3030` points at a non-existent host — imports never reach Data Plane v2. Repoint to `dpv2-documents-api:8010` with the DP v2 ingest path. Finding 2. `[live-curl]`/`[source-only]`
3. **P1 (security/IDOR):** `get_job` and `stream_job_events` have no auth and no org-scoping — any caller can read/stream any org's job. Add `require_internal_auth` + org filter. Finding 4. `[source-only]`
4. **P1 (deploy drift):** committed Phase 4 knowledge-sync route not in the running image (stale build layer). Rebuild `--no-cache` once Docker is unblocked. Finding 3. `[live-curl]`
5. **P2 (incomplete/over-claimed):** Temporal orchestration is a no-op; jobs are non-durable in-process tasks. Finding 5. `[source-only]`
6. **P2 (dead-end stub):** M365 provider-linked handler writes orphan `org_id=""` pending jobs and never provisions; `cleanup()` is log-only. Finding 6. `[source-only]`
7. **P3 (cleanup):** `alembic` dep unused; `shared_nats` docstring says AQENCIA_INGESTION vs actual VEREVON_INGESTION; README/COMPLETION_SUMMARY overstate Temporal + test coverage. `[source-only]`

## Bottom line

The committed imports-core code is genuinely good — real parsers, real credential-backed connectors, a careful token-free knowledge-sync worker, and real Notion body extraction, with no production stubs in the hot path. The problems are operational and integration-level, not "fake code": the running deployment's DB layer is currently throwing 500s on every job operation (P0, likely a stale connection pool after a Postgres restart), and even when healthy the service ships parsed documents to a mock host that does not exist, so nothing lands in Data Plane v2 (P0). Add authz to the read/SSE routes (P1), rebuild to pick up the committed knowledge-sync route (P1), and finish or remove the Temporal and M365 features (P2). imports-core is not the Visma path.
