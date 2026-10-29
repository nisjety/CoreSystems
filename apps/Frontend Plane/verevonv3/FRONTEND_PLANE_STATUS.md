# Frontend Plane (verevonv3) — Current Status

> **2026-07-15 Data Plane/GraphRAG wiring update:** every interactive Knowledge,
> document, retrieval, wiki, source, navbar-search, ingestion-source, onboarding
> graph-preview, and Operating Map request now uses a session-minted
> `aud=data-plane` bearer. Tenant identity is derived from canonical membership;
> token issuance failure returns 503 and never falls back to a shared internal
> key. The SPA normalizes the actual documents/wiki/retrieval response envelopes,
> paginates the workspace document inventory with an explicit truncation signal,
> and uses the real singular `document_id` chunk contract. Navbar search now
> calls `/v1/knowledge/search` rather than the nonexistent `/v1/search`.
> GraphRAG reads and onboarding preview use the verified tenant-bound graph API.
> Source evidence: **272/272 gateway tests**, **360/360 frontend tests**,
> TypeScript typecheck, production build, and lint (one pre-existing Solid
> reactivity warning, zero errors) pass. An isolated Data checkpoint also passed
> its seven-family HTTP auth matrix **28/28**, including authorized
> retrieval and graph reads, but it did not run the Verevon browser/gateway or
> replace the shared deployment. That checkpoint also predates the final
> Documents signed-ZDR/source-object guards. The shared running images predate
> these source changes, so end-to-end deployment effectiveness is not yet claimed.

> **2026-07-13 Model Plane handoff correction:** the 2026-07-11 chat-tool
> findings below are retained as history, but their prescribed “advertise tools
> on every turn” fix is not the current release policy. Plain chat is
> intentionally tool-free; Browse, composer-selected actions, Plan, and Agent
> Run Console are explicit user choices. The inline Model Gateway dispatcher
> currently recognizes bounded read tools and rejects unknown/write actions;
> governed writes belong on the agentic approval path. Frontend/Auth Core source
> now issues and forwards separate exact Model, inference, execution, session,
> capability, cost, and Data Plane credentials with explicit 503 on required
> issuance failure. Three focused gateway tests and 37 Auth Core tests pass, but
> the running Model Gateway/inference listeners are absent and capability
> catalogs still diverge. See
> [the handoff](docs/MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md) and
> [Model Plane status](../../Model%20Plane/MODEL_PLANE_STATUS.md).

Last verified: 2026-07-15 for the Data Plane/GraphRAG source wiring;
2026-07-13 for the Model Plane credential/capability handoff; 2026-07-11 for
the historical full-plane source audit below.

## Headline: verevonv3 is real and fully wired

verevonv3 (the canonical frontend; verevonv2 is deprecated) is **fully migrated off mocks**. `src/shared/mocks/` is gone, `verevon-operating-model.ts` has zero references, and all 20 SPA feature areas call real gateway APIs (`shared/api` has 56 client files). The ~50-domain Rust BFF gateway is real throughout — every domain forwards to its owning plane with a minted credential; no canned data, no dead 501 routes. The historical `x-verevon-org-id` cross-tenant IDOR is fixed and regression-tested (org derived from the verified session).

So "features that should work don't" is **not** the SPA faking data. Current
Model-dependent failures are the absent live inference path, divergent
capability semantics/UX, and incomplete shipping/approval integrations, plus
the historical backend and plane-local findings below.

## Per-area reality (2026-07-11)

- **18 areas fully REAL**: auth, billing, chat, core, cost, dashboard, finetune, inbox, ingestions, insights, knowledge, leads, onboarding, quality, router-policy, social, studio, tickets.
- **2 MIXED (honest)**: agents (real run console + honestly-labeled "Blueprint" design-preview), settings (mostly live; two residual fabricated rows — see below).

## Actionable findings

| # | Severity | Item | Fix location |
|---|---|---|---|
| 1 | **Fixed 2026-07-15** | `onboarding/graph-preview` previously trusted client `org_id`; it now derives tenant from the verified session and forwards a Data bearer | Regression-covered in gateway auth tests |
| 2 | MEDIUM (IDOR) | onboarding connector actions trust client body `org_id` (no membership gate) | `apps/gateway/src/onboarding/actions/connectors.rs` |
| 3 | Superseded policy finding | Plain chat sends no tools by design. Users need an explicit capability-backed Actions/Plan choice; do not default-advertise every tool. | Model-owned capability contract + Frontend action affordance |
| 4 | Not reproduced as executable write bypass | The inline gateway path is bounded to known read tools and rejects unknown/write actions; approval-required capabilities must still select agentic mode and need cross-plane E2E. | Capability contract + agentic selection regression tests |
| 5 | MEDIUM | settings `businessHourRows` + `roleRows` fabricated rows shown as live | `src/features/settings/components/WorkspaceSettingsPage.tsx` (empty them, per the Phase-4 de-fake pattern) |
| — | (fixed) | MCP-add form let stdio-transport + HTTPS URL create a dead record | `McpServersSection.tsx` — validation added, typecheck green |

## The chat-tools nuance (your headline)

The tool loop is real; a plain chat turn intentionally does not advertise
tools. Browse and selected read actions can use the bounded direct loop; Plan
and Agent Run Console use the governed path. The remaining product fix is an
explicit Actions affordance derived from the authoritative capability state,
with automatic agentic selection for approval-required actions. The literal
`get_shipping_quotes` example still is not in the SPA action registry or direct
Model executor, so it must not be advertised until both sides share that
contract.

## Confirmed solid (don't re-litigate)

- Gateway auth boundary: org from verified session, forged `x-verevon-org-id`/identity headers stripped at ingress, per-plane audience/HMAC tokens minted (not raw shared key). Regression-tested.
- Gateway domains all real thin proxies (chat→model-gateway, inbox→conversation-core, knowledge→Data Plane fan-out, social/insights/leads/shipping/mcp/cost, etc.).
- SPA fully off mocks; auth/session/onboarding flows real.

## Related docs

- `docs/core-research/plane-audit-2026-07-11.md` — full findings
- `docs/core-research/*.md` — updated (mock-backed-surfaces.md flagged for deletion)
- `FRONTEND_PLANE_ROADMAP.md` — fix plan
