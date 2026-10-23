# F4-scrape-leftovers: progress

Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b (branch quarryv2-next).
Workspace: apps/Ingestion Plane/Quarry-v2. Scratch: scratchpad/F4-scrape-leftovers/ (logs in logs/).

## State at 13:16 (resume agent, after the 06:40 usage-limit cut)

Commits from this unit: none yet (history from 39386dc5..HEAD has no F4 commit).

Owned uncommitted files (all written by the first agent, verified by the background runs below, then edited by the resume agent):
- crates/quarry-edge/tests/cache_policy.rs (M): LoopbackDriver test double (the production StaticDriver refuses loopback origins; HEAD had 4 of 5 tests red for that reason, see logs/baseline-cache-policy.log), page key is quarry:page:{org}:{fp}, 3 new tests: non-default options neither read nor write, default still reads and writes, ZDR never reads or writes.
- crates/quarry-edge/tests/scrape_records_durable.rs (new, untracked): FilesystemStore on a temp dir, restart edge, foreign org 404, ZDR no id and no file. Passed 3/3.
- crates/quarry-runtime/tests/doc_parse_sandbox.rs (M, +59): docx bomb test. Passed. Resume agent pinned the reason to too_large (the 32 MiB cap in docx.rs fires before RLIMIT_AS).
- crates/quarry-transform/tests/clean_html_selectors.rs (new): 14 tests + 2 added by resume agent (unclosed block accepted; exclude html with include does not panic). The invalid-selector test had a wrong expectation ("li:nth-child(2" parses), replaced by "li:nth-child(two)".
- crates/quarry-transform/tests/clean_html_cost.rs (new): #[ignore] measurement, 5 MB page, 4 cases.
- crates/quarry-transform/tests/fixtures/clean_html/{cards,nested}.html (new).
- crates/quarry-transform/src/clean_html.rs (M, 1 line + comment, CRLF kept): BUG FIX. The include pass walked doc.root_element().descendants(); scraper's root_element() panics "html node missing" when exclude_tags detached html. Now walks doc.tree.root().descendants() (same elements when html is present).

Not ours (do not stage): quarry-edge/tests/support/zdr_harness.rs (M), quarry-edge/tests/agent_run_control.rs, run_sweeper_integration.rs, zdr_gap_rows.rs, support/agent_harness.rs (untracked), apps/Model Plane/rust/services/model-gateway/src/{grpc,quarry}.rs, .plan-work/.

## Results so far (previous agent's background runs, logs/run-A-cache-durable.log and run-B-transform-sandbox.log)
- cache_policy: 8/8 passed (with Redis, before the resume agent's edits)
- scrape_records_durable: 3/3 passed
- quarry-transform (before resume edits): lib 144/144, clean_html 16/16, clean_html_selectors 13/14 (the wrong expectation, fixed), clean_html_cost ignored
- doc_parse_sandbox: 7/7 passed

## Run C (13:16-13:27, logs/run-C-all.log): all green
- quarry-transform: lib 144, clean_html 16, clean_html_selectors 16, transform 11 (clean_html_cost ignored)
- doc_parse_sandbox 7/7; cache_policy 8/8; scrape_records_durable 3/3; quarry-edge --lib scrape 17/17
- MEASURE (debug profile, median ms): default 4963.5, exclude_tags 6321.1, include_tags 4865.5, only_main_content false 4930.8

## Mutation check (13:27-13:29, logs/run-D-mutant.log)
- pre-fix root walk put back: excluding_the_html test FAILS with "html node missing" (scraper mod.rs:113). Fix restored byte-identical (scratch clean_html.fixed.rs). Note: the first restore attempt duplicated a comment; fixed by copying the saved file back.

## Docs
- docs/RUNBOOKS.md: 'Scrape cleaning cost' appended at the end (13 lines). WARNING: RUNBOOKS.md ALSO carries another unit's uncommitted hunk (22 lines, WS3.F 'Browser capacity, crashes, the idle reaper' section). Do NOT commit the whole file: commit HEAD + my section only (selective-commit.sh with a constructed LF file).

## Final verification run E (13:30, logs/run-E-final.log)
- same commands after the comment-only edits (clean_html_selectors header, cache_policy guard path)

## Commits (13:33-13:36)
- baa7a009 fix(quarry-transform): exclusion of html no longer panics the include pass; selector tests + fixtures (clean_html.rs, clean_html_selectors.rs, fixtures)
- 5f90ae3a test(quarry-transform): ignored cost measurement (clean_html_cost.rs). Its message says the RUNBOOKS section is "in the next commit": WRONG, see below.
- d08e41ae test(quarry-edge): page cache policy (cache_policy.rs)
- c0aca0d3 test(quarry-edge): durable scrape record (scrape_records_durable.rs)
- d873cca4 test(quarry-runtime): DOCX bomb (doc_parse_sandbox.rs)

## RUNBOOKS incident
- The 'Scrape cleaning cost' section is in HEAD via 37b7ebb0 (WS3.F's commit, 13:29:48), which committed docs/RUNBOOKS.md whole (its 22 lines + my 13). The text is identical to the one I wrote (checked). No separate commit for it; the history cannot be rewritten (amend/rebase forbidden). Reported in the final answer.

## Final state
- My owned paths: clean (git status empty). Nothing staged by me remains.
- Not ours, left untouched: quarry-edge/tests/support/zdr_harness.rs (M), agent_run_control.rs, run_sweeper_integration.rs, zdr_gap_rows.rs, support/agent_harness.rs (other units).

## Next
1. Final StructuredOutput: notes_file = this file (no separate report file, per the rule against report .md files).
2. Attribution: the system reminder says Claude Haiku 5.5, the unit text says Sonnet 5.5. Used Haiku 5.5 (the model this session runs as), and said so in the final answer.

## Traps
- redis-test.sh (first agent) used rust:1 and apt-get per run; redis-run.sh uses quarry-rust-env:1 instead.
- Docker lock dir: scratchpad/locks/docker.d (stale after 45 min is taken over).
- The first agent's usage limit hit at 06:40; its background jobs still completed and wrote logs.

## Round 2 (review fixes), resume agent
Findings: (1) RUNBOOKS claim in 5f90ae3a: correction appended, history not rewritten. (2) cache_policy Redis skips not gated: REQUIRE_CONTRACT gate added (as zdr_claim_floor.rs; quarry-v2-ci.yml sets neither Redis nor the variable, so CI still skips; stated in RUNBOOKS). (3) scrape_records_durable had no same-organization ZDR case: added. (4) cache gate read the page cache before choosing the driver: browser-driver signals (screenshot, pdf, prior_block_signals>=2, profile_required) answered from a static cached result. Fix in routes.rs (cache_allowed now needs plan.driver != Browser), test browser_signals_neither_read_nor_write_the_page_cache, one clause in docs/CONTRACTS.md 9.1 (outside owned paths, exact-anchor edit).
- Applied (uncommitted): cache_policy.rs (gate, remove_base64_images case, browser test), scrape_records_durable.rs (same-org ZDR test). Edit script: scratchpad/F4-scrape-leftovers/r2-edit-a.js.
- Run R2-1 (pre-fix, Redis + no-Redis gate check): logs/run-R2-1-pre.log. Expected: browser test FAILS (planted entry served), other cache tests pass, durable 4/4, gate on = failures, gate off = pass.
- Run R2-1 RESULT (logs/run-R2-1-pre.log): cache_policy 8 passed, 1 failed (browser_signals... panicked at cache_policy.rs:647, "the planted entry was read": the bug reproduced before the fix); durable 4/4 (incl. the new same-org test); REQUIRE_CONTRACT=1 with unreachable Redis: rc=101 (every case panics with the message); unset: 9 passed (skips), rc=0.
- Applied routes.rs gate fix (plan made before the cache gate; cache_allowed also needs plan.driver != Browser) and the CONTRACTS.md 9.1 clause (r2-edit-b.js). Uncommitted.
- Run R2-2 (post-fix, Redis): cache_policy, scrape_records_durable, zdr_claim_floor, quarry-edge --lib scrape: logs/run-R2-2-post.log.
- Run R2-2 RESULT (logs/run-R2-2-post.log): cache_policy 9/9 (browser_signals now passes), scrape_records_durable 4/4, zdr_claim_floor 22/22 (with Redis), quarry-edge --lib scrape 17/17.
- Applied: cache_policy.rs comment wording (no code change), RUNBOOKS section appended (r2-edit-c.js, 8 added lines, 0 removed).
- Run R2-3 (final, logs/run-R2-3-final.log): cache_policy 9/9 with Redis; REDIS_URL unreachable + REQUIRE_CONTRACT=1: 0 passed, 9 failed, rc 101; unset: 9 passed with 9 "skip: REDIS_URL unreachable" lines, rc 0.
- Commits (round 2): 0076d850 fix(quarry-edge) routes.rs + cache_policy.rs; 6cf2a7c7 docs CONTRACTS 9.1 clause; 6324640d test durable same-org ZDR; c2b3d2e9 docs RUNBOOKS correction + section. Attribution: Claude Haiku 5.5 (the model this session runs as; the workflow text said Sonnet 5.5).

## Round 2 final state and a commit mistake
- c2b3d2e9 is MIS-DESCRIBED. I ran qcommit.sh on docs/RUNBOOKS.md without a git diff right before it (my check was ~12 min earlier). P1-ws3f had appended more edits to the same file in the meantime: the QUARRY_BROWSER_REAPER row, two sentences in the "Keep both reaper limits" paragraph and one clause in the "Crash" paragraph (all in "Browser capacity, crashes, the idle reaper and /ready (WS3.F)"). They are in c2b3d2e9 now. The commit message says "8 added lines and 0 removed" and "nothing above it changed": both are false for that commit (stat: 11 insertions, 3 deletions). Not amended (forbidden), not reverted (that is another unit's work). P1-ws3f must be told; their notes (wf-P1-ws3f.md) should record that their hunk is in c2b3d2e9. My own RUNBOOKS hunk (the 'Page cache bypass' section and the correction) is also in that commit.
- The other 4 commits were checked with git diff before commit: each file held only my hunks (routes.rs 8 lines, cache_policy.rs 122, CONTRACTS.md 1 line, scrape_records_durable.rs 42).
- The index holds two files staged by another unit (crates/quarry-edge/tests/agent_run_control.rs, tests/support/agent_harness.rs). Path-limited commits left them staged and did not commit them.
- Traps: qcommit.sh stages and commits WHOLE files. Run git diff on each file immediately before qcommit, every time.
- Not done: CI does not run these cache tests with Redis (quarry-v2-ci.yml untouched: outside owned paths). The minimum in the finding (say so in the commit and RUNBOOKS) is done. The cleaning-cost figures were not measured again in round 2.
- Outside owned paths (exact-anchor edits, said so in the commit messages): routes.rs cache gate (shared hot file), docs/CONTRACTS.md 9.1 (one sentence).

## Round 3 (second review: two medium findings), resume agent
Findings: (1) c2b3d2e9 is mis-described (11 insertions and 3 deletions, not 8 and 0) and carries three WS3.F browser lines (QUARRY_BROWSER_REAPER row, "Keep both reaper limits", "Crash") that describe the uncommitted chromiumoxide.rs/slots.rs code. History cannot be rewritten (no amend, rebase or reset), so the record is corrected in place. (2) The browser-signals cache test has no non-empty `actions` case.
- Fix 1 = 329d2b98 docs(quarry): an append-only section at the end of docs/RUNBOOKS.md, "Correction to c2b3d2e9 (WS5.D): three browser lines it carries" (4 added lines, 0 removed). It names the three lines and what HEAD does: start_background returns early for REAPER=0 (HEAD has no prober then), and a dropped acquire keeps its place (HEAD's acquire releases only on the Err branch of ensure_browser). The WS3.F browser code must land in the same commit as those lines.
- Fix 2 = dda2618c test(quarry-edge): ("actions", driver_signals({"actions": ["click"]})) in browser_signals_neither_read_nor_write_the_page_cache; the doc comment names the five signals plan_from_signals routes to the browser (4 added lines, 0 removed).
- Run G1 (logs/run-G1-cache-durable.log; redis:7-alpine via redis-run.sh under the docker lock; REQUIRE_CONTRACT=1 with Redis reachable): cache_policy 9 passed, 0 failed; scrape_records_durable 4 passed, 0 failed; no skip lines; rc 0.
- Not done: no mutation run for the actions case (removing the branch would edit quarry-runtime/src/driver_plan.rs, not owned); the failure claim is from the code path. Nothing was sent to the WS3.F owner: their notes (wf-P1-ws3f*.md) should say their three RUNBOOKS lines went in with 329d2b98 and that their code is still uncommitted.
- Staged by me: nothing. My owned paths are clean in git status. Untouched, other units' uncommitted work: quarry-browser/src/{chromiumoxide,slots}.rs, gateway files, Model Plane grpc.rs and quarry.rs, .plan-work/.
- Attribution: Claude Haiku 5.5 (system reminder; the workflow text said Sonnet 5.5). Same as rounds 1 and 2.
- Notes file for the final answer: this progress file (no separate report file, per the rule against report .md files).
