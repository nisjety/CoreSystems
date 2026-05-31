# Backend Contract Catalog

Generated 2026-05-30. Evidence-based: file:line cited for every claim.

---

## Endpoint Catalog

### auth-core (:3011) — Better Auth, base path `/api/auth`

All Better Auth routes are under `/api/auth/*` (set via `baseURL` env; `auth-core/src/auth/auth.ts:219`).
Auth mechanism for all BA routes: **session cookie** (`idknuten.sid` by default; cookie name from env `SESSION_COOKIE_NAME`, default `sid`).

| Method | Path | Auth | Request body (key fields) | Response | Status | Handler file:line |
|--------|------|------|--------------------------|----------|--------|-------------------|
| POST | `/api/auth/sign-up/email` | none | `email: string, password: string, name?: string` | `{ user, session }` | 200/400 | Better Auth built-in (emailAndPassword plugin; `auth.ts:323`) |
| POST | `/api/auth/sign-in/email` | none | `email: string, password: string, rememberMe?: bool` | `{ user, session }` + Set-Cookie | 200/401/422 | Better Auth built-in; rate-limited: 3 req/10s (`auth.ts:229`) |
| POST | `/api/auth/sign-out` | session cookie | `{}` | `{ success: true }` + clears cookie | 200 | Better Auth built-in |
| GET | `/api/auth/get-session` | session cookie | — | `{ session: {...}, user: { id, email, name, image, emailVerified } }` or `null` | 200 | Better Auth built-in |
| POST | `/api/auth/forget-password` | none | `email: string, redirectTo?: string` | `{ status: true }` | 200/400 | Better Auth built-in (emailAndPassword; `auth.ts:328`) |
| POST | `/api/auth/reset-password` | none | `token: string, newPassword: string` | `{ status: true }` | 200/400 | Better Auth built-in |
| GET/POST | `/api/auth/verify-email` | none | `?token=...` (GET) or `{ token }` (POST) | redirect to callbackURL (`/dashboard`) | 200/302 | Better Auth built-in; `callbackURL` = `FRONTEND_URL/dashboard` (`auth.ts:371`) |
| POST | `/api/auth/two-factor/enable` | session cookie | `{ password: string }` | `{ totpURI, backupCodes[] }` | 200/401 | Better Auth twoFactor plugin (`auth.ts:938`) |
| POST | `/api/auth/two-factor/verify-totp` | session cookie | `{ code: string }` | `{ status: true }` + session update | 200/401; rate-limited 5/300s (`auth.ts:234`) | Better Auth twoFactor plugin |
| POST | `/api/auth/two-factor/verify-otp` | session cookie | `{ code: string }` | `{ status: true }` | 200/401; rate-limited 5/300s (`auth.ts:238`) | Better Auth twoFactor plugin |
| POST | `/api/auth/two-factor/send-otp` | session cookie | `{}` | `{ status: true }` | 200; rate-limited 2/60s (`auth.ts:241`) | Better Auth twoFactor plugin |
| GET | `/api/auth/sign-in/social` | none | `?provider=google\|microsoft\|vipps\|okta&callbackURL=...` | redirect to provider | 302 | Better Auth socialProviders (`auth.ts:1088`) |
| GET | `/api/auth/callback/:provider` | none | OAuth callback | redirect to frontend | 302 | Better Auth built-in |

**Plane Token routes** — `auth-core/src/auth/plane-token.controller.ts`:

| Method | Path | Auth | Request | Response | Status | file:line |
|--------|------|------|---------|----------|--------|-----------|
| GET | `/api/:audience/token` | session cookie (Better Auth) | — | `{ token: string, expiresAt: string, userId, orgId, role }` RS256 JWT | 200/400/401/404 | `plane-token.controller.ts:105` |
| POST | `/api/:audience/internal-token` | `X-Internal-Api-Key` header | `{ userId: string, orgId: string, email?: string, scopes?: string[] }` | `{ token: string, expiresAt: string }` RS256 JWT | 200/400/403/404 | `plane-token.controller.ts:168` |

Audiences: `data-plane`, `quarry`, `ingestion`, `control-plane`, `application-plane`. Unknown audience → 404.
Model-plane has its own controller: `model-plane-token.controller.ts:83` — same pattern, same return shape.

---

### user-core (:3012) — Go/Gin

Auth mechanism: **`X-Internal-Api-Key` header** (matches env `INTERNAL_API_KEY` / `INTERNAL_SERVICE_SECRET`) **or Bearer token** (validated via auth-core `/api/auth/get-session`). User identity forwarded via `X-User-Id` header.
Handler file: `user-core/internal/http/handlers.go`, routes registered at `user-core/internal/http/server.go:71`.

| Method | Path | Auth | Request body (key fields) | Response (key fields) | Status | file:line |
|--------|------|------|--------------------------|----------------------|--------|-----------|
| GET | `/api/v1/users/me` | internal key + X-User-Id | — | `{ user: { id, email, name, display_name, avatar, email_verified, onboarding_complete, status, account_status, position, department, first_name, last_name, phone, location, timezone, created_at, updated_at, last_login_at } }` | 200/401/500 | `handlers.go:139` |
| PATCH | `/api/v1/users/me` | internal key + X-User-Id | `{ name?, displayName?, avatar?, firstName?, lastName?, phoneNumber?, officeLocation?, timezone?, position?, department?, status? }` | same user object minus `created_at/email_verified/onboarding_complete` | 200/400/401/500 | `handlers.go:303` |
| DELETE | `/api/v1/users/me` | internal key + X-User-Id | — | `{ success: true }` | 200/401 | `server.go:91` |
| GET | `/api/v1/me/session-context` | internal key + X-User-Id | — | `{ userId: string, orgId?: string, role?: string, onboardingStatus: string }` | 200/401/500 | `handlers.go:236` |
| POST | `/api/v1/users/onboarding/complete` | internal key + X-User-Id (or `?email=`) | no body required; `?email=` fallback if no user_id in ctx | `{ success: true, message: string }` | 200/400/500 | `handlers.go:544` |
| GET | `/api/v1/users/me/onboarding-state` | internal key + X-User-Id | — | `{ step: string, state?: object }` | 200/401/500 | `handlers.go:594` |
| PUT | `/api/v1/users/me/onboarding-state` | internal key + X-User-Id | `{ step: string, state?: object }` | `{ success: true }` | 200/400/401/500 | `handlers.go:610` |
| GET | `/api/v1/users/by-email/:email` | internal key | — | `{ user: { id, email, name, avatar, email_verified, status, created_at, updated_at } }` | 200/404 | `handlers.go:511` |
| GET | `/api/v1/users/:id` | internal key | — | `{ user: { id, email, name, avatar, email_verified, status, created_at, updated_at } }` | 200/404 | `handlers.go:476` |
| POST | `/api/v1/api-keys` | internal key + X-User-Id | `{ name: string, description?: string, scopes?: string[], expires_at?: RFC3339 }` | `{ api_key: { id, name, description, prefix, scopes, expires_at, created_at }, key: string }` (key shown once) | 201/400/401 | `handlers.go:644` |
| GET | `/api/v1/api-keys` | internal key + X-User-Id | — | `{ api_keys: [...], total: int }` | 200/401 | `handlers.go:708` |
| DELETE | `/api/v1/api-keys/:id` | internal key + X-User-Id | — | `{ message: string }` | 200/404 | `handlers.go:755` |
| GET | `/api/v1/preferences` | internal key + X-User-Id | — | `{ preferences: { theme, language, timezone, notifications: { email: bool, push: bool } } }` | 200/401 | `handlers.go:790` |
| PATCH | `/api/v1/preferences` | internal key + X-User-Id | `{ theme?, language?, timezone?, notifications?: { [key]: bool } }` | `{ message: string }` | 200/400 | `handlers.go:857` |
| GET/PUT | `/api/v1/settings/appearance` | internal key + X-User-Id | PUT: `{ theme, colorScheme, fontSize, compactMode }` | `AppearanceSettings` | 200/400 | `handlers.go:997,1021` |
| GET/PUT | `/api/v1/settings/language` | internal key + X-User-Id | PUT: `{ language, region, dateFormat, timeFormat }` | `LanguageSettings` | 200/400 | `handlers.go:1049,1073` |
| GET/PUT | `/api/v1/settings/privacy` | internal key + X-User-Id | PUT: `{ shareStatus, shareActivity, allowAnalytics, dataRetention, telemetryEnabled, crashReporting }` | `PrivacySettings` | 200/400 | `handlers.go:1100,1126` |
| GET/PUT | `/api/v1/settings/notifications` | internal key + X-User-Id | PUT: `{ emailNotifications, pushNotifications, teamsNotifications, calendarReminders, quietHours: { enabled, start, end } }` | `NotificationSettings` | 200/400 | `handlers.go:1153,1180` |
| GET/PUT | `/api/v1/settings/security` | internal key + X-User-Id | PUT: `{ twoFactorEnabled, sessionTimeout, loginAlerts, trustedDevicesEnabled }` | `SecuritySettings` | 200/400 | `handlers.go:1219,1244` |
| GET/PUT | `/api/v1/settings/accessibility` | internal key + X-User-Id | PUT: `{ highContrast, reducedMotion, screenReaderOptimized, keyboardShortcutsEnabled }` | `AccessibilitySettings` | 200/400 | `handlers.go:1283,1308` |
| GET/PUT | `/api/v1/settings/ai` | internal key + X-User-Id | PUT: `{ aiEnabled, modelPreference, dataCollectionEnabled, personalizationEnabled, memoryEnabled }` | `AISettings` | 200/400 | `handlers.go:1348,1373` |
| GET/PUT | `/api/v1/settings/storage` | internal key + X-User-Id | PUT: `{ autoSync, clearCacheOnLogout, compressionEnabled, offlineAccessEnabled }` | `StorageSettings` | 200/400 | `handlers.go:1413,1438` |

**`GET /api/v1/me/session-context` — full response shape** (`users/types.go:213`):
```json
{ "userId": "string", "orgId": "string|omitted", "role": "string|omitted", "onboardingStatus": "CREATED|PROFILE_READY|..." }
```
Note: `orgId` and `role` are resolved live from org-core `/orgs/me` + `/orgs/:id/members` at call time (`handlers.go:259`). No `plan` in this response — plan lives in billing-core.

**POST `/api/v1/users/onboarding/complete`** — only marks `onboarding_complete = true` on the user row (`handlers.go:547`). Does NOT create org or billing. Does NOT publish NATS. Org creation and billing provisioning happen via separate flows.

---

### session-core — Go/Gin

Handler files: `session-core/internal/http/server.go:54`, `handlers.go`, `control_session_handlers.go`.

**Verdict: DUAL-PURPOSE — agent-run routes are NOT frontend-facing; control-session routes ARE.**

| Method | Path | Auth | Purpose | Response | Status | file:line |
|--------|------|------|---------|----------|--------|-----------|
| POST | `/v1/sessions` | internal key / bearer | Create agent-run session | `{ session_id, ... }` | 200 | `handlers.go` (agent-run, Model Plane, NOT for frontend) |
| GET | `/v1/sessions/:id/state` | internal key | Get agent-run state | session state | 200/404 | `handlers.go` |
| GET | `/v1/sessions/:id/events` | internal key | SSE stream of events | text/event-stream | 200 | `handlers.go` (SSE, no write timeout) |
| POST | `/v1/sessions/:id/messages` | internal key | Send message to agent | event | 200 | `handlers.go` |
| POST | `/v1/sessions/:id/approvals/:approval_id` | internal key | Resolve approval | — | 200 | `handlers.go` |
| POST | `/v1/sessions/:id/resume` | internal key | Resume paused session | — | 200 | `handlers.go` |
| **GET** | **`/api/v1/sessions/current`** | **internal key + X-User-Id** | **Control Session snapshot (G10)** | `{ ...snap }` aggregated from user-core | **200/401/502** | `control_session_handlers.go:16` |
| **POST** | **`/api/v1/sessions/refresh`** | **internal key + X-User-Id** | **Force re-aggregation, publishes `app.session.entitlements_changed`** | `{ ...snap }` | **200/401/502** | `control_session_handlers.go:48` |

The `/api/v1/sessions/*` routes (Control Session) are intended for frontend calls. The `/v1/sessions/*` routes are Model Plane agent-run routes, not meant to be called directly by the frontend.

---

### org-core (:8080) — Go/Gin

Auth: `X-Internal-Api-Key` header required on all routes except `/health`.
Routes registered at `org-core/internal/http/server.go:65`.
Handler file: `org-core/internal/http/handlers.go`.
Note: routes exist under both `/api/v1/...` and bare `/orgs/...` prefix (the bare prefix is the canonical frontend-proxy surface).

| Method | Path | Auth | Request body | Response | Status | file:line |
|--------|------|------|-------------|----------|--------|-----------|
| POST | `/orgs` | internal key + `x-user-id` header | `{ name: string (req), slug?: string, plan?: string, org_number?: string, brreg_data?: object }` | `Organization` object (full, re-fetched from DB) | 201/400/401/500 | `handlers.go:60` |
| GET | `/orgs` | internal key + `x-user-id` | — | `[]Organization` | 200/401 | `handlers.go:43` |
| GET | `/orgs/me` | internal key + `x-user-id` | — | `[]Organization` (user's orgs) | 200/401 | `handlers.go:43` |
| GET | `/orgs/:id` | internal key | — | `Organization` | 200/404 | `handlers.go:29` |
| GET | `/orgs/:id/entitlements` | internal key | — | `{ organization_id: string, entitlements: object }` | 200/404 | `handlers.go:285` |
| GET | `/orgs/:id/members` | internal key | — | `{ members: OrgMember[], count: int }` | 200/500 | `handlers.go:304` |
| POST | `/orgs/:id/members/invite` | internal key + `x-user-id` | `{ email: string (req), role?: "owner"\|"admin"\|"member"\|"viewer" }` | `{ invitation_id, status: "active"\|"pending", message, email, role }` | 200 (active) / 202 (pending) / 400 | `handlers.go:320` |
| DELETE | `/orgs/:id/members/:userId` | internal key | — | `{ ok: true }` | 200/500 | `handlers.go:388` |
| GET | `/orgs/:id/members/search` | internal key | `?q=string&limit=n` | `{ results: MemberSuggestion[], query, count }` | 200/400 | `handlers.go:645` |
| POST | `/orgs/:id/plan` | internal key + `x-user-id` | `{ plan: string (req), reason?: string }` | `Organization` | 200/400/404 | `handlers.go:256` |
| PATCH | `/orgs/:id/capabilities` | internal key | `{ capabilities: { [feature]: bool } }` | `Organization` | 200/400/404 | `handlers.go:214` |
| PATCH | `/orgs/:id/brreg` or `/api/v1/organizations/:id/brreg` | internal key | `{ org_number: string (req), brreg_data?: object, verification_status?: string }` | `Organization` | 200/400/404 | `handlers.go:177` |
| GET | `/api/v1/brreg/search` | internal key | `?q=string&size?=n(max 50)` | `{ results: Enhet[], count: int }` | 200/400/502 | `handlers.go:137` |
| GET | `/api/v1/brreg/:orgnr` | internal key | — | `Enhet` object | 200/404/502 | `handlers.go:160` |
| GET | `/orgs/:id/roles/catalog` | internal key | — | RBAC catalog | 200 | `server.go:97` |
| GET/POST | `/orgs/:id/roles` | internal key | — / `{ ... }` | roles | 200/201 | `server.go:98` |
| PATCH | `/orgs/:id/roles/:roleName` | internal key | `{ ... }` | role | 200 | `server.go:99` |
| DELETE | `/orgs/:id/roles/:roleName` | internal key | — | — | 200 | `server.go:100` |
| PATCH | `/orgs/:id/members/:userId/role` | internal key | `{ role: string }` | — | 200 | `server.go:101` |
| GET | `/internal/orgs/by-tenant` | internal key | `?provider=&tenantId=` | `Organization` | 200/404 | `handlers.go:400` |
| POST | `/internal/orgs/ensure-from-tenant` | internal key | `{ provider, tenantId (req), displayName, primaryDomain, domains[], region, defaultLocale }` | `{ organization, created: bool }` | 200/201 | `handlers.go:423` |
| POST | `/internal/orgs/:orgId/onboarding/state` | internal key | `{ status, steps }` | `{ ok: true }` | 200 | `handlers.go:466` |

**`Enhet` response shape** (`org-core/internal/brreg/client.go:46`):
```json
{ "organisasjonsnummer": "string", "navn": "string", "organisasjonsform": {...}, "forretningsadresse"?: {...}, "postadresse"?: {...}, "hjemmeside"?: "string", "naeringskode1"?: {...}, "antallAnsatte"?: int, "konkurs": bool, "underAvvikling": bool, "stiftelsesdato"?: "string", "epostadresse"?: "string", "telefon"?: "string" }
```

**`Organization` response shape** (`org-core/internal/org/types.go`; confirmed fields from handler):
Includes `id`, `name`, `slug`, `plan`, `status`, `org_number?`, `brreg_data?`, `verification_status`, `metadata`, `created_at`, `updated_at`.

---

### billing-core (:3013) — Go/Gin

Auth: `X-Internal-Api-Key` header required on all routes except `/health`.
Routes registered at `billing-core/internal/http/server.go:53`.
Handler file: `billing-core/internal/http/handlers.go`.

| Method | Path | Auth | Request body | Response | Status | file:line |
|--------|------|------|-------------|----------|--------|-----------|
| GET | `/api/v1/billing/orgs/:orgId/account` | internal key | — | `Account` | 200/500 | `handlers.go:21` |
| PUT | `/api/v1/billing/orgs/:orgId/account` | internal key | `Account` fields (plan, subscriptionState, credits, products, featureFlags, entitlements, quotaLimits, ...) | `Account` | 200/400/500 | `handlers.go:31` |
| POST | `/api/v1/billing/orgs/:orgId/usage` | internal key | `{ metric: string (req), quantity: float64 (req), event_id?, source?, occurred_at?: RFC3339, metadata? }` | `{ status: "usage recorded" }` | 202/400/500 | `handlers.go:55` |
| GET | `/api/v1/billing/orgs/:orgId/entitlements/:feature` | internal key | — | `{ org_id, feature, allowed: bool, plan: string, required: bool }` | 200 (allowed) / **402** (not allowed) | `handlers.go:98` |
| GET | `/api/v1/billing/orgs/:orgId/quotas/:metric` | internal key | — | `{ quota: QuotaStatus, remaining_readable: string }` | 200/500 | `handlers.go:122` |
| POST | `/api/v1/billing/orgs/:orgId/checkout-session` | internal key | `{ plan: string (req), success_url: string (req), cancel_url: string (req) }` | `{ id: string, url: string }` (Stripe checkout URL) | 201/400/502 | `handlers.go:182` |
| POST | `/api/v1/billing/orgs/:orgId/invoices` | internal key | `{ amount_cents: int64 (req), provider?, currency?, due_at?: RFC3339, auto_charge: bool, metadata? }` | `{ status: "invoice created" }` | 201/400 | `handlers.go:139` |

**`Account` shape** (`billing-core/internal/billing/types.go:28`):
```json
{ "org_id": "string", "plan": "string", "subscription_state": "trialing|active|past_due|canceled", "credits": int64, "products": {[key]:bool}, "feature_flags": {[key]:bool}, "entitlements": {[key]:bool}, "quota_limits": {[key]:float64}, "provider_customer_id": {[key]:string}, "metadata": {...}, "updated_at", "created_at" }
```

**`QuotaStatus` shape** (`types.go:53`):
```json
{ "org_id", "metric", "limit": float64, "used": float64, "remaining": float64, "is_exceeded": bool, "utilization": float64 }
```

**402 entitlement response** (`handlers.go:113`):
```json
{ "org_id": "string", "feature": "string", "allowed": false, "plan": "string", "required": true }
```

---

## session-core Role Verdict

**SPLIT**: The `/v1/sessions/*` routes (agent-run sessions: create, state, events SSE, messages, approvals, resume) are **not frontend-facing** — they belong to the Model Plane pipeline and are called by backend services. The `/api/v1/sessions/current` (GET) and `/api/v1/sessions/refresh` (POST) routes (G10 Control Session) **are frontend-facing** and return a snapshot of the user's active session state including entitlements. Auth on both groups: `X-Internal-Api-Key` + `X-User-Id` (forwarded by the frontend proxy).

Source: `session-core/internal/http/server.go:54–91`.

---

## NATS Event Map

| Subject | Publisher | Publisher file:line | Consumer | Consumer file:line | Trigger |
|---------|-----------|--------------------|-----------|--------------------|---------|
| `organization.created` | auth-core (via `organizationEventsPlugin` → `AuthEventPublisher`) | `auth-core/src/auth/organization-events.plugin.ts:49` | billing-core subscriber | `billing-core/internal/nats/subscriber.go:70` | Better Auth DB hook fires after org row created; billing-core calls `SyncOrganization` to auto-provision free account |
| `organization.created` | org-core `SharedPublisher.PublishOrgCreated` | `org-core/internal/org/service_enhanced.go:29` (interface) | billing-core subscriber (same subject) | `billing-core/internal/nats/subscriber.go:70` | POST /orgs handler calls service which calls publisher |
| `organization.updated` | org-core `SharedPublisher.PublishOrgUpdated` | `org-core/internal/org/service_enhanced.go:30` (interface) | billing-core subscriber | `billing-core/internal/nats/subscriber.go:90` | Any org field update |
| `organization.plan.changed` | org-core `SharedPublisher.PublishPlanChanged` | `org-core/internal/org/service_enhanced.go:33` (interface) | billing-core subscriber | `billing-core/internal/nats/subscriber.go:114` | POST /orgs/:id/plan → `service.UpdatePlan` |
| `organization.deleted` | org-core SharedPublisher | `org-core/internal/org/service_enhanced.go:32` (interface) | billing-core subscriber | `billing-core/internal/nats/subscriber.go:135` | Org deletion; billing-core calls `DeactivateOrganization` |
| `organization.member.added` | auth-core organizationEventsPlugin | `organization-events.plugin.ts:87` | no consumer found in this index | — | Better Auth member DB hook |
| `usage.>` (wildcard) | any service that publishes to `usage.*` | (multiple callers; POST /api/v1/billing/orgs/:orgId/usage also accepted via HTTP) | billing-core subscriber | `billing-core/internal/nats/subscriber.go:26` | Usage events from any plane |
| `user.created` | user-core `PublishUserCreated` | `user-core/internal/nats/publisher.go:24` | no consumer found in this index | — | User creation |
| `user.updated` | user-core `PublishUserUpdated` | `user-core/internal/nats/publisher.go:45` | no consumer found in this index | — | User update |
| `session.created` | session-core (subject constant defined) | `org-core/internal/nats/events.go:23` (constant), session-core impl | no consumer found in this index | — | Agent-run session created |

**Key finding**: `organization.created` is published by **both** auth-core (via Better Auth DB hook when an org is created through the BA organization plugin) AND by org-core (via SharedPublisher when POST /orgs is called directly). billing-core consumes both on the same subject and calls `SyncOrganization` idempotently — free plan is auto-provisioned on org creation. `org-core/internal/nats/events.go:10–15` defines the subject constants.

---

## Auth-core Prod Config

Source: `auth-core/src/auth/auth.ts`

| Setting | Value / Env var | file:line |
|---------|----------------|-----------|
| `secret` | `BETTER_AUTH_SECRET` (required in prod) | `auth.ts:218` |
| `baseURL` | `BETTER_AUTH_URL` (default `http://localhost:3011`) | `auth.ts:219` |
| `trustedOrigins` | Computed from: `localhost:3000`, `BETTER_AUTH_URL`, `FRONTEND_URL`, `NEXT_PUBLIC_APP_URL`, `BETTER_AUTH_TRUSTED_ORIGINS`, `AUTH_ALLOWED_ORIGINS` | `auth.ts:59–69` |
| Rate limiting | Enabled when `RATE_LIMIT_ENABLED=true`; Redis storage; global 100 req/60s | `auth.ts:223–250` |
| Rate limit `/sign-in/email` | 3 req / 10s window | `auth.ts:229` |
| Rate limit `/two-factor/verify-totp` + `/verify-otp` | `RATE_LIMIT_2FA_VERIFY_MAX` / `RATE_LIMIT_2FA_VERIFY_WINDOW` (default 5/300s) | `auth.ts:234–244` |
| Rate limit `/two-factor/send-otp` | `RATE_LIMIT_OTP_SEND_MAX` / `RATE_LIMIT_OTP_SEND_WINDOW` (default 2/60s) | `auth.ts:241–244` |
| Session cookie name | `SESSION_COOKIE_NAME` (default `sid`) | `auth.ts:283` |
| Cookie prefix | `COOKIE_PREFIX` (default `idknuten`) | `auth.ts:265` |
| Session expiry | `SESSION_EXPIRES_IN` seconds (default 604800 = 1 week) | `auth.ts:530` |
| Session update age | `SESSION_UPDATE_AGE` (default 3600 = 1h) | `auth.ts:531` |
| Session fresh age | `SESSION_FRESH_AGE` (default 300 = 5min) | `auth.ts:532` |
| Cookie cache | `SESSION_COOKIE_CACHE_ENABLED` (on by default); maxAge `SESSION_COOKIE_CACHE_MAX_AGE` (default 1800 = 30min) | `auth.ts:534–535` |
| Secure cookies | `USE_SECURE_COOKIES_AUTO=true` → auto by NODE_ENV; default always secure | `auth.ts:268–272` |
| SameSite | `COOKIE_SAME_SITE` (default `lax`) | `auth.ts:289` |
| Cross-subdomain cookies | `CROSS_SUBDOMAIN_COOKIES_ENABLED=true` + `COOKIE_DOMAIN` required | `auth.ts:275–279` |
| emailAndPassword enabled | `EMAIL_PASSWORD_ENABLED=true` | `auth.ts:324` |
| Require email verification | `REQUIRE_EMAIL_VERIFICATION=true` | `auth.ts:325` |
| Organization plugin | `ORGANIZATION_ENABLED=true` | `auth.ts:676` |
| 2FA | Active when `REQUIRE_2FA_ON_FIRST_SIGNIN=true` or `REQUIRE_2FA_ON_NEW_IP=true` | `auth.ts:935` |
| Secondary storage (sessions) | Redis (`db/redis.ts`) | `auth.ts:217` |
| Token encryption | `TOKEN_ENCRYPTION_KEY` (base64, 32-byte AES-256-GCM) | `auth.ts:198` |
| Email provider | `RESEND_API_KEY` | `auth.ts:138` |
| Social: Google | `GOOGLE_CLIENT_ID` + `GOOGLE_CLIENT_SECRET` | `auth.ts:1098` |
| Social: Microsoft | `MICROSOFT_CLIENT_ID` + `MICROSOFT_CLIENT_SECRET` + `MICROSOFT_TENANT_ID` | `auth.ts:1112` |
| Social: Vipps | `VIPPS_CLIENT_ID` + `VIPPS_CLIENT_SECRET` | `auth.ts:577` |

---

## Gaps / Ambiguities

1. **`/api/v1/me/session-context` does not include `plan`** — plan data must be fetched separately from billing-core `GET /api/v1/billing/orgs/:orgId/account`. The session context only surfaces `userId`, `orgId`, `role`, `onboardingStatus`.

2. **`OnboardingStatus` values are not enumerated in a const** — the handler sets `"CREATED"` and `"PROFILE_READY"` in `user-core/internal/http/handlers.go:265–266`. Other states (e.g. `"COMPLETE"`) are implied but not codified in types.go as an enum.

3. **`organization.created` dual-publish risk** — both auth-core and org-core publish to `organization.created`. billing-core's `SyncOrganization` appears idempotent (upsert pattern), but this should be verified in `billing-core/internal/billing/service.go` before relying on the auto-provision guarantee.

4. **`/api/v1/settings/*` auth on user-core** — all settings routes are protected by the same `authContextMiddleware` that requires `X-Internal-Api-Key`. The frontend must route through the edge proxy which injects this header; direct calls from the browser will 401.

5. **session-core `/v1/sessions/*` agent-run handlers** — actual handler source is in `session-core/internal/http/handlers.go` but was not read; the route shapes for those endpoints (particularly `createSession` request body) are known from `service_service.go:71` (domain.CreateSessionRequest) but the HTTP wire format was not confirmed.

6. **billing-core `checkout-session` adapter** — the response `CheckoutSession{ id, url }` is confirmed at `billing/types.go:81`. The `url` field is the Stripe-hosted checkout URL. The adapter is at `billing-core/internal/adapters/stripe/adapter.go` (not read).

7. **`usage.>` publishers** — who emits usage events to NATS (as opposed to HTTP POST) was not confirmed. Only billing-core's subscriber is evidenced; the actual emitting services (likely Model Plane runtime) are outside the Control Plane index.
