# Norway data-source roadmap completion status

**Updated:** 2026-07-21

This is the current operational status. It supersedes earlier claims that all
implemented adapters were merely `source_only`.

## Running artifact

| Check | Result |
|---|---|
| Docker image | Rebuilt from current source |
| Container | `information-core`, running healthy |
| Bind address | `127.0.0.1:3190` |
| `GET /health` | `200` |
| `GET /ready` | `200` |
| Internal API key | Present locally; `.env` is owner-readable only |

## Live provider verification

The following bounded smoke requests returned `200` from the running Docker
artifact on 2026-07-21:

- Kartverket Address REST and open property location
- SSB PxWebApi v2 metadata
- Entur Journey Planner v3
- Storting representatives
- Norges Bank SDMX
- MET Norway weather forecast
- Statens vegvesen NVDB API Les v4 and traffic data
- NVE avalanche warnings
- Riksantikvaren kulturmiljoer OGC Features
- Miljødirektoratet air-quality observations and aggregate

Two live adapter defects found during this verification were fixed before the
final matrix: Norges Bank SDMX series paths preserve the `/` separator, and
NVDB V4 sends the required `X-Client: coresystem-information-core` header.

## Credential and access status

| Source | State | Evidence or blocker | Required action |
|---|---|---|---|
| Lovdata current-law API | `source_only` | Route and fail-closed behavior tested; `LOVDATA_API_KEY` is missing and the route returns `503 source_not_configured`. | Obtain authorized API access via [Lovdata](https://lovdata.no/info/api), then inject the key through secrets management. |
| Statens vegvesen DATEX II | `source_only` | Adapter and fail-closed behavior tested; `DATEX_USERNAME` and `DATEX_PASSWORD` are missing and the route returns `503 source_not_configured`. | Submit the [NPRA access request](https://www.vegvesen.no/en/fag/technology/open-data/a-selection-of-open-data/what-is-datex/get-access/?lang=en), then inject issued credentials. |
| MET Norway Frost | `source_only` | Adapter and fail-closed behavior tested; `FROST_CLIENT_ID` is missing and the route returns `503 source_not_configured`. | Create a client/user through [Frost authentication](https://frost.met.no/authentication.html), then inject the client ID. |
| eInnsyn | `blocked` | API key and delegated organization onboarding are required; no adapter is enabled. | Approve purpose, read/search role, API key, delegation, retention, and legal review. |
| Matrikkel/Grunnbok | `blocked` | Agreement and disclosure controls required. | Approve a concrete property use case and access agreement. |
| Folkeregisteret | `blocked` | Legal basis, rights package, DPIA, delegation, audit, and retention required. | Complete privacy and authorization review. |
| Tolletaten | `blocked` | No approved shipping/customs use case for this Application Plane service. | Keep snapshots in Ingestion/Data and approve a shipping-domain consumer first. |
| Kystverket AIS | `discovery` | No approved maritime use case; open coverage excludes some vessel classes. | Define use case and coverage requirements before adapter work. |
| NVE GIS/HydAPI | `discovery` | Product-specific endpoint and coverage selection remains open. | Select the exact product and owner. |

No credentials were generated, guessed, or committed. Provider registration
requires authorized organization identity, purpose, contact information, and
acceptance of provider terms.

The safe implementation tranche of the 2026-07-20 roadmap is implemented. The
bounded public Application Plane routes listed above are `deployed_verified`
for smoke evidence; credentialed routes remain `source_only`; and the
Ingestion Plane has tested handoffs for catalog, series, legal/parliament, NAV,
TED, and Doffin data.

## Implemented and current state

`deployed_verified`: Kartverket Address REST and open property location; SSB
metadata; Norges Bank SDMX; Entur; Storting; MET weather; NVDB V4; traffic;
NVE warnings; Riksantikvaren OGC Features; Miljødirektoratet observations and
aggregates.

`source_only`: Lovdata search, DATEX II 3.1, Frost observations, Data.norge
catalog diff, curated official-series snapshots, Lovdata/Storting revision
snapshots, NAV deletion-aware feed, TED Search, and Doffin CSV diffs. These
have source or contract evidence but still need provider-specific live,
consumer, or scheduled-ingestion acceptance.

## Explicitly not completed by design

These cannot be honestly marked deployed or implemented without external authority:

- NVE GIS/HydAPI: product-specific endpoint/coverage selection remains discovery.
- eInnsyn: requires API key, read/search purpose, and Ansattporten/Altinn delegation.
- Tolletaten: blocked until an approved shipping/customs use case exists.
- Kystverket AIS: no approved maritime use case; open coverage is not complete vessel coverage.
- Matrikkel/Grunnbok: agreement, disclosure controls, and approved property purpose required.
- Folkeregisteret: legal basis, DPIA, delegation, audit, and strict retention required.
- Maskinporten/Altinn broker and closed AIS: post-MVP restricted capabilities.

## Deployment gate

Every route still needs production promotion evidence for provider rate limits,
consumers, feature flags, telemetry, retention, deletion handling, and ZDR
propagation. The public routes above have running-artifact/live-contract
evidence; credentialed and ingestion routes remain `source_only` until their
specific gates pass. No source-only adapter writes directly across a plane
boundary.

## Remaining promotion work

1. Provision and live-test Lovdata, DATEX II, and Frost.
2. Run consumer compatibility, tenant-membership, ZDR, rate-limit, telemetry,
   and feature-flag acceptance checks.
3. Finish discovery/restricted decisions listed above; do not move those rows
   into `information-core` without an approved owner and access model.
