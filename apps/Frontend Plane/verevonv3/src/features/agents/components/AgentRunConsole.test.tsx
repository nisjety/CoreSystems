// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { RunEventHandlers } from '@/shared/api/run-console-client'
import type { Approval } from '@/shared/api/orchestration-client'
import type { RunDetail } from '@/shared/api/runs-client'

// Regression coverage for task_d6420100: the approve/reject `decide` call can
// 502 even after the decision was actually recorded server-side (the gateway
// now retries once — see `apps/gateway/src/domains/orchestration.rs` — but a
// residual failure must still be reconciled here, not asserted as a hard
// failure). These tests drive `AgentRunConsole` through its real reactive
// state machine, mocking only the network-facing modules — same strategy as
// `run-console-client.test.ts`'s SSE dispatch tests, applied at the component
// level instead of the client-parsing level.

const {
  mockStreamChat,
  mockStreamRunEvents,
  mockListApprovals,
  mockDecideApproval,
  mockResumeRun,
  mockCancelRun,
  mockGetRun,
  mockListRuns,
  mockListSystemRuns,
} = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockStreamRunEvents: vi.fn(),
  mockListApprovals: vi.fn(),
  mockDecideApproval: vi.fn(),
  mockResumeRun: vi.fn(),
  mockCancelRun: vi.fn(),
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
}))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listApprovals: mockListApprovals,
    decideApproval: mockDecideApproval,
    resumeRun: mockResumeRun,
    cancelRun: mockCancelRun,
  }
})
vi.mock('@/shared/api/runs-client', () => ({
  getRun: mockGetRun,
  listRuns: mockListRuns,
  listSystemRuns: mockListSystemRuns,
}))

import AgentRunConsole from './AgentRunConsole'

// jsdom doesn't implement scrollIntoView; the approval-deck effect calls it
// unconditionally when a pending approval appears (unrelated pre-existing gap).
Element.prototype.scrollIntoView = vi.fn()

function renderConsole() {
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={AgentRunConsole} />
    </Router>
  ))
}

function pendingApproval(): Approval {
  return { id: 'appr_1', runId: 'run_1', kind: 'shipping.book_shipment', status: 'PENDING', detail: 'Book a shipment' }
}

function runDetail(): RunDetail {
  return {
    runId: 'run_1',
    threadId: 'thread_1',
    status: 'awaiting_approval',
    goal: 'Book a shipment',
    checkpointIndex: 1,
    stepsCompleted: 1,
    inputTokens: 100,
    outputTokens: 50,
  }
}

/** Types a goal, clicks "Run task", and drives the run to `paused` with one
 * pending approval visible — the state every reconciliation branch starts from. */
async function driveToApprovalDeck(): Promise<RunEventHandlers> {
  let runEventHandlers: RunEventHandlers | undefined

  mockStreamChat.mockImplementation((_request: unknown, handlers: ChatStreamHandlers) => {
    handlers.onConnected?.({ runId: 'run_1', threadId: 'thread_1', model: 'balanced' })
    return new Promise<void>(() => {}) // never resolves — nothing in these tests depends on chat-stream completion
  })
  mockStreamRunEvents.mockImplementation((_runId: string, handlers: RunEventHandlers) => {
    runEventHandlers = handlers
    return new Promise<void>(() => {})
  })
  mockListApprovals.mockResolvedValueOnce([pendingApproval()])

  fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Book a shipment' } })
  fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))

  await waitFor(() => expect(runEventHandlers).toBeDefined())
  runEventHandlers!.onRunPaused?.({ runId: 'run_1', approvalId: 'appr_1', at: '2026-08-05T12:00:00Z' })

  await waitFor(() => expect(screen.getByRole('button', { name: /godkjenn/i })).toBeTruthy())
  return runEventHandlers!
}

describe('AgentRunConsole approval decide reconciliation (task_d6420100)', () => {
  beforeEach(() => {
    mockStreamChat.mockReset()
    mockStreamRunEvents.mockReset()
    mockListApprovals.mockReset()
    mockDecideApproval.mockReset()
    mockResumeRun.mockReset()
    mockCancelRun.mockReset()
    mockGetRun.mockReset().mockResolvedValue(null)
    mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockResumeRun.mockResolvedValue(undefined)
    mockCancelRun.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('treats a 502 as success once the re-fetch confirms the decision actually landed', async () => {
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals
      .mockResolvedValueOnce([{ ...pendingApproval(), status: 'GRANTED' }])
      .mockResolvedValue([{ ...pendingApproval(), status: 'GRANTED' }])

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(mockResumeRun).toHaveBeenCalledWith('run_1'))
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows the existing failure copy — alongside the still-live Resume button — when the re-fetch confirms it is genuinely still pending', async () => {
    // Unlike the other tests, seed a real RunDetail so TelemetryPanel (and its
    // "Resume run" button) actually mounts — this is the other half of the
    // original bug symptom (a false error banner next to a live Resume
    // button); a suite where the button never renders can't catch a
    // regression in how the two combine.
    mockGetRun.mockResolvedValue(runDetail())
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals.mockResolvedValueOnce([pendingApproval()])

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(screen.queryByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).toMatch(/kunne ikke registrere avgjørelsen din/i)
    expect(mockResumeRun).not.toHaveBeenCalled()
    // The run is genuinely still paused, so the Resume control legitimately
    // stays available — that combination is correct here, unlike the
    // confirmed-success case above where no error should appear at all.
    expect(screen.getByRole('button', { name: /gjenoppta kjøring/i })).toBeTruthy()
  })

  it('shows a distinct message when the re-fetch shows it was already decided differently', async () => {
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals.mockResolvedValueOnce([{ ...pendingApproval(), status: 'DENIED' }])

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(screen.queryByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).toMatch(/allerede avgjort/i)
    expect(mockResumeRun).not.toHaveBeenCalled()
  })

  it('shows an ambiguous message when the re-fetch itself cannot confirm anything', async () => {
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals.mockRejectedValueOnce(new Error('network down'))

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(screen.queryByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).toMatch(/fikk ikke bekreftet/i)
    expect(mockResumeRun).not.toHaveBeenCalled()
  })

  it('also shows the ambiguous message when the re-fetch succeeds but no longer lists the approval at all', async () => {
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    // The re-fetch itself succeeds, but returns no matching approval — a
    // different failure mode than the re-fetch rejecting outright.
    mockListApprovals.mockResolvedValueOnce([])

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(screen.queryByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).toMatch(/fikk ikke bekreftet/i)
    expect(mockResumeRun).not.toHaveBeenCalled()
  })

  it('clears a stale banner from an earlier decision once a new one starts', async () => {
    renderConsole()
    await driveToApprovalDeck()

    // First decision: genuinely fails and still pending → banner shown.
    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals.mockResolvedValueOnce([pendingApproval()])
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))
    await waitFor(() => expect(screen.queryByRole('alert')).toBeTruthy())

    // A second pending approval arrives on the same run; deciding it succeeds
    // outright. The stale banner from the first decision must not linger.
    mockListApprovals.mockResolvedValueOnce([{ ...pendingApproval(), id: 'appr_2' }])
    await waitFor(() => expect(mockStreamRunEvents).toHaveBeenCalled())
    const handlers = mockStreamRunEvents.mock.calls[0]![1] as RunEventHandlers
    handlers.onRunPaused?.({ runId: 'run_1', approvalId: 'appr_2', at: '2026-08-05T12:05:00Z' })
    await waitFor(() => expect(screen.getByRole('button', { name: /godkjenn/i })).toBeTruthy())

    mockDecideApproval.mockResolvedValueOnce(null)
    mockListApprovals.mockResolvedValue([])
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(mockResumeRun).toHaveBeenCalledWith('run_1'))
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
