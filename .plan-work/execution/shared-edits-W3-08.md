## EP-8-S4
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: in `pub const FEATURES`, after the `sandbox_workspace` entry, add `    Feature {\n        key: "user_profile",\n        label: "the standing profile",\n    },`. Required before session-core and model-gateway tests run: feature_not_supported debug_asserts the key and from_status drops unknown keys. Both JSON copies were already edited to match catalogue_text().

## EP-8-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts: in the feature key union, after `  | 'sandbox_workspace'`, add `  | 'user_profile'`. In the labels map, after the `sandbox_workspace:` line, add `  user_profile: { no: 'den faste profilen', en: 'the standing profile' },`.

## EP-8-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs, `impl ... MemoryService for MockMemoryService`: after `reset_memory`, add three methods `get_user_profile(&self, _: Request<mp_contracts::model_plane::v1::GetUserProfileRequest>)`, `set_user_profile(... SetUserProfileRequest)` and `rebuild_user_profile(... RebuildUserProfileRequest)`, each returning `Result<Response<mp_contracts::model_plane::v1::UserProfileResponse>, Status>` as `Err(Status::unimplemented("<name> not needed in test"))`.

## EP-8-S4
shared-edit apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs, `impl mpv1::memory_service_server::MemoryService for MockMemory`: add the same three methods (mpv1::GetUserProfileRequest / SetUserProfileRequest / RebuildUserProfileRequest -> mpv1::UserProfileResponse), each returning Err(Status::unimplemented(..)).

## EP-8-S4
shared-edit apps/Model Plane/api/contracts/space-surface.v1.json (then re-vendor model-gateway/tests/fixtures/space-surface.v1.json and re-pin both .sha256): add two rows modelled on mg:PUT:/v1/memories/controls. Row 1: {"id":"mg:PUT:/v1/memories/profile","kind":"route","plane":"model","service":"model-gateway","method":"PUT","path":"/v1/memories/profile","handler":"http_routes.rs::set_user_profile","decision_type":"token_posture","min_role":"authenticated","zdr_class":"refuse","basis":"guard:set_user_profile","idempotency":"unspecified","flag":"SESSION_USER_PROFILE_ENABLED","payload_allow_list":[],"reason":"Session Core refuses a ZDR or waived credential before it stores the profile (memory_grpc.rs set_user_profile)."}. Row 2 is the same for "mg:POST:/v1/memories/profile/rebuild" (method POST, path /v1/memories/profile/rebuild, handler http_routes.rs::rebuild_user_profile, basis guard:rebuild_user_profile, reason naming rebuild_user_profile). If the flag field must stay null by schema, use null. Needed for zdr_posture.rs every_non_get_route_is_classified.

## EP-8-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml, session-core environment: next to `      MEMORY_CONTROLS_ENABLED: ${MEMORY_CONTROLS_ENABLED:-true}`, add `      SESSION_USER_PROFILE_ENABLED: ${SESSION_USER_PROFILE_ENABLED:-false}`.

## EP-8-S4
shared-edit apps/Model Plane/ZDR.md: add the store row 'user_profiles (session-core, EP-8 S4): ZDR read empty skip_reason=zdr; ZDR and waived Set/Rebuild refused zdr_feature_not_supported feature user_profile; erased by ResetMemory, memory delete (built lines) and organization erasure; Dreaming rebuild reads only accepted rows, and waived threads produce none.'

## EP-2-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, directly after line 244 `      MODEL_GATEWAY_WEB_READER: ${MODEL_GATEWAY_WEB_READER:-false}`, insert `      MODEL_GATEWAY_WEB_SEARCH_BATCH: ${MODEL_GATEWAY_WEB_SEARCH_BATCH:-false}` (LF file, six spaces).

## EP-2-S3
shared-edit apps/Model Plane/deploy/.env.example: no sibling switch line exists there, so this slice adds nothing; the owner decides whether to add MODEL_GATEWAY_WEB_SEARCH_BATCH=false (CRLF file; owner-only per the repo rule).

## EP-2-S3
shared-edit apps/Model Plane/api/contracts/zdr-posture-matrix.json.sha256: set the content to `eea374f9cf522bc34479be208f6d2ede6b97a87c7687f4c0141f54522dd73434  zdr-posture-matrix.json` (sha256 of the LF canonical file as edited; 31 enforcement rows).

## EP-2-S3
shared-edit scripts/tests/zdr-contract-sync.test.sh (GOLDENS, about line 111): replace c20446479a810ff59a9a2dcf7e9dfeb23371e3ceadaeeb49263ad2f916a21cba with eea374f9cf522bc34479be208f6d2ede6b97a87c7687f4c0141f54522dd73434 for apps/Model Plane/api/contracts/zdr-posture-matrix.json.

## EP-2-S3
shared-edit re-vendor (byte-identical copy of apps/Model Plane/api/contracts/zdr-posture-matrix.json, plus the new .sha256 beside each): apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json, apps/Model Plane/rust/services/execution-core/tests/fixtures/zdr-posture-matrix.json, apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/zdr-posture-matrix.json (and their .sha256 pins).

## EP-2-S3
shared-edit ENFORCEMENT_ROWS 30 -> 31: apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs:30, apps/Model Plane/rust/services/model-gateway/tests/zdr_mcp_observation_golden.rs:40, apps/Model Plane/rust/services/execution-core/tests/loop_fold_zdr_matrix.rs:57, apps/Model Plane/rust/services/execution-core/tests/zdr_mcp_golden.rs:40.

## EP-7-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in fn attention_reason_name, add the arm `AttentionReason::Goal => Some("goal"),` after `AttentionReason::Input => Some("input"),`, and update its doc comment to mention goal. This match is exhaustive, so model-gateway does not compile after the proto regen without it.

## EP-7-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/attention-client.ts: change `export type AttentionReason = 'approval' | 'plan' | 'input'` to include `| 'goal'`, and change `const REASONS` to `['approval', 'plan', 'input', 'goal']`.

## EP-7-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/attention-stream.ts: change `const REASONS` to `['approval', 'plan', 'input', 'goal']`.

## EP-7-S5
shared-edit apps/Application Plane/notification-core/internal/http/handlers.go: in isNotificationTypeAuthorized's "capability-core" map, add `"modelplane.goal_stopped": {},` after `"modelplane.scheduled_run_outcome": {},`.

## EP-7-S5
shared-edit apps/Application Plane/notification-core/internal/notification/service.go: in var nativeTypes, add `"modelplane.goal_stopped": {},` after `"modelplane.scheduled_run_outcome": {},`.

## EP-9-T3
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: after the line `import { ThreadWorkspacePanel } from './ThreadWorkspacePanel'` (line 44) add `import { EffectivePolicyPanel } from './EffectivePolicyPanel'`; after the line `          <ThreadWorkspacePanel threadId={state.threadId ?? ''} />` (line 1174, inside the Show that tests hasMessages(), state.threadId and not temporary) add `          <EffectivePolicyPanel scope={{ kind: 'thread', threadId: state.threadId ?? '' }} />` with the same 10-space indent. The file is LF. Anchor by content, since the import shifts the lines below it by one.

## EP-9-T3
shared-edit apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomHeader.tsx: after the line `import { SpaceRoomSettings } from './SpaceRoomSettings'` (line 11) add `import { EffectivePolicyPanel } from '@/features/chat/components/EffectivePolicyPanel'`; in the room-details dialog, after the closing `/>` of the `<SpaceRoomSettings ... />` element (line 220) and before `<Show when={canArchive()}>`, add `              <EffectivePolicyPanel scope={{ kind: 'space', spaceId: props.spaceRef }} />` with a 14-space indent. The file is LF; props.spaceRef is the identifier the /api/v1/spaces routes use. Anchor by content.

