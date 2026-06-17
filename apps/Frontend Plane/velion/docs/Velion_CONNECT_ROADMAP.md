# Velion Connect — Frontend Roadmap

> **Last verified against code**: 2026-05-09. When this doc and code disagree, code wins. For cross-cutting integration contracts see [velion-gap.md](../velion-gap.md); for architectural decisions see [docs/adr/](./adr/).

**Product 1: Knowledge / Connect**  
**Goal:** User connects company → system understands organization → team collaborates with company knowledge via search + chat.

---

## Architecture Overview

```
APPLICATION PLANE (this repo: apps/frontend)
        ↑
REALTIME COLLABORATION (apps/Application Plane/convex-core)
        ↑
REASONING PLANE (apps/Reasoning Plane/ai-core)
        ↑
DATA PLANE (Qdrant + Postgres)
        ↑
INGESTION PLANE (apps/Ingestion Plane/Quarry + imports-core)
        ↑
CONTROL PLANE (apps/Control Plane: auth-core, user-core, org-core)
-----------------------------
ORCHESTRATION (Temporal in Quarry)
SHARED INFRA  (NATS, Redis, Postgres clusters)
OBSERVABILITY (metrics, tracing, audit)
SECURITY      (mTLS, tenant isolation, GDPR)
```

**Frontend Stack:** Next.js 15 (App Router), TypeScript, shadcn/ui, Tailwind, Better Auth  
**Design System:** `apps/frontend/docs/base-design.md`  
**Backend Ports:** auth-service:3011, user-service:3012, org-core:8080  

---

## Route Structure

```
/                         → redirect /dashboard
/sign-in                  → (auth) sign-in page  [✅ done]
/sign-up                  → (auth) sign-up page
/onboarding/profile       → Step 1: who are you  [✅ exists]
/onboarding/organization  → Step 2: create / join org  [✅ exists]
/onboarding/website       → Step 3: connect company URL  [✅ shipped]
/onboarding/connect       → Step 4: connect data sources  [✅ shipped]
/onboarding/team          → Step 5: invite teammates  [✅ exists]
/onboarding/complete      → Step 6: finish  [✅ exists]
# Plan selection is DEFERRED — every new org gets plan='free' on creation
# and may upgrade later from /settings/billing. Was originally drafted as
# step 6 in this roadmap; the active wizard is 6 steps. See velion-gap.md G20.
/dashboard                → main home  [✅ done]
/search                   → global search workspace  [✅ Phase 4]
/chat                     → chat workspace  [✅ Phase 5]
/chat/[id]                → specific conversation  [✅ Phase 5]
/knowledge                → knowledge viewer  [✅ Phase 6]
/knowledge/sources        → sources list  [✅ Phase 6]
/knowledge/documents      → document library  [✅ Phase 6]
/team                     → team & permissions  [✅ Phase 7]
/settings                 → user / org settings  [✅ skeleton]
/settings/profile
/settings/integrations
/settings/billing
```

---

## Phase Status

| Phase | Name | Status | Target |
|-------|------|--------|--------|
| 1 | Auth + Entry | ✅ Complete | Week 1 |
| 2 | Onboarding Flow | ✅ Complete | Week 2–3 |
| 3 | Main Dashboard | ✅ Complete | Week 3–4 |
| 4 | Global Search | ✅ Complete | Week 5–6 |
| 5 | Chat Workspace | ✅ Complete | Week 6–8 |
| 6 | Knowledge View | ✅ Complete | Week 8–9 |
| 7 | Team & Permissions | ✅ Complete | Week 9–10 |
| 8 | Realtime (Convex) | 🟡 Partial — Chat (Phase 5) ships with Convex realtime; the Control Session aggregator + notification-core's `app.session.*` subscriber landed in Wave 3 (velion-gap.md §8.17 / ADR 0002). The remaining "Convex projection of the Control Session" is filed as **velion-gap.md G35**. Full reactive workspace + presence + activity feed still pending. | Week 10–12 |

---

## Phase 1 — Auth + Entry ✅

### Completed
- [x] `src/proxy.ts` — edge route protection (Next.js 15+ `proxy.ts` convention; was previously `middleware.ts`). Validates the session against auth-core `/api/v2/auth/getSession` once per request and caches the verdict by `sha256(cookieHeader)` for 30s. Accepts the cookie names `better-auth.session_token`, `auth_session`, `idknuten.sid`, `idknuten.session_token`, `session_token`, plus three `__Secure-*` patterns. See [velion-gap.md](../velion-gap.md) §8.2 for the gate's implementation and the cache caveats.
- [x] Sign-in page default redirect → `/dashboard`
- [x] OAuth callback default redirect → `/dashboard`
- [x] `/dashboard` route exists (was 404 due to Next.js route group)
- [x] `OnboardingGuard` wired into `(dashboard)/layout.tsx`
- [x] Full-bleed background on dashboard (layout passes children directly, page owns its canvas)
- [x] Root `/` redirect → `/dashboard`

### Post-auth decision flow
```
sign-in ──► auth callback
              ├── hasOrg + hasProfile → /dashboard
              └── else → /onboarding/profile
                           └── OnboardingService.getCurrentStep()
```

### Auth providers
- Microsoft OAuth (primary) — via Better Auth
- Google OAuth (optional)
- Email/password fallback
- Passkeys (configured)

---

## Phase 2 — Onboarding Flow ✅

**North star:** User should feel "this AI understands our company" within 5 minutes of completing onboarding.

### Steps

#### Step 1: Profile `/onboarding/profile`
**Status:** ✅ Component exists (`ProfileStep.tsx`)  
**Collects:** firstName, lastName, displayName, jobTitle, timezone  
**Calls:** `user-service:3012 POST /api/v1/profile`

#### Step 2: Organization `/onboarding/organization`
**Status:** ✅ Component exists (`OrganizationStep.tsx`)  
**Collects:** org name + slug, or invitation code to join existing  
**Norwegian:** BrregSearch to verify org number from Enhetsregisteret  
**Calls:** `org-core:8080 POST /api/v1/organizations`

#### Step 3: Connect Company Website `/onboarding/website` 🔲
**Status:** 🔲 MISSING — most critical step  
**This is the product moment.**  
**UI flow:**
```
Enter company URL: [_______________]
                    ▼
Live status ticker:
  ◉ Discovering pages
  ◉ Mapping site structure
  ◉ Extracting content
  ◉ Building knowledge base
  ◉ Done — 247 pages indexed
```
**Calls:** `Quarry (Ingestion Plane) POST /api/v1/crawl`  
**Realtime:** Convex subscription to job status  
**Files to create:**
- `src/app/(onboarding)/onboarding/website/page.tsx`
- `src/components/onboarding/core/WebsiteStep.tsx`
- `src/components/onboarding/ui/CrawlStatusTicker.tsx`

#### Step 4: Connect Data Sources `/onboarding/connect` 🔲
**Status:** 🔲 MISSING  
**UI:** Microsoft permission checklist (SharePoint, OneDrive, Teams, Outlook)  
**Calls:** Ingestion Plane connectors  
**Files to create:**
- `src/app/(onboarding)/onboarding/connect/page.tsx`
- `src/components/onboarding/core/ConnectStep.tsx`
- `src/components/onboarding/ui/PermissionChecklist.tsx`

#### Step 5: Team Setup `/onboarding/team`
**Status:** ✅ Component exists (`TeamStep.tsx`)  
**Collects:** invite emails + roles (admin / member)  
**Calls:** `org-core:8080 POST /api/v1/organizations/{id}/members`

#### ~~Step 6: Choose Plan~~ — DEFERRED
**Status:** Auto-assign `plan='free'` on org create. Upgrade flow lives at `/settings/billing`, not in onboarding. Not on the MVP path. See velion-gap.md G20.

#### Step 6: Complete `/onboarding/complete`
**Status:** ✅ Component exists (`CompleteStep.tsx`)  
**Action:** Mark onboarding done, redirect → `/dashboard`

### Onboarding Service
`src/components/onboarding/services/onboarding-service.ts`  
- Uses `localStorage` to persist step state  
- `needsOnboarding()` → checks `hasProfile` + `hasOrg`  
- `getCurrentStep()` → returns current incomplete step

### Phase 2 Completed
- [x] `WebsiteStep` component + `/onboarding/website` page
- [x] `CrawlStatusTicker` with realtime job polling + simulation fallback
- [x] `ConnectStep` component + `/onboarding/connect` page  
- [x] `PermissionChecklist` (Microsoft 365 OAuth scopes)
- [x] Wire step 3+4 into `onboarding-service.ts` state machine
- [x] Update `OnboardingGuard`/`getCurrentStep()` to include new steps
- [x] Polish all steps to match `base-design.md` (no rounding, 1px borders)
- [x] 6-step progress indicator across all onboarding pages

---

## Phase 3 — Main Dashboard ✅

**Current state:** Live module cards with real data from Quarry and org-core APIs.  
**Completed:** Photo card placeholder grid replaced with 4 module cards (2×2 grid, 1px separator lines).

### Dashboard Layout
```
────────────────────────────────────────────
Greeting + Search Bar     |    Chat Tab
────────────────────────────────────────────
Knowledge Status Card  |  Sources Card
Recent Activity        |  Team Activity
Crawl Status           |  Integrations
────────────────────────────────────────────
```

### Dashboard Cards (real data)
| Card | Source | Status |
|------|--------|--------|
| Website Knowledge | Quarry job status | ✅ |
| Documents | Quarry documents API | ✅ |
| Sources | Quarry sources API | ✅ |
| Team Activity | org-core members | ✅ |
| Crawl Status | Quarry realtime + polling | ✅ |
| Recent Conversations | future Chat | 🔲 |

### Phase 3 Completed
- [x] Replace placeholder category cards with real module cards
- [x] `KnowledgeStatusCard` — crawled pages count + last crawl timestamp
- [x] `SourcesCard` — source count + document count
- [x] `TeamActivityCard` — member count, solo empty state
- [x] `CrawlStatusCard` — live ingestion phase + polling when running
- [x] Empty states for each card (Norwegian copy, `#C8C1B3` muted)
- [x] `/api/ingestion/crawl` proxy routes (POST + GET)
- [x] `/api/ingestion/crawl/[id]/status` proxy route
- [x] Stats polling: `setInterval(fetchStats, 10_000)` when crawl running
- [x] Search hero → `router.push('/search?q=...')` on Enter

---

## Phase 4 — Global Search ✅

**Centerpiece of the product.** Glean-like search over org knowledge.

### Files created
- [x] `src/app/(dashboard)/search/page.tsx`
- [x] `src/components/search/SearchInput.tsx`
- [x] `src/components/search/SearchResults.tsx`
- [x] `src/components/search/CitationCard.tsx`
- [x] `src/components/search/StreamingAnswer.tsx`
- [x] `src/lib/api/search-api.ts` (SSE stream, mock fallback)

---

## Phase 5 — Chat Workspace ✅

### Files created
- [x] `src/app/(dashboard)/chat/page.tsx`
- [x] `src/app/(dashboard)/chat/[id]/page.tsx`

### Chat Types
1. **Personal Chat** — User ↔ Org AI (private)
2. **Collaborative Chat** — Team shared thread (Convex realtime)

### Layout
```
┌──────────────┬─────────────────────────────┐
│ Sidebar      │ Conversation                │
│  My Chats    │  Messages stream            │
│  Shared      │  Sources sidebar            │
│  Saved       │  Reasoning toggle           │
└──────────────┴─────────────────────────────┘
```

### Realtime (Convex)
- Live message delivery
- Typing indicators
- Shared thread state
- Presence

### Files to create (Phase 5)
- `src/app/(dashboard)/chat/page.tsx`
- `src/app/(dashboard)/chat/[id]/page.tsx`
- `src/components/chat/ChatSidebar.tsx`
- `src/components/chat/ConversationView.tsx`
- `src/components/chat/MessageBubble.tsx`
- `src/components/chat/SourcesSidebar.tsx`
- `src/lib/api/chat-api.ts`
- `src/lib/realtime/convex-chat.ts`

---

## Phase 6 — Knowledge View ✅

### Files created
- [x] `src/app/(dashboard)/knowledge/page.tsx`
- [x] `src/app/(dashboard)/knowledge/sources/page.tsx`
- [x] `src/app/(dashboard)/knowledge/documents/page.tsx`
- [x] `src/components/knowledge/SourcesList.tsx`
- [x] `src/components/knowledge/IndexStatus.tsx`

---

## Phase 7 — Team & Permissions ✅

### Files created
- [x] `src/app/(dashboard)/team/page.tsx`
- [x] `src/components/team/MembersTable.tsx`
- [x] `src/components/team/InviteModal.tsx`
- [x] `src/components/team/RoleBadge.tsx`

---

## Phase 8 — Realtime Collaboration 🔲

Convex (`apps/Application Plane/convex-core`) powers:
- Shared chat threads
- Crawl progress updates
- Ingestion job status
- Member presence
- Activity feed

### Integration points
- `NEXT_PUBLIC_CONVEX_URL` env var
- `ConvexProvider` in root layout
- Individual hooks per feature

---

## MVP Checklist

### Must Have (Phases 1–5)
- [x] Auth (Microsoft OAuth + email)
- [x] Org creation (create + join via invitation code)
- [x] URL scrape (Quarry integration)
- [x] Dashboard (real data, cookies forwarded)
- [x] Search (org knowledge) — proxied via /api/ai/search → Reasoning Plane /stream/chat
- [x] Chat (personal + team) — proxied via /api/chat/stream → Reasoning Plane /stream/chat
- [x] Team invite

### Not MVP
- HR / Marketing modules
- Automation & workflows
- Velion builder UI
- Analytics dashboards
- Billing UI (auto-assign free plan)

---

## Design System Reference

All UI must follow `apps/frontend/docs/base-design.md`:

| Token | Value |
|-------|-------|
| Background | `#F4F1EB` |
| Surface (cards) | `#EAE6DF` |
| Border | `#D8D2C6` |
| Primary text | `#2B2B2B` |
| Secondary text | `#4A4A48` |
| Accent | `#FF2E63` |
| Accent hover | `#FF4D7A` |
| Heading font | Cormorant Garamond (serif, weight 400) |
| Body font | Inter (sans-serif) |

**Rules:**
- No glassmorphism
- No heavy gradients
- 1px borders only
- Outline icons (no fill)
- Generous whitespace — whitespace is design
- UI must feel: controlled, quiet, precise

---

## Current Session: Next Actions

Starting now, in order:

1. **Phase 2 — Step 3: Website connect** (`/onboarding/website`)
   - `WebsiteStep.tsx` component
   - `CrawlStatusTicker.tsx` realtime poll
   - Page + route
   
2. **Phase 2 — Step 4: Data sources** (`/onboarding/connect`)
   - `ConnectStep.tsx` component
   - Microsoft permission checklist UI
   
3. **Phase 2 — Progress bar** across all onboarding steps

4. **Phase 2 — Design pass** on all existing onboarding steps (match base-design.md)

5. **Phase 3 — Dashboard cards** (replace placeholders with real modules)
