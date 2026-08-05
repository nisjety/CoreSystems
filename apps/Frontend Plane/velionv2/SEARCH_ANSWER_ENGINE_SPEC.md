# Verevon Search — Answer-Engine Spec (north star)

Reference: Dribbble shot (25s, laptop mock). Target = a **Perplexity / Arc-Search-style answer engine** as the core Verevon search surface. This is the direction for the dashboard "Søk" experience.

## Observed flow (from the reference video)

1. **Prompt / idle** — centered headline **"What do you want to know?"** over a single minimal pill input (`+` attach on the left). Airy, near-white "liquid glass" surface with a soft **animated wavy gradient**. Minimal chrome: brand mark top-left, avatar top-right, a slim left icon rail, small corner glyphs.
2. **Typeahead** — typing (`Find me…`) drops an **autocomplete list** of query completions ("Find me Grocery store", "Gym near me", "restaurants").
3. **Inline preview** — on a richer query ("Find me SPA near Berlin for 2 adults"), **entity/result cards** animate in directly under the input (hotel/brand cards) before the full view.
4. **Answer view** — transitions to a full results page:
   - Query echoed top-left; back/brand glyph.
   - **Category tabs:** Info · Videos · Map · Images · Shopping, plus **Filter**, **Sources**, regenerate.
   - **AI answer** per entity: title (e.g. "SO/ Berlin Das Stue Hotel") + generated summary text, an inline **image thumbnail row**, a small **Map** card, a **Sources** panel (right), and follow-up suggestion chips.
   - Multiple entities stacked (SO/ Berlin, ONO Spa at The Mandala…).
   - **Persistent "Ask follow up…" composer** pinned bottom-center (conversational continuation).
   - Background shifts white → soft pink/purple gradient; frosted cards, large radii, generous whitespace.

## Aesthetic
Liquid-glass / frosted panels, animated wavy gradient backdrop (white → pastel), very rounded corners, light hairline borders, lots of air, restrained typography with an accent color on key words. Subtle motion: cards animate in, gradient drifts, smooth tab transitions.

## Mapping to our backends (what exists vs needs work)

| Reference feature | Backend | Status |
|---|---|---|
| Typeahead suggestions | autocomplete-core `/v1/suggestions` (Sonic) | ✅ wired + NATS-feed fix landed |
| **Info** tab (web results) | Quarry-edge `POST /v1/search` | ✅ via `/api/v1/search/web` (needs a SERP provider key) |
| AI answer + **Sources** | Quarry `include_answer` / `/v1/answer` → Model Plane (Claude Sonnet) | ✅ available |
| URL fetch (paste a link) | Quarry `/v1/scrape` (Firecrawl-style) | ✅ URL-aware route landed |
| **Images** tab | Quarry image results / scrape `og:image`s | ⚠️ needs an images path |
| **Map** tab (places) | a places/maps provider (none wired) | ❌ needs provider |
| **Videos / Shopping** tabs | dedicated providers | ❌ needs providers |
| **Follow-up** composer (conversational) | Model Plane `/v1/invoke/stream` | ✅ `/api/chat/stream` built — reuse for follow-ups |
| Inline entity/preview cards | derived from search results + scrape metadata | ⚠️ UI build |
| KB ("Kunnskap") results | `/api/v1/navbar/search` (knowledge scope) | ✅ wired |

## Build phases (proposed)

- **P1 — Answer view + Info tab.** New full-width results layout: query header, category tab bar (Info active; others present but gated), AI-summary block + Sources panel + web results list. Reuse `/api/v1/search/web`. Submitting from the dashboard search routes here. *(Highest value; mostly frontend over existing backend.)*
- **P2 — Typeahead + inline preview.** Polish the suggestions dropdown (autocomplete-core) and animate inline entity cards under the input before navigation.
- **P3 — Images + Map tabs.** Images from Quarry/scrape og-images; Map from a places provider (decide provider). Videos/Shopping deferred until providers chosen.
- **P4 — Conversational follow-up.** Wire the pinned "Ask follow up…" composer to `/api/chat/stream` so the answer view becomes a thread (answer → follow-up → answer), with source-grounding.
- **P5 — Aesthetic pass.** Liquid-glass panels, animated wavy gradient backdrop, motion/transitions, rounded frosted cards — match the reference.

## Decisions needed
- **SERP provider** for keyword web search: Serper / Brave / SearXNG / Tavily (set `QUARRY_EDGE__SEARCH_PROVIDER` + key) — required for the Info tab to return live results.
- **Map/places** + **Videos/Shopping** providers — or drop those tabs for v1.
- Scope of v1: likely **Info + Images + follow-up** (all backed today) and hide Map/Videos/Shopping until providers exist.
