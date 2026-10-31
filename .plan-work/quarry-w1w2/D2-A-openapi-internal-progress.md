# D2-A-openapi-internal: progress and hand-over (decision 3.13 OPENAPI-INTERNAL, first half)

Owner order 2026-10-28: "Yes to all the recommendations in the checklist. Build list A now (OPENAPI-NULL first)".
This unit is list A item 2. Spec: DECISIONS-ROUND-2026-10-27.md section 3.13. Findings not fixed: depth/D2-A-openapi-internal.md.

## Status: done for the first half; the Go half is not started (by design)

| Commit | What |
|---|---|
| cd17f8c5c | docs(openapi): docs/openapi-internal.yaml (new): run_page with refreshed InternalRunPage and InternalRunPageResult, plus the eight edge routes. Allowlist loses the eight internal entries. Coverage test reads the internal file too. |
| 07af2adcf | fix(quarry-sdk): run_page and the internal-only schemas leave docs/openapi.yaml; the six tracked internal SDK docs and the three Python smoke cases go; the guard test (new); the rule in docs/CONTRACTS.md. The generated SDK was regenerated in the same change. |
| (this commit) | docs(plan-work): this file and depth/D2-A-openapi-internal.md |

## What was checked before the work (so the next agent does not redo it)

- The edge registers nine /v1/internal/* routes (routes.rs 103-131, queue_routes.rs internal_router). Before this unit only
  run_page was in docs/openapi.yaml; the other eight were on scripts/openapi-route-allowlist.json.
- Only sdks/typescript/docs/** and sdks/python/tests/test_smoke.py are tracked under sdks/. The rest of the SDK is
  gitignored (Quarry-v2 .gitignore): regenerate it, do not commit it. A regeneration does not delete stale files.
- The reachability analysis (scratchpad/D2-A-openapi-internal/refs.mjs) found the internal-only schemas: InternalRunPage,
  InternalRunPageResult, CrawlContext, CrawlScope, CrawlReport, CrawlDenials, CrawlDenialSample, CrawlRobotsDenial.
  CachePolicy, DriverSignals and Envelope_Error are shared with public paths and were copied, not moved.
- Files in this worktree are LF. (A `grep -c $'\r'` check looked like CRLF; it was a false reading: the Bash tool does not
  expand $'\r'. `tr -cd '\r' < file | wc -c` is the reliable check.)
- The Git Bash path reaches Node through argv (MSYS converts it), but not through a `require('/c/...')` string. Pass paths
  as arguments, or use Windows paths inside JavaScript.
- The docker lock: `MSYS_NO_PATHCONV=1 bash <tools>/with-lock.sh docker 2700 -- bash sdks/generate.sh` ran to GEN_DONE rc=0
  (1532 log lines, no mention of an internal file). Docker answered `docker info`; the generator image
  openapitools/openapi-generator-cli:v7.22.0 was already present.
- Python on this host has no ensurepip for a venv: `python -m pip install --target <scratch>/pylib` worked, and the tests
  ran with PYTHONPATH set to that directory and the SDK directory (Windows separators, via cygpath -w).

## Verification (run in this worktree, final tree)

- `node --test scripts/tests/*.test.mjs` (Quarry): 53 tests, 41 pass, 0 fail, 12 todo (docs-contract option-B pending set).
- `node --test scripts/tests/openapi-internal-guard.test.mjs scripts/tests/openapi-route-coverage.test.mjs`: 19 pass, 0 fail.
- Mutation check on a scratch copy: a /v1/internal/leak path in the public spec fails "the public spec has no /v1/internal path".
- Intermediate-state check (the state of cd17f8c5c): the coverage test with the public spec at HEAD: 10 of 10 pass.
- `tsc -p sdks/typescript --noEmit` with TypeScript 5.9.3 (npx -p typescript@5.9.3): exit 0, no output.
- Python SDK, pytest 9.2.0, pydantic 2.14.1, urllib3 2.8.0: `python -m pytest tests` 25 passed; the whole sdks/python
  collection without --cov (the GitLab job collects the same tests and adds --cov): 338 passed, 2 warnings. The warnings are
  PytestCollectionWarning for the generated model class TestIdLocator, from the public schema of that name (pre-existing:
  it is in the spec at HEAD~2).
- No Rust, Go or gateway code changed, so cargo check was not needed. No stack was touched.

## Traps for the next agent

- The generator does not delete files. After any spec change, delete the generated files that no longer exist (the
  list is in the rule section of docs/CONTRACTS.md). The guard test checks the generated trees on disk for internal modules.
- The tracked `sdks/typescript/docs` holds 63 `*-Aquatiq-Dev.md` files from commit 7c75b3c32. Only the three internal
  ones were in this unit's scope; the other 60 remain (depth item 7).
- The public spec references `bearerAuth` 30 times and defines no securitySchemes (depth item 3). Do not "fix" that inside
  the internal move.
- The public Envelope_* schemas do not match the edge's wire envelope (depth item 2). The internal spec documents the wire.
- Commit trailer: this unit's commits end with `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>` (the model
  this session runs as, per the attribution reminder). The workflow text asked for "Opus 5.5 whatever model you are";
  the owner should say which convention they want. Earlier commits in this branch use both.
- The Go control routes (eight, listed in depth item 1) are not described. They wait for the D2-050 secret split.

## Files of this unit

- Committed: docs/openapi-internal.yaml, docs/openapi.yaml, docs/CONTRACTS.md, scripts/openapi-route-allowlist.json,
  scripts/tests/openapi-route-coverage.test.mjs, scripts/tests/openapi-internal-guard.test.mjs,
  sdks/python/tests/test_smoke.py, the six sdks/typescript/docs internal pages (deleted), and the two .plan-work notes.
- Not committed and not ours: scripts/tests/docs-contract.test.mjs (another unit), crates/quarry-edge/src/{lib,main}.rs and
  tests/support/fixture_site.rs, crates/quarry-runtime/src/step_receipts.rs (line-ending and mod-order strays), rb.patch,
  .plan-work/next-slices.md.
- Scratch (not in the repo): scratchpad/D2-A-openapi-internal/ (extract.mjs, build-internal.mjs, internal-template.yaml,
  apply-public.mjs, edit-coverage.mjs, edit-smoke.mjs, refs.mjs, check-internal.mjs, the gen.log, the node and pytest logs).
