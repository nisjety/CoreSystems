# W2-U63 status: robots.txt parser gaps and enforce-refusal counting (D2-091)

Branch w23/lane-c. Date 2026-11-02.

## Built
- robots.rs (quarry-transform): the parser splits on CR and LF (`split(['', '
'])`) instead of `str::lines`, so a CR-only file no longer reads as one line (every rule lost, fail open). CRLF, LF, mixed endings and a CR-only file with a BOM all parse the same. A repeated `Crawl-delay` (in one group, in merged groups of one agent, or on a shared agent line) keeps the largest usable value regardless of order; negative, NaN and infinite values never displace a usable one (`merge_crawl_delay`). A lone unusable value is still stored as written and still reads as no delay in `crawl_delay_any`.
- Already existed, not rebuilt: leading BOM strip, empty `Allow:` ending the user-agent list without adding a pattern (both had tests). Added only a combined BOM + empty Allow + CR-only test.
- metrics.rs (quarry-edge): `robots_link_refusals(&CrawlReport) -> (disallowed, unreachable)` and `record_robots_link_refusals(&CrawlReport)`, which increments `quarry_robots_link_refusals_total{outcome="denied"|"unreachable"}`. Closed labels, content-free. Link refusals come from the discovery filter's cached peek, which never reaches the gate's counters, so enforce-mode link refusals had no counter.
- host_scheduler.rs (quarry-runtime): `acquire` now waits for the Crawl-delay start time BEFORE taking the per-host concurrency slot (it used to hold the slot while sleeping). New two-phase API: `reserve_start(host) -> StartTicket` (no slot held; `ticket.wait()`), `acquire_reserved(ticket) -> SlotPermit`. `acquire` = reserve + acquire_reserved; start spacing is unchanged.

## Partly (needs a call site outside this unit's files)
- The metric function is not called yet: routes.rs `with_crawl` (and crawl_sitemap.rs) must call `metrics::record_robots_link_refusals(&outcome.report)` when `gate.mode() == Enforce`. Those files belong to other units.
- The GLOBAL permit: pipeline.rs (~line 888) takes `autoscale.acquire()` BEFORE `scheduler.acquire(host)`, so a Crawl-delay wait still holds a global autoscale permit. The fix is in pipeline.rs: `let ticket = s.reserve_start(host).await; ticket.wait().await;` before the global acquire, then `s.acquire_reserved(ticket)` after. The scheduler side is built and tested.

## Commits
See `git log w23/lane-c` (robots.rs, host_scheduler.rs, metrics.rs, this status/depth).

## Tests (Rust via qtest-lane c)
- quarry-transform `--lib robots`: 32 passed, 0 failed (new: cr_only_line_endings_do_not_fail_open, a_duplicate_crawl_delay_keeps_the_largest_whatever_the_order, an_unusable_crawl_delay_never_displaces_a_usable_one, a_bom_and_an_empty_allow_are_handled_together_in_a_cr_only_file).
- quarry-runtime `--lib host_scheduler`: 20 passed, 0 failed (new: a_crawl_delay_wait_holds_no_concurrency_slot, queued_requests_keep_their_spacing_with_a_single_slot, a_ticket_can_be_waited_for_before_any_other_permit_is_taken, a_host_without_a_delay_gets_a_ticket_that_is_ready_at_once).
- quarry-edge `--lib metrics`: 10 passed, 0 failed (new: robots_link_refusals_read_only_the_two_robots_codes).

## Flag and default
No flag. Parser fixes are bug fixes (fail-open closed). The new metric series exists only once called. Robots observe/enforce defaults unchanged; no enforce flip.

## Assumed decisions
- Duplicate Crawl-delay: largest usable value wins (the polite choice; "first/largest" in the queue text).
- Wait-before-slot rather than reserving a slot: keeps start-to-start spacing identical.
- Link-refusal counter is a separate series, not folded into `quarry_robots_decisions_total`, whose unit is a page decision.

## Model Plane asks
None.

## Not verified
- The metrics counter was not exported through a real recorder (tests/metrics_recorder.rs is outside scope); only the extraction helper is tested.
- Sitemap-from-robots scanning (quarry-transform sitemap code) was not checked for CR-only endings.
- No integration or live run.
