# D-D-B2-zdr-pins: hand-over (2026-10-27)

Commits (local, branch quarryv2-next): 9cdd0c604 (D2-022 + span-export rig), d150cf0d9 (job cancel test + D2-047), cba5143d6 (D1-003 stream tombstone), 869c93a38 (log lint R5), d5f1a5a6f (matrix rows + profile test tokens + golden), 5b01c20f0 (contract-sync pin, my hunk only; the file's other hunk is the profiles-receipts unit's).

Results (qtest, quarry-edge): zdr_feature_matrix 13, zdr_gap_rows 9, zdr_refusal_modes 8, zdr_span_export 5, zdr_tombstone 29, zdr_claim_floor 25, log_field_discipline 22, job_lifecycle 13, all passed; --lib otel_zdr/zdr_tombstone filter 12 passed; quarry-core --lib zdr 13 passed; cargo check --workspace --all-targets rc 0; scripts/tests/zdr-contract-sync.test.sh 71 passed, 0 failed.

Where a TEST was corrected (not the code): job_lifecycle cancel (the double merged an accepted job, which control cancels at once, with a running one); profile save tests (WS17.P0 import scope); wind-down test (WS17.P0 read/probe refusal); zdr_span_export rig (provider dropped = exporter emptied). Where the CODE changed: otel_zdr strip_span (events/links), answer_stream tombstone guard, profile_restore helper rename.

Findings left: depth/D-D-B2-zdr-pins.md. Traps: qtest cannot run a script that lives in the scratchpad (only the Quarry tree is mounted): pass the commands inline; the zdr-contract-sync script pins the feature-matrix golden digest, so any matrix change needs the pin updated (and the script may hold another unit's hunk: commit with qcommit-patch.sh).
