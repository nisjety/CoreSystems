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
