# D2-ws3i (WS3.I): what was done, what is left, traps

Q = apps/Ingestion Plane/Quarry-v2. Design note: D2-ws3i-design.md (followed; deviations below).

## Commits (quarryv2-next, local)
1. 61906c93 feat(quarry-runtime): escalation.rs + `QUARRY_BLOCK_ESCALATION=legacy|honest` in fetch.rs, egress_broker.rs, fallback_driver.rs, driver_plan.rs; openapi `prior_block_signals` deprecated; default legacy.
2. 64346fd1 feat(quarry-control): preset `stealth` -> `careful` (Block challenge/abort), `stealth` kept as alias (`alias_of`, `deprecated`, Deprecation + Link headers on GET).
3. 83e920f3 docs: RUNBOOKS "Block escalation", POLICY, ARCHITECTURE, GAP, PROGRESS, ROADMAP, CONTRACTS, OSS_PARITY_BACKLOG, PARITY_IMPLEMENTATION_PLAN, SOVEREIGN_SCRAPING_RELIABILITY.
4. (last) default flip to honest: escalation.rs BUILD_DEFAULT + gate test, cache_policy.rs case made mode-aware, RUNBOOKS/PROGRESS wording. See `git log` for the hash.

## Deviations from the design note
- Design said `trigger_for_error(code)` maps UpstreamBlocked and RateLimited to a block trigger; done. The static driver also reports a connect failure as UpstreamBlocked, so under honest a connect failure no longer falls through to the TLS driver (documented in RUNBOOKS as the known limit). The same failure still rotates between proxies (inside StaticDriver).
- `blocked_error` message differs in honest ("egress identity answered with a block status and was not rotated; status=N"); codes unchanged.
- openapi: description rewritten and `deprecated: true` added; sdks/generate.sh (docker, MSYS_NO_PATHCONV=1 needed on this host) regenerated: `git status --porcelain -- sdks` stayed clean (the committed TS docs carry no field descriptions).
- Docs: POLICY.md, OSS_PARITY_BACKLOG.md, PARITY_IMPLEMENTATION_PLAN.md, SOVEREIGN_SCRAPING_RELIABILITY.md also edited (they described rotation on a block), beyond the plan's list.

## Not done / left
- No live run (rebuilt edge, two-entry QUARRY_PROXY_POOL, public 403 origin) and NO before/after measurement of rescued pages: both need a deployed edge and live origins. The flip rests on tests only; the commit message and RUNBOOKS say so.
- `tests/cache_policy.rs` needs Redis; without it the tests skip (all my runs skipped them). The case change is compiled but not executed here; CI runs it with Redis.
- Alias removal one release later (`deprecatedPresetAliases` in presets.go): add to the release checklist.
- Pre-existing, reported, not fixed: `attempt_record` in fetch.rs puts `proxy_uri` and the raw error text into `details.egress_attempts` (credential leak for a pool URI with userinfo). The host scheduler records a direct-egress block response as a good fetch (pipeline.rs record_good). `polite` (DefaultRunPolicy) still has Block {challenge, escalate} with no reader.
- GAP.md 336-337 (UA rotation claim) left untouched, unverified.

## Traps
- Long heredocs fail in the Bash tool: write scripts with Write, run with node.
- rustfmt --check on cache_policy.rs, fetch.rs shows pre-existing diffs at HEAD; only my own hunks were formatted.
- Other units' uncommitted edits broke `cargo test -p quarry-edge --lib` and `cargo check --workspace --all-targets` for one window (schedule_routes.rs `zdr`), and quarry-core `zdr_matrix` golden drift; both cleared/outside my files, see the verification entries in the final answer.
- qtest runs queue behind a global lock; waits of 10-20 minutes are normal.

## Errata (commit hashes)
- The RUNBOOKS.md hunks of the default flip (default cell `honest`, "Where the default stands", "Page cache" paragraph) were committed by another unit's `git add docs/RUNBOOKS.md` in efebf3e0 (WS4.H), BEFORE the flip commit 89097598. So the flip commit's message lists RUNBOOKS but its diff does not contain it, and between efebf3e0 and 89097598 RUNBOOKS says "honest" while BUILD_DEFAULT is still legacy. The tree at 89097598 is consistent. Cannot be amended (forbidden).
- Commit 61906c93's message was written before the final rustfmt-only edits; the code in the commit is already formatted (formatting was done before committing).

## Round 4 review fixes (2026-10-21)
Commits: 654806c2 (quarry-runtime: challenge gate + current() child tests), 8cf01450 (quarry-control: careful description advisory), 8566f2c1 (model-gateway browser() -> profile_required; RUNBOOKS hunks only).
- F1 (prior_block_signals callers): real. model-gateway moved to profile_required (4 wire pins updated, 56 quarry tests pass). verevonv3 gateway fetch_listing_markdown (`prior_block_signals: 1`) NOT changed: documented in RUNBOOKS as a known regression (TLS retry now repeats the static fetch).
- F2 real: child-process tests; unset -> honest, legacy/" LEGACY " -> legacy, blank/typo -> honest.
- F3 real: challenge_forbids_escalation in FallbackDriver; honest keeps a 200 strong-challenge body, legacy unchanged.
- F4 real: description fixed (advisory) rather than wiring Block.action (not done).
Trap: model-gateway tests need QROOT=apps/Model Plane (proto/ is a sibling of rust/), CARGO_TARGET_DIR=/src/target, and libprotobuf-dev (apt-get inside the throwaway container; the image lacks google/protobuf/*.proto).
Trap: the shared RUNBOOKS.md carries another unit's uncommitted WS4.H hunks; staged mine with git apply --cached of a filtered patch.

## Round 5 review fixes (2026-10-21)
Commit 39b0dec3 (quarry-runtime fallback_driver.rs, quarry-edge cache_policy.rs, RUNBOOKS).
- Finding 6 (medium, real): the "unfollowed redirect / thinner than the shell" branch of FallbackDriver handed a block status or a 200 strong challenge from Tls to the browser under honest. Reproduced by the new three-driver test (fails without the fix, mutation-checked). Fixed by a guard in that branch. Static [Static, Tls, Browser] chain was the trigger.
- Finding 3 (weak class untested): weak/non-signal shell test added; the review's C12 mutation now fails it.
- Finding 4 (cache honest case): new honest test in cache_policy.rs; executed against redis-server installed in the throwaway container (REQUIRE_CONTRACT=1): 10 passed in honest and legacy.
- Finding 5 (connect failure): behaviour kept (fail closed) and pinned by a real StaticDriver + dead proxy test; RUNBOOKS names it.
- Findings 1 and 2 (commit message counts, RUNBOOKS attribution): no code change possible; errata in the commit message of 39b0dec3: 8cf01450 has 7 TestPresets_ tests (not 8), 89097598's RUNBOOKS hunks are in efebf3e0.
Trap: Docker answered "not answering" for ~20 min in this round, then recovered; native Windows cargo cannot build quarry-tls (no libclang for boring-sys2).

## Round 6 (2026-10-21): verifier's two medium notes
- Note 1 (honest returns the TLS block/challenge, drops best_shell): design decision, matches the RUNBOOKS contract; no code change. If the contract changes, RUNBOOKS and the fallback_driver tests must change together.
- Note 2 (unverified suites): re-ran at HEAD 39b0dec3: quarry-runtime full 775 lib + integration ok (doc_parse_sandbox 10, dom_summary_proptest 4, integration_runtime 3, publisher_integration 1); quarry-edge --test log_field_discipline 22 ok, --test cache_policy 10 ok (a Redis was available this time); cargo check --workspace --all-targets finished (warnings only). No commit needed.

## 2026-10-23 re-run
No change. Image quarry-rust-env:1 missing locally, so Rust was not re-run.

## Errata 2 (2026-10-23, review of round 5)
- Commit 89097598 says that a passing honest run "showed that only the cache_policy case needed to change", and 61906c93 reports "cache_policy 9 passed". Both are wrong as evidence: the cache_policy cases skip without Redis (build_state() returns None, the test prints a skip and returns, libtest counts it as passed), and none of the runs behind those two messages had Redis. They showed nothing about the cache cases. The honest cache case was first executed at 39b0dec3, against a redis-server installed in the throwaway container (REQUIRE_CONTRACT=1, 10 passed in honest and legacy). The two messages cannot be amended; this entry and the message of the round-6 commit record the correction.

## Round 6 (2026-10-23): review findings fixed
- F1 (medium, real): under honest, a 200 JS shell held in best_shell was dropped when a later driver answered a block status or a strong challenge, while a later driver ERROR returned the shell. Now `FallbackDriver::refuses_rotation` (block status refused by the mode, or strong challenge) with a shell held returns the shell, browser not called. Two honest tests changed (block and strong challenge from TLS after a shell now assert the shell, served_by Static, browser calls 0); legacy tests unchanged. RUNBOOKS rows updated.
- F2 (medium, real, text only): errata above.
- F3 (medium, real): model-gateway scrape_readable escalated a 200-envelope page whose data.status was a block status. Now returns it as it is (is_block_status copy in quarry.rs, comment says to keep it in step with quarry_runtime). New test readable_scrape_does_not_escalate_a_block_status_page (loops 401/403/407/429/503, browser body mounted with expect(0)); doc comment and RUNBOOKS corrected. A 200 interstitial cannot be told apart by the client (the scrape output has no challenge marker): documented.
- Trap: image quarry-rust-env:1, rust:1 and the cargo/target volumes were gone from the local docker; re-pulled rust:1 and rebuilt the image from scratchpad/tools/qenv/Dockerfile.

## Round 7 (review of cfd8b57b), commit 181a2b3c
- Documented: shell returned after a TLS 403/429 shows the caller a 200, so autoscale record_ok and the host scheduler do not slow down; only the warn log marks it. Same trade-off as the Err arm. Not built: a status-carrying signal.
- RUNBOOKS block-status row no longer contradicts itself. Comment line re-wrapped.
- Trap: model-gateway tests cannot run in qtest (protoc include path / proto dir not mounted); run natively.
- RUNBOOKS also holds another unit's uncommitted hunk (WS6.A/WS5/WS2.E round 4); only my hunk was committed (git apply --cached of a filtered patch).
