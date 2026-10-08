# 006 — Delete the chat launch wash and the composer-dock entrance

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Motion / reduced motion, purpose & frequency
- **Estimated scope**: 3 files (`global.css`, `ChatPage.tsx`, `use-chat-controller.ts`), ~40 lines removed

## Problem

When Enter sends a new chat's first message (keyboard-initiated, the most frequent action in the product) three animations run: a 1200 ms full-page "wash", a 360 ms slide of the composer dock the user is typing in, and a 600 ms thread launch. Under `prefers-reduced-motion: reduce` the wash becomes an opaque panel covering the whole chat for 1.2 s, because only its keyframes set opacity and the reduced-motion block removes the animation:

```css
/* src/styles/global.css:24831 — current */
.verevon-chat-launch-wash {
  position: absolute;
  inset: 0;
  z-index: 100;
  background: var(--panel);
  pointer-events: none;
  animation: verevonChatLaunchWash 1200ms cubic-bezier(0.19, 1, 0.22, 1) both;
}
/* global.css:24844 */
@keyframes verevonChatLaunchWash { 0% { opacity: 0; } 15% { opacity: 0.5; } 100% { opacity: 0; } }

/* global.css:24869 — reduced-motion block */
@media (prefers-reduced-motion: reduce) {
  .verevon-chat-composer-dock,
  .verevon-thinking-dots span,
  .verevon-chat-streaming::after,
  .verevon-chat-launch-wash,
  .verevon-chat-page-launch .verevon-chat-thread {
    animation: none !important;
  }
```

```tsx
// src/features/chat/components/ChatPage.tsx:729 — current
<Show when={launchMotion()}>
  <div class="verevon-chat-launch-wash" aria-hidden="true" />
</Show>
```

```ts
// src/features/chat/components/use-chat-controller.ts:1906 — current
const triggerLaunchMotion = () => {
  requestAnimationFrame(() => {
    setLaunchMotion(true)
    setTimeout(() => setLaunchMotion(false), 1200)
  })
}
```

```css
/* global.css:22832 — current (the dock is defined three times: 22832, 28049, 46770) */
.verevon-chat-composer-dock {
  …
  animation: verevonChatComposerDockIn 360ms cubic-bezier(0.19, 1, 0.22, 1) both;
}
/* global.css:22854 */
@keyframes verevonChatComposerDockIn { 0% { opacity: 0; transform: translateY(22px) scale(0.985); } 100% { opacity: 1; transform: none; } }
```

`document.getAnimations()` shows the dock animation running on every visit to /chat, and it replays on ⌘⇧↓ because the dock remounts on `hasMessages()`.

## Target

- No wash element, no wash keyframes.
- No dock entrance animation; the dock is simply present.
- The thread launch stays but is short and reduced-motion safe:

```css
/* global.css:24850 — target */
.verevon-chat-page-launch .verevon-chat-thread {
  animation: verevonChatThreadLaunch 200ms cubic-bezier(0.19, 1, 0.22, 1) both;
}
@keyframes verevonChatThreadLaunch {
  0% { opacity: 0; transform: translateY(6px); }
  100% { opacity: 1; transform: translateY(0); }
}
@media (prefers-reduced-motion: reduce) {
  .verevon-chat-page-launch .verevon-chat-thread { animation-name: verevonChatThreadLaunchFade; }
}
@keyframes verevonChatThreadLaunchFade { 0% { opacity: 0; } 100% { opacity: 1; } }
```

and the timer matches: `setTimeout(() => setLaunchMotion(false), 200)`.

## Repo conventions to follow

- Chat keyframes live in `global.css` 22700–28300 and 44300–48380; the reduced-motion guard for chat is the block at 24869 ("Every new chat keyframe MUST be listed here").
- The dominant easing is `cubic-bezier(0.19, 1, 0.22, 1)`; keep it (or `var(--ease-out)` once plan 009 lands).

## Steps

1. `ChatPage.tsx:729–731`: delete the `<Show when={launchMotion()}>…wash…</Show>` block. Keep the `verevon-chat-page-launch` class toggle at line 721.
2. `global.css`: delete the `.verevon-chat-launch-wash` rule (24831–24838) and the `verevonChatLaunchWash` keyframes (24844–24848); remove `.verevon-chat-launch-wash,` from the reduced-motion list at 24869–24876.
3. `global.css:22840`: delete the `animation:` line from the first `.verevon-chat-composer-dock` block; delete the `verevonChatComposerDockIn` keyframes (22854–22866); remove `.verevon-chat-composer-dock,` from the reduced-motion list at 24870.
4. `global.css:24850`: change the thread launch to 200 ms and replace its keyframes with the **Target** (translateY 6px); add the fade-only variant under reduced motion.
5. `use-chat-controller.ts:1909`: change the timeout from `1200` to `200`.
6. Optional but recommended: merge the three `.verevon-chat-composer-dock` blocks (22832, 28049, 46770) into the last one so the cascade is readable; the winning values today are `z-index: 14; padding: 8px clamp(12px, 2vw, 22px) 16px; background: linear-gradient(…)` from 46770 plus `position: relative; flex-shrink: 0; backdrop-filter: blur(18px)` from 22832.

## Boundaries

- Do NOT touch the thinking dots or streaming cursor animations.
- Do NOT change the composer component (`DashboardComposer.tsx`).
- Do NOT add dependencies.
- If the wash markup or the dock `animation:` line is not where the excerpts say, STOP and report.

## Verification

- **Mechanical**: `grep -c "verevonChatLaunchWash\|verevonChatComposerDockIn\|verevon-chat-launch-wash" src/styles/global.css src/features/chat/components/ChatPage.tsx` → `0`. `pnpm typecheck && pnpm lint` exit 0. `pnpm vitest run src/features/chat` passes.
- **Feel check**: open http://localhost:5173/chat, click "Ny", type a message and press Enter. The composer does not move; the thread fades/rises 6px over 200 ms; no panel flashes over the page. In DevTools → Rendering → emulate `prefers-reduced-motion: reduce`, repeat: the thread fades only; nothing covers the chat. Navigate away and back to /chat: `document.getAnimations()` is empty when idle.
- **Done when**: sending the first message never covers the chat and the dock never animates.
