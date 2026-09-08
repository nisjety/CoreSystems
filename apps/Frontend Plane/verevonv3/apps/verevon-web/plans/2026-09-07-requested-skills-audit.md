# Verevon web: requested skills audit
Date: 7 September 2026; runtime verification and remediation updated 8 September 2026

**Current verdict: the launch-blocking accessibility and readability findings are resolved.** The V2 and V3 variants and fragile full-screen page loader have been removed; the menu, footer, response-time hero, reduced-motion behavior, deferred corridor loading, connector marquee, hidden SVG work, homepage client boundary, and public discovery metadata have been corrected. Remaining work is post-change performance measurement, re-encoding the large corridor source, and a visual redesign of the product-loop copy-panel geometry. The baseline audit below retains its original evidence for traceability.

This began as an audit. The 8 September remediation section records the application changes made from its findings.

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
| F06 corridor transfer | Mitigated | The below-fold corridor video has no source until it is within 200px of the viewport; it remains a poster for reduced-motion visitors. The 71.42 MiB source still needs a web rendition and fresh production transfer measurement; no local video encoder was available. |
| F08 reduced-motion marquee | Resolved | Connector names render once in a wrapped static list without a mask or clipping. |
| F14 short mobile menu | Resolved | The menu is a scrollable flex layout, so every control remains reachable at 320×568. |
| F15 response-time hero | Resolved | An inverse typography variant prevents semantic type styles from overriding the light hero copy. |
| F07 hidden SVG work | Resolved | Signal routes initialize only within 300px of view on desktop visitors without reduced-motion preference; they pause when leaving view. Hidden/mobile SVGs do no animation setup. |
| F09 product-loop geometry | Mitigated | The decorative ring and main frame now use composited translate, scale, and rotate changes. The independent copy panel retains layout geometry so its line wrapping stays readable; redesign it before replacing that behavior. |
| F10 homepage client boundary | Resolved | Homepage composition is again a Server Component. A small client shell owns the menu, scroll state, and footer parallax while section islands load only their own browser code. |
| F11 canonical and social metadata | Resolved | Added the `https://verevon.ai` metadata base, route canonicals, Open Graph URLs, Twitter metadata, and a generated 1200×630 social image. |
| F12 sitemap, crawler policy, entity data | Resolved | Added a sitemap, explicit crawler rules that permit search and block training crawlers, plus Organization, WebSite, and SoftwareApplication JSON-LD using visible product claims. |
| F13 showcase indexing | Resolved | The recording showcase now emits `noindex, nofollow` and remains outside the sitemap. |
| P3 motion preference updates | Resolved | Hero parallax now rebuilds or clears through `gsap.matchMedia` when reduced-motion changes live. Menu links override their initial opacity, translate, animation, and stagger for reduced-motion visitors. |
| V2/V3 cleanup | Resolved | Removed both routes, their component trees, preloader CSS, V3-only assets, and stale documentation references. `/v2` and `/v3` return 404. |
| Runtime loader | Resolved | Removed the initial full-screen loader, whose server-rendered state remained visible when development HMR failed. The local `dev` command now uses Webpack because the active Turbopack session panicked with “Next.js package not found.” |

Targeted browser verification at 320×568 confirmed initial close-button focus, focus wrapping within the menu, focus restoration to “Åpne meny”, a scrollable menu surface, an inert background, the response-time hero's light computed colors, and corridor source assignment only near the viewport. The repaired local preview at `http://localhost:3000` has no loader element or console errors; it emits a canonical, generated Open Graph image and JSON-LD. `robots.txt`, `sitemap.xml`, the social-image route, the noindexed showcase and trust canonical all return 200. The V1 route returned 200; `/v2` and `/v3` returned 404.

Remaining priority order: create a web-sized corridor rendition, then profile the product-loop copy-panel resize and large homepage media on a throttled device. Do not treat the pre-fix local network figures below as post-change performance results.

## Coverage of all 14 requested skills

| Skill | Application and result |
| --- | --- |
| apple-design | Applied agency, feedback, wayfinding, typography and accessibility criteria. Menu focus, footer contrast and misleading actions need correction. Custom brand fonts are a documented choice, not a defect. |
| emil-design-eng | Applied component and motion craft checks. Improve reduced-motion completeness and remove invisible background work. Before/After table below. |
| frontend-design-review | Built-in browser review completed. The initial modal focus, short-screen menu overflow, response hero color, and misleading navigation findings are resolved; remaining review scope is the listed performance and metadata work. |
| cloudflare:web-perf | User-authorized alternate MCP/CDP workflow measured local paint/layout-shift events, requests, transfers and headers. Large video downloads confirmed. Full DevTools insight traces, Lighthouse, field INP and production CDN behavior remain unmeasured. |
| animate | Used the frequency/purpose/tool/property gates to evaluate possible changes. Implementation mode was not run because the requested deliverable is findings. |
| animation-vocabulary | Used to name observed patterns: parallax, marquee, scroll-driven animation, page transition and press feedback. This is a naming reference, not a separate pass/fail scanner. |
| review-animations | Initial verdict was **Block motion sign-off**. The audited autoplay and reduced-motion marquee gaps are resolved; avoidable SVG and scroll-layout work remains. |
| improve-animations | Recon, parallel read-only review and vetting completed. Remediation addressed the highest-priority motion control gaps; remaining work is listed above. |
| find-animation-opportunities | One optional press-feedback improvement survives the gate; two additions explicitly rejected below. This site already has ample motion. |
| anthropic-skills:ai-seo | Content/discoverability readiness reviewed. Structured entity information and public supporting evidence can improve. Actual AI citation share and cross-platform rankings were not measured. |
| sanity:seo-aeo-best-practices | General SEO/AEO guidance applies; no Sanity integration exists to audit. Missing canonical/social/discovery metadata and showcase indexing policy identified. |
| build-web-apps:react-best-practices | Broad homepage client boundary and eager interaction graph are optimization opportunities. No application data-fetch waterfall was found in the inspected source. |
| vercel:next-cache-components | Applicability assessed. Cache Components are not enabled, but this package has static marketing content and no demonstrated cacheable server data workload. **No defect solely because use cache is absent.** |
| vercel:nextjs | App Router and server metadata placement are sound; production build and TypeScript passed, with static routes generated. Improve metadata completeness and client/server composition. No invalid async Client Component was identified. |

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

**Motion verdict: Block sign-off.** Accessibility and avoidable animation work require correction. Cinematic scroll sequences and the documented 320ms/640ms timing system are not automatically failures on a marketing site. No measured dropped-frame claim is made.

## Animation opportunities, with the gate applied

| Location | Today | Purpose | Frequency | Suggested motion |
| --- | --- | --- | --- | --- |
| [src/components/home/sections/FeatureCardFilms.tsx:504](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/FeatureCardFilms.tsx:504>) | Play/pause has icon, label and hover-color feedback but no press state | Feedback | Occasional marketing-demo interaction | Optional `scale(0.97)` on press, `transform 160ms cubic-bezier(0.23,1,0.32,1)`; use a shared press token rather than another timing system. Reduced motion: keep the existing icon/color feedback and omit scale. No hover motion required. |

Rejected additions:
- [src/components/home/sections/FeaturesSection.tsx:552](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/FeaturesSection.tsx:552>): cursor arrow/hand changes are frequent interaction feedback; leave them immediate.
- [src/components/home/sections/ProductLoopSection.tsx:229](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx:229>): the static mobile/reduced-motion sequence is intentionally stable; new entrance choreography fails the accessibility/function gate.

This interface needs more reliable motion controls before it needs additional animation. The optional press state is the only new-motion suggestion retained.

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
| Application source/config changes | V2/V3 cleanup, audit remediation, loader removal, Webpack development fallback, server homepage composition, and discovery metadata implemented; no deployment or commit |

Before remediation, the built-in browser loaded `/`, `/produkt/arbeidsflyten`, `/produkt/svartid`, `/plattform/felles-kontekst`, `/plattform/kontrollert-arbeid`, `/trust`, `/v2`, `/v3` and `/showcase-short`. Targeted checks found no page-level horizontal overflow at 390px. ArrowRight correctly changed the workflow tab from Kontekst to Utkast and the shared-context tab from Samtalen to Kildene, with roving focus behavior. Inspected console error logs were empty. After remediation, `/v2` and `/v3` return 404. This is targeted coverage, not an exhaustive interaction or screen-reader test.

The initial runtime review confirmed missing root canonical and JSON-LD, absent `/#produksjon` and `/#trust` targets, noindex on `/v2` and `/v3`, and no robots restriction on `/showcase-short`. Remediation removed the absent anchor targets and the V2/V3 routes, replaced the misleading search action, and replaced the clipped reduced-motion marquee. Canonical, JSON-LD, robots/sitemap, and showcase indexing remain open.

Tooling: the originally missing native trace tools were worked around under the user's explicit authorization. The supplied [Chrome DevTools MCP repository](https://github.com/benjaminr/chrome-devtools-mcp) was cloned into the Codex temporary directory and run through its registered Python tools/CDP client in an isolated environment. It required `mcp<2` (resolved to 1.30.0) because its FastMCP import was incompatible with the initially installed MCP 2.x. No global MCP configuration was modified. Its measurements supplement the built-in browser review; the exact skill's trace-insight workflow was not reproduced. CDP screenshot capture timed out, so visual checks used the built-in browser instead.

The production build warned that multiple lockfiles affected inferred workspace-root selection. Build succeeded; explicitly configure the intended root if that warning is undesirable. This is a configuration cleanup, not a demonstrated runtime fault.

Recommended order: produce a web-sized corridor rendition, then profile the remaining product-loop copy-panel geometry and large homepage media on a throttled device. Recheck zoom, screen-reader behavior, reduced motion toggled live, and deployed crawler/CDN behavior after deployment.
