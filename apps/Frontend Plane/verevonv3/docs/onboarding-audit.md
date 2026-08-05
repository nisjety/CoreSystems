# Verevon v3 Onboarding Audit

**Date:** 2026-06-10
**Scope:** `src/features/onboarding/**`, plus the auth feature, shared Verevon primitives, `src/app/shell`, and `src/styles/global.css` where they interact with onboarding.
**Method:** Four audit lenses (bottlenecks, bad-code/redundancy, UI/UX, reuse), each finding then adversarially verified against the code (and where relevant against `verevon-gateway-rs` and library internals). Every finding below survived that verification pass (`verdict.isReal = true`). 71 raw verified findings were deduplicated to **66 distinct findings** (5 cross-lens duplicates merged, keeping the higher severity). All file paths are relative to the `verevonv3` root.

---

## Executive summary

The onboarding feature is structurally sound SolidJS: the route is lazy-loaded, props are read through getters, page-level derivations use `createMemo`, SSE progress writes are partially batched, and shared chrome correctly delegates to `src/shared/ui/verevon` primitives. The flow is visually polished with genuinely good bones — resumable state, animated step transitions, inline busy labels on most async buttons.

Three problem clusters dominate:

1. **Happy-path-only error handling.** Connector discovery/sync failures are swallowed by an uninspected `Promise.allSettled` while the UI reports "Tilkoblet"; `finishOnboarding` wipes local state and hard-redirects even when the completion call fails; and the paywall — the one step where money changes hands — never renders the error signal at all, so checkout failures are a silent dead end (the single **critical** finding).
2. **A hot persistence path.** A `createEffect` deep-clones the entire onboarding store via `JSON.parse(JSON.stringify(...))` synchronously on every store write (every keystroke, every SSE crawl event), amplified 2–5x by unbatched multi-`setState` handlers, with the same state stringified up to three times per save cycle. Secondary network-shaped waste: a query key that embeds the full mutable context (one query-cache entry per keystroke), serialized independent round-trips in `commitPlan`/`submitOrganization`, and a redundant state-echo PUT on every page load.
3. **Accessibility and flow-integrity gaps.** No visible focus indicator on inputs (WCAG 2.4.7 failure), zero `aria-live`/`role="alert"` anywhere in the app, focus dropped to `<body>` on every step transition, 8×8px step-dot targets with ~1.5:1 contrast, free-jumping step dots that let users "complete" onboarding with nothing configured, and a refresh mid-crawl that restores a permanently stuck "running" UI.

Most fixes are localized to `src/features/onboarding/components/OnboardingPage.tsx`, `src/features/onboarding/lib/persistence.ts`, the step components, and `src/styles/global.css`; none require backend changes.

### Counts

| Category | Critical | High | Medium | Low | Total |
|---|---:|---:|---:|---:|---:|
| 1. Bottlenecks & redundant work | 0 | 1 | 4 | 6 | 11 |
| 2. Bad code & redundancy | 0 | 2 | 6 | 9 | 17 |
| 3. UI/UX & accessibility | 1 | 6 | 13 | 3 | 23 |
| 4. Reuse & consolidation | 0 | 0 | 7 | 8 | 15 |
| **Total** | **1** | **9** | **30** | **26** | **66** |

Unverified findings: **0** (every reported finding was independently confirmed). Refuted during verification: **2** (see "Checked and dismissed").

---

## 1. Bottlenecks & redundant work

### B-01 — Persistence effect deep-clones the entire onboarding state synchronously on every store write
**Severity: High** · `src/features/onboarding/lib/persistence.ts:21`

`createEffect` calls `createPersistedOnboardingState(options.state)`, which is `cloneOnboardingState = JSON.parse(JSON.stringify(state))` (`src/features/onboarding/lib/state.ts:64-77`). `JSON.stringify` reads every store leaf inside a tracked scope, subscribing the effect to the entire store — so it re-runs on **any** state change: every keystroke in the website URL/brief and org-name inputs, and every SSE crawl event (`OnboardingPage.tsx:216-241`). Only the localStorage/remote *writes* are debounced; the full stringify+parse runs synchronously per store flush. The same data is then stringified again in the local timer (`persistence.ts:26`) and a third time in the remote save body (`api.ts:291`) — 2 stringifies + 1 parse per change burst where one stringify in a debounced callback would do.

**Fix:** Keep a cheap tracked read (or version counter) in the effect body and move the heavy serialization inside the debounced timeout using `untrack()`; stringify once and reuse the string for localStorage. If building the snapshot with shallow spreads instead, note the verifier's caveat: `organization` must also be spread (its fields are written leaf-wise at `OnboardingPage.tsx:189-192,288-289,481`), otherwise org-name keystrokes silently stop persisting.

*Verifier note:* impact is tens of microseconds per clone on today's small state (snippets capped at 12), so the measured cost is low-to-medium — but it is a genuine over-tracking/redundant-serialization anti-pattern that scales with state size.

### B-02 — Unbatched multi-`setState` sequences amplify the persistence effect
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:202`

Several handlers issue consecutive top-level `setState` calls; each flushes effects, so B-01's full-state clone runs once per call. `runWebsitePreview` (lines 202-206): five consecutive `setState('website', ...)` calls. `selectOrganizationResult` (189-192): four. `advanceFromIntro` (175-176), `skipWebsite` (184-185), `submitOrganization` (288-289): two each. The code already uses `batch()` correctly in the SSE `onProgress`/`onDone` handlers (223-227, 236-240) — the pattern is known but applied inconsistently.

**Fix:** Wrap each multi-write handler in `batch()` (or collapse into a single `setState` with an object/`produce`), matching the existing SSE-handler pattern. Cuts persistence-effect runs 2-5x on the affected interactions. (Verifier: the recommendation-effect site at lines 164-165 is already coalesced by Solid 1.x — exclude it.)

### B-03 — Plan-recommendation queryKey embeds the full mutable context — per-keystroke query-cache churn
**Severity: Medium** · `src/features/onboarding/lib/queries.ts:41`

`planRecommendationQueryConfig` uses `queryKey: onboardingQueryKeys.planRecommendation(context())`, where `context` is the `recommendationContext` memo (`OnboardingPage.tsx:140-154`) depending on org name/size/employeeCount, website url/brief, and connectors. The query is created unconditionally at page setup (155-159), so while the user types on earlier steps, every keystroke recomputes the memo, re-runs the reactive options, hashes a brand-new structural key, and registers a fresh (disabled) Query cache entry — verified against `@tanstack/solid-query` 5.101.0 internals: `queryCache.build` creates a new Query per novel key hash even when `enabled: false`. Orphaned entries linger until the 5-minute `gcTime`.

**Fix:** Key the query by stable identifiers (e.g. `['onboarding','recommend-plan', orgId, step]`) and read the context inside `queryFn` via an untracked snapshot (it already reads `context()` lazily, and nothing invalidates by the structural key); or defer creating the query until `state.step === 'paywall'`.

### B-04 — `commitPlan` serializes three independent network round-trips before advancing
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:370`

`commitPlan` awaits `actions.setBrandTheme` (370) before either `setPlan` (trial path, 376) or `startCheckout` (388) + another awaited `setPlan` (399). All three are real cross-origin gateway requests (`api.ts:245-310`), and the backend confirms independence (theme writes to user-core; plan/checkout proxy to org-core). The user waits 2 sequential RTTs (trial) or 3 (paid) at the conversion-critical "Fortsett til oppsett" button.

**Fix:** `Promise.all([setBrandTheme, setPlan])` on the trial path; start `setBrandTheme` and `startCheckout` concurrently on the paid path, awaiting only what gates the step change. Bonus: shortens click-to-`window.open` time, which also helps popup-blocker transient activation (see U-05).

### B-05 — `submitOrganization` blocks step advance on a fire-and-forget ingest call
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:291`

After `createOrganization` resolves, `submitOrganization` does `await actions.startWebsiteIngest({...}).catch(() => undefined)` (291-297) before `setState('step', 'connect')` (299). The result is unused and errors are swallowed, yet the await adds a full extra round-trip to the primary "Fortsett" CTA (held disabled showing "Oppretter organisasjon" the whole time). Nothing downstream depends on the call settling — the connect step's graph preview polls independently and tolerates empty graphs.

**Fix:** Drop the await: `void actions.startWebsiteIngest({...}).catch(() => undefined)` and advance immediately, matching the existing fire-and-forget patterns at lines 89 and 353.

### B-06 — Every onboarding page load echoes the just-loaded server state back via PUT
**Severity: Low** · `src/features/onboarding/lib/persistence.ts:30`

The persistence effect tracks `options.hydratedFromServer()`. In `OnboardingPage.tsx` onMount (89-105), `loadOnboardingState` resolves, `setState(reconcileOnboardingState(snapshot))` applies the server state, then `setHydratedFromServer(true)` flips the tracked signal — re-running the effect and scheduling a remote `saveOnboardingState` whose payload is the state that just came from the server. Net: a redundant `PUT /api/v1/onboarding/state` on every page load. (Verifier: it is slightly worse — the `.catch` path at line 105 also flips the flag and PUTs local state after a *failed* server load.)

**Fix:** Capture the hydrated snapshot's serialized form and only schedule the PUT when the serialized state differs, or set a dirty flag only from user-driven writes.

### B-07 — Dev actor read from localStorage + `JSON.parse` on every gateway request
**Severity: Low** · `src/features/onboarding/lib/api.ts:93`

`requestJson` (93-97) and `streamCrawlPreview` (398-402) call `devActorHeaders()`, defaulting to `getBrowserActor()` (151) — a synchronous `localStorage.getItem` + `JSON.parse` per request. With the 2.5s graph poll and 500ms-debounced saves this runs continuously during the connect step, even though the actor was already resolved once in `createOnboardingGatewayActions` (`actions.ts:24`) and is invariant after first read. Dev-only path (`allowDevActorHeaders`), microseconds next to a fetch — but pure redundant work.

**Fix:** Cache the parsed actor in a module-level variable, or thread the actor captured by `createOnboardingGatewayActions` into `requestJson`/`streamCrawlPreview` (the optional param already exists).

### B-08 — Crawl phase list re-derives `activeCrawlPhase` ~4x per row on every SSE event
**Severity: Low** · `src/features/onboarding/components/steps/WebsiteStep.tsx:58`

Inside the `For` over `onboardingCrawlPhases`, each row defines `phaseIdx = () => activeCrawlPhase(props.website)` as a plain closure evaluated through `done()`/`active()` in ~4 reactive sites per row (classList entries, two `Show`s). With 5 rows, a single website store update during streaming triggers ~20 `activeCrawlPhase` evaluations. `WebsiteStepVisual` similarly calls `websiteProgressPercent(props.website)` twice (140, 142).

**Fix:** Hoist a single `const phaseIdx = createMemo(() => activeCrawlPhase(props.website))` above the `For` and share it; add a memo for `websiteProgressPercent`. The memo also adds equality cut-off so downstream effects stop re-running when the phase index is unchanged.

### B-09 — Paywall step swap tears down and rebuilds the entire OnboardingScreen chrome
**Severity: Low** · `src/features/onboarding/components/OnboardingPage.tsx:535`

The root `Show` (535-605) renders two near-identical `OnboardingScreen` instances — one for the paywall, one for everything else. Entering/leaving paywall destroys and recreates the full screen DOM (topbar, dots, footer, scaled chrome) instead of swapping inner content like every other step transition. The dummy `'paywall'` cases in `renderLeftStep`/`renderRightStep` (503-504, 529-530) exist only to paper over the 170ms transition lag this causes. See also R-06 (the duplicated props/styles) and C-13.

**Fix:** Render a single `OnboardingScreen` and switch only its children; `props.paywall` is consumed in reactive JSX ternaries (`OnboardingScreen.tsx:23-24`), so the flag can be derived (`paywall={currentStep() === 'paywall'}`).

### B-10 — AppShell rebuilds the model context pack 4x per navigation without a memo
**Severity: Low** · `src/app/shell/AppShell.tsx:17`

`contextPack` (17-22) is a plain arrow function invoked in four separate JSX positions (71, 85, 87, 89). Each compiles to an independent reactive computation tracking `location.pathname`, so every in-shell navigation re-runs all four, each allocating a new pack object and mapping the action registry (`src/shared/context-packs/context-pack.ts:24-33`). Small registry today (4 entries), so low severity.

**Fix:** `const contextPack = createMemo(() => buildModelContextPack({...}))` so all four reads share one computation per route change.

### B-11 — `finishOnboarding` exits the SPA with a full page reload
**Severity: Low** · `src/features/onboarding/components/OnboardingPage.tsx:448`

After `completeOnboarding` resolves, `finishOnboarding` sets `window.location.href = '/dashboard'` — a full document reload that re-executes the bundle and destroys the shared `QueryClient` (`src/app/providers/QueryProvider.tsx:5`), even though `/dashboard` is a registered lazy route (`src/app/App.tsx:5,19`). See also C-02 and U-17 for the error-handling side of the same function.

**Fix:** Capture `useNavigate()` at component setup (the pattern already exists in `AuthPage.tsx:15`) and soft-navigate after clearing the storage key. If a hard in-memory reset is intentional, document it.

---

## 2. Bad code & redundancy

### C-01 — Connector marked "connected" even when discovery/sync fail; `allSettled` results never inspected
**Severity: High** · `src/features/onboarding/components/OnboardingPage.tsx:341`

`connectSource()` runs `await Promise.allSettled([actions.discoverSource(source), actions.startIntegrationSync(source), ...warmSharePointDiscovery])` and never reads the results. `allSettled` never rejects, so any failure of discover-source or start-integration-sync is silently swallowed, and lines 349-352 unconditionally push the connector with `status: 'connected'` — the user sees "Tilkoblet" even when the backend sync never started. The `ConnectedSource` type even has a `'pending'` status (`lib/model.ts:34`) that is never produced anywhere. The verevonv2 original this was ported from added connectors as `'pending'` and downgraded to `'failed'` on discovery failure — v3 dropped that handling. (OAuth itself *is* verified before the `allSettled`; it is the discover/sync-start failures that vanish.)

**Fix:** Inspect the `allSettled` results: mark the connector `'pending'`/`'failed'` when discover or sync-start rejected, surface a non-blocking warning via `setError` (already in scope) or per-connector status, log rejection reasons. Use the existing `'pending'` status or delete it.

### C-02 — `finishOnboarding` swallows completion failure, then destroys local state and hard-redirects
**Severity: High** · `src/features/onboarding/components/OnboardingPage.tsx:434`

The completion timer calls `actions.completeOnboarding({...}).catch(() => undefined)` and then unconditionally runs `window.localStorage.removeItem(storageKey)` and `window.location.href = '/dashboard'` (445-448). If the gateway call fails, the onboarding-complete signal is effectively lost: local state is wiped and the user lands on the dashboard with the backend never told onboarding finished. No recovery path exists — `fetchOnboardingStatus` has zero consumers, and nothing gates `/dashboard` on completion. The project's own audit doc (`docs/core-research/onboarding-gateway.md`, ~line 50) flags this as a known gap.

**Fix:** Only clear localStorage and redirect after `completeOnboarding` resolves (or after a bounded retry); on failure keep the snapshot and show an error with retry. **Verifier corrections:** (a) do **not** add an `orgId` guard — the gateway handler (`verevon-gateway-rs/src/onboarding/session.rs:125-175`) never uses `org_id` (resolves by actor email) and the contract declares `orgId?: string`, so a guard would block users who legitimately skipped org creation; (b) waiting for resolve is necessary but insufficient — the gateway masks upstream failures as HTTP 200 with `completed: false`, so the fix must also inspect the `completed` field.

### C-03 — `commitPlan`: duplicated `setPlan` branch and `sourceCount` computed inconsistently with `recommendationContext`
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:381`

The trial branch (376-384) and paid branch (399-407) contain a copy-pasted `actions.setPlan({ orgId, plan, onboarding: { recommendation, sourceCount: state.connectors.length + 1 } })` plus `setState('step', 'assembly')` — both reduce to `persistPlan(selectedPlan)`. Both hardcode `+ 1` for the website source, while `recommendationContext` (line 153) correctly computes `state.connectors.length + (state.website.url ? 1 : 0)`. The website step is genuinely skippable (183-186, 469), so a user who skipped gets an inflated `sourceCount` persisted server-side via `POST .../set-plan`.

**Fix:** Extract a single `persistPlan(plan)` helper and a shared `sourceCount()` memo (`connectors.length + (website.url ? 1 : 0)`) reused by both `recommendationContext` and `setPlan`.

### C-04 — Untrusted localStorage/server snapshots cast to `Partial<OnboardingState>` with only partial validation
**Severity: Medium** · `src/features/onboarding/lib/state.ts:32`

`loadStoredOnboardingState` does `JSON.parse(raw) as Partial<OnboardingState>` — an unchecked cast of untrusted localStorage data. `reconcileOnboardingState` validates `step`, the two arrays, and `themeMode`, but blind-spreads everything else (`...raw` 47, `...raw?.website` 52, `...raw?.organization` 57) without validating `plan` against the `PlanId` union, `website.status` against its enum, `recommendation` shape, or individual connector entries (a malformed entry throws on `item.id`). A corrupted/stale snapshot (older schema version) flows straight into the typed store and into gateway payloads (`activePlan` → `setPlan`/`startCheckout`). The server snapshot reuses the same path (`OnboardingPage.tsx:92`).

**Fix:** Validate the parsed object at this boundary — zod (`^4.4.3` is already a dependency) or hand-rolled guards like the existing `isStep` — falling back to defaults per-field instead of blind spreads.

### C-05 — SSE crawl-preview payloads dispatched via unchecked `as` casts
**Severity: Medium** · `src/features/onboarding/lib/api.ts:461`

`dispatchCrawlPreviewPacket` JSON-parses the packet into `unknown` (good) but then immediately casts: `payload as CrawlSnippet`, `as CrawlProgress`, `as BrandingSignals` (463-478). Nothing checks that e.g. `progress.pages` is a number before it is written into the store and multiplied (`view.ts:109` computes `pages * 34`; a string yields `NaN` that survives the clamps and reaches `<meter value={NaN}>`). Malformed JSON is handled; well-formed JSON with the wrong shape is not. The SSE source is the first-party gateway, which tempers practical severity. (Verifier: the secondary multi-line SSE data-join nit at line 446 is technically a spec violation but observationally harmless for JSON payloads.)

**Fix:** Add minimal runtime guards per event type (typeof checks or a tiny zod schema) before invoking handlers.

### C-06 — Dead gateway actions: `cleanupSource`, `fetchSessionBootstrap`, `fetchOnboardingStatus` wired but never called
**Severity: Low** · `src/features/onboarding/lib/actions.ts:27`

`api.ts` exports `fetchSessionBootstrap` (163), `fetchOnboardingStatus` (167), and `cleanupSource` (350); `actions.ts` wraps all three. An exhaustive project-wide search finds no caller outside `api.ts`/`actions.ts` — no component, hook, query, test, or e2e spec. The two bootstrap fetchers also return weakly-typed `Record<string, unknown>`. Note `fetchOnboardingStatus` is exactly what C-02's fix needs, and `cleanupSource` is what U-18's "remove source" affordance needs — wire or delete.

**Fix:** Delete the three functions and wrapper entries, or wire them where intended (status check before `/dashboard`, source removal UI). If kept, give them real return types.

### C-07 — Protocol-stripping regex duplicated inline in WebsiteStep instead of reusing `stripUrlProtocol`
**Severity: Low** · `src/features/onboarding/components/steps/WebsiteStep.tsx:34`

The input handler does `event.currentTarget.value.replace(/^https?:\/\//, '').replace(/\s+/g, '')`, duplicating the regex that already exists as `stripUrlProtocol` (`lib/view.ts:92-94`) — which the same file imports and uses three lines earlier for the value prop.

**Fix:** `stripUrlProtocol(event.currentTarget.value).replace(/\s+/g, '')` — behaviorally identical, one source of truth.

### C-08 — Website URL sent to crawl-preview with no URL validation
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:196`

`runWebsitePreview` only checks `state.website.url.trim()` truthiness before POSTing to `/api/v1/onboarding/crawl-preview`. The input merely strips whitespace/protocol and prepends `https://` (`WebsiteStep.tsx:33-36`), so `https://%%%` or `https://foo` (no TLD) goes straight to the gateway. The same unvalidated URL feeds `startWebsiteIngest` (292), `recommendationContext`, and `completeOnboarding` metadata. No `new URL()` parse or hostname check exists anywhere in the feature — a violation of the validate-at-boundaries rule (wasted crawl jobs, garbage URLs persisted into org/ingest/recommendation payloads).

**Fix:** Validate with `new URL(url)` plus a hostname sanity check (contains a dot) before enabling "Analyser nettside"; show an inline field error for invalid input.

### C-09 — `requestJson` can throw `Error('')`; empty message makes the failure invisible to the user
**Severity: Low** · `src/features/onboarding/lib/api.ts:114` *(merged with the UI/UX-lens finding at api.ts:120 — same defect)*

When the response body is `{ error: {} }` (or `{ error: null }`), `maybeError?.message` is `undefined` and `throw new Error(message)` produces an Error with `.message === ''`; the `Request failed (${status})` fallback is unreachable for any object-typed error value. Handlers then call `setError(reason.message)` with `''`, and — verifier-corrected symptom — `<Show when={error()}>` (`OnboardingPage.tsx:565`) treats `''` as falsy, so the user sees **nothing at all** for a failed request: a fully silent failure.

**Fix:** Coalesce at the end: `const message = ... || \`Request failed (${response.status})\`` so every thrown error carries displayable text including the HTTP status.

### C-10 — AuthPage initializes email/password signals with hardcoded demo credentials
**Severity: Medium** · `src/features/auth/components/AuthPage.tsx:18`

`createSignal('navn@eksempel.no')` and `createSignal('password123')` pre-fill the login form with credential-looking literals, and `handleSubmit` (67) calls `completeAuth()` with zero validation — an empty email/password submit passes (no `required` attributes in `AuthFormPanel.tsx`) and navigates straight to `/onboarding`. Auth is documented as presentation-only (`docs/core-research/auth-boundary.md`), but a literal password string in source trips secret scanners and the missing validation seam makes eventual real wiring error-prone. (Verifier: severity arguably low given the documented stub status; the code-level claim is exact.)

**Fix:** Initialize both signals to `''` (placeholders already exist; the sibling `name` signal already uses `''`), add minimal required/format validation in `handleSubmit`.

### C-11 — Recommended-plan fallback `?? 'trial'` derived independently in two places
**Severity: Low** · `src/features/onboarding/components/steps/PaywallStep.tsx:17`

`PaywallStep` re-derives `recommendedPlanId = () => props.recommendation?.planId ?? 'trial'` while the parent already computes the identical `recommendedPlan` memo (`OnboardingPage.tsx:138`) over the same object. If the default ever changes in one place only, the selected card and the "anbefalt" badge diverge.

**Fix:** Pass `recommendedPlan()` down as a prop (idiomatic Solid — JSX props compile to getters) or export a single `recommendedPlanId(recommendation)` helper in `lib/view.ts`.

### C-12 — `getBrowserActor` mixes read with a localStorage write side effect and unchecked cast
**Severity: Low** · `src/features/onboarding/lib/api.ts:130`

A getter named `getBrowserActor` writes the fallback actor into localStorage as a side effect (143), casts stored JSON with `JSON.parse(raw) as Partial<ActionActor>` (142) without checking `userId` is a non-empty string (an empty/non-string value flows into the `x-user-id` header), and hardcodes the dev identity inline rather than in the file's existing top-of-module config block. The storage key is also fragmented from the sibling state key in `OnboardingPage.tsx:42`.

**Fix:** Split read and seed responsibilities, validate the parsed actor before merging, hoist the storage key + fallback actor into named module constants.

### C-13 — OnboardingPage is a 560-line god component mixing orchestration, handlers, and rendering
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:44`

The default export spans lines 44-607: timers, persistence wiring, five 13-53-line async handlers (`runWebsitePreview`, `connectSource`, `commitPlan` each ~52 lines), two render-switch functions, and two OnboardingScreen JSX trees whose `screenStyle`/`chromeStyle` objects are byte-identical copies including hardcoded `'#111111'`/`'#FF2E63'` (546-553 vs 587-594). Violates the project's <50-line-function guidance and makes the duplicated chrome easy to drift. Solid idiom favors extracted `create*` primitives, which this feature already uses (`createOnboardingState`, `createOnboardingPersistence`, `createCrawlPreviewStream`).

**Fix:** Extract the async flows into a `createOnboardingController(state, setState, actions)` factory under `lib/`; hoist `screenStyle` to a module constant (`chromeStyle` closes over the reactive `cardScale()` so it needs a shared component-scoped accessor); let the component own only composition. See also R-06.

### C-14 — `websiteProgressPercent` built from undocumented magic multipliers
**Severity: Low** · `src/features/onboarding/lib/view.ts:101`

The progress heuristic hardcodes 34, 25, 12, 94, 18 as bare literals with no constants or comments (34 ≈ 100/maxPages(3)). `activeCrawlPhase` similarly hardcodes the `snippets.length > 6` threshold, and the maxPages=3 assumption is duplicated as a literal in `OnboardingPage.tsx:212` and `api.ts:412`.

**Fix:** Define named constants in `model.ts` (`CRAWL_MAX_PAGES = 3`, `PROGRESS_PER_PAGE = 100 / CRAWL_MAX_PAGES`, …) and derive the multipliers so the page budget changes in one place (no circular-import risk — verified).

### C-15 — Persistence effect guards `window` for local writes but not the remote timer; remote-save failures fully swallowed
**Severity: Low** · `src/features/onboarding/lib/persistence.ts:32`

Lines 23-28 wrap the localStorage debounce in `typeof window !== 'undefined'`, but the remote path immediately below (32-39) calls `window.clearTimeout`/`window.setTimeout` unguarded — inconsistent within one function (cosmetic in this no-SSR SPA; the cleaner fix is removing the line-23 guard). The substantive issue: the remote save swallows every failure with `.catch(() => undefined)` (38, acknowledged in `docs/core-research/onboarding-gateway.md`), so persistent gateway outages never surface anywhere — not even console diagnostics.

**Fix:** One consistent guard policy; log or count remote-save failures (`console.warn` once per outage, or a `lastSaveFailed` signal surfaced at the `OnboardingPage.tsx:69` call site).

### C-16 — Inline structural type for `connectSource` duplicates `ConnectorOption`
**Severity: Low** · `src/features/onboarding/components/OnboardingPage.tsx:307`

`connectSource` declares its parameter as the inline shape `{ id: string; label: string; provider: string; sources: string[] }`, restating four of `ConnectorOption`'s six fields (`model.ts:22-29`). `ConnectStepContent`'s `onConnect` prop is typed with `ConnectorOption`, so the two declarations describe the same value and can drift on semantically divergent (structurally compatible) changes.

**Fix:** Type the parameter as `Pick<ConnectorOption, 'id' | 'label' | 'provider' | 'sources'>` (or plain `ConnectorOption`) imported from `lib/model` — verified to typecheck under strict mode.

### C-17 — AuthFormPanel passes every form prop as `Accessor`, diverging from the onboarding plain-value convention
**Severity: Low** · `src/features/auth/components/sections/AuthFormPanel.tsx:13`

`AuthFormPanelProps` types mode/locale/copy/email/password/name/showPassword as `Accessor<...>` while every onboarding step component receives plain reactive values read through props — both valid Solid, but contradictory conventions across features, and AuthFormPanel itself mixes them (`contentHeight` is a plain number at line 20 next to seven accessors). Every accessor is invoked eagerly inline, so nothing needs lazy evaluation.

**Fix:** Standardize on plain props (call the signals at the JSX call site — Solid compiles prop expressions to getters, so reactivity is preserved); reserve `Accessor` props for genuinely lazy/conditional reads.

---

## 3. UI/UX & accessibility

### U-01 — Paywall step never renders the error signal — checkout/plan/recommendation failures are silent
**Severity: CRITICAL** · `src/features/onboarding/components/OnboardingPage.tsx:596`

The `error()` `Show` block (565-567) exists only in the non-paywall **fallback** branch of the top-level `<Show>` — mutually exclusive with the paywall branch (579-604), which renders `<PaywallStep>` with no error output at all. Yet `commitPlan()` sets `setError` on failure (409), and the `createEffect` at 168-172 sets the error *specifically when* `state.step === 'paywall'` — i.e. exactly when it cannot render. Result: the user clicks "Fortsett til oppsett", the request fails, the button flips back from "Lagrer..." with zero feedback — a hard dead end on the revenue-critical step. (A recommendation failure alone is softened by the `'trial'` default; checkout/setPlan failure is the hard dead end.)

**Fix:** Pass the error into `PaywallStep` and render the same `<p class="onboarding-error">` block above `.onboarding-paywall__actions`, with `role="alert"` so it is announced (see U-08).

### U-02 — Step dots allow free navigation to any step, including paywall and assembly, skipping required work
**Severity: High** · `src/features/onboarding/components/OnboardingPage.tsx:544` *(merged: flagged independently by the bad-code and UI/UX lenses at the same line)*

`onSelectStep={(step) => setState('step', step)}` (544 and 586) is wired to `VerevonStepDots` buttons with no gating (`VerevonStepDots.tsx:15-22` renders always-enabled buttons; the topbar's `steps.slice(1)` excludes only `post-signin`). A user on the website step can click the last dot, land on `assembly`, press "Åpne dashboard" — `finishOnboarding()` calls `completeOnboarding` with `organization.id` undefined, swallows any failure, wipes localStorage, and hard-redirects to an unguarded `/dashboard`: onboarding "completed" with nothing configured. Jumping to `connect` first lands where every row errors. Downstream guards are silent no-ops: `commitPlan` returns early when `!orgId` (364) with no feedback — "Fortsett til oppsett" becomes a dead button.

**Fix:** Only allow dot navigation backwards/to visited steps — `if (onboardingSteps.indexOf(step) <= currentStepIndex()) setState('step', step)` (the `currentStepIndex` memo already exists at line 119) — and render future dots as non-interactive with `aria-disabled`. Replace `commitPlan`'s silent early-return with a `setError` like `connectSource` does at 314 (note: that error is only visible once U-01 is fixed, since the paywall branch currently renders no error).

### U-03 — Brreg organization search can only be triggered with the Enter key — no search button
**Severity: High** · `src/features/onboarding/components/steps/OrganizationStep.tsx:39`

`props.onSearch` is wired exclusively to an Enter `onKeyDown` on the name input (39-43). No search button, icon, or hint text exists anywhere in the step; the placeholder "Søk på organisasjonsnavn..." implies search but gives no affordance for how. Mouse-only and many touch/AT users cannot run the Enhetsregisteret lookup at all. (Mitigation: verification is optional, so the flow remains completable — the lookup is what's lost.)

**Fix:** Add a visible "Søk" button next to the input (or wrap input+button in a small form with `onSubmit`), keeping Enter as a shortcut; optionally debounce-search on input as a typeahead.

### U-04 — Persisted in-flight crawl status restored verbatim after refresh — permanently stuck spinner and read-only URL field
**Severity: High** · `src/features/onboarding/lib/state.ts:50`

`reconcileOnboardingState` spreads `raw.website` (50-54) without normalizing `status`, and the persistence effect saves live state within ~120ms. Refresh mid-crawl and the page restores `status: 'running'`: `WebsiteStep` renders the URL input `readOnly` (32), hides the brief textarea, and shows an eternally spinning phase row — but the SSE stream is gone, `crawlJobId` is written but never read, no reconnect exists, and nothing ever resets the status. Both "Prøv igjen" (failed-only) and "Analyser nettside" (idle fallback) are unreachable; the only escape is "Fortsett mens vi jobber". Both restore paths (localStorage and server hydration) funnel through the same reconcile.

**Fix:** In `reconcileOnboardingState`, downgrade transient statuses on load: map `'starting'`/`'running'` to `'idle'` (or `'failed'` with a "forhåndsvisningen ble avbrutt" warning) so the step is re-runnable.

### U-05 — Paid checkout: popup result ignored, popup blockage unchecked, success/cancel URLs reload the whole SPA inside the popup
**Severity: High** · `src/features/onboarding/components/OnboardingPage.tsx:395`

`commitPlan()` opens checkout with `window.open(checkout.url, '_blank', ...)` without checking the return value (popup blockers make it `null` silently — and the call happens after two awaits, so transient user activation may have lapsed), then immediately calls `setPlan` and `setState('step','assembly')` (407) regardless of whether the user ever pays — no postMessage listener, polling, or storage sync exists anywhere in the app. Meanwhile `successUrl`/`cancelUrl` point at `/onboarding?checkout=success|cancel` (391-392), so the payment provider redirects the 960×900 popup itself into a *second full OnboardingPage instance* — own store, own persistence effect writing the same localStorage key and remote endpoint, own timers. The `checkout` param is consumed in `onMount` (88-101) and never stripped, so a later refresh re-forces the step. Two diverging onboarding surfaces.

**Fix:** Check the `window.open` return and show an "åpne betaling" link if blocked; keep the main window on paywall in a "venter på betaling" state until a postMessage or polled status confirms; give success/cancel a tiny dedicated callback route that messages the opener and closes itself; strip the checkout param with `history.replaceState` after consuming it.

### U-06 — Three interactive-looking controls are dead: graph zoom buttons, language pill, billing-period switch
**Severity: High** · `src/features/onboarding/components/steps/ConnectStep.tsx:69` *(merged: the zoom buttons and billing switch were independently confirmed by the bad-code lens at ConnectStep.tsx:68 and PaywallStep.tsx:28)*

1. **Zoom controls** — `ConnectStepVisual` renders three real `<button>`s ("Zoom ut", "Zoom inn", "Tilbakestill") with aria-labels and a hardcoded `<span>100%</span>` but no onClick handlers and no zoom state; the prop type accepts only `graphNodes`, so no call site *can* wire them. Focusable, announced to screen readers, do nothing. The wrapper div carries `aria-label="Interaktiv kildegraf"` (67) on a role-less, non-interactive div.
2. **Language pill** — `OnboardingTopbar.tsx:41` renders `<VerevonLanguageButton code="NB" />` with a chevron affordance and no onClick — while the auth feature wires the *same component* to a working locale toggle (`AuthFormPanel.tsx:39-44`), so users arriving from `/auth` expect it to work.
3. **Billing switch** — `PaywallStep.tsx:28` renders `<VerevonSwitch label="Billing period" />` between "Månedlig"/"Årlig" with neither `checked` nor `onChange` (VerevonSwitch accepts both). As an uncontrolled Kobalte switch the thumb visually toggles — appearing responsive — but `onboardingPlanCards` has monthly-only prices with a hardcoded `/mnd` suffix, so nothing ever changes.

**Fix:** Implement them (wire zoom to a scale transform; reuse the auth locale toggle; add yearly price data and a `billingPeriod` signal following the existing `onSelectPlan` pattern) or remove/demote to non-focusable decorative elements (`aria-hidden`, drop the misleading "Interaktiv" label) until functional.

### U-07 — No visible keyboard focus indicator on onboarding text inputs (WCAG 2.4.7 failure)
**Severity: High** · `src/styles/global.css:1830`

`.onboarding-field input` sets `outline: none` (1830) and the global focus rule (65-70) covers `button, a, textarea` but omits `input`. The only focus feedback is a shadow alpha shift from 4% to 7% (1840-1844) — imperceptible. The URL input is worse: `.onboarding-field--url input { border: 0; box-shadow: none; }` (1853-1857) plus the wrapper's `overflow: hidden` (1862) clipping any inner shadow leaves the focused control visually identical to unfocused. Fails WCAG 2.4.7 on the primary inputs of the flow.

**Fix:** Give `.onboarding-field input:focus-visible` a 2px outline/border-color change, and put the focus ring on `.onboarding-url-control:focus-within` for the composite URL control. (Verifier: merely appending `input` to the global rule at line 65 loses the cascade to the later same-specificity `outline: none` — use the targeted variant.)

### U-08 — Errors, warnings, and search status are never announced — zero `aria-live`/`role="alert"` in the entire app
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:566`

A repo-wide search finds no `aria-live`, `role="alert"`, `role="status"`, or `aria-busy` anywhere under `src/`. The `.onboarding-error` paragraph (566), `.onboarding-warning` (`WebsiteStep.tsx:89`), and "Søker i Enhetsregisteret ..." status (`OrganizationStep.tsx:53`) appear/disappear silently for screen-reader users. Async failures (Brreg search, org creation, connector OAuth) are invisible to AT users; no toast system or announcer utility compensates.

**Fix:** Add `role="alert"` to error/warning paragraphs and `aria-live="polite"` (or `role="status"`) to the searching indicator and crawl phase list — or, more robustly, a persistent live region.

### U-09 — Stale error messages persist across step transitions
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:183`

`setError(undefined)` only happens at the start of each async action (201, 253, 267, 320, 366). Pure navigation — `skipWebsite` (183), website `onContinue` (468), connect `onContinue`/`onSkip` (495-496), `back()` (452), plus `onSkipStep` (486) and the step dots — never clears it, and the error renders step-agnostically in the left pane. Example: crawl preview fails, the user clicks "Hopp over", and "Could not preview the website." still sits under the organization form, attributed to the wrong context.

**Fix:** A small `createEffect` on `state.step` that calls `setError(undefined)` (safe — no error path navigates).

### U-10 — Skipping the organization step leads straight to a step that cannot be used
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:486`

OrganizationStep's "Hopp over" is wired to `onSkipStep={() => setState('step', 'connect')}` without creating an organization. On connect, every connector row then fails `connectSource`'s guard (313-316) with the English-only "Create the organization before connecting sources." and no affordance to create one besides discovering the back button. Worse (verifier): pressing "Fortsett" on connect still advances, and `commitPlan` then silently no-ops without an orgId — the skip path renders the entire flow uncompletable.

**Fix:** Hide/disable "Hopp over" until an org exists, create a placeholder org on skip, or show an inline "Gå tilbake og opprett organisasjon" CTA on connect when `state.organization.id` is missing.

### U-11 — "Hopp over verifisering" button only clears search results — label promises something it does not do
**Severity: Medium** · `src/features/onboarding/components/steps/OrganizationStep.tsx:48`

The link-button labelled "Hopp over verifisering" is wired to `props.onClearResults` → `clearSearchResults`, whose entire body is `setSearchResults([])` (`OnboardingPage.tsx:179-181`). It is rendered unconditionally, so in the common empty-results state clicking it is a visible no-op; it neither advances the step nor marks verification skipped. The same step's genuine skip button proves the mismatch.

**Fix:** Rename to reflect its function ("Tøm resultater") and hide it while `searchResults` is empty, or make it actually collapse the verification UI.

### U-12 — Brreg search has no empty-results state
**Severity: Medium** · `src/features/onboarding/components/steps/OrganizationStep.tsx:56`

Results render only via `<Show when={props.searchResults.length > 0}>`, and a zero-match search resolves with an empty array and no error — every conditional block disappears and the UI returns to its prior state with no feedback. The user cannot tell whether the search ran or found nothing. (Failures *do* show via the error paragraph; only the empty case is silent.)

**Fix:** Track a `searched` flag (reset in `clearResults`/`onNameInput`) and render "Ingen treff i Enhetsregisteret — fortsett med navnet du skrev" when a completed search yields nothing.

### U-13 — No focus management between steps — keyboard focus is dropped to `<body>` on every transition
**Severity: Medium** · `src/features/onboarding/lib/step-transition.ts:18`

`createOnboardingStepTransition` swaps `displayedStep` after a 170ms timeout (18-22), and the step content is rendered via the reactive `renderLeftStep(displayedStep())`/`renderRightStep(...)` expressions — Solid replaces the entire step subtree, removing the button the keyboard user just activated; focus silently falls to `document.body`. A repo-wide search finds zero `.focus()` calls, `autofocus`, `tabindex`, or live regions; no heading is focusable. Keyboard/SR users must Tab from the top (through topbar dots) after every step. (The paywall transitions through a separate `Show` swap with the same problem.)

**Fix:** After the entering phase begins, move focus to the new step's `h1` (`tabindex="-1"`) or first interactive element; announce the step change via the existing step pill in an `aria-live` region.

### U-14 — Step progress indicators disagree: dots, aria-labels, and "Steg X av 6" pill use different numberings; dots are 8px targets
**Severity: Medium** · `src/features/onboarding/components/shared/OnboardingTopbar.tsx:33`

The topbar slices off `post-signin` so 6 dots represent steps 2-7, but `VerevonStepDots` labels them `Step ${index()+1}` — the website dot is announced "Step 1" while the pill says "Steg 2 av 6" (`stepNumberFor`, `view.ts:78`). `stepNumberFor` also maps both `paywall` and `assembly` to 6, so on paywall the 5th of 6 dots is active while the pill reads "6 av 6". The dots are 8×8px buttons (`global.css:1566-1573`) — below the 24px WCAG 2.5.8 minimum even with the spacing exception — with inactive `#c9c9c9` on `#f7f7f6` at 1.54:1 non-text contrast, and English aria-labels in a Norwegian UI.

**Fix:** Use one numbering source (`stepNumberFor`) for both dot aria-labels and the pill; give paywall/assembly distinct numbers or merge their dots; enlarge the hit area via button padding (keep the visual dot small) and darken the inactive color.

### U-15 — Sub-AA contrast on field labels, eyebrows, skip-buttons, and connector badges (#a09890 on white, ~2.84:1)
**Severity: Medium** · `src/styles/global.css:1902`

`#a09890` on `#ffffff` is 2.84:1, far below the 4.5:1 AA threshold at the tiny sizes used: `.verevon-text-button` — the interactive "Hopp over" skip actions — 11px (1898-1909); `.onboarding-field span` labels 11px; `.onboarding-eyebrow` 10px; connector group headings 10px; the "Legg til" row badge. The step pill `#7b746d` on `#f7f7f6` computes to exactly 4.30:1 at 11px — also failing. These include primary affordances, not just decoration; no WCAG exemption applies.

**Fix:** Darken to at least `#767069` (4.89:1 on white) — the interactive `.verevon-text-button` especially; reserve `#a09890` for non-essential decoration. (Verifier: on the `#edebe7` card background `#767069` lands ~4.1:1 — go slightly darker there.)

### U-16 — Mixed English/Norwegian copy: assembly step, error messages, and switch label break the otherwise Norwegian flow
**Severity: Medium** · `src/features/onboarding/components/steps/AssemblyStep.tsx:12`

Every step is Norwegian except AssemblyStep, which is fully English ("Final step · Assembly", "Verevon is assembling the first workspace.", "pages grounded"/"sources started"/"launch plan") with one Norwegian button "Åpne dashboard". All user-facing `setError` fallback strings in OnboardingPage are English (245, 257, 301, 314, 355, 409), PaywallStep's switch label is "Billing period" (screen-reader-facing), and VerevonStepDots aria-labels are English. The auth feature has a working nb/en copy system (`auth/lib/model.tsx` `getAuthCopy`) that onboarding ignores.

**Fix:** Translate AssemblyStep and all setError fallbacks to Norwegian, or adopt the auth feature's locale/copy-table pattern across onboarding — which would also let the dead NB language pill (U-06) become functional.

### U-17 — Assembly finish button: no busy state, re-clickable during the 1.8s countdown, failures swallowed, full page reload
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:434`

"Åpne dashboard" (`AssemblyStep.tsx:27`) stays enabled after the click — unlike every other async step in the same file (`submittingOrg`, `connectingId`, `committingPlan`), breaking the file's own pattern. `finishOnboarding` (415-450) starts a tick animation and a 1800ms timer; repeated clicks clear and restart both, pushing navigation out another 1.8s each time. `completeOnboarding` failures are swallowed and localStorage cleared + `window.location.href = '/dashboard'` regardless (see C-02, B-11). The 4-item checklist ("Organization created", "Plan committed") also renders all-pending until pressed, implying work that already happened has not.

**Fix:** Disable the button and show "Åpner ..." once pressed; on failure show an error with retry instead of silently clearing state; navigate with `useNavigate()`; pre-tick checklist items that are already true (org id exists, plan set).

### U-18 — No global in-flight guard in `connectSource` — parallel OAuth popups possible
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:313`

`connectSource` sets `connectingId` (319) but never checks it on entry, and ConnectStep disables only the row whose id matches. While the Slack OAuth window is open (`runDirectOauthWindow` waits up to 120s), every other row is enabled — clicking Notion opens a *second* OAuth popup (`provider-auth-window.ts` uses `'_blank'`, so each call is a new window) with both promises racing; the "Åpner" badge moves to the new row and the first flow's `finally` clears `connectingId` mid-flight. Connected rows are also still clickable (silently re-running OAuth), and there is no way to remove a connected source — `cleanupSource` exists (C-06) but is never wired.

**Fix:** Early-return (or disable all rows) while `connectingId()` is set; differentiate the connected-row affordance ("Koble til på nytt" / a remove button wired to `cleanupSource`).

### U-19 — Back-then-continue on the organization step silently creates a duplicate organization
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:271`

`submitOrganization` always calls `actions.createOrganization` without checking the persisted `state.organization.id`. The topbar back button makes revisiting first-class (back from connect); pressing "Fortsett" again creates a second org — verified against the backend: org-core's `POST /orgs` generates a fresh `org_<unix-ms>` id per call with no slug uniqueness — and re-points `state.organization.id` at it, orphaning the first org along with any connectors/ingest started against it. Plan, checkout, graph preview, and completion all use the new id while `state.connectors` still shows sources "connected"; nothing looks wrong to the user.

**Fix:** If `state.organization.id` exists, call an update path or just advance to connect without re-creating (the skip-to-connect path already exists in the same component); at minimum reuse the existing id.

### U-20 — Scale-to-fit chrome plus `overflow: hidden` clips content on short viewports instead of scrolling
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:123`

`cardScale` clamps to a 0.52 floor against `onboardingCardBaseHeight = 1140` while `.onboarding-screen` is `height: 100dvh; overflow: hidden` (`global.css:1409-1412`). The transform is paint-only, so on viewports shorter than ~617px (landscape phones, small laptop windows) the card's top/bottom are clipped and unreachable — nothing scrolls. At floor scale, 32px controls render at ~17px effective touch size. The paywall branch fixes this correctly (`.onboarding-screen--paywall { overflow: auto }` plus `height: auto`/`flex-start` at small widths); the main flow does not. The ≤980px stacked layout makes the un-scaled card far taller than 1140, worsening clipping.

**Fix:** Allow vertical scrolling when `fittedScale` hits its floor — and copy the paywall's `height: auto`/`justify-content: flex-start` companions, since `overflow: auto` alone cannot reach centered-flex top overflow. Alternatively compute base height from the actual card via the already-imported `createElementHeight`.

### U-21 — Plan recommendation is computed once and never refreshed when the user goes back and adds sources
**Severity: Low** · `src/features/onboarding/lib/queries.ts:43`

The query uses `staleTime: Infinity` and is enabled only while `!state.recommendation` (`OnboardingPage.tsx:158`); the effect at 161-166 stores the first result permanently and nothing ever clears it. Reach paywall, go back, connect two more sources, return: the "Din beste match" card and "anbefalt" badge silently reflect the old context — the context-keyed queryKey never drives a refetch because `enabled` stays false.

**Fix:** Drop the `!state.recommendation` gate and let the context-keyed queryKey drive refetches (the `untrack` guard at 165 already protects a user-chosen plan), or clear `state.recommendation` whenever connectors/website change after it was computed.

### U-22 — Heavy animation (scanner, blur step transitions, spinners) with no `prefers-reduced-motion` support
**Severity: Low** · `src/styles/global.css:2560`

The single stylesheet contains no `prefers-reduced-motion` query and no JS uses `matchMedia`. Onboarding runs a perpetual scanner glow (`onboarding-scanner-move 5.5s ... infinite`), 520ms blur+translate step transitions, infinite spinners, and an autoplaying video (`IntroStep.tsx:21`); none are suppressed for motion-sensitive users.

**Fix:** Add a `@media (prefers-reduced-motion: reduce)` block disabling the scanner, replacing blur/translate transitions with opacity-only; suppress the video autoplay with a component-level `matchMedia` check.

### U-23 — Crawl progress `<meter>` styled for WebKit only; failed crawls leave the phase list with no failure marker
**Severity: Low** · `src/features/onboarding/components/steps/WebsiteStep.tsx:142`

The meter relies on `::-webkit-meter-bar`/`::-webkit-meter-optimum-value` (`global.css:2391-2400`); no `::-moz-meter-bar` exists, so Firefox shows the default native widget without the brand fill. Separately, when `status === 'failed'` (or `'cancelled'`), `activeCrawlPhase` returns -1 (`view.ts:16`) and the still-visible phase list renders all rows as inert gray circles with no failed indicator — only the detached error paragraph below explains anything.

**Fix:** Add `::-moz-meter-bar` styling (or replace the meter with a div-based bar), and render an explicit failed row state (red icon + "Mislyktes") when the crawl fails.

---

## 4. Reuse & consolidation

### R-01 — Step header cluster (eyebrow + h1 + lead) re-implemented in all seven step renderers
**Severity: Medium** · `src/features/onboarding/components/steps/WebsiteStep.tsx:20`

Every step repeats the identical `<section class="onboarding-copy onboarding-copy--X"><p class="onboarding-eyebrow">…</p><h1>…</h1><p>…</p>` composition: WebsiteStep 20-23, IntroStep 3-6, OrganizationStep 30-33, ConnectStep 20-23, SocialProofStep 6-9, AssemblyStep 11-14, plus an inline eyebrow-only copy for paywall in `OnboardingPage.tsx:504`. Differs only in modifier class, eyebrow, title, lead.

**Fix:** Extract `OnboardingStepSection` (props: `eyebrow`, `title`, `lead`, `modifier`, `children`) following the existing `components/shared/` pattern; emits identical DOM so the CSS keeps working.

### R-02 — Step action footer (primary Button + skip link) duplicated in three steps
**Severity: Medium** · `src/features/onboarding/components/steps/WebsiteStep.tsx:92`

The `.onboarding-actions` cluster pairing `<Button variant="primary" size="sm">` with an `<OnboardingLinkButton>` skip is rebuilt in WebsiteStep 92-124 (four Switch branches each restating the Button), OrganizationStep 98-111, ConnectStep 55-60 — and the pattern also recurs in AssemblyStep:27, PaywallStep:93, SocialProofStep:23.

**Fix:** Extract `OnboardingStepActions` (`primaryLabel`, `onPrimary`, `primaryDisabled`, `skipLabel?`, `onSkip?`, plus `fullWidth`/class passthrough); WebsiteStep's Switch then varies only labels/handlers.

### R-03 — PaywallStep re-implements the shared Badge primitive with raw spans and parallel CSS
**Severity: Medium** · `src/features/onboarding/components/steps/PaywallStep.tsx:46`

PaywallStep renders `<span class="onboarding-paywall-card__badge">`/`--trial` (46-55) and `<span class="onboarding-paywall__trial-pill">` (30) — all pill badges. The shared `Badge` (`src/shared/ui/Badge.tsx`, `.badge` at `global.css:377-403`) already provides the pill + tone modifiers and is used by sibling steps (ConnectStep:42, OrganizationStep:66/77). The duplicate CSS even introduces two slightly different greens for the same trial/success semantic: `#25734b/#d9f6e7` vs `#1f5135/#ddfbea`.

**Fix:** Add a `success` tone to Badge backed by tokens (`--success-fg`/`--success-bg`), use `<Badge tone="success">` in PaywallStep, delete the two pill clusters. (Verifier: the pills differ typographically from `.badge`, so Badge may need a small size/uppercase variant for a pixel-faithful swap.)

### R-04 — AuthPage hand-rolls content-height measurement instead of reusing `createElementHeight`
**Severity: Medium** · `src/features/auth/components/AuthPage.tsx:58`

AuthPage implements content-height tracking manually (contentRef + createEffect + rAF + scrollHeight, lines 23, 26, 58-65) to animate the form panel, while the shared `createElementHeight` (`src/shared/ui/verevon/createElementHeight.ts`) does this with a ResizeObserver and is used by OnboardingPage for the same purpose. The hand-rolled version only re-measures when `mode()` changes — verified to miss locale-toggle text changes the shared helper would catch.

**Fix:** Drop-in replace with `createElementHeight<HTMLDivElement>()` — `setElement` is assignable to `onContentRef`, and the mode-dependent offset layers cleanly on top.

### R-05 — Viewport-height tracking and card-scale formula duplicated between OnboardingPage and AuthPage
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:121` *(merged: flagged independently by the redundancy and reuse lenses)*

OnboardingPage 56-58, 82-86, 121-125 (viewportHeight signal + resize listener + `Math.min(1, Math.max(0.52, (height - 18) / baseHeight))`) duplicate AuthPage 22, 46-56, 30-34 nearly verbatim, including the magic constants 0.52 and 18 — which appear nowhere else in the repo. Only the base height (1140 vs 1080) and onboarding's 1.04 multiplier differ.

**Fix:** Extract `createViewportCardScale(baseHeight, opts)` next to `createElementHeight` in `src/shared/ui/verevon` (owning the resize listener via `onCleanup`), naming the 18px chrome inset and 0.52 floor as constants; compose the 1.04 multiplier on top in onboarding.

### R-06 — Duplicated OnboardingScreen invocation with copy-pasted style objects and a dead `--onboarding-rail` custom property
**Severity: Medium** · `src/features/onboarding/components/OnboardingPage.tsx:546`

The paywall `Show` branch repeats the entire `<OnboardingScreen>` invocation (538-554 vs 579-594): eight identical props and byte-identical `screenStyle`/`chromeStyle` objects. `'--onboarding-rail': '#FF2E63'` (548, 589) is never consumed — no CSS rule reads `var(--onboarding-rail)`; the rail color is instead hardcoded as `#ff2e63` in 7 rules (`global.css:889, 1521, 1527, 2398, 2421, 3194, 3203`). `'--onboarding-accent': '#111111'` merely restates the CSS fallback at 2087-2088.

**Fix:** Define both tokens once in CSS, switch the hardcoded rules to `var(--onboarding-rail)` (note: `global.css:889` is `.auth-back-link:hover`, used outside `.onboarding-screen` — token must be `:root`-scoped or that rule excluded), delete both inline style objects, and collapse the two invocations into one (only `children`, `backHref`, and the `paywall` flag differ; verified safe — props are read reactively in OnboardingScreen). Same change resolves B-09.

### R-07 — Onboarding CSS restates its own defined tokens as raw hex literals
**Severity: Medium** · `src/styles/global.css:1394`

`global.css:1394-1403` defines `--onboarding-text:#1c1c1c`, `--onboarding-muted:#a09890`, `--onboarding-label:#6b6660`, `--onboarding-copy:#66615b`, `--onboarding-surface:#ffffff` — yet the onboarding rules below mostly use literals: `#1c1c1c` ×11, `#a09890` ×10, `#6b6660` ×9, `#66615b` ×5, `#ffffff` ×19. Changing the palette requires touching dozens of rules even though tokens exist; sibling rules already use the tokens, proving inconsistency.

**Fix:** Replace literals with `var(--onboarding-*)` in rules rendered under `.onboarding-screen`. **Caveat (verified):** a blind file-wide sed is NOT safe — a handful of hits are auth-screen or global rules where the tokens are not defined (e.g. `.auth-hero-copy` at 959/968; several `#ffffff` in `:root`/buttons), and `VerevonFooterLinks` reuses `.onboarding-footer-links` on the auth screen. Scope replacements to onboarding rules or move the tokens to `:root`. Pairs with U-15 (the muted token should also be darkened).

### R-08 — Inter font stack literal repeated 44 times instead of a font token
**Severity: Low** · `src/styles/global.css:1780`

The literal stack `Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` appears 44 times, while the file already demonstrates the token pattern with `--font-geist-sans` (`:root` line 12).

**Fix:** Add `--font-inter: ...` to `:root` and replace the occurrences. Two of the 44 embed the stack as the fallback tail after `var(--font-geist-sans), "Geist"` — those become `var(--font-geist-sans), "Geist", var(--font-inter)`.

### R-09 — Adorned-input control duplicated: `.auth-field__control` vs `.onboarding-url-control`
**Severity: Low** · `src/styles/global.css:1859`

Two parallel CSS clusters implement the same "icon/prefix + borderless inner input inside a bordered row" control: `.onboarding-url-control` (1853-1888, WebsiteStep's Globe-prefixed URL field) and `.auth-field__control` (1000-1029, AuthFormPanel's three icon-led fields). Both sit inside VerevonField, both strip the inner input's border. (Verifier: divergence is more than radii/colors — alignment, sizing, shadow — but all expressible as modifier skins.)

**Fix:** Extract a shared `VerevonInputControl` (leading/trailing adornment + children input) with one `verevon-input-control` cluster plus auth/onboarding modifier skins.

### R-10 — AuthFormPanel tabs hand-roll a selected-toggle pattern
**Severity: Low** · `src/features/auth/components/sections/AuthFormPanel.tsx:49`

The signin/signup toggle uses raw buttons with string-concatenated classes (the only two template-literal class concats in all of `src/`) and no `aria-selected`/`aria-pressed` — worse, the wrapper declares `role="tablist"` while children lack `role="tab"` (invalid ARIA). The shared `VerevonChoiceChip` shows the codebase's selected-toggle pattern (aria-pressed + `--selected` via `cn`).

**Fix:** Extract a small `VerevonTabs` primitive (`role="tablist"` + `role="tab"`/`aria-selected`) in `src/shared/ui/verevon`; at minimum switch to `classList`. (Verifier: direct VerevonChoiceChip reuse does not work here — wrong ARIA semantics for tabs and conflicting base styles — prefer the tabs primitive.)

### R-11 — Two separate progress-checklist implementations: WebsiteStep phase list vs AssemblyStep assembly list
**Severity: Low** · `src/features/onboarding/components/steps/AssemblyStep.tsx:16`

WebsiteStep 54-86 renders a crawl checklist (`--done`/`--active` rows, CheckCircle2/Loader2/Circle icons; `.onboarding-phase-list` CSS) while AssemblyStep 16-25 renders the same concept with a second implementation (`.onboarding-assembly-list`) using raw text glyphs '✓' and '·'. The two CSS clusters duplicate layout/typography and have already drifted (hardcoded `#1f1b17` vs `var(--onboarding-text)`).

**Fix:** Extract an `OnboardingChecklist` (items, activeIndex/doneCount) reusing the phase-row markup and lucide icons; delete the `.onboarding-assembly-list` cluster.

### R-12 — `onboardingFooterLinks` imported from the onboarding feature by auth
**Severity: Low** · `src/features/auth/components/shared/AuthScreen.tsx:2`

`AuthScreen.tsx:2` imports `onboardingFooterLinks` from `@/features/onboarding/lib/model` (`model.ts:167`) to feed shared `VerevonScreen`/`VerevonFooterLinks` primitives — verified to be the **only** cross-feature import in the entire codebase. The links ("Om oss", "Personvern", …) are app-wide chrome, not onboarding data.

**Fix:** Move the constant to `src/shared/ui/verevon` (e.g. `verevonFooterLinks`) and import it from both screens.

### R-13 — Shared Verevon primitives carry `onboarding-*` class names and baked-in Norwegian copy
**Severity: Low** · `src/shared/ui/verevon/VerevonBackButton.tsx:20`

Five promoted primitives still emit feature-named classes: VerevonBackButton `onboarding-back`, VerevonLanguageButton `onboarding-language-pill`, VerevonStepDots `onboarding-dots`, VerevonStepPill `onboarding-step-pill`, VerevonFooterLinks `onboarding-footer-links` — unlike the correctly named `verevon-text-button`/`verevon-icon-button`/`verevon-switch`. Auth then restyles via override classes (`.auth-back-link`, `.auth-language-switcher`, `.auth-footer-links`). VerevonStepPill also hardcodes "Steg {n} av {m}" inside a shared primitive.

**Fix:** Rename the CSS clusters to `verevon-*` (the strings occur nowhere outside the five components and global.css — contained sed) and give VerevonStepPill a `label`/format prop, matching VerevonBackButton's existing overridable-label pattern.

### R-14 — `OnboardingLinkButton`'s `'link'` emphasis value is a dead alias adding a mapping layer over VerevonTextButton
**Severity: Low** · `src/features/onboarding/components/shared/OnboardingLinkButton.tsx:15`

The wrapper exists solely to remap emphasis `'link' | 'large'` to `'default' | 'large'`. No call site ever passes `emphasis="link"` (only `emphasis="large"` at OrganizationStep:48; all others pass none), so the `'link'` member and the ternary are dead; `undefined` already behaves as `'default'` in VerevonTextButton. Similarly `OnboardingField` only adds the static class `'onboarding-field'` to VerevonField.

**Fix:** Delete OnboardingLinkButton and use VerevonTextButton directly (verified drop-in at all five call sites), or align the union and pass through. Keep OnboardingField only if more behavior is planned.

### R-15 — AssemblyStepVisual fact tiles duplicate the dashboard stat-tile pattern
**Severity: Low** · `src/features/onboarding/components/steps/AssemblyStep.tsx:53`

AssemblyStep 53-66 hand-writes three `<article><strong>{value}</strong><span>{label}</span></article>` tiles (`.onboarding-summary-card__facts`), the same shape as `.tier-list article` in SettingsPage — and the verifier found two more copies (InboxPage `.handoff-grid`, KnowledgePage `.graph-grid`). (`MetricCard` is label-first with a required delta, so it is the inspiration, not a drop-in.)

**Fix:** Extract a `StatTile` primitive (value/label, parent-driven skins) and use it for assembly facts, settings tier list, inbox, and knowledge tiles; consolidate the article CSS clusters.

---

## Reuse / extraction recommendations

| # | Component / asset | Action |
|---|---|---|
| R-01 | Step header (eyebrow+h1+lead) ×7 | **Extract** `OnboardingStepSection` in `src/features/onboarding/components/shared/` |
| R-02 | Primary+skip action footer ×6 | **Extract** `OnboardingStepActions` in `src/features/onboarding/components/shared/` |
| R-03 | Paywall pill spans | **Reuse** `src/shared/ui/Badge.tsx` (+ new `success` tone backed by `--success-fg/bg` tokens) |
| R-04 | AuthPage rAF height measurement | **Reuse** `src/shared/ui/verevon/createElementHeight.ts` |
| R-05 | viewportHeight + cardScale (Auth & Onboarding) | **Extract** `createViewportCardScale(baseHeight, opts)` into `src/shared/ui/verevon` |
| R-06 / B-09 / C-13 | Duplicated `<OnboardingScreen>` + inline style objects | **Collapse** to one invocation; move `--onboarding-rail`/`--onboarding-accent` into CSS |
| R-07 | Onboarding hex literals (~54 across 5 colors) | **Reuse** existing `--onboarding-*` tokens (scoped replacement, not blind sed) |
| R-08 | Inter font stack ×44 | **Extract** `--font-inter` token in `:root` |
| R-09 | `.auth-field__control` / `.onboarding-url-control` | **Extract** `VerevonInputControl` in `src/shared/ui/verevon` |
| R-10 | Auth signin/signup tabs | **Extract** `VerevonTabs` (proper `role="tab"`/`aria-selected`) in `src/shared/ui/verevon` |
| R-11 | Crawl phase list vs assembly checklist | **Extract** `OnboardingChecklist`; delete `.onboarding-assembly-list` |
| R-12 | `onboardingFooterLinks` cross-feature import | **Move** constant to `src/shared/ui/verevon` as `verevonFooterLinks` |
| R-13 | `onboarding-*` classes in 5 shared primitives | **Rename** to `verevon-*`; add label prop to `VerevonStepPill` |
| R-14 | `OnboardingLinkButton` dead alias | **Delete**; use `VerevonTextButton` directly |
| R-15 | Stat tiles (assembly/settings/inbox/knowledge) | **Extract** `StatTile` primitive in `src/shared/ui` |
| C-03 | Duplicated `setPlan` branches | **Extract** `persistPlan(plan)` + shared `sourceCount()` memo |
| C-07 | Inline protocol regex | **Reuse** `stripUrlProtocol` from `lib/view.ts` |
| C-13 | OnboardingPage async flows | **Extract** `createOnboardingController(...)` factory under `lib/` |
| C-14 | Crawl magic numbers (34/25/12/94/18, maxPages=3, >6) | **Extract** named constants in `lib/model.ts` |

---

## Unverified findings

None. The adversarial verifier processed every finding; all reported findings above were independently confirmed (`isReal = true`).

---

## Checked and dismissed

Two candidate findings were refuted during adversarial verification and are **not** counted above:

1. **"Graph preview polls every 2.5s with no stop condition or backoff"** (`src/features/onboarding/lib/queries.ts`) — False as framed: `enabled()` reactively gates both `enabled` and `refetchInterval` (asserted in `queries.test.ts`), TanStack pauses background-tab polling by default, and the poll is the *only* channel surfacing the actively-growing graph during the fire-and-forget website ingest — the recommended "stop once settled" fix would break the live graph-assembly UX. Residual nit (idle polling of an empty graph after skipping the website step) is far below the claimed severity.
2. **"`recommendationContext` duplicates website data in both `website` and `websites` keys"** (`src/features/onboarding/components/OnboardingPage.tsx`) — The duplication is deliberate contract compliance, not prunable redundancy: the gateway's `RecommendContext` (`verevon-gateway-rs/src/onboarding/contracts.rs:136-144`) declares both keys and consumes both (`websites.len()` drives the source-count fallback; `website.agent_brief`/`url` drive keyword tiering and scope signals). Removing either changes gateway behavior. Only the side suggestion (replace `Record<string, unknown>` with a named type mirroring the contract) survives as hygiene.
