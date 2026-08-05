# leads-core

> **2026-07-13 superseding update.** The Rosetta/x86 TLS diagnosis below is stale. Host, daemon, image, and running container are arm64. The exact running container reports `aarch64`/Debian 12 and completed DNS, TLS, and a one-row public Brreg request with HTTP 200; an earlier authenticated tenant-shaped service probe also returned 200. No corrective architecture rebuild was needed or performed. The current service still uses one shared internal key plus caller-supplied org/user headers, so a key-holder can forge tenant scope; this is a critical plane-wide authority blocker. Other gaps are upstream rate/timeout/circuit evidence, immutable rollout correlation, and reproducible signed multi-architecture CI. Use `docs/runbooks/leads-core-native-build-deploy-2026-07-13.md`; do not repeat the older architecture conclusion as current fact.

_Audit date: 2026-07-11. Evidence grades: **[live-curl]** verified against the running container on host `:3164`; **[source-only]** read from disk (no exec/build/logs — Docker containerd content store is corrupted fleet-wide); **[inspect]** from `docker ps`/`docker inspect` config+state._

## Current State

`leads-core` (Go/Gin, container `leads-core`, host `:3164`) is a **real, fully wired** Application-Plane service. Build is clean (`go build ./...` exit 0, go1.26.2), `go test ./...` is green (brreg/leads/providerleads packages), and the git worktree for the service dir is **completely clean** — no uncommitted WIP. **[source-only]**

It has two clearly separated domains:

1. **Company lead-builder (W1)** — filtered Enhetsregisteret (Brreg) company search, org-scoped saved lists (`lead_lists` / `lead_list_companies`), and metered CSV export with a per-export NATS audit event. **COMPANY DATA ONLY.**
2. **Provider lead sync** — LinkedIn Lead Gen form responses pulled through integration-corev2's actions gateway into the `provider_leads` table. This is the one deliberately **person-data**-carrying surface, and it is contained (see PII posture below).

At startup (`cmd/server/main.go`): load config → connect `application-postgres` → run migrations → build Brreg client + leads service → (optionally) build the provider-lead syncer if `INTEGRATION_CORE_URL` set → (optionally) connect NATS for best-effort audit → start the interval sync worker → serve HTTP. All optional dependencies degrade honestly (503 / disabled-log), never silently faked.

## Historical July 11 finding — company lead-builder returned 502

- Host → `data.brreg.no` directly: **HTTP 200**, real data (kommune 4601 = 57,829 companies; first record `916627939`). Brreg is real and reachable from the host. **[live-curl]**
- Container `leads-core` → `/api/v1/leads/search`, `/companies/{orgnr}/financials`, `/companies/{orgnr}/branches`: all return **HTTP 502** with the honest handler error `{"error":{"code":"brreg_error",...}}`. **[live-curl]**
- Ruling out network isolation: both attached networks (`app-net`, `inter-plane-bus`) are ordinary bridges with gateways (`172.24.0.1`, `172.18.0.1`), **not** `internal: true`, so egress NAT exists in principle. **[inspect]**

Historical conclusion only: this was consistent with a container TLS/runtime failure at that time. It is disproved for the July 13 runtime by direct in-container arm64 DNS/TLS/HTTP 200 evidence and must not be used as a current release finding.

## Entry Points

- Main: `apps/Application Plane/leads-core/cmd/server/main.go`
- Routes: `apps/Application Plane/leads-core/internal/http/server.go`
- Handlers: `internal/http/handlers.go`, `internal/http/handlers_provider_leads.go`
- Brreg client: `internal/brreg/client.go` (+ `client_test.go` invariant test)
- Company service/repo: `internal/leads/{service,repository,types}.go`
- Provider-lead sync: `internal/providerleads/{sync,worker,repository,types}.go`
- Audit: `internal/audit/publisher.go`
- Config: `internal/config/config.go`; migrations: `internal/database/migrations/00{1,2}_*.sql`

## Exposed Surface

All `/api/v1/*` and `/internal/*` routes are gated by a constant-time `x-internal-api-key` check (`requireInternalKey`). `/health` and `/ready` are open.

- `GET  /health`, `GET /ready` — open **[live-curl 200]**
- `POST /api/v1/leads/search` — filtered Brreg `/enheter` company search (needs `x-org-id`)
- `GET  /api/v1/leads/companies/:orgnr/branches` — Brreg `/underenheter`
- `GET  /api/v1/leads/companies/:orgnr/financials` — Regnskapsregisteret `/regnskap/{orgnr}`
- `POST /api/v1/leads/build_list` — governed `leads.build_list` action (search → optional branch enrich → dedupe → save)
- `GET/POST /api/v1/leads/lists`, `GET/DELETE /api/v1/leads/lists/:id`, `GET /api/v1/leads/lists/:id/export.csv`
- `POST /internal/sync/provider-leads` — manual LinkedIn lead sync trigger (503 if `INTEGRATION_CORE_URL` unset)
- `DELETE /api/v1/provider-leads?organizationId=…` — org-scoped GDPR erasure

Live auth gating verified **[live-curl]**: no key → 401; wrong key → 401; unknown route → 404; search without `x-org-id` → 400 `missing_org_id` (validated before any outbound call).

## Verified guardrails

- **Company-only (real, test-enforced).** The Brreg client calls only `/enheter`, `/underenheter`, `/regnskap` — never `/enheter/{orgnr}/roller`, so it never fetches a person, role, or fødselsnummer. `rawEnhet`/`rawUnderenhet`/`rawRegnskap` map an explicit allowlist of company/aggregate fields. `client_test.go` has `TestCompanyOnlyInvariant_NoPIIFieldsOnAnyRecord`: it feeds a fixture that deliberately contains `roller`, `fodselsnummer`, "Ola Nordmann", `epostadresse`, `telefon` and asserts NONE appear on any serialized `Company`/`Branch`/`Financials`. Schema (migration 001), CSV header, and dedupe are all company-only too. **[source-only]**
- **PII posture is genuinely contained.** `provider_leads` (migration 002) is the ONE person-data table (raw LinkedIn form answers in a `fields` JSONB). It is never joined into the company tables, never enters the CSV export path, and erasure is one org-scoped `DELETE ... WHERE org_id = $1`. Sync-audit events carry **counts only** (`Connections/Forms/LeadsFetched/LeadsUpserted/Skipped`) — never form answers. The package doc + migration comment + README "PII posture" all state and the code enforces this. **[source-only]**
- **Metering/audit.** CSV export emits `verevon.audit.v1.application.lead_export` (org, user, list id, count) and sets an `X-Lead-Count` response header for the gateway; provider sync emits `verevon.audit.v1.application.provider_lead_sync`. Both are **best-effort** (a NATS publish failure never fails the operation). Metering here is an **audit trail with counts**, not a hard in-service quota/limit — enforced quotas would be Control Plane's job. audit-core subscribes `verevon.audit.v1.>`. **[source-only]**
- **IDOR-clean.** `x-org-id` is resolved server-side (gateway sets it) and is never read from the request body; every repository query is scoped `WHERE org_id = $1` (list/get/delete/export). `build_list` takes org/creator from the resolved identity, not the filter. **[source-only]**
- **Key-gated.** Yes — full HTTP surface behind `INTERNAL_API_KEY`; compose enforces `${INTERNAL_API_KEY:?...}` (no `change-me` default), and `config.Load()` fails startup if empty or if `DATABASE_URL` empty. **[source-only]** / **[inspect]** (`.env` has a real 64-char key).

## Relationships

- **integration-corev2** (`integration-api:3026`, default `INTEGRATION_CORE_URL`) — provider-lead sync lists `linkedin` connections and executes `linkedin.lead.forms` / `linkedin.lead.responses` via `POST /api/v1/actions/execute` with the shared `X-Internal-API-Key`. leads-core never sees provider OAuth tokens (token resolution + provider HTTP happen inside integration-corev2). Only connections carrying the `social.leads.read` capability with a resolvable owner URN are synced; the rest are honestly skipped with a recorded reason. **[source-only]**
- **application-postgres** — `DATABASE_URL` targets the shared Application-Plane Postgres (DB `notifications`, `APPLICATION_PLANE_DB_*`→`NOTIFICATION_DB_*` fallback). leads-core owns its own tables (`lead_lists`, `lead_list_companies`, `provider_leads`); this is same-plane co-tenancy of one Postgres instance, **not** a cross-plane DB crossing. **[inspect]**
- **verevon-nats** (`VEREVON_NATS_URL`, inter-plane bus) — best-effort audit publishing → audit-core.
- **data.brreg.no** — the only public-internet dependency (no API key). Real. **[live-curl from host]**

## Stub / Mock / Placeholder / Partial audit

- **No genuine stubs, mocks, fakes, TODOs, or placeholders in production code.** The only grep hit for those terms in non-test `.go` files is a doc comment ("Implemented by internal/integration.Client; faked in tests") — an honest description of the test double. **[source-only]**
- `providerleads/sync_test.go` uses an in-memory `fakeRepo` and a faked actions gateway — legitimate test doubles, not production shims.
- Honest degradation (not stubs): provider-lead routes return **503 `not_configured`** when the syncer/repo is unwired; a whole-run gateway failure now emits a single `failed` audit event so the failure is visible instead of silent.

## Container / build

- `docker ps`: `Up 2 days (unhealthy)`, `restarts=0`, running since 2026-07-09. The **"(unhealthy)"** is the Compose `CMD-SHELL wget` healthcheck failing under the broken exec/containerd layer — **not** the service; `/health` and `/ready` return 200 live. **[inspect]** / **[live-curl]**
- Dockerfile: multi-stage `golang:1.25-bookworm` → `debian:bookworm-slim`, non-root `appuser`, `CGO_ENABLED=0`, `ARG TARGETARCH` present (Rosetta-arch fix already applied at source). Not rebuilt this pass (containerd corruption). **[source-only]**

## Notes

The prior in-container Brreg blocker is resolved in the current native runtime. Company-only mapping and PII containment remain genuine strengths. The service is not IDOR-clean against a compromised internal caller because the shared key does not cryptographically bind organization/user scope; replace that boundary, add wrong-tenant/role tests, and retain real Brreg provenance plus honest degraded responses.
