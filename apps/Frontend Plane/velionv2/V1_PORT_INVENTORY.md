# Velion V1 → V2 Port Inventory
> Read-only audit. Evidence-based (file:line). Generated 2026-05-30.

---

## 1. v1 Auth Architecture

### Summary
v1 uses **Better Auth** in pure proxy mode — there is **no local Better Auth server instance** in the Next.js app. All `/api/auth/*` calls are forwarded verbatim to an external `auth-core` service (Better Auth server, port 3011).

- `AUTH_ARCHITECTURE.md` source: `src/components/auth/AUTH_ARCHITECTURE.md`

### Auth client (`src/lib/auth-client.ts` or `src/components/auth/lib/auth-client.ts`)
```ts
import { createAuthClient } from "better-auth/react";
import { twoFactorClient } from "better-auth/plugins/two-factor";

export const authClient = createAuthClient({
  baseURL: process.env.NEXT_PUBLIC_AUTH_BASE_URL,
  plugins: [twoFactorClient({ twoFactorPage: "/login" })],
});
```
`NEXT_PUBLIC_AUTH_BASE_URL` points to the frontend's own origin (i.e. `http://localhost:3000`), NOT the backend. The browser calls `/api/auth/*` on the Next.js host, which then proxies to auth-core.

### Catch-all proxy route (`src/app/api/auth/[...path]/route.ts`)
- Accepts all HTTP methods (GET, POST, PUT, PATCH, DELETE, OPTIONS) via a single `handler`.
- Reads `AUTH_SERVICE_URL` (default: `http://auth-service:3011`) via `getAuthServiceUrl()` from `src/app/api/_lib/control-plane-auth.ts`.
- Forwards all headers (minus `host`), adds `x-internal-api-key` from `INTERNAL_API_KEY` env.
- Preserves cookies and query params for OAuth callback flows.
- Target URL pattern: `${AUTH_SERVICE_URL}/api/auth/${authPath}${queryString}`

### Control-plane-auth lib (`src/app/api/_lib/control-plane-auth.ts`)
Key exports and env vars:
| Export | Purpose |
|---|---|
| `getAuthServiceUrl()` | `AUTH_SERVICE_URL` \|\| `http://auth-service:3011` |
| `getUserServiceUrl()` | `USER_SERVICE_URL` \|\| `http://user-core:3012` |
| `getSessionServiceUrl()` | `SESSION_SERVICE_URL` \|\| `http://session-core-service:3017` |
| `getInternalApiKey()` | `INTERNAL_API_KEY` \|\| `INTERNAL_SERVICE_SECRET` (throws if missing) |
| `getOptionalInternalApiKey()` | Same but returns `''` if missing |
| `getCurrentSession(req)` | WeakMap-cached fetch to `GET ${AUTH_SERVICE_URL}/api/auth/get-session` |
| `requireSession(req)` | Throws `ControlPlaneAuthError(401)` if no session |
| `buildControlPlaneHeaders(req, session)` | Returns headers: `X-Internal-Api-Key`, `X-User-Id`, `X-User-Email`, `X-User-Name`, `X-User-Avatar`, `X-Correlation-Id` |

Session fetch calls `GET /api/auth/get-session` on auth-core directly (per G30 v3 comment — not the oRPC wrapper).

### `middleware.ts`
**No `middleware.ts` exists in v1.** Route protection is done at two levels:
1. **Server components**: `requireCompletedOnboarding()` / `requireOnboardingAccess()` server-side calls in page/layout server components (see `src/lib/auth/onboarding-access.ts` or equivalent).
2. **Client-side guard**: `<OnboardingGuard>` in `src/app/(dashboard)/layout.tsx` — wraps all dashboard routes.
3. **Dashboard layout** (`src/app/(dashboard)/layout.tsx`): renders `<OnboardingGuard>` as outermost wrapper before `<SidebarProvider>` etc.

The `OnboardingGuard` component (`src/components/onboarding/guards.tsx`) calls `authClient.useSession()` / `onboardingService.needsOnboarding()` client-side and redirects to `/login` or `/onboarding` as needed.

### Sign-in / Sign-up flow
- `authClient.signIn.email({ email, password })` → `POST /api/auth/sign-in/email` → `POST http://auth-service:3011/api/auth/sign-in/email`
- `authClient.signUp.email({...})` → `POST /api/auth/sign-up/email` → same proxy
- OAuth: `authClient.signIn.social({ provider: 'microsoft' | 'google' })` → `/api/auth/sign-in/social`
- After sign-in: redirect to `/auth/callback` (`src/app/(auth)/auth/callback/AuthCallbackClient.tsx`), which calls `onboardingService.needsOnboarding()` then routes to `/onboarding` or `/dashboard`.

### AUTH_ARCHITECTURE.md intended design (summary)
- Browser → Next.js `/api/auth/*` (catch-all proxy) → auth-core Better Auth server → PostgreSQL
- Session cookies set by auth-core, forwarded through proxy with `manual` redirect handling
- `NEXT_PUBLIC_AUTH_BASE_URL` = frontend origin (not backend) so Better Auth React client resolves cookies correctly
- Security: CSRF via Better Auth built-in, rate limiting on auth-core, 2FA via `twoFactorClient`, passkey/WebAuthn support

---

## 2. BREG Port Kit

### `brreg-service.ts` (`src/lib/services/brreg-service.ts`)

**Proxy base**: `const ORG_PROXY_BASE = '/api/org/api/v1'`

All requests go to the Next.js host (same-origin), which the `/api/org/[...path]` route handler proxies to org-core, which in turn calls `data.brreg.no`.

**Types**:
```ts
interface BrregAddress { land, landkode, postnummer, poststed, adresse: string[], kommune, kommunenummer }
interface BrregOrgForm { kode, beskrivelse }
interface BrregNaeringskode { kode, beskrivelse }
interface BrregEnhet {
  organisasjonsnummer: string   // "org number"
  navn: string
  organisasjonsform?: BrregOrgForm
  forretningsadresse?: BrregAddress
  postadresse?: BrregAddress
  naeringskode1?: BrregNaeringskode
  antallAnsatte?: number
  stiftelsesdato?: string
  hjemmeside?: string
  epostadresse?: string
  telefon?: string
  konkurs: boolean
  underAvvikling: boolean
  registreringsdatoEnhetsregisteret?: string
}
```

**Functions**:
- `brregService.searchByName(name: string, size = 10): Promise<BrregEnhet[]>`
  - `GET /api/org/api/v1/brreg/search?q={name}&size={size}`
- `brregService.getByOrgNumber(orgNumber: string): Promise<BrregEnhet>`
  - `GET /api/org/api/v1/brreg/enheter/{orgNumber}`
- `formatBrregAddress(enhet: BrregEnhet): string` — formats Norwegian address for display

### `BrregSearch.tsx` (`src/components/onboarding/ui/BrregSearch.tsx`)

**Props**:
```ts
interface BrregSearchProps {
  initialQuery?: string          // pre-fills the input
  onSelect: (enhet: BrregEnhet) => void
  onSkip: () => void
}
```

**Behavior**:
- Internal `useDebounce(query, 300ms)` hook
- `useQuery({ queryKey: ['brreg-search', debouncedQuery], queryFn: () => brregService.searchByName(debouncedQuery, 10), enabled: debouncedQuery.trim().length > 1, staleTime: 60_000 })`
- Dropdown shown when `debouncedQuery.length > 1 && !selected`
- `handleSelect(enhet)` → sets local `selected`, calls `onSelect(enhet)`
- `handleClear()` → resets state
- `onSkip` button → emits skip event (shows plain text input in parent)
- i18n via `useLanguageSwitch()` — Norwegian (`nb`) / English (`en`) copy

### `OrganizationStep.tsx` (canonical — `src/components/auth/onboarding/steps/OrganizationStep.tsx`)

**Props**: `{ machine: OnboardingMachine }`

**Internal `createOrganization` function** (file:line ≈ :70):
```ts
async function createOrganization(payload: {
  name: string; slug: string; orgNumber?: string;
  brregData?: BrregEnhet; fallbackError: string
}): Promise<CreatedOrganization> {
  const response = await fetch('/api/org/orgs', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({
      name: payload.name,
      slug: payload.slug,
      plan: 'free',
      ...(payload.orgNumber ? { org_number: payload.orgNumber } : {}),
      ...(payload.brregData ? { brreg_data: payload.brregData } : {}),
    }),
  })
}
```
- `sizeFromEmployeeCount(count)` maps `antallAnsatte` → `'solo'|'small'|'medium'|'large'|'enterprise'`
- `slugify(value)` — NFKD normalize, strip accents, replace non-alnum with `-`
- `handleBrregSelect(enhet)` → sets `name`, `orgNumber`, `employeeCount`, `brregData`, infers `size` from employee count
- On submit → calls `createOrganization`, then `machine.next()` (advances to `website` step)
- `brregSkipped` state toggles between `<BrregSearch>` and plain `<Input>`

**Also exists**: older `src/components/onboarding/core/OrganizationStep.tsx` — uses `useOrganizationStepState` reducer, calls `onboardingService.setupOrganization(data)`. The `auth/onboarding/steps/` version is the canonical one wired to `useOnboardingMachine`.

---

## 3. `/api/org` Proxy / Rewrite Config

**Mechanism**: Next.js **route handler** (NOT a `next.config` rewrite). File: `src/app/api/org/[...path]/route.ts`

The v1 `next.config.ts` rewrites section only covers `/api/contracts`, `/api/xero`, `/api/contifico` — org/auth/user/billing all use route handlers.

**Proxy internals**:
```ts
const ORG_SERVICE_URL = (process.env.ORG_SERVICE_URL || 'http://org-core:8080').replace(/\/+$/, '')
const BILLING_SERVICE_URL = (process.env.BILLING_SERVICE_URL || 'http://billing-core-service:3014').replace(/\/+$/, '')

// buildHeaders: calls requireSession(request) → ControlPlaneSession
// then calls buildControlPlaneHeaders(request, session) which adds:
//   X-Internal-Api-Key, X-User-Id, X-User-Email, X-User-Name, X-User-Avatar, X-Correlation-Id
```

**Smart routing within `/api/org/[...path]`**:
- `isBillingPath(path)` → fan-out to `BILLING_SERVICE_URL` (`/api/v1/billing/...`)
- `isQuotaCollectionPath(path)` / `isQuotaItemPath(path)` → billing quota endpoints
- `isPlanPath(path)` → `handlePlanPost` — orchestrates org-core update + billing sync
- `isCheckoutSessionPath(path)` → billing checkout session
- Everything else → `fetchUpstream(ORG_SERVICE_URL, buildOrgPath(request, path), ...)`
- `normalizeOrgProxyPayload(pathSegments, data)` normalizes response shape

**Session requirement**: every handler calls `requireSession(request)` which throws 401 if not authenticated — unauthenticated requests cannot reach org-core.

**BRREG path**: `GET /api/org/api/v1/brreg/search` passes straight through to org-core (no special routing), which proxies to `data.brreg.no`.

---

## 4. v1 Onboarding Flow

### Machine-backed wizard (`src/components/auth/onboarding/state/`)

**Steps** (in order, from `types.ts`):
```ts
export const ONBOARDING_STEPS = [
  'post-signin',    // PostSignInStep — intro/social-proof animation
  'organization',   // OrganizationStep — BREG search + org create
  'website',        // WebsiteStep — company domain input + Quarry crawl
  'connect',        // ConnectStep — Microsoft/connector consent (now deferred)
  'social-proof',   // SocialProofStep
  'paywall',        // PaywallStep — plan selection
  'assembly',       // AssemblyStep — completion animation
] as const
```
`STORAGE_KEY = 'velion.onboarding.v1'` (localStorage)

**Step files** (`src/components/auth/onboarding/steps/`): `PostSignInStep`, `OrganizationStep`, `WebsiteStep`, `ConnectStep`, `PaywallStep`, `SocialProofStep`, `AssemblyStep`, `_shared.tsx`

**State machine** (`useOnboardingMachine.ts`):
- SSR-safe hydration from localStorage on `requestAnimationFrame` (avoids SSR/client mismatch)
- `goTo(step)`, `next()`, `back()`, `setOrganization()`, `setWebsite()`, `setConnectors()`, `setRecommendation()`, `markIntroPlayed()`, `reset()`
- All mutations call `persist(updater)` which updates state + writes localStorage synchronously
- `INITIAL_STATE = { step: 'post-signin', connectors: [], introPlayed: false, startedAt: Date.now() }`

**Legacy redirect bridge** (`LegacyOnboardingRedirect.tsx`): maps old `/onboarding/<slug>` URLs via `LEGACY_SLUG_TO_STEP` to new step IDs and writes localStorage before redirecting to `/login`.

### Per-step backend calls (via `onboarding-service.ts`)

| Step | Backend call | Endpoint |
|---|---|---|
| Profile (legacy service path) | `userService.updateCurrentUserProfile()` | `PATCH /api/v1/users/me` via `/api/user` proxy |
| Organization | `orgService.createOrganization(orgData, userId)` | `POST /api/org/orgs` |
| Organization | `_pushOrgOnboardingState(orgId, 'ORG_CREATED', {...})` | `POST /api/org/internal/orgs/{orgId}/onboarding/state` (best-effort) |
| Website | `_ingestCrawlResultsToDataPlane(crawlJobId, orgId, url)` | Quarry/Data Plane (non-blocking) |
| Website | `_pushOrgOnboardingState(orgId, 'WEBSITE_CONFIGURED', {...})` | `POST /api/org/internal/orgs/{orgId}/onboarding/state` |
| Completion | `userService.markOnboardingComplete()` | `POST /api/v1/users/onboarding/complete` |
| Completion (state sync) | `pushOnboardingStateToServer(step, state)` | `PUT /api/user/me/onboarding-state` |

**`needsOnboarding()` logic** (canonical, G18):
1. `userService.getSessionContext()` → `/api/v1/me/session-context` on user-core — checks `onboardingStatus === 'COMPLETED'`
2. Fallback to localStorage `completed` flag
3. Fallback heuristic: `userService.getCurrentUser()` → checks `onboardingComplete` field

### Comparison with v2
| Aspect | v1 | v2 |
|---|---|---|
| State | localStorage via `useOnboardingMachine` (7 steps) | `useReducer` wizard (7 steps per CONTROL_PLANE_PARITY_AUDIT.md) |
| Backend calls | org-core, user-core, billing-core, Quarry (each step) | Only `POST /api/v1/onboarding/status` |
| Profile step | Calls `userService.updateCurrentUserProfile` | Not yet wired |
| Org step | Calls `orgService.createOrganization` + BREG | Not yet wired |
| Completion | `userService.markOnboardingComplete()` | Not yet wired |

---

## 5. v1 Session / Org Context Provider + Workspace Gating

### Client-side gating: `<OnboardingGuard>` (`src/components/onboarding/guards.tsx`)
- Imported in `src/app/(dashboard)/layout.tsx` as outermost wrapper
- Calls `authClient.useSession()` (Better Auth React hook) to check auth status
- Calls `onboardingService.needsOnboarding()` to check completion
- Redirects to `/login` (no session) or `/onboarding` (session but incomplete)
- Dashboard renders only when `authenticated && !needsOnboarding`

### Server-side gating: `control-plane-auth.ts` helpers
- `requireCompletedOnboarding(callbackUrl)` — server component/page function: checks `getAuthGateState()` → redirects to `/login` or `/onboarding`
- `requireOnboardingAccess()` — for onboarding pages themselves: redirects away if already complete

### `getAuthGateState()` (`src/lib/auth/onboarding-access.ts` or equivalent):
```ts
const user = await getCurrentAuthUser()          // calls GET /api/auth/get-session
const ctx = await readUserCoreOnboardingContext(user)  // calls /api/v1/me/session-context
// Returns: { user, onboardingComplete, onboardingStatus, source }
```

### Workspace context (current user + org)
- **Client**: `authClient.useSession()` gives `{ user, session }` everywhere
- **Server**: `requireSession(request)` → `ControlPlaneSession { user: { id, email, name, image } }`
- **Org context**: No dedicated React org context provider found in v1 — org data is fetched per-page/component from `/api/org/orgs/{orgId}` or via session-context
- `buildControlPlaneHeaders` injects `X-User-Id`, `X-User-Email` into every backend request so backends know the current user

---

## 6. v1 Env / Config Surface

### Server-side (route handlers, never exposed to browser)
```env
AUTH_SERVICE_URL=http://auth-service:3011
USER_SERVICE_URL=http://user-core:3012
ORG_SERVICE_URL=http://org-core:8080
BILLING_SERVICE_URL=http://billing-core-service:3014
SESSION_SERVICE_URL=http://session-core-service:3017
INTERNAL_API_KEY=<shared secret>          # or INTERNAL_SERVICE_SECRET
CONTROL_SESSION_AUTHORITY_ENABLED=true|false
```

### Client-side (`NEXT_PUBLIC_*`, embedded at build time)
```env
NEXT_PUBLIC_APP_URL=http://localhost:3000
NEXT_PUBLIC_AUTH_BASE_URL=http://localhost:3000   # Frontend's OWN origin (not auth-core directly)
NEXT_PUBLIC_AUTH_SERVICE_URL=http://localhost:3011  # for direct browser debug access
NEXT_PUBLIC_API_URL=http://localhost:3011/api
NEXT_PUBLIC_DEBUG_ONBOARDING=1     # enables verbose onboarding debug logging
NEXT_PUBLIC_DEBUG=1
```

### `next.config.ts` rewrites (v1)
Only three service rewrites — **org/auth/user/billing use route handlers, not rewrites**:
```js
rewrites: [
  { source: '/api/contracts/:path*', destination: '${CONTRACT_MANAGEMENT_URL}/:path*' },
  { source: '/api/xero/:path*',      destination: '${XERO_SERVICE_URL}/api/xero/:path*' },
  { source: '/api/contifico/:path*', destination: '${CONTIFICO_SERVICE_URL}/:path*' },
]
```
Also exposes `serverRuntimeConfig` with `CONTRACT_MANAGEMENT_URL`, `XERO_SERVICE_URL`, `CONTIFICO_SERVICE_URL`.

---

## 7. Port Recommendations

| Item | v1 Source | Recommendation |
|---|---|---|
| `auth-client.ts` | `src/lib/auth-client.ts` (or `src/components/auth/lib/auth-client.ts`) | **Reuse as-is** — v2 already has identical content. Confirm `NEXT_PUBLIC_AUTH_BASE_URL` is set to frontend origin. |
| `/api/auth/[...path]/route.ts` catch-all proxy | `src/app/api/auth/[...path]/route.ts` | **Reuse as-is** — v2 already has this (proxies to auth-core). |
| `control-plane-auth.ts` | `src/app/api/_lib/control-plane-auth.ts` | **Reuse as-is** — v2 already has this. Verify `getOptionalInternalApiKey` is used everywhere optional, `getInternalApiKey` where required. |
| `brreg-service.ts` | `src/lib/services/brreg-service.ts` | **Reuse as-is** — copy verbatim. Proxy base `/api/org/api/v1` must resolve through v2's `/api/org/[...path]` route handler. |
| `BrregSearch.tsx` | `src/components/onboarding/ui/BrregSearch.tsx` | **Reuse as-is** — self-contained, uses `brregService` + TanStack Query + `useLanguageSwitch`. |
| `OrganizationStep.tsx` (auth version) | `src/components/auth/onboarding/steps/OrganizationStep.tsx` | **Adapt** — wire `createOrganization` function (lines ~70-100) into v2's useReducer wizard. The `sizeFromEmployeeCount`, `slugify`, and `handleBrregSelect` helpers are direct copy-paste. |
| `/api/org/[...path]/route.ts` | `src/app/api/org/[...path]/route.ts` | **Port with adaptation** — v2 does not have this yet. Copy the full file. Change `requireSession` import path if needed. The billing fan-out logic (plan/checkout/quota paths) is complex but self-contained. |
| `useOnboardingMachine` + `types.ts` | `src/components/auth/onboarding/state/` | **Adapt** — v2 uses `useReducer` already; merge the step list and payload shapes. `ONBOARDING_STEPS` and `OrganizationPayload` (with `brregOrgNumber`, `employeeCount`) should be imported or replicated. `STORAGE_KEY = 'velion.onboarding.v1'` must match for resume compatibility. |
| `onboarding-service.ts` | `src/components/onboarding/services/onboarding-service.ts` | **Selectively port** — extract `setupOrganization` (org-core call), `completeProfile` (user-core call), `needsOnboarding` (G18 session-context check), and `completeOnboarding` (marks complete in user-core). v2 currently only hits `/api/v1/onboarding/status` — these are the missing wires. |
| `OnboardingGuard` | `src/components/onboarding/guards.tsx` | **Port** — v2 needs a client-side guard in its dashboard layout. Can simplify to just `authClient.useSession()` + `requireCompletedOnboarding` pattern. |
| `middleware.ts` | Not present in v1 | **Skip** — v1 does not use Next.js middleware for auth. Use server component guards + client `OnboardingGuard` pattern instead. |
| Env vars | See section 6 | **Add to v2**: `ORG_SERVICE_URL`, `BILLING_SERVICE_URL`, `USER_SERVICE_URL`, `SESSION_SERVICE_URL`, `INTERNAL_API_KEY`. `NEXT_PUBLIC_AUTH_BASE_URL` should be frontend origin. |
| `next.config` rewrites | Contracts/Xero/Contifico only | **Not needed for core port** — org/auth/user/billing stay as route handlers. |
