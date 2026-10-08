# 009 — Make CTRL+K and the sidebar instant; introduce motion tokens

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Purpose & frequency, performance, cohesion & tokens
- **Estimated scope**: 2 files (`src/styles/global.css`, `src/features/core/components/CoreNavbar.tsx`), ~30 lines

## Problem

Three of the most frequent actions in the app animate layout for 300–440 ms, and there is no shared motion vocabulary.

1. The CTRL+K search panel plays a 440 ms entrance; the input is focused at t=0 while the panel is still invisible:

```tsx
// src/features/core/components/CoreNavbar.tsx:465 — current
<div class="core-search-overlay__panel verevon-panel-in">
```
```css
/* src/styles/global.css:16473 — current */
.verevon-panel-in { animation: verevonPanelIn 440ms cubic-bezier(0.19, 1, 0.22, 1) both; }
/* global.css:16516 */
@keyframes verevonPanelIn { 0% { opacity: 0; transform: translateY(18px) scale(0.985); } 100% { opacity: 1; transform: translateY(0) scale(1); } }
```

2. Opening or collapsing the sidebar tweens three layout properties for 300 ms (the panel is squeezed while it mounts and unmounts before collapse finishes):

```css
/* global.css:1905 — current (inside the .core-sidebar rule) */
transition:
  width 300ms ease-out,
  background-color 300ms ease-out;
/* global.css:7116 — current (.core-main) */
transition: padding-left 300ms ease-out, padding-top 160ms ease-out;
/* global.css:7174 — current (banner) */
transition: left 300ms ease-out;
```

3. No easing or duration tokens exist; curves are hardcoded (`cubic-bezier(0.19,1,0.22,1)` ×12, `(0.22,1,0.36,1)` ×6, `(0.65,0,0.35,1)` ×4) and two rules use `transition: all`:

```css
/* global.css:6682 — current */
.transition { transition: all 150ms ease; }
/* global.css:9691 — current (inside .verevon-search-expanded-shell) */
transition: all 500ms ease-out;
```

## Target

```css
/* :root — add after global.css:305 (the --verevon-z-* tokens) */
--ease-out: cubic-bezier(0.19, 1, 0.22, 1);
--ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
--dur-fast: 120ms;
--dur-base: 160ms;
--dur-slow: 240ms;

/* CTRL+K: no entrance animation */
/* CoreNavbar.tsx:465 → class="core-search-overlay__panel" (drop verevon-panel-in) */

/* other users of .verevon-panel-in keep a capped entrance */
.verevon-panel-in { animation: verevonPanelIn var(--dur-slow) var(--ease-out) both; }

/* sidebar: no layout tweens */
/* global.css:1905 → */ transition: background-color var(--dur-base) var(--ease-out);
/* global.css:7116 → */ transition: padding-top var(--dur-base) var(--ease-out);
/* global.css:7174 → */ /* delete the transition line */

/* explicit properties instead of `all` */
.transition {
  transition-property: color, background-color, border-color, opacity, transform;
  transition-duration: var(--dur-fast);
  transition-timing-function: var(--ease-out);
}
/* global.css:9691 → */ transition: opacity var(--dur-slow) var(--ease-out);
```

## Repo conventions to follow

- Tokens live in the `:root` block of `global.css`; z-index tokens at 300–305 are the closest siblings (`--verevon-z-popover: 40;`).
- The dominant curve in the file is `cubic-bezier(0.19, 1, 0.22, 1)`; `--ease-out` adopts it so existing motion does not change character.
- `.verevon-panel-in` is also used by the dashboard search panel and the voice modal (`DashboardSearchPanel.tsx:590`, voice modal near `global.css:28609`); those keep the capped version.

## Steps

1. Add the five tokens to `:root` after line 305.
2. `CoreNavbar.tsx:465`: remove `verevon-panel-in` from the class string. Confirm nothing else in `CoreNavbar.tsx` relies on the animation ending (search for `animationend`).
3. `global.css:16473–16475`: change the duration to `var(--dur-slow)` and the curve to `var(--ease-out)`.
4. `global.css:1905–1907`: remove `width 300ms ease-out,` and set `background-color var(--dur-base) var(--ease-out)`.
5. `global.css:7116`: remove `padding-left 300ms ease-out,`.
6. `global.css:7174`: delete `transition: left 300ms ease-out;`.
7. `global.css:6682`: replace with the explicit `.transition` rule from **Target**. Check its three users (`grep -rn '"transition\|class="[^"]*\btransition\b' src/features/agents`) still look right.
8. `global.css:9691`: replace `transition: all 500ms ease-out;` with `transition: opacity var(--dur-slow) var(--ease-out);`.

## Boundaries

- Do NOT replace every hardcoded `cubic-bezier(...)` in the file in this plan; only the rules listed. (A follow-up can migrate them to `var(--ease-out)`.)
- Do NOT change the CTRL+K markup, focus handling or results (separate MEDIUM finding).
- Do NOT add dependencies.
- If any excerpt does not match, STOP and report.

## Verification

- **Mechanical**: `grep -c "transition: all" src/styles/global.css` → `0`. `grep -c "\-\-ease-out:" src/styles/global.css` → `1`. `pnpm typecheck && pnpm lint` exit 0.
- **Feel check**: press CTRL+K on /chat: the panel is fully visible on the very next frame with the caret in the input; press Escape: it disappears instantly. Click a rail item to expand the secondary sidebar: the layout snaps to its final width with no tween, the panel content is not squeezed, and the main area does not slide. In DevTools → Animations at 10% speed, open the dashboard search panel: its entrance is a short rise (240 ms) with the same curve as before.
- **Done when**: CTRL+K has no animation, the sidebar toggle has no layout transition, and `transition: all` no longer appears in the stylesheet.
