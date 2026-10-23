# F1-docs progress (WS5.H leftovers: stale docs and SDK regeneration)

Resumed 2026-10-18 13:12 (Oslo). Previous agent was cut off by the usage limit after its doc commits.
Final state written 2026-10-18 about 13:50. Hand-over: wf-F1-docs.md (same folder).

## Committed by this agent (quarryv2-next, Co-Authored-By Haiku 5.5)
- 12ab61d3 docs(quarry): 9.1 says what an include_tags selection is cleaned like (CONTRACTS.md; qcommit)
- 38c65716 docs(quarry): default parser isolation is process for the edge binary only (GAP.md, OSS_PARITY_BACKLOG.md; qcommit)
- 3f590d6c docs(quarry): health_probe is an op in use (ERROR_REASONS.md; hunk-only commit)
- b964fe92 docs(quarry): non-Unix parser mode is inprocess (RUNBOOKS.md; hunk-only commit)
- f292d5bb docs: register this round of WS5.H corrections (apps/STALE_DOC_DELETION_REGISTER.md; qcommit)
- 97186586 chore(quarry): regenerate the TypeScript SDK docs from the committed openapi.yaml (WS5.H; 21 pages; qcommit)
- 469e7c7d docs: register the 63 legacy -Aquatiq-Dev SDK pages (register only; nothing deleted; qcommit)

## Committed by the previous agent (re-verified by this agent by reading code)
- b807f572 parity docs, 440579d6 CONTRACTS 9/9.1, f7a94512 RUNBOOKS, cee56eb5 ERROR_REASONS, aec85098 register rows.

## Still uncommitted, NOT ours (other unit, WS3.F): leave alone
- docs/ERROR_REASONS.md: 429 driver caps, 502 session lost, 503 /ready sections.
- docs/RUNBOOKS.md: "Browser capacity, crashes, the idle reaper and /ready (WS3.F)" section.
- docs/ZDR_MATRIX.md (generated), docs/SOVEREIGN_SCRAPING_RELIABILITY.md (not inspected in depth).

## SDK (TypeScript only, generate.sh's TypeScript step)
- Spec: HEAD openapi.yaml at d26c61a5 (blob 3d9312c1; unchanged on HEAD since). Export: scratchpad/F1-docs/sdk-head2/spec.
- Two runs identical (419 files each). Build in scratch (pnpm install --ignore-scripts; pnpm run build): exit 0, 414 .d.ts.
- SPA import names check (tsc --strict): exit 0; negative control with a missing name fails (TS2305).
- Committed 21 pages (11 changed, 10 new). 63 legacy -Aquatiq-Dev pages left in place (register row added).

## Verified facts (by reading code)
- SPA imports only ChangeTrackingApi, ResourcesApi, SchedulesApi, TeamApi, Configuration, ListJobsByKindKindEnum and 12 model types (apps/Frontend Plane/verevonv3/src/shared/api/quarry-client.ts).
- sdks/.gitignore keeps only docs/*.md of sdks/typescript (256 tracked before the regeneration; 1 new doc from WS3.H).

## Open
- Decide on deleting the 63 legacy -Aquatiq-Dev pages (separate commit).
- The Haiku trailer: the system attribution reminder says Claude Haiku 5.5; the workflow text asked for Sonnet 5.5. This agent used Haiku 5.5.

## Round 2 (review finding: quarry-edge cache_policy gate, medium)
- Committed: dedf5f51 docs(quarry): PROGRESS.md Tests snapshot row for cache_policy (4 -> 8 cases; says it needs REDIS_URL and that CI runs none of its assertions). qcommit; only that path.
- Verified by reading: scrape.rs is_default (613-620) excludes timeout_ms and max_inline_bytes; routes.rs cache_allowed (566-567) with the ZDR guard; CONTRACTS 9.1 rule matches. 9.1 unchanged.
- Verified the finding's facts: cache_policy.rs skips without Redis (try_redis 33-39, build_state 96-108, 8 skip branches); quarry-v2-ci.yml has no services block and no redis reference, cargo test --workspace at line 205; routes.rs test_state (1554) -> test_state_with_artifacts with cache None (1592); assertions bypass at 1884 and 1906.
- Run: qtest, quarry-rust-env:1, no Redis, cargo test -p quarry-edge --test cache_policy -- --nocapture: 8 skip lines, test result ok 8 passed, QTEST rc=0 (log: scratchpad/F1-docs/cache-policy-run.log).
- NOT DONE (outside owned paths, not a small edit): the gate is not enforced by any CI test. Needs a CI Redis service or an in-memory page-cache double, Some(cache) in the route tests, and cases for timeout_ms alone and max_inline_bytes alone. Files: crates/quarry-edge/tests/cache_policy.rs, src/routes.rs, .github/workflows/quarry-v2-ci.yml.
- Index: another agent's staged files (agent_run_control.rs, agent_harness.rs) were left staged and untouched.
- Follow-up committed: e4a67d44 names the default Redis address (127.0.0.1:6379) in the same row. Docs-only; no code or test changed in round 2.
- Final state: 2 commits this round (dedf5f51, e4a67d44). Finding status: partially fixed (doc overclaim corrected). Test/CI enforcement still open (see NOT DONE above).

## Round 3 (retry of the round-2 finding, 2026-10-18 about 14:20 Oslo; HEAD e1889655)
- Finding: "None from the fix ... the gate still has no CI coverage" (medium). Real, but the CI part is not F1-docs's to change. The docs from dedf5f51 and e4a67d44 are accurate at HEAD.
- Checked at HEAD: PROGRESS row 216 says 8 cases; `git show HEAD:.../cache_policy.rs` has 8 `#[tokio::test]`. `git show HEAD:.github/workflows/quarry-v2-ci.yml` has no redis, REQUIRE_CONTRACT or services. HEAD build_state (cache_policy.rs) has no REQUIRE_CONTRACT assert, so cases skip and pass without Redis. CROSS_PLANE_INTEGRATION.md line 105 is accurate. The CI file's last commit is bee604d4, which added steps but no Redis.
- Owner of the gate's test side: F4 scrape-leftovers (its notes, lines 11, 65-71). Its uncommitted work is cache_policy.rs (9 tests in the working tree, REQUIRE_CONTRACT assert in build_state, remove_base64_images case, browser-signals test), routes.rs (cache_allowed also needs plan.driver != Browser), docs/CONTRACTS.md (9.1 browser clause) and a new RUNBOOKS section. F4 writes "quarry-v2-ci.yml sets neither Redis nor the variable, so CI still skips". cache_policy.rs and routes.rs have another unit's hunks: do NOT commit them from F1-docs.
- Checked F4's uncommitted text in my owned docs: the browser clause matches driver_plan.rs plan_from_signals (37-41: actions, screenshot, pdf, prior_block_signals >= 2, profile_required) and is_default (scrape.rs 613-620). RUNBOOKS cache_state (hit, miss, bypass) matches routes.rs 1906 and openapi.yaml 4949. No edit needed.
- Decision: no code, test or CI change and no new commit in this round. Reason: owned paths are docs only; the CI change would race F4's in-flight gate commit and cannot be run in GitHub Actions here.
- Open after F4 commits: PROGRESS row 216 must say 9 cases and that REQUIRE_CONTRACT=1 turns a missing Redis into a failure. Re-check HEAD before editing (`git log -1 -- cache_policy.rs`; `git show HEAD:...cache_policy.rs | grep -c "#\[tokio::test\]"`).
- Proposed CI change for the owner (not applied, not run): a redis:7-alpine service on host port 6380 for cargo-check, and a step after "Test workspace" with env REDIS_URL=redis://127.0.0.1:6380 and REQUIRE_CONTRACT=1 running `cargo test -p quarry-edge --test cache_policy --test zdr_claim_floor`. Port 6380 keeps the workspace step's default 127.0.0.1:6379 empty, so that step is unchanged. Also update the cache_policy.rs header and the zdr_claim_floor.rs comment (1019-1023) that say CI has no Redis.
- Verification this round: reads and git commands only (above). No test run, because nothing changed. Test evidence from round 2 (dedf5f51): 8 skip lines, 8 passed, rc 0 without Redis.
