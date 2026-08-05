# Verevon marketing site — design system

The public marketing site (`verevon-web`) is a standalone Next.js + Tailwind v4
app. Its job: a Linear/Apple/wonderful.ai-grade premium surface that is
unmistakably **Verevon** — warm (terracotta), sovereign, and Norwegian-first.

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

---

## Motion language — three tiers

1. **Micro** (hover/focus) — `--verevon-dur-micro` (320ms), `--verevon-ease-out`.
2. **Reveal** (scroll-in) — `--verevon-dur-reveal` (640ms); `.verevon-reveal`
   + `data-revealed`, or scrubbed GSAP for section chrome.
3. **Cinematic** — the hero load timeline + parallax (GSAP, `VerevonHome.tsx`).

`prefers-reduced-motion` is honored globally: a safety net in `globals.css`
collapses all CSS animation/transition durations, and every GSAP timeline has a
reduced-motion branch that sets end-state instantly.

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
