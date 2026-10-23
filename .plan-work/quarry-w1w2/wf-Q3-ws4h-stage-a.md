# Q3-ws4h-stage-a, review round 4
Both medium findings real; fixed in 79c1ec1c (tests/zdr_refusal_modes.rs only).
- Enforce tests now assert zdr.refused events (row/action/org_id keys, one per refused write, no probe in log), via an ENFORCE_LOCK + mark/since delta because the capture is process-global.
- Profile store asserted empty (via GET /v1/profiles wind-down read) after refused writes; standard-org test asserts non-empty so the check is sensitive.
- GET /v1/change/latest and POST /v1/profiles/{id}/restore_probe exercised in the enforce wind-down test and the observe test.
Tests: zdr_refusal_modes 5, zdr_gap_rows 9, log_field_discipline 22 all pass. Not mutation-tested.
