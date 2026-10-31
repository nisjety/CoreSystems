# Q3-ws4h-stage-a progress
- cargo check --workspace --all-targets: rc 0. cargo check -p quarry-edge --features postgres-queue --all-targets: ok.
- targeted tests all green (lib zdr_refusal 2, zdr_refusal_modes 5, gap_rows 9, feature_matrix 12, claim_floor 22, log_capture 12, log_field_discipline 22).
- Next: full quarry-edge suite (log $S/Q3-ws4h-stage-a/full.log), then commits: (1) helper+flag+routes+wiring, (2) tests (+harness), (3) runbook. Only my hunks are in the tree for these files (main.rs 2 hunks, RUNBOOKS 1 hunk): whole-file qcommit is fine.

## Round 4b (review fixes) - DONE
- 1b8ee4ca: tests/zdr_refusal_modes.rs only. (1) enforce durable-writes test seeds a named user_private profile for the ZDR org, durable_writes takes the profile id, asserts list = [(id, seeded name)] and store metadata unchanged after refused update. (2) ENFORCE_LOCK renamed CAPTURE_LOCK, observe test takes it too.
- Tests: zdr_refusal_modes 5, zdr_gap_rows 9, log_field_discipline 22 passed via qtest.
