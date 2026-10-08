# 008 — Let the Support "Utkast" surface scroll

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Layout / states
- **Estimated scope**: 1 file (`src/styles/global.css`), ~8 lines

## Problem

The drafts surface sets its own viewport-height minimum but is excluded from the flex-child rule its siblings get, while the workspace clips overflow. Computed on /support?surface=drafts: `.verevon-drafts` `flex: 0 1 auto; overflow-y: visible; min-height: 902px` inside `.verevon-support-workspace { overflow: hidden }`. Once the list is taller than the pane, drafts below the fold are unreachable.

```css
/* src/styles/global.css:42599 — current */
.verevon-support-workspace {
  --verevon-support-tabs-height: 45px;
  display: flex;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  overflow: hidden;
  …
}

/* global.css:42614 — current: drafts is missing from this list */
.verevon-support-workspace > .verevon-inbox-page,
.verevon-support-workspace > .verevon-ticketing-page,
.verevon-support-workspace > .verevon-support-outbound {
  min-height: 0;
  flex: 1 1 auto;
}

/* global.css:43202 — current */
.verevon-drafts {
  min-height: calc(100dvh - 132px);
  padding: 28px clamp(16px, 4vw, 56px);
  background: var(--panel);
}
/* a second `min-height: calc(100dvh - 132px)` sits at 43044 in a sibling surface rule */
```

## Target

```css
.verevon-support-workspace > .verevon-inbox-page,
.verevon-support-workspace > .verevon-ticketing-page,
.verevon-support-workspace > .verevon-support-outbound,
.verevon-support-workspace > .verevon-drafts {
  min-height: 0;
  flex: 1 1 auto;
}

.verevon-drafts {
  overflow-y: auto;
  padding: 28px clamp(16px, 4vw, 56px);
  background: var(--panel);
}
```

## Repo conventions to follow

- Sibling surfaces (`.verevon-support-outbound`, `.verevon-inbox-page`) already use the `min-height: 0; flex: 1 1 auto` pattern from the workspace rule.

## Steps

1. Add `.verevon-support-workspace > .verevon-drafts` to the selector list at 42614–42618.
2. In `.verevon-drafts` (43202–43206) delete `min-height: calc(100dvh - 132px);` and add `overflow-y: auto;`.
3. Check the rule at 43044: if it is another support surface (`.verevon-support-outbound` or similar) that also sits inside the workspace, delete its `min-height: calc(100dvh - 132px)` as well; if it belongs to a page outside the workspace, leave it.

## Boundaries

- Do NOT change `VerevonDraftsQueue.tsx`.
- Do NOT touch the ticketing or outbound rules beyond step 3.
- If 42614 or 43202 differ from the excerpts, STOP and report.

## Verification

- **Mechanical**: `grep -n "calc(100dvh - 132px)" src/styles/global.css` shows no `.verevon-drafts` occurrence. `pnpm lint` exits 0.
- **Feel check**: open http://localhost:5173/support?surface=drafts. With no drafts the empty state is unchanged. Temporarily shrink the browser to 500px tall: the surface scrolls within the pane (the tab bar stays fixed at the top) and the page body itself does not scroll.
- **Done when**: `getComputedStyle(document.querySelector('.verevon-drafts')).overflowY === 'auto'` and the surface's `flex` is `1 1 auto`.
