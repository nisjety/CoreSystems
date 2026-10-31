# B-T2 hand-over: flaky scrape log test
Status: PARTIAL. Cause proven and fix written; in-tree verification and commit blocked by Docker not answering.
See wf-B-T2-flaky-runtime-log-test-design.md (cause, options) and wf-B-T2-flaky-runtime-log-test-progress.md (next steps).
Fix: uncommitted +27 lines in quarry-runtime/src/pipeline.rs (scrape_challenge_tests): two process-lifetime NoSubscriber
dispatchers switch off tracing-core's JustOne interest shortcut that lets a subscriber-less test thread cache "never".
Trap: tracing-core JustOne uses the CURRENT thread's dispatcher, so any set_default-based capture test races other threads' first hits.
Repro: wf-B-T2-repro/ (Cargo.toml, lib.rs -> src/lib.rs, race.rs -> tests/race.rs; run with --test-threads=9, FIX=1 enables the fix; build on host with CARGO_TARGET_DIR on a short path, the default scratchpad path hit LNK1104).
