# 007 — Show chat message actions on touch (gate the hover-reveal)

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Hit areas / touch, hover gating
- **Estimated scope**: 1 file (`src/styles/global.css`) + 1 attribute in `ChatPage.tsx`, ~20 lines

## Problem

Assistant message actions (Copy, 👍, 👎, Regenerate, Export, More) are hidden with `opacity: 0` until the message is hovered. There is no `@media (hover: hover)` gate anywhere in the chat CSS, so on touch devices the buttons are invisible but still clickable: a tap near the row can fire an unseen Regenerate.

```css
/* src/styles/global.css:28204 — current */
.verevon-chat-message--assistant .verevon-chat-action-button,
.verevon-chat-message--assistant .verevon-chat-menu > button {
  opacity: 0;
  transition:
    opacity 160ms ease,
    background-color 140ms ease,
    color 140ms ease;
}

.verevon-chat-message--assistant:hover .verevon-chat-action-button,
.verevon-chat-message--assistant:hover .verevon-chat-menu > button,
.verevon-chat-message-actions:focus-within .verevon-chat-action-button,
.verevon-chat-message-actions:focus-within .verevon-chat-menu > button {
  opacity: 1;
}
```

## Target

```css
/* only hide on devices that can hover; always show on the latest assistant turn */
@media (hover: hover) and (pointer: fine) {
  .verevon-chat-message--assistant:not([data-latest]) .verevon-chat-action-button,
  .verevon-chat-message--assistant:not([data-latest]) .verevon-chat-menu > button {
    opacity: 0;
  }

  .verevon-chat-message--assistant:hover .verevon-chat-action-button,
  .verevon-chat-message--assistant:hover .verevon-chat-menu > button,
  .verevon-chat-message-actions:focus-within .verevon-chat-action-button,
  .verevon-chat-message-actions:focus-within .verevon-chat-menu > button {
    opacity: 1;
  }
}

.verevon-chat-message--assistant .verevon-chat-action-button,
.verevon-chat-message--assistant .verevon-chat-menu > button {
  transition:
    opacity 160ms ease,
    background-color 140ms ease,
    color 140ms ease;
}
```

The last assistant `<article>` carries `data-latest` so its actions are always visible (it is the turn users act on most).

## Repo conventions to follow

- Existing hover gates: `global.css:6975` and `25179` — copy their form `@media (hover: hover) and (pointer: fine)`.
- The article class list is built in `ChatMessages.tsx` (`verevon-chat-message verevon-chat-message--assistant`); boolean data attributes elsewhere use `data-working`, `data-unread` (`global.css:48064, 48191`).

## Steps

1. In `global.css`, replace the two rules at 28204–28218 with the **Target** CSS.
2. In `ChatMessages.tsx`, where the assistant `<article>` is rendered, add `data-latest={props.isLatest ? '' : undefined}`; in `ChatPage.tsx:809` pass `isLatest={index() === messageRows().length - 1}` (or the equivalent accessor after plan 005).
3. Leave `:focus-within` reveal in place so keyboard users still see the actions on non-latest turns.

## Boundaries

- Do NOT change the 44px action-button recipe (`global.css:26281`).
- Do NOT add press feedback or other hover gating here (separate MEDIUM finding covers all chat hovers).
- If the rules at 28204 differ from the excerpt, STOP and report.

## Verification

- **Mechanical**: `pnpm typecheck && pnpm lint` exit 0.
- **Feel check**: desktop with a mouse — hover an older assistant message: actions fade in over 160 ms; move away: they fade out; the latest assistant message shows its actions at rest. DevTools → device toolbar (touch emulation, e.g. iPhone) — reload /chat: every assistant message shows its actions; nothing is invisible-but-tappable. Tab into a message-actions row: it reveals.
- **Done when**: on a touch viewport no action button has `opacity: 0`.
