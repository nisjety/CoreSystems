## WS12-S12
shared-edit apps/Frontend Plane/verevonv3/vite.config.ts: in test.exclude, after the line 'apps/gateway/scripts/**/*.test.mjs', add the line 'scripts/**/*.test.mjs', with a comment that the node:test budget test runs from verevonv3-ci.yml. Apply before the first push.

## WS06-S0
shared-edit: none. apps/Control Plane/docker-compose.yml:1079 (rules mount) and the rule_files glob in apps/Control Plane/observability/prometheus.yml both landed in 2f8b8caa. Do not add a second rule_files key.

## WS05-S6.code
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: after the soak, change the tool-risk row (around lines 62-63 and 147) to say the registry class and approval floor are the only classifier, name heuristics deleted. Not before.

## WS05-S6.code
shared-edit apps/Model Plane/deploy/docker-compose.yml: only after the soak and proof-stack evidence, change `TOOL_CLASS_GATE: ${TOOL_CLASS_GATE:-off}` (about line 1298) to default `enforce`. Not now.

