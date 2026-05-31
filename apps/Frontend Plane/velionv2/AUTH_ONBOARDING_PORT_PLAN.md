# Velion 1 → Velion 2: Auth + Onboarding faithful port

Goal: make the V2 auth page and onboarding flow an **almost copy‑paste of V1**
(correct UI/UX + real backend), add the missing auth capabilities, and wire
**full NO/EN i18n** across both surfaces.

Source of truth (V1): `../velion/src/components/auth/**`
Target (V2): `src/features/auth/**`, `src/features/onboarding-v2/**`, `src/lib/i18n/**`, `src/app/api/**`

---

## Gap summary (analysis result)

V2 already has a polished *redesign* of the auth page + a *simplified* onboarding.
What V1 has that V2 is missing:

### Auth
- [ ] Forgot/reset password flow (`requestPasswordReset` / `resetPassword`)
- [ ] Enterprise SSO mode (business email + org domain → SSO sign-in)
- [ ] Live per-field validation indicators (green/red as you type)
- [ ] Passkey feature-detection (`detectWebAuthnSupport`) + conditional autofill
- [ ] Email verification path
- [ ] Real NO/EN i18n (V2 switcher is cosmetic)

### Onboarding (V2 currently static / single-shot; V1 is a rich machine)
- [ ] localStorage-persisted **state machine** (resume on refresh) — `useOnboardingMachine`
- [ ] Full NO/EN i18n tables (`ONBOARDING_COPY`)
- [ ] `OnboardingFrame` two-pane shell + `BrandStrip` (live brand pill) + step dots + tooltips
- [ ] `PostSignInStep`: intro video, auto-advance
- [ ] `OrganizationStep`: BRREG search + skip-to-manual + employee→size inference
- [ ] `WebsiteStep`: **live SSE crawl preview** with falling snippet cards + brand extraction
- [ ] `ConnectStep`: **Nango embedded connect** + **live graph polling** (Obsidian-style SVG)
- [ ] `SocialProofStep`: logo wall + stats
- [ ] `PaywallStep`: **Model Plane recommendation** + local fallback engine + ElevenLabs-style full-screen grid + billing toggle + plan persistence
- [ ] `AssemblyStep`: tick animation + completion endpoints + machine reset

### Backend routes to add (same-origin Next routes → control plane)
- [ ] `POST /api/onboarding/crawl-preview` (SSE → quarry-control crawl)
- [ ] `GET  /api/onboarding/graph-preview` (Data Plane v2 graph-index)
- [ ] `POST /api/connections/create` (integration-core / Nango connect session)
- [ ] `GET  /api/knowledge/integrations` (resolve active orgId)
- [ ] `POST /api/onboarding/recommend-plan` (Model Plane plan recommendation)
- [ ] `POST /api/user/onboarding/complete` + `PUT /api/user/me/onboarding-state`
      (V2 currently has `POST /api/v1/onboarding/status` — reconcile)

Existing V2 routes reused as-is: `/api/org/orgs`, `/api/org/orgs/:id/plan`,
`/api/org/orgs/:id/checkout-session`, `/api/v1/users/me`.

---

## Architecture decisions for the port

1. **Mirror V1's file layout** under the V2 feature folder instead of the current
   single-file reducer, so steps stay small and match V1 1:1:
   ```
   src/features/onboarding-v2/
     lib/onboarding-machine.ts      (state machine + types, localStorage)
     lib/onboarding-i18n.ts         (ONBOARDING_COPY NO/EN + helpers)
     lib/onboarding-service.ts      (KEEP — clean backend wrappers, extend)
     components/OnboardingFrame.tsx
     components/onboarding-shared.tsx (LeftPane/RightPane/StepTitle/… + TopActions)
     components/steps/{PostSignIn,Organization,Website,Connect,SocialProof,Paywall,Assembly}Step.tsx
     components/BrregSearch.tsx     (KEEP)
   ```
   `VelionOnboardingPage.tsx` becomes a thin host that builds the machine and
   renders `<OnboardingFrame machine={…} />`.

2. **Shared i18n provider** `src/lib/i18n/locale-context.tsx` exposing
   `LocaleProvider`, `useLocale()`, `useLanguageSwitch()` (cookie-persisted
   `velion_locale`, NO default). Both the auth page and onboarding read from it,
   replacing the cosmetic switcher. Mounted in `src/app/providers.tsx`.

3. **Graceful degradation** preserved exactly as V1: every networked step
   advances anyway on failure/timeout (crawl SSE 12s safety advance, graph
   preview keeps last render, recommend-plan falls back to the local engine).

4. **Backend routes**: implement as thin proxies to the control-plane gateway
   following the existing `/api/org/[...path]` + `control-plane-auth.ts` pattern.
   Where a control-plane service is not yet reachable in V2, the route returns a
   well-formed empty/É‑degraded payload (e.g. graph-preview → `{nodes:[],edges:[],counts:…}`)
   so the UI matches V1 without fabricating data.

---

## Phases

**Phase 1 — Foundation (no UI yet):**
- `src/lib/i18n/locale-context.tsx` (LocaleProvider + hooks, cookie persistence)
- `src/features/onboarding-v2/lib/onboarding-machine.ts` (port types + machine)
- `src/features/onboarding-v2/lib/onboarding-i18n.ts` (port ONBOARDING_COPY)
- wire `LocaleProvider` into `src/app/providers.tsx`

**Phase 2 — Onboarding shell + light steps:**
- `onboarding-shared.tsx`, `OnboardingFrame.tsx`
- `PostSignInStep`, `SocialProofStep`, `AssemblyStep`
- rewrite `VelionOnboardingPage.tsx` as host

**Phase 3 — Onboarding interactive steps:**
- `OrganizationStep` (BRREG), `WebsiteStep` (SSE), `ConnectStep` (Nango + graph),
  `PaywallStep` (recommendation + grid)

**Phase 4 — Onboarding backend routes** (+ graceful stubs where service absent).

**Phase 5 — Auth gaps:** forgot/reset, SSO mode, live validation, passkey
detection, email verification.

**Phase 6 — Auth full i18n:** auth NO/EN tables, wire switcher to `useLocale`.

**Phase 7 — Tests + verification:** unit (machine, i18n format, recommendation
engine), update existing `onboarding-service.test.ts`, typecheck + build.

Each phase ends green (typecheck/build) before the next starts.
