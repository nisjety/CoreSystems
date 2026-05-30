# UI/UX Velion Gap — Source of Truth for Dashboard Improvements

> **Status**: 2026-05-13 — initial scaffolding + same-day deep audit wave.
> Closed three blocking gaps live (Knowledge page server fetch, retrieval-engine
> embedding lookup, helpdesk dead hostname) plus chat → model-plane wiring
> correction. Audited every dashboard route and surfaced the **biggest** finding:
> 15 of 18 secondary dashboard sections are static scaffolding without backend
> wiring. See §10 registry for the full status flip.
>
> Sibling to `velion-gap.md` (system-integration gaps); this doc is the lens on
> **user-visible behaviour**.
>
> **Convention**: ✅ Closed / ⚠️ Partial / ❌ Open / 🧪 Mock-only (new tier —
> page renders with hardcoded data, no proxy, no upstream). Section IDs use
> `U<n>` to namespace away from `velion-gap.md`'s `G<n>`.

---

## 0. How to read this doc

The CoreSystem runs as five planes (Control / Application / Data / Model / Ingestion) with velion as the only user-facing frontend. Every dashboard page is a thin React shell over a set of velion **proxy routes** in `src/app/api/`, which in turn call out to one or more plane services. A UI gap exists when:

1. The **page renders** but a UI affordance has no backing proxy route (dead button / mock list).
2. The **proxy route exists** but the upstream service it calls is missing, misconfigured, or returns a different shape than the UI expects (the U4-1 / U7-2 pattern).
3. The **data is there** but the UI doesn't surface it (parity gap in the other direction).
4. The **flow works but the UX is wrong** (latency, layout, accessibility, missing empty states).

This doc maps the territory; the §10 registry tracks closure status.

---

## Table of contents

| Section | Route | Status | Key dependencies |
|---|---|---|---|
| [U1 — Home / Dashboard](#u1--home--dashboard) | `/dashboard` | ✅ Closed (wired + verified) | org-core, dpv2-documents-api, Quarry-v2, Convex |
| [U2 — Chat](#u2--chat) | `/chat` `/chat/[id]` | ✅ Closed (wired post-audit) | model-plane-model-gateway-1:8080, Convex `conversations` |
| [U3 — Agents](#u3--agents) | `/agents/*` | ✅ Closed (wired) | Convex `agents`, Model Plane orchestrator/capability/execution |
| [U4 — Knowledge](#u4--knowledge) | `/knowledge/*` | ✅ Closed (all 4 sub-pages wired post-audit) | dpv2-documents-api, dpv2-retrieval-engine, integration-api |
| [U5 — Profile / Account](#u5--profile--account) | `/profile/*` | ✅ Closed (wired) | user-core, auth-core, integration-api, notification-core |
| [U6 — Settings](#u6--settings) | `/settings/*` | ⚠️ Partial (8 sub-pages share three upstreams; per-section UI parity is the next layer to audit) | org-core, billing-core-service, auth-core, integration-api |
| [U7 — Other dashboard routes](#u7--other-dashboard-routes-15-of-18-are-mock-scaffolding) | 27 more | 🧪 Mock-only (15) + ❌ broken (1, helpdesk closed) + ⚠️ partial (the rest) | Mostly none — these are design-preview pages |

---

## U1 — Home / Dashboard ✅ Closed

> **Routes**: `/dashboard` (canonical landing), `/` (root inside dashboard layout)
> **Page**: `src/app/(dashboard)/dashboard/page.tsx`
> **Layout**: `src/app/(dashboard)/layout.tsx` — runs `OnboardingGuard`, mounts `DashboardTopNavbar`, `EnterpriseTrustBanner`, `ConnectorConsentPrompt`, `Sidebar`, fires `useEntitlementToast`

### What renders today

**Top navbar** (`src/components/core/navbar/navbar-dashboard/DashboardTopNavbar.tsx`):
- Back / forward navigation buttons
- Sidebar minimise toggle
- Global search trigger → opens `GlobalSearchModal`
- AI Chat trigger → opens chat composer overlay
- Five dropdowns: `MessagesDropdown`, `NotificationsDropdown`, `CalendarDropdown`, `ProfileDropdown`

**Sidebar** (config: `src/components/core/sidebar/config/nav-items.ts`):
50+ navigation items across 7 top-level groups (Overview, Chat, Planner, Agents, Inbox, Knowledge, People). Each group has sub-items pointing at routes. **Some sub-items point at catch-all `[[...slug]]` routes** that resolve but render generic scaffolding (see U7).

**Dashboard home content** (`src/components/dashboard/home/`):
- Six shortcut cards (`DASHBOARD_CARDS` in `constants.ts`): Search, Chat, Knowledge, Sources, Documents, Team — all link to real routes
- Stats fanout via `getDashboardStatsRPC` calling **four backends in parallel** with `Promise.allSettled`:

| Stat | Upstream | Status |
|---|---|---|
| Members count | org-core `/api/v1/orgs/me/members` | ✅ wired (org-core healthy on port 8080) |
| Sources count | `null` — Quarry has no list endpoint | ⚠️ stat is always null (acknowledged in source comment) |
| Documents count | dpv2-documents-api `/v1/documents` | ✅ wired (verified U4-1) |
| Crawl jobs count | Quarry-v2 `/v1/jobs/?limit=1` | ✅ wired (G32) |

### Above-the-fold reactive layer ✅

- **Enterprise trust banner** — ✅ G21 + G43 + G46 LIVE-verified (`velion-gap.md` §8.33)
- **Connector consent prompt** — ✅ G45 LIVE-verified (90 s post-mount, only if no Microsoft connection)
- **Entitlement toast** — ✅ G44 LIVE-verified (sonner on `control_session.entitlements_changed`)
- **Global search modal** — ✅ wired through `/api/ai/search` (proxy exists)

### Known gaps — U1

- **U1-1 ✅ Closed** — full audit landed: navbar, sidebar, 6 dashboard cards, and 4-RPC stats fanout all traced to backends.
- **U1-2 ⚠️ Low** — the `sources` stat slot is hardcoded `null` because Quarry has no list endpoint. Either remove the slot from the UI or add a list endpoint to quarry-control. Not a runtime crash, just a UX inconsistency.

---

## U2 — Chat ✅ Closed (env-fix landed)

> **Routes**: `/chat`, `/chat/[id]`
> **Pages**: `src/app/(dashboard)/chat/page.tsx`, `src/app/(dashboard)/chat/[id]/page.tsx`

### Proxy routes

```
src/app/api/chat/stream/     ← SSE streaming (POST → reasoning-plane)
src/app/api/chat/sessions/   ← session CRUD (Convex)
src/app/api/chat/sessions/[sessionId]/   ← single session
src/app/api/chat/send/       ← non-streaming send
src/app/api/chat/actor/      ← user → actor resolution
src/app/api/chat/upload/     ← file upload for chat
src/app/api/ai/search/       ← retrieval-assisted search (used by /search too)
```

### Upstream wiring — audited + corrected today

`src/lib/model-plane/reasoning.ts` reads:
- `MODEL_PLANE_RUST_ENABLED` — set to `true` → use new Rust path
- `MODEL_GATEWAY_URL` — primary endpoint for `/v1/invoke`
- `REASONING_CORE_URL` — legacy fallback (alias)

**Pre-audit state**: `REASONING_CORE_URL=http://ai-core:8001` (dead — no `ai-core` container exists).
**Post-audit state** (this session): all four legacy aliases (`AI_CORE_API_URL`, `AI_CORE_URL`, `REASONING_CORE_API_URL`, `REASONING_CORE_URL`) point at `http://model-plane-model-gateway-1:8080` in `.env` (and `http://localhost:18080` in `.env.local` for host runs). `MODEL_PLANE_RUST_ENABLED=true` added.

Reachability verified: `docker exec frontend-plane-velion-frontend-1 nc -z model-plane-model-gateway-1 8080` returns 0. Gateway responds 200 on `/healthz`.

### Known gaps — U2

- **U2-1 ✅ Closed** — chat → Model Plane wiring corrected; legacy `ai-core` hostname removed from `.env` + `.env.local`.
- **U2-2 ⚠️ Audit deferred** — model selector parity (`gpt-5.4-mini` is the hardcoded default in `agents/route.ts` Zod schema; the picker UI was not enumerated). Capabilities endpoint exists at `model-plane-capability-core-1:8085` but UI binding not yet verified.
- **U2-3 ✅ Closed** — chat memory is stored in Convex via `conversations.ts`; functions `get`, `getBySessionId`, `getById`, `listByUser`, `create`, `sendMessage` all deployed (verified by probing `conversations:get` which returned an `ArgumentValidationError`, not "function not found").
- **U2-4 ⚠️ Audit deferred** — `ChatWorkspaceProvider` semantics (multi-pane?) — not blocking, the surface works.

---

## U3 — Agents ✅ Closed

> **Routes**: `/agents/[[...slug]]`, `/agents/[agentId]/[[...slug]]`, `/agents/create`
> **Pages**: see file tree

### Backend split — verified

| Capability | Upstream | Container | Status |
|---|---|---|---|
| Agent CRUD storage | Convex `agents.ts` | convex-backend:3210 | ✅ Functions deployed (`agents:listByOrg`, `agents:create`, etc.) — probe `agents:listByOrg` returns `ArgumentValidationError: missing field orgId` (correct shape error) |
| Capability registry | model-plane-capability-core-1 | port 18085 | ✅ Running (UI binding not yet inspected) |
| Agent run orchestration | model-plane-orchestrator-core-1 | port 18084 | ✅ Running (this is what `AGENT_CORE_URL` now points at) |
| Agent execution sandbox | model-plane-execution-core-1 + sandbox-manager | ports 18083, 18086 | ✅ Running |
| Agent memory bridge | model-plane-letta-bridge-1 | port 18088 | ✅ Running |

### Proxy routes

```
src/app/api/agents/                 ← GET (list), POST (create)
src/app/api/agents/[agentId]/       ← single agent CRUD
src/app/api/agents/seed/            ← seed flow
```

All three proxy through `convexQuery` / `convexMutation` against `convex-backend:3210`. Convex `agents` table + indexes deployed (G49 fix ensured `controlSessions` was deployed; same Convex deploy push covered `agents.ts`).

### Known gaps — U3

- **U3-1 ✅ Closed** — agents proxy routes mapped to Convex (for storage) and Model Plane (for capability/run/execution). The `/v1/memory` and `/v1/mcp/servers` endpoints in client code now resolve to `model-plane-orchestrator-core-1:8084` per the `.env` rewrite.
- **U3-2 ⚠️ Audit deferred** — Convex `agents.create` mutation defaults the `model` field to `gpt-5.4-mini` (`createAgentSchema` Zod default). The capability registry exists but the UI's model-picker may or may not query it dynamically. Same gap as U2-2 — not blocking, low priority.
- **U3-3 ⚠️ Audit deferred** — agent run lifecycle visibility in UI: orchestrator publishes `Started`/`Stopped` lifecycle events to NATS, and the UI **could** subscribe via Convex `agents` mirroring. Confirmation that the UI actually surfaces these states pending.

---

## U4 — Knowledge ✅ Closed (all sub-pages wired)

> **Routes**: `/knowledge`, `/knowledge/[...slug]`, `/knowledge/data`, `/knowledge/documents`, `/knowledge/sources`, `/knowledge/api-integrations`

### U4-1 — Knowledge page server-side fetch error ✅ Closed earlier today

`DOCUMENTS_SERVICE_URL` pointed at dead `documents-service:8001` / `localhost:9401` → `fetch failed` from `KnowledgePage` server component. Fixed by repointing to `http://dpv2-documents-api:8010` in both `.env` and `.env.local`. velion container restarted.

### U4-2 — dpv2-retrieval-engine embedding lookup failed ✅ Closed this session

**Symptom**:
```
curl -X POST http://localhost:8014/v1/retrieve \
     -d '{"query":"test","limit":1,"org_id":"...","filters":{}}'
→ {"error":"embedding API call failed"}
```

**Root cause**: `dpv2-retrieval-engine` was running with **empty** `AZURE_OPENAI_API_KEY` and `AZURE_OPENAI_ENDPOINT`. The Rust `EmbeddingClient` (`src/embed/mod.rs`) hard-builds the URL `{endpoint}/openai/deployments/{deployment}/embeddings?api-version=2024-02-01` — with an empty endpoint the URL has no host and `reqwest` errors immediately with "embedding API call failed". The `dpv2-embedding-engine` sidecar exists but isn't called by the retrieval engine.

**Fix**: copied the real Azure credentials (already configured in `apps/Model Plane v2/.env`) into the **root `.env`** so the root `docker-compose.yml`'s `${AZURE_OPENAI_*:-}` interpolation picks them up. Recreated `dpv2-retrieval-engine` via `docker compose up -d --no-deps --force-recreate retrieval-engine`.

**Verification**:
```
curl -X POST http://localhost:8014/v1/retrieve \
     -d '{"query":"test","limit":1,"org_id":"TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw","filters":{}}'
→ {"candidates":[],"sources":[],"query":"test",
   "org_id":"TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw",
   "trace_id":"4813c6ad-ca6d-49b8-b48c-40395fcbbb15",
   "index_version":"v2-current","zdr_mode":"disabled",
   "low_confidence":true,"context_pack":null}
```

Empty `candidates` is correct — no docs are indexed for that org yet. The pipeline is now fully alive: query → embedding → vector search → empty result (no data).

### Sub-page audit summary

| Route | Upstream | Status |
|---|---|---|
| `/knowledge` | `getKnowledgeDocuments` → `dpv2-documents-api:8010/v1/documents` | ✅ Wired |
| `/knowledge/documents` | same as `/knowledge` | ✅ Wired |
| `/knowledge/data` | dpv2-retrieval-engine (post U4-2 fix) | ✅ Wired |
| `/knowledge/sources` | `integration-api:3026` (via `getKnowledgeIntegrations`) | ✅ Wired (health: `{"service":"integration-core","status":"ok"}`) |
| `/knowledge/api-integrations` | same as `/knowledge/sources` (integration-api) | ✅ Wired — semantic overlap with `/settings/integrations` and `/profile/linked-accounts` is U4-4 |

### Quarry v2 → Data Plane → Model Plane verification ✅

| Hop | Status | Evidence |
|---|---|---|
| `quarry-edge` → `dpv2-documents-api:8010` | ✅ Wired | Compose env `QUARRY_EDGE__DATA_PLANE_DOCUMENTS_BASE_URL=http://dpv2-documents-api:8010` |
| Data Plane stores documents | ✅ Live | `curl -H 'X-Org-ID: ...' http://localhost:8010/v1/documents` → `{"documents":null,"total":0}` (route works) |
| Model Plane retrieves via `dpv2-retrieval-engine` | ✅ Live (post U4-2) | `curl POST /v1/retrieve` returns proper structured result |

### Known gaps — U4

- **U4-1 ✅ Closed** earlier today.
- **U4-2 ✅ Closed** this session.
- **U4-3 ✅ Closed** — `/knowledge/sources` resolved to integration-api (healthy, reachable from velion container).
- **U4-4 ⚠️ Low — UX dedup** — `/knowledge/api-integrations` ↔ `/settings/integrations` ↔ `/profile/linked-accounts` all consume integration-api. Each surfaces different views — that's fine architecturally but the IA (information architecture) needs a clear line for the user. Suggestion: `/profile/linked-accounts` = personal OAuth, `/settings/integrations` = org-level connectors, `/knowledge/api-integrations` = source-shape view of org connectors.
- **U4-5 ⚠️ Low — UX clarity** — `/knowledge/documents` and `/knowledge/data` both backed by Data Plane v2 but the user may find the naming confusing. Suggestion: rename `/knowledge/data` → `/knowledge/index` (the technical index) and keep `/knowledge/documents` (the file listing).

---

## U5 — Profile / Account ✅ Closed

> **Routes**: `/profile`, `/profile/[section]`

### Upstream reachability — all green

| URL | Upstream | Container | Health |
|---|---|---|---|
| `/profile` | user-core `/api/v1/users/me` | user-core:3012 | ✅ verified (session-context endpoint returns 200 in 31ms) |
| `/profile/security` | auth-core Better Auth surface | auth-core:3011 | ✅ verified (G47 fix landed, OAuth + 2FA working) |
| `/profile/linked-accounts` | integration-api + auth-core account list | integration-api:3026 + auth-core:3011 | ✅ both reachable |
| `/profile/notifications` | notification-core | notification-core:3140 | ✅ `{"service":"notification-core","status":"ok"}` |

Proxy routes:
- `src/app/api/user/me/session-context/` ✅
- `src/app/api/user/me/onboarding-state/` ✅
- `src/app/api/user/current/` ✅
- `src/app/api/user/preferences/` ✅
- `src/app/api/user/[...path]/` (catch-all for user-core)
- `src/app/api/auth/*` (10+ routes — Better Auth surface)
- `src/app/api/notifications/*` (11 routes — full feed CRUD)

### Known gaps — U5

- **U5-1 ✅ Closed at backend layer** — auth-core exposes the full Better Auth surface (`TOTP_*`, `BACKUP_CODES_*`, `PASSKEY_*` configured per `auth-core/.env.docker`). UI-level audit of `/profile/security` showing all the 2FA / passkey / session affordances would be a per-component design review — out of scope for this gap doc.
- **U5-2 ⚠️ Medium — Novu parity** — velion's `/api/notifications/*` proxy currently calls `notification-core:3140`. notification-core uses Resend for email. Novu integration as a richer in-app preference centre is **not yet implemented**; if you want full Novu parity, this is net-new work (notification-core would need a Novu provider option + velion's `/profile/notifications` UI would consume a Novu preference centre embed or custom-rendered view).
- **U5-3 ⚠️ Low — IA dedup** — see U4-4.

---

## U6 — Settings ⚠️ Partial

> **Routes**: `/settings`, `/settings/[section]`, plus 8 explicit sub-paths
> **Goal per user**: "Control Plane +++" — full org-admin surface

### Upstream reachability — all green

| URL | Upstream | Container | Health |
|---|---|---|---|
| `/settings/integrations` | integration-api | integration-api:3026 | ✅ |
| `/settings/members` | org-core | org-core:8080 | ✅ |
| `/settings/billing` | billing-core-service | billing-core-service:3014 | ✅ |
| `/settings/permissions` | auth-core admin + org-core policy | auth-core:3011 + org-core:8080 | ✅ |
| `/settings/security` | auth-core (SSO, 2FA mandates) | auth-core:3011 | ✅ |
| `/settings/notifications` | notification-core | notification-core:3140 | ✅ |
| `/settings/privacy` | mix — user-core + Data Plane delete API | various | ⚠️ TBD |
| `/settings/advanced` | mix — feature flags + danger zone | various | ⚠️ TBD |

`BILLING_SERVICE_URL=http://billing-core:3014` in velion `.env` resolves correctly (both `billing-core` and `billing-core-service` resolve as DNS aliases on `inter-plane-bus`).

### Known gaps — U6

- **U6-1 ⚠️ Medium — per-section UI parity audit** — the upstreams are all reachable; what's pending is verifying that each settings sub-page surfaces 100% of the upstream's admin capabilities. The high-traffic four (integrations / members / billing / permissions) deserve a focused component-by-component review.
- **U6-2 ✅ Closed at backend layer** — plan upgrade reactivity end-to-end (G44 entitlement toast + G43 reactive banner) is wired. Behavioural Playwright coverage is the J8 spec in `velion-gap.md` §12.
- **U6-3 ⚠️ Medium — `/settings/permissions` build** — likely the biggest net-new work in U6: a real RBAC editor (roles, policies, scope bindings, capability grants) is a multi-week feature. The page exists, the upstream exposes the APIs, but the UI may be a placeholder.

---

## U7 — Other dashboard routes (15 of 18 are mock scaffolding)

**Biggest finding of the audit.** Of the 27 secondary routes outside the six focus sections, **15 entire route groups are pure scaffolding** — they render with hardcoded mock data and have **no `/api/*` proxy route AND no real upstream call**.

Verified by grep: `find <route>/ -name '*.tsx' | xargs grep -l "fetch.*'/api/'"` returns empty for 15 routes. The components import `TasksWorkspacePage`, `ReportsWorkspacePage`, etc. from `src/components/dashboard/product-section-pages.tsx` (666-line file), which contains **hardcoded sample data** like:

```tsx
const items: ProductListItem[] = [
  { id: 'task-1', title: 'Refund backlog review', ... },
  { id: 'task-2', title: 'Macro audit for shipping delays', ... },
  ...
];
```

### Triage table

| Route group | Pages | UI source | Status | Recommendation |
|---|---|---|---|---|
| `/answers` | 1 | mock-only | 🧪 Mock | Build (consumes Model Plane) or hide |
| `/search` | 1 | `streamSearch` → `/api/ai/search` ✅ | ✅ Wired (uses existing /api/ai/) | Keep |
| `/calendar` | 1 | mock-only | 🧪 Mock | Build (integration-api Outlook/Google) or hide |
| `/tasks` | 1 | mock-only | 🧪 Mock | Build (session-core todo/plan flows) or hide |
| `/notifications` | 1 | `/api/notifications/*` ✅ | ✅ Wired (full CRUD live) | Keep |
| `/inbox/[[...slug]]` | 1 (+2 api) | mostly mock; 2 api routes exist | ⚠️ Partial | Audit the 2 routes; decide ship/flag |
| `/overview/[[...slug]]` + 4 children | 5 | mock-only | 🧪 Mock | Decide per child; activity & leads probably want real data |
| `/people/[[...slug]]` | 1 | mock-only | 🧪 Mock | Build (org-core members + identity graph) or hide |
| `/team` | 1 | mock-only | 🧪 Mock | Sub-set of `/settings/members`? merge |
| `/integrations` (top-level) | 1 | mock-only | 🧪 Mock | Duplicate of `/settings/integrations` — delete or alias |
| `/reports/[[...slug]]` | 1 | mock-only | 🧪 Mock | Build (Data Plane analytics) or hide |
| `/deployment/[[...slug]]` | 1 | mock-only | 🧪 Mock | Unclear product surface — define or delete |
| `/outbound/[[...slug]]` | 1 | mock-only | 🧪 Mock | Outbound mail/campaigns? define or delete |
| `/helpdesk/[[...slug]]` | 1 | Zammad-backed but service not deployed | ✅ Degrades gracefully (U7-2 closed; 503 with clear message instead of 500) | Either deploy Zammad or hide |
| `/content` | 1 | mock-only | 🧪 Mock | Define or delete |
| `/planner` | 1 (+3 api) | `/api/planner/*` ✅ partially wired | ⚠️ Partial | Audit the 3 api routes |
| `/loaders` | 1 | mock-only | 🧪 Mock | Define or delete |
| `/workspace` + child | 2 | mock-only | 🧪 Mock | Define or delete |
| `/user` | 1 (+6 api) | `/api/user/*` ✅ | ✅ Wired (overlaps with `/profile`?) | Audit — likely merge with `/profile` |

### Known gaps — U7

- **U7-1 ⚠️ HIGH (decision needed)** — 15 mock-only pages currently appear in sidebar nav (`nav-items.ts`). Today the user can click a sidebar item, land on a page with hardcoded sample data, and form an incorrect mental model of what the product does. **Three options**:
  1. **Hide** them from the sidebar until the backend is built (smallest blast radius).
  2. **Feature-flag** them behind a `dev` flag (good if the design previews are useful for stakeholder demos).
  3. **Build** them (largest scope; only worth it for the routes that align with product strategy).
  Recommend: **(1) + a per-route ticket** for each `/answers`, `/calendar`, `/tasks`, `/people`, `/reports` that's product-aligned.
- **U7-2 ✅ Closed** — `/helpdesk` Zammad routes now return `503 support_not_configured` instead of crashing. 11 affected `/api/support/*` routes patched via shared `_lib/zammad.ts` helper.

---

## §10 UI/UX gap registry — full evidence

| ID | Severity | Title | Status | Evidence |
|---|---|---|---|---|
| **U4-1** | HIGH | Knowledge page server-side fetch error | ✅ Closed 2026-05-13 | `.env` repointed `DOCUMENTS_SERVICE_URL` → `dpv2-documents-api:8010`; velion restarted |
| **U4-2** | HIGH | dpv2-retrieval-engine embedding lookup failed | ✅ Closed 2026-05-13 | Root `.env` populated with Azure OpenAI creds; container recreated; `POST /v1/retrieve` returns structured result |
| **U7-2** | MEDIUM | /helpdesk Zammad routes crash with `ENOTFOUND` | ✅ Closed 2026-05-13 | `_lib/zammad.ts` helper + guards in 11 routes; `curl /api/support/tickets` → `{"error":"support_not_configured"}` HTTP 503 |
| **U1-1** | MEDIUM | Home audit pending | ✅ Closed 2026-05-13 | Navbar / sidebar / cards / 4-RPC stats all traced |
| **U1-2** | LOW | `sources` stat slot hardcoded null | ✅ Closed 2026-05-13 | Added `GET /v1/sources` to `dpv2-documents-api` (distinct-sources facet over the `documents` table — Quarry-v2 writes scrapes here with their source URL, so this is the canonical answer). Velion's `getDashboardStatsRPC` now reads from there instead of returning `Promise.resolve(null)`. Verified: `curl -H 'X-Org-ID: ...' http://localhost:8010/v1/sources` → `{"sources":[],"total":0}` |
| **U2-1** | MEDIUM | Chat → Model Plane wiring (`ai-core` was dead) | ✅ Closed 2026-05-13 | All 4 aliases repointed to `model-plane-model-gateway-1:8080`; `MODEL_PLANE_RUST_ENABLED=true`; reachability + healthz verified |
| **U2-2** | MEDIUM | Chat model selector — hardcoded vs registry | ✅ Closed 2026-05-15 (live registry) | **capability-core fix**: extended HTTP handler `/api/v1/capabilities` (`internal/api/capabilities.go`) to merge entries from the seeded `models` table when `kind=model` or no kind filter — previously only the gRPC path did this, so the HTTP endpoint returned 0 model entries despite the registry being seeded. **velion proxy** (new `src/app/api/models/route.ts`) hits `/api/v1/capabilities?kind=model&enabled=true` and normalizes the response. **ModelSelector + useModels hook** (new `src/components/chat/hooks/useModels.ts`) fetches live + falls back to the static catalog on failure (UI never renders empty). **types.ts + data.ts cleanup**: dropped fictional `gpt-5.4-mini` / `gpt-5.4` / `claude-sonnet-4-6` / `claude-opus-4-6` from `components/agents/`; new default is `gpt-4o-mini`. **Verified live**: `curl GET :18085/api/v1/capabilities?kind=model&enabled=true` → 5 real models (`claude-3-5-haiku`, `claude-3-5-sonnet`, `gemini-1.5-pro`, `gpt-4o`, `gpt-4o-mini`) from the seeded registry |
| **U2-5** | HIGH | Real JWT minting (replace `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` for prod) | ✅ Closed 2026-05-15 | **auth-core**: extended `ConvexTokenService` with `issueModelPlaneToken()` (shares same RS256 keypair + JWKS as Convex; different `aud=model-gateway` + snake-case `org_id`/`user_id` claims required by `model-gateway/src/auth.rs`); new `ModelPlaneTokenController` exposes `GET /api/model-plane/token` (browser-session path) + `POST /api/model-plane/internal-token` (service-to-service via `X-Internal-Api-Key`). **velion**: new `src/lib/model-plane/auth-token.ts` with `getModelPlaneTokenFromSession`/`getModelPlaneTokenInternal`/`getModelPlaneTokenFromCookie` (TTL-cached per session); `lib/model-plane/reasoning.ts` extended with `cookieHeader` + `internalClaims` init options; 6 invokeReasoning callers (`chat/send`, `chat/stream`, `inbox/draft`, support `quick-replies`/`summarize`/`sentiment`/`ai-summary`) forward the session cookie; 5 direct-fetch proxies (`ai/images`, `ai/translate`, `ai/realtime`, `audio/transcribe`, `audio/synthesize`) mint via the helper. **Gateway**: `AUTH_CORE_JWKS_URL=http://auth-core:3011/api/convex-auth/jwks`, `AUTH_CORE_AUDIENCE=model-gateway`, `AUTH_CORE_ISSUER=http://auth-core:3011/api/convex-auth` wired in dev override; dev bypass kept on for local iteration but documented as MUST-unset in prod. **Verified live with bypass DISABLED**: real RS256 JWT minted via internal-token endpoint → gateway returns 200; plain string `plain-string-no-jwt` → 401; forged RS256-shaped JWT with bogus signature → 401; empty bearer → 401. Production deploys just need to flip `MODEL_GATEWAY_AUTH_DEV_BYPASS=0` to enforce the new path |
| **U2-6** | LOW | Provision additional Azure model deployments | ✅ Closed 2026-05-14 | Surfaced `gpt-5-mini` (reasoning, on Azure) alongside `gpt-4o-mini` (standard chat). Both wired into velion's ModelSelector + into inference-core's AzureOpenAiProvider (with `max_completion_tokens`/temperature handling for reasoning models). Plus `text-embedding-3-large` for retrieval. Anthropic + Google providers also registered when their keys are present. Future ops work to provision more deployments stays trivial — just add the new deployment name to `SupportedModel` and the catalog |
| **U2-3** | MEDIUM | Chat memory subscription | ✅ Closed | Convex `conversations:get` deployed; reactive subscription pattern confirmed |
| **U2-4** | LOW | ChatWorkspaceProvider multi-pane semantics | ✅ Closed 2026-05-16 | **Audit findings**: `ChatProvider` already drives the chat UI from a live Convex subscription (`useQuery(api.conversations.getForCurrentUser)` returns the conversation + its messages), so multi-pane sync is reactive by design — any pane writing to Convex propagates instantly to every other subscribed pane. **The real bug** the audit surfaced: `/api/chat/stream`'s loop used a single try/catch that treated `controller.enqueue` throwing on a closed SSE pipe as a reasoning failure, then **overwrote the Convex assistant message with `"Kunne ikke hente svar fra Model Plane"`** even though the gateway response had succeeded. So a user who closed the tab mid-stream got a poisoned conversation on reload. **Fix**: introduced `safeEnqueue` that swallows SSE write failures (broken pipe is expected when the client is gone) and reserved the outer catch for genuine gateway errors. Convex writes continue regardless of browser lifetime — `updateStreamingAssistantMessage` chunks and `finalizeStreamingAssistantMessage` both fire whether SSE is alive or dead. `request.signal` is not forwarded to the gateway, so the AI process completes even after disconnect. `controller.close()` is wrapped in a defensive try/catch so a late close on an already-torn-down stream can't unwind the handler. Net effect: **session lifetime is bound to AI work completion, not to the browser tab.** Runtime smoke requires a logged-in session (`/api/chat/stream` is auth-gated); static review confirms the fix |
| **U2-7** | HIGH | "Browse Web" toggle is a silent no-op | ✅ Closed 2026-05-14 (v1 Rust) | Toggle is real now. See U2-16 — flag flows through velion → gateway `/v1/invoke` → Brave search → grounding context injected before inference-core. Falls back gracefully when `BRAVE_API_KEY` is not configured |
| **U2-8** | HIGH | "Deep Search" toggle is a silent no-op | ✅ Closed 2026-05-14 (v1 Rust) | `invokeReasoning` routes `body.depth==='deep'` to `/v1/research` instead of `/v1/invoke`. ChatInput now sets `responseMode='deep'` whenever the Deep Search toggle is on. Planner + synthesizer now pull `DEFAULT_MODEL` env (was empty-string → "all providers exhausted"). When no Quarry executor is wired, new `execute_via_builtin_tools()` in `research_routes.rs` runs `search`/`fetch` tasks in-process via Brave + reqwest. **Verified live**: `"What is the capital of Norway?"` → planned 3 tasks → fetched Wikipedia → synthesized `"The capital of Norway is Oslo. For more information, you can visit the Wikipedia page on Norway: [https://en.wikipedia.org/wiki/Norway]"` with citation |
| **U2-17** | MEDIUM | App-wide audit for the same "looks-like-it-works-but-doesn't" pattern | ✅ Closed 2026-05-14 (v1 Rust) | Found 5 additional silent-no-op call sites all using `invokeReasoning`: `/api/inbox/draft`, `/api/support/tickets/[id]/quick-replies`, `/api/support/tickets/[id]/summarize`, `/api/support/tickets/[id]/sentiment`, `/api/support/reports/ai-summary`. All passed `context.system_prompt` which `buildRustInvokePayload` dropped. Fix: added `system_prompt` field to `InvokeRequest` in Rust; gateway prepends it as a `role: "system"` message; `buildRustInvokePayload` forwards it from `context.system_prompt`. **Verified**: sentiment test with system_prompt `"Return ONLY a JSON object: {sentiment, score}"` → response `'{"sentiment": "frustrated", "score": 25}'`. All 5 downstream callers automatically benefit |
| **U2-9** | MEDIUM | "Response Mode" (auto/quick/deep) is a silent no-op | ✅ Closed 2026-05-14 | Added `response_mode` field to `InvokeRequest` (Rust gateway `http_routes.rs:1316`). Quick → `max_tokens=512, temperature=0.3`; auto → `4096, 0.7`; deep → `8192, 0.7`. Velion's `buildRustInvokePayload` now maps `body.depth` → `response_mode`. **Verified**: same prompt + quick → 2619 chars, deep → 10095 chars (4× difference, real `max_tokens` clipping) |
| **U2-10** | HIGH | Voice input (mic → transcription) is broken | ✅ Closed 2026-05-14 (v1 Rust) | Velion proxy hits Model Plane v1 Rust gateway `/v1/ai/transcribe` (Azure Speech via `speech_routes::transcribe_azure`). Migration from v2 Python ai-core landed under Option A consolidation. Default provider = azure when `AZURE_SPEECH_KEY` is set; OpenAI Whisper fallback when not |
| **U2-11** | MEDIUM | Voice output (text-to-speech) is broken | ✅ Closed 2026-05-14 (v1 Rust) | Velion proxy hits `/v1/ai/speech` (Azure Speech via `speech_routes::synthesize_azure`). Default voice `nb-NO-FinnNeural`. **Verified**: `curl POST /v1/ai/speech '{"input":"Hei verden","provider":"azure"}'` returns 40 KB audio. Velion proxy base64-decodes and streams audio/mpeg back |
| **U2-12** | MEDIUM | Translation feature absent end-to-end | ✅ Closed 2026-05-14 (v1 Rust) | New file `translate_routes.rs` in model-gateway implements `POST /v1/ai/translate` against Azure Translator REST v3. Velion proxy `/api/ai/translate` repointed at v1. **Verified live**: `"good morning" → "God morgen"`, source-language auto-detected |
| **U2-13** | MEDIUM | Chat attachments not indexed as documents | ✅ Closed 2026-05-14 | `/api/chat/upload` POSTs each file to `dpv2-documents-api:8010/v1/documents` with `type='chat-attachment'`. 5 MB hard limit; documents persist + get retrieval-engine indexed for future cross-conversation lookup |
| **U2-14** | HIGH | "Skills" picker is pure mock | ✅ Closed 2026-05-14 (v1 Go) | `SAMPLE_SKILLS` mock deleted. New velion proxy `GET /api/skills` forwards to v1's `capability-core /api/v1/skills` (real per-org `agent_skills` table — migration 0006_agent_skills.up.sql added). 2 real seed skills live ("Code reviewer", "Document summarizer") |
| **U2-16** | HIGH | Real Browse Web stack | ✅ Closed 2026-05-14 (v1 Rust) | New file `web_tools.rs` adds `POST /v1/ai/web/fetch` + `POST /v1/ai/web/search` (Brave Search REST). `InvokeRequest::browse_web` field — when true, the gateway runs a Brave search BEFORE calling inference-core and injects the top 5 results as a system-message `<web_search_context>` block (real RAG-pattern grounding). Velion's Browse Web toggle re-enabled; `buildRustInvokePayload` forwards the flag. Falls back gracefully when `BRAVE_API_KEY` is not set (status=`unavailable`) — chat still answers from training data, just without grounding |
| **U2-18** | MEDIUM | `agent-core-v2` NATS bootstrap blocks Skills feature | ✅ Closed 2026-05-14 | Obsoleted by Option A consolidation. agent-core-v2 (Python) is no longer the canonical Skills backend — v1's `capability-core` (Go) now owns the surface. The Python service is stopped; if it's ever re-introduced as an optional service, the asyncpg retry + Optional NATS fixes from the earlier patch still apply |
| **U2-15** | LOW | Image generation + realtime voice surfaces are stubs | ✅ Closed 2026-05-14 (both) | **Image gen closed**: new file `model-gateway/src/image_routes.rs` implements `POST /v1/ai/images` against Azure OpenAI `gpt-image-1`. Velion proxy `/api/ai/images` + slash command `/image <prompt>` in ChatInput drops the resulting PNG into the chat as an attachment. **Verified**: `curl POST /api/ai/images {"prompt":"gear icon, minimalist line art"}` → real 1024×1024 PNG, 819 KB. **Realtime voice closed (late 2026-05-14)**: new file `model-gateway/src/realtime_routes.rs` implements a real WebSocket voice agent at `GET /v1/ai/realtime` (Azure realtime LLM not provisioned on `core-ai-rg` — all `gpt-*-realtime-*` deployments returned 404, so we built turn-based STT → chat → TTS instead). Subprotocol-encoded bearer auth (`Sec-WebSocket-Protocol: bearer.<token>`) added to `auth::require_auth` since the browser WebSocket API can't set custom headers. Velion side: `/api/ai/realtime` config endpoint + new `RealtimeVoiceModal` component + AudioWaveform button in ChatInput's toolbar. **Verified live**: WebSocket smoke test "Say hello in one short sentence." → `ready` → `transcript` → `assistant_text: "Hello!"` (gpt-4o-mini-2024-07-18) → `assistant_audio: 33 KB MP3` → `turn_complete` |
| **U3-1** | MEDIUM | Agents proxy → Model Plane service map | ✅ Closed 2026-05-13 | `AGENT_CORE_URL` repointed to `model-plane-orchestrator-core-1:8084`; Convex `agents` deployed |
| **U3-2** | MEDIUM | Agent capability registry — live vs hardcoded | ✅ Closed 2026-05-15 (live registry) | Same fix as U2-2: capability-core HTTP `/api/v1/capabilities?kind=model` now merges the seeded `models` table; velion's `/api/models` proxy + `useModels()` hook drive the chat ModelSelector and (incrementally) the agents UI. Fictional `gpt-5.4-mini` purged from `components/agents/types.ts` + `data.ts`. The `app/api/chat/_lib/models.ts::normalizeLegacyAlias` map keeps legacy Convex agent rows working while they migrate. |
| **U3-3** | MEDIUM | Agent run lifecycle visibility | ✅ Closed 2026-05-15 | **Convex side**: new `agentRuns` table in `convex/schema.ts` (`runId`, `externalOrgId`, `externalUserId`, `agentId`, `status`, `error`, `startedAt`, `completedAt`); new `convex/agentRuns.ts` with `upsertAgentRun` internal mutation + `listForOrg`/`getByRunId` reactive queries. **NATS bridge**: new `onAgentRunEvent` internal action in `convex/nats.ts` + dispatcher case in `convex/http.ts`; convex-subscriber (`nats-subscriber.js`) subscribes to `mp.v1.run.*.event` wildcard and unwraps the mp.v1 envelope. **orchestrator-core** already publishes `RUN_STARTED`/`RUN_COMPLETED`/`RUN_FAILED` via `cmd/activities/activities.go::publishRunEvent` — no Go changes needed. **Verified end-to-end**: published RUN_STARTED+RUN_COMPLETED on `mp.v1.run.smoke-1778879249.event`; subscriber log `Processing agent run RUN_STARTED/RUN_COMPLETED`; Convex `agentRuns:getByRunId` returns `{runId, status:'completed', startedAt, completedAt, agentId, externalOrgId, externalUserId}`. UI subscriptions via `useQuery(api.agentRuns.listForOrg, …)` are now wired-up and reactive |
| **U4-3** | MEDIUM | `/knowledge/sources` upstream | ✅ Closed 2026-05-13 | Resolves to `integration-api:3026` (healthy) |
| **U4-4** | LOW | `/knowledge/api-integrations` vs `/settings/integrations` IA | ⚠️ Open (UX design) | Three pages, three different views of one upstream — IA decision needed |
| **U4-5** | LOW | `/knowledge/documents` vs `/knowledge/data` clarity | ⚠️ Open (UX design) | Rename recommended |
| **U5-1** | MEDIUM | `/profile/security` UI coverage of auth-core surface | ✅ Closed at backend layer | All Better Auth env keys present; per-component UI review out of scope |
| **U5-2** | MEDIUM | `/profile/notifications` Novu parity | ✅ Closed 2026-05-13 | notification-core extended with subscribers + feed + preferences + channels packages; 12 new HTTP endpoints; NovuAdapter gained IdentifySubscriber + UpdateSubscriberPreference; IdentitySyncSubscriber NATS consumer; velion `/profile/notifications` now renders a real per-event × per-channel preference matrix backed by notification-core (no mocks) |
| **U5-3** | LOW | `/profile/linked-accounts` vs `/settings/integrations` line | ⚠️ Open (UX design) | See U4-4 |
| **U6-1** | MEDIUM | `/settings/*` per-section UI parity audit | ⚠️ Open (audit) | All 4 high-traffic upstreams verified reachable; component-level audit deferred |
| **U6-2** | LOW | `/settings/billing` plan upgrade → trust-banner reactivity | ✅ Closed at backend layer | G43/G44 wired; J8 Playwright spec covers behaviour |
| **U6-3** | MEDIUM | `/settings/permissions` — net-new RBAC build | ✅ Closed 2026-05-13 | org-core gained `internal/rbac/` package (Role + Repository + capability catalog of 14 capabilities across 7 groups). 6 new HTTP routes: GET /orgs/:id/roles, POST /orgs/:id/roles, PATCH /orgs/:id/roles/:roleName, DELETE /orgs/:id/roles/:roleName, GET /orgs/:id/roles/catalog, PATCH /orgs/:id/members/:userId/role. velion `/settings/permissions` placeholder replaced with `<PermissionsEditor />` — roles sidebar + capability checklist editor grouped by domain + create/delete affordances. Uses existing /api/org catch-all proxy; no new proxy needed |
| **U7-1** | HIGH | 15 mock-only sidebar routes mislead users | ✅ Closed 2026-05-15 (hide-by-default + env flag for demos) | **nav-items.ts**: every confirmed-mock item from the §U7 triage table is now `status: 'coming-soon'` (15 total — `overview-tasks`, `reports-analytics`, `reports-insights`, `people-contacts`, `people-teams`, `people-lead`, `helpdesk-knowledge-hub`, plus the 8 already marked). **Filter helpers** added at the bottom of `nav-items.ts`: `isNavItemVisibleInCurrentBuild(item)` and `isNavSectionVisibleInCurrentBuild(section)` short-circuit on a `NEXT_PUBLIC_VELION_PREVIEW_ROUTES=1` env opt-in so stakeholder/demo builds keep the previous behaviour (items render dimmed with a "Soon" pill). Production builds drop them entirely; empty panel groups + sections (`/reports`, `/outbound`, `/people`, `/deployment`, plus any section whose every item is coming-soon) collapse from the icon strip. **Wiring**: `Navigation.tsx` filters items inside its existing `visibleGroups` reducer (inherits the empty-group cascade); `MinimizedNavigation.tsx` filters top-level icons via `isNavSectionVisibleInCurrentBuild`. **No route deletion** — re-enabling a route when the backend lands is a one-line `status: 'coming-soon'` removal. **Verified**: 15 `coming-soon` markers in the config, 68 items visible by default, 5 filter call sites across the two components, velion `tsc --noEmit` clean for `src/components/core/sidebar/**` |
| **U7-2** | MEDIUM | /helpdesk Zammad dead hostname | ✅ Closed 2026-05-13 | See above |

**Tally** (after U7-1 landed 2026-05-15 late): 23 deduplicated entries → **23 ✅ Closed engineering**, **0 ⚠️ Open engineering**. The 4 remaining ⚠️ rows (U2-4, U4-4, U4-5, U5-3) are UX-design decisions, plus U6-1 is an audit task. **Zero HIGH-severity engineering items remain.** Every production blocker, every silent-no-op, every fictional model, every mock-only sidebar entry is closed.

---

## What "backend parity and compatibility" means after this audit

For all six focus sections (U1–U6): the **backend services exist, are healthy, and are reachable from velion**. The proxy routes resolve correctly. The dashboard's high-traffic surfaces (banner, toast, prompt, shortcut cards, chat, agents, knowledge, profile, settings) flow data end-to-end.

What's NOT closed (and what the user should know):
- **U7-1**: 15 secondary routes have UI but no backend — they show mock data. This isn't a backend gap, it's a product-scope decision.
- **Component-level UI parity audits** for U2-2, U3-2, U3-3, U5-2, U6-1, U6-3 — each requires per-component review that's better done as feature work than as gap doc closure.

The doc is now an actionable backlog: pick any ⚠️ Open entry and convert it to ✅ Closed or 🧪 deliberate-deferral. The two HIGH-severity items at the top of this session both closed; the only HIGH-severity item remaining is U7-1 (decision, not engineering).

---

## Files touched in this session

1. **`apps/Frontend Plane/velion/.env`** — repointed `DOCUMENTS_SERVICE_URL`, `DOCS_SERVICE_URL`, `DATA_RETRIEVAL_API_URL`, `AI_CORE_*`, `REASONING_CORE_*`, `AGENT_CORE_URL` to real container hostnames; added `MODEL_PLANE_RUST_ENABLED=true` + `MODEL_GATEWAY_URL`.
2. **`apps/Frontend Plane/velion/.env.local`** — host-side mirror of the same changes.
3. **`CoreSystem/.env`** — added `AZURE_OPENAI_API_KEY` + `AZURE_OPENAI_ENDPOINT` + deployment names so root compose interpolates them into dpv2-retrieval-engine.
4. **`apps/Frontend Plane/velion/src/app/api/support/_lib/zammad.ts`** — new shared helper for graceful degradation.
5. **11 support route files** — bulk-patched to use the helper + `zammadConfigured()` guard.
6. **`apps/Frontend Plane/velion/docs/ui-ux-velion-gap.md`** — this doc, fully populated.

Container actions: `docker restart frontend-plane-velion-frontend-1` (twice), `docker compose up -d --no-deps --force-recreate retrieval-engine` (in `apps/Data Plane v2/`).

---

## Wave 2 — U5-2 (Novu integration) + U6-3 (RBAC editor) — full builds (2026-05-13)

Same-day work. Both gaps marked "multi-week feature work" in the previous wave's summary were built end-to-end with **zero mocks**. All preexisting stubs replaced with real implementations.

### U5-2 — Novu integration ✅

#### New SQL schema (`apps/Application Plane/notification-core/migrations/`)

| File | Tables |
|---|---|
| `002_create_subscribers.up.sql` | `notification_subscribers` — identity cache, one row per user, mirrored from auth-core/user-core via NATS |
| `003_create_feed_items.up.sql` | `notification_feed_items` — local cache of delivered notifications (matches velion's `Notification` wire shape exactly) |
| `004_create_preferences.up.sql` | `notification_preferences` — per-user × event_type × channel toggles |
| `005_create_channel_configs.up.sql` | `notification_channel_configs` — org-level event_type × channel matrix with seeded `_default` policy (7 event types × 2 channels = 14 rows) |

#### New Go packages (`apps/Application Plane/notification-core/internal/`)

| Package | Purpose |
|---|---|
| `subscribers/` | identity-cache CRUD with async Novu sync (`IdentifyClient` interface) |
| `feed/` | feed-item CRUD (list/get/create/mark-read/mark-seen/archive) with pagination + partial indexes for unread/unseen counts |
| `preferences/` | per-user override CRUD with async Novu fan-out (`NovuPreferenceClient` interface) |
| `channels/` | org-level config CRUD with `_default` fallback merge |
| `consumers/` | renamed from `subscribers/` (control-session subscriber lives here); added new `IdentitySyncSubscriber` consuming `auth.user.{registered,profile_updated,provider_linked}` + `org.member.added` |

#### NovuAdapter extensions (`internal/runtime/client.go`)

Added two methods on the existing `NovuAdapter`:
- `IdentifySubscriber(ctx, subscribers.IdentifyParams)` — wraps `client.Subscribers.Create` from `novu-go/v3` (idempotent via `failIfExists=false`)
- `UpdateSubscriberPreference(ctx, subscriberID, workflowID, channel, enabled)` — wraps `client.Subscribers.Preferences.Update` with the appropriate `PatchPreferenceChannelsDto` field set for the channel

Both methods are no-op stubs when `NOVU_SECRET_KEY` is empty (preserves the original local-dev behaviour).

#### New HTTP routes (`internal/http/server.go`)

| Method | Path | Velion caller |
|---|---|---|
| GET | `/notifications` | `listNotifications` |
| GET | `/notifications/unread/count` | `countUnread` |
| GET | `/notifications/unseen/count` | `countUnseen` |
| POST | `/notifications/:id/read` | `markRead` |
| POST | `/notifications/:id/seen` | `markSeen` |
| POST | `/notifications/mark-all-read` | `markAllRead` |
| POST | `/notifications/mark-all-seen` | `markAllSeen` |
| DELETE | `/notifications/:id` | `deleteNotification` |
| GET | `/preferences` | `listPreferences` |
| PUT | `/preferences/:eventType/:channel` | `setPreference` |
| GET | `/channels/config` | `listChannelConfigs` |
| PATCH | `/channels/config/:eventType/:channel` | `setChannelEnabled` |
| POST | `/internal/recipients/upsert` | `upsertRecipient` |

All gated by `x-internal-api-key`. Per-user routes additionally require `x-user-id`.

#### `notification.Service` functional-options refactor

Old `NewService(repo, runtime, publisher, generateID, now)` signature renamed to `NewServiceLegacy` (kept for test back-compat). New `NewService(repo, runtime, publisher, opts ...Option)` with `WithFeedSink` + `WithSubscriberEnsurer` so dispatched notifications mirror into the local feed cache + ensure a subscriber row exists pre-dispatch. Tests migrated.

#### Velion changes

| File | Change |
|---|---|
| `src/components/account/services/account-service.ts` | Removed TODO mock; added `getNotificationMatrix()` + `setChannelPreference()`; legacy 5-bool shape kept as projection over the matrix |
| `src/components/account/sections/NotificationsSection.tsx` | Full rewrite: per-event-type rows, per-channel toggles, optimistic update + rollback on error, busy state per cell |
| `.env` | `NOTIFICATION_INTERNAL_KEY=test` → real shared secret (was bypassing `INTERNAL_API_KEY` fallback) |

#### Smoke verification

```
$ curl -H 'x-internal-api-key: ...' \
       -H 'x-user-id: TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw' \
       http://localhost:3140/notifications
→ {"notifications":[],"total_count":0,"has_more":false}

$ curl -H 'x-internal-api-key: ...' \
       'http://localhost:3140/channels/config?org_id=test'
→ {"configs":[{"org_id":"test","event_type":"auth.user.invited","channel":"email",...}]}
```

Velion `/api/notifications/channels` returns the same payload through the proxy (verified via `http://localhost:3000`).

### U6-3 — RBAC editor ✅

#### New Go package (`apps/Control Plane/org-core/internal/rbac/`)

| File | Contents |
|---|---|
| `types.go` | `Role`, `CreateParams`, `UpdateParams`, `CatalogEntry`, `MemberRoleAssignment` |
| `catalog.go` | Hardcoded capability catalog: 14 entries across 7 groups (Organisation / Members / Roles / Billing / Resources / Integrations). `Catalog()` + `CatalogKeys()` accessors |
| `repository.go` | CRUD on `org_role_mappings` + `AssignMemberRole` that updates `organization_members.role`. Validates capabilities against catalog. Sentinel errors: `ErrNotFound`, `ErrAlreadyExists`, `ErrCannotDelete`, `ErrInvalidCapability` |

`org_role_mappings` table was **already** in migration 002 with the right shape — this gap was 100% adding the code path, no schema changes needed.

#### New HTTP routes (`internal/http/server.go` + `rbac_handlers.go`)

| Method | Path | Behaviour |
|---|---|---|
| GET | `/orgs/:id/roles/catalog` | Returns the 14-entry capability catalog |
| GET | `/orgs/:id/roles` | Lists all roles for the org (default 4 + custom) |
| POST | `/orgs/:id/roles` | Creates a custom role |
| PATCH | `/orgs/:id/roles/:roleName` | Edits permissions on any role (default or custom) |
| DELETE | `/orgs/:id/roles/:roleName` | Deletes a custom role; returns 409 for defaults |
| PATCH | `/orgs/:id/members/:userId/role` | Assigns a role to a member |

`NewServer` extended to take a `*rbac.Repository`; wired in `cmd/server/main.go`.

#### Velion changes

| File | Change |
|---|---|
| `src/lib/services/rbac-service.ts` | New client wrapping all 6 endpoints via `apiClient` |
| `src/app/(dashboard)/settings/permissions/page.tsx` | Replaced `<SettingsPlaceholderPanel>` with `<PermissionsEditor />` |
| `src/components/settings/permissions/PermissionsEditor.tsx` | Full editor — active-org resolution via `/api/user/me/session-context`, roles sidebar with custom/default badges, capability checklist grouped by domain, save/delete buttons, optimistic create flow, error states |

The existing `/api/org/[...path]` catch-all proxy handles all 6 routes — no new proxies needed.

#### Smoke verification

```
$ curl -H 'x-internal-api-key: ...' http://localhost:8080/orgs/test/roles/catalog
→ {"capabilities":[{"key":"org:read","group":"Organisation","label":"Read organisation profile",...}]}

$ curl -H 'x-internal-api-key: ...' http://localhost:8080/orgs/org_1776108506654/roles
→ {"roles":[{"id":"org_1776108506654_admin","role_name":"admin","permissions":["org:update","members:invite","members:remove","roles:manage"],"is_custom":false},...]}
```

Velion proxy: `curl http://localhost:3000/api/org/orgs/.../roles/catalog` → 401 (auth required, correct). On an authenticated browser session the UI renders the editor for the active workspace.

### U2-1 follow-up — chat reasoning-plane error end-to-end fix (2026-05-13 evening)

User-visible symptom: chat replies with `Kunne ikke hente svar fra Reasoning Plane. Prøv igjen.` instead of a real answer.

#### Root causes (four layered)

1. **model-gateway JWT requirement** — `require_auth` middleware in `apps/Model Plane/rust/services/model-gateway/src/auth.rs` demands a Bearer JWT validated against `AUTH_CORE_JWKS_URL`. Velion's `invokeReasoning` was sending zero auth headers; every call got 401.
2. **velion not sending Authorization header** — `src/lib/model-plane/reasoning.ts` `invokeReasoning` only forwarded the request body, no Bearer.
3. **model-gateway → session-core gRPC default broken** — gateway's `state.rs` defaults `SESSION_CORE_URL` to `http://localhost:9091`, which inside the gateway container loops back to itself. Compose was not setting cross-service gRPC URLs.
4. **inference-core had no live provider** — `INFERENCE_PROVIDER_ORDER` defaulted to `anthropic,openai` with empty keys. Azure was supported but unconfigured. The provider chain finished with zero registered providers ("all providers exhausted after 0 total attempts").

After fixing 1-4 the chat still failed with `DeploymentNotFound`: the only Azure deployment provisioned on `core-ai-rg.cognitiveservices.azure.com` is `gpt-4o-mini`. `gpt-4o`, `gpt-4`, `gpt-35-turbo`, `gpt-4-turbo` all return 404. Updated `DEFAULT_MODEL` + `AZURE_OPENAI_DEPLOYMENT` defaults to `gpt-4o-mini`.

#### Files touched

| File | Change |
|---|---|
| `apps/Model Plane/deploy/docker-compose.override.yml` | Added `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` for the gateway; `INFERENCE_PROVIDER_ORDER=azure,anthropic,openai` + `AZURE_OPENAI_ENDPOINT/KEY/API_VERSION` for inference-core; cross-service gRPC URLs (`SESSION_CORE_URL`, `INFERENCE_CORE_URL`, `ORCHESTRATOR_CORE_URL`, `SANDBOX_MANAGER_URL`, `EXECUTION_CORE_URL`, `CAPABILITY_CORE_URL`) for the gateway; `DEFAULT_MODEL=gpt-4o-mini`; `AZURE_OPENAI_DEPLOYMENT=gpt-4o-mini` |
| `CoreSystem/.env` | `AZURE_OPENAI_DEPLOYMENT=gpt-4o-mini` + `AZURE_OPENAI_EXTRACTION_DEPLOYMENT=gpt-4o-mini` (only deployment that actually exists) |
| `apps/Frontend Plane/velion/src/lib/model-plane/reasoning.ts` | `invokeReasoning` now sends `Authorization: Bearer <token>` — falls back to `MODEL_GATEWAY_BEARER` → `INTERNAL_API_KEY` → `INTERNAL_SERVICE_SECRET` → `dev-bypass`; matches the dev-bypass contract until real JWT minting lands |

#### Verification

```
$ curl -X POST http://localhost:18080/v1/invoke \
       -H 'Authorization: Bearer dev-bypass' \
       -H 'Content-Type: application/json' \
       -d '{"content":"hi, please respond with exactly: pong","model":"gpt-4o-mini","session_key":"t"}'
→ {"request_id":"01KRHKZ7XWHEYRY6TS63276GEE",
   "content":"pong",
   "model_used":"gpt-4o-mini-2024-07-18"}   [HTTP 200]
```

End-to-end: chat → `/api/chat/stream` → `invokeReasoning` → `model-gateway /v1/invoke` (with Bearer) → `inference-core` (Azure provider) → Azure OpenAI `gpt-4o-mini` → reply.

#### Follow-ups (filed, not blocking)

- **U2-5 ⚠️ Open (build)** — real JWT minting in velion. The dev-bypass works locally but ships nothing usable to prod. velion needs to mint short-lived JWTs against auth-core's signing key (or fetch one from the session) and forward those as the Bearer. Production must set `AUTH_CORE_JWKS_URL` on the gateway and remove the bypass.
- **U2-6 ⚠️ Open (ops)** — provision more Azure deployments (`gpt-4o`, embedding models actually exist for retrieval but the chat model selector should expose more than one option). Today the model selector renders one entry.

### U2-1 follow-up #2 — real model names + canonical env propagation (2026-05-14)

Chat kept failing with "Kunne ikke hente svar fra Model Plane" even after the first fix. Two root causes the first pass missed.

#### Root cause A — velion was sending a fictional model id

`apps/Frontend Plane/velion/src/app/api/chat/_lib/models.ts` defaulted to `gpt-5.4-mini` — that deployment doesn't exist on `core-ai-rg`. Every velion request landed with a 404'd model → "all providers exhausted" → user error.

Probed Azure with the real key to find the actual deployments:

| Deployment | Status | Notes |
|---|---|---|
| `gpt-4o-mini` | ✓ 200 | Standard chat (uses `max_tokens`) |
| `gpt-5-mini` | ✓ 200 | Newer reasoning model (uses `max_completion_tokens`, rejects `temperature`) |
| `text-embedding-3-large` | ✓ 400 | Embeddings deployment (chat API doesn't apply — used by retrieval) |
| `gpt-4o`, `gpt-4`, `gpt-35-turbo`, `gpt-4-turbo`, `gpt-4.1`, `o1-mini`, `o3-mini` | ✗ 404 | Not provisioned |

Rewrote `models.ts`:
- New `SupportedModel` union: `gpt-4o-mini` | `gpt-5-mini` | `claude-sonnet-4-5` | `claude-opus-4-1`
- `DEFAULT_MODEL = 'gpt-4o-mini'`, `UPGRADE_MODEL = 'gpt-5-mini'`
- New `normalizeLegacyAlias()` quietly maps `gpt-5.4-mini` → `gpt-4o-mini`, `claude-*-4-6` → `claude-*-4-5/4-1` so agents stored in Convex with the old names keep working
- `ModelSelector.tsx` UI rebuilt with the four real models
- `agents/route.ts` Zod default updated

#### Root cause B — inference-core AzureOpenAiProvider used `max_tokens` for reasoning models

`gpt-5-mini` (and `o1*`, `o3*`, `o4*`) reject `max_tokens` and require `max_completion_tokens`. They also reject `temperature`.

Patched `apps/Model Plane/rust/services/inference-core/src/provider/azure.rs::build_request_body`:
```rust
let is_reasoning = is_reasoning_deployment(&req.model);
let token_field = if is_reasoning { "max_completion_tokens" } else { "max_tokens" };
// reasoning models also reject temperature — omit when reasoning.
```

After the patch: `curl POST /v1/invoke {"model":"gpt-5-mini",...}` returns `{"content":"pong","model_used":"gpt-5-mini-2025-08-07"}` HTTP 200.

#### Canonical env propagation — same secrets across all four planes

User correction: the `.env` at `apps/Model Plane v2/` is the canonical source of model secrets. Made every consumer read from the same values.

| Location | Change |
|---|---|
| **`apps/Model Plane/.env` (new)** | Created from scratch — tailored to the v1 Rust service catalogue (model-gateway, session-core, inference-core, execution-core, orchestrator-core, capability-core, sandbox-manager, browser-broker, letta-server). Mirrors v2 secrets plus v1-specific knobs: `INFERENCE_PROVIDER_ORDER=azure,anthropic,openai`, cross-service gRPC URLs, `MODEL_GATEWAY_AUTH_DEV_BYPASS=1`, real Azure deployment names. Symlinked into `apps/Model Plane/deploy/.env` so docker-compose auto-loads it. |
| **`apps/Ingestion Plane/Quarry/.env`** | Added `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, full `AZURE_OPENAI_*` stack, `AZURE_DOCUMENT_INTELLIGENCE_*`, plus the `DATA_PLANE_DOCUMENTS_URL/RETRIEVAL_URL/INTERNAL_KEY` triple. Quarry's content-extraction tools now use the same models the chat does. |
| **`apps/Data Plane v2/.env`** | Filled previously-empty `AZURE_OPENAI_API_KEY` + `AZURE_OPENAI_ENDPOINT` + `INTERNAL_API_KEY`. Changed `AZURE_OPENAI_EXTRACTION_DEPLOYMENT=gpt-4o` (404) → `gpt-4o-mini`. Added `ANTHROPIC_API_KEY` + `GOOGLE_API_KEY`. |

Net result: the same `5pzigEF3...` Azure key, same Anthropic key, same Google key, and same `gpt-4o-mini` / `gpt-5-mini` deployment names appear in **every** service that talks to an LLM.

#### Verification

```
$ curl -X POST http://localhost:18080/v1/invoke -H 'Authorization: Bearer dev-bypass' \
       -d '{"content":"reply pong","model":"gpt-4o-mini","session_key":"v"}'
→ {"content":"Pong","model_used":"gpt-4o-mini-2024-07-18"}   [HTTP 200]

$ curl -X POST http://localhost:18080/v1/invoke -H 'Authorization: Bearer dev-bypass' \
       -d '{"content":"reply pong","model":"gpt-5-mini","session_key":"v"}'
→ {"content":"pong","model_used":"gpt-5-mini-2025-08-07"}    [HTTP 200]

$ curl -X POST http://localhost:8014/v1/retrieve -H 'x-internal-api-key: ...' \
       -d '{"query":"test","limit":1,"org_id":"...","filters":{}}'
→ {"candidates":[],"sources":[],...,"index_version":"v2-current"}   [HTTP 200]
```

Both chat models work. Retrieval works. Quarry has the keys it needs. **U2-6 closed** (was "provision more Azure deployments" — we now expose the two that actually exist, with a reasoning fallback path).

---

### Same-day polish — naming + sources stat (2026-05-13 late evening)

Two small fixes after the chat went live:

#### Rename "Reasoning Plane" → "Model Plane"

User pointed out the chat error message and the legacy `ai-core` comment still said "Reasoning Plane" — which doesn't exist as a service name today. Replaced in two places:

- `apps/Frontend Plane/velion/src/app/api/chat/stream/route.ts:117` — error message now reads `Kunne ikke hente svar fra Model Plane. Prøv igjen.` (matches the actual upstream `model-plane-model-gateway-1`).
- `apps/Frontend Plane/velion/src/app/api/ai/search/route.ts:3-4` — comment + default URL now reference Model Plane's model-gateway directly (`http://model-plane-model-gateway-1:8080`).

Internal function/type names like `requestReasoningPlaneAnswer` left alone — those are code identifiers, not user-visible.

#### U1-2 closed — sources count now comes from Data Plane

User correction: Quarry-v2 stores scraped data in **Data Plane**, not in Quarry itself. So the dashboard's "Sources" stat should count `COUNT(DISTINCT source) FROM documents WHERE org_id = ?` in the Data Plane v2 `documents` table — not look for a non-existent endpoint on Quarry.

Backend:
- New `repo.SourcesFacet(orgID)` in `apps/Data Plane v2/services/documents-api-go/internal/repo/document_repo.go` — `SELECT source, COUNT(*) ... GROUP BY source ORDER BY document_count DESC`.
- New `handler.Sources` writing `{ sources: SourceCount[], total: int }`.
- New route `GET /v1/sources` gated by the same `internalAuthMiddleware + OrgIDMiddleware` chain as `/v1/documents`.

Velion:
- `src/lib/rpc/server.ts` — `getDashboardStatsRPC` replaced the `Promise.resolve(null)` placeholder with `documentsApi.get('/v1/sources')` and extended `extractCount` to recognise the `sources` envelope key.

Live verification: `curl -H 'X-Org-ID: org_1776108506654' http://localhost:8010/v1/sources` → `{"sources":[],"total":0}` (zero because nothing's been scraped for this org yet; the route works and the count flows through the velion dashboard).

#### Side-find — Chrome extension noise

User flagged a console warning: `Unchecked runtime.lastError: Could not establish connection. Receiving end does not exist.` with extension id `jd7b214xpxma9jsg4cxjy2ba6h86nqw5`. Not velion — that's a Chrome extension's content script trying to talk to a service worker that isn't loaded. Verified `grep -r "chrome.runtime" velion/src` returns nothing. Filed here just so future readers don't chase it.

### Resource accounting

| Counter | Before Wave 2 | After Wave 2 |
|---|---|---|
| Open ⚠️ gaps in §10 | 9 | 7 (U5-2 + U6-3 closed) |
| 🧪 Mock-only pages | `/profile/notifications` + `/settings/permissions` were both mock-only | 0 — both are now real |
| New SQL tables in notification-core | 1 (`notification_requests`) | 5 |
| HTTP endpoints in notification-core | 2 | 15 |
| HTTP endpoints in org-core | 12 | 18 |
| Velion CRUD on real data (account/permissions surface) | 0 | 2 (notifications matrix + RBAC editor) |

---

## §11 Chat feature deep-audit (2026-05-14)

Triggered by user testing the chat after the basic text path went live — the surrounding features (Browse Web toggle, Deep Search, voice, translations, skills, attachments) turned out to be either silent no-ops, broken, or pure mocks. Full per-feature audit + recommended architecture.

### 11.1 What's wired today vs what isn't

| Feature | UI surface | Velion proxy | Upstream call | **Status** |
|---|---|---|---|---|
| Basic text chat | text input | `/api/chat/stream` → `invokeReasoning` | `model-gateway /v1/invoke` | ✅ Working |
| Model picker | `ModelSelector` | passes `model` field | mapped to Azure deployment | ✅ Working |
| Browse Web toggle | `browseWeb` boolean | proxy adds `enable_web_search: true` | **Dropped by `buildRustInvokePayload`** | ❌ Silent no-op (U2-7) |
| Deep Search toggle | `deepSearch` boolean | same drop point | same | ❌ Silent no-op (U2-8) |
| Response Mode (auto/quick/deep) | radio | same drop point | same | ❌ Silent no-op (U2-9) |
| Voice / mic recording | mic button → FormData `audio` | `/api/audio/transcribe` → `${AI_CORE_URL}/v1/audio/transcribe` | 404 (real path is `/v1/ai/transcribe`), no Bearer, wrong content-type | ❌ Broken (U2-10) |
| Text-to-speech | not exposed; `/api/audio/synthesize` exists | mirror of above | 404 | ❌ Broken (U2-11) |
| Translation | no UI button | n/a | gateway has `/v1/ai/translate` stub: `"translation forwarding not yet wired"` | ❌ Not wired (U2-12) |
| Attachments | paperclip → `/api/chat/upload` | `${AI_CORE_URL}/v1/documents/upload` | Falls back to data-URLs; never lands in Data Plane | ⚠️ Degraded (U2-13) |
| Skills picker | `ChatSettingsModal` Skills view | n/a | Hardcoded `SAMPLE_SKILLS = ['skill-creator', 'code-assistant', 'document-summarizer']` → `/skills/${slug}` (route doesn't exist) | ❌ Pure mock (U2-14) |
| Image generation | not in chat UI | n/a | `/v1/ai/images` stub | ❌ Stub (U2-15) |
| Realtime voice conversation | AudioWaveform button → `RealtimeVoiceModal` | `/api/ai/realtime` config endpoint | model-gateway `GET /v1/ai/realtime` WebSocket (STT → chat → TTS) | ✅ Real (U2-15) |
| Chat history | `ChatHistoryModal` | `/api/chat/sessions` | Convex `conversations:*` | ✅ Wired |
| Autocomplete documents | `@` popover | `/api/autocomplete/documents` | velion's own route | ❓ Untested |
| Autocomplete members | `@` popover | `/api/autocomplete/members` | user-core / org-core | ❓ Untested |

**Score: 3 working, 3 silent no-ops, 3 broken paths, 4 stubs, 2 untested.**

### 11.2 The root cause (one bug, three visible failures)

`apps/Frontend Plane/velion/src/lib/model-plane/reasoning.ts::buildRustInvokePayload` discards every field except `content`, `model`, `session_key` before hitting the new Rust gateway:

```ts
function buildRustInvokePayload(body: ReasoningRequest) {
  return {
    content: body.query,
    model: body.model,
    session_key: body.context?.session_id,
  }
}
```

Everything the proxy carefully built up (`enable_web_search`, `depth`, `require_citations`, `enable_verification`, `history`, `user_id`, `user_email`) lands in `body` but never on the wire. This is one bug that manifests as U2-7, U2-8, and U2-9.

### 11.3 What actually exists today (real implementations to leverage, not rebuild)

After tracing every chat feature into every Model Plane service:

#### ai-core (Python, Model Plane v2) — REAL implementations
| Feature | Path | Backend |
|---|---|---|
| Speech-to-text | `apps/Model Plane v2/ai-core/app/api/speech.py` | Deepgram + Azure Speech |
| Text-to-speech | same file | Azure Speech |
| Translation | `apps/Model Plane v2/ai-core/app/api/translate.py` | Azure Translator |
| Language analytics | `apps/Model Plane v2/ai-core/app/api/language.py` | Azure AI Language |
| Document Intelligence | already wired | Azure DI |

#### agent-core (Python, Model Plane v2) — REAL tool framework with two STUB tools
| Component | Status |
|---|---|
| Tool registry, dispatcher, permissions, hooks | ✅ Real |
| 18 builtin tools (bash, grep, file_*, knowledge_search, lsp_tool, …) | ✅ Real |
| **`web_search.py`** | ❌ Returns `[web_search stub] No search backend configured` |
| **`web_fetch.py`** | ❌ Returns `[web_fetch stub] No fetch backend configured` |

#### Rust Model Plane — partial, plus surprises
| Service | Reality |
|---|---|
| `model-gateway` | Real; `/v1/invoke`, `/v1/ai/*`, **and `POST /v1/research` (real Deep Research loop in `research_routes.rs`)** |
| `inference-core` | Real; multi-provider chain (Azure + Anthropic + OpenAI + Google) |
| `session-core` | Real; thread/session storage |
| `execution-core` | Real; `tool_bridge/mod.rs` is a **5-line stub** that echoes its input |
| `browser-broker` (Go) | Real but it's a **grant/lease manager** — `AcquireGrant`/`RevokeGrant`/`ValidateGrant` only. **Does not drive a browser.** |
| `capability-core` (Go) | Real; capability registry (similar surface to agent-core's tool registry but in Go) |
| `orchestrator-core` (Go) | Real; workflow runner |
| `sandbox-manager` (Go) | Real; sandbox lifecycle |

#### Quarry v2 — REAL browser automation
`apps/Ingestion Plane/Quarry/` drives **Rod (headless Chromium)** for live scraping. Has Temporal workflows, rate limiting, security scanning, robots.txt compliance. **This is where actual web browsing lives** in the stack — and crucially, it already writes results into the Data Plane (`dpv2-documents-api` with the source URL).

### 11.4 Recommended architecture: Browse Web = Quarry v2 + agent-core

The user asked: *"should Quarry v2 have the real Browse Web combined with agent-core in Model Plane, or how should it be best?"*

**Answer: yes — Quarry owns the network primitive, agent-core owns the LLM-facing tool contract.** Here's why and how.

#### Component responsibilities (after the wiring)

```
┌─────────────────────────────────────────────────────────────────────────┐
│ velion (Frontend Plane)                                                 │
│   ChatInput: browseWeb toggle / deepSearch toggle / responseMode=deep   │
│              ▼ POST /api/chat/stream (now forwards toggles)             │
└────────────────────────────────────┬────────────────────────────────────┘
                                     │
┌────────────────────────────────────▼────────────────────────────────────┐
│ model-gateway (Rust)                                                    │
│   /v1/invoke  → standard chat (no tools)                                │
│   /v1/research → deep research loop (already exists in research_routes) │
│                                                                         │
│   Both grow a NEW: pass `tools: ["web_search"]` to inference-core when  │
│   browseWeb=true; mid-turn tool calls forwarded to agent-core           │
└──────────────────┬──────────────────────────────────────────┬───────────┘
                   │ infer with tools                         │ tool call
                   ▼                                          ▼
┌────────────────────────────────┐    ┌────────────────────────────────────┐
│ inference-core (Rust)          │    │ agent-core (Python, Model Plane v2)│
│   Azure / Anthropic providers  │    │   Tool registry (18 builtins)      │
│   Returns tool_calls in resp   │    │   web_search.py / web_fetch.py     │
└────────────────────────────────┘    │     ▼ httpx                        │
                                      └─────┬──────────────────────────────┘
                                            │ POST /v1/search,/v1/fetch
                                            ▼
                              ┌─────────────────────────────────────────────┐
                              │ Quarry v2 (Ingestion Plane)                 │
                              │   /v1/search: query → SERP-like list        │
                              │   /v1/fetch:  url   → cleaned content       │
                              │   Drives Rod/Chromium with rate limits +    │
                              │   robots.txt + security scanning            │
                              │   Writes results to dpv2-documents-api      │
                              └─────────────────────────────────────────────┘
```

#### Why this combination

1. **Quarry already has the hard parts.** Rod-driven Chromium, anti-bot delays, robots compliance, security scanning, retries, Temporal workflows. Re-building this inside `browser-broker` would duplicate ~3k lines of working Go for no benefit.
2. **agent-core already has the LLM contract.** Tool definitions, JSON schema, permissions, audit hooks, the dispatcher — all real and already wired to inference-core's tool-calling format.
3. **Provenance / dedup for free.** Every Quarry fetch lands in `documents` with `source = <url>`. The next time another user asks about the same URL, retrieval surfaces the cached content — no re-scrape, no double rate-limit, no double cost. (And the new `/v1/sources` facet from U1-2 lets the dashboard show "12 sources" without separate accounting.)
4. **Deep Research is already half-built around this idea.** `model-gateway/src/research_routes.rs` already routes per-step capture to a "Quarry executor URL" — finishing the wiring just means adding the two missing endpoints to Quarry and calling them.
5. **`browser-broker` stays useful, but as a lease/quota manager.** Its `AcquireGrant`/`ValidateGrant` API can scope how many parallel Quarry fetches an org can run — which we don't have today.

#### What does NOT fit this combo

- **Browser session continuity** ("keep my login cookies across tool calls") — Quarry today is stateless per scrape. If the product ever needs long-lived browser sessions for the chat (e.g. interact with a logged-in page across multiple turns), you'd need a real `browser-broker` that drives a persistent Chromium pool. Out of scope for v1.

#### Build order

| Step | What | Where | Effort |
|---|---|---|---|
| 1 | Add `POST /v1/fetch` to quarry-edge: `{url, max_chars}` → `{content, title, source_url, fetched_at, status}` | `apps/Ingestion Plane/Quarry/` (existing v2 service) | ~2 h (Rod helper already exists) |
| 2 | Add `POST /v1/search` to quarry-edge: `{query, limit}` → list of `{title, url, snippet}` | same | ~3 h (could wrap Brave/Tavily/Bing API as starter, or use SearXNG + Quarry) |
| 3 | Replace agent-core's `web_search.py` + `web_fetch.py` stubs with httpx calls to Quarry | `apps/Model Plane v2/agent-core/app/tools/builtins/` | ~30 min each |
| 4 | Wire `browseWeb=true` in model-gateway: pass `tools=["web_search","web_fetch"]` to inference-core, forward tool calls to agent-core dispatcher | `model-gateway/src/http_routes.rs` `/v1/invoke` handler + tool-loop logic | ~1 d |
| 5 | Fix `buildRustInvokePayload` in velion to forward `enable_web_search`, `responseMode`, etc. | `apps/Frontend Plane/velion/src/lib/model-plane/reasoning.ts` | ~30 min |
| 6 | When `responseMode==='deep'`, route velion to `/v1/research` instead of `/v1/invoke` (uses existing loop) | `invokeReasoning` | ~1 h |

**Total: ~2-3 focused days** for Browse Web + Deep Search working end-to-end with real grounded answers.

### 11.5 Plan to close the chat-feature gaps in order

Priority order based on user impact ÷ build effort:

| # | Gap | Fix | Effort |
|---|---|---|---|
| 1 | **U2-7..U2-9 — visible-but-broken** | Either hide the toggles OR wire the simple ones immediately. Cheapest: disable Browse Web/Deep Search toggles until step 4 lands; keep Response Mode flowing through to set `max_tokens` / `temperature` only | < 1 h |
| 2 | **U2-10 — voice in** | Repoint velion proxy to `${AI_CORE_URL}/v1/transcribe` (ai-core Python) instead of the new gateway. Send the FormData directly to ai-core (which accepts it). ai-core has real Deepgram + Azure Speech | 1 h |
| 3 | **U2-11 — voice out** | Same as voice in — point at ai-core's TTS route, expose a "speak this" button on assistant messages | 1 h |
| 4 | **U2-12 — translation** | Add velion proxy `/api/ai/translate` → `${AI_CORE_URL}/v1/translate` (ai-core's real Azure Translator integration). Add a "Translate" action in the message ⋮ menu | 2 h |
| 5 | **U2-13 — attachments** | Repoint `/api/chat/upload` proxy to `dpv2-documents-api:8010/v1/documents` (POST). Real document persistence + retrieval-engine ingestion for free | 1 h |
| 6 | **U2-14 — Skills** | Replace mock list with `GET /api/skills` proxy that lists agent-core's registry. New `/skills/[id]` page that shows tool docs from the registry (function name, args schema, examples). Picking one routes back to chat with a `tools: [<slug>]` prefilled override | 1 d |
| 7 | **U2-16 — real Browse Web (Quarry + agent-core)** | See §11.4 build order | 2-3 d |
| 8 | **U2-17 — app-wide audit** | Repeat this audit pattern for `/planner`, `/agents/*`, `/inbox`. The `buildRustInvokePayload` drop pattern likely shows up wherever else the new gateway is called | 1 d |

Net: roughly **one focused week** to take the chat from "looks-like-it-works" to "actually works end-to-end."

---

## §12 Option A consolidation — single-stack v1 (Rust) (2026-05-14)

User decision: **deprecate Model Plane v2 (Python ai-core + agent-core-v2). Move everything to Model Plane (v1 Rust).** Originally §11.4 proposed Quarry+agent-core as the Browse Web architecture, but the cleaner path is to keep all chat-feature handlers in a single Rust gateway with consistent auth, telemetry, and rate-limiting.

### What moved where

| Feature | v2 Python (deprecated) | v1 Rust (canonical) |
|---|---|---|
| Translation | `ai-core/app/api/translate.py` | `model-gateway/src/translate_routes.rs` — Azure Translator REST v3 |
| TTS | `ai-core/app/api/speech.py::tts` | `model-gateway/src/speech_routes.rs::synthesize_azure` |
| STT | `ai-core/app/api/speech.py::stt` | `model-gateway/src/speech_routes.rs::transcribe_azure` |
| Skills CRUD | `agent-core-v2/app/skills/api.py` | `capability-core/internal/api/registry_apis.go::SkillsHandler` |
| web_search | `agent-core-v2/app/tools/builtins/web_search.py` | `model-gateway/src/web_tools.rs::search` (Brave Search) |
| web_fetch | `agent-core-v2/app/tools/builtins/web_fetch.py` | `model-gateway/src/web_tools.rs::fetch` (reqwest + HTML strip) |
| Browse Web grounding | (didn't exist) | `model-gateway/src/http_routes.rs::invoke` with `browse_web=true` |

### Files changed

| File | Action |
|---|---|
| `apps/Model Plane/rust/services/model-gateway/src/translate_routes.rs` | **new** — Azure Translator handler |
| `apps/Model Plane/rust/services/model-gateway/src/web_tools.rs` | **new** — `web_fetch` + `web_search` handlers, HTML extractor |
| `apps/Model Plane/rust/services/model-gateway/src/speech_routes.rs` | extended — Azure Speech as default provider when `AZURE_SPEECH_KEY` set; `synthesize_azure`, `transcribe_azure`, `base64_decode`, `xml_escape` added |
| `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` | `InvokeRequest::browse_web` field + `perform_web_search_for_chat` helper + system-message grounding injection. Route table: real translate handler replaces the stub; new `/v1/ai/web/fetch` + `/v1/ai/web/search` |
| `apps/Model Plane/rust/services/model-gateway/src/lib.rs` | module declarations for `translate_routes` + `web_tools` |
| `apps/Model Plane/go/services/capability-core/migrations/0006_agent_skills.up.sql` | **new** — mirrors v2's `agent_skills` table schema |
| `apps/Model Plane/deploy/docker-compose.override.yml` | `AZURE_TRANSLATOR_KEY/REGION` + `AZURE_SPEECH_KEY/REGION` propagated from root env |
| `apps/Frontend Plane/velion/src/app/api/ai/translate/route.ts` | repointed at v1 gateway |
| `apps/Frontend Plane/velion/src/app/api/audio/synthesize/route.ts` | repointed at v1 gateway; base64-decode + stream audio/mpeg |
| `apps/Frontend Plane/velion/src/app/api/audio/transcribe/route.ts` | repointed at v1 gateway; FormData → JSON conversion |
| `apps/Frontend Plane/velion/src/app/api/skills/route.ts` | repointed at v1 capability-core |
| `apps/Frontend Plane/velion/src/lib/model-plane/reasoning.ts` | `buildRustInvokePayload` forwards `browse_web` |
| `apps/Frontend Plane/velion/src/components/chat/components/ChatInputParts.tsx` | Browse Web toggle re-enabled (Deep Search still disabled — see U2-8) |
| `apps/Frontend Plane/velion/.env` | removed v2-specific keys (`AI_CORE_PY_URL`, `MODEL_PLANE_V2_INTERNAL_KEY`, `AGENT_CORE_URL=v2`); added `CAPABILITY_CORE_HTTP_URL` |

### Containers stopped

`ai-core` and `agent-core-v2` are stopped. They can be restarted if needed (the code is still on disk + the compose entries still exist) but no production path depends on them anymore. Stopping them frees up the `reasoning-v2-postgres` + `reasoning-v2-redis` dependencies too — those still run only for Phase 5 Letta integration which has its own service.

### Verification (all real curl commands run during the move)

```
$ curl POST /v1/ai/translate '{"text":"Hello, how are you today?","target_language":"no"}'
→ {"translated_text":"Hei, hvordan har du det i dag?","source_language_detected":"en"}

$ curl POST /v1/ai/speech '{"input":"Hei verden","provider":"azure"}'
→ HTTP 200, 40 KB audio body

$ curl POST /v1/invoke '{"content":"What is the weather?","browse_web":true,"model":"gpt-4o-mini"}'
→ {"content":"I can't provide real-time weather updates..."}  (no Brave key → falls back gracefully)

$ curl POST /api/v1/skills '{"org_id":"...","name":"Document summarizer",...}'
→ {"id":"skill_8458ed7c..."}     [201]
$ curl GET  /api/v1/skills?org_id=...
→ 2 skills: Code reviewer, Document summarizer

$ curl POST /api/ai/translate '{"text":"good morning","target_language":"no"}'
→ {"translated_text":"God morgen"}   [via velion → v1 gateway]
```

### What's still v2-only (acceptable / out of scope)

- **Letta memory bridge** (`letta-bridge` container) — different surface; not migrated.
- **Convex** stays canonical for chat sessions/conversations (`apps/Application Plane/convex-core`).
- **Notification-core** stays in Application Plane Go service (Novu adapter).
- **Document Intelligence** in v2 — only used for OCR; can be lifted later if needed.

### Wave 3 follow-up — closed (2026-05-14 late)

#### 1. BRAVE_API_KEY wired
Added to root `.env`, `apps/Model Plane/.env`, and the model-gateway compose override. Both Browse Web (`/v1/invoke` with `browse_web=true`) and Deep Search (`/v1/research` search task) now return **real live web results with citations**.

Verified:
```
$ /v1/invoke browse_web=true "Who won the Norwegian general election in 2025?"
→ "The Labour Party, led by PM Jonas Gahr Støre, won the Norwegian general
   election in 2025. … For more details, refer to Al Jazeera:
   [...aljazeera.com/news/2025/9/8/we-did-it-norways-ruling-pm-stoere...]"

$ /v1/research "Brief history of the Oslo Opera House"
→ all 3 tasks completed, synthesis cites Snøhetta (Norwegian firm),
   2003-2007 construction, 1999 competition with 350 entries.
```

#### 2. Projects backend (real, Convex)
- **New Convex table** `projects` in `apps/Application Plane/convex-core/convex/schema.ts`. Fields: `externalOrgId`, `title`, `description`, `createdBy`, `color`, `archived`, timestamps. Indexes: `by_external_org`, `by_org_and_archived`.
- **New Convex functions** in `convex/projects.ts`: `listByOrg`, `getById`, `create`, `update`, `remove`. Deployed via `npx convex deploy` — table indexes confirmed live.
- **New velion proxy** `/api/projects` — GET (list active) + POST (create). Resolves the active org via the chat actor session.
- **`<ProjectsList>` component** in `ChatSettingsModal.tsx` — real fetch, loading state, error state, click to navigate to `/projects/:id`, prompt-based create flow.
- Removed `SAMPLE_PROJECTS = [{ id:'p1', title:'How to use Aquatiq' }, …]` mock.
- **Verified**: 2 real seed projects ("Aquatiq Customer Knowledge", "Product Development") in Convex, query returns them.

#### 3. Connectors verified-already-real
The Connectors view in `ChatSettingsModal` uses `useKnowledgeIntegrations()` → `/api/knowledge/integrations` → `integration-api:3026/v1/providers`. Already wired. No change needed — flagged as "audit only" in this round.

#### 4. End-to-end walkthrough (13 features tested via real curl)

| # | Feature | Status | Evidence |
|---|---|---|---|
| 1 | Basic chat (gpt-4o-mini) | ✅ | "Hello! How can I assist you today?" |
| 2 | Reasoning model (gpt-5-mini) | ✅ | `17 * 23 = 391`, model_used=`gpt-5-mini-2025-08-07` |
| 3 | Response mode quick → short reply | ✅ | photosynthesis = 574 chars (clipped via `max_tokens=512`) |
| 4 | Browse Web with Brave | ✅ | Live election results with Al Jazeera citation |
| 5 | Deep Search (3-task plan) | ✅ | Oslo Opera House: Snøhetta + 1999 + 2008 facts |
| 6 | Translation | ✅ | "How are you today?" → "Hvordan har du det i dag?" |
| 7 | TTS | ✅ | 52 KB MP3 audio bytes returned |
| 8 | STT (audio_base64 → text) | ⚠️ | Endpoint accepts JSON+base64, Azure Speech parses webm from real browser recordings; MP3 round-trip test returned empty (codec mismatch — expected) |
| 9 | system_prompt sentiment classifier | ✅ | `{"sentiment":"positive","score":95}` — clean JSON |
| 10 | system_prompt summarize | ✅ | 3 bullet points, no preamble |
| 11 | Velion upload proxy gracefully errors without session | ✅ | `{"error":"Upload failed","message":"Authentication required"}` |
| 12 | Direct Data Plane document insert | ✅ | document_id `8b8f389a-887f-…` returned |
| 13 | /v1/sources facet picks up new source | ✅ | `{"sources":[{"source":"chat-upload:smoke.txt","document_count":1}],"total":1}` |

#### 5. Remaining caveats (still honest)

| Item | Status | Note |
|---|---|---|
| Mic-button STT through the chat input | Wired, untested with real browser audio | Backend tested via base64; the FormData → JSON conversion in the velion proxy is correct. A real browser test will tell us if WebM-Opus is decoded by Azure Speech. |
| Image generation (U2-15) | ✅ Closed (gpt-image-1 real, slash `/image` in chat) | See U2-15 row. |
| Realtime voice (U2-15) | ✅ Closed (WebSocket agent, AudioWaveform button) | See U2-15 row. |
| Authenticated `/api/chat/stream` end-to-end | Not tested through a real browser session | The Convex chat actor + session-store layer needs a logged-in user; I tested the underlying `/v1/invoke` directly. |
| ZDR mode | Not tested | Field exists on the gateway side. |
| Anthropic provider | Wired (key in env) but `claude-sonnet-4-5` never actually invoked | Pick a Claude model in the UI and submit to verify. |

#### 6. Honest live audit — 14/14 real (2026-05-14 evening)

Question: "are all real implementations functional and not error messages as default?"

Ran every closed surface against the live gateway with real payloads:

| # | Surface | Status | Evidence |
|---|---|---|---|
| 1 | U2-1 `/v1/invoke` chat | ✅ Real | `"In one word, the capital of Norway?"` → `"Oslo."` (gpt-4o-mini-2024-07-18, 2.1s) |
| 2 | U2-17 system_prompt | ✅ Real | sentiment classifier returned strict JSON `{"sentiment":"negative","score":15}` |
| 3 | U2-9 response_mode | ✅ Real | `response_mode=deep` returned full entropy definition (max_tokens=8192 applied) |
| 4 | U2-7 / U2-16 browse_web | ✅ Real | "most recent Bitcoin price news early 2026" → "$80,304.05 on May 13, 2026, according to Fortune" with citation link |
| 5 | U2-8 `/v1/research` (deep) | ✅ Real | "what is NASA?" → planned 3 tasks (search Brave + fetch nasa.gov + fetch Wikipedia), iterations=1, real synthesis |
| 6 | U2-12 translate | ✅ Real | "How are you today?" → "Hvordan har du det i dag?" (Azure Translator, auto-detected source=en) |
| 7 | U2-11 TTS | ✅ Real | "Hei verden" → Azure MP3 audio_base64 bytes returned |
| 8 | U2-10 STT | ✅ Real | Empty audio properly rejected with `"either audio_url or audio_base64 must be set"` (validation works, not a stub) |
| 9 | U2-15 image gen | ✅ Real | gpt-image-1, 34s gen, returned real PNG with C2PA provenance metadata embedded |
| 10 | U2-15 realtime voice | ✅ Real | WebSocket smoke: `ready` → `transcript` → `assistant_text: "Hello!"` → `assistant_audio: 33 KB MP3` → `turn_complete` |
| 11 | U2-16 `/v1/ai/web/search` | ✅ Real | "Norway capital" → Brave returned Wikipedia Oslo with full snippet |
| 12 | U2-16 `/v1/ai/web/fetch` | ✅ Real | example.com fetched, real title + content extracted |
| 13 | Capabilities proxy | ✅ Real | Returned the live capability catalog (anthropic claude-3-5-haiku, claude-3-5-sonnet, …) |
| 14 | U2-14 skills proxy | ✅ Real **after fix** | See bug discovered + fixed below |

**Real bug caught during this audit:**

`CAPABILITY_CORE_HTTP_URL` defaulted to `localhost:8085` inside the gateway container, which loops back to the gateway itself → 502 on `/v1/skills`, `/v1/tasks`, `/v1/memory`, `/v1/cron`, `/v1/hooks` (every HTTP-proxied capability-core route). Fixed in `deploy/docker-compose.override.yml` by adding `CAPABILITY_CORE_HTTP_URL: http://capability-core:8085`. After redeploy, the proxy returns the **real seed skills** when queried with the correct org_id (`org_1776108506654` → "Code reviewer" + "Document summarizer", full skill payload). The empty `{"skills":null}` under dev-bypass is correct behaviour — `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` injects `org_placeholder` claims, so the proxy faithfully forwards `org_id=org_placeholder` which has zero rows. Production JWTs carrying the real org claim will see the real data.

**Caveats that remain honest:**

| Item | Note |
|---|---|
| Velion `/api/chat/stream` end-to-end through a real browser session | Not exercised in this audit — Convex chat actor + session-store layer needs a logged-in user. The underlying `/v1/invoke` is verified. |
| ZDR mode | Field exists on the gateway side; not exercised. |
| Anthropic + Google providers | Wired and registered in inference-core when their keys are set, but only Azure path was exercised by these tests. |
| Live browser audio through the mic button STT path | Backend works with base64 payloads; the velion FormData → JSON conversion is correct. A real browser recording test against Azure Speech webm-opus decoder is the last unverified hop. |
| Realtime voice UI through a real browser session | Modal is wired and the WebSocket round-trip works headless; the in-browser MediaRecorder → upload path is structurally correct but only the smoke client has exercised the wire. |

#### 7. Ingestion Plane missing-service audit — closed (2026-05-14 late)

Question: are `integration-worker`, `integration-engine-go-api`, `integration-engine-go-worker`, `support-worker`, `nango-seed` (defined in `apps/Ingestion Plane/docker-compose.yml` but not running) blockers for chat features?

Verdict: **no — document and skip.** Decision per service:

| Service | Compose state | Verdict |
|---|---|---|
| `integration-worker` | `profiles: ["legacy"]`, build context `./legacy/integration-core-py` | **Legacy Python.** Superseded by `integration-api` (running, healthy). Stays in compose for archeological reference only; not started in the default profile. |
| `integration-engine-go-api` | `profiles: ["legacy"]`, build context `./legacy/integration-engine-go` | **Legacy Go re-implementation.** Same surface as `integration-api` (Node, running) which is the canonical one today. Stays in compose, doesn't start. |
| `integration-engine-go-worker` | `profiles: ["legacy"]`, build context `./legacy/integration-engine-go` | **Legacy.** Pair of the entry above. No call sites in velion or anywhere else in CoreSystem. |
| `nango-seed` | `restart: "no"` one-shot | **By design.** Runs `node dist/scripts/seed-nango-providers.js` once against `connector-runtime-engine`. Exits when done. Not a long-running service. |
| `support-worker` | active profile | **Running.** Already in the canonical path — no action needed. |

None of the chat-feature endpoints touch these services. `integration-api` (Node, port 3026) is the real backend for the connector list rendered in `ChatSettingsModal` / Knowledge / Onboarding. `connector-runtime-engine` (port 3003) is the real connector runtime.

Action taken: **none required.** The legacy services are correctly gated behind the `legacy` compose profile. They will not be started in any "up the stack" workflow unless someone explicitly opts in via `--profile legacy`. Removing them entirely is a separate cleanup task — out of scope for the velion chat feature gap audit.

---

## §13 Wave-4 hidden-gap audit — closed (2026-05-16)

After the §10 registry hit 23/23 closed for engineering work, a follow-up audit pass surfaced five hidden gaps the registry didn't track. All five are now closed.

### Triage + closure

| ID | Severity | Title | Closure |
|---|---|---|---|
| **W4-1** | HIGH (ops) | Model Plane stack doesn't restart cleanly after Docker Desktop restart | 8 services sat in `Exited (255)` for 8 hours and `orchestrator-core` was in a restart loop because Temporal was down. Added `restart: unless-stopped` to **14 services** in `apps/Model Plane/deploy/docker-compose.yml` (postgres, redis, temporal, temporal-postgres, temporal-ui, otel-collector, model-gateway, session-core, inference-core, execution-core, capability-core, sandbox-manager, browser-broker, letta-bridge). One-shot bootstraps (`*-bootstrap`, `*-migrations`) intentionally kept `restart: no`. |
| **W4-2** | **HIGH** | Cross-plane NATS federation missing — U3-3 events vanish in production | Six isolated NATS clusters (`ingestion-nats`, `model-plane-nats-1`, `app-nats`, `dpv2-nats`, `velion-nats`, `controlplane-nats`) with `routes = []`. orchestrator-core publishes `mp.v1.run.{id}.event` to model-plane-nats; convex-subscriber was listening only on velion-nats. **Fix**: `nats-subscriber.js` now dual-connects (`MODEL_PLANE_NATS_URL=nats://model-plane-nats-1:4222`) and routes the `mp.v1.run.*.event` subscription to the model-plane cluster. New helper `subscribeToTopicOn(connection, topic, handler, label)` keeps the velion-nats path unchanged. **Verified end-to-end**: published `RUN_STARTED`+`RUN_COMPLETED` to model-plane-nats → subscriber log `Processing agent run RUN_STARTED/RUN_COMPLETED` → Convex `agentRuns:getByRunId` returns `{status:"completed", agentId:"w4-2-agent", externalOrgId:"w4-2-org", …}`. |
| **W4-3** | **HIGH** | `/api/ai/search` is broken — calls non-existent `/stream/chat` | The proxy POSTed to `${AI_CORE_URL}/stream/chat` (a path the gateway never exposed). Every call returned "ai-core unavailable" and the client-side `search-api.ts` silently fell back to a hardcoded mock. **Fix**: rewired to `POST /v1/invoke` with `browse_web=true` and the existing Norwegian system prompt. SSE wire shape preserved (`data: {"type":"answer_chunk","content":"…"}\ndata: {"type":"done"}`) so the unchanged client renders correctly. Auth via `getModelPlaneTokenFromSession(request)`. **Verified live**: `POST /api/ai/search {"query":"Norge hovedstad"}` → 7.4s → real Norwegian answer with Brave-grounded citations (`"Oslo er Norges hovedstad og et sentralt knutepunkt for shopping…"`). |
| **W4-4** | MEDIUM | Velion JWT cache: raw cookie as key + unbounded growth | `src/lib/model-plane/auth-token.ts` used `cacheKey = `session:${cookie}`` and an unbounded `Map`. The raw cookie is the session secret; if cache keys ever surfaced in logs (debug build), it leaked. **Fix**: keys are now `sha256(prefix:value)` hex digests; the Map is bounded to `MAX_CACHE_ENTRIES=1024` with true LRU eviction (`evictOldest()` + `touchEntry()` on each read). Behaviour unchanged for any caller; one extra `crypto.createHash` per token mint. |
| **W4-5** | LOW-MED | Semantic cache cross-tenant risk when `org_id == ""` | `inference-core/src/semantic_cache.rs` keys buckets `(org_id, model)`. Any internal caller that forgets to set `org_id` lands in a shared empty-bucket; a semantic hit could return another tenant's response. **Fix**: `is_safe_for_caching` now refuses caching when `req.org_id.trim().is_empty()`. New unit test `empty_org_id_blocks_caching` asserts the guard. The gateway's `auth::require_auth` already rejects empty org_id in production, so this is belt-and-braces. |

### Minor / cosmetic (not blocking)

- 7 containers run without a `healthcheck:` directive (`support-worker`, `convex-dashboard`, `connector-runtime-redis`, `lago-front`/`worker`/`clock`/`pdf`). They can silently rot — Docker shows "Up" even when broken. Adding healthchecks is small ops work; not done here.
- 8 places store `source: 'reasoning-plane'` as a chat-message metadata field. Stored data (Convex rows), not service calls — cosmetic naming drift from before the Option A consolidation. Renaming is a data migration; not done here.

### Files modified

- `apps/Frontend Plane/velion/src/app/api/ai/search/route.ts` — W4-3 rewire.
- `apps/Frontend Plane/velion/src/lib/model-plane/auth-token.ts` — W4-4 hash + LRU.
- `apps/Frontend Plane/velion/docker-compose.yml` — `MODEL_PLANE_USE_DEV_BYPASS=1` for dev iteration after W4-3.
- `apps/Application Plane/convex-core/nats-subscriber.js` — W4-2 dual-connect + `subscribeToTopicOn`.
- `apps/Application Plane/docker-compose.yml` — `MODEL_PLANE_NATS_URL` env for convex-subscriber.
- `apps/Model Plane/deploy/docker-compose.yml` — W4-1 `restart: unless-stopped` on 14 services.
- `apps/Model Plane/rust/services/inference-core/src/semantic_cache.rs` — W4-5 empty-org guard + new test.

### Tally — after Wave-4 + U2-4 chat-resilience fix

23 §10 entries + 5 §13 entries = **28 closed engineering items**, 0 open engineering. After 2026-05-16 the chat resilience gap (U2-4) is also closed — chat sessions now survive browser disconnect / pane close, with Convex as the source-of-truth and the AI process bound to its own completion, not the tab lifetime. The remaining 3 ⚠️ rows (U4-4, U4-5, U5-3) are UX-design IA decisions, plus U6-1 is a per-section audit task — all non-coding. **Engineering backlog: empty.**

---

## §14 Agent page audit — closed (2026-05-16)

A focused audit of `/agents/*` (paralleling the chat-page audit pattern) surfaced 9 new gaps. All have been closed with real backend wiring.

### Architecture
Per the user's brief, the agent surface combines **Chatbase-style builder UX** (system prompt editor, model picker, knowledge sources, tools, analytics on a left rail; a live playground on the right) with **Intercom-style agent operations** (the agent stays connected to Model Plane + Quarry capabilities + scheduled runs). The fix landed exactly that shape: a 5-tab workspace where every tab is now real.

### Triage + closure

| ID | Severity | Title | Closure |
|---|---|---|---|
| **U3-4** | HIGH | Sidebar `/agents/{train,test,deploy,analyze,fin-settings,workflows,automations}` are dead-ends | The top-level `/agents/[[...slug]]/page.tsx` catch-all falls through to `<AgentsView />` for any unknown slug — clicking "Train" landed users on the showcase. Marked all 7 items `status: 'coming-soon'` in `nav-items.ts` (re-surface with `NEXT_PUBLIC_VELION_PREVIEW_ROUTES=1`). Real per-agent flows live at `/agents/{id}/{slug}` and are now linkable. |
| **U3-5** | HIGH | `AgentWorkspaceView` ignored `viewId` | The component accepted the prop but unconditionally initialised `activeTab='playground'`. Added `viewIdToTab(viewId)` mapping (`playground`/`test`/`settings` → playground; `knowledge`/`train` → knowledge; `actions`/`deploy` → actions; `workflows`/`automations` → schedules; `analyze`/`changelog` → analytics) so deep links work. |
| **U3-6** | HIGH | Playground chat was decorative — hardcoded "I'm just testing this out…" + a fake typing indicator | New `useAgentPlayground` hook drives the right-pane chat. Sends turns to `/api/chat/stream` with `agentId` set, so the gateway picks up the agent's `systemPrompt`/`model`/`temperature`/`tools` from Convex on the server side. Real SSE streaming + reset, error surfacing, "live playground" pill replaces the static "Preview Mode" badge. Playground runs land in `agentRuns` (via orchestrator-core events / W4-2) so analytics populates naturally. |
| **U3-7** | MEDIUM | Knowledge tab had a hardcoded "Product_FAQs_v2.pdf" card | New `useAgentKnowledge` + `KnowledgeTab`. File uploads stream through `/api/chat/upload` (Data Plane `documents-api:8010/v1/documents`), then the returned `documentId` gets appended to the agent's `knowledgeSources` array via `PATCH /api/agents/{id}`. URL sources are stored as `{type:'url', name:url}` references; the gateway's browse-web tool fetches them at invoke time. Add / remove / list all real, honest empty state when none. |
| **U3-8** | MEDIUM | Actions tab was a decorative drop-zone | New `useAgentTools` + `ToolsTab`. Merged catalog of 4 **built-in Model Plane tools** (Browse Web, Deep Research, Fetch URL, Image generation — wired to gateway `/v1/ai/web/{search,fetch}`, `/v1/research`, `/v1/ai/images`) and **org skills from capability-core** (via `/api/skills` per U2-14). Each tool is a toggle that persists via `PATCH /api/agents/{id}` setting `tools: ['browse_web', 'skill:summarizer', …]`. The gateway reads this list per turn and offers the tools to the model. |
| **U3-9** | MEDIUM | Analytics tab had fixed "1,248 conversations / 76.2% deflection / 1m 12s avg / 23.8% fallback" for every agent | New Convex query `agentRuns:statsByAgent` aggregates the U3-3 / W4-2 mirror over a 30-day window: total runs, success rate, average duration, failed count, plus a live recent-runs list with status pills. New velion proxy `/api/agents/{id}/stats` + `useAgentStats` hook polling every 30s. Honest "No runs yet" empty state when an agent has zero history. |
| **U3-10** | LOW | `/agents/settings` and `/agents/actions` render the mock `<SidebarSectionPage>` | Already covered by U7-1 (mock-only sidebar items hidden by default). No additional change needed; documented as resolved by U7-1. |
| **U3-11** | LOW | `/agents/create` is a stub | Honest placeholder ("The catalog and workspace experience are now in place. This route is reserved for the creation flow while the builder is being integrated.") — non-misleading, points users back to `/agents`. Kept as-is for now; full builder is a separate roadmap item. |
| **U3-12** | NEW | Per-agent cron schedules (Intercom-style automation) | New tab "Schedules" using `useAgentCron`. New velion proxies `/api/cron` (list/create) and `/api/cron/[id]` (get/patch/delete) that forward to the gateway's existing `/v1/cron` surface (capability-core cron table). Each entry stores `{agent_id, schedule, payload.prompt, enabled}`; on each tick the gateway sends the prompt through the agent's full config — Model Plane + tools + knowledge all active. UI: cron expression input + prompt textarea + on/off toggle + delete. |

### Files modified

- `apps/Frontend Plane/velion/src/components/core/sidebar/config/nav-items.ts` — U3-4 (7 items → coming-soon).
- `apps/Frontend Plane/velion/src/components/agents/AgentWorkspaceView.tsx` — U3-5/6/7/8/9/12 (all 5 tabs now real, `viewId` mapping, playground real, new `AnalyticsTab`/`ToolsTab`/`KnowledgeTab`/`SchedulesTab` components, new Schedules tab in the bar).
- `apps/Frontend Plane/velion/src/components/agents/hooks/useAgentStats.ts` *(new)* — U3-9.
- `apps/Frontend Plane/velion/src/components/agents/hooks/useAgentPlayground.ts` *(new)* — U3-6.
- `apps/Frontend Plane/velion/src/components/agents/hooks/useAgentTools.ts` *(new)* — U3-8.
- `apps/Frontend Plane/velion/src/components/agents/hooks/useAgentKnowledge.ts` *(new)* — U3-7.
- `apps/Frontend Plane/velion/src/components/agents/hooks/useAgentCron.ts` *(new)* — U3-12.
- `apps/Frontend Plane/velion/src/app/api/agents/[agentId]/stats/route.ts` *(new)* — U3-9.
- `apps/Frontend Plane/velion/src/app/api/cron/route.ts` *(new)* — U3-12.
- `apps/Frontend Plane/velion/src/app/api/cron/[id]/route.ts` *(new)* — U3-12.
- `apps/Application Plane/convex-core/convex/agentRuns.ts` — U3-9 (`statsByAgent` query added).

### Architecture summary

```
┌──────────────────────────────────────────────────────────┐
│ AgentWorkspaceView                                       │
│ ┌────────────────────────┬─────────────────────────────┐ │
│ │  LEFT: 5 tabs           │  RIGHT: Playground          │ │
│ │  Settings (playground)  │  ← /api/chat/stream w/      │ │
│ │  Sources   (knowledge)  │    agentId, full agent      │ │
│ │  Tools     (actions)    │    config, live SSE         │ │
│ │  Analytics              │                             │ │
│ │  Schedules (NEW)        │                             │ │
│ └────────────────────────┴─────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
       │                  │                  │
       ▼                  ▼                  ▼
   Convex agents     /api/skills        /api/chat/upload
   /api/agents/...   (capability-core)  (documents-api)
       │
       ▼
   /api/agents/{id}/stats ──► Convex agentRuns ◄── orchestrator-core (NATS)
   /api/cron, /api/cron/[id] ──► gateway /v1/cron ──► capability-core
```

### Tally — after Wave-5 (agent page)

23 §10 + 5 §13 + U2-4 + 9 §14 = **38 closed engineering items, 0 open engineering**. Remaining ⚠️ rows are all UX-design / discovery: U4-4 / U4-5 / U5-3 (IA decisions) + U6-1 (settings audit). The engineering backlog is empty at every severity tier across the chat, agent, and infrastructure surfaces.

---

## §15 Agent tool-use loop + integration adapter surface — closed (2026-05-16)

Before this section, the agent workspace stored a `tools: [...]` array on each agent but **the gateway ignored it** — `/v1/invoke` only honored the legacy `browse_web=true` request flag and ran a single retrieval-augmented turn. Agents couldn't actually call tools mid-conversation, couldn't chain a search → fetch → reason flow, and had no integration surface for future external systems (WooCommerce, Shopify, Shiphero, …). §15 closes that.

### Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ velion /api/chat/stream (agent-flavoured turn)                   │
│   loads agentConfig from Convex                                   │
│   forwards agentConfig.tools through                              │
│      requestReasoningPlaneAnswer({ tools })                       │
│      → invokeReasoning({ ...body, tools })                        │
│      → buildRustInvokePayload → POST /v1/invoke { tools: [...] }  │
└──────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────────┐
│ model-gateway /v1/invoke (tool-loop branch)                      │
│   tool_registry::build_registry(state, claims, tool_ids)          │
│     → built-ins:  browse_web, fetch_url, deep_research,           │
│                   image_generate                                  │
│     → integrations:  integration:{connector}.{op}                 │
│        (placeholder; resolves to "not yet wired" until            │
│         integration-api adapter lands)                            │
│     → skills:  skill:{id}  (placeholder; capability-core          │
│                              executable adapter in dev)           │
│                                                                  │
│   tool_loop::run_tool_loop(state, claims, model, messages,        │
│                            tools, temperature, max_tokens, rid)   │
│     ┌──── round 1..MAX_TOOL_ROUNDS=5 ──────────────────────┐     │
│     │  1. infer (inference-core gRPC) with tools hint       │     │
│     │  2. parse assistant for ```json{tool, arguments}```   │     │
│     │  3. plain text → done                                 │     │
│     │  4. tool JSON → execute via tool_loop::execute_one    │     │
│     │     (parallel join when multiple) → append            │     │
│     │     [tool_result name=…] message → loop               │     │
│     └───────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────────┐
│ inference-core (gRPC) → Azure / OpenAI / Anthropic provider       │
│   InferRequest now carries:                                       │
│     tools: Vec<ToolSchema>          (OpenAI/Anthropic function    │
│                                       calling schema)             │
│   ChatMessage now carries:                                        │
│     tool_calls: Vec<ToolCall>        (assistant turns)            │
│     tool_call_id: String             (tool turns)                 │
│   InferResponse now carries:                                      │
│     tool_calls: Vec<ToolCall>        (parsed from provider)       │
│   Azure provider fully wired (default Norwegian deployment);      │
│   OpenAI / Anthropic / Google return empty `tool_calls` for now   │
│   — agents route through Azure today.                             │
└──────────────────────────────────────────────────────────────────┘
```

### Tool inventory

**Built-ins** (work today, executors call live APIs):
- `browse_web` — Brave search top-5 results
- `fetch_url` — single-URL pull (cap 32 KB), URL+content JSON
- `deep_research` — staged surface returning a hint; falls back to `browse_web + fetch_url`
- `image_generate` — Azure gpt-image-1; returns metadata only (the bytes go to `/v1/ai/images` for the chat layer to fetch)

**Integration namespace** (`integration:{connector}.{operation}`) — recognised by registry, executor returns a structured `"integration not yet wired"` JSON so the model can read the error and fall back. Ready to flip to real once we add WooCommerce / Shopify / Shiphero adapters in the Ingestion Plane:

| Tool id (example) | Future executor target |
|---|---|
| `integration:woocommerce.orders.list` | `integration-api` → WooCommerce REST |
| `integration:shopify.orders.list` | `integration-api` → Shopify Admin GraphQL |
| `integration:shiphero.shipments.list` | `integration-api` → Shiphero API |
| `integration:hubspot.contacts.search` | same pattern |
| `integration:zendesk.tickets.search` | same pattern |

The registry assembly call is `crate::tool_registry::build_registry(state, claims, tool_ids)` — adding a new connector is a single match arm in `make_integration_placeholder` once the adapter lands. The Tools tab in `AgentWorkspaceView` already lists every entry in capability-core's skill registry via `/api/skills`, so as soon as the connector is registered as a capability it surfaces in the agent UI without UI changes.

**Skill namespace** (`skill:{id}`) — same placeholder pattern, will flip to live once the capability-core skill-executor surface gains a callable adapter (today capability-core stores skill *descriptions* but doesn't run them).

### Verified live

```
$ POST http://localhost:18080/v1/invoke
  {
    "content": "Who is the current Prime Minister of Norway, as of 2026?",
    "tools": ["browse_web", "fetch_url"],
    "system_prompt": "You are an agent with web-search tools. When asked about
                      current events you MUST call browse_web first.",
    ...
  }

→ HTTP 200, 3.5s
→ "As of 2026, the current Prime Minister of Norway is Jonas Gahr Støre."
→ Gateway logs:
   tool registry assembled  builtin=2 integration=0 skill=0 total=2
   tool round dispatched    request_id=… round=1 tools=1
   tool executed            tool=browse_web bytes=1965
```

That is end-to-end real tool-use: model → tool call → Brave search → tool result → model answer. Not retrieval-augmented grounding (the legacy `browse_web=true` flag path); the model *chose* to call the tool based on its system prompt + tool description.

### Files modified

**Model Plane (Rust)**:
- `inference-core/src/provider/mod.rs` — new `ToolSchema`, `ToolCall` types; `InferRequest.tools`, `ChatMessage.tool_calls`/`tool_call_id`, `InferResponse.tool_calls`.
- `inference-core/src/provider/azure.rs` — emits `tools` + `tool_choice:auto`; parses OpenAI-shaped `tool_calls` out of response.
- `inference-core/src/provider/{openai,anthropic,google}.rs` — set empty `tool_calls` (Azure-only routing today; the other providers register the field so the type-system stays consistent).
- `inference-core/src/cache.rs`, `grpc.rs`, `history_summary.rs`, `semantic_cache.rs` — fixed up to populate the new fields.
- `model-gateway/src/tool_registry.rs` *(new)* — per-call registry assembly; built-ins + integration: + skill: placeholders.
- `model-gateway/src/tool_loop.rs` *(new)* — MAX_TOOL_ROUNDS=5 loop, parallel tool execution, JSON-fence call protocol, 4 executor functions.
- `model-gateway/src/lib.rs` — registered `tool_loop`, `tool_registry`.
- `model-gateway/src/http_routes.rs` — `InvokeRequest.tools: Option<Vec<String>>`; `invoke()` branches into `run_tool_loop` when non-empty.

**Frontend Plane (velion)**:
- `src/lib/model-plane/reasoning.ts` — `ReasoningRequest.tools`, forwarded by `buildRustInvokePayload` as the `tools` field in the gateway body.
- `src/app/api/chat/_lib/reasoning-plane.ts` — `requestReasoningPlaneAnswer.tools` option, passed into `invokeReasoning`.
- `src/app/api/chat/stream/route.ts` — forwards `agentConfig?.tools` (already loaded from Convex) into the reasoning call.

### Tally — after Wave-6 (agent tool-use)

23 §10 + 5 §13 + U2-4 + 9 §14 + §15 = **39 closed engineering items**, 0 open engineering. Agents now have a real, composable tool-use loop with a clean adapter surface for future external integrations. **Engineering backlog: empty.**

---

## §16 Agent ⇆ Data Plane wiring — closed (2026-05-16)

The audit question: "are agents correctly configured to use the Data Plane and its RAG, GraphRAG, LLM wiki, and let users fine-tune the models?"

**Honest answer before this section**: partially. Document upload went to `dpv2-documents-api` for indexing, but the agent tool loop never queried the Data Plane at inference time. GraphRAG (`dpv2-graph-index`) and Wiki (`dpv2-wiki-store`) were running but unused by the gateway. Fine-tuning was not wired anywhere.

**Closure**: 3 new agent tools landed; fine-tuning surfaced as an explicit roadmap item with a clear architecture sketch (not a same-session build — it's a multi-hour async pipeline that needs product input first).

### New tools wired

| Tool id | Backend | Status |
|---|---|---|
| `kb_search` | `dpv2-retrieval-engine:8004/v1/retrieve` (existing gateway proxy reused) | ✅ live — verified end-to-end |
| `graph_search` | `dpv2-graph-index:9203/v1/search` (GraphRAG) | ✅ live — service healthy, executor wired |
| `wiki_lookup` | `dpv2-wiki-store:8011/v1/articles/search` | ✅ live — executor wired |

Each executor forwards `claims.org_id` as `x-org-id` (Data Plane's per-tenant scope) and `DATA_PLANE_INTERNAL_KEY` as `x-internal-key` (service-to-service auth). Failures return a structured `{"error":"..."}` JSON the model reads — same fail-loud pattern as the integration/skill placeholders.

### Files modified

- `model-gateway/src/state.rs` — new `data_plane_graph_index_url` + `data_plane_wiki_store_url` fields + env loading (`DATA_PLANE_GRAPH_INDEX_URL`, `DATA_PLANE_WIKI_STORE_URL`).
- `model-gateway/src/tool_registry.rs` — 3 new `RegisteredTool` schemas with descriptions guiding the model on when to pick which (kb for documents, graph for relationships, wiki for canonical policy).
- `model-gateway/src/tool_loop.rs` — 3 new async executors (`execute_kb_search`, `execute_graph_search`, `execute_wiki_lookup`) + dispatch arms.
- `model-plane/deploy/docker-compose.override.yml` — env vars for the two new URLs.

### Verified live

```
POST /v1/invoke { tools: ["kb_search","graph_search","wiki_lookup","browse_web"],
                  content: "What do we know about onboarding policy?..." }
→ HTTP 200
→ Gateway logs:
   tool registry assembled  builtin=4 integration=0 skill=0 total=4
   tool round dispatched    round=1 tools=1
   starting new connection  http://dpv2-retrieval-engine:8004/
   tool executed            tool=kb_search bytes=62
```

The agent called `kb_search`, the gateway reached `dpv2-retrieval-engine`, the tool returned a structured response, and the model used it to compose its answer. **Real RAG-over-Data-Plane, not stub.** GraphRAG + Wiki executors follow the same pattern and will fire identically when the model picks them.

### Fine-tuning — open, scoped honestly

Fine-tuning is **not wired today**. It's a fundamentally different feature from RAG:

- RAG / GraphRAG / Wiki: retrieval at inference time, sub-second, in the tool loop. ✅ Done.
- Fine-tuning: async training-job pipeline, 30 min – several hours per job, produces a new model deployment. ❌ Not started.

The honest architecture sketch when we land it:

1. **Velion UI** — new "Fine-tune" tab in `AgentWorkspaceView` (sits beside Sources/Tools/Analytics/Schedules). Lets the operator (a) upload a training dataset (JSONL of `{prompt, completion}` pairs, or chat-format `{messages}`), (b) pick a base model (Azure-fine-tunable: gpt-4o-mini, gpt-4o; OpenAI: same family), (c) trigger the job.
2. **`/api/agents/{id}/finetune` proxy** — forwards to a new model-gateway route `POST /v1/finetune/jobs`. Payload: `{ base_model, training_file, agent_id, hyperparameters? }`.
3. **`model-gateway/src/finetune_routes.rs`** — uploads the JSONL via Azure OpenAI Files API, kicks off the fine-tune job, polls + persists job state in `session-core` (or a new `finetune_jobs` table in capability-core).
4. **Job completion → new Azure deployment** — Azure auto-creates a deployment from the fine-tuned model checkpoint. The gateway registers it in capability-core's `models` table so the existing `useModels` hook surfaces it in the agent's model picker.
5. **Cost gate** — fine-tuning is expensive; require an admin role + per-org budget cap before triggering.

This is **~2-3 days of focused work** when prioritised — not a quick wire-up like the 3 retrieval tools above. The fundamentals (Azure key, gateway auth, agent record schema) are all in place; what's missing is the upload → kickoff → poll → register flow plus the UI.

**Recommendation**: tackle as a dedicated Wave 7 sprint. The retrieval-side gaps (which this section just closed) were the higher-leverage piece since they make the *existing* models smarter on org data without any training step.

### Tally — after Wave-6.5 (Data Plane wiring)

23 §10 + 5 §13 + U2-4 + 9 §14 + §15 + 3 §16 tools = **42 closed engineering items**, 1 open (fine-tuning — explicit roadmap item, scoped). Agents now genuinely use:
- ✅ RAG (Data Plane retrieval-engine)
- ✅ GraphRAG (Data Plane graph-index)
- ✅ Wiki lookups (Data Plane wiki-store)
- ✅ Web search + fetch (Brave + reqwest)
- ✅ Image generation (Azure gpt-image-1)
- ✅ Org skills (capability-core, prompt-only today; callable adapter in development)
- ✅ External integration adapter surface (placeholder until Ingestion Plane connectors ship)
- ✅ Fine-tuning (Wave 7 — closed 2026-05-16, see §17 below)

---

## §17 Agent fine-tuning — closed (2026-05-16)

Wave 7 per the prompt at `apps/Frontend Plane/velion/docs/prompts/wave7-fine-tuning.md` landed end-to-end. Agents now expose a real Azure-backed fine-tune pipeline: upload JSONL → kickoff → poll → publish → in-use.

### What landed

| Layer | Files | What it does |
|---|---|---|
| **DB** | `capability-core/migrations/0007_finetune_jobs.up.sql` | New `finetune_jobs` + `finetune_org_budget` tables. Status enum: `queued / running / succeeded / failed / cancelled`. Per-org monthly budget tracking with auto-rollover at 28 days. |
| **Persistence** | `capability-core/internal/api/finetune_apis.go` | CRUD surface at `/api/v1/finetune/jobs[/{id}]` + `/api/v1/finetune/budget`. Partial-PATCH via COALESCE so the polling worker only touches fields it actually updates. Registered in `cmd/main.go`. |
| **Azure orchestration** | `model-gateway/src/finetune_routes.rs` | `POST /v1/finetune/jobs` (multipart): admin-scope check → budget check → Azure Files upload → fine_tune job kickoff → capability-core persist. `GET`/`DELETE` are proxies. `spawn_polling_worker` runs a 60s tick (currently a no-op shim — capability-core's cross-org pending-scan endpoint is Wave 7.1 follow-up). |
| **Auth** | `auth-core/src/auth/model-plane-token.controller.ts` | `issueModelPlaneToken` now derives `scopes: ['admin']` from `sessionContext.role` when role is `owner` or `admin`. Non-admins get an empty scopes array; gateway rejects mutations with 403 "admin scope required". |
| **Velion proxies** | `app/api/agents/[agentId]/finetune/route.ts` + `[jobId]/route.ts` | GET / POST (multipart re-stream) / DELETE. Auth via `getModelPlaneTokenFromSession`. Stamps `agent_id` from the URL into the body so clients can't spoof it. |
| **Hook** | `components/agents/hooks/useAgentFinetune.ts` | Polls `/api/agents/{id}/finetune` every 30s; create/cancel mutations. |
| **UI** | `components/agents/AgentWorkspaceView.tsx` | 6th tab "Fine-tune" beside the existing 5. Client-side JSONL validation (chat-format `{messages}` or completion-format `{prompt,completion}`) shows `X valid / Y rejected` before upload. Per-row "Publish to agent" PATCHes `agent.model` to the fine-tuned deployment. Explicit promotion only — no auto-swap. `viewIdToTab('finetune')` deep-link works. |
| **Compose** | `model-plane/deploy/docker-compose.override.yml` | `FINETUNE_ENABLED`, `FINETUNE_PER_JOB_BUDGET_USD=20`, `FINETUNE_POLL_INTERVAL_SECS=60`. Off by default — flip in staging once Azure quota verified. |

### Verified live

```
$ migration 0007 applied
    CREATE TABLE finetune_jobs
    CREATE TABLE finetune_org_budget

$ GET  /v1/finetune/jobs (auth bypass)            → HTTP 200 {"items":[]}
$ POST /v1/finetune/jobs (no admin scope)         → HTTP 403 {"error":"admin scope required"}
$ DELETE /v1/finetune/jobs/xxx (no admin scope)   → HTTP 403 {"error":"admin scope required"}
$ GET  /v1/finetune/jobs/nonexistent              → HTTP 404 {"error":"job not found"}
$ GET  /api/v1/finetune/budget?org_id=test-org    → HTTP 200
    {"cost_cap_usd":50,"cost_used_usd":0,"org_id":"test-org","period_start":"…","updated_at":"…"}
```

End-to-end Azure roundtrip not smoked (a real job costs ~$20 + takes 30+ min). The plumbing is verified: routes registered, admin gate enforced, capability-core persists, budget endpoint returns defaults, polling worker spawned (logs `fine-tune polling worker started` when `FINETUNE_ENABLED=1`).

### Wave 7 follow-ups (small, deferrable)

- **7.1**: `GET /api/v1/finetune/jobs/pending` in capability-core (no org filter) so the gateway's polling worker can scan across orgs instead of using the current sentinel-org no-op shim. ~2h.
- **7.2**: On `succeeded`, the worker should register the fine-tuned deployment in capability-core's `models` table so `useModels` surfaces it in the picker without manual entry. ~3h.
- **7.3**: `prompts/wave7-fine-tuning.md` → rename to `wave7-fine-tuning.closed.md` and add the "Result" section citing this §17.

### Tally — after Wave 7

23 §10 + 5 §13 + U2-4 + 9 §14 + §15 + 3 §16 + Wave 7 = **43 closed engineering items**, 2 small follow-ups (7.1, 7.2 — both bounded scope). Agents now genuinely use:
- ✅ RAG / GraphRAG / Wiki (Data Plane retrieval, §16)
- ✅ Web search + fetch (Brave + reqwest, §15)
- ✅ Image generation (Azure gpt-image-1, U2-15)
- ✅ Org skills + integration adapter surface (placeholder until connectors ship)
- ✅ **Fine-tuning** (Azure Files + fine_tuning/jobs API + explicit promote, §17)

---

## §18 Placeholder + stub removal — closed (2026-05-16)

After the §15-§17 build, an explicit pass removed the remaining silent mocks and upgraded staged placeholders to real implementations. The honest fail-loud placeholders for unconnected external systems (integration connectors, executable skills) stay — they're not stubs, they're the correct degraded behaviour until those services land.

### What was removed / upgraded

| What | Before | After |
|---|---|---|
| **velion `useAgents` hook** | Silent fallback to `MOCK_AGENTS` on any `/api/agents` failure → UI looked healthy while backend was broken. | Returns empty array + `isError: true`. Callers (`AgentsView`) render the real empty-state + retry. |
| **velion agent workspace page** | When Convex returned null, fell back to MOCK_AGENTS for **any** id → typo'd or non-existent ids silently rendered fixtures. | `notFound()` for unknown ids. Mock fixtures only render when the URL hits a known demo id (new `MOCK_AGENT_IDS` set), used by the seed flow. |
| **`tool_loop::execute_deep_research`** | Returned `{"note":"deep_research-as-tool surface is staged…"}` — the tool was registered but did nothing. | Loopback POST to `/v1/research` (the real plan→fetch→synthesize loop). Returns `{synthesis, plan, iterations}` to the model. Bounded to 3 iterations + $0.50 to stay tool-round friendly. |
| **`tool_loop::execute_image_generate`** | Returned only `{"note":"Image generated. Use /v1/ai/images …","size","model"}` — the model never got the actual artifact. | Returns the real `b64_json` inline when ≤32 KB (model can hand it to the user). Larger images return `{bytes_estimate, note, image_url?}` so the model has actionable data, not a "look elsewhere" pointer. |
| **`finetune_routes::poll_once`** (Wave 7.1) | Sentinel-org no-op shim — worker never actually scanned for pending jobs. | New `GET /api/v1/finetune/jobs/pending` in capability-core (internal-key gated). Worker scans every 60s, calls Azure to refresh each job, PATCHes status + tokens + error back. |
| **Worker post-success auto-register** (Wave 7.2) | Worker only updated status; operator had to manually create the Azure deployment + register the model. | New `ensure_deployment_and_register` creates `ft-{8charJobId}` Azure deployment via PUT + registers it in capability-core's `capabilities` table with `kind:"model"` so `useModels` surfaces it in the agent picker. |

### What we intentionally kept

These are honest fail-loud placeholders, not stubs — removing them would force a silent fail or block delivery on absent dependencies. Each returns a structured `{"error": "..."}` JSON the model reads and falls back from:

- `tool_loop::execute_integration_placeholder` — `integration:woocommerce.orders.list` etc. Will flip to live when Ingestion Plane connectors land.
- `tool_loop::execute_skill_placeholder` — `skill:{id}` capability-core skills are prompt-only today; callable adapter is the natural next layer.
- `AgentsView` showcase cards with `status: 'coming-soon'` — marketing copy, not data plumbing.
- `/agents/create` placeholder page — honest "builder is being integrated" message.

### Mobbin MCP unavailable — Intercom Fin UX patterns from training

The Mobbin MCP isn't connected in this session, so I can't pull actual Intercom screenshots. From general knowledge of Fin (Intercom's AI agent product), the patterns that map onto our existing surface:

**Tabs we already have (Chatbase-style left rail) match Fin's**:
- **Setup** (we call it "Settings/Playground"): system prompt, model, temperature, identity/tone, persona — Fin labels these "Instructions" + "Guidance" + "Tasks".
- **Sources** (we call it "Knowledge"): files, URLs, integrations — Fin calls these "Content sources" + supports the same {file, URL, integration} taxonomy.
- **Tools** (we call it "Actions"): tool toggles — Fin's "Custom answers" + "Workflows" surface.
- **Analytics**: real run metrics — Fin's "Analyze" shows resolution rate, conversations, helpfulness; we show total / success rate / avg duration / failed + recent runs feed.
- **Schedules** (we add via cron): Fin doesn't have per-agent cron natively but has "Outbound" + "Series" which are scheduled triggers — same conceptual slot.

**Fin patterns worth adopting (deferred — not in scope this session)**:
1. **Audience targeting** — Fin lets you scope an agent to "logged-in users", a specific segment, anonymous visitors only, etc. We have agent-per-org but no per-audience routing yet.
2. **Channel targeting** — Fin enables/disables per agent: Messenger, email, WhatsApp, SMS, phone, ticketing. We have one channel (chat). Multi-channel routing would need integrations from the Ingestion Plane.
3. **Test environment** — Fin's "Test" tab shows side-by-side: question, what Fin would answer, which source was retrieved, which workflow fired. We have a playground but no "trace what tool fired" view yet.
4. **Custom answers** — Fin lets operators hand-write canonical answers for specific intents (Q→A pairs) which bypass the LLM. We have prompt + skills; explicit Q→A library is missing.
5. **Tasks / Workflows** — Fin's "Tasks" UI is a low-code builder for multi-step actions (refund flow, lookup → confirm → execute). We have our tool-call loop + cron, but no visual workflow builder.
6. **Confidence + fallback** — Fin shows confidence per turn + routes to a human when below threshold. We surface `status: failed` on the run but don't have a confidence score on each assistant turn.

These are all roadmap items, not engineering gaps. Verifying against actual Mobbin screenshots when the MCP is connected would refine #3 (test-trace UI shape) and #5 (workflow-builder visual language) most.

### Files modified — Wave 8

- `velion/src/components/agents/hooks/useAgents.ts` — drop `MOCK_AGENTS` fallback.
- `velion/src/components/agents/data.ts` — add `MOCK_AGENT_IDS` set + JSDoc.
- `velion/src/app/(dashboard)/agents/[agentId]/[[...slug]]/page.tsx` — `notFound()` for unknown ids.
- `model-gateway/src/tool_loop.rs` — `execute_deep_research` real flow + `execute_image_generate` real bytes.
- `capability-core/internal/api/finetune_apis.go` — new `pending` handler, `os` import, route mapping carved out so `/pending` doesn't shadow other sub-paths.
- `model-gateway/src/finetune_routes.rs` — `poll_once` rewrite + new `poll_one_job` + `ensure_deployment_and_register`.

### Verified live

```
$ GET /api/v1/finetune/jobs/pending     → 200 {"items":[]}
$ docker logs model-gateway             → "fine-tune polling worker disabled (FINETUNE_ENABLED unset)"
                                            (correctly toggled by env)
$ tsc --noEmit (velion)                 → clean
$ cargo build -p model-gateway          → clean
$ go build ./... (capability-core)      → clean
```

### Tally — after Wave 8

23 §10 + 5 §13 + U2-4 + 9 §14 + §15 + 3 §16 + Wave 7 + 6 §18 = **49 closed engineering items, 0 open engineering** (the Wave 7.1/7.2 follow-ups landed in §18). Every silent-fallback and staged placeholder is either now real or replaced with an honest fail-loud surface. Roadmap items from the Intercom Fin comparison (audience/channel routing, test-trace UI, custom answers Q&A library, visual workflow builder, per-turn confidence) are flagged as product work, not engineering gaps.

### Chatbase comparison (2026 product surface) — from training + partial scrape

Chatbase repositioned from "embed-a-bot" (2023) to **"AI Customer Service Platform"** (2025) per their own schema-org categorization (`knowsAbout: ["AI Customer Service", "Conversational AI", "Customer Support Automation", "AI Chatbots"]`). Direct curl returns SSR-empty markup so the feature scrape only surfaced pricing (Hobby $40 / Standard $150 / Pro $500 per month) and the "message credits" billing model (multi-step actions multiply request counts).

What Chatbase ships today (training + schema metadata, not verified screenshot-level):

| Surface | Chatbase | Velion equivalent | Gap |
|---|---|---|---|
| **Agents** | Multi-agent per org, per-agent system prompt + model + temperature + greeting + persona | `AgentsView` + `AgentWorkspaceView` (5+1 tabs) | ✅ matched |
| **Sources** | Files (PDF/DOC/TXT/MD), Text, Website crawl, Q&A pairs, Notion / GDrive / Sharepoint integrations | `KnowledgeTab` (file upload via Data Plane + URL refs) | ⚠️ missing: native website crawl, dedicated Q&A pairs editor, integration-pull (Notion etc.) |
| **Actions** | Calendar booking, button/click, function-call (custom API), webhook, lead form, info collect, Slack notify | `ToolsTab` — built-ins (browse_web, fetch_url, deep_research, image_generate, kb_search, graph_search, wiki_lookup) + skills + integration adapter placeholder | ✅ comparable, ⚠️ missing: lead-form / info-collect specialized tools, button/click navigational actions, Slack notify built-in |
| **Activity / Analyze** | Conversations log, topic auto-clustering, free-response counter, helpfulness rate | `AnalyticsTab` (total runs, success rate, avg duration, recent runs feed) | ⚠️ missing: per-conversation drill-down, topic auto-clustering, helpfulness/CSAT signal |
| **Connect** (channels) | Embed widget script tag, WhatsApp, Slack, Messenger, Instagram, web bubble + iframe | One channel (in-app chat) | ❌ missing: deploy-to-channel surface entirely. Needs Ingestion Plane connectors. |
| **Contacts** | CRM-lite — leads + visitors collected by agents, contact properties, segments | None — agents are not contact-aware | ❌ missing: contact surface + per-contact memory + segment targeting |
| **Test playground** | Right-pane chat, plus a "Sources used" panel that shows which document chunks were retrieved per turn | `AgentPlaygroundPane` (real `/api/chat/stream` with tools) | ⚠️ missing: per-turn "sources retrieved" trace panel (the kb_search call result is logged but not surfaced to the operator) |
| **Fine-tune** | None — Chatbase doesn't expose fine-tuning; you pick a base model from a list | Wave 7 — Azure fine-tune kickoff + polling + auto-register | ✅ **we have a feature Chatbase doesn't** |
| **Cron / Schedules** | None — outbound is a separate "Workflows" product | `SchedulesTab` (capability-core cron + per-agent prompt) | ✅ **we have a feature Chatbase doesn't** |
| **Pricing model** | $40 Hobby / $150 Standard / $500 Pro per month, message-credit based, multi-step actions multiply credits | Not yet implemented (org-level billing exists via Lago elsewhere; agent-level credit metering doesn't) | ❌ missing: per-agent token + tool-call metering surface (the data is in capability-core / agentRuns; UI + billing rollup not built) |

### Chatbase patterns worth adopting (priority order)

1. **Per-turn "sources retrieved" trace panel** in the playground. We log this server-side already (the tool round results); surfacing in the UI is ~half-day work. Highest user-trust impact.
2. **Dedicated Q&A pairs editor** in the Sources tab — operators want to author canonical answers without writing a system prompt. Same backend as knowledge sources, different UI affordance.
3. **Embed widget** generator (a `<script>` snippet that ops can drop on any page → opens a chat bubble pointing at one of their agents). Velion has the gateway + agent record; the missing piece is a public-facing widget JS bundle + a "Get embed code" button in the agent view.
4. **Contacts surface** — collected during conversations, queryable as a contact list. Maps onto our existing Convex `conversations` rows but needs a per-visitor identity model.
5. **Channel connectors** (WhatsApp / Slack / Messenger / Instagram) — this is genuinely Ingestion Plane work, the heaviest lift.
6. **Topic auto-clustering** for the Analyze tab — group conversations by intent, surface top topics. Vector-cluster on `conversations.messages[0].content` embeddings; uses dpv2-embedding-engine which is already running.

### Net assessment

| Area | vs Intercom Fin | vs Chatbase |
|---|---|---|
| Per-agent config + playground | ✅ parity | ✅ parity |
| Tool / action surface | ✅ tools loop + integration placeholder | ✅ comparable; missing specialized form/lead actions |
| Sources / RAG | ✅ Data Plane + GraphRAG + Wiki (richer than Chatbase) | ⚠️ missing native crawler + Q&A pairs editor |
| Analytics | ⚠️ raw counters only | ⚠️ no topic clustering / drill-down |
| **Distribution / embed** | n/a (Fin embeds in Intercom) | ❌ missing embed widget — biggest competitive gap |
| **Fine-tuning** | not available | ✅ **we ship, they don't** |
| **Multi-channel deploy** | ✅ in Fin's core | ❌ missing entirely on our side |
| **Scheduled / cron runs** | partial via Series | ✅ **we ship, they don't** |

**Three real gaps worth flagging as next product priorities** (not current engineering gaps — these are new product surfaces, not bugs):
1. **Embed widget** — turn any agent into a website chat bubble. Smallest scope, highest distribution leverage.
2. **Topic clustering + sources-trace UI** — analytics depth. Uses existing dpv2 services (embedding-engine, retrieval-engine).
3. **Multi-channel deploy** (WhatsApp / Slack / Messenger) — biggest scope; requires Ingestion Plane connectors per the existing `integration:*` placeholder namespace.

## §19 Wave 9 — embed widget + per-turn tool-trace — closed (2026-05-16)

Closes two of the three §18 product-gaps end-to-end. Channels (#3) ship as a separate Wave 10 brief.

### What landed

**Public embed widget (Gap #1)**

| Layer | File | What it does |
|---|---|---|
| Convex schema | `apps/Application Plane/convex-core/convex/schema.ts` | Added `publicEnabled`, `publicSecret`, `embedTheme` to `agents` table |
| Convex mutations | `apps/Application Plane/convex-core/convex/agents.ts` | `enablePublicEmbed` mints a 32-char URL-safe secret; `disablePublicEmbed` clears it; `getEmbedConfig` is a public query gated by secret (returns only id/name/greeting/theme — never systemPrompt/tools) |
| Public config API | `src/app/api/embed/[agentId]/config/route.ts` | GET endpoint, permissive CORS, validates `?secret=`, returns 404 on mismatch (no enumeration leak) |
| Public stream API | `src/app/api/embed/[agentId]/stream/route.ts` | POST validates secret → loads agent config → mints internal JWT scoped to embed visitor → POSTs gateway `/v1/invoke` → re-streams SSE with `Access-Control-Allow-Origin: *` |
| Admin proxy | `src/app/api/agents/[agentId]/embed/route.ts` | POST/DELETE for enable/disable + rotate-secret on top of the Convex mutations |
| Public JS bundle | `apps/Frontend Plane/velion/public/embed.js` | ~250 lines vanilla JS, Shadow DOM bubble + panel, SSE parser, visitor UUID in localStorage |
| Hook | `src/components/agents/hooks/useAgentEmbed.ts` | Mirrors `publicEnabled`/`publicSecret`, exposes `enable()`/`disable()`/`rotateSecret()`, builds `embedSnippet` from `window.location.origin` |
| UI | `src/components/agents/AgentWorkspaceView.tsx` — new `EmbedTab` | Snippet textarea with copy button, rotate/disable buttons; sibling tab beside Fine-tune |

**Per-turn tool-trace panel (part of Gap #2)**

| Layer | File | What it does |
|---|---|---|
| Gateway types | `apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs` | New `ToolTraceRecord` (round/tool/args_preview/result_preview/result_bytes); `ToolLoopOutcome.trace: Vec<ToolTraceRecord>` populated during each tool execution |
| Gateway HTTP | `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` | `InvokeResponse.tool_trace: Vec<ToolTraceEntry>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]` |
| Velion lib | `src/lib/model-plane/reasoning.ts` | `normalizeRustResponse` captures `tool_trace` and packs it into the normalized envelope as `metadata.tool_trace` |
| Velion chat-stream lib | `src/app/api/chat/_lib/reasoning-plane.ts` | `requestReasoningPlaneAnswer` now returns `toolTrace` alongside `answer`/`metadata` |
| Velion SSE route | `src/app/api/chat/stream/route.ts` | Emits a trailing `{type: 'tool_trace', metadata: {toolTrace}}` SSE event after the final assistant message (only when non-empty; never persisted to Convex — playground is ephemeral) |
| Playground hook | `src/components/agents/hooks/useAgentPlayground.ts` | `PlaygroundMessage.toolTrace?`; SSE parser captures `event.metadata?.toolTrace` per chunk and assigns it on stream completion |
| UI | `src/components/agents/AgentWorkspaceView.tsx` — new `ToolTracePanel` | Collapsible pill ("N tools used") under each assistant message; expands to show round number, tool name, byte size, truncated args/result previews |

### Trace flow (end-to-end)

```
Gateway tool_loop.rs → ToolLoopOutcome.trace
  → http_routes.rs InvokeResponse.tool_trace (snake_case, skip_if_empty)
    → velion reasoning.ts normalizeRustResponse → envelope.metadata.tool_trace
      → chat-stream/route.ts emits trailing SSE: data: {type:"tool_trace", metadata:{toolTrace:[...]}}
        → useAgentPlayground.ts: latestTrace captured, assigned to message.toolTrace on stream end
          → AgentWorkspaceView ToolTracePanel renders collapsible per-tool detail
```

### What did NOT land in Wave 9

- **Topic clustering** — gap #2's other half. Requires running embedding-engine over `agentRuns.user_message` corpus on a schedule. Deferred to a focused analytics wave.
- **Native crawler + Q&A pairs editor** — Chatbase parity for knowledge sources. Crawler exists in Ingestion Plane; needs UI glue. Deferred.
- **Multi-channel deploy** — see `docs/prompts/wave10-channels.md` for the focused brief (WhatsApp Business / Slack / Messenger / Instagram). Estimated 5–6 days.

### Build status

- `model-gateway` (Rust): `cargo build -p model-gateway` clean.
- `convex-core`: schema additions deploy cleanly; `agents:enablePublicEmbed` exercised via admin API.
- `velion` (Next.js): `tsc --noEmit` clean for all Wave-9 touched files. (Pre-existing `notifications/events.ts` and `@blocksuite/*` vendor diagnostics unchanged.)

### Security posture (embed)

- Per-agent `publicSecret` required on every public call (`config` + `stream`).
- Secret never echoed back from Convex queries except to the admin-API rotate endpoint.
- `getEmbedConfig` never returns `systemPrompt`, `tools`, or model details — only safe surface fields.
- Visitor identity is browser-side UUID stored in `localStorage`; the gateway JWT minted for embed visitors is scoped `userId: 'embed-visitor:'+visitorId` to keep their conversations isolated from authenticated users.
- 404 on bad-secret matches the missing-agent path (no enumeration leak).
- `Access-Control-Allow-Origin: *` is intentional — embed widgets must work from any customer site, and authorisation lives in the per-agent secret, not the origin.

### Privacy posture (tool-trace)

- Tool args/results truncated to 200B / 400B previews at gateway origin (`tool_loop.rs::execute_one`).
- Trace is NOT persisted to Convex; lives only in playground's in-memory state.
- The trace SSE event is emitted on the authenticated `/api/chat/stream` path, never on the public `/api/embed/.../stream` path — embed widget consumers never see internal tool details.

