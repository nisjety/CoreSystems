import { ApiError, requestJson } from './http'

/**
 * The closed statuses of a thread goal (EP-7). `cleared` never reaches a view: a cleared goal reads
 * as no goal.
 */
export type ThreadGoalStatus = 'active' | 'paused' | 'met' | 'budget_reached' | 'turns_reached' | 'blocked'

/**
 * A thread goal as the read surfaces show it. The objective and criteria are the owner's own text;
 * the counters stay zero until the budget ledger (EP-7 S2) and the judge (EP-7 S3) fill them.
 * `revision` is the compare-and-set token an edit sends back (EP-7 S1); absent when the server sent none.
 */
export type ThreadGoal = {
  id: string
  threadId: string
  objective: string
  successCriteria: string[]
  status: ThreadGoalStatus
  statusReason?: string
  tokenBudget?: number
  tokensUsed: number
  maxTurns?: number
  turnsUsed: number
  lastVerdict?: string
  revision?: number
  /** The gateway offers Keep going on this deployment (EP-7 S4, `autoContinueAvailable` on the read). */
  autoContinueAvailable?: boolean
}

/** The D11 limits the server holds; the dialog checks them first so a refusal is rare. */
export const THREAD_GOAL_LIMITS = {
  objectiveChars: 2000,
  criteria: 8,
  criterionChars: 500,
  maxTurns: 50,
} as const

const STATUSES: readonly ThreadGoalStatus[] = ['active', 'paused', 'met', 'budget_reached', 'turns_reached', 'blocked']

/** The code the gateway answers (404) while the server's thread goals switch is off. */
export const THREAD_GOALS_DISABLED_CODE = 'thread_goals_disabled'

// Once the server says goals are off (or the route does not exist yet), this tab stops asking: the
// switch is server-controlled and does not flip under a running page, so one answer is enough for
// the session. A 404 that names another code (a thread that is gone) says nothing about the switch.
let goalsUnavailable = false

/** Test seam: forget that the server answered goals-off. */
export function resetThreadGoalAvailability(): void {
  goalsUnavailable = false
}

/** Whether the server has said it serves no goals; the goal bar then offers no goal entry. */
export function threadGoalsUnavailable(): boolean {
  return goalsUnavailable
}

function goalsOff(error: unknown): boolean {
  return (
    error instanceof ApiError &&
    (error.code === THREAD_GOALS_DISABLED_CODE || (error.status === 404 && !error.code))
  )
}

function goalPath(threadId: string): string {
  return `/api/v1/chat/threads/${encodeURIComponent(threadId)}/goal`
}

/**
 * The open or latest goal of a thread, or `null` when the thread has none, the goal was cleared, or
 * the server serves no goals.
 */
export async function getThreadGoal(threadId: string): Promise<ThreadGoal | null> {
  if (goalsUnavailable || !threadId) return null
  try {
    const body = await requestJson<unknown>(goalPath(threadId))
    return normalizeGoalResponse(body)
  } catch (error) {
    if (goalsOff(error)) {
      goalsUnavailable = true
      return null
    }
    throw error
  }
}

/** What the goal dialog sends to set a goal (EP-7 S1). The owner comes from the session. */
export type SetThreadGoalInput = {
  objective: string
  successCriteria?: string[]
  maxTurns?: number
  tokenBudget?: number
  /** Close the thread's open goal as cleared and set this one. */
  replace?: boolean
}

/**
 * Set a goal on one of the person's own threads. Typed refusals reach the caller as `ApiError`
 * (`thread_goal_exists`, `thread_goal_space_unsupported`, `invalid_thread_goal`, a ZDR refusal).
 */
export async function setThreadGoal(threadId: string, input: SetThreadGoalInput): Promise<ThreadGoal | null> {
  const body = await requestJson<unknown>(goalPath(threadId), {
    method: 'PUT',
    body: JSON.stringify({
      objective: input.objective.trim(),
      successCriteria: cleanCriteria(input.successCriteria),
      ...(input.maxTurns ? { maxTurns: input.maxTurns } : {}),
      ...(input.tokenBudget ? { tokenBudget: input.tokenBudget } : {}),
      ...(input.replace ? { replace: true } : {}),
    }),
  })
  return normalizeGoalResponse(body)
}

/** The fields an edit may change; a field left out keeps its value on the server. */
export type UpdateThreadGoalInput = {
  objective?: string
  successCriteria?: string[]
  maxTurns?: number
  tokenBudget?: number
  status?: 'active' | 'paused' | 'cleared'
}

/**
 * Edit, pause, resume or clear a goal under a compare-and-set on the revision the bar showed. A
 * goal changed elsewhere is refused as `thread_goal_revision_conflict`. Answers `null` once the
 * goal is cleared.
 */
export async function updateThreadGoal(
  goal: Pick<ThreadGoal, 'id' | 'threadId' | 'revision'>,
  input: UpdateThreadGoalInput,
): Promise<ThreadGoal | null> {
  const body = await requestJson<unknown>(goalPath(goal.threadId), {
    method: 'PATCH',
    body: JSON.stringify({
      goalId: goal.id,
      expectedRevision: goal.revision ?? 0,
      ...(input.objective !== undefined ? { objective: input.objective.trim() } : {}),
      ...(input.successCriteria !== undefined ? { successCriteria: cleanCriteria(input.successCriteria) } : {}),
      ...(input.maxTurns !== undefined ? { maxTurns: input.maxTurns } : {}),
      ...(input.tokenBudget !== undefined ? { tokenBudget: input.tokenBudget } : {}),
      ...(input.status !== undefined ? { status: input.status } : {}),
    }),
  })
  return normalizeGoalResponse(body)
}

function cleanCriteria(lines: string[] | undefined): string[] {
  return (lines ?? []).map((line) => line.trim()).filter((line) => line.length > 0)
}

/** Reads `{ goal, found }` (the `ThreadGoalResponse` shape); anything without a usable goal is `null`. */
export function normalizeGoalResponse(value: unknown): ThreadGoal | null {
  const record = objectValue(value)
  if (!record || record.found === false) return null
  const goal = normalizeGoal(record.goal)
  return goal && record.autoContinueAvailable === true ? { ...goal, autoContinueAvailable: true } : goal
}

function normalizeGoal(value: unknown): ThreadGoal | null {
  const record = objectValue(value)
  const id = text(record?.id)
  const objective = text(record?.objective)
  const status = STATUSES.find((candidate) => candidate === record?.status)
  if (!record || !id || !objective || !status) return null
  const criteria = Array.isArray(record.successCriteria)
    ? record.successCriteria.filter((line): line is string => typeof line === 'string' && line.trim().length > 0)
    : []
  const statusReason = text(record.statusReason)
  const tokenBudget = count(record.tokenBudget)
  const maxTurns = count(record.maxTurns)
  const lastVerdict = text(record.lastVerdict)
  const revision = count(record.revision)
  return {
    id,
    threadId: text(record.threadId),
    objective,
    successCriteria: criteria,
    status,
    ...(statusReason ? { statusReason } : {}),
    ...(tokenBudget > 0 ? { tokenBudget } : {}),
    tokensUsed: count(record.tokensUsed),
    ...(maxTurns > 0 ? { maxTurns } : {}),
    turnsUsed: count(record.turnsUsed),
    ...(lastVerdict ? { lastVerdict } : {}),
    ...(revision > 0 ? { revision } : {}),
  }
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function text(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.trunc(value)) : 0
}
