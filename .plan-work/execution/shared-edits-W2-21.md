## WS26-S1.code
shared-edit apps/Model Plane/deploy/docker-compose.yml: insert after the line `      MODEL_GATEWAY_RUN_CONTROL_TIMEOUT_MS: ${MODEL_GATEWAY_RUN_CONTROL_TIMEOUT_MS:-750}` (model-gateway service environment block, about line 305) these two lines: `      CONTENT_SAFETY_SHADOW_SAMPLE_RATE: ${CONTENT_SAFETY_SHADOW_SAMPLE_RATE:-0.2}` and, directly above it, the comment `      # WS26-S1: sample rate (0 to 1) of the semantic shadow pass over external tool results. Telemetry only; ZDR turns never reach it.`

## WS24-S3
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: after the line `pub mod workspace_promote;` insert `pub mod workspace_tools;`. Apply before any execution-core build.

## WS24-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment, after the line `      EXECUTION_CORE_PROCESS_HOST: ${EXECUTION_CORE_PROCESS_HOST:-}` (line 1450) insert `      WORKSPACE_TOOLS: ${WORKSPACE_TOOLS:-off}`.

