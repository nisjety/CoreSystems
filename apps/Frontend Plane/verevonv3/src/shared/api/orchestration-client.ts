import { requestJson } from './http'

// Human-in-the-loop run control for agentic chat runs. These call the gateway
// `/api/v1/orchestration/*` proxy (→ model-gateway → session-core/execution-core).
// Used by the chat "internal Claude Code" approval UI + plan mode: when an
// agentic run pauses for a risky tool (a `step_update` with status "paused"),
// the chat lists the run's pending approvals here, the user decides, and the
// run is resumed.

export type ApprovalDecision = 'approve' | 'reject'

/** A pending/decided approval as reported by model-gateway's orchestration API. */
export type Approval = {
  id: string
  runId?: string
  planId?: string
  stepId?: string
  /** Approval kind, e.g. tool/plan/command — provider string, rendered as-is. */
  kind?: string
  /** PENDING | GRANTED | DENIED | EXPIRED (uppercased provider enum name). */
  status?: string
  requestedBy?: string
  /** Free-form detail/summary of what needs approval, when present. */
  detail?: string
}

type RawApproval = Record<string, unknown>

function str(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/**
 * Canonicalize a provider approval-state string to the short form
 * `Approval.status` documents (`PENDING`/`GRANTED`/`DENIED`/`EXPIRED`).
 *
 * The live wire value is the FULL protobuf enum name from
 * `model_plane/v1/orchestration.proto`'s `ApprovalState`
 * (`APPROVAL_STATE_REQUESTED`, `APPROVAL_STATE_GRANTED`,
 * `APPROVAL_STATE_DENIED`, `APPROVAL_STATE_TIMED_OUT`) — never the bare
 * `PENDING`/`GRANTED`/`DENIED`/`EXPIRED` every caller of `listApprovals`
 * (`AgentRunConsole.tsx`, `use-chat-controller.ts`,
 * `KnowledgeComposer.tsx`'s browser-workspace HITL gate) filters on. Without
 * this normalization every one of them silently found zero pending
 * approvals — `(approval.status ?? 'PENDING').toUpperCase() === 'PENDING'`
 * never matched `'APPROVAL_STATE_REQUESTED'` — so a real, correctly-fired
 * backend approval gate looked to the UI exactly like there was nothing to
 * decide. Substring-matched (not exact) so it tolerates either convention.
 */
function canonicalApprovalStatus(raw: string | undefined): string | undefined {
  if (!raw) return raw
  const upper = raw.toUpperCase()
  if (upper.includes('REQUESTED') || upper.includes('PENDING')) return 'PENDING'
  if (upper.includes('GRANTED')) return 'GRANTED'
  if (upper.includes('DENIED')) return 'DENIED'
  if (upper.includes('TIMED_OUT') || upper.includes('EXPIRED')) return 'EXPIRED'
  return upper
}

/** Normalize a model-gateway approval object (snake_case) to our camelCase shape. */
function normalizeApproval(raw: RawApproval): Approval | null {
  const id = str(raw.id) ?? str(raw.approval_id)
  if (!id) return null
  return {
    id,
    runId: str(raw.run_id) ?? str(raw.runId),
    planId: str(raw.plan_id) ?? str(raw.planId),
    stepId: str(raw.step_id) ?? str(raw.stepId),
    kind: str(raw.kind) ?? str(raw.approval_kind),
    status: canonicalApprovalStatus(str(raw.status) ?? str(raw.state)),
    requestedBy: str(raw.requested_by) ?? str(raw.requestedBy),
    detail: str(raw.detail) ?? str(raw.summary) ?? str(raw.reason) ?? str(raw.tool),
  }
}

/** List a run's approvals; only PENDING ones are surfaced for a decision. */
export async function listApprovals(runId: string, signal?: AbortSignal): Promise<Approval[]> {
  const payload = await requestJson<{ approvals?: RawApproval[] }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/approvals`,
    { signal },
  )
  const list = Array.isArray(payload.approvals) ? payload.approvals : []
  return list.map(normalizeApproval).filter((a): a is Approval => a !== null)
}

/** Approve or reject a pending approval (unblocks/blocks the gated tool step). */
export async function decideApproval(
  approvalId: string,
  decision: ApprovalDecision,
  reason?: string,
  signal?: AbortSignal,
): Promise<Approval | null> {
  const payload = await requestJson<{ approval?: RawApproval }>(
    `/api/v1/orchestration/approvals/${encodeURIComponent(approvalId)}/decide`,
    {
      method: 'POST',
      body: JSON.stringify({ decision, reason: reason ?? '' }),
      signal,
    },
  )
  return payload.approval ? normalizeApproval(payload.approval) : null
}

/** Resume a run that paused for approval, after the decision is recorded. */
export async function resumeRun(runId: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(`/api/v1/orchestration/runs/${encodeURIComponent(runId)}/resume`, {
    method: 'POST',
    body: '{}',
    signal,
  })
}

/** Durable result of an authorized run cancellation. */
export type RunCancellation = {
  runId?: string
  cancelled: boolean
  status?: string
  /** Session Core RUN_CANCELLED event id, when a receipt exists. */
  receiptId?: string
}

/** Cancel a run (e.g. the user rejects and wants to stop the whole run). */
export async function cancelRun(runId: string, signal?: AbortSignal): Promise<RunCancellation> {
  const payload = await requestJson<Record<string, unknown>>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/cancel`,
    {
      method: 'POST',
      body: '{}',
      signal,
    },
  )
  return {
    runId: str(payload.run_id) ?? str(payload.runId),
    cancelled: payload.cancelled === true || str(payload.status)?.toLowerCase() === 'cancelled',
    status: str(payload.status),
    receiptId: str(payload.receipt_id) ?? str(payload.receiptId),
  }
}

// ── Run console reads ─────────────────────────────────────────────────────────
// Point-in-time snapshots for the agentic run console (plans/todos/lineage).
// The live `step_update`/`*_transitioned` stream comes from run-console-client;
// these GETs hydrate the initial state and reconcile after a resume.

type RawPlan = Record<string, unknown>
type RawTodo = Record<string, unknown>

/** A run's plan as reported by model-gateway's orchestration API. */
export type Plan = {
  id: string
  runId?: string
  threadId?: string
  author?: string
  /** Lifecycle state (uppercased provider enum name), rendered as-is. */
  state?: string
  /** Free-form human-friendly summary of the plan, when present. */
  summary?: string
  /** Ordered steps emitted by the orchestration service. */
  steps?: PlanStep[]
  /** Previous plan id when this plan supersedes an earlier revision. */
  supersedes?: string
}

export type PlanStep = {
  id: string
  title: string
  operation?: string
  state?: string
}

/** A thread's todo as reported by model-gateway's orchestration API. */
export type Todo = {
  id: string
  threadId?: string
  runId?: string
  /** Lifecycle state (uppercased provider enum name), rendered as-is. */
  state?: string
  /** Human-friendly todo title/description, when present. */
  title?: string
  description?: string
  priority?: string
  blockedBy?: string[]
}

function normalizePlan(raw: RawPlan): Plan | null {
  const id = str(raw.id) ?? str(raw.plan_id) ?? str(raw.planId)
  if (!id) return null
  const plan: Plan = {
    id,
    runId: str(raw.run_id) ?? str(raw.runId),
    state: str(raw.state) ?? str(raw.status),
    summary: str(raw.summary) ?? str(raw.detail) ?? str(raw.description),
  }
  const threadId = str(raw.thread_id) ?? str(raw.threadId)
  if (threadId) plan.threadId = threadId
  const author = str(raw.author)
  if (author) plan.author = author
  const rawSteps = Array.isArray(raw.steps) ? raw.steps : undefined
  if (rawSteps) {
    plan.steps = rawSteps
      .map((item): PlanStep | null => {
        if (!item || typeof item !== 'object' || Array.isArray(item)) return null
        const step = item as Record<string, unknown>
        const stepId = str(step.id) ?? str(step.step_id) ?? str(step.stepId)
        const title = str(step.title) ?? str(step.name) ?? str(step.operation)
        if (!stepId || !title) return null
        return {
          id: stepId,
          title,
          operation: str(step.operation),
          state: str(step.state) ?? str(step.status),
        }
      })
      .filter((step): step is PlanStep => step !== null)
  }
  const supersedes = str(raw.supersedes) ?? str(raw.supersedes_id) ?? str(raw.supersedesId)
  if (supersedes) plan.supersedes = supersedes
  return plan
}

function normalizeTodo(raw: RawTodo): Todo | null {
  const id = str(raw.id) ?? str(raw.todo_id) ?? str(raw.todoId)
  if (!id) return null
  const todo: Todo = {
    id,
    threadId: str(raw.thread_id) ?? str(raw.threadId),
    state: str(raw.state) ?? str(raw.status),
    title: str(raw.title) ?? str(raw.summary) ?? str(raw.detail) ?? str(raw.description),
  }
  const runId = str(raw.run_id) ?? str(raw.runId)
  if (runId) todo.runId = runId
  const description = str(raw.description)
  if (description) todo.description = description
  const priority = str(raw.priority)
  if (priority) todo.priority = priority
  const blockedBy = Array.isArray(raw.blocked_by)
    ? raw.blocked_by
    : Array.isArray(raw.blockedBy)
      ? raw.blockedBy
      : undefined
  if (blockedBy) {
    todo.blockedBy = blockedBy.filter(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    )
  }
  return todo
}

/** List a run's plans (initial hydration for the run console). */
export async function listPlans(runId: string, signal?: AbortSignal): Promise<Plan[]> {
  const payload = await requestJson<{ plans?: RawPlan[] }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/plans`,
    { signal },
  )
  const list = Array.isArray(payload.plans) ? payload.plans : []
  return list.map(normalizePlan).filter((p): p is Plan => p !== null)
}

function isAbortSignal(value: AbortSignal | { runId?: string; signal?: AbortSignal }): value is AbortSignal {
  return Boolean(value && typeof value === 'object' && 'aborted' in value && 'addEventListener' in value)
}

/** List a thread's todos (initial hydration for the run console). */
export async function listTodos(
  threadId: string,
  options: AbortSignal | { runId?: string; signal?: AbortSignal } = {},
): Promise<Todo[]> {
  const normalized = isAbortSignal(options) ? { signal: options } : options
  const query = normalized.runId ? `?run_id=${encodeURIComponent(normalized.runId)}` : ''
  const payload = await requestJson<{ todos?: RawTodo[] }>(
    `/api/v1/orchestration/threads/${encodeURIComponent(threadId)}/todos${query}`,
    { signal: normalized.signal },
  )
  const list = Array.isArray(payload.todos) ? payload.todos : []
  return list.map(normalizeTodo).filter((t): t is Todo => t !== null)
}

/**
 * Fetch a thread's sub-agent lineage tree. The shape is provider-defined and
 * rendered by the console as-is, so we return the raw payload unparsed.
 */
export async function getLineage(threadId: string, signal?: AbortSignal): Promise<unknown> {
  return requestJson<unknown>(
    `/api/v1/orchestration/threads/${encodeURIComponent(threadId)}/lineage`,
    { signal },
  )
}
