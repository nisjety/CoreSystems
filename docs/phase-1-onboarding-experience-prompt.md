# Phase 1 onboarding · experience prompt

This document is a single self-contained prompt. Paste it into Claude /
GPT-5 / Cursor / any coding agent to brief them on **how the Verevon
onboarding wizard should work** — user journey, backend wiring, state
machine, fallbacks, and success criteria. It is written in the second
person ("you build…") so the agent can act on it directly.

> Companions:
> [`phase-1-onboarding-prompt-setup.md`](./phase-1-onboarding-prompt-setup.md)
> (designer / LLM prompts per slot) and
> [`onboarding-asset-prompts.md`](./onboarding-asset-prompts.md)
> (per-asset designer briefs). This file is the **product spec
> prompt**; the others are the **prompt assets**.

> The wizard's chrome MUST be a 1:1 reuse of `<AuthPage>` — same
> outer wrapper, same scale logic, same grid, same left-pane card,
> same right-pane image frame. Only the **contents** of the two
> panes swap when the user transitions out of "auth mode" into
> "onboarding mode". The section below quotes the AuthPage code that
> the wizard must mirror.

---

## Prompt (copy from here)

> You are building Phase 1 of Verevon's onboarding wizard. Verevon is a
> customer-support AI platform (Intercom × Chatbase). The wizard's
> job is to take a freshly-signed-up user from "just verified my
> email" to "looking at my own dashboard with my own knowledge
> already indexed" in **under 90 seconds**, while feeling like the
> product is personalising itself live around them.
>
> ### Non-negotiable constraints
>
> 1. **Auth-embedded — reuse the literal AuthPage shell.** The
>    wizard renders inside the same outer wrapper, the same scale-
>    to-viewport logic, the same card, and the same two-pane grid as
>    `<AuthPage>`. Use this markup verbatim as the wizard's
>    `<OnboardingFrame>`:
>
>    ```tsx
>    <div className={`relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-800 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10 ${isPageVisible ? 'opacity-100' : 'opacity-0'}`}
>         style={{ '--primary': '#111111', '--primary-foreground': '#ffffff', '--ring': '#111111' } as React.CSSProperties}>
>      <div ref={cardRef}
>           className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]"
>           style={{ transform: `scale(${cardScale})`, transformOrigin: 'center center' }}>
>        {/* LEFT pane: white card */}
>        <div className="flex items-center justify-center rounded-l-[24px] bg-white px-5 py-6 sm:px-7 sm:py-7 md:px-8 md:py-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
>          {/* ...left-pane contents... */}
>        </div>
>        {/* RIGHT pane: image frame */}
>        <div className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
>          {/* ...right-pane visual... */}
>        </div>
>      </div>
>      {/* ...PageFooterLinks + global modals... */}
>    </div>
>    ```
>
>    Keep these exact values: outer wrapper colours
>    (`--primary:#111111`), card border `#D6D2CB`, card background
>    `#EDEBE7`, rounded-`24px`, grid `1.15fr / 0.85fr`, `max-w-
>    [70.5rem]` desktop / `xl:max-w-[72rem]`, left pane `bg-white`
>    with `rounded-l-[24px]`, right pane minimum heights `560 / 600 /
>    640 px` per breakpoint and `rounded-r-[24px]`. The shadow,
>    z-index `120`, and `overflow-visible` on the card are all
>    intentional — keep them.
>
>    Reuse the viewport-fit scale logic from AuthPage verbatim:
>
>    ```ts
>    const baseCardHeight = 1080
>    const cardScale = viewportHeight
>      ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / baseCardHeight))
>      : 1
>    ```
>
>    plus the same `useAuthPageState`-style reducer that tracks
>    `viewportHeight`, `contentHeight`, `isHydrated`, `isPageVisible`
>    so the fade-in and content-height animation match the auth
>    page's behaviour exactly.
>
> 2. **Left pane layout — same vertical rhythm as AuthPage.** Inside
>    the white card, the order from top to bottom is:
>
>      1. **Top row** — left: `← TILBAKE` link
>         (`text-xs font-bold tracking-tight text-[#111111]
>         hover:text-[#FF2E63]`) pointing to `/`. Right: language
>         switcher + a 1.5×1.5 px pulsing dot
>         `bg-[#FF2E63]/70`. The wizard reuses the *same row* — even
>         though the back link's destination changes per step (use
>         `machine.back` instead of `/`), keep the visual.
>      2. **Tab strip slot** — AuthPage puts `<AuthTabs>` here. The
>         wizard replaces it with `<ProgressDots current={n} total={6} />`
>         (same vertical space, 12 px tall pill row). Skipping it
>         leaves a visible gap so do not omit.
>      3. **Eyebrow** — `font-inter text-[10px] uppercase tracking-
>         [0.16em] text-[#A09890]`, e.g. `Step 3 of 6 · Website`.
>      4. **H1** — Cormorant Garamond, exactly the AuthPage clamp:
>         `text-[clamp(40px,5vw,72px)] font-normal leading-[1.1]
>         tracking-[-0.01em] text-[#1C1C1C]`. Apply the same
>         `toCamelCaseText()` capitalisation to multi-word titles.
>      5. **Body** — `mt-3 font-inter text-[15px] leading-[1.6]
>         text-[#66615B]`. One short paragraph max.
>      6. **Animated form region** — wrap the form in the same
>         `relative overflow-hidden transition-[height] duration-
>         [520ms] ease-in-out` div + inner `ref={contentRef}` so
>         step-to-step height changes animate smoothly. Measure with
>         `requestAnimationFrame` on mode / loading-state change,
>         exactly as AuthPage does.
>      7. **Primary CTA + Skip** — black `bg-[#111111]`, 11 px
>         uppercase, `tracking-[0.22em]`. Skip link is small uppercase
>         grey `#A09890`. Same shapes as AuthPage's submit buttons.
>      8. **Footer slot** — AuthPage shows `<SupportLinks>` here. The
>         wizard reuses the same slot for an inline help line
>         ("Stuck? support@verevon.com") so the bottom of the card is
>         never visually empty.
>
> 3. **Right pane layout — image frame the AuthPage establishes,
>    contents swap per step.** AuthPage paints the background with
>    `<div className="absolute inset-0 bg-cover bg-center"
>    style={{ backgroundImage: "url('/imagens/curved-interior-
>    sculpture.png')" }} />` plus a scanner-line strip on the left
>    (the `scanner-dot` keyframe animation) and three vertically-
>    stacked lucide icons (`ShieldCheck #10B981`, `Lock`,
>    `Fingerprint #FF2E63/80`). The wizard:
>
>      - Keeps the same right-pane frame (`min-h-[560px] / 600 / 640`,
>        `rounded-r-[24px]`, `overflow-hidden`).
>      - Replaces the sculpture image with the per-step visual
>        (`<RightPane>{step.visual}</RightPane>`). The visual fills the
>        full frame `h-full w-full object-cover`.
>      - Keeps the scanner-line strip on the left of the frame on
>        every step so the right pane reads as the same surface as
>        the auth page. Do not remove the keyframes block.
>      - Drops the icon column on `connect` (graph dark canvas would
>        clash) but keeps it on every other step.
>
> 4. **Aesthetic: Chatbase simplicity × Intercom imagery.** Single
>    accent `#FF2E63`. Cormorant Garamond on H1, Inter on body. Text
>    `#1C1C1C`, secondary `#66615B`, tertiary `#A09890`. Black CTA
>    `#111111`, white `#FFFFFF`. Right-pane dark canvas (graph only)
>    `#0F0F10`. No second accent colour anywhere.
>
> 5. **AuthPage swap rule.** The login page renders
>    `<AuthOrOnboardingPage>` which decides:
>
>      - `auth.isLoading` AND no cached wizard state → render
>        `<AuthPage>` (lets the verify-email + callback flows work).
>      - `auth.isLoading` AND cached wizard state exists → render
>        `<HydrationPlaceholder>` (minimal spinner inside the same
>        card shell so the AuthPage's `<CallbackModal>` does **not**
>        flash before the wizard mounts).
>      - `!auth.user` → render `<AuthPage>`.
>      - Authenticated + machine not hydrated → render
>        `<HydrationPlaceholder>`.
>      - Otherwise → render `<OnboardingFrame machine={machine}>`.
>
>    Keep the `verify-email` modal alive — it lives inside
>    `<AuthPage>` and only fires when an unverified user tries to
>    advance. **Suppress every other modal** (success, callback)
>    during the wizard's hydration window via the placeholder above.
>
> 6. **localStorage is the resume source-of-truth.** Persist every
>    transition to `verevon.onboarding.v1`. Hydrate from
>    localStorage on first mount (in `useEffect`, never SSR). If
>    missing or corrupted, start at `post-signin`. Server-side
>    persistence is deferred — for now the only server writes are
>    the two completion endpoints fired by `AssemblyStep`.
>
> 7. **Degrade open.** Every backend call (crawl, graph, plan
>    recommendation, completion write) has a fallback so the user
>    never gets stuck. Quarry unreachable → illustrative snippets.
>    Data Plane unreachable → seed graph. Model Plane unreachable →
>    trial card. Completion writes fail → ship to `/dashboard`
>    anyway and let the next `OnboardingGuard` run retry.
>
> ### Steps (in order)
>
> The state machine is
> `ONBOARDING_STEPS = ['post-signin', 'organization', 'website',
> 'connect', 'social-proof', 'paywall', 'assembly']`. Each step
> component lives at `components/auth/onboarding/steps/<Name>Step.tsx`
> and is dispatched by `<OnboardingFrame>`.
>
> 1. **`post-signin`** — Left: greeting + spinner ("Setter opp Verevon …").
>    Right: 3–5 s product-reveal video (loops). Auto-advance after 3 s
>    or `onEnded`. Marks `introPlayed=true` once.
> 2. **`organization`** — Left: `<BrregSearch>` + "Skip verification"
>    fallback for non-Norwegian orgs + 5-button size picker (`solo |
>    small | medium | large | enterprise`). Submit →
>    `machine.setOrganization()` → `website`. Right: 3 s globe pulse
>    with the org name appearing in the caption as the user types.
> 3. **`website`** — Left: URL field with `https://` prefix chip +
>    optional brief textarea + black Continue + Skip. On submit, open
>    SSE to `POST /api/onboarding/crawl-preview {url, brief?}` which
>    proxies to `quarry-control:8081` (`crawl_discover`, max 8 pages,
>    depth 1, `auto_commit=true`) and streams `snippet` events of
>    four kinds — `text | image | file | link` — derived from upstream
>    `content_type`. Right: white folder card with falling snippet
>    cards above it; each picks its own x-offset / delay / fall
>    duration so 2–3 are always in flight. Advance to `connect` on
>    `done`, after 4+ snippets land, or a 12 s safety timer.
> 4. **`connect`** — Left: connector picker grouped `chat | docs |
>    tools`. Bottom row: Continue + Skip + live counter `N sources ·
>    M nodes · K edges`. Right: dark (`#0F0F10`) SVG graph polled
>    from `GET /api/onboarding/graph-preview` every 3 s and on every
>    click. The route composes `dpv2-graph-index:9201`'s
>    `/v1/graph/entities` + `/v1/graph/expand` into `{nodes, edges,
>    counts, warning?}` scoped to the caller's org. Deterministic-
>    radial layout (`hash(id) → angle/radius`) so re-fetches do not
>    shuffle existing nodes. New nodes highlight green for 1.2 s.
>    Empty orgs get a seed graph with a `warning` field surfaced
>    bottom-left of the canvas.
> 5. **`social-proof`** — Left: 3-row stats list ("97 % … 42 % … SOC
>    2 …") + CTA "Se planene". Right: 3×2 greyscale logo wall from a
>    single SVG sprite (Apple, Microsoft, Slack, Notion, Zammad,
>    Sanity).
> 6. **`paywall`** — Left: copy + LLM-generated
>    `recommendation.reason` (Norwegian) + black "Velg {plan}". Right:
>    5 vertical plan cards (Hobby, Standard, Pro, Enterprise, Trial).
>    The recommended card gets a coral → indigo → emerald gradient
>    outline animated at 8 s loop. Recommendation: `POST
>    /api/onboarding/recommend-plan` mints a `model-plane` JWT
>    (`mintPlaneToken`), calls model-gateway `/v1/invoke` with
>    `claude-haiku-4-5` and a JSON-schema response, then applies a
>    deterministic floor server-side — 5+ connectors → enterprise,
>    3+ → pro, 1+ OR (website + size ≥ medium) → standard. The floor
>    only upgrades, never downgrades.
> 7. **`assembly`** — Left: "Setter sammen Verevon til deg" + 5-tick
>    checklist filling at 700 ms intervals. Right: 6 s dashboard-
>    assembly video. After the last tick + 400 ms: call
>    `markOnboardingCompleteOnServer()` (fires `PUT
>    /api/user/me/onboarding-state {step:'complete'}` AND `POST
>    /api/user/onboarding/complete` in parallel), `machine.reset()`,
>    then `router.push('/dashboard')`. **Critical**: the server write
>    is what breaks the dashboard ↔ /login loop that
>    `OnboardingGuard` otherwise creates by reading
>    `needsOnboarding()` from user-core.
>
> ### State machine contract
>
> Implement the wizard as a single `useOnboardingMachine()` hook
> that owns:
>
> - `state`: `{step, organization?, website?, connectors,
>   recommendation?, introPlayed, startedAt}`
> - `hydrated`: `false` on first render, `true` after the
>   localStorage read fires in `useEffect`
> - mutators: `goTo`, `next`, `back`, `setOrganization`,
>   `setWebsite`, `addConnector`, `removeConnector`,
>   `setRecommendation`, `markIntroPlayed`, `reset`
>
> Every mutator writes through to localStorage on the same tick via
> a `persist(updater)` helper. Read failures (Safari private,
> Lockdown Mode) are swallowed with a single `console.warn`. Every
> `window.localStorage` access is SSR-guarded.
>
> ### File layout (target)
>
> ```
> apps/Frontend Plane/verevon/src/
>   app/(auth)/login/page.tsx                       # renders <AuthOrOnboardingPage>
>   app/(onboarding)/onboarding/<slug>/page.tsx     # each → <LegacyOnboardingRedirect slug>
>   app/api/onboarding/recommend-plan/route.ts      # LLM + deterministic floor
>   app/api/onboarding/crawl-preview/route.ts       # SSE proxy to Quarry
>   app/api/onboarding/graph-preview/route.ts       # poll proxy to Data Plane
>   components/auth/onboarding/
>     AuthOrOnboardingPage.tsx                      # AuthPage ↔ OnboardingFrame router
>     OnboardingFrame.tsx                           # AuthPage shell + step switch
>     LegacyOnboardingRedirect.tsx                  # legacy-slug → new step bridge
>     steps/_shared.tsx                             # LeftPane, RightPane, ProgressDots, PrimaryButton, SkipLink, StepEyebrow/Title/Description
>     steps/PostSignInStep.tsx                      # step 1
>     steps/OrganizationStep.tsx                    # step 2 (BrregSearch + size)
>     steps/WebsiteStep.tsx                         # step 3 (SSE snippet drop)
>     steps/ConnectStep.tsx                         # step 4 (live graph)
>     steps/SocialProofStep.tsx                     # step 5 (logo wall)
>     steps/PaywallStep.tsx                         # step 6 (5 plan cards)
>     steps/AssemblyStep.tsx                        # step 7 (finale + completion writes)
>     state/types.ts                                # STEPS, payload types, STORAGE_KEY, LEGACY_SLUG_TO_STEP
>     state/useOnboardingMachine.ts                 # the hook above
> ```
>
> ### Success criteria
>
> A first-time user lands on `/login`, completes auth, sees the
> wizard mount inside the **identical AuthPage card** with no modal
> flash, fills in org + URL + a couple of connectors, watches real
> snippets fall from their own URL and real graph nodes appear from
> their own Data Plane, accepts a plan that matches what they
> actually configured, and arrives at `/dashboard` in under 90 s.
> Refreshing the tab at any point resumes the same step. Killing any
> backend plane still gets them to `/dashboard` — just with degraded
> visuals + a small warning string. Reaching `/dashboard` never
> bounces them back to `/login`.
>
> ### When you implement
>
> Build top-down: state machine first, then `<OnboardingFrame>` +
> `_shared`, then each step in order. After every step lands, run
> `tsc --noEmit` and the QA checklist in
> `phase-1-onboarding-prompt-setup.md §5`. Do not introduce a viz
> library for the graph — pure SVG + deterministic-radial layout is
> enough and the verevon bundle is already heavy. Do not introduce a
> form library — controlled React state + native `<form>` is enough
> at this surface area.
>
> Localise visible copy to Norwegian where the existing wizard
> already uses it (`StepDescription`, error messages, paywall reason)
> and English where the Chatbase-parity rewrite landed (step 3 + 4
> titles + CTAs). Do not break that pattern unless the user
> explicitly asks for a single language.
>
> Done.
