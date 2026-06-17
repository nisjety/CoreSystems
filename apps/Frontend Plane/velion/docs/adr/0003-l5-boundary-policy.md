# ADR 0003 — L5 boundary policy: velion's `src/app/api/*` proxies are the canonical L5 surface

- **Status**: accepted
- **Date**: 2026-05-09
- **Closes**: velion-gap.md G17 (decision); follow-up cleanup tracked separately
- **Supersedes**: none
- **Owners**: Frontend Plane (velion), Application Plane (convex-core, convex-gateway, notification-core)

---

## Context

`docs/ARCHITECTURE_DIAGRAM.md` (the Frontend Plane charter) declares a
strict layering rule:

> **Cross-Plane Contract Rule #1**: API-only access — Frontend calls
> Application Plane (L5) REST / WS endpoints; it never imports server-side
> packages from lower layers.
> **#5**: L5 is the ceiling — If a feature requires data from L1–L4, the
> Application Plane must expose a dedicated endpoint; frontend must not
> bypass L5.

Reality, verified against running code (2026-05-09):

| velion route | Upstream | Layer hit | Charter says |
|---|---|---|---|
| `/api/auth/[...path]` | auth-core `:3011` | L1 | should be L5 |
| `/api/user/[...path]` | user-core `:3012` | L1 | should be L5 |
| `/api/user/me/session-context` | user-core `:3012` | L1 | should be L5 |
| `/api/org/[...path]` | org-core `:8080` + billing-core `:3014` | L1 | should be L5 |
| `/api/ingestion/[...path]` | Quarry / imports-core | L3 | should be L5 |
| `/api/external/{zammad,nango,nohu}/[...path]` | externally hosted | (not in pyramid) | n/a |

Zero velion route currently goes through `convex-gateway` for L1–L3 data.
Convex is used only for reactive workspace data (Phase 8 of CONNECT_ROADMAP,
not yet started). The charter and the code disagree.

This ADR resolves the disagreement.

## Options considered

### Option A — Formalise the bypass: velion's `src/app/api/*` is the L5 surface

Update the charter to acknowledge that velion's Next.js route handlers
**are** the Frontend Plane's L5 ingress. They:

- Validate sessions via `control-plane-auth.ts` (single helper)
- Mint X-User-Id / X-Internal-Api-Key / X-Correlation-Id internal headers
- Enforce internal-key auth at the receiving Go cores (post-G15)
- Cache session verdicts (post-G1)
- Echo correlation IDs end-to-end (post-G15)

In exchange, velion accepts that any future second frontend (e.g.
mobile, public web, admin console) must replicate this proxy layer or
share a common helper package.

Convex-gateway stays in its lane: WebSocket fan-out for reactive
workspace data only. It does not mediate REST traffic.

**Pros**:
- Matches reality. No code refactor required.
- Lowest latency: velion proxies talk directly to L1 over the shared
  Docker network with no extra hop.
- All security primitives are already in place (G1, G2, G6, G15, G18,
  G24, G29).
- Single team owns the proxy code (Frontend Plane), single language
  (TypeScript), single deployment target (Vercel/Next.js standalone).

**Cons**:
- Charter doc rewrite required. Future contributors must understand
  the velion-as-gateway pattern.
- A second frontend (mobile app, admin console) cannot just point at
  `convex-gateway` for L1 data — it must either run its own proxy layer
  or call the velion routes (cross-frontend coupling).
- Operational rate-limiting / audit-logging / DLP must be implemented
  in velion's proxy layer rather than at one shared ingress.

### Option B — Extend `convex-gateway` to absorb the velion proxies

`convex-gateway` becomes the real L5 ingress. velion calls only
`convex-gateway`; the gateway forwards to L1–L4 services after
authentication and session validation.

**Pros**:
- Charter is accurate as written.
- Single ingress — natural place for cross-cutting concerns (rate limit,
  audit log, DLP, request-correlation, distributed tracing aggregation).
- Microfrontend-friendly: any new client points at convex-gateway with
  the same contract.
- Easier to evolve auth: change auth-core's session shape once, update
  the gateway, all clients see it.

**Cons**:
- ~5–15 ms extra latency per request (velion → gateway → core).
- Gateway becomes a SPOF for non-reactive traffic too. Today it's
  WS-only; making it required for REST raises its operational bar.
- Significant refactor: every velion route (~14 routes) must move into
  gateway or proxy through gateway. Risk of introducing regressions.
- Two-language gateway codebase (existing Node.js gateway proxies to
  Rust convex-backend; adding REST proxying complicates the ingress).

### Option C — Hybrid: velion proxies stay for now, but a future second-frontend triggers Option B

Codify Option A as the **interim** decision and explicitly schedule a
revisit when the second frontend appears.

**Pros**:
- Preserves optionality.
- Avoids premature abstraction (single frontend doesn't need a gateway
  yet).

**Cons**:
- Risk of "interim" becoming "permanent" without an explicit forcing
  function.
- Two future-state diagrams in flight at once is confusing.

## Decision

**Choose Option A**: formalise the bypass. velion's `src/app/api/*`
route handlers **are** the canonical Frontend Plane L5 ingress. The
charter is updated to match reality.

This is conditional on three guardrails:

1. **Common helper package** — all proxy code routes through
   `src/app/api/_lib/control-plane-auth.ts`. Any new proxy route must
   import `requireSession` and `buildControlPlaneHeaders` rather than
   re-implementing auth forwarding.
2. **Per-core internal-key middleware** — every L1–L4 core that velion
   talks to must enforce `X-Internal-Api-Key` server-side (already done
   for user/org/billing/session-core post-G15; documents-api-go and
   retrieval-engine-rs already done in the prior session).
3. **Audit triggers a revisit** — if a second frontend ships, or if
   cross-cutting concerns (rate limit / audit / DLP) become mandatory at
   ingress, this ADR is superseded by a new ADR that adopts Option B.

## Consequences

**Wins**:
- No refactor required. Today's stack ships as-is.
- Lowest latency for the single-frontend case.
- All security primitives are already verified live (G1, G15, G24).

**Costs**:
- The charter doc (`docs/ARCHITECTURE_DIAGRAM.md`) must be amended to
  describe velion's proxy layer as the de-facto L5 surface. Without
  this, new contributors keep tripping on the false constraint.
- Operational concerns that *would* live at a shared gateway (rate
  limiting, audit log shipping, DLP scanning) must be implemented per
  velion route or in a velion-internal middleware. Track each as a
  separate gap if/when the need arises.
- A second frontend triggers a re-evaluation. The ADR explicitly
  schedules this revisit.

**What we're giving up**:
- The cleanest possible mental model ("frontend → L5 → L1–4"). The
  reality is "frontend → frontend's own proxy → L1–4 with L5 for
  reactive only."
- Future microfrontends that expect a gateway-shaped ingress.

## Charter amendment (concrete text)

The following text replaces section "Cross-Plane Contract Rules" in
`docs/ARCHITECTURE_DIAGRAM.md`:

> ### Cross-Plane Contract Rules
>
> 1. **API-only access** — Frontend never imports server-side packages
>    from lower layers. All cross-plane data flows through HTTP/RPC.
> 2. **No direct DB access** — Frontend has zero database connections.
> 3. **Auth cookie only** — Authentication flows through HTTP-only
>    cookies issued by auth-core. The frontend never stores raw OAuth
>    tokens (refresh tokens live exclusively in auth-core; see ADR 0002
>    and velion-gap.md §2.1).
> 4. **Environment-driven URLs** — Backend URLs come from environment
>    variables. No hard-coded service hosts.
> 5. **Velion proxy as L5 ingress** — velion's `src/app/api/*` route
>    handlers are the canonical L5 ingress for the Frontend Plane.
>    They validate sessions (`control-plane-auth.ts`), mint internal
>    auth headers, propagate correlation IDs, and forward to L1–L4
>    cores over the shared Docker network. New proxy routes must use
>    the shared helper.
> 6. **Convex-gateway scope** — `convex-gateway` is reserved for
>    WebSocket fan-out of reactive workspace data (Application Plane,
>    Layer 5). It does not mediate REST traffic for L1–L4.
> 7. **Second-frontend trigger** — when a second frontend (mobile,
>    admin console, public web) ships, supersede ADR 0003 with a new
>    ADR that adopts a shared gateway pattern.

## Implementation plan

Most of the work is already done — this ADR ratifies the existing state.
Outstanding tasks:

1. **Amend `docs/ARCHITECTURE_DIAGRAM.md`** with the charter text in the
   previous section. Cite ADR 0003 inline.
2. **Update `velion-gap.md` §1**: replace the "Charter rule" callout with
   a reference to the amended charter. Drop the "highest-impact open
   question" wording.
3. **Lint**: add an ESLint rule (or grep-based pre-commit) that flags
   any new `src/app/api/*/route.ts` file that imports `fetch` directly
   instead of going through `control-plane-auth.ts`. Prevents drift.
4. **Track operational gaps** as separate velion-gap.md entries:
   - rate-limiting at velion proxy layer (TBD, low-priority until
     abuse signals appear)
   - audit log shipping for proxy traffic (medium-priority)
   - DLP scanning (out of scope for current product phase)
5. **No code changes** to the proxies themselves under this ADR. They
   already match the policy.

## Implementation notes

- **What about microfrontends?** If a small standalone widget or
  embedded component is required (e.g. a billing portal), prefer
  hosting it as another route in velion (`/billing-portal/...`) rather
  than as a separate app. Same proxy layer, same auth helper, no new
  ingress to operate.
- **What about server-to-server?** L1–L4 services already talk
  directly to each other over the shared Docker network using
  internal API keys (auth-core ↔ user-core ↔ org-core, etc.). This
  ADR does not change that. The L5 boundary applies to the
  *frontend* path only.
- **What about the Application Plane's own services calling Control
  Plane?** Same as above — direct service-to-service over the shared
  network. convex-core calls auth-core's JWKS for JWT validation
  today; that pattern continues unchanged.

## References

- `docs/ARCHITECTURE_DIAGRAM.md` (charter to be amended per §"Charter
  amendment" above)
- `velion-gap.md` G17 entry
- `velion-gap.md` §5.1 L5 boundary policy (charter vs reality)
- `src/app/api/_lib/control-plane-auth.ts` (the shared helper that
  embodies guardrail #1)
- `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md` (defines
  convex-gateway's scope; this ADR keeps that scope WS-only)
