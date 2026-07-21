# information-core

> **2026-07-13 superseding update.** Changed source no longer generates traffic volume or speed. Traffic measurements are nullable provenance-bearing observations with measured/estimated/synthetic/unavailable type, provider/source, timestamps, units, confidence/quality, freshness, and unavailable reason; Atlas station-only results are explicitly `metadata_only`. Derived road/county metadata is labeled estimated/low-quality or unavailable. Upstream HTTP/GraphQL errors are honest and not cached; coordinate/radius/search inputs are bounded. Go race tests pass; traffic coverage is 92.0% and `Latest` 86.7%. Velion v3 and the Model formatter preserve/label provenance. None of these changed runtimes is deployed, and legacy Velion v2 still expects numeric fields. The July 11 synthetic-fact text below is historical evidence of the defect, not current source behavior.

> **2026-07-21 source correction.** This historical audit predates the Norway-source expansion and the ownership repair. `information-core` no longer contains a Bring client or `/api/v1/shipping/track`; carrier tracking is owned by Ingestion Plane `shipping-core`, and Model Plane's `track_shipment` path now uses its tenant-scoped `/api/tracking/{trackingNo}` route. For the current route/source inventory, see [`information-core/docs/norway-data-sources-implementation.md`](../../information-core/docs/norway-data-sources-implementation.md).

_Audit refresh: 2026-07-11. Supersedes the 2026-06-07 / 2026-07-02 note. Evidence graded
`[live-curl]` (host curl to :3190), `[source-only]` (read from disk), `[inspect]` (docker
inspect/ps). Docker exec/build/logs are unavailable this pass (containerd content-store
corruption); every Application container reports `(unhealthy)` because the exec-based
healthcheck cannot run, not because the service is down._

## Verdict

`information-core` is **real, live, and honestly key-gated** — not a stub. It is a small Go
(Gin) read-only aggregator that wraps four public external APIs (Yr/met.no weather,
Statens Vegvesen traffic, Norwegian RSS news, Bring/Posten shipment tracking), caches the
responses in-memory, and exposes them behind a single `x-internal-api-key`-gated surface.
It carries **no plane database** and performs **no writes** — consistent with the "no
direct cross-plane DB crossing" rule. One genuine data-integrity caveat: the `trafficVolume`
and `averageSpeed` fields it returns are **synthetically fabricated** (see Findings).

## Current State

At startup (`cmd/server/main.go`) it: loads config, creates an in-memory TTL cache, builds
one shared `http.Client` (20 s timeout), wires the news / shipping / traffic / weather
services, and starts a Gin server on `:3190`. Graceful shutdown on SIGINT/SIGTERM. No DB,
NATS, Redis/Dragonfly, or other plane dependency is touched in the boot path. `[source-only]`

- Runtime state: `running`, started 2026-07-09, health `unhealthy` — the unhealthy flag is
  the known containerd/exec-healthcheck artifact, contradicted by the live 200s below. `[inspect]`

## Entry Points

- Main: `apps/Application Plane/information-core/cmd/server/main.go`
- Routes: `apps/Application Plane/information-core/internal/http/server.go`
- Handlers: `apps/Application Plane/information-core/internal/http/handlers.go`
- Services: `internal/{weather,traffic,news,shipping}/service.go`, `internal/cache/cache.go`

## Exposed Surface (verified against route source)

| Route | Auth | Live result |
|---|---|---|
| `GET /health` | none | `200 {"service":"information-core","status":"ok"}` `[live-curl]` |
| `GET /ready` | none | `200` (aliased to same handler) `[live-curl]` |
| `GET /api/v1/weather?lat&lon&altitude` | key | `200`, real met.no forecast `[live-curl]` |
| `GET /api/v1/weather/oslo` | key | `200`, current Oslo forecast (temp 27°C, dated 2026-07-12) `[live-curl]` |
| `GET /api/v1/traffic?lat&lon&radius&search` | key | `200`, real SVV stations near coord (Vestby syd, Dr. Eufemias Gt.) `[live-curl]` |
| `GET /api/v1/news?limit&offset&maxAge&category` | key | `200`, live Norwegian RSS (VG headlines dated 2026-07-12) `[live-curl]` |
| `GET /api/v1/shipping/track?trackingNumber=` | key | `400` on missing param; `502` on unresolvable/fake tracking no. `[live-curl]` |

Auth gate is genuine and correct `[live-curl] + [source-only]`:
- No header → `401 {"error":{"code":"unauthorized","message":"authentication required"}}`.
- Wrong key → `401 {"...":"invalid API key"}`.
- Correct key → `200`.
- `requireInternalKey` (server.go) uses `crypto/subtle.ConstantTimeCompare` and fails closed
  when `INTERNAL_API_KEY` is empty. Compose declares `INTERNAL_API_KEY:?...must be set` (no
  `change-me` default) and `.env` carries the real 64-hex shared key. No hardcoded-secret
  fallback exists here (unlike the Convex `change-me-internal-service-secret` default).

## Relationships

- **Model Plane agent tool target — confirmed REAL and wired** `[source-only]`.
  `apps/Model Plane/rust/services/execution-core/src/info_tools.rs` builds an
  `InfoToolsClient` from `INFORMATION_CORE_URL` (→ `INFORMATION_CORE_ADDR` → compose default
  `http://information-core:3190`) and sends `INFORMATION_CORE_INTERNAL_KEY` as the
  `x-internal-api-key` header. `runtime_loop/mod.rs` exposes the agent tools `yr_weather`,
  `traffic`, `news`, and `track_shipment`, each a real HTTP call to the routes above
  (all read-only). The same client also does public Brønnøysund company-registry lookups.
  `deploy/docker-compose.yml` (execution-core) sets `INFORMATION_CORE_URL_EXEC` default
  `http://host.docker.internal:3190` and `INFORMATION_CORE_INTERNAL_KEY` → `INTERNAL_API_KEY`.
  This puts information-core on the chat tool path.
- Historical caller `velionv2` (`src/app/api/v1/information/_lib/upstream.ts`) — deprecated
  plane per 2026-07-10; treat v2 as inactive. Current product traffic should be verified via
  the v3 gateway, not asserted from here.
- No inbound dependency on any Application-Plane peer; it is a leaf utility service.

## Findings

1. **Synthetic traffic volume/speed presented as real data** — severity **medium**
   (data-integrity/honesty). `internal/traffic/service.go` `buildGraphQLQuery` requests only
   `id`, `name`, and `location.coordinates` from the SVV Atlas GraphQL API. The
   `trafficVolume` and `averageSpeed` fields in the response are **fabricated deterministically**
   by `stableTrafficMetrics(id)` = an FNV-32a hash of the station ID (`550 + sum%950`,
   `55 + (sum/7)%35`). Station identity, name, and coordinates are genuine SVV data and
   distance is correctly computed (haversine); only the two metric numbers are invented.
   The README claims the module "Returns volume and speed metrics for stations," and the
   Model Plane `format_traffic` reads exactly `trafficVolume`/`averageSpeed` back — so the AI
   agent relays synthetic numbers to end users as if measured. `[source-only] + [live-curl]`
   (e.g. Vestby syd `volume 717, speed 83`; Dr. Eufemias Gt. `volume 988, speed 79` — both
   hash-derived, not from Atlas). Not a crash risk; a truthfulness risk on the chat path.

2. **`shipping/track` returns 502 for unresolved tracking numbers** — this is an **honest
   guard, not a bug**. Bring's public endpoint returns an error/empty consignment for an
   invalid number; the service maps that to `502 shipping_unavailable`. Real Bring numbers
   work; `BRING_API_UID`/`BRING_API_KEY` are optional (higher rate limits only) and are unset
   locally, which is expected. `[live-curl] + [source-only]`

3. No `TODO`/`FIXME`/`mock`/`stub`/`fake`/`placeholder`/`change-me`/`dummy` markers in
   non-test source. `NewServiceWithURL` in shipping is a legitimate test seam (points at
   `httptest`), not a runtime stub. `[source-only]`

## Stub / Mock / Placeholder Audit

- No runtime placeholder path. All four modules make real outbound calls to real upstreams
  and were live-verified returning current data. `[live-curl]`
- The only "fabricated" values are the traffic metrics in Finding 1 (deterministic synthesis,
  not a mock object). `go.uber.org/mock` is transitive dependency noise (Gin/validator), not
  wired into any code path. `[source-only]`

## Build / Test / Toolchain

- `go build ./...` → clean (exit 0). `go vet ./...` → clean. `[source-only]`
- `go test ./...` → all module tests pass: `news`, `shipping`, `traffic`, `weather` `ok`;
  `cmd/server`, `internal/{cache,config,http}` have no test files. Toolchain: Go 1.26.2
  darwin/arm64 (module pins `go 1.25`; Dockerfile builds on `golang:1.25-bookworm`). `[source-only]`
- Coverage gap: HTTP layer (auth middleware, handlers) and cache have no unit tests; the four
  service packages are the only tested ones. The auth gate is covered by the live probes above.

## Uncommitted WIP

None for this service. `git status --porcelain` and `git diff --stat` are empty for
`apps/Application Plane/information-core`. Last commit touching the dir: `61c492db`
(2026-06-21, "fix(compose): fail-loud on missing secrets instead of change-me defaults").
The large repo-wide working-tree changes noted elsewhere do **not** touch information-core. `[source-only]`

## Notes

Honest, modest, live internal-support API — not a collaborative core. Its role is a
read-only public-data aggregator for the workspace and the Model Plane agent. The one item
worth a follow-up ticket is Finding 1 (either fetch real Atlas volume/speed via the traffic
dataset endpoint, or label the synthesised metrics as estimates in the payload and to the
agent).
