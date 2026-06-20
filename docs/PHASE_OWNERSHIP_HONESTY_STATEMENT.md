# Per-User Data Ownership & Sharing — Honesty Statement

> **Status: NOT YET CLAIMABLE.** As of this writing the platform makes **no**
> per-user privacy guarantee to any user, on any surface. This document records
> exactly what is enforced, what is not, and the conditions that must ALL be true
> before any "Private" badge, ShareDialog guarantee, Trust-Center claim, or "the
> AI only sees your data" statement may ship. It is the written gate required by
> `docs/PHASE_OWNERSHIP_PLAN.md` (PR-4) and exists to make the dominant failure
> mode — a half-enforced ACL that *advertises* confidentiality it does not
> deliver — impossible to reach by accident.

## What IS enforced today (verified)

- **Single grant authority.** `resource_grants` (user-core, migration 012) is the
  one store for explicit per-subject grants; both dormant `document_acl` copies
  (user-core 009, Data Plane init.sql) are dropped. A guard test asserts no Go
  reads the legacy table; an ownable-vs-team taxonomy is a Go+Rust constant with a
  CI parity lint.
- **Document ownership columns.** `documents.owner_id` + `visibility ∈
  {private, org, shared}` exist; existing rows were grandfathered to
  `visibility='org'` (owner = creator, system account when unknown) in one atomic,
  zero-NULL-verified migration. **New documents also default to `org`.** Nothing is
  private yet unless a user explicitly opts in — which the UI does not yet offer.
- **Filter-at-source.** documents-api `GET`/`LIST` filter by
  `owner_id = viewer OR visibility IN ('org','shared') OR an explicit grant`
  (grants resolved cross-plane via the user-core authz facade). Integration tests
  prove a private doc is hidden from a non-owner, org docs are seen by all, and a
  grant scopes to exactly that user + doc.
- **Retrieval post-filter.** retrieval-engine step-6 (`filter_live_candidates`,
  post-fusion + post-rerank, over the unified dense+sparse+wiki list) applies the
  same ownership predicate when a viewer identity is present. Because it runs
  post-fusion it gates the dense **and** sparse arms uniformly. An integration test
  asserts the predicate (non-owner cannot see a private doc; a grant surfaces a
  shared doc; owner sees own; no-viewer legacy sees all).
- **Viewer identity propagation.** HTTP retrieve resolves a per-user `AuthContext`
  from `x-user-id` (forwarded by the gateway, which derives it from the session and
  strips any client-supplied value at ingress) and the authenticated id overrides
  any caller-supplied body value. The agent grounds as the run's verified
  `user_id` (model-gateway chat grounding + `knowledge_search` tool; execution-core
  RunAgent loop). The gRPC retrieve handler threads `RetrieveRequest.user_id`.
- **Always-on, decoupled.** Per-user ownership enforcement fires whenever a
  `user_id` is present, **independently of `CONTROL_PLANE_ENFORCEMENT`** (which
  gates only the coarse org-axis ACL). A default-off deploy cannot silently disable
  per-user privacy.

## What is NOT enforced yet (the honest gaps — must close before any claim)

These do not leak anything **today** because every document is `visibility='org'`.
Each becomes a live leak the moment a single document is set `private`, so all are
**blockers before `CONTROL_PLANE_ENFORCEMENT` is promoted to Strict and before any
privacy claim is shipped**:

1. ✅ **CLOSED (PR-4).** Secondary retrieval endpoints `/v1/retrieve/{chunks,
   sources,freshness}` and `compare(documents)` now apply the same ownership
   predicate at the SQL level (raw chunk text for a non-visible doc is never even
   fetched). `/v1/retrieve/pack` and the main `/v1/retrieve` are gated.
2. ⏳ **OPEN.** Graph grounding (`/v1/retrieve/graph`, `compare(entities)`,
   model-gateway `load_graph_grounding`) carries no `user_id`; graph
   entities/summaries are org-scoped. Out of the documents-only MVP scope — decide
   the ownership model for derived graph data, or exclude private-derived entities,
   before graph results are surfaced under a privacy claim.
3. ⏳ **OPEN.** `ExecuteStep` gRPC primitive carries no `user_id` (proto gap) →
   org-scoped fallback. The agentic grounding path (RunAgent) is correctly
   viewer-scoped; add `user_id` to `ExecuteStepRequest` to close the primitive too.
4. ⏳ **OPEN (mitigated).** `x-user-id` trust on the api-key path relies on the
   gateway stripping any client value at ingress (verified — the gateway strips it
   and derives identity from the session). Add a membership cross-check (or restrict
   user-scoped internal calls to the JWT path) as defense-in-depth before Strict.
5. ✅ **CLOSED (PR-4, code).** The retrieval visibility cache now has a NATS
   `aqencia.controlplane.acl.resource_grants.changed` subscriber that evicts the
   affected `(subject_id, org_id)` key on revoke (user-core's gRPC ACL handler
   publishes the event; the 5-minute TTL remains the backstop). **Deploy note:**
   retrieval-engine's NATS must reach the shared bus user-core publishes to (set
   `SHARED_NATS_URL`/dual-subscribe) for sub-TTL revocation; otherwise revocation
   falls back to the 5-minute TTL.

## MVP enforcement characteristics (so no one over-claims)

- **Enforcement is a POST-FILTER in canonical Postgres**, not a pre-filter. It is
  the authoritative gate and permanent backstop, but it is **recall-bounded under
  skew**: when many non-visible candidates crowd the fused set, the visible result
  can honestly **undershoot** `top_n` (we over-fetch `k_fetch = min(top_k*4, 1000)`
  to reduce this and emit a starvation signal — we never backfill with non-visible
  docs, and all counts/scores derive from the post-filter set).
- **The Qdrant payload ACL keys and the Quickwit/BM25 query-time ACL filters are
  INERT in the MVP.** Ownership is enforced only at the Postgres post-filter.
  Pre-filter (Qdrant payload + re-stamp + sparse query-time wiring) is FULL-phase,
  defense-in-depth only.
- **Wiki pages are org-shared knowledge**, not an ownable type; published wiki is
  intentionally exempt from the per-user gate.

## The release gate — ALL must be true before any privacy claim ships

1. Filter-at-source (documents GET/LIST) live. ✅
2. Retrieval post-filter live on **every** retrieval surface (main + pack done;
   **chunks/sources/freshness/compare/graph still open — gap #1, #2**).
3. Viewer identity reaches retrieval on every path (HTTP + agent done; **ExecuteStep
   gap #3**).
4. `CONTROL_PLANE_ENFORCEMENT` promoted Off → Permissive (audit-only shadow,
   deny-rate reviewed, moka/QPS load-tested) → **Strict** — never flip Strict and
   claim in the same step.
5. NATS revoke-invalidation wired (gap #5) so revocation is effective within
   seconds (5-min TTL backstop).
6. The release-blocking **four-path honesty test** green end-to-end: a doc marked
   Private by A is absent from B's (1) GET/LIST, (2) dense retrieval, (3) sparse
   retrieval, and (4) agent grounding through model-gateway.

Until **all six** hold, badges, ShareDialog guarantees, Trust-Center copy, and any
"the AI only sees your data" statement **must not render**. A badge derives from the
same authority retrieval enforces; it can never assert a property the retrieval path
does not deliver.
