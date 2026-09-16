# Verevon Web ↔ Verevon SPA design alignment

**Date:** 9 September 2026  
**Status:** Verified comparison and implementation direction  
**Scope:** Documentation only. No application source was changed by this pass.

## Decision

Verevon Web and the Verevon SPA already share most of the same raw colors, but
they do not yet read as one product family. The main difference is not the base
palette. It is the combination of typography, navigation material, density,
surface treatment, motion tokens, and the way the SPA layers newer chat rules on
top of older global CSS.

The alignment target is one shared brand foundation with two deliberate modes:

- **Verevon Web** is the editorial and explanatory mode: Arbeit display type,
  Protokoll narrative copy, large whitespace, cinematic media, and large type.
- **Verevon SPA** is the working mode: the same brand shell and first impression,
  with Geist retained for dense controls, transcripts, tables, code, and repeated
  daily work.

The apps should look related at first glance without forcing marketing-scale type,
large media radii, or cinematic motion into a high-frequency product interface.

## What was compared

- The current Web design contract in [`DESIGN.md`](../DESIGN.md).
- Web tokens and navigation material in
  [`src/app/globals.css`](../src/app/globals.css#L37) and
  [`Navbar.tsx`](../src/components/core/navbar/Navbar.tsx#L37).
- The SPA foundation and product shell in
  [`src/styles/global.css`](../../../src/styles/global.css#L3).
- The current chat-scoped Fjordlys pass in
  [`src/styles/global.css`](../../../src/styles/global.css#L45226).
- The supplied “Fjordlys v1 — verification pass” text.

The SPA stylesheet currently has **47,016 lines** and is approximately **1.03 MB**.
The supplied audit's 44,965-line baseline and several of its line references are
therefore stale. Whole-file counts such as 684 `.dark` occurrences, 1,077 radius
declarations, and 357 box-shadow declarations describe accumulated application
CSS, not the visible chat screen alone. Web's 1,117-line stylesheet is also not a
direct complexity comparison because much of its styling lives in Tailwind
classes inside TSX.

## Current comparison

| Area | Verevon Web | Verevon SPA today | Alignment result |
| --- | --- | --- | --- |
| Canvas and ink | `#f8f8f7`, `#1a1a1a`, `#171717`, white surfaces | The same root values and most of the same `--verevon-*` names | **Already aligned**; promote these to a shared foundation. |
| Brand accent | Coral `#ee7a50`, used sparingly for signal and action | Coral is present, while earth, warning, blue, green, and red still appear across legacy UI | **Partly aligned**; use one state contract instead of per-component color choices. |
| Typography | Arbeit for display and Protokoll for narrative/body; Geist fallback | Geist is loaded for chat, but root and dashboard body stacks still lead with unloaded Inter. Chat also uses 10.5/11.5 px sizes and 430/650 weights. | **Primary visual mismatch.** Share the brand fonts and assign explicit product roles. |
| Navigation | Full-width 64 px bar; transparent at top and warm liquid glass after scroll | Fixed 56 px solid dashboard bar with 14 px blur, a bottom rule, compact boxed controls, and a desktop `zoom: 0.9` shell | **Primary shell mismatch.** Reuse the Web material and brand treatment, then adapt workspace content. |
| Surfaces | Large editorial fields, low-contrast edges, broad whitespace, strong hierarchy | Chat's newest rules use borderless cards and soft lift, while older app surfaces still use many borders, small radii, and nested panels | **Partly aligned.** Fjordlys is a late override rather than the common source of truth. |
| Chat rhythm | Product demos use clear foreground UI on restrained fields | Current chat override sets a 720 px thread, 36 px gaps, 720 px composer, and 44 px action hit areas | **Good foundation.** Keep these values. |
| Motion | 320 ms micro and 640 ms reveal with a global reduced-motion safety net | Chat mixes 140, 300, 360, 600, and 1,200 ms; the five key chat motions and smooth scrolling now have a reduced-motion guard | **Behavior is safer; vocabulary still differs.** Share names and easing, retain faster product micro feedback. |
| Dark mode | Public Web is light-first | SPA has a chat-scoped dark token swap plus hundreds of older `.dark` patches | **Valid product-only extension.** Dark should inherit shared semantic roles rather than copy Web literally. |

## Verification of the supplied Fjordlys claims

The supplied document is useful as direction, but it should not be treated as a
current defect list. Several proposed fixes are already present in the working
SPA stylesheet.

| Supplied claim | Current state |
| --- | --- |
| Chat thread should use a 720 px measure and 36 px gaps | Implemented in the late chat pass at `global.css:45391–45395`. |
| Chat dark mode leaves the transcript white | Superseded by the chat-scoped dark palette at `global.css:45261–45301`. |
| Citation chips are cramped and mix accents | Superseded: current chip is 24 px minimum height with 4×10 px padding and uses fjord blue only for evidence identity at `global.css:25214–25240`. |
| Tool and approval cards rely on hard interior rules | Largely superseded: current tool and approval rules use tint changes, shadow lift, 12/16 px radii, and 14–20 px padding at `global.css:25530–25667`. |
| Chat action targets are 28×28 px | Superseded: the current action rule provides a 32 px visual inside a 44 px hit area at `global.css:25707–25734`. |
| The five chat keyframes and smooth scrolling ignore reduced motion | Superseded by the guard at `global.css:24461–24478`. A final global safety net also exists near the end of the file. |
| Geist is loaded while Inter remains the declared body family | **Still partly true.** Chat leads with Geist, but the root declaration and dashboard body still lead with unloaded Inter. |
| Fractional sizes and noncanonical weights create inconsistent type | **Still true.** The newest chat pass itself contains 10.5/11.5 px and 430/650 values. |
| Mobile hides labels instead of relaxing layout | **Still true in chat toolbar branches.** Some 44 px controls set `font-size: 0` or hide labels below 1,180/720 px. |
| The SPA is difficult to govern through tokens | **Still true.** The current Fjordlys work is appended after older definitions, so the cascade rather than the design contract determines the result. |

## Shared Verevon foundation

These values and meanings should be identical in both runtimes. The final source
can be generated CSS or a framework-neutral token package; React/Next and
Solid/Vite should consume the same output rather than maintain hand-copied maps.

### Color roles

| Role | Value | Contract |
| --- | --- | --- |
| Canvas | `#f8f8f7` | Default light background in Web and SPA. |
| Surface | `#ffffff` | Raised object, card, composer, or popover. |
| Surface soft | `#f4f4f3` | Hover, grouped control, or inset region. |
| Ink | `#1a1a1a` | Primary readable text. |
| Strong ink | `#171717` | Primary controls and deep brand fields. |
| Muted text | `#66615b` | One warm neutral ramp; avoid mixing it with cool gray in the same component. |
| Whisper line | `#e7e7e5` | Edge clarification only; whitespace or tint carries separation. |
| Glød | `#ee7a50` | Brand mark and the person's consequential action. |
| Glød ink | `#b8501f` | Accessible terracotta text on light surfaces. |
| Fjord | `#0071e3` | Machine activity, evidence, links, and focus. It is transient or functional. |
| Success / danger | Semantic tokens | Outcomes only; never decorative hover colors. |

Web does not need to show fjord blue on every page. It should still declare the
same semantic token so product illustrations, links, focus, and real machine-state
examples do not invent another blue.

### Typography roles

Use the same font assets and fallback order in both apps:

1. **Arbeit** — brand wordmark, major route title, cold-start heading, onboarding
   milestone, and other low-frequency brand moments.
2. **Protokoll** — explanatory copy, navigation labels, onboarding guidance, and
   product empty-state ledes where the SPA should sound like Verevon Web.
3. **Geist** — chat transcript, composer, dense controls, data tables, settings,
   source metadata, and repeated operational work.
4. **Geist Mono** — code, IDs, logs, and machine output only.

This gives the SPA the same face as Web at entry points while keeping dense work
legible. Remove phantom Inter references unless Inter is intentionally shipped.
Use whole-pixel product sizes with an 11 px floor and a 400/500/600 weight set.

### Navigation and shell

The shared shell target is the existing Web navbar material:

- full content-viewport width;
- 64 px geometry at the actual rendered scale;
- transparent over an approved hero or start surface;
- warm-white glass after scroll or when content needs separation;
- 28 px backdrop blur, restrained saturation, subtle reflection layers, and a
  soft shadow;
- solid reduced-transparency and high-contrast fallbacks;
- Arbeit wordmark, Protokoll route labels, and 44 px hit areas;
- identical type size before and after the material transition.

The SPA can replace Web's marketing links with workspace breadcrumbs, search,
notifications, and account actions. Its material, wordmark, height, typography,
and interaction quality should still be recognizably the same. The SPA's desktop
`zoom: 0.9` means raw CSS dimensions do not currently equal rendered dimensions;
resolve or explicitly compensate for that before claiming pixel parity.

### Surfaces and state

- Assistant answers remain typography on the canvas.
- User messages, composers, tool runs, approvals, and inspectable evidence are
  objects and may earn a surface.
- Selected state is monochrome fill plus lift.
- Glød marks user commitment: send, approve, or a brand identity moment.
- Fjord marks machine activity, evidence, links, and focus.
- Borders do not carry layout hierarchy. Use whitespace, tint, and one of the
  shared warm shadow recipes.
- Product controls use 8/12/16/24 px radii plus pill; 30 px editorial media radii
  stay in Web and should not migrate into dense SPA panels.

### Motion

Use one shared ease-out curve and named tiers across both runtimes:

- **140 ms product micro:** hover, press, selected state, action reveal;
- **320 ms surface:** navbar material, popover, panel, sheet;
- **640 ms editorial reveal:** Web sections and rare SPA onboarding moments;
- **ambient:** only live machine-state indicators, with a static reduced-motion
  state.

Marketing can remain more cinematic. Chat, tabs, keyboard actions, and frequent
workspace transitions should stay immediate or short.

## What should remain intentionally different

- Web's hero scale, long vertical rhythm, image galaxies, and cinematic sections
  are storytelling tools. They are not SPA component defaults.
- SPA tables, source traces, logs, and configuration screens need denser Geist
  typography and more visible state than a marketing section.
- SPA dark mode is a valid product capability. It should extend the shared
  semantic palette rather than force a dark marketing site.
- Large 28–30 px media corners in Web are for editorial imagery. Product cards
  use the smaller shared control/card scale.

## Prioritized delivery plan

| Priority | Deliverable | Completion evidence |
| --- | --- | --- |
| **P0** | Create one framework-neutral brand-token output consumed by Web and SPA | Canvas, ink, coral, fjord, neutral ramp, shadows, radii, focus, and motion names resolve to the same computed values in both apps. |
| **P0** | Share the Arbeit/Protokoll assets and assign the typography roles above | Web navbar and SPA entry/cold-start views use the same wordmark, headline, and narrative families; dense SPA content remains Geist. No unresolved Inter custom property remains. |
| **P0** | Align the SPA global shell with Web's full-width navbar material | Side-by-side captures show the same 64 px rendered height, material, wordmark, label scale, and accessibility fallbacks at desktop and mobile widths. |
| **P1** | Consolidate the SPA chat style into one source section | Duplicate thread/bubble/header/composer definitions and stale overrides are removed; current computed output remains visually stable. |
| **P1** | Finish Fjordlys typography and mobile density | No product text below 11 px, no fractional font sizes, only 400/500/600 weights, and mobile labels remain understandable without `font-size: 0`. |
| **P1** | Apply the shared accent and surface contract to the SPA shell | Coral, fjord, success, and danger each have one meaning; navigation and content no longer depend on stacked hairlines. |
| **P2** | Convert remaining SPA dark patches to semantic token swaps by feature | New components require no ad-hoc `.dark` selector when their tokens are correct. |
| **P2** | Tokenize Web's repeated arbitrary radii and shadows | The marketing site remains the visual reference without exporting its TSX-level one-off values as a false design system. |

## Acceptance matrix

Verify `/` in Verevon Web beside the SPA dashboard cold start and chat thread at
1440×900, 390×844, and 320×568.

- The canvas, wordmark, brand type, nav material, and accent meaning match at
  first glance.
- The SPA still supports dense work without borrowing Web's hero scale.
- Navbar geometry and font sizes remain stable before and after scroll.
- Focus, reduced motion, reduced transparency, and increased contrast work
  without relying on blur, color alone, or animation.
- All interactive targets are at least 44×44 CSS pixels at the rendered scale.
- No normal product text is smaller than 11 px.
- Chat, source, approval, and tool states remain legible in light and dark modes.
- A screenshot review checks relationship and hierarchy; computed-style checks
  verify the shared token values rather than relying on visual similarity alone.

## Implementation boundary for this pass

This comparison changes documentation only. The existing SPA Fjordlys block,
Verevon Web navbar, fonts, tokens, and application behavior remain untouched.
