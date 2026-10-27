# D-D-B2-profiles-receipts progress (2026-10-27)

Unit: profile owner/guard failures, receipt/observation masking failures, redaction gaps (D1-030, D2-064, D2-045).
Scratch: `scratchpad/D-D-B2-profiles-receipts/` (e1..e8.js edit scripts, run1..run5.log).

## Baseline run (qtest, before any edit)
profile_owner 5 failed (403 on save: the test claims held no import scope), profile_guard import_over_cap_is_413 (422: snapshot body lacked session_storage), redaction_wire 3 failed (501 on type_ref: harness driver does not claim atomic targets; "[redacted]" instead of "[redacted:password]": a user token without hint is a Human step = metadata receipt), observation 2 failed (ContentDriver lacks full_visual_fidelity -> Unsupported; lowercase JWT in element fingerprint normalized_text not masked).

## Edits (all verified green in run3/run4)
- observation.rs (test only): ContentDriver.visual_fidelity flag; the withheld-screenshot test sets it.
- quarry-core shapes.rs: shape.jwt matches `eyj` in any case (D1-030 sibling: the fingerprint text is lower-cased). json.rs: pair_kind (name/value pairs: credential header names, session cookie names under both profiles, key-rule names under Durable); X-Csrf-Token/X-Xsrf-Token/X-Csrftoken added to HEADER_KEYS. RULE_VERSION qr-redact/4; PINNED_FINGERPRINTS dbb6e9c0...; golden rule_version + 4 cases; .sha256 pin 237398d6...; sync digest in scripts/tests/zdr-contract-sync.test.sh; docs/SECRET_REDACTION.md.
- observation_redaction.rs: action_for_event masks a raw `type` with no known control and no selector hint as a secret (D2-064). Receipt text default unchanged (X-19).
- profile_owner.rs: claims carry the import scope (test expectation contradicted WS17.P0). profile_guard.rs test: body has session_storage.
- redaction_wire.rs: typing steps are sent as the agent's (initiated_by agent; the human branch is metadata only, WS8.B); stale type_ref test claims atomic targets through a new RecordingDriver::claim_atomic_targets (agent_harness.rs, additive).
- lifecycle.rs: start_run with a body.profile_id calls profile_guard::authorize(Attach) (D2-045: the guard had no call site). tests/profile_attach.rs (3 tests).

## Test results
See the commit messages and the final answer.

## Final
Commits: b5a517eab (D1-030, qr-redact/4), 7b36ea222 (D2-064, observation test double), e46bb66cb (D2-045 guard call, profile_owner/profile_guard tests), 4f3eeb43c (redaction_wire tests, harness claim_atomic_targets).
Last run (run5, qtest): edge profile_owner 8, profile_guard 8, profile_attach 3, redaction_wire 8, attach_cobrowse 4, agent_events_replay 6, profile_roundtrip 11 passed; quarry-runtime --lib 997 passed; quarry-core all targets passed (245+4+31+10+15); cargo check --workspace --all-targets no error; scripts/tests/zdr-contract-sync.test.sh 71 passed 0 failed.
Open items: depth/D-D-B2-profiles-receipts.md.
