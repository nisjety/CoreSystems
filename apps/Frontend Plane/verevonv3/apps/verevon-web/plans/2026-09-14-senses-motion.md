# Senses: Verevon in motion

Implemented three illustrated workflows in the existing Senses section. Approved Norwegian copy is retained, including the promise for selected tasks, the team's control, and Verevon learning from feedback. Product demo work remains separate.

## Three stories

| Chapter | Beginning | Working state | Outcome |
| --- | --- | --- | --- |
| La Verevon gjøre jobben | Delegate overdue-invoice follow-up | Search inbox, extract invoice number/date/amount, compare with Visma | Follow-up sent and log updated within the team's rules |
| Dere er ekspertene | Demonstrate a weekly report | Ask for deviations first; Verevon remembers the feedback | Next Monday's report follows the learned preference |
| Se arbeidet. Styr retningen | Three tasks progressing in a shared workspace | Supplier agreement comes forward for review; Kari comments | The team approves, then Verevon proceeds |

## Visual direction

Paper-white foreground panels over architectural glass, terracotta signals and cursors, with a darker setting for team control. Background work recedes with blur as the relevant result moves forward. Each card tells a complete story rather than cycling through unrelated UI fragments.

Reference pages inspected: [Lassie](https://www.lassie.ai/), [Cohere](https://cohere.com/), [Taito](https://taito.ai/), [Wonderful](https://www.wonderful.ai/agents), [Intercom](https://www.intercom.com/), [Grok Bot](https://x.ai/bot), [Anima](https://www.animaapp.com/), and [Beam](https://beam.ai/). The visual references informed depth, focus, teaching gestures, and state changes. No competitor artwork or text is reproduced.

The glass background was generated with Higgsfield; provenance is in `public/verevon-senses/SOURCES.md`.

## Playback and editing

- Remotion Player in the page, with three editable Studio compositions: `Senses-Delegering`, `Senses-Laering`, and `Senses-Oversikt`.
- 720 × 640, 30 fps, 540 frames / 18 seconds per loop; three six-second chapters.
- Individual pause/play, restart, and chapter selection controls.
- Lazy loading near the viewport; playback pauses offscreen and when the page is hidden.
- Reduced-motion preference shows a still outcome by default. Explicit playback remains available.
- Frame-driven interface animation; short transform/opacity entrances, deliberate reading holds, quiet loop transitions. The original Senses scroll choreography is retained: viewport-centered copy, chapter-clipped handoffs, 25% fade-in / 50% hold / 25% fade-out (first chapter starts visible), and alternating media parallax from scale 1.1 / y -100 to scale 1 / y 100. Each desktop chapter reserves 75svh. Below 1024px and when the site's motion policy selects reduced motion, content stays in normal flow. The active scroll layout uses the same decision as GSAP, including the existing site-wide full-motion setting.
- Graphics are marked as examples, with a textual equivalent for assistive technology. They are illustrative workflows, not a connection to a live account.

## Verification

- Production build passed with all 13 application routes.
- Desktop browser review of all three scenes and their key states.
- All three outcomes and playback controls reviewed in a 390 × 844 same-origin iframe rendering the actual homepage.
- Keyboard play/pause verified. Chapter jumps, pause and restart checked during browser review.
- Offscreen pause verified: the learning scene's progress remained at `0.881262` while the oversight scene was operated.
- No browser console errors during the final mobile check.
- Final learning card spacing and the oversight completion message were corrected after visual review.
- Restored scroll behavior reviewed on 15 September: all three desktop chapters, reverse scrolling, centered fixed copy, 774 px chapter heights at a 1032 px viewport, media parallax, and working player controls. No browser errors during that review.
- Operating-system reduced-motion behavior is implemented and code-reviewed; OS preference emulation was not available in the browser tool.

Temporary responsive-review files were removed after verification. The source compositions remain editable; no rendered video is required for the live website.
