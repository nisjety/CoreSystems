## EP-13-S5
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/api/http.ts: in extractError, the details guard currently reads `e.details && typeof e.details === 'object' && !Array.isArray(e.details) ? (e.details as Record<string, unknown>) : null`. Change it to `e.details && typeof e.details === 'object' ? (e.details as Record<string, unknown>) : null` so the invalid_role_table issues array reaches ApiError.details. Then check any http test that pins the array-drop rule and any consumer that reads ApiError.details as a record, and run tsc6. Until this lands, per-row role errors never show in production; the generic banner still shows.

## EP-13-S7
shared-edit apps/Model Plane/rust/services/inference-core/src/metrics.rs: after the TOKENS_TOTAL describe_counter! block (around lines 121-124), add describe_counter!("inference_gemini_tool_dropped_total", "Gemini tool declarations left out of a request because their schema cannot be expressed for Vertex, by refusal reason."); Optional for merge; the metric works without it.

## EP-14-S10
shared-edit apps/Model Plane/rust/services/inference-core/src/provider/policy_client.rs:200: in the SetRoutingPolicyRequest literal add a line `expected_version: 0,` after `updated_by`. Required before inference-core compiles (E0063 otherwise); 0 keeps today's unchecked write.

## EP-14-S10
shared-edit apps/Model Plane/rust/services/inference-core/src/provider/mod.rs: add `pub mod routing_diff;` before `pub mod routing_policy;` (EP-13 S7 owns this file in the same batch, so I did not touch it). Then delete the `// The offline routing diff ...` comment block and the `#[path = "routing_diff.rs"] pub mod routing_diff;` lines at the top of provider/intent.rs (after its `use super::{ChatMessage, ToolDefinition};` line).

## EP-14-S10
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: RoutingPolicy row: policy and candidate store owned by session-core (routing_policy, routing_policy_candidates); offline diff owned by inference-core provider/routing_diff.rs; live evaluation by scripts/eval-routing-candidate.sh.

## EP-14-S10
shared-edit apps/Model Plane/deploy/docker-compose.yml: session-core environment add `SESSION_CORE_ROUTING_CANDIDATES_ENABLED: ${SESSION_CORE_ROUTING_CANDIDATES_ENABLED:-false}`; inference-core environment add `ROUTING_POLICY_PROMOTION_REQUIRED: ${ROUTING_POLICY_PROMOTION_REQUIRED:-false}`.

## EP-14-S10
shared-edit apps/Model Plane/deploy/.env.example: add `SESSION_CORE_ROUTING_CANDIDATES_ENABLED=false` and `ROUTING_POLICY_PROMOTION_REQUIRED=false`.

