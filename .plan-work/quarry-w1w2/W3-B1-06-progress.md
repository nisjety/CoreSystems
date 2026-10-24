# W3-B1-06 progress (E3.2-b keyboard-first approval card)

Status: built dark, committed in three steps. Card not mounted (swap waits for W2 U62 / H12).

## Committed

- `61c4e8d6e`: contract and client. Holds the content of action-registry.ts (reason cap of 1000 and exported schema), orchestration-client.ts (decideApproval validates against the contract before POST) and orchestration-client.decide-approval.test.ts. The content landed inside another unit's WS8.E RUNBOOKS commit because a concurrent commit picked up those paths. The message is wrong for these files; history was not rewritten.
- `e94ae3737`: feat(chat) `matchApprovalShortcut` in use-chat-shortcuts.ts, plus use-chat-shortcuts.approval.test.ts.
- `cfdc60c1d`: feat(browser) new features/browser/approval/ (approval-model.ts, BrowserApprovalDecisionCard.tsx, browser-approval-card.css, approval-model.test.ts, BrowserApprovalDecisionCard.test.tsx) and docs/BROWSER_APPROVAL_CARD_E3_2B.md.

## Verified (what was run)

- `vitest run src/features/browser/approval/approval-model.test.ts src/features/chat/lib/use-chat-shortcuts.approval.test.ts`: 2 files, 24 tests, passed (56 s).
- `tsc -p tsconfig.app.json --noEmit` in apps/Frontend Plane/verevonv3: 33 errors in total, none in any file this unit touched (RemoteSupportPage.tsx, quarry-client.ts and others predate it). Log in scratchpad W3-B1-06/tsc2.log.

## Written, not run (per the build-first rule)

- BrowserApprovalDecisionCard.test.tsx (7 cases, Solid testing-library, jsdom).
- orchestration-client.decide-approval.test.ts (5 cases incl. the schema-identity check).

## Not done

- Redirect text is not read by the owner: execution-core browser_agent.rs planner path (E3.2 (B), Model Plane). The box stays off (allowRedirect default false).
- Swap of BrowserApprovalCard (BrowserChrome.tsx) and ApprovalRequests (ChatMessages.tsx): waits for W2 U62 (H12).
- D9 (render location with no dock open) is undecided; auto-focus on mount is a decision for the swap.
- Playwright flow for approve-once, deny-beats-approval, audit row: needs WS1 evidence and the swap.
- RUNBOOKS.md not appended (shared working copy has another unit's hunks); slice doc written instead.

## Uncommitted files of this unit

None. All unit paths are clean in `git status`.

## Next (for the swap slice)

1. Decide how a browser gate with no approval id yet is decided (depth bullet, high).
2. Keep allowRedirect off until execution-core reads the reject reason.
3. Mount the card in BrowserChrome.tsx and ChatMessages.tsx (H12, after U62).

Depth bullets: .plan-work/quarry-w1w2/depth/W3-B1-06.md
Hand-over: .plan-work/quarry-w1w2/wf-W3-B1-06.md
