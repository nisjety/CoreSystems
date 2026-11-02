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

## Batch W2-B2 (W2), integrated 2026-11-02

Units and merge commits (--no-ff, no conflicts in any merge; resumed after a usage limit, merges were already done):
- W2-U25-29 (lane a): 93ac67b02 merge: w23/lane-a. Checkpoint carries trace_seq and the start policy ask; a resume keeps the event seq and merges the stored ask tighten-only (D2-061, D2-051, D2-052 egress gap).
- W2-U25b (lane b): 2b5723898 merge: w23/lane-b. Agent-class events never reach JetStream raw (is_agent_class gate in publish_encoded, D2-098; leak fix, no flag).
- W2-U28a (lane c): 5b1c9293c merge: w23/lane-c. QUARRY_CONTROL_INTERNAL_SECRET for the orchestrator-only control routes (falls back to QUARRY_INTERNAL_SECRET with a warning); finalize refuses another org; 8 Go control internal routes in openapi-internal.yaml.
- W2-CP-R13-2 (lane d): b83be5ee1 merge: w23/lane-d. quarry-edge/quarry-control credentials in org-core, billing-core, audit-core compose; dark flag defaults; Prometheus token mount; quarry-edge scrape job and six Quarry alert rules.

Integration follow-ups: none needed. The four .sha256 files left modified by the interrupted attempt (auth-core cross-plane-token-requests, usage-ledger and retrieval-engine usage-envelopes.v2, quarry-runtime telemetry-field-contract) have no content difference from HEAD (git diff empty, LF in index and worktree): stat/EOL noise only, left alone, nothing to commit. No generated file changed (no new event types or routes): ZDR matrix tests 6 + 14 pass without QUARRY_UPDATE_GOLDEN.

Checks at w23/integ:
- Quarry Rust (qtest-lane integ): cargo check --workspace --tests clean (exit 0). quarry-runtime --lib 1134 passed; quarry-edge --lib 864 passed. quarry-edge integration: agent_checkpoint_policy 11, agent_resume_step_keys 6, web_policy_agent 7, agent_run_control 35, agent_events_replay 7, run_events_scope 3, agent_landed_page 8, agent_step_timeout 2, agent_run_budget 11 (3 ignored), zdr_refusal_modes 8, zdr_feature_matrix 14. quarry-core zdr_matrix lib 6; challenge_schema 2 and scrape_schema 2 (with --features jsonschema).
- Go (GOWORK=off): quarry-control and quarry-orchestrator go vet clean, go test all packages ok. org-core, billing-core, audit-core go vet clean, go test ./... ok with REQUIRE_CONTRACT=1.
- node --test scripts/tests/*.test.mjs: 73 tests, 71 pass, 2 fail, both known inherited (/v1/audit/page undocumented; tracked internal SDK files).
- No TS package touched; auth-core, verevonv3 untouched. Not rerun here: promtool (lane d ran it: 16 quarry cases ok), validate-release-credentials-test.sh (cannot run on this Windows host).

Open items:
- Postgres-gated tests (checkpoint round trip, schedules scoping) not run against a database; no live stack, no compose parse of the nested QUARRY_CONTROL_INTERNAL_SECRET default.
- Rollout: set QUARRY_CONTROL_INTERNAL_SECRET on control and orchestrator together, never on the edge; rebuild org-core before recreating it with the new compose (new scopes must be allowlisted).
- Docs still describe one shared control secret (CROSS_PLANE_AUTH.md, ARCHITECTURE.md, RUNBOOKS.md).
- 46 depth lines across lanes in W23-depth.md; notable: W23-b-U25b-S1 (medium, agent events still go through the durable EventPublisher into control's event log), R13-2 security row on org:keys:unwrap:any (medium, WS17.E2a design), W2-U28a unsigned requests bypass the secret split when QUARRY_INTERNAL_HMAC_REQUIRED=0, SR-U25-2/3/4.
- Model Plane asks: none from this batch.

## Batch W2-B3 (W2), integrated 2026-11-02

Units: W2-U60 (lane a, CrawlJobWF replay fixtures and scope tests), W2-U38 (lane b, scheduled wrapper cancel/timeout/replay plus overlap-cancel fix), W2-U42 (lane c, quarry-control ZDR request span), W2-CP-WS7H (lane d, org-core web policy: migrations 025-027, policy, history, audit outbox, routes).

Merge commits: 0547e442e (lane a), 9888d666b (lane b), 8676a69c6 (lane c), f7124152f (lane d). No conflicts in any merge. Follow-up c8de51f11: both lane a and lane b added a test helper named loadReplayHistory in package workflows (different signatures); lane a's was renamed to loadCrawlReplayHistory (CRLF kept).

Generated files: none regenerated. No new event types or routes in Quarry (org-core routes are outside the Quarry route-count pin), so no ZDR matrix, schema golden or route-count change was needed.

Checks at w23/integ:
- Quarry Rust (qtest-lane integ): cargo check --workspace --tests clean (exit 0). quarry-edge --lib otel_zdr 12 passed; quarry-edge --test zdr_span_export 5 passed. No Rust source changed in this batch, so the earlier full counts (W2-B2) stand.
- Go (GOWORK=off): go vet clean and go test ./... ok in org-core (11 packages ok, REQUIRE_CONTRACT=1, Postgres-gated tests skipped), pkg/quarryotel (1 ok), quarry-control (16 ok), quarry-orchestrator (all packages ok after the helper rename; Replay|Scope|Scheduled run: 61 PASS, 0 FAIL, 2 SKIP, the skips are the fixture regenerators).
- node --test scripts/tests/*.test.mjs: 73 tests, 71 pass, 2 fail, both known inherited (/v1/audit/page undocumented; tracked internal SDK files).
- No TS package touched.

Open items:
- Postgres-gated org-core web-policy tests (8) not run at integ (lane d ran them on a throwaway postgres:16 container: all pass); org-core Docker image not built, go:embed of testdata in the image unproven.
- Replay fixtures (lane a and lane b) are synthetic, not exported from a live Temporal; replay a real export before the crawl-scope deploy.
- WS7.I must treat a 404 from the internal web-policy route as an error, never as an empty policy (W2-CP-WS7H security review); the golden auth-core-org-core-web-policy.contract.json must be matched by WS7.I's spec, and zdr-contract-sync rows plus contract-goldens.yml globs for it are not added.
- Flag-off by default: WEB_POLICY_CLAIM_ENABLED=0 (nothing reads the policy), QUARRY_SCHEDULED_JOBS_ENABLED=0. W2-U42 span is a no-op unless OTEL_EXPORTER_OTLP_ENDPOINT is set.
- 43 depth rows appended to W23-depth.md; notable: W2-U42 trace headers adopted before HMAC auth (low), no audit-flush loop in org-core main.go, plan change does not move policy rev, free-plan paid search turns off once claim and enforcement are live, ZDR classification of stored host lists, likely E2a merge conflict in the service_auth validator hunk.
- Model Plane asks: none from this batch.

## Batch W2-B1 addendum (W2), integrated 2026-11-02

Units: W2-U16 (lane a), W2-U31 (lane b), W2-U30-33a (lane c), W2-CP-R13-1 (lane d). The code of all four was already on w23/integ from the 2026-11-01 W2-B1 row; this merge brings in the lanes' later commits, which are docs only (security-review depth rows and the W2-U30-33a status commits section).

Merge commits (--no-ff, no conflicts): dd86c52bb (lane a), f839d5a4e (lane b), 36641841f (lane c), ea11ef634 (lane d). Diff against the pre-merge integ tip 35a3d9f2f is 5 files under .plan-work/quarry-w1w2 only (18 insertions, 1 deletion), so no code changed and nothing was regenerated (no ZDR matrix, schema golden or route-count change).

Checks at the merged tree:
- Quarry Rust via the integ wrapper: cargo check --workspace --tests exit 0 (warnings only). quarry-edge answer_gate 11 passed, job_follow_zdr 5, profile_roundtrip 12, run_events_scope 3; quarry-runtime --lib token_request_contract service_tokens 31. All 0 failed.
- Go (GOWORK=off): go vet ./... clean and go test -count=1 ./... all ok in quarry-control and quarry-orchestrator.
- node --test scripts/tests/*.test.mjs: 73 tests, 71 pass, 2 fail, both the known inherited ones (/v1/audit/page undocumented; tracked internal SDK files).
- scripts/tests/zdr-contract-sync.test.sh: 128 passed, 0 failed.
- auth-core jest (plane-service-principals-policy.contract, plane-service-principal.spec, plane-principal-registry-assembly): 3 suites, 185 passed, 1 skipped, 0 failed. Run in the innh-lane-d worktree, whose auth-core tree is byte-identical to integ (git diff empty); integ has no node_modules.
- No TS package was touched by this merge, so no tsc run.

Open items:
- principal-registry-parity.test.sh did not finish in about 30 minutes on this host (no output) and was stopped; the lane d run recorded 37 passed, 0 failed earlier. Not re-verified here.
- Open security depth rows (14 new, appended to W23-depth.md): SR-U16-5 (medium: schedules/desired capped at 500 with no cursor, so the reaper can delete real schedules once more than 500 exist; only with QUARRY_SCHEDULED_JOBS_ENABLED=1), W2-CP-R13-1 gRPC models:invoke reach (medium, Model Plane change recommended, not added to MP-ASKS.md), W2-U30-33a trailing-dot host deny bypass in host_matches (quarry-core web_policy), W2-U31 S6 to S8 (low).
- Four tracked .sha256 files in the integ worktree show as modified (CRLF/LF checkout artifacts, pre-existing) and a stray untracked q.json; none were committed.
- Model Plane asks: none.
