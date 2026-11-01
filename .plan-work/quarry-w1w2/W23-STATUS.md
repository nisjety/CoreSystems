# W23 status (integration ledger)

One row per integrated batch on branch w23/integ. Unit status files: W23-<unit>-status.md. Depth rows: W23-depth.md.

## Batch W2-B1 (W2), integrated 2026-11-01

Units and merge commits (--no-ff, no conflicts in any merge):
- W2-U16 (lane a): 0cb9f1b11 merge: w23/lane-a. GET /v1/schedules without org_id is always 400 (D2-042).
- W2-U31 (lane b): 78195efad merge: w23/lane-b. ZDR floor on follow_job and the legacy job-event drain, plus run-events org scope and redaction (security fix).
- W2-U30-33a (lane c): f7b5fe92c merge: w23/lane-c. Dev-bypass stub carries no scopes; /v1/answer, /v1/answer/stream and search include_answer gate result URLs with the web policy; restricted-policy callers skip the answer caches.
- W2-CP-R13-1 (lane d): e0875288e merge: w23/lane-d. Registry rows, manifest rows model-extract*, InferenceCore/Ingestion audiences; integration-corev2 refuses service bearers without integration:read/write.

Integration follow-ups (commit after the merges):
- zdr-refusals vendored copy re-vendored from apps/Model Plane/api/contracts (Model Plane W2-25 added connector_read and message_versions); quarry-edge tests/zdr_feature_matrix.rs catalogue feature count 30 -> 32.
- zdr-contract-sync.test.sh: zdr-feature-matrix.json digest pin refreshed to the committed golden (fbcb6d3c...; the golden itself did not change under QUARRY_UPDATE_GOLDEN=1, the pin was stale from INNH-11).
- .gitattributes: auth-core src/auth/fixtures/*.sha256 pinned to LF (vendored pin was compared byte for byte and checked out as CRLF).

Checks at w23/integ:
- Quarry Rust (qtest-lane integ): cargo check --workspace --tests clean. quarry-edge tests: answer_gate 11, answer_provider_status 14, job_follow_zdr 5, job_lifecycle 14, job_results 7, profile_guard 8, profile_roundtrip 12, run_events_scope 3, web_policy_routes 11, zdr_feature_matrix 14, all pass; quarry-edge --lib 856 pass; quarry-runtime --lib 1130 pass (includes token_request_contract and service_tokens). Redis on, REQUIRE_CONTRACT=1.
- Generated files: QUARRY_UPDATE_GOLDEN=1 left the ZDR matrix golden/.sha256/ZDR_MATRIX.md unchanged. No new routes, so no schema or route-count regeneration.
- Go: quarry-control go vet clean, go test all packages ok (Postgres-gated tests skip without QUARRY_TEST_DSN); integration-corev2 go vet clean, internal/auth ok.
- node --test scripts/tests/*.test.mjs: 72 tests, 70 pass, 2 fail, both known inherited (/v1/audit/page undocumented; tracked internal SDK files).
- scripts/tests/zdr-contract-sync.test.sh: 128 passed, 0 failed (after the follow-ups above).
- auth-core jest src/auth (via the lane d node_modules): 40 suites passed, 6 skipped, 683 tests passed, 0 failed. Full auth-core suite not rerun.
- No tsc package touched except auth-core (jest compiles it); verevonv3 untouched.

Open items:
- Postgres-gated tests (schedules scoping) not run against a database.
- Not run live: no deploy, no container swap, no flag flip.
- Depth rows (27 across lanes) in W23-depth.md; notable: SR-U16-2 internal schedules/desired HMAC posture, W23-b-U31-S1 control GET /v1/runs/{id}/events org check (Go, not fixed), R13-1 security rows (apiKeyExchange principal split, model-invoke authorisable by Auth Core).
- Model Plane asks: none from this batch.
