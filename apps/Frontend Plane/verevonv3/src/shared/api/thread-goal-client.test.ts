import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, requestJson } from './http'
import {
  getThreadGoal,
  normalizeGoalResponse,
  resetThreadGoalAvailability,
  setThreadGoal,
  threadGoalsUnavailable,
  updateThreadGoal,
} from './thread-goal-client'

vi.mock('./http', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./http')>()),
  requestJson: vi.fn(),
}))

const requestJsonMock = vi.mocked(requestJson)

const wireGoal = {
  id: 'goal_1',
  threadId: 'thread-1',
  objective: 'Ship the report',
  successCriteria: ['Numbers reconciled', '  '],
  status: 'active',
  maxTurns: 10,
  revision: 3,
  tokensUsed: 0,
  turnsUsed: 0,
}

describe('thread goal client', () => {
  beforeEach(() => {
    requestJsonMock.mockReset()
    resetThreadGoalAvailability()
  })

  it('reads the goal from the found envelope and drops blank criteria', async () => {
    requestJsonMock.mockResolvedValue({ found: true, goal: wireGoal })

    const goal = await getThreadGoal('thread-1')

    expect(requestJsonMock).toHaveBeenCalledWith('/api/v1/chat/threads/thread-1/goal')
    expect(goal).toMatchObject({ id: 'goal_1', status: 'active', revision: 3, successCriteria: ['Numbers reconciled'] })
  })

  it('reads a thread without a goal, or with an unknown status, as no goal', () => {
    expect(normalizeGoalResponse({ found: false, goal: null })).toBeNull()
    expect(normalizeGoalResponse({ found: true, goal: { ...wireGoal, status: 'cleared' } })).toBeNull()
  })

  it('stops asking once the server says goals are off, and not for a thread that is gone', async () => {
    requestJsonMock.mockRejectedValueOnce(new ApiError('gone', 404, 'thread_not_found'))
    await expect(getThreadGoal('thread-1')).rejects.toBeInstanceOf(ApiError)
    expect(threadGoalsUnavailable()).toBe(false)

    requestJsonMock.mockRejectedValueOnce(new ApiError('off', 404, 'thread_goals_disabled'))
    expect(await getThreadGoal('thread-1')).toBeNull()
    expect(threadGoalsUnavailable()).toBe(true)
    expect(await getThreadGoal('thread-2')).toBeNull()
    expect(requestJsonMock).toHaveBeenCalledTimes(2)
  })

  it('sets a goal with trimmed text and only the fields it was given', async () => {
    requestJsonMock.mockResolvedValue({ found: true, goal: wireGoal })

    await setThreadGoal('thread-1', { objective: '  Ship the report ', successCriteria: ['a', ' '] })

    const [path, init] = requestJsonMock.mock.calls[0] ?? []
    expect(path).toBe('/api/v1/chat/threads/thread-1/goal')
    expect(init?.method).toBe('PUT')
    expect(JSON.parse(String(init?.body))).toEqual({ objective: 'Ship the report', successCriteria: ['a'] })
  })

  it('edits under the revision the bar showed and sends only the changed fields', async () => {
    requestJsonMock.mockResolvedValue({ found: false, goal: null })

    const result = await updateThreadGoal({ id: 'goal_1', threadId: 'thread-1', revision: 3 }, { status: 'cleared' })

    const [, init] = requestJsonMock.mock.calls[0] ?? []
    expect(init?.method).toBe('PATCH')
    expect(JSON.parse(String(init?.body))).toEqual({ goalId: 'goal_1', expectedRevision: 3, status: 'cleared' })
    expect(result).toBeNull()
  })
})
