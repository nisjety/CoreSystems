# Phase 1 onboarding — asset prompts

Source-of-truth prompts for every video / animation / illustration the
auth-embedded onboarding wizard expects on the right-side pane. Each
slot ships with a placeholder (the mock image already in
`public/imagens/`) so the flow is testable end-to-end before any real
asset is generated. Replace each placeholder by dropping the rendered
asset at the noted path and updating the `src` in the matching step
component under `components/auth/onboarding/steps/`.

Format conventions:
- Aspect ratio: **portrait 9:16 inside a 0.85fr column** that's ~432×640 on
  desktop (the existing auth right-pane). All videos loop seamlessly.
- Duration: 3–5 s for personalization animations; ≤8 s for the dashboard
  assembly finale.
- Audio: muted by default.
- Background: must blend with `#EDEBE7` card. Keep edges soft / fade-out
  rather than hard cuts.

---

## Slot 1 — Post-sign-in product reveal (3–5 s loop)

**Path**: `public/videos/onboarding/product-reveal.webm`
**Placeholder today**: `public/imagens/auth-right-current.png` (whatever
the existing auth right-pane illustration is).

**Prompt**:
> Cinematic 4-second loop showing the Velion dashboard coming alive.
> Camera glides across a clean cream surface; cards slide into place —
> a chat thread reveals an AI agent reply, a knowledge graph blooms
> behind it, a small token-cost meter ticks up, a CSAT chart fills.
> Palette: Intercom cream (#F4EFE5) base, charcoal #111 text, single
> coral accent (#FF2E63) on the live indicator. Editorial serif on
> headlines (Cormorant Garamond). 60fps. No voiceover. Ends on a still
> frame showing the agent reply card centred so the transition into the
> "smart pitch" overlay is clean.

---

## Slot 2 — Organization personalization (3 s loop)

**Path**: `public/videos/onboarding/org-personalization.webm`
**Placeholder**: a still image of a Norwegian / EU map fragment with a
single pulsing dot. Use `public/imagens/org-personalization-mock.png`.

**Prompt**:
> 3-second loop. A stylised globe (continents in line-art, no oceans),
> the user's org pin drops in, three concentric rings ripple outward.
> Below the globe a one-line caption scrolls right-to-left:
> "Fetching public business registry data · employees · domain · NACE
> code". Palette matches Slot 1. Subtle film grain (4 %) to bridge with
> the auth-page noise. Ends on the globe still + the caption holding at
> the centre.

---

## Slot 3 — Website snippet drop (live, JS-driven, not video)

**Path**: rendered live by `<WebsiteSnippetDrop />`; no asset needed
beyond the existing public folder card.

**Prompt** (for the designer mocking it):
> White folder card centred on the cream pane (hybrid of the Taskello
> note-card and the "4 Files / 500–700MB" folder seen in the Mobbin
> Craft + ElevenLabs onboarding refs). Top edge has a small tab.
> Snippets — small white rounded squares (96×72 px) containing either
> 2–3 lines of crawled text or a thumbnail extracted from the page —
> fall from above the frame in a slight curve, settle into the folder
> with a soft springy bounce. Progress bar at the bottom of the folder
> fills as snippets land. A counter beside it (`12 / 40` snippets,
> `1.4 MB`). When the crawl completes the folder tab flips up showing
> a checkmark.

This step is driven by real data from Quarry. Snippets render from
`POST /api/onboarding/crawl-preview` (returns the first ~40 chunks
quarry-edge has indexed so far for the supplied URL).

---

## Slot 4 — Knowledge GraphRAG reveal (live, SVG-driven, not video)

**Path**: rendered live by `<KnowledgeGraphReveal />`.

**Prompt** (for designer):
> Dark canvas (#0F0F10) centred on the right pane (rounded 16 px,
> matches the rest of the cream UI by sitting on top of it). 60-node
> force-directed graph. Nodes start as small grey circles (#5B5B5C);
> as the user adds a connector or a doc, a fresh node fades in,
> highlights green (#34D399) for 1 s, then settles to grey. Edges draw
> with a 200 ms stroke-dashoffset animation. A faint glow follows the
> most-recently-added node. Bottom-right shows live counters:
> `nodes 42 · edges 71 · org documents 18`. Reference: imagen #5 in the
> Mobbin packet (Obsidian-style graph with green active nodes).

Powered by Data Plane v2 `graph-index` via
`GET /api/onboarding/graph-preview`.

---

## Slot 5 — Social proof logo wall (static SVG sprite)

**Path**: `public/imagens/onboarding/logos.svg`
**Placeholder**: a list of inline SVG logos (Apple, Microsoft mock,
Slack, Notion, Zammad, Sanity) in a 3×2 grid.

**Prompt**:
> Six greyscale (#444) logos in a 3×2 grid, evenly spaced, each in its
> own 80×40 cell. No frames, no colour. Hover: each logo lifts +1 px and
> tints to #111. The asset is a single SVG sprite with `<symbol>`
> definitions so each tile is `<svg><use xlink:href="#logo-apple"/></svg>`.

---

## Slot 6 — Plan card sparkle (CSS-only, no asset)

The 4 plan cards + 1 trial card use the existing Intercom-style border
treatment. The LLM-recommended card gets a thin gradient outline
(coral → indigo → emerald) animated at 8 s loop. No video asset.

---

## Slot 7 — Dashboard assembly finale (4–8 s loop)

**Path**: `public/videos/onboarding/dashboard-assembly.webm`
**Placeholder**: a static screenshot of the dashboard at
`public/imagens/onboarding/dashboard-assembly-mock.png`.

**Prompt**:
> 6-second sequence. Camera pulls back from a single chat reply card
> (the same shot we ended on in Slot 1). One by one, the dashboard
> chrome assembles around it: sidebar slides in from the left, top
> navbar drops in from above, knowledge graph card fades in on the
> right, usage stats card slides up from the bottom. Final frame is
> the empty velion dashboard with the user's org name appearing in the
> top-left workspace switcher. Soft cream background, subtle film
> grain. Caption fades in over the final frame: "Workspace klar."
> (Norwegian for "Workspace ready.")

---

## Tone notes (apply to all video assets)

- Motion is **slow and confident**, never frantic. 1.2× ease-in-out, no
  hard cuts.
- The single accent colour across the whole flow is coral `#FF2E63`.
  Don't introduce a second.
- Type: any on-screen captions use Cormorant Garamond (serif) for
  headlines and Inter (sans) for sub-copy.
- No people, no faces, no hands. All abstract / product UI in motion.
- Audio: mute by default. If we ever unmute later, ambient room tone
  ≤ −30 dBFS, no music score, no voiceover.
