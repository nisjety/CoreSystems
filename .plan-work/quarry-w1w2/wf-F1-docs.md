# Hand-over: F1-docs (WS5.H leftovers: stale docs and TypeScript SDK regeneration)

Date: 2026-10-18. Branch quarryv2-next, local commits only (no push, no PR).

## Done (commits, in order)
- 12ab61d3 CONTRACTS 9.1: an include_tags selection goes through clean_full whatever extractor and remove_base64_images say; noscript; invalid_selector names exclude_tags too.
- 38c65716 GAP.md and OSS_PARITY_BACKLOG.md: the default parser isolation is process only for the edge binary.
- 3f590d6c ERROR_REASONS.md: health_probe is an op in use (background prober), not reserved. Hunk-only commit.
- b964fe92 RUNBOOKS.md: the non-Unix parser mode is inprocess, silent. Hunk-only commit.
- f292d5bb register: four rows for this round.
- 469e7c7d register: row for the 63 legacy -Aquatiq-Dev SDK pages (action: delete, pending decision; nothing deleted).
- 97186586 sdks/typescript/docs: regenerated from the committed openapi.yaml (d26c61a5, blob 3d9312c1): 11 changed, 10 new pages. Deterministic (two runs identical), builds like the CI SDK step, SPA import names resolve.
- Earlier, by the previous agent (re-verified by reading code): b807f572, 440579d6, f7a94512, cee56eb5, aec85098.

## Traps found
1. Shared hot files: another unit appended WS3.F sections to docs/RUNBOOKS.md and docs/ERROR_REASONS.md while this unit worked. qcommit.sh would have committed their hunks. Used scratchpad/F1-docs/commit-hunk.sh (HEAD content plus one exact-anchor edit, committed via a temporary GIT_INDEX_FILE; the pre-commit hook still runs; the real index entry is then synced). The other unit committed those sections itself in 37b7ebb0 (WS3.F); this unit did not review them beyond the health_probe wording.
2. Generating from the working tree pulls other units' uncommitted spec hunks (the ChallengeSignal hunk, committed later in ed45220b). Always export the spec with git show HEAD:... and record the commit it came from (scratchpad/F1-docs/sdk-head2/spec/SOURCE_COMMIT.txt).
3. HEAD moves while a job waits for the docker lock (about 15 min here). Re-export the spec right before a generator run that was queued for a long time; my first queued run used a stale export and was killed before it ran.
4. sdks/.gitignore keeps only docs/*.md of the TypeScript SDK (256 tracked pages now). src/, dist/ and the python client are generated and ignored.
5. 63 tracked pages named *-Aquatiq-Dev.md came in with the snapshot commit 7c75b3c3; nothing refers to them; the generator does not write them.
6. The Haiku trailer: the system attribution reminder asks for Claude Haiku 5.5 on commits; the workflow text asked for Sonnet 5.5. This agent used Haiku 5.5 (its own model identity).
7. The local ignored sdks/typescript/src and dist still reflect the first run (working-tree spec). generate.sh rewrites them.

## Left (not done, not ours or needs a decision)
- Decision: delete the 63 *-Aquatiq-Dev.md SDK pages (register row added; separate commit once the owner agrees).
- Not run: the python SDK regeneration into the tree (git-ignored output); a full SPA typecheck (its dependencies are not installed in this worktree); a green verevonv3-ci or quarry-v2-ci run (no push).
- Not verified here: the 429 rows for org_backlog_full and org_render_cap against a live refusal (the reason strings exist in code: handoff.rs, lease_pool.rs).
- Not inspected: docs/SOVEREIGN_SCRAPING_RELIABILITY.md (another unit's, uncommitted when this unit started; its status now not checked).

## Progress file
scratchpad mirror: C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/side-session-notes/local_6a0cd694-160f-4e6d-9b45-da7a2400e1ec/wf-F1-docs-progress.md

## Round 2 (review finding on cache_policy)
- Docs: dedf5f51 corrects the PROGRESS.md Tests snapshot row for quarry-edge cache_policy. The row said 4 cases with a green check; the file has 8, and each needs a live Redis at REDIS_URL. CI has no Redis, so the cases skip there.
- Checked and unchanged: CONTRACTS 9.1 (the gate, the option list, env vars, limits and error codes all match the code).
- Not fixed here, and why: the finding's core is that no CI test enforces the page-cache gate. The fix is in tests and CI, outside this unit's owned paths: a CI Redis service (quarry-v2-ci.yml has none) or an in-memory page-cache double (PageCache is a concrete Redis wrapper), Some(cache) in the route tests (routes.rs test_state_with_artifacts passes cache: None), and new cases for timeout_ms alone and max_inline_bytes alone. It is a decision for the owner of those files.
- Also not covered by any test: scrape.rs asserts is_default() only for defaults, formats, exclude_tags and remove_base64_images (lines 748, 762, 766, 870). include_tags, only_main_content false and a non-readability extractor have no such assertion.
- Trap: the WS5.D commit d08e41ae says 8/8 passed with a throwaway redis:7-alpine. That is true for that run; CI does not reproduce it.
- Trailer: Claude Haiku 5.5, as the system attribution reminder requires (the workflow text asked for Sonnet 5.5).
- Round 2 commits: dedf5f51 (row correction) and e4a67d44 (names the default Redis address). Nothing else changed.

## Round 3 (retry of the same finding)
- No new commits. The finding is accurate about CI: at HEAD e1889655 the page-cache gate has no CI coverage. The fix is in test and CI files that F4 scrape-leftovers is changing now (uncommitted cache_policy.rs, routes.rs, CONTRACTS.md, RUNBOOKS.md). F4 leaves quarry-v2-ci.yml unchanged. Not done here: a CI Redis service with REQUIRE_CONTRACT=1 (proposal in the progress file).
- Re-verified at HEAD: 8 cases, no Redis or REQUIRE_CONTRACT in quarry-v2-ci.yml. F4's uncommitted browser clause (CONTRACTS 9.1) matches driver_plan.rs and is_default. Its cache_state text matches routes.rs and openapi.yaml.
- Open: when F4 commits, PROGRESS row 216 must say 9 cases and that REQUIRE_CONTRACT=1 turns a missing Redis into a failure.
- Trap: cache_policy.rs and routes.rs carry F4's uncommitted hunks. Do not stage them from F1-docs.
