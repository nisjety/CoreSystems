# B-T2 progress
- Docker was NOT answering (qtest.sh: DOCKER IS NOT ANSWERING; `docker info` hung 30 s). Per the rules: no retry loop.
- Cause proven by a standalone host repro (notes/wf-B-T2-repro; target dir C:/Users/IMAFER~1/AppData/Local/Temp/qrepro-target):
  baseline 156/200 failures with left 0 right 1, fix 0/200.
- Fix APPLIED, UNCOMMITTED, in apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs (+27 lines, scrape_challenge_tests only):
  `keep_per_thread_subscribers_reliable()` called from `Installed::new` and at the top of the log test. Not compiled in-tree
  (quarry-runtime cannot build on the host); the snippet uses the same tracing APIs the repro compiled.
- Not committed because the pre-commit hook needs docker (gitleaks) and I would not claim in-tree results I did not see.
- NEXT (when docker answers):
  1. `qtest.sh 'cargo test -p quarry-runtime --lib scrape_challenge_tests 2>&1 | grep -E "test result|FAILED|^error" | head'` (compile check)
  2. loop >=20x the full lib at default threads in ONE container (write end marker, check ${PIPESTATUS[0]}), expect 0 failures;
     also loop the module filter (was 35/43 failing) at least 20x.
  3. whole quarry-runtime lib once. 4. commit with qcommit.sh: pipeline.rs only, message "test(quarry-runtime): ... (R8b)".
  5. update docs/PROGRESS.md Tests snapshot row only if the team wants (R8b text says "Not fixed").
