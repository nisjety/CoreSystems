# Lovdata and Storting revision handoff

**Status:** source-only Phase 3 handoff  
**Updated:** 2026-07-21

`Quarry-v2/services/quarry-control/internal/revisions` captures bounded
revision snapshots from:

- Lovdata current-law search, using the configured `X-API-Key`, up to three
  terms and twenty results per request.
- Storting open-data exports, using an allowlist of documented export
  resources and XML or JSON format negotiation.

Every snapshot includes provider/dataset identity, resource ID when available,
provider version when present, content type, query hash, content hash,
retrieval time, licence, and the original payload. Storting XML versions are
read from `<versjon>`; the content hash remains the fallback revision identity
when a provider response has no explicit version.

This is not a legal interpretation layer. Lovdata document/API access and
terms must be reviewed before enabling recurring collection; the collector
does not perform mass downloads or crawl the public website. The next step is
an approved Data Plane document/source-object mapping with immutable version
identity, citation links, deletion semantics, idempotency, and ZDR policy.
