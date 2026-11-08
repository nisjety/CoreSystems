## WS15-S7
shared-edit apps/Model Plane/scripts/gen-sse-event-taxonomy.py: delete the two lines 81-82 inside LITERAL_PRODUCERS, which are the entry starting "run_parked": "parked agentic run on the owner's turn stream; the orchestration " and its continuation line "`RunParked` event shares the name",. Then PYTHONUTF8=1 python "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check must print: sse taxonomy up to date (29 events).

## WS15-S7
shared-edit scripts/tests/test_sse_taxonomy.py line 49 (8-space indent): replace self.assertIn('| `run_parked` | parked agentic run', taxonomy.doc) with self.assertIn('| `run_parked` | — (control) | `ChatEvent::RunParked` | ✅ |', taxonomy.doc). Then python3 -m unittest scripts/tests/test_sse_taxonomy.py must give 11 OK.

## WS16-S6
shared-edit apps/Application Plane/notification-core/cmd/server/main.go and apps/Application Plane/notification-core/internal/config/config.go, with the test beside config.go apps/Application Plane/notification-core/internal/config/config_webpush_test.go: already in the working copy, additive only (main.go +19 lines, config.go +32 lines; the exact text is the + lines of git diff for the two files). Accept them with the WS16-S6 commit, or revert all three together because the test references the new config fields. WS16-S4 owns both source files, so reconcile when it lands.

## WS16-S6
shared-edit apps/Frontend Plane/verevonv3/docs/endpoint-map.md: after the row | PUT | `/preferences/:eventType/:channel` | Update one preference | HTTP | core | add three rows: | GET | `/push/config` (BFF `/api/v1/push/config`) | Web Push config: VAPID public key and whether push is on | HTTP | core | ; | POST | `/push/subscriptions` (BFF `/api/v1/push/subscriptions`) | Register one browser push endpoint for the caller | HTTP | core | ; | DELETE | `/push/subscriptions/:id` (BFF `/api/v1/push/subscriptions/{id}`) | Revoke one of the caller's push endpoints | HTTP | core |

## WS22-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/tools.rs: insert '        thread_id: String::new(),' after line 235 ('        space_id: space_id.to_owned(),'), inside the ExecuteStepRequest literal at line 209. REQUIRED: cargo check -p model-gateway fails E0063 without it (WS22-S3 later sets the real thread id).

## WS22-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/browser_run.rs: insert '            thread_id: String::new(),' after line 1512 ('            space_id: String::new(),'), inside the ExecuteStepRequest literal at line 1493. REQUIRED, same reason.

## WS22-S2
shared-edit (cross-plane catalogue, REQUIRED for cargo test -p mp-contracts): from apps/Model Plane/rust run 'MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal' to regenerate apps/Model Plane/api/contracts/zdr-refusals.json (adds the sandbox_workspace feature); refresh zdr-refusals.json.sha256; update the pinned digest on line 114 of scripts/tests/zdr-contract-sync.test.sh; re-vendor apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/zdr-refusals/zdr-refusals.json (+ .sha256) and apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json (+ .sha256); add the feature to apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts if it mirrors FEATURES; then rerun scripts/tests/zdr-contract-sync.test.sh.

## WS20-S8
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: directly after the line 'cronHandler.WithScheduleContract(os.Getenv("SCHEDULE_CONTRACT_ENABLED") == "true")' add: '\t// WS20-S8: approval guards refuse a bare re-enable of a lapsed approval. Strict literal true; off by default.' and '\tcronHandler.WithContractGuards(os.Getenv("CRON_CONTRACT_GUARDS_ENABLED") == "true")'.

## WS20-S8
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: replace the line 'go cron.NewSweeper(pool, cronAuthorizer).Start(ctx)' (inside the 'else' of cronAuthErr, around line 350) with: 'cronSweeper := cron.NewSweeper(pool, cronAuthorizer).WithContractGuards(os.Getenv("CRON_CONTRACT_GUARDS_ENABLED") == "true")' then '// WS20-S8 pause notices follow the WS20-S7 switch: without it a pause still happens, unnoticed.' then 'if notifier, ok := notifyclient.New(os.Getenv("NOTIFICATION_CORE_URL"), os.Getenv("NOTIFICATION_CAPABILITY_CORE_SERVICE_TOKEN"), nil); ok && os.Getenv("CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED") == "true" {' then 'cronSweeper.WithPauseNotifier(notifier)' then '}' then 'go cronSweeper.Start(ctx)'. The ok guard matters: passing a nil *notifyclient.Client would be a typed nil inside the interface.

## WS20-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the capability-core environment block, after 'SCHEDULE_CONTRACT_ENABLED: ${SCHEDULE_CONTRACT_ENABLED:-false}' (about line 1742), add the comment '# WS20-S8: approval guards (auto-pause on expiry and failures, renewal refusal). Only the literal `true` enables them.' and 'CRON_CONTRACT_GUARDS_ENABLED: ${CRON_CONTRACT_GUARDS_ENABLED:-false}'.

