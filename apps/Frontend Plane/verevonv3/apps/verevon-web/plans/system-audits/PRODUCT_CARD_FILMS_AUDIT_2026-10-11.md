# Product card films: audit, 2026-10-11

Audit only. Nothing was built. Scope: replace the visuals of product-loop stages 02 Plan and 03 Undersøk with 8-second looping product films, make stage 04 Følg opp look like the real dashboard, and answer whether the films can show the real dashboard solving real cases.

## What exists today

| Piece | Location | State |
|---|---|---|
| Marketing site (verevon.com) | `apps/verevon-web` (Next 16, `pnpm dev -p 3100`) | Live, product loop at `#flyt` |
| Product loop | `src/components/home/sections/ProductLoopSection.tsx` | 5 pinned stages: 01 image, 02/03/04 mock, 05 composer |
| Stage 02/03/04 visual | `src/components/home/product-demo/product-demo.tsx` (987 lines) + `demo-data.ts` | Hand-written React mock ("Velmora Demo", Orion scenarios). Not the real SPA. This is why 04 "does not look like the original". |
| Video layer support | `ProductLoopProductDemos.tsx` → `ProductLoopVideo` | Already supports `media.kind === "video"` with visibility-aware autoplay/pause. Wiring a film into a stage is trivial. |
| Remotion | `remotion/` (Remotion 4.0.399, `@remotion/player` installed) | 4 feature films already rendered to `public/feature-films` (960×1200, 30 fps, 12 s) by `scripts/render-feature-films.mjs`. Site tokens and Manrope/Geist fonts wired in `remotion/theme.ts`. |
| Real-recording pipeline | `scripts/capture-product.mjs`, `tests/e2e/product-recording.spec.ts`, `plans/product-recordings/manifest.json` | Playwright 1440×900 WebM of the real SPA on live Terra runs. One private customer take exists (121 s raw, 45 s short). 03/04 failed on validation timeout. No public media approved; `recordings` branch of the loop never renders. Policy: no speed-up, 25–60 s edits, 5 consecutive passes. |
| Real dashboard | SPA at `:5173` (Solid + Vite, parent folder) | Entra sign-in required. Chat has a split artifact view (`.verevon-chat-artifact-view`, "Resultat" tab, resizable divider) which matches the brief's "right-side panel". No fixture/demo mode exists (no msw, no `VITE_DEMO`). |
| Tooling on this machine | ffmpeg 9 + ffprobe, Playwright chromium 1243 | Present |

## What aside.com actually does

The three use-case cards are `<canvas>` gradient animations (408×320) in a `rounded-lg bg-neutral-100` tile with a caption below. There is no `<video>` element on the page. The hero "browser" is live DOM, not a film. So "like Aside" means: quiet soft-motion tile, neutral frame, caption outside the tile, no chrome, no device mockup.

## Can the films show the real dashboard solving real cases?

Three routes. Verdict per route.

### A. Real screen recording of a live run (existing pipeline)

Not the vehicle for an 8 s loop. A real turn is 40–105 s p95 on Terra. The site's own recording policy forbids speed-up and requires 25–60 s edits with disclosure strips. It also needs the owner's sign-in and the whole Docker stack healthy. Keep it for the long-form "Se hele oppgaven" player, not for the cards.

### B. Remotion re-creation of the dashboard (current feature-film pattern)

Full control: 1280×800, 60 fps, exact 250–450 ms ease-out, scripted cursor, seamless reset beat, mp4 + webm + poster from one render script. Cost: it is a drawing of the product, and drawings drift. Stage 04's mock is the proof: it drifted from the SPA and the owner noticed. Every SPA redesign reopens the films.

### C. Real SPA components, fixture-driven, frame-stepped capture (recommended)

Add a dev-only demo route in the SPA (for example `/demo/orion-plan`) that mounts the real chat shell and artifact panel with a fixture transcript and a scripted timeline. Capture it with a CDP virtual-time clock (`Emulation.setVirtualTimePolicy` + `Page.captureScreenshot` per 16.67 ms) so every frame is deterministic at 60 fps, then encode with ffmpeg. Real pixels, real tokens, real components, zero backend, zero sign-in, re-capturable after every UI change by running one script.

Why C over B: it answers the owner's actual complaint (mock drift) permanently. Why C over A: timing, format and loop are scripted, so the 8 s brief is met exactly. The existing `ChatAppearancePreview.tsx` shows the SPA can already mount chat UI from static props.

Trade-offs of C to accept: the route must be excluded from production builds; the cursor is not captured by screenshots, so it is an overlay element driven by the same timeline; the hover state is triggered by the timeline, not by a real pointer.

## Brief versus site: conflicts to resolve before filming

1. **Language.** Brief is English. Site, demo data and recording policy are nb-NO. Films must be Norwegian.
2. **Story versus cards.** The brief's story (delivery status → sources → findings → customer draft → "Prepare follow-up") is a Følg opp story. The owner wants films on 02 Plan and 03 Undersøk. Either the brief is a template and each card gets its own beat sheet from `demo-data.ts` (Orion plan; export blocked by missing `customer_id`), or the stage copy changes. Recommend the former so the films, the card copy and the linked product pages agree.
3. **Source chips must be real connectors.** "Bring tracking" is not a known Verevon connector. Chips should name sources the product has (documents, Inbox/e-post, Visma.net, meeting notes). The site's truth policy (`plans/product-recordings/PRODUCT-SECTION.md`, `product-recording-contract.ts`) refuses fabricated results; a film that is a fixture must be labelled as an illustration, as the feature films are.
4. **Loop closure.** The brief's end state (draft + button) is not the opening state (empty composer). Needs a 0.5 s reset beat at 7.4–8.0 s, as `remotion/timeline.ts` already does with its `reset` phase.
5. **Frame.** Master is 16:10 1280×800. The stage-02 frame marker is 696×405 (≈1.72:1), stage-01/03 markers differ, and an Aside-style tile is 1.28:1. Decide: keep the pinned morphing loop (film as a stage layer, `object-fit: cover` crops) or replace with a static 3-tile row like Aside. The loop already has the video plumbing; the tile row is a layout rewrite.
6. **Card 04 "like the original".** Same root cause as the films. Long-term fix is the same demo route embedded (static Vite build served under verevon.com `/demo/`), not another hand-written mock. Short-term fix is porting the SPA's tokens into `product-demo.module.css`.

## Next steps if approved

1. Owner picks: loop stage vs tile row; two beat sheets in Norwegian for 02 and 03; connector names for chips.
2. SPA: `/demo/<scenario>` route behind a dev flag, fixture transcript, timeline hook with `window.__seek(ms)` and overlay cursor.
3. Capture script: CDP virtual time, 480 frames, ffmpeg to h264 crf 19 + vp9 + poster, into `public/product-films/`.
4. Wire `loopStages[1..2]` to `kind: "video"`; keep 04 on the demo route embed.
5. Reduced-motion fallback: poster frame only.
