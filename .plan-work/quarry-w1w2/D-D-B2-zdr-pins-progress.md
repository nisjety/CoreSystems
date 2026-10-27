# D-D-B2-zdr-pins progress (2026-10-27)

Nothing committed yet. Edits made (uncommitted, mine), scripts in scratchpad D-B2-zdr/e*.js:
- otel_zdr.rs: strip_span strips sensitive keys from events + links too (D2-022) + unit test.
- tests/zdr_span_export.rs: Rig keeps the SdkTracerProvider alive (dropping it shut the in-memory exporter down and emptied it: every test read 0 spans); new test a_kept_spans_events_lose_the_sensitive_keys_too.
- tests/job_lifecycle.rs: control double models running job (pending 202) vs ACCEPTED_JOB (cancel applied at once 200).
- tests/zdr_claim_floor.rs: Harness::get + job_errors_are_refused_for_a_zdr_org_user_and_open_to_a_standard_org (D2-047).
- tests/support/zdr_harness.rs + zdr_gap_rows.rs + zdr_refusal_modes.rs: profile save needs quarry:profiles:import (WS17.P0): token_with_scopes helpers; wind-down test expects profiles read/probe refused under the flag.
- zdr_matrix.rs: 8 new rows/entries for W2 routes (job_lifecycle_reads, job_errors, job_control, agent_attach, internal_schedule_preflight, internal_org_profile_purge, crawl/batch events, tab control, enqueue_batch), profiles row text; zdr_feature_matrix.rs PENDING_FEATURES + without_a_test list (agent_attach). Golden/markdown regenerated via QUARRY_UPDATE_GOLDEN=1.
- quarry-browser profile_restore.rs: classify -> emulation_cdp_class (log lint R5, 2 sites).
- zdr_tombstone.rs StreamEnd + Pending::finish_with; answer_routes.rs answer_stream uses it (D1-003); 4 new tests in tests/zdr_tombstone.rs.
- D1-036: telemetry.rs already puts the EnvFilter on each output layer (no registry-level filter); no change.
Next: run QT (see task), fix, regen golden, commit per item.

## Final (2026-10-27): all committed
9cdd0c604, d150cf0d9, cba5143d6, 869c93a38, d5f1a5a6f, 5b01c20f0. See wf-D-D-B2-zdr-pins.md and depth/D-D-B2-zdr-pins.md.
