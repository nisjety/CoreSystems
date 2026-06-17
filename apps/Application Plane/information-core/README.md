# information-core

Real-time contextual data aggregator for the Application Plane. Wraps public, unauthenticated external APIs, caches their responses, and exposes a single internal HTTP surface for Velion's workspace and AI model.

All routes are guarded by `x-internal-api-key` and are **not** reachable from the public internet.

## Design rule

This service is the right home for integrations that are:
- **Public / no OAuth** — key-free or at most an optional API key
- **Read-only** — no writes, no webhooks
- **Ambient or entity-specific lookups** — weather, traffic, news, shipment tracking

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
- Use case: Logistics planning, road condition awareness for service technicians on site visits.

### News — multiple RSS sources

Aggregates Norwegian and industry-specific RSS feeds into a normalised article list. Supports category filtering and age-based cutoffs.

- Cache TTL: 1800 s (30 min)
- Use case: Surface relevant news in the Velion home feed; the AI can reference recent developments when answering questions.

### Shipping — `Bring / Posten`

Tracks shipments by tracking number against the Bring Tracking API. Returns current status, estimated delivery date, and full event history.

- Source: `https://tracking.bring.com/tracking.json`
- Cache TTL: 600 s (10 min)
- Use case: Display live parcel status on ticket cards. When a service engineer asks "where is the equipment?", Velion answers with real carrier data.
- Auth: Works without credentials (public endpoint, rate-limited). Set `BRING_API_UID` + `BRING_API_KEY` for higher limits via a [Mybring account](https://developer.bring.com/api/tracking/).

## Endpoints

All endpoints are under `GET /api/v1/` and require the header `x-internal-api-key`.

| Endpoint | Params | Description |
|---|---|---|
| `/weather` | `lat`, `lon`, `altitude` | Forecast for any coordinate |
| `/weather/oslo` | — | Shortcut for Oslo (59.9139, 10.7522) |
| `/traffic` | `lat`, `lon`, `radius`, `search` | Traffic stations near a point |
| `/news` | `limit`, `offset`, `maxAge`, `category` | Latest news articles |
| `/shipping/track` | `trackingNumber` | Shipment status from Bring/Posten |
| `/health` | — | Liveness probe |
| `/ready` | — | Readiness probe |

### Example: track a shipment

```
GET /api/v1/shipping/track?trackingNumber=370000000000000000
x-internal-api-key: <key>
```

```json
{
  "trackingNumber": "370000000000000000",
  "status": "In transit",
  "statusCode": "IN_TRANSIT",
  "description": "Package is on its way",
  "carrier": "Bring/Posten",
  "estimatedDelivery": "2026-06-15",
  "lastUpdate": "2026-06-13T10:00:00",
  "events": [
    {
      "timestamp": "2026-06-13T10:00:00",
      "description": "Loaded on vehicle",
      "location": "Oslo, NO"
    }
  ]
}
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3190` | HTTP listen port |
| `SERVICE_NAME` | `information-core` | Reported in health checks |
| `INTERNAL_API_KEY` | *(required)* | Shared secret for internal callers |
| `INFORMATION_CORE_USER_AGENT` | `VelionInformationCore/1.0 ...` | Sent to upstream APIs that require it (Yr, Nominatim) |
| `BRING_API_UID` | — | Mybring account email (optional, enables higher rate limits) |
| `BRING_API_KEY` | — | Mybring API key (required if `BRING_API_UID` is set) |

## Running locally

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
