## WS27-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service, directly after the line `      MODEL_GATEWAY_TURN_USAGE_COMPLETE: ${MODEL_GATEWAY_TURN_USAGE_COMPLETE:-false}` (about line 371), add a comment line stating that usage_progress frames are off by default and only the literal true enables them, then the line `      MODEL_GATEWAY_USAGE_PROGRESS: ${MODEL_GATEWAY_USAGE_PROGRESS:-false}`.

## WS27-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/ag-ui-client.ts: in toAgUiEvent, insert the line `    case 'usage.progress':` directly above `    case 'lifecycle.retry':` (about line 195) so it joins the existing `return null` group. Without it `tsc -b` reports TS2366 at ag-ui-client.ts(127,53) for the new VerevonUiEvent member.

## WS23-S7.code
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/provider_reads.rs and apps/Model Plane/rust/services/execution-core/src/integration_tools.rs: in shape_read set more_available to next_page_token.is_some() || result.get("moreAvailable").and_then(Value::as_bool) == Some(true) (provider_reads.rs:335, integration_tools.rs:982). Update the Rust tests that assume the old value.

## WS23-S7.code
shared-edit the same two Rust files, shape_file_content (provider_reads.rs:342-365, integration_tools.rs:989-1013): read mime_type (fallback mimeType) and base64-decode content_base64 (STANDARD) instead of content, with the existing UTF-8 and text/* rule. The executor returns content_base64 and mime_type for drive.file.content on both providers (drive_read.go:229 and :555). Update a_binary_file_is_unsupported_not_a_blob and the shaping tests that use content.

## WS23-S7.code
shared-edit the same two Rust files, read_fields (provider_reads.rs ~397-440, integration_tools.rs ~1044-1090): calendar.events add summary and htmlLink (Google rows); sharepoint.sites add displayName and keep name; profile add name (Google userinfo); mail.messages and gmail.messages add bodyPreview and date; drive.files and sharepoint.drives add folder, and shape_item reads file.mimeType into mimeType.

## WS23-S7.code
shared-edit apps/Model Plane/rust/services/execution-core/src/integration_tools.rs: replace the private shape_read, shape_file_content, shape_item and read_fields copy (~lines 792-1090) with calls into mp_contracts::provider_reads, so the Do and Ask loops cannot drift.

## WS23-S7.code
shared-edit apps/Ingestion Plane/integration-corev2/README.md: in the Outlook paragraph, after the sentence that ends with the Graph host under /v1.0/me/, add: A link that is refused gives moreAvailable: true and no token.

## WS24-S8
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: after line 52 (`pub mod owner_actions;`) add `pub mod owner_reads;`. Then in runtime_loop/mod.rs delete the `// WS24-S8` comment and the `#[path = "../owner_reads.rs"]` line, rename `crate::runtime_loop::owner_reads` to `crate::owner_reads` in runtime_loop/mod.rs and runtime_loop/agent.rs, and rename `execution_core::runtime_loop::owner_reads` to `execution_core::owner_reads` in tests/agent_action_admission_contract.rs. Precedent: scheduled_agent in the same file.

## WS24-S8
shared-edit scripts/tests/zdr-contract-sync.test.sh: line 157, replace `68ff49dcbdc79c38afe7a8ba16b7540e581cff169e92ce0a37191968ee400fc2` with `6169af41d89cf10519d11c0885e1b694fa2a71b84ed7093031988a0eb0766f9b`. Until this is applied the sync check reports one not-ok (the pinned digest).

## WS21-S7
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: in the gateway service, directly after the line '      BROWSER_DO_ENABLED: ${BROWSER_DO_ENABLED:-false}' (file line 141), insert these three lines with the same 6-space indentation: a comment '# WS21-S7: a workspace AI run starts under a real broker grant and carries the browser bearer. Off by default; only the literal true. Keep it off until Quarry accepts broker grants and the broker site policy is shadow or enforce.' then 'BROWSER_AI_RUN_REAL_GRANT: ${BROWSER_AI_RUN_REAL_GRANT:-false}'. Compose is not in this slice's file list, so the default is also off in code without it.

