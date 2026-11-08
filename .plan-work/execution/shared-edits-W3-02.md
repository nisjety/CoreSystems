## EP-14-S3
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/styles/global.css: after the .verevon-chat-thinking__activity rule (about line 24715) add `.verevon-chat-step-strip { min-width: 0; overflow: hidden; color: var(--verevon-e-subtle, var(--subtle)); font-size: 12px; line-height: 1.4; white-space: nowrap; text-overflow: ellipsis; }` and `.verevon-chat-step-strip:not(:empty) { margin-top: 8px; }`. Do not hide the region with display:none, because the live region has to stay in the accessibility tree.

## EP-2-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: after the line '      MODEL_GATEWAY_CATALOGUE_PRICES: ${MODEL_GATEWAY_CATALOGUE_PRICES:-false}' in the model-gateway environment block, add '      MODEL_GATEWAY_WEB_READER: ${MODEL_GATEWAY_WEB_READER:-false}'. RESULT_HANDLES_EXTENDED has no compose pass-through today, so the reader is inert in a deployed gateway; with the S11 flip decide whether to add '      RESULT_HANDLES_EXTENDED: ${RESULT_HANDLES_EXTENDED:-false}'.

## EP-2-S2
shared-edit apps/Model Plane/deploy/.env.example: optional. Add 'MODEL_GATEWAY_WEB_READER=false' only if the example is meant to list gateway switches (it lists six MODEL_GATEWAY_ keys today).

## EP-2-S2
shared-edit apps/Model Plane/api/contracts/zdr-posture-matrix.json.sha256: replace the line with 'c20446479a810ff59a9a2dcf7e9dfeb23371e3ceadaeeb49263ad2f916a21cba  zdr-posture-matrix.json'.

## EP-2-S2
shared-edit scripts/tests/zdr-contract-sync.test.sh, GOLDENS line 111: replace the pinned digest 9f61e99040d6895248fc8e22cec2adaf60e14dba7fefa1f3bf8ad7ccf149c201 with c20446479a810ff59a9a2dcf7e9dfeb23371e3ceadaeeb49263ad2f916a21cba.

## EP-2-S2
shared-edit apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json.sha256: the same digest line as item 4 (the fixture JSON is already byte-identical to the canonical file).

## EP-2-S2
shared-edit apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/zdr-posture-matrix.json and its .sha256: copy the canonical bytes and the digest line from item 4. The crate's own tests read their copy and still pass at 28 rows.

## EP-2-S2
shared-edit apps/Model Plane/rust/services/execution-core/tests/fixtures/zdr-posture-matrix.json and its .sha256: same copy and digest. Then set ENFORCEMENT_ROWS from 28 to 30 in apps/Model Plane/rust/services/execution-core/tests/loop_fold_zdr_matrix.rs (line 57) and in apps/Model Plane/rust/services/execution-core/tests/zdr_mcp_golden.rs (line 40), which pin the vendored row count.

## EP-2-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/audit.rs, fn tool_data_category: add '| "read_page" | "find_in_page"' to the arm '"web_search" | "fetch_url" | "web_fetch" | "news" | "traffic" | "yr_weather"' so reader reads are audited as public_non_personal, not unclassified.

## EP-2-S2
shared-edit apps/Model Plane/rust/crates/mp-content-safety/src/trust.rs, TOOL_TRUST: add ('read_page', TrustClass::ExternalWeb) and ('find_in_page', TrustClass::ExternalWeb) beside the fetch_url row. Defence in depth: the arms replay the parked verdict, so no current path classifies these names by name.

## EP-2-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/room_run.rs, BUILTIN_TOOL_CLASSES: after the fetch_url row add ('read_page', Some(StepClass::Web)) and ('find_in_page', Some(StepClass::Web)). Cosmetic; otherwise room steps show the generic Tool class.

