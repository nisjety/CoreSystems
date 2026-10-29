# Homepage refinement — 11 September 2026

## Delivered direction

- Main section headings match the **pre-existing prefooter**, not an additional
  20% reduction: Arbeit Light 300, `clamp(1.45rem,3.6vw,4.95rem)`, line height
  0.88, tracking -0.08em. Mobile gets a readable 28px minimum. Semantic headings
  remain intact; card titles keep their subordinate scale.
- Shared content edge: 100px minimum on wide desktop, expanding to center a
  1512px content area. This reproduces the supplied feature-card screenshot's
  approximately 1512px visual width. Backgrounds can remain full width.
- Product frame entrance and final state use those same content bounds. Copy
  and intermediate imagery occupy separate columns. The ring is 2px at 25%
  ink rather than 1px at 10%. The five-step footer strip is removed from both
  markup and animation dependencies.
- Problem cards: 332px desktop (302 × 1.10 rounded); mobile active cards have a
  300px minimum and expand if needed. Body copy uses warm white, a stronger
  gradient, and a text shadow. Problem heading retains its reveal with a 55%
  readable starting opacity and a light halo.
- Dashboard replaces the navbar email icon and links to `http://localhost:5173`.
  Menu and scrolled navbar share 80% white / 12px blur with solid fallbacks.
  Hero scroll cue removed.
- Real Norwegian SPA composer captured with an **unsent example prompt**.
  Account, customer data, greeting, and conversation history are outside the
  delivered crop. An eight-second Remotion film places it on a muted abstract
  background. It describes a workflow; it does not present fabricated execution
  results. Source: `remotion/VerevonProductShowcase.tsx`. Final scene plays once
  when visible and respects reduced motion. The composer stage also uses the
  existing abstract background and retains its interactive preview.

## Images versus video: recommendation

Use **still images by default on the homepage problem cards**, optionally with
one short muted preview on hover/focus; put longer explanatory videos on the
linked pages. Keep the homepage's main motion investment in the actual product
demonstration. The three problem cards communicate pain and establish relevance;
their text needs to be scannable while visitors decide where to go.

This is a design recommendation, not a measured conversion uplift for Verevon.
NN/g's [Video Usability](https://www.nngroup.com/articles/video-usability/) notes
that page movement can distract, and that essential information should also be
available as text. Its [homepage principles](https://www.nngroup.com/articles/homepage-design-principles/)
also discuss problems caused by intense autoplay without pause controls.
The current cards remain poster-first with intentional hover playback; no
unrequested replacement of all cards with static-only media was made.

For Awwwards, a coherent visual language and an original, understandable product
story are a better target than the number of videos. Awwwards' published scoring
on [The Colors of Motion](https://www.awwwards.com/sites/the-colors-of-motion-1)
weights design 40%, usability 30%, creativity 20%, and content 10%. Its
[mobile guidelines](https://www.awwwards.com/mobile-excellence-guidelines.pdf)
also cover performance and usability. None of these imply that autoplay wins
awards, or that an award predicts more qualified customers.

Test poster-first versus hover-preview with qualified dashboard/demo clicks as
the primary outcome, and card-link engagement, mobile performance, and reading
completion as supporting signals. Do not optimize just for video views.

## Cybercab footage shortlist

The selected control card is card **03**, despite the comment saying “first”.
The existing driving clip is not established as Cybercab footage.

- Best visual reference: Tesla's official
  [Cybercab — The Future is Autonomous](https://www.youtube.com/watch?v=Qfj4urMF8CU).
  The gold exterior and warm interior are closer to the other cards than bright
  blue daytime driving footage. Select a 4–6 second quiet exterior/interior shot,
  reduce saturation, retain warm neutral highlights, and avoid event graphics.
- A separately licensable source was found at
  [Reuters Connect](https://www.reutersconnect.com/item/tesla-cybercab-makes-debut-in-austin/dGFnOnJldXRlcnMuY29tLDIwMjY6bmV3c21sX1ZBMDE4MzAzMDkyMDI2UlAx).
  This is a candidate, not an acquired asset or verified commercial-use license.

**Not replaced:** no licensed downloadable Cybercab master was available in the
workspace. An official YouTube link is a reference, not a local background-video
asset. The existing control media remains until a usable master is selected.
I would also consider a Verevon approval interaction here: it explains this
product's control promise more directly than a recognizable car brand.

## Validation

TypeScript, targeted ESLint, and `git diff --check` passed. Production build
generated all 13 routes successfully (the existing multiple-lockfile warning
remains). The poster and 240-frame H.264 film rendered successfully; the film is
about 577kB. Desktop 1440px, mobile 390px and the restored 1922px viewport were
checked. At 1922px, the content viewport is 1907px and both gutters are 197.5px,
leaving exactly 1512px for content. Main headings and prefooter both compute to
69.192px. No horizontal overflow or captured browser console errors remained.
The menu computes to 80% white and 12px blur, matching the scrolled navbar.
The final video stayed paused while hidden, played when revealed, and stopped
after eight seconds. Temporary viewport and SPA language changes were restored.
