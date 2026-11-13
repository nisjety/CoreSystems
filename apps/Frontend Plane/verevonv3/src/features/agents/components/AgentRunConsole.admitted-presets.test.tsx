// @vitest-environment jsdom

// W3-EP-6-S11-ui: the run console narrows its curated presets by the owner actions the org admits. The admitted set
// is read once, only while VITE_PRESET_AGENT_ACTIONS is the literal "true". A preset whose required owner actions
// are all in the set is offered and runs with exactly those actions; until the set loads, and on a failed read
// (null, fail closed), such a preset reads "Not available in this org" and sends nothing. With the switch off,
// nothing is read and every preset is offered, tool-free, as before.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { mockStreamChat, mockListAdmitted } = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockListAdmitted: vi.fn(),
}))

vi.mock('@/shared/api/agent-actions-client', () => ({ listAdmittedOwnerActions: mockListAdmitted }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, streamChat: mockStreamChat }
})
vi.mock('@/shared/api/authored-kinds-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/authored-kinds-client')>()
  return { ...actual, listAuthored: vi.fn(async () => ({ items: [], kindsEnabled: false })) }
})
vi.mock('@/shared/api/thread-goal-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/thread-goal-client')>()
  return { ...actual, getThreadGoal: vi.fn(async () => null) }
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

import AgentRunConsole from './AgentRunConsole'

Element.prototype.scrollIntoView = vi.fn()

const TRIAGE_LABEL = 'Triage & escalate urgent tickets'
const NOT_AVAILABLE = /Ikke tilgjengelig i denne organisasjonen|Not available in this org/

function renderConsole() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function presetButton(label: string): HTMLButtonElement {
  const text = screen.getAllByText(label)[0]!
  return text.closest('button') as HTMLButtonElement
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  mockStreamChat.mockReset().mockImplementation(() => new Promise<void>(() => {}))
  mockListAdmitted.mockReset()
})

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('curated presets with VITE_PRESET_AGENT_ACTIONS on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
  })

  it('offers a preset whose required owner action the org admits, and runs it with exactly that action', async () => {
    mockListAdmitted.mockResolvedValue({ mode: 'table', actionIds: ['tickets.create', 'inbox.assign'] })
    const { unmount } = renderConsole()
    await waitFor(() => expect(mockListAdmitted).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(presetButton(TRIAGE_LABEL).disabled).toBe(false))
    expect(presetButton(TRIAGE_LABEL).textContent).not.toMatch(NOT_AVAILABLE)

    fireEvent.click(presetButton(TRIAGE_LABEL))
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))

    await waitFor(() => expect(mockStreamChat).toHaveBeenCalledTimes(1))
    const [request] = mockStreamChat.mock.calls[0] as [{ actions?: Array<{ id: string; kind: string }>; planMode?: boolean }]
    expect(request.actions?.map((action) => action.id)).toEqual(['tickets.create'])
    expect(request.planMode).toBe(true)
    unmount()
  })

  it('keeps the preset unavailable when the org does not admit its required action', async () => {
    mockListAdmitted.mockResolvedValue({ mode: 'table', actionIds: ['inbox.assign'] })
    const { unmount } = renderConsole()
    await waitFor(() => expect(mockListAdmitted).toHaveBeenCalled())
    await settle()
    expect(presetButton(TRIAGE_LABEL).disabled).toBe(true)
    expect(presetButton(TRIAGE_LABEL).textContent).toMatch(NOT_AVAILABLE)
    // A preset that needs no owner action stays offered.
    expect(presetButton('Draft reply with sources').disabled).toBe(false)
    unmount()
  })

  it('fails closed when the admitted set cannot be read', async () => {
    mockListAdmitted.mockResolvedValue(null)
    const { unmount } = renderConsole()
    await waitFor(() => expect(mockListAdmitted).toHaveBeenCalled())
    await settle()
    expect(presetButton(TRIAGE_LABEL).disabled).toBe(true)
    expect(presetButton(TRIAGE_LABEL).textContent).toMatch(NOT_AVAILABLE)
    unmount()
  })

  it('keeps the preset unavailable while the admitted set is still loading', async () => {
    mockListAdmitted.mockImplementation(() => new Promise(() => {}))
    const { unmount } = renderConsole()
    await waitFor(() => expect(mockListAdmitted).toHaveBeenCalled())
    await settle()
    expect(presetButton(TRIAGE_LABEL).disabled).toBe(true)
    unmount()
  })
})

describe('curated presets with VITE_PRESET_AGENT_ACTIONS off', () => {
  it('reads nothing and offers every preset tool-free, as before', async () => {
    const { unmount } = renderConsole()
    await settle()
    expect(mockListAdmitted).not.toHaveBeenCalled()
    expect(presetButton(TRIAGE_LABEL).disabled).toBe(false)
    expect(presetButton(TRIAGE_LABEL).textContent).not.toMatch(NOT_AVAILABLE)

    fireEvent.click(presetButton(TRIAGE_LABEL))
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /kjør oppgave/i }))
    await waitFor(() => expect(mockStreamChat).toHaveBeenCalledTimes(1))
    const [request] = mockStreamChat.mock.calls[0] as [{ actions?: unknown[] }]
    expect(request.actions ?? []).toEqual([])
    unmount()
  })
})
