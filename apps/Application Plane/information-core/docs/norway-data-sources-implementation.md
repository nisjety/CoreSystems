# Norway data-source implementation status

**Updated:** 2026-07-21  
**Basis:** `norway-data-sources-roadmap-audit-2026-07-20.md` and `norway-data-sources-roadmap.proposed.md`

This records implementation, runtime evidence, and provider-access gaps. It does not change the proposed roadmap's delivery-state vocabulary.

## Gate 0 changes implemented in source

- `information-core` no longer owns a Bring client, Bring credentials, or a duplicate `/api/v1/shipping/track` route.
- Carrier adapters and shipment tracking remain owned by Ingestion Plane `shipping-core`, which already has authenticated carrier adapters and tenant-scoped booking tracking.
- `information-core /health` remains liveness-only; `/ready` now returns `503` when its required internal API key is missing.
- Weather, traffic, and news responses now carry the shared `source` envelope. Traffic remains explicitly metadata-only/partial; it does not fabricate measurements.

These changes are deployed in the rebuilt running artifact. Public route smoke
evidence is recorded below; consumer compatibility, tenant membership, ZDR,
and production rate-limit evidence remain open promotion gates.

## Bounded lookup adapters implemented

Kartverket Address REST is implemented in `internal/address` and exposed as:

```text
GET /api/v1/address?q=<bounded query>&limit=<1..50>&page=<0..>&fuzzy=true|false
```

The adapter uses the documented `GET https://ws.geonorge.no/adresser/v1/sok` contract, pins API version `1.2.0`, requests `EPSG:4258`, caps page size at 50, caps query length at 200 Unicode characters, caches successful lookups for five minutes, and returns the canonical source envelope.

The returned cadastral fields are only the open address response's reference fields. The adapter does not expose ownership, title, rights, valuation, or Grunnbok data.

The following bounded adapters are also implemented in `information-core`:

- SSB PxWebApi v2: `POST /api/v1/statistics/query` and `GET /api/v1/statistics/metadata`. JSON-stat2 is preserved, query size is bounded, and a reproducible query hash is returned.
- Entur Journey Planner v3: `POST /api/v1/journey/plan`. The adapter requires a configured `ET-Client-Name` value and accepts only bounded place or coordinate inputs.
- Storting current representatives: `GET /api/v1/parliament/representatives`. Email fields are intentionally omitted from the application response.
- Lovdata current-law search: `GET /api/v1/legal/search`. The route fails closed until `LOVDATA_API_KEY` is configured; the key is never included in the source envelope or error response.
- Norges Bank SDMX: `GET /api/v1/exchange`. Series identifiers and date windows are bounded and the query hash is returned.
- Kartverket open property location: `GET /api/v1/property/lookup`. This is location/boundary context only and is marked `partial` because the source is not a title or rights system.
- Miljødirektoratet measured air quality: `GET /api/v1/air-quality`. The route exposes measured observations only and bounds the coordinate radius.
- NVDB API Les v4 road objects: `GET /api/v1/roads`. Object type, municipality and result count are bounded; bulk/history remains outside this service.
- NVE avalanche warnings: `GET /api/v1/nve/avalanche-warnings`. The warning window and coordinates are bounded; this is not a property-safety or landslide conclusion.
- Riksantikvaren OGC Features: `GET /api/v1/heritage/features`. The collection is fixed to `kulturmiljoer` and queries are bounded by `bbox` and `limit`.
- Miljødirektoratet aggregate observations: `GET /api/v1/air-quality/aggregate`. Time window and radius are bounded to the provider's public aggregate contract.

The following bounded routes are now `deployed_verified` for the smoke requests
used on 2026-07-21: Address REST, SSB metadata, Entur Journey Planner,
Storting representatives, Norges Bank SDMX, open property location, measured
air quality and its aggregate, NVDB V4, NVE avalanche warnings, Riksantikvaren
OGC Features, traffic, weather, and news. This means the rebuilt Docker
artifact reached the documented upstream contract and returned a successful
response; consumer rollout, production rate limits, tenant membership, ZDR,
and full feature permutations still require separate acceptance evidence.

Lovdata, DATEX II, and Frost remain `source_only`: their route contracts and
fail-closed behavior are verified, but the running environment has no
`LOVDATA_API_KEY`, `DATEX_USERNAME`/`DATEX_PASSWORD`, or `FROST_CLIENT_ID`.

The first Phase 3 Ingestion handoff is now implemented in
`apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/catalogwatch`.
It queries the public Data.norge SPARQL endpoint and returns a deterministic
catalog snapshot plus an added/removed/changed diff. It is intentionally not
scheduled or persisted yet; review, retention, deletion handling and the
Data Plane write contract must be approved before enabling a recurring job.

The next Phase 3 source-only handoff is
`apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/officialseries`.
It collects bounded SSB JSON-stat2 and Norges Bank SDMX snapshots with a
provider/query hash and does not write directly to Data Plane storage. The
remaining step is an approved Data Plane source-object/document contract for
versioning, deletion semantics, idempotency and ZDR propagation.

The following Phase 3 handoff is
`apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/revisions`.
It captures bounded Lovdata search revisions and allowlisted Storting XML/JSON
exports, preserving provider versions where available and content hashes as a
fallback. It remains source-only and does not interpret law, crawl websites,
or write directly to Data Plane storage.

The next Phase 3 handoff is
`apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/navacancies`.
It consumes NAV's authenticated change feed with conditional polling and
explicit inactive deletion tombstones. The normalized model excludes full
vacancy details and contact lists; live activation still requires a consumer
token plus approved privacy, retention and Data Plane deletion handling.

The procurement Phase 3 handoff is
`apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/procurement`.
It provides bounded TED Search API requests and versioned Doffin CSV parsing;
TED and Doffin remain separate coverage paths, and neither is scheduled or
written directly to Data Plane storage yet.

The remaining safe registered-source adapters are now implemented in
`information-core`: `/api/v1/datex/situation` preserves authenticated DATEX II
3.1 XML, while `/api/v1/frost/observations` provides bounded authenticated
Frost observations. Both fail closed without provider registration/credentials
and remain `source_only` pending live evidence.

Tolletaten remains intentionally out of `information-core`. Its dated tariff snapshots belong in Ingestion/Data, while shipping-specific landed-cost or quote behavior belongs in `shipping-core`; adding a second tariff or customs adapter here would recreate the ownership duplication this repair is removing.

## Verification performed

```text
go test ./...
go vet ./...
go build ./...
docker compose --env-file .env up -d --build --force-recreate information-core
GET /health -> 200
GET /ready -> 200
public-provider smoke matrix -> 200 for configured anonymous/identified APIs
credential-gated matrix -> 503 source_not_configured for Lovdata, DATEX II, Frost
```

The package suite passes, including adapter, cache, upstream-failure,
readiness, authentication, duplicate-route, SDMX path, and NVDB client-header
tests. The live Docker smoke matrix passed after fixing the Norges Bank path
separator and adding the required NVDB `X-Client` header. Runtime consumer
compatibility, tenant membership, ZDR, and rate-limit evidence remain required
for production rollout.

## Credential acquisition status

Credentials cannot be generated from this repository. They require an
authorized organization or user registration:

| Provider | Environment | Current state | Next action |
|---|---|---|---|
| Lovdata | `LOVDATA_API_KEY` | Missing; endpoint returns `503 source_not_configured` | Request/confirm access through [Lovdata API](https://lovdata.no/info/api), then inject the key through the deployment secret manager. |
| Statens vegvesen DATEX II | `DATEX_USERNAME`, `DATEX_PASSWORD` | Missing; endpoint returns `503 source_not_configured` | Submit the [NPRA DATEX access request](https://www.vegvesen.no/en/fag/technology/open-data/a-selection-of-open-data/what-is-datex/get-access/?lang=en); inject the issued credentials without committing them. |
| MET Norway Frost | `FROST_CLIENT_ID` | Missing; endpoint returns `503 source_not_configured` | Create an authorized client/user per [Frost authentication](https://frost.met.no/authentication.html); inject the client ID. |

No provider registration form was submitted because that requires organization
identity, purpose, contact details, and acceptance of terms.

## Next aligned slices

1. Provision Lovdata, DATEX II, and Frost access, then run live contract checks and promote those three routes from `source_only`.
2. Verify consumer compatibility, tenant membership, ZDR propagation, rate limits, telemetry, and feature flags before production promotion.
3. Complete the remaining NVE GIS/HydAPI products and any additional Miljødirektoratet products in their dedicated operational/geospatial owners after contract confirmation.
4. Keep feeds, bulk downloads, immutable versions, and retrieval corpora in Ingestion/Data Plane rather than extending `information-core` into a corpus service. The source-only handoffs now cover the Data.norge catalog diff, curated SSB/Norges Bank series, bounded Lovdata/Storting revisions, NAV deletion-aware changes, and TED/Doffin procurement paths.
5. Keep Maskinporten/Altinn, Matrikkel/Grunnbok, Folkeregisteret and closed AIS blocked until purpose, legal basis, delegation and retention controls are approved.
