# Hand-over, W2-U18 (WS9.A omnibox search)

Built and committed in a3c2b6f3c (dark: VITE_BROWSER_OMNIBOX_SEARCH unset = off). Full status in W2-U18-progress.md; open items in depth/W2-U18.md.

Traps found while building:
- The gateway module root is apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions.rs, not domains/actions/actions.rs as the plan says.
- The plan cites docs/BROWSER_WORKSPACE_PLAN.md and tests/action-surface-contract.test.mjs; neither is in this tree.
- A Node edit script that fails on a non-unique anchor writes nothing, so a failed run leaves the file untouched. Check the anchor count before a second run.
- `find` under the frontend folder walks node_modules and stalls the Bash tool for minutes. Use Glob or Grep with a path.
- The background tsc run wrote an empty log when started with `( ... ) &` in the same call; running `pnpm exec tsc --noEmit -p tsconfig.app.json` in the foreground with a timeout worked (about 1 to 2 minutes).
- Single-file vitest on this shared machine did not start its worker within 60 s. Not retried, per the build-first rule.
