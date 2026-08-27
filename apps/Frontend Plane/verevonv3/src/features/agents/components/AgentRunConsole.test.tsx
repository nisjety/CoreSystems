// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { ProofBundle, RunEventHandlers } from '@/shared/api/run-console-client'
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
  mockGetRunProofBundle,
  mockListApprovals,
  mockDecideApproval,
  mockResumeRun,
  mockCancelRun,
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
  mockDecideApproval: vi.fn(),
  mockResumeRun: vi.fn(),
  mockCancelRun: vi.fn(),
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
    decideApproval: mockDecideApproval,
    resumeRun: mockResumeRun,
    cancelRun: mockCancelRun,
    // The plan panel's resource fires as soon as a run id exists. Stubbed so
    // console tests stay deterministic and offline; PlanPanel's own rendering is
    // asserted directly below.
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

import AgentRunConsole, { BrowserObservationShot, PlanPanel } from './AgentRunConsole'

// jsdom doesn't implement scrollIntoView; the approval-deck effect calls it
// unconditionally when a pending approval appears (unrelated pre-existing gap).
Element.prototype.scrollIntoView = vi.fn()

function renderConsole() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
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
  flush()
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
    mockGetRunProofBundle.mockReset().mockResolvedValue(null)
    mockListApprovals.mockReset()
    mockDecideApproval.mockReset()
    mockResumeRun.mockReset()
    mockCancelRun.mockReset()
    mockGetRun.mockReset().mockResolvedValue(null)
    mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockWatchRun.mockReset().mockResolvedValue({ watching: true })
    mockUnwatchRun.mockReset().mockResolvedValue(undefined)
    mockGetRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
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

  it('treats a 502 as success for a rejection too, once the re-fetch confirms it landed', async () => {
    // The other "confirmed success" test only ever exercises approve — this
    // covers applyDecisionFollowThrough's other branch (cancelRun + the
    // cancelled-status flip), which otherwise has zero coverage.
    renderConsole()
    await driveToApprovalDeck()

    mockDecideApproval.mockRejectedValueOnce(new Error('502 Bad Gateway'))
    mockListApprovals
      .mockResolvedValueOnce([{ ...pendingApproval(), status: 'DENIED' }])
      .mockResolvedValue([{ ...pendingApproval(), status: 'DENIED' }])

    fireEvent.click(screen.getByRole('button', { name: /avvis/i }))

    await waitFor(() => expect(mockCancelRun).toHaveBeenCalledWith('run_1'))
    expect(screen.queryByRole('alert')).toBeNull()
    // Exact match: the header status pill's own text is exactly "Kansellert" —
    // a substring/regex match would also hit AnswerPanel's unrelated sentence
    // ("Kjøring kansellert — delvis output over"), matching two elements.
    expect(screen.getByText('Kansellert')).toBeTruthy()
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

// ── Verevon Proof Bundle panel ───────────────────────────────────────────────
// The panel's whole contract is honesty about absence. `execution: null`,
// `outcome: null`, and `verification: null` each mean "not proven", and none of
// them may render as either success or failure — a reader must be able to tell
// "not proven yet" from "nothing happened". The `unavailable` sections are the
// same claim at bundle level and are always shown, reasons included.

/** Starts a run and connects it, so the console pins to `run_1` and fetches that
 * run's proof bundle. Nothing is pending — these tests are about the durable
 * evidence record, not the live HITL deck. */
function driveToConnectedRun(): void {
  mockStreamChat.mockImplementation((_request: unknown, handlers: ChatStreamHandlers) => {
    handlers.onConnected?.({ runId: 'run_1', threadId: 'thread_1', model: 'balanced' })
    return new Promise<void>(() => {}) // never resolves — nothing here depends on chat-stream completion
  })
  mockStreamRunEvents.mockImplementation(() => new Promise<void>(() => {}))
  mockListApprovals.mockResolvedValue([])

  fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Book a shipment' } })
  flush()
  fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))
}

function proofBundle(overrides: Partial<ProofBundle> = {}): ProofBundle {
  return {
    bundleVersion: 1,
    runId: 'run_1',
    orgId: 'org-1',
    generatedAt: '2026-08-08T10:00:00Z',
    run: { goal: 'Book a shipment', agentId: 'general-v1', status: 'completed', createdAt: '2026-08-08T09:00:00Z' },
    approvals: [],
    unavailable: [
      { section: 'known', reason: 'Retrieval context is not captured per run.' },
      { section: 'charged', reason: 'Billing is reconciled outside the run record.' },
      { section: 'retained', reason: 'Retention policy is evaluated at the storage boundary.' },
    ],
    ...overrides,
  }
}

/** The evidence-chain block whose pill reads `label`. */
function claimFor(label: string): HTMLElement {
  const node = screen.getByText(label).closest('.verevon-run-proof__claim')
  expect(node).toBeTruthy()
  return node as HTMLElement
}

describe('AgentRunConsole proof bundle panel', () => {
  beforeEach(() => {
    mockStreamChat.mockReset()
    mockStreamRunEvents.mockReset()
    mockGetRunProofBundle.mockReset().mockResolvedValue(null)
    mockListApprovals.mockReset()
    mockDecideApproval.mockReset()
    mockResumeRun.mockReset().mockResolvedValue(undefined)
    mockCancelRun.mockReset().mockResolvedValue(undefined)
    mockGetRun.mockReset().mockResolvedValue(null)
    mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockWatchRun.mockReset().mockResolvedValue({ watching: true })
    mockUnwatchRun.mockReset().mockResolvedValue(undefined)
    mockGetRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
  })

  it('renders a granted approval with no execution as unproven — neither failed nor succeeded', async () => {
    mockGetRunProofBundle.mockResolvedValue(proofBundle({
      approvals: [{
        approvalId: 'appr_1',
        kind: 'tool',
        status: 'granted',
        requestedBy: 'model',
        decidedBy: 'user-1',
        decisionReason: 'looks right',
        requestedAt: '2026-08-08T09:10:00Z',
        decidedAt: '2026-08-08T09:11:00Z',
        execution: null,
      }],
    }))

    renderConsole()
    driveToConnectedRun()

    await waitFor(() => expect(screen.getByText('Ingen utførelse registrert')).toBeTruthy())
    const claim = claimFor('Ingen utførelse registrert')
    expect(claim.classList.contains('verevon-run-proof__claim--unproven')).toBe(true)
    expect(claim.querySelector('.verevon-run-proof__pill--error')).toBeNull()
    expect(claim.querySelector('.verevon-run-proof__pill--ok')).toBeNull()
    expect(claim.querySelector('.verevon-run-proof__pill--unproven')).toBeTruthy()

    // The authorization itself is still reported truthfully as granted; only
    // what followed from it is unproven.
    expect(screen.getByText('Godkjent')).toBeTruthy()
    // Nothing executed, so there is no outcome to verify and no verification
    // claim is made either way.
    expect(screen.queryByText('Verifisering')).toBeNull()
  })

  it('does not render a finalized outcome with no verification as verified', async () => {
    mockGetRunProofBundle.mockResolvedValue(proofBundle({
      approvals: [{
        approvalId: 'appr_1',
        kind: 'shipping.book_shipment',
        status: 'granted',
        decidedBy: 'user-1',
        execution: {
          receiptId: 'rcpt_1',
          deliveryId: 'dlv_1',
          actionFingerprint: 'a'.repeat(64),
          executionServiceId: 'execution-core',
          descriptorVersion: 1,
          startedAt: '2026-08-08T09:30:00Z',
          outcome: {
            outcome: 'completed',
            providerReceiptId: 'LC652849244NO',
            finalizedAt: '2026-08-08T09:31:00Z',
            verification: null,
          },
        },
      }],
    }))

    renderConsole()
    driveToConnectedRun()

    await waitFor(() => expect(screen.getByText('Ingen uavhengig verifisering')).toBeTruthy())
    const claim = claimFor('Ingen uavhengig verifisering')
    expect(claim.classList.contains('verevon-run-proof__claim--unproven')).toBe(true)
    expect(claim.querySelector('.verevon-run-proof__pill--ok')).toBeNull()
    expect(claim.querySelector('.verevon-run-proof__pill--error')).toBeNull()

    // The executor's own "finalized" report is still shown — and stays
    // visibly distinct from an independently verified outcome.
    expect(screen.getByText('Sluttført')).toBeTruthy()
    expect(screen.queryByText('Verifisert')).toBeNull()
  })

  it('renders a started-but-unfinalized execution as in flight, not as a failure', async () => {
    mockGetRunProofBundle.mockResolvedValue(proofBundle({
      approvals: [{
        approvalId: 'appr_1',
        kind: 'tool',
        status: 'granted',
        execution: {
          receiptId: 'rcpt_1',
          actionFingerprint: 'b'.repeat(64),
          executionServiceId: 'execution-core',
          startedAt: '2026-08-08T09:30:00Z',
          outcome: null,
        },
      }],
    }))

    renderConsole()
    driveToConnectedRun()

    await waitFor(() => expect(screen.getByText('Under utførelse')).toBeTruthy())
    const claim = claimFor('Under utførelse')
    expect(claim.querySelector('.verevon-run-proof__pill--error')).toBeNull()
    expect(claim.querySelector('.verevon-run-proof__pill--ok')).toBeNull()
    // Work that has not finalized cannot be verified, so no verification claim
    // is made — an absent verification here must not read as a missing one.
    expect(screen.queryByText('Ingen uavhengig verifisering')).toBeNull()
  })

  it('always renders the unavailable sections and every reason, even with approvals present', async () => {
    mockGetRunProofBundle.mockResolvedValue(proofBundle({
      approvals: [{ approvalId: 'appr_1', kind: 'tool', status: 'granted', execution: null }],
    }))

    renderConsole()
    driveToConnectedRun()

    await waitFor(() => expect(screen.getByText('Ikke dekket av denne pakken')).toBeTruthy())
    expect(screen.getByText('Hva kjøringen visste')).toBeTruthy()
    expect(screen.getByText('Hva det kostet')).toBeTruthy()
    expect(screen.getByText('Hvilken lagring som gjaldt')).toBeTruthy()
    expect(screen.getByText('Retrieval context is not captured per run.')).toBeTruthy()
    expect(screen.getByText('Billing is reconciled outside the run record.')).toBeTruthy()
    expect(screen.getByText('Retention policy is evaluated at the storage boundary.')).toBeTruthy()
  })

  it('says a failed proof fetch is a fetch failure, not evidence that nothing happened', async () => {
    mockGetRunProofBundle.mockRejectedValue(new Error('503 Service Unavailable'))

    renderConsole()
    driveToConnectedRun()

    await waitFor(() => expect(screen.getByText(/kunne ikke hente bevispakken/i)).toBeTruthy())
    expect(screen.getByText(/ikke et bevis på at ingenting skjedde/i)).toBeTruthy()
  })
})

// ── Run watcher toggle (AUTO-2) ──────────────────────────────────────────────
// "Notify me when this run finishes" — a standing per-user subscription
// registered/cancelled through the gateway's `/api/v1/runs/:run_id/watchers`
// proxy (`runs-client`'s `watchRun`/`unwatchRun`/`getRunWatchStatus`).
// Distinct from the live event stream, which these tests don't otherwise
// exercise: `mockGetRun` resolves a real `RunDetail` so `TelemetryPanel` (and
// the toggle it hosts) actually mounts, same technique the reconciliation
// suite above uses for its "still-live Resume button" case.

describe('AgentRunConsole run watcher toggle', () => {
  beforeEach(() => {
    mockStreamChat.mockReset()
    mockStreamRunEvents.mockReset()
    mockGetRunProofBundle.mockReset().mockResolvedValue(null)
    mockListApprovals.mockReset().mockResolvedValue([])
    mockDecideApproval.mockReset()
    mockResumeRun.mockReset().mockResolvedValue(undefined)
    mockCancelRun.mockReset().mockResolvedValue(undefined)
    mockGetRun.mockReset().mockResolvedValue(runDetail())
    mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockWatchRun.mockReset().mockResolvedValue({ watching: true })
    mockUnwatchRun.mockReset().mockResolvedValue(undefined)
    mockGetRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('loads the initial status and registers a subscription on click', async () => {
    renderConsole()
    driveToConnectedRun()

    const toggle = await screen.findByRole('button', { name: /varsle meg/i })
    await waitFor(() => expect(mockGetRunWatchStatus).toHaveBeenCalledWith('run_1'))

    fireEvent.click(toggle)

    await waitFor(() => expect(mockWatchRun).toHaveBeenCalledWith('run_1'))
    await waitFor(() => expect(screen.getByRole('button', { name: /varsler/i })).toBeTruthy())
    expect(mockUnwatchRun).not.toHaveBeenCalled()
  })

  it('cancels an existing subscription on a second click', async () => {
    mockGetRunWatchStatus.mockResolvedValue({ watching: true })

    renderConsole()
    driveToConnectedRun()

    const toggle = await screen.findByRole('button', { name: /varsler/i })
    fireEvent.click(toggle)

    await waitFor(() => expect(mockUnwatchRun).toHaveBeenCalledWith('run_1'))
    await waitFor(() => expect(screen.getByRole('button', { name: /varsle meg/i })).toBeTruthy())
    expect(mockWatchRun).not.toHaveBeenCalled()
  })

  it('reverts the optimistic state when the watch call fails', async () => {
    mockWatchRun.mockRejectedValue(new Error('502 Bad Gateway'))

    renderConsole()
    driveToConnectedRun()

    const toggle = await screen.findByRole('button', { name: /varsle meg/i })
    fireEvent.click(toggle)

    await waitFor(() => expect(mockWatchRun).toHaveBeenCalled())
    // A failed registration must not leave the button falsely claiming it
    // succeeded — it reverts to "Notify me" rather than staying on "Watching".
    await waitFor(() => expect(screen.getByRole('button', { name: /varsle meg/i })).toBeTruthy())
  })
})

describe('AgentRunConsole chat thread origin', () => {
  beforeEach(() => {
    mockStreamChat.mockReset()
    mockListApprovals.mockReset().mockResolvedValue([])
    mockGetRun.mockReset().mockResolvedValue(null)
    mockGetRunProofBundle.mockReset().mockResolvedValue(null)
    mockListRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockListSystemRuns.mockReset().mockResolvedValue({ runs: [], hasMore: false })
    mockGetRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
  })

  // Regression coverage: with no sessionKey, model-gateway's create_thread
  // falls back to a random session key, which session-core's
  // resolve_thread_origin classifies as "chat" — landing this console's runs
  // in the Verevon chat history it must stay invisible to (see
  // apps/gateway/src/domains/chat/history.rs's `chat_history_sessions`).
  it('tags the chat stream with an agent_run/ scoped session key, not a bare chat thread', async () => {
    mockStreamChat.mockImplementation(() => new Promise<void>(() => {}))

    renderConsole()

    fireEvent.input(screen.getByLabelText(/hva skal agenten gjøre/i), { target: { value: 'Book a shipment' } })
    fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))

    await waitFor(() => expect(mockStreamChat).toHaveBeenCalled())
    const [request] = mockStreamChat.mock.calls[0] as [{ sessionKey?: string }]
    expect(request.sessionKey).toMatch(
      /^agent_run\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    )
  })
})

describe('PlanPanel', () => {
  it('states absence rather than rendering an empty panel', () => {
    // An empty panel reads as a loading failure. A run with no plan is an
    // ordinary outcome and has to say so — same honesty rule as the Support
    // rail's "not reported".
    const { container, unmount } = render(() => (
      <PlanPanel plans={[]} todos={[]} loading={false} />
    ))
    expect(container.textContent).toContain('Ingen plan registrert for denne kjøringen.')
    unmount()
  })

  it('distinguishes loading from empty', () => {
    const { container, unmount } = render(() => (
      <PlanPanel plans={[]} todos={[]} loading={true} />
    ))
    expect(container.textContent).toContain('Laster')
    expect(container.textContent).not.toContain('Ingen plan registrert')
    unmount()
  })

  it('renders the plan content the timeline cannot show', () => {
    const { container, unmount } = render(() => (
      <PlanPanel
        plans={[{ id: 'p1', state: 'EXECUTING', summary: 'Reconcile the July invoices' }]}
        todos={[
          { id: 't1', state: 'COMPLETED', title: 'Fetch invoice list' },
          { id: 't2', state: 'PENDING', title: 'Match against ledger' },
        ]}
        loading={false}
      />
    ))
    const text = container.textContent ?? ''
    // The point of the panel: the plan's CONTENT, not just its transitions.
    expect(text).toContain('Reconcile the July invoices')
    expect(text).toContain('Fetch invoice list')
    expect(text).toContain('Match against ledger')
    // Provider enum names are rendered as-is, not relabelled.
    expect(text).toContain('EXECUTING')
    expect(text).toContain('PENDING')
    unmount()
  })

  it('names a missing summary instead of showing a blank row', () => {
    const { container, unmount } = render(() => (
      <PlanPanel plans={[{ id: 'p1', state: 'DRAFT' }]} todos={[]} loading={false} />
    ))
    expect(container.textContent).toContain('(ingen sammendrag)')
    unmount()
  })
})

describe('BrowserObservationShot', () => {
  it('loads the screenshot by REFERENCE through the gateway, never inline bytes', () => {
    const { container, unmount } = render(() => (
      <BrowserObservationShot artifactId="art_abc123" pageUrl="https://example.test/a" />
    ))
    const img = container.querySelector('img')
    expect(img?.getAttribute('src')).toBe('/api/v1/chat/browser-artifacts/art_abc123')
    // Lazy: a long browser run can produce many observations, and eagerly
    // fetching every screenshot would pull megabytes through the gateway for
    // steps the user never scrolls to.
    expect(img?.getAttribute('loading')).toBe('lazy')
    // The alt text names the page, so the image is described rather than decorative.
    expect(img?.getAttribute('alt')).toContain('example.test')
    unmount()
  })

  it('encodes the artifact id rather than interpolating it raw', () => {
    const { container, unmount } = render(() => (
      <BrowserObservationShot artifactId="a/b?c=1" />
    ))
    const src = container.querySelector('img')?.getAttribute('src') ?? ''
    expect(src).not.toContain('a/b?c=1')
    expect(src).toContain(encodeURIComponent('a/b?c=1'))
    unmount()
  })

  it('names a load failure instead of leaving a blank gap', () => {
    // A screenshot that existed and could not be loaded is a different state
    // from a step that never had one, and only the former is a problem.
    const { container, unmount } = render(() => (
      <BrowserObservationShot artifactId="art_gone" />
    ))
    const img = container.querySelector('img')
    expect(img).toBeTruthy()
    img!.dispatchEvent(new Event('error'))
    expect(container.textContent).toContain('Kunne ikke laste skjermbildet')
    expect(container.querySelector('img')).toBeNull()
    unmount()
  })
})
