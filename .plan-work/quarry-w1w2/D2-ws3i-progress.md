# D2-ws3i progress (design reviewer, Opus)

- Status: design note written: .plan-work/quarry-w1w2/D2-ws3i-design.md. No repository file edited, no commit (unit is design-only).
- Read: plan l.903-909, breakdown item d, baseline WS3.I row; code fetch.rs 195-450, egress_broker.rs 90-210, fallback_driver.rs 1-240 and 566-570, driver_plan.rs, fingerprint_rotation.rs, pipeline.rs 598-612 and 815-845, challenge.rs 85-150 and 1050-1090, routes.rs 570-600, cache_policy.rs 585-640, presets.go, presets_test.go, resources.go 510-528, openapi.yaml 5145-5165.
- Key findings beyond the plan: (1) FallbackDriver falls through on Err(UpstreamBlocked) (is_retryable_driver_error), a status-driven identity change via the error path; Honest must gate it. (2) EgressBroker::mark_http_status cools a proxy down for 60 s on a block status, so the next request rotates; Honest must treat statuses as transport-healthy. (3) cache_policy.rs "prior_block_signals 2" case must change in the flip commit. (4) attempt_record puts proxy_uri and raw error text into error details (possible credential leak; follow-up, not in scope). (5) openapi line is 5155, not 4443.
- Tests run: none (design unit).
- Next: builder unit implements per the note's commit order.

## Rev 2 (2026-10-23, re-run "Prøv igjen")
- Slice found fully built (61906c93, 64346fd1, 83e920f3, 89097598 flip, 654806c2, 8cf01450, 8566f2c1, 39b0dec3). Design note rewritten against the current code (line anchors re-checked by grep), trimmed toward the 600-900 word target, adds: connect-failure-as-UpstreamBlocked side effect, challenge_forbids_escalation, verevonv3 products.rs:216 still sending prior_block_signals: 1.
- No repository file edited, no commit, no tests run (design unit).
