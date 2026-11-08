## WS10-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml (inference-core environment block): ORG_CORE_URL: ${ORG_CORE_URL:-http://org-core:8080}  (use the org-core address the model-gateway block already uses for ORG_CORE_URL), INFERENCE_ORG_CORE_SERVICE_TOKEN: ${INFERENCE_ORG_CORE_SERVICE_TOKEN:-}, MODEL_ENTITLEMENT_MODE: ${MODEL_ENTITLEMENT_MODE:-off}

## WS10-S3
shared-edit apps/Model Plane/deploy/.env.example: MODEL_ENTITLEMENT_MODE=off (comment: off|shadow|enforce, unknown value means enforce) and INFERENCE_ORG_CORE_SERVICE_TOKEN= (empty; minted by apps/Control Plane/scripts/run-control-plane.sh for the org-core principal inference-core, scope org:model-entitlement:read:any); never write the value into a file

