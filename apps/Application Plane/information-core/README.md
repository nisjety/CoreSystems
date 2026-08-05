# information-core

Real-time contextual data aggregator for the Application Plane. Wraps bounded read-only external APIs, caches their responses, and exposes a single internal HTTP surface for Verevon's workspace and AI model.

All routes are guarded by `x-internal-api-key` and are **not** reachable from the public internet.

## Design rule

This service is the right home for integrations that are:
- **Public or service-credential read access** — no end-user delegation
- **Read-only** — no writes, no webhooks
- **Ambient or entity-specific lookups** — addresses, weather, traffic, and news

It is **not** the right home for authenticated ERP integrations (Visma, Tripletex). Those require OAuth flows, per-tenant credentials, and write operations — they belong in a dedicated `erp-core` service.

## Modules

### Weather — `Yr / met.no`

Fetches location forecasts from the Norwegian Meteorological Institute's free API. Returns current conditions and a 5-day daily forecast.

- Source: `https://api.met.no/weatherapi/locationforecast/2.0/compact`
- Cache TTL: 600 s (10 min)
- Use case: Surface weather context on task cards, flag heat/cold alerts for Aquatiq's food-safety workflows.

### Traffic — `Statens Vegvesen Atlas`

Queries traffic registration points via the SVV GraphQL API. Returns volume and speed metrics for stations near a given coordinate.

- Source: `https://trafikkdata-api.atlas.vegvesen.no`
- Cache TTL: 300 s (5 min)
- Use case: Station metadata for logistics context; measured traffic values are not fabricated.

### News — multiple RSS sources

Aggregates Norwegian and industry-specific RSS feeds into a normalised article list. Supports category filtering and age-based cutoffs.

- Cache TTL: 1800 s (30 min)
- Use case: Surface relevant news in the Verevon home feed; the AI can reference recent developments when answering questions.

### Address — `Kartverket Address REST`

Searches the open national address register through Kartverket's Address REST API. This module returns address and coordinate data only; it does not expose ownership, title, rights, valuation, or Grunnbok facts.

- Source: `https://ws.geonorge.no/adresser/v1/sok`
- API version: 1.2.0
- Cache TTL: 300 s (5 min)
- Query bound: 1–50 results per page, 200-character search string
- Auth: No provider registration required; the internal route still requires `x-internal-api-key`.

### Norway source lookups

The service also exposes bounded, read-only source adapters for official statistics, transit planning, parliamentary data, current-law search, exchange-rate series, open property location, measured air quality, and NVDB road objects. Responses retain the upstream payload under `data` and include the canonical `source` envelope.

| Endpoint | Method | Required input | Source |
|---|---|---|---|
| `/statistics/query` | POST | bounded table/variable selection JSON | SSB PxWebApi v2 |
| `/statistics/metadata` | GET | `table` | SSB PxWebApi v2 |
| `/journey/plan` | POST | bounded `from`/`to` places or coordinates | Entur Journey Planner v3 |
| `/parliament/representatives` | GET | — | Storting open data |
| `/legal/search` | GET | `q`; requires `LOVDATA_API_KEY` | Lovdata API |
| `/exchange` | GET | `series`, optional date window or `lastN` | Norges Bank SDMX |
| `/property/lookup` | GET | `matrikkelnummer` | Kartverket open property location |
| `/air-quality` | GET | `lat`, `lon`, optional `radius` | Miljødirektoratet |
| `/roads` | GET | `objectType`, `municipality`, optional `limit` | NVDB API Les v4 |

## Endpoints

All endpoints are under `/api/v1/` and require the header `x-internal-api-key`.

| Endpoint | Params | Description |
|---|---|---|
| `/address` | `q`, `limit`, `page`, `fuzzy` | Bounded Kartverket address lookup |
| `/weather` | `lat`, `lon`, `altitude` | Forecast for any coordinate |
| `/weather/oslo` | — | Shortcut for Oslo (59.9139, 10.7522) |
| `/traffic` | `lat`, `lon`, `radius`, `search` | Traffic stations near a point |
| `/news` | `limit`, `offset`, `maxAge`, `category` | Latest news articles |
| `/statistics/query` | POST body with bounded table/selection | SSB JSON-stat2 query |
| `/statistics/metadata` | `table` | SSB table metadata |
| `/journey/plan` | POST body with bounded places | Entur journey plan |
| `/parliament/representatives` | — | Current Storting representatives |
| `/legal/search` | `q`, `limit`, `offset`, `base` | Lovdata current-law search; runtime key required |
| `/exchange` | `series`, optional date window or `lastN` | Norges Bank SDMX series |
| `/property/lookup` | `matrikkelnummer` | Kartverket open property location |
| `/air-quality` | `lat`, `lon`, optional `radius` | Miljødirektoratet measured observations |
| `/air-quality/aggregate` | `meanType`, `from`, `to`, `lat`, `lon`, `radius` | Time-bounded Miljødirektoratet aggregate |
| `/roads` | `objectType`, `municipality`, optional `limit` | NVDB API Les v4 road objects |
| `/datex/situation` | Registered DATEX II credentials required | Road-operational DATEX II XML snapshot |
| `/frost/observations` | `sources`, `elements`, `referencetime`, optional `limit`; client ID required | MET Norway Frost observations |
| `/nve/avalanche-warnings` | `lat`, `lon`, `startDate`, `endDate`, optional `language` | NVE avalanche warning; not a safety conclusion |
| `/heritage/features` | `minLon`, `minLat`, `maxLon`, `maxLat`, optional `limit` | Riksantikvaren kulturmiljoer OGC features |
| `/health` | — | Liveness probe |
| `/ready` | — | Readiness probe |

### Example: address lookup

```
GET /api/v1/address?q=Storgata%202%20Oslo&limit=10
x-internal-api-key: <key>
```

```json
{
  "source": {
    "provider": "kartverket",
    "dataset": "address-rest-v1.2.0",
    "source_url": "https://ws.geonorge.no/adresser/v1/sok",
    "retrieved_at": "2026-07-21T00:00:00Z",
    "quality": "authoritative_provider",
    "coverage": "national_address_register",
    "status": "measured"
  },
  "data": [{
    "addressText": "Storgata 2",
    "municipalityCode": "0301",
    "postalCode": "0155",
    "representationPoint": {"epsg": "4258", "lat": 59.9139, "lon": 10.7522}
  }]
}
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3190` | HTTP listen port |
| `SERVICE_NAME` | `information-core` | Reported in health checks |
| `INTERNAL_API_KEY` | *(required)* | Shared secret for internal callers |
| `INFORMATION_CORE_USER_AGENT` | `VerevonInformationCore/1.0 ...` | Sent to upstream APIs that require it (Yr, Nominatim) |
| `ENTUR_CLIENT_NAME` | `coresystem-information-core` | Required provider identification header for Entur |
| `LOVDATA_API_KEY` | *(optional; required for `/legal/search`)* | Lovdata API key, injected at runtime and never logged |
| `DATEX_URL` | *(optional; required for `/datex/situation`)* | Registered DATEX II 3.1 pull endpoint |
| `DATEX_USERNAME` | *(optional; required for `/datex/situation`)* | Registered DATEX II username |
| `DATEX_PASSWORD` | *(optional; required for `/datex/situation`)* | Registered DATEX II password, never logged |
| `FROST_CLIENT_ID` | *(optional; required for `/frost/observations`)* | MET Norway Frost client ID |
| `FROST_URL` | `https://frost.met.no/observations/v0.jsonld` | Frost observations endpoint |

Bring and other carrier adapters belong to `shipping-core` in the Ingestion Plane. This service no longer calls carrier providers or exposes a duplicate tracking route.

## Running locally

Copy `.env.example` to `.env`, keep `.env` owner-readable only, and inject
provider credentials only after the relevant registration is approved. The
current local runtime has the internal API key configured; Lovdata, DATEX II,
and Frost remain fail-closed until their optional credentials are provisioned.

```bash
cd "apps/Application Plane/information-core"
INTERNAL_API_KEY=dev go run ./cmd/server
```

Or via the Application Plane compose stack:

```bash
cd "apps/Application Plane"
docker compose up information-core
```

## Tests

```bash
go test ./...
go test -race ./...
```
