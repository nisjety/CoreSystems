# Verevon marketing site — design system

The public marketing site (`verevon-web`) is a standalone Next.js + Tailwind v4
app. Its job: a Linear/Apple/wonderful.ai-grade premium surface that is
unmistakably **Verevon** — warm (terracotta), sovereign, and Norwegian-first.

The cross-runtime relationship with the Solid/Vite product is defined in
[`plans/2026-09-09-spa-web-design-alignment.md`](plans/2026-09-09-spa-web-design-alignment.md).
That contract makes Web the editorial expression of one shared Verevon foundation
and the SPA its working expression. Palette, brand typography, navigation
material, accent meaning, focus, and motion names should match; density and
cinematic scale may differ by context.

Positioning source of truth: `/VEREVON.md` at the repo root. Copy is
**Norwegian-first (Bokmål)** with English support. The **honesty gate** is
load-bearing: never present a capability VEREVON.md marks roadmap/partial as if
live. Use `StatusBadge` for every maturity claim.

All tokens live in `src/app/globals.css` (`:root`). This file documents them.

---

## Type scale (fluid)

One source of truth. Defined as `--text-*` so Tailwind v4 also generates
`text-display`, `text-h2`, … utilities, and as semantic classes for new code.

| Token / class | Size (clamp) | Line height | Use |
|---|---|---|---|
| `--text-display` / `.verevon-display` | 3.35→8.2rem | 0.9 | Hero H1 only |
| `--text-h1` | 3→6.2rem | 0.94 | Section opener headline |
| `--text-h2` / `.verevon-h2` | 2.5→4.7rem | 0.98 | Standard section heading |
| `--text-h3` / `.verevon-h3` | 1.55→2.2rem | 1.04 | Card / sub-block title |
| `--text-body-lg` / `.verevon-body-lg` | 1.08→1.32rem | 1.5 | Section lede |
| `--text-body` / `.verevon-body` | 1→1.15rem | 1.55 | Paragraph |
| `--text-body-sm` | 0.92→1.02rem | 1.45 | Captions, card text |
| `--text-eyebrow` / `.verevon-eyebrow` | 0.72→0.82rem | — | Uppercase kicker |
| `--text-label` | 0.68→0.76rem | — | Micro labels, monospace-ish tags |

Tracking presets: `--tracking-display` (-0.072em), `--tracking-heading`
(-0.055em), `--tracking-eyebrow` (0.28em), `--tracking-label` (0.14em).

**Typefaces:** display/headlines = **Arbeit** (light 300 / book 400); body, eyebrows,
labels = **Protokoll** (light 300 / medium 500). Geist is the fallback stack.

---

## Color & accent discipline

Base palette (unchanged): warm off-white `--background:#f8f8f7`, near-black
`--foreground:#1a1a1a`, white cards, neutrals via `--verevon-*`.

**Terracotta is a scalpel, not a brush.** `--verevon-coral:#ee7a50` gets *one
job per view*:

- the single active-state underline / tick,
- one CTA accent (ArrowButton `coral`, send-state),
- signal dots and the focus ring (`--ring`),
- the sovereign **trust tint** on governance surfaces.

Trust/governance tokens:
`--verevon-trust-tint` (#fff1e7), `--verevon-trust-line`, and the three honesty
states — `--verevon-status-{live,progress,planned}` (+ `-soft` backgrounds)
consumed by `StatusBadge`.

---

## Elevation

`--verevon-shadow-sm` (cards) · `--verevon-shadow-md` (panels) ·
`--verevon-shadow-lg` (floating product frames) · `--verevon-shadow-hero`.
Prefer these over ad-hoc `shadow-[...]`.

---

## Spacing & rhythm

Page-level rhythm vars are set on the home root and reused by sections:
`--verevon-edge` (gutter), `--verevon-page-pad`, `--verevon-section-gap`,
`--verevon-section-vpad`. New sections should pad with these rather than fresh
`clamp()`s.

The homepage uses a **1512px maximum content width**, matching the supplied
feature-card reference at 90% browser zoom. The desktop gutter is at least
100px on wide screens and grows to center that content width. Mobile sections use
`--verevon-page-pad`, which remains between 24px and 48px. Navbar, logo band,
homepage sections and footer align to the same edge variable; a full-bleed
background may cross that edge, but its text and controls should not.

The September 11 density pass targets **80% of original content measures** where
the copy fits, independently of heading typography. After visual feedback, all
main homepage headings match the earlier prefooter exactly: Arbeit Light 300,
`clamp(1.45rem, 3.6vw, 4.95rem)`, 0.88 leading and -0.08em tracking.
Mobile uses a readable 28px minimum and looser leading. Card titles retain their
smaller hierarchy. Navigation, form controls and pointer/touch targets retain
their usable sizes.
Do not reproduce this pass with a root `font-size` or a transformed page wrapper:
those approaches reduce readable text, distort fixed geometry and interfere with
the Product Loop's measured scroll positions.

---

## Motion language — three tiers

1. **Micro** (hover/focus) — `--verevon-dur-micro` (320ms), `--verevon-ease-out`.
2. **Reveal** (scroll-in) — `--verevon-dur-reveal` (640ms); `.verevon-reveal`
   + `data-revealed`, or scrubbed GSAP for section chrome.
3. **Cinematic** — the hero load timeline + parallax (GSAP, `VerevonHome.tsx`).

`prefers-reduced-motion` is honored globally: a safety net in `globals.css`
collapses all CSS animation/transition durations, and every GSAP timeline has a
reduced-motion branch that sets end-state instantly.

Motion preferences that affect server-rendered attributes must use
`usePrefersReducedMotion`. Its identical server and initial-client value prevents
hydration mismatches; the browser media query is applied after mount and remains
reactive when the preference changes.

### Product Loop layer rule

The light loop circle is a background motif. Product frames, headings, controls
and supporting copy must remain visually above it. Composer stage 04 deliberately
has no large rectangular image or gradient background: its heading, composer and
supporting copy sit directly over the page and loop circle until the stage 05
layer crossfade begins.

The current verified implementation uses direct responsive frame dimensions from
the remote-default source. A future compositing optimization must separate the
decorative frame transform from the content surface; scaling the shared wrapper
clips and distorts the Composer UI.

The desktop entrance follows the measured TriodeLab reference behavior: the
centered product frame grows linearly from its initial marker (about 43–44vw) to
the full viewport over the 78vh approach to the sticky sequence. This entrance
changes frame geometry only; its content and later stage transitions remain
unchanged. Mobile and reduced-motion visitors keep the existing static fallback.

---

## Primitives (`src/components/ui`)

- `StatusBadge` — `level="live|progress|planned"` → Live / Under arbeid /
  Planlagt. **Required** for any trust/maturity claim.
- `SectionHeading` + `Eyebrow` — token-backed heading block (`eyebrow`,
  `title`, `lede`, `align`, `marker`).
- `ArrowButton` — primary text-link CTA (`dark|light|muted|coral`).
- `Button` (shadcn) — pill/outline actions.

---

## Trust surfaces

- Homepage **Trust section** (teaser) → links to `/trust`.
- **`/trust`** — dedicated public Trust Center: defensible *live* differentiators
  first (ZDR-by-default, EU-resident Sweden Central, per-action approval +
  "Used by AI?" audit, data classification, erasure, 0-SaaS search,
  Norwegian-native), an honest certifications **roadmap** table (SOC 2 Type II,
  ISO 27001, **ISO 42001**, EU AI Act, pentest — all *Planlagt/Under arbeid*),
  subprocessors + data-flow with the **CLOUD Act disclosure**, and a
  request-security-access affordance.

Never render an unearned certification as held. Never claim "we avoid US clouds"
— Verevon runs on Azure; lead on **EU-resident-by-default + ISO 42001 / EU AI Act
readiness** as the sovereign wedge.

---

## Proposed reference direction — 9 September 2026

This section records the approved audit direction and the implementation rules for
the reference-led work already selected. Items not explicitly marked as implemented
remain proposals. Detailed evidence, working copy, priorities and acceptance gates live in
[`plans/2026-09-07-requested-skills-audit.md`](plans/2026-09-07-requested-skills-audit.md#reference-led-design-audit--9-september-2026).

### Narrative hierarchy

The homepage should explain the product in a sequence a first-time visitor can
repeat:

1. **Promise:** the business outcome in plain Norwegian.
2. **Connection proof:** Verevon works with the systems the organization already
   uses.
3. **Problem:** one oversized statement names the fragmented-work problem while
   human work moments and real Verevon surfaces show where that problem appears.
4. **Product proof:** realistic states show input, evidence, proposed output and
   the human control point.
5. **Knowledge:** the accumulated source graph becomes a visible product surface
   with owners, freshness, relationships and citations.
6. **Enterprise trust:** residency, isolation, identity, approval and audit claims
   appear only with verified evidence and maturity status.
7. **Capabilities and close:** supporting breadth follows the explanation rather
   than competing with it.

The design goal is institutional calm with operational evidence. Use large type,
generous whitespace and a small number of legible product moments. Do not add a
second image galaxy or stack several cinematic sections without a sparse editorial
beat between them.

### Problem image galaxy

The homepage uses one Wolverine-inspired image galaxy in `ProblemSection`. It is
an editorial problem statement, not a product carousel. The thin Arbeit heading
matches the pre-footer statement's `clamp(1.5rem, 4vw, 5.5rem)` scale, 0.88 line
height and -0.08em tracking. It states “Kunnskapen finnes. Men den er spredt.” and a restrained body line
answers it with “Verevon samler kildene, forstår sammenhengen og gjør neste steg
klart — med dere i kontroll.” Small images show people reading, deciding,
coordinating, working with AI and reviewing Verevon output.

- use original Verevon words and owned project assets;
- mix human work imagery with a small number of authentic product surfaces;
- let selected images cross the type plane, while the statement remains readable;
- keep ample empty space so the composition feels calm rather than collage-like;
- keep the complete scene at three-quarters of the viewport height (`75svh`) with
  a 630px safety floor for short screens; render 32 square particles at 35px on small
  screens and 73px on desktop inside a 150%-height, 800px-perspective field;
- cycle five relative movement speeds and nine `translateZ` depths; run ambient
  drift and scroll response at 45% of the original rate (a 55% reduction), add
  scroll velocity at a `0.0225` multiplier, and recycle particles as they leave
  either edge;
- layer the restored moving signal routes behind the particle field and type;
- interpolate scale from `0.5` to `1.4` by vertical position and veil the distant
  negative-depth particles with the page color;
- preserve Verevon's word-by-word scroll reveal: begin each word at 55%
  opacity, reveal through a 0.62 progress window with cubic ease-out, and lift the
  complete heading by 24px between `top 46%` and `top 12%` with a `0.16` scrub;
- show the complete statement, a static particle field and the static signal
  routes for reduced-motion visitors; gate the
  continuous ticker until the section is near the viewport;
- keep every image decorative in the accessibility tree because the statement and
  supporting copy carry the meaning;
- end with one action into the product explanation; keep the restored problem
  tabs and media cards in the following `ProblemCardsSection` so the galaxy
  remains a single editorial statement.

### Problem cards

`ProblemCardsSection` restores the three original problem arguments as a distinct
homepage section after the image galaxy. It preserves the approved Norwegian copy,
destinations, posters and ordered video sequences. On desktop, choosing or hovering
a label expands its card while the other two remain visual previews. On touch and
keyboard, explicit buttons switch the active card. Videos load only when requested,
pause and reset when the pointer leaves, and remain still when reduced motion is
enabled. The section is borderless and uses a 20% tighter vertical composition than
the original restoration. It must not duplicate the galaxy heading or particle field.

### Product-proof scenes

Use a shared scene shell for **Finn**, **Forstå**, **Få gjort** and **Kontroll**.
Each scene must show:

- the user's question or task;
- the sources, context or state Verevon is using;
- the concrete proposed result;
- the approval, policy or audit evidence that keeps the person in control.

Place crisp UI on a restrained abstract field, keep explanatory copy outside the
active texture, and label illustrative states as `Produktdemo` or `Simulering`.
Prefer existing Remotion output with a poster and efficient video formats for
linear scenes. Start media near view, pause it offscreen and show the poster for
reduced motion.

### Knowledge and enterprise surfaces

Use customer language such as **Kunnskapsbasen** or **Levende
kunnskapsgrunnlag**; keep `Data Plane v2` internal. The visual explanation should
show recognizable objects—documents, excerpts, entities, relationships, owners,
freshness and citations—before any abstract network motif.

Enterprise sections may borrow the scale and spatial clarity of the reviewed
references, but every location, residency, isolation, security and compliance
statement remains behind the honesty gate. A map is useful only when deployed
regions and failover implications have been verified.

### Navigation material

The homepage header uses an about:blank-inspired full-width scroll material implemented
with native CSS. It remains transparent over the hero, then becomes a 64px
translucent white bar after the hero-to-content fog begins at 60% of the
viewport height. Navigation color follows the 640ms fog transition to preserve
contrast; the glass material waits for that transition to finish, then enters over
its own 640ms interval. The material spans the complete content viewport; it is
flat and square rather than an inset pill. Its 64px geometry and type sizes remain
unchanged between the transparent and glass states.

- the active material uses 80% white, 12px backdrop blur and the reference's
  one-pixel `rgb(217, 217, 217)` lower boundary, without tint, reflection or shadow;
- navigation copy inherits a dark scrolled-state tone with explicit opacity
  levels, while the transparent hero state remains white;
- solid fallback when backdrop filters or transparency preferences require it;
- switch at the shared threshold without interpolation for reduced-motion users;
- opaque high-contrast variant for `prefers-contrast: more`;
- focus rings, 44px hit areas and text contrast remain valid without the effect;
- the scrolled state is based on the shared absolute fade threshold and does not
  disappear when the visitor reverses scroll direction.

The current information architecture does not justify a mega menu. Revisit one
when six to eight substantive destinations can form meaningful groups and the
keyboard, touch and mobile accordion behavior is designed at the same time.

### Graphics and dependency gate

- `acdlite/react-fiber-architecture` is explanatory React documentation, not a
  visual or 3D dependency.
- `liquid-glass-js` should not power a persistent production navbar; CSS provides
  the required material with a smaller lifecycle and accessibility surface.
- `shadergradient` remains an isolated prototype option. Do not add Three.js or
  React Three Fiber to the initial route for a decorative background.

Any shader or continuous abstract loop must stay out of the initial route bundle,
stop offscreen, provide static fallbacks, preserve text/focus contrast without the
effect, and pass a production-build comparison plus physical mid-range device
profiling before adoption.

### Delivery order

1. Three-level AI explanation.
2. Unified product-proof scenes.
3. Enterprise statement and verified trust module.
4. Knowledgebase entry surface.
5. CSS navbar material and page-rhythm pass.
6. Deferred mega-menu study and one gated visual experiment, only if still useful.
