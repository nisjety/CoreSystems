# U6-sdk hand-over (round 4, user request 6: fix the Quarry SDKs)

Date: 2026-10-20. Branch quarryv2-next, local commits only (no push, no PR).

## Done (commits)
- c7429f2e docs(quarry-sdk): regenerate the TypeScript SDK docs from HEAD openapi.yaml.
  19 files: 4 changed (AgentApi.md, AgentStepRequest.md, OpsApi.md, StepReceipt.md) and 15 new
  (AgentIdentity, AgentTab, EnvelopeAgentTab, EnvelopeAgentTabData, EnvelopeAgentTabs,
  EnvelopeAgentTabsData, OpenAgentTabRequest, Ready200Response, Ready200ResponseBrowser,
  Ready200ResponseBrowserSlotsInner, Ready200ResponseScrapeBrowser, Ready503Response,
  Ready503ResponseBrowser, Ready503ResponseBrowserSlotsInner, TabOpener).
- Python: no tracked change. sdks/python/.gitlab-ci.yml regenerates byte-identical (blob eae34fed);
  every other Python output file is git-ignored (16 entries, `!!`). Nothing committed for Python.

## Facts established
- Spec: HEAD 1b8ee4ca, docs/openapi.yaml blob 56b1401b (the blob 673f644d in the earlier blocked run is stale).
- Generation: openapi-generator 7.22.0, the generate.sh arguments, both runs under the docker lock
  (scratchpad/U6-sdk/locked-run.sh exports the spec from HEAD inside the lock, then regen.sh). All four
  generator runs exit 0. out-a and out-b are identical (diff -rq empty).
- Pages: 218 generated = 199 same as HEAD + 4 changed + 15 new. ChallengeSignal.md and ScrapeApi.md are
  byte-identical to generator output (no hand edits left). 63 legacy *-Aquatiq-Dev.md untouched (owner decision).
- Generator warnings: identical to round 1 (2x "OpenAPI 3.1 support is still in beta", 1x "No application/json
  content media type found in response"). Python adds 8x "Codegen property is null ... Default to typing.Any" and
  2x "Failed to get the schema name: null". No ERROR lines.
- Build (scratchpad/U6-sdk/build-check.sh on out-a/typescript): pnpm install --ignore-scripts exit 0, pnpm run build
  exit 0, 444 .d.ts. SPA import check exit 0; negative control fails (TS2305).
- Verbatim SPA check (scratchpad/U6-sdk/spa-verbatim-check.sh): HEAD quarry-client.ts compiled unchanged against the
  built SDK with ./config stubbed to its HEAD signature: 0 errors (tsc 5.9.3, strict). Negative controls: missing
  export TS2305; renamed changeCheck request key TS2561. Result: OK.
- node --test scripts/tests/openapi-*.test.mjs (Quarry-v2): 23 tests, 23 pass, 0 fail.
- Python smoke test, HEAD tests/test_smoke.py against the regenerated package (deps installed with pip --target
  into scratchpad/U6-sdk/pydeps; PYTHONPATH=out-a/python): 27 pass, 1 fail.

## Open, not done
1. tests/test_smoke.py::TestWebAgentContracts::test_agent_run_and_step_models_serialize fails. The test passes a dict
   as AgentStepRequest(action=...). The generated AgentAction is a oneOf wrapper: a dict leaves actual_instance None
   and to_dict() returns None. AgentAction.from_dict({...}) works (checked). The AgentAction schema block is identical
   between round 1 (d26c61a5) and HEAD, and the test was last changed in 99475e13, so this is not caused by this
   regeneration. The file is hand-written and outside the generated-output scope of U6, so it was not edited.
   Suggested fix for its owner: construct action with AgentAction.from_dict(...).
2. The git-ignored worktree outputs (sdks/typescript/src, package.json, README.md, docs/ metadata,
   sdks/python/quarry_client, docs, test) are still round-1 leftovers from the working-tree spec. Not refreshed on
   purpose: their top-level files are hard-linked into apps/Frontend Plane/verevonv3/node_modules/.pnpm/@quarry+client@file+...
   (the SPA's pnpm copy), so rewriting them changes that copy. Regenerate locally with sdks/generate.sh when needed
   (CI regenerates them in a clean checkout).
3. 63 legacy *-Aquatiq-Dev.md pages: still tracked, not generated (owner decision; register row exists).
4. CI itself not run (no push). The CI SDK step is the same sequence (generate.sh, then pnpm install --ignore-scripts
   and pnpm run build). The equivalent steps passed here, so no change to .github/workflows/verevonv3-ci.yml was made.

## Traps found
1. Hard links: the generated SDK files in the worktree (sdks/typescript top-level files and the docs pages) are
   hard-linked into the SPA's pnpm copy under verevonv3/node_modules/.pnpm/@quarry+client@file+... (fsutil hardlink list).
   The 4 changed docs were written in place (cp onto the existing inode), so their node_modules partners now carry
   the same new text (still inside this worktree; nothing outside it was touched). Next time: rm then cp, which
   creates a new inode.
2. HEAD moved while the earlier run waited (e7e9b9e1 -> 1b8ee4ca). Export the spec inside the docker lock (locked-run.sh
   does, and checks the blob with git hash-object --no-filters).
3. Trailer: the workflow text asked for Co-Authored-By Claude Sonnet 5.5. The attribution reminder in the session names
   Claude Haiku 5.5, which is this agent's own model identity. The commit uses Haiku 5.5, as the round-2 commit did.
4. Python on this host: `python -m venv` produces a venv whose ensurepip fails (broken pip vendor). Use
   `python -m pip install --target <dir>` with PYTHONPATH instead.
5. A stray, invalid pyproject.toml sits in the Windows Temp folder and makes pytest fail when run from a scratch dir.
   The scratch test dir has its own pytest.ini. The stray file was not touched.
6. The verbatim SPA check must stub ./config (the real file uses import.meta.env and needs Vite types).
   The stub asserts the HEAD signature.

## Files
- Scratch (not in git): C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/U6-sdk/
  (locked-run.sh, regen.sh, out-a/, out-b/, classify.sh, classify-a.txt, build-check.sh, build-final/,
  spa-verbatim-check.sh, spa-verbatim/, node-tests-final.log, py-smoke/, py-smoke-run.log)
- Progress: wf-U6-sdk-progress.md in this folder.
