# Phase A implementation plan — multi-tenant trust + product surface (2026-05-20)

Phase A as scoped from the product-gap analysis:

1. **Wave 3** — close gap-data §15 (multi-tenant trust)
2. **First-run onboarding wizard** — complete the partial scaffolding
3. **Admin / usage / billing / API keys / audit log** UIs — surface existing backends
4. **Inbox MVP** — wire Zammad + support-worker into the existing `/inbox` page

Plan written after a session-wide audit. Many Phase A items are **completion
work** on existing scaffolding rather than ground-up builds; the plan calls
each one out explicitly.

---

## A1. Wave 3 — multi-tenant trust enforcement

**Why it's first**: every other Phase A item is multi-tenant-unsafe today
because `X-Org-ID` is trusted blindly across plane boundaries. Until A1
lands, surfacing admin / usage / billing data per-org leaks across tenants.

### Current state (verified)
- `auth-core/src/auth/model-plane-token.controller.ts` already mints
  short-lived RS256 JWTs signed with the same JWKS as Convex
  (`/api/convex-auth/jwks`).
- `auth-core/src/grpc/auth-grpc.controller.ts` validates JWTs for gRPC calls
  but uses `JWT_SECRET` env var (HS256) — needs migration to JWKS-verified
  RS256 for consistency with the model-plane token.
- `model-gateway` (Rust) honors `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` and
  accepts the dev-bypass bearer; production should verify against JWKS.
- **Data Plane v2** — `X-Org-ID` is read but never verified.
- **Quarry-control / quarry-edge** — `QUARRY_CONTROL_API_KEY` is the only
  guard; no per-org claim resolution.
- **org-core** has the canonical membership data but no shared client.

### Target architecture
Every cross-plane request carries `Authorization: Bearer <JWT>` minted by
auth-core, with claims:
```
{
  "iss": "auth-core",
  "aud": <plane-specific>,
  "sub": <user_id>,
  "org_id": <active org>,
  "roles": ["owner" | "admin" | "member"],
  "scopes": ["read:documents", "write:tickets", ...],
  "exp": <60s-300s>
}
```
- Velion mints the JWT via auth-core on every server-side fetch
  (cache for the duration of the request; never re-use across requests).
- Each plane validates the JWT against the JWKS at
  `http://auth-core:3011/api/convex-auth/jwks` (cached 15 min).
- The plane derives `org_id` from JWT, NEVER from `X-Org-ID` header.
- A shared `AuthContext` middleware is published as:
  - Go: `pkg/authctx` (control-plane workspace) + reused via go workspace
  - Rust: `mp-authctx` crate (model-plane workspace)
  - TS: `@coresystem/auth-context` (velion local package or auth-core
    library export)

### Concrete deliverables
**A1.1 — Auth contracts**
- [ ] `auth-core`: publish JWKS URL + JWT issuance docs in `auth-plan.md`.
- [ ] `auth-core`: add `/api/auth/internal/mint-plane-token?audience=...`
  endpoint that velion server-side calls on every cross-plane fetch (60s exp).
- [ ] Define `AuthClaims` shape as a shared protobuf message under
  `apps/Model Plane/proto/control/v1/auth.proto` (re-use across planes).

**A1.2 — Go middleware (`pkg/authctx`)**
- [ ] Verifies JWT via cached JWKS.
- [ ] Resolves `org_id` from claims into a request-scoped `AuthContext`.
- [ ] Calls `org-core` only to refresh membership when the JWT `org_id`
  is older than the membership cache TTL (5 min).
- [ ] HTTP middleware: chi/gin-style, sets `r.Context()` value.
- [ ] gRPC interceptor: same, sets `ctx` value.
- [ ] Wire into `documents-api`, `wiki-store`, `data-orchestrator`,
  `data-quality`, `quarry-control`, `quarry-edge`, `org-core`,
  `user-core`, `billing-core`, `session-core`, `imports-api`,
  `integration-api`, `finspo-api`.

**A1.3 — Rust middleware (`mp-authctx` crate)**
- [ ] tower-http layer that validates JWT, exposes `AuthCtx` extension.
- [ ] Wire into `model-gateway`, `session-core`, `inference-core`,
  `execution-core`, `retrieval-engine`, `index-engine`,
  `embedding-engine`, `graph-index`.

**A1.4 — TS helper (`@coresystem/auth-context`)**
- [ ] `mintPlaneToken(audience: "model" | "data" | "quarry" | ...)`:
  Promise<string> — server-side helper called from velion route handlers.
- [ ] Auto-attaches to `fetch` via a wrapper.
- [ ] Wire into every velion `app/api/**/route.ts` that does a server-side
  cross-plane fetch.

**A1.5 — Cost-core integration**
- [ ] Each plane's middleware logs `(user_id, org_id, plane, op, tokens,
  bytes, cost_cents)` to NATS subject `velion.usage.v1.<plane>.<op>`.
- [ ] `cost-core` aggregates by org_id and exposes `GET /v1/usage?org_id=...`.

**A1.6 — Audit log**
- [ ] Same NATS subject pattern → `velion.audit.v1.<plane>.<event>`.
- [ ] New `audit-core` (Go, port 8087) tiny service that subscribes,
  persists to Postgres, exposes `GET /v1/audit?org_id=...&since=...`.
  (Lives in Control Plane.)

### Estimate: 2–3 weeks

---

## A2. First-run onboarding wizard

### Current state (verified)
Pages already exist under `(onboarding)/onboarding/`:
- `organization/page.tsx` — create org
- `profile/page.tsx` — user profile
- `team/page.tsx` — invite teammates
- `website/page.tsx` — first data source (web crawl seed?)
- `connect/page.tsx` — connect external integrations
- `complete/page.tsx` — wrap-up

Also: `onboarding/connect/callback/page.tsx` for OAuth return.

### What needs to land
**A2.1 — End-to-end functional pass**
- [ ] Audit each page: data flow, server-side mutations, error states,
  redirect-on-completion. Many likely still TODO.
- [ ] Wire the chosen "first data source" (website crawl seed) to actually
  trigger a Quarry crawl job via `POST /api/ingestion/crawl`.
- [ ] Wire team invites to a `user-core` membership add + email notification
  via notification-core.
- [ ] On `complete/page.tsx`, mint a first **embed snippet** (HTML + JS
  pointing to `/api/embed/<agentId>/{config,stream}`) and copy-to-clipboard.

**A2.2 — Auth gating**
- [ ] Add middleware that forces new users into `/onboarding/*` until
  `org_id` + `member_count >= 1` + `data_source_count >= 1` +
  `agent_count >= 1`.
- [ ] Skip-onboarding link for power users (sets a flag in user-core
  `onboarding_dismissed_at`).

**A2.3 — First-agent default**
- [ ] After website crawl indexes, auto-create a default "Support Agent"
  via Model Plane `capability-core` with the right tools wired
  (retrieval over the freshly-crawled docs + zammad handoff).

### Estimate: 1 week (mostly completion work)

---

## A3. Admin + usage + billing + API keys + audit log UIs

### Current state (verified)
Pages already exist:
- `(admin)/admin/page.tsx` — admin landing
- `(admin)/admin/organizations/page.tsx` — list orgs
- `(admin)/admin/users/page.tsx` — list users
- `(admin)/admin/billing/page.tsx` — billing
- `(dashboard)/settings/billing/page.tsx` — per-org billing
- `(dashboard)/settings/members/page.tsx` — per-org members
- `(dashboard)/settings/integrations/page.tsx` — connector mgmt
- `(dashboard)/settings/notifications/page.tsx`
- `(dashboard)/settings/security/page.tsx`
- `(dashboard)/settings/permissions/page.tsx`
- `(dashboard)/settings/privacy/page.tsx`
- `(dashboard)/settings/advanced/page.tsx`

### What needs to land
**A3.1 — Org admin (`/settings/*`)**
- [ ] `members`: list + invite + role-edit + remove, hitting `user-core`
  + `org-core`. Roles enforced by Wave 3 JWT claims.
- [ ] `integrations`: list connections from Nango via `integration-api`;
  per-connection status, last-sync, errors.
- [ ] `permissions`: surface `org-core` entitlements + quotas
  (read-only first; write later).
- [ ] `security`: 2FA toggle, sessions list (delete other sessions),
  password change.
- [ ] `privacy`: ZDR opt-in, data-deletion request, data-export request.

**A3.2 — Billing (`/settings/billing` + `/admin/billing`)**
- [ ] Show current plan + usage progress bars (this week / this month).
- [ ] "Upgrade" button → Lago checkout link (`lago-api:3000/v1/...`
  via billing-core proxy).
- [ ] Invoice list (last 12) + download PDF (gotenberg-rendered by
  `lago-pdf`).
- [ ] Payment method management.

**A3.3 — Usage dashboard (`/settings/usage` — new page)**
- [ ] Tokens / runs / cost / per-agent breakdown — from cost-core.
- [ ] Per-plane: Model, Data, Quarry, Application costs.
- [ ] CSV export.

**A3.4 — API keys (`/settings/api-keys` — new page)**
- [ ] Mint scoped keys via `auth-core` `/api/auth/api-keys` (needs new
  endpoint; existing scaffolding only has Better Auth session tokens).
- [ ] Key shown once on creation; only prefix + last-used after.
- [ ] Revoke + rotate.

**A3.5 — Audit log (`/settings/audit-log` — new page)**
- [ ] Query `audit-core` (new from A1.6).
- [ ] Filter by user / event type / time range.
- [ ] CSV export.

**A3.6 — Platform admin (`/admin/*`)**
- [ ] `organizations`: list, suspend, delete (impersonation handled
  separately under feature flag).
- [ ] `users`: list, suspend, role-grant.
- [ ] New `/admin/system-health` page surfacing each plane's `/healthz`
  + key metrics from OTEL collector.

### Estimate: 2 weeks (mostly completion + 3 new pages)

---

## A4. Inbox MVP

### Current state (verified)
- `(dashboard)/inbox/[[...slug]]/page.tsx` exists
- `grep -i zammad` across `velion/src/` returns **zero hits** — Zammad
  integration in velion is not wired
- Backend: `support-worker` already runs Temporal workflows
  (triage, sla, csat) consuming Zammad webhook events via NATS
- Zammad container exists at `zammad-foundation/` in Application Plane

### What needs to land
**A4.1 — Velion proxy for Zammad REST**
- [ ] New `app/api/tickets/route.ts` (and `[id]/route.ts`,
  `[id]/articles/route.ts`) that proxy to Zammad's REST with
  `ZAMMAD_API_TOKEN`.
- [ ] Server-side only; never expose Zammad token to the browser.
- [ ] Wave 3 JWT enforces org-scoping (Zammad ticket→org mapping
  stored in `org-core`).

**A4.2 — Inbox UI**
- [ ] Three-pane layout: ticket list (left) · selected ticket
  conversation (center) · ticket metadata + customer panel (right).
- [ ] Live updates via Convex subscription on `tickets` table (mirrored
  from Zammad via NATS).
- [ ] Compose reply with: rich text, file attachment, internal note vs
  public reply toggle.
- [ ] Triage signals from `support-worker`: classify result, suggested
  reply, suggested macros, SLA deadline.

**A4.3 — Convex mirror**
- [ ] Add a `tickets` table in convex-core with subset of Zammad fields.
- [ ] `convex-subscriber` already listens on
  `velion.support.ticket.{created,updated,assigned}` and
  `velion.support.article.added` — extend it to upsert into the new table.
- [ ] Add ID mapping `(zammad_ticket_id ↔ convex_ticket_id ↔ org_id)`.

**A4.4 — Reply path**
- [ ] Velion `POST /api/tickets/:id/reply` → proxies to Zammad
  `POST /api/v1/ticket_articles` → emits NATS
  `velion.support.article.added` → support-worker sees it → SLA timer
  resets if external.

### Estimate: 1.5 weeks

---

## Suggested execution order

| Sprint | Deliverable | Unblocks |
|---|---|---|
| 1a | A1.1 + A1.4 (TS helper) + A1.2 stub in `documents-api` | every other A1 piece |
| 1b | A1.6 audit-core + A1.5 cost-core hooks | A3.5 + A3.3 |
| 2a | A2 onboarding completion | first end-to-end user flow |
| 2b | A3.2 billing + A3.3 usage UI | revenue-side flow |
| 3a | A4 inbox MVP | support flow |
| 3b | A3.1 settings polish + A3.4 API keys + A3.6 platform admin | full operator surface |
| 4 | A1.3 Rust middleware rollout across model-plane services | hardening |

Total elapsed: **4–5 weeks** of focused work.

## Acceptance criteria for Phase A "done"

- A single new user signs up → creates an org → invites a teammate →
  connects a website source → first agent is auto-created → embed snippet
  works → first ticket appears in the inbox → reply sent → SLA timer
  observed in real time. **All without admin manual intervention.**
- Two tenants concurrently: every cross-plane call carries an auth-core
  JWT; querying `?org_id=<other>` returns 403; cost-core attributes spend
  correctly; audit log shows the cross-tenant attempt as `DENIED`.
- Billing checkout returns the user to velion with the new plan reflected
  in `/settings/billing` within 5 seconds.
- All UIs render with the existing design system (no new dependencies).

## Out-of-scope (deferred per user annotations)
- Embed widget v1 with hand-off / CSAT / hours-of-operation (Phase D)
- Visual agent builder (Phase B)
- Help-center authoring (Phase B)
- Voice browser SDK (Phase D)
- White-label / custom domain (Phase D)
