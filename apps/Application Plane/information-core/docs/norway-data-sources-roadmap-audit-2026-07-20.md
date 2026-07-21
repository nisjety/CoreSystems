# Norway public-data roadmap: factual and architectural audit

> **Post-audit runtime addendum — 2026-07-21:** The service has since been
> rebuilt and deployed locally in Docker. Live public-provider evidence and
> credential gaps are tracked in
> `norway-data-sources-completion-2026-07-21.md`; the dated findings below
> remain the architectural audit baseline.

**Date:** 2026-07-20  
**Scope:** the seven documents named in the request, `information-core/README.md`, the relevant `information-core` service surface, and current official provider documentation.  
**Overall confidence:** high for access/authentication and architecture findings; medium where the provider does not publish a stable consumer API or the claim depends on internal customer information.

## Executive assessment

The roadmap has the right strategic instinct—reuse existing planes, prefer authoritative public sources, and avoid premature consumer products—but it is not implementation-ready. Its admission rule, “public, no OAuth, read-only,” is too coarse, several access claims are wrong or stale, and its sequencing ignores the Application Plane's own secure-MVP gates.

The most important corrections are:

1. **Bring tracking is no longer unauthenticated.** Bring says unauthenticated Tracking API calls lost access on 13 May 2024, and all API connections require a Mybring user and API key. The current `information-core` documentation and optional-credential behavior are stale. Tracking can also expose personal data. ([Bring Tracking API](https://developer.bring.com/api/tracking/), [Bring API authentication](https://developer.bring.com/api/))
2. **“Kartverket address / matrikkel / grunnbok” is not one open API.** Address search and the separate property-location service need no registration; full Matrikkel and Grunnbok machine-to-machine access requires an application/agreement and is limited by applicable disclosure rules. The open property service is normally a day behind, and its geometry may be incomplete. ([Address API guide](https://www.kartverket.no/api-og-data/eiendomsdata/brukarrettleiing-adresse-api), [open property service](https://ws.geonorge.no/eiendom/v1/), [data requiring application](https://www.kartverket.no/api-og-data/eiendomsdata/data-som-krever-soknad), [API access application](https://www.kartverket.no/api-og-data/eiendomsdata/soknad-api-tilgang))
3. **Lovdata now has an open API.** Since November 2025, current laws and central regulations are available free under NLOD 2.0. The roadmap's “not a live JSON API” premise is stale, although durable indexing still belongs in the Data Plane. ([Lovdata API announcement](https://lovdata.no/artikkel/lovdata_tilrettelegger_for_bruk_tilpasset_var_ki-hverdag_gjor_api_av_oppdatert_regelverk_gratis_tilgjengelig_for_alle/5277), [API overview](https://lovdata.no/info/api), [use terms](https://lovdata.no/info/vilkar))
4. **eInnsyn is not an anonymous, low-friction search API.** Its official specification includes search but applies API-key security globally; production onboarding is organization-oriented through Ansattporten and delegated Altinn administration. Validate the intended read rights, purpose and limits before scheduling it, and do not build against an internal website endpoint. ([OpenAPI specification](https://github.com/felleslosninger/einnsyn-api-spec), [production onboarding](https://docs.digdir.no/docs/eInnsyn/publisering_med_api.html))
5. **Frost is not anonymous.** Free observations require a registered client ID via Basic authentication or OAuth2; confidential data requires OAuth2. ([Frost authentication](https://frost.met.no/authentication.html))
6. **New sources should not precede the existing provenance/security rollout.** The local status and audit say the running information service still has the old traffic contract, the provenance fix is source-only, and the Application Plane is not secure-MVP ready.

## 1. Local-document consistency findings

### 1.0 Credential and runtime verification after this audit

The current local environment contains the required internal service key only.
No provider secret was found for `LOVDATA_API_KEY`, `DATEX_USERNAME`,
`DATEX_PASSWORD`, or `FROST_CLIENT_ID`. No credentials were generated or
submitted on behalf of the organization because each provider requires an
authorized registration, organization details, or acceptance of provider
terms.

The rebuilt Docker artifact is healthy on `127.0.0.1:3190`. Live calls returned
`200` for Kartverket Address REST, SSB metadata, Norges Bank SDMX, Entur
Journey Planner, Storting representatives, MET weather, Miljødirektoratet
observations and aggregates, NVDB V4, NVE avalanche warnings, Riksantikvaren
OGC Features, news, and traffic. Lovdata, DATEX II, and Frost were tested for
their fail-closed behavior and correctly return `503 source_not_configured`
until their provider access is provisioned.

The two defects found during this verification were fixed: Norges Bank SDMX
series paths now preserve the `/` dimension separator, and NVDB requests now
send the required `X-Client` header. Regression tests and live calls pass after
the Docker rebuild.

Credential acquisition paths:

- Lovdata: request/confirm API access through [Lovdata's API information](https://lovdata.no/info/api).
- DATEX II: submit the [Statens vegvesen DATEX access request](https://www.vegvesen.no/en/fag/technology/open-data/a-selection-of-open-data/what-is-datex/get-access/?lang=en); NPRA issues the username/password.
- Frost: create an authorized user/client ID as described in [Frost authentication](https://frost.met.no/authentication.html).
- eInnsyn and restricted sources: keep blocked until API-key onboarding, purpose, delegation, legal basis, and retention controls are approved.

### 1.1 Runtime truth is blurred

The roadmap calls weather, traffic, news, and Bring tracking “already live.” The more authoritative status documents say the running `information-core` is old and contains the deployed traffic-honesty defect; the corrected provenance contract is not deployed. The roadmap needs four explicit states:

- `deployed_verified`
- `deployed_known_defect`
- `source_only`
- `proposed`

Relevant local evidence:

- [APPLICATION_PLANE_STATUS.md](</Volumes/Lagring/Triodelab/CoreSystem/apps/Application Plane/APPLICATION_PLANE_STATUS.md>) lines 25–35
- [plane-audit-2026-07-13.md](</Volumes/Lagring/Triodelab/CoreSystem/apps/Application Plane/docs/core-research/plane-audit-2026-07-13.md>) lines 106–115
- [core-research README](</Volumes/Lagring/Triodelab/CoreSystem/apps/Application Plane/docs/core-research/README.md>) line 67

### 1.2 Bring tracking has two owners

`information-core` directly calls Bring and exposes `/api/v1/shipping/track`, while `shipping-core` declares carrier tracking orchestration as its bounded responsibility and exposes booking tracking. This is an existing duplication, not merely a future risk.

Recommended decision:

- Make `shipping-core` authoritative for carrier adapters and tracking.
- Deprecate direct Bring access in `information-core`.
- If the Application Plane needs a convenience lookup, make it a contract client of `shipping-core`, not a second Bring client.
- Do not cache shipment details in a shared ambient-data cache; treat tracking numbers and returned events as customer-linked data.

The shipping README also overstates its present security: it calls rate shopping authenticated and tenant-scoped, but records 200 responses without authentication and says no auth/tenant/ZDR middleware is mounted. Fix that before feeding it enriched address data.

### 1.3 “Shipping already handles addresses” is overstated

`shipping-core` validates address-shaped request fields. The reviewed docs do not establish canonical address lookup, normalization, geocoding, Matrikkel IDs, property resolution, or address lifecycle management. A new address contract is required; it cannot be assumed to exist.

### 1.4 Brreg consolidation omits `leads-core`

The roadmap mentions `org-core` and `execution-core`, while the audit and deep dive identify `leads-core` as another real Brreg consumer. Consolidation must include all three contexts. Prefer one maintained provider adapter/schema and domain-owned façades over a single catch-all service that mixes onboarding, lead generation, and agent lookups.

### 1.5 Source placement is confused with feature placement

The local architecture says:

- Application: app-facing live lookup/projection
- Ingestion: connector acquisition and change feeds
- Data: durable documents, embeddings, retrieval, and corpora
- Control/Auth: identity, credentials, delegation, authorization
- Model: tool selection and reasoning

Therefore a source can span planes. For example, SSB can have a live bounded query tool in `information-core` and curated series ingested into the Data Plane. The roadmap currently assigns whole institutions to one service.

## 2. Claim-by-claim verification

| Roadmap claim | Finding | Required correction |
|---|---|---|
| Bring tracking is public/no OAuth and works without credentials | **False/stale.** Authentication has been required since May 2024; authenticated responses may include personal data. | Require Mybring credentials, make readiness fail when missing, update the endpoint/version, and move ownership to `shipping-core`. |
| Kartverket address/matrikkel/grunnbok is public/no OAuth | **Materially false.** Address REST and property-location lookup are registration-free, but the latter can lag and have incomplete geometry. Full Matrikkel and Grunnbok access is application/agreement controlled. | Split the products/access classes; keep owner/rights data out of open address/property-location features. |
| Geonorge is an open/free sibling to Kartverket | **Misleading.** Geonorge is Kartverket's national catalog/distribution platform, not a separate data authority. | Name the exact dataset and distribution—Address REST, WFS, WMS, OGC API, or download—and its licence/access level. |
| NVE is one public flood/landslide/hydrology API | **Over-broad.** NVE provides separate warning REST APIs, HydAPI products, and changing GIS/ArcGIS/WMS services. Warnings have presentation and attribution requirements. | Model `warning`, `susceptibility`, `hazard_zone`, and `observation` separately; do not return a binary “property safe/unsafe” answer. ([NVE open APIs](https://www.nve.no/om-nve/aapne-data-og-api-fra-nve/), [flood warnings](https://api.nve.no/doc/flomvarsling/), [GIS change notices](https://www.nve.no/kart/nytt-om-gis-api/)) |
| Entur is public/no OAuth and covers Norway | **Mostly correct for the Journey Planner, but incomplete.** Journey Planner v3 covers all Norwegian public transport and is NLOD/open, but requires `ET-Client-Name`; privileged endpoints use OAuth2. Source-data richness varies by operator. | State the exact Journey Planner endpoint and mandatory client identification; preserve scheduled vs realtime provenance. ([Journey Planner v3](https://developer.entur.org/pages-journeyplanner-journeyplanner/), [authentication](https://developer.entur.org/pages-intro-authentication/), [timetable completeness](https://developer.entur.org/stops-and-timetable-data/)) |
| SSB is public/no OAuth | **Correct, with operational qualifications.** PxWebApi v2 is the current API, uses JSON-stat2 by default, allows 800,000 cells, and limits an IP to 30 queries/minute. It is CC BY 4.0. | Pin v2, retain table/dimension codes, statuses and footnotes, bound queries, and cache reproducibly. ([SSB v2 guide](https://www.ssb.no/en/api/pxwebapiv2)) |
| Altinn/Maskinporten is one per-org OAuth consent layer | **Conflated.** Maskinporten is machine-to-machine client authentication; Altinn adds resource authorization, delegation/system-user semantics, and in some APIs token exchange. | Build a credential/token broker in Control/Integration, then place each domain API by capability. Do not route all authorized data to `information-core`. ([Altinn Maskinporten guide](https://docs.altinn.studio/en/authorization/getting-started/authentication/maskinporten/), [Altinn authentication](https://docs.altinn.studio/en/dialogporten/user-guides/authenticating/)) |
| Doffin notice search is an unauthenticated API | **False/outdated.** The documented Doffin Notices API is submission-oriented and credentialed. The official catalog currently lists monthly CC BY 4.0 CSV resources and zero read APIs. | Use the Doffin CSV for national batch coverage and TED Search API for fresher EU/above-threshold notices; disclose that TED is not complete Norwegian coverage. ([Doffin dataset record](https://data.norge.no/en/datasets/a77b0408-d84b-36e9-a7bf-112437867171/kunngjoringer-av-offentlig-anskaffelser), [TED Search API](https://docs.ted.europa.eu/api/latest/search.html)) |
| Lovdata is only a restricted web corpus | **False since November 2025.** Current laws and central regulations are available through a free NLOD 2.0 API. | Add a live `legal-reference` lookup option; keep bulk/versioned indexing in Ingestion/Data. |
| Folkeregisteret should be omitted | **Directionally sound but too absolute.** Access can be granted to private/public businesses with an approved purpose, legal basis, rights package, terms, Altinn delegation and Maskinporten. | Classify it as `restricted/post-MVP`, not “refer users away forever.” Require a concrete legal basis, DPIA, controller/processor design, audit and no shared caching. ([access process](https://www.skatteetaten.no/deling/folkeregisteret/intro/fa-tilgang/), [rights packages](https://www.skatteetaten.no/deling/folkeregisteret/intro/finne-data/rettighetspakker/)) |
| Storting API is open and covers bills/votes/representatives | **Verified.** It is free, needs no registration, uses NLOD and is limited to 100 calls/minute. Documents are often XML/HTML, not uniformly JSON. | Add rate handling, stable identifiers, format-specific parsers, and citation links. ([service overview](https://data.stortinget.no/om-datatjenesten/), [terms](https://data.stortinget.no/om-datatjenesten/bruksvilkar/), [technical docs](https://data.stortinget.no/dokumentasjon-og-hjelp/teknisk-dokumentasjon/)) |
| Frost is a simple open weather sibling | **Data is open; access is authenticated.** A client ID is mandatory. | Put it in `registered_read`, not `anonymous_read`; preserve station, element, time resolution, quality code and licence. |
| NVDB exposes road geometry/speed limits/attributes | **Verified, but use v4.** API Les v4 became current in 2025; v3 is being retired. Read access is public/NLOD. | Pin v4 and the datakatalog version; do not hard-code object/property IDs without metadata resolution. ([NVDB v4 migration](https://nvdb-docs.atlas.vegvesen.no/nvdbapil/Migrering/), [public service record](https://dataut.vegvesen.no/en/dataservice/nvdb-les-nasjonal-vegdatabank)) |
| eInnsyn API can search post journals/documents | **Unsupported as an anonymous, low-friction integration.** The official OpenAPI includes search but applies `X-EIN-API-KEY` security globally; production-key onboarding is documented for organizations through Ansattporten and delegated Altinn administration. | Treat it as credentialed discovery. Verify that the granted role permits the intended read/search use, plus purpose, limits and terms, before prioritizing. ([OpenAPI specification](https://github.com/felleslosninger/einnsyn-api-spec), [production onboarding](https://docs.digdir.no/docs/eInnsyn/publisering_med_api.html)) |
| Brreg sub-registers are one small extension | **Partly correct, partly oversimplified.** The Enhetsregister API includes Frivillighetsregister data. Regnskapsregisteret has a separate API with an open latest-key-figures part and a restricted detailed part. Bankruptcy announcements/search and XML subscriptions have different access/retention/commercial rules. | Inventory exact Brreg products and current clients before estimating. ([Enhetsregister API](https://data.brreg.no/enhetsregisteret/api/dokumentasjon/no/index.html), [Regnskapsregister API](https://data.brreg.no/regnskapsregisteret/regnskap/swagger-ui/swagger-ui/index.html), [bankruptcy announcements](https://www.brreg.no/registersok/kunngjoringer/om-kunngjoringer/kunngjoringer-fra-konkursregisteret/), [XML subscription](https://www.brreg.no/bruke-data-fra-bronnoysundregistrene/abonnement/abonnement-pa-kunngjoringer-i-xml-format/)) |
| NAV vacancies are a narrow public feed | **Verified, but not low-governance.** Use is free, yet republication requires prompt update/removal and the ads may contain personal data. | Treat as a feed with deletion/update obligations, not a long-lived cache. ([NAV API terms](https://arbeidsplassen.nav.no/vilkar-api)) |
| Miljødirektoratet is one environmental API | **Too vague.** There are multiple independent services such as air-quality measurements, Vannmiljø, Naturbase/ArcGIS, and download feeds. | Select exact datasets. Air quality is a strong first candidate; contaminated-ground/nature datasets require different schemas and caveats. ([air quality API](https://data.norge.no/en/data-services/839dd201-bf3f-3275-8845-ca9de645d008/luftkvalitetsmalinger-i-norge-api), [selected nature types](https://data.norge.no/en/data-services/68374a72-8065-359b-b431-a65881c1588c)) |
| Riksantikvaren/Kulturminnesøk is merely a niche website | **Under-specified.** Riksantikvaren now publishes OGC API Features/WFS/WMS datasets under NLOD; sensitive owner/interior/vulnerable-site data is excluded. | Use the exact OGC dataset and protection status for property preflight. ([OGC API record](https://data.norge.no/nb/data-services/4f097e4d-ea26-3a60-9f0d-ad1ed01a05cc/kulturminner-kulturmiljoer-ogc-api-features), [sharing rules](https://dokumentasjon.ra.no/askeladden_brukerveiledning/aapne_data_og_karttjenester.html)) |
| Kystverket AIS is simply an open maritime API | **Partly correct.** Open AIS is free/NLOD without registration, but excludes small fishing and recreational vessels; the closed component requires application and restricts purpose/redistribution. | Split `open_ais` and `restricted_ais`; prefer BarentsWatch's documented API adapter for application use. ([Kystverket AIS access](https://www.kystverket.no/en/sea-transport-and-ports/ais/access-to-ais-data/)) |
| data.norge.no should be checked periodically | **Correct, but it can be automated.** Data.norge exposes public SPARQL and Resource Service APIs; the internal Search API is explicitly unstable. | Build a catalog-diff job using SPARQL/Resource Service and DCAT-AP-NO metadata, not manual checks or the internal Search API. ([SPARQL API](https://data.norge.no/en/technical/api/sparql), [Resource Service](https://data.norge.no/en/technical/api/resource-service), [Search API warning](https://data.norge.no/en/technical/api/search)) |

## 3. Unsupported assumptions

The following may be valid internal knowledge but were not supported by the listed files or authoritative public evidence found in this review:

- “Prokom, 19+ Norwegian municipalities” as a direct customer signal
- the Ayfie “Innsynsagenten” comparison
- current `org-core` Brreg coverage
- any assumption that the Doffin submission API also provides an anonymous read/search contract

Cobrief's own site does support the general competitive-space statement: it markets AI tender search, alerts, evaluation, writing, and follow-up. That does not by itself justify a `tender-core`; it instead argues for an explicit product/competition decision before going beyond neutral notice lookup. ([Cobrief description](https://cobrief.com/no/forklarer/cobrief/))

Label these as `internal_signal_needs_owner` or cite an internal CRM/ADR. Do not present them as externally verified facts.

## 4. Better architecture and harmonization

### 4.1 Replace the admission rule

A provider belongs in `information-core` only when all are true:

- bounded live lookup, not bulk acquisition or durable corpus management;
- read-only and low external side effect;
- data class is public or safely tenant-scoped;
- terms permit the intended caching, transformation and display;
- authentication is anonymous or a service credential, not end-user consent/delegation;
- the service can fail honestly and expose provenance/freshness;
- the query and response can be bounded for abuse, cost and privacy.

OAuth alone should not decide plane placement. A registered client ID can still fit; a public endpoint can still be privacy-sensitive or contractually unsuitable.

### 4.2 Canonical source envelope

Every new response should carry at least:

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

Also propagate dataset/API version, attribution text, upstream request correlation, transformation version, CRS for geometry, and upstream warnings/footnotes. This extends the traffic provenance contract already required by the July 13 audit.

### 4.3 Harmonized domain contracts

- **Location/Address:** normalized input, Kartverket address ID, municipality code, postcode, coordinates and CRS; never silently add owner/rights data.
- **PropertyContext:** cadastral reference plus separately permissioned facts; no binary “safe property” conclusion.
- **HazardAssessment:** `warning`, `susceptibility`, `mapped_hazard_zone`, `observation`; include coverage gaps and authority disclaimers.
- **TimeSeriesObservation:** source, station/series ID, element, value, unit, quality/status, observed time, revision.
- **Journey:** scheduled legs, realtime deltas, cancellations, data-source coverage and accessibility; distinguish “no realtime feed” from “on time.”
- **Statistic:** SSB table ID, dimension/value codes, unit, period, status markers, footnotes and query hash.
- **LegislativeDocument:** official ID, type, version/effective dates, status, source URL and citations; store immutable snapshots only in Data Plane.
- **ProcurementNotice:** Doffin/TED identifiers, buyer org number, CPV, procedure/status/deadlines, source version and linked documents.

### 4.4 Concrete composite features

1. **Logistics context:** address normalization → NVDB vehicle/road attributes → DATEX incidents/weather/travel time → MET/NVE conditions → Tolletaten tariff reference → `shipping-core` quote/tracking. DATEX requires registration even though use is free. Tolletaten publishes open JSON/XML tariff, commodity-code, duty, quota and exchange-rate data under CC BY 4.0; estimates must be labelled non-binding and tied to a dated tariff snapshot. ([Statens vegvesen DATEX](https://www.vegvesen.no/en/fag/technology/open-data/a-selection-of-open-data/what-is-datex/), [Tolletaten open data](https://www.toll.no/no/bedrift/apne-data/), [Tolletaten data catalog](https://data.toll.no/organization/tolletaten?res_format=JSON&res_format=XML))
2. **Property preflight:** open address/cadastral location → NVE hazard layers → Riksantikvaren protection → selected Miljødirektoratet layers. Output evidence and caveats, never valuation, legal title or insurance advice.
3. **Norway regulatory monitor:** Lovdata current rule version + Storting bill/vote/proceeding history. Live lookup can be Application-facing; change capture and semantic retrieval belong in Ingestion/Data.
4. **Public procurement discovery:** use TED Search API for fresher EU/above-threshold notices and ingest Doffin's official monthly CSV for broader national coverage. State the coverage difference. Saved searches, alerts, document ingestion and workflow state are the threshold for a dedicated `tender-core`.
5. **Economic context:** SSB PxWebApi v2 + Klass codelists + Norges Bank SDMX rates/series. Norges Bank's API is open, free and unauthenticated. ([Norges Bank open-data record](https://data.norge.no/en/datasets/23076ce8-b442-407c-817f-0d4ec3cbe744/api-for-apne-data))
6. **Catalog intelligence:** nightly Data.norge SPARQL diff producing candidate changes, deprecations, access/licence changes and owner contacts for human approval.

## 5. Recommended additional APIs/data sources

| Priority | Source | Why it adds value | Placement |
|---|---|---|---|
| P0 | Statens vegvesen DATEX II | Restores real incidents, road weather and travel times; more valuable than adding another metadata-only traffic endpoint. | Registered read adapter in `information-core`; durable feed history in Ingestion/Data if needed. |
| P0 | Lovdata open API | Corrects a stale assumption and enables current-law citations with low access friction. | Live lookup in Application; versioned corpus in Data. |
| P0 | data.norge SPARQL/Resource Service | Makes source discovery and deprecation monitoring systematic. | Ingestion catalog-watch job, not an end-user information module. |
| P1 | Norges Bank SDMX | Adds rates and monetary/financial context that complements SSB. | `information-core` bounded queries; curated series may be ingested. |
| P1 | Tolletaten open tariff data | Adds commodity codes, duties, quotas and exchange-rate reference to shipping/customs estimates. | Versioned Ingestion/Data snapshots with a bounded shipping-domain reference adapter. |
| P1 | Riksantikvaren OGC API Features | Strong property/municipality/planning complement with an open standard. | Geospatial adapter shared by property-context features. |
| P1 | Miljødirektoratet air quality | Clear, current measured-observation API and natural extension of weather/health context. | Observation adapter in `information-core`. |
| P1 | TED Search API | Supported anonymous procurement search and reusable notice data. | Neutral lookup first; domain service only after product approval. |
| P2 | NAV job feed | Useful for labour/HR use cases, but has removal and personal-data obligations. | Ingestion feed with TTL/deletion compliance, not ambient cache. |
| P2 | Kystverket/BarentsWatch open AIS | Strong shipping/logistics enrichment; open coverage has explicit vessel exclusions and must not be represented as complete. | Shipping/maritime domain adapter; no closed AIS without approved purpose. |
| Restricted | Folkeregisteret, full Matrikkel/Grunnbok, Altinn data, closed AIS | High-value but purpose-bound, credentialed and potentially sensitive. | Post-MVP Control/Integration + domain service + privacy governance. |

## 6. Revised sequence and acceptance gates

### Gate 0 — repair and deploy current truth

1. Deploy the existing information provenance fix with Model/UI consumers and verify source/runtime correlation.
2. Correct Bring authentication and endpoint assumptions; make missing credentials a non-ready state or remove the module.
3. Decide Bring tracking ownership and remove the duplicate provider call.
4. Put canonical membership, tenant boundaries, ZDR/log redaction and per-source rate limits in front of all new routes.

### Phase 1 — low-friction, bounded lookups

- Kartverket Address REST only
- Kartverket open property-location lookup, with freshness/geometry caveats
- SSB PxWebApi v2 + Klass
- Entur Journey Planner v3 with `ET-Client-Name`
- Storting open data
- Lovdata open current-law API
- Norges Bank SDMX
- Tolletaten tariff reference for an approved shipping/customs use case

Each ships independently behind a feature flag with provider contract tests, attribution, cache policy, query bounds, tool-catalog wiring and live negative/failure tests.

### Phase 2 — geospatial and operational context

- NVE warnings and GIS layers as separate products
- NVDB API Les v4
- DATEX II after registration
- Riksantikvaren OGC APIs
- selected Miljødirektoratet APIs

Build shared geometry/CRS and coverage semantics before composing property or logistics answers.

### Phase 3 — feeds/corpora and domain workflows

- Data.norge catalog watcher
- curated SSB/Norges Bank series
- Lovdata/Storting version ingestion
- NAV feed with deletion compliance
- TED/Doffin procurement only after product and competition review

### Phase 4 — restricted delegated data

- Maskinporten/Altinn token broker and resource authorization
- full Matrikkel/Grunnbok where an approved purpose exists
- Folkeregisteret only with legal basis and privacy controls
- closed AIS only with purpose approval

## 7. Document changes to make permanent

The proposed revised roadmap accompanying this report implements these changes. It also adds fields the current notes are missing:

- exact provider product and endpoint family;
- authentication class: `anonymous`, `identified`, `registered_key`, `oauth_service`, `delegated`, `agreement`;
- licence/attribution and caching/redistribution terms;
- personal/sensitive-data class;
- query mode: lookup, feed, bulk, corpus;
- plane/core owner and consuming features;
- coverage, freshness, limits and deprecation channel;
- verification date and official source;
- rollout state and acceptance criteria.

## Methodology and limitations

I read all listed local documents plus the `information-core` README and relevant service/router/provider code. Web verification prioritized official provider, Digdir, data.norge, and EU documentation. The reviewed official Doffin sources document credentialed submission plus monthly CSV reuse, not an anonymous read API; eInnsyn search is API-key protected. Public evidence was not found for the Prokom customer-count claim or the Ayfie comparison, so those remain explicitly unverified. As of the audit date, no production endpoints, credentials, customer data or state-changing operations were used; the post-audit 2026-07-21 runtime addendum used only bounded read-only public-provider smoke requests and no provider credentials.
