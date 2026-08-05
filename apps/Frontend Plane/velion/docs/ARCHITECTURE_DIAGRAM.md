# Frontend Plane Architecture

**Last Updated:** 2025-01-27

## Pyramid Placement

The **Frontend Plane** sits at **Layer 6** (top) of the CoreSystem pyramid.
It is the outermost presentation layer — the only layer that runs in the
user's browser. Every piece of data it displays and every action it triggers
flows through the Application Plane (L5) APIs; it **never** calls Control,
Data, Ingestion, or Model Plane services directly.

### Authority Rules

- **Canonical owner** of UI rendering, client-side routing, browser state,
  and design-system tokens.
- **Does NOT own** user identity, organisation data, documents, embeddings,
  AI reasoning, collaborative sync, or notifications — consumes those via
  Application Plane (L5) endpoints only.
- All authentication state is obtained via JWTs issued by auth-core (L1)
  and validated by the Application Plane gateway before reaching the
  frontend.

## 🏗️ Service Structure

```
┌──────────────────────────────────────────────────────────┐
│                  FRONTEND PLANE  (Layer 6)               │
│                                                          │
│  ┌────────────────────┐  ┌──────────────────────────┐    │
│  │  verevon (Next.js)  │  │  triodelab-web (static)  │    │
│  │  :3000             │  │                          │    │
│  └─────────┬──────────┘  └────────────┬─────────────┘    │
│            │                          │                  │
│            ▼                          ▼                  │
│     Application Plane (L5) APIs only                     │
└──────────────────────────────────────────────────────────┘
```

## 📦 Service Responsibilities

### 1. **verevon** (Port 3000)

**Domain**: Primary web application  
**Technology**: Next.js (TypeScript)  
**Owns**:
- ✅ Page routing & navigation
- ✅ Component tree & layout
- ✅ Client-side state management
- ✅ Design-system tokens & theming
- ✅ Server-side rendering for initial page loads

### 2. **triodelab-web**

**Domain**: Static marketing / documentation site  
**Technology**: Static build  
**Owns**:
- ✅ Public-facing content pages
- ✅ Marketing assets

---

## Does NOT Own

| Capability           | Canonical Owner                 | How Frontend Accesses It          |
| -------------------- | ------------------------------- | --------------------------------- |
| User identity / JWT  | auth-core (L1 Control)         | Reads cookie set by auth-core     |
| Organisation data    | org-core (L1 Control)          | Via Application Plane API         |
| Documents / files    | documents-service (L2 Data)    | Via Application Plane API         |
| Embeddings / search  | retrieval-service (L2 Data)    | Via Application Plane API         |
| AI reasoning         | ai-core (L4 Model)             | Via Application Plane API         |
| Collaborative sync   | convex-backend (L5 Application)| WebSocket via convex-gateway      |
| Notifications        | notification-core (L5 App)     | Via Application Plane API         |

## Cross-Plane Contract Rules

> Amended per [ADR 0003 — L5 boundary policy](./adr/0003-l5-boundary-policy.md)
> (accepted 2026-05-09). Rules 5–7 ratify the existing verevon-as-L5-ingress
> pattern; the previous "L5 is the ceiling" wording overstated the constraint
> and was contradicted by every running route.

1. **API-only access** — Frontend never imports server-side packages from
   lower layers. All cross-plane data flows through HTTP/RPC.
2. **No direct DB access** — Frontend has zero database connections.
3. **Auth cookie only** — Authentication flows through HTTP-only cookies
   issued by auth-core. The frontend never stores raw OAuth tokens (refresh
   tokens live exclusively in auth-core; see ADR 0002 and verevon-gap.md §2.1).
4. **Environment-driven URLs** — Backend URLs come from environment variables.
   No hard-coded service hosts.
5. **Verevon proxy as L5 ingress** — verevon's `src/app/api/*` route handlers
   are the canonical L5 ingress for the Frontend Plane. They validate sessions
   (`control-plane-auth.ts`), mint internal auth headers, propagate correlation
   IDs, and forward to L1–L4 cores over the shared Docker network. New proxy
   routes **must** use the shared helper rather than re-implementing auth
   forwarding (enforced by `scripts/lint-proxy-routes.sh`).
6. **Convex-gateway scope** — `convex-gateway` is reserved for WebSocket
   fan-out of reactive workspace data (Application Plane, Layer 5). It does
   not mediate REST traffic for L1–L4.
7. **Second-frontend trigger** — when a second frontend (mobile, admin
   console, public web) ships, supersede ADR 0003 with a new ADR that adopts
   a shared gateway pattern.

## Network Topology

> Per [ADR 0004 — Docker network topology](./adr/0004-network-topology.md)
> (proposed 2026-05-11). Renamed from `verevon-net` on cutover.

Docker networks split into two roles:

- **Plane-specific networks** — one per plane, scopes intra-plane traffic.
  `controlplane-net`, `app-net`, `dpv2-net`, `ingestion-net`,
  `model-plane-network`. Containers within a plane talk to each other
  over these. A new service in plane X **must** attach to plane X's
  network even if `inter-plane-bus` is also attached.

- **`inter-plane-bus`** — the cross-cutting bus where cross-plane edges
  live. Verevon (L5 ingress per ADR 0003) reaches every plane's public
  surface over this bus. notification-core's subscriber to
  `app.session.*` events (G14) flows here. Auth-core JWKS reads from
  model-plane services flow here. **Not** for intra-plane traffic.

Renamed from `verevon-net` per ADR 0004 — the old name conflated "verevon's
network" with "the shared bus." A future ADR may shrink the bus to only
the cross-plane edges; see ADR 0004 § "Forcing function for Option B."

---

# Component Hierarchy & Architecture

## System Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Root App (layout.tsx)                     │
│                                                                  │
│  ┌─ SessionProvider ─────────────────────────────────────────┐ │
│  │                                                            │ │
│  │  ┌─ DashboardLayout ──────────────────────────────────┐  │ │
│  │  │                                                    │  │ │
│  │  │  ┌─ Sidebar ─┐  ┌─ Navbar ─┐  ┌─ Content ──────┐ │  │ │
│  │  │  │           │  │          │  │                 │ │  │ │
│  │  │  │ Nav Links │  │ Actions  │  │ MasterPageLayout├─┤  │ │
│  │  │  │           │  │ Profile  │  │ (with auth)    │ │  │ │
│  │  │  └─────────┘  │ Notif     │  │                 │ │  │ │
│  │  │               └────────────┘  │ GridContainer  │ │  │ │
│  │  │                               │  ├─ StatCard   │ │  │ │
│  │  │                               │  ├─ CommonCard │ │  │ │
│  │  │                               │  └─ SectionCard│ │  │ │
│  │  │                               └─────────────────┘ │  │ │
│  │  └────────────────────────────────────────────────────┘  │ │
│  │                                                            │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Component Hierarchy

### Page Level
```
Page (dashboard/{section}/page.tsx)
  ↓
MasterPageLayout
  ├─ Header (title + description)
  └─ Content Area
     ├─ GridContainer
     │  ├─ StatCard
     │  ├─ StatCard
     │  └─ StatCard
     └─ SectionCard
        └─ CommonTable or CustomContent
```

### Module Level
```
MasterPageLayout
  ├─ LoadingState (while fetching)
  ├─ ErrorState (on API error)
  ├─ EmptyState (no data)
  └─ Content
     ├─ GridContainer
     │  └─ StatCard[]
     ├─ SectionCard
     │  └─ CommonCard[]
     └─ CommonModal (for actions)
```

### Card Progression
```
Page Content
  ↓
GridContainer (responsive grid)
  ↓
CommonCard (base component)
  ├─ StatCard (for metrics)
  ├─ SectionCard (for grouped content)
  └─ Custom (extends CommonCard)
```

## Data Flow Architecture

```
Component
  ↓
useSession (check auth)
  ↓
useFetch (get data)
  ─→ apiGet (with retries)
     ↓
     [Success]           [Error]
     ↓                        ↓
  setData          ErrorState / retry
     ↓
  render(data)
```

## State Management Pattern

```
Page Component (async)
  ↓
getSession() → SessionProvider
  ↓
useFetch() → custom hook
  ↓
ApiResponse<T>
  ├─ { success: true, data }
  ├─ { success: false, error }
  └─ { success: false, error: timeout }
     ↓
render(LoadingState | ErrorState | Content)
```

## File Import Paths

```
Component Usage:
  import { MasterPageLayout, StatCard } from '@/components/shared'
  import { useFetch } from '@/lib/common-hooks'
  import { apiGet } from '@/lib/api-utils'
  import type { User } from '@/lib/common-types'
  import { useSession } from '@/lib/session-context'
  import { THEME } from '@/lib/design-tokens'
```

## Shared Components Map

```
┌─ Layouts
│  ├─ MasterPageLayout (full page)
│  ├─ GridContainer (responsive grid)
│  └─ CommonModal (dialog)
│
├─ Cards
│  ├─ CommonCard (base)
│  ├─ SectionCard (grouped)
│  ├─ StatCard (metrics)
│  └─ ActivityCard
│
├─ States
│  ├─ LoadingState (skeletons)
│  ├─ EmptyState (no data)
│  ├─ ErrorState (error)
│  └─ ComingSoonState
│
├─ UI Elements
│  ├─ Button (primary, secondary, danger)
│  ├─ Badge (status labels)
│  ├─ Divider (separator)
│  ├─ Input (form field)
│  ├─ Select (dropdown)
│  └─ Textarea
│
└─ Special
   ├─ CommonTable (list view)
   ├─ CommonForm (form builder)
   ├─ CommonPagination
   └─ CommonSearchBar
```

## Integration Flow

```
Step 1: Setup
  └─ Wrap app in SessionProvider

Step 2: Use in Pages
  └─ MasterPageLayout + useSession + useFetch

Step 3: Build UI
  └─ GridContainer + Cards + Buttons

Step 4: Handle Data
  └─ useOptimisticMutation + apiPost

Step 5: Show States
  └─ LoadingState | ErrorState | EmptyState | Content
```

## Common Patterns

### Pattern 1: Feature Page
```typescript
export default async function Page() {
  const session = await getSession()
  if (!session) redirect('/auth/login')
  
  return (
    <MasterPageLayout title="Projects">
      <GridContainer cols={3}>
        {projects.map(p => <StatCard key={p.id} ... />)}
      </GridContainer>
    </MasterPageLayout>
  )
}
```

### Pattern 2: Data Fetching
```typescript
function MyComponent() {
  const { user } = useSession()
  const { data, isLoading, error } = useFetch('/api/projects')
  
  if (isLoading) return <LoadingState />
  if (error) return <ErrorState onRetry={refetch} />
  if (!data) return <EmptyState />
  
  return <div>{data}</div>
}
```

### Pattern 3: Create/Update
```typescript
function CreateModal({ onClose }) {
  const { mutate, isPending } = useOptimisticMutation(
    data => apiPost('/api/projects', data)
  )
  
  return (
    <CommonModal onSubmit={() => mutate(formData)}>
      <CommonForm fields={[...]} />
    </CommonModal>
  )
}
```

## Dependency Graph

```
app/layout.tsx
  └─ SessionProvider
     └─ Components using useSession()

Dashboard Pages
  ├─ MasterPageLayout
  ├─ GridContainer
  ├─ StatCard/CommonCard
  └─ useFetch + apiGet

Modal Components
  └─ CommonModal
     └─ CommonForm
        └─ Button

List Views
  └─ CommonTable
     └─ Button + Badge
```

## Type Safety Flow

```
Backend API Response
  ↓
ApiResponse<T> (defined type)
  ↓
useFetch<T> (returns typed data)
  ↓
Component receives typed data
  ↓
TypeScript ensures correct usage
```

## Performance Optimization

```
Code Splitting:
  └─ Each section loads MasterPageLayout + GridContainer (cached)

Caching:
  └─ Design tokens cached (THEME)
  └─ API responses cached (useFetch options)

Reusability:
  └─ StatCard used 100+ times
  └─ CommonCard used 50+ times
  └─ Button used 200+ times
```

