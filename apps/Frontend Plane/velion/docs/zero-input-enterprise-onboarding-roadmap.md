# Zero-Input Enterprise Onboarding Roadmap (Control Plane First)

## Scope
This roadmap operationalizes zero-input onboarding for Microsoft (Entra ID) across:
- `auth-core` (token custody and provider link events)
- `user-core` (identity enrichment + membership orchestration)
- `org-core` (tenant resolution + org lifecycle/onboarding state)
- `frontend` (post-login routing + trust/progress UX)

Goal: user clicks **Sign in with Microsoft** and lands in dashboard without manual org/profile forms.

---

## Target Outcome
After Microsoft sign-in, the platform should:
1. Identify tenant + domains.
2. Resolve existing org by tenant or auto-provision one.
3. Auto-create/enrich user profile.
4. Ensure membership and deterministic role assignment.
5. Route directly to dashboard with progress state.

---

## Delivery Plan

## Execution Status (Updated 2026-02-23)
- ✅ **Phase 0**: Principles and ownership documented (token custody + tokenRef contract direction).
- 🟡 **Phase 1**: Data model scaffolding implemented in `user-core` and `org-core` migrations.
- 🟡 **Phase 1.5**: `org-core` internal APIs started:
  - `GET /internal/orgs/by-tenant`
  - `POST /internal/orgs/ensure-from-tenant`
  - `POST /internal/orgs/:orgId/onboarding/state`
- ✅ **Slice A (user-core session context)**: Implemented endpoint + service/repository plumbing.
- ✅ **Slice B (provider enrichment in user-core)**: TokenRef validation + scope/expiry hydration + soft profile-hints enrichment added; auth callback now emits profileHints/scopes/tokenRef for both first-time OAuth registration and provider-linked events.
- 🟡 **Auth-core token exchange**: Internal token endpoint implemented (`/internal/oauth/token`) with tokenRef lookup and decryption; refresh flow scaffolded.
- ⏳ **Phase 2+**: Frontend post-login routing and provider-refresh orchestration still pending.

## Phase 0 — Contracts and Ownership (1–2 days)
- `auth-core` owns OAuth tokens (access/refresh/scopes/expiry).
- `user-core`/`org-core` consume only references (`tokenRef`) + identity metadata.
- Lock internal contract:
  - Event or endpoint payload: `AuthProviderLinked(userId, provider, providerUserId, tenantId, scopes, tokenRef)`.
- Add correlation id propagation (`x-correlation-id`) across internal calls.

Exit criteria:
- Signed contract in repo docs.
- Internal endpoint/event schema versioned.

## Phase 1 — Data Model Readiness (3–5 days)
- Extend `user-core` persistence for enterprise identity + memberships.
- Extend `org-core` persistence for tenant links + onboarding state.
- Ensure all `ensure` semantics are idempotent (`ON CONFLICT`).

Exit criteria:
- Migrations applied in local/dev.
- No breaking changes in existing API paths.

## Phase 2 — Microsoft Graph Enrichment (5–8 days)
- `user-core` requests short-lived access token from `auth-core` via `tokenRef`.
- Fetch `/me` (MVP) and `/me/photo/$value`.
- Optional early tenant enrichment from `/organization`.

Exit criteria:
- New sign-in receives enriched profile fields without user input.

## Phase 3 — Auto Org Provisioning (5–10 days)
- Deterministic resolver in `org-core`:
  - find by `(provider=microsoft, tenantId)`
  - else create org + tenant link.
- Role rule v1:
  - first membership in org => `OWNER`
  - subsequent => `MEMBER`

Exit criteria:
- Enterprise sign-ins auto-map to same org by tenant.

## Phase 4 — Zero-Input UX Flow (3–6 days)
- Frontend uses `GET /me/session-context`.
- Route to dashboard or progress shell based on onboarding status.
- Trust UI: show resolved org/domain/role/region.

Exit criteria:
- No mandatory onboarding form for enterprise users.

## Phase 5 — Connector Consent Upgrade (2–5 days)
- Ask consent after first value, not before.
- Track connector state and onboarding transition (`CONNECTORS_PENDING` → `COMPLETED`).

Exit criteria:
- Higher activation with deferred consent.

---

## Current Gap Analysis (Phase 1 baseline)

## `user-core` (today)
Already present:
- `users` with `last_login_at`, `onboarding_complete`.
- `provider_accounts` table (provider, provider_user_id, tenant_id, metadata).

Gaps:
- Missing explicit `tokenRef` linkage to `auth-core` token storage.
- Missing `scopes_granted[]` storage.
- Missing `last_synced_at` on provider link.
- Missing first-class `user_org_memberships` table in `user-core` domain.

## `org-core` (today)
Already present:
- `organizations` and `organization_members`.

Gaps:
- Missing `org_tenant_links` for deterministic tenant resolution.
- Missing `org_onboarding_states` table.
- Missing first-class org profile fields for enterprise defaults (`primary_domain`, `region`, `default_locale`).

---

## Phase 1 Work Started (implemented in this iteration)
1. `user-core`: migration scaffold for provider-link enrichment fields + `user_org_memberships`.
2. `org-core`: migration scaffold for org tenant links + onboarding states + enterprise org profile defaults.

Note:
- Existing plan enum (`free/pro/enterprise`) is left unchanged for backward compatibility.
- Role/bootstrap rules will be wired in service layer during Phase 3.

---

## API Changes Planned (next slices)
- `user-core`
  - ✅ `GET /me/session-context`
  - ✅ `POST /internal/users/enrich-from-provider`
  - ✅ `POST /internal/memberships/ensure`
- `org-core`
  - ✅ `GET /internal/orgs/by-tenant`
  - ✅ `POST /internal/orgs/ensure-from-tenant`
  - ✅ `POST /internal/orgs/:orgId/onboarding/state`
- `auth-core`
  - ✅ `POST /internal/oauth/token`
  - 🟡 `POST /internal/oauth/refresh` (scaffold only)

---

## Success Metrics
- Sign-in click → dashboard latency (`p50`, `p95`).
- Auto-org resolve/provision success rate.
- First-user-owner correctness rate.
- Time-to-first-value before connector upsell.
- Failure rates per internal step (token retrieval, enrichment, org ensure, membership ensure).

---

## Risks and Guardrails
- Race conditions during concurrent first logins → enforce unique constraints and idempotent upserts.
- Tenant mismatch or missing tenant info → fallback to domain-based provisional org + `CREATED` onboarding state.
- Scope drift over time → persist granted scopes snapshot and re-sync timestamp.

---

## Immediate Next Tasks
1. ✅ Wire `user-core` enrichment path to call `auth-core /internal/oauth/token` with `tokenRef`.
2. Implement provider-specific refresh behavior behind `auth-core /internal/oauth/refresh`.
3. Wire frontend post-login router to `session-context`.
4. Add `user-core` outbound orchestration to `org-core ensure-from-tenant` + `internal/memberships/ensure`.
5. Add observability (`correlationId`, step-level logs/metrics).

---

## Remaining Roadmap (Detailed Build Order)

## Slice A (next): user-core session context
- Add membership repository methods:
  - `EnsureMembership(userId, orgId, role, status)`
  - `GetPrimaryMembership(userId)`
- New endpoint:
  - `GET /api/v1/me/session-context`
  - Response: `{ userId, orgId, role, onboardingStatus }`
- Acceptance:
  - Existing user with membership gets deterministic context response.

## Slice B: provider enrichment contract in user-core
- Endpoint: `POST /internal/users/enrich-from-provider`
- Input: `userId, provider, providerUserId, microsoftTenantId, emailFromProvider, scopesGranted, tokenRef, profileHints`
- Behavior:
  - upsert `provider_accounts`
  - soft-update user profile (`displayName`, `avatar`, `locale`, `timezone`) only when placeholder/empty
- Acceptance:
  - endpoint is idempotent under retries.

## Slice C: auth-core token reference exchange
- Endpoint: `POST /internal/oauth/token`
- Input: `tokenRef`
- Output: short-lived `access_token` + expiry
- Acceptance:
  - user-core can fetch token without direct frontend token exposure.

## Slice D: Graph enrichment worker
- Read enrichment jobs/events from auth callback pipeline.
- Call Graph:
  - `/me`
  - `/me/photo/$value`
  - optional `/organization`
- Invoke `org-core ensure-from-tenant` + `user-core ensure membership`.

## Slice E: frontend post-login router
- Replace ad-hoc branching with `GET /me/session-context`.
- Route to:
  - progress shell when onboarding not completed
  - dashboard when completed

## Slice F: consent after first value
- Trigger connector consent once first useful result is observed.
- Transition onboarding state:
  - `CONNECTORS_PENDING` -> `COMPLETED`
