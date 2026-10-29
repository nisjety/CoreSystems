# NAV vacancy feed handoff

**Status:** source-only Phase 3 handoff  
**Updated:** 2026-07-21

`Quarry-v2/services/quarry-control/internal/navacancies` consumes the current
authenticated NAV job-vacancy feed. It uses the configured bearer token,
supports provider pagination, and forwards `ETag` and `Last-Modified` values
for conditional polling.

The normalized change model keeps only high-level vacancy metadata: ID, URL,
title, business name, municipality, status, modification time, and a content
hash. It does not fetch or retain full vacancy details or contact lists.

`Apply` maintains an active-only state and emits explicit `Upserted` and
`Removed` sets. An `INACTIVE` feed item is always treated as a deletion
tombstone; absence from one page is not treated as deletion because the NAV
feed is paginated and continuous. The next Data Plane mapping must preserve
this deletion behavior, apply a short retention policy, and prevent inactive
contact information from being republished.

The collector is not scheduled or persisted yet. A NAV consumer token and
approved privacy/retention configuration are required before live activation.
