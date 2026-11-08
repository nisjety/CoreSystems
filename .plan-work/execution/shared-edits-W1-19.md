## WS07-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: in the org block next to 'org.update_support_ai_mode', add the line  'org.update_agent_learning_policy': exempt('human_identity_or_security', 'external_write'),  Then regenerate the golden: cd 'apps/Frontend Plane/verevonv3' && VEREVON_UPDATE_GOLDEN=1 pnpm vitest run src/shared/actions/admission-contract.test.ts (rewrites src/shared/api/contracts/agent-action-admission.json with the new row, schemaHash computed by the test), then pnpm vitest run src/shared/actions and pnpm typecheck must be green for those files; the JSON is vendored by other planes, so run their pinning tests (capability-core).

## WS07-S4
shared-edit apps/Control Plane/docker-compose.yml: line 543-544 ORG_CORE_SERVICE_CREDENTIALS (one JSON line; merge by hand): change the session-core principal's "scopes":["org:read:any"] to "scopes":["org:read:any","org:agent-policy:read:any"]. Do not touch any token. Check that retention_posture_contract_test.go and org-core's registry validation still pass after the edit (go test ./internal/http in apps/Control Plane/org-core).

## WS07-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block (service starts near line 795) add: ORG_CORE_URL: ${ORG_CORE_URL:-http://org-core:8080} ; SESSION_ORG_CORE_SERVICE_TOKEN: ${SESSION_ORG_CORE_SERVICE_TOKEN:-} ; ORG_POLICY_POLL_SECS: ${ORG_POLICY_POLL_SECS:-60} ; ORG_POLICY_MAX_STALE: ${ORG_POLICY_MAX_STALE:-900} ; ORG_POLICY_FAIL_CLOSED: ${ORG_POLICY_FAIL_CLOSED:-false} ; ORG_LEARNING_DEFAULT: ${ORG_LEARNING_DEFAULT:-disabled}. In the production overlay (apps/Model Plane/deploy/docker-compose.production.yml) set ORG_POLICY_FAIL_CLOSED to "true" for session-core. ORG_POLICY_MAX_STALE is in seconds (900 = 15 min). Leaving SESSION_ORG_CORE_SERVICE_TOKEN empty keeps the whole mirror dormant; never print or copy the token value.

## WS07-S4
shared-edit apps/Model Plane/deploy/.env.example: document the six variables above (ORG_POLICY_FAIL_CLOSED only the literal true enables; ORG_LEARNING_DEFAULT only the literal proposals enables).

## WS07-S4
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md and MODEL_PLANE_STATUS.md: org memory/learning policy owner = org-core (metadata.agentLearning); session-core holds only the mirror; mark as source-built, not live-verified.

