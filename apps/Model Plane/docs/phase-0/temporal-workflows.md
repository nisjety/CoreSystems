# Temporal Workflows — Phase 0 Freeze

**Workers:** `go/cmd/session-core`, `go/cmd/inference-core`, `go/cmd/execution-core`
**Task queues:** one per core. Namespace: `model-plane`.
**SDK:** Go (`go.temporal.io/sdk`).

## 1. Task queues

| Task queue | Worker | Workflows | Activities |
|------------|--------|-----------|------------|
| `mp-session` | session-core | `SessionWorkflow`, `RunWorkflow` | `PersistRunStart`, `PersistRunEnd`, `EmitEvent`, `CreateCheckpoint` |
| `mp-inference` | inference-core | `InferenceWorkflow` | `InvokeModel`, `RecordUsage`, `EmitEvent` |
| `mp-execution` | execution-core | `ExecutionWorkflow` | `ExecuteStep`, `ToolCallActivity`, `EmitEvent` |

## 2. Workflow contracts

### 2.1 `SessionWorkflow`

Lifecycle of a user session. Long-running; one instance per `SessionKey`.

- **Workflow ID**: `session-{session_key}`
- **Input**: `SessionStartInput { session_key, org_id, user_id, agent_id, policy }`
- **Signals**:
  - `SubmitRun(RunRequest)` → enqueues a child `RunWorkflow`.
  - `Cancel` → terminates active run and closes the session.
- **Queries**:
  - `GetState()` → `{ status, active_run_id?, run_history[] }`
- **Events emitted**: `SESSION_START`, `SESSION_END` (via `EmitEvent` activity).
- **Retry policy**: workflow itself non-retryable; child runs retry per `RunWorkflow` policy.

### 2.2 `RunWorkflow` (child of `SessionWorkflow`)

Single agent run. Orchestrates inference + execution steps.

- **Workflow ID**: `run-{run_id}`
- **Input**: `RunInput { run_id, session_key, org_id, agent_id, input, resume_from_checkpoint? }`
- **Children**: invokes `InferenceWorkflow` and `ExecutionWorkflow` as needed.
- **Signals**: `Interrupt`, `ProvideUserInput(payload)`.
- **Queries**: `GetCurrentStep()`, `GetUsage()`.
- **Events emitted**: `RUN_STARTED`, `RUN_COMPLETED` | `RUN_FAILED`, `CHECKPOINT_CREATED`.
- **Retry policy**:
  - Initial interval: 1s, backoff 2.0, max interval 60s, max attempts 3.
  - Non-retryable errors: `ERR_POLICY_DENIED`, `ERR_QUOTA_EXCEEDED`, `ERR_INPUT_INVALID`.
- **Timeout**: workflow run timeout = policy-driven, default 30 min.

### 2.3 `InferenceWorkflow` (child of `RunWorkflow`)

One model invocation. Usually short; may stream via `INFERENCE_TOKEN` events.

- **Workflow ID**: `inference-{run_id}-{step_id}`
- **Input**: `InferenceInput { run_id, step_id, model, prompt, params }`
- **Activities**:
  - `InvokeModel` — calls provider SDK, returns completion + usage.
  - `RecordUsage` — publishes `USAGE_RECORDED` via `EmitEvent`.
- **Events emitted**: `INFERENCE_STARTED`, `INFERENCE_COMPLETED` | `INFERENCE_FAILED`.
- **Retry policy**: initial 500ms, backoff 2.0, max 5 attempts; non-retryable on 4xx from provider.
- **Activity timeouts**: `InvokeModel` start-to-close = 5 min, schedule-to-close = 10 min.

### 2.4 `ExecutionWorkflow` (child of `RunWorkflow`)

One tool / capability step.

- **Workflow ID**: `execution-{run_id}-{step_id}`
- **Input**: `ExecutionInput { run_id, step_id, tool, args, lease_ref? }`
- **Activities**:
  - `ExecuteStep` — dispatches to the capability service (sandbox-manager / browser-broker / etc.).
  - `ToolCallActivity` — wraps a single tool call, serialisable args/results.
- **Events emitted**: `STEP_STARTED`, `STEP_COMPLETED` | `STEP_FAILED`,
  `TOOL_CALL_REQUESTED`, `TOOL_CALL_RESULT`.
- **Retry policy**: initial 1s, backoff 2.0, max 5 attempts. Non-retryable:
  `ERR_TOOL_INVALID_ARGS`, `ERR_POLICY_DENIED`.
- **Activity timeouts**: `ExecuteStep` start-to-close = 10 min; `ToolCallActivity` = 2 min.

## 3. Activity contracts

| Activity | Input | Output | Emits | Idempotent? |
|----------|-------|--------|-------|-------------|
| `PersistRunStart` | `RunId, org_id, agent_id, input` | `()` | writes row | ✅ via PK |
| `PersistRunEnd` | `RunId, status, output, usage` | `()` | updates row | ✅ |
| `CreateCheckpoint` | `RunId, StepId, state_blob` | `CheckpointId` | `CHECKPOINT_CREATED` | ✅ via idempotency_key |
| `InvokeModel` | `model, prompt, params` | `completion, usage` | `INFERENCE_STARTED/COMPLETED/FAILED` | ⚠️ retry-safe via provider idempotency key |
| `RecordUsage` | `UsageRecord` | `()` | `USAGE_RECORDED` | ✅ |
| `ExecuteStep` | `tool, args, lease_ref?` | `step_result` | `STEP_STARTED/COMPLETED/FAILED` | ⚠️ tool-dependent |
| `ToolCallActivity` | `tool, args` | `tool_output` | `TOOL_CALL_REQUESTED/RESULT` | ⚠️ tool-dependent |
| `EmitEvent` | `Envelope` | `()` | publishes to NATS | ✅ via `event_id` + `idempotency_key` |

All activities MUST:
1. Accept a `context.Context` with Temporal-propagated trace headers.
2. Carry `org_id` and `run_id` in every log line.
3. Set explicit `StartToCloseTimeout` (no implicit defaults).
4. Use heartbeat for any activity > 30s.

## 4. Versioning

- Workflow versioning uses `workflow.GetVersion(ctx, "change-id", min, max)`.
- New branches gated behind `GetVersion` for at least one deployed release before retiring
  the default branch.
- Activity signature changes are treated as new activities (e.g. `InvokeModel` → `InvokeModelV2`).

## 5. Failure model

- Error codes surfaced on `RUN_FAILED` / `STEP_FAILED` match the `mp_errors` catalogue
  (phase 1 artefact). Phase-0 freeze enumerates **non-retryable** codes only:
  - `ERR_POLICY_DENIED`
  - `ERR_QUOTA_EXCEEDED`
  - `ERR_INPUT_INVALID`
  - `ERR_TOOL_INVALID_ARGS`
  - `ERR_NOT_FOUND`
  - `ERR_UNAUTHENTICATED` / `ERR_FORBIDDEN`
- All other errors are retryable per §2 per-workflow policy.

## 6. Review checklist

- [x] Every workflow and activity listed here has a corresponding Go implementation or
      `// TODO(phase-N):` anchor in `go/cmd/*`.
- [x] Every event emitted from an activity appears in [`event-schemas.md`](./event-schemas.md).
- [x] No activity exceeds 10 min start-to-close without heartbeat.
- [x] Non-retryable error codes are consistent with `mp_errors` (phase 1).
