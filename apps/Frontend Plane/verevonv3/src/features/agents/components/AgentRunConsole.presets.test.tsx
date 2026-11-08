// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AuthoredBehaviour, AuthoredList } from '@/shared/api/authored-kinds-client'

// EP-11 S6: the person's authored presets list under the curated ones. Applying one fills the
// console's visible controls only; nothing runs until Run is pressed. Only the network-facing
// modules are mocked.

const { mockStreamChat, mockListAuthored } = vi.hoisted(() => ({
  mockStreamChat: vi.fn(),
  mockListAuthored: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, streamChat: mockStreamChat }
})
vi.mock('@/shared/api/authored-kinds-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/authored-kinds-client')>()
  return { ...actual, listAuthored: mockListAuthored }
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

const PRESET: AuthoredBehaviour = {
  id: 'preset_1',
  kind: 'preset',
  name: 'Konkurrentpriser',
  description: 'Prissammenligning med nettsøk',
  content: 'Sammenlign prisene til våre tre største konkurrenter.',
  settings: { browseWeb: true, planMode: true, modeId: 'not-a-console-mode' },
}

function renderConsole() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: AgentRunConsole }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  vi.clearAllMocks()
})

describe('authored presets in the run console', () => {
  it('lists authored presets under the curated ones', async () => {
    mockListAuthored.mockResolvedValue({ items: [PRESET], kindsEnabled: true } satisfies AuthoredList)
    const { container, unmount } = renderConsole()
    const authored = await screen.findByText('Konkurrentpriser')
    const curated = screen.getAllByText('Draft reply with sources')[0]!
    // The curated picker comes first in the document.
    expect(curated.compareDocumentPosition(authored) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(container.querySelector('[data-authored-preset="preset_1"]')).not.toBeNull()
    expect(mockListAuthored).toHaveBeenCalledWith('preset')
    unmount()
  })

  it('applying a preset fills the controls and runs nothing', async () => {
    mockListAuthored.mockResolvedValue({ items: [PRESET], kindsEnabled: true } satisfies AuthoredList)
    const { container, unmount } = renderConsole()
    fireEvent.click(await screen.findByText('Konkurrentpriser'))

    const goal = container.querySelector('#run-goal') as HTMLTextAreaElement
    await waitFor(() => expect(goal.value).toBe(PRESET.content))
    expect(container.textContent).toContain('Nett på')
    // A mode the console does not offer is ignored, not sent.
    expect(container.textContent).not.toContain('not-a-console-mode')
    expect(mockStreamChat).not.toHaveBeenCalled()
    unmount()
  })

  it('shows nothing extra while authored kinds are off', async () => {
    mockListAuthored.mockResolvedValue({ items: [], kindsEnabled: false } satisfies AuthoredList)
    const { unmount } = renderConsole()
    await waitFor(() => expect(mockListAuthored).toHaveBeenCalled())
    expect(screen.queryByText('Dine forhåndsinnstillinger')).toBeNull()
    expect(screen.getAllByText('Draft reply with sources').length).toBeGreaterThan(0)
    unmount()
  })
})
