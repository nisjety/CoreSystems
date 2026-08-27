// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { RunEventHandlers } from '@/shared/api/run-console-client'
import type { RunDetail } from '@/shared/api/runs-client'

// Coverage for the run console's residency/tier line: the "Personvern" stat
// sits beside the existing cost stat and renders ONLY what the durable run
// record actually carries — absent cleanly when the run has no provenance.

const {
  mockStreamChat,
  mockStreamRunEvents,
  mockGetRunProofBundle,
  mockListApprovals,
  mockGetRun,
  mockListRuns,
  mockListSystemRuns,
} = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockStreamRunEvents: vi.fn(),
  mockGetRunProofBundle: vi.fn(),
  mockListApprovals: vi.fn(),
  mockGetRun: vi.fn(),
  mockListRuns: vi.fn(),
  mockListSystemRuns: vi.fn(),
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
  return { ...actual, listApprovals: mockListApprovals }
})
vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return {
    ...actual,
    getRun: mockGetRun,
    listRuns: mockListRuns,
    listSystemRuns: mockListSystemRuns,
  }
})

import AgentRunConsole from './AgentRunConsole'

Element.prototype.scrollIntoView = vi.fn()

function runDetail(overrides: Partial<RunDetail>): RunDetail {
  return {
    runId: 'run_1',
    threadId: 'thread_1',
    status: 'completed',
    goal: 'Book a shipment',
    checkpointIndex: 0,
    stepsCompleted: 1,
    inputTokens: 100,
    outputTokens: 50,
    ...overrides,
  }
}

async function runToCompletion(detail: RunDetail) {
  let handlers: RunEventHandlers | undefined
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
  mockStreamChat.mockImplementation((_request: unknown, chatHandlers: ChatStreamHandlers) => {
    chatHandlers.onConnected?.({ runId: 'run_1', threadId: 'thread_1', model: 'balanced' })
    return new Promise<void>(() => {})
  })
  mockStreamRunEvents.mockImplementation((_runId: string, eventHandlers: RunEventHandlers) => {
    handlers = eventHandlers
    return new Promise<void>(() => {})
  })
  mockListApprovals.mockResolvedValue([])
  mockGetRun.mockResolvedValue(detail)

  fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Book a shipment' } })
  fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))
  await waitFor(() => expect(handlers).toBeDefined())
  handlers!.onDone?.()
  await waitFor(() => expect(screen.getByText(/kjøretelemetri/i)).toBeTruthy())
}

beforeEach(() => {
  mockStreamChat.mockReset()
  mockStreamRunEvents.mockReset()
  mockGetRunProofBundle.mockReset().mockResolvedValue(null)
  mockListApprovals.mockReset().mockResolvedValue([])
  mockGetRun.mockReset().mockResolvedValue(null)
  mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
  mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
})

describe('run console privacy provenance line', () => {
  it('renders tier + region beside the cost stats when the run carries them', async () => {
    await runToCompletion(runDetail({ privacyTier: 'sovereign', residency: 'norway-east' }))

    expect(screen.getByText('Personvern')).toBeTruthy()
    const value = screen.getByText(/Norge · norway-east/)
    expect(value).toBeTruthy()
  })

  it('falls back to the raw region alone when the tier is unspecified', async () => {
    await runToCompletion(runDetail({ residency: 'eu-central-1' }))

    expect(screen.getByText('Personvern')).toBeTruthy()
    expect(screen.getByText('eu-central-1')).toBeTruthy()
  })

  it('shows nothing at all when the run record carries no provenance — never a fabricated claim', async () => {
    await runToCompletion(runDetail({}))

    expect(screen.queryByText('Personvern')).toBeNull()
  })
})
