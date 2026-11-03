import { ApiError, requestJson } from './http'

/**
 * The closed statuses of a thread goal (EP-7). `cleared` never reaches a view: a cleared goal reads
 * as no goal.
 */
export type ThreadGoalStatus = 'active' | 'paused' | 'met' | 'budget_reached' | 'turns_reached' | 'blocked'

/**
 * A thread goal as the read surfaces show it. The objective and criteria are the owner's own text;
 * the counters stay zero until the budget ledger (EP-7 S2) and the judge (EP-7 S3) fill them.
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
}

const STATUSES: readonly ThreadGoalStatus[] = ['active', 'paused', 'met', 'budget_reached', 'turns_reached', 'blocked']

/** The code the gateway answers (404) while the server's thread goals switch is off. */
export const THREAD_GOALS_DISABLED_CODE = 'thread_goals_disabled'

// Once the server says goals are off (or the route does not exist yet), this tab stops asking: the
// switch is server-controlled and does not flip under a running page, so one 404 answers for the session.
let goalsUnavailable = false

/** Test seam: forget that the server answered goals-off. */
export function resetThreadGoalAvailability(): void {
  goalsUnavailable = false
}

/**
 * The open or latest goal of a thread, or `null` when the thread has none, the goal was cleared, or
 * the server serves no goals. Read-only (EP-7 S5); setting and editing a goal belong to the chat
 * thread's goal bar (EP-7 S1, decision D1).
 */
export async function getThreadGoal(threadId: string): Promise<ThreadGoal | null> {
  if (goalsUnavailable || !threadId) return null
  try {
    const body = await requestJson<unknown>(`/api/v1/chat/threads/${encodeURIComponent(threadId)}/goal`)
    return normalizeGoalResponse(body)
  } catch (error) {
    if (error instanceof ApiError && (error.status === 404 || error.code === THREAD_GOALS_DISABLED_CODE)) {
      goalsUnavailable = true
      return null
    }
    throw error
  }
}

/** Reads `{ goal, found }` (the `ThreadGoalResponse` shape); anything without a usable goal is `null`. */
export function normalizeGoalResponse(value: unknown): ThreadGoal | null {
  const record = objectValue(value)
  if (!record || record.found === false) return null
  return normalizeGoal(record.goal)
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
