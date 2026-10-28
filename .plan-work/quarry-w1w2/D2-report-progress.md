# D2-report progress (2026-10-28)

Unit: D2-report (docs and notes only). Owner order 2026-10-28: yes to all 13 recommendations; build list A, prepare list B.

## Done (files)

1. Plan `docs/plans/quarry-browser-build-plan-2026-10-14.md` (CRLF kept, exact-anchor edits with lib-edit.js, no reformat): ADR-6 recorded as ratified and the X-20 gates rewritten per decision 3.8 (X-20 row, X-20 paragraph ruling, ADR-6 row and bullet, not-in-scope bullet, WS13.F gate row, WS13.5 row, the cross-session sentence, the flag table row, a ledger entry in "Owner decisions of 2026-10-28"); one dated 2026-10-28 line in the STATUS AT THE STOP block.
2. `X-9-control-plane-ask.md` (new): the two asks (consumer row; emit on a plan change), what Quarry does, what waits, counsel's question. Facts checked in the Control Plane tree: subject and payload (`org-core/internal/nats/shared_publisher.go`), outbox written only in `SetInteractiveRetention` on false-to-true (`repository.go`), the Conversation Core consumer row (`audit-core/internal/provisioner/provisioner.go`), `GetRetentionPostureState`.
3. `STACK-SESSION-RUNBOOK.md` (new): preparation, SC1 to SC7 (L2 legs), then X-12, D16, X-10, X-19, ADR-6, list A leftovers that need the stack, owner questions, results header. [USER] and [AGENT] marks.
4. `DECISIONS-ROUND-2026-10-27.md`: section "Implementation status (2026-10-28)" after section 4 (one row per decision, commits).
5. `STATUS-2026-10-24.md`: section 8.

All commit hashes in these files were checked with `git log -1` (46 of 46 exist).

## Commit

One docs commit with qcommit.sh: the plan, the five notes files above, this file and `depth/D2-report.md`. NOT COMMITTED. One attempt on 2026-10-28: the pre-commit gitleaks hook runs through Docker, the engine answered 500 (`_ping`), and the hook reported "gitleaks found a secret" (a false block from the dead engine; the files contain no secret). Not retried, no --no-verify. The eight files are staged (qcommit's `git add` ran). Once Docker answers, re-run exactly:
`bash <scratchpad>/tools/qcommit.sh <scratchpad>/D2-report/msg.txt docs/plans/quarry-browser-build-plan-2026-10-14.md .plan-work/quarry-w1w2/X-9-control-plane-ask.md .plan-work/quarry-w1w2/STACK-SESSION-RUNBOOK.md .plan-work/quarry-w1w2/DECISIONS-ROUND-2026-10-27.md .plan-work/quarry-w1w2/STATUS-2026-10-24.md .plan-work/quarry-w1w2/D2-report-progress.md .plan-work/quarry-w1w2/depth/D2-report.md`
(`<scratchpad>` = `C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad`). The message file is there.

## Not mine, left alone

`D2-test-2026-10-28.md` and `depth/D2-test-2026-10-28.md` are staged by the D2-test unit and not committed (blocked by Docker). qcommit.sh commits with a pathspec, so they are not swept into this commit. `wf-D2-A-openapi-internal-progress.md` (untracked), `.plan-work/next-slices.md`, `rb.patch`, the quarry-edge and quarry-runtime strays.
