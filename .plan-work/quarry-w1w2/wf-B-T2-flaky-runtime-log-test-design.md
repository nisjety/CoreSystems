# B-T2 design: the flaky scrape log test (pipeline.rs, scrape_challenge_tests)

## Symptom
`the_scrape_path_logs_the_reason_and_the_status_and_never_the_host` fails with `left 0, right 1` and an empty
event list: a tracing event was dropped, not miscounted. 2 of 12 full-lib runs, 35 of 43 runs of the module filter
at default threads, 0 at --test-threads=1.

## Cause (now proven by experiment, not only suspected)
tracing-core 0.1.36 caches an Interest per callsite when it is first hit. In `callsite.rs`, `Dispatchers::rebuilder()`
returns `Rebuilder::JustOne` while `has_just_one` is true (at most one dispatcher registered process-wide), and
`JustOne` is evaluated with `dispatcher::get_default`, i.e. the CURRENT THREAD's default dispatcher. A scoped
`set_default` subscriber (the EventLog) is registered process-wide but visible only to its own thread. So:
thread A (another test, no subscriber) pushes the callsite at pipeline.rs:843/859 and then computes its interest
through JustOne = NoSubscriber = "never"; thread B's `set_default` rebuild may run in between and set "sometimes", and
A's later `set_interest(never)` overwrites it. The callsite stays "never" and B's event is dropped. The other tests of
the module (a_strong_block_page..., the_log_phase..., a_zdr_scrape...) are the ones that hit those two lines with no subscriber.

Evidence: a standalone crate (notes/wf-B-T2-repro: tracing 0.1.44, tracing-core 0.1.36, the same EventLog shape,
8 "noise" tests and 1 capture test released together by a Barrier, test-threads=9): the capture test failed with the same
`left: 0, right: 1` in 156 of 200 runs. The tracing-core source reading above predicts exactly this.

## Options
1. Move the log assertions to their own integration-test binary. Rejected: `TEST_CHALLENGE_CONFIG` and the private
   `PageRunner` fields are `cfg(test)`/crate-private, so the integration binary would need production hooks; and
   it would fix only this test, not the mechanism.
2. `Interest::always` from the EventLog's `register_callsite` plus `rebuild_interest_cache`. Rejected: the wrong value
   is computed by the OTHER thread through JustOne, which never asks our subscriber, so it does not help.
3. (chosen) Keep two `NoSubscriber` dispatchers alive in a process-wide `OnceLock`, created before the first
   scrape of the module (in `Installed::new`, which every test of the module calls, and at the top of the log test). Two
   registered dispatchers set `has_just_one = false` permanently (the flag is only recomputed at registration, and the live
   count never drops below 2). Interest is then always the `and` of all registered dispatchers: NoSubscriber (never) and
   EventLog (sometimes) give "sometimes", so every event asks the hitting thread's own subscriber; a thread without
   one gets `enabled == false`. Registration and rebuild are serialised by the dispatcher RwLock, so no stale "never" can survive.
   Test-only, 27 added lines, no production code, no new dependency, no baseline change.

## Verification plan
Same standalone repro with the two dispatchers: 0 of 200 failures (FIX=1). In-tree: quarry-runtime lib at default
threads looped 20+ times in one container, the module filter at default threads looped, then the whole lib once.
