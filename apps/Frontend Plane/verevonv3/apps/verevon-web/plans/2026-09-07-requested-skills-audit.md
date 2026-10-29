# Verevon web: requested skills audit
Date: 7 September 2026; runtime verification and remediation updated 8 September 2026; reference-led design audit finalized 9 September 2026; selected implementation verification updated 10 September 2026

**Current verdict: the launch-blocking accessibility and readability findings are resolved.** The V2 and V3 variants and fragile full-screen page loader have been removed; the menu, footer, response-time hero, reduced-motion behavior, corridor transfer, connector marquee, hidden SVG work, homepage client boundary, and public discovery metadata have been corrected. The Product Loop's Composer stage is visually corrected, while its direct scroll-time geometry remains a P2 performance item. The 9 September reference audit is complete and records a focused design roadmap. The selected Problem and navbar modules are implemented; the remaining reference-led modules remain recommendations. Remaining validation work is production and physical-device performance measurement. The baseline audit below retains its original evidence for traceability.

This began as an audit. The 8 September remediation section records the application changes made from its findings. The 9 September reference pass changed documentation only.

## Scope and evidence

Reviewed the standalone Next.js **16.2.9**, React **19.2.4**, Tailwind v4 marketing package. This is distinct from the surrounding Solid/Vite application. Root route uses `VerevonHome`; the former `/v2` and `/v3` preview variants were removed on 8 September. Other routes cover workflow, response time, shared context, controlled work, trust, and a recording showcase.

Evidence comes from source, import reachability, design documentation, metadata/configuration, local asset sizes, calculated CSS color contrast, the Codex built-in browser, and local production-preview CDP measurements. Two read-only motion reviewers were used as explicitly requested by the improve-animations workflow; cited findings were checked again.

Following the user's authorization to use the built-in browser or the supplied Chrome DevTools MCP repository, dependencies were restored from the offline pnpm cache and a production preview was built and inspected at `http://127.0.0.1:3100`. Browser checks covered all nine page routes, desktop/mobile layouts, menu keyboard behavior, two tab interfaces, and reduced-motion playback. Screen-reader testing, Figma/Storybook comparison, production crawler/CDN behavior, physical-device profiling and slow-motion recordings remain unverified.

The initial interrupted lint/typecheck attempts were superseded by completed checks. The remediation validation repeated `node node_modules/next/dist/bin/next build` successfully, with TypeScript and static generation passing; `pnpm test` passed all 38 tests; and `node node_modules/eslint/bin/eslint.js src --no-cache` exited 0 with 11 `no-img-element` warnings in `src/components/ui/logo-cloud.tsx` (not imported by the active homepage). No formatter, deployment or commit was performed; existing monorepo changes were preserved.

## Remediation update — 8 September 2026

The table describes the live code state after the fixes. “Mitigated” means the risky behavior is prevented in code, while its post-change production metric still needs measurement.

| Audit item | Status | Implemented outcome |
| --- | --- | --- |
| F01 menu focus | Resolved | The menu now has initial focus, tab containment, Escape handling, focus restoration, and an inert/hidden page background. |
| F02 footer contrast | Resolved | Normal footer copy now uses the 65% text token, calculated at approximately 5.39:1 on the declared background. |
| F03/F04 destinations | Resolved | Removed dead homepage anchors and misleading social/search/legal labels; navigation now uses actual product, trust, mail, and contact destinations. |
| F05 reduced motion | Resolved for audited videos | Homepage and response-time hero videos wait for the no-preference setting before playing, and pause when reduced motion is enabled. |
| F06 corridor transfer | Resolved | The below-fold corridor video has no source until it is within 200px of the viewport and remains a poster for reduced-motion visitors. Its supplied media is now a silent 1280×720, 24fps H.264 rendition of 5.96 MiB, down from the 71.42 MiB 1440p source; local browser playback was verified. |
| F08 reduced-motion marquee | Resolved | Connector names render once in a wrapped static list without a mask or clipping. |
| F14 short mobile menu | Resolved | The menu is a scrollable flex layout, so every control remains reachable at 320×568. |
| F15 response-time hero | Resolved | An inverse typography variant prevents semantic type styles from overriding the light hero copy. |
| F07 hidden SVG work | Resolved | Signal routes initialize only within 300px of view on desktop visitors without reduced-motion preference; they pause when leaving view. Hidden/mobile SVGs do no animation setup. |
| F09 product-loop geometry | Open P2; visual regression and entrance motion corrected 9 September | The composited wrapper experiment scaled and clipped the Composer content. The verified remote-default implementation was restored so the frame and copy keep their intended geometry. Stage 04 now removes the large peach image and gradient backdrop while keeping its heading, composer and supporting copy above the loop circle until the stage 05 crossfade. The entrance reproduces the measured reference behavior with a centered, linear enlargement from about 43vw to the full viewport. Direct scroll-time frame dimensions still need profiling and a future optimization that does not transform the content itself. |
| F10 homepage client boundary | Resolved | Homepage composition is again a Server Component. A small client shell owns the menu, scroll state, and footer parallax while section islands load only their own browser code. |
| F11 canonical and social metadata | Resolved | Added the `https://verevon.ai` metadata base, route canonicals, Open Graph URLs, Twitter metadata, and a generated 1200×630 social image. |
| F12 sitemap, crawler policy, entity data | Resolved | Added a sitemap, explicit crawler rules that permit search and block training crawlers, plus Organization, WebSite, and SoftwareApplication JSON-LD using visible product claims. |
| F13 showcase indexing | Resolved | The recording showcase now emits `noindex, nofollow` and remains outside the sitemap. |
| P3 motion preference updates | Resolved | Hero parallax now rebuilds or clears through `gsap.matchMedia` when reduced-motion changes live. Menu links override their initial opacity, translate, animation, and stagger for reduced-motion visitors. |
| Layer hydration under reduced motion | Resolved 10 September | `LayerSection` now reads the shared hydration-safe motion preference. Server markup and the first client render use the same layer opacity, scale and position, then apply the live media query after mount. A clean reload with reduced motion enabled produced no hydration mismatch or Motion warning. |
| V2/V3 cleanup | Resolved | Removed both routes, their component trees, preloader CSS, V3-only assets, and stale documentation references. `/v2` and `/v3` return 404. |
| Runtime loader | Resolved | Removed the initial full-screen loader, whose server-rendered state remained visible when development HMR failed. The local `dev` command now uses Webpack because the active Turbopack session panicked with “Next.js package not found.” |
| Problem image galaxy | Resolved and refined 10 September | Replaced the small collage with one `75svh` editorial scene and a 630px short-screen safety floor. Its 32 particles are 30% smaller at 35px/73px, reuse the curated human/work/product set through five relative speeds and nine depth layers, and move at 45% of the earlier rate after a 55% speed reduction. They continue to react to scroll velocity and direction, scale by vertical position, recycle at both edges, and stop for reduced motion. The earlier moving signal-guide layer is restored behind the particles and type, while visible section divider borders are removed. Its signals begin with the reveal and continue while the section is visible instead of pausing at the reveal's midpoint; the routes remain visible as a static guide when reduced motion is enabled. The section restores Verevon's original two-level structure: the thin Arbeit heading “Kunnskapen finnes. Men den er spredt.” uses the pre-footer statement's type scale, line height and tracking together with the scroll-linked word-opacity reveal, while the approved Verevon sentence sits beneath it as restrained body copy. The three original problem cards now follow in their own `ProblemCardsSection`, keeping this scene focused. |
| Problem cards | Implemented and refined 10 September | Restored the three original arguments, copy, destinations, posters and ordered video sequences as a separate section between the image galaxy and Product Loop. The active card expands on desktop; labels, keyboard controls and inactive-card buttons select a card. Video is requested only on hover, pauses and resets on exit, and stays on its poster for reduced motion. Its card height, gap and vertical padding are reduced by 20%, and the section divider borders are removed. |

Targeted browser verification at 320×568 confirmed initial close-button focus, focus wrapping within the menu, focus restoration to “Åpne meny”, a scrollable menu surface, an inert background, the response-time hero's light computed colors, and corridor source assignment only near the viewport. The optimized corridor video reached ready state 4 and played locally without a media or console error. On 9 September, the Product Loop entrance measured 571px, 943px, and 1314px wide at scroll progress 0, 0.5, and 1 on a 1314px content viewport. Its horizontal center remained at 657px in all three samples, confirming a centered linear enlargement from about 43vw through 72vw to the full viewport. Continuing the same forward scroll to stage 04 confirmed that only the Composer layer was visible, its layer and shared frame computed to transparent with no border or shadow, the removed peach background and overlay were absent from the DOM, and the heading, controls and supporting copy remained above the loop circle. The repaired local preview at `http://localhost:3000` has no loader element or console errors; it emits a canonical, generated Open Graph image and JSON-LD. `robots.txt`, `sitemap.xml`, the social-image route, the noindexed showcase and trust canonical all return 200. The V1 route returned 200; `/v2` and `/v3` returned 404.

The Problem rewrite was visually checked at 1329×1032 in the built-in browser. The final 10 September pass restores the original thin Arbeit, 16-character measure and word-by-word opacity sequence on the compact problem heading, followed by the approved Verevon sentence in the body tier. At the middle sample before this typography combination, the first, eighth and final heading words measured `0.9985`, `0.4065` and `0.1` opacity; after the section crossed its end boundary, all three measured `1` and the heading completed its 24px lift. Particles continued crossing behind the type, and the CTA remained readable below it. Earlier stationary DOM samples measured the first particle moving from `translateY(1054.4px)` to `985.5px` over 500ms; PageDown advanced it to `934.7px`, and continued observation confirmed edge recycling plus scale interpolation. After the requested 55% speed reduction, a clean-reload stationary sample at the lowest relative speed moved 55.9px over one second. The final section measured 774px in the 1032px viewport, exactly three-quarters of its height, while desktop particles retained their 73px base width. The scrolled navbar was also checked in the same viewport: the full-width 77% cool-neutral material, dark links and icons remained legible while the 64px geometry stayed fixed. `pnpm build` compiled, type-checked and generated all 13 routes successfully. Two Vitest attempts did not start test code: the default fork pool reported six worker-start timeouts, and the single-thread retry stalled before its first test, so this change has build and browser verification but no completed automated test run.

Remaining priority: measure production transfer, decode, and scroll performance on a throttled physical device after deployment. Do not treat the pre-fix local network figures below as post-change performance results.

## Reference-led design audit — 9 September 2026

### Decision

**Documentation status:** complete. **Implementation status:** selected items in progress. The Wolverine-inspired problem scene and Finseo-inspired native-CSS navbar material are implemented; the remaining content, trust, product-proof, knowledgebase, and dependency decisions remain recommendations.

**Needs focused iteration; a visual reset is not justified.** Verevon already has a distinctive editorial system, generous spacing, strong type, purposeful photography, product UI demonstrations, and a documented motion language. The more important gap is explanatory: visitors still have to infer why Verevon is different, how knowledge becomes governed action, and what the product is doing in concrete moments.

The next design pass should make the operating model legible before adding more spectacle. The highest-value additions are a three-level AI comparison, a consistent series of product-proof scenes, and a more visible enterprise trust/infrastructure story. Glass and shader treatments can support those ideas in isolated places, but they are not the product narrative.

### Desktop gutter and density study — 10 September 2026

The reference pass measured the left edge of the primary editorial content at a
1440px desktop viewport. The result is a directional comparison rather than a
claim that every section on each site uses one immutable container:

| Reference | Observed gutter |
| --- | ---: |
| [Mimir](https://trymimir.com/) | 165px |
| [Ayfie](https://ayfie.com/) | 120px |
| [Nolla](https://www.nollahealth.com/) | 80px |
| [Sui](https://www.sui.io/) | 20px |
| [Wonderful](https://www.wonderful.ai/) | 60px |
| [Wolverine Worldwide](https://wolverineworldwide.com/) | 48px |
| [Finseo](https://www.finseo.ai/) | 64px |
| [Hellomatik](https://hellomatik.com/) | 96px |
| [Warmwind](https://warmwind.com/) | 168px |
| [Peec](https://peec.ai/) | 125px |
| [Attio](https://attio.com/) | 24px |
| [Osmo](https://www.osmo.supply/) | 30px |
| [Sana](https://sanalabs.com/) | 32px |

Across all thirteen references the mean is **79.4px** and the median is **64px**.
The five references selected for Verevon's premium-density target—Ayfie,
Wonderful, Warmwind, Finseo and Nolla—average **98.4px**, which supports a clean
**100px desktop gutter**. The homepage now resolves `--verevon-edge` to 100px at
desktop sizes, aligns the navbar, logo proof, main sections and footer to that
edge, and scales down fluidly for narrower screens.

The accompanying density adjustment is **10%**, revised from the initial 20%
trial after visual review. It applies to display headings and selected content
measures across Hero, Problem, Product Loop, Knowledge, Modules, Platform and the
closing statement. Body text, navigation labels, controls and touch targets retain
their established readable sizes. Scroll-driven product-frame geometry also stays
unchanged so the adjustment does not invalidate the Product Loop choreography.

The implementation was checked in the running local page after hot reload. At a
2400px browser viewport, where the fluid values had reached their caps, the navbar
reported 100px left padding, the hero heading reported 67.68px (90% of its former
75.2px cap), and the Problem heading reported 79.2px (90% of its former 88px cap).
The Problem scene remained centered and its image galaxy continued behind the text.
Targeted ESLint completed without findings, `git diff --check` passed, and
`pnpm build` compiled, type-checked and generated all 13 routes successfully.

This pass used the supplied screenshots, direct inspection of the public reference pages and their public repositories, plus Verevon's active homepage source. It applies Apple Design hierarchy and agency principles, the frontend design review's friction/craft/trust rubric, React client-boundary and bundle guidance, and the web-performance motion/rendering checks. There is no Figma or Storybook source in the evidence set, so exact design-file compliance was not assessed. The Chrome DevTools trace commands required by the web-performance skill are not exposed in the current session; this update therefore adds no new Lighthouse, Core Web Vitals, frame-rate, or GPU measurements.

### Cross-runtime alignment decision

The supplied Fjordlys chat direction has now been compared with the current
Verevon Web implementation. The result is documented in
[`2026-09-09-spa-web-design-alignment.md`](2026-09-09-spa-web-design-alignment.md).
Web and SPA already share their warm canvas, ink, surface, border, and coral
values. Their visible split comes mainly from fonts, navbar material and height,
product density, motion names, and the SPA's accumulated cascade. The alignment
plan makes Arbeit/Protokoll the shared brand voice, retains Geist for dense SPA
work, carries Web's full-width glass material into the SPA shell, and gives coral
and fjord blue one consistent human/machine-state meaning.

The Fjordlys text is no longer an exact current-state defect list. Its 44,965-line
baseline is stale, and the active SPA stylesheet already includes the 720 px chat
measure, 36 px message rhythm, dark chat token swap, fjord citation state, 44 px
action targets, and reduced-motion coverage. Remaining high-priority gaps are the
unloaded Inter-first stacks outside chat, fractional product type, the 56 px solid
SPA navbar, label removal at narrow widths, and consolidation of late chat
overrides into a maintainable source of truth. This comparison changed
documentation only.

### What to take from each reference

| Reference | Useful principle | Verevon decision |
| --- | --- | --- |
| [Hellomatik](https://hellomatik.com/) three-level comparison | One repeated business question makes the difference between a general model, a connected assistant, and an operational company model immediately understandable. | **Adopt now, with original Verevon language and architecture.** This is the clearest missing explanation and should be the first P1 content change. |
| [Wonderful AI OS](https://www.wonderful.ai/ai-os) unified-platform cards | A short platform thesis followed by a few large visual territories makes a broad product feel coherent. | **Adapt.** Use it to connect knowledge, reasoning, execution, and control as one Verevon flow rather than presenting disconnected capabilities. |
| [Wonderful](https://www.wonderful.ai/) enterprise statement | Large editorial copy can move trust from a small feature list into the main story. | **Adopt the communication pattern.** Write claims from Verevon's verified controls and deployment facts; do not inherit unverified compliance language. |
| [Warmwind](https://warmwind.com/) infrastructure map | Geography, isolation, and hosting become easier to understand when shown spatially, followed by concrete control cards. | **Adapt for `/trust` or an enterprise section.** Show an EEA deployment/residency diagram only after each region and isolation claim is verified. |
| [Nolla](https://www.nollahealth.com/) Knowledge card | A dedicated visual entry makes the knowledge system feel like a product surface rather than backend plumbing. | **Adapt as “Kunnskapsbasen” or “Levende kunnskapsgrunnlag.”** Show sources, relationships, owners, freshness, and citations instead of the internal name “Data Plane v2.” |
| [Nolla](https://www.nollahealth.com/) ambient media with UI | A familiar human setting plus a crisp product layer explains where the software enters the work. | **Use once.** Pair a short human-work clip with one legible Verevon interaction and an explicit demo label. Avoid full-page video repetition. |
| [Finseo](https://www.finseo.ai/) mega menu | Grouped destinations and a preview panel are effective when the information architecture is large enough. | **Defer.** Verevon currently exposes four primary destinations; a mega menu would create complexity before there is enough real navigation. Reconsider at six to eight substantive destinations. |
| [Finseo](https://www.finseo.ai/) scroll-aware navigation | The header gains a stable material as page content passes beneath it, improving continuity and legibility. | **Adapt now with CSS.** Transparency is not itself a WCAG feature; the accessible outcome comes from maintaining text, focus, and boundary contrast over every background. |
| [Wolverine Worldwide](https://wolverineworldwide.com/) image galaxy | Small human images interrupt an oversized statement and give an abstract promise real-world texture. Its public implementation uses 32 particles, five speed values, nine depth values, continuous directional drift, scroll-velocity impulse, vertical scale interpolation and edge recycling. | **Reference-matched particle model; Verevon text language retained.** `ProblemSection` uses that particle behavior with original Verevon copy and assets, while the heading and supporting paragraph keep Verevon's earlier two-level hierarchy. The heading retains its word-opacity scroll reveal. A near-viewport execution gate and static reduced-motion state remain in place. |
| Lassie workflow cards, supplied screenshot | Large abstract color fields give small, realistic UI states enough visual presence to carry a narrative. | **Adopt through the existing demo/Remotion workflow.** Build a small, reusable family of Verevon work scenes rather than isolated decorative mockups. |
| Lassie abstract backgrounds, supplied screenshot | Soft photographic or painted motion creates atmosphere while the interface stays crisp and readable. | **Use selectively.** Keep copy outside the active texture or on a solid inner surface and provide a static reduced-motion poster. |
| [Lightspark](https://www.lightspark.com/) whitespace and restraint, supplied screenshot | Space around proof, copy, and partner marks makes an infrastructure product feel calm and consequential. | **Adopt as a pacing rule.** Add one sparse enterprise statement between denser product sections; do not turn every section into another tall scroll scene. |
| [`acdlite/react-fiber-architecture`](https://github.com/acdlite/react-fiber-architecture) | Explains React's Fiber reconciliation and scheduling model. It is not a 3D component or animation library. | **Reject as a dependency.** React 19 already uses Fiber internally. The document can inform scheduling decisions, but it supplies no rendering effect or production component. |
| [`liquid-glass-js`](https://github.com/dashersw/liquid-glass-js) | WebGL refraction and blur can imitate optical glass by sampling the page with `html2canvas`. | **Reject for the production navbar.** Its imperative WebGL/canvas approach is excessive for a persistent header and adds lifecycle, performance, contrast, and fallback risk. Use native CSS materials. |
| [`shadergradient`](https://github.com/ruucm/shadergradient) | A configurable live gradient can create a polished ambient field in React. | **Conditional prototype only.** It requires Three.js and React Three Fiber, which are absent from this app. Prefer a prerendered loop or image when the result is purely decorative. |

These references are design evidence, not reusable content or asset sources. Verevon should reproduce the communication principles with original words, imagery, UI, and brand tokens.

### Recommended homepage story

The active composition currently runs Hero → connector proof → Problem → Product Loop → knowledge story → features → platform layer → closing statement in [VerevonHome.tsx:21](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/VerevonHome.tsx:21>). Keep that backbone, but sharpen the job of each section:

1. **Hero — the promise.** Keep the restrained headline and one primary action. It should state the business outcome in plain language before naming architecture.
2. **Proof — it connects to the real stack.** Keep the connector row as supporting evidence, with a static complete list for reduced motion.
3. **Problem — how Verevon joins knowledge to controlled action.** The implemented [ProblemSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProblemSection.tsx>) uses the approved statement and a field of human, AI and Verevon moments. [ProblemCardsSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProblemCardsSection.tsx>) follows as a distinct section with the three detailed problem arguments before the product loop.
4. **Product proof — what happens.** Turn the existing product-loop demos into four consistent work states: find, understand, act, and control. Each scene should show a believable task, visible sources or state, and a clear result.
5. **Knowledge — what compounds.** Give the current `kunnskap` story in [SensesSection.tsx:285](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/SensesSection.tsx:285>) a concrete entry card for the knowledgebase. Show source excerpts, relationships, freshness, ownership, and citation trails, not a generic network sphere on its own.
6. **Enterprise trust — where and under whose control.** Add a sparse statement and link into `/trust`; place the detailed residency/isolation diagram on the trust route where it can be supported by precise facts.
7. **Capabilities and close.** Keep features and the platform layer as substantiation, then close with the existing call to action.

This changes the page rhythm from several cinematic claims into a sequence visitors can retell: **Verevon understands the task, grounds it in company knowledge, performs approved work, and leaves the person in control.**

### P1 content module: three levels of AI

The structure should use one exact question across all three cards so the consequence of each level is visible. This is working copy for product and legal validation, not approved marketing text:

> **Det finnes tre nivåer av AI. Bare ett kan gjøre arbeidet ferdig.**
>
> Et godt svar er ikke det samme som en gjennomført oppgave.

| Level | Suggested label | Suggested explanation | Example outcome for “Hvilke kunder trenger oppfølging denne uken?” |
| --- | --- | --- | --- |
| 01 | **En språkmodell** | Formulerer godt, men kjenner ikke virksomhetens kilder, regler eller historikk. | Gir råd om hvordan man kan prioritere kunder. |
| 02 | **AI koblet til systemene** | Kan hente informasjon, men mangler fortsatt felles arbeidskontekst, prioriteringer og godkjente handlingsgrenser. | Returnerer poster fra CRM, e-post og økonomisystemet som noen må sammenstille. |
| 03 | **Verevon** | Samler kilder, kunnskap, regler og godkjente verktøy i én arbeidsflyt. Viser grunnlaget, foreslår neste steg og stopper når dere skal godkjenne. | Prioriterer kundene, viser hvorfor, lager utkast til oppfølging og ber om godkjenning før handling. |

The third card should be visually emphasized through border, surface, and proof detail, not a louder animated effect. Show source chips, an approval gate, and an audit event. Avoid promising deterministic answers, autonomous execution without approval, or certifications the current product cannot prove.

### P1 product-proof system

The current Product Loop already imports reusable product demonstrations and `FeatureComposerCycle` in [ProductLoopSection.tsx:8](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx:8>). Build on that work instead of adding a parallel 3D showcase. Create three or four reusable proof scenes with a shared shell:

| Scene | Human question | Visible Verevon state | Proof of control |
| --- | --- | --- | --- |
| **Finn** | “Hva har endret seg siden sist?” | Search across approved sources with excerpts and timestamps. | Source scope and citations are visible. |
| **Forstå** | “Hva betyr dette for saken?” | Context pack with relationships, constraints, and uncertainty. | The user can inspect or correct the basis. |
| **Få gjort** | “Forbered neste steg.” | Draft document, system update, or delegated task. | Exact proposed action and destination are shown before execution. |
| **Kontroll** | “Hva skjedde, og hvem godkjente?” | Timeline of proposal, approval, execution, and result. | Actor, policy, timestamp, and reversible state are visible. |

Visually, alternate the copy and media side as in the supplied Lassie examples. Use one large rounded field with restrained brand color or a slow prerendered texture, then place a crisp UI card inside it. Keep type outside the effect area. Label illustrative states as “Produktdemo” or “Simulering” when they do not show a live customer system.

For purely linear visuals, render short loops from the existing Remotion workflow and serve an efficient poster plus video rather than hydrating another graphics runtime. Start media only near the viewport, pause it offscreen, expose playback control where motion persists, and use the poster under reduced motion.

### P1 enterprise and trust narrative

Use the editorial scale of Wonderful and the spatial explanation from Warmwind, with facts taken from the Verevon trust model:

> **Bygget for virksomheter der data, ansvar og handling må henge sammen.**
>
> Fra utrulling og datalagring til tilgangsstyring og revisjon er hvert lag laget for kontrollert bruk av AI.

On the homepage, keep this to the statement, four verified proof points, and a link to `/trust`. On the trust route, the supporting module can show:

- verified data regions and residency boundaries;
- tenant or environment isolation, described at the level the product actually guarantees;
- identity, roles, and approval boundaries;
- audit history and source traceability;
- how Zero Data Retention propagates across content-persisting boundaries.

Do not draw a Norway, Nordic, or EEA infrastructure map until the deployed regions and failover implications are confirmed. Do not convert planned controls or certifications into present-tense claims.

### P2 knowledgebase presentation

The Nolla card is useful because it treats knowledge as a destination. Verevon should pair a human-work card with a knowledge card:

- **Verevon i arbeid:** a concrete question moving through sources, judgment, proposal, and approval.
- **Kunnskapsbasen:** the durable knowledge that makes the next task faster and more consistent.

The knowledge visual should expose recognizable product objects: documents, source snippets, entities, links, freshness, owners, and citations. An abstract network can sit behind those objects, but it should not be the explanation. Public copy should use customer language; “Data Plane v2” remains an internal architecture term.

### P2 navigation material and information architecture — implemented 9 September

[HomeClientShell.tsx:30](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/HomeClientShell.tsx:30>) owns scroll state, and [Navbar.tsx:29](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/navbar/Navbar.tsx:29>) renders the material without a WebGL dependency. The implemented behavior is:

- at the page top, use the existing transparent treatment when contrast is proven over the hero;
- when the hero-to-content fog begins at 60% of the viewport height, transition navigation color with the fog, then fade in an about:blank-inspired full-width 64px surface after the fog's 640ms interval has completed; the surface uses 80% white, `backdrop-filter: blur(12px)`, a one-pixel `rgb(217, 217, 217)` lower boundary and no shadow;
- use a solid token fallback under `@supports` failure and `prefers-reduced-transparency`;
- switch at the shared threshold without a transition under `prefers-reduced-motion`;
- use an opaque high-contrast surface and visible border under `prefers-contrast: more`;
- keep focus rings, 44px menu hit areas, current-page state, and text contrast independent of the glass effect;
- keep the 64px geometry and type sizes identical before and after the material transition;
- keep the scrolled state stable while reversing scroll direction.

Do not add a mega menu yet. When the site has enough real routes to group Platform, Solutions, Enterprise, and Resources, the Finseo pattern can be revisited with keyboard navigation, Escape and focus restoration, pointer-intent delay, click support, and mobile accordions. It must never rely on hover alone.

### Library and performance decision

The app already carries three overlapping motion packages—Framer Motion, GSAP, and Motion—in [package.json:17](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/package.json:17>). Adding a fourth graphics stack for a decorative background would work against the earlier client-boundary and runtime improvements.

| Candidate | Production decision | If explored |
| --- | --- | --- |
| React Fiber architecture repository | Do not install. It is explanatory documentation for React internals. | Use only as reading about prioritization and interruptible work. |
| `liquid-glass-js` | Do not use in the navbar. | An isolated lab may compare the optical look, but the production solution should remain CSS with accessible fallbacks. |
| `shadergradient` | Do not add to the main bundle. | One lazy prototype may load only when near view, with `ssr: false`, a single WebGL context, `pointer-events: none`, capped pixel density, offscreen pause/disposal, and static fallbacks. Prefer exporting the accepted result to video or AVIF/WebP. |

Any graphics prototype must meet these acceptance conditions before it is merged:

- zero graphics-library code in the initial route bundle;
- no new layout shift and no text or focus contrast that depends on the effect rendering;
- no live shader on reduced-motion, reduced-transparency, data-saving, or unsupported contexts;
- no persistent WebGL canvas on small/mobile viewports unless physical-device profiling proves it safe;
- media and canvas work starts near view and stops or disposes after leaving;
- a before/after production build comparison and a physical mid-range mobile trace show no material regression in LCP, INP, long tasks, scroll frame pacing, memory, or battery-sensitive continuous work.

This audit intentionally records gates rather than fabricated performance numbers. Use the existing local baseline only as historical diagnostic evidence; make the design decision from a new production build and device trace.

### Ranked delivery plan

| Priority | Status | Deliverable | Completion test |
| --- | --- | --- | --- |
| **P1** | Implemented and refined 10 September | Wolverine-inspired Problem story | The particle field follows the selected reference while the compact heading and supporting paragraph preserve Verevon's original typographic hierarchy. The heading keeps the word-opacity scroll animation; the approved sentence stays fully readable beneath it. The scene resolves to a static visible state for reduced-motion visitors and hands off directly to the product loop. This user-selected direction supersedes the proposed three-level comparison on the homepage. |
| **P1** | Proposed | Three or four unified “Verevon in action” product-proof scenes | Each scene shows input, evidence/state, output, and control; mocks are labeled and work without autoplay motion. |
| **P1** | Proposed; claim review required | Enterprise statement on home plus verified infrastructure/control module on `/trust` | Every residency, isolation, security, and compliance statement has an owner and evidence source. |
| **P2** | Proposed | Knowledgebase entry surface | Users can see what knowledge contains, where it came from, who owns it, and how fresh it is. |
| **P2** | Implemented and refined 10 September | Full-width CSS scroll material for the existing navbar | Local browser verification confirmed a transparent hero state and a full-width 64px scrolled surface. Its about:blank-derived computed material is 80% white with 12px blur, a one-pixel `rgb(217, 217, 217)` lower boundary and no shadow. Navigation color follows the hero-to-content fog; the material waits until the fog's 640ms transition has settled, then fades in over its own 640ms interval. Native blur, solid reduced-transparency/high-contrast fallbacks, and unchanged logo/navigation-link sizes remain in place. |
| **P2** | Implemented and refined 10 September | Page-rhythm pass | The homepage uses a 100px desktop gutter based on the selected premium reference cohort and a 10% reduction in display/content scale. Body text, controls, touch targets and Product Loop frame geometry remain unchanged; gutters scale down for tablet and mobile. |
| **P3** | Deferred | Mega-menu IA study | Start only after real routes support meaningful groups and mobile/keyboard designs exist. |
| **P3** | Deferred experiment | Single shader or abstract-loop experiment | Continue only if it clarifies a product scene and passes the bundle, accessibility, motion, and device gates above. |

The remaining recommended implementation order is: product-proof scenes → enterprise/trust narrative → knowledgebase surface → complete the page-rhythm review. The P3 studies should not delay the P1 content work.

Apple Design assessment: the proposed direction improves hierarchy, deference, and user agency by letting product evidence carry the visual weight and making approvals visible. Frontend design assessment: **friction is orange** because differentiation takes too long to understand; **craft is green/orange** because the system is distinctive but several dense motion sections compete; **trust is orange** because the architecture is credible but enterprise proof appears too late. The target after the P1 work is green across all three without increasing animation density.

## Coverage of all 14 requested skills

| Skill | Application and result |
| --- | --- |
| apple-design | Applied agency, feedback, wayfinding, typography and accessibility criteria. The audited menu focus, footer contrast and misleading actions were corrected in the 8 September remediation. Custom brand fonts are a documented choice, not a defect. |
| emil-design-eng | Applied component and motion craft checks. Improve reduced-motion completeness and remove invisible background work. Before/After table below. |
| frontend-design-review | Built-in browser review completed. The initial modal focus, short-screen menu overflow, response hero color, and misleading navigation findings are resolved; remaining review scope is deployed and physical-device performance validation. |
| cloudflare:web-perf | User-authorized alternate MCP/CDP workflow measured local paint/layout-shift events, requests, transfers and headers. Large video downloads confirmed. Full DevTools insight traces, Lighthouse, field INP and production CDN behavior remain unmeasured. |
| animate | Used the frequency/purpose/tool/property gates to evaluate possible changes. Implementation mode was not run because the requested deliverable is findings. |
| animation-vocabulary | Used to name observed patterns: parallax, marquee, scroll-driven animation, page transition and press feedback. This is a naming reference, not a separate pass/fail scanner. |
| review-animations | Initial verdict was **Block motion sign-off**. The 8 September remediation resolved autoplay, reduced-motion marquee and hidden SVG work. The Product Loop's visual hierarchy is corrected, while its direct scroll-layout geometry remains an open P2 measurement and optimization item. |
| improve-animations | Recon, parallel read-only review and vetting completed. Remediation addressed the identified motion control and avoidable-work gaps; remaining work is post-deployment measurement. |
| find-animation-opportunities | One optional press-feedback improvement survives the gate; two additions explicitly rejected below. This site already has ample motion. |
| anthropic-skills:ai-seo | Content/discoverability readiness reviewed. Entity metadata and crawler policy were added in remediation; dated case studies, benchmarks and public supporting evidence remain content opportunities. Actual AI citation share and cross-platform rankings were not measured. |
| sanity:seo-aeo-best-practices | General SEO/AEO guidance applies; no Sanity integration exists to audit. Canonical/social/discovery metadata and the showcase indexing policy were added in remediation. |
| build-web-apps:react-best-practices | The broad homepage client boundary was split into server composition and focused client islands in remediation. No application data-fetch waterfall was found in the inspected source. |
| vercel:next-cache-components | Applicability assessed. Cache Components are not enabled, but this package has static marketing content and no demonstrated cacheable server data workload. **No defect solely because use cache is absent.** |
| vercel:nextjs | App Router and server metadata placement are sound; canonical/social metadata and client/server composition were corrected in remediation. Production build and TypeScript passed with static routes generated. No invalid async Client Component was identified. |

## Prioritized findings

P1 = address before launch; P2 = important correction/optimization; P3 = secondary improvement. Browser reproductions and measurements are explicitly distinguished from source-only findings.

The following detailed entries preserve the pre-remediation baseline. Use the remediation table above for each item's current status.

### F01 — P1: full-screen menu does not manage keyboard focus
**Evidence:** [src/components/ui/MenuModal.tsx:19](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/ui/MenuModal.tsx:19>) declares a modal dialog but has no initial focus, focus containment, focus restoration, or inert background. [src/components/home/VerevonHome.tsx:213](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/VerevonHome.tsx:213>) adds scroll locking and Escape handling only.

Opening it leaves focus on the trigger behind the overlay; Tab can escape into page content. Use a modal primitive with these behaviors, retaining the existing appearance and Escape support. Modal behavior requires more than `aria-modal=true`. [W3C dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).

**Browser reproduction:** open “Åpne meny” with Enter at 1440×900. Focus remains on the trigger outside the dialog. Tab from the final YouTube link moves to the underlying hero's “Se arbeidsflyten” link. Escape closes the menu without restoring focus to its trigger.

### F02 — P1: important footer text has insufficient contrast
**Evidence:** [src/components/core/footer/Footer.tsx:81](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/footer/Footer.tsx:81>) and [src/components/core/footer/Footer.tsx:97](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/footer/Footer.tsx:97>) use `text-verevon-j-text/35`; the tokens are foreground `#171717` and background `#f6f6f4` in [src/app/globals.css:86](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/globals.css:86>).

Alpha-compositing these declared colors gives approximately **2.20:1** contrast. The 48% description is approximately **3.15:1**, and the 50% bottom row approximately **3.35:1**. These are normal-size text, below the **4.5:1** WCAG AA threshold. Use an accessible semantic muted-text token; the existing 65% combination calculates to about **5.39:1** on this background. These are calculated declared-color values, not screenshot measurements. [W3C contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html).

### F03 — P2: footer links target sections absent from the active homepage
**Evidence:** [src/components/core/footer/Footer.tsx:12](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/footer/Footer.tsx:12>) links to `/#produksjon`; line 17 links to `/#trust`. Neither ID exists in the sections actually composed by [src/components/home/VerevonHome.tsx:298](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/VerevonHome.tsx:298>). Matching IDs in unused extras or preview variants do not supply targets on `/`.

Point these links to the current product/trust destinations or restore intentional anchor targets. Also, the footer's “Verevon hjem” logo uses `#top` at line 46, which stays on the current subpage instead of going home.

### F04 — P2: search, social and legal labels promise different destinations
**Evidence:** [src/components/core/navbar/Navbar.tsx:74](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/navbar/Navbar.tsx:74>) labels a link “Søk” but sends it to `#kontakt`. [src/components/core/footer/Footer.tsx:138](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/footer/Footer.tsx:138>) and [src/components/ui/MenuModal.tsx:102](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/ui/MenuModal.tsx:102>) send all social networks to the same contact anchor. [src/components/core/footer/Footer.tsx:151](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/core/footer/Footer.tsx:151>) labels `/trust` as “Personvern og vilkår,” although [src/components/trust/TrustCenter.tsx:318](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/TrustCenter.tsx:318>) composes a security/trust overview.

Connect actual destinations, rename the links to what they do, or omit unavailable actions. This is a navigation/content finding, not a legal compliance determination.

### F05 — P2: reduced motion does not stop several autoplay videos
**Evidence:** [src/components/home/sections/HeroSection.tsx:29](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/HeroSection.tsx:29>) unconditionally loops a full-viewport video. [src/components/platform/ResponseTimePage.tsx:62](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/platform/ResponseTimePage.tsx:62>) explicitly calls `play()`; its hero cycles clips and the lower video at line 177 loops. These components lack preference-based playback branches or pause controls.

CSS animation-duration overrides do not stop HTML video. Supply static posters for reduced motion, react to preference changes, and provide pause control for ongoing ambient playback. The existing FeatureCardFilms preference/visibility handling is a useful project-local reference.

**Browser reproduction:** with reduced motion emulated before loading `/` at 390×844, the media query returned true while the hero video remained playing (`paused:false`, playback time 4.70 seconds). See [raw reduced-motion evidence](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/runtime-evidence/reduced-motion.json>).

### F06 — P2: a below-fold decorative video has a 74.89 MB source
**Evidence:** [src/components/platform/ResponseTimePage.tsx:177](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/platform/ResponseTimePage.tsx:177>) autoplays [public/verevon-vibe/problem-waiting/corridor.mp4](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/public/verevon-vibe/problem-waiting/corridor.mp4>), **74,890,332 bytes** (71.42 MiB), with no viewport gate. The root hero source is **13,627,554 bytes**.

**Measured:** during approximately 13 seconds on the local production response-time page, requests transferred **33.70 MB**, including **21.63 MB of corridor.mp4** while that element was below the viewport and paused. The root page transferred **14.61 MB desktop / 14.45 MB mobile**, including **13.63 MB** for its hero video. These are observed transfers, not an assertion that the entire 74.89 MB corridor source downloads immediately. Create an appropriately encoded short web rendition and gate source attachment/loading near visibility; retain a poster. Decode cost and savings after a change remain unmeasured. Full conditions and raw records appear below.

### F07 — P2: decorative SVG work runs while the layer is invisible
**Evidence:** [src/components/home/sections/SignalPathLayer.tsx:359](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/SignalPathLayer.tsx:359>) computes points along SVG paths and writes attributes on every update. Lines 376 and 384 create infinite tweens; the hero starts them immediately. Line 444 hides the layer with CSS below 760px, without preventing JS setup. The problem variant starts on entry but does not pause on leave.

Gate initialization by the visible breakpoint and pause/restart with section visibility. This removes proven unnecessary work; FPS, INP and battery savings are unmeasured.

### F08 — P2: reduced-motion marquee permanently clips connector names
**Evidence:** [src/components/home/sections/BrandLogosSection.tsx:37](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/BrandLogosSection.tsx:37>) stops translation at zero, but line 100 retains a `w-max` row, lines 114–115 duplicate children, and [src/components/home/sections/BrandLogosSection.tsx:187](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/BrandLogosSection.tsx:187>) retains overflow clipping, a fade mask and 112px gaps.

Later names can never enter the visible area once movement stops. Render one wrapped static list without the edge mask for reduced motion. Applies to the root and both previews.

### F09 — P2: scroll-driven product animation changes layout properties
**Evidence:** [src/components/home/sections/ProductLoopSection.tsx:410](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx:410>) animates `height/left/top/width`; [src/components/home/sections/ProductLoopSection.tsx:621](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx:621>) applies these to the product frame, with ring geometry changes nearby.

This requires layout/paint work during scroll. Replace positional movement with transforms and use scale for the decorative ring where visual equivalence permits; profile the product-frame resize before redesigning it. This sequence is already gated to desktop with no reduced-motion preference, so this finding does **not** claim mobile or reduced-motion exposure.

### F10 — P2 optimization opportunity: the whole homepage is a client import graph
**Evidence:** [src/components/home/VerevonHome.tsx:1](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/VerevonHome.tsx:1>) marks the composition component client-side and directly imports every section plus Footer. Static editorial/footer components consequently enter that client graph; several animation sections are needed only further down the page.

Move static composition to a Server Component and isolate navigation/scroll controllers and demos as client islands; assess deferred loading for heavy below-fold interaction code. Preserve server-rendered readable content. A Client Component can still be server-rendered initially: this is **not** a claim that the current page is blank to crawlers. Bundle byte savings are unmeasured. [Next.js client/server composition](https://nextjs.org/docs/app/getting-started/server-and-client-components).

### F11 — P2: canonical URLs and social preview images are missing
**Evidence:** [src/app/layout.tsx:17](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/layout.tsx:17>) supplies title, description and Open Graph text but no `metadataBase`, canonical URL, `og:url`, or share image. No route-specific canonical or file-based Open Graph/Twitter image was found in the app tree.

Set the verified production origin, per-route canonicals and share images. Existing page descriptions and Norwegian locale are a good foundation. Missing canonicals are a hardening opportunity, not proof of a duplicate-content penalty.

### F12 — P2 readiness gap: no sitemap, explicit crawler policy or structured entity data
**Evidence:** the `src/app` and `public` inventories contain no sitemap/robots implementation, and source search found no JSON-LD/schema.org markup.

Publish a sitemap for the intended public routes; add an explicit crawler policy reflecting the business's search and training choices; add accurate Organization and appropriate product/software entity data matching visible claims. Missing robots.txt does **not** imply crawlers are blocked. Missing structured data does **not** make a page ineligible for AI answers.

Do not blindly follow the AI SEO skill's suggestion that allowing GPTBot is required for citation. OpenAI's current documentation distinguishes **OAI-SearchBot for search** from **GPTBot for training**; they can be configured independently. [OpenAI crawler documentation](https://developers.openai.com/api/docs/bots). Google requires no special AI files or schema for its AI search features. [Google AI search guidance](https://developers.google.com/search/docs/appearance/ai-features).

### F13 — P2: recording showcase lacks the previews' noindex policy
**Evidence:** [src/app/showcase-short/page.tsx:4](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/showcase-short/page.tsx:4>) describes a vertical showcase “built for recording” but has no `robots` restriction. `/v2` and `/v3` explicitly set `index:false`.

If it is a production-accessible recording utility, add noindex and exclude it from public discovery. The absence is confirmed in code; deployment exposure and actual indexing were not verified.

### F14 — P2: full-screen menu overflows short mobile viewports
**Browser evidence:** at **320×568**, the dialog's content height was 592px with `overflow-y:visible`, while the body was scroll-locked. The YouTube link occupied y=568–592, entirely below the viewport; “Kontakt” overlapped the “Følg oss” region. The menu fitted the taller 390×844 viewport, so a width-only check misses this failure.

**Source:** [MenuModal.tsx:28](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/ui/MenuModal.tsx:28>) fixes the overlay to the viewport with three grid rows and large vertical spacing; the navigation and social rows are at lines 59 and 93. Make the modal content scrollable and let its rows flow without overlap on short screens. Recheck keyboard reachability and zoom after correcting focus behavior in F01.

### F15 — P1: response-time hero text renders dark over dark video
**Browser evidence:** at 390×844 on `/produkt/svartid`, the headline was nearly unreadable. Its markup contains `text-white`, but computed color was **rgb(23, 23, 23)**. The supporting paragraph computed to **rgb(102, 97, 91)** despite `text-white/82`; the eyebrow also remained dark.

**Cause:** [ResponseTimePage.tsx:101](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/platform/ResponseTimePage.tsx:101>) combines semantic typography classes with white Tailwind utilities. The unlayered `.verevon-display`, `.verevon-eyebrow` and `.verevon-body-lg` rules in [globals.css:952](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/globals.css:952>) explicitly set dark colors and override the layered utilities. Separate typography from color, use an explicit inverse variant, or place semantic styles in the appropriate cascade layer. Verify contrast against every video/poster state; no single numerical contrast ratio is asserted for the changing background.

## Secondary findings and editorial opportunities

- **P3: live reduced-motion changes do not stop all existing GSAP motion.** [src/components/home/sections/HeroParallax.tsx:31](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/HeroParallax.tsx:31>) checks the preference once; line 54 retains 80%/40% parallax afterward. Use the reactive media setup already present in SensesSection.
- **P3: menu links retain reduced-motion delay.** [src/components/ui/MenuModal.tsx:75](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/ui/MenuModal.tsx:75>) starts links invisible with 0/80/160/240ms stagger; [src/app/globals.css:928](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/globals.css:928>) reduces duration but does not reset delay.
- **P3, preview only: CPU particles ignore reduced motion.** [src/components/ui/cpu-architecture.tsx:102](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/ui/cpu-architecture.tsx:102>) and seven following SVG animateMotion elements repeat indefinitely on `/v2`. CSS duration overrides do not control SVG SMIL.
- **AI SEO evidence gap:** the public routes contain useful product descriptions and candid trust disclosures, but no dedicated published case-study/benchmark/research route or public pricing/plan explanation was found. Add dated, attributable evidence and a concise early-access/plan explanation when facts are available. Do not invent performance statistics, prices, certifications or customer endorsements. This is a content opportunity, not measured low AI visibility.

## Motion review: Before / After

| Before | After | Why |
| --- | --- | --- |
| Autoplay continues under reduced motion | Static poster plus explicit playback choice | Restore user control over large moving backgrounds |
| Marquee freezes inside clipped container | Single wrapped list with all connector names | Preserve information when animation stops |
| SVG tweens run offscreen/on hidden mobile layer | Start only when visible; pause on exit | Eliminate unnecessary per-frame work |
| Product frame/ring repeatedly change layout geometry | Transform positional movement; scale decorative ring; profile remaining resize | Reduce avoidable layout and paint cost |
| GSAP reads reduced-motion preference once | Reactive media lifecycle with cleanup and static end state | Respect preference changes without reload |
| Menu links remain delayed after CSS duration override | No transform or stagger delay for reduced motion | Make navigation immediately readable |

**Initial motion verdict: Block sign-off.** The accessibility portion of this verdict is superseded by the resolved items in the 8 September table. Product Loop geometry remains open at P2 after restoring visual correctness on 9 September. Cinematic scroll sequences and the documented 320ms/640ms timing system are not automatically failures on a marketing site. Production frame pacing remains unmeasured, so no dropped-frame claim is made.

## Animation opportunities, with the gate applied

| Location | Today | Purpose | Frequency | Suggested motion |
| --- | --- | --- | --- | --- |
| [src/components/home/sections/FeatureCardFilms.tsx:504](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/FeatureCardFilms.tsx:504>) | Play/pause has icon, label and hover-color feedback but no press state | Feedback | Occasional marketing-demo interaction | Optional `scale(0.97)` on press, `transform 160ms cubic-bezier(0.23,1,0.32,1)`; use a shared press token rather than another timing system. Reduced motion: keep the existing icon/color feedback and omit scale. No hover motion required. |

Rejected additions:
- [src/components/home/sections/FeaturesSection.tsx:552](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/FeaturesSection.tsx:552>): cursor arrow/hand changes are frequent interaction feedback; leave them immediate.
- [src/components/home/sections/ProductLoopSection.tsx:229](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx:229>): the static mobile/reduced-motion sequence is intentionally stable; new entrance choreography fails the accessibility/function gate.

The original interface needed more reliable motion controls before additional animation. Those controls were corrected in the 8 September remediation; the optional press state remains the only new-motion suggestion retained.

## What is already working

- Brand-specific Arbeit/Protokoll typography, fluid type, centralized warm-neutral/terracotta tokens and a documented design direction.
- Norwegian `lang="nb"`, meaningful server metadata, and a single public homepage instead of preview variants.
- Trust content visibly separates live controls from planned certifications and describes ZDR as an optional paid addition.
- FeatureCardFilms combines visibility, reduced-motion handling, posters and a playback control.
- SensesSection uses media-query-aware animation setup.
- Route transitions have a reduced-motion bypass; GSAP contexts generally clean up.
- No cache invalidation issue or server data waterfall was established. Adding Cache Components merely to satisfy a checklist is not warranted.
- The initial branded loader was removed after its server-rendered state persisted during a local Turbopack HMR crash.

## Production-preview runtime measurements

Measured 8 September 2026 against a local `next start` production build, with browser cache disabled, a warmed local server, no CPU/network throttling and one approximately 13-second sample per case. Mobile means a **390×844 emulated viewport on the same desktop machine**, not a physical phone. Desktop was 1440×900. MB below uses decimal bytes.

| Route / viewport | FCP | Last observed LCP | CLS session-window maximum | Observed network transfer |
| --- | ---: | ---: | ---: | ---: |
| `/` desktop | 360 ms | 360 ms | 0.00685 | 14.61 MB |
| `/` mobile | 224 ms | 224 ms | 0 | 14.45 MB |
| `/produkt/svartid` desktop | 184 ms | 184 ms | 0.00220 | 33.70 MB |

These are local diagnostic samples, **not field Core Web Vitals or a Lighthouse score**. LCP was observed before interaction; the root candidate was hero text, and the response-page candidate was the hero video's poster. The initial loader in this baseline was subsequently removed. CLS excludes shifts flagged with recent input and uses the largest session window rather than summing unrelated shifts. INP, Speed Index, standard Lighthouse TBT, scroll frame rates and video decode cost were not measured.

The root hero accounted for about 93–94% of observed root transfer. On the response page, the paused corridor video was approximately 1964px below the viewport top and transferred 21.63 MB during the observation. Several hero sequence clips also downloaded. Transfer totals include completed requests and received bytes for ongoing requests; they are bounded observations, not eventual page weight.

Local static JavaScript responses had gzip compression and `public, max-age=31536000, immutable`. Hero media used range responses with ETag and `public, max-age=0`. These are local Next.js responses; deployed CDN behavior is unverified. Version media URLs before assigning long-lived immutable caching.

Raw records: [desktop homepage](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/runtime-evidence/desktop-home.json>), [mobile homepage](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/runtime-evidence/mobile-home.json>), [desktop response time](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/runtime-evidence/desktop-response-time.json>), [reduced motion](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/runtime-evidence/reduced-motion.json>).

## Browser checks and validation limits

| Check | Result |
| --- | --- |
| Source/import/metadata review | Completed |
| Local asset sizes and declared footer color contrast | Measured/calculated |
| Current desktop/mobile visual and keyboard review | Initial defects reproduced, then F01, F14 and F15 fixes verified in the browser |
| FCP / LCP / CLS | Local samples above; no field percentile claims |
| INP / Lighthouse TBT / Speed Index / Lighthouse score / frame pacing | Not measured |
| Requests, media transfer and compression | Measured on local production preview |
| Production CDN caching | Not verified |
| Production robots, canonicals, indexing and AI citations | Not verified |
| Lint | Passed, 0 errors / 11 no-img-element warnings in logo-cloud.tsx |
| Build / TypeScript | Passed production build including TypeScript and static generation |
| Automated test suite | 38 tests passed after remediation |
| Application source/config changes | V2/V3 cleanup, audit remediation, loader removal, Webpack development fallback, server homepage composition, discovery metadata, and the Product Loop stage-04 foreground and linear full-viewport entrance corrections implemented; no deployment or commit |

Before remediation, the built-in browser loaded `/`, `/produkt/arbeidsflyten`, `/produkt/svartid`, `/plattform/felles-kontekst`, `/plattform/kontrollert-arbeid`, `/trust`, `/v2`, `/v3` and `/showcase-short`. Targeted checks found no page-level horizontal overflow at 390px. ArrowRight correctly changed the workflow tab from Kontekst to Utkast and the shared-context tab from Samtalen to Kildene, with roving focus behavior. Inspected console error logs were empty. After remediation, `/v2` and `/v3` return 404. This is targeted coverage, not an exhaustive interaction or screen-reader test.

The initial runtime review confirmed missing root canonical and JSON-LD, absent `/#produksjon` and `/#trust` targets, noindex on `/v2` and `/v3`, and no robots restriction on `/showcase-short`. Remediation removed the absent anchor targets and the V2/V3 routes, replaced the misleading search action and clipped reduced-motion marquee, and added canonical, JSON-LD, robots/sitemap, social-image, and showcase-indexing controls.

Tooling: the originally missing native trace tools were worked around under the user's explicit authorization. The supplied [Chrome DevTools MCP repository](https://github.com/benjaminr/chrome-devtools-mcp) was cloned into the Codex temporary directory and run through its registered Python tools/CDP client in an isolated environment. It required `mcp<2` (resolved to 1.30.0) because its FastMCP import was incompatible with the initially installed MCP 2.x. No global MCP configuration was modified. Its measurements supplement the built-in browser review; the exact skill's trace-insight workflow was not reproduced. CDP screenshot capture timed out, so visual checks used the built-in browser instead.

The production build warned that multiple lockfiles affected inferred workspace-root selection. Build succeeded; explicitly configure the intended root if that warning is undesirable. This is a configuration cleanup, not a demonstrated runtime fault.

Recommended order: produce a web-sized corridor rendition, then profile the remaining product-loop copy-panel geometry and large homepage media on a throttled device. Recheck zoom, screen-reader behavior, reduced motion toggled live, and deployed crawler/CDN behavior after deployment.
