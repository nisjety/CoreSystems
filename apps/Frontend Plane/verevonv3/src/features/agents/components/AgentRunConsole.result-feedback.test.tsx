// @vitest-environment jsdom

// A finished agent answer is rated by its run, with the same thumbs as a chat answer. Off by
// default: with the switch on the console offers the rating once the answer is done; with it
// off the console is unchanged.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'

const {
  mockStreamChat,
  mockStreamRunEvents,
  mockGetRunProofBundle,
  mockListApprovals,
  mockGetRun,
  mockListRuns,
  mockListSystemRuns,
  mockWatchRun,
  mockUnwatchRun,
  mockGetRunWatchStatus,
} = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockStreamRunEvents: vi.fn(),
  mockGetRunProofBundle: vi.fn(),
  mockListApprovals: vi.fn(),
  mockGetRun: vi.fn(),
  mockListRuns: vi.fn(),
  mockListSystemRuns: vi.fn(),
  mockWatchRun: vi.fn(),
  mockUnwatchRun: vi.fn(),
  mockGetRunWatchStatus: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, streamChat: mockStreamChat }
})
vi.mock('@/shared/api/run-console-client', () => ({
  streamRunEvents: mockStreamRunEvents,
  getRunProofBundle: mockGetRunProofBundle,
}))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listApprovals: mockListApprovals,
    decideApproval: vi.fn(),
    resumeRun: vi.fn(),
    cancelRun: vi.fn(),
    listPlans: vi.fn(async () => []),
    listTodos: vi.fn(async () => []),
  }
})
vi.mock('@/shared/api/runs-client', () => ({
  getRun: mockGetRun,
  listRuns: mockListRuns,
  listSystemRuns: mockListSystemRuns,
  watchRun: mockWatchRun,
  unwatchRun: mockUnwatchRun,
  getRunWatchStatus: mockGetRunWatchStatus,
}))

import AgentRunConsole from './AgentRunConsole'

// jsdom does not implement scrollIntoView; the approval-deck effect calls it.
Element.prototype.scrollIntoView = vi.fn()

function renderConsole() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

/** Rating calls are accepted. The console's other reads get an empty 404 and fall back to nothing. */
function routedFetch() {
  return vi.fn(async (path: string, _init?: RequestInit) =>
    path === '/api/v1/chat/feedback'
      ? jsonResponse({ accepted: true })
      : jsonResponse({ error: { code: 'not_found', message: 'none' } }, 404),
  )
}

/** The bodies sent to the rating route, in order. */
function sentRatings(fetchMock: { mock: { calls: unknown[][] } }): Array<Record<string, unknown>> {
  return fetchMock.mock.calls
    .filter((call) => call[0] === '/api/v1/chat/feedback')
    .map((call) => JSON.parse(String((call[1] as RequestInit).body)))
}

/** A run that streams one answer and then finishes. */
function streamAFinishedAnswer() {
  mockStreamChat.mockImplementation((_request: unknown, handlers: ChatStreamHandlers) => {
    handlers.onConnected?.({ runId: 'run_1', threadId: 'thread_1', model: 'balanced' })
    handlers.onMessage?.({ content: 'Prisene er samlet i sammendraget.' })
    return Promise.resolve()
  })
}

/** Starts a task from the launcher and waits until its answer is finished. */
async function runTaskToFinish() {
  renderConsole()
  fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Samle prisene' } })
  flush()
  fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))
  await waitFor(() => expect(document.querySelector('.verevon-run-answer__status--done')).toBeTruthy())
}

beforeEach(() => {
  mockStreamChat.mockReset()
  mockStreamRunEvents.mockReset().mockImplementation(() => new Promise<void>(() => {}))
  mockGetRunProofBundle.mockReset().mockResolvedValue(null)
  mockListApprovals.mockReset().mockResolvedValue([])
  mockGetRun.mockReset().mockResolvedValue(null)
  mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
  mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
  mockWatchRun.mockReset().mockResolvedValue({ watching: true })
  mockUnwatchRun.mockReset().mockResolvedValue(undefined)
  mockGetRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('AgentRunConsole result feedback', () => {
  it('rates a finished agent answer by its run id, with the same thumbs as a chat answer', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = routedFetch()
    vi.stubGlobal('fetch', fetchMock)
    streamAFinishedAnswer()

    await runTaskToFinish()
    fireEvent.click(await screen.findByRole('button', { name: 'Godt svar' }))

    await waitFor(() => expect(sentRatings(fetchMock)).toEqual([{ run_id: 'run_1', rating: 'positive' }]))
  })

  it('adds no rating to a finished answer while the switch is off', async () => {
    const fetchMock = routedFetch()
    vi.stubGlobal('fetch', fetchMock)
    streamAFinishedAnswer()

    await runTaskToFinish()

    expect(screen.queryByRole('group', { name: 'Vurder svaret' })).toBeNull()
    expect(sentRatings(fetchMock)).toEqual([])
  })
})
