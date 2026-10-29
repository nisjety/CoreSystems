# Curated official-series handoff

**Status:** source-only Phase 3 handoff  
**Updated:** 2026-07-21

`Quarry-v2/services/quarry-control/internal/officialseries` collects bounded
official-series payloads for later Data Plane ingestion:

- SSB PxWebApi v2 JSON-stat2 data by five-digit table and explicit variable
  selections.
- Norges Bank SDMX data by series and either an explicit period window or at
  most 100 recent observations.

Each result is an immutable snapshot containing provider, dataset, source URL,
licence, retrieval time, a SHA-256 query hash, and the validated JSON payload.
The client caps request/response sizes and rejects unbounded or inverted time
windows.

This package does not write directly to Data Plane tables. The next handoff
must map approved snapshots to the Data Plane document/source-object contract,
including source identity, versioning, deletion semantics, ZDR classification,
and an idempotency key derived from provider plus query hash.
