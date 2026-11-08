## EP-14-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core service environment add `SESSION_CORE_REQUEST_INSPECTOR_READ: ${SESSION_CORE_REQUEST_INSPECTOR_READ:-false}`; in the model-gateway service environment add `MODEL_GATEWAY_REQUEST_INSPECTOR: ${MODEL_GATEWAY_REQUEST_INSPECTOR:-false}`; add both to the compose-contract assertions the other EP-14 switches use

## EP-14-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'chat.set_transcript_density': exempt('ui_state_only', 'external_write'),` add `  'chat.inspect_run_request': exempt('no_owner_executor', 'read'),`

## EP-14-S7
shared-edit apps/Model Plane/docs/ZDR.md (feature table) and zdr_features.rs if it lists routes per feature: add `GET /v1/runs/:run_id/requests/:call_seq` under feature `run_inspection` (ZDR bearer refused 403 zdr_feature_not_supported; nothing is stored for ZDR or waived runs)

