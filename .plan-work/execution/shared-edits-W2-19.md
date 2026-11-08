## WS13-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core block, after line 1260 `      EXECUTION_LOOP_PARKING_ENABLED: ${EXECUTION_LOOP_PARKING_ENABLED:-false}` (line 284 is the model-gateway block and is not the anchor), add `      APPROVAL_DENIAL_MEMORY_ENABLED: ${APPROVAL_DENIAL_MEMORY_ENABLED:-false}`, with an optional comment line above it.

## WS13-S5
shared-edit apps/Model Plane/rust/services/model-gateway: the final model-gateway verification is still owed. Run from apps/Model Plane/rust with CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0: cargo test -q -p model-gateway --lib -- step_up only_the_gateway_calls_decide_approval approval_value auth:: http_routes::. The last two edits (step_up_projected and its test, plus a rustfmt wrap) were not compiled.

## WS13-S5
shared-edit apps/Control Plane/auth-core/src/auth/convex-token.service.ts (LF): in `type ModelPlaneJwtClaims = {` add `  stepUp?: { at: number; method: 'password' };` before the closing `};`; in issueModelPlaneToken's `const payload = {` add `      ...(claims.stepUp ? { step_up: claims.stepUp } : {}),` directly before the `      zdr,` line that follows the 'Interactive posture comes from Auth Core's exact organization policy' comment.

## WS13-S5
shared-edit apps/Control Plane/auth-core/src/app.module.ts (CRLF, keep CRLF): add `import { StepUpController } from './auth/step-up.controller';` after the ModelPlaneTokenController import, and `    StepUpController,` after `    ModelPlaneTokenController,` in the controllers array.

## WS13-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block, after the line `      MCP_TOOL_OBSERVATION_ENABLED: ${MCP_TOOL_OBSERVATION_ENABLED:-false}` (around line 789), add `      APPROVAL_STEP_UP_ENFORCED: ${APPROVAL_STEP_UP_ENFORCED:-off}` with a short comment that only the literal shadow or enforce turns it on.

