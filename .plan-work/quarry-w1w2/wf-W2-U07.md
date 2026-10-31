# Hand-over, unit W2-U07 (WS8.E driver foundation)

## What was done

- Tab pins, background open, per-tab egress and tab-scoped observations in the chromiumoxide driver, with the trait defaults and capability bits. Details: W2-U07-progress.md. Code: commit 6ec6a34bb. RUNBOOKS section: in HEAD (commit 61c4e8d6e, see the incident).
- Decisions taken while building, for the reader of the code:
  - Egress resolution is tab-first, then session, then deny-all, as the plan's "key lookup order" says. This is not a meet, so a wider tab policy widens its tab. Depth bullet (high). No caller installs a tab policy yet.
  - A pinned handle never falls back: actions, goto, and reads of a closed pin return NOT_FOUND with details.reason pinned_tab_closed. Session-loss errors still take precedence over the pin error.
  - A background open activates the new tab only when the session has no active tab, so a session always has somewhere to act.
  - Pinned observation filters apply after the sequence continuity check of egress receipts, so a gap in the session's stream is still reported to a pinned caller.
  - respond_dialog on a pinned handle is refused with TargetRepairRequired when the dialog belongs to another tab.

## What is left

- Run the unit tests and the seven live tests (final test phase; see RUNBOOKS for the command).
- The WS8.A measurements (F3 block count, broker latency at 20 actions/s, two-tab RSS).
- The Model Plane ledger entry (sibling (b)).
- The tab-first vs meet decision before any caller (depth, high).

## Incident: shared index and a RUNBOOKS commit (2026-10-23)

What happened: I committed the RUNBOOKS hunk with `git commit` (no pathspec) from a hand-built index entry. The worktree index is shared with other sessions. Two things went wrong:

1. Commit 61c4e8d6e took whatever was staged at that moment: besides my RUNBOOKS blob it contained three other sessions' staged files (apps/.../src/shared/actions/action-registry.ts, apps/.../src/shared/api/orchestration-client.ts, and orchestration-client.decide-approval.test.ts). Those files are now in history on quarryv2-next. Their owner should check that they are the intended content.
2. My RUNBOOKS blob was built from a HEAD that another session had moved on from, so 61c4e8d6e dropped the body of the E3.5-2 webhook section (its heading was replaced). Another session then restored the eval CI section (6e6ea62c3). The current HEAD RUNBOOKS has the webhook section, my section and the eval CI section. I checked the heading list against 7c250ea01 (only the duplicated webhook heading is gone). I did not rewrite history: the branch moved on several times while I worked.

I also tried `git update-ref` to move HEAD back and the compare-and-swap refused (the branch had moved). No history was rewritten. The index entry for RUNBOOKS was reset to the HEAD blob, so nothing of mine stays staged.

Traps for the next reader:
- Commit with explicit paths (`git commit -- <paths>`, or qcommit.sh), never a bare `git commit`, in this shared tree.
- Do not build a commit from a HEAD read earlier in the same session: another session may have committed in between. Re-read HEAD immediately before any plumbing commit, and use update-ref with the old value.
- RUNBOOKS.md is edited by several sessions at once; its working-tree copy is mixed-EOL and does not contain the WS8.E section (the section is in HEAD). Whoever commits that file from the working tree must check the section survives.

## Verification status

See W2-U07-progress.md, "Verified" and "Not run".
