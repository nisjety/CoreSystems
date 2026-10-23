# wf-R8a-ci progress (unit R8a-ci: CI Redis gate and stale PROGRESS rows)

Scratch: scratchpad/R8a-ci/ (ci-head.yml = HEAD copy, ci-mine.yml = HEAD + my two hunks, logs/).

## State (2026-10-18)
- Design (follows F1's proposal in wf-F1-docs-progress.md:54, port 6380 so the workspace step's default 127.0.0.1:6379 stays unused):
  cargo-check job gets services.redis (redis:7-alpine, 6380:6379, healthcheck) and a step after "Test workspace",
  "Page-cache and ZDR cache gates (Redis)", env REDIS_URL=redis://127.0.0.1:6380 + REQUIRE_CONTRACT=1,
  run: cargo test -p quarry-edge --test cache_policy --test zdr_claim_floor.
- YAML parses (pyyaml in scratch venv; duplicate-key check); HEAD 12 steps, mine 13.
- NOT committed yet. The working-tree quarry-v2-ci.yml also carries ANOTHER unit's two uncommitted steps
  ("Test challenge signal schema golden", "OpenAPI challenge parity"). Do NOT commit them. Commit path is:
  build blob from HEAD + my hunks (ci-mine.yml), git hash-object -w --path, git update-index --cacheinfo, git commit (under git lock).
- Docker proof queued: scratchpad/R8a-ci/r8a-proof.sh (actionlint head+mine, P1 redis gate on, P2 no redis gate on, P3 no redis gate off).
- Attribution: the session reminder says Co-Authored-By Claude Haiku 5.5; the workflow text says Sonnet 5.5. Using Haiku 5.5 (the model I run as); flag to owner.

## Next
1. Run proof under docker lock (logs/proof.log, end marker R8A_PROOF_DONE).
2. Commit CI (step 1) with the proof numbers in the message.
3. Docs (step 2): PROGRESS.md Tests snapshot (cache_policy row, new binaries, zdr_claim_floor, gateway lint, total note),
   RUNBOOKS.md append-only correction (line 340 says CI has no Redis), CROSS_PLANE_INTEGRATION.md:105 exact-anchor,
   cache_policy.rs header comment lines 6-9 and zdr_claim_floor.rs comment 1019-1023 (comment-only, exact anchors).
4. Full counts: cargo test -p quarry-edge --no-fail-fast (no Redis env), quarry-transform clean_html_selectors/clean_html_cost,
   gateway tests/log_field_discipline natively.

## Milestone 1 done (2026-10-18 ~19:52)
- COMMITTED 7fffb2ac ci(quarry-v2): run the page-cache and ZDR cache gates against a Redis service (workflow only, +26 lines).
  Committed via scratchpad/R8a-ci/commit-step1.sh (blob built from HEAD 6a5016ad + our hunks; index entry set for that path only).
  The working copy still holds another unit's two steps (Test challenge signal schema golden, OpenAPI challenge parity): NOT ours.
- Proof (scratchpad/R8a-ci/logs/): p1-redis-gate-on.log: cache_policy 9 passed, zdr_claim_floor 22 passed, 0 skip, rc 0.
  p2-noredis-gate-on.log: rc 101; cache_policy 0/9 (9 failed), zdr_claim_floor 21 passed 1 failed (Redis case).
  p3-noredis-gate-off.log: rc 0; cache_policy 9 passed with 9 skip lines; zdr_claim_floor 22 passed, 1 skip line.
  actionlint (lint.log / actionlint-*.log, proof2.log): HEAD copy rc 0, candidate rc 0, negative control rc 1 (2 errors).
- Gateway lint (native, logs/gateway-lint.log): 13 passed, 0 failed (GWLINT_DONE rc=0).
- Comment-only edits (UNCOMMITTED, step 2): cache_policy.rs header 7-9, zdr_claim_floor.rs 1021-1023; RUNBOOKS.md appended section (+4); CROSS_PLANE_INTEGRATION.md line 105 sentence.
- Queued: scratchpad/R8a-ci/r8a-counts.sh -> logs/quarry-edge-full.log and logs/quarry-transform-clean-html.log (end marker R8A_COUNTS_DONE in logs/counts.log).
- Next: PROGRESS.md rows from the counts run, then commit step 2 (path-limited; check git diff of each file first).

## Round 3 (started 2026-10-19 ~09:15, after the round-2 cut-off)
- Step 1 is COMMITTED as 7fffb2ac (ancestor of HEAD). Do not redo it.
- The round-2 counts (logs/quarry-edge-full.log, 2026-10-18 20:01) are STALE: other units committed quarry-edge and
  quarry-transform changes at 2026-10-19 09:06-09:13 (9a2bc002, 486bbd28, a6049bc2, e50046a9, 4c4dd4d2, a4447aff,
  217fc548, 14eb0367). The gateway lint (logs/gateway-lint.log, 19:07) is also older than gateway commits 62f1b66a..44450389.
- Re-run on the CURRENT tree: scratchpad/R8a-ci/r8a-counts2.sh under the docker lock -> logs/counts2.log (end marker R8A_COUNTS2_DONE),
  per-run logs logs/r3-*.log. Gateway lint natively -> logs/gateway-lint2.log (end marker GWLINT2_DONE).
- Step 2 (NOT committed yet): PROGRESS.md (whole file, only our hunks), CROSS_PLANE_INTEGRATION.md line 105 (only ours),
  cache_policy.rs and zdr_claim_floor.rs comment hunks (only ours). RUNBOOKS.md: commit ONLY our appended section, via an
  index blob = HEAD + our section (the working file also has WS3.H and R7 sections of other units: leave them uncommitted).
- Pending small fix: our RUNBOOKS section says "The paragraph above" but the stale paragraph is line 340 under the heading
  "Page cache bypass and the Redis-gated cache tests (WS5.D)"; refer to it by heading.
- Round-3 milestone 2 COMMITTED as 31810d46 (docs + comments; 5 files; RUNBOOKS only our section via index blob).
  Committed via scratchpad/R8a-ci/commit-step2-v2.sh (msg-step2.txt). Gitleaks hook: no leaks found.
- Round-3 runs on HEAD 23e1ad7f (+ comment-only edits; ab8a5b99 after it touches sdks docs only), logs/r3-*.log, end marker R8A_COUNTS2_DONE:
  quarry-edge no env: 902 passed, 0 failed (lib 362, bin 356, 21 integration targets = 184); cache_policy 9, zdr_claim_floor 22.
  clean_html_selectors 16 passed; clean_html_cost 0 passed, 1 ignored (measurement).
  P1 Redis 6380 + REQUIRE_CONTRACT=1: cache_policy 9, zdr_claim_floor 22, rc 0. P2 no Redis + flag: cache 0/9 failed (cache_policy.rs:104), zdr 21 pass 1 fail (zdr_claim_floor.rs:1036), rc 101.
  Gateway lint native (logs/gateway-lint2.log, GWLINT2_DONE rc=0): 13 passed.
- Remaining: actionlint on HEAD workflow (logs/actionlint-now.log, marker ACTIONLINT_NOW_rc). Then final report.
- Traps: working copy EOL is CRLF but blobs are LF (autocrlf=true); HEAD moves under you (other units commit every few minutes); the
  scratch S variable in bash is POSIX (/tmp/...) so node needs the Windows form (SW) in scripts.

## Round 3 continuation (2026-10-19, reviewer finding: ZDR probe key wrong)
- Commits 7fffb2ac and 31810d46 are on HEAD (do not redo). Reviewer medium finding: zdr_claim_floor.rs Redis case probed
  quarry:page:{fp} (no org) and its cleanup deleted quarry:page:{cached_fp}; the write-side check could never fail.
- UNCOMMITTED (ours): zdr_claim_floor.rs tail edit (scratchpad/R8a-ci/zdr-probe-edit.js): positive control
  page_cache.get(standard_org, cached_fp) is_some, then get(zdr_org, zdr_fp) is_none via public PageCache::get; cleanup DELs
  both org-keyed keys. Candidate copy: scratchpad/R8a-ci/zdr_claim_floor.candidate.rs.
- Proof script: scratchpad/R8a-ci/r8a-zdr-proof.sh (P1 redis+flag with key dump, P2 no redis+flag, P3 no redis no flag),
  log logs/zdr-proof.log, end marker R8A_ZDR_PROOF_DONE. Results: (pending)
- Optional mutant probe (temporary edit, must be reverted and cmp'd against the candidate copy before any commit).
- Round-3 continuation RESULTS (2026-10-19 ~11:30): COMMITTED fd049304 (zdr_claim_floor.rs only, 19+/6-, gitleaks: no leaks).
  scratchpad/R8a-ci/logs/zdr-proof.log (R8A_ZDR_PROOF_DONE): P1 redis 6380 + REQUIRE_CONTRACT=1 rc=0, cache_policy 9 passed,
  zdr_claim_floor 22 passed, keys left in redis = 0; P2 no redis + flag rc=101 (cache_policy 9 failed, zdr 21 pass / 1 fail at
  zdr_claim_floor.rs:1036); P3 no redis no flag rc=0 (9 + 22 passed, skip lines).
  logs/zdr-mutant.log (R8A_MUTANT_DONE): P5 fixed probe + planted ZDR-org entry rc=101 (fails on the ZDR write assertion);
  P6 pre-fix probe + same entry rc=0 (blind). Mutant block reverted; working copy cmp == candidate.
- Not ours, untouched: run_sweeper_integration.rs, start_run_grants.rs (modified by others), RUNBOOKS WS3.H/R7 sections.
- Left: no hand-over .md written (system rule: no report files); this file is the hand-over. Attribution: Haiku 5.5 trailer used.
- FINAL re-run on the committed tree (logs/zdr-proof-final.log, R8A_ZDR_PROOF_DONE, HEAD 43b15b46 descends from fd049304):
  P1 rc=0 (cache_policy 9, zdr 22 passed, redis keys left 0); P2 rc=101 (cache 9 failed, zdr 21/1 at zdr_claim_floor.rs:1036);
  P3 rc=0 (9 + 22 passed, skips). Working tree also held other units' uncommitted quarry-runtime hunks (challenge.rs,
  observation.rs, pipeline.rs) during that run; they compiled and the tests passed.
- fd049304's message says "HEAD 9867a8be plus this change"; its parent is 3aac8a8a (another unit landed it 11:30). Cannot amend.
- Left for the owner: attribution (session reminder says Haiku 5.5, the computed task says Sonnet 5.5; Haiku used, as in 7fffb2ac and 31810d46).
