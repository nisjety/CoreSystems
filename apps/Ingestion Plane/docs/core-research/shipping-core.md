# shipping-core Research Dive

Generated: 2026-07-11
Scope: `apps/Ingestion Plane/shipping-core`

> **2026-07-12 source update:** the headline "NOT fixed" section below is retained as historical 2026-07-11 live evidence. Current source now parses Bring's top-level promise, authenticates all API routes with RS256 ingestion tokens, tenant-pins booking/manifest storage, records approval/idempotency/ZDR/retention, reports production/sandbox/mock provenance, and mints scoped Data Plane tokens for delivery evidence. Full Go test/vet is green. None of this is deployed while Docker rebuild is blocked.

Evidence grades used below: **[live-curl]** = observed against the running
container on host port `3156`; **[source-only]** = read from the working-tree
source, not proven live (rebuild is Docker-blocked); **[inspect]** = from
`docker inspect`; **[logs]** = from container logs.

> Environment caveat: the containerd content store is corrupted, so
> `docker exec` and `docker build`/redeploy do not work, and
> `docker logs shipping-core` fails with an input/output error (no [logs]
> evidence available this pass). All live checks below are host `curl` +
> `docker inspect` only.

## Snapshot

shipping-core is the Ingestion Plane's carrier-neutral freight service (Go,
chi router, pgx/Postgres). It fans a normalized quote request out to a fleet
of carrier adapters, returns cheapest-first quotes, and (in current source)
adds a two-step booking lifecycle, reliability scoring, an AI recommendation
route, NATS lifecycle events, and Data Plane delivery-evidence handoff.

Runtime posture this pass:

- Health `GET /healthz` → 200; `GET /health` → 404 (the health path is
  `/healthz`). **[live-curl]**
- The running image was built `2026-07-04T18:58:39Z` **[inspect]** — it
  predates the entire large uncommitted transformation, so several current-
  source routes are not live (see Deploy drift).
- Container is `(unhealthy)` in `docker ps` only because the healthcheck runs
  via exec, which the corrupted content store breaks; the process itself
  serves traffic fine (healthz 200). **[live-curl]+[inspect]**
- `go build ./...` and `go vet ./...` clean; `go test ./...` all green (16
  packages ok; `internal/booking`, `internal/reliability`, `db`, `docgen`
  have no test files). **[source-only]**

## HEADLINE: Bring delivery-time defect is NOT fixed

This is the direct cause of "can you check shipping time Oslo→Trondheim"
returning nothing usable.

- The Bring adapter is unchanged by the uncommitted work — `git status`
  shows `internal/carrier/bring/*` is neither modified nor untracked (dhl,
  fedex, ups were changed; bring was not). **[source-only]**
- `internal/carrier/bring/wire.go:96-101` still models
  `expectedDeliveryResponse` with **only** `alternativeDeliveryDates[]`. There
  are still no top-level `workingDays` / `formattedExpectedDeliveryDate`
  fields, which is exactly where Bring returns the promise. **[source-only]**
- `internal/carrier/bring/bring.go:175-183` (`toDomainQuote`) still reads
  `p.ExpectedDelivery.AlternativeDeliveryDates[0]` only, so when that array
  is empty the quote keeps `TransitDays=0` and a zero-value `EstimatedDelivery`.
  **[source-only]**
- Live confirmation — `POST /api/quotes` Oslo `0150` → Trondheim `7010`, 5 kg
  b2b, returned real Bring **prices** but zero delivery time for every Bring
  product: **[live-curl]**

  | carrier | service | price | transit_days | estimated_delivery |
  |---|---|---|---:|---|
  | bring | 3570 | 142.06 NOK | 0 | `0001-01-01T00:00:00Z` |
  | bring | 3584 | 142.06 NOK | 0 | `0001-01-01T00:00:00Z` |
  | bring | 9300 | 272.73 NOK | 0 | `0001-01-01T00:00:00Z` |
  | bring | 5000/9000/4850/9600/… | … | 0 | `0001-01-01T00:00:00Z` |

- The 2026-07-10 direct-Bring capture in the shipping-core README shows Bring
  DID return promises (e.g. product `3570`: 4 working days, ETA 2026-07-15) —
  so this is a parse bug (data present, dropped), not missing upstream data.

Consequence for the user goal: any surface reading `/api/quotes` (including
the Model Plane shipping tool) gets `transit_days:0` / year-0001 for Bring and
cannot state a real Bring transit time. It CAN, misleadingly, read a non-zero
time from the **mock** carriers in the same response (mock-dsv 4 days,
mock-postnord 3 days — fabricated), which is worse than none if surfaced as
fact. The README's own "Required tests and fix" list (top-level field +
production-shaped fixture + top-level-first mapping + incomplete-marking +
E2E year-1 assertion) remains unimplemented.

### Delivery-time parsing across the other real adapters

- DHL `dhl.go:160-186` parses `DeliveryCapabilities.TotalTransitDays` +
  `EstimatedDeliveryDateAndTime` — correct. (Live: DHL request to the `/test`
  sandbox timed out, so no data returned this pass.) **[source-only]+[live-curl]**
- UPS `ups.go:166-171` parses `GuaranteedDelivery.BusinessDaysInTransit` —
  code is correct, but the CIE sandbox returned no `guaranteedDelivery` for
  this lane, so live UPS also showed `transit_days:0` / year-0001. This is
  data-absent (honest 0), not a parse bug like Bring. **[source-only]+[live-curl]**
- FedEx `fedex.go:159-162` parses `commit.transitDays` via an enum word-map —
  correct; unknown values honestly stay 0. (Live: FedEx 403'd, see below.)

## Live route/behaviour matrix

| Method/Path | Live result | Notes |
|---|---|---|
| `GET /healthz` | 200 **[live-curl]** | |
| `GET /api/carriers` | 200, no auth **[live-curl]** | still `is_mock` only |
| `POST /api/quotes` | 200, no auth **[live-curl]** | Bring priced, 0 transit |
| `GET /api/carriers/reliability` | **404** **[live-curl]** | wired in source `main.go:99`, not in the 07-04 image |
| `POST /api/quotes/recommend` | **404** **[live-curl]** | wired in source `main.go:100`, not in the 07-04 image |

Auth: the router mounts only `platform.RequestLogger` (`main.go:94`); there is
no authentication, session, tenant-derivation, role, rate-limit, or ZDR
middleware. `/api/carriers` and `/api/quotes` return 200 with no credentials.
**[live-curl]+[source-only]** (a junk `Authorization` header changes nothing;
the 400 seen with a minimal body was request validation, not auth.) The
booking lifecycle (`booking.Routes`, `main.go:107`) shares the same
unauthenticated router — do not exercise write/booking routes against live
carrier credentials.

## Carrier provenance — still `is_mock` only, not the recommended fields

`GET /api/carriers` live: **[live-curl]**

```
mock-postnord/mock-dsv/mock-helthjem/mock-porterbuddy → is_mock:true
bring/dhl/ups/fedex                                   → is_mock:false
```

`quoteengine/http.go:105` still derives the flag as
`IsMock: strings.HasPrefix(info.Code, "mock-")`. The README-recommended
provenance replacement (`mode`/`environment`/`availability`/`verified_at`)
was **not** implemented, even in the working-tree source. **[source-only]**

Why `is_mock:false` is misleading — the running container's carrier bases are
sandbox/test, not production: **[inspect]**

| carrier | live base URL | reality |
|---|---|---|
| Bring | default `api.bring.com/shippingguide/api/v2` (`bring.go:23`) | production Shipping Guide (read-only rates) |
| DHL | `https://express.api.dhl.com/mydhlapi/test` | sandbox/test |
| FedEx | `https://apis-sandbox.fedex.com` | sandbox (403'd) |
| UPS | `https://wwwcie.ups.com` | CIE test |

Genuine mocks (canned quotes, clearly labelled): PostNord, DSV, Helthjem,
Porterbuddy — all in `internal/carrier/mock/mock.go` `DefaultCarriers()`
(`mock.go:210-255`). The mock adapter is intentional and honest (`Label`
watermarks "DEMO LABEL - NOT VALID FOR SHIPPING", `mock.go:103`).
Credential-backed (sandbox) real adapters: Bring (production), DHL, UPS,
FedEx.

FedEx live still fails authorization: `403 FORBIDDEN.ERROR "We could not
authorize your credentials."` **[live-curl]** — a portal-entitlement/account
issue (Rate API not enabled or account mismatch), not a code defect; the
adapter and config (`fedex/config.go`) are correct.

## The large uncommitted transformation (new subdirs)

All five new packages are real implementations, not stubs — but note the
deployed image has none of them, and the compose wiring gap below means even a
rebuild would leave the cross-plane ones inert.

- `internal/modelplane/client.go` — **outbound** client: shipping-core mints an
  RS256 service token from auth-core `/api/model-plane/internal-token` and
  calls model-gateway `/v1/invoke` with a structured-output schema, fail-closed
  when unconfigured (`Configured()`, `client.go:81`). This does **not** expose
  shipping-core as a Model Plane tool; it is shipping-core calling the Model
  Plane for the F5 recommendation. Uses a fixed service identity
  (`SystemOrgID="shipping-core-system"`, `client.go:32`) because no per-request
  org/user flows in yet. **[source-only]**
- `internal/recommend/{recommend,http.go}` — F5 route `POST /api/quotes/recommend`:
  fans out quotes, annotates reliability, asks the model to pick one, guards
  against a hallucinated carrier_code (`recommend.go:94-103`), and returns an
  honest `available:false` rather than fabricating a pick. Real. **[source-only]**
- `internal/reliability/store.go` — F8 on-time score from the `bookings` table
  (`estimated_delivery` vs `actual_delivered_at`), `MinSample=5`, 180-day
  window; absent (not fabricated) below threshold. Backed by migration
  `db/migrations/0003_reliability.up.sql` (adds `estimated_delivery`,
  `actual_delivered_at`, partial index). Real. Live quotes show
  `reliability_score:null` — expected, no delivered bookings yet. **[source-only]+[live-curl]**
- `internal/events/publisher.go` — NATS publisher for `booking.delivered` /
  `recommendation.generated` with a `NoopPublisher` fallback. Real. But
  `main.go:248-255` (`buildEventPublisher`) requires **all three** of
  `ALLOW_UNVERIFIED_LEGACY_EVENTS=1` + `ALLOW_INSECURE_DEV_DEFAULTS=1` +
  `ISOLATED_E2E=1` or it returns `NoopPublisher{}` even when `NATS_URL` is set —
  so shipping NATS events are effectively OFF in normal posture (they are
  unsigned). **[source-only]**
- `internal/dataplane/documents.go` — pushes factual delivery-outcome
  documents to Data Plane v2 `documents-api-go POST /v1/documents` with
  `X-Internal-Api-Key` + `X-Org-ID`, content-required + `Configured()`-gated.
  Real; fires from `deliveryHooks.OnDelivered` (`main.go:281-325`). **[source-only]**

New carrier `booking.go` files (dhl/fedex/ups, untracked) implement
Book/Label/Track with a hard sandbox-safe gate: `bookingAllowed()` refuses a
non-`/test` (DHL), non-CIE (UPS), non-sandbox (FedEx) host unless
`*_LIVE_BOOKING=true` (e.g. `dhl/booking.go:108-119`). Bring pickup is
honestly "deliberately NOT implemented … not stubbed" (`bring/booking.go:12-17`).
New booking tests exist for dhl/fedex/ups and pass. **[source-only]**

## Is shipping-core reachable as a Model Plane tool? (the verevon-chat path)

Yes in source, with caveats. The inbound tool lives in the **Model Plane**, not
here: `apps/Model Plane/rust/services/execution-core/src/shipping_tools.rs:116`
reads `SHIPPING_CORE_URL` (default `http://host.docker.internal:3156`) and calls
`/api/quotes`, `/api/carriers`, `/api/bookings`; `runtime_loop/mod.rs:538+`
dispatches quote/carrier/book tools. **[source-only]** So a chat turn that
activates the shipping tool reaches this service — and gets the same broken
Bring transit time. Two caveats: (1) per the shipping-core README, normal chat
does not auto-receive shipping tools (Browse/tools/Plan mode must activate
them); (2) that path depends on the execution-core image having the tool built
and reachable (Model Plane / Phase 4 to verify live). "Test the Visma MCP" is
out of shipping-core's scope — the only Visma surface here is a passthrough
`VismaOrderRef *string` audit field on `BookingRequest` (`carrier/adapter.go:136`);
there is no Visma call in shipping-core.

## Deploy drift and the compose wiring gap

- Running image built 2026-07-04 **[inspect]** → reliability + recommend
  routes 404 live though wired in current `main.go`. Rebuild is required (and
  currently Docker-blocked) before source == deployment. The Dockerfile still
  applies no OCI revision label.
- Config gap even after a rebuild: the `shipping-core` service block in
  `apps/Ingestion Plane/docker-compose.yml` passes only carrier creds
  (`BRING_*`, `DHL_*`, `UPS_*`, `FEDEX_*`); it does **not** map
  `AUTH_CORE_URL`, `MODEL_GATEWAY_URL`, `INTERNAL_API_KEY`, `NATS_URL`,
  `NATS_TOKEN`, `DATA_PLANE_DOCUMENTS_URL`, or `DATA_PLANE_INTERNAL_API_KEY`.
  `docker inspect` confirms the running container has none of them.
  **[inspect]** The host `.env` DOES define all of them, and
  `shipping-core/.env.example` now documents them — so the missing piece is the
  compose env mapping. Until added, `/api/quotes/recommend` would return
  `recommendation.available=false` (Model Plane unconfigured), delivery
  evidence to Data Plane is skipped, and NATS events are Noop — all degrade
  gracefully, none error.

## Stub / mock inventory

- Genuine mocks (intentional, labelled): PostNord, DSV, Helthjem, Porterbuddy
  via `mock.DefaultCarriers()` (`internal/carrier/mock/mock.go:210`).
- Honest documented gap: Bring pickup ordering (`bring/booking.go:12`).
- No hidden stubs/fakes/`TODO`/`FIXME`/"not implemented" elsewhere — a
  full-tree grep returned only `toDomain` false-positives plus the two items
  above. **[source-only]**

## Remaining gaps / priorities (shipping-core)

1. Fix the Bring delivery parse (top-level `expectedDelivery.workingDays` /
   `formattedExpectedDeliveryDate` first, alternatives as fallback) + add the
   production-shaped fixture and the year-1/`transit_days=0` E2E assertion.
   This is the user's headline blocker. **[source-only]**
2. Add auth/tenant/role/rate-limit/ZDR middleware to the router; the booking
   lifecycle currently shares an unauthenticated router. **[live-curl]+[source-only]**
3. Replace `is_mock` with real `mode`/`environment`/`verified_at` provenance in
   `GET /api/carriers`. **[source-only]**
4. Add the MP/DP/NATS env mappings to the compose `shipping-core` block, then
   rebuild a revision-labelled image and verify the reliability/recommend
   routes go live. **[inspect]**
5. FedEx sandbox: enable the Rate API on the developer-portal project (403 is
   entitlement, not code). **[live-curl]**
