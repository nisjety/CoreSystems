# Ingestion Plane Audit

Baseline date: 2026-07-02
Live verification dates: 2026-07-10 (first pass) and 2026-07-11 (full 7-service re-verification, this pass)

Scope: `apps/Ingestion Plane` (shipping-core, Quarry-v2, integration-corev2, imports-core, finspo-core, autocomplete-core, support-worker)

> **Follow-up 2026-07-12:** this file remains the historical live audit. The source remediations and current release decision are tracked in `../../INGESTION_PLANE_STATUS.md`; deployment evidence below has not changed because Docker rebuild/restart still requires operator approval. The principal source fixes cover shipping/imports/integration auth and tenant pinning, durable imports, real Data Plane contracts, fail-closed quota, connector SSRF, production Quarry HMAC, autocomplete fail-closed startup, and truthful support-worker declassification.

See also `INGESTION_PLANE_STATUS.md` (current-state snapshot) and `INGESTION_PLANE_ROADMAP.md` (fix plan) at the plane root, and the per-service docs in this directory (all re-verified 2026-07-11, plus a new `shipping-core.md`).

## 2026-07-11 re-verification — executive summary

Verified with **host-side curl + source/config reading only**, because Docker's containerd content store is currently corrupted (`openat etc/passwd: input/output error`): `docker exec`, image rebuild, and `docker logs` are all broken fleet-wide. Every app container therefore shows "(unhealthy)" — that is a **false negative** from the exec-based healthcheck failing, not the service: 11 of 12 host-published Ingestion endpoints returned HTTP 200. Findings are marked `[live-curl]`, `[source-only]`, or `[inspect]` by evidence grade.

### Headline: the shipping-time defect is now FIXED IN SOURCE (deploy pending)

The user's opening complaint — verevon chat can't answer "shipping time Oslo→Trondheim" — traced to a live-confirmed **Bring delivery-time parsing defect** in shipping-core: `/api/quotes` returned real Bring prices but `transit_days:0` and `0001-01-01` for every Bring product, because `internal/carrier/bring/wire.go` modelled only `alternativeDeliveryDates[]` while Bring returns the promise at the **top level** of `expectedDelivery` (`workingDays` / `formattedExpectedDeliveryDate` / structured `expectedDeliveryDate`). The large uncommitted shipping-core transformation left the Bring adapter untouched, so the defect was still live.

**Fixed this pass (2026-07-11):** `wire.go` now models the top-level fields + a structured-date fallback; `bring.go`'s `toDomainQuote` prefers the top-level promise and falls back to `alternativeDeliveryDates[0]` only when the top level is absent; a new `parseBringDeliveryDate` helper resolves the formatted `dd.MM.yyyy` string with a structured year/month/day fallback for localized strings. Two production-shaped tests were added (`TestAdapter_Quote_ParsesTopLevelExpectedDelivery`, `TestAdapter_Quote_FallsBackToStructuredDate`) — the old test had encoded the bug by using the `alternativeDeliveryDates` shape in its fixture. `go build`/`go vet`/`go test ./...` all green (8/8 Bring quote tests pass). **Not yet live**: the running shipping-core image is from 2026-07-04 and cannot be rebuilt until the Docker content store is repaired.

### Findings by service (2026-07-11)

| Service | State | Key findings |
|---|---|---|
| **shipping-core** :3156 | Live, real, 1 fix applied | Bring delivery-time defect **fixed in source** (above). Still open: no auth/authz/tenant/rate-limit/ZDR on the router (`/api/quotes`, `/api/carriers`, and the whole booking lifecycle serve 200 with no credential) `[live-curl]`; carrier provenance still `is_mock`-only (reports `is_mock:false` for sandbox DHL/FedEx/UPS — only Bring is production) `[live-curl]`; deploy drift (reliability + recommend routes 404 in the 2026-07-04 image though wired in source); compose block never maps `AUTH_CORE_URL`/`MODEL_GATEWAY_URL`/`INTERNAL_API_KEY`/`NATS_URL`/`DATA_PLANE_*`, so the new modelplane/recommend/dataplane/events features stay inert even after rebuild. The 5 new subdirs (modelplane, recommend, reliability, events, dataplane) are **real, tested implementations, not stubs**. shipping-core does NOT self-expose as a Model Plane tool — verevon chat reaches it via Model Plane `execution-core/src/shipping_tools.rs` → `SHIPPING_CORE_URL` :3156. |
| **Quarry-v2** :8082/:8081 | Live, fetch real; posture unsafe | Scrape/fetch path is genuinely real (`/v1/scrape` returned extracted markdown + artifacts) `[live-curl]`. **3 unsafe dev-posture findings still stand**: `QUARRY_EDGE_AUTH_DEV_BYPASS=1` (any non-empty bearer accepted), `QUARRY_INTERNAL_HMAC_REQUIRED=0` (control accepts unsigned `/v1/*`), quarry-control host-published on `0.0.0.0:8081` — together an **unauthenticated LAN-reachable durable control plane (HIGH)**. Web search returns **zero results** — bundled SearXNG's engines all fail (`HTTP connection error`), a SearXNG egress problem, not Quarry code (corrects the 2026-07-10 claim that SearXNG results came back). Non-durable in-memory artifact/index/queue backends live in the edge. Temporal schedule/backfill still stubbed. Resolved since baseline: edge 500s now 0/12h (was 62), `/v1/sources` now real org-scoped CRUD, `DataPlaneIngestRequest` contract 21/21. Uncommitted ZDR gRPC hardening not yet in images. |
| **integration-corev2** :3026 | Live, real | `/health/detailed` 200 (20 providers, all capabilities up); `/api/v1/connections` correctly 401s; GitHub webhook signature verification **re-confirmed fail-closed** (HMAC-SHA256, constant-time). **Visma is ABSENT from the 20-provider catalog** — the user's "test the Visma MCP" cannot be done through integration-corev2. Shipping is catalog-metadata only (no action dispatch). Uncommitted `DataPlaneServiceToken` handoff hardening (shared key + caller `X-Org-ID` → scoped Bearer JWT, no forwarded identity headers) is safe, respects the collision lock, and is currently dormant. |
| **imports-core** :3025 | **Live outage + misconfig** | **Every DB-touching endpoint 500s right now** (`GET job` and authenticated upload; non-DB endpoints 200) — leading cause a stale asyncpg pool after ingestion-postgres restarted under a 3-day-old imports-api container; fix = restart imports-api (unconfirmable — `docker logs` blocked) `[live-curl]`. `DOCUMENT_SERVICE_URL=http://mock-document-service:3030` points at a **non-existent host**, so imports never reach Data Plane v2 (violates "persist via Data Plane contracts only") `[source+live]`. IDOR: job GET/SSE read routes have no auth/org-scoping `[source-only]`. Committed Phase 4 knowledge-sync not in the running image (stale build). Temporal orchestration is a no-op (in-process asyncio tasks). M365 handler is a dead-end stub writing orphan `org_id=""` jobs. |
| **finspo-core** :3130 | Live, strongest-shaped | SharePoint Graph delta-sync is **genuinely implemented + test-covered** (delta pagination/cursor resume, tombstones, ACL capture, scheduler, real PDF extraction), persists only through Data Plane documents-api, ZDR-stamped, auth enforced before every handler (stronger than imports-core), destructive execution hard-gated off. Uncommitted `DataPlaneServiceToken` rename is complete, not WIP. One warning: `/ready` reports the finspo Postgres pool timed out (503) though ingestion-postgres is healthy — likely a stale pool on the degraded host. Not on the headline path; not a Model Plane tool. |
| **autocomplete-core** :3219 | **Down — cannot start** | Real, fully-tested Rust/Axum typeahead sidecar (Sonic + SQLite + NATS JetStream, per-org isolation), but **entirely absent from the fleet**: the ROOT monorepo compose (not the Ingestion compose) gates it + quarry-sonic on `${AUTOCOMPLETE_INTERNAL_TOKEN:?}` and `${SONIC_PASSWORD:?}`, which the root `.env` never sets, so `compose up` aborts before creating them. Even if started, its NATS consumer points at a non-existent `quarry-nats` host on the wrong network, so the index would stay empty. Net user effect: the verevonv3 searchbar returns empty suggestions on every keystroke, silently masked by the gateway's degrade-to-empty handler. |
| **support-worker** | Live but inert | Real Temporal worker + NATS→Temporal bridge for Zammad support-ticket automation (no stubs in src), but **functionally dead-ended**: no producer publishes `verevon.support.*`, no Zammad is deployed, `classifyActivity` targets a phantom `ai-core:8001`, `patchZammad` targets an absent host with an empty token, and the notify path 404s on notification-core. Alive but receives no input and no path can complete. |

### Cross-plane / headline resolution

- **"Shipping time Oslo→Trondheim"**: root cause (Bring parsing) fixed in source this pass; needs a shipping-core rebuild to go live, plus the compose env wiring so the Model Plane path is fully connected. The chat path itself is real (Model Plane `execution-core` shipping tools → shipping-core :3156).
- **"Test the Visma MCP"**: not an Ingestion Plane capability. Visma is absent from integration-corev2's catalog; shipping-core only carries a passthrough `VismaOrderRef` audit field, no Visma call. The Visma Net MCP is a separate connected MCP server (requires its own auth), not wired into any Ingestion service.

### Legacy doc review (13 top-level docs)

Edited in place (update): `README.md`, `PORT_MAPPING.md` (was badly stale — claimed a non-existent Quarry API :8090; corrected to quarry-edge :8082 / quarry-control :8081, NATS 4224, Dragonfly-not-Redis), `INGESTION_PLANE_DEEP_DIVE.md` (added missing shipping-core/support-worker/integration-email-worker), `IMPORTS_COMPLETION_SUMMARY.md`. Report-only recommendations (added to `apps/STALE_DOC_DELETION_REGISTER.md`): `QUICKSTART.md` → **delete** (targets a non-existent `docker-compose.full.yml` and the deleted Quarry/ Go dir); `COMPLETION_REPORT.md`, `END_TO_END_TESTS.md`, `FINAL_TEST_SUMMARY.md`, `TESTING_SUMMARY.md`, `TESTING_INDEX.md`, `TEST_DOCUMENTATION_INDEX.md`, `TEST_REPORT.md` → **archive** as one cluster (all Feb-19/20 point-in-time reports describing a defunct 9000-series two-service Quarry+Imports stack). `TESTING_CHECKLIST.md` was not reached (one agent hit the output-retry cap).

---

## Prior passes (preserved below)

This is a plane-local audit report. It focuses on Quarry-v2, import/connectors, and cross-plane ingestion boundaries without editing service code.

> **Live update — 2026-07-10:** The original report below is preserved as the 2026-07-02 baseline. A running Docker stack was verified on 2026-07-10 using read-only or otherwise non-mutating probes. No shipment was booked/confirmed/cancelled, no import was created, no provider action was executed, and no social post was published. The update below supersedes historical status statements where explicitly noted.

## 2026-07-10 Live Verification

### Evidence policy

The verification distinguished:

- container health from effective endpoint behavior;
- configured providers from live token/API usability;
- production carrier APIs from vendor sandboxes and local mocks;
- current working-tree source from the older running images; and
- a route being implemented from Verevon chat actually receiving that tool.

Secrets were read only indirectly where an internal authenticated read was necessary. No key, token, customer number, account identifier, organization identifier, connection identifier, or user content was printed or added to this document.

### Running services

| Container | Published endpoint | Health | Restart count at verification |
|---|---|---|---:|
| `shipping-core` | `0.0.0.0:3156 -> 8080` | healthy | 3 |
| `integration-api` | `0.0.0.0:3026` | healthy | 2 |
| `imports-api` | `0.0.0.0:3025` | healthy | 1 |
| `quarry-edge` | `127.0.0.1:8082` | healthy | 0 |
| `quarry-control` | `0.0.0.0:8081` | healthy | 0 |
| `social-core` | `0.0.0.0:3162` | healthy | 0 |

Health is not readiness proof. Integration had three unusable tokens despite green readiness, and Quarry logged recent HTTP 500s despite green health.

### Sanitized commands and results

Representative requests used during the live verification:

```bash
# Public health/readiness
curl -sS http://127.0.0.1:3156/healthz
curl -sS http://127.0.0.1:3026/health/detailed
curl -sS http://127.0.0.1:3025/health
curl -sS http://127.0.0.1:8082/ready
curl -sS http://127.0.0.1:3162/health

# Non-mutating shipping inventory and quote
curl -sS http://127.0.0.1:3156/api/carriers
curl -sS -H 'Content-Type: application/json' \
  --data-binary '{"from":{"name":"Verevon Test","postal_code":"0150","city":"Oslo","country":"NO","is_business":true},"to":{"name":"Verevon Test","postal_code":"7010","city":"Trondheim","country":"NO","is_business":true},"package":{"weight_kg":5,"length_cm":30,"width_cm":20,"height_cm":15,"dangerous_good":false},"segment":"b2b"}' \
  http://127.0.0.1:3156/api/quotes

# Auth boundary checks
curl -sS http://127.0.0.1:3026/api/v1/connections
curl -sS http://127.0.0.1:3025/api/v1/import/jobs/00000000-0000-0000-0000-000000000000
curl -sS -H 'Content-Type: application/json' \
  --data-binary '{"query":"Bring Norway shipping guide","limit":5}' \
  http://127.0.0.1:8082/v1/search

# Quarry read with ZDR/no-ingest; a disposable bearer was sufficient only
# because the inspected dev container had auth bypass enabled.
curl -sS -H 'Authorization: Bearer <disposable-test-value>' \
  -H 'Content-Type: application/json' \
  --data-binary '{"url":"https://www.bring.no/en/","zdr":true,"ingest":false,"cache":{"mode":"bypass","max_age_s":0,"vary_on":[]}}' \
  http://127.0.0.1:8082/v1/scrape
```

The quote used a 5 kg, `30 x 20 x 15 cm`, non-dangerous B2B parcel from Oslo `0150` to Trondheim `7010`, with test-only sender/recipient names.

| Probe | HTTP/result | Evidence |
|---|---|---|
| Shipping health/readiness | 200 / DB `ok` | Process and shipping database reachable. |
| Shipping carrier inventory | 200 without auth | Eight adapters: four explicit mocks and four credential-backed adapters. |
| Shipping quote, direct and through Verevon gateway | 200 without auth | 15 options; Bring/DHL/UPS returned rates, FedEx returned a sanitized authorization error. |
| Shipping reliability and recommendation | 404 in running images | Routes exist in current source but are absent from the deployed binaries. |
| Integration detailed health | 200 | Reports OAuth, token broker, discovery, actions and webhook hot path enabled. |
| Integration connections without auth | 401 | External authentication gate works for this surface. |
| Integration provider catalog | 20 providers | No Visma provider is present. |
| Integration live discovery | 8 success / 3 token failures | Active database status does not imply an effective token. |
| Imports upload without internal key | 401 | Write boundary rejected missing auth. |
| Imports unknown job GET without auth | 404, not 401 | Job lookup executes before any authorization; database contained zero jobs at verification. |
| Quarry Edge search without bearer | 401 | Missing bearer rejected. |
| Quarry Edge search with arbitrary bearer | 200 | Dev bypass accepts any non-empty bearer. SearXNG returned five current Bring-related results. |
| Quarry Edge ZDR scrape | 200 | Bring page fetched with static driver, `zdr=on`, no ingest, cache bypass and third-party processing disabled. |
| Quarry Control jobs without auth | 200 | Dev deployment had `QUARRY_INTERNAL_HMAC_REQUIRED=0`. |
| Social accounts without internal key | 401 | Shared-key boundary active. |
| Social read paths with internal key | 200 | Four accounts across connected organizations; three tokens available, one expired. |

### Carrier provenance and the Bring delivery-time defect

| Carrier | Runtime classification | Live quote result | Important limitation |
|---|---|---|---|
| Bring | Credential-backed, production Shipping Guide endpoint | Success | Shipping-core drops the returned delivery time. Booking was not exercised. |
| DHL Express | Credential-backed vendor test endpoint | Success | Sandbox/test data, although `/api/carriers` reports only `is_mock=false`. |
| UPS | Credential-backed CIE test endpoint | Success | Sandbox/test data, although `/api/carriers` reports only `is_mock=false`. |
| FedEx | Credential-backed sandbox endpoint | Failed authorization | Configuration exists, but effective rate access is not working. |
| PostNord, DSV, Helthjem, Porterbuddy | Explicit local mock adapters | Mock rates returned where segment-compatible | Must never be presented as a live carrier quote. |

A sanitized direct read-only call to Bring's production Shipping Guide for three products on the same Oslo-to-Trondheim parcel returned:

| Bring product | Upstream working days | Upstream expected date |
|---|---:|---|
| `3570` | 4 | 2026-07-15 |
| `3584` | 4 | 2026-07-15 |
| `9300` | 2 | 2026-07-14 |

Shipping-core returned `transit_days: 0` and `0001-01-01T00:00:00Z` for those products. The live Bring payload places `workingDays` and `formattedExpectedDeliveryDate` directly under `expectedDelivery`, with an empty `alternativeDeliveryDates` array. Current code models and reads only `alternativeDeliveryDates[0]` in:

- `shipping-core/internal/carrier/bring/wire.go`
- `shipping-core/internal/carrier/bring/bring.go`

This is the direct reason shipping-core cannot currently answer the user's shipping-time question even when its live Bring connection is invoked.

Required correction:

1. Model top-level `workingDays`, `formattedExpectedDeliveryDate`, and structured `expectedDeliveryDate`.
2. Prefer the top-level promised date; use an alternative only when explicitly present and appropriate.
3. Add a contract fixture shaped like the observed production response.
4. Replace `is_mock` with provenance fields such as `mode`, `environment`, `verified_at`, and `availability`.

### Integration, Imports, Quarry and Social status

Integration Core returned 11 records marked `active`. Live metadata discovery succeeded for GitHub, Google, one Meta connection, one Slack connection, LinkedIn, Notion, X and Discord. An older Meta connection, an older Slack connection, and Microsoft failed at the token broker. Connection state should include verified effective health and a reason, rather than equating persistence with usability.

Imports Core currently had no import jobs, so a successful unauthorized record read was not manufactured merely to prove the issue. The route definitions and the live 404 response both show that job GET and SSE reach storage without auth. Those routes must require the same internal/bearer identity and organization match as job creation.

Quarry's real search/fetch path works. However, the inspected runtime is explicitly a development posture:

- `QUARRY_EDGE_AUTH_DEV_BYPASS=1`, so any non-empty bearer is accepted and placeholder tenant claims are injected.
- `QUARRY_INTERNAL_HMAC_REQUIRED=0` on control.
- Quarry Control is host-published and returned the job collection without authentication.
- The last 12 hours of edge logs contained 62 HTTP 500 responses, 3,381 Chromium/WebSocket decode warnings, six vector-retrieval fallbacks and repeated auth-bypass warnings.

Social Core's Integration-backed account path is real. Three of four observed account tokens were available; one Meta token was expired. No write/publish was attempted. The service still trusts a shared internal key plus caller-supplied organization/user headers, so it should remain an internal-only surface behind a canonical gateway authorization decision.

### Deployment/source drift

| Image | Created | Revision label | Observed drift |
|---|---|---|---|
| `ingestion-plane-shipping-core:latest` | 2026-07-04 | absent | Current-source reliability/recommendation routes return 404. |
| `frontend-plane-verevonv3-gateway:latest` | 2026-07-08 | absent | Current-source reliability/recommendation proxy routes return 404. |
| `ingestion-plane-integration-api:latest` | 2026-07-07 | absent | Live behavior verified, exact source revision unprovable. |
| `ingestion-plane-imports-api:latest` | 2026-07-04 | absent | Live behavior verified, exact source revision unprovable. |
| `ingestion-plane-quarry-edge:latest` | 2026-07-09 | absent | Live search/fetch verified under dev auth bypass. |
| `application-plane-social-core:latest` | 2026-07-07 | absent | Live read path verified, exact source revision unprovable. |

Every image should carry an OCI revision label and expose build revision/config mode in a safe status endpoint. Documentation must not claim a source feature is deployed until the running revision is known and the endpoint is probed.

### Corrected status of 2026-07-02 findings

| July 2 finding | Status on 2026-07-10 |
|---|---|
| Quarry Rust tests failed to compile | Closed in current source: workspace tests, formatting and clippy passed in the 2026-07-10 audit. |
| Quarry Rust/Go formatting drift | Rust formatting closed; Go formatting drift remains in multiple Quarry files. |
| GitHub webhook signatures failed open | Closed in current source and focused tests; not re-posted to the live provider endpoint in this read-only verification. |
| Quarry Control job registry was open in rollout mode | Still open in the inspected dev deployment: unauthenticated GET returned 200 and HMAC enforcement was disabled. |
| Quarry Edge registry protection was unproven | Missing bearer returns 401, but effective protection is defeated by the enabled any-bearer dev bypass. |
| HMAC deployment needed verification | Verified unsafe for this local deployment: enforcement is off. Production remains unverified. |
| Endpoint health was not tested | Superseded by the live matrix above. |
| Import/integration current gates were not tested | Partially superseded by live health/auth/discovery checks. A real import was not created. |
| Top-level Makefile targets legacy/stale services | Still open; `test-endpoints` uses obsolete Quarry/Temporal ports and old compose assumptions. |

### Updated remediation order

1. Add authentication, organization ownership and authorization to all shipping routes before any booking lifecycle is enabled through Verevon.
2. Fix Bring top-level expected-delivery parsing and add a production-shaped fixture.
3. Add explicit production/sandbox/mock provenance to every carrier response and AI citation.
4. Rebuild shipping-core and gateway from a revision-labelled commit; verify reliability/recommendation routes after deployment.
5. Protect Imports job GET/SSE and add cross-organization negative tests.
6. Disable Quarry Edge auth bypass, require Control HMAC and stop publishing Control directly to the host.
7. Make Integration and Social health reflect token usability, not only persisted connection status.
8. Replace stale/mutating smoke scripts with a safe read-only smoke profile plus isolated disposable-state E2E tests.

## Historical 2026-07-02 Baseline

### Current Shape

Ingestion Plane owns evidence capture and acquisition. `Quarry-v2` is the active web/search ingestion target for Verevon v3. Legacy `Quarry/` references still exist in tooling/docs and should not be used for new Verevon v3 work.

### Commands Run

| Command | Result | Notes |
|---|---|---|
| `cargo test --workspace` in `Quarry-v2` | Fail | Compile failure from stale `DataPlaneIngestRequest` constructors. |

### Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` in `Quarry-v2` | Fail | Formatting drift in `quarry-runtime` CAS, scheduler, ingest client, lib, and page-image modules. |
| `cargo clippy --workspace --all-targets -- -D warnings` in `Quarry-v2` | Fail | Same stale `DataPlaneIngestRequest` constructors plus `clippy::unnecessary_to_owned` in `pipeline.rs`. |
| `gofmt -l services pkg` | Fail | Formatting drift in quarry-control, quarry-orchestrator, and quarrycontracts files. |
| Go vet over Quarry services/packages | Pass | `quarry-control`, `quarry-orchestrator`, `quarrycontracts`, and `quarryotel` pass vet. |
| `staticcheck` over Quarry Go services/packages | Partial/blocking | Reports unused `store.mu` and unused `scheduleName`; analyzer also hits Go 1.26 vs Go 1.25 toolchain skew. |

### Live Validation Addendum

Additional validations run against the local Ingestion runtime:

| Probe | Result | Notes |
|---|---|---|
| Updated `smoke-test-integration-api.sh` | Fail on real boundary | Current health, provider catalog, auth rejection, internal key, connect-session, and not-found checks pass; GitHub webhook signature checks fail open. |
| GitHub webhook without signature | Fail | `/api/v1/webhooks/github` returned 200 accepted with no signature. |
| GitHub webhook with invalid signature | Fail | `/api/v1/webhooks/github` returned 200 accepted with `sha256=invalid`. |
| Quarry control job registry | Open risk | Host `/v1/jobs` on control returns 200 under rollout mode. |
| Quarry edge job registry | Protected/unproven | Host `/v1/jobs` on edge returns 401 without edge auth/HMAC context. Verevon onboarding still calls control directly in source. |

### High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P0 | Verevon v3 onboarding still has a documented direct `quarry-control` path that bypasses `quarry-edge`. | `Quarry-v2/docs/ARCHITECTURE.md` says onboarding crawl handlers post to control `/v1/jobs/` directly and work only while HMAC rollout mode trusts private-network calls. | Migrate onboarding crawl handlers to `quarry-edge` and block direct cross-plane control calls. |
| P0 | Integration API accepts GitHub webhooks with missing or invalid signatures in the live environment. | Updated smoke script fails because `/api/v1/webhooks/github` returns 200 accepted for both no signature and invalid signature. | Require configured provider webhook secrets or reject unsigned provider webhooks by default. |
| P1 | Quarry-v2 tests fail to compile after Data Plane ingest contract expansion. | `crates/quarry-core/tests/contracts.rs:231` and `crates/quarry-runtime/src/ingest_client.rs:380` construct `DataPlaneIngestRequest` without `initiator_user_id` and `visibility`. | Update constructors and contract tests with explicit initiator/visibility behavior. |
| P1 | Top-level Ingestion Makefile still targets legacy `Quarry`. | `apps/Ingestion Plane/Makefile` uses `cd Quarry` for setup/dev/test and docs output. | Update targets to Quarry-v2 or explicitly label legacy commands. |
| P2 | Quarry-v2 has Rust and Go formatting drift. | `cargo fmt --all -- --check` and `gofmt -l services pkg` both fail. | Run mechanical formatters after coordinating with active branches. |
| P2 | HMAC rollout mode and trust-the-network behavior need deployment verification. | Quarry-v2 architecture docs describe `quarry-control` as HMAC-internal and note degraded trust when secret is unset. | Verify production env requires HMAC and that gateway callers target edge. |
| P2 | Quarry-control resource/schedule surfaces still contain partial behavior. | Existing core research notes identify stubbed schedule/source/backfill behavior in control resources. | Convert partial resource families into tracked service issues with endpoint-level tests. |
| P2 | Runtime durability still has in-memory compatibility paths. | Existing core research notes identify in-memory store/queue paths in control/runtime. | Document allowed dev-only use and add production config guards. |
| P3 | Staticcheck found likely unused Go symbols before hitting toolchain skew. | `services/quarry-control/internal/store/store.go:219` has unused `mu`; `services/quarry-control/internal/temporal/client.go:113` has unused `scheduleName`. | Confirm these are not future hooks, then remove or wire them once staticcheck is upgraded. |
| P3 | Rust clippy has one production cleanup beyond the contract compile failure. | `crates/quarry-runtime/src/pipeline.rs:582` reports unnecessary `to_string()`. | Use the suggested borrowed value after the contract compile fix. |

### Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Endpoint health | This pass did not start Docker Compose or run `make test-endpoints`. | Run after compile failures are fixed and local services are available. |
| imports/integration/finspo current gates | No Python/Go tests were rerun in this pass. | Run service-local tests for imports-core, integration-corev2, finspo-core, autocomplete-core, and support-worker. |
| Data Plane ingest semantics | `initiator_user_id` and `visibility` need product/security decisions. | Confirm with Data and Control owners before patching defaults. |

### Quality Gate

- Quarry-v2 Rust workspace tests: fail at compile time.
- Quarry-v2 Rust format/clippy: fail.
- Quarry Go format: fail.
- Quarry Go vet: pass.
- Staticcheck: partial findings, then blocked by analyzer/toolchain mismatch.
- Endpoint smoke tests: not run.
- Import/connectors tests: not run in this pass.

### Recommended Remediation Order

1. Patch Quarry-v2 `DataPlaneIngestRequest` constructors and rerun `cargo test --workspace`.
2. Move Verevon onboarding crawl jobs through `quarry-edge`.
3. Update top-level Ingestion Makefile/help output away from legacy `Quarry`.
4. Verify HMAC-required deployment behavior.
5. Run endpoint and connector/import service tests.
