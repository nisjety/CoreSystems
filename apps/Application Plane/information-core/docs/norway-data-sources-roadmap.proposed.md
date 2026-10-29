# Norway authoritative-data integration roadmap — proposed revision

**Status:** proposal with runtime evidence  
**Verified:** 2026-07-21  
**Scope:** Norwegian and adjacent European authoritative sources used by Application Plane features.

## 1. Purpose

Add authoritative data without weakening tenant isolation, privacy, provenance, or the existing plane boundaries. Institutions are not assigned wholesale to one service: each concrete API or dataset is placed according to how it is acquired and used.

This roadmap does not authorize a new plane or product. A new bounded core requires an ADR showing durable domain state, workflow ownership, or independent scaling/compliance needs.

## 2. Delivery-state vocabulary

Every integration must use exactly one state:

- `deployed_verified` — running artifact tested against its documented contract
- `deployed_known_defect` — running, but with a recorded correctness or security defect
- `source_only` — implemented in source but not verified against a successful provider response in the running artifact, or awaiting required provider access
- `discovery` — provider contract is being verified
- `proposed` — accepted candidate, not implemented
- `blocked` — cannot proceed until a named access, legal, security, or product decision is resolved

The live evidence snapshot is maintained in
`norway-data-sources-completion-2026-07-21.md`. `deployed_verified` means the
running Docker artifact returned a successful documented response for the
bounded smoke request; it does not imply consumer rollout, production rate
limits, or approval for restricted data.

Do not use “live,” “done,” or “available” without runtime evidence.

## 3. Governing architecture

| Concern | Default owner |
|---|---|
| Bounded, app-facing lookup or current projection | Application Plane |
| Connector acquisition, polling, feeds, bulk downloads, change capture | Ingestion Plane |
| Durable documents, immutable versions, embeddings and retrieval corpora | Data Plane |
| Credentials, Maskinporten tokens, delegation and authorization | Control/Auth or shared Integration capability |
| Tool selection, reasoning and synthesis | Model Plane |
| Carrier adapters, shipment booking and shipment tracking | `shipping-core` |

A source may support capabilities in several planes. For example, SSB may have a bounded live query in the Application Plane and a curated series ingested into the Data Plane.

## 4. Admission rule for `information-core`

An adapter belongs in `information-core` only when all are true:

1. It provides a bounded, read-only live lookup rather than bulk acquisition or durable corpus management.
2. Its data is public or safely tenant-scoped.
3. The intended display, transformation, caching and redistribution comply with the provider's terms.
4. Authentication is anonymous, client identification, or a service credential—not end-user delegation or purpose-bound access.
5. Queries and responses can be bounded for abuse, cost and privacy.
6. The adapter exposes provenance, freshness, coverage and honest unavailability.
7. No existing domain core already owns the capability.

“Public” does not mean anonymous, unlimited, non-personal, redistributable, or supported by an SLA. OAuth by itself does not determine placement.

## 5. Mandatory provider record

Before implementation, record:

- exact product, endpoint family and version;
- official documentation and deprecation channel;
- authority, licence and required attribution;
- authentication class: `anonymous`, `identified`, `registered_key`, `oauth_service`, `delegated`, or `agreement`;
- data classification and possible personal/sensitive fields;
- permitted caching, retention, transformation and redistribution;
- coverage, freshness, limits, pagination and maximum query shape;
- feature owner, plane owner and consuming services;
- failure semantics, retry policy and upstream health behavior;
- verification date, reviewer and acceptance evidence.

## 6. Current baseline and required repair

| Capability | Current assessment | Required action |
|---|---|---|
| Weather | `deployed_verified` for the bounded live smoke request | Complete consumer compatibility and production telemetry evidence. |
| Traffic | `deployed_verified` for the bounded live smoke request; metadata-only measurements remain explicit | Test failure honesty with Model/UI consumers and do not fabricate measured values. |
| News | `deployed_verified` for the bounded live RSS smoke request; feed terms still need inventory | Record each feed, terms, freshness and cache policy. |
| Bring tracking | Authentication assumption is stale and ownership duplicates `shipping-core` | Require Mybring credentials, treat tracking as customer-linked data, and move the provider adapter to `shipping-core`. |

Bring states that unauthenticated Tracking API access ended on 13 May 2024 and that API connections require a Mybring user and API key. ([Tracking API](https://developer.bring.com/api/tracking/), [authentication](https://developer.bring.com/api/))

### Gate 0 — repair current truth before expansion

1. Deploy and verify the existing traffic provenance fix.
2. Resolve Bring tracking ownership and remove the duplicate provider call.
3. Make missing required credentials a non-ready state.
4. Verify membership, tenant boundaries, ZDR behavior, log redaction and per-source rate limits on every exposed route.
5. Add provider contract tests, negative tests and upstream-failure tests.

Discovery and isolated prototypes may proceed in parallel, but no new integration is production-ready until Gate 0 passes.

## 7. Candidate registry

| Source/product | Access and factual correction | Best first feature | Default placement | Priority/state |
|---|---|---|---|---|
| Kartverket Address REST | Anonymous; do not conflate with Matrikkel or Grunnbok access; maximum response size is 10,000, so use downloads for bulk. ([guide](https://www.kartverket.no/api-og-data/eiendomsdata/brukarrettleiing-adresse-api)) | Address normalization and geocoding | Application lookup | P1 / deployed_verified |
| Kartverket open property-location service | Anonymous; typically one day behind and boundary geometry may be incomplete/imprecise. ([API documentation](https://ws.geonorge.no/eiendom/v1/)) | Cadastral-reference and registered-boundary lookup | Application geospatial lookup | P1 / deployed_verified |
| Kartverket Matrikkel/Grunnbok APIs | Application/agreement and disclosure controls required. ([access classes](https://www.kartverket.no/api-og-data/eiendomsdata/data-som-krever-soknad), [application](https://www.kartverket.no/api-og-data/eiendomsdata/soknad-api-tilgang)) | Permissioned property facts | Control/Auth plus property-domain capability | Restricted / blocked pending purpose |
| SSB PxWebApi v2 + Klass | Anonymous; JSON-stat2; 800,000-cell response ceiling; 30 queries/min/IP; CC BY 4.0. ([v2 guide](https://www.ssb.no/en/api/pxwebapiv2)) | Reproducible official statistics | Application lookup; curated series in Ingestion/Data | P1 / deployed_verified |
| Entur Journey Planner v3 | Open/NLOD; `ET-Client-Name` required; privileged APIs use OAuth2; realtime completeness varies. ([Journey Planner](https://developer.entur.org/pages-journeyplanner-journeyplanner/), [authentication](https://developer.entur.org/pages-intro-authentication/)) | Norway-wide journey lookup | Application lookup | P1 / deployed_verified |
| Storting open data | Anonymous, NLOD, 100 calls/min; XML/HTML as well as JSON. ([overview](https://data.stortinget.no/om-datatjenesten/), [terms](https://data.stortinget.no/om-datatjenesten/bruksvilkar/)) | Bills, votes and representatives | Application lookup; versions in Data | P1 / deployed_verified |
| Lovdata open API | Current laws and central regulations are free under NLOD 2.0 since November 2025. ([API](https://lovdata.no/info/api), [announcement](https://lovdata.no/artikkel/lovdata_tilrettelegger_for_bruk_tilpasset_var_ki-hverdag_gjor_api_av_oppdatert_regelverk_gratis_tilgjengelig_for_alle/5277)) | Current-law reference with official citations | Application lookup; versioned corpus in Ingestion/Data | P1 / source_only — provider key missing |
| Norges Bank SDMX | Free, anonymous open-data API. ([service record](https://data.norge.no/en/datasets/23076ce8-b442-407c-817f-0d4ec3cbe744/api-for-apne-data)) | Exchange-rate and economic series | Application lookup; curated series in Data | P1 / deployed_verified |
| NVE warning APIs | Separate warning products with attribution/presentation requirements; not one generic hazard API. ([open APIs](https://www.nve.no/om-nve/aapne-data-og-api-fra-nve/), [flood warnings](https://api.nve.no/doc/flomvarsling/)) | Regional warning context | Application lookup | P2 / deployed_verified |
| NVE GIS/HydAPI products | Product-specific coverage and changing GIS endpoints. ([GIS notices](https://www.nve.no/kart/nytt-om-gis-api/)) | Hazard layers and observations | Geospatial adapter; bulk/history in Ingestion/Data | P2 / discovery |
| NVDB API Les v4 | Public/NLOD; use v4, resolve object/property metadata, and send `X-Client`. ([migration](https://nvdb-docs.atlas.vegvesen.no/nvdbapil/Migrering/)) | Road geometry and attributes | Application geospatial lookup | P2 / deployed_verified |
| Statens vegvesen DATEX II | Registration required although use is free. ([overview](https://www.vegvesen.no/en/fag/technology/open-data/a-selection-of-open-data/what-is-datex/)) | Incidents, road weather and travel time | Registered Application adapter; history in Ingestion/Data | P1 / source_only — NPRA access missing |
| Riksantikvaren OGC API Features | Open/NLOD; sensitive owner/interior/vulnerable-site data excluded. ([service record](https://data.norge.no/nb/data-services/4f097e4d-ea26-3a60-9f0d-ad1ed01a05cc/kulturminner-kulturmiljoer-ogc-api-features)) | Cultural-heritage property context | Shared geospatial adapter | P2 / deployed_verified |
| Miljødirektoratet air quality | Exact measured-observation service, unlike a generic institution-level integration. ([service record](https://data.norge.no/en/data-services/839dd201-bf3f-3275-8845-ca9de645d008/luftkvalitetsmalinger-i-norge-api)) | Local air-quality context | Application observation adapter | P2 / deployed_verified |
| TED Search API | Supported anonymous procurement search. ([documentation](https://docs.ted.europa.eu/api/latest/search.html)) | Neutral procurement-notice lookup | Application lookup; documents in Ingestion/Data | P2 / source_only |
| Doffin notices | The documented API is credentialed/submission-oriented; the official catalog lists monthly CC BY 4.0 CSV and zero read APIs. ([dataset record](https://data.norge.no/en/datasets/a77b0408-d84b-36e9-a7bf-112437867171/kunngjoringer-av-offentlig-anskaffelser)) | National procurement batch coverage | Ingestion of official CSV; use TED for fresher EU/above-threshold search | P2 / source_only |
| eInnsyn | Official OpenAPI contains search but globally requires `X-EIN-API-KEY`; production onboarding uses Ansattporten and delegated Altinn administration. ([specification](https://github.com/felleslosninger/einnsyn-api-spec), [onboarding](https://docs.digdir.no/docs/eInnsyn/publisering_med_api.html)) | Only after read/search rights, purpose and limits are verified | Credentialed domain adapter; no internal website scraping | Discovery / blocked |
| Frost observations | Client ID required through Basic auth or OAuth2. ([authentication](https://frost.met.no/authentication.html)) | Historical station observations | Registered Application adapter; bulk/history in Data | P2 / source_only — client ID missing |
| Data.norge SPARQL/Resource Service | Public catalog APIs; internal Search API is unstable. ([SPARQL](https://data.norge.no/en/technical/api/sparql), [Resource Service](https://data.norge.no/en/technical/api/resource-service), [warning](https://data.norge.no/en/technical/api/search)) | Catalog-change and deprecation monitor | Ingestion job | P0 / source_only |
| Tolletaten open data | Free JSON/XML under CC BY 4.0; includes commodity codes, tariff structure, duties, quotas and exchange rates. ([overview](https://www.toll.no/no/bedrift/apne-data/), [catalog](https://data.toll.no/organization/tolletaten?res_format=JSON&res_format=XML)) | Dated, non-binding customs/landed-cost reference | Versioned Ingestion/Data snapshots plus shipping-domain lookup | P1 / blocked pending approved shipping/customs use case |
| NAV vacancy feed | Free, but prompt update/removal is required and ads may contain personal data. ([terms](https://arbeidsplassen.nav.no/vilkar-api)) | Labour-market feed | Ingestion with deletion/TTL compliance | P3 / source_only |
| Kystverket open AIS | Anonymous/NLOD but excludes small fishing and recreational vessels. Closed AIS is restricted. ([access](https://www.kystverket.no/en/sea-transport-and-ports/ais/access-to-ais-data/)) | Maritime operational context | Shipping/maritime domain capability | P3 / discovery |
| Folkeregisteret | Purpose, legal basis, rights package, terms, Altinn delegation and Maskinporten required. ([access](https://www.skatteetaten.no/deling/folkeregisteret/intro/fa-tilgang/)) | Approved identity/address facts only | Restricted domain service plus Control/Auth | Restricted / blocked pending legal basis |
| Altinn + Maskinporten | Maskinporten authenticates the machine; Altinn supplies resource authorization/delegation semantics. ([guide](https://docs.altinn.studio/en/authorization/getting-started/authentication/maskinporten/)) | Shared credential and authorization foundation | Control/Auth or Integration, followed by domain adapters | Restricted / post-MVP |

## 8. Harmonized contracts

### 8.1 Canonical source envelope

Every response must include:

```json
{
  "source": {
    "provider": "nve",
    "dataset": "flood-warning-v1.0.10",
    "source_url": "https://api01.nve.no/...",
    "license": "NLOD-2.0",
    "retrieved_at": "2026-07-20T12:00:00Z",
    "effective_at": "2026-07-20T06:00:00Z",
    "expires_at": "2026-07-21T06:00:00Z",
    "quality": "authoritative_provider",
    "coverage": "regional_warning",
    "status": "measured|forecast|estimated|partial|stale|unavailable",
    "unavailable_reason": null
  },
  "data": {}
}
```

Also preserve API/dataset version, transformation version, required attribution, upstream request correlation, warnings/footnotes, and CRS for geometry. Never substitute synthetic or estimated data without declaring it.

### 8.2 Domain contracts

- **Location/Address:** normalized input, Kartverket address ID, municipality code, postcode, coordinates and CRS. Owner/rights data is never added implicitly.
- **PropertyContext:** cadastral reference plus separately permissioned facts. It must not issue legal-title, valuation, insurance, or binary safety conclusions.
- **HazardAssessment:** distinguish `warning`, `susceptibility`, `mapped_hazard_zone` and `observation`; expose coverage gaps.
- **TimeSeriesObservation:** source, station/series ID, element, value, unit, quality, observed time and revision status.
- **Journey:** scheduled legs, realtime deltas, cancellations, accessibility and source coverage. “No realtime feed” is not “on time.”
- **Statistic:** table ID, dimension/value codes, period, unit, status markers, footnotes and a reproducible query hash.
- **LegislativeDocument:** official ID, type, version/effective dates, status, source URL and citations.
- **ProcurementNotice:** Doffin/TED identifiers, buyer organization number, CPV, procedure, status, deadlines, source version and linked documents.

## 9. Composite features

### Logistics context

Address normalization → NVDB road/vehicle attributes → DATEX incidents/weather/travel times → MET/NVE conditions → Tolletaten tariff reference → `shipping-core` quote or tracking. `shipping-core` remains the sole carrier-integration owner. Any landed-cost result is explicitly non-binding and records the commodity code, origin grouping, currency/date and tariff snapshot.

### Property preflight

Open address/cadastral location → NVE hazards → Riksantikvaren protection → selected Miljødirektoratet layers. Return evidence, coverage and caveats; never infer ownership or legal suitability from open location data.

### Norway regulatory monitor

Lovdata current rule + Storting bill, vote and proceeding history. Application serves current bounded lookups; Ingestion/Data captures revisions and supports retrieval.

### Public-procurement discovery

Use TED's documented search API for fresher EU/above-threshold notices and Doffin's official monthly CSV for wider national batch coverage. State that the two sources are not coverage-equivalent. Saved searches, alerts, document workflows and bid state—not another provider adapter alone—justify a future `tender-core` ADR.

### Economic context

SSB statistics and Klass codes + Norges Bank SDMX. Preserve series/table identifiers, revisions, footnotes and query hashes so generated answers remain reproducible.

### Catalog intelligence

A scheduled Data.norge SPARQL/Resource Service diff identifies new datasets, deprecations, licence/access changes and owner contacts for human review. Do not depend on Data.norge's unstable internal Search API.

## 10. Canonical harmonization keys

Join authoritative data on stable identifiers with validity intervals, never only on free-text names:

- SSB Klass municipality, county and industry codes;
- Brreg organization number;
- Kartverket address/property identifiers plus declared CRS/EPSG;
- NVDB road-system references;
- Entur stop, service-journey and trip identifiers;
- MMSI/IMO for vessels;
- Tolletaten commodity/HS code plus tariff validity interval;
- Lovdata document ID plus version/effective date.

## 11. Delivery sequence

### Phase 1 — low-friction bounded lookups

- Kartverket Address REST only
- Kartverket open property-location lookup, with freshness and geometry caveats
- SSB PxWebApi v2 + Klass
- Entur Journey Planner v3
- Storting open data
- Lovdata open API
- Norges Bank SDMX
- Tolletaten open tariff data for an approved shipping/customs use case

### Phase 2 — geospatial and operational context

- DATEX II after registration
- NVE warnings and GIS products as separate adapters
- NVDB API Les v4
- Riksantikvaren OGC APIs
- selected Miljødirektoratet APIs

Build shared geometry, CRS and coverage semantics before composing property or logistics answers.

### Phase 3 — feeds, corpora and workflows

- Data.norge catalog watcher
- curated SSB/Norges Bank series
- Lovdata/Storting version ingestion
- NAV vacancy feed with deletion compliance
- TED procurement lookup and document ingestion
- monthly Doffin CSV ingestion for national coverage

### Phase 4 — restricted delegated data

- Maskinporten token broker and Altinn resource authorization
- Matrikkel/Grunnbok where an approved purpose exists
- Folkeregisteret only with legal basis, DPIA, audit and strict retention
- closed AIS only with an approved purpose and redistribution controls

## 12. Definition of done per integration

- provider record approved and ownership unambiguous;
- licence, attribution, privacy and cache/retention decisions recorded;
- schemas validate all external inputs and bound query size;
- canonical source envelope returned on success, partial data, stale data and failure;
- timeouts, retries, circuit breaking and rate-limit handling verified;
- unit, integration and critical end-to-end tests pass with the project's coverage threshold;
- credentials never appear in source, logs, errors or shared caches;
- feature flag, rollback path and provider health telemetry exist;
- running artifact and consumers are tested; state becomes `deployed_verified` only with recorded evidence;
- documentation names the exact endpoint/version and its last verification date.

## 13. Evidence discipline

Customer counts, named-customer demand, competitor capabilities, and implementation-effort estimates require an owner and an internal source such as CRM evidence, an ADR, or a dated benchmark. Until then, label them `internal_signal_needs_owner` rather than factual justification.
