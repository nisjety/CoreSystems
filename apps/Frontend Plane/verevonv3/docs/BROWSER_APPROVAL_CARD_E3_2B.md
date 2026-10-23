# Browser approval card (E3.2-b) runbook

Status: built dark. The card is not mounted anywhere yet. The swap of
`BrowserApprovalCard` (BrowserChrome.tsx) and `ApprovalRequests` (ChatMessages.tsx)
waits for W2 U62 (H12). The redirect box is off until the planner path takes
redirect text (E3.2 (B), execution-core `browser_agent.rs`).

## Where it lives

| Piece | File |
|---|---|
| Card component | `src/features/browser/approval/BrowserApprovalDecisionCard.tsx` |
| Pure view model, redirect check, decision request | `src/features/browser/approval/approval-model.ts` |
| Keyboard matcher (pure) | `matchApprovalShortcut` in `src/features/chat/lib/use-chat-shortcuts.ts` |
| Action contract | `orchestration.decide_approval` in `src/shared/actions/action-registry.ts` (`ORCHESTRATION_APPROVAL_REASON_MAX` = 1000) |
| Decision call | `decideApproval` in `src/shared/api/orchestration-client.ts` (checks input against the contract before POST) |
| Styles | `src/features/browser/approval/browser-approval-card.css` |

## Keys (only inside the card)

- Cmd/Ctrl+Enter: approve. Refused while redirect text is typed.
- Cmd/Ctrl+Shift+Enter: reject. With redirect text it sends the redirect.
- Enter in the redirect box: send the redirect (needs text). Shift+Enter: newline.
- Escape in the redirect box: clear the text.
- IME composition and Alt combos are ignored.

On mount the card takes focus only when nothing else has focus (the composer keeps its focus).

## Wire shape

- Approve: `POST /api/v1/orchestration/approvals/{id}/decide` with `{"decision":"approve","reason":""}`.
- Plain reject: same route with `reason:""`.
- Redirect: same route with `decision:"reject"` and `reason` = trimmed text (max 1000 chars).

The gateway passes `reason` through unchanged. A reject with reason aborts the run
until the owner reads the redirect text.

## Turning it on (later slices)

1. Swap the browser gate and the chat deck to the card (after W2 U62 / H12).
2. Set `allowRedirect` only when execution-core accepts redirect text on reject.

## Tests

- `approval-model.test.ts`, `use-chat-shortcuts.approval.test.ts`,
  `orchestration-client.decide-approval.test.ts`, `BrowserApprovalDecisionCard.test.tsx`.
- Written in W3-B1-06 and compile-checked only. Run with `pnpm test` (or
  `npx vitest run src/features/browser/approval src/features/chat/lib/use-chat-shortcuts.approval.test.ts src/shared/api/orchestration-client.decide-approval.test.ts`).
