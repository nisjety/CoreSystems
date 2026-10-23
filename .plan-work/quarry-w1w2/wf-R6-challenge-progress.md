# R6-challenge progress

Started 2026-10-18. Plan of record: tasks 1-6 of the unit.
Findings so far:
- Plan flag table says `v2`, enforce (`log` while validating): ambiguous -> decision doc, code default kept (Enforce).
- Evaluate is already denied by the edge (admission.rs) and by ObservationRunner::execute (raw_script_denied) AFTER reject_if_exhausted; decision: refuse Evaluate on exhausted host (defence in depth).
- Scrape path: PageRunner::run_inner returns Ok(resp) for block statuses/interstitial bodies; nothing classifies them.
Status: nothing committed yet.

## Milestone 1 (all uncommitted, mine)
Edits in tree: challenge.rs (Evaluate refused, classify_fetched, doc, tests), observation.rs (Evaluate in refused list test), pipeline.rs (scrape-path check + scrape_challenge_tests module appended), quarry-core contracts.rs (jsonschema derive on 5 types), error.rs (challenge_detected_on_fetch), Cargo.toml ([[test]] challenge_schema), tests/contracts.rs (+1 test), tests/challenge_schema.rs (new), scripts/tests/openapi-challenge-parity.test.mjs (new), .github/workflows/quarry-v2-ci.yml (2 steps), docs openapi.yaml/SOVEREIGN/CONTRACTS (9.2 appended)/RUNBOOKS (section appended).
FOREIGN hunks in docs/CONTRACTS.md (takeover paragraph) and docs/RUNBOOKS.md (R8a section) -> commit mine via index (hash-object + update-index of HEAD+my section), not qcommit on the whole path.
Still to do: golden docs/schemas/challenge-signal.schema.json (UPDATE_SCHEMAS=1 run), run all tests (t1.log was an early queued run), SDK docs regen (task 5), commits.
Notes written: wf-R6-challenge-decision.md, wf-R6-challenge-gateway-contract.md.

## Milestone 2
Verified so far (qtest): quarry-runtime lib -- challenge:: observation:: host_scheduler:: = 76 passed; lib -- ... pipeline:: (117 passed incl. scrape_challenge_tests); quarry-core challenge_schema + scrape_schema (jsonschema) passed after golden was generated (docs/schemas/challenge-signal.schema.json); node openapi-challenge-parity 8 passed, mutation (removed enum values from openapi) -> 2 failures, restored.
t3 (contracts test, cargo check --workspace --all-targets, cargo test -p quarry-edge) queued: scratchpad/R6-challenge/t3.log (marker QTEST_DONE).
Commit tooling: scratchpad/R6-challenge/hunkcommit.js + jobs-1..4.js + msg-N.txt (HEAD+my hunk through a temp index; shared files openapi.yaml, CONTRACTS.md, RUNBOOKS.md, quarry-v2-ci.yml carry foreign hunks). Run: bash tools/with-lock.sh git 900 -- node hunkcommit.js msg-N.txt jobs-N.js
Plan: C1 (Evaluate, jobs-1), C2 (schema golden + parity + CI, jobs-2), C3 (scrape path, jobs-3), C4 (RUNBOOKS, jobs-4), then SDK docs regen from HEAD spec (task 5).

## Milestone 3 (round 3, 2026-10-19)
C1 = 08b0a8be (Evaluate), C2 = 91a64773 (schema golden + parity + CI) COMMITTED. 
Fixed a borrow error in quarry-core tests/contracts.rs (error.details.clone()). t3 (old) only failed on that.
Running scratchpad/R6-challenge/t4.sh -> t4.log (marker QTEST_DONE): contracts+challenge_schema+scrape_schema, cargo check --workspace --all-targets, runtime lib challenge/observation/host_scheduler/pipeline.
Next: C3 = jobs-3.js with msg-3.txt (scrape path), C4 = jobs-4.js with msg-4.txt (RUNBOOKS), then SDK docs regen (task 5), then final notes.
Run: bash tools/with-lock.sh git 900 -- node hunkcommit.js msg-N.txt jobs-N.js (cwd scratchpad/R6-challenge)

## Milestone 4: ALL COMMITTED (2026-10-19)
08b0a8be (Evaluate + table tests), 91a64773 (golden+parity+CI), d8e62fa3 (scrape path), 23e1ad7f (RUNBOOKS), ab8a5b99 (SDK docs ChallengeSignal+ScrapeApi). t4 results: runtime lib 118 passed, core contracts 30/challenge_schema 2/scrape_schema 2, check --workspace clean; edge all ok (t3). node parity 13 pass.
Remaining uncommitted in Q: not mine (cache_policy, zdr_claim_floor, CROSS_PLANE_INTEGRATION, RUNBOOKS R8a section).

## Milestone 5 (round 3 review fixes, 2026-10-19), UNCOMMITTED, all mine
Edits: challenge.rs (default Log, from_values only `enforce` enables, ChallengeConfig::enforcing(), renamed table test), observation.rs (ContentDriver back/evaluate + new runner-level history test; tests use enforcing()), pipeline.rs (comment fix for the HIGH finding, EventLog test for scrape log lines, tests use enforcing()), docs (openapi 403+ChallengeSignal desc, SOVEREIGN, CONTRACTS 9.2, RUNBOOKS), notes in side-session-notes.
Test run: scratchpad/R6-challenge/r3-t1.log (marker QTEST_DONE): runtime lib challenge/observation/host_scheduler/pipeline.
Next: after green -> commit (hunkcommit.js for shared docs with foreign hunks: RUNBOOKS has R8a hunk, openapi/CONTRACTS maybe), regen SDK docs (ChallengeSignal.md + ScrapeApi.md) from HEAD spec, node parity tests, cargo check, quarry-edge tests.

## Milestone 6: ROUND 3 ALL COMMITTED
1b1a9810 (default log, scrape log test, runner-level Back test, comment fix), 12e37792 (docs), d2829c30 (SDK pages). Verified: runtime lib filter 119 passed; mutants both killed; cargo check --workspace --all-targets ok; quarry-edge tests all ok; node parity 13 pass.

## Milestone 7 (round 4 review: 3 medium findings): committed 6f918335 (current_thread pin + ZDR no-host assert; 'page extracted' URL line = existing behaviour, no change). Test 86 passed.
