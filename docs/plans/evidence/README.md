# Space evidence tree

Created by slice `M0-T0` of the Space hybrid build plan (`docs/plans/space-hybrid-build-plan-2026-10-14.md`, sections 4.1.1, 6.0, 6.4 and 6.6).

| Path | Content |
|---|---|
| `baseline-<yyyy-mm-dd>/` | Day-0 failing test ids, one JSON per watch-list command (`<command>.json`, plus `manifest.json` with the git SHA and tool versions). Written by `node scripts/tests/baseline-capture.mjs`. |
| `<yyyy-mm-dd>-<WSnn or Mn>/` | Evidence bundle of one slice or milestone: `manifest.json`, `commands.txt`, SQL outputs, HAR per flow (tokens redacted), screenshots. Defined in plan 6.4. |

## Baseline

The baseline is the failing set on the BASE commit, so capture it from a clean checkout of that commit, not from a tree that already carries Space edits:

```
git worktree add ../space-baseline <base-sha>
node scripts/tests/baseline-capture.mjs --root ../space-baseline --out docs/plans/evidence/baseline-<date>
node scripts/tests/baseline-capture.mjs --list          # the commands and their ids
```

Compare a later run with the stored baseline (exit 1 on any new failing id):

```
node scripts/tests/baseline-capture.mjs --out <dir-for-current-run>
node scripts/tests/baseline-diff.mjs docs/plans/evidence/baseline-<date> <dir-for-current-run>
```

Never trust a process exit code on its own: a non-zero exit with no parsed id is stored as `<command>::exit-<code>`, so it still appears in the diff.

## Live Space proof prerequisites

`bash scripts/space-e2e-seed.sh --apply` seeds users A (owner), B, C, E, D (org Y) and F (ZDR org Z), writes `E2E_STORAGE_STATE_A..F` under `apps/Frontend Plane/verevonv3/tests/e2e/.auth/` (git-ignored) and an env file to source. Space specs use `tests/e2e/space-fixtures.ts`; under `E2E_REQUIRE_SPACE=1` a skipped Space spec is a failure. Space specs never run an agent on a Verevon-routed model (`verevon`, `verevon-*`, `auto`, `default`): they name an explicit model in `E2E_SPACE_MODEL`.
