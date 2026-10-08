# 005 — Stop the Support ticket list and the Chat transcript from remounting

- **Status**: DONE - applied 2026-09-29 in the working tree (uncommitted); deviations in plans/README.md
- **Commit**: 83927eda
- **Severity**: HIGH
- **Category**: Solid reactivity / interruptibility / focus
- **Estimated scope**: 4 files (`InboxPage.tsx`, `TicketQueue.tsx`, `ChatPage.tsx`, `ChatMessages.tsx`), ~30 lines

## Problem

Both lists feed `<For>` with freshly created objects, and Solid 2's `<For>` matches items by reference by default (`node_modules/solid-js/types/client/flow.d.ts:17–21`). Every recompute therefore destroys and recreates every row.

Support — verified live: a focused row's node was detached after the 15 s poll (`isConnected: false`), focus fell to `<body>`, and the first row became a new node. Every email iframe is rebuilt every 15 s.

```ts
// src/features/inbox/components/InboxPage.tsx:255 — current (plain function, new objects each call)
const tickets = () => baseTickets().map((ticket) => ({
  ...ticket,
  supportTicket: supportTicketByConversation().get(ticket.conversationId) ?? null,
}))
// InboxPage.tsx:620
const interval = window.setInterval(refreshInbox, 15_000)
```

```tsx
// src/features/inbox/components/TicketQueue.tsx:448 — current
<For each={sortedTickets()}>
  {(ticket) => (
    <li>
```

Chat — verified live: clicking Copy detached the button and its `<article>`; focus fell to `<body>`; the "Copied" button was a different node. Thumbs state, the "Takk — merket som" note, open citation `<details>` and the Detaljer popover all reset for the same reason.

```ts
// src/features/chat/components/ChatPage.tsx:268 — current
const messageRows = createMemo(() => state.turns.map((turn, index, turns) => ({
  turn,
  createdAt: turn.createdAt,
  showDateDivider: shouldShowDateDivider(turns[index - 1], turn),
  copied: copiedTurnId() === turn.id,
  planApproval: { pending: planApprovalPending() === turn.id, error: planApprovalError()[turn.id] },
  version: index === turns.length - 1 ? finalExchangeVersion() : null,
  editLocked: turn.role === 'user' && isEffectfulChatTurn(turns[index + 1]?.effectClass),
})))
// ChatPage.tsx:809 renders <For each={messageRows()}> without `keyed`
```

## Target

Solid 2's `<For>` accepts `keyed={(item) => key}`; in that mode the render callback receives **accessors** for both `item` and `index` (`flow.d.ts:21`).

```tsx
// TicketQueue.tsx — target
<For each={sortedTickets()} keyed={(t) => t.conversationId ?? t.id}>
  {(ticket) => (
    <li>
      <TicketRow ticket={ticket()} … />
```

```ts
// InboxPage.tsx — target
const tickets = createMemo(() => baseTickets().map((ticket) => ({
  ...ticket,
  supportTicket: supportTicketByConversation().get(ticket.conversationId) ?? null,
})))
```

```tsx
// ChatPage.tsx:809 — target
<For each={messageRows()} keyed={(row) => row.turn.id}>
  {(row) => <MessageBlock row={row()} … />}
```

Inside `MessageBlock`, the node list keeps its deliberate unkeyed behaviour for markdown blocks (see the comment at `ChatMessages.tsx:614–618`); the outer per-turn list is what must be stable.

## Repo conventions to follow

- Derived values are `createMemo` (exemplar: `ChatPage.tsx:268`).
- `<For>` usage exemplar with a `<li>` wrapper: `TicketQueue.tsx:448`.
- The transcript's article list is rendered in `ChatPage.tsx` around line 809; `MessageBlock` lives in `ChatMessages.tsx`.

## Steps

1. `InboxPage.tsx:255`: change `const tickets = () => …` to `const tickets = createMemo(() => …)`; import `createMemo` from `solid-js` if not already imported.
2. `TicketQueue.tsx:448`: add `keyed={(t) => t.conversationId ?? t.id}` to the `<For>` and update the callback to read `ticket()` wherever it currently reads `ticket` (the row component props, the `active`/`unread`/`pinned` lookups, the `onClick`).
3. In the conversation transcript (`ConversationPanel.tsx`, the `<For>` over `props.articles` near line 556) add `keyed={(a) => a.id}` and switch the callback to accessor reads.
4. `ChatPage.tsx:809`: add `keyed={(row) => row.turn.id}` to the messages `<For>` and change the callback parameter to an accessor (`row()`), passing `row()` where the row object is consumed. Where the callback reads `row.turn`, `row.copied`, etc., read them through `row()` so updates flow without remounting.
5. Run the tests for both features.

## Boundaries

- Do NOT add `keyed` to the inner node list in `ChatMessages.tsx` (614–618 explains why).
- Do NOT change poll intervals or fetching in this plan.
- Do NOT add virtualization here; it is a follow-up (`content-visibility: auto` is listed in the audit as a first step).
- If `<For>` in the installed Solid version does not accept `keyed`, STOP and report the installed version (`node_modules/.pnpm/solid-js@*`).

## Verification

- **Mechanical**: `pnpm typecheck` exits 0. `pnpm vitest run src/features/inbox src/features/chat` passes.
- **Feel check (Support)**: open http://localhost:5173/support, click a row so it is selected and focused, wait 20 s (one poll). The row stays focused (`document.activeElement` is still the row button), the open email keeps any text selection, and the list does not flicker.
- **Feel check (Chat)**: open a thread, hover an assistant message, click Copy. The button stays in place and switches to "Copied" without disappearing; keyboard focus remains on it. Click 👍 then 👎: the state toggles without the row flashing. Open the "Detaljer" popover, then click Copy on another message: the popover stays open.
- **Done when**: neither list detaches a focused node on refresh or on per-row state changes.
