# Registered weather and road-feed handoff

**Status:** source-only Phase 2 completion slice  
**Updated:** 2026-07-21

Two previously proposed registered-source adapters are now implemented in
`information-core`:

- `/api/v1/datex/situation` is fail-closed until a registered DATEX II 3.1
  endpoint, username, and password are configured. It preserves provider XML
  and does not invent traffic measurements.
- `/api/v1/frost/observations` is fail-closed until a MET Norway Frost client
  ID is configured. Sources, elements, reference time, and result count are
  bounded; the response remains provider JSON with provenance.

DATEX credentials and Frost client IDs are never placed in source envelopes,
logs, errors, or cache keys. Live provider verification and registration
evidence are still required before either roadmap row becomes
`deployed_verified`.
