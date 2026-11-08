// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'

// EP-7 S5: the console shows the goal of the run's thread read-only, beside the plan panel, and
// links to the chat thread for edits (decision D1). Only the network-facing modules are mocked.

const { mockStreamChat, mockGetThreadGoal } = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockGetThreadGoal: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, streamChat: mockStreamChat }
})
vi.mock('@/shared/api/thread-goal-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/thread-goal-client')>()
  return { ...actual, getThreadGoal: mockGetThreadGoal }
})
vi.mock('@/shared/api/run-console-client', () => ({
  streamRunEvents: vi.fn(() => new Promise<void>(() => {})),
  getRunProofBundle: vi.fn(async () => null),
}))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listApprovals: vi.fn(async () => []),
    decideApproval: vi.fn(),
    resumeRun: vi.fn(),
    cancelRun: vi.fn(),
    listPlans: vi.fn(async () => []),
    listTodos: vi.fn(async () => []),
  }
})
vi.mock('@/shared/api/runs-client', () => ({
  getRun: vi.fn(async () => null),
  listRuns: vi.fn(async () => ({ runs: [] })),
  listSystemRuns: vi.fn(async () => ({ runs: [] })),
  watchRun: vi.fn(),
  unwatchRun: vi.fn(),
  getRunWatchStatus: vi.fn(async () => ({ watching: false })),
}))

import AgentRunConsole, { GoalPanel } from './AgentRunConsole'
import { normalizeGoalResponse } from '@/shared/api/thread-goal-client'

Element.prototype.scrollIntoView = vi.fn()

function goal(overrides: Partial<ThreadGoal> = {}): ThreadGoal {
  return {
    id: 'goal_1',
    threadId: 'thread_1',
    objective: 'Find three EU suppliers with a website each',
    successCriteria: ['Three suppliers', 'Each has a website'],
    status: 'turns_reached',
    tokenBudget: 200000,
    tokensUsed: 41000,
    maxTurns: 10,
    turnsUsed: 10,
    lastVerdict: 'not_met',
    ...overrides,
  }
}

function renderConsole() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function startRun() {
  mockStreamChat.mockImplementation((_request: unknown, handlers: ChatStreamHandlers) => {
    handlers.onConnected?.({ runId: 'run_1', threadId: 'thread_1', model: 'balanced' })
    return new Promise<void>(() => {})
  })
  fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Book a shipment' } })
  flush()
  fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))
}

beforeEach(() => {
  mockStreamChat.mockReset()
  mockGetThreadGoal.mockReset()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('GoalPanel', () => {
  it('shows the objective, criteria, usage line, status and last verdict', () => {
    const { container, unmount } = render(() => <GoalPanel goal={goal()} threadId="thread_1" />)
    const text = container.textContent ?? ''
    expect(text).toContain('Find three EU suppliers with a website each')
    expect(text).toContain('Three suppliers')
    expect(text).toContain('41000 av 200000 tokens')
    expect(text).toContain('tur 10 av 10')
    expect(text).toContain('turns_reached')
    expect(text).toContain('not_met')
    unmount()
  })

  it('links to the chat thread for edits and offers no goal controls', () => {
    const { container, unmount } = render(() => <GoalPanel goal={goal()} threadId="thread 1" />)
    const link = container.querySelector('a.verevon-run-goal__open')
    expect(link?.getAttribute('href')).toBe('/chat?thread=thread%201')
    expect(link?.textContent).toContain('Åpne i chat')
    // Read-only (D1): no button and no input of any kind.
    expect(container.querySelectorAll('button, input, textarea, select')).toHaveLength(0)
    unmount()
  })

  it('omits the usage line and the verdict before the ledger and the judge fill them', () => {
    const bare = goal({ tokenBudget: undefined, maxTurns: undefined, lastVerdict: undefined, tokensUsed: 0, turnsUsed: 0 })
    const { container, unmount } = render(() => <GoalPanel goal={bare} threadId="thread_1" />)
    expect(container.querySelector('.verevon-run-goal__usage')).toBeNull()
    expect(container.querySelector('.verevon-run-goal__verdict')).toBeNull()
    unmount()
  })
})

describe('AgentRunConsole goal view', () => {
  it('shows the thread goal once the run has a thread', async () => {
    mockGetThreadGoal.mockResolvedValue(goal())
    renderConsole()
    expect(mockGetThreadGoal).not.toHaveBeenCalled()
    startRun()
    await waitFor(() => expect(screen.getByText('Find three EU suppliers with a website each')).toBeTruthy())
    expect(mockGetThreadGoal).toHaveBeenCalledWith('thread_1')
  })

  it('shows no goal panel when the thread has no goal or the read fails', async () => {
    mockGetThreadGoal.mockResolvedValueOnce(null)
    const first = renderConsole()
    startRun()
    await waitFor(() => expect(mockGetThreadGoal).toHaveBeenCalled())
    expect(first.container.querySelector('.verevon-run-goal')).toBeNull()
    first.unmount()

    mockGetThreadGoal.mockRejectedValueOnce(new Error('boom'))
    const second = renderConsole()
    startRun()
    await waitFor(() => expect(mockGetThreadGoal).toHaveBeenCalledTimes(2))
    expect(second.container.querySelector('.verevon-run-goal')).toBeNull()
    expect(second.container.textContent).not.toContain('boom')
    second.unmount()
  })
})

describe('normalizeGoalResponse', () => {
  it('reads a found goal and drops a cleared, unknown or missing one', () => {
    const wire = {
      found: true,
      goal: { id: 'goal_1', threadId: 'thread_1', objective: 'Ship it', status: 'met', successCriteria: ['a', 7, ''] },
    }
    expect(normalizeGoalResponse(wire)).toMatchObject({ id: 'goal_1', status: 'met', successCriteria: ['a'] })
    expect(normalizeGoalResponse({ found: false, goal: wire.goal })).toBeNull()
    expect(normalizeGoalResponse({ found: true, goal: { ...wire.goal, status: 'cleared' } })).toBeNull()
    expect(normalizeGoalResponse({ found: true, goal: { ...wire.goal, objective: '' } })).toBeNull()
    expect(normalizeGoalResponse(null)).toBeNull()
  })
})
