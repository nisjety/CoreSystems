# D-B1-crawl-robots progress (batch 1, track W2-green)

Unit: crawl scope and robots failures (WS14.S2, S4, S6) plus the observe one-page crawl bug (D2-029).
Worktree: `C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`, local commits only.
Scratch: `C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/D-B1-crawl-robots/`.
No continuation file existed at start (checked for `wf-D-B1*`, `D-D-B1*`).

## Root causes found by reading (before any run)

1. All 13 `crawl_scope_robots` tests that call `spawn_edge`/`spawn_gated_edge` panic at setup: the test secret
   `crawl-scope-internal-secret` is 27 bytes and `InternalSigner::new` refuses fewer than 32 (internal_auth.rs:82-88).
   The two `map_*` tests in that file do not build a signer and are not in the failing list.
2. D2-029: `CompiledScope::evaluate` sets `scope_applied` only in enforce (crawl_filter.rs:265). The orchestrator
   ends every crawl whose page report lacks it (`appliedBy`, crawl_scope.go:108, workflows.go:674 and 831), so the
   default observe mode stops after one page. Go is not changed in this batch; the edge reports the proof in both modes.
3. `robots::path_matches` runs `wildcard_match` as a full-string match even for patterns without `$`. A robots
   pattern without `$` is a prefix match, so `/*?session=` never matched `/page?session=9`. Same cause for
   `robots_gate::tests::query_aware_rules_deny_the_session_url`.
4. `robots::RobotsTxt::parse` ignores blank lines, so `User-agent: quarrybot`, a blank line, then `User-agent: *`
   with `Disallow: /` gave the named empty group the `*` rules. The W2-U63 decision record says a named empty group
   allows everything (W2-U63-progress.md:18).
5. `crawl_scope::tests::denial_samples_cap_per_code_and_per_page`: the scenario has three codes, which can hold at
   most nine samples, so the per-page cap of 20 can never be reached. The assertion is impossible as written.
6. `crawl_limits::zero_max_pages_is_the_default_never_unlimited`: `parse_body` turns a lenient `max_pages: 0` into
   `None`, so the ack and the job params lose the echoed `0`. The W1 decision (limits.rs:720 asserts
   `requested == Some(0)`; plan line 1236) says the zero is echoed. The crawl_request unit test that asserts `None`
   is the outlier.
7. `crawl_request::tests::pattern_total_boundary_is_8192_characters`: reading says it should pass at HEAD. Not
   confirmed: no run has been made in this session yet (see the verification section).

## Decisions taken inside this unit (not the owner's decisions)

- Robots enforce default stays off (X-10). Nothing in this unit flips a default.
- D2-029 keeps Go unchanged. The edge now reports `scope_applied: true` in observe; the two tests that asserted
  `false` were corrected to `true` (the orchestrator contract at crawl_types.go:75 and RUNBOOKS line 1786 define the
  field as the proof that the edge understood the context).
- Robots rule `Disallow: /*?session=` (plan line 2635 example) is kept. The test line that expected
  `/a/b?x=1&session=2` to be denied was corrected: `?` is literal, so that URL is not matched.
- Zero `max_pages` in lenient mode is kept as `Some(0)` in `ParsedCrawl` (the resolver still takes the default).
  `max_depth` zero stays `None`; the crawl_request test for depth is unchanged.

## Status (final)

Commits (local, branch quarryv2-next, no push, no PR, no --no-verify):
- afeb90c6a test(quarry-edge): crawl_scope_robots fixture uses a valid internal secret (item 1)
- 9d127e536 fix(quarry-runtime): observe reports scope_applied so a crawl is not ended after one page (D2-029) (item 2)
- 7c07498e7 test(quarry-core): denial sample cap test reaches the per-page cap (item 3)
- 2f67e6262 fix(quarry-edge): a lenient max_pages of 0 is echoed as requested; the pattern guard works (item 4)
- 67e3b3d3e fix(quarry-transform): robots rules without $ are prefix matches; a blank line ends a named group (item 5)
- 8a36c8471 docs(quarry-v2): correct the robots unreachable env name in CRAWL_SCOPE_ROBOTS (D2-032) (item 6)
- f53cd3658 docs(openapi): scope_applied is set in both modes (D2-029) (item 6; outside the owned list, one line)

Verified after the last code change (qtest, run2.log in the scratchpad folder D-B1-crawl-robots):
- quarry-edge --test crawl_scope_robots: 15 passed, 0 failed (13 named tests and 2 map tests)
- quarry-edge --test crawl_limits: 5 passed (includes zero_max_pages_is_the_default_never_unlimited)
- quarry-core --lib crawl_scope: 13 passed (includes denial_samples_cap_per_code_and_per_page)
- quarry-runtime --lib robots_gate: 26 passed (includes query_aware_rules_deny_the_session_url)
- quarry-runtime --lib crawl_filter: 32 passed (includes observe_reports_scope_applied_on_every_page_kind)
- quarry-transform --lib robots: 23 passed (includes a_named_group_with_no_rules_allows_everything and query_aware_rules_match_path_and_query)
- quarry-edge --lib crawl_request: 20 passed (includes pattern_total_boundary_is_8192_characters)
- cargo check --workspace --all-targets: exit 0 (Finished; warnings only, none in the files this unit changed)

Before the pattern-guard fix, run1.log showed pattern_total_boundary_is_8192_characters failing with two errors
(patterns and crawl.scope). The guard fix (2f67e6262) reduced it to one error. Every other named test passed in run1.

Uncommitted and not ours: crates/quarry-edge/tests/support/fixture_site.rs (EOL-only stray; git diff --ignore-cr-at-eol is empty). Left as is.

## Not done and findings left open

- D2-072 (crawl_request.rs overwrite): recorded, no code change. Committed history has one add of the file (6dd1456ce,
  WS14.S6). S5 (7edf11250) put its check in batch_check.rs and does not reference crawl_request. An overwritten
  uncommitted draft cannot be recovered from git; the owner should ask the S5 and S6 authors whether a draft existed.
- D2-029 Go side: unchanged in this batch. appliedBy still fail-closes on a missing scope_applied (version skew).
  No go test was run for this change.
- scope_applied is now true for batch pages too (the batch path sends observe). The Go BatchJobWF was not re-read for
  that path; it reads the same flag.
- Robots blank-line rule (item 5): the W2-U63 decision is followed. It was not checked against the RFC 9309 text in
  this session; the owner should confirm it there.
- max_depth zero is still not echoed as requested (max_depth_requested is null for 0 while max_pages_requested is 0).
  Low. The depth test is unchanged. Recorded here, not fixed.
- routes.rs: no edit was needed for the named tests (the zero echo lives in crawl_request.rs).
- Robots enforce default stays off (X-10). No default changed.
- No stack, live Chromium or Go test was run. The full lib suites of quarry-transform, quarry-core, quarry-runtime and
  quarry-edge were run once after the commits (regression run, see below).
- Trailer: Co-Authored-By Claude Haiku 5.5, per the session attribution reminder. The unit text asked for Opus 5.5;
  the owner should confirm which line the repository carries (the W2-U64 hand-over raised the same question).
- No separate depth/D-B1-crawl-robots.md was written, because the subagent rule forbids findings .md files. The
  findings are in this file and in the final JSON.

## Regression run after the commits (qtest, reg.log)

- quarry-transform --lib: 200 passed, 0 failed.
- quarry-core --lib: 241 passed, 0 failed.
- quarry-edge --lib: 602 passed, 0 failed.
- quarry-runtime --lib: 994 passed, 2 failed. The two failures are observation::tests::an_explicit_screenshot_action_is_refused_when_the_image_is_withheld
  and observation_redaction::tests::the_dom_summary_masks_shapes_and_keeps_the_fingerprint_id. Both are in the W2 failure list
  (batch 2, D-B2-profiles-receipts) and do not use crawl_filter or robots. Not touched by this unit.
