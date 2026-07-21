# TED and Doffin procurement handoff

**Status:** source-only Phase 3 handoff  
**Updated:** 2026-07-21

`Quarry-v2/services/quarry-control/internal/procurement` keeps the two
procurement paths separate:

- TED uses the anonymous Search API `POST /v3/notices/search` for bounded,
  current EU-level notice lookup. Page-number requests are capped at the
  documented 250 notices per page and 15,000-notice pagination ceiling.
- Doffin uses the official year-addressed CSV distribution because the
  catalog currently lists no read API. The parser supports the documented
  semicolon-separated CSV shape, requires a stable notice identifier, hashes
  normalized fields, and diffs complete snapshots into added/updated/removed
  notices.

Neither collector invents national coverage: TED is not a complete Doffin
replacement, and Doffin CSV availability/version cadence must be verified for
each year before scheduling. Procurement notices are retained as source
records only; later Data Plane mapping must preserve notice IDs, buyer
identity, CPV/procedure/status/deadlines where present, source version, linked
documents, and deletion/retraction semantics.
