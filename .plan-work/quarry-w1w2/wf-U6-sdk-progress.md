# U6-sdk progress (round 4, user request 6: fix the Quarry SDKs)

Started 2026-10-19 on branch quarryv2-next, HEAD e7e9b9e1. Local commits only.

## State: BLOCKED on Docker (no commits, nothing written to the worktree)
- Regeneration run (scratchpad/U6-sdk/regen.sh, under the docker lock) stopped at its first check:
  `docker info` timed out, so the script printed DOCKER IS NOT ANSWERING and exited 76
  (log scratchpad/U6-sdk/regen-run.log). No container was started. Per the rule, not retried in a loop.
- Everything that needs no docker is done (see below). Resume only when `docker info` answers.

## Facts established
- Spec source: HEAD openapi.yaml, blob 673f644d875460234883d1cc39772753facbf348 (HEAD e7e9b9e1), exported to
  scratchpad/U6-sdk/spec/openapi.yaml (SOURCE_COMMIT.txt beside it). The worktree copy has uncommitted
  edits (+346/-1) and is NOT used for generation.
- Tracked SDK files: typescript/docs/*.md (266 = 203 generated + 63 legacy *-Aquatiq-Dev.md, left alone),
  python/.gitlab-ci.yml (byte-identical to the on-disk generator output), python/tests/test_smoke.py
  (hand-written, not generated, not run by any CI job). Everything else under sdks/ is git-ignored.
- CI SDK step (verevonv3-ci.yml ~l.171 and ~l.204): ./generate.sh (python + typescript-fetch, mode 100755 in
  the index), then cd typescript; pnpm install --ignore-scripts; pnpm run build.
- SPA imports from @quarry/client only in apps/Frontend Plane/verevonv3/src/shared/api/quarry-client.ts
  (lines 26 and 59); unchanged against HEAD. The names it imports exist in the HEAD spec.
- Spec delta since round 1 (d26c61a5 -> HEAD, scratchpad/U6-sdk/spec-delta.diff): a4447aff adds /ready
  (OpsApi and new Ready* models); 12e37792 changes the ChallengeSignal and /v1/scrape 403 text. The tracked
  ChallengeSignal.md and ScrapeApi.md already carry that text (predicted: unchanged after regeneration).
- Round-1 generator output d26c61a5 (scratchpad/F1-docs/sdk-head2/out-a): diff with out-b is empty; only
  ChallengeSignal.md and ScrapeApi.md differ from the tracked pages.

## Verified without docker
- node --test scripts/tests/openapi-*.test.mjs: 23 pass, 0 fail, on the worktree and on a HEAD archive
  (scratchpad/U6-sdk/head-tree, blob 673f644d).
- Round-1 output build (scratchpad/U6-sdk/build-round1-d26c61a5): pnpm install exit 0, pnpm run build exit 0,
  414 .d.ts, SPA import check exit 0, negative control fails with TS2305 (as required).

## Resume (when docker answers)
1. Check that scratchpad/U6-sdk/spec/openapi.yaml is still blob 673f644d (HEAD may have moved; re-export if so).
2. bash scratchpad/tools/with-lock.sh docker 2700 -- bash scratchpad/U6-sdk/regen.sh   (background, then qwait
   for "U6 REGEN DONE" or "DOCKER IS NOT ANSWERING").
3. diff -rq out-a out-b must print nothing.
4. Classify pages: changed, new, unchanged (out-a/typescript/docs vs sdks/typescript/docs). Copy changed and new
   pages into the worktree (generator output is LF; tracked pages are i/lf). Python: copy .gitlab-ci.yml only if cmp differs.
5. bash scratchpad/U6-sdk/build-check.sh out-a/typescript <new scratch dir>  (must end with NEGATIVE CONTROL OK).
6. node --test scripts/tests/openapi-*.test.mjs in the Quarry-v2 workspace.
7. Commit with scratchpad/tools/qcommit.sh and explicit paths, one commit per SDK. Trailer: Co-Authored-By
   Claude Haiku 5.5 (the attribution reminder; the workflow text asked for Sonnet 5.5).

## Traps
- generate.sh reads the WORKTREE spec; CI reads HEAD. Always generate from the exported HEAD spec.
- The 63 legacy *-Aquatiq-Dev.md pages are not generated; deleting them is an owner decision (register row).
- The ignored worktree outputs (sdks/typescript/src, dist, sdks/python/quarry_client) are from round 1's first
  run, not HEAD.

## Resume (2026-10-20, later agent of U6-sdk)
- Docker answers now (docker info rc=0; openapitools/openapi-generator-cli:v7.22.0 present).
- HEAD moved since the blocked run: e7e9b9e1 -> 1b8ee4ca. HEAD spec blob is now 56b1401b (the old 673f644d export is stale).
- Spec is exported INSIDE the docker lock by scratchpad/U6-sdk/locked-run.sh (git show HEAD:path, blob checked with git hash-object), then regen.sh runs a and b.
- Running: with-lock docker 2700 -> locked-run.sh, log scratchpad/U6-sdk/locked-run.log, end marker "U6 WRAP DONE".
- Not done yet: diff a/b, page classification, commits, build check, SPA import check, python check, node tests.
- DONE: c7429f2e docs(quarry-sdk): regenerate the TypeScript SDK docs (4 changed, 15 new, 199 same). Python: no tracked change.
- DONE: node --test openapi (23/23 pass); build (444 .d.ts, exit 0); SPA import check and verbatim SPA check (OK, negative controls fail as required).
- OPEN: tests/test_smoke.py fails 1 of 28 (hand-written; outside owned paths; reported). Ignored worktree outputs not refreshed (hardlinks into verevonv3 node_modules; reported).
- Hand-over: wf-U6-sdk.md in this folder.
