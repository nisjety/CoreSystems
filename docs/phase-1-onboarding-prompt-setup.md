# Phase 1 onboarding · prompt setup

Single source of truth for every prompt the auth-embedded onboarding
wizard needs — designer briefs for the 7 right-pane asset slots, LLM
system prompts the routes call, per-step microcopy, and the QA
checklist the team runs before each release.

This document is intended to be copy-pasted into Runway / Sora /
Midjourney / Veo for video, Figma AI / Midjourney for static assets,
and Claude / GPT for LLM system prompts. Keep the brand prefix at the
top of every visual prompt so style stays consistent across slots.

---

## 0 · Brand prefix (paste at the top of every visual prompt)

> **Velion onboarding · house style.** Aesthetic: Intercom-cream
> minimalism × Chatbase simplicity. Background `#EDEBE7`
> (cream). Right-pane card `#F4EFE5`. Text `#1F1B17`. Single accent
> `#FF2E63` (coral). Editorial serif (Cormorant Garamond) on
> headlines, Inter on body. 60 fps for video; portrait 9:16 inside a
> 432×640 frame; subtle 4 % film grain to bridge with the auth-page
> noise. Motion is slow and confident, 1.2× ease-in-out, no hard
> cuts. No people, no faces, no hands — abstract product UI only.
> Audio: mute. End on a still that fades back into cream.

---

## 1 · Per-slot asset prompts

### Slot 1 — Post-sign-in product reveal (3–5 s loop, video)

- **Path**: `public/videos/onboarding/product-reveal.webm`
- **Step**: `PostSignInStep`
- **Format**: webm, 432×640, 4 s, seamless loop

> Cinematic 4-second loop showing the Velion dashboard coming alive.
> Camera glides across a clean cream surface; cards slide into place
> — a chat thread reveals an AI agent reply, a knowledge graph blooms
> behind it, a small token-cost meter ticks up, a CSAT chart fills.
> Coral accent only on the live indicator. Ends on a still frame
> showing the agent reply card centred so the transition into the
> "smart pitch" overlay is clean.

---

### Slot 2 — Organization personalization (3 s loop, video)

- **Path**: `public/videos/onboarding/org-personalization.webm`
- **Step**: `OrganizationStep`
- **Format**: webm, 432×640, 3 s, seamless loop

> 3-second loop. A stylised globe (continents in line-art, no
> oceans), the user's org pin drops in, three concentric rings
> ripple outward. Below the globe a one-line caption scrolls
> right-to-left: "Fetching public business registry data ·
> employees · domain · NACE code". Ends on the globe still + the
> caption holding at the centre.

---

### Slot 3 — Website snippet drop (live, JS-driven)

- **Live component**: `SnippetDropFolder` inside `WebsiteStep.tsx`
- **Backend**: `POST /api/onboarding/crawl-preview {url, brief?}` →
  SSE stream of `snippet` events `{kind, title, excerpt?, thumbUrl?,
  url, contentType}` from `quarry-control:8081`
- **Asset**: no video — only static folder card mock at
  `public/imagens/onboarding/snippet-folder-mock.png` (optional)

> White folder card centred on the cream pane (hybrid of Taskello
> note-card + Mobbin "4 Files / 500–700 MB" folder). Small tab on
> the top edge. Four card shapes — text card with title + 2-line
> excerpt, image thumb with 96×72 photo, file row with PDF/DOC
> icon, rounded link chip — fall from above in parallel, each
> picking its own x-offset / delay / fall duration so 2–3 are
> always in flight. Progress bar at the bottom fills as snippets
> land. Counter beside it (`12 / 40` snippets, `1.4 MB`). When
> the crawl completes the folder tab flips up showing a checkmark.

---

### Slot 4 — Knowledge graph reveal (live, SVG-driven)

- **Live component**: `GraphReveal` inside `ConnectStep.tsx`
- **Backend**: `GET /api/onboarding/graph-preview` →
  `{nodes, edges, counts, warning?}` from `dpv2-graph-index:9201`,
  polled every 3 s and on every connector click
- **Asset**: none — SVG rendered live

> Dark canvas (`#0F0F10`) sitting on top of the cream pane (rounded
> 16 px). Up to 60 nodes radial-laid around a single golden org
> anchor (`#F5E5A8`). Cluster colour by entity group: person → light
> blue (`#9BD0E8`), product → coral (`#F0A8A1`), document → lavender
> (`#C7B0F0`), channel → mint (`#A8E0B6`), other → mid-grey
> (`#5B5B5C`). New nodes fade in, highlight green (`#34D399`) for
> 1 s, then settle to their cluster colour. Edges are thin 0.7 px
> light-grey lines (`#3B3B3D`). Bottom-right counter:
> `nodes 42 · edges 71 · groups 4`. Bottom-left shows the `warning`
> text in 10 px white/70 when present.

---

### Slot 5 — Social-proof logo wall (static SVG sprite)

- **Path**: `public/imagens/onboarding/logos.svg`
- **Step**: `SocialProofStep`
- **Format**: SVG sprite with `<symbol>` definitions

> Six greyscale (`#444`) logos in a 3×2 grid, evenly spaced, each in
> its own 80×40 cell. No frames, no colour. Hover: each logo lifts
> +1 px and tints to `#111`. Logos: Apple, Microsoft, Slack, Notion,
> Zammad, Sanity. Single SVG sprite so each tile is
> `<svg><use xlink:href="#logo-apple" /></svg>`.

---

### Slot 6 — Plan card sparkle (CSS-only)

- **Live component**: `PaywallStep.tsx`
- **Asset**: none — CSS gradient ring

> The 4 plan cards + 1 trial card use the Intercom-style border. The
> LLM-recommended card gets a thin gradient outline (coral → indigo
> → emerald) animated at 8 s loop. The trial card is the same
> rounded-xl shape but with no price block — instead a coral
> `14 days free` badge top-right.

---

### Slot 7 — Dashboard assembly finale (4–8 s loop, video)

- **Path**: `public/videos/onboarding/dashboard-assembly.webm`
- **Step**: `AssemblyStep`
- **Format**: webm, 432×640, 6 s, seamless loop

> 6-second sequence. Camera pulls back from a single chat reply card
> (the same shot we ended on in Slot 1). One by one, the dashboard
> chrome assembles around it: sidebar slides in from the left, top
> navbar drops in from above, knowledge graph card fades in on the
> right, usage stats card slides up from the bottom. Final frame is
> the empty Velion dashboard with the user's org name appearing in
> the top-left workspace switcher. Caption fades in over the final
> frame: "Workspace klar." (Norwegian for "Workspace ready.")

---

## 2 · LLM system prompts

### Plan recommender (`/api/onboarding/recommend-plan`)

Model: `claude-haiku-4-5` (configurable via `ONBOARDING_REC_MODEL`).
Response format: `json_schema` with required `planId` and `reason`
fields. Plane: model-plane via `mintPlaneToken({audience: 'model-plane'})`.

System prompt (verbatim):

> Du er en intern rådgiver i Velion. Du svarer KUN med JSON som
> matcher response_format. Bruk norsk i feltet "reason".

User-prompt template (assembled by `buildPrompt` in the route — keep
in sync if you edit either):

> Organisasjon: {{name}}
> Størrelse: {{size}}
> Nettside: {{url}}
> Agentens oppgave: {{agentBrief}}
> Koblede kilder: {{connector labels, comma-separated}}
>
> Velg den planen som passer best for dette teamet. Mulige verdier:
> - hobby (1 person, sideprosjekt)
> - standard (lite team som vokser)
> - pro (etablert team med flere kilder)
> - enterprise (51+ ansatte, eller flere connectorer)
> - trial (de bør teste først)

**Deterministic floor** is applied *after* the LLM responds (in
`applyDeterministicFloor`):

| Signal                                          | Minimum plan |
|-------------------------------------------------|--------------|
| 5+ connectors                                   | enterprise   |
| 3+ connectors                                   | pro          |
| 1+ connector OR (website + size ≥ medium)       | standard     |
| website only, solo/small                        | LLM pick     |

The floor only ever *upgrades* the LLM pick — it never downgrades.

---

## 3 · Per-step microcopy (left pane)

| Step | Eyebrow                       | Title (serif)                          | Body (Inter, optional)                                                                                          | CTA          | Skip            |
|------|-------------------------------|----------------------------------------|-----------------------------------------------------------------------------------------------------------------|--------------|-----------------|
| 1    | —                             | Du er inne.                            | Vi forbereder arbeidsplassen din. Det tar et øyeblikk.                                                          | — (auto)     | —               |
| 2    | Step 2 of 6 · Organisasjon    | Hva heter organisasjonen din?          | Søk i Enhetsregisteret eller skriv inn navnet manuelt. Du kan endre alt senere.                                 | Fortsett     | Skip verification (→ free-text mode) |
| 3    | Step 3 of 6 · Website         | Show Velion where to learn.            | Paste your company URL. We'll skim the public pages and turn them into the agent's first knowledge base.        | Continue     | Skip for now    |
| 4    | Step 4 of 6 · Sources         | Connect your knowledge.                | Pick the systems Velion should learn from. Each one becomes a cluster of nodes in the graph on the right.       | Continue     | Skip            |
| 5    | Step 5 of 6 · Sosial proof    | Selskap som bygger med Velion.         | Vi gir samme infrastruktur som store team — uten oppsettet.                                                     | Se planene   | —               |
| 6    | Step 6 of 6 · Plan            | Velg plan.                             | LLM-generated reason from `recommendation.reason` (Norwegian).                                                  | Velg {name}  | —               |
| 7    | Ferdig                        | Setter sammen Velion til deg.          | Vi flytter inn alt vi har samlet — kunnskap, integrasjoner og agenten din — og åpner dashboardet om noen sek.   | — (auto)     | —               |

Progress dots: 6 dots, current step pill-wide, completed pills dark,
upcoming pills cream-grey. Sits above the eyebrow on every input step.

---

## 4 · Failure-mode copy

| Trigger                                              | UI surface                                  | Copy                                                                                       |
|------------------------------------------------------|---------------------------------------------|--------------------------------------------------------------------------------------------|
| Crawl-preview SSE fails / Quarry unreachable          | Below the form, amber                       | `Live crawl er midlertidig utilgjengelig — fortsetter med eksempler.`                       |
| Crawl-preview emits `warning`                         | Below the form, amber                       | Echo `warning.message` verbatim.                                                            |
| Graph-preview returns synthetic seed (empty org)      | Bottom-left of canvas, 10 px white/70       | `Ingen kunnskap er indeksert ennå — vi viser et eksempeloppsett til kildene dine kommer inn.` |
| Graph-preview unreachable                             | Bottom-left of canvas, 10 px white/70       | `graph-index is not reachable`                                                              |
| Plan recommendation LLM unavailable                   | Replaces `recommendation.reason`            | `Anbefalingen er midlertidig utilgjengelig — prøv gratis.`                                  |
| Plan recommendation skipped (no signal)               | Replaces `recommendation.reason`            | `Du har ikke koblet til kilder ennå — start med 14 dagers prøveperiode.`                    |
| localStorage write fails (Safari private / Lockdown)  | Browser console warn (no UI)                | `[onboarding] localStorage write failed; resume-on-refresh disabled for this session`        |

---

## 5 · QA checklist (run before each release)

- [ ] Sign in → wizard mounts in the auth chrome (cream card, 1.15fr / 0.85fr grid). No flash of CallbackModal.
- [ ] Refresh on any step → step is preserved from `localStorage`.
- [ ] Step 2: BrregSearch returns Aquatiq AS for org-number `983 851 245`. Skip-verification reveals plain input.
- [ ] Step 3: Submitting a real URL streams ≥4 multi-type cards within 6 s. Image cards render with crossorigin; CORS-blocked ones fall back to placeholder.
- [ ] Step 3: Killing Quarry locally still advances the step (fallback snippets + warning).
- [ ] Step 4: Toggling a connector immediately refreshes the graph. Polling continues every 3 s.
- [ ] Step 4: Empty graph for a new org shows seed nodes + warning.
- [ ] Step 6: With 5 connectors picked, recommended card is `enterprise` regardless of LLM pick.
- [ ] Step 6: With 0 connectors + no website, recommended card is `trial`.
- [ ] Step 7: Final tick fires `PUT /api/user/me/onboarding-state {step:'complete'}` + `POST /api/user/onboarding/complete` before `/dashboard`.
- [ ] Reaching `/dashboard` does NOT redirect back to `/login` (OnboardingGuard sees `completedAt`).
- [ ] All progress dots render, current pill is wide, completed pills are dark.
- [ ] Tab away mid-stream and back → no duplicate SSE connections, no leaked timers.

---

## 6 · Where to extend

- **New step**: add it to `ONBOARDING_STEPS` (`state/types.ts`),
  register in `OnboardingFrame` switch, add row in §3 microcopy table,
  bump progress-dot total.
- **New connector**: append to `CONNECTORS` in `ConnectStep.tsx` with
  category `chat | docs | tools`. Backend graph nodes will appear
  automatically once Data Plane indexes them.
- **New plan tier**: update `PLANS` in `PaywallStep.tsx`, extend the
  `planId` union in `state/types.ts`, mirror in the LLM schema in
  `recommend-plan/route.ts`, and add a floor row in §2 if the new
  tier should be auto-recommended.
- **New asset slot**: append a new section to §1 above with the same
  schema (path, step, format, prompt) and reference it from the step
  component's right pane.
