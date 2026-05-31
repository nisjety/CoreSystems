# velionv2 — Auth & Onboarding architecture review

Companion to [AUTH_ONBOARDING_PORT_PLAN.md](./AUTH_ONBOARDING_PORT_PLAN.md). This
document maps the **real backend API surface** the auth page and onboarding
wizard must integrate with, flags where the V1 contracts have **drifted**, and
recommends the **layers, dependencies, performance and consistency** changes to
make the port fast and correct rather than a literal copy.

Stack of record: **Next.js 16.2.6, React 19.2.4, better-auth 1.6.12 (+passkey),
Zod 4, Tailwind 4**. No data-fetching lib, no `next-intl`, no `framer-motion`.

---

## 1. The system as it actually is

**velionv2 (`:3107`) is itself the gateway/BFF.** There is no separate API
gateway. The browser holds only the better-auth session cookie (`sid`, httpOnly,
prefix `idknuten`). Same-origin route handlers proxy server-side to each core,
forwarding the cookie for identity and injecting `INTERNAL_API_KEY` +
`X-User-Id/Email` for trust (`src/app/api/_lib/control-plane-auth.ts`,
request-memoized via a `WeakMap<NextRequest>`).

| Plane / service | Tech · port | Endpoints relevant to auth/onboarding |
|---|---|---|
| **auth-core** | NestJS+better-auth · `3011` | `/api/auth/*` (sign-in/up, **password reset, email verify, 2FA, OAuth, passkey, SSO, organization**); audience token mint `/api/:audience/token`, `/api/model-plane/token` |
| **user-core** | Go/Gin · `3012` | `GET/PATCH /api/v1/users/me`; `POST /onboarding/complete`; `GET|PUT /me/onboarding-state`; **`GET /me/session-context`** (drives the wizard: COMPLETED / CONNECTORS_PENDING / …) |
| **org-core** | Go/Gin · `8080` | `POST /organizations` (persists BRREG snapshot, emits `organization.created`); `POST /organizations/:id/plan`; **`GET /brreg/search`**, `GET /brreg/:orgnr`; `/internal/orgs/:id/onboarding/state` |
| **billing-core** | Go/Gin · `3014` | `POST /orgs/:id/checkout-session` (Stripe, paid only); `GET /orgs/:id/entitlements/:feature`; Lago + Stripe adapters; consumes `organization.created` → auto-provisions free account (**async / eventually consistent**) |
| **session-core** | Go/Gin · `3017` | model-plane harness sessions + SSE (`/v1/sessions/:id/events`) — *not* browser session list |
| **audit-core** | Go/chi · `8187` | `GET/POST /v1/audit`, `GET /v1/usage` |
| **graph-index-rs** | Rust/axum · `9203` | **`GET /v1/graphs/{org_id}`** → `{nodes,edges,node_count,edge_count,truncated}` — powers the Connect-step graph preview. Needs `X-Org-ID` + `INTERNAL_API_KEY` |
| **documents-api-go** | Go · `8010` | `POST /v1/documents` (content-push ingest), `GET /v1/sources` (per-source doc counts). Needs `X-Org-ID` |
| **Model Plane inference** | gRPC `9092` / `/v1/ai/*` | generic chat/inference w/ structured output. **No purpose-built plan-recommendation endpoint exists.** |
| **quarry-edge** | Rust/axum · `8082` | `POST /v1/crawl` (fire-and-forget handoff → `{request_id}`), **`POST /v1/scrape/stream` (SSE, single page)**, `GET /v1/runs/:id/events` (`branding_extracted`…). **Requires auth-core JWT Bearer.** |
| **integration-core** | TS/Fastify · `3026` | **Nango-backed**. `POST /api/v1/providers/:provider/connect-session` → `{sessionToken, connectUrl, expiresAt}`. **Gated by `requirePlan('pro')`.** Providers: microsoft, google(-drive), notion, slack, github, … |

NATS streams for consistency: `USER_EVENTS`, `ORGANIZATION_EVENTS`,
`CONTROL_PLANE_EVENTS` (`user.*`, `organization.*`, `session.*`, `billing.*`,
`usage.*`).

---

## 2. The V1 onboarding backend has drifted — don't copy the routes literally

V1's onboarding called five routes that **do not exist in this stack** and whose
real equivalents have different shapes. Map them deliberately:

| V1 route (copy-paste target) | Reality in this stack | Action |
|---|---|---|
| `POST /api/onboarding/crawl-preview` (SSE snippets+branding) | **No streaming crawl.** quarry-edge has `POST /v1/scrape/stream` (SSE, **single page**) + `POST /v1/crawl` (handoff) + `GET /v1/runs/:id/events` | Build `/api/onboarding/crawl-preview` as a **server-side composition**: scrape-stream the seed page for instant snippet+branding cards, fire `/v1/crawl` for the rest, relay `runs/:id/events`. Requires minting an **auth-core JWT** for quarry (audience token), not just the internal key. |
| `GET /api/onboarding/graph-preview` | graph-index-rs `GET /v1/graphs/{org_id}` (`:9203`) | Thin proxy; inject `X-Org-ID` (from session-context) + `INTERNAL_API_KEY`. Map `{node_count,edge_count}`→`counts`. Degrade to `{nodes:[],edges:[],counts:{…0}}`. |
| `POST /api/connections/create` | integration-core `POST /api/v1/providers/:provider/connect-session`; fields renamed (`connectUrl`/`sessionToken`) | Proxy + field-map. **Resolve the `requirePlan('pro')` conflict** (see §4). |
| `POST /api/onboarding/recommend-plan` | **Nothing in Model Plane.** | Keep the **local recommendation engine** (it already exists in V1's PaywallStep and works offline) as the source of truth; optionally enrich via `/v1/ai/chat` structured-output later behind a flag. Don't block the UI on it. |
| `POST /api/user/onboarding/complete`, `PUT /api/user/me/onboarding-state` | user-core `POST /api/v1/users/onboarding/complete`, `GET|PUT /me/onboarding-state`, `GET /me/session-context` | V2 already wraps session-context in `/api/v1/onboarding/status`. Reuse it; align the completion call. |

Org create + plan + checkout + BRREG already have real homes and a working V2
proxy (`/api/org/[...path]` → org-core/billing-core). Reuse them.

---

## 3. Recommended layers & dependencies

The current app has **two divergent proxy styles** (raw cookie-forward proxy vs
typed integration libs) and **no client data layer** (raw `fetch` + manual state
everywhere, blanket `force-dynamic`/`no-store`). Introduce four thin layers:

### 3.1 A typed control-plane client (server) — *no new dep*
Consolidate the divergent proxies behind one module
(`src/lib/control-plane/client.ts`) exposing typed functions per core
(`orgCore.createOrganization`, `graphIndex.getGraph(orgId)`,
`quarry.scrapeStream(url)`, `integrationCore.connectSession(...)`). One place for:
base-URL resolution (fix the **localhost-vs-docker default drift**), header
injection, **Zod-validated** responses, and error normalization. This is the
api-design "one envelope, one client" principle and removes per-route bespoke fetch.

### 3.2 A client data layer — **add `@tanstack/react-query` v5**
For the *interactive* client surfaces (graph polling, connect status, session
context, navbar, inbox) raw `fetch` + `useState` is the biggest source of
inconsistency. React Query gives dedup, caching, polling (`refetchInterval` for
the 5 s graph poll), retry/backoff, and request cancellation for free, replacing
the hand-rolled `graphRequestInFlightRef`/`queuedGraphRefreshRef` machinery in
ConnectStep. Mount `QueryClientProvider` in `providers.tsx`. (SWR is a lighter
alternative; React Query wins for the polling + mutation mix here.)

### 3.3 An SSE/stream helper — *small util*
`src/lib/net/sse.ts` — the `fetch` + `ReadableStream` + `\n\n` frame parser is
copied inline in V1's WebsiteStep. Extract it once (typed event union, abort
support) so crawl-preview, scrape-stream and session-core events share it.

### 3.4 i18n — keep the in-house provider, or adopt `next-intl`
The `LocaleProvider` added in Phase 1 is sufficient for the two surfaces. **If**
localization expands app-wide, migrate to **`next-intl`** (App-Router native,
server+client messages, ICU plurals) — but that's a larger commit; not required
for this port.

**New runtime deps:** `@tanstack/react-query`, `framer-motion` (PaywallStep's
animated ring/`m.div` — currently absent; either add it or convert to the CSS
keyframes the rest of the flow already uses). **Dev:** none beyond existing.

**Auth gaps need no new infra** — better-auth in auth-core already exposes reset /
verify / SSO / passkey; the work is client wiring + `authClient` plugins.

---

## 4. Consistency (the higher-risk area)

1. **org → billing is eventually consistent.** billing-core provisions the free
   account *asynchronously* off `organization.created`. The paywall/assembly steps
   must not assume entitlements exist the instant the org is created. Treat a
   missing account as "free, provisioning" and poll/retry, or gate the
   plan-write with idempotency.
2. **`requirePlan('pro')` vs connect-before-paywall.** V1 runs Connect (step 4)
   *before* the paywall (step 6), but `connect-session` requires a pro plan.
   Options: (a) reorder so plan selection precedes Connect, (b) grant a
   time-boxed onboarding entitlement, or (c) call connect-session with the
   internal key (which can set any org) during onboarding. **Decision needed** —
   flagging, not assuming.
3. **Idempotency.** Org creation and plan writes should send an idempotency key
   so the machine's resume-on-refresh (Phase 1) can't double-create. org-core
   `POST /organizations` + documents-api `CreateDocumentInput.idempotency_key`
   already support this.
4. **Cache invalidation via NATS.** The recurring "stale after change" class of
   bug (see the Data Plane v2 freshness note) argues for tag-based invalidation:
   when the app caches org/entitlement/graph reads, key them with
   `next: { tags: ['org:'+id] }` and revalidate on the relevant NATS event
   (bridged through a webhook/route). Single source of truth = the event, not a TTL.
5. **One base-URL config module.** The localhost-vs-docker default drift across
   `user-core`/`audit-core` libs vs `control-plane-auth.ts` is a latent prod bug.
   Centralize in the §3.1 client.

---

## 5. Performance (Next.js 16)

- **Stop blanket `force-dynamic` + `no-store`.** It's correct for mutations and
  per-user reads, but BRREG search, the plan catalog, and static config can use
  **Cache Components (`use cache`)** or `fetch(..., { next: { revalidate, tags }})`.
  Next 16 is dynamic-by-default, so caching is opt-in and safe. ([Next 16 caching](https://nextjs.org/docs/app/getting-started/caching-and-revalidating))
- **Server-resolve the session in `proxy.ts`** (Next 16 renamed middleware →
  `proxy.ts` with full Node APIs) so `requireOnboardingAccess` /
  `redirectAuthenticatedUserFromAuth` don't each re-hit `get-session`; the
  WeakMap memo only covers a single request. ([Upgrading v16](https://nextjs.org/docs/app/guides/upgrading/version-16))
- **Stream the onboarding shell.** Wrap the networked right-pane visuals
  (graph, snippet drop) in `<Suspense>` so the left-pane form is interactive
  immediately — matches V1's "advance anyway" resilience and Next 16 streaming.
- **Parallelize independent reads** (session-context + entitlements + org) with
  `Promise.all` in server components instead of sequential awaits.
- **Turbopack is default** in 16 — the existing `next dev`/`next build` already
  benefit; enable Turbopack filesystem caching for faster cold builds.

---

## 6. Frontend quality

- **Accessibility** (apply the accessibility-review lens): the ported V1 markup
  is largely sound (labeled inputs, `role="tablist"`, `aria-pressed`), but: the
  scanner/marquee animations need `prefers-reduced-motion` guards; the paywall
  cards are `<article onClick>` — make them real `<button>`/radio semantics with
  keyboard selection and `aria-checked`; the live graph SVG is `aria-hidden`
  (fine) but the counts beside it should be an `aria-live="polite"` region; the
  cookie `<dialog>` needs focus-trap + `Esc`. Target WCAG 2.2 AA.
- **Design system** (design-system / design-critique): the flow hardcodes hex
  (`#EDEBE7`, `#1F1B17`, `#FF2E63`, `#A09890`…) and `font-inter` inline across
  every file. Promote these to **Tailwind 4 `@theme` tokens** (`--color-canvas`,
  `--color-ink`, `--color-accent`, `--font-display`) so auth/onboarding/shell
  share one source and theming/dark-mode become possible. Extract the repeated
  `LeftPane/RightPane/PrimaryButton/StepTitle` into the shared primitives
  (already planned in Phase 2) and treat them as the design-system seed.
- **UX copy** (ux-copy): the copy is bilingual and strong; once full i18n lands,
  run a consistency pass (sentence case, "Velion" casing, error tone) and ensure
  every error has a recovery action.
- **Code quality**: collapse the two proxy styles (§3.1), delete the
  hand-rolled in-flight/poll refs in favor of React Query (§3.2), and add unit
  tests for the recommendation engine + i18n `formatOnboardingText` + the SSE
  parser (pure functions, high value).

---

## 7. SEO / AEO — scoped out for these surfaces

`/login` and `/onboarding` are authenticated and should be `noindex`; AEO/SEO
work does **not** belong here. Where it *does* pay off: the public marketing
pages should emit **`Organization` + `FAQPage` JSON-LD** — and the **BRREG
Organization data captured during onboarding** (legal name, org number, address)
is ideal structured-data fuel for the tenant's public profile/help pages.
Attribute-rich schema earns materially higher AI-citation rates. ([AEO 2026](https://surferseo.com/blog/answer-engine-optimization/), [schema for AEO](https://www.thehoth.com/blog/structured-data-for-ai-search/))

---

## 8. How this changes the port plan

- **Phase 4 (backend routes)** is rewritten per §2 — the routes are
  *compositions/proxies over real, drifted contracts*, plus an **auth-core
  audience-token mint** for quarry/integration Bearer auth.
- **Add a Phase 0.5**: the §3.1 typed client + §3.3 SSE helper + React Query
  provider, before the interactive steps consume them.
- **Decisions — RESOLVED (2026-05-31):**
  - (a) Connect order: **keep V1 order** (Connect before paywall). Let the user
    connect everything during onboarding so we accumulate signal; the proxy calls
    `connect-session` with the **internal key** during onboarding (bypasses the
    `pro` gate). **Compatibility is enforced at the paywall**: if the chosen plan
    doesn't support a connected integration, block/flag at plan selection. See §9.
  - (b) Recommendation: **progressive context + dedicated Model Plane endpoint**
    (see §10). Local engine stays as the instant, never-blocking default.
  - (c) Dependencies: **adopt `@tanstack/react-query` + `framer-motion`.**

---

## 9. Paywall & plan strategy (Intercom/Linear-informed)

**Velion's model is Intercom-shaped** — per-seat tiers + **per-AI-resolution**
usage ("X kr per henvendelse løst av AI") + free **Lite seats** — with a
**Linear-shaped generous Free tier**. The existing V2 paywall copy already
encodes this (Essential / Advanced / Expert / Custom + Free/trial). Reference
pricing: Intercom Essential $29 / Advanced $85 / Expert $132 per seat + Fin
**$0.99/resolution** + 14-day trial; Linear Free → Basic ~$8–10 → Business
~$14–16 → Enterprise per-seat. ([Intercom pricing](https://www.intercom.com/pricing), [Fin pricing](https://fin.ai/pricing), [Linear pricing](https://linear.app/pricing))

**The 14-day Pro trial → Free model** (your spec). On org creation the user is
put on a **Pro trial for 14 days**; if they pick nothing, it **auto-downgrades to
Free**. This must be modeled in **billing-core**, not just the UI:
- account carries `plan='pro'`, `status='trialing'`, `trial_ends_at`, and a
  `downgrade_to='free'` target;
- a scheduled job / NATS-timer flips `trialing → active(free)` at `trial_ends_at`
  unless a paid plan was chosen (emit `billing.plan.changed`);
- entitlement checks treat `trialing` as full Pro so onboarding Connect works.

**UI affordances to add** (validated against Mobbin — Intercom, Melio, Arcade):
- A **right-rail order summary** like Intercom's "Build your subscription":
  *"14-day Pro trial · Due today: 0 kr · Reverts to Free on {date}"*. ([Intercom build-subscription screen](https://mobbin.com/screens/55743b8c-e496-4d99-b5ad-f3ee32364d64))
- A **"Current trial" badge** on the Pro card + a trial-ends banner, à la Melio's
  onboarding paywall. ([Melio plan step](https://mobbin.com/screens/67518093-a65c-482d-92cb-93b57cc6f88c))
- Keep the **recommended-plan highlight** (Velion's animated ring) + the
  monthly/yearly toggle with savings badge (Arcade/Intercom/Melio all do this).
- **"Need help choosing? Chat"** affordance (Arcade/Intercom).
- **Plan↔integration compatibility**: connected sources that a lower tier doesn't
  support show an inline "included in Advanced+" note; selecting an incompatible
  plan disables Continue with a clear reason (don't silently drop the integration).

## 10. Recommendation pipeline (progressive context → Model Plane)

**Accumulate a structured `OnboardingContext` as the user moves through steps**
(org size/employees from BRREG, website host + agent brief, connector picks,
source counts). It already lives in the onboarding machine state (Phase 1) — add
a serializer that produces a stable, PII-light context object.

**Right before the paywall mounts**, send `{context, catalog, criteria, locale}`
to Model Plane and render its suggestion (not a lock-in). Two-tier so we never
block the UI:
1. **Instant**: the **local engine** (ported from V1) renders a recommendation
   immediately from the context — zero latency, offline-safe.
2. **Authoritative**: the Model Plane call refines it; swap in when it returns.

**Decision: build a dedicated `POST /v1/recommend/plan` endpoint in Model Plane**
rather than calling `/v1/ai/chat` from the BFF. You prioritized
performance + quality + consistency, and a dedicated endpoint wins on all three:
- **Consistency/quality**: the prompt + JSON-output schema + offering criteria
  are **versioned server-side** (one source of truth), not duplicated in the
  frontend; structured-output is validated centrally.
- **Performance**: **cache by `hash(context + catalogVersion)`** so identical
  signals don't re-bill an LLM call; set tight `max_tokens`; run with **ZDR**.
- **Observability**: one place to log/measure recommendation acceptance.
  Internally it wraps `InferenceCore` (the same `/v1/ai/chat` machinery), so it's
  thin — but the contract is owned by Model Plane.

  Contract:
  ```
  POST /v1/recommend/plan        (model-plane, internal-key auth, org-scoped)
  → { context: OnboardingContext, catalog: PlanCatalog, locale }
  ← { planId, confidence, reason, summary, modelVersion, generatedAt }
  ```
  BFF proxy `POST /api/onboarding/recommend-plan` injects org id + key, validates
  with Zod, and **falls back to the local engine** on any error/timeout.

  If building the endpoint slips, the MVP is the BFF calling `/v1/ai/chat` with a
  structured-output schema — same contract to the client — and we harden into the
  dedicated endpoint later. Same client code either way.

## 11. Reference UI study (Mobbin)

| Product | Pattern worth stealing | Screen |
|---|---|---|
| Intercom | 3-step "Build your subscription" + **right-rail order summary** w/ "14-day trial / Due today $0" | [link](https://mobbin.com/screens/55743b8c-e496-4d99-b5ad-f3ee32364d64) |
| Intercom | "What would you like to trial?" radio path selector | [link](https://mobbin.com/screens/5576b5d7-7908-4c6e-8919-fffce0f69b6a) |
| Melio | Onboarding paywall: **"Current trial" badge** + trial-ends banner + monthly/annual "Save 20%" | [link](https://mobbin.com/screens/67518093-a65c-482d-92cb-93b57cc6f88c) |
| Arcade | Recommended plan highlight ("For your team") + per-tier credits + "Chat with us" | [link](https://mobbin.com/screens/db2eec9e-5b2e-47fc-b9af-8d4bfa9ffd55) |
| Webflow | Audience segmentation tabs (in-house vs agency) — relevant if Velion adds buyer types | [link](https://mobbin.com/screens/aa8edb35-1f2f-4e96-b541-4b8ecedd7d00) |
