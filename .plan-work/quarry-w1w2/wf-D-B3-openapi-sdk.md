# Hand-over: D-B3-openapi-sdk (2026-10-27)

Read first: `D-D-B3-openapi-sdk-progress.md` (what is committed, with commit hashes and the checks) and `depth/D-D-B3-openapi-sdk.md` (what is open and who decides it).

## What was done

- OpenAPI: the file parses as one document again (duplicate keys in CreateScheduleRequest removed; the preset description fixed). The WS13.5 `data.actions[]` shape from the scrape-actions unit is documented.
- Route coverage and scrape parity tests now match the W2 routes and the anyOf ScrapeFormat golden.
- docs-contract gate restored from the live tree (C:/dev/CoresSystem, untracked) with a whole-file YAML subset check in `scripts/yaml-subset.mjs`. Its 12 option-B and QB-T17 assertions are todo (see depth item 1-2).
- WS16.S0 docs (CROSS_PLANE_AUTH model-plane row, SELF_HOST model section, json_schema header), D2-037 (GAP rows).
- D1-024: challenge classifier fixes in challenge.rs and one call site in observation.rs.

## What is left, and who decides

1. Owner: option B (the rerank wire and config) is in the live tree only. Commit it onto this branch, or accept the pending gate.
2. Owner: QB-T17 compose defaults (AUTOPROMPT on; six switches missing). A compose change, not made here.
3. Owner or spec author: OpenAPI 3.1 `type: 'null'` (13 places, WS17.P0). The generator cannot emit it. Choose a shape, then rerun `sdks/generate.sh` (with `MSYS_NO_PATHCONV=1`) and build the TypeScript SDK with TypeScript 5.9.3. Nothing under `sdks/` is committed.
4. Owner: D1-024 policy for RespondDialog and MouseWheel on an exhausted host (currently allowed, as RUNBOOKS says).
5. Model Plane session: ledger DR-15.1 and the gateway.proto comment (WS16.S0).
6. Whoever brings model_plane_gate.rs to this branch: correct its header (see depth item 4).

## Traps

- Docker lock: another agent holds it for minutes. Queue long jobs in the background with a log and wait with `qwait.sh`.
- Bash tool: a call over 120 s is moved to the background unless `timeout` is set on the call.
- `git diff` on a shared file shows other units' hunks; commit only your own with `qcommit-patch.sh` (one-line hunks are easy to lose).
- `sdks/typescript/src/` and `sdks/python` are generated; `src/` is gitignored and was overwritten by a failed-to-compile regeneration in this worktree.
- The live tree (C:/dev/CoresSystem, branch main) has uncommitted option-B work; do not edit or commit it from here.
