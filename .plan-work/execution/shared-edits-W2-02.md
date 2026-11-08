## WS27-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: insert after line 296 (`MODEL_GATEWAY_PLAN_EXECUTE_ENABLED: ${MODEL_GATEWAY_PLAN_EXECUTE_ENABLED:-false}`, inside the model-gateway service environment) the line `      MODEL_GATEWAY_BUDGET_WINDOW: ${MODEL_GATEWAY_BUDGET_WINDOW:-false}` (6-space indent, match the file's line endings).

