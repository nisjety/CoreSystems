# velionv2 → Control Plane: Production Cutover & Verification (Phase 5)

Build/test verification is green (tsc 0 · 120 unit tests · `next build` ok). The
steps below are what's required to verify the wiring against a **live Control
Plane** and cut over to production. Nothing here can be confirmed from build
artifacts alone — it needs the cores running.

## 1. Environment matrix

| Var | Purpose | Local | Docker (in-cluster) |
|---|---|---|---|
| `NEXT_PUBLIC_APP_URL` | app origin | `http://localhost:3107` | `https://app.velion…` |
| `NEXT_PUBLIC_AUTH_BASE_URL` | Better Auth client base (= app origin) | `http://localhost:3107` | `https://app.velion…` |
| `AUTH_CORE_URL` | auth-core (proxy target) | `http://localhost:3011` | `http://auth-service:3011` |
| `USER_CORE_URL` | user-core | `http://localhost:3012` | `http://user-service:3012` |
| `ORG_SERVICE_URL` | org-core (+BREG) | `http://localhost:8080` | `http://org-core-service:8080` |
| `BILLING_SERVICE_URL` | billing-core | `http://localhost:3014` | `http://billing-core-service:3014` |
| `AUDIT_SERVICE_URL` | audit-core | `http://localhost:8187` | `http://audit-core-service:8187` |
| `INTERNAL_API_KEY` | service-to-service key (MUST equal Control Plane `INTERNAL_API_KEY`/`INTERNAL_SERVICE_SECRET`) | — | — |
| `BETTER_AUTH_TRUSTED_ORIGINS` | CSRF/redirect allowlist | app origin | all app origins |
| `RATE_LIMIT_ENABLED` | enable auth-core rate limiting | `true` | `true` |

> **Service-name alignment (prior drift risk):** the in-cluster defaults above
> match `apps/Control Plane/docker-compose.yml` `container_name:` values
> (`auth-service`, `user-service`, `org-core-service`, `billing-core-service`,
> `audit-core-service`). Confirm velionv2's runtime env points at these exact
> names and that velionv2 is attached to the `inter-plane-bus` network.

## 2. Pre-flight assertions
- `GET /api/v1/auth/config` returns `mode: "control-plane"` (NOT `standalone`). If `standalone`, `AUTH_CORE_URL` is unset → the prod auth route now returns **503** by design (fail-closed).
- `INTERNAL_API_KEY` present (else user-core/billing-core/audit-core calls return 503).
- auth-core has email/password + 2FA enabled; Google/Microsoft OAuth providers + `trustedOrigins` configured for the app origin.

## 3. Smoke checklist (live stack)
1. **Sign-in** → `/api/auth/*` proxies to auth-core; session cookie set; redirect resolves (`/onboarding` or `/dashboard`).
2. **2FA** (if enabled) → TOTP challenge verifies.
3. **Onboarding org step** → type a company name → BREG results appear (org-core → data.brreg.no) → select → continue → org created in org-core **with `brreg_data` snapshot + org_number** (verify in org-core DB / `GET /api/org/orgs/me`).
4. **Plan step** → free/trial persists (`POST /api/org/orgs/:id/plan`); paid → Stripe checkout redirect → return resumes at assembly.
5. **Complete** → `POST /api/v1/users/onboarding/complete`; redirect to `/dashboard`.
6. **Billing auto-provision** → after org create, billing-core has a free account (NATS `organization.created`). Verify `GET /api/org/orgs/:id/billing`.
7. **Paywall** → a premium route guarded by `requireEntitlement(feature)` redirects to upgrade when the plan lacks the feature; allows when granted.
8. **Settings** → billing card shows live plan/credits; members list live; **audit "Recent security events" shows live audit-core events**.
9. **Gating** → unauthenticated `/dashboard` → `/login` (proxy.ts); completed user hitting `/onboarding` → `/dashboard`.

## 4. E2E
`pnpm test:e2e` runs `tests/e2e/*.spec.ts` against `pnpm dev`:
- `cp-parity.spec.ts` gating tests run as-is (no backend).
- The mock-driven onboarding journey is `test.fixme` — validate the wizard selectors against the running UI, then un-fixme. It mocks the CP at the network boundary (no live stack needed for the journey assertions).
- `smoke.spec.ts` continues to cover the v1-parity surfaces.

## 5. Known follow-ups
- **Passkey**: requires adding the Better Auth passkey package (not in 1.6.11 as installed) + WebAuthn config (rpId/origin) in auth-core. Sign-in button shows an accurate "not enabled" message until then.
- **session-core (D4)**: deferred; enable Control Session authority via `CONTROL_SESSION_AUTHORITY_ENABLED=true` + `SESSION_SERVICE_URL` if durable cross-plane session lineage is needed.
- **Entitlement UX**: paywall is fail-closed — a billing-core outage denies premium features. Confirm this is the desired posture vs. a cached grace window.
