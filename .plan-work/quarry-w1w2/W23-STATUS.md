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

## Batch W2-B4 (W2), integrated 2026-11-02

Units: W2-U34 (lane a, built: 6b policy snapshot), W2-U64 (lane b, built: crawl scope skew), W2-U63 (lane c, partly built: robots CR-only/Crawl-delay parser, host scheduler wait-before-slot, link-refusal counter helper), W2-U33b (lane d, NOT built in this merge: only the security-review depth commit 9dc9a6868 was committed; the builder's audio_routes.rs, resource_routes.rs, schedule_routes.rs, source_registrar.rs and tests/url_gate_resources.rs were still uncommitted in the innh-lane-d worktree and are not on integ).

Merge commits (--no-ff, no conflicts, nothing regenerated): adb6aec26 (lane a), 86a5b5526 (lane b), 717322529 (lane c), 83a01de52 (lane d). Lanes a and b both edited orchestrator workflows.go and merged cleanly.

Checks at the merged tree:
- Quarry Rust via the integ wrapper: cargo check --workspace --tests exit 0 (warnings only). quarry-core --lib web_policy 7 passed; quarry-transform --lib robots 32 passed; quarry-runtime --lib host_scheduler 20 passed; quarry-edge --lib 872 passed. quarry-edge integration tests: web_policy_backstop 18, web_policy_routes 11, crawl_scope_robots 16, batch_handoff_check 3, crawl_options_off 3, crawl_options_on 7, zdr_posture_stamp 13, web_policy_claim 12. All 0 failed.
- Go (GOWORK=off) quarry-orchestrator: go vet ./... clean, go test ./... all 9 packages ok. quarry-control not touched by the batch.
- node --test scripts/tests/*.test.mjs: 73 tests, 71 pass, 2 fail, both the known inherited ones (/v1/audit/page undocumented; tracked internal SDK files).
- scripts/tests/zdr-contract-sync.test.sh: 128 passed, 0 failed.
- No TS package touched, so no tsc run. Golden, schema and route-count regeneration were not needed (no diff in those files; zdr_posture_stamp passes).

Open items:
- W2-U33b code is unmerged: re-run the lane d builder and merge its commit(s) in the next batch.
- W2-U63 call sites still to wire: pipeline.rs (reserve_start before the global permit), routes.rs with_crawl and crawl_sitemap.rs (record_robots_link_refusals).
- W2-U34 is flag-gated (QUARRY_EDGE__WEB_POLICY_SNAPSHOT, default off). Deploy order: page edges first, then the orchestrator, then turn the handoff flag on. Egress-receipt host-only under ZDR was decided but not built.
- Open security depth rows: SR-U34-2 (medium: run binding has no freshness or job binding, pre-existing), SR-U34-3/4/5 (low), W2-U33b rows 2 to 5 (low: unparsed audio_url host passes the deny list, STT contract gap with Model Plane /v1/ai/transcribe, schedules gated at create time only, other URL keys unchecked). 27 rows appended to W23-depth.md.
- Four tracked .sha256 files in the integ worktree show as modified (CRLF/LF checkout artifacts, empty diff, pre-pending); none committed.
- Model Plane asks: none from U34, U64, U63; the U33b transcribe contract gap is recorded as a depth row only (not added to MP-ASKS.md).

## Batch W2-B5 (W2), integrated 2026-11-02

Units: W2-U18 (lane a, built: omnibox scope refusal notice, typed outside_scope reason, results stay open), W2-U65-51 (lane b, built: crawl scope options and scrape capture toggles in the composer, flag VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED off), W2-U27 (lane c, built: ZDR receipt reads keep only the host of the page URL, D2-063; Playwright spec for the navigation error card and reload body, D2-133; security review found no blocker), W2-FE-L5a (lane d, built: ingestions run-control and schedule-state action contracts, client job-control calls, jobControl capability, RunJobControls pause/resume/cancel; no flag, SPA shows nothing unless the gateway reports jobControl true).

Merge commits (--no-ff, no conflicts, nothing regenerated): 930657800 (lane a), bdd7308ee (lane b), cc1c210ac (lane c), 8a4946016 (lane d). Only lane c touched Rust (quarry-edge agent_routes/receipts.rs); no Go module touched.

Checks at the merged tree:
- Quarry Rust via the integ wrapper: cargo check --workspace --tests exit 0 (warnings only). quarry-edge --lib agent_routes 181 passed; --lib receipt_url_tests 4 passed; --test redaction_wire 8 passed; --test zdr_feature_matrix 14 passed. No route, schema, ZDR matrix or route-count change in the batch, so no golden regeneration.
- Go: no module touched, go vet and go test not run.
- node --test scripts/tests/*.test.mjs: 73 tests, 71 pass, 2 fail, both the known inherited ones (/v1/audit/page undocumented; tracked internal SDK files).
- Verevon v3 (host vitest, node_modules is an untracked junction to the vigorous-perlman checkout): src/features/browser, src/features/dashboard/home, src/features/ingestions, src/shared/actions, src/shared/api, src/shared/context-packs: 190 files, 189 passed, 1 failed to load (quarry-client.test.ts: @quarry/client does not resolve, inherited); 1918 tests passed, 0 failed.
- tsc6 -p tsconfig.app.json --noEmit: 34 errors, all in untouched files (RemoteSupportPage.tsx 12, quarry-client.ts 21, schedule-contract.test.ts 1). The queue baseline of 33 is stale (lane a/d reported 34 as well); no error in any file this batch touched.
- scripts/tests/zdr-contract-sync.test.sh: 127 passed, 1 failed. RED, expected: the SPA copy apps/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json (regenerated by W2-FE-L5a) differs from the Model Plane golden. See open items.

Open items:
- RED until Model Plane acts: the agent-action admission golden. MP-ASKS.md (W2-FE-L5a entry) asks Model Plane to copy the SPA file over apps/Model Plane/api/contracts/agent-action-admission.json (LF sha256 bcf3d6ffbf020946ceb76994bd9e6ef0cccb7e249d1ed2fa5c95318bd51c0482), update the digest in scripts/tests/zdr-contract-sync.test.sh and re-vendor the capability-core, execution-core and model-gateway copies. Do not merge to main before that.
- W2-FE-L5a: the gateway routes (L5b) for status, pause, resume and cancel do not exist yet; controls stay hidden until jobControl is reported true. The gateway still dispatches the old run_schedule_action id.
- W2-U65-51: new composer text inputs and the W2-FE-L5a Actions column have no CSS (global.css outside the units), unlooked at in a browser.
- W2-U27: Playwright spec not run against a live stack. Depth rows record the latent ZDR gap (durable read learns ZDR only from the caller floor), the list_receipts vs browser_session_timeline mismatch, and the owner question on host-only vs registrable domain.
- W2-U18: D-U18-1 gateway matches Quarry's message text to derive browser_navigation_outside_scope (fragile).
- 21 depth rows appended to W23-depth.md (a 4, b 4, c 7, d 6, plus the section headings).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.
- Disk after hygiene: /wt 104G, C: 143G free of 951G.

## Batch W2-B6 (W2), integrated 2026-11-02

Units: W2-U52 (lane a, built: isolated scan world for screenshot masking and cross-origin iframe withhold, D2-066/067, both flags default to old behaviour; security review found no blocker), W2-U58 (lane b, built: typed profile load-failure report and setCookies acceptance/read-back check, D2-028/D2-117, bug fix; security review no blocker), W2-U55 (lane c, built: IndexedDB capture of closed-tab origins behind QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS, default off, D2-070; security review found one blocker, fixed in ac01a9551: the phase now reads through a blank carrier page, never a live tab), W2-FE-L5b (lane d, built: gateway crawl job proxy routes, scrape run kind, origin/phase/desiredState on run rows, job-control action dispatchers; QUARRY_EDGE__JOB_CONTROL stays off at the edge).

Merge commits (--no-ff, no conflicts, nothing regenerated): 8ba332cbf (lane a), 29bebf408 (lane b), cfe5b5b27 (lane c), c90fb59e3 (lane d). Only Rust touched: quarry-browser (lanes a, b, c) and the verevonv3 gateway (lane d). No Go module, no TS package, no golden, schema or route-count change, so no regeneration was needed.

Checks at integ:
- cargo check --workspace --tests (Quarry, qtest-lane integ): clean, exit 0 (warnings only, all pre-existing).
- cargo test -p quarry-browser --lib: 235 passed, 0 failed.
- cargo test -p quarry-browser --features chromiumoxide --lib: 363 passed, 0 failed, 41 ignored (live tests, need Chromium).
- Verevon v3 gateway, host cargo test in apps/gateway: 1065 passed, 0 failed, 1 ignored in the main binary; integration files 1 + 7 + 13 + 3 + 1 passed.
- node --test scripts/tests/*.test.mjs (Quarry-v2): 73 tests, 71 passed, 2 failed, both known inherited (/v1/audit/page undocumented; tracked internal SDK files).
- GOWORK=off go vet / go test: not run, no Go module touched. tsc: not run, no TS file touched.

Open items:
- W2-FE-L5a/L5b: the gateway does not yet report jobControl in the schedule-capabilities answer (config.rs and schedules.rs were outside L5b); SPA job controls stay hidden. The edge run list carries no desired state, so desiredState is null on the list.
- Every new W2-U52/U55 behaviour ships dark and none of the live Chromium legs were run: U52 isolated world, OOPIF withhold and the mask_ms p95 test (3 ignored live tests); U55 closed-origin IDB (1 live test); U58 cookie read-back host matching. Run them before flipping QUARRY_BROWSER_SCREENSHOT_SCAN_WORLD=isolated (D2-066 is a leak fix, recommend flipping first), QUARRY_BROWSER_SCREENSHOT_OOPIF=withhold or QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS.
- W2-U58: ProfileLoadError::restore_report() is not wired into load_first_tab_snapshot (chromiumoxide.rs was outside the unit); W2-U55: ProfileCapture.closed is not read by persist_profile_v2.
- New env flags are not yet in RUNBOOKS, SECRET_REDACTION, openapi or compose (D-U52-3).
- The earlier RED stays: the agent-action admission golden waits for Model Plane (MP-ASKS.md, W2-FE-L5a entry).
- 30 depth rows appended to W23-depth.md (a 12, b 5, c 9, d 4, plus the lane headings and the lane b review note).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts, empty content diff); none committed.

## main merged into w23/integ, 2026-11-02 (after W2-B6, before W2-B7)

Merge commit 6edc55ae7 (`git merge --no-ff main`, main is the base). main brought the remote merge (Connectors + Engagement planes, verevon-web, anbud, visibility, Algolia/GSC), the earlier uncommitted live work, the browser-broker policy row (9531e117d) and the NATS namespace cutover (`controlplane.*`, stream CONTROLPLANE).

Conflicts (3, all Control Plane, resolved as unions):
- `apps/Control Plane/docker-compose.yml` ORG_CORE_SERVICE_CREDENTIALS: main's rows; auth-core holds org:web-policy:read:any (W2) and org:domain:read:any (main); the W2 quarry-edge row is kept.
- `org-core/internal/http/server.go`: webPolicy (W2) and identity/SSO/SCIM fields (main).
- `org-core/internal/http/service_auth.go`: authCorePrincipal (W2) plus domainReaderPrincipal and scimEdgePrincipal (main).
- `retention_posture_contract_test.go`: the reviewed auth-core scope set now lists both scopes.

NATS: w23/integ adds no `aqencia.controlplane` / `AQENCIA_CONTROLPLANE` use beyond what main already has (same count per file in all 8 files that mention it: the migration tool, historical migration SQL, scripts, docs), so the cutover comes in with main unchanged.

Checks on the merged tree:
- org-core `GOWORK=off go test ./...`: all packages ok.
- Quarry (qtest, quarry-target-integ): `cargo check --workspace --tests` clean; libs quarry-core/runtime/edge/browser/transform/security 2911 passed, 0 failed; `cargo test -p quarry-edge --tests` 90 binaries, 2464 passed, 0 failed.
- verevonv3 gateway (host): the main binary needs `RUST_MIN_STACK=16777216` on Windows (default stack overflows). With it: 1270 passed, 3 failed: `domains::spaces::tests::{a_viewer_cannot_add_or_remove_room_members, an_owner_adds_a_person_and_control_gets_the_roster, an_unconfirmed_roster_is_reported_rather_than_claimed_as_success}` (404 instead of 200/502/403). They fail alone too, and w23/integ changes no Space or router file in the gateway (its gateway diff vs main is 5 files: actions dispatchers/handlers, ingestions runs, knowledge, knowledge/quarry), so these are inherited from main's Space work, not from this merge.

The lanes continue from 6edc55ae7: each lane branch must merge w23/integ (or be reset to it by the program) before W2-B7.

## Step S1 (W2), integrated 2026-11-03

Units: W2-U63-callsites and W2-U39a (lane c), W2-U41a and W2-U07 (lane a), W2-U21a (lane b), W2-U39b and W2-CP-WS7I (lane d). W2-U49a (lane b) is NOT merged: its session could not write into innh-lane-b, so the work sits as a patch outside the repo (scratchpad u49a/W23-W2-U49a.patch) and has no commit on w23/lane-b.

Merge commits (git merge --no-ff, base 4c6b78669):
- c89b3042c merge: w23/lane-c (S1: W2-U63-callsites, W2-U39a)
- 998305e0b merge: w23/lane-a (S1: W2-U41a, W2-U07)
- 5c0204198 merge: w23/lane-b (S1: W2-U21a)
- 5c38e2b35 merge: w23/lane-d (S1: W2-U39b, W2-CP-WS7I)

Conflicts: none (all four merges were clean).

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ, quarry-target-integ): `cargo check --workspace --tests` exit 0, warnings only (all pre-existing kinds). Extra check with the production edge features: `cargo check -p quarry-runtime -p quarry-edge --tests --features quarry-edge/postgres-queue,quarry-edge/grpc` exit 0 (covers the postgres-queue-gated W2-U41a families and the main.rs cfg block). No compile fix commits were needed.
- Go: no Go module touched. verevonv3 (SPA and gateway): not touched. action-registry.ts unchanged, so no admission golden regeneration and no new action ids.
- auth-core TS (W2-CP-WS7I) is NOT compile-checked: the integ worktree has no auth-core node_modules and the guard list has no auth-core step. Run `pnpm install` + `tsc --noEmit -p tsconfig.json` and the three jest specs (web-policy, quarry-token-zdr-claims.golden with UPDATE_GOLDENS=1 to confirm the Python-made golden, digest 76cc4af7...) before relying on it.
- Golden, schema and route-count regeneration: not needed by any guard step.

Open items:
- W2-U49a: apply the staged patch in innh-lane-b from a session that may write there, commit in four commits, then merge in the next step.
- W2-U21a: the driver-refusal call sites (agent_routes/step.rs REST and WebSocket, attach.rs) still call failed_step_cost; switch them to failed_step_cost_for_error and drop the dead_code allow.
- W2-U39a: attach.rs, tabs.rs and lifecycle.rs still call the run-less validate_navigation (guard skipped under allow_private_hosts); the egress proxy still refuses those connections.
- W2-U07: edge callers (agent_routes/tabs.rs new_tab, attach.rs open_tab) neither record nor close a tab kept on TIMEOUT (D-U07-1, SR-U07-1 security medium) - must land before WS8.F ATTACH=1.
- W2-U41a: quarry-control's edge_purger.go does not send ?scope=org, so the new families never run (D-U41a-1, high); RUNBOOKS/compose/openapi-internal entries for the three new flags are missing.
- W2-U63: Crawl-delay spacing is enforced on reserved, not actual, fetch starts (owner decision before enforce, X-10). W23-STATUS.md line about wiring the W2-U63 call sites (W2-B batch open items) is now done by this step.
- W2-U39b: renewer stays unwired pending the MP ask (service RenewGrant); W2-CP-WS7I: MP ask to re-vendor the model-gateway golden copy; zdr-contract-sync.test.sh is expected RED on the two model-gateway rows until it lands (not run).
- MP asks added to MP-ASKS.md by W2-U39b and W2-CP-WS7I.
- 42 depth rows appended to W23-depth.md (a 13, b 8 plus the table header, c 7, d 14).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S2 (W2), integrated 2026-11-03

Units: W2-CP-U37 (lane a), W2-U49 and W2-U45 (lane b), W2-U62-66 and W2-CP-INNH10B (lane c), W2-CP-U35 and W2-CP-E2a (lane d). W2-U56 (lane a) is NOT merged: it is BLOCKED with nothing committed. Its openapi.yaml edit sits uncommitted in the innh-lane-a working tree (equal to scratchpad U56/openapi-envelopes.patch), and the two test patches are in the scratchpad only.

Merge commits (git merge --no-ff, base bb5fb9473):
- b33f01689 merge: w23/lane-a (S2: W2-CP-U37; W2-U56 blocked, not committed)
- 3fd805f9b merge: w23/lane-b (S2: W2-U49, W2-U45)
- c17898bf6 merge: w23/lane-c (S2: W2-U62-66, W2-CP-INNH10B)
- ac2756d25 merge: w23/lane-d (S2: W2-CP-U35, W2-CP-E2a)

Conflicts: none (all four merges were clean).

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ, quarry-target-integ): `cargo check --workspace --tests` exit 0, warnings only (pre-existing kinds, none in the new files).
- Go, GOWORK=off: billing-core build + vet clean; quarry-control build + vet clean; org-core build FAILED (W2-CP-E2a: `EnvMasterKeys` declared both as the env-name constant and as the provider type). Fixed in b50ff11d8 (type renamed `EnvMasterKeyProvider`, constant kept); org-core build + vet then clean.
- verevonv3 gateway: `cargo check --tests` (CARGO_TARGET_DIR=%TEMP%/w23cg-gw) exit 0.
- verevonv3 SPA: no src TS changed (only tests/e2e/browser-workspace-live-takeover.spec.ts, which no tsconfig includes); `tsc6 -p tsconfig.app.json` 34 errors = baseline, no new.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No golden, schema or route-count regeneration needed.
- auth-core TS (W2-CP-INNH10B) is NOT compile-checked: the integ worktree has no auth-core node_modules and the guard list has no auth-core step. Before relying on it: pnpm install, `prettier --write --end-of-line auto` on the 6 files, `tsc --noEmit -p tsconfig.json`, `npx jest api-key-metadata` (diff failing sets vs base: 15 pre-existing failures in 3 suites).

Open items:
- W2-U56: finish from a session that may write innh-lane-a (commit openapi.yaml + the two test patches, status file, depth rows), then run sdks/generate.sh and commit sdks/typescript/docs + sdks/python or CI spa-verify fails on drift.
- W2-CP-U37: Control nats.conf needs a quarry-edge-control user + ledger password before QUARRY_EDGE__USAGE_BUS_URL is set (D-U37-1); Lago must register the seven metric codes before USAGE_BROWSER_METRICS (D-U37-3).
- W2-U49: PostgresStepReceiptStore has no purge_for_actor override (501 through the store; edge route uses the pool directly); flag QUARRY_CONTROL_GDPR_USER_RECEIPTS not in compose; route not in RUNBOOKS/OpenAPI.
- W2-U45: edge half of D2-134 (claim org vs body org in scrape_capture.rs) still open.
- W2-U62-66: edge tabs.rs responses still lack openedBy (Quarry ask); keepalive not gated by BROWSER_HUMAN_BROWSE; gateway HTTP tests for human grant create/keepalive missing; Playwright spec unrun (stack session).
- W2-CP-INNH10B: no HTTP route for apiKeys.updateAgent (add POST api-keys/update-agent in src/orpc/auth.controller.ts, W2-FE-GW2); MP ask for the broker to consume api_key_revoked (MP-ASKS.md); INNH10A exchange not on this base.
- W2-CP-U35: snapshot=enforce would bind the observed claim (stamp_policy_snapshot); env var not in SELF_HOST/RUNBOOKS/compose.
- W2-CP-E2a: org-core cmd/server/main.go not wired (ValidateOrgKeysConfig, NewServiceFromEnv, SetAuditPublisher, SetOrgKeys, FlushAudit ticker) so flag-on routes answer 503; org:keys:admin:any and org:keys:destroy:any not allowlisted (403); service_auth.go edited outside the file set; audit-core subject acceptance unchecked.
- 45 depth rows appended to W23-depth.md (a 5, b 10, c 11, d 12, plus 7 W2-U56 rows from its report).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S3 (W2), integrated 2026-11-03

Units: W2-CP-L1 + W2-CP-E2b (lane a), W2-CP-INNH10A + W2-Q-S7a (lane b), W2-DOC-A + W2-CP-S7b (lane c), W2-Q-WS7K + W2-Q-S3 (lane d). All code only, flags default off.

Merge commits (w23/integ, git merge --no-ff, base fa99419a8):
- c76b96097 merge: w23/lane-a (S3: W2-CP-L1, W2-CP-E2b) into w23/integ
- 1a5f6ee2c merge: w23/lane-b (S3: W2-CP-INNH10A, W2-Q-S7a) into w23/integ
- 010879302 merge: w23/lane-c (S3: W2-DOC-A, W2-CP-S7b) into w23/integ
- ec8fc035d merge: w23/lane-d (S3: W2-Q-WS7K, W2-Q-S3) into w23/integ

Conflicts: .plan-work/quarry-w1w2/MP-ASKS.md on the lane-c and lane-d merges (append-only sections); resolved keeping both sides (integ sections first, then the lane's). Lanes a and b merged clean.

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only (pre-existing kinds, e.g. unused imports in tests/browser_grants_prod.rs).
- Go, GOWORK=off, build + vet: billing-core clean; audit-core clean; quarry-control clean; quarry-orchestrator clean. No fixes needed.
- verevonv3 SPA and gateway: untouched by S3, not run.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed.
- auth-core TS (W2-CP-INNH10A) is NOT compile-checked: no auth-core node_modules in the integ worktree and the guard list has no auth-core step. Before relying on it: pnpm install, prettier `--end-of-line auto` on the 4 files, `tsc --noEmit -p tsconfig.json`, `npx jest api-key-exchange plane-token convex-token plane-token-exchange quarry-token-zdr-claims` (diff failing sets vs base: 15 pre-existing failures in 3 suites).

Open items:
- W2-CP-L1: no Postgres test for Service.QuarryLimits; W2-CP-L2 must send X-Quarry-Org-Id and handle 403 organization_inactive; quarry.batch.url has no usage emitter (D-L1-1..7).
- W2-CP-E2b: BLOCKER for the org_core flip (D-E2b-1): envelope_profile_store.rs (~lines 125, 249) must call is_current_key(org_id, &env.key_id) instead of comparing with current_key_id(); profiles_backfill still env-only; owner must copy QUARRY_ORG_CORE_SERVICE_TOKEN and QUARRY_BILLING_CORE_SERVICE_TOKEN into the Ingestion secrets ledger before any flip.
- W2-CP-INNH10A: not compiled (above); OD-11 per-key exchange rate not enforced separately; MP-1/MP-3 asks in MP-ASKS.md.
- W2-Q-S7a: orchestrator workflows/inputs.go (lines 69, 131) still reads params.robots as a string, so override jobs run as respect until it uses activities.RobotsModeFromParam (W23-b-S7a-1); OpenAPI/SELF_HOST/compose lack QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS and effective.robots_override; ship edge before orchestrator.
- W2-CP-S7b: schedule-fired jobs (schedule_runs.go registerScheduledRun) skip the robots audit; AUDIT_CORE_* env for quarry-control not in Ingestion compose.
- W2-DOC-A: scripts/tests/ownership-docs-contract.test.mjs reads the missing apps/master-ownership-matrix.md path (pre-existing); four MP asks; model_plane_gate rows() maps every feature to model-invoke; ai_formats.rs calls /v1/invoke which refuses service principals.
- W2-Q-WS7K: grant-layer gate not wired (admission::validate_run_grant drops the grant; step.rs/tabs.rs/attach.rs do not call the new gate fns); no HTTP tests; browser egress gap for denied sub-resources; Model Plane half in MP-ASKS.md.
- W2-Q-S3: control flag must stay off until gdpr_purge.go deletes schedule_requests, edge schedule_routes.rs is updated (GET requests/{rid}, ZDR refusal), temporal client cleanup, Backfill overlap policy, controlauth.IsControlInternalPath listing; its orchestrator files landed inside commit b4de27454 (WS7K docs commit).
- 53 depth lines appended to W23-depth.md (lanes a, b, c, d).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S4 (W2 + W3), integrated 2026-11-03

Units: W2-Q-U41b, W3-E3.5-4 (lane a); W2-CP-L2, W2-AP-S4c (lane b); W2-U47-69a, W2-AP-S7a (lane c); W2-CP-X9EVENT, W2-FE-GW1 (lane d).

Merge commits on w23/integ:
- 676171253 merge: w23/lane-a (S4: W2-Q-U41b, W3-E3.5-4) into w23/integ
- baa47066b merge: w23/lane-b (S4: W2-CP-L2, W2-AP-S4c) into w23/integ
- 0b74c0754 merge: w23/lane-c (S4: W2-U47-69a, W2-AP-S7a) into w23/integ
- 5eabb6904 merge: w23/lane-d (S4: W2-CP-X9EVENT, W2-FE-GW1) into w23/integ

Conflicts:
- quarry-edge src/internal_routes.rs (lane a vs W2-U49 on integ): module docs and the helper block; kept both (U49 user-receipts docs, path_id_ok and USER_RECEIPTS_FAMILY, then U41b's run_families). Added `key_shred: None` to user_purge_report's FamilyReport (new field from U41b), inside the merge commit.
- .plan-work/quarry-w1w2/MP-ASKS.md on the lane-d merge (append-only sections); kept both, integ first.
- Lanes b and c merged clean.

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only. New one in S4 code: `change_text_fp_enabled` reported never used in the bin test target only (the lib uses it; same dual-module pattern as other edge modules).
- Go, GOWORK=off, build + vet: notification-core clean; org-core clean; quarry-control clean; quarry-orchestrator clean.
- verevonv3 SPA: `pnpm typecheck` stops early in packages/remote-core (no node_modules there: libsodium-wrappers, fzstd; environment, not S4). `tsc6 -p tsconfig.app.json --noEmit`: 34 errors, equal to the baseline, none in src/features/browser/session.
- verevonv3 gateway: `cargo check --tests` (CARGO_TARGET_DIR=%TEMP%/w23cg-gw) exit 0.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed.
- No follow-up fix commits needed.
- Not compile-checked (no guard step): convex-core TS (W2-AP-S7a; run `npx tsx --test convex/agentRuns.browserActivity.test.ts` and a convex typecheck before relying on it).

Open items:
- W2-Q-U41b: BLOCKER for the flag (D-U41b-1): org-core must allow and grant `org:keys:destroy:any` to quarry-edge, or every shredding erasure retries forever; pending set is in-process (D-U41b-2); main.rs/config, compose, openapi-internal, RUNBOOKS not done.
- W3-E3.5-4: docs (openapi.yaml, CHANGE_TRACKING.md, RUNBOOKS.md) not updated; roll orchestrator before the flag (D-E354-1); workflow replay with change-text-fp-v1 not tested; Postgres column accepting `text:` values unchecked.
- W2-CP-L2: lease pool does not use plan render permits (W23-b-L2-1); main.rs comment stale; QUARRY_JOB_CAP_MAX / QUARRY_JOB_BACKLOG_MAX not in compose.
- W2-AP-S4c: migration number 016 clashes with R-32/E3.2 and the Space plan, settle before main; ZDR notices 500 under NOTIFICATION_DELIVERY_WORKER_ENABLED=1; quarry-control emitter must sign x-user-id = schedule created_by; no PG round-trip test.
- W2-U47-69a: same-tick tests rely on Solid 2 rc latest() semantics, run the controller vitest; scheduled crawls skip the enforce-without-scope refusal (lane b inputs.go/scheduled.go).
- W2-AP-S7a: MP ask (nats.conf subscribe mp.v1.trace.run.*); listForOrg/rate leak browserActivity to every org member (high, fix before S7b); confirm Convex skips *.test.ts when bundling.
- W2-CP-X9EVENT: inert until org-core main.go wiring (SetRetentionPostureReader, SetZDRSignal), audit-core provisioner consumer row, control-shared-nats.conf grants, compose flag.
- W2-FE-GW1: `build_router` in apps/gateway/src/main.rs not yet merging `domains::api_key_agent::router` (left for an explicit wiring step; route-count pin may need ROUTE_BASELINE_UPDATE then); MP-3 key-grant door not built; in-memory run binding breaks with several replicas.
- 45 depth rows appended to W23-depth.md (lanes a 10, b 13, c 12, d 10).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S5 (W2 + W3), integrated 2026-11-03

Units: W2-Q-S4a, W3-E3.5-7 (lane a); W2-FE-WS16S7 (lane b; W2-FE-GW2 BLOCKED, nothing committed); W2-AP-S7b (lane c; W2-FE-WS9Ga BLOCKED, nothing committed); W3-E3.1-ab, W3-E3.1-g (lane d).

Merge commits on w23/integ:
- 5511d622c merge: w23/lane-a (S5: W2-Q-S4a, W3-E3.5-7) into w23/integ
- 86ac0e7c4 merge: w23/lane-b (S5: W2-FE-WS16S7) into w23/integ
- 5a73044fe merge: w23/lane-c (S5: W2-AP-S7b) into w23/integ
- cb564c9aa merge: w23/lane-d (S5: W3-E3.1-ab, W3-E3.1-g) into w23/integ

Conflicts: none.

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only (all in files outside S5 or pre-existing). The FillOtpRef arms added in quarry-core/quarry-edge/quarry-runtime compile.
- Go, GOWORK=off, build + vet: Quarry-v2/pkg/quarrycontracts clean; Quarry-v2/services/quarry-control clean; integration-corev2 clean.
- verevonv3 SPA: no TS change in S5 (the two FE units that would have changed it are blocked), so no typecheck run.
- verevonv3 gateway: `cargo check --tests` (CARGO_TARGET_DIR=%TEMP%/w23cg-gw) exit 0; one unused-import warning in domains/browser.rs (not S5).
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed.
- No follow-up fix commits needed.
- Not compile-checked (no guard step, no node_modules in integ): convex-core TS (W2-AP-S7b; run `npx tsx --test convex/nats.browserActivity.test.ts` and a convex typecheck before relying on it).

Open items:
- W2-FE-GW2: BLOCKED (harness different-worktree guard). Drafts in the vigorous-perlman scratchpad `w2-fe-gw2/` (action-registry insert, settings-client insert, ApiKeyAgentSection.tsx + test). Rerun from a lane-b session or copy in; registering the action as an ActionDescriptor needs an owner decision (MODEL_ACCESS row, 240 count, admission golden). Gateway agent-policy/agent create/patch routes and CP `api-keys/update-agent` REST route still missing.
- W2-FE-WS9Ga: BLOCKED (same guard). Drafts `ws9ga-*` in the vigorous-perlman scratchpad; re-check use-chat-shortcuts.ts against the lane HEAD before copying; flag VITE_CHAT_BROWSER_DOCK off.
- W2-Q-S4a: orchestrator does not send target_kind/enabled nor read the new fields; Quarry-v2/deploy/compose/docker-compose.yml not edited; RUNBOOKS/openapi not updated; control pause route accepting the edge's signed forward unchecked live; QUARRY_ORG_CORE_SERVICE_TOKEN must be mirrored by hand from Control's ledger.
- W3-E3.5-7: persistence (store field, pg column, migration 029) and API fields missing, so no filter applies yet; resources.go change_detected batch path still fans out unfiltered (must fix before any filter is enabled); no producer for `new`/`removed`; docs and compose line for QUARRY_EDGE__CHANGE_WEBHOOK_DELTA.
- W2-FE-WS16S7: schedules.rs build_schedule_config copies schema unchecked; quarry-core json_schema treats a non-object schema as permissive (depth rows -1, -2).
- W2-AP-S7b: nats-subscriber.js subscription and convex/http.ts case not wired (inert); listForOrg/rate browserActivity leak (from S7a) must be fixed before the flag; producer run_id assumption unconfirmed.
- W3-E3.1-ab: MP ask (browser-broker grant.go fill_otp) appended to MP-ASKS.md; admission.rs require_action_capabilities lacks FillOtpRef; origin binding and restart-safe single-use set open; OpenAPI/TS SDK lack fill_otp_ref; runtime refuses the action until E3.1-c/d.
- W3-E3.1-g: inert until server.go fills ExecuteInput.Caller, SetOTPAuditPublisher is wired, requiredCapabilityForOperation maps mail.otp.read, auth middleware admits integration:otp:read for that route; owner decision on the personal-connection owner rule; no tests.
- 39 depth rows appended to W23-depth.md (lanes a 15, b 5, c 6, d 13).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S6 (W2 + W3), integrated 2026-11-03

Units: W2-FE-WS9Gb, W3-E3.5-6 (lane a); W3-E3.7.7a (lane b; W3-E3.2-g1 BLOCKED, nothing committed); W3-E3.4-8 (lane c; W2-Q-U67a-U03 BLOCKED, nothing committed); W2-Q-S5, W3-E3.8.1 (lane d).

Merge commits on w23/integ:
- f5e7e0ef9 merge: w23/lane-a (S6: W2-FE-WS9Gb, W3-E3.5-6) into w23/integ
- c8728f7cc merge: w23/lane-b (S6: W3-E3.7.7a) into w23/integ
- 5b42e5e83 merge: w23/lane-c (S6: W3-E3.4-8) into w23/integ
- 82b427926 merge: w23/lane-d (S6: W2-Q-S5, W3-E3.8.1) into w23/integ

Conflicts: none.

Compile guard (no test runs):
- Quarry Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only (none in S6 code: search_routes.rs, ingest_client.rs, error.rs, fallback_driver.rs).
- Go, GOWORK=off, build + vet: Quarry-v2/services/quarry-control clean; Quarry-v2/services/quarry-orchestrator clean.
- verevonv3 SPA (tsc6 -p tsconfig.app.json): after the merges 38 errors = 34 baseline + 4 new, all W2-FE-WS9Gb imports of the missing W2-FE-WS9Ga modules (browser-dock-state, BrowserDockControls, toggleBrowserDock option). Fixed by follow-up commit 0f210509a, which lands the W2-FE-WS9Ga drafts (`ws9ga-*` from the vigorous-perlman scratchpad; the use-chat-shortcuts.ts change was additive against HEAD). Re-check: 0 errors in any S6/Ga file. That re-check ran against epic-shaw's node_modules (the vigorous-perlman node_modules the integ junction pointed at had disappeared mid-step; the integ junction now points at epic-shaw-4d613b's node_modules). In that environment the only remaining error is the pre-existing src/shared/api/schedule-contract.test.ts TS2352 (not touched in S6); the quarry-client/remote-core baseline errors do not appear there because those packages are built.
- verevonv3 gateway: not changed in S6, no check.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed.

Open items:
- W2-FE-WS9Ga: now landed on integ through the compile fix only. It has no status file of its own; its tests (browser-dock-state, BrowserDockControls, use-chat-shortcuts.dock) and Gb's tests and Playwright spec are written, not run. Flag VITE_CHAT_BROWSER_DOCK default off.
- W3-E3.2-g1: BLOCKED (harness different-worktree guard). Planned change set in the unit report (registry ids browser.get_org_policy / browser.set_org_policy, model-access rows, gateway proxy_json_with_if_match + domains/browser_policy.rs, dispatch arms, admission golden regen). Rerun from a lane-b session.
- W2-Q-U67a-U03: BLOCKED (same guard). Drafts `draft/attach_enabled.rs` and `draft/spike_ws8a.rs` in the vigorous-perlman scratchpad; copy into quarry-edge/tests and quarry-browser/tests, compile-check, commit. Open decision: POST /v1/agent/runs and /v1/agent/procedures/* not covered by the I5 table.
- W3-E3.5-6: API routes (create, PATCH, ?tag=) not built; Rust edge drops source tags and ignores ?tag= (D-E356-1/2); owner decision one tag vs a set (D-E356-4); DSN-gated pg tests need QUARRY_TEST_DSN.
- W3-E3.7.7a: page fetcher follows redirects without re-checking hosts (W23-b-E377a-1, medium, shared with include_answer and /v1/answer); OpenAPI/CONTRACTS left for E3.7.11.
- W3-E3.4-8: not wired (E3.4-4/-6/-7); gRPC ingest adapter hard-codes type="web"; Data Plane owner to confirm the four download_* document types.
- W2-Q-S5: blocks the scheduled-jobs flip (last schedule of an org never reaped), the request-path flip (edge maps control 409/501 to 502 DRIVER_FAILED), and enforce with no org-core token denies every fire; docs-contract.test.mjs not run.
- W3-E3.8.1: no producer names dns/tls/connect yet (all chains report all_drivers_failed); merge lane d before E3.8.2 (lane a also edits fallback_driver.rs); owner decision on Temporal retries for all_drivers_failed and tls; openapi/CONTRACTS/compose/RUNBOOKS entries missing.
- 58 depth rows appended to W23-depth.md (lanes a 24, b 5, c 5, d 24).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S7 (W2 + W3), integrated 2026-11-03

Units: W3-E3.8.2 (lane a; W2-FE-WS9H NOT LANDED, drafts only in the vigorous-perlman scratchpad ws9h-drafts/, nothing committed); W3-E3.3-e1, W3-E3.4-4 (lane b); W3-E3.1-d, W3-E3.6-5 (lane c); W3-E3.2-g2, W3-E3.2-e3 (lane d). All code only, flags default off.

Merge commits on w23/integ (base e30b70f82), no conflicts:
- 83cd6612f merge: w23/lane-a (S7: W3-E3.8.2)
- 0b640dd46 merge: w23/lane-b (S7: W3-E3.3-e1, W3-E3.4-4)
- 7b780d6ca merge: w23/lane-c (S7: W3-E3.1-d, W3-E3.6-5)
- 13379850a merge: w23/lane-d (S7: W3-E3.2-g2, W3-E3.2-e3)
- 6f18a23cc fix(quarry-edge): drop the unused ArtifactStore import from the download settle path (compile-guard follow-up; it was a new warning, not an error)

Compile guard (no test runs):
- Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only; the one new S7 warning (step.rs unused ArtifactStore import) fixed in 6f18a23cc.
- Go, GOWORK=off, build + vet: apps/Control Plane/user-core clean.
- verevonv3 SPA (tsc6 -p tsconfig.app.json): 1 error, the pre-existing src/shared/api/schedule-contract.test.ts TS2352; 0 errors in BrowserPolicySection(.test).tsx. `pnpm typecheck` stops in the @verevon/remote-core build (libsodium-wrappers / fzstd modules not installed in this node_modules), pre-existing environment, not S7.
- verevonv3 gateway: not changed in S7, no check.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration run (the E3.6-5 schema goldens are known stale, see open items).

Open items:
- W2-FE-WS9H: NOT LANDED (different-worktree guard). Apply from an innh-lane-a session: copy ws9h-drafts/ new files into src/features/browser/dock/ (CSS to CRLF), git apply ws9h-chatpanels.patch and ws9h-chatpage.patch, commit, typecheck. Flag VITE_CHAT_BROWSER_DOCK default off.
- W3-E3.8.2: no production caller passes FetchPolicy (budget is always the 30 s default with QUARRY_EDGE__FETCH_DEADLINE on); reasons tier_stalled / fetch_deadline_exceeded not yet in quarry-core error.rs, Go mirror, ERROR_REASONS.md; compose passthrough and RUNBOOKS missing; browser lease release on a stalled tier unverified (D-E382-3).
- W3-E3.3-e1: rect never produced; needs BrowserDriver::target_box + DOM.getBoxModel in chromiumoxide.rs behind a default-off flag; CONTRACTS/openapi/SDK docs.
- W3-E3.4-4: refused downloads stay in artifact storage (no delete); ledger not released on run close; delegate step path ungated; driver still quarantines unsolicited downloads until E3.4-6.
- W3-E3.1-d: observation.rs FillOtpRef arm still Unsupported; E3.1-c adapter onto OtpFillTarget; mail connection id source.
- W3-E3.6-5: HIGH crawl_scrape.rs UNSUPPORTED_KEYS lacks redact_pii/redactPii (crawl runs unredacted silently in lenient mode); docs/schemas goldens stale (UPDATE_SCHEMAS=1 needed before the jsonschema-feature test); OpenAPI parity script; owner to confirm non-ZDR refusal (requires_zdr).
- W3-E3.2-g2: hidden until E3.2-g1 registers browser.get_org_policy / browser.set_org_policy (still BLOCKED); not mounted in WorkspaceSettingsPage.tsx; Norwegian copy unreviewed.
- W3-E3.2-e3: migration 044 never applied to a real DB; RLS inert under a superuser role; no reader of the decisions (broker, Quarry, SPA); no gateway proxy; DSAR export likely misses user_settings.
- 46 depth rows appended to W23-depth.md (lanes a 5, b 12, c 14, d 15).
- Four tracked .sha256 files in the integ worktree still show as modified (CRLF/LF checkout artifacts); none committed.

## Step S8 (W3), integrated 2026-11-03

Units: W3-E3.1-c, W3-E3.6-4a (lane a); W3-E3.3-j, W3-E3.8.3 (lane b); W3-E3.4-1 (lane c; W3-E3.3-a NOT LANDED, see open items); W3-E3.1-h, W3-E3.2-f (lane d). All code only, flags default off (E3.4-1 has no flag; it adds stored statuses nothing writes yet).

Merge commits on w23/integ (base 1b2bbc9ae), no conflicts:
- 5d8bc888a merge: w23/lane-a (S8: W3-E3.1-c, W3-E3.6-4a)
- b0513b184 merge: w23/lane-b (S8: W3-E3.3-j, W3-E3.8.3)
- 0a9525c76 merge: w23/lane-c (S8: W3-E3.4-1)
- b32273bb0 merge: w23/lane-d (S8: W3-E3.1-h, W3-E3.2-f)
- The lane-d merge was first refused by the line-ending-only auth-core cross-plane-token-requests.json.sha256 artifact; its bytes equal HEAD, `git update-index --refresh` cleared it (and the other three artifacts), nothing restored or stashed.

Compile guard (no test runs):
- Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0, warnings only, all pre-existing quarry-edge ones; no new warning from S8 files. Extra: `cargo check -p quarry-browser -p quarry-runtime --features quarry-browser/chromiumoxide,quarry-runtime/chromiumoxide --tests` exit 0, no warnings (covers the E3.1-c chromiumoxide.rs code).
- Go, GOWORK=off, build + vet: apps/Control Plane/org-core clean.
- auth-core TS (web-policy.ts, origin-consent spec, golden spec): not type-checked (not in the guard list); verevonv3 SPA and gateway not changed in S8, no check.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed. Manifest digest (cross-plane-token-requests 6e5f14b3...) and quarry token claims golden (6c3044d5...) changed only in lane d, so no cross-lane digest merge was needed.

Open items:
- W3-E3.3-a: NOT LANDED (different-worktree guard refused lane-c writes). Patches in the vigorous-perlman scratchpad e33a/spa.patch and e33a/gateway.patch; apply from an innh-lane-c session with `patch --binary -p1` (not git apply, CRLF), commit the three commits named in its report, then gateway cargo check + tsc. Its six depth rows are not in W23-depth.md yet.
- W3-E3.1-c: the OTP methods are inherent ChromiumoxideDriver methods, unreachable through Arc<dyn BrowserDriver> (needs trait methods in lib.rs, D-E31c-1 high); nothing reads the credential-delivered mark (E3.1-e, D-E31c-2 high); no trusted grant origin until the broker carries it (D-E31c-7 high); the http /otp-login fixture fails otp_not_https (D-E31c-3).
- W3-E3.6-4a: routes.rs still calls run_with_inline_request, so per-request redact_pii does not reach the stage (D-E364a-2); edge caches ignore the org default (D-E364a-3); a caught scanner panic can print page text through the panic hook (D-E364a-1).
- W3-E3.3-j: the queue row's flag name QUARRY_BROWSER_SETTLE_MS is the live hydration budget; the unit uses QUARRY_BROWSER_ACTION_SETTLE(_MS) instead (queue row to correct); openapi/SDK/SELF_HOST docs for settle_ms missing.
- W3-E3.8.3: no retry while QUARRY_PROXY_POOL is set; stream and crawl run-page paths not wrapped; flag not in compose/RUNBOOKS/openapi; E3.8.1 sub-codes not produced yet so dns/connect retries are dormant.
- W3-E3.4-1: migration is 0020 (0019 reserved for W2-U49a, not on this branch, so the sequence has a gap until it lands); edge run_is_resumable still treats only Active as resumable; OpenAPI/SDK/gateway status values not widened; roll every edge before the TTL flag is on.
- W3-E3.1-h: nothing constructs IntegrationCoreOtpSource (no edge config/wiring); integration-core route does not pass the verified caller; personal-connection owner decision open.
- W3-E3.2-f: MP asks appended (re-vendor model-gateway golden copy + pin; accept unknown web_policy.caps keys; broker origin-extension grants honour both keys); the model-gateway rows of zdr-contract-sync fail until that lands; nothing enforces the keys yet.
- 50 depth rows appended to W23-depth.md (lanes a 13, b 22, c 4, d 11).
- Four tracked .sha256 files are clean after the index refresh; none committed.

## Step S9 (W3), integrated 2026-11-03

Units: W3-E3.3-h, W3-E3.7.1 (lane a); W3-E3.1-e, W3-E3.4-2-3 (lane b); W3-E3.6-4b (lane c; W3-E3.2-b-swap NOT LANDED, see open items); W3-E3.2-e1, W3-E3.5-3 (lane d). All code only, flags default off (QUARRY_EDGE__HIT_TEST, QUARRY_EDGE__BATCH_IDEMPOTENCY, QUARRY_EDGE__RUN_TTL, QUARRY_EDGE__PII_REDACTION reused, QUARRY_EDGE__ORIGIN_CONSENT, QUARRY_EDGE__JOB_WEBHOOK, QUARRY_CONTROL__JOB_WEBHOOK). Not flagged: migration 028 (org-scoped job Idempotency-Key, a cross-org bug fix) and the E3.1-e read denial (only withholds more; its trigger fill_otp_ref stays behind QUARRY_EDGE__FILL_OTP).

Merge commits on w23/integ (base 2c3ce34a4):
- 2cd162156 merge: w23/lane-a (S9: W3-E3.3-h, W3-E3.7.1)
- d9b3f458c merge: w23/lane-b (S9: W3-E3.1-e, W3-E3.4-2-3)
- 93632ea98 merge: w23/lane-c (S9: W3-E3.6-4b)
- 94515ab58 merge: w23/lane-d (S9: W3-E3.2-e1, W3-E3.5-3)
- One conflict: .plan-work/quarry-w1w2/MP-ASKS.md (lane b E3.4-2-3 and lane d E3.2-e1 both appended a section at the end); kept both, lane b first, CRLF kept.

Compile guard (no test runs):
- Rust (qtest-lane integ): `cargo check --workspace --tests` exit 0; 33 warnings, all pre-existing (the agent_policy_gate.rs dead-code ones come from W2-Q-WS7K c3f894d41, run_live.rs:519 fetch_update deprecation from 6efc33cc9); no warning from S9 code. Extra: `cargo check -p quarry-browser -p quarry-runtime --features quarry-browser/chromiumoxide,quarry-runtime/chromiumoxide --tests` exit 0, no warnings (covers the E3.3-h hit_test_point code).
- Go, GOWORK=off, build + vet: apps/Ingestion Plane/Quarry-v2/services/quarry-control clean (exit 0 both).
- No TS, verevonv3 gateway or action-registry.ts change: no tsc/gateway check, no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed (the guard compiled; the new hit-test route is not in the ZDR matrix yet, D-E33h-9).
- No follow-up fix commits were needed.

Open items:
- W3-E3.2-b-swap (lane c): NOT BUILT. The unit agent refused to write into innh-lane-c (different-worktree guard) and did not use the python fallback; nothing written. Rerun from a session that can write lane c (planned change in its report: flag VITE_BROWSER_APPROVAL_CARD, ChatMessages.tsx + BrowserChrome.tsx; MP ask for execution-core gate_risky_action redirect text + 1000-char cap not yet appended).
- W3-E3.3-h: route dark even with the flag on (501) until a BrowserDriver trait method + BrowserPointHit land in quarry-browser lib.rs (ask 1); backend node id is exposed in the public fingerprint_id (D-E33h-1, ask 2 accessor in observation.rs); ZDR matrix/OpenAPI/SDK rows missing (ask 3).
- W3-E3.7.1: migration 028 not applied anywhere; Idempotency-Key not covered by the internal HMAC (D-E371-1); crawl replay with a different URL re-attaches silently (D-E371-2); createJob and memory store untouched (D-E371-3).
- W3-E3.1-e: runtime stores screenshot/visual artifacts and emits ObservationReady before the edge strips (high, observation.rs); attach.rs delegate_step skips the check (high); marks not released on close/sweep; SPA route change counts as a new page.
- W3-E3.4-2-3: types.rs run_is_resumable still treats only active as resumable (blocks QUARRY_EDGE__RUN_TTL); close.rs drain does not mark suspended; checkpoint has no timestamps; wire/OpenAPI/SDK lack suspended/expired; MP ask (execution-core resume_run_id) appended.
- W3-E3.6-4b: per-request redacting runs still render a page PNG into the CAS (D-E364b-1 high; pipeline.rs must skip and wrap in PiiEnforcingStore); ingest_client.rs no sink check; JSON number-run false positives unmeasured.
- W3-E3.2-e1: no route calls admit_origin_extension; extension checked once, not per step (RunEntry in types.rs) - condition for flag on; other lanes/tabs do not get the widening; MP ask (origin_extension grant kind) appended; OD13 counsel review still gates the flag.
- W3-E3.5-3: crawl handoff (routes.rs) does not save req.webhook, so the edge flag must stay off; batch not wired; control create does not call NormalizeJobWebhook; no delivery path; docs/compose/SDK not updated.
- 38 depth rows appended to W23-depth.md (lanes a 14, b 9, c 4, d 11).
- The four line-ending-only .sha256 artifacts were not staged.

## Step S12 (W3), integrated 2026-11-03

Units: W3-E3.3-g (lane b, landed). NOT LANDED (no lane commits, nothing to merge): W3-E3.4-6 and W3-E3.7.4-6w (lane a), W3-E3.7.9b (lane b), W3-E3.9.6-7 and W3-E3.3-c (lane c), W3-E3.7.10-CP (lane d). Lanes a, c and d had no new commits; their units were blocked by the pre-commit gitleaks hook while the Docker engine was down (lane a, c, d) or by the different-worktree write guard (E3.7.9b). Flag of the landed unit: VITE_CHAT_BROWSER_STEER (default off).

Merge commits on w23/integ (base 5d4143522):
- 6fb6a2902 merge: w23/lane-b (S12: W3-E3.3-g). No conflicts.

Follow-up commits:
- f58d2ddc3 chore(contracts): regenerated agent-action admission golden (VEREVON_UPDATE_GOLDEN=1 admission-contract.test.ts, 5/5), copied byte-identically to the four Model Plane copies, sha256 pin f0dd9c5731f0f89fd95f16f167d3cbfe3490b544e876bada1dc7b9cf2b33e592 written to agent-action-admission.json.sha256 and scripts/tests/zdr-contract-sync.test.sh. New row: browser_run.steer, access none (exempt no_owner_executor, context_write). Model Plane owner: new action id browser_run.steer.
- effcd5737 fix(frontend): steer composer typings (add/change setters returned the array where void was declared; onInput arrow returned a string; maxLength -> maxlength for Solid textarea typings).

Compile guard (no test runs):
- Quarry Rust: not run, the step changed no Quarry Rust (only lane b frontend files merged).
- Go: no Go module touched.
- TS: tsc6 -p tsconfig.app.json --noEmit: 3 new errors from E3.3-g (steer-input.ts x2, SteerInput.tsx x1), fixed in effcd5737; after the fix 1 error remains, src/shared/api/schedule-contract.test.ts(28,16) TS2352, pre-existing and not from this step (whole-project count is now 1, below the old 34 baseline). `pnpm typecheck` stops earlier at the @verevon/remote-core build (missing libsodium-wrappers/fzstd in this worktree's node_modules), pre-existing and unrelated.
- verevonv3 gateway: not changed, no cargo check.
- No ZDR/schema/route-count regeneration needed.

Open items:
- Re-run the six not-landed units once Docker is up / from sessions that can write their lanes: lane a has E3.4-6 (patch .plan-work/quarry-w1w2/W23-W3-E3.4-6-chromiumoxide.patch, commit order behind E3.3-k and E3.8.7) and E3.7.4-6w uncommitted in innh-lane-a; lane c has E3.9.6-7 and E3.3-c uncommitted (E3.3-c files staged in the lane index); lane d has E3.7.10-CP uncommitted; E3.7.9b exists only as a scratchpad file (vigorous-perlman scratchpad 4b2ee830.../CrawlApprovalPrompt.tsx).
- W3-E3.3-g: no gateway arm for browser_run.steer (registry-only); page wiring (controller per run, settle/markDelivered) not built; MP ask (steer inbox route, browser_run.steer_taken event) appended to MP-ASKS.md; Model Plane copies of the admission golden are re-vendored here, the MP owner must accept the row.
- 8 depth rows appended to W23-depth.md (lane b 8).
- The four line-ending-only .sha256 artifacts were not staged.

## Step S13 (W3), integrated 2026-11-03

Units: W3-E3.7.3 and W3-E3.6-6b (lane a). Flags: QUARRY_EDGE__JOB_CONTROL (existing, default off; gates the new `wait`/`resume` on GET /v1/jobs|crawl|batch/:id) and QUARRY_EDGE__OFFICE_VIA_IMPORTS (new, default off; only effective with QUARRY_EDGE__DOC_ROUTING=on).

Merge commits on w23/integ (base dc6c316e5):
- ce2b5f8db merge: w23/lane-a (S13: W3-E3.7.3, W3-E3.6-6b). No conflicts.

Follow-up commits:
- 240fc644d fix(quarry-runtime): E3.6-6b calls `content_route::extract_office_via_imports`, which E3.6-6a (on no branch) was to add. Added a placeholder with that name and signature in content_route.rs that refuses every body with the typed 415 (UnsupportedMedia, details.reason imports_client_unavailable, route imports). Flag off: unchanged. Flag on before 6a: Office kinds keep a 415, as today. E3.6-6a must replace this body (expect a conflict on that function when 6a merges).

Compile guard (no test runs):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): first run exit 101, 1 error (E0425 extract_office_via_imports not found in content_route, pipeline.rs:320); after 240fc644d exit 0, no warnings in the touched files.
- Go: no Go module touched.
- TS: not changed (action-registry.ts untouched, no admission golden regeneration, no new action ids).
- verevonv3 gateway: not changed.
- No ZDR/schema/route-count regeneration needed.

Open items:
- E3.6-6a still unbuilt: real imports-core client (POST /api/v1/import/extract, verified ingestion token, 400/413/415/422 mapping); ODF/EPUB are 415 in imports-core; PPTX 60k char cap.
- E3.7.3: no wait test in tests/job_lifecycle.rs; gateway does not forward wait/resume; OpenAPI/CONTRACTS/SDK in E3.7.11; owner to confirm that only the wait (not cancel/pause/resume) is non-ZDR; 408 reuses TIMEOUT.
- Unit tests of both units compiled but not run.
- 22 depth rows appended to W23-depth.md (lane a 22: E3.7.3 11, E3.6-6b 11).
- The four line-ending-only .sha256 artifacts were not staged.

## Step S14 (W3), integrated 2026-11-03

Units: W3-E3.8.8 (lane a) and W3-E3.7.11 (lane c). Flags: QUARRY_EDGE__NO_EGRESS (new, default off; only 1/true/on enable it; nothing builds the driver yet, so no request path changes). E3.7.11 is docs only and adds no flag.

Merge commits on w23/integ (base d3cc76b5a):
- 6851552a8 merge: w23/lane-a (S14: W3-E3.8.8). No conflicts.
- 51787421a merge: w23/lane-c (S14: W3-E3.7.11). No conflicts.

Follow-up commits: none (the guard was green on the first run).

Compile guard (no test runs):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): exit 0 on the first run; no warnings in data_plane_http.rs or fetch.rs (the 33 warnings are pre-existing, in quarry-edge).
- Go: no Go module touched.
- TS: not changed (action-registry.ts untouched, no admission golden regeneration, no new action ids).
- verevonv3 gateway: not changed.
- No ZDR/schema/route-count regeneration needed.
- openapi.yaml was not parsed (no YAML parser available locally; node parity tests not run).

Open items:
- E3.8.8: Data Plane owner must accept or change the proposed POST /v1/artifacts/lookup contract and pick the store for page captures; token-request manifest row + Auth Core registry entry for the artifacts read; edge wiring (NoEgressDriver::from_env, route no_egress scrapes, ADR topic 1 catalogue key); quarry-core driver kind for stored reads; decision on robots/sitemap discovery under no_egress; run no_egress_tests, artifact_reader_tests, log_hygiene and the manifest tests at the test phase.
- E3.7.11: SDKs not regenerated (needs the openapi-generator 7.22.0 Docker image); openapi parse and node parity tests not run; E3.7.3 408 envelope, E3.7.4 /v1/map gzip and E3.7.7b answer reuse documented as not served.
- 14 depth rows appended to W23-depth.md (lane a 6, lane c 8).
- The four line-ending-only .sha256 artifacts were not staged.

## W3 finish, lane d, integrated 2026-11-04

Units: W3-E3.2-e2 (339edcf36), W3-E3.1-i (9291e09de), W3-E3.1-j (d2a069911), W3-E3.7.10-CP (f75a24382), plus lane d finish depth rows (a461698ae). Flags (all default off): QUARRY_EDGE__ORIGIN_CONSENT (E3.2-e2), VITE_BROWSER_OTP_APPROVAL_CARD (E3.1-i), VEREVON_GATEWAY_AGENT_WEB_TOOLSET (E3.7.10-CP). E3.1-j has no flag of its own; a real run needs QUARRY_EDGE__FILL_OTP and INTEGRATION_OTP_READ.

Merge commit on w23/integ (base 9bea43473):
- e762de7c5 merge: w23f/lane-d (W3 finish: W3-E3.2-e2, W3-E3.1-i, W3-E3.1-j, W3-E3.7.10-CP) into w23/integ. No conflicts (the lane had already resolved the MP-ASKS.md append clash against 9bea43473: main's E3.3-g/E3.3-l sections first, then lane d's E3.1-i/E3.7.10-CP sections).

Follow-up commits: none for code (the guard was green on the first run).

Compile guard (no test runs):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): exit 0 on the first run; 33 warnings, all pre-existing (the one in tab_ledger.rs, unused import LaneState at line 21, is on the base already).
- Go: no Go module touched.
- TS verevonv3 (tsc6 -p tsconfig.app.json --noEmit): 1 error, the pre-existing baseline (src/shared/api/schedule-contract.test.ts TS2352); no new errors from OtpApprovalNotice.tsx or otp-approval.ts.
- verevonv3 gateway (cargo check --tests): exit 0; 1 warning, pre-existing (unused import ResponseTemplate in src/domains/browser.rs), none in actions/handlers.rs.
- auth-core TS: not checked (no node_modules in the integ worktree); prettier not run on plane-service-principals-policy.contract.spec.ts.
- action-registry.ts unchanged: no admission golden regeneration, no new action ids. No ZDR/schema/route-count regeneration needed.
- otp-login-fixture.sh: only bash -n and node --check in the lane; not run.

Open items:
- Decision needed: execution-core's quarry token is ZDR and quarry-edge refuses durable jobs under ZDR, so crawl:write can never start a job on that token even once the edge routes service principals.
- quarry-edge still refuses service principals on the job routes and on /v1/crawl and /v1/batch; the Control rows (jobs:read/jobs:control, crawl:write, map:read) are inert there except map:read.
- The four web.* ids have no action-registry entries (owned by W3-E3.7.9a); web.crawl_site fails closed with 403 crawl_approval_required until E3.7.9a lands.
- E3.2-e2: no route calls the consent check (Model Plane attach driver missing, ask in MP-ASKS.md); finish_attach would close the person's own tab in attach mode.
- E3.1-i: nothing mounts the OTP card until the E3.2-b swap. E3.1-j: blockers B1 to B4 keep the OTP chain inert; script written, not run.
- Gateway toolset flag is read with std::env::var per request (convention is AppState).
- 38 depth rows appended to W23-depth.md (E3.1-i 10, E3.1-j 14, E3.2-e2 4, E3.7.10-CP 9, lane d finish 1).
- The four line-ending-only .sha256 artifacts were not staged.

## W3 finish, lane b, integrated 2026-11-04

Units: W3-E3.4-5 (1c1c0e754, fix ed25081ad), W3-E3.4-7 (db3a3c566), W3-E3.6-7 (47ff80b0f), W3-E3.8.5 (f087c9d15), W3-E3.7.9b (6b6bf9981), plus lane b depth rows (cce283a7f). Flags (all default off): QUARRY_EDGE__LIVE_VIEW_ONLY and BROWSER_LIVE_VIEW_ONLY (E3.4-5), VITE_BROWSER_DOWNLOADS_DRAWER (E3.4-7), QUARRY_EDGE__OCR and QUARRY_EDGE__PII_MODEL (E3.6-7), VITE_KNOWLEDGE_CRAWL_APPROVAL_ENABLED (E3.7.9b). E3.8.5 has no flag of its own; its codes only arrive with QUARRY_EDGE__FETCH_SUB_CODES on.

Merge commit on w23/integ (after lane d, 55d960f74):
- e1226bffe merge: w23f/lane-b (W3 finish: W3-E3.4-5, W3-E3.4-7, W3-E3.6-7, W3-E3.8.5, W3-E3.7.9b) into w23/integ. No conflicts.

Follow-up commits:
- 2e465238a chore(contracts): admission golden regenerated (VEREVON_UPDATE_GOLDEN=1 admission-contract test, 5 passed); copied byte-identically to apps/Model Plane/api/contracts and the capability-core testdata, execution-core and model-gateway fixtures; sha pin and zdr-contract-sync.test.sh line moved 0df146d9... -> 091abfa2....
- aa68cecbb fix(verevonv3): DownloadsDrawer.tsx read `current.state` on the union RowState (TS2339); narrowed with `'state' in current`.

New action ids: browser_download.send_to_knowledge (admission row access: none, exemptReason no_owner_executor, zdr_allowed false).

Compile guard (no test runs, except the golden writer):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): exit 0 on the first run; 33 warnings, all pre-existing (none in live.rs, ws.rs or mp_client.rs).
- Go: no Go code touched (only the vendored capability-core testdata JSON).
- TS verevonv3 (tsc6 -p tsconfig.app.json --noEmit): first run 2 errors (1 new in DownloadsDrawer.tsx, fixed in aa68cecbb); after the fix 1, the pre-existing baseline (schedule-contract.test.ts TS2352).
- verevonv3 gateway (cargo check --tests): exit 0; 1 warning, pre-existing (unused import ResponseTemplate in src/domains/browser.rs, not a lane b line).
- auth-core TS: not changed, not checked.
- No Quarry ZDR/schema/route-baseline regeneration done: no test run here shows a need (the new gateway route /api/v1/browser/sessions/{id}/view/frames/ws is not yet in the ZDR route matrix, depth W23-b-E345-5).

Open items:
- E3.4-7: no gateway arm or Ingestion owner route serves browser_download.send_to_knowledge; the registry-to-dispatcher parity test may flag it (as browser_run.steer); no projection reports downloads; nothing mounts the drawer.
- E3.7.9b: nothing sets the crawlApproval signal until W3-E3.7.9a (lane a) lands; startCrawlJob has no prompt path.
- E3.4-5: no view-only SSE route (proxy_sse_stream has no extra-header parameter); REST /step still takes no scope.
- E3.6-7: OCR/PII lanes are client-only, no caller in the pipeline.
- E3.8.5: fast dns/tls static retry (ask 4) open.
- None of the new flags are in compose or the runbooks.
- 28 depth rows appended to W23-depth.md (E3.4-5 5, E3.4-7 5, E3.6-7 7, E3.8.5 7, E3.7.9b 4); W23-b-E347-3 (golden) is done by 2e465238a.
- The four line-ending-only .sha256 artifacts were not staged.

## Step: W3 finish, lane a

Units (branch w23f/lane-a, base 9bea43473): W3-E3.3-k (7486b7eb3, docs 7bc5d63af), W3-E3.7.2 (5c7fa4b91, docs 482fd103e), W3-E3.8.7 (a37837d12, docs 16e5e6c5e), W3-E3.7.9a (507daffcd, docs bbaebd09d), W3-E3.4-6 (121903141, docs ff66fd481), W3-E3.7.4-6w (f865de101, docs e1095a0f1), W2-FE-WS9H (b804dec7d, docs 4a693694e), plus lane a depth rows (e6338ec42). Flags (all default off or observe): QUARRY_BROWSER_AX_PER_FRAME (E3.3-k), QUARRY_BATCH_PARALLEL (E3.7.2), QUARRY_EDGE__REPUTATION_MODE (E3.8.7), CRAWL_APPROVAL_MODE=observe and CRAWL_APPROVAL_PAGES=50, lower only (E3.7.9a), QUARRY_EDGE__DOWNLOADS (E3.4-6), QUARRY_ORCHESTRATOR__CANONICAL_FRONTIER and QUARRY_EDGE__MAP_GZIP_SITEMAP (E3.7.4-6w), VITE_CHAT_BROWSER_DOCK (WS9H).

Merge commit on w23/integ (after lane b, b27dde3c5):
- 9a78e1ba4 merge: w23f/lane-a (W3 finish: W3-E3.3-k, W3-E3.7.2, W3-E3.8.7, W3-E3.7.9a, W3-E3.4-6, W3-E3.7.4-6w, W2-FE-WS9H) into w23/integ. No conflicts.

Follow-up commits: none needed (no compile errors).

New action ids: none. action-registry.ts changed (knowledge.crawl_site description, new helpers actionRequiresApproval / crawlSiteRequiresApproval); the admission golden was regenerated (VEREVON_UPDATE_GOLDEN=1 admission-contract test, 5 passed) and came out byte-identical (sha stays 091abfa2...), so no Model Plane copies, sha pin or zdr-contract-sync line moved. D-FIN-a-4 answered: no knowledge.crawl_site row change.

Compile guard (no test runs, except the golden writer):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): exit 0 on the first run; 0 errors; warnings all in quarry-edge files lane a did not touch (pre-existing set; none in map_routes.rs).
- Quarry Rust, feature build (cargo check -p quarry-browser --features chromiumoxide --tests, per D-FIN-a-1; the workspace check does not enable chromiumoxide, so chromiumoxide.rs and the include!d trusted_input_live.rs are only compiled here): exit 0, 0 errors, 0 warnings.
- Go quarry-orchestrator (GOWORK=off go build ./... and go vet ./...): both exit 0, no output.
- TS verevonv3 (tsc6 -p tsconfig.app.json --noEmit): 1 error, the pre-existing baseline (schedule-contract.test.ts TS2352); ChatPage.tsx, ChatPanels.tsx, the dock files and crawl-site-approval.test.ts typecheck clean.
- verevonv3 gateway (cargo check --tests): exit 0; 1 warning, pre-existing (unused import ResponseTemplate in src/domains/browser.rs, not a lane a line).
- auth-core TS: not changed, not checked.
- No Quarry ZDR/schema/route-baseline regeneration done: no route was added by lane a.

Open items:
- E3.3-k: asks 1 to 3 (warnings field and wire to quarry-core, docs); warnings are dropped at the trait boundary.
- E3.7.2: nothing sets max_parallel on a batch job.
- E3.8.7: no caller of with_reputation at the edge; no per-org reputation setting.
- E3.7.9a: settings read from env (OnceLock), not AppState; SPA prompt wiring with E3.7.9b's crawlApproval signal; price band has no source; keep CRAWL_APPROVAL_MODE at observe (D-FIN-a-3).
- E3.7.4-6w: ask B (edge seen route, final_url, mark_seen) not built; redirect seen-marking open.
- WS9H: step-budget row and FloatingComposer not built.
- None of the new flags are in compose or the runbooks.
- 64 depth rows appended to W23-depth.md under "W3 finish lane a" (E3.3-k 8, E3.7.2 5, E3.8.7 6, E3.7.9a 12, E3.4-6 6, E3.7.4-6w 8, WS9H 10 plus 2 security review rows, D-FIN-a 7). D-FIN-a-1, D-FIN-a-2 and D-FIN-a-4 are answered by this guard.
- The four line-ending-only .sha256 artifacts were not staged.

## Step: W3 finish, lane c

Units (branch w23f/lane-c, base 9bea43473): W3-E3.7.7b (5dc363192), W3-E3.6-6a (74f27fe33), W3-E3.3-d (bf2986a30), W3-E3.8.4d (b68c1b4d2, partial), W3-E3.9.6-7 (a35e7b155), W3-E3.3-c (e70ad477e), W3-E3.3-a (284d5cd27), W3-E3.2-b-swap (acd24e3c2); depth rows 6664a3dba.

Merge commit on w23/integ (after lane a, 739e0549e):
- 245148e33 merge: w23f/lane-c (W3 finish: W3-E3.7.7b, W3-E3.6-6a, W3-E3.3-d, W3-E3.8.4d, W3-E3.9.6-7, W3-E3.3-c, W3-E3.3-a, W3-E3.2-b-swap) into w23/integ. One conflict: .plan-work/quarry-w1w2/MP-ASKS.md (lane b's W3-E3.6-7 ask vs lane c's E3.3-d, E3.3-c and E3.2-b swap asks); kept both, lane b first.

Follow-up commits: none needed (no compile errors).

New action ids: none. action-registry.ts not changed by lane c, so the admission golden was not regenerated. The ZDR matrix golden (zdr-feature-matrix.json), its .sha256 and the zdr-contract-sync.test.sh pin agree after the merge (842bc936...); no other Quarry golden regeneration needed.

Flags (all default off): QUARRY_EDGE__OFFICE_VIA_IMPORTS, BROWSER_REGION_ATTACH, BROWSER_TAB_MENTIONS, VITE_BROWSER_TAB_MENTIONS, VITE_BROWSER_APPROVAL_CARD (APPROVAL_REDIRECT_ENABLED = false), QUARRY_EVAL_SPEND_CAP_USD (unset = no gate), repo variable QUARRY_TIER_C_ENABLED.

Compile guard (no test runs):
- Quarry Rust (cargo check --workspace --tests, CARGO_INCREMENTAL=0): exit 0 on the first run; 0 errors; 33 warning lines, all in quarry-edge files lane c did not touch (pre-existing set); none in content_route.rs, pipeline.rs, answer.rs, release_matrix.rs, scrape_cache.rs or zdr_matrix.rs.
- Go: lane c touched no Go module; not run.
- TS verevonv3 (tsc6 -p tsconfig.app.json --noEmit): 1 error, the pre-existing baseline (schedule-contract.test.ts TS2352); DashboardComposer.tsx, BrowserChrome.tsx, ChatMessages.tsx, the approval and region-attach files and the new tests typecheck clean.
- verevonv3 gateway (cargo check --tests): exit 0; 1 warning, pre-existing (unused import ResponseTemplate in src/domains/browser.rs, not a lane c line); browser_context.rs and browser_evidence.rs clean.
- auth-core TS: not changed, not checked.
- .github/workflows/quarry-v2-eval-tier-c.yml: not linted (no YAML parser).

Open items:
- E3.6-6a: nothing calls install_office_extract_client and there is no office-extract manifest row, so with the flags on an Office file still gets the typed 415; ODF/EPUB need imports-core support.
- E3.3-c: the chat turn path does not call resolve_turn_tab_mentions; no action contract.
- E3.3-d: no drag overlay, composer chip or browser-client export.
- E3.7.7b: search_routes.rs does not pass the prefetched pages yet.
- E3.8.4d: log canary, Redis behavioural tests and the crawl max_age key not built.
- E3.2-b swap: no component tests with the flag on; parent must dedupe a double press; redirect waits on the execution-core MP ask.
- E3.9.6-7: no producer for usage_meters.json; prices not ratified; Browserless token in URL (D-E3967-1, security high).
- 53 lane c depth rows plus 1 integration row appended to W23-depth.md under "W3 finish lane c".
- The four line-ending-only .sha256 artifacts were not staged.

## W2/W3 host test gate (2026-11-04)

Native Windows run on w23/integ (one cargo at a time, CARGO_INCREMENTAL=0, scratch Redis on 127.0.0.1:16399, REQUIRE_CONTRACT=1), after the test-fix round. Every failure below that is not marked fixed also failed at 9bea43473.

| Crate | Passed | Failed | Ignored | Failures |
|---|---|---|---|---|
| quarry-core (--features jsonschema) | 416 | 0 | 1 | none |
| quarry-security | 40 | 0 | 0 | none |
| quarry-transform | 310 | 0 | 0 | none |
| quarry-browser | 264 | 0 | 0 | none |
| quarry-tls | 26 | 0 | 0 | none |
| quarry-runtime | 1452 | 10 | 0 | platform only (artifact_store fs tests x9, artifact_org_stamp x1) |
| quarry-edge | 2858 | 14 | 3 | platform only (artifacts_fs_list x6, scrape_records_durable x4, zdr_posture_stamp run_page x1, parse_route x1, profile_guard x1, zdr_tombstone x1) |
| quarry-evals | 92 | 0 | 0 | none |

Go (GOWORK=off go vet ./... and go test ./...): quarry-orchestrator and quarry-control green.

Fixes (all on w23/integ):
- 9cba74992 fix(quarry-edge): closed error labels for unreadable mode switches (log_field_discipline R2; W2-Q-S4a and the human-grant renewal switch).
- 79b9ba1b1 test(quarry-edge): schedule_preflight pins QUARRY_EDGE__SCHEDULE_POSTURE=off for the S2 answer (W2-Q-S4a changed the default to observe on purpose).
- 7194abbb5 fix(quarry-runtime): challenge gate test samples all 35 AgentAction variants (E3.1-a FillOtpRef; Reload was never sampled).
- 779d73da4 fix(quarry-runtime): the org-core key provider refuses an org id with whitespace before calling (W2-CP-E2b).
- 881085d57 fix(quarry): web_policy_routes and ZDR matrix rows for the W2-U49 user receipt purge and the E3.3-h hit-test route; catalogue pinned at 40 keys.
- 21bc3019e fix(quarry-edge): an observed empty web_policy claim is no org layer, so the page and answer caches stay on (zdr_claim_floor; W2-CP-U35 x WS7.I).
- 5da331172 fix(quarry-evals): Tier B corpus may start on a path the TestSite deliberately does not serve (E3.9.2).
- ce93dcde3 docs(quarry): scrape option goldens regenerated (UPDATE_SCHEMAS=1) and redact_pii documented in openapi.yaml ScrapeRequest (W3-E3.6-5); openapi-scrape-parity 10/10.
- 35a7b5b2d docs(quarry): ZDR feature matrix golden, .sha256, docs/ZDR_MATRIX.md (46 rows, 6 gaps) and the zdr-contract-sync pin regenerated.

Platform failures (Windows only; they pass in the Linux container):
- The fs artifact store writes `page=blake3:<hash>` directories; ":" is invalid in a Windows directory name (os error 267): artifact_store fs tests, artifact_org_stamp, artifacts_fs_list, scrape_records_durable, zdr_posture_stamp run_page_with_a_zdr_stamp_persists_no_artifact_event_or_index_write.
- An early 413 closes the socket while the client is still uploading, so Windows answers ConnectionAborted 10053: parse_route a_body_over_the_limit_is_a_413_whether_or_not_it_declares_its_length, and profile_guard import_over_cap_is_413 (timing dependent: passed in the earlier head run, failed 3 of 3 re-runs now; no profile code changed since 9bea43473).
- zdr_tombstone the_http_sink_maps_a_refusal_to_a_status_and_a_dead_peer_to_a_class: Transport("timeout") instead of ("connect") for a dead peer.

Pre-existing (same failure set at 9bea43473, not fixed here):
- scripts/tests/zdr-contract-sync.test.sh: 170 passed, 15 failed. Every failure is a working-copy CRLF row of a .sha256 pin or vendored copy that fails the same way in the w23-base checkout (base: 165 passed, 20 failed). The quarry-core web-policy-catalogue.v1.json.sha256 working copy had CRLF here only (index LF, no diff); it was rewritten to LF in the working tree.
- node --test scripts/tests/*.test.mjs (Quarry-v2): 68 passed, 5 failed, identical to w23-base: openapi-internal guard (the /v1/internal/orgs/{}/users/{}/receipts route, W2-U49, is not in openapi-internal.yaml; the Go control service registers 10 internal routes, the test expects 8), tracked internal SDK files (InternalApi.ts, InternalRunPage*.ts), route coverage (/v1/agent/runs/{}/hit-test, /v1/audit/page and the user receipts route are undocumented) and the pinned route count.

Not run: Chromium #[ignore] live tests (quarry-core 1, quarry-edge 3 ignored), stack proofs, the Linux container gate.

Open from the fixers:
- W2-Q-S4a defaults QUARRY_EDGE__SCHEDULE_POSTURE to observe, not off: the preflight answer changes (posture unknown, observed_reason) and every fire reads org-core. Owner decision whether the default should be off.
- The org-core key provider refuses only whitespace in an org id; the stricter quarry-edge limits.rs valid_org_id (letters, digits, _ and -) was not adopted.
- Under the enforce claim mode an org claim with an empty layer and a real rev still stamps org_rev into the digest, so the caches stay off for that org.
- The agent_hit_test matrix row says 501 until the E3.3-h driver method lands; edit it then.
- E3.6-5: a crawl does not refuse redact_pii yet (UNSUPPORTED_KEYS in crawl_scrape.rs).

## W3 B4 completion (W3), integrated 2026-11-07

Units: W3-E3.5-4 documentation (ba15d31dc: CHANGE_TRACKING.md, RUNBOOKS.md, openapi-internal.yaml; the docs-contract phantom-name guard now accepts the E3.5-4 name and pins it to change_routes.rs; no code change, flag default off). W3-E3.2-g1 (cf54d1ef5 gateway proxy behind BROWSER_POLICY_ADMIN_API, default off; ddf826ad0 registry, model-access rows, port and mount behind VITE_BROWSER_POLICY_SETTINGS, default off; admission golden +2 rows). Status: .plan-work/quarry-w1w2/W23-W3-E3.2-g1-status.md.

Checks: gateway cargo check exit 0, the 10 new gateway tests pass, the full gateway bin suite 1812 passed, 0 failed, 4 ignored; SPA tsc6 exit 0, 0 errors; the specs of the touched folders as recorded in the unit status file. docs-contract node suite: 68 pass, 5 fail, the same five as before (route count pin and the known inherited ones).

Cross-plane: zdr-contract-sync reports one new red row, the SPA admission copy against the canonical Model Plane golden. The Model Plane copies were not edited (FINISH-QUEUE rule); the ask is in MP-ASKS.md (W3-E3.2-g1).

Not built in this step: W3-E3.4-8 wiring (nothing calls DownloadHandoff; it needs the E3.4-7 gateway arm and an edge route that loads the admitted artifact, W23-b-E347-1); B5 (E3.5-1, E3.5-7, E3.7.7, E3.5-5), B6, B7 and B8 open items are not started in this step.
